//! Rule: a path's declared latency range must fit inside its `max_jitter`.
//!
//! `max_jitter` is a requirement on the SPREAD of a path's latency, and
//! `min_latency` exists so that it can be falsified (phase 67): every other
//! bound in the vocabulary is an upper one. Until phase 70 `min_latency` was
//! parsed and read by nothing, so the jitter requirement was checked only
//! against sampling jitter (`jitter-feasibility`, in the resolver) and never
//! against the path's own declared range.
//!
//! Three verdicts, and the distinction between the last two is the point:
//!
//! - `min_latency > max_latency` — a contradiction, error.
//! - both declared and `max_latency - min_latency > max_jitter` — the
//!   declarations cannot all hold, error.
//! - `max_jitter` declared, `max_latency` above it, `min_latency` ABSENT —
//!   the bound is unverifiable from declarations, info. An absent floor is
//!   not a floor of zero: an upper bound of 40ms says nothing about whether
//!   the latencies cluster at 38..40ms or range over 0..40ms, and reading
//!   absence as zero would have turned every jitter requirement on a path
//!   with a wide budget into a hard error. That is the absent-versus-zero
//!   confusion phase 60 removed from the chain checker, and it does not get
//!   to come back here. `play_launch measure` produces the floor.

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
    if max <= jitter {
        // Whatever the floor, the spread cannot exceed the ceiling.
        return;
    }
    match min {
        Some(floor) if max - floor > jitter => ctx.error(
            rule_id,
            &format!("{yaml_path}.max_jitter"),
            format!(
                "{what} declares max_jitter ({jitter:.2}ms) but its own latency range \
                 {floor:.2}..{max:.2}ms spans {:.2}ms. The declarations cannot all hold: \
                 tighten max_latency, raise min_latency, or loosen max_jitter",
                max - floor
            ),
        ),
        Some(_) => {}
        None => ctx.emit(
            rule_id,
            crate::check::Severity::Info,
            &format!("{yaml_path}.max_jitter"),
            format!(
                "{what} declares max_jitter ({jitter:.2}ms) below its max_latency \
                 ({max:.2}ms) and no min_latency, so the spread cannot be verified from \
                 declarations — `play_launch measure` produces the floor"
            ),
        ),
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

    fn infos(yaml: &str) -> Vec<String> {
        let manifest = parse_manifest_str(yaml).unwrap();
        let graph = DataflowGraph::build(&manifest);
        let mut ctx = CheckContext::new();
        JitterRangeRule.check(&manifest, &graph, &mut ctx);
        ctx.diagnostics
            .into_iter()
            .filter(|d| d.severity == crate::Severity::Info)
            .map(|d| d.message)
            .collect()
    }

    /// An absent floor is not a floor of zero: no error, an info that says
    /// what would make the bound checkable.
    #[test]
    fn no_floor_is_unverifiable_not_wrong() {
        let yaml = "version: 1\nnodes:\n  n:\n    pub:\n      o: {}\n    paths:\n      p:\n        output: [o]\n        max_latency: 20ms\n        max_jitter: 5ms\n";
        assert!(errors(yaml).is_empty());
        let infos = infos(yaml);
        assert_eq!(infos.len(), 1, "{infos:?}");
        assert!(infos[0].contains("cannot be verified"), "{infos:?}");
    }

    /// A declared floor makes it checkable, and here it fails.
    #[test]
    fn a_declared_range_wider_than_the_jitter_bound_is_an_error() {
        let yaml = "version: 1\nnodes:\n  n:\n    pub:\n      o: {}\n    paths:\n      p:\n        output: [o]\n        max_latency: 20ms\n        min_latency: 4ms\n        max_jitter: 5ms\n";
        let errs = errors(yaml);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].contains("4.00..20.00ms spans 16.00ms"), "{errs:?}");
    }

    /// And a floor close enough to the ceiling passes.
    #[test]
    fn a_declared_range_inside_the_jitter_bound_is_clean() {
        let yaml = "version: 1\nnodes:\n  n:\n    pub:\n      o: {}\n    paths:\n      p:\n        output: [o]\n        max_latency: 20ms\n        min_latency: 16ms\n        max_jitter: 5ms\n";
        assert!(errors(yaml).is_empty());
        assert!(infos(yaml).is_empty());
    }

    /// A ceiling at or under the bound needs no floor at all.
    #[test]
    fn a_ceiling_under_the_bound_is_clean_without_a_floor() {
        let yaml = "version: 1\nnodes:\n  n:\n    pub:\n      o: {}\n    paths:\n      p:\n        output: [o]\n        max_latency: 5ms\n        max_jitter: 5ms\n";
        assert!(errors(yaml).is_empty());
        assert!(infos(yaml).is_empty());
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
        let yaml = "version: 1\npaths:\n  e2e:\n    input: [/a]\n    output: [/b]\n    max_latency: 80ms\n    min_latency: 20ms\n    max_jitter: 10ms\n";
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
