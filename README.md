# ros-launch-manifest

Static checking, runtime monitoring contracts, and portable scheduling
specification for ROS 2 launch files.

A **launch manifest** (contract file, `<stem>.contract.yaml`) is a sidecar
YAML file that describes what a launch file contributes to the
communication graph: nodes, topics, services, timing contracts, and the
hazards and operational modes that say what happens when a contract is
violated. Where a launch file says *what to run*, the manifest says
*what communicates, at what quality, and what the system owes when that
quality is lost*. A **platform file** (`<stem>.system.<target>.yaml`)
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
- **[Design Issues](docs/design-issues.md)** — the decision log: every
  design question raised against the spec, with its resolution. A
  historical record — entries keep the vocabulary of the day they were
  decided, and each carries a status line saying whether it still holds.
- **[Slides](docs/slides.md)** — a marp deck; the fastest orientation.

## Crate Structure

Five workspace members:

| Crate     | Description                                                                  |
|-----------|------------------------------------------------------------------------------|
| `types/`  | Manifest data types, span-tracking YAML parser, substitution, condition filtering. The grammar is the field table (`types/src/field_table.rs`); an unknown key is a parse error |
| `check/`  | Single-manifest static validation (**19 rules**, incl. Z3 satisfiability), diagnostic emitters |
| `sched/`  | Portable scheduling spec: platform files, the mapper registry, chain-aware ranking, the POSIX realizer, legacy TOML bridge |
| `model/`  | SystemModel system-config types (`execution.tiers` reuses `sched::TierDef`)  |
| `derive/` | The ONE derivation of the mapper's input from a resolved model — `mapper_input_from_model`, `resolve_chains`, `DeriveFacts`, `DeriveReport`. Both consumers call it; neither reimplements it (design issue #52, shipped in v0.1.37) |

Consumers: **play_launch** (via `ros-launch-resolve`) on Linux;
**nano-ros** for RTOS targets. Checks that need the MERGED tree —
`consistency`, `budget-overflow`, the critical-path form of
`scope-budget`, `scope-sampling-feasibility`, `jitter-feasibility`,
`lifespan-age`, `fault-reaction-budget`, the `derivable-*`/`*-mismatch`
comparisons, the mode and ladder rules — run in the consumer's merge
layer, not here.

## What a Contract May Say

A contract states **facts** (what the code does) and **requirements**
(what it must achieve). Anything computable from those two is a
**consequence** and is derived, never written: a route between two
topics, a total latency, a downstream rate, a node's criticality. The
`kind` column of the format reference marks which is which.

Top-level blocks: `version`, `args`, `nodes`, `topics`, `services`,
`actions`, `includes`, `paths` (scope paths: two topics and a budget),
`external_topics`, `hazards`, `functions`, `modes`, `severity_levels`.

Retired spellings are parse errors that name their replacement —
`chains:`/`segments:` (state a scope path, the route is derived),
endpoint `jitter` (use `max_jitter` on a path), `correlation` (use
`sync:`), `exclude_patterns` (use `external:`), and the nine `_ms`
name-suffix aliases (write `max_latency: 30ms`, so a unit cannot be
1000x wrong and still parse).

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
