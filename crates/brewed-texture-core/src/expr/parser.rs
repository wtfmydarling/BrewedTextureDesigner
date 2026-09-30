//! Precedence-climbing parser producing a small AST.
//!
//! Grammar (lowest to highest precedence):
//! ```text
//! expr    := or ( '?' expr ':' expr )?
//! or      := and ( '||' and )*
//! and     := cmp ( '&&' cmp )*
//! cmp     := add ( ('<' | '<=' | '>' | '>=' | '==' | '!=') add )*
//! add     := mul ( ('+' | '-') mul )*
//! mul     := unary ( ('*' | '/' | '%') unary | <implicit product> )*
//! unary   := ('-' | '+' | '!') unary | power
//! power   := primary ( '^' unary )?          // right associative, -2^2 = -4
//! primary := NUMBER | IDENT | IDENT '(' args ')' | '(' expr ')'
//! ```
//! Implicit multiplication is accepted after a number or a closing parenthesis:
//! `2x`, `3(a + b)`, `(a + 1)(a - 1)`; but not of a name given a value, which begins the next
//! statement of a script (see [`super::script`]):
//! ```text
//! script    := ( ';' | statement )* EOF
//! statement := IDENT '=' expr ( ',' expr )*
//! ```

use super::builtins::{self, Func};
use super::lexer::{Token, TokenKind};
use super::{ExprError, Span};

const MAX_DEPTH: usize = 96;

/// The deepest a value can be made of values: `a + b + c + …` is as deep as it is long, and
/// what reads, computes, writes as a shader or drops a value walks down it, on the stack of
/// the thread that does it (1 MB for the main thread of Windows); the shader compiler also
/// refuses shaders nested deeper than 256.
pub const MAX_TREE: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Num(f64),
    Var(usize),
    Unary(UnOp, Box<Node>),
    Binary(BinOp, Box<Node>, Box<Node>),
    Call(Func, Vec<Node>),
    Cond(Box<Node>, Box<Node>, Box<Node>),
}

pub struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    depth: usize,
    /// How deep the value read last is (see [`MAX_TREE`]).
    tree: usize,
    pub vars: Vec<String>,
    /// Where each of [`Parser::vars`] is first read: the statement of a script (0 in an
    /// expression) and the place.
    pub first_read: Vec<(usize, Span)>,
    /// The statement of a script being read.
    statement: usize,
}

/// A `name = value` of a script (see [`Parser::parse_script`]).
pub struct Assignment {
    pub name: String,
    pub name_span: Span,
    pub values: Vec<Node>,
    /// From the name to the end of the value.
    pub span: Span,
}

type PResult = Result<Node, ExprError>;

impl<'a> Parser<'a> {
    pub fn new(tokens: &'a [Token]) -> Self {
        Self { tokens, pos: 0, depth: 0, tree: 0, vars: Vec::new(), first_read: Vec::new(), statement: 0 }
    }

    /// The token after the next one.
    fn peek2(&self) -> &Token {
        &self.tokens[(self.pos + 1).min(self.tokens.len() - 1)]
    }

    /// Whether a statement of a script begins here: a name given a value.
    fn at_statement_start(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Ident(_)) && matches!(self.peek2().kind, TokenKind::Assign)
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn prev(&self) -> Option<&Token> {
        self.pos.checked_sub(1).map(|p| &self.tokens[p])
    }

    fn advance(&mut self) -> &Token {
        let t = &self.tokens[self.pos.min(self.tokens.len() - 1)];
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if &self.peek().kind == kind {
            self.advance();
            true
        } else {
            false
        }
    }

    fn error_at(&self, tok: &Token, message: impl Into<String>) -> ExprError {
        ExprError { message: message.into(), span: tok.span }
    }

    fn unexpected(&self) -> ExprError {
        let tok = self.peek();
        match &tok.kind {
            TokenKind::Error(msg) => self.error_at(tok, msg.clone()),
            TokenKind::Eof => self.error_at(tok, "Expression is incomplete: expected a value"),
            TokenKind::Semicolon => self.error_at(tok, "Unexpected ';': expected a value"),
            TokenKind::RParen => self.error_at(tok, "Unexpected ')': expected a value"),
            other => self.error_at(tok, format!("Unexpected {}: expected a value", other.describe())),
        }
    }

    fn enter(&mut self) -> Result<(), ExprError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            let tok = self.peek().clone();
            return Err(self.error_at(&tok, "Expression is nested too deeply"));
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth -= 1;
    }

    /// A value made of values the deepest of which is `below` deep: how deep it is, refused
    /// past [`MAX_TREE`].
    fn made(&mut self, below: usize) -> Result<usize, ExprError> {
        self.tree = below + 1;
        if self.tree > MAX_TREE {
            let tok = self.prev().unwrap_or(self.peek()).clone();
            return Err(self.error_at(&tok, format!("Expression is too long: at most {MAX_TREE} operations in a row")));
        }
        Ok(self.tree)
    }

    /// Parse a whole expression and make sure nothing is left over.
    /// One value, or a list of up to four separated by commas (the red, green, blue and alpha
    /// of Pixel Math): `u, v, 0.5`.
    pub fn parse_list(&mut self) -> Result<Vec<Node>, ExprError> {
        if matches!(self.peek().kind, TokenKind::Eof) {
            let tok = self.peek().clone();
            return Err(self.error_at(&tok, "Expression is empty"));
        }
        let list = self.values()?;
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Eof => Ok(list),
            TokenKind::Assign => Err(self.error_at(&tok, "Unexpected '=' (use '==' to compare values)")),
            TokenKind::Semicolon => Err(self.error_at(&tok, "Unexpected ';': an expression is one value, or a list of them")),
            _ => Err(self.leftover(&tok)),
        }
    }

    /// Parse a script: its statements, in order (see [`super::script`]).
    pub fn parse_script(&mut self) -> Result<Vec<Assignment>, ExprError> {
        let mut statements = Vec::new();
        loop {
            while self.eat(&TokenKind::Semicolon) {}
            if matches!(self.peek().kind, TokenKind::Eof) {
                return Ok(statements);
            }
            let tok = self.peek().clone();
            let TokenKind::Ident(name) = &tok.kind else {
                return Err(self.error_at(&tok, format!("Expected a name given a value (name = value), found {}", tok.kind.describe())));
            };
            if !matches!(self.peek2().kind, TokenKind::Assign) {
                return Err(self.error_at(&tok, format!("Expected '=' after '{name}': each statement is name = value")));
            }
            self.advance();
            self.advance();
            self.statement = statements.len();
            if matches!(self.peek().kind, TokenKind::Eof | TokenKind::Semicolon) {
                let t = self.peek().clone();
                return Err(self.error_at(&t, format!("'{name} =' needs a value")));
            }
            let values = self.values()?;
            let end = self.prev().map_or(tok.span.end, |t| t.span.end);
            let next = self.peek().clone();
            match &next.kind {
                TokenKind::Eof | TokenKind::Semicolon => {}
                _ if self.at_statement_start() => {}
                _ => return Err(self.leftover(&next)),
            }
            statements.push(Assignment { name: name.clone(), name_span: tok.span, values, span: Span::new(tok.span.start, end) });
        }
    }

    /// What is wrong with `tok`, found after a whole value.
    fn leftover(&self, tok: &Token) -> ExprError {
        match &tok.kind {
            TokenKind::RParen => self.error_at(tok, "Unmatched ')'"),
            TokenKind::Error(msg) => self.error_at(tok, msg.clone()),
            TokenKind::Number(_) | TokenKind::Ident(_) | TokenKind::LParen => {
                self.error_at(tok, format!("Unexpected {}: missing operator before it?", tok.kind.describe()))
            }
            other => self.error_at(tok, format!("Unexpected {}", other.describe())),
        }
    }

    /// One value, or a list of up to four separated by commas.
    fn values(&mut self) -> Result<Vec<Node>, ExprError> {
        let mut list = vec![self.expr()?];
        while self.eat(&TokenKind::Comma) {
            if matches!(self.peek().kind, TokenKind::Eof | TokenKind::Comma | TokenKind::Semicolon) || self.at_statement_start() {
                let tok = self.peek().clone();
                return Err(self.error_at(&tok, "Missing value after ','"));
            }
            let start = self.peek().span.start;
            list.push(self.expr()?);
            if list.len() > 4 {
                let end = self.prev().map_or(start, |t| t.span.end);
                return Err(ExprError { message: "At most four values: red, green, blue and alpha".into(), span: Span::new(start, end) });
            }
        }
        Ok(list)
    }

    fn expr(&mut self) -> PResult {
        self.enter()?;
        let cond = self.or()?;
        let result = if self.eat(&TokenKind::Question) {
            let deep = self.tree;
            let a = self.expr()?;
            let deep = deep.max(self.tree);
            if !self.eat(&TokenKind::Colon) {
                let tok = self.peek().clone();
                self.leave();
                return Err(self.error_at(&tok, "Expected ':' in conditional 'cond ? a : b'"));
            }
            let b = self.expr()?;
            self.made(deep.max(self.tree))?;
            Node::Cond(Box::new(cond), Box::new(a), Box::new(b))
        } else {
            cond
        };
        self.leave();
        Ok(result)
    }

    fn or(&mut self) -> PResult {
        let mut lhs = self.and()?;
        while self.eat(&TokenKind::OrOr) {
            let deep = self.tree;
            let rhs = self.and()?;
            self.made(deep.max(self.tree))?;
            lhs = Node::Binary(BinOp::Or, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn and(&mut self) -> PResult {
        let mut lhs = self.cmp()?;
        while self.eat(&TokenKind::AndAnd) {
            let deep = self.tree;
            let rhs = self.cmp()?;
            self.made(deep.max(self.tree))?;
            lhs = Node::Binary(BinOp::And, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn cmp(&mut self) -> PResult {
        let mut lhs = self.add()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Less => BinOp::Lt,
                TokenKind::LessEq => BinOp::Le,
                TokenKind::Greater => BinOp::Gt,
                TokenKind::GreaterEq => BinOp::Ge,
                TokenKind::EqEq => BinOp::Eq,
                TokenKind::NotEq => BinOp::Ne,
                _ => break,
            };
            self.advance();
            let deep = self.tree;
            let rhs = self.add()?;
            self.made(deep.max(self.tree))?;
            lhs = Node::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn add(&mut self) -> PResult {
        let mut lhs = self.mul()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let deep = self.tree;
            let rhs = self.mul()?;
            self.made(deep.max(self.tree))?;
            lhs = Node::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn implicit_product_follows(&self) -> bool {
        let prev_ok = matches!(self.prev().map(|t| &t.kind), Some(TokenKind::Number(_) | TokenKind::RParen));
        let next_ok = matches!(self.peek().kind, TokenKind::Ident(_) | TokenKind::LParen);
        prev_ok && next_ok && !self.at_statement_start()
    }

    fn mul(&mut self) -> PResult {
        let mut lhs = self.unary()?;
        loop {
            let op = match self.peek().kind {
                TokenKind::Star => Some(BinOp::Mul),
                TokenKind::Slash => Some(BinOp::Div),
                TokenKind::Percent => Some(BinOp::Mod),
                _ => None,
            };
            if let Some(op) = op {
                self.advance();
                let deep = self.tree;
                let rhs = self.unary()?;
                self.made(deep.max(self.tree))?;
                lhs = Node::Binary(op, Box::new(lhs), Box::new(rhs));
            } else if self.implicit_product_follows() {
                let deep = self.tree;
                let rhs = self.unary()?;
                self.made(deep.max(self.tree))?;
                lhs = Node::Binary(BinOp::Mul, Box::new(lhs), Box::new(rhs));
            } else {
                break;
            }
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> PResult {
        self.enter()?;
        let result = match self.peek().kind {
            TokenKind::Minus => {
                self.advance();
                let inner = self.unary()?;
                self.made(self.tree)?;
                Node::Unary(UnOp::Neg, Box::new(inner))
            }
            TokenKind::Plus => {
                self.advance();
                self.unary()?
            }
            TokenKind::Bang => {
                self.advance();
                let inner = self.unary()?;
                self.made(self.tree)?;
                Node::Unary(UnOp::Not, Box::new(inner))
            }
            _ => self.power()?,
        };
        self.leave();
        Ok(result)
    }

    fn power(&mut self) -> PResult {
        let base = self.primary()?;
        if self.eat(&TokenKind::Caret) {
            let deep = self.tree;
            let exponent = self.unary()?;
            self.made(deep.max(self.tree))?;
            Ok(Node::Binary(BinOp::Pow, Box::new(base), Box::new(exponent)))
        } else {
            Ok(base)
        }
    }

    fn primary(&mut self) -> PResult {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Number(v) => {
                self.advance();
                self.tree = 1;
                Ok(Node::Num(*v))
            }
            TokenKind::LParen => {
                self.advance();
                if matches!(self.peek().kind, TokenKind::RParen) {
                    let t = self.peek().clone();
                    return Err(self.error_at(&t, "Empty parentheses: expected a value"));
                }
                let inner = self.expr()?;
                if !self.eat(&TokenKind::RParen) {
                    let next = self.peek().clone();
                    let message = if matches!(next.kind, TokenKind::Eof) {
                        "Missing ')' to close this '('".to_string()
                    } else {
                        format!("Expected ')' to close '(' but found {}", next.kind.describe())
                    };
                    let span = if matches!(next.kind, TokenKind::Eof) { tok.span } else { next.span };
                    return Err(ExprError { message, span });
                }
                Ok(inner)
            }
            TokenKind::Ident(name) if self.at_statement_start() => {
                Err(self.error_at(&tok, format!("Expected a value, found '{name} =', which begins the next statement")))
            }
            TokenKind::Ident(name) => {
                self.advance();
                if matches!(self.peek().kind, TokenKind::LParen) {
                    return self.call(name, &tok);
                }
                self.tree = 1;
                if let Some(value) = builtins::find_constant(name) {
                    Ok(Node::Num(value))
                } else if builtins::find_function(name).is_some() {
                    Err(self.error_at(&tok, format!("'{name}' is a function: write {name}(...) with its arguments")))
                } else {
                    let index = match self.vars.iter().position(|v| v == name) {
                        Some(i) => i,
                        None => {
                            self.vars.push(name.clone());
                            self.first_read.push((self.statement, tok.span));
                            self.vars.len() - 1
                        }
                    };
                    Ok(Node::Var(index))
                }
            }
            _ => Err(self.unexpected()),
        }
    }

    fn call(&mut self, name: &str, name_tok: &Token) -> PResult {
        let info = match builtins::find_function(name) {
            Some(info) => info,
            None => {
                let message = if builtins::find_constant(name).is_some() {
                    format!("'{name}' is a constant, not a function")
                } else if let Some(s) = builtins::suggest_function(name) {
                    format!("Unknown function '{name}'. Did you mean '{s}'?")
                } else {
                    format!("Unknown function '{name}' (for multiplication write '{name}*(...)')")
                };
                return Err(self.error_at(name_tok, message));
            }
        };
        let open = self.advance().clone(); // '('
        let mut args = Vec::new();
        let mut deep = 0;
        if !matches!(self.peek().kind, TokenKind::RParen) {
            loop {
                if matches!(self.peek().kind, TokenKind::Comma | TokenKind::RParen) {
                    let t = self.peek().clone();
                    return Err(self.error_at(&t, format!("Missing argument in call to '{name}'")));
                }
                args.push(self.expr()?);
                deep = self.tree.max(deep);
                if self.eat(&TokenKind::Comma) {
                    continue;
                }
                break;
            }
        }
        if !self.eat(&TokenKind::RParen) {
            let next = self.peek().clone();
            if matches!(next.kind, TokenKind::Eof) {
                return Err(ExprError {
                    message: format!("Missing ')' to close the call to '{name}'"),
                    span: Span::new(name_tok.span.start, open.span.end),
                });
            }
            return Err(self.error_at(
                &next,
                format!("Expected ',' or ')' in call to '{name}' but found {}", next.kind.describe()),
            ));
        }
        let n = args.len();
        if n < info.min_args || n > info.max_args {
            let expected = if info.min_args == info.max_args {
                format!("{} argument{}", info.min_args, if info.min_args == 1 { "" } else { "s" })
            } else if info.max_args == builtins::VARIADIC {
                format!("at least {} arguments", info.min_args)
            } else {
                format!("{} to {} arguments", info.min_args, info.max_args)
            };
            let span = Span::new(name_tok.span.start, self.prev().map_or(name_tok.span.end, |t| t.span.end));
            return Err(ExprError {
                message: format!("'{name}' expects {expected}, got {n} — usage: {}", info.signature),
                span,
            });
        }
        // What `sample` reads is an image input: named, not computed.
        if info.func == builtins::Func::Sample && !matches!(args.first(), Some(Node::Var(_))) {
            let span = Span::new(name_tok.span.start, self.prev().map_or(name_tok.span.end, |t| t.span.end));
            return Err(ExprError {
                message: format!("The first argument of '{name}' is the name of an image input — usage: {}", info.signature),
                span,
            });
        }
        self.made(deep)?;
        Ok(Node::Call(info.func, args))
    }
}
