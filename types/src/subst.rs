//! `$(var ...)` substitution engine for manifest args.
//!
//! Resolves `$(var name)` references in all string fields of a manifest
//! using a provided args map. Runs before static checks and namespace resolution.

use std::collections::{BTreeMap, HashMap};

/// Error during substitution.
#[derive(Debug, thiserror::Error)]
pub enum SubstError {
    #[error("unresolved variable: $(var {name}) — not in args")]
    Unresolved { name: String },
    #[error("required arg '{name}' has no default and was not provided")]
    RequiredArgMissing { name: String },
    #[error("malformed substitution: {expr}")]
    Malformed { expr: String },
}

/// Resolve manifest args: merge caller-provided args over manifest defaults.
///
/// Returns the final resolved args map. Errors if a required arg (default = None)
/// is missing from `caller_args`.
pub fn resolve_args(
    manifest_args: &BTreeMap<String, Option<String>>,
    caller_args: &HashMap<String, String>,
) -> Result<HashMap<String, String>, SubstError> {
    let mut resolved = HashMap::new();

    for (name, default) in manifest_args {
        if let Some(value) = caller_args.get(name) {
            resolved.insert(name.clone(), value.clone());
        } else if let Some(default_value) = default {
            resolved.insert(name.clone(), default_value.clone());
        } else {
            return Err(SubstError::RequiredArgMissing { name: name.clone() });
        }
    }

    // Also include caller args not declared in manifest (pass-through)
    for (name, value) in caller_args {
        if !resolved.contains_key(name) {
            resolved.insert(name.clone(), value.clone());
        }
    }

    Ok(resolved)
}

/// Replace all `$(var name)` in a string with values from the args map.
pub fn substitute_str(s: &str, args: &HashMap<String, String>) -> Result<String, SubstError> {
    let mut result = String::with_capacity(s.len());
    let mut remaining = s;

    while let Some(start) = remaining.find("$(var ") {
        result.push_str(&remaining[..start]);
        let after_prefix = &remaining[start + 6..]; // skip "$(var "
        let end = after_prefix
            .find(')')
            .ok_or_else(|| SubstError::Malformed {
                expr: remaining[start..].to_string(),
            })?;
        let var_name = after_prefix[..end].trim();
        let value = args.get(var_name).ok_or_else(|| SubstError::Unresolved {
            name: var_name.to_string(),
        })?;
        result.push_str(value);
        remaining = &after_prefix[end + 1..];
    }

    result.push_str(remaining);
    Ok(result)
}

/// Substitute all `$(var ...)` in a Vec<String>.
pub fn substitute_vec(
    v: &[String],
    args: &HashMap<String, String>,
) -> Result<Vec<String>, SubstError> {
    v.iter().map(|s| substitute_str(s, args)).collect()
}

/// Substitute all `$(var ...)` in an Option<String>.
pub fn substitute_opt(
    o: &Option<String>,
    args: &HashMap<String, String>,
) -> Result<Option<String>, SubstError> {
    match o {
        Some(s) => Ok(Some(substitute_str(s, args)?)),
        None => Ok(None),
    }
}

/// Substitute all `$(var ...)` references in a manifest's string fields.
///
/// This walks all topics, services, imports, exports, and endpoint references.
/// Does NOT modify node/topic keys (they are manifest-local identifiers).
pub fn substitute_manifest(
    manifest: &crate::Manifest,
    args: &HashMap<String, String>,
) -> Result<crate::Manifest, SubstError> {
    let mut m = manifest.clone();

    // Nodes: conditions
    for node in m.nodes.values_mut() {
        node.if_condition = substitute_opt(&node.if_condition, args)?;
        node.unless_condition = substitute_opt(&node.unless_condition, args)?;
        // Node-level paths: conditions + fields
        for path in node.paths.values_mut() {
            path.if_condition = substitute_opt(&path.if_condition, args)?;
            path.unless_condition = substitute_opt(&path.unless_condition, args)?;
            path.input = substitute_vec(&path.input, args)?;
            path.output = substitute_vec(&path.output, args)?;
            path.correlation = substitute_opt(&path.correlation, args)?;
        }
    }

    // Topics: conditions + fields
    for topic in m.topics.values_mut() {
        topic.if_condition = substitute_opt(&topic.if_condition, args)?;
        topic.unless_condition = substitute_opt(&topic.unless_condition, args)?;
        topic.msg_type = substitute_str(&topic.msg_type, args)?;
        topic.publishers = substitute_vec(&topic.publishers, args)?;
        topic.subscribers = substitute_vec(&topic.subscribers, args)?;
    }

    // Services: conditions + fields
    for svc in m.services.values_mut() {
        svc.if_condition = substitute_opt(&svc.if_condition, args)?;
        svc.unless_condition = substitute_opt(&svc.unless_condition, args)?;
        svc.srv_type = substitute_str(&svc.srv_type, args)?;
        svc.server = substitute_vec(&svc.server, args)?;
        svc.client = substitute_vec(&svc.client, args)?;
    }

    // Actions: conditions + fields
    for act in m.actions.values_mut() {
        act.if_condition = substitute_opt(&act.if_condition, args)?;
        act.unless_condition = substitute_opt(&act.unless_condition, args)?;
        act.action_type = substitute_str(&act.action_type, args)?;
        act.server = substitute_vec(&act.server, args)?;
        act.client = substitute_vec(&act.client, args)?;
    }

    // Imports: endpoint lists
    for members in m.imports.values_mut() {
        *members = substitute_vec(members, args)?;
    }

    // Exports: endpoint lists
    for members in m.exports.values_mut() {
        *members = substitute_vec(members, args)?;
    }

    // Scope paths: conditions + fields
    for path in m.paths.values_mut() {
        path.if_condition = substitute_opt(&path.if_condition, args)?;
        path.unless_condition = substitute_opt(&path.unless_condition, args)?;
        path.input = substitute_vec(&path.input, args)?;
        path.output = substitute_vec(&path.output, args)?;
        path.correlation = substitute_opt(&path.correlation, args)?;
    }

    // Includes: external manifest path
    for include in m.includes.values_mut() {
        if let crate::IncludeDecl::External { manifest } = include {
            *manifest = substitute_str(manifest, args)?;
        }
    }

    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_substitute_str_basic() {
        let args = HashMap::from([("name".into(), "hello".into())]);
        assert_eq!(substitute_str("$(var name)", &args).unwrap(), "hello");
        assert_eq!(
            substitute_str("prefix_$(var name)_suffix", &args).unwrap(),
            "prefix_hello_suffix"
        );
    }

    #[test]
    fn test_substitute_str_multiple() {
        let args = HashMap::from([("a".into(), "X".into()), ("b".into(), "Y".into())]);
        assert_eq!(substitute_str("$(var a)/$(var b)", &args).unwrap(), "X/Y");
    }

    #[test]
    fn test_substitute_str_no_vars() {
        let args = HashMap::new();
        assert_eq!(
            substitute_str("plain string", &args).unwrap(),
            "plain string"
        );
    }

    #[test]
    fn test_substitute_str_unresolved() {
        let args = HashMap::new();
        let err = substitute_str("$(var missing)", &args).unwrap_err();
        assert!(matches!(err, SubstError::Unresolved { .. }));
    }

    #[test]
    fn test_substitute_str_malformed() {
        let args = HashMap::new();
        let err = substitute_str("$(var unclosed", &args).unwrap_err();
        assert!(matches!(err, SubstError::Malformed { .. }));
    }

    #[test]
    fn test_resolve_args_defaults() {
        let manifest_args = BTreeMap::from([
            ("a".into(), Some("default_a".into())),
            ("b".into(), Some("default_b".into())),
        ]);
        let caller_args = HashMap::from([("a".into(), "override_a".into())]);
        let resolved = resolve_args(&manifest_args, &caller_args).unwrap();
        assert_eq!(resolved["a"], "override_a");
        assert_eq!(resolved["b"], "default_b");
    }

    #[test]
    fn test_resolve_args_required_present() {
        let manifest_args = BTreeMap::from([("req".into(), None)]);
        let caller_args = HashMap::from([("req".into(), "provided".into())]);
        let resolved = resolve_args(&manifest_args, &caller_args).unwrap();
        assert_eq!(resolved["req"], "provided");
    }

    #[test]
    fn test_resolve_args_required_missing() {
        let manifest_args = BTreeMap::from([("req".into(), None)]);
        let caller_args = HashMap::new();
        let err = resolve_args(&manifest_args, &caller_args).unwrap_err();
        assert!(matches!(err, SubstError::RequiredArgMissing { .. }));
    }

    #[test]
    fn test_resolve_args_passthrough() {
        let manifest_args = BTreeMap::new();
        let caller_args = HashMap::from([("extra".into(), "val".into())]);
        let resolved = resolve_args(&manifest_args, &caller_args).unwrap();
        assert_eq!(resolved["extra"], "val");
    }

    #[test]
    fn test_substitute_manifest_topics() {
        let yaml = r#"
args:
  topic_name: /perception/objects

version: 1
nodes:
  planner:
    sub: [objects]
topics:
  objects:
    type: autoware_perception_msgs/msg/PredictedObjects
    sub: [planner/objects]
"#;
        let manifest = crate::parse_manifest_str(yaml).unwrap();
        assert_eq!(manifest.args.len(), 1);
        assert_eq!(
            manifest.args["topic_name"],
            Some("/perception/objects".into())
        );

        // Substitution on a manifest with $(var ...) in fields
        let yaml_with_var = r#"
args:
  topic_name: /default/topic

version: 1
topics:
  input:
    type: $(var topic_name)
    pub: []
    sub: []
"#;
        let m = crate::parse_manifest_str(yaml_with_var).unwrap();
        let args = HashMap::from([("topic_name".into(), "/resolved/topic".into())]);
        let substituted = substitute_manifest(&m, &args).unwrap();
        assert_eq!(substituted.topics["input"].msg_type, "/resolved/topic");
    }
}
