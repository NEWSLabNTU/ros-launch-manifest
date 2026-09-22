//! Rule: every service client (`cli:`) must have a matching server declaration.
//!
//! For each `cli:` endpoint on a node, checks that there is a `services:` entry
//! at scope level whose `server:` list is non-empty. This catches cases where a
//! node calls a service that no one serves.
//!
//! ...unless the service says its server is EXTERNAL. `external: server` is the
//! author stating the fact this rule would otherwise guess at, and
//! `dangling-entity` — which asks the same question from the other end — has
//! honoured it since the mark existed. Two rules in one registry disagreeing
//! about whether a client-only manifest is fine pushes the author toward
//! declaring a server the image does not run, which is exactly what the mark
//! was added to avoid.

use super::{ValidationRule, dangling_entity::server_is_external};
use crate::{CheckContext, graph::DataflowGraph};
use ros_launch_manifest_types::{Manifest, ServiceDecl};

pub struct ServiceWiringRule;

impl ValidationRule for ServiceWiringRule {
    fn id(&self) -> &str {
        "service-wiring"
    }

    fn check(&self, manifest: &Manifest, _graph: &DataflowGraph, ctx: &mut CheckContext) {
        // Collect all service names that have servers — declared here, or
        // declared to be somewhere else.
        let served: std::collections::HashSet<&str> = manifest
            .services
            .iter()
            .filter(|(_, svc)| has_a_server(svc))
            .map(|(name, _)| name.as_str())
            .collect();

        // Check each node's cli: endpoints
        for (node_name, node) in &manifest.nodes {
            for cli_name in node.cli.keys() {
                // Look for a services: entry that lists this node/endpoint as a client,
                // or a services: entry with the same name as the cli endpoint
                let full_ref = format!("{node_name}/{cli_name}");
                let has_server = manifest
                    .services
                    .values()
                    .any(|svc| svc.client.contains(&full_ref) && has_a_server(svc))
                    || served.contains(cli_name.as_str());

                if !has_server {
                    ctx.warning(
                        self.id(),
                        &format!("nodes.{node_name}.cli.{cli_name}"),
                        format!("service client '{cli_name}' has no matching server in services:"),
                    );
                }
            }
        }
    }
}

/// Whether this service has a server at all: one declared in `server:`, or one
/// the author marked external.
fn has_a_server(svc: &ServiceDecl) -> bool {
    !svc.server.is_empty() || server_is_external(svc.external)
}
