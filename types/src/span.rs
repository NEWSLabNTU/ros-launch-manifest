//! Source span tracking for diagnostic output.

use std::ops::Range;

/// A value paired with its source location.
#[derive(Debug, Clone)]
pub struct Spanned<T> {
    pub value: T,
    pub span: Range<usize>,
}

impl<T> Spanned<T> {
    pub fn new(value: T, span: Range<usize>) -> Self {
        Self { value, span }
    }

    /// Create a Spanned with no location info (span 0..0).
    pub fn no_span(value: T) -> Self {
        Self {
            value,
            span: 0..0,
        }
    }
}

impl<T: PartialEq> PartialEq for Spanned<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

/// Build a line-starts table from source text.
/// Returns byte offsets where each line begins.
pub fn line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

/// Convert a (line, col) pair to a byte offset.
/// Lines and columns are 0-based.
pub fn line_col_to_offset(starts: &[usize], line: usize, col: usize) -> usize {
    if line < starts.len() {
        starts[line] + col
    } else {
        starts.last().copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_starts() {
        let src = "abc\ndef\nghi";
        let starts = line_starts(src);
        assert_eq!(starts, vec![0, 4, 8]);
    }

    #[test]
    fn test_line_col_to_offset() {
        let starts = vec![0, 4, 8];
        assert_eq!(line_col_to_offset(&starts, 0, 0), 0);
        assert_eq!(line_col_to_offset(&starts, 0, 2), 2);
        assert_eq!(line_col_to_offset(&starts, 1, 0), 4);
        assert_eq!(line_col_to_offset(&starts, 2, 1), 9);
    }
}
