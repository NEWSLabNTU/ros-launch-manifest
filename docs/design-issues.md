# Design Issues

Open design questions for the manifest format, with proposed solutions.

## Resolved Issues

Issues resolved in prior phases, preserved in git history:

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
- **33**: Topic keys as ROS names — Done (docs). Resolves #7, #19,
  #20, #21, #26, #27. Code pending
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

## 17. Cross-Scope Service Wiring Has No Suppression Mechanism

### Problem

Cross-scope service wiring has no enforcement path:

1. `mrm_handler` declares `cli:` with 3 cross-scope service clients
2. `mrm_comfortable_stop_operator` declares `srv:` with 1 server
3. No parent manifest exists to wire them in a `services:` section
4. The `service-wiring` rule warns about orphan `cli:` endpoints
5. There is no way to mark these as "expected cross-scope" to suppress

This creates **permanent noise** in check output — 3+ warnings per run
that can never be resolved without a parent manifest.

### Options

| Option                        | Effort | Description                                              |
|-------------------------------|--------|----------------------------------------------------------|
| A. `# nolint: service-wiring` | Small  | Inline suppression comment                               |
| B. `--suppress` CLI flag      | Small  | Suppress specific rules/paths                            |
| C. Cross-manifest wiring      | Large  | Checker loads multiple manifests and wires across scopes |
| D. Accept the noise           | None   | Document as known limitation                             |

**Recommendation**: Option A or B. A lightweight suppression mechanism
would benefit other expected-warning scenarios too (e.g., unwired scope
interface endpoints).

---

## ~~18. CLI Should Support Per-Rule Filtering~~ — Done

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

Resolved by moving `max_age_ms` from scope paths to **subscriber
endpoints**. Age is now a data freshness constraint at the point of
consumption (`now - header.stamp` at `rcl_take`), checked at runtime
by the interception layer. No chain tracing needed. Static checking
verifies local consistency (age budget vs known latency budget) but
doesn't attempt a full chain proof.

---

## ~~29. `exclude_patterns` Override Behavior Undocumented~~ — Done

Documented: user declaration **replaces** defaults. `exclude_patterns: []`
includes all topics.

---

## ~~30. No Example Error Messages for Validation Rules~~ — Done

Added example diagnostics block to the Static Validation section
covering 7 rules: `endpoint-unique`, `wiring`, `rate-hierarchy`,
`budget-overflow`, `dangling-entity`, `consistency`, `satisfiability`.

---

## ~~31. `correlation: latest` Output Timestamp Unspecified~~ — Done

Specified: `correlation: latest` output stamp = **primary (first listed)
input's stamp**. Based on analysis of 9 Autoware fusion nodes — 7 of 9
propagate the primary input's timestamp. Secondary inputs enrich the
data but don't determine the timestamp. Age follows the primary branch
only. Updated launch-manifest.md and contract-theory.md (parallel
composition section + summary table).

---

## ~~32. Capture Mode Buried in Theory Appendix~~ — Done

Added "Generating Manifests from a Running System" section to
launch-manifest.md with `--save-manifest-dir` usage, what it generates,
and a link to the statistical derivation in contract-theory.md Appendix C.

---

## 33. Topic Keys as ROS Topic Names

Resolved (docs). Resolves #7, #19, #20, #21, #26, #27. Code pending.

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

## ~~17. Cross-Scope Service Wiring Has No Suppression Mechanism~~ — Resolved by #41

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

## ~~43. Scope Path Dataflow Tracing Underspecified~~ — Done

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

## 44. `max_transport_ms` Ambiguous for Multi-Subscriber Topics

### Problem

`max_transport_ms` is declared per topic, but a single ROS 2 topic can
have subscribers with different transport characteristics — one on the
same machine via shared memory (< 1ms), another across a network bridge
(5-10ms). A single value per topic can't express this.

### Options

| Option | Pros | Cons |
|--------|------|------|
| A. Worst-case across all subscribers | Simple | Pessimistic for same-machine subs |
| B. Per-subscriber transport override | Precise | More complex format |
| C. Document as worst-case | No format change | Users must declare the worst case |

---

## 45. No QoS Publisher-Subscriber Compatibility Check

### Problem

The `qos-compat` rule validates that QoS field values are from the
allowed set (e.g., `reliability: maybe` → error). But it does not
check **publisher-subscriber QoS compatibility** — one of the most
common ROS 2 deployment bugs:

- `best_effort` publisher + `reliable` subscriber → **incompatible**
  (no data flows)
- `volatile` publisher + `transient_local` subscriber →
  **incompatible** (late-joining subscriber misses data)

The manifest declares QoS on topics and the checker merges across
scopes. After merge, the checker has both publisher and subscriber QoS
— it should validate compatibility using the ROS 2 compatibility
matrix.

### Fix

Add a `qos-match` rule that checks publisher QoS against subscriber
QoS on merged topics. The ROS 2 compatibility rules:

| Publisher | Subscriber | Compatible? |
|-----------|-----------|-------------|
| reliable | reliable | Yes |
| reliable | best_effort | Yes |
| best_effort | reliable | **No** |
| best_effort | best_effort | Yes |
| transient_local | transient_local | Yes |
| transient_local | volatile | Yes |
| volatile | transient_local | **No** |
| volatile | volatile | Yes |

---

## ~~46. No Guidance on Manifest Node Naming~~ — Done

Added to §Nodes: manifest node name must match the ROS 2 node name
(`name=` attribute / `__node:=` remap, as shown in `ros2 node list`).

---

## ~~47. Missing Inline Include Example~~ — Done

Added inline include example to §Includes.

---

## ~~48. `header.stamp` Propagation Stated as Rule but Is Convention~~ — Done

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

## ~~50. `min_latency_ms` Poorly Motivated~~ — Done

Removed from both node and scope path field tables. Not used in any
rule, example, or validation.

---

## Summary

Issues not in this table are either resolved (struck through above)
or superseded by a later issue. Only open issues and their phase
assignments are shown here.

| #  | Issue                               | Type                  | Effort  | Status                                 |
|----|-------------------------------------|-----------------------|---------|----------------------------------------|
| 44 | `max_transport_ms` multi-subscriber | Format design         | Small   | Open — future format extension         |
| 45 | QoS pub/sub compatibility check     | New rule              | Small   | Open — needs per-endpoint QoS (future) |

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
| 50 | `min_latency_ms` removed            | Phase 34    |
