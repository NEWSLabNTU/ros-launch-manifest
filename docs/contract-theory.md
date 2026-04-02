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
| $L_{\max}$, $L_{\min}$ | Maximum / minimum latency (ms) |
| $A_{\max}$ | Maximum data age from original source (ms) |
| $f$ | Frequency (Hz) |
| $P$ | Timer period (ms) |
| $J$ | Jitter — max deviation from ideal period (ms) |
| $d$ | Drop rate: fraction of messages lost ($d = N/W$) |
| $\mathcal{R}$ | Delivery rate: $\mathcal{R} = 1 - d$ |
| $N$ | Max drops in a window |
| $W$ | Window size (messages) |
| $K$ | Max consecutive drops |
| $L(t)$ | Transport latency of topic $t$ (typically ~0 for same-machine) |

## What is a Contract?

A **contract** is a pair $C = (A, G)$:

- **Assumption** ($A$) — constraints on inputs that must hold for the
  guarantee to be valid. "I need sensor data at 10 Hz or faster."
- **Guarantee** ($G$) — constraints on outputs that the component
  promises, given the assumption holds. "I will produce a result within
  30ms."

A component $M$ **satisfies** contract $C$ when: if the assumption holds
on the inputs, the guarantee holds on the outputs.

$$M \cap A \subseteq G$$

In plain English: "if you give me what I asked for, I'll deliver what I
promised."

### Three Levels

The manifest defines contracts at three levels:

| Level | What it describes | Assumption | Guarantee |
|-------|-------------------|------------|-----------|
| **Topic** | A communication channel | Publisher produces at `rate_hz` | Channel delivers at `rate_hz` with drops ≤ `drop` |
| **Node** | A single computation | Inputs arrive per `min_rate_hz`, `state`, `required` | Output within `max_latency_ms`, drops ≤ `drop` per path |
| **Scope** | An entire launch file | Parent wires scope interface endpoints | E2E `max_latency_ms` and `max_age_ms` |

These compose hierarchically: topic contracts constrain the channels,
node contracts describe per-node timing, scope contracts abstract the
internal graph into an end-to-end budget.

### Example: NDT Scan Matcher

Here is one node's contract in both YAML and formal notation:

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

When nodes are connected into a pipeline, their contracts compose.
The rules depend on the connection topology.

### Series (Pipeline)

Nodes connected in sequence: $A \to B \to C$.

```
in → [A: 5ms] → [B: 15ms] → [C: 30ms] → out
```

**Latency adds:**

$$L_{\max}(A \to B \to C) = L_{\max}(A) + L(t_{AB}) + L_{\max}(B) + L(t_{BC}) + L_{\max}(C)$$

In this example: $5 + 0 + 15 + 0 + 30 = 50$ ms (transport $L(t) \approx 0$ for same-machine).

**Delivery rates multiply** (each stage independently drops):

$$\mathcal{R}_{\text{chain}} = \prod_i \mathcal{R}_i$$

Three nodes each dropping 2/100: $\mathcal{R} = 0.98^3 = 0.941$, so
the chain drops about 5.9%.

**Age accumulates:**

$$A_{\max}(C) = A_{\max}(\text{source}) + \sum L_{\max}(p_i) + \sum L(t_j)$$

### Parallel (Fork-Join)

Two branches merging at a fusion node:

```
      ┌→ A (50ms) →┐
in →  │             ├→ C (20ms) → out
      └→ B (30ms) →┘
```

**Latency = slowest branch + fusion:**

$$L_{\max} = \max(L_{\max}(A), L_{\max}(B)) + L_{\max}(C)$$

Example: $\max(50, 30) + 20 = 70$ ms. The barrier waits for the slowest
branch even in the best case, so $L_{\min}$ also uses $\max$.

**Rate = slowest branch:**

$$f = \min(f_A, f_B)$$

**Age = oldest branch + fusion:**

$$A_{\max} = \max(A_{\max}(A), A_{\max}(B)) + L_{\max}(C)$$

Drop at the barrier is **not composed statically** — the user declares
the fusion node's observed drops directly, combining all causes
(upstream propagation, correlation mismatch, computation) into one value.

### Periodic (Timer-Driven)

A node that runs on a timer with period $P$ and jitter $J$, reading
state inputs:

```
upstream → [state buffer] → [periodic node, P=100ms] → out
```

**Worst-case latency through a periodic node:**

$$L_{\max} = L_{\max}(\text{upstream}) + P + J + C$$

Worst case: state arrives just after the timer fires, waits a full period.

**Best case:**

$$L_{\min} = L_{\min}(\text{upstream}) + C$$

Best case: timer fires right as state updates (zero wait). $C$ is
computation time.

**Rate is independent of upstream:**

$$f = 1000 / P$$

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

## Data Age

The **age** of an output message is the time since the original sensor
data was created:

$$\text{age}(o) = t_{\text{pub}}(o) - \text{header.stamp}(\text{source})$$

This works because causal paths preserve `header.stamp` through the
chain — each node copies the input stamp to the output (see
[Timestamps and Data Flow](launch-manifest.md#timestamps-and-data-flow)).

**Static computation:** sum max latencies along the causal chain:

$$A_{\max}(o) = \sum_{\text{chain}} L_{\max}(p_i) + \sum_{\text{chain}} L(t_j)$$

For multi-input paths (barrier), the age is the **maximum** of all input
ages — the output is as stale as its oldest input:

$$A_{\max}(o) = \max_{i \in \text{inputs}} A_{\max}(\text{input}_i) + L_{\max}(p)$$

**`max_age_ms`** on a scope path constrains freshness. The checker
verifies the declared age budget is >= the statically computed age.

## Burstiness

The drop composition math assumes **independent drops** (Bernoulli model).
In practice, DDS transport drops are often **bursty** — network
congestion, OS scheduling, or queue overflow cause drops to cluster.

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

**Observed max run** $L_{\max}$. Compare to Bernoulli prediction:

$$E[L_{\max}^{\text{Bernoulli}}] \approx \frac{\ln(N \cdot (1-d))}{\ln(1/d)}$$

If $L_{\max} \gg E[L_{\max}^{\text{Bernoulli}}]$, drops are burstier
than the model predicts.

**Estimated mean burst length** (reported when $DI > 1.5$):

$$\hat{r} = 1 - P(\text{drop} \mid \text{previous drop})$$
$$\text{mean burst} = 1 / \hat{r}$$

## Appendix C: Empirical Contract Derivation

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
