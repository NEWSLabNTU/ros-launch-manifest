# Service calls on a reaction route

Status: grammar and model implemented in v0.2.5 (2026-10-27); the checker
and the runtime companion are play_launch phase 95. Replaces play_launch
phase 85's D2 (which proposed an edge cost alone).

## The problem

Since play_launch phase 82 the reaction walk crosses a service edge: a
client's request reaches the server, and the walk continues through the
server's path that the request triggers, or through its timer when the server
latches the request. The hop costs nothing today. D2 proposed charging it
`max_transport`, as a topic hop is charged.

That bounds the one case in which nothing goes wrong. The contract grammar is
generic: rclcpp, rclpy and nano-ros all realise it. In stock ROS 2 a call can
fail six ways, and five of them make a route through the call unbounded:

| | Failure | What the client sees in stock ROS 2 |
|---|---|---|
| F1 | no server when the request is sent | nothing; the request is lost unless the client checked `wait_for_service` |
| F2 | the server dies after taking the request | a future that never completes: rclcpp has no default timeout, and the pending entry stays until `remove_pending_request`/`prune_pending_requests` |
| F3 | the request or the response is lost | as F1 (request) or F2 (response) |
| F4 | the server is slow | a late response |
| F5 | the server refuses (`success: false`) | a response, and no effect |
| F6 | a blocking call from a callback whose response can only be taken by the same executor thread or callback group | a deadlock |

Services have no liveliness or deadline events, so nothing in the middleware
reports F1-F3. A route through a call is therefore a number the system may
never meet, and the checker has been printing it as if it would.

## Related work

### RPC on the web

- **gRPC** makes the deadline part of every call; a call without one is a
  known anti-pattern. A deadline PROPAGATES: a server calling onward passes
  the remaining time, so a nested call cannot outlive the caller's budget. A
  retry policy and a hedging policy are per method and mutually exclusive;
  retries are limited by a per-server token budget (a failure costs a token, a
  success returns `tokenRatio`, retries stop below half), and a server can push
  back (`grpc-retry-pushback-ms`) or forbid retry. Retrying is safe only for an
  idempotent method.
- **Erlang/OTP** `gen_server:call` has a DEFAULT timeout (5 s) and crashes the
  caller when it expires: the failure is never silent, and the supervisor tree
  decides what happens next.
- **Production practice** (circuit breakers, retry budgets, idempotency keys)
  answers what happens to the server's work when the client has given up: a
  late request may still execute, so a non-idempotent operation needs a key or
  a refusal of stale requests.

### Industrial systems

- **OPC UA** carries `timeoutHint` in every request header: the client's stack
  times the call out, and the server MAY cancel the operation and answer
  `Bad_Timeout`. The server is told how long the client will wait, which ROS 2
  never does.
- **Modbus** masters configure a response timeout and a retry count per
  request; the protocol has no other failure signal.
- **Transaction monitors** (e.g. OpenTP1) tell the server the client's limit,
  and the server discards requests that arrive after it and aborts the work in
  progress. Where the server is not told, it keeps working after the client
  gave up.
- **nav2's lifecycle manager** pairs every lifecycle service call with a
  timeout (`service_timeout`, 5 s on Kilted and Rolling) AND supervises each
  managed node's liveness separately with a heartbeat bond (`bond_timeout`,
  4 s): the call's failure and the server's death are detected by different
  mechanisms.

### Safety-critical systems

- **AUTOSAR Classic** gives a server call point a `timeout`; a synchronous
  `Rte_Call` returns `RTE_E_TIMEOUT`, an asynchronous one reports it through
  `Rte_Result`, and a response arriving after the timeout is discarded. The
  E2E library protects client-server exchanges with a counter that pairs a
  response with its request.
- **IEC 61784-3** (safety fieldbuses over a black channel) and ISO 26262-6
  Annex D list the communication faults a safety layer must handle:
  corruption, unintended repetition, incorrect sequence, loss, unacceptable
  delay, insertion, masquerade, addressing. The measures are a sequence
  number, a timeout with receipt, a source identity and an integrity check;
  on too many errors the consumer substitutes fail-safe values.
- **ISO 26262 supervision** (alive, deadline, logical) and the island's own
  design supervise the EFFECT rather than the exchange: the island's
  comfortable stop is supervised to standstill by a timer above the executor,
  whatever happened to the `operate` call.
- **Partitioned and cyclic designs** (ARINC 653 ports, PROFIsafe's cyclic
  exchange) keep request/response off the critical path entirely: commands
  are periodic state, so a lost message is recovered by the next one.

### What carries over

1. Every mature RPC system makes the client's wait EXPLICIT at the call site.
   rclcpp's infinite default is the outlier, so the contract must make the
   author state it.
2. A deadline propagates. Here that is free: a call's failure branch is part
   of the rung's route, which is already checked against the hazard's FTTI.
3. The server keeps working after the client gives up unless it is told or
   refuses stale requests. A late effect is a real hazard: the comfortable
   operator starting a stop after the handler has fallen to the emergency
   stop.
4. A retry is sound only on an idempotent operation, and only with a bound.
5. Liveness of the server and failure of the call are different facts,
   detected by different mechanisms (nav2's bond versus its call timeout).
6. Supervising the effect covers every failure at once; supervising the call
   covers all but refusal and deadlock.

## Design

### Client facts (`nodes.<n>.cli.<ep>`)

```yaml
cli:
  comfortable_stop_operate:
    call: blocking                                  # existing (rlm v0.1.69)
    timeout: $(param timeout_call_mrm_behavior) s   # how long the code waits
    on_timeout: { reaction: emergency_stop }        # what it does then: a mode or scope path
    on_rejected: { reaction: emergency_stop }       # what it does on success: false
    max_transport: 1.5ms                            # the request leg, success case
```

- `timeout:` is a FACT about the code: the client gives up after this
  (`spin_until_future_complete` with a timeout, `prune_requests_older_than`,
  a tick-counting poll). `$(param …)` reads it from the node's own parameter.
- `on_timeout:` is the failure edge, the client's counterpart of a
  subscriber's `on_violation`: a mode (the hazard falls to it) or a scope path.
  `{ retry: { attempts: N, backoff: D }, then: { reaction: … } }` retries,
  and only when the server declares the operation `idempotent`.
- `on_rejected:` is required only when the route depends on the response
  (a blocking call whose path continues at the client).
- `max_transport:` is the request leg's cost, with a topic's precedence:
  `services.<s>.max_transport` for every client, `cli.<ep>.max_transport` for
  one.

### Server facts (`nodes.<n>.srv.<ep>`)

```yaml
srv:
  operate:
    idempotent: true       # a repeated request has the effect of one
    stale_after: 50ms      # requests older than this are refused (rmw source timestamp)
```

- `idempotent:` licenses a retry.
- `stale_after:` says the server refuses a request the client has probably
  given up on. Without it, a request the client timed out on may still act
  (lesson 3).

### Arithmetic

The route through a call is the WORST of its branches:

```
success = max_transport + the server's route (+ the response leg if blocking and the route continues at the client)
failure = timeout x attempts + backoff x (attempts - 1) + on_timeout's route
route   = max(success, failure)
```

A supervised rung (`safe_state.supervise`) needs no failure branch at the
call: its failure case is the supervisor's due plus the escalation route,
which `supervised-fit` already computes.

### Rules

| Rule | Severity | Fires when |
|---|---|---|
| `call-unbounded` | error | a reaction route crosses a call with no `timeout:` + `on_timeout:`, and the rung is not supervised |
| `call-retry-not-idempotent` | error | `retry:` on a call whose server does not declare `idempotent: true` |
| `call-retry-unbounded` | error | `retry:` without `attempts`, or without `then:` |
| `call-late-effect` | warning | `on_timeout:` falls to a reaction that is NOT a lower rung of the call's own ladder, and the server declares no `stale_after:` (a late request can act after the fallback) |
| `call-rejected-unhandled` | warning | the route continues at the client after a blocking call, and no `on_rejected:` |
| `call-deadlock` | warning | `call: blocking` from a path of a node with no `concurrency:` declaration, so every callback serialises and the response cannot be taken, unless the wait spins its own executor, which the contract cannot see |

A call that is on no reaction route is not judged: a lifecycle manager's
startup calls, a parameter client, a map loader. Only the walk applies these
rules.

### Coverage

| | timeout + fallback | supervised effect | server liveness guard |
|---|---|---|---|
| F1 no server | yes | yes | yes |
| F2 server dies | yes | yes | yes |
| F3 request lost, server alive | yes | yes | no |
| F3 response lost, server acted | yes (fallback is spurious: harmless only to a lower rung) | yes | no |
| F4 slow | yes | yes | no |
| F5 refused | only with `on_rejected` | yes | no |
| F6 deadlock | no (static rule only) | yes (a supervisor above the executor) | no |

So a server-liveness guard (a function on the server's status topic, nav2's
bond) is NOT a bound on its own: it detects death sooner, and nothing else.
The bound is a supervised effect, or a timeout with a fallback.

### Runtime companion (play_launch)

The interceptor hooks `rcl_send_request`, `rcl_take_request`,
`rcl_send_response` and `rcl_take_response`, pairs them by sequence number,
and reports `call-unanswered` on the Health page for a request still open past
its client's `timeout:` (or 10 s when none). `measure` prints a
`max_transport` from the request legs it timed. This detects F1-F3 live on a
host and fills the cost the design needs.

## Simulated workflows

### S1: the safety island's comfortable stop (async, supervised effect)

The handler fires `operate` on its tick and drains the reply later (`call:
async`); the comfortable rung is supervised to standstill (`supervise: {
enters: emergency_stop, mechanism: timer }`).

- Contract change: none required. Optionally `max_transport: 1.5ms` on the
  client.
- Check: the walk reaches the call, finds the rung supervised, and takes no
  failure branch at the call. `call-unbounded` does not fire. The success
  route grows by 1.5 ms (149 to 150.5).
- F5 (refused): the supervisor fires at its due and escalates; nothing new to
  state.
- Late effect: the fallback (`emergency_stop`) is a lower rung of the same
  ladder, so `call-late-effect` stays silent, which is right: the emergency
  stop supersedes a late comfortable stop.

### S2: upstream Autoware's `mrm_handler` (blocking, 10 ms)

Upstream waits up to `timeout_call_mrm_behavior: 0.01` on the future inside
its timer callback. On failure it logs and returns; the next tick asks again.

- The author first writes `call: blocking` and nothing else. Check:
  `error[call-unbounded]: hazards.<h> crosses /system/mrm/comfortable_stop/operate
  (mrm_handler.cli.comfortable_stop_operate) with no timeout: and no
  supervised rung`.
- They add `timeout: $(param timeout_call_mrm_behavior) s` and, reading the
  code, `on_timeout: { retry: { attempts: … } }`, and discover there is no
  attempt limit: upstream retries every tick, forever. `call-retry-unbounded`
  fires. This is a real finding: upstream's fallback for a dead or deaf
  operator is the operator's status going silent or NOT_AVAILABLE, which the
  handler reads elsewhere. That is a liveness guard, which the coverage table
  shows does not bound F3-F5.
- `call-deadlock` warns unless the contract declares `concurrency:` putting
  the client callback in its own group. Upstream uses a separate callback
  group for its clients on a multi-threaded executor; the declaration states
  exactly that.
- Outcome: the contract cannot be made clean without either supervising the
  effect (as the island does) or giving the call an attempt limit and a
  fallback. That is the purpose: the route was never bounded.

### S3: nav2 lifecycle bring-up

`lifecycle_manager` calls `change_state` on each managed node with a timeout,
during startup. No hazard's reaction crosses these calls, so no rule applies
and the checker prints nothing new. Its bond is a liveness guard and could be
declared as a function, unchanged by this design.

### S4: a retried, non-idempotent call

A client retries `/brake_test/trigger` three times on timeout. The server does
not declare `idempotent`. Check: `error[call-retry-not-idempotent]`, naming
the server: a retry after a lost response would run the test twice.

### S5: a timeout longer than the budget

A blocking call with `timeout: 2s` sits on a rung of a hazard with
`ftti: 1.5s`. No new rule fires: the failure branch (2 s + the fallback's
route) is the rung's route, so the existing `ladder-rung-budget` refuses it
and names the term: `… 2000.00 ms timeout (cli.operate) + 188.33 ms
emergency_stop route > 1500 ms`. Deadline propagation needs no rule of its
own.

### S6: response lost, server acted

The comfortable operator takes `operate`, starts the stop, and its response
is lost. The client times out and falls to the emergency stop. Under S1's
ladder the fallback is lower, so the stronger stop wins, which is the right
outcome, and the coverage table records it as harmless. Had `on_timeout`
named an UNRELATED path (a different actuator), `call-late-effect` would
warn: two reactions are now acting.

## What the simulations changed

- A server-liveness guard was an early candidate for a third bound. S2 and the
  coverage table showed it bounds only death (F1, F2), so it is a detection
  aid, not a bound.
- `call-late-effect` came out of S6 and OpenTP1/OPC UA's server-side
  handling: without it, a timeout fallback can leave two reactions acting.
- Deadline propagation (gRPC) needed no rule: S5 showed the existing rung
  budget already carries it, provided the timeout is a term of the route.

## Open questions

- `stale_after:` needs the request's source timestamp at the server
  (`rmw_service_info_t.source_timestamp`). Confirm every RMW in use fills it,
  and what nano-ros offers.
- Whether `call-deadlock` should be an error when the node is known to run a
  single-threaded executor (play_launch can see a stock container's
  executable; a contract cannot).
- Actions (goal, cancel, result) share most of this; whether they get the same
  keys under `act:` in the same phase.
- nano-ros's agreement that `timeout:`/`on_timeout:` derive no deadline or
  monitor beyond what a subscriber's `on_violation` does.

## Sources

- gRPC retry guide, <https://grpc.io/docs/guides/retry/>; request hedging,
  <https://grpc.io/docs/guides/request-hedging>; gRFC A6 client retries,
  <https://github.com/jdcormie/proposal/blob/master/A6-client-retries.md>
- OPC UA `RequestHeader.TimeoutHint`, Unified Automation SDK reference,
  <https://documentation.unified-automation.com/uasdkc/1.10.1/html/structOpcUa__RequestHeader.html>;
  OPC Foundation issue 49, <https://mantis.opcfoundation.org/view.php?id=49>
- OpenTP1 client timeout handling,
  <https://itpfdoc.hitachi.co.jp/manuals/3000/30003D5130e/PROG0035.HTM>,
  <https://itpfdoc.hitachi.co.jp/manuals/3000/30003D5920e/CLTJ0028.HTM>
- nav2 lifecycle manager configuration,
  <https://docs.nav2.org/jazzy/configuration_and_development/configuration_guide/core_servers/configuring_lifecycle_manager/>;
  source, <https://api.nav2.org/nav2-rolling/html/lifecycle__manager_8cpp_source.html>
- rclcpp `Client` (`remove_pending_request`, `prune_pending_requests`),
  <https://docs.ros.org/en/iron/p/rclcpp/generated/classrclcpp_1_1Client.html>;
  `prune_requests_older_than` (Kilted and later),
  <https://api.nav2.org/kilted/html/classrclcpp_1_1GenericClient.html>
- AUTOSAR E2E protocol (client-server counter handling),
  <https://autosar.org/fileadmin/standards/R24-11/FO/AUTOSAR_FO_PRS_E2EProtocol.pdf>;
  E2E transformer, <https://www.autosar.org/fileadmin/standards/R22-11/CP/AUTOSAR_SWS_E2ETransformer.pdf>;
  RTE acceptance tests (client-server timeouts),
  <https://www.autosar.org/fileadmin/standards/R1.2.0/TC/AUTOSAR_ATS_RTE.pdf>
- IEC 61784-3 error-to-measure mapping, OPC UA Part 15 §5.3,
  <https://reference.opcfoundation.org/specs/OPC-10000-15/5.3>; 61508
  Association, functional safety and communications,
  <https://61508.org/wp-content/uploads/2024/11/11A-Functional-Safety-and-Communications-V1-e092024.pdf>
- The safety island: `src/autoware_mrm_handler` (send-and-poll clients) and
  `safety_island.contract.yaml` (`supervise:` on the comfortable stop).

Not verified against primary text, and stated from general knowledge: the
AUTOSAR RTE's discarding of late responses and `RTE_E_TIMEOUT` (SWS_RTE),
Erlang's 5 s `gen_server:call` default, and Modbus master timeouts.
