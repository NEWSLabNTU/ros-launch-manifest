//! Phase 91 E0: can Numbat be embedded as rlx's expression core?
//! Each probe prints PASS/FAIL/NOTE with what it found.

use numbat::module_importer::{BuiltinModuleImporter, ModuleImporter};
use numbat::resolver::{CodeSource, ModulePath};
use numbat::{Context, InterpreterResult, Statement};
use std::path::PathBuf;

/// The restricted prelude: dimensions, units and pure math only.
const PRELUDE: &str = "
use core::scalar
use core::quantities
use core::dimensions
use core::functions
use core::lists
use core::numbers
use math::constants
use units::si
use units::time
";

/// Imports only what PRELUDE names; refuses everything with effects.
struct Restricted {
    inner: BuiltinModuleImporter,
}

const REFUSED: &[&str] = &["core::random", "core::debug"];

impl ModuleImporter for Restricted {
    fn import(&self, path: &ModulePath) -> Option<(String, Option<PathBuf>)> {
        let name = path.to_string();
        if name == "prelude" {
            return Some((PRELUDE.to_string(), None));
        }
        if REFUSED.contains(&name.as_str())
            || name.starts_with("datetime")
            || name.starts_with("plot")
            || name.starts_with("extra")
        {
            return None;
        }
        self.inner.import(path)
    }
    fn list_modules(&self) -> Vec<ModulePath> {
        self.inner.list_modules()
    }
}

fn ctx() -> Context {
    let mut c = Context::new(Restricted { inner: BuiltinModuleImporter::default() });
    c.interpret("use prelude", CodeSource::Internal).expect("prelude loads");
    c
}

/// What an rlx loader would refuse in a parsed file, using only the public
/// typed `Statement`: procedures (print/assert/assert_eq/type) and bodyless
/// function declarations (which bind to Rust FFI by name).
fn refusals(stmts: &[Statement]) -> Vec<String> {
    let mut out = vec![];
    for s in stmts {
        match s {
            Statement::ProcedureCall { .. } => out.push("procedure call".into()),
            Statement::DefineFunction { function_name, body: None, .. } => {
                out.push(format!("bodyless fn '{function_name}' (FFI binding)"))
            }
            _ => {}
        }
    }
    out
}

fn run(c: &mut Context, code: &str) -> Result<(Vec<String>, String), String> {
    match c.interpret(code, CodeSource::Text) {
        Ok((stmts, res)) => {
            let refused = refusals(&stmts);
            let val = match res {
                InterpreterResult::Value(v) => format!("{v}"),
                InterpreterResult::Continue => "-".into(),
            };
            Ok((refused, val))
        }
        Err(e) => Err(format!("{e}")),
    }
}

fn probe(title: &str, code: &str) {
    let mut c = ctx();
    match run(&mut c, code) {
        Ok((refused, val)) => println!("[{title}] ok value={val} refusals={refused:?}"),
        Err(e) => println!("[{title}] error: {}", e.lines().next().unwrap_or("")),
    }
}

const BRAKING: &str = "
fn braking_time(v0: Velocity, a: Acceleration, j: Jerk) -> Time =
    if v0 > v_r then a / j + (v0 - v_r) / a else sqrt(2 v0 / j)
  where v_r = a^2 / (2 j)

fn settle_duration(response: Time, entry_speed: Velocity, decel: Acceleration, jerk: Jerk) -> Time =
    response + braking_time(entry_speed, decel, jerk)
";

// rail.ebd: the format layer slices the table into bands capped at the
// entry speed (`a_ebd.bands(0 m/s, entry_speed)`) and hands them over as a
// list of structs; Numbat has no partial application or lambdas, so `map`
// needs a one-argument function.
const EBD: &str = "
struct Band { lo: Velocity, hi: Velocity, a: Acceleration }

fn band_time(b: Band) -> Time = (b.hi - b.lo) / b.a
fn band_dist(b: Band) -> Length = (b.hi^2 - b.lo^2) / (2 b.a)

fn ebd_settle(cut: Time, build: Time, bands: List<Band>) -> Time =
    cut + build + sum(map(band_time, bands))

fn ebd_distance(cut: Time, build: Time, v: Velocity, bands: List<Band>) -> Length =
    v * (cut + build) + sum(map(band_dist, bands))
";

fn main() {
    println!("== restriction");
    probe("random() unavailable", "random()");
    probe("now() unavailable", "now()");
    probe("FFI escape: declare random", "fn random() -> Scalar\nrandom()");
    probe("print is a procedure", "print(1)");
    probe("assert is a procedure", "assert(1 == 1)");
    probe("string functions gone", "str_length(\"abc\")");

    println!("\n== recursion");
    probe(
        "recursive fn accepted?",
        "fn f(n: Scalar) -> Scalar = if n <= 0 then 0 else 1 + f(n - 1)\nf(5)",
    );
    probe(
        "mutual recursion accepted?",
        "fn ping(n: Scalar) -> Scalar = if n <= 0 then 0 else pong(n - 1)\nfn pong(n: Scalar) -> Scalar = ping(n)\nping(3)",
    );

    println!("\n== units checked in every branch");
    probe(
        "unit error in untaken branch",
        "fn k(v: Velocity) -> Time = if v > 0 m/s then 1 s else 1 m\nk(1 m/s)",
    );
    probe(
        "exploration's unit bug sqrt(2 v0 / a)",
        "fn bt(v0: Velocity, a: Acceleration) -> Time = sqrt(2 v0 / a)\nbt(1 m/s, 1 m/s^2)",
    );
    probe("km/h converts", "30 km/h -> m/s");

    println!("\n== vehicle.braking (island hpc_loss: 4421.33 ms expected)");
    let mut c = ctx();
    run(&mut c, BRAKING).expect("braking loads");
    let (_, v) = run(&mut c, "settle_duration(256 ms, 30 km/h, 2.5 m/s^2, 1.5 m/s^3) -> ms").unwrap();
    println!("[settle] {v}");
    let (_, v) = run(&mut c, "settle_duration(0 ms, 1 km/h, 2.5 m/s^2, 1.5 m/s^3) -> ms").unwrap();
    println!("[settle, low speed branch] {v}");

    println!("\n== rail.ebd (64.35 s and 1529.75 m expected)");
    let mut c = ctx();
    match run(&mut c, EBD) {
        Ok(_) => {
            for q in [
                "let bands = [Band { lo: 0 km/h, hi: 100 km/h, a: 0.75 m/s^2 }, Band { lo: 100 km/h, hi: 160 km/h, a: 0.70 m/s^2 }]",
                "ebd_settle(1.0 s, 2.5 s, bands) -> s",
                "ebd_distance(1.0 s, 2.5 s, 160 km/h, bands) -> m",
            ] {
                match run(&mut c, q) {
                    Ok((_, v)) => println!("[ebd] {v}"),
                    Err(e) => println!("[ebd] error: {}", e.lines().next().unwrap_or("")),
                }
            }
        }
        Err(e) => println!("[ebd load] error: {e}"),
    }

    println!("\n== characters");
    probe("Cyrillic a in identifier", "let v\u{0430} = 1 m\nv\u{0430}");
    probe("Greek omicron identifier", "let \u{03bf} = 1 m\n\u{03bf}");
    probe("bidi control in comment", "1 m # \u{202e}evil\n");
    probe("zero-width in identifier", "let a\u{200b}b = 1\na\u{200b}b");
    probe("non-NFC (e + combining acute)", "let e\u{0301}x = 1\ne\u{0301}x");

    println!("\n== AST access for trace and Z3");
    let mut c = ctx();
    let (stmts, _) = c.interpret(BRAKING, CodeSource::Text).unwrap();
    for s in &stmts {
        if let Statement::DefineFunction { function_name, local_variables, body, .. } = s {
            println!(
                "[fn {function_name}] locals={} body_present={} (body type is numbat-private: not nameable, no visitor)",
                local_variables.len(),
                body.is_some()
            );
        }
    }
}
