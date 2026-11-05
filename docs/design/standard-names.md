# Standard names: the contract vocabulary in the terms its readers know

**Status (2026-11-04): decided; the names in §2 are final (phase 98 N0, the
table in §2.6) and implemented: read with a warning in v0.3.3-v0.3.5,
refused naming the new name since v0.3.6, and kept out of user-facing text
by `scripts/check_terms.py` (`types/tests/retired_terms.rs`); since v0.3.9
an old S1-S5 rule id gets no message (R+2: it reads as `ack-stale`). Those
in §3 are proposals until their area is touched.**
Implementation is tracked by play_launch phase 98. Design issue 57 records the
decision.

## 1. The rule

A word the contract, the model, a diagnostic or a guide uses for a concept
that a standard already names takes the standard's word. A word of our own is
kept only where no standard term exists, or where the standard term would be
read as something else here.

The readers this vocabulary is written for are safety assessors (ISO 26262,
ISO 21448, UL 4600), real-time engineers (scheduling theory, AUTOSAR OS and
TIMEX) and ROS 2 developers. A private metaphor costs each of them a lookup;
a standard word used for a different concept costs more, because it is read
without a lookup and read wrong.

The audit behind this document read the whole contract grammar
(`types/src/field_table.rs`, 184 rows), about 140 diagnostic rule ids, the
`model`, `sched` and `derive` types, and play_launch's interception and Health
vocabulary, and counted each candidate across this repository, play_launch
and nano-ros.

## 2. Wrong: a standard word for a different concept, or a clash

These are the ones a reviewer from the field would stop at. They are fixed
first.

### S1. `severity` holds an integrity level

`hazards.<h>.severity: ASIL_D`, `severity_levels: [QM, ASIL_A, ...]`, the
profiles' scales, and `severity-unknown`.

In ISO 26262 the **severity** of a hazardous event is its class S0 to S3. The
**ASIL** is derived from severity, exposure (E0 to E4) and controllability
(C0 to C3) together (Part 3, Table 4); IEC 61508's SIL and DO-178C's DAL are
integrity levels in the same sense. The key holds the result, not the
severity, so a reader who knows the standard reads `severity: ASIL_D` as a
category error.

**Proposal:** `integrity` and `integrity_levels`; `integrity-unknown`. The
profile vocabulary (`types::profile`) follows. `asil` was considered and
rejected: the bundled profiles carry SIL, DAL and other scales, and the key
must name all of them.

### S2. `rung` and `ladder` name a degradation level and its order

`fallback:` lists the modes a lost mode falls to, in order. The docs,
diagnostics and types call each entry a rung and the list a ladder:
`RungRole`, `RungRoute`, the model's `rungs[]` (phase 85 D4),
`ladder-rung-budget`, `ladder-unterminated`.

No standard says rung. AUTOSAR speaks of degradation modes; the graceful
degradation literature of **degradation levels**; the contract's own key
already says fallback.

**Proposal:** **degradation level** (a level) and **fallback sequence** (the
ordered list, after the key that states it); `RungRoute` becomes
`LevelRoute`, `RungRole` becomes `LevelRole`, the model's `rungs[]` becomes
`degradation_levels[]`, `ladder-rung-budget` becomes
`degradation-level-budget`, `ladder-unterminated` becomes
`fallback-without-mrc` (see S3). "Degradation chain" was considered and
rejected: S5 gives "chain" to the event chain, and two chains in one rule
vocabulary is the clash this document removes.

### S3. `floor` is a minimal risk condition

The last entry of a fallback list, which must require nothing losable, is
called the floor (`call-unbounded-floor`, "the floor against `ftti`").

SAE J3016, ISO 21448, UL 4600 and UNECE R157 call the state a system falls to
when nothing else is available the **minimal risk condition** (MRC), and the
manoeuvre that reaches it the minimal risk manoeuvre (MRM). Autoware uses
MRM for exactly these nodes.

**Proposal:** **minimal risk condition** in prose, `mrc` in identifiers:
`call-unbounded-floor` becomes `call-unbounded-mrc`.

### S4. `window` is a dwell time

`modes.<m>.window` is a transitional level that holds AT LEAST its duration
before the next one (phase 84: "a window is a least time").

A window means an interval something must happen inside, and this codebase
uses it that way too (Health's 30-minute trend window, the one-second
endpoint windows). The concept here is the switched-systems **dwell time**: a
minimum time in a mode before switching. UNECE R157 calls the driver-takeover
instance a transition demand.

**Proposal:** `min_dwell` (with `param:` unchanged); `window-expiry` becomes
`dwell-expiry`, `window-param` becomes `dwell-param`.

### S5. "scope path" is an end-to-end timing chain

A manifest-level `paths:` entry states an end-to-end requirement between two
topics; the docs call it a scope path, and the derived route between its ends
a route.

AUTOSAR TIMEX calls this an **event chain** with a latency constraint; the
academic literature (Becker et al., Dürr et al.) a **cause-effect chain**.
"Scope" already means a launch-file scope in this code base (the scope table,
`ScopeInfo`), so the term clashes inside our own vocabulary.

**Proposal:** **event chain** in prose and diagnostics; the key `paths:` is
kept (it is short and not wrong). `scope-budget` becomes
`chain-latency`, `scope-sampling-feasibility` becomes
`chain-sampling-feasibility`.

One trap: `chains:` was a key until phase 68 (a WRITTEN route, retired as a
second copy of the graph), and `chain-sampling-feasibility` was this rule's
id before then. Reusing "chain" for the requirement is right by the standards
but means the `chains:` removal message must keep saying what it removed (a
written route) and point at `paths:`, and the reused rule id must not be read
as the old one's return.

### 2.6 Decided (N0)

| Old | New | Where |
|---|---|---|
| `hazards.<h>.severity` | `integrity` | key |
| `severity_levels` | `integrity_levels` | key |
| `severity-unknown` | `integrity-unknown` | rule |
| rung / ladder | degradation level / fallback sequence | prose, messages |
| `RungRole`, `RungRoute`, `rungs[]` | `LevelRole`, `LevelRoute`, `degradation_levels[]` | types, model |
| `ladder-rung-budget` | `degradation-level-budget` | rule |
| `ladder-unterminated` | `fallback-without-mrc` | rule |
| floor | minimal risk condition (MRC) | prose, messages |
| `call-unbounded-floor` | `call-unbounded-mrc` | rule |
| `modes.<m>.window` | `min_dwell` | key |
| `window-expiry`, `window-param` | `dwell-expiry`, `dwell-param` | rules |
| scope path | event chain | prose, messages |
| `scope-budget` | `chain-latency` | rule |
| `scope-sampling-feasibility` | `chain-sampling-feasibility` | rule |

## 3. Imprecise: a standard term exists

Fixed when the area is next touched, in the same release discipline (§5).

Progress (2026-11-04): M1, M2, M4 and M5 are at release R in v0.3.7 --
the contract keys and the platform-file key take the new spelling, the old
one reads with a warning and `migrate` rewrites it (M1's platform-file key
by hand). The model's field names and the Rust types are unchanged until
nano-ros moves with them. M6 is done in play_launch (`chains.json` format
2). M3 is at release R in v0.3.8: `fault-reaction-time`, `degradation-level-time`, `latency-bound-overflow`. M7 is decided (2026-11-05): **task**; play_launch phase 99 carries it.

Progress (2026-11-05): v0.3.9 is R+1 for M1-M5. Every old key, the
`spontaneous` trigger value, the platform file's `mapper:` and the three
old M3 rule ids are refused naming the new spelling; the model moved with
them (schema 4: `monitors`, `arrival_curve`, `aperiodic`, `kind:
aperiodic`; schema 3 and older read for good), as did the Rust types;
nano-ros follows with its own names too (its phase 534 W3).
`rule_names` carries each rename's stage (`Accepted`, `Refused`,
`Retired`).

Progress (2026-11-05): v0.3.10 is release R for M7. `system.toml` reads
`[tasks.<name>]` and `[[component]] group_tasks`; `[tiers.*]` and
`group_tiers` read this release with one `deprecated-spelling` diagnostic
per key naming the new one, and `migrate` rewrites the file
(`system_config::migrate_system_toml_str`, a textual TOML edit). The sched
types are `TaskDef`, `TaskPlatformSpec`, `ResolvedTask`,
`ResolvedTaskTable` (field `tasks`), `DEFAULT_TASK`, with the old names as
`#[deprecated]` aliases for this release. The model keeps
`execution.tiers` (schema 4) until R+1.

Progress (2026-11-05): v0.3.11 is R+1 for M7. `[tiers.*]` and
`group_tiers` are refused by `parse_system_config`, each naming the new
key, what the file wrote under it and `migrate` (`SYSTEM_TOML_REMOVED`, the
`system.toml` counterpart of the `removed` rows); `migrate` keeps the
rewrite. The model is schema 5: `execution.tasks`, a schema-4 model's
`execution.tiers` rewritten on read for good. The deprecated type aliases
and `SystemConfigToml::deprecated_spellings` are gone.

| # | Ours | Standard | Source | Notes |
|---|---|---|---|---|
| M1 | `mapper:` (`rate_monotonic`, `deadline_monotonic`, `manual`), `SchedMapper` | **priority assignment** policy | Liu & Layland; Audsley's optimal priority assignment | platform-file key `priority_assignment:` |
| M2 | `hazards.<h>.guards` | **monitored signals** (`monitors`) | AUTOSAR WdgM "supervised entity"; ISO 26262-6 monitoring | in formal methods a guard is a transition condition |
| M3 | `budget` for a time LIMIT (`scope-budget`, `fault-reaction-budget`, `ladder-rung-budget`, `budget-overflow`) | **deadline** / **latency bound** | — | `budget` + `budget_period` for CPU time per server period is the standard reservation term and stays; only the limit sense moves |
| M4 | `ingress: { rate_hz, burst }` | **arrival curve** (token bucket, σ/ρ) | network calculus (Cruz; Le Boudec & Thiran) | key `arrival_curve:` |
| M5 | `on_demand`, the `spontaneous` trigger | **aperiodic** | the periodic / sporadic / aperiodic task model | |
| M6 | play_launch's **carried** edges (beside **sampled**) | **triggered** (event-triggered) vs **sampled** (register, last-is-best) | cause-effect chain analysis | play_launch only: `chains.json`, `/api/chains`, the Chains page |
| M7 | **tier** (`TierDef`, `ResolvedTier`): one callback group's scheduling policy and placement | **task** (decided 2026-11-05; scheduling context was the alternative) | AUTOSAR OS / OSEK task; nano-ros's own retired `[[scheduling.contexts]]` | by far the costliest (§7) |

## 4. Kept

These are standard already, or our own where nothing standard fits:

- `ftti`, FDTI, FRTI (ISO 26262); `supervise`/supervision with alive,
  deadline and logical kinds, `mechanism` (AUTOSAR WdgM, ISO 26262-6);
  hazard; safe state; `on: omission | late` (AADL EMV2).
- `max_latency` with reaction and age semantics, data age (AUTOSAR TIMEX);
  `min_interarrival` (sporadic task model); jitter; deadline.
- `miss`, `max_consecutive`, `tolerate` (weakly-hard constraints); `budget`
  with `budget_period` (reservation servers); `reservations:`.
- mode, `fallback`, criticality, safety island, contract.
- `frontier` (play_launch's per-topic timestamp tracking): stream processing
  says watermark, but timely dataflow says frontier; not worth a rename.
- `acknowledged:`: certification practice says waiver or deviation (MISRA).
  Optional; a doc note is enough.
- fact / requirement / consequence: contract-based design (Benveniste et al.)
  says guarantee / assumption, and the correspondence is noted in
  `contract-theory.md` rather than renamed.

## 5. How a name moves

The same discipline as earlier key changes (phase 69, phase 96):

1. **Authored contracts.** The new key lands; the old one becomes a
   `deprecated alias` row in `field_table.rs` for ONE release (a warning
   naming the new key), then a parse error naming it. `rlm migrate` rewrites
   the old spelling.
2. **Models.** A saved `system_model.yaml` is read backward-compatibly for
   good: `SystemModel::from_yaml_str` rewrites the old field names before the
   typed read, as it already does for legacy `execution.tiers` keys. A new
   model writes only the new names, so an older reader refuses it, and the
   schema number moves.
3. **Rule ids.** A diagnostic id is an interface: `acknowledged: [{ rule }]`
   names it and `check --format json` emits it. A renamed rule keeps its old
   id as an alias for one release: an `acknowledged:` entry naming the old
   id still matches (with a deprecation warning), and `migrate` rewrites it.
   Without the alias every acknowledged finding would turn into `ack-stale`
   plus an unacknowledged warning on upgrade.
4. **Run artifacts.** `chains.json` and the web API change field names only
   with a version field present, so a reader can tell.
5. **nano-ros** mirrors the model types (`rungs[]`, tiers): each model rename
   ships with a nano-ros pull request bumping the rlm tag.

## 6. Retiring the old names

An alias left in place is a second spelling for good, and the vocabulary
then has two words for one thing, which is the condition this document
removes. So every rename has a scheduled end, in three releases:

| Release | Authored contract | Rule id in `acknowledged:` | Model on read | Code, docs, messages |
|---|---|---|---|---|
| **R** (rename) | new key; old key accepted with `deprecated-spelling` warning naming the new key | old id matches, with a warning | old field names rewritten | new names everywhere; old names only in the alias rows and the rewrite |
| **R+1** (refuse) | old key a parse error naming the new key (`removed` row, its own message) | old id is an error naming the new id | still rewritten | alias rows gone; the `removed` rows remain |
| **R+2** (purge) | unchanged from R+1 (the removal message stays: it is how a stale contract learns the new name) | the old-id message is dropped | still rewritten (permanent) | `check-terms` gate (§8) lists the old words as retired |

R+1 is not the same release as R for the same reason phase 68 recorded: a
deprecation window nobody installs warns nobody. R+1 ships only after a
release carrying R has been tagged and published to PyPI and nano-ros has
merged the matching pull request.

Steps per release, in order:

**At R**

1. Add the new key to `field_table.rs`; turn the old row into a `deprecated
   alias` naming it.
2. Teach `migrate` the rename (keys and `acknowledged:` rule ids).
3. Rename the types, model fields and rule ids; add the old-name rewrite to
   `SystemModel::from_yaml_str` and bump the schema number.
4. Run `migrate` over every fixture and contract in rlm, play_launch, the
   safety island and nano-ros; the gates must then report zero
   `deprecated-spelling` warnings, so no shipped file relies on the alias.
5. Regenerate `format-reference.md` and `key-inventory.md`; rewrite guides
   and design docs to the new words (history entries keep the words of
   their time, with a status line naming the rename).
6. nano-ros pull request: mirror types renamed, rlm tag bumped.

**At R+1**

1. Turn each `deprecated alias` row into a `removed` row with its own
   message naming the new key (the generic unknown-key check is only the
   backstop, as in phase 69).
2. Make an old rule id in `acknowledged:` an error naming the new id.
3. Delete alias handling from the parser, `migrate` keeps the rewrite.
4. Test: each old key and each old rule id is refused with a message
   naming its replacement (one negative case per name, so a removal that
   forgot one fails).

**At R+2**

1. Add the old words to the `check-terms` retired list; fix what it finds.
2. Delete the old-id message from the acknowledgement matcher.
3. Keep, permanently: the `removed` rows (a contract last edited before R
   must still be told the new name, not "unknown key"), the model rewrite
   (a saved model must still load), and `migrate`'s rewrite.

What is never retired is what the past needs: a saved model, an old contract
being told where its key went, and `migrate`. Everything else that carries
an old name is gone by R+2.

## 7. Cost

Word counts at 2026-11-03, `.rs`/`.md`/`.yaml`/`.py`/`.js`, build output
excluded; generic senses inflate some (`window`, `guard`):

| term | play_launch | rlm | nano-ros |
|---|---|---|---|
| rung | 591 | 167 | 2398 |
| ladder | 221 | 92 | 1396 |
| floor | 382 | 65 | 1582 |
| scope path | 274 | 201 | 24 |
| mapper | 514 | 268 | 92 |
| guard | 704 | 193 | 4255 |
| tier | 927 | 363 | 14554 |

S1–S5 are one rlm release. M7 (tier) is fourteen thousand mentions in
nano-ros for a clarity gain, and is decided separately: renaming it in the
docs' prose ("a tier, i.e. a scheduling context") may be enough.

## 8. Not decided

- The final word for each proposal (N0 in phase 98). The alternatives
  considered are given above where there were any.
- The shape of the `check-terms` gate that R+2 relies on: it fails on a
  retired word in user-facing text (docs, guides, diagnostic messages),
  allowlisted by file (history entries, the `removed` rows, the model
  rewrite, `migrate`), as `check-rt-docs` does for scheduling prose. Where it
  lives: one copy in each repository, or one script both run.
