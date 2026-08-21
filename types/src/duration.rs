//! A duration that carries its unit in the value.
//!
//! Phase 59. `budget_us: 8` when the author meant 8 ms is off by three orders
//! of magnitude, it flows into a scheduling parameter the kernel admits or
//! rejects, and nothing in the schema can catch it — both are valid integers.
//! `budget: 8ms` makes that mistake unrepresentable rather than merely
//! discouraged.
//!
//! This is the same species of defect as a deadline used as a cost, or a
//! `SCHED_OTHER` tier clamped into an RT band: in each case a WRONG value was
//! structurally representable. The fix is the same shape — make the type
//! refuse it.
//!
//! # Grammar
//!
//! Decimal number, optional whitespace, unit from `ns | us | ms | s`.
//! Decimals are accepted (`1.5ms`), because forcing `1500us` is unit
//! gymnastics that authors get wrong in the other direction.
//!
//! **A bare number is an error, not a default.** Guessing a unit reintroduces
//! exactly the ambiguity this type exists to remove, and it would guess wrong
//! silently — which is worse than the status quo, where at least the field
//! name says what was meant.
//!
//! # Representation
//!
//! Nanoseconds in an `i64`: exact for every value these schemas carry, unifies
//! the contract side's `f64` milliseconds with the platform side's `u64`
//! microseconds, and keeps arithmetic away from floats. ~292 years of range.

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::fmt;

/// A duration with an explicit unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Duration {
    nanos: i64,
}

/// Why a duration string could not be parsed. Each variant names what the
/// author should write instead — these reach a human editing a contract.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DurationParseError {
    #[error(
        "`{0}` has no unit — write it with one of ns, us, ms, s (for example `{0}ms`). \
         A bare number is rejected on purpose: guessing the unit is how a value ends up \
         1000x wrong"
    )]
    MissingUnit(String),
    #[error("`{0}` is not a number followed by a unit (expected e.g. `12ms`, `1.5s`, `800us`)")]
    Malformed(String),
    #[error("`{unit}` is not a duration unit — use ns, us, ms or s")]
    UnknownUnit { unit: String },
    #[error("`{0}` does not fit in the supported range (about 292 years)")]
    OutOfRange(String),
}

const NS: i64 = 1;
const US: i64 = 1_000;
const MS: i64 = 1_000_000;
const S: i64 = 1_000_000_000;

impl Duration {
    pub const fn from_nanos(nanos: i64) -> Self {
        Self { nanos }
    }
    pub const fn as_nanos(self) -> i64 {
        self.nanos
    }
    /// Whole microseconds, for the platform boundary that still speaks them.
    pub fn as_micros(self) -> u64 {
        (self.nanos / 1_000).max(0) as u64
    }
    pub fn as_micros_f64(self) -> f64 {
        self.nanos as f64 / US as f64
    }
    pub fn as_millis_f64(self) -> f64 {
        self.nanos as f64 / MS as f64
    }
    pub fn as_secs_f64(self) -> f64 {
        self.nanos as f64 / S as f64
    }

    /// Build from milliseconds, for migrating call sites that held `f64` ms.
    pub fn from_millis_f64(ms: f64) -> Self {
        Self {
            nanos: (ms * MS as f64).round() as i64,
        }
    }
    /// Build from microseconds, for the platform side's `u64` us.
    pub fn from_micros(us: i64) -> Self {
        Self {
            nanos: us.saturating_mul(US),
        }
    }

    /// Canonical text: the COARSEST unit that keeps the value an integer.
    ///
    /// `12ms` not `12000us`, `1500us` not `1.5ms`. Two reasons: a
    /// `system_model.yaml` re-emitted from an unchanged input must diff
    /// empty, which a float cannot guarantee; and no float should reach a
    /// scheduling parameter, where rounding is a real error rather than a
    /// display concern.
    pub fn canonical(self) -> String {
        let n = self.nanos;
        if n == 0 {
            return "0s".to_string();
        }
        for (div, suffix) in [(S, "s"), (MS, "ms"), (US, "us")] {
            if n % div == 0 {
                return format!("{}{}", n / div, suffix);
            }
        }
        format!("{n}ns")
    }
}

impl std::str::FromStr for Duration {
    type Err = DurationParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let t = s.trim();
        if t.is_empty() {
            return Err(DurationParseError::Malformed(s.to_string()));
        }
        // Split at the first character that cannot continue a number.
        let split = t
            .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
            .ok_or_else(|| DurationParseError::MissingUnit(t.to_string()))?;
        let (num, unit) = t.split_at(split);
        let unit = unit.trim();
        if num.is_empty() {
            return Err(DurationParseError::Malformed(s.to_string()));
        }
        let value: f64 = num
            .parse()
            .map_err(|_| DurationParseError::Malformed(s.to_string()))?;
        let scale = match unit {
            "ns" => NS,
            "us" | "µs" => US,
            "ms" => MS,
            "s" => S,
            other => {
                return Err(DurationParseError::UnknownUnit {
                    unit: other.to_string(),
                });
            }
        };
        let nanos = value * scale as f64;
        if !nanos.is_finite() || nanos.abs() >= i64::MAX as f64 {
            return Err(DurationParseError::OutOfRange(s.to_string()));
        }
        Ok(Self {
            nanos: nanos.round() as i64,
        })
    }
}

impl fmt::Display for Duration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.canonical())
    }
}

impl Serialize for Duration {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.canonical())
    }
}

impl<'de> Deserialize<'de> for Duration {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl de::Visitor<'_> for V {
            type Value = Duration;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a duration with a unit, e.g. `12ms`, `800us`, `1.5s`")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Duration, E> {
                v.parse().map_err(E::custom)
            }
            // A YAML scalar without quotes still arrives as a string when it
            // has a unit suffix. A NUMBER arriving here means the author wrote
            // a bare value, which is the mistake this type exists to catch.
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Duration, E> {
                Err(E::custom(DurationParseError::MissingUnit(v.to_string())))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Duration, E> {
                Err(E::custom(DurationParseError::MissingUnit(v.to_string())))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Duration, E> {
                Err(E::custom(DurationParseError::MissingUnit(v.to_string())))
            }
        }
        d.deserialize_any(V)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_unit_in_the_grammar() {
        for (text, nanos) in [
            ("5ns", 5),
            ("800us", 800_000),
            ("12ms", 12_000_000),
            ("1.5s", 1_500_000_000),
            ("1.5ms", 1_500_000),
        ] {
            assert_eq!(
                text.parse::<Duration>().unwrap().as_nanos(),
                nanos,
                "{text}"
            );
        }
    }

    /// The whole point of the phase. A bare number must not parse — not to
    /// milliseconds, not to anything.
    #[test]
    fn a_bare_number_is_rejected_not_guessed() {
        let err = "8".parse::<Duration>().unwrap_err();
        assert!(matches!(err, DurationParseError::MissingUnit(_)), "{err:?}");
        // And the message tells the author what to write.
        assert!(err.to_string().contains("8ms"), "{err}");
        assert!(err.to_string().contains("1000x"), "{err}");
    }

    #[test]
    fn an_unknown_unit_names_the_ones_that_work() {
        let err = "12min".parse::<Duration>().unwrap_err();
        assert!(err.to_string().contains("ns, us, ms or s"), "{err}");
    }

    /// Canonical form is the COARSEST unit that stays integral, so a
    /// round-trip of an unchanged file diffs empty.
    #[test]
    fn canonical_form_is_stable_and_integral() {
        for (text, canon) in [
            ("12ms", "12ms"),
            ("12000us", "12ms"),
            ("1.5ms", "1500us"),
            ("1500000ns", "1500us"),
            ("2s", "2s"),
            ("0ms", "0s"),
        ] {
            let d: Duration = text.parse().unwrap();
            assert_eq!(d.canonical(), canon, "{text}");
            // And canonical text must itself re-parse to the same value.
            assert_eq!(canon.parse::<Duration>().unwrap(), d, "{canon} round-trip");
        }
    }

    #[test]
    fn serde_round_trips_through_the_canonical_string() {
        let d: Duration = "1.5ms".parse().unwrap();
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(json, "\"1500us\"");
        assert_eq!(serde_json::from_str::<Duration>(&json).unwrap(), d);
    }

    /// A bare number in a document is the 1000x error. serde must refuse it
    /// with a message an author can act on, not coerce it.
    #[test]
    fn serde_refuses_a_bare_number_in_a_document() {
        let err = serde_json::from_str::<Duration>("8")
            .unwrap_err()
            .to_string();
        assert!(err.contains("no unit"), "{err}");
    }

    #[test]
    fn conversions_match_the_units_they_replace() {
        let d: Duration = "12ms".parse().unwrap();
        assert_eq!(d.as_millis_f64(), 12.0);
        assert_eq!(d.as_micros_f64(), 12_000.0);
        assert_eq!(Duration::from_millis_f64(12.0), d);
        assert_eq!(Duration::from_micros(12_000), d);
    }
}

/// serde adapters for the phase-59 deprecation window.
///
/// One type (`Duration`) and one pair of functions, applied uniformly to every
/// timing field rather than a bespoke wrapper per side. A field gets:
///
/// ```ignore
/// #[serde(default, alias = "budget_us", deserialize_with = "compat::opt_micros")]
/// pub budget: Option<Duration>,
/// ```
///
/// The adapter accepts the phase-59 spelling (`"8ms"`, unit in the value) and
/// the deprecated one (`8000`, unit implied by the field name it was aliased
/// from). Serialization is always canonical, so writing a file back migrates
/// it.
///
/// **Why a bare number is accepted here and refused elsewhere.** `serde`'s
/// alias mechanism gives the deserializer ONE field; it cannot report which
/// spelling matched. Refusing bare numbers under the new name would therefore
/// require a hand-written `Deserialize` for every containing struct. Reading a
/// bare number as the legacy unit is the conservative alternative — an
/// un-migrated file cannot change meaning — and the leniency disappears with
/// the alias at the phase-63 W6 sunset.
///
/// The hand-rolled contract parser, which exists for source spans that serde
/// cannot provide, applies the SAME rule through [`from_legacy_scalar`], so the
/// two paths cannot drift.
pub mod compat {
    use super::{Duration, LegacyUnit};
    use serde::{Deserialize, Deserializer, de};
    use std::fmt;

    /// One newtype per legacy unit, so `Option<T>` deserialization comes free
    /// from serde and no intermediate value type is needed. Routing through
    /// `serde_json::Value` was the first attempt and was wrong twice: the
    /// error types do not line up, and it drags JSON into a YAML path.
    macro_rules! compat_newtype {
        ($name:ident, $unit:expr, $fname:ident) => {
            struct $name(Duration);

            impl<'de> Deserialize<'de> for $name {
                fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                    struct V;
                    impl de::Visitor<'_> for V {
                        type Value = Duration;
                        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                            write!(
                                f,
                                "a duration like `8ms`, or a bare number of {}",
                                $unit.name()
                            )
                        }
                        fn visit_str<E: de::Error>(self, v: &str) -> Result<Duration, E> {
                            v.parse().map_err(E::custom)
                        }
                        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Duration, E> {
                            Ok($unit.scale(v as f64))
                        }
                        fn visit_i64<E: de::Error>(self, v: i64) -> Result<Duration, E> {
                            Ok($unit.scale(v as f64))
                        }
                        fn visit_f64<E: de::Error>(self, v: f64) -> Result<Duration, E> {
                            Ok($unit.scale(v))
                        }
                    }
                    d.deserialize_any(V).map($name)
                }
            }

            pub fn $fname<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Duration>, D::Error> {
                Ok(Option::<$name>::deserialize(d)?.map(|x| x.0))
            }
        };
    }

    /// Required-field variants. `ChainDecl::max_latency` is not optional, and
    /// `Option<T>`-shaped adapters cannot serve it.
    pub fn millis<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        Ok(CompatMillis::deserialize(d)?.0)
    }

    #[allow(dead_code)]
    pub fn micros<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        Ok(CompatMicros::deserialize(d)?.0)
    }

    compat_newtype!(CompatMillis, LegacyUnit::Millis, opt_millis);
    compat_newtype!(CompatMicros, LegacyUnit::Micros, opt_micros);
}

/// The unit a deprecated field name implied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyUnit {
    Millis,
    Micros,
}

impl LegacyUnit {
    pub fn name(self) -> &'static str {
        match self {
            LegacyUnit::Millis => "milliseconds",
            LegacyUnit::Micros => "microseconds",
        }
    }
    pub fn scale(self, v: f64) -> Duration {
        match self {
            LegacyUnit::Millis => Duration::from_millis_f64(v),
            LegacyUnit::Micros => Duration::from_micros(v as i64),
        }
    }
}

/// Read a value that may be either spelling, for callers that are not serde.
///
/// The hand-rolled contract parser uses this so its behaviour and the serde
/// adapters' cannot drift: one rule, two entry points.
pub fn from_legacy_scalar(
    text: Option<&str>,
    number: Option<f64>,
    unit: LegacyUnit,
) -> Result<Option<Duration>, DurationParseError> {
    if let Some(t) = text {
        return t.parse().map(Some);
    }
    Ok(number.map(|n| unit.scale(n)))
}

#[cfg(test)]
mod compat_tests {
    use super::*;

    /// One type, one rule, two entry points. These moved here when the
    /// sched-local wrapper type was deleted — the tests that pin the
    /// deprecation behaviour belong beside the thing that implements it.
    #[derive(serde::Deserialize)]
    struct Holder {
        #[serde(default, alias = "budget_us", deserialize_with = "compat::opt_micros")]
        budget: Option<Duration>,
        #[serde(
            default,
            alias = "max_latency_ms",
            deserialize_with = "compat::opt_millis"
        )]
        max_latency: Option<Duration>,
    }

    #[test]
    fn compat_adapters_accept_both_spellings() {
        let new: Holder = serde_json::from_str(r#"{"budget":"8ms"}"#).unwrap();
        let old: Holder = serde_json::from_str(r#"{"budget_us":8000}"#).unwrap();
        assert_eq!(new.budget, old.budget);
        assert_eq!(new.budget.unwrap().as_micros(), 8_000);

        let new_ms: Holder = serde_json::from_str(r#"{"max_latency":"12ms"}"#).unwrap();
        let old_ms: Holder = serde_json::from_str(r#"{"max_latency_ms":12}"#).unwrap();
        assert_eq!(new_ms.max_latency, old_ms.max_latency);
        assert_eq!(new_ms.max_latency.unwrap().as_millis_f64(), 12.0);
    }

    /// The documented asymmetry: through serde a bare number takes the legacy
    /// unit, because `alias` cannot report which spelling matched. Pinned so
    /// the leniency is deliberate and visible, and so removing it at the W6
    /// sunset is a test change rather than a surprise.
    #[test]
    fn a_bare_number_takes_the_legacy_unit_through_serde() {
        let h: Holder = serde_json::from_str(r#"{"budget":50000}"#).unwrap();
        assert_eq!(
            h.budget.unwrap().as_millis_f64(),
            50.0,
            "bare number is microseconds"
        );
        let h2: Holder = serde_json::from_str(r#"{"max_latency":12}"#).unwrap();
        assert_eq!(
            h2.max_latency.unwrap().as_millis_f64(),
            12.0,
            "bare number is milliseconds"
        );
    }

    /// The non-serde entry point must agree with the serde one. The
    /// hand-rolled contract parser uses `from_legacy_scalar` because it needs
    /// source spans serde cannot provide, and the two must not drift.
    #[test]
    fn the_two_entry_points_agree() {
        let via_serde: Holder = serde_json::from_str(r#"{"budget_us":8000}"#).unwrap();
        let via_scalar = from_legacy_scalar(None, Some(8000.0), LegacyUnit::Micros).unwrap();
        assert_eq!(via_serde.budget, via_scalar);

        let text = from_legacy_scalar(Some("8ms"), None, LegacyUnit::Micros).unwrap();
        assert_eq!(
            text, via_scalar,
            "a unit in the value overrides the legacy unit"
        );
    }
}
