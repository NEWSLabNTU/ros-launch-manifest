# What the SystemModel may carry

Status: **decided** (2026-10-10). Not yet implemented. The work is tracked in
nano-ros phase 486 (`docs/roadmap/phase-486-launch-model-boundary.md`); this
repository's part is W6 (stop projecting) and the W8 retirement row. The
companion rulings are nano-ros RFC-0060's 2026-10-10 amendment and
play_launch phase 86.

## The rule

> **The SystemModel holds what any realizer reads with the same meaning: the
> system's topology, its platform-agnostic contract, and its scheduling. A
> fact that means something to one realizer's BUILD belongs to that
> realizer's own layer, never to the model.**

The `model` crate describes itself as "shared by play_launch (Linux runtime)
and nano-ros (embedded build)". A field only one of them reads costs the
other twice. It must carry a field it ignores, and it cannot tell an ignored
field from a missing one. The test is asked per field: *would play_launch on
Linux, given this field, do the same thing nano-ros does with it?*

## What fails the test today

`system_config::SystemConfigToml` parses nano-ros's `system.toml`, and its doc
comments name the owner of most keys. `apply_to_launch` projects three nano-ros
build facts into the model:

| Model field | From | Why it fails |
| --- | --- | --- |
| `Execution.features` | `[system] features`, `[param_services]` | nano-ros capability switches. An rclcpp node has parameter services unless its code opts out; the switch is nano-ros build policy |
| `lifecycle_autostart` set on every lifecycle node from one table | `[lifecycle] autostart` | the per-node field passes (Jazzy `LifecycleNode(autostart=)`); a system-wide default is nano-ros boot policy |
| `Deploy.target = mcu:<board>`, `Deploy.extra` | `[deploy.<n>]` `board`, `framework`, `profile`, `optimize`, `features`, `[deploy.<n>.nros]` | these describe a build, as this crate's own comment says. nano-ros moved them to its own `[image.*]` and `[board_config.*]`; no tracked nano-ros `system.toml` writes `[deploy.*]` |

These pass, and stay:
- placement (`[host.*]`, `[deploy.<n>] kind` / `nodes` / `launch`);
- `[tiers.*]` and `group_tiers` (scheduling, in the shared `sched` schema);
- `[[component]] params` / `params_files` (ROS parameter values);
- `[system] rmw` / `domain_id` / `locator`, `[[transport]]`, `[[bridge]]`;
- per-node `lifecycle` and `lifecycle_autostart`. The enum keeps
  `Configure`: a launch file can state only `Active`, but "configure and wait"
  means something to any lifecycle manager.

## What changes here

1. **Stop projecting (nano-ros phase 486 W6).** `apply_to_launch` stops
   writing:
   - `Execution.features`;
   - lifecycle defaults onto nodes;
   - `Deploy.target = mcu:<board>`;
   - `Deploy.extra`.

   The per-node lifecycle fields are now filled only from the launch file
   (play_launch phase 86) and the contract.
2. **Stay lax about keys this crate does not own.** `SystemConfigToml` keeps
   PARSING `[param_services]`, `[lifecycle]`, `[system] features` and the
   build fields of `[deploy.*]`, into nothing. The same file is read by
   nano-ros's strict parser, which owns its validation. If rlm refused a key
   it no longer projects, every valid nano-ros `system.toml` would fail to
   resolve.
3. **Old models still load.** `Execution.features` and `Deploy.extra` become
   read-tolerant: they deserialise and nothing reads them. This is the
   `jitter_ms` precedent (phase 68), and the golden fixture keeps them on disk
   as the evidence. `SCHEMA_VERSION` stays 1, because a reader of a new model
   loses nothing it could act on.
4. **Retirement (nano-ros phase 486 W8).**
   - Once nano-ros's re-exports drop them, the struct fields go:
     `SystemConfigToml.lifecycle`, `.param_services`,
     `SystemDefaults.features`, and `DeployBlock`'s build fields.
   - The read-tolerant model fields are deleted one minor release after
     nano-ros has bumped past W6, provided no tracked model or fixture except
     the golden one carries them.

## Where those facts go instead

nano-ros writes them to its own overlay, `nros.toml`, beside the model it
resolves (`<ws>/build/nros/models/<bringup>/`). play_launch has nothing to
move: none of the three had a Linux meaning, and a `system.toml` passed
through the legacy `--sched` bridge parses as before.
