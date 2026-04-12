//! Rule: QoS profile structural validity and pub/sub compatibility hints.
//!
//! In the current manifest design, topic QoS is a single channel-level
//! profile shared by all publishers and subscribers. This rule catches:
//!
//! 1. **Structural invariants** — combinations that are nonsensical or
//!    misconfigured (e.g. `keep_last` with `depth: 0`).
//! 2. **DDS pitfalls** — combinations that ROS 2 / DDS allow but that
//!    silently break common use cases (e.g. `transient_local` durability
//!    requires `reliable` reliability for late-joining to work).
//!
//! Per-endpoint pub vs sub QoS comparison (the classic "best_effort pub
//! cannot deliver to reliable sub" check) is **not applicable** here:
//! the manifest declares one QoS per topic, so both sides agree by
//! construction. The cross-scope `consistency` rule catches the case
//! where two scopes declare the same topic with different QoS values.

use super::ValidationRule;
use crate::{CheckContext, graph::DataflowGraph};
use ros_launch_manifest_types::{Manifest, QosDecl};

pub struct QosMatchRule;

impl ValidationRule for QosMatchRule {
    fn id(&self) -> &str {
        "qos-match"
    }

    fn check(&self, manifest: &Manifest, _graph: &DataflowGraph, ctx: &mut CheckContext) {
        for (topic_name, topic) in &manifest.topics {
            let Some(qos) = &topic.qos else {
                continue;
            };
            check_qos_profile(self.id(), topic_name, qos, ctx);
        }
    }
}

/// Validate a single QoS profile for structural consistency and DDS pitfalls.
fn check_qos_profile(rule_id: &str, topic_name: &str, qos: &QosDecl, ctx: &mut CheckContext) {
    let path = format!("topics.{topic_name}.qos");

    // depth must be > 0 if declared (regardless of history kind)
    if let Some(depth) = qos.depth
        && depth == 0
    {
        ctx.error(
            rule_id,
            &path,
            format!(
                "topic '{topic_name}' qos.depth is 0; depth must be >= 1 \
                 to enable any message buffering"
            ),
        );
    }

    // keep_last history without depth defaults to ROS 2's depth=10, but
    // declaring keep_last with depth=0 explicitly is invalid (caught above).
    // Caveat: keep_all + depth is valid but depth is ignored — warn so users
    // don't develop the wrong mental model.
    if let (Some(history), Some(_depth)) = (qos.history.as_deref(), qos.depth)
        && history == "keep_all"
    {
        ctx.warning(
            rule_id,
            &path,
            format!(
                "topic '{topic_name}' qos uses history='keep_all' with depth set; \
                 depth is ignored when history is keep_all"
            ),
        );
    }

    // transient_local durability is meaningful only with reliable reliability.
    // best_effort + transient_local is allowed by DDS but late-joining
    // subscribers cannot receive historical data over best_effort.
    if let (Some(rel), Some(dur)) = (qos.reliability.as_deref(), qos.durability.as_deref())
        && rel == "best_effort"
        && dur == "transient_local"
    {
        ctx.warning(
            rule_id,
            &path,
            format!(
                "topic '{topic_name}' qos combines best_effort + transient_local; \
                 late-joining subscribers will not receive historical data over best_effort"
            ),
        );
    }
}
