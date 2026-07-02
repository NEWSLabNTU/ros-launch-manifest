//! Selector-based tier resolution.

/// Errors from parsing or resolving the scheduling spec.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SchedError {
    #[error("failed to parse system scheduling TOML: {0}")]
    Parse(String),
}
