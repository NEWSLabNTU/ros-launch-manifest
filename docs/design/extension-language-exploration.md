# Domain extensions for rlm: a language exploration

Status: design exploration, no implementation. Grammar baseline: `main` at
v0.1.58 (`6211471`). Scope: UX and expressivity of a mechanism that moves
domain vocabulary (vehicle braking, machine safety distance, infusion
occlusion, ...) out of the rlm core into extensions, and the language those
extensions are written in.

Conventions in this document:

- "core" is what `types/src/field_table.rs` must keep; "extension" is a file
  shipped by someone else that adds readable domain keys and derives core
  values from them.
- Numbers in the worked examples are illustrative, chosen to be plausible
  and to exercise branches. They are not data for any product.
- A claim about a standard carries a clause where one was found. `[unverified]`
  marks a clause number, constant or parameter name that was recalled or
  taken from a secondary source and was not checked against the primary
  text. Treat those as pointers, not citations.

---

## 1. Recommendation

1. **Core keeps one Bound primitive and one Predicate primitive; physical
   models and standard profiles move out.** A Bound is a duration, rate or
   count with `source:` and optional `param:` binding, under the readable
   aliases rlm already has (`max_latency`, `ftti`, `window`, `settle`, ...).
   An extension may only *produce* Bounds (`source: derived`, with a
   derivation) and Predicates (a named check that must hold), never new
   checker semantics.
2. **Write extensions in a small, purpose-built, units-typed expression
   language** (working name *rlx*), whose surface borrows from Numbat and F#
   units of measure: `fn`, `let`/`where`, `if ... then ... else`, `min`,
   `max`, `sqrt`, bounded `sum over` a finite table, quantity literals with
   units (`2.5 m/s^2`, `15 km/h`, `300 mmHg`). No recursion, no loops other
   than over a declared finite table, no IO, no clock, no randomness. Total
   by construction, dimension-checked at load time (both branches of every
   `if`), and every `let` is a line of `--explain`.
3. **Worst case is a declared, machine-checked property**, not a hope:
   `worst_case: increasing in entry_speed, decreasing in decel`. The checker
   discharges it with Z3 (already an optional dependency of `check/` for the
   satisfiability rule), using nonlinear real arithmetic over the declared
   parameter domains; a goal Z3 cannot decide is reported `unproved`, and the
   extension may then ship a **Lean 4 proof** as an optional, maintainer-side
   artifact checked in CI, never on the user's machine.
4. **Profiles (severity scales, criticality policy) are data, not code**: a
   YAML table in the extension package is enough for SIL/DAL/ASIL/class
   scales. The only profile feature that wants code (ISO 26262 ASIL
   decomposition) is a 10-line rlx function.
5. **Distribution**: an extension is a versioned file in an ament package's
   `share/<pkg>/rlm/` directory, named in the contract by `use:` with a
   semver range, and its content hash is printed in `--explain` and recorded
   in the derived budget's `source`.
6. **Alternatives, in order**: (a) embed Numbat (MIT/Apache-2.0, Rust, real
   dimension types) with a custom prelude and an AST pass that refuses
   recursion and the impure builtins, as a fast prototype of (2); (b)
   Starlark (starlark-rust) with a host-provided Quantity type, if
   extensions turn out to need general programming (macro-style expansion of
   many core entries), accepting runtime-only unit errors; (c) Nickel if the
   dominant need becomes merging and validating configuration rather than
   formulas. Lua, Rhai, Jsonnet and CEL are workable but weaker; CUE, Dhall,
   Pkl and KCL are poor fits for the formula part; Lean 4 is a proof
   companion, not a host.

Cost of the recommendation, stated plainly: rlx is a language rlm must own.
A parser, dimension checker, evaluator with tracing, and the Z3 encoding are
roughly 3-5 kLOC of Rust plus a reference page and test corpus, and there is
no editor support on day one (a tree-sitter grammar is a later, separate
cost). The Numbat route (6a) cuts the first 60 % of that at the price of a
large dependency whose language keeps moving.

### 1.1 Comparison table

Scores: `++` strong, `+` adequate, `o` workable with host effort, `-` weak,
`--` disqualifying for this use. "Units" means: can a dimension error
(`m/s` where `m/s^2` is wanted, `ms` vs `s`, `km/h` vs `m/s`) be caught, and
when.

| criterion | rlx (custom) | Numbat | Starlark | Lua (mlua/Luau) | Rhai | Nickel | CUE | Dhall | Lean 4 | Jsonnet | CEL | Pkl | KCL |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| readability for a non-programmer | ++ math-like | ++ math-like | + Python-like | + | + Rust/JS-like | o functional | o lattice | - | -- for proofs, o for defs | o | + one expression | + | + Python-like |
| units of measure | ++ static, load time | ++ static | o runtime, host type | o runtime, metatables | o runtime, host type | - none (record encoding) | - none | -- none | o (no std units; ad hoc types) | - none | o `duration` only | o `Duration`, `DataSize` only | - none |
| units checked in untaken branch | ++ | ++ | -- | -- | -- | -- | -- | n/a | ++ | -- | -- | o | -- |
| totality / termination | ++ by construction | o recursion allowed (refuse by AST pass) | + no recursion, no `while` (spec) | o fuel hook | o `max_operations` fuel | o recursion allowed | ++ no general recursion | ++ total | ++ total unless `partial` | o recursion allowed | ++ non-Turing-complete | o | o |
| determinism (no IO/clock/random) | ++ | o `now()`, `random()`, network feature: strip | ++ hermetic by design | o strip `os`, `io`, `math.random` | + nothing unless registered | + pure (imports only) | ++ | ++ (hashed imports) | ++ (eval in `IO` only if asked) | ++ | ++ | o reads env/files unless restricted | o |
| sandboxing in-process | ++ | + | ++ | + (C code, vendored) | ++ pure Rust | + | -- Go, out of process | + | -- out of process | + jrsonnet | ++ | -- JVM/native subprocess | o |
| error messages | ++ ours to design | + good | + | o runtime tracebacks | + positions | ++ contract blame | + | + | o elaborate | o | + | + | + |
| `--explain` derivation | ++ every `let`, every branch | o host must instrument the evaluator | o via traced Quantity values; branches invisible | o same | o same | - | - | - | o `#eval` traces nothing | - | o | - | - |
| monotonicity / worst case | + declared, Z3 discharges | o same encoding possible from its AST | - not analysable (general code) | - | - | - | - | - | ++ provable, by hand | - | o tiny AST, encodable | - | - |
| proof capability | o Z3 (decidable NRA fragment) | o same | -- | -- | -- | -- | -- | -- | ++ full, costly | -- | -- | -- | -- |
| Rust embedding | ++ native | + crate `numbat` 1.24 | + crate `starlark` 0.14 (Meta) | + crate `mlua` 0.12 | ++ crate `rhai` 1.26 | o crate `nickel-lang-core` 0.19, API churn | -- Go (libcue via cgo, or `cue` subprocess) | o crate `dhall` 0.13, lags the spec [unverified] | -- toolchain subprocess | + crate `jrsonnet-evaluator` (pre-release tags) | + crate `cel-interpreter` 0.10 | -- `pkl` binary; `pkl-rs` 0.0.0 | o Rust core, crates not on crates.io under a stable name [unverified] |
| binary weight added | small | medium (disable default features) | medium | small (C Lua ~300 KB) | small-medium | large | n/a | medium | n/a (toolchain ~hundreds of MB, Mathlib cache several GB) | medium | small | n/a | large |
| license | ours | MIT/Apache-2.0 | Apache-2.0 | MIT (mlua), MIT (Lua) | MIT/Apache-2.0 | MIT | Apache-2.0 | BSD-2 [unverified] | Apache-2.0 | MIT | Apache-2.0/MIT [unverified] | Apache-2.0 | Apache-2.0 |
| profile tables (scales, policy) | + | o | + | + | + | ++ | ++ | ++ | o | + | - | ++ | ++ |
| versioning story | + semver header + hash | o module, no version | + `load()` labels | o | o | + imports | + modules | ++ sha256-pinned imports | ++ lake manifest | o | o | + package URIs | + |

The two rows that decide it are *units in the untaken branch* and
*explain*. Every dynamically typed host (Starlark, Lua, Rhai, Jsonnet)
finds a dimension error only on the input that executes it: a contract whose
entry speed puts it in the ramp branch never evaluates the other branch, so
`(v0 - v_r) / a` with a wrong unit ships, and the first contract that crosses
`v_r` fails at check time in someone else's workspace. A statically
dimensioned language rejects the extension when it is loaded. And a host
that evaluates general code can trace arithmetic (through instrumented
values) but not *why* a branch was taken, which is precisely what a safety
reviewer reads first.

### 1.2 What the core keeps, what moves

The split, applied to the v0.1.58 keys:

| stays in core | moves to an extension |
|---|---|
| launch structure (`nodes`, `topics`, `services`, `actions`, `includes`, `args`, `if`/`unless`) | **physical models**: hazard `entry_speed`; `safe_state.settle` `decel`/`jerk` -> `vehicle.braking` |
| ROS interface facts incl. `qos` | **standard profiles**: the default severity scale (ISO 26262 QM, ASIL_A..D) and severity -> criticality policy -> `iso26262`, `iec61508`, `do178c`, `iec62304`, ... |
| execution structure: paths, `trigger`, timers (`rate_hz`, `jitter`, `param`/`period_param`), `sync`, `concurrency` | |
| **Bound** (one primitive, readable aliases): `max_latency`, `min_latency`, `max_jitter`, timer `jitter`, `max_age`, `max_transport`, rates, `max_response`, `within`, `window`, `ftti`, settle `duration`/`response`; map form `{ duration, param, source }`, `source.measured` | |
| loss tolerance: `drop`, `miss` | |
| safety logic: `functions`/`when` (Predicate), `hazards` (`guards`, `on`, `when`, `reaction`, `severity` as a label on a declared ordered scale), `on_violation`, `modes` (`requires`, `fallback`, `window`, `exit`, `overrides`), `safe_state` (`emits`, `settle`) | |

The rule for an extension: it may add keys in a declared core context
(`hazards.<name>`, `safe_state.settle`, `modes.<name>`, `nodes.<name>`), and
its outputs land in core keys that already exist (`settle.duration`,
`ftti`, `window.duration`, a node's `criticality`) with `source: derived`.
It cannot add a rule that core does not know how to run; the one exception is
an extension **predicate**, which core runs as an opaque boolean with a
printed derivation (the ISO 13855 distance check and the UAV energy check
below need one).

---

## 2. Design of an extension (the recommended shape)

### 2.1 The extension file

`vehicle_braking.rlx`, shipped in `share/rlm_ext_vehicle/rlm/`:

```
extension vehicle.braking 1.2.0
requires rlm >= 0.2.0

-- Keys this extension adds, with the core context they live in.
key hazard.entry_speed : Velocity
    "The speed the vehicle may travel at when the fault occurs (ODD bound)."
key settle.decel       : param Acceleration magnitude
    "Node parameter holding the commanded deceleration."
key settle.jerk        : param Jerk magnitude
    "Node parameter holding the jerk the deceleration ramps at."
-- A core key this extension reads (core keeps settle.response as a Bound).
input settle.response  : Time = 0 s

-- What it produces: the core settle duration of the reaction's safe state.
derive settle.duration : Time =
    response + braking_time(entry_speed, decel, jerk)
  worst_case:
    increasing in entry_speed
    decreasing in decel, jerk
  domain:
    entry_speed in (0 m/s, 70 m/s]
    decel       in (0 m/s^2, 15 m/s^2]
    jerk        in (0 m/s^3, 50 m/s^3]

fn braking_time(v0: Velocity, a: Acceleration, j: Jerk) -> Time =
    if v0 > v_r then a / j + (v0 - v_r) / a
    else sqrt(2 v0 / j)
  where v_r = a^2 / (2 j)   -- speed shed during the jerk ramp
```

What each line buys:

- `key ... : Velocity` gives the readable domain name and its dimension. A
  contract writing `entry_speed: 15 km/h` is converted; `entry_speed: 15` is
  refused ("a quantity needs a unit", the same rule that removed `_ms`
  suffixes); `entry_speed: 15 s` is refused with both dimensions named.
- `param Acceleration magnitude` says the value comes from the node's launch
  parameter (as today), is a plain double in SI base units, and is read as
  `|x|`.
- `derive settle.duration` names the core key it fills. Core sees a settle
  duration with `source: derived` and a derivation record; every existing
  rule (`fault-reaction-budget`, `ladder-rung-budget`, `settle-conflict`
  against a measured duration) runs unchanged.
- `worst_case` and `domain` are proof obligations (section 2.4).
  `increasing` means non-decreasing and `decreasing` non-increasing: below
  the ramp the braking time does not depend on `decel` at all.

### 2.2 The contract side

```yaml
rlm: ">=v0.2.0"
use:
  vehicle.braking: "^1.2"          # resolved through the ament index

hazards:
  odd_exit:
    guards: [/system/operation_mode/availability]
    on: reported
    when: { field: autonomous, equals: false }
    entry_speed: 15.2 km/h          # key from vehicle.braking
    ftti: 30s
    reaction: l3_engaged

nodes:
  mrm_emergency_stop_operator:
    params:
      target_acceleration: { type: double }
      target_jerk: { type: double }
    paths:
      on_timer:
        trigger: { timer: { rate_hz: 30 } }
        output: [emergency_control_cmd]
        safe_state:
          emits: emergency_control_cmd
          settle:
            decel: target_acceleration     # key from vehicle.braking
            jerk: target_jerk              # key from vehicle.braking
            response: { duration: 300ms, source: requirement }
```

Readable names stay bare (`entry_speed`, `decel`, `jerk`) as long as exactly
one used extension declares that key in that context. Two extensions both
declaring `settle.decel` in one contract is an error that asks for the
qualified spelling `vehicle.braking/decel:`. (Open question Q2.)

### 2.3 What `play_launch check --explain` prints

```
info[settle-derived]: hazard 'odd_exit' -> mode emergency_stop
    -> island.emergency_stop -> mrm_emergency_stop_operator/on_timer
  settle = 2822.22 ms   source: derived by vehicle.braking 1.2.0
                        (share/rlm_ext_vehicle/rlm/vehicle_braking.rlx, sha256 3f9c..e1)
  inputs
    entry_speed  v0 = 4.2222 m/s     15.2 km/h, hazard odd_exit (system.contract.yaml:9)
    decel        a  = 2.5 m/s^2      |target_acceleration| = |-2.5| (launch value)
    jerk         j  = 1.5 m/s^3      |target_jerk| = |-1.5| (launch value)
    response        = 300 ms         requirement (system.contract.yaml:26)
  derivation
    v_r = a^2 / (2 j)                = 2.0833 m/s
    v0 > v_r                         4.2222 > 2.0833: ramp completes, then constant decel
    a / j                            = 1666.67 ms
    (v0 - v_r) / a                   = 855.56 ms
    braking_time                     = 2522.22 ms
    settle = response + braking_time = 2822.22 ms
  worst case
    increasing in entry_speed        proved (z3, 14 ms) over the declared domain
    decreasing in decel, jerk        proved (z3, 31 ms)
    -> the declared entry_speed is the worst case; a lower launch decel or
       jerk than the one read would lengthen the settle.
```

(For comparison, `docs/launch-manifest.md` quotes 2525.33 ms of braking
time at v0 = 4.23 m/s; 15.2 km/h is 4.2222 m/s, hence 2522.22 ms here.
Arithmetic in appendix A.)

The worst-case block is the point. A contract states `entry_speed` as an
operating-domain bound, and the run that motivated `response:` showed that
a settle derived at a speed below the worst case passes the budget while
the vehicle does not. With a proved monotonicity the checker can say
"evaluating at the bound is evaluating at the worst case"; without it, it
must say "evaluated at the bound; not shown to be the worst case".

### 2.4 Worst case and proof obligations

An extension states, per derived output, how it moves with each input over
a declared domain. The checker turns each claim into an SMT query on the
rlx AST: for `increasing in v0`, assert `v1 < v2`, both in the domain, the
other inputs equal and in the domain, and `f(v1) > f(v2)`; `unsat` proves
the claim. `sqrt(x)` is encoded as a fresh `s >= 0, s*s = x`; `if` becomes
`ite`; `min`/`max` become `ite`. This is nonlinear real arithmetic, which Z3
decides (nlsat) but with no time guarantee, so the checker gives it a
budget (e.g. 2 s per obligation) and reports `unproved` on timeout rather
than failing.

What Z3 is good at here: polynomial and rational formulas with a few
branches (all five models in section 4 are of that kind). What it is bad
at: transcendental functions (`exp`, `log`, `sin`), sums over a table of
symbolic length, and anything quantified over functions. For those the
extension can attach a Lean 4 theorem (section 4.6), checked by the
extension's own CI against a hash of the rlx source; the contract checker
only verifies that the hash matches and prints `proved (lean, ext CI)`.

Two lessons the models below teach about worst cases:

- **The worst-case direction is model-specific.** Braking time grows with
  speed (worst case: the maximum). Occlusion detection time shrinks with
  flow rate (worst case: the *minimum* rate). A core that hard-codes "the
  bound is the worst case" is wrong for half the domains; the extension
  must say which end of its domain is the worst case, and the checker must
  evaluate there.
- **Monotone in each input is not the same as worst case at a corner of a
  box when inputs are coupled.** Braking time is non-increasing in `a`
  (above the ramp `d/da = 1/(2j) - v0/a^2 < 0`) and in `j`, so the worst
  corner of `{v0 max, a min, j min}` is well defined. Interval arithmetic
  alone would overestimate (the dependency problem on `a/j - a/(2j)`), which
  is why rlx evaluates at the corner the proved directions name instead of
  propagating intervals.

### 2.5 A profile extension

Profiles are tables. The recommended form is plain YAML data inside the
extension package, with rlx only where a rule is genuinely computational:

```yaml
# share/rlm_profile_iec61508/rlm/iec61508.profile.yaml
profile: iec61508
version: 1.0.0
requires: { rlm: ">=0.2.0" }
severity_scale: [none, SIL_1, SIL_2, SIL_3, SIL_4]     # ascending
criticality:                                           # core's scheduling input
  none: null          # first entry derives no criticality (core rule)
  SIL_1: low
  SIL_2: medium
  SIL_3: high
  SIL_4: high
```

The DO-178C profile is the same with `[E, D, C, B, A]` (DAL E least) and the
IEC 62304 one with `[A, B, C]`. ISO 26262 decomposition (an ASIL D
requirement met by two independent ASIL B(D) elements, ISO 26262-9:2018
clause 5 [unverified clause]) is the one rule that is a function:

```
profile iso26262 1.1.0
scale severity = [QM, ASIL_A, ASIL_B, ASIL_C, ASIL_D]

criticality(s) = match s { QM => none, ASIL_A | ASIL_B => low,
                           ASIL_C => medium, ASIL_D => high }

-- A redundant guard set (all_of) may carry a decomposed ASIL; the
-- checker asks whether the pair covers the hazard's ASIL.
fn covers(target: Severity, x: Severity, y: Severity) -> Bool =
    match target {
      ASIL_D => (x >= ASIL_B and y >= ASIL_B) or max(x, y) == ASIL_D
      ASIL_C => (x >= ASIL_A and y >= ASIL_B) or (x >= ASIL_B and y >= ASIL_A)
                or max(x, y) == ASIL_C
      ASIL_B => (x >= ASIL_A and y >= ASIL_A) or max(x, y) >= ASIL_B
      _      => max(x, y) >= target
    }
```

Contract side: `use: { iso26262: "^1" }` and `severity: ASIL_C` as today;
without a `use:` of a profile the core default scale stays only if Q5
decides so.

How the same profile reads in the other candidates is shown in section 5.

---

## 3. Domain survey

For each domain: hazards, the deadline (the FTTI equivalent and its name),
detection and reaction terms, the physical settle/stop model, and the
fallback ladder. The point of the survey is the vocabulary an extension
must keep readable, and the shapes the core primitives must carry.

### 3.1 Road vehicles

Standards: ISO 26262:2018 (functional safety), ISO 21448:2022 (SOTIF), UN
Regulation No. 157 (ALKS), plus ISO 22737 (low-speed automated driving) for
low-speed shuttles.

- **Hazards**: hazardous events from the HARA, rated by severity S0-S3,
  exposure E0-E4, controllability C0-C3 -> ASIL QM, A-D (ISO 26262-3:2018
  clause 6 [unverified clause]). SOTIF adds triggering conditions and
  performance limitations without a fault (ISO 21448 clauses 6-7
  [unverified]).
- **Deadline**: *fault tolerant time interval (FTTI)*: the minimum time from
  a fault to a possible hazardous event if the safety mechanisms are not
  activated (ISO 26262-1:2018, 3.61 [unverified number]). Split into
  *fault detection time interval (FDTI)* and *fault reaction time interval
  (FRTI)*; the 2018 edition requires `FDTI + FRTI <= FTTI` (ISO 26262-1
  3.56/3.62 [unverified numbers]). *Emergency operation time interval*
  (EOTI) and *emergency operation tolerance time interval* (EOTTI) bound how
  long a degraded emergency operation may last (ISO 26262-1 3.40/3.41
  [unverified numbers]). *Multiple-point fault detection interval* applies
  to latent faults.
- **Detection / reaction terms**: safety mechanism, diagnostic coverage,
  diagnostic test interval, safe state, warning and degradation concept,
  emergency operation.
- **Settle model**: braking from `v0` with deceleration ramped at jerk `j`
  to `a` after a response (dead) time `t_d`:
  `v_r = a^2/(2j)`; `t = t_d + a/j + (v0 - v_r)/a` when `v0 > v_r`, else
  `t_d + sqrt(2 v0 / j)`; stopping distance
  `s = v0 t_d + v0 a/j - a^3/(6 j^2) + (v0 - v_r)^2/(2a) + ...` (the ramp
  segment integrated). Inputs: `v0` m/s, `a` m/s^2, `j` m/s^3, `t_d` s. The
  current rlm hard-codes the time form. UN R157 (and its ALKS-specific
  annexes) states braking in terms of deceleration (an MRM decelerates to
  standstill; emergency manoeuvre limits [unverified values]).
- **Ladder**: nominal -> degraded / limp-home -> transition demand (driver
  takeover, a time window) -> minimum risk manoeuvre (MRM) -> standstill.
  UN R157 5.4.4.1: an MRM "shall be started, earliest 10 s after the start
  of the transition demand" [secondary source: L3Pilot slides; verify
  against the regulation text]. This is the shape rlm's `window` and `exit`
  already encode.

### 3.2 Industrial machinery and robots

Standards: IEC 61508 (generic E/E/PE), ISO 13849-1:2023 (PL a-e), IEC 62061
(SIL), ISO 13855 (positioning of protective equipment), IEC 60204-1:2016
(electrical equipment, stop categories), ISO 10218-1/-2:2025 (industrial
robots), ISO/TS 15066:2016 (collaborative robots, now largely folded into
ISO 10218-2:2025 [unverified]), IEC 61800-5-2 (drive safety functions).

- **Hazards**: reaching a hazard zone while the machine moves; unexpected
  start-up; collision with a person (cobots: transient and quasi-static
  contact).
- **Deadline**: not named as a time but as a **distance**. The protective
  device must be far enough away that the machine stops before a person
  reaches the hazard: ISO 13855 *minimum distance*
  `S = K * T + C` [ISO 13855:2010, 6.2 / 6.2.3 unverified clause numbers]
  with `T = t1 + t2`, `t1` the protective device response time and `t2` the
  *machine stopping time* (overall system stopping performance). `K` is the
  approach speed: 2000 mm/s, and if the result exceeds 500 mm recompute with
  1600 mm/s, never below 500 mm (ISO 13855:2010 for electro-sensitive
  devices; secondary sources disagree on the body-part labelling, and the
  2024 edition revises the approach-speed treatment [unverified]). For a
  light curtain of detection capability `d <= 40 mm`, `C = 8 (d - 14)` mm
  [unverified]. So the time deadline is *derived*: `T_max = (S_installed -
  C) / K`.
- **Cobots**: ISO/TS 15066 speed and separation monitoring, protective
  separation distance
  `S_p = S_h + S_r + S_s + C + Z_d + Z_r` with `S_h` the human's travel
  during the robot's reaction and stopping time, `S_r` the robot's travel
  during its reaction time, `S_s` the robot's stopping distance, `C` the
  intrusion distance, `Z_d`, `Z_r` the sensing and robot position
  uncertainties [ISO/TS 15066:2016, 5.5.4 unverified clause; term set from
  secondary sources].
- **Detection / reaction terms**: response time of the ESPE, safety function
  response time, stopping time, stopping distance, *overall system stopping
  performance*.
- **Stop categories** (IEC 60204-1:2016, 9.2.2): category 0, uncontrolled
  stop by immediate removal of power; category 1, controlled stop with power
  available until standstill, then removed; category 2, controlled stop with
  power left on. IEC 61800-5-2 names the drive functions STO (safe torque
  off, cat. 0), SS1 (safe stop 1, cat. 1: decelerate, then STO, either
  monitored deceleration ramp or a time delay), SS2 (safe stop 2, cat. 2,
  then safe operating stop SOS) [IEC 61800-5-2:2016, 4.2.3 unverified].
  A category 1 stop time is `t = t_resp + v / a_ramp` with a monitored ramp,
  or `t = t_resp + t_delay` for the time-controlled SS1 variant; robots
  publish measured stopping times and distances per axis at 33/66/100 % of
  speed and payload (ISO 10218-1 annex [unverified]).
- **Ladder**: normal -> reduced speed (SLS) -> protective stop (cat. 1/2)
  -> STO (cat. 0). Short and fixed; no takeover window.

### 3.3 Process industry (SIS)

Standards: IEC 61508; IEC 61511-1:2016 (+A1:2017).

- **Hazards**: loss of containment, overpressure, runaway reaction, defined
  per safety instrumented function (SIF) in a hazard and risk assessment
  (HAZOP, LOPA).
- **Deadline**: *process safety time (PST)*: the time between a failure
  occurring in the process or the basic process control system and the
  occurrence of the hazardous event if the safety function is not performed
  (IEC 61511-1:2016, 3.2.52 [unverified number]). Practice (ANSI/ISA
  84.00.01 commentary, secondary) is that the SIF should complete within
  half the PST. The next edition is reported to replace PST with a
  differently named interval [secondary source, unverified].
- **Detection / reaction terms**: *SIF response time* (sensor detection,
  logic solver scan, final element stroke), *proof test interval*,
  *diagnostic test interval*, *mean time to restoration*, *demand mode* vs
  *continuous mode*.
- **Settle model**: valve stroke time, plus the process response after the
  valve closes (pressure decay, level change). Typical first-order form:
  time for a tank level to reach a limit at net inflow `q` into area `A`:
  `t = A (h_lim - h0) / q`; pressure rise in a blocked-in vessel
  `dp/dt = (gamma p / V) * q_v` [illustrative]. The PST itself is often
  derived from such a model: the extension is a *deadline* producer, not a
  settle producer.
- **Ladder**: alarm -> operator response (with an *operator response time*,
  commonly 10-30 min in LOPA practice [secondary]) -> SIF trip -> safe state
  (often de-energize-to-trip). A two-rung ladder whose first rung is a timed
  window, the same shape as a takeover request.

### 3.4 Medical devices

Standards: IEC 62304:2006+A1:2015 (software class A/B/C), ISO 14971:2019
(risk management), IEC 60601-1, IEC 60601-1-8:2006+A1+A2 (alarm systems),
IEC 60601-2-24:2012 (infusion pumps), IEC 62366-1 (usability); closed-loop
controllers: IEC 60601-1-10 (physiologic closed-loop controllers).

- **Hazards**: over-/under-delivery of a drug, occlusion not detected,
  bolus on occlusion release, air in line, alarm not perceived.
- **Deadline**: there is no FTTI by name. ISO 14971 rates harm by severity
  and probability; timing appears in the alarm standard: *alarm condition
  delay* (onset of the triggering event in the patient to the alarm system
  deciding the alarm condition exists, `t2 - t1`) and *alarm signal
  generation delay* (onset of the alarm condition to generation of its alarm
  signal) (IEC 60601-1-8, 3.x and 6.4 [secondary reproduction; verify
  numbering per amendment]). The clinical deadline (how long therapy may be
  interrupted) is a requirement from the risk analysis, drug-specific.
- **Detection / reaction terms**: occlusion alarm threshold (pressure),
  *time to occlusion alarm*, bolus volume after occlusion release
  (IEC 60601-2-24 measures both, test apparatus fig. 201.112 [unverified]).
- **Settle / detect model**: an occlusion at flow `Q` raises line pressure
  at `dp/dt = Q / C_sys` (system compliance `C_sys` in mL/mmHg), so the time
  to alarm from baseline `p0` to threshold `p_th` is
  `t = (p_th - p0) * C_sys / Q`, and the bolus released on clearing is
  `V = C_sys * (p_th - p0)`. The *worst case is the lowest flow rate*,
  which is why low-rate syringe pumps alarm late (tens of minutes at
  1 mL/h). Closed-loop insulin: the safety deadline is glucose trajectory
  based (time to a hypoglycaemia threshold given insulin on board), a
  pharmacokinetic model rather than a mechanical one.
- **Ladder**: alarm (low/medium/high priority) -> stop infusion (pump) ->
  hold; closed-loop: closed loop -> fallback basal rate -> open loop with
  alarm. The fallback rung is "deliver a safe default", not "stop".

### 3.5 Aerospace

Standards: SAE ARP4761A (safety assessment), ARP4754B (development), RTCA
DO-178C (software), DO-254 (hardware), 14 CFR / CS 25.1309 with AC
25.1309-1B.

- **Hazards**: *failure conditions* classified catastrophic, hazardous,
  major, minor, no safety effect, with quantitative targets per flight hour
  (catastrophic `< 1e-9`, hazardous `< 1e-7`, major `< 1e-5`) and a
  development assurance level A-E for each (AC 25.1309-1B; ARP4754B).
- **Deadline**: expressed as **exposure time** and probability, not a
  reaction deadline. Latent failures have an exposure time equal to the
  check interval; a *significant latent failure* is restricted (AC
  25.1309-1B 5.3.6 [section title seen, text not verified]). Real-time
  deadlines appear at system level: *maximum latency* of a function,
  *transport delay* in flight controls, *time to annunciate* a failure to
  the crew.
- **Detection / reaction terms**: monitor, built-in test, annunciation,
  crew procedure, *exposure time*, *at-risk time*.
- **Settle model**: rarely a stop; a control law reconfiguration with a
  transient that must remain inside a bound, or a fault-to-annunciation
  latency. Probability of a dual failure with a latent first failure:
  `P approx lambda_1 * lambda_2 * T_exposure * t_flight / 2`
  [standard approximation, unverified source clause].
- **Ladder**: normal law -> alternate law -> direct law -> mechanical
  backup; crew alerting levels warning/caution/advisory.

### 3.6 UAVs

Sources: EASA Specific category / SORA 2.5 (JARUS), ASTM F3548 (UTM), and
autopilot practice (PX4, ArduPilot) for the native vocabulary.

- **Hazards**: lost command and control (C2) link, geofence breach,
  battery depletion, loss of navigation.
- **Deadline**: *lost link timeout* (PX4 `COM_DL_LOSS_T`, then
  `NAV_DLL_ACT`: hold/return/land/terminate [PX4 docs v1.15+]); geofence:
  distance-based (contingency volume, ground risk buffer in SORA
  [unverified section]); battery: *reserve* expressed as time or energy
  remaining at a threshold (PX4 `BAT_LOW_THR`, `BAT_CRIT_THR`,
  `BAT_EMERGEN_THR`, and `COM_FLTT_LOW_ACT` for "just enough for return"
  [partly secondary]).
- **Settle model**: descent to ground: `t = (h - h_slow) / v_desc + h_slow
  / v_land` (PX4 multicopter: `MPC_Z_VEL_MAX_DN`, `MPC_LAND_ALT1/2`,
  `MPC_LAND_SPEED` [parameter names from memory; verify per PX4 version]),
  and return-to-launch time `d / v_cruise + climb + descent`. The energy
  check: `P_hover * (t_detect + t_descent) <= E_cap * (soc_trigger -
  soc_reserve)`. That is a **predicate**, not a bound.
- **Ladder**: mission -> hold/loiter (timed) -> RTL -> land -> terminate
  (parachute / motor cut). Very close to rlm's ladder with windows.

### 3.7 Rail

Standards: EN 50126-1/-2:2017 (RAMS), EN 50128:2011 / EN 50716:2023
(software), EN 50129:2018 (safety-related electronics), ERTMS/ETCS SUBSET-026
(system requirements, 3.13 braking curves), EN 14531 (stopping distance
calculation), EN 16834:2019 Annex F (ETCS conversion model), EN 17997:2025
(gamma-train brake curve parameters).

- **Hazards**: passing the end of movement authority (EOA), overspeed,
  collision, derailment. SIL 0-4 with tolerable hazard rates per hour.
- **Deadline**: **distance-based supervision**. ETCS computes the
  emergency brake deceleration curve (EBD) toward a supervised location and
  places speed limits ahead of it: emergency brake intervention (EBI),
  service brake intervention (SBI), warning, permitted, indication
  (SUBSET-026 3.13, per ERA's braking curves tool handbook). Time terms
  enter as *equivalent brake build-up time* `T_be` (emergency and service),
  *traction cut-off time* `T_traction_cut_off`, and driver reaction time
  for the indication curve.
- **Detection / reaction terms**: brake reaction time, brake build-up time,
  overlap, safety margin, release speed.
- **Settle model**: `d = v0 (T_trac + T_be) + integral of v/A_ebd(v) dv`;
  with `A_ebd` piecewise constant in speed bands (`A_brake_emergency(V)`,
  with correction factors `Kdry_rst`, `Kwet_rst`, gradient) [SUBSET-026
  3.13.6 unverified]. Stopping time `t = T_trac + T_be + sum_i dv_i / A_i`.
  Inputs in km/h on the driver side, m/s and m/s^2 in the model: the
  standard place for a unit error.
- **Ladder**: full supervision -> service brake intervention -> emergency
  brake intervention -> standstill; trip mode on passing EOA.

### 3.8 Maritime (autonomous ships) - brief

IMO MASS Code (non-mandatory, under development, adoption expected around
2026 [unverified]), DNV class guidelines (DNV-CG-0264 autonomous and
remotely operated ships [unverified number]), COLREGs for the collision
logic. Native terms: *minimum risk condition* (MRC, also used by road),
*fallback state*, *closest point of approach* (CPA) and *time to CPA*
(TCPA) as the deadline, crash-stop distance and *head reach* (stopping
distance measured along the track, IMO MSC.137(76) manoeuvring standards:
full astern stopping track reach `<= 15` ship lengths [unverified]). The
settle is minutes long and dominated by hydrodynamics; extensions would
take measured trial data rather than a formula.

### 3.9 Elevators - brief

EN 81-20:2020 / EN 81-50:2020. Overspeed governor trips at `>= 115 %` of
rated speed; progressive safety gear average retardation between 0.2 gn
and 1 gn with rated load in free fall; instantaneous gear only up to
0.63 m/s rated speed [secondary sources; verify clause numbers, e.g.
5.6.2]. Ascending car overspeed and unintended car movement protection
(UCMP) give a stopping-distance bound. The model is a two-phase stop
(trip delay, then constant retardation) of the same shape as rail's.

### 3.10 What the survey says about the core

| domain | deadline is a | settle/derived output is a | ladder shape |
|---|---|---|---|
| road | time (FTTI) | time (braking) | windowed takeover, then MRM |
| machinery | **distance** (S) -> time by derivation | time (stopping) + distance check | short, fixed |
| process | time (PST), often derived | time (valve + process) | operator window, then trip |
| medical | clinical time + alarm delays | **detection** time (occlusion) | alarm, stop, safe default |
| aerospace | probability x exposure time | latency / annunciation time | control-law degradation |
| UAV | time (link timeout) + **energy** | time (descent) + energy check | hold (window) -> RTL -> land |
| rail | **distance** (EOA) | time and distance | supervision curves |

Three consequences:

1. **A derived value can be a deadline, a detection time or a settle**, not
   only a settle. The extension output must be allowed to fill `ftti`,
   `within`/detection, `settle.duration`, and `window.duration`.
2. **Distance and energy appear as predicates.** Core should not grow a
   distance Bound; an extension predicate ("required distance <= installed
   distance") with its derivation is enough, and keeps the core temporal.
   The core's computed reaction total must be an *input* to such a
   predicate (ISO 13855 needs `T`, which core already derives).
3. **Worst-case direction varies** (max speed for braking, min flow for
   occlusion, max height and min state of charge for UAV), so it has to be
   declared and checked per model.

---
## 4. The same extensions, side by side

Model 1 (vehicle braking) is written in every candidate, because it is the
one rlm hard-codes today and the baseline everyone can compare. Models 2-5
are written in rlx and in the four general hosts that remain plausible
after model 1 (Starlark, Lua, Rhai, Nickel), with notes where another
language would differ materially. Model 3 also gets a Lean proof.

Host assumptions for the dynamic languages: the checker provides a
`Quantity` value type (number + dimension vector + trace node) with
overloaded arithmetic and comparison, unit constants, `sqrt`, and the
`extension`/`param` registration helpers. Without that host work, none of
them has units at all; with it, they get *runtime* units.

### 4.1 Model 1: vehicle braking settle

Contract side is the one in section 2.2 for every language (that is the
point of the design: the contract does not know what language the extension
is in). Only the extension changes.

**rlx** - see section 2.1. (In the hosts below, the `settle.response`
entry stands for the same thing: a core Bound read as an input, not a new
key.)

**Numbat** (formulas; the key declarations would live in a small manifest
beside it, since Numbat has no notion of keys):

```numbat
dimension Jerk = Length / Time^3      # if the prelude does not define it

fn braking_time(v0: Velocity, a: Acceleration, j: Jerk) -> Time =
  if v0 > v_r then a / j + (v0 - v_r) / a else sqrt(2 v0 / j)
  where v_r = a^2 / (2 j)

fn settle(entry_speed: Velocity, decel: Acceleration, jerk: Jerk,
          response: Time) -> Time =
  response + braking_time(entry_speed, decel, jerk)
```

Nearly identical to rlx, and dimension errors are compile errors. What it
does not have: key declarations, `param` binding, `worst_case`, a trace, a
termination guarantee (recursion is allowed), and a pure prelude (it has
`now()`, `random()` and a currency-fetching feature that must be compiled
out) [prelude-replacement API to verify].

**Starlark** (starlark-rust; no `**` operator in Starlark, so `a * a`):

```python
load("@rlm//units.star", "Velocity", "Acceleration", "Jerk", "Time", "s", "sqrt")

def braking_time(v0, a, j):
    v_r = a * a / (2 * j)
    if v0 > v_r:
        return a / j + (v0 - v_r) / a
    return sqrt(2 * v0 / j)

extension(
    name = "vehicle.braking",
    version = "1.2.0",
    keys = {
        "hazard.entry_speed": Velocity,
        "settle.decel": param(Acceleration, magnitude = True),
        "settle.jerk": param(Jerk, magnitude = True),
        "settle.response": default(Time, 0 * s),
    },
    derive = {
        "settle.duration": lambda x: x.response + braking_time(x.entry_speed, x.decel, x.jerk),
    },
)
```

**Lua** (mlua, Lua 5.4; `a^2` via the `__pow` metamethod):

```lua
local rlm = require("rlm")      -- host module
local U = rlm.units

return rlm.extension {
  name = "vehicle.braking", version = "1.2.0",
  keys = {
    { "hazard.entry_speed", U.velocity },
    { "settle.decel",    rlm.param(U.acceleration, { magnitude = true }) },
    { "settle.jerk",     rlm.param(U.jerk, { magnitude = true }) },
    { "settle.response", U.time, default = 0 * U.s },
  },
  derive = {
    ["settle.duration"] = function(x)
      local v0, a, j = x.entry_speed, x.decel, x.jerk
      local v_r = a^2 / (2 * j)
      local t
      if v0 > v_r then
        t = a / j + (v0 - v_r) / a
      else
        t = rlm.sqrt(2 * v0 / j)
      end
      return x.response + t
    end,
  },
}
```

**Rhai**:

```rust
fn braking_time(v0, a, j) {
    let v_r = a * a / (2.0 * j);
    if v0 > v_r { a / j + (v0 - v_r) / a } else { sqrt(2.0 * v0 / j) }
}

extension("vehicle.braking", "1.2.0")
    .key("hazard.entry_speed", Velocity)
    .key("settle.decel", param(Acceleration).magnitude())
    .key("settle.jerk", param(Jerk).magnitude())
    .key("settle.response", Time, 0.0 * s)
    .derive("settle.duration",
            |x| x.response + braking_time(x.entry_speed, x.decel, x.jerk));
```

**Nickel** (numbers are plain; the unit string is a label the host
converts at the boundary, nothing inside the formula is checked):

```nickel
let rlm = import "rlm.ncl" in
let braking_time = fun v0 a j =>
  let v_r = a * a / (2 * j) in
  if v0 > v_r then a / j + (v0 - v_r) / a
  else std.number.pow (2 * v0 / j) 0.5
in
{
  name = "vehicle.braking",
  version = "1.2.0",
  keys = {
    "hazard.entry_speed" = rlm.quantity "m/s",
    "settle.decel" = rlm.param { unit = "m/s^2", magnitude = true },
    "settle.jerk" = rlm.param { unit = "m/s^3", magnitude = true },
    "settle.response" = rlm.quantity "s" & { default = 0 },
  },
  derive."settle.duration" = fun x =>
    x.response + braking_time x.entry_speed x.decel x.jerk,
}
```

**CUE** (no `if` expression; the branch is a list-comprehension idiom; Go
only, so the checker would shell out to `cue export` or link libcue):

```cue
package braking

import "math"

// inputs in SI base units, filled by the host
entry_speed: number & >0 & <=70 // m/s
decel:       number & >0        // m/s^2, |param|
jerk:        number & >0        // m/s^3
response:    *0 | number & >=0  // s

_v_r: decel * decel / (2 * jerk)
_t: [
	if entry_speed > _v_r {decel/jerk + (entry_speed-_v_r)/decel},
	if entry_speed <= _v_r {math.Sqrt(2 * entry_speed / jerk)},
][0]
settle_duration: response + _t
```

**Dhall** - cannot express the model. `Double` has no arithmetic at all
(only `Double/show`), and `Natural` has no division. Dhall can carry the
schema and nothing else:

```dhall
let Braking = { entry_speed : Double, decel : Text, jerk : Text, response : Double }
in  Braking
-- the settle formula is not writable in Dhall
```

**Lean 4** (definition, plus the monotonicity statement; the proof body is
deliberately left as `sorry`, because writing it is the cost being
estimated):

```lean
import Mathlib

/-- Braking time from `v0` with deceleration `a` reached at jerk `j` (SI). -/
noncomputable def brakingTime (v0 a j : Real) : Real :=
  let vr := a ^ 2 / (2 * j)
  if vr < v0 then a / j + (v0 - vr) / a else Real.sqrt (2 * v0 / j)

theorem brakingTime_monotoneOn (a j : Real) (ha : 0 < a) (hj : 0 < j) :
    MonotoneOn (fun v0 => brakingTime v0 a j) (Set.Ici 0) := by
  intro x hx y hy hxy
  simp only [brakingTime]
  split_ifs with h1 h2
  -- 4 cases. Same-branch cases: linear in v0, or Real.sqrt_le_sqrt.
  -- Mixed case (x <= vr < y): sqrt (2 x / j) <= sqrt (2 vr / j) = a / j
  -- <= a / j + (y - vr) / a; needs sqrt (a^2 / j^2) = a / j.
  all_goals sorry
```

Estimate for a fluent Lean user: 40-80 lines and an afternoon, longer on
first contact with Mathlib's lemma names. `Real` is noncomputable, so the
checker cannot evaluate this definition; it would evaluate the rlx (or a
`Float` twin), and the link between what is proved and what is evaluated
is a hash and a review, not a theorem.

**Jsonnet** (jrsonnet):

```jsonnet
local brakingTime(v0, a, j) =
  local v_r = a * a / (2 * j);
  if v0 > v_r then a / j + (v0 - v_r) / a else std.sqrt(2 * v0 / j);
{
  name: 'vehicle.braking',
  version: '1.2.0',
  derive(x):: { 'settle.duration': x.response + brakingTime(x.entry_speed, x.decel, x.jerk) },
}
```

**CEL** (one expression; no `let` without the `cel.bind` extension, so
`v_r` is repeated; `math.sqrt` is an extension library [unverified in the
Rust interpreter]):

```
entry_speed > decel * decel / (2.0 * jerk)
  ? response + decel / jerk + (entry_speed - decel * decel / (2.0 * jerk)) / decel
  : response + math.sqrt(2.0 * entry_speed / jerk)
```

**Pkl** (has a real `Duration` type with unit literals, but no general
units; JVM or native `pkl` binary as a subprocess):

```pkl
module vehicle.braking

import "pkl:math"

entrySpeed: Float(this > 0)   // m/s
decel: Float(this > 0)        // m/s^2
jerk: Float(this > 0)         // m/s^3
response: Duration = 0.s

local vR = decel * decel / (2 * jerk)
local tBrake: Float =
  if (entrySpeed > vR) decel / jerk + (entrySpeed - vR) / decel
  else math.sqrt(2 * entrySpeed / jerk)

settleDuration: Duration = response + tBrake.s
```

**KCL** (syntax approximate [unverified]):

```python
import math

schema Braking:
    entry_speed: float   # m/s
    decel: float
    jerk: float
    response: float = 0.0
    settle_duration: float = response + (decel / jerk + (entry_speed - decel * decel / (2 * jerk)) / decel \
        if entry_speed > decel * decel / (2 * jerk) else math.sqrt(2 * entry_speed / jerk))

    check:
        entry_speed > 0, "entry_speed must be positive"
        decel > 0
        jerk > 0
```

**F# units of measure** (not embeddable in a Rust checker; listed because it
is the model rlx's type system copies: dimensions are type parameters and
inference is decidable):

```fsharp
[<Measure>] type m
[<Measure>] type s
let brakingTime (v0: float<m/s>) (a: float<m/s^2>) (j: float<m/s^3>) : float<s> =
    let vr = a * a / (2.0 * j)
    if v0 > vr then a / j + (v0 - vr) / a
    else sqrt (2.0 * v0 / j)   // sqrt over measures needs a helper [unverified]
```

Koka was considered and dropped: it has no units-of-measure feature, and its
effect system, while elegant for "no IO", has no Rust embedding.

#### The telling difference: a dimension bug in the branch nobody ran

Suppose the author mistypes the ramp branch as `sqrt(2 v0 / a)` (should be
`/ j`). `2 v0 / a` is `(m/s) / (m/s^2) = s`, and `sqrt(s)` is `s^(1/2)`,
which is not a Time.

rlx and Numbat, when the extension is loaded, before any contract is read:

```
error[rlx-dimension]: vehicle_braking.rlx:22:10
   |
22 |     else sqrt(2 v0 / a)
   |          ^^^^^^^^^^^^^^ this is Time^(1/2)
   |
   = note: `braking_time` returns Time; the other branch (line 21) is Time
   = help: 2 v0 / j is Time^2, whose sqrt is Time
```

Starlark, Lua, Rhai, with the host Quantity type: the extension loads
fine. A contract at 15.2 km/h never enters the branch and passes. A contract
at 5 km/h (1.39 m/s, below `v_r` = 2.08 m/s) fails at check time, in the
integrator's workspace, with:

```
error[extension-runtime]: vehicle.braking 1.2.0: settle.duration:
  Quantity: cannot add s^(1/2) to s  (vehicle_braking.lua:20)
```

Nickel, CUE, Jsonnet, CEL, Pkl, KCL: nothing ever fails. `sqrt(2 v0 / a)`
is a number; the settle is wrong by a dimension and the budget check passes
or fails on a meaningless value.

#### What `--explain` can show, per host

rlx prints the block in section 2.3: inputs with provenance, every `where`
binding with its value, the branch condition with both sides, and the
worst-case verdicts.

Starlark/Lua/Rhai with a traced Quantity: the arithmetic tree of the value
that came out, but not the branch, because `if` is control flow the value
never sees:

```
settle = 2822.22 ms  (vehicle.braking 1.2.0, Starlark)
  = x.response + ((a / j) + ((v0 - ((a * a) / (2 * j))) / a))
  = 300 ms + ((2.5 m/s^2 / 1.5 m/s^3) + ((4.2222 m/s - 2.0833 m/s) / 2.5 m/s^2))
  worst case: not analysed (general code)
```

The reviewer sees which formula was used, but has to infer from its shape
which branch that was and why, and gets no worst-case statement. Recovering
names (`v_r`) needs the author to wrap them (`v_r = named("v_r", ...)`).

Nickel/CUE/Dhall/Jsonnet/CEL/Pkl/KCL: the result only, unless the checker
re-implements their evaluators.

### 4.2 Model 2: ISO 13855 safety distance (a predicate fed by core)

The guard is a light curtain driver's topic; the reaction is a stop
command; the press's stopping time is the settle (measured, as ISO 13855
requires). Core already derives `reaction_total = detection + route +
settle`. The extension adds the device response `t1` and checks the
installed distance. Its output is a **predicate**, not a bound.

Contract:

```yaml
use:
  machine.iso13855: "^0.3"
hazards:
  reach_into_press:
    severity: PL_d                 # from a machinery profile (section 2.5)
    guards: [/safety/light_curtain/state]
    on: reported
    when: { field: interrupted, equals: true }
    reaction: press.stop
    detection_capability: 30 mm    # keys from machine.iso13855
    device_response: 15ms
    installed_distance: 520 mm
nodes:
  press_controller:
    paths:
      on_curtain:
        trigger: { input: [curtain] }
        output: [stop_cmd]
        max_latency: 12ms
        safe_state:
          emits: stop_cmd
          settle: { duration: 180ms, source: { measured: { max: 150ms, n: 200, margin: 0.2 } } }
```

**rlx**:

```
extension machine.iso13855 0.3.0
requires rlm >= 0.2.0

key hazard.detection_capability : Length  "ESPE detection capability d"
key hazard.device_response      : Time    "ESPE response time t1"
key hazard.installed_distance   : Length  "detection zone to hazard zone"
input reaction_total : Time               -- from core: detection + route + settle

check safety_distance: required <= installed_distance
  where T        = device_response + reaction_total
    and C        = if detection_capability <= 40 mm
                   then 8 * (detection_capability - 14 mm)
                   else 850 mm
    and s_2000   = 2000 mm/s * T + C
    and required = if s_2000 <= 500 mm then max(s_2000, 100 mm)
                   else max(1600 mm/s * T + C, 500 mm)
  worst_case: increasing in reaction_total, device_response
  domain: detection_capability in [14 mm, 40 mm]
```

**Starlark**:

```python
load("@rlm//units.star", "Length", "Time", "mm", "s")

def required_distance(t, d):
    c = 8 * (d - 14 * mm) if d <= 40 * mm else 850 * mm
    s2000 = 2000 * mm / s * t + c
    if s2000 <= 500 * mm:
        return max(s2000, 100 * mm)
    return max(1600 * mm / s * t + c, 500 * mm)

extension(
    name = "machine.iso13855", version = "0.3.0",
    keys = {"hazard.detection_capability": Length,
            "hazard.device_response": Time,
            "hazard.installed_distance": Length},
    inputs = ["reaction_total"],
    checks = {"safety_distance": lambda x:
        required_distance(x.device_response + x.reaction_total,
                          x.detection_capability) <= x.installed_distance},
)
```

**Lua**:

```lua
local rlm = require("rlm"); local U = rlm.units
local mm, s = U.mm, U.s

local function required_distance(t, d)
  local c = (d <= 40 * mm) and 8 * (d - 14 * mm) or 850 * mm
  local s2000 = 2000 * mm / s * t + c
  if s2000 <= 500 * mm then return rlm.max(s2000, 100 * mm) end
  return rlm.max(1600 * mm / s * t + c, 500 * mm)
end

return rlm.extension {
  name = "machine.iso13855", version = "0.3.0",
  keys = { {"hazard.detection_capability", U.length},
           {"hazard.device_response", U.time},
           {"hazard.installed_distance", U.length} },
  inputs = { "reaction_total" },
  checks = { safety_distance = function(x)
    return required_distance(x.device_response + x.reaction_total,
                             x.detection_capability) <= x.installed_distance
  end },
}
```

**Rhai**:

```rust
fn required_distance(t, d) {
    let c = if d <= 40.0 * mm { 8.0 * (d - 14.0 * mm) } else { 850.0 * mm };
    let s2000 = 2000.0 * mm / s * t + c;
    if s2000 <= 500.0 * mm { max(s2000, 100.0 * mm) }
    else { max(1600.0 * mm / s * t + c, 500.0 * mm) }
}
extension("machine.iso13855", "0.3.0")
    .key("hazard.detection_capability", Length)
    .key("hazard.device_response", Time)
    .key("hazard.installed_distance", Length)
    .input("reaction_total")
    .check("safety_distance", |x| required_distance(
        x.device_response + x.reaction_total, x.detection_capability)
        <= x.installed_distance);
```

**Nickel** (all lengths in mm by convention, times in s):

```nickel
let required_distance = fun t d =>
  let c = if d <= 40 then 8 * (d - 14) else 850 in
  let s2000 = 2000 * t + c in
  if s2000 <= 500 then std.number.max s2000 100
  else std.number.max (1600 * t + c) 500
in
{
  name = "machine.iso13855", version = "0.3.0",
  inputs = ["reaction_total"],
  checks.safety_distance = fun x =>
    required_distance (x.device_response + x.reaction_total) x.detection_capability
      <= x.installed_distance,
}
```

The Nickel version is the clearest illustration of the unit problem: `2000 *
t` is right only if `t` is in seconds and the result is in mm; nothing
enforces either. A `detection_capability` delivered as `0.03` (m) instead of
`30` (mm) makes `C = 8 * (0.03 - 14)` negative, the required distance
shrinks, and the check passes on a meaningless number. In rlx, `30 mm` and
`0.03 m` are the same Length, and a bare `30` is refused.

`--explain` (rlx):

```
info[iso13855.safety_distance]: hazard 'reach_into_press' holds
  check  required <= installed_distance:  500.0 mm <= 520 mm  (margin 20.0 mm)
  inputs
    device_response      t1 = 15 ms        hazard reach_into_press (cell.contract.yaml:11)
    reaction_total          = 192 ms       core: detection 0 ms (reported, event-driven)
                                           + route 12 ms (press_controller/on_curtain)
                                           + settle 180 ms (measured: 150 ms x 1.2, n=200)
    detection_capability d  = 30 mm
    installed_distance      = 520 mm
  derivation
    T = t1 + reaction_total                   = 207 ms
    d <= 40 mm                                30 mm <= 40 mm
    C = 8 (d - 14 mm)                         = 128 mm
    s_2000 = 2000 mm/s * T + C                = 542.0 mm
    s_2000 <= 500 mm                          542.0 > 500: recompute with K = 1600 mm/s
    required = max(1600 mm/s * T + C, 500 mm) = max(459.2 mm, 500 mm) = 500.0 mm
  worst case
    increasing in reaction_total, device_response   proved (z3, 9 ms)
  note: the settle is a MEASURED budget (not an upper bound); ISO 13855
        requires the stopping time of the overall system, measured or
        stated by the machine manufacturer [unverified clause].
```

Core gap exposed: core must expose `reaction_total` per hazard as an
extension input. Today it is printed by `fault-reaction-budget` but not a
named value. That is a small, generic addition (any hazard's derived
FDTI, route and settle as read-only inputs).

### 4.3 Model 3: infusion pump occlusion detection time

The derived value is the **detection** time (the alarm condition delay of
IEC 60601-1-8 for this alarm), not a settle. The worst case is at the
minimum flow rate.

Contract:

```yaml
use:
  medical.occlusion: "^0.1"
hazards:
  therapy_interrupted:
    severity: C                      # IEC 62304 class, from a profile
    description: downstream occlusion stops a vasoactive infusion
    guards: [/pump/line_pressure]
    on: reported
    when: { field: pressure_mmhg, ge: 300 }
    ftti: 30min                      # clinical: from the risk analysis
    reaction: pump.alarm_and_stop
    min_rate: 1 mL/h                 # keys from medical.occlusion
    compliance: 0.003 mL/mmHg        # worst-case syringe and line set
    baseline_pressure: 20 mmHg
    alarm_threshold: pump_driver.occlusion_limit   # param, mmHg
    signal_delay: 5s                 # alarm signal generation delay
```

**rlx**:

```
extension medical.occlusion 0.1.0
requires rlm >= 0.2.0

dimension Compliance = Volume / Pressure
dimension Flow       = Volume / Time

key hazard.min_rate          : Flow              "lowest programmable rate covered"
key hazard.compliance        : Compliance        "system compliance (syringe + line)"
key hazard.baseline_pressure : Pressure = 0 mmHg
key hazard.alarm_threshold   : param Pressure unit mmHg   "occlusion alarm limit"
key hazard.signal_delay      : Time = 0 s        "alarm signal generation delay"

derive hazard.detection : Time =
    (alarm_threshold - baseline_pressure) * compliance / min_rate + signal_delay
  worst_case:
    decreasing in min_rate
    increasing in compliance, alarm_threshold
  domain: min_rate in (0 mL/h, 1200 mL/h], alarm_threshold > baseline_pressure

report bolus_on_release : Volume = compliance * (alarm_threshold - baseline_pressure)
```

**Starlark**:

```python
load("@rlm//units.star", "Flow", "Compliance", "Pressure", "Time", "mmHg", "s")

extension(
    name = "medical.occlusion", version = "0.1.0",
    keys = {
        "hazard.min_rate": Flow,
        "hazard.compliance": Compliance,
        "hazard.baseline_pressure": default(Pressure, 0 * mmHg),
        "hazard.alarm_threshold": param(Pressure, unit = mmHg),
        "hazard.signal_delay": default(Time, 0 * s),
    },
    derive = {"hazard.detection": lambda x:
        (x.alarm_threshold - x.baseline_pressure) * x.compliance / x.min_rate
        + x.signal_delay},
    reports = {"bolus_on_release": lambda x:
        x.compliance * (x.alarm_threshold - x.baseline_pressure)},
)
```

**Lua**:

```lua
local rlm = require("rlm"); local U = rlm.units
return rlm.extension {
  name = "medical.occlusion", version = "0.1.0",
  keys = {
    {"hazard.min_rate", U.volume / U.time},
    {"hazard.compliance", U.volume / U.pressure},
    {"hazard.baseline_pressure", U.pressure, default = 0 * U.mmHg},
    {"hazard.alarm_threshold", rlm.param(U.pressure, { unit = U.mmHg })},
    {"hazard.signal_delay", U.time, default = 0 * U.s},
  },
  derive = { ["hazard.detection"] = function(x)
    return (x.alarm_threshold - x.baseline_pressure) * x.compliance / x.min_rate
           + x.signal_delay
  end },
}
```

**Rhai**:

```rust
extension("medical.occlusion", "0.1.0")
    .key("hazard.min_rate", Volume / Time)
    .key("hazard.compliance", Volume / Pressure)
    .key("hazard.baseline_pressure", Pressure, 0.0 * mmHg)
    .key("hazard.alarm_threshold", param(Pressure).unit(mmHg))
    .key("hazard.signal_delay", Time, 0.0 * s)
    .derive("hazard.detection", |x|
        (x.alarm_threshold - x.baseline_pressure) * x.compliance / x.min_rate
        + x.signal_delay);
```

**Nickel** (pressures in mmHg, volumes in mL, times in h, by convention):

```nickel
{
  name = "medical.occlusion", version = "0.1.0",
  derive."hazard.detection" = fun x =>
    (x.alarm_threshold - x.baseline_pressure) * x.compliance / x.min_rate
    + x.signal_delay / 3600,   # signal_delay given in s; the author must remember
}
```

**Lean 4** (this one is short enough to prove; lemma names drift across
Mathlib releases, which is part of the cost; not compiled for this note):

```lean
import Mathlib

def occlusionTime (dp c q : Real) : Real := dp * c / q

theorem occlusionTime_antitoneOn (dp c : Real) (hdp : 0 <= dp) (hc : 0 <= c) :
    AntitoneOn (fun q => occlusionTime dp c q) (Set.Ioi 0) := by
  intro x hx y hy hxy
  exact div_le_div_of_nonneg_left (mul_nonneg hdp hc) hx hxy
```

Z3 proves the same in milliseconds from the rlx AST, so the Lean proof buys
nothing here; it is shown to size the Lean cost on the easiest possible
case.

`--explain` (rlx), the failing case:

```
error[fault-reaction-budget]: hazard 'therapy_interrupted': detection
  50.48 min + reaction 0.21 s = 50.49 min exceeds the fault-tolerant time
  interval 30 min by 20.49 min
  detection = 50.48 min   source: derived by medical.occlusion 0.1.0
    inputs
      alarm_threshold    p_th = 300 mmHg    pump_driver.occlusion_limit = 300.0 (launch)
      baseline_pressure  p0   = 20 mmHg
      compliance         C    = 0.003 mL/mmHg
      min_rate           Q    = 1 mL/h
      signal_delay            = 5 s
    derivation
      (p_th - p0) * C / Q            = 280 mmHg * 0.003 mL/mmHg / (1 mL/h)
                                     = 0.84 mL / (1 mL/h) = 50.40 min
      + signal_delay                 = 50.48 min
    worst case
      decreasing in min_rate         proved (z3, 3 ms): the declared minimum
                                     rate is the worst case; every faster
                                     rate detects sooner
      increasing in compliance       proved (z3, 3 ms)
    to fit 30 min at this threshold and compliance: min_rate >= 1.685 mL/h,
      or p_th - p0 <= 166.2 mmHg at 1 mL/h
  info[medical.occlusion.bolus_on_release]: 0.84 mL
```

The "to fit" line is solvable only because the extension's formula is an
analysable expression with declared monotonicity: the checker inverts it by
bisection over the declared domain. A Starlark lambda cannot be inverted
that way without evaluating it blindly, and cannot be trusted to be
monotone.

### 4.4 Model 4: UAV lost-link descent with a battery reserve check

Lost link is detected by a `max_age` on the telemetry heartbeat (core,
bound to the autopilot's timeout parameter). The extension derives the
landing settle and checks an energy predicate that needs core's
`reaction_total`.

Contract:

```yaml
use:
  uav.descent: "^0.2"
hazards:
  lost_link:
    guards: [/gcs/heartbeat]
    on: [omission, late]
    ftti: 120s
    reaction: land
    max_altitude: 120 m             # keys from uav.descent
    hover_power: 600 W
    battery_capacity: 222 Wh
    trigger_soc: autopilot_bridge.bat_low_thr      # param, fraction
    reserve_soc: autopilot_bridge.bat_crit_thr     # param, fraction
nodes:
  link_monitor:
    sub:
      heartbeat:
        max_age: { duration: 10s, param: link_monitor.dl_loss_timeout }
  autopilot_bridge:
    params:
      mpc_z_vel_max_dn: { type: double }
      mpc_land_alt2: { type: double }
      mpc_land_speed: { type: double }
      bat_low_thr: { type: double }
      bat_crit_thr: { type: double }
    paths:
      land:
        trigger: { input: [failsafe_cmd] }
        output: [vehicle_command]
        safe_state:
          emits: vehicle_command
          settle:
            descent_speed: mpc_z_vel_max_dn     # m/s
            slow_altitude: mpc_land_alt2        # m
            land_speed: mpc_land_speed          # m/s
```

(PX4 parameters are not ROS parameters; this assumes a bridge node that
mirrors them, which is itself a modelling decision for the integrator.)

**rlx**:

```
extension uav.descent 0.2.0
requires rlm >= 0.2.0

key hazard.max_altitude      : Length
key hazard.hover_power       : Power
key hazard.battery_capacity  : Energy
key hazard.trigger_soc       : param Fraction
key hazard.reserve_soc       : param Fraction
key settle.descent_speed     : param Velocity
key settle.slow_altitude     : param Length
key settle.land_speed        : param Velocity
input reaction_total : Time

derive settle.duration : Time =
    if max_altitude > slow_altitude
    then (max_altitude - slow_altitude) / descent_speed + slow_altitude / land_speed
    else max_altitude / land_speed
  worst_case:
    increasing in max_altitude
    decreasing in descent_speed, land_speed
    increasing in slow_altitude
  domain: land_speed <= descent_speed

check energy_reserve: hover_power * reaction_total
                      <= battery_capacity * (trigger_soc - reserve_soc)
  worst_case: increasing in reaction_total
```

`increasing in slow_altitude` holds only when `land_speed <= descent_speed`
(`d/dh_slow = 1/v_land - 1/v_desc`). Remove the `domain:` line and Z3
returns a counterexample, which the checker prints:

```
error[rlx-worst-case]: uav.descent 0.2.0: `increasing in slow_altitude`
  is false: counterexample descent_speed = 1 m/s, land_speed = 2 m/s,
  slow_altitude 10 m -> 20 m, max_altitude = 120 m: settle 115 s -> 110 s
  = help: add `domain: land_speed <= descent_speed`, or drop the claim
```

That is the kind of finding a safety reviewer cannot get from any of the
general hosts.

**Starlark**:

```python
def descent_time(h, h_slow, v_desc, v_land):
    if h > h_slow:
        return (h - h_slow) / v_desc + h_slow / v_land
    return h / v_land

extension(
    name = "uav.descent", version = "0.2.0",
    keys = {...},   # as rlx, eight entries
    inputs = ["reaction_total"],
    derive = {"settle.duration": lambda x: descent_time(
        x.max_altitude, x.slow_altitude, x.descent_speed, x.land_speed)},
    checks = {"energy_reserve": lambda x:
        x.hover_power * x.reaction_total
        <= x.battery_capacity * (x.trigger_soc - x.reserve_soc)},
)
```

**Lua**:

```lua
local function descent_time(h, h_slow, v_desc, v_land)
  if h > h_slow then return (h - h_slow) / v_desc + h_slow / v_land end
  return h / v_land
end
-- rlm.extension { ... } as in model 1, with
--   derive = { ["settle.duration"] = function(x) return descent_time(
--       x.max_altitude, x.slow_altitude, x.descent_speed, x.land_speed) end },
--   checks = { energy_reserve = function(x) return x.hover_power * x.reaction_total
--       <= x.battery_capacity * (x.trigger_soc - x.reserve_soc) end },
```

**Rhai**:

```rust
fn descent_time(h, h_slow, v_desc, v_land) {
    if h > h_slow { (h - h_slow) / v_desc + h_slow / v_land } else { h / v_land }
}
// extension(...).derive("settle.duration", |x| descent_time(...))
//               .check("energy_reserve", |x| x.hover_power * x.reaction_total
//                      <= x.battery_capacity * (x.trigger_soc - x.reserve_soc));
```

**Nickel**:

```nickel
let descent_time = fun h h_slow v_desc v_land =>
  if h > h_slow then (h - h_slow) / v_desc + h_slow / v_land else h / v_land
in
{
  derive."settle.duration" = fun x =>
    descent_time x.max_altitude x.slow_altitude x.descent_speed x.land_speed,
  checks.energy_reserve = fun x =>
    x.hover_power * x.reaction_total / 3600       # W * s -> Wh, by hand
      <= x.battery_capacity * (x.trigger_soc - x.reserve_soc),
}
```

The `/ 3600` is the unit bug waiting to happen; in rlx `W * s` and `Wh` are
both Energy and compare directly.

`--explain` (rlx):

```
info[settle-derived]: hazard 'lost_link' -> land -> autopilot_bridge/land
  settle = 50.95 s   source: derived by uav.descent 0.2.0
    max_altitude  h      = 120 m
    slow_altitude h_slow = 10 m       autopilot_bridge.mpc_land_alt2 = 10.0
    descent_speed v_desc = 3 m/s      autopilot_bridge.mpc_z_vel_max_dn = 3.0
    land_speed    v_land = 0.7 m/s    autopilot_bridge.mpc_land_speed = 0.7
    h > h_slow                        120 m > 10 m
    (h - h_slow) / v_desc             = 36.67 s
    h_slow / v_land                   = 14.29 s
    settle                            = 50.95 s
  worst case: increasing in max_altitude (proved), decreasing in
    descent_speed, land_speed (proved), increasing in slow_altitude
    (proved under land_speed <= descent_speed; holds: 0.7 <= 3.0)
info[fault-reaction-budget]: hazard 'lost_link': detection 10.00 s
  (link_monitor/heartbeat max_age) + reaction 50.95 s (route 0.00 s +
  settle 50.95 s) = 60.95 s fits 120 s with 59.05 s of slack
info[uav.descent.energy_reserve]: holds
  hover_power * reaction_total = 600 W * 60.95 s = 10.16 Wh
  battery_capacity * (trigger_soc - reserve_soc) = 222 Wh * (0.15 - 0.07) = 17.76 Wh
  10.16 Wh <= 17.76 Wh (margin 7.60 Wh)
```

### 4.5 Model 5: ETCS-style emergency braking (a piecewise table)

Simplified from SUBSET-026 3.13: a train at `v0` loses traction after
`T_traction_cut_off`, the emergency brake builds up over an equivalent time
`T_be`, then decelerates at `A_ebd(v)`, a step function of speed. No
gradient, no correction factors, no traction during cut-off. This model is
here because it needs a construct the others do not: a bounded sum over a
table.

Contract:

```yaml
use:
  rail.ebd: "^0.1"
hazards:
  eoa_overrun:
    severity: SIL_4
    guards: [/etcs/movement_authority]
    on: [omission, late]
    reaction: obu.emergency_brake
    entry_speed: 160 km/h                # rail.ebd/entry_speed if vehicle.braking is also used
    distance_to_eoa: 1600 m
nodes:
  obu:
    paths:
      emergency_brake:
        safe_state:
          emits: brake_cmd
          settle:
            traction_cut_off: 1.5s
            brake_build_up: 2s
            a_ebd: { 0 km/h: 0.75 m/s^2, 100 km/h: 0.70 m/s^2 }
```

**rlx** (`table` is the only collection type; `sum over` iterates its bands
clipped to an interval, so termination is by the table's length):

```
extension rail.ebd 0.1.0
requires rlm >= 0.2.0

key hazard.entry_speed      : Velocity
key hazard.distance_to_eoa  : Length
key settle.traction_cut_off : Time    "T_traction_cut_off"
key settle.brake_build_up   : Time    "T_be, emergency, equivalent"
key settle.a_ebd            : table Velocity -> Acceleration
    "A_brake_emergency(V); each entry holds from its speed up to the next"

derive settle.duration : Time =
    traction_cut_off + brake_build_up
    + sum over b in a_ebd.bands(0 m/s, entry_speed): (b.hi - b.lo) / b.value
  worst_case: increasing in entry_speed; decreasing in a_ebd

check stop_before_eoa: stopping_distance <= distance_to_eoa
  where stopping_distance =
      entry_speed * (traction_cut_off + brake_build_up)
      + sum over b in a_ebd.bands(0 m/s, entry_speed):
            (b.hi^2 - b.lo^2) / (2 b.value)
  worst_case: increasing in entry_speed
```

**Starlark** (bounded `for` over a list is allowed; `while` is not):

```python
def bands(table, v0):
    # table: sorted list of (speed, decel); yields (lo, hi, a) clipped to [0, v0]
    out = []
    for i, (v, a) in enumerate(table):
        hi = table[i + 1][0] if i + 1 < len(table) else v0
        hi = min(hi, v0)
        if hi > v:
            out.append((v, hi, a))
    return out

def brake_time(x):
    t = x.traction_cut_off + x.brake_build_up
    for lo, hi, a in bands(x.a_ebd, x.entry_speed):
        t += (hi - lo) / a
    return t

def stopping_distance(x):
    d = x.entry_speed * (x.traction_cut_off + x.brake_build_up)
    for lo, hi, a in bands(x.a_ebd, x.entry_speed):
        d += (hi * hi - lo * lo) / (2 * a)
    return d
```

**Lua** and **Rhai** read the same as Starlark with `for i = 1, #t` /
`for (b, i) in t`; Lua's loops are unbounded in general (`while`), so
termination rests on the instruction-count hook, Rhai's on
`max_operations`.

**Nickel**:

```nickel
let bands = fun table v0 =>
  std.array.generate (fun i =>
    let lo = (std.array.at i table).speed in
    let hi = if i + 1 < std.array.length table
             then std.number.min (std.array.at (i + 1) table).speed v0 else v0 in
    { lo = lo, hi = hi, a = (std.array.at i table).decel })
    (std.array.length table)
  |> std.array.filter (fun b => b.hi > b.lo)
in
let brake_time = fun x =>
  x.traction_cut_off + x.brake_build_up
  + std.array.fold_left (fun acc b => acc + (b.hi - b.lo) / b.a) 0
      (bands x.a_ebd x.entry_speed)
in ...
```

**CUE**: `list.Sum([for b in _bands {(b.hi - b.lo) / b.a}])` works and is
total; the band construction is a comprehension over indices. **Dhall**:
the fold is expressible (`List/fold`), the arithmetic is not.

`--explain` (rlx):

```
info[settle-derived]: hazard 'eoa_overrun' -> obu/emergency_brake
  settle = 64.35 s   source: derived by rail.ebd 0.1.0
    entry_speed v0 = 44.444 m/s       160 km/h
    traction_cut_off = 1.5 s, brake_build_up = 2 s
    a_ebd bands over [0, 44.444 m/s]
      [27.778, 44.444) m/s  at 0.70 m/s^2   dv / a = 23.81 s
      [0, 27.778) m/s       at 0.75 m/s^2   dv / a = 37.04 s
    settle = 1.5 s + 2 s + 23.81 s + 37.04 s = 64.35 s
info[rail.ebd.stop_before_eoa]: holds
  stopping_distance = 44.444 m/s * 3.5 s          = 155.56 m
                    + (44.444^2 - 27.778^2) / 1.4 = 859.79 m
                    + (27.778^2 - 0) / 1.5        = 514.40 m
                    = 1529.75 m <= 1600 m   (margin 70.25 m)
  worst case: increasing in entry_speed (proved per contract: z3, table
    unrolled to 2 bands, 22 ms)
```

Note the worst-case claim over a table is discharged per contract (the
table is concrete, the speed symbolic), not once per extension. That is
fine for the checker, but it means the extension author cannot know in
advance that every user's table will prove; Lean could prove it once for
all tables (decreasing step function) at real cost.

### 4.6 Lean 4 as a companion: what it can prove and what it costs

What is provable and worth it:

- Monotonicity and its direction for any of the models above, including
  over all tables (model 5) and with transcendental terms that Z3 cannot
  handle.
- That an extension's two outputs are consistent (e.g. that the time and
  distance of model 5 come from the same motion: `d = integral of v dt`).
- That a closed form equals the integral of the physical ODE it claims to
  solve (the jerk-limited braking profile), which no one checks today.

Costs:

- Toolchain: `elan` + a pinned Lean (~hundreds of MB) and Mathlib (a
  multi-GB build cache). Not something a contract checker can require of
  an integrator, nor run in a CI that checks launch files.
- Authors: proof engineering is a separate skill. A safety engineer will
  not write the 40-80 lines of model 1; an extension maintainer with a
  Lean colleague might.
- Gap between proof and evaluation: Mathlib's `Real` is noncomputable. The
  checker evaluates rlx in `f64`; Lean proves facts about the `Real`
  formula. Linking them is a hash plus a review that the two formulas are
  the same text (a translator from rlx to Lean would close most of that
  gap, and is a natural follow-on if Lean proves its worth).
- Churn: Mathlib lemma names change between releases (the `div_le_div_*`
  family was renamed during 2024-2025 [unverified exact release]); a pinned
  toolchain in the extension's `lakefile` is mandatory.

So: Lean is an optional proof certificate shipped by an extension
(`proofs/` directory, `lake build` in the extension's CI, a hash in the
extension header). The contract checker never runs Lean; it prints
`proved (lean, ext CI, hash ...)` when the hash matches and the claim is
one Z3 could not settle.

---

## 5. A profile extension in each candidate

The profile is: an ordered severity scale, and a map from severity to the
scheduling criticality core consumes (`high | medium | low`, first entry
none). Shown for IEC 61508 SIL; DO-178C DAL and IEC 62304 classes are the
same shape.

**YAML data** (recommended; no language needed):

```yaml
profile: iec61508
version: 1.0.0
severity_scale: [none, SIL_1, SIL_2, SIL_3, SIL_4]
criticality: { none: null, SIL_1: low, SIL_2: medium, SIL_3: high, SIL_4: high }
```

**rlx** (only needed when a rule is computational, e.g. ISO 26262
decomposition, section 2.5):

```
profile iec61508 1.0.0
scale severity = [none, SIL_1, SIL_2, SIL_3, SIL_4]
criticality(s) = match s { none => none, SIL_1 => low, SIL_2 => medium,
                           SIL_3 | SIL_4 => high }
```

**Starlark**:

```python
profile(
    name = "iec61508", version = "1.0.0",
    severity_scale = ["none", "SIL_1", "SIL_2", "SIL_3", "SIL_4"],
    criticality = {"SIL_1": "low", "SIL_2": "medium", "SIL_3": "high", "SIL_4": "high"},
)
```

**Lua**: the same table literal in `return { ... }`. **Rhai**: an object
map `#{ ... }`. **Jsonnet**: identical to JSON.

**Nickel** (its strength: the profile is a typed record, and a contract can
reject an unknown label with blame pointing at the contract line):

```nickel
let Sil = std.enum.TagOrString |> [| 'none, 'SIL_1, 'SIL_2, 'SIL_3, 'SIL_4 |] in
{
  name = "iec61508", version = "1.0.0",
  severity_scale | Array Sil = ['none, 'SIL_1, 'SIL_2, 'SIL_3, 'SIL_4],
  criticality = { SIL_1 = 'low, SIL_2 = 'medium, SIL_3 = 'high, SIL_4 = 'high },
}
```

(the enum contract spelling is approximate [unverified]).

**CUE** (its strength: the scale is a disjunction, so a contract value is
validated by unification):

```cue
#Severity: "none" | "SIL_1" | "SIL_2" | "SIL_3" | "SIL_4"
severity_scale: ["none", "SIL_1", "SIL_2", "SIL_3", "SIL_4"]
criticality: [S=#Severity]: "low" | "medium" | "high" | null
criticality: {none: null, SIL_1: "low", SIL_2: "medium", SIL_3: "high", SIL_4: "high"}
```

**Dhall** (a union type and a total function; Dhall's best case):

```dhall
let Sil = < none | SIL_1 | SIL_2 | SIL_3 | SIL_4 >
let Crit = < low | medium | high >
let criticality =
      \(s : Sil) ->
        merge { none = None Crit, SIL_1 = Some Crit.low, SIL_2 = Some Crit.medium
              , SIL_3 = Some Crit.high, SIL_4 = Some Crit.high } s
in  { scale = [ Sil.none, Sil.SIL_1, Sil.SIL_2, Sil.SIL_3, Sil.SIL_4 ], criticality }
```

**Pkl / KCL**: a typealias of string literals plus a `Mapping`/dict; both
read well. **CEL**: no definitions, unsuitable.

**Lean 4** (overkill; shown to make the point):

```lean
inductive Sil | none | sil1 | sil2 | sil3 | sil4
  deriving DecidableEq, Repr, Ord
inductive Crit | low | medium | high

def criticality : Sil -> Option Crit
  | .none => none | .sil1 => some .low | .sil2 => some .medium
  | .sil3 => some .high | .sil4 => some .high

theorem criticality_monotone : ... -- the scale and the map agree in order
```

The one property worth checking about a profile is that the map is
monotone in the scale (a higher severity never gets a lower criticality);
rlx states it as `criticality: monotone` and the checker verifies it by
enumeration, which needs neither Z3 nor Lean.

---

## 6. Cross-cutting notes

### 6.1 Determinism and numbers

rlx evaluates in IEEE-754 `f64` with no fused or reordered operations and
prints results rounded the way budgets already are (up, to the next
nanosecond for durations, and the `budget-rule` convention of rounding a
derived budget up to the next ms where a budget is compared). `sqrt` is
correctly rounded in IEEE-754, so results are bit-identical across hosts.
No clock, no environment, no randomness, no file access other than the
extension file itself and its declared `import`s (other rlx files in the
same package).

### 6.2 Sandboxing summary

- rlx: nothing to sandbox; the language has no effects.
- Starlark: hermetic by specification; only host-registered globals exist.
- Lua (mlua): load with a restricted environment (no `io`, `os`, `debug`,
  `package`, `load`, `math.random`); set an instruction-count hook. C code,
  so a memory-safety bug in Lua is in-process.
- Rhai: no IO unless registered; set `max_operations`, `max_call_levels`,
  `max_string_size`, `max_array_size`; disable `eval`.
- Nickel, Jsonnet: pure; imports must be confined to the extension package.
- CUE, Pkl, Lean: out of process; sandbox is the subprocess's.

### 6.3 Embedding in the Rust checker

| host | crate (latest seen) | notes |
|---|---|---|
| rlx | in-tree | parser (e.g. `chumsky` or hand-written), dimension checker, tracing evaluator, Z3 encoder behind the existing `smt` feature |
| Numbat | `numbat` 1.24.0, MIT/Apache-2.0 | disable default features (`fetch-exchangerates`, `plotting`); supply own prelude; AST pass to refuse recursion and impure builtins |
| Starlark | `starlark` 0.14.2, Apache-2.0 (Meta) | mature (Buck2 uses it); Quantity as a `StarlarkValue` with arithmetic; typechecker is optional |
| Lua | `mlua` 0.12.2, MIT | vendored Lua 5.4 or Luau (Luau adds gradual types, not units) |
| Rhai | `rhai` 1.26.1, MIT/Apache-2.0 | pure Rust, custom types and operators easy; dynamic |
| Nickel | `nickel-lang-core` 0.19.0, MIT | language stable since 1.0, core crate API is not; heavy |
| Dhall | `dhall` / `serde_dhall` 0.13.0 | conformance to latest standard lags [unverified] |
| Jsonnet | `jrsonnet-evaluator` 0.5.0-pre | pre-release versioning |
| CEL | `cel-interpreter` 0.10.0 | small; expressions only |
| CUE | none (Go); libcue C API | cgo shared library or subprocess |
| Pkl | `pkl-rs` 0.0.0 | effectively subprocess to `pkl server` |
| KCL | Rust core in the kcl-lang repo | not a stable crates.io dependency [unverified] |
| Lean 4 | `lean-sys` 0.0.9 | in-process use impractical; run as CI tool only |

Where it runs: extensions are evaluated by the contract checker on the
host. Nothing of the extension reaches an embedded image; the derived
values do, exactly as a written `settle: 2525ms` would. RTOS consumers
(nano-ros) are unaffected beyond reading derived numbers.

### 6.4 Versioning and distribution

- **File**: `share/<pkg>/rlm/<name>.rlx` (or `.profile.yaml`), installed by
  the package's CMake `install(DIRECTORY rlm/ ...)`. The package registers
  an ament index resource of type `rlm_extensions` so the checker finds
  extensions without scanning `share/` (scans are what made workspace
  checks slow before).
- **Name and version**: in the file header (`extension vehicle.braking
  1.2.0`); semver. Major bump when a key is removed or renamed, or a
  derivation changes a value for the same inputs; minor for new keys or
  outputs; patch for text.
- **Use**: `use: { vehicle.braking: "^1.2" }` in the contract, plus
  `package.xml` `<exec_depend>` (or `<test_depend>` if only the checker
  needs it) on the providing package.
- **Lock**: the checker prints the resolved version and sha256 in
  `--explain`, and writes them into the derived budget's `source` (e.g.
  `source: { derived: { by: vehicle.braking, version: 1.2.0, sha256: ... } }`),
  so a contract's evaluated numbers can be reproduced and a silent
  extension change is visible in a diff of the explain output.
- **Grammar coupling**: `requires rlm >= 0.2.0` in the extension, checked
  like the contract's `rlm:` key today.

### 6.5 Error messages a safety engineer will see

Design target, rlx: every error names the extension, version, file:line,
the contract key and its file:line, both dimensions in an `m/s`-style
spelling (not a dimension vector), and a `help:` with the likely fix. The
units-in-contract errors are the most frequent and must be the best:

```
error[unit-mismatch]: system.contract.yaml:9:18: entry_speed: 15.2 km/s
  is Velocity, but 54720 km/h exceeds the extension's domain
  (0 m/s, 70 m/s]; did you mean km/h?
error[unit-missing]: system.contract.yaml:9:18: entry_speed: 15.2
  needs a unit (Velocity: m/s, km/h, mph)
```

---

## 7. Open questions

1. **Own a language or adopt one?** rlx is the best fit but is rlm's to
   maintain forever. Embedding Numbat (restricted) is the cheaper first
   step and keeps the door open to rlx later with nearly the same surface.
   Which is preferred: build rlx directly, or prototype on Numbat first?
2. **Bare vs qualified keys.** Bare domain names (`decel`) when unambiguous,
   qualified (`vehicle.braking/decel`) on collision: acceptable, or should
   every extension key be qualified always (safer, uglier)?
3. **What may an extension produce?** Proposed: core Bound keys (`settle`,
   `ftti`, a hazard detection bound, `window`) with `source: derived`, plus
   opaque predicates. Is a hazard-level `detection:` Bound (model 3) a core
   addition you accept? It is needed whenever detection is a physical
   integration rather than a QoS/timer mechanism.
4. **Core values as extension inputs.** Models 2 and 4 need core's
   `reaction_total` (detection, route, settle) per hazard as a read-only
   input. Accept that dependency direction (core -> extension -> core
   predicate)? It creates an evaluation order: core timing first,
   extension predicates after.
5. **Default severity scale.** Today ISO 26262 ASIL is the default. Move it
   to an `iso26262` profile with no default (a contract with `severity:`
   must `use:` a profile), or keep it as the built-in default for
   compatibility?
6. **Worst-case policy.** When a `worst_case` claim is `unproved` (Z3
   timeout or unsupported function), should the derived bound be an error,
   a warning, or an info? Proposed: warning, and the derived value is
   printed with "not shown to be the worst case".
7. **Distance-deadline hazards (rail, machinery).** A hazard whose deadline
   is a distance predicate and not an `ftti`: allow a hazard with no `ftti`
   when an extension predicate stands in for it, or require a time bound
   anyway?
8. **Lean.** Worth carrying at all in the design (as an optional extension
   CI artifact), or drop it and rely on Z3 plus review?
9. **Migration.** `entry_speed` and `settle: { decel, jerk }` are grammar
   today (since v0.1.46). Ship `vehicle.braking` as a bundled extension
   that the checker loads implicitly for one minor version with a
   deprecation warning, then require `use:`?

---

## Appendix A. Arithmetic of the worked examples

All computed in `f64`, printed to two decimals.

- Model 1: a = 2.5 m/s^2, j = 1.5 m/s^3, v_r = 6.25 / 3 = 2.0833 m/s.
  v0 = 4.23 m/s: a/j = 1666.67 ms, (v0 - v_r)/a = 858.67 ms, braking
  2525.33 ms (matches `docs/launch-manifest.md`). v0 = 15.2 km/h = 4.2222
  m/s: (v0 - v_r)/a = 855.56 ms, braking 2522.22 ms, + response 300 ms =
  2822.22 ms. v0 = 3.0 m/s: 2033.33 ms.
- Model 2: T = 15 + 192 = 207 ms; C = 8 (30 - 14) = 128 mm; 2000 * 0.207 +
  128 = 542.0 mm > 500; 1600 * 0.207 + 128 = 459.2 mm; max(459.2, 500) =
  500 mm; 520 - 500 = 20 mm margin. S(T) is continuous at T = 0.186 s (where
  2000 T + C = 500 mm) and non-decreasing.
- Model 3: (300 - 20) mmHg * 0.003 mL/mmHg = 0.84 mL; / 1 mL/h = 0.84 h =
  50.40 min; + 5 s = 50.48 min. Rate to fit 30 min: 0.84 mL / (30 min - 5 s)
  = 1.685 mL/h. Pressure span to fit at 1 mL/h: (30 min - 5 s) * 1 mL/h /
  0.003 mL/mmHg = 166.2 mmHg.
- Model 4: 110 / 3 = 36.67 s; 10 / 0.7 = 14.29 s; 50.95 s. With 10 s
  detection: 60.95 s; 600 W * 60.95 s = 36571 J = 10.16 Wh; 222 Wh * 0.08 =
  17.76 Wh. Counterexample: v_desc = 1, v_land = 2, h = 120: h_slow = 10 ->
  110 + 5 = 115 s; h_slow = 20 -> 100 + 10 = 110 s.
- Model 5: 160 km/h = 44.444 m/s; 100 km/h = 27.778 m/s; 16.667 / 0.70 =
  23.81 s; 27.778 / 0.75 = 37.04 s; total 64.35 s with 3.5 s. Distance
  155.56 + 859.79 + 514.40 = 1529.75 m.

## Appendix B. Sources consulted

Primary or near-primary where available; everything else is marked
`[unverified]` in the text.

- rlm grammar: `types/src/field_table.rs`, `docs/format-reference.md`,
  `docs/launch-manifest.md` at `6211471`.
- UN R157 / EU 2021/389 text (EUR-Lex) for MRM definitions; the 10 s
  transition demand rule via L3Pilot slides (secondary).
- ERA, "Introduction to ETCS braking curves" and the ERA braking curves
  tool handbook (aligned with SUBSET-026 3.13).
- FAA AC 25.1309-1B (table of contents seen; probability targets via the
  ARP4761 summary).
- PX4 user guide, Safety (Failsafe) Configuration, v1.15-v1.17.
- IEC 60601-1-8 terms and subclause 6.4 rationale (secondary
  reproduction).
- IEC 60204-1 stop categories, ISO 13855 formula, ISO/TS 15066 SSM terms,
  IEC 61511 PST: vendor and practitioner sources (Lenze, Keyence, Leuze,
  Kenexis, Robotiq, NIST); clause numbers unverified.
- EN 81-20 figures: secondary engineering sources.
- Crate versions: `cargo search` on 2026-10-15.
