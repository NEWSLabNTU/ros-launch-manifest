//! The evaluator: a checked extension, a contract's values, a number and
//! the trace that explains it.
//!
//! Pure and total (design 5.2): no IO, no clock, no randomness, and every
//! evaluation terminates, because recursion was refused at load and `sum
//! over` ranges over a finite table. Arithmetic is IEEE-754 `f64` in a
//! fixed order (operands left to right, `where` bindings in order), so the
//! same inputs give the same number and the same trace text, byte for
//! byte. Every numeric failure (division by zero, the square root of a
//! negative, a value that is not finite, a `bands` slice outside its table)
//! is an error with a span into the extension, never a panic and never a
//! silently wrong number.
//!
//! # API
//!
//! - [`identity`]`(&Checked) -> Identity { name, version, sha256 }`; the
//!   hash is over the canonical ASCII form, so the Unicode and ASCII
//!   spellings of one file hash the same.
//! - [`Env::reads`](crate::typeck::Env::reads)`(&ItemRef) -> Option<Vec<Need>>`:
//!   the keys and inputs an item reads, with their types and whether each
//!   has a default.
//! - [`Value`]: what the consumer supplies per key, keyed in [`Inputs`] by
//!   the key's full declared path (`hazard.entry_speed`, `settle.response`,
//!   `reaction_total`). A contract's text converts with
//!   [`Env::parse_value`](crate::typeck::Env::parse_value) and
//!   [`Value::from_quantity`]; a node parameter's raw number with
//!   [`param_value`], which applies `magnitude` and `unit U`, so that rule
//!   lives here only.
//! - [`eval_derive`], [`eval_check`] and [`eval_report`] evaluate one item.
//!   A missing input with a default takes the default (evaluated, and
//!   marked so in the trace); one without is `rlx-missing-input`. The
//!   item's `domain:` is checked before its body; an input outside it is
//!   `rlx-domain`, since a worst-case claim holds only inside the domain.
//!
//! # Trace
//!
//! One line per step, indented two spaces per level: each input the item
//! reads, each `where` binding (`name = expression = value`), each `if`
//! (the condition, the values it compared, the branch taken), each user
//! `fn` call with its result and, beneath it, its parameters and steps,
//! each `sum over` with its total and one line per band, and the final
//! value. Expressions are printed in canonical ASCII on one line
//! ([`crate::fmt::ascii_span`]); values follow [`crate::display`].

use crate::{
    Checked, Style,
    ast::*,
    diag::{Diagnostic, Span, code},
    dim::Dim,
    display, fmt,
    typeck::{Env, ItemRef, KeyTy},
    units, value,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};

/// What a checked extension is, for `--explain` and a lock file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub name: String,
    /// `X.Y.Z`
    pub version: String,
    /// Lowercase hex SHA-256 of the canonical ASCII form.
    pub sha256: String,
}

pub fn identity(checked: &Checked) -> Identity {
    let (name, version) = checked
        .file()
        .items
        .iter()
        .find_map(|i| match i {
            Item::Extension(e) => Some((
                e.name.dotted(),
                format!(
                    "{}.{}.{}",
                    e.version.major, e.version.minor, e.version.patch
                ),
            )),
            _ => None,
        })
        .expect("a checked file has its extension header");
    let digest = Sha256::digest(checked.format(Style::Ascii).as_bytes());
    let sha256 = digest.iter().map(|b| format!("{b:02x}")).collect();
    Identity {
        name,
        version,
        sha256,
    }
}

/// A value the consumer supplies for a key or input.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// In SI base units. `written` is the contract's text (`30 km/h`), shown
    /// in the trace beside the value.
    Quantity {
        si: f64,
        dim: Dim,
        written: Option<String>,
    },
    /// A `table A -> B` key: (key, value) pairs in SI, keys strictly
    /// increasing; each entry holds from its key up to the next.
    Table(Vec<(f64, f64)>),
}

impl Value {
    pub fn from_quantity(q: value::Quantity, written: Option<&str>) -> Value {
        Value::Quantity {
            si: q.si,
            dim: q.dim,
            written: written.map(str::to_string),
        }
    }

    pub fn table(rows: &[(value::Quantity, value::Quantity)]) -> Value {
        Value::Table(rows.iter().map(|(k, v)| (k.si, v.si)).collect())
    }
}

/// Inputs by full declared path.
pub type Inputs = BTreeMap<String, Value>;

/// A node parameter's raw number as the value of a `param` key: its
/// absolute value under `magnitude`, and read in `U` under `unit U` (else in
/// SI base units). `None` for a key that is not a `param` key.
pub fn param_value(raw: f64, ty: &KeyTy) -> Option<Value> {
    let KeyTy::Param {
        dim,
        magnitude,
        unit,
    } = ty
    else {
        return None;
    };
    let v = if *magnitude { raw.abs() } else { raw };
    let si = v * unit.as_ref().map_or(1.0, |u| u.scale);
    Some(Value::Quantity {
        si,
        dim: *dim,
        written: None,
    })
}

/// One step of a trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceLine {
    pub depth: usize,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Trace {
    pub lines: Vec<TraceLine>,
}

impl Trace {
    /// The lines, two spaces of indent per level, each ending in a newline.
    pub fn render(&self) -> String {
        let mut s = String::new();
        for l in &self.lines {
            for _ in 0..l.depth {
                s.push_str("  ");
            }
            s.push_str(&l.text);
            s.push('\n');
        }
        s
    }
}

/// A derived or reported value.
#[derive(Debug, Clone, PartialEq)]
pub struct Evaluation {
    pub si: f64,
    pub dim: Dim,
    /// The value as the trace prints it.
    pub display: String,
    pub trace: Trace,
}

/// The two sides of a check whose predicate is one comparison.
#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    pub op: CmpOp,
    pub lhs: f64,
    pub rhs: f64,
    pub dim: Dim,
    /// How far the comparison is from failing, in SI: `rhs - lhs` for `<`
    /// and `<=`, `lhs - rhs` for `>` and `>=`; `None` for `==` and `!=`.
    pub margin: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CheckOutcome {
    pub pass: bool,
    pub comparison: Option<Comparison>,
    pub trace: Trace,
}

pub fn eval_derive(
    checked: &Checked,
    target: &str,
    inputs: &Inputs,
) -> Result<Evaluation, Diagnostic> {
    let d = checked
        .file()
        .items
        .iter()
        .find_map(|i| match i {
            Item::Derive(d) if d.target.dotted() == target => Some(d),
            _ => None,
        })
        .ok_or_else(|| no_item("derive", target))?;
    let mut ev = Ev::new(checked, inputs);
    ev.inputs_of(&ItemRef::Derive(target.to_string()))?;
    ev.domain(&d.body.domain)?;
    let mut scope = Scope::default();
    ev.bindings(&d.body.bindings, &mut scope)?;
    let (si, dim) = ev.eval(&d.body.value, &mut scope)?.q();
    let display = display::quantity(si, dim);
    let text = ev.text(d.body.value.span);
    ev.line(format!("{target} = {text} = {display}"));
    Ok(Evaluation {
        si,
        dim,
        display,
        trace: ev.trace,
    })
}

pub fn eval_report(
    checked: &Checked,
    name: &str,
    inputs: &Inputs,
) -> Result<Evaluation, Diagnostic> {
    let r = checked
        .file()
        .items
        .iter()
        .find_map(|i| match i {
            Item::Report(r) if r.name.name == name => Some(r),
            _ => None,
        })
        .ok_or_else(|| no_item("report", name))?;
    let mut ev = Ev::new(checked, inputs);
    ev.inputs_of(&ItemRef::Report(name.to_string()))?;
    let mut scope = Scope::default();
    ev.bindings(&r.bindings, &mut scope)?;
    let (si, dim) = ev.eval(&r.value, &mut scope)?.q();
    let display = display::quantity(si, dim);
    let text = ev.text(r.value.span);
    ev.line(format!("{name} = {text} = {display}"));
    Ok(Evaluation {
        si,
        dim,
        display,
        trace: ev.trace,
    })
}

pub fn eval_check(
    checked: &Checked,
    name: &str,
    inputs: &Inputs,
) -> Result<CheckOutcome, Diagnostic> {
    let c = checked
        .file()
        .items
        .iter()
        .find_map(|i| match i {
            Item::Check(c) if c.name.name == name => Some(c),
            _ => None,
        })
        .ok_or_else(|| no_item("check", name))?;
    let mut ev = Ev::new(checked, inputs);
    ev.inputs_of(&ItemRef::Check(name.to_string()))?;
    ev.domain(&c.body.domain)?;
    let mut scope = Scope::default();
    ev.bindings(&c.body.bindings, &mut scope)?;
    let text = ev.text(c.body.value.span);
    let (pass, sides) = ev.predicate(&c.body.value, &mut scope)?;
    let verdict = if pass { "PASS" } else { "FAIL" };
    let comparison = match (&c.body.value.kind, sides) {
        (
            ExprKind::Binary {
                op: BinOp::Cmp(op), ..
            },
            Some((V::Q(l, dim), V::Q(r, _))),
        ) => {
            let margin = match op {
                CmpOp::Lt | CmpOp::Le => Some(r - l),
                CmpOp::Gt | CmpOp::Ge => Some(l - r),
                CmpOp::Eq | CmpOp::Ne => None,
            };
            Some(Comparison {
                op: *op,
                lhs: l,
                rhs: r,
                dim,
                margin,
            })
        }
        _ => None,
    };
    let detail = match &comparison {
        Some(cmp) => {
            let mut s = format!(
                ": {} {} {}",
                display::quantity(cmp.lhs, cmp.dim),
                cmp.op.ascii(),
                display::quantity(cmp.rhs, cmp.dim)
            );
            if let Some(m) = cmp.margin {
                s.push_str(&format!(" (margin {})", display::quantity(m, cmp.dim)));
            }
            s
        }
        None => String::new(),
    };
    ev.line(format!("{name}: {text}{detail} {verdict}"));
    Ok(CheckOutcome {
        pass,
        comparison,
        trace: ev.trace,
    })
}

fn no_item(kind: &str, name: &str) -> Diagnostic {
    Diagnostic::new(
        code::NO_ITEM,
        Span::default(),
        format!("the extension has no {kind} `{name}`"),
    )
}

#[derive(Debug, Clone, Copy)]
struct Band {
    lo: f64,
    hi: f64,
    value: f64,
}

/// A value during evaluation.
#[derive(Debug, Clone)]
enum V {
    Q(f64, Dim),
    B(bool),
    Table(Vec<(f64, f64)>, Dim, Dim),
    List(Vec<Band>, Dim, Dim),
    Band(Band, Dim, Dim),
}

impl V {
    /// The checker guarantees the kind; these unwrap it.
    fn q(self) -> (f64, Dim) {
        match self {
            V::Q(v, d) => (v, d),
            other => unreachable!("type-checked to a quantity, got {other:?}"),
        }
    }

    fn b(self) -> bool {
        match self {
            V::B(b) => b,
            other => unreachable!("type-checked to a boolean, got {other:?}"),
        }
    }

    fn show(&self) -> String {
        match self {
            V::Q(v, d) => display::quantity(*v, *d),
            V::B(b) => b.to_string(),
            V::Table(rows, ..) => format!("a table of {} entries", rows.len()),
            V::List(bands, ..) => format!("{} bands", bands.len()),
            V::Band(b, a, v) => band_text(b, *a, *v),
        }
    }
}

fn band_text(b: &Band, a: Dim, v: Dim) -> String {
    let hi = if b.hi.is_infinite() {
        "inf".to_string()
    } else {
        display::quantity(b.hi, a)
    };
    format!(
        "[{}, {hi}] {}",
        display::quantity(b.lo, a),
        display::quantity(b.value, v)
    )
}

#[derive(Default)]
struct Scope {
    binds: Vec<(String, V)>,
    sums: Vec<(String, V)>,
    params: Vec<(String, V)>,
}

impl Scope {
    fn find(&self, name: &str) -> Option<V> {
        let look = |v: &Vec<(String, V)>| {
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

type R<T> = Result<T, Diagnostic>;

struct Ev<'a> {
    checked: &'a Checked<'a>,
    env: &'a Env,
    inputs: &'a Inputs,
    fns: HashMap<&'a str, &'a FnDecl>,
    defaults: HashMap<String, &'a Expr>,
    /// Key values, once read.
    keys: Vec<Option<V>>,
    resolving: Vec<bool>,
    trace: Trace,
    depth: usize,
}

fn fail(code: &'static str, span: Span, msg: impl Into<String>) -> Diagnostic {
    Diagnostic::new(code, span, msg)
}

/// Refuse a value that is not finite: an overflow or an undefined power
/// must not flow on as a number.
fn finite(v: f64, span: Span) -> R<f64> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(fail(
            code::NON_FINITE,
            span,
            format!("this evaluates to {v}, which is not a finite number"),
        ))
    }
}

impl<'a> Ev<'a> {
    fn new(checked: &'a Checked<'a>, inputs: &'a Inputs) -> Self {
        let file = checked.file();
        let mut fns = HashMap::new();
        let mut defaults = HashMap::new();
        for item in &file.items {
            match item {
                Item::Fn(f) => {
                    fns.entry(f.name.name.as_str()).or_insert(f);
                }
                Item::Key(KeyDecl {
                    path,
                    default: Some(d),
                    ..
                })
                | Item::Input(InputDecl {
                    path,
                    default: Some(d),
                    ..
                }) => {
                    defaults.insert(path.dotted(), d);
                }
                _ => {}
            }
        }
        let n = checked.env.keys.len();
        Ev {
            checked,
            env: &checked.env,
            inputs,
            fns,
            defaults,
            keys: vec![None; n],
            resolving: vec![false; n],
            trace: Trace::default(),
            depth: 0,
        }
    }

    fn text(&self, span: Span) -> String {
        let s = &self.checked.syntax;
        fmt::ascii_span(s.source, &s.tokens, span)
    }

    fn line(&mut self, text: String) {
        self.trace.lines.push(TraceLine {
            depth: self.depth,
            text,
        });
    }

    /// Push a line whose text is known only after its children, and indent.
    fn open(&mut self) -> usize {
        self.line(String::new());
        self.depth += 1;
        self.trace.lines.len() - 1
    }

    fn close(&mut self, at: usize, text: String) {
        self.depth -= 1;
        self.trace.lines[at].text = text;
    }

    /// Read every key the item needs, up front and in declaration order, so
    /// a missing one is reported whichever branch would have read it, and
    /// the trace lists the inputs first.
    fn inputs_of(&mut self, item: &ItemRef) -> R<()> {
        let needs = self.env.reads(item).unwrap_or_default();
        for need in needs {
            let idx = self
                .env
                .keys
                .iter()
                .position(|k| k.path == need.path)
                .expect("reads names declared keys");
            self.key(idx, self.env.keys[idx].span)?;
        }
        Ok(())
    }

    fn key(&mut self, idx: usize, at: Span) -> R<V> {
        if let Some(v) = &self.keys[idx] {
            return Ok(v.clone());
        }
        let info = &self.env.keys[idx];
        let (path, last) = (info.path.clone(), info.last.clone());
        let (v, note) = match self.inputs.get(&path) {
            Some(given) => {
                let v = self.convert(idx, given, at)?;
                let note = match given {
                    Value::Quantity {
                        written: Some(w), ..
                    } => format!("   ({w})"),
                    _ => String::new(),
                };
                (v, note)
            }
            None => {
                let Some(default) = self.defaults.get(&path).copied() else {
                    return Err(fail(
                        code::MISSING_INPUT,
                        at,
                        format!(
                            "no value for `{path}`: the contract must give it, since it has \
                             no default"
                        ),
                    ));
                };
                if self.resolving[idx] {
                    return Err(fail(
                        code::MISSING_INPUT,
                        default.span,
                        format!("the default of `{path}` depends on itself"),
                    ));
                }
                self.resolving[idx] = true;
                let mut scope = Scope::default();
                let v = self.eval(default, &mut scope);
                self.resolving[idx] = false;
                (v?, "   (default)".to_string())
            }
        };
        self.line(format!("{last} = {}{note}", v.show()));
        self.keys[idx] = Some(v.clone());
        Ok(v)
    }

    /// Check a supplied value against the key's type.
    fn convert(&self, idx: usize, given: &Value, at: Span) -> R<V> {
        let info = &self.env.keys[idx];
        let bad = |msg: String| fail(code::INPUT, at, format!("`{}`: {msg}", info.path));
        match (&info.ty, given) {
            (KeyTy::Value(d) | KeyTy::Param { dim: d, .. }, Value::Quantity { si, dim, .. }) => {
                if dim != d {
                    return Err(bad(format!(
                        "the value is a {}, but the key is a {}",
                        self.env.name_of(*dim),
                        self.env.name_of(*d)
                    )));
                }
                if !si.is_finite() {
                    return Err(bad(format!("{si} is not a finite number")));
                }
                Ok(V::Q(*si, *d))
            }
            (KeyTy::Table { from, to }, Value::Table(rows)) => {
                if rows.is_empty() {
                    return Err(bad("a table needs at least one entry".into()));
                }
                if rows.iter().any(|(k, v)| !k.is_finite() || !v.is_finite()) {
                    return Err(bad("a table entry is not a finite number".into()));
                }
                if rows.windows(2).any(|w| w[1].0 <= w[0].0) {
                    return Err(bad("table keys must increase strictly".into()));
                }
                Ok(V::Table(rows.clone(), *from, *to))
            }
            (KeyTy::Table { .. }, Value::Quantity { .. }) => Err(bad(
                "the key is a table, but a single value was given".into(),
            )),
            (_, Value::Table(_)) => {
                Err(bad("a table was given, but the key is not a table".into()))
            }
        }
    }

    fn key_index(&self, path: &str, single: bool) -> Option<usize> {
        let keys = &self.env.keys;
        keys.iter().position(|k| k.path == path).or_else(|| {
            if single {
                keys.iter().position(|k| k.last == path)
            } else {
                None
            }
        })
    }

    fn domain(&mut self, constraints: &[Constraint]) -> R<()> {
        for c in constraints {
            let mut scope = Scope::default();
            match c {
                Constraint::In { var, interval, .. } => {
                    let idx = self
                        .key_index(&var.dotted(), var.segments.len() == 1)
                        .expect("checked at load");
                    let (v, dim) = self.key(idx, var.span)?.q();
                    let (lo, _) = self.eval(&interval.lo, &mut scope)?.q();
                    let (hi, _) = self.eval(&interval.hi, &mut scope)?.q();
                    let above = if interval.lo_closed { v >= lo } else { v > lo };
                    let below = if interval.hi_closed { v <= hi } else { v < hi };
                    if !(above && below) {
                        return Err(fail(
                            code::DOMAIN,
                            var.span,
                            format!(
                                "`{}` = {} is outside its domain {}{}, {}{}; the worst-case \
                                 claim holds only inside it",
                                var.dotted(),
                                display::quantity(v, dim),
                                if interval.lo_closed { "[" } else { "(" },
                                display::quantity(lo, dim),
                                display::quantity(hi, dim),
                                if interval.hi_closed { "]" } else { ")" },
                            ),
                        ));
                    }
                }
                Constraint::Pred(e) => {
                    let (ok, sides) = self.predicate(e, &mut scope)?;
                    if !ok {
                        let detail = match (&e.kind, sides) {
                            (
                                ExprKind::Binary {
                                    op: BinOp::Cmp(op), ..
                                },
                                Some((l, r)),
                            ) => format!(" ({} {} {})", l.show(), op.ascii(), r.show()),
                            _ => String::new(),
                        };
                        return Err(fail(
                            code::DOMAIN,
                            e.span,
                            format!(
                                "the domain constraint `{}` does not hold{detail}; the \
                                 worst-case claim holds only inside the domain",
                                self.text(e.span)
                            ),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn bindings(&mut self, bindings: &[Binding], scope: &mut Scope) -> R<()> {
        for b in bindings {
            let at = self.open();
            let v = self.eval(&b.value, scope)?;
            let text = format!(
                "{} = {} = {}",
                b.name.name,
                self.text(b.value.span),
                v.show()
            );
            self.close(at, text);
            scope.binds.push((b.name.name.clone(), v));
        }
        Ok(())
    }

    /// A boolean, and the two sides when it is one comparison.
    fn predicate(&mut self, e: &Expr, scope: &mut Scope) -> R<(bool, Option<(V, V)>)> {
        if let ExprKind::Binary {
            op: BinOp::Cmp(op),
            lhs,
            rhs,
        } = &e.kind
        {
            let l = self.eval(lhs, scope)?;
            let r = self.eval(rhs, scope)?;
            let b = compare(*op, &l, &r);
            return Ok((b, Some((l, r))));
        }
        Ok((self.eval(e, scope)?.b(), None))
    }

    fn eval(&mut self, e: &Expr, scope: &mut Scope) -> R<V> {
        match &e.kind {
            ExprKind::Number { value, .. } => Ok(V::Q(*value, Dim::SCALAR)),
            ExprKind::Quantity { value, unit, .. } => {
                let (dim, scale) = units::product(unit).expect("units are checked at parse");
                Ok(V::Q(finite(value * scale, e.span)?, dim))
            }
            ExprKind::Path(p) => self.path(p, scope),
            ExprKind::Call { callee, args } => self.call(e, callee, args, scope),
            ExprKind::Unary { op, operand } => {
                let v = self.eval(operand, scope)?;
                Ok(match op {
                    UnOp::Neg => {
                        let (x, d) = v.q();
                        V::Q(-x, d)
                    }
                    UnOp::Not => V::B(!v.b()),
                })
            }
            ExprKind::Binary { op, lhs, rhs } => self.binary(e, *op, lhs, rhs, scope),
            ExprKind::If { cond, then, els } => {
                let (c, sides) = self.predicate(cond, scope)?;
                let detail = match sides {
                    Some((l, r)) => {
                        let op = match &cond.kind {
                            ExprKind::Binary {
                                op: BinOp::Cmp(op), ..
                            } => op.ascii(),
                            _ => unreachable!("sides come from a comparison"),
                        };
                        format!(": {} {op} {}", l.show(), r.show())
                    }
                    None => String::new(),
                };
                let branch = if c { "then" } else { "else" };
                self.line(format!(
                    "if {}{detail}, {c}: take `{branch}`",
                    self.text(cond.span)
                ));
                self.eval(if c { then } else { els }, scope)
            }
            ExprKind::Sum { var, over, body } => {
                let at = self.open();
                let list = self.eval(over, scope)?;
                let V::List(bands, a, b) = list else {
                    unreachable!("type-checked to a list of bands")
                };
                let mut total: Option<(f64, Dim)> = None;
                for band in &bands {
                    scope.sums.push((var.name.clone(), V::Band(*band, a, b)));
                    let term = self.eval(body, scope);
                    scope.sums.pop();
                    let (t, d) = term?.q();
                    self.line(format!(
                        "{} = {}: {}",
                        var.name,
                        band_text(band, a, b),
                        display::quantity(t, d)
                    ));
                    let sum = total.map_or(t, |(s, _)| s + t);
                    total = Some((finite(sum, body.span)?, d));
                }
                // An empty slice sums to zero; its dimension is the body's,
                // which the checker knows but an empty loop never computed.
                let (s, d) = match total {
                    Some(t) => t,
                    None => (0.0, self.empty_sum_dim(&var.name, body, scope, a, b)),
                };
                let text = format!(
                    "sum over {} in {}: {} = {}",
                    var.name,
                    self.text(over.span),
                    self.text(body.span),
                    display::quantity(s, d)
                );
                self.close(at, text);
                Ok(V::Q(s, d))
            }
        }
    }

    /// The dimension of a sum's body, computed on a unit band (whose value
    /// is discarded), for an empty slice.
    fn empty_sum_dim(&mut self, var: &str, body: &Expr, scope: &mut Scope, a: Dim, b: Dim) -> Dim {
        let lines = self.trace.lines.len();
        let depth = self.depth;
        let probe = Band {
            lo: 0.0,
            hi: 1.0,
            value: 1.0,
        };
        scope.sums.push((var.to_string(), V::Band(probe, a, b)));
        let d = self
            .eval(body, scope)
            .map(|v| v.q().1)
            .unwrap_or(Dim::SCALAR);
        scope.sums.pop();
        self.trace.lines.truncate(lines);
        self.depth = depth;
        d
    }

    fn path(&mut self, p: &Path, scope: &mut Scope) -> R<V> {
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
                if let Some(idx) = self.key_index(&prefix, k == 1) {
                    found = Some((idx, k));
                    break;
                }
            }
            let (idx, k) = found.expect("names are resolved at load");
            (self.key(idx, p.span)?, k)
        };
        for seg in &segs[rest..] {
            v = match (v, seg.name.as_str()) {
                (V::Band(b, a, _), "lo") => V::Q(b.lo, a),
                (V::Band(b, a, _), "hi") => V::Q(b.hi, a),
                (V::Band(b, _, d), "value") => V::Q(b.value, d),
                (other, f) => unreachable!("field `{f}` of {other:?} passed the checker"),
            };
        }
        Ok(v)
    }

    fn call(&mut self, e: &Expr, callee: &Path, args: &[Expr], scope: &mut Scope) -> R<V> {
        if callee.segments.len() > 1 {
            return self.bands(e, callee, args, scope);
        }
        let name = callee.segments[0].name.as_str();
        match name {
            "min" | "max" => {
                let mut best: Option<(f64, Dim)> = None;
                for a in args {
                    let (x, d) = self.eval(a, scope)?.q();
                    best = Some(match best {
                        None => (x, d),
                        Some((b, _)) if (name == "min" && x < b) || (name == "max" && x > b) => {
                            (x, d)
                        }
                        Some(b) => b,
                    });
                }
                let (x, d) = best.expect("min and max take two or more arguments");
                Ok(V::Q(x, d))
            }
            "abs" => {
                let (x, d) = self.eval(&args[0], scope)?.q();
                Ok(V::Q(x.abs(), d))
            }
            "sqrt" => {
                let (x, d) = self.eval(&args[0], scope)?.q();
                if x < 0.0 {
                    return Err(fail(
                        code::NEGATIVE_SQRT,
                        args[0].span,
                        format!(
                            "`sqrt` of a negative value: `{}` is {}",
                            self.text(args[0].span),
                            display::quantity(x, d)
                        ),
                    ));
                }
                Ok(V::Q(
                    x.sqrt(),
                    d.sqrt().expect("even exponents, checked at load"),
                ))
            }
            _ => {
                let f = *self.fns.get(name).expect("functions are resolved at load");
                let mut vals = Vec::new();
                for a in args {
                    vals.push(self.eval(a, scope)?);
                }
                let at = self.open();
                let mut inner = Scope::default();
                for (p, v) in f.params.iter().zip(vals) {
                    self.line(format!("{} = {}", p.name.name, v.show()));
                    inner.params.push((p.name.name.clone(), v));
                }
                self.bindings(&f.bindings, &mut inner)?;
                let v = self.eval(&f.value, &mut inner)?;
                let text = format!("{} = {}", self.text(e.span), v.show());
                self.close(at, text);
                Ok(v)
            }
        }
    }

    /// `t.bands(lo, hi)`: the bands of a step table clipped to `[lo, hi]`.
    /// Each entry holds from its key to the next key, the last to infinity,
    /// so a slice starting below the first key is undefined.
    fn bands(&mut self, e: &Expr, callee: &Path, args: &[Expr], scope: &mut Scope) -> R<V> {
        let recv = Path {
            segments: callee.segments[..callee.segments.len() - 1].to_vec(),
            span: callee.span,
        };
        let V::Table(rows, a, b) = self.path(&recv, scope)? else {
            unreachable!("a table, checked at load")
        };
        let (lo, _) = self.eval(&args[0], scope)?.q();
        let (hi, _) = self.eval(&args[1], scope)?.q();
        if lo > hi {
            return Err(fail(
                code::BANDS,
                e.span,
                format!(
                    "`bands` from {} to {}: the lower bound is above the upper",
                    display::quantity(lo, a),
                    display::quantity(hi, a)
                ),
            ));
        }
        if lo < rows[0].0 {
            return Err(fail(
                code::BANDS,
                e.span,
                format!(
                    "`bands` from {}: the table starts at {}, so below that it has no value",
                    display::quantity(lo, a),
                    display::quantity(rows[0].0, a)
                ),
            ));
        }
        let mut out = Vec::new();
        for (i, &(k, v)) in rows.iter().enumerate() {
            let next = rows.get(i + 1).map_or(f64::INFINITY, |r| r.0);
            let (l, h) = (k.max(lo), next.min(hi));
            if l < h {
                out.push(Band {
                    lo: l,
                    hi: h,
                    value: v,
                });
            }
        }
        Ok(V::List(out, a, b))
    }

    fn binary(&mut self, e: &Expr, op: BinOp, lhs: &Expr, rhs: &Expr, scope: &mut Scope) -> R<V> {
        match op {
            // Short-circuit, like `if`: `x > 0 m and 1 m / x > 2` must not
            // divide by zero when the guard is false.
            BinOp::And => {
                if !self.eval(lhs, scope)?.b() {
                    return Ok(V::B(false));
                }
                Ok(V::B(self.eval(rhs, scope)?.b()))
            }
            BinOp::Or => {
                if self.eval(lhs, scope)?.b() {
                    return Ok(V::B(true));
                }
                Ok(V::B(self.eval(rhs, scope)?.b()))
            }
            BinOp::Cmp(c) => {
                let l = self.eval(lhs, scope)?;
                let r = self.eval(rhs, scope)?;
                Ok(V::B(compare(c, &l, &r)))
            }
            _ => {
                let (x, dx) = self.eval(lhs, scope)?.q();
                let (y, dy) = self.eval(rhs, scope)?.q();
                let (v, d) = match op {
                    BinOp::Add => (x + y, dx),
                    BinOp::Sub => (x - y, dx),
                    BinOp::Mul | BinOp::ImplicitMul => (x * y, dx.times(dy)),
                    BinOp::Div => {
                        if y == 0.0 {
                            return Err(fail(
                                code::DIVISION_BY_ZERO,
                                rhs.span,
                                format!("division by zero: `{}` is 0", self.text(rhs.span)),
                            ));
                        }
                        (x / y, dx.per(dy))
                    }
                    BinOp::Pow => {
                        let d = if dx.is_scalar() {
                            Dim::SCALAR
                        } else {
                            dx.powi(y as i32)
                        };
                        (x.powf(y), d)
                    }
                    _ => unreachable!("handled above"),
                };
                Ok(V::Q(finite(v, e.span)?, d))
            }
        }
    }
}

fn compare(op: CmpOp, l: &V, r: &V) -> bool {
    match (l, r) {
        (V::Q(x, _), V::Q(y, _)) => match op {
            CmpOp::Lt => x < y,
            CmpOp::Le => x <= y,
            CmpOp::Gt => x > y,
            CmpOp::Ge => x >= y,
            CmpOp::Eq => x == y,
            CmpOp::Ne => x != y,
        },
        (V::B(x), V::B(y)) => match op {
            CmpOp::Eq => x == y,
            CmpOp::Ne => x != y,
            _ => unreachable!("booleans are ordered nowhere; checked at load"),
        },
        _ => unreachable!("comparison kinds are checked at load"),
    }
}
