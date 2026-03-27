# Design Issues

Open design questions for the manifest format, with proposed solutions.

---

## 1. Conditional Declarations (REP-149 Style)

### Problem

Several Autoware nodes use launch argument substitutions for topic names:
```xml
<remap from="~/input/dynamic_objects" to="$(var input_objects_topic_name)"/>
<remap from="~/input/no_ground_pointcloud" to="$(var input_pointcloud_topic_name)"/>
```

The resolved topic name depends on which arguments are passed at launch time.
Today the manifest uses generic import names like `predicted_objects` and
`pointcloud`, which don't capture the actual wiring. The `play_launch check`
command resolves these via the scope table, but the manifest itself can't
express the conditional mapping.

A bigger issue: some launch files conditionally include entire node groups
based on arguments (e.g., `use_pointcloud_container`, `use_multi_thread`).
The manifest currently has no way to express "this node only exists when
arg X is true."

### Proposed Solution: REP-149 Conditions

[REP-149](https://www.ros.org/reps/rep-0149.html) introduced `condition`
attributes for `package.xml` dependencies:

```xml
<depend condition="$ROS_VERSION == 2">rclcpp</depend>
```

The condition grammar supports:
- Variable references: `$VAR_NAME` (substituted as strings)
- Comparison: `==`, `!=`, `<`, `<=`, `>`, `>=` (string comparison)
- Logic: `and`, `or`, parentheses
- Literals: bare words, quoted strings

We adopt the same grammar for manifest YAML with a `condition` field:

```yaml
nodes:
  pointcloud_container:
    condition: "$use_pointcloud_container == 'true'"
    pub: [output]

topics:
  no_ground_points:
    type: sensor_msgs/msg/PointCloud2
    condition: "$use_pointcloud_container == 'true'"
    pub: [cropbox/output]
    sub: [ground_filter/input]
```

### Evaluation

Conditions are evaluated at **check time** using the scope's `args` from the
launch tree. The scope table in `record.json` already captures all launch
arguments per scope:

```json
{
  "id": 88,
  "origin": {"pkg": "tier4_planning_launch", "file": "motion_planning.launch.xml"},
  "ns": "/planning/.../motion_planning",
  "args": {
    "input_objects_topic_name": "/perception/object_recognition/objects",
    "input_pointcloud_topic_name": "/perception/obstacle_segmentation/pointcloud",
    ...
  }
}
```

The manifest loader resolves `$input_objects_topic_name` from the scope's
args, then evaluates condition expressions. Entities where the condition is
false are excluded from checking.

### Variable Topic Names

For topic name substitution specifically, we can support `$var` in topic
fields:

```yaml
imports:
  predicted_objects:
    - motion_velocity_planner/predicted_objects   # actual name: $input_objects_topic_name
  pointcloud:
    - motion_velocity_planner/pointcloud          # actual name: $input_pointcloud_topic_name
```

The `$var` references are resolved from the scope's args during manifest
loading, before static checks run. This is strictly more expressive than
the current generic names — the manifest documents the intent, and the
checker verifies the actual wiring.

### Implementation Path

1. Add `condition: Option<String>` to `NodeDecl`, `TopicDecl`, `ServiceDecl`,
   `PathDecl`, `IncludeDecl` in the types crate
2. Add a condition evaluator in the types crate (parse REP-149 grammar,
   substitute variables, evaluate boolean expression)
3. In `manifest_loader.rs`, pass the scope's `args` to the evaluator before
   constructing the resolved index — filter out entities with false conditions
4. In the checker, skip filtered entities

### Scope

This is a manifest format extension. It doesn't change the static check rules —
it only changes which entities are visible to them.

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

## Summary

| Issue                     | Type                             | Effort | Priority                                          |
|---------------------------|----------------------------------|--------|---------------------------------------------------|
| REP-149 conditions        | Format extension + loader change | Medium | High — enables accurate variable topic checking   |
| Service wiring check      | New static rule                  | Small  | Medium — catches missing service providers        |
| Service latency contracts | Format + runtime                 | Large  | Low — no ROS 2 native support, needs interception |
