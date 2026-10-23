# Writing an extension

An **extension** adds a domain's vocabulary to contracts without changing
the checker: a few keys a contract may write on a hazard or a safe state,
and the arithmetic that turns them into a bound the core already checks (a
settle, a detection time, an interval) or into a check of its own (a
distance, an energy). It is one file in rlx, a small language with units of
measure, shipped in a ROS package. This guide is for the author of one.
The keys of the extensions this repository publishes are listed in
[extension-reference.md](extension-reference.md); the core grammar is
[format-reference.md](format-reference.md); the design and its reasons are
[design/extension-system.md](design/extension-system.md), and seven domains
are worked in [design/extension-examples.md](design/extension-examples.md).

An extension is the wrong tool for a severity scale: that is a **profile**,
YAML data (`<name>.profile.yaml`: a scale and its criticality map), and
seven reference profiles ship with the checker (`iso26262`, `iec61508`,
`iec61511`, `do178c`, `iec62304`, `iso13849`, `en50129`).

## A first extension

A conveyor stops by braking its belt at a fixed deceleration. Its settle is
the drive's response plus the time to shed the belt speed, and a hand
inside the guard must be farther than the belt travels meanwhile:

```rlx
-- share/rlm_ext_conveyor/rlm/belt.rlx
extension machine.belt 0.1.0
requires rlm >= 0.2.0

key hazard.belt_speed       : Velocity                   "the fastest belt speed"
key hazard.guard_distance   : Length                     "guard opening to the nip point"
key settle.decel            : param Acceleration magnitude
input settle.response       : Time = 0 s                 -- the core bound, written on the settle

derive settle.duration : Time = response + belt_speed / decel
  worst_case:
    increasing in belt_speed, response
    decreasing in decel
  domain: belt_speed > 0 m/s, decel > 0 m/s^2

check clear_of_nip: travel <= guard_distance
  where travel = belt_speed * response + belt_speed^2 / (2 decel)
  worst_case: increasing in belt_speed
  domain: belt_speed > 0 m/s, decel > 0 m/s^2, response >= 0 s
```

And a contract that uses it:

```yaml
use: { machine.belt: "^0.1", iso13849: "^1" }
hazards:
  reach_into_nip:
    severity: PL_d
    guards: [/conveyor/guard_switch]
    on: reported
    when: { field: open, equals: true }
    ftti: 500ms
    reaction: conveyor.stop
    belt_speed: 0.8 m/s
    guard_distance: 300 mm
nodes:
  belt_drive:
    params:
      stop_decel: { type: double }
    paths:
      stop:
        trigger: { input: [guard] }
        output: [drive_cmd]
        max_latency: 10ms
        safe_state:
          emits: drive_cmd
          settle: { decel: stop_decel, response: 40ms }
```

`play_launch check --explain` then charges the derived settle in the
hazard's fault-reaction sum like a written one, prints the derivation's
trace with the extension's version and sha256, runs the check with the
hazard's `reaction_total` available if it asks for it, and states which
worst-case claims Z3 proved.

## The file

**Header.** `extension <name> <version>` first: the name is dotted
lowercase words (`machine.belt`; the first word is the domain), the version
`MAJOR.MINOR.PATCH`. `requires rlm >= 0.2.0` names the grammar the file
needs; the checker refuses an extension whose requirement it does not meet
(`extension-invalid`). The extension system is grammar 0.2.0.

**Keys** a contract writes, each in a context: `hazard.` (on the hazard) or
`settle.` (on a safe state's `settle:` map). A key has a type:

- a dimension, `key hazard.belt_speed : Velocity`: the contract writes a
  quantity with a unit (`0.8 m/s`, `30 km/h`, `15ms`); a number without a
  unit is refused, so the unit the author meant is never guessed;
- `param <Dim>`: the contract writes a node **parameter name**, and the
  checker reads that parameter's launch value -- on a `settle.` key, from
  the node whose path declares the safe state; on a `hazard.` key, from the
  node that publishes the hazard's guard. The value is a plain number in SI
  units unless `unit <u>` says otherwise (`param Pressure unit mmHg`);
  `magnitude` takes its absolute value (a deceleration written negative).
  Reading the parameter is what makes the derived number the one the image
  runs;
- `table <Dim> -> <Dim>`: a step function the contract writes as a map
  (`a_ebd: { 0 km/h: 0.75 m/s^2, 100 km/h: 0.70 m/s^2 }`), each entry
  holding from its key up to the next.

A key may have a default (`= 0 s`), so a contract may leave it out, and a
string documenting it, which `extension-reference.md` prints.

**Inputs** are core values the extension reads: `settle.response` (the
response written on the safe state's settle) and `reaction_total` (the
hazard's detection + route + settle, from the core walk; a check that reads
it runs after the walk).

**Items.** What the file produces:

- `derive <target> : <Dim> = <expr>` fills a core bound. Three targets
  exist: `settle.duration` (per hazard, since it may read the hazard's
  keys), `hazard.detection` (a physical detection time, charged before the
  detectors') and `hazard.ftti` (the interval itself, from the process).
  A contract may not also write a bound an extension derives
  (`derive-conflict`); a measured settle beside a derived one is compared
  (`settle-conflict`) and the derived one charged.
- `check <name>: <predicate>` is a verdict. It is an error when the
  hazard's `deadline_by: <extension>/<check>` names it (the check is then
  the hazard's requirement, and a hazard may state no `ftti`), a warning
  otherwise.
- `report <name> : <Dim> = <expr>` is a number for the reviewer, with no
  verdict.

**Expressions** have units checked when the file loads, in every branch:
`+ - * / ^`, `if <a> then <b> else <c>`, `min`, `max`, `sqrt`, `where`
bindings, `fn` definitions (`fn braking_time(v0: Velocity, ...) -> Time =
...`), `dimension Flow = Volume / Time` for a dimension of your own, and a
bounded `sum over b in <table>.bands(lo, hi): ...` over a table's bands
(`b.lo`, `b.hi`, `b.value`). A number written beside a name is a product
(`2 v0`); a name on the unit list after a number is a unit (`30 km/h`).
There are no loops, recursion, IO, clock or randomness: an extension always
terminates and cannot see anything but its inputs. An expression names a
key by its last segment (`belt_speed`).

## Worst cases

The checker evaluates a derive at the contract's declared values and calls
the result the worst case. That is sound only if the formula moves the
right way in each input, so an item states it: `worst_case: increasing in
belt_speed, response; decreasing in decel`. Each claim is **proved** by Z3
at the contract's point over the item's `domain:` (proved: a line in the
trace; refuted: `worst-case-refuted`, an error, with a counterexample the
evaluator reproduced and, where one exists, the domain that would make it
hold; undecided: `worst-case-unknown`, a warning, an error under `check
--strict`). The prover assumes no sign the file does not state: a speed
that might be negative turns most monotonicity claims false, so state the
signs in `domain:`. Run the prover on the file alone while writing it:

```
rlx check belt.rlx
rlx prove belt.rlx
rlx fmt --ascii --check belt.rlx
```

## Shipping it

Install the file to `share/<pkg>/rlm/` and register the package under the
ament resource `rlm_extensions`, so the checker searches it (a package that
does not register is never searched; the checker does not walk every
package's `share/`):

```cmake
install(DIRECTORY rlm DESTINATION share/${PROJECT_NAME})
ament_index_register_resource(rlm_extensions)
```

A contract may also keep an extension in an `rlm/` directory beside it,
which is searched first. For each `use:` entry the highest version the
requirement accepts wins (`^0.1`, `~0.2`, `=1.0.0`, `>=1.0.0`), a file on
disk winning a tie with one bundled in the checker.

**Versions.** A change to a derive's formula or to a key's dimension is a
major version; a widened domain or a new key is a minor one. The identity
the checker prints and locks is the sha256 of the file's canonical ASCII
form, so reformatting does not change it and any change to the arithmetic
does.

**Locks.** `play_launch check --update-lock` writes `rlm.lock` beside each
contract (name to version, sha256 and kind); with a lock present the locked
version is preferred while it is installed and accepted, and drift is a
warning (`lock-drift`), an error under `check --locked`, which is what CI
should run.

**Keys of two extensions.** A contract writes a key bare while exactly one
used extension declares it in that context; when two do, it writes the
qualified spelling `<extension>/<key>:`, and the bare one is refused
(`extension-key-ambiguous`).

## What leaves the checker

nano-ros and any other consumer read the resolved model, not the
extension: every derived bound is a plain number there (a safe state's
`settle_ms`, a hazard's `ftti_ms` and `detection_ms`) with a
`DerivedSource` beside it naming the extension, its version, its sha256 and
the inputs it read. The image build never evaluates rlx.
