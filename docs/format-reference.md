# Contract File — Format Reference

**Generated from `types/src/field_table.rs`. Do not edit by hand** — run `UPDATE_FORMAT_REFERENCE=1 cargo test -p ros-launch-manifest-types` after changing the table.

Every key below is accepted in the context that heads its section, and **a key that is not listed is a parse error**. Contexts whose keys are chosen by the author — `nodes:`, `topics:`, `services:`, `actions:`, `includes:`, `paths:`, `args:`, `external_topics:`, the endpoint maps under `pub:`/`sub:`/`srv:`/`cli:`, and the parameter names under a node's `params:` — have no section here, because no allowlist applies to them.

This is the **core** grammar. A contract that `use:`s an extension may also write the keys that extension declares, in `hazards.<name>` and `safe_state.settle`: those are listed per extension in [extension-reference.md](extension-reference.md) (the bundled `vehicle.braking` and the reference extensions), and [extension-guide.md](extension-guide.md) says how to write one.

The **kind** column is the rule of `contract-primitives.md` as data: a *fact* is what the code does, a *requirement* is what it must achieve, *meta* is structure, and a **consequence** is computable from the facts — a second copy the graph already knows, kept only where the graph has no answer (an external source). `scripts/derivation_census.py` counts how often each consequence agrees.

## `manifest`

| key | kind | status | meaning |
|---|---|---|---|
| `version` | meta |  | Manifest format version. Absent means 1. |
| `use` | meta |  | The extensions this contract uses, a map of name -> version requirement (`use: { vehicle.braking: "^1" }`; `^`, `~`, `=` or `>=`). With it, `hazards.<name>` and `safe_state.settle` may carry the keys those extensions declare; without it an unknown key is an error. |
| `rlm` | meta | since v0.1.49 | The grammar release this file needs, `v<major>.<minor>.<patch>` (or `>=` that). Checked before any other key: a checker whose grammar is older refuses the file naming both releases, instead of refusing the first key it does not know as if it were a typo. A checker before v0.1.49 refuses the key itself as unknown. Optional; beside `version`, which is the file format and stays 1. |
| `args` | meta |  | Arguments this manifest requires, supplied by the launch scope. |
| `exclude_patterns` |  | **removed** | Removed in phase 70 — it was parsed and read by nothing, so it excluded nothing. Mark an expected-absent side with `external:`. |
| `nodes` | meta |  | Node declarations, keyed by bare node name. |
| `topics` | meta |  | Topic declarations, keyed by topic name. |
| `services` | meta |  | Service declarations, keyed by service name. |
| `actions` | meta |  | Action declarations, keyed by action name. |
| `includes` | meta |  | Child scopes, each an inline manifest (the file-reference form was removed in v0.1.59). |
| `paths` | meta |  | Scope paths: end-to-end requirements naming two topics and a budget. |
| `hazards` | meta |  | Hazards: what is watched, how long the system has, and which path reaches the safe state. |
| `functions` | meta |  | Named guard groups: what a set of topics together provides. A mode requires functions; a hazard may guard one by name. |
| `modes` | meta |  | Operational modes: what each requires, and the ordered ladder to fall to when it is lost. |
| `severity_levels` | meta |  | The severity scale hazards draw from, ascending; the first entry derives no criticality. Default: QM, ASIL_A..ASIL_D (ISO 26262), so writing that scale out changes nothing and is only for the reader. A contract that uses a profile takes the profile's scale and may not also write this. |
| `external_topics` |  | deprecated — use `topics.<name>.external` | Deprecated in v0.1.59 (phase 90 G3): the second place for the fact `topics.<name>.external` states. Move each entry under `topics:` as `<name>: { external: <side>, type, qos }`; `play_launch migrate` does it. Removed in the next grammar release. |
| `chains` |  | **removed** | Removed in phase 68 — state the requirement as a scope path and let the route be derived. |
| `if` | meta |  | Only on an INLINE `includes:` entry, whose value is a nested manifest: include this child only when the condition holds. Refused at the root of a standalone manifest. |
| `unless` | meta |  | Only on an INLINE `includes:` entry: include this child unless the condition holds. Refused at the root of a standalone manifest. |
| `policy` | meta | since v0.1.61 | Project policy for the consumer's rules: `{ unsupervised_measured: warn | error }`. Absent: the defaults. |
| `supervision` | fact | since v0.1.61 | The platform terms of a supervised budget's detection delay, stated once: `{ tick, switch }`. Read by the consumer's overrun case (`supervised-fit`); required once any budget states `supervise:`. |
| `acknowledged` | meta | since v0.2.3 | Findings the author has read and keeps: a list of `{ rule, at, reason }`, at the contract's root only. The consumer reports a warning or info whose rule id and path match an entry as acknowledged, with the reason, and counts it apart from the warnings; an entry matching an error is itself an error (errors cannot be acknowledged), and one matching nothing is a warning, so an acknowledgement cannot outlive its finding. |

## `nodes.<name>`

| key | kind | status | meaning |
|---|---|---|---|
| `if` | meta |  | Include this node only when the condition holds. |
| `unless` | meta |  | Include this node unless the condition holds. |
| `lifecycle` | fact |  | True for a ROS 2 managed node; runtime monitors skip checks until it is Active. |
| `pub` | meta |  | Publisher endpoints, keyed by endpoint name. A key is a LOCAL name: the node's FQN is prefixed to it, so a `/` in a key is refused; connect the topic with a launch remap or a `topics:` entry. |
| `sub` | meta |  | Subscriber endpoints, keyed by endpoint name. A key is a LOCAL name: the node's FQN is prefixed to it, so a `/` in a key is refused; connect the topic with a launch remap or a `topics:` entry. |
| `srv` | meta |  | Service server endpoints, keyed by LOCAL endpoint name. Not neutral when omitted: a consumer sizes the image's pools from this list (an embedded realizer derives its service counts from it), so a server the code creates and the contract leaves out is found at boot, not here. |
| `cli` | meta |  | Service client endpoints, keyed by LOCAL endpoint name. Not neutral when omitted, for the same reason as `srv:`: pools are sized from the list. A reaction route crosses a client -> server edge as a message (the resolver's reaction walk); a server with no path its request triggers latches the request for its own timer, and the walk charges that timer's period as a sampling hop. |
| `paths` | meta |  | This node's internal paths, keyed by path name. |
| `criticality` | **consequence** |  | Scheduling criticality: high | medium | low. A CONSEQUENCE of the hazards a node guards, reacts for, or feeds (phase 72): the MAX over the hazards that reach it, never a sum, so a node reached by two ASIL_B hazards is ASIL_B; the severity-to-criticality map is under "Severity to criticality" below. Written on a node a hazard reaches it is an error (`derivable-criticality`, `criticality-mismatch`: delete it). Written on a node no hazard reaches it stands, and is the scheduler mapper's input there. |
| `concurrency` | fact |  | Which of this node's paths may NOT run concurrently. Absent means all of them serialize: every path of the node waits for any other that is running (a single-threaded executor), and the checker charges that wait (`path-exclusion`). Declare `exclusive` groups to say which may in fact overlap. |
| `params` | fact |  | Parameters this node declares, keyed by name, each `{ type: <ROS 2 type> }`. Names and types only: a string or array capacity is a board fact, not a contract one. `params: {}` means the node declares no parameters; a missing `params:` means not stated. The two stay distinct in the model: `{}` becomes an empty `contracts.node_params` entry, a missing key none. |

## `pub/sub/cli.<endpoint>`

| key | kind | status | meaning |
|---|---|---|---|
| `min_rate_hz` | fact (pub) / requirement (sub) |  | Lower bound on this endpoint's rate. A REQUIREMENT, not a mechanism: nothing fires when a period passes. A fault is detected only by a mechanism that does -- a QoS `lease_duration` or `deadline`, or a `max_age` someone evaluates (`on_violation.mechanism`) -- so `min_rate_hz` never counts as a hazard's detector. A publisher that promises no rate states `on_demand: true` instead of leaving this out. |
| `max_rate_hz` | fact (pub) / requirement (sub) |  | Upper bound on this endpoint's rate. |
| `max_age` | requirement |  | Subscriber: maximum data age at receive (now - header.stamp). |
| `max_age_ms` |  | **removed** | Removed in phase 70 — write `max_age: <n>ms` (or ns/us/s). The unit in a NAME is what lets a value be 1000x wrong and still parse. |
| `state` | fact |  | Subscriber: read-latest rather than causal. The node caches the message and a timer path reads it, so a reaction arriving here waits for that timer: the walk charges one period (the timer's `jitter` too, when stated) as a sampling hop, shown as `(+Nms sampling)` in a route. |
| `required` | fact |  | Subscriber: this endpoint must be connected. |
| `qos` | fact |  | QoS overrides for this endpoint. |
| `max_transport` | requirement |  | Subscriber: the worst-case transport into THIS subscriber, overriding the topic's (one topic does not have one cost: in-process, another process, another machine). Charged in path and chain latencies, and in a hazard's reaction route on the GUARD EDGE (the hop into the subscriber that detects the fault), but not after a window's deadline, which the window's owner reads on its own clock. A declaration on no guard edge changes no hazard's numbers (`declared-not-charged`). |
| `max_transport_ms` |  | **removed** | Removed in phase 70 — write `max_transport: <n>ms` (or ns/us/s). The unit in a NAME is what lets a value be 1000x wrong and still parse. |
| `on_violation` | requirement |  | The reaction this subscriber owes when its assumption is violated. |
| `ingress` | requirement | since v0.1.64 | Subscriber: what this endpoint is made to ABSORB, a token bucket `{ rate_hz, burst }`. Not a QoS policy, and not the rate it needs (`min_rate_hz`): a consumer bounds the device's read path and the router's pacing by it. Both keys required, so an unstated burst never reads as unbounded. Refused under `pub:`/`cli:`. |
| `min_interarrival` | requirement | since v0.1.68 | Subscriber: the least time between two arrivals. Its callback is then a sporadic task (T_min). An event input with no rate bound, no `min_interarrival` and no `budget` is unanalysable: a consumer that supervises a deadline refuses it. Refused under `pub:`/`cli:`, and above `1 / min_rate_hz`. |
| `budget` | requirement | since v0.1.68 | Subscriber: the execution time its callback may use per `budget_period`. Its callback is then served (a server of budget Q per period P, analysed as a periodic task). Requires `budget_period`, and may not exceed it. Refused under `pub:`/`cli:`. |
| `budget_period` | requirement | since v0.1.68 | Subscriber: the server period P of `budget`. Only beside `budget`. |
| `call` | fact | since v0.1.69 | Client: how the code waits for the response. `blocking`: the callback that sends the request waits for it, so under a preemptive executor the server must run at a more urgent level (a less urgent one is a priority inversion, its own level a deadlock). `async`: the request is sent and a later callback takes the response. Absent means not stated: a consumer checks a stated call and reports an unstated one. Refused under `pub:`/`sub:`. |
| `buffer` | fact |  | Buffering discriminator for a state subscriber: latest | queue. |
| `on_demand` | fact | since v0.1.49 | Publisher: publishes only when asked (a service call, an operator) and promises no rate. Unlike an omitted `min_rate_hz`, which says nothing, this is acted on: no rate is derived for the topic, no runtime rate monitor is generated for the endpoint, and a subscriber's `min_rate_hz` or a topic `rate_hz` against it is a `rate-hierarchy` error. Refused beside `min_rate_hz`, and under `sub:`/`cli:`. |
| `jitter` |  | **removed** | Removed in phase 68 — jitter is a property of a route, not one publisher. Use `max_jitter` on a path. |
| `jitter_ms` |  | **removed** | Removed in phase 68 — see `jitter`. |

## `srv.<endpoint>`

| key | kind | status | meaning |
|---|---|---|---|
| `max_response` | requirement |  | Deadline for answering a request on this service. |
| `min_interarrival` | requirement | since v0.1.68 | The least time between two requests. The server's callback is then a sporadic task (T_min). See `min_interarrival` on a subscriber. |
| `budget` | requirement | since v0.1.68 | The execution time the server's callback may use per `budget_period` (a served callback). Requires `budget_period`, and may not exceed it. |
| `budget_period` | requirement | since v0.1.68 | The server period P of `budget`. Only beside `budget`. |
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
| `max_transport` |  | deprecated — use `nodes.<n>.sub.<ep>.max_transport` | Deprecated in v0.1.60 (phase 90 G3): a transport is the hop into ONE subscriber, stated on that subscriber's endpoint. Still read for one release as the `max_transport` of every subscriber of this topic that states none of its own (charged as the subscriber's is: path and chain latencies, and a reaction route's guard edge). `play_launch migrate` copies the value onto each such subscriber declared in the same file and deletes this key. Removed in the next grammar release. |
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
| `call` | fact | since v0.1.69 | How every client in `client` waits for its result: `blocking` (the callback that sends the goal waits for the result) or `async` (a later callback takes it). Absent means not stated. See `call` on a `cli:` endpoint. |

## `includes.<name>`

| key | kind | status | meaning |
|---|---|---|---|
| `manifest` |  | **removed** | Removed in v0.1.59 (phase 90 G7) -- nothing ever loaded the named file, so the include declared nothing. Write the child contract inline as `includes.<name>: { nodes: ..., ... }` (its `if`/`unless` at that inline manifest's root), or delete the entry; a launch include's own contract is found next to its launch file. |

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
| `input` |  | deprecated — use `trigger: { input: [...] }` | Deprecated in v0.1.59 (phase 90 G3): the legacy spelling of `trigger: { input: [...] }`, read as that trigger when no `trigger:` is written; `play_launch migrate` rewrites it. An empty or missing `input:` does NOT make a timer: a timer is `trigger: { timer: ... }`, and a path with neither is unclassified. Removed in the next grammar release. |
| `output` | fact |  | Endpoints (node path) or topics (scope path) this path produces. |
| `max_latency` | requirement |  | Latency budget for this path. On a NODE path: take -> publish of one callback, and the cost a reaction route charges for the hop; an embedded realizer may ALSO turn it into a derived node deadline and a runtime latency monitor in the image, so a cost that must not be a deadline (a queueing wait, a link) belongs in its own key (`max_transport`, a timer's `jitter`), not here. On a SCOPE path: the NOMINAL end-to-end traversal of the subtree, checked by `scope-budget`; a hazard's fault route is derived separately by walking the reactions, and does not read it. |
| `max_latency_ms` |  | **removed** | Removed in phase 70 — write `max_latency: <n>ms` (or ns/us/s). The unit in a NAME is what lets a value be 1000x wrong and still parse. |
| `correlation` |  | **removed** | Removed in phase 70 — nothing ever read it. State fan-in policy with `sync:`. |
| `tolerance` | fact |  | Max `header.stamp` spread between a fan-in path's inputs still treated as one set. |
| `tolerance_ms` |  | **removed** | Removed in phase 70 — write `tolerance: <n>ms` (or ns/us/s). The unit in a NAME is what lets a value be 1000x wrong and still parse. |
| `drop` | requirement |  | Scope path only: the end-to-end loss budget, `max_count: N / W` and `max_consecutive: K` (or the bare `N / W`). No topic on the path's route may permit more loss than the whole (`drop-tightness`). On a node path it is refused since v0.1.60 (phase 90 G3): nothing read it there; the loss belongs on the topic the path outputs (`topics.<t>.drop`), where `play_launch migrate` moves it. |
| `trigger` | fact |  | What causes this path's output: timer | input | once | spontaneous. A timer path is a node path whose `input` is empty and whose trigger carries the rate; a downstream node's timer that reads a message (a `state: true` subscriber, a latched service request) is where a reaction waits for a tick. |
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
| `rate_hz` | fact |  | Timer rate, Hz. Required, and must be greater than zero. A bare number, or the map form `{ value, source }` (since v0.1.61) to say where the rate comes from: `source: allocated` lets the consumer refuse a period outside the range it derives. |
| `jitter` | fact | since v0.1.49 | Release jitter: how late a tick may be released after its nominal instant, measured (a duration, less than one period). A reaction that waits for this timer is charged `period + jitter` (the reaction walk's sampling hop and `window-expiry`'s notice) instead of a bare period. Absent: ticks are on time. A budget: the map form `{ duration, source }` applies. |
| `param` | fact | since v0.1.58 | The parameter of this path's node that holds the timer's RATE, Hz (`update_rate`). Its resolved launch value must equal `rate_hz` (`timer-param`). At most one of `param` and `period_param`. |
| `period_param` | fact | since v0.1.58 | The parameter of this path's node that holds the timer's PERIOD, seconds. Its resolved launch value must equal `1 / rate_hz` (`timer-param`). |

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

## `params.<name>`

| key | kind | status | meaning |
|---|---|---|---|
| `type` | fact |  | ROS 2 parameter type: bool | integer | double | string | byte_array | bool_array | integer_array | double_array | string_array. Required; an unknown type is an error. |

## `hazards.<name>`

| key | kind | status | meaning |
|---|---|---|---|
| `severity` | requirement |  | Severity label from the HARA (`ASIL_D`, `SIL_3`, …). Consumed, never computed. |
| `description` | meta |  | What the hazardous event is. |
| `guards` | meta |  | Topics watched for the fault. A bare name is one guard (a topic, or a function by name: a redundant set is `functions.<f>: { all_of: [...] }`, guarded as `f`); a list is a group lost when any member is. |
| `on` | fact |  | Fault classes the guards report, one or a list: omission | late | loss | reported. Omitted means every class, the strictest reading; `[]` is an error. |
| `ftti` | requirement |  | Fault-tolerant time interval: fault to hazardous event, absent reaction. Checked by `fault-reaction-budget`. |
| `enters` | meta | since v0.1.59 | The mode the hazard enters: the top of the `fallback:` ladder it walks to a safe state, each rung checked in its own right (`ladder-rung-budget`) and the floor against `ftti`. Exactly one of `enters` and `reaction`; a name that is not a mode of this scope is `enters-unknown-mode`. |
| `reaction` | meta |  | The scope path whose route reaches the safe state: a one-rung reaction. Exactly one of `enters` and `reaction`. A mode named here is the pre-0.1.59 spelling of `enters` (`deprecated-spelling`; `play_launch migrate` rewrites it). |
| `when` | fact |  | The value that is the fault, `{ field: <name>, <op>: <constant> }`, on a hazard whose guard is a detector's output. Requires `reported` among the `on:` classes (`when-requires-reported`). |
| `detection` | fact |  | Fault to detection when that is physics, not a detector's timeout (an occlusion building pressure), a duration or `{ duration, source }`. Charged before the detection the consumer derives from the detectors, so stating it never buys slack. Usually derived by an extension (`source: { derived: ... }`). |
| `deadline_by` | requirement |  | `<extension>/<check>`: the check that is this hazard's requirement when it is not a time (a safety distance). A hazard states `ftti`, `deadline_by`, or both. |
| `reaction_total` |  | **removed** | Read-only: the consumer computes it (detection + route + settle) and hands it to extension checks. A contract never writes it. |

## `modes.<name>`

| key | kind | status | meaning |
|---|---|---|---|
| `description` | meta |  | What this mode is. |
| `requires` | fact |  | Functions (or bare topics) this mode needs; it is available while every one holds. |
| `fallback` | requirement |  | Modes to fall to, in order, when this one is lost. The last rung is the floor and must require nothing losable. |
| `reaction` | meta |  | The scope path that reaches this mode's safe state. |
| `overrides` | requirement |  | Requirement values that apply IN THIS MODE, pinned over the scalar declared elsewhere. |
| `window` | requirement |  | A transitional rung: while it stays available the system stays AT LEAST this long (the time the rung promises whoever it waits for, a driver's takeover time), then takes the next rung. The rung below starts no sooner than the deadline and within its own reaction route after it, and that route is where the deadline's notice is charged (`window-expiry`); its first hop crosses no link (the owner reads its own clock), so a guard edge's `max_transport` is not charged there. A duration, or `{ duration, param }` binding it to the parameter that enforces it. Never on the floor (`ladder-window-floor`); unbound is `window-unbound`. |
| `exit` | requirement |  | `{ holds: <function>, to: <mode> }`: while this rung is active, the function holding ends the ladder in `to`. Only on a windowed rung; `to` is outside every `fallback:` list. |

## `hazards.<name>.guards[]`

| key | kind | status | meaning |
|---|---|---|---|
| `all_of` |  | **removed** | Removed in v0.1.59 (phase 90 G7) -- an inline redundant group is the same group `functions.<f>: { all_of: [...] }` states by name. Declare the function and guard it by name: `guards: [<f>]`; `play_launch migrate` does it. |

## `on_violation`

| key | kind | status | meaning |
|---|---|---|---|
| `on` | fact |  | Fault classes that trigger the reaction: omission | late | loss | reported. |
| `reaction` | meta |  | A path on this node whose trigger includes the subscription. |
| `within` | requirement |  | This hop's share of the fault reaction time. |
| `mechanism` | fact |  | Where the runtime observer reads the violation: qos (default) | diagnostics | application. |

## `ingress`

| key | kind | status | meaning |
|---|---|---|---|
| `rate_hz` | requirement | since v0.1.64 | Sustained messages per second the subscriber absorbs. Finite and > 0, and not below the endpoint's own `min_rate_hz`. |
| `burst` | requirement | since v0.1.64 | Messages that may arrive back to back before pacing applies. An integer >= 1. |

## `safe_state`

| key | kind | status | meaning |
|---|---|---|---|
| `emits` | fact |  | The endpoint this reaction commands the safe state on. |
| `settle` | fact |  | How long the plant takes to reach the safe state once commanded. `settle: <d>` is shorthand for `settle: { duration: <d> }`; the map is `{ duration ; response ; source }` plus the keys of a used extension that derives `settle.duration` (v0.2.0: the braking profile `decel`, `jerk` with the hazard's `entry_speed` is the bundled `vehicle.braking`'s; `settle-derived` prints its trace). `response` is added to either (omitted: 0). Both a duration and a derivation: the derivation is charged and the two compared (`settle-conflict`). |

## `functions.<name>`

| key | kind | status | meaning |
|---|---|---|---|
| `all_of` | meta |  | Members of a redundant group, lost only when every member is. At least two. |
| `of` | meta |  | The topics a value function reads. Requires `when:`; without one, write the bare list. |
| `when` | fact |  | The predicate that LOSES the function by value. A value function is lost by the value or by silence. |

## `when`

| key | kind | status | meaning |
|---|---|---|---|
| `field` | fact |  | One scalar field of the message, a dotted path from its root. Checked against the `.msg` when the ament index resolves it (`when-field-unknown`). |
| `equals` | fact |  | The field equals the constant: a bool, a number, a message constant name (upper snake case) or a string. |
| `not_equals` | fact |  | The field differs from the constant. |
| `lt` | fact |  | The field is below the constant, a number. |
| `le` | fact |  | The field is at most the constant, a number. |
| `gt` | fact |  | The field is above the constant, a number. |
| `ge` | fact |  | The field is at least the constant, a number. |

## `modes.<name>.exit`

| key | kind | status | meaning |
|---|---|---|---|
| `holds` | fact | since v0.1.59 | The function whose holding ends the ladder. Some node implementing the rung must take its topic on a path trigger (`mode-exit-unwired`). |
| `on` |  | deprecated — use `holds` | Deprecated spelling. |
| `to` | meta |  | The mode the ladder ends in. Not a member of any `fallback:` list (`mode-exit-target`). |

## `safe_state.settle`

| key | kind | status | meaning |
|---|---|---|---|
| `duration` | fact |  | The written settle (a measured duration): `settle: { duration: 2s }` is `settle: 2s`. Beside an extension's keys, the fallback when a hazard does not write a key the derivation needs (`settle-entry-missing`), and compared against the derivation (`settle-conflict`). Required when no extension derives the settle. |
| `response` | fact | since v0.1.58 | The vehicle's response: from the braking command to the deceleration the plant then holds (a duration). Added to the settle, a written duration or a derived one alike (an extension that reads `settle.response`, as `vehicle.braking` does, adds it in its own formula; `settle-derived` and `--explain` print both parts). Omitted: 0, so a written duration that already includes the response omits it. The map form `{ duration, source }` applies. |
| `source` | meta | since v0.1.60 | Where the written `duration` comes from, as a budget's `source:` (phase 10). Requires `duration`: a profile's settle is derived from its parameters. |
| `supervise` | requirement | since v0.1.61 | The runtime supervision of the WHOLE settle (since v0.1.62): from the safe state's command to standstill, the written `duration` or the derived profile plus the `response`. `{ reaction: <path> | enters: <mode>, mechanism: executor | timer, observes: <node>/<subscriber> }`: the platform arms due = command + settle + how late it sees standstill (`observes`: that input's period + its `max_transport`), and a vehicle not at standstill by then starts the reaction. The consumer's overrun case charges the rung up to the settle's end, that observation, the detection and the reaction's route and settle. v0.1.61 took it on a written `duration` only. |

## `<budget> (map form)`

| key | kind | status | meaning |
|---|---|---|---|
| `duration` | requirement | since v0.1.58 | The budget itself, with a unit. Required. `max_latency: { duration: 149ms, source: ... }` states the same budget as `max_latency: 149ms`, plus where it comes from. Accepted by `max_latency`, `max_jitter`, `max_transport`, `max_age`, `max_response`, `within`, `ftti`, a timer's `jitter`, a settle's `response` and a mode's `window` (where it is the least time the rung lasts while it stays available: the rung below starts no sooner; positive). See Bounds below. |
| `param` | fact | since v0.1.58 | The parameter whose resolved launch value, in seconds, must equal `duration`. On `max_age`: a parameter of this subscriber's node (`max-age-param`). On a mode's `window`: `<node>.<parameter>` (`window-param`). No other bound takes one. |
| `source` | meta | since v0.1.58 | Where the budget comes from: `requirement` (chosen: a hazard analysis, a decision, a configured value), `measured` (an observed maximum plus a margin: NOT a bound), `derived` (computed from other numbers of the contract or the image's parameters) or `analysed` (an upper bound from an analysis). `{ measured: { max, n, margin } }` states the observations, and the consumer checks that `max x (1 + margin)`, rounded up to the next ms, reproduces `duration` (`budget-rule`). `allocated` (since v0.1.61): chosen inside a range the consumer derives. |
| `supervise` | requirement | since v0.1.61 | The runtime checks the budget while the system runs and a miss starts a fallback: `{ reaction: <path> | enters: <mode>, mechanism: executor | timer }`. The consumer derives the detection delay and checks that the overrun case (the budget + the detection + the fallback's route and settle) fits the hazard's FTTI. A budget's level is derived from this and `source`: analysed -> bound; supervised; measured alone -> unsupervised. Not on a `window` (a least time). |

## `source.measured`

| key | kind | status | meaning |
|---|---|---|---|
| `max` | fact | since v0.1.58 | Single-term form: the largest value observed (a duration). For a timer tick with `period`, the largest lateness beyond the period. |
| `n` | fact | since v0.1.58 | Single-term form: how many samples the maximum is over. |
| `period` | fact | since v0.1.58 | Single-term form: a timer tick's period, charged without margin: the term is `period + max x (1 + margin)`. |
| `margin` | requirement | since v0.1.58 | The margin over the observed maximum, a fraction (`0.2` = 20 %). Required with `max` or `terms`. |
| `terms` | fact | since v0.1.58 | Several terms summed, a map of name -> `{ max, n, period }`: each is rounded up to the next ms on its own, then they are added (`tick: { period: 100ms, max: 14.84ms }`, `work: { max: 25.04ms }` -> 118 + 31 = 149 ms). Refused beside `max`. |

## `source.derived`

| key | kind | status | meaning |
|---|---|---|---|
| `by` | fact |  | The extension that derived the value, a dotted name (`vehicle.braking`). Required. |
| `version` | fact |  | The extension's version, `MAJOR.MINOR.PATCH`. Required. |
| `sha256` | fact |  | The sha256 of the extension's canonical ASCII form, 64 lowercase hex digits. |
| `inputs` | fact |  | The inputs the derivation read, a map of name -> value as written (`entry_speed: 8.33 m/s`). |
| `trace` | fact |  | The derivation's steps, a list of lines, printed by `--explain`. |

## `source.measured.terms.<name>`

| key | kind | status | meaning |
|---|---|---|---|
| `max` | fact | since v0.1.58 | The largest value observed for this term (a duration). Required. |
| `n` | fact | since v0.1.58 | How many samples the maximum is over. |
| `period` | fact | since v0.1.58 | A timer tick's period, charged without margin. |

## `<budget>.supervise`

| key | kind | status | meaning |
|---|---|---|---|
| `reaction` | fact | since v0.1.61 | The path a miss starts: a path of the budget's own node (as `on_violation.reaction`), else a scope path of the manifest. Exactly one of `reaction` and `enters`. |
| `enters` | fact | since v0.1.61 | The mode a miss enters; its `reaction:` scope path is what runs. Exactly one of `reaction` and `enters`. |
| `mechanism` | fact | since v0.1.61 | What checks the budget. Required. `executor`: checked in the spin loop, so the detection waits for the longest callback on that executor, and a stuck callback is not caught (slowness, not a hang). `timer`: an independent timer and thread, which catches both and adds the platform's switch (`supervision.switch`). |
| `observes` | fact | since v0.1.62 | A settle's supervision only: the input the platform sees standstill on, `<node>/<subscriber>` of this manifest. The consumer derives how late standstill is seen: the input's period (the slowest of the topic's `rate_hz` and the subscriber's `min_rate_hz`) + the subscriber's `max_transport`, and adds it to the supervised deadline. Refused on any other budget. |

## `acknowledged[]`

| key | kind | status | meaning |
|---|---|---|---|
| `rule` | meta | since v0.2.3 | The rule id of the finding, as the diagnostic prints it (`reaction-unguarded`). Required, non-empty. |
| `at` | meta | since v0.2.3 | The dotted path the diagnostic reports (`hazards.<h>.reaction`; `check --format json` prints it as `path`). Required, non-empty. Two entries naming the same rule and path are refused. |
| `reason` | meta | since v0.2.3 | Why the finding is kept, printed beside it. Required, non-empty. |

## `policy`

| key | kind | status | meaning |
|---|---|---|---|
| `unsupervised_measured` | requirement | since v0.1.61 | The severity of a measured budget nothing supervises (`measured-unsupervised`): `warn` (the default) or `error`. |

## `supervision`

| key | kind | status | meaning |
|---|---|---|---|
| `tick` | fact | since v0.1.61 | The period the monitor's clock is read at: the executor's tick, the supervision timer's resolution (a duration, positive). Part of every supervised budget's detection delay. |
| `switch` | fact | since v0.1.61 | The interrupt plus the thread switch before a `timer` mechanism's reaction runs (a duration). Required by a `mechanism: timer` supervision. |

## `trigger.timer.rate_hz (map form)`

| key | kind | status | meaning |
|---|---|---|---|
| `value` | fact | since v0.1.61 | The rate, Hz. Required; greater than zero. |
| `source` | meta | since v0.1.61 | Where the rate comes from, as a budget's `source:`. `allocated`: chosen inside the range the consumer derives, which refuses a period outside it. |

## Bounds

Every key below is a spelling of one bound: a value with a unit, what it limits (its **kind**), from which side, where the number comes from (`source`, in the map form) and, for two of them, the parameter of the image that must carry it (`param`). The keys keep their names; each parses to the bound its row gives. **map form** says what the key accepts besides its bare value: `{ duration, source }` (the budget map form above), or that plus `param`.

| key | where | kind | limit | unit | map form |
|---|---|---|---|---|---|
| `max_latency` | `paths.<name>` | latency | at most | duration | `{ duration, source }` |
| `min_latency` | `paths.<name>` | latency | at least | duration |  |
| `max_jitter` | `paths.<name>` | jitter | at most | duration | `{ duration, source }` |
| `jitter` | `trigger.timer` | jitter | at most | duration | `{ duration, source }` |
| `min_rate_hz` | `pub/sub/cli.<endpoint>` | rate | at least | Hz |  |
| `max_rate_hz` | `pub/sub/cli.<endpoint>` | rate | at most | Hz |  |
| `max_age` | `pub/sub/cli.<endpoint>` | age | at most | duration | `{ duration, source, param }`; `param`: a parameter of the subscriber's node, seconds (`max-age-param`) |
| `max_transport` | `pub/sub/cli.<endpoint>` | transport | at most | duration | `{ duration, source }` |
| `max_transport` | `topics.<name>` | transport | at most | duration | `{ duration, source }` **deprecated** (phase 90 G3) |
| `max_response` | `srv.<endpoint>` | response | at most | duration | `{ duration, source }` |
| `within` | `on_violation` | deadline | at most | duration | `{ duration, source }` |
| `ftti` | `hazards.<name>` | deadline | at most | duration | `{ duration, source }` |
| `detection` | `hazards.<name>` | latency | at most | duration | `{ duration, source }` |
| `window` | `modes.<name>` | least time | at least | duration | `{ duration, source, param }`; `param`: `<node>.<parameter>`, seconds (`window-param`) |
| `settle` | `safe_state` | response | at most | duration | `{ duration ; response ; source }` plus a used extension's keys (`settle: <d>` is `{ duration: <d> }`) |
| `duration` | `safe_state.settle` | response | at most | duration |  |
| `response` | `safe_state.settle` | response | at most | duration | `{ duration, source }` |
| `max_count` | `drop` | count | at most | count / window |  |
| `max_consecutive` | `drop` | count | at most | count |  |
| `tolerate` | `miss` | count | at most | count / window |  |
| `consecutive` | `miss` | count | at most | count |  |

## Severity to criticality

A node a hazard reaches (it feeds, detects for, or reacts for the hazard) takes a scheduling criticality from the hazard's `severity`, the MAX over every hazard that reaches it. The first entry of `severity_levels` is "no safety requirement" and derives nothing; the other `n` entries split into three buckets by `ceil(3 rank / n)` (1 low, 2 medium, 3 high). A written `criticality` on such a node is an error; on a node no hazard reaches it stands. The default scale (no `severity_levels` declared):

| severity | criticality |
|---|---|
| `QM` | none |
| `ASIL_A` | low |
| `ASIL_B` | medium |
| `ASIL_C` | high |
| `ASIL_D` | high |

A contract that uses a profile (phase 91 E6; `use: { iec61508: "^1" }`) draws severities from the profile's `severity_scale` and takes each level's criticality from the profile's `criticality` map, stated level by level, instead of the split above; it may not also write `severity_levels:`. Reference profiles ship with the checker: `iso26262`, `iec61508`, `iec61511`, `do178c`, `iec62304`, `iso13849`, `en50129` (`iso26262` is the default scale and map above).

