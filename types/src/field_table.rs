//! The contract grammar, enumerated.
//!
//! Until phase 69 the grammar of a contract file existed in three places —
//! the structs in [`crate::types`], the hand-written reader in
//! [`crate::parse`], and the Format Reference section of
//! `docs/launch-manifest.md` — and nothing checked the three against each
//! other. Measured on 2026-09-04: 22 of 66 struct fields were absent from the
//! Format Reference and **six appeared nowhere in that 1752-line document at
//! all** (`lease_duration`, `min_latency`, `concurrency`, `exclusive`,
//! `max_count`, `tolerate`).
//!
//! The consequence was not merely stale prose. The parser accepted any key it
//! did not recognise, so `max_latencyy: 5ms` silently deleted a budget and
//! `rate_hzz: 100` silently deleted the declaration that the
//! `derivable-rate` rule reads — which means the one diagnostic pointing at
//! the mistake was silenced *by* the mistake.
//!
//! This table is now the single source. [`crate::parse`] rejects a key that
//! is not in it, and the Format Reference is generated from it.
//!
//! # What this table deliberately does NOT carry
//!
//! No `kind` (fact / requirement / consequence) and no `consumer` column.
//! Both are wanted — they are what make
//! `docs/design/contract-primitives.md`'s rule mechanical — but neither can
//! be filled honestly without the consumer census, which reads every rule and
//! every mapper in a *different* repository. Adding the columns now and
//! leaving them blank would create exactly the write-only fields this
//! campaign exists to remove.

/// A container in the contract grammar whose keys are fixed by the schema.
///
/// Contexts whose keys are chosen by the *author* — `nodes:`, `topics:`,
/// `services:`, `actions:`, `includes:`, `paths:`, `args:`,
/// `external_topics:`, and the endpoint maps under `pub:`/`sub:`/`srv:`/
/// `cli:` — are deliberately absent. An allowlist must never apply to them,
/// and their absence here is the mechanism: there is no way to ask this table
/// for the legal keys of a map whose keys are node names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Context {
    /// Top level of a manifest, and every inline `includes:` scope.
    Manifest,
    /// A value under `nodes:`.
    Node,
    /// A value under `pub:`, `sub:` or `cli:`.
    Endpoint,
    /// A value under `srv:`.
    SrvEndpoint,
    /// A value under `topics:`.
    Topic,
    /// A value under `external_topics:`.
    ExternalTopic,
    /// A value under `services:`.
    Service,
    /// A value under `actions:`.
    Action,
    /// A value under `includes:` that names another manifest file. An include
    /// WITHOUT `manifest:` is a nested [`Context::Manifest`], not this.
    IncludeExternal,
    /// A value under `args:`.
    Arg,
    /// A value under `paths:`, at either manifest or node level.
    Path,
    /// The map under `trigger:`.
    Trigger,
    /// The map under `trigger: { timer: ... }`.
    TriggerTimer,
    /// The map under `sync:`.
    Sync,
    /// The map form of `drop:`.
    Drop,
    /// The map under `qos:`.
    Qos,
    /// The map form of `miss:`.
    Miss,
    /// The map under `concurrency:`.
    Concurrency,
}

impl Context {
    /// The name used in generated documentation and in error messages.
    pub fn label(self) -> &'static str {
        match self {
            Context::Manifest => "manifest",
            Context::Node => "nodes.<name>",
            Context::Endpoint => "pub/sub/cli.<endpoint>",
            Context::SrvEndpoint => "srv.<endpoint>",
            Context::Topic => "topics.<name>",
            Context::ExternalTopic => "external_topics.<name>",
            Context::Service => "services.<name>",
            Context::Action => "actions.<name>",
            Context::IncludeExternal => "includes.<name>",
            Context::Arg => "args.<name>",
            Context::Path => "paths.<name>",
            Context::Trigger => "trigger",
            Context::TriggerTimer => "trigger.timer",
            Context::Sync => "sync",
            Context::Drop => "drop",
            Context::Qos => "qos",
            Context::Miss => "miss",
            Context::Concurrency => "concurrency",
        }
    }
}

/// Why a key is in the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// The current spelling.
    Live,
    /// An older spelling that still parses. The `canonical` field of
    /// [`Field`] names what replaced it.
    DeprecatedAlias,
    /// Removed from the language, and kept here ONLY so that the
    /// unknown-key check defers to the dedicated error, which explains the
    /// replacement. Without these rows a retired key would get a bare "not a
    /// known key" instead of the migration message.
    Removed,
}

/// One legal key, in one context.
#[derive(Debug, Clone, Copy)]
pub struct Field {
    pub key: &'static str,
    pub context: Context,
    pub status: Status,
    /// For [`Status::DeprecatedAlias`], the key that replaced it. Empty
    /// otherwise.
    pub canonical: &'static str,
    /// One line, used to generate the Format Reference.
    pub doc: &'static str,
}

const fn live(key: &'static str, context: Context, doc: &'static str) -> Field {
    Field {
        key,
        context,
        status: Status::Live,
        canonical: "",
        doc,
    }
}

const fn alias(key: &'static str, context: Context, canonical: &'static str) -> Field {
    Field {
        key,
        context,
        status: Status::DeprecatedAlias,
        canonical,
        doc: "Deprecated spelling.",
    }
}

const fn removed(key: &'static str, context: Context, doc: &'static str) -> Field {
    Field {
        key,
        context,
        status: Status::Removed,
        canonical: "",
        doc,
    }
}

/// Every key the contract grammar accepts, in every context that has fixed
/// keys.
pub const FIELDS: &[Field] = &[
    // ── manifest ──
    live(
        "version",
        Context::Manifest,
        "Manifest format version. Absent means 1.",
    ),
    live(
        "args",
        Context::Manifest,
        "Arguments this manifest requires, supplied by the launch scope.",
    ),
    live(
        "exclude_patterns",
        Context::Manifest,
        "Node-name globs this manifest deliberately does not describe.",
    ),
    live(
        "nodes",
        Context::Manifest,
        "Node declarations, keyed by bare node name.",
    ),
    live(
        "topics",
        Context::Manifest,
        "Topic declarations, keyed by topic name.",
    ),
    live(
        "services",
        Context::Manifest,
        "Service declarations, keyed by service name.",
    ),
    live(
        "actions",
        Context::Manifest,
        "Action declarations, keyed by action name.",
    ),
    live(
        "includes",
        Context::Manifest,
        "Child manifests, either a file reference or an inline manifest.",
    ),
    live(
        "paths",
        Context::Manifest,
        "Scope paths: end-to-end requirements naming two topics and a budget.",
    ),
    live(
        "external_topics",
        Context::Manifest,
        "Topics produced or consumed outside the loaded manifest tree.",
    ),
    removed(
        "chains",
        Context::Manifest,
        "Removed in phase 68 — state the requirement as a scope path and let the route be derived.",
    ),
    // ── nodes.<name> ──
    live(
        "if",
        Context::Node,
        "Include this node only when the condition holds.",
    ),
    live(
        "unless",
        Context::Node,
        "Include this node unless the condition holds.",
    ),
    live(
        "lifecycle",
        Context::Node,
        "True for a ROS 2 managed node; runtime monitors skip checks until it is Active.",
    ),
    live(
        "pub",
        Context::Node,
        "Publisher endpoints, keyed by endpoint name.",
    ),
    live(
        "sub",
        Context::Node,
        "Subscriber endpoints, keyed by endpoint name.",
    ),
    live(
        "srv",
        Context::Node,
        "Service server endpoints, keyed by endpoint name.",
    ),
    live(
        "cli",
        Context::Node,
        "Service client endpoints, keyed by endpoint name.",
    ),
    live(
        "paths",
        Context::Node,
        "This node's internal paths, keyed by path name.",
    ),
    live(
        "criticality",
        Context::Node,
        "Platform-agnostic scheduling criticality hint: high | medium | low.",
    ),
    live(
        "concurrency",
        Context::Node,
        "Which of this node's paths may NOT run concurrently. Absent means all of them serialize.",
    ),
    // ── pub/sub/cli.<endpoint> ──
    live(
        "min_rate_hz",
        Context::Endpoint,
        "Lower bound on this endpoint's rate.",
    ),
    live(
        "max_rate_hz",
        Context::Endpoint,
        "Upper bound on this endpoint's rate.",
    ),
    live(
        "max_age",
        Context::Endpoint,
        "Subscriber: maximum data age at receive (now - header.stamp).",
    ),
    alias("max_age_ms", Context::Endpoint, "max_age"),
    live(
        "state",
        Context::Endpoint,
        "Subscriber: read-latest rather than causal.",
    ),
    live(
        "required",
        Context::Endpoint,
        "Subscriber: this endpoint must be connected.",
    ),
    live("qos", Context::Endpoint, "QoS overrides for this endpoint."),
    live(
        "max_transport",
        Context::Endpoint,
        "Transport latency budget for this endpoint.",
    ),
    alias("max_transport_ms", Context::Endpoint, "max_transport"),
    live(
        "buffer",
        Context::Endpoint,
        "Buffering discriminator for a state subscriber: latest | queue.",
    ),
    removed(
        "jitter",
        Context::Endpoint,
        "Removed in phase 68 — jitter is a property of a route, not one publisher. Use `max_jitter` on a path.",
    ),
    removed(
        "jitter_ms",
        Context::Endpoint,
        "Removed in phase 68 — see `jitter`.",
    ),
    // ── srv.<endpoint> ──
    live(
        "max_response",
        Context::SrvEndpoint,
        "Deadline for answering a request on this service.",
    ),
    alias("max_response_ms", Context::SrvEndpoint, "max_response"),
    // ── topics.<name> ──
    live(
        "if",
        Context::Topic,
        "Declare this topic only when the condition holds.",
    ),
    live(
        "unless",
        Context::Topic,
        "Declare this topic unless the condition holds.",
    ),
    live("type", Context::Topic, "ROS message type. Required."),
    live(
        "pub",
        Context::Topic,
        "Publishing endpoints, as `node/endpoint`.",
    ),
    live(
        "sub",
        Context::Topic,
        "Subscribing endpoints, as `node/endpoint`.",
    ),
    live(
        "qos",
        Context::Topic,
        "Topic-level QoS, overridable per endpoint.",
    ),
    live(
        "rate_hz",
        Context::Topic,
        "Publication rate. Derivable from the timers that drive it — see `derivable-rate`.",
    ),
    live(
        "max_transport",
        Context::Topic,
        "Transport latency budget for every subscriber of this topic.",
    ),
    alias("max_transport_ms", Context::Topic, "max_transport"),
    live(
        "drop",
        Context::Topic,
        "Permitted message loss on this topic.",
    ),
    live(
        "external",
        Context::Topic,
        "Mark one side of this topic as provided by an external system.",
    ),
    // ── external_topics.<name> ──
    live(
        "side",
        Context::ExternalTopic,
        "Which side is external: pub | sub | both.",
    ),
    alias("external", Context::ExternalTopic, "side"),
    live(
        "type",
        Context::ExternalTopic,
        "ROS message type, when known.",
    ),
    live(
        "qos",
        Context::ExternalTopic,
        "QoS the external side uses, when known.",
    ),
    // ── services.<name> ──
    live(
        "if",
        Context::Service,
        "Declare this service only when the condition holds.",
    ),
    live(
        "unless",
        Context::Service,
        "Declare this service unless the condition holds.",
    ),
    live("type", Context::Service, "ROS service type."),
    live(
        "server",
        Context::Service,
        "Server endpoints, as `node/endpoint`.",
    ),
    live(
        "client",
        Context::Service,
        "Client endpoints, as `node/endpoint`.",
    ),
    // ── actions.<name> ──
    live(
        "if",
        Context::Action,
        "Declare this action only when the condition holds.",
    ),
    live(
        "unless",
        Context::Action,
        "Declare this action unless the condition holds.",
    ),
    live("type", Context::Action, "ROS action type."),
    live(
        "server",
        Context::Action,
        "Server endpoints, as `node/endpoint`.",
    ),
    live(
        "client",
        Context::Action,
        "Client endpoints, as `node/endpoint`.",
    ),
    // ── includes.<name>, file-reference form ──
    live(
        "manifest",
        Context::IncludeExternal,
        "Path to the included manifest file.",
    ),
    // ── args.<name> ──
    live(
        "type",
        Context::Arg,
        "Argument type: bool, or omitted for a free string.",
    ),
    live(
        "choices",
        Context::Arg,
        "Permitted values. Consulted only when `type` is absent.",
    ),
    // ── paths.<name> ──
    live(
        "if",
        Context::Path,
        "Declare this path only when the condition holds.",
    ),
    live(
        "unless",
        Context::Path,
        "Declare this path unless the condition holds.",
    ),
    live(
        "input",
        Context::Path,
        "Legacy trigger spelling. Prefer `trigger: { input: [...] }`.",
    ),
    live(
        "output",
        Context::Path,
        "Endpoints (node path) or topics (scope path) this path produces.",
    ),
    live(
        "max_latency",
        Context::Path,
        "Latency budget for this path.",
    ),
    alias("max_latency_ms", Context::Path, "max_latency"),
    live(
        "correlation",
        Context::Path,
        "How multiple inputs are correlated into one output.",
    ),
    live("tolerance", Context::Path, "Correlation tolerance."),
    alias("tolerance_ms", Context::Path, "tolerance"),
    live(
        "drop",
        Context::Path,
        "Permitted message loss along this path.",
    ),
    live(
        "trigger",
        Context::Path,
        "What causes this path's output: timer | input | once | spontaneous.",
    ),
    live(
        "sync",
        Context::Path,
        "Fan-in synchronization policy for an input trigger with two or more endpoints.",
    ),
    live(
        "max_jitter",
        Context::Path,
        "Permitted variation in this path's latency.",
    ),
    live(
        "min_latency",
        Context::Path,
        "Best-case latency. Exists so that `max_jitter` is falsifiable.",
    ),
    live(
        "miss",
        Context::Path,
        "What a missed deadline costs and what to do about it.",
    ),
    removed(
        "segments",
        Context::Path,
        "Removed in phase 68 with `chains:` — a written route is a second copy of the graph.",
    ),
    // ── trigger ──
    live("timer", Context::Trigger, "Periodic self-clocked callback."),
    live(
        "input",
        Context::Trigger,
        "Output caused by these input endpoints or topics.",
    ),
    // ── trigger.timer ──
    live(
        "rate_hz",
        Context::TriggerTimer,
        "Timer rate. Required, and must be greater than zero.",
    ),
    // ── sync ──
    live("policy", Context::Sync, "Fan-in policy. Required."),
    live(
        "max_interval",
        Context::Sync,
        "Widest permitted spread between matched inputs.",
    ),
    alias("max_interval_ms", Context::Sync, "max_interval"),
    live(
        "timeout",
        Context::Sync,
        "How long to wait for the remaining inputs.",
    ),
    alias("timeout_ms", Context::Sync, "timeout"),
    // ── drop ──
    live(
        "max_count",
        Context::Drop,
        "Permitted losses over a window, as `N / W`.",
    ),
    live(
        "max_consecutive",
        Context::Drop,
        "Permitted consecutive losses.",
    ),
    // ── qos ──
    live("reliability", Context::Qos, "reliable | best_effort."),
    live("durability", Context::Qos, "volatile | transient_local."),
    live("depth", Context::Qos, "History depth."),
    live("history", Context::Qos, "keep_last | keep_all."),
    live(
        "lifespan",
        Context::Qos,
        "How long a message stays valid after publication.",
    ),
    alias("lifespan_ms", Context::Qos, "lifespan"),
    live("liveliness", Context::Qos, "automatic | manual_by_topic."),
    live(
        "deadline",
        Context::Qos,
        "QoS deadline between consecutive messages.",
    ),
    live("lease_duration", Context::Qos, "Liveliness lease duration."),
    // ── miss ──
    live(
        "tolerate",
        Context::Miss,
        "Permitted misses over a window, as `N / W`.",
    ),
    live(
        "consecutive",
        Context::Miss,
        "Permitted consecutive misses.",
    ),
    live(
        "action",
        Context::Miss,
        "What to do on a miss: continue | skip_next | abort.",
    ),
    // ── concurrency ──
    live(
        "exclusive",
        Context::Concurrency,
        "Groups of path names that may not run at the same time.",
    ),
];

/// Render the whole grammar as the Format Reference.
///
/// This is the document. `docs/format-reference.md` is this string written to
/// disk, and a test fails when the two disagree — which is what makes the
/// drift measured in phase 69 (22 of 66 fields missing from the hand-written
/// reference, six of them absent from the entire document) unrepeatable.
pub fn render_markdown() -> String {
    let mut out = String::new();
    out.push_str("# Contract File — Format Reference\n\n");
    out.push_str(
        "**Generated from `types/src/field_table.rs`. Do not edit by hand** — \
         run `UPDATE_FORMAT_REFERENCE=1 cargo test -p ros-launch-manifest-types` \
         after changing the table.\n\n\
         Every key below is accepted in the context that heads its section, and \
         **a key that is not listed is a parse error**. Contexts whose keys are \
         chosen by the author — `nodes:`, `topics:`, `services:`, `actions:`, \
         `includes:`, `paths:`, `args:`, `external_topics:`, and the endpoint \
         maps under `pub:`/`sub:`/`srv:`/`cli:` — have no section here, because \
         no allowlist applies to them.\n\n",
    );

    let mut contexts: Vec<Context> = FIELDS.iter().map(|f| f.context).collect();
    contexts.dedup();

    for context in contexts {
        out.push_str(&format!("## `{}`\n\n", context.label()));
        out.push_str("| key | status | meaning |\n|---|---|---|\n");
        for f in allowed(context) {
            let status = match f.status {
                Status::Live => "".to_string(),
                Status::DeprecatedAlias => format!("deprecated — use `{}`", f.canonical),
                Status::Removed => "**removed**".to_string(),
            };
            out.push_str(&format!("| `{}` | {} | {} |\n", f.key, status, f.doc));
        }
        out.push('\n');
    }
    out
}
/// The keys legal in `context`, for the unknown-key check and for error
/// messages. Includes deprecated and removed spellings: a removed key must
/// reach its own dedicated error rather than a generic one.
pub fn allowed(context: Context) -> impl Iterator<Item = &'static Field> {
    FIELDS.iter().filter(move |f| f.context == context)
}

/// The table row for `key` in `context`, if any.
pub fn lookup(context: Context, key: &str) -> Option<&'static Field> {
    allowed(context).find(|f| f.key == key)
}

/// The keys a diagnostic should offer as legal in `context`. Removed
/// spellings are excluded: naming them here would advertise a key that is
/// rejected on sight.
pub fn suggestions(context: Context) -> Vec<&'static str> {
    let mut keys: Vec<&'static str> = allowed(context)
        .filter(|f| f.status != Status::Removed)
        .map(|f| f.key)
        .collect();
    keys.sort_unstable();
    keys
}

/// The accepted key in `context` closest to `key`, for a "did you mean"
/// suggestion. `None` when nothing is close enough to be worth printing —
/// an unrelated key produces no suggestion rather than a misleading one.
pub fn nearest(context: Context, key: &str) -> Option<&'static str> {
    let budget = match key.len() {
        0..=4 => 1,
        5..=8 => 2,
        _ => 3,
    };
    allowed(context)
        .filter(|f| f.status != Status::Removed)
        .map(|f| (edit_distance(f.key, key), f.key))
        .filter(|(d, _)| *d <= budget)
        .min_by_key(|(d, k)| (*d, *k))
        .map(|(_, k)| k)
}

/// Levenshtein distance, two rows.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A key may appear once per context and no more. A duplicate row is how
    /// two spellings of the same field silently diverge.
    #[test]
    fn no_key_is_declared_twice_in_one_context() {
        let mut seen = std::collections::BTreeSet::new();
        for f in FIELDS {
            assert!(
                seen.insert((f.context, f.key)),
                "`{}` is declared twice in {}",
                f.key,
                f.context.label()
            );
        }
    }

    /// A deprecated alias that names a canonical key which does not exist in
    /// its own context is a dangling migration message.
    #[test]
    fn every_alias_names_a_live_key_in_the_same_context() {
        for f in FIELDS
            .iter()
            .filter(|f| f.status == Status::DeprecatedAlias)
        {
            assert!(
                !f.canonical.is_empty(),
                "alias `{}` names no canonical key",
                f.key
            );
            assert!(
                allowed(f.context).any(|c| c.key == f.canonical && c.status == Status::Live),
                "alias `{}` in {} names `{}`, which is not a live key there",
                f.key,
                f.context.label(),
                f.canonical
            );
        }
    }

    /// The suggestion must fire on the typos that motivated this table, and
    /// must NOT fire on a key that merely happens to be short.
    #[test]
    fn a_one_character_typo_gets_a_suggestion() {
        assert_eq!(nearest(Context::Path, "max_latencyy"), Some("max_latency"));
        assert_eq!(nearest(Context::Topic, "rate_hzz"), Some("rate_hz"));
        assert_eq!(nearest(Context::Path, "bogus_field"), None);
    }

    /// [`render_markdown`] emits one section per context by walking FIELDS in
    /// order, so a context whose rows are not contiguous would get two
    /// sections — a document that silently contradicts itself.
    #[test]
    fn rows_are_grouped_by_context() {
        let mut seen: Vec<Context> = Vec::new();
        let mut last: Option<Context> = None;
        for f in FIELDS {
            if last != Some(f.context) {
                assert!(
                    !seen.contains(&f.context),
                    "{} rows are split across the table",
                    f.context.label()
                );
                seen.push(f.context);
                last = Some(f.context);
            }
        }
    }

    /// The checked-in Format Reference is this table, rendered. A mismatch
    /// means the document and the grammar have diverged — the exact failure
    /// phase 69 was opened to end.
    #[test]
    fn the_format_reference_matches_the_table() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/format-reference.md");
        let rendered = render_markdown();
        if std::env::var("UPDATE_FORMAT_REFERENCE").is_ok() {
            std::fs::write(&path, &rendered).expect("write the reference");
            return;
        }
        let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(
            on_disk, rendered,
            "docs/format-reference.md is stale — regenerate with \
             `UPDATE_FORMAT_REFERENCE=1 cargo test -p ros-launch-manifest-types`"
        );
    }

    /// Every field carries a documentation line, because the Format
    /// Reference is generated from this table and a blank row would silently
    /// become a blank documentation entry.
    #[test]
    fn every_field_is_documented() {
        for f in FIELDS {
            assert!(
                !f.doc.is_empty(),
                "`{}` in {} has no doc line",
                f.key,
                f.context.label()
            );
        }
    }
}
