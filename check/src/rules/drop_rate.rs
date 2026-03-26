//! Rule: scope drop rate budget covers the chain delivery rate.

use super::ValidationRule;
use crate::{CheckContext, graph::DataflowGraph};
use ros_launch_manifest_types::Manifest;

pub struct DropRateRule;

impl ValidationRule for DropRateRule {
    fn id(&self) -> &str {
        "drop-rate"
    }

    fn check(&self, manifest: &Manifest, _graph: &DataflowGraph, ctx: &mut CheckContext) {
        // Check scope paths
        for (path_name, path) in &manifest.paths {
            let scope_drop = match &path.drop {
                Some(d) => d,
                None => continue,
            };
            let scope_count = match &scope_drop.max_count {
                Some(c) => c,
                None => continue,
            };
            let scope_drop_rate = scope_count.drop_rate();

            // Compute chain delivery rate from all nodes
            let mut ln_delivery = 0.0_f64;
            for node in manifest.nodes.values() {
                for node_path in node.paths.values() {
                    if let Some(drop) = &node_path.drop
                        && let Some(count) = &drop.max_count
                    {
                        let r = count.delivery_rate();
                        if r > 0.0 {
                            ln_delivery += r.ln();
                        }
                    }
                }
            }

            // Add topic transport drops
            for topic in manifest.topics.values() {
                if let Some(drop) = &topic.drop
                    && let Some(count) = &drop.max_count
                {
                    let r = count.delivery_rate();
                    if r > 0.0 {
                        ln_delivery += r.ln();
                    }
                }
            }

            let chain_delivery = ln_delivery.exp();
            let chain_drop_rate = 1.0 - chain_delivery;

            if scope_drop_rate < chain_drop_rate {
                ctx.error(
                    self.id(),
                    &format!("paths.{path_name}.drop"),
                    format!(
                        "scope drop budget {scope_count} (rate {scope_drop_rate:.4}) < chain drop rate {chain_drop_rate:.4}"
                    ),
                );
            }
        }
    }
}
