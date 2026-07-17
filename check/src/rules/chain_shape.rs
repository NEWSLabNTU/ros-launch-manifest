//! Rule: per-manifest chain shape checks beyond what parsing already
//! validates (Vocabulary v2, Phase 44.1 §4).
//!
//! Parsing (`types/src/parse.rs`, W1) already rejects: empty
//! `segments:`, a first/last segment that isn't a path segment, two
//! adjacent `via` segments, and a `via` segment combined with
//! `scope`/`path`. What's left for a single-manifest view:
//!
//! - **Cyclic chains** (error): the same `{scope, path}` pair referenced
//!   by two different path segments of one chain. A chain composes a
//!   forward causal walk; revisiting a segment is a feedback loop, not a
//!   chain (chain-aware-mapper spec: "Cyclic chain declarations are
//!   rejected — a feedback loop is not a chain").
//!
//! Cross-file `{scope, path}` existence and `via` topic linkage
//! (output-of-preceding / consumed-by-following) require the merged
//! `ManifestIndex` and are NOT checkable from a single manifest — that's
//! `play_launch`'s cross-scope `chain-link` rule
//! (`src/play_launch/src/ros/chain_checks.rs`).

use super::ValidationRule;
use crate::{CheckContext, graph::DataflowGraph};
use ros_launch_manifest_types::{ChainSegment, Manifest};
use std::collections::HashSet;

pub struct ChainShapeRule;

impl ValidationRule for ChainShapeRule {
    fn id(&self) -> &str {
        "chain-shape"
    }

    fn check(&self, manifest: &Manifest, _graph: &DataflowGraph, ctx: &mut CheckContext) {
        for (chain_name, chain) in &manifest.chains {
            let mut seen: HashSet<(&str, &str)> = HashSet::new();
            for seg in &chain.segments {
                if let ChainSegment::Path { scope, path } = seg {
                    let key = (scope.as_str(), path.as_str());
                    if !seen.insert(key) {
                        ctx.error(
                            self.id(),
                            &format!("chains.{chain_name}"),
                            format!(
                                "chain '{chain_name}' references segment (scope='{scope}', \
                                 path='{path}') more than once — a feedback loop is not a \
                                 chain (cyclic chain declarations are rejected)"
                            ),
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CheckContext;
    use ros_launch_manifest_types::parse::parse_manifest_str;

    fn run(yaml: &str) -> Vec<crate::Diagnostic> {
        let manifest = parse_manifest_str(yaml).unwrap();
        let graph = DataflowGraph::build(&manifest);
        let mut ctx = CheckContext::new();
        ChainShapeRule.check(&manifest, &graph, &mut ctx);
        ctx.diagnostics
    }

    #[test]
    fn repeated_segment_errors() {
        let yaml = r#"
version: 1
chains:
  loop_chain:
    semantics: reaction
    max_latency_ms: 100
    segments:
      - { scope: /a, path: p1 }
      - { via: /t1 }
      - { scope: /b, path: p2 }
      - { via: /t2 }
      - { scope: /a, path: p1 }
"#;
        let errors: Vec<_> = run(yaml)
            .into_iter()
            .filter(|d| d.severity == crate::Severity::Error)
            .collect();
        assert_eq!(errors.len(), 1, "got: {errors:?}");
    }

    #[test]
    fn distinct_segments_do_not_error() {
        let yaml = r#"
version: 1
chains:
  ok_chain:
    semantics: reaction
    max_latency_ms: 100
    segments:
      - { scope: /a, path: p1 }
      - { via: /t1 }
      - { scope: /b, path: p2 }
"#;
        assert!(run(yaml).is_empty());
    }
}
