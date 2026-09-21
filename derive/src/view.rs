//! The model, read once into the shape the derivation walks.
//!
//! Everything in this module is a READ of `SystemModel` fields: no rule of
//! the derivation lives here except the two lookups that turn a model entry
//! into a fact the rules consume -- [`path_trigger`] and
//! [`node_criticality`] (effective first, label second). Every read of the
//! model is in this file, so a consumer asking "which field decides X" has
//! one place to look.
//!
//! Key shapes, from the model's own docs: node paths are keyed
//! `"<node FQN>/<path name>"`, scope paths `"<scope id>/<path name>"`, and
//! endpoint refs are `"<node FQN>/<endpoint>"`. A path name and an endpoint
//! name contain no `/`, so `rsplit_once('/')` splits every key unambiguously
//! even though a node FQN or a scope id may itself contain `/`.

use std::collections::{BTreeMap, BTreeSet};

use ros_launch_manifest_model::{PathContract, SystemModel};
use ros_launch_manifest_sched::{ConcurrencyContract, Criticality, EffectiveTrigger, MapperMiss};

/// One declared node path with its facts resolved from the model.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PathFacts {
    pub name: String,
    /// The trigger fact the model carries for this path, or `None` when the
    /// model carries none. `None` is NOT a timer and NOT an input trigger:
    /// see [`PathFacts::effective_trigger`].
    pub trigger: Option<EffectiveTrigger>,
    /// Endpoint refs the path takes from (the Input trigger's endpoints).
    pub inputs: Vec<String>,
    /// Endpoint refs the path publishes.
    pub outputs: Vec<String>,
    pub max_latency_ms: Option<f64>,
    pub max_jitter_ms: Option<f64>,
    pub miss: Option<MapperMiss>,
}

impl PathFacts {
    /// The trigger the derivation ranks by. A model entry with no trigger
    /// fact is `Unclassified` -- never a timer, whatever its outputs promise
    /// as `min_rate_hz`, and never an input trigger, whatever `input` lists
    /// (the same reading as `PathContract::effective_trigger`).
    /// The error case must stay loud: `Unclassified` ranks nothing, and
    /// `DeriveReport::paths_without_trigger` names the path.
    pub fn effective_trigger(&self) -> EffectiveTrigger {
        self.trigger
            .clone()
            .unwrap_or(EffectiveTrigger::Unclassified)
    }

    /// The timer rate, for a `Timer` trigger with a positive rate.
    pub fn timer_rate_hz(&self) -> Option<f64> {
        match self.effective_trigger() {
            EffectiveTrigger::Timer { rate_hz } if rate_hz > 0.0 => Some(rate_hz),
            _ => None,
        }
    }
}

/// One node of `structure.nodes` with the contract facts keyed to it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NodeView {
    pub fqn: String,
    pub scope: String,
    /// The effective criticality when the model carries one, else the
    /// advisory label parsed -- see [`node_criticality`].
    pub criticality: Option<Criticality>,
    /// Declared paths by name.
    pub paths: BTreeMap<String, PathFacts>,
    /// The declared exclusion relation; absent means every path serialises.
    pub concurrency: Option<ConcurrencyContract>,
    /// `max_response_ms` of every service this node serves.
    pub srv_max_response_ms: Vec<f64>,
}

impl NodeView {
    /// Worst-case `max_latency_ms` across this node's paths, 0.0 when no path
    /// declares one. The FALLBACK cost of a traversal no declared path
    /// accounts for.
    pub fn max_latency_ms(&self) -> f64 {
        self.paths
            .values()
            .filter_map(|p| p.max_latency_ms)
            .fold(0.0_f64, f64::max)
    }

    /// Names of paths whose `output` publishes `ep_ref`.
    pub fn paths_producing(&self, ep_ref: &str) -> Vec<&str> {
        self.paths
            .iter()
            .filter(|(_, p)| {
                p.outputs
                    .iter()
                    .any(|o| endpoint_matches(o, &self.fqn, ep_ref))
            })
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// Names of paths whose EFFECTIVE trigger consumes `ep_ref`. Timer, once,
    /// spontaneous and unclassified paths consume nothing: a route cannot
    /// arrive into them, which is what makes a timer path a chain boundary.
    pub fn paths_consuming(&self, ep_ref: &str) -> Vec<&str> {
        self.paths
            .iter()
            .filter(|(_, p)| match p.effective_trigger() {
                EffectiveTrigger::Input(eps) => {
                    eps.iter().any(|e| endpoint_matches(e, &self.fqn, ep_ref))
                }
                _ => false,
            })
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// Latency charged for traversing this node by way of `path`; `None`
    /// charges the node-wide maximum. A timer path is a clock boundary and
    /// costs `period + exec`: a message arriving at an arbitrary point in the
    /// period waits up to a whole period for the callback that forwards it.
    pub fn traversal_latency_ms(&self, path: Option<&str>) -> f64 {
        let Some(facts) = path.and_then(|name| self.paths.get(name)) else {
            return self.max_latency_ms();
        };
        let exec = facts.max_latency_ms.unwrap_or(0.0);
        match facts.timer_rate_hz() {
            Some(rate_hz) => 1000.0 / rate_hz + exec,
            None => exec,
        }
    }

    /// The sampling half of [`NodeView::traversal_latency_ms`]: one period
    /// for a timer path, 0 otherwise.
    pub fn sampling_cost_ms(&self, path: Option<&str>) -> f64 {
        path.and_then(|name| self.paths.get(name))
            .and_then(|p| p.timer_rate_hz())
            .map_or(0.0, |rate_hz| 1000.0 / rate_hz)
    }
}

/// A topic's wiring plus the one channel fact the route needs.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TopicView {
    /// Publisher endpoint refs.
    pub publishers: Vec<String>,
    /// Subscriber endpoint refs.
    pub subscribers: Vec<String>,
    /// Worst-case transport for this hop; an undeclared hop contributes 0.
    pub max_transport_ms: Option<f64>,
}

/// One `contracts.scope_paths` entry: the requirement a chain is derived for.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ScopePathView {
    /// The model key, `"<scope id>/<name>"`.
    pub key: String,
    pub scope: String,
    pub name: String,
    /// Topic FQNs.
    pub inputs: Vec<String>,
    /// Topic FQNs.
    pub outputs: Vec<String>,
    pub max_latency_ms: Option<f64>,
}

/// The model as the derivation walks it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ModelView {
    /// In `structure.nodes` order, which is launch-traversal order.
    pub nodes: Vec<NodeView>,
    pub topics: BTreeMap<String, TopicView>,
    /// Subscriber endpoint refs declared `state: true` (polled, not causal).
    pub state_subs: BTreeSet<String>,
    /// Scope id -> parent scope id (`None` at a root).
    pub scope_parent: BTreeMap<String, Option<String>>,
    /// In model key order.
    pub scope_paths: Vec<ScopePathView>,
}

impl ModelView {
    pub fn node(&self, fqn: &str) -> Option<&NodeView> {
        self.nodes.iter().find(|n| n.fqn == fqn)
    }

    /// Read every fact the derivation needs out of the model.
    pub fn from_model(model: &SystemModel) -> Self {
        // Node paths, grouped by owner. One pass over the map rather than
        // one scan per node.
        let mut paths_by_owner: BTreeMap<&str, BTreeMap<String, PathFacts>> = BTreeMap::new();
        for (key, pc) in &model.contracts.node_paths {
            let Some((owner, name)) = key.rsplit_once('/') else {
                continue;
            };
            paths_by_owner
                .entry(owner)
                .or_default()
                .insert(name.to_string(), path_facts(name, pc));
        }

        let mut srv_by_owner: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
        for (key, sc) in &model.contracts.srv_endpoints {
            if let (Some((owner, _)), Some(ms)) = (key.rsplit_once('/'), sc.max_response_ms) {
                srv_by_owner.entry(owner).or_default().push(ms);
            }
        }

        let nodes = model
            .structure
            .nodes
            .iter()
            .map(|(fqn, node)| NodeView {
                fqn: fqn.clone(),
                scope: node.scope.clone(),
                criticality: node_criticality(model, fqn),
                paths: paths_by_owner.remove(fqn.as_str()).unwrap_or_default(),
                concurrency: model.contracts.node_concurrency.get(fqn).cloned(),
                srv_max_response_ms: srv_by_owner.remove(fqn.as_str()).unwrap_or_default(),
            })
            .collect();

        let topics = model
            .structure
            .topics
            .iter()
            .map(|(fqn, wiring)| {
                (
                    fqn.clone(),
                    TopicView {
                        publishers: wiring.publishers.clone(),
                        subscribers: wiring.subscribers.clone(),
                        max_transport_ms: model
                            .contracts
                            .topics
                            .get(fqn)
                            .and_then(|t| t.max_transport_ms),
                    },
                )
            })
            .collect();

        let state_subs = model
            .contracts
            .sub_endpoints
            .iter()
            .filter(|(_, sc)| sc.state)
            .map(|(ep_ref, _)| ep_ref.clone())
            .collect();

        let scope_parent = model
            .structure
            .scopes
            .iter()
            .map(|(id, info)| (id.clone(), info.parent.clone()))
            .collect();

        let scope_paths = model
            .contracts
            .scope_paths
            .iter()
            .filter_map(|(key, pc)| {
                let (scope, name) = key.rsplit_once('/')?;
                Some(ScopePathView {
                    key: key.clone(),
                    scope: scope.to_string(),
                    name: name.to_string(),
                    inputs: pc.input.clone(),
                    outputs: pc.output.clone(),
                    max_latency_ms: pc.max_latency_ms,
                })
            })
            .collect();

        ModelView {
            nodes,
            topics,
            state_subs,
            scope_parent,
            scope_paths,
        }
    }
}

fn path_facts(name: &str, pc: &PathContract) -> PathFacts {
    PathFacts {
        name: name.to_string(),
        trigger: path_trigger(pc),
        inputs: pc.input.clone(),
        outputs: pc.output.clone(),
        max_latency_ms: pc.max_latency_ms,
        max_jitter_ms: pc.max_jitter_ms,
        miss: pc.miss.clone(),
    }
}

/// The trigger fact a model path carries: `PathContract::trigger`, the
/// value of `PathDecl::effective_trigger()` at resolve time (design issue
/// #52, R1). `None` on the wire is a model resolved before that field
/// existed; the resolver of that era lowered Timer, Once, Spontaneous and
/// Unclassified alike to `input: []`, so nothing else on the path can say
/// what fires it, and [`PathFacts::effective_trigger`] reads it as
/// `Unclassified`.
fn path_trigger(pc: &PathContract) -> Option<EffectiveTrigger> {
    pc.trigger.clone()
}

/// The criticality the mapper ranks a node by.
///
/// `Contracts::node_criticality` first: the EFFECTIVE value after the
/// phase-72 derivation (hazards decide, the label only where none reaches),
/// keyed by node FQN. The advisory label on `NodeInstance` second, parsed
/// with [`parse_criticality_label`], for a model whose resolver wrote no
/// map -- and there the label is all the checker had too.
fn node_criticality(model: &SystemModel, fqn: &str) -> Option<Criticality> {
    if let Some(c) = model.contracts.node_criticality.get(fqn) {
        return Some(*c);
    }
    model
        .structure
        .nodes
        .get(fqn)
        .and_then(|n| n.criticality.as_deref())
        .and_then(parse_criticality_label)
}

/// `high` | `medium` | `low`, case-insensitive; anything else is `None`
/// (advisory, never an error -- the same rule as play_launch's
/// `sched_derive::parse_criticality`).
pub fn parse_criticality_label(raw: &str) -> Option<Criticality> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "high" => Some(Criticality::High),
        "medium" => Some(Criticality::Medium),
        "low" => Some(Criticality::Low),
        _ => None,
    }
}

/// Does a path's declared endpoint name `declared` denote the endpoint ref
/// `ep_ref` of node `node_fqn`?
///
/// The model spells endpoints as refs (`"<node FQN>/<endpoint>"`), and a ref
/// matches itself. A bare name (no `/`) is the manifest's spelling, which is
/// what `PathDecl::effective_trigger()` carries; it matches the ref whose
/// node is `node_fqn` and whose last segment is the name, so a trigger
/// lowered in either spelling resolves the same route.
pub(crate) fn endpoint_matches(declared: &str, node_fqn: &str, ep_ref: &str) -> bool {
    if declared == ep_ref {
        return true;
    }
    if declared.contains('/') {
        return false;
    }
    ep_ref
        .strip_prefix(node_fqn)
        .and_then(|rest| rest.strip_prefix('/'))
        .is_some_and(|ep| ep == declared)
}

/// `"<node FQN>/<endpoint>"` -> `(node FQN, endpoint)`; `None` when either
/// half is empty.
pub(crate) fn split_endpoint_ref(ep_ref: &str) -> Option<(&str, &str)> {
    let (node, ep) = ep_ref.rsplit_once('/')?;
    (!node.is_empty() && !ep.is_empty()).then_some((node, ep))
}
