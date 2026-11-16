//! Rule: QoS field value validity (topic-level and per-endpoint).
//!
//! Validates that declared QoS field values come from the allowed set
//! (e.g. `reliability` ∈ {reliable, best_effort}). Applies to both
//! topic-level `qos:` blocks and per-endpoint overrides on publishers
//! and subscribers. Pub/sub compatibility is handled by the separate
//! `qos-match` rule.

use super::ValidationRule;
use crate::{CheckContext, graph::DataflowGraph};
use ros_launch_manifest_types::{Manifest, QosDecl};
use std::collections::BTreeMap;

pub struct QosCompatRule;

impl ValidationRule for QosCompatRule {
    fn id(&self) -> &str {
        "qos-compat"
    }

    fn check(&self, manifest: &Manifest, _graph: &DataflowGraph, ctx: &mut CheckContext) {
        // Topic-level QoS blocks.
        for (topic_name, topic) in &manifest.topics {
            if let Some(qos) = &topic.qos {
                check_qos_values(self.id(), &format!("topics.{topic_name}.qos"), qos, ctx);
            }
        }

        // Per-endpoint QoS overrides on every node's pub/sub/cli endpoints.
        // `EndpointProps` applies to every endpoint kind that carries it,
        // and `qos:` on a `cli` is meaningful for service-client QoS profiles.
        for (node_name, node) in &manifest.nodes {
            check_node_endpoints(self.id(), node_name, "pub", &node.publishers, ctx);
            check_node_endpoints(self.id(), node_name, "sub", &node.subscribers, ctx);
            check_node_endpoints(self.id(), node_name, "cli", &node.cli, ctx);
        }
    }
}

fn check_node_endpoints(
    rule_id: &str,
    node_name: &str,
    kind: &str,
    eps: &BTreeMap<String, ros_launch_manifest_types::EndpointProps>,
    ctx: &mut CheckContext,
) {
    for (ep_name, props) in eps {
        if let Some(qos) = &props.qos {
            let path = format!("nodes.{node_name}.{kind}.{ep_name}.qos");
            check_qos_values(rule_id, &path, qos, ctx);
        }
    }
}

fn check_qos_values(rule_id: &str, path: &str, qos: &QosDecl, ctx: &mut CheckContext) {
    use ros_launch_manifest_types::qos::{QosField, Written, classify};
    for f in QosField::ALL {
        let Some(v) = f.get(qos) else { continue };
        let at = format!("{path}.{}", f.key());
        match classify(v) {
            Written::Literal(l) if f.is_allowed(l) => {}
            // The parser admits exactly `$(var <arg>)`; the consumer evaluates
            // it per scope (`substitute_manifest`). Read alone, the contract
            // has no value for it: say so rather than pass it.
            Written::Var(arg) => ctx.warning(
                rule_id,
                &at,
                format!(
                    "{} '{v}' is not evaluated here (no launch scope passes `{arg}`); it is \
                     checked when a launch file passes the arg",
                    f.key()
                ),
            ),
            _ => ctx.error(
                rule_id,
                &at,
                format!(
                    "invalid {} value '{v}'; accepted: {}",
                    f.key(),
                    f.allowed_list()
                ),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CheckContext;
    use ros_launch_manifest_types::parse::parse_manifest_str;

    fn run(yaml: &str) -> Vec<crate::Diagnostic> {
        run_set(yaml, |_| {})
    }

    /// The parser refuses an invalid value (issue #0083), so a test of the
    /// rule writes one into a parsed manifest: the rule is still the last
    /// line for a manifest built in code.
    fn run_set(yaml: &str, set: impl FnOnce(&mut Manifest)) -> Vec<crate::Diagnostic> {
        let mut manifest = parse_manifest_str(yaml).unwrap();
        set(&mut manifest);
        let graph = DataflowGraph::build(&manifest);
        let mut ctx = CheckContext::new();
        QosCompatRule.check(&manifest, &graph, &mut ctx);
        ctx.diagnostics
    }

    #[test]
    fn topic_invalid_reliability_errors() {
        let yaml = r#"
version: 1
topics:
  /foo:
    type: std_msgs/msg/String
    qos:
      reliability: reliable
"#;
        let errs: Vec<_> = run_set(yaml, |m| {
            m.topics
                .get_mut("/foo")
                .unwrap()
                .qos
                .as_mut()
                .unwrap()
                .reliability = Some("maybe".into())
        })
        .into_iter()
        .filter(|d| d.severity == crate::Severity::Error)
        .collect();
        assert_eq!(errs.len(), 1);
        assert!(errs[0].message.contains("maybe"));
        assert!(errs[0].path.ends_with("reliability"));
    }

    #[test]
    fn endpoint_invalid_reliability_errors() {
        let yaml = r#"
version: 1
nodes:
  pub_node:
    pub:
      data:
        qos: { reliability: reliable }
"#;
        let errs: Vec<_> = run_set(yaml, |m| {
            let ep = m.nodes.get_mut("pub_node").unwrap();
            let q = ep.publishers.get_mut("data").unwrap().qos.as_mut().unwrap();
            q.reliability = Some("kinda_reliable".into());
        })
        .into_iter()
        .filter(|d| d.severity == crate::Severity::Error)
        .collect();
        assert_eq!(errs.len(), 1, "got: {errs:?}");
        assert!(errs[0].path.contains("pub_node.pub.data.qos.reliability"));
        assert!(errs[0].message.contains("kinda_reliable"));
    }

    #[test]
    fn endpoint_invalid_durability_errors() {
        let yaml = r#"
version: 1
nodes:
  sub_node:
    sub:
      data:
        qos: { durability: volatile }
"#;
        let errs: Vec<_> = run_set(yaml, |m| {
            let ep = m.nodes.get_mut("sub_node").unwrap();
            let q = ep
                .subscribers
                .get_mut("data")
                .unwrap()
                .qos
                .as_mut()
                .unwrap();
            q.durability = Some("maybe".into());
        })
        .into_iter()
        .filter(|d| d.severity == crate::Severity::Error)
        .collect();
        assert_eq!(errs.len(), 1);
        assert!(errs[0].path.contains("sub_node.sub.data.qos.durability"));
    }

    #[test]
    fn invalid_history_errors() {
        let yaml = r#"
version: 1
topics:
  /foo:
    type: std_msgs/msg/String
    qos: { history: keep_last }
"#;
        let errs: Vec<_> = run_set(yaml, |m| {
            m.topics
                .get_mut("/foo")
                .unwrap()
                .qos
                .as_mut()
                .unwrap()
                .history = Some("keep_some".into())
        })
        .into_iter()
        .filter(|d| d.severity == crate::Severity::Error)
        .collect();
        assert_eq!(errs.len(), 1);
        assert!(errs[0].message.contains("keep_some"));
    }

    #[test]
    fn invalid_liveliness_errors() {
        let yaml = r#"
version: 1
topics:
  /foo:
    type: std_msgs/msg/String
    qos: { liveliness: automatic }
"#;
        let errs: Vec<_> = run_set(yaml, |m| {
            m.topics
                .get_mut("/foo")
                .unwrap()
                .qos
                .as_mut()
                .unwrap()
                .liveliness = Some("best_effort".into())
        })
        .into_iter()
        .filter(|d| d.severity == crate::Severity::Error)
        .collect();
        assert_eq!(errs.len(), 1);
        assert!(errs[0].path.ends_with("liveliness"));
    }

    #[test]
    fn valid_topic_and_endpoint_qos_no_errors() {
        let yaml = r#"
version: 1
nodes:
  pub_node:
    pub:
      data:
        qos: { reliability: reliable, durability: transient_local }
  sub_node:
    sub:
      data:
        qos: { reliability: best_effort }
topics:
  /channel:
    type: std_msgs/msg/String
    pub: [pub_node/data]
    sub: [sub_node/data]
    qos:
      reliability: reliable
      durability: volatile
      history: keep_last
      liveliness: automatic
"#;
        let errs: Vec<_> = run(yaml)
            .into_iter()
            .filter(|d| d.severity == crate::Severity::Error)
            .collect();
        assert!(errs.is_empty(), "expected no errors, got: {errs:?}");
    }

    #[test]
    fn ros_2_defaults_are_valid_values() {
        let yaml = r#"
version: 1
topics:
  /foo:
    type: std_msgs/msg/String
    qos:
      reliability: best_available
      durability: system_default
      history: system_default
      liveliness: best_available
"#;
        assert!(run(yaml).is_empty());
    }

    #[test]
    fn an_unevaluated_var_is_reported_not_passed() {
        let yaml = r#"
version: 1
args:
  dur:
    choices: [volatile, transient_local]
topics:
  /foo:
    type: std_msgs/msg/String
    qos: { durability: $(var dur) }
"#;
        let d = run(yaml);
        assert_eq!(d.len(), 1, "got: {d:?}");
        assert_eq!(d[0].severity, crate::Severity::Warning);
        assert!(d[0].message.contains("not evaluated"));
        assert!(d[0].path.ends_with("qos.durability"));
    }

    #[test]
    fn the_error_names_the_allowed_set() {
        let yaml = "version: 1\ntopics:\n  /foo:\n    type: T\n    qos: { durability: volatile }\n";
        let d = run_set(yaml, |m| {
            m.topics
                .get_mut("/foo")
                .unwrap()
                .qos
                .as_mut()
                .unwrap()
                .durability = Some("transient-local".into())
        });
        assert_eq!(d.len(), 1);
        assert!(
            d[0].message
                .contains("accepted: volatile, transient_local, system_default, best_available"),
            "{}",
            d[0].message
        );
    }
}
