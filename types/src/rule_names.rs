//! Renamed diagnostic rule ids (phase 98, `docs/design/standard-names.md`
//! section 5).
//!
//! A rule id is an interface: `acknowledged: [{ rule }]` names it and
//! `check --format json` emits it. So a renamed rule keeps its old id for
//! one release: an `acknowledged:` entry naming the old id still matches
//! the renamed rule (and is warned as `deprecated-spelling`), and
//! `migrate` rewrites it. The release after refuses it, naming the new id.
//! Without this table an upgrade would turn every acknowledged finding of
//! a renamed rule into `ack-stale` plus an unacknowledged warning.

/// One renamed rule id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenamedRule {
    /// The id before the rename.
    pub old: &'static str,
    /// The id the rule emits now.
    pub new: &'static str,
    /// The grammar release that renamed it.
    pub since: &'static str,
}

/// Every renamed rule id, oldest first.
pub const RENAMED_RULES: &[RenamedRule] = &[
    RenamedRule {
        old: "severity-unknown",
        new: "integrity-unknown",
        since: "0.3.3",
    },
    RenamedRule {
        old: "severity-scale-conflict",
        new: "integrity-scale-conflict",
        since: "0.3.3",
    },
    RenamedRule {
        old: "ladder-rung-budget",
        new: "degradation-level-budget",
        since: "0.3.3",
    },
    RenamedRule {
        old: "ladder-unterminated",
        new: "fallback-without-mrc",
        since: "0.3.3",
    },
    RenamedRule {
        old: "ladder-window-floor",
        new: "dwell-at-mrc",
        since: "0.3.3",
    },
    RenamedRule {
        old: "call-unbounded-floor",
        new: "call-unbounded-mrc",
        since: "0.3.3",
    },
    RenamedRule {
        old: "window-expiry",
        new: "dwell-expiry",
        since: "0.3.3",
    },
    RenamedRule {
        old: "window-param",
        new: "dwell-param",
        since: "0.3.3",
    },
    RenamedRule {
        old: "window-unbound",
        new: "dwell-unbound",
        since: "0.3.3",
    },
    RenamedRule {
        old: "scope-budget",
        new: "chain-latency",
        since: "0.3.3",
    },
    RenamedRule {
        old: "scope-sampling-feasibility",
        new: "chain-sampling-feasibility",
        since: "0.3.3",
    },
];

/// A renamed key as a diagnostic path spells it: the segment `old` under a
/// map entry of `section` (`hazards.<h>.severity`), or at a manifest level
/// when `section` is empty (`severity_levels`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenamedKey {
    pub section: &'static str,
    pub old: &'static str,
    pub new: &'static str,
}

/// Every renamed key a diagnostic path can name (phase 98). An
/// `acknowledged:` entry's `at:` written with an old key still matches the
/// diagnostic at the new path this release.
pub const RENAMED_KEYS: &[RenamedKey] = &[
    RenamedKey {
        section: "",
        old: "severity_levels",
        new: "integrity_levels",
    },
    RenamedKey {
        section: "hazards",
        old: "severity",
        new: "integrity",
    },
    RenamedKey {
        section: "modes",
        old: "window",
        new: "min_dwell",
    },
];

/// `at` with every renamed key segment replaced by its new name. A
/// segment is matched only where the grammar puts the key: directly at a
/// manifest level (the root, or under `includes.<name>.`) for a root key,
/// two segments under its section for an entry key. Names containing dots
/// are not split apart: the match is on the segments around them.
pub fn current_path(at: &str) -> String {
    let segs: Vec<&str> = at.split('.').collect();
    let mut out: Vec<&str> = segs.clone();
    for (i, seg) in segs.iter().enumerate() {
        for k in RENAMED_KEYS {
            if *seg != k.old {
                continue;
            }
            let at_level = i == 0 || (i >= 2 && segs[i - 2] == "includes");
            let in_section = !k.section.is_empty()
                && i >= 2
                && segs[i - 2] == k.section
                && (i == 2 || segs[i - 3] == "includes" || i >= 4 && segs[i - 4] == "includes");
            if (k.section.is_empty() && at_level) || in_section {
                out[i] = k.new;
            }
        }
    }
    out.join(".")
}

/// The rule id `id` names now: its new id when it was renamed, else itself.
/// A per-mode finding's id (`mode:<rule>`) keeps its prefix.
pub fn current(id: &str) -> std::borrow::Cow<'_, str> {
    match renamed(id) {
        Some(r) => match id.strip_prefix("mode:") {
            Some(_) => format!("mode:{}", r.new).into(),
            None => r.new.into(),
        },
        None => id.into(),
    }
}

/// The rename `id` is the old side of, if any (a `mode:` prefix is
/// looked through).
pub fn renamed(id: &str) -> Option<&'static RenamedRule> {
    let bare = id.strip_prefix("mode:").unwrap_or(id);
    RENAMED_RULES.iter().find(|r| r.old == bare)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_old_id_maps_to_its_new_one_and_others_to_themselves() {
        assert_eq!(current("severity-unknown"), "integrity-unknown");
        assert_eq!(current("integrity-unknown"), "integrity-unknown");
        assert_eq!(current("scope-budget-x"), "scope-budget-x");
        assert_eq!(current("mode:scope-budget"), "mode:chain-latency");
    }

    #[test]
    fn a_renamed_key_in_a_path_is_rewritten_where_the_grammar_puts_it() {
        assert_eq!(current_path("hazards.h.severity"), "hazards.h.integrity");
        assert_eq!(current_path("severity_levels"), "integrity_levels");
        assert_eq!(current_path("modes.tor.window"), "modes.tor.min_dwell");
        assert_eq!(current_path("modes.tor.window.param"), "modes.tor.min_dwell.param");
        assert_eq!(
            current_path("includes.sub.hazards.h.severity"),
            "includes.sub.hazards.h.integrity"
        );
        assert_eq!(current_path("includes.sub.severity_levels"), "includes.sub.integrity_levels");
        // A node or topic that happens to be named like a key is not one.
        assert_eq!(current_path("nodes.window.paths.p"), "nodes.window.paths.p");
        assert_eq!(current_path("topics.severity"), "topics.severity");
        assert_eq!(current_path("nodes.n.paths.window"), "nodes.n.paths.window");
    }

    #[test]
    fn no_new_id_is_another_rename_s_old_id_and_none_repeats() {
        for r in RENAMED_RULES {
            assert!(renamed(r.new).is_none(), "{} -> {} chains", r.old, r.new);
            assert_eq!(
                RENAMED_RULES.iter().filter(|o| o.old == r.old).count(),
                1,
                "{} renamed twice",
                r.old
            );
        }
    }
}
