# Contract Theory for Launch Manifests

*This is the formal theory behind manifest timing contracts. For the
manifest format itself, see [launch-manifest.md](launch-manifest.md).*

## Motivation

Consider a perception pipeline with five nodes:

```
sensor → cropbox (5ms) → ground_filter (15ms) → detector (30ms) → tracker (20ms)
```

The pipeline has a 100ms latency budget. Today it takes 70ms — plenty of
headroom. Then a developer upgrades the detector model. It now takes 45ms.
The pipeline takes 85ms — still under budget. But add one more node to the
chain, or a bad scheduling day, and you silently break the 100ms contract.

**Without contracts**, you discover this at integration time (or in
production). **With contracts**, each node declares its own budget, and the
checker verifies that the sum fits the scope budget — at authoring time,
before any code runs.

The manifest's contract system makes these budgets explicit, composable,
and statically checkable. This document describes the formal foundations.

For the manifest format itself, see [launch-manifest.md](launch-manifest.md).

## Notation

Symbols used throughout this document:

| Symbol | Meaning |
|--------|---------|
| $C = (A, G)$ | Contract: assumption $A$ + guarantee $G$ |
| $M$ | Component (a node or scope that satisfies a contract) |
| $L_{\text{node}}(X)$ | Worst-case processing time of node $X$ (from `max_latency`) |
| $L_{\text{transport}}(X \to Y)$ | Worst-case transport time between nodes $X$ and $Y$ (from topic's `max_transport`; 0 when omitted) |
| $L_{\max}$, $L_{\min}$ | Worst / best case end-to-end latency of a path or scope |
| $A_{\max}$ | Maximum data age at a subscriber (ms) — runtime checked via `max_age` |
| $f$ | Frequency (Hz) |
| $P$ | Timer period (ms) |
| $J$ | Timer jitter — max deviation from the ideal period (ms) |
| $S$ | Sampling cost of a route: one full period per timer boundary it crosses |
| $d$ | Drop rate: fraction of messages lost, $n/w$ from `drop.max_count`, range 0-1 |
| $\mathcal{R}$ | Delivery rate: $\mathcal{R} = 1 - d$ (fraction that survives) |
| $K$ | Max consecutive drops (from `drop.max_consecutive`) |
| $\ell_{\max}$ | Observed longest consecutive drop run (runtime) |
| $T_{\text{FTTI}}$ | Fault-tolerant time interval — fault to hazardous event, absent any reaction (declared, `hazards.<h>.ftti`) |
| $T_{\text{FDTI}}$ | Fault-detection time interval — derived from the guards' detectors |
| $T_{\text{FRTI}}$ | Fault-reaction time interval — derived from the reaction route plus the plant's settle |
| `budget-overflow` | Verification check: descendant budget exceeds ancestor budget (error) |
| `scope-budget` | Verification check: sum of children exceeds scope budget (warning) |

## What is a Contract?

A **contract** is a pair $C = (A, G)$:

- **Assumption** ($A$) — constraints on inputs that must hold for the
  guarantee to be valid. "I need sensor data at 10 Hz or faster."
- **Guarantee** ($G$) — constraints on outputs that the component
  promises, given the assumption holds. "I will produce a result within
  30ms."

A component **satisfies** its contract when: if the assumption holds
on the inputs, the guarantee holds on the outputs. "If you give me what
I asked for, I'll deliver what I promised." *(Formally: $M \cap A \subseteq G$
— the component's behaviors, intersected with the assumed inputs, are
all within the guaranteed outputs.)*

### Three Levels

The manifest defines contracts at three levels. To make this concrete,
consider a perception pipeline:

```
pointcloud → [cropbox: 5ms] → [ground_filter: 15ms] → [detector: 30ms]
```

**Topic contract** — for the channel between cropbox and ground_filter:
- *Assumption:* cropbox publishes at ≥ 10 Hz
- *Guarantee:* the channel delivers at 10 Hz with `drop: 1 / 100`

**Node contract** — for ground_filter:
- *Assumption:* receives filtered points at ≥ 10 Hz
- *Guarantee:* output within 15ms

**Scope contract** — for the whole perception pipeline:
- *Assumption:* parent scope wires the pointcloud input
- *Guarantee:* end-to-end latency ≤ 50ms

Summary:

| Level | What it describes | Assumption | Guarantee |
|-------|-------------------|------------|-----------|
| **Topic** | A communication channel | Publisher produces at `rate_hz` | Channel delivers at `rate_hz` with drops ≤ `drop.max_count` |
| **Node** | A single computation | Inputs arrive per `min_rate_hz`, `state`, `required` | Output within `max_latency` |
| **Scope** | An entire launch file | Topic declarations consistent across tree | E2E `max_latency`, `drop:` |

These compose hierarchically: topic contracts constrain the channels,
node contracts describe per-node timing, scope contracts abstract the
internal graph into an end-to-end budget.

### Example: NDT Scan Matcher

Now that we've seen the three levels, let's look at one node's contract
in detail — both the YAML declaration and its formal interpretation:

```yaml
nodes:
  ndt_scan_matcher:
    sub:
      input_points:
        min_rate_hz: 10
      initial_pose:
        state: true
        required: true
      map:
        state: true
        required: true
    pub:
      ndt_pose:
        min_rate_hz: 10
    paths:
      main:
        trigger:
          input: [input_points]
        output: [ndt_pose]
        max_latency: 30ms
```

(From `tests/fixtures/manifest_ndt/manifest.yaml`. `trigger:` is the
canonical spelling of the causal fact; the bare `input:` list it replaced
still parses, and `explicit-trigger` emits an info wherever a path has no
explicit trigger.)

**Assumption** ($A$):
- `input_points` arrives at $f \geq 10$ Hz (the causal trigger)
- `map` and `initial_pose` have each been received at least once
  (`required`) and are polled rather than reacted to (`state`)

**Guarantee** ($G$):
- `ndt_pose` published at $f \geq 10$ Hz
- $L_{\max} \leq 30$ ms (trigger to output)

(Drops are declared on the topics that carry `ndt_pose`, not on the
node path — see [Drop Budgets](#drop-budgets).)

The **assumption/guarantee separation** is what makes contracts useful
for diagnosis. At runtime:

| Assumption | Guarantee | Diagnosis |
|------------|-----------|-----------|
| met | met | Nominal |
| met | violated | **Node bug** — computation exceeds its declared budget |
| violated | met | Upstream problem, but this node is robust |
| violated | violated | Upstream problem — not this node's fault |

## Composition

A single node's contract says what it promises in isolation. But a
pipeline's guarantee depends on how nodes are connected. The composition
rules below show how to compute the pipeline's end-to-end latency
from the individual node contracts.

### Understanding Worst-Case Latency

A message traverses a pipeline by passing through nodes and the
transport channels (topics) between them. At each stage, time is spent:

- **Node processing** — the time a node takes to receive an input,
  compute, and publish the output. This is the node's `max_latency`.
- **Transport** — the time for a published message to reach the next
  subscriber via DDS. On the same machine this is typically < 1ms;
  across machines it depends on the network.

The **worst-case latency** of a path is the total time assuming every
stage takes its maximum. This is a pessimistic bound — the real latency
is usually lower, but the bound is what the contract guarantees.

We write:
- $L_{\text{node}}(X)$ — worst-case processing time of node $X$
  (from the node's `max_latency`)
- $L_{\text{transport}}(X \to Y)$ — worst-case transport time between
  node $X$ and node $Y$ (from the topic's `max_transport`; 0 when
  omitted)

### Series (Pipeline)

Nodes connected in sequence. Each message passes through every node,
spending time in processing and transport at each hop:

```
in → [A: 5ms] —transport→ [B: 15ms] —transport→ [C: 30ms] → out
```

The worst case is when every node and every transport takes its maximum
time. Latencies simply add up:

$$L_{\max} = L_{\text{node}}(A) + L_{\text{transport}}(A \to B) + L_{\text{node}}(B) + L_{\text{transport}}(B \to C) + L_{\text{node}}(C)$$

In this example with same-machine transport (~0):

$$5 + 0 + 15 + 0 + 30 = 50 \text{ ms}$$

**Why it's the sum:** the message cannot be in two nodes at once. It
arrives at A, waits for A to finish, travels to B, waits for B to
finish, and so on. Each delay is sequential.

**Transport is declared per topic.** Each topic can declare
`max_transport` — the worst-case time for a published message to
reach the subscriber via DDS. Topics without `max_transport`
contribute 0 to the budget sum; their transport is absorbed into the
scope's residual headroom. On the same machine, transport is typically
< 1ms and can be omitted. For cross-machine hops (sensor ECUs, network
bridges), declare `max_transport` to make the budget explicit.

**Age accumulates** along the chain — each node and transport hop adds
to the total time since the original sensor reading. Age is checked at
runtime on subscriber endpoints (`max_age`), not statically composed.
See [Data Age](#data-age).

### Parallel (Fork-Join)

Two branches merging at a fusion node. The fusion node waits for inputs
from both branches before producing output:

```
      ┌→ A (50ms) →┐
in →  │             ├→ C (20ms) → out
      └→ B (30ms) →┘
```

The worst case: the **slower branch** determines how long the fusion
node waits. Then add the fusion node's own processing time:

$$L_{\max} = \max(L_{\max}(\text{branch } A),\; L_{\max}(\text{branch } B)) + L_{\text{node}}(C)$$

where $L_{\max}(\text{branch } A)$ is the end-to-end latency of branch A
— which may itself be a series pipeline of multiple nodes.

Example: $\max(50, 30) + 20 = 70$ ms — `tests/fixtures/manifest_parallel_pipeline/`,
where the flat sum a topology-unaware check takes would be 100. The
route derivation is fork-join correct by construction: `critical_path`
(`derive/src/graph.rs`) is a forward DP over the path-level graph that
sums along a branch and takes the **max** at a join, and a test pins this
example at 70.

Note: the best-case latency $L_{\min}$ also uses $\max$ — the fusion
barrier waits for the slowest branch even when both are fast.

**Rate depends on `sync:`, and the two cases differ by more than a
detail.** With `sync:` the fusion node emits one output per matched set, so
it is paced by its slowest input: $f = \min(f_A, f_B)$. Without `sync:` the
callback fires once per message on *each* topic it is registered for, so the
output rate is the **sum**: $f = f_A + f_B$. Taking the min in both cases is
the natural-looking mistake, and it understates a fan-in node's load by
exactly the factor that decides whether it fits. This is the rule
`derive_topic_rates` applies (`resolve/src/ros/manifest_graph.rs`) — see
[Derived Quantities](#derived-quantities).

**Age depends on whether the inputs are synchronised (`sync:`):**

- With `sync:` — the output stamp is the **oldest** input stamp of the
  matched set. Age = max of branch ages + fusion processing:

$$A_{\max} = \max(A_{\max}(\text{branch } A),\; A_{\max}(\text{branch } B)) + L_{\text{node}}(C)$$

- Without `sync:` — the output stamp is the **triggering input's** stamp;
  `state: true` inputs are polled. Age follows only the triggering branch:

$$A_{\max} = A_{\max}(\text{primary branch}) + L_{\text{node}}(C)$$

The unsynchronised form is how most Autoware fusion nodes work: one causal
input triggers the callback, the rest are polled state. The output inherits
the triggering input's provenance.

Drop at the barrier is **not composed statically** — the user declares
the fusion node's observed drops directly, combining all causes
(upstream propagation, correlation mismatch, computation) into one value.

### Periodic (Timer-Driven)

A timer-driven node runs at fixed intervals (period $P$), reading the
latest state from a buffer. It does not react to individual messages —
it wakes up on the timer and processes whatever is available.

```
upstream → [state buffer] → [periodic node, P=100ms, J=5ms] → out
```

**Worst-case latency through a periodic node.** The worst case happens
when new data arrives just *after* the timer fires. The data sits in the
buffer for nearly one full period before the next timer tick processes it:

$$L_{\max}(\text{periodic}) = P + J + L_{\text{node}}(\text{periodic})$$

Where:
- $P$ — one full timer period of waiting (worst case)
- $J$ — timer jitter (the timer itself may be late)
- $L_{\text{node}}(\text{periodic})$ — the periodic node's processing time

Upstream processing time is **not** included — it is already accounted
for by the upstream node's own `max_latency` in series composition.
The periodic node's contribution to the chain is the buffer wait plus
its own processing.

**Best case:** the timer fires right as data arrives (zero wait):

$$L_{\min}(\text{periodic}) = L_{\text{node}}(\text{periodic})$$

**Rate is independent of upstream:** $f = 1000 / P$ (where $P$ is in ms). The periodic node
produces output at its own timer rate regardless of how fast or slow
the upstream is.

**Periodic nodes reset the consecutive drop chain.** Upstream
consecutive drops don't propagate because the timer fires regardless
of whether new data arrived. Each segment (before and after the
periodic node) is checked independently.

### Drop Budgets

Drops are declared in a `drop:` block — `max_count: N / W`, from which
the drop rate $d = n/w$ is computed, and `max_consecutive: K`. It sits
on **topics** (transport drops) and **scope paths** (E2E drops). The
shared path schema accepts it on node paths too; it is range-validated
like any other, but plays no role in composition — if a node internally
drops messages, model the effect as a lower `pub.min_rate_hz` on its
output.

**Static checking** (`drop-sanity`) validates local consistency only:

- Values in range: $0 \leq d \leq 1$, `n \leq w` in `"N / W"` counts,
  `max_consecutive` a positive integer
- Rate-drop compatibility: a topic's effective delivery rate must meet
  subscriber demand:

$$f_{\text{topic}} \cdot (1 - d_{\text{topic}}) \geq f_{\min}(\text{sub})$$

Cross-scope, two scopes declaring the same topic must agree on its drop
budget — the `consistency` rule, which runs in the consumer's merge layer
(`resolve/src/ros/manifest_loader.rs`), not in this crate's registry. A
scope-vs-topic tightness check (a scope's
`drop.max_count` must not be tighter than a topic's on its path — part >
whole) is part of the design but not currently implemented.

**Runtime monitoring** handles composition — it depends on actual
transport conditions (burstiness, congestion) that cannot be proven
statically. The runtime rule engine checks the observed delivery ratio
against `drop.max_count`; `max_consecutive` checking and burstiness
detection are designed but not yet implemented. See
[Burstiness](#burstiness) for the detection metrics and Appendix A for
the underlying theory.

### Composition Summary

| Topology | Latency | Rate | Age | Drop |
|----------|---------|------|-----|------|
| **Series** | sum of nodes + transport | preserved | sum along chain | runtime monitoring |
| **Parallel, with `sync:`** | max(branches) + fusion | $\min$ of branches | max(branches) + fusion | runtime monitoring |
| **Parallel, without `sync:`** | max(branches) + fusion | **sum** of branches | triggering branch + fusion | runtime monitoring |
| **Periodic** | $P$ + $J$ + node | $1000/P$ (independent) | resets stamp chain | runtime monitoring |

## Verification Rules

The checker verifies that declared budgets are consistent across the
scope tree. Two separate checks apply to latency, drop, and age — each
with property-specific composition math but the same structural rules.

*As implemented:* Check 1 corresponds to the cross-scope
`budget-overflow` rule and Check 2 to the `scope-budget` rule, each a
narrower slice of the theory below. `budget-overflow` compares
**scope-path budgets only**: a child scope's path against an ancestor
scope's path with matching input/output endpoints — node
`max_latency` is not compared against scope budgets by any current
rule. For Check 2, the single-manifest checker
(`check/src/rules/scope_budget.rs`) runs a conservative flat sum: every
node contributes the **maximum** over its declared paths, plus declared
topic `max_transport` (inline includes included, external includes
skipped). The consumer's cross-scope layer
(`resolve/src/ros/manifest_loader.rs`) computes a topology-aware critical
path over the merged tree and then **deletes** the per-manifest warning
for every scope path a route was found for, so one path never carries two
different totals; a path with no traceable route keeps the flat sum,
which is then the only estimate there is. The residual INFO reporting
described below is design, not yet emitted. See
[contract-verification.md](contract-verification.md) for the full rule
inventory and where each rule runs.

For precise measurement point definitions, see
[Latency and Data Freshness](launch-manifest.md#latency-and-data-freshness) in the manifest spec.

### Opaque vs Transparent Scopes

A scope's behavior in the budget check depends on whether it declares
its own budget:

- **Opaque** (has budget declared): the parent uses the declared value.
  The scope is a black box — its internal decomposition is its own
  responsibility.
- **Transparent** (no budget declared): the parent looks through it
  and sees the children directly. It's just organizational grouping
  (namespacing), not a timing boundary.

### Check 1: Budget Overflow (Error)

No descendant's budget may exceed any ancestor's budget. A node with
20ms inside a scope with 10ms is always wrong — the part cannot be
bigger than the whole, regardless of transport or topology.

The checker walks the scope tree from root to leaves, tracking the
tightest ancestor budget seen so far. At each node or scope with a
declared budget, it verifies the budget fits within the ancestor's.
Transparent scopes (no budget) don't tighten the constraint — the
walk passes through them.

```
Implementation sketch:

check_overflow(node, ancestor_budget):
  if node.budget > ancestor_budget → ERROR
  effective = min(node.budget, ancestor_budget)
  for each child:
    check_overflow(child, effective)
```

This check is **topology-unaware** — it doesn't distinguish series from
parallel. For parallel branches, each branch is individually less than
the scope budget (since max(branches) < scope is a weaker constraint
than sum). The overflow check catches the trivially wrong case; the
sum check (below) catches composition errors.

### Check 2: Budget Sum (Warning)

For each scope with a declared budget, the sum of its direct children's
budgets must fit within the scope budget. Children without budgets
contribute to the **residual** — the unallocated portion of the scope
budget that covers transport, scheduling variance, and undeclared nodes.

For transparent scopes (no budget), the parent looks through and
collects grandchildren directly.

```
Implementation sketch:

check_sum(scope):
  if scope has no budget → skip
  declared_nodes = collect children with budgets (look through transparent scopes)
  undeclared_nodes = children without budgets
  declared_transport = sum of max_transport on topics within scope (0 when omitted)
  undeclared_transport_count = topics without max_transport
  total = sum(declared_nodes) + declared_transport
  if total > scope.budget → WARNING
  residual = scope.budget - total
  if undeclared_nodes or undeclared_transport_count:
    → INFO: "Xms residual across N undeclared nodes and M topics without transport budget"
```

The sum check is a **warning** (not an error) because the sum is a lower
bound — topics without `max_transport` contribute 0. The gap between
the sum and the scope budget is the allowance for undeclared transport,
scheduling variance, and undeclared nodes.

Like the overflow check, the sum check is **topology-unaware**. For
parallel branches, the sum is conservative (sum > max), so the check
may warn even when the actual composition fits. This is the right
direction — a false warning is better than a missed violation.

### Latency Example

```
scope S: max_latency: 100ms
  ├── sub-scope P: max_latency: 50ms    (opaque)
  │     ├── node A: max_latency: 20ms
  │     ├── topic T1: max_transport: 2ms  (A → B)
  │     └── node B: max_latency: 25ms
  ├── sub-scope Q: (no budget)            (transparent)
  │     └── node C: max_latency: 30ms
  ├── topic T2: (no transport budget)     (P → C)
  ├── topic T3: (no transport budget)     (C → E)
  └── node E: (no budget)
```

**Overflow check** — walk the tree with tightest ancestor:
- A(20) ≤ min(S:100, P:50) = 50. OK.
- B(25) ≤ 50. OK.
- C(30) ≤ min(S:100) = 100. OK. (Q has no budget, doesn't tighten.)

**Sum check on P** (opaque, checks its own children):
- A(20) + T1(2) + B(25) = 47 ≤ 50. Passes. 3ms residual.

**Sum check on S** (looks through transparent Q):
- P(50, opaque) + C(30, from Q look-through) = 80. E is undeclared.
  T2 and T3 have no transport budget (contribute 0).
- 80 ≤ 100. Passes. Residual: 20ms across 1 undeclared node (E) and
  2 topics without transport budget.

If E later gets a budget of 25ms: 50 + 30 + 25 = 105 > 100. Warning.

### Drop Example

Drop budgets are checked statically for local consistency (sanity) and
at runtime for actual behavior. Composition is defined over **topics**
(transport) and **scope paths** (E2E); a node path's own `drop:` is
range-validated but composes nothing.

```
scope S path: drop: { max_count: 10 / 100 }   → d = 0.10
  topic T1:   drop: { max_count:  3 / 100 }   → d = 0.03  (A → B transport)
  topic T2:   drop: { max_count:  3 / 100 }   → d = 0.03  (B → C transport)
  topic T3:   (no drop budget)                            (C → D transport)
```

**Sanity check**: T1 drop (3%) < S drop (10%). T2 (3%) < 10%. OK.
No topic has a tighter drop budget than the scope.

**Rate-drop check**: if T1 has `rate_hz: 10` and subscriber B has
`min_rate_hz: 9`: effective delivery = $10 \times (1 - 0.03) = 9.7$ Hz ≥ 9. Passes.

**Runtime monitoring** observes actual E2E drop rates and checks them
against the scope's $d_s = 0.10$ and its `max_consecutive` (if
declared). See [Burstiness](#burstiness) and Appendix A for the theory.

### Partial Decomposition

A scope contract is valid without full node-level decomposition. Three
scenarios:

| Scope budget | Node budgets | Checker behavior |
|-------------|-------------|-----------------|
| Declared | All declared | Verify composition ≤ scope, check overflow |
| Declared | Some missing | Verify declared fit, report residual (latency) |
| Declared | None declared | Accept — runtime monitoring checks E2E |

This supports a **top-down workflow**: start with the E2E requirement,
fill in node budgets as you measure them. The residual (or gap report)
tells you what's unaccounted for.

## Data Age

The **age** of a message at a subscriber is the time since the original
sensor data was created:

$$\text{age} = t_{\text{take}} - \text{header.stamp}(\text{source})$$

This works because causal paths preserve `header.stamp` through the
chain — each node copies the input stamp to the output (see
[Timestamps and Data Flow](launch-manifest.md#timestamps-and-data-flow)).

**`max_age`** is declared on **subscriber endpoints**, not on paths.
It constrains data freshness at the point of consumption:

```yaml
nodes:
  planner:
    sub:
      objects:
        max_age: 200ms          # data must be fresher than 200ms
```

**Runtime checking:** the interception layer reads `header.stamp` on
every `rcl_take` and compares to current time. If
`now - stamp > max_age`, a violation is flagged.

**Static checking** does not trace the full causal chain (which would
require every upstream node to have a latency budget). A local
feasibility check — subscriber `max_age` vs the `max_latency` of
the scope path feeding it — is possible in principle but not currently
implemented. Two other rules do read the declaration: `lifespan-age`
(cross-scope) rejects a `max_age` longer than the topic's `qos.lifespan`,
since DDS has already discarded a sample that old and no runtime behaviour
can satisfy both; and `max_age` is one of the mechanisms
[FDTI](#fdti--detection) counts when a subscriber declares an
`on_violation` for a `late` fault.

**For multi-input nodes:** the age at a subscriber depends on whether
the inputs are synchronised. With `sync:`, age reflects the oldest input
of the matched set. Without it, age follows the triggering input only. See
[Parallel composition](#parallel-fork-join) for the formulas.

## Cross-Scope Chains and Sampling Cost

Scope paths compose budgets *within* one scope subtree, and the same
spelling names an end-to-end requirement that crosses scope boundaries:
two ends (`trigger: { input: [...] }` and `output:`) plus one E2E
`max_latency`. The cause-effect sequence between them is DERIVED from
the `trigger:`/`output:` facts the nodes already declare.

An earlier vocabulary wrote that sequence by hand — `chains:`, an
alternating list of `{scope, path}` segments joined by `via:` topics,
with a `reaction`/`age` semantics tag. It was removed in phase 68 W4: a
written route is a second copy of the graph, and the `chain-link` rule
existed only to catch the two disagreeing.

A derived route's hops divide into two kinds, following the periodic
composition rule above:

- **Causal hops** — runs of input-triggered paths. A message flows
  through them; their latency contributions add as in series
  composition (and a fork-join contributes `max` over its branches, not
  a sum).
- **Boundaries** — timer-triggered paths. A message arriving at an
  arbitrary point in the period waits up to a whole period for the
  callback that forwards it, so traversing boundary $i$ costs
  $P_i + C_i$: one full period plus the boundary's own processing
  (`traversal_latency_ms`, `derive/src/view.rs`).

**Of that, only the period is beyond scheduling's reach**, and the
route's **sampling cost** is therefore the period term alone:

$$S = \sum_{i \in \text{boundaries}} P_i \qquad P_i = 1000 / \texttt{rate\_hz}_i$$

summed over the boundaries of the *winning* route, not of the subgraph
(`sampling_cost_ms` in `derive/src/view.rs`, accumulated along the
back-walk from the sink in `derive/src/graph.rs`, and ported in
`resolve/src/ros/manifest_graph.rs`; a test pins it as one period per
boundary). Two cross-scope rules read $S$, and both state something no
priority assignment can fix:

- **`scope-sampling-feasibility`** — $S \geq L_{\text{budget}}$. The
  route spends its whole budget waiting for clocks before any callback
  runs. This is a different and worse claim than a total over budget, so
  it is emitted **before** `scope-budget`: the budget warning necessarily
  fires too, and on its own it invites an author to optimise callbacks
  that are not the problem.
- **`jitter-feasibility`** — $S > \texttt{max\_jitter}$. A clock crossing
  contributes its whole period to end-to-end *variation* as well, whatever
  the callback costs: a message arriving just after a tick waits a full
  period, one arriving just before waits none. This is the half of the
  jitter requirement that needs no best-case fact, unlike `jitter-range`
  (below), which needs a declared `min_latency`.

`scope-budget` compares the route's full total — causal hops, declared
transport, and $\sum (P_i + C_i)$ at the boundaries — against the declared
`max_latency`, and names the
sampling term separately in its message, because that is the part of the
overrun the author cannot schedule away.

The scheduling mapper applies the same distinction one level up, on the
chain it derives from the scope path: `chain_feasibility`
(`sched/src/chain_aware_mapper.rs`) sums $P_i + C_i$ over the chain's
boundaries and calls the chain infeasible when

$$L_{\text{controllable}} = L_{\text{budget}} - \sum_{i \in \text{boundaries}} (P_i + C_i) \leq 0$$

excluding it from priority shaping with a `ChainInfeasible` warning; its
members keep their local-fact priorities. An absent $C_i$ is counted as
zero — there is nothing better to count — but the absence is *recorded*
and reported as `ChainFeasibleWithoutWcet`, because a verdict that cannot
tell absent from zero claims headroom nobody measured. The same facts
drive priority derivation — see [scheduling.md](scheduling.md).

## Derived Quantities

A contract states **facts** (what the code does) and **requirements** (what
it must achieve). Anything computable from those two is a **consequence**,
and a consequence written by hand is a second copy of something the tool
already knows — two copies that can disagree.
[format-reference.md](format-reference.md) carries the classification per
field in its `kind` column; this section is the arithmetic behind the
consequences.

### Rate Propagation

A topic's publication rate is derived by propagating from the timer paths
that ultimately drive it (`derive_topic_rates`,
`resolve/src/ros/manifest_graph.rs`):

- **Timer path** → its own `trigger.timer.rate_hz`. This is the only
  source; nothing else creates messages.
- **Input-triggered, no `sync:`** → the **sum** of its inputs' rates. A
  subscription callback fires once per message on *each* topic it is
  registered for, so a path triggered by two 10 Hz topics runs 20 times a
  second.
- **Input-triggered, with `sync:`** → the **min**. A synchronizer emits one
  output per matched set, so it is paced by its slowest input.
- **Several paths producing one endpoint, or several nodes publishing one
  topic** → the sum, for the same reason: they publish independently.

$$f_{\text{out}} = \begin{cases}
\texttt{rate\_hz} & \text{timer} \\
\min_i f_i & \text{input, with \texttt{sync:}} \\
\sum_i f_i & \text{input, without \texttt{sync:}}
\end{cases}$$

Taking the min in both input cases is the natural-looking mistake, and it
understates a fan-in node's load by exactly the factor that decides whether
it fits.

A `once` or `spontaneous` trigger, an unclassified path, an externally
driven publisher and a feedback cycle each yield **`Unknown` with a
reason**, never 0 — and any unknown contributor makes the whole sum unknown,
because a partial sum would be a lower bound presented as a rate. A topic
whose rate is `Unknown` gets no verdict and its declared value stands: a
contract is then the only place that number can come from.

Where the rate *is* derivable, the declaration is graded against it:

| Verdict | Severity | Condition |
|---------|----------|-----------|
| `derivable-rate` | Info | declared `topics.<t>.rate_hz` equals the derived rate — a deletable copy |
| `rate-mismatch` | Warning | they disagree (relative tolerance $10^{-6}$, since a derived rate is a quotient and an authored one a round number) |
| `derivable-min-rate` | Info | a publisher's `min_rate_hz` equals the derived rate, attributed only where the topic has exactly **one** publisher (with several the derived rate is their sum, and dividing it back out would present a bound as a rate) |
| `min-rate-mismatch` | Warning | a publisher's `min_rate_hz` **exceeds** the derived rate — it guarantees more than the timers driving it can produce. A promise *below* the derived rate is a loose but true bound and gets nothing |
| `derived-rate-hierarchy` | Warning | a subscriber's `min_rate_hz` exceeds the derived rate. The subscriber side is a requirement, not a copy, so it is never "derivable" — but deleting the declared topic rate would otherwise leave it unchecked |

### Rate Hierarchy

The declared form is checked in **both** directions
(`check/src/rules/rate_hierarchy.rs`, all errors):

$$\texttt{pub.min\_rate\_hz} \;\geq\; \texttt{topic.rate\_hz} \;\geq\; \max_{\text{sub}} \texttt{sub.min\_rate\_hz}$$

$$\texttt{pub.max\_rate\_hz} \;\geq\; \texttt{topic.rate\_hz} \qquad \texttt{topic.rate\_hz} \;\leq\; \min_{\text{sub}} \texttt{sub.max\_rate\_hz}$$

The upper bounds matter for queue overrun, which is the failure the lower
bounds cannot see: a topic faster than a subscriber declares it can drain
backs up regardless of scheduling.

### Jitter

`max_jitter` is a requirement on the **spread** of a path's latency, and
`min_latency` exists so that it can be falsifiable — every other bound in
the vocabulary is an upper one. Endpoint-level `jitter` was removed in
phase 68: what destabilises a controller is how much the end-to-end latency
varies, which one publisher's spread does not determine.

`jitter-range` (`check/src/rules/jitter_range.rs`) has three verdicts:

- `min_latency > max_latency` — a contradiction, error.
- both declared and $L_{\max} - L_{\min} > \texttt{max\_jitter}$ — the
  declarations cannot all hold, error.
- `max_jitter` declared, `max_latency` above it, `min_latency` **absent** —
  the bound is unverifiable from declarations, **info**. An absent floor is
  not a floor of zero: an upper bound of 40ms says nothing about whether the
  latencies cluster at 38..40ms or range over 0..40ms. `play_launch measure`
  produces the floor.

A `max_latency` at or below `max_jitter` needs no floor at all — whatever
it is, the spread cannot exceed the ceiling — and is clean. The sampling
half of the same requirement is `jitter-feasibility`, above, which needs no
best-case fact.

### Criticality

Criticality is a consequence of the declared hazards, not a property of a
component: severity is allocated *inward* from an outcome, the way every
safety standard does it (`derive_criticality_from_hazards`,
`resolve/src/ros/manifest_loader.rs`). A node takes a hazard's severity when
it

- **feeds** it — publishes a guard topic, or lies in the upstream causal
  closure of one. State edges are included: a stale map produces a hazardous
  plan as surely as a stale scan does;
- **detects** it — subscribes to a guard and declares an `on_violation`;
- **reacts** to it — lies on the reaction walk to the safe state.

Over several hazards the node takes the **max**, never a sum.

`severity_levels:` declares the scale, ascending, defaulting to ISO 26262's
`[QM, ASIL_A, ASIL_B, ASIL_C, ASIL_D]`; its first entry derives no
criticality. A `hazards.<h>.severity` outside the scale is
`severity-unknown`, not a silent `None`. The scale folds into the mapper's
three buckets by rank: entry 0 is no requirement, and the rest split evenly
with the top third to `High` — on the default scale ASIL_A → low,
ASIL_B → medium, ASIL_C and ASIL_D → high.

The label a node may still carry is graded against the derivation:
`derivable-criticality` (info) when they agree — the label is redundant —
and `criticality-mismatch` (warning) when they do not, the derivation
winning for scheduling. A node no hazard reaches derives nothing and its
label stands: that is the underivable case, and it is why `criticality`
remains in the vocabulary as a consequence rather than being deleted.

## Fault Detection and Reaction

A rate floor says a rate must hold. It does not say what happens when it
does not, or how fast that must be noticed — and ISO 26262 requires both.
The vocabulary adds exactly one requirement, one reaction edge and one
fact, and derives the rest:

- **`hazards.<h>`** — `guards:` (topics watched; a bare name is one guard,
  `{ all_of: [...] }` a redundant set), `on:` (the fault class:
  `omission | late | loss | reported`), **`ftti:`** (the requirement), and
  `reaction:` naming the scope path — or, since phase 75, the mode — that
  reaches the safe state.
- **`sub.<e>.on_violation`** — `{ on, reaction, within, mechanism }` on the
  subscriber that detects. This is the `cmd_vel` timeout of every mobile
  base, written down.
- **`paths.<p>.safe_state`** — `{ emits, settle }`: what the reaction
  commands, and how long the plant takes to get there. Measured, not
  authored.

The budget is the interval declared on the hazard:

$$T_{\text{FDTI}} + T_{\text{FRTI}} \;\leq\; T_{\text{FTTI}}$$

emitted as `fault-reaction-budget` — an error when it fails, an info naming
the slack when it holds, and in both cases naming every term.

### FDTI — detection

Per guard group, the detection interval is the **fastest detector among the
subscribers that REACT**: a subscriber that notices and does nothing has
not detected anything the system can use, so one without an `on_violation`
is not counted. A subscriber detects when any of its mechanisms fires, so
its own interval is the **min** over them
(`detector_interval_ms`, `resolve/src/ros/manifest_loader.rs`):

| Fault class | Mechanism read |
|-------------|----------------|
| `omission` | `qos.lease_duration` |
| `late` | `qos.deadline`, `sub.max_age` |
| `loss` | `drop.max_consecutive` × the topic's period |
| `reported` | the guard **is** a detector's output: its period + the publishing path's `max_latency` |

**A rate floor is not a detector.** `min_rate_hz` is a requirement;
nothing fires when a period passes unless a QoS deadline or an application
watchdog is declared. Counting the period here made a 50 Hz floor "detect"
a dead lidar in 20ms while the real lease was 100ms. A guard with no
reacting detector is `hazard-unguarded` (error) — nothing would ever
notice.

Within an `all_of` group the fault is the loss of *every* member, so the
group is detected when the **last** one is noticed gone — the slowest
member. Across groups the **worst** group governs, because the budget must
hold for whichever one faults.

### FRTI — reaction

The reaction time is a walk over **reaction edges**, not the critical path
(`walk_reaction`, same file). The distinction is arithmetic, not
presentation: the guard's publisher is the thing that failed, so a critical
path through the normal graph charges a clock boundary that will never tick
again and the nominal callbacks rather than the reactions.

The walk starts at the guard, where only a subscriber with an
`on_violation` moves — the nominal path there is waiting for a message that
will not come. From the first reaction onward it follows `on_violation`
where one is declared and otherwise the ordinary input-triggered paths,
because a reaction is a real message and downstream nodes forward it the
way they forward anything. A fork-join takes the **longest branch**. The
walk ends at a path whose `safe_state.emits` publishes onto the hazard's
reaction sink, and

$$T_{\text{FRTI}} = \sum_{\text{hops on the longest branch}} \texttt{max\_latency} \;+\; \texttt{safe\_state.settle}$$

Missing evidence is reported rather than assumed: no route and no declared
budget gives `reaction-unbudgeted` (warning, "the FTTI check runs on
INCOMPLETE EVIDENCE"), a reaction nothing would run gives
`reaction-unreachable` (error), and a reaction sink no subscriber guards
gives `reaction-unguarded` (warning) — a stalled reaction would otherwise
go unnoticed. `reaction-within` checks a subscriber's declared `within`
against its own reaction path's `max_latency`.

### Operational Modes

A hazard's `reaction:` may name a **mode**, and then the fallback ladder is
the reaction. The single-path form above is the one-rung case, unchanged.

`functions.<f>` names a guard group; `modes.<m>` carries `requires` (the
functions it needs), `fallback` (the ordered ladder), `reaction` and
`overrides`. Three rules:

- **`ladder-rung-budget`** (error) — each rung is judged against the FTTI
  **in its own right**: $T_{\text{FDTI}} + \text{route} + \text{settle}$ for
  that rung's own reaction path. A graded reaction is a promise, not merely
  a step on the way to the floor. The **last** rung is what
  `fault-reaction-budget` measures, because it is the floor the system is
  guaranteed to reach.
- **`ladder-unterminated`** (error) — a mode with no `fallback:` reaches no
  safe state, and **a last rung that requires anything this hazard's own
  guards remove is not a floor**: losing the guard takes the whole ladder
  with it.
- **`mode-requires-unguarded`** (warning) — a mode requires a function no
  subscriber watches, so its loss would never be observed.

`modes.<m>.overrides` pins a requirement value for one mode by naming its
contract path, so every requirement keeps one value where it is declared
and no scalar becomes a map. An override naming nothing is
`override-target-missing`; targets are read section-from-front and
field-from-back, because scope-path names contain dots. The checker then
**re-runs** the requirement checks once per mode whose overrides differ and
diffs against the default run, reporting only what that mode introduces,
tagged `mode:<rule>`.

## Burstiness

The drop composition rules assume each drop is independent (Bernoulli
model). In practice, DDS transport drops are often **bursty** — network
congestion, scheduling jitter, or queue overflow cause drops to cluster.
When drops are bursty, the declared `max_count` and `max_consecutive`
thresholds may be violated more often than the Bernoulli model predicts.

*Implementation status:* the runtime rule engine (play_launch
`--enforce-rules`, fed by the Phase 29 interception layer) checks
`drop.max_count` as a delivery-rate ratio (`drop-rate-runtime`), plus
`rate-hierarchy-runtime`, `max-age-runtime`, `max-latency-runtime`,
`qos-match-runtime`, `consistency-runtime`, `graph-deviation-runtime` and
the DDS `deadline-runtime` / `liveliness-runtime` events. Since phase 73
it also carries the hazard vocabulary — `hazard-detected`,
`hazard-reaction`, `hazard-recovered` and `mode-availability` — so the
FTTI arithmetic above is checked live as well as statically.
`max_consecutive` and the burstiness *detection* metrics below are the
designed extension — not yet implemented — for diagnosing why drop
thresholds trip:

- **Lag-1 autocorrelation** ($\rho_1$) — measures whether a drop
  predicts the next drop. $\rho_1 \approx 0$: independent. $\rho_1 > 0.05$
  with 1000+ samples: significant burstiness.

- **Dispersion index** ($DI$) — variance-to-mean ratio of drops per
  window. $DI \approx 1$: Poisson-like. $DI > 1.5$: overdispersed (bursty).

When burstiness is detected, the monitor recommends increasing
`max_consecutive` or investigating the burst cause. See Appendix B
for the detailed metric formulas.

## Appendix A: Drop Composition Theory

The formulas below describe the theoretical relationships between
per-topic drop rates and chain-level behavior. They are the design
basis for **runtime** drop analysis and alerting (not yet implemented —
the current runtime engine checks the observed delivery ratio only) and
are not used by the static checker, which only validates local
consistency.

### A.1 Delivery Rate Composition

Each topic on the critical path declares a drop budget whose rate is
$d_i = n_i/w_i$ (`drop: { max_count: n / w }`). The delivery rate:

$$\mathcal{R}_i = 1 - d_i$$

For a series chain of independent transports, a message must survive
every hop:

$$\mathcal{R}_{\text{chain}} = \prod_i \mathcal{R}_i = \prod_i (1 - d_i)$$

In log form (convenient for implementation):

$$\ln \mathcal{R}_{\text{chain}} = \sum_i \ln(1 - d_i)$$

The scope's own `drop.max_count` gives $d_s$. The check:

$$d_s \geq 1 - \mathcal{R}_{\text{chain}}$$

### A.2 Consecutive Drop: Poisson Derivation

Model each message as an independent Bernoulli trial with drop
probability $d$ (the composed drop rate for the route, or a single
topic's own $n/w$). We want the probability of $K$ or more
consecutive drops in $W$ messages, where $W$ is a runtime monitoring
window.

**Run starts.** A run of $K$ consecutive drops can start at positions
$1, 2, \ldots, W - K + 1$. For position $i > 1$: messages $i$ through
$i+K-1$ must drop ($d^K$) and message $i-1$ must not drop ($1-d$).
Per-position probability: $\approx d^K \cdot (1-d)$.

**Poisson approximation.** When runs are rare ($d^K$ small), the
count of runs $\geq K$ is approximately Poisson with mean:

$$\lambda = (W - K + 1) \cdot d^K \cdot (1-d)$$

Probability of at least one such run:

$$P(\text{max run} \geq K) \approx 1 - e^{-\lambda}$$

### A.3 Scope Consecutive Check

The scope declares `drop.max_consecutive: K_s`. Using a monitoring window
of $W$ messages (runtime parameter), we require the probability of
violation to be below confidence threshold $\epsilon$ (default 0.01):

$$(W - K_s + 1) \cdot d^{K_s} \cdot (1-d) \leq -\ln(1 - \epsilon) \approx 0.01$$

### A.4 Example: Three-Topic Pipeline

Three topics in series, each `drop: 2 / 100` ($d = 0.02$).

Chain: $\mathcal{R} = (1 - 0.02)^3 = 0.98^3 = 0.941$, $d = 0.059$.

Scope declares `drop: { max_count: 6 / 100, max_consecutive: 3 }`.

**Rate check:** $0.06 \geq d = 0.059$. Passes.

**Consecutive check** (monitoring window $W = 200$, $K = 3$):

$$198 \times 0.059^3 \times 0.941 = 0.038$$

$0.038 > 0.01$: **Fails.** Three consecutive drops are too likely (3.8%).

**Try** `max_consecutive: 4`:

$$198 \times 0.059^4 \times 0.941 = 0.002$$

$0.002 \leq 0.01$: **Passes.**

### A.5 Example: With Periodic Reset

Topics: T1 (`drop: 1 / 100`, cropbox→centerpoint) and
T2 (`drop: 2 / 100`, centerpoint→tracker). Tracker is periodic
(`drop: 1 / 100` on the output topic T3).

**Pre-tracker segment:** $\mathcal{R} = 0.99 \times 0.98 = 0.970$, $d = 0.030$.
**Post-tracker segment:** periodic resets the chain. $d = 0.01$ (T3 only).

Scope declares `drop.max_consecutive: 3`, monitoring window $W = 200$.

Pre-tracker: $198 \times 0.030^3 \times 0.970 = 0.005 \leq 0.01$. Passes.
Post-tracker: $198 \times 0.010^3 \times 0.990 = 0.0002 \leq 0.01$. Passes.

Both segments pass independently. The periodic node prevents upstream
bursts from propagating.

## Appendix B: Burstiness Metric Formulas

**Lag-1 autocorrelation.** Given binary trace $x[t] \in \{0, 1\}$
(1=delivered, 0=dropped):

$$\rho_1 = \frac{\sum_{t=1}^{N-1} (x[t] - \bar{x})(x[t+1] - \bar{x})}{\sum_{t=1}^{N} (x[t] - \bar{x})^2}$$

**Dispersion index.** Divide trace into windows of size $W$. Count
drops per window $d_i$:

$$DI = \frac{\mathrm{Var}(d_i)}{E(d_i)}$$

**Observed max run** $\ell_{\max}$ (longest consecutive drop sequence).
Compare to Bernoulli prediction:

$$E[\ell_{\max}^{\text{Bernoulli}}] \approx \frac{\ln(N \cdot (1-d))}{\ln(1/d)}$$

If $\ell_{\max} \gg E[\ell_{\max}^{\text{Bernoulli}}]$, drops are burstier
than the model predicts.

**Estimated mean burst length** (reported when $DI > 1.5$):

$$\hat{r} = 1 - P(\text{drop} \mid \text{previous drop})$$
$$\text{mean burst} = 1 / \hat{r}$$

## Appendix C: Empirical Contract Derivation

When writing a manifest for an existing system without documented timing
requirements, you need initial values for `max_latency`, `min_rate_hz`,
and `drop.max_count`. Capture mode bootstraps these from runtime
measurements.

*Implementation status:* capture mode as described below — deriving
`max_latency`, `min_rate_hz` and `drop.max_count` from observed traces —
is designed but not implemented; there is no CLI flag for it. What exists
is `play_launch measure <run-dir> --model <m.yaml>`, which turns a
recorded run into a pasteable fragment on stdout (never written back): a
platform-file `budget_us` per node, taken as the observed **maximum**
thread-CPU cost rather than a percentile (under CBS an overrun is
throttled to the next replenishment, so a p99 budget converts the slowest
1% of invocations into a full-period stall), and, as comments under a
header saying they belong in the *contract*, the measured
`nodes.<n>.paths.<p>.min_latency` floors that make `max_jitter`
falsifiable. Paths it cannot measure are printed with the reason —
timer-triggered, unstamped, not exercised — because omitting them would
read as "costs nothing". The interception layer records the per-topic
traces the rest of this appendix would need (`frontier_summary.json`,
`stats_summary.json`, `events.jsonl`).

Capture mode derives contracts from observed traces:

$$\hat{G}_L = \max(\text{observed latencies}) \times \alpha$$
$$\hat{A}_R = \min(\text{observed inter-arrivals}) / \alpha$$

where $\alpha > 1$ is the safety margin (default 1.2).

After $N$ observations without violation, at confidence level $c$:

$$P(\text{violation per trial}) \leq 1 - (1 - c)^{1/N}$$

For $N = 1000$ at $c = 0.99$: $P \leq 0.0046$.

Capture provides a starting point. Users refine manually or tighten
margins as more data is collected.

## References

- Benveniste et al., ["Contracts for System Design"](https://doi.org/10.1561/2500000017) (Foundations and Trends in EDA, 2018)
- de Alfaro & Henzinger, ["Interface Automata"](https://doi.org/10.1145/366927.366984) (POPL 2001)
- Casini et al., ["Response-Time Analysis of ROS 2 Processing Chains"](https://doi.org/10.4230/LIPIcs.ECRTS.2019.6) (ECRTS 2019)
- Becker et al., ["End-to-End Timing Analysis of Cause-Effect Chains"](https://doi.org/10.4230/LIPIcs.ECRTS.2017.9) (ECRTS 2017)
- [SAE AS5506C](https://www.sae.org/standards/content/as5506c/) (AADL) — flow latency analysis
- [AUTOSAR TIMEX R22-11](https://www.autosar.org/standards/r22-11) — event chain timing constraints
- [CARET](https://github.com/tier4/caret) — Chain-Aware ROS 2 Evaluation Tool
- Erdos & Renyi, "On a new law of large numbers" (*J. Analyse Math.* 1970) — longest runs in Bernoulli sequences
- Gilbert, "Capacity of a Burst-Noise Channel" (*Bell System Technical J.* 1960) — burst error model
