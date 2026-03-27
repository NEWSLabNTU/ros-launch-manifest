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

    // Filter node-level paths too
    for node in manifest.nodes.values_mut() {
        node.paths.retain(|_, path| {
            should_include(
                path.if_condition.as_deref(),
                path.unless_condition.as_deref(),
            )
        });
    }
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
  disabled_topic:
    if: "false"
    type: std_msgs/msg/String
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
}
