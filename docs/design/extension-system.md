# The extension system: domain vocabulary outside the core

Status: **design draft** (2026-10-15), stage 2 of the grammar work. Stage 1
(play_launch phase 90, "a tidy grammar") folds and renames core keys and
lands first; this design assumes its result. Inputs:
`extension-language-exploration.md` (languages compared, domains surveyed)
and `extension-examples.md` (seven domains worked).

## 1. Goals and non-goals

Goals:

- **A small core.** rlm keeps a handful of primitives every ROS user needs.
  Domain vocabulary (a vehicle's braking profile, a press's safety
  distance, an infusion pump's occlusion time, a severity scale) lives in
  extensions.
- **Readable contracts.** A contract keeps domain names
  (`entry_speed: 30 km/h`, `decel: target_acceleration`), not generic ones.
- **Reviewable derivations.** Every derived number shows its formula,
  inputs and branch in `--explain`, with the extension's version and hash.
- **Units and worst cases checked by the tool.** A unit error is caught
  when an extension loads, in every branch; a claimed worst-case direction
  is proved or reported.
- **Extensions without rebuilding the checker.** An extension is a file
  shipped in a ROS package.

Non-goals:

- Extensions do not add checker rules, change core semantics or run code
  with effects.
- No general-purpose language: no loops beyond bounded sums, no recursion,
  no IO, no clock, no randomness.
- No proof assistant on the user's machine.

## 2. The core after stage 1

Seven primitives:

| Primitive | Holds |
|---|---|
| entity | nodes, endpoints, channels (topic, service, action), parameters |
| path | trigger (input or timer) to outputs inside a node |
| bound | a duration, rate or count; its kind (latency, age, transport, jitter, response, rate, count, deadline, least time); where it applies; `source`; optional `param` binding |
| predicate | a condition over a message field (`functions`, `when`) |
| mode | a degraded state: requires, fallback, window, exit |
| hazard | guard, fault classes, deadline (`ftti`), reaction |
| safe state | what is emitted, and the time to reach it (`settle`, a bound) |

The readable keys (`max_latency`, `max_age`, `ftti`, `window`, `settle`,
`response`) are spellings of a bound. Stage 2 adds to the core only what
the seven domains showed every extension interface needs:

- `hazard.detection`: a bound, optional; the time from the fault to its
  detection when that is physics, not a timeout (medical).
- `hazard.deadline_by: <extension>/<check>`: a hazard whose requirement is a
  check, not a time (machinery's safety distance, rail's end of authority).
  A hazard has an `ftti`, a `deadline_by`, or both.
- `reaction_total`: a read-only value per hazard (detection + route +
  settle), available to extension checks (machinery, UAV energy).

## 3. What an extension is

An extension is one rlx file, or a YAML profile, in a ROS package. It may:

- **declare keys** in a core context, each with a dimension (`key
  hazard.entry_speed : Velocity`) or as a parameter name
  (`param Acceleration magnitude`);
- **read inputs**: core values (`settle.response`, `reaction_total`);
- **derive** exactly the core bounds named in section 4;
- **check** a predicate (a distance, an energy) that the checker reports
  like any rule, at the severity the contract gives it (`deadline_by`
  makes it the hazard's requirement);
- **report** a value for the reviewer, with no verdict (a bolus volume).

It may not add rules, read files, read the clock, or call another
extension.

## 4. The interface: derive targets

| Target | Domains that needed it |
|---|---|
| `settle.duration` | vehicle, UAV, rail, process |
| `hazard.detection` | medical |
| `hazard.ftti` | process (process safety time from the process's dynamics) |

A derived bound carries `source: derived` with a record of the extension,
version, sha256, inputs and trace. Every core rule reads it as it reads a
written bound. A contract may not write a bound an extension derives for the
same place (`derive-conflict`), except a measured `settle` beside a derived
one, which the core already compares (`settle-conflict`).

## 5. The language: rlx

### 5.1 Two layers

- **The extension format (owned by rlm):** `extension`, `requires`, `key`,
  `input`, `derive`, `check`, `report`, `fn`, `where`, `worst_case`,
  `domain`, `dimension`. This layer binds to rlm's contexts, and nothing
  existing provides it.
- **The expression core (rlx's own; E0 ruled out borrowing Numbat, section
  13):** arithmetic with units and dimensions, `if/then/else`, `min`,
  `max`, `sqrt`, powers, a bounded `sum over` a table's bands. Numbat (Rust,
  MIT/Apache, dimensions as static types, a full unit library) was the
  candidate, embedded with a restricted prelude and recursion refused; its
  public API cannot check a file without running it and does not expose the
  expression tree the trace, the recursion check and the Z3 translation
  need.

### 5.2 Properties

- **Total**: no recursion; `sum over` ranges over a finite table; every
  expression terminates.
- **Pure**: no IO, clock or randomness; the same inputs give the same bits
  (f64, IEEE-754, fixed evaluation order).
- **Unit-checked at load**: every expression in every branch, so a unit
  error in a branch no contract reaches is still refused.
- **Traceable**: the evaluator records every `where` binding, the branch
  taken and the values, which `--explain` prints.

### 5.3 Characters

Rules taken from Lean 4, Coq and Isabelle, made stricter where a safety
reviewer needs it:

- UTF-8, NFC required (a non-NFC file is refused, not normalized).
- Identifiers start with an ASCII letter, `_`, or Greek
  `α β γ δ ε ζ η θ κ μ ξ π ρ σ τ φ χ ψ ω Γ Δ Θ Λ Ξ Φ Ψ Ω`; confusable
  Greek (`ο ν ι υ` and capitals that look Latin) is refused; `λ Π Σ` are
  reserved (as in Lean).
- Identifiers continue with digits, `_`, `'`, and subscripts (Lean's
  ranges: `₀-₉`, `ₐ-ₜ`, `ᵢ-ᵪ`, `ⱼ`); one script per identifier.
- Each Unicode operator has exactly one ASCII twin:
  `→ ->`, `≤ <=`, `≥ >=`, `≠ !=`, `· *`, `÷ /`, `− -`, `√ sqrt`,
  `² ^2`, `³ ^3`, `∈ in`, `Σ sum`.
- Refused everywhere, comments and strings included: bidi controls,
  zero-width characters, other control characters.
- **The canonical form is ASCII** (Isabelle's idea): the hash, the version
  identity and diffs use it. `rlx fmt --ascii | --unicode` converts;
  editors offer `\alpha`-style input (Lean's idea); `--explain --pretty`
  renders symbols.
- Contract keys stay ASCII.

## 6. Worst cases

A `derive` or `check` may declare its direction over a domain:

```
worst_case: increasing in entry_speed, response; decreasing in decel, jerk
domain: entry_speed in (0 m/s, 70 m/s]
```

- The checker proves it with Z3 (already in play_launch behind `smt`) over
  the declared domain; nonlinear terms (`sqrt`, division) are encoded with
  side conditions.
- Proved: `--explain` says so. Refuted: an error with Z3's counterexample
  and, where found, the domain that would make it hold. Unknown (a timeout,
  or beyond Z3): a warning, an error under `--strict`.
- Why it matters: the checker evaluates a derived bound at the contract's
  declared value (the entry speed, the minimum rate) and calls it the worst
  case. The direction is what makes that true; board runs at the declared
  entry speed showed the difference between a profile and the worst run.
- A heavier proof (Lean, KeYmaera X) may live in an extension's own CI and
  be recorded by hash; the checker never runs it.

## 7. Profiles

A profile is YAML data: an ordered severity scale and its map to scheduling
criticality.

```yaml
profile: iec61508
version: 1.0.0
requires: { rlm: ">=0.2.0" }
severity_scale: [none, SIL_1, SIL_2, SIL_3, SIL_4]
criticality: { none: null, SIL_1: low, SIL_2: medium, SIL_3: high, SIL_4: high }
```

`iso26262`, `iec61508`, `iec61511`, `do178c`, `iec62304`, `iso13849`,
`en50129` ship as reference profiles. The core keeps "severity is a label
on a declared ordered scale"; ASIL leaves the core default (section 10).
ISO 26262 decomposition is the one rule that needs a function; it is an rlx
`fn` in the `iso26262` profile.

## 8. Packaging, resolution and identity

- An extension ships in `share/<pkg>/rlm/` and registers in the ament index
  (resource type `rlm_extensions`).
- A contract names what it uses: `use: { vehicle.braking: "^1", iso26262:
  "^1" }`. Resolution follows the launch tree's ament prefix path.
- Keys are bare while exactly one used extension declares them in that
  context; two declaring the same key need the qualified spelling
  (`vehicle.braking/decel:`).
- `--explain` prints each derived value's extension, version and the
  sha256 of its canonical ASCII form. `check --locked` refuses a resolution
  that differs from `rlm.lock` beside the contract (written by
  `play_launch check --update-lock`).
- Semver: a change to a derive's formula or a key's dimension is a major
  version; a widened domain or a new key is a minor.

## 9. What leaves the checker unchanged

- Core rules read bounds; a derived bound is a bound. Fault-reaction sums,
  ladder rungs, window rules and `--explain`'s tables are unchanged in
  shape; they gain a `derived by` column.
- **nano-ros** reads the resolved model play_launch exports. Derived values
  appear there as plain bounds with their source, so the image build never
  evaluates rlx, and its runtime monitors and sizing are unchanged.

## 10. Migration

- `hazard.entry_speed` and `settle: { decel, jerk }` become keys of a
  bundled `vehicle.braking` extension. One minor release loads it
  implicitly when a contract writes those keys without a `use:`, with a
  `deprecated-implicit-extension` warning and a fix-it; the next requires
  `use:`.
- The ASIL default scale moves to the bundled `iso26262` profile on the
  same schedule: a contract with `severity: ASIL_x` and no profile gets the
  warning, then an error.
- `play_launch migrate` (from stage 1) adds the `use:` lines.

## 11. Decisions (folded)

| Question | Decision |
|---|---|
| Language | Own the extension format and the expression core (E0: Numbat cannot be embedded as published, section 13) |
| Unicode | Allowed under 5.3; canonical form ASCII |
| Bare or qualified keys | Bare unless two used extensions collide |
| Core `detection` bound | Yes |
| `reaction_total` as an input | Yes; core timing first, extension checks after |
| A hazard with no `ftti` | Yes, with `deadline_by` naming the check |
| ASIL default | Moves to the `iso26262` profile, with a deprecation release |
| Unproved worst case | Warning; error under `--strict`; refuted is an error |
| Lean, KeYmaera X | Optional, in an extension's CI, recorded by hash |
| Migration of the vehicle keys | Bundled `vehicle.braking`, implicit for one minor release |

## 12. Open

Found while building the front end (E2, `rlx/`), for E3 and later:

- The canonical form is ASCII for operators and keywords, but identifiers
  stay as written: allowing Greek identifiers and an all-ASCII canonical
  form cannot both hold. The hash is over this form.
- One script per identifier refuses `Δt` and `ω_max`, the most natural
  physics names, though `Δ` and `ω` are allowed letters. Decide whether a
  Greek letter may lead a Latin tail.
- How expressions name keys is unspecified. E2 reads the last segment
  (`settle.response` is `response`), which collides when two contexts
  declare the same key name; E3 needs a rule.
- Units and variables: after a number, a name on the unit list is a unit,
  any other name a factor (`30 km/h` a quantity, `2 v0` a product). The
  design never says implicit multiplication exists; the examples need it.
  Single-letter variable names common in the examples (`T`, `C`, `t`, `d`)
  are left off the unit list, and a binding named like a unit is refused.
- `param <Dim> unit mmHg` (example 3) is not in section 3, which names only
  `magnitude`.
- Version requirements are spelled `requires rlm >= 0.2.0` in rlx and
  `requires: { rlm: ">=0.2.0" }` in a YAML profile.
- Interop export to SysML v2 calculations or AADL property sets (later).
- Whether `check` severities other than error and warning are needed.

## 13. E0 result: Numbat is not embedded (2026-10-17)

The spike (`spikes/e0-numbat/`, `cargo run --release` there) embeds
`numbat` 1.24.0 with `default-features = false` (the defaults fetch
exchange rates over the network and pull in plotting), a custom
`ModuleImporter` whose prelude names only `core::{scalar, quantities,
dimensions, functions, lists, numbers}`, `math::constants`, `units::si` and
`units::time`, and refuses `core::random`, `core::debug`, `datetime`,
`plot` and `extra`. `core::error` and `core::strings` stay: the prelude's
own modules import them, and both are pure.

What works:

- **Dimensions are checked at load, in every branch.** A function whose
  untaken `else` returns a length where a time is declared is refused
  ("Incompatible types in condition"), and the exploration's unit bug
  `sqrt(2 v0 / a)` is refused ("Could not solve the following constraint:
  Time = Scalar") before anything calls it.
- **The unit library**: `30 km/h -> m/s` is 8.33333 m/s.
- **Both ports compute the documented numbers.** `vehicle.braking` gives
  4422.67 ms for 30 km/h, 2.5 m/s^2, 1.5 m/s^3 and a 256 ms response (the
  low-speed branch also evaluates). `rail.ebd` gives 64.35 s and 1529.75 m
  once its table arrives as a list of `struct Band { lo, hi, a }`: Numbat
  has neither lambdas nor partial application, so `map` needs a
  one-argument function, and slicing the table into bands capped at the
  entry speed is the format layer's job (`a_ebd.bands(...)`) anyway.
- **Mutual recursion cannot occur**: a name must be defined before it is
  called.
- **Cost**: 74 crates, +2.7 MB stripped (0.34 to 3.04 MB against a
  hello-world), a clean release build of 6.5 s on 48 cores and 13.9 s at
  `-j4`.

What does not, and decides it:

- **No check without running.** `Context::interpret` resolves, type-checks
  and executes in one call, and returns the typed statements only after
  they ran; there is no parse- or check-only entry point. A file's
  top-level statements therefore run at load, before a loader can refuse
  them: in the spike `print(1)` printed and a declared `random()` returned
  a random number before the refusal pass saw either. Refusing a file
  after its effects ran is not refusing it.
- **Bodyless `fn` declarations bind to Rust FFI by name.** A restricted
  prelude is not a restriction: `fn random() -> Scalar` restores
  `random()`, and `fn now() -> DateTime` would restore the clock. The
  public `Statement` shows such a declaration (`body: None`) and the
  procedures (`ProcedureCall`: `print`, `assert`, `assert_eq`, `type`,
  which are grammar, not prelude), but only after execution, as above.
- **The expression tree is private.** `numbat::Statement` is public;
  `typed_ast::Expression`, the parser and the tokenizer are not. A function
  body cannot be named or walked, so three things E0 had to show are out
  of reach: the trace of every `where` binding and branch (the bytecode VM
  has no hook either), the Z3 translation of an expression, and refusing
  direct self-recursion, which Numbat accepts (`f(5)` ran). Only scraping
  `Debug` output would reach them, which is not a foundation.
- **Characters**: Numbat accepts a Cyrillic `а` and a Greek `ο` in
  identifiers, a bidi override in a comment and a non-NFC identifier; only
  zero-width characters are refused. Section 5.3 needs a pre-pass over the
  raw text whatever the expression core is, so this alone would not have
  decided it.

Options considered: embedding as published fails the first and third
points. Forking Numbat to expose its parser, a check-only entry point and
the typed tree is possible, but it is a fork of about 27 kLOC of Rust to
track, and the trace still needs hooks in its VM. Using Numbat to
type-check beside an
rlx parser for the trace and Z3 means two parsers of one syntax that must
agree, which is the two-copies defect the contract rules exist to remove.

Decision: rlx has its own expression core (E2 grows by about 2-3 kLOC, as
planned for this outcome): an expression AST rlx owns, dimensions as
vectors of base-dimension exponents, a small unit table (SI base and
derived units, the prefixes, `h`, `min`, `km/h`, `g`), a tree-walking
evaluator that records each binding and branch, and the Z3 encoding over
the same tree. Numbat's checking rules (a dimension per expression,
branches unified, `sqrt` halving exponents) are the reference for its
type checker.

One number to carry into E8: the example in `extension-examples.md` wrote
`entry_speed: 30 km/h` beside 4421.33 ms, which is the arithmetic for a
written `8.33` m/s (what the island contract states). Exact 30 km/h is
4422.67 ms. Migrating a contract from `8.33` to `30 km/h` moves its totals
by 1.34 ms, so E8's identical-totals gate keeps the value as written.
