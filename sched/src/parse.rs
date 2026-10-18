//! TOML parsing for the system scheduling document.

use crate::{resolve::SchedError, types::SystemSched};

/// Parse a system scheduling document from a TOML string.
pub fn parse_system_sched(input: &str) -> Result<SystemSched, SchedError> {
    toml::from_str(input).map_err(|e| SchedError::Parse(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_generic_head_and_platform_subtable() {
        let src = r#"
[tiers.control]
class = "real_time"
deadline_us = 50000
period_us = 20000

[tiers.control.posix]
priority = 80
sched_class = "SCHED_FIFO"
core = 1

[tiers.control.freertos]
priority = 12
stack_bytes = 8192
deadline_us = 40000

[[assign]]
tier = "control"
nodes = ["ndt_localizer", "ekf_localizer"]

[[assign]]
tier = "perception"
scope = "/perception/lidar"
"#;
        let sched = parse_system_sched(src).expect("must parse");
        let control = sched.tiers.get("control").expect("control tier");
        assert_eq!(control.class.as_deref(), Some("real_time"));
        // The input still uses the deprecated `deadline_us` spelling, so this
        // asserts the phase-59 alias as well as the value. 50000us unchanged.
        assert_eq!(control.deadline.unwrap().as_micros(), 50_000);
        assert_eq!(control.platform("posix").unwrap().priority, 80);
        assert_eq!(
            control.platform("posix").unwrap().sched_class.as_deref(),
            Some("SCHED_FIFO")
        );
        assert_eq!(
            control
                .platform("freertos")
                .unwrap()
                .deadline
                .unwrap()
                .as_micros(),
            40_000
        );
        assert_eq!(sched.assign.len(), 2);
        assert_eq!(
            sched.assign[0].nodes,
            vec!["ndt_localizer", "ekf_localizer"]
        );
        assert_eq!(sched.assign[1].scope.as_deref(), Some("/perception/lidar"));
    }

    #[test]
    fn priority_number_on_generic_head_is_rejected() {
        // deny_unknown_fields: a bare `priority` at the tier head must error —
        // this is what protects portability.
        let src = r#"
[tiers.control]
priority = 80
"#;
        let err = parse_system_sched(src).unwrap_err();
        assert!(matches!(err, SchedError::Parse(_)));
    }

    #[test]
    fn above_names_a_reserved_band_on_the_head() {
        use crate::ReservedBand;
        let src = r#"
[tiers.safety]
period = "1ms"
above = "transport"
"#;
        let sched = parse_system_sched(src).expect("parses");
        let t = &sched.tiers["safety"];
        assert_eq!(t.above, Some(ReservedBand::Transport));
        assert_eq!(t.above.unwrap().as_str(), "transport");
        // Serializes back as written; absent stays absent.
        let back = toml::to_string(&sched).unwrap();
        assert!(back.contains("above = \"transport\""), "{back}");
        let plain = parse_system_sched("[tiers.x]\nperiod = \"1ms\"\n").unwrap();
        assert_eq!(plain.tiers["x"].above, None);
        assert!(!toml::to_string(&plain).unwrap().contains("above"));
    }

    #[test]
    fn above_an_unknown_band_is_rejected() {
        // The band names are a closed set: a typo is a parse error naming the
        // one band a tier may name, never a tier silently left in the pool.
        let src = r#"
[tiers.safety]
above = "trasnport"
"#;
        let err = parse_system_sched(src).unwrap_err();
        let SchedError::Parse(msg) = &err else {
            panic!("{err:?}")
        };
        assert!(msg.contains("transport"), "{msg}");
    }

    #[test]
    fn above_in_a_platform_subtable_is_rejected() {
        // It is a system fact, stated once on the head.
        let src = r#"
[tiers.safety.zephyr]
priority = 3
above = "transport"
"#;
        assert!(matches!(
            parse_system_sched(src).unwrap_err(),
            SchedError::Parse(_)
        ));
    }

    #[test]
    fn empty_document_is_valid_and_default() {
        let sched = parse_system_sched("").expect("empty parses");
        assert!(sched.tiers.is_empty());
        assert!(sched.assign.is_empty());
    }
}
