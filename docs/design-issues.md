# Design Issues

Open design questions for the manifest format, with proposed solutions.

---

## 1. Args, Substitutions, and Conditions

### Problem

ROS 2 launch files are parameterized templates. The same launch file produces
different graphs depending on arguments:

```xml
<!-- motion_planning.launch.xml -->
<arg name="input_objects_topic_name" default="/perception/object_recognition/objects"/>
<arg name="launch_obstacle_stop_module" default="true"/>

<!-- Topic name depends on arg -->
<remap from="~/input/dynamic_objects" to="$(var input_objects_topic_name)"/>

<!-- Node conditionally included -->
<let name="modules" value="$(eval '...')" if="$(var launch_obstacle_stop_module)"/>
```

Today the manifest uses generic import names (`predicted_objects`, `pointcloud`)
that don't match the actual resolved topic names. The manifest also can't
express conditional nodes/topics. This means:

1. **Wiring checks are incomplete** — the checker can't verify that
   `motion_velocity_planner/predicted_objects` maps to
   `/perception/object_recognition/objects` in this specific invocation
2. **Conditional nodes are invisible** — if a node only exists when
   `launch_obstacle_stop_module == 'true'`, the manifest either always
   declares it (false positive when disabled) or never declares it (misses it)

### Proposed Solution: Mirror Launch File Primitives

Make the manifest a parameterized template, just like the launch file it
describes. Three features, matching ROS 2 launch XML:

#### 1. Args (like `<arg>`)

```yaml
args:
  input_objects_topic_name: /perception/object_recognition/objects   # has default
  input_pointcloud_topic_name: /perception/obstacle_segmentation/pointcloud
  launch_obstacle_stop_module: "true"
  use_multithread: "true"
  vehicle_model:              # required — no default, must be provided by caller
  sensor_model:               # required
```

Plain value = default. Null (empty) = required — must be provided by the
scope's args or the check fails. Same shorthand pattern as endpoint
properties (`sub: { topic_a: {}, topic_b: null }`).

At check time, the scope's `args` from the launch tree override these defaults.
The scope table in `record.json` already captures all resolved values —
both `<arg>` declarations and `<let>` assignments are stored in the same
`args` map after parse-time evaluation. The manifest doesn't need to model
`<let>` separately; it just consumes the final resolved values.

```json
{
  "id": 88,
  "origin": {"pkg": "tier4_planning_launch", "file": "motion_planning.launch.xml"},
  "args": {
    "input_objects_topic_name": "/perception/object_recognition/objects",
    "input_pointcloud_topic_name": "/perception/obstacle_segmentation/pointcloud",
    "launch_obstacle_stop_module": "true",
    ...
  }
}
```

#### 2. String Substitutions (like `$(var ...)`)

`$(var name)` in any string value is replaced with the resolved arg value:

```yaml
nodes:
  motion_velocity_planner:
    sub:
      predicted_objects:          # resolved to actual topic name from args
        state: true

topics:
  input_objects:
    type: autoware_perception_msgs/msg/PredictedObjects
    sub: [motion_velocity_planner/predicted_objects]

imports:
  # Actual topic name comes from launch args — matches the remap in launch XML
  objects_input:
    topic: $(var input_objects_topic_name)
    endpoints: [motion_velocity_planner/predicted_objects]
  pointcloud_input:
    topic: $(var input_pointcloud_topic_name)
    endpoints: [motion_velocity_planner/pointcloud]
```

Substitution happens **before** static checks and namespace resolution.
Unresolved `$(var ...)` (no matching arg) is an error.

#### 3. Conditions (like `if=`/`unless=`)

Two forms, matching ROS 2 launch XML:

- **`if:`** — entity included when condition is true
- **`unless:`** — entity included when condition is false (syntactic sugar)

Evaluated after substitution, before checking. Entity is excluded when the
condition evaluates to exclusion.

**Boolean form** (same as launch XML `if=`/`unless=`):

```yaml
nodes:
  obstacle_stop_module_loader:
    if: $(var launch_obstacle_stop_module)       # included when "true"
    sub: [trajectory]
    pub: [modified_trajectory]

  legacy_planner:
    unless: $(var use_new_planner)               # included when NOT "true"
    sub: [route]
    pub: [trajectory]
```

Boolean evaluation: the substituted string is compared to `"true"` (case-
sensitive). `"true"` → true, anything else → false. This matches how
ROS 2 launch XML evaluates `if="$(var x)"`.

**Expression form** (for complex conditions):

```yaml
topics:
  obstacle_stop_trajectory:
    if: $(var launch_obstacle_stop_module) == 'true'
    type: autoware_planning_msgs/msg/Trajectory
    pub: [obstacle_stop_module_loader/modified_trajectory]
    sub: [next_module/input]

  velodyne_points:
    if: $(var sensor_model) == 'velodyne' or $(var sensor_model) == 'hesai'
    type: sensor_msgs/msg/PointCloud2
    pub: [driver/pointcloud]
```

**Condition grammar** (adapted from REP-149, simplified):

```
expr     := compare (('and' | 'or') compare)*
compare  := value (('==' | '!=') value)?
value    := substitution | literal | bare_bool
literal  := quoted_string | bare_word
bare_bool := result of $(var ...) substitution, no comparison operator
```

When the expression is a single substituted value with no comparison
operator, it's evaluated as a boolean: `"true"` → true, else false.

All comparisons are **string equality**. No numeric comparison — launch args
are always strings. `and`/`or` have equal precedence (use parentheses to
disambiguate).

Examples:
```yaml
if: $(var use_sim_time)                          # boolean: true when "true"
if: $(var use_sim_time) == 'true'                # explicit comparison (same result)
if: $(var sensor_model) == 'velodyne'            # string comparison
unless: $(var use_legacy_mode)                   # included when NOT "true"
if: $(var a) == 'x' and $(var b) == 'y'         # compound
```

### How It Works End-to-End

```
manifest.yaml (template)
    + scope args (from record.json)
    ──→ substitution (resolve $(var ...))
    ──→ condition evaluation (filter if: false)
    ──→ namespace resolution (apply scope ns)
    ──→ static checks (9 rules)
    ──→ diagnostics
```

This is the same pipeline as today, with two new steps inserted before
namespace resolution. The checker itself doesn't change.

### Example: motion_planning.yaml with args

```yaml
args:
  input_objects_topic_name: /perception/object_recognition/objects
  input_pointcloud_topic_name: /perception/obstacle_segmentation/pointcloud
  launch_obstacle_stop_module: "true"

version: 1

nodes:
  motion_velocity_planner:
    sub:
      input_trajectory:
        min_rate_hz: 10
      predicted_objects:            # actual topic: $(var input_objects_topic_name)
        state: true
      pointcloud:                   # actual topic: $(var input_pointcloud_topic_name)
        state: true
      # ... other state subs
    pub:
      output_trajectory:
        min_rate_hz: 10

imports:
  objects_input:
    topic: $(var input_objects_topic_name)
    endpoints: [motion_velocity_planner/predicted_objects]
  pointcloud_input:
    topic: $(var input_pointcloud_topic_name)
    endpoints: [motion_velocity_planner/pointcloud]
```

When checked with `play_launch check --manifest-dir . autoware_launch planning_simulator.launch.xml`,
the scope args override defaults, `$(var input_objects_topic_name)` resolves to
`/perception/object_recognition/objects`, and the wiring check can verify the
actual topic name matches the runtime graph.

### Comparison with Launch File Syntax

| Launch XML                    | Manifest YAML           | Notes                                              |
|-------------------------------|-------------------------|----------------------------------------------------|
| `<arg name="x" default="v"/>` | `args: { x: v }`       | Value = default; null = required                   |
| `<arg name="x"/>` (required)  | `args: { x: }`         | Null means no default                              |
| `$(var x)`                    | `$(var x)`              | Same syntax                                        |
| `if="$(var x)"`               | `if: $(var x)`          | Same semantics — boolean ("true" = true)           |
| `unless="$(var x)"`           | `unless: $(var x)`      | Same semantics — boolean negation                  |
| `if="$(eval ...)"`            | `if: $(var x) == 'val'` | No eval needed — use explicit comparison           |
| `$(eval ...)`                 | Not supported           | Manifests don't need eval — args are pre-resolved  |

### Implementation Path

1. **Types crate**: add `args: BTreeMap<String, Option<String>>` to `Manifest`
   (value = default, None = required), add `if_condition: Option<String>` and
   `unless_condition: Option<String>` (YAML keys `if:`, `unless:`) to `NodeDecl`,
   `TopicDecl`, `ServiceDecl`, `PathDecl`, `IncludeDecl`
2. **Types crate**: add `substitute(manifest, args) -> Manifest` that resolves
   all `$(var ...)` in string fields
3. **Types crate**: add `evaluate_condition(expr, args) -> bool` parser for
   the condition grammar
4. **Manifest loader** (`manifest_loader.rs`): merge scope args over manifest
   defaults, run substitution, filter by conditions, then proceed to namespace
   resolution and checking
5. **Tests**: manifest with args + conditions, checked with different arg sets

---

## 2. Service Contracts

### Problem

ROS 2 services use a request-response pattern. Unlike topics, there are no
native QoS mechanisms for service latency guarantees:

- **No deadline policy** for services in shipping ROS 2 (design exists in
  [REP-2009 design doc](https://design.ros2.org/articles/qos_deadline_liveliness_lifespan.html)
  but was deferred)
- **No lifespan** (request timeout) at the RMW layer
- Only application-level `wait_for_service()` and `future.wait_for()` timeouts

The manifest format has `srv:` and `cli:` for declaring service endpoints,
and `services:` at scope level for wiring, but no contract properties beyond
existence.

### Proposed Solution: Service Contract Properties

Extend `SrvEndpointProps` with an optional latency contract:

```yaml
nodes:
  pose_initializer:
    srv:
      initialize:
        max_response_ms: 5000       # server must respond within 5s

  mrm_handler:
    cli:
      comfortable_stop_operate:
        max_response_ms: 100        # client expects response within 100ms
```

At scope level, `services:` already declares type and wiring:

```yaml
services:
  initialize:
    type: autoware_localization_srvs/srv/Initialize
    server: [pose_initializer/initialize]
    client: [adapi/localization_node/initialize]
```

`required` is not needed on service endpoints. Unlike topic subscribers
(which silently receive nothing if no publisher exists), service clients
explicitly fail when the server is unavailable — the request-response
pattern inherently requires the server to exist. The `service_wiring`
static rule catches missing servers at authoring time.

Service QoS is always `reliable/volatile/depth10`
(`rmw_qos_profile_services_default`) and is not configurable per-service
in ROS 2, so no `qos:` field is needed on services.

### Contract Semantics

| Field             | Meaning                               | Enforcement                                |
|-------------------|---------------------------------------|--------------------------------------------|
| `max_response_ms` | Maximum time from request to response | Runtime monitoring only (no DDS mechanism) |

### What Can Be Checked Statically

- **Wiring**: every `cli:` endpoint has a matching `services:` entry with a server
- **Type compatibility**: client and server use the same service type

### What Requires Runtime

- **max_response_ms**: measured via service call interception (future work —
  would need to hook `rcl_send_request`/`rcl_take_response` similar to
  topic interception)

### Implementation Path

1. Add `max_response_ms: Option<f64>` to `SrvEndpointProps` in the types crate
2. Add a `service_wiring` validation rule: check that every `cli:` endpoint
   has a matching `services:` entry with a `server:` list
3. Add a `service_type` validation rule: check that service type matches
   between client and server declarations
4. Defer `max_response_ms` enforcement to runtime monitoring

### Scope

Steps 1-3 are manifest format + static checker extensions.
Step 4 is runtime work that depends on service call interception.

---

## 3. Stale Descriptions in launch-manifest.md

Issues found during review against the current implementation. These are
doc fixes, not design changes.

### 3a. Parser vs executor loading

**Lines 21-25** say "The parser loads manifests alongside launch files."
Wrong — we decided the parser stays decoupled (31.4 decision). The executor
loads manifests at startup using the scope table from `record.json`.

**Fix**: Replace with "The executor loads manifests at startup. For each
file scope in the launch tree, it looks up `<manifest_dir>/<pkg>/<stem>.yaml`
and applies the scope's namespace to relative names."

### 3b. `max_latency_ms` on services

**Line 452** still uses `max_latency_ms: 100` on `srv: trigger_node`.
Renamed to `max_response_ms`.

**Fix**: Replace with `max_response_ms: 100`.

### 3c. Design doc cross-references

**Lines 530-531** reference `docs/design/contract-theory.md`. These files
moved to `docs/contract-theory.md` (relative to the submodule root).

**Fix**: Update paths.

---

## 4. Missing `cli:` Documentation

The doc describes `srv:` (service servers) but doesn't mention `cli:` (service
clients). The types crate supports `cli:` on nodes, and the Autoware manifests
use it (e.g., `mrm_handler` has 3 `cli:` entries for OperateMrm calls).

**Fix**: Add `cli:` to the node endpoint documentation alongside `srv:`.
Both use `SrvEndpointProps` (currently just `max_response_ms`). Clarify:

- `srv:` = this node **serves** the service (receives requests)
- `cli:` = this node **calls** the service (sends requests)

Both can declare `max_response_ms` — on `srv:` it's the server's commitment,
on `cli:` it's the client's expectation.

---

## 5. Endpoint Names vs Topic Names

### Problem

The doc says (lines 107-109):

> Endpoint names are the node's pre-remap topic names (before launch file
> `<remap>` is applied). Each endpoint must have a corresponding
> `<remap from="...">` in the launch file.

This is confusing and doesn't match practice. In the Autoware manifests,
endpoint names are **manifest-local identifiers** (e.g., `predicted_objects`,
`kinematic_state`) that are wired to actual topics via the `topics:` section.
They don't need to match the node's internal C++ topic name.

With the args/substitution design, the actual resolved topic name is captured
via imports with `topic: $(var input_objects_topic_name)`, not via endpoint
naming.

### Proposed clarification

Endpoint names are **local identifiers within the manifest**. They serve
two purposes:

1. **Wiring**: referenced in `topics:` as `node_name/endpoint_name` to
   connect publishers and subscribers
2. **Diagnostics**: shown in error messages to identify which endpoint
   has a problem

They don't need to match the node's internal C++ topic name or the launch
file's `<remap from="...">`. The manifest's `topics:` section is the single
source of truth for how endpoints map to ROS topics.

**Fix**: Replace lines 107-109 with this clarification.

---

## 6. Import Topic Mapping

### Problem

The current import format only lists endpoints:

```yaml
imports:
  raw_data: [cropbox_filter/input]
```

This doesn't capture what actual ROS topic the import maps to. When the topic
name depends on a launch arg (`$(var input_objects_topic_name)`), there's no
way to express the mapping.

### Proposed solution

Add an optional `topic:` field to imports that captures the resolved topic name:

```yaml
imports:
  objects_input:
    topic: $(var input_objects_topic_name)
    endpoints: [motion_velocity_planner/predicted_objects]
  pointcloud_input:
    topic: $(var input_pointcloud_topic_name)
    endpoints: [motion_velocity_planner/pointcloud]
```

The short form (list of endpoints) remains valid for imports where the topic
name is obvious or matches the endpoint name:

```yaml
imports:
  raw_data: [cropbox_filter/input]     # short form — topic name inferred
```

At check time, the `topic:` field is resolved via `$(var ...)` substitution,
then used for wiring verification against the runtime graph.

### Implementation

1. Change `imports` type from `BTreeMap<String, Vec<String>>` to
   `BTreeMap<String, ImportDecl>` where `ImportDecl` is either a list
   (shorthand) or a struct with `topic` + `endpoints` fields
2. Parser handles both forms (same pattern as endpoint list vs map)
3. Manifest loader resolves `$(var ...)` in `topic:` field from scope args

---

## 7. Global Topics Wiring Gap

### Problem

`global_topics:` declares type/QoS for absolute topics (`/tf`, `/clock`) but
doesn't wire them to node endpoints. A node can `sub: [tf]` but there's no
mechanism to say "this endpoint subscribes to the global `/tf`" without
creating a local topic entry.

### Proposed solution

Allow `global_topics:` names in topic `pub:`/`sub:` lists directly:

```yaml
global_topics:
  /tf: { type: tf2_msgs/msg/TFMessage }

nodes:
  ndt_scan_matcher:
    sub:
      tf:
        state: true

# No local topic entry needed — the checker sees "ndt_scan_matcher/tf"
# and recognizes it as a reference to the global /tf topic
```

Alternatively, keep it simple: global topics are just documentation. The
checker doesn't enforce them — they exist only for runtime graph auditing.
This avoids complexity in the static checker.

**Recommendation**: Keep it simple. Global topics are documentation only.
The checker ignores them. Runtime auditing (Phase 31.7) can use them.

---

## 8. External Include File Naming

### Problem

The doc examples use `.launch.yaml` suffix for external includes:

```yaml
includes:
  lidar:
    manifest: tier4_perception_launch/lidar_perception.launch.yaml
```

But `resolve_manifest_path()` strips `.launch.xml`/`.launch.py` from the
scope's file name and appends `.yaml`. So the actual files are named
`lidar_perception.yaml`, not `lidar_perception.launch.yaml`.

### Fix

Update examples to use `.yaml` without `.launch`:

```yaml
includes:
  lidar:
    manifest: tier4_perception_launch/lidar_perception.yaml
```

This matches the actual file naming convention in `autoware-contract/`.

---

## Summary

| Issue                           | Type                    | Effort  | Priority                                                       |
|---------------------------------|-------------------------|---------|----------------------------------------------------------------|
| Args + substitutions + `if:`    | Format + types + loader | Medium  | High — enables exact topic name matching and conditional nodes |
| Service wiring check            | New static rule         | Small   | Medium — catches missing service providers                     |
| Service `max_response_ms`       | Format + runtime        | Large   | Low — no DDS mechanism, needs service call interception        |
| Stale descriptions (3a-3c)      | Doc fix                 | Trivial | High — misleading                                              |
| Missing `cli:` docs (4)         | Doc fix                 | Trivial | Medium                                                         |
| Endpoint name clarification (5) | Doc fix                 | Small   | High — source of confusion                                     |
| Import topic mapping (6)        | Format extension        | Small   | High — needed for args/substitution                            |
| Global topics wiring (7)        | Design decision         | Small   | Low — keep as documentation only                               |
| External include naming (8)     | Doc fix                 | Trivial | Medium — inconsistent examples                                 |
