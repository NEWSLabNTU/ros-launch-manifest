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
| *(in source code)* | `pub:` / `sub:` | Scope boundary interface |

The key difference: in launch files, topics are implicit in source code
and connected via `<remap>`. In manifests, topics are **first-class** —
declared with message type, QoS, and rate, and explicitly wired to node
endpoints.

### Directory Structure

Launch files live in each package's install tree. Manifest files are
organized in a flat manifest directory, keyed by `package/stem.yaml`:

```
ROS install tree                        Manifest directory
─────────────────                       ──────────────────
share/                                  manifests/
├── tier4_control_launch/               ├── tier4_control_launch/
│   └── launch/                         │   └── control.yaml
│       └── control.launch.xml          │
├── tier4_system_launch/                ├── tier4_system_launch/
│   └── launch/                         │   └── system.yaml
│       └── system.launch.xml           │
├── autoware_mrm_handler/               ├── autoware_mrm_handler/
│   └── launch/                         │   └── mrm_handler.yaml
│       └── mrm_handler.launch.xml      │
├── tier4_planning_launch/              ├── tier4_planning_launch/
│   └── launch/                         │   ├── planning.yaml
│       ├── planning.launch.xml         │   ├── behavior_planning.yaml
│       └── .../behavior_planning/      │   └── motion_planning.yaml
│           └── behavior_planning.      │
│               launch.xml              │
└── autoware_launch/                    └── autoware_launch/
    └── launch/                             └── planning_simulator.yaml
        └── planning_simulator.
            launch.xml
```

The manifest path is derived from the launch file: strip the `launch/`
subdirectory and `.launch.xml` / `.launch.py` extension, replace with
`.yaml`. The manifest loader resolves `<manifest_dir>/<pkg>/<stem>.yaml`
for each scope in the launch tree.

## The Manifest Model

A manifest describes a **scope** — one launch file's contribution to the
graph. Scopes contain nodes, topics, services, and child scopes.

![Manifest model: scopes, nodes, and wiring](img/manifest-model.png)

### Concepts

- **Scope.** A manifest file describes one scope. A scope corresponds to
  one launch file (or one `<group>` block). Scopes form a tree that
  mirrors the launch file include hierarchy.

- **Node.** A leaf execution entity — a ROS 2 node or composable node.
  Declares named **endpoints**: pub, sub, srv, cli. Optionally declares
  causal **paths** with timing constraints. Composable nodes appear as
  regular nodes — the container is a deployment detail. A composable node
  belongs to the manifest of the launch file that contains the
  `<load_composable_node>` tag, not the container's launch file.

- **Endpoint.** A named port on a node. Four kinds: `pub` (publishes),
  `sub` (subscribes), `srv` (serves a service), `cli` (calls a service).
  Endpoints can have properties: rate, jitter, state, required.

  Endpoint names are **local identifiers within the manifest**, not ROS
  topic names. A node declares `pub: [cmd]` — this is not the ROS topic
  `/control/cmd`. The `topics:` section is where endpoints get wired to
  actual topics: `pub: [controller/cmd]` means "controller's cmd endpoint
  publishes on this topic." The launch file's `<remap>` determines the
  real ROS name; the manifest uses logical names for wiring.

- **Topic.** First-class wiring between endpoints. Declares message type,
  which endpoints publish, which subscribe, QoS, and channel rate.
  In launch files, this information is split across `<remap>` tags,
  source code, and convention. The manifest makes it explicit.

- **Service / Action.** Request-response wiring. Declares service type,
  server endpoints, client endpoints. Actions have server and client sides.

- **Scope Interface.** The scope's boundary — top-level `pub:`, `sub:`,
  `srv:`, `cli:` groups that declare which internal endpoints are visible
  to the parent scope. The parent wires children together by referencing
  `child_name/group_name` in its topics and services.

  Note: `pub:` / `sub:` appear at three levels — don't confuse them:
  - On a **node** — declares endpoint names (`pub: [cmd]`)
  - At **top level** — declares the scope boundary (`pub: control_output: [controller/cmd]`)
  - Inside a **topic** — lists which endpoints are wired (`pub: [controller/cmd]`)

- **Include.** A child scope. Maps to `<include>` in launch files. The
  include name is the ROS namespace (from `<push-ros-namespace>`). Each
  include references a child manifest file.

- **Args and Conditions.** Manifests can declare `args:` (named parameters
  resolved from the launch tree) and `if:` / `unless:` conditions on any
  entity. These mirror `<arg>` and `if="$(var ...)"` in launch XML.

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

- **Paths.** Named causal relations (input → output) with timing
  constraints: max latency, max age, drop tolerance. Declared on nodes
  (node-level paths) and scopes (scope-level paths). No launch file
  equivalent — this is the contract layer that manifests add.

## Key Concepts in Detail

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

### Latency vs Age

Both describe "how old is the data?" but measure different things.

**`max_latency_ms`** — the time from when the *triggering input arrives
at this node/scope* to when the output is published.

```
sensor_points ──→ [cropbox: 5ms] ──→ [ground_filter: 15ms] ──→ [detector: 30ms]
                  ├── 5ms ──┤        ├──── 15ms ────┤          ├── 30ms ──┤
                  └────────────── scope max_latency_ms: 50ms ─────────────┘
```

**Measurement points:**

- **Node `max_latency_ms`**: from `rcl_take` of the trigger input to
  `rcl_publish` of the output. This is pure processing time — transport
  to the next node is NOT included.
- **Scope `max_latency_ms`**: from the first `rcl_take` inside the
  scope to the last `rcl_publish` out of the scope. This INCLUDES
  internal transport between nodes within the scope.

The scope budget is always ≥ the sum of node budgets because the scope
includes internal transport that individual nodes don't. Transport
latency is not declared separately — it is absorbed into the scope
budget as headroom.

**`max_age_ms`** — the time from when the data was *originally produced*
(the `header.stamp` at the sensor source) to when the scope's output is
published. Age includes all upstream latency, transport delays, and
processing before the data reached this scope.

```
age = now - header.stamp (at the point of output publish)
```

Age is always >= latency, because age includes time before the data
entered the current scope. Age computation relies on causal paths
preserving `header.stamp` — see [Timestamps and Data Flow](#timestamps-and-data-flow).

**When to use each:**

- `max_latency_ms` — declare on paths when you have a timing budget.
  This is the node/scope's own processing budget.
- `max_age_ms` — declare on scope paths when data freshness matters to
  downstream consumers. A planning module may need sensor data no older
  than 200ms, regardless of how many nodes processed it upstream.

**If omitted:** `max_latency_ms` omitted means the node/scope has no
timing budget. The checker treats it as **transparent** — parent scopes
look through it when computing budget sums. `max_age_ms` omitted means
no freshness constraint.

**Partial decomposition:** You don't need to declare `max_latency_ms` on
every node. A scope with a budget is valid even if some (or all) of its
children have no budgets. The checker reports the **residual** — the
unallocated portion of the scope budget — so you can see how much
headroom remains. This supports a top-down workflow: start with the E2E
requirement, fill in per-node budgets as you measure them.

See [Verification Rules](contract-theory.md#verification-rules) for the
full composition and checking rules.

### Drop Budgets

Messages can be lost in transport between nodes — DDS queue overflow,
network congestion, or QoS mismatch. The manifest lets you declare how
much loss is acceptable.

Drops are declared on **topics** (transport drops) and **scope paths**
(E2E drops). Node paths do not have drop fields — if a node internally
skips messages, the effect shows as a lower `pub.min_rate_hz` on its
output.

**`max_drop_rate: 0.05`** — up to 5% of messages may be lost. This is
a long-run average, declared as a fraction (0-1).

**`max_consecutive: 3`** — never lose more than 3 in a row. Consecutive
drops cause visible glitches (e.g., a planner that misses 3 consecutive
obstacle updates).

```yaml
topics:
  pointcloud:
    rate_hz: 10
    max_drop_rate: 0.05        # 5% transport loss
    max_consecutive: 3         # never 3+ in a row

paths:
  perception:
    input: raw_data
    output: [detections]
    max_latency_ms: 85
    max_drop_rate: 0.08        # 8% E2E (all transport hops combined)
    max_consecutive: 5
```

**Rate-drop interaction:** a topic's effective delivery rate accounts
for drops. If `rate_hz: 10` and `max_drop_rate: 0.05`, the subscriber
effectively receives at least `10 * (1 - 0.05) = 9.5 Hz`. The checker
verifies: `rate_hz * (1 - max_drop_rate) >= sub.min_rate_hz`.

**Composition:** drop rates multiply through a chain. If topic T1 has
`max_drop_rate: 0.02` and T2 has `max_drop_rate: 0.03`, the chain drop
rate is `1 - (0.98 * 0.97) ≈ 0.049`. The scope's `max_drop_rate` must
be ≥ the composed rate.

**If omitted:** no drop checking. The `drop-rate` and `drop-consecutive`
rules only fire when drop values are declared.

### QoS Defaults

When `qos:` is omitted on a topic, the checker does not validate QoS.
When declared, the checker validates that values are from the allowed set:

| Field | Allowed values | ROS 2 default (when omitted) |
|-------|---------------|------------------------------|
| `reliability` | `reliable`, `best_effort` | `reliable` |
| `durability` | `volatile`, `transient_local` | `volatile` |
| `depth` | integer | `10` |
| `history` | `keep_last`, `keep_all` | `keep_last` |
| `lifespan_ms` | integer | unlimited |
| `liveliness` | `automatic`, `manual_by_topic` | `automatic` |

Declaring QoS is recommended for topics where publisher and subscriber
must agree (e.g., `transient_local` for map data, `best_effort` for
high-rate sensor streams). The `qos-compat` rule errors on invalid values
like `reliability: maybe`.

### Timestamps and Data Flow

Timestamps (`header.stamp`) are the thread that connects latency, age,
correlation, and state. The manifest imposes rules on how timestamps
flow through the graph.

**Causal paths preserve timestamps.** When a node has a causal path
`input → output`, the output message's `header.stamp` must equal the
input's `header.stamp`. This is how data provenance is tracked through
a pipeline — every node passes the original sensor timestamp forward:

```
sensor (stamp=T) → cropbox (stamp=T) → detector (stamp=T) → planner
                                                                │
                                         age = now - T ─────────┘
```

This is what makes `max_age_ms` meaningful. At any point in the chain,
age is `now - header.stamp`, and the stamp traces back to the original
sensor reading.

**Periodic nodes reset the timestamp chain.** A timer-driven node
(empty `input: []` path) generates its own timestamps — the output
`stamp` is the current time, not propagated from an input. For example,
a tracker runs on a 10 Hz timer, polls the latest fused objects
(`state: true`), and publishes tracked objects with `stamp = now`.
This is why `max_age_ms` is typically declared on scope paths rather
than node paths — a periodic node breaks the provenance chain.

**Multi-input correlation matches timestamps.** When a node fuses
multiple causal inputs (e.g., lidar + camera), `correlation` specifies
how input timestamps are paired:

- `correlation: timestamp` — inputs must have matching `header.stamp`
  within `tolerance_ms`. If lidar and camera stamps differ by more than
  the tolerance, the pair is discarded. The output `stamp` is the oldest
  input stamp (preserving the earliest provenance).

- `correlation: latest` — use the most recent message from each input,
  regardless of stamp difference. No timestamp matching is performed.

```yaml
paths:
  fusion:
    input: [lidar_objects, camera_objects]
    output: [fused]
    correlation: timestamp
    tolerance_ms: 50           # lidar and camera stamps must be within 50ms
    max_latency_ms: 20
```

**State subscribers don't contribute timestamps.** A `state: true`
subscriber reads the latest value regardless of its timestamp. The
state data's `stamp` is *not* propagated to the output — only causal
inputs contribute. EKF reads map data (`state: true`, stamp from minutes
ago) and sensor data (causal, `stamp=T`). The output pose has `stamp=T`,
not the map's ancient timestamp.

## Worked Example

A perception pipeline with tracking and prediction stages.

**Launch files:**

```xml
<!-- perception.launch.xml -->
<push-ros-namespace namespace="perception"/>
<include file="tracking/tracking.launch.xml"/>
<include file="prediction/prediction.launch.xml"/>

<!-- tracking/tracking.launch.xml -->
<node pkg="autoware_multi_object_tracker" exec="tracker"/>

<!-- prediction/prediction.launch.xml -->
<node pkg="autoware_map_based_prediction" exec="predictor"/>
```

**Manifest files:**

```yaml
# tier4_perception_launch/perception.yaml
version: 1

includes:
  tracking:
    manifest: tier4_perception_launch/tracking.yaml
  prediction:
    manifest: tier4_perception_launch/prediction.yaml

topics:
  tracked_objects:
    type: autoware_perception_msgs/msg/TrackedObjects
    pub: [tracking/objects]
    sub: [prediction/objects]
    rate_hz: 10
    max_drop_rate: 0.02

sub:
  pointcloud: [tracking/detected_objects]
  vector_map: [prediction/vector_map]

pub:
  objects: [prediction/objects]

paths:
  main:
    input: pointcloud
    output: [objects]
    max_latency_ms: 50
    max_age_ms: 150
    max_drop_rate: 0.05
```

```yaml
# tier4_perception_launch/tracking.yaml
version: 1

nodes:
  multi_object_tracker:
    sub:
      detected: { min_rate_hz: 10 }
    pub:
      tracked: { min_rate_hz: 10 }
    paths:
      main: { input: detected, output: [tracked], max_latency_ms: 20 }

topics:
  tracked_objects:
    type: autoware_perception_msgs/msg/TrackedObjects
    pub: [multi_object_tracker/tracked]
    rate_hz: 10

sub:
  detected_objects: [multi_object_tracker/detected]

pub:
  objects: [multi_object_tracker/tracked]
```

```yaml
# tier4_perception_launch/prediction.yaml
version: 1

nodes:
  map_based_prediction:
    sub:
      tracked: { min_rate_hz: 10 }
      vector_map: { state: true, required: true }
    pub:
      predicted: { min_rate_hz: 10 }
    paths:
      main: { input: tracked, output: [predicted], max_latency_ms: 15 }

sub:
  objects: [map_based_prediction/tracked]
  vector_map: [map_based_prediction/vector_map]

pub:
  objects: [map_based_prediction/predicted]
```

The parent (`perception.yaml`) wires children: `tracking/objects` →
`prediction/objects` via the `tracked_objects` topic. Each child
declares its scope interface (`sub:` / `pub:`) so the parent knows
what ports to connect.

### Mapping from ROS Topics to Manifest Declarations

When writing a manifest for an existing system, you start with runtime
topic names (from `ros2 topic list`) and need to map them into manifest
declarations. Here is one concrete mapping using the perception example
above:

**Runtime topic:** `/perception/tracking/tracked_objects`

| Layer               | File              | Key                              | Value                            |
|---------------------|-------------------|----------------------------------|----------------------------------|
| Publisher endpoint  | `tracking.yaml`   | `nodes.multi_object_tracker.pub` | `tracked`                        |
| Internal topic      | `tracking.yaml`   | `topics.tracked_objects.pub`     | `[multi_object_tracker/tracked]` |
| Child export        | `tracking.yaml`   | `pub.objects`                    | `[multi_object_tracker/tracked]` |
| Parent wiring       | `perception.yaml` | `topics.tracked_objects.pub`     | `[tracking/objects]`             |
| Subscriber endpoint | `prediction.yaml` | `nodes.map_based_prediction.sub` | `tracked`                        |
| Child import        | `prediction.yaml` | `sub.objects`                    | `[map_based_prediction/tracked]` |
| Parent wiring       | `perception.yaml` | `topics.tracked_objects.sub`     | `[prediction/objects]`           |

The ROS topic name `/perception/tracking/tracked_objects` comes from
namespace resolution in the launch file (`<push-ros-namespace>` +
`<remap>`). The manifest uses **logical names** — the topic key
`tracked_objects` is a manifest-local identifier, not the ROS name.
Endpoint names like `tracked` are local to the node declaration. The
`child/export` references (e.g., `tracking/objects`) combine the include
name with the child's scope interface export name.

### Example: Args, Conditions, and State

A control scope with one always-present controller and an optional
validator gated by a boolean launch arg:

```yaml
# tier4_control_launch/control.yaml
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
        max_latency_ms: 10

  validator:
    if: $(var launch_validator)  # only present when arg is "true"
    sub:
      control_cmd: {}
      predicted_trajectory: {}

topics:
  control_cmd:
    type: autoware_control_msgs/msg/Control
    pub: [controller/control_cmd]
    sub: [validator/control_cmd]  # auto-optional: validator is conditional
    rate_hz: 30

sub:
  trajectory_input: [controller/trajectory]

pub:
  control_output: [controller/control_cmd]

paths:
  control:
    input: trajectory_input
    output: [control_output]
    max_latency_ms: 15
```

Key features demonstrated:
- **`args:` with `type: bool`** — enables Z3 satisfiability checking
  across all valid configurations
- **`if:`** — validator only exists when `launch_validator` is `"true"`;
  its topic refs are automatically dropped when it's filtered out
- **`state: true`** — operation_mode is polled, doesn't create a causal
  dependency in the dataflow graph
- **`required: true`** — controller needs at least one operation_mode
  message before it can operate
- **Scope path** — E2E latency budget for the control scope

## Format Reference

Use this section as a lookup reference. Each subsection shows the YAML
syntax, field table with defaults, and when to use.

### Metadata

| Field              | Required | Description | If omitted |
|--------------------|----------|-------------|------------|
| `version`          | yes      | Format version (currently `1`) | Error |
| `exclude_patterns` | no       | Topic prefixes to ignore | `/rosout`, `/parameter_events` |

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

### Nodes

Declare a node for each ROS 2 node or composable node in the launch file.

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
        max_response_ms: 100
    cli:
      operate: {}
    paths:
      main:
        input: trajectory
        output: [cmd]
        max_latency_ms: 10
```

Endpoints can be a list (`pub: [a, b]`) or a map with properties.

**Subscriber properties:**

| Field         | Meaning                                       | If omitted |
|---------------|-----------------------------------------------|------------|
| `min_rate_hz` | Minimum expected receive rate                 | Not checked |
| `max_rate_hz` | Maximum expected receive rate                 | Not checked |
| `state`       | Polled (read-latest), not causal              | `false` — causal |
| `required`    | Must receive at least once before operational | `false` — optional |

**Publisher properties:**

| Field         | Meaning                                   | If omitted |
|---------------|-------------------------------------------|------------|
| `min_rate_hz` | Minimum publish rate                      | Not checked |
| `max_rate_hz` | Maximum publish rate                      | Not checked |
| `jitter_ms`   | Max deviation from ideal period           | Not checked |

**Service/client properties:**

| Field             | Meaning                        | If omitted |
|-------------------|--------------------------------|------------|
| `max_response_ms` | Max request-to-response time   | Not checked |

### Topics

Declare a topic when two nodes communicate. Even if the launch file
doesn't have an explicit `<remap>`, the topic exists in the runtime graph.

```yaml
topics:
  control_cmd:
    type: autoware_control_msgs/msg/Control
    pub: [controller/cmd]
    sub: [validator/input]
    rate_hz: 30
    max_drop_rate: 0.01
    max_consecutive: 3
    qos:
      reliability: reliable
      durability: transient_local
      depth: 1
```

| Field            | Required | Description | If omitted |
|------------------|----------|-------------|------------|
| `type`           | yes      | ROS message type (`pkg/msg/Name`) | Error |
| `pub`            | no       | Publisher endpoint refs (`node/endpoint`) | Empty list |
| `sub`            | no       | Subscriber endpoint refs | Empty list |
| `rate_hz`        | no       | Negotiated channel rate | Rate hierarchy not checked |
| `max_drop_rate`  | no       | Transport drop rate (fraction 0-1) | Drop not checked |
| `max_consecutive`| no       | Max consecutive transport drops | Consecutive not checked |
| `qos`            | no       | QoS profile | QoS not validated |
| `if`/`unless`    | no       | Condition | Always included |

**Rate hierarchy with drops:**

```
pub.min_rate_hz  >=  rate_hz  >=  rate_hz * (1 - max_drop_rate)  >=  sub.min_rate_hz
     30                30          30 * (1 - 0.01) = 29.7               29
```

The publisher must produce at least as fast as the channel rate. The
effective delivery rate (after transport drops) must meet every
subscriber's minimum demand. Think of it as: supply ≥ channel ≥
effective delivery ≥ demand.

### Services and Actions

```yaml
services:
  operate:
    type: tier4_system_msgs/srv/OperateMrm
    server: [operator/operate]
    client: [handler/operate]

actions:
  navigate:
    type: nav2_msgs/action/NavigateToPose
    server: [navigator/navigate]
    client: [planner/navigate]
```

### Scope Interface

Top-level `pub:`, `sub:`, `srv:`, `cli:` declare the scope's boundary.
Parent scopes reference these as `child_name/group_name`.

```yaml
pub:
  control_output: [controller/cmd]
sub:
  trajectory_input: [controller/trajectory]
srv:
  operate: [operator/operate]
cli:
  operate_mrm: [handler/comfortable_stop_operate]
```

Actions use `action_server:` and `action_client:`.

### Includes

Child scopes. The name is the ROS namespace.

```yaml
includes:
  tracking:
    manifest: tier4_perception_launch/tracking.yaml
  prediction:
    manifest: tier4_perception_launch/prediction.yaml
  system_monitor:
    if: $(var launch_system_monitor)
    manifest: autoware_system_monitor/system_monitor.yaml
```

Inline includes (for `<group>` blocks) embed the manifest structure
directly instead of referencing a file.

### Global Topics

Absolute topic names referenced without local declaration.

```yaml
global_topics:
  /tf: { type: tf2_msgs/msg/TFMessage, qos: { reliability: reliable, depth: 100 } }
  /clock: { type: rosgraph_msgs/msg/Clock }
```

### Paths

Named causal relations with timing constraints. Declared on nodes and
scopes. See [Latency vs Age](#latency-vs-age) for definitions.

```yaml
# Node-level path (latency only — drops are on topics, not node paths)
nodes:
  centerpoint:
    sub: [pointcloud]
    pub: [objects]
    paths:
      main:
        input: pointcloud
        output: [objects]
        max_latency_ms: 30

# Scope-level path (latency + age + E2E drops)
paths:
  perception:
    input: raw_data
    output: [detections]
    max_latency_ms: 85
    max_age_ms: 150
    max_drop_rate: 0.08
    max_consecutive: 5
```

**Node path fields:**

| Field            | Meaning | If omitted |
|------------------|---------|------------|
| `input`          | Trigger endpoint(s) from `sub:` | Empty = periodic (timer-driven) |
| `output`         | Result endpoint(s) from `pub:` | Required |
| `max_latency_ms` | Worst-case input-to-output time (see definition above) | Not checked; parent looks through (transparent) |
| `min_latency_ms` | Best-case time — faster is suspicious (stale cache?) | Not checked |
| `correlation`    | Multi-input stamp matching: `timestamp` or `latest` | No correlation check |
| `tolerance_ms`   | Max `header.stamp` difference between correlated inputs | Required if `correlation: timestamp` |

Node paths have latency and correlation only. Age is scope-level because
it's a chain property from the original source to the output — an
individual node doesn't know how far upstream the source is. Drops are
topic-level (transport) and scope-level (E2E).

**Scope path fields** (all of the above, plus):

| Field             | Meaning | If omitted |
|-------------------|---------|------------|
| `max_age_ms`      | Max data age from original source | Not checked |
| `max_drop_rate`   | E2E drop rate across the scope (fraction 0-1) | Drop not checked |
| `max_consecutive` | E2E max consecutive drops | Consecutive not checked |

## Static Validation

The checker runs 14 rules on each manifest:

| Rule               | What it catches                                                    | Severity      |
|--------------------|--------------------------------------------------------------------|---------------|
| `endpoint-unique`  | Duplicate endpoint names within a node                             | Error         |
| `wiring`           | Path endpoints not connected by any topic                          | Warning       |
| `qos-compat`       | Invalid QoS values                                                 | Error         |
| `rate-hierarchy`   | Publisher rate < topic rate < subscriber rate                       | Error         |
| `rate-chain`       | Export rate unachievable from upstream                              | Warning       |
| `budget-overflow`  | Descendant budget exceeds ancestor budget (part > whole)           | Error         |
| `scope-budget`     | Sum of children exceeds scope budget; age < latency                | Warning/Error |
| `causal-dag`       | Cycles in the dataflow graph (`state:` breaks cycles)              | Error         |
| `drop-rate`        | Scope max_drop_rate < composed topic rates; effective delivery rate < sub.min_rate_hz | Error |
| `drop-consecutive` | max_consecutive statistically infeasible given drop rate            | Error/Warning |
| `service-wiring`   | Service client with no matching server                             | Warning       |
| `service-type`     | Service with no type; server/client not on node                    | Error/Warning |
| `dangling-entity`  | Topic with 0 publishers; service/action with 0 servers             | Error/Warning |
| `satisfiability`   | Arg combination produces dangling entities; unreachable nodes      | Error/Warning |

**Satisfiability checking**: when args have `type: bool` or `choices:`,
the checker uses Z3 to verify no valid arg combination produces a
structurally broken manifest. A passing manifest is **variant-complete**.

**Dangling entities**: after condition filtering, topics with 0 publishers
(warning — may be wired by parent), services/actions with 0 servers
(error), and empty entities (silently removed) are flagged.

## References

- **AUTOSAR Timing Extensions** (R22-11): event chains, age/reaction constraints
- **CARET**: cause-effect chain latency measurement for ROS 2
- **ROS 2 message_filters**: ApproximateTimeSynchronizer algorithm
- Contract theory foundations: `docs/contract-theory.md`

## Non-Goals

- Automatic manifest resolution (sidecar, AMENT_PREFIX_PATH) — `--manifest-dir` only
- Blocking enforcement via RCL interception (future)
- User-defined chains — scope hierarchy IS the chain structure
- Semantic component extraction — manifests are user-authored
