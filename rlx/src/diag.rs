//! Diagnostics: a stable code, a message and the byte span it is about.
//!
//! Every refusal rlx makes reaches a person reviewing an extension, often one
//! who did not write it, so each one carries a code that never changes
//! meaning (tests, CI filters and documentation key on it) and a position
//! they can jump to. Rendering is `file:line:col: error[code]: message`, the
//! shape compilers use, so editors that already parse that shape link it.
//!
//! Lines and columns are 1-based. A column counts Unicode scalar values from
//! the start of the line, so a tab is one column and `≤` is one column: the
//! column is what a reader counts, not what a byte offset says.

use std::fmt;

/// A half-open byte range into the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// The smallest span covering both.
    pub fn to(self, other: Span) -> Span {
        Span::new(self.start.min(other.start), self.end.max(other.end))
    }
}

/// The stable diagnostic codes. Kept as constants so a test can enumerate
/// them and a typo in one place is a compile error rather than a new code.
pub mod code {
    /// The file is not valid UTF-8.
    pub const UTF8: &str = "rlx-utf8";
    /// The file is valid UTF-8 but not in Unicode normalization form C.
    pub const NOT_NFC: &str = "rlx-not-nfc";
    /// A bidirectional control character, anywhere in the file.
    pub const BIDI: &str = "rlx-bidi";
    /// An invisible (zero-width or format) character, anywhere in the file.
    pub const ZERO_WIDTH: &str = "rlx-zero-width";
    /// A control character other than LF and tab, anywhere in the file.
    pub const CONTROL: &str = "rlx-control";
    /// An identifier character that looks like a Latin letter it is not.
    pub const CONFUSABLE: &str = "rlx-confusable";
    /// A letter rlx reserves (`λ`, `Π`, `Σ`), used in an identifier.
    pub const RESERVED: &str = "rlx-reserved";
    /// An identifier mixing Latin and Greek letters.
    pub const MIXED_SCRIPT: &str = "rlx-mixed-script";
    /// Any other letter not allowed in an identifier.
    pub const LETTER: &str = "rlx-letter";
    /// A non-ASCII symbol outside comments and strings that has no meaning.
    pub const SYMBOL: &str = "rlx-symbol";
    /// The text does not follow the grammar.
    pub const PARSE: &str = "rlx-parse";
    /// A `fn` calls itself, directly or through other `fn`s.
    pub const RECURSION: &str = "rlx-recursion";
    /// A name that a quantity literal would read as a unit.
    pub const SHADOW: &str = "rlx-shadow";

    /// Every code, for tests that check each one has a refused example.
    pub const ALL: &[&str] = &[
        UTF8,
        NOT_NFC,
        BIDI,
        ZERO_WIDTH,
        CONTROL,
        CONFUSABLE,
        RESERVED,
        MIXED_SCRIPT,
        LETTER,
        SYMBOL,
        PARSE,
        RECURSION,
        SHADOW,
    ];
}

/// One refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: &'static str,
    pub message: String,
    pub span: Span,
}

impl Diagnostic {
    pub fn new(code: &'static str, span: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            span,
        }
    }

    /// `file:line:col: error[code]: message`.
    ///
    /// Takes the raw bytes rather than a `&str` because the one diagnostic
    /// raised before the text is known to be UTF-8 (`rlx-utf8`) still needs a
    /// position.
    pub fn render(&self, file: &str, src: &[u8]) -> String {
        let (line, col) = line_col(src, self.span.start);
        format!(
            "{file}:{line}:{col}: error[{}]: {}",
            self.code, self.message
        )
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error[{}]: {}", self.code, self.message)
    }
}

/// 1-based line and column of a byte offset. The column counts scalar values
/// (decoding lossily, so an invalid sequence before the offset counts as one
/// replacement character per error rather than failing).
pub fn line_col(src: &[u8], offset: usize) -> (usize, usize) {
    let offset = offset.min(src.len());
    let before = &src[..offset];
    let line_start = before
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |i| i + 1);
    let line = before.iter().filter(|&&b| b == b'\n').count() + 1;
    let col = String::from_utf8_lossy(&src[line_start..offset])
        .chars()
        .count()
        + 1;
    (line, col)
}

/// `U+XXXX`, the spelling every message uses for a code point, so a reader
/// can search a character table for it.
pub fn code_point(c: char) -> String {
    format!("U+{:04X}", c as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_count_scalar_values() {
        let src = "ab\nα≤x".as_bytes();
        // `x` is the third scalar value on line 2 although it is byte 5 of it.
        let off = src.len() - 1;
        assert_eq!(line_col(src, off), (2, 3));
        assert_eq!(line_col(src, 0), (1, 1));
    }

    #[test]
    fn render_shape() {
        let d = Diagnostic::new(code::PARSE, Span::new(3, 4), "expected `:`");
        assert_eq!(
            d.render("f.rlx", b"ab\ncd"),
            "f.rlx:2:1: error[rlx-parse]: expected `:`"
        );
    }
}
