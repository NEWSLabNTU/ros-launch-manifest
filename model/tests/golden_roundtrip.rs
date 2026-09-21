//! Golden-file round-trip: the fixture must parse, survive
//! serialize→reparse unchanged, and carry the values the design doc promises.

use pretty_assertions::assert_eq;
use ros_launch_manifest_model::*;

fn golden() -> SystemModel {
    let yaml = include_str!("golden/perception.system_model.yaml");
    SystemModel::from_yaml_str(yaml).expect("golden fixture must parse")
}

#[test]
fn roundtrip_is_lossless() {
    let model = golden();
    let emitted = model.to_yaml_string().unwrap();
    let reparsed = SystemModel::from_yaml_str(&emitted).expect("re-emitted YAML must parse");
    assert_eq!(model, reparsed);
}

#[test]
fn serialization_is_deterministic() {
    let model = golden();
    assert_eq!(
        model.to_yaml_string().unwrap(),
        model.to_yaml_string().unwrap()
    );
    // BTreeMap ordering: re-emitting a reparsed model is byte-identical —
    // the property the provenance hash relies on.
    let emitted = model.to_yaml_string().unwrap();
    let again = SystemModel::from_yaml_str(&emitted)
        .unwrap()
        .to_yaml_string()
        .unwrap();
    assert_eq!(emitted, again);
}

#[test]
fn meta_carries_binding_and_provenance() {
    let m = golden().meta;
    assert_eq!(m.version, SCHEMA_VERSION);
    assert_eq!(m.args["mode"], "velodyne");
    assert_eq!(m.inputs.len(), 2);
    assert_eq!(m.inputs[0].path, "perception.yaml");
    assert_eq!(m.resolver.unwrap().tool, "play_launch");
    assert_eq!(
        m.diagnostics.len(),
        1,
        "warnings embed; errors refuse emission"
    );
}

#[test]
fn structure_layer_resolved_shapes() {
    let s = golden().structure;
    // scope tree with parent links
    assert_eq!(
        s.scopes["/perception/detection"].parent.as_deref(),
        Some("/perception")
    );
    // plain node vs composable vs lifecycle
    let det = &s.nodes["/perception/detection/detector"];
    assert_eq!(det.exec.as_deref(), Some("detector_node"));
    assert!(!det.lifecycle);
    // Phase 46.1b — launch spawn inputs (remaps/ros_args/respawn/env).
    assert_eq!(
        det.remaps,
        vec![Remap {
            from: "/points".to_string(),
            to: "/sensing/lidar/points".to_string(),
        }]
    );
    assert_eq!(
        det.ros_args,
        vec![
            "--log-level".to_string(),
            "detector_node:=debug".to_string()
        ]
    );
    assert_eq!(det.respawn, Some(true));
    assert_eq!(det.respawn_delay, Some(2.5));
    // A `<timer>` start delay is independent of the respawn delay: this node
    // carries both, and they mean different waits.
    assert_eq!(det.start_delay_secs, Some(3.0));
    assert_eq!(
        det.env,
        vec![EnvVar {
            name: "CUDA_VISIBLE_DEVICES".to_string(),
            value: "0".to_string(),
        }]
    );
    assert_eq!(
        s.nodes["/sensing/imu_node"].criticality.as_deref(),
        Some("high")
    );
    // R1-M4/M6 — resolved params + lifecycle autostart
    let imu = &s.nodes["/sensing/imu_node"];
    assert_eq!(imu.params["rate_hz"], ParamValue::Int(100));
    assert_eq!(
        imu.params["frame_id"],
        ParamValue::Str("imu_link".to_string())
    );
    assert_eq!(imu.params["use_filter"], ParamValue::Bool(true));
    assert_eq!(
        imu.params["offsets"],
        ParamValue::StrList(vec!["0.1".to_string(), "0.2".to_string()])
    );
    assert_eq!(
        s.nodes["/perception/tracker"].lifecycle_autostart,
        Some(Autostart::Active)
    );
    let tracker = &s.nodes["/perception/tracker"];
    assert_eq!(tracker.plugin.as_deref(), Some("tracker::TrackerNode"));
    assert_eq!(
        tracker.container.as_deref(),
        Some("/perception/pipeline_container")
    );
    assert!(tracker.lifecycle);
    // no explicit launch-spawn-fields on this node → additive-schema defaults.
    assert!(tracker.remaps.is_empty());
    assert!(tracker.ros_args.is_empty());
    assert_eq!(tracker.respawn, None);
    assert_eq!(tracker.respawn_delay, None);
    assert_eq!(tracker.start_delay_secs, None);
    assert!(tracker.env.is_empty());
    // wiring uses "<node FQN>/<endpoint>" refs
    assert_eq!(
        s.topics["/perception/objects"].publishers,
        vec!["/perception/detection/detector/objects"]
    );
    assert_eq!(
        s.services["/perception/trigger"].srv_type,
        "std_srvs/srv/Trigger"
    );
}

#[test]
fn contracts_layer_numbers() {
    let c = golden().contracts;
    // `jitter_ms` was removed from `PubContract` (phase 68): it was declared,
    // copied here, and read by nothing on either side of the toolchain. The
    // golden file on disk deliberately STILL CARRIES it — this asserts that an
    // old model deserializes anyway, the same backward-compatible removal the
    // retired `record:` field documents on `SystemModel`. If someone ever adds
    // `deny_unknown_fields`, this test is what fails.
    assert!(
        include_str!("golden/perception.system_model.yaml").contains("jitter_ms:"),
        "the golden model must keep a retired field, or it stops proving \
         old models still load"
    );
    // Same for `correlation:` on a path (phase 70).
    assert!(
        include_str!("golden/perception.system_model.yaml").contains("correlation:"),
        "the golden model must keep `correlation:` on disk for the same reason"
    );
    // R1-M5 — per-endpoint QoS
    assert_eq!(
        c.pub_endpoints["/perception/detection/detector/objects"]
            .qos
            .as_ref()
            .unwrap()
            .reliability
            .as_deref(),
        Some("best_effort")
    );
    let sub = &c.sub_endpoints["/perception/detection/detector/pointcloud"];
    assert_eq!(sub.max_age_ms, Some(100.0));
    assert!(!sub.state);
    assert!(c.sub_endpoints["/perception/tracker/input"].state);
    assert_eq!(
        c.srv_endpoints["/perception/detection/detector/trigger"].max_response_ms,
        Some(100.0)
    );
    // node path = processing budget; scope path = E2E with correlation + drops
    assert_eq!(
        c.node_paths["/perception/detection/detector/main"].max_latency_ms,
        Some(30.0)
    );
    let e2e = &c.scope_paths["/perception/e2e"];
    assert_eq!(e2e.max_latency_ms, Some(85.0));
    assert_eq!(e2e.drop.as_ref().unwrap().max_drop_rate, Some(0.08));
    // topic channel contract
    let pc = &c.topics["/sensing/pointcloud"];
    assert_eq!(pc.max_transport_ms, Some(5.0));
    assert_eq!(
        pc.qos.as_ref().unwrap().reliability.as_deref(),
        Some("best_effort")
    );
}

/// Design issue #52: the model carries every fact the CHECKER resolves per
/// entity, so that one derivation of the mapper input can read them and
/// neither consumer re-derives them from the manifest (or, worse, from a
/// neighbouring promise). The golden model has one timer path carrying the
/// per-path facts, one input path carrying `sync`, one `state: true`
/// subscriber carrying `buffer`, and the two top-level maps.
#[test]
fn contracts_carry_the_checker_facts_a_scheduler_reads() {
    use ros_launch_manifest_sched::{Criticality, EffectiveTrigger};

    let c = golden().contracts;

    // The timer's rate is a fact of the PATH. Its output carries no
    // `min_rate_hz` on purpose: a consumer that needed one to find the rate
    // was reading a promise as a trigger (issue #52, row 1).
    let ctrl = &c.node_paths["/sensing/imu_node/ctrl"];
    assert!(ctrl.input.is_empty(), "a timer path has no inputs");
    assert_eq!(
        ctrl.trigger,
        Some(EffectiveTrigger::Timer { rate_hz: 100.0 })
    );
    assert_eq!(ctrl.effective_trigger().period_ms(), Some(10.0));
    assert_eq!(ctrl.max_latency_ms, Some(8.0));
    assert_eq!(ctrl.min_latency_ms, Some(1.0));
    assert_eq!(ctrl.max_jitter_ms, Some(2.0));
    assert!(ctrl.sync.is_none());
    assert!(
        !c.pub_endpoints.contains_key("/sensing/imu_node/imu"),
        "the fixture must not let a publisher promise stand in for the timer"
    );

    // An input path keeps its sources in `input` AND names them as the
    // trigger's value; `sync` rides on the path they fan into.
    let main = &c.node_paths["/perception/detection/detector/main"];
    assert_eq!(
        main.trigger,
        Some(EffectiveTrigger::Input(vec![
            "/perception/detection/detector/pointcloud".to_string()
        ]))
    );
    assert_eq!(
        main.input,
        vec!["/perception/detection/detector/pointcloud"]
    );
    let sync = main.sync.as_ref().expect("the input path declares sync");
    assert_eq!(sync.policy, SyncPolicy::Approximate);
    assert_eq!(sync.max_interval_ms, Some(10.0));
    assert_eq!(sync.timeout_ms, None);

    // Scope paths are not triggered by anything a node schedules.
    assert!(c.scope_paths["/perception/e2e"].trigger.is_none());

    // A `state: true` subscriber says how it holds data between takes.
    let state = &c.sub_endpoints["/perception/tracker/input"];
    assert!(state.state);
    assert_eq!(state.buffer, Some(BufferContract::Queue));
    assert_eq!(
        c.sub_endpoints["/perception/detection/detector/pointcloud"].buffer,
        None
    );

    // The severity scale and the EFFECTIVE criticality, in sched's own
    // spelling. The detector has no label on its NodeInstance; its entry
    // here is what a hazard derived for it.
    assert_eq!(
        c.severity_levels,
        vec!["QM", "ASIL_A", "ASIL_B", "ASIL_C", "ASIL_D"]
    );
    assert_eq!(c.node_criticality["/sensing/imu_node"], Criticality::High);
    assert_eq!(
        c.node_criticality["/perception/detection/detector"],
        Criticality::Medium
    );
    assert!(
        golden().structure.nodes["/perception/detection/detector"]
            .criticality
            .is_none(),
        "the label and the effective value are different facts"
    );
    assert!(!c.node_criticality.contains_key("/perception/tracker"));
}

/// The golden model with every issue-#52 key deleted from the text: what a
/// play_launch older than the fields emitted, and what nano-ros keeps
/// reading. It must parse, every new field must come back absent, and the
/// absent trigger must read as `Unclassified`, never as a timer.
#[test]
fn a_model_without_the_issue_52_fields_still_parses_and_ranks_nothing() {
    use ros_launch_manifest_sched::EffectiveTrigger;

    const NEW_KEYS: &[&str] = &[
        "trigger:",
        "sync:",
        "min_latency_ms:",
        "buffer:",
        "severity_levels:",
        "node_criticality:",
    ];
    // A line is one of the new keys when the key opens it; a substring
    // match would catch the `/perception/trigger` service.
    let opens_new_key = |line: &str| NEW_KEYS.iter().any(|k| line.trim_start().starts_with(k));
    let golden_text = include_str!("golden/perception.system_model.yaml");
    for key in NEW_KEYS {
        assert!(
            golden_text.lines().any(|l| l.trim_start().starts_with(key)),
            "the golden model must carry {key}"
        );
    }

    // Drop each new key with its indented children.
    let mut kept = String::new();
    let mut skip_deeper_than: Option<usize> = None;
    for line in golden_text.lines() {
        if line.trim().is_empty() {
            kept.push('\n');
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if let Some(depth) = skip_deeper_than {
            if indent > depth {
                continue;
            }
            skip_deeper_than = None;
        }
        if opens_new_key(line) {
            skip_deeper_than = Some(indent);
            continue;
        }
        kept.push_str(line);
        kept.push('\n');
    }
    assert!(
        !kept.lines().any(opens_new_key),
        "a new key survived the strip:\n{kept}"
    );

    let old = SystemModel::from_yaml_str(&kept).expect("an older model must still parse");
    let c = &old.contracts;
    let ctrl = &c.node_paths["/sensing/imu_node/ctrl"];
    assert_eq!(ctrl.trigger, None);
    assert_eq!(
        ctrl.effective_trigger(),
        EffectiveTrigger::Unclassified,
        "no inputs is not a timer: the rate is gone with the field"
    );
    assert_eq!(ctrl.effective_trigger().period_ms(), None);
    assert_eq!(ctrl.min_latency_ms, None);
    assert_eq!(
        ctrl.max_latency_ms,
        Some(8.0),
        "the old fields are untouched"
    );
    let main = &c.node_paths["/perception/detection/detector/main"];
    assert_eq!(main.trigger, None);
    assert_eq!(main.effective_trigger(), EffectiveTrigger::Unclassified);
    assert!(main.sync.is_none());
    assert_eq!(c.sub_endpoints["/perception/tracker/input"].buffer, None);
    assert!(c.severity_levels.is_empty());
    assert!(c.node_criticality.is_empty());

    // Re-emitting the old model invents none of the new keys.
    let re_emitted = old.to_yaml_string().unwrap();
    assert!(
        !re_emitted.lines().any(opens_new_key),
        "a new key was invented:\n{re_emitted}"
    );
    let again = SystemModel::from_yaml_str(&re_emitted).unwrap();
    assert_eq!(old, again);
}

/// Every `EffectiveTrigger` variant crosses the boundary in the adjacently
/// tagged shape the sched crate chose, and absent stays absent.
#[test]
fn a_path_contract_carries_each_trigger_kind_across_the_boundary() {
    use ros_launch_manifest_sched::EffectiveTrigger;

    for (trigger, spelled) in [
        (
            EffectiveTrigger::Timer { rate_hz: 30.0 },
            "kind: timer\n  value:\n    rate_hz: 30.0",
        ),
        (
            EffectiveTrigger::Input(vec!["a".to_string(), "b".to_string()]),
            "kind: input\n  value:\n  - a\n  - b",
        ),
        (EffectiveTrigger::Once, "kind: once"),
        (EffectiveTrigger::Spontaneous, "kind: spontaneous"),
        (EffectiveTrigger::Unclassified, "kind: unclassified"),
    ] {
        let pc = PathContract {
            output: vec!["/out".to_string()],
            trigger: Some(trigger.clone()),
            sync: Some(SyncContract {
                policy: SyncPolicy::TimeoutAny,
                max_interval_ms: None,
                timeout_ms: Some(50.0),
            }),
            min_latency_ms: Some(0.5),
            ..Default::default()
        };
        let yaml = serde_yaml_ng::to_string(&pc).expect("serializes");
        assert!(yaml.contains(spelled), "{trigger:?} spelled as:\n{yaml}");
        assert!(yaml.contains("policy: timeout_any"), "{yaml}");
        let back: PathContract = serde_yaml_ng::from_str(&yaml).expect("round-trips");
        assert_eq!(back, pc);
        assert_eq!(back.effective_trigger(), trigger);
    }

    let bare = PathContract {
        output: vec!["/out".to_string()],
        ..Default::default()
    };
    let yaml = serde_yaml_ng::to_string(&bare).unwrap();
    assert!(!yaml.contains("trigger"), "{yaml}");
    assert!(!yaml.contains("sync"), "{yaml}");
    assert!(!yaml.contains("min_latency"), "{yaml}");
}

#[test]
fn execution_layer_slices() {
    let e = golden().execution;
    // the two consumer slices
    assert_eq!(
        e.deploy["/perception/detection/detector"].target,
        Some(Target::Linux)
    );
    assert_eq!(
        e.deploy["/sensing/imu_node"].target,
        Some(Target::Mcu {
            board: "stm32f4".into()
        })
    );
    // R1-M1/M2/M3 — deploy fields, transports, bridges, features
    let imu = &e.deploy["/sensing/imu_node"];
    assert_eq!(imu.domain, Some(7));
    assert_eq!(imu.locator.as_deref(), Some("tcp/10.0.2.1:7447"));
    assert_eq!(imu.rmw.as_deref(), Some("zenoh"));
    assert_eq!(imu.extra["optimize"], ExtraValue::Str("size".to_string()));
    assert_eq!(
        imu.extra["features"],
        ExtraValue::StrList(vec!["safety".to_string()])
    );

    let t = &e.transports[0];
    assert_eq!(t.kind, "ethernet");
    assert_eq!(t.id.as_deref(), Some("eth0"));
    assert_eq!(t.mac.as_deref(), Some("02:00:00:00:00:01"));
    assert_eq!(t.domain, Some(7));
    assert_eq!(e.bridges[0].from, "eth0");
    assert_eq!(e.bridges[0].topics, vec!["/perception/objects"]);
    assert_eq!(e.features, vec!["safety"]);

    // tier table (sched crate TierDef schema) + per-callback-group binding
    let high = &e.tiers["high"];
    assert_eq!(high.class.as_deref(), Some("real_time"));
    assert_eq!(high.deadline.unwrap().as_micros(), 2_000);
    assert_eq!(high.spin_period.unwrap().as_micros(), 1_000);
    let posix = high.posix.as_ref().unwrap();
    assert_eq!(posix.priority, 80);
    assert_eq!(posix.sched_class.as_deref(), Some("SCHED_FIFO"));
    assert_eq!(posix.core, Some(2));
    assert_eq!(high.threadx.as_ref().unwrap().preempt_threshold, Some(4));
    assert_eq!(e.bindings["/sensing/imu_node/ctrl"], "high");
}

/// Phase 46.1b backward-compat: a `NodeInstance` written before
/// remaps/ros_args/respawn/env existed (no such keys at all) must still
/// parse, with every new field defaulting to empty/`None` — the
/// additive-schema guarantee `docs/design/unified-system-model.md` promises
/// nano-ros (it vendors this crate and must keep reading old models).
#[test]
fn node_instance_without_launch_fields_parses_with_defaults() {
    let yaml = "\
scope: /perception
pkg: lidar_centerpoint
exec: detector_node
";
    let node: NodeInstance = serde_yaml_ng::from_str(yaml).unwrap();
    assert_eq!(node.exec.as_deref(), Some("detector_node"));
    assert!(node.remaps.is_empty());
    assert!(node.ros_args.is_empty());
    assert_eq!(node.respawn, None);
    assert_eq!(node.respawn_delay, None);
    assert_eq!(node.start_delay_secs, None);
    assert!(node.env.is_empty());

    // and re-emitting it doesn't invent any of the new keys (no noise for
    // artifacts that never carried them).
    let re_emitted = serde_yaml_ng::to_string(&node).unwrap();
    assert!(!re_emitted.contains("remaps:"), "{re_emitted}");
    assert!(!re_emitted.contains("ros_args:"), "{re_emitted}");
    assert!(!re_emitted.contains("respawn:"), "{re_emitted}");
    assert!(!re_emitted.contains("start_delay_secs:"), "{re_emitted}");
    assert!(!re_emitted.contains("env:"), "{re_emitted}");
}

/// The two phase-67 facts that reach the model, and the reason they are
/// carried here at all.
///
/// `max_jitter` and `miss` existed in the contract and in the sched crate's
/// `MapperPath` from phase 67, but not on `PathContract` — and the model is
/// the ONLY thing a second toolchain reads. nano-ros builds its `MapperPath`
/// from the model rather than from the manifest, so it could not see either
/// fact: not "chose not to use", could not. This asserts the boundary carries
/// them, in `sched`'s own spelling rather than a model-local mirror.
#[test]
fn a_path_contract_carries_jitter_and_miss_across_the_boundary() {
    use ros_launch_manifest_model::PathContract;
    use ros_launch_manifest_sched::{MapperMiss, MapperMissAction};

    let pc = PathContract {
        output: vec!["/out".to_string()],
        max_latency_ms: Some(20.0),
        max_jitter_ms: Some(5.0),
        miss: Some(MapperMiss {
            tolerate_n: Some(1),
            tolerate_w: Some(10),
            consecutive: None,
            action: Some(MapperMissAction::SkipNext),
        }),
        ..Default::default()
    };
    let yaml = serde_yaml_ng::to_string(&pc).expect("serializes");
    assert!(yaml.contains("max_jitter_ms: 5.0"), "{yaml}");
    assert!(
        yaml.contains("skip_next"),
        "the action must cross as sched spells it: {yaml}"
    );

    let back: PathContract = serde_yaml_ng::from_str(&yaml).expect("round-trips");
    assert_eq!(back, pc);

    // Absent stays absent — a path that declares neither must not gain an
    // empty `miss:` in every model anyone emits.
    let bare = PathContract {
        output: vec!["/out".to_string()],
        ..Default::default()
    };
    let yaml = serde_yaml_ng::to_string(&bare).unwrap();
    assert!(!yaml.contains("max_jitter"), "{yaml}");
    assert!(!yaml.contains("miss"), "{yaml}");
}

/// Absent is not empty, across the boundary.
///
/// An absent `concurrency:` means every path of a node serialises; an empty
/// `exclusive: []` means every path may run concurrently. They are opposite
/// claims, and a `Vec` alone would make them identical — so the declaration is
/// PRESENCE in the map, and this asserts a round-trip preserves both.
#[test]
fn an_empty_exclusion_survives_the_round_trip_and_is_not_an_absent_one() {
    use ros_launch_manifest_model::Contracts;
    use ros_launch_manifest_sched::ConcurrencyContract;

    let mut c = Contracts::default();
    c.node_concurrency.insert(
        "/claims".to_string(),
        ConcurrencyContract { exclusive: vec![] },
    );
    c.node_concurrency.insert(
        "/serialises".to_string(),
        ConcurrencyContract {
            exclusive: vec![vec!["a".to_string(), "b".to_string()]],
        },
    );

    let yaml = serde_yaml_ng::to_string(&c).expect("serializes");
    let back: Contracts = serde_yaml_ng::from_str(&yaml).expect("round-trips");

    assert!(
        back.node_concurrency.contains_key("/claims"),
        "an empty declaration must survive as a DECLARATION, not vanish into \
         looking like an absent one: {yaml}"
    );
    assert!(back.node_concurrency["/claims"].exclusive.is_empty());
    assert_eq!(back.node_concurrency["/serialises"].exclusive.len(), 1);
    assert!(
        !back.node_concurrency.contains_key("/undeclared"),
        "and a node that declared nothing must not acquire an entry"
    );
}
