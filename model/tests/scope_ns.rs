//! A scope records the namespace its file was included under (play_launch's
//! web launch tree, read off a model with no launch-time dump).

use ros_launch_manifest_model::{ScopeInfo, SystemModel};

#[test]
fn a_scope_namespace_round_trips_and_is_absent_at_the_root() {
    let mut m = SystemModel::default();
    m.structure
        .scopes
        .insert("bringup.launch.xml".into(), ScopeInfo::default());
    m.structure.scopes.insert(
        "perception.launch.xml".into(),
        ScopeInfo {
            parent: Some("bringup.launch.xml".into()),
            ns: Some("/perception".into()),
            ..Default::default()
        },
    );
    let yaml = m.to_yaml_string().unwrap();
    assert_eq!(yaml.matches("ns:").count(), 1, "{yaml}");
    let back = SystemModel::from_yaml_str(&yaml).unwrap();
    assert_eq!(back, m);
    assert_eq!(
        back.structure.scopes["perception.launch.xml"].ns.as_deref(),
        Some("/perception")
    );
}
