//! Launch manifest types and YAML parser.
//!
//! This crate defines the typed AST for launch manifest files and
//! provides a parser that reads YAML with source span tracking.

pub mod parse;
pub mod span;
pub mod types;

pub use parse::parse_manifest;
pub use span::Spanned;
pub use types::*;
