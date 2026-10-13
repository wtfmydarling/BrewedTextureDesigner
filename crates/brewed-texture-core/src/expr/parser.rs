//! Precedence-climbing parser producing a small AST, and how many components each value it
//! reads has: a number is one, a vector two to four (`vec3(r, g, b)`, `c.rgb`).
//!
//! Grammar (lowest to highest precedence):
//! ```text
//! program   := ( ';' | statement )* list?        // the list: the value of an expression
//! statement := IDENT '=' list
//! list      := expr ( ',' expr )*
//! expr      := or ( '?' expr ':' expr )?
//! or        := and ( '||' and )*
//! and       := cmp ( '&&' cmp )*
//! cmp       := add ( ('<' | '<=' | '>' | '>=' | '==' | '!=') add )*
//! add       := mul ( ('+' | '-') mul )*
//! mul       := unary ( ('*' | '/' | '%') unary | <implicit product> )*
//! unary     := ('-' | '+' | '!') unary | power
//! power     := postfix ( '^' unary )?          // right associative, -2^2 = -4
//! postfix   := primary ( '.' CHANNELS )*       // v.x, c.rgb, img.a
//! primary   := NUMBER | IDENT | IDENT '(' args ')' | '(' expr ')'
//! ```
//! Implicit multiplication is accepted after a number or a closing parenthesis:
//! `2x`, `3(a + b)`, `(a + 1)(a - 1)`; but not of a name given a value, which begins the next
//! statement (see [`super::script`]), nor of what begins a line: the value of an expression
//! after its statements does, as it does in `n = normal.rgb\nn.z`.
//!
//! Arithmetic and the functions of numbers work on each component of a vector, a number with
//! a vector as with a vector of it (`v * 2`, `min(c, 0.5)`); a vector with a vector of another
//! length is an error, and so is a vector compared, or where a number is asked for.

use std::collections::HashMap;

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

impl BinOp {
    /// Whether it compares numbers or joins truths, giving a number that is 1 or 0.
    pub fn is_logical(self) -> bool {
        !matches!(self, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::Pow)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Num(f64),
    Var(usize),
    Unary(UnOp, Box<Node>),
    Binary(BinOp, Box<Node>, Box<Node>),
    Call(Func, Vec<Node>),
    Cond(Box<Node>, Box<Node>, Box<Node>),
    /// Channels of a value, by their places, in the order named (`v.x`, `c.bgr`): of a vector
    /// its components; of a name read from outside (or what `sample` reads) the channels of
    /// its image, red, green, blue and alpha (a value is the same in all three, and opaque).
    Swizzle(Box<Node>, Vec<u8>),
}

/// A `name = value` of a program (see [`Parser::parse_program`]).
pub struct Assignment {
    pub name: String,
    pub name_span: Span,
    /// Its value, or the values of a list, each with its components.
    pub values: Vec<(Node, u8)>,
    /// From the name to the end of the value.
    pub span: Span,
}

pub struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    depth: usize,
    /// How deep the value read last is (see [`MAX_TREE`]).
    tree: usize,
    /// How many components the value read last has.
    dim: u8,
    pub vars: Vec<String>,
    /// Where each of [`Parser::vars`] is first read: the statement of a program (0 in an
    /// expression) and the place.
    pub first_read: Vec<(usize, Span)>,
    /// The statement of a program being read.
    statement: usize,
    /// The names statements before this one set, with the components of each.
    set: HashMap<String, u8>,
    /// Names there from the start, which are read as numbers (the outputs of a script).
    outputs: &'a [&'a str],
}

type PResult = Result<Node, ExprError>;

/// The number of components `dim` makes, said in an error.
pub fn describe_dim(dim: u8) -> &'static str {
    match dim {
        1 => "a number",
        2 => "a vec2",
        3 => "a vec3",
        _ => "a vec4",
    }
}

/// The places of the channels `letters` names: x, y, z, w or r, g, b, a, one to four.
fn channels(letters: &str) -> Option<Vec<u8>> {
    if letters.is_empty() || letters.len() > 4 {
        return None;
    }
    let of = |set: &str| letters.chars().map(|c| set.find(c).map(|i| i as u8)).collect::<Option<Vec<u8>>>();
    of("xyzw").or_else(|| of("rgba"))
}

impl<'a> Parser<'a> {
    pub fn new(tokens: &'a [Token]) -> Self {
        Self { tokens, pos: 0, depth: 0, tree: 0, dim: 1, vars: Vec::new(), first_read: Vec::new(), statement: 0, set: HashMap::new(), outputs: &[] }
    }

    /// The token after the next one.
    fn peek2(&self) -> &Token {
        &self.tokens[(self.pos + 1).min(self.tokens.len() - 1)]
    }

    /// Whether a statement of a program begins here: a name given a value.
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

    /// From `from` to the token read last.
    fn span_from(&self, from: &Token) -> Span {
        Span::new(from.span.start, self.prev().map_or(from.span.end, |t| t.span.end.max(from.span.end)))
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

    /// Whether variable `i` is a name the program sets (or an output, there from the start).
    fn is_set(&self, i: usize) -> bool {
        let name = &self.vars[i];
        self.set.contains_key(name) || self.outputs.contains(&name.as_str())
    }

    /// Components of what arithmetic `op` makes of values of `a` and `b` components: as many
    /// as either, a number going with a vector as a vector of it.
    fn combine(&self, op: &Token, a: u8, b: u8) -> Result<u8, ExprError> {
        match (a, b) {
            _ if a == b => Ok(a),
            (1, _) => Ok(b),
            (_, 1) => Ok(a),
            _ => Err(self.error_at(
                op,
                format!("{} and {} do not go together: take as many components of one as the other has (.xy, .xyz)", describe_dim(a), describe_dim(b)),
            )),
        }
    }

    /// `op` takes numbers: of `a` and `b` components.
    fn numbers(&self, op: &Token, a: u8, b: u8) -> Result<u8, ExprError> {
        match a.max(b) {
            1 => Ok(1),
            d => Err(self.error_at(op, format!("{} takes numbers, not {} (take a component: .x)", op.kind.describe(), describe_dim(d)))),
        }
    }

    /// Parse a program: its statements, in order, and its value (when `value`): a list of up to
    /// four values, or of vectors of as many components in all (the red, green, blue and alpha
    /// of Pixel Math). `outputs` are names there from the start, read as numbers.
    #[allow(clippy::type_complexity)]
    pub fn parse_program(&mut self, value: bool, outputs: &'a [&'a str]) -> Result<(Vec<Assignment>, Vec<(Node, u8)>), ExprError> {
        self.outputs = outputs;
        let mut statements = Vec::new();
        loop {
            while self.eat(&TokenKind::Semicolon) {}
            if matches!(self.peek().kind, TokenKind::Eof) {
                break;
            }
            let tok = self.peek().clone();
            if !self.at_statement_start() {
                if !value {
                    return Err(match &tok.kind {
                        TokenKind::Ident(name) => self.error_at(&tok, format!("Expected '=' after '{name}': each statement is name = value")),
                        _ => self.error_at(&tok, format!("Expected a name given a value (name = value), found {}", tok.kind.describe())),
                    });
                }
                self.statement = statements.len();
                let list = self.values()?;
                while self.eat(&TokenKind::Semicolon) {}
                let next = self.peek().clone();
                return match &next.kind {
                    TokenKind::Eof => Ok((statements, list)),
                    TokenKind::Assign => Err(self.error_at(&next, "Unexpected '=' (use '==' to compare values)")),
                    _ if self.at_statement_start() => {
                        Err(self.error_at(&next, "The value of the expression comes last, after the statements that give names values"))
                    }
                    _ => Err(self.leftover(&next)),
                };
            }
            let TokenKind::Ident(name) = &tok.kind else { unreachable!("a statement begins with a name") };
            self.advance();
            self.advance();
            self.statement = statements.len();
            if matches!(self.peek().kind, TokenKind::Eof | TokenKind::Semicolon) || self.at_statement_start() {
                let t = self.peek().clone();
                return Err(self.error_at(&t, format!("'{name} =' needs a value")));
            }
            let values = self.values()?;
            let span = self.span_from(&tok);
            let next = self.peek().clone();
            match &next.kind {
                TokenKind::Eof | TokenKind::Semicolon => {}
                _ if self.at_statement_start() => {}
                _ if value => {}
                _ => return Err(self.leftover(&next)),
            }
            // Read from here on: an output as a number; a name of the program's own as what it
            // is, which it stays.
            let dim = if values.len() == 1 && !outputs.contains(&name.as_str()) { values[0].1 } else { 1 };
            if !outputs.contains(&name.as_str())
                && let Some(&before) = self.set.get(name)
                && before != dim
            {
                return Err(ExprError { message: format!("'{name}' is {}: it cannot become {}", describe_dim(before), describe_dim(dim)), span: tok.span });
            }
            self.set.insert(name.clone(), dim);
            statements.push(Assignment { name: name.clone(), name_span: tok.span, values, span });
        }
        if !value {
            return Ok((statements, Vec::new()));
        }
        let tok = self.peek().clone();
        Err(if statements.is_empty() {
            self.error_at(&tok, "Expression is empty")
        } else {
            let first = &statements[0].name;
            self.error_at(
                &tok,
                format!("Expected the value of the expression after the statements ('{first} = …' gives {first} a value; to compare, write ==)"),
            )
        })
    }

    /// Parse a whole expression and make sure nothing is left over (a program with no
    /// statements).
    pub fn parse_list(&mut self) -> Result<Vec<(Node, u8)>, ExprError> {
        let (statements, list) = self.parse_program(true, &[])?;
        match statements.first() {
            Some(s) => Err(ExprError { message: "Expected one expression, not statements".into(), span: s.name_span }),
            None => Ok(list),
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

    /// One value, or a list of values separated by commas: four components at most in all.
    fn values(&mut self) -> Result<Vec<(Node, u8)>, ExprError> {
        let first = self.peek().clone();
        let mut list = vec![(self.expr()?, self.dim)];
        let mut components = self.dim;
        while self.eat(&TokenKind::Comma) {
            if matches!(self.peek().kind, TokenKind::Eof | TokenKind::Comma | TokenKind::Semicolon) || self.at_statement_start() {
                let tok = self.peek().clone();
                return Err(self.error_at(&tok, "Missing value after ','"));
            }
            let start = self.peek().span.start;
            list.push((self.expr()?, self.dim));
            components += self.dim;
            if list.len() > 4 || components > 4 {
                let end = self.prev().map_or(start, |t| t.span.end);
                return Err(ExprError { message: "At most four values: red, green, blue and alpha".into(), span: Span::new(start, end) });
            }
        }
        if list.len() == 1 && components > 4 {
            return Err(ExprError { message: "At most four values: red, green, blue and alpha".into(), span: self.span_from(&first) });
        }
        Ok(list)
    }

    fn expr(&mut self) -> PResult {
        self.enter()?;
        let start = self.peek().clone();
        let cond = self.or()?;
        let result = if self.peek().kind == TokenKind::Question {
            let question = self.advance().clone();
            if self.dim != 1 {
                return Err(self.error_at(&start, format!("The condition before '?' is a number, not {}", describe_dim(self.dim))));
            }
            let deep = self.tree;
            let a = self.expr()?;
            let (deep, da) = (deep.max(self.tree), self.dim);
            if !self.eat(&TokenKind::Colon) {
                let tok = self.peek().clone();
                self.leave();
                return Err(self.error_at(&tok, "Expected ':' in conditional 'cond ? a : b'"));
            }
            let b = self.expr()?;
            self.dim = self.combine(&question, da, self.dim)?;
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
        while self.peek().kind == TokenKind::OrOr {
            let op = self.advance().clone();
            let (deep, da) = (self.tree, self.dim);
            let rhs = self.and()?;
            self.dim = self.numbers(&op, da, self.dim)?;
            self.made(deep.max(self.tree))?;
            lhs = Node::Binary(BinOp::Or, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn and(&mut self) -> PResult {
        let mut lhs = self.cmp()?;
        while self.peek().kind == TokenKind::AndAnd {
            let op = self.advance().clone();
            let (deep, da) = (self.tree, self.dim);
            let rhs = self.cmp()?;
            self.dim = self.numbers(&op, da, self.dim)?;
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
            let tok = self.advance().clone();
            let (deep, da) = (self.tree, self.dim);
            let rhs = self.add()?;
            self.dim = self.numbers(&tok, da, self.dim)?;
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
            let tok = self.advance().clone();
            let (deep, da) = (self.tree, self.dim);
            let rhs = self.mul()?;
            self.dim = self.combine(&tok, da, self.dim)?;
            self.made(deep.max(self.tree))?;
            lhs = Node::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn implicit_product_follows(&self) -> bool {
        let prev_ok = matches!(self.prev().map(|t| &t.kind), Some(TokenKind::Number(_) | TokenKind::RParen));
        let next_ok = matches!(self.peek().kind, TokenKind::Ident(_) | TokenKind::LParen) && !self.peek().line;
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
                let tok = self.advance().clone();
                let (deep, da) = (self.tree, self.dim);
                let rhs = self.unary()?;
                self.dim = self.combine(&tok, da, self.dim)?;
                self.made(deep.max(self.tree))?;
                lhs = Node::Binary(op, Box::new(lhs), Box::new(rhs));
            } else if self.implicit_product_follows() {
                let tok = self.peek().clone();
                let (deep, da) = (self.tree, self.dim);
                let rhs = self.unary()?;
                self.dim = self.combine(&tok, da, self.dim)?;
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
                let tok = self.advance().clone();
                let inner = self.unary()?;
                self.dim = self.numbers(&tok, self.dim, 1)?;
                self.made(self.tree)?;
                Node::Unary(UnOp::Not, Box::new(inner))
            }
            _ => self.power()?,
        };
        self.leave();
        Ok(result)
    }

    fn power(&mut self) -> PResult {
        let base = self.postfix()?;
        if self.peek().kind == TokenKind::Caret {
            let tok = self.advance().clone();
            let (deep, da) = (self.tree, self.dim);
            let exponent = self.unary()?;
            self.dim = self.combine(&tok, da, self.dim)?;
            self.made(deep.max(self.tree))?;
            Ok(Node::Binary(BinOp::Pow, Box::new(base), Box::new(exponent)))
        } else {
            Ok(base)
        }
    }

    /// A value and the channels taken of it: `.x`, `.rgb`, again and again.
    fn postfix(&mut self) -> PResult {
        let mut node = self.primary()?;
        // Four channels to take of a name read from outside, and of what `sample` reads: an
        // image's.
        let mut image = matches!(&node, Node::Var(i) if !self.is_set(*i)) || matches!(&node, Node::Call(Func::Sample, _));
        while self.peek().kind == TokenKind::Dot {
            self.advance();
            let tok = self.peek().clone();
            let TokenKind::Ident(letters) = &tok.kind else {
                return Err(self.error_at(&tok, "Expected channels after '.': x, y, z, w or r, g, b, a"));
            };
            let Some(places) = channels(letters) else {
                return Err(self.error_at(&tok, format!("'.{letters}': channels are one to four of x, y, z, w or of r, g, b, a")));
            };
            let has = if image { 4 } else { self.dim };
            if let Some(&beyond) = places.iter().find(|p| **p >= has) {
                let letter = letters.chars().nth(places.iter().position(|p| *p == beyond).unwrap_or(0)).unwrap_or('?');
                return Err(self.error_at(&tok, format!("{} has no '{letter}': it has {has} component{}", describe_dim(has), if has == 1 { "" } else { "s" })));
            }
            self.advance();
            self.dim = places.len() as u8;
            node = Node::Swizzle(Box::new(node), places);
            image = false;
            self.made(self.tree)?;
        }
        Ok(node)
    }

    fn primary(&mut self) -> PResult {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Number(v) => {
                self.advance();
                self.tree = 1;
                self.dim = 1;
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
                self.dim = 1;
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
                    if !self.outputs.contains(&name.as_str()) {
                        self.dim = self.set.get(name).copied().unwrap_or(1);
                    }
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
        let mut dims = Vec::new();
        let mut deep = 0;
        if !matches!(self.peek().kind, TokenKind::RParen) {
            loop {
                if matches!(self.peek().kind, TokenKind::Comma | TokenKind::RParen) {
                    let t = self.peek().clone();
                    return Err(self.error_at(&t, format!("Missing argument in call to '{name}'")));
                }
                args.push(self.expr()?);
                dims.push(self.dim);
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
                return Err(ExprError { message: format!("Missing ')' to close the call to '{name}'"), span: Span::new(name_tok.span.start, open.span.end) });
            }
            return Err(self.error_at(&next, format!("Expected ',' or ')' in call to '{name}' but found {}", next.kind.describe())));
        }
        let span = self.span_from(name_tok);
        let n = args.len();
        if n < info.min_args || n > info.max_args {
            let expected = if info.min_args == info.max_args {
                format!("{} argument{}", info.min_args, if info.min_args == 1 { "" } else { "s" })
            } else if info.max_args == builtins::VARIADIC {
                format!("at least {} arguments", info.min_args)
            } else {
                format!("{} to {} arguments", info.min_args, info.max_args)
            };
            return Err(ExprError { message: format!("'{name}' expects {expected}, got {n} — usage: {}", info.signature), span });
        }
        // What `sample` reads is an image input: named, not computed, nor set by the program.
        if info.func == Func::Sample && !matches!(args.first(), Some(Node::Var(i)) if !self.is_set(*i)) {
            return Err(ExprError { message: format!("The first argument of '{name}' is the name of an image input — usage: {}", info.signature), span });
        }
        self.dim = builtins::result_dim(info.func, &dims).map_err(|message| ExprError { message: format!("'{name}': {message}"), span })?;
        self.made(deep)?;
        Ok(Node::Call(info.func, args))
    }
}
