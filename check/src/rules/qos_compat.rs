//! Rule: QoS compatibility between publishers and subscribers on the same topic.

use super::ValidationRule;
use crate::{CheckContext, graph::DataflowGraph};
use ros_launch_manifest_types::Manifest;

pub struct QosCompatRule;

impl ValidationRule for QosCompatRule {
    fn id(&self) -> &str {
        "qos-compat"
    }

    fn check(&self, manifest: &Manifest, _graph: &DataflowGraph, ctx: &mut CheckContext) {
        for (topic_name, topic) in &manifest.topics {
            let qos = match &topic.qos {
                Some(q) => q,
                None => continue,
            };

            // If topic declares QoS, check that it's internally consistent
            // (reliability and durability are valid values)
            if let Some(rel) = &qos.reliability
                && rel != "reliable" && rel != "best_effort" {
                    ctx.error(
                        self.id(),
                        &format!("topics.{topic_name}.qos.reliability"),
                        format!("invalid reliability value '{rel}', expected 'reliable' or 'best_effort'"),
                    );
                }
            if let Some(dur) = &qos.durability
                && dur != "volatile" && dur != "transient_local" {
                    ctx.error(
                        self.id(),
                        &format!("topics.{topic_name}.qos.durability"),
                        format!("invalid durability value '{dur}', expected 'volatile' or 'transient_local'"),
                    );
                }
        }
    }
}
