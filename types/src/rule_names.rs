//! Renamed diagnostic rule ids (phase 98, `docs/design/standard-names.md`
//! section 5).
//!
//! A rule id is an interface: `acknowledged: [{ rule }]` names it and
//! `check --format json` emits it. Every rename goes through three
//! [`Stage`]s, one release each (design doc section 6): an entry naming
//! the old id or an old path still matches, warned as `deprecated-spelling`
//! ([`Stage::Accepted`], release R); then it is an error naming the new one
//! ([`Stage::Refused`], R+1); then the message is dropped and the entry
//! reads as any other id that matches nothing, `ack-stale`
//! ([`Stage::Retired`], R+2). `migrate` rewrites old ids and paths at every
//! stage, for good. Without this table an upgrade would turn every
//! acknowledged finding of a renamed rule into `ack-stale` plus an
//! unacknowledged warning with no word of where the id went.

/// Where a rename is in its retirement (design doc section 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Release R: the old spelling still matches, warned.
    Accepted,
    /// Release R+1: the old spelling is an error naming the new one.
    Refused,
    /// Release R+2 and later: no message; the old spelling matches
    /// nothing. Only `migrate` still maps it.
    Retired,
}

/// One renamed rule id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenamedRule {
    /// The id before the rename.
    pub old: &'static str,
    /// The id the rule emits now.
    pub new: &'static str,
    /// The grammar release that renamed it.
    pub since: &'static str,
    /// Where the rename is in its retirement.
    pub stage: Stage,
}

/// Every renamed rule id, oldest first.
pub const RENAMED_RULES: &[RenamedRule] = &[
    RenamedRule {
        old: "severity-unknown",
        new: "integrity-unknown",
        since: "0.3.3",
        stage: Stage::Retired,
    },
    RenamedRule {
        old: "severity-scale-conflict",
        new: "integrity-scale-conflict",
        since: "0.3.3",
        stage: Stage::Retired,
    },
    RenamedRule {
        old: "ladder-rung-budget",
        new: "degradation-level-time",
        since: "0.3.3",
        stage: Stage::Retired,
    },
    RenamedRule {
        old: "ladder-unterminated",
        new: "fallback-without-mrc",
        since: "0.3.3",
        stage: Stage::Retired,
    },
    RenamedRule {
        old: "ladder-window-floor",
        new: "dwell-at-mrc",
        since: "0.3.3",
        stage: Stage::Retired,
    },
    RenamedRule {
        old: "call-unbounded-floor",
        new: "call-unbounded-mrc",
        since: "0.3.3",
        stage: Stage::Retired,
    },
    RenamedRule {
        old: "window-expiry",
        new: "dwell-expiry",
        since: "0.3.3",
        stage: Stage::Retired,
    },
    RenamedRule {
        old: "window-param",
        new: "dwell-param",
        since: "0.3.3",
        stage: Stage::Retired,
    },
    RenamedRule {
        old: "window-unbound",
        new: "dwell-unbound",
        since: "0.3.3",
        stage: Stage::Retired,
    },
    RenamedRule {
        old: "scope-budget",
        new: "chain-latency",
        since: "0.3.3",
        stage: Stage::Retired,
    },
    RenamedRule {
        old: "scope-sampling-feasibility",
        new: "chain-sampling-feasibility",
        since: "0.3.3",
        stage: Stage::Retired,
    },
    // Phase 98 N6 M3 (v0.3.8): "budget" in the sense of a time limit is a
    // deadline or a bound; `budget` stays the reservation's CPU time.
    // Accepted in v0.3.8, refused since v0.3.9.
    RenamedRule {
        old: "fault-reaction-budget",
        new: "fault-reaction-time",
        since: "0.3.8",
        stage: Stage::Refused,
    },
    RenamedRule {
        old: "degradation-level-budget",
        new: "degradation-level-time",
        since: "0.3.8",
        stage: Stage::Refused,
    },
    RenamedRule {
        old: "budget-overflow",
        new: "latency-bound-overflow",
        since: "0.3.8",
        stage: Stage::Refused,
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
    /// As [`RenamedRule::stage`]; from [`Stage::Refused`] on the old key is
    /// a `removed` row.
    pub stage: Stage,
}

/// Every renamed key a diagnostic path can name (phase 98). An
/// `acknowledged:` entry's `at:` written with an old key is treated as its
/// rename's [`Stage`] says.
pub const RENAMED_KEYS: &[RenamedKey] = &[
    RenamedKey {
        section: "",
        old: "severity_levels",
        new: "integrity_levels",
        stage: Stage::Retired,
    },
    RenamedKey {
        section: "hazards",
        old: "severity",
        new: "integrity",
        stage: Stage::Retired,
    },
    RenamedKey {
        section: "modes",
        old: "window",
        new: "min_dwell",
        stage: Stage::Retired,
    },
    // Phase 98 N6 (v0.3.7), refused since v0.3.9.
    RenamedKey {
        section: "hazards",
        old: "guards",
        new: "monitors",
        stage: Stage::Refused,
    },
];

/// `at` with every renamed key segment replaced by its new name. A
/// segment is matched only where the grammar puts the key: directly at a
/// manifest level (the root, or under `includes.<name>.`) for a root key,
/// two segments under its section for an entry key. Names containing dots
/// are not split apart: the match is on the segments around them.
pub fn current_path(at: &str) -> String {
    rewrite_path(at, |_| true)
}

fn rewrite_path(at: &str, applies: impl Fn(Stage) -> bool) -> String {
    let segs: Vec<&str> = at.split('.').collect();
    let mut out: Vec<String> = segs.iter().map(|s| s.to_string()).collect();
    for (i, seg) in segs.iter().enumerate() {
        // `guards[0]`: the key is the part before an index.
        let (base, index) = seg.split_at(seg.find('[').unwrap_or(seg.len()));
        for k in RENAMED_KEYS {
            if base != k.old || !applies(k.stage) {
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
/// the old one is still accepted, else `id` itself (a refused or retired
/// old id matches nothing).
pub fn accepted(id: &str) -> std::borrow::Cow<'_, str> {
    match renamed(id) {
        Some(r) if r.stage == Stage::Accepted => current(id),
        _ => id.into(),
    }
}

/// As [`accepted`] for a path: only renames still accepted are applied.
pub fn accepted_path(at: &str) -> String {
    rewrite_path(at, |s| s == Stage::Accepted)
}

/// The id a consumer tells an `acknowledged:` entry naming `id` to write:
/// the new id while the rename is accepted or refused, else `id` itself
/// (a retired old id gets no message; it matches nothing, `ack-stale`).
pub fn noticed(id: &str) -> std::borrow::Cow<'_, str> {
    match renamed(id) {
        Some(r) if r.stage != Stage::Retired => current(id),
        _ => id.into(),
    }
}

/// As [`noticed`] for a path.
pub fn noticed_path(at: &str) -> String {
    rewrite_path(at, |s| s != Stage::Retired)
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

    /// Phase 98 N6 M3 at R+1 (v0.3.9): the budget-as-limit ids are
    /// refused (noticed, not matched); the S1-S5 ids are retired (neither);
    /// `migrate`'s `current` maps every one, and the older
    /// `ladder-rung-budget` names the newest id directly.
    #[test]
    fn each_rename_is_matched_noticed_or_neither_by_its_stage() {
        for r in RENAMED_RULES {
            let matched = accepted(r.old) != r.old;
            let noticed_ = noticed(r.old) != r.old;
            assert_eq!(current(r.old), r.new, "{}", r.old);
            match r.stage {
                Stage::Accepted => assert!(matched && noticed_, "{}", r.old),
                Stage::Refused => assert!(!matched && noticed_, "{}", r.old),
                Stage::Retired => assert!(!matched && !noticed_, "{}", r.old),
            }
        }
        assert_eq!(noticed("fault-reaction-budget"), "fault-reaction-time");
        assert_eq!(
            noticed("mode:degradation-level-budget"),
            "mode:degradation-level-time"
        );
        assert_eq!(noticed("budget-overflow"), "latency-bound-overflow");
        assert_eq!(accepted("budget-overflow"), "budget-overflow");
        assert_eq!(current("ladder-rung-budget"), "degradation-level-time");
        assert_eq!(noticed("ladder-rung-budget"), "ladder-rung-budget");
        assert_eq!(noticed("scope-budget"), "scope-budget");
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

    /// Phase 98 N6: a rename applies to an indexed segment too. `guards`
    /// is refused since v0.3.9 (noticed, not matched); `window` is retired.
    #[test]
    fn guards_in_a_path_is_noticed_and_window_is_retired() {
        assert_eq!(noticed_path("hazards.h.guards[1]"), "hazards.h.monitors[1]");
        assert_eq!(accepted_path("hazards.h.guards[1]"), "hazards.h.guards[1]");
        assert_eq!(current_path("hazards.h.guards"), "hazards.h.monitors");
        assert_eq!(noticed_path("topics.guards"), "topics.guards");
        assert_eq!(noticed_path("modes.m.window"), "modes.m.window");
        assert_eq!(current_path("modes.m.window"), "modes.m.min_dwell");
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
