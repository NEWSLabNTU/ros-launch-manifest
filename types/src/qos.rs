//! The QoS enum fields of a contract: their ROS 2 values, the one test of a
//! written value, and the DDS offered-vs-requested match (play_launch issue
//! #0083).
//!
//! `reliability`, `durability`, `history` and `liveliness` used to be read
//! as any string, and both copies of the match (this crate's `qos-match`,
//! play_launch's cross-scope one) passed a pair they did not recognise. So a
//! typo (`transient-local`) or an unevaluated `$(var ...)` reached the match
//! and every pub/sub pair on the topic was "compatible". The value sets are
//! stated here once and every reader takes them from here.
//!
//! A value is a literal from the field's set, or exactly `$(var <arg>)`
//! naming an arg of the contract's `args:`. The parser refuses anything
//! else, and refuses an arg whose `choices:` hold a value outside the set;
//! [`crate::subst::substitute_manifest`] evaluates the reference per scope
//! and tests the value it produced.

/// One of the four enum-valued QoS keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QosField {
    Reliability,
    Durability,
    History,
    Liveliness,
}

/// `rmw_qos_reliability_policy_e`, without `UNKNOWN` (a query result, not a
/// setting).
pub const RELIABILITY: &[&str] = &[
    "reliable",
    "best_effort",
    "system_default",
    "best_available",
];
/// `rmw_qos_durability_policy_e`, without `UNKNOWN`.
pub const DURABILITY: &[&str] = &[
    "volatile",
    "transient_local",
    "system_default",
    "best_available",
];
/// `rmw_qos_history_policy_e`, without `UNKNOWN`. ROS defines no
/// `best_available` history.
pub const HISTORY: &[&str] = &["keep_last", "keep_all", "system_default"];
/// `rmw_qos_liveliness_policy_e`, without `UNKNOWN` and the deprecated
/// `MANUAL_BY_NODE` (refused by rmw since Galactic).
pub const LIVELINESS: &[&str] = &[
    "automatic",
    "manual_by_topic",
    "system_default",
    "best_available",
];

impl QosField {
    pub const ALL: [QosField; 4] = [
        QosField::Reliability,
        QosField::Durability,
        QosField::History,
        QosField::Liveliness,
    ];

    /// The key as written under `qos:`.
    pub fn key(self) -> &'static str {
        match self {
            QosField::Reliability => "reliability",
            QosField::Durability => "durability",
            QosField::History => "history",
            QosField::Liveliness => "liveliness",
        }
    }

    /// The values ROS 2 defines for the field.
    pub fn allowed(self) -> &'static [&'static str] {
        match self {
            QosField::Reliability => RELIABILITY,
            QosField::Durability => DURABILITY,
            QosField::History => HISTORY,
            QosField::Liveliness => LIVELINESS,
        }
    }

    /// The value of this field in `qos`.
    pub fn get(self, qos: &crate::QosDecl) -> Option<&String> {
        match self {
            QosField::Reliability => qos.reliability.as_ref(),
            QosField::Durability => qos.durability.as_ref(),
            QosField::History => qos.history.as_ref(),
            QosField::Liveliness => qos.liveliness.as_ref(),
        }
    }

    /// The value of this field in `qos`, to replace.
    pub fn get_mut(self, qos: &mut crate::QosDecl) -> &mut Option<String> {
        match self {
            QosField::Reliability => &mut qos.reliability,
            QosField::Durability => &mut qos.durability,
            QosField::History => &mut qos.history,
            QosField::Liveliness => &mut qos.liveliness,
        }
    }

    /// The allowed set for a message: `volatile, transient_local, ...`.
    pub fn allowed_list(self) -> String {
        self.allowed().join(", ")
    }

    /// Whether `value` is one of the field's values.
    pub fn is_allowed(self, value: &str) -> bool {
        self.allowed().contains(&value)
    }

    /// `Err` naming the value and the allowed set when `value` is not one of
    /// the field's values.
    pub fn check(self, value: &str) -> Result<(), String> {
        if self.is_allowed(value) {
            return Ok(());
        }
        Err(format!(
            "`{}: {value}` is not a ROS 2 {} value; accepted: {}",
            self.key(),
            self.key(),
            self.allowed_list()
        ))
    }
}

/// What a written enum value is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Written<'a> {
    /// A literal, not yet tested against the set.
    Literal(&'a str),
    /// Exactly `$(var <name>)`.
    Var(&'a str),
    /// Any other substitution text (`$(param ...)`, `$(var x @n)`, text
    /// around a reference).
    Other,
}

/// Classify a written enum value.
pub fn classify(value: &str) -> Written<'_> {
    if !value.contains("$(") {
        return Written::Literal(value);
    }
    let v = value.trim();
    if let Some(inner) = v.strip_prefix("$(var ").and_then(|r| r.strip_suffix(')')) {
        let name = inner.trim();
        if !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Written::Var(name);
        }
    }
    Written::Other
}

/// The outcome of matching an offered value against a requested one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Match {
    /// DDS matches the pair.
    Compatible,
    /// DDS refuses the pair.
    Incompatible,
    /// The pair depends on the RMW: one side is `system_default`, whose
    /// value is the middleware's, not the contract's. Carries the reason.
    RmwDefined(String),
}

/// Rank of a value on a field's "offered >= requested" order.
fn rank(field: QosField, v: &str) -> Option<u8> {
    match (field, v) {
        (QosField::Reliability, "best_effort") => Some(0),
        (QosField::Reliability, "reliable") => Some(1),
        (QosField::Durability, "volatile") => Some(0),
        (QosField::Durability, "transient_local") => Some(1),
        (QosField::Liveliness, "automatic") => Some(0),
        (QosField::Liveliness, "manual_by_topic") => Some(1),
        _ => None,
    }
}

/// The DDS offered >= requested match on one field of one (publisher,
/// subscriber) pair. `Err` when either value is not one of the field's
/// values: an unknown value is never compatible, it is unchecked, and the
/// message says so. `history` has no match (DDS does not compare it); it
/// answers `Compatible` for two valid values.
pub fn compatible(field: QosField, offered: &str, requested: &str) -> Result<Match, String> {
    for (side, v) in [("offered", offered), ("requested", requested)] {
        if !field.is_allowed(v) {
            let what = match classify(v) {
                Written::Literal(_) => "is not a ROS 2 value".to_string(),
                _ => "is a reference that was not evaluated".to_string(),
            };
            return Err(format!(
                "{side} {} '{v}' {what} (accepted: {}), so the pair cannot be matched",
                field.key(),
                field.allowed_list()
            ));
        }
    }
    if field == QosField::History {
        return Ok(Match::Compatible);
    }
    // BEST_AVAILABLE takes the strictest value that still matches every
    // endpoint on the other side (rmw `qos_profiles.h`), so it never makes a
    // pair incompatible.
    if offered == "best_available" || requested == "best_available" {
        return Ok(Match::Compatible);
    }
    if offered == "system_default" || requested == "system_default" {
        return Ok(Match::RmwDefined(format!(
            "{} 'system_default' is the RMW's value, not the contract's: the pair \
             (offered '{offered}', requested '{requested}') is not checked; state the value",
            field.key()
        )));
    }
    let (o, r) = (rank(field, offered), rank(field, requested));
    match (o, r) {
        (Some(o), Some(r)) if o >= r => Ok(Match::Compatible),
        (Some(_), Some(_)) => Ok(Match::Incompatible),
        // Unreachable: every allowed value but the two handled above ranks.
        _ => Err(format!(
            "{} pair (offered '{offered}', requested '{requested}') has no rank",
            field.key()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sets_are_the_ros_2_values() {
        assert!(QosField::Durability.check("transient_local").is_ok());
        assert!(QosField::Durability.check("best_available").is_ok());
        assert!(QosField::History.check("system_default").is_ok());
        assert!(QosField::History.check("best_available").is_err());
        assert!(QosField::Liveliness.check("manual_by_node").is_err());
        let e = QosField::Durability.check("transient-local").unwrap_err();
        assert!(e.contains("transient-local") && e.contains("volatile, transient_local"));
    }

    #[test]
    fn classifies_references() {
        assert_eq!(classify("volatile"), Written::Literal("volatile"));
        assert_eq!(classify("$(var dur)"), Written::Var("dur"));
        assert_eq!(classify(" $(var  dur ) "), Written::Var("dur"));
        assert_eq!(classify("$(param dur)"), Written::Other);
        assert_eq!(classify("$(var dur @n)"), Written::Other);
        assert_eq!(classify("x$(var dur)"), Written::Other);
    }

    #[test]
    fn the_match() {
        use Match::*;
        let d = QosField::Durability;
        assert_eq!(compatible(d, "transient_local", "volatile"), Ok(Compatible));
        assert_eq!(
            compatible(d, "volatile", "transient_local"),
            Ok(Incompatible)
        );
        assert_eq!(
            compatible(d, "best_available", "transient_local"),
            Ok(Compatible)
        );
        assert!(matches!(
            compatible(d, "system_default", "volatile"),
            Ok(RmwDefined(_))
        ));
        // An unknown value is never compatible.
        let e = compatible(d, "transient-local", "volatile").unwrap_err();
        assert!(e.contains("transient-local") && e.contains("accepted"));
        let e = compatible(d, "volatile", "$(var x)").unwrap_err();
        assert!(e.contains("not evaluated"));
        let r = QosField::Reliability;
        assert_eq!(compatible(r, "best_effort", "reliable"), Ok(Incompatible));
        let l = QosField::Liveliness;
        assert_eq!(
            compatible(l, "automatic", "manual_by_topic"),
            Ok(Incompatible)
        );
        assert_eq!(
            compatible(l, "manual_by_topic", "automatic"),
            Ok(Compatible)
        );
    }
}
