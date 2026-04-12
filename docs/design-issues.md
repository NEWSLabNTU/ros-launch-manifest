# Design Issues

Open design questions for the manifest format, with proposed solutions.

## Resolved Issues

Issues resolved in prior phases, preserved in git history:

- **1–6, 8–16**: Args, substitutions, conditions, service contracts,
  doc fixes, parser bugs, unified scope interface, dangling entity
  checks, arg types + satisfiability, YAML `?` suffix — Done
  (Phases 32–33)
- **7, 19, 20, 21, 26, 27**: Global topics, `pub:`/`sub:` overloading,
  scope interface duplication, topic key naming, worked example
  indirection, ROS topic mapping — Superseded by #33
- **24**: Transport latency — Done (`max_transport_ms` on topics)
- **25**: Periodic formula double-counts — Done (upstream removed)
- **28**: `args:` position — Done (`version:` first in examples)
- **17**: Cross-scope service suppression — Resolved by #41
- **33**: Topic keys as ROS names — Done (docs). Resolves #7, #19,
  #20, #21, #26, #27. Code pending
- **34**: Scope paths use topic names — Done (Option A)
- **35**: Parent manifest purpose — Done (E2E contracts via scope paths)
- **36**: Rate check cross-scope merge — Done (note added)
- **37**: Absolute names verbose — Accepted (tooling helps)
- **38**: Relative vs absolute guidance — Done (added to Topics section)
- **39**: `type:` field table — Done (consistent `yes` / `Error`)
- **40**: `global_topics:` note — Done (removed)
- **41**: Services follow topic pattern — Done (ROS names, cross-scope merge)
- **30**: Example error messages — Done (7 rules illustrated)
- **32**: Capture mode — Done (section added to launch-manifest.md)
- **29**: `exclude_patterns` — Done (replaces defaults, `[]` includes all)
- **31**: `correlation: latest` stamp — Done (primary input's stamp)
- **23**: Age on subscriber endpoints — Done (moved from paths)
- **22**: Drop composition — Done (static sanity only, composition is runtime)

---

## 17. Cross-Scope Service Wiring Has No Suppression Mechanism

### Problem

Cross-scope service wiring has no enforcement path:

1. `mrm_handler` declares `cli:` with 3 cross-scope service clients
2. `mrm_comfortable_stop_operator` declares `srv:` with 1 server
3. No parent manifest exists to wire them in a `services:` section
4. The `service-wiring` rule warns about orphan `cli:` endpoints
5. There is no way to mark these as "expected cross-scope" to suppress

This creates **permanent noise** in check output — 3+ warnings per run
that can never be resolved without a parent manifest.

### Options

| Option                        | Effort | Description                                              |
|-------------------------------|--------|----------------------------------------------------------|
| A. `# nolint: service-wiring` | Small  | Inline suppression comment                               |
| B. `--suppress` CLI flag      | Small  | Suppress specific rules/paths                            |
| C. Cross-manifest wiring      | Large  | Checker loads multiple manifests and wires across scopes |
| D. Accept the noise           | None   | Document as known limitation                             |

**Recommendation**: Option A or B. A lightweight suppression mechanism
would benefit other expected-warning scenarios too (e.g., unwired scope
interface endpoints).

---

## 18. CLI Should Support Per-Rule Filtering

### Problem

The `play_launch check` CLI runs all 15 validation rules and outputs
all diagnostics. Users interested in a specific rule (e.g., satisfiability
results only) must grep the output. The `just check-sat` recipe in
autoware-contract does exactly this — a shell grep wrapper.

### Proposed Solution

Add `--rule <RULE_ID>` filter to the CLI:

```bash
# Show only satisfiability results
play_launch check --rule satisfiability --manifest-dir . autoware_launch planning_simulator.launch.xml

# Show only errors from specific rules
play_launch check --rule dangling-entity --rule optional-ref --manifest-dir . ...
```

This is a small CLI change — filter `CheckResult.diagnostics` by
`rule_id` before rendering.

---

## ~~22. Drop Composition Assumes Independence Despite Burstiness~~ — Done

Resolved: drop composition moved from static checker to **runtime
monitoring only**. The static checker now validates local consistency
only (values in range, scope drop rate not tighter than any topic's,
effective delivery meets subscriber demand). Chain composition and
`max_consecutive` enforcement are runtime concerns — they depend on
actual transport conditions. Appendix A retained as runtime monitoring
theory.

---

## ~~23. Age Verification Effectively Unimplementable Statically~~ — Done

Resolved by moving `max_age_ms` from scope paths to **subscriber
endpoints**. Age is now a data freshness constraint at the point of
consumption (`now - header.stamp` at `rcl_take`), checked at runtime
by the interception layer. No chain tracing needed. Static checking
verifies local consistency (age budget vs known latency budget) but
doesn't attempt a full chain proof.

---

## ~~29. `exclude_patterns` Override Behavior Undocumented~~ — Done

Documented: user declaration **replaces** defaults. `exclude_patterns: []`
includes all topics.

---

## ~~30. No Example Error Messages for Validation Rules~~ — Done

Added example diagnostics block to the Static Validation section
covering 7 rules: `endpoint-unique`, `wiring`, `rate-hierarchy`,
`budget-overflow`, `dangling-entity`, `consistency`, `satisfiability`.

---

## ~~31. `correlation: latest` Output Timestamp Unspecified~~ — Done

Specified: `correlation: latest` output stamp = **primary (first listed)
input's stamp**. Based on analysis of 9 Autoware fusion nodes — 7 of 9
propagate the primary input's timestamp. Secondary inputs enrich the
data but don't determine the timestamp. Age follows the primary branch
only. Updated launch-manifest.md and contract-theory.md (parallel
composition section + summary table).

---

## ~~32. Capture Mode Buried in Theory Appendix~~ — Done

Added "Generating Manifests from a Running System" section to
launch-manifest.md with `--save-manifest-dir` usage, what it generates,
and a link to the statistical derivation in contract-theory.md Appendix C.

---

## 33. Topic Keys as ROS Topic Names

Resolved (docs). Resolves #7, #19, #20, #21, #26, #27. Code pending.

### Design

Topic keys are ROS topic names — relative (resolved by the checker
using the scope's namespace from the launch tree) or absolute (`/`).
The same topic can appear in multiple manifests; `type:` must agree,
`pub:`/`sub:` are merged. Scope interface removed. `global_topics:`
removed.

### Data (Autoware 1.5.0, 182 topics)

| Category             | Count     | Key format |
|----------------------|-----------|------------|
| Within one subsystem | 147 (81%) | Relative   |
| Cross-subsystem      | 35 (19%)  | Absolute   |

327 topic×scope pairs: 71% sub-only, 26% pub-only, 3% both.
Only 4 topics have publishers in multiple scopes.

### Consistency Rule

- `type:` — required in every declaration, must match across scopes
- `rate_hz:`, `qos:` — must agree if declared in multiple scopes
- `pub:`, `sub:` — merged across scopes by the checker
- New rule: `topic-consistency` validates agreement

### Resolved Questions

1. **Duplicate declarations** → consistency rule (agree, not SSoT)
2. **Deep endpoint refs** → scope-local only (each scope refs own nodes)
3. **Discoverability** → checker merges by resolved name
4. **Namespace** → no `ns:` field; checker uses scope table at check time
5. **Standalone launch** → each manifest self-contained with `type:`

---

## ~~34. Scope-Level Paths Contradict Scope-Local Refs~~ — Done (Option A)

Scope paths now use **topic names** as input/output (not node/endpoint
refs). The checker traces dataflow between the named topics, considering
only nodes within the declaring scope's subtree. When parent and child
declare paths with the same resolved (input, output) topics,
`budget-overflow` checks child budget ≤ parent budget.

---

## ~~35. Parent Manifest Has No Purpose Without Scope Paths~~ — Done

Resolved by #34. Parent manifests declare E2E contracts (scope paths
with topic-name input/output) over child subtrees. The parent also
declares `includes:` for the manifest tree structure.

---

## ~~37. Absolute Topic Names Verbose for Sibling Scopes~~ — Accepted

Accept the verbosity (Option A/D). 81% of topics use short relative
keys. The 19% cross-subsystem topics use absolute names that match
`ros2 topic list` output. Capture mode generates the names
automatically. No format complexity needed.

---

## ~~41. Services Don't Follow the Topic Naming Pattern~~ — Done

Services and actions now follow the same pattern as topics: ROS names
as keys (relative or absolute), cross-scope merge, consistency rule.
`service-wiring` checks the merged service across the tree. Also
resolves #17 — cross-scope services are wired by name matching, no
orphan warnings.

---

## ~~17. Cross-Scope Service Wiring Has No Suppression Mechanism~~ — Resolved by #41

Cross-scope services are now wired by ROS name matching across the
manifest tree, same as topics. No orphan `cli:` warnings — the
`service-wiring` rule checks the merged service after cross-scope merge.

---

## Summary

| #  | Issue                               | Type                  | Effort  | Status                    |
|----|-------------------------------------|-----------------------|---------|---------------------------|
| 18 | Per-rule CLI filter                 | UX / CLI              | Small   | Open                      |
| 22 | Drop independence assumption        | Theory / static check | Medium  | Done — runtime only       |
| 23 | Age check needs full decomposition  | Theory / static check | —       | Done — age on subscribers |
| 29 | `exclude_patterns` override         | Doc fix               | Trivial | Done                      |
| 31 | `correlation: latest` stamp         | Spec gap              | Small   | Done                      |
| 33 | Topic keys as ROS names             | Format redesign       | Large   | Done (docs), code pending |
