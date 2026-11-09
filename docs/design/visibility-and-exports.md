# Visibility: which consumers a topic may have

Status: **draft** (2026-11-09). Proposes grammar for v0.4. Realizer
companions: nano-ros RFC-0125 (`docs/design/0125-topic-visibility-and-locality.md`,
the board realizer) and play_launch `docs/design/topic-visibility-linux.md`
(the Linux realizer).

## The problem

A contract states every endpoint a launch tree declares, but it cannot say
who else may use a topic. Every topic is implicitly public: any process on the
graph, a `ros2 topic echo`, a bag, a bridge or another launch tree may
subscribe at any time. So no realizer can treat a topic as internal, and every
one pays for the wire on every topic:

- a publisher declares itself, announces liveliness and serializes every
  sample, even when its only subscriber is in the same process or image;
- on a microcontroller, each declared endpoint costs a fixed slot and a wire
  buffer sized for the worst case, whether anything remote ever listens;
- discovery traffic carries the whole graph to every participant.

The one mark the grammar has is the opposite one: `external:` says a side of a
topic lives OUTSIDE the tree. Nothing says a topic must stay inside.

Where this is solved today, it is solved per transport and by hand: PX4's
`dds_topics.yaml` and its Zenoh `pub.csv`/`sub.csv`, zenoh-bridge-ros2dds
allow/deny regexes, the DDS Router allowlist, `domain_bridge` YAML, DDS
partitions. Each is a second list of the same boundary in its own dialect, and
each drifts: a PX4 topic present in one release's list and missing in the
next appears to a user as a topic that silently does not exist.

## Related work

- **AUTOSAR compositions.** Components connect through assembly connectors
  inside a composition; only delegation ports face out. The deployment
  mapping (component to ECU) comes later, and the RTE generator turns each
  connector into an in-ECU call or a bus signal from that mapping. Interface
  and placement are separate inputs, and the data path is derived. This
  document follows that split.
- **Programming-language visibility.** Rust `pub`, `pub(crate)`,
  `pub(super)`; a module's interface is what it exports, and everything else
  is checked to be unused outside it.
- **ROS 2.** Names have scopes (`~/x`, relative names) but no visibility.
  `ROS_AUTOMATIC_DISCOVERY_RANGE` is per host or process, never per topic, and
  rmw_zenoh does not read it. rclcpp intra-process is per node, and adds a
  path rather than restricting one. A `_`-prefixed topic is hidden from tools
  only.
- **Zenoh.** Publishers can be restricted to `SessionLocal` (`allowed_destination`,
  unstable), and a publisher can learn whether any subscriber matches it.

## Two questions, two owners

| question | answered by | carried in |
|---|---|---|
| **Visibility**: which consumers may this topic have? | the author, in the contract | the contract and the SystemModel (this crate) |
| **Locality**: where do its endpoints actually run? | placement (`[host.*]`) and each realizer's own image/process assignment | derived; never written by the author |

Visibility is runtime-agnostic: it means the same to every realizer, so by
`model-boundary.md`'s rule it belongs in the model. Locality depends on
placement and on a realizer's grouping of nodes into processes or images, so
each realizer derives it, with one shared function from this crate so that no
two realizers compute it differently.

## Grammar

### `exports` on a manifest scope

A manifest scope is a contract file's root or an inline `includes.<name>`
entry. A scope with `exports:` is CLOSED: a topic, service or action declared
inside it may be used from outside the scope only if it is exported.

```yaml
# perception.contract.yaml, beside perception.launch.py
exports:
  out: [objects, occupancy_grid]   # outside may subscribe
  in:  [~/enable]                  # outside may publish
  services:
    server: [~/reset]              # outside may call
nodes: ...
```

| key | kind | meaning |
|---|---|---|
| `exports.out` | requirement | topics a consumer outside the scope may subscribe to |
| `exports.in` | requirement | topics a producer outside the scope may publish to |
| `exports.services.server` / `.client` | requirement | services served to, or called from, outside |
| `exports.actions.server` / `.client` | requirement | likewise for actions |

Rules:

1. **Absent means open.** A scope without `exports:` restricts nothing, and
   its children's exports pass through it unchanged. Stock and unannotated
   trees keep today's meaning exactly.
2. **Names are relative to the scope's namespace,** resolved after the
   include site's namespace push and remaps. Re-namespacing an include never
   breaks its exports; a remap at the include site renames the exported
   interface, as an import alias does.
3. **Re-export is explicit.** A parent that is itself closed lists which of
   its children's exports it exposes; a child export it does not list is
   internal at the parent's level.
4. **Direction is part of the interface.** `out` and `in` follow PX4's
   `/fmu/out` and `/fmu/in`: exporting a topic for reading does not let the
   outside publish into it.
5. **`external:` must be exported.** A topic marked `external` inside a
   closed scope must appear in that scope's exports on the matching side;
   otherwise the contract contradicts itself (`external-not-exported`).
6. **Static.** Exports do not depend on `if`/`unless` or launch arguments. An
   exported topic whose only endpoint is switched off has no endpoint; that is
   not an error.

### `visibility` shorthand

For a single topic, `topics.<name>.visibility: private | internal | public`
(requirement) avoids an `exports:` block:

- `private`: no consumer outside the declaring scope (same as omitting it from
  a closed scope's exports);
- `internal`: no consumer outside the contract tree's root, including tools;
- `public`: the default.

A topic may carry `visibility` or be governed by its scope's `exports`, not
both (`visibility-and-exports`).

## The model

The SystemModel gains, per topic, service and action:

- `visibility`: the widest scope that may hold a consumer, as the scope's
  path in the include tree (or `public`);
- `closed`: whether every endpoint the model knows lies inside that scope,
  with the reason when it cannot say (`unknown_endpoints: [<node>...]`).

A node whose endpoints are not fully stated (no contract and no code probe), or
a topic name computed at runtime, makes `closed` unknown for every topic of its
scope. Unknown is never treated as closed.

## Checker rules

| rule | severity | fires when |
|---|---|---|
| `export-violated` | error | an endpoint outside a scope uses a topic the scope does not export, on that side |
| `external-not-exported` | error | rule 5 above |
| `visibility-and-exports` | error | a topic carries both forms |
| `closure-unknown` | warning | a non-public topic's `closed` is unknown; names the nodes that make it so |
| `export-unused` | info | an exported topic no endpoint outside the scope uses, in any configured root |

## Locality, derived once

This crate provides `locality(model, placement, grouping) -> per topic`, where
`grouping` is the realizer's own assignment of nodes to processes or images.
It returns one class per topic:

| class | meaning |
|---|---|
| `local` | every endpoint in one process or image |
| `host` | every endpoint on one host |
| `bounded(<scope>)` | crosses hosts, but no endpoint outside `<scope>` |
| `open` | public, or `closed` unknown |

Both realizers call this one function. A realizer that computes locality on
its own is the two-derivations defect this crate exists to prevent.

## What a realizer may do with it

The contract grants permission; it never demands an optimization.

- **Tier 1, any topic, no declaration needed:** send on the wire only while a
  remote consumer is matched; deliver locally always. Safe for a public topic,
  because a tool that subscribes later becomes a remote match.
- **Tier 2, `local` and non-public and `closed`:** remove the wire side
  entirely: no declaration, liveliness, slot or wire buffer.
- **Bounded:** restrict the wire to the scope (router ACLs, partitions,
  bridge allowlists), all generated from `exports`, none written by hand.

A realizer must fail loudly, never silently: an attempt to consume a
non-exported topic from outside its scope is reported with the topic, the
scope and the consumer. A realizer offers one debug switch that re-exposes a
scope for a session and reports that the optimization is off.

## Out of scope

- Per-boundary QoS (rate caps, downsampling) on an export. It fits here
  (`exports.out: [{ name, max_rate_hz }]`), and nano-ros issue 0760's ingress
  pacing is the same concern; it is a follow-up.
- Security. Visibility is a design contract, not access control; SROS2
  governance is the tool for that.

## Open questions

1. Should `internal` (tree root) exist at all, or is a closed root scope
   enough?
2. Should `export-unused` read observed runtime data (play_launch's monitor)
   as well as the static graph?
