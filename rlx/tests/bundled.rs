//! The extensions bundled with the checker (phase 91 E8): each type-checks,
//! `vehicle.braking` reproduces the braking arithmetic the core carried from
//! v0.1.46 to v0.1.71 (the island's numbers), and its worst case is proved.

use rlx::{
    check_source,
    eval::{Inputs, Value, eval_derive, param_value},
    typeck::{ItemRef, KeyTy},
};
use ros_launch_manifest_types::extension::{BUNDLED, LEGACY_KEYS, VEHICLE_BRAKING};
use std::collections::BTreeMap;

fn braking() -> &'static str {
    BUNDLED
        .iter()
        .find(|(f, _)| f.starts_with(VEHICLE_BRAKING))
        .unwrap()
        .1
}

#[test]
fn every_bundled_extension_type_checks_and_is_named_by_its_file() {
    for (file, src) in BUNDLED {
        let checked = check_source(src.as_bytes()).unwrap_or_else(|d| panic!("{file}: {d:?}"));
        let id = rlx::eval::identity(&checked);
        assert_eq!(format!("{}.rlx", id.name), file);
    }
}

#[test]
fn the_legacy_keys_are_the_keys_vehicle_braking_declares() {
    let checked = check_source(braking().as_bytes()).unwrap();
    for (ctx, key) in LEGACY_KEYS {
        let prefix = match ctx {
            ros_launch_manifest_types::field_table::Context::Hazard => "hazard",
            _ => "settle",
        };
        let path = format!("{prefix}.{key}");
        assert!(checked.env.key(&path).is_some(), "{path}");
    }
}

/// The settle at `v0` m/s, decel `a` m/s^2 and jerk `j` m/s^3 (signed, as
/// Autoware writes them), with `response` seconds, in ms.
fn settle_ms(v0: f64, a: f64, j: f64, response: Option<f64>) -> f64 {
    let checked = check_source(braking().as_bytes()).unwrap();
    let ty = |p: &str| checked.env.key(p).unwrap().ty.clone();
    let q = |si: f64, dim: &str| Value::Quantity {
        si,
        dim: rlx::dim::builtin(dim).unwrap(),
        written: None,
    };
    let param = |raw: f64, p: &str| {
        let t = ty(p);
        assert!(matches!(t, KeyTy::Param { .. }), "{p}");
        param_value(raw, &t).unwrap()
    };
    let mut inputs: Inputs = BTreeMap::from([
        ("hazard.entry_speed".into(), q(v0, "Velocity")),
        ("settle.decel".into(), param(a, "settle.decel")),
        ("settle.jerk".into(), param(j, "settle.jerk")),
    ]);
    if let Some(r) = response {
        inputs.insert("settle.response".into(), q(r, "Time"));
    }
    eval_derive(&checked, "settle.duration", &inputs)
        .unwrap()
        .si
        * 1000.0
}

/// decel 2.5, jerk 1.5, so v_r = 2.0833 m/s: 3.0 m/s is past the ramp and
/// gives 2033.33 ms; phase 7's observed 4.23 m/s gives 2525.33 ms; below
/// v_r the vehicle stops inside the ramp; the two branches meet at v_r; the
/// response is added.
#[test]
fn vehicle_braking_reproduces_the_islands_numbers() {
    assert!((settle_ms(3.0, -2.5, -1.5, None) - 2033.333).abs() < 0.01);
    assert!((settle_ms(4.23, -2.5, -1.5, None) - 2525.333).abs() < 0.01);
    let slow = settle_ms(1.5, 2.5, 1.5, None);
    assert!((slow - (2.0f64 * 1.5 / 1.5).sqrt() * 1000.0).abs() < 1e-9);
    let v_r = 2.5 * 2.5 / 3.0;
    assert!((settle_ms(v_r, 2.5, 1.5, None) - settle_ms(v_r + 1e-9, 2.5, 1.5, None)).abs() < 1e-3);
    assert!(
        (settle_ms(3.0, -2.5, -1.5, Some(0.3)) - 2333.333).abs() < 0.01,
        "the response is added"
    );
}

#[cfg(feature = "smt")]
#[test]
fn vehicle_braking_worst_case_is_proved() {
    use rlx::prove::{Options, Verdict, prove};
    let checked = check_source(braking().as_bytes()).unwrap();
    let claims = prove(
        &checked,
        &ItemRef::Derive("settle.duration".into()),
        &Options::default(),
    );
    assert_eq!(claims.len(), 4);
    for c in &claims {
        assert_eq!(c.verdict, Verdict::Proved, "{}", c.report());
    }
}

#[cfg(not(feature = "smt"))]
#[allow(dead_code)]
fn _item(_: ItemRef) {}
