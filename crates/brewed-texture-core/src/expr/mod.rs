//! The expression language of the Math, Pixel Math and FX-Map nodes.
//!
//! `compile("(1+x)*sin(myVar)-2.4/cos(myVar2)")` produces a [`Program`] whose free identifiers
//! (`x`, `myVar`, `myVar2`) become node inputs. A value is a number or a vector of two to four
//! components (`vec3(r, g, b)`, `img.rgb`), and a program may give names values before its
//! value, one statement after another (see [`script`]):
//! ```text
//! n = normal.rgb * 2 - 1
//! saturate(dot(n, normalize(vec3(1, 1, 1))))
//! ```
//! Invalid input never panics: it yields an [`ExprError`] with a message and the offending byte
//! span.

pub mod builtins;
pub mod lexer;
pub mod parser;
pub mod script;
pub mod wgsl;

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use builtins::Func;
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

/// A value: a number, or a vector of two to four components.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vector {
    pub len: u8,
    pub c: [f64; 4],
}

impl Vector {
    pub const fn number(v: f64) -> Vector {
        Vector { len: 1, c: [v, 0.0, 0.0, 0.0] }
    }

    fn of(c: &[f64]) -> Vector {
        let mut v = Vector { len: c.len().clamp(1, 4) as u8, c: [0.0; 4] };
        v.c[..c.len().min(4)].copy_from_slice(&c[..c.len().min(4)]);
        v
    }

    /// Component `i`: a number is the same in every one.
    pub fn at(&self, i: usize) -> f64 {
        if self.len == 1 { self.c[0] } else { self.c[i.min(3)] }
    }

    /// Its components, in order.
    pub fn components(&self) -> &[f64] {
        &self.c[..usize::from(self.len)]
    }
}

/// A successfully parsed and constant-folded program: the statements that give names values,
/// then its value.
#[derive(Clone, Debug)]
pub struct Program {
    /// The statements before the value, in order.
    pub statements: Vec<script::Statement>,
    /// The value (the first of a list).
    pub root: Node,
    /// The values after the first of a list (`r, g, b, a` in Pixel Math): up to three.
    pub more: Vec<Node>,
    /// How many components each value of the list has, `root`'s first.
    pub dims: Vec<u8>,
    /// Free variables in order of first appearance: variables `0..vars.len()` of its
    /// expressions.
    pub vars: Vec<String>,
    /// The names its statements give values, in the order they first do: the variables after
    /// [`Program::vars`].
    pub locals: Vec<String>,
    /// How many components each of [`Program::locals`] has.
    pub local_dims: Vec<u8>,
}

impl Program {
    /// Evaluate with one value per entry of [`Program::vars`] (missing values are 0): the first
    /// component of its value.
    pub fn eval(&self, vars: &[f64]) -> f64 {
        self.eval_all(vars)[0]
    }

    /// Every component of its value, in order.
    pub fn eval_all(&self, vars: &[f64]) -> Vec<f64> {
        let rand = |k| builtins::call(Func::Rand, &[k]);
        let inputs: Vec<Vector> = (0..self.vars.len()).map(|i| Vector::number(vars.get(i).copied().unwrap_or(0.0))).collect();
        let (slots, _) = script::run(&self.statements, &inputs, &self.local_dims, &[], &rand);
        std::iter::once(&self.root).chain(&self.more).flat_map(|n| eval_with(n, &slots, &rand).components().to_vec()).collect()
    }

    /// True when the expression does not depend on any variable.
    pub fn is_constant(&self) -> bool {
        self.statements.is_empty() && matches!(self.root, Node::Num(_)) && self.more.iter().all(|n| matches!(n, Node::Num(_)))
    }

    /// How many values the expression gives: the components of its value, one, or up to four.
    pub fn components(&self) -> usize {
        self.dims.iter().map(|d| usize::from(*d)).sum()
    }

    /// How many components variable `i` of its expressions has: an input one.
    pub fn var_dim(&self, i: usize) -> u8 {
        i.checked_sub(self.vars.len()).and_then(|l| self.local_dims.get(l)).copied().unwrap_or(1)
    }
}

/// What an expression of a node is: one value made of values (Math), a value of each pixel
/// (Pixel Math), or a script of the values of each splat (FX-Map).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Math,
    Pixel,
    Splats,
}

impl Language {
    /// The names the language gives an expression to read.
    pub fn builtins(self) -> &'static [&'static str] {
        match self {
            Language::Math => crate::nodes::MATH_BUILTINS,
            Language::Pixel => crate::nodes::PIXEL_MATH_BUILTINS,
            Language::Splats => crate::nodes::FX_MAP_BUILTINS,
        }
    }

    /// The rules of its scripts, for a language of scripts.
    pub fn script_rules(self) -> Option<&'static script::ScriptRules> {
        match self {
            Language::Splats => Some(&crate::nodes::FX_MAP_RULES),
            _ => None,
        }
    }

    /// What `src` reads, built-ins too, in the order it first reads them; or why it cannot be
    /// read.
    pub fn reads(self, src: &str) -> Result<Vec<String>, String> {
        match self.script_rules() {
            Some(rules) => match &*script::compile_script_cached(src, rules) {
                Ok(s) => Ok(s.vars.clone()),
                Err(e) => Err(format!("The program is incorrect: {}", e.message)),
            },
            None => match &*compile_cached(src) {
                Ok(p) => Ok(p.vars.clone()),
                Err(e) => Err(format!("The expression is incorrect: {}", e.message)),
            },
        }
    }

    /// The inputs of `src`: what it reads that is not built in, stable while it is being
    /// typed (see [`variables_lenient`] and [`script::script_vars_lenient`]).
    pub fn inputs_lenient(self, src: &str) -> Vec<String> {
        match self.script_rules() {
            Some(rules) => script::script_vars_lenient(src, rules),
            None => {
                let mut vars = variables_lenient(src);
                vars.retain(|v| !self.builtins().contains(&v.as_str()));
                vars
            }
        }
    }

    /// Where `src` is wrong, if it is.
    pub fn error(self, src: &str) -> Option<ExprError> {
        match self.script_rules() {
            Some(rules) => script::compile_script_cached(src, rules).as_ref().as_ref().err().cloned(),
            None => compile_cached(src).as_ref().as_ref().err().cloned(),
        }
    }
}

pub fn compile(src: &str) -> Result<Program, ExprError> {
    if src.len() > script::MAX_SCRIPT {
        return Err(ExprError { message: format!("Expression is too long (max {} characters)", script::MAX_SCRIPT), span: Span::new(0, src.len()) });
    }
    let tokens = lexer::tokenize(src);
    let mut parser = parser::Parser::new(&tokens);
    let (assignments, list) = parser.parse_program(true, &[])?;
    let linked = script::link(assignments, list, &parser, &script::ScriptRules::NONE)?;
    let mut values = linked.value.into_iter();
    let (root, dim) = values.next().expect("a program of an expression has a value");
    let (more, more_dims): (Vec<Node>, Vec<u8>) = values.unzip();
    Ok(Program {
        statements: linked.statements,
        root,
        more,
        dims: std::iter::once(dim).chain(more_dims).collect(),
        vars: linked.vars,
        locals: linked.locals,
        local_dims: linked.local_dims,
    })
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

/// The words of `src` that are read, not called, not set and not channels taken of a value,
/// in order: the variables of a program that does not read (see [`variables_lenient`]).
pub(crate) fn words_read(src: &str, own: &dyn Fn(&str) -> bool) -> Vec<String> {
    let tokens = lexer::tokenize(src);
    let set: Vec<&str> = tokens
        .windows(2)
        .filter_map(|w| match (&w[0].kind, &w[1].kind) {
            (TokenKind::Ident(name), TokenKind::Assign) => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let mut vars: Vec<String> = Vec::new();
    for (i, t) in tokens.iter().enumerate() {
        if let TokenKind::Ident(name) = &t.kind {
            let next = tokens.get(i + 1).map(|t| &t.kind);
            let called = matches!(next, Some(TokenKind::LParen));
            let setting = matches!(next, Some(TokenKind::Assign));
            let channels = i > 0 && tokens[i - 1].kind == TokenKind::Dot;
            if !called && !setting && !channels && !builtins::is_builtin(name) && !own(name) && !set.contains(&name.as_str()) && !vars.contains(name) {
                vars.push(name.clone());
            }
        }
    }
    vars
}

/// Variables of an expression. When the expression does not parse (e.g. while the user
/// is still typing) the identifiers are recovered from the token stream so that node
/// sockets stay stable instead of flickering away.
pub fn variables_lenient(src: &str) -> Vec<String> {
    match &*compile_cached(src) {
        Ok(p) => p.vars.clone(),
        Err(_) => words_read(src, &|_| false),
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
    Comment,
    Error,
}

pub fn highlight(src: &str) -> Vec<(Span, TokenClass)> {
    let tokens = lexer::tokenize(src);
    let mut out = Vec::with_capacity(tokens.len());
    // Comments are no tokens: `#` to the end of its line (no word of the language has a `#`).
    let mut comments = Vec::new();
    let mut at = 0;
    while let Some(start) = src[at..].find('#').map(|i| at + i) {
        let end = src[start..].find('\n').map_or(src.len(), |i| start + i);
        comments.push((Span::new(start, end), TokenClass::Comment));
        at = end;
    }
    for (i, t) in tokens.iter().enumerate() {
        let class = match &t.kind {
            TokenKind::Eof => continue,
            TokenKind::Number(_) => TokenClass::Number,
            // The channels taken of a value.
            TokenKind::Ident(_) if i > 0 && tokens[i - 1].kind == TokenKind::Dot => TokenClass::Operator,
            TokenKind::Ident(name) => {
                let is_call = matches!(tokens.get(i + 1).map(|t| &t.kind), Some(TokenKind::LParen));
                if is_call {
                    if builtins::find_function(name).is_some() { TokenClass::Function } else { TokenClass::Error }
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
    if !comments.is_empty() {
        out.extend(comments);
        out.sort_by_key(|(span, _)| span.start);
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

/// Of each component: `f` of the components at the same place of `args` (a number is the same
/// at every place), as many as the longest has.
fn each(args: &[Vector], f: impl Fn(&[f64]) -> f64) -> Vector {
    let len = args.iter().map(|a| a.len).max().unwrap_or(1);
    let mut out = Vector { len, c: [0.0; 4] };
    let mut values = [0.0f64; 8];
    for k in 0..usize::from(len) {
        if args.len() <= values.len() {
            for (slot, a) in values.iter_mut().zip(args) {
                *slot = a.at(k);
            }
            out.c[k] = f(&values[..args.len()]);
        } else {
            let values: Vec<f64> = args.iter().map(|a| a.at(k)).collect();
            out.c[k] = f(&values);
        }
    }
    out
}

/// The value of `node`: `vars` those of its variables, `rand(k)` being `rand`'s.
pub(crate) fn eval_with(node: &Node, vars: &[Vector], rand: &dyn Fn(f64) -> f64) -> Vector {
    match node {
        Node::Num(v) => Vector::number(*v),
        Node::Var(i) => vars.get(*i).copied().unwrap_or(Vector::number(0.0)),
        Node::Unary(UnOp::Neg, a) => each(&[eval_with(a, vars, rand)], |x| -x[0]),
        Node::Unary(UnOp::Not, a) => Vector::number(bool_f(!truthy(eval_with(a, vars, rand).c[0]))),
        Node::Binary(op, a, b) => {
            let (a, b) = (eval_with(a, vars, rand), eval_with(b, vars, rand));
            if op.is_logical() { Vector::number(apply_binary(*op, a.c[0], b.c[0])) } else { each(&[a, b], |x| apply_binary(*op, x[0], x[1])) }
        }
        Node::Call(Func::Rand, args) => Vector::number(rand(args.first().map_or(0.0, |k| eval_with(k, vars, rand).c[0]))),
        // What an image input is at (x, y): the variable's value (only an image differs).
        Node::Call(Func::Sample, args) => args.first().map_or(Vector::number(0.0), |v| eval_with(v, vars, rand)),
        Node::Call(func, args) => {
            let values: Vec<Vector> = args.iter().map(|a| eval_with(a, vars, rand)).collect();
            call_vectors(*func, &values)
        }
        Node::Cond(c, a, b) => {
            let (a, b) = (eval_with(a, vars, rand), eval_with(b, vars, rand));
            let chosen = if truthy(eval_with(c, vars, rand).c[0]) { a } else { b };
            // As many components as the wider of the two.
            each(&[chosen, Vector { len: a.len.max(b.len), c: [0.0; 4] }], |x| x[0])
        }
        Node::Swizzle(base, places) => {
            let v = eval_with(base, vars, rand);
            // A number's channels: the same in red, green and blue, and opaque.
            let channels = if v.len == 1 { [v.c[0], v.c[0], v.c[0], 1.0] } else { v.c };
            let picked: Vec<f64> = places.iter().map(|p| channels[usize::from(*p).min(3)]).collect();
            Vector::of(&picked)
        }
    }
}

/// `func` of values that may be vectors.
fn call_vectors(func: Func, args: &[Vector]) -> Vector {
    let a = args.first().copied().unwrap_or(Vector::number(0.0));
    let b = args.get(1).copied().unwrap_or(Vector::number(0.0));
    let dot = |a: &Vector, b: &Vector| (0..usize::from(a.len.max(b.len))).map(|k| a.at(k) * b.at(k)).sum::<f64>();
    match func {
        Func::Vec2 | Func::Vec3 | Func::Vec4 => {
            let n = match func {
                Func::Vec2 => 2,
                Func::Vec3 => 3,
                _ => 4,
            };
            if args.len() == 1 && a.len == 1 {
                return Vector::of(&vec![a.c[0]; n]);
            }
            let all: Vec<f64> = args.iter().flat_map(|v| v.components().to_vec()).collect();
            Vector::of(&all[..all.len().min(n)])
        }
        Func::Dot => Vector::number(dot(&a, &b)),
        Func::Length => Vector::number(dot(&a, &a).sqrt()),
        Func::Distance => {
            let d = each(&[a, b], |x| x[0] - x[1]);
            Vector::number(dot(&d, &d).sqrt())
        }
        Func::Normalize if a.len == 1 => Vector::number(builtins::call(Func::Sign, &[a.c[0]])),
        Func::Normalize => {
            let l = dot(&a, &a).sqrt();
            each(&[a], |x| x[0] / l)
        }
        Func::Cross => Vector::of(&[a.at(1) * b.at(2) - a.at(2) * b.at(1), a.at(2) * b.at(0) - a.at(0) * b.at(2), a.at(0) * b.at(1) - a.at(1) * b.at(0)]),
        // Of numbers only.
        Func::Hash | Func::Hypot | Func::Rand | Func::Sample => {
            let values: Vec<f64> = args.iter().map(|v| v.c[0]).collect();
            Vector::number(builtins::call(func, &values))
        }
        Func::If => {
            let chosen = if truthy(a.c[0]) { args[1] } else { args[2] };
            each(&[chosen, Vector { len: args[1].len.max(args[2].len), c: [0.0; 4] }], |x| x[0])
        }
        // Of each component.
        _ => each(args, |values| builtins::call(func, values)),
    }
}

/// Constant folding: collapse every variable-free subtree that is a number into one.
pub(crate) fn fold(node: Node) -> Node {
    let constant = |n: &Node| matches!(n, Node::Num(_));
    let folded = match node {
        Node::Unary(op, a) => Node::Unary(op, Box::new(fold(*a))),
        Node::Binary(op, a, b) => Node::Binary(op, Box::new(fold(*a)), Box::new(fold(*b))),
        Node::Call(func, args) => Node::Call(func, args.into_iter().map(fold).collect()),
        Node::Cond(c, a, b) => {
            let (c, a, b) = (fold(*c), fold(*a), fold(*b));
            // Only numbers are chosen ahead: a branch as long as the other keeps its length.
            match (&c, &a, &b) {
                (Node::Num(v), Node::Num(x), Node::Num(y)) => return Node::Num(if truthy(*v) { *x } else { *y }),
                _ => Node::Cond(Box::new(c), Box::new(a), Box::new(b)),
            }
        }
        Node::Swizzle(base, places) => Node::Swizzle(Box::new(fold(*base)), places),
        other => return other,
    };
    let all_constant = match &folded {
        Node::Unary(_, a) => constant(a),
        Node::Binary(_, a, b) => constant(a) && constant(b),
        Node::Call(func, args) => !matches!(func, Func::Hash | Func::Rand) && args.iter().all(constant),
        Node::Swizzle(base, _) => constant(base),
        _ => false,
    };
    if !all_constant {
        return folded;
    }
    let value = eval_with(&folded, &[], &|k| builtins::call(Func::Rand, &[k]));
    if value.len == 1 { Node::Num(value.c[0]) } else { folded }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(src: &str, vars: &[(&str, f64)]) -> f64 {
        let p = compile(src).unwrap_or_else(|e| panic!("{src}: {e}"));
        let values: Vec<f64> = p.vars.iter().map(|name| vars.iter().find(|(n, _)| n == name).map_or(0.0, |(_, v)| *v)).collect();
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

    /// The shader of an expression grows as the expression does: a value a function reads
    /// twice (`fract`, `lerp`) is written once, else `fract(fract(fract(…)))` would double at
    /// every call, to more than the memory holds.
    #[test]
    fn the_shader_of_an_expression_grows_as_it_does() {
        for f in ["fract(", "lerp(", "sin(", "clamp("] {
            let rest = match f {
                "lerp(" => ", v, 0.5)",
                "clamp(" => ", 0, 1)",
                _ => ")",
            };
            let src = format!("{}u{}", f.repeat(40), rest.repeat(40));
            let p = compile(&src).unwrap();
            let code = wgsl::to_wgsl(&p, &|i| format!("v{i}"));
            assert!(code.len() < 20 * src.len(), "{f}: {} bytes of shader for {} of expression", code.len(), src.len());
        }
    }

    /// The deepest expressions read are read, computed, written as shaders and dropped on a
    /// stack smaller than any thread of the program has (the main thread of Windows has 1 MB);
    /// longer chains of operations, which the 4096 characters of an expression can hold 2048
    /// of, are refused, saying why.
    #[test]
    fn the_deepest_expressions_fit_the_stack() {
        let longest = parser::MAX_TREE - 1;
        let read = [
            ("u+".repeat(longest) + "u", 128.0),
            ("u*".repeat(longest) + "u", 1.0),
            ("(u+".repeat(40) + &"u*".repeat(80) + "u" + &")".repeat(40), 41.0),
            ("-(".repeat(20) + "u" + &")".repeat(20), 1.0),
            ("max(".to_string() + &"u, ".repeat(1300) + "2)", 2.0),
        ];
        let refused = ["u+".repeat(2047) + "u", "u+".repeat(longest + 1) + "u", "u^".repeat(40) + &"u-".repeat(2000) + "u"];
        std::thread::Builder::new()
            .stack_size(256 << 10)
            .spawn(move || {
                for (src, value) in &read {
                    assert!(src.len() <= 4096, "{}", src.len());
                    let p = compile(src).unwrap_or_else(|e| panic!("{e} in {src}"));
                    assert_eq!(p.eval(&[1.0]), *value, "{src}");
                    let code = wgsl::to_wgsl(&p, &|i| format!("v{i}"));
                    assert!(code.len() < 10 * src.len(), "{src}");
                    let _ = (variables_lenient(src), highlight(src), p.clone());
                }
                for src in &refused {
                    let e = compile(src).unwrap_err();
                    assert!(e.message.contains("at most 128 operations in a row"), "{e}");
                    assert_eq!(variables_lenient(src), ["u"]);
                    let _ = highlight(src);
                }
            })
            .unwrap()
            .join()
            .unwrap();
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

    /// A list of up to four values (red, green, blue and alpha of Pixel Math), separated by
    /// commas outside calls.
    #[test]
    fn lists_of_values() {
        let p = compile("u, v * 2, 0.5").unwrap();
        assert_eq!((p.components(), p.vars.clone()), (3, vec!["u".to_string(), "v".to_string()]));
        assert_eq!(compile("min(a, b), c").unwrap().components(), 2);
        assert_eq!(compile("1, 2, 3, 4").unwrap().components(), 4);
        assert!(compile("1, 2, 3, 4").unwrap().is_constant());
        assert!(!compile("1, x").unwrap().is_constant());
        assert!(err("a, b, c, d, e").message.contains("At most four"));
        assert!(err("a,").message.contains("Missing value"));
        assert!(err("a,, b").message.contains("Missing value"));
        let (_, code) = wgsl::program_wgsl(&p, &env(&p), &|k| format!("L{k}"));
        assert_eq!(code.len(), 3);
        assert_eq!(code[2], "f32(0.5)");
    }

    /// How the WGSL of the tests reads what it reads.
    fn env(p: &Program) -> wgsl::Env<'_> {
        let inputs = p.vars.len();
        wgsl::Env {
            var: Box::leak(Box::new(move |i: usize| if i < inputs { format!("V{i}") } else { format!("L{}", i - inputs) })),
            dim: Box::leak(Box::new(|i: usize| p.var_dim(i))),
            texel: Box::leak(Box::new(|i: usize| format!("T{i}"))),
            sample: Box::leak(Box::new(|i: usize, x: &str, y: &str| format!("S{i}[{x}|{y}]"))),
            sample_texel: Box::leak(Box::new(|i: usize, x: &str, y: &str| format!("ST{i}[{x}|{y}]"))),
        }
    }

    fn eval_all(src: &str, vars: &[(&str, f64)]) -> Vec<f64> {
        let p = compile(src).unwrap_or_else(|e| panic!("{src}: {e}"));
        let values: Vec<f64> = p.vars.iter().map(|name| vars.iter().find(|(n, _)| n == name).map_or(0.0, |(_, v)| *v)).collect();
        p.eval_all(&values)
    }

    /// Vectors of two to four components: made of numbers and vectors, their components taken
    /// in any order, and worked on component by component, a number going with a vector as a
    /// vector of it.
    #[test]
    fn vectors() {
        assert_eq!(eval_all("vec3(1, 2, 3)", &[]), [1.0, 2.0, 3.0]);
        assert_eq!(eval_all("vec3(0.5)", &[]), [0.5, 0.5, 0.5]);
        assert_eq!(eval_all("vec4(vec2(1, 2), 3, 4).wzyx", &[]), [4.0, 3.0, 2.0, 1.0]);
        assert_eq!(eval_all("vec3(1, 2, 3) * 2 + 1", &[]), [3.0, 5.0, 7.0]);
        assert_eq!(eval_all("vec2(1, 4) / vec2(2, 8)", &[]), [0.5, 0.5]);
        assert_eq!(eval_all("min(vec3(1, 5, 3), 2)", &[]), [1.0, 2.0, 2.0]);
        assert_eq!(eval_all("clamp(vec2(-1, 2), 0, 1)", &[]), [0.0, 1.0]);
        assert_eq!(eval_all("dot(vec3(1, 2, 3), vec3(4, 5, 6))", &[]), [32.0]);
        assert_eq!(eval_all("length(vec2(3, 4))", &[]), [5.0]);
        assert_eq!(eval_all("distance(vec2(1, 1), vec2(4, 5))", &[]), [5.0]);
        assert_eq!(eval_all("normalize(vec2(0, 2))", &[]), [0.0, 1.0]);
        assert_eq!(eval_all("cross(vec3(1, 0, 0), vec3(0, 1, 0))", &[]), [0.0, 0.0, 1.0]);
        assert_eq!(eval_all("vec3(1, 2, 3).zx", &[]), [3.0, 1.0]);
        assert_eq!(eval_all("x > 0 ? vec2(1, 2) : 0", &[("x", 1.0)]), [1.0, 2.0]);
        assert_eq!(eval_all("x > 0 ? vec2(1, 2) : 0", &[("x", -1.0)]), [0.0, 0.0]);
        // A value read from outside is the same in red, green and blue, and opaque.
        assert_eq!(eval_all("k.rgba", &[("k", 0.25)]), [0.25, 0.25, 0.25, 1.0]);
        // A list of vectors and numbers: four components at most in all.
        let p = compile("vec2(u, v), 0.5").unwrap();
        assert_eq!((p.components(), p.dims.clone()), (3, vec![2, 1]));
        assert_eq!(compile("img.rgb").unwrap().components(), 3);
        assert!(err("vec3(1, 2, 3), 4, 5").message.contains("At most four"));
        assert!(err("vec2(1, 2) + vec3(1, 2, 3)").message.contains("a vec2 and a vec3 do not go together"));
        assert!(err("vec2(1, 2) < 1").message.contains("takes numbers"));
        assert!(err("vec3(1, 2)").message.contains("takes 3 components"));
        assert!(err("cross(vec2(1, 2), vec2(3, 4))").message.contains("vec3"));
        assert!(err("vec2(1, 2).z").message.contains("has no 'z'"));
        assert!(err("v.xq").message.contains("channels are"));
        assert!(err("hash(vec2(1, 2))").message.contains("takes numbers"));
        assert!(err("vec2(1, 2) ? 1 : 2").message.contains("condition"));
    }

    /// An expression may give names values before its value; each is read after it, and stays
    /// what it first was.
    #[test]
    fn names_given_values_before_the_value() {
        assert_eq!(eval_all("a = 2; a * 3", &[]), [6.0]);
        assert_eq!(eval_all("n = vec3(0, 3, 4)\nl = length(n)\nn / l", &[]), [0.0, 0.6, 0.8]);
        assert_eq!(eval_all("a = x; a = a + 1; a", &[("x", 1.0)]), [2.0]);
        let p = compile("n = img.rgb * 2 - 1\ndot(n, normalize(vec3(1, 1, 1)))").unwrap();
        assert_eq!((p.vars.clone(), p.locals.clone(), p.local_dims.clone()), (vec!["img".to_string()], vec!["n".to_string()], vec![3]));
        assert!(err("a = 1").message.contains("value of the expression after the statements"));
        assert!(err("a = b; b = 1; a").message.contains("'b' is read before"));
        assert!(err("a = 1; a = vec2(1, 2); a").message.contains("'a' is a number: it cannot become a vec2"));
        assert!(err("a = 1, 2; a").message.contains("'a' takes one value"));
        assert!(err("p = 1; sample(p, 0, 0)").message.contains("name of an image input"));
        // The statements come first.
        assert!(err("1; a = 2").message.contains("comes last"));
        // Not an assignment: a comparison was meant.
        assert!(err("a = 1 = 2").message.contains("=="));
    }

    /// The WGSL of vectors: a number widened where it goes with one, the channels of an image
    /// read where they are taken, the statements before the value.
    #[test]
    fn vectors_as_wgsl() {
        let p = compile("n = img.rgb * 2 - 1\nm = mod(n, 0.5)\nm.xy, sample(img, u, v).a").unwrap();
        let (body, values) = wgsl::program_wgsl(&p, &env(&p), &|k| format!("L{k}"));
        assert!(body.contains("var L0: vec3<f32> = vec3<f32>(0.0);"), "{body}");
        assert!(body.contains("L0 = (((T0).xyz * f32(2.0)) - f32(1.0));"), "{body}");
        assert!(body.contains("fx_mod3((L0), vec3<f32>(f32(0.5)))"), "{body}");
        assert_eq!(values, ["fx_value0.x", "fx_value0.y", "(ST0[(V1)|(V2)]).w"]);
    }

    /// `sample(image, x, y)` reads an image input anywhere (in Pixel Math); what it reads is
    /// named, and a value, read anywhere, is itself.
    #[test]
    fn sample_names_the_image_it_reads() {
        let p = compile("sample(height, u + 0.01, v) - height").unwrap();
        assert_eq!(p.vars, vec!["height", "u", "v"]);
        assert_eq!(eval("sample(k, 0.2, 0.9) * 2", &[("k", 3.0)]), 6.0);
        assert!(err("sample(0.5, u, v)").message.contains("name of an image input"));
        assert!(err("sample(a + b, u, v)").message.contains("name of an image input"));
        assert!(err("sample(a, u)").message.contains("expects 3 arguments"));
        // Never folded away: what it reads is only known on the GPU.
        let code = wgsl::to_wgsl_sampling(&p, &|i| format!("V{i}"), &|i, x, y| format!("S{i}[{x}|{y}]"));
        assert!(code.contains("S0[((V1) + f32(0.01))|(V2)]"), "{code}");
        assert!(wgsl::to_wgsl(&p, &|i| format!("V{i}")).contains("(V0)"));
    }
}
