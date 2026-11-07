# Safety functions: what Safe-ROS teaches the contract

**Status (2026-11-07): proposed; revised three times the same day. A
use-case survey (§4) found the final element; a survey of large systems
(`fault-tolerance.md`, design issue 59) added silence, redundancy and
independence; the third revision (§5) holds every key to the contract's
primitives rule and moves use-case specifics to extensions, leaving one
new key here, `modes.<m>.commands`, plus `hazards.<h>.requirement`. §7's
questions are decided.**
Nothing here is implemented. Design issue 58 records it. Implementation
is tracked by play_launch phase 100, which also carries the play_launch
and nano-ros halves. Every key is additive: no existing spelling moves, so
the rename schedule of `standard-names.md` §6 does not apply.

## 1. What Safe-ROS is

Safe-ROS (Benjumea, Farrell, Dennis; University of Manchester; FMAS 2025,
arXiv 2511.14433; code at `github.com/dianabenjumea/Safe-ROS`) splits a
robot into two parts:

- an autonomous control system for routine work (the ROS Noetic navigation
  stack, `move_base` publishing `/cmd_vel`);
- a safety system of Safety Instrumented Functions (SIFs, the IEC 61508 /
  IEC 61511 term) that oversees it independently.

The one SIF built is requirement R1 from the authors' hazard analysis:
"When the robot detects that an obstacle is within 5cm of it, then it must
stop immediately."

- **Specification.** R1 is written in NASA FRET's structured English and
  compiled to LTL, `G(too_close -> F stopped)`. "Immediately" became
  "eventually" because the model checker cannot express a bound.
- **Implementation.** A Gwendolen BDI agent in Java reads `/scan` over a
  rosbridge WebSocket. When the minimum range is under 0.05 it publishes
  `true` on `/gwendolen_control`. A Python node, `cmd_vel_interceptor`,
  forwards `/cmd_vel` while no stop is set and publishes a zero `Twist`
  while one is.
- **Verification.** The agent is model-checked with AJPF. The interceptor
  is proved in Dafny: stop set implies zero output, otherwise the output is
  the input.
- **Evaluation.** Gazebo and a lab, qualitative: the robot stopped each
  time. No timing is stated, measured or bounded.

## 2. What its published code shows

Read from the repository on 2026-11-07, not run:

1. **The safe state depends on the traffic it overrides.** The interceptor
   publishes only inside its `/cmd_vel` callback. The stop flag is read
   when a command arrives. If `move_base` has stopped publishing, nothing
   is published and the base's own command timeout decides what happens.
2. **The safety function's silence is unobserved.** If the agent, the
   WebSocket or rosbridge dies, `/gwendolen_control` goes quiet and the
   interceptor keeps forwarding commands.
3. **The stop latches.** The agent only ever publishes `true`. The paper
   says forwarding resumes when stop signals stop arriving; the code does
   not.
4. **The model check may be vacuous.** The verification environment
   generates the percept `stopped` at random and never `too_close`, so if
   that environment is the whole model, R1's trigger never fires. The
   property is also about the agent's belief, not the robot's motion.
5. **The threshold is not checked against speed.** Detection takes one
   scan period plus the bridge plus the agent's cycle, unbounded, and the
   robot keeps moving through it.

## 3. Where this contract system already stands

Points 2, 5 and the absent timing are what this grammar exists for:

- a hazard with `monitors:` and `on: omission` makes the SIF's own silence
  a fault, and `reaction-unguarded` warns when a reaction's output is
  watched by nothing;
- `ftti`, the derived detection and reaction times and `safe_state.settle`
  bound "immediately";
- `deadline_by` with a reference extension (`machine.iso13855`,
  `rail.ebd`, `uav.descent`) states a requirement that is a distance or an
  energy, not a time.

What Safe-ROS has and this grammar lacks is the other half: a statement of
**what** the safe state commands, of **which node** has the last word on
the actuator, and of **which requirement** a hazard implements.

## 4. Use cases

Seven safety functions, chosen to cover the domains the bundled profiles
and reference extensions already serve. For each: what is sensed, who
decides, what is finally commanded, and what "safe" means at that point.

| # | Use case | Sensed | Decides | Finally commanded (the channel) | Safe there means |
|---|---|---|---|---|---|
| U1 | Mobile robot protective stop (Safe-ROS; ISO 3691-4 driverless trucks) | LiDAR range in a protective field | safety node, or a safety laser scanner's own logic | base velocity, through a mux | velocity zero; in the warning field, speed under a limit |
| U2 | Automated driving minimal risk manoeuvre (Autoware, the safety island) | operation-mode availability, takeover request | MRM handler | vehicle command, through `vehicle_cmd_gate`; each degradation level reaches it from a different topic (`max_velocity_candidates` for comfortable stop, `emergency_control_cmd` for emergency stop) | velocity command zero and acceleration at most zero; deceleration ramps at a jerk, so the final value is not in the first message |
| U3 | Collaborative robot, speed and separation monitoring (ISO 10218-2, ISO/TS 15066) | person distance | safety controller | joint or TCP speed | speed under a limit set by the distance zone; zero in the stop zone |
| U4 | UAV low battery or geofence (`uav.descent`) | state of charge, position | failsafe logic | flight mode, in the flight controller (often outside ROS) | mode is LAND or RTL; descent speed bounded |
| U5 | Infusion pump occlusion (`medical.occlusion`) | line pressure | pump controller | pump rate | rate zero, alarm raised |
| U6 | Process plant overpressure (IEC 61511, `process.pst`) | pressure | logic solver | valve position (the extension already calls the valve the "final element") | valve commanded closed |
| U7 | Rail emergency brake (`rail.ebd`) | speed against the supervised location | onboard supervision | brake command | emergency brake applied; traction cut |

What the seven have in common, and where the first draft was wrong:

1. **Every safety function ends at a final element.** IEC 61511 names the
   three parts of a SIF: sensors, logic solver, final elements. The
   contract already has the first two (`monitors:`, the detecting node's
   `on_violation`) and has nothing for the third. `process.pst` already
   uses the word for its valve.
2. **The final element is not where a level's reaction emits.** In U2 the
   two degradation levels emit on different topics and both reach the
   vehicle through the gate. Checking the value at `safe_state.emits`, as
   the first draft proposed, checks an intermediate command. What the
   vehicle receives is downstream, at the gate's output.
3. **Every final element has an arbiter.** A mux (U1), `vehicle_cmd_gate`
   (U2), a safety controller (U3), the flight controller (U4), the pump
   controller (U5), the logic solver (U6), the supervision (U7). The
   arbiter is a property of the channel, not of a topic in isolation.
4. **Safe is a predicate, not a value, and it depends on the level.**
   Zero (U1 stop, U5), a bound (U1 warning field, U3 zones), an
   enumeration (U4 mode, U6 closed), and in U2 a sign rather than a value,
   because a jerk-limited deceleration does not command its final value in
   its first message. The same channel has a different predicate per
   degradation level (U1: warning field, protective field; U3: zones; U2:
   comfortable stop, emergency stop).
5. **Safe must hold for as long as the level lasts.** The first draft
   checked the first message after the fault. Safe-ROS's own defect (§2.3,
   a stop that never releases, against a paper that says it does) is a
   claim about the whole interval, and so is the opposite defect, a gate
   that sends one zero and then forwards commands again. What must hold is
   an invariant: from the reaction's bound until the level is left, every
   message on the channel satisfies the level's predicate.
6. **The final element is often outside ROS** (U4 flight controller, U5
   and U7 embedded controllers, U2's island). Then the channel is the last
   topic before it, observed where it is taken, as an off-host sink
   already is (`observed_at: take`).
7. **The requirement distance comes from an extension** (U1 field size,
   U3 separation, U7 braking curve). That pattern exists (`deadline_by`).
   U1's speed-dependent field sets are degradation levels selected by
   speed, each with its own distance; they need no new core key.

## 5. The model, in primitives

**Revised 2026-11-07 (third revision).** The second revision added a
`final_elements:` section (channel, arbiter, safe predicate, silence).
Held against this repository's rule, that a contract states what the code
does and what it must achieve and that anything computable from those is
derived (play_launch `docs/design/contract-primitives.md`), and against
the rule that use-case specifics belong in extensions, most of it was a
second copy of something the contract or the launch tree already says:

| Proposed | Verdict | Why |
|---|---|---|
| `final_elements.<e>.arbiter` | **derived** | The arbiter is the publisher of the commanded topic, which the resolved graph knows. What is a requirement is that there is exactly one, and that follows from the topic being commanded by a level (below). |
| `final_elements.<e>.safe` | **folded** into `modes.<m>.commands` | The safe predicate is the minimal risk condition's own condition, and the minimal risk condition is a mode. Stating it on the element and on the mode would be two copies. |
| `final_elements.<e>.channel` | **folded** | A channel is a topic a level commands; naming it in `commands` declares it. |
| `final_elements.<e>.silence` | **derived**, one fact where it cannot be | What the consumer does when the channel goes quiet is its subscriber's `max_age` and `on_violation` when the consumer is in the contract. Only an external consumer needs it stated (`fault-tolerance.md` §4.4). |
| `final_elements:` as a section | **dropped** | Nothing is left in it. "Final element" stays the checker's word for a topic some level commands, in diagnostics and `--explain`. |
| `hazards.<h>.requirement` | **core** | A trace to the hazard analysis is not computable from anything, and every safety standard has one. |
| protective-stop distance | **extension** | Use-case specific arithmetic, on the existing `deadline_by` pattern. |

### The one new key: `modes.<m>.commands`

```yaml
modes:
  slow:
    requires: [scan_ok]
    commands:
      /cmd_vel_filtered: [{ field: linear.x, le: 0.3 }]
  stopped:
    requires: []
    reaction: safety.stop
    commands:
      /cmd_vel_filtered: [{ field: linear.x, equals: 0 }, { field: angular.z, equals: 0 }]
```

- A requirement: while this level is active, every message on the named
  topic satisfies the predicates (the `when:` shape: `field`, then
  `equals`, `not_equals`, `lt`, `le`, `gt`, `ge`; fields checked against
  the `.msg`).
- A topic named in any level's `commands` is a **final element**. Derived
  from that, with no further key:
  - it has exactly one publisher in the resolved graph, its arbiter
    (`arbiter-bypass`, error, statically and at run time from endpoint
    owners);
  - every reaction route of a level that commands it reaches it through
    that publisher (`reaction-bypasses-arbiter`, error);
  - the runtime invariant runs from the reaction's derived bound until the
    level is left (`safe-state-violated`, error);
  - silence on it while the level holds is judged by what its consumer
    does on silence: the consumer's `sub.max_age` + `on_violation` when the
    consumer is in the contract, the external side's fact when it is not
    (`fault-tolerance.md` §4.4). A consumer that does nothing on silence
    makes silence `final-element-silent` (error).
- `paths.<p>.safe_state` is unchanged: `emits` and `settle` stay the
  reaction's own facts. The walk checks that the route continues from
  `emits` to the commanded topic.
- This is still run-time assurance's shape (ASTM F3269): the arbiter is
  the switch, the level's reaction the recovery function, its `commands`
  what bounds the recovered behaviour.

## 6. Proposals, final

### S1. `modes.<m>.commands` (replaces `final_elements:`)

As §5. Model: `ModeContract.commands`, additive. Static rules:
`arbiter-bypass`, `reaction-bypasses-arbiter`, `commands-field-unknown`.
Runtime rules: `safe-state-violated`, `final-element-silent`, runtime
`arbiter-bypass`. The arbiter is dominance only; independence is
`fault-tolerance.md` §4.5.

### S2. The requirement a hazard implements: `hazards.<h>.requirement`

Core. FRET's value to an assessor is a readable sentence traced to a
numbered requirement; a hazard has free-text `description:` only.

```yaml
hazards:
  obstacle_too_close:
    requirement: { id: R1, source: "HARA rev 3, section 4.2" }
```

The id goes into the model and every diagnostic about the hazard, and
`check --explain` renders each hazard as one sentence built from the
contract's own numbers: "When /scan is omitted, the system shall reach
stopped within 300 ms, and while stopped /cmd_vel_filtered holds
linear.x = 0 (R1)."

### S3. A reaction that waits on the traffic it overrides

No key; a rule, if needed. Safe-ROS's interceptor emits the safe state
only when the next `/cmd_vel` arrives. First establish what the walk does
in this shape; if it charges nothing, or the input's rate silently, add
`reaction-waits-on-traffic` (error). At run time the invariant already
reports it: the commanded topic goes silent.

### S4. A protective stop on a moving robot: extension

`mobile.protective_stop`, a reference extension on the existing
`deadline_by` pattern: `field >= v * reaction_total + v^2 / (2 * decel)
+ margin`, the field read by `$(param ...)`. Speed-dependent field sets
are degradation levels, one check each. Illustrative until ISO 3691-4's
text is checked.

### S5. Investigate: a profile for nuclear instrumentation and control

A profile is an extension-level artifact already. Find out whether IEC
61513's classification (IEC 61226 categories, system classes) fits a
profile's `integrity_scale` and `criticality` before proposing one.

### No new key: a monitor for the safety function itself

A hazard whose `monitors:` is the safety function's own output, with
`on: omission`; `reaction-unguarded` already warns when it is missing. A
guide example, not a key.

## 7. Decisions (2026-11-07)

1. `final_elements` was decided as a section name, then the section was
   dropped (§5); the word stays as the checker's name for a commanded
   topic.
2. The invariant starts at the reaction's derived bound, not at the first
   message that satisfies the predicates. It is the stronger check, and a
   late reaction is then reported twice, once by `fault-reaction-time` and
   once here, each naming its own evidence.
3. `commands` is on modes only. A one-level reaction is written as a mode
   when it needs `commands`; allowing it on event chains too would be two
   places for one fact.
4. A relational predicate (a limit that is a function of a measured
   quantity) is out of scope; discrete levels cover the practice, and an
   extension can check the continuous form statically.

## 8. Not adopted

- **Verifying a node's code.** Safe-ROS proves its agent and arbiter. The
  contract states what a node must do and checks it against the running
  system; proving the code is the code's own evidence, and
  `requirement.source` is where a reference to it can go.
- **A safety layer as a separate agent runtime.** Out of scope for the
  grammar. The contract describes whatever the safety function is.
