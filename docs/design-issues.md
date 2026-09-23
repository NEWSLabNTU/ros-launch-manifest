# Design Issues

Design questions for the manifest format, with proposed solutions and
their resolutions.

## How to read this file

**This is a log of decisions, not a description of the format.** An
entry records what was decided *at the time*, in the vocabulary of that
time. An entry that says `chains:` was removed still says `chains:`;
one that resolved `max_transport_ms` still says `max_transport_ms`,
years after the `_ms` name-suffix spellings became parse errors. That is
deliberate — rewriting an entry into today's vocabulary would destroy
the record and make the reasoning incoherent.

For what the grammar **is** today, read
[format-reference.md](format-reference.md), which is generated from
`types/src/field_table.rs` and cannot drift.

Every entry carries one of four statuses, in its heading and — where
the story did not end with the entry — in a `**Status (date):**` line
directly beneath it:

| Status | Meaning |
|---|---|
| **Done** | Decided, implemented, and still how the code behaves. |
| **Done; superseded by …** | The decision was right then and has since been overtaken. The body is left untouched; the status line names what replaced it. |
| **Accepted** | Decided to change nothing. |
| **Open** | Not settled. |

Status lines were last reconciled against the code on **2026-09-23**
(19 rules in `check/src/rules/mod.rs::default_rules()`, five workspace
members, format reference as generated).

**Roll-up.** Issues 1–51, #53 and #54 are resolved — of those, #29,
#31, #45, #48 and #50 have since been superseded, and #50 was
*reversed* (see its status line). #52 is open on the consumer side only: the four
units it planned for this repository all landed and shipped as
`v0.1.37`.

## Resolved Issues

Issues resolved in prior phases, preserved in git history. **Field names
below are as of the decision**, not as of today:

- **1–6, 8–16**: Args, substitutions, conditions, service contracts,
  doc fixes, parser bugs, unified scope interface, dangling entity
  checks, arg types + satisfiability, YAML `?` suffix — Done
  (Phases 32–33)
- **7, 19, 20, 21, 26, 27**: Global topics, `pub:`/`sub:` overloading,
  scope interface duplication, topic key naming, worked example
  indirection, ROS topic mapping — Superseded by #33
- **24**: Transport latency — Done (`max_transport_ms` on topics)
- **25**: Periodic formula double-counts — Done (upstream removed)
- **28**: `args:` position — Done (`version:` first in examples)
- **17**: Cross-scope service suppression — Resolved by #41
- **33**: Topic keys as ROS names — Done (spec + code). Resolves #7,
  #19, #20, #21, #26, #27
- **34**: Scope paths use topic names — Done (Option A)
- **35**: Parent manifest purpose — Done (E2E contracts via scope paths)
- **36**: Rate check cross-scope merge — Done (note added)
- **37**: Absolute names verbose — Accepted (tooling helps)
- **38**: Relative vs absolute guidance — Done (added to Topics section)
- **39**: `type:` field table — Done (consistent `yes` / `Error`)
- **40**: `global_topics:` note — Done (removed)
- **41**: Services follow topic pattern — Done (ROS names, cross-scope merge)
- **30**: Example error messages — Done (7 rules illustrated)
- **32**: Capture mode — Done (section added to launch-manifest.md)
- **29**: `exclude_patterns` — Done (replaces defaults, `[]` includes all)
- **31**: `correlation: latest` stamp — Done (primary input's stamp)
- **23**: Age on subscriber endpoints — Done (moved from paths)
- **22**: Drop composition — Done (static sanity only, composition is runtime)

---

## ~~17. Cross-Scope Service Wiring Has No Suppression Mechanism~~ — Resolved by #41

Services and actions now follow the same naming pattern as topics
(ROS names, cross-scope merge). The `service-wiring` rule checks the
merged service across the tree, so cross-scope clients no longer
produce orphan `cli:` warnings — they are wired by name matching.
No suppression mechanism is needed. See #41 for details.

---

## ~~18. CLI Should Support Per-Rule Filtering~~ — Done

**Status (2026-09-23): holds; one flag in the example is gone.**
`--rule <RULE_ID>` is live and repeatable. `--manifest-dir` was
replaced by the contract CHANNELS (`--contracts <dir>`,
`$PLAY_LAUNCH_CONTRACTS`, XDG, `/etc`, then the provider sidecar beside
the launch file), so the example line no longer runs as written.

Implemented in Phase 34.8. The `play_launch check` command now accepts
a repeatable `--rule <RULE_ID>` flag that filters diagnostics
(per-scope and cross-scope) by rule ID. The summary line shows the
active filter. Example:

```bash
play_launch check --manifest-dir manifests/ --rule consistency --rule budget-overflow <pkg> <launch>
```

---

## ~~22. Drop Composition Assumes Independence Despite Burstiness~~ — Done

Resolved: drop composition moved from static checker to **runtime
monitoring only**. The static checker now validates local consistency
only (values in range, scope drop rate not tighter than any topic's,
effective delivery meets subscriber demand). Chain composition and
`max_consecutive` enforcement are runtime concerns — they depend on
actual transport conditions. Appendix A retained as runtime monitoring
theory.

---

## ~~23. Age Verification Effectively Unimplementable Statically~~ — Done

**Status (2026-09-23): holds.** Only the spelling moved: the key is
`max_age` (a duration with a unit suffix), `max_age_ms` being one of the
nine `_ms` aliases phase 70 turned into parse errors. `lifespan-age`
(cross-scope) is the rule that reads it.

Resolved by moving `max_age_ms` from scope paths to **subscriber
endpoints**. Age is now a data freshness constraint at the point of
consumption (`now - header.stamp` at `rcl_take`), checked at runtime
by the interception layer. No chain tracing needed. Static checking
verifies local consistency (age budget vs known latency budget) but
doesn't attempt a full chain proof.

---

## ~~29. `exclude_patterns` Override Behavior Undocumented~~ — Done; superseded

**Status (2026-09-23): superseded by phase 70 — `exclude_patterns` was
removed from the grammar.** The consumer census found it had three
mentions in the whole codebase (the table row, the struct field, the
parse line) and **no consuming read**: the suppression suppressed
nothing. The key is now a parse error naming its replacement, the
per-topic `external:` mark (and the `external_topics:` block), which
`dangling-entity` honours on the side it names — see design issue #54
and `docs/format-reference.md`.

Documented: user declaration **replaces** defaults. `exclude_patterns: []`
includes all topics.

---

## ~~30. No Example Error Messages for Validation Rules~~ — Done

**Status (2026-09-23): holds.** Two of the seven rules it illustrates,
`budget-overflow` and `consistency`, are cross-scope rules emitted by
the consumer's merge layer rather than members of this crate's 19 — the
example diagnostics are still what a user sees, but not all from the
same pass.

Added example diagnostics block to the Static Validation section
covering 7 rules: `endpoint-unique`, `wiring`, `rate-hierarchy`,
`budget-overflow`, `dangling-entity`, `consistency`, `satisfiability`.

---

## ~~31. `correlation: latest` Output Timestamp Unspecified~~ — Done; superseded

**Status (2026-09-23): superseded by phase 70 — `correlation` was
removed from the grammar.** It parsed, reached the causal graph and
lowered to a `model::Correlation` enum that no arithmetic ever branched
on — the same write-only shape as `semantics: age`. `timestamp` /
`latest` is `sync:` present / absent, which three rules and the rate
derivation do read. The primary-input-stamp ruling below still
describes how a fan-in path's output stamp behaves; only the key that
spelled it is gone.

Specified: `correlation: latest` output stamp = **primary (first listed)
input's stamp**. Based on analysis of 9 Autoware fusion nodes — 7 of 9
propagate the primary input's timestamp. Secondary inputs enrich the
data but don't determine the timestamp. Age follows the primary branch
only. Updated launch-manifest.md and contract-theory.md (parallel
composition section + summary table).

---

## ~~32. Capture Mode Buried in Theory Appendix~~ — Done (doc only)

**Status (2026-09-23): the doc move landed; the feature did not.**
`--save-manifest-dir` does not exist in play_launch, so
`launch-manifest.md`'s "Generating Manifests from a Running System"
section documents a flag no binary accepts. `slides.md` lists capture
mode as an open item. Either the section grows a "not implemented"
marker or the flag gets built; `launch-manifest.md` is not owned by
this file.

Added "Generating Manifests from a Running System" section to
launch-manifest.md with `--save-manifest-dir` usage, what it generates,
and a link to the statistical derivation in contract-theory.md Appendix C.

---

## ~~33. Topic Keys as ROS Topic Names~~ — Done

**Status (2026-09-23): holds; the rule it proposes is named
differently.** "New rule: `topic-consistency`" shipped as
**`consistency`**, and it lives in the consumer's cross-scope merge,
not in this crate's 19 — the in-crate placeholder of that id was
removed in `v0.1.38` (see #54). The field it checks is spelled
`max_transport` today.

Resolved (spec + code). Resolves #7, #19, #20, #21, #26, #27.

Code: `manifest_loader.rs` qualifies relative topic keys against the
scope namespace (`qualify_name`, `qualify_endpoint_ref`) and merges
topic declarations across scopes by FQN (`merge_topic`,
`ResolvedTopic`). Topic-level field consistency (`type:`, `rate_hz:`,
`qos:`, `max_transport_ms`) is enforced under the `consistency`
rule ID during merge.

### Design

Topic keys are ROS topic names — relative (resolved by the checker
using the scope's namespace from the launch tree) or absolute (`/`).
The same topic can appear in multiple manifests; `type:` must agree,
`pub:`/`sub:` are merged. Scope interface removed. `global_topics:`
removed.

### Data (Autoware 1.5.0, 182 topics)

| Category             | Count     | Key format |
|----------------------|-----------|------------|
| Within one subsystem | 147 (81%) | Relative   |
| Cross-subsystem      | 35 (19%)  | Absolute   |

327 topic×scope pairs: 71% sub-only, 26% pub-only, 3% both.
Only 4 topics have publishers in multiple scopes.

### Consistency Rule

- `type:` — required in every declaration, must match across scopes
- `rate_hz:`, `qos:` — must agree if declared in multiple scopes
- `pub:`, `sub:` — merged across scopes by the checker
- New rule: `topic-consistency` validates agreement

### Resolved Questions

1. **Duplicate declarations** → consistency rule (agree, not SSoT)
2. **Deep endpoint refs** → scope-local only (each scope refs own nodes)
3. **Discoverability** → checker merges by resolved name
4. **Namespace** → no `ns:` field; checker uses scope table at check time
5. **Standalone launch** → each manifest self-contained with `type:`

---

## ~~34. Scope-Level Paths Contradict Scope-Local Refs~~ — Done (Option A)

Scope paths now use **topic names** as input/output (not node/endpoint
refs). The checker traces dataflow between the named topics, considering
only nodes within the declaring scope's subtree. When parent and child
declare paths with the same resolved (input, output) topics,
`budget-overflow` checks child budget ≤ parent budget.

---

## ~~35. Parent Manifest Has No Purpose Without Scope Paths~~ — Done

Resolved by #34. Parent manifests declare E2E contracts (scope paths
with topic-name input/output) over child subtrees. The parent also
declares `includes:` for the manifest tree structure.

---

## ~~37. Absolute Topic Names Verbose for Sibling Scopes~~ — Accepted

Accept the verbosity (Option A/D). 81% of topics use short relative
keys. The 19% cross-subsystem topics use absolute names that match
`ros2 topic list` output. Capture mode generates the names
automatically. No format complexity needed.

---

## ~~41. Services Don't Follow the Topic Naming Pattern~~ — Done

Services and actions now follow the same pattern as topics: ROS names
as keys (relative or absolute), cross-scope merge, consistency rule.
`service-wiring` checks the merged service across the tree. Also
resolves #17 — cross-scope services are wired by name matching, no
orphan warnings.

---

## ~~17. Cross-Scope Service Wiring Has No Suppression Mechanism~~ — Resolved by #41 (duplicate heading)

**Status (2026-09-23): this is a second copy of the #17 entry above**,
with the same resolution in different words. Kept rather than deleted
because the log is append-only; read either one.

Cross-scope services are now wired by ROS name matching across the
manifest tree, same as topics. No orphan `cli:` warnings — the
`service-wiring` rule checks the merged service after cross-scope merge.

---

## ~~42. Topology-Unaware Sum Check Produces False Warnings~~ — Done

Resolved in Phase 35.1–35.4 (Option A: topology-aware check).

The cross-scope critical-path check in `manifest_loader.rs` builds a
global dataflow graph and uses forward DP with `max` over predecessors
at fork-join points, correctly handling parallel branches. The
`manifest_parallel_pipeline` fixture (lidar 50ms + camera 30ms →
fusion 20ms) verifies that `max(50, 30) + 20 = 70ms` is accepted
without false-warning on the sum (100ms).

The per-manifest `scope-budget` sum check remains as a conservative
fallback for standalone checking (documented as such in
`scope_budget.rs`). For full cross-scope trees, the precise
critical-path check supersedes it.

---

## ~~43. Scope Path Dataflow Tracing Underspecified~~ — Done; partly superseded

**Status (2026-09-23): the algorithm holds; it moved twice.** The file
is now `src/ros-launch-resolve/resolve/src/ros/manifest_graph.rs` in
play_launch, and #52's R2 ported `build_global_graph`,
`subgraph_for_scope_path` and `critical_path` off `ManifestIndex` onto
the SystemModel, in this repository, as `derive::resolve_chains`. The
"opaque-scope optimization" noted below as future work is still not
done. Two later corrections are recorded in play_launch rather than
here: per-path attribution (a node-keyed `max_latency_ms` over-charges
a multi-output node's route) and the fact that a scope path's subgraph
starting AT the input topic leaves an upstream timer boundary outside
the traced region — which is what `scope-sampling-feasibility` exists
for.

Resolved in Phase 35.1–35.4. The algorithm is now specified and
implemented in `src/play_launch/src/ros/manifest_graph.rs`:

- **Graph construction**: `build_global_graph(&ManifestIndex)` builds
  a cross-scope graph from merged topic publishers/subscribers.
  State edges (subscribers with `state: true`) are marked separately
  and skipped in causal traversal.
- **Subgraph extraction**: `subgraph_for_scope_path()` restricts to
  nodes in the scope's subtree and identifies sources/sinks from the
  scope path's resolved input/output topics.
- **Critical path**: `critical_path()` uses topological sort followed
  by forward DP. At each node, `latency = max(predecessor.latency +
  edge.transport) + node.processing`, which correctly handles:
  - Series chains (sum)
  - Fork-join (max at the join point)
  - Multi-input nodes (max over predecessors)
  - State edges (skipped)
- **Diamond patterns**: handled naturally by the topological sort —
  each node's latency is computed once based on its predecessors,
  regardless of how many paths converged on it.
- **Opaque vs transparent scopes**: the current implementation treats
  all scopes as transparent (walks into their nodes). Opaque-scope
  optimization (using declared child budgets without traversing) is
  a potential future improvement but not required for correctness.

---

## ~~44. `max_transport_ms` Ambiguous for Multi-Subscriber Topics~~ — Done

**Status (2026-09-23): holds.** The per-subscriber override and the
edge-weight rule are as decided; the key is now spelled
`max_transport` on both the topic and the subscriber endpoint
(`max_transport_ms` is a parse error since phase 70). The critical-path
DP it describes was ported onto the SystemModel in `derive::
resolve_chains` — see #52.

Resolved (spec + code).

### Problem

`max_transport_ms` is declared per topic, but a single ROS 2 topic can
have subscribers with different transport characteristics — one
intra-process (~0ms), one on the same machine via shared memory (<1ms),
one across a network bridge (5–10ms). A single value per topic can't
express this without forcing every path through the worst case.

Transport latency is a **deployment** property of the (publisher,
subscriber) pair, not of the topic itself. Endpoint-level QoS (#45)
does not solve this — QoS profile fields don't capture physical
topology.

### Design (Option B — per-subscriber override)

**Format extension.** `EndpointProps` (subscriber side, mirrored on the
publisher side for symmetry but unused there in v1) gains an optional
`max_transport_ms: f64` field. Topic-level `max_transport_ms` remains
as the default applied to every subscriber that does not override.

**Override rule.** Edge weight in critical-path computation:

```
edge[pub → sub].transport = sub.max_transport_ms
                         ?? topic.max_transport_ms
                         ?? 0
```

**Critical path becomes per-sink.** The DP recurrence in
`manifest_graph.rs::critical_path()` is updated to use the per-edge
weight:
`latency[node] = max_pred( latency[pred] + edge[pred → node].transport )
              + processing[node]`.
The change is local — replaces the current per-topic transport lookup
with a per-(pred, node) lookup.

**Pub-side intentionally omitted.** A publisher does not know which
subscriber will consume data over which transport. Subscribers know
their own consumption pattern (intra-process, SHM, network), so the
override naturally lives on the sub. Pub-side `max_transport_ms` is
not part of the format.

**Cross-scope.** Topic-level `max_transport_ms` stays under
`consistency` rule — must agree across declarations. Per-sub overrides
live on a node and are local to the declaring scope, no cross-scope
agreement required.

### Spec

`docs/launch-manifest.md` §Latency and Data Freshness now documents the
heterogeneous-transport case with the override rule, edge-weight
resolution, per-sink DP recurrence, and a worked example with
intra-process / SHM / network subscribers on the same topic. The
subscriber properties table includes `max_transport_ms`. The topic
field description notes overridability.

### Code — done

- `types/src/types.rs`: `EndpointProps.max_transport_ms` (sub only).
- `types/src/parse.rs`: endpoint parsing extended.
- `ros-launch-resolve` `resolve/src/ros/manifest_graph.rs::critical_path()`:
  per-(pred, node) edge weight lookup with the per-sub override.

---

## ~~45. No QoS Publisher-Subscriber Compatibility Check~~ — Done; partly superseded

**Status (2026-09-23): holds, and its deferrals have since been taken
up.** "v1 covers `reliability` and `durability` only; `liveliness`,
`deadline` and `lifespan` compatibility are deferred" is no longer
true: phase 70 W4 added `liveliness` and its `lease_duration` to
`qos-match` (a publisher asserting less often than the subscriber's
lease is one it will periodically declare dead), in this crate and in
the consumer's cross-scope copy — which for a lease is the normal case,
since publisher and subscriber usually sit in different files. Phase 74
went further than checking: a contract's `qos.deadline` /
`liveliness` / `lease_duration` is now APPLIED, written into the node's
parameters as rclcpp's `qos_overrides.<topic>.<entity>.<policy>`.

Resolved (spec + code).

### Problem

The `qos-compat` rule validates that QoS field values are from the
allowed set (e.g., `reliability: maybe` → error). It does not check
**publisher-subscriber QoS compatibility** — one of the most common
ROS 2 deployment bugs:

- `best_effort` publisher + `reliable` subscriber → **incompatible**
  (no data flows)
- `volatile` publisher + `transient_local` subscriber →
  **incompatible** (late-joining subscriber misses data)

To express the mismatch, the format had to grow: in the original spec
QoS was declared once per topic, so there was no way to model a
publisher and subscriber that disagree on the same channel.

### Design

**Format extension.** `EndpointProps` (publisher and subscriber) gains
an optional `qos: QosDecl` field. Topic-level `qos:` remains as the
default, applied to every endpoint that does not specify its own.

**Override rule (field-level).** Effective QoS for an endpoint is
computed per field: `endpoint.qos.<f> ?? topic.qos.<f> ?? unspecified`.
An endpoint that overrides only `reliability` still inherits other
fields from the topic. Empty `qos: {}` on an endpoint inherits the
topic default in full. Overrides are silent.

**Cross-scope.** Topic-level `qos:` must still agree across scopes
(`consistency` rule). Endpoint-level overrides live on a node and are
local to its declaring scope — they do not participate in cross-scope
merge.

**`qos-match` rule.** After cross-scope merge, the checker computes
effective QoS for each pub and sub on each merged topic and checks
compatibility for every (pub, sub) pair. Rule: **offered ≥ requested**.

| Field         | Pub               | Sub               | Compatible? |
|---------------|-------------------|-------------------|-------------|
| `reliability` | `reliable`        | `reliable`        | yes |
| `reliability` | `reliable`        | `best_effort`     | yes |
| `reliability` | `best_effort`     | `reliable`        | **no** |
| `reliability` | `best_effort`     | `best_effort`     | yes |
| `durability`  | `transient_local` | `transient_local` | yes |
| `durability`  | `transient_local` | `volatile`        | yes |
| `durability`  | `volatile`        | `transient_local` | **no** |
| `durability`  | `volatile`        | `volatile`        | yes |

A field is checked only when both sides specify it (directly or
inherited). The checker does not assume ROS 2 defaults — multiple
profiles (sensor data, services, parameters) have different defaults,
so guessing is wrong. v1 covers `reliability` and `durability` only.
`liveliness`, `deadline`, and `lifespan` compatibility are deferred.

**Conditional endpoints.** When args gate publishers or subscribers,
`qos-match` runs per satisfiable arg model (sharing infrastructure with
`satisfiability`); errors are emitted only for pairs that coexist in
some valid configuration.

**Deferred.** Service/action endpoint QoS, `state-durability` lint
(warn when `state: true` sub uses `volatile`).

### Spec

`docs/launch-manifest.md` §Quality of Service documents the format,
override semantics, match rule, and example diagnostic.

### Code — done

- `types/src/types.rs`: `EndpointProps.qos: Option<QosDecl>`, with
  `QosDecl::effective(topic, endpoint)` field-level overlay.
- `types/src/parse.rs`: endpoint `qos:` parsing.
- `check/src/rules/qos_match.rs`: `qos-match` rule (structural checks +
  reliability/durability pub-sub compatibility).

---

## ~~46. No Guidance on Manifest Node Naming~~ — Done

Added to §Nodes: manifest node name must match the ROS 2 node name
(`name=` attribute / `__node:=` remap, as shown in `ros2 node list`).

---

## ~~47. Missing Inline Include Example~~ — Done

Added inline include example to §Includes.

---

## ~~48. `header.stamp` Propagation Stated as Rule but Is Convention~~ — Done; superseded in part

**Status (2026-09-23): the ruling holds, its spelling does not.** "A
periodic path is one with `input: []`" was the Vocabulary v1
convention; Vocabulary v2 replaced it with an explicit
`trigger: { timer: { rate_hz: N } }`, and the distinction is
load-bearing rather than cosmetic — an empty `input:` cannot tell a
timer from `once`, from `spontaneous`, or from a path whose trigger was
simply never declared, and the derivations must answer `Unknown` for
the last three instead of assuming a clock. `PathDecl::
effective_trigger()` is the one reader. See #52, whose first table row
is the cost of that convention leaking into the SystemModel.

Softened to "should" with guidance: nodes that reset the stamp should
be modeled as periodic paths (`input: []`).

---

## ~~49. Lifecycle Nodes Not Addressed~~ — Done (Option B)

Added `lifecycle: Option<bool>` field on `NodeDecl`. When set to true,
the node is marked as a ROS 2 managed node, and runtime monitors
gate contract checks (rate, latency, age) on the node being in the
Active state. Static checking is unaffected.

Example:

```yaml
nodes:
  lidar_driver:
    lifecycle: true
    pub:
      pointcloud: { min_rate_hz: 10 }   # applies when driver is Active
```

Updated:
- `types/src/types.rs` — added `lifecycle` field
- `types/src/parse.rs` — parses `lifecycle` key, new test `test_lifecycle_node`
- `docs/launch-manifest.md` — Background section explains lifecycle
  semantics; Nodes format reference documents the field

**Out of scope for v1** (future work):
- Per-state contracts (different rate/latency per state)
- Activation ordering / dependency graph
- Parser auto-detection from launch file (`LifecycleNode` action)

---

## ~~50. `min_latency_ms` Poorly Motivated~~ — Done; **reversed**

**Status (2026-09-23): this decision was reversed, and the reversal was
never recorded here.** `min_latency` (no `_ms` suffix) is back in the
grammar, on both node and scope paths — phase 67 reintroduced it with a
motivation the original removal did not have, and phase 70 gave it the
consumer it was missing. The motivation: every other bound in the
vocabulary is an UPPER bound, so `max_jitter` had nothing to be
falsified against. The `jitter-range` rule (`check/src/rules/
jitter_range.rs`, registered in `default_rules()`) now errors on
`min_latency > max_latency`, warns when
`max_latency - min_latency > max_jitter`, and reports an ABSENT
`min_latency` as *unverifiable* — reading absence as zero was a defect
caught on the `contract_w2` fixture. `play_launch measure` emits a
measured floor as `nodes.<n>.paths.<p>.min_latency`.

Removed from both node and scope path field tables. Not used in any
rule, example, or validation.

---

## ~~51. No Way to Declare External Topics / Producers~~ — Done; extended

**Status (2026-09-23): holds, and the asymmetry it left was closed by
#54.** Until then only the CONSUMER's cross-scope re-run of
`dangling-entity` honoured a topic's `external:` mark; the in-crate
rule did not, so one pass warned and the other did not, on the same
file. The in-crate rule now reads `external: pub | both` (and the
`external_topics:` block) exactly as it reads the service and action
marks, and answers only for the side the mark names. Layer 3 (the
ancestor walk) is still deferred. `exclude_patterns`, discussed below
as the tool that does not solve this, was removed outright in phase 70
— see #29.

Resolved (spec + code). Implementation: top-level `external_topics:`
block per Option B + per-topic `external:` flag per Option A.

### Implementation summary

- **types crate**: `Manifest::external_topics: BTreeMap<String, ExternalTopicDecl>`,
  `TopicDecl::external: Option<ExternalSide>`, new enum
  `ExternalSide { Pub, Sub, Both }`, new struct
  `ExternalTopicDecl { side, msg_type?, qos? }`.
- **parser**: parses `external_topics:` block (accepts `side:` or
  `external:` field name) and per-topic `external:` flag. Validates
  side value ∈ `{pub, sub, both}`. Substitution applied to
  `external_topics.<fqn>.type` like other type fields.
- **`manifest_loader`**: `ManifestIndex.externals: BTreeMap<Fqn, ExternalSide>`
  merged across all loaded manifests in `collect_externals()` (qualified
  per declaring scope's ns; conflicting sides upgrade to `Both`).
  Cross-scope `dangling-entity` skips externally-marked sides on both
  `no publishers` and the newly added `no subscribers` checks.
- **consistency rule**: cross-checks `external_topics.<fqn>.type` against
  any internal `topics.<fqn>.type` of the same FQN. Mismatches emit
  `consistency` errors (this immediately caught real TODO-type bugs in
  the Autoware contract repo).
- **per-manifest noise suppression**: in cross-scope mode
  (`--manifest-dir`), per-manifest `dangling-entity` and `service-wiring`
  rules are dropped before emission — the cross-scope merge is
  authoritative. Prevents O(n) duplicate warnings on legitimate
  cross-scope endpoints.

### Verification

Applied to `~/repos/autoware-contract`: 49 `external_topics:` entries
in `autoware_launch/planning_simulator.yaml`. End-to-end check result:
**0 errors, 0 warnings** across 63 manifests. Type cross-check surfaced
8 real type-mismatch bugs during the migration (now fixed).

### Layers implemented vs deferred

| Layer | Status |
|-------|--------|
| 1. Full `--manifest-dir` visibility for cross-scope merge | Done (pre-existing) |
| 2. Top-level `external_topics:` block at any manifest | Done |
| 3. Ancestor walk for standalone-leaf checks | Deferred (no demand yet) |
| 4. Per-topic `external:` flag | Done |

Layer 3 (collect ancestors' external_topics when checking only a
subtree) is the only deferred piece. Add when a real workflow needs
standalone-leaf checks with deep external dependencies.

---

## 51-archive. Original problem description (preserved for context)

### Problem

After cross-scope merge, the `dangling-entity` rule warns on any topic
with zero publishers (or zero subscribers) anywhere in the manifest
tree. This is correct when the manifest tree is meant to cover a
self-contained launch graph. It is **wrong** when the launch tree
intentionally consumes from systems outside the manifest scope:

- **Sensor drivers** publishing `/sensing/lidar/...` not authored as
  manifests yet
- **Vehicle interface** publishing `/vehicle/status/*` and consuming
  `/vehicle/command/*` (real hardware bridge or CARLA bridge)
- **Map loader** publishing `/map/vector_map`, `/map/pointcloud_map`,
  `/map/projector_info` (its own package, optionally manifest-covered)
- **TF broadcaster** publishing `/tf`, `/tf_static`
- **External cmd source** publishing `/external/local/*`,
  `/external/remote/*` (joystick, web teleop, etc.)
- **rviz2 / debug consumers** subscribing to debug topics
- **bag replay** providing data on any sensor topic
- **Cross-deployment dependencies** (e.g. perception subscribes to a
  topic that another deployment publishes)

In a real Autoware contract repo (`~/repos/autoware-contract`), the
post-Phase-9 check shows ~40 such warnings — every one is an external
producer or consumer. The warnings are noise that hides real problems.

### Why Existing Tools Don't Solve It

- `exclude_patterns:` filters topics out of cross-scope merge entirely;
  but these topics ARE consumed by our nodes — they just have an
  external producer. Excluding them silences the wiring check that
  validates our consumer-side declaration is correct.
- Manually adding stub manifests for every external system is tedious
  and pollutes the tree with non-actionable nodes whose only purpose
  is to satisfy `dangling-entity`.
- `# nolint` comments don't exist in the format.

### Options

| Option                                    | Effort | Description                                                                                      |
|-------------------------------------------|--------|--------------------------------------------------------------------------------------------------|
| A. `external: true` flag on topic         | Small  | Marks a topic's missing side as expected-external. Suppresses dangling-entity for that side only |
| B. Top-level `external_topics:` block     | Small  | Manifest-level list of FQNs known to come from outside. Same effect as A, less per-topic noise   |
| C. Stub-manifest convention               | Medium | Author thin manifests under `external/` package — node-less, pure topic decls. Status quo + tooling |
| D. CLI suppression flag                   | Small  | `play_launch check --external <fqn>...` per invocation                                           |
| E. Cross-scope dangling severity = info   | Trivial| Demote warning to info; lose the safety net entirely                                             |

### Recommendation

**B (preferred) + A (secondary).**

- B gives a single per-manifest declaration of expected external
  producers/consumers, with optional `type:` for cross-check:
  ```yaml
  external_topics:
    /tf:
      type: tf2_msgs/msg/TFMessage
      pub: external                # external system publishes
    /vehicle/command/control_cmd:
      type: autoware_control_msgs/msg/Control
      sub: external                # external system subscribes
    /map/vector_map:
      type: autoware_map_msgs/msg/LaneletMapBin
      pub: external
  ```
  Behavior: `dangling-entity` skips any FQN listed here on the matching
  side. `consistency` still applies (`type:` must agree with declarations
  in other scopes). `qos-match` runs against the listed `qos:` if
  declared.

- A as a shorthand for one-off cases: a topic block can mark its missing
  side directly without a separate top-level list. Useful inside a
  scope's leaf manifest:
  ```yaml
  topics:
    /sensing/lidar/concatenated/pointcloud:
      type: sensor_msgs/msg/PointCloud2
      external: pub                # consumed by us, produced externally
      sub: [lidar_processor/input]
  ```

### Cross-scope merge interaction

When a topic appears in `external_topics:` of scope A and in `topics:`
(with internal pubs) of scope B, the merge resolves to internal —
external is a fallback declaration, not authoritative. This is what
makes the convention safe under partial migration: if an internal
producer manifest is added later, the warning naturally goes away
without the external list needing edit.

### Status

Open. Spec change small (~20 lines in launch-manifest.md + a new field
on `Manifest` and `TopicDecl`). Code change small (filter
`dangling-entity` in `manifest_loader::run_cross_scope_checks` against
the merged external set). High value — removes ~95% of the dangling
warnings on a fully-migrated tree without losing the rule's safety net.

---

## 52. Two Consumers, Two Derivations of the Same Mapper Input — Open (consumer side)

**Status (2026-09-23): this repository's four units all landed and
shipped; the issue stays open because the CONSUMERS have not migrated.**
The `### Status` section at the foot of this entry still says "no code
yet" — that was true when the entry was written on 2026-09-21 and is
contradicted by the tree today: `derive/` is the fifth workspace member
(`Cargo.toml` `members = ["types", "check", "sched", "model",
"derive"]`), it exports `mapper_input_from_model`, `resolve_chains`,
`DeriveFacts` and `DeriveReport`, and `model/src/lib.rs` carries all six
R1 fields (`PathContract::trigger`, `::sync`, `::min_latency_ms`,
`SubContract::buffer`, `Contracts::severity_levels`,
`::node_criticality`). R1–R4 landed 2026-09-21 as `9563b54`, `2367972`,
`13c3f63`, `bfbe075`, tagged **`v0.1.37`**. The per-unit `Status:` lines
in the Parallel plan below are the accurate record. What remains is
consumer-side: play_launch phase-78 (pin, lower, delete its copy, ship
0.12.0) and nano-ros phase-457. One seam is already known and recorded
at the foot of R4: `MapperNode::scope` must be the NAMESPACE the manual
mapper's `[[assign]] scope =` selector matches, while `derive` copies
`NodeInstance::scope`, the file-scope key — patched consumer-side, owed
by this crate.

### Problem

The 2026-07-20 decision (`f090400`, RFC-0050 "Input model") kept the
`chain_aware` ALGORITHM shared and made the DERIVATION of its input
"per-consumer, sharing the `MapperInput` type". Fourteen months on, the two
derivations disagree on most of the facts the algorithm ranks by, and the
model they were both supposed to derive from cannot express the first of
them. Verified 2026-09-21 against rlm `ea5cbea`, play_launch `5eaa3191`
(0.11.0) and nano-ros `783cdfa14`:

| fact | play_launch (`ros-launch-resolve/resolve/src/ros/sched_derive.rs`) | nano-ros (`nros-orchestration-ir/src/mapper_input.rs`) |
|---|---|---|
| effective trigger | `PathDecl::effective_trigger()` from the parsed manifest (:142) | `input.is_empty()` means Timer, rate = the FIRST output's `pub.min_rate_hz`, else 0.0 (:72-81) |
| chains | derived from scope paths + the global graph's critical path (:251) | `chains: Vec::new()` (:155); the core degrades to the bucket fallback by construction |
| criticality | hazard-derived first (`index.derived_criticality`, phase 72), label second (:496) | the label only (:130): the model carries `NodeInstance.criticality` as the raw string (model_builder.rs:669) |
| `claims_concurrency` | "no merged group covers every path" (:195) | "some path is outside every set" (:140); `[[a, b], [c]]` over `{a, b, c}` differs |
| `rate_hz` (RM mapper) | max over topic `rate_hz`, derived rate, pub `min_rate_hz` (:424) | unset |
| `deadline_us` (DM mapper) | min over path `max_latency` and srv `max_response` (:462) | unset; `realize_rtos` re-derives from paths, without `max_response` |
| `exec_ms` | platform `budget_us` per node, attributed only when the node has one path (:131) | `[wcet]` profile per boundary id (:92) |
| `max_jitter_ms`, `miss` | read through | read through (nano-ros phase 434) - the one row that agrees |

The first row is the seam: the resolver lowers Timer, Once, Spontaneous and
Unclassified alike to `input: []` (model_builder.rs:954-960) and drops
`rate_hz`, so a `once` map loader whose output happens to carry a
`min_rate_hz` is a Timer to nano-ros, and a timer whose output carries none
has period `None` and never ranks. `PathContract.input`'s doc (lib.rs:1130)
still says "Empty = periodic (timer-driven)", the convention Vocabulary v2
retired in the `types` crate. On the safety island (four timer paths, 10 and
30 Hz) the two toolchains agree today only because every timer output also
promises the timer's rate as `min_rate_hz` - the 14 `derivable-min-rate`
infos the resolver prints are the reason the schedule is right, which is
backwards.

Not lowered at all: `trigger`, `sync`, `min_latency`, `buffer` (a
`state: true` subscription's discipline), `severity_levels`. A consumer that
wanted to rank by them, or explain a criticality, could not.

Why the previous attempt does not settle this. `execution.sched` (78f637d,
reverted by f090400) embedded the mapper's OUTPUT - resolved chains, per-path
ranks, the mapper's identity - and that was wrong for a reason that still
holds: ranks are realizer-specific and stale on replay, and a second
toolchain reading them would inherit the Linux realization. That decision
said nothing about the INPUT, and its "derivation stays per-consumer" clause
was a scoping choice (nano-ros had no model-side derivation yet), not a
finding. The table above is what that clause cost.

### Options

| Option | Effort | Description |
|---|---|---|
| A. Status quo plus lints | Small | Keep two derivations; add a cross-repo test that compares rankings on a fixture. Detects drift, never removes it; the model still cannot say "timer at 10 Hz". |
| B. Lower the trigger, keep two derivations | Small | `PathContract.trigger` carries `sched::EffectiveTrigger`; nano-ros reads it. Fixes row 1; rows 2-7 stay as they are and drift again. |
| C. One derivation in rlm, model carries the checker's facts | Medium | New crate `derive/` (`ros-launch-manifest-derive`): `mapper_input_from_model(&SystemModel, &DeriveFacts) -> (MapperInput, DeriveReport)` and `resolve_chains(&SystemModel, &DeriveFacts) -> Vec<ResolvedChain>`, consumed by both. The model gains the per-entity facts the checker resolves (effective trigger, effective criticality, sync, min_latency, buffer, severity_levels). No mapper output is embedded. |
| D. One derivation in `ros-launch-resolve` | Medium | Same function, hosted in play_launch's tree. nano-ros already vendors that crate through its play_launch submodule (`nros-launch-resolve`), but `ros-launch-resolve` links tokio, the Python loader and the parser; `nros-orchestration-ir` is a core crate the `nros::main!` proc-macro depends on and cannot take that tree. It would also leave the route derivation on `ManifestIndex`, an object only play_launch has. |
| E. Embed `MapperInput` in the model | Small | The 45.2 shape again, one level up. A `MapperInput` is not a serde type on purpose (mapper.rs:57), and `exec_ms` is a platform fact that differs per consumer, so the embedded copy would be wrong for one of them by construction. |

### Recommendation

**C.** The rule that separates it from the reverted embedding, stated once:

> The model carries every fact the CHECKER resolves per entity - effective
> trigger, effective criticality, sync, buffer, bounds. It carries nothing
> the MAPPER resolves - no route, no rank, no tier. The derivation from the
> first to the second is one function, in this repository, that both
> consumers call and neither reimplements.

Effective trigger and effective criticality are contract facts fixed by the
contract alone, target-independent, and no different in kind from `miss` and
`max_jitter_ms`, which already cross for exactly this reason (lib.rs:1148-
1169). A route or a rank depends on the whole graph and on the realizer,
which is why those stay out.

Model changes (all additive; `SCHEMA_VERSION` stays 1, an older model parses
with the new fields absent):

- `PathContract.trigger: Option<sched::EffectiveTrigger>` - the value of
  `PathDecl::effective_trigger()` at resolve time, in the adjacent-tagged
  `kind`/`value` shape chain.rs:45 was already built for. `input` keeps the
  Input trigger's endpoints and its doc string stops claiming that empty
  means periodic. A consumer reading a model with `trigger: None` treats the
  path as `Unclassified`, never as a timer - the error case must stay loud.
- `PathContract.sync: Option<SyncContract { policy, max_interval_ms,
  timeout_ms }>` and `PathContract.min_latency_ms: Option<f64>`.
- `SubContract.buffer: Option<BufferContract>` (`latest` | `queue`).
- `Contracts.severity_levels: Vec<String>` (empty = the ISO 26262 default)
  and `Contracts.node_criticality: BTreeMap<String, sched::Criticality>`,
  the EFFECTIVE criticality after the phase-72 derivation (hazards decide
  first, the label where none reaches), keyed by node FQN. The advisory
  string on `NodeInstance` stays for dashboards; the mapper reads the map.

The `derive` crate. It depends on `model` and `sched` (the reverse is
impossible: `model` already depends on `sched` for `MapperMiss`), which is
why it is neither of them. `sched` stays parser-free and `model` stays a
schema. Its contents:

- `mapper_input_from_model`: one `MapperNode` per `structure.nodes` entry
  with `paths` from `contracts.node_paths` (trigger, `max_latency_ms`,
  `max_jitter_ms`, `miss`, inputs, outputs), `criticality` from
  `node_criticality`, `claims_concurrency` by the merged-group rule
  (sched_derive.rs:195, the one that models what an executor does),
  `rate_hz` = the fastest timer trigger among the node's paths and NOTHING
  else, `deadline_us` = min over path `max_latency_ms` and
  `srv_endpoints.max_response_ms`, `exec_ms` from `DeriveFacts`. `legacy`
  is left `None`; the `.toml` bridge sets it afterwards.
- `resolve_chains`: `manifest_graph::{build_global_graph,
  subgraph_for_scope_path, critical_path}` ported from `ManifestIndex` onto
  the model - `structure.topics` endpoint refs, `node_paths` inputs and
  outputs, `sub_endpoints.state` to break cycles, `structure.scopes` for the
  subtree, `scope_paths` for the declared budget. Chain criticality is the
  max over members, boundaries are timer paths with `period_ms = 1000 /
  rate_hz`. Everything it needs is already in the model, which is the test
  of whether the model is what it claims to be.
- `DeriveFacts { path_exec_ms: BTreeMap<"<node>/<path>", f64>, node_exec_ms:
  BTreeMap<"<node>", f64> }`: the consumer's cost facts. play_launch fills
  `node_exec_ms` from the platform file's `budget_us`, nano-ros fills
  `path_exec_ms` from its `[wcet]` profile; the "a node budget is attributed
  only when the node has one path" rule moves into the crate so it is applied
  the same way by both. No WCET is invented.
- `DeriveReport { paths_without_trigger, chains_resolved, chains_skipped }`
  so a consumer can say which paths a pre-migration model left unclassified
  instead of silently ranking nothing.

Authored `topics.<t>.rate_hz` and `pub.<ep>.min_rate_hz`: derive-only for the
scheduler, promises everywhere else. No mapper reads them (this retires the
`rate_hz` row of scheduling.md's fact table). They stay in the model because
the runtime monitors read them - nano-ros's `PubMonitorCell` and
`queue_depth`, play_launch `measure` - and the resolver keeps checking them
against the timers: `rate-mismatch` and `min-rate-mismatch` stay warnings,
`derivable-rate` and `derivable-min-rate` stay infos. A promise that agrees
with the timer is redundant, not wrong; the scheduler simply no longer
depends on it agreeing. The one visible change is to `rate_monotonic`: a
node with no timer path has no rate to be monotonic about and lands on the
default tier, where `chain_aware` already put it.

Parity, asserted three times:

1. `derive` owns a fixture model (play_launch's `contract_derived_chain`,
   resolved and checked in) and a `RankedPlan` snapshot; a test pins
   `chain_aware_rank(&mapper_input_from_model(..))` to it, next to the
   existing rank-vs-realize split-parity test (chain_aware_mapper.rs:1038).
2. play_launch, during its transition: `mapper_input_from_dump(dump, index)
   == mapper_input_from_model(&build_checked_model(..))` on every contract
   fixture, then `sched_derive.rs`'s derivation and `manifest_graph`'s route
   copy are deleted.
3. nano-ros: the same fixture model yields the same `RankedPlan` Debug text
   as snapshot 1, and `realize_rtos` receives the shared `MapperInput`
   unchanged.

What each consumer keeps: play_launch keeps `realize_posix`, the apply layer,
and `execution.tiers`/`bindings` as the applied outcome; nano-ros keeps
`realize_rtos`, `SchedCaps`, `Degradation`, `TierSpec`, the callback-group
gate and WCET profile selection. What nano-ros deletes: `node_paths_for`,
`pub_rate_hz`, `parse_criticality`, and its own `claims_concurrency` -
`mapper_input.rs` becomes a `DeriveFacts` builder and one call.

Migration order, by tag: rlm ships the fields and the crate as v0.1.37;
play_launch pins v0.1.37, lowers the new fields, lands parity test 2 and
deletes its copy, ships as 0.12.0; nano-ros bumps both pins (rlm v0.1.37,
play_launch v0.12.0 - its gitlink is at v0.9.0-158-g07f0461e today), replaces
`mapper_input.rs`, lands parity test 3 and re-resolves its models, whose
`meta.resolver.version` changes with the pin. The order matters: a model
resolved by the old play_launch carries no `trigger`, and the shared
function ranks nothing on it, which is a regression from today's coincidental
agreement until the pin moves - `DeriveReport.paths_without_trigger` is what
makes that visible.

Cross-references: play_launch `docs/roadmap/phase-78-one-derivation-two-consumers.md`
(producer side and the transition gate); nano-ros
`docs/roadmap/phase-457-consume-the-shared-derivation.md` (consumer side).

### Parallel plan

Four units, each a branch and a PR, so that separate sessions can take one
each. This repository has no claim tool (nano-ros has `just claim
phase-NNN-Wk`): the claim is the branch named below, pushed with an early
draft PR, so a second session sees it before it starts. The `Status:` line
under a unit is edited in the PR that lands it. Consumers: play_launch
`docs/roadmap/phase-78-one-derivation-two-consumers.md` (W1 pins the tag R4
cuts) and nano-ros `docs/roadmap/phase-457-consume-the-shared-derivation.md`
(its W1 waits on play_launch's 0.12.0).

| unit | depends on | owns | gate | starts now? | branch |
|---|---|---|---|---|---|
| R1 model fields | none | `model/src/lib.rs` (`PathContract` :1129, `SubContract` :1093, `Contracts` :749), `model/tests/golden/perception.system_model.yaml`, `model/tests/golden_roundtrip.rs` | `cargo test -p ros-launch-manifest-model`; `UPDATE_FORMAT_REFERENCE=1 cargo test -p ros-launch-manifest-types` only if `types/src/field_table.rs` is touched | yes | `phase-52-R1` |
| R2 the `derive` crate | none to start; rebases onto R1 | new `derive/` (`derive/Cargo.toml`, `derive/src/lib.rs`), the `members` line of `Cargo.toml` | `cargo test -p ros-launch-manifest-derive`; `cargo test --workspace` | yes, on the current model | `phase-52-R2` |
| R3 parity tests and the golden snapshot | R1 and R2 merged | `derive/tests/` (fixture model, `RankedPlan` snapshot, the tests); `sched/src/chain_aware_mapper.rs` only if the existing split-parity test's snapshot is factored out for reuse | `cargo test -p ros-launch-manifest-derive`; `cargo test -p ros-launch-manifest-sched chain_aware_rank_is_priorityless_and_split_is_parity` | no | `phase-52-R3` |
| R4 tag v0.1.37 and the CHANGELOG | R3 merged | `CHANGELOG.md` (new), the `v0.1.37` tag | `cargo test --workspace` green on the tagged commit; play_launch phase-78 W1 pins the tag | no | `phase-52-R4` |

R1 and R2 proceed in parallel: they share no file. R2 builds on the model
as it is today, where every path has `trigger: None` and is therefore
`Unclassified`; `mapper_input_from_model` ranks nothing on it and
`DeriveReport.paths_without_trigger` lists every path, which is the
documented pre-migration behaviour and a test in its own right. When R1
merges, R2 rebases and reads the six fields. R3 waits on both, because a
snapshot taken over a model without a trigger would pin "ranks nothing".
R4 waits on R3, because the tag is what play_launch pins and the snapshot
is what its W2 gate compares against.

**R1 - model fields.** The additive fields listed under "Model changes"
above: `PathContract.trigger`, `PathContract.sync`,
`PathContract.min_latency_ms`, `SubContract.buffer`,
`Contracts.severity_levels`, `Contracts.node_criticality`; and the
`input` doc string at lib.rs:1130 stops saying that empty means periodic.
The golden model gains one timer path carrying every new field so the round
trip covers them, and a second test loads the golden model with the fields
absent. These are SystemModel fields, not manifest keys, so the format table
is not touched unless a key is added to the authored grammar.

Claim: `phase-52-R1`. Depends on: nothing. Owns: `model/src/lib.rs`,
`model/tests/golden/perception.system_model.yaml`,
`model/tests/golden_roundtrip.rs`. Gate: `cargo test -p
ros-launch-manifest-model`; `UPDATE_FORMAT_REFERENCE=1 cargo test -p
ros-launch-manifest-types` if `types/src/field_table.rs` changes. Status: landed in 9563b54 (main, 2026-09-21); model gate 38+12+7+13 passed,
workspace green. R2 may rebase and read the six fields.

**R2 - the `derive` crate.** `ros-launch-manifest-derive`, the fifth
workspace member, depending on `model` and `sched`, with
`mapper_input_from_model`, `resolve_chains`, `DeriveFacts` and
`DeriveReport` as specified above. `resolve_chains` is the port of
play_launch's `manifest_graph::{build_global_graph, subgraph_for_scope_path,
critical_path}` (`src/ros-launch-resolve/resolve/src/ros/manifest_graph.rs`
:237, :453, :662 at 0.11.0) from `ManifestIndex` onto the model's
`structure.topics`, `node_paths`, `sub_endpoints.state`, `structure.scopes`
and `scope_paths`. Until R1 lands, `trigger` reads as `None` and the crate
treats every path as `Unclassified`; the rebase onto R1 is a field read, not
a redesign.

Claim: `phase-52-R2`. Depends on: nothing to start; rebases onto R1 before
merge. Owns: `derive/Cargo.toml`, `derive/src/lib.rs`, the `members` line of
`Cargo.toml`. Gate: `cargo test -p ros-launch-manifest-derive`; `cargo test
--workspace`. Status: landed in 2367972 (main, 2026-09-21); derive gate 20 passed,
workspace green, rustfmt clean. R3 may start.

**R3 - parity tests and the golden snapshot.** Parity assertion 1 above:
a fixture model under `derive/tests/`, a `RankedPlan` snapshot, and a test
pinning `chain_aware_rank(&mapper_input_from_model(..))` to it, the
`derive`-side twin of `chain_aware_rank_is_priorityless_and_split_is_parity`
(chain_aware_mapper.rs:1038; `sched` cannot depend on `derive`, so the twin
lives in `derive`). The fixture is play_launch's `contract_derived_chain`.
Its first checked-in copy is the 0.11.0 resolution with R1's fields filled
by hand from the contract, because the play_launch that lowers them
(phase-78 W1) pins the tag R4 cuts after this unit; phase-78 W1 re-emits the
fixture and phase-78 W2's `from_dump == from_model` gate is what proves the
hand copy right.

Claim: `phase-52-R3`. Depends on: R1, R2. Owns: `derive/tests/`;
`sched/src/chain_aware_mapper.rs` only to factor the existing snapshot out
for reuse. Gate: `cargo test -p ros-launch-manifest-derive`; `cargo test -p
ros-launch-manifest-sched chain_aware_rank_is_priorityless_and_split_is_parity`.
Status: landed in 13c3f63 (main, 2026-09-21); derive 20+5 passed, 1 ignored
(the pre-R1 empty-rank assertion, until a trigger-less hop is skipped), sched
split-parity 1 passed, workspace green. Snapshot at
derive/tests/snapshots/contract_derived_chain.ranked_plan.txt; R4 may tag.

**R4 - tag v0.1.37 and the CHANGELOG.** This repository has no
`CHANGELOG.md` today and `v0.1.34`..`v0.1.36` are lightweight tags whose
notes are their commit messages. R4 adds the file with a `v0.1.37` entry
naming the six fields, the crate, the `trigger: None` = `Unclassified`
rule and the `rate_monotonic` change, then tags. The tag name is what
play_launch phase-78 W1 writes into its four `tag = "v0.1.36"` pins.

Claim: `phase-52-R4`. Depends on: R3. Owns: `CHANGELOG.md`, the `v0.1.37`
tag. Gate: `cargo test --workspace` green on the tagged commit; play_launch
phase-78 W1 resolves the tag. Status: landed in bfbe075 (main, 2026-09-21); tag v0.1.37 on bfbe075
(annotated); the pre-R1 empty-rank assertion un-ignored in d7e96dd (a
trigger-less hop is skipped, NoPathOnRoute); workspace green, snapshot
unchanged, Cargo version stays 0.1.4. play_launch phase-78 W1 may pin the tag.
Seam found by play_launch phase-78 W2 (07be64dd): `MapperNode::scope` is the
namespace the manual mapper's `[[assign]] scope =` selector matches, while
`derive` copies `NodeInstance::scope`, the file-scope key; fixed consumer-side
for now, the crate should derive the namespace from the FQN.

### Status

~~Open (2026-09-21). Design agreed across the three repositories; no code yet.
Units R1 and R2 are claimable now (Parallel plan above); R3 and R4 follow
in that order.~~

**Superseded 2026-09-23 by the banner at the head of this entry.** All
four units landed on 2026-09-21 and shipped as `v0.1.37`; the issue is
open on the consumer side only. This paragraph is kept so the sequence
is legible — it is what the entry claimed on the day the units were
still unclaimed.

---

## ~~53. Equal Periods, Unequal Priorities~~ - Done

### Problem

`rate_monotonic` gave two 30 Hz nodes priorities 40 and 30, and two 10 Hz
nodes 20 and 10, in a band of 10-40. Nothing in the contract said one
preempts the other: `mapper.rs`'s `build_ranked_plan` handed every RANK its
own tier through `spread_priority(i, n, band)` with `n` the number of NODES,
and the ranking's last tie-break was `a.name.cmp(&b.name)`. So the order
within a tie was the alphabet, and renaming a node changed who preempts whom.
`deadline_monotonic` had the same shape.

Rate-monotonic theory assigns equal periods equal priority. Any fixed order
among them is schedulable, so this was never a correctness bug — but a
DIFFERENT priority is a policy statement, and here nobody made it. It also
spent the band: four nodes at two rates took four of 31 levels where two
would do, which matters once overrides and reservations compete for the same
band. nano-ros consumes the same `SchedPlan`, where distinct priorities
additionally change thread-pool grouping.

The crate's own third mapper already did it the other way. `chain_aware`
collapses items with exactly equal `(criticality, budget)` into one rank
(`tie_group`, unconditionally — not only under band scarcity) and then
DECIDES what a tie means: `SCHED_RR` when the host's global slice is shorter
than the shortest period among the tied nodes, otherwise `SCHED_FIFO` with an
`UnmitigatedPriorityTie` warning naming both numbers. One mapper treated an
exact tie as a fact to preserve and mitigate; the other two silently ordered
it by name.

The existing tests protected the old output by omission:
`rate_monotonic_ties_broken_by_name_asc` asserted only that `/a` came before
`/b` in the output table, which is equally true of a name-ordered spread and
of a collapsed tie.

### Decision

The three mappers agree: an exact tie is a fact to preserve.

- `rank_groups` collapses consecutive nodes whose ranking key is EXACTLY
  equal (`rate_hz`, `deadline_us`) into one rank. Equality is on the value as
  declared — 30 and 30.000001 do not tie, and nothing is rounded into one:
  the collapse states a fact the contract carries, and a tolerance would
  invent one it does not.
- `spread_priority`'s `n` is now the number of DISTINCT values, so the band
  is spent on facts rather than on nodes.
- A rank with more than one node is ONE tier carrying all of them as
  `members`, in node-name order. It names itself after the shared fact
  (`rate_hz=30`, `deadline_us=5000`), never after one member — a tier named
  `/a` holding `/a` and `/b` would read as a tier holding only `/a`. A
  one-node rank still names itself after its node, which is the shape every
  consumer has seen since the mapper existed. The consumer explodes grouped
  tiers into one tier per member before applying overrides
  (`flatten_to_one_tier_per_node`), so a multi-member tier is transparent
  downstream.
- The tie is then handed to `chain_aware`'s own decision. `rr_policy_for_ties`
  took a `&MapperInput` and read each node's period off its declared paths;
  it now takes a `&dyn Fn(&str) -> Option<u64>` period lookup, because WHERE
  that fact lives differs per mapper — `rate_monotonic` ranks on `rate_hz`
  (period = `1/rate_hz`) and `deadline_monotonic` on `deadline_us` (the
  implicit-deadline assumption), and neither populates `paths` at all. One
  function, three callers, one answer to "what does a tie mean".
- `rate_monotonic` and `deadline_monotonic` therefore override
  `map_with_diagnostics` (they emit warnings now; `details` stays empty —
  per-rank provenance for these two is not in scope here).

Two consequences worth stating, because neither is forced by the issue:

- A tie created by BAND COMPRESSION takes the same decision. Five distinct
  rates in a three-level band collapse adjacent ranks — a tie the mapper
  produces rather than derives — and judging it by a different rule would be
  a third policy. `chain_aware` already treats it as a tie after its own
  compression. Practical effect: on a platform file that states
  `rr_timeslice`, a band-compressed `rate_monotonic` plan can now carry
  `SCHED_RR` where it carried `SCHED_FIFO`; where the slice does not fit, a
  warning appears that was previously absent. Pinned by
  `rate_monotonic_band_compression_ties_are_ties_too`.
- The RR decision is expressed in `sched_class` only. These two mappers have
  never written the typed `posix` placement (`ResolvedTier::posix` stays
  `None`, as before), and filling it here would change how the consumer reads
  every `rate_monotonic` tier — `derive_reservations` and
  `report_jitter_placement` both test `tier.posix` for real-time-ness — which
  is a separate decision from this one.

Absent facts stay absent: a node with no usable period makes
`shortest_period_us` `None`, and RR is declined and reported rather than
derived from a number nobody stated. A non-positive or non-finite `rate_hz`
is not a period.

Docs: `docs/scheduling.md` (the two mapper descriptions and the diagnostics
list). Tests: `rate_monotonic_equal_rates_share_one_tier_and_priority`,
`deadline_monotonic_equal_deadlines_share_one_tier_and_priority`,
`rate_monotonic_spreads_over_distinct_rates_not_nodes`, and the three RR
cases beside them.

---

## ~~54. Three In-Crate Rules Ignored the Escape Hatches the Grammar Offers~~ — Done

**Status (2026-09-23): holds; its doc follow-ups have landed.** The
closing paragraph below names three documents still saying "20 rules"
and listing a `chain-shape` rule. All three now say 19 and none lists a
chain rule: `README.md`, `docs/contract-verification.md` §Rule Registry
(where `consistency` has moved to the cross-scope table), and
`docs/launch-manifest.md` §rule table. `docs/README.md` was the last
holdout and was corrected on this date. One stale mention remains
outside those three: `docs/scheduling.md` still calls
`scope-sampling-feasibility` by its retired name
`chain-sampling-feasibility`. The deliberate NOT-done — merging
`service-wiring` into `dangling-entity` — is still not done.

### Problem

Three asymmetries in `check/src/rules/`, each visible by reading the rule
beside the type it consumes.

1. `dangling-entity` read `svc.external` and `act.external` (through
   `server_is_external`) and NOT `topic.external`, so a topic marked
   `external: pub` still got "has no publishers (no data source)". The
   consumer's cross-scope re-run of the SAME rule honoured the mark, so one
   pass warned and the other did not, on the same file.
2. `service-wiring` built its `served` set from services with a non-empty
   `server:` list, so `external: server` — the exact case
   `dangling-entity`'s own header calls normal, and which that rule exempts —
   still warned. Two rules in one registry disagreed about whether a
   client-only manifest is fine.
3. `consistency` was a registered rule whose body was a comment about
   "phase 34.5", counted in the documented registry. A reader of the registry,
   or of a `--rule consistency` run, was told a rule ran that did nothing.

(1) and (2) pushed authors toward the two workarounds `dangling-entity`'s
header explicitly calls out as bad: declaring a server or publisher the image
does not run, or leaving the entity out of the contract.

### Decision

1. The topic branch is symmetric with the service and action branches:
   `external: pub | both` excuses a missing publisher, `sub | both` a missing
   subscriber, and the side that is NOT named still warns — the mark answers
   for the side it names, never `external.is_some()`. The manifest-level
   `external_topics:` block is read as the same fact, since it is the other
   spelling of it. That lookup normalises a leading slash and nothing else:
   a `topics:` key may be written relative to the declaring scope, and
   resolving that needs a namespace this crate never sees (FQN resolution is
   the consumer's).
2. `service-wiring` treats `external: server | both` as served, through the
   same `server_is_external` predicate `dangling-entity` uses.
3. `consistency` is unregistered and its file deleted; the registry holds 19
   rules. The ID stays live — it is what the consumer's CROSS-SCOPE
   consistency rule emits under, and `--rule consistency` filters those
   diagnostics — so nothing that reads the id breaks. A reserved id belongs
   in the docs, not in `default_rules()`.

NOT done, deliberately: merging `service-wiring` into `dangling-entity`'s
service branch. They do answer the same question from two ends — one walks
`cli:` endpoints, the other walks `services:` entries — and the merge is
worth doing, but it changes which rule id a diagnostic arrives under, which
is a user-visible change for `--rule` filters and deserves its own change.

Docs owned elsewhere still say "20 rules" and list a `chain-shape` rule that
is no longer registered (`README.md`, `docs/contract-verification.md`
§Rule Registry, `docs/launch-manifest.md` §rule table): those need the count
corrected to 19 and the `consistency` row moved to the cross-scope list.

Tests: `test_dangling_topic_external_pub_is_accepted`,
`..._external_sub_is_accepted`, `..._external_both_covers_either_side`,
`..._external_sub_still_needs_a_publisher` (the negative control),
`..._external_topics_block_is_accepted`,
`test_service_wiring_external_server_is_served`,
`test_service_wiring_external_client_still_warns`,
`test_registry_has_no_placeholder_rules`.

---

## Summary

Design issues 1–51, #53 (equal periods, equal priorities) and #54 (the
checker's escape hatches) are resolved. #52 (one shared derivation of
the mapper input) is open **on the consumer side only** — its four
units in this repository landed on 2026-09-21 and shipped as `v0.1.37`.

Four resolved entries have since been overtaken, and their status lines
say so:

| # | Was | Now |
|---|---|---|
| 29 | `exclude_patterns` semantics documented | The key was removed (phase 70); `external:` replaces it |
| 31 | `correlation: latest` stamp specified | The key was removed (phase 70); `sync:` states the policy |
| 45 | `qos-match` v1: reliability + durability | `liveliness` and `lease_duration` added (phase 70); QoS also APPLIED (phase 74) |
| 48 | A periodic path is `input: []` | Vocabulary v2's explicit `trigger: { timer: … }`; an empty `input:` no longer means a clock |
| 50 | `min_latency_ms` removed as unmotivated | **Reversed** — `min_latency` is back, and `jitter-range` reads it |

The summary table below preserves the most recent phases.

**Recently resolved** (Phase 34/35):

| #  | Issue                               | Resolved in |
|----|-------------------------------------|-------------|
| 18 | Per-rule CLI filter                 | Phase 34.8  |
| 22 | Drop composition assumes independence | Phase 34 (runtime-only) |
| 23 | Age on subscriber endpoints         | Phase 34    |
| 29 | `exclude_patterns` override         | Phase 34    |
| 30 | Example error messages              | Phase 34    |
| 31 | `correlation: latest` stamp         | Phase 34    |
| 32 | Capture mode doc location           | Phase 34    |
| 33 | Topic keys as ROS names             | Phase 34    |
| 34 | Scope paths use topic names         | Phase 34    |
| 35 | Parent manifest purpose             | Phase 34    |
| 37 | Absolute name verbosity (accepted)  | Phase 34    |
| 41 | Services follow topic pattern       | Phase 34    |
| 42 | Topology-aware budget check         | Phase 35.1–35.4 |
| 43 | Scope path tracing algorithm        | Phase 35.1–35.4 |
| 46 | Node naming guidance                | Phase 34    |
| 47 | Inline include example              | Phase 34    |
| 48 | `header.stamp` as convention        | Phase 34    |
| 49 | Lifecycle node `lifecycle:` flag    | Phase 35 (post-35.8) |
| 44 | `max_transport_ms` per-sub override | Phase 35.9  |
| 45 | QoS pub/sub `qos-match` rule        | Phase 35.9  |
| 50 | `min_latency_ms` removed            | Phase 34    |
| 51 | External topics (`external_topics:` + per-topic `external:` flag) | Phase 35.10 |
