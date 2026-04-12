//! Validation rules for manifest checking.

mod causal_dag;
mod consistency;
mod dangling_entity;
mod drop_sanity;
mod endpoint_unique;
mod qos_compat;
mod rate_hierarchy;
mod satisfiability;
mod scope_budget;
mod service_type;
mod service_wiring;
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
        Box::new(scope_budget::ScopeBudgetRule),
        Box::new(causal_dag::CausalDagRule),
        Box::new(drop_sanity::DropSanityRule),
        Box::new(service_wiring::ServiceWiringRule),
        Box::new(service_type::ServiceTypeRule),
        Box::new(dangling_entity::DanglingEntityRule),
        Box::new(satisfiability::SatisfiabilityRule),
        Box::new(consistency::ConsistencyRule),
    ]
}
