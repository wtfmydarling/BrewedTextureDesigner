//! The names a template gives its files: text, and parts that stand for what is exported,
//! written `$(graph)` as Substance Designer writes them. Which parts there are, and what each
//! stands for, is the program's to say (a texture designer's graph and package, a texture
//! painter's mesh and texture set); this says how they are written and filled in. A part in
//! brackets is left out where it stands for nothing, with what else is in the brackets, as
//! Substance Painter leaves them out: `T_$(graph)(_$(package))`.

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

/// What a name is made of: text, parts, and the brackets around a part left out where it
/// stands for nothing.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Piece<'p> {
    Text(&'p str),
    Part(&'p str),
    Open,
    Close,
}

fn pieces(pattern: &str) -> Vec<Piece<'_>> {
    let mut pieces = Vec::new();
    let mut rest = pattern;
    while let Some(c) = rest.chars().next() {
        if let Some(after) = rest.strip_prefix("$(")
            && let Some(end) = after.find(')')
        {
            pieces.push(Piece::Part(&after[..end]));
            rest = &after[end + 1..];
            continue;
        }
        let length = c.len_utf8();
        pieces.push(match c {
            '(' => Piece::Open,
            ')' => Piece::Close,
            _ => Piece::Text(&rest[..length]),
        });
        rest = &rest[length..];
    }
    pieces
}

/// `pattern` with each `$(key)` of `values` written as its value, made a file's (see
/// [`crate::file_stem`]); of an empty value, as nothing. A part of no value given stays as it
/// is written. Brackets with a part in them are left out, with all in them, where a part in
/// them stands for nothing (of no value, or an empty one), and are not written where they are
/// not; brackets of no part, or not closed, are text.
pub fn expand(pattern: &str, values: &[(&str, &str)]) -> String {
    let value = |key: &str| values.iter().find(|(k, _)| *k == key).map(|(_, v)| if v.trim().is_empty() { String::new() } else { crate::file_stem(v) });
    let pieces = pieces(pattern);
    let mut name = String::new();
    let mut i = 0;
    while i < pieces.len() {
        match pieces[i] {
            Piece::Open => {
                // Up to the bracket that closes it, where no other is opened before.
                let close = pieces[i + 1..].iter().position(|p| matches!(p, Piece::Open | Piece::Close)).map(|n| i + 1 + n);
                if let Some(j) = close.filter(|&j| pieces[j] == Piece::Close && pieces[i + 1..j].iter().any(|p| matches!(p, Piece::Part(_)))) {
                    let filled: Option<String> = pieces[i + 1..j]
                        .iter()
                        .map(|p| match p {
                            Piece::Text(text) => Some(text.to_string()),
                            Piece::Part(key) => value(key).filter(|v| !v.is_empty()),
                            Piece::Open | Piece::Close => None,
                        })
                        .collect();
                    name.push_str(&filled.unwrap_or_default());
                    i = j + 1;
                    continue;
                }
                name.push('(');
            }
            Piece::Close => name.push(')'),
            Piece::Text(text) => name.push_str(text),
            Piece::Part(key) => name.push_str(&value(key).unwrap_or_else(|| format!("$({key})"))),
        }
        i += 1;
    }
    name
}

/// The parts of `pattern` that are not among `tokens`, as they are written: `$(mesh)` where a
/// graph is exported.
pub fn unknown_parts(pattern: &str, tokens: &[Token]) -> Vec<String> {
    let mut unknown: Vec<String> = Vec::new();
    for piece in pieces(pattern) {
        if let Piece::Part(key) = piece
            && !tokens.iter().any(|t| t.key == key)
        {
            let written = format!("$({key})");
            if !unknown.contains(&written) {
                unknown.push(written);
            }
        }
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
        let tokens = [Token { key: "graph", about: "" }, Token { key: "package", about: "" }];
        assert_eq!(unknown_parts("$(graph)_$(mesh)_$(udim)_$(mesh)_$(x", &tokens), ["$(mesh)", "$(udim)"]);
        assert_eq!(tokens[0].written(), "$(graph)");
    }

    /// A part in brackets, and what else is in them, is left out where it stands for nothing:
    /// no `T__BaseColor` where there is no package. Brackets of no part are text.
    #[test]
    fn parts_in_brackets_are_left_out_where_they_stand_for_nothing() {
        let named = [("graph", "Wall"), ("package", "Walls")];
        let unnamed = [("graph", "Wall"), ("package", " ")];
        assert_eq!(expand("T_$(graph)(_$(package))_BaseColor", &named), "T_Wall_Walls_BaseColor");
        assert_eq!(expand("T_$(graph)(_$(package))_BaseColor", &unnamed), "T_Wall_BaseColor");
        assert_eq!(expand("T_$(package)_$(graph)", &unnamed), "T__Wall", "out of brackets, an empty part is nothing");
        assert_eq!(expand("$(graph)(_$(mesh))(.$(udim))", &named), "Wall", "parts of no value given");
        assert_eq!(expand("($(package)_$(mesh))$(graph)", &named), "Wall", "one of them stands for nothing: all left out");
        assert_eq!(expand("T_(2)_$(graph)", &named), "T_(2)_Wall", "brackets of no part");
        assert_eq!(expand("$(graph)(_$(package)", &named), "Wall(_Walls", "a bracket not closed");
        assert_eq!(expand("$(graph))(", &named), "Wall)(");
    }
}
