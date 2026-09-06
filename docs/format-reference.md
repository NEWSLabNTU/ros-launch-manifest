# Contract File — Format Reference

**Generated from `types/src/field_table.rs`. Do not edit by hand** — run `UPDATE_FORMAT_REFERENCE=1 cargo test -p ros-launch-manifest-types` after changing the table.

Every key below is accepted in the context that heads its section, and **a key that is not listed is a parse error**. Contexts whose keys are chosen by the author — `nodes:`, `topics:`, `services:`, `actions:`, `includes:`, `paths:`, `args:`, `external_topics:`, and the endpoint maps under `pub:`/`sub:`/`srv:`/`cli:` — have no section here, because no allowlist applies to them.

The **kind** column is the rule of `contract-primitives.md` as data: a *fact* is what the code does, a *requirement* is what it must achieve, *meta* is structure, and a **consequence** is computable from the facts — a second copy the graph already knows, kept only where the graph has no answer (an external source). `scripts/derivation_census.py` counts how often each consequence agrees.

## `manifest`

| key | kind | status | meaning |
|---|---|---|---|
| `version` | meta |  | Manifest format version. Absent means 1. |
| `args` | meta |  | Arguments this manifest requires, supplied by the launch scope. |
| `exclude_patterns` |  | **removed** | Removed in phase 70 — it was parsed and read by nothing, so it excluded nothing. Mark an expected-absent side with `external:`. |
| `nodes` | meta |  | Node declarations, keyed by bare node name. |
| `topics` | meta |  | Topic declarations, keyed by topic name. |
| `services` | meta |  | Service declarations, keyed by service name. |
| `actions` | meta |  | Action declarations, keyed by action name. |
| `includes` | meta |  | Child manifests, either a file reference or an inline manifest. |
| `paths` | meta |  | Scope paths: end-to-end requirements naming two topics and a budget. |
| `hazards` | meta |  | Hazards: what is watched, how long the system has, and which path reaches the safe state. |
| `functions` | meta |  | Named guard groups: what a set of topics together provides. A mode requires functions; a hazard may guard one by name. |
| `modes` | meta |  | Operational modes: what each requires, and the ordered ladder to fall to when it is lost. |
| `severity_levels` | meta |  | The severity scale hazards draw from, ascending; the first entry derives no criticality. Default: QM, ASIL_A..ASIL_D. |
| `external_topics` | meta |  | Topics produced or consumed outside the loaded manifest tree. |
| `chains` |  | **removed** | Removed in phase 68 — state the requirement as a scope path and let the route be derived. |

## `nodes.<name>`

| key | kind | status | meaning |
|---|---|---|---|
| `if` | meta |  | Include this node only when the condition holds. |
| `unless` | meta |  | Include this node unless the condition holds. |
| `lifecycle` | fact |  | True for a ROS 2 managed node; runtime monitors skip checks until it is Active. |
| `pub` | meta |  | Publisher endpoints, keyed by endpoint name. |
| `sub` | meta |  | Subscriber endpoints, keyed by endpoint name. |
| `srv` | meta |  | Service server endpoints, keyed by endpoint name. |
| `cli` | meta |  | Service client endpoints, keyed by endpoint name. |
| `paths` | meta |  | This node's internal paths, keyed by path name. |
| `criticality` | **consequence** |  | Scheduling criticality: high | medium | low. A CONSEQUENCE of the hazards a node guards, reacts for, or feeds (phase 72); the label stands only where no hazard reaches the node. |
| `concurrency` | fact |  | Which of this node's paths may NOT run concurrently. Absent means all of them serialize. |

## `pub/sub/cli.<endpoint>`

| key | kind | status | meaning |
|---|---|---|---|
| `min_rate_hz` | fact (pub) / requirement (sub) |  | Lower bound on this endpoint's rate. |
| `max_rate_hz` | fact (pub) / requirement (sub) |  | Upper bound on this endpoint's rate. |
| `max_age` | requirement |  | Subscriber: maximum data age at receive (now - header.stamp). |
| `max_age_ms` |  | **removed** | Removed in phase 70 — write `max_age: <n>ms` (or ns/us/s). The unit in a NAME is what lets a value be 1000x wrong and still parse. |
| `state` | fact |  | Subscriber: read-latest rather than causal. |
| `required` | fact |  | Subscriber: this endpoint must be connected. |
| `qos` | fact |  | QoS overrides for this endpoint. |
| `max_transport` | requirement |  | Transport latency budget for this endpoint. |
| `max_transport_ms` |  | **removed** | Removed in phase 70 — write `max_transport: <n>ms` (or ns/us/s). The unit in a NAME is what lets a value be 1000x wrong and still parse. |
| `on_violation` | requirement |  | The reaction this subscriber owes when its assumption is violated. |
| `buffer` | fact |  | Buffering discriminator for a state subscriber: latest | queue. |
| `jitter` |  | **removed** | Removed in phase 68 — jitter is a property of a route, not one publisher. Use `max_jitter` on a path. |
| `jitter_ms` |  | **removed** | Removed in phase 68 — see `jitter`. |

## `srv.<endpoint>`

| key | kind | status | meaning |
|---|---|---|---|
| `max_response` | requirement |  | Deadline for answering a request on this service. |
| `max_response_ms` |  | **removed** | Removed in phase 70 — write `max_response: <n>ms` (or ns/us/s). The unit in a NAME is what lets a value be 1000x wrong and still parse. |

## `topics.<name>`

| key | kind | status | meaning |
|---|---|---|---|
| `if` | meta |  | Declare this topic only when the condition holds. |
| `unless` | meta |  | Declare this topic unless the condition holds. |
| `type` | fact |  | ROS message type. Required. |
| `pub` | meta |  | Publishing endpoints, as `node/endpoint`. |
| `sub` | meta |  | Subscribing endpoints, as `node/endpoint`. |
| `qos` | fact |  | Topic-level QoS, overridable per endpoint. |
| `rate_hz` | **consequence** |  | Publication rate. Derivable from the timers that drive it — see `derivable-rate`. |
| `max_transport` | requirement |  | Transport latency budget for every subscriber of this topic. |
| `max_transport_ms` |  | **removed** | Removed in phase 70 — write `max_transport: <n>ms` (or ns/us/s). The unit in a NAME is what lets a value be 1000x wrong and still parse. |
| `drop` | requirement |  | Permitted message loss on this topic. |
| `external` | meta |  | Mark one side of this topic as provided by an external system. |

## `external_topics.<name>`

| key | kind | status | meaning |
|---|---|---|---|
| `side` | meta |  | Which side is external: pub | sub | both. |
| `external` |  | deprecated — use `side` | Deprecated spelling. |
| `type` | fact |  | ROS message type, when known. |
| `qos` | fact |  | QoS the external side uses, when known. |

## `services.<name>`

| key | kind | status | meaning |
|---|---|---|---|
| `if` | meta |  | Declare this service only when the condition holds. |
| `unless` | meta |  | Declare this service unless the condition holds. |
| `type` | fact |  | ROS service type. |
| `server` | meta |  | Server endpoints, as `node/endpoint`. |
| `client` | meta |  | Client endpoints, as `node/endpoint`. |
| `external` | meta |  | Mark one side of this service as provided by an external system: server | client | both. |

## `actions.<name>`

| key | kind | status | meaning |
|---|---|---|---|
| `if` | meta |  | Declare this action only when the condition holds. |
| `unless` | meta |  | Declare this action unless the condition holds. |
| `type` | fact |  | ROS action type. |
| `server` | meta |  | Server endpoints, as `node/endpoint`. |
| `client` | meta |  | Client endpoints, as `node/endpoint`. |
| `external` | meta |  | Mark one side of this action as provided by an external system: server | client | both. |

## `includes.<name>`

| key | kind | status | meaning |
|---|---|---|---|
| `manifest` | meta |  | Path to the included manifest file. |

## `args.<name>`

| key | kind | status | meaning |
|---|---|---|---|
| `type` | meta |  | Argument type: bool, or omitted for a free string. |
| `choices` | meta |  | Permitted values. Consulted only when `type` is absent. |

## `paths.<name>`

| key | kind | status | meaning |
|---|---|---|---|
| `if` | meta |  | Declare this path only when the condition holds. |
| `unless` | meta |  | Declare this path unless the condition holds. |
| `input` | fact |  | Legacy trigger spelling. Prefer `trigger: { input: [...] }`. |
| `output` | fact |  | Endpoints (node path) or topics (scope path) this path produces. |
| `max_latency` | requirement |  | Latency budget for this path. |
| `max_latency_ms` |  | **removed** | Removed in phase 70 — write `max_latency: <n>ms` (or ns/us/s). The unit in a NAME is what lets a value be 1000x wrong and still parse. |
| `correlation` |  | **removed** | Removed in phase 70 — nothing ever read it. State fan-in policy with `sync:`. |
| `tolerance` | fact |  | Max `header.stamp` spread between a fan-in path's inputs still treated as one set. |
| `tolerance_ms` |  | **removed** | Removed in phase 70 — write `tolerance: <n>ms` (or ns/us/s). The unit in a NAME is what lets a value be 1000x wrong and still parse. |
| `drop` | requirement |  | Permitted message loss along this path. |
| `trigger` | fact |  | What causes this path's output: timer | input | once | spontaneous. |
| `sync` | fact |  | Fan-in synchronization policy for an input trigger with two or more endpoints. |
| `max_jitter` | requirement |  | Permitted variation in this path's latency. |
| `min_latency` | fact |  | Best-case latency. Exists so that `max_jitter` is falsifiable. |
| `safe_state` | fact |  | What this path produces when it is a reaction: the endpoint it commands, and the plant's settle time. |
| `miss` | requirement |  | What a missed deadline costs and what to do about it. |
| `segments` |  | **removed** | Removed in phase 68 with `chains:` — a written route is a second copy of the graph. |

## `trigger`

| key | kind | status | meaning |
|---|---|---|---|
| `timer` | fact |  | Periodic self-clocked callback. |
| `input` | fact |  | Output caused by these input endpoints or topics. |

## `trigger.timer`

| key | kind | status | meaning |
|---|---|---|---|
| `rate_hz` | fact |  | Timer rate. Required, and must be greater than zero. |

## `sync`

| key | kind | status | meaning |
|---|---|---|---|
| `policy` | fact |  | Fan-in policy. Required. |
| `max_interval` | fact |  | Widest permitted spread between matched inputs. |
| `max_interval_ms` |  | **removed** | Removed in phase 70 — write `max_interval: <n>ms` (or ns/us/s). The unit in a NAME is what lets a value be 1000x wrong and still parse. |
| `timeout` | fact |  | How long to wait for the remaining inputs. |
| `timeout_ms` |  | **removed** | Removed in phase 70 — write `timeout: <n>ms` (or ns/us/s). The unit in a NAME is what lets a value be 1000x wrong and still parse. |

## `drop`

| key | kind | status | meaning |
|---|---|---|---|
| `max_count` | requirement |  | Permitted losses over a window, as `N / W`. |
| `max_consecutive` | requirement |  | Permitted consecutive losses. |

## `qos`

| key | kind | status | meaning |
|---|---|---|---|
| `reliability` | fact |  | reliable | best_effort. |
| `durability` | fact |  | volatile | transient_local. |
| `depth` | fact |  | History depth. |
| `history` | fact |  | keep_last | keep_all. |
| `lifespan` | fact |  | How long a message stays valid after publication. |
| `lifespan_ms` |  | **removed** | Removed in phase 70 — write `lifespan: <n>ms` (or ns/us/s). The unit in a NAME is what lets a value be 1000x wrong and still parse. |
| `liveliness` | fact |  | automatic | manual_by_topic. |
| `deadline` | fact |  | QoS deadline between consecutive messages. |
| `lease_duration` | fact |  | Liveliness lease: how often a publisher must assert it is alive. Checked pub against sub by `qos-match`. |

## `miss`

| key | kind | status | meaning |
|---|---|---|---|
| `tolerate` | requirement |  | Permitted misses over a window, as `N / W`. |
| `consecutive` | requirement |  | Permitted consecutive misses. |
| `action` | requirement |  | What to do on a miss: continue | skip_next | abort. |

## `concurrency`

| key | kind | status | meaning |
|---|---|---|---|
| `exclusive` | fact |  | Groups of path names that may not run at the same time. |

## `hazards.<name>`

| key | kind | status | meaning |
|---|---|---|---|
| `severity` | requirement |  | Severity label from the HARA (`ASIL_D`, `SIL_3`, …). Consumed, never computed. |
| `description` | meta |  | What the hazardous event is. |
| `guards` | meta |  | Topics watched for the fault. A bare name is one guard; `{ all_of: [...] }` is a redundant set that faults only when every member does. |
| `on` | fact |  | Fault class the guards report: omission | late | loss | reported. |
| `ftti` | requirement |  | Fault-tolerant time interval: fault to hazardous event, absent reaction. Checked by `fault-reaction-budget`. |
| `reaction` | meta |  | The scope path whose route reaches the safe state. |

## `modes.<name>`

| key | kind | status | meaning |
|---|---|---|---|
| `description` | meta |  | What this mode is. |
| `requires` | fact |  | Functions (or bare topics) this mode needs; it is available while every one holds. |
| `fallback` | requirement |  | Modes to fall to, in order, when this one is lost. The last rung is the floor and must require nothing losable. |
| `reaction` | meta |  | The scope path that reaches this mode's safe state. |
| `overrides` | requirement |  | Requirement values that apply IN THIS MODE, pinned over the scalar declared elsewhere. |

## `hazards.<name>.guards[]`

| key | kind | status | meaning |
|---|---|---|---|
| `all_of` | meta |  | Members of a redundant guard set. |

## `on_violation`

| key | kind | status | meaning |
|---|---|---|---|
| `on` | fact |  | Fault classes that trigger the reaction: omission | late | loss | reported. |
| `reaction` | meta |  | A path on this node whose trigger includes the subscription. |
| `within` | requirement |  | This hop's share of the fault reaction time. |
| `mechanism` | fact |  | Where the runtime observer reads the violation: qos (default) | diagnostics | application. |

## `safe_state`

| key | kind | status | meaning |
|---|---|---|---|
| `emits` | fact |  | The endpoint this reaction commands the safe state on. |
| `settle` | fact |  | How long the plant takes to reach the safe state once commanded. Measured, not authored. |

