# Introduction — the spec and the theory

This crate is the contract layer under
[`play_launch`](https://github.com/NEWSLabNTU/play_launch): ROS-free, it says
what a launch scope declares, what it must achieve, and what follows from those
two. Two documents carry the weight, and they answer different questions.

## The problem it exists for

A ROS 2 launch file says what runs. It does not say what any of it is supposed
to *achieve*, so the timing requirements live in a design document, the
scheduling priorities live in a separate hand-written table, and nothing
compares them. When they disagree, the system tells you by missing a deadline.

The obvious fix — write the requirements down beside the launch file — has a
failure mode of its own, and it is worth stating before the grammar, because
the grammar is shaped by it. Measured on a three-node fixture in `play_launch`:
the number `100` appeared **nine times** in one contract, once as a timer's
`rate_hz` and eight more as topic rates and `min_rate_hz` floors that followed
from it. Roughly a third of the file was a second copy of something already
stated. Every copy is a chance to disagree, and a contract that disagrees with
itself is worse than none: it reports clean while describing a system nobody
runs.

So:

> **A contract states facts and requirements, never consequences.**

A trigger is a fact — "this path fires on a timer at 100 Hz". A budget is a
requirement — "this route must finish within 40 ms". A route, a total, a
downstream rate, a node's criticality: all computable from those two, so all
*derived*. Deleting the eight redundant copies from that fixture left the
derived schedule **byte-identical**.

## The spec — [`launch-manifest.md`](launch-manifest.md)

A manifest is what one launch scope *declares*: its nodes, the endpoints each
publishes and subscribes, the topics those wire to, and the requirements that
must hold. Here is a real one, from Autoware's ground-segmentation node
(`types/tests/fixtures/scan_ground_filter.contract.yaml`, trimmed):

```yaml
version: 1

nodes:
  scan_ground_filter:
    sub:
      input:
        state: true
    pub:
      no_ground_pointcloud:
        min_rate_hz: 10
    paths:
      main:
        input: input
        output: [no_ground_pointcloud]
        max_latency: 30ms

topics:
  /sensing/lidar/concatenated/pointcloud:
    type: sensor_msgs/msg/PointCloud2
    sub: [scan_ground_filter/input]
    qos:
      reliability: best_effort
      depth: 1

  /perception/obstacle_segmentation/pointcloud:
    type: sensor_msgs/msg/PointCloud2
    pub: [scan_ground_filter/no_ground_pointcloud]
    rate_hz: 10
```

Three things in that snippet are worth pointing at, because they are the three
questions newcomers ask:

**Endpoint names are local to the node.** `input` and `no_ground_pointcloud`
are this node's names for its own ports, chosen to match its `~/input` and
`~/output` remaps. The `topics:` section is what wires them to ROS topics, as
`node/endpoint` refs. The same word means different things at the two levels,
and that is deliberate: a node's contract stays valid when someone remaps it.

**`state: true` is not decoration.** It says this subscription is read-latest
rather than causal — the node polls the newest scan, it is not woken by it. The
checker uses that to break cycles in the dataflow graph, so a feedback loop
between a controller and a plant is not reported as a causal cycle. Get it
wrong and you get either a spurious `causal-dag` error or a latency chain that
runs backwards through time.

**A path is where timing lives.** Not on the node, not on the topic: on the
named route from an input to its outputs. `max_latency: 30ms` is a requirement
about *this* path, which is what makes a multi-output node analysable — the
route through one output is not charged for the cost of another.

Read the spec when writing or reviewing a contract. It covers every top-level
block, conditions, QoS, drop budgets, hazards with their reaction edges, and
operational modes with the fallback ladder. It also carries a table of retired
spellings, each naming its replacement — the vocabulary has been through four
rounds of subtraction, and that table is how you migrate.

## The theory — [`contract-theory.md`](contract-theory.md)

Why the vocabulary is that shape, and what the tool computes from it. The core
is a composition algebra, and two of its results are the ones people get wrong.

### A fork-join takes the max, not the sum

This fixture (`tests/fixtures/manifest_parallel_pipeline/`) exists to pin it:

```yaml
version: 1

nodes:
  lidar_detector:
    sub: { input: { min_rate_hz: 10 } }
    pub: { out: { min_rate_hz: 10 } }
    paths:
      main: { input: input, output: [out], max_latency: 50ms }

  camera_detector:
    sub: { input: { min_rate_hz: 10 } }
    pub: { out: { min_rate_hz: 10 } }
    paths:
      main: { input: input, output: [out], max_latency: 30ms }

  fusion:
    sub:
      lidar: { min_rate_hz: 10 }
      camera: { min_rate_hz: 10 }
    pub: { out: { min_rate_hz: 10 } }
    paths:
      main:
        input: [lidar, camera]
        output: [out]
        max_latency: 20ms

topics:
  /sensor/raw:
    type: sensor_msgs/msg/PointCloud2
    sub: [lidar_detector/input, camera_detector/input]
    rate_hz: 10
  /perception/lidar_objects:
    type: autoware_perception_msgs/msg/DetectedObjects
    pub: [lidar_detector/out]
    sub: [fusion/lidar]
    rate_hz: 10
  /perception/camera_objects:
    type: autoware_perception_msgs/msg/DetectedObjects
    pub: [camera_detector/out]
    sub: [fusion/camera]
    rate_hz: 10
  /perception/fused_objects:
    type: autoware_perception_msgs/msg/DetectedObjects
    pub: [fusion/out]
    rate_hz: 10

paths:
  pipeline:
    input: /sensor/raw
    output: [/perception/fused_objects]
    max_latency: 70ms
```

The two detectors run in parallel, so the budget the scope path must meet is
`max(50, 30) + 20 = 70ms`, not the flat sum of `100ms`. A checker that adds up
node budgets rejects this system; the topology-aware one accepts it. The 70 is
not written anywhere as a consequence — it is derived, and `max_latency: 70ms`
on the scope path is the *requirement* it is checked against.

### A fan-in publishes at the sum of its input rates

The one that reads backwards until you think about the callback. Without
`sync:`, a node subscribed to two 10 Hz topics fires **twenty** times a second,
not ten — a callback runs once per message on *each* topic it is registered
for. So the output rate is the **sum**. Add `sync:` and the node emits one
output per matched set, and the rate becomes the **min**.

Taking the min in both cases is the natural-looking mistake, and it understates
a fan-in node's load by exactly the factor that decides whether it fits. This
document asserted the min unconditionally until v0.1.39.

### What else is derived

Topic rates propagate from the timers that drive them. Any unknown contributor
— a `once` path, a `spontaneous` one, an external publisher, a cycle — makes
the answer `Unknown` **with a reason**, never zero, because a zero silently
passes every budget.

From a hazard's fault-tolerant time interval the tool derives how fast a fault
must be *detected* (FDTI — the fastest detector among the subscribers that
actually react; a rate floor is not a detector, because nothing fires when a
period merely passes) and how fast it must be *reacted to* (FRTI — a walk over
reaction edges to the safe state, which is not the critical path: the failed
publisher's clock never ticks again). Criticality follows the same way: a node
that feeds, detects or reacts to a hazard takes that hazard's severity, max
over hazards, rather than carrying a free `high | medium | low` label.

Read the theory when you want to know *why* a rule fired, or before proposing a
new field: it is where the argument lives for what belongs in a contract at
all. Every formula cites the function that implements it.

## Remarks a reader will want

**The generated reference wins.** [`format-reference.md`](format-reference.md)
is generated from `types/src/field_table.rs` and enforced by a test. Where the
prose and the reference disagree, the reference is right and the prose is a
bug — that is how v0.1.39 found six live fields the specification had never
mentioned.

**Unknown keys are errors, and that is recent.** Until phase 69 a typo was
discarded in silence: `max_latencyy: 5ms` deleted a budget and the checker
reported clean, exit 0. Worse, `rate_hzz: 100` deleted the very declaration
the mismatch rule reads — so the one diagnostic pointing at the mistake was
silenced *by* the mistake. Unknown keys now fail with a suggestion, and a
manifest that no longer parses is reported as such rather than skipped.

**Diagnostics name the path, not just the problem.** They read like

```
warning[dangling-entity]: topic '/perception/objects' has no publishers (no data source) (at topics./perception/objects)
error[rate-hierarchy]: topic rate_hz (10) < subscriber 'planner/objects' min_rate_hz (20) (at topics./perception/objects)
```

The YAML path is part of the line (`check/src/emit/terminal.rs`), so the
finding points at the declaration rather than at the file. There is also a
`codespan`-backed emitter that renders the offending span with the source
excerpt.

**Declaring nothing is allowed.** A manifest that lists nodes and wiring with
no timing at all is valid and useful — the structural rules (dangling
entities, endpoint uniqueness, QoS compatibility, service wiring, causal
cycles) all run without a single number. Timing is opt-in, per path.

**It is checked, not enforced.** The crate analyses; `play_launch` applies. A
contract can say a path must finish in 40 ms, and the checker will tell you
whether the declared parts add up — but only the runtime knows whether it did.
That is the consumer's job: `play_launch check` for the static verdict,
`--enforce-rules` for the running one, and the mappers here for turning a
contract into Linux or RTOS scheduling parameters.

**The decision log is history.** [`design-issues.md`](design-issues.md) keeps
the vocabulary of the day each entry was written — an entry explaining why
`chains:` was removed still says `chains:`. Read its status lines before
quoting it as current.

## The rest of the set

- [`format-reference.md`](format-reference.md) — every accepted key, per
  context. Generated; normative.
- [`contract-verification.md`](contract-verification.md) — the 19 in-crate
  rules, and the cross-scope rules that live in the consumer.
- [`scheduling.md`](scheduling.md) — the platform-file schema and the mappers
  that turn a contract into priorities.
- [`slides.md`](slides.md) — the same story in ~20 slides.
- [`README.md`](README.md) — the full index and a reading order.
