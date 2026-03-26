//! Rule: scope latency budget >= critical path through internal graph.

use super::ValidationRule;
use crate::{CheckContext, graph::DataflowGraph};
use ros_launch_manifest_types::Manifest;

pub struct ScopeBudgetRule;

impl ValidationRule for ScopeBudgetRule {
    fn id(&self) -> &str {
        "scope-budget"
    }

    fn check(&self, manifest: &Manifest, _graph: &DataflowGraph, ctx: &mut CheckContext) {
        // Collect node max_latency_ms values for critical path computation
        let mut node_latencies: std::collections::HashMap<String, f64> =
            std::collections::HashMap::new();

        for (node_name, node) in &manifest.nodes {
            // Take the max latency across all paths on this node
            let max_lat = node
                .paths
                .values()
                .filter_map(|p| p.max_latency_ms)
                .fold(0.0_f64, f64::max);
            if max_lat > 0.0 {
                node_latencies.insert(node_name.clone(), max_lat);
            }
        }

        // Include child scope latencies
        for (scope_name, include) in &manifest.includes {
            if let ros_launch_manifest_types::IncludeDecl::Inline(inner) = include {
                let max_lat = inner
                    .paths
                    .values()
                    .filter_map(|p| p.max_latency_ms)
                    .fold(0.0_f64, f64::max);
                if max_lat > 0.0 {
                    node_latencies.insert(scope_name.clone(), max_lat);
                }
            }
            // For external includes, we can't check without loading them
        }

        // Check scope-level paths
        for (path_name, path) in &manifest.paths {
            if let Some(declared) = path.max_latency_ms {
                // Simple sum of all node latencies as an approximation
                // (proper critical path requires graph traversal)
                let total: f64 = node_latencies.values().sum();
                if total > 0.0 && declared < total {
                    ctx.warning(
                        self.id(),
                        &format!("paths.{path_name}"),
                        format!(
                            "scope max_latency_ms ({declared}) may be less than sum of node latencies ({total}). \
                             Run critical path analysis for precise check."
                        ),
                    );
                }
            }

            // Check max_age_ms if declared
            if let Some(declared_age) = path.max_age_ms
                && let Some(declared_latency) = path.max_latency_ms
                    && declared_age < declared_latency {
                        ctx.error(
                            self.id(),
                            &format!("paths.{path_name}"),
                            format!(
                                "max_age_ms ({declared_age}) < max_latency_ms ({declared_latency}). \
                                 Age must be >= latency since age includes upstream delays."
                            ),
                        );
                    }
        }
    }
}
