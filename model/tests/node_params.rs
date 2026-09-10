//! `contracts.node_params` -- the declared parameters, per node, on the
//! model -- and the type check a launch value is held to.

use ros_launch_manifest_model::{
    ParamContract, ParamType, ParamValue, SystemModel, param_file_values,
};
use std::collections::BTreeMap;

fn model_with_params() -> SystemModel {
    let mut m = SystemModel::default();
    m.contracts.node_params.insert(
        "/system/mrm_handler".to_string(),
        BTreeMap::from([
            (
                "update_rate".to_string(),
                ParamContract {
                    ty: ParamType::Integer,
                },
            ),
            (
                "turning_hazard_on.emergency".to_string(),
                ParamContract {
                    ty: ParamType::Bool,
                },
            ),
        ]),
    );
    m
}

#[test]
fn node_params_round_trip_through_yaml() {
    let m = model_with_params();
    let yaml = m.to_yaml_string().unwrap();
    assert!(yaml.contains("node_params:"), "{yaml}");
    assert!(yaml.contains("type: integer"), "{yaml}");
    let back = SystemModel::from_yaml_str(&yaml).unwrap();
    assert_eq!(back, m);
    assert_eq!(
        back.contracts.node_params["/system/mrm_handler"]["update_rate"].ty,
        ParamType::Integer
    );
}

#[test]
fn a_model_without_declarations_emits_no_key() {
    let yaml = SystemModel::default().to_yaml_string().unwrap();
    assert!(!yaml.contains("node_params"), "{yaml}");
    assert!(
        !model_with_params().contracts.is_empty(),
        "a model carrying only declarations must still serialize its contracts"
    );
}

#[test]
fn every_type_accepts_its_own_values() {
    let list = |xs: &[&str]| ParamValue::StrList(xs.iter().map(|s| s.to_string()).collect());
    let cases = [
        (ParamType::Bool, ParamValue::Bool(true)),
        (ParamType::Integer, ParamValue::Int(-3)),
        (ParamType::Double, ParamValue::Float(0.5)),
        (ParamType::String, ParamValue::Str("x".into())),
        (ParamType::ByteArray, list(&["0", "255"])),
        (ParamType::BoolArray, list(&["true", "false"])),
        (ParamType::IntegerArray, list(&["1", "-2"])),
        (ParamType::DoubleArray, list(&["1.0", "2.5"])),
        (ParamType::StringArray, list(&["a", "1"])),
        (ParamType::IntegerArray, ParamValue::Str("[1, 2]".into())),
        (ParamType::DoubleArray, list(&[])),
    ];
    for (ty, v) in cases {
        assert_eq!(ty.check(&v), Ok(()), "{ty} must accept {v:?}");
    }
}

#[test]
fn a_value_of_another_type_is_refused() {
    let list = |xs: &[&str]| ParamValue::StrList(xs.iter().map(|s| s.to_string()).collect());
    let cases = [
        (ParamType::Bool, ParamValue::Str("yes".into())),
        (ParamType::Integer, ParamValue::Float(1.5)),
        (ParamType::String, ParamValue::Int(10)),
        (ParamType::ByteArray, list(&["256"])),
        (ParamType::BoolArray, list(&["1"])),
        (ParamType::IntegerArray, list(&["1.5"])),
        (ParamType::DoubleArray, list(&["1", "2"])),
        (ParamType::StringArray, ParamValue::Str("abc".into())),
        (ParamType::Integer, list(&["1"])),
    ];
    for (ty, v) in cases {
        assert!(ty.check(&v).is_err(), "{ty} must refuse {v:?}");
    }
}

/// rclcpp refuses an integer override for a double declaration; the message
/// says how to write it.
#[test]
fn an_integer_is_not_a_double() {
    let err = ParamType::Double.check(&ParamValue::Int(5)).unwrap_err();
    assert!(err.contains("5.0"), "{err}");
}

#[test]
fn one_file_projects_onto_one_node() {
    let file = "/**:\n  ros__parameters:\n    shared: 1\n/a/n:\n  ros__parameters:\n    own: 2.0\n/a/other:\n  ros__parameters:\n    not_mine: true\n";
    let v = param_file_values(file, "/a/n");
    assert_eq!(v.get("shared"), Some(&ParamValue::Int(1)));
    assert_eq!(v.get("own"), Some(&ParamValue::Float(2.0)));
    assert!(!v.contains_key("not_mine"));
}
