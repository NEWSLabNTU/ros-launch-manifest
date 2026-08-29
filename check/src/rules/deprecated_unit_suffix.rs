//! Rule: nudge timing fields off unit-suffixed names (phase 59 / 63).
//!
//! `budget_us: 8` when the author meant 8 ms is a thousandfold error that
//! type-checks, and it flows into a scheduling parameter the kernel admits or
//! rejects. `budget: 8ms` makes it unrepresentable. Both spellings parse
//! during the deprecation window; this is what tells an author the old one is
//! on the way out.
//!
//! Info severity, following the `explicit-trigger` precedent: an un-migrated
//! contract is valid, not broken, and a warning would punish people for a
//! rename they have not been asked to do yet. It becomes an error at the
//! phase-63 W6 sunset, which is what the contract `version:` lever is for.
//!
//! # Why it reads the span index rather than the parsed manifest
//!
//! By the time a `Manifest` exists, both spellings have become the same
//! `Duration` — deliberately, since that is what makes an un-migrated contract
//! behave identically. The information about WHICH name a document used
//! survives only in the source, and `SpanIndex` already maps every document
//! key to its byte range. So the lint asks the index, which also gives the
//! diagnostic a location for free.
//!
//! An earlier design threaded a "was legacy" flag out of the parser. That
//! coupled the lint to the parse path, and the flag had to survive every
//! call site; reading the index needs neither.

use super::ValidationRule;
use crate::{CheckContext, Severity, graph::DataflowGraph};
use ros_launch_manifest_types::Manifest;

/// Deprecated field name → its phase-59 replacement.
///
/// Only TIME-valued fields. Rate fields (`rate_hz`, `min_rate_hz`,
/// `max_rate_hz`) keep their suffix on purpose: frequency is not a duration,
/// it would need its own unit set, and the error it prevents is far cheaper.
const RENAMED: &[(&str, &str)] = &[
    ("max_latency_ms", "max_latency"),
    ("max_age_ms", "max_age"),
    ("max_transport_ms", "max_transport"),
    ("max_response_ms", "max_response"),
    ("max_interval_ms", "max_interval"),
    ("timeout_ms", "timeout"),
    ("tolerance_ms", "tolerance"),
    ("lifespan_ms", "lifespan"),
    ("deadline_us", "deadline"),
    ("period_us", "period"),
    ("budget_us", "budget"),
    ("spin_period_us", "spin_period"),
    ("time_slice_us", "time_slice"),
];

pub struct DeprecatedUnitSuffixRule;

impl ValidationRule for DeprecatedUnitSuffixRule {
    fn id(&self) -> &str {
        "deprecated-unit-suffix"
    }

    fn check(&self, _manifest: &Manifest, _graph: &DataflowGraph, ctx: &mut CheckContext) {
        // Without spans there is no source to inspect, and guessing from the
        // parsed manifest is impossible by construction. Staying silent is
        // correct: a missing index means the caller did not ask for source
        // locations, not that the contract is clean.
        let Some(spans) = ctx.spans.as_ref() else {
            return;
        };

        let mut found: Vec<(String, &str, &str)> = Vec::new();
        for path in spans.spans.keys() {
            let mut segs = path.rsplit('.');
            let leaf = segs.next().unwrap_or(path);
            let parent = segs.next().unwrap_or("");
            // A schema field never sits directly under a NAME map. Endpoint
            // and topic names are author-chosen and can be anything — this
            // corpus has topics ending in `processing_time_ms` — so matching
            // a leaf without checking its position would report a ROS topic
            // as a deprecated field. Not hypothetical: a first pass over 77
            // Autoware contracts produced 70 such hits.
            if matches!(
                parent,
                "pub" | "sub" | "srv" | "cli" | "topics" | "external_topics"
            ) {
                continue;
            }
            if let Some((old, new)) = RENAMED.iter().find(|(o, _)| *o == leaf) {
                found.push((path.clone(), old, new));
            }
        }
        // Deterministic order: the index is a HashMap, and a diagnostic list
        // that reorders between runs is unreadable in a diff.
        found.sort();

        for (path, old, new) in found {
            let unit = if old.ends_with("_us") { "us" } else { "ms" };
            ctx.emit(
                self.id(),
                Severity::Info,
                &path,
                format!(
                    "`{old}` is deprecated — write `{new}` with the unit in the value \
                     (for example `{new}: 12{unit}`). The unit in a NAME is what lets \
                     a value be 1000x wrong and still parse; both spellings work for now"
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ros_launch_manifest_types::{SpanIndex, parse_manifest_str};

    fn run(yaml: &str) -> Vec<crate::Diagnostic> {
        let manifest = parse_manifest_str(yaml).expect("fixture should parse");
        let graph = DataflowGraph::build(&manifest);
        let mut ctx = CheckContext::with_spans(SpanIndex::build(yaml));
        DeprecatedUnitSuffixRule.check(&manifest, &graph, &mut ctx);
        ctx.diagnostics
    }

    #[test]
    fn flags_a_deprecated_name_and_names_its_replacement() {
        let d = run("nodes:\n  a:\n    paths:\n      main:\n        max_latency_ms: 50\n");
        assert_eq!(d.len(), 1, "{d:?}");
        assert!(
            d[0].message.contains("`max_latency_ms` is deprecated"),
            "{:?}",
            d[0]
        );
        assert!(d[0].message.contains("max_latency: 12ms"), "{:?}", d[0]);
        assert_eq!(
            d[0].severity,
            Severity::Info,
            "un-migrated is valid, not broken"
        );
    }

    /// The migrated spelling must be silent, or the lint would punish exactly
    /// the authors who did the work.
    #[test]
    fn says_nothing_about_a_migrated_contract() {
        let d = run("nodes:\n  a:\n    paths:\n      main:\n        max_latency: 50ms\n");
        assert!(d.is_empty(), "{d:?}");
    }

    /// Rate fields keep their suffix by design — frequency is not a duration.
    #[test]
    fn leaves_rate_fields_alone() {
        let d = run("nodes:\n  a:\n    pub:\n      t:\n        min_rate_hz: 50\n");
        assert!(d.is_empty(), "rate fields are out of scope: {d:?}");
    }

    /// A topic or endpoint NAME that happens to end in a unit suffix is not a
    /// deprecated field. The Autoware corpus publishes
    /// `.../debug/processing_time_ms`, and an endpoint could just as easily be
    /// named `max_latency_ms`; reporting either as a rename would be noise an
    /// author cannot act on.
    #[test]
    fn does_not_flag_endpoint_or_topic_names() {
        let d = run(concat!(
            "nodes:\n  a:\n    pub:\n      max_latency_ms: {}\n",
            "topics:\n  /debug/processing_time_ms:\n    type: std_msgs/msg/Float64\n"
        ));
        assert!(
            d.is_empty(),
            "author-chosen names must not be flagged: {d:?}"
        );
    }
}
