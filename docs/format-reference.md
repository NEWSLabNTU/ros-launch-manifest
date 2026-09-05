# Contract File — Format Reference

**Generated from `types/src/field_table.rs`. Do not edit by hand** — run `UPDATE_FORMAT_REFERENCE=1 cargo test -p ros-launch-manifest-types` after changing the table.

Every key below is accepted in the context that heads its section, and **a key that is not listed is a parse error**. Contexts whose keys are chosen by the author — `nodes:`, `topics:`, `services:`, `actions:`, `includes:`, `paths:`, `args:`, `external_topics:`, and the endpoint maps under `pub:`/`sub:`/`srv:`/`cli:` — have no section here, because no allowlist applies to them.

## `manifest`

| key | status | meaning |
|---|---|---|
| `version` |  | Manifest format version. Absent means 1. |
| `args` |  | Arguments this manifest requires, supplied by the launch scope. |
| `exclude_patterns` |  | Node-name globs this manifest deliberately does not describe. |
| `nodes` |  | Node declarations, keyed by bare node name. |
| `topics` |  | Topic declarations, keyed by topic name. |
| `services` |  | Service declarations, keyed by service name. |
| `actions` |  | Action declarations, keyed by action name. |
| `includes` |  | Child manifests, either a file reference or an inline manifest. |
| `paths` |  | Scope paths: end-to-end requirements naming two topics and a budget. |
| `external_topics` |  | Topics produced or consumed outside the loaded manifest tree. |
| `chains` | **removed** | Removed in phase 68 — state the requirement as a scope path and let the route be derived. |

## `nodes.<name>`

| key | status | meaning |
|---|---|---|
| `if` |  | Include this node only when the condition holds. |
| `unless` |  | Include this node unless the condition holds. |
| `lifecycle` |  | True for a ROS 2 managed node; runtime monitors skip checks until it is Active. |
| `pub` |  | Publisher endpoints, keyed by endpoint name. |
| `sub` |  | Subscriber endpoints, keyed by endpoint name. |
| `srv` |  | Service server endpoints, keyed by endpoint name. |
| `cli` |  | Service client endpoints, keyed by endpoint name. |
| `paths` |  | This node's internal paths, keyed by path name. |
| `criticality` |  | Platform-agnostic scheduling criticality hint: high | medium | low. |
| `concurrency` |  | Which of this node's paths may NOT run concurrently. Absent means all of them serialize. |

## `pub/sub/cli.<endpoint>`

| key | status | meaning |
|---|---|---|
| `min_rate_hz` |  | Lower bound on this endpoint's rate. |
| `max_rate_hz` |  | Upper bound on this endpoint's rate. |
| `max_age` |  | Subscriber: maximum data age at receive (now - header.stamp). |
| `max_age_ms` | deprecated — use `max_age` | Deprecated spelling. |
| `state` |  | Subscriber: read-latest rather than causal. |
| `required` |  | Subscriber: this endpoint must be connected. |
| `qos` |  | QoS overrides for this endpoint. |
| `max_transport` |  | Transport latency budget for this endpoint. |
| `max_transport_ms` | deprecated — use `max_transport` | Deprecated spelling. |
| `buffer` |  | Buffering discriminator for a state subscriber: latest | queue. |
| `jitter` | **removed** | Removed in phase 68 — jitter is a property of a route, not one publisher. Use `max_jitter` on a path. |
| `jitter_ms` | **removed** | Removed in phase 68 — see `jitter`. |

## `srv.<endpoint>`

| key | status | meaning |
|---|---|---|
| `max_response` |  | Deadline for answering a request on this service. |
| `max_response_ms` | deprecated — use `max_response` | Deprecated spelling. |

## `topics.<name>`

| key | status | meaning |
|---|---|---|
| `if` |  | Declare this topic only when the condition holds. |
| `unless` |  | Declare this topic unless the condition holds. |
| `type` |  | ROS message type. Required. |
| `pub` |  | Publishing endpoints, as `node/endpoint`. |
| `sub` |  | Subscribing endpoints, as `node/endpoint`. |
| `qos` |  | Topic-level QoS, overridable per endpoint. |
| `rate_hz` |  | Publication rate. Derivable from the timers that drive it — see `derivable-rate`. |
| `max_transport` |  | Transport latency budget for every subscriber of this topic. |
| `max_transport_ms` | deprecated — use `max_transport` | Deprecated spelling. |
| `drop` |  | Permitted message loss on this topic. |
| `external` |  | Mark one side of this topic as provided by an external system. |

## `external_topics.<name>`

| key | status | meaning |
|---|---|---|
| `side` |  | Which side is external: pub | sub | both. |
| `external` | deprecated — use `side` | Deprecated spelling. |
| `type` |  | ROS message type, when known. |
| `qos` |  | QoS the external side uses, when known. |

## `services.<name>`

| key | status | meaning |
|---|---|---|
| `if` |  | Declare this service only when the condition holds. |
| `unless` |  | Declare this service unless the condition holds. |
| `type` |  | ROS service type. |
| `server` |  | Server endpoints, as `node/endpoint`. |
| `client` |  | Client endpoints, as `node/endpoint`. |
| `external` |  | Mark one side of this service as provided by an external system: server | client | both. |

## `actions.<name>`

| key | status | meaning |
|---|---|---|
| `if` |  | Declare this action only when the condition holds. |
| `unless` |  | Declare this action unless the condition holds. |
| `type` |  | ROS action type. |
| `server` |  | Server endpoints, as `node/endpoint`. |
| `client` |  | Client endpoints, as `node/endpoint`. |
| `external` |  | Mark one side of this action as provided by an external system: server | client | both. |

## `includes.<name>`

| key | status | meaning |
|---|---|---|
| `manifest` |  | Path to the included manifest file. |

## `args.<name>`

| key | status | meaning |
|---|---|---|
| `type` |  | Argument type: bool, or omitted for a free string. |
| `choices` |  | Permitted values. Consulted only when `type` is absent. |

## `paths.<name>`

| key | status | meaning |
|---|---|---|
| `if` |  | Declare this path only when the condition holds. |
| `unless` |  | Declare this path unless the condition holds. |
| `input` |  | Legacy trigger spelling. Prefer `trigger: { input: [...] }`. |
| `output` |  | Endpoints (node path) or topics (scope path) this path produces. |
| `max_latency` |  | Latency budget for this path. |
| `max_latency_ms` | deprecated — use `max_latency` | Deprecated spelling. |
| `correlation` |  | How multiple inputs are correlated into one output. |
| `tolerance` |  | Correlation tolerance. |
| `tolerance_ms` | deprecated — use `tolerance` | Deprecated spelling. |
| `drop` |  | Permitted message loss along this path. |
| `trigger` |  | What causes this path's output: timer | input | once | spontaneous. |
| `sync` |  | Fan-in synchronization policy for an input trigger with two or more endpoints. |
| `max_jitter` |  | Permitted variation in this path's latency. |
| `min_latency` |  | Best-case latency. Exists so that `max_jitter` is falsifiable. |
| `miss` |  | What a missed deadline costs and what to do about it. |
| `segments` | **removed** | Removed in phase 68 with `chains:` — a written route is a second copy of the graph. |

## `trigger`

| key | status | meaning |
|---|---|---|
| `timer` |  | Periodic self-clocked callback. |
| `input` |  | Output caused by these input endpoints or topics. |

## `trigger.timer`

| key | status | meaning |
|---|---|---|
| `rate_hz` |  | Timer rate. Required, and must be greater than zero. |

## `sync`

| key | status | meaning |
|---|---|---|
| `policy` |  | Fan-in policy. Required. |
| `max_interval` |  | Widest permitted spread between matched inputs. |
| `max_interval_ms` | deprecated — use `max_interval` | Deprecated spelling. |
| `timeout` |  | How long to wait for the remaining inputs. |
| `timeout_ms` | deprecated — use `timeout` | Deprecated spelling. |

## `drop`

| key | status | meaning |
|---|---|---|
| `max_count` |  | Permitted losses over a window, as `N / W`. |
| `max_consecutive` |  | Permitted consecutive losses. |

## `qos`

| key | status | meaning |
|---|---|---|
| `reliability` |  | reliable | best_effort. |
| `durability` |  | volatile | transient_local. |
| `depth` |  | History depth. |
| `history` |  | keep_last | keep_all. |
| `lifespan` |  | How long a message stays valid after publication. |
| `lifespan_ms` | deprecated — use `lifespan` | Deprecated spelling. |
| `liveliness` |  | automatic | manual_by_topic. |
| `deadline` |  | QoS deadline between consecutive messages. |
| `lease_duration` |  | Liveliness lease duration. |

## `miss`

| key | status | meaning |
|---|---|---|
| `tolerate` |  | Permitted misses over a window, as `N / W`. |
| `consecutive` |  | Permitted consecutive misses. |
| `action` |  | What to do on a miss: continue | skip_next | abort. |

## `concurrency`

| key | status | meaning |
|---|---|---|
| `exclusive` |  | Groups of path names that may not run at the same time. |

