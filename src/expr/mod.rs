//! Scalar expression language for the Math nodes.
//!
//! `compile("(1+x)*sin(myVar)-2.4/cos(myVar2)")` produces a [`Program`] whose
//! free identifiers (`x`, `myVar`, `myVar2`) become node inputs. Invalid input never
//! panics: it yields an [`ExprError`] with a message and the offending byte span.

pub mod builtins;
pub mod lexer;
pub mod parser;
pub mod wgsl;

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use lexer::TokenKind;
use parser::{BinOp, Node, UnOp};

/// Byte range inside the expression source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExprError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for ExprError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (at {}..{})", self.message, self.span.start, self.span.end)
    }
}

/// A successfully parsed and constant-folded expression.
#[derive(Clone, Debug)]
pub struct Program {
    pub root: Node,
    /// Free variables in order of first appearance.
    pub vars: Vec<String>,
}

impl Program {
    /// Evaluate with one value per entry of [`Program::vars`] (missing values are 0).
    pub fn eval(&self, vars: &[f64]) -> f64 {
        eval_node(&self.root, vars)
    }

    /// True when the expression does not depend on any variable.
    pub fn is_constant(&self) -> bool {
        matches!(self.root, Node::Num(_))
    }
}

pub fn compile(src: &str) -> Result<Program, ExprError> {
    if src.len() > 4096 {
        return Err(ExprError {
            message: "Expression is too long (max 4096 characters)".into(),
            span: Span::new(0, src.len()),
        });
    }
    let tokens = lexer::tokenize(src);
    let mut parser = parser::Parser::new(&tokens);
    let root = parser.parse_all()?;
    let root = fold(root);
    Ok(Program { root, vars: parser.vars })
}

pub type Compiled = Arc<Result<Program, ExprError>>;

thread_local! {
    static CACHE: RefCell<HashMap<String, Compiled>> = RefCell::new(HashMap::new());
}

/// Content-addressed compile cache: cheap to call every frame.
pub fn compile_cached(src: &str) -> Compiled {
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(hit) = cache.get(src) {
            return hit.clone();
        }
        if cache.len() > 512 {
            cache.clear();
        }
        let compiled = Arc::new(compile(src));
        cache.insert(src.to_string(), compiled.clone());
        compiled
    })
}

/// Variables of an expression. When the expression does not parse (e.g. while the user
/// is still typing) the identifiers are recovered from the token stream so that node
/// sockets stay stable instead of flickering away.
pub fn variables_lenient(src: &str) -> Vec<String> {
    match &*compile_cached(src) {
        Ok(p) => p.vars.clone(),
        Err(_) => {
            let tokens = lexer::tokenize(src);
            let mut vars: Vec<String> = Vec::new();
            for (i, t) in tokens.iter().enumerate() {
                if let TokenKind::Ident(name) = &t.kind {
                    let is_call = matches!(tokens.get(i + 1).map(|t| &t.kind), Some(TokenKind::LParen));
                    if !is_call && !builtins::is_builtin(name) && !vars.contains(name) {
                        vars.push(name.clone());
                    }
                }
            }
            vars
        }
    }
}

/// Token classes for syntax highlighting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenClass {
    Number,
    Function,
    Constant,
    Variable,
    Operator,
    Paren,
    Error,
}

pub fn highlight(src: &str) -> Vec<(Span, TokenClass)> {
    let tokens = lexer::tokenize(src);
    let mut out = Vec::with_capacity(tokens.len());
    for (i, t) in tokens.iter().enumerate() {
        let class = match &t.kind {
            TokenKind::Eof => continue,
            TokenKind::Number(_) => TokenClass::Number,
            TokenKind::Ident(name) => {
                let is_call = matches!(tokens.get(i + 1).map(|t| &t.kind), Some(TokenKind::LParen));
                if is_call {
                    if builtins::find_function(name).is_some() {
                        TokenClass::Function
                    } else {
                        TokenClass::Error
                    }
                } else if builtins::find_constant(name).is_some() {
                    TokenClass::Constant
                } else if builtins::find_function(name).is_some() {
                    TokenClass::Error
                } else {
                    TokenClass::Variable
                }
            }
            TokenKind::LParen | TokenKind::RParen | TokenKind::Comma => TokenClass::Paren,
            TokenKind::Error(_) => TokenClass::Error,
            _ => TokenClass::Operator,
        };
        out.push((t.span, class));
    }
    out
}

fn truthy(v: f64) -> bool {
    v != 0.0
}

fn bool_f(b: bool) -> f64 {
    if b { 1.0 } else { 0.0 }
}

pub(crate) fn apply_binary(op: BinOp, a: f64, b: f64) -> f64 {
    match op {
        BinOp::Add => a + b,
        BinOp::Sub => a - b,
        BinOp::Mul => a * b,
        BinOp::Div => a / b,
        BinOp::Mod => a - b * (a / b).floor(),
        BinOp::Pow => a.powf(b),
        BinOp::Lt => bool_f(a < b),
        BinOp::Le => bool_f(a <= b),
        BinOp::Gt => bool_f(a > b),
        BinOp::Ge => bool_f(a >= b),
        BinOp::Eq => bool_f(a == b),
        BinOp::Ne => bool_f(a != b),
        BinOp::And => bool_f(truthy(a) && truthy(b)),
        BinOp::Or => bool_f(truthy(a) || truthy(b)),
    }
}

fn eval_node(node: &Node, vars: &[f64]) -> f64 {
    match node {
        Node::Num(v) => *v,
        Node::Var(i) => vars.get(*i).copied().unwrap_or(0.0),
        Node::Unary(UnOp::Neg, a) => -eval_node(a, vars),
        Node::Unary(UnOp::Not, a) => bool_f(!truthy(eval_node(a, vars))),
        Node::Binary(op, a, b) => apply_binary(*op, eval_node(a, vars), eval_node(b, vars)),
        Node::Call(func, args) => {
            let mut values = [0.0f64; 8];
            if args.len() <= values.len() {
                for (slot, arg) in values.iter_mut().zip(args) {
                    *slot = eval_node(arg, vars);
                }
                builtins::call(*func, &values[..args.len()])
            } else {
                let values: Vec<f64> = args.iter().map(|a| eval_node(a, vars)).collect();
                builtins::call(*func, &values)
            }
        }
        Node::Cond(c, a, b) => {
            if truthy(eval_node(c, vars)) {
                eval_node(a, vars)
            } else {
                eval_node(b, vars)
            }
        }
    }
}

/// Constant folding: collapse every variable-free subtree into a number.
fn fold(node: Node) -> Node {
    match node {
        Node::Unary(op, a) => {
            let a = fold(*a);
            match a {
                Node::Num(v) => Node::Num(eval_node(&Node::Unary(op, Box::new(Node::Num(v))), &[])),
                a => Node::Unary(op, Box::new(a)),
            }
        }
        Node::Binary(op, a, b) => {
            let (a, b) = (fold(*a), fold(*b));
            match (&a, &b) {
                (Node::Num(x), Node::Num(y)) => Node::Num(apply_binary(op, *x, *y)),
                _ => Node::Binary(op, Box::new(a), Box::new(b)),
            }
        }
        Node::Call(func, args) => {
            let args: Vec<Node> = args.into_iter().map(fold).collect();
            if func != builtins::Func::Hash && args.iter().all(|a| matches!(a, Node::Num(_))) {
                let values: Vec<f64> =
                    args.iter().map(|a| if let Node::Num(v) = a { *v } else { 0.0 }).collect();
                Node::Num(builtins::call(func, &values))
            } else {
                Node::Call(func, args)
            }
        }
        Node::Cond(c, a, b) => {
            let (c, a, b) = (fold(*c), fold(*a), fold(*b));
            match c {
                Node::Num(v) => {
                    if truthy(v) {
                        a
                    } else {
                        b
                    }
                }
                c => Node::Cond(Box::new(c), Box::new(a), Box::new(b)),
            }
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(src: &str, vars: &[(&str, f64)]) -> f64 {
        let p = compile(src).unwrap_or_else(|e| panic!("{src}: {e}"));
        let values: Vec<f64> = p
            .vars
            .iter()
            .map(|name| vars.iter().find(|(n, _)| n == name).map_or(0.0, |(_, v)| *v))
            .collect();
        p.eval(&values)
    }

    fn err(src: &str) -> ExprError {
        match compile(src) {
            Ok(_) => panic!("'{src}' should not compile"),
            Err(e) => e,
        }
    }

    #[test]
    fn user_example() {
        let src = "(1+x)*sin(myVar)-2.4/cos(myVar2)";
        let p = compile(src).unwrap();
        assert_eq!(p.vars, vec!["x", "myVar", "myVar2"]);
        let (x, a, b) = (0.5f64, 1.2f64, 0.3f64);
        let expected = (1.0 + x) * a.sin() - 2.4 / b.cos();
        assert!((p.eval(&[x, a, b]) - expected).abs() < 1e-12);
    }

    #[test]
    fn precedence() {
        assert_eq!(eval("1 + 2 * 3", &[]), 7.0);
        assert_eq!(eval("(1 + 2) * 3", &[]), 9.0);
        assert_eq!(eval("-2^2", &[]), -4.0);
        assert_eq!(eval("2^3^2", &[]), 512.0);
        assert_eq!(eval("2^-1", &[]), 0.5);
        assert_eq!(eval("10 - 4 - 3", &[]), 3.0);
        assert_eq!(eval("24 / 4 / 2", &[]), 3.0);
        assert_eq!(eval("2 ** 3", &[]), 8.0);
        assert_eq!(eval("-7 % 3", &[]), 2.0);
        assert_eq!(eval("1 < 2 && 3 > 2", &[]), 1.0);
        assert_eq!(eval("0 || 0", &[]), 0.0);
        assert_eq!(eval("!0", &[]), 1.0);
        assert_eq!(eval("x > 1 ? 10 : 20", &[("x", 2.0)]), 10.0);
        assert_eq!(eval("x > 1 ? 10 : x > 0 ? 5 : 0", &[("x", 0.5)]), 5.0);
    }

    #[test]
    fn implicit_multiplication() {
        assert_eq!(eval("2x", &[("x", 3.0)]), 6.0);
        assert_eq!(eval("2x^2", &[("x", 3.0)]), 18.0);
        assert!((eval("2pi", &[]) - std::f64::consts::TAU).abs() < 1e-12);
        assert_eq!(eval("3(1+1)", &[]), 6.0);
        assert_eq!(eval("(1+1)(2+2)", &[]), 8.0);
        assert_eq!(eval("2e", &[]), 2.0 * std::f64::consts::E);
    }

    #[test]
    fn functions_and_constants() {
        let close = |src: &str, expected: f64| {
            let v = eval(src, &[]);
            assert!((v - expected).abs() < 1e-9, "{src} = {v}, expected {expected}");
        };
        close("min(3, 1, 2)", 1.0);
        close("max(3, 1, 2)", 3.0);
        close("clamp(5, 0, 1)", 1.0);
        close("lerp(0, 10, 0.25)", 2.5);
        close("log(100)", 2.0);
        close("log(8, 2)", 3.0);
        close("ln(e)", 1.0);
        close("sqrt(16)", 4.0);
        close("fract(-0.25)", 0.75);
        close("sign(-3)", -1.0);
        close("round(2.5)", 3.0);
        close("smoothstep(0, 1, 0.5)", 0.5);
        close("if(1, 2, 3)", 2.0);
        close("degrees(pi)", 180.0);
        let h = eval("hash(x)", &[("x", 3.0)]);
        assert!((0.0..1.0).contains(&h));
        assert_eq!(h, eval("hash(x)", &[("x", 3.0)]));
    }

    #[test]
    fn constant_folding() {
        let p = compile("2 * pi + sin(0)").unwrap();
        assert!(p.is_constant());
        let p = compile("x * (2 + 3)").unwrap();
        match &p.root {
            Node::Binary(BinOp::Mul, _, b) => assert_eq!(**b, Node::Num(5.0)),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn errors_are_reported_not_panicked() {
        assert!(err("").message.contains("empty"));
        assert!(err("   ").message.contains("empty"));
        assert!(err("(1 + 2").message.contains("Missing ')'"));
        assert!(err("1 + 2)").message.contains("Unmatched ')'"));
        assert!(err("1 +").message.contains("incomplete"));
        assert!(err("foo(1)").message.contains("Unknown function"));
        assert!(err("Sin(1)").message.contains("Did you mean 'sin'"));
        assert!(err("sin").message.contains("is a function"));
        assert!(err("sin(1, 2)").message.contains("expects 1 argument"));
        assert!(err("min(1)").message.contains("at least 2"));
        assert!(err("pi(2)").message.contains("constant"));
        assert!(err("x y").message.contains("missing operator"));
        assert!(err("1 = 2").message.contains("=="));
        assert!(err("1 $ 2").message.contains("Unexpected character"));
        assert!(err("()").message.contains("Empty parentheses"));
        assert!(err("max(1,,2)").message.contains("Missing argument"));
        assert!(err("1 ? 2").message.contains("':'"));
        assert!(err("1.2.3").message.contains("decimal point"));
        let e = err("1 + @");
        assert_eq!(e.span, Span::new(4, 5));
    }

    #[test]
    fn deep_nesting_is_an_error() {
        let src = format!("{}1{}", "(".repeat(500), ")".repeat(500));
        assert!(err(&src).message.contains("deeply"));
        let src = "-".repeat(1000) + "1";
        assert!(compile(&src).is_err());
    }

    #[test]
    fn fuzz_never_panics() {
        // Deterministic pseudo random strings over the language alphabet.
        let alphabet: Vec<char> = "0123456789.eExyz_ +-*/%^()!<>=&|?:,sincoqrtlgpmaπ@#".chars().collect();
        let mut state = 0x1234_5678u32;
        for _ in 0..20_000 {
            let len = (builtins::pcg_hash(state) % 24) as usize;
            let mut s = String::new();
            for _ in 0..len {
                state = builtins::pcg_hash(state);
                s.push(alphabet[state as usize % alphabet.len()]);
            }
            state = state.wrapping_add(1);
            let _ = compile(&s);
            let _ = highlight(&s);
            let _ = variables_lenient(&s);
            if let Ok(p) = compile(&s) {
                let _ = p.eval(&vec![0.5; p.vars.len()]);
                let _ = wgsl::to_wgsl(&p, &|i| format!("v{i}"));
            }
        }
    }

    #[test]
    fn lenient_variables_while_typing() {
        assert_eq!(variables_lenient("sin(myVar"), vec!["myVar"]);
        assert_eq!(variables_lenient("x + y *"), vec!["x", "y"]);
    }
}
