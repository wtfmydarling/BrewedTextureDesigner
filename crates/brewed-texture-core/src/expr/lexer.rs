//! Tokenizer for the scalar expression language used by the Math nodes.
//!
//! The lexer never fails: characters it does not understand become
//! [`TokenKind::Error`] tokens. The parser turns those into readable errors,
//! and the syntax highlighter can still color everything else.

use super::Span;

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Number(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    LParen,
    RParen,
    Comma,
    Less,
    LessEq,
    Greater,
    GreaterEq,
    EqEq,
    NotEq,
    AndAnd,
    OrOr,
    Bang,
    Question,
    Colon,
    /// `=`: gives a name a value in a script (see [`super::script`]); in an expression, a
    /// mistake for `==`.
    Assign,
    /// `;`: between the statements of a script.
    Semicolon,
    /// Something the language does not understand, with a human readable reason.
    Error(String),
    Eof,
}

impl TokenKind {
    /// Short description used in error messages.
    pub fn describe(&self) -> String {
        match self {
            TokenKind::Number(v) => format!("number {v}"),
            TokenKind::Ident(name) => format!("'{name}'"),
            TokenKind::Plus => "'+'".into(),
            TokenKind::Minus => "'-'".into(),
            TokenKind::Star => "'*'".into(),
            TokenKind::Slash => "'/'".into(),
            TokenKind::Percent => "'%'".into(),
            TokenKind::Caret => "'^'".into(),
            TokenKind::LParen => "'('".into(),
            TokenKind::RParen => "')'".into(),
            TokenKind::Comma => "','".into(),
            TokenKind::Less => "'<'".into(),
            TokenKind::LessEq => "'<='".into(),
            TokenKind::Greater => "'>'".into(),
            TokenKind::GreaterEq => "'>='".into(),
            TokenKind::EqEq => "'=='".into(),
            TokenKind::NotEq => "'!='".into(),
            TokenKind::AndAnd => "'&&'".into(),
            TokenKind::OrOr => "'||'".into(),
            TokenKind::Bang => "'!'".into(),
            TokenKind::Question => "'?'".into(),
            TokenKind::Colon => "':'".into(),
            TokenKind::Assign => "'=' (to compare values, write '==')".into(),
            TokenKind::Semicolon => "';'".into(),
            TokenKind::Error(_) => "invalid input".into(),
            TokenKind::Eof => "end of expression".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Splits `src` into tokens. Always ends with an [`TokenKind::Eof`] token.
pub fn tokenize(src: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let byte_at = |i: usize| chars.get(i).map_or(src.len(), |(b, _)| *b);
    let char_at = |i: usize| chars.get(i).map(|(_, c)| *c);

    let mut i = 0;
    while i < chars.len() {
        let c = chars[i].1;
        let start = byte_at(i);

        if c.is_whitespace() {
            i += 1;
            continue;
        }

        // A comment, to the end of the line.
        if c == '#' {
            while char_at(i).is_some_and(|c| c != '\n') {
                i += 1;
            }
            continue;
        }

        // Numbers: 12, 1.5, .5, 1., 2e-3
        if c.is_ascii_digit() || (c == '.' && char_at(i + 1).is_some_and(|n| n.is_ascii_digit())) {
            let mut j = i;
            while char_at(j).is_some_and(|d| d.is_ascii_digit()) {
                j += 1;
            }
            if char_at(j) == Some('.') {
                j += 1;
                while char_at(j).is_some_and(|d| d.is_ascii_digit()) {
                    j += 1;
                }
            }
            // Exponent only if it is followed by digits, so `2e` lexes as `2` `e`
            // (which the parser reads as the implicit product 2·e).
            if matches!(char_at(j), Some('e' | 'E')) {
                let mut k = j + 1;
                if matches!(char_at(k), Some('+' | '-')) {
                    k += 1;
                }
                if char_at(k).is_some_and(|d| d.is_ascii_digit()) {
                    while char_at(k).is_some_and(|d| d.is_ascii_digit()) {
                        k += 1;
                    }
                    j = k;
                }
            }
            let end = byte_at(j);
            let text = &src[start..end];
            let kind = match text.parse::<f64>() {
                Ok(v) => TokenKind::Number(v),
                Err(_) => TokenKind::Error(format!("Invalid number '{text}'")),
            };
            tokens.push(Token { kind, span: Span::new(start, end) });
            // A second dot right after a number ("1.2.3") is a typo worth naming.
            if char_at(j) == Some('.') {
                let s = byte_at(j);
                tokens.push(Token {
                    kind: TokenKind::Error("Unexpected '.' (a number can only have one decimal point)".into()),
                    span: Span::new(s, byte_at(j + 1)),
                });
                j += 1;
            }
            i = j;
            continue;
        }

        if is_ident_start(c) {
            let mut j = i + 1;
            while char_at(j).is_some_and(is_ident_continue) {
                j += 1;
            }
            let end = byte_at(j);
            tokens.push(Token { kind: TokenKind::Ident(src[start..end].to_string()), span: Span::new(start, end) });
            i = j;
            continue;
        }

        let next = char_at(i + 1);
        let (kind, len) = match (c, next) {
            ('*', Some('*')) => (TokenKind::Caret, 2), // python style power
            ('<', Some('=')) => (TokenKind::LessEq, 2),
            ('>', Some('=')) => (TokenKind::GreaterEq, 2),
            ('=', Some('=')) => (TokenKind::EqEq, 2),
            ('!', Some('=')) => (TokenKind::NotEq, 2),
            ('&', Some('&')) => (TokenKind::AndAnd, 2),
            ('|', Some('|')) => (TokenKind::OrOr, 2),
            ('+', _) => (TokenKind::Plus, 1),
            ('-' | '−', _) => (TokenKind::Minus, 1),
            ('*' | '×' | '·', _) => (TokenKind::Star, 1),
            ('/' | '÷', _) => (TokenKind::Slash, 1),
            ('%', _) => (TokenKind::Percent, 1),
            ('^', _) => (TokenKind::Caret, 1),
            ('(', _) => (TokenKind::LParen, 1),
            (')', _) => (TokenKind::RParen, 1),
            (',', _) => (TokenKind::Comma, 1),
            ('<', _) => (TokenKind::Less, 1),
            ('>', _) => (TokenKind::Greater, 1),
            ('!', _) => (TokenKind::Bang, 1),
            ('?', _) => (TokenKind::Question, 1),
            (':', _) => (TokenKind::Colon, 1),
            ('π', _) => (TokenKind::Ident("pi".into()), 1),
            ('τ', _) => (TokenKind::Ident("tau".into()), 1),
            ('=', _) => (TokenKind::Assign, 1),
            (';', _) => (TokenKind::Semicolon, 1),
            ('&', _) => (TokenKind::Error("Unexpected '&' (use '&&' for logical AND)".into()), 1),
            ('|', _) => (TokenKind::Error("Unexpected '|' (use '||' for logical OR)".into()), 1),
            ('[' | '{', _) => (TokenKind::Error(format!("Unexpected '{c}' (use '(' for grouping)")), 1),
            (']' | '}', _) => (TokenKind::Error(format!("Unexpected '{c}' (use ')' for grouping)")), 1),
            _ => (TokenKind::Error(format!("Unexpected character '{c}'")), 1),
        };
        let end = byte_at(i + len);
        tokens.push(Token { kind, span: Span::new(start, end) });
        i += len;
    }

    tokens.push(Token { kind: TokenKind::Eof, span: Span::new(src.len(), src.len()) });
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokenKind> {
        tokenize(src).into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn numbers() {
        assert_eq!(kinds("1.5"), vec![TokenKind::Number(1.5), TokenKind::Eof]);
        assert_eq!(kinds(".5"), vec![TokenKind::Number(0.5), TokenKind::Eof]);
        assert_eq!(kinds("2e-3"), vec![TokenKind::Number(2e-3), TokenKind::Eof]);
        assert_eq!(kinds("2e"), vec![TokenKind::Number(2.0), TokenKind::Ident("e".into()), TokenKind::Eof]);
    }

    #[test]
    fn operators_and_unicode() {
        assert_eq!(
            kinds("a**2 × π"),
            vec![
                TokenKind::Ident("a".into()),
                TokenKind::Caret,
                TokenKind::Number(2.0),
                TokenKind::Star,
                TokenKind::Ident("pi".into()),
                TokenKind::Eof
            ]
        );
        let toks = tokenize("x @ y");
        assert!(matches!(toks[1].kind, TokenKind::Error(_)));
        assert_eq!(toks[1].span, Span::new(2, 3));
    }

    #[test]
    fn double_dot() {
        let toks = tokenize("1.2.3");
        assert_eq!(toks[0].kind, TokenKind::Number(1.2));
        assert!(matches!(toks[1].kind, TokenKind::Error(_)));
    }
}
