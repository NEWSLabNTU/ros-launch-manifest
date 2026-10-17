//! `rlx check <file>...` and `rlx fmt --ascii|--unicode [--check] <file>...`.
//!
//! Exit status: 0 when every file checks (and, under `fmt --check`, is
//! already in the requested form), 1 otherwise, 2 for a usage error.
//! Diagnostics go to stderr as `file:line:col: error[code]: message`.
//! `fmt` without `--check` writes the formatted text to stdout and takes one
//! file, so its output is never an ambiguous concatenation.

use rlx::{Style, check_source};
use std::{fs, process::ExitCode};

const USAGE: &str =
    "usage: rlx check <file>...\n       rlx fmt (--ascii | --unicode) [--check] <file>...";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("check") if args.len() > 1 => check(&args[1..]),
        Some("fmt") => fmt(&args[1..]),
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
        match check_source(&bytes) {
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
