//! Integration tests for the manifest static checker.

use ros_launch_manifest_check::{Severity, run_checks, run_checks_with_spans};
use ros_launch_manifest_types::{parse_manifest_str, parse_manifest_str_with_spans};

fn errors(yaml: &str) -> Vec<String> {
    let m = parse_manifest_str(yaml).unwrap();
    let result = run_checks(&m);
    result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| format!("[{}] {}", d.rule_id, d.message))
        .collect()
}

fn warnings(yaml: &str) -> Vec<String> {
    let m = parse_manifest_str(yaml).unwrap();
    let result = run_checks(&m);
    result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .map(|d| format!("[{}] {}", d.rule_id, d.message))
        .collect()
}

fn all_diags(yaml: &str) -> Vec<String> {
    let m = parse_manifest_str(yaml).unwrap();
    let result = run_checks(&m);
    result
        .diagnostics
        .iter()
        .map(|d| format!("[{}] {}", d.rule_id, d.message))
        .collect()
}

// ── Clean manifests (no violations) ──

#[test]
fn test_clean_simple() {
    let yaml = r#"
version: 1
nodes:
  talker:
    pub: [chatter]
  listener:
    sub: [chatter]
topics:
  chatter:
    type: std_msgs/msg/String
    pub: [talker/chatter]
    sub: [listener/chatter]
"#;
    let m = parse_manifest_str(yaml).unwrap();
    let result = run_checks(&m);
    assert!(
        !result.has_errors(),
        "expected no errors: {:?}",
        errors(yaml)
    );
}

#[test]
fn test_clean_pipeline() {
    let yaml = r#"
version: 1
nodes:
  cropbox:
    pub: [output]
    sub: [input]
    paths:
      main: { input: input, output: [output], max_latency_ms: 5 }
  ground:
    pub: [output]
    sub: [input]
    paths:
      main: { input: input, output: [output], max_latency_ms: 15 }
  centerpoint:
    pub: [objects]
    sub: [pointcloud]
    paths:
      main: { input: pointcloud, output: [objects], max_latency_ms: 30 }
topics:
  cropped:
    type: PointCloud2
    pub: [cropbox/output]
    sub: [ground/input]
  no_ground:
    type: PointCloud2
    pub: [ground/output]
    sub: [centerpoint/pointcloud]
imports:
  raw_data: [cropbox/input]
exports:
  detections: [centerpoint/objects]
paths:
  main:
    input: raw_data
    output: [detections]
    max_latency_ms: 60
"#;
    let _diags = all_diags(yaml);
    // Should have no errors (scope budget 60 >= 5+15+30=50)
    let errs = errors(yaml);
    assert!(errs.is_empty(), "unexpected errors: {errs:?}");
}

// ── Endpoint uniqueness ──

#[test]
fn test_endpoint_duplicate() {
    // Construct a manifest programmatically with duplicate endpoints
    // (parser catches this from YAML, but checker also validates)
    use ros_launch_manifest_types::*;
    use std::collections::BTreeMap;

    let mut node = NodeDecl::default();
    node.publishers
        .insert("data".into(), EndpointProps::default());
    node.cli.insert("data".into(), EndpointProps::default());

    let mut m = Manifest::default();
    m.nodes.insert("relay".into(), node);

    let result = run_checks(&m);
    let errs: Vec<_> = result.errors().map(|d| d.message.clone()).collect();
    assert!(
        errs.iter().any(|e| e.contains("duplicate endpoint")),
        "expected duplicate error: {errs:?}"
    );
}

// ── QoS compatibility ──

#[test]
fn test_qos_invalid_reliability() {
    let yaml = r#"
version: 1
topics:
  pointcloud:
    type: PointCloud2
    qos:
      reliability: maybe
"#;
    let errs = errors(yaml);
    assert!(
        errs.iter().any(|e| e.contains("invalid reliability")),
        "expected QoS error: {errs:?}"
    );
}

#[test]
fn test_qos_valid() {
    let yaml = r#"
version: 1
topics:
  pointcloud:
    type: PointCloud2
    qos:
      reliability: best_effort
      durability: volatile
"#;
    let errs = errors(yaml);
    assert!(errs.is_empty(), "unexpected errors: {errs:?}");
}

// ── Rate hierarchy ──

#[test]
fn test_rate_pub_below_topic() {
    let yaml = r#"
version: 1
nodes:
  driver:
    pub:
      pointcloud:
        min_rate_hz: 5
topics:
  pointcloud:
    type: PointCloud2
    pub: [driver/pointcloud]
    rate_hz: 10
"#;
    let errs = errors(yaml);
    assert!(
        errs.iter()
            .any(|e| e.contains("min_rate_hz (5) < topic rate_hz (10)")),
        "expected rate hierarchy error: {errs:?}"
    );
}

#[test]
fn test_rate_topic_below_sub() {
    let yaml = r#"
version: 1
nodes:
  consumer:
    sub:
      input:
        min_rate_hz: 15
topics:
  data:
    type: String
    sub: [consumer/input]
    rate_hz: 10
"#;
    let errs = errors(yaml);
    assert!(
        errs.iter()
            .any(|e| e.contains("topic rate_hz (10) < subscriber")),
        "expected rate hierarchy error: {errs:?}"
    );
}

#[test]
fn test_rate_hierarchy_ok() {
    let yaml = r#"
version: 1
nodes:
  driver:
    pub:
      pointcloud:
        min_rate_hz: 15
  consumer:
    sub:
      input:
        min_rate_hz: 5
topics:
  pointcloud:
    type: PointCloud2
    pub: [driver/pointcloud]
    sub: [consumer/input]
    rate_hz: 10
"#;
    let errs = errors(yaml);
    assert!(errs.is_empty(), "unexpected errors: {errs:?}");
}

// ── Scope budget ──

#[test]
fn test_scope_budget_tight() {
    let yaml = r#"
version: 1
nodes:
  a:
    sub: [input]
    pub: [output]
    paths:
      main: { input: input, output: [output], max_latency_ms: 30 }
  b:
    sub: [input]
    pub: [output]
    paths:
      main: { input: input, output: [output], max_latency_ms: 40 }
topics:
  mid:
    type: String
    pub: [a/output]
    sub: [b/input]
paths:
  main:
    input: raw
    output: [out]
    max_latency_ms: 50
"#;
    let warns = warnings(yaml);
    assert!(
        warns.iter().any(|w| w.contains("may be less than sum")),
        "expected budget warning: {warns:?}"
    );
}

#[test]
fn test_scope_age_less_than_latency() {
    let yaml = r#"
version: 1
paths:
  main:
    input: raw
    output: [out]
    max_latency_ms: 100
    max_age_ms: 50
"#;
    let errs = errors(yaml);
    assert!(
        errs.iter()
            .any(|e| e.contains("max_age_ms (50) < max_latency_ms (100)")),
        "expected age error: {errs:?}"
    );
}

// ── Causal DAG ──

#[test]
fn test_causal_dag_ok() {
    let yaml = r#"
version: 1
nodes:
  a:
    pub: [output]
    sub: [input]
  b:
    pub: [output]
    sub: [input]
topics:
  mid:
    type: String
    pub: [a/output]
    sub: [b/input]
"#;
    let errs = errors(yaml);
    let dag_errs: Vec<_> = errs.iter().filter(|e| e.contains("causal-dag")).collect();
    assert!(dag_errs.is_empty(), "unexpected DAG error: {dag_errs:?}");
}

#[test]
fn test_causal_dag_cycle() {
    let yaml = r#"
version: 1
nodes:
  a:
    pub: [output]
    sub: [input]
  b:
    pub: [output]
    sub: [input]
topics:
  a_to_b:
    type: String
    pub: [a/output]
    sub: [b/input]
  b_to_a:
    type: String
    pub: [b/output]
    sub: [a/input]
"#;
    let errs = errors(yaml);
    assert!(
        errs.iter().any(|e| e.contains("cycle")),
        "expected cycle error: {errs:?}"
    );
}

// ── Drop rate ──

#[test]
fn test_drop_rate_feasible() {
    let yaml = r#"
version: 1
nodes:
  a:
    paths:
      main:
        input: x
        output: [y]
        drop: 2 / 100
  b:
    paths:
      main:
        input: x
        output: [y]
        drop: 3 / 100
paths:
  main:
    input: raw
    output: [out]
    drop: 6 / 100
"#;
    let errs = errors(yaml);
    let drop_errs: Vec<_> = errs.iter().filter(|e| e.contains("drop-rate")).collect();
    assert!(
        drop_errs.is_empty(),
        "unexpected drop errors: {drop_errs:?}"
    );
}

#[test]
fn test_drop_rate_infeasible() {
    let yaml = r#"
version: 1
nodes:
  a:
    paths:
      main:
        input: x
        output: [y]
        drop: 5 / 100
  b:
    paths:
      main:
        input: x
        output: [y]
        drop: 5 / 100
paths:
  main:
    input: raw
    output: [out]
    drop: 5 / 100
"#;
    let errs = errors(yaml);
    assert!(
        errs.iter().any(|e| e.contains("drop-rate")),
        "expected drop rate error: {errs:?}"
    );
}

// ── Drop consecutive ──

#[test]
fn test_drop_consecutive_feasible() {
    let yaml = r#"
version: 1
nodes:
  a:
    paths:
      main:
        input: x
        output: [y]
        drop:
          max_count: 2 / 100
          max_consecutive: 3
paths:
  main:
    input: raw
    output: [out]
    drop:
      max_count: 3 / 100
      max_consecutive: 5
"#;
    let m = parse_manifest_str(yaml).unwrap();
    let result = run_checks(&m);
    let consec_errs: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "drop-consecutive" && d.severity == Severity::Error)
        .collect();
    assert!(consec_errs.is_empty(), "unexpected errors: {consec_errs:?}");
}

#[test]
fn test_drop_consecutive_scope_stricter_than_node() {
    let yaml = r#"
version: 1
nodes:
  a:
    paths:
      main:
        input: x
        output: [y]
        drop:
          max_count: 5 / 100
          max_consecutive: 5
paths:
  main:
    input: raw
    output: [out]
    drop:
      max_count: 6 / 100
      max_consecutive: 3
"#;
    let errs = errors(yaml);
    assert!(
        errs.iter()
            .any(|e| e.contains("scope max_consecutive (3) < node max_consecutive (5)")),
        "expected consecutive error: {errs:?}"
    );
}

// ── Wiring ──

#[test]
fn test_unwired_path_endpoint() {
    let yaml = r#"
version: 1
nodes:
  processor:
    sub: [input]
    pub: [output]
    paths:
      main:
        input: input
        output: [output]
        max_latency_ms: 10
"#;
    let warns = warnings(yaml);
    assert!(
        warns.iter().any(|w| w.contains("not wired")),
        "expected wiring warning: {warns:?}"
    );
}

// ── Full pipeline (Autoware-like) ──

#[test]
fn test_full_perception_pipeline() {
    let yaml = r#"
version: 1
nodes:
  cropbox:
    sub:
      input:
        min_rate_hz: 10
    pub: [output]
    paths:
      main: { input: input, output: [output], max_latency_ms: 5 }
  ground:
    sub: [input]
    pub: [output]
    paths:
      main: { input: input, output: [output], max_latency_ms: 15 }
  centerpoint:
    sub: [pointcloud]
    pub:
      objects:
        min_rate_hz: 10
    paths:
      main:
        input: pointcloud
        output: [objects]
        max_latency_ms: 30
        drop: 5 / 100
topics:
  cropped:
    type: PointCloud2
    pub: [cropbox/output]
    sub: [ground/input]
    rate_hz: 10
  no_ground:
    type: PointCloud2
    pub: [ground/output]
    sub: [centerpoint/pointcloud]
    rate_hz: 10
imports:
  raw_data: [cropbox/input]
exports:
  detections: [centerpoint/objects]
paths:
  main:
    input: raw_data
    output: [detections]
    max_latency_ms: 60
    max_age_ms: 150
    drop: 6 / 100
"#;
    let m = parse_manifest_str(yaml).unwrap();
    let result = run_checks(&m);
    let errs: Vec<_> = result.errors().collect();
    assert!(
        errs.is_empty(),
        "expected no errors on clean pipeline: {:?}",
        errs.iter().map(|d| d.to_string()).collect::<Vec<_>>()
    );
}

// ── Source span tests ──

#[test]
fn test_span_qos_error_has_byte_range() {
    let yaml = r#"version: 1
topics:
  pointcloud:
    type: PointCloud2
    qos:
      reliability: maybe
"#;
    let parsed = parse_manifest_str_with_spans(yaml).unwrap();
    let result = run_checks_with_spans(&parsed.manifest, parsed.spans);

    let qos_errs: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "qos-compat" && d.severity == Severity::Error)
        .collect();
    assert!(!qos_errs.is_empty());

    // At least one diagnostic should have a span
    let with_span: Vec<_> = qos_errs.iter().filter(|d| d.span.is_some()).collect();
    assert!(
        !with_span.is_empty(),
        "expected at least one QoS diagnostic with span, got: {:?}",
        qos_errs
            .iter()
            .map(|d| (&d.path, &d.span))
            .collect::<Vec<_>>()
    );

    // The span should point at actual source text
    let span = with_span[0].span.as_ref().unwrap();
    let source_slice = &yaml[span.clone()];
    assert!(
        !source_slice.is_empty(),
        "span should point at non-empty source text"
    );
}

#[test]
fn test_span_rate_hierarchy_error_has_byte_range() {
    let yaml = r#"version: 1
nodes:
  driver:
    pub:
      pointcloud:
        min_rate_hz: 5
topics:
  pointcloud:
    type: PointCloud2
    pub: [driver/pointcloud]
    rate_hz: 10
"#;
    let parsed = parse_manifest_str_with_spans(yaml).unwrap();
    let result = run_checks_with_spans(&parsed.manifest, parsed.spans);

    let rate_errs: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "rate-hierarchy")
        .collect();
    assert!(!rate_errs.is_empty());

    let with_span: Vec<_> = rate_errs.iter().filter(|d| d.span.is_some()).collect();
    assert!(
        !with_span.is_empty(),
        "expected rate-hierarchy diagnostic with span"
    );
}

#[test]
fn test_span_scope_budget_error_has_byte_range() {
    let yaml = r#"version: 1
paths:
  main:
    input: raw
    output: [out]
    max_latency_ms: 100
    max_age_ms: 50
"#;
    let parsed = parse_manifest_str_with_spans(yaml).unwrap();
    let result = run_checks_with_spans(&parsed.manifest, parsed.spans);

    let budget_errs: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "scope-budget" && d.severity == Severity::Error)
        .collect();
    assert!(!budget_errs.is_empty());

    let with_span: Vec<_> = budget_errs.iter().filter(|d| d.span.is_some()).collect();
    assert!(
        !with_span.is_empty(),
        "expected scope-budget diagnostic with span"
    );
}

#[test]
fn test_no_spans_without_index() {
    // run_checks (no spans) should produce diagnostics with span=None
    let yaml = r#"version: 1
topics:
  pc:
    type: PointCloud2
    qos:
      reliability: maybe
"#;
    let m = parse_manifest_str(yaml).unwrap();
    let result = run_checks(&m);

    assert!(result.has_errors());
    for diag in &result.diagnostics {
        assert!(diag.span.is_none(), "expected no spans without index");
    }
}

// ── Service wiring and type checks ──

#[test]
fn test_service_wiring_clean() {
    let yaml = r#"
version: 1
nodes:
  server_node:
    srv:
      my_service: {}
  client_node:
    cli:
      my_service: {}
services:
  my_service:
    type: std_srvs/srv/Trigger
    server: [server_node/my_service]
    client: [client_node/my_service]
"#;
    let m = parse_manifest_str(yaml).unwrap();
    let result = run_checks(&m);
    let svc_diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.rule_id.starts_with("service"))
        .collect();
    assert!(
        svc_diags.is_empty(),
        "expected no service issues: {svc_diags:?}"
    );
}

#[test]
fn test_service_wiring_missing_server() {
    let yaml = r#"
version: 1
nodes:
  client_node:
    cli:
      orphan_service: {}
"#;
    let m = parse_manifest_str(yaml).unwrap();
    let result = run_checks(&m);
    let warns: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "service-wiring")
        .collect();
    assert!(
        !warns.is_empty(),
        "expected service-wiring warning for orphan client"
    );
}

#[test]
fn test_service_type_missing() {
    let yaml = r#"
version: 1
services:
  bad_service:
    server: [node/srv]
"#;
    let m = parse_manifest_str(yaml).unwrap();
    let result = run_checks(&m);
    let errs: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "service-type" && d.severity == Severity::Error)
        .collect();
    assert!(
        !errs.is_empty(),
        "expected service-type error for missing type"
    );
}

#[test]
fn test_service_type_server_not_on_node() {
    let yaml = r#"
version: 1
nodes:
  my_node:
    pub: [output]
services:
  my_service:
    type: std_srvs/srv/Trigger
    server: [my_node/nonexistent_srv]
"#;
    let m = parse_manifest_str(yaml).unwrap();
    let result = run_checks(&m);
    let warns: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.rule_id == "service-type" && d.severity == Severity::Warning)
        .collect();
    assert!(
        !warns.is_empty(),
        "expected warning for server not found on node"
    );
}

// ── Combined args + conditions ──

#[test]
fn test_args_conditions_combined() {
    use ros_launch_manifest_types::{filter_manifest, resolve_args, substitute_manifest};
    use std::collections::HashMap;

    let yaml = r#"
args:
  use_sensor: "true"
  sensor_topic: sensor_msgs/msg/PointCloud2

version: 1
nodes:
  sensor_driver:
    if: $(var use_sensor)
    pub:
      pointcloud:
        min_rate_hz: 10
  processor:
    sub: [input]
    pub: [output]
topics:
  sensor_data:
    if: $(var use_sensor)
    type: $(var sensor_topic)
    pub: [sensor_driver/pointcloud]
    sub: [processor/input]
    rate_hz: 10
"#;
    let mut m = parse_manifest_str(yaml).unwrap();

    // Case 1: use_sensor=true — sensor_driver and sensor_data present
    let args = resolve_args(&m.args, &HashMap::new()).unwrap();
    let mut m1 = substitute_manifest(&m, &args).unwrap();
    filter_manifest(&mut m1);

    assert!(m1.nodes.contains_key("sensor_driver"));
    assert!(m1.nodes.contains_key("processor"));
    assert!(m1.topics.contains_key("sensor_data"));
    assert_eq!(
        m1.topics["sensor_data"].msg_type,
        "sensor_msgs/msg/PointCloud2"
    );

    let result = run_checks(&m1);
    assert!(
        !result.has_errors(),
        "case 1 (sensor enabled) should be clean: {:?}",
        result.errors().map(|d| d.to_string()).collect::<Vec<_>>()
    );

    // Case 2: use_sensor=false — sensor_driver and sensor_data filtered out
    let override_args = HashMap::from([("use_sensor".into(), "false".into())]);
    let args2 = resolve_args(&m.args, &override_args).unwrap();
    let mut m2 = substitute_manifest(&m, &args2).unwrap();
    filter_manifest(&mut m2);

    assert!(!m2.nodes.contains_key("sensor_driver"));
    assert!(m2.nodes.contains_key("processor"));
    assert!(!m2.topics.contains_key("sensor_data"));

    let result2 = run_checks(&m2);
    assert!(
        !result2.has_errors(),
        "case 2 (sensor disabled) should be clean: {:?}",
        result2.errors().map(|d| d.to_string()).collect::<Vec<_>>()
    );
}
