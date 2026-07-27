//! nano-ros #276 — `params_files` YAML projects into concrete parameters.

use ros_launch_manifest_model::{NodeInstance, ParamValue};

fn node_with(files: Vec<String>, inline: &[(&str, ParamValue)]) -> NodeInstance {
    let mut n = NodeInstance {
        params_files: files,
        ..Default::default()
    };
    for (k, v) in inline {
        n.params.insert((*k).to_string(), v.clone());
    }
    n
}

#[test]
fn wildcard_and_fqn_sections_project_with_inline_winning() {
    let yaml = r#"
/**:
  ros__parameters:
    shared_rate: 10
    qos:
      depth: 5
/ctrl/planner:
  ros__parameters:
    target_acceleration: 1.5
    mode: "sport"
    gains: [1.0, 2.0]
    enabled: true
"#;
    let n = node_with(
        vec![yaml.to_string()],
        &[("mode", ParamValue::Str("inline-wins".into()))],
    );
    let p = n.resolved_params("/ctrl/planner");

    // wildcard section applies
    assert_eq!(p.get("shared_rate"), Some(&ParamValue::Int(10)));
    // nested maps flatten with dots
    assert_eq!(p.get("qos.depth"), Some(&ParamValue::Int(5)));
    // node-specific section applies, with types preserved
    assert_eq!(p.get("target_acceleration"), Some(&ParamValue::Float(1.5)));
    assert_eq!(p.get("enabled"), Some(&ParamValue::Bool(true)));
    assert_eq!(
        p.get("gains"),
        Some(&ParamValue::StrList(vec!["1.0".into(), "2.0".into()]))
    );
    // inline `<param>` beats the file
    assert_eq!(p.get("mode"), Some(&ParamValue::Str("inline-wins".into())));
}

#[test]
fn later_file_overrides_earlier_and_foreign_sections_are_ignored() {
    let a = "/**:\n  ros__parameters:\n    rate: 1\n";
    let b = "planner:\n  ros__parameters:\n    rate: 2\n";
    let other = "/other/node:\n  ros__parameters:\n    rate: 99\n";
    let n = node_with(vec![a.to_string(), b.to_string(), other.to_string()], &[]);
    let p = n.resolved_params("/ctrl/planner");
    // bare-name section matches; later file wins; the foreign node is skipped
    assert_eq!(p.get("rate"), Some(&ParamValue::Int(2)));
}

#[test]
fn unparsable_file_is_skipped_not_fatal() {
    let n = node_with(vec!["{{{ not yaml".to_string()], &[]);
    assert!(n.resolved_params("/x").is_empty());
}

#[test]
fn rcl_style_wildcard_sections_match() {
    // Partial wildcards are what real Autoware configs use; rcl's
    // `rcl_yaml_param_parser` accepts `**` (any depth) and `*` (one segment).
    let yaml = r#"
/sensing/**:
  ros__parameters:
    from_subtree: 1
/*/planner:
  ros__parameters:
    from_single_seg: 2
/other/**:
  ros__parameters:
    must_not_apply: 3
"#;
    let n = node_with(vec![yaml.to_string()], &[]);

    let deep = n.resolved_params("/sensing/lidar/driver");
    assert_eq!(deep.get("from_subtree"), Some(&ParamValue::Int(1)));
    assert!(deep.get("must_not_apply").is_none(), "{deep:?}");

    let one = n.resolved_params("/ctrl/planner");
    assert_eq!(one.get("from_single_seg"), Some(&ParamValue::Int(2)));
    assert!(one.get("from_subtree").is_none(), "{one:?}");

    // `**` also matches zero segments: /sensing itself.
    let zero = n.resolved_params("/sensing");
    assert_eq!(zero.get("from_subtree"), Some(&ParamValue::Int(1)));

    // A single-segment wildcard must NOT span two segments.
    let too_deep = n.resolved_params("/a/b/planner");
    assert!(too_deep.get("from_single_seg").is_none(), "{too_deep:?}");
}
