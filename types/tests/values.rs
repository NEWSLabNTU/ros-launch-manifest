//! Values read from the launch tree (play_launch phase 94): the grammar,
//! every parse-time refusal, and the two passes.

use ros_launch_manifest_types::{
    OptBound, Trigger, parse_manifest_str,
    values::{Lookup, ParamNode, Read, ReadKind, RefAt, resolve_manifest_str},
};

fn err(src: &str) -> String {
    parse_manifest_str(src).unwrap_err().to_string()
}

/// A value with no reference is the literal it always was: same bound, no
/// expression kept.
#[test]
fn a_literal_parses_as_before() {
    let m = parse_manifest_str(
        "nodes:\n  n:\n    sub:\n      a: { max_age: 500ms, min_rate_hz: 10 }\n      \
         b: { max_age: { duration: 500ms, source: requirement } }\n",
    )
    .unwrap();
    assert!(m.value_refs.is_empty());
    assert!(m.lets.is_empty());
    assert_eq!(m.nodes["n"].subscribers["a"].max_age.ms(), Some(500.0));
    assert_eq!(m.nodes["n"].subscribers["a"].min_rate_hz.hz(), Some(10.0));
    assert_eq!(m.nodes["n"].subscribers["b"].max_age.ms(), Some(500.0));
}

/// Without a launch tree a reference is unknown: the key is absent, the
/// expression is kept with its path and node.
#[test]
fn a_reference_alone_is_unknown() {
    let m = parse_manifest_str(
        "nodes:\n  n:\n    pub:\n      o: { min_rate_hz: $(param rate) Hz }\n    \
         sub:\n      i: { max_age: { duration: $(param t) s, param: t } }\n    \
         paths:\n      p:\n        trigger: { timer: { rate_hz: $(param rate) Hz } }\n        \
         output: [o]\n",
    )
    .unwrap();
    let n = &m.nodes["n"];
    assert!(n.publishers["o"].min_rate_hz.is_none());
    assert!(n.subscribers["i"].max_age.is_none());
    // An unknown rate is an unknown trigger, never a guessed timer.
    assert!(n.paths["p"].trigger.is_none());
    let paths: Vec<&str> = m.value_refs.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "nodes.n.pub.o.min_rate_hz",
            "nodes.n.sub.i.max_age.duration",
            "nodes.n.paths.p.trigger.timer.rate_hz",
        ]
    );
    assert!(m.value_refs.iter().all(|r| r.node.as_deref() == Some("n")));
    assert_eq!(m.value_refs[1].key(), "max_age");
}

/// A `let:` of literals evaluates without a launch tree, and a value that
/// reads it is put in place, carrying its provenance.
#[test]
fn let_of_literals_evaluates_in_place() {
    let m = parse_manifest_str(
        "let:\n  odd: 8.33 m/s\n  ftti: 500ms\n  twice: $(let ftti) * 2\n\
         hazards:\n  h: { guards: [/a], ftti: $(let twice), detection: $(let ftti) / 5 }\n\
         modes:\n  m: { requires: [/a], window: { duration: $(let ftti), param: n.t } }\n",
    )
    .unwrap();
    assert_eq!(m.lets.len(), 3);
    assert_eq!(m.hazards["h"].ftti.ms(), Some(1000.0));
    assert_eq!(m.hazards["h"].detection.ms(), Some(100.0));
    let w = m.modes["m"].window.as_ref().unwrap();
    assert_eq!(w.ms(), Some(500.0));
    assert!(w.param.is_some());
    let r = m
        .value_refs
        .iter()
        .find(|r| r.path == "hazards.h.ftti")
        .unwrap();
    let reads = &r.resolved.as_ref().unwrap().reads;
    assert!(matches!(&reads[0].kind, ReadKind::Let { name } if name == "twice"));
}

#[test]
fn every_parse_time_refusal() {
    for (src, needle) in [
        // A reference without a unit.
        (
            "nodes:\n  n:\n    sub:\n      i: { max_age: $(param t) }\n",
            "`$(param t)` needs a unit",
        ),
        // A unit that is not one.
        (
            "nodes:\n  n:\n    sub:\n      i: { max_age: $(param t) sec }\n",
            "`sec` is not a unit",
        ),
        // A malformed reference.
        (
            "nodes:\n  n:\n    sub:\n      i: { max_age: $(param) s }\n",
            "malformed",
        ),
        (
            "nodes:\n  n:\n    sub:\n      i: { max_age: $(env T) s }\n",
            "`$(env …)` is not read",
        ),
        // A `$(let)` naming nothing.
        (
            "nodes:\n  n:\n    sub:\n      i: { max_age: $(let nope) }\n",
            "names no earlier entry",
        ),
        // A forward reference, and a cycle.
        ("let:\n  a: $(let b) * 2\n  b: 1 s\n", "declared after it"),
        ("let:\n  a: $(let a)\n", "reads itself"),
        ("let:\n  a: $(let b)\n  b: $(let a)\n", "declared after it"),
        // A `let:` entry reading a parameter without a unit.
        ("let:\n  a: $(param x)\n", "needs a unit"),
        // `let:` belongs to the file.
        (
            "includes:\n  c:\n    let:\n      a: 1 s\n",
            "`let:` is the contract file's",
        ),
        // `source:` beside a reference, in each braces form.
        (
            "nodes:\n  n:\n    sub:\n      i: { max_age: { duration: $(param t) s, source: requirement } }\n",
            "source-on-reference",
        ),
        (
            "modes:\n  m: { requires: [/a], window: { duration: $(param n t) s, source: requirement } }\n",
            "source-on-reference",
        ),
        (
            "nodes:\n  n:\n    pub:\n      o: {}\n    paths:\n      p:\n        trigger: { timer: { rate_hz: { value: $(param r) Hz, source: allocated } } }\n        output: [o]\n",
            "source-on-reference",
        ),
        // A default that is not a number for a number type.
        (
            "nodes:\n  n:\n    params:\n      r: { type: double, default: fast }\n",
            "`fast` is not a number",
        ),
    ] {
        let e = err(src);
        assert!(e.contains(needle), "{src}\n-> {e}");
    }
}

/// `params.<p>.default` parses.
#[test]
fn a_parameter_default_parses() {
    let m =
        parse_manifest_str("nodes:\n  n:\n    params:\n      r: { type: double, default: 2.5 }\n")
            .unwrap();
    let p = &m.nodes["n"].params.as_ref().unwrap()["r"];
    assert_eq!(p.default.as_deref(), Some("2.5"));
}

/// A count takes a reference without a unit.
#[test]
fn a_count_takes_a_bare_reference() {
    let m = parse_manifest_str(
        "topics:\n  /a: { type: std_msgs/msg/Empty, drop: { max_consecutive: $(param n k) } }\n",
    )
    .unwrap();
    assert_eq!(m.value_refs[0].path, "topics./a.drop.max_consecutive");
}

struct Tree;

impl Lookup for Tree {
    fn param(&self, _: &RefAt, node: &ParamNode, name: &str) -> Read {
        match (node, name) {
            (ParamNode::Here(Some(n)), "rate_hz") if n == "lidar" => Read::Value {
                raw: 100.0,
                at: "bringup.launch.xml".into(),
                node: Some("/safety/lidar".into()),
            },
            (ParamNode::Here(Some(n)), "watchdog_ms") if n == "det" => Read::Value {
                raw: 100.0,
                at: "bringup.launch.xml".into(),
                node: Some("/safety/det".into()),
            },
            _ => Read::Unknown,
        }
    }
    fn var(&self, _: &RefAt, name: &str, _: Option<&str>) -> Read {
        match name {
            "lidar_hz" => Read::Value {
                raw: 100.0,
                at: "command line".into(),
                node: None,
            },
            _ => Read::Unknown,
        }
    }
}

/// The two passes: the values the lookup gives are put in place, a `ms`
/// parameter is read in `ms`, and a timer read from the launch tree is a
/// timer again.
#[test]
fn the_launch_tree_fills_the_values() {
    let src = "let:\n  lidar_rate: $(var lidar_hz) Hz\n\
               nodes:\n  lidar:\n    pub:\n      scan: { min_rate_hz: $(let lidar_rate) }\n    \
               paths:\n      sample:\n        trigger: { timer: { rate_hz: $(param rate_hz) Hz } }\n        \
               output: [scan]\n  \
               det:\n    sub:\n      scan:\n        min_rate_hz: $(let lidar_rate)\n        \
               qos: { liveliness: manual_by_topic, lease_duration: $(param watchdog_ms) ms }\n        \
               max_age: 3 / $(let lidar_rate)\n";
    let (pr, res) = resolve_manifest_str(src, &Tree).unwrap();
    assert!(res.errors.is_empty(), "{:?}", res.errors);
    let m = pr.manifest;
    assert!(matches!(
        m.nodes["lidar"].paths["sample"].trigger,
        Some(Trigger::Timer { rate_hz, .. }) if rate_hz == 100.0
    ));
    let det = &m.nodes["det"].subscribers["scan"];
    assert_eq!(det.min_rate_hz.hz(), Some(100.0));
    assert_eq!(det.max_age.ms(), Some(30.0));
    assert_eq!(
        det.qos
            .as_ref()
            .unwrap()
            .lease_duration
            .unwrap()
            .as_millis_f64(),
        100.0
    );
    let lease = m
        .value_refs
        .iter()
        .find(|r| r.path == "nodes.det.sub.scan.qos.lease_duration")
        .unwrap();
    let reads = &lease.resolved.as_ref().unwrap().reads;
    assert_eq!(reads[0].written, "$(param watchdog_ms) ms");
    assert_eq!(reads[0].value, "100 ms");
    assert!(matches!(&reads[0].kind, ReadKind::Param { node, name }
        if node == "/safety/det" && name == "watchdog_ms"));
}
