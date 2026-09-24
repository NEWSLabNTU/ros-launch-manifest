//! Condition evaluator for `if:` and `unless:` fields.
//!
//! Supports two forms:
//! - **Boolean**: bare string → `"true"` means true, anything else false
//! - **Expression**: `value == value`, `value != value`, `and`, `or`, parentheses
//!
//! All comparisons are string equality. Mirrors ROS 2 launch XML `if=`/`unless=`.

/// Evaluate a condition expression.
///
/// Boolean form: a bare string (no operator) is compared to `"true"`.
/// Expression form: supports `==`, `!=`, `and`, `or`, parentheses.
pub fn evaluate(expr: &str) -> bool {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return false;
    }
    match parse_expr(trimmed) {
        Some((result, rest)) if rest.trim().is_empty() => result,
        _ => {
            // If parsing fails, fall back to boolean interpretation
            trimmed == "true"
        }
    }
}

/// Check if an entity should be included based on its `if:` and `unless:` conditions.
///
/// Returns `true` if the entity should be kept.
pub fn should_include(if_cond: Option<&str>, unless_cond: Option<&str>) -> bool {
    if let Some(expr) = if_cond
        && !evaluate(expr)
    {
        return false;
    }
    if let Some(expr) = unless_cond
        && evaluate(expr)
    {
        return false;
    }
    true
}

/// Filter a manifest in place, removing entities where conditions are false.
pub fn filter_manifest(manifest: &mut crate::Manifest) {
    // Collect conditional node names BEFORE filtering — used by cleanup
    // to infer which refs are optional (their node had a condition).
    //
    // An include is collected into the same set (issue #0043). A topic names
    // a child scope's group as `include_name/group_name`, which is the same
    // `owner/member` shape a node endpoint ref has, so a ref into an include
    // that a condition removed has to be dropped for exactly the reason a ref
    // into a removed node is — and a ref into an UNCONDITIONAL missing one
    // has to survive for exactly the reason that one does, or the cleanup
    // starts hiding typos.
    let mut conditional_nodes: std::collections::HashSet<String> = manifest
        .nodes
        .iter()
        .filter(|(_, node)| node.if_condition.is_some() || node.unless_condition.is_some())
        .map(|(name, _)| name.clone())
        .collect();
    conditional_nodes.extend(
        manifest
            .includes
            .iter()
            .filter(|(_, inc)| inc.if_condition.is_some() || inc.unless_condition.is_some())
            .map(|(name, _)| name.clone()),
    );

    manifest.nodes.retain(|_, node| {
        should_include(
            node.if_condition.as_deref(),
            node.unless_condition.as_deref(),
        )
    });
    manifest.topics.retain(|_, topic| {
        should_include(
            topic.if_condition.as_deref(),
            topic.unless_condition.as_deref(),
        )
    });
    manifest.services.retain(|_, svc| {
        should_include(svc.if_condition.as_deref(), svc.unless_condition.as_deref())
    });
    manifest.actions.retain(|_, act| {
        should_include(act.if_condition.as_deref(), act.unless_condition.as_deref())
    });
    manifest.paths.retain(|_, path| {
        should_include(
            path.if_condition.as_deref(),
            path.unless_condition.as_deref(),
        )
    });
    manifest.includes.retain(|_, inc| {
        should_include(inc.if_condition.as_deref(), inc.unless_condition.as_deref())
    });

    // Filter node-level paths too
    for node in manifest.nodes.values_mut() {
        node.paths.retain(|_, path| {
            should_include(
                path.if_condition.as_deref(),
                path.unless_condition.as_deref(),
            )
        });
    }

    // Clear conditions on surviving entities — they've been evaluated
    for node in manifest.nodes.values_mut() {
        node.if_condition = None;
        node.unless_condition = None;
        for path in node.paths.values_mut() {
            path.if_condition = None;
            path.unless_condition = None;
        }
    }
    for topic in manifest.topics.values_mut() {
        topic.if_condition = None;
        topic.unless_condition = None;
    }
    for svc in manifest.services.values_mut() {
        svc.if_condition = None;
        svc.unless_condition = None;
    }
    for act in manifest.actions.values_mut() {
        act.if_condition = None;
        act.unless_condition = None;
    }
    for path in manifest.paths.values_mut() {
        path.if_condition = None;
        path.unless_condition = None;
    }
    for inc in manifest.includes.values_mut() {
        inc.if_condition = None;
        inc.unless_condition = None;
    }

    // Recurse into the inline includes that survived. An inline include is a
    // nested scope -- `<group>` in the launch file -- so a condition on a node
    // INSIDE one is as real as a condition on the group, and evaluating the
    // outer while ignoring the inner would be a contract that mirrors only the
    // top level of the launch tree. Before conditional includes existed this
    // was invisible: an inner conditional node simply survived with its
    // condition still set, which is also the one place a surviving entity kept
    // one. Recursion happens AFTER the outer retain, so a dropped include is
    // never walked, and each nested manifest gets its own ref cleanup against
    // its own node set.
    for inc in manifest.includes.values_mut() {
        if let crate::types::IncludeKind::Inline(inner) = &mut inc.kind {
            filter_manifest(inner);
        }
    }

    // Clean up dangling endpoint references
    cleanup_dangling_refs(manifest, &conditional_nodes);
}

/// Remove refs to filtered-out conditional nodes (inferred as optional).
/// Refs to unconditional nodes that are missing are kept — the checker will error.
fn cleanup_dangling_refs(
    manifest: &mut crate::Manifest,
    conditional_nodes: &std::collections::HashSet<String>,
) {
    // Owners a `owner/member` ref can name: the manifest's nodes, and the
    // child scopes its `includes:` bring in (`check/src/graph.rs` wires a
    // topic to a scope vertex through exactly that spelling).
    let node_names: std::collections::HashSet<&str> = manifest
        .nodes
        .keys()
        .chain(manifest.includes.keys())
        .map(|s| s.as_str())
        .collect();

    for topic in manifest.topics.values_mut() {
        cleanup_ref_list(&mut topic.publishers, &node_names, conditional_nodes);
        cleanup_ref_list(&mut topic.subscribers, &node_names, conditional_nodes);
    }
    for svc in manifest.services.values_mut() {
        cleanup_ref_list(&mut svc.server, &node_names, conditional_nodes);
        cleanup_ref_list(&mut svc.client, &node_names, conditional_nodes);
    }
    for act in manifest.actions.values_mut() {
        cleanup_ref_list(&mut act.server, &node_names, conditional_nodes);
        cleanup_ref_list(&mut act.client, &node_names, conditional_nodes);
    }

    // Remove empty entities (both pub and sub / both server and client gone)
    manifest
        .topics
        .retain(|_, t| !t.publishers.is_empty() || !t.subscribers.is_empty());
    manifest
        .services
        .retain(|_, s| !s.server.is_empty() || !s.client.is_empty());
    manifest
        .actions
        .retain(|_, a| !a.server.is_empty() || !a.client.is_empty());
}

/// Remove refs whose node was conditional and got filtered out (inferred optional).
/// Keep refs to unconditional nodes even if missing (checker will error — likely a typo).
fn cleanup_ref_list(
    refs: &mut Vec<String>,
    node_names: &std::collections::HashSet<&str>,
    conditional_nodes: &std::collections::HashSet<String>,
) {
    refs.retain(|r| {
        if let Some((node, _)) = r.split_once('/') {
            if node_names.contains(node) {
                // Node present — keep
                true
            } else if conditional_nodes.contains(node) {
                // Node was conditional and got filtered — drop silently
                false
            } else {
                // Node was unconditional but missing — keep (checker will error)
                true
            }
        } else {
            // No slash — not a node/endpoint ref, keep as-is
            true
        }
    });
}

// ── Expression parser ──
// Grammar:
//   expr     := or_expr
//   or_expr  := and_expr ('or' and_expr)*
//   and_expr := atom ('and' atom)*
//   atom     := '(' expr ')' | compare
//   compare  := value (('==' | '!=') value)?
//   value    := quoted_string | bare_word

fn parse_expr(input: &str) -> Option<(bool, &str)> {
    parse_or_expr(input)
}

fn parse_or_expr(input: &str) -> Option<(bool, &str)> {
    let (mut result, mut rest) = parse_and_expr(input)?;
    loop {
        let trimmed = rest.trim_start();
        if let Some(after) = trimmed.strip_prefix("or") {
            if after.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
                break; // "or" is part of a word like "orange"
            }
            let (rhs, new_rest) = parse_and_expr(after)?;
            result = result || rhs;
            rest = new_rest;
        } else {
            break;
        }
    }
    Some((result, rest))
}

fn parse_and_expr(input: &str) -> Option<(bool, &str)> {
    let (mut result, mut rest) = parse_atom(input)?;
    loop {
        let trimmed = rest.trim_start();
        if let Some(after) = trimmed.strip_prefix("and") {
            if after.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
                break; // "and" is part of a word like "android"
            }
            let (rhs, new_rest) = parse_atom(after)?;
            result = result && rhs;
            rest = new_rest;
        } else {
            break;
        }
    }
    Some((result, rest))
}

fn parse_atom(input: &str) -> Option<(bool, &str)> {
    let trimmed = input.trim_start();

    // Parenthesized expression
    if let Some(inner) = trimmed.strip_prefix('(') {
        let (result, rest) = parse_expr(inner)?;
        let rest = rest.trim_start().strip_prefix(')')?;
        return Some((result, rest));
    }

    // Value, optionally followed by comparison
    let (lhs, rest) = parse_value(trimmed)?;
    let rest_trimmed = rest.trim_start();

    if let Some(after) = rest_trimmed.strip_prefix("==") {
        let (rhs, rest2) = parse_value(after)?;
        Some((lhs == rhs, rest2))
    } else if let Some(after) = rest_trimmed.strip_prefix("!=") {
        let (rhs, rest2) = parse_value(after)?;
        Some((lhs != rhs, rest2))
    } else {
        // Bare value — boolean: "true" = true, else false
        Some((lhs == "true", rest))
    }
}

fn parse_value(input: &str) -> Option<(&str, &str)> {
    let trimmed = input.trim_start();

    // Quoted string (single or double)
    if let Some(inner) = trimmed.strip_prefix('\'') {
        let end = inner.find('\'')?;
        Some((&inner[..end], &inner[end + 1..]))
    } else if let Some(inner) = trimmed.strip_prefix('"') {
        let end = inner.find('"')?;
        Some((&inner[..end], &inner[end + 1..]))
    } else {
        // Bare word: alphanumeric, underscore, dash, dot, slash
        let end = trimmed
            .find(|c: char| !c.is_alphanumeric() && c != '_' && c != '-' && c != '.' && c != '/')
            .unwrap_or(trimmed.len());
        if end == 0 {
            return None;
        }
        Some((&trimmed[..end], &trimmed[end..]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Boolean evaluation ──

    #[test]
    fn test_bool_true() {
        assert!(evaluate("true"));
    }

    #[test]
    fn test_bool_false() {
        assert!(!evaluate("false"));
    }

    #[test]
    fn test_bool_empty() {
        assert!(!evaluate(""));
    }

    #[test]
    fn test_bool_arbitrary() {
        assert!(!evaluate("yes"));
        assert!(!evaluate("1"));
    }

    // ── Comparison ──

    #[test]
    fn test_eq_true() {
        assert!(evaluate("'true' == 'true'"));
    }

    #[test]
    fn test_eq_false() {
        assert!(!evaluate("'true' == 'false'"));
    }

    #[test]
    fn test_neq() {
        assert!(evaluate("'a' != 'b'"));
        assert!(!evaluate("'a' != 'a'"));
    }

    #[test]
    fn test_bare_words() {
        assert!(evaluate("true == true"));
        assert!(!evaluate("true == false"));
    }

    // ── Compound ──

    #[test]
    fn test_and() {
        assert!(evaluate("true and true"));
        assert!(!evaluate("true and false"));
        assert!(!evaluate("false and true"));
    }

    #[test]
    fn test_or() {
        assert!(evaluate("true or false"));
        assert!(evaluate("false or true"));
        assert!(!evaluate("false or false"));
    }

    #[test]
    fn test_compound_with_comparison() {
        assert!(evaluate("'a' == 'a' and 'b' == 'b'"));
        assert!(!evaluate("'a' == 'a' and 'b' == 'c'"));
        assert!(evaluate("'a' == 'x' or 'b' == 'b'"));
    }

    #[test]
    fn test_parentheses() {
        assert!(evaluate("(true)"));
        assert!(evaluate("(true and true)"));
        assert!(evaluate("(false or true) and true"));
        assert!(!evaluate("(false or true) and false"));
    }

    // ── should_include ──

    #[test]
    fn test_include_no_conditions() {
        assert!(should_include(None, None));
    }

    #[test]
    fn test_include_if_true() {
        assert!(should_include(Some("true"), None));
    }

    #[test]
    fn test_include_if_false() {
        assert!(!should_include(Some("false"), None));
    }

    #[test]
    fn test_include_unless_true() {
        assert!(!should_include(None, Some("true")));
    }

    #[test]
    fn test_include_unless_false() {
        assert!(should_include(None, Some("false")));
    }

    #[test]
    fn test_include_if_and_unless() {
        assert!(should_include(Some("true"), Some("false")));
        assert!(!should_include(Some("true"), Some("true")));
        assert!(!should_include(Some("false"), Some("false")));
    }

    // ── filter_manifest ──

    #[test]
    fn test_filter_manifest() {
        let yaml = r#"
version: 1
nodes:
  enabled_node:
    if: "true"
    pub: [output]
  disabled_node:
    if: "false"
    pub: [output]
  unless_enabled:
    unless: "false"
    pub: [output]
  unless_disabled:
    unless: "true"
    pub: [output]
  no_condition:
    pub: [output]
topics:
  enabled_topic:
    if: "true"
    type: std_msgs/msg/String
    pub: [enabled_node/output]
  disabled_topic:
    if: "false"
    type: std_msgs/msg/String
    pub: [disabled_node/output]
"#;
        let mut m = crate::parse_manifest_str(yaml).unwrap();
        assert_eq!(m.nodes.len(), 5);
        assert_eq!(m.topics.len(), 2);

        filter_manifest(&mut m);

        assert_eq!(m.nodes.len(), 3);
        assert!(m.nodes.contains_key("enabled_node"));
        assert!(m.nodes.contains_key("unless_enabled"));
        assert!(m.nodes.contains_key("no_condition"));
        assert!(!m.nodes.contains_key("disabled_node"));
        assert!(!m.nodes.contains_key("unless_disabled"));

        assert_eq!(m.topics.len(), 1);
        assert!(m.topics.contains_key("enabled_topic"));
    }

    // ── Optional endpoint refs (? suffix) ──

    #[test]
    fn test_conditional_ref_dropped_when_node_filtered() {
        let yaml = r#"
version: 1
nodes:
  always_node:
    pub: [output]
  conditional_node:
    if: "false"
    sub: [input]
topics:
  data:
    type: std_msgs/msg/String
    pub: [always_node/output]
    sub:
      - conditional_node/input
"#;
        let mut m = crate::parse_manifest_str(yaml).unwrap();
        filter_manifest(&mut m);

        assert!(!m.nodes.contains_key("conditional_node"));
        // Ref to conditional node silently dropped (inferred optional)
        let subs = &m.topics["data"].subscribers;
        assert!(
            subs.is_empty(),
            "conditional ref should be dropped: {subs:?}"
        );
    }

    #[test]
    fn test_conditional_ref_kept_when_node_present() {
        let yaml = r#"
version: 1
nodes:
  always_node:
    pub: [output]
  conditional_node:
    if: "true"
    sub: [input]
topics:
  data:
    type: std_msgs/msg/String
    pub: [always_node/output]
    sub:
      - conditional_node/input
"#;
        let mut m = crate::parse_manifest_str(yaml).unwrap();
        filter_manifest(&mut m);

        assert!(m.nodes.contains_key("conditional_node"));
        // Conditional node present — ref kept
        let subs = &m.topics["data"].subscribers;
        assert_eq!(subs, &vec!["conditional_node/input".to_string()]);
    }

    #[test]
    fn test_unconditional_ref_kept_even_when_node_missing() {
        // Ref to an unconditional node that somehow doesn't exist
        // (shouldn't happen, but ref is kept so checker can error)
        let yaml = r#"
version: 1
nodes:
  always_node:
    pub: [output]
topics:
  data:
    type: std_msgs/msg/String
    pub: [always_node/output]
    sub:
      - nonexistent_node/input
"#;
        let mut m = crate::parse_manifest_str(yaml).unwrap();
        filter_manifest(&mut m);

        // Ref to unconditional node kept (checker will error)
        let subs = &m.topics["data"].subscribers;
        assert_eq!(subs, &vec!["nonexistent_node/input".to_string()]);
    }

    #[test]
    fn test_mixed_conditional_and_unconditional_refs() {
        let yaml = r#"
version: 1
nodes:
  required_node:
    sub: [input]
  optional_node_a:
    if: "false"
    sub: [input]
  optional_node_b:
    if: "true"
    sub: [input]
topics:
  data:
    type: std_msgs/msg/String
    pub: []
    sub:
      - required_node/input
      - optional_node_a/input
      - optional_node_b/input
"#;
        let mut m = crate::parse_manifest_str(yaml).unwrap();
        filter_manifest(&mut m);

        let subs = &m.topics["data"].subscribers;
        assert_eq!(subs.len(), 2);
        assert_eq!(subs[0], "required_node/input"); // unconditional — kept
        assert_eq!(subs[1], "optional_node_b/input"); // conditional, node present — kept
        // optional_node_a conditional + filtered — dropped
    }

    // ── Empty entity and scope group removal ──

    #[test]
    fn test_empty_topic_removed_after_filtering() {
        let yaml = r#"
version: 1
nodes:
  opt_pub:
    if: "false"
    pub: [out]
  opt_sub:
    if: "false"
    sub: [in_data]
topics:
  gone_topic:
    type: std_msgs/msg/String
    pub: [opt_pub/out]
    sub: [opt_sub/in_data]
  half_topic:
    type: std_msgs/msg/String
    pub: [opt_pub/out]
    sub: []
"#;
        let mut m = crate::parse_manifest_str(yaml).unwrap();
        filter_manifest(&mut m);

        // gone_topic: both sides empty → removed
        assert!(!m.topics.contains_key("gone_topic"));
        // half_topic: pub empty but sub also empty → removed
        assert!(!m.topics.contains_key("half_topic"));
    }

    // ── Conditional includes (issue #0043) ──

    #[test]
    fn test_conditions_inside_an_inline_include_are_evaluated() {
        // An inline include is a nested scope — `<group>` in the launch file —
        // so a condition on a node INSIDE one is as real as a condition on the
        // group itself. Before the recursion this node survived with its
        // condition still set, which made it the one surviving entity in a
        // filtered manifest that kept one.
        let yaml = r#"
version: 1
includes:
  sensors:
    if: "true"
    nodes:
      lidar_driver:
        if: "false"
        pub:
          scan: {}
      camera_driver:
        if: "true"
        pub:
          image: {}
"#;
        let mut m = crate::parse::parse_manifest_str(yaml).unwrap();
        crate::cond::filter_manifest(&mut m);

        let inner = m.includes["sensors"].inline().expect("inline include");
        assert!(
            !inner.nodes.contains_key("lidar_driver"),
            "a false node inside an inline include must be dropped, got {:?}",
            inner.nodes.keys().collect::<Vec<_>>()
        );
        assert!(inner.nodes.contains_key("camera_driver"));
        assert!(
            inner.nodes["camera_driver"].if_condition.is_none(),
            "a surviving nested node must have its condition cleared, like every other survivor"
        );
    }

    #[test]
    fn test_conditional_include_dropped_and_refs_cleaned() {
        // Both spellings of a false condition, and a topic wired into each.
        let yaml = r#"
version: 1
nodes:
  always_node:
    pub: [output]
    sub: [feedback]
includes:
  perception:
    manifest: perception.yaml
    if: "false"
  planning:
    unless: "true"
    nodes:
      planner:
        pub: [route]
topics:
  data:
    type: std_msgs/msg/String
    pub: [always_node/output]
    sub:
      - perception/points_in
  feedback:
    type: std_msgs/msg/String
    pub: [planning/route_out]
    sub: [always_node/feedback]
"#;
        let mut m = crate::parse_manifest_str(yaml).unwrap();
        assert_eq!(m.includes.len(), 2);

        filter_manifest(&mut m);

        assert!(m.includes.is_empty(), "both includes are false");
        // The ref into the dropped external include is gone; the topic keeps
        // its publisher, so it survives with no subscriber.
        assert!(
            m.topics["data"].subscribers.is_empty(),
            "ref into a dropped include should be cleaned up: {:?}",
            m.topics["data"].subscribers
        );
        // The ref into the dropped inline include is gone, but the topic keeps
        // its other side, so it survives with no publisher.
        assert!(m.topics["feedback"].publishers.is_empty());
        assert_eq!(
            m.topics["feedback"].subscribers,
            vec!["always_node/feedback".to_string()]
        );
    }

    #[test]
    fn test_true_include_survives_with_condition_cleared() {
        let yaml = r#"
version: 1
nodes:
  always_node:
    pub: [output]
includes:
  perception:
    manifest: perception.yaml
    if: "true"
  planning:
    unless: "false"
    nodes:
      planner:
        pub: [route]
topics:
  data:
    type: std_msgs/msg/String
    pub: [always_node/output]
    sub:
      - perception/points_in
"#;
        let mut m = crate::parse_manifest_str(yaml).unwrap();
        filter_manifest(&mut m);

        assert_eq!(m.includes.len(), 2);
        for (name, inc) in &m.includes {
            assert!(
                inc.if_condition.is_none() && inc.unless_condition.is_none(),
                "condition on surviving include {name} should be cleared"
            );
        }
        assert_eq!(
            m.includes["perception"].external(),
            Some("perception.yaml"),
            "the external form keeps its target"
        );
        assert!(m.includes["planning"].inline().is_some());
        // Ref into a surviving include is kept.
        assert_eq!(
            m.topics["data"].subscribers,
            vec!["perception/points_in".to_string()]
        );
    }

    #[test]
    fn test_unconditional_missing_include_ref_kept() {
        // The negative control: without it, the cleanup above would be
        // indistinguishable from one that silently swallows typos.
        let yaml = r#"
version: 1
nodes:
  always_node:
    pub: [output]
includes:
  perception:
    manifest: perception.yaml
topics:
  data:
    type: std_msgs/msg/String
    pub: [always_node/output]
    sub:
      - percption/points_in
"#;
        let mut m = crate::parse_manifest_str(yaml).unwrap();
        filter_manifest(&mut m);

        assert_eq!(m.includes.len(), 1, "unconditional include survives");
        assert_eq!(
            m.topics["data"].subscribers,
            vec!["percption/points_in".to_string()],
            "a ref to no known owner is kept for the checker to error on"
        );
    }
}
