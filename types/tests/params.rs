//! `nodes.<n>.params` -- the parameters a node declares: names and ROS 2
//! types, nothing else.

use ros_launch_manifest_types::{ParamDecl, ParamType, parse_manifest_str};

fn one_param(body: &str) -> String {
    format!("nodes:\n  n:\n    params:\n      p: {body}\n")
}

fn parse_err(yaml: &str) -> String {
    parse_manifest_str(yaml)
        .expect_err("must not parse")
        .to_string()
}

#[test]
fn every_ros2_parameter_type_parses() {
    for ty in ParamType::ALL {
        let m = parse_manifest_str(&one_param(&format!("{{ type: {ty} }}")))
            .unwrap_or_else(|e| panic!("`{ty}` must parse: {e}"));
        assert_eq!(m.nodes["n"].params["p"], ParamDecl { ty }, "{ty}");
    }
    let spelled: Vec<&str> = ParamType::ALL.map(ParamType::as_str).to_vec();
    assert_eq!(
        spelled,
        [
            "bool",
            "integer",
            "double",
            "string",
            "byte_array",
            "bool_array",
            "integer_array",
            "double_array",
            "string_array"
        ]
    );
}

/// The phase-446 design's own example, dotted names included.
#[test]
fn the_design_example_parses() {
    let yaml = r#"
nodes:
  mrm_handler:
    params:
      update_rate: { type: integer }
      timeout_operation_mode_availability: { type: double }
      use_emergency_holding: { type: bool }
      turning_hazard_on.emergency: { type: bool }
"#;
    let m = parse_manifest_str(yaml).unwrap();
    let p = &m.nodes["mrm_handler"].params;
    assert_eq!(p.len(), 4);
    assert_eq!(p["update_rate"].ty, ParamType::Integer);
    assert_eq!(
        p["timeout_operation_mode_availability"].ty,
        ParamType::Double
    );
    assert_eq!(p["use_emergency_holding"].ty, ParamType::Bool);
    assert_eq!(p["turning_hazard_on.emergency"].ty, ParamType::Bool);
}

#[test]
fn an_unknown_type_is_an_error_not_a_skip() {
    let err = parse_err(&one_param("{ type: float }"));
    assert!(err.contains("nodes.n.params.p.type"), "{err}");
    assert!(
        err.contains("`float` is not a ROS 2 parameter type"),
        "{err}"
    );
    assert!(
        err.contains("double_array"),
        "lists what is accepted: {err}"
    );
}

#[test]
fn a_missing_type_is_an_error() {
    for body in ["{}", "", "{ type: }"] {
        let err = parse_err(&one_param(body));
        assert!(
            err.contains("nodes.n.params.p.type") && err.contains("missing"),
            "`p: {body}` -> {err}"
        );
    }
}

#[test]
fn a_type_of_the_wrong_yaml_kind_is_an_error() {
    let err = parse_err(&one_param("{ type: 3 }"));
    assert!(err.contains("expected a string"), "{err}");
}

/// The shorthand is refused with the spelling to use, so the map form stays
/// the one grammar.
#[test]
fn the_bare_type_shorthand_is_refused_with_the_fix() {
    let err = parse_err(&one_param("integer"));
    assert!(err.contains("write `{ type: integer }`"), "{err}");
}

/// Size bounds are a board fact; the contract has no key for one.
#[test]
fn a_size_bound_is_an_unknown_key() {
    let err = parse_err(&one_param("{ type: string, max_len: 64 }"));
    assert!(
        err.contains("unknown key") && err.contains("params.<name>"),
        "{err}"
    );
}

#[test]
fn params_must_be_a_mapping() {
    let err = parse_err("nodes:\n  n:\n    params: [a, b]\n");
    assert!(err.contains("nodes.n.params"), "{err}");
    assert!(err.contains("a mapping of parameter name"), "{err}");
}

/// A contract without `params:` parses exactly as before: an empty map, and
/// nothing new in what it serializes to.
#[test]
fn an_absent_section_changes_nothing() {
    let yaml = "nodes:\n  n:\n    pub: [out]\n    lifecycle: true\n";
    let m = parse_manifest_str(yaml).unwrap();
    assert!(m.nodes["n"].params.is_empty());
    let json = serde_json::to_value(&m).unwrap();
    assert_eq!(
        json["nodes"]["n"],
        serde_json::json!({"lifecycle": true, "pub": {"out": {}}})
    );
}

/// Serialized, a declaration is the shape it was written in, and it parses
/// back to the same declaration (JSON is YAML).
#[test]
fn a_declaration_round_trips() {
    let yaml = r#"
nodes:
  n:
    params:
      a: { type: bool }
      b: { type: integer }
      c: { type: double }
      d: { type: string }
      e: { type: byte_array }
      f: { type: bool_array }
      g: { type: integer_array }
      h: { type: double_array }
      i: { type: string_array }
"#;
    let m = parse_manifest_str(yaml).unwrap();
    let json = serde_json::to_value(&m).unwrap();
    assert_eq!(
        json["nodes"]["n"]["params"]["e"],
        serde_json::json!({"type": "byte_array"})
    );
    let back = parse_manifest_str(&serde_json::to_string(&m).unwrap()).unwrap();
    assert_eq!(back.nodes["n"].params, m.nodes["n"].params);
    assert_eq!(back.nodes["n"].params.len(), 9);
}
