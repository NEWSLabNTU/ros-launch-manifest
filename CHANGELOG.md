# Changelog

Release notes for `ros-launch-manifest`, newest first. The tag is the
release: play_launch and nano-ros pin this repository by tag, and the
workspace's Cargo version moves only when a crate's API breaks. Tags before
`v0.1.37` are lightweight and their notes are their commit messages
(`git show v0.1.36`).

## v0.1.38 - 2026-09-22

Three issues filed against `ea5cbea` from the 2026-09-18 safety-island runs,
tracked in play_launch's tracker (`docs/issues/0037`, `0038`, `0039` there —
this repository has none of its own). The workspace Cargo version stays
`0.1.4`: `mod consistency` was private, so the rule's removal is not an API
change, and `rr_policy_for_ties` is `pub(crate)`.

### The checker honours the escape hatches the grammar offers (#0037)

Three rules ignored `external:`, each checkable by reading the rule beside the
type it consumes:

- `dangling-entity` warned on a topic with no publishers or no subscribers
  without reading `topic.external`, while the SAME rule read `svc.external`
  and `act.external` through `server_is_external`. The topic branch now skips
  the no-publishers warning for `pub | both` and the no-subscribers warning for
  `sub | both`, through a `topic_side_is_external` shaped like its service
  twin, and also reads the manifest-level `external_topics:` block. Key
  matching there is exact plus leading-slash normalisation: a `topics:` key may
  be relative, and resolving that needs a scope namespace this crate never
  sees.
- `service-wiring` warned for a `cli:` endpoint whose service was marked
  `external: server` — the exact case `dangling-entity`'s own header calls
  normal and exempts. Both rules now agree, through one `has_a_server`.
- `consistency` was a registered no-op whose body was a comment about phase
  34.5, counted in the documented 20 rules. Removed; the registry is 19. The
  id stays live because the real cross-scope rule is the consumer's, which
  emits it from seven sites in `manifest_loader.rs`, and `--rule` is a free
  string list, so `--rule consistency` still filters real diagnostics.

Eight tests, including two negative controls (`external: sub` still demands a
publisher; `external: client` still warns) and a registry test pinning 19
rules with unique ids. Verified non-vacuous: with `check/src/rules` stashed, 6
of the 8 fail and the 2 controls pass.

### Equal periods, equal priority (#0039)

`rate_monotonic` sorted by rate and broke ties by NODE NAME, giving two 30 Hz
nodes priorities 40 and 30 — a policy statement (this node preempts that one)
whose policy was the alphabet, so renaming a node changed who preempts whom.
The crate's own `chain_aware` already did the opposite: an exact tie collapses
into one rank and then takes a `SCHED_RR`-if-the-slice-fits decision, warning
`UnmitigatedPriorityTie` where FIFO cannot be made fair. The three mappers now
agree.

`rank_groups` collapses consecutive nodes with exactly equal `rate_hz` /
`deadline_us` into one group; the spread runs over DISTINCT values, and one
tier per rank carries all tied nodes as sorted `members`. `chain_aware`'s
decision is reused rather than reimplemented: `rr_policy_for_ties` now takes a
period closure instead of `&MapperInput`, which was necessary rather than
cosmetic — it read each node's period off `node.paths`, which the two simple
mappers never populate, so they would have seen `None` on every tie and could
never have derived RR.

Consequences worth knowing downstream:

- A multi-member tier names itself after the shared fact (`rate_hz=30`); a
  one-member tier still names itself after the node, so nothing moves for
  untied plans. play_launch's `flatten_to_one_tier_per_node` explodes grouped
  tiers before applying overrides.
- Band-compression ties take the same decision, deliberately. Five distinct
  rates in a three-level band already produced equal priorities, silently
  FIFO; judging a mapper-created tie by a different rule than a derived one
  would be a third policy. On a platform file that states `rr_timeslice`, such
  a plan can now carry `SCHED_RR` where it carried `SCHED_FIFO`, and where the
  slice does not fit a warning appears that was previously absent.
- `ResolvedTier::posix` stays `None` for these two mappers, so RR is expressed
  in `sched_class` only. Filling the typed placement would be more consistent
  with `chain_aware`, but play_launch's `derive_reservations` and
  `report_jitter_placement` both test `tier.posix` for real-time-ness, so every
  `rate_monotonic` tier would become reservation-eligible and stop producing
  jitter warnings.

`deadline_monotonic` had the identical test protected by the same omission
(the issue named only `rate_monotonic`) and got the same fix.
`derive/tests/snapshots/contract_derived_chain.ranked_plan.txt` does not move:
it snapshots `chain_aware_rank`, which this does not touch.

### The prose says what the grammar accepts, and a test keeps it that way (#0038)

`docs/format-reference.md` is generated and was correct; the hand-written docs
taught `max_drop_rate`, topic-level `max_consecutive`, `_ms`/`_us` spellings,
the removed scope-interface blocks and the deleted chain rules — all parse
errors or deletions at HEAD. A contract author copying the first example under
"drops" or "timing" got a parse error whose hint named a key the doc never
showed.

`sched/tests/docs_yaml.rs` feeds every fenced yaml block in `README.md` and
`docs/*.md` to the parser that owns it (a top-level `target:`/`mapper:` makes
it a platform file, everything else a contract) — 40 blocks, 38 parse, 1
marked `expect-error` (the `chains:` migration example), 1 marked `skip` (a
shape sketch whose body is an ellipsis). Markers are HTML comments carrying a
required reason. This is the part that keeps the prose from drifting again.

Two things the test found that no issue listed:

- **`if:` on an include is a parse error in both forms**, and `IncludeDecl`
  has no field to store a condition while `filter_manifest` never filtered
  includes — so the documented claim that "the include entry exists to carry
  per-child conditions" was never true. The examples and the prose now say what
  the grammar does; whether per-child include conditions SHOULD exist is a code
  question, not a doc one.
- **Node paths do accept `drop:`** — the schema allows it, `drop-sanity`
  checks it, and two fixtures author it, against the doc's "node paths have
  latency only".

`cargo test --workspace`: 551 passed, 0 failed.

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
