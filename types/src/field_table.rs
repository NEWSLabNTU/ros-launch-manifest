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
/// `external_topics:`, the endpoint maps under `pub:`/`sub:`/`srv:`/
/// `cli:`, and the parameter names under `params:` — are deliberately absent. An allowlist must never apply to them,
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
    /// A value under a node's `params:` map. The map's own keys are
    /// parameter names, chosen by the author; this is the `{ type: ... }`
    /// under each.
    Param,
    /// `modes.<name>` (phase 75).
    Mode,
    /// `hazards.<name>` (phase 71).
    Hazard,
    /// One `{ all_of: [...] }` guard group.
    HazardGuard,
    /// `on_violation` on a subscriber.
    OnViolation,
    /// `safe_state` on a path.
    SafeState,
    /// The map form of `functions.<name>` (v0.1.46): `{ all_of: [...] }` or
    /// `{ of: [...], when: {...} }`.
    Function,
    /// A `when:` predicate, on a hazard or a function (v0.1.46).
    Predicate,
    /// The map form of `modes.<name>.window` (v0.1.46).
    Window,
    /// `modes.<name>.exit` (v0.1.46).
    Exit,
    /// The map form of `safe_state.settle`: a braking profile (v0.1.46).
    Settle,
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
            Context::Param => "params.<name>",
            Context::Mode => "modes.<name>",
            Context::Hazard => "hazards.<name>",
            Context::HazardGuard => "hazards.<name>.guards[]",
            Context::OnViolation => "on_violation",
            Context::SafeState => "safe_state",
            Context::Function => "functions.<name>",
            Context::Predicate => "when",
            Context::Window => "modes.<name>.window",
            Context::Exit => "modes.<name>.exit",
            Context::Settle => "safe_state.settle",
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
    /// What sort of statement a live key makes — the `contract-primitives.md`
    /// rule made mechanical. See [`Kind`].
    pub kind: Kind,
    /// For [`Status::DeprecatedAlias`], the key that replaced it. Empty
    /// otherwise.
    pub canonical: &'static str,
    /// One line, used to generate the Format Reference.
    pub doc: &'static str,
}

/// What a key STATES. The rule of `contract-primitives.md`: a contract
/// states what the code does and what it must achieve, and anything
/// computable from those two is derived, never written. This column is that
/// rule as data, so a test can hold it.
///
/// Phase 70 W2 made five of these judgments implicitly (delete or implement,
/// per unread field) and wrote none of them down. Phase 69 deliberately left
/// the column out until the census could fill the `consumer` half; this is
/// the `kind` half.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Structure and plumbing: names, wiring, conditions, includes. Neither a
    /// fact about the code nor a requirement on it.
    Meta,
    /// What the code DOES: a trigger, an output, a QoS profile, a floor it
    /// was measured at. Verifiable against the running system.
    Fact,
    /// What the system MUST ACHIEVE: a budget, a spread, a miss policy, a
    /// drop bound. Checked, never derived.
    Requirement,
    /// A fact on a publisher, a requirement on a subscriber — the one key
    /// whose kind depends on which side of the endpoint map it sits under.
    ByEndpoint,
    /// Computable from the facts. A live key of this kind is a second copy of
    /// something the graph already knows, and `scripts/derivation_census.py`
    /// measures how often the copy agrees. There should be very few of these,
    /// and a test pins the list.
    Consequence,
}

const fn live(key: &'static str, context: Context, kind: Kind, doc: &'static str) -> Field {
    Field {
        key,
        context,
        status: Status::Live,
        kind,
        canonical: "",
        doc,
    }
}

const fn alias(key: &'static str, context: Context, canonical: &'static str) -> Field {
    Field {
        key,
        context,
        status: Status::DeprecatedAlias,
        kind: Kind::Meta,
        canonical,
        doc: "Deprecated spelling.",
    }
}

const fn removed(key: &'static str, context: Context, doc: &'static str) -> Field {
    Field {
        key,
        context,
        status: Status::Removed,
        kind: Kind::Meta,
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
        Kind::Meta,
        "Manifest format version. Absent means 1.",
    ),
    live(
        "args",
        Context::Manifest,
        Kind::Meta,
        "Arguments this manifest requires, supplied by the launch scope.",
    ),
    removed(
        "exclude_patterns",
        Context::Manifest,
        "Removed in phase 70 — it was parsed and read by nothing, so it excluded nothing. Mark an expected-absent side with `external:`.",
    ),
    live(
        "nodes",
        Context::Manifest,
        Kind::Meta,
        "Node declarations, keyed by bare node name.",
    ),
    live(
        "topics",
        Context::Manifest,
        Kind::Meta,
        "Topic declarations, keyed by topic name.",
    ),
    live(
        "services",
        Context::Manifest,
        Kind::Meta,
        "Service declarations, keyed by service name.",
    ),
    live(
        "actions",
        Context::Manifest,
        Kind::Meta,
        "Action declarations, keyed by action name.",
    ),
    live(
        "includes",
        Context::Manifest,
        Kind::Meta,
        "Child manifests, either a file reference or an inline manifest.",
    ),
    live(
        "paths",
        Context::Manifest,
        Kind::Meta,
        "Scope paths: end-to-end requirements naming two topics and a budget.",
    ),
    live(
        "hazards",
        Context::Manifest,
        Kind::Meta,
        "Hazards: what is watched, how long the system has, and which path reaches the safe state.",
    ),
    live(
        "functions",
        Context::Manifest,
        Kind::Meta,
        "Named guard groups: what a set of topics together provides. A mode requires functions; a hazard may guard one by name.",
    ),
    live(
        "modes",
        Context::Manifest,
        Kind::Meta,
        "Operational modes: what each requires, and the ordered ladder to fall to when it is lost.",
    ),
    live(
        "severity_levels",
        Context::Manifest,
        Kind::Meta,
        "The severity scale hazards draw from, ascending; the first entry derives no criticality. Default: QM, ASIL_A..ASIL_D.",
    ),
    live(
        "external_topics",
        Context::Manifest,
        Kind::Meta,
        "Topics produced or consumed outside the loaded manifest tree.",
    ),
    removed(
        "chains",
        Context::Manifest,
        "Removed in phase 68 — state the requirement as a scope path and let the route be derived.",
    ),
    live(
        "if",
        Context::Manifest,
        Kind::Meta,
        "Only on an INLINE `includes:` entry, whose value is a nested manifest: include this child only when the condition holds. Refused at the root of a standalone manifest.",
    ),
    live(
        "unless",
        Context::Manifest,
        Kind::Meta,
        "Only on an INLINE `includes:` entry: include this child unless the condition holds. Refused at the root of a standalone manifest.",
    ),
    // ── nodes.<name> ──
    live(
        "if",
        Context::Node,
        Kind::Meta,
        "Include this node only when the condition holds.",
    ),
    live(
        "unless",
        Context::Node,
        Kind::Meta,
        "Include this node unless the condition holds.",
    ),
    live(
        "lifecycle",
        Context::Node,
        Kind::Fact,
        "True for a ROS 2 managed node; runtime monitors skip checks until it is Active.",
    ),
    live(
        "pub",
        Context::Node,
        Kind::Meta,
        "Publisher endpoints, keyed by endpoint name. A key is a LOCAL name: the node's FQN is prefixed to it, so a `/` in a key is refused; connect the topic with a launch remap or a `topics:` entry.",
    ),
    live(
        "sub",
        Context::Node,
        Kind::Meta,
        "Subscriber endpoints, keyed by endpoint name. A key is a LOCAL name: the node's FQN is prefixed to it, so a `/` in a key is refused; connect the topic with a launch remap or a `topics:` entry.",
    ),
    live(
        "srv",
        Context::Node,
        Kind::Meta,
        "Service server endpoints, keyed by endpoint name.",
    ),
    live(
        "cli",
        Context::Node,
        Kind::Meta,
        "Service client endpoints, keyed by endpoint name.",
    ),
    live(
        "paths",
        Context::Node,
        Kind::Meta,
        "This node's internal paths, keyed by path name.",
    ),
    live(
        "criticality",
        Context::Node,
        Kind::Consequence,
        "Scheduling criticality: high | medium | low. A CONSEQUENCE of the hazards a node guards, reacts for, or feeds (phase 72); the label stands only where no hazard reaches the node.",
    ),
    live(
        "concurrency",
        Context::Node,
        Kind::Fact,
        "Which of this node's paths may NOT run concurrently. Absent means all of them serialize.",
    ),
    live(
        "params",
        Context::Node,
        Kind::Fact,
        "Parameters this node declares, keyed by name, each `{ type: <ROS 2 type> }`. Names and types only: a string or array capacity is a board fact, not a contract one. `params: {}` means the node declares no parameters; a missing `params:` means not stated. The two stay distinct in the model: `{}` becomes an empty `contracts.node_params` entry, a missing key none.",
    ),
    // ── pub/sub/cli.<endpoint> ──
    live(
        "min_rate_hz",
        Context::Endpoint,
        Kind::ByEndpoint,
        "Lower bound on this endpoint's rate.",
    ),
    live(
        "max_rate_hz",
        Context::Endpoint,
        Kind::ByEndpoint,
        "Upper bound on this endpoint's rate.",
    ),
    live(
        "max_age",
        Context::Endpoint,
        Kind::Requirement,
        "Subscriber: maximum data age at receive (now - header.stamp).",
    ),
    removed(
        "max_age_ms",
        Context::Endpoint,
        "Removed in phase 70 — write `max_age: <n>ms` (or ns/us/s). The unit in a NAME is what lets a value be 1000x wrong and still parse.",
    ),
    live(
        "state",
        Context::Endpoint,
        Kind::Fact,
        "Subscriber: read-latest rather than causal.",
    ),
    live(
        "required",
        Context::Endpoint,
        Kind::Fact,
        "Subscriber: this endpoint must be connected.",
    ),
    live(
        "qos",
        Context::Endpoint,
        Kind::Fact,
        "QoS overrides for this endpoint.",
    ),
    live(
        "max_transport",
        Context::Endpoint,
        Kind::Requirement,
        "Transport latency budget for this endpoint.",
    ),
    removed(
        "max_transport_ms",
        Context::Endpoint,
        "Removed in phase 70 — write `max_transport: <n>ms` (or ns/us/s). The unit in a NAME is what lets a value be 1000x wrong and still parse.",
    ),
    live(
        "on_violation",
        Context::Endpoint,
        Kind::Requirement,
        "The reaction this subscriber owes when its assumption is violated.",
    ),
    live(
        "buffer",
        Context::Endpoint,
        Kind::Fact,
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
        Kind::Requirement,
        "Deadline for answering a request on this service.",
    ),
    removed(
        "max_response_ms",
        Context::SrvEndpoint,
        "Removed in phase 70 — write `max_response: <n>ms` (or ns/us/s). The unit in a NAME is what lets a value be 1000x wrong and still parse.",
    ),
    // ── topics.<name> ──
    live(
        "if",
        Context::Topic,
        Kind::Meta,
        "Declare this topic only when the condition holds.",
    ),
    live(
        "unless",
        Context::Topic,
        Kind::Meta,
        "Declare this topic unless the condition holds.",
    ),
    live(
        "type",
        Context::Topic,
        Kind::Fact,
        "ROS message type. Required.",
    ),
    live(
        "pub",
        Context::Topic,
        Kind::Meta,
        "Publishing endpoints, as `node/endpoint`.",
    ),
    live(
        "sub",
        Context::Topic,
        Kind::Meta,
        "Subscribing endpoints, as `node/endpoint`.",
    ),
    live(
        "qos",
        Context::Topic,
        Kind::Fact,
        "Topic-level QoS, overridable per endpoint.",
    ),
    live(
        "rate_hz",
        Context::Topic,
        Kind::Consequence,
        "Publication rate. Derivable from the timers that drive it — see `derivable-rate`.",
    ),
    live(
        "max_transport",
        Context::Topic,
        Kind::Requirement,
        "Transport latency budget for every subscriber of this topic.",
    ),
    removed(
        "max_transport_ms",
        Context::Topic,
        "Removed in phase 70 — write `max_transport: <n>ms` (or ns/us/s). The unit in a NAME is what lets a value be 1000x wrong and still parse.",
    ),
    live(
        "drop",
        Context::Topic,
        Kind::Requirement,
        "Permitted message loss on this topic.",
    ),
    live(
        "external",
        Context::Topic,
        Kind::Meta,
        "Mark one side of this topic as provided by an external system.",
    ),
    // ── external_topics.<name> ──
    live(
        "side",
        Context::ExternalTopic,
        Kind::Meta,
        "Which side is external: pub | sub | both.",
    ),
    alias("external", Context::ExternalTopic, "side"),
    live(
        "type",
        Context::ExternalTopic,
        Kind::Fact,
        "ROS message type, when known.",
    ),
    live(
        "qos",
        Context::ExternalTopic,
        Kind::Fact,
        "QoS the external side uses, when known.",
    ),
    // ── services.<name> ──
    live(
        "if",
        Context::Service,
        Kind::Meta,
        "Declare this service only when the condition holds.",
    ),
    live(
        "unless",
        Context::Service,
        Kind::Meta,
        "Declare this service unless the condition holds.",
    ),
    live("type", Context::Service, Kind::Fact, "ROS service type."),
    live(
        "server",
        Context::Service,
        Kind::Meta,
        "Server endpoints, as `node/endpoint`.",
    ),
    live(
        "client",
        Context::Service,
        Kind::Meta,
        "Client endpoints, as `node/endpoint`.",
    ),
    live(
        "external",
        Context::Service,
        Kind::Meta,
        "Mark one side of this service as provided by an external system: server | client | both.",
    ),
    // ── actions.<name> ──
    live(
        "if",
        Context::Action,
        Kind::Meta,
        "Declare this action only when the condition holds.",
    ),
    live(
        "unless",
        Context::Action,
        Kind::Meta,
        "Declare this action unless the condition holds.",
    ),
    live("type", Context::Action, Kind::Fact, "ROS action type."),
    live(
        "server",
        Context::Action,
        Kind::Meta,
        "Server endpoints, as `node/endpoint`.",
    ),
    live(
        "client",
        Context::Action,
        Kind::Meta,
        "Client endpoints, as `node/endpoint`.",
    ),
    live(
        "external",
        Context::Action,
        Kind::Meta,
        "Mark one side of this action as provided by an external system: server | client | both.",
    ),
    // ── includes.<name>, file-reference form ──
    live(
        "manifest",
        Context::IncludeExternal,
        Kind::Meta,
        "Path to the included manifest file.",
    ),
    live(
        "if",
        Context::IncludeExternal,
        Kind::Meta,
        "Include this child manifest only when the condition holds.",
    ),
    live(
        "unless",
        Context::IncludeExternal,
        Kind::Meta,
        "Include this child manifest unless the condition holds.",
    ),
    // ── args.<name> ──
    live(
        "type",
        Context::Arg,
        Kind::Meta,
        "Argument type: bool, or omitted for a free string.",
    ),
    live(
        "choices",
        Context::Arg,
        Kind::Meta,
        "Permitted values. Consulted only when `type` is absent.",
    ),
    // ── paths.<name> ──
    live(
        "if",
        Context::Path,
        Kind::Meta,
        "Declare this path only when the condition holds.",
    ),
    live(
        "unless",
        Context::Path,
        Kind::Meta,
        "Declare this path unless the condition holds.",
    ),
    live(
        "input",
        Context::Path,
        Kind::Fact,
        "Legacy trigger spelling. Prefer `trigger: { input: [...] }`.",
    ),
    live(
        "output",
        Context::Path,
        Kind::Fact,
        "Endpoints (node path) or topics (scope path) this path produces.",
    ),
    live(
        "max_latency",
        Context::Path,
        Kind::Requirement,
        "Latency budget for this path.",
    ),
    removed(
        "max_latency_ms",
        Context::Path,
        "Removed in phase 70 — write `max_latency: <n>ms` (or ns/us/s). The unit in a NAME is what lets a value be 1000x wrong and still parse.",
    ),
    removed(
        "correlation",
        Context::Path,
        "Removed in phase 70 — nothing ever read it. State fan-in policy with `sync:`.",
    ),
    live(
        "tolerance",
        Context::Path,
        Kind::Fact,
        "Max `header.stamp` spread between a fan-in path's inputs still treated as one set.",
    ),
    removed(
        "tolerance_ms",
        Context::Path,
        "Removed in phase 70 — write `tolerance: <n>ms` (or ns/us/s). The unit in a NAME is what lets a value be 1000x wrong and still parse.",
    ),
    live(
        "drop",
        Context::Path,
        Kind::Requirement,
        "Permitted message loss along this path.",
    ),
    live(
        "trigger",
        Context::Path,
        Kind::Fact,
        "What causes this path's output: timer | input | once | spontaneous.",
    ),
    live(
        "sync",
        Context::Path,
        Kind::Fact,
        "Fan-in synchronization policy for an input trigger with two or more endpoints.",
    ),
    live(
        "max_jitter",
        Context::Path,
        Kind::Requirement,
        "Permitted variation in this path's latency.",
    ),
    live(
        "min_latency",
        Context::Path,
        Kind::Fact,
        "Best-case latency. Exists so that `max_jitter` is falsifiable.",
    ),
    live(
        "safe_state",
        Context::Path,
        Kind::Fact,
        "What this path produces when it is a reaction: the endpoint it commands, and the plant's settle time.",
    ),
    live(
        "miss",
        Context::Path,
        Kind::Requirement,
        "What a missed deadline costs and what to do about it.",
    ),
    removed(
        "segments",
        Context::Path,
        "Removed in phase 68 with `chains:` — a written route is a second copy of the graph.",
    ),
    // ── trigger ──
    live(
        "timer",
        Context::Trigger,
        Kind::Fact,
        "Periodic self-clocked callback.",
    ),
    live(
        "input",
        Context::Trigger,
        Kind::Fact,
        "Output caused by these input endpoints or topics.",
    ),
    // ── trigger.timer ──
    live(
        "rate_hz",
        Context::TriggerTimer,
        Kind::Fact,
        "Timer rate. Required, and must be greater than zero.",
    ),
    // ── sync ──
    live(
        "policy",
        Context::Sync,
        Kind::Fact,
        "Fan-in policy. Required.",
    ),
    live(
        "max_interval",
        Context::Sync,
        Kind::Fact,
        "Widest permitted spread between matched inputs.",
    ),
    removed(
        "max_interval_ms",
        Context::Sync,
        "Removed in phase 70 — write `max_interval: <n>ms` (or ns/us/s). The unit in a NAME is what lets a value be 1000x wrong and still parse.",
    ),
    live(
        "timeout",
        Context::Sync,
        Kind::Fact,
        "How long to wait for the remaining inputs.",
    ),
    removed(
        "timeout_ms",
        Context::Sync,
        "Removed in phase 70 — write `timeout: <n>ms` (or ns/us/s). The unit in a NAME is what lets a value be 1000x wrong and still parse.",
    ),
    // ── drop ──
    live(
        "max_count",
        Context::Drop,
        Kind::Requirement,
        "Permitted losses over a window, as `N / W`.",
    ),
    live(
        "max_consecutive",
        Context::Drop,
        Kind::Requirement,
        "Permitted consecutive losses.",
    ),
    // ── qos ──
    live(
        "reliability",
        Context::Qos,
        Kind::Fact,
        "reliable | best_effort.",
    ),
    live(
        "durability",
        Context::Qos,
        Kind::Fact,
        "volatile | transient_local.",
    ),
    live("depth", Context::Qos, Kind::Fact, "History depth."),
    live("history", Context::Qos, Kind::Fact, "keep_last | keep_all."),
    live(
        "lifespan",
        Context::Qos,
        Kind::Fact,
        "How long a message stays valid after publication.",
    ),
    removed(
        "lifespan_ms",
        Context::Qos,
        "Removed in phase 70 — write `lifespan: <n>ms` (or ns/us/s). The unit in a NAME is what lets a value be 1000x wrong and still parse.",
    ),
    live(
        "liveliness",
        Context::Qos,
        Kind::Fact,
        "automatic | manual_by_topic.",
    ),
    live(
        "deadline",
        Context::Qos,
        Kind::Fact,
        "QoS deadline between consecutive messages.",
    ),
    live(
        "lease_duration",
        Context::Qos,
        Kind::Fact,
        "Liveliness lease: how often a publisher must assert it is alive. Checked pub against sub by `qos-match`.",
    ),
    // ── miss ──
    live(
        "tolerate",
        Context::Miss,
        Kind::Requirement,
        "Permitted misses over a window, as `N / W`.",
    ),
    live(
        "consecutive",
        Context::Miss,
        Kind::Requirement,
        "Permitted consecutive misses.",
    ),
    live(
        "action",
        Context::Miss,
        Kind::Requirement,
        "What to do on a miss: continue | skip_next | abort.",
    ),
    // ── concurrency ──
    live(
        "exclusive",
        Context::Concurrency,
        Kind::Fact,
        "Groups of path names that may not run at the same time.",
    ),
    // -- params.<name> --
    live(
        "type",
        Context::Param,
        Kind::Fact,
        "ROS 2 parameter type: bool | integer | double | string | byte_array | bool_array | integer_array | double_array | string_array. Required; an unknown type is an error.",
    ),
    // ── hazards.<name> ──
    live(
        "severity",
        Context::Hazard,
        Kind::Requirement,
        "Severity label from the HARA (`ASIL_D`, `SIL_3`, …). Consumed, never computed.",
    ),
    live(
        "description",
        Context::Hazard,
        Kind::Meta,
        "What the hazardous event is.",
    ),
    live(
        "guards",
        Context::Hazard,
        Kind::Meta,
        "Topics watched for the fault. A bare name is one guard; `{ all_of: [...] }` is a redundant set that faults only when every member does.",
    ),
    live(
        "on",
        Context::Hazard,
        Kind::Fact,
        "Fault classes the guards report, one or a list: omission | late | loss | reported. Omitted means every class, the strictest reading; `[]` is an error.",
    ),
    live(
        "ftti",
        Context::Hazard,
        Kind::Requirement,
        "Fault-tolerant time interval: fault to hazardous event, absent reaction. Checked by `fault-reaction-budget`.",
    ),
    live(
        "reaction",
        Context::Hazard,
        Kind::Meta,
        "The scope path whose route reaches the safe state.",
    ),
    live(
        "when",
        Context::Hazard,
        Kind::Fact,
        "The value that is the fault, `{ field: <name>, <op>: <constant> }`, on a hazard whose guard is a detector's output. Requires `reported` among the `on:` classes (`when-requires-reported`).",
    ),
    live(
        "entry_speed",
        Context::Hazard,
        Kind::Requirement,
        "The speed the vehicle may travel at when the fault occurs, in m/s, a plain positive number: the operating domain's speed bound. A braking-profile settle is derived from it (`settle-derived`).",
    ),
    // ── modes.<name> ──
    live(
        "description",
        Context::Mode,
        Kind::Meta,
        "What this mode is.",
    ),
    live(
        "requires",
        Context::Mode,
        Kind::Fact,
        "Functions (or bare topics) this mode needs; it is available while every one holds.",
    ),
    live(
        "fallback",
        Context::Mode,
        Kind::Requirement,
        "Modes to fall to, in order, when this one is lost. The last rung is the floor and must require nothing losable.",
    ),
    live(
        "reaction",
        Context::Mode,
        Kind::Meta,
        "The scope path that reaches this mode's safe state.",
    ),
    live(
        "overrides",
        Context::Mode,
        Kind::Requirement,
        "Requirement values that apply IN THIS MODE, pinned over the scalar declared elsewhere.",
    ),
    live(
        "window",
        Context::Mode,
        Kind::Requirement,
        "A transitional rung: while it stays available the system stays AT LEAST this long (the time the rung promises whoever it waits for, a driver's takeover time), then takes the next rung. The rung below starts no sooner than the deadline and within its own reaction route after it, and that route is where the deadline's notice is charged (`window-expiry`). A duration, or `{ duration, param }` binding it to the parameter that enforces it. Never on the floor (`ladder-window-floor`); unbound is `window-unbound`.",
    ),
    live(
        "exit",
        Context::Mode,
        Kind::Requirement,
        "`{ on: <function>, to: <mode> }`: while this rung is active, the function holding ends the ladder in `to`. Only on a windowed rung; `to` is outside every `fallback:` list.",
    ),
    // ── hazards.<name>.guards[] ──
    live(
        "all_of",
        Context::HazardGuard,
        Kind::Meta,
        "Members of a redundant guard set.",
    ),
    // ── on_violation ──
    live(
        "on",
        Context::OnViolation,
        Kind::Fact,
        "Fault classes that trigger the reaction: omission | late | loss | reported.",
    ),
    live(
        "reaction",
        Context::OnViolation,
        Kind::Meta,
        "A path on this node whose trigger includes the subscription.",
    ),
    live(
        "within",
        Context::OnViolation,
        Kind::Requirement,
        "This hop's share of the fault reaction time.",
    ),
    live(
        "mechanism",
        Context::OnViolation,
        Kind::Fact,
        "Where the runtime observer reads the violation: qos (default) | diagnostics | application.",
    ),
    // ── safe_state ──
    live(
        "emits",
        Context::SafeState,
        Kind::Fact,
        "The endpoint this reaction commands the safe state on.",
    ),
    live(
        "settle",
        Context::SafeState,
        Kind::Fact,
        "How long the plant takes to reach the safe state once commanded: a measured duration, or `{ decel: <param>, jerk: <param> }`, a braking profile by parameter name from which the settle is derived with the hazard's `entry_speed`.",
    ),
    // -- functions.<name> (map form) --
    live(
        "all_of",
        Context::Function,
        Kind::Meta,
        "Members of a redundant group, lost only when every member is. At least two.",
    ),
    live(
        "of",
        Context::Function,
        Kind::Meta,
        "The topics a value function reads. Requires `when:`; without one, write the bare list.",
    ),
    live(
        "when",
        Context::Function,
        Kind::Fact,
        "The predicate that LOSES the function by value. A value function is lost by the value or by silence.",
    ),
    // -- when --
    live(
        "field",
        Context::Predicate,
        Kind::Fact,
        "One scalar field of the message, a dotted path from its root. Checked against the `.msg` when the ament index resolves it (`when-field-unknown`).",
    ),
    live(
        "equals",
        Context::Predicate,
        Kind::Fact,
        "The field equals the constant: a bool, a number, a message constant name (upper snake case) or a string.",
    ),
    live(
        "not_equals",
        Context::Predicate,
        Kind::Fact,
        "The field differs from the constant.",
    ),
    live(
        "lt",
        Context::Predicate,
        Kind::Fact,
        "The field is below the constant, a number.",
    ),
    live(
        "le",
        Context::Predicate,
        Kind::Fact,
        "The field is at most the constant, a number.",
    ),
    live(
        "gt",
        Context::Predicate,
        Kind::Fact,
        "The field is above the constant, a number.",
    ),
    live(
        "ge",
        Context::Predicate,
        Kind::Fact,
        "The field is at least the constant, a number.",
    ),
    // -- modes.<name>.window --
    live(
        "duration",
        Context::Window,
        Kind::Requirement,
        "The least time the rung lasts while it stays available: the rung below starts no sooner. Required, positive.",
    ),
    live(
        "param",
        Context::Window,
        Kind::Fact,
        "`<node>.<parameter>`: the parameter, in seconds, whose resolved launch value must equal `duration` (`window-param`).",
    ),
    // -- modes.<name>.exit --
    live(
        "on",
        Context::Exit,
        Kind::Fact,
        "The function whose holding ends the ladder. Some node implementing the rung must take its topic on a path trigger (`mode-exit-unwired`).",
    ),
    live(
        "to",
        Context::Exit,
        Kind::Meta,
        "The mode the ladder ends in. Not a member of any `fallback:` list (`mode-exit-target`).",
    ),
    // -- safe_state.settle (profile form) --
    live(
        "decel",
        Context::Settle,
        Kind::Fact,
        "The parameter on this path's node holding the deceleration, m/s^2, read as a magnitude.",
    ),
    live(
        "jerk",
        Context::Settle,
        Kind::Fact,
        "The parameter on this path's node holding the jerk the deceleration ramps at, m/s^3, read as a magnitude.",
    ),
    live(
        "duration",
        Context::Settle,
        Kind::Fact,
        "A measured settle beside the profile: the fallback when the hazard states no entry speed, and compared against the derivation (`settle-conflict`).",
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
         `includes:`, `paths:`, `args:`, `external_topics:`, the endpoint \
         maps under `pub:`/`sub:`/`srv:`/`cli:`, and the parameter names under \
         a node's `params:` — have no section here, because \
         no allowlist applies to them.\n\n\
         The **kind** column is the rule of `contract-primitives.md` as data: a \
         *fact* is what the code does, a *requirement* is what it must achieve, \
         *meta* is structure, and a **consequence** is computable from the facts — \
         a second copy the graph already knows, kept only where the graph has no \
         answer (an external source). `scripts/derivation_census.py` counts how \
         often each consequence agrees.\n\n",
    );

    let mut contexts: Vec<Context> = FIELDS.iter().map(|f| f.context).collect();
    contexts.dedup();

    for context in contexts {
        out.push_str(&format!("## `{}`\n\n", context.label()));
        out.push_str("| key | kind | status | meaning |\n|---|---|---|---|\n");
        for f in allowed(context) {
            let status = match f.status {
                Status::Live => "".to_string(),
                Status::DeprecatedAlias => format!("deprecated — use `{}`", f.canonical),
                Status::Removed => "**removed**".to_string(),
            };
            let kind = match (f.status, f.kind) {
                (Status::Removed | Status::DeprecatedAlias, _) => "",
                (_, Kind::Meta) => "meta",
                (_, Kind::Fact) => "fact",
                (_, Kind::Requirement) => "requirement",
                (_, Kind::ByEndpoint) => "fact (pub) / requirement (sub)",
                (_, Kind::Consequence) => "**consequence**",
            };
            out.push_str(&format!(
                "| `{}` | {} | {} | {} |\n",
                f.key, kind, status, f.doc
            ));
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

    /// The consequences are the retirement backlog, and the list must not grow
    /// by accident: a new live key of this kind is a second copy of something
    /// the graph knows, and the census has to be told about it.
    #[test]
    fn the_live_consequences_are_a_topics_rate_and_a_nodes_criticality() {
        let consequences: Vec<(Context, &str)> = FIELDS
            .iter()
            .filter(|f| f.status == Status::Live && f.kind == Kind::Consequence)
            .map(|f| (f.context, f.key))
            .collect();
        assert_eq!(
            consequences,
            vec![(Context::Node, "criticality"), (Context::Topic, "rate_hz")]
        );
    }

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
