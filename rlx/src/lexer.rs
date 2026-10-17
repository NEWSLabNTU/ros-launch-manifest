//! Tokens, and the identifier and symbol half of the character layer.
//!
//! Newlines are not tokens: an expression may continue on the next line
//! (the rail example starts a line with `+`), and items and clauses begin
//! with reserved words, which is what ends the expression before them.
//! Comments run from `--` to the end of the line and are not tokens either;
//! the formatter works on the source text, so they survive formatting
//! without being carried through the parser.
//!
//! Every Unicode operator lexes to the same token as its ASCII twin, with
//! `unicode` set so the formatter knows the source spelling. `²` and `³` lex
//! to two tokens, `^` and the digit, the digit with an empty span at the end
//! of the superscript (it has no text of its own).

use crate::{
    chars,
    diag::{Diagnostic, Span, code, code_point},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kw {
    Extension,
    Requires,
    Key,
    Input,
    Derive,
    Check,
    Report,
    Fn,
    Dimension,
    Where,
    And,
    Or,
    Not,
    If,
    Then,
    Else,
    Sum,
    Over,
    In,
    WorstCase,
    Domain,
    Increasing,
    Decreasing,
    Param,
    Table,
}

impl Kw {
    pub const ALL: &[(&'static str, Kw)] = &[
        ("extension", Kw::Extension),
        ("requires", Kw::Requires),
        ("key", Kw::Key),
        ("input", Kw::Input),
        ("derive", Kw::Derive),
        ("check", Kw::Check),
        ("report", Kw::Report),
        ("fn", Kw::Fn),
        ("dimension", Kw::Dimension),
        ("where", Kw::Where),
        ("and", Kw::And),
        ("or", Kw::Or),
        ("not", Kw::Not),
        ("if", Kw::If),
        ("then", Kw::Then),
        ("else", Kw::Else),
        ("sum", Kw::Sum),
        ("over", Kw::Over),
        ("in", Kw::In),
        ("worst_case", Kw::WorstCase),
        ("domain", Kw::Domain),
        ("increasing", Kw::Increasing),
        ("decreasing", Kw::Decreasing),
        ("param", Kw::Param),
        ("table", Kw::Table),
    ];

    pub fn lookup(s: &str) -> Option<Kw> {
        Self::ALL.iter().find(|(n, _)| *n == s).map(|(_, k)| *k)
    }

    pub fn as_str(self) -> &'static str {
        Self::ALL.iter().find(|(_, k)| *k == self).unwrap().0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    Kw(Kw),
    /// A decimal number, as written.
    Number(String),
    /// `X.Y.Z`, as written.
    Version(String),
    /// A string's value, escapes resolved.
    Str(String),
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Semi,
    Dot,
    Eq,
    EqEq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    Arrow,
    Eof,
}

impl Tok {
    /// How a message names this token.
    pub fn describe(&self) -> String {
        match self {
            Tok::Ident(s) => format!("identifier `{s}`"),
            Tok::Kw(k) => format!("`{}`", k.as_str()),
            Tok::Number(s) => format!("number `{s}`"),
            Tok::Version(s) => format!("version `{s}`"),
            Tok::Str(_) => "a string".to_string(),
            Tok::Eof => "end of file".to_string(),
            other => format!("`{}`", other.ascii()),
        }
    }

    /// The ASCII spelling of a punctuation token.
    pub fn ascii(&self) -> &'static str {
        match self {
            Tok::LParen => "(",
            Tok::RParen => ")",
            Tok::LBracket => "[",
            Tok::RBracket => "]",
            Tok::Comma => ",",
            Tok::Colon => ":",
            Tok::Semi => ";",
            Tok::Dot => ".",
            Tok::Eq => "=",
            Tok::EqEq => "==",
            Tok::Ne => "!=",
            Tok::Lt => "<",
            Tok::Le => "<=",
            Tok::Gt => ">",
            Tok::Ge => ">=",
            Tok::Plus => "+",
            Tok::Minus => "-",
            Tok::Star => "*",
            Tok::Slash => "/",
            Tok::Caret => "^",
            Tok::Arrow => "->",
            _ => "",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
    /// Written as the Unicode twin (`≤`, `√`, `²`, ...).
    pub unicode: bool,
}

/// Tokenize text that passed [`chars::prepass`]. Identifier, symbol and
/// lexical errors are collected; the caller does not parse when there are
/// any, because a refused character is the finding and a parse error after
/// it would only be noise.
pub fn lex(src: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    let mut lx = Lexer {
        src,
        pos: 0,
        out: Vec::new(),
        diags: Vec::new(),
    };
    lx.run();
    let end = src.len();
    lx.out.push(Token {
        tok: Tok::Eof,
        span: Span::new(end, end),
        unicode: false,
    });
    (lx.out, lx.diags)
}

struct Lexer<'a> {
    src: &'a str,
    pos: usize,
    out: Vec<Token>,
    diags: Vec<Diagnostic>,
}

impl Lexer<'_> {
    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn peek2(&self) -> Option<char> {
        let mut it = self.src[self.pos..].chars();
        it.next();
        it.next()
    }

    fn push(&mut self, tok: Tok, start: usize, unicode: bool) {
        self.out.push(Token {
            tok,
            span: Span::new(start, self.pos),
            unicode,
        });
    }

    fn run(&mut self) {
        while let Some(c) = self.peek() {
            let start = self.pos;
            if c == ' ' || c == '\t' || c == '\n' {
                self.pos += 1;
                continue;
            }
            if c == '-' && self.peek2() == Some('-') {
                while let Some(c) = self.peek() {
                    if c == '\n' {
                        break;
                    }
                    self.pos += c.len_utf8();
                }
                continue;
            }
            if c == '"' {
                self.string();
                continue;
            }
            if c.is_ascii_digit() {
                self.number();
                continue;
            }
            if chars::is_ident_start(c) {
                self.ident();
                continue;
            }
            self.pos += c.len_utf8();
            let two = |lx: &mut Self, next: char, long: Tok, short: Tok| {
                if lx.peek() == Some(next) {
                    lx.pos += 1;
                    long
                } else {
                    short
                }
            };
            let tok = match c {
                '(' => Tok::LParen,
                ')' => Tok::RParen,
                '[' => Tok::LBracket,
                ']' => Tok::RBracket,
                ',' => Tok::Comma,
                ':' => Tok::Colon,
                ';' => Tok::Semi,
                '.' => Tok::Dot,
                '+' => Tok::Plus,
                '*' => Tok::Star,
                '/' => Tok::Slash,
                '^' => Tok::Caret,
                '-' => two(self, '>', Tok::Arrow, Tok::Minus),
                '<' => two(self, '=', Tok::Le, Tok::Lt),
                '>' => two(self, '=', Tok::Ge, Tok::Gt),
                '=' => two(self, '=', Tok::EqEq, Tok::Eq),
                '!' if self.peek() == Some('=') => {
                    self.pos += 1;
                    Tok::Ne
                }
                _ => {
                    if let Some(t) = self.unicode_twin(c, start) {
                        if let Some(t) = t {
                            self.push(t, start, true);
                        }
                        continue;
                    }
                    let span = Span::new(start, self.pos);
                    let d = if c.is_ascii() {
                        Diagnostic::new(code::PARSE, span, format!("unexpected character `{c}`"))
                    } else if c.is_whitespace() {
                        Diagnostic::new(
                            code::SYMBOL,
                            span,
                            format!(
                                "non-ASCII space {} outside a comment or string; use an ASCII \
                                 space or tab",
                                code_point(c)
                            ),
                        )
                    } else {
                        let hint = chars::symbol_hint(c)
                            .map(|h| format!("; {h}"))
                            .unwrap_or_default();
                        Diagnostic::new(
                            code::SYMBOL,
                            span,
                            format!(
                                "non-ASCII symbol `{c}` ({}) has no meaning in rlx outside a \
                                 comment or string; the only Unicode operators are the twins \
                                 → ≤ ≥ ≠ · ÷ − √ ² ³ ∈ Σ{hint}",
                                code_point(c)
                            ),
                        )
                    };
                    self.diags.push(d);
                    continue;
                }
            };
            self.push(tok, start, false);
        }
    }

    /// `Some(Some(tok))` for a twin that is one token, `Some(None)` for one
    /// this function pushed itself (or refused), `None` for no twin.
    fn unicode_twin(&mut self, c: char, start: usize) -> Option<Option<Tok>> {
        Some(Some(match c {
            '→' => Tok::Arrow,
            '≤' => Tok::Le,
            '≥' => Tok::Ge,
            '≠' => Tok::Ne,
            '·' => Tok::Star,
            '÷' => Tok::Slash,
            '−' => Tok::Minus,
            '∈' => Tok::Kw(Kw::In),
            'Σ' => Tok::Kw(Kw::Sum),
            '√' => {
                // `√` is the twin of the function name `sqrt`, so it must be
                // followed by the call's parenthesis. Allowing `√x` would make
                // `√` an operator with a precedence of its own, and its ASCII
                // twin would no longer be a token-for-token rewrite.
                if self.peek() != Some('(') {
                    self.diags.push(Diagnostic::new(
                        code::PARSE,
                        Span::new(start, self.pos),
                        "`√` is the twin of `sqrt` and takes a parenthesized argument \
                         directly after it: write `√(x)`",
                    ));
                    return Some(None);
                }
                Tok::Ident("sqrt".to_string())
            }
            '²' | '³' => {
                self.push(Tok::Caret, start, true);
                let digit = if c == '²' { "2" } else { "3" };
                self.out.push(Token {
                    tok: Tok::Number(digit.to_string()),
                    span: Span::new(self.pos, self.pos),
                    unicode: true,
                });
                return Some(None);
            }
            _ => return None,
        }))
    }

    fn string(&mut self) {
        let start = self.pos;
        self.pos += 1;
        let mut value = String::new();
        loop {
            match self.peek() {
                None | Some('\n') => {
                    self.diags.push(Diagnostic::new(
                        code::PARSE,
                        Span::new(start, start + 1),
                        "unterminated string; a string ends with `\"` on the same line",
                    ));
                    return;
                }
                Some('"') => {
                    self.pos += 1;
                    break;
                }
                Some('\\') => {
                    let esc_start = self.pos;
                    self.pos += 1;
                    match self.peek() {
                        Some(e @ ('"' | '\\')) => {
                            value.push(e);
                            self.pos += 1;
                        }
                        other => {
                            if let Some(o) = other {
                                self.pos += o.len_utf8();
                            }
                            self.diags.push(Diagnostic::new(
                                code::PARSE,
                                Span::new(esc_start, self.pos),
                                "unknown escape; a string knows only `\\\"` and `\\\\`",
                            ));
                        }
                    }
                }
                Some(c) => {
                    value.push(c);
                    self.pos += c.len_utf8();
                }
            }
        }
        self.push(Tok::Str(value), start, false);
    }

    fn digits(&mut self) {
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
        }
    }

    /// `123`, `1.5`, `2e-3`, or a version `1.0.0`. A version is lexed here
    /// because `1.0.0` would otherwise be the number `1.0` followed by `.0`.
    fn number(&mut self) {
        let start = self.pos;
        self.digits();
        let after_dot_digit =
            |lx: &Self| lx.peek() == Some('.') && lx.peek2().is_some_and(|c| c.is_ascii_digit());
        if after_dot_digit(self) {
            self.pos += 1;
            self.digits();
            if after_dot_digit(self) {
                self.pos += 1;
                self.digits();
                let text = self.src[start..self.pos].to_string();
                self.push(Tok::Version(text), start, false);
                return;
            }
        }
        // An exponent only when a digit follows, so `2e` stays `2` then `e`.
        if matches!(self.peek(), Some('e' | 'E')) {
            let save = self.pos;
            self.pos += 1;
            if matches!(self.peek(), Some('+' | '-')) {
                self.pos += 1;
            }
            if self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.digits();
            } else {
                self.pos = save;
            }
        }
        let text = self.src[start..self.pos].to_string();
        self.push(Tok::Number(text), start, false);
    }

    fn ident(&mut self) {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if !chars::is_ident_continue(c) {
                break;
            }
            self.pos += c.len_utf8();
        }
        let text = &self.src[start..self.pos];
        if let Some(d) = chars::check_ident(text, start) {
            self.diags.push(d);
        }
        let tok = match Kw::lookup(text) {
            Some(k) => Tok::Kw(k),
            None => Tok::Ident(text.to_string()),
        };
        self.push(tok, start, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<Tok> {
        let (t, d) = lex(src);
        assert!(d.is_empty(), "{d:?}");
        t.into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn versions_numbers_paths() {
        assert_eq!(
            kinds("1.0.0 0.75 b.hi 2e-3 2e"),
            vec![
                Tok::Version("1.0.0".into()),
                Tok::Number("0.75".into()),
                Tok::Ident("b".into()),
                Tok::Dot,
                Tok::Ident("hi".into()),
                Tok::Number("2e-3".into()),
                Tok::Number("2".into()),
                Tok::Ident("e".into()),
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn twins_lex_alike() {
        assert_eq!(kinds("a ≤ b → c²"), kinds("a <= b -> c^2"));
        assert_eq!(
            kinds("√(x) Σ ∈ − · ÷ ≠ ≥"),
            kinds("sqrt(x) sum in - * / != >=")
        );
    }

    #[test]
    fn comments_and_strings() {
        assert_eq!(
            kinds("a -- ≤ ignored\n\"x \\\" y\""),
            vec![Tok::Ident("a".into()), Tok::Str("x \" y".into()), Tok::Eof]
        );
    }

    #[test]
    fn refused_symbol() {
        let (_, d) = lex("a × b");
        assert_eq!(d[0].code, code::SYMBOL);
        assert!(d[0].message.contains("write `*`"));
    }
}
