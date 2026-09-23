# Contract Verification — Implementation

How manifest contracts are verified, as implemented by the `types/` and
`check/` crates in this workspace. For the manifest format see
[launch-manifest.md](launch-manifest.md); for the formal foundations see
[contract-theory.md](contract-theory.md).

> **Scope note.** This document describes the *per-manifest* checker that
> lives in this repository. Cross-file checks (cross-scope declaration
> agreement, cross-scope QoS reconciliation, the topology-aware
> critical-path budget check, route derivation for a scope path) run in
> the consumer's merge layer (`ros-launch-resolve`, invoked by
> `play_launch check`) because they need the merged launch tree. So does
> everything that DERIVES a consequence and grades a declaration against
> it — topic and endpoint rates, node criticality, the FDTI/FRTI
> arithmetic — for the same reason: the graph those walk spans files. The
> arithmetic itself is stated in
> [contract-theory.md](contract-theory.md#derived-quantities). See
> [Division of Labor](#division-of-labor-with-the-consumer).

## Pipeline

```
YAML file ──→ parse with spans ──→ Manifest AST ──→ build DataflowGraph ──→ run rules ──→ Diagnostics ──→ emit
              (types/, yaml-rust2)  (types::Manifest)  (check/src/graph.rs)   (check/src/rules/)            (terminal | codespan)
```

1. **Parse** (`types/src/parse.rs`) — a hand-rolled deserializer over
   plain `yaml-rust2` values (Serde is used for *serialization* only).
   Spans come from a second pass: `SpanIndex::build` re-scans the source
   with the event parser's `Marker`s and produces a YAML-path → byte-range
   index (`types/src/span.rs`) that diagnostics resolve against.
   Entry points: `parse_manifest`, `parse_manifest_str`, and the
   `*_with_spans` variants returning
   `ParseResult { manifest, source, spans }`.
2. **Filter & substitute** (`types/src/`) — `evaluate_condition` /
   `filter_manifest` apply `if:`/`unless:` conditions for a given arg
   assignment; `resolve_args` / `substitute_manifest` perform `$(var)`
   substitution.
3. **Graph** (`check/src/graph.rs`) — a `petgraph::DiGraph<GraphNode,
   GraphEdge>` over manifest nodes and include scopes, with edges from
   topic publisher → subscriber. Subscribers tagged `state: true` are
   skipped — that is how declared feedback loops break the causal cycle
   check.
4. **Rules** (`check/src/rules/`) — each rule is an independent module
   implementing:

   ```rust
   pub trait ValidationRule: Send + Sync {
       fn id(&self) -> &str;
       fn check(&self, manifest: &Manifest, graph: &DataflowGraph, ctx: &mut CheckContext);
   }
   ```

   `run_checks` / `run_checks_with_spans` (`check/src/check.rs`) build the
   graph, then run `rules::default_rules()` in registration order.
5. **Diagnostics** — rules emit into `CheckContext` (`emit` / `error` /
   `warning`), producing:

   ```rust
   pub struct Diagnostic {
       pub rule_id: String,
       pub severity: Severity,        // Info | Warning | Error
       pub message: String,
       pub path: String,              // YAML path, e.g. "topics.pointcloud.qos"
       pub span: Option<Range<usize>>, // resolved from SpanIndex when available
   }
   ```

## Why manual parsing (not Serde deserialize)

Serde's data model has no concept of source locations — by the time a
struct is populated, span information is gone. The manual `yaml-rust2`
layer keeps a `SpanIndex` from YAML path to byte range, so any rule can
point a diagnostic at the exact offending line, including in multi-file
output.

## Rule Registry

19 rules, in registration order (`check/src/rules/mod.rs::default_rules`).
Severity is what the rule emits; several rules emit at more than one
severity.

The registry is exactly these 19. **`consistency` is deliberately not one
of them**: the id is live, but it belongs to the cross-scope rule the
consumer emits ([below](#division-of-labor-with-the-consumer)), and
`--rule consistency` filters those diagnostics. What used to sit here was a
no-op body reserving the name, counted in this registry — so a reader of
the table, or of a `--rule consistency` run, was told a rule ran that did
nothing. It was removed in v0.1.38; a reserved id belongs in the docs, not
in `default_rules()`.

| # | Rule | Severity | What it catches |
|---|------|----------|-----------------|
| 1 | `endpoint-unique` | Error | Duplicate endpoint name across a node's pub/sub/srv/cli |
| 2 | `wiring` | Warning | Path input/output endpoint not wired by any topic |
| 3 | `qos-compat` | Error | Invalid QoS value token (`reliability`, `durability`, `history`, `liveliness`) at topic or endpoint level |
| 4 | `qos-match` | Error / Warning | Structural: `depth: 0` (E), `keep_all` with depth (W), `best_effort` + `transient_local` (W). DDS pub/sub compatibility on `reliability`, `durability`, `liveliness` and `lease_duration` (E) — offered ≥ requested, checked only when both sides specify (no implicit ROS defaults) |
| 5 | `rate-hierarchy` | Error | `pub.min_rate_hz < topic.rate_hz`; `topic.rate_hz < sub.min_rate_hz`; and since phase 70 the upper bounds `topic.rate_hz > pub.max_rate_hz` and `topic.rate_hz > sub.max_rate_hz` |
| 6 | `scope-budget` | Warning | Flat conservative sum: scope `max_latency` < Σ node latencies + declared topic `max_transport`, each node contributing the **max** over its declared paths. Per-manifest fallback — the topology-aware critical path is the consumer's cross-scope diagnostic, which deletes this warning for every scope path it resolves a route for |
| 7 | `causal-dag` | Error | Cycle in the causal dataflow graph (`state: true` on feedback endpoints breaks it) |
| 8 | `drop-sanity` | Error | Effective delivery rate < subscriber demand; a `drop.max_count` rate outside [0,1]; `n > w` in `"N / W"`; `max_consecutive == 0`. Checked on topics, scope paths and node paths |
| 9 | `service-wiring` | Warning | Service client with no matching server |
| 10 | `service-type` | Error / Warning | Service without `type` (E); server/client ref not declared on its node (W) |
| 11 | `dangling-entity` | Warning / Error | Topic with 0 pubs or 0 subs (W); service/action with 0 servers (E) — unless that side is declared `external:` |
| 12 | `satisfiability` | Warning / Error / Info | **Z3-backed.** Node unreachable under all valid arg assignments (W); some valid arg assignment produces a dangling entity (E). Skips topics whose subscribers are all state-only. Built without the `smt` feature, a stub with the same id emits one Info saying the analysis was not run — checking less is never silent |
| 13 | `state-consistency` | Warning | Likely-missing `state: true` on a subscriber that is neither state-tagged nor referenced by any path trigger (two noise-gated heuristics) |
| 14 | `explicit-trigger` | Info | Path has no explicit `trigger:` — migration lint toward the Vocabulary v2 taxonomy |
| 15 | `inherited-rate` | Warning | Non-`input` explicit trigger combined with a stale legacy `input:` list |
| 16 | `once-durability` | Warning | `once`-triggered path publishes to a topic whose effective durability is not `transient_local` |
| 17 | `sync-feasibility` | Warning | `sync.max_interval` / `sync.timeout` shorter than the slowest declared input period |
| 18 | `queue-drain-rate` | Warning | Timer path `rate_hz` lower than the summed input rates of its `buffer: queue` subscriptions |
| 19 | `jitter-range` | Error / Info | `min_latency` above `max_latency` (E); `max_latency - min_latency > max_jitter` when both bounds are declared (E); `max_jitter` declared with no `min_latency`, so the bound cannot be checked (Info — an absent floor is unknown, not zero). A `max_latency` at or below `max_jitter` is clean without a floor: whatever it is, the spread cannot exceed the ceiling |

Shared helper: `rules/endpoint_topic.rs` resolves `node/endpoint`
references to their declaring topic (used by `once-durability`,
`sync-feasibility`, `queue-drain-rate`).

### Z3 and satisfiability

Args declared with `type: bool` or `choices:` define a finite
configuration space. The `satisfiability` rule encodes `if:`/`unless:`
conditions as SMT formulas (crate `z3`) and asks, per entity: *is there a
valid arg assignment under which this topic/service ends up with zero
publishers/servers?* Errors carry the witness assignment:
`"topic 'pose' has 0 publishers when pose_source=gnss"`.

Z3 is used only inside this rule — there is no SMT-LIB file output.

## Emitters

`check/src/emit/` has exactly two backends:

- **`terminal`** — plain stderr lines:
  `"{severity}[{rule_id}]: {message} (at {path})"` plus an
  error/warning count summary.
- **`diagnostic`** — `codespan-reporting` rendering with `rule_id` as the
  diagnostic code and the span as a primary label; falls back to an
  `at {path}` note when the manifest was parsed without spans.

Example codespan output:

```
error[qos-match]: incompatible reliability on topic 'pointcloud': pub best_effort < sub reliable
  ┌─ manifests/sensing/sensing.launch.contract.yaml:12:5
  │
12│     reliability: best_effort
  │     ^^^^^^^^^^^^^^^^^^^^^^^^ topics.pointcloud.qos.reliability
```

## Division of Labor with the Consumer

The checker in this repo is deliberately **single-manifest**. Checks that
need the merged launch tree run in the consumer's cross-scope layer —
the `ros-launch-resolve` resolve crate, which `play_launch check`
invokes. Cross-scope rule ids, emitted from
`resolve/src/ros/manifest_loader.rs` and `causal_dag_global.rs`:

| Rule | What it checks |
|------|----------------|
| `manifest-parse` | A contract file that could not be read at all. Counted and reported separately from the per-manifest tallies, because the file it names is absent from the index and would otherwise count as clean |
| `consistency` | Topic/QoS/rate declarations agree across the scopes that declare the same topic (the in-crate no-op of the same id was removed in v0.1.38 — this is the only `consistency` rule) |
| `budget-overflow` | Cross-scope path budgets: a child scope's path budget must not exceed a matched ancestor path's budget (theory doc "Check 1") |
| `scope-budget` | Topology-aware critical path over the merged dataflow DAG, including per-sink `max_transport` overrides (the local flat-sum rule is the standalone fallback) |
| `scope-sampling-feasibility` | The derived route's sampling cost alone already meets the budget — structurally infeasible, so no priority assignment can fix it. Emitted before `scope-budget`, so the structural verdict reads first |
| `jitter-feasibility` | A scope path's `max_jitter` is below the sampling jitter its derived route already carries (one whole period per clock boundary crossed) |
| `sync-budget` | A `sync:` window wider than the path's own `max_latency` — a contradiction in the declaration, not a performance problem |
| `causal-dag-global` | Cycles in the merged graph, including edges derived from launch-file remaps |
| `rate-hierarchy`, `qos-match`, `dangling-entity` | Cross-scope variants of the local rules, run after merge |
| `derivable-rate` / `rate-mismatch`, `derivable-min-rate` / `min-rate-mismatch`, `derived-rate-hierarchy` | A declared rate against the one derived from the timers that drive it: info when they agree, warning when they do not |
| `sync-feasibility` | The local rule's twin on DERIVED rates, run only where an input's rate is derived but not declared. Deleting a `rate_hz` the `derivable-rate` info calls redundant must not silence the window check that reads it |
| `graph-from-remaps` | How many topics and endpoints the graph took from the launch file's own remaps rather than from a contract, and how many remaps were too ambiguous to give a direction (counted, never guessed) |
| `derivable-criticality` / `criticality-mismatch`, `severity-unknown` | A declared `criticality` against the one derived from the hazards reaching the node; a `severity:` outside the declared `severity_levels:` scale |
| `fault-reaction-budget`, `reaction-unreachable`, `reaction-within`, `reaction-unbudgeted`, `reaction-unguarded`, `hazard-unguarded` | FDTI + FRTI against a hazard's `ftti`, and the structural preconditions for deriving them |
| `ladder-rung-budget`, `ladder-unterminated`, `mode-requires-unguarded`, `override-target-missing` | Operational modes: each fallback rung judged against the ftti in its own right, a floor that requires nothing losable, and override targets that name a real contract path |
| `lifespan-age`, `response-blocking`, `concurrency-decl`, `path-exclusion` | Declarations that contradict each other across the merged tree |

Two properties of that layer are worth stating, because neither follows
from the table. **Most rules have nothing to say without a declared
requirement, but some do**: `causal-dag-global` and `graph-from-remaps`
run even when the tree carries no manifest at all, because a cycle is a
defect whether or not anyone wrote a budget. And a mode with `overrides:`
makes the requirement checks **run again**: the pass clones the index,
applies the overrides to the declaration and to the resolved copies the
checks read, re-runs them and diffs against the default, reporting only
what that mode introduces under `mode:<rule>`.

Runtime monitors live in play_launch's interception layer (Phase 29), fed
by `rcl_publish`/`rcl_take` and the DDS QoS events: `drop-rate-runtime`,
`rate-hierarchy-runtime`, `max-age-runtime`, `max-latency-runtime`,
`qos-match-runtime`, `consistency-runtime`, `graph-deviation-runtime`,
`deadline-runtime`, `liveliness-runtime`, and — since phase 73 — the
hazard rules `hazard-detected`, `hazard-reaction`, `hazard-recovered` and
`mode-availability`.

Invocation:

```bash
# Check the manifests of a launch tree (merged, cross-file checks included)
play_launch check <pkg> <launch_file>

# Filter to one rule, JSON output
play_launch check --rule qos-match --format json <pkg> <launch_file>

# Scheduling plan check with per-node provenance
play_launch check --sched <platform.yaml> --explain <pkg> <launch_file>
```

## Crate Choices (as shipped)

| Concern | Crate | Notes |
|---------|-------|-------|
| YAML parsing with spans | `yaml-rust2` | `MarkedYaml` gives line/col per node; converted to byte offsets |
| Diagnostic rendering | `codespan-reporting` | Multi-file, FileId-based |
| Graph analysis | `petgraph` | Dataflow DAG, cycle detection |
| Satisfiability | `z3` | Finite arg-space checking in the `satisfiability` rule |

Ideas from earlier drafts of this document that were **not** built: a
`Constraint`/`ConstraintKind` intermediate representation, a formal-notation
emitter, SMT-LIB output, `good_lp` budget optimization, and RTLola
monitors. The rule → `Diagnostic` path proved sufficient; revisit only
with a concrete need.
