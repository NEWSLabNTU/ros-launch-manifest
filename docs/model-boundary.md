# What the SystemModel may carry

Status: **decided** (2026-10-10, revised 2026-10-12). **W6 implemented in v0.1.55** (the model stops carrying the realizer facts); **W8 implemented in v0.1.57** (the system config and the model name no realizer's facts; `[deploy.*]` is gone, placement is `[host.*]` alone).
The cross-repository work is tracked in nano-ros phase 486
(`docs/roadmap/phase-486-launch-model-boundary.md`); this repository's part
is W6 (stop writing realizer facts, make the system config neutral) and the
W8 retirement rows. The companion plan on the Linux side is play_launch
phase 86.

The 2026-10-12 revision corrects three statements the first version made
about other repositories, each checked with `git grep` on their `main`, and
adds one rule (the system config is this crate's own format):

- the system-wide lifecycle default is projected onto nodes by play_launch's
  resolver, not by `apply_to_launch` (which writes the execution layer only);
- no realizer reads `Deploy.target`, so `Target::Mcu` is not kept as
  placement — it has one writer (`[deploy.<n>] board`) and no tracked config
  uses it;
- `Deploy.extra` also carries `host_name` and `deploy_name`; nobody reads
  either, and they leave with the rest of `extra`;
- `SCHEMA_VERSION` is 2 since v0.1.51 (list parameters), not 1.

## The rule

> **The SystemModel holds what any realizer reads with the same meaning: the
> system's topology, its platform-agnostic contract, and its scheduling. A
> fact that means something to one realizer's BUILD belongs to that
> realizer's own layer, never to the model.**

A field only one realizer reads costs every other realizer twice. It must
carry a field it ignores, and it cannot tell an ignored field from a missing
one. The test is asked per field: *would a different realizer, given this
field, do the same thing with it?*

The same rule applies one level up, to this crate itself: **this crate names
no realizer.** Its types, its system-config format and its doc comments
describe launch systems, not any one tool that consumes them.

## What fails the test today

`apply_to_launch` writes four facts that only one realizer reads:

| Model field | From | Why it fails |
| --- | --- | --- |
| `Execution.features` | `[system] features`, `[param_services]` | capability switches of one embedded build. A ROS 2 node has parameter services unless its own code opts out; the switch is build policy |
| `Deploy.extra` (`kind`, `target`, `framework`, `profile`, `optimize`, `features`, `[deploy.<n>.nros]`) | `[deploy.<n>]` | these describe a BUILD, as this crate's own comment says |
| `Deploy.extra` (`host_name`, `deploy_name`) | `[host.<n>]`, `[deploy.<n>]` | the key of the table that placed the node. Nothing reads it; the facts the placement carries (`domain`, `locator`, `rmw`) are typed fields already |
| `Deploy.target` (`Linux`, `Mcu { board }`) | `[deploy.<n>] board` | no realizer reads it. Its only consumer slices nodes per board for a `[deploy.*]` table that no tracked config writes |

And one realizer-side projection, outside this crate but resting on its
accessor: the system-wide `[lifecycle] autostart` is copied onto every
lifecycle node by play_launch's resolver (`resolve/src/model.rs`), through
`SystemConfigToml::lifecycle_autostart()`. The per-node field passes the test
(launch_ros `LifecycleNode(autostart=)`); one default for the whole system is
boot policy.

These pass, and stay:
- placement facts that change behaviour on any realizer: per-node `domain`,
  `locator`, `rmw` in `Deploy`, from `[host.*]` (its `nodes` / `launch`
  scoping included) and from `[system]`;
- `[tiers.*]` and `group_tiers` (scheduling, in the shared `sched` schema);
- `[[component]] params` / `params_files` (ROS parameter values);
- `[system] rmw` / `domain_id` / `locator`, `[[transport]]`, `[[bridge]]`;
- per-node `lifecycle` and `lifecycle_autostart`. The enum keeps
  `Configure`: a launch file can state only `Active`, but "configure and wait"
  means something to any lifecycle manager.

## The system config is this crate's format

`system_config::SystemConfigToml` today is a lax copy of one realizer's
`system.toml`: its doc comments name that realizer, and it parses keys only
that realizer means anything by (`[lifecycle]`, `[param_services]`,
`[system] features`, `[deploy.*]`'s build fields, `[deploy.<n>.nros]`). That
is two parsers for one file, and the copy decides what reaches the model.

The decision: **`SystemConfigToml` is the integrator's system config for
the shared layers, and this crate owns its format.**

- It declares exactly the tables whose facts pass the test above: `[system]`
  (`rmw`, `domain_id`, `locator`), `[host.*]`, `[tiers.*]`, `[[component]]`,
  `[[transport]]`, `[[bridge]]`.
- **Every other table is ignored by design**, not refused. A realizer extends
  the same file with its own tables and validates them itself. This is
  what lets one `system.toml` serve several realizers without this crate
  knowing any of them.
- A realizer that wants a strict parse deserialises the shared keys with
  this struct and its own tables with its own, from the same document, and
  refuses only what BOTH ignored, rather than keeping a second parser of the
  shared keys. (`#[serde(flatten)]` cannot do this: serde does not combine
  it with `deny_unknown_fields`.)
- No doc comment in this crate names a realizer. History and cross-references
  live in the CHANGELOG and in the realizers' own docs.

## What changes here

1. **Stop writing realizer facts (W6).** `apply_to_launch` stops writing
   `Execution.features`, `Deploy.extra` and `Deploy.target`. play_launch
   deletes its lifecycle-default loop in the same window (its phase 86 W0).
2. **Stay lax about tables this crate does not own.** The realizer-only keys
   keep PARSING, into nothing, until the realizer that owns them has moved
   its readers (phase 486 W8). Then they leave the struct, and the
   "ignored by design" rule above covers them.
3. **Old models still load.** `Execution.features`, `Deploy.extra` and
   `Deploy.target` become read-tolerant: they deserialise and nothing reads
   them (the `jitter_ms` precedent, phase 68), and the golden fixture keeps
   them on disk as the evidence. Removing writes needs no schema bump: a
   reader of a new model loses nothing it could act on. `SCHEMA_VERSION`
   stays at its current value, 2.
4. **Retirement (W8, done in v0.1.57).**
   - The realizer-only struct fields went: `SystemConfigToml.lifecycle`,
     `.param_services`, `.deploy` (the whole `[deploy.*]` table, its
     placement half included, since `[host.*]` covers placement),
     `SystemDefaults.features`, and the `lifecycle_autostart()` accessor.
   - The read-tolerant model fields, and `Target` itself, went in the same
     release: both known consumers pinned v0.1.56, past W6, and no tracked
     model or fixture except the golden one carries them.
   - The doc comments name no realizer (the rule above, v0.1.56).

## Where those facts go instead

Into each realizer's own layer, which it writes beside the model it
resolves, from its own tables in the same `system.toml`. A realizer with no
such facts has nothing to move.
