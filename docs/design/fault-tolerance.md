# Fault tolerance: safety when most of the system is safety critical

**Status (2026-11-07): proposed.** Nothing here is implemented. Design
issue 59 records it; implementation is tracked by play_launch phase 101.
It extends `safety-functions.md` (design issue 58, the final element),
which this survey also revises in one place (§4.4, silence at the final
element). Every key is additive.

## 1. The question

`safety-functions.md` modelled one safety function beside a system that
does the work: sense, decide, command a final element into a safe state.
That is the shape of a protective stop. It is not the shape of an aircraft
flight control system, a spacecraft, a level-4 vehicle at speed or a
closed-loop insulin pump, where most of the components are safety critical
and there is often no safe state to fall to. This document surveys how
such systems are made safe and asks what the contract needs to state them.

## 2. Case studies

Sources are listed in §8. Several are secondary (vendor explainers,
patents, slides); the findings below use only what more than one source,
or a primary source, supports.

| # | System | Domain | How it stays safe |
|---|---|---|---|
| C1 | Airbus fly-by-wire | aerospace | Each flight control computer has a command (COM) and a monitor (MON) path with different software; a COM/MON disagreement makes the computer fail silent and another takes over. Control laws degrade normal -> alternate -> direct with the number and kind of failures, removing envelope protections at each step. |
| C2 | Boeing 777 primary flight computers | aerospace | Three computers, each with three internal lanes on dissimilar processors and compilers; the actuator electronics select one output by a voting algorithm. |
| C3 | Spacecraft FDIR (ESA missions) | space | Faults are handled at the lowest level that can (unit, then subsystem, then onboard computer, then ground), first returning to the previous mode (fail-operational) and escalating on recurrence; safe mode gives the ground time. In a critical phase (orbit insertion) safe mode is forbidden, because interrupting thrust is the hazard. |
| C4 | ARINC 653 integrated modular avionics | aerospace | Applications share a computer in time and space partitions. Health monitoring tables assign each error a level (process, partition, module) and a recovery action (ignore, restart, stop), the integrator's configuration. |
| C5 | Level 3+ automated driving | automotive | Fail-operational: after a fault, the intended operation continues for a limited time, long enough to reach a minimal risk condition, because stopping at once is not safe at speed. Monitor-control pairs; full triple redundancy is seldom used. ISO 26262 requires freedom from interference between elements of different ASIL: timing and execution, memory, exchange of information. |
| C6 | Nuclear protection systems (IAEA SSR-2/1) | nuclear | Five levels of defence in depth, independent as far as practicable; redundant channels voted (2-of-3, 2-of-4); diversity against common cause failure, since identical software in identical channels fails together. Protection trips by de-energising. |
| C7 | Closed-loop insulin delivery | medical | A safety supervision layer, always active, supervises every pump command and can veto any delivery the control layer requests; it suspends insulin below a glucose threshold, or predicts and acts earlier. The supervision layer can be approved first and the controller added above it. |
| C8 | Ventilators (ISO 80601-2-12) | medical | Technical alarms on power failure; spontaneous breathing must remain possible during loss of ventilation, which designs meet with a valve that opens when the device fails. |
| C9 | Surgical robots (da Vinci) | medical | Redundant sensors compared at startup and in use, health watchdogs across many controllers, brakes on the arms, single-failure operation. |
| C10 | F-16 Auto-GCAS | aerospace | Run-time assurance: a monitor predicts the trajectory against terrain and, at the last moment, takes over with a recovery manoeuvre the pilot can end. The recovery must not itself put the aircraft in an uncontrollable state. |
| C11 | Railway safety communication (EN 50159) | rail | Over an untrusted channel, each message carries a sequence number, a timestamp and a safety code, against repetition, deletion, insertion, resequencing, corruption, delay and masquerade. |
| C12 | AUTOSAR watchdog manager | automotive | Three supervisions of a software entity: alive (it runs at the right rate), deadline (between two checkpoints in time), logical (its control flow passes the checkpoints in a legal order); failures escalate. |

## 3. What they have in common

1. **Fail-operational, not only fail-safe.** C1, C3, C5 and C10 have
   phases with no safe state: in flight, during orbit insertion, at speed
   before a stop is reached. The requirement there is that a function is
   not lost, or not lost for longer than a bound, rather than that a safe
   state is reached. The contract states the second kind only.
2. **Redundancy is more than "lost when all are lost".** C1 and C2 vote or
   compare; C6 votes k-of-n. Availability redundancy (any one of n is
   enough) is what `functions.<f>: { all_of }` states today. Integrity
   redundancy (k of n must agree) catches a wrong value that availability
   redundancy passes on.
3. **A monitor-control pair turns a wrong value into silence.** C1's
   COM/MON, C5's monitor-control pairs, C7's supervisor: a lane that
   disagrees with its monitor stops sending. Downstream, a value fault
   becomes an omission, which the contract already detects. The pair is
   the cheapest way to make value faults visible without voting.
4. **Recovery comes before degradation, and escalates.** C3's levels,
   C4's health monitoring table, C12's escalation: retry or restart
   locally, then switch to a redundant unit, then degrade, then the safe
   mode, each step taken when the previous one failed or recurred. The
   contract jumps from detection to a degradation level; a restart, which
   play_launch already performs (`respawn`), is not credited.
5. **The fallback depends on the phase.** C3 forbids safe mode in orbit
   insertion; C5's minimal risk condition is different on a motorway and
   in a car park. A mode's fallback sequence already expresses this when
   each phase is its own mode, but nothing checks that a phase without a
   reachable safe state has the fail-operational redundancy it needs.
6. **Independence is the argument, and it is checkable here.** C6's
   independent levels and diversity, C5's freedom from interference, C2's
   dissimilar lanes: redundancy is worth nothing if the members share a
   fault. Most of the facts that decide it are ones play_launch knows:
   which process, container, host, CPU set and memory group a node runs
   in, and which nodes share an executor. The R6 review asked this for
   barriers; large systems ask it for every redundant group.
7. **Communication has its own fault classes.** C11 lists seven threats;
   ISO 26262's freedom from interference names exchange of information as
   one of three kinds of interference. The contract's `on:` set is
   omission, late, loss and reported. Repetition (the same stamp twice),
   resequencing (a stamp going backwards) and insertion or masquerade (a
   publisher that should not exist) are observable by the interceptor
   today; corruption is not (DDS checks its own frames).
8. **Silence can be the safe state.** C6 trips by de-energising, C8's
   valve opens when the device fails, C9's brakes engage without power.
   For such a final element, a channel that goes silent is not a failure
   of the reaction; it is the reaction, taken by the actuator after its
   own timeout. Safe-ROS's base is the same: its command timeout is what
   actually stops it when the interceptor goes quiet.
9. **The safety supervisor sits between control and actuator, with
   veto.** C7, C10 and C1's monitor all have the shape of the final
   element's arbiter in `safety-functions.md`. That model holds; what is
   new is that a large system has many of them, one per actuator channel,
   each with its own lanes.
10. **Logical supervision is the one check with no contract counterpart.**
    C12's control-flow checkpoints. A node's paths already name what
    triggers what; whether the order of path executions is legal is a
    finer claim than the contract makes. Not proposed; recorded.

## 4. The model, revised

The safety-functions model (a final element per actuator channel, with an
arbiter and a predicate per degradation level) holds for every case. Five
additions make it fit systems where most components are critical. Each is
one key or one rule; none changes an existing one.

### 4.1 Redundancy with a count: `functions.<f>.k_of`

```yaml
functions:
  attitude:
    k_of: { k: 2, of: [/fcc1/cmd, /fcc2/cmd, /fcc3/cmd] }   # 2-of-3 must agree
  wheel_odometry:
    all_of: [/odom/left, /odom/right]                       # 1-of-2 is enough
```

- `all_of` stays: available while any member is (1-of-n).
- `k_of` adds integrity: the function holds while at least k members are
  present **and** agree. Agreement is stated as a tolerance on a field
  (`agree: { field: <path>, within: <value> }`); without one, presence
  alone counts and `k_of` is availability with a higher threshold.
- Runtime: the observer counts healthy members and reports the function
  lost below k, as it reports a mode lost today.

### 4.2 Recovery before degradation: `recovery:`

```yaml
hazards:
  planner_crash:
    monitors: [/planning/trajectory]
    on: omission
    ftti: 2s
    recovery:                        # tried first, in order; each must fit the ftti
      - restart: /planning/planner   # the node's respawn, timed from the model
        attempts: 2                  # recurrence beyond this escalates
      - switchover: backup_planner   # a function with a standby member
    enters: degraded                 # only if recovery fails or does not fit
```

- A recovery step is credited only if detection plus the step's time fits
  the FTTI. A restart's time is the node's `respawn_delay` plus its
  measured startup (`startup_timeline.json`); a switchover's is the
  standby's detection and takeover.
- `attempts` is C3's and C12's escalation on recurrence.
- New rule `recovery-unfit` (warning): a step that cannot fit the FTTI,
  so the hazard will in fact degrade.

### 4.3 Phases without a safe state: `fail_operational:`

A mode marked `fail_operational: true` (orbit insertion, highway driving
before the MRC is reached) has no minimal risk condition to fall to within
its own time. The checker requires that every hazard that can occur in it
is covered by recovery (§4.2) or by a `k_of`/`all_of` function, not by
degradation alone (`fail-operational-uncovered`, error).

### 4.4 Silence at the final element (revises `safety-functions.md` §5)

```yaml
final_elements:
  base_velocity:
    channel: /cmd_vel_filtered
    arbiter: /safety/cmd_vel_interceptor
    safe: [{ field: linear.x, equals: 0 }]
    silence: { after: 500ms, becomes: safe }   # the base stops on its own timeout
```

- `silence: { after, becomes: safe }` is a fact about the actuator:
  de-energise to trip, a valve that opens, a base command timeout.
- With it, a reaction may end in silence, and its settle is `after` plus
  the actuator's own response. `final-element-silent` is then not an error
  but the reaction's success.
- Without it (`becomes: hold`, the default), silence leaves the actuator
  at its last command, and `final-element-silent` stays an error.

### 4.5 Independence, from what play_launch knows: `independent:`

```yaml
functions:
  attitude:
    k_of: { k: 2, of: [/fcc1/cmd, /fcc2/cmd, /fcc3/cmd] }
    independent: [process, container, cpus]     # what the members must not share
final_elements:
  base_velocity:
    arbiter: /safety/cmd_vel_interceptor
    independent_of: [/navigation/move_base]      # the arbiter vs. what it overrides
    independent: [process, host]
```

- The checker compares the members' deployment facts the model already
  carries (process, container, host, CPU set from the platform file,
  memory group from the cgroup config) and reports each shared one
  (`independence-violated`, error).
- **Freedom from interference** follows from the same facts, without a
  key: a node whose derived integrity is above another's, in the same
  process (a shared container under `observable` or `stock`), is
  `interference-shared-process` (warning, naming ISO 26262's memory and
  timing interference). Under `isolated` the two are separate processes
  and the rule is silent.
- This is the R6 review's question answered for redundancy, where the
  answer is mechanical. Diversity (C2's dissimilar processors, C6's
  diverse software) is not a deployment fact and stays a stated claim.

### 4.6 Communication fault classes in `on:`

Extend the closed `on:` set with the three the interceptor can observe:

- `repetition`: a message whose stamp equals the previous one from the
  same publisher;
- `resequencing`: a stamp earlier than the previous one;
- `insertion`: a publisher on the topic that the contract does not name
  (on a final element's channel this is `arbiter-bypass`).

Corruption is left out: it is not observable above DDS, and a contract
that claims to detect it would claim something nothing checks.

## 5. Better models considered

- **Run-time assurance (ASTM F3269) as the whole model.** Complex
  function, safety monitor, recovery control function, switch. It fits a
  single protective function (C10) and the final element maps onto it, but
  it has nothing for redundancy, recovery levels or independence, which is
  what large systems are made of. Kept as the vocabulary of one final
  element, not adopted as the model.
- **AADL's error model (EMV2).** Error types, propagations, component
  error behaviour as state machines, composite behaviour (k-of-n). The
  most complete existing model, and the contract's `on:` set was already
  closed under it. Adopting it whole would make every author write state
  machines; the k-of-n and propagation ideas are taken (§4.1, §4.5), the
  state machines are not.
- **A safety case (GSN) as the organising structure.** Goals, strategies,
  evidence. It is how an assessor reads the argument, not what a checker
  verifies. The requirement trace (`safety-functions.md` S2) is the hook a
  GSN tool needs; generating GSN from a checked contract is a possible
  later export.
- **FDIR levels as a numbered hierarchy.** Missions number them
  differently (§2, C3), so a fixed numbering would fit none exactly. The
  ordered `recovery:` list keeps the escalation without the numbers.

## 6. Open questions

1. Whether `k_of` agreement belongs on the function or on each final
   element's arbiter (C2 votes at the actuator electronics, C6 at the logic
   solver).
2. Whether a restart's time should come from a measured startup only, or
   may be stated, as a cost is (`budget`) when nothing is measured.
3. Whether `fail_operational` is a mode property or follows from the mode
   having no `fallback:` that can be reached in time.
4. Which deployment facts count for `independent:`. Process, container,
   host and CPU set are certain; a shared DDS participant, a shared power
   supply or a shared sensor are not deployment facts play_launch has.

## 7. Not adopted

- **Logical (control-flow) supervision** (C12). Finer than the contract's
  path model; recorded, not proposed.
- **Corruption** as a fault class (§4.6).
- **Proving lane diversity.** A stated claim, not checked.

## 8. Sources

Primary or official where available; secondary sources marked.

- Airbus COM/MON and control laws: patents US8761969, US9168995
  (patents.google.com); control-law summary, code7700.com (secondary).
- Boeing 777 PFC: patents US5515282, US5550731 (secondary for the lane
  details).
- Spacecraft FDIR: A&A 2021 (aanda.org, aa38519-20, Table 3); ESA
  presentation "FDIR variability and impact on avionics"
  (indico.esa.int); Eu:CROPIS and ExoMars papers (secondary).
- ARINC 653 health monitoring: Event-B formalisation, arXiv 1508.06479.
- Fail-operational driving: TUM dissertation (mediatum.ub.tum.de 1535122);
  Elektrobit, "The safe state" (secondary).
- Freedom from interference: TASKING whitepaper; arXiv 1804.04349.
- Nuclear defence in depth: IAEA SSR-2/1 presentations (iaea.org); ONR
  NS-TAST-GD-036.
- Closed-loop insulin safety layers: PMC4738202, PMC6551970.
- Ventilators: ISO 80601-2-12 tables of contents (clause 201.103);
  ventilator valve patent US8656913.
- Surgical robots: Alemzadeh et al., arXiv 1507.03518; Intuitive
  Surgical presentation (lirmm.fr, 2009).
- Auto-GCAS: NASA NTRS 20200001821; Hobbs et al., arXiv 2110.03506.
- EN 50159: railwaynews.net summary; ERA SUBSET-098 (secondary for the
  threat list).
- AUTOSAR watchdog manager: AUTOSAR CP SWS WatchdogManager R23-11.
