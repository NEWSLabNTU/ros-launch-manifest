//! Integration tests that load YAML fixture files and run the static checker.

use ros_launch_manifest_check::{Severity, run_checks};
use ros_launch_manifest_types::{parse_manifest, parse_manifest_str};
use std::path::PathBuf;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/fixtures")
        .join(name)
        .join("manifest.yaml")
}

// ── manifest_simple: talker/listener, clean ──

#[test]
fn fixture_simple_parses() {
    let m = parse_manifest(&fixture_path("manifest_simple")).unwrap();
    assert_eq!(m.version, 1);
    assert_eq!(m.nodes.len(), 2);
    assert!(m.nodes.contains_key("talker"));
    assert!(m.nodes.contains_key("listener"));
    assert_eq!(m.topics.len(), 1);
    assert!(m.topics.contains_key("chatter"));
}

#[test]
fn fixture_simple_clean() {
    let m = parse_manifest(&fixture_path("manifest_simple")).unwrap();
    let result = run_checks(&m);
    assert!(
        !result.has_errors(),
        "simple fixture should be clean: {:?}",
        result.errors().map(|d| d.to_string()).collect::<Vec<_>>()
    );
}

// ── manifest_pipeline: perception pipeline, clean ──

#[test]
fn fixture_pipeline_parses() {
    let m = parse_manifest(&fixture_path("manifest_pipeline")).unwrap();
    assert_eq!(
        m.nodes.len(),
        4,
        "cropbox + ground_filter + fusion + tracker"
    );
    assert_eq!(m.topics.len(), 5);
    assert!(!m.imports.is_empty());
    assert!(!m.exports.is_empty());
    assert!(!m.paths.is_empty());
}

#[test]
fn fixture_pipeline_clean() {
    let m = parse_manifest(&fixture_path("manifest_pipeline")).unwrap();
    let result = run_checks(&m);
    assert!(
        !result.has_errors(),
        "pipeline fixture should be clean: {:?}",
        result.errors().map(|d| d.to_string()).collect::<Vec<_>>()
    );
}

#[test]
fn fixture_pipeline_state_endpoint() {
    let m = parse_manifest(&fixture_path("manifest_pipeline")).unwrap();
    let fusion = &m.nodes["fusion"];
    let camera = &fusion.subscribers["camera_objects"];
    assert!(
        camera.state.unwrap_or(false),
        "camera_objects should be state"
    );
}

// ── manifest_ndt: NDT localization with feedback cycle ──

#[test]
fn fixture_ndt_parses() {
    let m = parse_manifest(&fixture_path("manifest_ndt")).unwrap();
    assert_eq!(
        m.nodes.len(),
        3,
        "voxel_grid + ndt_scan_matcher + ekf_localizer"
    );
    assert!(m.services.contains_key("trigger_ndt"));
}

#[test]
fn fixture_ndt_clean() {
    let m = parse_manifest(&fixture_path("manifest_ndt")).unwrap();
    let result = run_checks(&m);
    // The EKF→NDT feedback uses state: true, so it should NOT be a causal cycle
    let cycle_errs: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "causal-dag" && d.severity == Severity::Error)
        .collect();
    assert!(
        cycle_errs.is_empty(),
        "state feedback should not create causal cycle: {cycle_errs:?}"
    );
    assert!(
        !result.has_errors(),
        "NDT fixture should be clean: {:?}",
        result.errors().map(|d| d.to_string()).collect::<Vec<_>>()
    );
}

#[test]
fn fixture_ndt_required_endpoints() {
    let m = parse_manifest(&fixture_path("manifest_ndt")).unwrap();
    let ndt = &m.nodes["ndt_scan_matcher"];
    assert!(ndt.subscribers["initial_pose"].required.unwrap_or(false));
    assert!(ndt.subscribers["map"].required.unwrap_or(false));
    assert!(ndt.subscribers["initial_pose"].state.unwrap_or(false));
    assert!(ndt.subscribers["map"].state.unwrap_or(false));
}

#[test]
fn fixture_ndt_service() {
    let m = parse_manifest(&fixture_path("manifest_ndt")).unwrap();
    let svc = &m.services["trigger_ndt"];
    assert_eq!(svc.srv_type, "std_srvs/srv/Trigger");
    assert!(
        svc.server
            .contains(&"ndt_scan_matcher/trigger_node".to_string())
    );
}

// ── manifest_periodic: timer-driven nodes ──

#[test]
fn fixture_periodic_parses() {
    let m = parse_manifest(&fixture_path("manifest_periodic")).unwrap();
    assert_eq!(m.nodes.len(), 3);
    // All paths should have empty input (timer-driven)
    for (name, node) in &m.nodes {
        for (path_name, path) in &node.paths {
            assert!(
                path.input.is_empty(),
                "node {name} path {path_name} should have empty input (periodic)"
            );
        }
    }
}

#[test]
fn fixture_periodic_clean() {
    let m = parse_manifest(&fixture_path("manifest_periodic")).unwrap();
    let result = run_checks(&m);
    assert!(
        !result.has_errors(),
        "periodic fixture should be clean: {:?}",
        result.errors().map(|d| d.to_string()).collect::<Vec<_>>()
    );
}

#[test]
fn fixture_periodic_all_state_subs() {
    let m = parse_manifest(&fixture_path("manifest_periodic")).unwrap();
    let controller = &m.nodes["controller"];
    assert!(controller.subscribers["trajectory"].state.unwrap_or(false));
    assert!(
        controller.subscribers["current_velocity"]
            .state
            .unwrap_or(false)
    );
}

// ── manifest_violations: intentional violations ──

#[test]
fn fixture_violations_parses() {
    let m = parse_manifest(&fixture_path("manifest_violations")).unwrap();
    assert!(!m.nodes.is_empty());
}

#[test]
fn fixture_violations_qos_error() {
    let m = parse_manifest(&fixture_path("manifest_violations")).unwrap();
    let result = run_checks(&m);
    let qos_errs: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "qos-compat" && d.severity == Severity::Error)
        .collect();
    assert!(
        !qos_errs.is_empty(),
        "expected QoS error for 'persistent' durability"
    );
}

#[test]
fn fixture_violations_rate_hierarchy() {
    let m = parse_manifest(&fixture_path("manifest_violations")).unwrap();
    let result = run_checks(&m);
    let rate_errs: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "rate-hierarchy" && d.severity == Severity::Error)
        .collect();
    assert!(
        rate_errs.len() >= 2,
        "expected at least 2 rate hierarchy errors (pub < topic, topic < sub): {rate_errs:?}"
    );
}

#[test]
fn fixture_violations_causal_cycle() {
    let m = parse_manifest(&fixture_path("manifest_violations")).unwrap();
    let result = run_checks(&m);
    let cycle_errs: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "causal-dag" && d.severity == Severity::Error)
        .collect();
    assert!(
        !cycle_errs.is_empty(),
        "expected causal cycle error from a↔b"
    );
}

#[test]
fn fixture_violations_scope_budget() {
    let m = parse_manifest(&fixture_path("manifest_violations")).unwrap();
    let result = run_checks(&m);
    // max_age_ms (30) < max_latency_ms (100) should be an error
    let age_errs: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "scope-budget" && d.severity == Severity::Error)
        .collect();
    assert!(!age_errs.is_empty(), "expected age < latency error");

    // Sum of node latencies (40+25+25+10+10=110) > scope latency (100) should warn
    let budget_warns: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "scope-budget" && d.severity == Severity::Warning)
        .collect();
    assert!(!budget_warns.is_empty(), "expected scope budget warning");
}

#[test]
fn fixture_violations_drop_rate() {
    let m = parse_manifest(&fixture_path("manifest_violations")).unwrap();
    let result = run_checks(&m);
    let drop_errs: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "drop-rate" && d.severity == Severity::Error)
        .collect();
    assert!(
        !drop_errs.is_empty(),
        "expected drop rate infeasibility error"
    );
}

#[test]
fn fixture_violations_drop_consecutive() {
    let m = parse_manifest(&fixture_path("manifest_violations")).unwrap();
    let result = run_checks(&m);
    // scope max_consecutive (2) < node max_consecutive (5) — error
    let consec_errs: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "drop-consecutive" && d.severity == Severity::Error)
        .collect();
    assert!(!consec_errs.is_empty(), "expected consecutive drop error");
}

#[test]
fn fixture_violations_wiring() {
    let m = parse_manifest(&fixture_path("manifest_violations")).unwrap();
    let result = run_checks(&m);
    let wiring_warns: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "wiring" && d.severity == Severity::Warning)
        .collect();
    assert!(
        !wiring_warns.is_empty(),
        "expected wiring warning for unwired_node"
    );
}

#[test]
fn fixture_violations_has_many_errors() {
    let m = parse_manifest(&fixture_path("manifest_violations")).unwrap();
    let result = run_checks(&m);
    let error_count = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    assert!(
        error_count >= 5,
        "expected at least 5 distinct errors, got {error_count}: {:?}",
        result
            .errors()
            .map(|d| format!("[{}] {}", d.rule_id, d.message))
            .collect::<Vec<_>>()
    );
}

// ── manifest_multi_scope: nested includes ──

#[test]
fn fixture_multi_scope_parses() {
    let m = parse_manifest(&fixture_path("manifest_multi_scope")).unwrap();
    assert_eq!(m.includes.len(), 2, "perception + planning");
    assert!(m.nodes.contains_key("driver"));
}

#[test]
fn fixture_multi_scope_inline_structure() {
    let m = parse_manifest(&fixture_path("manifest_multi_scope")).unwrap();
    match &m.includes["perception"] {
        ros_launch_manifest_types::IncludeDecl::Inline(inner) => {
            assert_eq!(inner.nodes.len(), 2, "cropbox + detector");
            assert!(inner.topics.contains_key("filtered_points"));
            assert!(!inner.imports.is_empty());
            assert!(!inner.exports.is_empty());
            assert!(!inner.paths.is_empty());
        }
        ros_launch_manifest_types::IncludeDecl::External { .. } => {
            panic!("perception should be inline, not external");
        }
    }
    match &m.includes["planning"] {
        ros_launch_manifest_types::IncludeDecl::Inline(inner) => {
            assert_eq!(inner.nodes.len(), 1, "planner");
            assert!(
                inner.nodes["planner"].subscribers["route"]
                    .state
                    .unwrap_or(false)
            );
            assert!(
                inner.nodes["planner"].subscribers["route"]
                    .required
                    .unwrap_or(false)
            );
        }
        ros_launch_manifest_types::IncludeDecl::External { .. } => {
            panic!("planning should be inline, not external");
        }
    }
}

#[test]
fn fixture_multi_scope_clean() {
    let m = parse_manifest(&fixture_path("manifest_multi_scope")).unwrap();
    let result = run_checks(&m);
    // Multi-scope should have no errors (inline scopes are well-formed)
    assert!(
        !result.has_errors(),
        "multi-scope fixture should be clean: {:?}",
        result.errors().map(|d| d.to_string()).collect::<Vec<_>>()
    );
}

// ── Cross-fixture: parse all fixtures via parse_manifest_str round-trip ──

#[test]
fn all_fixtures_round_trip() {
    let fixtures = [
        "manifest_simple",
        "manifest_pipeline",
        "manifest_ndt",
        "manifest_periodic",
        "manifest_violations",
        "manifest_multi_scope",
    ];
    for name in fixtures {
        let path = fixture_path(name);
        let yaml = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!("failed to read {}: {e}", path.display());
        });
        let m = parse_manifest_str(&yaml).unwrap_or_else(|e| {
            panic!("failed to parse {name}: {e}");
        });
        assert!(m.version > 0, "{name}: version should be > 0");
        // Just confirm the checker doesn't panic
        let _ = run_checks(&m);
    }
}
