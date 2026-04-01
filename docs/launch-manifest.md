# Launch Manifest

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

![Manifest model: scopes, nodes, and wiring](img/manifest-model.svg)

### Concepts

**Scope.** A manifest file describes one scope. A scope corresponds to
one launch file (or one `<group>` block). Scopes form a tree that mirrors
the launch file include hierarchy.

**Node.** A leaf execution entity — a ROS 2 node or composable node.
Declares named **endpoints**: pub, sub, srv, cli. Optionally declares
causal **paths** with timing constraints.

**Endpoint.** A named port on a node. Four kinds: `pub` (publishes),
`sub` (subscribes), `srv` (serves a service), `cli` (calls a service).
Endpoints can have properties: rate, jitter, state, required.

**Topic.** First-class wiring between endpoints. Declares message type,
which endpoints publish, which subscribe, QoS, and channel rate.
In launch files, this information is split across `<remap>` tags,
source code, and convention. The manifest makes it explicit.

**Service / Action.** Request-response wiring. Declares service type,
server endpoints, client endpoints. Actions have server and client sides.

**Scope Interface.** The scope's boundary — top-level `pub:`, `sub:`,
`srv:`, `cli:` groups that declare which internal endpoints are visible
to the parent scope. The parent wires children together by referencing
`child_name/group_name` in its topics and services.

**Include.** A child scope. Maps to `<include>` in launch files. The
include name is the ROS namespace (from `<push-ros-namespace>`). Each
include references a child manifest file.

**Args and Conditions.** Manifests can declare `args:` (named parameters
resolved from the launch tree) and `if:` / `unless:` conditions on any
entity. These mirror `<arg>` and `if="$(var ...)"` in launch XML.

**Paths.** Named causal relations (input → output) with timing constraints:
max latency, max age, drop tolerance. Declared on nodes (node-level paths)
and scopes (scope-level paths). No launch file equivalent — this is the
contract layer that manifests add.

## Worked Example

A perception pipeline with two detection stages feeding a tracker.

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

sub:
  pointcloud: [tracking/detected_objects]
  vector_map: [prediction/vector_map]

pub:
  objects: [prediction/objects]
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

## Format Reference

### Metadata

| Field              | Required | Description                                                        |
|--------------------|----------|--------------------------------------------------------------------|
| `version`          | yes      | Format version (currently `1`)                                     |
| `exclude_patterns` | no       | Topic prefixes to ignore (default: `/rosout`, `/parameter_events`) |

### Args

Named parameters resolved from the launch tree's scope table.

```yaml
args:
  input_topic:                     # free string (default)
  launch_feature:
    type: bool                     # "true" or "false" only
  pose_source:
    choices: [ndt, eagleye, gnss]  # enum — explicit valid values
```

`$(var name)` substitutions work in any string field. Resolved before
condition evaluation and static checks.

Typed args (`bool`, `choices`) enable satisfiability checking — the
checker can verify all valid arg combinations produce sound manifests.

### Conditions

`if:` / `unless:` on any node, topic, service, action, or path.
Evaluated after substitution.

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

```yaml
nodes:
  controller:
    pub:
      cmd: { min_rate_hz: 30 }
    sub:
      trajectory: { min_rate_hz: 10 }
      map: { state: true, required: true }
    srv:
      trigger: { max_response_ms: 100 }
    cli:
      operate: {}
    paths:
      main: { input: trajectory, output: [cmd], max_latency_ms: 10 }
```

Endpoints can be a list (`pub: [a, b]`) or a map with properties.

**Subscriber properties:**

| Field         | Meaning                                       |
|---------------|-----------------------------------------------|
| `min_rate_hz` | Minimum expected receive rate                 |
| `max_rate_hz` | Maximum expected receive rate                 |
| `state`       | Polled (read-latest), not causal              |
| `required`    | Must receive at least once before operational |

**Publisher properties:**

| Field         | Meaning                                   |
|---------------|-------------------------------------------|
| `min_rate_hz` | Minimum publish rate                      |
| `max_rate_hz` | Maximum publish rate                      |
| `jitter_ms`   | Max deviation from ideal period           |

**Service/client properties:**

| Field             | Meaning                        |
|-------------------|--------------------------------|
| `max_response_ms` | Max request-to-response time   |

### Topics

```yaml
topics:
  control_cmd:
    type: autoware_control_msgs/msg/Control
    pub: [controller/cmd]
    sub: [validator/cmd]
    rate_hz: 30
    qos:
      reliability: reliable
      durability: transient_local
      depth: 1
    drop: 1 / 100
```

**Fields:**

| Field         | Required | Description                               |
|---------------|----------|-------------------------------------------|
| `type`        | yes      | ROS message type                          |
| `pub`         | no       | Publisher endpoint refs (`node/endpoint`)  |
| `sub`         | no       | Subscriber endpoint refs                  |
| `rate_hz`     | no       | Negotiated channel rate                   |
| `qos`         | no       | QoS profile (reliability, durability, etc)|
| `drop`        | no       | Drop tolerance: `N / W` or full form      |
| `if`/`unless` | no       | Condition                                 |

**QoS fields:** `reliability` (reliable / best_effort), `durability`
(volatile / transient_local), `depth`, `history`, `lifespan_ms`,
`liveliness`.

**Drop notation:** `drop: 5 / 100` means "up to 5 drops per 100 messages."
Full form: `drop: { max_count: 5 / 100, max_consecutive: 3 }`.

**Rate hierarchy:** `pub.min_rate_hz >= topic.rate_hz >= sub.min_rate_hz`.

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
scopes.

```yaml
# Node-level path
nodes:
  centerpoint:
    sub: [pointcloud]
    pub: [objects]
    paths:
      main:
        input: pointcloud
        output: [objects]
        max_latency_ms: 30
        drop: 5 / 100

# Scope-level path
paths:
  perception:
    input: raw_data
    output: [detections]
    max_latency_ms: 85
    max_age_ms: 150
```

**Path fields:**

| Field            | Meaning                                          |
|------------------|--------------------------------------------------|
| `input`          | Trigger endpoint(s) from `sub:` (empty = periodic)|
| `output`         | Result endpoint(s) from `pub:`                   |
| `max_latency_ms` | Worst-case input-to-output time                  |
| `min_latency_ms` | Best-case time (anomaly detection)               |
| `max_age_ms`     | Max data age from original source                |
| `correlation`    | Multi-input matching: `timestamp` or `latest`    |
| `tolerance_ms`   | Max timestamp difference for correlation         |
| `drop`           | Drop tolerance                                   |

## Static Validation

The checker runs 13 rules on each manifest:

| Rule               | What it catches                                                    | Severity      |
|--------------------|--------------------------------------------------------------------|---------------|
| `endpoint-unique`  | Duplicate endpoint names within a node                             | Error         |
| `wiring`           | Path endpoints not connected by any topic                          | Warning       |
| `qos-compat`       | Invalid QoS values                                                 | Error         |
| `rate-hierarchy`   | Publisher rate < topic rate < subscriber rate                       | Error         |
| `rate-chain`       | Export rate unachievable from upstream                              | Warning       |
| `scope-budget`     | Scope latency < sum of node latencies; age < latency               | Warning/Error |
| `causal-dag`       | Cycles in the dataflow graph (`state:` breaks cycles)              | Error         |
| `drop-rate`        | Scope drop budget tighter than chain delivery rate                 | Error         |
| `drop-consecutive` | Consecutive drop bound statistically infeasible                    | Error/Warning |
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
