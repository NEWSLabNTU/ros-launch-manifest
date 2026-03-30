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
  input_objects_topic_name:     # required — value from record.json scope
  input_pointcloud_topic_name:  # required
  launch_obstacle_stop_module:  # required
  use_multithread:              # required
  vehicle_model:                # required
```

**Decision (Option B)**: All args should be declared as **required** (null /
no default). `record.json` is the single source of truth for arg values —
the scope table captures all resolved launch arguments, `<let>` assignments,
and YAML config parameters. Hardcoding defaults in the manifest duplicates
values from launch files and creates maintenance drift.

Defaults are syntactically supported (value present = default) but
discouraged. The manifest declares *which* args it needs; the scope table
provides the values.

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
  input_objects_topic_name:     # required — value from record.json
  input_pointcloud_topic_name:  # required
  launch_obstacle_stop_module:  # required

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
| `<arg name="x" default="v"/>` | `args: { x: }`         | Recommended: required. record.json has the value   |
| `<arg name="x"/>` (required)  | `args: { x: }`         | Same — all args are required                       |
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

## ~~6. Import Topic Mapping~~ — Dropped

The parent manifest's `topics:` section already wires child imports to actual
topic names via `sub: [child_scope/import_group]`. The child import only
declares the scope boundary — the resolved topic name is the parent's job.
Adding `topic:` to imports would be redundant with the existing composition
model.

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

## 9. Dangling Endpoint References After Condition Filtering

### Problem

When `filter_manifest()` removes a conditional node, topics that reference
its endpoints are left with dangling references:

```yaml
nodes:
  lane_departure_checker:
    if: $(var launch_lane_departure_checker)
    sub: [predicted_trajectory]

topics:
  predicted_trajectory:
    sub:
      - control_validator/predicted_trajectory
      - lane_departure_checker/predicted_trajectory  # ← dangling if node filtered
```

Users may assume every endpoint in a topic's `pub:`/`sub:` list exists at
runtime. Dangling refs are confusing and may trigger false wiring warnings.

### Why Not `if:` on Individual Endpoint References?

The `if:` condition uses args from the manifest where the node is declared.
In cross-scope scenarios, the parent manifest wires child imports/exports —
the condition context of the child node is not available in the parent's
`topics:` section. Conditions can't cross manifest boundaries.

Even intra-scope, adding `if:` to individual `pub:`/`sub:` entries would
duplicate the condition already on the node. This violates DRY and creates
a maintenance burden.

Prior art supports this constraint:
- **AUTOSAR**: variation points on connectors use the same mechanism as
  components, but in practice the connector condition matches the endpoint's
  component condition — it's the same `SW-SYSCOND` expression.
- **AADL**: `in modes (...)` on connections must be consistent with the
  subcomponent's mode membership — the language enforces this.
- Both systems accept that the full topology is a **superset** — variant
  resolution selects the active subset.

### Solution: Post-Filter Cleanup + Comment Convention

**Tool behavior** — after `filter_manifest()` removes conditional nodes,
a second pass removes dangling endpoint references from topic `pub:`/`sub:`
and service `server:`/`client:` lists:

```rust
fn cleanup_dangling_refs(manifest: &mut Manifest) {
    let node_names: HashSet<&str> = manifest.nodes.keys().map(|s| s.as_str()).collect();
    for topic in manifest.topics.values_mut() {
        topic.publishers.retain(|r| {
            r.split_once('/').map_or(true, |(node, _)| node_names.contains(node))
        });
        topic.subscribers.retain(|r| {
            r.split_once('/').map_or(true, |(node, _)| node_names.contains(node))
        });
    }
    for svc in manifest.services.values_mut() {
        svc.server.retain(|r| {
            r.split_once('/').map_or(true, |(node, _)| node_names.contains(node))
        });
        svc.client.retain(|r| {
            r.split_once('/').map_or(true, |(node, _)| node_names.contains(node))
        });
    }
}
```

**`?` suffix on endpoint references** — a trailing `?` marks an endpoint
ref as optional (the referenced node may be conditional):

```yaml
topics:
  predicted_trajectory:
    type: autoware_planning_msgs/msg/Trajectory
    pub: [controller_node_exe/predicted_trajectory]
    sub:
      - control_validator/predicted_trajectory?                  # when launch_control_validator
      - lane_departure_checker/predicted_trajectory?             # when launch_lane_departure_checker
      - autonomous_emergency_braking/predicted_trajectory?       # when launch_autonomous_emergency_braking
```

- **Unmarked** ref → required. Checker errors if the node doesn't exist
  after condition filtering.
- **`?` suffix** → optional. Silently dropped during post-filter cleanup
  if the referenced node was filtered out.

`?` is unambiguous — it's not a valid character in ROS 2 topic, service,
or node names (only alphanumeric, underscore, slash allowed per REP-144
and rcl validation).

### Validation Rule: `optional-ref`

The checker enforces `?` correctness (12th rule, total now 12):

- Ref to a node with `if:`/`unless:` **must** have `?` suffix → error if missing
- Ref to a node without condition **must not** have `?` suffix → error if present

This rule runs on the raw (pre-filter) manifest. After `filter_manifest()`,
conditions are cleared on surviving entities and `?` is stripped — so the
rule is a no-op on the filtered manifest.

### Implementation — Done

1. `cleanup_dangling_refs()` in `cond.rs` — called from `filter_manifest()`
2. `filter_manifest()` clears `if_condition`/`unless_condition` on surviving
   entities after evaluation
3. `optional_ref.rs` validation rule — enforces `?` matches conditionality
4. 7 new tests: 4 in `cond::tests` (cleanup behavior), 3 in `checker_tests`
   (rule validation)

---

## 10. Cross-Scope Service Wiring (`service_imports:` / `service_exports:`)

### Problem

The manifest has `imports:`/`exports:` for topic endpoints at scope
boundaries, but no equivalent for services. MRM handler's `cli:` targets
are servers in different scopes — the `service-wiring` rule can't verify
cross-scope calls.

### Proposed Solution: `service_imports:` / `service_exports:`

Mirror the topic import/export pattern:

**Child scope (mrm_handler.yaml)**:
```yaml
nodes:
  mrm_handler:
    cli:
      comfortable_stop_operate: {}
      emergency_stop_operate: {}

service_imports:
  # cli endpoints that call servers in other scopes
  comfortable_stop:
    - mrm_handler/comfortable_stop_operate
  emergency_stop:
    - mrm_handler/emergency_stop_operate
```

**Child scope (mrm_comfortable_stop_operator.yaml)**:
```yaml
nodes:
  mrm_comfortable_stop_operator:
    srv:
      operate: {}

service_exports:
  # srv endpoints available to other scopes
  comfortable_stop:
    - mrm_comfortable_stop_operator/operate
```

**Parent scope (system.yaml, if it existed)**:
```yaml
services:
  comfortable_stop_operate:
    type: tier4_system_msgs/srv/OperateMrm
    server: [mrm_comfortable_stop_operator/comfortable_stop]  # via service_exports
    client: [mrm_handler/comfortable_stop]                     # via service_imports
```

### Semantics

- `service_imports:` — `cli:` endpoints that **call** servers outside this scope
- `service_exports:` — `srv:` endpoints that **serve** clients outside this scope
- Parent scope wires them in `services:` using `child_name/export_group_name`

### Checker Rules

- `service-wiring`: if a `cli:` endpoint is in `service_imports:`, don't warn
  about missing `services:` entry (the parent handles the wiring)
- `service-type`: parent's `services:` entry must have `type:` matching both sides

### Implementation

1. Add `service_imports: BTreeMap<String, Vec<String>>` and
   `service_exports: BTreeMap<String, Vec<String>>` to `Manifest` type
2. Parser handles the same list-of-endpoints format as topic imports/exports
3. Update `service-wiring` rule to exclude `cli:` endpoints that appear in
   `service_imports:` (those are wired by the parent)
4. Substitution engine walks `service_imports`/`service_exports` strings
5. Test: cross-scope service wiring with parent manifest

---

## 11. Parser Should Record All Resolved Args in Scope Table

### Problem

The play_launch parser only records args in `scope.args` that were
explicitly passed from a parent via `<arg name="x" value="..."/>` in the
`<include>` tag. Args with defaults in the launch XML (`<arg name="x"
default="v"/>`) that are never overridden by the parent are resolved
locally but **not stored** in `scope.args`.

This causes manifest arg resolution to fail: the manifest declares
`args: [input_traffic_light_topic_name]` (required), but the scope table
doesn't have it because the parser used the launch XML default without
recording it.

### Impact

Discovered during Phase 4 end-to-end validation:
- `behavior_planning.yaml` had to remove 2 args that weren't in scope.args
- Any arg with a default in the launch XML that isn't passed from the parent
  is invisible to the manifest system

### Fix

**In the parser** (`play_launch_parser`): when processing `<arg name="x"
default="v"/>`, always store the resolved value in the scope's args map,
regardless of whether it was passed from the parent or resolved from the
default.

This ensures `scope.args` contains **all** resolved arg values — matching
the principle that record.json is the single source of truth.

### Scope

This is a parser change, not a manifest format change. Affects:
- `src/play_launch_parser/` — scope arg recording during include processing
- All downstream consumers of `scope.args` benefit automatically

### Implementation — Done (Phase 33.1)

Added `ScopeTable::update_args()` method. All 6 include paths now call
`update_args()` after traversal to capture locally-resolved defaults.
Autoware `behavior_planning` scope went from 166 to 174 args.

---

## 12. Unified Scope Interface (`pub:`/`sub:`/`srv:`/`cli:` at Top Level)

### Problem

The current `imports:`/`exports:` only handle topic endpoints. Services and
actions have no scope boundary mechanism. The naming is directional
(`imports` = in, `exports` = out) which doesn't fit bidirectional actions.

### Proposed Solution: Scope as Component

A scope declares its external interface using the same fields as a node:
top-level `pub:`, `sub:`, `srv:`, `cli:`. The scope **is** a composite
component with typed ports.

```yaml
version: 1

# Scope-level interface — this launch file's external ports
pub:
  control_output:
    - controller/control_cmd
    - controller/predicted_trajectory
sub:
  trajectory_input:
    - controller/trajectory
  localization:
    - controller/kinematic_state
    - lane_departure_checker/kinematic_state?   # when launch_lane_departure_checker
srv:
  operate:
    - mrm_operator/operate
cli:
  operate_mrm:
    - mrm_handler/comfortable_stop_operate
    - mrm_handler/emergency_stop_operate

nodes:
  controller:
    pub: [control_cmd, predicted_trajectory]
    sub: [trajectory, kinematic_state]
  # ...
```

**Replaces**: `imports:` (→ top-level `sub:` + `cli:`) and `exports:`
(→ top-level `pub:` + `srv:`).

**Groups**: each entry is a named group of endpoints (list form). The parent
references `child_scope/group_name`.

**Actions**: actions are bidirectional — an action server both receives goals
and sends feedback. Use the node-level declaration type:

```yaml
# Child scope with action server
action_server:
  navigate:
    - navigator/navigate

# Child scope with action client
action_client:
  navigate:
    - mission_planner/navigate
```

The parent wires them in `actions:`:

```yaml
actions:
  navigation:
    type: nav2_msgs/action/NavigateToPose
    server: [navigator_scope/navigate]
    client: [planner_scope/navigate]
```

### `?` on Group Members

The `?` suffix applies to individual members within a group, not to the
group name. After condition filtering:

- Members with `?` whose node was filtered → removed from group
- Group becomes empty → group itself is removed
- Parent ref to removed group → dangling (parent should use `?`)

Optionality propagates upward through the tree automatically.

### Implementation — Done (Phase 33.2)

Replaced `imports:`/`exports:` with top-level `pub:`/`sub:`/`srv:`/`cli:`
plus `action_server:`/`action_client:`. Updated types, parser, substitution
engine, condition filter, checker rules, all 8 fixture YAMLs, and 36
Autoware contract manifests. Total: 6 scope interface fields on `Manifest`.

---

## 13. Dangling Entity Checks After Condition Filtering

### Problem

After condition filtering removes nodes and `cleanup_dangling_refs` removes
`?` endpoint refs, some entities may become structurally invalid:

- A topic with no publishers (no data source)
- A service with no server (calls will fail)
- An action with no server (goals can't be sent)
- A scope endpoint group that is empty

### Proposed Rules

**Topics:**

| Situation | Severity | Rationale |
|-----------|----------|-----------|
| 0 pub, ≥1 sub | Warning | No data source — may be wired by parent |
| ≥1 pub, 0 sub | Warning | Output unused — may be an export |
| 0 pub, 0 sub | Remove | Empty after filtering — silent cleanup |

**Services:**

| Situation | Severity | Rationale |
|-----------|----------|-----------|
| 0 server, ≥1 client | Error | Service calls will fail |
| ≥1 server, 0 client | Warning | Server unused — may be an export |
| 0 server, 0 client | Remove | Empty after filtering — silent cleanup |

**Actions:**

| Situation | Severity | Rationale |
|-----------|----------|-----------|
| 0 server, ≥1 client | Error | Goals can't be processed |
| ≥1 server, 0 client | Warning | Action server unused — may be an export |
| 0 server, 0 client | Remove | Empty after filtering — silent cleanup |

**Scope endpoint groups:**

| Situation | Action |
|-----------|--------|
| Group non-empty | Keep |
| Group empty (all `?` members filtered) | Remove group |

**Parent wiring:**

When a parent references `child/group` and the group was removed:
- If parent ref has `?` → silently dropped
- If parent ref has no `?` → error (required group missing)

### "May be wired by parent" Cases

Single-scope manifests always have unwired endpoints — that's the purpose
of the scope interface (top-level `pub:`/`sub:`/etc.). The parent manifest
completes the wiring. Warnings for "no pub" or "no sub" on a topic only
apply to **fully intra-scope** topics (both pub and sub declared within
the same manifest). Topics that reference scope interface groups are
expected to be partially wired.

### Implementation

1. Add `dangling_entity` validation rule that runs **after** condition
   filtering and `cleanup_dangling_refs`:
   - Check topics: 0 pub or 0 sub → warning; 0 pub + 0 sub → remove
   - Check services: 0 server → error; 0 server + 0 client → remove
   - Check actions: same as services
2. Add empty group removal to `cleanup_dangling_refs()`
3. Tests for each case

### Implementation — Done (Phase 33.4)

`dangling_entity.rs` (13th rule). Empty entity removal added to
`cleanup_dangling_refs()` in `cond.rs`. Scope group `?` cleanup + empty
group removal for all 6 scope fields. 6 new tests.

---

## 14. Arg Types, Choices, and Satisfiability Checking

### Problem

Manifest args are untyped strings. The checker can't distinguish a boolean
flag (`launch_collision_detector`) from a topic name (`input_objects_topic_name`)
from an enum (`pose_source`). Without type information:

1. The checker can't enumerate valid configurations
2. Typos in boolean args (`"True"` vs `"true"`) go undetected
3. Variant consistency can't be verified — there's no way to prove that
   every valid arg combination produces a manifest without dangling entities

### Prior Art: ROS 2 Launch

ROS 2 `DeclareLaunchArgument` already supports typed constraints:

```xml
<arg name="prediction_model_type" default="map_based">
  <choice value="map_based"/>
  <choice value="simpl"/>
</arg>
```

Python API: `DeclareLaunchArgument('x', choices=['ndt', 'eagleye'])`.

At launch time, values not in the choice list produce an error. Autoware
uses `<choice>` in perception launch files (e.g., `prediction.launch.xml`).

### Proposed Solution: Arg Type Declarations

Extend the manifest `args:` syntax with optional type/choice constraints:

```yaml
args:
  # Boolean arg — shorthand for choices: ["true", "false"]
  launch_collision_detector:
    type: bool

  # Enum arg — explicit valid values
  pose_source:
    choices: [ndt, eagleye]

  # Free string — no constraint (default behavior)
  input_objects_topic_name:

  # Boolean with shorthand (most common case)
  use_multithread:
    type: bool
```

**Shorthand for simple cases** — bare name means free string (current behavior):

```yaml
args:
  input_objects_topic_name:       # free string
  launch_collision_detector:      # free string (unless type: bool declared)
```

**Full form:**

```yaml
args:
  pose_source:
    choices: [ndt, eagleye]       # enum — only these values allowed
  launch_collision_detector:
    type: bool                    # boolean — only "true" or "false"
  input_objects_topic_name:
    type: string                  # explicit free string (default)
```

### Satisfiability Checking

With typed args, the checker can **enumerate all valid configurations** and
verify that no combination produces dangling entities.

**Two-tier approach:**

#### Tier 1: Enumeration (small arg spaces)

For manifests with few finite-domain args, enumerate all valid
configurations:

1. Collect all args with finite value sets:
   - `type: bool` → `{"true", "false"}`
   - `choices: [a, b, c]` → `{"a", "b", "c"}`
   - Free string → skip (infinite domain)

2. Compute Cartesian product. For each configuration:
   - Substitute args → evaluate conditions → filter manifest
   - Run dangling entity checks (Issue #13)
   - If any configuration produces an error → report the specific arg
     values that trigger the problem

3. If all pass → manifest is **variant-complete**.

Practical for ≤15 finite-domain args (2^15 = 32K configs, <1s).

#### Tier 2: Z3 SMT Solver (large arg spaces)

For manifests with many finite-domain args (>15) or complex conditions,
use Z3 to find violating configurations without enumerating all of them.

**Z3 Rust crate**: `z3 = "0.12"` (MIT, FFI to libz3).
Install: `apt install libz3-dev` or use `features = ["vendored"]`.

**Encoding:**

1. For each finite-domain arg, create a Z3 **enum sort**:

   ```rust
   // type: bool → enum {"true", "false"}
   let (bool_sort, bool_consts, _) = Sort::enumeration(&ctx,
       "launch_collision_detector".into(),
       &["true".into(), "false".into()]);

   // choices: [ndt, eagleye] → enum {"ndt", "eagleye"}
   let (pose_sort, pose_consts, _) = Sort::enumeration(&ctx,
       "pose_source".into(),
       &["ndt".into(), "eagleye".into()]);
   ```

2. For each `if:` condition, translate to Z3 constraints:

   ```
   if: $(var x) == 'ndt'   →   z3_x._eq(&ndt_const)
   if: $(var x)             →   z3_x._eq(&true_const)   [boolean shorthand]
   unless: $(var x)         →   z3_x._eq(&true_const).not()
   and / or                 →   z3 Bool::and / Bool::or
   ```

3. For each potential dangling entity (topic with all `?` publishers,
   service with all `?` servers), construct a Z3 query:

   "Is there an arg assignment where ALL publishers of this topic are
   filtered out?"

   ```rust
   // Topic has pub: [ndt_node/pose?, eagleye_node/pose?]
   // ndt_node has if: $(var pose_source) == 'ndt'
   // eagleye_node has if: $(var pose_source) == 'eagleye'
   //
   // Dangling = both filtered = neither condition is true
   let ndt_active = pose_var._eq(&ndt_const);
   let eagleye_active = pose_var._eq(&eagleye_const);
   let topic_dangling = ndt_active.not().and(&[&eagleye_active.not()]);

   solver.push();
   solver.assert(&topic_dangling);
   if solver.check() == SatResult::Sat {
       // Found a violating assignment
       let model = solver.get_model().unwrap();
       // Extract the arg values that cause the problem
   }
   solver.pop(1);
   ```

4. If Z3 returns UNSAT → no valid arg combination causes the dangling.
   If SAT → report the counterexample.

**Why Z3 over brute force:**

- Z3's enum sorts are internally small integers — constraint solving is
  extremely fast (microseconds per query).
- Complex conditions with `and`/`or`/`!=` map directly to Z3 boolean
  algebra — no need to evaluate the manifest for each combination.
- Z3 finds counterexamples directly — no enumeration of the 2^N space.
- For conditions that reference free-string args (`$(var topic_name) == ...`),
  Z3 can still reason about the boolean structure without knowing the
  string value.

**Invalid value detection:**

Z3 also catches conditions that are always false on a typed arg:

```yaml
args:
  launch_feature:
    type: bool

nodes:
  bad_node:
    if: $(var launch_feature) == 'wtf'    # always false — "wtf" ∉ {"true", "false"}
```

After substitution, the condition becomes `"true" == 'wtf'` or
`"false" == 'wtf'` — both false. Z3 proves `bad_node` is **unreachable**:
no valid arg value makes its condition true. The checker can report this
as a warning: "node `bad_node` is unreachable — condition is always false
for all valid values of `launch_feature`."

**Dependency consideration:**

Z3 adds a native dependency (`libz3-dev`). Options:
- Required: `apt install libz3-dev` — simple on Ubuntu
- Vendored: `z3 = { version = "0.12", features = ["vendored"] }` — builds
  from source, ~5-10 min first build, needs CMake + C++17
- Feature-gated: `z3` feature flag on the checker crate — opt-in, fallback
  to Tier 1 enumeration when Z3 is not available

**Recommendation**: feature-gate Z3 behind `--features z3` on the checker
crate. Tier 1 enumeration is the default (no native deps). Z3 is opt-in
for users who need it.

### Example: Variant Consistency

```yaml
args:
  pose_source:
    choices: [ndt, eagleye]

nodes:
  ndt_node:
    if: $(var pose_source) == 'ndt'
    pub: [pose]
  eagleye_node:
    if: $(var pose_source) == 'eagleye'
    pub: [pose]

topics:
  localization_pose:
    type: geometry_msgs/msg/PoseStamped
    pub:
      - ndt_node/pose?
      - eagleye_node/pose?
    sub: [controller/pose_input]
```

The checker enumerates `{ndt, eagleye}`:
- `pose_source=ndt` → `ndt_node` active, `eagleye_node` filtered.
  Topic has 1 pub. ✓
- `pose_source=eagleye` → `eagleye_node` active, `ndt_node` filtered.
  Topic has 1 pub. ✓

Both configurations have ≥1 publisher. Manifest is variant-complete.

If someone adds `pose_source: choices: [ndt, eagleye, gnss]` but forgets
a `gnss_node`:
- `pose_source=gnss` → both nodes filtered. Topic has 0 pub. ✗
- Checker reports: "with `pose_source=gnss`, topic `localization_pose`
  has no publishers"

### Implementation — Done (Phase 33.3 + 33.5)

**Arg types (33.3)**:

`ArgDecl` enum with three variants: `String` (free), `Bool` ("true"/"false"),
`Choices(Vec<String>)` (enum). `valid_values()` method returns the finite
domain. `resolve_args()` validates values against type constraints.

**Z3 satisfiability (33.5)**:

`satisfiability.rs` (14th rule) uses `z3 = "0.12"`:

1. Creates Z3 enum sorts per finite-domain arg
2. Translates `if:`/`unless:` conditions to Z3 constraints (`and`, `or`,
   `==`, `!=`, parenthesized expressions, boolean shorthand)
3. For topics with all-optional publishers: asserts all conditional nodes
   inactive, checks SAT → reports specific arg values
4. Same for services/actions with all-optional servers
5. Unreachable node detection: builds constraint from condition, checks
   if satisfiable. Unsat → warning. Invalid domain reference → also
   unreachable.

Z3 is a required dependency (not feature-gated) — it's fast and
commonly available via `apt install libz3-dev`.

---

## 15. `?` Suffix Breaks Standard YAML Parsers

### Problem

The `?` suffix on optional endpoint references (design issue #9) works
in yaml-rust2 (the Rust parser) but breaks standard YAML parsers when
used inside **flow sequences** (`[...]`):

```yaml
# Works in yaml-rust2, fails in PyYAML / yamllint / yq / IDE linting
sub: [control_validator/control_cmd?]

# Works everywhere
sub:
  - control_validator/control_cmd?
```

`?` is a YAML mapping key indicator. In flow context, `[foo?]` is
ambiguous — is it a sequence item `foo?` or the start of a mapping key?
yaml-rust2 treats it as a plain scalar; strict parsers reject it.

### Impact

- IDEs with YAML validation show errors on every `?` ref in flow form
- `yamllint`, `yq`, `python3 -c 'yaml.safe_load(...)'` all fail
- Users can't validate manifests with standard YAML tooling
- 15+ occurrences in `control.yaml` alone (Autoware contracts)

### Options

| Option | Pros | Cons |
|--------|------|------|
| A. Quote flow refs | `["node/ep?"]` — standard compliant | Verbose, easy to forget |
| B. Block form only for `?` | Always use `- node/ep?` | Inconsistent formatting |
| C. Different marker | `node/ep~` or `node/ep!optional` | Breaking change, less intuitive |
| D. Accept it | Works with our parser | Breaks ecosystem tools |

**Resolution**: Eliminated `?` entirely. Optionality is now inferred
from node conditions — refs to nodes with `if:`/`unless:` are
automatically treated as optional during `filter_manifest()`. The
`optional-ref` validation rule was removed (13 → 13 rules). All
YAML is now standard-compliant.

---

## 16. Satisfiability Rule Should Skip State-Only Subscribers

### Problem

The `satisfiability` rule flags topics where all publishers are optional
and at least one subscriber exists. However, when the only subscribers
are `state: true` (polled, not causal), having 0 publishers is harmless —
the subscriber just reads nothing.

Example: a `twist_data` topic with optional publishers (eagleye, twist_estimator)
and a single `state: true` subscriber (ekf/twist_input). When both
publishers are filtered, the topic has 0 pub + 1 state sub. The rule
reports an error, but the system works fine — EKF just has no twist data.

### Fix — Done

The satisfiability rule now resolves each subscriber ref (`node/endpoint`)
to its endpoint properties in the node declaration. If every subscriber
is `state: true` and none are `required: true`, the topic is skipped —
0 publishers is harmless for polled, non-required subscribers.

The four combinations:

| `state` | `required` | 0 publishers OK?                         |
|---------|------------|------------------------------------------|
| false   | false      | No — causal callback expects messages    |
| false   | true       | No — causal + must have initial data     |
| true    | false      | **Yes** — polls nothing, node works fine |
| true    | true       | No — node needs at least one message     |

3 new tests: state-only skip, state+required still errors, mixed
state+causal still checked.

---

## 17. Cross-Scope Service Wiring Has No Suppression Mechanism

### Problem

The unified scope interface (design issue #12) added scope-level `srv:`
and `cli:` groups. However, cross-scope service wiring still has no
enforcement path:

1. `mrm_handler` declares `cli:` with 3 cross-scope service clients
2. `mrm_comfortable_stop_operator` declares `srv:` with 1 server
3. No parent manifest exists to wire them in a `services:` section
4. The `service-wiring` rule warns about orphan `cli:` endpoints
5. There is no way to mark these as "expected cross-scope" to suppress

This creates **permanent noise** in check output — 3+ warnings per run
that can never be resolved without a parent manifest.

### Options

| Option | Effort | Description |
|--------|--------|-------------|
| A. `# nolint: service-wiring` | Small | Inline suppression comment |
| B. `--suppress` CLI flag | Small | Suppress specific rules/paths |
| C. Cross-manifest wiring | Large | Checker loads multiple manifests and wires across scopes |
| D. Accept the noise | None | Document as known limitation |

**Recommendation**: Option A or B. A lightweight suppression mechanism
would benefit other expected-warning scenarios too (e.g., unwired scope
interface endpoints).

---

## 18. CLI Should Support Per-Rule Filtering

### Problem

The `play_launch check` CLI runs all 14 validation rules and outputs
all diagnostics. Users interested in a specific rule (e.g., satisfiability
results only) must grep the output. The `just check-sat` recipe in
autoware-contract does exactly this — a shell grep wrapper.

### Proposed Solution

Add `--rule <RULE_ID>` filter to the CLI:

```bash
# Show only satisfiability results
play_launch check --rule satisfiability --manifest-dir . autoware_launch planning_simulator.launch.xml

# Show only errors from specific rules
play_launch check --rule dangling-entity --rule optional-ref --manifest-dir . ...
```

This is a small CLI change — filter `CheckResult.diagnostics` by
`rule_id` before rendering.

---

## Summary

| Issue                              | Type                    | Effort  | Status                           |
|------------------------------------|-------------------------|---------|----------------------------------|
| Args + substitutions + `if:` (1-3) | Format + types + loader | Medium  | Done (Phase 32)                  |
| Service wiring check (2)           | New static rule         | Small   | Done (Phase 32.5)                |
| Service `max_response_ms` (2)      | Format + runtime        | Large   | Done (format); runtime deferred  |
| Stale descriptions (3a-3c)         | Doc fix                 | Trivial | Done (Phase 32.1)                |
| Missing `cli:` docs (4)            | Doc fix                 | Trivial | Done (Phase 32.1)                |
| Endpoint name clarification (5)    | Doc fix                 | Small   | Done (Phase 32.1)                |
| ~~Import topic mapping (6)~~       | ~~Dropped~~             | —       | Parent topics: handles this      |
| Global topics wiring (7)           | Design decision         | Small   | Kept as documentation only       |
| External include naming (8)        | Doc fix                 | Trivial | Done (Phase 32.1)                |
| ~~Optional refs `?` suffix (9)~~   | ~~Format + rule~~       | Small   | Superseded by #15 — `?` removed, optionality inferred |
| ~~Service imports/exports (10)~~   | ~~Superseded by #12~~   | —       | Replaced by unified scope interface |
| Parser scope.args incomplete (11)  | Parser bug              | Small   | Done (Phase 33.1) — `update_args()` at all 6 include paths |
| Unified scope interface (12)       | Format redesign         | Medium  | Done (Phase 33.2) — top-level `pub:`/`sub:`/`srv:`/`cli:` |
| Dangling entity checks (13)        | New validation rule     | Small   | Done (Phase 33.4) — `dangling-entity` rule |
| Arg types + satisfiability (14)    | Format + analysis       | Medium  | Done (Phase 33.3 + 33.5) — `ArgDecl` types + Z3 satisfiability |
| ~~`?` in flow sequences (15)~~    | ~~YAML compat~~         | Small   | Done — `?` removed, optionality inferred from node conditions |
| State-only sub skip (16)          | Satisfiability refinement| Small   | Done — skip topics where all subs are state-only |
| Cross-scope suppression (17)      | UX / CLI                | Small   | Open — no way to suppress expected cross-scope warnings |
| Per-rule CLI filter (18)          | UX / CLI                | Small   | Open — `--rule <ID>` flag for focused output |
