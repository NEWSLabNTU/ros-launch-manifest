# Changelog

Release notes for `ros-launch-manifest`, newest first. The tag is the
release: play_launch and nano-ros pin this repository by tag, and the
workspace's Cargo version moves only when a crate's API breaks. Tags before
`v0.1.37` are lightweight and their notes are their commit messages
(`git show v0.1.36`).

## v0.1.37 - 2026-09-21

Design issue #52 (`docs/design-issues.md`): one derivation of the
scheduling mapper's input for both consumers. Before this release
play_launch (`sched_derive.rs`) and nano-ros (`mapper_input.rs`) each built
the `chain_aware` mapper's `MapperInput` from their own view of the system
and disagreed on the trigger, the chains, the criticality,
`claims_concurrency`, `rate_hz`, `deadline_us` and `exec_ms`. The model now
carries every fact the checker resolves per entity, and one function in
this repository derives the mapper's input from it. Landed as four units:
R1 `9563b54`, R2 `2367972`, R3 `13c3f63`, R4 (this release).

The workspace Cargo version stays `0.1.4`. Every model change below is
additive, `SCHEMA_VERSION` stays 1, an older model parses with the new
fields absent, and no manifest key was added (the format reference and the
`types` field table are untouched). The new crate is the fifth workspace
member; nothing that existed changed its signature.

### Model (R1): the checker's facts, per entity

`ros-launch-manifest-model`, all `Option`al or empty by default, absent from
the wire form when unset:

- `PathContract.trigger: Option<sched::EffectiveTrigger>`, the value of
  `PathDecl::effective_trigger()` at resolve time, serialized in the
  adjacent `kind`/`value` shape the `sched` crate already used
  (`{ kind: timer, value: { rate_hz: 100 } }`, `{ kind: input, value:
  [..] }`, `once`, `spontaneous`, `unclassified`).
  `PathContract::effective_trigger()` reads it.
- `PathContract.sync: Option<SyncContract { policy: exact | approximate |
  timeout_any, max_interval_ms, timeout_ms }>` and
  `PathContract.min_latency_ms: Option<f64>`.
- `SubContract.buffer: Option<BufferContract>`, `latest` | `queue`, the
  discipline of a `state: true` subscription.
- `Contracts.severity_levels: Vec<String>` (empty = the ISO 26262 default)
  and `Contracts.node_criticality: BTreeMap<String, sched::Criticality>`,
  the EFFECTIVE criticality after the hazard derivation (hazards decide
  first, the label where none reaches), keyed by node FQN. The advisory
  string on `NodeInstance.criticality` stays for dashboards; the mapper
  reads the map.
- `PathContract.input` no longer documents "Empty = periodic (timer-driven)".
  That convention was retired with Vocabulary v2 in the `types` crate; the
  trigger fact is the field above. The rule for a model without one:
  `trigger: None` reads as `Unclassified`, never as a timer, so a model
  resolved by a pre-`v0.1.37` play_launch ranks nothing, loudly, instead of
  something wrongly.
- `Contracts::is_empty` now also counts `node_concurrency`, which it had
  missed.

The golden model (`model/tests/golden/perception.system_model.yaml`) gains a
100 Hz timer path carrying every per-path fact, and a test loads the golden
text with all six keys stripped to prove the pre-migration form still
parses.

### The `derive` crate (R2): one function from the model to the mapper

`ros-launch-manifest-derive`, depending on `model` and `sched` (the reverse
is impossible: `model` already depends on `sched`), so `sched` stays
parser-free and `model` stays a schema. Its API:

- `mapper_input_from_model(&SystemModel, &DeriveFacts) -> (MapperInput,
  DeriveReport)`: one `MapperNode` per `structure.nodes` entry, paths from
  `contracts.node_paths` (trigger, `max_latency_ms`, `max_jitter_ms`,
  `miss`, inputs, outputs); `criticality` from `node_criticality` first and
  the parsed label second; `rate_hz` = the fastest `Timer` trigger among
  the node's paths and nothing else; `deadline_us` = min over path
  `max_latency_ms` and `srv_endpoints.max_response_ms`;
  `claims_concurrency` by play_launch's merged-group rule (the one that
  models an executor; it differs from nano-ros's old rule on `[[a, b],
  [c]]` over `{a, b, c}`); `legacy` left `None` for the `.toml` bridge.
- `resolve_chains(&SystemModel, &DeriveFacts) -> Vec<ResolvedChain>`:
  play_launch's `manifest_graph::{build_global_graph,
  subgraph_for_scope_path, critical_path}` ported from `ManifestIndex`
  onto the model (`structure.topics`, `node_paths` inputs and outputs,
  `sub_endpoints.state` to break cycles, `structure.scopes` for the
  subtree, `scope_paths` for the budget). One chain per scope path with a
  `max_latency_ms`; timer paths are boundaries at `period_ms = 1000 /
  rate_hz`, the rest segments; criticality is the max over members.
- `DeriveFacts { path_exec_ms: BTreeMap<"<node FQN>/<path>", f64>,
  node_exec_ms: BTreeMap<"<node>", f64> }` and
  `DeriveFacts::exec_ms_for`: the consumer's cost facts, the one input the
  model does not carry. play_launch fills `node_exec_ms` from the platform
  file's `budget_us`, nano-ros fills `path_exec_ms` from its `[wcet]`
  profile. A node budget is attributed only when the node has exactly one
  path, and the same rule applies to `MapperPath::exec_ms` and to chain
  boundaries. No WCET is invented.
- `DeriveReport { paths_without_trigger, chains_resolved, chains_skipped:
  Vec<SkippedChain { scope_path, reason: ChainSkip }> }`, `ChainSkip` being
  `NoBudget | NoEndpoints | Cycle | NoRoute | NoPathOnRoute`, so a consumer
  can say which paths a stale model left unclassified and why a scope path
  produced no chain, instead of silently ranking nothing.
- `derive::view::ModelView` (the model as the derivation reads it) and
  `derive::graph` (the causal graph, subgraph and critical path) are public
  for consumers that explain a route; `parse_criticality_label` is
  re-exported.

Authored `topics.<t>.rate_hz` and `pub.<ep>.min_rate_hz` are derive-only
for the scheduler from here on: runtime promises the monitors read and the
resolver keeps checking against the timers (`rate-mismatch`,
`min-rate-mismatch` stay warnings; `derivable-rate`, `derivable-min-rate`
stay infos), but no mapper reads them. The one visible change is to
`rate_monotonic`: a node with no timer path has no rate to be monotonic
about and lands on the default tier, where `chain_aware` already put it.

### Parity (R3): the fixture and the golden snapshot

- `derive/tests/fixtures/contract_derived_chain.system_model.yaml`:
  play_launch's `contract_derived_chain`, resolved by the 0.10.0 binary
  (identical to 0.11.0's resolution for this launch) with R1's fields
  filled by hand from the contract; its header lists the four hand edits.
  play_launch phase-78 W1 re-emits it and phase-78 W2's `from_dump ==
  from_model` gate is what proves the hand copy right.
- `derive/tests/snapshots/contract_derived_chain.ranked_plan.txt`: the
  `{:#?}` text of `chain_aware_rank(&mapper_input_from_model(fixture, no
  facts))`, compared byte for byte by `derive/tests/parity.rs`
  (`UPDATE_RANKED_PLAN_SNAPSHOT=1` re-takes it). Both consumers' gates
  compare against this text: control > filter (one segment) > sensor tick
  (the 100 Hz boundary), one `ChainFeasibleWithoutWcet` naming the
  boundary. Authored rates added at 7 Hz to every topic and publisher, and
  then removed, rank to the same text and the same `MapperInput`.

Two seams R3 found that the consumers must reconcile when they pin this
tag:

1. The scope-path key. The model documents `contracts.scope_paths` keys as
   `"<scope id>/<path name>"` (`bringup.launch.xml/points_to_cmd`), and
   `resolve_chains` splits the key at its last `/` to find the scope.
   play_launch 0.11.0's resolver writes `/bringup.launch.xml/points_to_cmd`
   (`model_builder::fqn` prepends a slash the scope id does not carry), and
   read as emitted the derivation finds no scope `/bringup.launch.xml` and
   resolves no chain. The fixture is normalised to the documented shape;
   play_launch phase-78 W1 must emit that shape (or the model's doc must
   change, which it did not here).
2. The one-path budget rule on a multi-path timer node. play_launch's
   `sched_derive.rs` gave a node's `budget_us` to `MapperPath::exec_ms`
   only when the node has one path, but gave it to the chain BOUNDARY
   whatever the path count. The crate applies the one-path rule in both
   places: on the fixture with a second timer path on the sensor and a 2 ms
   node budget, play_launch counted 12 ms of sampling cost with no warning;
   the derivation counts 10 ms, says so (`ChainFeasibleWithoutWcet`), and
   reaches 12 ms only from a per-path fact
   (`DeriveFacts::path_exec_ms`). A consumer that wants the old number
   supplies the per-path fact.

### Fixed (R4)

- `chains_from_view` linked a hop whose declared path carries no trigger
  fact as a segment member, so a pre-R1 model ranked one item where the
  design says it ranks nothing (a hop is attributed to a path by the
  output it publishes, which needs no trigger). Such a hop is now skipped
  like an undeclared node; a route left with no classified hop is reported
  as `ChainSkip::NoPathOnRoute`. The snapshot is unchanged.

### Migration order

rlm ships this tag; play_launch pins `v0.1.37`, lowers the new fields,
lands parity test 2 and deletes its derivation, ships as 0.12.0; nano-ros
bumps both pins, replaces `mapper_input.rs` with a `DeriveFacts` builder and
one call, lands parity test 3 and re-resolves its models. Until a
consumer's pin moves, a model it resolved carries no `trigger`, and the
shared function ranks nothing on it; `DeriveReport.paths_without_trigger`
is what makes that visible.
