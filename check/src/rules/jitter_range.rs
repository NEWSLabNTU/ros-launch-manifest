//! Rule: a path's declared latency range must fit inside its `max_jitter`.
//!
//! `max_jitter` is a requirement on the SPREAD of a path's latency, and
//! `min_latency` exists so that it can be falsified (phase 67): every other
//! bound in the vocabulary is an upper one. Until phase 70 `min_latency` was
//! parsed and read by nothing, so the jitter requirement was checked only
//! against sampling jitter (`jitter-feasibility`, in the resolver) and never
//! against the path's own declared range.
//!
//! The check is `max_latency - min_latency > max_jitter`, with `min_latency`
//! defaulting to 0. That default is the conservative one: it says a path
//! that may take anywhere from 0 to `max_latency` has a spread of
//! `max_latency`, which is the worst the declarations allow. Declaring a
//! measured floor (`play_launch measure` produces one) only ever RELAXES
//! the verdict, never tightens it — so a path that fails this rule with no
//! `min_latency` fails on its upper bound alone.
//!
//! `min_latency > max_latency` is a contradiction and an error in its own
//! right.

use super::ValidationRule;
use crate::{CheckContext, graph::DataflowGraph};
use ros_launch_manifest_types::{Manifest, PathDecl};

pub struct JitterRangeRule;

impl ValidationRule for JitterRangeRule {
    fn id(&self) -> &str {
        "jitter-range"
    }

    fn check(&self, manifest: &Manifest, _graph: &DataflowGraph, ctx: &mut CheckContext) {
        for (node_name, node) in &manifest.nodes {
            for (path_name, path) in &node.paths {
                check_path(
                    self.id(),
                    &format!("nodes.{node_name}.paths.{path_name}"),
                    &format!("path '{path_name}' on node '{node_name}'"),
                    path,
                    ctx,
                );
            }
        }
        for (path_name, path) in &manifest.paths {
            check_path(
                self.id(),
                &format!("paths.{path_name}"),
                &format!("scope path '{path_name}'"),
                path,
                ctx,
            );
        }
    }
}

fn check_path(rule_id: &str, yaml_path: &str, what: &str, path: &PathDecl, ctx: &mut CheckContext) {
    let max = path.max_latency.map(|d| d.as_millis_f64());
    let min = path.min_latency.map(|d| d.as_millis_f64());

    if let (Some(max), Some(min)) = (max, min)
        && min > max
    {
        ctx.error(
            rule_id,
            &format!("{yaml_path}.min_latency"),
            format!(
                "{what} declares min_latency ({min:.2}ms) above its own max_latency \
                 ({max:.2}ms) — a contradiction, not a requirement"
            ),
        );
        return;
    }

    let Some(jitter) = path.max_jitter.map(|d| d.as_millis_f64()) else {
        return;
    };
    let Some(max) = max else {
        // A spread requirement with no upper bound has nothing to be checked
        // against. `explicit-trigger` and the scope-path rules already ask
        // for a budget; this rule does not repeat that.
        return;
    };
    let floor = min.unwrap_or(0.0);
    let spread = max - floor;
    if spread > jitter {
        let floor_note = if min.is_some() {
            String::new()
        } else {
            " (min_latency is undeclared, so the floor is taken as 0 — declaring a \
             measured one can only relax this)"
                .to_string()
        };
        ctx.error(
            rule_id,
            &format!("{yaml_path}.max_jitter"),
            format!(
                "{what} declares max_jitter ({jitter:.2}ms) but its own latency range \
                 {floor:.2}..{max:.2}ms spans {spread:.2}ms{floor_note}. The \
                 declarations cannot all hold: tighten max_latency, raise min_latency, \
                 or loosen max_jitter"
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CheckContext;
    use ros_launch_manifest_types::parse::parse_manifest_str;

    fn errors(yaml: &str) -> Vec<String> {
        let manifest = parse_manifest_str(yaml).unwrap();
        let graph = DataflowGraph::build(&manifest);
        let mut ctx = CheckContext::new();
        JitterRangeRule.check(&manifest, &graph, &mut ctx);
        ctx.diagnostics
            .into_iter()
            .filter(|d| d.severity == crate::Severity::Error)
            .map(|d| d.message)
            .collect()
    }

    /// With no floor declared, the spread is the whole upper bound.
    #[test]
    fn no_floor_means_the_spread_is_max_latency() {
        let yaml = "version: 1\nnodes:\n  n:\n    pub:\n      o: {}\n    paths:\n      p:\n        output: [o]\n        max_latency: 20ms\n        max_jitter: 5ms\n";
        let errs = errors(yaml);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].contains("0.00..20.00ms spans 20.00ms"), "{errs:?}");
        assert!(errs[0].contains("undeclared"), "{errs:?}");
    }

    /// Declaring a floor relaxes the verdict — and only relaxes it.
    #[test]
    fn a_declared_floor_can_make_the_same_path_clean() {
        let yaml = "version: 1\nnodes:\n  n:\n    pub:\n      o: {}\n    paths:\n      p:\n        output: [o]\n        max_latency: 20ms\n        min_latency: 16ms\n        max_jitter: 5ms\n";
        assert!(errors(yaml).is_empty());
    }

    #[test]
    fn a_floor_above_the_ceiling_is_a_contradiction() {
        let yaml = "version: 1\nnodes:\n  n:\n    pub:\n      o: {}\n    paths:\n      p:\n        output: [o]\n        max_latency: 10ms\n        min_latency: 12ms\n";
        let errs = errors(yaml);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].contains("contradiction"), "{errs:?}");
    }

    #[test]
    fn scope_paths_are_checked_too() {
        let yaml = "version: 1\npaths:\n  e2e:\n    input: [/a]\n    output: [/b]\n    max_latency: 80ms\n    max_jitter: 10ms\n";
        let errs = errors(yaml);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].starts_with("scope path 'e2e'"), "{errs:?}");
    }

    #[test]
    fn a_jitter_bound_without_a_budget_is_not_this_rules_business() {
        let yaml = "version: 1\nnodes:\n  n:\n    pub:\n      o: {}\n    paths:\n      p:\n        output: [o]\n        max_jitter: 5ms\n";
        assert!(errors(yaml).is_empty());
    }
}
