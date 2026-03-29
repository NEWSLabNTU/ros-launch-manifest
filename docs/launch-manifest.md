# Launch Manifest Design

## Problem

ROS 2 launch files declare which nodes to run but not which topics they
create. Topic creation happens in source code — publishers and subscribers
are invisible until runtime. The Autoware planning simulator launches ~110
nodes that collectively create ~500 topics. If a code change adds or removes
a topic, the divergence goes unnoticed unless someone manually checks.

play_launch already has runtime graph introspection (Phase 25) and RCL
interception (Phase 29). The missing piece is a **reference specification**
to compare against.

## Model

A manifest file describes what one launch file contributes to the
communication graph: its nodes and the topics they create. Manifest files
are organized by package and launch file in a manifest directory.

The executor loads manifests at startup using the scope table from
`record.json`. For each file scope in the launch tree, it looks up
`<manifest_dir>/<pkg>/<stem>.yaml` and applies the scope's namespace
to relative names, so manifest files are reusable across different
namespace contexts.

**Seven concepts:**

1. **Args** — named parameters with defaults. `$(var name)` substitutions
   are resolved from scope args before checking.

2. **Conditions** — `if:` / `unless:` on any entity. Entities where the
   condition is false are excluded from checking.

3. **Node** — a leaf execution entity. Declares named endpoints
   (pub/sub/srv/cli ports) with optional rate/jitter properties.
   Optionally declares causal `paths:` with timing constraints.

4. **Topic** — wires node endpoints together. Carries message type,
   QoS, and optional channel properties (rate, transport drops).

5. **Include** — a child scope (separate manifest or inline group).
   Name = ROS namespace. Has its own nodes, topics, and scope interface.

6. **Scope Interface** — the scope's boundary. Top-level `pub:`, `sub:`,
   `srv:`, `cli:` declare named groups of endpoints that parent scopes
   use to wire children together. (Replaces `imports:`/`exports:`.)

7. **Paths** — named causal relations (input→output) with timing
   constraints. Declared on nodes and scopes.

![Manifest for perception.launch.xml](img/manifest-perception.svg)

## Format

### Quick Example

```yaml
# Minimal manifest — no contracts
version: 1

nodes:
  talker:
    pub: [chatter]
  listener:
    sub: [chatter]

topics:
  chatter:
    type: std_msgs/msg/String
    pub: [talker/chatter]
    sub: [listener/chatter]

exports:
  output: [talker/chatter]
```

```yaml
# With contracts (rate, jitter, drop)
version: 1

nodes:
  talker:
    pub:
      chatter:
        min_rate_hz: 10
        jitter_ms: 5
  listener:
    sub:
      chatter:
        min_rate_hz: 10

topics:
  chatter:
    type: std_msgs/msg/String
    pub: [talker/chatter]
    sub: [listener/chatter]
    rate_hz: 10

exports:
  output: [talker/chatter]
```

```yaml
# With args and conditions (args are required — values from record.json)
args:
  input_topic:                  # required — resolved from scope table
  use_debug_node:               # required

version: 1

nodes:
  processor:
    sub: [input]
    pub: [output]
  debug_viewer:
    if: $(var use_debug_node)
    sub: [output]

topics:
  data:
    type: $(var input_topic)
    pub: [processor/output]
    sub: [debug_viewer/output]
```

### Metadata

| Field              | Required | Description                                                        |
|--------------------|----------|--------------------------------------------------------------------|
| `version`          | yes      | Manifest format version (currently `1`)                            |
| `exclude_patterns` | no       | Topic prefixes to ignore (default: `/rosout`, `/parameter_events`) |

### Args and Substitutions

Manifests declare **args** — named parameters that the manifest needs from
its launch file context. `$(var name)` references in string fields are
replaced with resolved values at check time.

```yaml
args:
  input_objects_topic_name:     # from launch <arg> or <let>
  input_pointcloud_topic_name:  # from launch <arg> or <let>
  use_multithread:              # from launch <arg>
```

**Recommended practice**: declare all args as **required** (null / no
default). The scope table in `record.json` is the single source of truth
for arg values — it captures all resolved launch arguments (`<arg>`
declarations, `<let>` assignments, and expanded YAML config parameters)
per scope. Hardcoding defaults in the manifest duplicates values from
launch files and creates maintenance drift.

**Arg type declarations** (optional — enables satisfiability checking):

```yaml
args:
  # Free string — no constraint (default)
  input_objects_topic_name:

  # Boolean — only "true" or "false"
  launch_collision_detector:
    type: bool

  # Enum — explicit valid values (mirrors ROS 2 <choice>)
  pose_source:
    choices: [ndt, eagleye]
```

Args with `type: bool` or `choices:` enable the checker to enumerate all
valid configurations and verify no combination produces dangling entities
(see Satisfiability Checking below). Free-string args can't be enumerated.

**Shorthand forms:**

```yaml
# Map form — one key per line
args:
  input_topic:                    # free string
  launch_feature:
    type: bool                    # boolean
  mode:
    choices: [fast, slow]         # enum

# List form — all free strings
args: [input_topic, output_topic]
```

**`$(var name)`** substitutions work in any string field — topic types,
endpoint references, import/export lists, include paths, condition
expressions:

```yaml
topics:
  input_objects:
    type: $(var input_objects_topic_name)
    sub: [planner/predicted_objects]

nodes:
  optional_node:
    if: $(var use_feature)
    pub: [output]
```

**Resolution order**:

1. Parse manifest (args declared, mostly required)
2. Merge scope args from `record.json` over any defaults
3. Error if a required arg is not in scope args
4. Replace all `$(var name)` with resolved values
5. Evaluate `if:`/`unless:` conditions and filter excluded entities
6. Proceed to namespace resolution and static checks

The scope table captures all resolved values from the launch tree —
`<arg>` declarations, `<let>` assignments, and YAML config file
parameters expanded by the parser. The manifest doesn't need to model
`<let>` or config file loading separately.

### Conditions

Any node, topic, service, action, or path can have `if:` or `unless:`
fields. These are evaluated after `$(var ...)` substitution and before
static checks. Entities where the condition is false are removed.

```yaml
nodes:
  obstacle_stop_module:
    if: $(var launch_obstacle_stop_module)
    sub: [trajectory]
    pub: [modified_trajectory]

  legacy_planner:
    unless: $(var use_new_planner)
    sub: [route]
    pub: [trajectory]

topics:
  obstacle_trajectory:
    if: $(var launch_obstacle_stop_module) == 'true'
    type: autoware_planning_msgs/msg/Trajectory
    pub: [obstacle_stop_module/modified_trajectory]
```

**Two forms:**

- **Boolean** — bare value: `if: $(var x)` → included when resolved
  value is `"true"` (case-sensitive), excluded otherwise. Matches
  ROS 2 launch XML `if="$(var x)"`.
- **Expression** — comparison: `if: $(var x) == 'value'` → string
  equality. Supports `==`, `!=`, `and`, `or`, parentheses.

`unless:` is the inverse — entity included when condition is **not** true.

```yaml
if: $(var use_sim_time)                          # boolean
if: $(var sensor_model) == 'velodyne'            # string comparison
unless: $(var use_legacy_mode)                   # boolean negation
if: $(var a) == 'x' and $(var b) == 'y'         # compound
if: ($(var a) == 'x' or $(var a) == 'y') and $(var b) == 'z'  # parentheses
```

### Nodes

Nodes declare **endpoints** — named pub/sub/service/action ports.
Endpoint names are **local identifiers within the manifest** used for
wiring (referenced in `topics:` as `node_name/endpoint_name`) and
diagnostics. They don't need to match the node's internal C++ topic
name or the launch file's `<remap from="...">`.

Endpoint names must be **unique per node** across pub, sub, srv, cli.

Endpoints can be a plain list (no properties) or a map with optional
per-endpoint properties:

```yaml
nodes:
  # Map form — with endpoint properties
  lidar_driver:
    pub:
      pointcloud:
        min_rate_hz: 10
        jitter_ms: 5

  # Plain list — no properties
  cropbox_filter:
    pub: [output]
    sub: [input]

  # Sub endpoints with state/required markers
  ndt_scan_matcher:
    sub:
      sensor_points:
        min_rate_hz: 10
      initial_pose:
        required: true
      regularization_pose:
        state: true

  # With service servers
  map_loader:
    srv:
      get_map:
        max_response_ms: 1000

  # With service clients
  mrm_handler:
    cli:
      comfortable_stop_operate: {}
      emergency_stop_operate: {}

  # Minimal — just registers existence
  evaluator:
```

**Subscriber endpoint properties** (all optional):

| Field            | Meaning                                               |
|------------------|-------------------------------------------------------|
| `min_rate_hz`    | Floor — "I need at least this rate"                   |
| `max_rate_hz`    | Ceiling — "I can't process faster" (burst prevention) |
| `state: true`    | Read-latest, not causal (breaks feedback cycles)      |
| `required: true` | Must receive at least once before operational         |

**Publisher endpoint properties** (all optional):

| Field         | Meaning                                        |
|---------------|------------------------------------------------|
| `min_rate_hz` | Floor — "I produce at least this fast"         |
| `max_rate_hz` | Ceiling — "something is wrong if faster"       |
| `jitter_ms`   | Max deviation from ideal period (timer-driven) |

**Service endpoint properties** (all optional):

- `srv:` — this node **serves** the service (receives requests)
- `cli:` — this node **calls** the service (sends requests)

| Field              | Meaning                                                            |
|--------------------|--------------------------------------------------------------------|
| `max_response_ms`  | Max time from request to response (runtime monitoring, no DDS mechanism) |

On `srv:`, `max_response_ms` is the server's commitment. On `cli:`, it's the
client's expectation. `required` is not needed — service clients explicitly
fail when the server is unavailable (unlike topic subscribers which silently
receive nothing). Service QoS is always `reliable/volatile/depth10`
(`rmw_qos_profile_services_default`) and not configurable per-service in ROS 2.

### Composable Nodes

Composable nodes appear as regular nodes. The container is a deployment
detail — from the topic graph perspective, composable nodes publish and
subscribe like any other node. When a composable node is loaded into a
container declared in a different launch file, the node belongs to the
manifest of the launch file that contains `<load_composable_node>`.

### Topics

Topics wire node endpoints together. Each topic declares its type,
which endpoints publish to it, and which subscribe. Endpoints are
referenced as `node/endpoint` or `include_name/export_or_import_name`
for cross-scope wiring.

```yaml
topics:
  # Full form
  cropped:
    type: sensor_msgs/msg/PointCloud2
    pub: [cropbox_filter/output]
    sub: [ground_filter/input]
    qos:
      reliability: best_effort
      depth: 1
    rate_hz: 10
    drop: 1 / 100

  # Shorthand: type only (no wiring)
  debug_output: sensor_msgs/msg/PointCloud2
```

Topic names are **relative** — the parser applies the namespace from
the include context. The real topic name becomes `<ns>/topic_name`.

**Optional endpoint references** — a trailing `?` marks an endpoint ref
as optional. The referenced node may be conditional (`if:`/`unless:`).
After condition filtering, optional refs to filtered-out nodes are silently
dropped. Unmarked refs are required — the checker errors if the node
doesn't exist.

```yaml
topics:
  predicted_trajectory:
    type: autoware_planning_msgs/msg/Trajectory
    pub: [controller/predicted_trajectory]
    sub:
      - validator/predicted_trajectory?       # optional — node has if: condition
      - checker/predicted_trajectory?         # optional
```

`?` is unambiguous — ROS 2 names only allow alphanumeric, underscore, and
slash characters.

**Undeclared topics**: if a node endpoint is not wired by any topic in
the manifest, the auditor emits a warning (not an error). This allows
gradual adoption.

**QoS fields** (all optional — omitted = ROS defaults, not audited):

| Field         | Values                              |
|---------------|-------------------------------------|
| `reliability` | `reliable` \| `best_effort`         |
| `durability`  | `volatile` \| `transient_local`     |
| `depth`       | integer (history depth, keep\_last) |
| `history`     | `keep_last` \| `keep_all`           |
| `lifespan_ms` | integer                             |
| `liveliness`  | `automatic` \| `manual_by_topic`    |

**Channel properties** (all optional):

| Field      | Meaning                                              |
|------------|------------------------------------------------------|
| `rate_hz`  | Negotiated channel rate                              |
| `drop`     | Transport drop tolerance (`N / W` notation)          |

**Rate hierarchy**: `pub.min_rate_hz >= topic.rate_hz >= sub.min_rate_hz`.
The topic `rate_hz` is the negotiated agreement. Per-endpoint
`min_rate_hz` / `max_rate_hz` are optional overrides when sides differ.
Static check: `topic.rate_hz >= max(all sub.min_rate_hz)`.

**Drop notation**: `drop: 5 / 100` means "up to 5 drops per 100
messages." Full form:

```yaml
drop:
  max_count: 5 / 100
  max_consecutive: 3
```

### Services and Actions

Same pattern as topics — declared with type, wire endpoints.

```yaml
services:
  configure:
    type: std_srvs/srv/SetBool
    server: [driver/configure]
    client: [controller/configure]

actions:
  navigate:
    type: nav2_msgs/action/NavigateToPose
    server: [navigator/navigate]
    client: [planner/navigate]
```

### Scope Interface

> **Note**: This section describes the planned unified interface design.
> The current implementation still uses `imports:`/`exports:`. Migration
> is tracked in design-issues.md #12.

A scope declares its external interface using the same endpoint types as
a node: top-level `pub:`, `sub:`, `srv:`, `cli:`. The scope is a
**composite component** — its interface declares what flows in and out.

```yaml
# Scope-level interface — typed endpoint groups
pub:
  control_output:
    - controller/control_cmd
    - controller/predicted_trajectory
sub:
  trajectory_input:
    - controller/trajectory
  localization:
    - controller/kinematic_state
    - lane_departure_checker/kinematic_state?    # when launch_lane_departure_checker
srv:
  operate:
    - mrm_operator/operate
cli:
  operate_mrm:
    - mrm_handler/comfortable_stop_operate
    - mrm_handler/emergency_stop_operate
```

Each entry is a **named group** of endpoint references. The parent scope
wires children using `child_scope/group_name` in topic/service declarations.

**Actions** use `action_server:` and `action_client:` at scope level:

```yaml
action_server:
  navigate: [navigator/navigate]
action_client:
  navigate: [planner/navigate]
```

**`?` suffix**: individual members can be optional (referenced node is
conditional). After condition filtering, `?` members whose node was
filtered are silently removed. If the group becomes empty, the group
itself is removed.

**Cross-scope aggregation**: groups can reference child scope groups:

```yaml
pub:
  detections:
    - lidar/detections
    - camera/detections
```

**Current implementation** (`imports:`/`exports:`):

The current code uses `imports:` (= top-level `sub:` + `cli:`) and
`exports:` (= top-level `pub:` + `srv:`). These will be migrated to the
unified interface in a future release. Both forms are topic-only — service
and action boundary endpoints are not yet supported.

### Includes

Includes represent `<include>` (separate manifest) or `<group>` blocks
(inline). The include **name is the ROS namespace** — it maps to the
`<push-ros-namespace>` in the launch file.

```yaml
includes:
  # External — loaded from separate manifest file
  lidar:
    manifest: tier4_perception_launch/lidar_perception.yaml
  camera:
    manifest: tier4_perception_launch/camera_perception.yaml

  # Inline — from <group> block
  safety:
    nodes:
      emergency_stop:
        pub: [stop_cmd]
        sub: [diagnostics]
    topics:
      stop_command:
        type: std_msgs/msg/Bool
        pub: [emergency_stop/stop_cmd]
    exports:
      commands: [emergency_stop/stop_cmd]
```

External includes use `manifest:` with `package/file.yaml` — resolved
as `<manifest_dir>/package/file.yaml`.

Inline includes have the same structure as a top-level manifest (minus
`version:`). They can contain `nodes:`, `topics:`, `includes:`,
`imports:`, `exports:`, and `paths:`.

Each include must correspond to exactly one `<push-ros-namespace>` in
the launch file.

### Global Topics

Absolute topic names (`/tf`, `/clock`) are references to the runtime
environment. Any scope can reference them in topic `pub:`/`sub:` lists
without local declaration. Optionally constrain their type and QoS:

```yaml
global_topics:
  /tf: { type: tf2_msgs/msg/TFMessage, qos: { reliability: reliable, depth: 100 } }
  /clock: { type: rosgraph_msgs/msg/Clock }
```

Any manifest can declare `global_topics:` — this allows inner launch
files to be launched directly without the top-level manifest.

### Causal Paths

The `paths:` section on a node or scope declares **named causal
paths** — explicit input→output relations with timing constraints.

Each path is a causal relation: "when input arrives, output is
produced." Paths are named (map keys) for diagnostic clarity.

```yaml
paths:
  localization:
    input: sensor_points
    output: [ndt_pose, ndt_pose_with_covariance]
    max_latency_ms: 50
    drop:
      max_count: 10 / 100
      max_consecutive: 5
  debug:
    input: sensor_points
    output: [exe_time_ms, transform_probability]
```

**Path fields:**

| Field              | Meaning                                            |
|--------------------|----------------------------------------------------|
| `input`            | Single endpoint or list of endpoints (from `sub:`)  |
| `output`           | List of endpoints (from `pub:`)                    |
| `max_latency_ms`   | Worst-case time from trigger to output publish     |
| `min_latency_ms`   | Best-case time (optional, anomaly detection)       |
| `max_age_ms`       | Max data age from original source (optional)       |
| `correlation`      | Multi-input matching: `timestamp` or `latest`      |
| `tolerance_ms`     | Max timestamp difference for correlation           |
| `drop`             | Drop tolerance (`N / W` or full form)              |

**Trigger is implicit** from path structure:
- 0 inputs → source/periodic (output-only path)
- 1 input → on_arrival
- N inputs → all_ready (barrier)

#### Node Examples

```yaml
nodes:
  # Simple pipe
  centerpoint:
    sub: [pointcloud]
    pub: [objects]
    paths:
      main: { input: pointcloud, output: [objects], max_latency_ms: 30 }

  # Fusion — all inputs, timestamp-correlated
  fusion_node:
    sub: [lidar_objects, camera_objects]
    pub: [fused]
    paths:
      fusion:
        input: [lidar_objects, camera_objects]
        output: [fused]
        correlation: timestamp
        tolerance_ms: 50
        max_latency_ms: 20
        drop:
          max_count: 10 / 100
          max_consecutive: 3

  # Timer-driven tracker — input is state
  tracker:
    sub:
      fused:
        state: true
    pub:
      tracked_objects:
        min_rate_hz: 10
        jitter_ms: 10
    paths:
      tracking:
        output: [tracked_objects]
        max_age_ms: 200

  # NDT with feedback cycle and map precondition
  ndt_scan_matcher:
    sub:
      sensor_points:
        min_rate_hz: 10
      initial_pose:
        required: true
      regularization_pose:
        state: true
    pub:
      ndt_pose:
        min_rate_hz: 10
      exe_time_ms:
    srv:
      trigger_node:
        max_response_ms: 100
    paths:
      localization:
        input: sensor_points
        output: [ndt_pose]
        max_latency_ms: 50
        min_latency_ms: 10        # < 10ms is suspicious (stale cache?)
        drop:
          max_count: 10 / 100
          max_consecutive: 5
      debug:
        input: sensor_points
        output: [exe_time_ms]

  # Source — periodic output (no paths needed)
  lidar_driver:
    pub:
      pointcloud:
        min_rate_hz: 10
        jitter_ms: 5

```

### Scope Paths

A scope declares `paths:` at the top level using the same structure.
Paths reference import/export group names.

```yaml
imports:
  raw_data: [cropbox_filter/input]
exports:
  detections: [centerpoint/objects]

paths:
  main:
    input: raw_data
    output: [detections]
    max_latency_ms: 50
    max_age_ms: 120              # from original sensor to scope export
```

**Composition**: parent scope budgets compose from children's paths.
Fork-join follows the critical path:

```
perception
  max_latency_ms: 85
  = max(lidar:50, camera:30) + fusion:20 + tracker:15 - overlap
```

The scope tree from Phase 30 provides the composition hierarchy.

### Measurement Sources

| Metric     | Definition                                                | Source                      |
|------------|-----------------------------------------------------------|-----------------------------|
| Latency    | `t_pub - t_take(trigger_input)`                           | RCL interception timestamps |
| Sync       | `max(stamp_i) - min(stamp_i)`                             | RCL interception stamps     |
| Rate       | `t_pub[n] - t_pub[n-1]`                                   | Stats plugin                |
| Jitter     | `|interval - 1/rate_hz|`                                  | Stats plugin                |
| Age        | sum of chain latencies (static) or `t_pub - header.stamp` | Static / interception       |
| Node drop  | `1 - output_count / input_count`                          | Stats plugin                |
| Burst      | max consecutive missing outputs                           | Stats plugin                |
| Xport drop | `1 - sub_take_count / pub_count`                          | Stats plugin                |
| Burstiness | lag-1 autocorrelation, dispersion index, max run          | Stats plugin (always-on)    |

All measurements use the existing Phase 29 interception infrastructure.
Burstiness diagnostics are always-on (negligible cost) but reported
selectively — full detail for topics with `drop:` declared, discovery
alerts for undeclared topics with anomalies.

### Static Validation Rules

The checker runs 11 rules on each manifest:

| Rule | What it catches | Severity |
|------|----------------|----------|
| `endpoint-unique` | Duplicate endpoint names within a node | Error |
| `wiring` | Path endpoints not connected by any topic | Warning |
| `qos-compat` | Invalid QoS values (reliability, durability) | Error |
| `rate-hierarchy` | Publisher rate < topic rate < subscriber rate | Error |
| `rate-chain` | Export rate unachievable from upstream | Warning |
| `scope-budget` | Scope latency < sum of node latencies; age < latency | Warning/Error |
| `causal-dag` | Cycles in the dataflow graph (`state:` breaks cycles) | Error |
| `drop-rate` | Scope drop budget tighter than chain delivery rate | Error |
| `drop-consecutive` | Consecutive drop bound statistically infeasible | Error/Warning |
| `service-wiring` | Service client with no matching server | Warning |
| `service-type` | Service with no type; server/client not on node | Error/Warning |
| `optional-ref` | `?` suffix must match node conditionality | Error |

**Planned rules** (not yet implemented):

| Rule | What it catches | Severity |
|------|----------------|----------|
| `dangling-entity` | Topic with 0 pub after filtering; service/action with 0 server | Error/Warning |
| `satisfiability` | Arg combination that produces dangling entities | Error |
| `unreachable` | Node/topic condition always false for all valid arg values | Warning |

### Dangling Entity Checks

After condition filtering removes nodes and `?` endpoint references are
cleaned up, some entities may become structurally invalid:

- **Topic with 0 publishers** — no data source (warning; may be wired by parent)
- **Topic with 0 publishers and 0 subscribers** — empty, silently removed
- **Service with 0 servers** — calls will fail (error)
- **Action with 0 servers** — goals can't be processed (error)
- **Scope endpoint group empty** — all `?` members filtered (group removed)

### Satisfiability Checking

When args have `type: bool` or `choices:`, the checker can enumerate all
valid configurations and verify no combination produces dangling entities.

For small arg spaces (≤15 finite-domain args), brute-force enumeration
is used. For larger spaces, Z3 SMT solver can find counterexamples
symbolically without enumerating all 2^N combinations.

A manifest that passes satisfiability checking is **variant-complete**:
every valid arg combination produces a structurally sound manifest.

See `docs/design-issues.md` #14 for the full algorithm and Z3 encoding.

### Formal Foundations

The endpoint properties, paths, and topic rate/drop form an
assume-guarantee contract system. Node paths compose to scope paths
via series/parallel rules.

See `docs/contract-theory.md` for the formal foundations and
`docs/contract-verification.md` for implementation tooling.

## Pipeline Example

Autoware-like perception pipeline with branches, merge, and planning.

```
sensing.launch.xml ──→ lidar_perception.launch.xml ──┐
                                                      ├──→ fusion (inline)
sensing.launch.xml ──→ camera_perception.launch.xml ──┘
                                                      ──→ planning.launch.xml
```

**Manifest directory:**
```
manifests/
├── tier4_sensing_launch/
│   └── sensing.yaml
├── tier4_perception_launch/
│   ├── perception.yaml
│   ├── lidar_perception.yaml
│   └── camera_perception.yaml
├── tier4_planning_launch/
│   └── planning.yaml
└── autoware_launch/
    └── planning_simulator.yaml
```

**`tier4_sensing_launch/sensing.yaml`**:
```yaml
version: 1

nodes:
  lidar_driver:
    pub:
      pointcloud:
        min_rate_hz: 10
        jitter_ms: 5
  camera_driver:
    pub:
      image:
        min_rate_hz: 30
        jitter_ms: 3

topics:
  pointcloud:
    type: sensor_msgs/msg/PointCloud2
    pub: [lidar_driver/pointcloud]
    rate_hz: 10
  image:
    type: sensor_msgs/msg/Image
    pub: [camera_driver/image]
    rate_hz: 30

exports:
  pointcloud: [lidar_driver/pointcloud]
  image: [camera_driver/image]
```

**`tier4_perception_launch/lidar_perception.yaml`**:
```yaml
version: 1

nodes:
  cropbox_filter:
    pub: [output]
    sub: [input]
    paths:
      main: { input: input, output: [output], max_latency_ms: 5 }
  ground_filter:
    pub: [output]
    sub: [input]
    paths:
      main: { input: input, output: [output], max_latency_ms: 15 }
  centerpoint:
    pub: [objects]
    sub: [pointcloud]
    paths:
      main: { input: pointcloud, output: [objects], max_latency_ms: 30 }

topics:
  cropped:
    type: sensor_msgs/msg/PointCloud2
    pub: [cropbox_filter/output]
    sub: [ground_filter/input]
  no_ground:
    type: sensor_msgs/msg/PointCloud2
    pub: [ground_filter/output]
    sub: [centerpoint/pointcloud]
  detected_objects:
    type: autoware_perception_msgs/msg/DetectedObjects
    pub: [centerpoint/objects]
    rate_hz: 10
    drop: 5 / 100

imports:
  raw_data: [cropbox_filter/input]
exports:
  detections: [centerpoint/objects]

paths:
  main: { input: raw_data, output: [detections], max_latency_ms: 50 }
```

**`tier4_perception_launch/camera_perception.yaml`**:
```yaml
version: 1

nodes:
  rectifier:
    pub: [output]
    sub: [input]
    paths:
      main: { input: input, output: [output], max_latency_ms: 5 }
  yolo:
    pub: [objects]
    sub: [input]
    paths:
      main: { input: input, output: [objects], max_latency_ms: 25 }

topics:
  rectified:
    type: sensor_msgs/msg/Image
    pub: [rectifier/output]
    sub: [yolo/input]
  detected_objects:
    type: autoware_perception_msgs/msg/DetectedObjects2D
    pub: [yolo/objects]

imports:
  raw_data: [rectifier/input]
exports:
  detections: [yolo/objects]

paths:
  main: { input: raw_data, output: [detections], max_latency_ms: 30 }
```

**`tier4_perception_launch/perception.yaml`**:
```yaml
version: 1

includes:
  lidar:
    manifest: tier4_perception_launch/lidar_perception.yaml
  camera:
    manifest: tier4_perception_launch/camera_perception.yaml

nodes:
  fusion_node:
    sub: [lidar_objects, camera_objects]
    pub: [fused_objects]
    paths:
      fusion:
        input: [lidar_objects, camera_objects]
        output: [fused_objects]
        correlation: timestamp
        tolerance_ms: 50
        max_latency_ms: 20
        drop:
          max_count: 10 / 100
          max_consecutive: 3
  tracker:
    sub:
      fused:
        state: true
    pub:
      tracked_objects:
        min_rate_hz: 10
        jitter_ms: 10
    paths:
      tracking:
        output: [tracked_objects]
        max_age_ms: 200

topics:
  lidar_objects:
    type: autoware_perception_msgs/msg/DetectedObjects
    pub: [lidar/detections]
    sub: [fusion_node/lidar_objects]
    rate_hz: 10
  camera_objects:
    type: autoware_perception_msgs/msg/DetectedObjects2D
    pub: [camera/detections]
    sub: [fusion_node/camera_objects]
    rate_hz: 10
  fused:
    type: autoware_perception_msgs/msg/DetectedObjects
    pub: [fusion_node/fused_objects]
    sub: [tracker/fused]
    qos: { reliability: reliable, depth: 1 }
  tracked:
    type: autoware_perception_msgs/msg/TrackedObjects
    pub: [tracker/tracked_objects]
    rate_hz: 10

imports:
  raw_data:
    - lidar/raw_data
    - camera/raw_data
exports:
  detections: [tracker/tracked_objects]

paths:
  main: { input: raw_data, output: [detections], max_latency_ms: 85, max_age_ms: 150 }
```

**`tier4_planning_launch/planning.yaml`**:
```yaml
version: 1

nodes:
  prediction:
    sub: [tracked_objects]
    pub: [predicted_objects]
    paths:
      main: { input: tracked_objects, output: [predicted_objects], max_latency_ms: 35 }
  motion_planner:
    sub: [predicted_objects]
    pub: [trajectory]
    paths:
      main: { input: predicted_objects, output: [trajectory], max_latency_ms: 65 }

topics:
  predicted:
    type: autoware_perception_msgs/msg/PredictedObjects
    pub: [prediction/predicted_objects]
    sub: [motion_planner/predicted_objects]
    qos: { reliability: reliable, depth: 1 }
  trajectory:
    type: autoware_planning_msgs/msg/Trajectory
    pub: [motion_planner/trajectory]

imports:
  tracked_data: [prediction/tracked_objects]
exports:
  plan: [motion_planner/trajectory]

paths:
  main: { input: tracked_data, output: [plan], max_latency_ms: 100 }
```

**`autoware_launch/planning_simulator.yaml`**:
```yaml
version: 1
exclude_patterns: [/rosout, /parameter_events]

global_topics:
  /tf: { type: tf2_msgs/msg/TFMessage, qos: { reliability: reliable, depth: 100 } }
  /clock: { type: rosgraph_msgs/msg/Clock }

includes:
  sensing:
    manifest: tier4_sensing_launch/sensing.yaml
  perception:
    manifest: tier4_perception_launch/perception.yaml
  planning:
    manifest: tier4_planning_launch/planning.yaml

topics:
  pointcloud:
    type: sensor_msgs/msg/PointCloud2
    pub: [sensing/pointcloud]
    sub: [perception/raw_data]
  image:
    type: sensor_msgs/msg/Image
    pub: [sensing/image]
    sub: [perception/raw_data]
  tracked_objects:
    type: autoware_perception_msgs/msg/TrackedObjects
    pub: [perception/detections]
    sub: [planning/tracked_data]
  trajectory:
    type: autoware_planning_msgs/msg/Trajectory
    pub: [planning/plan]

exports:
  plan: [planning/plan]

paths:
  sensor_to_plan:
    input: [sensing/pointcloud, sensing/image]
    output: [plan]
    max_latency_ms: 200
    max_age_ms: 250              # includes cross-machine transport
```

### Latency Analysis

Each scope's paths are verified independently. The E2E critical
path is derived from the scope tree:

```
sensing (source, 10 Hz)
  → max(lidar: 50ms, camera: 30ms)
  → fusion: 20ms + tracker (periodic, 10 Hz)
  → planning: 100ms
Critical path: 50 + 20 + 100 = 170ms
```

No user-defined chains — the scope hierarchy IS the chain structure.

## References

- **AUTOSAR Timing Extensions** (R22-11): Event chains with fork/join, age
  and reaction constraints.
  ([spec](https://www.autosar.org/fileadmin/standards/R22-11/CP/AUTOSAR_TPS_TimingExtensions.pdf))
- **CARET**: Chain-Aware ROS 2 Evaluation Tool for cause-effect chain latency.
  ([paper](https://www.researchgate.net/publication/369815699))
- **ROS 2 `message_filters`**: ApproximateTimeSynchronizer pivot algorithm.
  ([docs](https://docs.ros.org/en/humble/p/message_filters/doc/index.html))
- **Prior art survey**: `docs/research/manifest-prior-art.md` — Jsonnet, CUE,
  K8s reconciliation, protobuf contracts, system_modes, CARET, ros2-performance.
- **Data quality semantics**: `docs/research/data-quality-semantics.md`

## Non-Goals (v4)

- **Automatic manifest resolution** (sidecar, central store, AMENT_PREFIX_PATH
  lookup) — for now, `--manifest-dir` is the only mechanism.
- **Blocking enforcement** via RCL interception (future).
- **User-defined chains** — replaced by scope-level I/O contracts that
  compose through the launch tree hierarchy.
- **Semantic component extraction** (namespace-based splitting is mechanical;
  meaningful grouping is user-authored).
