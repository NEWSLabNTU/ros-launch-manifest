//! Parity assertion 1 of design issue #52: the golden `RankedPlan` both
//! consumers compare their ranking of the shared fixture against.
//!
//! The fixture is play_launch's `contract_derived_chain`, resolved and
//! checked in at `fixtures/contract_derived_chain.system_model.yaml` (its
//! header says which fields were filled by hand and why). The snapshot at
//! `snapshots/contract_derived_chain.ranked_plan.txt` is the `Debug` text of
//! `chain_aware_rank(&mapper_input_from_model(fixture, no facts))`, compared
//! byte for byte: play_launch (phase-78 W2) asserts its `from_dump` input
//! ranks to the same text, nano-ros (phase-457 W3) asserts the same of the
//! fixture through its own load path. This is the `derive`-side twin of
//! `sched`'s `chain_aware_rank_is_priorityless_and_split_is_parity`; it
//! lives here because `sched` cannot depend on `derive`.
//!
//! To re-take the snapshot after a deliberate change to the derivation or
//! the ranker: `UPDATE_RANKED_PLAN_SNAPSHOT=1 cargo test -p
//! ros-launch-manifest-derive --test parity`, then review the diff. Both
//! consumers' gates move with it, which is the point.

use std::collections::BTreeMap;
use std::path::PathBuf;

use ros_launch_manifest_derive::{ChainSkip, DeriveFacts, SkippedChain, mapper_input_from_model};
use ros_launch_manifest_model::{
    PathContract, PubContract, SystemModel, TopicContract, TopicWiring,
};
use ros_launch_manifest_sched::{
    ChainElement, EffectiveTrigger, MapWarning, MapperInput, RankedPlan, chain_aware_rank,
};

const FIXTURE: &str = include_str!("fixtures/contract_derived_chain.system_model.yaml");
const SNAPSHOT_REL: &str = "tests/snapshots/contract_derived_chain.ranked_plan.txt";
const SCOPE_PATH: &str = "bringup.launch.xml/points_to_cmd";
const SENSOR: &str = "/perception/sensor_node";
const FILTER: &str = "/perception/filter_component";
const CONTROL: &str = "/control/control_node";

fn fixture() -> SystemModel {
    SystemModel::from_yaml_str(FIXTURE).expect("the fixture parses")
}

fn snapshot_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(SNAPSHOT_REL)
}

/// The one rendering every consumer compares against: `{:#?}` plus a
/// trailing newline, so the file is a text file.
fn render(plan: &RankedPlan) -> String {
    format!("{plan:#?}\n")
}

fn rank(model: &SystemModel, facts: &DeriveFacts) -> (MapperInput, RankedPlan) {
    let (input, _) = mapper_input_from_model(model, facts);
    let plan = chain_aware_rank(&input);
    (input, plan)
}

fn golden() -> String {
    std::fs::read_to_string(snapshot_path())
        .unwrap_or_else(|e| panic!("read {}: {e}", snapshot_path().display()))
}

/// Compare a rendering with the snapshot, or rewrite the snapshot when
/// asked to. The first mismatching line is named, so a consumer that fails
/// this gate sees the fact that moved rather than two screens of text.
fn assert_matches_snapshot(actual: &str) {
    if std::env::var_os("UPDATE_RANKED_PLAN_SNAPSHOT").is_some() {
        std::fs::write(snapshot_path(), actual).expect("write the snapshot");
        return;
    }
    let expected = golden();
    if actual == expected {
        return;
    }
    let first_diff = actual
        .lines()
        .zip(expected.lines())
        .position(|(a, e)| a != e)
        .map_or(actual.lines().count().min(expected.lines().count()), |i| i)
        + 1;
    panic!(
        "the ranking of the fixture differs from {} at line {first_diff}\n\
         --- expected\n{expected}\n--- actual\n{actual}\n\
         (UPDATE_RANKED_PLAN_SNAPSHOT=1 rewrites it; both consumers' gates \
         then move with it)",
        snapshot_path().display()
    );
}

// ---------------------------------------------------------------------------
// 1. the golden snapshot
// ---------------------------------------------------------------------------

/// `chain_aware_rank(mapper_input_from_model(fixture))` is the snapshot,
/// byte for byte, and the snapshot says what play_launch's own
/// `check --explain` says of the same system today: one chain, drained
/// toward its sink (`control` above `filter`, one segment), the 100 Hz
/// timer boundary below both, no other item, and a feasibility verdict that
/// names the boundary it took on trust.
#[test]
fn the_fixture_ranks_to_the_golden_snapshot() {
    let model = fixture();
    let (input, report) = mapper_input_from_model(&model, &DeriveFacts::default());

    // The migrated model leaves nothing unsaid.
    assert!(
        report.paths_without_trigger.is_empty(),
        "every path carries a trigger: {:?}",
        report.paths_without_trigger
    );
    assert_eq!(report.chains_resolved, vec![SCOPE_PATH.to_string()]);
    assert!(
        report.chains_skipped.is_empty(),
        "{:?}",
        report.chains_skipped
    );

    let plan = chain_aware_rank(&input);
    assert_matches_snapshot(&render(&plan));

    // What the snapshot pins, in words, so a diff is readable as a fact.
    let order: Vec<(&str, &str)> = plan
        .items
        .iter()
        .map(|it| (it.node.as_str(), it.path.as_str()))
        .collect();
    assert_eq!(
        order,
        vec![(CONTROL, "control"), (FILTER, "filter"), (SENSOR, "tick")],
        "drain toward the sink, then the boundary; the container has no path"
    );
    assert_eq!(
        plan.items[0].fine_group, plan.items[1].fine_group,
        "control and filter are one event segment"
    );
    assert_ne!(plan.items[1].fine_group, plan.items[2].fine_group);
    assert!(
        plan.items
            .iter()
            .all(|it| it.coarse_group.as_deref() == Some("points_to_cmd")),
        "every item is the chain's"
    );
    assert!(plan.items.iter().all(|it| it.tie_group.is_none()));
    assert!(
        plan.items[0]
            .provenance
            .contains("points_to_cmd segment drain 1/2")
    );
    assert!(
        plan.items[1]
            .provenance
            .contains("points_to_cmd segment drain 2/2")
    );
    assert!(
        plan.items[2]
            .provenance
            .contains("points_to_cmd boundary RM")
    );
    assert_eq!(
        plan.warnings,
        vec![MapWarning::ChainFeasibleWithoutWcet {
            chain: "points_to_cmd".to_string(),
            boundaries_without_wcet: vec![format!("{SENSOR}/tick")],
        }],
        "no facts were supplied, so the verdict says the boundary cost zero"
    );

    // The per-node facts the ranking was derived from.
    let sensor = input.nodes.iter().find(|n| n.name == SENSOR).unwrap();
    assert_eq!(
        sensor.rate_hz,
        Some(100.0),
        "the timer's rate, from the trigger"
    );
    assert_eq!(sensor.deadline_us, None);
    assert_eq!(sensor.criticality, None);
    let control = input.nodes.iter().find(|n| n.name == CONTROL).unwrap();
    assert_eq!(control.rate_hz, None);
    assert_eq!(control.deadline_us, Some(10_000));
    assert_eq!(
        control.criticality,
        Some(ros_launch_manifest_sched::Criticality::High)
    );
    assert!(input.nodes.iter().all(|n| !n.claims_concurrency));
    assert_eq!(
        input.legacy, None,
        "the .toml bridge sets it, not the derivation"
    );
}

/// The snapshot file is what the fixture ranks to and nothing stale: the
/// test above would catch a drift in either direction, but a reader of the
/// file should also be able to trust its first lines without running
/// anything.
#[test]
fn the_snapshot_is_a_ranked_plan_of_three_items() {
    let text = golden();
    assert!(text.starts_with("RankedPlan {\n"), "{text}");
    assert!(text.ends_with("}\n"), "the file ends in one newline");
    assert_eq!(text.matches("RankItem {").count(), 3);
    assert_eq!(text.matches("ChainFeasibleWithoutWcet").count(), 1);
    assert!(text.is_ascii(), "the snapshot is keyboard ASCII");
}

// ---------------------------------------------------------------------------
// 2. authored rates are derive-only for the scheduler
// ---------------------------------------------------------------------------

/// `topics.<t>.rate_hz` and `pub.<ep>.min_rate_hz` are runtime promises
/// the monitors read; no mapper does. The resolved fixture carries none of
/// them (phase 70 made them consequences of the timer). Add one to every
/// topic and every publisher, at a rate that CONTRADICTS the timer, and the
/// ranking is the snapshot; remove every one and it is the snapshot again.
/// The scheduler's only rate is the trigger's.
#[test]
fn authored_rates_change_nothing_the_scheduler_sees() {
    let base = fixture();
    let expected = golden();

    // A model promising a rate nothing else agrees with.
    let mut promised = base.clone();
    let topics: Vec<(String, TopicWiring)> = promised
        .structure
        .topics
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    assert_eq!(topics.len(), 3);
    for (fqn, wiring) in &topics {
        promised.contracts.topics.insert(
            fqn.clone(),
            TopicContract {
                rate_hz: Some(7.0),
                ..Default::default()
            },
        );
        for ep in &wiring.publishers {
            promised.contracts.pub_endpoints.insert(
                ep.clone(),
                PubContract {
                    min_rate_hz: Some(7.0),
                    ..Default::default()
                },
            );
        }
    }
    assert_eq!(promised.contracts.pub_endpoints.len(), 3);
    let (input_promised, plan_promised) = rank(&promised, &DeriveFacts::default());
    assert_eq!(render(&plan_promised), expected, "promises rank nothing");

    // The same model with every promise removed, including the ones the
    // fixture never had, so the test does not depend on what the resolver
    // of the day chose to lower.
    let mut bare = promised.clone();
    for t in bare.contracts.topics.values_mut() {
        t.rate_hz = None;
    }
    for p in bare.contracts.pub_endpoints.values_mut() {
        p.min_rate_hz = None;
    }
    let (input_bare, plan_bare) = rank(&bare, &DeriveFacts::default());
    assert_eq!(
        render(&plan_bare),
        expected,
        "and their absence ranks nothing"
    );

    // The fact both ranked by: the timer's 100 Hz, not the promised 7.
    for input in [&input_promised, &input_bare] {
        let sensor = input.nodes.iter().find(|n| n.name == SENSOR).unwrap();
        assert_eq!(sensor.rate_hz, Some(100.0));
        let filter = input.nodes.iter().find(|n| n.name == FILTER).unwrap();
        assert_eq!(filter.rate_hz, None, "an input path has no rate of its own");
        match &input.chains[0].elements[0] {
            ChainElement::Boundary { period_ms, .. } => assert_eq!(*period_ms, 10.0),
            other => panic!("expected the timer boundary, got {other:?}"),
        }
    }
    assert_eq!(input_promised, input_bare, "the mapper input is identical");
}

// ---------------------------------------------------------------------------
// 3. a pre-R1 model
// ---------------------------------------------------------------------------

/// The fixture with every R1 field removed, as a resolver older than the
/// v0.1.37 pin would have written it, checked to be absent from the wire
/// form and parsed back.
fn pre_r1_model() -> SystemModel {
    let mut model = fixture();
    for pc in model.contracts.node_paths.values_mut() {
        pc.trigger = None;
        pc.sync = None;
        pc.min_latency_ms = None;
    }
    for sc in model.contracts.sub_endpoints.values_mut() {
        sc.buffer = None;
    }
    model.contracts.severity_levels.clear();
    model.contracts.node_criticality.clear();
    let yaml = model.to_yaml_string().unwrap();
    for field in [
        "trigger:",
        "sync:",
        "min_latency_ms:",
        "buffer:",
        "severity_levels:",
        "node_criticality:",
    ] {
        assert!(
            !yaml.contains(field),
            "{field} must be absent from the wire form"
        );
    }
    SystemModel::from_yaml_str(&yaml).unwrap()
}

/// A model resolved before the R1 fields existed: every path Unclassified
/// and named in `paths_without_trigger`, in node order; no rate anywhere;
/// the label still reaching the mapper, because it is all the checker of
/// that era had. And design issue #52's rule for its ROUTE: no chain, an
/// empty rank, the scope path skipped for want of a path on it.
///
/// A hop is attributed to a path by the OUTPUT it publishes, which needs no
/// trigger, so `filter` (whose output feeds the sink) lands on the route
/// even here; `chains_from_view` then declines to link a hop whose path
/// carries no trigger fact, exactly as it declines an undeclared one. R3
/// pinned the pre-fix number (one Unclassified segment, one ranked item) as
/// an ignored twin of this test; R4 landed the arm and this is the one
/// assertion left.
#[test]
fn a_pre_r1_model_yields_no_chain_and_an_empty_rank() {
    let model = pre_r1_model();
    let (input, report) = mapper_input_from_model(&model, &DeriveFacts::default());
    assert_eq!(
        report.paths_without_trigger,
        vec![
            format!("{SENSOR}/tick"),
            format!("{CONTROL}/control"),
            format!("{FILTER}/filter"),
        ],
        "every path, in structure.nodes order"
    );
    for node in &input.nodes {
        assert_eq!(node.rate_hz, None, "{}: no trigger, no rate", node.name);
        for p in &node.paths {
            assert_eq!(p.effective_trigger, EffectiveTrigger::Unclassified);
        }
    }
    let control = input.nodes.iter().find(|n| n.name == CONTROL).unwrap();
    assert_eq!(
        control.criticality,
        Some(ros_launch_manifest_sched::Criticality::High),
        "the label, parsed, where the effective map is absent"
    );

    // The route: found, then skipped for want of a classified hop.
    assert!(input.chains.is_empty(), "no trigger, no link, no chain");
    assert!(report.chains_resolved.is_empty());
    assert_eq!(
        report.chains_skipped,
        vec![SkippedChain {
            scope_path: SCOPE_PATH.to_string(),
            reason: ChainSkip::NoPathOnRoute,
        }]
    );
    let plan = chain_aware_rank(&input);
    assert!(plan.items.is_empty(), "ranks nothing: {:?}", plan.items);
    assert!(plan.warnings.is_empty());
    assert_ne!(render(&plan), golden(), "the snapshot is not this plan");
}

// ---------------------------------------------------------------------------
// 4. the one-path budget rule on a multi-path timer node
// ---------------------------------------------------------------------------

/// play_launch's `sched_derive.rs` (0.11.0) applied two rules to one fact:
/// `extract_paths` gives a node's `budget_us` to `MapperPath::exec_ms` only
/// when the node has one path, while `resolve_chains_derived` gives it to
/// the chain BOUNDARY whatever the path count (R2's contradiction 1). The
/// crate applies the one-path rule in both places. This pins the divergence
/// to a number: on the fixture with a second timer path on the sensor and a
/// 2 ms node budget, play_launch counted the boundary at 2 ms (sampling
/// cost 12 ms, no warning); the derivation counts it at zero, says so, and
/// reaches 12 ms only from a per-path fact.
#[test]
fn a_node_budget_is_not_attributed_to_a_boundary_of_a_multi_path_node() {
    let mut model = fixture();
    // A second timer on the sensor: a 1 Hz diagnostic tick.
    model.contracts.node_paths.insert(
        format!("{SENSOR}/diag"),
        PathContract {
            output: vec![format!("{SENSOR}/diag")],
            trigger: Some(EffectiveTrigger::Timer { rate_hz: 1.0 }),
            ..Default::default()
        },
    );
    model.structure.topics.insert(
        "/perception/diag".to_string(),
        TopicWiring {
            msg_type: "std_msgs/msg/String".to_string(),
            publishers: vec![format!("{SENSOR}/diag")],
            subscribers: Vec::new(),
        },
    );
    // play_launch's platform file: `overrides: { sensor_node: { budget_us:
    // 2000 } }`, which its `DeriveFacts` builder lowers to a node fact under
    // the bare name the selector took.
    let node_budget = DeriveFacts {
        node_exec_ms: BTreeMap::from([("sensor_node".to_string(), 2.0)]),
        ..Default::default()
    };

    let (input, plan) = rank(&model, &node_budget);
    let sensor = input.nodes.iter().find(|n| n.name == SENSOR).unwrap();
    assert_eq!(sensor.paths.len(), 2);
    assert!(
        sensor.paths.iter().all(|p| p.exec_ms.is_none()),
        "a node budget is not split over two paths: {:?}",
        sensor.paths
    );
    assert_eq!(
        sensor.rate_hz,
        Some(100.0),
        "the fastest timer, not the newest"
    );
    let boundary_exec = match &input.chains[0].elements[0] {
        ChainElement::Boundary {
            node,
            path,
            exec_ms,
            ..
        } => {
            assert_eq!((node.as_str(), path.as_str()), (SENSOR, "tick"));
            *exec_ms
        }
        other => panic!("expected the timer boundary, got {other:?}"),
    };
    assert_eq!(
        boundary_exec, None,
        "the boundary follows the same rule as the path"
    );

    // The known number. Sampling cost = period + exec over the boundaries.
    let sampling_ms = |input: &MapperInput| -> f64 {
        input.chains[0]
            .elements
            .iter()
            .filter_map(|e| match e {
                ChainElement::Boundary {
                    period_ms, exec_ms, ..
                } => Some(period_ms + exec_ms.unwrap_or(0.0)),
                ChainElement::Segment { .. } => None,
            })
            .sum()
    };
    let play_launch_counted_ms = 10.0 + 2.0;
    assert_eq!(sampling_ms(&input), 10.0);
    assert_ne!(sampling_ms(&input), play_launch_counted_ms);
    assert_eq!(
        plan.warnings,
        vec![MapWarning::ChainFeasibleWithoutWcet {
            chain: "points_to_cmd".to_string(),
            boundaries_without_wcet: vec![format!("{SENSOR}/tick")],
        }],
        "and the verdict says the boundary was counted at zero"
    );

    // The ranking itself is the snapshot's three items plus the new timer
    // as a non-chain item below them: the divergence is in the evidence
    // the verdict rests on, not in the order.
    let order: Vec<(&str, &str)> = plan
        .items
        .iter()
        .map(|it| (it.node.as_str(), it.path.as_str()))
        .collect();
    assert_eq!(
        order,
        vec![
            (CONTROL, "control"),
            (FILTER, "filter"),
            (SENSOR, "tick"),
            (SENSOR, "diag"),
        ]
    );
    assert_eq!(plan.items[3].coarse_group, None, "diag is not on the chain");
    assert!(plan.items[3].provenance.contains("non-chain"));

    // The same 2 ms as a per-path fact is attributed, and the numbers meet
    // play_launch's: the disagreement was over WHICH fact may say so.
    let path_budget = DeriveFacts {
        path_exec_ms: BTreeMap::from([(format!("{SENSOR}/tick"), 2.0)]),
        ..Default::default()
    };
    let (input, plan) = rank(&model, &path_budget);
    match &input.chains[0].elements[0] {
        ChainElement::Boundary { exec_ms, .. } => assert_eq!(*exec_ms, Some(2.0)),
        other => panic!("expected the timer boundary, got {other:?}"),
    }
    assert_eq!(sampling_ms(&input), play_launch_counted_ms);
    assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
    let tick = sensor_path(&input, "tick");
    assert_eq!(tick.exec_ms, Some(2.0));
    assert_eq!(sensor_path(&input, "diag").exec_ms, None);
}

fn sensor_path<'a>(
    input: &'a MapperInput,
    name: &str,
) -> &'a ros_launch_manifest_sched::MapperPath {
    input
        .nodes
        .iter()
        .find(|n| n.name == SENSOR)
        .and_then(|n| n.paths.iter().find(|p| p.name == name))
        .unwrap()
}
