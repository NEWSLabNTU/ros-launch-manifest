# Introduction — the spec and the theory

This crate is the contract layer under
[`play_launch`](https://github.com/NEWSLabNTU/play_launch): ROS-free, it says
what a launch scope declares, what it must achieve, and what follows from
those two. Two documents carry the weight, and they answer different
questions.

## The spec — [`launch-manifest.md`](launch-manifest.md)

A manifest is what one launch scope *declares*: its nodes, the endpoints each
publishes and subscribes, the topics those wire to, and the requirements that
must hold.

One rule governs the whole grammar — **a contract states facts and
requirements, never consequences.** A trigger is a fact ("this path fires on a
timer at 100 Hz"), a budget is a requirement ("this route must finish within
40 ms"), and anything computable from those two — a route, a total, a
downstream rate, a node's criticality — is *derived*. Writing a consequence by
hand makes a second copy of something the tool already knows, and two copies
can disagree; several rules exist only to report it when they do.

Read it when writing or reviewing a contract. It covers every top-level block,
both endpoint spellings (names are local to a node; `topics:` wires them as
`node/endpoint`), conditions, QoS, drop budgets, hazards with their reaction
edges, and operational modes with the fallback ladder. It also carries a table
of retired spellings, each naming its replacement — the vocabulary has been
through four rounds of subtraction, and that table is how you migrate.

## The theory — [`contract-theory.md`](contract-theory.md)

Why the vocabulary is that shape, and what the tool computes from it.

The core is a composition algebra. Latencies sum along a series; a fork-join
takes the **max** over parallel branches (`max(50,30)+20` is 70, not 100); and
a fan-in publishes at the **sum** of its input rates without `sync:`, and the
min only with it — because one callback fires once per message on *each* topic
it is registered for.

From those it derives what nobody should be asked to write by hand: topic
rates, the critical path and its sampling cost, how fast a fault must be
detected (FDTI) and reacted to (FRTI) to fit inside a hazard's fault-tolerant
time interval, and a node's criticality as a consequence of the hazards it
feeds, detects or reacts to — rather than a free `high | medium | low` label.

Read it when you want to know *why* a rule fired, or before proposing a new
field: it is where the argument lives for what belongs in a contract at all.
Every formula cites the function that implements it.

## The rest of the set

- [`format-reference.md`](format-reference.md) — generated from the field
  table, so it is the oracle the prose is checked against. If the two
  disagree, this one is right.
- [`contract-verification.md`](contract-verification.md) — the 19 in-crate
  rules, and the cross-scope rules that live in the consumer.
- [`scheduling.md`](scheduling.md) — the platform-file schema and the mappers
  that turn a contract into priorities.
- [`design-issues.md`](design-issues.md) — the decision log. It is **history**:
  entries keep the vocabulary of the day they were written, and their status
  lines say which have since been superseded.
- [`README.md`](README.md) — the full index and a reading order.
