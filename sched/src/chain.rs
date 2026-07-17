//! Chain-aware types (Phase 44.3, design
//! `docs/superpowers/specs/2026-07-17-chain-aware-mapper-design.md`):
//! per-path trigger facts and resolved chains, consumed by
//! [`crate::chain_aware_mapper::ChainAwareMapper`].
//!
//! These mirror `ros-launch-manifest-types::EffectiveTrigger`/`ChainDecl`
//! (W1 report) minimally rather than depending on the `types` crate — the
//! sched crate has no dependency on `ros-launch-manifest-types` today (see
//! `sched/Cargo.toml`) and is kept FQN-string-based/dependency-light,
//! consistent with [`crate::resolve::SchedNode`] and
//! [`crate::mapper::MapperNode`]. Callers (play_launch, wave 4) translate
//! from the real `types` crate's `PathDecl::effective_trigger()` /
//! `ChainDecl` into these when building [`crate::mapper::MapperInput`].

use crate::mapper::Criticality;

/// Mirrors `ros_launch_manifest_types::EffectiveTrigger` (W1 report): the
/// resolved trigger fact for one declared path, after the legacy-`input:`
/// derivation rule has already been applied by the caller.
#[derive(Clone, Debug, PartialEq)]
pub enum EffectiveTrigger {
    /// Clock-sampled: fires on a wall-clock timer at `rate_hz`.
    Timer { rate_hz: f64 },
    /// Event-driven: fires when (all/any of, per `sync:`) these endpoints
    /// receive data. The endpoint names themselves aren't interpreted by
    /// this crate.
    Input(Vec<String>),
    /// Fires exactly once (e.g. a map loader).
    Once,
    /// Fires at caller-unpredictable times (e.g. a service/action server).
    Spontaneous,
    /// No trigger fact could be derived (no explicit `trigger:`, no legacy
    /// `input:`).
    Unclassified,
}

impl EffectiveTrigger {
    /// The timer period in milliseconds, or `None` for anything but
    /// `Timer` with a positive `rate_hz`.
    pub fn period_ms(&self) -> Option<f64> {
        match self {
            EffectiveTrigger::Timer { rate_hz } if *rate_hz > 0.0 => Some(1000.0 / rate_hz),
            _ => None,
        }
    }
}

/// One of a node's declared causal paths, as seen by the mapper (design
/// steps 1 and 6).
#[derive(Clone, Debug, PartialEq)]
pub struct MapperPath {
    pub name: String,
    pub effective_trigger: EffectiveTrigger,
    /// Declared end-to-end latency budget for this path (`max_latency_ms`
    /// in the contract vocabulary), when present.
    pub max_latency_ms: Option<f64>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
}

/// A chain's declared latency semantics (mirrors `types::ChainSemantics`).
/// Not otherwise interpreted by the mapper (the reaction-vs-age distinction
/// affects the companion checker's arithmetic, not rank assignment).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainSemantics {
    Reaction,
    Age,
}

/// One element of a resolved chain's clock-segmented decomposition (design
/// "Model: clock-segmented chains"). Already resolved by the caller
/// (wave 4): `via` scopes flattened, and each segment's
/// `nodes_in_topo_order` already computed in source-to-sink order (design
/// issue #3 — fan-in longest-path-to-sink/deadline/name tie-breaks require
/// the launch DAG, which only the caller has; this crate consumes the
/// already-linearized result).
#[derive(Clone, Debug, PartialEq)]
pub enum ChainElement {
    /// A maximal run of `trigger: input` path links, in source-to-sink
    /// topological order. Each entry is `(node_name, path_name)`.
    Segment {
        nodes_in_topo_order: Vec<(String, String)>,
    },
    /// A `trigger: timer` hop crossed by the chain. `period_ms` is
    /// `1000 / rate_hz`; `exec_ms` is an optional declared execution-time
    /// fact (no WCETs are invented — `None` when not declared).
    Boundary {
        node: String,
        path: String,
        period_ms: f64,
        exec_ms: Option<f64>,
    },
}

/// A declared chain (`chains:`), fully resolved against the launch DAG
/// (design steps 1–2 input).
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedChain {
    pub name: String,
    /// Chain criticality, used for chain ordering (design step 3:
    /// "criticality desc"). The W1 vocabulary has no chain-level
    /// `criticality` field, so the caller derives this as the **max**
    /// over the chain's member nodes' `criticality` (documented choice —
    /// a pure aggregation of already-available per-node facts, not
    /// mapper logic).
    pub criticality: Criticality,
    /// The chain's declared end-to-end latency budget.
    pub max_latency_ms: f64,
    pub semantics: ChainSemantics,
    pub elements: Vec<ChainElement>,
}

/// One rank decision's per-(node, path) detail, for `--explain` support
/// (design step 8). Returned by
/// [`crate::mapper::SchedMapper::map_with_diagnostics`].
#[derive(Clone, Debug, PartialEq)]
pub struct ChainAwareDetail {
    pub node: String,
    /// `None` only if a future caller ever needs a node-level (not
    /// path-level) rank decision; the `chain_aware` mapper always
    /// populates this today.
    pub path: Option<String>,
    /// The path's final priority (POSIX RT priority, same convention as
    /// [`crate::resolve::ResolvedTier::priority`]).
    pub priority: i64,
    /// One-line human-readable provenance, e.g.
    /// `derived(chain_aware: sensing_to_actuation S2 drain 2/2 -> prio 44)`.
    pub provenance: String,
}

/// A diagnostic surfaced while deriving a plan (design step 8: chain
/// feasibility, co-location warnings, etc). Additive — not every mapper
/// produces any.
#[derive(Clone, Debug, PartialEq)]
pub enum MapWarning {
    /// A chain's `sampling_cost` (design "Model: clock-segmented chains")
    /// consumes its entire declared budget or more — scheduling cannot fix
    /// this (period/architecture change required). The chain is excluded
    /// from priority shaping; its members keep their local-fact priorities.
    ChainInfeasible {
        chain: String,
        sampling_cost_ms: f64,
        budget_ms: f64,
    },
    /// The platform's `rt_priority_band` is narrower than the number of
    /// distinct priority classes that remain after every *legal* collapse
    /// (within segments, then within the same chain) has been applied —
    /// design step 7 / issue #5 forbids collapsing across the
    /// chain/non-chain divide or across criticality buckets, so the mapper
    /// keeps the least-bad assignment (clamping the lowest classes into
    /// `band.min`, which introduces cross-chain/bucket *ties*, never
    /// inversions) and reports it here instead of silently pretending the
    /// band fit.
    BandTooNarrow {
        /// Distinct priority classes remaining after legal collapse.
        distinct_classes: usize,
        /// The band's inclusive width (`max - min + 1`).
        band_width: usize,
    },
}

/// The richer output of [`crate::mapper::SchedMapper::map_with_diagnostics`]:
/// per-(node, path) rank provenance plus any warnings, alongside the
/// [`crate::mapper::SchedPlan`] itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MapDiagnostics {
    pub details: Vec<ChainAwareDetail>,
    pub warnings: Vec<MapWarning>,
}
