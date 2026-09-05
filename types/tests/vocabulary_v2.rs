//! Vocabulary v2 (Phase 44.1) — trigger, sync, buffer.
//!
//! Covers: serde round-trip for every trigger form, golden parse of the
//! design-spec's own worked examples, legacy-contract compatibility
//! (real autoware-contract files parse identically under the new
//! effective-trigger derivation), and every validation error case.

use ros_launch_manifest_types::{
    Buffer, EffectiveTrigger, Sync, SyncPolicy, Trigger, parse_manifest_str,
};

// ── Serde round-trip: every trigger form ──

#[test]
fn trigger_timer_serializes_as_spec_shape() {
    let t = Trigger::Timer { rate_hz: 10.0 };
    let json = serde_json::to_value(&t).unwrap();
    assert_eq!(json, serde_json::json!({"timer": {"rate_hz": 10.0}}));
}

#[test]
fn trigger_input_serializes_as_spec_shape() {
    let t = Trigger::Input(vec!["a".into(), "b".into()]);
    let json = serde_json::to_value(&t).unwrap();
    assert_eq!(json, serde_json::json!({"input": ["a", "b"]}));
}

#[test]
fn trigger_once_serializes_as_bare_string() {
    let t = Trigger::Once;
    let json = serde_json::to_value(&t).unwrap();
    assert_eq!(json, serde_json::json!("once"));
}

#[test]
fn trigger_spontaneous_serializes_as_bare_string() {
    let t = Trigger::Spontaneous;
    let json = serde_json::to_value(&t).unwrap();
    assert_eq!(json, serde_json::json!("spontaneous"));
}

// ── Golden parse: spec's own examples (§1) ──

#[test]
fn golden_parse_trigger_forms_vehicle_cmd_gate_style() {
    let yaml = r#"
version: 1
nodes:
  vehicle_cmd_gate:
    sub: [control_cmd_in]
    pub: [control_cmd_out, gate_status]
    paths:
      forward:
        trigger: { input: [control_cmd_in] }
        output: [control_cmd_out]
        max_latency: 5ms
      status_tick:
        trigger: { timer: { rate_hz: 10 } }
        output: [gate_status]
  map_loader:
    pub: [map]
    paths:
      publish_map:
        trigger: once
        output: [map]
  remote_interface:
    pub: [external_cmd]
    paths:
      operator_cmd:
        trigger: spontaneous
        output: [external_cmd]
"#;
    let m = parse_manifest_str(yaml).unwrap();

    let forward = &m.nodes["vehicle_cmd_gate"].paths["forward"];
    assert_eq!(
        forward.trigger,
        Some(Trigger::Input(vec!["control_cmd_in".into()]))
    );
    assert_eq!(
        forward.effective_trigger(),
        EffectiveTrigger::Input(vec!["control_cmd_in".into()])
    );

    let status_tick = &m.nodes["vehicle_cmd_gate"].paths["status_tick"];
    assert_eq!(status_tick.trigger, Some(Trigger::Timer { rate_hz: 10.0 }));
    assert_eq!(
        status_tick.effective_trigger(),
        EffectiveTrigger::Timer { rate_hz: 10.0 }
    );

    let publish_map = &m.nodes["map_loader"].paths["publish_map"];
    assert_eq!(publish_map.trigger, Some(Trigger::Once));
    assert_eq!(publish_map.effective_trigger(), EffectiveTrigger::Once);

    let operator_cmd = &m.nodes["remote_interface"].paths["operator_cmd"];
    assert_eq!(operator_cmd.trigger, Some(Trigger::Spontaneous));
    assert_eq!(
        operator_cmd.effective_trigger(),
        EffectiveTrigger::Spontaneous
    );
}

// ── Golden parse: sync (§2) ──

#[test]
fn golden_parse_sync_fusion_example() {
    let yaml = r#"
version: 1
nodes:
  fuse:
    sub: [cloud_top, cloud_left, cloud_right]
    pub: [cloud_fused]
    paths:
      fuse:
        trigger: { input: [cloud_top, cloud_left, cloud_right] }
        sync:
          policy: approximate
          max_interval: 50ms
          timeout: 100ms
        output: [cloud_fused]
        max_latency: 30ms
"#;
    let m = parse_manifest_str(yaml).unwrap();
    let fuse = &m.nodes["fuse"].paths["fuse"];
    assert_eq!(
        fuse.sync,
        Some(Sync {
            policy: SyncPolicy::Approximate,
            max_interval: Some(
                ros_launch_manifest_types::duration::Duration::from_millis_f64(50.0)
            ),
            timeout: Some(ros_launch_manifest_types::duration::Duration::from_millis_f64(100.0)),
        })
    );
}

// ── Golden parse: buffer (§3) ──

#[test]
fn golden_parse_buffer_discriminator() {
    let yaml = r#"
version: 1
nodes:
  ekf:
    sub:
      control_cmd: { state: true }
      twist: { state: true, buffer: queue }
"#;
    let m = parse_manifest_str(yaml).unwrap();
    let ekf = &m.nodes["ekf"];
    assert_eq!(ekf.subscribers["control_cmd"].buffer, None);
    assert_eq!(ekf.subscribers["twist"].buffer, Some(Buffer::Queue));
}

// ── Legacy compatibility ──

#[test]
fn legacy_input_list_derives_input_trigger_without_explicit_trigger() {
    let yaml = r#"
version: 1
nodes:
  ndt:
    sub: [sensor_points]
    pub: [ndt_pose]
    paths:
      localization:
        input: sensor_points
        output: [ndt_pose]
        max_latency: 50ms
"#;
    let m = parse_manifest_str(yaml).unwrap();
    let path = &m.nodes["ndt"].paths["localization"];
    assert_eq!(path.trigger, None);
    assert_eq!(
        path.effective_trigger(),
        EffectiveTrigger::Input(vec!["sensor_points".into()])
    );
}

#[test]
fn path_with_no_trigger_and_no_input_is_unclassified_never_assumed_timer() {
    let yaml = r#"
version: 1
nodes:
  n:
    pub: [out]
    paths:
      p:
        output: [out]
"#;
    let m = parse_manifest_str(yaml).unwrap();
    let path = &m.nodes["n"].paths["p"];
    assert_eq!(path.trigger, None);
    assert_eq!(path.effective_trigger(), EffectiveTrigger::Unclassified);
}

#[test]
fn explicit_trigger_wins_over_legacy_input_when_agreeing() {
    let yaml = r#"
version: 1
nodes:
  n:
    sub: [a]
    pub: [out]
    paths:
      p:
        input: a
        trigger: { input: [a] }
        output: [out]
"#;
    let m = parse_manifest_str(yaml).unwrap();
    let path = &m.nodes["n"].paths["p"];
    assert_eq!(path.trigger, Some(Trigger::Input(vec!["a".into()])));
    assert_eq!(path.input, vec!["a".to_string()]);
    assert_eq!(
        path.effective_trigger(),
        EffectiveTrigger::Input(vec!["a".into()])
    );
}

/// Real-world regression: real autoware-contract files (vehicle_cmd_gate,
/// scan_ground_filter, map_based_prediction — all pre-Vocabulary-v2,
/// legacy `input:` style) must parse identically under the new code,
/// with effective-trigger deriving input-triggered for every `paths:`
/// entry that declares a legacy `input:`. (The files were migrated off the
/// `_ms` spellings in phase 70; the trigger form is still the legacy one.)
#[test]
fn real_autoware_contracts_parse_with_legacy_trigger_derivation() {
    type Expectation<'a> = (&'a str, &'a str, &'a [&'a str]);
    let fixtures: &[(&str, &[Expectation])] = &[
        // (file, &[(node, path_name, expected_input_endpoints)])
        (
            "vehicle_cmd_gate.contract.yaml",
            &[
                ("vehicle_cmd_gate", "command", &["auto_control_cmd"]),
                ("vehicle_cmd_gate", "gate_status", &["gate_mode"]),
            ],
        ),
        (
            "scan_ground_filter.contract.yaml",
            &[("scan_ground_filter", "main", &["input"])],
        ),
        (
            "map_based_prediction.contract.yaml",
            &[("map_based_prediction", "main", &["tracked_objects"])],
        ),
    ];

    for (file, expectations) in fixtures {
        let full_path = format!("{}/tests/fixtures/{file}", env!("CARGO_MANIFEST_DIR"));
        let src = std::fs::read_to_string(&full_path)
            .unwrap_or_else(|e| panic!("failed to read fixture {full_path}: {e}"));
        let m = parse_manifest_str(&src)
            .unwrap_or_else(|e| panic!("fixture {file} failed to parse: {e}"));

        for (node, path_name, expected_inputs) in *expectations {
            let path = m.nodes[*node].paths.get(*path_name).unwrap_or_else(|| {
                panic!("fixture {file}: node '{node}' has no path '{path_name}'")
            });
            // No explicit trigger in these legacy files.
            assert_eq!(
                path.trigger, None,
                "fixture {file}: path '{path_name}' unexpectedly has an explicit trigger"
            );
            let expected: Vec<String> = expected_inputs.iter().map(|s| s.to_string()).collect();
            assert_eq!(
                path.effective_trigger(),
                EffectiveTrigger::Input(expected),
                "fixture {file}: path '{path_name}' effective trigger mismatch"
            );
        }
    }
}

// ── Validation error cases ──

#[test]
fn trigger_input_disagreeing_with_legacy_input_is_error() {
    let yaml = r#"
version: 1
nodes:
  n:
    sub: [a, b]
    pub: [out]
    paths:
      p:
        input: [a]
        trigger: { input: [b] }
        output: [out]
"#;
    let err = parse_manifest_str(yaml).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("disagrees"), "got: {msg}");
}

#[test]
fn trigger_input_redundant_with_legacy_input_is_ok() {
    // Same set, different order — must not error (agreeing/redundant OK).
    let yaml = r#"
version: 1
nodes:
  n:
    sub: [a, b]
    pub: [out]
    paths:
      p:
        input: [a, b]
        trigger: { input: [b, a] }
        output: [out]
"#;
    let m = parse_manifest_str(yaml).unwrap();
    let path = &m.nodes["n"].paths["p"];
    assert_eq!(
        path.trigger,
        Some(Trigger::Input(vec!["b".into(), "a".into()]))
    );
}

#[test]
fn trigger_timer_requires_rate_hz() {
    let yaml = r#"
version: 1
nodes:
  n:
    pub: [out]
    paths:
      p:
        trigger: { timer: {} }
        output: [out]
"#;
    let err = parse_manifest_str(yaml).unwrap_err();
    assert!(err.to_string().contains("rate_hz"), "got: {err}");
}

#[test]
fn trigger_timer_rate_hz_must_be_positive() {
    let yaml = r#"
version: 1
nodes:
  n:
    pub: [out]
    paths:
      p:
        trigger: { timer: { rate_hz: 0 } }
        output: [out]
"#;
    let err = parse_manifest_str(yaml).unwrap_err();
    assert!(err.to_string().contains("> 0"), "got: {err}");

    let yaml_neg = r#"
version: 1
nodes:
  n:
    pub: [out]
    paths:
      p:
        trigger: { timer: { rate_hz: -5 } }
        output: [out]
"#;
    assert!(parse_manifest_str(yaml_neg).is_err());
}

#[test]
fn trigger_invalid_bare_string_is_error() {
    let yaml = r#"
version: 1
nodes:
  n:
    pub: [out]
    paths:
      p:
        trigger: periodic
        output: [out]
"#;
    let err = parse_manifest_str(yaml).unwrap_err();
    assert!(err.to_string().contains("invalid trigger"), "got: {err}");
}

#[test]
fn trigger_unknown_map_key_is_error() {
    let yaml = r#"
version: 1
nodes:
  n:
    pub: [out]
    paths:
      p:
        trigger: { bogus: 1 }
        output: [out]
"#;
    let err = parse_manifest_str(yaml).unwrap_err();
    assert!(
        err.to_string().contains("invalid trigger key"),
        "got: {err}"
    );
}

#[test]
fn trigger_multiple_map_keys_is_error() {
    let yaml = r#"
version: 1
nodes:
  n:
    pub: [out]
    paths:
      p:
        trigger: { timer: { rate_hz: 10 }, input: [a] }
        output: [out]
"#;
    let err = parse_manifest_str(yaml).unwrap_err();
    assert!(
        err.to_string()
            .contains("exactly one of 'timer' or 'input'"),
        "got: {err}"
    );
}

#[test]
fn sync_timeout_any_requires_timeout_ms() {
    let yaml = r#"
version: 1
nodes:
  n:
    sub: [a, b]
    pub: [out]
    paths:
      p:
        trigger: { input: [a, b] }
        sync: { policy: timeout_any }
        output: [out]
"#;
    let err = parse_manifest_str(yaml).unwrap_err();
    // Names the canonical spelling: the field is absent under both, so the
    // useful thing to report is what to write, not what was deprecated.
    assert!(err.to_string().contains("timeout"), "got: {err}");
}

#[test]
fn sync_exact_requires_max_interval_ms() {
    let yaml = r#"
version: 1
nodes:
  n:
    sub: [a, b]
    pub: [out]
    paths:
      p:
        trigger: { input: [a, b] }
        sync: { policy: exact }
        output: [out]
"#;
    let err = parse_manifest_str(yaml).unwrap_err();
    // Canonical spelling; see sync_timeout_any_requires_timeout_ms.
    assert!(err.to_string().contains("max_interval"), "got: {err}");
}

#[test]
fn sync_approximate_requires_max_interval_ms() {
    let yaml = r#"
version: 1
nodes:
  n:
    sub: [a, b]
    pub: [out]
    paths:
      p:
        trigger: { input: [a, b] }
        sync: { policy: approximate }
        output: [out]
"#;
    let err = parse_manifest_str(yaml).unwrap_err();
    assert!(err.to_string().contains("max_interval"), "got: {err}");
}

#[test]
fn sync_requires_input_trigger_with_at_least_two_endpoints_no_trigger() {
    let yaml = r#"
version: 1
nodes:
  n:
    pub: [out]
    paths:
      p:
        sync: { policy: exact, max_interval: 10ms }
        output: [out]
"#;
    let err = parse_manifest_str(yaml).unwrap_err();
    assert!(
        err.to_string()
            .contains("input trigger with at least 2 endpoints"),
        "got: {err}"
    );
}

#[test]
fn sync_requires_input_trigger_with_at_least_two_endpoints_single_input() {
    let yaml = r#"
version: 1
nodes:
  n:
    sub: [a]
    pub: [out]
    paths:
      p:
        trigger: { input: [a] }
        sync: { policy: exact, max_interval: 10ms }
        output: [out]
"#;
    let err = parse_manifest_str(yaml).unwrap_err();
    assert!(
        err.to_string()
            .contains("input trigger with at least 2 endpoints"),
        "got: {err}"
    );
}

#[test]
fn sync_requires_input_trigger_not_timer() {
    let yaml = r#"
version: 1
nodes:
  n:
    pub: [out]
    paths:
      p:
        trigger: { timer: { rate_hz: 10 } }
        sync: { policy: exact, max_interval: 10ms }
        output: [out]
"#;
    let err = parse_manifest_str(yaml).unwrap_err();
    assert!(
        err.to_string()
            .contains("input trigger with at least 2 endpoints"),
        "got: {err}"
    );
}

#[test]
fn sync_with_legacy_multi_input_and_no_explicit_trigger_is_ok() {
    // effective_trigger() derives Input from the legacy `input:` list —
    // sync should be satisfied without an explicit trigger.
    let yaml = r#"
version: 1
nodes:
  n:
    sub: [a, b]
    pub: [out]
    paths:
      p:
        input: [a, b]
        sync: { policy: exact, max_interval: 10ms }
        output: [out]
"#;
    let m = parse_manifest_str(yaml).unwrap();
    let path = &m.nodes["n"].paths["p"];
    assert!(path.sync.is_some());
}

#[test]
fn buffer_without_state_is_error() {
    let yaml = r#"
version: 1
nodes:
  n:
    sub:
      x: { buffer: queue }
"#;
    let err = parse_manifest_str(yaml).unwrap_err();
    assert!(err.to_string().contains("only meaningful"), "got: {err}");
}

#[test]
fn buffer_invalid_value_is_error() {
    let yaml = r#"
version: 1
nodes:
  n:
    sub:
      x: { state: true, buffer: eager }
"#;
    let err = parse_manifest_str(yaml).unwrap_err();
    assert!(err.to_string().contains("invalid buffer"), "got: {err}");
}

#[test]
fn buffer_with_state_true_is_ok() {
    let yaml = r#"
version: 1
nodes:
  n:
    sub:
      x: { state: true, buffer: queue }
"#;
    let m = parse_manifest_str(yaml).unwrap();
    assert_eq!(m.nodes["n"].subscribers["x"].buffer, Some(Buffer::Queue));
}

// ── Chains, removed (phase 68 W4) ──

/// `chains:` is rejected outright rather than ignored.
///
/// Ignoring it would leave a contract whose end-to-end budget is silently
/// unchecked: it still resolves, still produces a schedule, and the missing
/// requirement surfaces as a missed deadline on a running system instead of as
/// a message. Phase 47 set the precedent when `record.json` became a clap
/// failure rather than a warning.
#[test]
fn chains_is_rejected_and_names_the_replacement() {
    let yaml = r#"
version: 1
chains:
  sensing_to_actuation:
    semantics: reaction
    max_latency: 150ms
    segments:
      - { scope: /sensing, path: capture }
      - { via: /sensing/points }
      - { scope: /control, path: react }
"#;
    let err = parse_manifest_str(yaml).unwrap_err().to_string();
    assert!(err.contains("removed"), "{err}");
    assert!(err.contains("paths:"), "must name the replacement: {err}");
    assert!(err.contains("max_latency"), "{err}");
}

/// `semantics: age` is not a reason to keep `chains:`. Nothing ever branched
/// on it — no check, no mapper, no arithmetic — so the error message says the
/// line can be dropped rather than pretending a requirement is being lost.
#[test]
fn an_age_chain_is_rejected_too_and_says_the_line_can_go() {
    let yaml = r#"
version: 1
chains:
  c:
    semantics: age
    max_latency: 10ms
    segments:
      - { scope: /, path: a }
"#;
    let err = parse_manifest_str(yaml).unwrap_err().to_string();
    assert!(err.contains("semantics"), "{err}");
    assert!(
        err.contains("max_age"),
        "must point at the fact that does state staleness: {err}"
    );
}

// ── Endpoint `jitter:`, removed (phase 68) ──

/// An endpoint `jitter:` is rejected, and the error names where the
/// requirement now lives.
///
/// It was declared, copied into the model, and read by NOTHING — no check, no
/// mapper, no runtime monitor, on either side of the toolchain. Rejecting is
/// the point: ignoring it would leave an author believing a jitter
/// requirement is being enforced when the only thing that ever happened to it
/// was being written down.
#[test]
fn endpoint_jitter_is_rejected_and_names_the_path_requirement() {
    let yaml = r#"
version: 1
nodes:
  n:
    pub:
      out:
        min_rate_hz: 10
        jitter: 5ms
"#;
    let err = parse_manifest_str(yaml).unwrap_err().to_string();
    assert!(err.contains("removed"), "{err}");
    assert!(
        err.contains("max_jitter"),
        "must name the replacement: {err}"
    );
    assert!(
        err.contains("route"),
        "must say why an endpoint is the wrong place: {err}"
    );
}

/// The legacy unit-suffixed spelling is rejected too — it was an alias for the
/// same field, so leaving it accepted would let the removal be bypassed by
/// writing the older form.
#[test]
fn the_legacy_jitter_ms_spelling_is_rejected_as_well() {
    let yaml = r#"
version: 1
nodes:
  n:
    pub:
      out:
        jitter_ms: 5
"#;
    let err = parse_manifest_str(yaml).unwrap_err().to_string();
    assert!(err.contains("max_jitter"), "{err}");
}
