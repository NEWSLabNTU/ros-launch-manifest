# Domain extensions by example

Companion to `extension-language-exploration.md`. Shows the design on
seven domains, each as the extension file (rlx), the contract that uses it,
and what `play_launch check --explain` prints. Written before the
implementation (2026-10-15); built in play_launch phase 91 (rlm v0.2.0).
The extension files are published as the bundled `vehicle.braking`
(`types/extensions/`) and the five reference extensions
(`types/extensions/reference/`), and rlx's `tests/reference.rs` holds each
to its block here. The printed output is the proposal's shape; the
checker's actual output for each, on play_launch's fixtures, is recorded
beside them (`tests/fixtures/contract_ext_<domain>/expected_explain.txt`). Numbers are worked
in the exploration's appendix A unless marked otherwise; domain constants are
illustrative, not values taken from a standard.

## The design these examples follow

- **Core keeps seven primitives**: entity, path, bound, predicate, mode,
  hazard, safe state. A bound is a duration, rate or count with a `source`
  and an optional `param`; the readable keys (`max_latency`, `ftti`,
  `window`, `settle`, `response`, ...) are its spellings. A hazard may carry
  a `detection:` bound (new) besides its `ftti`.
- **An extension adds keys and fills core bounds or adds checks.** It may
  `derive` a core bound (`settle.duration`, `hazard.detection`,
  `hazard.ftti`) or declare a `check` (a predicate). It never adds a checker
  rule.
- **Extensions are rlx files**: a small expression language with units of
  measure, `fn`, `where`, `if/then/else`, `min`, `max`, `sqrt` and a bounded
  `sum over` a table. Always terminates; no IO, clock or randomness; units
  checked when the file loads, in every branch.
- **Core values an extension may read**: `reaction_total` (the hazard's
  detection + route + settle, from core). Evaluation order: core timing
  first, extension checks after.
- **Worst case is declared and proved** (`worst_case:` over a `domain:`),
  by Z3. An unproved claim is a warning (an error under `--strict`); a
  refuted one is an error with the counterexample.
- **Profiles are data** (YAML): a severity scale and its criticality map.
- **Keys are bare** in contracts while one used extension declares them;
  two extensions declaring the same key need the qualified spelling
  `vehicle.braking/decel:`.
- **Distribution**: `share/<pkg>/rlm/` in an ament package; a contract
  names it with `use:`; `--explain` prints its version and sha256.

---

## 1. Road vehicle: braking settle with a response

The island's emergency stop, as it would read after the move.

```
-- share/rlm_ext_vehicle/rlm/braking.rlx
extension vehicle.braking 1.0.0
requires rlm >= 0.2.0

key hazard.entry_speed : Velocity              "speed bound of the operating domain"
key settle.decel       : param Acceleration magnitude  "node parameter: the deceleration held"
key settle.jerk        : param Jerk magnitude          "node parameter: the jerk it ramps at"
input settle.response  : Time = 0 s            -- core bound

derive settle.duration : Time =
    response + braking_time(entry_speed, decel, jerk)
  worst_case:
    increasing in entry_speed, response
    decreasing in decel, jerk
  domain:
    entry_speed in (0 m/s, 70 m/s]

fn braking_time(v0: Velocity, a: Acceleration, j: Jerk) -> Time =
    if v0 > v_r then a / j + (v0 - v_r) / a
    else sqrt(2 v0 / j)
  where v_r = a^2 / (2 j)                      -- speed shed during the jerk ramp
```

```yaml
use: { vehicle.braking: "^1", iso26262: "^1" }
hazards:
  hpc_loss:
    severity: ASIL_D
    guards: [/system/operation_mode/availability]
    on: omission
    entry_speed: 30 km/h
    ftti: 10s
    reaction: l3_engaged
nodes:
  mrm_emergency_stop_operator:
    paths:
      on_timer:
        safe_state:
          emits: emergency_control_cmd
          settle:
            decel: target_acceleration
            jerk: target_jerk
            response: { duration: 256ms, source: { measured: { max: 213.11ms, n: 3, margin: 0.2 } } }
```

```
info[settle-derived]: hazard 'hpc_loss' -> emergency_stop
  settle = 4422.67 ms   derived by vehicle.braking 1.0.0 (sha256 ...)
    entry_speed v0 = 8.3333 m/s 30 km/h, hazard hpc_loss
    decel       a  = 2.5 m/s^2  |target_acceleration| = |-2.5|
    jerk        j  = 1.5 m/s^3  |target_jerk| = |-1.5|
    response       = 256 ms     measured: 213.11 x 1.2 -> 256 (n=3)
    v_r = a^2/(2j) = 2.0833 m/s;  v0 > v_r
    a/j            = 1666.67 ms
    (v0 - v_r)/a   = 2500.00 ms
    settle         = 256 + 1666.67 + 2500.00 = 4422.67 ms
  worst case: proved increasing in entry_speed, response; decreasing in decel, jerk
```

What it shows: today's keys keep their names; the formula, its inputs and
its worst-case direction are visible to a reviewer. (The island states
`entry_speed: 8.33`, in m/s, and gets 4421.33 ms; exact 30 km/h is the
4422.67 ms above. E0 found the difference, extension-system.md section 13.)

---

## 2. Industrial machinery: a press guarded by a light curtain (ISO 13855)

The requirement here is a distance, not a time: the stop must complete
before a hand reaches the hazard. The hazard has no `ftti`; the extension's
check stands in for it, and the hazard says so.

```
-- share/rlm_ext_machine/rlm/iso13855.rlx
extension machine.iso13855 0.3.0
requires rlm >= 0.2.0

key hazard.detection_capability : Length  "ESPE detection capability d"
key hazard.device_response      : Time    "ESPE response time"
key hazard.installed_distance   : Length  "detection zone to hazard zone"
input reaction_total : Time               -- core: detection + route + settle

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

```yaml
use: { machine.iso13855: "^0.3", iso13849: "^1" }
hazards:
  reach_into_press:
    severity: PL_d
    guards: [/safety/light_curtain/state]
    on: reported
    when: { field: interrupted, equals: true }
    deadline_by: machine.iso13855/safety_distance   # no ftti: the check is the requirement
    reaction: press.stop
    detection_capability: 30 mm
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

```
info[check]: hazard 'reach_into_press': machine.iso13855/safety_distance PASS
    reaction_total = 192 ms      core: route 12 + settle 180
    T = 15 + 192   = 207 ms
    C = 8 (30 - 14) = 128 mm     d = 30 mm <= 40 mm
    2000 mm/s * T + C = 542.0 mm > 500 mm, so K = 1600 mm/s
    1600 mm/s * T + C = 459.2 mm; required = max(459.2, 500) = 500 mm
    required 500 mm <= installed 520 mm   (margin 20 mm)
  worst case: proved increasing in reaction_total, device_response
```

What it shows: a requirement that is a distance; core timing feeding an
extension; the stop category (a cat-1 stop is a measured settle) needs no
extension at all.

---

## 3. Medical device: infusion pump occlusion detection

Detection is the slow part, and its worst case is at the lowest rate, not
the highest. The extension derives the hazard's core `detection:` bound.

```
-- share/rlm_ext_medical/rlm/occlusion.rlx
extension medical.occlusion 0.1.0
requires rlm >= 0.2.0

dimension Compliance = Volume / Pressure
dimension Flow       = Volume / Time

key hazard.min_rate          : Flow              "lowest programmable rate covered"
key hazard.compliance        : Compliance        "syringe + line compliance"
key hazard.baseline_pressure : Pressure = 0 mmHg
key hazard.alarm_threshold   : param Pressure unit mmHg
key hazard.signal_delay      : Time = 0 s        "alarm signal generation delay"

derive hazard.detection : Time =
    (alarm_threshold - baseline_pressure) * compliance / min_rate + signal_delay
  worst_case:
    decreasing in min_rate
    increasing in compliance, alarm_threshold
  domain: min_rate in (0 mL/h, 1200 mL/h], alarm_threshold > baseline_pressure,
          compliance > 0 mL/mmHg

report bolus_on_release : Volume = compliance * (alarm_threshold - baseline_pressure)
```

```yaml
use: { medical.occlusion: "^0.1", iec62304: "^1" }
hazards:
  downstream_occlusion:
    severity: C                     # IEC 62304 software safety class
    guards: [/pump/line_pressure]
    on: reported
    when: { field: occluded, equals: true }
    ftti: 30min                     # illustrative clinical limit
    reaction: alarm_and_stop
    min_rate: 1 mL/h
    compliance: 0.003 mL/mmHg
    baseline_pressure: 20 mmHg
    alarm_threshold: occlusion_limit_mmhg      # node parameter, 300
    signal_delay: 5s
```

```
error[fault-reaction-budget]: hazard 'downstream_occlusion': detection 50.48 min > ftti 30 min
  detection derived by medical.occlusion 0.1.0
    (300 - 20) mmHg * 0.003 mL/mmHg = 0.84 mL
    0.84 mL / 1 mL/h = 50.40 min;  + 5 s = 50.48 min
  worst case: proved decreasing in min_rate (the declared minimum is the worst case)
  what would fit: min_rate >= 1.685 mL/h, or a pressure span <= 166.2 mmHg at 1 mL/h
  report: bolus_on_release = 0.84 mL
```

What it shows: a derived detection bound; a worst case at a minimum; the
checker can say what would fit because the formula is analysable.

---

## 4. UAV: lost link, return by descent, battery reserve

Two outputs: the descent is the settle, and an energy check reads core's
reaction total. The first worst-case claim is wrong, and the checker says
so.

```
-- share/rlm_ext_uav/rlm/descent.rlx
extension uav.descent 0.2.0
requires rlm >= 0.2.0

key hazard.max_altitude     : Length
key hazard.hover_power      : Power
key hazard.battery_capacity : Energy
key hazard.trigger_soc      : param Fraction
key hazard.reserve_soc      : param Fraction
key settle.descent_speed    : param Velocity
key settle.slow_altitude    : param Length
key settle.land_speed       : param Velocity
input reaction_total : Time

derive settle.duration : Time =
    if max_altitude > slow_altitude
    then (max_altitude - slow_altitude) / descent_speed + slow_altitude / land_speed
    else max_altitude / land_speed
  worst_case:
    increasing in max_altitude, slow_altitude
    decreasing in descent_speed, land_speed

check energy_reserve:
    hover_power * reaction_total <= battery_capacity * (trigger_soc - reserve_soc)
  worst_case: increasing in reaction_total
```

```yaml
use: { uav.descent: "^0.2" }
hazards:
  lost_link:
    guards: [/fmu/telemetry/link]
    on: omission
    ftti: 120s
    reaction: land_now
    max_altitude: 120 m
    hover_power: 600 W
    battery_capacity: 222 Wh
    trigger_soc: low_battery_fraction      # 0.30
    reserve_soc: reserve_fraction          # 0.22
nodes:
  failsafe:
    paths:
      land:
        safe_state:
          emits: land_cmd
          settle:
            descent_speed: descent_speed   # 3 m/s
            slow_altitude: slow_altitude   # 10 m
            land_speed: land_speed         # 0.7 m/s
```

First check, with the claim as written (play_launch's
`contract_ext_uav_refuted`, verbatim):

```
error[worst-case-refuted]: uav.descent 0.2.0: settle.duration is not increasing in slow_altitude
  counterexample (Z3): max_altitude = 120 m, descent_speed = 3 m/s, land_speed = 5 m/s
    slow_altitude 1 m -> 39.87 s;  2 m -> 39.73 s
  help: it holds on `domain: land_speed <= descent_speed, land_speed > 0 m/s`
```

Four more claims are refuted the same way, each because a speed, an
altitude or the hover power might be negative: the prover assumes no sign
the file does not state. `max_altitude <= slow_altitude` would also make
the slow_altitude claim hold (the settle no longer depends on it), and is
not offered, because the contract's 120 m and 10 m are outside it.

After the author adds the domains the checker names:

```
  domain: descent_speed > 0 m/s, land_speed > 0 m/s, land_speed <= descent_speed,
          max_altitude > 0 m, slow_altitude > 0 m
...
  domain: hover_power > 0 W
```

```
info[settle-derived]: hazard 'lost_link' -> land_now
  settle = 50.95 s   derived by uav.descent 0.2.0
    (120 - 10) m / 3 m/s = 36.67 s;  10 m / 0.7 m/s = 14.29 s
  worst case: proved increasing in max_altitude, slow_altitude; decreasing in descent_speed, land_speed
info[check]: uav.descent/energy_reserve PASS
    reaction_total = 60.95 s   core: detection 10 s + settle 50.95 s
    600 W * 60.95 s = 10.16 Wh <= 222 Wh * (0.30 - 0.22) = 17.76 Wh
  worst case: proved increasing in reaction_total
```

What it shows: a predicate fed by core; a refuted worst-case claim with a
counterexample and a fix.

---

## 5. Rail: emergency braking against a supervision table

The deceleration depends on speed band, given as a table; the settle is a
bounded sum over it, and a distance check stands beside the FTTI.

```
-- share/rlm_ext_rail/rlm/ebd.rlx
extension rail.ebd 0.1.0
requires rlm >= 0.2.0

key hazard.entry_speed      : Velocity
key hazard.distance_to_eoa  : Length                  "to the end of movement authority"
key settle.traction_cut_off : Time
key settle.brake_build_up   : Time
key settle.a_ebd            : table Velocity -> Acceleration
    "emergency deceleration; each entry holds from its speed up to the next"

derive settle.duration : Time =
    traction_cut_off + brake_build_up
    + sum over b in a_ebd.bands(0 m/s, entry_speed): (b.hi - b.lo) / b.value
  worst_case: increasing in entry_speed; decreasing in a_ebd
  domain: a_ebd in (0 m/s^2, 5 m/s^2]

check stop_before_eoa: stopping_distance <= distance_to_eoa
  where stopping_distance =
      entry_speed * (traction_cut_off + brake_build_up)
      + sum over b in a_ebd.bands(0 m/s, entry_speed): (b.hi^2 - b.lo^2) / (2 b.value)
  worst_case: increasing in entry_speed
  domain: a_ebd in (0 m/s^2, 5 m/s^2], traction_cut_off >= 0 s, brake_build_up >= 0 s
```

```yaml
use: { rail.ebd: "^0.1", en50129: "^1" }
hazards:
  overspeed_at_eoa:
    severity: SIL_4
    guards: [/evc/supervision]
    on: reported
    when: { field: intervention, equals: true }
    ftti: 90s
    reaction: emergency_brake
    entry_speed: 160 km/h
    distance_to_eoa: 1600 m
nodes:
  evc:
    paths:
      supervise:
        safe_state:
          emits: brake_cmd
          settle:
            traction_cut_off: 1.0s
            brake_build_up: 2.5s
            a_ebd: { 0 km/h: 0.75 m/s^2, 100 km/h: 0.70 m/s^2 }
```

```
info[settle-derived]: settle = 64.35 s   derived by rail.ebd 0.1.0
    3.5 s + (27.78 m/s)/0.75 + (16.67 m/s)/0.70 = 3.5 + 37.04 + 23.81 s
  worst case: proved increasing in entry_speed; decreasing in a_ebd (for a_ebd's 2 keys as written)
info[check]: rail.ebd/stop_before_eoa PASS
    155.56 + 514.40 + 859.79 = 1529.75 m <= 1600 m   (margin 70.25 m)
  worst case: proved increasing in entry_speed (for a_ebd's 2 keys as written)
```

What it shows: a table input with a bounded sum, and two requirements on
one hazard (a time and a distance).

---

## 6. Process industry: a safety instrumented function against process safety time

The deadline is not given; it comes from the process. The extension derives
the hazard's `ftti` (the process safety time) from how fast the process
moves, and core checks the SIF against it. (Illustrative constants.)

```
-- share/rlm_ext_process/rlm/pst.rlx
extension process.pst 0.1.0
requires rlm >= 0.2.0

dimension PressureRate = Pressure / Time

key hazard.trip_setpoint   : param Pressure unit bar
key hazard.hazard_pressure : Pressure          "pressure at which harm begins"
key hazard.max_rise_rate   : PressureRate      "worst credible rise, from the HAZOP"
key settle.valve_stroke    : Time              "final element: closed from command"

derive hazard.ftti : Time = (hazard_pressure - trip_setpoint) / max_rise_rate
  worst_case: decreasing in max_rise_rate; increasing in hazard_pressure
  domain: hazard_pressure > trip_setpoint, max_rise_rate > 0 bar/s

derive settle.duration : Time = valve_stroke
```

```yaml
use: { process.pst: "^0.1", iec61511: "^1" }
hazards:
  vessel_overpressure:
    severity: SIL_2
    guards: [/plant/pt101]
    on: reported
    when: { field: above_trip, equals: true }
    reaction: sif_close_inlet
    trip_setpoint: pt101_trip_bar          # 9 bar
    hazard_pressure: 11 bar
    max_rise_rate: 0.5 bar/s
nodes:
  sif_logic:
    paths:
      on_pt101:
        max_latency: 300ms
        safe_state: { emits: inlet_valve_cmd, settle: { valve_stroke: 2s } }
```

```
info[fault-reaction-budget]: hazard 'vessel_overpressure': total 2.40 s fits ftti 4.00 s (slack 1.60 s)
  ftti derived by process.pst 0.1.0: (11 - 9) bar / 0.5 bar/s = 4.00 s
  detection 100 ms (pt101 at 10 Hz) + route 300 ms + settle 2.00 s = 2.40 s
```

What it shows: an extension may derive the deadline itself; core's
arithmetic is unchanged.

---

## 7. Aerospace: a profile, no physical model

Many aerospace timing requirements are latencies and monitor reactions that
core already states. The domain part is the assurance scale and its
mapping, which is data.

```yaml
# share/rlm_profile_do178c/rlm/do178c.profile.yaml
profile: do178c
version: 1.0.0
requires: { rlm: ">=0.2.0" }
severity_scale: [DAL_E, DAL_D, DAL_C, DAL_B, DAL_A]   # ascending
criticality:
  DAL_E: null
  DAL_D: low
  DAL_C: medium
  DAL_B: high
  DAL_A: high
```

```yaml
use: { do178c: "^1" }
hazards:
  pitch_command_frozen:
    severity: DAL_A
    guards: [/fcc/pitch_cmd]
    on: [omission, late]
    ftti: 200ms
    reaction: fcc.switch_to_backup
nodes:
  fcc_monitor:
    sub:
      pitch_cmd: { max_age: 50ms, on_violation: { reaction: switch_lane, within: 20ms } }
```

```
info[fault-reaction-budget]: hazard 'pitch_command_frozen': detection 50 ms + route 20 ms
  + settle (backup lane, declared on its path; illustrative) 40 ms = 110 ms fits ftti 200 ms
info[criticality]: fcc_monitor -> high (DAL_A via do178c 1.0.0)
```

What it shows: a domain that needs only a profile; core's primitives carry
the timing unchanged.

---

## What the seven say about the core

- Every domain's timing reduced to core bounds (detection, route, settle,
  ftti) plus, in three of seven, a check (a distance or an energy).
- Extensions derived four different core bounds: `settle.duration`
  (vehicle, UAV, rail, process), `hazard.detection` (medical) and
  `hazard.ftti` (process). That is the whole interface besides checks and
  reports.
- Only one core input was needed: `reaction_total` (machinery, UAV).
- One hazard needed no `ftti` (machinery), declared with `deadline_by:`.
- Profiles were data in every case; only ISO 26262 decomposition (not shown
  here; see the exploration, section 2.5) needs a function.
