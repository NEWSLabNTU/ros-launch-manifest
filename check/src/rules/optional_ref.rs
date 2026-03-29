//! Rule: `?` suffix must match node conditionality.
//!
//! - Refs to conditional nodes (with `if:` or `unless:`) MUST have `?` suffix
//! - Refs to unconditional nodes MUST NOT have `?` suffix

use super::ValidationRule;
use crate::{CheckContext, graph::DataflowGraph};
use ros_launch_manifest_types::Manifest;
use std::collections::HashSet;

pub struct OptionalRefRule;

impl ValidationRule for OptionalRefRule {
    fn id(&self) -> &str {
        "optional-ref"
    }

    fn check(&self, manifest: &Manifest, _graph: &DataflowGraph, ctx: &mut CheckContext) {
        // Collect conditional node names
        let conditional_nodes: HashSet<&str> = manifest
            .nodes
            .iter()
            .filter(|(_, node)| node.if_condition.is_some() || node.unless_condition.is_some())
            .map(|(name, _)| name.as_str())
            .collect();

        // Check topic pub/sub refs
        for (topic_name, topic) in &manifest.topics {
            for r in topic.publishers.iter().chain(topic.subscribers.iter()) {
                check_ref(
                    r,
                    &conditional_nodes,
                    &format!("topics.{topic_name}"),
                    ctx,
                    self.id(),
                );
            }
        }

        // Check service server/client refs
        for (svc_name, svc) in &manifest.services {
            for r in svc.server.iter().chain(svc.client.iter()) {
                check_ref(
                    r,
                    &conditional_nodes,
                    &format!("services.{svc_name}"),
                    ctx,
                    self.id(),
                );
            }
        }
    }
}

fn check_ref(
    r: &str,
    conditional_nodes: &HashSet<&str>,
    path: &str,
    ctx: &mut CheckContext,
    rule_id: &str,
) {
    let (bare, has_suffix) = if let Some(b) = r.strip_suffix('?') {
        (b, true)
    } else {
        (r, false)
    };

    let node_name = match bare.split_once('/') {
        Some((node, _)) => node,
        None => return, // not a node/endpoint ref
    };

    let is_conditional = conditional_nodes.contains(node_name);

    if is_conditional && !has_suffix {
        ctx.error(
            rule_id,
            path,
            format!(
                "ref '{r}' to conditional node '{node_name}' must use '?' suffix (e.g., '{bare}?')"
            ),
        );
    } else if !is_conditional && has_suffix {
        ctx.error(
            rule_id,
            path,
            format!("ref '{r}' to unconditional node '{node_name}' must not use '?' suffix"),
        );
    }
}
