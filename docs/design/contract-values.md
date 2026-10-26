# Contract values read from the launch tree

Status: design, accepted 2026-10-27; wave 1 implemented in rlm v0.2.4.
Implementation: play_launch phase 94
(`docs/roadmap/phase-94-contract-values.md` in play_launch), which is
play_launch phase 85's D9.

## The rule

A number in a contract that is the system's CONFIGURATION is read from the
launch tree, never retyped. A number that is a REQUIREMENT is written in the
contract, and the parameter that enforces it is checked against it.

Both already half exist. Three keys bind a parameter to CHECK it
(`trigger.timer.param`/`period_param`, `max_age: { duration, param }`, a
mode's `window: { duration, param }`), and an rlx extension key typed
`param <Dimension>` READS a parameter by bare name (`decel:
target_acceleration`). Nothing else can name a parameter, so every other
configured number (a timer rate, a lease, a rate shared by four nodes
through one launch argument) is typed twice, once in the launch tree and
once in the contract, and nothing notices when the two drift. On
`rt_av_demo`, `play_launch check bringup.launch.xml lidar_hz:=100` reads a
contract still saying 50 Hz.

## Three references

| Reference | Reads |
|---|---|
| `$(param [<node>] <name>)` | the node's EFFECTIVE parameter value: globals, then parameter files, then inline values, ROS precedence, file sections matched as a spawn matches them |
| `$(var <name> [@<node>])` | a launch configuration: an `<arg>`, a `<let>`, an include argument |
| `$(let <name>)` | an entry of the contract's own root `let:` |

The spelling is the launch file's own, so an author reads a contract the way
they read the launch file next to it.

### Addressing a node

`$(param …)` with one argument names a parameter of the node the key sits
under (`nodes.<n>.…`). Under any other key (`hazards:`, `modes:`, `topics:`,
`let:`) one argument is an error that names the nodes having a parameter of
that name.

With two arguments the first is a node:

- a bare name is a NODE KEY of this contract, resolved exactly as every other
  key of the contract (the scope's namespace, the `-N` ordinal of a node the
  launch file does not name);
- a name starting with `/` is a fully qualified node name anywhere in the
  launch tree.

A parameter name is dotted as in ROS (`qos.deadline`, a nested section of a
parameter file).

### Which value a `$(var)` reads

A launch configuration has no single value: `<let>` runs in order and may
reassign a name, an include does not scope configurations (a child's `<let>`
reaches its parent), and a scoped `<group>` pops them. So:

- `$(var x)` is the value in force when the contract's launch file finished.
  It is accepted only if every assignment of `x` made during that file gave
  the same value; otherwise `error[var-ambiguous]` lists each assignment with
  its file and line.
- `$(var x @<node>)` is the value in force where `<node>` was declared: the
  value that node's own `$(var x)` read.

Where both exist, `$(param …)` is usually the better reference: it reads what
the node got, after every override. `$(var …)` is for a knob of the launch
file itself.

### Contract-local names

`let:` is a root mapping of names to values, evaluated in order. An entry may
use earlier entries; a forward reference or a cycle is an error. `$(let …)`
and `$(var …)` are separate namespaces, so a contract name never shadows a
launch configuration and a reader always knows which side a value came from.
`let:` also replaces YAML anchors for a number stated once and used in
several places.

## Values are expressions with units

```
value  := term (('+' | '-') term)*
term   := factor (('*' | '/') factor)*
factor := number unit? | ref unit | '$(let' name ')' | '(' value ')'
ref    := '$(param' … ')' | '$(var' … ')'
```

- A `$(param)` or `$(var)` takes a UNIT, always. A parameter carries none,
  and the obvious default is wrong in practice: `rt_av_demo` passes
  `watchdog_ms: 100`, so reading parameters in SI would make a 100 ms lease a
  100 s one, silently. Autoware mixes `timeout: 0.5` (seconds),
  `update_rate: 10` (Hz) and `*_ms` names.
- A `$(let)` carries the unit its entry was given.
- Dimensions are checked with rlx's evaluator and unit table (`Hz`, `ms`,
  `m/s^2`, …): `2 / $(param update_rate) Hz` is a Time. A value of the wrong
  dimension for its key is `error[param-dimension]`, with a suggestion where
  one inverts to the other (`1 / … Hz` for a Time).
- An expression is accepted wherever the grammar takes a quantity: a
  duration, a rate, a speed, an acceleration, a jerk, a count, and the
  `duration:` of a braces form. An rlx extension key typed `param` accepts
  `$(param <name>) <unit>`; its bare form (`decel: target_acceleration`)
  stays legal, and `migrate` rewrites it.

A value with no reference is the literal it always was: `500ms` and
`{ duration: 500ms, source: requirement }` parse exactly as before.

## Two forms, two meanings

```yaml
# REQUIREMENT: the contract is the source, the parameter must agree.
window: { duration: 10s, param: mrm_handler.takeover_request_timeout, source: requirement }

# CONFIGURATION: the parameter is the source, the contract reads it.
trigger: { timer: { rate_hz: $(param update_rate) Hz } }
```

The check form is unchanged, with its rules (`window-param`, `timer-param`,
`max-age-param`). The read form's provenance is automatic,
`source: { param: … }`; writing `source:` beside a reference is
`error[source-on-reference]`. Choosing the form is the author's statement of
which numbers somebody chose: a takeover window from a hazard analysis is a
requirement, and reading it from the parameter would let a parameter edit
change the requirement unseen.

## Resolution

References are resolved by the consumer that has the launch tree, never by
the contract parser. The parser keeps each reference as an expression; the
resolver evaluates it per scope instance against the launch tree's values
and hands every rule a plain number, so no rule, model field or downstream
reader sees an expression.

| Situation | Outcome |
|---|---|
| a launch source sets the parameter | its value; provenance is that source and its line |
| no launch source sets it, `params: { x: { default: … } }` declares one | the default; provenance is the contract line |
| no launch source sets it, no default | `error[param-unset]`: the node runs its compiled-in default, which no contract can see |
| the node is not in the launch tree | `error[param-node-unknown]`, with near-miss names |
| one argument under a key outside a node | `error[param-node-missing]`, naming the nodes having that parameter |
| the value is a string, list or boolean | `error[param-not-number]` |
| the dimension disagrees with the key | `error[param-dimension]` |
| a `$(var)` assigned two values in the file | `error[var-ambiguous]`; `@<node>` resolves it |
| no launch tree at all (a contract checked alone) | the value is UNKNOWN, and the rules that need it report incomplete evidence, never zero |

`params:` gains `default:` for the fallback.

### Instances

A contract bound to two include scopes (`camera.launch.xml` with `cam:=left`
and `cam:=right`) is evaluated once per scope, and a diagnostic naming a
resolved value carries the instance: `camera.contract.yaml:40
[/sensing/left]: …`.

### The model

The model carries resolved numbers only, so a reader of the model is
unchanged. Each number read through a reference carries its provenance:

```yaml
rate_hz: 10.0
rate_hz_source: { param: { node: /mrm_handler, name: update_rate, at: "mrm_handler.param.yaml" } }
```

### `--explain`

```
/mrm_handler on_timer  rate_hz = 10 Hz  <- $(param update_rate) Hz
                                        <- mrm_handler.param.yaml (/**: update_rate: 10)
/safety/lidar_driver   rate_hz = 100 Hz <- $(param rate_hz) Hz
                                        <- <param> bringup.launch.xml:29 = $(var lidar_hz)
                                        <- command line lidar_hz:=100
```

## Worked examples

### The safety island

Before:

```yaml
hazards:
  hpc_loss:
    entry_speed: &odd_speed_bound 8.33 m/s
nodes:
  mrm_emergency_stop_operator:
    paths:
      on_timer:
        trigger:
          timer: { rate_hz: 30, param: update_rate }
        safe_state:
          settle: { decel: target_acceleration, jerk: target_jerk }
```

After:

```yaml
let:
  odd_speed: 8.33 m/s                                  # the ODD bound, stated once
hazards:
  hpc_loss:
    entry_speed: $(let odd_speed)
modes:
  takeover_request:                                    # a requirement: unchanged
    window: { duration: 10s, param: mrm_handler.takeover_request_timeout, source: requirement }
nodes:
  mrm_emergency_stop_operator:
    paths:
      on_timer:
        trigger:
          timer: { rate_hz: $(param update_rate) Hz }  # configuration: read
        safe_state:
          settle:
            decel: $(param target_acceleration) m/s^2
            jerk: $(param target_jerk) m/s^3
  mrm_handler:
    sub:
      availability:                                    # a requirement: unchanged
        max_age: { duration: 500ms, param: timeout_operation_mode_availability, source: requirement }
```

### `rt_av_demo`

```yaml
let:
  lidar_rate: $(var lidar_hz) Hz
nodes:
  lidar_driver:
    paths:
      scan: { trigger: { timer: { rate_hz: $(param rate_hz) Hz } }, output: [points] }
  obstacle_detector:
    sub:
      points:
        min_rate_hz: $(let lidar_rate)
        qos: { liveliness: manual_by_topic, lease_duration: $(param watchdog_ms) ms }
        max_age: 3 / $(let lidar_rate)
```

### The errors, as an author meets them

```
error[param-dimension]: safety_island.contract.yaml:837: `max_age` is a Time, and
  `$(param update_rate) Hz` is a Frequency (10 Hz, mrm_handler.param.yaml).
  Did you mean `1 / $(param update_rate) Hz`?

error[param-node-missing]: safety_island.contract.yaml:472: `hazards.takeover.ftti`
  is under no node, so `$(param timeout)` must name one: `$(param <node> timeout)`.
  Nodes of this contract with a `timeout` parameter: mrm_handler.

error[param-unset]: safety_island.contract.yaml:520: `$(param init_timeout) s`: no
  launch source sets `init_timeout` on /mrm_handler (sources: mrm_handler.param.yaml),
  so it runs the code's default, which a contract cannot see. Set it in the parameter
  file, or declare `params: { init_timeout: { type: double, default: … } }`.

error[var-ambiguous]: bringup.contract.yaml:12: `$(var timeout)` was assigned 0.5
  (bringup.launch.xml:3) and 2.0 (bringup.launch.xml:9). Name the point:
  `$(var timeout @gate)`.
```

## Out of scope

- A structural switch: a rung enabled by a boolean parameter
  (`use_comfortable_stop: false`). A reference is a number.
- Indexing a list parameter.
- `$(env …)`: it differs per host, and a launch argument already carries the
  choice on a vehicle.
- A parameter changed at run time (`ros2 param set`): the contract states the
  launch value; play_launch's Health page reports drift (`param_drift`).

## Rejected

- A `bindings:` table keyed by contract path. Complete, but the binding sits
  away from its number and every bound number costs three lines.
- A braces form `{ value, param, unit }` on every key. Every reader of a
  scalar key would have to change.
- Textual substitution before parsing. Loses provenance, and a contract would
  no longer parse without its launch tree.
- SI as the default unit of a parameter. See `watchdog_ms` above.
- One namespace for `let:` and launch configurations. Whether `$(var x)` read
  the contract or the launch would depend on declarations elsewhere.
