# Safety functions: what Safe-ROS teaches the contract

**Status (2026-11-07): proposed.** Nothing here is implemented. Design
issue 58 records it. Implementation is tracked by play_launch phase 100,
which also carries the play_launch and nano-ros halves. Every key below is
additive: no existing spelling moves, so the rename schedule of
`standard-names.md` §6 does not apply.

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
- `deadline_by: machine.iso13855/safety_distance` already states a
  distance requirement for a stationary machine.

What Safe-ROS has and this grammar lacks is the other half: a statement of
**what** the safe state commands, of **which node** has the last word on
the actuator, and of **which requirement** a hazard implements. Those are
S1 to S3. S4 and S5 close gaps its code exposed.

## 4. Proposals

### S1. The value the safe state commands: `safe_state.value`

Today `hazard-reaction` (play_launch's observer, and nano-ros's route
monitors) counts a reaction as the first publish on `safe_state.emits`
after the fault. A gate that publishes the wrong command on time passes.

```yaml
paths:
  stop:
    trigger: { input: [/safety/stop] }
    output: [/cmd_vel]
    safe_state:
      emits: /cmd_vel
      settle: 400ms
      value:                      # every one must hold on the safe-state message
        - { field: linear.x, equals: 0 }
        - { field: angular.z, equals: 0 }
```

- Shape: a list of the `when:` predicate, `{ field, <op>: <constant> }`,
  with the same operators and the same `.msg` lookup. A field the type does
  not have is `value-field-unknown`, a warning when no `.msg` is found, as
  for `when-field-unknown`.
- Static meaning: none beyond the field check. It is a requirement on the
  running system.
- Model: `PathContract.safe_state.value`, additive.
- Runtime meaning: the first publish on `emits` after the fault must
  satisfy every predicate; otherwise the reaction is `safe-state-value`
  (error), naming the field and the value seen.
- Not `holds:`: that word already means "a function holds" under `exit:`.

### S2. The node with the last word on an actuator: `topics.<t>.arbiter`

Safe-ROS is only sound if every command to the base passes through
`cmd_vel_interceptor`. Autoware's `vehicle_cmd_gate` is the same pattern.
Nothing in the grammar can say so, and nothing checks it.

```yaml
topics:
  /cmd_vel_filtered:
    arbiter: /safety/cmd_vel_interceptor
```

- Static: the arbiter must be the only publisher of the topic in the
  resolved graph (`arbiter-bypass`, error), and some hazard's reaction
  route must reach the topic through it (`arbiter-unused`, warning).
- Runtime: a publisher created on the topic by any other node is
  `arbiter-bypass` (error). The interceptor names each endpoint's owner, so
  this needs no new event.
- This is the structural half of the R6 independence review (dominance of
  the barrier). It does not claim independence: two nodes in one process,
  container or host remain one fault unit, and saying so stays R6's work.

### S3. The requirement a hazard implements: `hazards.<h>.requirement`

FRET's value to an assessor is a readable sentence traced to a numbered
requirement of the hazard analysis. A hazard carries free-text
`description:` only.

```yaml
hazards:
  obstacle_too_close:
    requirement: { id: R1, source: "HARA rev 3, section 4.2" }
```

- Meta: carried to the model and into every diagnostic about the hazard,
  so `fault-reaction-time` reads `hazard 'obstacle_too_close' (R1)`.
- The checker renders each hazard as one sentence in `check --explain`:
  "When /system/operation_mode/availability is omitted, the system shall
  reach emergency_stop within 2 s (R1)." The numbers are the contract's, so
  the bound FRET could not state is the one printed.
- An export to FRET or MTL is possible later; it is not proposed here.

### S4. A protective stop on a moving robot: a reference extension

`machine.iso13855` sizes the distance for a stationary machine and an
approaching person. Safe-ROS has the opposite case: a moving robot and an
obstacle. The requirement is that the trigger distance covers the distance
travelled while detecting and reacting, plus the braking distance:
`threshold >= v * reaction_total + v^2 / (2 * decel) + margin`.

- A reference extension, `mobile.protective_stop`, marked illustrative like
  the others until a standard's text is checked (ISO 3691-4 for driverless
  industrial trucks is the candidate).
- Keys: the robot's maximum speed, its deceleration, the margin, and the
  trigger distance, which a contract reads from the node with
  `$(param <name>) m` so the number checked is the one the node runs.
- The check is the hazard's `deadline_by`. On Safe-ROS's numbers it
  refuses 5 cm for any speed and any nonzero reaction time.

### S5. A reaction that waits on the traffic it overrides

Safe-ROS's interceptor emits the safe state only when the next `/cmd_vel`
arrives. In contract terms its path is `trigger: { input: [/cmd_vel] }`
with `/gwendolen_control` a `state: true` subscriber. A reaction arriving
at a state subscriber waits for the node's next trigger; when that trigger
is a timer, the walk charges one period. When it is another topic, the
wait is bounded only by that topic's rate, and the fault may be what
stopped it.

- First, establish what the reaction walk does today in this shape (a
  fixture, phase 100). If it charges nothing or the input's rate silently,
  that is the defect.
- Proposed rule `reaction-waits-on-traffic` (error): a reaction route
  reaches a `state:` subscriber whose node's paths to the sink are
  triggered only by inputs outside the reaction route. The fix it names is
  a timer path or an input-triggered path on the reaction topic.

### S6. Investigate: a profile for nuclear instrumentation and control

Safe-ROS targets the UK nuclear sector and cites IEC 61513. Its
classification (categories of IEC 61226 for functions, classes for
systems) is not a SIL ladder. Find out whether it fits a profile's
`integrity_scale` and `criticality` map before proposing one.

## 5. Not adopted

- **Verifying a node's code.** Safe-ROS proves its agent and arbiter. The
  contract states what a node must do and checks it against the running
  system; proving the code is the code's own evidence, and S3's `source:`
  is where a reference to it can go.
- **A safety layer as a separate agent runtime.** Out of scope for the
  grammar. The contract describes whatever the safety function is.
