//! Renamed diagnostic rule ids (phase 98, `docs/design/standard-names.md`
//! section 5).
//!
//! A rule id is an interface: `acknowledged: [{ rule }]` names it and
//! `check --format json` emits it. A renamed rule kept its old id for one
//! release (v0.3.3-v0.3.5: an entry naming it still matched, warned as
//! `deprecated-spelling`); since v0.3.6 the consumer refuses an entry
//! naming an old id or an old path, by a message naming the new one, and
//! `migrate` rewrites both. Without this table an upgrade would have
//! turned every acknowledged finding of a renamed rule into `ack-stale`
//! plus an unacknowledged warning, and would now refuse with no name.

/// One renamed rule id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenamedRule {
    /// The id before the rename.
    pub old: &'static str,
    /// The id the rule emits now.
    pub new: &'static str,
    /// The grammar release that renamed it.
    pub since: &'static str,
    /// The old id is refused (an `acknowledged:` entry naming it is an
    /// error naming `new`), no longer matched. False for the one release
    /// after `since`; true from the next.
    pub refused: bool,
}

/// Every renamed rule id, oldest first.
pub const RENAMED_RULES: &[RenamedRule] = &[
    RenamedRule {
        old: "severity-unknown",
        new: "integrity-unknown",
        since: "0.3.3",
        refused: true,
    },
    RenamedRule {
        old: "severity-scale-conflict",
        new: "integrity-scale-conflict",
        since: "0.3.3",
        refused: true,
    },
    RenamedRule {
        old: "ladder-rung-budget",
        new: "degradation-level-budget",
        since: "0.3.3",
        refused: true,
    },
    RenamedRule {
        old: "ladder-unterminated",
        new: "fallback-without-mrc",
        since: "0.3.3",
        refused: true,
    },
    RenamedRule {
        old: "ladder-window-floor",
        new: "dwell-at-mrc",
        since: "0.3.3",
        refused: true,
    },
    RenamedRule {
        old: "call-unbounded-floor",
        new: "call-unbounded-mrc",
        since: "0.3.3",
        refused: true,
    },
    RenamedRule {
        old: "window-expiry",
        new: "dwell-expiry",
        since: "0.3.3",
        refused: true,
    },
    RenamedRule {
        old: "window-param",
        new: "dwell-param",
        since: "0.3.3",
        refused: true,
    },
    RenamedRule {
        old: "window-unbound",
        new: "dwell-unbound",
        since: "0.3.3",
        refused: true,
    },
    RenamedRule {
        old: "scope-budget",
        new: "chain-latency",
        since: "0.3.3",
        refused: true,
    },
    RenamedRule {
        old: "scope-sampling-feasibility",
        new: "chain-sampling-feasibility",
        since: "0.3.3",
        refused: true,
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
    /// As [`RenamedRule::refused`]: the old key is a `removed` row.
    pub refused: bool,
}

/// Every renamed key a diagnostic path can name (phase 98). An
/// `acknowledged:` entry's `at:` written with an old key still matches the
/// diagnostic at the new path this release.
pub const RENAMED_KEYS: &[RenamedKey] = &[
    RenamedKey {
        section: "",
        old: "severity_levels",
        new: "integrity_levels",
        refused: true,
    },
    RenamedKey {
        section: "hazards",
        old: "severity",
        new: "integrity",
        refused: true,
    },
    RenamedKey {
        section: "modes",
        old: "window",
        new: "min_dwell",
        refused: true,
    },
    // Phase 98 N6 (v0.3.7), accepted for one release.
    RenamedKey {
        section: "hazards",
        old: "guards",
        new: "monitors",
        refused: false,
    },
];

/// `at` with every renamed key segment replaced by its new name. A
/// segment is matched only where the grammar puts the key: directly at a
/// manifest level (the root, or under `includes.<name>.`) for a root key,
/// two segments under its section for an entry key. Names containing dots
/// are not split apart: the match is on the segments around them.
pub fn current_path(at: &str) -> String {
    rewrite_path(at, true)
}

fn rewrite_path(at: &str, refused_too: bool) -> String {
    let segs: Vec<&str> = at.split('.').collect();
    let mut out: Vec<String> = segs.iter().map(|s| s.to_string()).collect();
    for (i, seg) in segs.iter().enumerate() {
        // `guards[0]`: the key is the part before an index.
        let (base, index) = seg.split_at(seg.find('[').unwrap_or(seg.len()));
        for k in RENAMED_KEYS {
            if base != k.old || (k.refused && !refused_too) {
                continue;
            }
            let at_level = i == 0 || (i >= 2 && segs[i - 2] == "includes");
            let in_section = !k.section.is_empty()
                && i >= 2
                && segs[i - 2] == k.section
                && (i == 2 || segs[i - 3] == "includes" || i >= 4 && segs[i - 4] == "includes");
            if (k.section.is_empty() && at_level) || in_section {
                out[i] = format!("{}{index}", k.new);
            }
        }
    }
    out.join(".")
}

/// The id an `acknowledged:` entry naming `id` matches: the new id while
/// the old one is still accepted, else `id` itself (a refused old id
/// matches nothing, and the consumer reports it).
pub fn accepted(id: &str) -> std::borrow::Cow<'_, str> {
    match renamed(id) {
        Some(r) if !r.refused => current(id),
        _ => id.into(),
    }
}

/// As [`accepted`] for a path: only renames still accepted are applied.
pub fn accepted_path(at: &str) -> String {
    rewrite_path(at, false)
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
        assert_eq!(
            current_path("modes.tor.window.param"),
            "modes.tor.min_dwell.param"
        );
        assert_eq!(
            current_path("includes.sub.hazards.h.severity"),
            "includes.sub.hazards.h.integrity"
        );
        assert_eq!(
            current_path("includes.sub.severity_levels"),
            "includes.sub.integrity_levels"
        );
        // A node or topic that happens to be named like a key is not one.
        assert_eq!(current_path("nodes.window.paths.p"), "nodes.window.paths.p");
        assert_eq!(current_path("topics.severity"), "topics.severity");
        assert_eq!(current_path("nodes.n.paths.window"), "nodes.n.paths.window");
    }

    /// Phase 98 N6: an accepted rename applies to an indexed segment too,
    /// and to matching as well as to `migrate`.
    #[test]
    fn guards_in_a_path_reads_as_monitors_this_release() {
        assert_eq!(
            accepted_path("hazards.h.guards[1]"),
            "hazards.h.monitors[1]"
        );
        assert_eq!(current_path("hazards.h.guards"), "hazards.h.monitors");
        assert_eq!(accepted_path("topics.guards"), "topics.guards");
        // A refused rename is not accepted.
        assert_eq!(accepted_path("modes.m.window"), "modes.m.window");
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
