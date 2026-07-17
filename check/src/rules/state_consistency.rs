//! Rule: sibling `state:` tagging consistency within a single node
//! (Vocabulary v2, Phase 44.1 §5 — generalized).
//!
//! Motivated by a real Autoware contract bug (Phase 42 study, Q1/Q2):
//! `simple_planning_simulator` tags its `gear_cmd`, `turn_indicators_cmd`,
//! and `hazard_lights_cmd` subscribers `state: true` (all populated by the
//! same lambda-store-then-poll pattern), but leaves its behaviorally
//! identical `control_cmd` subscriber untagged — an inconsistent
//! application of the existing `state:` boolean within one contract file,
//! not a missing schema feature.
//!
//! Generalized rule (Vocabulary v2 spec §5, supersedes the original
//! "exactly one unaccounted stray sub" heuristic): a subscriber endpoint
//! is flagged when it is **neither**
//! - tagged `state: true`, **nor**
//! - referenced by any of the node's paths' [`EffectiveTrigger::Input`]
//!   endpoint list (the *effective* trigger — explicit `trigger: {input:
//!   [...]}` or the legacy `input:` derivation both count).
//!
//! Unlike the original heuristic, this fires for every unaccounted sub on
//! a node (not just a lone straggler among ≥2 state-tagged siblings) — the
//! coherence rule from the vocabulary v2 design: "a `state:` sub never
//! appears in any `trigger.input`" is now checked in the other direction
//! too (every non-state sub should appear in some path's input, or be
//! explicitly `state:`).

use super::ValidationRule;
use crate::{CheckContext, graph::DataflowGraph};
use ros_launch_manifest_types::{EffectiveTrigger, Manifest};
use std::collections::HashSet;

pub struct StateConsistencyRule;

impl ValidationRule for StateConsistencyRule {
    fn id(&self) -> &str {
        "state-consistency"
    }

    fn check(&self, manifest: &Manifest, _graph: &DataflowGraph, ctx: &mut CheckContext) {
        for (node_name, node) in &manifest.nodes {
            if node.subscribers.is_empty() {
                continue;
            }

            // Endpoint names referenced as the effective input trigger of
            // any of this node's paths (explicit trigger.input OR legacy
            // input: derivation — both count via effective_trigger()).
            let path_inputs: HashSet<String> = node
                .paths
                .values()
                .filter_map(|path| match path.effective_trigger() {
                    EffectiveTrigger::Input(eps) => Some(eps),
                    _ => None,
                })
                .flatten()
                .collect();

            for (ep_name, props) in &node.subscribers {
                if props.state.unwrap_or(false) {
                    continue;
                }
                if path_inputs.contains(ep_name.as_str()) {
                    continue;
                }
                ctx.warning(
                    self.id(),
                    &format!("nodes.{node_name}.sub.{ep_name}"),
                    format!(
                        "node '{node_name}' subscriber '{ep_name}' is neither tagged \
                         'state: true' nor referenced as the effective input trigger of any \
                         of this node's 'paths:' entries — likely a missed 'state:' tag or a \
                         missing causal path (see the simple_planning_simulator control_cmd \
                         bug in Phase 42's study for the motivating pattern)."
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CheckContext;
    use ros_launch_manifest_types::parse::parse_manifest_str;

    fn run(yaml: &str) -> Vec<crate::Diagnostic> {
        let manifest = parse_manifest_str(yaml).unwrap();
        let graph = DataflowGraph::build(&manifest);
        let mut ctx = CheckContext::new();
        StateConsistencyRule.check(&manifest, &graph, &mut ctx);
        ctx.diagnostics
    }

    /// Positive case: the exact motivating pattern — sibling subs tagged
    /// `state: true`, and one behaviorally-identical sub left untagged and
    /// not referenced by any node path. Should warn on the untagged sub.
    #[test]
    fn untagged_sibling_of_state_subs_warns() {
        let yaml = r#"
version: 1
nodes:
  simple_planning_simulator:
    sub:
      gear_cmd:
        state: true
      turn_indicators_cmd:
        state: true
      control_cmd: {}
"#;
        let warnings: Vec<_> = run(yaml)
            .into_iter()
            .filter(|d| d.severity == crate::Severity::Warning)
            .collect();
        assert_eq!(warnings.len(), 1, "got: {warnings:?}");
        assert!(warnings[0].path.ends_with("control_cmd"));
        assert!(warnings[0].message.contains("simple_planning_simulator"));
    }

    /// Negative case: single state-tagged sub, no other subs — nothing to
    /// flag.
    #[test]
    fn single_state_sub_does_not_warn() {
        let yaml = r#"
version: 1
nodes:
  n:
    sub:
      a:
        state: true
"#;
        assert!(run(yaml).is_empty());
    }

    /// Negative case: the untagged sub is accounted for via a node path's
    /// explicit `trigger: { input: [...] }` — a deliberate causal input,
    /// not a missed state tag.
    #[test]
    fn untagged_sub_referenced_by_explicit_trigger_input_does_not_warn() {
        let yaml = r#"
version: 1
nodes:
  n:
    sub:
      a:
        state: true
      b:
        state: true
      c: {}
    pub:
      out: {}
    paths:
      main:
        trigger: { input: [c] }
        output: [out]
"#;
        assert!(run(yaml).is_empty());
    }

    /// Negative case: the untagged sub is accounted for via the legacy
    /// `input:` list (no explicit trigger — effective_trigger() still
    /// derives Input).
    #[test]
    fn untagged_sub_referenced_by_legacy_input_does_not_warn() {
        let yaml = r#"
version: 1
nodes:
  n:
    sub:
      a:
        state: true
      b:
        state: true
      c: {}
    pub:
      out: {}
    paths:
      main:
        input: [c]
        output: [out]
"#;
        assert!(run(yaml).is_empty());
    }

    /// Generalized behavior (differs from the old "exactly one" heuristic):
    /// a hybrid-design node with multiple unaccounted subs now warns on
    /// EACH unaccounted sub, not zero.
    #[test]
    fn multiple_unaccounted_subs_each_warn() {
        let yaml = r#"
version: 1
nodes:
  n:
    sub:
      a:
        state: true
      b:
        state: true
      c: {}
      d: {}
"#;
        let warnings: Vec<_> = run(yaml)
            .into_iter()
            .filter(|d| d.severity == crate::Severity::Warning)
            .collect();
        assert_eq!(warnings.len(), 2, "got: {warnings:?}");
        assert!(warnings.iter().any(|w| w.path.ends_with("c")));
        assert!(warnings.iter().any(|w| w.path.ends_with("d")));
    }

    /// Negative case: all subs consistently tagged `state: true` — nothing
    /// to flag.
    #[test]
    fn all_subs_tagged_state_does_not_warn() {
        let yaml = r#"
version: 1
nodes:
  n:
    sub:
      a:
        state: true
      b:
        state: true
      c:
        state: true
"#;
        assert!(run(yaml).is_empty());
    }

    /// Generalized behavior: even a lone unaccounted sub with NO other
    /// state-tagged siblings now warns (old heuristic required >= 2 state
    /// siblings before firing at all).
    #[test]
    fn lone_unaccounted_sub_with_no_state_siblings_warns() {
        let yaml = r#"
version: 1
nodes:
  n:
    sub:
      a: {}
"#;
        let warnings = run(yaml);
        assert_eq!(warnings.len(), 1, "got: {warnings:?}");
        assert!(warnings[0].path.ends_with("a"));
    }
}
