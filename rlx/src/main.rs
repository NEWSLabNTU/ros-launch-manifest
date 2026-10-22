//! `rlx check <file>...`, `rlx fmt --ascii|--unicode [--check] <file>...`
//! and, built with the `smt` feature, `rlx prove [--strict] [--timeout-ms=N] <file>...`.
//!
//! Exit status: 0 when every file checks (and, under `fmt --check`, is
//! already in the requested form; under `prove`, no worst case is refuted,
//! nor unproved with `--strict`), 1 otherwise, 2 for a usage error.
//! Diagnostics go to stderr as `file:line:col: error[code]: message`.
//! `fmt` without `--check` writes the formatted text to stdout and takes one
//! file, so its output is never an ambiguous concatenation.

use rlx::{Style, check_source, parse_source};
use std::{fs, process::ExitCode};

const USAGE: &str = "usage: rlx check <file>...\n       rlx fmt (--ascii | --unicode) [--check] \
                     <file>...\n       rlx prove [--strict] [--timeout-ms=N] <file>...";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("check") if args.len() > 1 => check(&args[1..]),
        Some("fmt") => fmt(&args[1..]),
        Some("prove") => prove(&args[1..]),
        Some("-h" | "--help") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        _ => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn read(path: &str) -> Option<Vec<u8>> {
    match fs::read(path) {
        Ok(b) => Some(b),
        Err(e) => {
            eprintln!("{path}: cannot read: {e}");
            None
        }
    }
}

fn report(path: &str, bytes: &[u8], diags: &[rlx::Diagnostic]) {
    for d in diags {
        eprintln!("{}", d.render(path, bytes));
    }
}

fn check(files: &[String]) -> ExitCode {
    let mut ok = true;
    for path in files {
        let Some(bytes) = read(path) else {
            ok = false;
            continue;
        };
        if let Err(diags) = check_source(&bytes) {
            report(path, &bytes, &diags);
            ok = false;
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn fmt(args: &[String]) -> ExitCode {
    let mut style = None;
    let mut check_only = false;
    let mut files = Vec::new();
    for a in args {
        match a.as_str() {
            "--ascii" => style = Some(Style::Ascii),
            "--unicode" => style = Some(Style::Unicode),
            "--check" => check_only = true,
            f if !f.starts_with("--") => files.push(f.to_string()),
            _ => return usage(),
        }
    }
    let Some(style) = style else {
        return usage();
    };
    if files.is_empty() || (!check_only && files.len() != 1) {
        return usage();
    }
    let mut ok = true;
    for path in &files {
        let Some(bytes) = read(path) else {
            ok = false;
            continue;
        };
        match parse_source(&bytes) {
            Err(diags) => {
                report(path, &bytes, &diags);
                ok = false;
            }
            Ok(checked) => {
                let out = checked.format(style);
                if check_only {
                    if out.as_bytes() != bytes.as_slice() {
                        eprintln!("{path}: not in {} form", style_name(style));
                        ok = false;
                    }
                } else {
                    print!("{out}");
                }
            }
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn style_name(s: Style) -> &'static str {
    match s {
        Style::Ascii => "canonical ASCII",
        Style::Unicode => "Unicode",
    }
}

#[cfg(feature = "smt")]
fn prove(args: &[String]) -> ExitCode {
    use rlx::prove::{Options, Verdict, prove_all};
    let strict = args.iter().any(|a| a == "--strict");
    let mut opts = Options::default();
    for a in args.iter().filter(|a| a.starts_with("--timeout-ms=")) {
        match a["--timeout-ms=".len()..].parse() {
            Ok(ms) => opts.timeout_ms = ms,
            Err(_) => return usage(),
        }
    }
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let known = |a: &str| a == "--strict" || a.starts_with("--timeout-ms=");
    if files.is_empty() || args.iter().any(|a| a.starts_with("--") && !known(a)) {
        return usage();
    }
    let mut ok = true;
    for path in files {
        let Some(bytes) = read(path) else {
            ok = false;
            continue;
        };
        let checked = match check_source(&bytes) {
            Ok(c) => c,
            Err(diags) => {
                report(path, &bytes, &diags);
                ok = false;
                continue;
            }
        };
        for claim in prove_all(&checked, &opts) {
            let (label, fails) = match claim.verdict {
                Verdict::Proved => ("info", false),
                Verdict::Refuted(_) => ("error", true),
                Verdict::Unknown(_) => (if strict { "error" } else { "warning" }, strict),
            };
            println!("{path}: {label}: {}", claim.report());
            ok &= !fails;
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(not(feature = "smt"))]
fn prove(_: &[String]) -> ExitCode {
    eprintln!("rlx prove: this rlx was built without the `smt` feature, so it cannot prove");
    ExitCode::from(2)
}
