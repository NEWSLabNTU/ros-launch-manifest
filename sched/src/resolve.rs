//! Selector-based tier resolution.

use std::collections::BTreeMap;

use crate::types::AssignRule;

/// The synthesized tier for nodes matched by no assign rule.
pub const DEFAULT_TIER: &str = "default";

/// One node as seen by the resolver — dependency-free (no parser types).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchedNode {
    /// Fully-qualified node name (e.g. `/perception/lidar/ndt_localizer`).
    pub name: String,
    /// The node's namespace / scope path (e.g. `/perception/lidar`).
    pub scope: String,
}

/// Errors from parsing or resolving the scheduling spec.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SchedError {
    #[error("failed to parse system scheduling TOML: {0}")]
    Parse(String),
    #[error("assign rule node selector `{selector}` matches no node in the system")]
    UnknownNodeSelector { selector: String },
    #[error("assign rule scope selector `{selector}` matches no node's scope in the system")]
    UnknownScopeSelector { selector: String },
    #[error(
        "node `{node}` is matched by two tiers (`{tier_a}` and `{tier_b}`); \
         a node must belong to exactly one tier"
    )]
    NodeMatchedByMultipleTiers {
        node: String,
        tier_a: String,
        tier_b: String,
    },
}

/// Normalize a namespace/scope path: ensure a single leading slash, no trailing.
fn norm_scope(s: &str) -> String {
    let trimmed = s.trim_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        format!("/{trimmed}")
    }
}

/// A node selector matches a node by full name or by its bare (last-segment) name.
fn node_selector_matches(selector: &str, node: &SchedNode) -> bool {
    node.name == selector || node.name.rsplit('/').next() == Some(selector)
}

/// A scope selector matches a node whose scope equals it or is a descendant.
fn scope_selector_matches(selector: &str, node: &SchedNode) -> bool {
    let sel = norm_scope(selector);
    let ns = norm_scope(&node.scope);
    ns == sel || ns.starts_with(&format!("{}/", sel.trim_end_matches('/')))
}

/// Insert a node→tier assignment; error if it conflicts with a prior one.
fn assign_one<'a>(
    map: &mut BTreeMap<String, &'a str>,
    node: &str,
    tier: &'a str,
) -> Result<(), SchedError> {
    if let Some(prev) = map.insert(node.to_string(), tier) {
        if prev != tier {
            return Err(SchedError::NodeMatchedByMultipleTiers {
                node: node.to_string(),
                tier_a: prev.to_string(),
                tier_b: tier.to_string(),
            });
        }
    }
    Ok(())
}

/// Build tier-name → sorted member node names. Unmatched nodes → `DEFAULT_TIER`.
pub(crate) fn bind_nodes(
    assigns: &[AssignRule],
    nodes: &[SchedNode],
) -> Result<BTreeMap<String, Vec<String>>, SchedError> {
    let mut node_tier: BTreeMap<String, &str> = BTreeMap::new();

    // Pass 1: explicit node selectors (highest precedence).
    for rule in assigns {
        for sel in &rule.nodes {
            let hits: Vec<&SchedNode> =
                nodes.iter().filter(|n| node_selector_matches(sel, n)).collect();
            if hits.is_empty() {
                return Err(SchedError::UnknownNodeSelector {
                    selector: sel.clone(),
                });
            }
            for n in hits {
                assign_one(&mut node_tier, &n.name, &rule.tier)?;
            }
        }
    }

    // Pass 2: scope selectors — fill only nodes still unassigned by pass 1.
    for rule in assigns {
        if let Some(scope_sel) = &rule.scope {
            let hits: Vec<&SchedNode> = nodes
                .iter()
                .filter(|n| scope_selector_matches(scope_sel, n))
                .collect();
            if hits.is_empty() {
                return Err(SchedError::UnknownScopeSelector {
                    selector: scope_sel.clone(),
                });
            }
            for n in hits {
                if node_tier.contains_key(&n.name) {
                    continue; // explicit node rule wins
                }
                assign_one(&mut node_tier, &n.name, &rule.tier)?;
            }
        }
    }

    // Collect members, defaulting the unmatched.
    let mut members: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for n in nodes {
        let tier = node_tier
            .get(&n.name)
            .copied()
            .unwrap_or(DEFAULT_TIER)
            .to_string();
        members.entry(tier).or_default().push(n.name.clone());
    }
    for v in members.values_mut() {
        v.sort();
    }
    Ok(members)
}

#[cfg(test)]
mod bind_tests {
    use super::*;

    fn node(name: &str, scope: &str) -> SchedNode {
        SchedNode {
            name: name.to_string(),
            scope: scope.to_string(),
        }
    }

    fn rule_nodes(tier: &str, names: &[&str]) -> AssignRule {
        AssignRule {
            tier: tier.to_string(),
            nodes: names.iter().map(|s| s.to_string()).collect(),
            scope: None,
        }
    }

    fn rule_scope(tier: &str, scope: &str) -> AssignRule {
        AssignRule {
            tier: tier.to_string(),
            nodes: vec![],
            scope: Some(scope.to_string()),
        }
    }

    #[test]
    fn unmatched_nodes_go_to_default() {
        let nodes = vec![node("/a", "/"), node("/b", "/")];
        let m = bind_nodes(&[], &nodes).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m[DEFAULT_TIER], vec!["/a".to_string(), "/b".to_string()]);
    }

    #[test]
    fn explicit_node_selector_by_bare_name() {
        let nodes = vec![node("/ns/ndt_localizer", "/ns"), node("/ns/other", "/ns")];
        let m = bind_nodes(&[rule_nodes("control", &["ndt_localizer"])], &nodes).unwrap();
        assert_eq!(m["control"], vec!["/ns/ndt_localizer".to_string()]);
        assert_eq!(m[DEFAULT_TIER], vec!["/ns/other".to_string()]);
    }

    #[test]
    fn scope_selector_matches_subtree() {
        let nodes = vec![
            node("/perception/lidar/a", "/perception/lidar"),
            node("/perception/lidar/deep/b", "/perception/lidar/deep"),
            node("/control/c", "/control"),
        ];
        let m = bind_nodes(&[rule_scope("perception", "/perception/lidar")], &nodes).unwrap();
        assert_eq!(
            m["perception"],
            vec![
                "/perception/lidar/a".to_string(),
                "/perception/lidar/deep/b".to_string()
            ]
        );
        assert_eq!(m[DEFAULT_TIER], vec!["/control/c".to_string()]);
    }

    #[test]
    fn explicit_node_wins_over_scope() {
        let nodes = vec![node("/p/a", "/p"), node("/p/b", "/p")];
        let assigns = vec![
            rule_scope("bg", "/p"),
            rule_nodes("control", &["/p/a"]),
        ];
        let m = bind_nodes(&assigns, &nodes).unwrap();
        assert_eq!(m["control"], vec!["/p/a".to_string()]);
        assert_eq!(m["bg"], vec!["/p/b".to_string()]);
    }

    #[test]
    fn conflicting_node_rules_error() {
        let nodes = vec![node("/a", "/")];
        let assigns = vec![rule_nodes("hi", &["/a"]), rule_nodes("lo", &["/a"])];
        let err = bind_nodes(&assigns, &nodes).unwrap_err();
        assert!(matches!(err, SchedError::NodeMatchedByMultipleTiers { .. }));
    }

    #[test]
    fn unknown_node_selector_errors() {
        let nodes = vec![node("/a", "/")];
        let err = bind_nodes(&[rule_nodes("hi", &["ghost"])], &nodes).unwrap_err();
        assert!(matches!(err, SchedError::UnknownNodeSelector { .. }));
    }

    #[test]
    fn unknown_scope_selector_errors() {
        let nodes = vec![node("/a", "/")];
        let err = bind_nodes(&[rule_scope("hi", "/nowhere")], &nodes).unwrap_err();
        assert!(matches!(err, SchedError::UnknownScopeSelector { .. }));
    }
}

