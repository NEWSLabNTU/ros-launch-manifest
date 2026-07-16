//! Rule: sibling `state:` tagging consistency within a single node.
//!
//! Motivated by a real Autoware contract bug (Phase 42 study, Q1/Q2):
//! `simple_planning_simulator` tags its `gear_cmd`, `turn_indicators_cmd`,
//! and `hazard_lights_cmd` subscribers `state: true` (all populated by the
//! same lambda-store-then-poll pattern), but leaves its behaviorally
//! identical `control_cmd` subscriber untagged — an inconsistent
//! application of the existing `state:` boolean within one contract file,
//! not a missing schema feature.
//!
//! Heuristic (intentionally narrow, to avoid false positives on nodes that
//! legitimately mix causal and state inputs): a node whose subscribers
//! include at least 2 tagged `state: true` (an established "this node
//! polls its inputs" pattern) AND **exactly 1** subscriber that is neither
//! tagged `state: true` nor referenced as an `input` in any of the node's
//! own `paths:` entries (so it isn't otherwise accounted for as a
//! deliberately-causal path member) is flagged as a likely missed
//! `state:` tag on that lone unaccounted subscriber.
//!
//! The "exactly 1" restriction (rather than "at least 1") matters in
//! practice: nodes with 2+ unaccounted-for subscribers are much more
//! likely to be a genuine hybrid design — e.g. Autoware's
//! `vehicle_cmd_gate`, which is event-driven for several independent
//! control-cmd sources (auto/remote/emergency) while polling everything
//! else — than a copy-paste oversight. A *single* stray sub among many
//! consistently-tagged siblings, with no other causal role, is the
//! specific shape of the motivating bug: every sibling uses the same
//! lambda-store-then-poll pattern, and one was simply missed.

use super::ValidationRule;
use crate::{CheckContext, graph::DataflowGraph};
use ros_launch_manifest_types::Manifest;
use std::collections::HashSet;

pub struct StateConsistencyRule;

impl ValidationRule for StateConsistencyRule {
    fn id(&self) -> &str {
        "state-consistency"
    }

    fn check(&self, manifest: &Manifest, _graph: &DataflowGraph, ctx: &mut CheckContext) {
        for (node_name, node) in &manifest.nodes {
            let state_count = node
                .subscribers
                .values()
                .filter(|props| props.state.unwrap_or(false))
                .count();

            if state_count < 2 {
                continue;
            }

            // Endpoint names referenced as `input` in any of this node's paths.
            let path_inputs: HashSet<&str> = node
                .paths
                .values()
                .flat_map(|path| path.input.iter().map(String::as_str))
                .collect();

            let unaccounted: Vec<&str> = node
                .subscribers
                .iter()
                .filter(|(ep_name, props)| {
                    !props.state.unwrap_or(false) && !path_inputs.contains(ep_name.as_str())
                })
                .map(|(ep_name, _)| ep_name.as_str())
                .collect();

            // Only flag a single lone stray sub — 2+ unaccounted subs looks
            // like a deliberate hybrid design (multiple independent causal
            // inputs), not a one-off missed tag. See module docs.
            let [ep_name] = unaccounted.as_slice() else {
                continue;
            };

            ctx.warning(
                self.id(),
                &format!("nodes.{node_name}.sub.{ep_name}"),
                format!(
                    "node '{node_name}' has {state_count} sibling subscriber(s) tagged \
                     'state: true', but '{ep_name}' is neither tagged 'state: true' nor \
                     referenced as an 'input' in any of this node's 'paths:' entries — \
                     likely a missed 'state:' tag (see the simple_planning_simulator \
                     control_cmd bug in Phase 42's study for the motivating pattern)."
                ),
            );
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

    /// Positive case: the exact motivating pattern — 2+ sibling subs tagged
    /// `state: true`, and one behaviorally-identical sub left untagged and
    /// not referenced by any node path. Should warn on the untagged sub.
    #[test]
    fn untagged_sibling_of_two_state_subs_warns() {
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

    /// Negative case: fewer than 2 state-tagged siblings — heuristic doesn't
    /// fire (avoid false positives on nodes that only have one state input).
    #[test]
    fn single_state_sub_does_not_warn() {
        let yaml = r#"
version: 1
nodes:
  n:
    sub:
      a:
        state: true
      b: {}
"#;
        assert!(run(yaml).is_empty());
    }

    /// Negative case: the untagged sub is accounted for via a node path's
    /// `input:` list — a deliberate causal input, not a missed state tag.
    #[test]
    fn untagged_sub_referenced_by_path_does_not_warn() {
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

    /// Negative case: 2+ unaccounted subs — a hybrid-design node (e.g.
    /// `vehicle_cmd_gate`'s multiple independent event-driven control-cmd
    /// sources) rather than a lone missed tag. Must not warn.
    #[test]
    fn multiple_unaccounted_subs_does_not_warn() {
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
        assert!(run(yaml).is_empty());
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
}
