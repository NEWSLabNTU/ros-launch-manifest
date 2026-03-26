//! Rule: scope max_consecutive is feasible given chain drop rate (Erdos-Renyi).

use super::ValidationRule;
use crate::{CheckContext, graph::DataflowGraph};
use ros_launch_manifest_types::Manifest;

/// Confidence threshold for Poisson approximation.
const EPSILON: f64 = 0.01;

pub struct DropConsecutiveRule;

impl ValidationRule for DropConsecutiveRule {
    fn id(&self) -> &str {
        "drop-consecutive"
    }

    fn check(&self, manifest: &Manifest, _graph: &DataflowGraph, ctx: &mut CheckContext) {
        // Check scope paths
        for (path_name, path) in &manifest.paths {
            let scope_drop = match &path.drop {
                Some(d) => d,
                None => continue,
            };
            let scope_k = match scope_drop.max_consecutive {
                Some(k) => k,
                None => continue,
            };
            let scope_count = match &scope_drop.max_count {
                Some(c) => c,
                None => continue, // Need window size from max_count
            };

            // Necessary condition: scope K >= max(node K)
            let max_node_k = collect_max_node_consecutive(manifest);
            if scope_k < max_node_k {
                ctx.error(
                    self.id(),
                    &format!("paths.{path_name}.drop.max_consecutive"),
                    format!(
                        "scope max_consecutive ({scope_k}) < node max_consecutive ({max_node_k}). \
                         Scope can't be stricter than any individual node."
                    ),
                );
            }

            // Compute chain drop rate
            let chain_drop_rate = compute_chain_drop_rate(manifest);
            if chain_drop_rate <= 0.0 {
                continue;
            }

            // Poisson approximation: P(max run >= K in W) = 1 - exp(-lambda)
            // lambda = (W - K + 1) * d^K * (1 - d)
            let w = scope_count.w as f64;
            let k = scope_k as f64;
            let d = chain_drop_rate;

            let lambda = (w - k + 1.0) * d.powf(k) * (1.0 - d);
            let p_violation = 1.0 - (-lambda).exp();

            if p_violation > EPSILON {
                // Find minimum feasible K
                let mut min_k = scope_k;
                loop {
                    min_k += 1;
                    let lam =
                        (w - min_k as f64 + 1.0) * d.powf(min_k as f64) * (1.0 - d);
                    if 1.0 - (-lam).exp() <= EPSILON || min_k > 100 {
                        break;
                    }
                }

                ctx.warning(
                    self.id(),
                    &format!("paths.{path_name}.drop.max_consecutive"),
                    format!(
                        "max_consecutive {scope_k} is likely to be violated (p={p_violation:.4} > {EPSILON}) \
                         given chain drop rate {d:.4} over window {w}. \
                         Minimum feasible: max_consecutive: {min_k} (p={:.4})",
                        1.0 - (-(w - min_k as f64 + 1.0) * d.powf(min_k as f64) * (1.0 - d)).exp()
                    ),
                );
            }
        }
    }
}

fn collect_max_node_consecutive(manifest: &Manifest) -> u32 {
    let mut max_k = 0u32;
    for node in manifest.nodes.values() {
        for path in node.paths.values() {
            if let Some(drop) = &path.drop
                && let Some(k) = drop.max_consecutive
            {
                max_k = max_k.max(k);
            }
        }
    }
    max_k
}

fn compute_chain_drop_rate(manifest: &Manifest) -> f64 {
    let mut ln_delivery = 0.0_f64;

    for node in manifest.nodes.values() {
        for path in node.paths.values() {
            if let Some(drop) = &path.drop
                && let Some(count) = &drop.max_count
            {
                let r = count.delivery_rate();
                if r > 0.0 {
                    ln_delivery += r.ln();
                }
            }
        }
    }

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

    1.0 - ln_delivery.exp()
}
