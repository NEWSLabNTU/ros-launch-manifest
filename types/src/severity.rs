//! Severity to criticality: the map a node's derived scheduling criticality
//! is read through (phase 90 G5).
//!
//! A hazard declares a `severity` on the contract's ordered
//! `severity_levels` scale (ISO 26262's ASIL list when none is declared). A
//! node the hazard reaches takes a criticality from it, on the closed set the
//! scheduler mappers rank by: `low`, `medium`, `high`. The map used to live
//! only in the consumer's code; it is grammar, so it is stated here, and the
//! Format Reference prints it ([`render_markdown`]).
//!
//! The rule: rank 0 (the scale's first entry) is "no safety requirement" and
//! derives nothing; the remaining `n = len - 1` entries split evenly into
//! three buckets by `ceil(3 rank / n)`: 1 is `low`, 2 is `medium`, 3 is
//! `high`. On the default scale: QM none, ASIL_A low, ASIL_B medium, ASIL_C
//! and ASIL_D high.

/// The severity scale a contract that declares no `severity_levels` draws
/// from, ascending: ISO 26262's. Writing it out changes nothing.
pub const DEFAULT_SEVERITY_LEVELS: [&str; 5] = ["QM", "ASIL_A", "ASIL_B", "ASIL_C", "ASIL_D"];

/// The criticality labels, ascending: the closed set of
/// `nodes.<n>.criticality` and of the scheduler mappers' rank.
pub const CRITICALITY_LEVELS: [&str; 3] = ["low", "medium", "high"];

/// The criticality a severity of `rank` on a scale of `scale_len` entries
/// derives, as one of [`CRITICALITY_LEVELS`]; `None` for rank 0 (no safety
/// requirement) and for a scale of fewer than two entries.
pub fn criticality_of(rank: usize, scale_len: usize) -> Option<&'static str> {
    let n = scale_len.saturating_sub(1);
    if rank == 0 || n == 0 {
        return None;
    }
    Some(match (rank * 3).div_ceil(n) {
        0 | 1 => CRITICALITY_LEVELS[0],
        2 => CRITICALITY_LEVELS[1],
        _ => CRITICALITY_LEVELS[2],
    })
}

/// The map over a whole scale: each level with the criticality it derives.
pub fn criticality_map<S: AsRef<str>>(scale: &[S]) -> Vec<(&str, Option<&'static str>)> {
    scale
        .iter()
        .enumerate()
        .map(|(rank, level)| (level.as_ref(), criticality_of(rank, scale.len())))
        .collect()
}

/// The Format Reference section that states the map.
pub fn render_markdown() -> String {
    let mut out = String::from("## Severity to criticality\n\n");
    out.push_str(
        "A node a hazard reaches (it feeds, detects for, or reacts for the hazard) \
         takes a scheduling criticality from the hazard's `severity`, the MAX over \
         every hazard that reaches it. The first entry of `severity_levels` is \"no \
         safety requirement\" and derives nothing; the other `n` entries split into \
         three buckets by `ceil(3 rank / n)` (1 low, 2 medium, 3 high). A written \
         `criticality` on such a node is an error; on a node no hazard reaches it \
         stands. The default scale (no `severity_levels` declared):\n\n",
    );
    out.push_str("| severity | criticality |\n|---|---|\n");
    for (level, crit) in criticality_map(&DEFAULT_SEVERITY_LEVELS) {
        out.push_str(&format!("| `{level}` | {} |\n", crit.unwrap_or("none")));
    }
    out.push('\n');
    out.push_str(
        "A contract that uses a profile (phase 91 E6; `use: { iec61508: \"^1\" }`) draws \
         severities from the profile's `severity_scale` and takes each level's criticality \
         from the profile's `criticality` map, stated level by level, instead of the split \
         above; it may not also write `severity_levels:`. Reference profiles ship with the \
         checker: ",
    );
    let names: Vec<String> = crate::profile::reference()
        .iter()
        .map(|p| format!("`{}`", p.name))
        .collect();
    out.push_str(&names.join(", "));
    out.push_str(" (`iso26262` is the default scale and map above).\n\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_scale_maps_as_documented() {
        assert_eq!(
            criticality_map(&DEFAULT_SEVERITY_LEVELS),
            vec![
                ("QM", None),
                ("ASIL_A", Some("low")),
                ("ASIL_B", Some("medium")),
                ("ASIL_C", Some("high")),
                ("ASIL_D", Some("high")),
            ]
        );
    }

    #[test]
    fn other_scales_split_into_thirds() {
        // IEC 61508: none + SIL 1..4, the same shape as the ASIL list.
        let sil = ["none", "SIL_1", "SIL_2", "SIL_3", "SIL_4"];
        let got: Vec<_> = criticality_map(&sil).into_iter().map(|(_, c)| c).collect();
        assert_eq!(
            got,
            vec![
                None,
                Some("low"),
                Some("medium"),
                Some("high"),
                Some("high")
            ]
        );
        // Two levels: the one requirement is the top bucket.
        assert_eq!(criticality_of(1, 2), Some("high"));
        // One level, or rank 0: nothing.
        assert_eq!(criticality_of(0, 5), None);
        assert_eq!(criticality_of(1, 1), None);
    }
}
