//! The load-time checker: names, dimensions and kinds, in every branch.
//!
//! A unit error in a branch no contract reaches is still an error: the
//! branch is what a contract written next year will reach. So the checker
//! walks every expression of every item, whatever an evaluation would
//! take, and refuses the file before anything is evaluated (design 5.2).
//!
//! # Types
//!
//! An expression is a quantity of some [`Dim`], a boolean (a comparison,
//! `and`, `or`, `not`), or one of the three table kinds: a `table A -> B`
//! key, the list of bands `t.bands(lo, hi)` returns, and one band `b` of a
//! `sum over b in ...`, whose fields are `lo` and `hi` (dimension `A`) and
//! `value` (dimension `B`). There are no other methods or fields.
//!
//! # Names
//!
//! A bare name resolves, first match wins, to a `where` binding, a `sum
//! over` variable, a `fn` parameter, then a key or input by the LAST
//! segment of its path (`settle.response` is `response`). The full dotted
//! path resolves to the key too. Two keys or inputs ending in the same name
//! are refused (`rlx-ambiguous-key`), so a bare name never has two
//! meanings. A local may shadow a key, which is the reason for the order,
//! but not another local: a binding named like a parameter or an earlier
//! binding, or a summation variable named like any local, is refused
//! (`rlx-redefinition`), so the order among locals never decides anything.
//! A `where` binding sees the bindings before it; the item's expression
//! sees all of them.
//!
//! Built in to every extension:
//!
//! - functions `min(a, b, ...)` and `max(a, b, ...)` (two or more
//!   arguments of one dimension), `abs(x)`, and `sqrt(x)` (every exponent
//!   of `x` even; the result halves them);
//! - the core input `reaction_total : Time` (design section 2). Declaring
//!   `input reaction_total : Time` is allowed and must agree.
//!
//! # Other rules
//!
//! - Domain constraints see keys and inputs only: a domain is a statement
//!   about what a contract may write, so it cannot mention a binding. An
//!   interval's bounds have the dimension of the name it bounds.
//! - `x ^ n` with a dimensioned `x` needs an integer literal `n` (a unary
//!   minus allowed), since only then is the result's dimension known at
//!   load. A dimensionless base takes any dimensionless exponent.
//! - A `worst_case` name is a key or input that the item reads, directly,
//!   through its `where` bindings, or through the `fn`s it calls; one that
//!   is not read states nothing and is refused, as is a name listed twice.
//! - A `derive` targets one of [`DERIVE_TARGETS`], each at most once per
//!   extension, declared `Time` (design section 4).
//! - `dimension` declarations may come in any order; a cycle among them is
//!   refused, as is redefining a built-in or declared name.
//! - `if` branches agree in kind and dimension; equality (`==`, `!=`)
//!   also compares booleans, the ordering comparisons only quantities.
//! - Errors do not cascade: an expression already reported is not reported
//!   again by the expressions containing it.

use crate::{
    ast::*,
    diag::{self, Diagnostic, Span, code},
    dim::{self, Dim},
    units,
};
use std::collections::{BTreeSet, HashMap};

/// Functions every extension can call.
pub const BUILTIN_FNS: &[&str] = &["min", "max", "abs", "sqrt"];

/// Core values every extension can read without declaring them.
pub const CORE_INPUTS: &[(&str, &str)] = &[("reaction_total", "Time")];

/// The core bounds an extension may derive (design section 4), all times.
pub const DERIVE_TARGETS: &[&str] = &["settle.duration", "hazard.detection", "hazard.ftti"];

/// A key's or input's declared type, resolved.
#[derive(Debug, Clone, PartialEq)]
pub enum KeyTy {
    /// A quantity of this dimension, written with a unit in the contract.
    Value(Dim),
    /// The contract names a node parameter holding the value.
    Param {
        dim: Dim,
        magnitude: bool,
        unit: Option<UnitSpec>,
    },
    /// A step function from `from` to `to`.
    Table { from: Dim, to: Dim },
}

/// `unit U` on a `param` key: the parameter is a plain number in `U`.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitSpec {
    pub text: String,
    pub dim: Dim,
    pub scale: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    Key,
    Input,
    /// A core value read without a declaration (`reaction_total`).
    Core,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KeyInfo {
    /// The full dotted path, `settle.response`.
    pub path: String,
    /// The name an expression uses, `response`.
    pub last: String,
    pub ty: KeyTy,
    pub kind: KeyKind,
    /// The declaration gives a default, so a contract may leave it out.
    pub has_default: bool,
    pub span: Span,
}

/// An item that produces a value: a `derive` by its target path, a
/// `check` or `report` by its name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ItemRef {
    Derive(String),
    Check(String),
    Report(String),
}

/// A key or input an item reads, for the consumer to gather.
#[derive(Debug, Clone, PartialEq)]
pub struct Need {
    pub path: String,
    pub ty: KeyTy,
    pub kind: KeyKind,
    pub has_default: bool,
}

#[derive(Debug, Clone)]
struct FnSig {
    params: Vec<(String, Option<Dim>)>,
    ret: Option<Dim>,
}

/// What a checked extension declares, for the contract value parser and
/// the evaluator.
#[derive(Debug, Clone)]
pub struct Env {
    /// Named dimensions, built-in first then declared, in naming order.
    dims: Vec<(String, Dim)>,
    pub keys: Vec<KeyInfo>,
    fns: HashMap<String, FnSig>,
    /// The keys each item reads (see [`Env::reads`]).
    reads: Vec<(ItemRef, BTreeSet<usize>)>,
}

impl Env {
    /// A named dimension, built-in or declared.
    pub fn dim(&self, name: &str) -> Option<Dim> {
        self.dims.iter().find(|(n, _)| n == name).map(|(_, d)| *d)
    }

    /// The nicest spelling of a dimension: its first name when it has one
    /// (`Velocity`, a declared `Flow`), else the base form (`Length^4`).
    pub fn name_of(&self, d: Dim) -> String {
        self.dims
            .iter()
            .find(|(_, v)| *v == d)
            .map(|(n, _)| n.clone())
            .unwrap_or_else(|| d.render_base())
    }

    /// The keys and inputs an item reads, in declaration order: directly,
    /// through its `where` bindings, through the `fn`s it calls, and in its
    /// `domain`. A key read only by another key's default is not listed.
    /// `None` when the extension has no such item.
    pub fn reads(&self, item: &ItemRef) -> Option<Vec<Need>> {
        let (_, set) = self.reads.iter().find(|(i, _)| i == item)?;
        Some(
            set.iter()
                .map(|&i| {
                    let k = &self.keys[i];
                    Need {
                        path: k.path.clone(),
                        ty: k.ty.clone(),
                        kind: k.kind,
                        has_default: k.has_default,
                    }
                })
                .collect(),
        )
    }

    /// A key or input by its full path or its last segment.
    pub fn key(&self, path: &str) -> Option<&KeyInfo> {
        self.keys
            .iter()
            .find(|k| k.path == path)
            .or_else(|| self.keys.iter().find(|k| k.last == path))
    }
}

/// Check a parsed file. On success, the declarations the contract side
/// needs.
pub fn check(file: &File) -> Result<Env, Vec<Diagnostic>> {
    let mut cx = Cx {
        env: Env {
            dims: dim::BUILTIN
                .iter()
                .map(|(n, v)| (n.to_string(), Dim(*v)))
                .collect(),
            keys: Vec::new(),
            fns: HashMap::new(),
            reads: Vec::new(),
        },
        poisoned_keys: BTreeSet::new(),
        fn_used: HashMap::new(),
        diags: Vec::new(),
    };
    cx.dimensions(file);
    cx.keys(file);
    cx.fn_signatures(file);
    cx.key_defaults(file);
    cx.fn_bodies(file);
    cx.items(file);
    if cx.diags.is_empty() {
        Ok(cx.env)
    } else {
        cx.diags.sort_by_key(|d| d.span.start);
        Err(cx.diags)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Ty {
    Q(Dim),
    Bool,
    Table(Dim, Dim),
    List(Dim, Dim),
    Band(Dim, Dim),
    /// Already reported; suppresses every message that would follow from
    /// it.
    Err,
}

/// Names in scope inside one item.
#[derive(Default)]
struct Locals {
    binds: Vec<(String, Ty)>,
    sums: Vec<(String, Ty)>,
    params: Vec<(String, Ty)>,
    /// Keys and inputs the item reads, by index.
    used: BTreeSet<usize>,
    /// User functions the item calls.
    calls: BTreeSet<String>,
}

impl Locals {
    fn find(&self, name: &str) -> Option<Ty> {
        let lookup =
            |v: &Vec<(String, Ty)>| v.iter().rev().find(|(n, _)| n == name).map(|(_, t)| *t);
        lookup(&self.binds)
            .or_else(|| lookup(&self.sums))
            .or_else(|| lookup(&self.params))
    }
}

struct Cx {
    env: Env,
    /// Keys whose declared type did not resolve; reading one is `Ty::Err`.
    poisoned_keys: BTreeSet<usize>,
    /// Keys each `fn` reads, directly or through the fns it calls.
    fn_used: HashMap<String, BTreeSet<usize>>,
    diags: Vec<Diagnostic>,
}

fn kind(t: Ty) -> &'static str {
    match t {
        Ty::Q(_) => "a quantity",
        Ty::Bool => "a boolean",
        Ty::Table(..) => "a table",
        Ty::List(..) => "a list of bands",
        Ty::Band(..) => "a band",
        Ty::Err => "an error",
    }
}

/// An integer literal exponent, possibly negated.
fn int_literal(e: &Expr) -> Option<i32> {
    match &e.kind {
        ExprKind::Number { value, .. }
            if value.fract() == 0.0 && value.abs() <= i32::MAX as f64 =>
        {
            Some(*value as i32)
        }
        ExprKind::Unary {
            op: UnOp::Neg,
            operand,
        } => int_literal(operand).map(|n| -n),
        _ => None,
    }
}

impl Cx {
    fn err(&mut self, code: &'static str, span: Span, msg: impl Into<String>) {
        let msg = diag::fix_articles(&msg.into());
        self.diags.push(Diagnostic::new(code, span, msg));
    }

    fn name(&self, d: Dim) -> String {
        self.env.name_of(d)
    }

    // -- dimensions ----------------------------------------------------------

    /// Resolve `dimension` declarations in any order (a declaration may use
    /// one written below it), refusing unknown names, cycles and
    /// redefinitions.
    fn dimensions(&mut self, file: &File) {
        let mut decls: Vec<&DimensionDecl> = Vec::new();
        for item in &file.items {
            let Item::Dimension(d) = item else { continue };
            if dim::builtin(&d.name.name).is_some() {
                self.err(
                    code::REDEFINITION,
                    d.name.span,
                    format!(
                        "`{}` is a built-in dimension and cannot be redefined",
                        d.name.name
                    ),
                );
            } else if decls.iter().any(|o| o.name.name == d.name.name) {
                self.err(
                    code::REDEFINITION,
                    d.name.span,
                    format!("dimension `{}` is declared twice", d.name.name),
                );
            } else {
                decls.push(d);
            }
        }
        let mut state: HashMap<String, Option<Dim>> = HashMap::new();
        let mut reported_cycle = BTreeSet::new();
        for d in &decls {
            let mut stack = Vec::new();
            self.resolve_decl(d, &decls, &mut state, &mut stack, &mut reported_cycle);
        }
        // Named in declaration order, after the built-ins.
        for d in &decls {
            if let Some(Some(v)) = state.get(&d.name.name) {
                self.env.dims.push((d.name.name.clone(), *v));
            }
        }
    }

    fn resolve_decl(
        &mut self,
        d: &DimensionDecl,
        decls: &[&DimensionDecl],
        state: &mut HashMap<String, Option<Dim>>,
        stack: &mut Vec<String>,
        reported: &mut BTreeSet<String>,
    ) -> Option<Dim> {
        if let Some(done) = state.get(&d.name.name) {
            return *done;
        }
        if let Some(pos) = stack.iter().position(|n| *n == d.name.name) {
            let cycle: Vec<String> = stack[pos..].to_vec();
            if cycle.iter().all(|n| !reported.contains(n)) {
                reported.extend(cycle.iter().cloned());
                let first = decls
                    .iter()
                    .find(|o| o.name.name == cycle[0])
                    .map_or(d.name.span, |o| o.name.span);
                let names = cycle
                    .iter()
                    .map(|n| format!("`{n}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let path = format!("{} -> {}", cycle.join(" -> "), cycle[0]);
                self.err(
                    code::CYCLE,
                    first,
                    format!("dimensions {names} are defined in terms of each other ({path})"),
                );
            }
            return None;
        }
        stack.push(d.name.name.clone());
        let mut out = Some(Dim::SCALAR);
        for f in &d.value.factors {
            let base = if let Some(b) = dim::builtin(&f.name.name) {
                Some(b)
            } else if let Some(other) = decls.iter().find(|o| o.name.name == f.name.name) {
                self.resolve_decl(other, decls, state, stack, reported)
            } else {
                self.unknown_dimension(&f.name);
                None
            };
            out = match (out, base) {
                (Some(acc), Some(b)) => Some(acc.times(b.powi(f.power))),
                _ => None,
            };
        }
        stack.pop();
        state.insert(d.name.name.clone(), out);
        out
    }

    fn unknown_dimension(&mut self, name: &Ident) {
        self.err(
            code::UNKNOWN_DIMENSION,
            name.span,
            format!(
                "unknown dimension `{}`; declare it (`dimension {} = ...`) or use a built-in \
                 one ({})",
                name.name,
                name.name,
                dim::BUILTIN
                    .iter()
                    .map(|(n, _)| *n)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
    }

    /// A dimension expression in a type position.
    fn resolve_product(&mut self, p: &Product) -> Option<Dim> {
        let mut out = Some(Dim::SCALAR);
        for f in &p.factors {
            let base = self.env.dim(&f.name.name);
            if base.is_none() {
                self.unknown_dimension(&f.name);
            }
            out = match (out, base) {
                (Some(acc), Some(b)) => Some(acc.times(b.powi(f.power))),
                _ => None,
            };
        }
        out
    }

    // -- keys and inputs -----------------------------------------------------

    fn keys(&mut self, file: &File) {
        let mut declared_core = BTreeSet::new();
        for item in &file.items {
            let (path, ty, kind, has_default) = match item {
                Item::Key(k) => {
                    let ty = self.key_type(&k.ty);
                    (&k.path, ty, KeyKind::Key, k.default.is_some())
                }
                Item::Input(i) => (
                    &i.path,
                    self.resolve_product(&i.dim).map(KeyTy::Value),
                    KeyKind::Input,
                    i.default.is_some(),
                ),
                _ => continue,
            };
            let dotted = path.dotted();
            let last = path.last().name.clone();
            // An `input` of a core value must agree with the core.
            if let Some((_, core_dim)) = CORE_INPUTS.iter().find(|(n, _)| *n == dotted)
                && kind == KeyKind::Input
            {
                let want = dim::builtin(core_dim).expect("core dimensions are built in");
                if let Some(KeyTy::Value(d)) = &ty
                    && *d != want
                {
                    self.err(
                        code::DIMENSION,
                        path.span,
                        format!(
                            "the core input `{dotted}` is a {}, declared here as {}",
                            self.name(want),
                            self.name(*d)
                        ),
                    );
                }
                declared_core.insert(dotted.clone());
            }
            if let Some(other) = self.env.keys.iter().find(|o| o.last == last) {
                let msg = if other.path == dotted {
                    (code::REDEFINITION, format!("`{dotted}` is declared twice"))
                } else {
                    (
                        code::AMBIGUOUS_KEY,
                        format!(
                            "`{}` and `{dotted}` both end in `{last}`, so the bare name would \
                             be ambiguous; rename one",
                            other.path
                        ),
                    )
                };
                self.err(msg.0, path.span, msg.1);
                continue;
            }
            let idx = self.env.keys.len();
            if ty.is_none() {
                self.poisoned_keys.insert(idx);
            }
            self.env.keys.push(KeyInfo {
                path: dotted,
                last,
                ty: ty.unwrap_or(KeyTy::Value(Dim::SCALAR)),
                kind,
                has_default,
                span: path.span,
            });
        }
        for (name, d) in CORE_INPUTS {
            if declared_core.contains(*name) {
                continue;
            }
            if let Some(other) = self.env.keys.iter().find(|o| o.last == *name) {
                let (path, span) = (other.path.clone(), other.span);
                self.err(
                    code::AMBIGUOUS_KEY,
                    span,
                    format!(
                        "`{path}` ends in `{name}`, the name of a core input every extension \
                         reads; rename it"
                    ),
                );
                continue;
            }
            self.env.keys.push(KeyInfo {
                path: name.to_string(),
                last: name.to_string(),
                ty: KeyTy::Value(dim::builtin(d).expect("built in")),
                kind: KeyKind::Core,
                has_default: false,
                span: Span::default(),
            });
        }
    }

    fn key_type(&mut self, ty: &KeyType) -> Option<KeyTy> {
        match ty {
            KeyType::Value(p) => self.resolve_product(p).map(KeyTy::Value),
            KeyType::Table { from, to } => {
                let from = self.resolve_product(from);
                let to = self.resolve_product(to);
                Some(KeyTy::Table {
                    from: from?,
                    to: to?,
                })
            }
            KeyType::Param {
                dim,
                magnitude,
                unit,
            } => {
                let d = self.resolve_product(dim)?;
                let unit = match unit {
                    None => None,
                    Some(u) => {
                        let (udim, scale) =
                            units::product(u).expect("the parser accepts only unit symbols");
                        let text = u
                            .factors
                            .iter()
                            .map(|f| f.name.name.as_str())
                            .collect::<Vec<_>>()
                            .join(" ");
                        if udim != d {
                            self.err(
                                code::UNIT,
                                u.span,
                                format!(
                                    "`unit {text}` is a unit of {}, but the parameter is a {}",
                                    self.name(udim),
                                    self.name(d)
                                ),
                            );
                            return None;
                        }
                        Some(UnitSpec {
                            text,
                            dim: udim,
                            scale,
                        })
                    }
                };
                Some(KeyTy::Param {
                    dim: d,
                    magnitude: *magnitude,
                    unit,
                })
            }
        }
    }

    fn key_ty(&self, idx: usize) -> Ty {
        if self.poisoned_keys.contains(&idx) {
            return Ty::Err;
        }
        match &self.env.keys[idx].ty {
            KeyTy::Value(d) | KeyTy::Param { dim: d, .. } => Ty::Q(*d),
            KeyTy::Table { from, to } => Ty::Table(*from, *to),
        }
    }

    fn key_by_last(&self, name: &str) -> Option<usize> {
        self.env.keys.iter().position(|k| k.last == name)
    }

    fn key_by_path(&self, path: &str) -> Option<usize> {
        self.env.keys.iter().position(|k| k.path == path)
    }

    fn key_defaults(&mut self, file: &File) {
        for item in &file.items {
            let (path, default) = match item {
                Item::Key(k) => (&k.path, &k.default),
                Item::Input(i) => (&i.path, &i.default),
                _ => continue,
            };
            let Some(default) = default else { continue };
            let Some(idx) = self.key_by_path(&path.dotted()) else {
                continue;
            };
            let want = self.key_ty(idx);
            let mut loc = Locals::default();
            let got = self.infer(default, &mut loc, None);
            match (want, got) {
                (Ty::Err, _) | (_, Ty::Err) => {}
                (Ty::Q(w), Ty::Q(g)) if w != g => {
                    let msg = format!(
                        "the default of `{}` is a {}, but the key is a {}",
                        path.dotted(),
                        self.name(g),
                        self.name(w)
                    );
                    self.err(code::DIMENSION, default.span, msg);
                }
                (Ty::Q(_), Ty::Q(_)) => {}
                (w, g) => self.err(
                    code::TYPE,
                    default.span,
                    format!(
                        "the default of `{}` is {}, but the key is {}",
                        path.dotted(),
                        kind(g),
                        kind(w)
                    ),
                ),
            }
        }
    }

    // -- functions -----------------------------------------------------------

    fn fn_signatures(&mut self, file: &File) {
        for item in &file.items {
            let Item::Fn(f) = item else { continue };
            if BUILTIN_FNS.contains(&f.name.name.as_str()) {
                self.err(
                    code::REDEFINITION,
                    f.name.span,
                    format!("`{}` is a built-in function", f.name.name),
                );
                continue;
            }
            if self.env.fns.contains_key(&f.name.name) {
                self.err(
                    code::REDEFINITION,
                    f.name.span,
                    format!("fn `{}` is declared twice", f.name.name),
                );
                continue;
            }
            let mut params: Vec<(String, Option<Dim>)> = Vec::new();
            for p in &f.params {
                let d = self.resolve_product(&p.dim);
                if params.iter().any(|(n, _)| *n == p.name.name) {
                    self.err(
                        code::REDEFINITION,
                        p.name.span,
                        format!("parameter `{}` is declared twice", p.name.name),
                    );
                }
                params.push((p.name.name.clone(), d));
            }
            let ret = self.resolve_product(&f.ret);
            self.env
                .fns
                .insert(f.name.name.clone(), FnSig { params, ret });
        }
    }

    fn fn_bodies(&mut self, file: &File) {
        let mut direct: HashMap<String, (BTreeSet<usize>, BTreeSet<String>)> = HashMap::new();
        for item in &file.items {
            let Item::Fn(f) = item else { continue };
            let Some(sig) = self.env.fns.get(&f.name.name).cloned() else {
                continue;
            };
            let mut loc = Locals {
                params: sig
                    .params
                    .iter()
                    .map(|(n, d)| (n.clone(), d.map_or(Ty::Err, Ty::Q)))
                    .collect(),
                ..Locals::default()
            };
            self.bindings(&f.bindings, &mut loc);
            let got = self.infer(&f.value, &mut loc, sig.ret);
            if let (Some(want), Ty::Q(g)) = (sig.ret, got)
                && want != g
            {
                let msg = format!(
                    "fn `{}` returns {}, but its body is {}",
                    f.name.name,
                    self.name(want),
                    self.name(g)
                );
                self.err(code::DIMENSION, f.value.span, msg);
            } else if !matches!(got, Ty::Q(_) | Ty::Err) {
                self.err(
                    code::TYPE,
                    f.value.span,
                    format!(
                        "fn `{}` returns a quantity, but its body is {}",
                        f.name.name,
                        kind(got)
                    ),
                );
            }
            direct.insert(f.name.name.clone(), (loc.used, loc.calls));
        }
        // Close over calls. Recursion was refused before this stage, but the
        // walk keeps a visited set rather than rely on it.
        let names: Vec<String> = direct.keys().cloned().collect();
        for name in names {
            let mut used = BTreeSet::new();
            let mut seen = BTreeSet::new();
            let mut todo = vec![name.clone()];
            while let Some(n) = todo.pop() {
                if !seen.insert(n.clone()) {
                    continue;
                }
                if let Some((u, c)) = direct.get(&n) {
                    used.extend(u.iter().copied());
                    todo.extend(c.iter().cloned());
                }
            }
            self.fn_used.insert(name, used);
        }
    }

    /// `where` bindings, each seeing the ones before it.
    fn bindings(&mut self, bindings: &[Binding], loc: &mut Locals) {
        for b in bindings {
            let ty = self.infer(&b.value, loc, None);
            if loc.binds.iter().any(|(n, _)| *n == b.name.name) {
                self.err(
                    code::REDEFINITION,
                    b.name.span,
                    format!("`{}` is bound twice in this `where`", b.name.name),
                );
            } else if loc.params.iter().any(|(n, _)| *n == b.name.name) {
                self.err(
                    code::REDEFINITION,
                    b.name.span,
                    format!(
                        "`{}` is already a parameter of this fn; a binding cannot reuse it",
                        b.name.name
                    ),
                );
            }
            loc.binds.push((b.name.name.clone(), ty));
        }
    }

    // -- items ---------------------------------------------------------------

    fn items(&mut self, file: &File) {
        let mut targets: Vec<String> = Vec::new();
        let mut named: Vec<String> = Vec::new();
        for item in &file.items {
            match item {
                Item::Derive(d) => {
                    let target = d.target.dotted();
                    let declared = self.resolve_product(&d.dim);
                    let time = dim::builtin("Time").expect("built in");
                    if !DERIVE_TARGETS.contains(&target.as_str()) {
                        self.err(
                            code::DERIVE_TARGET,
                            d.target.span,
                            format!(
                                "`{target}` is not a bound an extension can derive; the \
                                 targets are {} (design section 4)",
                                DERIVE_TARGETS.join(", ")
                            ),
                        );
                    } else if targets.contains(&target) {
                        self.err(
                            code::REDEFINITION,
                            d.target.span,
                            format!("`{target}` is derived twice"),
                        );
                    } else if let Some(dd) = declared
                        && dd != time
                    {
                        let msg = format!(
                            "`{target}` is a Time, but this derive declares {}",
                            self.name(dd)
                        );
                        self.err(code::DIMENSION, d.dim.span, msg);
                    }
                    targets.push(target.clone());
                    let (got, span, used) = self.body(&d.body, declared, "derive");
                    self.env.reads.push((ItemRef::Derive(target.clone()), used));
                    if let (Some(want), Ty::Q(g)) = (declared, got)
                        && want != g
                    {
                        let msg = format!(
                            "derive `{target}` is declared {}, but its body is {}",
                            self.name(want),
                            self.name(g)
                        );
                        self.err(code::DIMENSION, span, msg);
                    } else if !matches!(got, Ty::Q(_) | Ty::Err) {
                        self.err(
                            code::TYPE,
                            span,
                            format!("derive `{target}` needs a quantity, found {}", kind(got)),
                        );
                    }
                }
                Item::Check(c) => {
                    self.unique_name(&c.name, &mut named);
                    let (got, span, used) = self.body(&c.body, None, "check");
                    self.env
                        .reads
                        .push((ItemRef::Check(c.name.name.clone()), used));
                    if !matches!(got, Ty::Bool | Ty::Err) {
                        self.not_boolean(got, span, &format!("check `{}`", c.name.name));
                    }
                }
                Item::Report(r) => {
                    self.unique_name(&r.name, &mut named);
                    let declared = self.resolve_product(&r.dim);
                    let mut loc = Locals::default();
                    self.bindings(&r.bindings, &mut loc);
                    let got = self.infer(&r.value, &mut loc, declared);
                    let used = self.with_fn_reads(&loc);
                    self.env
                        .reads
                        .push((ItemRef::Report(r.name.name.clone()), used));
                    if let (Some(want), Ty::Q(g)) = (declared, got)
                        && want != g
                    {
                        let msg = format!(
                            "report `{}` is declared {}, but its value is {}",
                            r.name.name,
                            self.name(want),
                            self.name(g)
                        );
                        self.err(code::DIMENSION, r.value.span, msg);
                    } else if !matches!(got, Ty::Q(_) | Ty::Err) {
                        self.err(
                            code::TYPE,
                            r.value.span,
                            format!(
                                "report `{}` needs a quantity, found {}",
                                r.name.name,
                                kind(got)
                            ),
                        );
                    }
                }
                _ => {}
            }
        }
    }

    fn unique_name(&mut self, name: &Ident, named: &mut Vec<String>) {
        if named.contains(&name.name) {
            self.err(
                code::REDEFINITION,
                name.span,
                format!("a check or report named `{}` already exists", name.name),
            );
        } else {
            named.push(name.name.clone());
        }
    }

    fn not_boolean(&mut self, got: Ty, span: Span, what: &str) {
        let found = match got {
            Ty::Q(d) => format!("a {}", self.name(d)),
            other => kind(other).to_string(),
        };
        self.err(
            code::NOT_BOOLEAN,
            span,
            format!("{what} must be a boolean (a comparison), found {found}"),
        );
    }

    /// The keys `loc` read, plus those the fns it called read.
    fn with_fn_reads(&self, loc: &Locals) -> BTreeSet<usize> {
        let mut used = loc.used.clone();
        for f in &loc.calls {
            if let Some(u) = self.fn_used.get(f) {
                used.extend(u.iter().copied());
            }
        }
        used
    }

    /// A derive's or check's expression and clauses. Returns its type, the
    /// span of its expression, and every key it reads (the domain's
    /// included, since the domain is checked before evaluating).
    fn body(&mut self, b: &Body, expect: Option<Dim>, what: &str) -> (Ty, Span, BTreeSet<usize>) {
        let mut loc = Locals::default();
        self.bindings(&b.bindings, &mut loc);
        let ty = self.infer(&b.value, &mut loc, expect);
        let mut used = self.with_fn_reads(&loc);
        self.worst_case(&b.worst_case, &b.bindings, &used, what);
        used.extend(self.domain(&b.domain));
        (ty, b.value.span, used)
    }

    fn worst_case(
        &mut self,
        groups: &[Monotone],
        bindings: &[Binding],
        used: &BTreeSet<usize>,
        what: &str,
    ) {
        let mut seen: Vec<(usize, Direction)> = Vec::new();
        for g in groups {
            for p in &g.vars {
                let name = p.dotted();
                if p.segments.len() == 1 && bindings.iter().any(|b| b.name.name == name) {
                    self.err(
                        code::WORST_CASE,
                        p.span,
                        format!(
                            "`{name}` is a `where` binding; a worst-case direction is stated \
                             over keys and inputs, the values a contract supplies"
                        ),
                    );
                    continue;
                }
                let idx = if p.segments.len() == 1 {
                    self.key_by_last(&name)
                } else {
                    self.key_by_path(&name)
                };
                let Some(idx) = idx else {
                    self.err(
                        code::UNKNOWN_NAME,
                        p.span,
                        format!("`{name}` in `worst_case` is not a declared key or input"),
                    );
                    continue;
                };
                if !used.contains(&idx) {
                    self.err(
                        code::WORST_CASE,
                        p.span,
                        format!(
                            "this {what} does not read `{name}`, so a direction over it \
                             states nothing"
                        ),
                    );
                    continue;
                }
                if let Some((_, d)) = seen.iter().find(|(i, _)| *i == idx) {
                    let msg = if *d == g.direction {
                        format!("`{name}` is listed twice in `worst_case`")
                    } else {
                        format!("`{name}` is declared both increasing and decreasing")
                    };
                    self.err(code::WORST_CASE, p.span, msg);
                    continue;
                }
                seen.push((idx, g.direction));
            }
        }
    }

    fn domain(&mut self, constraints: &[Constraint]) -> BTreeSet<usize> {
        let mut read = BTreeSet::new();
        for c in constraints {
            let mut loc = Locals::default();
            match c {
                Constraint::Pred(e) => {
                    let t = self.infer(e, &mut loc, None);
                    if !matches!(t, Ty::Bool | Ty::Err) {
                        self.not_boolean(t, e.span, "a domain constraint");
                    }
                }
                Constraint::In { var, interval, .. } => {
                    let idx = if var.segments.len() == 1 {
                        self.key_by_last(&var.dotted())
                    } else {
                        self.key_by_path(&var.dotted())
                    };
                    let Some(idx) = idx else {
                        self.err(
                            code::UNKNOWN_NAME,
                            var.span,
                            format!(
                                "`{}` in `domain` is not a declared key or input",
                                var.dotted()
                            ),
                        );
                        continue;
                    };
                    read.insert(idx);
                    let want = match self.key_ty(idx) {
                        Ty::Q(d) => d,
                        Ty::Err => continue,
                        other => {
                            self.err(
                                code::TYPE,
                                var.span,
                                format!(
                                    "`{}` is {}; a domain interval bounds a quantity",
                                    var.dotted(),
                                    kind(other)
                                ),
                            );
                            continue;
                        }
                    };
                    for bound in [&interval.lo, &interval.hi] {
                        let got = self.infer(bound, &mut loc, Some(want));
                        if let Some(g) = self.expect_q(got, bound.span, "an interval bound")
                            && g != want
                        {
                            let msg = format!(
                                "the interval bounds `{}`, a {}, but this bound is a {}",
                                var.dotted(),
                                self.name(want),
                                self.name(g)
                            );
                            self.err(code::DIMENSION, bound.span, msg);
                        }
                    }
                }
            }
            read.extend(loc.used);
        }
        read
    }

    // -- expressions ---------------------------------------------------------

    /// A quantity's dimension, or `None` after reporting (or for `Err`).
    fn expect_q(&mut self, t: Ty, span: Span, what: &str) -> Option<Dim> {
        match t {
            Ty::Q(d) => Some(d),
            Ty::Err => None,
            other => {
                self.err(
                    code::TYPE,
                    span,
                    format!("{what} needs a quantity, found {}", kind(other)),
                );
                None
            }
        }
    }

    fn expect_bool(&mut self, t: Ty, span: Span, what: &str) {
        if !matches!(t, Ty::Bool | Ty::Err) {
            self.not_boolean(t, span, what);
        }
    }

    /// Report unless `a` and `b` are one dimension.
    fn same(&mut self, a: Dim, b: Dim, span: Span, what: &str) -> bool {
        if a == b {
            return true;
        }
        let msg = format!("{what}, found {} and {}", self.name(a), self.name(b));
        self.err(code::DIMENSION, span, msg);
        false
    }

    /// `expect` is the dimension the context wants, when it knows one; it
    /// only improves messages (`sqrt` says what its argument should be).
    fn infer(&mut self, e: &Expr, loc: &mut Locals, expect: Option<Dim>) -> Ty {
        match &e.kind {
            ExprKind::Number { .. } => Ty::Q(Dim::SCALAR),
            ExprKind::Quantity { unit, .. } => {
                Ty::Q(units::product(unit).map_or(Dim::SCALAR, |(d, _)| d))
            }
            ExprKind::Path(p) => self.path(p, loc),
            ExprKind::Call { callee, args } => self.call(callee, args, loc, expect),
            ExprKind::Unary { op, operand } => {
                let t = self.infer(operand, loc, expect);
                match op {
                    UnOp::Neg => self
                        .expect_q(t, operand.span, "unary `-`")
                        .map_or(Ty::Err, Ty::Q),
                    UnOp::Not => {
                        self.expect_bool(t, operand.span, "the operand of `not`");
                        Ty::Bool
                    }
                }
            }
            ExprKind::Binary { op, lhs, rhs } => self.binary(e, *op, lhs, rhs, loc, expect),
            ExprKind::If { cond, then, els } => {
                let c = self.infer(cond, loc, None);
                self.expect_bool(c, cond.span, "the condition of `if`");
                let a = self.infer(then, loc, expect);
                let b = self.infer(els, loc, expect);
                match (a, b) {
                    (Ty::Err, t) | (t, Ty::Err) => t,
                    (Ty::Q(x), Ty::Q(y)) => {
                        if self.same(x, y, e.span, "the branches of `if` differ in dimension") {
                            Ty::Q(x)
                        } else {
                            Ty::Err
                        }
                    }
                    (Ty::Bool, Ty::Bool) => Ty::Bool,
                    (x, y) => {
                        self.err(
                            code::TYPE,
                            e.span,
                            format!("the branches of `if` are {} and {}", kind(x), kind(y)),
                        );
                        Ty::Err
                    }
                }
            }
            ExprKind::Sum { var, over, body } => {
                let o = self.infer(over, loc, None);
                let band = match o {
                    Ty::List(a, b) => Ty::Band(a, b),
                    Ty::Err => Ty::Err,
                    other => {
                        self.err(
                            code::TYPE,
                            over.span,
                            format!(
                                "`sum over` ranges over a table's bands (`t.bands(lo, hi)`), \
                                 found {}",
                                kind(other)
                            ),
                        );
                        Ty::Err
                    }
                };
                if loc.find(&var.name).is_some() {
                    self.err(
                        code::REDEFINITION,
                        var.span,
                        format!(
                            "summation variable `{}` reuses the name of a binding, parameter \
                             or enclosing summation variable",
                            var.name
                        ),
                    );
                }
                loc.sums.push((var.name.clone(), band));
                let t = self.infer(body, loc, expect);
                loc.sums.pop();
                self.expect_q(t, body.span, "the body of `sum over`")
                    .map_or(Ty::Err, Ty::Q)
            }
        }
    }

    fn binary(
        &mut self,
        e: &Expr,
        op: BinOp,
        lhs: &Expr,
        rhs: &Expr,
        loc: &mut Locals,
        expect: Option<Dim>,
    ) -> Ty {
        match op {
            BinOp::And | BinOp::Or => {
                let word = if op == BinOp::And { "and" } else { "or" };
                let a = self.infer(lhs, loc, None);
                let b = self.infer(rhs, loc, None);
                self.expect_bool(a, lhs.span, &format!("an operand of `{word}`"));
                self.expect_bool(b, rhs.span, &format!("an operand of `{word}`"));
                Ty::Bool
            }
            BinOp::Pow => {
                let base = self.infer(lhs, loc, None);
                let Some(d) = self.expect_q(base, lhs.span, "`^`") else {
                    return Ty::Err;
                };
                if let Some(n) = int_literal(rhs) {
                    return Ty::Q(d.powi(n));
                }
                let x = self.infer(rhs, loc, None);
                let Some(xd) = self.expect_q(x, rhs.span, "an exponent") else {
                    return Ty::Err;
                };
                if d.is_scalar() && xd.is_scalar() {
                    return Ty::Q(Dim::SCALAR);
                }
                let msg = if xd.is_scalar() {
                    format!(
                        "the base of `^` is a {}, so its exponent must be an integer literal \
                         (`2`, `-1`): only then does the result have one dimension",
                        self.name(d)
                    )
                } else {
                    format!("an exponent must be a Scalar, found {}", self.name(xd))
                };
                self.err(code::EXPONENT, rhs.span, msg);
                Ty::Err
            }
            BinOp::Cmp(c) => {
                let a = self.infer(lhs, loc, None);
                let b = self.infer(rhs, loc, None);
                match (a, b) {
                    (Ty::Err, _) | (_, Ty::Err) => {}
                    (Ty::Q(x), Ty::Q(y)) => {
                        self.same(
                            x,
                            y,
                            e.span,
                            &format!("`{}` compares values of one dimension", c.ascii()),
                        );
                    }
                    (Ty::Bool, Ty::Bool) if matches!(c, CmpOp::Eq | CmpOp::Ne) => {}
                    (x, y) => self.err(
                        code::TYPE,
                        e.span,
                        format!("`{}` cannot compare {} and {}", c.ascii(), kind(x), kind(y)),
                    ),
                }
                Ty::Bool
            }
            BinOp::Add | BinOp::Sub => {
                let sym = if op == BinOp::Add { "+" } else { "-" };
                let a = self.infer(lhs, loc, expect);
                let b = self.infer(rhs, loc, expect);
                let x = self.expect_q(a, lhs.span, &format!("`{sym}`"));
                let y = self.expect_q(b, rhs.span, &format!("`{sym}`"));
                match (x, y) {
                    (Some(x), Some(y)) => {
                        if self.same(
                            x,
                            y,
                            e.span,
                            &format!("`{sym}` needs one dimension on both sides"),
                        ) {
                            Ty::Q(x)
                        } else {
                            Ty::Err
                        }
                    }
                    _ => Ty::Err,
                }
            }
            BinOp::Mul | BinOp::ImplicitMul | BinOp::Div => {
                let a = self.infer(lhs, loc, None);
                let b = self.infer(rhs, loc, None);
                let what = if op == BinOp::Div { "`/`" } else { "`*`" };
                let x = self.expect_q(a, lhs.span, what);
                let y = self.expect_q(b, rhs.span, what);
                match (x, y) {
                    (Some(x), Some(y)) => Ty::Q(if op == BinOp::Div {
                        x.per(y)
                    } else {
                        x.times(y)
                    }),
                    _ => Ty::Err,
                }
            }
        }
    }

    fn path(&mut self, p: &Path, loc: &mut Locals) -> Ty {
        let segs = &p.segments;
        let first = &segs[0].name;
        // A local first, then the longest key path prefix.
        let (mut ty, rest) = if let Some(t) = loc.find(first) {
            (t, 1)
        } else {
            let mut found = None;
            for k in (1..=segs.len()).rev() {
                let prefix = segs[..k]
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join(".");
                let idx = self.key_by_path(&prefix).or_else(|| {
                    if k == 1 {
                        self.key_by_last(&prefix)
                    } else {
                        None
                    }
                });
                if let Some(idx) = idx {
                    found = Some((idx, k));
                    break;
                }
            }
            match found {
                Some((idx, k)) => {
                    loc.used.insert(idx);
                    (self.key_ty(idx), k)
                }
                None => {
                    self.err(
                        code::UNKNOWN_NAME,
                        p.span,
                        format!(
                            "unknown name `{}`: not a `where` binding, summation variable, fn \
                             parameter, declared key or input, or core input",
                            if segs.len() == 1 {
                                first.clone()
                            } else {
                                p.dotted()
                            }
                        ),
                    );
                    return Ty::Err;
                }
            }
        };
        for seg in &segs[rest..] {
            ty = self.field(ty, seg);
        }
        ty
    }

    fn field(&mut self, ty: Ty, seg: &Ident) -> Ty {
        match (ty, seg.name.as_str()) {
            (Ty::Err, _) => Ty::Err,
            (Ty::Band(a, _), "lo" | "hi") => Ty::Q(a),
            (Ty::Band(_, b), "value") => Ty::Q(b),
            (Ty::Band(..), other) => {
                self.err(
                    code::MEMBER,
                    seg.span,
                    format!("a band has the fields `lo`, `hi` and `value`, not `{other}`"),
                );
                Ty::Err
            }
            (Ty::Table(..), "bands") => {
                self.err(
                    code::MEMBER,
                    seg.span,
                    "`bands` is a method of a table: call it, `t.bands(lo, hi)`",
                );
                Ty::Err
            }
            (Ty::Table(..), other) => {
                self.err(
                    code::MEMBER,
                    seg.span,
                    format!("a table has one method, `bands(lo, hi)`, and no field `{other}`"),
                );
                Ty::Err
            }
            (t, other) => {
                self.err(
                    code::MEMBER,
                    seg.span,
                    format!("{} has no field `{other}`", kind(t)),
                );
                Ty::Err
            }
        }
    }

    fn call(&mut self, callee: &Path, args: &[Expr], loc: &mut Locals, expect: Option<Dim>) -> Ty {
        if callee.segments.len() > 1 {
            return self.method(callee, args, loc);
        }
        let name = callee.segments[0].name.as_str();
        match name {
            "min" | "max" => {
                if args.len() < 2 {
                    self.arity(callee, args.len(), "two or more");
                }
                let mut dim: Option<Dim> = None;
                let mut ok = true;
                for a in args {
                    let t = self.infer(a, loc, expect);
                    match self.expect_q(t, a.span, &format!("`{name}`")) {
                        Some(d) => match dim {
                            None => dim = Some(d),
                            Some(prev) => {
                                ok &= self.same(
                                    prev,
                                    d,
                                    a.span,
                                    &format!("`{name}` needs all its arguments in one dimension"),
                                );
                            }
                        },
                        None => ok = false,
                    }
                }
                match (ok, dim) {
                    (true, Some(d)) => Ty::Q(d),
                    _ => Ty::Err,
                }
            }
            "abs" | "sqrt" => {
                if args.len() != 1 {
                    self.arity(callee, args.len(), "one");
                    for a in args {
                        self.infer(a, loc, None);
                    }
                    return Ty::Err;
                }
                let inner_expect = if name == "abs" {
                    expect
                } else {
                    expect.map(|d| d.powi(2))
                };
                let t = self.infer(&args[0], loc, inner_expect);
                let Some(d) = self.expect_q(t, args[0].span, &format!("`{name}`")) else {
                    return Ty::Err;
                };
                if name == "abs" {
                    return Ty::Q(d);
                }
                match d.sqrt() {
                    Some(h) => Ty::Q(h),
                    None => {
                        let hint = match expect {
                            Some(want) => format!(
                                "; the result should be a {}, so the argument should be {}",
                                self.name(want),
                                self.name(want.powi(2))
                            ),
                            None => String::new(),
                        };
                        let msg = format!(
                            "`sqrt` halves every exponent of its argument, which must therefore \
                             all be even, but the argument is {}{hint}",
                            self.name(d)
                        );
                        self.err(code::DIMENSION, args[0].span, msg);
                        Ty::Err
                    }
                }
            }
            _ => {
                let Some(sig) = self.env.fns.get(name).cloned() else {
                    self.err(
                        code::UNKNOWN_NAME,
                        callee.span,
                        format!(
                            "unknown function `{name}`; the built-ins are {}, and any other \
                             function is a `fn` in this file",
                            BUILTIN_FNS.join(", ")
                        ),
                    );
                    for a in args {
                        self.infer(a, loc, None);
                    }
                    return Ty::Err;
                };
                loc.calls.insert(name.to_string());
                if args.len() != sig.params.len() {
                    let n = sig.params.len().to_string();
                    self.arity(callee, args.len(), &n);
                }
                for (i, a) in args.iter().enumerate() {
                    let want = sig.params.get(i).and_then(|(_, d)| *d);
                    let t = self.infer(a, loc, want);
                    let got = self.expect_q(t, a.span, &format!("an argument of `{name}`"));
                    if let (Some(w), Some(g)) = (want, got)
                        && w != g
                    {
                        let msg = format!(
                            "argument {} of `{name}` (`{}`) must be a {}, found a {}",
                            i + 1,
                            sig.params[i].0,
                            self.name(w),
                            self.name(g)
                        );
                        self.err(code::DIMENSION, a.span, msg);
                    }
                }
                sig.ret.map_or(Ty::Err, Ty::Q)
            }
        }
    }

    fn arity(&mut self, callee: &Path, got: usize, want: &str) {
        self.err(
            code::ARITY,
            callee.span,
            format!(
                "`{}` takes {want} argument{}, given {got}",
                callee.dotted(),
                if want == "one" || want == "1" {
                    ""
                } else {
                    "s"
                }
            ),
        );
    }

    /// `t.bands(lo, hi)`, the one method.
    fn method(&mut self, callee: &Path, args: &[Expr], loc: &mut Locals) -> Ty {
        let (method, recv_segs) = callee.segments.split_last().expect("len > 1");
        let recv = Path {
            segments: recv_segs.to_vec(),
            span: recv_segs[0].span.to(recv_segs[recv_segs.len() - 1].span),
        };
        let r = self.path(&recv, loc);
        let (a, b) = match r {
            Ty::Table(a, b) => (a, b),
            Ty::Err => {
                for x in args {
                    self.infer(x, loc, None);
                }
                return Ty::Err;
            }
            other => {
                self.err(
                    code::MEMBER,
                    method.span,
                    format!(
                        "`{}` is {}, which has no methods; only a table has one, `bands`",
                        recv.dotted(),
                        kind(other)
                    ),
                );
                return Ty::Err;
            }
        };
        if method.name != "bands" {
            self.err(
                code::MEMBER,
                method.span,
                format!(
                    "a table has one method, `bands(lo, hi)`, not `{}`",
                    method.name
                ),
            );
            return Ty::Err;
        }
        if args.len() != 2 {
            self.arity(callee, args.len(), "2");
        }
        for x in args {
            let t = self.infer(x, loc, Some(a));
            if let Some(g) = self.expect_q(t, x.span, "an argument of `bands`") {
                self.same(
                    a,
                    g,
                    x.span,
                    "the bounds of `bands` have the dimension of the table's keys",
                );
            }
        }
        Ty::List(a, b)
    }
}

#[cfg(test)]
mod tests {
    use crate::check_source;

    fn codes(src: &str) -> Vec<String> {
        let full = format!("extension t.t 0.1.0\n{src}");
        match check_source(full.as_bytes()) {
            Ok(_) => vec![],
            Err(d) => d
                .iter()
                .map(|d| format!("{}: {}", d.code, d.message))
                .collect(),
        }
    }

    #[test]
    fn clean() {
        assert!(codes("key hazard.v : Velocity\nfn f(a: Time) -> Time = abs(a)\n").is_empty());
        assert!(codes("report r : Scalar = 2^0.5\n").is_empty());
    }

    #[test]
    fn errors_in_untaken_branches_are_reported() {
        let c = codes("fn f(x: Time) -> Time = if 1 > 2 then x else 1 m\n");
        assert_eq!(c.len(), 1, "{c:?}");
        assert!(c[0].starts_with("rlx-dimension"), "{c:?}");
        assert!(c[0].contains("Time and Length"), "{c:?}");
    }

    #[test]
    fn one_error_does_not_cascade() {
        let c = codes("fn f(x: Time) -> Time = (nope + x) * 2 + x\n");
        assert_eq!(c.len(), 1, "{c:?}");
        assert!(c[0].starts_with("rlx-unknown-name"));
    }

    #[test]
    fn locals_and_keys() {
        // A binding may shadow a key, not another local.
        assert!(
            codes("key hazard.v : Velocity\ncheck c: v > 0 m/s\n  where v = 1 m/s\n").is_empty()
        );
        let c = codes("fn f(x: Time) -> Time = y\n  where y = x and x = 1 s\n");
        assert!(c[0].starts_with("rlx-redefinition"), "{c:?}");
        // Full paths resolve.
        assert!(codes("key hazard.v : Velocity\ncheck c: hazard.v > 0 m/s\n").is_empty());
        // The core input agrees, or is refused.
        let c = codes("input reaction_total : Length\n");
        assert!(c[0].starts_with("rlx-dimension"), "{c:?}");
        let c = codes("key hazard.reaction_total : Time\n");
        assert!(c[0].starts_with("rlx-ambiguous-key"), "{c:?}");
    }

    #[test]
    fn worst_case_reads_through_fns() {
        let src = "key hazard.v : Velocity\nkey hazard.w : Velocity\n\
                   fn g(x: Velocity) -> Time = 1 s + 0 s * x / v\n\
                   derive settle.duration : Time = g(1 m/s)\n  worst_case: increasing in v\n";
        assert!(codes(src).is_empty(), "{:?}", codes(src));
        let src = src.replace("increasing in v", "increasing in w");
        let c = codes(&src);
        assert!(c[0].starts_with("rlx-worst-case"), "{c:?}");
    }
}
