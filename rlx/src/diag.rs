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

    /// Two expressions that must have one dimension do not, `sqrt` of a
    /// dimension with an odd exponent, or a value with the wrong dimension
    /// for its key.
    pub const DIMENSION: &str = "rlx-dimension";
    /// A name no binding, summation variable, parameter, key or input
    /// declares, or a call to an undeclared function.
    pub const UNKNOWN_NAME: &str = "rlx-unknown-name";
    /// A dimension name that is neither built in nor declared.
    pub const UNKNOWN_DIMENSION: &str = "rlx-unknown-dimension";
    /// Two keys or inputs whose last path segments are the same name.
    pub const AMBIGUOUS_KEY: &str = "rlx-ambiguous-key";
    /// A call with the wrong number of arguments.
    pub const ARITY: &str = "rlx-arity";
    /// A condition, a `check`, or an operand of `and`, `or`, `not` that is
    /// not a boolean.
    pub const NOT_BOOLEAN: &str = "rlx-not-boolean";
    /// A value of the wrong kind: a boolean, table or band where a quantity
    /// belongs, or the reverse.
    pub const TYPE: &str = "rlx-type";
    /// A power of a dimensioned value whose exponent is not an integer
    /// literal.
    pub const EXPONENT: &str = "rlx-exponent";
    /// A field or method a table or band does not have.
    pub const MEMBER: &str = "rlx-member";
    /// `param Dim unit U` with a unit that is not of the dimension.
    pub const UNIT: &str = "rlx-unit";
    /// `dimension` declarations that define each other.
    pub const CYCLE: &str = "rlx-cycle";
    /// A second declaration of a name: a built-in dimension, a function, a
    /// key path, a derive target, a binding or a parameter.
    pub const REDEFINITION: &str = "rlx-redefinition";
    /// A `worst_case` name that is not a key or input the item uses.
    pub const WORST_CASE: &str = "rlx-worst-case";
    /// A `derive` whose target is not a core bound an extension may fill.
    pub const DERIVE_TARGET: &str = "rlx-derive-target";
    /// A contract value that is not a value of its key's type.
    pub const VALUE: &str = "rlx-value";

    /// Evaluation: the extension has no item of that name.
    pub const NO_ITEM: &str = "rlx-no-item";
    /// Evaluation: an input without a default was not given.
    pub const MISSING_INPUT: &str = "rlx-missing-input";
    /// Evaluation: a supplied input is not a value of its key's type.
    pub const INPUT: &str = "rlx-input";
    /// Evaluation: an input lies outside the item's `domain`.
    pub const DOMAIN: &str = "rlx-domain";
    /// Evaluation: a division by zero.
    pub const DIVISION_BY_ZERO: &str = "rlx-division-by-zero";
    /// Evaluation: `sqrt` of a negative value.
    pub const NEGATIVE_SQRT: &str = "rlx-negative-sqrt";
    /// Evaluation: a `bands` slice outside its table.
    pub const BANDS: &str = "rlx-bands";
    /// Evaluation: a result that is infinite or not a number.
    pub const NON_FINITE: &str = "rlx-non-finite";

    /// The codes raised only while evaluating (see `crate::eval`).
    pub const EVAL: &[&str] = &[
        NO_ITEM,
        MISSING_INPUT,
        INPUT,
        DOMAIN,
        DIVISION_BY_ZERO,
        NEGATIVE_SQRT,
        BANDS,
        NON_FINITE,
    ];

    /// Every code an rlx file can raise, for tests that check each one has
    /// a refused example. `VALUE` is raised only by the contract value
    /// parser and is tested there.
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
        DIMENSION,
        UNKNOWN_NAME,
        UNKNOWN_DIMENSION,
        AMBIGUOUS_KEY,
        ARITY,
        NOT_BOOLEAN,
        TYPE,
        EXPONENT,
        MEMBER,
        UNIT,
        CYCLE,
        REDEFINITION,
        WORST_CASE,
        DERIVE_TARGET,
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

/// `a Acceleration` to `an Acceleration`: messages put a dimension name
/// after "a", and dimension names are the only capitalized words that do,
/// so the article is fixed where the name is known rather than at every
/// format string.
pub fn fix_articles(msg: &str) -> String {
    let mut out = String::with_capacity(msg.len() + 4);
    let mut rest = msg;
    while let Some(i) = rest.find("a ") {
        let at_word = i == 0 || !rest[..i].ends_with(|c: char| c.is_alphanumeric() || c == '_');
        let next = rest[i + 2..].chars().next();
        out.push_str(&rest[..i]);
        if at_word && next.is_some_and(|c| "AEIOU".contains(c)) {
            out.push_str("an ");
        } else {
            out.push_str("a ");
        }
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out
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
    fn articles() {
        assert_eq!(
            fix_articles("a Acceleration, a Time, data Area, a unit"),
            "an Acceleration, a Time, data Area, a unit"
        );
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
