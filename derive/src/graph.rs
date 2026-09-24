//! The dataflow route a scope path resolves to.
//!
//! A port of play_launch's `manifest_graph::{build_global_graph,
//! subgraph_for_scope_path, critical_path}` (`ros-launch-resolve/resolve/
//! src/ros/manifest_graph.rs` at 0.11.0) from `ManifestIndex` onto the
//! model: `structure.topics` endpoint refs are the edges, `node_paths`
//! triggers and outputs attribute each hop to a path, `sub_endpoints.state`
//! breaks cycles, `structure.scopes` gives the subtree and `scope_paths` the
//! two ends. The arithmetic is unchanged -- series hops sum, a fork-join
//! takes the slowest branch, a timer path costs one period plus its exec --
//! so the scope-path form and the chain form of one system keep agreeing.
//! The WEIGHTS are per edge rather than per topic: a subscriber may state
//! its own transport, and the topic's value is the default for those that
//! do not (play_launch issue #0042, design issue #55).
//!
//! Everything here is keyed by `BTreeMap`/`BTreeSet`, so a given model
//! yields one route, in one order, on every run.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::view::{ModelView, NodeView, split_endpoint_ref};

/// A directed edge: one publisher endpoint to one subscriber endpoint over
/// one topic. Several may join the same pair of nodes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Edge {
    pub from: String,
    pub to: String,
    pub topic: String,
    /// Publisher endpoint ref on `from`: the half that lets a hop be
    /// attributed to the PATH whose `output` names it.
    pub pub_ep: String,
    /// Subscriber endpoint ref on `to`.
    pub sub_ep: String,
    /// Transport for THIS edge: [`TopicView::transport_ms`] of `sub_ep`,
    /// which is the subscriber's declared value or the topic's default.
    pub max_transport_ms: Option<f64>,
    /// The subscriber is `state: true`: polled, not causal. A state edge
    /// carries no latency and never closes a cycle.
    pub is_state: bool,
}

/// The global dataflow graph over every node in the model.
#[derive(Debug)]
pub(crate) struct Graph<'a> {
    pub view: &'a ModelView,
    pub edges: Vec<Edge>,
    /// Topic FQN -> publisher node FQNs.
    pub topic_publishers: BTreeMap<String, Vec<String>>,
    /// Topic FQN -> subscriber node FQNs.
    pub topic_subscribers: BTreeMap<String, Vec<String>>,
}

/// Build the graph: one vertex per `structure.nodes` entry, one edge per
/// (publisher, subscriber) pair of every topic whose two ends are both
/// modeled nodes.
pub(crate) fn build_global_graph(view: &ModelView) -> Graph<'_> {
    let mut graph = Graph {
        view,
        edges: Vec::new(),
        topic_publishers: BTreeMap::new(),
        topic_subscribers: BTreeMap::new(),
    };
    for (topic_fqn, topic) in &view.topics {
        let pubs: Vec<(&str, &str)> = topic
            .publishers
            .iter()
            .filter_map(|r| split_endpoint_ref(r).map(|(n, _)| (n, r.as_str())))
            .collect();
        let subs: Vec<(&str, &str)> = topic
            .subscribers
            .iter()
            .filter_map(|r| split_endpoint_ref(r).map(|(n, _)| (n, r.as_str())))
            .collect();

        graph
            .topic_publishers
            .entry(topic_fqn.clone())
            .or_default()
            .extend(pubs.iter().map(|(n, _)| n.to_string()));
        graph
            .topic_subscribers
            .entry(topic_fqn.clone())
            .or_default()
            .extend(subs.iter().map(|(n, _)| n.to_string()));

        for (pub_node, pub_ep) in &pubs {
            for (sub_node, sub_ep) in &subs {
                graph.edges.push(Edge {
                    from: pub_node.to_string(),
                    to: sub_node.to_string(),
                    topic: topic_fqn.clone(),
                    pub_ep: pub_ep.to_string(),
                    sub_ep: sub_ep.to_string(),
                    max_transport_ms: topic.transport_ms(sub_ep),
                    is_state: view.state_subs.contains(*sub_ep),
                });
            }
        }
    }
    graph
}

/// `root` and every scope below it, from `structure.scopes` parent links.
pub(crate) fn subtree_scopes(view: &ModelView, root: &str) -> BTreeSet<String> {
    let mut subtree = BTreeSet::from([root.to_string()]);
    loop {
        let before = subtree.len();
        for (scope, parent) in &view.scope_parent {
            if parent.as_deref().is_some_and(|p| subtree.contains(p)) {
                subtree.insert(scope.clone());
            }
        }
        if subtree.len() == before {
            return subtree;
        }
    }
}

/// The graph restricted to a scope subtree, with the ends of one scope path.
#[derive(Debug)]
pub(crate) struct Subgraph<'a> {
    pub graph: &'a Graph<'a>,
    pub subtree: BTreeSet<String>,
    /// Publishers AND subscribers of the input topics inside the subtree:
    /// an input published outside the subtree has no publisher to start
    /// from, so its first subscriber inside is a source too.
    pub sources: Vec<String>,
    /// Publishers of the output topics inside the subtree.
    pub sinks: Vec<String>,
}

impl Subgraph<'_> {
    pub fn node(&self, fqn: &str) -> Option<&NodeView> {
        self.graph
            .view
            .node(fqn)
            .filter(|n| self.subtree.contains(&n.scope))
    }
}

pub(crate) fn subgraph_for_scope_path<'a>(
    graph: &'a Graph<'a>,
    subtree: BTreeSet<String>,
    input_topics: &[String],
    output_topics: &[String],
) -> Subgraph<'a> {
    let in_subtree = |fqn: &str| {
        graph
            .view
            .node(fqn)
            .is_some_and(|n| subtree.contains(&n.scope))
    };
    let push = |list: &mut Vec<String>, fqn: &String| {
        if in_subtree(fqn) && !list.contains(fqn) {
            list.push(fqn.clone());
        }
    };

    let mut sources = Vec::new();
    for topic in input_topics {
        for fqn in graph.topic_publishers.get(topic).into_iter().flatten() {
            push(&mut sources, fqn);
        }
        for fqn in graph.topic_subscribers.get(topic).into_iter().flatten() {
            push(&mut sources, fqn);
        }
    }
    let mut sinks = Vec::new();
    for topic in output_topics {
        for fqn in graph.topic_publishers.get(topic).into_iter().flatten() {
            push(&mut sinks, fqn);
        }
    }

    Subgraph {
        graph,
        subtree,
        sources,
        sinks,
    }
}

/// A vertex of the PATH-level graph: one declared path of one node, or
/// (`None`) the node reached by a hop no declared path accounts for, which
/// is charged the node-wide maximum.
pub(crate) type PathVertex = (String, Option<String>);

/// The longest source-to-sink route.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CriticalPath {
    pub total_ms: f64,
    /// One period per timer path crossed: the part no scheduling can remove.
    pub sampling_cost_ms: f64,
    /// The `(node, path)` pairs traversed, source first.
    pub vertices: Vec<PathVertex>,
}

/// Why a scope path yielded no route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteFailure {
    /// No source or no sink inside the subtree.
    NoEndpoints,
    /// The causal (non-state) edges among the subtree's paths form a cycle.
    Cycle,
    /// No causal route joins a source to a sink.
    NoRoute,
}

/// Lower the node-level subgraph to path granularity: a vertex is a path
/// and an edge joins the path that PUBLISHED a topic to the path whose
/// trigger CONSUMES it. Facts are declared per path, so a node with two
/// causal outputs at different costs has two vertices, not one.
fn build_path_graph(sg: &Subgraph) -> (Vec<PathVertex>, Vec<(usize, usize, f64)>) {
    let mut vset: BTreeSet<PathVertex> = BTreeSet::new();
    for node in &sg.graph.view.nodes {
        if !sg.subtree.contains(&node.scope) {
            continue;
        }
        if node.paths.is_empty() {
            vset.insert((node.fqn.clone(), None));
        } else {
            for name in node.paths.keys() {
                vset.insert((node.fqn.clone(), Some(name.clone())));
            }
        }
    }

    let mut raw: Vec<(PathVertex, PathVertex, f64)> = Vec::new();
    for edge in &sg.graph.edges {
        if edge.is_state {
            continue;
        }
        let (Some(from), Some(to)) = (sg.node(&edge.from), sg.node(&edge.to)) else {
            continue;
        };
        let producers = attributed(from.paths_producing(&edge.pub_ep));
        let consumers = attributed(to.paths_consuming(&edge.sub_ep));
        let transport = edge.max_transport_ms.unwrap_or(0.0);
        for p in &producers {
            for c in &consumers {
                let a = (edge.from.clone(), p.clone());
                let b = (edge.to.clone(), c.clone());
                vset.insert(a.clone());
                vset.insert(b.clone());
                raw.push((a, b, transport));
            }
        }
    }

    let vertices: Vec<PathVertex> = vset.into_iter().collect();
    let index: BTreeMap<&PathVertex, usize> =
        vertices.iter().enumerate().map(|(i, v)| (v, i)).collect();
    let edges = raw
        .iter()
        .filter_map(|(a, b, t)| Some((*index.get(a)?, *index.get(b)?, *t)))
        .collect();
    (vertices, edges)
}

/// A hop no path claims still has to land somewhere: on the node's fallback
/// vertex.
fn attributed(paths: Vec<&str>) -> Vec<Option<String>> {
    if paths.is_empty() {
        vec![None]
    } else {
        paths.into_iter().map(|s| Some(s.to_string())).collect()
    }
}

/// Kahn's algorithm; `None` on a cycle. Ascending seed order keeps the
/// result deterministic.
fn topo_sort(n: usize, edges: &[(usize, usize, f64)]) -> Option<Vec<usize>> {
    let mut indeg = vec![0usize; n];
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(a, b, _) in edges {
        indeg[b] += 1;
        out[a].push(b);
    }
    let mut queue: VecDeque<usize> = (0..n).filter(|i| indeg[*i] == 0).collect();
    let mut order = Vec::with_capacity(n);
    while let Some(v) = queue.pop_front() {
        order.push(v);
        for &w in &out[v] {
            indeg[w] -= 1;
            if indeg[w] == 0 {
                queue.push_back(w);
            }
        }
    }
    (order.len() == n).then_some(order)
}

/// The worst-case (longest) route from any source to any sink, by forward
/// DP over the path graph. Series hops sum; at a join only the slowest
/// branch counts; state edges carry nothing.
pub(crate) fn critical_path(sg: &Subgraph) -> Result<CriticalPath, RouteFailure> {
    if sg.sources.is_empty() || sg.sinks.is_empty() {
        return Err(RouteFailure::NoEndpoints);
    }
    let (vertices, edges) = build_path_graph(sg);
    if vertices.is_empty() {
        return Err(RouteFailure::NoRoute);
    }
    let order = topo_sort(vertices.len(), &edges).ok_or(RouteFailure::Cycle)?;

    let mut incoming: Vec<Vec<(usize, f64)>> = vec![Vec::new(); vertices.len()];
    for &(a, b, t) in &edges {
        incoming[b].push((a, t));
    }
    let is_source = |fqn: &str| sg.sources.iter().any(|s| s == fqn);
    let is_sink = |fqn: &str| sg.sinks.iter().any(|s| s == fqn);

    let mut latency: Vec<Option<f64>> = vec![None; vertices.len()];
    let mut prev: Vec<Option<usize>> = vec![None; vertices.len()];
    for &v in &order {
        let (fqn, path) = &vertices[v];
        let cost = sg
            .graph
            .view
            .node(fqn)
            .map_or(0.0, |n| n.traversal_latency_ms(path.as_deref()));

        // A source may start the route but must not truncate one: when the
        // input's publisher is also inside the subtree, its subscriber is a
        // source too, and "nothing upstream" would drop the hop that
        // produced its data. A source takes the larger of "start here" and
        // "arrive from upstream".
        let mut best: Option<(f64, usize)> = None;
        for &(pred, transport) in &incoming[v] {
            let Some(pred_lat) = latency[pred] else {
                continue;
            };
            let candidate = pred_lat + transport;
            if best.is_none_or(|(b, _)| candidate > b) {
                best = Some((candidate, pred));
            }
        }
        match (best, is_source(fqn)) {
            (Some((arrival, pred)), false) => {
                latency[v] = Some(arrival + cost);
                prev[v] = Some(pred);
            }
            (Some((arrival, pred)), true) => {
                if arrival + cost > cost {
                    latency[v] = Some(arrival + cost);
                    prev[v] = Some(pred);
                } else {
                    latency[v] = Some(cost);
                }
            }
            (None, true) => latency[v] = Some(cost),
            (None, false) => {}
        }
    }

    let mut best_sink: Option<(usize, f64)> = None;
    for (v, (fqn, _)) in vertices.iter().enumerate() {
        if !is_sink(fqn) {
            continue;
        }
        let Some(lat) = latency[v] else {
            continue;
        };
        if best_sink.is_none_or(|(_, b)| lat > b) {
            best_sink = Some((v, lat));
        }
    }
    let (sink_v, total_ms) = best_sink.ok_or(RouteFailure::NoRoute)?;

    // Walk back to the source. The sampling term is summed over the same
    // walk, so it is the sampling cost of the winning route alone.
    let mut traversed: Vec<PathVertex> = Vec::new();
    let mut sampling_cost_ms = 0.0_f64;
    let mut cur = Some(sink_v);
    while let Some(v) = cur {
        let (fqn, path) = &vertices[v];
        traversed.push((fqn.clone(), path.clone()));
        sampling_cost_ms += sg
            .graph
            .view
            .node(fqn)
            .map_or(0.0, |n| n.sampling_cost_ms(path.as_deref()));
        cur = prev[v];
    }
    traversed.reverse();

    Ok(CriticalPath {
        total_ms,
        sampling_cost_ms,
        vertices: traversed,
    })
}
