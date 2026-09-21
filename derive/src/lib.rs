//! The one derivation of the scheduling mapper's input from a `SystemModel`.
//!
//! Design issue #52 (`docs/design-issues.md`): play_launch and nano-ros each
//! derived `ros_launch_manifest_sched::MapperInput` from their own view of
//! the system, and the two derivations disagreed on most of the facts the
//! `chain_aware` algorithm ranks by. This crate is the replacement both
//! consumers call and neither reimplements:
//!
//! - [`mapper_input_from_model`] builds the `MapperInput` -- one
//!   `MapperNode` per `structure.nodes` entry, its paths from
//!   `contracts.node_paths`, and the chains from [`resolve_chains`] -- and a
//!   [`DeriveReport`] saying what the model could not tell it.
//! - [`resolve_chains`] derives one `ResolvedChain` per scope path from the
//!   dataflow route through the model (the port of play_launch's
//!   `manifest_graph`, see [`graph`]).
//! - [`DeriveFacts`] carries the one thing the model does not: each
//!   consumer's execution-cost facts (a platform file's `budget_us`, a
//!   `[wcet]` profile). The attribution rule is in here so both apply it
//!   the same way.
//!
//! The rule that separates this from the reverted `execution.sched`
//! embedding: the model carries every fact the CHECKER resolves per entity
//! (trigger, criticality, bounds); it carries nothing the MAPPER resolves
//! (no route, no rank, no tier). This crate is the function from the first
//! to the second. It sits above `model` because `model` already depends on
//! `sched`, and it must not be `sched` because `sched` stays parser-free.
//!
//! # The rules, stated once
//!
//! | fact | rule |
//! |---|---|
//! | effective trigger | the model's trigger fact; a path without one is `Unclassified`, never a timer (`DeriveReport::paths_without_trigger`) |
//! | `rate_hz` | the fastest `Timer` trigger among the node's paths, and nothing else: authored `topics.<t>.rate_hz` and `pub.<ep>.min_rate_hz` are runtime promises no mapper reads |
//! | `deadline_us` / `path_budget_ms` | min over the node's paths' `max_latency_ms` and its services' `max_response_ms` |
//! | criticality | the model's effective value when it carries one, else the advisory label (`high`/`medium`/`low`, case-insensitive) |
//! | `claims_concurrency` | play_launch's merged-group rule: declared exclusion sets sharing a member merge; concurrency is claimed unless one merged group covers every declared path (`exclusive: []` claims it; an absent declaration or a single path does not) |
//! | `exec_ms` | `DeriveFacts::path_exec_ms` by `"<node>/<path>"`, else `DeriveFacts::node_exec_ms` only when the node has exactly one path; never invented |
//! | chains | one per scope path with a budget: the longest causal route from the input topics to the output topics inside the scope's subtree, timer paths as boundaries (`period_ms = 1000 / rate_hz`), the rest as segments; criticality = max over members |
//!
//! On a model resolved before the R1 fields existed (`PathContract::trigger`
//! is `None` everywhere) every path reads as `Unclassified`, the derivation
//! ranks nothing, and the report lists every path. That is deliberate: a
//! stale model must be visible as such, not scheduled from a guess.

pub mod graph;
pub mod view;

use std::collections::{BTreeMap, BTreeSet};

use ros_launch_manifest_model::SystemModel;
use ros_launch_manifest_sched::{
    ChainElement, ChainSemantics, Criticality, EffectiveTrigger, MapperInput, MapperNode,
    MapperPath, ResolvedChain, SegmentNode,
};

pub use view::parse_criticality_label;
use view::{ModelView, NodeView};

/// The consumer's execution-cost facts, the one input the model does not
/// carry because it differs per platform. play_launch fills `node_exec_ms`
/// from its platform file's `budget_us` (in ms here); nano-ros fills
/// `path_exec_ms` from its `[wcet]` profile. Either may be empty. No WCET
/// is ever invented from a deadline.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeriveFacts {
    /// `"<node FQN>/<path name>"` -> execution-time bound, milliseconds.
    pub path_exec_ms: BTreeMap<String, f64>,
    /// `"<node FQN>"` -> execution-time bound for the whole node,
    /// milliseconds. A bare node name is accepted too, because a platform
    /// file's override selectors have always taken either form.
    pub node_exec_ms: BTreeMap<String, f64>,
}

impl DeriveFacts {
    /// The cost of one path. A per-path fact wins. A per-NODE fact is
    /// attributed only when the node has exactly one path: where it has
    /// several the split is unknown (`play_launch measure` emits a node
    /// budget as the SUM of its per-path maxima), and giving the sum to any
    /// one path would overstate it. Absent is the honest answer, and the
    /// feasibility diagnostic reports absent cost as "feasible on incomplete
    /// evidence" rather than as feasible.
    pub fn exec_ms_for(
        &self,
        node_fqn: &str,
        path_name: &str,
        node_path_count: usize,
    ) -> Option<f64> {
        if let Some(ms) = self.path_exec_ms.get(&format!("{node_fqn}/{path_name}")) {
            return Some(*ms);
        }
        if node_path_count != 1 {
            return None;
        }
        self.node_exec_ms.get(node_fqn).copied().or_else(|| {
            node_fqn
                .rsplit('/')
                .next()
                .filter(|bare| !bare.is_empty() && *bare != node_fqn)
                .and_then(|bare| self.node_exec_ms.get(bare).copied())
        })
    }
}

/// Why a scope path produced no chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainSkip {
    /// The scope path declares no `max_latency_ms`; a chain is a budget
    /// over a route, and there is no budget.
    NoBudget,
    /// No publisher or subscriber of an input topic, or no publisher of an
    /// output topic, lies inside the scope's subtree.
    NoEndpoints,
    /// The causal edges among the subtree's paths form a cycle that no
    /// `state: true` subscription breaks.
    Cycle,
    /// No causal route joins a source to a sink.
    NoRoute,
    /// A route exists but no hop on it is attributed to a declared path
    /// with a trigger fact, so it has neither a boundary nor a segment
    /// link. This is every route of a model with `trigger: None`.
    NoPathOnRoute,
}

/// One scope path that produced no chain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedChain {
    /// The `contracts.scope_paths` key, `"<scope id>/<path name>"`.
    pub scope_path: String,
    pub reason: ChainSkip,
}

/// What the derivation could and could not read off the model, so a
/// consumer can say so instead of silently ranking nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeriveReport {
    /// `"<node FQN>/<path name>"` of every node path the model carries no
    /// trigger fact for, in `MapperInput::nodes` order (the model's
    /// launch-traversal order), paths by name within a node. Each is
    /// `Unclassified` in the output. On a model resolved before the R1
    /// migration this is every path.
    pub paths_without_trigger: Vec<String>,
    /// The scope-path keys that produced a chain, in output order.
    pub chains_resolved: Vec<String>,
    /// The scope-path keys that did not, with why.
    pub chains_skipped: Vec<SkippedChain>,
}

/// Derive the mapper's input from the model, plus the report.
///
/// `legacy` is left `None`: the `.toml` bridge sets it afterwards, and no
/// other mapper reads it.
pub fn mapper_input_from_model(
    model: &SystemModel,
    facts: &DeriveFacts,
) -> (MapperInput, DeriveReport) {
    let view = ModelView::from_model(model);
    derive_from_view(&view, facts)
}

/// One `ResolvedChain` per scope path whose route the model resolves. The
/// same chains [`mapper_input_from_model`] puts in `MapperInput::chains`.
pub fn resolve_chains(model: &SystemModel, facts: &DeriveFacts) -> Vec<ResolvedChain> {
    let view = ModelView::from_model(model);
    let mut report = DeriveReport::default();
    chains_from_view(&view, facts, &mut report)
}

pub(crate) fn derive_from_view(
    view: &ModelView,
    facts: &DeriveFacts,
) -> (MapperInput, DeriveReport) {
    let mut report = DeriveReport::default();
    let nodes = view
        .nodes
        .iter()
        .map(|n| mapper_node(n, facts, &mut report))
        .collect();
    let chains = chains_from_view(view, facts, &mut report);
    (
        MapperInput {
            nodes,
            legacy: None,
            chains,
        },
        report,
    )
}

fn mapper_node(node: &NodeView, facts: &DeriveFacts, report: &mut DeriveReport) -> MapperNode {
    let path_count = node.paths.len();
    let paths: Vec<MapperPath> = node
        .paths
        .values()
        .map(|p| {
            if p.trigger.is_none() {
                report
                    .paths_without_trigger
                    .push(format!("{}/{}", node.fqn, p.name));
            }
            MapperPath {
                name: p.name.clone(),
                effective_trigger: p.effective_trigger(),
                max_latency_ms: p.max_latency_ms,
                exec_ms: facts.exec_ms_for(&node.fqn, &p.name, path_count),
                inputs: p.inputs.clone(),
                outputs: p.outputs.clone(),
                max_jitter_ms: p.max_jitter_ms,
                miss: p.miss.clone(),
            }
        })
        .collect();

    // The fastest timer among the node's paths: the rate that would starve
    // first if the node were under-prioritised. A node with no timer path
    // has no rate to be monotonic about and lands on the default tier.
    let rate_hz = node
        .paths
        .values()
        .filter_map(|p| p.timer_rate_hz())
        .fold(None, |acc: Option<f64>, v| {
            Some(acc.map_or(v, |a| a.max(v)))
        });

    // The tightest bound on how long this node may take to produce an
    // answer: a path's latency budget and a service's response bound are
    // the same kind of claim, so they join one fold.
    let min_ms = node
        .paths
        .values()
        .filter_map(|p| p.max_latency_ms)
        .chain(node.srv_max_response_ms.iter().copied())
        .fold(None, |acc: Option<f64>, v| {
            Some(acc.map_or(v, |a| a.min(v)))
        });

    MapperNode {
        name: node.fqn.clone(),
        scope: node.scope.clone(),
        rate_hz,
        deadline_us: min_ms.map(|ms| (ms * 1000.0).round() as u64),
        criticality: node.criticality,
        path_budget_ms: min_ms,
        paths,
        claims_concurrency: claims_concurrency(node),
    }
}

/// Does this node claim that some of its callbacks may run concurrently?
///
/// The merged-group rule play_launch's `sched_derive::claims_concurrency`
/// applies, the one that models what an executor does: a maximal mutually
/// exclusive set IS a callback group, so declared sets sharing a member
/// merge (`[[a, b], [b, c]]` is one group), and concurrency is claimed
/// unless one merged group covers every declared path. `exclusive: []`
/// therefore claims full concurrency, the opposite of omitting the
/// declaration, which means everything serialises. One path cannot contend
/// with itself, whatever is declared.
///
/// nano-ros's former rule ("some path is outside every set") gave a
/// different answer on `[[a, b], [c]]` over `{a, b, c}`: no path is outside
/// a set, yet `a` and `c` may run at once, so concurrency IS claimed.
fn claims_concurrency(node: &NodeView) -> bool {
    let Some(decl) = &node.concurrency else {
        return false;
    };
    if node.paths.len() <= 1 {
        return false;
    }
    let mut groups: Vec<BTreeSet<&str>> = Vec::new();
    for declared in &decl.exclusive {
        let mut merged: BTreeSet<&str> = declared.iter().map(String::as_str).collect();
        groups.retain(|g| {
            if g.is_disjoint(&merged) {
                true
            } else {
                merged.extend(g.iter().copied());
                false
            }
        });
        if !merged.is_empty() {
            groups.push(merged);
        }
    }
    !groups
        .iter()
        .any(|g| node.paths.keys().all(|p| g.contains(p.as_str())))
}

fn chains_from_view(
    view: &ModelView,
    facts: &DeriveFacts,
    report: &mut DeriveReport,
) -> Vec<ResolvedChain> {
    let graph = graph::build_global_graph(view);
    let mut out = Vec::new();
    for sp in &view.scope_paths {
        let mut skip = |reason| {
            report.chains_skipped.push(SkippedChain {
                scope_path: sp.key.clone(),
                reason,
            })
        };
        let Some(max_latency_ms) = sp.max_latency_ms else {
            skip(ChainSkip::NoBudget);
            continue;
        };
        let subtree = graph::subtree_scopes(view, &sp.scope);
        let sg = graph::subgraph_for_scope_path(&graph, subtree, &sp.inputs, &sp.outputs);
        let route = match graph::critical_path(&sg) {
            Ok(route) => route,
            Err(graph::RouteFailure::NoEndpoints) => {
                skip(ChainSkip::NoEndpoints);
                continue;
            }
            Err(graph::RouteFailure::Cycle) => {
                skip(ChainSkip::Cycle);
                continue;
            }
            Err(graph::RouteFailure::NoRoute) => {
                skip(ChainSkip::NoRoute);
                continue;
            }
        };

        let mut elements: Vec<ChainElement> = Vec::new();
        let mut criticality = Criticality::Low;
        for (fqn, path_name) in &route.vertices {
            // A hop no declared path accounts for carries no trigger fact,
            // so it can be neither a boundary nor a segment link. Skipping
            // it keeps the route honest rather than inventing a category.
            let Some(path_name) = path_name else {
                continue;
            };
            let Some(node) = view.node(fqn) else {
                continue;
            };
            if let Some(c) = node.criticality
                && c > criticality
            {
                criticality = c;
            }
            let path = node.paths.get(path_name);
            match path.map(|p| p.effective_trigger()) {
                Some(EffectiveTrigger::Timer { rate_hz }) if rate_hz > 0.0 => {
                    elements.push(ChainElement::Boundary {
                        node: fqn.clone(),
                        path: path_name.clone(),
                        period_ms: 1000.0 / rate_hz,
                        exec_ms: facts.exec_ms_for(fqn, path_name, node.paths.len()),
                    });
                }
                _ => push_segment_node(&mut elements, fqn.clone(), path_name.clone()),
            }
        }
        if elements.is_empty() {
            skip(ChainSkip::NoPathOnRoute);
            continue;
        }
        report.chains_resolved.push(sp.key.clone());
        out.push(ResolvedChain {
            name: sp.name.clone(),
            criticality,
            max_latency_ms,
            // Every derived route is a reaction; `PathContract` has no
            // `semantics` field and nothing branches on this.
            semantics: ChainSemantics::Reaction,
            elements,
        });
    }
    out
}

/// Append to the last `Segment` when the previous element was one, else
/// start a new one: consecutive non-boundary links merge into one segment,
/// in source-to-sink order.
fn push_segment_node(elements: &mut Vec<ChainElement>, node: String, path: String) {
    let entry = SegmentNode { node, path };
    if let Some(ChainElement::Segment {
        nodes_in_topo_order,
    }) = elements.last_mut()
    {
        nodes_in_topo_order.push(entry);
    } else {
        elements.push(ChainElement::Segment {
            nodes_in_topo_order: vec![entry],
        });
    }
}

#[cfg(test)]
mod tests;
