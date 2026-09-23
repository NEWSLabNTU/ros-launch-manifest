# Launch Manifest

*This is the manifest format specification. For the formal theory
behind timing contracts and composition rules, see
[contract-theory.md](contract-theory.md).*

## Introduction

ROS 2 launch files declare which nodes to run but not which topics they
create. Topic creation happens in source code — publishers and subscribers
are invisible until runtime.

A **launch manifest** is a sidecar YAML file that describes what one launch
file contributes to the communication graph: its nodes, their endpoints,
the topics and services that wire them, and optional timing contracts.
Where a launch file says *what to run*, the manifest says *what communicates
and at what quality*.

**Terminology:** "manifest" is the document; "contract file" is the file
that carries it on disk (`<stem>.contract.yaml`, matching the launch
file's stem). The two are used interchangeably — CLI surfaces
(`--contracts`, `--no-provider-contracts`) say "contract", the format
model says "manifest". They are one artifact, not two.

**How to read this document:**

- **[Manifest Elements](#manifest-elements)** — the building blocks: scope, node, topic, path, etc.
- **[Background](#background)** — design principles, dataflow patterns, contracts, timing, and timestamps.
- **[Worked Example](#worked-example)** — a complete multi-scope perception pipeline.
- **[Format Reference](#format-reference)** — field-level syntax lookup for writing manifests.
- **[Vocabulary v2](#vocabulary-v2)** — `trigger:`, `sync:`, `buffer:`, scope `paths:` and the derived route.
- **[Fault detection and reaction](#fault-detection-and-reaction)** — `hazards:`, `on_violation:`, `safe_state:`, and the FTTI arithmetic.
- **[Operational modes](#operational-modes)** — `functions:`, `modes:`, the fallback ladder and per-mode `overrides:`.
- **[Static Validation](#static-validation)** — checker rules and example diagnostics.

## From Launch Files to Manifests

A manifest mirrors the launch file it describes. Each launch file concept
maps to a manifest element. Matching colors show the correspondence:

![Launch file to manifest mapping](img/manifest-mapping.png)

| Launch XML | Manifest YAML | What the manifest adds |
|------------|---------------|------------------------|
| `<arg>` | `args:` | Typed parameters: bool, choices, string |
| `<node>` / `<group>` | `nodes:` | Named endpoints (pub/sub/srv/cli) |
| `<remap>` | `topics:` | First-class wiring: type + QoS + rate |
| `if="$(var ...)"` | `if:` / `unless:` | Conditions on any entity |
| `<include>` | `includes:` | Child scope with its own manifest |
| *(in source code)* | `paths:` | Timing contracts: latency, age, drops |

The key difference: in launch files, topics are implicit in source code
and connected via `<remap>`. In manifests, topics are **first-class** —
declared with message type, QoS, and rate, and explicitly wired to node
endpoints.

### Directory Structure

Contracts are resolved through two channels, checked in order for every
scope (first hit wins): **overlay** then **provider**. Both derive the
same `<stem>` from the launch file — strip the `launch/` subdirectory and
`.launch.xml` / `.launch.py` extension, append `.contract.yaml`.

**Provider sidecar** (on by default) — the contract ships right next to
the launch file it describes, in the package's own install tree:

```
share/
├── tier4_control_launch/
│   └── launch/
│       ├── control.launch.xml
│       └── control.contract.yaml
├── tier4_system_launch/
│   └── launch/
│       ├── system.launch.xml
│       └── system.contract.yaml
└── autoware_launch/
    └── launch/
        ├── planning_simulator.launch.xml
        └── planning_simulator.contract.yaml
```

This is the natural home for a contract a package maintainer ships
alongside their own launch file: `<launch-file-dir>/<stem>.contract.yaml`.
Disable it with `--no-provider-contracts` (`play_launch check`/`launch`).

**User overlay** (`--contracts <dir>`) — a separate tree, keyed by
`package/launch/stem.contract.yaml`, for contracts a downstream user
supplies without touching (or having write access to) the installed
package:

```
<dir>/
├── tier4_control_launch/
│   └── launch/
│       └── control.contract.yaml
├── tier4_system_launch/
│   └── launch/
│       └── system.contract.yaml
└── autoware_launch/
    └── launch/
        └── planning_simulator.contract.yaml
```

The overlay channel is checked first, so an overlay entry always wins
over a provider sidecar for the same scope — useful for patching a
contract without rebuilding the package. `pkg: None` scopes (a launch
file referenced by raw path, not through a ROS package) use the `_`
convention in both channels: `<dir>/_/launch/<stem>.contract.yaml`.

The flat `<manifest-dir>/<pkg>/<stem>.yaml` layout (`--manifest-dir`) was
the original transitional channel and has been removed (Phase 40.6) now
that every known user has migrated to the provider/overlay layout above.

## The Manifest Model

A manifest describes a **scope** — one launch file's contribution to the
graph. Scopes contain nodes, topics, services, and child scopes.

![Manifest model: scopes, nodes, and wiring](img/manifest-model.png)

### Manifest Elements

- **Scope.** A manifest file describes one scope. A scope corresponds to
  one launch file (or one `<group>` block). Scopes form a tree that
  mirrors the launch file include hierarchy. The scope's namespace
  (from the launch tree) determines how relative topic keys are resolved.
  See [Scopes](#scopes), [Includes](#includes),
  [Directory Structure](#directory-structure).

- **Node.** A leaf execution entity — a ROS 2 node or composable node.
  Declares named **endpoints**: pub, sub, srv, cli. Optionally declares
  causal **paths** with timing constraints. Composable nodes appear as
  regular nodes — the container is a deployment detail. A composable node
  belongs to the manifest of the launch file that contains the
  `<load_composable_node>` tag, not the container's launch file.
  See [Nodes](#nodes).

- **Endpoint.** A named port on a node. Four kinds: `pub` (publishes),
  `sub` (subscribes), `srv` (serves a service), `cli` (calls a service).
  Endpoints can have properties: rate, state, required, max_age.
  See [Nodes](#nodes),
  [Subscriber Modes](#subscriber-modes-state-and-required).

  Endpoint names are **local to the node** within the manifest. The
  `topics:` section wires endpoints to ROS topics using `node/endpoint`
  refs:

  ```yaml
  nodes:
    controller:
      pub: [cmd]                   # "cmd" is the endpoint name

  topics:
    command/control_cmd:
      type: autoware_control_msgs/msg/Control
      pub: [controller/cmd]        # node_name/endpoint_name
  ```

  `pub:` / `sub:` appear at two levels — don't confuse them:
  - On a **node** — declares endpoint names (`pub: [cmd]`)
  - Inside a **topic** — lists which endpoints are wired (`pub: [controller/cmd]`)

- **Topic.** First-class wiring between endpoints. Topic keys are **ROS
  topic names** — either relative or absolute. See
  [Topic Name Resolution](#topic-name-resolution), [Topics](#topics),
  [Quality of Service](#quality-of-service),
  [Timestamps and Data Flow](#timestamps-and-data-flow).

  The same topic can appear in multiple manifests across the scope tree.
  Contract fields — `type:`, `rate_hz:`, `max_transport:`, the topic-level
  `qos:` block and `drop:` — must agree across all declarations; `pub:`
  and `sub:` endpoint lists are merged by the checker. Per-endpoint `qos:` overrides live on a node and are
  local to its declaring scope. Each scope only references its own nodes
  in endpoint lists.

- **Service / Action.** Request-response wiring. Service and action keys
  follow the same naming rules as topics — ROS names, relative or
  absolute. The same service can appear in multiple manifests; `type:`
  must agree, `server:`/`client:` are merged.
  See [Services and Actions](#services-and-actions).

- **Include.** A child scope. Maps to `<include>` in launch files. The
  include name is the ROS namespace (from `<push-ros-namespace>`). Each
  include references a child manifest file. See [Includes](#includes).

- **Args and Conditions.** Manifests can declare `args:` (named parameters
  resolved from the launch tree) and `if:` / `unless:` conditions on a
  node, topic, service, action or path — the five entities the grammar
  accepts them on; an include takes neither. These mirror `<arg>` and
  `if="$(var ...)"` in launch XML.
  See [Args](#args), [Conditions](#conditions).

  When args and conditions are used, the manifest goes through a pipeline
  before checking:

  1. **Substitute** — replace `$(var name)` with values from scope args
  2. **Filter** — remove entities where `if:` is false or `unless:` is true
  3. **Cleanup** — refs to removed conditional nodes are silently dropped;
     topics/services that lose all endpoints on both sides are removed
  4. **Check** — run validation rules on the filtered manifest

  Example: if `launch_validator` is `"false"`, the validator node is
  removed, and `[validator/input]` is dropped from the topic's sub list.
  If the topic still has publishers, it survives with a warning. If it
  loses both sides, it's silently removed.

- **Paths.** Named causal relations (trigger → output) with timing
  constraints: `max_latency`, `min_latency`, `max_jitter`, `drop:`,
  `miss:`, and `safe_state:` when the path is a hazard reaction. Declared on nodes
  (node-level paths, input/output are endpoint names) and scopes
  (scope-level paths, input/output are topic names). No launch file
  equivalent — this is the contract layer that manifests add.
  See [Paths](#paths), [Latency and Data Freshness](#latency-and-data-freshness),
  [Drop Budgets](#drop-budgets).

## Background

### Modularity and Standalone Checking

The manifest design is **modular** — each manifest is self-contained
and semantically valid regardless of where it sits in the launch
hierarchy. You can check the full `autoware.launch.xml` (all
subsystems), just `control.launch.xml` (one subsystem), or a single
leaf launch file. The checker produces valid results at every level.

This is why the same topic can appear in multiple manifests. When
`control.yaml` subscribes to `/localization/kinematic_state`, it
declares `type: nav_msgs/msg/Odometry` — even though `localization.yaml`
already declares the same type for the same topic. The duplication is
intentional: if you launch `control.launch.xml` alone (without
localization), the checker still knows the expected message type and
can validate the manifest independently.

The **`consistency` rule** is the mechanism that makes this work. It is a
cross-scope rule and runs only in the consumer's merge layer
(`resolve/src/ros/manifest_loader.rs`); there is no per-manifest half.

- When checking a single manifest, all declarations are local — no
  conflicts possible.
- When checking a manifest tree (multiple scopes), the checker merges
  declarations for the same resolved topic name. Five fields must agree
  where two scopes both declare them — `type:`, `rate_hz:`,
  `max_transport:`, the topic-level `qos:` block and `drop:` (same `N / W`
  and same `max_consecutive`) — and each is an error when they do not.
  Where only one scope declares a field, the merged topic takes it.
  `pub:` and `sub:` lists are merged. Per-endpoint `qos:` overrides are
  not merged — they apply to the endpoint they decorate.

This is the opposite of a centralized model where a parent manifest
"owns" topic declarations. A centralized model would break standalone
checking — a leaf manifest would be incomplete without its parent.

### Independence from ROS Launch

The manifest format is a **plain YAML specification** — it does not
parse launch files, evaluate substitutions, or depend on ROS
infrastructure. The manifest doesn't know its own namespace or args
until check time.

The **launch tree provides context** to the manifest:

- **Namespace**: from `<push-ros-namespace>` — used to resolve relative
  topic keys
- **Args**: from `<arg>` declarations and `<let>` assignments — used to
  substitute `$(var name)` and evaluate conditions
- **Parent-child relationships**: from `<include>` tags — used for
  scope-tree budget checks

This context is captured in the **scope table** (produced by the launch
parser; carried in the SystemModel's `structure.scopes` — the
`record.json` artifact that originally carried it is retired). The
checker receives the scope
table and applies it: substitute args, resolve relative names, filter
conditions. The manifest itself is inert.

This separation means manifests work with any tool that produces a
scope table — not just the ROS 2 launch system. A different build
system, a test harness, or a manual scope table all work the same way.

### Subscriber Modes: `state` and `required`

ROS 2 nodes subscribe to topics in two patterns:

- **Causal** (default) — the node reacts to each incoming message via a
  callback. If no messages arrive, the node does nothing. This creates a
  causal dependency in the dataflow graph.

- **Polled** (`state: true`) — the node reads the *latest* value when
  it needs it (e.g., on a timer tick), ignoring intermediate messages.
  Common pattern: `InterProcessPollingSubscriber` in Autoware. Polled
  subscriptions do **not** create causal dependencies.

The `required` flag is orthogonal — it says whether the node needs to
receive at least one message before it can operate.

**Example:** NDT scan matcher needs a pointcloud map before it can
localize, but reads the map once and polls for updates. Sensor points
drive the computation causally:

```yaml
nodes:
  ndt_scan_matcher:
    sub:
      sensor_points:
        min_rate_hz: 10          # causal — triggers alignment
      map:
        state: true              # polled — read latest, not every update
        required: true           # must receive at least one map
      initial_pose:
        state: true              # polled — feedback from EKF
    pub:
      ndt_pose:
        min_rate_hz: 10
```

**The four combinations:**

| `state` | `required` | Behavior | Example |
|---------|-----------|----------|---------|
| false | false | Causal, optional — callback on each message | Debug subscriber |
| false | true | Causal, must have data — node waits for first message | Sensor input |
| true | false | Polled, can be absent — reads latest or nothing | Velocity hint |
| true | true | Polled, needs initial value — reads latest, but must get one first | Map data |

**If omitted:** `state` defaults to `false` (causal). `required` defaults
to `false` (optional). An endpoint with no properties is causal and
optional — the most common case.

**Effect on the graph:** `state: true` breaks feedback cycles. If EKF
publishes a pose that NDT subscribes to as `state: true`, the checker
does not flag it as a causal cycle — the data flows but doesn't create
a dependency loop.

**Lifecycle (managed) nodes.** A ROS 2 lifecycle node has a state
machine (Unconfigured → Inactive → Active → Finalized) and only
publishes / runs callbacks in the Active state. Contracts on such
nodes apply to steady-state active operation; the runtime monitor
gates checks on the node being in the Active state, skipping
false violations during startup and state transitions.

Declare a lifecycle node with `lifecycle: true` on the node:

```yaml
nodes:
  lidar_driver:
    lifecycle: true
    pub:
      pointcloud: { min_rate_hz: 10 }   # applies when driver is Active
```

Static checking is unaffected — the flag is purely a signal for runtime
monitoring. See [Nodes](#nodes) for the field definition.

See [Nodes](#nodes) for the full endpoint property tables.

### Topic Name Resolution

Topic keys, service keys, and scope path `input:`/`output:` follow the
same resolution rule:

- **Absolute** (`/localization/kinematic_state`): used as-is
- **Relative** (`command/control_cmd`): resolved by the checker as
  `<scope_ns>/<key>` using the scope's namespace from the launch tree

Example: scope ns `/control`, topic key `command/control_cmd`
→ resolved: `/control/command/control_cmd`

The scope namespace is **not** declared in the manifest — it comes from
the launch tree's scope table at check time. This means the same
manifest resolves to different absolute names depending on where it's
included in the launch hierarchy.

**When to use each:**
- **Relative** for topics you publish — they're naturally in your namespace
- **Absolute** for topics you subscribe to from other scopes — makes the
  cross-scope dependency explicit
- **Relative** for intra-scope wiring between your own nodes

See [Topics](#topics) for the full field table and consistency rules.

### Dataflow Topologies

Nodes connect in three basic topologies, each with different latency
and timing behavior:

**Pipeline (series)** — the most common pattern. Each node processes
and forwards to the next. Latencies add:

```
in → [cropbox: 5ms] → [ground_filter: 15ms] → [detector: 30ms] → out
                        total: 5 + 15 + 30 = 50ms
```

In Autoware, the perception pipeline (sensor → preprocessing →
detection → tracking → prediction) is a series chain.

**Fork-join (parallel)** — two branches merge at a fusion node. The
fusion node waits for both inputs. Latency = max(branches) + fusion:

```
      ┌→ lidar detection (50ms) →┐
in →  │                          ├→ fusion (20ms) → out
      └→ camera detection (30ms) →┘
                   total: max(50, 30) + 20 = 70ms
```

In Autoware, object merger and radar fusion follow this pattern —
multiple sensor streams converge at a fusion node. See
[Multi-Input Fusion](#multi-input-fusion)
for how timestamps are handled at the fusion point.

**Periodic (timer-driven)** — a timer-driven node polls the latest
state from a buffer. Breaks the causal chain:

```
upstream → [state buffer] → [EKF, period=100ms] → out
             worst case: data waits up to one full period
```

In Autoware, the EKF localizer runs on a timer, polling buffered pose
and twist measurements. The multi-object tracker can also run
periodically with delay compensation.

See [contract-theory.md](contract-theory.md#composition) for the formal
composition rules (latency, rate, age, drop) for each topology.

### Derived Rates

A topic's publication rate is a **consequence**, not a declaration. The
facts are already in the manifest — a timer path publishes at its own
rate, an input-triggered path publishes at the rate its inputs arrive — so
`topics.<t>.rate_hz` written by hand is a second copy of something the
graph computes. The derivation is
`resolve/src/ros/manifest_graph.rs::derive_topic_rates`, memoised over the
merged topic index.

Per path:

| Trigger | Derived rate |
|---------|--------------|
| `timer: { rate_hz: R }` | **R**. This is the only source; nothing else creates messages. |
| `input: [...]`, **no** `sync:` | the **SUM** of its inputs' rates |
| `input: [...]`, **with** `sync:` | the **MIN** of its inputs' rates |
| `once` | `Unknown` — it fires exactly once |
| `spontaneous` | `Unknown` — the contract says nothing about when |
| unclassified (no trigger, no `input:`) | `Unknown` — a path with no declared trigger |

The fan-in case is the one worth stating twice. A subscription callback
fires once per message on *each* topic it is registered for, so a path
triggered by two 10 Hz topics runs **20 times a second**. Taking the min
in both cases is the natural-looking mistake, and it understates a fan-in
node's load by exactly the factor that decides whether it fits. A
synchroniser is the other case: it emits one output per matched set, so it
is paced by its slowest input — which is what `sync:` present *is*.

Per topic: the rates of every path producing it **add**, across endpoints
and across publishing nodes alike, because they publish independently.

`Unknown` is a first-class answer carrying a **reason**, never zero and
never an error. A topic with no declared publisher, a publisher endpoint
no declared path produces, or a cycle in the dataflow graph each yield
`Unknown` with that reason, and **any unknown contributor makes the whole
sum unknown** — a partial sum would be a lower bound presented as a rate.
Reporting 0 Hz would be a claim; omitting the topic silently would read as
"nothing to say".

**What reads the derivation:**

- `derivable-rate` (info) / `rate-mismatch` (warning) — a declared
  `topics.<t>.rate_hz` against the derived one. The info says the
  declaration is a deletable copy.
- `derivable-min-rate` (info) / `min-rate-mismatch` (warning) — the same
  for a publisher's `min_rate_hz`, which is the topic rate one hop
  earlier. Attributed **only where the topic has exactly one publisher**:
  with several the derived rate is their sum, and dividing it back out
  would present a bound as a rate. A promise *below* the derived rate gets
  nothing — it is a true but loose lower bound.
- `derived-rate-hierarchy` (warning) — a subscriber asking for more than
  the graph delivers. This is the form that survives deleting the declared
  `rate_hz`, which `rate-hierarchy` alone reads.
- `sync-feasibility` runs a second time on derived rates
  (`manifest_loader.rs::check_sync_feasibility_on_derived_rates`), only
  where an input's rate is derived but not declared, so deleting the
  declared copy cannot silence a real warning.

Measured on `rt_workspace`: deleting all three `rate_hz` **and** all five
`min_rate_hz` left the derived schedule byte-identical — eight of that
file's nine copies of `100` were consequences of its one timer.

### Latency and Data Freshness

Latency and age serve different concerns:

- **Latency** constrains processing — declared on **paths**
- **Age** constrains data freshness — declared on **subscriber endpoints**

**`max_latency`** — the time from when the *triggering input arrives
at this node/scope* to when the output is published. Declared on node
paths and scope paths.

```
sensor_points ──→ [cropbox: 5ms] ──→ [ground_filter: 15ms] ──→ [detector: 30ms]
                  ├── 5ms ──┤        ├──── 15ms ────┤          ├── 30ms ──┤
                  └────────────── scope max_latency: 50ms ─────────────┘
```

**Measurement points:**

- **Node `max_latency`**: from `rcl_take` of the trigger input to
  `rcl_publish` of the output. This is pure processing time — transport
  to the next node is NOT included.
- **Scope `max_latency`**: from the first `rcl_take` inside the
  scope to the last `rcl_publish` out of the scope. This INCLUDES
  internal transport between nodes within the scope.

A scope budget covers internal transport that individual node budgets do
not — but it is **not** required to exceed their sum, and the two checks
that read it differ on exactly this point. The per-manifest
`scope-budget` rule compares the budget against the flat sum Σ node
latencies + declared transport, which is conservative and wrong for
parallel branches; the consumer's topology-aware check compares it
against the critical path, where two branches contribute `max`, not their
sum. Where a route can be traced the second supersedes the first (see
[The derived route](#the-derived-route-and-what-it-costs)). Transport
latency can be declared per topic via `max_transport` (default for
all subscribers) and overridden per subscriber via the same field on
a `sub:` endpoint — a single ROS topic can have heterogeneous transport
(intra-process ~0ms, SHM <1ms, network 5-10ms) so per-subscriber
override is necessary for accurate critical-path computation. Topics
without `max_transport` (and no per-sub override) contribute 0 to
the budget sum — the undeclared transport is absorbed into the scope's
residual headroom.

**`max_age`** — the maximum acceptable age of data when a subscriber
receives it. Declared on **subscriber endpoints**, not on paths.

```
age = now - header.stamp (at the point of rcl_take)
```

Age is an end-to-end property: it includes all upstream latency,
transport delays, and processing before the data reached this
subscriber. The subscriber doesn't need to know the internal chain
structure — it just declares how fresh its input must be.

```yaml
nodes:
  planner:
    sub:
      objects:
        min_rate_hz: 10
        max_age: 200ms          # data must be fresher than 200ms
      map:
        state: true
        required: true           # no age constraint — map data can be old
```

**Runtime monitoring** checks `max_age` on every `rcl_take` via
the interception layer, which already reads `header.stamp`. If
`now - stamp > max_age`, a violation is flagged.

**Static checking** does not compare a subscriber's `max_age` against any
route total: an end-to-end age depends on when the stamp was set, which
the graph does not know for a route that crosses a timer. `max_age` is
read statically in exactly two places, both local. `lifespan-age` compares
it against the topic's QoS `lifespan` — a message discarded by the
middleware before the subscriber's age bound expires makes that bound
unmeetable. And the fault-reaction derivation reads it as a **late**
detector: `max_age` is one of the intervals within which a subscriber
would notice its assumption violated (see
[Fault detection and reaction](#fault-detection-and-reaction)).
Everything else about age is runtime.

See [Nodes](#nodes) for the `max_age` field table and
[Paths](#paths) for the `max_latency` field table. See
[Verification Rules](contract-theory.md#verification-rules) for the
full composition and checking rules.

**Heterogeneous transport on a single topic.** The same ROS 2 topic can
have subscribers with very different transport latency: an
intra-process subscriber sees ~0ms, a same-machine SHM subscriber sees
< 1ms, and a cross-network subscriber sees 5–10ms. To express this,
`max_transport` can be declared **on the subscriber endpoint** to
override the topic-level default. The checker uses the per-subscriber
value as the edge weight from the publisher to that subscriber in
critical-path computation:

```
edge[pub → sub].transport = sub.max_transport
                         ?? topic.max_transport
                         ?? 0
```

Critical path becomes per-sink:
`latency = max_pred( pred.latency + edge[pred → node].transport )
         + node.processing`.

```yaml
topics:
  /sensor/pointcloud:
    type: sensor_msgs/msg/PointCloud2
    pub: [lidar/output]
    sub: [perception/input, debug_recorder/input, remote_viz/input]
    max_transport: 10ms                # default (worst-case fallback)

nodes:
  perception:
    sub:
      input:
        max_transport: 0ms             # intra-process
  debug_recorder:
    sub:
      input:
        max_transport: 1ms             # same-machine SHM
  remote_viz:
    sub:
      input: {}                         # inherits topic default → 10ms
```

The override is sub-side only — a publisher does not know which
subscriber receives data over which transport, but a subscriber knows
how it consumes. Pub-side `max_transport` is not part of the format.

### Node and Scope Contracts

Contracts are defined at two levels, serving different roles:

**Node contract** — owned by the component developer. Declares what the
node needs (assumption) and what it promises (guarantee):

```yaml
nodes:
  ground_filter:
    sub:
      input: { min_rate_hz: 10 }        # assumption: 10 Hz input
    pub:
      output: { min_rate_hz: 10 }       # guarantee: 10 Hz output
    paths:
      main:
        input: input
        output: [output]
        max_latency: 15ms              # guarantee: 15ms processing
```

A node contract is testable in isolation — give it input at the assumed
rate and verify the output meets the guarantee. The
assumption/guarantee separation helps diagnosis at runtime:

| Assumption | Guarantee | Diagnosis |
|------------|-----------|-----------|
| met | met | Nominal |
| met | violated | **Node bug** — exceeds its declared budget |
| violated | met | Upstream problem, but this node is robust |
| violated | violated | Upstream problem — not this node's fault |

**Scope contract** — owned by the system integrator. Declares an E2E
budget across a subtree of nodes:

```yaml
paths:
  perception:
    input: /sensing/pointcloud
    output: [/perception/objects]
    max_latency: 85ms                  # E2E budget for the whole pipeline
    drop: 2 / 25                       # at most 2 of every 25 messages lost
```

The scope path uses topic names as entry/exit points. The checker
traces the dataflow between them, considering only nodes within the
scope's subtree.

**Partial decomposition** connects the two levels: start with the scope
budget (top-down), fill in node budgets as you measure them (bottom-up).
A node with no `max_latency` contributes nothing to either the flat sum
or the critical path, so an undeclared node is *transparent* rather than
free — the headroom it consumes is real and simply unaccounted. The
**residual** (how much of the scope budget remains after subtracting the
declared node budgets) is specified in
[contract-theory.md](contract-theory.md#partial-decomposition) but is
**not emitted today**; the only verdicts a checker gives on a scope path
are `scope-budget`, `scope-sampling-feasibility` and
`jitter-feasibility`.

See [contract-theory.md](contract-theory.md#what-is-a-contract) for
the formal contract definitions and
[Partial Decomposition](contract-theory.md#partial-decomposition) for
checker behavior with incomplete budgets.

### Drop Budgets

Messages can be lost in transport between nodes — DDS queue overflow,
network congestion, or QoS mismatch. The manifest lets you declare how
much loss is acceptable.

Loss is declared in a **`drop:`** block, on **topics** (transport
drops), on **scope paths** (E2E drops) and on **node paths** (messages
the node itself skips). It has two keys, and there is no third:

**`max_count: N / W`** — at most N of every W messages may be lost. A
count over a window, not a fraction: the drop *rate* is the checker's
`n / w`, so `2 / 100` is the 2% an earlier vocabulary spelled
`max_drop_rate: 0.02`. The window is what makes the claim falsifiable at
runtime — a bare fraction says nothing about over how many messages it
must hold.

**`max_consecutive: 3`** — never lose more than 3 in a row. Consecutive
drops cause visible glitches (e.g., a planner that misses 3 consecutive
obstacle updates).

A bare scalar is shorthand for `max_count` alone: `drop: 2 / 100`.

```yaml
topics:
  /sensing/pointcloud:
    type: sensor_msgs/msg/PointCloud2
    rate_hz: 10
    drop:
      max_count: 5 / 100       # 5% transport loss
      max_consecutive: 3       # never 3+ in a row

paths:
  perception:
    input: /sensing/pointcloud
    output: [/perception/objects]
    max_latency: 85ms
    drop:
      max_count: 8 / 100       # 8% E2E (all transport hops combined)
      max_consecutive: 5
```

**Rate-drop interaction:** a topic's effective delivery rate accounts
for drops. With `rate_hz: 10` and `drop: { max_count: 5 / 100 }`, the
subscriber effectively receives at least `10 * (1 - 0.05) = 9.5 Hz`. The
checker verifies: `rate_hz * (1 - n/w) >= sub.min_rate_hz`.

**Static vs runtime:** the static checker validates local consistency
(`n <= w`, a non-zero `max_consecutive`, effective delivery meets
subscriber demand). Drop **composition** along a route and
`max_consecutive` enforcement are **runtime-only** — they depend on
actual transport conditions that can't be proven statically.
See [Burstiness](contract-theory.md#burstiness) for runtime detection.

**If omitted:** no drop checking. The `drop-sanity` rule only fires
when a `drop:` block is declared.

See [Topics](#topics) and [Paths](#paths) for the `drop:` field tables.

### Quality of Service

QoS can be declared at two levels:

- **Topic-level** (`topics.<name>.qos`): default profile applied to every
  publisher and subscriber on the topic that does not specify its own.
- **Endpoint-level** (`nodes.<n>.pub.<ep>.qos` and
  `nodes.<n>.sub.<ep>.qos`): per-endpoint override of one or more fields.

**Allowed values:**

| Field            | Allowed values                 | Meaning |
|------------------|--------------------------------|---------|
| `reliability`    | `reliable`, `best_effort`      | Delivery guarantee |
| `durability`     | `volatile`, `transient_local`  | What a late joiner receives |
| `depth`          | integer                        | History depth |
| `history`        | `keep_last`, `keep_all`        | History kind |
| `lifespan`       | duration (`500ms`, `2s`, …)    | How long a message stays valid after publication |
| `liveliness`     | `automatic`, `manual_by_topic` | Who asserts the publisher is alive |
| `deadline`       | duration                       | Maximum time DDS allows between consecutive messages before reporting a violation |
| `lease_duration` | duration                       | Liveliness lease: how often a publisher must assert it is alive |

An omitted field is **unspecified**, not defaulted. The checker does not
fill in the ROS 2 profile defaults, because a real deployment uses
several profiles (sensor data, services, parameters) whose defaults
differ — an assumed default would be a claim the manifest never made.
What the *middleware* then uses is the profile the node picked in its
own source, which the manifest does not see.

The `qos-compat` rule errors on invalid values like `reliability: maybe`.

**Override semantics (field-level):**

The effective QoS for an endpoint is computed per-field:

```
effective.<field> = endpoint.qos.<field>
                 ?? topic.qos.<field>
                 ?? unspecified
```

Endpoint declarations override topic-level on a per-field basis — an
endpoint that overrides only `reliability` still inherits `depth` and
`durability` from the topic. `qos: {}` on an endpoint inherits the
topic default in full. Overrides are silent — they are an intentional
DDS-level pattern (e.g., a reliable logger and a best-effort visualizer
on the same sensor topic).

Topic-level QoS is subject to the cross-scope `consistency` rule: when
the same topic is declared in multiple scopes, the topic-level `qos:`
blocks must agree. Endpoint-level overrides live on a node and are local to the
declaring scope — they do not participate in cross-scope merge.

**Pub/sub compatibility (`qos-match` rule):**

After cross-scope merge, the checker computes the effective QoS for
every publisher and subscriber on each merged topic and checks DDS
compatibility for every (pub, sub) pair. The compatibility rule is
**offered ≥ requested**:

| Field         | Pub                | Sub                | Compatible? |
|---------------|--------------------|--------------------|-------------|
| `reliability` | `reliable`         | `reliable`         | yes |
| `reliability` | `reliable`         | `best_effort`      | yes |
| `reliability` | `best_effort`      | `reliable`         | **no** |
| `reliability` | `best_effort`      | `best_effort`      | yes |
| `durability`  | `transient_local`  | `transient_local`  | yes |
| `durability`  | `transient_local`  | `volatile`         | yes |
| `durability`  | `volatile`         | `transient_local`  | **no** |
| `durability`  | `volatile`         | `volatile`         | yes |
| `liveliness`  | `manual_by_topic`  | `automatic`        | yes |
| `liveliness`  | `manual_by_topic`  | `manual_by_topic`  | yes |
| `liveliness`  | `automatic`        | `automatic`        | yes |
| `liveliness`  | `automatic`        | `manual_by_topic`  | **no** |

`lease_duration` is checked as a number rather than a token, on the same
offered ≥ requested principle: the publisher's lease is how often it
promises to assert, the subscriber's is how long it waits before
declaring the publisher dead, so `pub.lease_duration > sub.lease_duration`
is an error — that is exactly a publisher the subscriber will
periodically declare dead. Both were parsed and dropped until phase 70,
while the other policies were checked (`check/src/rules/qos_match.rs`).

A field is checked only when both sides have it specified (directly or
inherited from topic-level). Fields specified on only one side are
skipped — the checker does not assume ROS 2 defaults, because real
deployments use multiple QoS profiles (sensor data, services,
parameters) with different defaults.

`history`, `depth`, `lifespan` and `deadline` are not checked pairwise by
`qos-match`. `lifespan` is read by the consumer's cross-scope
`lifespan-age` rule (a lifespan shorter than a subscriber's `max_age`
makes the age requirement unmeetable); `deadline` is derived onto the
running node by the resolver rather than compared between endpoints.

Arg conditions are handled by **filtering, not by enumeration**:
`if:`/`unless:` are evaluated and the removed endpoints are gone from the
manifest before any rule runs, so `qos-match` only ever sees a (pub, sub)
pair that exists in the configuration being checked. Whether some *other*
arg assignment would produce a broken graph is the `satisfiability`
rule's question, and it is the only rule that enumerates arg models.

**Example:**

```yaml
topics:
  /sensor/pointcloud:
    type: sensor_msgs/msg/PointCloud2
    pub: [lidar_driver/output]
    sub: [perception/input, debug_recorder/input]
    qos:                          # default for all endpoints on this topic
      reliability: best_effort
      depth: 5

nodes:
  perception:
    sub:
      input:
        qos: { reliability: reliable }   # override → qos-match error
  debug_recorder:
    sub:
      input: {}                          # inherits topic default — ok
```

The pub `lidar_driver/output` (effective `reliability: best_effort`)
against sub `perception/input` (effective `reliability: reliable`)
produces a `qos-match` error.

### Timestamps and Data Flow

Timestamps (`header.stamp`) are the thread that connects latency, age,
and fan-in synchronisation. The manifest imposes rules on how timestamps flow
through the graph.

**Causal paths should preserve timestamps.** When a node has a causal
path `input → output`, the output message's `header.stamp` should equal
the input's `header.stamp`. The manifest assumes this convention for age
tracking. Nodes that reset the stamp (e.g., using current time) should
be modeled as periodic paths (`trigger: { timer: { rate_hz: ... } }`)
even if they are callback-driven. This is how data provenance is tracked
through a pipeline:

```
sensor (stamp=T) → cropbox (stamp=T) → detector (stamp=T) → planner
                                                                │
                                         age = now - T ─────────┘
```

This is what makes `max_age` meaningful. At any point in the chain,
age is `now - header.stamp`, and the stamp traces back to the original
sensor reading.

**Periodic nodes reset the timestamp chain.** A timer-driven node
(a `trigger: { timer: ... }` path) generates its own timestamps — the output
`stamp` is the current time, not propagated from an input. For example,
the EKF localizer runs on a 10 Hz timer, polls buffered pose/twist
measurements (`state: true`), and publishes with `stamp = now`.
Subscribers downstream see age relative to the EKF's timer, not the
original sensor.

**State subscribers don't contribute timestamps.** A `state: true`
subscriber reads the latest value regardless of its timestamp. The
state data's `stamp` is *not* propagated to the output — only causal
inputs contribute. EKF reads map data (`state: true`, stamp from minutes
ago) and sensor data (causal, `stamp=T`). The output pose has `stamp=T`,
not the map's ancient timestamp.

### Multi-Input Fusion

When a node fuses multiple inputs (the fork-join topology from
[Dataflow Topologies](#dataflow-topologies)), the manifest states how the
callback treats them with `sync:` on the node path. Analysis of 9 Autoware
fusion nodes shows two dominant patterns:

**Pattern 1: Timestamp synchronization** (object merger, radar fusion,
cluster merger, image projection fusion). Inputs are matched by stamp via
`message_filters::ApproximateTimeSynchronizer`; one output per matched set.

```yaml
nodes:
  object_merger:
    sub:
      lidar_objects: { min_rate_hz: 10 }
      radar_objects: { min_rate_hz: 10 }
    pub:
      merged: { min_rate_hz: 10 }
    paths:
      main:
        trigger: { input: [lidar_objects, radar_objects] }
        output: [merged]
        sync: { policy: approximate, max_interval: 50ms }
        tolerance: 50ms          # stamp spread the callback still accepts
        max_latency: 20ms
```

**Pattern 2: Primary input with polled secondaries** (map-based prediction,
BEVFusion, distortion corrector). The node triggers on one causal input and
reads the latest value of `state: true` subscriptions; no `sync:` at all.

```yaml
nodes:
  map_based_prediction:
    sub:
      tracked: { min_rate_hz: 10 }
      vector_map: { state: true, required: true }
      traffic_signals: { state: true }
    pub:
      predicted: { min_rate_hz: 10 }
    paths:
      main:
        trigger: { input: [tracked] }   # only causal inputs are in the path
        output: [predicted]
        max_latency: 15ms
```

`sync-feasibility` checks the window against the inputs' rates — declared
or derived — and `sync-budget` checks it against `max_latency`. The
presence of `sync:` is also what decides fan-in rate derivation: the
**min** of the inputs with it, the **sum** without (see
[Derived Rates](#derived-rates)).

> `correlation: timestamp | latest` used to sit beside `sync:`. Phase 70
> removed it: it was parsed, exported and lowered into the model, and no
> check, mapper or monitor ever branched on it. `sync:` present/absent is
> the same distinction, and it is read.

## Worked Example

A perception pipeline with tracking and prediction stages.

**Launch files:**

```xml
<!-- perception.launch.xml (ns: /perception/object_recognition) -->
<push-ros-namespace namespace="perception/object_recognition"/>
<include file="tracking/tracking.launch.xml"/>
<include file="prediction/prediction.launch.xml"/>

<!-- tracking/tracking.launch.xml (ns: .../tracking) -->
<push-ros-namespace namespace="tracking"/>
<node pkg="autoware_multi_object_tracker" exec="tracker"/>

<!-- prediction/prediction.launch.xml (ns: .../prediction) -->
<push-ros-namespace namespace="prediction"/>
<node pkg="autoware_map_based_prediction" exec="predictor"/>
```

**Manifest files:**

Each manifest declares topics using ROS topic names as keys. Relative
keys are resolved by the checker using the scope's namespace from the
launch tree. Each scope only references its own nodes in endpoint lists.

```yaml
# tier4_perception_launch/tracking.contract.yaml
# scope ns (from launch tree): /perception/object_recognition/tracking
version: 1

nodes:
  multi_object_tracker:
    sub:
      detected: { min_rate_hz: 10 }
    pub:
      tracked: { min_rate_hz: 10 }
    paths:
      main: { input: detected, output: [tracked], max_latency: 20ms }

topics:
  # relative → /perception/object_recognition/tracking/objects
  objects:
    type: autoware_perception_msgs/msg/TrackedObjects
    pub: [multi_object_tracker/tracked]
    rate_hz: 10
```

```yaml
# tier4_perception_launch/prediction.contract.yaml
# scope ns (from launch tree): /perception/object_recognition/prediction
version: 1

nodes:
  map_based_prediction:
    sub:
      tracked:
        min_rate_hz: 10
        max_age: 150ms            # tracked objects must be fresher than 150ms
      vector_map: { state: true, required: true }
    pub:
      predicted: { min_rate_hz: 10 }
    paths:
      main: { input: tracked, output: [predicted], max_latency: 15ms }

topics:
  # absolute — subscribes to tracking's output topic
  /perception/object_recognition/tracking/objects:
    type: autoware_perception_msgs/msg/TrackedObjects
    sub: [map_based_prediction/tracked]

  # absolute — subscribes to map data from outside perception
  /map/vector_map:
    type: autoware_map_msgs/msg/LaneletMapBin
    sub: [map_based_prediction/vector_map]

  # relative → /perception/object_recognition/prediction/objects
  objects:
    type: autoware_perception_msgs/msg/PredictedObjects
    pub: [map_based_prediction/predicted]
```

```yaml
# tier4_perception_launch/perception.contract.yaml
# scope ns (from launch tree): /perception/object_recognition
version: 1

includes:
  tracking:
    manifest: tier4_perception_launch/tracking.contract.yaml
  prediction:
    manifest: tier4_perception_launch/prediction.contract.yaml

paths:
  main:
    input: /perception/obstacle_segmentation/pointcloud
    output: [prediction/objects]       # relative → /perception/object_recognition/prediction/objects
    max_latency: 50ms
    drop: 5 / 100
```

Key points:
- **No scope interface** — each manifest declares topics directly using
  ROS topic names. No `pub:`/`sub:` export/import groups.
- **Topic keys are ROS names** — `objects` in tracking.yaml resolves to
  `/perception/object_recognition/tracking/objects`. prediction.yaml
  subscribes using the absolute name.
- **Consistency across scopes** — the topic
  `/perception/object_recognition/tracking/objects` appears in both
  tracking.yaml (pub) and prediction.yaml (sub). The `type:` must agree;
  the checker merges `pub:` and `sub:` lists.
- **Each scope is self-contained** — prediction.yaml can be checked
  standalone (e.g., when launching prediction.launch.xml directly).

### Example: Args, Conditions, and State

A control scope with one always-present controller and an optional
validator gated by a boolean launch arg:

```yaml
# tier4_control_launch/control.contract.yaml
# scope ns (from launch tree): /control
version: 1

args:
  launch_validator:
    type: bool                   # enables satisfiability checking

nodes:
  controller:
    sub:
      trajectory:
        min_rate_hz: 10          # causal — triggers control loop
      operation_mode:
        state: true              # polled — read latest, not every update
        required: true           # must know mode before operating
    pub:
      control_cmd:
        min_rate_hz: 30
    paths:
      main:
        input: trajectory
        output: [control_cmd]
        max_latency: 10ms

  validator:
    if: $(var launch_validator)  # only present when arg is "true"
    sub:
      control_cmd: {}
      predicted_trajectory: {}

topics:
  # relative → /control/command/control_cmd
  command/control_cmd:
    type: autoware_control_msgs/msg/Control
    pub: [controller/control_cmd]
    sub: [validator/control_cmd]  # auto-optional: validator is conditional
    rate_hz: 30

  # absolute — subscribes to topic from planning subsystem
  /planning/trajectory:
    type: autoware_planning_msgs/msg/Trajectory
    sub: [controller/trajectory]

  # absolute — subscribes to topic from system subsystem
  /system/operation_mode/state:
    type: autoware_system_msgs/msg/OperationModeState
    sub: [controller/operation_mode]

paths:
  control:
    input: /planning/trajectory
    output: [command/control_cmd]   # relative → /control/command/control_cmd
    max_latency: 15ms
```

Key features demonstrated:
- **Topic keys are ROS names** — relative `command/control_cmd` resolves
  to `/control/command/control_cmd`; absolute `/planning/trajectory`
  reaches outside the scope
- **`args:` with `type: bool`** — enables Z3 satisfiability checking
  across all valid configurations
- **`if:`** — validator only exists when `launch_validator` is `"true"`;
  its topic refs are automatically dropped when it's filtered out
- **`state: true`** — operation_mode is polled, doesn't create a causal
  dependency in the dataflow graph
- **`required: true`** — controller needs at least one operation_mode
  message before it can operate
- **Scope path** — E2E latency budget; `input:`/`output:` reference
  topic names (same resolution rules as topic keys)

## Format Reference

> **The exhaustive key list is [format-reference.md](format-reference.md)**,
> generated from `types/src/field_table.rs` and checked by a test. That file
> is normative: a key it does not list is a parse error. This section stays as
> the *tutorial* pass — syntax, defaults, and when to use each construct — and
> may lag on completeness where the generated table cannot.
>
> The split exists because prose could not be kept complete by hand. Measured
> on 2026-09-04, this section was missing 22 of 66 fields, six of which
> appeared nowhere in this document at all.

Use this section as a lookup reference. Each subsection shows the YAML
syntax, field table with defaults, and when to use.

### Metadata

| Field     | Required | Description | If omitted |
|-----------|----------|-------------|------------|
| `version` | no       | Format version (currently `1`) | `1` — `parse.rs` reads `yaml_u32("version").unwrap_or(1)` |

Every other manifest-level key opens one of the sections below. The
complete top-level vocabulary, and nothing else, is:

| Key | Section |
|-----|---------|
| `args` | [Args](#args) |
| `nodes` | [Nodes](#nodes) |
| `topics` | [Topics](#topics) |
| `services`, `actions` | [Services and Actions](#services-and-actions) |
| `includes` | [Includes](#includes) |
| `paths` | [Paths](#paths) — scope paths |
| `external_topics` | [External Topics](#external-topics) |
| `hazards` | [Fault detection and reaction](#fault-detection-and-reaction) |
| `severity_levels` | [Severity scale](#severity-scale) |
| `functions`, `modes` | [Operational modes](#operational-modes) |

> `exclude_patterns` was accepted here until phase 70. It had three
> mentions in the entire codebase — the grammar row, the struct field and
> the parse line — so it excluded nothing. A side that is expected to be
> absent is declared with `external:` on the topic, service or action.

### Args

Declare args when your launch file has `<arg>` declarations that affect
the graph topology or topic names.

```yaml
args:
  input_topic:                     # free string (default)
  launch_feature:
    type: bool                     # "true" or "false" only
  pose_source:
    choices: [ndt, eagleye, gnss]  # enum — explicit valid values
```

| Field | Description | If omitted |
|-------|-------------|------------|
| *(bare name)* | Free string, no constraint | — |
| `type: bool` | Only `"true"` or `"false"` accepted | Free string |
| `choices: [...]` | Only listed values accepted | Free string |

`$(var name)` substitutions work in any string field. Resolved before
condition evaluation and static checks.

Typed args (`bool`, `choices`) enable satisfiability checking — the
checker can verify all valid arg combinations produce sound manifests.

### Conditions

Use `if:` when a node or topic only exists in certain launch configurations.

```yaml
nodes:
  validator:
    if: $(var launch_validator)         # boolean: true when "true"
  legacy:
    unless: $(var use_new_mode)         # included when NOT "true"
  sensor:
    if: $(var mode) == 'velodyne'       # string comparison
```

Supports `==`, `!=`, `and`, `or`, parentheses. All comparisons are
string equality.

After filtering, refs to conditional nodes that were removed are
silently dropped. Refs to unconditional nodes are always required.

### Scopes

Each manifest file describes one scope — one launch file's contribution
to the graph. The scope's properties come from the launch tree, not the
manifest:

- **Namespace**: from `<push-ros-namespace>` in the launch file. Used
  to resolve relative topic/service keys at check time.
- **Parent/child relationships**: from `<include>` tags. Determines the
  scope tree for budget checks.
- **Args**: from `<arg>` declarations and `<let>` assignments, captured
  in the scope table.

<!-- yaml-check: skip — a shape sketch: `{ ... }` is an ellipsis, not YAML -->

```yaml
# tracking.yaml
# Scope properties (from launch tree, not declared here):
#   ns: /perception/object_recognition/tracking
#   parent: perception.yaml
#   args: { ... }
version: 1

nodes: { ... }
topics: { ... }
```

A scope can contain nodes, topics, services, includes (child scopes),
and paths. When the checker loads a manifest tree, it walks the scope
hierarchy for budget-overflow and scope-budget checks.

### Nodes

Declare a node for each ROS 2 node or composable node in the launch file.
The manifest node name must match the ROS 2 **node name** — the `name=`
attribute in the launch XML (or `__node:=` remap). This is the name that
appears in `ros2 node list`.

```yaml
nodes:
  controller:
    pub:
      cmd:
        min_rate_hz: 30
    sub:
      trajectory:
        min_rate_hz: 10
      map:
        state: true
        required: true
    srv:
      trigger:
        max_response: 100ms
    cli:
      operate: {}
    paths:
      main:
        input: trajectory
        output: [cmd]
        max_latency: 10ms

  lidar_driver:
    lifecycle: true              # managed node — contracts apply when Active
    pub:
      pointcloud: { min_rate_hz: 10 }
```

Endpoints can be a list (`pub: [a, b]`) or a map with properties.

**Node properties:**

| Field         | Meaning                                       | If omitted |
|---------------|-----------------------------------------------|------------|
| `lifecycle`   | True if ROS 2 lifecycle (managed) node — contracts are runtime-gated on Active state | `false` — regular node |
| `if` / `unless` | Condition                                   | Always included |
| `criticality` | `high` \| `medium` \| `low`, and **nothing else** — the set is closed and any other value is a parse error. A **consequence**, not a hint: since phase 72 it is derived from the hazards a node feeds, detects or reacts for, and the derivation is what the mapper reads. The label stands only where no hazard reaches the node | Derived from the hazards; if none reaches this node, it buckets below all criticality-tagged nodes |

`criticality` used to accept any string, and its one reader answered an
unknown value with a debug log and `None` — so `criticality: urgent`
scheduled a node exactly as if nothing had been declared. Where a hazard
does reach the node, the consumer compares the label against the
derivation: `derivable-criticality` (info) when they agree,
`criticality-mismatch` (warning) when they do not. See
[Fault detection and reaction](#fault-detection-and-reaction)
for `hazards:`, and the `severity_levels:` scale they draw from.

**Subscriber properties:**

| Field              | Meaning                                       | If omitted |
|--------------------|-----------------------------------------------|------------|
| `min_rate_hz`      | Minimum expected receive rate                 | Not checked |
| `max_rate_hz`      | Maximum expected receive rate                 | Not checked |
| `max_age`       | Max data age at receive (`now - header.stamp`) | Not checked |
| `state`            | Polled (read-latest), not causal              | `false` — causal |
| `required`         | Must receive at least once before operational | `false` — optional |
| `qos`              | Per-endpoint QoS override (see [QoS](#quality-of-service)) | Inherits topic-level `qos:` |
| `max_transport` | Per-subscriber transport latency override (a duration) — used as the edge weight from publisher to this subscriber in critical-path computation | Inherits topic-level `max_transport` |
| `buffer`           | Buffering discipline for a `state: true` subscriber: `latest` \| `queue` | `latest`; a parse error without `state: true` |
| `on_violation`     | The reaction this subscriber owes when its assumption is violated (see [Fault detection and reaction](#fault-detection-and-reaction)) | This subscriber detects nothing |

**Publisher properties:**

| Field         | Meaning                                   | If omitted |
|---------------|-------------------------------------------|------------|
| `min_rate_hz` | Minimum publish rate — a **fact** on a publisher, and derivable (`derivable-min-rate`) | Not checked |
| `max_rate_hz` | Maximum publish rate — checked by `rate-hierarchy` since phase 70 | Not checked |
| `qos`         | Per-endpoint QoS override (see [QoS](#quality-of-service)) | Inherits topic-level `qos:` |

`pub:`, `sub:` and `cli:` share **one** grammar
(`pub/sub/cli.<endpoint>` in
[format-reference.md](format-reference.md)), so the split above is by
meaning, not by what parses. The same key is a *fact* on a publisher and a
*requirement* on a subscriber: `min_rate_hz` on a `pub:` is what the node
promises to produce, on a `sub:` it is what the node needs to receive, and
`rate-hierarchy` reads the two ends against the channel between them.

**`jitter:` on an endpoint was removed** (phase 68). It was declared, copied
into the model, and read by nothing — its own row in this table said *"Not
checked"* for its whole life. Jitter is a property of a ROUTE, not of one
endpoint: what destabilises a controller is how much the end-to-end latency
varies, and a single publisher's spread does not determine that. Declare
`max_jitter:` on the path or scope path you mean; `jitter-feasibility` checks
it against the sampling jitter the route already carries.

**Service-server properties (`srv:`):**

| Field          | Meaning                      | If omitted |
|----------------|------------------------------|------------|
| `max_response` | Deadline for answering a request on this service | The node's deadline is taken from its paths alone |

`max_response` is the **only** key `srv.<endpoint>` accepts; anything else
is a parse error. It is a deadline, so it is read like one: the derivation
takes a node's `deadline_us` as the **min over its paths' `max_latency` and
its services' `max_response`** (`derive/src/view.rs`,
`derive/src/lib.rs`), which is why a node declaring nothing but
`srv: { lookup: { max_response: 5ms } }` is still schedulable. The
cross-scope `response-blocking` rule reads the same number against the
node's own callback declarations.

A **client** endpoint (`cli:`) is an ordinary endpoint and takes the
`pub/sub/cli.<endpoint>` keys, not `max_response` — a client does not
promise a response time, it consumes one.

**Declared parameters (`params:`)** — the parameters the node declares,
by name and ROS 2 type, and nothing else. A string's capacity or an
array's bound is a board fact, not a contract one:

```yaml
nodes:
  mrm_handler:
    params:
      update_rate: { type: integer }
      timeout_operation_mode_availability: { type: double }
      use_emergency_holding: { type: bool }
      turning_hazard_on.emergency: { type: bool }
```

`type:` is required under each name and the set is closed — `bool`,
`integer`, `double`, `string`, `byte_array`, `bool_array`,
`integer_array`, `double_array`, `string_array`
(`rcl_interfaces/msg/ParameterType` less `NOT_SET`). An unknown spelling
is a parse error, not a skipped entry.

`params: {}` and a missing `params:` are **different statements**:
the first says the node declares no parameters, the second says the
contract does not state. The distinction survives into the model as an
empty `contracts.node_params` entry versus no entry at all, so a consumer
sizing a parameter store from the declarations can tell them apart.

**Path exclusion (`concurrency:`)** — which of a node's paths may **not**
run at the same time:

```yaml
nodes:
  detector:
    sub: { image: { min_rate_hz: 30 } }
    pub: { boxes: {}, masks: {} }
    concurrency:
      exclusive:
        - [to_boxes, to_masks]
    paths:
      to_boxes:
        trigger: { input: [image] }
        output: [boxes]
        max_latency: 20ms
      to_masks:
        trigger: { input: [image] }
        output: [masks]
        max_latency: 35ms
```

`exclusive:` is a list of groups of path names. Groups sharing a member
are merged transitively, so `[[a, b], [b, c]]` is the one group
`{a, b, c}`: exclusion is not transitive by intent, but a shared member
makes all three serialise in any realization that maps a group to one
thread. A maximal mutually exclusive set **is** a callback group, which is
why the group is derived from the relation rather than written.

The default is the load-bearing part. An **absent** `concurrency:` means
every path of the node is in one group — which is what both realizations
already do (`rclcpp`'s implicit per-node callback group is
`MutuallyExclusive`, and nano-ros's `default_cbg_type` is the same), so an
author writes nothing unless claiming *more* concurrency than the safe
answer. An explicit `exclusive: []` is therefore **not** the same as
omitting the section: it says every path may run concurrently. The two
stay distinct through parsing (`types/src/parse.rs::parse_concurrency`)
and into the model, and the derivation reads the difference —
`claims_concurrency` is true unless one merged group covers every declared
path (`derive/src/lib.rs`). Summing a route's latencies is sound only
under the serialising default; the cross-scope `concurrency-decl` and
`path-exclusion` rules report declarations that contradict each other.

### Topics

Declare a topic when your scope publishes or subscribes to it. Topic
keys are **ROS topic names** — relative or absolute. See
[Topic Name Resolution](#topic-name-resolution) for the resolution rule
and guidance on when to use each.

The same topic can appear in multiple manifests across the scope tree.
Contract fields (`type:`, `rate_hz:`, `max_transport:`, topic-level
`qos:`, `drop:`) must agree; endpoint lists (`pub:`, `sub:`) are merged by
the checker. Per-endpoint
`qos:` overrides on a node's `pub:`/`sub:` entries are local to the
declaring scope and not subject to cross-scope agreement.

```yaml
topics:
  # relative — resolved using scope ns
  command/control_cmd:
    type: autoware_control_msgs/msg/Control
    pub: [controller/cmd]
    sub: [validator/input]
    rate_hz: 30
    drop:
      max_count: 1 / 100          # 1% transport loss
      max_consecutive: 3
    max_transport: 5ms            # cross-machine hop
    qos:
      reliability: reliable
      durability: transient_local
      depth: 1

  # absolute — cross-scope subscription
  /planning/trajectory:
    type: autoware_planning_msgs/msg/Trajectory
    sub: [controller/trajectory]
```

| Field              | Required | Description | If omitted |
|--------------------|----------|-------------|------------|
| `type`             | yes      | ROS message type (`pkg/msg/Name`) | Error |
| `pub`              | no       | Publisher endpoint refs (`node/endpoint`) | Empty list |
| `sub`              | no       | Subscriber endpoint refs | Empty list |
| `rate_hz`          | no       | Negotiated channel rate. A **consequence** — derivable from the timers that drive it (see [Derived Rates](#derived-rates)) | Rate hierarchy not checked against a declared rate; `derived-rate-hierarchy` still checks the derived one |
| `drop`             | no       | Permitted transport loss: `{ max_count: N / W, max_consecutive: K }`, or the bare `N / W` shorthand | Drop not checked |
| `max_transport`    | no       | Worst-case transport latency (a duration) — default for every subscriber on this topic; overridable per `sub:` endpoint | 0 — absorbed into scope residual |
| `qos`              | no       | QoS profile | QoS not validated |
| `external`         | no       | Mark one side as provided outside the tree: `pub` \| `sub` \| `both` | Both sides expected internally |
| `if`/`unless`      | no       | Condition | Always included |

`type` is required in every topic declaration so each manifest is
self-contained for standalone checking. The cross-scope `consistency`
rule validates that all declarations of the same resolved topic agree.

**Rate hierarchy with drops:**

```
pub.min_rate_hz  >=  rate_hz  >=  rate_hz * (1 - n/w)  >=  sub.min_rate_hz
     30                30          30 * (1 - 0.01) = 29.7      29
                                   drop: { max_count: 1 / 100 }
```

The publisher must produce at least as fast as the channel rate. The
effective delivery rate (after transport drops) must meet every
subscriber's minimum demand. Think of it as: supply ≥ channel ≥
effective delivery ≥ demand.

`max_rate_hz` bounds the same hierarchy from above, and since phase 70
`rate-hierarchy` checks it: `pub.max_rate_hz >= rate_hz` and
`rate_hz <= min(sub.max_rate_hz)`. This is the half that matters for
queue overrun — the OVER-fast publisher is the one that overruns a
subscriber, and a topic faster than a subscriber can drain backs up
regardless of scheduling.

When a topic is declared across multiple scopes, the checker merges
all declarations before running rate and drop checks. The publisher's
`rate_hz` in one scope is checked against the subscriber's `min_rate_hz`
in another.

### Services and Actions

Service and action keys follow the same naming rules as topics — **ROS
names**, either relative or absolute. The same service can appear in
multiple manifests; `type:` must agree, `server:`/`client:` are merged.

```yaml
services:
  # relative → /system/mrm/operate
  mrm/operate:
    type: tier4_system_msgs/srv/OperateMrm
    server: [operator/operate]
    client: [handler/operate]

  # absolute — cross-scope service call
  /system/mrm/comfortable_stop:
    type: tier4_system_msgs/srv/OperateMrm
    client: [mrm_handler/comfortable_stop_operate]

actions:
  # relative → resolved via scope ns
  navigate:
    type: nav2_msgs/action/NavigateToPose
    server: [navigator/navigate]
    client: [planner/navigate]
```

### Includes

Child scopes. The name is the ROS namespace.

```yaml
includes:
  tracking:
    manifest: tier4_perception_launch/tracking.contract.yaml
  prediction:
    manifest: tier4_perception_launch/prediction.contract.yaml
```

The `manifest:` value is **informational** — child contract files are
not loaded through it. The launch tree's own `<include>` structure
determines the child scopes, and each child's contract file is resolved
per launch file through the overlay/provider channels (see
[Directory Structure](#directory-structure)). The include entry exists
to name the scope for budget checks.

`manifest:` is the **only** key the file-reference form accepts. An
`if:` / `unless:` on an include is a parse error, in both forms — the
condition that decides whether a child scope exists lives in the launch
file, and the launch tree is what produces the scope table the checker
walks. Condition a *node*, a *topic* or an entity inside the child
manifest instead.

Inline includes (for `<group>` blocks) embed the manifest structure
directly instead of referencing a file. The value is then a whole
manifest, and takes manifest-level keys only:

```yaml
includes:
  sensor_group:
    nodes:
      lidar_driver:
        pub: [pointcloud]
    topics:
      pointcloud:
        type: sensor_msgs/msg/PointCloud2
        pub: [lidar_driver/pointcloud]
```

### Paths

Named causal relations with timing constraints. Declared on nodes and
scopes. See [Latency and Data Freshness](#latency-and-data-freshness) for definitions.

```yaml
# Node-level path
nodes:
  centerpoint:
    sub: [pointcloud]
    pub: [objects]
    paths:
      main:
        trigger: { input: [pointcloud] }
        output: [objects]
        max_latency: 30ms

# Scope-level path (latency + E2E drops)
# input/output are topic names (relative or absolute)
# The checker traces dataflow between these topics, considering
# only nodes within this scope's subtree (from includes: tree)
paths:
  perception:
    input: /perception/obstacle_segmentation/pointcloud
    output: [/perception/object_recognition/prediction/objects]
    max_latency: 85ms
    drop:
      max_count: 8 / 100
      max_consecutive: 5
```

Node paths and scope paths share **one** grammar (`paths.<name>` in
[format-reference.md](format-reference.md)); what differs is what the
names in `input:`/`output:` refer to — endpoint names on a node,
topic names on a scope. The fields below are the common ones:

| Field          | Meaning | If omitted |
|----------------|---------|------------|
| `trigger`      | What causes the output: `{ timer: { rate_hz } }`, `{ input: [...] }`, `once`, `spontaneous` | Derived from `input:`; see below |
| `input`        | Legacy trigger spelling — prefer `trigger: { input: [...] }` | See `trigger` |
| `output`       | Result endpoint(s) or topic(s) | Required |
| `max_latency`  | Worst-case input-to-output time (see definition above) | Not checked; parent looks through (transparent) |
| `min_latency`  | Best-case latency. Exists so `max_jitter` is falsifiable — usually measured, not authored | `max_jitter` reported unverifiable |
| `max_jitter`   | Permitted variation in this path's latency | Not checked |
| `sync`         | Fan-in policy: `exact`, `approximate` or `timeout_any` with its window | Inputs unsynchronised; each fires the callback |
| `tolerance`    | Max `header.stamp` spread between inputs still treated as one set | Not checked against the budget |
| `drop`         | Permitted loss along this path: `{ max_count: N / W, max_consecutive: K }` | Drop not checked |
| `miss`         | What a missed deadline costs: `{ tolerate: N / W, consecutive: K, action: continue \| skip_next \| abort }` | Not checked |
| `safe_state`   | What this path emits when it is a hazard reaction, and the plant's settle time | Path is not a reaction |

**An empty `input:` is not a timer.** With no `trigger:` and no
non-empty `input:`, the path is **unclassified**: no trigger fact is
derived, and it is never silently assumed to be periodic
(`PathDecl::effective_trigger`). Say `trigger: { timer: { rate_hz: 10 } }`
when you mean periodic — that rate is a real scheduling input, and an
unclassified path gives the mapper nothing.

Age is declared on **subscriber endpoints** (see
[Latency and Data Freshness](#latency-and-data-freshness)), not on
paths. Drops may be declared at all three levels: on a topic (transport
loss), on a scope path (E2E), and on a node path (messages the node
itself skips) — `drop-sanity` checks all three.

The checker traces the dataflow between the input and output topics,
considering only nodes within this scope's subtree. When a parent scope
and child scope declare paths with the same resolved (input, output)
topics, `budget-overflow` checks that the child's budget ≤ the parent's.

### External Topics

Topics produced or consumed by systems outside the loaded manifest tree
(hardware bridges, separate-package map loaders, joystick / teleop
sources, rviz consumers) cannot be expressed by `topics:` alone —
declaring them there would leave one side empty and trip
`dangling-entity`.

Two ways to mark a topic as external:

**1. Top-level `external_topics:` block** — manifest-wide list, useful
at the top of the launch tree where the boundary is known:

```yaml
external_topics:
  /tf:
    side: pub                      # external producer (we may sub)
    type: tf2_msgs/msg/TFMessage
  /vehicle/engage:
    side: pub
    type: autoware_vehicle_msgs/msg/Engage
  /visualization:
    side: sub                      # external consumer (we pub)
  /passthrough/relay:
    side: both                     # passthrough we don't model
    type: std_msgs/msg/String
    qos: { reliability: best_effort }
```

An entry takes exactly three keys: `side:` (required), `type:` and `qos:`.
`type:` is cross-checked against any internal `topics:` declaration of the
same FQN by `consistency`; `qos:` is the profile the external side uses,
and participates in `qos-match` like any other endpoint's.

> The side selector is also spelled `external:`, which is what
> `external_topics:` accepted first and what the field table marks
> **deprecated**. Both parse, `side:` wins when both are present
> (`types/src/parse.rs::parse_external_topics`), and `side:` is the
> spelling to write. Inside a `topics:` entry the key is `external:` and
> always was — the two blocks do not share a name for the same idea, which
> is the whole reason `side:` exists.

**2. Per-topic `external:` flag** — inline on a `topics:` entry, useful
for one-off cases inside a leaf manifest:

```yaml
topics:
  /sensor/raw:
    type: sensor_msgs/msg/PointCloud2
    external: pub                  # consumed by us, external producer
    sub: [lidar_processor/input]
```

**Semantics:**

- `external: pub` — producer is external. `dangling-entity` won't fire
  on "no publishers" for this FQN. Internal subscribers may still be
  declared and are checked normally.
- `external: sub` — consumer is external. `dangling-entity` won't fire
  on "no subscribers". Internal publishers may still be declared.
- `external: both` — both sides external (passthrough).

**Override and merge:**

- Multiple `external_topics:` entries for the same FQN across scopes
  merge by taking the more permissive side (`Both` ≥ either single).
- An internal `pub:`/`sub:` declaration anywhere in the merged tree
  takes precedence over the external mark on that same side — when a
  team adds the missing producer manifest later, the external mark
  becomes a no-op automatically.
- `type:` in `external_topics:` is cross-checked against any internal
  `topics:` declaration of the same FQN by the `consistency` rule.
  Mismatches are reported as errors.

**When to put it where:**

| Boundary | Put `external_topics:` here |
|----------|----------------------------|
| Repo edge (e.g. truly external to your contract repo) | Top-level manifest of the launch tree |
| Subsystem edge (e.g. perception's sensor inputs) | Subsystem manifest |
| One-off case in a single leaf | Per-topic `external:` flag on the topic |

For a typical autonomous-vehicle launch tree, ~10–50 entries at the
top-level manifest cover the entire repo. Per-leaf duplication is not
required — the loader resolves every scope's contract across the whole
launch tree (via the overlay/provider channels), so a single external
declaration in any ancestor suffices.

## Vocabulary v2

*Phase 44.1. All fields below are additive and optional — existing
contracts (75+ Autoware files, rt_workspace) parse unmodified. See the
play_launch repo's
`docs/superpowers/specs/2026-07-17-contract-vocabulary-v2-design.md`
for the full design rationale; this section is the field reference.*

### Path triggers (`trigger:`)

Every `paths:` entry (node-level or scope-level) may declare an explicit,
closed-taxonomy `trigger:` — what causes the path's output:

```yaml
paths:
  forward:                                  # message-driven
    trigger: { input: [control_cmd_in] }
    output: [control_cmd_out]
    max_latency: 5ms
  status_tick:                              # periodic, self-clocked
    trigger: { timer: { rate_hz: 10 } }
    output: [gate_status]
  publish_map:                              # one-shot at startup
    trigger: once
    output: [map]
  operator_cmd:                             # externally caused, irregular
    trigger: spontaneous
    output: [external_cmd]
```

| Form | Meaning |
|------|---------|
| `{ timer: { rate_hz: <f64> } }` | Periodic self-clocked callback. `rate_hz` must be > 0. |
| `{ input: [ep, ...] }` | Message-driven; caused by these input endpoint/topic names. |
| `once` | Published once (startup latch); scheduling-irrelevant. |
| `spontaneous` | Caused outside the graph (operator, network, hardware); event-like. |

**Legacy derivation.** When `trigger:` is absent: a non-empty `input:`
list derives an input trigger (today's contracts parse identically under
this rule); an empty or missing `input:` with no `trigger:` is
**unclassified** — no trigger fact is derived, never silently assumed to
be a timer. `PathDecl::effective_trigger()` implements this rule and is
the single source of truth for downstream consumers (checker rules,
mapper).

**Compatibility validation** (parse-time): when both an explicit
`trigger: { input: [...] }` and the legacy `input:` list are present,
they must agree as sets — disagreement is a parse error; a
redundant/reordered restatement is fine.

### Fan-in sync (`sync:`)

`sync:` declares the fan-in matching policy for an input-triggered path
with 2 or more endpoints:

```yaml
paths:
  fuse:
    trigger: { input: [cloud_top, cloud_left, cloud_right] }
    sync:
      policy: approximate        # exact | approximate | timeout_any
      max_interval: 50ms        # match window (exact/approximate)
      timeout: 100ms             # timeout_any: publish partial set after this
    output: [cloud_fused]
    max_latency: 30ms
```

| Field | Meaning | Required when |
|-------|---------|----------------|
| `policy` | `exact` (message_filters ExactTime), `approximate` (ApproximateTime), or `timeout_any` (collect-until-timeout, publish partial) | Always |
| `max_interval` | Match window | `policy: exact` or `policy: approximate` |
| `timeout` | Collect-until-timeout duration | `policy: timeout_any` |

**Parse-time validation:** `sync:` is only meaningful on a path whose
[effective trigger](#path-triggers-trigger) is `input` with at least 2
endpoints (explicit `trigger.input` or legacy `input:` — either
satisfies this); otherwise a parse error. `exact`/`approximate` require
`max_interval`; `timeout_any` requires `timeout`.

### Buffer discriminator (`buffer:`)

`buffer:` on a `state: true` subscriber selects the buffering discipline:

```yaml
nodes:
  vehicle_cmd_gate:
    sub:
      control_cmd: { state: true }                 # buffer: latest is the default
      twist:       { state: true, buffer: queue }  # drained batch-wise per tick
```

- `latest` (default) — read-latest; staleness is the failure mode.
- `queue` — bounded queue, drained batch-wise by the consuming timer
  callback; backlog is the failure mode.

**Parse-time validation:** `buffer:` is only meaningful alongside
`state: true` — a parse error otherwise.

### Cross-scope end-to-end budgets (scope `paths:`)

An end-to-end requirement is stated as a top-level `paths:` entry naming
its two ends and a budget. The route between them is **derived** from
the `trigger:`/`output:` facts the nodes already declare, joined through
the topic graph:

```yaml
paths:
  sensing_to_actuation:
    trigger: { input: [/perception/points] }   # where the requirement starts
    output: [/control/cmd]                     # where it ends
    max_latency: 150ms                         # end-to-end budget
```

| Field | Meaning |
|-------|---------|
| `trigger` | What starts the requirement. `{ input: [<topic>] }` for a route beginning at a topic. |
| `output` | The topic(s) the requirement ends at. |
| `max_latency` | End-to-end budget the derived route must fit within. |

#### The derived route, and what it costs

The route is the **critical path** of the subgraph between those ends,
computed by forward dynamic programming over a topological order
(`resolve/src/ros/manifest_graph.rs::critical_path`; the subgraph is
`subgraph_for_scope_path`, restricted to the scope's own subtree via
`subtree_scope_ids`).

The arithmetic, one rule per topology:

- **Series** — latencies **sum** along a branch.
- **Fork-join** — a node with several incoming edges waits for the
  slowest, so the join takes the **max over predecessors**, not the sum.
  `max(50, 30) + 20 = 70`, not 100; there is a test pinning exactly that
  (see [Dataflow Topologies](#dataflow-topologies)).
- **Transport** — each edge carries a weight, the per-sink
  `sub.max_transport ?? topic.max_transport ?? 0`, added on arrival.
- **State edges** — a `state: true` subscription does not propagate
  latency, so it is not on any route.
- **Timer hops** — a `timer`-triggered hop costs `1000 / rate_hz` of
  **sampling cost** *plus* its own `max_latency`: a message arriving just
  after a tick waits a whole period. That is `traversal_latency_ms`; the
  period alone is `sampling_cost_ms`, and it is summed over the winning
  route only, not over the whole subgraph.

**Granularity is per PATH, not per node.** A route through a node is
charged the latency of the path that actually produced the traversed
topic, so a second, unrelated output of the same node no longer inflates
it. Where a hop matches no declared path, the node-wide maximum still
applies — never less conservative than charging by node.

**Rule severities** (see [Static Validation](#static-validation)), all
warnings, all cross-scope:

- **`scope-sampling-feasibility`** — `sampling_cost >= max_latency`. This
  is a different and worse claim than a total over budget: it is the time
  the route spends *waiting for clocks* rather than running, so no
  priority assignment can reduce it. The only fixes are a faster boundary
  rate or a looser budget. It is emitted **before** `scope-budget`, so the
  structural verdict reads first — the budget warning necessarily fires
  too, and on its own it invites an author to go optimise callbacks that
  are not the problem.
- **`scope-budget`** — `critical_path > max_latency`. The message names
  the route and, when there is one, splits the total into
  `event-segment + sampling_cost`, because the second half is the part the
  author cannot reduce. Computing a route here also **supersedes** the
  per-manifest flat-sum `scope-budget` for that scope path: the local
  warning is retracted from the tally, and kept only where no route could
  be traced and the flat sum is the sole estimate available.
- **`jitter-feasibility`** — `sampling_cost > max_jitter`, strictly. A
  clock crossing contributes its *whole* period to end-to-end jitter
  whatever the callback costs, so sampling jitter alone can exceed a
  declared bound. This is the half of the jitter requirement that needs no
  best-case fact; the other half needs `min_latency` and is
  `jitter-range`.

**`chains:`/`segments:` were removed** (phase 68 W4). A written route
was a second copy of the graph, and the `chain-link` rule existed
solely to catch the two disagreeing; a contract still carrying `chains:`
is now a parse error naming this replacement. A `semantics:` line can be
dropped with it — nothing ever branched on `reaction` vs `age`, and a
subscriber's `max_age:` is what states staleness today.

What that looks like — this **does not parse**, and the error names
`paths:`:

<!-- yaml-check: expect-error — `chains:` was removed in phase 68; the error is the point -->

```yaml
chains:
  sensing_to_actuation:
    segments:
      - { scope: sensing, path: capture }
      - { scope: control, path: main }
    max_latency: 150ms
```

Every other retired spelling behaves the same way — a parse error naming
the typed duration or the replacement field, never a silently ignored
key. That is the whole list, and there is no sixteenth:

| Removed | Write instead |
|---------|---------------|
| `chains:`, `segments:` | a scope path: two ends and a budget, route derived (phase 68) |
| `jitter`, `jitter_ms` on an endpoint | `max_jitter` on a path or scope path (phase 68) |
| `correlation` | `sync:`, present or absent (phase 70) |
| `exclude_patterns` | `external:` on the topic, service or action (phase 70) |
| `max_latency_ms`, `max_age_ms`, `max_transport_ms`, `max_response_ms`, `tolerance_ms`, `timeout_ms`, `max_interval_ms`, `lifespan_ms` | the same key with a typed duration: `<n>ns \| us \| ms \| s` (phase 70) |

`semantics:` was deleted with `chains:` rather than migrated: nothing ever
branched on `reaction` versus `age`, so the two produced identical
results, and a subscriber's `max_age:` is what states staleness today.

The generated [format-reference.md](format-reference.md) marks every one
of these **removed** and carries its replacement text; the unit in a NAME
is what let a value be 1000× wrong and still parse.

## Fault detection and reaction

A rate or age requirement says what must be true. It does not say what
happens when it is not, how fast that must be noticed, or how long until
the system is safe. ISO 26262 calls that number the **fault-tolerant time
interval** (FTTI), and splits it into detection (FDTI) and reaction (FRTI).
Detection is something the contract already declares — a liveliness lease,
a QoS deadline, `max_age`, `min_rate_hz` — so the vocabulary here is small:
one requirement on a hazard, one reaction edge on the subscriber that
detects, one fact on the reaction path. FDTI and FRTI are derived.

```yaml
hazards:
  drive_blind:
    severity: ASIL_D                      # from the HARA; consumed, never computed
    guards:
      - /safety/scan                      # any guard faulting is the hazard …
      - all_of: [/loc/ndt, /loc/gnss]     # … except a redundant set, which faults when ALL do
    on: omission                          # omission | late | loss | reported
    ftti: 500ms                           # physics: fault → hazardous event, absent reaction
    reaction: safety.stop                 # the scope path whose route reaches the safe state

nodes:
  brake_controller:
    sub:
      obstacles:
        max_age: 60ms
        on_violation:                     # the WdgM "expired → reaction" edge
          on: [late, omission]
          reaction: emergency_stop        # a path on THIS node
          within: 20ms                    # this hop's share of the reaction time
          mechanism: qos                  # qos (default) | diagnostics | application
    paths:
      emergency_stop:
        trigger: { input: [obstacles] }
        output: [brake_cmd]
        max_latency: 5ms
        safe_state: { emits: brake_cmd, settle: 200ms }   # plant fact; measured
```

`on: reported` names an application detector's *output* topic as the guard
— the fault is whatever that node checks (a covariance monitor, a
plausibility check). The contract never inspects a value; it accounts for
the node that does.

### The arithmetic

`FDTI + FRTI <= ftti`. Everything in that inequality but `ftti` is
derived, in `resolve/src/ros/manifest_loader.rs::check_fault_reaction`.

**FDTI — detection.** A subscriber's detection interval is the **min**
over the mechanisms it declares, because it notices when *any* of them
fires (`detector_interval_ms`), and a guard topic's interval is the
**min** over its subscribers — the fastest detector wins:

| `on:` class | Mechanism read |
|-------------|----------------|
| `omission` | the effective QoS `lease_duration` |
| `late` | the effective QoS `deadline`, and the subscriber's `max_age` |
| `loss` | `drop.max_consecutive × period` on the guard topic |

Only subscribers that **react** count — a subscriber with no
`on_violation` is a bystander, and a guard none of whose subscribers
declares one is `hazard-unguarded` (error): nothing would ever notice.
(A guard naming a topic no manifest in the tree declares is the same
error.)

`on: reported` is the one class computed from the *publisher* side: the
guard topic **is** a detector node's output, so detection is that node's
publish period plus its own path `max_latency`, minimised over the
publishers. The period is the guard topic's rate — **derived first**, the
declared `rate_hz` only as a fallback — so the same number is used here
that [Derived Rates](#derived-rates) computes.

> **A rate floor is not a detector.** `min_rate_hz` is a requirement;
> nothing fires when a period merely passes unless a QoS deadline or an
> application watchdog is declared. Counting the period as a detection
> interval made a 50 Hz floor "detect" a dead lidar in 20ms while the real
> lease was 100ms.

Across a guard **group**: a bare topic is one member; an `all_of:` set
faults only when every member does, so it is detected when the **last**
one is noticed gone — the **max** over members. Across a hazard's several
guard groups, FDTI is again the **max**, because the hazard must cover
its slowest fault. Three levels, three operators: min over a subscriber's
mechanisms, min over a member's reacting subscribers, max over members
and over groups.

**FRTI — reaction.** `walk_reaction` follows the route that actually
runs, which is *not* the critical path of the nominal graph: the guard's
publisher is the thing that failed, so a nominal route would charge a
clock boundary that will never tick again and callbacks that will never
fire. The walk is:

1. At the guard topic, only a subscriber with an `on_violation` moves,
   and it moves along the path that `on_violation.reaction` names.
2. From there onward it follows an `on_violation` where one is declared
   and otherwise the ordinary **input-triggered** paths — a reaction is a
   real message, and downstream nodes forward it the way they forward
   anything.
3. It ends at a path that publishes onto one of the reaction scope path's
   output topics; a `safe_state` whose `emits` lands there contributes its
   `settle`, and a path that merely publishes there ends the walk with no
   settle.
4. Fork-join takes the **longest branch**. The walk is depth-bounded (16)
   rather than cycle-detected: a reaction that re-triggers itself is a
   declaration error worth a wrong number, not a hang.

`FRTI = route + settle`. Both halves can be missing, and each absence is
reported rather than assumed:

| Rule | Severity | When |
|------|----------|------|
| `fault-reaction-budget` | Error / Info | `FDTI + FRTI > ftti` (error, naming every term); otherwise an info stating the slack |
| `reaction-unreachable` | Error | An `on_violation.reaction` naming no path on its own node, or one whose trigger does not include the subscription (the violation would never start it); a hazard `reaction:` naming no scope path in its scope; or no chain of `on_violation` reactions leading from the guards to that path's output — the declared `max_latency` is then used as the route |
| `reaction-unbudgeted` | Warning | No route **and** no declared `max_latency`, or a route that reaches the sink with no `safe_state` settle — the check runs on INCOMPLETE EVIDENCE |
| `reaction-unguarded` | Warning | No subscriber of the reaction's output declares an `on_violation` — a stalled reaction would go unnoticed |
| `reaction-within` | Error | An `on_violation.within` smaller than the reaction path's own `max_latency` |
| `hazard-unguarded` | Error | A guard no reacting subscriber detects |

`on_violation.within` is one hop's declared share of the reaction time; it
does not enter the FRTI sum, which is derived from the route.

Design of record: `docs/design/fault-reaction-primitives.md` (in the
play_launch repository).

### Severity scale

`severity_levels:` declares the scale `hazards.<h>.severity` draws from,
ascending. Absent, it is ISO 26262's
`[QM, ASIL_A, ASIL_B, ASIL_C, ASIL_D]`; a team working to IEC 61508 or
DO-178C names its own. The **first entry is "no safety requirement"** and
derives no criticality. A `severity:` outside the declared scale is
`severity-unknown` — an explicit diagnostic rather than the silent `None`
the parser used to produce.

<!-- yaml-check: skip — a fragment showing one manifest-level key -->

```yaml
severity_levels: [QM, SIL_1, SIL_2, SIL_3, SIL_4]
```

### Criticality is derived

`nodes.<n>.criticality` is a **consequence**, not a hint. A bare
`high | medium | low` label is an ordering with no meaning attached to
it; every safety standard allocates severity *inward from an outcome*,
and since phase 71 the outcomes are declared. The derivation is
`manifest_loader.rs::derive_criticality_from_hazards`.

A hazard reaches a node three ways:

- **feeds** — a publisher of a guard topic is an element whose fault *is*
  the hazard, and the severity propagates **upstream** from it along every
  causal edge, transitively, **state edges included**: a stale map
  produces a hazardous plan as surely as a stale scan does.
- **detects** — a subscriber of a guard topic that declares an
  `on_violation`.
- **reacts** — every node on the reaction walk to the safe state.

A node takes the **max** over the hazards that reach it, ranked by
position in `severity_levels:` — never a sum. `sched_derive` reads the
derivation before any label.

Where a hazard reaches the node, the label is compared against it:
`derivable-criticality` (info — the label is a deletable second copy) when
they agree, `criticality-mismatch` (warning — the derivation wins for
scheduling, and one of the two is wrong) when they do not. Where **no**
hazard reaches the node the label stands: that is the underivable case,
the same absence of information the rate derivation reports as `Unknown`,
and it is why the key stays live.

The label's own grammar is closed — `high`, `medium`, `low`, and nothing
else. It used to accept any string, and its one reader answered an unknown
value with a debug log and `None`, so `criticality: urgent` scheduled a
node exactly as if nothing had been declared.

## Operational modes

A hazard's `reaction:` may name a **mode** instead of a scope path, and
then the fallback ladder *is* the reaction. Phase 71's single-path form is
the one-rung case, byte-identical.

- `functions.<f>` names a **guard group**: what a set of topics together
  provides. The three shapes are a bare topic, a list (any member lost is
  the fault) and `{ all_of: [...] }` (lost only when every member is,
  which needs at least two members).
- `modes.<m>` carries `requires:` (the functions, or bare topics, the mode
  needs — it is available while every one holds), `fallback:` (the ordered
  ladder to fall down when it is lost), `reaction:` (the scope path
  reaching this mode's safe state), `overrides:` and `description:`.

```yaml
version: 1
functions:
  pose_estimation: { all_of: [/loc/ndt, /loc/gnss] }
  trajectory: [/planning/trajectory]
  scan: /sensing/scan
modes:
  autonomous:
    description: full autonomy
    requires: [pose_estimation, trajectory]
    fallback: [comfortable_stop, emergency_stop]
  comfortable_stop:
    requires: [pose_estimation]
    reaction: system.comfortable_stop
    overrides:
      paths:
        lidar_to_brake: { max_latency: 100ms }
      nodes:
        detector:
          sub: { scan: { min_rate_hz: 10 } }
  emergency_stop:
    requires: []
    reaction: system.emergency_stop
hazards:
  lost_pose:
    guards: [pose_estimation]
    ftti: 2s
    reaction: autonomous
```

**Each rung is judged in its own right.** A graded reaction is a promise,
not merely a step toward the floor, so `ladder-rung-budget` (error) checks
every rung but the last with the same arithmetic the terminal one gets:
`FDTI + rung route + settle <= ftti`. The last rung is what
`fault-reaction-budget` measures, because it is the floor the system is
guaranteed to reach. Autoware's real four-mode ladder proves
`comfortable_stop` cannot cover a 2 s interval (500 + 4000 = 4500 ms) that
the emergency floor beneath it can.

**A ladder must terminate.** `ladder-unterminated` (error) fires two ways:
a mode named as a reaction with no `fallback:` at all, and — the one worth
stating — a **last rung that requires something the hazard's own guards
can take away**. That is not a floor: losing the guard takes the whole
ladder with it. A rung naming a mode that does not exist, or a rung with
no `reaction:` path of its own, is `reaction-unreachable`.

**A mode that cannot fall is not a mode.** `mode-requires-unguarded`
(error) fires when a mode requires a function no subscriber of which
declares an `on_violation`: nothing would notice it was lost, so the mode
can never be declared unavailable and the ladder below it can never be
taken.

### Per-mode requirement values (`overrides:`)

`overrides:` is a nested mapping mirroring the contract's own shape,
flattened at parse time to dotted `(target, value)` pairs —
`paths.lidar_to_brake.max_latency`,
`nodes.detector.sub.scan.min_rate_hz`. This is how a requirement takes a
different value in a degraded mode **without any scalar becoming a map**,
so no reader changes: every requirement keeps one value where it is
declared, and a mode pins another by naming its contract path.

An override naming no requirement that exists is `override-target-missing`
(error) — with nothing to pin over, it would be silently ignored. Such an
entry is also **not applied**, or one override would get two answers:
rejected by the rule and honoured by the arithmetic.

The checker then **runs per mode**. For each mode whose `overrides:`
change something, it clones the merged index, applies them to both the
declaration and the resolved copies the checks read, re-runs the
requirement checks (critical path, sync budget, rate hierarchy,
lifespan/age, fault reaction) and **diffs against the default**. Only what
the mode introduces is reported, under `mode:<rule>` — a
`mode:scope-budget`, say, with the message prefixed `in mode '<m>':`.
A degraded mode's `max_latency: 200ms` is a different contract, and the
arithmetic that clears the default one says nothing about it.

> Target paths are read **section-from-front, field-from-back**, because a
> scope-path name may itself contain dots (`safety.stop`). Splitting
> positionally fired `override-target-missing` on a correct contract.

## Static Validation

The table below is the rule set registered in this crate's
`default_rules()` (`check/src/rules/mod.rs`), in registration order —
**19 rules**, one row each. The cross-scope rules that need the merged
`ManifestIndex` live in the consumer's merge layer and follow in a
second table.

| # | Rule                | What it catches                                                | Severity      |
|---|---------------------|----------------------------------------------------------------|---------------|
| 1 | `endpoint-unique`   | Duplicate endpoint names within a node                         | Error         |
| 2 | `wiring`            | Path endpoints not connected by any topic                      | Warning       |
| 3 | `qos-compat`        | Invalid QoS values                                             | Error         |
| 4 | `qos-match`         | Publisher and subscriber QoS incompatible per DDS offered ≥ requested, on `reliability`, `durability`, `liveliness` and `lease_duration` | Error |
| 5 | `rate-hierarchy`    | Lower bounds `pub.min_rate_hz >= rate_hz >= sub.min_rate_hz`, and (phase 70) upper bounds `pub.max_rate_hz >= rate_hz <= sub.max_rate_hz` | Error |
| 6 | `scope-budget`      | Conservative flat sum: scope budget < Σ node latencies + declared transport (the consumer runs the topology-aware version) | Warning |
| 7 | `causal-dag`        | Cycles in the dataflow graph (`state:` breaks cycles)          | Error         |
| 8 | `drop-sanity`       | Effective delivery rate < sub.min_rate_hz; drop values out of range (`n > w`, `max_consecutive: 0`) — on topics, scope paths and node paths alike | Error |
| 9 | `service-wiring`    | Service client with no matching server across tree             | Warning       |
| 10 | `service-type`     | Service with no type; server/client not on node                | Error/Warning |
| 11 | `dangling-entity`  | Topic with 0 publishers or 0 subscribers (warning); service/action with 0 servers (error) — **unless** the missing side is declared `external:`, which for a service is the normal case, a client and its server often being two images | Error/Warning |
| 12 | `satisfiability`   | Arg combination produces dangling entities; unreachable nodes. Built without the `smt` feature it becomes a stub emitting one Info saying the analysis was not run | Error/Warning |
| 13 | `state-consistency` | Node has ≥2 sibling subs tagged `state: true` and *exactly one* other sub is neither tagged `state:` nor referenced as a path `input` — likely a missed `state:` tag | Warning |
| 14 | `explicit-trigger` (44.1/44.2) | Path has no explicit `trigger:` — authoring-hygiene nudge toward the four-way taxonomy, fires regardless of legacy `input:` derivation | Info |
| 15 | `inherited-rate` (44.1/44.2) | A path has a non-`Input` explicit `trigger:` (`timer`/`once`/`spontaneous`) alongside a stale, now-ignored legacy `input:` list | Warning |
| 16 | `once-durability` (44.1/44.2) | A `once`-triggered path's output topic is not `durability: transient_local` — late joiners lose the startup-latch message | Warning |
| 17 | `sync-feasibility` (44.1/44.2) | `sync:` `max_interval`/`timeout` too narrow for the slowest declared input's inter-arrival period | Warning |
| 18 | `queue-drain-rate` (44.1/44.2) | Sum of `buffer: queue` producer `rate_hz` exceeds the consuming `timer` path's rate — backlog accumulates every period | Warning |
| 19 | `jitter-range` (70 W2) | `min_latency` above `max_latency`; both bounds declared and `max_latency - min_latency > max_jitter` (errors); `max_jitter` with no `min_latency`, so the bound is unverifiable (info — an absent floor is unknown, not zero) | Error/Info |

The cross-scope rules run in the consumer's merge layer
(`ros-launch-resolve`, invoked by `play_launch check`), which has the
merged `ManifestIndex` needed to resolve names, budgets and routes
across manifest files. The ones that bear on this document:

| Rule | What it catches | Severity |
|------|-----------------|----------|
| `manifest-parse` | A contract file that could not be read at all. Counted and reported separately from the per-manifest tallies, because the file it names is absent from the index and would otherwise count as clean | Error |
| `consistency` | Same resolved topic/service has conflicting `type:`, `rate_hz:`, `max_transport:`, topic-level `qos:` or `drop:` across scopes; or an `external_topics:` `type:` contradicting the internal `topics:` one | Error |
| `budget-overflow` | Descendant path budget exceeds a matched ancestor path budget (part > whole) | Error |
| `scope-budget` | A scope path's DERIVED route total (critical path, `max` over fork-join branches) exceeds its declared `max_latency`; supersedes the per-manifest flat sum where a route was traced | Warning |
| `scope-sampling-feasibility` | A scope path's sampling cost (clock boundaries alone) already meets or exceeds its budget — structurally infeasible, no scheduling assignment can fix it. Emitted before `scope-budget` | Warning |
| `jitter-feasibility` | A scope path's declared `max_jitter` is below the sampling jitter its route already carries — one whole period per clock boundary crossed | Warning |
| `sync-budget` | A `sync:` window wider than the path's own `max_latency` — the synchroniser may wait that long before the callback starts | Warning |
| `causal-dag-global` | Cycles in the merged graph, including edges derived from launch-file remaps | Error |
| `lifespan-age` | A topic's `lifespan` is shorter than a subscriber's `max_age` — the age requirement cannot be met | Warning |
| `response-blocking`, `concurrency-decl`, `path-exclusion` | A service `max_response` promised beside a callback that would block it, and `concurrency:` declarations that contradict each other across the merged tree | Warning |
| `derivable-rate`, `rate-mismatch` | A declared `topics.<t>.rate_hz` agrees with (info) or contradicts (warning) the rate derived from the timers that drive it | Info/Warning |
| `derivable-min-rate`, `min-rate-mismatch`, `derived-rate-hierarchy` | The same comparison for a publisher's `min_rate_hz`, and a subscriber asking for more than the derived rate delivers | Info/Warning |
| `derivable-criticality`, `criticality-mismatch`, `severity-unknown` | A declared `criticality` agrees with (info) or contradicts (warning) the one derived from the hazards reaching the node; a `severity:` outside the declared `severity_levels:` scale | Info/Warning/Error |
| `fault-reaction-budget`, `reaction-unreachable`, `reaction-within`, `reaction-unbudgeted`, `reaction-unguarded`, `hazard-unguarded` | FDTI + FRTI against a hazard's `ftti`, and the structural preconditions for computing them | Error/Warning/Info |
| `ladder-rung-budget`, `ladder-unterminated`, `mode-requires-unguarded`, `override-target-missing` | Operational modes: each fallback rung against the ftti in its own right, a floor that requires nothing losable, and override targets that name a real contract path | Error |
| `rate-hierarchy`, `qos-match`, `dangling-entity` | Cross-scope variants of the local rules, re-run after merge | Error/Warning |

A mode with `overrides:` re-runs the requirement checks on the modified
contract and reports only what that mode introduces, under a `mode:`
prefix — `mode:scope-budget`, `mode:fault-reaction-budget`, and so on. See
[Per-mode requirement values](#per-mode-requirement-values-overrides).

`consistency` has only one implementation. A no-op body of the same id
used to sit in `default_rules()`, reserving the name and being counted in
the registry, so a reader of the registry — or of a `--rule consistency`
run — was told a rule ran that did nothing; it was removed in v0.1.38 and
the id now belongs entirely to the consumer's merge layer.
`scope-budget` genuinely does exist on both sides: in-crate a
conservative flat sum, in the consumer a topology-aware critical path
that **supersedes** it — where a route is computed, the local warning for
that scope path is retracted from the tally.

**Drop checking** is split between static and runtime:
- **Static (`drop-sanity`)**: validates values are in range and that
  effective delivery rate meets subscriber demand
  (`rate_hz × (1 - n/w) >= sub.min_rate_hz`). No composition along a
  route — drop rates depend on runtime conditions.
- **Runtime monitoring**: checks the observed delivery ratio against
  `drop.max_count`. Burstiness detection (autocorrelation, dispersion
  index) and `max_consecutive` checking are designed extensions — see
  [Burstiness](contract-theory.md#burstiness) for the metrics and
  implementation status.

**Satisfiability checking**: when args have `type: bool` or `choices:`,
the checker uses Z3 to verify no valid arg combination produces a
structurally broken manifest. A passing manifest is **variant-complete**.

**Consistency**: when checking a manifest tree, the checker merges all
declarations for the same resolved topic or service name. `type:` must
match across all scopes, as must `rate_hz:`, `max_transport:`, the
topic-level `qos:` block and `drop:` wherever two scopes both declare
them. Endpoint lists (`pub:`/`sub:`, `server:`/`client:`) are merged.
Endpoint-level `qos:` overrides live on a node and are local to the
declaring scope — they do not need to agree across scopes.

**Dangling entities**: after condition filtering and cross-scope merge,
topics with 0 publishers across the entire manifest tree (warning —
may be published by an external system), services/actions with 0 servers
(error), and empty entities (silently removed) are flagged.

**Example diagnostics**, in the wording the rules actually emit:

```
error[endpoint-unique]: duplicate endpoint name 'cmd' across pub/sub/srv/cli
  --> control.contract.yaml:5:9                      nodes.controller

warning[wiring]: path input 'trajectory' is not wired by any topic
  (expected 'controller/trajectory' in a topic's sub list)
  --> control.contract.yaml:12:9         nodes.controller.paths.main

error[rate-hierarchy]: publisher 'controller/cmd' min_rate_hz (10) < topic
  rate_hz (30)
  --> control.contract.yaml:20:5         topics.command/control_cmd

error[drop-sanity]: effective delivery rate (9.50 Hz = 10 Hz * 0.9500
  delivery) < subscriber 'tracker/input' min_rate_hz (10)
  --> perception.contract.yaml:15:5      topics./perception/pointcloud

error[qos-match]: incompatible QoS on topic '/sensor/pointcloud' field
  'reliability': pub 'lidar_driver/output' offers 'best_effort', sub
  'perception/input' requests 'reliable'

error[qos-match]: incompatible QoS on topic '/safety/scan' field
  'lease_duration': pub 'lidar/scan' asserts every 200.00ms, sub
  'brake/obstacles' declares it dead after 100.00ms

warning[dangling-entity]: topic '/sensor/imu' has no publishers (no data source)

error[satisfiability]: topic 'ndt_pose' has 0 publishers when pose_source=gnss
```

And from the consumer's merge layer, where the route is known:

```
warning[scope-sampling-feasibility]: scope path 'perception' (scope 3) is
  structurally infeasible: sampling cost alone (100.00ms, one period per
  clock boundary crossed: ekf → tracker) meets or exceeds the declared
  max_latency (85ms). No priority assignment can reduce this — raise the
  boundary's rate or the budget

warning[scope-budget]: scope path 'perception' (scope 3) max_latency_ms (85)
  is less than critical path: cropbox → ground_filter → detector = 125.00ms
  (25.00ms event-segment + 100.00ms sampling_cost)

error[budget-overflow]: scope path 'detect' (scope 7) max_latency_ms (60)
  exceeds ancestor path 'perception' (scope 3) max_latency_ms (50) — child
  budget cannot exceed parent budget on the same (input, output) topics

error[consistency]: topic '/localization/kinematic_state' type mismatch:
  'nav_msgs/msg/Odometry' (existing) vs 'geometry_msgs/msg/PoseStamped' in
  scope 4 (tier4_control_launch/control.launch.xml)

info[fault-reaction-budget]: hazard 'drive_blind': detection 100.00ms
  (/safety/scan -> brake/obstacles detects within 100.00ms) + reaction
  205.00ms (reaction route brake/emergency_stop = 5.00ms + settle 200.00ms)
  = 305.00ms fits the fault-tolerant time interval 500.00ms with 195.00ms
  of slack
```

`fault-reaction-budget` is an **error** when the sum exceeds the interval
and an **info** when it fits; the fitting case is shown because it is the
one that states the whole derivation in a single line.

## References

- **AUTOSAR Timing Extensions** (R22-11): event chains, age/reaction constraints
- **CARET**: cause-effect chain latency measurement for ROS 2
- **ROS 2 message_filters**: ApproximateTimeSynchronizer algorithm
- Contract theory foundations: [contract-theory.md](contract-theory.md)

## Non-Goals

- Discovery beyond the provider sidecar / overlay channels (e.g. scanning
  `AMENT_PREFIX_PATH` for contracts outside those two layouts)
- Blocking enforcement via RCL interception (future)
- Semantic component extraction — manifests are user-authored
