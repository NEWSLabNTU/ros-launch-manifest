# Scheduling Specification Crate

**Crate:** `ros-launch-manifest-sched` (sibling to `ros-launch-manifest-types` in the `src/ros-launch-manifest/` workspace)

**Purpose:** Portable scheduling specification shared between `play_launch` (Linux RT) and `nano-ros` (RTOS). Authors specify generic tier definitions, deadline/period/budget requirements, and node-to-tier binding once; platform-specific placement (priority, scheduler class, core affinity, stack) is supplied per target in the same form on every platform.

**Key invariant:** The generic layer carries no priority numbers, ensuring byte-identical portability across platforms.

## Design Principles

1. **Two orthogonal axes, kept separate:**
   - Generic (portable) layer: tier class, deadline, period, budget, deadline policy, spin period
   - Platform-specific layer: priority, scheduler class, core affinity, stack, optional per-platform deadline override

2. **Tier membership is the callback group.** Node-level tier assignment replaces scattered per-package callback-group authoring. A tier's members (the set of nodes assigned to it via the `[[assign]]` table) form the execution group for that platform.

3. **Sparse selector binding (UX).** Users author minimal `[[assign]]` rules; selectors match by explicit node name or launch-scope path. A node matched by no rule synthesizes into a `default` tier.

4. **No priority leakage.** Schema enforcement via `serde(deny_unknown_fields)` on the generic `TierDef` head prevents stray priority numbers from breaking portability.

## TOML Schema

**File format:** one system-level TOML file with two sections:

```toml
# ===== GENERIC (portable — byte-identical across platforms) =====
[tiers.control]            # tier head: naming + generic requirements
class = "real_time"        # scheduling class: best_effort | real_time | time_triggered | interrupt
deadline_us = 50000        # generic deadline (derived from callback frequency)
period_us   = 20000        # for periodic / time_triggered
budget_us   = 5000         # EDF/sporadic execution budget
deadline_policy = "warn"   # deadline breach handling: ignore | warn | skip | fault
spin_period_us  = 1000     # executor spin period

[[assign]]                 # binding = tier membership
tier  = "control"
nodes = ["ndt_localizer", "ekf_localizer"]   # explicit node selectors
[[assign]]
tier  = "perception"
scope = "/perception/lidar"                  # launch-scope subtree selector
# any node matched by no rule → synthesized "default" tier

# ===== PLATFORM (same shape per target, values differ) =====
[tiers.control.posix]      # Linux RT (shared with nano-ros)
priority    = 80
sched_class = "SCHED_FIFO"
core        = 1
[tiers.control.freertos]   # RTOS example
priority    = 12
stack_bytes = 8192
deadline_us = 40000        # optional per-platform deadline tighten
```

### Type Reference

- **`SystemSched`** — top-level document: `tiers: BTreeMap<String, TierDef>`, `assign: Vec<AssignRule>`

- **`TierDef`** — `[tiers.<name>]` generic head:
  - `class: Option<String>` — scheduling class
  - `deadline_us: Option<u64>` — generic deadline (microseconds)
  - `period_us: Option<u64>` — callback period (microseconds)
  - `budget_us: Option<u64>` — execution budget (microseconds)
  - `deadline_policy: Option<String>` — deadline breach policy
  - `spin_period_us: Option<u64>` — executor spin period (microseconds)
  - Per-platform sub-tables: `posix`, `freertos`, `zephyr`, `threadx`, `nuttx` (each `Option<TierPlatformSpec>`; `native` is accepted as an alias for `posix`)

- **`TierPlatformSpec`** — `[tiers.<name>.<target>]` concrete placement:
  - `priority: i64` — OS/RTOS priority (i64 to admit Zephyr negative coop priorities)
  - `stack_bytes: Option<u32>` — task stack size
  - `core: Option<u32>` — CPU core index (SMP pinning; `None` = unpinned)
  - `sched_class: Option<String>` — POSIX scheduler class (e.g. `"SCHED_FIFO"`, `"SCHED_RR"`)
  - `preempt_threshold: Option<i64>` — ThreadX preemption threshold
  - `deadline_us: Option<u64>` — per-platform deadline override (tighter than generic head)

- **`AssignRule`** — `[[assign]]` sparse binding:
  - `tier: String` — target tier name
  - `nodes: Vec<String>` — explicit node selectors (default empty)
  - `scope: Option<String>` — launch-scope subtree selector

- **`ResolvedTier`** — one tier after resolution for a target:
  - Generic policy fields: `class`, `period_us`, `budget_us`, `deadline_us` (effective value), `deadline_policy`, `spin_period_us`
  - Platform placement: `priority`, `core`, `sched_class`, `stack_bytes`, `preempt_threshold`
  - `members: Vec<String>` — sorted list of assigned node names

- **`ResolvedTierTable`** — ordered tier table for one platform:
  - `tiers: Vec<ResolvedTier>` — sorted highest-priority-first
  - `is_single_tier()` — true when system collapsed to single `default` tier

## Resolver

### Signature

```rust
pub fn resolve(
    tiers: &BTreeMap<String, TierDef>,
    assigns: &[AssignRule],
    nodes: &[SchedNode],
    target: &str,  // "posix" | "freertos" | "zephyr" | etc.
) -> Result<ResolvedTierTable, SchedError>
```

Where `SchedNode` carries:
- `name: String` — fully-qualified node name (e.g. `/perception/lidar/ndt_localizer`)
- `scope: String` — node's namespace / scope path (e.g. `/perception/lidar`)

### Selector Precedence

1. **Explicit node selectors** — highest precedence. Match by full FQN or bare name (last path segment).
2. **Scope subtree selectors** — middle precedence. Match by exact scope or descendant (nodes under the scope path).
3. **Synthesized `default` tier** — lowest precedence. Unmatched nodes fall here.

A node claimed by two rules at the **same precedence level** (node-vs-node or scope-vs-scope) for different tiers is a `NodeMatchedByMultipleTiers` error. An explicit node rule always wins over a scope rule for the same node — no error is raised.

### Effective Deadline Rule

For each resolved tier, the effective deadline is:
- Platform override (`spec.deadline_us`), if present
- Otherwise, generic head (`def.deadline_us`)

### Output

- Tiers ordered highest-`priority` first
- Degenerate case (no `[[assign]]` rules): single synthesized `default` tier with priority 0 and all nodes as members (no platform lookup needed)
- All other cases: platform sub-table lookup required for each named tier

## Validation (Error Handling)

`SchedError` enum variants:

- **`Parse`** — TOML parse failure
- **`UnknownTier { tier }`** — an `[[assign]]` references a tier with no `[tiers.<tier>]` definition
- **`UnknownNodeSelector { selector }`** — an explicit node selector matches no node in the system
- **`UnknownScopeSelector { selector }`** — a scope selector matches no node's scope
- **`NodeMatchedByMultipleTiers { node, tier_a, tier_b }`** — a node resolved to two different tiers (conflict)
- **`MissingPlatformSpec { tier, target }`** — a populated tier lacks the required `[tiers.<tier>.<target>]` sub-table for the resolve target

## Consumers

### play_launch (Linux RT) — Validate Now, Apply Later

**Now (v1, implemented):** `play_launch check --sched <system.toml>` parses the scheduling spec, loads `record.json`, extracts node names and scope paths, resolves for `target = "posix"`, runs all checks, and reports diagnostics. **No change to how nodes are spawned.**

**Phase 2 (documented, not implemented):** an apply-layer consumes the resolved `posix` `ResolvedTierTable` and applies:
- `sched_setscheduler` (SCHED_FIFO/RR from `sched_class`, priority from `priority`)
- `sched_setaffinity` (core from `core`)

This requires `CAP_SYS_NICE` or root. The crate exposes the resolved `posix` numbers as the hook point; `play_launch` fills the syscall layer later.

### nano-ros (RTOS) — Full Apply

`codegen-system` calls `resolve(..., target = "freertos" | "zephyr" | ...)`, bakes the `ResolvedTierTable` into the system plan, and emits one task/executor per tier with the resolved platform numbers. The central `[[assign]]` table replaces scattered per-package callback-group authoring.

## Distribution & Cross-Repo Sharing

- **Authored in:** `play_launch` (`src/ros-launch-manifest/sched/`)
- **Dependencies:** `serde`, `thiserror` only. Pure host code, no `no_std`, no runtime deps.
- **Vendoring:** `nano-ros` vendors it via git submodule (same mechanism as `ros-launch-manifest`) and pins the commit in `nros-sdk-index.toml`.
- **Portability:** Generic layer byte-identical across all platforms. Platform-specific placement lives in per-target sub-tables, so the same system TOML (generic fields) resolves identically on all platforms.

## Usage Example

```toml
[tiers.control]
class = "real_time"
deadline_us = 50000
period_us = 20000

[tiers.control.posix]
priority = 80
sched_class = "SCHED_FIFO"
core = 1

[tiers.perception]
class = "real_time"
deadline_us = 100000

[tiers.perception.posix]
priority = 60
core = 2

[[assign]]
tier = "control"
nodes = ["ndt_localizer", "ekf_localizer"]

[[assign]]
tier = "perception"
scope = "/perception/lidar"
```

On `posix`, this resolves to:
- **control tier:** priority 80, SCHED_FIFO, core 1, members `[ndt_localizer, ekf_localizer]`
- **perception tier:** priority 60, no scheduler class/core (inherited from platform sub-table if specified), members include all nodes under `/perception/lidar`
- **default tier:** priority 0, all other nodes

## Design of Record

See `docs/superpowers/specs/2026-07-01-shared-scheduling-crate-design.md` (the canonical design document) for detailed rationale on schema orthogonality, selector binding, platform separation, and migration strategy.
