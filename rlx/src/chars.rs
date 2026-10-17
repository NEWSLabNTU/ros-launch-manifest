//! The character layer (design section 5.3), run before tokenizing.
//!
//! An extension is reviewed by someone who reads its text and trusts that
//! what they read is what the checker evaluates. Every rule here closes a way
//! for the two to differ:
//!
//! - **UTF-8 and NFC.** Two byte sequences can render identically (`é` as one
//!   code point or as `e` plus a combining accent). The canonical form is
//!   hashed, so rlx refuses a non-NFC file rather than normalizing it: a
//!   normalizing reader would hash bytes the author never wrote.
//! - **Bidi controls** reorder what is displayed (the "Trojan Source"
//!   attack: a comment that visually closes before code that is in fact
//!   inside it). Refused everywhere, comments and strings included, because
//!   that is exactly where the attack hides them.
//! - **Invisible characters** (zero-width spaces and joiners, the byte-order
//!   mark, the soft hyphen, variation selectors, tag characters) make two
//!   identifiers that look equal differ. Refused everywhere. A leading BOM is
//!   not special-cased: it is invisible, so it is refused like any other.
//! - **Control characters** other than LF and tab. CR is refused too: the
//!   canonical form uses LF line endings, and a lone CR moves a terminal's
//!   cursor back over what was printed. U+2028 and U+2029 are counted as
//!   controls, since editors disagree on whether they break a line.
//!
//! Identifier and symbol rules need to know what is inside a comment or a
//! string, so they run in the lexer, through the classification functions at
//! the end of this module.

use crate::diag::{Diagnostic, Span, code, code_point};
use unicode_normalization::{UnicodeNormalization, char::canonical_combining_class, is_nfc};

/// Run the file-wide checks. Returns the text when they all pass.
///
/// Every violation is reported, in source order, except invalid UTF-8,
/// where nothing after the first bad byte can be located reliably.
pub fn prepass(bytes: &[u8]) -> Result<&str, Vec<Diagnostic>> {
    let text = match std::str::from_utf8(bytes) {
        Ok(t) => t,
        Err(e) => {
            let at = e.valid_up_to();
            return Err(vec![Diagnostic::new(
                code::UTF8,
                Span::new(at, at + 1),
                format!("invalid UTF-8 at byte {at}; rlx files are UTF-8"),
            )]);
        }
    };
    let mut diags = Vec::new();
    let mut seen_cr = false;
    for (i, c) in text.char_indices() {
        let span = Span::new(i, i + c.len_utf8());
        if let Some(name) = bidi_name(c) {
            diags.push(Diagnostic::new(
                code::BIDI,
                span,
                format!(
                    "bidirectional control {} ({name}) is refused anywhere in an rlx file, \
                     comments and strings included: it changes the order text is displayed in",
                    code_point(c)
                ),
            ));
        } else if let Some(name) = invisible_name(c) {
            diags.push(Diagnostic::new(
                code::ZERO_WIDTH,
                span,
                format!(
                    "invisible character {} ({name}) is refused anywhere in an rlx file: \
                     it makes text that looks the same differ",
                    code_point(c)
                ),
            ));
        } else if c == '\r' {
            // One diagnostic for a file of CRLF line endings, not one per
            // line: the fix is a single conversion.
            if !seen_cr {
                let n = text.matches('\r').count();
                diags.push(Diagnostic::new(
                    code::CONTROL,
                    span,
                    format!(
                        "carriage return (U+000D) is refused: rlx files use LF line endings \
                         ({n} carriage return{} in the file)",
                        if n == 1 { "" } else { "s" }
                    ),
                ));
            }
            seen_cr = true;
        } else if is_control(c) {
            diags.push(Diagnostic::new(
                code::CONTROL,
                span,
                format!(
                    "control character {} is refused; only LF and tab are allowed",
                    code_point(c)
                ),
            ));
        }
    }
    // NFC is checked per line. A line feed is a starter that never composes
    // with its neighbours, so the file is NFC exactly when every line is, and
    // the first failing line locates the problem.
    let mut line_start = 0;
    for line in text.split('\n') {
        if !is_nfc(line) {
            diags.push(nfc_diagnostic(line, line_start));
        }
        line_start += line.len() + 1;
    }
    diags.sort_by_key(|d| d.span.start);
    if diags.is_empty() {
        Ok(text)
    } else {
        Err(diags)
    }
}

/// Locate the first character of a non-NFC line that normalization would
/// change, and say what NFC would write there.
fn nfc_diagnostic(line: &str, line_start: usize) -> Diagnostic {
    let normalized: String = line.nfc().collect();
    let orig: Vec<(usize, char)> = line.char_indices().collect();
    let norm: Vec<char> = normalized.chars().collect();
    let i = orig
        .iter()
        .zip(norm.iter())
        .position(|((_, a), b)| a != b)
        .unwrap_or(orig.len().min(norm.len()));
    let cluster = |chars: &mut dyn Iterator<Item = char>| -> String {
        let mut out = Vec::new();
        for (k, c) in chars.enumerate() {
            if k > 0 && canonical_combining_class(c) == 0 {
                break;
            }
            out.push(code_point(c));
        }
        out.join(" ")
    };
    let found = cluster(&mut orig[i.min(orig.len())..].iter().map(|&(_, c)| c));
    let nfc = cluster(&mut norm[i.min(norm.len())..].iter().copied());
    let at = orig.get(i).map_or(line.len(), |&(o, _)| o) + line_start;
    let end = orig.get(i).map_or(at, |&(_, c)| at + c.len_utf8());
    Diagnostic::new(
        code::NOT_NFC,
        Span::new(at, end),
        format!(
            "text is not in Unicode normalization form C (found {found}, NFC writes {nfc}); \
             rlx refuses a non-NFC file rather than normalizing it, so the bytes hashed \
             are the bytes written"
        ),
    )
}

fn bidi_name(c: char) -> Option<&'static str> {
    Some(match c {
        '\u{061C}' => "ARABIC LETTER MARK",
        '\u{200E}' => "LEFT-TO-RIGHT MARK",
        '\u{200F}' => "RIGHT-TO-LEFT MARK",
        '\u{202A}' => "LEFT-TO-RIGHT EMBEDDING",
        '\u{202B}' => "RIGHT-TO-LEFT EMBEDDING",
        '\u{202C}' => "POP DIRECTIONAL FORMATTING",
        '\u{202D}' => "LEFT-TO-RIGHT OVERRIDE",
        '\u{202E}' => "RIGHT-TO-LEFT OVERRIDE",
        '\u{2066}' => "LEFT-TO-RIGHT ISOLATE",
        '\u{2067}' => "RIGHT-TO-LEFT ISOLATE",
        '\u{2068}' => "FIRST STRONG ISOLATE",
        '\u{2069}' => "POP DIRECTIONAL ISOLATE",
        _ => return None,
    })
}

fn invisible_name(c: char) -> Option<&'static str> {
    Some(match c {
        '\u{00AD}' => "SOFT HYPHEN",
        '\u{034F}' => "COMBINING GRAPHEME JOINER",
        '\u{180E}' => "MONGOLIAN VOWEL SEPARATOR",
        '\u{200B}' => "ZERO WIDTH SPACE",
        '\u{200C}' => "ZERO WIDTH NON-JOINER",
        '\u{200D}' => "ZERO WIDTH JOINER",
        '\u{2060}' => "WORD JOINER",
        '\u{2061}'..='\u{2064}' => "INVISIBLE OPERATOR",
        '\u{FEFF}' => "ZERO WIDTH NO-BREAK SPACE / BYTE ORDER MARK",
        '\u{FE00}'..='\u{FE0F}' => "VARIATION SELECTOR",
        '\u{E0000}'..='\u{E007F}' => "TAG CHARACTER",
        '\u{E0100}'..='\u{E01EF}' => "VARIATION SELECTOR SUPPLEMENT",
        _ => return None,
    })
}

fn is_control(c: char) -> bool {
    match c {
        '\n' | '\t' => false,
        '\u{0000}'..='\u{001F}' | '\u{007F}'..='\u{009F}' => true,
        '\u{2028}' | '\u{2029}' => true,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Identifier characters
// ---------------------------------------------------------------------------

/// The script an identifier character belongs to, for the one-script rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    Latin,
    Greek,
}

/// How one character may be used in an identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentClass {
    /// A letter that may start or continue an identifier.
    Letter(Script),
    /// A digit, `_`, `'` or subscript: continues an identifier, belongs to
    /// no script. `_` may also start one; the lexer handles that.
    Neutral,
    /// Looks like the named Latin letter (script, name of the character).
    Confusable { latin: char, what: &'static str },
    /// Reserved by rlx (`λ`, `Π`, `Σ`).
    Reserved,
    /// Any other letter.
    Other,
}

/// The Greek letters an identifier may use: the ones a physicist writes and
/// no reader mistakes for Latin.
const GREEK_ALLOWED: &str = "αβγδεζηθκμξπρστφχψωΓΔΘΛΞΦΨΩ";

/// Greek that reads as Latin, with the Latin letter it reads as.
const GREEK_CONFUSABLE: &[(char, char)] = &[
    ('ο', 'o'),
    ('ν', 'v'),
    ('ι', 'i'),
    ('υ', 'u'),
    ('Α', 'A'),
    ('Β', 'B'),
    ('Ε', 'E'),
    ('Ζ', 'Z'),
    ('Η', 'H'),
    ('Ι', 'I'),
    ('Κ', 'K'),
    ('Μ', 'M'),
    ('Ν', 'N'),
    ('Ο', 'O'),
    ('Ρ', 'P'),
    ('Τ', 'T'),
    ('Υ', 'Y'),
    ('Χ', 'X'),
];

/// Cyrillic letters that render as Latin ones. Cyrillic is refused anyway;
/// naming the look-alike tells the reader why an identifier that reads
/// correctly is refused.
const CYRILLIC_CONFUSABLE: &[(char, char)] = &[
    ('а', 'a'),
    ('е', 'e'),
    ('о', 'o'),
    ('р', 'p'),
    ('с', 'c'),
    ('у', 'y'),
    ('х', 'x'),
    ('і', 'i'),
    ('ј', 'j'),
    ('ѕ', 's'),
    ('А', 'A'),
    ('В', 'B'),
    ('Е', 'E'),
    ('К', 'K'),
    ('М', 'M'),
    ('Н', 'H'),
    ('О', 'O'),
    ('Р', 'P'),
    ('С', 'C'),
    ('Т', 'T'),
    ('Х', 'X'),
];

/// Lean 4's subscript ranges: digits `₀-₉`, letters `ₐ-ₜ`, `ᵢ-ᵪ` and `ⱼ`.
pub fn is_subscript(c: char) -> bool {
    matches!(c, '\u{2080}'..='\u{2089}' | '\u{2090}'..='\u{209C}' | '\u{1D62}'..='\u{1D6A}' | '\u{2C7C}')
}

/// May this character be part of an identifier token at all? The lexer
/// takes the longest run of such characters and then validates it with
/// [`classify`], so a refused letter is reported as a refused letter in an
/// identifier rather than as a stray symbol splitting it in two.
pub fn is_ident_continue(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || c == '_'
        || c == '\''
        || is_subscript(c)
        || (!c.is_ascii() && c.is_alphabetic())
}

/// May an identifier token start here? `Σ` is excluded because at the start
/// of a token it is the `sum` keyword's twin.
pub fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic()
        || c == '_'
        || is_subscript(c)
        || (!c.is_ascii() && c.is_alphabetic() && c != 'Σ')
}

pub fn classify(c: char) -> IdentClass {
    if c.is_ascii_alphabetic() {
        return IdentClass::Letter(Script::Latin);
    }
    if c.is_ascii_digit() || c == '_' || c == '\'' || is_subscript(c) {
        return IdentClass::Neutral;
    }
    if GREEK_ALLOWED.contains(c) {
        return IdentClass::Letter(Script::Greek);
    }
    if matches!(c, 'λ' | 'Π' | 'Σ') {
        return IdentClass::Reserved;
    }
    if let Some(&(_, latin)) = GREEK_CONFUSABLE.iter().find(|(g, _)| *g == c) {
        return IdentClass::Confusable {
            latin,
            what: "Greek",
        };
    }
    if let Some(&(_, latin)) = CYRILLIC_CONFUSABLE.iter().find(|(g, _)| *g == c) {
        return IdentClass::Confusable {
            latin,
            what: "Cyrillic",
        };
    }
    IdentClass::Other
}

/// Validate one identifier token. Returns the first violation; one per
/// identifier is enough to locate it, and a second would usually repeat it.
pub fn check_ident(text: &str, start: usize) -> Option<Diagnostic> {
    let mut script: Option<(Script, char)> = None;
    for (k, (off, c)) in text.char_indices().enumerate() {
        let span = Span::new(start + off, start + off + c.len_utf8());
        if k == 0 && (c.is_ascii_digit() || c == '\'' || is_subscript(c)) {
            return Some(Diagnostic::new(
                code::LETTER,
                span,
                format!(
                    "`{c}` ({}) cannot start an identifier; an identifier starts with \
                     a letter or `_`",
                    code_point(c)
                ),
            ));
        }
        match classify(c) {
            IdentClass::Neutral => {}
            IdentClass::Letter(s) => match script {
                None => script = Some((s, c)),
                Some((first, fc)) if first != s => {
                    return Some(Diagnostic::new(
                        code::MIXED_SCRIPT,
                        span,
                        format!(
                            "identifier `{text}` mixes {} (`{fc}`) and {} (`{c}`) letters; \
                             an identifier uses one script",
                            script_name(first),
                            script_name(s)
                        ),
                    ));
                }
                Some(_) => {}
            },
            IdentClass::Confusable { latin, what } => {
                return Some(Diagnostic::new(
                    code::CONFUSABLE,
                    span,
                    format!(
                        "{what} `{c}` ({}) in identifier `{text}` looks like Latin `{latin}`; \
                         write `{latin}` if that is what you mean",
                        code_point(c)
                    ),
                ));
            }
            IdentClass::Reserved => {
                return Some(Diagnostic::new(
                    code::RESERVED,
                    span,
                    format!(
                        "`{c}` ({}) is reserved and cannot appear in an identifier",
                        code_point(c)
                    ),
                ));
            }
            IdentClass::Other => {
                let hint = if c == '\u{00B5}' {
                    "; the micro sign is not Greek `μ`, and units are spelled in ASCII (`us`, `um`)"
                } else {
                    ""
                };
                return Some(Diagnostic::new(
                    code::LETTER,
                    span,
                    format!(
                        "`{c}` ({}) is not allowed in an identifier: identifiers use ASCII \
                         letters, digits, `_`, `'`, subscripts and the Greek letters \
                         {GREEK_ALLOWED}{hint}",
                        code_point(c)
                    ),
                ));
            }
        }
    }
    None
}

fn script_name(s: Script) -> &'static str {
    match s {
        Script::Latin => "Latin",
        Script::Greek => "Greek",
    }
}

/// A hint for a non-ASCII symbol that is not one of the twins but is easily
/// mistaken for one.
pub fn symbol_hint(c: char) -> Option<&'static str> {
    Some(match c {
        '×' | '⋅' | '∙' | '•' => "write `*` (or its one twin `·`, U+00B7)",
        '–' | '—' | '‐' | '‑' | '‒' => "write `-` (or its one twin `−`, U+2212)",
        '∕' | '⁄' => "write `/` (or its one twin `÷`)",
        '⩽' => "write `<=` (or its one twin `≤`)",
        '⩾' => "write `>=` (or its one twin `≥`)",
        '⟶' | '➝' | '⇒' => "write `->` (or its one twin `→`)",
        '∑' => "write `sum` (or its one twin `Σ`, U+03A3)",
        '“' | '”' | '„' => "strings are delimited by ASCII `\"`",
        '°' | '℃' | '℉' => {
            "rlx has no affine temperature units (°C, °F); write a temperature \
             difference in `K`, or an angle in `deg`"
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_text_passes() {
        assert!(prepass("x -- ≤ é\n\tok\n".as_bytes()).is_ok());
    }

    #[test]
    fn crlf_refused() {
        let e = prepass(b"a\r\nb").unwrap_err();
        assert_eq!(e[0].code, code::CONTROL);
    }

    #[test]
    fn nfc_located() {
        let src = "ok\nab\u{0065}\u{0301}c";
        let e = prepass(src.as_bytes()).unwrap_err();
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].code, code::NOT_NFC);
        assert_eq!(e[0].span.start, 5);
        assert!(e[0].message.contains("U+0065 U+0301"), "{}", e[0].message);
        assert!(
            e[0].message.contains("NFC writes U+00E9"),
            "{}",
            e[0].message
        );
    }

    #[test]
    fn identifiers() {
        assert!(check_ident("v₀", 0).is_none());
        assert!(check_ident("αβ'", 0).is_none());
        assert!(check_ident("_x1", 0).is_none());
        assert_eq!(check_ident("vα", 0).unwrap().code, code::MIXED_SCRIPT);
        assert_eq!(check_ident("ο", 0).unwrap().code, code::CONFUSABLE);
        assert_eq!(check_ident("а", 0).unwrap().code, code::CONFUSABLE);
        assert_eq!(check_ident("ж", 0).unwrap().code, code::LETTER);
        assert_eq!(check_ident("λ", 0).unwrap().code, code::RESERVED);
        assert_eq!(check_ident("ς", 0).unwrap().code, code::LETTER);
    }
}
