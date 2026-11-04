//! Phase 98 N5: no retired word (`rung`, `ladder`, `severity_levels`, an
//! old rule id, ...) in user-facing text. The scan is
//! `scripts/check_terms.py`; this test is what runs it, and its negative
//! control, on every `cargo test`.

use std::path::Path;
use std::process::Command;

fn run(args: &[&str]) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let out = Command::new("python3")
        .arg(root.join("scripts/check_terms.py"))
        .args(args)
        .current_dir(root)
        .output()
        .expect("python3 runs scripts/check_terms.py (the phase 98 terms gate needs it)");
    assert!(
        out.status.success(),
        "check_terms.py {args:?} failed:\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn the_terms_gate_reports_an_injected_word() {
    run(&["--self-test"]);
}

#[test]
fn no_retired_word_in_user_facing_text() {
    run(&[]);
}
