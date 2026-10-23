//! The reference profiles' rlx (phase 91 E6): every `functions:` block
//! type-checks, and ISO 26262's decomposition rule reproduces Part 9's
//! table of permitted decompositions.

use rlx::{
    check_source,
    eval::{Inputs, Value, eval_check},
};
use ros_launch_manifest_types::profile::reference;
use std::collections::BTreeMap;

#[test]
fn every_profile_function_block_type_checks() {
    for p in reference() {
        if let Some(src) = p.functions_source() {
            check_source(src.as_bytes()).unwrap_or_else(|d| panic!("{}: {d:?}", p.name));
        }
    }
}

#[test]
fn asil_decomposition_matches_iso26262_part_9() {
    let p = reference()
        .into_iter()
        .find(|p| p.name == "iso26262")
        .unwrap();
    let src = format!(
        "{}\nkey hazard.parent : Fraction\nkey hazard.a : Fraction\nkey hazard.b : Fraction\n\
         check permitted: asil_decomposition_margin(parent, a, b) >= 0\n",
        p.functions_source().unwrap()
    );
    let checked = check_source(src.as_bytes()).unwrap_or_else(|d| panic!("{d:?}"));
    let rank = |l: &str| p.rank(l).unwrap() as f64;
    let permitted = |parent: &str, a: &str, b: &str| {
        let q = |si: f64| Value::Quantity {
            si,
            dim: rlx::dim::builtin("Fraction").unwrap(),
            written: None,
        };
        let inputs: Inputs = BTreeMap::from([
            ("hazard.parent".into(), q(rank(parent))),
            ("hazard.a".into(), q(rank(a))),
            ("hazard.b".into(), q(rank(b))),
        ]);
        eval_check(&checked, "permitted", &inputs).unwrap().pass
    };
    // Part 9, clause 5.4: every permitted split of each ASIL.
    for (parent, a, b) in [
        ("ASIL_D", "ASIL_C", "ASIL_A"),
        ("ASIL_D", "ASIL_B", "ASIL_B"),
        ("ASIL_D", "ASIL_D", "QM"),
        ("ASIL_C", "ASIL_B", "ASIL_A"),
        ("ASIL_C", "ASIL_C", "QM"),
        ("ASIL_B", "ASIL_A", "ASIL_A"),
        ("ASIL_B", "ASIL_B", "QM"),
        ("ASIL_A", "ASIL_A", "QM"),
    ] {
        assert!(permitted(parent, a, b), "{parent} = {a} + {b}");
    }
    // And the splits that fall short by one.
    for (parent, a, b) in [
        ("ASIL_D", "ASIL_B", "ASIL_A"),
        ("ASIL_D", "ASIL_C", "QM"),
        ("ASIL_C", "ASIL_A", "ASIL_A"),
        ("ASIL_B", "ASIL_A", "QM"),
        ("ASIL_A", "QM", "QM"),
    ] {
        assert!(!permitted(parent, a, b), "{parent} != {a} + {b}");
    }
}
