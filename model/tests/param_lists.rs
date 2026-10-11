//! Schema 2: a list parameter is a `StrList`, a `Str` is always a string, and
//! a version-1 model is upgraded on read (play_launch issue #0067).
//!
//! In version 1 an inline list rode as a `Str` of its flow text, so a string
//! whose text looked like a list (`type="str"`, `value="[a, b]"`) could not be
//! told from one, and was spawned as a list.

use ros_launch_manifest_model::{ParamSource, ParamValue, SCHEMA_VERSION, SystemModel};

fn list(items: &[&str]) -> ParamValue {
    ParamValue::StrList(items.iter().map(|s| s.to_string()).collect())
}

/// Each element is spelled so it reads back as what it was: a string that
/// would read as a number, a bool or nothing is single-quoted.
#[test]
fn a_flow_list_keeps_each_elements_type_in_its_spelling() {
    assert_eq!(
        ParamValue::list_from_flow("[a, 5, '5', true, 'true', 1.5, 1.0, '', 'it''s']"),
        Some(list(&["a", "5", "'5'", "true", "'true'", "1.5", "1.0", "''", "it's"]))
    );
    assert_eq!(ParamValue::list_from_flow("[]"), Some(list(&[])));
    // Not a flow sequence, or not one of scalars: not a list.
    assert_eq!(ParamValue::list_from_flow("a, b"), None);
    assert_eq!(ParamValue::list_from_flow("[[1], [2]]"), None);
}

const V1_MODEL: &str = r#"
meta:
  version: 1
structure:
  nodes:
    /t:
      scope: /
      pkg: demo_nodes_cpp
      exec: talker
      params:
        names: "['a', 'b']"
        plain: hello
      param_sources:
      - kind: inline
        name: ints
        value: '[1, 2]'
"#;

/// A version-1 model meant a list by flow text; read now, it is a list.
#[test]
fn a_version_1_flow_text_is_upgraded_to_a_list() {
    let m = SystemModel::from_yaml_str(V1_MODEL).unwrap();
    assert_eq!(m.meta.version, SCHEMA_VERSION);
    let node = &m.structure.nodes["/t"];
    assert_eq!(node.params["names"], list(&["a", "b"]));
    assert_eq!(node.params["plain"], ParamValue::Str("hello".into()));
    match &node.param_sources[0] {
        ParamSource::Inline { value, .. } => assert_eq!(*value, list(&["1", "2"])),
        other => panic!("{other:?}"),
    }
}

/// In schema 2 the same text is a string, and stays one.
#[test]
fn a_version_2_string_that_looks_like_a_list_stays_a_string() {
    let v2 = V1_MODEL.replace("version: 1", &format!("version: {SCHEMA_VERSION}"));
    let m = SystemModel::from_yaml_str(&v2).unwrap();
    assert_eq!(
        m.structure.nodes["/t"].params["names"],
        ParamValue::Str("['a', 'b']".into())
    );
}

/// A list written by a schema-2 producer round-trips as a list.
#[test]
fn a_list_round_trips() {
    let v2 = V1_MODEL.replace("version: 1", &format!("version: {SCHEMA_VERSION}"));
    let mut m = SystemModel::from_yaml_str(&v2).unwrap();
    m.structure
        .nodes
        .get_mut("/t")
        .unwrap()
        .params
        .insert("mixed".into(), list(&["a", "'5'", "5"]));
    let back = SystemModel::from_yaml_str(&m.to_yaml_string().unwrap()).unwrap();
    assert_eq!(back, m);
}
