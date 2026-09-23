# Scheduling Specification Crate

**Crate:** `ros-launch-manifest-sched` (this repository's workspace,
alongside `types`, `check`, `model` and `derive`).

**Purpose:** portable scheduling specification and derivation shared
between `play_launch` (Linux RT, via `ros-launch-resolve`) and `nano-ros`
(RTOS targets). The integrator ships one small **platform file** per
target; per-node scheduling is **derived** from the timing facts the launch
tree and its contracts already state — read off the resolved SystemModel by
the `derive` crate — by a named, pluggable **mapper**, with explicit
per-node **overrides** that always beat derived values.

**Key invariant:** everything platform-agnostic (timing facts, chain
structure, priority *ordering*) is kept separate from platform
realization (OS priority numbers, scheduler classes, cores). The shared
part is the **algorithm**, not the output — each consumer runs its own
realizer over the same ranking core.

Two schemas coexist:

- **v2 platform file** (`<stem>.system.<target>.yaml`) — the current
  default: mapper name + platform facts + overrides. This is what ships
  through the contract discovery channels.
- **v1 `system.toml`** (hand-written tiers + `[[assign]]`) — legacy,
  still fully supported via a bridge to the `manual` mapper. Explicit
  `--sched <path>.toml` only; never discovered through channels.

## Where scheduling facts come from

The mapper input is derived **once**, in this repository, by the `derive`
crate (`derive/src/lib.rs`): `mapper_input_from_model(&SystemModel,
&DeriveFacts) -> (MapperInput, DeriveReport)` reads the facts the *checker*
resolved per entity out of the resolved SystemModel and returns what the
mappers below rank, plus a report of what the model could not tell it. The
contract keys behind those facts are in
[launch-manifest.md](launch-manifest.md) and
[format-reference.md](format-reference.md).

| Fact | Rule (`derive/src/lib.rs`) |
|------|----------------------------|
| effective trigger | the model's per-path `trigger`; a path carrying none is `Unclassified`, **never** a timer (listed in `DeriveReport::paths_without_trigger`) |
| `rate_hz` | the fastest `Timer` trigger among the node's paths, and nothing else — authored `topics.<t>.rate_hz` and `pub.<ep>.min_rate_hz` are runtime promises no mapper reads |
| `deadline_us` / `path_budget_ms` | min over the node's paths' `max_latency` and its services' `max_response` |
| `criticality` | the model's effective (hazard-derived, phase 72) value when it carries one, else the advisory `high`/`medium`/`low` label |
| `claims_concurrency` | declared `concurrency.exclusive` sets sharing a member merge; concurrency is claimed unless one merged group covers every declared path (`exclusive: []` claims it; an absent declaration, or a single path, does not) |
| `exec_ms` | `DeriveFacts::path_exec_ms` keyed `"<node FQN>/<path>"`, else `DeriveFacts::node_exec_ms` and only when the node has exactly ONE path — with several, the split is unknown and attributing the whole node's cost to any one of them would overstate it |
| chains | `resolve_chains`: one per scope path that declares a budget — the longest causal route from the scope's input topics to its output topics within its subtree, timer paths as `Boundary` elements (`period_ms = 1000 / rate_hz`), the rest as `Segment`s; chain criticality = max over members. A scope path that yields none is reported with a reason (`ChainSkip::{NoBudget, NoEndpoints, Cycle, NoRoute, NoPathOnRoute}`) |

**Cost is authorable, and a deadline is never a cost.** `DeriveFacts` is the
one input the model does not carry, because it differs per platform:
play_launch fills `node_exec_ms` from the platform file's `budget`
([below](#v2-platform-file)), nano-ros from its `[wcet]` profile. Neither is
synthesized from a `max_latency` — absent cost stays ABSENT, which is what
lets `MapWarning::ChainFeasibleWithoutWcet` report a chain as feasible *on
incomplete evidence* rather than as feasible
([Diagnostics](#diagnostics)).

`sched` itself stays a pure algorithm crate — no parser, no `model`
dependency — so `derive` sits *above* `model` (which already depends on
`sched`) instead of inside it.

On a model resolved before the trigger fact existed, every path reads as
`Unclassified`, the derivation ranks nothing, and the report lists every
path. That is deliberate: a stale model must be visible as such, not
scheduled from a guess.

> **Design issue #52** (landed R1–R4, tag `v0.1.37`): before this, each
> consumer built `MapperInput` itself and the two disagreed on the trigger,
> the chains, the criticality, `claims_concurrency`, `rate_hz`,
> `deadline_us` and `exec_ms`. `resolve_chains` is the port of
> play_launch's `manifest_graph` onto the model (`derive/src/graph.rs`).
> Adoption is staged: play_launch consumes it (phase-78), nano-ros's
> `nros-orchestration-ir::mapper_input` is still its own until phase-457.
> One seam is open — `MapperNode::scope` is the namespace the `manual`
> mapper's `[[assign]] scope =` selector matches, while `derive` copies the
> model's file-scope key; patched consumer-side for now. See
> [design-issues.md](design-issues.md).

## v2 Platform File

One file names one target (`target:` header). `posix` (Linux RT, with
`native` accepted as an alias) is typed concretely; any other target
parses as raw passthrough — the consumer (nano-ros) validates its own
per-target vocabulary. Schema and every rule below:
`sched/src/platform.rs`.

```yaml
target: posix                # required, non-empty
mapper: chain_aware          # SchedMapper name, looked up in MapperRegistry
reservations: off            # off (default) | required — POLICY, so not under resources
resources:                   # platform facts, typed per target
  rt_priority_band: { min: 10, max: 40 }
  isolated_cpus: [0]
  rr_timeslice: 100ms        # the host's GLOBAL SCHED_RR slice; absent = unknown
overrides:                   # explicit per-node pins; beat derived values, always
  control_node: { priority: 20, core: 0 }
  obstacle_detector: { budget: 8000us, uclamp_max: 800 }
  telemetry_logger: { sched_class: SCHED_BATCH, nice: 10 }
  planner: { cpus: [4, 5] }
```

- **`posix` `resources`** (`PosixResources`, `deny_unknown_fields`):
  `rt_priority_band: Option<PriorityBand>` (`{ min: i64, max: i64 }`,
  inclusive), `isolated_cpus: Vec<u32>` (advisory; not enforced by this
  crate). For `target: posix` the band is validated at parse time:
  `min <= max` and entirely inside Linux's legal `SCHED_FIFO`/`SCHED_RR`
  range `1..=99` (`POSIX_RT_PRIORITY_MIN`/`MAX`), else
  `PlatformError::InvalidPriorityBand`.
- **`posix` `overrides.<node>`** (`PosixOverride`): `priority:
  Option<i64>`, `core: Option<u32>`, `cpus: Vec<u32>`, `sched_class:
  Option<String>`, `nice: Option<i32>`, `uclamp_min`/`uclamp_max:
  Option<u32>`, `budget: Option<Duration>`.
  Keys use the same selector vocabulary as v1 `[[assign]].nodes`: full
  FQN or bare last segment. Parsing lives here; *applying* overrides is
  the caller's job (see [Consumers](#consumers)).

  Every combination rule is checked at parse time
  (`validate_posix_override`), because an override that reaches the
  syscall layer comes back as an `EINVAL` with no node name attached:
  `sched_class` must be one of the six real policies (**unknown is an
  error** — it used to become `SCHED_OTHER` silently, so a typo dropped a
  node out of real-time); `priority` only with `SCHED_FIFO`/`SCHED_RR`
  and `nice` only with `SCHED_OTHER`/`SCHED_BATCH`; `SCHED_DEADLINE`
  takes no CPU pin at all; `core` and `cpus` are mutually exclusive;
  `nice ∈ -20..=19`; `uclamp ∈ 0..=1024` with `min <= max` (the kernel's
  scale, `UCLAMP_MAX`, not a percentage). `uclamp_min` is a **no-op on RT
  policies** — they already default to 1024/1024 — so it parses and the
  consumer warns rather than silently doing nothing; `uclamp_max` is the
  useful RT knob, letting an RT thread run *below* the maximum performance
  point.

  `budget` is the **declared execution cost** — the only legitimate
  source for a `SCHED_DEADLINE` reservation's runtime, and the field
  `play_launch measure` emits. Written `8000us`/`8ms`, with `budget_us:
  8000` accepted as a deprecated alias. It is not a proven WCET; it is a
  declared high-percentile observed cost used *as* an upper bound.
  `budget: 0us` is rejected: absent and zero are different answers, and a
  declared zero would silently mean "free".

- **`reservations`** (`ReservationMode`, top level beside `mapper:`):
  `off` (default) or `required`. Deliberately not inside `resources:` —
  that holds facts about the machine, and whether to reserve is a policy
  choice. Opt-in because reservations are all-or-nothing within a band: a
  reserved node preempts every fixed-priority thread regardless of
  priority, so a band holding both loses the ordering the mapper
  computed. Without the switch, adding one `budget` would turn that
  rule into a hard error nobody asked for.

- **`rr_timeslice`**: the host's `SCHED_RR` slice, written `100ms`
  (`rr_timeslice_us: 100000` is a deprecated alias). A platform *fact*,
  which is why it sits in `resources:` — on Linux the slice is a global
  sysctl (`/proc/sys/kernel/sched_rr_timeslice_ms`, default **100 ms**),
  not a per-task value, so the per-tier `TierPlatformSpec::time_slice`
  cannot express it. Every mapper that can produce a priority tie —
  `rate_monotonic`, `deadline_monotonic` and `chain_aware` — needs it to
  decide whether `SCHED_RR` is worth deriving for that tie at all. Absent
  means unknown, never "assume the default".
- **Unknown target** (`zephyr`, `freertos`, …): `resources`/`overrides`
  parse as raw `serde_yaml_ng::Value` (`PlatformResources::Raw` /
  `PlatformOverrideEntry::Raw`) — untyped, never range-validated (e.g.
  Zephyr's negative cooperative priorities), passed through for the
  consumer to validate.
- Entry point: `parse_platform_file(path) -> Result<PlatformFile,
  PlatformError>` dispatches on extension — `.yaml`/`.yml` → this schema
  (`parse_platform_file_yaml`), `.toml` → the
  [legacy bridge](#legacy-toml-bridge).

```rust
pub struct PlatformFile {
    pub target: String,
    pub mapper: String,
    pub reservations: ReservationMode,
    pub resources: PlatformResources,
    pub overrides: BTreeMap<String, PlatformOverrideEntry>,
    pub legacy: Option<SystemSched>,   // Some only via the .toml bridge
}
```

## `SchedMapper` Trait and Registry

```rust
pub trait SchedMapper {
    fn name(&self) -> &str;
    fn map(&self, input: &MapperInput, facts: &PlatformFacts)
        -> Result<SchedPlan, MapError>;
    fn map_with_diagnostics(&self, input: &MapperInput, facts: &PlatformFacts)
        -> Result<(SchedPlan, MapDiagnostics), MapError> { /* default: map() + empty */ }
}
```

- **`MapperInput`** — dependency-free facts, built by `derive`
  ([above](#where-scheduling-facts-come-from)); not a serde type:

  ```rust
  pub struct MapperNode {
      pub name: String,                    // FQN
      pub scope: String,                   // namespace / scope path
      pub rate_hz: Option<f64>,
      pub deadline_us: Option<u64>,
      pub criticality: Option<Criticality>, // Low < Medium < High
      pub path_budget_ms: Option<f64>,     // filled by `derive`; read by no built-in mapper
      pub paths: Vec<MapperPath>,          // per-path facts (chain_aware only)
      pub claims_concurrency: bool,        // `false` == every path serialises
  }
  pub struct MapperInput {
      pub nodes: Vec<MapperNode>,
      pub legacy: Option<SystemSched>,     // manual mapper only (.toml bridge)
      pub chains: Vec<ResolvedChain>,      // chain_aware only
  }
  ```

  `claims_concurrency` is the one bit a mapper needs from a node's
  `concurrency:` declaration: a node whose callbacks all serialise behaves
  as a single-threaded executor and one claiming concurrency does not,
  which is what decides whether a **per-thread** reservation is sound.
  `false` is the safe default and matches an absent declaration.

- **`PlatformFacts`** — alias for `PlatformResources` (the platform
  file's parsed `resources`).
- **`SchedPlan`** — alias for `ResolvedTierTable` (deliberately reused:
  "ordered priority/core/sched_class placement grouped by tier, with
  member node names" is what every mapper produces). A rank holding one
  node emits a one-member tier named after it; a rank holding several
  (nodes whose ranking fact is exactly equal) emits one tier carrying all
  of them, named after the shared fact. Nodes with no usable facts
  collapse into `DEFAULT_TIER` (priority 0, no `sched_class` — non-RT).
- **`MapperRegistry`** — `register(Box<dyn SchedMapper>)`, `get(name)`;
  `with_builtins()` pre-registers **four** mappers: `manual`,
  `rate_monotonic`, `deadline_monotonic`, `chain_aware`. Consumers
  register additional mappers at link time; no dynamic loading.
- **`MapError`** — `MissingPriorityBand`, `InvalidPriorityBand`,
  `MissingLegacySpec`, `Resolve(SchedError)`.

## Built-in Mappers

- **`manual`** — legacy semantics. Requires `input.legacy` (populated
  only by the `.toml` bridge); delegates to `resolve()` against the v1
  tiers + `[[assign]]` for `target = "posix"`, reproducing v1 output
  exactly. Ignores `facts`.
- **`rate_monotonic`** — higher `rate_hz` → higher priority, spread
  linearly across `resources.rt_priority_band` (rank 0 → `band.max`,
  last → `band.min`; a narrow band produces ties, never inversions).
  Deterministic: rate descending; nodes at **exactly** the same rate
  collapse into ONE rank, with members ordered by node name. No `rate_hz`
  → `DEFAULT_TIER`. Requires a valid posix band
  (`sched/src/mapper.rs`).
- **`deadline_monotonic`** — same shape, ranked by `deadline_us`
  ascending (shorter deadline → higher priority); equal deadlines collapse
  the same way.
- **`chain_aware`** — chain-first shaping; the primary mapper for
  systems declaring end-to-end scope `paths:`. Detailed below.

### An exact tie is a fact, and every mapper treats it the same way

All three deriving mappers share ONE answer to "what does a tie mean". It is
stated here because `rate_monotonic` and `deadline_monotonic` used to answer
it differently: they sorted by rate/deadline and broke the tie by NODE NAME,
so two 30 Hz nodes got priorities 40 and 30. A different priority between
equal-period nodes is a policy statement, and there the policy was the
alphabet — renaming a node changed who preempts whom (design issue #53, in
`v0.1.38`).

**Collapse.** Rate-monotonic theory assigns equal periods equal priority, so
the band is spread over the number of **distinct** values, not the number of
nodes: four nodes at two rates take two levels, not four
(`rank_groups`/`spread_priority`, `sched/src/mapper.rs`). Equality is exact
and on the value as declared — 30 and 30.000001 do not tie, and nothing is
rounded into one: the collapse states a fact the contract carries, and a
tolerance would invent one it does not. A rank holding more than one node is
ONE tier carrying all of them as sorted `members`, named after the fact they
share (`rate_hz=30`, `deadline_us=5000`) rather than after any one member — a
tier named `/a` holding `/a` and `/b` would read as a tier holding only `/a`.
A one-node rank still names itself after its node, so nothing moves for
untied plans. The consumer explodes a grouped tier into one tier per member
before applying overrides, so a multi-member tier is transparent downstream.

**Decide.** The tied set is then handed to `chain_aware`'s decision,
`rr_policy_for_ties` (`sched/src/chain_aware_mapper.rs`): `SCHED_RR` when
`resources.rr_timeslice` is strictly shorter than the shortest period among
the tied nodes — the only case in which rotating between them changes
anything — otherwise `SCHED_FIFO` plus a
`MapWarning::UnmitigatedPriorityTie` naming both numbers. That function takes
a **period closure**, not the whole `MapperInput`: it used to read each
node's period off `node.paths`, which the two simple mappers never populate,
so they would have seen `None` on every tie and could never have derived RR.
Each mapper answers from the fact it ranks on — `1/rate_hz`
(`period_from_rate`), the deadline under the implicit-deadline assumption, or
for `chain_aware` the shortest budget among the node's paths
(`shortest_node_budget_us`). Absent is not zero: a node with no usable period
makes the slice comparison unanswerable, and RR is declined and reported
rather than guessed. A non-positive or non-finite rate is not a period.

**Band-compression ties take the same decision, deliberately.** Five distinct
rates in a three-level band already produced equal priorities, silently FIFO;
judging a tie the mapper *creates* by a different rule than one it *derives*
would be a third policy. Practical effect: on a platform file that states
`rr_timeslice`, a compressed plan can now carry `SCHED_RR` where it carried
`SCHED_FIFO`, and where the slice does not fit, a warning appears that was
previously absent.

Both simple mappers emit `class: real_time` and `sched_class: SCHED_FIFO`
(or `SCHED_RR` for a mitigated tie) per ranked node, and populate `warnings`
only — `details` (per-rank `--explain` provenance) remains a `chain_aware`
feature. Neither writes the typed `posix` placement: **`ResolvedTier::posix`
stays `None` for these two**, so RR is expressed in `sched_class` alone, and
the consumer reads the `sched_class`/`priority`/`core` trio for these plans.
That looks like an oversight and is not — play_launch's `derive_reservations`
and `report_jitter_placement` both test `tier.posix` for real-time-ness, so
filling it would make every `rate_monotonic` tier reservation-eligible and
silence its jitter warnings. Changing it is a separate decision from the tie
rule.

**Applying `overrides` is not part of the trait** — "override beats
derived, always" is mapper-independent logic the caller applies after
`map()` returns.

## The `chain_aware` Mapper

Derives a global priority order from the chains in `MapperInput::chains`
(PiCAS-style: drain chains toward their sinks), falling back to
criticality-bucketed rate/deadline ordering for everything else. Those
chains are *derived* routes, one per scope path with a budget — the
`chains:` vocabulary was removed in phase 68 W4 and nothing authors a route
by hand. When `input.chains` is empty the mapper degrades gracefully to the
fallback, which is how a system with no scope paths (and nano-ros today)
runs it. Implementation: `sched/src/chain_aware_mapper.rs`.

Algorithm (steps 1–4 platform-agnostic, 5–6 POSIX realization):

1. **Feasibility** (`chain_feasibility`). Per chain: `sampling_cost_ms = Σ
   over boundary elements (period_ms + exec_ms)`; `controllable_ms =
   max_latency_ms − sampling_cost_ms`. Not strictly positive →
   `MapWarning::ChainInfeasible`, chain excluded from shaping (members fall
   through to the non-chain path). An ABSENT `exec_ms` is still *counted* as
   zero — there is nothing better to count — but each such boundary is
   recorded and reported as `MapWarning::ChainFeasibleWithoutWcet`, so a
   `feasible` verdict computed from missing evidence does not read as a
   measured one. (Same rule as the static `scope-sampling-feasibility` check
   — see
   [contract-theory.md](contract-theory.md#cross-scope-chains-and-sampling-cost).)
2. **Chain order.** Criticality descending (chain criticality = max over
   member nodes, derived by the caller), controllable-slack ascending,
   name ascending.
3. **Within a chain** (`chain_item_order`). Walk elements sink→source: each
   causal *segment* ranks drain-toward-sink (`nodes_in_topo_order`
   reversed); each maximal run of timer *boundaries* keeps its walk position
   but is internally re-ordered rate-monotonically (shorter `period_ms`
   first, node name to break that).
4. **Non-chain remainder** (`non_chain_item_order`). Bucket by criticality
   (High, Medium, Low, none), then order by one unified ascending time
   budget per path: the timer period (`1000/rate_hz`) for timer paths, the
   declared `max_latency_ms` for input paths. Under the implicit-deadline
   assumption RM *is* DM, so the one comparison reproduces both orderings
   and interleaves a mixed bucket instead of splitting it by trigger kind.
   Paths with no derivable budget — `once`, `spontaneous`, `unclassified`,
   or an input path with no declared `max_latency` — never rank, and their
   node falls to `DEFAULT_TIER` unless another of its paths ranked. Items
   with **exactly** equal (criticality, budget) collapse into one rank
   (`tie_group`).
5. **Band compression** (POSIX realizer, `assign_priorities_compressed`).
   Runs are seeded one per item, except that a maximal run sharing one
   `Some(tie_group)` is merged up front — that collapse is unconditional,
   not scarcity-driven. While the run count exceeds the band's inclusive
   width (`max − min + 1`), adjacent runs merge tail-first, first within the
   same `fine_group` (segment / boundary run / bucket), then within the same
   `coarse_group` (the same chain); never across the chain/non-chain divide
   or a criticality bucket. If still too wide, the overflow clamps into
   `band.min` (ties, never inversions) with `MapWarning::BandTooNarrow`
   naming the irreducible class count, the width and the clamped classes.
   Priorities are dense from `band.max` downward — not the linear spread of
   the simple mappers.
6. **Node projection.** A node's final priority = **max** over all its
   ranked paths (this also resolves nodes shared across chains). The RT
   policy is then chosen per node by the shared tie rule
   ([above](#an-exact-tie-is-a-fact-and-every-mapper-treats-it-the-same-way)),
   with the period read as the shortest budget among the node's paths
   (`shortest_node_budget_us`). Unlike the two simple mappers, `chain_aware`
   *does* write the typed placement: each ranked tier carries
   `posix: Some(PosixPlacement { sched: Fifo|Rr, affinity: Inherit, uclamp:
   None })` beside the `sched_class` and `priority` fields.

### Agnostic core / realizer split

The mapper is split so RTOS consumers can reuse the ranking without the
Linux priority model:

- **`chain_aware_rank(&MapperInput) -> RankedPlan`** (also
  `ChainAwareMapper::rank`) — steps 1–4 only. No `PlatformFacts`, no
  band, no OS priorities, infallible.

  ```rust
  pub struct RankItem {
      pub node: String,
      pub path: String,
      pub fine_group: usize,            // segment / boundary-run / bucket;
                                        // doubles as RTOS executor grouping
      pub coarse_group: Option<String>, // chain name; None = non-chain
      pub tie_group: Option<usize>,     // Some ⇒ unconditional collapse
      pub provenance: String,
  }
  pub struct RankedPlan { pub items: Vec<RankItem>, pub warnings: Vec<MapWarning> }
  ```

  `items` order *is* the priority order, highest first.
- **`realize_posix(ranked, input, facts)`** — private; steps 5–6.
  Reached through `ChainAwareMapper::map` / `map_with_diagnostics`.
  Split-parity is test-asserted: realize(rank(input)) is byte-identical
  to the pre-split combined output.
- nano-ros implements its **own realizer** (`realize_rtos`) over the
  same `RankedPlan` — see [Consumers](#consumers).

### Diagnostics

`map_with_diagnostics` returns `MapDiagnostics { details, warnings }`
(deliberately non-serializable — diagnostic output, never embedded in a
model):

- `ChainAwareDetail { node, path: Option<String>, priority, provenance }` —
  per-(node, path) `--explain` rows; `chain_aware` always fills `path`.
  Provenance strings, verbatim from the mapper:
  `derived(chain_aware: <chain> segment drain <k>/<n>)`,
  `derived(chain_aware: <chain> boundary RM period=<p>ms)`,
  `derived(chain_aware: non-chain criticality=Some(High) budget_ms=<b>)`
  (criticality rendered as Rust `Debug` of the `Option`),
  each suffixed `-> prio <p>` by the realizer — e.g.
  `derived(chain_aware: points_to_cmd segment drain 2/2) -> prio 39`.
- `MapWarning`, all four variants:
  - `ChainInfeasible { chain, sampling_cost_ms, budget_ms }` — sampling cost
    alone consumes the chain's budget; no priority assignment fixes it.
  - `ChainFeasibleWithoutWcet { chain, boundaries_without_wcet }` — the
    verdict was `feasible`, but one or more timer boundaries carry no
    `exec_ms` and were counted as ZERO, so it is feasible *on incomplete
    evidence*, optimistic by an unknown amount. Each entry is a
    `"<node>/<path>"`. An evidence problem, not a scheduling one; this is
    what an absent declared cost produces, and why no cost is ever
    substituted from a deadline.
  - `UnmitigatedPriorityTie { priority, nodes, rr_timeslice_us,
    shortest_period_us }` — two or more nodes ended at the same priority and
    `SCHED_RR` could not be derived to rotate between them, either because
    the platform file states no `rr_timeslice` (unknown, never "assume the
    100 ms default"), because no tied node has a usable period, or because
    the slice is not shorter than the shortest of them. Both numbers are
    carried, both `Option`. Emitted by `rate_monotonic` and
    `deadline_monotonic` too since `v0.1.38` (design issue #53), not only by
    `chain_aware`.
  - `BandTooNarrow { distinct_classes, band_width, clamped }` — the band
    could not hold the classes remaining after every legal collapse; the
    lowest were clamped into `band.min` (ties, never inversions).
    `clamped` labels them `chain '<name>'` or `non-chain bucket`, deduped,
    in ranked order.

`rate_monotonic` and `deadline_monotonic` populate `warnings` only —
`details` (per-rank `--explain` provenance) remains a `chain_aware` feature.

## Chain Vocabulary (`chain.rs`)

A deliberate minimal mirror of the `types` crate's Vocabulary v2, so that
`sched` stays a pure algorithm crate: it never reads a contract, never
parses one, and depends on `types` for nothing but the `Duration` spelling
(no `check`, `model` or `derive` dep). All data types serde round-trip; the two
data-carrying enums (`EffectiveTrigger`, `ChainElement`) are adjacently
tagged so their YAML stays plain mappings, no `!tags`:

- `EffectiveTrigger` — `Timer { rate_hz } | Input(endpoints) | Once |
  Spontaneous | Unclassified`; `period_ms() = 1000/rate_hz`, `None` for a
  non-positive or non-finite rate.
- `MapperPath { name, effective_trigger, max_latency_ms, exec_ms,
  inputs, outputs, max_jitter_ms, miss }` — the per-(node, path)
  requirement unit. `max_latency_ms` is the **deadline** fact; `exec_ms` is
  the separate **cost** slot, in milliseconds, filled only from a declared
  budget ("no WCET is ever invented"). `max_jitter_ms` is a spread, not a
  second deadline: a path carrying one has a stability requirement that
  best-effort placement cannot honour, which is all a mapper does with it.
- `MapperMiss { tolerate_n, tolerate_w, consecutive, action }` +
  `MapperMissAction { Continue, SkipNext, Abort }` — deadline-miss handling,
  mirrored from the contract vocabulary rather than re-exported (these
  types are `Serialize + Deserialize`; the contract types are
  Serialize-only). `requires_detection()` is true of every form of it;
  `is_enforceable_on_linux()` is true only of `Continue`, which is what CBS
  already does on an exhausted reservation — the other two are obligations
  on the node, and a realizer must report them rather than silently
  substituting `Continue`.
- `ConcurrencyContract { exclusive: Vec<Vec<String>> }` — a node's declared
  exclusion relation between its own paths. **Absent is not empty**: no
  declaration means every path serialises (what `rclcpp`'s implicit
  `MutuallyExclusive` group and nano-ros's `default_cbg_type` already do),
  while `exclusive: []` is the opposite claim. The map that holds it
  carries the distinction; a bare `Vec` would lose it.
- `ChainSemantics { Reaction, Age }` — **nothing branches on this**, here or
  in the checker. It survives only as a field of `ResolvedChain` that
  crosses into `system_model.yaml` and on to nano-ros; every route this
  crate now sees is derived, and a derived route is `Reaction`.
- `SegmentNode { node, path }`; `ChainElement` —
  `Segment { nodes_in_topo_order: Vec<SegmentNode> }`
  | `Boundary { node, path, period_ms, exec_ms }`.
- `ResolvedChain { name, criticality, max_latency_ms, semantics,
  elements }`. Chain-level criticality does not exist in the authored
  vocabulary — it is derived as the max over member nodes.

**Where the route comes from.** This crate takes chains already resolved:
it has no launch DAG and no topic graph, by design. `derive::resolve_chains`
(`derive/src/graph.rs`) builds them from the model — the critical path of
the subgraph between a scope path's two ends, topologically sorted by
construction and fork-join correct (`max` over branches, not a sum). Before
phase 68 W4 the caller instead took an authored `segments:` list verbatim,
which was already author-linearized — so the fan-in tie-break rule in the
crate's doc comments (longest-path-to-sink, then deadline, then name) had
never had an input that would distinguish it from declaration order. Both
`chains:` and `segments:` are parse errors today.

## Validation Helpers

Pure functions over an already-derived `SchedPlan`; the caller decides
warn-vs-strict and presentation (`--sched-apply`, `--explain`):

- **`band_violations(plan, band)`** — every non-default tier whose
  priority falls outside `[band.min, band.max]`. Two kinds of tier are
  skipped rather than compared, because neither HAS a priority to compare:
  the synthesized default tier, and any tier whose typed placement returns
  `PosixSched::priority() == None` — i.e. a `SCHED_DEADLINE` reservation,
  whose `priority` field is absent, not zero. Comparing those was the same
  defect class as the `SCHED_OTHER 10` tier once "clamped" into the RT band:
  a state Linux cannot represent, and fatal under strict mode.
- **`rate_priority_contradictions(input, plan)`** /
  **`deadline_priority_contradictions(input, plan)`** — pairwise scan: a
  node with strictly higher rate (or strictly shorter deadline) than
  another must not land at strictly lower priority.
  `rate_monotonic`/`deadline_monotonic` never trigger this by
  construction. `chain_aware` triggers it **by design** — chain rank
  deliberately overrides raw timing facts — so the consumer suppresses
  those as chain-intended; hand-authored overrides and the `manual`
  mapper's independent tiers can trigger it as genuine mistakes.

## Legacy v1 Schema (`system.toml`)

Hand-written tiers + sparse node binding. Still fully supported; sole
implementation of the `manual` mapper. Scheduled for retirement (Phase
41.6) only after nano-ros migrates off it — no flag day.

```toml
# ===== GENERIC (portable — byte-identical across platforms) =====
[tiers.control]
class = "real_time"        # best_effort | real_time | time_triggered | interrupt
deadline    = "50ms"       # deadline_us = 50000 is the deprecated alias
period      = "20ms"       # period_us
budget      = "5ms"        # budget_us
deadline_policy = "warn"   # ignore | warn | skip | fault
spin_period = "1ms"        # spin_period_us

[[assign]]
tier  = "control"
nodes = ["ndt_localizer", "ekf_localizer"]   # FQN or bare-name selectors
[[assign]]
tier  = "perception"
scope = "/perception/lidar"                  # launch-scope subtree selector
# unmatched nodes → synthesized "default" tier (priority 0, non-RT)

# ===== PLATFORM (same shape per target, values differ) =====
[tiers.control.posix]      # `native` accepted as alias
priority    = 80
sched_class = "SCHED_FIFO"
core        = 1
[tiers.control.freertos]
priority    = 12
stack_bytes = 8192
deadline    = "40ms"       # optional per-platform tighten
```

Every duration field above is written with a unit (`ns`/`us`/`ms`/`s`); a
bare number is read as MICROSECONDS, and the `<name>_us` spellings
(`deadline_us`, `period_us`, `budget_us`, `spin_period_us`, `time_slice_us`,
and `rr_timeslice_us` in the v2 file) are accepted as deprecated aliases.

- **`SystemSched { tiers: BTreeMap<String, TierDef>, assign:
  Vec<AssignRule> }`**, `deny_unknown_fields` on the generic `TierDef`
  head — a stray `priority` on the head is a parse error; that is what
  enforces "no priority leakage" portability.
- **`TierPlatformSpec`** per target sub-table (`posix`, `freertos`,
  `zephyr`, `threadx`, `nuttx`): `priority: i64` (i64 admits Zephyr
  negative coop priorities), `stack_bytes`, `core`, `sched_class`,
  `preempt_threshold`, and per-platform overrides of the generic head:
  `deadline`, `budget`/`period` (sporadic budget + replenishment period,
  both required for a sporadic policy — lets one platform's kernel sporadic
  server engage, e.g. NuttX `SCHED_SPORADIC`, without affecting other
  targets), and `time_slice` (round-robin slice among same-priority tiers;
  ThreadX-only today, ignored elsewhere — on Linux the slice is the global
  `resources.rr_timeslice`, not a per-tier value).
- **`resolve(tiers, assigns, nodes, target) -> ResolvedTierTable`**
  (`resolve.rs`): explicit `nodes` selectors win over `scope` selectors
  (silently); a same-level double-claim for two *different* tiers →
  `SchedError::NodeMatchedByMultipleTiers` (duplicate claims for the
  same tier are accepted); missing
  `[tiers.<t>.<target>]` → `MissingPlatformSpec`; unmatched selectors →
  `UnknownNodeSelector`/`UnknownScopeSelector`. Output sorted priority
  descending. `ResolvedTier` is the flat 14-field record (placement from
  the platform sub-table, policy from the generic head, effective
  `deadline_us = spec ?? head`, sorted `members`, plus the typed
  `posix: Option<PosixPlacement>`); its duration fields are microseconds
  (`period_us`, `budget_us`, `deadline_us`, `spin_period_us`) because it is
  the wire to play_launch and nano-ros, while `TierDef` carries `Duration`.
  **`SCHED_DEADLINE` is not authorable in v1**: the tier schema has no
  per-tier runtime, so a `sched_class = "SCHED_DEADLINE"` sub-table is a
  `PosixError::UnknownSchedClass` naming the v2 platform file instead of a
  reservation invented from nothing.
- These types do double duty: the SystemModel's `execution.tiers`
  reuses `TierDef`/`TierPlatformSpec` (re-exported through the `model`
  crate), so one schema serves v1 authoring, the model's applied-tier
  layer, and the mapper pipeline.

### Legacy `.toml` bridge

`parse_legacy_toml` (`bridge.rs`) wraps a v1 document into a v2
`PlatformFile { target: "posix", mapper: "manual", legacy: Some(sched),
resources/overrides: empty }`. Reached via `parse_platform_file`'s
`.toml` dispatch. Equivalence is test-asserted: TOML → bridge → `manual`
mapper output equals calling `resolve()` directly. RTOS targets don't go
through the bridge — nano-ros calls `resolve(..., target)` directly when
it consumes v1 tiers.

## Consumers

### play_launch / ros-launch-resolve (Linux RT) — shipped

The full pipeline lives in `ros-launch-resolve`
(`resolve/src/ros/sched_loader.rs`, `sched_derive.rs`); `play_launch`
consumes it and owns the apply layer. User guide:
play_launch `docs/guide/rt-scheduling.md`.

- **Discovery** (v2 files only): explicit `--sched <path>` > overlay
  (`--contracts` / `$PLAY_LAUNCH_CONTRACTS` / XDG / `/etc`, layout
  `<root>/<pkg>/launch/<stem>.system.<target>.yaml`) > provider sidecar
  next to the launch file. Same channels as contracts. `--target`
  (default `posix`) must match the file's `target:` header.
- **Derive pipeline** (`derive_sched_plan`): build the `MapperInput` with
  `derive` ([above](#where-scheduling-facts-come-from)) → parse platform
  file → look up mapper in `with_builtins()` → `map_with_diagnostics` →
  flatten grouped tiers to one tier per node → **apply overrides**
  (selector = FQN or bare name; a priority-only override implies
  `SCHED_FIFO` + `real_time` and promotes the node out of the default tier;
  overriding a chain member below its chain rank warns) → band violations
  (clamp + warn, or error under `--sched-apply strict`) → rate/deadline
  contradiction warnings (with chain-intended suppression).
- **`SCHED_DEADLINE` is derived HERE, not in this crate.** The mapper never
  emits a reservation; `sched_loader` does, because the decision needs the
  platform file's `reservations:` mode, the declared budgets and which
  nodes are containers — none of which the mapper sees. The rules:
  runtime from the declared `budget`, period from `1/rate_hz` propagated
  along a chain from its source, deadline declared-else-period;
  all-or-nothing within the RT band (a reserved thread preempts every
  fixed-priority one, so a band holding both loses the derived ordering);
  scoped to nodes carrying a timing fact; **containers exempt**, since
  `--container-mode isolated` is the default and without the exemption
  nearly every system would hard-error. The parameters ride the portable
  tier head (`budget_us`/`deadline_us`/`period_us`) so a later `up`, which
  reads the model and never the platform file, can rebuild them.
- **Where it runs**: fresh derive on `check --sched [--explain]`,
  `resolve`, `launch`, and `run`. `up <model.yaml>` does **not**
  re-run the mapper — it reads the model's `execution.tiers` +
  `execution.bindings` (see below).
- **What lands in the SystemModel**: only the applied schedule —
  synthesized `TierDef`s + `bindings` (FQN → tier, default tier
  excluded). **No resolved plan is embedded** (`execution.sched` landed
  and was reverted, 2026-07-20 maintainer decision, rlm `f090400`): the
  model is *input*; causality + execution modeling is each consumer's
  job. Mapper identity, chain decomposition, per-path ranks, and
  diagnostics exist only on a fresh derive — hence `up --explain`
  shows degraded `derived((applied): tier ...)` provenance.
- **Apply layer** (play_launch): `--sched-apply off|warn|strict`
  (default `warn`; on `launch`, `up`, and `run`); per-TID
  `sched_setattr(2)` across `/proc/<pid>/task/*` — not
  `sched_setscheduler(2)`, which cannot express `SCHED_DEADLINE`, uclamp or
  any `sched_flags` value — plus `sched_setaffinity(2)` where the policy
  permits one; non-root via the `CAP_SYS_NICE` `play_launch_rt_helper`
  (`play_launch setcap`). Applied to regular nodes, container processes
  (re-applied on respawn), and composable nodes on their LOADED event.

  ```bash
  # Sidecar <stem>.system.posix.yaml shipped next to the launch file
  # (or in the overlay) is discovered automatically:
  play_launch check --sched --explain <pkg> <launch_file>  # derived plan + provenance
  play_launch launch <pkg> <launch_file>                   # derive + apply (warn on failure)
  play_launch launch <pkg> <launch_file> --sched-apply strict   # abort if apply fails

  # Explicit platform file (also the only way to use a legacy .toml):
  play_launch check --sched bringup.system.posix.yaml --explain <pkg> <launch_file>
  ```

### nano-ros (RTOS) — shipped, derived path opt-in

nano-ros pins this repository and consumes the **agnostic core**, never the
posix realizer. Design of record: nano-ros RFC-0050 §"Input model" and
RFC-0052 §"system-model RTOS mapper".

- **Derivation**: `nros-orchestration-ir::mapper_input` still builds
  `MapperInput` itself, from the resolved SystemModel's input layers
  (`structure.nodes`, `contracts.node_paths`, `contracts.pub_endpoints`)
  — never from any embedded plan. It reconstructs the trigger from
  `input: []` plus the first output's `min_rate_hz`, passes no chains and
  reads the criticality label, so it ranks from different facts than
  play_launch does. Its phase-457 replaces the module with a call into
  `derive` ([above](#where-scheduling-facts-come-from)), at which point the
  `min_rate_hz` reconstruction goes and `resolve_chains` gives nano-ros the
  same chains play_launch ranks by. Until then `chain_aware_rank` degrades
  to the criticality-bucketed rate/deadline fallback by construction.
- **Realizer**: `realize_rtos` maps the `RankedPlan` onto per-RTOS
  capabilities (`SchedCaps`: priority count, numbering direction, EDF,
  sporadic reservation, preemption threshold, affinity) for
  posix/Zephyr/FreeRTOS/ThreadX/NuttX, recording per-dimension
  native/backfill/degrade provenance. Its v1 realizes activation,
  urgency, deadline, and budget (e.g. Zephyr native EDF via
  `k_thread_deadline_set`, NuttX `SCHED_SPORADIC` budgets); placement
  and preemption-threshold are modeled in `SchedCaps` with runtime
  support landed, but the realizer does not emit them yet (later
  waves). `RankItem.fine_group` doubles as its executor grouping.
- **Authoring**: nano-ros authors its own `system.toml` (its bringup
  config: `[tiers.*]`, `[[node_overrides]]`, lifecycle, bridges — a
  superset role, ingested via the model's system-config layer, reusing
  `sched::TierDef`). It does **not** yet author v2
  `<stem>.system.<target>.yaml` platform files — its resolver plumbs
  `--sched`, but the workspace sync never passes it. The derived
  (mapper) path activates only when a model carries no declared
  `execution.tiers`.

## Distribution & Cross-Repo Sharing

- **Authored in** this repo and consumed as a **git dependency pinned by
  tag** — there is no vendored copy and no submodule. play_launch pins it
  from three manifests that must name the same tag (naming one tag is what
  makes cargo resolve ONE instance; two revisions would make `SystemModel`
  two incompatible types); nano-ros pins its own. Consumers may sit on
  different tags — check them before assuming API parity, and read
  [CHANGELOG.md](../CHANGELOG.md) for what moved between two.
- **Dependencies:** `serde`, `thiserror`, `toml`, `serde_yaml_ng`, and
  `ros-launch-manifest-types` for the `Duration` spelling. Pure host code;
  no parser, no `check`/`model`/`derive` deps, no runtime deps — which is
  what lets `derive` depend on `sched` and not the reverse.
- **Portability:** generic facts and the ranking core are byte-identical
  across platforms; platform numbers exist only in per-target sub-tables
  (v1), the posix realizer (Linux), or consumer-owned realizers (RTOS).

## Design of Record

- play_launch `docs/superpowers/specs/2026-07-01-shared-scheduling-crate-design.md`
  — v1 / shared-crate design.
- play_launch `docs/superpowers/specs/2026-07-16-rt-config-v2-design.md`
  — v2 derived-scheduling design (Phase 41).
- nano-ros `docs/design/0050-system-model.md`, `0052-system-model-rtos-mapper.md`
  — cross-repo agreement: input-only model, algorithm-shared-not-output,
  per-consumer realizers (2026-07-20, supersedes the earlier
  "scheduling SSoT" direction).

## Typed `posix` Placement (`posix.rs`)

`sched_class: Option<String>` + `priority: i64` + `core: Option<u32>` made
illegal states representable and left every consumer re-deriving which fields
were live from a string. A tier could carry `sched_class: "SCHED_OTHER"` beside
`priority: 10` — a state Linux cannot express, which shipped into
`system_model.yaml` and was fatal under strict mode.

```rust
pub enum PosixSched {
    Idle,
    Batch    { nice: i32 },
    Other    { nice: i32 },
    Fifo     { priority: i32 },
    Rr       { priority: i32 },
    Deadline { runtime_ns: u64, deadline_ns: u64, period_ns: u64, overrun: bool },
}

pub enum PosixAffinity { Inherit, Cpus { cpus: Vec<u32> }, Cpuset { path: String } }

pub struct PosixPlacement {
    pub sched: PosixSched,
    pub affinity: PosixAffinity,
    pub uclamp: Option<(u32, u32)>,
}
```

Each variant names only the parameters that policy actually has, so `Batch` has
no priority, `Fifo` has no nice, and `Deadline` has neither — it carries a
reservation. `PosixAffinity` is an enum rather than two optional fields because
a CPU mask and a cpuset are mutually exclusive *and* policy-dependent:
`SCHED_DEADLINE` may not use `sched_setaffinity(2)` at all, since a deadline
thread's affinity may not be narrower than the root domain it was created on
(`EPERM`). `PosixPlacement::validate` rejects the rest.

Two consequences worth stating, because both correspond to shipped defects:

- **`PosixSched::priority()` returns `None` for `Deadline`** — absent, not
  zero. A deadline thread preempts every fixed-priority thread regardless of RT
  priority, which is why the kernel gives it a reservation instead of a number.
  `band_violations` skips such tiers rather than comparing them against the RT
  band, the same way it skips the default tier.
- **`requires_reset_on_fork()` is a method, not a field**, and true only for
  `Deadline`. `SCHED_FLAG_RESET_ON_FORK` reads as hygiene but the kernel resets
  scheduling in `sched_fork()`, which runs for *thread* creation as well — so
  setting it on `SCHED_FIFO` stops threads created after an apply sweep from
  inheriting the policy, leaving an arbitrary subset of a node's threads at
  `SCHED_OTHER`. Measured downstream, not theorised.

`overrun` is `SCHED_FLAG_DL_OVERRUN` (deliver `SIGXCPU` on an exhausted
reservation), set from `deadline_policy: fault`. The enum serializes with an
internal `policy:` tag in `snake_case`.

`ResolvedTier::posix: Option<PosixPlacement>` is **additive**: `sched_class`,
`priority` and `core` remain for one release so consumers migrate without a
lockstep bump. When present, the typed placement is authoritative. It is
filled by the v1 `resolve()` path for `posix`/`native` targets and by
`chain_aware`; `rate_monotonic` and `deadline_monotonic` leave it `None`
([above](#an-exact-tie-is-a-fact-and-every-mapper-treats-it-the-same-way)),
and no mapper in this crate ever emits `Deadline` — that is the consumer's
derivation ([Consumers](#consumers)).
