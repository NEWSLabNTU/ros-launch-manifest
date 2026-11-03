//! Rule: scope latency budget >= sum of node latencies (conservative).
//!
//! This is a per-manifest fallback that runs when checking a single
//! manifest in isolation (no cross-scope merge). It uses a flat sum
//! of all node latencies plus each topic hop's declared transport (its
//! largest edge, subscriber's own value else the topic's), which is a
//! conservative upper bound — correct for series chains but
//! pessimistic for parallel (fork-join) topologies where the actual
//! critical path is `max(branches) + fusion`.
//!
//! When checking a manifest tree via the manifest loader, the precise
//! topology-aware critical path is computed from the global dataflow
//! graph (Phase 35.3) and reported via the cross-scope `chain-latency`
//! diagnostic. The two diagnostics are deduplicated by their `path`
//! field.

use super::ValidationRule;
use crate::{CheckContext, graph::DataflowGraph};
use ros_launch_manifest_types::{Manifest, OptBound};

pub struct ScopeBudgetRule;

impl ValidationRule for ScopeBudgetRule {
    fn id(&self) -> &str {
        "chain-latency"
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
                .filter_map(|p| p.max_latency.ms())
                .fold(0.0_f64, f64::max);
            if max_lat > 0.0 {
                node_latencies.insert(node_name.clone(), max_lat);
            }
        }

        // Include child scope latencies
        for (scope_name, include) in &manifest.includes {
            if let Some(inner) = include.inline() {
                let max_lat = inner
                    .paths
                    .values()
                    .filter_map(|p| p.max_latency.ms())
                    .fold(0.0_f64, f64::max);
                if max_lat > 0.0 {
                    node_latencies.insert(scope_name.clone(), max_lat);
                }
            }
            // For external includes, we can't check without loading them
        }

        // The transport each topic hop is charged: its largest edge, by the
        // one precedence (a subscriber's own value, else the topic's;
        // play_launch phase 97 P1). Before, only the topic's own value was
        // summed, so per-subscriber values never reached this total.
        let per_topic: Vec<Option<f64>> = manifest
            .topics
            .keys()
            .map(|t| manifest.topic_transport_ms(t))
            .collect();
        let topic_transport: f64 = per_topic.iter().flatten().sum();
        let undeclared_transport_count = per_topic.iter().filter(|t| t.is_none()).count();

        // Check scope-level paths
        for (path_name, path) in &manifest.paths {
            if let Some(declared) = path.max_latency.ms() {
                // Sum: node latencies + declared topic transport.
                // Topics without max_transport_ms contribute 0; their transport
                // is absorbed into the scope's residual headroom.
                let nodes_total: f64 = node_latencies.values().sum();
                let total = nodes_total + topic_transport;
                if total > 0.0 && declared < total {
                    let suffix = if undeclared_transport_count > 0 {
                        format!(
                            " ({undeclared_transport_count} topic(s) have no max_transport; \
                             actual total may be larger)"
                        )
                    } else {
                        String::new()
                    };
                    ctx.warning(
                        self.id(),
                        &format!("paths.{path_name}"),
                        format!(
                            "scope max_latency_ms ({declared}) may be less than sum of node \
                             latencies ({nodes_total}) + declared topic transport ({topic_transport}) = {total}.{suffix}"
                        ),
                    );
                }
            }
        }
    }
}
