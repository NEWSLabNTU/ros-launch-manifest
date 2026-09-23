//! Unit tests, on models hand-lowered from the checker fixtures under
//! `tests/fixtures` (`manifest_pipeline`, `manifest_periodic`,
//! `manifest_chain_a`, `manifest_parallel_pipeline`, `manifest_multi_scope`)
//! the way `play_launch resolve` lowers them: names fully qualified,
//! endpoints as `"<node FQN>/<endpoint>"` refs, durations in milliseconds,
//! and the trigger as the checker's `effective_trigger()`. This repository
//! has no resolver, so the lowering is by hand; R3's fixture model is the
//! resolver's own output.

use std::collections::BTreeMap;

use ros_launch_manifest_model::{
    Contracts, NodeInstance, PathContract, PubContract, ScopeInfo, SrvContract, Structure,
    SubContract, SystemModel, TopicContract, TopicWiring,
};
use ros_launch_manifest_sched::{
    ChainElement, ConcurrencyContract, Criticality, EffectiveTrigger, MapperMiss, SegmentNode,
    chain_aware_rank,
};

use super::*;

// ---------------------------------------------------------------------------
// builders
// ---------------------------------------------------------------------------

fn s(v: &str) -> String {
    v.to_string()
}

fn refs(node: &str, eps: &[&str]) -> Vec<String> {
    eps.iter().map(|e| format!("{node}/{e}")).collect()
}

struct Model(SystemModel);

impl Model {
    fn new(scopes: &[(&str, Option<&str>)]) -> Self {
        let mut m = SystemModel::default();
        for (id, parent) in scopes {
            m.structure.scopes.insert(
                s(id),
                ScopeInfo {
                    parent: parent.map(s),
                    ..Default::default()
                },
            );
        }
        Model(m)
    }

    fn node(mut self, fqn: &str, scope: &str, criticality: Option<&str>) -> Self {
        self.0.structure.nodes.insert(
            s(fqn),
            NodeInstance {
                scope: s(scope),
                criticality: criticality.map(s),
                ..Default::default()
            },
        );
        self
    }

    /// A node path. `trigger` is the checker's effective trigger; `inputs`
    /// are the Input trigger's endpoint refs (empty otherwise).
    fn path(
        mut self,
        node: &str,
        name: &str,
        trigger: Option<EffectiveTrigger>,
        outputs: &[&str],
        max_latency_ms: Option<f64>,
    ) -> Self {
        let input = match &trigger {
            Some(EffectiveTrigger::Input(eps)) => eps.clone(),
            _ => Vec::new(),
        };
        self.0.contracts.node_paths.insert(
            format!("{node}/{name}"),
            PathContract {
                input,
                output: refs(node, outputs),
                max_latency_ms,
                trigger,
                ..Default::default()
            },
        );
        self
    }

    fn timer(
        self,
        node: &str,
        name: &str,
        rate_hz: f64,
        outputs: &[&str],
        ms: Option<f64>,
    ) -> Self {
        self.path(
            node,
            name,
            Some(EffectiveTrigger::Timer { rate_hz }),
            outputs,
            ms,
        )
    }

    fn input(
        self,
        node: &str,
        name: &str,
        inputs: &[&str],
        outputs: &[&str],
        ms: Option<f64>,
    ) -> Self {
        self.path(
            node,
            name,
            Some(EffectiveTrigger::Input(refs(node, inputs))),
            outputs,
            ms,
        )
    }

    fn topic(mut self, fqn: &str, pubs: &[&str], subs: &[&str]) -> Self {
        self.0.structure.topics.insert(
            s(fqn),
            TopicWiring {
                msg_type: s("t"),
                publishers: pubs.iter().map(|p| s(p)).collect(),
                subscribers: subs.iter().map(|p| s(p)).collect(),
            },
        );
        self
    }

    fn state_sub(mut self, ep_ref: &str) -> Self {
        self.0.contracts.sub_endpoints.insert(
            s(ep_ref),
            SubContract {
                state: true,
                ..Default::default()
            },
        );
        self
    }

    fn scope_path(
        mut self,
        scope: &str,
        name: &str,
        inputs: &[&str],
        outputs: &[&str],
        ms: Option<f64>,
    ) -> Self {
        self.0.contracts.scope_paths.insert(
            format!("{scope}/{name}"),
            PathContract {
                input: inputs.iter().map(|t| s(t)).collect(),
                output: outputs.iter().map(|t| s(t)).collect(),
                max_latency_ms: ms,
                ..Default::default()
            },
        );
        self
    }

    fn concurrency(mut self, node: &str, exclusive: Vec<Vec<&str>>) -> Self {
        self.0.contracts.node_concurrency.insert(
            s(node),
            ConcurrencyContract {
                exclusive: exclusive
                    .iter()
                    .map(|g| g.iter().map(|p| s(p)).collect())
                    .collect(),
            },
        );
        self
    }

    fn done(self) -> SystemModel {
        self.0
    }
}

fn node_of<'a>(input: &'a MapperInput, fqn: &str) -> &'a MapperNode {
    input
        .nodes
        .iter()
        .find(|n| n.name == fqn)
        .unwrap_or_else(|| panic!("no node {fqn}"))
}

fn path_of<'a>(input: &'a MapperInput, fqn: &str, path: &str) -> &'a MapperPath {
    node_of(input, fqn)
        .paths
        .iter()
        .find(|p| p.name == path)
        .unwrap_or_else(|| panic!("no path {fqn}/{path}"))
}

fn segment(nodes: &[(&str, &str)]) -> ChainElement {
    ChainElement::Segment {
        nodes_in_topo_order: nodes
            .iter()
            .map(|(n, p)| SegmentNode {
                node: s(n),
                path: s(p),
            })
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// fixtures
// ---------------------------------------------------------------------------

/// `manifest_pipeline`, resolved under `/perception`: an INPUT chain
/// `lidar -> cropbox -> ground_filter -> fusion -> tracker`, with a STATE
/// edge into `fusion` from a camera branch that must not join the route.
fn pipeline() -> SystemModel {
    Model::new(&[("/perception", None)])
        .node("/perception/cropbox", "/perception", Some("medium"))
        .node("/perception/ground_filter", "/perception", None)
        .node("/perception/camera_detector", "/perception", Some("low"))
        .node("/perception/fusion", "/perception", Some("high"))
        .node("/perception/tracker", "/perception", Some("MEDIUM"))
        .input(
            "/perception/cropbox",
            "main",
            &["raw_points"],
            &["cropped_points"],
            Some(5.0),
        )
        .input(
            "/perception/ground_filter",
            "filter",
            &["input_points"],
            &["no_ground", "ground"],
            Some(15.0),
        )
        .input(
            "/perception/camera_detector",
            "detect",
            &["image"],
            &["objects"],
            Some(90.0),
        )
        .input(
            "/perception/fusion",
            "main",
            &["lidar_objects"],
            &["fused_objects"],
            Some(10.0),
        )
        .input(
            "/perception/tracker",
            "main",
            &["detected_objects"],
            &["tracked_objects"],
            Some(20.0),
        )
        .topic(
            "/sensing/lidar/pointcloud",
            &[],
            &["/perception/cropbox/raw_points"],
        )
        .topic(
            "/sensing/camera/image",
            &[],
            &["/perception/camera_detector/image"],
        )
        .topic(
            "/perception/cropped_points",
            &["/perception/cropbox/cropped_points"],
            &["/perception/ground_filter/input_points"],
        )
        .topic(
            "/perception/no_ground_points",
            &["/perception/ground_filter/no_ground"],
            &["/perception/fusion/lidar_objects"],
        )
        .topic(
            "/perception/camera_detections",
            &["/perception/camera_detector/objects"],
            &["/perception/fusion/camera_objects"],
        )
        .topic(
            "/perception/fused_objects",
            &["/perception/fusion/fused_objects"],
            &["/perception/tracker/detected_objects"],
        )
        .topic(
            "/perception/tracked_objects",
            &["/perception/tracker/tracked_objects"],
            &[],
        )
        .state_sub("/perception/fusion/camera_objects")
        .scope_path(
            "/perception",
            "perception",
            &["/sensing/lidar/pointcloud"],
            &["/perception/tracked_objects"],
            Some(60.0),
        )
        .done()
}

/// The `rt_workspace` shape that `manifest_graph`'s acceptance test pins
/// (`chain_checks` reports it as 25 ms = 15 ms segment + 10 ms sampling):
/// a TIMER chain, `sensor (100 Hz) -> filter -> control`.
fn timer_chain() -> SystemModel {
    Model::new(&[("/", None)])
        .node("/perception/sensor_node", "/", Some("low"))
        .node("/perception/filter_component", "/", Some("medium"))
        .node("/control/control_node", "/", Some("high"))
        .timer(
            "/perception/sensor_node",
            "tick",
            100.0,
            &["points_raw"],
            None,
        )
        .input(
            "/perception/filter_component",
            "filter",
            &["points_raw"],
            &["points_filtered"],
            Some(5.0),
        )
        .input(
            "/control/control_node",
            "control",
            &["points_filtered"],
            &["cmd"],
            Some(10.0),
        )
        .topic(
            "/perception/points_raw",
            &["/perception/sensor_node/points_raw"],
            &["/perception/filter_component/points_raw"],
        )
        .topic(
            "/perception/points_filtered",
            &["/perception/filter_component/points_filtered"],
            &["/control/control_node/points_filtered"],
        )
        .topic("/control/cmd", &["/control/control_node/cmd"], &[])
        .scope_path(
            "/",
            "sensing_to_actuation",
            &["/perception/points_raw"],
            &["/control/cmd"],
            Some(20.0),
        )
        .done()
}

/// `manifest_periodic`: three timer nodes whose subscriptions are all
/// `state: true`, plus the `min_rate_hz` promises the old nano-ros
/// derivation read the rate from.
fn periodic() -> SystemModel {
    let mut m = Model::new(&[("/", None)])
        .node("/localization", "/", None)
        .node("/trajectory_planner", "/", None)
        .node("/controller", "/", None)
        .timer(
            "/localization",
            "periodic",
            50.0,
            &["current_pose"],
            Some(2.0),
        )
        .timer(
            "/trajectory_planner",
            "periodic",
            10.0,
            &["trajectory"],
            Some(30.0),
        )
        .timer("/controller", "periodic", 30.0, &["control_cmd"], Some(5.0))
        .topic(
            "/current_pose",
            &["/localization/current_pose"],
            &["/trajectory_planner/current_pose"],
        )
        .topic(
            "/trajectory",
            &["/trajectory_planner/trajectory"],
            &["/controller/trajectory"],
        )
        .topic("/control_cmd", &["/controller/control_cmd"], &[])
        .state_sub("/trajectory_planner/current_pose")
        .state_sub("/controller/trajectory")
        .scope_path(
            "/",
            "pose_to_cmd",
            &["/current_pose"],
            &["/control_cmd"],
            Some(50.0),
        )
        .done();
    for (ep, hz) in [
        ("/localization/current_pose", 50.0),
        ("/trajectory_planner/trajectory", 10.0),
        ("/controller/control_cmd", 30.0),
    ] {
        m.contracts.pub_endpoints.insert(
            s(ep),
            PubContract {
                min_rate_hz: Some(hz),
                ..Default::default()
            },
        );
    }
    m.contracts.topics.insert(
        s("/trajectory"),
        TopicContract {
            rate_hz: Some(10.0),
            ..Default::default()
        },
    );
    m
}

/// A ONCE loader feeding an input path: `map_loader (once) -> planner`.
/// The loader's output promises a `min_rate_hz` of 1, which the old
/// derivation would have read as a 1 Hz timer.
fn once_loader() -> SystemModel {
    let mut m = Model::new(&[("/", None)])
        .node("/map_loader", "/", Some("low"))
        .node("/planner", "/", Some("high"))
        .path(
            "/map_loader",
            "load",
            Some(EffectiveTrigger::Once),
            &["map"],
            Some(500.0),
        )
        .input("/planner", "plan", &["map"], &["route"], Some(40.0))
        .topic("/map", &["/map_loader/map"], &["/planner/map"])
        .topic("/route", &["/planner/route"], &[])
        .scope_path("/", "map_to_route", &["/map"], &["/route"], Some(1000.0))
        .done();
    m.contracts.pub_endpoints.insert(
        s("/map_loader/map"),
        PubContract {
            min_rate_hz: Some(1.0),
            ..Default::default()
        },
    );
    m
}

// ---------------------------------------------------------------------------
// the pre-#52 model: trigger absent everywhere
// ---------------------------------------------------------------------------

/// A model resolved before R1 carries `trigger: None` on every path. Every
/// path is `Unclassified` -- the timer with a 50 Hz `min_rate_hz` promise
/// included -- the report names them all, no chain resolves, and the
/// ranker ranks nothing. Loud, not wrong.
#[test]
fn a_model_without_triggers_is_unclassified_everywhere_and_says_so() {
    let mut model = periodic();
    for pc in model.contracts.node_paths.values_mut() {
        pc.trigger = None;
    }
    let (input, report) = mapper_input_from_model(&model, &DeriveFacts::default());

    assert_eq!(
        report.paths_without_trigger,
        vec![
            "/localization/periodic",
            "/trajectory_planner/periodic",
            "/controller/periodic"
        ],
        "every path, in node order"
    );
    for node in &input.nodes {
        assert_eq!(node.rate_hz, None, "{}: no trigger, no rate", node.name);
        for p in &node.paths {
            assert_eq!(p.effective_trigger, EffectiveTrigger::Unclassified);
        }
    }
    assert!(input.chains.is_empty());
    assert!(chain_aware_rank(&input).items.is_empty(), "ranks nothing");
}

/// The same model with its triggers: nothing is reported, and the rate is
/// the timer's.
#[test]
fn a_migrated_model_reports_no_path_without_trigger() {
    let (input, report) = mapper_input_from_model(&periodic(), &DeriveFacts::default());
    assert!(report.paths_without_trigger.is_empty());
    assert_eq!(node_of(&input, "/localization").rate_hz, Some(50.0));
    assert_eq!(
        path_of(&input, "/localization", "periodic").effective_trigger,
        EffectiveTrigger::Timer { rate_hz: 50.0 }
    );
}

// ---------------------------------------------------------------------------
// per-node facts
// ---------------------------------------------------------------------------

/// `rate_hz` comes from timer triggers and from nothing else. The planner
/// promises 10 Hz on its endpoint AND on its topic; delete the trigger's
/// rate and the promises must not fill the gap.
#[test]
fn rate_hz_is_the_fastest_timer_and_ignores_every_promise() {
    let mut model = periodic();
    model
        .contracts
        .node_paths
        .get_mut("/trajectory_planner/periodic")
        .unwrap()
        .trigger = Some(EffectiveTrigger::Input(vec![s(
        "/trajectory_planner/current_pose",
    )]));
    let (input, _) = mapper_input_from_model(&model, &DeriveFacts::default());
    assert_eq!(
        node_of(&input, "/trajectory_planner").rate_hz,
        None,
        "a 10 Hz min_rate_hz promise is not a timer"
    );
    assert_eq!(node_of(&input, "/controller").rate_hz, Some(30.0));

    // Two timers on one node: the fastest.
    let model = Model::new(&[("/", None)])
        .node("/n", "/", None)
        .timer("/n", "slow", 1.0, &["a"], None)
        .timer("/n", "fast", 25.0, &["b"], None)
        .path(
            "/n",
            "event",
            Some(EffectiveTrigger::Spontaneous),
            &["c"],
            None,
        )
        .done();
    let (input, _) = mapper_input_from_model(&model, &DeriveFacts::default());
    assert_eq!(node_of(&input, "/n").rate_hz, Some(25.0));
}

/// `manifest_chain_a`: a spontaneous path beside a 1 Hz timer. The timer
/// gives the rate; the spontaneous path gives nothing and ranks nowhere.
#[test]
fn chain_a_spontaneous_beside_a_timer() {
    let model = Model::new(&[("/link", None)])
        .node("/link/producer", "/link", None)
        .path(
            "/link/producer",
            "make",
            Some(EffectiveTrigger::Spontaneous),
            &["out"],
            Some(40.0),
        )
        .timer("/link/producer", "make_slow", 1.0, &["out2"], Some(5.0))
        .topic("/link/out", &["/link/producer/out"], &[])
        .topic("/link/out2", &["/link/producer/out2"], &[])
        .done();
    let (input, report) = mapper_input_from_model(&model, &DeriveFacts::default());
    let producer = node_of(&input, "/link/producer");
    assert_eq!(producer.rate_hz, Some(1.0));
    assert_eq!(producer.deadline_us, Some(5_000));
    assert!(
        !producer.claims_concurrency,
        "no declaration: everything serialises"
    );
    assert!(report.paths_without_trigger.is_empty());
    let ranked = chain_aware_rank(&input);
    assert_eq!(ranked.items.len(), 1);
    assert_eq!(ranked.items[0].path, "make_slow");
}

/// A `once` loader is not a timer, whatever its output promises, and is a
/// segment link on the route it starts, never a boundary.
#[test]
fn a_once_loader_is_neither_a_timer_nor_a_boundary() {
    let (input, report) = mapper_input_from_model(&once_loader(), &DeriveFacts::default());
    let loader = node_of(&input, "/map_loader");
    assert_eq!(
        loader.rate_hz, None,
        "min_rate_hz: 1 on its output is a promise, not a timer"
    );
    assert_eq!(
        path_of(&input, "/map_loader", "load").effective_trigger,
        EffectiveTrigger::Once
    );
    assert_eq!(report.chains_resolved, vec!["//map_to_route"]);
    assert_eq!(input.chains.len(), 1);
    assert_eq!(
        input.chains[0].elements,
        vec![segment(&[("/map_loader", "load"), ("/planner", "plan")])]
    );
    assert_eq!(
        input.chains[0].criticality,
        Criticality::High,
        "max over members"
    );
}

#[test]
fn deadline_is_the_tightest_of_path_budgets_and_service_bounds() {
    let mut model = Model::new(&[("/", None)])
        .node("/svc", "/", None)
        .node("/quiet", "/", None)
        .input("/svc", "loose", &["a"], &["x"], Some(50.0))
        .input("/svc", "tight", &["b"], &["y"], Some(10.0))
        .done();
    model.contracts.srv_endpoints.insert(
        s("/svc/lookup"),
        SrvContract {
            max_response_ms: Some(2.5),
        },
    );
    let (input, _) = mapper_input_from_model(&model, &DeriveFacts::default());
    let svc = node_of(&input, "/svc");
    assert_eq!(
        svc.deadline_us,
        Some(2_500),
        "the service bound is the tightest claim"
    );
    assert_eq!(svc.path_budget_ms, Some(2.5));
    let quiet = node_of(&input, "/quiet");
    assert_eq!(quiet.deadline_us, None);
    assert_eq!(quiet.path_budget_ms, None);
    assert!(quiet.paths.is_empty());
}

/// The effective map wins over the label; the label is read where the
/// map has no entry; case-insensitive; unrecognised is `None`.
#[test]
fn criticality_prefers_the_effective_map_over_the_label() {
    let mut model = Model::new(&[("/", None)])
        .node("/hazard_reaches", "/", Some("low"))
        .node("/label_only", "/", Some("HIGH"))
        .node("/nonsense", "/", Some("urgent"))
        .node("/silent", "/", None)
        .done();
    model
        .contracts
        .node_criticality
        .insert(s("/hazard_reaches"), Criticality::High);
    let (input, _) = mapper_input_from_model(&model, &DeriveFacts::default());
    assert_eq!(
        node_of(&input, "/hazard_reaches").criticality,
        Some(Criticality::High)
    );
    assert_eq!(
        node_of(&input, "/label_only").criticality,
        Some(Criticality::High)
    );
    assert_eq!(node_of(&input, "/nonsense").criticality, None);
    assert_eq!(node_of(&input, "/silent").criticality, None);
}

/// The merged-group rule, on the case the two consumers disagreed on:
/// `[[a, b], [c]]` over `{a, b, c}` leaves `a` and `c` free to run at once.
#[test]
fn claims_concurrency_by_the_merged_group_rule() {
    fn claims(paths: &[&str], decl: Option<Vec<Vec<&str>>>) -> bool {
        let mut m = Model::new(&[("/", None)]).node("/n", "/", None);
        for p in paths {
            m = m.input("/n", p, &["in"], &["out"], Some(1.0));
        }
        if let Some(decl) = decl {
            m = m.concurrency("/n", decl);
        }
        let (input, _) = mapper_input_from_model(&m.done(), &DeriveFacts::default());
        node_of(&input, "/n").claims_concurrency
    }
    assert!(
        !claims(&["a", "b", "c"], None),
        "absent: everything serialises"
    );
    assert!(
        claims(&["a", "b"], Some(vec![])),
        "exclusive: [] claims full concurrency"
    );
    assert!(
        !claims(&["a"], Some(vec![])),
        "one path cannot contend with itself"
    );
    assert!(
        claims(&["a", "b", "c"], Some(vec![vec!["a", "b"], vec!["c"]])),
        "[[a,b],[c]]: a and c may run together"
    );
    assert!(
        !claims(&["a", "b", "c"], Some(vec![vec!["a", "b"], vec!["b", "c"]])),
        "[[a,b],[b,c]] merges to one group"
    );
    assert!(
        claims(&["a", "b", "health"], Some(vec![vec!["a", "b"]])),
        "an unnamed path is concurrent"
    );
}

/// Per-path facts read straight through, and paths come out sorted by name.
#[test]
fn jitter_miss_inputs_and_outputs_read_through() {
    let mut model = timer_chain();
    let pc = model
        .contracts
        .node_paths
        .get_mut("/perception/filter_component/filter")
        .unwrap();
    pc.max_jitter_ms = Some(4.0);
    pc.miss = Some(MapperMiss {
        tolerate_n: Some(1),
        tolerate_w: Some(10),
        ..Default::default()
    });
    let (input, _) = mapper_input_from_model(&model, &DeriveFacts::default());
    let filter = path_of(&input, "/perception/filter_component", "filter");
    assert_eq!(filter.max_jitter_ms, Some(4.0));
    assert_eq!(filter.miss.as_ref().and_then(|m| m.tolerate_w), Some(10));
    assert_eq!(
        filter.inputs,
        vec!["/perception/filter_component/points_raw"]
    );
    assert_eq!(
        filter.outputs,
        vec!["/perception/filter_component/points_filtered"]
    );
    assert_eq!(filter.max_latency_ms, Some(5.0));
    let tick = path_of(&input, "/perception/sensor_node", "tick");
    assert!(tick.inputs.is_empty());
    // Node order is the model's launch-traversal order.
    let names: Vec<&str> = input.nodes.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "/perception/sensor_node",
            "/perception/filter_component",
            "/control/control_node"
        ]
    );
}

// ---------------------------------------------------------------------------
// exec_ms attribution
// ---------------------------------------------------------------------------

/// A per-path fact reaches its path. A per-node fact reaches a path only
/// when the node has exactly one; a bare node name selects too. The same
/// answer lands on a chain boundary. Nothing is invented from a deadline.
#[test]
fn exec_ms_is_attributed_by_the_one_path_rule_on_paths_and_boundaries() {
    let mut model = timer_chain();
    // Give the sensor a second path so its node budget has no home.
    model.contracts.node_paths.insert(
        s("/perception/sensor_node/diag"),
        PathContract {
            output: vec![s("/perception/sensor_node/diag")],
            trigger: Some(EffectiveTrigger::Timer { rate_hz: 1.0 }),
            ..Default::default()
        },
    );
    let facts = DeriveFacts {
        path_exec_ms: BTreeMap::from([(s("/control/control_node/control"), 3.0)]),
        node_exec_ms: BTreeMap::from([
            (s("/perception/sensor_node"), 2.0),
            (s("filter_component"), 1.5),
            (s("/control/control_node"), 9.0),
        ]),
    };
    let (input, _) = mapper_input_from_model(&model, &facts);
    assert_eq!(
        path_of(&input, "/control/control_node", "control").exec_ms,
        Some(3.0),
        "the path fact wins over the node fact"
    );
    assert_eq!(
        path_of(&input, "/perception/filter_component", "filter").exec_ms,
        Some(1.5),
        "a bare-name node fact selects a single-path node"
    );
    assert_eq!(
        path_of(&input, "/perception/sensor_node", "tick").exec_ms,
        None,
        "a node budget is not split over two paths"
    );
    assert_eq!(
        path_of(&input, "/perception/sensor_node", "diag").exec_ms,
        None
    );

    // The boundary follows the same rule: absent here...
    let chain = &input.chains[0];
    match &chain.elements[0] {
        ChainElement::Boundary { node, exec_ms, .. } => {
            assert_eq!(node, "/perception/sensor_node");
            assert_eq!(
                *exec_ms, None,
                "a deadline is never a cost, and a node budget is not split"
            );
        }
        other => panic!("expected a boundary, got {other:?}"),
    }
    // ...and present once the fact is per path.
    let facts = DeriveFacts {
        path_exec_ms: BTreeMap::from([(s("/perception/sensor_node/tick"), 0.4)]),
        ..Default::default()
    };
    let chains = resolve_chains(&model, &facts);
    match &chains[0].elements[0] {
        ChainElement::Boundary { exec_ms, .. } => assert_eq!(*exec_ms, Some(0.4)),
        other => panic!("expected a boundary, got {other:?}"),
    }
}

#[test]
fn no_facts_means_no_exec_ms_anywhere() {
    let (input, _) = mapper_input_from_model(&pipeline(), &DeriveFacts::default());
    assert!(
        input
            .nodes
            .iter()
            .flat_map(|n| &n.paths)
            .all(|p| p.exec_ms.is_none())
    );
}

// ---------------------------------------------------------------------------
// chains
// ---------------------------------------------------------------------------

/// The input chain of `manifest_pipeline`: one segment along the lidar
/// branch. The camera branch reaches `fusion` over a `state: true`
/// subscription, so it is neither on the route nor a fan-in the route
/// waits on; its Low criticality does not dilute the chain's High.
#[test]
fn an_input_chain_is_one_segment_and_a_state_edge_stays_off_it() {
    let (input, report) = mapper_input_from_model(&pipeline(), &DeriveFacts::default());
    assert_eq!(report.chains_resolved, vec!["/perception/perception"]);
    assert!(report.chains_skipped.is_empty());
    assert_eq!(input.chains.len(), 1);
    let chain = &input.chains[0];
    assert_eq!(chain.name, "perception");
    assert_eq!(chain.max_latency_ms, 60.0);
    assert_eq!(chain.criticality, Criticality::High);
    assert_eq!(
        chain.elements,
        vec![segment(&[
            ("/perception/cropbox", "main"),
            ("/perception/ground_filter", "filter"),
            ("/perception/fusion", "main"),
            ("/perception/tracker", "main"),
        ])]
    );
    // Same answer from the standalone entry point.
    assert_eq!(
        resolve_chains(&pipeline(), &DeriveFacts::default()),
        input.chains
    );
}

/// The timer chain: the 100 Hz tick is a boundary of one 10 ms period,
/// followed by one segment. Criticality is the max over members.
#[test]
fn a_timer_chain_starts_with_a_boundary() {
    let (input, report) = mapper_input_from_model(&timer_chain(), &DeriveFacts::default());
    assert_eq!(report.chains_resolved, vec!["//sensing_to_actuation"]);
    let chain = &input.chains[0];
    assert_eq!(chain.criticality, Criticality::High);
    assert_eq!(
        chain.elements,
        vec![
            ChainElement::Boundary {
                node: s("/perception/sensor_node"),
                path: s("tick"),
                period_ms: 10.0,
                exec_ms: None,
            },
            segment(&[
                ("/perception/filter_component", "filter"),
                ("/control/control_node", "control"),
            ]),
        ]
    );
    // The ranker sees the chain: every member ranks under its name.
    let ranked = chain_aware_rank(&input);
    assert_eq!(ranked.items.len(), 3);
    assert!(
        ranked
            .items
            .iter()
            .all(|i| i.coarse_group.as_deref() == Some("sensing_to_actuation")),
        "{:?}",
        ranked.items
    );
}

/// `manifest_periodic`: every hop is a `state: true` subscription, so the
/// route from `/current_pose` to `/control_cmd` does not exist. The state
/// edges are not causal and the chain is skipped, named.
#[test]
fn a_state_only_topology_has_no_route() {
    let (input, report) = mapper_input_from_model(&periodic(), &DeriveFacts::default());
    assert!(input.chains.is_empty());
    assert_eq!(
        report.chains_skipped,
        vec![SkippedChain {
            scope_path: s("//pose_to_cmd"),
            reason: ChainSkip::NoRoute,
        }]
    );
}

/// A feedback loop closed by a `state: true` subscription resolves; the
/// same loop with a causal subscription is a cycle and is reported as one.
#[test]
fn a_state_edge_breaks_a_cycle_and_a_causal_one_does_not() {
    let loop_model = |state: bool| {
        let mut m = Model::new(&[("/", None)])
            .node("/planner", "/", None)
            .node("/controller", "/", None)
            .input(
                "/planner",
                "plan",
                &["odom", "feedback"],
                &["traj"],
                Some(20.0),
            )
            .input(
                "/controller",
                "ctl",
                &["traj"],
                &["cmd", "feedback"],
                Some(5.0),
            )
            .topic("/odom", &[], &["/planner/odom"])
            .topic("/traj", &["/planner/traj"], &["/controller/traj"])
            .topic(
                "/feedback",
                &["/controller/feedback"],
                &["/planner/feedback"],
            )
            .topic("/cmd", &["/controller/cmd"], &[])
            .scope_path("/", "odom_to_cmd", &["/odom"], &["/cmd"], Some(30.0));
        if state {
            m = m.state_sub("/planner/feedback");
        }
        m.done()
    };
    let (input, report) = mapper_input_from_model(&loop_model(true), &DeriveFacts::default());
    assert_eq!(report.chains_resolved, vec!["//odom_to_cmd"]);
    assert_eq!(
        input.chains[0].elements,
        vec![segment(&[("/planner", "plan"), ("/controller", "ctl")])]
    );

    let (input, report) = mapper_input_from_model(&loop_model(false), &DeriveFacts::default());
    assert!(input.chains.is_empty());
    assert_eq!(report.chains_skipped[0].reason, ChainSkip::Cycle);
}

/// `manifest_parallel_pipeline`: at a join only the slowest branch counts,
/// and the route is that branch. `max(50, 30) + 20`, never the sum.
#[test]
fn a_fork_join_route_is_the_slowest_branch() {
    let model = Model::new(&[("/p", None)])
        .node("/p/lidar_detector", "/p", None)
        .node("/p/camera_detector", "/p", None)
        .node("/p/fusion", "/p", None)
        .input(
            "/p/lidar_detector",
            "main",
            &["input"],
            &["out"],
            Some(50.0),
        )
        .input(
            "/p/camera_detector",
            "main",
            &["input"],
            &["out"],
            Some(30.0),
        )
        .input(
            "/p/fusion",
            "main",
            &["lidar", "camera"],
            &["out"],
            Some(20.0),
        )
        .topic(
            "/sensor/raw",
            &[],
            &["/p/lidar_detector/input", "/p/camera_detector/input"],
        )
        .topic(
            "/p/lidar_objects",
            &["/p/lidar_detector/out"],
            &["/p/fusion/lidar"],
        )
        .topic(
            "/p/camera_objects",
            &["/p/camera_detector/out"],
            &["/p/fusion/camera"],
        )
        .topic("/p/fused_objects", &["/p/fusion/out"], &[])
        .scope_path(
            "/p",
            "pipeline",
            &["/sensor/raw"],
            &["/p/fused_objects"],
            Some(70.0),
        )
        .done();
    let chains = resolve_chains(&model, &DeriveFacts::default());
    assert_eq!(
        chains[0].elements,
        vec![segment(&[
            ("/p/lidar_detector", "main"),
            ("/p/fusion", "main")
        ])]
    );

    // The graph's own arithmetic, for the total the checker would report.
    let view = view::ModelView::from_model(&model);
    let graph = graph::build_global_graph(&view);
    let sg = graph::subgraph_for_scope_path(
        &graph,
        graph::subtree_scopes(&view, "/p"),
        &[s("/sensor/raw")],
        &[s("/p/fused_objects")],
    );
    let route = graph::critical_path(&sg).expect("fork-join has a route");
    assert_eq!(route.total_ms, 70.0);
    assert_eq!(route.sampling_cost_ms, 0.0);
}

/// `manifest_multi_scope`: a scope path sees only its subtree. The
/// perception path stops at perception's border; the root path spans
/// both children; a path whose output is only published outside its
/// subtree has no sink.
#[test]
fn a_scope_path_is_confined_to_its_subtree() {
    let model = Model::new(&[
        ("/", None),
        ("/perception", Some("/")),
        ("/planning", Some("/")),
    ])
    .node("/driver", "/", None)
    .node("/perception/cropbox", "/perception", None)
    .node("/perception/detector", "/perception", None)
    .node("/planning/planner", "/planning", None)
    .timer("/driver", "periodic", 10.0, &["raw_points"], Some(1.0))
    .input(
        "/perception/cropbox",
        "main",
        &["input"],
        &["cropped"],
        Some(5.0),
    )
    .input(
        "/perception/detector",
        "main",
        &["points"],
        &["objects"],
        Some(30.0),
    )
    .input(
        "/planning/planner",
        "main",
        &["obstacles"],
        &["trajectory"],
        Some(20.0),
    )
    .topic(
        "/raw_points",
        &["/driver/raw_points"],
        &["/perception/cropbox/input"],
    )
    .topic(
        "/perception/filtered_points",
        &["/perception/cropbox/cropped"],
        &["/perception/detector/points"],
    )
    .topic(
        "/detections_topic",
        &["/perception/detector/objects"],
        &["/planning/planner/obstacles"],
    )
    .topic(
        "/planning/trajectory",
        &["/planning/planner/trajectory"],
        &[],
    )
    .scope_path(
        "/perception",
        "detect",
        &["/raw_points"],
        &["/detections_topic"],
        Some(40.0),
    )
    .scope_path(
        "/perception",
        "overreach",
        &["/raw_points"],
        &["/planning/trajectory"],
        Some(40.0),
    )
    .scope_path(
        "/",
        "end_to_end",
        &["/raw_points"],
        &["/planning/trajectory"],
        Some(80.0),
    )
    .scope_path(
        "/",
        "unbudgeted",
        &["/raw_points"],
        &["/planning/trajectory"],
        None,
    )
    .done();
    let (input, report) = mapper_input_from_model(&model, &DeriveFacts::default());
    assert_eq!(
        report.chains_resolved,
        vec!["//end_to_end", "/perception/detect"]
    );
    assert_eq!(
        report.chains_skipped,
        vec![
            SkippedChain {
                scope_path: s("//unbudgeted"),
                reason: ChainSkip::NoBudget,
            },
            SkippedChain {
                scope_path: s("/perception/overreach"),
                reason: ChainSkip::NoEndpoints,
            },
        ]
    );
    let by_name = |n: &str| input.chains.iter().find(|c| c.name == n).unwrap();
    assert_eq!(
        by_name("detect").elements,
        vec![segment(&[
            ("/perception/cropbox", "main"),
            ("/perception/detector", "main")
        ])],
        "the driver's timer is outside /perception and must not lead the route"
    );
    assert_eq!(
        by_name("end_to_end").elements,
        vec![
            ChainElement::Boundary {
                node: s("/driver"),
                path: s("periodic"),
                period_ms: 100.0,
                exec_ms: None,
            },
            segment(&[
                ("/perception/cropbox", "main"),
                ("/perception/detector", "main"),
                ("/planning/planner", "main"),
            ]),
        ]
    );
}

/// A route through a node that declares no path lands on the node's
/// fallback vertex, which carries no trigger fact: it is skipped from the
/// elements, and a route made only of such hops yields no chain.
#[test]
fn a_hop_through_an_undeclared_node_is_not_an_element() {
    let model = Model::new(&[("/", None)])
        .node("/src", "/", None)
        .node("/relay", "/", None)
        .node("/sink", "/", None)
        .timer("/src", "tick", 20.0, &["out"], None)
        .input("/sink", "main", &["in"], &["done"], Some(3.0))
        .topic("/a", &["/src/out"], &["/relay/in"])
        .topic("/b", &["/relay/out"], &["/sink/in"])
        .topic("/done", &["/sink/done"], &[])
        .scope_path("/", "through_relay", &["/a"], &["/done"], Some(100.0))
        .done();
    let chains = resolve_chains(&model, &DeriveFacts::default());
    assert_eq!(
        chains[0].elements,
        vec![
            ChainElement::Boundary {
                node: s("/src"),
                path: s("tick"),
                period_ms: 50.0,
                exec_ms: None,
            },
            segment(&[("/sink", "main")]),
        ],
        "the relay is crossed but has no path to name"
    );

    let mut model = model;
    model.contracts.node_paths.clear();
    let (_, report) = mapper_input_from_model(&model, &DeriveFacts::default());
    assert_eq!(report.chains_skipped[0].reason, ChainSkip::NoPathOnRoute);
}

/// A trigger lowered with bare endpoint names resolves the same route as
/// one lowered with refs.
#[test]
fn a_bare_endpoint_name_in_a_trigger_matches_the_ref() {
    let mut model = timer_chain();
    model
        .contracts
        .node_paths
        .get_mut("/perception/filter_component/filter")
        .unwrap()
        .trigger = Some(EffectiveTrigger::Input(vec![s("points_raw")]));
    let chains = resolve_chains(&model, &DeriveFacts::default());
    assert_eq!(chains.len(), 1);
    assert_eq!(chains[0].elements.len(), 2);
    assert!(view::endpoint_matches(
        "points_raw",
        "/a/b",
        "/a/b/points_raw"
    ));
    assert!(!view::endpoint_matches(
        "points_raw",
        "/a",
        "/a/b/points_raw"
    ));
    assert!(!view::endpoint_matches(
        "/x/points_raw",
        "/a/b",
        "/a/b/points_raw"
    ));
}

/// The model's own round trip carries everything the derivation reads: a
/// model written to YAML and read back derives byte-identically.
#[test]
fn the_derivation_survives_the_model_round_trip() {
    let model = timer_chain();
    let yaml = model.to_yaml_string().unwrap();
    let back = SystemModel::from_yaml_str(&yaml).unwrap();
    assert_eq!(
        mapper_input_from_model(&model, &DeriveFacts::default()),
        mapper_input_from_model(&back, &DeriveFacts::default())
    );
    // The model types used by the builders above are all reachable.
    let _ = (Contracts::default(), Structure::default());
}

/// R5 of issue 52: `MapperNode::scope` is the node's NAMESPACE, which is
/// what `sched`'s `[[assign]] scope =` selector matches, and not the
/// owning launch-file scope id the model happens to key nodes by.
#[test]
fn mapper_node_scope_is_the_namespace_not_the_scope_id() {
    assert_eq!(node_namespace("/perception/lidar/a"), "/perception/lidar");
    assert_eq!(node_namespace("/control/c"), "/control");
    // A node at the root has the root namespace, not an empty string: an
    // empty scope would match no selector, and `norm_scope("")` is not `/`.
    assert_eq!(node_namespace("/a"), "/");
    // Defensive: a name that carries no separator at all.
    assert_eq!(node_namespace("a"), "/");
}

/// The namespace this crate derives is the one `sched` would bind against.
/// Stated as the selector rule itself so the two cannot drift apart:
/// a selector matches when it equals the namespace or is an ancestor of it.
#[test]
fn derived_scope_binds_under_the_selector_rule() {
    let ns = node_namespace("/perception/lidar/deep/b");
    assert_eq!(ns, "/perception/lidar/deep");
    let matches = |sel: &str| ns == sel || ns.starts_with(&format!("{}/", sel));
    assert!(matches("/perception/lidar/deep"));
    assert!(matches("/perception/lidar"));
    assert!(matches("/perception"));
    // A false prefix is not an ancestor.
    assert!(!matches("/perception/lid"));
}

/// R5 end to end, on a fixture where the two trees genuinely disagree:
/// `timer_chain` declares one launch scope, `/`, and puts nodes at
/// `/perception/...` and `/control/...` inside it. Copying the scope id
/// gave every node the scope `/`, so a platform file's
/// `[[assign]] scope = "/perception"` selected NOTHING and the nodes fell
/// to the default tier -- silently, because an unmatched selector is only
/// an error when it matches no node in the SYSTEM, and `/` always does.
#[test]
fn a_node_below_its_launch_scope_carries_its_own_namespace() {
    let (input, _) = mapper_input_from_model(&timer_chain(), &DeriveFacts::default());

    let scope_of = |name: &str| {
        input
            .nodes
            .iter()
            .find(|n| n.name == name)
            .unwrap_or_else(|| panic!("{name} missing from the derived input"))
            .scope
            .clone()
    };

    assert_eq!(scope_of("/perception/sensor_node"), "/perception");
    assert_eq!(scope_of("/perception/filter_component"), "/perception");
    assert_eq!(scope_of("/control/control_node"), "/control");

    // The model's own scope id is untouched: it is a different tree, and
    // `graph.rs` still tests subtree membership with it.
    assert_eq!(
        timer_chain().structure.nodes["/perception/sensor_node"].scope,
        "/"
    );

    // The point of the fix: a `/perception` selector now binds the two
    // perception nodes and leaves the control node alone.
    // Sorted: the derived order is the ranking's business, not this test's.
    let under = |sel: &str| {
        let mut v = input
            .nodes
            .iter()
            .filter(|n| n.scope == sel || n.scope.starts_with(&format!("{sel}/")))
            .map(|n| n.name.as_str())
            .collect::<Vec<_>>();
        v.sort_unstable();
        v
    };
    assert_eq!(
        under("/perception"),
        vec!["/perception/filter_component", "/perception/sensor_node"]
    );
    assert_eq!(under("/control"), vec!["/control/control_node"]);
}
