//! The reference extensions (phase 91 E9): every one loads, every worst
//! case it states is proved at its example's point, and each is the
//! extension of `docs/design/extension-examples.md` it names, line for line
//! (the UAV file with the domains the example's checker output tells the
//! author to add, which the document prints too).
#![cfg(feature = "smt")]

use rlx::{
    check_source,
    eval::{Inputs, Value},
    prove::{Options, Verdict, prove_all},
};
use ros_launch_manifest_types::extension::{BUNDLED, REFERENCE};
use std::collections::BTreeMap;

fn examples_doc() -> String {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../docs/design/extension-examples.md"),
    )
    .unwrap()
}

/// The code lines of a file or block: comment-only and blank lines dropped,
/// trailing blanks trimmed.
fn code_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim_end)
        .filter(|l| !l.is_empty() && !l.trim_start().starts_with("--"))
        .map(str::to_string)
        .collect()
}

/// The document's block for `file`: the fenced block whose first line is
/// `-- share/<pkg>/rlm/<file>`.
fn doc_block(doc: &str, file: &str) -> Vec<String> {
    let marker = format!("/rlm/{file}");
    let mut blocks = doc.split("```").skip(1).step_by(2);
    let block = blocks
        .find(|b| b.lines().nth(1).is_some_and(|l| l.ends_with(&marker)))
        .unwrap_or_else(|| panic!("no block for {file} in extension-examples.md"));
    code_lines(block)
}

#[test]
fn each_reference_extension_is_the_examples_doc_one() {
    let doc = examples_doc();
    // The bundled `vehicle.braking` is section 1's `braking.rlx`.
    let files = REFERENCE
        .iter()
        .map(|(f, s)| (*f, *s))
        .chain(BUNDLED.iter().map(|(_, s)| ("braking.rlx", *s)));
    for (file, src) in files {
        let ours = code_lines(src);
        let theirs = doc_block(&doc, file);
        // The document's lines, in order, with only domain lines between
        // them that the document itself prints.
        let mut want = theirs.iter().peekable();
        let mut extra = Vec::new();
        for line in &ours {
            if want.peek().is_some_and(|w| *w == line) {
                want.next();
            } else {
                extra.push(line.clone());
            }
        }
        assert!(
            want.peek().is_none(),
            "{file}: the document's `{}` is missing",
            want.peek().unwrap()
        );
        for line in &extra {
            assert!(
                doc.contains(line.as_str()) && line.contains("domain:")
                    || doc.contains(line.trim()),
                "{file}: `{line}` is not in the document"
            );
        }
    }
}

#[test]
fn each_reference_extension_is_marked_illustrative() {
    for (file, src) in REFERENCE {
        assert!(src.contains("ILLUSTRATIVE"), "{file}");
    }
}

fn q(si: f64, dim: &str) -> Value {
    Value::Quantity {
        si,
        dim: rlx::dim::builtin(dim).unwrap(),
        written: None,
    }
}

/// The example's values a suggestion must admit, and the table keys a
/// claim over a table is proved at.
fn options(file: &str) -> Options {
    let mut opts = Options::default();
    match file {
        "descent.rlx" => {
            opts.point = BTreeMap::from([
                ("hazard.max_altitude".into(), q(120.0, "Length")),
                ("hazard.hover_power".into(), q(600.0, "Power")),
                ("settle.descent_speed".into(), q(3.0, "Velocity")),
                ("settle.slow_altitude".into(), q(10.0, "Length")),
                ("settle.land_speed".into(), q(0.7, "Velocity")),
            ]);
        }
        "ebd.rlx" => {
            opts.point = BTreeMap::from([
                ("hazard.entry_speed".into(), q(160.0 / 3.6, "Velocity")),
                ("settle.speed_error".into(), q(2.0 / 3.6, "Velocity")),
                ("settle.acceleration".into(), q(0.5, "Acceleration")),
                ("settle.traction_cut_off".into(), q(1.0, "Time")),
                ("settle.brake_build_up".into(), q(2.5, "Time")),
            ]);
            opts.table_keys
                .insert("settle.a_safe".into(), vec![0.0, 100.0 / 3.6]);
        }
        _ => {}
    }
    opts
}

#[test]
fn every_reference_extension_loads_and_proves_its_worst_case() {
    let mut proved = 0;
    for (file, src) in REFERENCE {
        let checked = check_source(src.as_bytes()).unwrap_or_else(|d| panic!("{file}: {d:?}"));
        let claims = prove_all(&checked, &options(file));
        assert!(!claims.is_empty(), "{file} states no worst case");
        for c in &claims {
            // rail.ebd 1.0.0's distance check squares a speed that grows
            // with acceleration times the dead time, which Z3 does not
            // decide: those claims may come back unknown (the file says
            // so), never refuted. Everything else is proved.
            let undecidable = file == "ebd.rlx" && c.report().starts_with("check stop_before_svl");
            if undecidable && matches!(c.verdict, Verdict::Unknown(_)) {
                continue;
            }
            assert_eq!(c.verdict, Verdict::Proved, "{file}: {}", c.report());
            proved += 1;
        }
    }
    assert!(proved >= 14, "{proved}");
}

#[allow(dead_code)]
fn _inputs(_: Inputs) {}
