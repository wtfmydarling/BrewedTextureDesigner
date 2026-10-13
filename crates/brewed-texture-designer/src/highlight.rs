//! Syntax highlighting of expressions in the `text_editor` of the Math nodes.

use std::ops::Range;

use brewed_texture_core::expr::{self, TokenClass};
use iced::advanced::text::highlighter::{self, Highlighter};
use iced::{Color, Font};

use crate::style;

/// The error to underline: line index and byte range in that line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    pub error: Option<(usize, Range<usize>)>,
}

impl Settings {
    /// Convert an error span in the whole source into a (line, range) pair.
    pub fn for_error(source: &str, span: Option<expr::Span>) -> Settings {
        let Some(span) = span else { return Settings::default() };
        let mut line_start = 0;
        for (i, line) in source.split('\n').enumerate() {
            let line_end = line_start + line.len();
            if span.start <= line_end {
                let start = span.start.saturating_sub(line_start).min(line.len());
                let end = span.end.saturating_sub(line_start).clamp(start, line.len());
                return Settings { error: Some((i, start..end)) };
            }
            line_start = line_end + 1;
        }
        Settings::default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Highlight {
    Token(TokenClass),
    Error,
}

impl Highlight {
    pub fn color(self) -> Color {
        match self {
            Highlight::Token(TokenClass::Number) => style::rgb(181, 206, 168),
            Highlight::Token(TokenClass::Function) => style::rgb(220, 220, 170),
            Highlight::Token(TokenClass::Constant) => style::rgb(86, 182, 194),
            Highlight::Token(TokenClass::Variable) => style::rgb(156, 220, 254),
            Highlight::Token(TokenClass::Operator) => style::rgb(212, 212, 212),
            Highlight::Token(TokenClass::Paren) => style::rgb(200, 160, 220),
            Highlight::Token(TokenClass::Comment) => style::rgb(128, 128, 128),
            Highlight::Token(TokenClass::Error) | Highlight::Error => style::rgb(255, 96, 86),
        }
    }

    pub fn to_format(self, _theme: &iced::Theme) -> highlighter::Format<Font> {
        highlighter::Format { color: Some(self.color()), font: None }
    }
}

pub struct ExpressionHighlighter {
    settings: Settings,
    line: usize,
}

impl Highlighter for ExpressionHighlighter {
    type Settings = Settings;
    type Highlight = Highlight;
    type Iterator<'a> = std::vec::IntoIter<(Range<usize>, Highlight)>;

    fn new(settings: &Settings) -> Self {
        ExpressionHighlighter { settings: settings.clone(), line: 0 }
    }

    fn update(&mut self, settings: &Settings) {
        self.settings = settings.clone();
        self.line = 0;
    }

    fn change_line(&mut self, line: usize) {
        self.line = line;
    }

    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        let error = self.settings.error.as_ref().filter(|(l, _)| *l == self.line).map(|(_, r)| r.clone());
        self.line += 1;
        let tokens = expr::highlight(line);
        let mut out = Vec::with_capacity(tokens.len());
        for (i, (span, class)) in tokens.iter().enumerate() {
            let range = span.start..span.end.min(line.len());
            let in_error = error.as_ref().is_some_and(|e| {
                if e.start == e.end {
                    // An empty error range points between tokens: mark the token before it.
                    range.end <= e.start && tokens.get(i + 1).is_none_or(|(next, _)| next.start >= e.start)
                } else {
                    range.start < e.end && e.start < range.end
                }
            });
            out.push((range, if in_error { Highlight::Error } else { Highlight::Token(*class) }));
        }
        out.into_iter()
    }

    fn current_line(&self) -> usize {
        self.line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_positions() {
        let s = Settings::for_error("a + (b", Some(expr::Span::new(6, 6)));
        assert_eq!(s.error, Some((0, 6..6)));
        let s = Settings::for_error("a +\nsin(", Some(expr::Span::new(4, 7)));
        assert_eq!(s.error, Some((1, 0..3)));
    }

    #[test]
    fn marks_the_offending_token() {
        let settings = Settings::for_error("1 + foo(2)", Some(expr::Span::new(4, 7)));
        let mut h = ExpressionHighlighter::new(&settings);
        let spans: Vec<_> = h.highlight_line("1 + foo(2)").collect();
        assert!(spans.iter().any(|(r, hl)| *r == (4..7) && *hl == Highlight::Error));
        assert!(spans.iter().any(|(_, hl)| *hl == Highlight::Token(TokenClass::Number)));
    }
}
