//! Launch manifest types and YAML parser.
//!
//! This crate defines the typed AST for launch manifest files and
//! provides a parser that reads YAML with source span tracking.

pub mod bound;
pub mod cond;
pub mod duration;
pub mod field_table;
pub mod parse;
pub mod span;
pub mod subst;
pub mod types;

pub use bound::{
    BOUND_KEYS, Bound, BoundKey, BoundKind, Form, Limit, OptBound, ParamBinding, Quantity,
    bound_key, spelled,
};
pub use cond::{evaluate as evaluate_condition, filter_manifest};
pub use parse::{
    ParseResult, parse_manifest, parse_manifest_str, parse_manifest_str_with_spans,
    parse_manifest_with_spans,
};
pub use span::{SpanIndex, Spanned};

/// The contract grammar this crate reads: the rlm release it is. A contract's `rlm:` header names the release it needs, and
/// [`parse_manifest_str`] refuses one newer than this before reading a key
/// of the body, naming both. Kept equal to the newest released heading of
/// `CHANGELOG.md` by a test, so a release cannot forget it.
pub const GRAMMAR_VERSION: &str = "0.1.58";
pub use subst::{SubstError, resolve_args, substitute_manifest, substitute_str};
pub use types::*;
