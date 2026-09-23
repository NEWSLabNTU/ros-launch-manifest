# Documentation Index

What each document is, and a suggested reading order.

## Reading order

**New to the project?**

1. [slides.md](slides.md) — presentation deck: the problem, the manifest
   idea, the derived consequences, hazards and the checker in ~20
   slides. Fastest orientation.
2. [launch-manifest.md](launch-manifest.md) — the **specification**: the
   manifest format, worked Autoware examples, the format reference, and
   the validation-rule inventory. The normative document.
3. [format-reference.md](format-reference.md) — the **exhaustive key
   list**, generated from the field table and enforced by a test. Every key
   the parser accepts, per context; anything absent is a parse error.
4. [contract-theory.md](contract-theory.md) — the **theory**: why the
   composition rules are what they are (latency, drops, age, chains,
   burstiness), with the formal derivations.

**Working on or with the checker?**

5. [contract-verification.md](contract-verification.md) — the
   **implementation**: parsing with spans, the rule registry (**19**
   single-manifest rules, `check/src/rules/mod.rs::default_rules()`),
   emitters, and which checks run here vs in the consumer's cross-scope
   layer. `consistency` is a live rule id but NOT one of the 19 — it is
   what the consumer's cross-scope merge emits under.

**Working on scheduling?**

6. [scheduling.md](scheduling.md) — the **sched crate**: platform files,
   the `SchedMapper` registry (`manual`, `rate_monotonic`,
   `deadline_monotonic`, `chain_aware`), the platform-agnostic ranking
   core + POSIX realizer split, validation helpers, and the legacy
   `system.toml` bridge.

**Archaeology / rationale?**

7. [design-issues.md](design-issues.md) — the issue log: every design
   question raised against the spec, with resolution and rationale.
   Read when you want to know *why* the spec says what it says.

   It is **history, not current truth**. An entry keeps the vocabulary
   it was decided in — an entry about `chains:` still says `chains:`,
   years after the key became a parse error — because rewriting it
   would destroy the record. Each entry carries a status line saying
   whether the decision still holds, was superseded (naming the
   successor), or is open. For what the grammar IS today, read
   [format-reference.md](format-reference.md).

## Document roles

| Document | Role | Authority |
|----------|------|-----------|
| `format-reference.md` | Exhaustive key list, **generated** from `types/src/field_table.rs` | Normative for the grammar — never edited by hand |
| `launch-manifest.md` | Manifest format specification | Normative for the format |
| `contract-theory.md` | Formal foundations | Normative for composition math |
| `contract-verification.md` | Checker implementation | Descriptive (follows the code) |
| `scheduling.md` | Scheduling crate reference | Descriptive (follows the code) |
| `design-issues.md` | Decision log | Historical record — read the status lines, not the bodies |
| `slides.md` | Marp presentation deck | Informal overview |

Consumer-side documentation lives with the consumers:

- play_launch user guide: `docs/guide/rt-scheduling.md` (play_launch repo)
- Scheduling design of record: play_launch
  `docs/superpowers/specs/2026-07-16-rt-config-v2-design.md` (v2) and
  `2026-07-01-shared-scheduling-crate-design.md` (v1 / shared crate)
- nano-ros integration: nano-ros RFC-0050 / RFC-0052

The crate inventory (five workspace members: `types`, `check`, `sched`,
`model`, `derive`) is in the repository [README](../README.md).

`img/` holds diagram sources (Typst) and rendered assets;
`compile-images.sh` regenerates them.
