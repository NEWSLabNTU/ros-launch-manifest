# ros-launch-manifest

Static checking, runtime monitoring contracts, and portable scheduling
specification for ROS 2 launch files.

A **launch manifest** (contract file, `<stem>.contract.yaml`) is a sidecar
YAML file that describes what a launch file contributes to the
communication graph: nodes, topics, services, and timing contracts. Where
a launch file says *what to run*, the manifest says *what communicates and
at what quality*. A **platform file** (`<stem>.system.<target>.yaml`)
supplies the scheduling side: a mapper name, platform facts, and explicit
overrides from which per-node scheduling is derived.

## Documentation

Start with [docs/README.md](docs/README.md) — the index with a suggested
reading order. Direct links:

- **[Format Reference](docs/format-reference.md)** — every accepted key,
  **generated** from `types/src/field_table.rs`. Normative: a key it does
  not list is a parse error.
- **[Launch Manifest Specification](docs/launch-manifest.md)** — the
  manifest format: elements, background, worked examples, the tutorial
  pass over the format (including Vocabulary v2: `trigger:`, `sync:`,
  `buffer:`, scope `paths:`), and validation rules.
- **[Contract Theory](docs/contract-theory.md)** — formal foundations:
  latency/drop/age composition, the sampling cost a derived route pays
  at every timer boundary, burstiness, empirical contract derivation.
- **[Contract Verification](docs/contract-verification.md)** — the
  checker as implemented: parsing with spans, the rule registry,
  emitters, and the split between this crate and the consumer's
  cross-scope layer.
- **[Scheduling](docs/scheduling.md)** — the sched crate: platform
  files, the `SchedMapper` registry (`manual`, `rate_monotonic`,
  `deadline_monotonic`, `chain_aware`), the platform-agnostic
  ranking core + POSIX realizer split, and the legacy `system.toml`
  bridge.
- **[Design Issues](docs/design-issues.md)** — open and resolved design
  questions with rationale.

## Crate Structure

| Crate    | Description                                                                  |
|----------|------------------------------------------------------------------------------|
| `types/` | Manifest data types, span-tracking YAML parser, substitution, condition filtering |
| `check/` | Single-manifest static validation (20 rules incl. Z3 satisfiability), diagnostic emitters |
| `sched/` | Portable scheduling spec: platform files, mappers, chain-aware ranking, legacy TOML bridge |
| `model/` | SystemModel system-config types (`execution.tiers` reuses `sched::TierDef`)  |

Consumers: **play_launch** (via `ros-launch-resolve`) on Linux;
**nano-ros** for RTOS targets. Cross-scope checks (`consistency`,
`budget-overflow`, critical-path `scope-budget`,
`scope-sampling-feasibility`, `fault-reaction-budget`, …) run in the
consumer's merge layer, not here.

## Quick Start

```bash
# Check the contracts of a launch tree (provider sidecars + overlay)
play_launch check <pkg> <launch_file>

# Check with an explicit contracts overlay
play_launch check --contracts ./contracts <pkg> <launch_file>

# Inspect the derived scheduling plan with provenance
# (--sched optional when a <stem>.system.<target>.yaml sidecar ships with the launch file)
play_launch check --sched <platform.yaml> --explain <pkg> <launch_file>

# Apply the derived schedule at runtime (default --sched-apply warn)
play_launch launch <pkg> <launch_file> --sched-apply strict
```
