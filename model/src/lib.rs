//! SystemModel — the resolved, checked system artifact.
//!
//! One `SystemModel` describes ONE concrete system variant (early binding:
//! args bound, conditions evaluated, every name fully qualified), produced by
//! `play_launch resolve` from the launch tree + contract manifests + the
//! integrator's system config, and consumed by
//!
//! - the play_launch Linux runtime (spawn/supervise + contract monitors), and
//! - the nano-ros build system (bake each `mcu:*` node's slice into its image).
//!
//! Design: play_launch `docs/design/system-model.md` (producer side) and
//! nano-ros RFC-0050 (consumer side). This crate is PURE SCHEMA — types,
//! serde, and YAML round-trip; no resolver logic. The resolver refuses to
//! emit a model when the manifest checker reports Error severity, so a model
//! in hand is always a checked one; warnings ride along in
//! [`Meta::diagnostics`].
//!
//! All collections are `BTreeMap`/sorted so serialization is deterministic —
//! the YAML form is hashed for provenance and caching.

use std::collections::{BTreeMap, BTreeSet};

pub mod system_config;

use serde::{Deserialize, Serialize};

/// Schema version this crate reads and writes.
///
/// Versioned independently of the manifest format version: the manifest is
/// the authoring surface, the model is a derived artifact.
pub const SCHEMA_VERSION: u32 = 1;

/// Errors loading or saving a model.
#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("YAML: {0}")]
    Yaml(#[from] serde_yaml_ng::Error),
    #[error("unsupported SystemModel schema version {found} (this reader supports <= {supported})")]
    UnsupportedVersion { found: u32, supported: u32 },
}

/// The resolved system artifact. See the crate docs for the contract.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SystemModel {
    pub meta: Meta,
    /// Layer 1 — from the launch tree: who exists and how it is wired.
    pub structure: Structure,
    /// Layer 2 — from the manifests, post-merge: what timing/QoS is promised.
    #[serde(default, skip_serializing_if = "Contracts::is_empty")]
    pub contracts: Contracts,
    /// Layer 3 — from the integrator's system config: where and how it runs.
    #[serde(default, skip_serializing_if = "Execution::is_empty")]
    pub execution: Execution,
}

impl SystemModel {
    /// Parse from YAML, rejecting schema versions newer than this reader.
    pub fn from_yaml_str(s: &str) -> Result<Self, ModelError> {
        let model: SystemModel = serde_yaml_ng::from_str(s)?;
        if model.meta.version > SCHEMA_VERSION {
            return Err(ModelError::UnsupportedVersion {
                found: model.meta.version,
                supported: SCHEMA_VERSION,
            });
        }
        Ok(model)
    }

    /// Canonical YAML form (deterministic: all maps are ordered).
    pub fn to_yaml_string(&self) -> Result<String, ModelError> {
        Ok(serde_yaml_ng::to_string(self)?)
    }
}

// ---------------------------------------------------------------------------
// meta
// ---------------------------------------------------------------------------

/// Provenance + diagnostics. Everything needed to answer "what exactly was
/// resolved, from what inputs, by what tool" — and the cache key.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    /// SystemModel schema version (see [`SCHEMA_VERSION`]).
    pub version: u32,
    /// The exact argument binding this model was resolved from.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub args: BTreeMap<String, String>,
    /// Content hashes of every input file (manifests, launch files, system
    /// config). Sorted by path; the resolver's cache key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<InputHash>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolver: Option<ResolverInfo>,
    /// Checker warnings embedded at resolve time (Errors refuse emission).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<String>,
    /// The spawn-info companion (record.json) this model is bound to —
    /// the runtime refuses a record whose hash differs (Phase 43.1).
    /// `None` when the model was resolved to stdout (nothing to bind).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<InputHash>,
}

/// One input file's content hash.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InputHash {
    pub path: String,
    pub sha256: String,
}

/// The tool that produced this model.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ResolverInfo {
    pub tool: String,
    pub version: String,
}

// ---------------------------------------------------------------------------
// layer 1 — structure
// ---------------------------------------------------------------------------

/// The resolved graph: node instances, entities, and the scope tree (kept
/// for diagnostics and budget attribution). All names fully qualified.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Structure {
    /// Scope id (e.g. `/perception/object_recognition/tracking`) → info.
    /// The scope id is its resolved namespace path.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub scopes: BTreeMap<String, ScopeInfo>,
    /// Node FQN (`/ns/node_name`) → instance.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub nodes: BTreeMap<String, NodeInstance>,
    /// Topic FQN → wiring. Endpoint refs are `"<node FQN>/<endpoint>"`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub topics: BTreeMap<String, TopicWiring>,
    /// Service FQN → wiring.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub services: BTreeMap<String, ServiceWiring>,
    /// Action FQN → wiring.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub actions: BTreeMap<String, ServiceWiring>,
}

/// One scope (launch file / group) in the resolved tree.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScopeInfo {
    /// Parent scope id; `None` for the root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Source manifest path, when this scope had one (diagnostics only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<String>,
}

/// One resolved node instance.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NodeInstance {
    /// Owning scope id.
    pub scope: String,
    /// ROS package.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pkg: Option<String>,
    /// Executable name (plain node).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exec: Option<String>,
    /// Composable plugin type (composable node) — mutually exclusive with
    /// `exec`; `container` names the hosting container node FQN.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    /// ROS 2 lifecycle (managed) node — contracts are runtime-gated on the
    /// Active state.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub lifecycle: bool,
    /// R1-M6 — managed-node boot autostart policy (`none` = services
    /// registered, externally driven; `configure`; `active`). `None` =
    /// unspecified (consumer default `none`). Only meaningful when
    /// `lifecycle` is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle_autostart: Option<Autostart>,
    /// R1-M4 — RESOLVED ROS parameter values for this node. Parameters
    /// are system semantics (not spawn info — the two-artifact split
    /// excludes cmd/env/param FILES, not values); the embedded consumer
    /// has no record.json to read them from.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, ParamValue>,
    /// Advisory scheduling criticality (`high` | `medium` | `low`) carried
    /// through from the manifest (RT config v2 §2.1). The resolver's
    /// `SchedMapper` already consumed it when deriving
    /// [`Execution::bindings`]; it rides along for runtime diagnosis
    /// dashboards. Unrecognized values are ignored, never an error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub criticality: Option<String>,
}

/// R1-M6 — lifecycle boot autostart policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Autostart {
    None,
    Configure,
    Active,
}

/// R1-M4 — a resolved ROS parameter value. Untagged: YAML scalars map
/// naturally; order matters (Bool before Int before Float before Str so
/// `true`/`1`/`1.5`/`"x"` each hit the right arm; a float-typed param
/// authored as `1` must be written `1.0`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    StrList(Vec<String>),
}

/// Topic wiring: type + endpoint refs (`"<node FQN>/<endpoint>"`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TopicWiring {
    /// ROS message type (`pkg/msg/Name`).
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(rename = "pub", default, skip_serializing_if = "Vec::is_empty")]
    pub publishers: Vec<String>,
    #[serde(rename = "sub", default, skip_serializing_if = "Vec::is_empty")]
    pub subscribers: Vec<String>,
}

/// Service/action wiring: type + server/client endpoint refs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ServiceWiring {
    #[serde(rename = "type")]
    pub srv_type: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub server: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub client: Vec<String>,
}

// ---------------------------------------------------------------------------
// layer 2 — contracts
// ---------------------------------------------------------------------------

/// Post-merge contract numbers. Keys mirror layer 1: endpoint refs are
/// `"<node FQN>/<endpoint>"`, path keys are `"<owner>/<path name>"` where
/// the owner is a node FQN (node paths) or scope id (scope paths).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Contracts {
    /// Publisher endpoint guarantees.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub pub_endpoints: BTreeMap<String, PubContract>,
    /// Subscriber endpoint assumptions.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sub_endpoints: BTreeMap<String, SubContract>,
    /// Service server guarantees.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub srv_endpoints: BTreeMap<String, SrvContract>,
    /// Node processing paths (take → publish).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub node_paths: BTreeMap<String, PathContract>,
    /// Scope end-to-end paths (entry topic → exit topic within the subtree).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub scope_paths: BTreeMap<String, PathContract>,
    /// Per-topic channel contracts (rate, transport, drops, QoS).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub topics: BTreeMap<String, TopicContract>,
    /// Externally-provided topics: FQN → which side is external
    /// (`pub` | `sub` | `both`). Dangling-entity and runtime graph checks
    /// skip the external side.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub externals: BTreeMap<String, ExternalSide>,
}

impl Contracts {
    pub fn is_empty(&self) -> bool {
        self.pub_endpoints.is_empty()
            && self.sub_endpoints.is_empty()
            && self.srv_endpoints.is_empty()
            && self.node_paths.is_empty()
            && self.scope_paths.is_empty()
            && self.topics.is_empty()
            && self.externals.is_empty()
    }
}

/// Which side of an external topic lives outside the modeled system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExternalSide {
    Pub,
    Sub,
    Both,
}

/// Publisher guarantee.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PubContract {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_rate_hz: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rate_hz: Option<f64>,
    /// Max deviation from the ideal period.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jitter_ms: Option<f64>,
    /// R1-M5 — per-endpoint QoS (overrides the topic-level profile for
    /// this endpoint; manifest-side per-endpoint QoS + the retired 211.H
    /// launch-param overlay both land here).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qos: Option<Qos>,
}

/// Subscriber assumption.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SubContract {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_rate_hz: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rate_hz: Option<f64>,
    /// Max data age at receive: `now - header.stamp` at take.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_age_ms: Option<f64>,
    /// Polled (read-latest), not causal — breaks cycles in the dataflow DAG.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub state: bool,
    /// Must receive at least once before the node is operational.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub required: bool,
    /// R1-M5 — per-endpoint QoS (see [`PubContract::qos`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qos: Option<Qos>,
}

/// Service server guarantee.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SrvContract {
    /// Max request-to-response time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_response_ms: Option<f64>,
}

/// A causal path with a latency budget. Node paths: input/output are
/// endpoint refs and the budget is pure processing (take → publish). Scope
/// paths: input/output are topic FQNs and the budget is E2E across the
/// scope's subtree (includes internal transport); only scope paths carry
/// drop budgets.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PathContract {
    /// Empty = periodic (timer-driven).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input: Vec<String>,
    pub output: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_latency_ms: Option<f64>,
    /// Multi-input stamp matching.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation: Option<Correlation>,
    /// Max `header.stamp` spread between correlated inputs; required when
    /// `correlation` is `timestamp`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance_ms: Option<f64>,
    /// E2E drop budget (scope paths only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drop: Option<DropContract>,
}

/// Multi-input correlation mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Correlation {
    Timestamp,
    Latest,
}

/// Per-topic channel contract.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TopicContract {
    /// Negotiated channel rate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_hz: Option<f64>,
    /// Worst-case transport latency for this hop; undeclared hops contribute
    /// 0 to scope budgets (absorbed into the residual).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_transport_ms: Option<f64>,
    /// Transport drop budget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drop: Option<DropContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qos: Option<Qos>,
}

/// Drop budget, resolved to a fraction (the manifest's `N / W` form is
/// normalized by the resolver).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DropContract {
    /// Long-run drop fraction in `[0, 1]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_drop_rate: Option<f64>,
    /// Never lose more than this many in a row (runtime-only check).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_consecutive: Option<u32>,
}

/// QoS profile (string forms match the manifest format reference).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Qos {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reliability: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub durability: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifespan_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub liveliness: Option<String>,
}

// ---------------------------------------------------------------------------
// layer 3 — execution / deployment
// ---------------------------------------------------------------------------

/// Integrator-owned placement + scheduling. play_launch's runtime consumes
/// the `linux` deploy subset; the nano-ros build system consumes `mcu:*`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Execution {
    /// Node FQN → placement.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub deploy: BTreeMap<String, Deploy>,
    /// Tier name → definition (sched crate schema — see [`TierDef`]).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tiers: BTreeMap<String, TierDef>,
    /// `"<node FQN>"` or `"<node FQN>/<callback group>"` → tier name.
    /// RESOLVED form: the resolver flattens the authoring-side sparse
    /// `assign` rules (`sched::AssignRule`, scope prefixes + node lists)
    /// into explicit per-node bindings — early binding, like everything
    /// else in the model.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bindings: BTreeMap<String, String>,
    /// R1-M2 — transport/session declarations (network identity + the
    /// (rmw, locator, domain) session tuple). Folds multi-domain
    /// routing: a node binds a transport by `id`, each transport is one
    /// session. The embedded boot bake reads its board's transport here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transports: Vec<Transport>,
    /// R1-M3 — in-binary topic-relay bridges (nano-ros RFC-0009 shape).
    /// Topic types resolve from layer 1 wiring, never written here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bridges: Vec<Bridge>,
    /// R1-M3 — system capability axes (`safety`, `param_services`, …).
    /// Unknown names are a consumer bake-time error.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
    /// Phase 45.2 — the resolved scheduling plan (`docs/design/
    /// system-model-sched-ssot.md`: "SSoT owns the structure; each back-end
    /// owns its realization"). `None` when `resolve` had no chain/mapper
    /// facts to embed (no `chains:`/platform-file mapper input) — old
    /// `system_model.yaml` artifacts (tiers+bindings only) parse unchanged.
    /// See [`ExecutionSched`] for the structure/realization split.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sched: Option<ExecutionSched>,
}

/// R1-M2 — one transport/session declaration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Transport {
    /// `ethernet` | `wifi` | `serial` | `can` | `loopback`.
    pub kind: String,
    /// Stable id nodes/bridges bind by; `None` ⇒ keyed by `rmw`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// IPv4 CIDR (`"10.0.2.50/24"`) or `"dhcp"` — ethernet/wifi.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    /// Ethernet MAC; `None` ⇒ board fused MAC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac: Option<String>,
    /// Default IPv4 gateway — ethernet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<String>,
    /// NIC names this session multi-homes over (one discovery graph).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interfaces: Vec<String>,
    /// WiFi SSID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssid: Option<String>,
    /// WiFi passphrase.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// Device handle (`"UART0"`, `"CAN0"`) — serial/can.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// Serial baud / CAN bitrate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baudrate: Option<u32>,
    /// RMW riding this transport; `None` ⇒ deploy/system default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rmw: Option<String>,
    /// Session locator; `None` ⇒ platform default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,
    /// ROS domain this session joins; `None` ⇒ system default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<u8>,
}

/// R1-M3 — one in-binary bridge.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Bridge {
    pub name: String,
    /// Source transport id (or rmw key).
    pub from: String,
    /// Destination transport id (or rmw key).
    pub to: String,
    /// Forwarded topic FQNs; empty ⇒ every declared topic.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub topics: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bidirectional: bool,
}

impl Execution {
    pub fn is_empty(&self) -> bool {
        self.deploy.is_empty()
            && self.tiers.is_empty()
            && self.bindings.is_empty()
            && self.transports.is_empty()
            && self.bridges.is_empty()
            && self.features.is_empty()
            && self.sched.is_none()
    }
}

// ---------------------------------------------------------------------------
// layer 3 — resolved scheduling plan (Phase 45.2/45.3)
// ---------------------------------------------------------------------------

/// The resolved scheduling plan the SSoT design
/// (`docs/design/system-model-sched-ssot.md`) embeds in `execution:`.
/// Reconciled with the nano-ros track 2026-07-19
/// (`src/ros-launch-manifest/docs/scheduling.md` §"Cross-repo design
/// agreement"; nano-ros RFC-0050/0052): **the SSoT owns the *structure*;
/// each back-end owns its *realization*.**
///
/// - [`Self::chains`] and [`Self::requirements`] are SHARED STRUCTURE — the
///   mapper INPUT both play_launch's Linux runtime and nano-ros's own
///   RTOS-framework-aware mapper read. Resolved once, at `resolve` time
///   (`sched_derive::resolve_chains`, play_launch, Phase 44.4), never
///   re-derived per consumer.
/// - [`Self::mapper`] and [`Self::ranks`] are the LINUX REALIZATION —
///   play_launch writes and reads them (PiCAS fixed-priority ranks); nano-ros
///   IGNORES both fields and derives its own kernel-feature bindings
///   (EDF / preemption-threshold / sporadic / affinity) from the shared
///   structure above.
///
/// All four fields are independently optional/empty — a `resolve` with only
/// a manual `system.toml` (no `chains:`, no derived mapper) may embed
/// `mapper`/`ranks` but empty `chains`/`requirements`, and vice versa for a
/// hypothetical structure-only embed. The whole struct is `None` when there
/// is nothing to embed at all (see [`Execution::sched`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExecutionSched {
    /// SHARED STRUCTURE — resolved chains: FQN-qualified `via` topics, the
    /// segment/boundary decomposition, each chain's budget/semantics.
    /// Embedded verbatim from `ros_launch_manifest_sched` — no parallel
    /// copy (`docs/design/system-model-sched-ssot.md` "Type sharing").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chains: Vec<ros_launch_manifest_sched::ResolvedChain>,
    /// SHARED STRUCTURE — per-(node, path) requirement facts, FQN-keyed:
    /// the four facts nano-ros's six-dim RTOS mapper needs, carried once and
    /// never re-derived per consumer. Per path (on
    /// [`ros_launch_manifest_sched::MapperPath`]): effective **trigger**
    /// (`effective_trigger`), **deadline** (`max_latency_ms`), and **budget**
    /// (`exec_ms` — the WCET/execution-time dimension, distinct from the
    /// deadline; optional, `None` when undeclared — no invented WCET). Per
    /// node (on [`NodeSchedRequirement`]): **criticality**.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requirements: Vec<NodeSchedRequirement>,
    /// LINUX REALIZATION — identity of the `SchedMapper` that produced the
    /// applied plan (provenance: `"chain_aware"`, `"rate_monotonic"`,
    /// `"deadline_monotonic"`, `"manual"`, …). nano-ros IGNORES this: it
    /// runs its own mapper over [`Self::chains`]/[`Self::requirements`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mapper: Option<String>,
    /// LINUX REALIZATION — per-(node, path) PiCAS fixed-priority ranks +
    /// provenance, play_launch's Linux-specific projection of the shared
    /// structure. Harmless to carry (nano-ros ignores it; `provenance` may
    /// still surface for cross-platform diagnostics) but NOT the shared
    /// interface.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ranks: Vec<ros_launch_manifest_sched::ChainAwareDetail>,
    /// LINUX REALIZATION — FQNs of nodes whose final priority/class/core was
    /// pinned by an explicit `overrides.<selector>` entry in the platform
    /// file (Phase 45.6 review). This is the fact `--explain`'s provenance
    /// renderer needs to label a node `override(...)` **exactly**: an
    /// override's applied value can coincide with what the mapper would have
    /// derived anyway (a pinning override, or coincidence after a contract
    /// change), so a model consumer cannot recover "was this overridden?"
    /// from `bindings`/`tiers`/`ranks` alone — carrying the set is the only
    /// way to reproduce the fresh-derive renderer's classification (which
    /// reads its own live `overridden` map). nano-ros IGNORES this. Empty /
    /// absent when nothing was overridden (or an older artifact) — old
    /// models parse unchanged.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub overrides: BTreeSet<String>,
}

impl ExecutionSched {
    pub fn is_empty(&self) -> bool {
        self.chains.is_empty()
            && self.requirements.is_empty()
            && self.mapper.is_none()
            && self.ranks.is_empty()
            && self.overrides.is_empty()
    }
}

/// One node's per-path scheduling requirement facts (Phase 45.2) — FQN-
/// keyed shared structure. Wraps [`ros_launch_manifest_sched::MapperPath`]
/// (already exactly "per path: name / effective trigger / max_latency_ms /
/// exec_ms / inputs / outputs") with the per-node `criticality` the FQN
/// groups them under, rather than hand-mirroring a parallel `PathRequirement`
/// type.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NodeSchedRequirement {
    /// The node's fully-qualified name (matches [`Structure::nodes`] keys).
    pub node_fqn: String,
    /// Advisory scheduling criticality carried through from the manifest
    /// (mirrors [`NodeInstance::criticality`], but typed — this is the
    /// mapper-facing fact, not the diagnostics-facing string).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub criticality: Option<ros_launch_manifest_sched::Criticality>,
    /// The node's declared causal paths with their resolved trigger/budget
    /// facts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<ros_launch_manifest_sched::MapperPath>,
}

/// Where a node runs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Deploy {
    /// `linux` or `mcu:<board>` (see [`Target`]).
    pub target: Target,
    /// Host name for multi-host Linux deployments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// R1-M1 — ROS domain for this node's session (RFC-0045 baked rung
    /// on embedded). `None` = system default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<u8>,
    /// R1-M1 — session locator override. `None` = backend default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,
    /// R1-M1 — RMW backend for this node's session (`zenoh` | `xrce` |
    /// `cyclonedds` | …). `None` = system default. The embedded bake
    /// cannot pick a backend without one of the two.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rmw: Option<String>,
    /// R1-M1 — consumer-defined extras (build tuning, runner knobs:
    /// `profile`, `optimize`, `kind`, `framework`, cargo `features`…).
    /// Open map by design: these are per-consumer build semantics, not
    /// cross-runtime system semantics — but they ride the model so the
    /// consumer never parses `system.toml` directly (canonical-path
    /// decision).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, ExtraValue>,
}

/// R1-M1 — a deploy-extra value (untagged; Bool before Int before Float
/// before Str, then string lists).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExtraValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    StrList(Vec<String>),
}

/// Deployment target. Serialized as `linux` or `mcu:<board>`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Target {
    #[default]
    Linux,
    Mcu {
        board: String,
    },
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Target::Linux => write!(f, "linux"),
            Target::Mcu { board } => write!(f, "mcu:{board}"),
        }
    }
}

impl std::str::FromStr for Target {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "linux" {
            return Ok(Target::Linux);
        }
        if let Some(board) = s.strip_prefix("mcu:") {
            if board.is_empty() {
                return Err("empty board in `mcu:<board>` target".into());
            }
            return Ok(Target::Mcu {
                board: board.to_string(),
            });
        }
        Err(format!(
            "unknown target `{s}` (expected `linux` or `mcu:<board>`)"
        ))
    }
}

impl Serialize for Target {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Target {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Tier definitions reuse the sched crate's schema (RT config v2, phase 41):
/// a portable head (`class`, `deadline_us`, `period_us`, `budget_us`,
/// `deadline_policy`, `spin_period_us`) plus fixed per-platform placement
/// sub-tables (`posix`/`freertos`/`zephyr`/`threadx`/`nuttx` — `priority`,
/// `stack_bytes`, `core`, `sched_class`, `preempt_threshold`, per-platform
/// `deadline_us` override). One schema for the authoring form
/// (`system.toml`), the model, and the `SchedMapper` pipeline.
pub use ros_launch_manifest_sched::types::{TierDef, TierPlatformSpec};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_string_forms() {
        assert_eq!("linux".parse::<Target>().unwrap(), Target::Linux);
        assert_eq!(
            "mcu:stm32f4".parse::<Target>().unwrap(),
            Target::Mcu {
                board: "stm32f4".into()
            }
        );
        assert_eq!(
            Target::Mcu {
                board: "stm32f4".into()
            }
            .to_string(),
            "mcu:stm32f4"
        );
        assert!("mcu:".parse::<Target>().is_err());
        assert!("cloud".parse::<Target>().is_err());
    }

    #[test]
    fn version_gate_rejects_newer_schema() {
        let yaml = "meta:\n  version: 999\nstructure: {}\n";
        match SystemModel::from_yaml_str(yaml) {
            Err(ModelError::UnsupportedVersion {
                found: 999,
                supported,
            }) => {
                assert_eq!(supported, SCHEMA_VERSION)
            }
            other => panic!("expected UnsupportedVersion, got {other:?}"),
        }
    }

    #[test]
    fn empty_layers_are_omitted_from_yaml() {
        let model = SystemModel {
            meta: Meta {
                version: SCHEMA_VERSION,
                ..Default::default()
            },
            ..Default::default()
        };
        let yaml = model.to_yaml_string().unwrap();
        assert!(
            !yaml.contains("contracts:"),
            "empty contracts serialized: {yaml}"
        );
        assert!(
            !yaml.contains("execution:"),
            "empty execution serialized: {yaml}"
        );
    }

    /// Phase 45.2/45.3 — builds an `Execution` with the full sched
    /// structure (chains + per-node requirements + realization
    /// mapper/ranks), round-trips it through YAML, and asserts both
    /// equality and the shape of the emitted document (structure vs
    /// realization grouping under one `sched:` key).
    #[test]
    fn execution_sched_roundtrips_with_full_structure() {
        use ros_launch_manifest_sched::{
            ChainAwareDetail, ChainElement, ChainSemantics, Criticality, EffectiveTrigger,
            MapperPath, ResolvedChain, SegmentNode,
        };

        let execution = Execution {
            sched: Some(ExecutionSched {
                chains: vec![ResolvedChain {
                    name: "sensing_to_actuation".into(),
                    criticality: Criticality::High,
                    max_latency_ms: 100.0,
                    semantics: ChainSemantics::Reaction,
                    elements: vec![
                        ChainElement::Segment {
                            nodes_in_topo_order: vec![SegmentNode {
                                node: "/sensing/detector".into(),
                                path: "main".into(),
                            }],
                        },
                        ChainElement::Boundary {
                            node: "/planning/planner".into(),
                            path: "main".into(),
                            period_ms: 20.0,
                            exec_ms: None,
                        },
                    ],
                }],
                requirements: vec![NodeSchedRequirement {
                    node_fqn: "/sensing/detector".into(),
                    criticality: Some(Criticality::High),
                    paths: vec![MapperPath {
                        name: "main".into(),
                        effective_trigger: EffectiveTrigger::Input(vec![
                            "/sensing/pointcloud".into(),
                        ]),
                        max_latency_ms: Some(30.0),
                        // budget (WCET) fact declared for this path.
                        exec_ms: Some(4.0),
                        inputs: vec!["/sensing/pointcloud".into()],
                        outputs: vec!["/sensing/objects".into()],
                    }],
                }],
                mapper: Some("chain_aware".into()),
                ranks: vec![ChainAwareDetail {
                    node: "/sensing/detector".into(),
                    path: Some("main".into()),
                    priority: 44,
                    provenance: "derived(chain_aware: sensing_to_actuation S1 -> prio 44)".into(),
                }],
                overrides: BTreeSet::from(["/planning/planner".to_string()]),
            }),
            ..Default::default()
        };

        let yaml = serde_yaml_ng::to_string(&execution).unwrap();
        // structure/realization grouping — one `sched:` key, five sub-keys.
        assert!(yaml.contains("sched:"), "{yaml}");
        assert!(yaml.contains("chains:"), "{yaml}");
        assert!(yaml.contains("requirements:"), "{yaml}");
        assert!(yaml.contains("mapper: chain_aware"), "{yaml}");
        assert!(yaml.contains("ranks:"), "{yaml}");
        // overridden-node set (Phase 45.6): FQNs pinned by platform overrides.
        assert!(yaml.contains("overrides:"), "{yaml}");
        assert!(yaml.contains("- /planning/planner"), "{yaml}");
        // FQN-keyed requirement fact, not a map (Vec<NodeSchedRequirement>
        // with an explicit `node_fqn` field — see doc comment rationale).
        assert!(yaml.contains("node_fqn: /sensing/detector"), "{yaml}");
        // per-path budget (WCET) fact carried alongside the deadline.
        assert!(yaml.contains("exec_ms: 4.0"), "{yaml}");
        // named segment entries: node:/path: mappings, not bare tuples.
        assert!(yaml.contains("node: /sensing/detector"), "{yaml}");

        let reparsed: Execution = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(execution, reparsed);
    }

    /// An old `system_model.yaml` execution block (tiers+bindings only, no
    /// sched structure at all) must still parse, with the new fields
    /// defaulting to empty/`None` — the additive-schema guarantee.
    #[test]
    fn execution_without_sched_key_parses_with_defaults() {
        let yaml = "\
tiers:
  high:
    posix:
      priority: 80
bindings:
  /some/node: high
";
        let execution: Execution = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(execution.tiers["high"].posix.as_ref().unwrap().priority, 80);
        assert_eq!(execution.bindings["/some/node"], "high");
        assert!(execution.sched.is_none());
        assert!(!execution.is_empty());

        // and round-tripping it back doesn't invent a `sched:` key.
        let re_emitted = serde_yaml_ng::to_string(&execution).unwrap();
        assert!(!re_emitted.contains("sched:"), "{re_emitted}");
    }

    /// `ExecutionSched` itself round-trips (belt-and-suspenders on top of
    /// the full-`Execution` test above).
    #[test]
    fn execution_sched_struct_roundtrips_standalone() {
        use ros_launch_manifest_sched::Criticality;

        let sched = ExecutionSched {
            mapper: Some("rate_monotonic".into()),
            requirements: vec![NodeSchedRequirement {
                node_fqn: "/a/b".into(),
                criticality: Some(Criticality::Medium),
                paths: vec![],
            }],
            ..Default::default()
        };
        let yaml = serde_yaml_ng::to_string(&sched).unwrap();
        let reparsed: ExecutionSched = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(sched, reparsed);
    }

    /// Backward-compat for the Phase 45.6 `overrides` field: a `sched:`
    /// block written before that field existed (no `overrides:` key) parses
    /// with the field defaulting to an empty set; and a set with entries
    /// round-trips losslessly, emitting the key only when non-empty
    /// (`skip_serializing_if`).
    #[test]
    fn execution_sched_overrides_field_is_backward_compatible() {
        // 1. Old artifact: a `sched:` with mapper/ranks but no `overrides:`.
        let old_yaml = "\
mapper: chain_aware
ranks:
  - node: /a
    priority: 44
    provenance: 'derived(chain_aware: c -> prio 44)'
";
        let sched: ExecutionSched = serde_yaml_ng::from_str(old_yaml).unwrap();
        assert!(
            sched.overrides.is_empty(),
            "missing overrides: key must default to empty"
        );

        // 2. Empty set is omitted from the emitted YAML (no noise).
        let re_emitted = serde_yaml_ng::to_string(&sched).unwrap();
        assert!(!re_emitted.contains("overrides:"), "{re_emitted}");

        // 3. A populated set round-trips.
        let with_overrides = ExecutionSched {
            mapper: Some("chain_aware".into()),
            overrides: BTreeSet::from(["/a".to_string(), "/b/c".to_string()]),
            ..Default::default()
        };
        let yaml = serde_yaml_ng::to_string(&with_overrides).unwrap();
        assert!(yaml.contains("overrides:"), "{yaml}");
        let reparsed: ExecutionSched = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(with_overrides, reparsed);
    }
}
