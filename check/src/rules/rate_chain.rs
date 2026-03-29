//! Rule: scope export rate achievable from upstream rates.

use super::ValidationRule;
use crate::{CheckContext, graph::DataflowGraph};
use ros_launch_manifest_types::Manifest;

pub struct RateChainRule;

impl ValidationRule for RateChainRule {
    fn id(&self) -> &str {
        "rate-chain"
    }

    fn check(&self, manifest: &Manifest, _graph: &DataflowGraph, ctx: &mut CheckContext) {
        // Check that scope pub (export) endpoints have achievable rates
        for (group_name, members) in &manifest.scope_pub {
            for member in members {
                if let Some((node_name, ep_name)) = member.split_once('/')
                    && let Some(node) = manifest.nodes.get(node_name)
                    && let Some(props) = node.publishers.get(ep_name)
                {
                    // If endpoint declares min_rate_hz, check it's achievable
                    // from the node's path inputs
                    if let Some(pub_rate) = props.min_rate_hz {
                        check_rate_achievable(
                            manifest,
                            node_name,
                            pub_rate,
                            group_name,
                            ctx,
                            self.id(),
                        );
                    }
                }
            }
        }
    }
}

fn check_rate_achievable(
    manifest: &Manifest,
    node_name: &str,
    required_rate: f64,
    export_group: &str,
    ctx: &mut CheckContext,
    rule_id: &str,
) {
    let node = match manifest.nodes.get(node_name) {
        Some(n) => n,
        None => return,
    };

    // Check if any path has all-state inputs (periodic node)
    // A periodic node's rate is independent of upstream
    let has_periodic_path = node.paths.values().any(|p| p.input.is_empty());

    if has_periodic_path {
        // Rate is from the timer, not upstream — achievable by definition
        return;
    }

    // For reactive nodes, check that upstream topics have sufficient rate
    for path in node.paths.values() {
        for input_ep in &path.input {
            // Find which topic wires to this input
            let full_ref = format!("{node_name}/{input_ep}");
            for (topic_name, topic) in &manifest.topics {
                if topic.subscribers.contains(&full_ref)
                    && let Some(topic_rate) = topic.rate_hz
                    && topic_rate < required_rate
                {
                    ctx.warning(
                                rule_id,
                                &format!("exports.{export_group}"),
                                format!(
                                    "export requires {required_rate} Hz but upstream topic '{topic_name}' only provides {topic_rate} Hz"
                                ),
                            );
                }
            }
        }
    }
}
