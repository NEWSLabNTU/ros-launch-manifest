//! The E2 gate: every refused file is refused with exactly its expected
//! diagnostics, every accepted file parses, and formatting round-trips.

use rlx::{Style, check_source, diag::code, format_source};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(name)
}

fn rlx_files(d: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(d)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "rlx"))
        .collect();
    v.sort();
    v
}

fn name(p: &Path) -> String {
    p.file_name().unwrap().to_string_lossy().into_owned()
}

#[test]
fn refused_corpus() {
    let files = rlx_files(&dir("refused"));
    assert!(!files.is_empty());
    let mut covered = BTreeSet::new();
    let mut failures = Vec::new();
    for f in &files {
        let bytes = fs::read(f).unwrap();
        let expected_path = f.with_extension("expected");
        let expected = fs::read_to_string(&expected_path)
            .unwrap_or_else(|_| panic!("{} has no .expected file", name(f)));
        let diags = match check_source(&bytes) {
            Ok(_) => {
                failures.push(format!("{}: accepted, expected a refusal", name(f)));
                continue;
            }
            Err(d) => d,
        };
        for d in &diags {
            covered.insert(d.code);
        }
        let got = diags
            .iter()
            .map(|d| d.render(&name(f), &bytes))
            .collect::<Vec<_>>()
            .join("\n");
        if got.trim_end() != expected.trim_end() {
            failures.push(format!(
                "{}:\n  expected: {}\n  got:      {}",
                name(f),
                expected.trim_end(),
                got
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let missing: Vec<&str> = code::ALL
        .iter()
        .copied()
        .filter(|c| !covered.contains(c))
        .collect();
    assert!(
        missing.is_empty(),
        "no refused example for {missing:?}; add one to tests/refused"
    );
}

#[test]
fn accepted_corpus_round_trips() {
    let files = rlx_files(&dir("accepted"));
    assert!(files.len() >= 7, "{files:?}");
    for f in &files {
        let n = name(f);
        let canonical = fs::read(f).unwrap();
        let canonical_s = String::from_utf8(canonical.clone()).unwrap();
        let checked = check_source(&canonical).unwrap_or_else(|d| {
            panic!(
                "{n}: {}",
                d.iter()
                    .map(|d| d.render(&n, &canonical))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        });
        assert!(!checked.file.items.is_empty());

        // The canonical form is a fixed point of `fmt --ascii`.
        assert_eq!(
            checked.format(Style::Ascii),
            canonical_s,
            "{n}: fmt --ascii"
        );

        // `fmt --unicode` matches the committed variant, which checks, and
        // `fmt --ascii` takes it back to the canonical form.
        let unicode = checked.format(Style::Unicode);
        assert_ne!(unicode, canonical_s, "{n}: --unicode rendered nothing");
        let variant = fs::read_to_string(dir("accepted").join("unicode").join(&n))
            .unwrap_or_else(|_| panic!("{n}: no unicode variant"));
        assert_eq!(unicode, variant, "{n}: fmt --unicode");
        let back = format_source(variant.as_bytes(), Style::Ascii)
            .unwrap_or_else(|d| panic!("{n}: unicode variant refused: {d:?}"));
        assert_eq!(back, canonical_s, "{n}: --unicode then --ascii");
        // And the Unicode form is itself a fixed point of `--unicode`.
        assert_eq!(
            format_source(variant.as_bytes(), Style::Unicode).unwrap(),
            variant,
            "{n}: fmt --unicode on the unicode form"
        );
    }
}

/// The accepted files are the design's examples verbatim, so the examples
/// and the parser cannot drift apart unnoticed.
#[test]
fn accepted_files_are_the_design_examples() {
    let doc = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/design/extension-examples.md"),
    )
    .unwrap();
    let mut found = 0;
    let mut in_block = false;
    let mut block = String::new();
    for line in doc.lines() {
        if line.starts_with("```") {
            if in_block {
                if let Some(first) = block.lines().next()
                    && let Some(rest) = first.strip_prefix("-- share/")
                    && let Some(file) = rest.rsplit('/').next()
                    && file.ends_with(".rlx")
                {
                    let ours = fs::read_to_string(dir("accepted").join(file))
                        .unwrap_or_else(|_| panic!("example {file} is not in tests/accepted"));
                    assert_eq!(ours, block, "{file} differs from the design example");
                    found += 1;
                }
                block.clear();
            }
            in_block = !in_block;
        } else if in_block {
            block.push_str(line);
            block.push('\n');
        }
    }
    assert_eq!(found, 6, "the design has six rlx examples");
}

fn ascii(src: &str) -> String {
    format_source(src.as_bytes(), Style::Ascii).unwrap_or_else(|d| panic!("{src}: {d:?}"))
}

const HEAD: &str = "extension t.t 0.1.0\n";

/// Rewriting a word twin or `−` must never merge tokens: `b∈t` is not
/// `bint`, and `−−b` is not a comment.
#[test]
fn ascii_rewrite_keeps_tokens_apart() {
    let cases = [
        (
            "report r : Time = Σ over b∈t: b\n",
            "report r : Time = sum over b in t: b\n",
        ),
        (
            "report r : Time = Σover b ∈t: b\n",
            "report r : Time = sum over b in t: b\n",
        ),
        ("report r : Time = a −−b\n", "report r : Time = a - -b\n"),
        ("report r : Time = a²·2\n", "report r : Time = a^2*2\n"),
        ("report r : Time = 2√(a)\n", "report r : Time = 2 sqrt(a)\n"),
        ("fn f(x: Time) → Time = x\n", "fn f(x: Time) -> Time = x\n"),
    ];
    for (src, want) in cases {
        let got = ascii(&format!("{HEAD}{src}"));
        assert_eq!(got, format!("{HEAD}{want}"), "{src}");
    }
}

#[test]
fn layout_is_normalized() {
    assert_eq!(
        ascii("extension t.t 0.1.0   \n\nkey a.b : Time\t\n\n\n"),
        "extension t.t 0.1.0\n\nkey a.b : Time\n"
    );
    // Comments keep their Unicode in both directions.
    let src = "extension t.t 0.1.0 -- a ≤ b, x^2\n";
    assert_eq!(ascii(src), src);
    assert_eq!(format_source(src.as_bytes(), Style::Unicode).unwrap(), src);
}

#[test]
fn binary_exit_codes() {
    let bin = env!("CARGO_BIN_EXE_rlx");
    let good = dir("accepted").join("braking.rlx");
    let bad = dir("refused").join("recursion_direct.rlx");
    let unicode = dir("accepted").join("unicode").join("braking.rlx");

    let out = Command::new(bin).arg("check").arg(&good).output().unwrap();
    assert!(out.status.success());
    let out = Command::new(bin).arg("check").arg(&bad).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("error[rlx-recursion]"));

    let out = Command::new(bin)
        .args(["fmt", "--ascii", "--check"])
        .arg(&good)
        .output()
        .unwrap();
    assert!(out.status.success());
    let out = Command::new(bin)
        .args(["fmt", "--ascii", "--check"])
        .arg(&unicode)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let out = Command::new(bin)
        .args(["fmt", "--ascii"])
        .arg(&unicode)
        .output()
        .unwrap();
    assert_eq!(out.stdout, fs::read(&good).unwrap());
    let out = Command::new(bin).arg("fmt").arg(&good).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}
