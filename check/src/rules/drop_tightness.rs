//! Rule: a scope path's end-to-end drop budget is not tighter than a topic
//! on its route (phase 90 G3; `contract-theory.md`, Drop Budgets).
//!
//! A scope path's `drop.max_count` (`N / W`, the rate `d = N / W`) bounds the
//! loss of the whole route; a topic's bounds the loss of one hop. A hop that
//! may lose more than the whole is a part larger than its whole: one of the
//! two numbers is wrong. Only `max_count` is compared (the design's check);
//! consecutive runs reset at periodic nodes, so `max_consecutive` does not
//! compose this way.
//!
//! This is the per-manifest check: the route is the topics of this manifest
//! between the path's inputs and outputs. A consumer that merges a launch
//! tree checks the same thing on the global graph, across scopes.

use super::ValidationRule;
use crate::{CheckContext, graph::DataflowGraph};
use petgraph::{Direction, graph::NodeIndex};
use ros_launch_manifest_types::{Manifest, OptBound};
use std::collections::{BTreeSet, HashSet, VecDeque};

pub struct DropTightnessRule;

impl ValidationRule for DropTightnessRule {
    fn id(&self) -> &str {
        "drop-tightness"
    }

    fn check(&self, manifest: &Manifest, graph: &DataflowGraph, ctx: &mut CheckContext) {
        for (path_name, path) in &manifest.paths {
            let Some(whole) = path.drop.as_ref().and_then(|d| d.max_count.count_per()) else {
                continue;
            };
            let inputs = match path.effective_trigger() {
                ros_launch_manifest_types::EffectiveTrigger::Input(eps) => eps,
                _ => Vec::new(),
            };
            for topic in route_topics(manifest, graph, &inputs, &path.output) {
                let Some(part) = manifest.topics[&topic]
                    .drop
                    .as_ref()
                    .and_then(|d| d.max_count.count_per())
                else {
                    continue;
                };
                if part.drop_rate() > whole.drop_rate() {
                    ctx.warning(
                        self.id(),
                        &format!("paths.{path_name}.drop"),
                        format!(
                            "scope path '{path_name}' permits a loss of {whole} ({:.2}%) end to \
                             end, but topic '{topic}' on its route permits {part} ({:.2}%) on \
                             one hop: a part looser than the whole. Tighten \
                             `topics.{topic}.drop` or loosen the scope path's",
                            whole.drop_rate() * 100.0,
                            part.drop_rate() * 100.0
                        ),
                    );
                }
            }
        }
    }
}

/// The key under `topics:` that `name` (a path's input or output) names:
/// itself, or the same name with or without the root's leading `/`.
fn topic_key(manifest: &Manifest, name: &str) -> Option<String> {
    if manifest.topics.contains_key(name) {
        return Some(name.to_string());
    }
    let other = match name.strip_prefix('/') {
        Some(rel) => rel.to_string(),
        None => format!("/{name}"),
    };
    manifest.topics.contains_key(&other).then_some(other)
}

/// The vertex an endpoint reference (`node/endpoint`, or `scope/...`) sits on.
fn vertex(graph: &DataflowGraph, ep_ref: &str) -> Option<NodeIndex> {
    let (owner, _) = ep_ref.split_once('/')?;
    graph.index_map.get(owner).copied()
}

fn reach(graph: &DataflowGraph, from: &[NodeIndex], dir: Direction) -> HashSet<NodeIndex> {
    let mut seen: HashSet<NodeIndex> = from.iter().copied().collect();
    let mut queue: VecDeque<NodeIndex> = from.iter().copied().collect();
    while let Some(v) = queue.pop_front() {
        for w in graph.graph.neighbors_directed(v, dir) {
            if seen.insert(w) {
                queue.push_back(w);
            }
        }
    }
    seen
}

/// The topics of this manifest on a scope path's route: its input and output
/// topics, and every topic edge between a vertex reachable from the inputs
/// and one that reaches the outputs.
fn route_topics(
    manifest: &Manifest,
    graph: &DataflowGraph,
    inputs: &[String],
    outputs: &[String],
) -> BTreeSet<String> {
    let ins: Vec<String> = inputs
        .iter()
        .filter_map(|t| topic_key(manifest, t))
        .collect();
    let outs: Vec<String> = outputs
        .iter()
        .filter_map(|t| topic_key(manifest, t))
        .collect();
    let mut topics: BTreeSet<String> = ins.iter().chain(outs.iter()).cloned().collect();
    let starts: Vec<NodeIndex> = ins
        .iter()
        .flat_map(|t| manifest.topics[t].subscribers.iter())
        .filter_map(|r| vertex(graph, r))
        .collect();
    let ends: Vec<NodeIndex> = outs
        .iter()
        .flat_map(|t| manifest.topics[t].publishers.iter())
        .filter_map(|r| vertex(graph, r))
        .collect();
    let fwd = reach(graph, &starts, Direction::Outgoing);
    let bwd = reach(graph, &ends, Direction::Incoming);
    for e in graph.graph.edge_indices() {
        let (a, b) = graph.graph.edge_endpoints(e).expect("edge");
        if fwd.contains(&a) && bwd.contains(&a) && fwd.contains(&b) && bwd.contains(&b) {
            topics.insert(graph.graph[e].topic.clone());
        }
    }
    topics
}
