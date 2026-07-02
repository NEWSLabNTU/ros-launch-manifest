//! Shared, portable scheduling spec for launch-based systems.
//!
//! Consumed by both `play_launch` (Linux RT, target `posix`) and `nano-ros`
//! (RTOS targets). The generic layer (tier class, deadline, binding) is
//! platform-independent and carries no priority numbers; per-platform
//! placement lives in `[tiers.<name>.<target>]` sub-tables.

pub mod parse;
pub mod resolve;
pub mod types;

pub use parse::parse_system_sched;
pub use resolve::SchedError;
pub use types::{AssignRule, SystemSched, TierDef, TierPlatformSpec};
