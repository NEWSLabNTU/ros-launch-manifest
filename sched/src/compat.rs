//! Reading platform durations during the phase-59 deprecation window.
//!
//! The contract side and the platform side need DIFFERENT alias mechanisms,
//! which the phase-63 plan did not anticipate. Contracts are read by a
//! hand-rolled YAML parser, so `types::parse::yaml_duration` can inspect both
//! field names and reject a bare number under the new one. Platform files go
//! through serde (`toml`, `serde_yaml_ng`), and `#[serde(alias = "budget_us")]`
//! cannot tell the deserializer WHICH spelling matched — it sees one field.
//!
//! So on this side the compatibility type accepts either representation:
//!
//! ```text
//! budget    = "8ms"     # phase-59 spelling, unit in the value
//! budget_us = 8000      # deprecated name, unit in the name
//! budget    = 8000      # accepted, read as microseconds — see below
//! ```
//!
//! That third line is a deliberate, documented weakening. A bare number under
//! the NEW name should be an error, and on the contract side it is. Here serde
//! cannot distinguish it from the aliased spelling without a hand-written
//! `Deserialize` for every containing struct, so during the window it is read
//! as microseconds — the same meaning the old name carried, which is the
//! conservative choice: an un-migrated file cannot change behaviour.
//!
//! The leniency and the alias are removed together at the phase-63 W6 sunset,
//! which is what the contract `version:` lever is for. Until then this is the
//! one place the phase's "bare numbers are rejected" rule does not hold, and
//! it is recorded here rather than discovered later.

use ros_launch_manifest_types::duration::Duration;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::fmt;

/// A platform duration that reads both the phase-59 and deprecated forms.
///
/// Serializes ONLY as the canonical phase-59 string, so a platform file
/// written back is migrated by the act of writing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct PlatformDuration(pub Duration);

impl PlatformDuration {
    pub fn as_micros(self) -> u64 {
        (self.0.as_nanos() / 1_000).max(0) as u64
    }
    pub fn from_micros(us: u64) -> Self {
        Self(Duration::from_micros(us as i64))
    }
}

impl From<PlatformDuration> for Duration {
    fn from(p: PlatformDuration) -> Self {
        p.0
    }
}

impl Serialize for PlatformDuration {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

impl<'de> Deserialize<'de> for PlatformDuration {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl de::Visitor<'_> for V {
            type Value = PlatformDuration;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a duration like `8ms`, or a bare number of microseconds (deprecated)")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<PlatformDuration, E> {
                v.parse::<Duration>()
                    .map(PlatformDuration)
                    .map_err(E::custom)
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<PlatformDuration, E> {
                Ok(PlatformDuration::from_micros(v))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<PlatformDuration, E> {
                Ok(PlatformDuration::from_micros(v.max(0) as u64))
            }
        }
        d.deserialize_any(V)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_both_spellings_to_the_same_value() {
        let new: PlatformDuration = serde_json::from_str("\"8ms\"").unwrap();
        let old: PlatformDuration = serde_json::from_str("8000").unwrap();
        assert_eq!(new, old, "8ms and 8000us must be the same duration");
        assert_eq!(new.as_micros(), 8_000);
    }

    /// The documented weakening: a bare number is microseconds here, where the
    /// contract side rejects it. Pinned so the difference is deliberate and
    /// visible rather than a surprise found later.
    #[test]
    fn a_bare_number_is_microseconds_on_this_side() {
        let d: PlatformDuration = serde_json::from_str("50000").unwrap();
        assert_eq!(d.0.as_millis_f64(), 50.0);
    }

    /// Writing a platform file back migrates it: only the canonical spelling
    /// is ever emitted.
    #[test]
    fn only_the_canonical_form_is_written() {
        let d = PlatformDuration::from_micros(50_000);
        assert_eq!(serde_json::to_string(&d).unwrap(), "\"50ms\"");
    }
}
