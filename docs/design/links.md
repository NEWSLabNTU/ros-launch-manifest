# Links: transport as a measured, sourced assumption

**Status (2026-11-03): decided; implementation tracked by play_launch phase
97. L1 and L2 in rlm v0.3.4 (`links:`, `sub.<ep>.link`, `link-unsourced`,
`transport-inline`; the per-manifest chain sum reads the one precedence).
Only a subscriber names a link, not a topic: a transport is the hop into
one subscriber (phase 90 G3 deprecated the topic-level `max_transport`).
L3 and L4 in play_launch 0.24.0. L5 (runtime monitor) is NOT adopted, by
user decision (2026-11-04): links are measured and sourced, and
`check --against` holds the bound; `monitor:` stays in the grammar,
parsed and not read.**
Refines design issue 55 ("Transport Is a Consequence of Placement, and
Only Sometimes a Guarantee") into a practical form.

## 1. The rule

A contract states only what can be held in practice. A link's transport time
is one of two kinds:

| Kind | Examples | What the contract states | How it is held |
|---|---|---|---|
| **Guaranteed by configuration** | TSN with a Qbv schedule; CAN with a fixed schedule; a pointer handoff in one process | a bound derived from the configuration (an extension) | the configuration itself |
| **Best effort** | serial + zenoh-pico; TCP; DDS between processes or hosts | a **measured** bound with its **source** | **checked against runs** (`check --against`): a message in a run older than the bound is `transport-exceeded` |

A best-effort link's latency is mostly platform behaviour (batching, flush
timing, scheduling of the network tasks, gateways, queueing behind other
traffic). No formula gives its worst case: a derivation from bytes and
bandwidth yields a lower bound beside the same measured number. So it is not
derived. It is measured, written down with where it came from, and checked
against later runs, so a broken assumption shows up in the next run that
breaks it rather than staying silent. (A runtime monitor, the
assume-and-monitor pattern of AUTOSAR E2E timeouts, was L5 and is not
adopted: see L5.)

## 2. Pieces

### L1. Links are named objects

```yaml
links:
  host_serial:
    max_transport: 56ms
    source: "measured: <run>, observed max 46.42 ms + 20 %"
topics:
  /system/operation_mode/availability:
    link: host_serial
```

- A topic, or one subscriber of it, names its link; the link carries the one
  number. Today a contract repeats `max_transport` per subscriber.
- `source:` is required on a best-effort link (`link-unsourced`, a warning;
  an error under `--strict`). Free text: a run directory, a date, a method.
- `max_transport` written on a topic or subscriber stays legal (one release
  of overlap) and is reported as `transport-inline` (info) suggesting a link.

### L2. One reader for transport

Issue 55 step 1 (play_launch #0042): the per-subscriber value reaches the
model, one derivation computes every edge's transport, and the other two
readers (`scope_budget`'s flat sum, `manifest_graph`'s precedence) read it.
Gate: a two-subscriber fixture with a per-subscriber override, both consumers
asserted equal. This is a correctness fix on its own: contracts that use
per-subscriber overrides can be totalled differently by the walk and the
scope check today.

### L3. Measured proposal

`play_launch measure <run-dir>` proposes each link's `max_transport` from the
interception record (publish to take, per message) as observed max x margin
(default 1.2), with the run named as the source. On one host
`CLOCK_MONOTONIC` is comparable; across hosts it is not: there the proposal
uses the receiver-side stamp age if the clocks are synchronised (stated on
the link: `clock: synced`), else it reports the inter-arrival gap only and
says the transport could not be measured.

### L4. Checked against runs

`check --against <run-dir>` adds `transport-exceeded` (an error: a message
in the run took longer than its link's bound) and `link-unmeasured` (a
warning: the run carries no samples for a declared link).

### L5. Runtime monitor (NOT adopted)

Not adopted (user decision, play_launch phase 97 P5): on the safety island's
unsynchronised UART such a monitor could only time the gap between arrivals,
which would replace the receiver's own 500 ms silence detector and its
reaction with about 31 ms of margin over the observed gaps. `monitor:` is
parsed and not read; no `link-unmonitored` rule exists. The design as
written, kept for reference:

A best-effort link's `monitor:` makes the receiver check each message's age
against the link's bound (with synchronised clocks), or the gap between
arrivals against the sender's period + jitter + bound (without). A miss is a
link fault that starts the declared reaction, and the reaction walk charges
it like any other detection (`detection = bound + check period`). A
best-effort link on a reaction route with no monitor is `link-unmonitored`
(the phase 71 rule for assumptions without a reaction).

### L6. Guaranteed links (later)

An extension derives a link's bound from a configuration that guarantees it
(a TSN gate control list, a CAN schedule). Only these links may omit
`source:`. Not built until a consumer has such a link (the
AUO SGSS project's TSN phase).

## 3. Not done, on purpose

- No derivation of a best-effort link from baud rate, frame size or protocol
  overhead. At most a cheap `transport-below-wire` warning where the wire
  time is trivial to compute (serial, CAN), and only if it is ever asked for.
- No per-class platform figures (issue 55 step 2) for now: a named link with
  a measured source covers the same need with less machinery.
