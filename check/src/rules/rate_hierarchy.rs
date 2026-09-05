//! Rule: rate hierarchy.
//!
//! Lower bounds: `pub.min_rate_hz >= topic.rate_hz >= max(sub.min_rate_hz)`.
//!
//! Upper bounds (phase 70): `pub.max_rate_hz >= topic.rate_hz` and
//! `topic.rate_hz <= min(sub.max_rate_hz)`. `max_rate_hz` was parsed and
//! lowered for its whole life and read by no rule — which is backwards for
//! queue overrun, since the OVER-fast publisher is the one that overruns a
//! subscriber. A subscriber's `max_rate_hz` is the rate it can drain; a topic
//! faster than that backs up regardless of scheduling.

use super::ValidationRule;
use crate::{CheckContext, graph::DataflowGraph};
use ros_launch_manifest_types::Manifest;

pub struct RateHierarchyRule;

impl ValidationRule for RateHierarchyRule {
    fn id(&self) -> &str {
        "rate-hierarchy"
    }

    fn check(&self, manifest: &Manifest, _graph: &DataflowGraph, ctx: &mut CheckContext) {
        for (topic_name, topic) in &manifest.topics {
            let topic_rate = match topic.rate_hz {
                Some(r) => r,
                None => continue,
            };

            // Check publisher min_rate_hz >= topic rate_hz
            for pub_ref in &topic.publishers {
                if let Some(rate) = resolve_pub_min_rate(pub_ref, manifest)
                    && rate < topic_rate
                {
                    ctx.error(
                            self.id(),
                            &format!("topics.{topic_name}"),
                            format!(
                                "publisher '{pub_ref}' min_rate_hz ({rate}) < topic rate_hz ({topic_rate})"
                            ),
                        );
                }
            }

            // Check topic rate_hz >= max(sub.min_rate_hz)
            let mut max_sub_rate: Option<(f64, String)> = None;
            for sub_ref in &topic.subscribers {
                if let Some(rate) = resolve_sub_min_rate(sub_ref, manifest)
                    && max_sub_rate.as_ref().is_none_or(|(r, _)| rate > *r)
                {
                    max_sub_rate = Some((rate, sub_ref.clone()));
                }
            }
            if let Some((sub_rate, sub_ref)) = max_sub_rate
                && topic_rate < sub_rate
            {
                ctx.error(
                        self.id(),
                        &format!("topics.{topic_name}"),
                        format!(
                            "topic rate_hz ({topic_rate}) < subscriber '{sub_ref}' min_rate_hz ({sub_rate})"
                        ),
                    );
            }

            // Upper bounds. A publisher cannot drive a topic faster than it
            // declares it can publish; a subscriber cannot drain one faster
            // than it declares it can consume.
            for pub_ref in &topic.publishers {
                if let Some(max) = resolve_endpoint(pub_ref, manifest, |n| &n.publishers)
                    .and_then(|p| p.max_rate_hz)
                    && topic_rate > max
                {
                    ctx.error(
                        self.id(),
                        &format!("topics.{topic_name}"),
                        format!(
                            "topic rate_hz ({topic_rate}) > publisher '{pub_ref}' max_rate_hz ({max})"
                        ),
                    );
                }
            }
            for sub_ref in &topic.subscribers {
                if let Some(max) = resolve_endpoint(sub_ref, manifest, |n| &n.subscribers)
                    .and_then(|p| p.max_rate_hz)
                    && topic_rate > max
                {
                    ctx.error(
                        self.id(),
                        &format!("topics.{topic_name}"),
                        format!(
                            "topic rate_hz ({topic_rate}) > subscriber '{sub_ref}' max_rate_hz \
                             ({max}) — it will be overrun regardless of scheduling"
                        ),
                    );
                }
            }
        }
    }
}

fn resolve_endpoint<'a, F>(
    ep_ref: &str,
    manifest: &'a Manifest,
    select: F,
) -> Option<&'a ros_launch_manifest_types::EndpointProps>
where
    F: Fn(
        &'a ros_launch_manifest_types::NodeDecl,
    ) -> &'a std::collections::BTreeMap<String, ros_launch_manifest_types::EndpointProps>,
{
    let (node_name, ep_name) = ep_ref.split_once('/')?;
    let node = manifest.nodes.get(node_name)?;
    select(node).get(ep_name)
}

/// Resolve "node/endpoint" to the publisher's min_rate_hz.
fn resolve_pub_min_rate(pub_ref: &str, manifest: &Manifest) -> Option<f64> {
    resolve_endpoint(pub_ref, manifest, |n| &n.publishers)?.min_rate_hz
}

/// Resolve "node/endpoint" to the subscriber's min_rate_hz.
fn resolve_sub_min_rate(sub_ref: &str, manifest: &Manifest) -> Option<f64> {
    resolve_endpoint(sub_ref, manifest, |n| &n.subscribers)?.min_rate_hz
}

#[cfg(test)]
mod max_rate_tests {
    use super::*;
    use crate::CheckContext;
    use ros_launch_manifest_types::parse::parse_manifest_str;

    fn errors(yaml: &str) -> Vec<String> {
        let manifest = parse_manifest_str(yaml).unwrap();
        let graph = DataflowGraph::build(&manifest);
        let mut ctx = CheckContext::new();
        RateHierarchyRule.check(&manifest, &graph, &mut ctx);
        ctx.diagnostics
            .into_iter()
            .filter(|d| d.severity == crate::Severity::Error)
            .map(|d| d.message)
            .collect()
    }

    /// The finding that motivated this: a topic faster than its subscriber
    /// can drain was accepted in silence.
    #[test]
    fn a_topic_faster_than_a_subscriber_can_drain_is_an_error() {
        let yaml = "version: 1\nnodes:\n  src:\n    pub:\n      out: {}\n  ekf:\n    sub:\n      in:\n        max_rate_hz: 20\ntopics:\n  data:\n    type: T\n    rate_hz: 50\n    pub: [src/out]\n    sub: [ekf/in]\n";
        let errs = errors(yaml);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].contains("max_rate_hz (20)"), "{errs:?}");
    }

    #[test]
    fn a_topic_faster_than_its_publisher_can_publish_is_an_error() {
        let yaml = "version: 1\nnodes:\n  src:\n    pub:\n      out:\n        max_rate_hz: 10\ntopics:\n  data:\n    type: T\n    rate_hz: 50\n    pub: [src/out]\n";
        let errs = errors(yaml);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(
            errs[0].contains("publisher 'src/out' max_rate_hz (10)"),
            "{errs:?}"
        );
    }

    #[test]
    fn a_topic_within_both_bounds_is_clean() {
        let yaml = "version: 1\nnodes:\n  src:\n    pub:\n      out:\n        min_rate_hz: 50\n        max_rate_hz: 100\n  ekf:\n    sub:\n      in:\n        min_rate_hz: 10\n        max_rate_hz: 60\ntopics:\n  data:\n    type: T\n    rate_hz: 50\n    pub: [src/out]\n    sub: [ekf/in]\n";
        assert!(errors(yaml).is_empty());
    }
}
