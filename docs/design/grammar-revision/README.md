# Grammar revision: open issues by topic

Status: **exploration** (2026-11-13); the S1 decisions (T1, and T2's `tolerance`) were taken in v0.3.15 and v0.3.16. Otherwise nothing here is decided. This file is
the overview; each topic has its own file (`T1.md` ... `T10.md`) with the
verified issues, the counts behind them, every revision method compared
(change, cost, breakage, migration, risks), a recommendation, and the
questions left for a decision.

**How it was made.** A first pass reviewed the v0.3.14 grammar
(`format-reference.md`), the design docs and play_launch phase 102's
evaluation (`play_launch/docs/reports/safety-model/`), and grouped what it
found into ten topics with candidate methods. A second pass checked every
issue against the code (`file:line`), counted each affected key over 192
contracts (rlm 35, play_launch 123, the safety island 1, nano-ros 33), and
probed feasibility where a method depended on it. The probes ran in
play_launch's `tmp/grammar-revision/` and are not kept in this tree. The
second pass corrected the first in several places (listed below); the topic
files are the current state.

## Who owns what

Three repositories share the contract, and each change below belongs to one
of them:

| Repository | Role | Owns |
|---|---|---|
| **rlm** (this one) | The runtime-agnostic specification | The grammar (`field_table.rs`, the parser, `migrate`), the SystemModel schema, what every key and rule MEANS (normative text), the extension language and its reference extensions, and the realizer-neutral derivations in its crates (`derive`, `check`, `sched`). It names no realizer (`model-boundary.md`). |
| **play_launch** | The Linux realizer | Resolving a ROS 2 launch tree into the model, the static checks of a launch-plus-contract system, the interceptor and the run-time observer on Linux, `check --against` and `measure` over recorded runs, the platform file's Linux scheduling. |
| **nano-ros** | The RTOS realizer | Reading the model into an image for a board (Zephyr, FreeRTOS, ...), the monitors the image generates, the board's platform facts (`system.toml`). |

Two consequences for this revision:

- **A meaning is defined once, in rlm.** Three of the defects below exist
  because a meaning was implemented separately on each side with no rlm
  text to hold them together: what entering a mode means (the checker, the
  Linux observer and nano-ros's `ftti_budget` each answer differently), the
  default of an omitted `on:` (rlm's doc, play_launch's message, nano-ros
  each differ), and a constant predicate's value (the model carries a name
  that only a resolver with the `.msg` files can turn into a value). The
  fix in each case starts with an rlm decision and a model field, and only
  then the two realizers.
- **The static resolver sits in play_launch's repository but is not
  Linux-specific.** Its layer 2 (no ROS linked) is what nano-ros also builds
  to resolve launch trees. Rules that are part of the specification (the
  reaction walk, criticality derivation) live there today; a meaning they
  implement is still rlm's to state, and rlm's `derive` crate is where such
  a rule moves when both realizers need the same arithmetic.

Three questions were asked of every key: is the grammar **complete** (can a
real safety argument be stated), is each key **primitive** (not derivable,
not domain content that belongs in an extension), and is each **name** clear
and used for one thing only.

## The topics

| # | Topic | Verdict | Recommendation |
|---|---|---|---|
| [T1](T1.md) | Retirement debt | Confirmed and larger: five aliases deprecated at v0.1.59/60 survived 33 tags; `sub.<ep>.max_transport` inline ("legal for one release" since v0.3.4, 22 uses) was never scheduled; `links.monitor` is write-only in grammar and model. | Refuse all five at once; schedule `transport-inline`; delete `links.monitor`; a registry so value- and rule-level deprecations carry their release and a test fails when one is overdue. |
| [T2](T2.md) | Second copies of one fact | Mixed. `tolerance` duplicates `sync.max_interval` (the model copy is unread; the declaration is read by `sync-budget`). Topic wiring is derivable from remaps only on Autoware. `topics.rate_hz` is mostly on internal topics, and the model never carries the derived rate. `emits`, `buffer: latest`, `args`, `lifecycle` are not redundant. | Delete `tolerance` with a `migrate` that folds only into an existing `sync:`. Agreement census before touching topic wiring. Move an external publisher's rate onto its external side; refuse internal `rate_hz` only after the model carries the derived rate. Keep the rest. |
| [T3](T3.md) | Two forms for one structure | The model already has one hazard `reaction`; desugaring the chain form to a mode changes no route, budget or task on 37 fixtures. But "entering a mode" means three different things in code. | First route every rule through one ladder resolution and settle what entering a mode means; parser desugar is then cheap and optional. Services take `links` now (0 uses). `integrity_levels` migrates losslessly to `use:` (22 of 23 equal the default). |
| [T4](T4.md) | Domain and platform content in the core | `settle.response` is generic (wording only). `early.drop` is a braking quantity with its own unit parser and a model field nobody reads. `observes` carries no field, so a run time must guess it. `supervision {tick, switch}` is a platform fact. No contract writes `policy`. | `early` into `vehicle.braking` statically, plus a domain-free run-time form (`observes: { input, field, reached }`, `early: { within, decrease }`). `supervision` to the platform file (plumbing needed). `policy` becomes a generic `check --deny <rule>`. Reword `response`. |
| [T5](T5.md) | One word, several meanings | `reaction` confirmed (7 contexts, 4 target kinds, 312 writes; 22 of 23 `on_timeout.reaction` values name a mode). `drop` and `functions` confirmed. `within` and `response` are NOT ambiguous. `monitor` clashes mainly with nano-ros's word for `supervise`. | Rename `reaction` (`enters` for a mode, a separate key for a chain or path) and `functions`, in the contract only. A contract-only rename costs nano-ros a tag bump (it writes none of these keys); a model rename costs about 8 nano-ros files. |
| [T6](T6.md) | Naming consistency | Rate keys keep `_hz` in the name (470 writes); `$(param x) Hz` is already accepted while a literal `100Hz` is refused with a wrong message. The model says `scope_paths` (a retired word, ~147 identifiers), not `event_chains`; `standard-names.md` S5 had decided to keep `paths:`. Stale `spontaneous` in a description. | Rates as `Hz` values through rlx's quantity parser, contract keys only. Reopening `paths:` is a decision, not a fix. Extend `check-terms` to the field table's live rows. Fix the stale text and the two wrong messages now. |
| [T7](T7.md) | Fault model | Corruption and masquerade are missing, and the reason given for excluding corruption (DDS checks its frames) is overstated: RTPS has no mandatory checksum, shared memory none. Repetition and resequencing are judged on the sender's publish only; stamp equality misfires on state topics; `insertion` cannot catch masquerade. The default of an omitted `on:` is defined three ways. | Settle the `on:` default first. Then a topic's end-to-end protection as a fact: `counter { field, wraps, max_delta }`, `id { field | value }`, `crc` (presence only); the counter replaces the stamp for repetition and resequencing; `corruption` only with `crc` or an application mechanism. |
| [T8](T8.md) | Predicate expressiveness | The quoted-string case is real, and the YAML library in use (yaml-rust2) exposes scalar style, which rlm already walks. No contract needs sequences or relational limits yet. | Quoting decides (as evaluation E1 ruled), not an explicit `{ constant: }` key. Sequences and relational limits stay out until a case needs them; quantified paths with in-process (min, max, len) reduction when one does. |
| [T9](T9.md) | Interfaces without timing | Actions: a reaction route through one is refused, not merely uncharged; no contract puts an action on a route, and Autoware 1.5.0 links no `rcl_action`. Transforms: the interceptor sees TF only on publish, while the age that matters is at the consumer's lookup. | Actions: fix the fallback now; endpoint maps mirroring `srv`/`cli` when a contract needs them (not timing on the action declaration). Transforms: publishers state their frames, consumers state `transforms.<pair>.max_age`. |
| [T10](T10.md) | Deployment beyond one host | Worse than "not checkable": independence naming another host's node passes clean. The island (two launch trees, two realizers) is the only real two-host system, and resolving one launch per host cannot express it. | A resolve-only fix now (an absent node is `independence-unchecked`). Then `check --system hosts.yaml`: per-host launch, arguments and platform as check input; each model carries its host; cross-model rules run over the set. |

## Defects found in the current code

The exploration found these in shipped behaviour. They need no grammar
change and are listed apart, because they should not wait for a revision:

| Defect | Owner: where | Topic |
|---|---|---|
| A predicate naming a message constant is never judged at run time, silently: the model carries the name, not the value. A value function then reads as held (fail-unsafe) and `safe-state-violated` never fires. 22 fixtures and the island's `driver_took_over`. | rlm: the model carries the value; play_launch: observer judges it; nano-ros: the image when it judges values | T8 |
| A predicate's value is not checked against the field's type; a mismatch is silently unjudged at run time. | play_launch: `value_rules.rs` (rule text in rlm) | T8 |
| Criticality derivation reads only the chain form of a hazard's reaction, so a hazard that enters a mode credits no integrity to its reacting nodes (15 of 18 resolvable models differ; the island and rt_av_demo included). | play_launch: resolver layer 2 `manifest_loader.rs` | T3 |
| "Entering a mode" means three things: the checker walks `fallback`, the observer and `measure` use the last level, `supervise.enters` and nano-ros use the mode's own `reaction`. | rlm: the meaning; then play_launch (checker, observer, `measure`) and nano-ros (`ftti_budget`) | T3 |
| An independence entry naming a node absent from this model passes clean; an absent absolute node key is accepted with no diagnostic. | play_launch: resolver layer 2 `independence.rs` | T10 |
| A reaction route through an action is refused, and the fallback then drops the settle (200 ms reported against a true 307 ms). An action client under `cli:` parses with no refusal. | play_launch: reaction walk; rlm: parser | T9 |
| Repetition and resequencing are judged on publish only; an unstamped monitored topic is never judged for them, with no warning. | play_launch: observer | T7 |
| An omitted `on:` has three definitions (rlm doc, play_launch message, nano-ros). | rlm: the meaning; then play_launch and nano-ros | T7 |
| A literal `100Hz` under a `_hz` key is refused as "a quoted string -- remove the quotes"; a timer rate error names `paths.<p>.rate_hz`. | rlm: parser | T6 |
| `paths.<p>.trigger`'s description still lists `spontaneous`. | rlm: `field_table.rs` | T6 |
| The field census grades `links.monitor` consumed through unrelated identifiers of the same name. | play_launch: `field_census.py` | T1 |
| `EarlyCheckContract.drop_mps` has no reader in play_launch or nano-ros; the island copies the value into its board configuration by hand. | rlm: model; play_launch and nano-ros: no reader | T4 |
| `key-inventory.md` is stale on the island (16 contracts listed, 1 in the tree). | rlm: docs | T4 |

## Corrections to the first pass

The first pass of this document said, and the second pass found otherwise:

- `tolerance` has no reader (the declaration is read; only the model copy
  is not); `buffer: latest` is unread (nano-ros treats it apart from
  absent); `lifecycle` is derivable on Jazzy only (Python `LifecycleNode`
  exists on Humble); `topics.rate_hz` is kept only for external publishers
  (66 of 78 uses are internal); every fixture is migrated (one still emits
  `deprecated-spelling`); `emits` can follow `commands` (only 14 of 119 name
  a commanded topic).
- The model says `event_chains` (it says `scope_paths`; only
  `--export-graph` says `event_chains`).
- `within` and `response` are ambiguous (each is one sense in both places);
  `early.within` leaves the core with T4 (it stays).
- Moving `early` needs an rlx language change (rlx has no context list; the
  restriction is in rlm's parser and the consumer).
- Strings should take an explicit `{ constant: }` key (the YAML library
  exposes quoting, and E1 had already ruled that quoting decides).
- Actions should take timing on the action declaration (endpoint maps fit
  the services' shape); resolving per host comes first (it cannot express
  the island).

## Sequencing

1. **Fix the defects above** (play_launch and rlm, no grammar change). The
   constant predicate and the criticality of mode-form hazards first: both
   are silent and one is fail-unsafe.
2. **One removal release (T1, and T2's deletions):** refuse the overdue
   aliases, delete `links.monitor` and `tolerance`, add the deprecation
   registry and its test.
3. **Settle two meanings before any rename:** what entering a mode means
   (T3), and the default of an omitted `on:` (T7).
4. **One rename release (T4, T5, T6):** contract keys only, so nano-ros pays
   a tag bump, not a mirror rewrite; the R / R+1 / R+2 schedule of
   `standard-names.md` §6.
5. **Additions, each independent (T7 to T10):** each waits for a contract
   that needs it, except T10's resolve-only fix, which is in step 1.
