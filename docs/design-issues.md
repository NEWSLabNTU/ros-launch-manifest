# Design Issues

Open design questions for the manifest format, with proposed solutions.

## Resolved Issues (1–6, 8–16)

Issues 1–6, 8–16 were resolved in Phases 32–33 and are preserved in
git history. Briefly:

- **1–3**: Args, substitutions, conditions, service contracts, stale
  descriptions — Done (Phase 32, 32.1, 32.5)
- **4–5**: Missing `cli:` docs, endpoint name clarification — Done
  (Phase 32.1)
- **6**: Import topic mapping — Dropped (parent `topics:` handles this)
- **8**: External include file naming — Done (Phase 32.1)
- **9**: Optional refs `?` suffix — Superseded by #15
- **10**: Cross-scope service wiring — Superseded by #12
- **11**: Parser scope.args incomplete — Done (Phase 33.1)
- **12**: Unified scope interface — Done (Phase 33.2)
- **13**: Dangling entity checks — Done (Phase 33.4)
- **14**: Arg types + satisfiability — Done (Phase 33.3 + 33.5)
- **15**: `?` suffix breaks YAML parsers — Done (removed `?` entirely)
- **16**: Satisfiability skip state-only subs — Done

---

## 7. Global Topics Wiring Gap

### Problem

`global_topics:` declares type/QoS for absolute topics (`/tf`, `/clock`) but
doesn't wire them to node endpoints. A node can `sub: [tf]` but there's no
mechanism to say "this endpoint subscribes to the global `/tf`" without
creating a local topic entry.

### Proposed solution

Allow `global_topics:` names in topic `pub:`/`sub:` lists directly:

```yaml
global_topics:
  /tf: { type: tf2_msgs/msg/TFMessage }

nodes:
  ndt_scan_matcher:
    sub:
      tf:
        state: true

# No local topic entry needed — the checker sees "ndt_scan_matcher/tf"
# and recognizes it as a reference to the global /tf topic
```

Alternatively, keep it simple: global topics are just documentation. The
checker doesn't enforce them — they exist only for runtime graph auditing.
This avoids complexity in the static checker.

**Recommendation**: Keep it simple. Global topics are documentation only.
The checker ignores them. Runtime auditing (Phase 31.7) can use them.

---

## 17. Cross-Scope Service Wiring Has No Suppression Mechanism

### Problem

The unified scope interface (design issue #12) added scope-level `srv:`
and `cli:` groups. However, cross-scope service wiring still has no
enforcement path:

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

The `play_launch check` CLI runs all 14 validation rules and outputs
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

## 19. `pub:`/`sub:` Keyword Overloading

### Problem

`pub:` and `sub:` appear at three levels with different semantics:

1. **Node endpoints** — declares named ports (`pub: [cmd]`)
2. **Scope boundary** — declares which internal endpoints are exported
   (`pub: control_output: [controller/cmd]`)
3. **Topic wiring** — lists which endpoints publish/subscribe on this
   topic (`pub: [controller/cmd]`)

The spec acknowledges this (launch-manifest.md §Concepts, lines 120–123)
but treats it as a documentation problem. Three overloaded meanings for
the same keyword cause persistent authoring errors — users must infer
the level from context.

### Options

| Option | Pros | Cons |
|--------|------|------|
| A. Distinct keywords | `exports:`/`imports:` for scope, `publishers:`/`subscribers:` in topics | Breaking change to all manifests |
| B. Syntactic distinction | Scope uses `export_pub:`, topics use `topic_pub:` | Verbose |
| C. Accept it, improve docs | No format change | Ambiguity persists |

---

## 20. Scope Interface Duplicates Topic Wiring

### Problem

Writing a child manifest requires declaring endpoints twice: once in
`topics:` (wiring) and again in the top-level scope interface (`pub:`/
`sub:`). In the worked example, `tracking.yaml` declares
`multi_object_tracker/tracked` in both `topics.tracked_objects.pub` and
`pub.objects`.

The scope boundary could be inferred from which internal endpoints the
parent references. Manual duplication is a consistency burden — if a
topic wiring changes but the scope export doesn't, the manifest silently
drifts.

### Options

| Option | Pros | Cons |
|--------|------|------|
| A. Infer exports from parent refs | Eliminates duplication | Implicit, harder to read the child in isolation |
| B. Lint rule for drift | Catches mismatch | Still requires duplication |
| C. Accept it | Explicit is better than implicit | Maintenance burden at scale |

---

## 21. Topic Keys Indistinguishable from ROS Topic Names

### Problem

Topic keys like `tracked_objects` or `control_cmd` are manifest-local
identifiers, not ROS topic names. Nothing in the syntax distinguishes
them. Users will confuse manifest topic keys with actual ROS names,
especially since the doc calls topics "first-class."

The mapping from manifest topic key → actual ROS topic name (determined
by remaps and namespace resolution) is never demonstrated in any example.
A user looking at a running system with `ros2 topic list` output has no
guidance on how to map those names into manifest declarations.

---

## 22. Drop Composition Assumes Independence Despite Burstiness

### Problem

The drop algebra (contract-theory.md Appendix A) is built on independent
Bernoulli drops. The burstiness section then admits real drops are
correlated (DDS queue overflow, network congestion), undermining the
static guarantees. The static checker will pass manifests whose drop
guarantees are meaningless under bursty conditions.

The doc hand-waves to "runtime monitor detects this gap," but the static
check gives false confidence.

### Options

| Option                                     | Pros                     | Cons                                                                     |
|--------------------------------------------|--------------------------|--------------------------------------------------------------------------|
| A. Downgrade static drop checks to warning | Honest about limitations | Users may ignore warnings                                                |
| B. Use Gilbert-Elliott burst model         | Accurate                 | Requires two parameters (good/bad state probabilities), harder to author |
| C. Keep as-is, document limitation         | No code change           | Static guarantees misleading                                             |

---

## 23. Age Verification Effectively Unimplementable Statically

### Problem

The age check requires tracing the full causal chain and summing every
node's `max_latency_ms`. Any node without a declared latency creates a
"gap" making the check incomplete (contract-theory.md §Age: Chain-Based
Verification). In Autoware (100+ nodes), most nodes won't have latency
budgets initially. The age check will almost always produce INFO
"incomplete" messages.

The "top-down workflow" explicitly says full node-level decomposition
isn't required — but the age check needs it. These two design goals
are in tension.

---

## ~~24. Transport Latency Unmodeled~~ — Done (Option A)

Added `max_transport_ms` (optional) to the topic format. Worst-case
transport time per topic hop. Topics without it contribute 0 — their
transport is absorbed into the scope residual. Updated:
- launch-manifest.md: topic example, field table, Latency vs Age section
- contract-theory.md: notation, series section, sum check sketch,
  latency example (P's sum now includes T1's 2ms transport)

---

## ~~25. Periodic Node Latency Formula Double-Counts Upstream~~ — Done

Fixed: removed `L_node(upstream)` from the periodic worst-case and
best-case formulas in contract-theory.md. The periodic node's
contribution is `P + J + L_node(periodic)` — upstream processing is
already counted by the upstream node's own budget in series composition.
Updated the composition summary table to match.

---

## ~~26. Worked Example Indirection Not Explained~~ — Done

Added a naming convention callout before the perception manifest example
in launch-manifest.md, explaining that `tracking/objects` means "the
`objects` export of the `tracking` include." Also added inline comments
on the topic wiring lines.

---

## ~~27. No Example Shows Actual ROS Topic Name Mapping~~ — Done

Added "Mapping from ROS Topics to Manifest Declarations" subsection to
the worked example in launch-manifest.md. Shows the full mapping from
`/perception/tracking/tracked_objects` through each manifest layer
(endpoint → internal topic → child export → parent wiring).

---

## ~~28. `args:` Position Inconsistent with `version:`~~ — Done

Fixed: control example in launch-manifest.md now has `version: 1` before
`args:`, consistent with the format reference.

---

## 29. `exclude_patterns` Override Behavior Undocumented

### Problem

The metadata table says the default for `exclude_patterns` is `/rosout`,
`/parameter_events`. It is unclear whether:

- `exclude_patterns: []` suppresses the defaults (includes those topics)
- The defaults are always prepended regardless of user declaration
- There is no way to include `/rosout` in the manifest

### Fix

Document the behavior explicitly: does user declaration replace or
extend the defaults?

---

## 30. No Example Error Messages for Validation Rules

### Problem

The 14 validation rules are listed with severity but no example
diagnostics. Users cannot predict what a failure looks like or how to
fix it.

### Fix

Add one example error message per rule, or at least for the most common
rules (`wiring`, `rate-hierarchy`, `budget-overflow`, `dangling-entity`).

---

## 31. `correlation: latest` Output Timestamp Unspecified

### Problem

`correlation: timestamp` specifies "the output stamp is the oldest input
stamp" (preserving the earliest provenance). `correlation: latest` has
no specification for the output stamp. Possible behaviors:

- **Latest input stamp** — breaks age provenance differently
- **Current time** — resets the timestamp chain (like periodic nodes)
- **Oldest input stamp** — same as `timestamp` mode

This matters for downstream age checking. Without a spec, the checker
cannot reason about age through a `correlation: latest` node.

### Fix

Specify the output timestamp for `correlation: latest`. Likely answer:
it should reset the chain (output stamp = current time), since the node
is explicitly not doing timestamp-based correlation.

---

## 32. Capture Mode Buried in Theory Appendix

### Problem

`--save-manifest-dir` (contract-theory.md Appendix C) bootstraps
manifests from runtime measurements — arguably the most important UX
feature for adoption. Users reading only `launch-manifest.md` won't
know it exists. The feature is mentioned only in an appendix of the
formal theory document.

### Fix

Add a prominent section to `launch-manifest.md` (e.g., "Getting Started:
Generating Manifests from a Running System") that describes capture mode
and links to the appendix for statistical details.

---

## Summary

| #  | Issue                              | Type                    | Effort  | Status                           |
|----|------------------------------------|-------------------------|---------|----------------------------------|
| 7  | Global topics wiring               | Design decision         | Small   | Kept as documentation only       |
| 17 | Cross-scope suppression            | UX / CLI                | Small   | Open — no way to suppress expected cross-scope warnings |
| 18 | Per-rule CLI filter                | UX / CLI                | Small   | Open — `--rule <ID>` flag for focused output |
| 19 | `pub:`/`sub:` keyword overloading  | Format design           | Large   | Open |
| 20 | Scope interface duplicates wiring  | Format design           | Medium  | Open |
| 21 | Topic keys vs ROS topic names      | UX / naming             | Small   | Open |
| 22 | Drop independence assumption       | Theory / static check   | Medium  | Open |
| 23 | Age check needs full decomposition | Theory / static check   | —       | Open — inherent tension with top-down workflow |
| 24 | Transport latency unmodeled        | Format design           | Small   | Done — `max_transport_ms` on topics |
| 25 | Periodic formula double-counts     | Theory / doc fix        | Trivial | Done — upstream removed from formula |
| 26 | Worked example indirection         | Doc fix                 | Trivial | Done — `child/export` convention callout added |
| 27 | No ROS topic name mapping example  | Doc fix                 | Small   | Done — "Mapping from ROS Topics" subsection added |
| 28 | `args:` position inconsistent      | Doc fix                 | Trivial | Done — `version:` now first in control example |
| 29 | `exclude_patterns` override        | Doc fix                 | Trivial | Open |
| 30 | No example error messages          | Doc fix                 | Small   | Open |
| 31 | `correlation: latest` stamp        | Spec gap                | Small   | Open |
| 32 | Capture mode buried in appendix    | Doc fix                 | Small   | Open |
