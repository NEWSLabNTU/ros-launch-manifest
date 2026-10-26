//! Worst-case proofs (phase 91 E5) over the design's examples: the gate is
//! that the vehicle, medical and rail claims are proved, the UAV claim as
//! first written is refuted with a counterexample and proved once the
//! suggested domain is added, and a deliberately wrong direction is refuted.
#![cfg(feature = "smt")]

use rlx::{
    check_source,
    eval::{Inputs, Value},
    prove::{Claim, Options, Verdict, prove_all},
    typeck::ItemRef,
};
use std::{collections::BTreeMap, fs, path::Path};

fn accepted(name: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/accepted")
            .join(name),
    )
    .unwrap()
}

fn prove_text(src: &str, opts: &Options) -> Vec<Claim> {
    let checked = check_source(src.as_bytes()).unwrap_or_else(|d| panic!("{d:?}"));
    prove_all(&checked, opts)
}

fn verdicts(claims: &[Claim]) -> Vec<(String, &'static str)> {
    claims
        .iter()
        .map(|c| {
            let v = match c.verdict {
                Verdict::Proved => "proved",
                Verdict::Refuted(_) => "refuted",
                Verdict::Unknown(_) => "unknown",
            };
            (format!("{}: {}", c.subject(), c.statement()), v)
        })
        .collect()
}

fn all_proved(claims: &[Claim]) {
    assert!(!claims.is_empty());
    for c in claims {
        assert_eq!(c.verdict, Verdict::Proved, "{}", c.report());
    }
}

#[test]
fn the_braking_profile_is_proved() {
    let claims = prove_text(&accepted("braking.rlx"), &Options::default());
    assert_eq!(claims.len(), 4);
    all_proved(&claims);
}

#[test]
fn a_wrong_direction_is_refuted_with_a_counterexample_the_evaluator_reproduces() {
    let src = accepted("braking.rlx").replace(
        "increasing in entry_speed, response",
        "decreasing in entry_speed; increasing in response",
    );
    let claims = prove_text(&src, &Options::default());
    let c = &claims[0];
    assert_eq!(c.statement(), "decreasing in entry_speed");
    let Verdict::Refuted(cex) = &c.verdict else {
        panic!("{}", c.report())
    };
    // The counterexample is the evaluator's: two derived values, the lower
    // speed giving the smaller settle.
    assert!(
        cex.at_lo.ends_with('s') && cex.at_hi.ends_with('s'),
        "{cex:?}"
    );
    assert!(c.report().contains("counterexample (Z3)"), "{}", c.report());
    all_proved(&claims[1..]);
}

#[test]
fn the_medical_and_process_derivations_are_proved() {
    all_proved(&prove_text(&accepted("occlusion.rlx"), &Options::default()));
    all_proved(&prove_text(&accepted("pst.rlx"), &Options::default()));
    all_proved(&prove_text(&accepted("iso13855.rlx"), &Options::default()));
}

#[test]
fn a_missing_sign_is_refuted_and_the_sign_is_suggested() {
    // occlusion without its `compliance > 0`: a negative compliance turns
    // the detection time negative and its direction over.
    let src = accepted("occlusion.rlx").replace(",\n          compliance > 0 mL/mmHg", "");
    let claims = prove_text(&src, &Options::default());
    let c = claims
        .iter()
        .find(|c| c.var == "min_rate")
        .expect("min_rate's claim");
    assert!(matches!(c.verdict, Verdict::Refuted(_)), "{}", c.report());
    assert_eq!(
        c.holds_on.as_deref(),
        Some(&["compliance > 0 L/Pa".to_string()][..]),
        "{}",
        c.report()
    );
}

/// The rail example at the contract's table keys (0 and 100 km/h): all
/// twelve claims are proved. The check's claims over the speeds and times
/// square a speed that grows with acceleration times the dead time, which
/// Z3 decided only once the prover orders the `where` bindings first.
#[test]
fn the_rail_claims_are_proved_at_the_written_keys() {
    let claims = prove_text(&accepted("ebd.rlx"), &rail_options());
    assert_eq!(claims.len(), 12);
    for c in &claims {
        assert_eq!(c.verdict, Verdict::Proved, "{}", c.report());
        assert!(
            c.report().ends_with("(for a_safe's 2 keys as written)"),
            "{}",
            c.report()
        );
    }
}

/// The same check with the braking distance written inline, as rail.ebd
/// 1.0.0 wrote it: no binding names the sum, and the claims are still
/// proved, through the bindings that remain.
#[test]
fn the_rail_check_is_proved_with_the_braking_distance_inline() {
    let src = accepted("ebd.rlx").replace(
        "    and braking = sum over b in a_safe.bands(0 m/s, v_bec): (b.hi^2 - b.lo^2) / (2 b.value)\n    and stopping_distance = v0 * dead + acceleration * dead^2 / 2 + braking\n",
        "    and stopping_distance =\n      v0 * dead + acceleration * dead^2 / 2\n      + sum over b in a_safe.bands(0 m/s, v_bec): (b.hi^2 - b.lo^2) / (2 b.value)\n",
    );
    assert!(!src.contains("braking ="), "the replacement missed");
    let claims = prove_text(&src, &rail_options());
    let check: Vec<_> = claims
        .iter()
        .filter(|c| c.report().starts_with("check stop_before_svl"))
        .collect();
    assert_eq!(check.len(), 6);
    for c in check {
        assert_eq!(c.verdict, Verdict::Proved, "{}", c.report());
    }
}

/// A wrong direction on the rail check is refuted, not left unknown: the
/// binding facts make a true claim provable and must not hide a false one,
/// and the counterexample is the model of the last satisfiable query (one
/// read after an assertion or a timed-out query was absent or stale, which
/// reported "Z3 gave no model" or one the evaluator did not reproduce).
#[test]
fn a_wrong_direction_on_the_rail_check_is_refuted() {
    let src = accepted("ebd.rlx");
    let check = src.find("check stop_before_svl").expect("the check");
    let (head, tail) = src.split_at(check);
    let tail = tail.replacen(
        "    increasing in entry_speed, speed_error, acceleration, traction_cut_off, brake_build_up\n    decreasing in a_safe\n",
        "    decreasing in acceleration\n",
        1,
    );
    assert!(
        tail.contains("decreasing in acceleration\n"),
        "the replacement missed"
    );
    // A short budget: after the refutation the prover looks for a domain
    // that would make the claim hold, and for a backwards claim there is
    // none, so every candidate runs its budget out.
    let opts = Options {
        timeout_ms: 1000,
        ..rail_options()
    };
    let claims = prove_text(&format!("{head}{tail}"), &opts);
    let c = claims
        .iter()
        .find(|c| c.report().starts_with("check stop_before_svl"))
        .expect("the check's claim");
    let Verdict::Refuted(cex) = &c.verdict else {
        panic!("{}", c.report())
    };
    assert_eq!(
        (cex.at_lo.as_str(), cex.at_hi.as_str()),
        ("PASS", "FAIL"),
        "{cex:?}"
    );
}

fn rail_options() -> Options {
    let mut opts = Options::default();
    opts.table_keys
        .insert("settle.a_safe".into(), vec![0.0, 100.0 / 3.6]);
    opts
}

fn uav_point() -> Inputs {
    let q = |si: f64, dim: &str| Value::Quantity {
        si,
        dim: rlx::dim::builtin(dim).unwrap(),
        written: None,
    };
    BTreeMap::from([
        ("hazard.max_altitude".into(), q(120.0, "Length")),
        ("hazard.hover_power".into(), q(600.0, "Power")),
        ("settle.descent_speed".into(), q(3.0, "Velocity")),
        ("settle.slow_altitude".into(), q(10.0, "Length")),
        ("settle.land_speed".into(), q(0.7, "Velocity")),
    ])
}

/// The design's UAV story: the claim as first written is refuted; the help
/// names a domain that admits the contract's values; with it added, proved.
#[test]
fn the_uav_claim_is_refuted_then_proved_with_the_suggested_domain() {
    let opts = Options {
        point: uav_point(),
        ..Options::default()
    };
    let src = accepted("descent.rlx");
    let claims = prove_text(&src, &opts);
    let c = claims
        .iter()
        .find(|c| c.var == "slow_altitude")
        .expect("slow_altitude's claim");
    let Verdict::Refuted(_) = &c.verdict else {
        panic!("{}", c.report())
    };
    let holds_on = c.holds_on.clone().expect("a suggested domain");
    assert!(
        holds_on.contains(&"land_speed <= descent_speed".to_string()),
        "{}",
        c.report()
    );
    // Gather every claim's suggestion into the derive's domain.
    let mut derive_domain: Vec<String> = Vec::new();
    let mut check_domain: Vec<String> = Vec::new();
    for c in &claims {
        let into = match c.item {
            ItemRef::Check(_) => &mut check_domain,
            _ => &mut derive_domain,
        };
        for h in c.holds_on.clone().expect("each claim has a suggestion") {
            if !into.contains(&h) {
                into.push(h);
            }
        }
    }
    let fixed = src
        .replace(
            "    decreasing in descent_speed, land_speed\n",
            &format!(
                "    decreasing in descent_speed, land_speed\n  domain: {}\n",
                derive_domain.join(", ")
            ),
        )
        .replace(
            "  worst_case: increasing in reaction_total\n",
            &format!(
                "  worst_case: increasing in reaction_total\n  domain: {}\n",
                check_domain.join(", ")
            ),
        );
    let claims = prove_text(&fixed, &opts);
    assert_eq!(verdicts(&claims).len(), 5);
    all_proved(&claims);
}

#[test]
fn an_empty_domain_is_not_a_proof() {
    let src = accepted("braking.rlx").replace(
        "entry_speed in (0 m/s, 70 m/s]",
        "entry_speed in (0 m/s, 70 m/s], entry_speed > 80 m/s",
    );
    for c in prove_text(&src, &Options::default()) {
        let Verdict::Unknown(why) = &c.verdict else {
            panic!("{}", c.report())
        };
        assert!(why.starts_with("vacuous"), "{why}");
    }
}

#[test]
fn a_power_beyond_the_encoding_is_unknown_not_a_guess() {
    let src = "extension t.pow 0.1.0\n\
               key hazard.x : Fraction\n\
               key hazard.y : Fraction\n\
               derive hazard.ftti : Time = 1 s * x ^ y\n\
               \x20 worst_case: increasing in x\n";
    let claims = prove_text(src, &Options::default());
    let Verdict::Unknown(why) = &claims[0].verdict else {
        panic!("{}", claims[0].report())
    };
    assert!(why.contains("beyond the encoding"), "{why}");
}
