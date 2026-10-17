//! `rlx fmt --ascii | --unicode`: rewriting operator spellings in place.
//!
//! The formatter rewrites tokens, not layout. The examples align key types
//! and doc strings in columns and break long formulas by hand where they
//! read best; a pretty-printer would either have to reproduce every such
//! choice or overwrite them, and the reviewer's diff would be layout noise.
//! So the canonical form is defined as:
//!
//! - every operator, keyword and `sqrt` in its ASCII spelling;
//! - no trailing spaces or tabs on any line;
//! - exactly one line feed at the end of the file.
//!
//! Comments and strings are left exactly as written in both directions:
//! they are prose, and prose may quote a `≤`. Identifiers are left too: a
//! Greek identifier has no ASCII twin, so "the canonical form is ASCII"
//! holds for everything except the letters an author chose (see the design
//! note in the crate documentation).
//!
//! `--unicode` renders each twin: `→ ≤ ≥ ≠ · ÷ − ² ³ ∈ Σ √`, with three
//! restrictions that keep it readable and keep `--ascii` its exact inverse:
//! operators inside a unit or dimension keep ASCII (`km/h`, not `km÷h`),
//! `in` becomes `∈` only where it means membership (a domain interval or a
//! `sum over` binder, not `increasing in`), and `^2`/`^3` become `²`/`³`
//! only when written with no space between.

use crate::{
    diag::Span,
    lexer::{Kw, Tok, Token},
    parser::Role,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Ascii,
    Unicode,
}

/// Rewrite `src`, whose tokens and roles are given, in `style`.
pub fn format(src: &str, tokens: &[Token], roles: &[Role], style: Style) -> String {
    let mut edits: Vec<(Span, String)> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        i += match style {
            Style::Ascii => ascii_edit(src, tokens, i, &mut edits),
            Style::Unicode => unicode_edit(tokens, roles, i, &mut edits),
        };
    }
    let mut out = String::with_capacity(src.len());
    let mut at = 0;
    for (span, text) in edits {
        out.push_str(&src[at..span.start]);
        out.push_str(&text);
        at = span.end;
    }
    out.push_str(&src[at..]);
    normalize_layout(&out)
}

fn is_wordish(t: &Tok) -> bool {
    matches!(
        t,
        Tok::Ident(_) | Tok::Kw(_) | Tok::Number(_) | Tok::Version(_)
    )
}

/// Replace a Unicode twin with its ASCII spelling. Returns how many tokens
/// it consumed.
///
/// A word twin (`∈`, `Σ`, `√`) gets a space on a side where the ASCII word
/// would otherwise run into a neighbouring word (`b∈t` must not become
/// `bint`), and `−` gets one where `-` would otherwise form `--` (a
/// comment) or `->`.
fn ascii_edit(src: &str, tokens: &[Token], i: usize, edits: &mut Vec<(Span, String)>) -> usize {
    let t = &tokens[i];
    if !t.unicode || t.span.start == t.span.end {
        return 1;
    }
    let prev = i.checked_sub(1).map(|p| &tokens[p]);
    let next = tokens.get(i + 1);
    let touches_prev = prev.is_some_and(|p| p.span.end == t.span.start);
    let touches_next = |n: Option<&Token>| n.is_some_and(|n| n.span.start == t.span.end);
    let word = |w: &str| {
        let mut s = String::new();
        if touches_prev && prev.is_some_and(|p| is_wordish(&p.tok)) {
            s.push(' ');
        }
        s.push_str(w);
        if touches_next(next) && next.is_some_and(|n| is_wordish(&n.tok)) {
            s.push(' ');
        }
        s
    };
    let text = match &t.tok {
        Tok::Kw(Kw::In) => word("in"),
        Tok::Kw(Kw::Sum) => word("sum"),
        Tok::Ident(_) => word("sqrt"),
        Tok::Caret => {
            // `²`: the digit is the next token, with an empty span.
            let digit = match tokens.get(i + 1).map(|n| &n.tok) {
                Some(Tok::Number(d)) => d.clone(),
                _ => unreachable!("a superscript caret is followed by its digit"),
            };
            let mut s = format!("^{digit}");
            if touches_next(tokens.get(i + 2))
                && tokens.get(i + 2).is_some_and(|n| is_wordish(&n.tok))
            {
                s.push(' ');
            }
            edits.push((t.span, s));
            return 2;
        }
        Tok::Minus => {
            let before = src[..t.span.start].chars().next_back();
            let after = src[t.span.end..].chars().next();
            let mut s = String::new();
            // A neighbouring `−` pads on its own side, so only an ASCII `-`
            // before needs a space here.
            if before == Some('-') {
                s.push(' ');
            }
            s.push('-');
            if matches!(after, Some('-' | '−' | '>')) {
                s.push(' ');
            }
            s
        }
        other => other.ascii().to_string(),
    };
    edits.push((t.span, text));
    1
}

fn unicode_edit(
    tokens: &[Token],
    roles: &[Role],
    i: usize,
    edits: &mut Vec<(Span, String)>,
) -> usize {
    let t = &tokens[i];
    if t.unicode {
        return 1;
    }
    let role = roles[i];
    let text = match &t.tok {
        Tok::Arrow => "→",
        Tok::Le => "≤",
        Tok::Ge => "≥",
        Tok::Ne => "≠",
        Tok::Star if role != Role::Unit => "·",
        Tok::Slash if role != Role::Unit => "÷",
        Tok::Minus if role != Role::Unit => "−",
        Tok::Kw(Kw::In) if role == Role::Membership => "∈",
        Tok::Kw(Kw::Sum) => "Σ",
        Tok::Ident(_) if role == Role::SqrtCall => "√",
        Tok::Caret => {
            if let Some(n) = tokens.get(i + 1)
                && n.span.start == t.span.end
                && let Tok::Number(d) = &n.tok
                && (d == "2" || d == "3")
            {
                let sup = if d == "2" { "²" } else { "³" };
                edits.push((t.span.to(n.span), sup.to_string()));
                return 2;
            }
            return 1;
        }
        _ => return 1,
    };
    edits.push((t.span, text.to_string()));
    1
}

/// Strip trailing spaces and tabs from each line and end with exactly one
/// line feed.
fn normalize_layout(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for line in s.split('\n') {
        out.push_str(line.trim_end_matches([' ', '\t']));
        out.push('\n');
    }
    let trimmed = out.trim_end_matches('\n');
    let mut out = trimmed.to_string();
    out.push('\n');
    out
}
