# Safety functions: what Safe-ROS teaches the contract

**Status (2026-11-07): proposed, revised the same day after a use-case
survey (§4).** Nothing here is implemented. The first draft proposed a
per-path `safe_state.value` and a per-topic `arbiter`. Seven use cases
showed that the value and the arbiter belong to one thing, the actuator
channel the safety function finally commands, and that what must hold
there is an invariant over the whole degradation level, not one message.
§5 is that model, and §6 the proposals revised to it. Design issue 58
records it. Implementation is tracked by play_launch phase 100, which also
carries the play_launch and nano-ros halves. Every key is additive: no
existing spelling moves, so the rename schedule of `standard-names.md` §6
does not apply.

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

## 5. The model: a final element per actuator channel

A safety function is sense, decide, act. The contract states the first two
today. This adds the third as one declaration per channel, which every
hazard and every degradation level that reaches it shares:

```yaml
final_elements:
  base_velocity:
    channel: /cmd_vel_filtered            # the last topic before the actuator
    arbiter: /safety/cmd_vel_interceptor  # the only node that may publish it
    safe: [{ field: linear.x, equals: 0 }, { field: angular.z, equals: 0 }]

modes:
  slow:
    requires: [obstacle_clear]
    commands: { base_velocity: [{ field: linear.x, le: 0.3 }] }
  stopped:
    requires: []
    reaction: safety.stop                 # reaches base_velocity through the arbiter
```

- **`final_elements.<e>.channel`** is the topic the actuator consumes, or
  the last one before it when the actuator is outside ROS.
- **`arbiter`** is the only node that may publish the channel. Static: it
  is the only publisher in the resolved graph (`arbiter-bypass`, error),
  and every reaction route that ends at the element passes through it
  (`reaction-bypasses-arbiter`, error). Runtime: a publisher created on the
  channel by another node is `arbiter-bypass` (the interceptor names each
  endpoint's owner).
- **`safe`** is the predicate of the minimal risk condition on this
  element, in the `when:` predicate shape (`field`, then `equals`,
  `not_equals`, `lt`, `le`, `gt`, `ge`), fields checked against the
  `.msg`. One fact, stated once, shared by every hazard that ends there.
- **`modes.<m>.commands`** gives a degradation level its own predicate per
  element. A level that names no predicate for an element it reaches is
  held to nothing there, except the minimal risk condition, which is held
  to the element's `safe`.
- **The invariant.** From the moment the reaction's derived bound expires
  until the level is left (its `exit:` or recovery), every message on the
  channel satisfies the level's predicate. A message that does not is
  `safe-state-violated` (error) naming the level, the field and the value.
  Silence on the channel while the level holds is reported as such
  (`final-element-silent`), because a channel that stops publishing leaves
  the actuator to its own timeout, which is U1's §2.1 defect.
- **`safe_state` on a path stays** what it is today, the settle and the
  topic the reaction emits on; the walk checks that the route continues
  from `emits` to a final element.

This is also the vocabulary of run-time assurance (ASTM F3269): a complex
function, a safety monitor, a recovery control function and a switch. The
switch is the arbiter, the recovery control function is the reaction, and
the safe predicate is what bounds the recovered behaviour.

## 6. Proposals, revised

### S1. `final_elements:` (replaces the first draft's S1 and S2)

As §5. Model: `Contracts.final_elements` and `ModeContract.commands`,
additive. Static rules: `arbiter-bypass`, `reaction-bypasses-arbiter`,
`final-element-unreached` (a declared element no hazard's reaction
reaches, warning), `safe-field-unknown`. Runtime rules:
`safe-state-violated`, `final-element-silent`, runtime `arbiter-bypass`.
The arbiter is dominance only; independence of the barrier stays the R6
review's question.

### S2. The requirement a hazard implements: `hazards.<h>.requirement`

Unchanged from the first draft. FRET's value to an assessor is a readable
sentence traced to a numbered requirement of the hazard analysis; a hazard
has free-text `description:` only.

```yaml
hazards:
  obstacle_too_close:
    requirement: { id: R1, source: "HARA rev 3, section 4.2" }
```

The id goes into the model and every diagnostic about the hazard, and
`check --explain` renders each hazard as one sentence with the contract's
own numbers: "When /scan is omitted, the system shall reach stopped within
300 ms, and while stopped base_velocity holds linear.x = 0 (R1)." The
sentence now includes the final element's predicate, which the first
draft could not state.

### S3. A reaction that waits on the traffic it overrides

Unchanged. Safe-ROS's interceptor emits the safe state only when the next
`/cmd_vel` arrives: its path is `trigger: { input: [/cmd_vel] }` with the
stop flag a `state: true` subscriber. A reaction arriving at a state
subscriber waits for the node's next trigger; when that is another topic,
the wait is bounded only by that topic's rate, and the fault may be what
stopped it. First establish what the walk does in this shape; if it
charges nothing, or the input's rate silently, add
`reaction-waits-on-traffic` (error). With S1 the runtime half is already
covered: the channel goes silent and `final-element-silent` reports it.

### S4. A protective stop on a moving robot: a reference extension

Demoted to an example. The pattern exists (`deadline_by` with an
extension). `mobile.protective_stop` checks
`field >= v * reaction_total + v^2 / (2 * decel) + margin` with the field
read by `$(param ...)`; speed-dependent field sets are degradation levels,
one check each. Illustrative until ISO 3691-4's text is checked.

### S5. Investigate: a profile for nuclear instrumentation and control

Unchanged. Safe-ROS cites IEC 61513, whose classification (IEC 61226
categories for functions, classes for systems) is not a SIL ladder. Find
out whether it fits a profile's `integrity_scale` and `criticality` before
proposing one.

### No new key: a monitor for the safety function itself

The standards all require it (the third level of automotive three-level
monitoring, IEC 61508's diagnostics) and the grammar already says it: a
hazard whose `monitors:` is the safety function's own output, with
`on: omission`. `reaction-unguarded` already warns when it is missing.
This needs a guide example, not a key.

## 7. Open questions for the decision

1. The key's name. `final_elements` is IEC 61511's word, which this
   vocabulary's own rule prefers (`standard-names.md` §1), and
   `process.pst` already uses it. `actuators` reads more plainly to a ROS
   author but names the device, not the command channel.
2. Whether the invariant starts at the reaction's derived bound, or at the
   first message after the fault that satisfies the predicate. The first is
   the stronger check and catches a late reaction twice (once here, once in
   `fault-reaction-time`).
3. Whether `commands` belongs on modes only, or also on an event-chain
   reaction that is not a mode.
4. A relational predicate (U3's speed limit as a function of the measured
   distance) is out of scope; discrete zones as levels cover the practice.
   Revisit if a use case needs the continuous form.

## 8. Not adopted

- **Verifying a node's code.** Safe-ROS proves its agent and arbiter. The
  contract states what a node must do and checks it against the running
  system; proving the code is the code's own evidence, and
  `requirement.source` is where a reference to it can go.
- **A safety layer as a separate agent runtime.** Out of scope for the
  grammar. The contract describes whatever the safety function is.
