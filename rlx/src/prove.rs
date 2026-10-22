//! Worst-case proofs (phase 91 E5): a `derive` or `check` that states
//! `worst_case: increasing in x` is proved, refuted or left unknown by Z3.
//!
//! # What is proved
//!
//! The checker evaluates a derived bound at the contract's declared value
//! (an entry speed, a minimum rate) and calls it the worst case. The
//! direction is what makes that true, so each named key is a claim:
//!
//! - a `derive` is a bound, larger is worse: `increasing in x` means
//!   `x1 < x2` never gives `f(x1) > f(x2)`; `decreasing in x` means it never
//!   gives `f(x1) < f(x2)`. Non-strict: a flat stretch is not a violation.
//! - a `check` is a predicate, failing is worse: `increasing in x` means a
//!   check that passes at `x2` passes at every `x1 < x2`; `decreasing in x`
//!   the mirror.
//!
//! Every other key and input is free and shared by the two evaluations, so
//! the claim is about every value the contract could write, not only the one
//! it does. Each evaluation must be inside the item's `domain:` and DEFINED:
//! the evaluator refuses a division by zero, the square root of a negative
//! and a `bands` slice outside its table, so a point where it would refuse is
//! not a counterexample. Facts the declarations state are assumed too: a
//! `magnitude` parameter is not negative, a table's keys increase strictly.
//! Nothing else is: a speed with no domain may be negative, and a claim that
//! needs it positive is refuted, with the domain that would make it hold.
//!
//! # Encoding
//!
//! Quantities are SI reals (a literal's unit is applied exactly as the
//! evaluator applies it); `if`, `min`, `max` and `abs` are `ite`; `sqrt(x)`
//! is a fresh `s >= 0` with `s * s = x` wherever `x >= 0`; an integer power
//! is a product. A table is the number of entries the contract writes (or 1
//! to 3 when no contract is given), each key and value a variable; a claim
//! over a table varies every value, keys held, and is violated when raising
//! values pointwise moves the result the wrong way. `bands` and `sum over`
//! unroll over the entries. A power with a non-integer or non-constant
//! exponent is beyond the encoding: unknown, never a guess.
//!
//! # Verdicts
//!
//! - Proved: Z3 reports the violation unsatisfiable.
//! - Refuted: Z3 found one, and the EVALUATOR reproduces it at the two
//!   points (so the counterexample's numbers are the evaluator's, and an
//!   encoding defect cannot report a false refutation). Where some set of
//!   at most two simple constraints (`x > 0`, `a <= b` between keys of one
//!   dimension) proves the claim and still admits the contract's values,
//!   that set is offered as the domain that would make it hold.
//! - Unknown: a timeout, a construct beyond the encoding, or a model the
//!   evaluator does not reproduce.

use crate::{
    Checked,
    ast::*,
    dim::Dim,
    display,
    eval::{self, Inputs, Value},
    typeck::{Env, ItemRef, KeyKind, KeyTy},
    units,
};
use std::collections::{BTreeMap, HashMap};
use z3::{
    Config, Context, Params, SatResult, Solver,
    ast::{Ast, Bool, Real},
};

/// How a claim came out.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Proved,
    Refuted(Box<Counterexample>),
    Unknown(String),
}

/// Two points that break a claim, as the evaluator computes them.
#[derive(Debug, Clone, PartialEq)]
pub struct Counterexample {
    /// The other keys the item reads, `(name, value)`, shared by both points.
    pub shared: Vec<(String, String)>,
    /// The claim's key at the lower and the higher point.
    pub lo: String,
    pub hi: String,
    /// The item at each point: a derived value, or `PASS`/`FAIL`.
    pub at_lo: String,
    pub at_hi: String,
}

/// One `increasing in x` or `decreasing in x`.
#[derive(Debug, Clone, PartialEq)]
pub struct Claim {
    pub item: ItemRef,
    /// The key as the `worst_case` clause names it.
    pub var: String,
    pub direction: Direction,
    pub verdict: Verdict,
    /// Refuted only: constraints that prove it, in rlx syntax, to add to the
    /// item's `domain:`.
    pub holds_on: Option<Vec<String>>,
    /// The tables the proof assumed, described (`a_ebd's 2 keys as
    /// written`, `a 1- to 3-entry a_ebd`).
    pub tables: Vec<String>,
}

impl Claim {
    /// `increasing in entry_speed`.
    pub fn statement(&self) -> String {
        format!("{} in {}", direction_word(self.direction), self.var)
    }

    /// `settle.duration`, `check energy_reserve`.
    pub fn subject(&self) -> String {
        match &self.item {
            ItemRef::Derive(t) => t.clone(),
            ItemRef::Check(n) => format!("check {n}"),
            ItemRef::Report(n) => format!("report {n}"),
        }
    }

    /// ` (for a_ebd's 2 keys as written)`, or nothing when no table was
    /// involved.
    fn table_note(&self) -> String {
        if self.tables.is_empty() {
            return String::new();
        }
        format!(" (for {})", self.tables.join(", "))
    }

    /// The claim's outcome as a consumer prints it: one line when proved or
    /// unknown; a refutation adds the counterexample and, when found, the
    /// domain that would make it hold.
    pub fn report(&self) -> String {
        match &self.verdict {
            Verdict::Proved => format!(
                "{}: worst case proved {}{}",
                self.subject(),
                self.statement(),
                self.table_note()
            ),
            Verdict::Unknown(why) => format!(
                "{}: worst case {} not proved{}: {why}",
                self.subject(),
                self.statement(),
                self.table_note()
            ),
            Verdict::Refuted(c) => {
                let mut s = format!(
                    "{} is not {}{}\n  counterexample (Z3): {}\n    {} {} -> {};  {} -> {}",
                    self.subject(),
                    self.statement(),
                    self.table_note(),
                    if c.shared.is_empty() {
                        "no other inputs".to_string()
                    } else {
                        c.shared
                            .iter()
                            .map(|(k, v)| format!("{k} = {v}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    },
                    self.var.rsplit('.').next().unwrap_or(&self.var),
                    c.lo,
                    c.at_lo,
                    c.hi,
                    c.at_hi
                );
                if let Some(h) = &self.holds_on {
                    s.push_str(&format!("\n  help: it holds on `domain: {}`", h.join(", ")));
                }
                s
            }
        }
    }
}

pub fn direction_word(d: Direction) -> &'static str {
    match d {
        Direction::Increasing => "increasing",
        Direction::Decreasing => "decreasing",
    }
}

#[derive(Debug, Clone)]
pub struct Options {
    /// Per Z3 query.
    pub timeout_ms: u32,
    /// A table's keys as the contract writes them, in SI, by full path:
    /// the proof holds them and varies the values. A table key not listed
    /// is proved for 1, 2 and 3 entries with symbolic keys, which Z3 may not
    /// decide in time (rail's distance check at three entries does not).
    pub table_keys: BTreeMap<String, Vec<f64>>,
    /// The contract's values, by full path: a suggested domain must admit
    /// them. Keys missing here constrain nothing.
    pub point: Inputs,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            timeout_ms: 5000,
            table_keys: BTreeMap::new(),
            point: Inputs::new(),
        }
    }
}

/// Every claim of every `derive` and `check` in the file, in file order.
pub fn prove_all(checked: &Checked, opts: &Options) -> Vec<Claim> {
    let mut out = Vec::new();
    for item in &checked.file().items {
        let r = match item {
            Item::Derive(d) if !d.body.worst_case.is_empty() => ItemRef::Derive(d.target.dotted()),
            Item::Check(c) if !c.body.worst_case.is_empty() => ItemRef::Check(c.name.name.clone()),
            _ => continue,
        };
        out.extend(prove(checked, &r, opts));
    }
    out
}

/// The claims of one item. Empty when it states no `worst_case`.
pub fn prove(checked: &Checked, item: &ItemRef, opts: &Options) -> Vec<Claim> {
    let Some(body) = body_of(checked, item) else {
        return Vec::new();
    };
    let env = &checked.env;
    let reads = env.reads(item).unwrap_or_default();
    let tables: Vec<(usize, String)> = reads
        .iter()
        .filter(|n| matches!(n.ty, KeyTy::Table { .. }))
        .map(|n| (index_of(env, &n.path), n.path.clone()))
        .collect();
    // Each table shape to prove for: the contract's keys, else 1 to 3
    // symbolic entries.
    let size_sets: Vec<Vec<(usize, Shape)>> = {
        let mut sets: Vec<Vec<(usize, Shape)>> = vec![Vec::new()];
        for (idx, path) in &tables {
            let choices: Vec<Shape> = match opts.table_keys.get(path) {
                Some(keys) if !keys.is_empty() => vec![Shape::Keys(keys.clone())],
                _ => (1..=3).map(Shape::Size).collect(),
            };
            sets = sets
                .into_iter()
                .flat_map(|s| {
                    choices.iter().map(move |c| {
                        let mut s = s.clone();
                        s.push((*idx, c.clone()));
                        s
                    })
                })
                .collect();
        }
        sets
    };
    let describe = |shapes: &[(usize, Shape)], all: bool| -> Vec<String> {
        shapes
            .iter()
            .map(|(i, shape)| {
                let last = &env.keys[*i].last;
                match shape {
                    Shape::Keys(k) => format!("{last}'s {} keys as written", k.len()),
                    Shape::Size(_) if all => format!("a 1- to 3-entry {last}"),
                    Shape::Size(n) => format!("a {n}-entry {last}"),
                }
            })
            .collect()
    };
    let mut claims = Vec::new();
    for group in &body.worst_case {
        for var in &group.vars {
            let name = var.dotted();
            let idx = key_index(env, &name, var.segments.len() == 1)
                .expect("worst_case names are resolved at load");
            let mut claim = Claim {
                item: item.clone(),
                var: name,
                direction: group.direction,
                verdict: Verdict::Proved,
                holds_on: None,
                tables: Vec::new(),
            };
            claim.tables = size_sets.last().map_or(Vec::new(), |s| describe(s, true));
            for sizes in &size_sets {
                let p = Problem {
                    checked,
                    item,
                    body,
                    var: idx,
                    direction: group.direction,
                    sizes,
                    opts,
                };
                let (verdict, holds_on) = p.run();
                if verdict != Verdict::Proved {
                    claim.tables = describe(sizes, false);
                    claim.verdict = verdict;
                    claim.holds_on = holds_on;
                    break;
                }
            }
            claims.push(claim);
        }
    }
    claims
}

fn body_of<'a>(checked: &'a Checked, item: &ItemRef) -> Option<&'a Body> {
    checked.file().items.iter().find_map(|i| match (i, item) {
        (Item::Derive(d), ItemRef::Derive(t)) if &d.target.dotted() == t => Some(&d.body),
        (Item::Check(c), ItemRef::Check(n)) if &c.name.name == n => Some(&c.body),
        _ => None,
    })
}

fn index_of(env: &Env, path: &str) -> usize {
    env.keys
        .iter()
        .position(|k| k.path == path)
        .expect("a read key is declared")
}

/// The evaluator's lookup: the full path, or the last segment for a
/// one-segment name.
fn key_index(env: &Env, path: &str, single: bool) -> Option<usize> {
    env.keys.iter().position(|k| k.path == path).or_else(|| {
        if single {
            env.keys.iter().position(|k| k.last == path)
        } else {
            None
        }
    })
}

fn key_dim(ty: &KeyTy) -> Dim {
    match ty {
        KeyTy::Value(d) | KeyTy::Param { dim: d, .. } => *d,
        KeyTy::Table { to, .. } => *to,
    }
}

/// One claim at one set of table sizes.
struct Problem<'a> {
    checked: &'a Checked<'a>,
    item: &'a ItemRef,
    body: &'a Body,
    var: usize,
    direction: Direction,
    sizes: &'a [(usize, Shape)],
    opts: &'a Options,
}

/// How a table is modelled: its keys as written, or this many symbolic
/// entries.
#[derive(Debug, Clone, PartialEq)]
enum Shape {
    Keys(Vec<f64>),
    Size(usize),
}

/// A key's variables in one copy.
#[derive(Clone)]
enum KeyVar<'c> {
    R(Real<'c>),
    Table {
        keys: Vec<Real<'c>>,
        vals: Vec<Real<'c>>,
    },
}

#[derive(Clone)]
struct BandT<'c> {
    lo: Real<'c>,
    hi: Real<'c>,
    value: Real<'c>,
    on: Bool<'c>,
}

#[derive(Clone)]
enum T<'c> {
    R(Real<'c>),
    B(Bool<'c>),
    Table(usize),
    Bands(Vec<BandT<'c>>),
    Band(BandT<'c>),
}

type E<T> = Result<T, String>;

struct Enc<'c, 'a> {
    ctx: &'c Context,
    env: &'a Env,
    fns: HashMap<&'a str, &'a FnDecl>,
    keys: Vec<KeyVar<'c>>,
    /// Definitions of auxiliary variables (`sqrt`), asserted globally.
    axioms: &'a mut Vec<Bool<'c>>,
    /// Each `sqrt` as (result, argument), for the monotonicity lemmas.
    roots: &'a mut Vec<(Real<'c>, Real<'c>)>,
}

#[derive(Default, Clone)]
struct Scope<'c> {
    binds: Vec<(String, T<'c>)>,
    sums: Vec<(String, T<'c>)>,
    params: Vec<(String, T<'c>)>,
}

impl<'c> Scope<'c> {
    fn find(&self, name: &str) -> Option<T<'c>> {
        let look = |v: &Vec<(String, T<'c>)>| {
            v.iter()
                .rev()
                .find(|(n, _)| n == name)
                .map(|(_, x)| x.clone())
        };
        look(&self.binds)
            .or_else(|| look(&self.sums))
            .or_else(|| look(&self.params))
    }
}

/// An f64 as an exact Z3 rational, through its shortest decimal spelling.
fn rational<'c>(ctx: &'c Context, x: f64) -> E<Real<'c>> {
    if !x.is_finite() {
        return Err(format!("{x} is not a finite number"));
    }
    let s = format!("{:e}", x.abs());
    let (mant, exp) = s.split_once('e').expect("`{:e}` has an exponent");
    let exp: i32 = exp.parse().expect("an integer exponent");
    let (int, frac) = mant.split_once('.').unwrap_or((mant, ""));
    let digits = format!("{int}{frac}");
    let e = exp - frac.len() as i32;
    let (num, den) = if e >= 0 {
        (
            format!("{digits}{}", "0".repeat(e as usize)),
            "1".to_string(),
        )
    } else {
        (digits, format!("1{}", "0".repeat((-e) as usize)))
    };
    let num = if x < 0.0 { format!("-{num}") } else { num };
    Real::from_real_str(ctx, &num, &den).ok_or_else(|| format!("cannot encode {x}"))
}

fn and<'c>(ctx: &'c Context, xs: &[&Bool<'c>]) -> Bool<'c> {
    if xs.is_empty() {
        Bool::from_bool(ctx, true)
    } else {
        Bool::and(ctx, xs)
    }
}

impl<'c, 'a> Enc<'c, 'a> {
    fn t(&self) -> Bool<'c> {
        Bool::from_bool(self.ctx, true)
    }

    fn r(t: T<'c>) -> Real<'c> {
        match t {
            T::R(r) => r,
            _ => unreachable!("type-checked to a quantity"),
        }
    }

    fn b(t: T<'c>) -> Bool<'c> {
        match t {
            T::B(b) => b,
            _ => unreachable!("type-checked to a boolean"),
        }
    }

    fn key(&self, idx: usize) -> T<'c> {
        match &self.keys[idx] {
            KeyVar::R(r) => T::R(r.clone()),
            KeyVar::Table { .. } => T::Table(idx),
        }
    }

    /// The value and the condition under which the evaluator computes it
    /// without refusing.
    fn enc(&mut self, e: &Expr, scope: &mut Scope<'c>) -> E<(T<'c>, Bool<'c>)> {
        let ctx = self.ctx;
        Ok(match &e.kind {
            ExprKind::Number { value, .. } => (T::R(rational(ctx, *value)?), self.t()),
            ExprKind::Quantity { value, unit, .. } => {
                let (_, scale) = units::product(unit).expect("units are checked at parse");
                (T::R(rational(ctx, value * scale)?), self.t())
            }
            ExprKind::Path(p) => (self.path(p, scope)?, self.t()),
            ExprKind::Call { callee, args } => self.call(callee, args, scope)?,
            ExprKind::Unary { op, operand } => {
                let (v, d) = self.enc(operand, scope)?;
                match op {
                    UnOp::Neg => (T::R(-Self::r(v)), d),
                    UnOp::Not => (T::B(Self::b(v).not()), d),
                }
            }
            ExprKind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs, scope)?,
            ExprKind::If { cond, then, els } => {
                let (c, cd) = self.enc(cond, scope)?;
                let c = Self::b(c);
                let (t, td) = self.enc(then, scope)?;
                let (f, fd) = self.enc(els, scope)?;
                let v = match (t, f) {
                    (T::R(a), T::R(b)) => T::R(c.ite(&a, &b)),
                    (T::B(a), T::B(b)) => T::B(c.ite(&a, &b)),
                    _ => return Err("an `if` over tables or bands".into()),
                };
                (v, and(ctx, &[&cd, &c.ite(&td, &fd)]))
            }
            ExprKind::Sum { var, over, body } => {
                let (list, ld) = self.enc(over, scope)?;
                let T::Bands(bands) = list else {
                    unreachable!("type-checked to a list of bands")
                };
                let mut total = rational(ctx, 0.0)?;
                let mut defs = vec![ld];
                for band in bands {
                    scope.sums.push((var.name.clone(), T::Band(band.clone())));
                    let term = self.enc(body, scope);
                    scope.sums.pop();
                    let (t, td) = term?;
                    total += band.on.ite(&Self::r(t), &rational(ctx, 0.0)?);
                    defs.push(band.on.implies(&td));
                }
                let refs: Vec<&Bool> = defs.iter().collect();
                (T::R(total), and(ctx, &refs))
            }
        })
    }

    fn path(&self, p: &Path, scope: &Scope<'c>) -> E<T<'c>> {
        let segs = &p.segments;
        let (mut v, rest) = if let Some(v) = scope.find(&segs[0].name) {
            (v, 1)
        } else {
            let mut found = None;
            for k in (1..=segs.len()).rev() {
                let prefix = segs[..k]
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join(".");
                if let Some(idx) = key_index(self.env, &prefix, k == 1) {
                    found = Some((idx, k));
                    break;
                }
            }
            let (idx, k) = found.expect("names are resolved at load");
            (self.key(idx), k)
        };
        for seg in &segs[rest..] {
            v = match (v, seg.name.as_str()) {
                (T::Band(b), "lo") => T::R(b.lo),
                (T::Band(b), "hi") => T::R(b.hi),
                (T::Band(b), "value") => T::R(b.value),
                (_, f) => unreachable!("field `{f}` passed the checker"),
            };
        }
        Ok(v)
    }

    fn call(
        &mut self,
        callee: &Path,
        args: &[Expr],
        scope: &mut Scope<'c>,
    ) -> E<(T<'c>, Bool<'c>)> {
        let ctx = self.ctx;
        if callee.segments.len() > 1 {
            return self.bands(callee, args, scope);
        }
        let name = callee.segments[0].name.as_str();
        match name {
            "min" | "max" => {
                let mut best: Option<Real<'c>> = None;
                let mut defs = Vec::new();
                for a in args {
                    let (x, d) = self.enc(a, scope)?;
                    let x = Self::r(x);
                    defs.push(d);
                    best = Some(match best {
                        None => x,
                        Some(b) => {
                            let take = if name == "min" { x.lt(&b) } else { x.gt(&b) };
                            take.ite(&x, &b)
                        }
                    });
                }
                let refs: Vec<&Bool> = defs.iter().collect();
                Ok((T::R(best.expect("two or more arguments")), and(ctx, &refs)))
            }
            "abs" => {
                let (x, d) = self.enc(&args[0], scope)?;
                let x = Self::r(x);
                let zero = rational(ctx, 0.0)?;
                Ok((T::R(x.lt(&zero).ite(&-x.clone(), &x)), d))
            }
            "sqrt" => {
                let (x, d) = self.enc(&args[0], scope)?;
                let x = Self::r(x);
                let zero = rational(ctx, 0.0)?;
                let s = Real::fresh_const(ctx, "sqrt");
                let nonneg = x.ge(&zero);
                self.axioms.push(
                    nonneg.implies(&and(ctx, &[&s.ge(&zero), &(s.clone() * s.clone())._eq(&x)])),
                );
                self.roots.push((s.clone(), x));
                Ok((T::R(s), and(ctx, &[&d, &nonneg])))
            }
            _ => {
                let f = *self.fns.get(name).expect("functions are resolved at load");
                let mut inner = Scope::default();
                let mut defs = Vec::new();
                for (p, a) in f.params.iter().zip(args) {
                    let (v, d) = self.enc(a, scope)?;
                    defs.push(d);
                    inner.params.push((p.name.name.clone(), v));
                }
                defs.push(self.bindings(&f.bindings, &mut inner)?);
                let (v, d) = self.enc(&f.value, &mut inner)?;
                defs.push(d);
                let refs: Vec<&Bool> = defs.iter().collect();
                Ok((v, and(ctx, &refs)))
            }
        }
    }

    /// `t.bands(lo, hi)`, unrolled over the table's entries.
    fn bands(
        &mut self,
        callee: &Path,
        args: &[Expr],
        scope: &mut Scope<'c>,
    ) -> E<(T<'c>, Bool<'c>)> {
        let ctx = self.ctx;
        let recv = Path {
            segments: callee.segments[..callee.segments.len() - 1].to_vec(),
            span: callee.span,
        };
        let T::Table(idx) = self.path(&recv, scope)? else {
            unreachable!("a table, checked at load")
        };
        let KeyVar::Table { keys, vals } = self.keys[idx].clone() else {
            unreachable!("a table key has table variables")
        };
        let (lo, ld) = self.enc(&args[0], scope)?;
        let (hi, hd) = self.enc(&args[1], scope)?;
        let (lo, hi) = (Self::r(lo), Self::r(hi));
        let mut out = Vec::new();
        for i in 0..keys.len() {
            let l = keys[i].gt(&lo).ite(&keys[i], &lo);
            let h = match keys.get(i + 1) {
                Some(next) => next.lt(&hi).ite(next, &hi),
                None => hi.clone(),
            };
            out.push(BandT {
                on: l.lt(&h),
                lo: l,
                hi: h,
                value: vals[i].clone(),
            });
        }
        let def = and(ctx, &[&ld, &hd, &lo.le(&hi), &lo.ge(&keys[0])]);
        Ok((T::Bands(out), def))
    }

    fn binary(
        &mut self,
        op: BinOp,
        lhs: &Expr,
        rhs: &Expr,
        scope: &mut Scope<'c>,
    ) -> E<(T<'c>, Bool<'c>)> {
        let ctx = self.ctx;
        let (l, ld) = self.enc(lhs, scope)?;
        if op == BinOp::Pow {
            let n = const_int(rhs).ok_or_else(|| {
                "a power whose exponent is not a constant integer is beyond the encoding"
                    .to_string()
            })?;
            let x = Self::r(l);
            let mut p = rational(ctx, 1.0)?;
            for _ in 0..n.unsigned_abs() {
                p *= x.clone();
            }
            return Ok(if n < 0 {
                let zero = rational(ctx, 0.0)?;
                (
                    T::R(rational(ctx, 1.0)? / p),
                    and(ctx, &[&ld, &x._eq(&zero).not()]),
                )
            } else {
                (T::R(p), ld)
            });
        }
        let (r, rd) = self.enc(rhs, scope)?;
        Ok(match op {
            BinOp::And => {
                let (a, b) = (Self::b(l), Self::b(r));
                let def = and(ctx, &[&ld, &a.implies(&rd)]);
                (T::B(and(ctx, &[&a, &b])), def)
            }
            BinOp::Or => {
                let (a, b) = (Self::b(l), Self::b(r));
                let def = and(ctx, &[&ld, &a.not().implies(&rd)]);
                (T::B(Bool::or(ctx, &[&a, &b])), def)
            }
            BinOp::Cmp(c) => {
                let def = and(ctx, &[&ld, &rd]);
                let v = match (l, r) {
                    (T::R(x), T::R(y)) => match c {
                        CmpOp::Lt => x.lt(&y),
                        CmpOp::Le => x.le(&y),
                        CmpOp::Gt => x.gt(&y),
                        CmpOp::Ge => x.ge(&y),
                        CmpOp::Eq => x._eq(&y),
                        CmpOp::Ne => x._eq(&y).not(),
                    },
                    (T::B(x), T::B(y)) => match c {
                        CmpOp::Eq => x._eq(&y),
                        _ => x._eq(&y).not(),
                    },
                    _ => unreachable!("comparison kinds are checked at load"),
                };
                (T::B(v), def)
            }
            _ => {
                let (x, y) = (Self::r(l), Self::r(r));
                let def = and(ctx, &[&ld, &rd]);
                match op {
                    BinOp::Add => (T::R(x + y), def),
                    BinOp::Sub => (T::R(x - y), def),
                    BinOp::Mul | BinOp::ImplicitMul => (T::R(x * y), def),
                    BinOp::Div => {
                        let zero = rational(ctx, 0.0)?;
                        let nz = y._eq(&zero).not();
                        // A fresh quotient, `q * y = x` wherever `y != 0`:
                        // a polynomial constraint, which nlsat decides where
                        // Z3's `/` left rail's distance sum incomplete.
                        let q = Real::fresh_const(ctx, "quot");
                        self.axioms.push(nz.implies(&(q.clone() * y)._eq(&x)));
                        (T::R(q), and(ctx, &[&def, &nz]))
                    }
                    _ => unreachable!("handled above"),
                }
            }
        })
    }

    /// `where` bindings in order; the evaluator computes every one, used or
    /// not, so each must be defined.
    fn bindings(&mut self, bindings: &[Binding], scope: &mut Scope<'c>) -> E<Bool<'c>> {
        let mut defs = Vec::new();
        for b in bindings {
            let (v, d) = self.enc(&b.value, scope)?;
            defs.push(d);
            scope.binds.push((b.name.name.clone(), v));
        }
        let refs: Vec<&Bool> = defs.iter().collect();
        Ok(and(self.ctx, &refs))
    }

    /// The item's domain, as the evaluator checks it.
    fn domain(&mut self, constraints: &[Constraint]) -> E<Bool<'c>> {
        let ctx = self.ctx;
        let mut parts = Vec::new();
        for c in constraints {
            let mut scope = Scope::default();
            match c {
                Constraint::In { var, interval, .. } => {
                    let idx = key_index(self.env, &var.dotted(), var.segments.len() == 1)
                        .expect("checked at load");
                    let values = match &self.keys[idx] {
                        KeyVar::R(r) => vec![r.clone()],
                        KeyVar::Table { vals, .. } => vals.clone(),
                    };
                    let (lo, lod) = self.enc(&interval.lo, &mut scope)?;
                    let (hi, hid) = self.enc(&interval.hi, &mut scope)?;
                    let (lo, hi) = (Self::r(lo), Self::r(hi));
                    parts.push(lod);
                    parts.push(hid);
                    for v in values {
                        parts.push(if interval.lo_closed {
                            v.ge(&lo)
                        } else {
                            v.gt(&lo)
                        });
                        parts.push(if interval.hi_closed {
                            v.le(&hi)
                        } else {
                            v.lt(&hi)
                        });
                    }
                }
                Constraint::Pred(e) => {
                    let (v, d) = self.enc(e, &mut scope)?;
                    parts.push(d);
                    parts.push(Self::b(v));
                }
            }
        }
        let refs: Vec<&Bool> = parts.iter().collect();
        Ok(and(ctx, &refs))
    }
}

/// An exponent the encoding can unroll: an integer literal, possibly
/// negated.
fn const_int(e: &Expr) -> Option<i64> {
    match &e.kind {
        ExprKind::Number { value, .. } if value.fract() == 0.0 && value.abs() <= 16.0 => {
            Some(*value as i64)
        }
        ExprKind::Unary {
            op: UnOp::Neg,
            operand,
        } => const_int(operand).map(|n| -n),
        _ => None,
    }
}

/// One copy's evaluation: the item's value (a quantity or a predicate), and
/// the condition that it is defined and inside the domain.
struct Copy<'c> {
    value: T<'c>,
    ok: Bool<'c>,
}

/// A simple domain constraint a suggestion may add.
#[derive(Clone)]
struct Candidate {
    text: String,
    /// `x > 0`: (key); `a <= b`: (a, b).
    kind: CandKind,
}

#[derive(Clone)]
enum CandKind {
    Positive(usize),
    Le(usize, usize),
}

impl Problem<'_> {
    fn run(&self) -> (Verdict, Option<Vec<String>>) {
        let mut cfg = Config::new();
        cfg.set_model_generation(true);
        let ctx = Context::new(&cfg);
        match self.solve(&ctx) {
            Ok(v) => v,
            Err(reason) => (Verdict::Unknown(reason), None),
        }
    }

    fn fns(&self) -> HashMap<&str, &FnDecl> {
        self.checked
            .file()
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Fn(f) => Some((f.name.name.as_str(), f)),
                _ => None,
            })
            .collect()
    }

    /// Fresh variables for every key, tables at their sizes.
    fn vars<'c>(&self, ctx: &'c Context, tag: &str) -> E<Vec<KeyVar<'c>>> {
        let env = &self.checked.env;
        env.keys
            .iter()
            .enumerate()
            .map(|(i, k)| {
                Ok(match k.ty {
                    KeyTy::Table { .. } => {
                        let shape = self
                            .sizes
                            .iter()
                            .find(|(j, _)| *j == i)
                            .map_or(Shape::Size(1), |(_, s)| s.clone());
                        let keys = match &shape {
                            Shape::Keys(ks) => ks
                                .iter()
                                .map(|&x| rational(ctx, x))
                                .collect::<E<Vec<_>>>()?,
                            Shape::Size(n) => (0..*n)
                                .map(|j| Real::new_const(ctx, format!("{}{tag}#k{j}", k.path)))
                                .collect(),
                        };
                        let vals = (0..keys.len())
                            .map(|j| Real::new_const(ctx, format!("{}{tag}#v{j}", k.path)))
                            .collect();
                        KeyVar::Table { keys, vals }
                    }
                    _ => KeyVar::R(Real::new_const(ctx, format!("{}{tag}", k.path))),
                })
            })
            .collect()
    }

    fn copy<'c>(
        &self,
        ctx: &'c Context,
        keys: Vec<KeyVar<'c>>,
        axioms: &mut Vec<Bool<'c>>,
        roots: &mut Vec<(Real<'c>, Real<'c>)>,
    ) -> E<Copy<'c>> {
        let mut enc = Enc {
            ctx,
            env: &self.checked.env,
            fns: self.fns(),
            keys,
            axioms,
            roots,
        };
        let dom = enc.domain(&self.body.domain)?;
        let mut scope = Scope::default();
        let bd = enc.bindings(&self.body.bindings, &mut scope)?;
        let (value, vd) = enc.enc(&self.body.value, &mut scope)?;
        Ok(Copy {
            value,
            ok: and(ctx, &[&dom, &bd, &vd]),
        })
    }

    /// What the declarations themselves state, over one copy's variables.
    fn facts<'c>(&self, ctx: &'c Context, keys: &[KeyVar<'c>]) -> E<Vec<Bool<'c>>> {
        let zero = rational(ctx, 0.0)?;
        let mut out = Vec::new();
        for (k, v) in self.checked.env.keys.iter().zip(keys) {
            match (&k.ty, v) {
                (
                    KeyTy::Param {
                        magnitude: true, ..
                    },
                    KeyVar::R(r),
                ) => out.push(r.ge(&zero)),
                (KeyTy::Table { .. }, KeyVar::Table { keys, .. }) => {
                    for w in keys.windows(2) {
                        out.push(w[0].lt(&w[1]));
                    }
                }
                _ => {}
            }
        }
        Ok(out)
    }

    fn solve(&self, ctx: &Context) -> E<(Verdict, Option<Vec<String>>)> {
        let lo_keys = self.vars(ctx, "@1")?;
        // The second point shares every variable but the claim's.
        let mut hi_keys = lo_keys.clone();
        let varied = self.vars(ctx, "@2")?;
        hi_keys[self.var] = match (&lo_keys[self.var], &varied[self.var]) {
            (KeyVar::Table { keys, .. }, KeyVar::Table { vals, .. }) => KeyVar::Table {
                keys: keys.clone(),
                vals: vals.clone(),
            },
            (_, v) => v.clone(),
        };
        let (mut axioms, mut roots) = (Vec::new(), Vec::new());
        let lo = self.copy(ctx, lo_keys.clone(), &mut axioms, &mut roots)?;
        let hi = self.copy(ctx, hi_keys.clone(), &mut axioms, &mut roots)?;
        let mut base = axioms;
        // `sqrt` is monotone. Z3 can derive it from `s * s = x`, but on two
        // copies of a braking profile it did not within minutes; stated, it
        // is linear.
        for (i, (s, x)) in roots.iter().enumerate() {
            for (t, y) in &roots[i + 1..] {
                let zero = rational(ctx, 0.0)?;
                let both = and(ctx, &[&x.ge(&zero), &y.ge(&zero)]);
                for (a, u, b, v) in [(x, s, y, t), (y, t, x, s)] {
                    base.push(and(ctx, &[&both, &a.le(b)]).implies(&u.le(v)));
                    base.push(and(ctx, &[&both, &a.lt(b)]).implies(&u.lt(v)));
                }
            }
        }
        base.extend(self.facts(ctx, &lo_keys)?);
        base.extend(self.facts(ctx, &hi_keys)?);
        base.push(lo.ok.clone());
        base.push(hi.ok.clone());
        // The claim's key is lower at the first point.
        match (&lo_keys[self.var], &hi_keys[self.var]) {
            (KeyVar::R(a), KeyVar::R(b)) => base.push(a.lt(b)),
            (KeyVar::Table { vals: a, .. }, KeyVar::Table { vals: b, .. }) => {
                for (x, y) in a.iter().zip(b) {
                    base.push(x.le(y));
                }
            }
            _ => unreachable!("one key, two copies of one kind"),
        }
        let worse = self.violation(&lo.value, &hi.value);

        let solver = self.solver(ctx, self.opts.timeout_ms);
        for b in &base {
            solver.assert(b);
        }
        solver.assert(&worse);
        // A counterexample at positive values reads as physics; take one
        // when there is one.
        let positive = self.all_positive(ctx, &lo_keys, &hi_keys)?;
        let ms = self.opts.timeout_ms;
        let mut result = bounded(ctx, ms, || {
            solver.check_assumptions(std::slice::from_ref(&positive))
        });
        if result == SatResult::Sat {
            // Keep the counterexample physical while it is made readable.
            solver.assert(&positive);
        } else {
            result = bounded(ctx, ms, || solver.check());
        }
        match result {
            SatResult::Unsat => {
                // Not vacuous: an encoding that made every point undefined
                // would prove anything, so the domain must admit two
                // defined points with the claim's key ordered.
                let any = self.solver(ctx, ms);
                for b in &base {
                    any.assert(b);
                }
                Ok(match bounded(ctx, ms, || any.check()) {
                    SatResult::Sat => (Verdict::Proved, None),
                    SatResult::Unsat => (
                        Verdict::Unknown(
                            "vacuous: no two points inside the domain are both defined".into(),
                        ),
                        None,
                    ),
                    SatResult::Unknown => (
                        Verdict::Unknown(
                            "Z3 found no violation, but could not show the domain non-empty".into(),
                        ),
                        None,
                    ),
                })
            }
            SatResult::Unknown => Ok((
                Verdict::Unknown(format!(
                    "Z3 could not decide it ({})",
                    solver
                        .get_reason_unknown()
                        .unwrap_or_else(|| "no reason given".into())
                )),
                None,
            )),
            SatResult::Sat => {
                self.round(ctx, &solver, &lo_keys, &hi_keys);
                let model = solver.get_model().ok_or("Z3 gave no model")?;
                let (p1, p2) = (
                    self.point_of(&model, &lo_keys)?,
                    self.point_of(&model, &hi_keys)?,
                );
                let Some(cex) = self.confirm(&p1, &p2) else {
                    return Ok((
                        Verdict::Unknown(
                            "Z3 found a counterexample the evaluator does not reproduce".into(),
                        ),
                        None,
                    ));
                };
                let holds_on = self.suggest(ctx, &base, &worse, &lo_keys, &hi_keys, &p1, &p2);
                Ok((Verdict::Refuted(Box::new(cex)), holds_on))
            }
        }
    }

    /// Make the counterexample readable: pin each key, one at a time, to
    /// the contract's value or a small whole number in SI when a violation
    /// still exists with it, so the report reads `descent_speed = 1 m/s`
    /// rather than `0.4547 m/s`. Leaves the solver satisfiable, with the
    /// pins asserted.
    fn round<'c>(
        &self,
        ctx: &'c Context,
        solver: &Solver<'c>,
        lo_keys: &[KeyVar<'c>],
        hi_keys: &[KeyVar<'c>],
    ) {
        let budget = (self.opts.timeout_ms / 10).max(200);
        let env = &self.checked.env;
        let mut vars: Vec<(Real<'c>, Option<f64>)> = Vec::new();
        for (i, (a, b)) in lo_keys.iter().zip(hi_keys).enumerate() {
            let (KeyVar::R(a), KeyVar::R(b)) = (a, b) else {
                continue;
            };
            let at = match self.opts.point.get(&env.keys[i].path) {
                Some(Value::Quantity { si, .. }) if i != self.var => Some(*si),
                _ => None,
            };
            vars.push((a.clone(), at));
            if i == self.var {
                vars.push((b.clone(), None));
            }
        }
        for (v, at) in vars {
            let mut tries: Vec<f64> = at.into_iter().collect();
            tries.extend([1.0, 2.0, 3.0, 5.0, 10.0, 20.0, 0.5, 0.0, -1.0]);
            for x in tries {
                let Ok(r) = rational(ctx, x) else { continue };
                let pin = v._eq(&r);
                if bounded(ctx, budget, || {
                    solver.check_assumptions(std::slice::from_ref(&pin))
                }) == SatResult::Sat
                {
                    solver.assert(&pin);
                    break;
                }
            }
        }
        // The model the caller reads must be the last satisfiable one.
        let _ = bounded(ctx, budget.max(self.opts.timeout_ms), || solver.check());
    }

    fn solver<'c>(&self, ctx: &'c Context, timeout_ms: u32) -> Solver<'c> {
        // Nonlinear real arithmetic: the default solver's arithmetic theory
        // gave up as incomplete on rail's distance sum, which nlsat decides.
        let solver = Solver::new_for_logic(ctx, "QF_NRA").unwrap_or_else(|| Solver::new(ctx));
        let mut p = Params::new(ctx);
        p.set_u32("timeout", timeout_ms);
        solver.set_params(&p);
        solver
    }

    /// The second point is worse than the first, the way the claim says it
    /// never is.
    fn violation<'c>(&self, lo: &T<'c>, hi: &T<'c>) -> Bool<'c> {
        match (lo, hi, self.item, self.direction) {
            (T::R(a), T::R(b), _, Direction::Increasing) => a.gt(b),
            (T::R(a), T::R(b), _, Direction::Decreasing) => a.lt(b),
            // A check: passing at the worse point and failing at the better.
            (T::B(a), T::B(b), _, Direction::Increasing) => Bool::and(a.get_ctx(), &[b, &a.not()]),
            (T::B(a), T::B(b), _, Direction::Decreasing) => Bool::and(a.get_ctx(), &[a, &b.not()]),
            _ => unreachable!("a derive is a quantity and a check a predicate"),
        }
    }

    fn all_positive<'c>(
        &self,
        ctx: &'c Context,
        a: &[KeyVar<'c>],
        b: &[KeyVar<'c>],
    ) -> E<Bool<'c>> {
        let zero = rational(ctx, 0.0)?;
        let mut parts = Vec::new();
        for v in a.iter().chain(b) {
            match v {
                KeyVar::R(r) => parts.push(r.gt(&zero)),
                KeyVar::Table { vals, .. } => parts.extend(vals.iter().map(|x| x.gt(&zero))),
            }
        }
        let refs: Vec<&Bool> = parts.iter().collect();
        Ok(and(ctx, &refs))
    }

    /// The model's values of the keys the item reads, as evaluator inputs.
    fn point_of(&self, model: &z3::Model, keys: &[KeyVar]) -> E<Inputs> {
        let env = &self.checked.env;
        let reads = env.reads(self.item).unwrap_or_default();
        let value = |r: &Real| -> E<f64> {
            let v = model.eval(r, true).ok_or("no value in the model")?;
            if let Some((n, d)) = v.as_real() {
                return Ok(n as f64 / d as f64);
            }
            // A rational too large for i64 prints as `(/ N.0 D.0)`, a
            // negative one inside `(- ...)`; an algebraic number does not.
            numeral(&v.to_string()).ok_or_else(|| format!("the counterexample is irrational ({v})"))
        };
        let mut out = Inputs::new();
        for need in &reads {
            if need.kind == KeyKind::Core && need.path != "reaction_total" {
                continue;
            }
            let idx = index_of(env, &need.path);
            let v = match &keys[idx] {
                KeyVar::R(r) => Value::Quantity {
                    si: value(r)?,
                    dim: key_dim(&need.ty),
                    written: None,
                },
                KeyVar::Table { keys, vals } => Value::Table(
                    keys.iter()
                        .zip(vals)
                        .map(|(k, v)| Ok((value(k)?, value(v)?)))
                        .collect::<E<Vec<_>>>()?,
                ),
            };
            out.insert(need.path.clone(), v);
        }
        Ok(out)
    }

    /// The evaluator at both points; `Some` when it reproduces the
    /// violation.
    fn confirm(&self, p1: &Inputs, p2: &Inputs) -> Option<Counterexample> {
        let (a, b, worse) = match self.item {
            ItemRef::Derive(t) => {
                let a = eval::eval_derive(self.checked, t, p1).ok()?;
                let b = eval::eval_derive(self.checked, t, p2).ok()?;
                let worse = match self.direction {
                    Direction::Increasing => a.si > b.si,
                    Direction::Decreasing => a.si < b.si,
                };
                (a.display, b.display, worse)
            }
            ItemRef::Check(n) => {
                let a = eval::eval_check(self.checked, n, p1).ok()?.pass;
                let b = eval::eval_check(self.checked, n, p2).ok()?.pass;
                let worse = match self.direction {
                    Direction::Increasing => b && !a,
                    Direction::Decreasing => a && !b,
                };
                let v = |p: bool| if p { "PASS" } else { "FAIL" }.to_string();
                (v(a), v(b), worse)
            }
            ItemRef::Report(_) => return None,
        };
        if !worse {
            return None;
        }
        let env = &self.checked.env;
        let var_path = &env.keys[self.var].path;
        let show = |v: &Value, ty: &KeyTy| match (v, ty) {
            (Value::Quantity { si, dim, .. }, _) => display::quantity(*si, *dim),
            (Value::Table(rows), KeyTy::Table { from, to }) => format!(
                "{{ {} }}",
                rows.iter()
                    .map(|(k, x)| format!(
                        "{}: {}",
                        display::quantity(*k, *from),
                        display::quantity(*x, *to)
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            _ => String::new(),
        };
        let shared = p1
            .iter()
            .filter(|(k, _)| *k != var_path)
            .map(|(k, v)| {
                let info = env.key(k).expect("a read key");
                (info.last.clone(), show(v, &info.ty))
            })
            .collect();
        let ty = &env.keys[self.var].ty;
        Some(Counterexample {
            shared,
            lo: show(&p1[var_path], ty),
            hi: show(&p2[var_path], ty),
            at_lo: a,
            at_hi: b,
        })
    }

    /// The candidate constraints: each scalar key the item reads positive,
    /// and each pair of one dimension ordered.
    fn candidates(&self) -> Vec<Candidate> {
        let env = &self.checked.env;
        let reads = env.reads(self.item).unwrap_or_default();
        let scalars: Vec<usize> = reads
            .iter()
            .filter(|n| !matches!(n.ty, KeyTy::Table { .. }))
            .map(|n| index_of(env, &n.path))
            .collect();
        let mut out = Vec::new();
        for &a in &scalars {
            for &b in &scalars {
                if a != b && key_dim(&env.keys[a].ty) == key_dim(&env.keys[b].ty) {
                    out.push(Candidate {
                        text: format!("{} <= {}", env.keys[a].last, env.keys[b].last),
                        kind: CandKind::Le(a, b),
                    });
                }
            }
        }
        for &a in &scalars {
            let dim = key_dim(&env.keys[a].ty);
            let Some(unit) = unit_text(dim) else { continue };
            let zero = if unit.is_empty() {
                "0".to_string()
            } else {
                format!("0 {unit}")
            };
            out.push(Candidate {
                text: format!("{} > {zero}", env.keys[a].last),
                kind: CandKind::Positive(a),
            });
        }
        out
    }

    fn cand_bool<'c>(&self, ctx: &'c Context, c: &Candidate, keys: &[KeyVar<'c>]) -> Bool<'c> {
        let r = |i: usize| match &keys[i] {
            KeyVar::R(r) => r.clone(),
            KeyVar::Table { .. } => unreachable!("candidates are scalar"),
        };
        match c.kind {
            CandKind::Positive(a) => r(a).gt(&Real::from_real(ctx, 0, 1)),
            CandKind::Le(a, b) => r(a).le(&r(b)),
        }
    }

    /// The candidate at a point; `None` when the point lacks a key.
    fn cand_holds(&self, c: &Candidate, p: &Inputs) -> Option<bool> {
        let env = &self.checked.env;
        let si = |i: usize| match p.get(&env.keys[i].path)? {
            Value::Quantity { si, .. } => Some(*si),
            Value::Table(_) => None,
        };
        Some(match c.kind {
            CandKind::Positive(a) => si(a)? > 0.0,
            CandKind::Le(a, b) => si(a)? <= si(b)?,
        })
    }

    /// A set of at most two candidates that excludes the counterexample,
    /// admits the contract's values, leaves the domain satisfiable and
    /// proves the claim.
    #[allow(clippy::too_many_arguments)]
    fn suggest<'c>(
        &self,
        ctx: &'c Context,
        base: &[Bool<'c>],
        worse: &Bool<'c>,
        lo_keys: &[KeyVar<'c>],
        hi_keys: &[KeyVar<'c>],
        p1: &Inputs,
        p2: &Inputs,
    ) -> Option<Vec<String>> {
        let usable: Vec<Candidate> = self
            .candidates()
            .into_iter()
            .filter(|c| self.cand_holds(c, &self.opts.point) != Some(false))
            .collect();
        let excludes = |c: &Candidate| {
            self.cand_holds(c, p1) == Some(false) || self.cand_holds(c, p2) == Some(false)
        };
        let mut sets: Vec<Vec<&Candidate>> = Vec::new();
        for c in usable.iter().filter(|c| excludes(c)) {
            sets.push(vec![c]);
        }
        for (i, a) in usable.iter().enumerate() {
            for b in &usable[i + 1..] {
                if excludes(a) || excludes(b) {
                    sets.push(vec![a, b]);
                }
            }
        }
        // Three signs, for a quotient of several keys (`s / l` and `a / l`
        // need `s`, `a` and `l` positive).
        let positives: Vec<&Candidate> = usable
            .iter()
            .filter(|c| matches!(c.kind, CandKind::Positive(_)))
            .collect();
        for (i, a) in positives.iter().enumerate() {
            for (j, b) in positives.iter().enumerate().skip(i + 1) {
                for c in &positives[j + 1..] {
                    if excludes(a) || excludes(b) || excludes(c) {
                        sets.push(vec![*a, *b, *c]);
                    }
                }
            }
        }
        // Smaller sets first, and among sets of one size those that only
        // state signs: `x > 0` is a fact an author knows, `a <= b` a design
        // choice.
        let relational = |set: &Vec<&Candidate>| {
            set.iter()
                .filter(|c| matches!(c.kind, CandKind::Le(..)))
                .count()
        };
        sets.sort_by_key(|set| (set.len(), relational(set)));
        let budget = (self.opts.timeout_ms / 5).max(500);
        for set in sets.into_iter().take(96) {
            let extra: Vec<Bool> = set
                .iter()
                .flat_map(|c| {
                    [
                        self.cand_bool(ctx, c, lo_keys),
                        self.cand_bool(ctx, c, hi_keys),
                    ]
                })
                .collect();
            // Not vacuous: the narrowed domain still has a defined point.
            let s = self.solver(ctx, budget);
            for b in base.iter().chain(&extra) {
                s.assert(b);
            }
            if bounded(ctx, budget, || s.check()) != SatResult::Sat {
                continue;
            }
            s.assert(worse);
            if bounded(ctx, budget, || s.check()) == SatResult::Unsat {
                return Some(set.iter().map(|c| c.text.clone()).collect());
            }
        }
        None
    }
}

/// A Z3 numeral as printed: `3`, `2.5`, `(- X)`, `(/ X Y)`.
fn numeral(t: &str) -> Option<f64> {
    let t = t.trim();
    if let Some(inner) = t.strip_prefix("(- ").and_then(|r| r.strip_suffix(')')) {
        return numeral(inner).map(|x| -x);
    }
    if let Some(inner) = t.strip_prefix("(/ ").and_then(|r| r.strip_suffix(')')) {
        let (a, b) = inner.split_once(' ')?;
        return Some(numeral(a)? / numeral(b)?);
    }
    t.parse().ok()
}

/// Run one Z3 query with a deadline. The solver's own `timeout` parameter
/// is not honoured by the nonlinear arithmetic engine (a braking profile ran
/// past two minutes under a five-second one), so a watchdog thread
/// interrupts the context; the query then answers `Unknown`.
fn bounded(ctx: &Context, ms: u32, query: impl FnOnce() -> SatResult) -> SatResult {
    let handle = ctx.handle();
    let (done, wait) = std::sync::mpsc::channel::<()>();
    std::thread::scope(|scope| {
        scope.spawn(move || {
            if wait
                .recv_timeout(std::time::Duration::from_millis(ms.into()))
                .is_err()
            {
                handle.interrupt();
            }
        });
        let result = query();
        let _ = done.send(());
        result
    })
}

/// A unit spelling of `dim` that rlx parses, for a `0 <unit>` bound. The
/// bound is zero, so any unit of the dimension is exact: a coherent one, else
/// any one, else a quotient of two (`mL/mmHg`), else the SI base spelling.
fn unit_text(dim: Dim) -> Option<String> {
    if dim.is_scalar() {
        return Some(String::new());
    }
    let all: Vec<(&str, Dim, bool)> = units::symbols()
        .filter_map(|s| {
            let u = units::lookup(s)?;
            Some((s, Dim(u.dim), u.scale == 1.0))
        })
        .collect();
    for coherent in [true, false] {
        if let Some((s, _, _)) = all.iter().find(|(_, d, c)| *d == dim && (*c || !coherent)) {
            return Some(s.to_string());
        }
    }
    for coherent in [true, false] {
        for (a, da, ca) in &all {
            for (b, db, cb) in &all {
                if da.per(*db) == dim && ((*ca && *cb) || !coherent) {
                    return Some(format!("{a}/{b}"));
                }
            }
        }
    }
    Some(units::example(dim))
}
