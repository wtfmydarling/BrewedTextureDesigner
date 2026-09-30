//! Scripts: values given names in order, one `name = value` after another (FX-Map's program).
//!
//! ```text
//! # Where each splat goes, and how big it is.
//! x = rand(1)
//! y = rand(2)
//! r = rand(3)
//! size = 0.05 + 0.1 * r
//! ```
//!
//! A statement begins where a name is followed by `=`: statements need no `;` between them,
//! nor lines of their own (`;` may separate them all the same), and `#` begins a comment to
//! the end of its line. A value is an expression of the language, or a list of them for the
//! names that take lists. What the script sets is its own from there on: a later value reads
//! it. What it reads and never sets is its inputs (and the built-ins of the language); a
//! name read before the script sets it is an error, but for its outputs, which are there
//! from the start, with values of their own.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use super::builtins::{self, Func};
use super::lexer::{self, TokenKind};
use super::parser::{Node, Parser};
use super::{ExprError, Span, eval_with, fold};

/// The longest script read (an expression is at most 4096 characters).
pub const MAX_SCRIPT: usize = 16384;

/// What a language of scripts gives a script and takes from it.
#[derive(Debug)]
pub struct ScriptRules {
    /// Names the language gives the script to read, which it cannot set.
    pub builtins: &'static [&'static str],
    /// What the script makes: names there from the start, each with how many values it takes,
    /// at least and at most (a list of them where more than one).
    pub outputs: &'static [(&'static str, usize, usize)],
}

/// One `name = value` of a script.
#[derive(Clone, Debug)]
pub struct Statement {
    /// What it sets: a place in [`Script::locals`].
    pub local: usize,
    /// Its value, or the values of a list.
    pub values: Vec<Node>,
    /// Where the name it sets is in the source.
    pub span: Span,
}

/// A script read and checked. Its expressions read variable `k`: an input where `k` is less
/// than the number of [`Script::vars`], else local `k - vars.len()`.
#[derive(Clone, Debug)]
pub struct Script {
    pub statements: Vec<Statement>,
    /// What it reads and never sets, in the order it first reads them: its inputs, and the
    /// built-ins it reads.
    pub vars: Vec<String>,
    /// What it sets: the outputs of its language (all of them, in their order), then its own
    /// names, in the order it first sets them.
    pub locals: Vec<String>,
    /// Which locals it sets.
    pub set: Vec<bool>,
    /// Its own names (not outputs) that it sets and never reads: most likely a misspelled
    /// output.
    pub unread: Vec<String>,
}

impl Script {
    /// Where an output of the language is among the locals.
    pub fn output(&self, name: &str) -> Option<usize> {
        self.locals.iter().position(|l| l == name)
    }

    /// Whether the script sets `name`.
    pub fn sets(&self, name: &str) -> bool {
        self.output(name).is_some_and(|l| self.set[l])
    }

    /// Run the script: `vars` the values of its [`Script::vars`], `start` those of its outputs
    /// before it sets them, `rand` what `rand(k)` gives. The values of every local: its list,
    /// or its one value (a local it never sets, its start or none).
    pub fn run(&self, vars: &[f64], start: &[Vec<f64>], rand: &dyn Fn(f64) -> f64) -> Vec<Vec<f64>> {
        let mut values: Vec<Vec<f64>> = (0..self.locals.len()).map(|l| start.get(l).cloned().unwrap_or_default()).collect();
        let mut slots: Vec<f64> = vars.to_vec();
        slots.resize(self.vars.len(), 0.0);
        slots.extend(values.iter().map(|v| v.first().copied().unwrap_or(0.0)));
        for s in &self.statements {
            let list: Vec<f64> = s.values.iter().map(|n| eval_with(n, &slots, rand)).collect();
            slots[self.vars.len() + s.local] = list[0];
            values[s.local] = list;
        }
        values
    }
}

/// Read a script of the language `rules` speaks.
pub fn compile_script(src: &str, rules: &ScriptRules) -> Result<Script, ExprError> {
    if src.len() > MAX_SCRIPT {
        return Err(ExprError {
            message: format!("The program is too long (at most {MAX_SCRIPT} characters)"),
            span: Span::new(0, src.len()),
        });
    }
    let tokens = lexer::tokenize(src);
    let mut parser = Parser::new(&tokens);
    let assignments = parser.parse_script()?;
    let names = parser.vars.clone();
    let first_read = parser.first_read.clone();

    let mut locals: Vec<String> = rules.outputs.iter().map(|(name, ..)| name.to_string()).collect();
    let mut set = vec![false; locals.len()];
    // The local each name read is, if it is one: for each name the parser met, where it
    // became one (the statement setting it first), if ever.
    let mut local_of: HashMap<&str, usize> = HashMap::new();
    for (i, name) in locals.iter().enumerate() {
        local_of.insert(name.as_str(), i);
    }
    let mut set_at: HashMap<String, usize> = HashMap::new();
    for (k, a) in assignments.iter().enumerate() {
        if let Some(message) = refused_name(&a.name, rules) {
            return Err(ExprError { message, span: a.name_span });
        }
        set_at.entry(a.name.clone()).or_insert(k);
    }
    for a in &assignments {
        if !locals.contains(&a.name) {
            locals.push(a.name.clone());
            set.push(false);
        }
    }
    let local_index = |name: &str, locals: &[String]| locals.iter().position(|l| l == name);

    // A name read before the statement that first sets it (an output is there from the
    // start: it may be read any time).
    for (i, name) in names.iter().enumerate() {
        let output = rules.outputs.iter().any(|(o, ..)| o == name);
        if output || rules.builtins.contains(&name.as_str()) {
            continue;
        }
        if let Some(&first_set) = set_at.get(name) {
            let (statement, span) = first_read[i];
            if statement <= first_set {
                return Err(ExprError { message: format!("'{name}' is read before it is given a value"), span });
            }
        }
    }

    // Inputs: what is read and is neither a local nor set; in the order first read.
    let vars: Vec<String> = names.iter().filter(|n| local_index(n, &locals).is_none()).cloned().collect();
    let slot = |name: &str| -> usize {
        match local_index(name, &locals) {
            Some(l) => vars.len() + l,
            None => vars.iter().position(|v| v == name).expect("an input"),
        }
    };
    let map: Vec<usize> = names.iter().map(|n| slot(n)).collect();

    let mut read = vec![false; locals.len()];
    let mut statements = Vec::with_capacity(assignments.len());
    for a in assignments {
        let local = local_index(&a.name, &locals).expect("a local");
        let (least, most) = rules.outputs.iter().find(|(o, ..)| *o == a.name).map_or((1, 1), |(_, l, m)| (*l, *m));
        let n = a.values.len();
        if n < least || n > most {
            let takes = match (least, most) {
                (1, 1) => "one value".to_string(),
                (l, m) if l == m => format!("{l} values"),
                (l, m) => format!("{l} to {m} values"),
            };
            let what = if rules.outputs.iter().any(|(o, ..)| *o == a.name) {
                String::new()
            } else {
                let lists: Vec<&str> = rules.outputs.iter().filter(|(_, _, m)| *m > 1).map(|(o, ..)| *o).collect();
                format!(" (lists are for {})", lists.join(" and "))
            };
            return Err(ExprError { message: format!("'{}' takes {takes}{what}", a.name), span: a.span });
        }
        let values: Vec<Node> = a.values.into_iter().map(|v| fold(remap(v, &map, &mut read, vars.len()))).collect();
        set[local] = true;
        statements.push(Statement { local, values, span: a.name_span });
    }
    let outputs = rules.outputs.len();
    let unread = (outputs..locals.len()).filter(|&l| !read[l]).map(|l| locals[l].clone()).collect();
    Ok(Script { statements, vars, locals, set, unread })
}

/// Why `name` cannot be set, if it cannot: a function, a constant, or a built-in.
fn refused_name(name: &str, rules: &ScriptRules) -> Option<String> {
    if builtins::find_function(name).is_some() {
        Some(format!("'{name}' is a function: it cannot be given a value"))
    } else if builtins::find_constant(name).is_some() {
        Some(format!("'{name}' is a constant: it cannot be given a value"))
    } else if rules.builtins.contains(&name) {
        Some(format!("'{name}' is built in: it cannot be given a value"))
    } else {
        None
    }
}

/// `node` with its variables where the script keeps them; the locals it reads marked.
fn remap(node: Node, map: &[usize], read: &mut [bool], inputs: usize) -> Node {
    match node {
        Node::Var(i) => {
            let at = map[i];
            if at >= inputs {
                read[at - inputs] = true;
            }
            Node::Var(at)
        }
        Node::Unary(op, a) => Node::Unary(op, Box::new(remap(*a, map, read, inputs))),
        Node::Binary(op, a, b) => {
            let a = remap(*a, map, read, inputs);
            Node::Binary(op, Box::new(a), Box::new(remap(*b, map, read, inputs)))
        }
        Node::Call(f, args) => Node::Call(f, args.into_iter().map(|a| remap(a, map, read, inputs)).collect()),
        Node::Cond(c, a, b) => {
            let c = remap(*c, map, read, inputs);
            let a = remap(*a, map, read, inputs);
            Node::Cond(Box::new(c), Box::new(a), Box::new(remap(*b, map, read, inputs)))
        }
        n @ Node::Num(_) => n,
    }
}

pub type CompiledScript = Arc<Result<Script, ExprError>>;

thread_local! {
    static CACHE: RefCell<HashMap<(usize, String), CompiledScript>> = RefCell::new(HashMap::new());
}

/// [`compile_script`], cached by the source and the language: cheap to call every frame.
pub fn compile_script_cached(src: &str, rules: &'static ScriptRules) -> CompiledScript {
    let key = (rules as *const ScriptRules as usize, src.to_string());
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(hit) = cache.get(&key) {
            return hit.clone();
        }
        if cache.len() > 256 {
            cache.clear();
        }
        let compiled = Arc::new(compile_script(src, rules));
        cache.insert(key, compiled.clone());
        compiled
    })
}

/// The inputs of a script (built-ins not listed). While it does not read (the user is still
/// typing) they are gathered from its words: those that are not called, not set and not the
/// language's, so that the node's sockets stay as they are.
pub fn script_vars_lenient(src: &str, rules: &'static ScriptRules) -> Vec<String> {
    let own = |name: &str| rules.builtins.contains(&name) || rules.outputs.iter().any(|(o, ..)| *o == name);
    match &*compile_script_cached(src, rules) {
        Ok(s) => s.vars.iter().filter(|v| !own(v)).cloned().collect(),
        Err(_) => {
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
                    if !called && !setting && !builtins::is_builtin(name) && !own(name) && !set.contains(&name.as_str()) && !vars.contains(name) {
                        vars.push(name.clone());
                    }
                }
            }
            vars
        }
    }
}

/// `rand(k)` of the splat, pixel or whatever else the random numbers of `seed` are of: the
/// same on the CPU as on the GPU (see `fx_rand` in [`super::wgsl::PRELUDE`]).
pub fn rand(seed: u32, k: f64) -> f64 {
    let h = builtins::pcg_hash(seed ^ builtins::pcg_hash((k as f32).to_bits()));
    f64::from(h >> 8) / 16_777_216.0
}

/// Whether `node` calls `rand` anywhere.
pub fn calls_rand(node: &Node) -> bool {
    match node {
        Node::Call(Func::Rand, _) => true,
        Node::Call(_, args) => args.iter().any(calls_rand),
        Node::Unary(_, a) => calls_rand(a),
        Node::Binary(_, a, b) => calls_rand(a) || calls_rand(b),
        Node::Cond(c, a, b) => calls_rand(c) || calls_rand(a) || calls_rand(b),
        Node::Num(_) | Node::Var(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static RULES: ScriptRules = ScriptRules { builtins: &["i", "u"], outputs: &[("x", 1, 1), ("size", 1, 2), ("color", 3, 4)] };

    fn script(src: &str) -> Script {
        compile_script(src, &RULES).unwrap_or_else(|e| panic!("{src}: {e}"))
    }

    fn error(src: &str) -> ExprError {
        match compile_script(src, &RULES) {
            Ok(_) => panic!("'{src}' should not be read"),
            Err(e) => e,
        }
    }

    fn run(src: &str, vars: &[(&str, f64)]) -> Vec<Vec<f64>> {
        let s = script(src);
        let values: Vec<f64> = s.vars.iter().map(|v| vars.iter().find(|(n, _)| n == v).map_or(0.0, |(_, x)| *x)).collect();
        s.run(&values, &[vec![0.5], vec![1.0], vec![1.0, 1.0, 1.0, 1.0]], &|k| k / 10.0)
    }

    /// Statements need nothing between them but their names: not `;`, not lines of their own;
    /// and a name followed by a value is not a product of them.
    #[test]
    fn statements_begin_where_names_are_given_values() {
        let s = script("x = 2 size = 3");
        assert_eq!(s.statements.len(), 2);
        assert_eq!(run("x = 2 size = 3", &[])[..2], [vec![2.0], vec![3.0]]);
        assert_eq!(run("x = 2\nsize = 3", &[])[..2], [vec![2.0], vec![3.0]]);
        assert_eq!(run("x = 2; size = 3;;", &[])[..2], [vec![2.0], vec![3.0]]);
        // Still a product within a value.
        assert_eq!(run("x = 2u", &[("u", 4.0)])[0], vec![8.0]);
        assert_eq!(run("x = 2(1 + 1)", &[])[0], vec![4.0]);
        assert!(error("x = 2 3").message.contains("missing operator"));
        assert!(error("x = ").message.contains("value"));
        assert!(error("x = size =").message.contains("value"));
        assert!(error("2 + 3").message.contains("name = value"));
    }

    /// What is set is read from there on; what is read and never set is an input (or a
    /// built-in); an output is there from the start, with its value.
    #[test]
    fn names_set_are_read_after() {
        let s = script("r = rand(1)\nx = r * width\nsize = x, u");
        assert_eq!(s.vars, ["width", "u"]);
        assert_eq!(s.locals, ["x", "size", "color", "r"]);
        assert_eq!(s.set, [true, true, false, true]);
        assert!(s.unread.is_empty());
        assert_eq!(run("r = rand(3)\nx = r * width\nsize = x, i", &[("width", 2.0), ("i", 7.0)])[..2], [vec![0.6], vec![0.6, 7.0]]);
        // Set again: from then on the new value.
        assert_eq!(run("x = 1; x = x + 1; size = x", &[])[1], vec![2.0]);
        // An output read before it is set is what it is at the start.
        assert_eq!(run("size = x * 2", &[])[1], vec![1.0]);
        let e = error("x = r\nr = 1");
        assert!(e.message.contains("'r' is read before"), "{e}");
        assert_eq!(e.span, Span::new(4, 5));
        assert!(error("r = r + 1").message.contains("before"));
        // Names of its own it never reads are told (most likely a misspelled output).
        assert_eq!(script("sise = 2").unread, ["sise"]);
    }

    /// Lists are for the outputs that take them, of as many values as they take.
    #[test]
    fn lists_where_they_are_taken() {
        assert_eq!(run("color = 1, 0.5, 0", &[])[2], vec![1.0, 0.5, 0.0]);
        assert_eq!(run("size = 0.2, 0.1", &[])[1], vec![0.2, 0.1]);
        assert!(error("color = 1").message.contains("'color' takes 3 to 4 values"));
        assert!(error("size = 1, 2, 3").message.contains("1 to 2 values"));
        assert!(error("x = 1, 2").message.contains("'x' takes one value"));
        assert!(error("k = 1, 2").message.contains("lists are for size and color"));
        // A list is read as its first value.
        assert_eq!(run("size = 0.3, 0.7; x = size", &[])[0], vec![0.3]);
    }

    /// What cannot be set says so.
    #[test]
    fn functions_constants_and_built_ins_are_not_set() {
        assert!(error("sin = 1").message.contains("function"));
        assert!(error("pi = 3").message.contains("constant"));
        assert!(error("i = 3").message.contains("built in"));
        assert!(error("x = foo(1)").message.contains("Unknown function"));
    }

    /// `#` begins a comment to the end of its line.
    #[test]
    fn comments() {
        assert_eq!(run("# where\nx = 3 # three\n# size = 9\n", &[])[..2], [vec![3.0], vec![1.0]]);
        let highlighted = super::super::highlight("x = 1 # one");
        assert_eq!(highlighted.last(), Some(&(Span::new(6, 11), super::super::TokenClass::Comment)));
    }

    /// While a script does not read, its inputs are still its words that are read and not set
    /// (so the sockets stay).
    #[test]
    fn inputs_while_typing() {
        static R: &ScriptRules = &RULES;
        assert_eq!(script_vars_lenient("x = mask * 2\nsize = height", R), ["mask", "height"]);
        assert_eq!(script_vars_lenient("x = mask * \nk = 1\nsize = k + height +", R), ["mask", "height"]);
        assert_eq!(script_vars_lenient("x = u + i", R), Vec::<String>::new());
    }

    /// `rand(k)` is never worked out ahead: it is different for each splat.
    #[test]
    fn rand_is_never_folded() {
        let s = script("x = rand(2) * 3");
        assert!(calls_rand(&s.statements[0].values[0]));
        let a = rand(5, 1.0);
        assert!((0.0..1.0).contains(&a));
        assert_ne!(a, rand(6, 1.0));
        assert_ne!(a, rand(5, 2.0));
        assert_eq!(a, rand(5, 1.0));
    }

    #[test]
    fn fuzz_never_panics() {
        let alphabet: Vec<char> = "0123456789.xyzsize = rand()+-*/;#\n,iu?:".chars().collect();
        let mut state = 0x9e37_79b9u32;
        for _ in 0..20_000 {
            let len = (builtins::pcg_hash(state) % 32) as usize;
            let mut s = String::new();
            for _ in 0..len {
                state = builtins::pcg_hash(state);
                s.push(alphabet[state as usize % alphabet.len()]);
            }
            state = state.wrapping_add(1);
            if let Ok(script) = compile_script(&s, &RULES) {
                let _ = script.run(&vec![0.5; script.vars.len()], &[], &|k| k);
            }
            let _ = super::super::highlight(&s);
        }
    }
}
