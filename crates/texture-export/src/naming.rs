//! The names a template gives its files: text, and parts that stand for what is exported,
//! written `$(graph)` as Substance Designer writes them. Which parts there are is the
//! program's to say (a graph's and a package's here; a program of texture sets would have its
//! mesh and its sets); this says how they are written and filled in.

/// A part of a file's name that stands for something: its key, as it is written between
/// `$(` and `)`, and what it is, said to whoever writes a name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub key: &'static str,
    pub about: &'static str,
}

impl Token {
    /// As it is written in a name: `$(graph)`.
    pub fn written(self) -> String {
        format!("$({})", self.key)
    }
}

/// The parts of a name a graph's export fills in.
pub const GRAPH_TOKENS: [Token; 2] = [
    Token { key: "graph", about: "The graph's name, as a file's (blanks and what a file name cannot have as _)" },
    Token { key: "package", about: "The package's name, as a file's" },
];

/// `pattern` with each `$(key)` of `values` written as its value, made a file's (see
/// [`crate::file_stem`]). A part of no value given stays as it is written.
pub fn expand(pattern: &str, values: &[(&str, &str)]) -> String {
    let mut name = pattern.to_string();
    for (key, value) in values {
        name = name.replace(&format!("$({key})"), &crate::file_stem(value));
    }
    name
}

/// The parts of `pattern` that are not among `tokens`, as they are written: `$(mesh)` where a
/// graph is exported.
pub fn unknown_parts(pattern: &str, tokens: &[Token]) -> Vec<String> {
    let mut unknown = Vec::new();
    let mut rest = pattern;
    while let Some(at) = rest.find("$(") {
        let after = &rest[at + 2..];
        let Some(end) = after.find(')') else { break };
        let key = &after[..end];
        if !tokens.iter().any(|t| t.key == key) {
            unknown.push(format!("$({key})"));
        }
        rest = &after[end + 1..];
    }
    unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_filled_in() {
        let values = [("graph", "Rusty Metal"), ("package", "Walls")];
        assert_eq!(expand("T_$(graph)_BaseColor", &values), "T_Rusty_Metal_BaseColor");
        assert_eq!(expand("$(package)/$(graph)", &values), "Walls/Rusty_Metal");
        assert_eq!(expand("$(mesh)_x", &values), "$(mesh)_x", "a part of no value stays");
        assert_eq!(unknown_parts("$(graph)_$(mesh)_$(udim", &GRAPH_TOKENS), ["$(mesh)"]);
        assert_eq!(GRAPH_TOKENS[0].written(), "$(graph)");
    }
}
