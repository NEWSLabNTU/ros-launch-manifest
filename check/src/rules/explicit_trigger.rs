//! Rule: nudge every path toward an explicit `trigger:` declaration
//! (Vocabulary v2, Phase 44.1 §5).
//!
//! Fires (info severity — authoring hygiene, not a defect) whenever a
//! path has no explicit `trigger:` field: the path is `Unclassified`
//! (the legacy `input:` list that used to derive an input trigger was
//! removed in v0.3.15).
//!
//! A timer whose `rate_hz` is a reference not evaluated at check time
//! (`rate_hz: $(param rate) Hz` with no launch tree) IS an explicit timer
//! trigger: only its period is unknown (`PathDecl::timer_rate_unread`), so
//! this rule stays quiet on it (rlm#9). A rule that needs the period reports
//! the unknown value where it would have used it (`queue-drain-rate`).

use super::ValidationRule;
use crate::{CheckContext, Severity, graph::DataflowGraph};
use ros_launch_manifest_types::Manifest;

pub struct ExplicitTriggerRule;

impl ValidationRule for ExplicitTriggerRule {
    fn id(&self) -> &str {
        "explicit-trigger"
    }

    fn check(&self, manifest: &Manifest, _graph: &DataflowGraph, ctx: &mut CheckContext) {
        for (node_name, node) in &manifest.nodes {
            for (path_name, path) in &node.paths {
                if path.trigger.is_none() && path.timer_rate_unread.is_none() {
                    ctx.emit(
                        self.id(),
                        Severity::Info,
                        &format!("nodes.{node_name}.paths.{path_name}"),
                        format!(
                            "node '{node_name}' path '{path_name}' has no explicit 'trigger:' \
                             — add one of timer/input/once/aperiodic (Vocabulary v2)"
                        ),
                    );
                }
            }
        }
        // Scope-level (top-of-manifest) paths carry the same PathDecl shape.
        for (path_name, path) in &manifest.paths {
            if path.trigger.is_none() && path.timer_rate_unread.is_none() {
                ctx.emit(
                    self.id(),
                    Severity::Info,
                    &format!("paths.{path_name}"),
                    format!(
                        "event chain '{path_name}' has no explicit 'trigger:' — add one of \
                         timer/input/once/aperiodic (Vocabulary v2)"
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
        ExplicitTriggerRule.check(&manifest, &graph, &mut ctx);
        ctx.diagnostics
    }

    #[test]
    fn path_without_trigger_infos() {
        let yaml = r#"
version: 1
nodes:
  n:
    sub:
      a: {}
    pub:
      out: {}
    paths:
      main:
        output: [out]
"#;
        let diags = run(yaml);
        assert_eq!(diags.len(), 1, "got: {diags:?}");
        assert_eq!(diags[0].severity, Severity::Info);
        assert!(diags[0].path.ends_with("main"));
    }

    #[test]
    fn path_with_explicit_trigger_does_not_fire() {
        let yaml = r#"
version: 1
nodes:
  n:
    sub:
      a: {}
    pub:
      out: {}
    paths:
      main:
        trigger: { input: [a] }
        output: [out]
"#;
        assert!(run(yaml).is_empty());
    }

    #[test]
    fn unclassified_path_also_infos() {
        let yaml = r#"
version: 1
nodes:
  n:
    pub:
      out: {}
    paths:
      main:
        output: [out]
"#;
        let diags = run(yaml);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Info);
    }

    #[test]
    fn scope_path_without_trigger_infos() {
        let yaml = r#"
version: 1
paths:
  e2e:
    output: [b]
"#;
        let diags = run(yaml);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].path.starts_with("paths."));
    }

    #[test]
    fn timer_with_unread_param_rate_is_explicit() {
        // rlm#9: an unevaluated `$(param ...) Hz` rate is still a timer.
        let yaml = r#"
version: 1
nodes:
  n:
    params:
      rate: { type: double }
    pub:
      out: {}
    paths:
      tick:
        trigger: { timer: { rate_hz: $(param rate) Hz } }
        output: [out]
"#;
        let manifest = parse_manifest_str(yaml).unwrap();
        let path = &manifest.nodes["n"].paths["tick"];
        assert!(path.trigger.is_none(), "no guessed rate");
        assert_eq!(path.timer_rate_unread.as_deref(), Some("$(param rate) Hz"));
        assert!(run(yaml).is_empty(), "got: {:?}", run(yaml));
    }

    #[test]
    fn timer_with_unread_map_form_rate_is_explicit() {
        let yaml = r#"
version: 1
nodes:
  n:
    params:
      rate: { type: double }
    pub:
      out: {}
    paths:
      tick:
        trigger: { timer: { rate_hz: { value: $(param rate) Hz } } }
        output: [out]
"#;
        assert!(run(yaml).is_empty(), "got: {:?}", run(yaml));
    }
}
