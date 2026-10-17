//! E4 (evaluator): the six example extensions compute the numbers in
//! `docs/design/extension-examples.md` from their contracts' values.

use rlx::{
    Checked, Style, check_source,
    diag::code,
    eval::{Inputs, Value, eval_check, eval_derive, eval_report, identity, param_value},
    format_source,
    typeck::ItemRef,
    value::ContractValue,
};
use std::{fs, path::Path};

fn read(name: &str) -> Vec<u8> {
    fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/accepted")
            .join(name),
    )
    .unwrap()
}

/// How a test supplies a key: contract text, or a node parameter's raw
/// number (for a `param` key), or a table.
enum In<'a> {
    Text(&'a str),
    Param(f64),
    Table(&'a [(&'a str, &'a str)]),
}

fn inputs(checked: &Checked, given: &[(&str, In)]) -> Inputs {
    let env = &checked.env;
    let mut out = Inputs::new();
    for (key, how) in given {
        let info = env.key(key).unwrap_or_else(|| panic!("no key {key}"));
        let v = match how {
            In::Text(t) => match env.parse_value(t, &info.ty).unwrap() {
                ContractValue::Quantity(q) => Value::from_quantity(q, Some(t)),
                ContractValue::Param(p) => panic!("{key}: {p} is a parameter name"),
            },
            In::Param(raw) => param_value(*raw, &info.ty).expect("a param key"),
            In::Table(rows) => Value::table(&env.parse_table(rows, &info.ty).unwrap()),
        };
        out.insert(info.path.clone(), v);
    }
    out
}

fn close(got: f64, want: f64, tol: f64) {
    assert!(
        (got - want).abs() <= tol,
        "{got} != {want} (tolerance {tol})"
    );
}

#[test]
fn vehicle_braking() {
    let src = read("braking.rlx");
    let c = check_source(&src).unwrap();
    let inp = inputs(
        &c,
        &[
            ("entry_speed", In::Text("30 km/h")),
            ("decel", In::Param(-2.5)),
            ("jerk", In::Param(-1.5)),
            ("response", In::Text("256ms")),
        ],
    );
    let e = eval_derive(&c, "settle.duration", &inp).unwrap();
    close(e.si * 1e3, 4422.67, 0.005);
    assert_eq!(
        e.trace.render(),
        "\
entry_speed = 8.3333 m/s   (30 km/h)
decel = 2.5 m/s^2
jerk = 1.5 m/s^3
response = 256 ms   (256ms)
braking_time(entry_speed, decel, jerk) = 4.1667 s
  v0 = 8.3333 m/s
  a = 2.5 m/s^2
  j = 1.5 m/s^3
  v_r = a^2 / (2 j) = 2.0833 m/s
  if v0 > v_r: 8.3333 m/s > 2.0833 m/s, true: take `then`
settle.duration = response + braking_time(entry_speed, decel, jerk) = 4.4227 s
"
    );
    // Below the ramp speed the other branch runs.
    let slow = inputs(
        &c,
        &[
            ("entry_speed", In::Text("1 m/s")),
            ("decel", In::Param(2.5)),
            ("jerk", In::Param(1.5)),
        ],
    );
    let e = eval_derive(&c, "settle.duration", &slow).unwrap();
    // response defaults to 0 s; sqrt(2 * 1 / 1.5)
    close(e.si, (2.0f64 / 1.5).sqrt(), 1e-12);
    assert!(e.trace.render().contains("response = 0 s   (default)"));
    assert!(e.trace.render().contains("take `else`"));
}

#[test]
fn machinery_safety_distance() {
    let src = read("iso13855.rlx");
    let c = check_source(&src).unwrap();
    let inp = inputs(
        &c,
        &[
            ("detection_capability", In::Text("30 mm")),
            ("device_response", In::Text("15ms")),
            ("installed_distance", In::Text("520 mm")),
            ("reaction_total", In::Text("192 ms")),
        ],
    );
    let o = eval_check(&c, "safety_distance", &inp).unwrap();
    assert!(o.pass);
    let cmp = o.comparison.unwrap();
    close(cmp.lhs, 0.5, 1e-12);
    close(cmp.rhs, 0.52, 1e-12);
    close(cmp.margin.unwrap(), 0.02, 1e-12);
    let t = o.trace.render();
    assert!(
        t.contains("T = device_response + reaction_total = 207 ms"),
        "{t}"
    );
    assert!(t.contains("C = if detection_capability <= 40 mm then 8 * (detection_capability - 14 mm) else 850 mm = 128 mm"), "{t}");
    assert!(t.contains("s_2000 = 2000 mm/s * T + C = 542 mm"), "{t}");
    assert!(t.contains("= 500 mm"), "{t}");
    assert!(
        t.ends_with(
            "safety_distance: required <= installed_distance: 500 mm <= 520 mm (margin 20 mm) PASS\n"
        ),
        "{t}"
    );
}

#[test]
fn medical_occlusion() {
    let src = read("occlusion.rlx");
    let c = check_source(&src).unwrap();
    let inp = inputs(
        &c,
        &[
            ("min_rate", In::Text("1 mL/h")),
            ("compliance", In::Text("0.003 mL/mmHg")),
            ("baseline_pressure", In::Text("20 mmHg")),
            // occlusion_limit_mmhg = 300, a plain number in mmHg.
            ("alarm_threshold", In::Param(300.0)),
            ("signal_delay", In::Text("5s")),
        ],
    );
    let e = eval_derive(&c, "hazard.detection", &inp).unwrap();
    close(e.si / 60.0, 50.48, 0.005);
    assert_eq!(e.display, "50.48 min");
    let r = eval_report(&c, "bolus_on_release", &inp).unwrap();
    close(r.si * 1e6, 0.84, 1e-9);
    assert_eq!(r.display, "0.84 mL");
}

#[test]
fn uav_descent() {
    let src = read("descent.rlx");
    let c = check_source(&src).unwrap();
    let inp = inputs(
        &c,
        &[
            ("max_altitude", In::Text("120 m")),
            ("hover_power", In::Text("600 W")),
            ("battery_capacity", In::Text("222 Wh")),
            ("trigger_soc", In::Param(0.30)),
            ("reserve_soc", In::Param(0.22)),
            ("descent_speed", In::Param(3.0)),
            ("slow_altitude", In::Param(10.0)),
            ("land_speed", In::Param(0.7)),
            ("reaction_total", In::Text("60.95 s")),
        ],
    );
    let e = eval_derive(&c, "settle.duration", &inp).unwrap();
    close(e.si, 50.95, 0.005);
    let o = eval_check(&c, "energy_reserve", &inp).unwrap();
    assert!(o.pass);
    let cmp = o.comparison.unwrap();
    close(cmp.lhs / 3600.0, 10.16, 0.005);
    close(cmp.rhs / 3600.0, 17.76, 0.005);
    assert!(o.trace.render().contains("10.16 Wh <= 17.76 Wh"));
}

#[test]
fn rail_braking_table() {
    let src = read("ebd.rlx");
    let c = check_source(&src).unwrap();
    let table: &[(&str, &str)] = &[("0 km/h", "0.75 m/s^2"), ("100 km/h", "0.70 m/s^2")];
    let inp = inputs(
        &c,
        &[
            ("entry_speed", In::Text("160 km/h")),
            ("distance_to_eoa", In::Text("1600 m")),
            ("traction_cut_off", In::Text("1.0s")),
            ("brake_build_up", In::Text("2.5s")),
            ("a_ebd", In::Table(table)),
        ],
    );
    let e = eval_derive(&c, "settle.duration", &inp).unwrap();
    close(e.si, 64.35, 0.005);
    assert_eq!(
        e.trace.render(),
        "\
entry_speed = 44.44 m/s   (160 km/h)
traction_cut_off = 1 s   (1.0s)
brake_build_up = 2.5 s   (2.5s)
a_ebd = a table of 2 entries
sum over b in a_ebd.bands(0 m/s, entry_speed): (b.hi - b.lo) / b.value = 60.85 s
  b = [0 m/s, 27.78 m/s] 0.75 m/s^2: 37.04 s
  b = [27.78 m/s, 44.44 m/s] 0.7 m/s^2: 23.81 s
settle.duration = traction_cut_off + brake_build_up + sum over b in a_ebd.bands(0 m/s, entry_speed): (b.hi - b.lo) / b.value = 64.35 s
"
    );
    let o = eval_check(&c, "stop_before_eoa", &inp).unwrap();
    assert!(o.pass);
    let cmp = o.comparison.unwrap();
    close(cmp.lhs, 1529.75, 0.005);
    close(cmp.margin.unwrap(), 70.25, 0.005);
}

#[test]
fn process_safety_time() {
    let src = read("pst.rlx");
    let c = check_source(&src).unwrap();
    let inp = inputs(
        &c,
        &[
            // The contract names `pt101_trip_bar` (9 bar), but the key is
            // `param Pressure` with no `unit bar`, so a raw 9 would read as
            // 9 Pa and give 22 s. The example's 4 s needs the parameter in
            // SI, 9e5; the extension should say `param Pressure unit bar`.
            ("trip_setpoint", In::Param(9e5)),
            ("hazard_pressure", In::Text("11 bar")),
            ("max_rise_rate", In::Text("0.5 bar/s")),
            ("valve_stroke", In::Text("2s")),
        ],
    );
    let e = eval_derive(&c, "hazard.ftti", &inp).unwrap();
    close(e.si, 4.0, 1e-12);
    let e = eval_derive(&c, "settle.duration", &inp).unwrap();
    close(e.si, 2.0, 1e-12);
}

#[test]
fn reads_lists_what_to_gather() {
    let src = read("braking.rlx");
    let c = check_source(&src).unwrap();
    let needs = c
        .env
        .reads(&ItemRef::Derive("settle.duration".into()))
        .unwrap();
    let paths: Vec<(&str, bool)> = needs
        .iter()
        .map(|n| (n.path.as_str(), n.has_default))
        .collect();
    assert_eq!(
        paths,
        [
            ("hazard.entry_speed", false),
            ("settle.decel", false),
            ("settle.jerk", false),
            ("settle.response", true)
        ]
    );
    // A check's reads include what its where bindings read.
    let src = read("iso13855.rlx");
    let c = check_source(&src).unwrap();
    let needs = c
        .env
        .reads(&ItemRef::Check("safety_distance".into()))
        .unwrap();
    assert!(needs.iter().any(|n| n.path == "reaction_total"));
    assert!(c.env.reads(&ItemRef::Check("nope".into())).is_none());
}

#[test]
fn identity_is_spelling_independent() {
    let ascii = read("braking.rlx");
    let unicode =
        fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/accepted/unicode/braking.rlx"))
            .unwrap();
    assert_ne!(ascii, unicode);
    let a = identity(&check_source(&ascii).unwrap());
    let u = identity(&check_source(&unicode).unwrap());
    assert_eq!(a, u);
    assert_eq!(a.name, "vehicle.braking");
    assert_eq!(a.version, "1.0.0");
    assert_eq!(a.sha256.len(), 64);
    // The hash is of the canonical form itself.
    let canonical = format_source(&unicode, Style::Ascii).unwrap();
    assert_eq!(canonical.as_bytes(), ascii.as_slice());
}

/// Same inputs, same trace, byte for byte.
#[test]
fn deterministic() {
    let src = read("ebd.rlx");
    let c = check_source(&src).unwrap();
    let table: &[(&str, &str)] = &[("0 km/h", "0.75 m/s^2"), ("100 km/h", "0.70 m/s^2")];
    let inp = inputs(
        &c,
        &[
            ("entry_speed", In::Text("160 km/h")),
            ("distance_to_eoa", In::Text("1600 m")),
            ("traction_cut_off", In::Text("1.0s")),
            ("brake_build_up", In::Text("2.5s")),
            ("a_ebd", In::Table(table)),
        ],
    );
    let a = eval_check(&c, "stop_before_eoa", &inp).unwrap();
    let b = eval_check(&c, "stop_before_eoa", &inp).unwrap();
    assert_eq!(a.trace.render(), b.trace.render());
    assert_eq!(
        a.comparison.unwrap().lhs.to_bits(),
        b.comparison.unwrap().lhs.to_bits()
    );
}

// -- errors -------------------------------------------------------------------

fn err_code(src: &str, item: &str, given: &[(&str, In)]) -> (String, String) {
    let full = format!("extension t.t 0.1.0\n{src}");
    let c = check_source(full.as_bytes()).unwrap_or_else(|d| panic!("{d:?}"));
    let inp = inputs(&c, given);
    let r = if let Some(t) = item.strip_prefix("derive:") {
        eval_derive(&c, t, &inp).map(|_| ())
    } else if let Some(n) = item.strip_prefix("check:") {
        eval_check(&c, n, &inp).map(|_| ())
    } else {
        eval_report(&c, item, &inp).map(|_| ())
    };
    let d = r.expect_err("evaluation should fail");
    (d.code.to_string(), d.message)
}

#[test]
fn missing_input() {
    let (c, m) = err_code(
        "key hazard.v : Velocity\nreport r : Velocity = v\n",
        "r",
        &[],
    );
    assert_eq!(c, code::MISSING_INPUT);
    assert!(m.contains("`hazard.v`"), "{m}");
}

#[test]
fn division_by_zero() {
    let (c, _) = err_code(
        "key hazard.v : Velocity\nkey hazard.a : Acceleration\nreport r : Time = v / a\n",
        "r",
        &[("v", In::Text("1 m/s")), ("a", In::Text("0 m/s^2"))],
    );
    assert_eq!(c, code::DIVISION_BY_ZERO);
}

#[test]
fn negative_sqrt() {
    let (c, m) = err_code(
        "key hazard.x : Area\nreport r : Length = sqrt(x)\n",
        "r",
        &[("x", In::Text("-4 m^2"))],
    );
    assert_eq!(c, code::NEGATIVE_SQRT);
    assert!(m.contains("-4 m^2"), "{m}");
}

#[test]
fn bands_outside_table() {
    let src = "key hazard.v : Velocity\nkey settle.t : table Velocity -> Acceleration\n\
               report r : Time = sum over b in t.bands(v, 50 m/s): (b.hi - b.lo) / b.value\n";
    let table: &[(&str, &str)] = &[("10 m/s", "1 m/s^2")];
    let (c, m) = err_code(
        src,
        "r",
        &[("v", In::Text("5 m/s")), ("t", In::Table(table))],
    );
    assert_eq!(c, code::BANDS);
    assert!(m.contains("starts at 10 m/s"), "{m}");
    let (c, _) = err_code(
        src,
        "r",
        &[("v", In::Text("60 m/s")), ("t", In::Table(table))],
    );
    assert_eq!(c, code::BANDS);
}

#[test]
fn non_finite() {
    let (c, _) = err_code(
        "key hazard.x : Scalar\nreport r : Scalar = x^1000\n",
        "r",
        &[("x", In::Text("1e10"))],
    );
    assert_eq!(c, code::NON_FINITE);
}

#[test]
fn input_of_the_wrong_kind() {
    let src = "extension t.t 0.1.0\nkey hazard.v : Velocity\nreport r : Velocity = v\n";
    let c = check_source(src.as_bytes()).unwrap();
    let mut inp = Inputs::new();
    inp.insert("hazard.v".into(), Value::Table(vec![(0.0, 1.0)]));
    assert_eq!(eval_report(&c, "r", &inp).unwrap_err().code, code::INPUT);
    let mut inp = Inputs::new();
    let time = c.env.dim("Time").unwrap();
    inp.insert(
        "hazard.v".into(),
        Value::Quantity {
            si: 1.0,
            dim: time,
            written: None,
        },
    );
    let d = eval_report(&c, "r", &inp).unwrap_err();
    assert_eq!(d.code, code::INPUT);
    assert!(d.message.contains("Time"), "{}", d.message);
    assert_eq!(
        eval_report(&c, "nope", &inp).unwrap_err().code,
        code::NO_ITEM
    );
}

#[test]
fn outside_the_domain() {
    let src = read("braking.rlx");
    let c = check_source(&src).unwrap();
    let inp = inputs(
        &c,
        &[
            ("entry_speed", In::Text("80 m/s")),
            ("decel", In::Param(2.5)),
            ("jerk", In::Param(1.5)),
        ],
    );
    let d = eval_derive(&c, "settle.duration", &inp).unwrap_err();
    assert_eq!(d.code, code::DOMAIN);
    assert_eq!(
        d.message,
        "`entry_speed` = 80 m/s is outside its domain (0 m/s, 70 m/s]; the worst-case claim \
         holds only inside it"
    );
    // A predicate constraint.
    let (c, m) = err_code(
        "key hazard.a : Pressure\nkey hazard.b : Pressure\n\
         derive hazard.ftti : Time = 1 s\n  domain: a > b\n",
        "derive:hazard.ftti",
        &[("a", In::Text("1 bar")), ("b", In::Text("2 bar"))],
    );
    assert_eq!(c, code::DOMAIN);
    assert!(
        m.contains("`a > b` does not hold (100 kPa > 200 kPa)"),
        "{m}"
    );
}
