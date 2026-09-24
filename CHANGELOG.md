# Changelog

Release notes for `ros-launch-manifest`, newest first. The tag is the
release: play_launch and nano-ros pin this repository by tag, and the
workspace's Cargo version moves only when a crate's API breaks. Tags before
`v0.1.37` are lightweight and their notes are their commit messages
(`git show v0.1.36`).

## v0.1.41 - 2026-09-24

An include can carry a condition, and the conditions inside one are evaluated.
**Workspace Cargo version `0.1.4` -> `0.1.5`**: `IncludeDecl` changes shape,
which is an API break for anything that matches on it.

### Why

`includes:` was the ONE structural element in this grammar with no `if:` /
`unless:`. Nodes, topics, services, actions, scope paths and node paths all
carry them and are all filtered by `cond.rs::filter_manifest`. The
specification claimed carrying per-child conditions was what an include entry
was FOR, and neither spelling parsed.

The usage measurement argued for retiring the block instead: one fixture in
this repository uses it, no real contract does, external includes are never
loaded by the checker, and the consumer composes scopes through its own scope
table without reading `includes:` at all. The project owner ruled the other
way, on better grounds:

> Our contract should reflect the launch file structure. If the launch file has
> a condition on any X, the contract should have one too.

`IncludeDecl::Inline`'s own doc comment says it comes "from `<group>` block",
and a `<group>` is exactly what a launch author writes `if=` on. Low usage is
then a statement about adoption rather than about whether the feature belongs.
Recorded as design issue #56.

### The shape

`IncludeDecl` becomes a struct carrying `if_condition` / `unless_condition`
plus an `IncludeKind` (`External { manifest }` / `Inline(Box<Manifest>)`),
which is where every other declaration keeps its conditions. Two accessors
(`.inline()`, `.external()`) removed the `match` at all four call sites, so
the change outside `types/` is three lines.

The serde representation is `untagged` + `flatten` on purpose: the old
externally-tagged enum serialized as `{"External":{"manifest":...}}`, which
the parser could not read back. A serialized include is now the YAML the
parser accepts.

### The spellings

- External: `if:` / `unless:` beside `manifest:`.
- Inline: at the nested manifest's root, beside `nodes:` / `topics:`.
- A root condition on a STANDALONE manifest is refused, naming the include
  entry as where it belongs -- at a root nobody includes it would select
  nothing, and silently ignoring it is the failure mode phase 69 removed.

### The filtering, including the part that is easy to get wrong

`filter_manifest` retains includes on their condition and clears it on
survivors, like every other entity. For refs into a scope
(`include_name/group_name`), the existing mechanism is reused rather than
duplicated: conditional include names join the `conditional_nodes` set, and
SURVIVING include names join the owner set. Both halves are load-bearing --
adding only the first silently drops every ref into an include that survived
its own `if: "true"`.

**Conditions inside an inline include are now evaluated too.** The filter
walked the outer include and never recursed, so a node declaring
`if: "false"` inside a surviving group stayed, with its condition still set --
making it the one surviving entity in a filtered manifest that kept one. That
gap predates this release and was invisible while the container itself could
not be filtered. Recursion runs after the outer retain, so a dropped include
is never walked.

### Tests

Six added, `cargo test --workspace` **554 -> 560**, 0 failed. Each new
behaviour was checked by removing the code that implements it and watching the
test fail with the symptom the issue described; the ref-cleanup control
(a ref into an UNCONDITIONAL missing include is kept, so the cleanup cannot
swallow a typo) passes before and after, as a control should.

## v0.1.40 - 2026-09-23

One behaviour fix, in `derive`: `MapperNode::scope` now carries the node's
ROS NAMESPACE instead of the owning launch-file scope id. Issue 52 R5, the
last seam of the shared derivation, and the reason both consumers carried a
one-line FQN workaround. The workspace version stays `0.1.4`: no API
changes shape, only what one field holds.

`sched`'s `[[assign]] scope =` selector matches a node whose scope equals
the selector or is a descendant of it (`scope_selector_matches`,
`sched/src/resolve.rs`), so the field has to be the namespace:
`/perception/lidar` for `/perception/lidar/a`. `derive` copied
`NodeInstance::scope`, which is a different tree. The two agree only when
every launch scope pushes exactly its own namespace segment.

The failure was silent, not loud. On a model with one launch scope `/`
holding `/perception/sensor_node` and `/control/control_node`, every node
carried the scope `/`, so `[[assign]] scope = "/perception"` selected
nothing and those nodes fell to the default tier. No error was raised: a
selector is an error only when it matches no node in the SYSTEM, and `/`
always matches.

`NodeView::scope` is unchanged and still the scope id, because `graph.rs`
tests scope-path subtree membership with it. Both meanings are real; only
the mapper field was wrong about which one it wanted.

The golden `RankedPlan` snapshot is byte-for-byte unchanged, as it must be:
scope feeds tier binding, not the order. Three tests in
`derive/src/tests.rs` hold the fix, one of them end to end on the
`timer_chain` fixture.

**For consumers.** play_launch and nano-ros may drop their FQN workaround
on this tag. Keeping it is harmless, since deriving the namespace of a
namespace is the namespace, so the two sides can migrate independently.

## v0.1.39 - 2026-09-23

Documentation only, plus five stale strings in code. No grammar change, no
arithmetic change, no API change: the workspace version stays `0.1.4` and a
contract that parsed at v0.1.38 parses here.

v0.1.38 fixed the EXAMPLES in the hand-written docs and left the prose and the
arithmetic unverified. This release checks both against the source, one
document at a time.

### The specification describes the language that exists

`docs/launch-manifest.md`: a mechanical diff of the generated
`docs/format-reference.md` against the prose found **six live fields the spec
never mentioned** — `nodes.<n>.params:`, `nodes.<n>.concurrency.exclusive:`,
the whole `functions:`/`modes:` vocabulary, `severity_levels:`, and the
subscriber's `buffer` and `on_violation`. That gap is now zero. Retired
spellings are collected into one fifteen-row table, each naming its
replacement and the phase that removed it.

Three field errors: every `external_topics:` example used the deprecated
`external:` key rather than `side:`; `max_response` sat under a heading
implying `cli:` accepts it (that spelling is a parse error) and was marked
"Not checked" when it is read as a deadline and by `response-blocking`; and
`version:` was documented as required when the parser defaults it to 1.

### The arithmetic is stated as it computes

Most of it had never been written down at all. Now documented, each citing
its implementation: derived rates, the derived route and its cost, FDTI and
FRTI, criticality, and the mode ladder.

Two formulas were WRONG rather than missing, and both in the direction that
reads as correct:

- **Fan-in rate.** `contract-theory.md` asserted `f = min(f_A, f_B)`
  unconditionally and `slides.md` had a table row saying the same. It is the
  SUM of the input rates without `sync:` — one callback fires once per
  message on EACH topic it is registered for — and the min only with it.
  Taking the min in both cases understates a fan-in node's load by exactly
  the factor that decides whether it fits.
- **Sampling cost.** The theory document gave `S = Sum(P_i + C_i)` and
  attributed the verdict to `scope-sampling-feasibility`. `sampling_cost_ms`
  is the sum of the sampling PERIODS alone, which is what that rule judges;
  `P_i + C_i` is the traversal cost, which is what the mapper's feasibility
  check sums.

Also corrected: `max_age` was described as runtime-only (`lifespan-age` reads
it, and it is an FDTI mechanism); a `max_age`-versus-budget consistency check
was documented that exists nowhere; `qos-match` was said to run per
satisfiable arg model when it has no arg logic at all; `consistency` was said
to merge three fields when it merges five; and the example diagnostics
throughout the spec were invented rather than the strings the rules emit.

### The scheduling document had gone stale at the crate boundary

`docs/scheduling.md` still said "this derivation is per-consumer", which
v0.1.37 made false — `derive/` is the one derivation both consumers call, and
its fact rules are not the ones the old table listed. It also claimed `sched`
has no `types` dependency (it does, for `Duration`), described a submodule
that does not exist, called `ResolvedTier` a 13-field record (14), and gave
the apply layer as `sched_setscheduler(2)` rather than `sched_setattr(2)`.
The v0.1.38 tie rule is now taught as the general rule rather than a footnote,
including why `ResolvedTier::posix` stays `None` for the two simple mappers.

### The design log reads as history

`docs/design-issues.md` keeps every entry's body and vocabulary — an entry
explaining why `chains:` was removed must keep saying `chains:` — and gains a
status legend and per-entry status lines. Two entries were contradicted by
the code rather than merely dated: **#50** records a decision to drop
`min_latency` that was REVERSED in phase 67 and never written down, and
**#52** still said "no code yet" while R1-R4 had all landed and `derive` had
shipped as the fifth workspace member.

### The doc test could not see a class of example

`sched/tests/docs_yaml.rs` matched a fence with `trim_end()` and no
`trim_start()`, so every INDENTED fence was invisible to it — and three
existed, two of them teaching a spelling the parser rejects. A guard that
cannot see a class of example is worse than no guard, because it reads as
coverage. Fixed, with bodies dedented by the fence's own indentation so an
indented block is not judged on its leading spaces. **45 blocks parsed, 2
expect-error, 2 skipped, 49 total** — up from 40 blocks, of which five were
unreachable.

### Five strings in code

`NodeDecl.criticality`'s doc comment still said the field is "advisory, not
schema-enforced" and that unrecognised values "are ignored, never a parse
error" — false since phase 70 made it a closed set, and predating phase 72,
which made the label a consequence. `scope-budget` printed the retired
`max_transport_ms` in a diagnostic authors read. Three module docs named
`max_interval_ms`, `timeout_ms`, `max_drop_rate` and `rr_timeslice_us` as if
they were current field names.

### Known divergence, not fixed here

A subscriber's `max_transport` is legal grammar and the checker honours it
(preferring it over the topic's, for the heterogeneous-transport case it was
added for), but `model::SubContract` has no transport field, so `derive`
reads the topic's value alone and the two copies of one derivation compute
different route totals wherever a contract uses the override. Filed as
play_launch issue #0042; fixing it needs a model field and a lowering, which
is a release of its own.

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
