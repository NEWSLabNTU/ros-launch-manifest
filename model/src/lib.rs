//! SystemModel — the resolved, checked system artifact.
//!
//! One `SystemModel` describes ONE concrete system variant (early binding:
//! args bound, conditions evaluated, every name fully qualified), produced by
//! a resolver from the launch tree + contract manifests + the integrator's
//! system config, and consumed by realizers, for example
//!
//! - a process runtime (spawn/supervise + contract monitors), and
//! - an embedded build system (bake each node's slice into its image).
//!
//! What the model may carry: `docs/model-boundary.md`. This crate names no
//! realizer; history lives in the CHANGELOG. This crate is PURE SCHEMA — types,
//! serde, and YAML round-trip; no resolver logic. The resolver refuses to
//! emit a model when the manifest checker reports Error severity, so a model
//! in hand is always a checked one; warnings ride along in
//! [`Meta::diagnostics`].
//!
//! All collections are `BTreeMap`/sorted so serialization is deterministic —
//! the YAML form is hashed for provenance and caching.

use indexmap::IndexMap;
use std::collections::BTreeMap;

pub mod system_config;

use serde::{Deserialize, Serialize};

/// Schema version this crate reads and writes.
///
/// Versioned independently of the manifest format version: the manifest is
/// the authoring surface, the model is a derived artifact.
///
/// - **2** (v0.1.51): a [`ParamValue::Str`] is always a string, and a list
///   parameter is a [`ParamValue::StrList`] (see its doc for the element
///   spelling). In version 1 an inline list rode as a `Str` holding its YAML
///   flow text, so a string whose text looked like a list (`type="str"`,
///   `value="[a, b]"`) could not be told from one, and was spawned as a list
///   (history: CHANGELOG). [`SystemModel::from_yaml_str`] upgrades a
///   version-1 model on read, giving it the meaning it had then.
pub const SCHEMA_VERSION: u32 = 2;

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
    ///
    /// A version-1 model is upgraded to the current schema: an inline
    /// parameter whose `Str` is YAML flow text (`[a, b]`) meant a list then,
    /// and becomes a [`ParamValue::StrList`] (see [`SCHEMA_VERSION`]).
    pub fn from_yaml_str(s: &str) -> Result<Self, ModelError> {
        let mut model: SystemModel = serde_yaml_ng::from_str(s)?;
        if model.meta.version > SCHEMA_VERSION {
            return Err(ModelError::UnsupportedVersion {
                found: model.meta.version,
                supported: SCHEMA_VERSION,
            });
        }
        // Exactly 1: version 0 is only `Meta::default()`, which no producer
        // writes, and treating it as old would change a model in memory.
        if model.meta.version == 1 {
            model.upgrade_v1_lists();
            model.meta.version = SCHEMA_VERSION;
        }
        Ok(model)
    }

    /// Version 1 -> 2: a `Str` holding flow-sequence text was a list.
    fn upgrade_v1_lists(&mut self) {
        let upgrade = |v: &mut ParamValue| {
            if let ParamValue::Str(text) = v
                && let Some(list) = ParamValue::list_from_flow(text)
            {
                *v = list;
            }
        };
        for inst in self.structure.nodes.values_mut() {
            inst.params.values_mut().for_each(upgrade);
            for src in &mut inst.param_sources {
                if let ParamSource::Inline { value, .. } = src {
                    upgrade(value);
                }
            }
        }
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
    // The `record` field (a bound `record.json`
    // companion) is retired: the producer only ever wrote `None`, the files it
    // named were never committed, and its sole reader was a round-trip test.
    // Old models still carry `record:` on disk; without `deny_unknown_fields`
    // it deserializes and is dropped, so removal is backward-compatible on read.
}

/// Render an input path for [`InputHash::path`], relative to `base` when the
/// path lies under it.
///
/// These were `canonicalize`d absolute paths, baked into a COMMITTED artifact.
/// That is not merely untidy — consumers read them back (a realizer looks for
/// the `.toml` input to find the system.toml the model was resolved
/// against), and an absolute path from another machine fails its existence
/// check and silently falls back to a fixed filename, reintroducing the
/// per-target leak that recording the input was meant to prevent.
///
/// Relative to the bringup package root the model is portable, and the
/// consumer resolves it against its own root. An input OUTSIDE that root (an
/// include from a sibling package) keeps its absolute path: no relative form
/// is meaningful across packages, and a `../..` chain would be worse than
/// useless in an install space.
pub fn input_path_string(path: &std::path::Path, base: Option<&std::path::Path>) -> String {
    let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if let Some(base) = base {
        let base = std::fs::canonicalize(base).unwrap_or_else(|_| base.to_path_buf());
        if let Ok(rel) = canon.strip_prefix(&base) {
            return rel.display().to_string();
        }
    }
    canon.display().to_string()
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
    /// Scope id (e.g. `autoware_launch/planning_simulator.launch.xml`) → info.
    /// The scope id is STRUCTURAL — the launch file (`<pkg>/<file>`) the scope
    /// corresponds to, disambiguated with `#<id>` when a file is included more
    /// than once. It is NOT a namespace (Phase 48: namespace is a per-node
    /// property, not a scope property). `<group>` scopes are folded into their
    /// launch file, so this map is the include tree.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub scopes: BTreeMap<String, ScopeInfo>,
    /// Node key → instance.
    ///
    /// **The key is a resolve-time identity, not necessarily a ROS name.** It
    /// is the node's fully-qualified ROS name exactly when the launch file
    /// declared one (`NodeInstance::node_name.is_some()`, always true for
    /// containers and composable nodes). For a `<node>` with no `name=`, the
    /// key is built from the EXECUTABLE — with a `-N` ordinal marking it as
    /// such — while the process registers whatever name it was compiled with:
    ///
    /// ```text
    /// key:   /localization/pose_twist_fusion_filter/autoware_ekf_localizer_node-1
    /// graph: /localization/pose_twist_fusion_filter/ekf_localizer
    /// ```
    ///
    /// So **do not join this map against `ros2 node list` unless
    /// `node_name.is_some()`.** 17 of 145 keys on one Autoware stack are
    /// executable-derived, and the mismatch raises no error: nothing in the
    /// resolve pipeline looks at a live system, and the checker builds its
    /// graph from the manifest.
    ///
    /// A producer must NOT close the gap by emitting `-r __node:=<key>`.
    /// Stock `launch_ros` emits that remap only for a declared name
    /// (`node.py:493`), so forcing it renames a node away from its
    /// internally-hardcoded default and breaks discovery for anything
    /// addressing it by that name — LifecycleNode services included. The key
    /// is the synthetic identifier; the node's own name is authoritative.
    ///
    /// An ORDER-PRESERVING map, not a `BTreeMap`. Construct order
    /// is semantic: components initialize (and `configure()`) in the order the
    /// emitter walks this mapping, and the launch file is where the user
    /// expresses it. A `BTreeMap` alphabetized that away at resolve time, so a
    /// launch declaring `talker` before `listener` produced an entry that built
    /// `listener` first. The resolver already inserts in launch-traversal
    /// order; this keeps it.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub nodes: IndexMap<String, NodeInstance>,
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

/// One launch-file scope in the resolved include tree (Phase 48: group scopes
/// are folded into their file scope).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScopeInfo {
    /// Parent scope id; `None` for the root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Source manifest path, when this scope had one (diagnostics only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<String>,
    /// ROS package of the launch file, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// Launch file basename (e.g. `control.launch.xml`), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// ROS namespace in effect where this file was included (`/perception`
    /// for an include under `<push-ros-namespace namespace="perception"/>`).
    /// Absent means the root namespace, `/`. For launch-tree inspection: a
    /// node's own namespace is in its FQN, but the namespace an include was
    /// made under is not recoverable from its nodes, which may set their own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ns: Option<String>,
    /// Resolved launch arguments passed to this include (`<include>`'s
    /// `<arg>`s), for launch-tree inspection. File scopes only.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub args: BTreeMap<String, String>,
}

/// One resolved node instance.
/// What a node's exit does to the rest of the system (`NodeInstance::on_exit`).
/// `launch` defines one such action for the XML/YAML frontends, `shutdown`;
/// the Python API's other `on_exit` actions are not modelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnExit {
    /// End the launch: `Shutdown()`, or an `EmitEvent` of
    /// `launch.events.Shutdown`.
    Shutdown,
}

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
    /// has no record.json to read them from. Phase 46.3a: the producer
    /// pre-merges scope-wide `SetParameter`/global params (lower
    /// precedence) with node-specific inline `<param>` values (higher
    /// precedence, matching `node_cmdline.rs`'s `global` → `node_specific`
    /// chain order) BEFORE lowering into this map — global params have no
    /// separate field here, they're already folded in with the right
    /// precedence. See [`Self::params_files`] for externally-referenced
    /// YAML, which is NOT folded into this map (kept verbatim).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, ParamValue>,
    /// The criticality LABEL as authored (`nodes.<n>.criticality`, RT
    /// config v2 section 2.1), a raw string carried for runtime diagnosis
    /// dashboards. It is not the fact a scheduler reads: a hazard that
    /// reaches the node may raise its criticality above the label (phase
    /// 72), and only [`Contracts::node_criticality`] carries that result.
    /// Unrecognized values are ignored, never an error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub criticality: Option<String>,

    // -- Phase 46.1b — launch spawn inputs (docs/design/unified-system-model.md
    // decision (a)) -------------------------------------------------------
    //
    // The following fields are launch-derived spawn INPUTS a process runtime
    // consumes to derive argv/env at spawn time. An embedded realizer may
    // consume `remaps` (baking the rules into entity creation) and ignore the
    // other three — it has no argv/process/respawn model for its targets.
    // They ride along per the "all launch info in the shared model"
    // principle rather than a realizer-private side channel. Additive: old
    // models with none of these keys parse unchanged (all default to
    // empty/`None`).
    /// Topic/service name remappings (`<remap from= to=/>`), in launch
    /// declaration order. Regular nodes, containers, and composable nodes
    /// (`load_node`) may all carry remaps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remaps: Vec<Remap>,
    /// Extra CLI args appended after `--ros-args` (the launch API's
    /// `ros_arguments`/`ros_args`, distinct from the plain `args` a raw
    /// executable gets). Only regular nodes and containers have a process
    /// to append these to; composable nodes don't spawn one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ros_args: Vec<String>,
    /// Respawn-on-exit policy. `None` = launch default (no respawn).
    /// Nodes/containers only — composable nodes have no process lifecycle
    /// independent of their container.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub respawn: Option<bool>,
    /// Delay in seconds before a respawn attempt. Only meaningful when
    /// `respawn` is `Some(true)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub respawn_delay: Option<f64>,
    /// `on_exit=Shutdown()` (Python) / `on_exit="shutdown"` (XML, YAML): this
    /// process is REQUIRED, and when it exits the whole system is shut down.
    /// `None` = launch default (an exit ends nothing else). A launch fact, like
    /// `respawn` beside it: it says what the system does when a process ends,
    /// not how anything is built (`docs/model-boundary.md`). Nodes/containers
    /// only — a composable node has no process of its own. Without this
    /// field the flag lived only in a launch-time dump, so a runtime started
    /// from the model alone could not honour it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_exit: Option<OnExit>,
    /// Launch-declared environment variables (`<env name= value=/>`), in
    /// declaration order — a `Vec` (not `BTreeMap`) because launch env lists
    /// may legitimately repeat a name (last one spawn-time wins) and order
    /// is part of the launch author's intent. Nodes/containers only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<EnvVar>,

    // -- Phase 46.3a — close the spawn-completeness gaps found while
    // auditing whether the model can drive a process runtime (Wave B)
    // without `record.json` (`docs/design/unified-system-model.md`,
    // `.superpowers/sdd/p46-w3-analysis.md`). Additive: old models with
    // none of these keys parse unchanged (all default to empty). --------
    /// Extra non-ROS CLI arguments appended before `--ros-args` (the launch
    /// API's `arguments=[...]` / `<node><arg .../></node>`), distinct from
    /// [`Self::ros_args`] which lands *after* `--ros-args`. Regular nodes
    /// and containers only — composable nodes have no process to append
    /// these to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// Resolved content of externally referenced param YAML files
    /// (`<param from="…"/>`, or the launch API's `parameters=[...]` file
    /// list), one string per file, in declaration order. Already
    /// `$(var …)`-resolved by the parser (`load_and_resolve_param_file`) —
    /// carried VERBATIM, not flattened into [`Self::params`]: nested/
    /// wildcard YAML (`/**: ros__parameters: {...}`) cannot survive a flat
    /// `String -> ParamValue` map without losing the wildcard-vs-node
    /// namespace distinction. The consumer materializes each string to a
    /// temp file and passes `--params-file <path>` at spawn — the exact
    /// pattern `node_cmdline.rs` already uses for `NodeRecord::params_files`
    /// today. Regular nodes and containers only (composable nodes take
    /// parameters only via [`Self::params`] in their `LoadNode` request).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params_files: Vec<String>,
    /// The ORDERED parameter-source list.
    /// [`Self::params`] and [`Self::params_files`] are the legacy split views,
    /// which cannot express ROS's ordering (they force files-then-inline).
    /// When this is non-empty it is AUTHORITATIVE: fold it in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_sources: Vec<ParamSource>,
    /// Raw command line for `<executable>` tags — a non-ROS process with no
    /// package/executable identity ([`Self::pkg`] is `None` and
    /// [`Self::exec`] carries no meaningful ament-resolvable value in this
    /// case). Mirrors `NodeRecord::cmd` verbatim: the first element is the
    /// full command string (split on whitespace at spawn time, the same way
    /// `node_cmdline.rs::from_raw_executable` does it today), remaining
    /// elements are extra args appended as-is. Empty for every regular ROS
    /// node/container (`pkg` is `Some`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw_cmd: Vec<String>,
    /// Composable-node LoadNode extra arguments (⊃ `use_intra_process_comms`),
    /// forwarded verbatim into the `LoadNode` service request's
    /// `extra_arguments`. The C++ container reads only
    /// `use_intra_process_comms` from it today (per project docs); the rest
    /// rides along for forward-compat. Composable nodes only — regular
    /// nodes/containers have no equivalent load-time argument channel
    /// (their extra CLI input is [`Self::args`]/[`Self::ros_args`]).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra_args: BTreeMap<String, String>,

    // -- Phase 46.3b — provenance the model previously inferred and lost,
    // needed for a faithful model-driven spawn (`.superpowers/sdd/
    // p46-w3b-review.md`). Additive: old models without these keys parse
    // unchanged (`node_name` defaults to `None`, `is_container` to
    // `false`). ----------------------------------------------------------
    /// The node name AS DECLARED in the launch file (`<node name="…">` /
    /// `Node(name=…)`), distinct from [`Self::exec`] — `None` when the
    /// launch declared no `name=` at all.
    ///
    /// Why this can't be recovered from the `structure.nodes` FQN key: the
    /// key is `name.or(exec_name)` (Phase 46.3a's GAP-1 fallback, so
    /// `name=None` nodes aren't dropped from the model), which erases
    /// whether the last FQN segment came from an explicit `name=` or the
    /// `exec_name` fallback. The Linux spawn path needs that distinction to
    /// decide whether to emit the `-r __node:=<name>` remap: forcing
    /// `__node` onto a `name=None` node silently renames it away from its
    /// own internally-hardcoded default name, breaking service/action
    /// discovery for e.g. LifecycleNodes whose lifecycle services register
    /// under the internal name — a regression a resolver once had to fix
    /// on its pre-model path ("Use None for the node name if it's not set
    /// instead of exec_name fallback"). With this field the
    /// model path reproduces that conditional exactly (emit `__node` iff
    /// `node_name.is_some()`). Containers and composable nodes always carry
    /// `Some` (their record types have a non-optional name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_name: Option<String>,
    /// Whether this instance is a `<node_container>` (as opposed to a plain
    /// `<node>` or a composable `<composable_node>`). The producer knows this
    /// unambiguously (it comes from the record's `dump.container[]` array); a
    /// consumer CANNOT reconstruct it from the other fields — `model_builder`
    /// emits containers with `plugin: None, container: None`, structurally
    /// identical to a plain node. The Linux spawn path keys a container
    /// package/executable override (a replacement container with
    /// `--isolated`/`--use_multi_threaded_executor`) off this bit; inferring it
    /// by reverse-resolving composable `container=` references (the pre-46.3b
    /// heuristic) could misclassify a regular node whose name coincides with a
    /// dangling/ambiguous composable target and then corrupt its executable.
    /// Plain nodes and composable nodes carry `false`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_container: bool,

    // -- Launch `<timer>` start delay. Additive: a model written before this
    // key existed parses with `start_delay_secs: None`, and a consumer that
    // ignores it spawns exactly as it did before — an embedded realizer is
    // such a consumer, since (per the Phase 46.1b note above) it has no argv/
    // process model to delay in the first place. -------------------------
    /// Delay in seconds before this instance is spawned for the FIRST time,
    /// from a launch `<timer period="…">` (`TimerAction`) enclosing it.
    /// Distinct from [`Self::respawn_delay`], which is the wait before a
    /// RE-spawn once the process has already run and exited: an instance may
    /// carry either, both, or neither.
    ///
    /// Nested timers accumulate — a `<timer period="3.0">` around a
    /// `<timer period="8.0">` gives `11.0`, matching ROS 2, where the inner
    /// timer starts counting only when the outer one fires.
    ///
    /// `None` = no enclosing timer, start immediately; the undelayed case is
    /// never written as `0.0`.
    ///
    /// Carried for composable nodes too, where the delayed event is the
    /// `LoadNode` request rather than a process spawn — the producer knows a
    /// timer enclosed one, and a consumer that cannot defer a load should say
    /// so rather than be unable to tell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_delay_secs: Option<f64>,
}

/// One `<remap from= to=/>` pair. Named struct (not a bare tuple) so the
/// YAML reads as `- from: … / to: …` — same "named struct over tuple"
/// precedent as [`ros_launch_manifest_sched::SegmentNode`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Remap {
    pub from: String,
    pub to: String,
}

/// One `<env name= value=/>` pair.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EnvVar {
    pub name: String,
    pub value: String,
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
///
/// `Str` is always a string, whatever its text looks like (schema 2). A list is
/// `StrList`, each element spelled as a YAML scalar that reads back as what it
/// was: an integer `5`, a double `1.5`, a bool `true`, a string `a` — and a
/// string that would read as something else single-quoted (`'5'`, `'true'`,
/// `''`), the convention a launch parser that types by YAML 1.1 shape uses for
/// a scalar. A consumer that needs the list as YAML writes `[` + the elements
/// joined by `, ` + `]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    StrList(Vec<String>),
}

impl ParamValue {
    /// A list parameter from YAML flow text (`[a, 5, '5']`), elements spelled
    /// as [`ParamValue`]'s doc describes; `None` when the text is not a flow
    /// sequence of scalars. For a producer lowering an inline list, and for
    /// the version-1 upgrade.
    pub fn list_from_flow(text: &str) -> Option<ParamValue> {
        let t = text.trim();
        if !(t.starts_with('[') && t.ends_with(']')) {
            return None;
        }
        let seq: Vec<serde_yaml_ng::Value> = serde_yaml_ng::from_str(t).ok()?;
        seq.iter()
            .map(list_element_text)
            .collect::<Option<Vec<_>>>()
            .map(ParamValue::StrList)
    }

    /// Render for a consumer that bakes parameters as STRINGS — e.g. an
    /// embedded realizer's compile-time codegen, where the value is re-typed
    /// at runtime by inference over this text.
    ///
    /// Single-sourced because the inference is unforgiving: `1.0f64.to_string()`
    /// is `"1"`, which re-types as an INTEGER and silently changes a launch
    /// double's type. `{:?}` keeps the `.0`. Every consumer that hand-rolled
    /// this match got at least one arm wrong.
    pub fn to_bake_string(&self) -> String {
        match self {
            ParamValue::Bool(b) => b.to_string(),
            ParamValue::Int(i) => i.to_string(),
            ParamValue::Float(f) => format!("{f:?}"),
            ParamValue::Str(s) => s.clone(),
            ParamValue::StrList(l) => l.join(","),
        }
    }
}

/// One parameter source, in ROS's ORDERED model.
///
/// `launch_ros` keeps a single list per node: `parse_nested_parameters` appends
/// one entry per `<param>` child in document order, and `execute` emits
/// `--params-file` / `-p` in that order — materializing an inline dict into a
/// temp params FILE first. Kind therefore carries NO precedence; position does,
/// and a file written after an inline value wins.
///
/// A consumer that cannot delegate to rcl (a compile-time bake) must
/// fold this list IN ORDER, applying [`Self::File`] entries with FILE semantics
/// (`ros__parameters` sections, `/**` and partial wildcards) rather than as a
/// flat key/value overlay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ParamSource {
    /// An inline `<param name= value=/>`, or a global param — globals occupy
    /// the HEAD of the list, mirroring ROS applying them before the node's own.
    Inline { name: String, value: ParamValue },
    /// A `<param from=/>` file: the resolved YAML CONTENT, same shape as the
    /// entries of [`NodeInstance::params_files`].
    File { content: String },
}

impl NodeInstance {
    /// Project `params_files` YAML into concrete parameter
    /// values, merged under the inline [`Self::params`].
    ///
    /// Embedded consumers have no launch runtime to pass `--params-file` to:
    /// they bake parameters into the image, so an upstream node that declares
    /// a parameter with NO default and receives its value from a
    /// `config/*.param.yaml` needs those values resolved at bake time. This is
    /// that projection — the same precedence a spawn would produce
    /// (param files in declaration order, each overriding the previous; inline
    /// `<param>` values win over all of them).
    ///
    /// Section matching follows ROS 2: a file maps NODE KEYS to a
    /// `ros__parameters` block. A key matches this node when it is the
    /// wildcard `/**`, the node's fully-qualified name, or its bare name
    /// (with or without a leading `/`). Nested maps under `ros__parameters`
    /// flatten with `.` (`qos.depth: 10` → `"qos.depth"`), matching rclcpp's
    /// nested-parameter naming. Sequences become [`ParamValue::StrList`];
    /// nulls and unmatched sections are skipped. A file that fails to parse
    /// is skipped (the resolver already validated it; a consumer must not
    /// hard-fail a bake on a YAML dialect it does not model).
    pub fn resolved_params(&self, fqn: &str) -> BTreeMap<String, ParamValue> {
        let bare = fqn.rsplit('/').next().unwrap_or(fqn);
        let mut out: BTreeMap<String, ParamValue> = BTreeMap::new();

        // phase-54 — when the ORDERED list is present it is authoritative: fold
        // in list order, so a file written AFTER an inline value wins (ROS's
        // rule; the legacy split below cannot express it).
        if !self.param_sources.is_empty() {
            for src in &self.param_sources {
                match src {
                    ParamSource::Inline { name, value } => {
                        out.insert(name.clone(), value.clone());
                    }
                    ParamSource::File { content } => {
                        merge_param_file(content, fqn, bare, &mut out);
                    }
                }
            }
            return out;
        }

        for raw in &self.params_files {
            merge_param_file(raw, fqn, bare, &mut out);
        }
        // Legacy split view: inline `<param>` values are highest precedence.
        for (k, v) in &self.params {
            out.insert(k.clone(), v.clone());
        }
        out
    }
}

/// The values ONE parameter file gives the node `fqn`, with the section
/// matching and precedence [`NodeInstance::resolved_params`] applies.
///
/// Public so a consumer can attribute a value to the source that set it --
/// the folded map `resolved_params` returns has lost that, and a message
/// about a bad value has to say where the value came from.
pub fn param_file_values(content: &str, fqn: &str) -> BTreeMap<String, ParamValue> {
    let bare = fqn.rsplit('/').next().unwrap_or(fqn);
    let mut out = BTreeMap::new();
    merge_param_file(content, fqn, bare, &mut out);
    out
}

/// Merge one param-file YAML's matching sections into `out`.
///
/// Section precedence inside a file is by SPECIFICITY, not by textual order:
/// rcl buckets a file's entries per node and lets a node-specific block
/// override what `/**` set, however the two are ordered in the YAML. So a file
/// that writes `/ctrl/planner:` above `/**:` still resolves `planner`'s own
/// value — ordering only decides between sources ([`ParamSource`]), never
/// between sections of one file.
fn merge_param_file(raw: &str, fqn: &str, bare: &str, out: &mut BTreeMap<String, ParamValue>) {
    let Ok(doc) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(raw) else {
        return;
    };
    let serde_yaml_ng::Value::Mapping(sections) = doc else {
        return;
    };
    let mut matched: Vec<(u8, &serde_yaml_ng::Value)> = Vec::new();
    for (key, body) in &sections {
        let Some(k) = key.as_str() else { continue };
        if !node_key_matches(k, fqn, bare) {
            continue;
        }
        let Some(params) = body.get("ros__parameters") else {
            continue;
        };
        matched.push((key_specificity(k), params));
    }
    // Stable: equally-specific sections keep file order, so two `/**` blocks
    // (or two spellings of the same node) still resolve last-wins.
    matched.sort_by_key(|(rank, _)| *rank);
    for (_, params) in matched {
        flatten_params("", params, out);
    }
}

/// How specific a param-file node key is: pure wildcard < partial wildcard <
/// literal. Only the ORDER matters, not the numbers.
fn key_specificity(key: &str) -> u8 {
    match key.trim().trim_start_matches('/') {
        "**" | "*" => 0,
        k if k.contains('*') => 1,
        _ => 2,
    }
}

/// Does a param-file node key select this node?
///
/// Mirrors `rcl_yaml_param_parser`'s matching so a bake-time projection sees
/// the same sections a spawned node would: an exact fully-qualified name, or a
/// pattern using `**` (matches any number of namespace segments, including
/// none) and `*` (exactly one segment) — e.g. `/**`, `/sensing/**`,
/// `/*/planner`, `/foo/*/bar`. A bare node name (no leading `/`) is also
/// accepted as the convenience form launch files commonly use for
/// root-namespace nodes.
fn node_key_matches(key: &str, fqn: &str, bare: &str) -> bool {
    if key == fqn {
        return true;
    }
    let segs: Vec<&str> = key.trim_start_matches('/').split('/').collect();
    // Bare-name convenience: a single literal segment naming the node.
    if segs.len() == 1 && !segs[0].contains('*') {
        return segs[0] == bare;
    }
    let target: Vec<&str> = fqn.trim_start_matches('/').split('/').collect();
    glob_match(&segs, &target)
}

/// Segment-wise glob: `**` consumes any number of segments, `*` exactly one.
fn glob_match(pat: &[&str], target: &[&str]) -> bool {
    match pat.split_first() {
        None => target.is_empty(),
        Some((&"**", rest)) => {
            // `**` may consume 0..=target.len() segments.
            (0..=target.len()).any(|skip| glob_match(rest, &target[skip..]))
        }
        Some((&head, rest)) => match target.split_first() {
            Some((&t, t_rest)) if head == "*" || head == t => glob_match(rest, t_rest),
            _ => false,
        },
    }
}

/// Flatten a `ros__parameters` subtree into dotted keys.
fn flatten_params(
    prefix: &str,
    node: &serde_yaml_ng::Value,
    out: &mut BTreeMap<String, ParamValue>,
) {
    let serde_yaml_ng::Value::Mapping(map) = node else {
        return;
    };
    for (k, v) in map {
        let Some(name) = k.as_str() else { continue };
        let full = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}.{name}")
        };
        match v {
            serde_yaml_ng::Value::Mapping(_) => flatten_params(&full, v, out),
            serde_yaml_ng::Value::Bool(b) => {
                out.insert(full, ParamValue::Bool(*b));
            }
            serde_yaml_ng::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    out.insert(full, ParamValue::Int(i));
                } else if let Some(f) = n.as_f64() {
                    out.insert(full, ParamValue::Float(f));
                }
            }
            serde_yaml_ng::Value::String(st) => {
                out.insert(full, ParamValue::Str(st.clone()));
            }
            serde_yaml_ng::Value::Sequence(seq) => {
                // Same element spelling as an inline list (`ParamValue`'s
                // doc), so a string element `'5'` stays a string.
                let items: Vec<String> = seq
                    .iter()
                    .map(|e| list_element_text(e).unwrap_or_default())
                    .collect();
                out.insert(full, ParamValue::StrList(items));
            }
            _ => {}
        }
    }
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
    /// Service client facts (since v0.1.69), keyed like the other endpoint
    /// maps. A client with nothing stated has no entry.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub cli_endpoints: BTreeMap<String, CliContract>,
    /// Action client facts (since v0.1.69), keyed by the client endpoint ref
    /// `actions.<a>.client` lists (`"<node FQN>/<endpoint>"`). The
    /// contract's `actions.<a>.call` applies to every client of the action.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub action_cli_endpoints: BTreeMap<String, CliContract>,
    /// Node processing paths (take → publish).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub node_paths: BTreeMap<String, PathContract>,
    /// Scope end-to-end paths (entry topic → exit topic within the subtree).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub scope_paths: BTreeMap<String, PathContract>,
    /// Which of a node's paths may not run concurrently (phase 67), keyed by
    /// node FQN.
    ///
    /// **Presence in this map is the declaration.** An absent entry means
    /// every path of that node serialises — the safe answer, and what both
    /// realizations already do (`rclcpp`'s implicit callback group is
    /// `MutuallyExclusive`, and embedded executors commonly default the
    /// same way). An entry
    /// with an EMPTY `exclusive` is the opposite claim: every path may run
    /// concurrently. Flattening the two is the mistake this shape exists to
    /// prevent.
    ///
    /// Carried on the model because the model is the only artifact a second
    /// toolchain reads. A realizer may INFER callback groups from causal
    /// coupling and default an uncoupled callback to `Reentrant` — the
    /// OPPOSITE of the contract's default. A declared
    /// relation is the fact that settles which is right for a given system,
    /// and until it crossed this boundary neither side could see the other's
    /// answer.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub node_concurrency: BTreeMap<String, ros_launch_manifest_sched::ConcurrencyContract>,
    /// The parameters each node declares (contract `nodes.<n>.params`),
    /// keyed by node FQN, then by parameter name. Names and ROS 2 types
    /// only: a capacity for a string or an array is a board fact, and the
    /// model carries none.
    ///
    /// **Presence in this map is the declaration.** A node with no entry
    /// declared nothing, and neither its launch values nor its code are
    /// checked against a declaration -- today's behaviour. Carried so a
    /// second toolchain can size its parameter store from what is declared
    /// instead of from built-in worst cases.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub node_params: BTreeMap<String, BTreeMap<String, ParamContract>>,
    /// Per-topic channel contracts (rate, transport, drops, QoS).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub topics: BTreeMap<String, TopicContract>,
    /// Externally-provided topics: FQN → which side is external
    /// (`pub` | `sub` | `both`). Dangling-entity and runtime graph checks
    /// skip the external side.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub externals: BTreeMap<String, ExternalSide>,
    /// Hazards (phase 71), keyed by `"<scope id>/<name>"`, with guard topics
    /// resolved to FQNs. Read by `measure` to attribute observed detection
    /// and reaction times to the hazard they cover.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub hazards: BTreeMap<String, HazardContract>,
    /// Named guard groups (phase 75), keyed `"<scope id>/<name>"` with
    /// members resolved to topic FQNs.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub functions: BTreeMap<String, FunctionContract>,
    /// Operational modes (phase 75), keyed the same way. Read by the
    /// runtime observer to derive availability, and by a second toolchain
    /// that schedules per mode.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub modes: BTreeMap<String, ModeContract>,
    /// The severity scale `hazards.<h>.severity` is drawn from, ascending
    /// (design issue #52). Empty means the ISO 26262 default
    /// `[QM, ASIL_A, ASIL_B, ASIL_C, ASIL_D]`; the first entry is "no
    /// safety requirement" and derives no criticality. Carried so a
    /// consumer can explain a `node_criticality` entry from the hazard
    /// that produced it without re-reading the manifest.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub severity_levels: Vec<String>,
    /// The EFFECTIVE criticality of each node after the phase-72
    /// derivation, keyed by node FQN (design issue #52): a hazard whose
    /// reaction route reaches the node decides first, the authored label
    /// (`NodeInstance::criticality`) where no hazard reaches. This is the
    /// value the mapper ranks by; the label stays on the node for
    /// dashboards. A node absent from this map has no criticality fact.
    ///
    /// Carried in `sched`'s own [`ros_launch_manifest_sched::Criticality`]
    /// for the same reason `PathContract::miss` is: a second spelling on
    /// either side of the boundary is how two toolchains come to rank the
    /// same system from different facts.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub node_criticality: BTreeMap<String, ros_launch_manifest_sched::Criticality>,
}

/// One mode after merge (phase 75).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModeContract {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Keys into `functions` (or bare topic FQNs).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
    /// Keys into `modes`, in fallback order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback: Vec<String>,
    /// Key into `scope_paths`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reaction: Option<String>,
    /// `(target, value)` pairs: a requirement named by its contract path and
    /// the value it takes in this mode.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<ModeOverrideContract>,
    /// A transitional rung's window (v0.1.46): the least time the system
    /// spends here, while the rung stays available, before taking the next
    /// rung (v0.1.47 corrects "the most": the window is a guarantee to
    /// whoever the rung waits for, and the rung below's route, charged from
    /// the deadline, bounds how late it may start).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowContract>,
    /// The success exit of a windowed rung (v0.1.46).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<ExitContract>,
    /// Where each number of this entry read from the launch tree came from
    /// (play_launch phase 94), keyed by the model field it fills
    /// (`max_age_ms`, `qos.lease_duration_ms`, `rate_hz`). The number is in
    /// its field as any written one; absent for a written number.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sources: BTreeMap<String, ValueSource>,
}

/// `modes.<m>.window` after merge (v0.1.46).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WindowContract {
    pub duration_ms: f64,
    /// `<node FQN>.<parameter>`: the parameter that enforces the window,
    /// read in seconds. Absent means the window is unbound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
}

/// `modes.<m>.exit` after merge (v0.1.46).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExitContract {
    /// Key into `functions`: the contract's `exit.holds` (spelled `exit.on`
    /// before v0.1.59; the model's field keeps the old name).
    pub on: String,
    /// Key into `modes`.
    pub to: String,
}

/// A named function after merge (phase 75): a guard group, plus (v0.1.46)
/// the predicate that loses it by value. Read by the runtime observer and by
/// a contract probe that evaluates the predicate on the topic's samples.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FunctionContract {
    pub members: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all_of: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<PredicateContract>,
}

/// A `when:` predicate after merge (v0.1.46): one field, one operator, one
/// constant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PredicateContract {
    pub field: String,
    pub op: CompareOp,
    pub value: PredicateValueContract,
}

/// The operator of a predicate. Mirrors the contract crate's closed set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompareOp {
    Equals,
    NotEquals,
    Lt,
    Le,
    Gt,
    Ge,
}

/// The constant of a predicate. A message constant name is carried as
/// `{ constant: MANUAL }` so a reader never mistakes it for a string
/// literal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PredicateValueContract {
    Bool(bool),
    Int(i64),
    Float(f64),
    Constant { constant: String },
    Str(String),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModeOverrideContract {
    /// The requirement, as the contract wrote it: a path in the declaring
    /// scope's own vocabulary (`paths.<name>.max_latency`,
    /// `nodes.<n>.sub.<e>.min_rate_hz`, `topics.<t>.rate_hz`).
    pub target: String,
    pub value: String,
    /// v0.1.63 -- the same requirement in the MODEL's keys, resolved by the
    /// producer that knows the declaring scope's namespace: which contract
    /// table, the key in it, and the field. A consumer that applies a mode to
    /// the model reads this rather than re-resolving `target` against scope
    /// names it does not have. `None` when the producer did not resolve it --
    /// a model written before v0.1.63, or a target naming no declared
    /// requirement (which the producer reports).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved: Option<ResolvedOverride>,
}

/// v0.1.63 -- a mode override's target in the model's own keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedOverride {
    /// The `Contracts` table the requirement lives in.
    pub table: OverrideTable,
    /// Its key there: a node path's or an endpoint's ref
    /// (`<node FQN>/<name>`), a scope path's `<scope id>/<name>`, or a topic
    /// FQN.
    pub key: String,
    /// The requirement's field, as the contract spells it (`max_latency`,
    /// `min_rate_hz`, `max_age`, `rate_hz`, ...). Its value is the
    /// override's `value`, typed against that field.
    pub field: String,
}

/// The `Contracts` table a [`ResolvedOverride`] keys into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverrideTable {
    /// `contracts.node_paths`.
    NodePaths,
    /// `contracts.scope_paths`.
    ScopePaths,
    /// `contracts.pub_endpoints`.
    PubEndpoints,
    /// `contracts.sub_endpoints`.
    SubEndpoints,
    /// `contracts.topics`.
    Topics,
}

/// A hazard after merge (phase 71).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HazardContract {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    /// Each entry is one fault: one topic, or a redundant set that faults
    /// only when every member does.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guards: Vec<GuardContract>,
    /// The fault classes the guards report, as a set (phase 82). Empty
    /// means the contract omitted `on:`, read as every class. On the wire a
    /// set of one is a bare scalar -- exactly what a model written before
    /// v0.1.43 carried -- and a larger set is a list; both read back.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        serialize_with = "fault_kinds::serialize",
        deserialize_with = "fault_kinds::deserialize"
    )]
    pub on: Vec<FaultKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ftti_ms: Option<f64>,
    /// Where the hazard's reaction starts: a key into `modes` when the
    /// hazard enters a mode (the contract's `enters:`, or the deprecated
    /// `reaction: <mode>`), else a key into `scope_paths` (the contract's
    /// `reaction: <path>`, one rung). The model keeps one field for both, so
    /// a reader resolves it the way the checker does: a mode first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reaction: Option<String>,
    /// The value that is the fault (v0.1.46).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<PredicateContract>,
    /// Where `ftti_ms` came from when an extension derived it (phase 91
    /// E10): the number is read as any written one; this says why it is
    /// that number. Absent for a written interval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ftti_derived: Option<DerivedSource>,
    /// The hazard's own detection bound, ms (phase 91 E1: a physical
    /// detection time, written or derived), charged before the detectors'.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detection_ms: Option<f64>,
    /// Where `detection_ms` came from when an extension derived it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detection_derived: Option<DerivedSource>,
    /// Where each number of this entry read from the launch tree came from
    /// (play_launch phase 94), keyed by the model field it fills
    /// (`max_age_ms`, `qos.lease_duration_ms`, `rate_hz`). The number is in
    /// its field as any written one; absent for a written number.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sources: BTreeMap<String, ValueSource>,
}

/// Where a number read from the launch tree came from (play_launch phase
/// 94): the value as the contract wrote it, and every read behind it. The
/// number itself is in its field, read as any written one; this record is
/// for a reviewer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ValueSource {
    /// The value as written (`$(param update_rate) Hz`).
    pub written: String,
    /// Each reference it read, in order (a `$(let)` first, then what that
    /// entry read).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reads: Vec<ValueRead>,
}

/// One read behind a [`ValueSource`], tagged by `kind:` (a YAML-tag-free
/// form every model reader parses).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ValueRead {
    /// `$(param …)`: the node's effective parameter, and where its value was
    /// set (a launch file, a parameter file, `default:` in the contract).
    Param {
        node: String,
        name: String,
        value: String,
        at: String,
    },
    /// `$(var …)`: a launch configuration, and where it was set.
    Var {
        name: String,
        value: String,
        at: String,
    },
    /// `$(let …)`: an entry of the contract's `let:`.
    Let { name: String, value: String },
}

/// The source of a bound an extension derived (phase 91 E10). The model
/// carries every derived bound as a plain number beside its written
/// siblings, so a consumer (nano-ros's image build) reads it unchanged and
/// never evaluates an extension; this record is for a reviewer, and for a
/// lock: the extension, its version and the sha256 of its canonical form,
/// and the inputs it read as the contract wrote them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DerivedSource {
    /// The extension (`vehicle.braking`).
    pub by: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// For a settle, which differs per hazard: the hazard whose values give
    /// the number, the largest over the hazards whose reaction ends at the
    /// safe state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hazard: Option<String>,
    /// The inputs read, name to value as written (`hazard.entry_speed: 30
    /// km/h`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GuardContract {
    pub members: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all_of: bool,
}

/// What a detector detects (phase 71). Mirrors the contract crate's closed
/// set; carried here so a second toolchain can read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultKind {
    Omission,
    Late,
    Loss,
    Reported,
    /// A supervised budget overran (phase 93): the verdict of a
    /// `supervise:`'s monitor, never a subscriber's `on:`.
    Overrun,
}

/// A supervised budget lowered to a route monitor (phase 93): the budget
/// the monitor arms a deadline with, the reaction a miss starts and what
/// checks it. On a node path whose `max_latency` states `supervise:` and is
/// input-triggered, so the route monitor stamps the take of its first input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuperviseContract {
    /// The supervised budget, ms: due = take + this.
    pub route_deadline_ms: f64,
    /// The path a miss starts, by its key in this model: a node path's key
    /// in `contracts.node_paths`, or a scope path's in
    /// `contracts.scope_paths` (an `enters:` mode lowers to its reaction
    /// scope path).
    pub reaction: String,
    /// What checks the budget.
    pub mechanism: SuperviseMechanism,
}

/// What checks a supervised budget (phase 93).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SuperviseMechanism {
    /// The executor's spin loop: slowness, not a stuck callback.
    Executor,
    /// An independent timer and thread: both.
    Timer,
}

/// `HazardContract.on` on the wire: one kind as a scalar (the pre-v0.1.43
/// shape, so an older reader still reads every single-kind model), several
/// as a list.
mod fault_kinds {
    use super::FaultKind;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(kinds: &[FaultKind], s: S) -> Result<S::Ok, S::Error> {
        match kinds {
            [one] => one.serialize(s),
            many => many.serialize(s),
        }
    }

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(FaultKind),
        Many(Vec<FaultKind>),
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<FaultKind>, D::Error> {
        Ok(match OneOrMany::deserialize(d)? {
            OneOrMany::One(k) => vec![k],
            OneOrMany::Many(v) => v,
        })
    }
}

/// Where the runtime observer reads a violation (phase 71).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectMechanism {
    #[default]
    Qos,
    Diagnostics,
    Application,
}

/// The reaction a subscriber owes on a violated assumption (phase 71). Read
/// by the runtime observer to know which event to watch and which path's
/// output marks the reaction.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OnViolationContract {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on: Vec<FaultKind>,
    /// Key into `node_paths`.
    pub reaction: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub within_ms: Option<f64>,
    #[serde(default)]
    pub mechanism: DetectMechanism,
}

/// What a reaction path produces (phase 71).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SafeStateContract {
    /// Endpoint ref the safe state is commanded on.
    pub emits: String,
    /// The settle, ms, response included: a written duration, or (phase
    /// 91 E10) the one an extension derives -- a braking profile among them
    /// -- at the largest of the hazards whose reaction ends here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settle_ms: Option<f64>,
    /// Where `settle_ms` came from when an extension derived it. v0.1.46 to
    /// v0.1.71 carried a braking profile's parameter names here
    /// (`settle_profile`) and no number; a model with that key still reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settle_derived: Option<DerivedSource>,
    /// The supervision of the whole settle (v0.1.62), when the contract
    /// states one and the producer lowered it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supervise: Option<SettleSuperviseContract>,
}

/// A supervised settle lowered to a standstill deadline (v0.1.62). The
/// realizer arms due = the safe state's command + the settle + `observe_ms`
/// and, when the vehicle is not at standstill on `observes` by then, starts
/// `reaction`. The settle is `settle_ms`, response included, written or
/// derived (phase 91 E10; before it a derived settle was absent from the
/// model and the realizer computed it from the profile).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettleSuperviseContract {
    /// The settle's response, ms: already inside `settle_ms`, stated apart
    /// so a reader can tell the vehicle's share from the plant's.
    #[serde(default)]
    pub response_ms: f64,
    /// How late standstill is seen, ms: the observed input's period + its
    /// `max_transport`.
    pub observe_ms: f64,
    /// The input standstill is seen on, by its key in
    /// `contracts.sub_endpoints`.
    pub observes: String,
    /// The path a miss starts, by its key in this model (as
    /// [`SuperviseContract::reaction`]).
    pub reaction: String,
    /// What checks the deadline.
    pub mechanism: SuperviseMechanism,
}

impl Contracts {
    pub fn is_empty(&self) -> bool {
        self.pub_endpoints.is_empty()
            && self.sub_endpoints.is_empty()
            && self.srv_endpoints.is_empty()
            && self.cli_endpoints.is_empty()
            && self.action_cli_endpoints.is_empty()
            && self.node_paths.is_empty()
            && self.scope_paths.is_empty()
            && self.topics.is_empty()
            && self.externals.is_empty()
            && self.hazards.is_empty()
            && self.functions.is_empty()
            && self.modes.is_empty()
            && self.node_params.is_empty()
            && self.node_concurrency.is_empty()
            && self.severity_levels.is_empty()
            && self.node_criticality.is_empty()
    }
}

/// One declared parameter after merge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParamContract {
    #[serde(rename = "type")]
    pub ty: ParamType,
}

/// The ROS 2 parameter types. Mirrors the contract crate's closed set;
/// carried here so a second toolchain can read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamType {
    Bool,
    Integer,
    Double,
    String,
    ByteArray,
    BoolArray,
    IntegerArray,
    DoubleArray,
    StringArray,
}

impl ParamType {
    /// The contract spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            ParamType::Bool => "bool",
            ParamType::Integer => "integer",
            ParamType::Double => "double",
            ParamType::String => "string",
            ParamType::ByteArray => "byte_array",
            ParamType::BoolArray => "bool_array",
            ParamType::IntegerArray => "integer_array",
            ParamType::DoubleArray => "double_array",
            ParamType::StringArray => "string_array",
        }
    }

    pub fn is_array(self) -> bool {
        matches!(
            self,
            ParamType::ByteArray
                | ParamType::BoolArray
                | ParamType::IntegerArray
                | ParamType::DoubleArray
                | ParamType::StringArray
        )
    }

    /// Does a launch value parse as this type?
    ///
    /// As strict as `rclcpp` is when it applies a parameter override to a
    /// typed declaration: an integer is not a double (`5` must be written
    /// `5.0`), and `10` is not a string. Array elements reach the model as
    /// text ([`ParamValue::StrList`]), so each is re-read here; an inline
    /// launch value written as a flow sequence (`"[1, 2]"`) is read the way
    /// `launch_ros` reads it, as YAML. An empty list is every array type.
    ///
    /// `Err` says what was found, for a message that also names the node,
    /// the parameter and the source.
    pub fn check(self, value: &ParamValue) -> Result<(), String> {
        let found = || describe_value(value);
        let ok = match (self, value) {
            (ParamType::Bool, ParamValue::Bool(_))
            | (ParamType::Integer, ParamValue::Int(_))
            | (ParamType::Double, ParamValue::Float(_))
            | (ParamType::String, ParamValue::Str(_)) => true,
            (ParamType::Double, ParamValue::Int(i)) => {
                return Err(format!(
                    "{}; a double is written with a decimal point ({i}.0)",
                    found()
                ));
            }
            (t, ParamValue::StrList(items)) if t.is_array() => {
                items.iter().all(|e| t.accepts_element(e))
            }
            (t, ParamValue::Str(s)) if t.is_array() => match flow_sequence(s) {
                Some(items) => items.iter().all(|e| t.accepts_element(e)),
                None => false,
            },
            _ => false,
        };
        if ok { Ok(()) } else { Err(found()) }
    }

    fn accepts_element(self, e: &str) -> bool {
        match self {
            ParamType::ByteArray => e.parse::<u8>().is_ok(),
            ParamType::BoolArray => e == "true" || e == "false",
            ParamType::IntegerArray => e.parse::<i64>().is_ok(),
            // An integer element is not a double, for the same reason a
            // scalar is not.
            ParamType::DoubleArray => e.parse::<i64>().is_err() && e.parse::<f64>().is_ok(),
            ParamType::StringArray => true,
            _ => false,
        }
    }
}

impl std::fmt::Display for ParamType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

fn describe_value(v: &ParamValue) -> String {
    match v {
        ParamValue::Bool(b) => format!("got a bool ({b})"),
        ParamValue::Int(i) => format!("got an integer ({i})"),
        ParamValue::Float(f) => format!("got a double ({f:?})"),
        ParamValue::Str(s) => format!("got a string ({s:?})"),
        ParamValue::StrList(l) => format!("got a list ([{}])", l.join(", ")),
    }
}

/// `"[1, 2]"` as `launch_ros` would read it; `None` when it is not one.
fn flow_sequence(s: &str) -> Option<Vec<String>> {
    let t = s.trim();
    if !(t.starts_with('[') && t.ends_with(']')) {
        return None;
    }
    let seq: Vec<serde_yaml_ng::Value> = serde_yaml_ng::from_str(t).ok()?;
    seq.iter().map(yaml_scalar_text).collect()
}

/// One list element as a YAML scalar that reads back as what it was
/// (`ParamValue`'s doc): a string that would read as another type is
/// single-quoted, with any `'` inside doubled.
///
/// The element is written inside a flow sequence, where more than the
/// scalar rule breaks it: `a: b` reads as a mapping, `a, b` as two elements,
/// `a #b` loses its comment. So a string that does not read back as itself
/// between `[` and `]` is quoted too.
fn list_element_text(e: &serde_yaml_ng::Value) -> Option<String> {
    match e {
        serde_yaml_ng::Value::String(s) => {
            Some(if string_would_retype(s) || !reads_back_in_flow(s) {
                format!("'{}'", s.replace('\'', "''"))
            } else {
                s.clone()
            })
        }
        serde_yaml_ng::Value::Bool(b) => Some(b.to_string()),
        serde_yaml_ng::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Whether a string's text would be read as something other than this
/// string by a consumer that types by shape: the YAML 1.1 rule a launch
/// parser that types by shape applies to a scalar, kept equal to it.
fn string_would_retype(s: &str) -> bool {
    s.is_empty()
        || matches!(s, "true" | "True" | "false" | "False")
        || s.parse::<f64>().is_ok()
        || s.starts_with(['\'', '['])
}

/// Whether `s`, written bare as the only element of a flow sequence, reads
/// back as exactly the string `s`.
fn reads_back_in_flow(s: &str) -> bool {
    serde_yaml_ng::from_str::<Vec<serde_yaml_ng::Value>>(&format!("[{s}]"))
        .is_ok_and(|v| matches!(v.as_slice(), [serde_yaml_ng::Value::String(r)] if r == s))
}

fn yaml_scalar_text(e: &serde_yaml_ng::Value) -> Option<String> {
    match e {
        serde_yaml_ng::Value::String(s) => Some(s.clone()),
        serde_yaml_ng::Value::Bool(b) => Some(b.to_string()),
        serde_yaml_ng::Value::Number(n) => Some(n.to_string()),
        _ => None,
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
    /// R1-M5 — per-endpoint QoS (overrides the topic-level profile for
    /// this endpoint; manifest-side per-endpoint QoS + the retired 211.H
    /// launch-param overlay both land here).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qos: Option<Qos>,
    /// The publisher publishes when asked and promises NO rate (contract
    /// `on_demand: true`, v0.1.49). What a consumer must
    /// derive from it: NO rate monitor for this endpoint (and none for a
    /// subscriber of its topic on its account), and no period for the
    /// topic. `min_rate_hz` is then absent, and an absent `min_rate_hz`
    /// without this flag still means "not stated", not "no rate". The
    /// Autoware Safety Island's board image raised two
    /// `rate-hierarchy-runtime` violations at start-up on exactly such
    /// endpoints when its contract had to give them a 10 Hz floor.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub on_demand: bool,
    /// Where each number of this entry read from the launch tree came from
    /// (play_launch phase 94), keyed by the model field it fills
    /// (`max_age_ms`, `qos.lease_duration_ms`, `rate_hz`). The number is in
    /// its field as any written one; absent for a written number.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sources: BTreeMap<String, ValueSource>,
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
    /// The reaction owed on a violated assumption (phase 71).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_violation: Option<OnViolationContract>,
    /// Buffering discipline of a `state: true` endpoint (Vocabulary v2,
    /// design issue #52): `latest` (read-latest; staleness is the failure
    /// mode, and the default when absent) or `queue` (a bounded queue the
    /// consuming timer callback drains; backlog is the failure mode).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buffer: Option<BufferContract>,
    /// Worst-case transport into THIS subscriber, overriding
    /// [`TopicContract::max_transport_ms`] for the edges that end here
    /// (design issue #55).
    ///
    /// One ROS topic does not have one transport cost: the same message
    /// reaching a subscriber in the publisher's own process is a pointer
    /// handoff, reaching another process on the host is an RMW hop, and
    /// reaching another machine is the network. Placement decides which,
    /// and placement is per subscriber, so the override belongs here and
    /// the topic value is the default for subscribers that do not state
    /// one.
    ///
    /// Design issue #55 records what the number MEANS in each case, and
    /// the three cases do not agree: the first two are managed by the
    /// toolchain — it places the processes and sets their priorities,
    /// cgroups and container membership — so a bound there is a promise it
    /// can be held to, while a cross-host bound is an assumption about an
    /// environment nothing here manages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_transport_ms: Option<f64>,
    /// What this subscriber is made to ABSORB (since v0.1.64; nano-ros
    /// RFC-0074, issue 0760): a token bucket. Both numbers are always present
    /// when the contract states one -- the grammar requires both -- so a
    /// consumer never has to invent a burst.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ingress: Option<IngressContract>,
    /// The least time between two arrivals, ms (contract
    /// `sub.<e>.min_interarrival`, since v0.1.68; nano-ros RFC-0108 D1): a
    /// sporadic task's T_min. Absent means not stated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_interarrival_ms: Option<f64>,
    /// The execution budget Q per [`Self::budget_period_ms`], ms (contract
    /// `budget`, since v0.1.68): the callback is served, analysed as a
    /// periodic task (Q, P). Present exactly when `budget_period_ms` is: the
    /// grammar refuses one without the other, and Q > P.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_ms: Option<f64>,
    /// The server period P of [`Self::budget_ms`], ms (since v0.1.68).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_period_ms: Option<f64>,
    /// Where each number of this entry read from the launch tree came from
    /// (play_launch phase 94), keyed by the model field it fills
    /// (`max_age_ms`, `qos.lease_duration_ms`, `rate_hz`). The number is in
    /// its field as any written one; absent for a written number.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sources: BTreeMap<String, ValueSource>,
}

/// A subscriber's ingress bound (since v0.1.64). See [`SubContract::ingress`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct IngressContract {
    /// Sustained messages per second. Finite and > 0.
    pub rate_hz: f64,
    /// Messages that may arrive back to back before pacing applies. >= 1.
    pub burst: u32,
}

/// How a `state: true` subscriber holds data between takes (design issue
/// #52). Mirrors the contract crate's closed set; carried here so a second
/// toolchain can size the queue it allocates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BufferContract {
    /// Read-latest: one slot, overwritten on every arrival.
    #[default]
    Latest,
    /// Bounded queue, drained batch-wise by the consuming callback.
    Queue,
}

/// Service server guarantee.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SrvContract {
    /// Max request-to-response time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_response_ms: Option<f64>,
    /// The least time between two requests, ms (contract
    /// `srv.<e>.min_interarrival`, since v0.1.68). See
    /// [`SubContract::min_interarrival_ms`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_interarrival_ms: Option<f64>,
    /// The execution budget per `budget_period_ms`, ms (since v0.1.68). See
    /// [`SubContract::budget_ms`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_ms: Option<f64>,
    /// The server period of `budget_ms`, ms (since v0.1.68).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_period_ms: Option<f64>,
    /// Where each number of this entry read from the launch tree came from
    /// (play_launch phase 94), keyed by the model field it fills
    /// (`max_age_ms`, `qos.lease_duration_ms`, `rate_hz`). The number is in
    /// its field as any written one; absent for a written number.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sources: BTreeMap<String, ValueSource>,
}

/// A client's facts (since v0.1.69): a service client (`cli.<e>`) or an
/// action client (`actions.<a>.client`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CliContract {
    /// How the code waits for the response (contract `call`). Absent means
    /// not stated, which is not the same as `async`: a consumer that refuses
    /// a blocking call to a level that cannot preempt the client checks a
    /// stated call and reports an unstated one (nano-ros RFC-0108 D8.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call: Option<CallMode>,
}

/// How a client waits for its response (since v0.1.69).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CallMode {
    /// The callback that sends the request waits for the response.
    Blocking,
    /// The request is sent and a later callback takes the response.
    Async,
}

/// A causal path with a latency budget. Node paths: input/output are
/// endpoint refs and the budget is pure processing (take → publish). Scope
/// paths: input/output are topic FQNs and the budget is E2E across the
/// scope's subtree (includes internal transport); only scope paths carry
/// drop budgets.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PathContract {
    /// The endpoints this path consumes: the `Input` trigger's sources on a
    /// node path, the entry topics on a scope path. Empty says NOTHING
    /// about what fires the path: a timer, a `once` loader and an
    /// unclassified path all have no inputs, and only [`Self::trigger`]
    /// tells them apart. ("Empty = periodic" was the legacy convention
    /// Vocabulary v2 retired; see design issue #52.)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input: Vec<String>,
    pub output: Vec<String>,
    /// The trigger in effect for this path, as the checker resolved it
    /// (`PathDecl::effective_trigger()`: an explicit `trigger:` wins, a
    /// legacy non-empty `input:` derives `Input`, anything else is
    /// `Unclassified`). Node paths only; scope paths carry `None`.
    ///
    /// In `sched`'s own [`ros_launch_manifest_sched::EffectiveTrigger`],
    /// adjacently tagged (`kind: timer`, `value: {rate_hz: 10.0}`), for the
    /// reason [`Self::miss`] gives. The timer's rate lives HERE and nowhere
    /// else: a consumer that recovers it from a publisher's `min_rate_hz`
    /// is guessing (design issue #52, row 1).
    ///
    /// `None` on the wire is a model written before this field existed. A
    /// consumer treats it as `Unclassified` and says so
    /// ([`Self::effective_trigger`]); it never assumes a timer, so that a
    /// stale model ranks nothing loudly rather than something wrongly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<ros_launch_manifest_sched::EffectiveTrigger>,
    /// The release jitter of a timer trigger, ms (contract `trigger: {
    /// timer: { rate_hz, jitter } }`, v0.1.49): how
    /// late a tick may be released, so that a message the path reads on its
    /// tick waits up to `period + jitter`. Beside [`Self::trigger`] rather
    /// than inside its `Timer` variant, because that enum is `sched`'s and
    /// every consumer constructs it. `None` for any other trigger, and for a
    /// timer that states none (ticks on time). What reads it: a resolver's
    /// reaction walk and `window-expiry`; the chain derivation's sampling
    /// cost is still one period.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timer_jitter_ms: Option<f64>,
    /// Fan-in synchronisation policy of an `Input` path with two or more
    /// sources (Vocabulary v2; design issue #52). Absent when the path
    /// declared none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync: Option<SyncContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_latency_ms: Option<f64>,
    /// Best-case latency for this path (phase 67 `min_latency`), the lower
    /// bound that makes `max_jitter_ms` falsifiable. Usually measured
    /// rather than authored; carried when the contract declares it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_latency_ms: Option<f64>,
    /// Max `header.stamp` spread between a fan-in path's inputs that the
    /// callback still treats as one set.
    ///
    /// `correlation` used to sit beside this (phase 70 removed it): declared,
    /// lowered to an enum here, and branched on by nothing on either side of
    /// the toolchain. Old models still carrying `correlation:` load — the
    /// golden fixture keeps one on disk to prove it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance_ms: Option<f64>,
    /// E2E drop budget (scope paths only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drop: Option<DropContract>,
    /// Tolerable spread in this path's contribution to end-to-end latency
    /// (phase 67). A *spread*, not a second budget: control stability depends
    /// on how much latency VARIES, not only on its maximum.
    ///
    /// This is the field the endpoint-level `jitter:` was retired in favour
    /// of, and it is carried HERE — on the model — because the model is the
    /// only thing a second toolchain reads. Phase 67 added it to the contract
    /// and to the sched crate's `MapperPath`, but not to this struct, so a
    /// realizer that builds its `MapperPath` from the model rather than from
    /// the manifest could not see it at all: not "did not use it", could
    /// not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_jitter_ms: Option<f64>,
    /// What a missed deadline means for this path (phase 67): how many are
    /// tolerated, and what to do about one.
    ///
    /// Deliberately `sched`'s own [`MapperMiss`] rather than a model-local
    /// mirror of it. A second spelling of the same concept on either side of
    /// this boundary is how two toolchains start scheduling the same system
    /// from different information — the seam this field exists to close.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub miss: Option<ros_launch_manifest_sched::MapperMiss>,
    /// What this path produces when it is a reaction (phase 71).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safe_state: Option<SafeStateContract>,
    /// The supervision of `max_latency_ms` (phase 93), when the contract
    /// states one and the producer lowered it to a route monitor. Node
    /// paths only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supervise: Option<SuperviseContract>,
    /// Where each number of this entry read from the launch tree came from
    /// (play_launch phase 94), keyed by the model field it fills
    /// (`max_age_ms`, `qos.lease_duration_ms`, `rate_hz`). The number is in
    /// its field as any written one; absent for a written number.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sources: BTreeMap<String, ValueSource>,
}

impl PathContract {
    /// The trigger a consumer schedules by. A model that carries no
    /// `trigger` (written before design issue #52) yields
    /// [`ros_launch_manifest_sched::EffectiveTrigger::Unclassified`], never
    /// a timer: the absence must stay visible to whoever ranks the path.
    pub fn effective_trigger(&self) -> ros_launch_manifest_sched::EffectiveTrigger {
        self.trigger
            .clone()
            .unwrap_or(ros_launch_manifest_sched::EffectiveTrigger::Unclassified)
    }
}

/// Fan-in synchronisation on an `Input` path (Vocabulary v2; design issue
/// #52), durations lowered to milliseconds like every other bound here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncContract {
    pub policy: SyncPolicy,
    /// Match window for `exact` / `approximate`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_interval_ms: Option<f64>,
    /// Collect-until-timeout duration for `timeout_any`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<f64>,
}

/// Fan-in matching policy. Mirrors the contract crate's closed set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncPolicy {
    /// `message_filters` ExactTime.
    Exact,
    /// `message_filters` ApproximateTime.
    Approximate,
    /// Collect until the timeout, then publish the partial set.
    TimeoutAny,
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
    /// Where each number of this entry read from the launch tree came from
    /// (play_launch phase 94), keyed by the model field it fills
    /// (`max_age_ms`, `qos.lease_duration_ms`, `rate_hz`). The number is in
    /// its field as any written one; absent for a written number.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sources: BTreeMap<String, ValueSource>,
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
    /// QoS deadline between consecutive messages (phase 74). Applied to the
    /// running node as a `qos_overrides.<topic>.<entity>.deadline` parameter
    /// where the node opted in; the live observer reads it as a detector.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline_ms: Option<f64>,
    /// Liveliness lease (phase 74). Same route as `deadline_ms`. Lowered so
    /// the runtime observer can judge a guard's silence against the lease
    /// the contract declared rather than the topic's cadence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_duration_ms: Option<f64>,
}

// ---------------------------------------------------------------------------
// layer 3 — execution / deployment
// ---------------------------------------------------------------------------

/// Integrator-owned placement + scheduling: facts every realizer reads with
/// the same meaning (`docs/model-boundary.md`).
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
    /// R1-M3 — in-binary topic-relay bridges.
    /// Topic types resolve from layer 1 wiring, never written here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bridges: Vec<Bridge>,
    // `features` (one realizer's build switches) was written until v0.1.54
    // and read-tolerant until v0.1.56. A model that still carries it loads:
    // `Execution` does not deny unknown fields, so the key is ignored.
    // NOTE: the resolved scheduling plan is NOT carried in the model
    // (2026-07-20 maintainer decision — reverted the 45.2/45.3 `sched`
    // embedding). The model is INPUT only; causality + execution modeling is
    // each consumer's job (scheduling.md §"Cross-repo design agreement"). The
    // chain/DAG algorithm is a standalone reusable crate both runtimes call,
    // not an embedded output.
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
    }
}

/// Where a node runs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Deploy {
    // `target` (`linux` / `mcu:<board>`) and `extra` (one realizer's build
    // fields) were written until v0.1.54 and read-tolerant until v0.1.56. A
    // model that still carries them loads: `Deploy` does not deny unknown
    // fields, so the keys are ignored. No realizer read either.
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
}

/// Tier definitions reuse the sched crate's schema (RT config v2, phase 41):
/// a portable head (`class`, `deadline_us`, `period_us`, `budget_us`,
/// `deadline_policy`, `spin_period_us`) plus fixed per-platform placement
/// sub-tables (`posix`/`freertos`/`zephyr`/`threadx`/`nuttx` — `priority`,
/// `stack_bytes`, `core`, `sched_class`, `preempt_threshold`, per-platform
/// `deadline_us` override). One schema for the authoring form
/// (`system.toml`), the model, and the `SchedMapper` pipeline.
pub use ros_launch_manifest_sched::types::{ReservedBand, TierDef, TierPlatformSpec};

#[cfg(test)]
mod order_tests {
    use super::*;

    /// Issue 0382 — `structure.nodes` keeps DECLARATION order through a
    /// serialize/deserialize round trip. A `BTreeMap` alphabetized it away, so
    /// a launch declaring `talker` before `listener` emitted an entry that
    /// constructed `listener` first. Names chosen so alphabetical != insertion.
    #[test]
    fn nodes_keep_declaration_order_through_yaml() {
        let mut st = Structure::default();
        for fqn in ["/talker", "/listener", "/aaa_last"] {
            st.nodes.insert(fqn.to_string(), NodeInstance::default());
        }
        let before: Vec<&str> = st.nodes.keys().map(String::as_str).collect();
        assert_eq!(before, vec!["/talker", "/listener", "/aaa_last"]);

        let yaml = serde_yaml_ng::to_string(&st).expect("serialize");
        // The wire form must not be alphabetized either — the emitter reads it
        // in file order.
        let t = yaml.find("/talker").expect("talker in yaml");
        let l = yaml.find("/listener").expect("listener in yaml");
        let a = yaml.find("/aaa_last").expect("aaa_last in yaml");
        assert!(t < l && l < a, "yaml lost declaration order:\n{yaml}");

        let back: Structure = serde_yaml_ng::from_str(&yaml).expect("deserialize");
        let after: Vec<&str> = back.nodes.keys().map(String::as_str).collect();
        assert_eq!(after, before, "round trip lost declaration order");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Phase 94: a number read from the launch tree carries its provenance
    /// beside it, and a reader of the number is unchanged: the field reads
    /// as before, and an entry with no source serializes as before.
    #[test]
    fn a_read_value_carries_its_source_beside_the_number() {
        let mut p = PathContract {
            output: vec!["/o".into()],
            trigger: Some(ros_launch_manifest_sched::EffectiveTrigger::Timer { rate_hz: 10.0 }),
            ..Default::default()
        };
        let plain = serde_yaml_ng::to_string(&p).unwrap();
        assert!(!plain.contains("sources"), "{plain}");
        p.sources.insert(
            "rate_hz".into(),
            ValueSource {
                written: "$(param update_rate) Hz".into(),
                reads: vec![ValueRead::Param {
                    node: "/mrm_handler".into(),
                    name: "update_rate".into(),
                    value: "10 Hz".into(),
                    at: "mrm_handler.param.yaml".into(),
                }],
            },
        );
        let text = serde_yaml_ng::to_string(&p).unwrap();
        assert!(text.contains("sources:\n  rate_hz:\n    written: $(param update_rate) Hz\n    reads:\n    - kind: param\n      node: /mrm_handler"), "{text}");
        let back: PathContract = serde_yaml_ng::from_str(&text).unwrap();
        assert_eq!(back, p);
        // A reader that predates the field: the number is where it was.
        #[derive(serde::Deserialize)]
        struct Old {
            trigger: ros_launch_manifest_sched::EffectiveTrigger,
        }
        let old: Old = serde_yaml_ng::from_str(&text).unwrap();
        assert_eq!(
            old.trigger,
            ros_launch_manifest_sched::EffectiveTrigger::Timer { rate_hz: 10.0 }
        );
    }

    /// `meta.inputs` paths must be portable.
    ///
    /// They used to be canonicalised into a COMMITTED artifact, so a model was
    /// only correct on the machine that produced it: everywhere else the
    /// consumer's existence check failed and it silently fell back to a fixed
    /// filename.
    #[test]
    fn input_paths_are_recorded_relative_to_the_base() {
        let tmp = tempfile::tempdir().unwrap();
        let bringup = tmp.path().join("src").join("demo_bringup");
        std::fs::create_dir_all(bringup.join("launch")).unwrap();
        let launch = bringup.join("launch").join("system.launch.xml");
        std::fs::write(&launch, "<launch/>").unwrap();
        let sys = bringup.join("system.toml");
        std::fs::write(&sys, "[system]\n").unwrap();

        assert_eq!(
            input_path_string(&launch, Some(&bringup)),
            "launch/system.launch.xml"
        );
        assert_eq!(input_path_string(&sys, Some(&bringup)), "system.toml");

        // No base -> absolute, the pre-0293 behaviour, kept for callers that
        // have no meaningful root.
        assert!(
            std::path::Path::new(&input_path_string(&launch, None)).is_absolute(),
            "without a base the path stays absolute"
        );

        // Outside the root (an include from a sibling package) has no useful
        // relative form, so it stays absolute rather than growing a `../..`
        // chain that means nothing in an install space.
        let elsewhere = tmp.path().join("other_pkg").join("extra.launch.xml");
        std::fs::create_dir_all(elsewhere.parent().unwrap()).unwrap();
        std::fs::write(&elsewhere, "<launch/>").unwrap();
        assert!(
            std::path::Path::new(&input_path_string(&elsewhere, Some(&bringup))).is_absolute(),
            "an input outside the base stays absolute"
        );
    }

    /// v0.1.46: the takeover keys round-trip, and a message constant name
    /// never comes back as a string literal (or the other way round).
    #[test]
    fn the_takeover_keys_round_trip() {
        let h = HazardContract {
            on: vec![FaultKind::Reported],
            when: Some(PredicateContract {
                field: "autonomous".into(),
                op: CompareOp::Equals,
                value: PredicateValueContract::Bool(false),
            }),
            ftti_derived: Some(DerivedSource {
                by: "process.pst".into(),
                version: "0.1.0".into(),
                sha256: Some("ab".repeat(32)),
                hazard: None,
                inputs: BTreeMap::from([("hazard.max_rise_rate".into(), "0.5 bar/s".into())]),
            }),
            ..Default::default()
        };
        let y = serde_yaml_ng::to_string(&h).unwrap();
        assert_eq!(serde_yaml_ng::from_str::<HazardContract>(&y).unwrap(), h);

        let f = FunctionContract {
            members: vec!["/vehicle/status/control_mode".into()],
            all_of: false,
            when: Some(PredicateContract {
                field: "mode".into(),
                op: CompareOp::Equals,
                value: PredicateValueContract::Constant {
                    constant: "MANUAL".into(),
                },
            }),
        };
        let y = serde_yaml_ng::to_string(&f).unwrap();
        assert!(y.contains("constant: MANUAL"), "{y}");
        assert_eq!(serde_yaml_ng::from_str::<FunctionContract>(&y).unwrap(), f);
        let lit = PredicateValueContract::Str("MANUAL".into());
        let y = serde_yaml_ng::to_string(&lit).unwrap();
        assert_eq!(
            serde_yaml_ng::from_str::<PredicateValueContract>(&y).unwrap(),
            lit
        );
        for v in [
            PredicateValueContract::Int(4),
            PredicateValueContract::Float(16.7),
        ] {
            let y = serde_yaml_ng::to_string(&v).unwrap();
            assert_eq!(
                serde_yaml_ng::from_str::<PredicateValueContract>(&y).unwrap(),
                v
            );
        }

        let m = ModeContract {
            window: Some(WindowContract {
                duration_ms: 10_000.0,
                param: Some("/mrm_handler.takeover_request_timeout".into()),
            }),
            exit: Some(ExitContract {
                on: "0/driver_took_over".into(),
                to: "0/manual".into(),
            }),
            ..Default::default()
        };
        let y = serde_yaml_ng::to_string(&m).unwrap();
        assert_eq!(serde_yaml_ng::from_str::<ModeContract>(&y).unwrap(), m);

        let ss = SafeStateContract {
            emits: "/op/cmd".into(),
            settle_ms: Some(2033.33),
            settle_derived: Some(DerivedSource {
                by: "vehicle.braking".into(),
                version: "1.0.0".into(),
                sha256: None,
                hazard: Some("hpc_loss".into()),
                inputs: BTreeMap::from([("hazard.entry_speed".into(), "3 m/s".into())]),
            }),
            supervise: None,
        };
        let y = serde_yaml_ng::to_string(&ss).unwrap();
        assert!(!y.contains("supervise"), "{y}");
        assert_eq!(
            serde_yaml_ng::from_str::<SafeStateContract>(&y).unwrap(),
            ss
        );
        // Phase 91 E10: a model written before it (a braking profile by
        // name, an entry speed) still reads; the keys are dropped.
        let old: SafeStateContract = serde_yaml_ng::from_str(
            "emits: /op/cmd\nsettle_profile: { decel: target_acceleration, jerk: target_jerk }\n",
        )
        .unwrap();
        assert_eq!((old.settle_ms, old.settle_derived), (None, None));
        let old: HazardContract =
            serde_yaml_ng::from_str("on: omission\nentry_speed_mps: 8.33\n").unwrap();
        assert_eq!(old.on, vec![FaultKind::Omission]);
        // v0.1.64: a subscriber's ingress bound round-trips, and a
        // subscriber written before it (no `ingress`) still reads.
        let sub = SubContract {
            min_rate_hz: Some(10.0),
            ingress: Some(IngressContract {
                rate_hz: 200.0,
                burst: 4,
            }),
            ..Default::default()
        };
        let y = serde_yaml_ng::to_string(&sub).unwrap();
        assert!(y.contains("burst: 4"), "{y}");
        assert_eq!(serde_yaml_ng::from_str::<SubContract>(&y).unwrap(), sub);
        let old: SubContract = serde_yaml_ng::from_str("min_rate_hz: 10.0\n").unwrap();
        assert_eq!(old.ingress, None);
        assert!(!serde_yaml_ng::to_string(&old).unwrap().contains("ingress"));
        // v0.1.68: an input's release bound round-trips on a subscriber and
        // a server, and one written before it still reads.
        let sub = SubContract {
            min_interarrival_ms: Some(50.0),
            budget_ms: Some(2.0),
            budget_period_ms: Some(100.0),
            ..Default::default()
        };
        let y = serde_yaml_ng::to_string(&sub).unwrap();
        assert!(y.contains("min_interarrival_ms: 50.0"), "{y}");
        assert_eq!(serde_yaml_ng::from_str::<SubContract>(&y).unwrap(), sub);
        let srv = SrvContract {
            min_interarrival_ms: Some(100.0),
            ..Default::default()
        };
        let y = serde_yaml_ng::to_string(&srv).unwrap();
        assert_eq!(serde_yaml_ng::from_str::<SrvContract>(&y).unwrap(), srv);
        let old: SrvContract = serde_yaml_ng::from_str("max_response_ms: 5.0\n").unwrap();
        assert_eq!(
            (old.min_interarrival_ms, old.budget_ms, old.budget_period_ms),
            (None, None, None)
        );
        let old: SubContract = serde_yaml_ng::from_str("min_rate_hz: 10.0\n").unwrap();
        assert_eq!(
            (old.min_interarrival_ms, old.budget_ms, old.budget_period_ms),
            (None, None, None)
        );
        assert!(!serde_yaml_ng::to_string(&old).unwrap().contains("budget"));
        // v0.1.69: a client's call mode round-trips in both maps, and a
        // model written before it still reads.
        let mut c = Contracts::default();
        c.cli_endpoints.insert(
            "/n/ask".into(),
            CliContract {
                call: Some(CallMode::Blocking),
            },
        );
        c.action_cli_endpoints.insert(
            "/n/goal".into(),
            CliContract {
                call: Some(CallMode::Async),
            },
        );
        assert!(!c.is_empty());
        let y = serde_yaml_ng::to_string(&c).unwrap();
        assert!(y.contains("call: blocking"), "{y}");
        assert!(y.contains("call: async"), "{y}");
        assert_eq!(serde_yaml_ng::from_str::<Contracts>(&y).unwrap(), c);
        let old: Contracts = serde_yaml_ng::from_str("srv_endpoints: {}\n").unwrap();
        assert!(old.cli_endpoints.is_empty() && old.action_cli_endpoints.is_empty());
        assert!(
            !serde_yaml_ng::to_string(&old)
                .unwrap()
                .contains("cli_endpoints")
        );
        // v0.1.63: a mode override carries its resolved target, and one
        // written before v0.1.63 (no `resolved`) still reads.
        let ov = ModeOverrideContract {
            target: "nodes.planner.sub.scan.min_rate_hz".into(),
            value: "5".into(),
            resolved: Some(ResolvedOverride {
                table: OverrideTable::SubEndpoints,
                key: "/nav/planner/scan".into(),
                field: "min_rate_hz".into(),
            }),
        };
        let y = serde_yaml_ng::to_string(&ov).unwrap();
        assert!(y.contains("table: sub_endpoints"), "{y}");
        assert_eq!(
            serde_yaml_ng::from_str::<ModeOverrideContract>(&y).unwrap(),
            ov
        );
        let old: ModeOverrideContract =
            serde_yaml_ng::from_str("target: paths.stop.max_latency\nvalue: 50ms\n").unwrap();
        assert_eq!(old.resolved, None);
        assert!(!serde_yaml_ng::to_string(&old).unwrap().contains("resolved"));
        // v0.1.62: a supervised settle's lowering round-trips.
        let sv = SafeStateContract {
            supervise: Some(SettleSuperviseContract {
                response_ms: 943.0,
                observe_ms: 156.0,
                observes: "/h/kinematic_state".into(),
                reaction: "island.emergency_stop".into(),
                mechanism: SuperviseMechanism::Timer,
            }),
            ..ss.clone()
        };
        let y = serde_yaml_ng::to_string(&sv).unwrap();
        assert_eq!(
            serde_yaml_ng::from_str::<SafeStateContract>(&y).unwrap(),
            sv
        );
        // Absent keys stay absent: a model with none of them is unchanged.
        let plain = serde_yaml_ng::to_string(&HazardContract::default()).unwrap();
        assert!(
            !plain.contains("when") && !plain.contains("entry_speed"),
            "{plain}"
        );
    }

    /// Phase 82: a hazard's `on` is a set, but a set of one stays a bare
    /// scalar on the wire so a model a pre-v0.1.43 reader consumes is
    /// byte-identical; a scalar and a list both read back, and an omitted
    /// key is the empty set.
    #[test]
    fn hazard_on_is_a_scalar_when_single_and_reads_either_shape() {
        let one = HazardContract {
            on: vec![FaultKind::Omission],
            ..Default::default()
        };
        let y = serde_yaml_ng::to_string(&one).unwrap();
        assert!(y.contains("on: omission\n"), "{y}");
        assert_eq!(serde_yaml_ng::from_str::<HazardContract>(&y).unwrap(), one);

        let two = HazardContract {
            on: vec![FaultKind::Omission, FaultKind::Late],
            ..Default::default()
        };
        let y = serde_yaml_ng::to_string(&two).unwrap();
        assert!(y.contains("- omission") && y.contains("- late"), "{y}");
        assert_eq!(serde_yaml_ng::from_str::<HazardContract>(&y).unwrap(), two);

        let none: HazardContract = serde_yaml_ng::from_str("ftti_ms: 5.0\n").unwrap();
        assert!(none.on.is_empty());
        assert!(!serde_yaml_ng::to_string(&none).unwrap().contains("on:"));
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

    /// Phase 46.1b — `NodeInstance` launch spawn fields round-trip with
    /// clean named-struct YAML shapes (not bare tuples): `remaps` as
    /// `- from: … / to: …` and `env` as `- name: … / value: …`, mirroring
    /// the `SegmentNode` precedent for pair lists in this crate.
    #[test]
    fn node_instance_launch_fields_roundtrip() {
        let node = NodeInstance {
            scope: "/perception".into(),
            pkg: Some("lidar_centerpoint".into()),
            exec: Some("detector_node".into()),
            remaps: vec![
                Remap {
                    from: "/points".into(),
                    to: "/sensing/lidar/points".into(),
                },
                Remap {
                    from: "/diagnostics".into(),
                    to: "/diag/detector".into(),
                },
            ],
            ros_args: vec!["--log-level".into(), "detector_node:=debug".into()],
            respawn: Some(true),
            respawn_delay: Some(2.5),
            on_exit: Some(OnExit::Shutdown),
            env: vec![EnvVar {
                name: "CUDA_VISIBLE_DEVICES".into(),
                value: "0".into(),
            }],
            ..Default::default()
        };

        let yaml = serde_yaml_ng::to_string(&node).unwrap();
        // named struct shapes, not bare tuples.
        assert!(yaml.contains("from: /points"), "{yaml}");
        assert!(yaml.contains("to: /sensing/lidar/points"), "{yaml}");
        assert!(yaml.contains("name: CUDA_VISIBLE_DEVICES"), "{yaml}");
        assert!(yaml.contains("value: '0'"), "{yaml}");
        assert!(yaml.contains("respawn: true"), "{yaml}");
        assert!(yaml.contains("respawn_delay: 2.5"), "{yaml}");
        assert!(yaml.contains("on_exit: shutdown"), "{yaml}");

        let reparsed: NodeInstance = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(node, reparsed);
    }

    /// Backward-compat: a `NodeInstance` with none of the Phase 46.1b launch
    /// fields (pre-46 artifact) parses with every new field defaulting to
    /// empty/`None`, and re-emitting it invents none of the new keys.
    #[test]
    fn node_instance_without_launch_fields_parses_with_defaults() {
        let yaml = "\
scope: /perception
pkg: lidar_centerpoint
exec: detector_node
";
        let node: NodeInstance = serde_yaml_ng::from_str(yaml).unwrap();
        assert!(node.remaps.is_empty());
        assert!(node.ros_args.is_empty());
        assert_eq!(node.respawn, None);
        assert_eq!(node.respawn_delay, None);
        assert_eq!(node.on_exit, None);
        assert_eq!(node.start_delay_secs, None);
        assert!(node.env.is_empty());

        let re_emitted = serde_yaml_ng::to_string(&node).unwrap();
        assert!(!re_emitted.contains("remaps:"), "{re_emitted}");
        assert!(!re_emitted.contains("ros_args:"), "{re_emitted}");
        assert!(!re_emitted.contains("respawn:"), "{re_emitted}");
        assert!(!re_emitted.contains("on_exit:"), "{re_emitted}");
        assert!(!re_emitted.contains("start_delay_secs:"), "{re_emitted}");
        assert!(!re_emitted.contains("env:"), "{re_emitted}");
    }

    /// The `<timer>` start delay round-trips, and is INDEPENDENT of
    /// `respawn_delay` — the two are different waits (before the first spawn
    /// vs. between a death and the next one), and the only reason to test
    /// them together is that their names invite conflating them.
    #[test]
    fn node_instance_start_delay_roundtrips_beside_respawn_delay() {
        let node = NodeInstance {
            scope: "/nav".into(),
            pkg: Some("nav2_bringup".into()),
            exec: Some("bt_navigator".into()),
            // an outer 3s timer around an inner 3s one, ROS-style.
            start_delay_secs: Some(6.0),
            respawn: Some(true),
            respawn_delay: Some(2.5),
            ..Default::default()
        };

        let yaml = serde_yaml_ng::to_string(&node).unwrap();
        assert!(yaml.contains("start_delay_secs: 6.0"), "{yaml}");
        assert!(yaml.contains("respawn_delay: 2.5"), "{yaml}");

        let reparsed: NodeInstance = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(node, reparsed);

        // Either one alone is representable.
        let delayed_only = NodeInstance {
            start_delay_secs: Some(20.0),
            ..Default::default()
        };
        let yaml = serde_yaml_ng::to_string(&delayed_only).unwrap();
        assert!(yaml.contains("start_delay_secs: 20.0"), "{yaml}");
        assert!(!yaml.contains("respawn"), "{yaml}");
        assert_eq!(
            serde_yaml_ng::from_str::<NodeInstance>(&yaml).unwrap(),
            delayed_only
        );
    }

    /// Phase 46.3a — the six spawn-completeness gap fields (`args`,
    /// `params_files`, `raw_cmd`, `extra_args`) round-trip. `args`/
    /// `params_files` on a regular node, `extra_args` on a composable node,
    /// `raw_cmd` on a raw-executable-shaped instance (no `pkg`/`exec`).
    #[test]
    fn node_instance_gap_fields_roundtrip() {
        let node = NodeInstance {
            scope: "/perception".into(),
            pkg: Some("lidar_centerpoint".into()),
            exec: Some("detector_node".into()),
            args: vec!["--verbose".into(), "--config".into(), "a.yaml".into()],
            params_files: vec!["/**:\n  ros__parameters:\n    a: 1\n".into()],
            ..Default::default()
        };
        let yaml = serde_yaml_ng::to_string(&node).unwrap();
        assert!(yaml.contains("args:"), "{yaml}");
        assert!(yaml.contains("--verbose"), "{yaml}");
        assert!(yaml.contains("params_files:"), "{yaml}");
        assert!(yaml.contains("ros__parameters"), "{yaml}");
        let reparsed: NodeInstance = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(node, reparsed);

        let composable = NodeInstance {
            scope: "/perception".into(),
            plugin: Some("tracker::TrackerNode".into()),
            container: Some("/perception/pipeline_container".into()),
            extra_args: BTreeMap::from([(
                "use_intra_process_comms".to_string(),
                "True".to_string(),
            )]),
            ..Default::default()
        };
        let yaml = serde_yaml_ng::to_string(&composable).unwrap();
        assert!(yaml.contains("extra_args:"), "{yaml}");
        assert!(yaml.contains("use_intra_process_comms"), "{yaml}");
        let reparsed: NodeInstance = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(composable, reparsed);

        let raw_exec = NodeInstance {
            scope: "/".into(),
            raw_cmd: vec!["/usr/bin/carla-server".into(), "-quality-level=Low".into()],
            ..Default::default()
        };
        let yaml = serde_yaml_ng::to_string(&raw_exec).unwrap();
        assert!(yaml.contains("raw_cmd:"), "{yaml}");
        assert!(yaml.contains("carla-server"), "{yaml}");
        let reparsed: NodeInstance = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(raw_exec, reparsed);
    }

    /// Backward-compat: the Phase 46.3a fields default to empty and are
    /// omitted from re-emitted YAML when unset (pre-46.3a artifacts parse
    /// unchanged, and models that never touch these gaps stay noise-free).
    #[test]
    fn node_instance_without_gap_fields_parses_with_defaults() {
        let yaml = "\
scope: /perception
pkg: lidar_centerpoint
exec: detector_node
";
        let node: NodeInstance = serde_yaml_ng::from_str(yaml).unwrap();
        assert!(node.args.is_empty());
        assert!(node.params_files.is_empty());
        assert!(node.raw_cmd.is_empty());
        assert!(node.extra_args.is_empty());

        let re_emitted = serde_yaml_ng::to_string(&node).unwrap();
        assert!(!re_emitted.contains("args:"), "{re_emitted}");
        assert!(!re_emitted.contains("params_files:"), "{re_emitted}");
        assert!(!re_emitted.contains("raw_cmd:"), "{re_emitted}");
        assert!(!re_emitted.contains("extra_args:"), "{re_emitted}");
    }

    /// Phase 46.3b — `node_name` (declared name, distinct from `exec`) and
    /// `is_container` round-trip, and a `name=None` node emits neither
    /// key (the whole point: the model must be able to say "no declared
    /// name").
    #[test]
    fn node_instance_provenance_fields_roundtrip() {
        let named = NodeInstance {
            scope: "/perception".into(),
            pkg: Some("lidar_centerpoint".into()),
            exec: Some("detector_node".into()),
            node_name: Some("detector".into()),
            ..Default::default()
        };
        let yaml = serde_yaml_ng::to_string(&named).unwrap();
        assert!(yaml.contains("node_name: detector"), "{yaml}");
        assert!(!yaml.contains("is_container"), "{yaml}");
        let reparsed: NodeInstance = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(named, reparsed);

        let container = NodeInstance {
            scope: "/perception".into(),
            pkg: Some("rclcpp_components".into()),
            exec: Some("component_container".into()),
            node_name: Some("pipeline_container".into()),
            is_container: true,
            ..Default::default()
        };
        let yaml = serde_yaml_ng::to_string(&container).unwrap();
        assert!(yaml.contains("is_container: true"), "{yaml}");
        let reparsed: NodeInstance = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(container, reparsed);

        // name=None node: node_name is None, so no `node_name:` key at all
        // (that absence IS the "no declared name" signal the spawn path
        // reads).
        let unnamed = NodeInstance {
            scope: "/perception".into(),
            pkg: Some("lidar_centerpoint".into()),
            exec: Some("detector_node".into()),
            node_name: None,
            ..Default::default()
        };
        let yaml = serde_yaml_ng::to_string(&unnamed).unwrap();
        assert!(!yaml.contains("node_name"), "{yaml}");
        let reparsed: NodeInstance = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(unnamed, reparsed);
        assert_eq!(reparsed.node_name, None);
    }

    /// Backward-compat: pre-46.3b artifacts (no `node_name`/`is_container`)
    /// parse with `node_name: None`, `is_container: false`.
    #[test]
    fn node_instance_without_provenance_fields_parses_with_defaults() {
        let yaml = "\
scope: /perception
pkg: lidar_centerpoint
exec: detector_node
";
        let node: NodeInstance = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(node.node_name, None);
        assert!(!node.is_container);

        let re_emitted = serde_yaml_ng::to_string(&node).unwrap();
        assert!(!re_emitted.contains("node_name"), "{re_emitted}");
        assert!(!re_emitted.contains("is_container"), "{re_emitted}");
    }
}
