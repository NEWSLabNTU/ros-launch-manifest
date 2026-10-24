# Extensions -- Reference

**Generated from `types/extensions/`. Do not edit by hand** -- run `UPDATE_EXTENSION_REFERENCE=1 cargo test -p ros-launch-manifest-rlx --test extension_reference` after changing an extension.

The core grammar is [format-reference.md](format-reference.md); a key not listed there is a parse error unless the contract `use:`s an extension that declares it in that context (`hazards.<name>` or `safe_state.settle`). This page lists the extensions this repository publishes. How to write one: [extension-guide.md](extension-guide.md).

- **Bundled** extensions ship inside the checker and resolve for any `use:` entry with no file on disk.
- **Reference** extensions are the worked examples of [design/extension-examples.md](design/extension-examples.md) as files. They are not bundled: a contract that wants one ships a copy in its package's `rlm/` directory. They are illustrative: [design/reference-extensions-verification.md](design/reference-extensions-verification.md) compares each with the standard it models and lists what each leaves out.

## `vehicle.braking 1.0.0`

**Bundled.** The keys were core grammar from rlm v0.1.46 to v0.1.71. play_launch 0.19 loaded this extension implicitly, with a warning, for a contract that writes them without `use:`; later releases refuse such a contract (`deprecated-implicit-extension`).

Requires: `rlm >= 0.2.0`.

Keys a contract writes:

| key | type | default | meaning |
|---|---|---|---|
| `hazard.entry_speed` | Velocity | no | speed bound of the operating domain |
| `settle.decel` | param Acceleration magnitude | no | node parameter: the deceleration held |
| `settle.jerk` | param Jerk magnitude | no | node parameter: the jerk it ramps at |

Core values it reads:

| input | dimension | default |
|---|---|---|
| `settle.response` | Time | yes |

What it produces:

| item | name | result | worst case |
|---|---|---|---|
| derive | `settle.duration` | Time | increasing in `entry_speed`, `response`; decreasing in `decel`, `jerk` |

sha256 of the canonical form: `349731db33beff03738e0b25a67301edb5910930a67bbfd62c4ec974e1db1b8f`. Source (`types/extensions/vehicle.braking.rlx`):

```
-- vehicle.braking: the settle of a vehicle that ramps its deceleration up
-- at a jerk and holds it until it stops. Bundled with the checker (rlm
-- v0.2.0, play_launch phase 91 E8); the keys were core grammar from rlm
-- v0.1.46 to v0.1.71 (`hazards.<h>.entry_speed`, `settle: { decel, jerk }`).
-- A contract writes the node's own parameters (`decel: target_acceleration`),
-- so the settle charged is the one the image runs.
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

## `machine.iso13855 0.3.0`

**Reference, illustrative.**

Requires: `rlm >= 0.2.0`.

Keys a contract writes:

| key | type | default | meaning |
|---|---|---|---|
| `hazard.detection_capability` | Length | no | ESPE detection capability d |
| `hazard.device_response` | Time | no | ESPE response time |
| `hazard.installed_distance` | Length | no | detection zone to hazard zone |

Core values it reads:

| input | dimension | default |
|---|---|---|
| `reaction_total` | Time | no |

What it produces:

| item | name | result | worst case |
|---|---|---|---|
| check | `safety_distance` | verdict | increasing in `reaction_total`, `device_response` |

sha256 of the canonical form: `594b23c059caff947adf55d3a7864c1786d73274181d7a8fba74b1246d3ea03e`. Source (`types/extensions/reference/iso13855.rlx`):

```
-- Reference extension (machinery): ISO 13855 (safety distance of an electro-sensitive protective device).
-- Phase 91 E9, rlm extension-examples.md. ILLUSTRATIVE: the formulas follow
-- the published examples, but no clause of the standard named here has been
-- checked against the standard's text, and every constant a contract gives
-- it in the examples is an example value. Verify both before calling this a
-- reference implementation of the standard.
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

## `medical.occlusion 0.1.0`

**Reference, illustrative.**

Requires: `rlm >= 0.2.0`.

Keys a contract writes:

| key | type | default | meaning |
|---|---|---|---|
| `hazard.min_rate` | Flow | no | lowest programmable rate covered |
| `hazard.compliance` | Compliance | no | syringe + line compliance |
| `hazard.baseline_pressure` | Pressure | yes |  |
| `hazard.alarm_threshold` | param Pressure unit mmHg | no |  |
| `hazard.signal_delay` | Time | yes | alarm signal generation delay |

What it produces:

| item | name | result | worst case |
|---|---|---|---|
| derive | `hazard.detection` | Time | decreasing in `min_rate`; increasing in `compliance`, `alarm_threshold` |
| report | `bolus_on_release` | Volume | no verdict |

sha256 of the canonical form: `4e822b48647c6773f043b0e90323d808e00c8a6ce127871d6927f0f1521aef0f`. Source (`types/extensions/reference/occlusion.rlx`):

```
-- Reference extension (medical devices): an infusion pump's downstream occlusion detection (IEC 60601-2-24 style).
-- Phase 91 E9, rlm extension-examples.md. ILLUSTRATIVE: the formulas follow
-- the published examples, but no clause of the standard named here has been
-- checked against the standard's text, and every constant a contract gives
-- it in the examples is an example value. Verify both before calling this a
-- reference implementation of the standard.
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

## `uav.descent 0.2.0`

**Reference, illustrative.**

Requires: `rlm >= 0.2.0`.

Keys a contract writes:

| key | type | default | meaning |
|---|---|---|---|
| `hazard.max_altitude` | Length | no |  |
| `hazard.hover_power` | Power | no |  |
| `hazard.battery_capacity` | Energy | no |  |
| `hazard.trigger_soc` | param Fraction | no |  |
| `hazard.reserve_soc` | param Fraction | no |  |
| `settle.descent_speed` | param Velocity | no |  |
| `settle.slow_altitude` | param Length | no |  |
| `settle.land_speed` | param Velocity | no |  |

Core values it reads:

| input | dimension | default |
|---|---|---|
| `reaction_total` | Time | no |

What it produces:

| item | name | result | worst case |
|---|---|---|---|
| derive | `settle.duration` | Time | increasing in `max_altitude`, `slow_altitude`; decreasing in `descent_speed`, `land_speed` |
| check | `energy_reserve` | verdict | increasing in `reaction_total` |

sha256 of the canonical form: `574db07d94e7907d8234ac98da0bf68a62a2f1527c7294f9d395539d736e92a6`. Source (`types/extensions/reference/descent.rlx`):

```
-- Reference extension (UAV): a lost-link descent and its battery reserve.
-- Phase 91 E9, rlm extension-examples.md. ILLUSTRATIVE: the formulas follow
-- the published examples, but no clause of the standard named here has been
-- checked against the standard's text, and every constant a contract gives
-- it in the examples is an example value. Verify both before calling this a
-- reference implementation of the standard.
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
  domain: descent_speed > 0 m/s, land_speed > 0 m/s, land_speed <= descent_speed,
          max_altitude > 0 m, slow_altitude > 0 m

check energy_reserve:
    hover_power * reaction_total <= battery_capacity * (trigger_soc - reserve_soc)
  worst_case: increasing in reaction_total
  domain: hover_power > 0 W
```

## `rail.ebd 1.0.0`

**Reference, illustrative.**

Requires: `rlm >= 0.2.0`.

Keys a contract writes:

| key | type | default | meaning |
|---|---|---|---|
| `hazard.entry_speed` | Velocity | no | the highest speed supervised at intervention |
| `hazard.distance_to_svl` | Length | no | to the supervised location the emergency brake must stop short of |
| `settle.speed_error` | Velocity | no | worst speed under-reading at entry_speed (V_ura) |
| `settle.acceleration` | Acceleration | no | worst acceleration held from intervention until full braking (A_est) |
| `settle.traction_cut_off` | Time | no | intervention to zero traction (T_traction) |
| `settle.brake_build_up` | Time | no | equivalent emergency build-up time (T_be: react + half of increase) |
| `settle.a_safe` | table Velocity -> Acceleration | no | safe deceleration A_safe = A_brake_safe (Kdry, Kwet applied) + the worst gradient over the braking distance; each entry holds from its speed up to the next |

What it produces:

| item | name | result | worst case |
|---|---|---|---|
| derive | `settle.duration` | Time | increasing in `entry_speed`, `speed_error`, `acceleration`, `traction_cut_off`, `brake_build_up`; decreasing in `a_safe` |
| check | `stop_before_svl` | verdict | increasing in `entry_speed`, `speed_error`, `acceleration`, `traction_cut_off`, `brake_build_up`; decreasing in `a_safe` |

sha256 of the canonical form: `9b6efcac3c094cfe50f40f09c704a29ecc025d0cb67fd8cbbb2d2cffa4110478`. Source (`types/extensions/reference/ebd.rlx`):

```
-- Reference extension (rail): emergency brake deceleration supervision, after ERA SUBSET-026-3 section 3.13.
-- Phase 91 E9, rlm extension-examples.md. ILLUSTRATIVE: the formulas follow
-- the published examples, and docs/design/reference-extensions-verification.md
-- compares them with SUBSET-026 v4.0.0 section 3.13; every constant a contract
-- gives it in the examples is an example value. 1.0.0 overlaps the two dead
-- times, lets the train accelerate through them from its under-read speed,
-- takes the table as A_safe (gradient included) and stops short of the
-- supervised location. The settle's worst cases are proved; the distance
-- check's are past what Z3 decides (the braking distance squares a speed
-- that grows with acceleration times the dead time) and come back unknown,
-- never refuted.
extension rail.ebd 1.0.0
requires rlm >= 0.2.0

key hazard.entry_speed      : Velocity      "the highest speed supervised at intervention"
key hazard.distance_to_svl  : Length        "to the supervised location the emergency brake must stop short of"
key settle.speed_error      : Velocity      "worst speed under-reading at entry_speed (V_ura)"
key settle.acceleration     : Acceleration  "worst acceleration held from intervention until full braking (A_est)"
key settle.traction_cut_off : Time          "intervention to zero traction (T_traction)"
key settle.brake_build_up   : Time          "equivalent emergency build-up time (T_be: react + half of increase)"
key settle.a_safe           : table Velocity -> Acceleration
    "safe deceleration A_safe = A_brake_safe (Kdry, Kwet applied) + the worst gradient over the braking distance; each entry holds from its speed up to the next"

derive settle.duration : Time =
    dead + sum over b in a_safe.bands(0 m/s, v_bec): (b.hi - b.lo) / b.value
  where dead  = max(traction_cut_off, brake_build_up)  -- both run from the one command
    and v_bec = entry_speed + speed_error + acceleration * dead
  worst_case:
    increasing in entry_speed, speed_error, acceleration, traction_cut_off, brake_build_up
    decreasing in a_safe
  domain: a_safe in (0 m/s^2, 5 m/s^2], entry_speed in (0 m/s, 100 m/s],
          speed_error in [0 m/s, 10 m/s], acceleration in [0 m/s^2, 2 m/s^2],
          traction_cut_off in [0 s, 30 s], brake_build_up in [0 s, 30 s]

check stop_before_svl: stopping_distance <= distance_to_svl
  where dead  = max(traction_cut_off, brake_build_up)
    and v0    = entry_speed + speed_error
    and v_bec = v0 + acceleration * dead
    and stopping_distance =
      v0 * dead + acceleration * dead^2 / 2
      + sum over b in a_safe.bands(0 m/s, v_bec): (b.hi^2 - b.lo^2) / (2 b.value)
  worst_case:
    increasing in entry_speed, speed_error, acceleration, traction_cut_off, brake_build_up
    decreasing in a_safe
  domain: a_safe in (0 m/s^2, 5 m/s^2], entry_speed in (0 m/s, 100 m/s],
          speed_error in [0 m/s, 10 m/s], acceleration in [0 m/s^2, 2 m/s^2],
          traction_cut_off in [0 s, 30 s], brake_build_up in [0 s, 30 s]
```

## `process.pst 0.1.0`

**Reference, illustrative.**

Requires: `rlm >= 0.2.0`.

Keys a contract writes:

| key | type | default | meaning |
|---|---|---|---|
| `hazard.trip_setpoint` | param Pressure unit bar | no |  |
| `hazard.hazard_pressure` | Pressure | no | pressure at which harm begins |
| `hazard.max_rise_rate` | PressureRate | no | worst credible rise, from the HAZOP |
| `settle.valve_stroke` | Time | no | final element: closed from command |

What it produces:

| item | name | result | worst case |
|---|---|---|---|
| derive | `hazard.ftti` | Time | decreasing in `max_rise_rate`; increasing in `hazard_pressure` |
| derive | `settle.duration` | Time | none stated |

sha256 of the canonical form: `7c2e84dde81df3f00695d897fd9b93868aa37547f282a19329b75667841a3987`. Source (`types/extensions/reference/pst.rlx`):

```
-- Reference extension (process industry): process safety time against a safety instrumented function (IEC 61511 style).
-- Phase 91 E9, rlm extension-examples.md. ILLUSTRATIVE: the formulas follow
-- the published examples, but no clause of the standard named here has been
-- checked against the standard's text, and every constant a contract gives
-- it in the examples is an example value. Verify both before calling this a
-- reference implementation of the standard.
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
