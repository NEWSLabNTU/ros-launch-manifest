//! Validation rules for manifest checking.

mod causal_dag;
mod drop_consecutive;
mod drop_rate;
mod endpoint_unique;
mod qos_compat;
mod rate_chain;
mod rate_hierarchy;
mod scope_budget;
mod wiring;

use crate::{check::CheckContext, graph::DataflowGraph};
use ros_launch_manifest_types::Manifest;

/// A validation rule that checks one aspect of the manifest.
pub trait ValidationRule: Send + Sync {
    fn id(&self) -> &str;
    fn check(&self, manifest: &Manifest, graph: &DataflowGraph, ctx: &mut CheckContext);
}

/// Default set of validation rules.
pub fn default_rules() -> Vec<Box<dyn ValidationRule>> {
    vec![
        Box::new(endpoint_unique::EndpointUniqueRule),
        Box::new(wiring::WiringRule),
        Box::new(qos_compat::QosCompatRule),
        Box::new(rate_hierarchy::RateHierarchyRule),
        Box::new(rate_chain::RateChainRule),
        Box::new(scope_budget::ScopeBudgetRule),
        Box::new(causal_dag::CausalDagRule),
        Box::new(drop_rate::DropRateRule),
        Box::new(drop_consecutive::DropConsecutiveRule),
    ]
}
