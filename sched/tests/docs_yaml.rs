//! Every fenced ```yaml block in the hand-written docs is fed to the parser
//! that owns it.
//!
//! The docs are prose, and prose drifts: issue 0038 found `max_drop_rate`,
//! `max_latency_ms` and a top-level `sub:` scope interface still being taught
//! years after each became a parse error, because nothing ever fed an example
//! from the spec to the parser it specifies. `format-reference.md` is
//! generated and cannot drift; everything else in `docs/` is checked here.
//!
//! The docs describe two YAML languages, so the test dispatches on the block
//! rather than skipping one of them: a block with a top-level `target:` is a
//! **platform file** and goes to `parse_platform_file_yaml`; everything else
//! is a **contract** and goes to `parse_manifest_str`. `target:` is not a
//! manifest key and `nodes:`/`topics:` are not platform-file keys, so the two
//! sets cannot be confused. This test lives in `sched/` for exactly that
//! reason — it is the crate that can reach both parsers.
//!
//! # Markers
//!
//! A block that is NOT a whole manifest — a fragment, a platform file, a
//! launch XML snippet mislabelled `yaml`, or an example written to fail —
//! carries an HTML comment on the line directly above its opening fence
//! (blank lines between are allowed). HTML comments render as nothing in
//! every Markdown viewer, including marp, so the marker is invisible to a
//! reader:
//!
//! ```text
//! <!-- yaml-check: skip — a fragment, not a whole manifest -->
//! <!-- yaml-check: expect-error — `chains:` is a parse error since phase 68 -->
//! ```
//!
//! - `skip` means "do not parse this": the reason after the em dash is
//!   REQUIRED, so a block cannot be exempted silently.
//! - `expect-error` means "this must NOT parse": the test fails if it does,
//!   which is what keeps an example of a removed spelling honest.
//!
//! Everything else must parse. Adding a marker is a deliberate act; adding a
//! broken example is not possible without one.

use ros_launch_manifest_sched::parse_platform_file_yaml;
use ros_launch_manifest_types::parse_manifest_str;
use std::path::{Path, PathBuf};

/// Parse a documented block with the parser that owns its language.
fn parse_block(body: &str) -> Result<&'static str, String> {
    let is_platform_file = body
        .lines()
        .any(|l| l.starts_with("target:") || l.starts_with("mapper:"));
    if is_platform_file {
        parse_platform_file_yaml(body)
            .map(|_| "platform file")
            .map_err(|e| e.to_string())
    } else {
        parse_manifest_str(body)
            .map(|_| "contract")
            .map_err(|e| e.to_string())
    }
}

/// One fenced block, with where it came from.
struct Block {
    file: String,
    /// 1-based line of the opening fence.
    line: usize,
    body: String,
    marker: Marker,
}

#[derive(PartialEq)]
enum Marker {
    MustParse,
    MustFail(String),
    Skip(String),
}

fn docs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("types/ has a parent")
        .to_path_buf()
}

/// Every `.md` under the repo root and `docs/`, sorted.
fn doc_files() -> Vec<PathBuf> {
    let root = docs_dir();
    let mut out = vec![root.join("README.md")];
    let mut docs: Vec<PathBuf> = std::fs::read_dir(root.join("docs"))
        .expect("docs/ must exist")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    docs.sort();
    out.extend(docs);
    out
}

/// Read the marker comment, if any, that precedes `idx` in `lines`.
fn marker_before(lines: &[&str], idx: usize) -> Marker {
    let mut i = idx;
    while i > 0 {
        i -= 1;
        let l = lines[i].trim();
        if l.is_empty() {
            continue;
        }
        let Some(rest) = l
            .strip_prefix("<!--")
            .and_then(|r| r.strip_suffix("-->"))
            .map(str::trim)
            .and_then(|r| r.strip_prefix("yaml-check:"))
            .map(str::trim)
        else {
            return Marker::MustParse;
        };
        // The kind is the first word; the rest is the reason, with any
        // leading dash stripped. Splitting on the dash itself would cut
        // `expect-error` in half.
        let (kind, reason) = match rest.split_once(char::is_whitespace) {
            Some((k, r)) => (k.trim(), r.trim().trim_start_matches(['—', '-']).trim()),
            None => (rest, ""),
        };
        return match kind {
            "skip" => Marker::Skip(reason.to_string()),
            "expect-error" => Marker::MustFail(reason.to_string()),
            other => panic!("unknown yaml-check marker `{other}` (skip | expect-error)"),
        };
    }
    Marker::MustParse
}

fn blocks() -> Vec<Block> {
    let mut out = Vec::new();
    for path in doc_files() {
        let text = std::fs::read_to_string(&path).expect("doc must be readable");
        let name = path
            .strip_prefix(docs_dir())
            .unwrap_or(&path)
            .display()
            .to_string();
        let lines: Vec<&str> = text.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            // A fence may be INDENTED -- inside a list item, or under a
            // numbered step. The first version of this scanner matched on
            // `trim_end()` alone, so every indented block was invisible to it
            // and three existed, two of them teaching a spelling the parser
            // rejects. A guard that cannot see a class of example is worse
            // than no guard, because it reads as coverage.
            let indent = lines[i].len() - lines[i].trim_start().len();
            let info = lines[i].trim();
            // Only a fence opening a yaml block; the language is the first
            // word of the info string.
            let is_yaml_fence = info
                .strip_prefix("```")
                .map(|rest| rest.split_whitespace().next().unwrap_or("") == "yaml")
                .unwrap_or(false);
            if !is_yaml_fence {
                i += 1;
                continue;
            }
            let start = i;
            let mut body = String::new();
            i += 1;
            while i < lines.len() && !lines[i].trim_start().starts_with("```") {
                // Dedent by the fence's own indentation. Feeding the parser
                // the indented text would make a mapping look like the value
                // of something, so an indented block would fail for a reason
                // that has nothing to do with what it teaches.
                let line = lines[i];
                let cut = line
                    .char_indices()
                    .take_while(|(n, c)| *c == ' ' && *n < indent)
                    .count();
                body.push_str(&line[cut..]);
                body.push('\n');
                i += 1;
            }
            i += 1; // closing fence
            out.push(Block {
                file: name.clone(),
                line: start + 1,
                body,
                marker: marker_before(&lines, start),
            });
        }
    }
    out
}

#[test]
fn every_documented_yaml_block_parses() {
    let blocks = blocks();
    assert!(
        blocks.len() >= 30,
        "expected the docs to carry yaml examples; found {}",
        blocks.len()
    );

    let mut failures = Vec::new();
    let (mut parsed, mut failed_as_told, mut skipped) = (0, 0, 0);

    for b in &blocks {
        match &b.marker {
            Marker::Skip(reason) => {
                if reason.is_empty() {
                    failures.push(format!(
                        "{}:{}: `yaml-check: skip` without a reason — say why, \
                         so a block cannot be exempted silently",
                        b.file, b.line
                    ));
                }
                skipped += 1;
            }
            Marker::MustParse => match parse_block(&b.body) {
                Ok(_) => parsed += 1,
                Err(e) => failures.push(format!(
                    "{}:{}: example does not parse: {e}\n--- block ---\n{}-------------",
                    b.file, b.line, b.body
                )),
            },
            Marker::MustFail(reason) => match parse_block(&b.body) {
                Err(_) => failed_as_told += 1,
                Ok(_) => failures.push(format!(
                    "{}:{}: marked `expect-error` ({reason}) but it parses — \
                     drop the marker or fix the example",
                    b.file, b.line
                )),
            },
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} documented yaml blocks are wrong:\n\n{}",
        failures.len(),
        blocks.len(),
        failures.join("\n\n")
    );

    eprintln!(
        "docs yaml: {parsed} parsed, {failed_as_told} expected-error, {skipped} skipped, \
         {} blocks total",
        blocks.len()
    );
}
