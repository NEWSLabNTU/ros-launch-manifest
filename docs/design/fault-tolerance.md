# Fault tolerance: safety when most of the system is safety critical

**Status (2026-11-07): proposed; §4 revised the same day to the
contract's primitives rule; §6's questions decided the same day.** Nothing here is implemented. Design issue 59
records it; implementation is tracked by play_launch phase 101. It extends
`safety-functions.md` (design issue 58). After the revision the core gains
three things here: `m` on a redundant group (MooN), one `independence` form, and
`on_silence` on an external subscriber side, plus three `on:` classes;
recovery and fail-operational are derived, and use-case specifics are
extensions (§4.7). Every key is additive.

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

## 4. The model, in primitives

**Revised 2026-11-07.** The first version of this section proposed five
keys (`k_of` with `agree`, `recovery:`, `fail_operational:`, `silence:`,
`independent:`/`independent_of:`). Held to the contract's primitives rule
(state what the code does and what it must achieve; derive the rest) and
to extensions for use-case specifics, most were second copies:

| Proposed | Verdict | Why |
|---|---|---|
| `recovery: [restart]` | **derived** | A restart is the launch file's `respawn` and `respawn_delay`, which the model carries. Writing it again in the contract is a second copy. |
| `recovery: [switchover]` | **derived** | A hot standby is a redundant function member: losing one member does not lose the function, so nothing is entered. A cold standby that takes over is the standby node's own subscriber `max_age` + `on_violation` reaction, grammar that exists. |
| `recovery.attempts` | **dropped** | ROS launch's respawn has no bound to state, so an escalation count would be a fact nothing implements. |
| `fail_operational: true` | **derived** | A level with no safe state in time is one whose degradation cannot meet the FTTI, which `fault-reaction-time` already reports. With recovery and redundancy credited (§4.2), the same verdict is the fail-operational check. |
| `k_of.agree` (value voting) | **extension territory** | Agreement is a comparison whose tolerance and metric are domain-specific (a pose, a pressure, a surface angle). A member wrong by its own value is already a value function (`of` + `when`). Comparing members against each other needs a runtime hook extensions do not have; recorded, not proposed. |
| `silence:` on a final element | **derived**, one fact where it cannot be | §4.4. |
| `independent` and `independent_of` | **one form** | Two spellings of one claim. §4.5. |
| `m` on a redundant group | **core** | The count is a fact of the design and computable from nothing. |
| `on:` repetition, resequencing, insertion | **core** | Fault classes are the closed vocabulary every monitor uses. |

### 4.1 A redundant group's count: `functions.<f>.m`

```yaml
functions:
  attitude:
    all_of: [/fcc1/cmd, /fcc2/cmd, /fcc3/cmd]
    m: 2                        # 2oo3: held while at least 2 members hold
```

- `all_of` keeps its meaning with `m` absent (`m: 1`, 1ooN: held while
  any member holds). `m` raises the threshold.
- Members may be functions, so a member lost by its own value (a value
  function, `of` + `when`) counts as lost. That is how a monitor-control
  lane that fails silent, or reports itself wrong, takes part in k-of-n
  without a voting key.
- Runtime: the observer counts held members, as it evaluates a function
  today.
- Name: IEC 61508 writes the architecture as MooN (2oo3): M of N. N is
  the member list's length, so only M is written, and it is spelled `m`
  (decided 2026-11-07).

### 4.2 Recovery, credited and not written

- **Restart.** A node with `respawn` is credited when detection plus
  `respawn_delay` plus its startup time fits the FTTI. The startup time
  comes from a run (`check --against <run-dir>`, as link transport does,
  from `startup_timeline.json`), and only from a run (decided
  2026-11-07): absent one it is unknown, and the restart is not credited. `recovery-unfit` (warning) names a restart that
  cannot fit, so the hazard will degrade.
- **Hot standby.** A redundant group at or above its `m` after the loss keeps
  the mode; nothing is entered and nothing is charged.
- **Cold standby.** The standby's subscriber on the primary's output, with
  `max_age` and `on_violation: { reaction: take_over }`, is an ordinary
  reaction route, charged by the walk.
- **Fail-operational.** No key: a level that cannot degrade in time fails
  `fault-reaction-time`, and is rescued only by the credits above.

### 4.3 (Removed: fail-operational is §4.2's derived verdict)

### 4.4 Silence at a commanded topic

What a final element's consumer does when the topic goes quiet is a fact
of that consumer:

- **Consumer in the contract:** its `sub.<e>.max_age` and `on_violation`
  already say it. Derived; nothing new.
- **Consumer external** (a base controller, a flight controller, a safety
  PLC): one new fact on the external side,

  ```yaml
  topics:
    /cmd_vel_filtered:
      external: { side: sub, on_silence: { after: 500ms } }   # it stops by itself
  ```

  meaning the external consumer reaches its safe state after `after` of
  silence. With it, a level may end in silence: the settle is `after` plus
  the consumer's response. Without it, silence while a level holds is
  `final-element-silent` (error).

### 4.5 Independence, one form

```yaml
independence:
  - of: [/fcc1, /fcc2, /fcc3]                       # nodes or functions, pairwise
    not_sharing: [process, container, cpus]
  - of: [/safety/cmd_vel_gate, /navigation/move_base]   # arbiter vs. what it overrides
    not_sharing: [process, cpus]
```

- A requirement checked against deployment facts the model and play_launch
  have: process, container, CPU set (`independence-violated`, error).
  Composables in one container share a process unless play_launch runs
  them `isolated`; the model does not carry the mode, so `check` assumes
  the shared case and says so, and `launch`/`up` re-check with the mode
  they run.
- **Freedom from interference is derived, with no key:** nodes of
  different derived integrity in one process (a shared container under
  `observable` or `stock`) are `interference-shared-process` (warning).
  Under `isolated` the rule is silent.
- Diversity (dissimilar software or hardware) is not a deployment fact and
  stays out.

### 4.6 Communication fault classes in `on:`

The closed `on:` set gains the three the interceptor can observe:
`repetition` (a stamp equal to the previous one from the same publisher),
`resequencing` (an earlier stamp), `insertion` (a publisher on the topic
the contract does not name; on a commanded topic this is `arbiter-bypass`,
reported once). Corruption is left out: not observable above DDS.

### 4.7 Use-case specifics are extensions

| Need | Where |
|---|---|
| Protective field or separation distance | reference extension on `deadline_by` (`mobile.protective_stop`, `machine.iso13855`) |
| Braking, descent, occlusion, valve stroke physics | existing reference extensions (`vehicle.braking`, `uav.descent`, `medical.occlusion`, `process.pst`) |
| Alarm priorities and a clinician's response time (IEC 60601-1-8) | an extension deriving `settle` for an alarm reaction |
| Value agreement across redundant members | recorded; needs a runtime extension hook |
| A domain's integrity scale (IEC 61513, ...) | a profile |

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
  differently (§2, C3), so a fixed numbering would fit none exactly, and
  the steps are already facts elsewhere: a restart is the launch file's
  `respawn`, a takeover is a standby's reaction, degradation is the modes.
  The checker credits them in that order (§4.2) without a list to write.

## 6. Decisions (2026-11-07)

1. The count of a redundant group is spelled `m`, IEC 61508's MooN.
2. A node's startup time comes only from a run (`check --against`); the
   contract does not state one. A restart without a run is not credited,
   and the output says so.
3. `independence` counts three deployment facts: `process`, `container`
   and `cpus` (the CPU set from the platform file). A shared DDS
   participant, power supply or sensor is not a fact play_launch has, and
   is not accepted in `not_sharing`. `host` was decided first and is
   proposed for removal (play_launch phase 102, finding 2): `Deploy.host`
   left the model on 2026-07-31, so every node of one resolved model is on
   one host, and independence across hosts is the separation of two
   models, which no single check sees.

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
