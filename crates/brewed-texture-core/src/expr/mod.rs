//! Scalar expression language for the Math nodes.
//!
//! `compile("(1+x)*sin(myVar)-2.4/cos(myVar2)")` produces a [`Program`] whose
//! free identifiers (`x`, `myVar`, `myVar2`) become node inputs. Invalid input never
//! panics: it yields an [`ExprError`] with a message and the offending byte span.

pub mod builtins;
pub mod lexer;
pub mod parser;
pub mod script;
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
    /// The value (the first of a list).
    pub root: Node,
    /// The values after the first of a list (`r, g, b, a` in Pixel Math): up to three.
    pub more: Vec<Node>,
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
        matches!(self.root, Node::Num(_)) && self.more.iter().all(|n| matches!(n, Node::Num(_)))
    }

    /// How many values the expression gives: one, or a list of up to four.
    pub fn components(&self) -> usize {
        1 + self.more.len()
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
            Language::Math => &[],
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
    if src.len() > 4096 {
        return Err(ExprError {
            message: "Expression is too long (max 4096 characters)".into(),
            span: Span::new(0, src.len()),
        });
    }
    let tokens = lexer::tokenize(src);
    let mut parser = parser::Parser::new(&tokens);
    let mut list = parser.parse_list()?.into_iter().map(fold);
    let root = list.next().expect("a list has a value");
    Ok(Program { root, more: list.collect(), vars: parser.vars })
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

fn eval_node(node: &Node, vars: &[f64]) -> f64 {
    eval_with(node, vars, &|k| builtins::call(builtins::Func::Rand, &[k]))
}

/// The value of `node`, `rand(k)` being `rand`'s.
pub(crate) fn eval_with(node: &Node, vars: &[f64], rand: &dyn Fn(f64) -> f64) -> f64 {
    match node {
        Node::Num(v) => *v,
        Node::Var(i) => vars.get(*i).copied().unwrap_or(0.0),
        Node::Unary(UnOp::Neg, a) => -eval_with(a, vars, rand),
        Node::Unary(UnOp::Not, a) => bool_f(!truthy(eval_with(a, vars, rand))),
        Node::Binary(op, a, b) => apply_binary(*op, eval_with(a, vars, rand), eval_with(b, vars, rand)),
        Node::Call(builtins::Func::Rand, args) => rand(args.first().map_or(0.0, |k| eval_with(k, vars, rand))),
        Node::Call(func, args) => {
            let mut values = [0.0f64; 8];
            if args.len() <= values.len() {
                for (slot, arg) in values.iter_mut().zip(args) {
                    *slot = eval_with(arg, vars, rand);
                }
                builtins::call(*func, &values[..args.len()])
            } else {
                let values: Vec<f64> = args.iter().map(|a| eval_with(a, vars, rand)).collect();
                builtins::call(*func, &values)
            }
        }
        Node::Cond(c, a, b) => {
            if truthy(eval_with(c, vars, rand)) {
                eval_with(a, vars, rand)
            } else {
                eval_with(b, vars, rand)
            }
        }
    }
}

/// Constant folding: collapse every variable-free subtree into a number.
pub(crate) fn fold(node: Node) -> Node {
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
            let random = matches!(func, builtins::Func::Hash | builtins::Func::Rand);
            if !random && args.iter().all(|a| matches!(a, Node::Num(_))) {
                let values: Vec<f64> = args.iter().map(|a| if let Node::Num(v) = a { *v } else { 0.0 }).collect();
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
        let values: Vec<f64> =
            p.vars.iter().map(|name| vars.iter().find(|(n, _)| n == name).map_or(0.0, |(_, v)| *v)).collect();
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
        let code = wgsl::components_wgsl(&p, &|i| format!("V{i}"), &|i, _, _| format!("S{i}"));
        assert_eq!(code.len(), 3);
        assert_eq!(code[2], "f32(0.5)");
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
