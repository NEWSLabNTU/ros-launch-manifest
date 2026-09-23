---
marp: true
theme: default
paginate: true
---

# Launch Manifest
## Communication Contracts for ROS 2 Launch Files

---

# The Problem

ROS 2 launch files declare **what to run** — but not what communicates.

- Topics are created in source code — invisible until runtime
- No reference specification for the expected communication graph
- A code change can silently break timing or remove a topic

**Example:** Autoware planning_simulator — 110 nodes, 500+ topics.
Who checks that the perception pipeline meets its 100ms latency budget?

---

# What is a Launch Manifest?

A sidecar YAML file that describes what one launch file **contributes
to the communication graph**.

| Launch file says | Manifest adds |
|-----------------|--------------|
| Which nodes to run | Named endpoints (pub/sub/srv/cli) |
| How to remap names | First-class topic wiring with type + QoS |
| Which files to include | Child scopes with their own manifests |
| *(nothing)* | Timing contracts: latency, age, drops |
| *(nothing)* | Hazards, reactions and operational modes |

One manifest per launch file. The tree of manifests mirrors the tree
of launch file includes.

---

# From Launch Files to Manifests

![w:900](img/manifest-mapping.png)

---

# Nodes and Endpoints

Nodes declare named **endpoints** — logical ports for communication.

```yaml
nodes:
  controller:
    pub:
      cmd:
        min_rate_hz: 30           # I produce at least 30 Hz
    sub:
      trajectory:
        min_rate_hz: 10           # I need at least 10 Hz
      map:
        state: true               # polled, not causal
        required: true            # must receive at least once
```

- `state: true` — read-latest (breaks feedback cycles in the graph)
- `required: true` — node can't operate without initial data

---

# Topics: First-Class Wiring

Topics wire endpoints together with type, QoS, rate, and drop tolerance.

```yaml
topics:
  control_cmd:
    type: autoware_control_msgs/msg/Control
    pub: [controller/cmd]
    sub: [validator/input]
    rate_hz: 30
    drop:
      max_count: 1 / 100          # 1% transport loss allowed
      max_consecutive: 3          # never 3+ drops in a row
    qos:
      reliability: reliable
      durability: transient_local
```

In launch files, this information is split across `<remap>`, source
code, and convention. The manifest makes it **explicit and checkable**.

---

# Rate Hierarchy with Drops

The publisher produces, transport may drop, subscriber demands:

```
pub.min_rate_hz  >=  rate_hz  >=  rate_hz × (1 - n/w)  >=  sub.min_rate_hz
     30                30          30 × 0.99 = 29.7           29
                                   drop: { max_count: 1 / 100 }
```

**supply ≥ channel ≥ effective delivery ≥ demand**

Drop rates compose multiplicatively through a pipeline:

$$\mathcal{R}_{\text{chain}} = \prod_i (1 - d_i)$$

Three topics at 2% each: $0.98^3 = 0.941$ → 5.9% E2E drop rate.

---

# Timing Contracts: Latency and Age

**`max_latency: 30ms`** — processing time (trigger input → output publish)

```
sensor → [cropbox: 5ms] → [ground_filter: 15ms] → [detector: 30ms]
         ├── 5ms ──┤      ├──── 15ms ────┤         ├── 30ms ──┤
         └──────────── scope: 50ms ─────────────────────────────┘
```

- **Node**: `rcl_take` → `rcl_publish` (processing only)
- **Scope**: first take → last publish (includes internal transport)

**`max_age: 200ms`** — data freshness from original source

$$\text{age} = \text{now} - \text{header.stamp}$$

Causal paths preserve `header.stamp`. Periodic nodes reset the chain.

---

# Composition: No Scope Interface

Scopes compose through **ROS topic names**, not through export/import
blocks. Each manifest declares the topics it touches; the checker merges
declarations of the same resolved name across the tree.

```yaml
# tracking.contract.yaml — ns /perception/.../tracking
topics:
  objects:                              # relative → .../tracking/objects
    type: autoware_perception_msgs/msg/TrackedObjects
    pub: [multi_object_tracker/tracked]
```

```yaml
# prediction.contract.yaml — a different scope, the same topic
topics:
  /perception/object_recognition/tracking/objects:
    type: autoware_perception_msgs/msg/TrackedObjects
    sub: [map_based_prediction/tracked]
```

Each manifest stays checkable **standalone** — that is why both declare
the `type:`, and why `consistency` (a *cross-scope* rule, emitted by the
consumer's merge layer) requires them to agree.

**Opaque scope** (has budget) → parent trusts the declared value
**Transparent scope** (no budget) → parent looks through to children

---

# Facts, Requirements, Consequences

A contract states **what the code does** (facts) and **what it must
achieve** (requirements). Anything computable from those is a
**consequence** — derived, never written.

```yaml
nodes:
  detector:
    sub:
      scan:
        max_age: 120ms          # REQUIREMENT: freshness at receive
    pub:
      boxes: {}
    paths:
      detect:
        trigger: { input: [scan] }   # FACT: what causes the output
        output: [boxes]
        max_latency: 30ms       # REQUIREMENT: this path's budget
```

Derived from those: the **route** between two topics (a scope path
names two ends and a budget — the route is the graph's answer), the
**total** along it, a topic's **rate** (propagated from the timers that
drive it), and a node's **criticality**.

A second copy of a consequence can disagree with the graph. Where one is
written anyway, `derivable-*` says they agree and `*-mismatch` says they
do not.

---

# Criticality Is a Consequence

`criticality: high | medium | low` is not a free label. It is allocated
**inward from the hazards**, the way every safety standard does it:

- a node that **feeds** a guard (its publishers, and their upstream
  causal closure, state edges included),
- one that **detects** one (a subscriber carrying `on_violation:`),
- or one that **reacts** (on the walk to the safe state)

takes that hazard's severity — **max** over the hazards that reach it.
`severity_levels:` declares the scale (default: ISO 26262's
`QM, ASIL_A..ASIL_D`).

The mapper reads the derivation **before** any written label.
`derivable-criticality` (info) and `criticality-mismatch` (warning)
compare the two; a node no hazard reaches keeps its label — that is the
underivable case, and the reason the key still exists.

---

# Faults, Reactions, Modes

A rate that *must* hold and no statement of what happens when it does
not is a performance wish. ISO 26262's number is the **fault-tolerant
time interval**.

```yaml
hazards:
  drive_blind:
    severity: ASIL_D
    guards: [/scan]              # what is watched
    on: omission                 # omission | late | loss | reported
    ftti: 300ms                  # REQUIREMENT: fault -> hazardous event
    reaction: stop_now           # the scope path reaching the safe state
paths:                           # a SCOPE path: two ends and a budget
  stop_now:
    trigger: { input: [/scan] }  # `input:` alone is the v1 spelling
    output: [/brake]
    max_latency: 80ms
    safe_state: { emits: brake/cmd, settle: 400ms }
```

**FDTI** (detect) and **FRTI** (react) are *derived*: the fastest
detector among the guard's subscribers that react, plus the walk over
reaction edges, plus the plant's settle time. `fault-reaction-budget`
names every term of the sum.

`modes:` make the reaction a **ladder** rather than one step — each rung
is checked against the ftti in its own right, and the last rung must
require nothing losable.

---

# Verification Rules

**19 single-manifest checks** + cross-scope checks in the consumer,
all at authoring time, before any code runs:

| Rule | What it catches | Severity |
|------|----------------|----------|
| `rate-hierarchy` | pub rate < topic rate < sub rate, and the upper bounds too | Error |
| `budget-overflow`* | Child path budget > ancestor path budget (part > whole) | Error |
| `scope-budget` | Flat sum > scope budget (cross-scope: derived critical path) | Warning |
| `drop-sanity` | `drop:` values out of range; effective rate < sub demand | Error |
| `causal-dag` | Feedback cycle (state: true breaks it) | Error |
| `satisfiability` | Arg combo produces dangling entities (Z3) | Error/Warning |
| `dangling-entity` | Topic with 0 publishers after filtering | Warning/Error |

Plus: `endpoint-unique`, `wiring`, `qos-compat`, `qos-match`,
`service-wiring`, `service-type`, `state-consistency`,
`explicit-trigger`, `inherited-rate`, `once-durability`,
`sync-feasibility`, `queue-drain-rate`, `jitter-range`
(\* = cross-scope, in the consumer's merge layer — with `consistency`,
`scope-sampling-feasibility`, `jitter-feasibility`, `lifespan-age`,
`fault-reaction-budget`, `reaction-*`, the mode and `ladder-*` rules,
the `derivable-*` / `*-mismatch` comparisons, …)

---

# Partial Decomposition

You don't need contracts on every node. Start top-down:

```
scope S: max_latency: 100ms
  ├── sub-scope P: max_latency: 50ms     (opaque — black box)
  │     ├── node A: max_latency: 20ms
  │     └── node B: max_latency: 25ms
  ├── sub-scope Q: (no budget)            (transparent — look through)
  │     └── node C: max_latency: 30ms
  └── node E: (no budget)                 → 20ms residual
```

- P checks: 20 + 25 = 45 ≤ 50 ✓ (5ms transport headroom)
- S checks: P(50) + C(30) = 80 ≤ 100 ✓ (20ms residual for E)

Those flat sums are the **standalone** check — conservative on purpose,
since one manifest cannot see the topology. Across the merged tree the
consumer computes the **critical path** instead: `max` over parallel
branches, `sum` along one. A lidar branch of 50ms beside a camera branch
of 30ms feeding a 20ms fusion node costs `max(50, 30) + 20 = 70`, not
the sum 100 — so the precise check accepts trees the sum would reject.

Fill in per-node budgets as you measure them.

---

# Composition Summary

| Topology | Latency | Rate | Drop | Age |
|----------|---------|------|------|-----|
| **Series** | sum | preserved | multiply $\mathcal{R}$ | sum along chain |
| **Fan-in** | max over branches + fusion | **sum**, or **min** with `sync:` | user-declared | max + fusion |
| **Periodic** | +P+J (wait) | 1000/P | resets consecutive | resets stamp chain |

Fan-in rate is the one people get backwards: a callback registered on
two inputs fires once per message on *each*, so the rates **add**. Only
a synchronizer (`sync:`) emits one output per matched set, and that is
paced by its **slowest** input. Taking the min in both cases understates
a fan-in node's load by exactly the factor that decides whether it fits.

Drop composition — one `drop:` block, three places it may sit:
- **`max_count: N / W`** on a topic (transport), a scope path (E2E) or
  a node path (messages the node itself skips)
- **`max_consecutive: K`** beside it — never K+ in a row
- A bare `drop: 2 / 100` is shorthand for `max_count` alone

---

# Conditional Configurations (Z3)

Args with `type: bool` or `choices:` enable exhaustive checking:

```yaml
args:
  launch_validator:
    type: bool                    # 2 valid values
  pose_source:
    choices: [ndt, eagleye, gnss] # 3 valid values
```

The checker uses **Z3 SMT solver** to verify: no valid arg combination
produces a structurally broken manifest (0 publishers, 0 servers).

Example: `pose_source: gnss` but no `gnss_node` declared →
*"topic 'pose' has 0 publishers when pose_source=gnss"*

---

# Status

**Grammar**: enumerable — `types/src/field_table.rs` is the single
source, `docs/format-reference.md` is generated from it, and an unknown
key is a **parse error**
**Checker**: 19 single-manifest rules (incl. Z3 satisfiability) +
cross-scope rules in the consumer (`consistency`, `budget-overflow`,
critical-path `scope-budget`, `scope-sampling-feasibility`,
`fault-reaction-budget`, the mode and `ladder-*` rules, the
`derivable-*` comparisons, …)
**Runtime**: rate/age/latency/drop enforcement via RCL interception
(`--enforce-rules`, default `warn`), plus the live fault observer —
`hazard-detected`, `hazard-reaction`, `hazard-recovered`,
`mode-availability`
**Scheduling**: 4 mappers (`manual`, `rate_monotonic`,
`deadline_monotonic`, `chain_aware`) deriving per-node RT priorities
from these contracts
**Derivation**: one shared `derive/` crate — both consumers build the
mapper's input from the resolved model through the same function
**Autoware contracts**: 76 contract files covering the full
planning_simulator tree

Open items: burstiness detection metrics, capture mode
(contract bootstrapping from traces — not implemented)

---

# Links

(paths relative to the `ros-launch-manifest` repo root)

- **Docs index**: `docs/README.md`
- **Manifest spec**: `docs/launch-manifest.md`
- **Contract theory**: `docs/contract-theory.md`
- **Checker implementation**: `docs/contract-verification.md`
- **Scheduling**: `docs/scheduling.md`
- **Design issues**: `docs/design-issues.md`
- **Autoware contracts**: the `autoware-contract` repository
  (76 contract files for the planning_simulator tree)
