//! Rule: every node endpoint in a path must be wired by a topic.

use super::ValidationRule;
use crate::{CheckContext, graph::DataflowGraph};
use ros_launch_manifest_types::Manifest;
use std::collections::HashSet;

pub struct WiringRule;

impl ValidationRule for WiringRule {
    fn id(&self) -> &str {
        "wiring"
    }

    fn check(&self, manifest: &Manifest, _graph: &DataflowGraph, ctx: &mut CheckContext) {
        // Collect all endpoint references from topic wiring
        let mut wired_endpoints: HashSet<String> = HashSet::new();
        for topic in manifest.topics.values() {
            for ep in topic.publishers.iter().chain(topic.subscribers.iter()) {
                wired_endpoints.insert(ep.clone());
            }
        }

        // Check that every path input/output endpoint is wired
        for (node_name, node) in &manifest.nodes {
            for (path_name, path) in &node.paths {
                for input_ep in &path.input {
                    let full_ref = format!("{node_name}/{input_ep}");
                    if !wired_endpoints.contains(&full_ref)
                        && !is_import_ref(input_ep, &manifest.imports)
                    {
                        ctx.warning(
                            self.id(),
                            &format!("nodes.{node_name}.paths.{path_name}"),
                            format!(
                                "path input '{input_ep}' is not wired by any topic (expected '{full_ref}' in a topic's sub list)"
                            ),
                        );
                    }
                }
                for output_ep in &path.output {
                    let full_ref = format!("{node_name}/{output_ep}");
                    if !wired_endpoints.contains(&full_ref) {
                        ctx.warning(
                            self.id(),
                            &format!("nodes.{node_name}.paths.{path_name}"),
                            format!(
                                "path output '{output_ep}' is not wired by any topic (expected '{full_ref}' in a topic's pub list)"
                            ),
                        );
                    }
                }
            }
        }

        // Check unresolved imports
        for (group_name, members) in &manifest.imports {
            for member in members {
                // Check if any topic wires this import
                let is_wired = manifest
                    .topics
                    .values()
                    .any(|t| t.subscribers.iter().any(|s| s == member || s == group_name));
                if !is_wired {
                    ctx.warning(
                        self.id(),
                        &format!("imports.{group_name}"),
                        format!("import member '{member}' is not wired by any topic's sub list"),
                    );
                }
            }
        }
    }
}

fn is_import_ref(name: &str, imports: &std::collections::BTreeMap<String, Vec<String>>) -> bool {
    imports.contains_key(name)
}
