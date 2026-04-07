# Contract Theory for Launch Manifests

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
| $L_{\text{node}}(X)$ | Worst-case processing time of node $X$ (from `max_latency_ms`) |
| $L_{\text{transport}}(X \to Y)$ | Worst-case transport time between nodes $X$ and $Y$ (typically ~0 for same-machine) |
| $L_{\max}$, $L_{\min}$ | Worst / best case end-to-end latency of a path or scope |
| $A_{\max}$ | Maximum data age from original source (ms) |
| $f$ | Frequency (Hz) |
| $P$ | Timer period (ms) |
| $J$ | Jitter — max deviation from ideal period (ms) |
| $d$ | Drop rate: fraction of messages lost ($d = N/W$) |
| $\mathcal{R}$ | Delivery rate: $\mathcal{R} = 1 - d$ (fraction that survives) |
| $N$ | Max drops in a window |
| $W$ | Window size (messages) |
| $K$ | Max consecutive drops |
| $\ell_{\max}$ | Observed longest consecutive drop run (runtime) |
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
- *Guarantee:* the channel delivers at 10 Hz with drops ≤ 1/100

**Node contract** — for ground_filter:
- *Assumption:* receives filtered points at ≥ 10 Hz
- *Guarantee:* output within 15ms, drops ≤ 2/100

**Scope contract** — for the whole perception pipeline:
- *Assumption:* parent scope wires the pointcloud input
- *Guarantee:* end-to-end latency ≤ 50ms, data age ≤ 150ms

Summary:

| Level | What it describes | Assumption | Guarantee |
|-------|-------------------|------------|-----------|
| **Topic** | A communication channel | Publisher produces at `rate_hz` | Channel delivers at `rate_hz` with drops ≤ `drop` |
| **Node** | A single computation | Inputs arrive per `min_rate_hz`, `state`, `required` | Output within `max_latency_ms`, drops ≤ `drop` per path |
| **Scope** | An entire launch file | Parent wires scope interface endpoints | E2E `max_latency_ms` and `max_age_ms` |

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
      sensor_points:
        min_rate_hz: 10
      map:
        state: true
        required: true
    pub:
      ndt_pose:
        min_rate_hz: 10
    paths:
      localization:
        input: sensor_points
        output: [ndt_pose]
        max_latency_ms: 50
        drop:
          max_count: 10 / 100
          max_consecutive: 5
```

**Assumption** ($A$):
- `sensor_points` arrives at $f \geq 10$ Hz (causal trigger)
- `map` has been received at least once (`required`) and is polled (`state`)

**Guarantee** ($G$):
- `ndt_pose` published at $f \geq 10$ Hz
- $L_{\max} \leq 50$ ms (trigger to output)
- $d \leq 10/100$, $K \leq 5$ consecutive

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
rules below show how to compute the pipeline's end-to-end latency, drop
rate, and age from the individual node contracts.

### Understanding Worst-Case Latency

A message traverses a pipeline by passing through nodes and the
transport channels (topics) between them. At each stage, time is spent:

- **Node processing** — the time a node takes to receive an input,
  compute, and publish the output. This is the node's `max_latency_ms`.
- **Transport** — the time for a published message to reach the next
  subscriber via DDS. On the same machine this is typically < 1ms;
  across machines it depends on the network.

The **worst-case latency** of a path is the total time assuming every
stage takes its maximum. This is a pessimistic bound — the real latency
is usually lower, but the bound is what the contract guarantees.

We write:
- $L_{\text{node}}(X)$ — worst-case processing time of node $X$
  (from the node's `max_latency_ms`)
- $L_{\text{transport}}(X \to Y)$ — worst-case transport time between
  node $X$ and node $Y$ (typically ~0 for same-machine)

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

**Transport is implicit in the scope budget.** Transport latency between
nodes is not declared separately in the manifest — it is absorbed into
the scope's `max_latency_ms` as headroom. The gap between the sum of
node budgets and the scope budget accounts for transport and scheduling
variance. On the same machine, transport is typically < 1ms.

**Delivery rates also compose in series** — each stage independently
drops messages, so a message must survive every stage. See
[Drop Composition](#drop-composition) below for the formulas and examples.

**Age accumulates** — the age at the output is the age at the source
plus all processing and transport time along the chain:

$$A_{\max} = A_{\max}(\text{source}) + \sum_i L_{\text{node}}(p_i) + \sum_j L_{\text{transport}}(t_j)$$

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

Example: $\max(50, 30) + 20 = 70$ ms.

Note: the best-case latency $L_{\min}$ also uses $\max$ — the fusion
barrier waits for the slowest branch even when both are fast.

**Rate = slowest branch:** $f = \min(f_A, f_B)$

**Age = oldest branch + fusion** — the output is as stale as its
oldest input:

$$A_{\max} = \max(A_{\max}(\text{branch } A),\; A_{\max}(\text{branch } B)) + L_{\text{node}}(C)$$

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

$$L_{\max} = L_{\text{node}}(\text{upstream}) + P + J + L_{\text{node}}(\text{periodic})$$

Where:
- $L_{\text{node}}(\text{upstream})$ — time for data to reach the buffer
- $P$ — one full timer period of waiting (worst case)
- $J$ — timer jitter (the timer itself may be late)
- $L_{\text{node}}(\text{periodic})$ — the periodic node's processing time

**Best case:** the timer fires right as data arrives (zero wait):

$$L_{\min} = L_{\text{node}}(\text{upstream}) + L_{\text{node}}(\text{periodic})$$

**Rate is independent of upstream:** $f = 1000 / P$. The periodic node
produces output at its own timer rate regardless of how fast or slow
the upstream is.

**Periodic nodes reset the consecutive drop chain.** Upstream
consecutive drops don't propagate because the timer fires regardless
of whether new data arrived. Each segment (before and after the
periodic node) is checked independently.

### Drop Composition

Define **delivery rate** $\mathcal{R} = 1 - d$ where $d = N/W$ from the
manifest's `drop: N / W` declaration.

**Series:** delivery rates multiply.

$$\mathcal{R}_{\text{chain}} = \prod_i \mathcal{R}_i$$

**Scope check:** the scope declares `drop: N_s / W_s`. Valid when:

$$\frac{N_s}{W_s} \geq 1 - \mathcal{R}_{\text{chain}}$$

**Consecutive drops:** given chain delivery rate $\mathcal{R}$ and
drop probability $d = 1 - \mathcal{R}$, the probability of $K$ or more
consecutive drops in $W$ messages (Poisson approximation):

$$P(\text{max run} \geq K \mid W) \approx 1 - \exp\!\left(-(W - K + 1) \cdot d^K \cdot (1-d)\right)$$

The scope declares `max_consecutive: K_s`. The checker verifies this
probability is below 1% (see Appendix A for the full derivation).

**Necessary condition:** $K_s \geq \max_i K_i$ — the scope can't be
stricter than any individual node.

## Verification Rules

The checker verifies that declared budgets are consistent across the
scope tree. Two separate checks apply to latency, drop, and age — each
with property-specific composition math but the same structural rules.

For precise measurement point definitions, see
[Latency vs Age](launch-manifest.md#latency-vs-age) in the manifest spec.

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
  declared = collect children with budgets (look through transparent scopes)
  undeclared = children without budgets
  if sum(declared) > scope.budget → WARNING
  residual = scope.budget - sum(declared)
  if undeclared is not empty and scope has paths:
    → INFO: "Xms residual across N undeclared nodes"
```

The sum check is a **warning** (not an error) because the sum is a lower
bound — it doesn't include transport between nodes. The gap between the
sum and the scope budget is the allowance for transport, scheduling
variance, and undeclared nodes.

Like the overflow check, the sum check is **topology-unaware**. For
parallel branches, the sum is conservative (sum > max), so the check
may warn even when the actual composition fits. This is the right
direction — a false warning is better than a missed violation.

### Latency Example

```
scope S: max_latency_ms: 100
  ├── sub-scope P: max_latency_ms: 50    (opaque)
  │     ├── node A: max_latency_ms: 20
  │     └── node B: max_latency_ms: 25
  ├── sub-scope Q: (no budget)            (transparent)
  │     └── node C: max_latency_ms: 30
  └── node E: (no budget)
```

**Overflow check** — walk the tree with tightest ancestor:
- A(20) ≤ min(S:100, P:50) = 50. OK.
- B(25) ≤ 50. OK.
- C(30) ≤ min(S:100) = 100. OK. (Q has no budget, doesn't tighten.)

**Sum check on P** (opaque, checks its own children):
- A(20) + B(25) = 45 ≤ 50. Passes. 5ms residual (transport/headroom).

**Sum check on S** (looks through transparent Q):
- P(50, opaque) + C(30, from Q look-through) = 80. E is undeclared.
- 80 ≤ 100. Passes. Residual: 20ms across 1 undeclared node (E).

If E later gets a budget of 25ms: 50 + 30 + 25 = 105 > 100. Warning.

### Drop Example

Drop budgets compose multiplicatively. The structural rules (opaque,
transparent, overflow) still apply, but the "sum" check uses delivery
rate multiplication instead of addition.

```
scope S: drop: 10/100
  ├── node A: drop: 3/100
  ├── node B: drop: 3/100
  └── node C: (no drop budget)
```

**Overflow check**: A's drop rate (3%) < S's (10%). B's (3%) < 10%. OK.

**Composition check**: chain delivery rate $\mathcal{R} = 0.97 \times 0.97 = 0.9409$.
Chain drop rate: 5.91%. Scope allows 10%.
5.91% ≤ 10%. Passes.

Residual drop budget: the chain uses 5.91% of the 10% budget. The
remaining ~4.09% covers C's drops and transport losses. As C gets a
drop budget, the chain rate increases and the residual shrinks.

### Age: Chain-Based Verification

Age behaves differently from latency and drop. Age is a **chain
property** — it's the sum of all latencies from the original source
to the output, computed along the causal path. It does not decompose
hierarchically like a latency budget.

When the checker verifies `max_age_ms`, it traces the causal chain
from the scope's output back to the source, summing each node's
`max_latency_ms` and transport along the way. If any node on the
chain has no `max_latency_ms`, the chain has a gap — the static
age check cannot be completed.

```
scope S: max_age_ms: 200
  paths: { input: sensor, output: [plan] }
  ├── node A: max_latency_ms: 30
  ├── node B: (no latency budget)       ← gap in the chain
  └── node C: max_latency_ms: 50
```

Known chain contribution: A(30) + C(50) = 80ms. B's processing time
is unknown, so the checker cannot prove age ≤ 200ms.

Result: INFO — "age check incomplete: node B on the critical path has
no latency budget." The scope's `max_age_ms` contract is accepted but
unverified statically. Runtime monitoring checks the actual age.

The **overflow check** still applies to age: if a scope declares
`max_age_ms: 100` and a child scope declares `max_age_ms: 150`,
that's an error (child age cannot exceed parent age).

### Partial Decomposition

A scope contract is valid without full node-level decomposition. Three
scenarios:

| Scope budget | Node budgets | Checker behavior |
|-------------|-------------|-----------------|
| Declared | All declared | Verify composition ≤ scope, check overflow |
| Declared | Some missing | Verify declared fit, report residual (latency/drop) or gap (age) |
| Declared | None declared | Accept — runtime monitoring checks E2E |

This supports a **top-down workflow**: start with the E2E requirement,
fill in node budgets as you measure them. The residual (or gap report)
tells you what's unaccounted for.

## Data Age

The **age** of an output message is the time since the original sensor
data was created:

$$\text{age}(o) = t_{\text{pub}}(o) - \text{header.stamp}(\text{source})$$

This works because causal paths preserve `header.stamp` through the
chain — each node copies the input stamp to the output (see
[Timestamps and Data Flow](launch-manifest.md#timestamps-and-data-flow)).

**Static computation:** sum node processing times and transport times
along the causal chain:

$$A_{\max}(o) = \sum_{\text{chain}} L_{\text{node}}(p_i) + \sum_{\text{chain}} L_{\text{transport}}(t_j)$$

For multi-input paths (barrier), the age is the **maximum** of all input
ages — the output is as stale as its oldest input:

$$A_{\max}(o) = \max_{i \in \text{inputs}} A_{\max}(\text{input}_i) + L_{\text{node}}(p)$$

**`max_age_ms`** on a scope path constrains freshness. The checker
verifies the declared age budget is >= the statically computed age.

## Burstiness

The drop composition rules in the previous section assume each drop is
independent (Bernoulli model). If drops are actually bursty — clustered
together due to network congestion or OS scheduling — the static
guarantees are weaker than they appear. The runtime monitor detects
this gap.

In practice, DDS transport drops are often **bursty** — network
congestion, scheduling jitter, or queue overflow cause drops to cluster.

The runtime monitor detects burstiness via two lightweight metrics
(~5 ns/message overhead):

- **Lag-1 autocorrelation** ($\rho_1$) — measures whether a drop
  predicts the next drop. $\rho_1 \approx 0$: independent. $\rho_1 > 0.05$
  with 1000+ samples: significant burstiness.

- **Dispersion index** ($DI$) — variance-to-mean ratio of drops per
  window. $DI \approx 1$: Poisson-like. $DI > 1.5$: overdispersed (bursty).

When burstiness is detected, the monitor recommends increasing
`max_consecutive` or investigating the burst cause. See Appendix B
for the detailed metric formulas.

## Appendix A: Drop Composition Derivation

### A.1 Delivery Rate Composition

Each node declares `drop: N_i / W_i`. The delivery rate:

$$\mathcal{R}_i = 1 - \frac{N_i}{W_i}$$

For a series chain of independent components, a message must survive
every stage:

$$\mathcal{R}_{\text{chain}} = \prod_i \mathcal{R}_i$$

In log form (convenient for implementation):

$$\ln \mathcal{R}_{\text{chain}} = \sum_i \ln \mathcal{R}_i$$

### A.2 Consecutive Drop: Poisson Derivation

Model each message as an independent Bernoulli trial with drop
probability $d = 1 - \mathcal{R}_{\text{chain}}$. We want the
probability of $K$ or more consecutive drops in $W$ messages.

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

The scope declares `max_consecutive: K_s` over window $W_s$. We
require $P(\text{max run} \geq K_s) \leq \epsilon$ (default $\epsilon = 0.01$).

Rearranging:

$$(W_s - K_s + 1) \cdot d^{K_s} \cdot (1-d) \leq -\ln(1 - \epsilon) \approx 0.01$$

### A.4 Example: Three-Node Pipeline

Three nodes, each `drop: 2/100`. Chain: $\mathcal{R} = 0.98^3 = 0.941$,
$d = 0.059$.

Scope declares `drop: 12/200, max_consecutive: 3`.

**Rate check:** $12/200 = 0.06 \geq d = 0.059$. Passes.

**Consecutive check** ($W=200$, $K=3$):

$$198 \times 0.059^3 \times 0.941 = 0.038$$

$0.038 > 0.01$: **Fails.** Three consecutive drops are too likely (3.8%).

**Try** `max_consecutive: 4`:

$$198 \times 0.059^4 \times 0.941 = 0.002$$

$0.002 \leq 0.01$: **Passes.**

### A.5 Example: With Periodic Reset

cropbox (`drop: 1/100`) → centerpoint (`drop: 2/100`) → tracker
(periodic, `drop: 1/100`).

**Pre-tracker:** $\mathcal{R} = 0.99 \times 0.98 = 0.970$, $d = 0.030$.
**Post-tracker:** periodic resets the chain. $d = 0.01$.

Scope declares `max_consecutive: 3` over $W=200$.

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
requirements, you need initial values for `max_latency_ms`, `min_rate_hz`,
and `drop`. Capture mode bootstraps these from runtime measurements.

Capture mode (`--save-manifest-dir`) derives contracts from observed
traces:

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
