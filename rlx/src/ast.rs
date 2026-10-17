//! The syntax tree rlx owns.
//!
//! E0 ruled out borrowing Numbat because its tree is private, and the tree is
//! what three later stages walk: the dimension checker (E3), the tracing
//! evaluator (E4) and the Z3 encoding (E5). So every node carries its span,
//! and nothing here is resolved: units and dimensions are names with
//! exponents, exactly as written, and a name in an expression is a path the
//! later stages look up. The tree does not record spelling (`≤` versus
//! `<=`, parentheses, layout); the formatter works on the source text and
//! needs none of it.

use crate::diag::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

/// A dotted name: `hazard.entry_speed`, `b.hi`, `a_ebd.bands`.
#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    pub segments: Vec<Ident>,
    pub span: Span,
}

impl Path {
    /// The name an expression uses for a key or input: its last segment
    /// (`settle.response` is read as `response`).
    pub fn last(&self) -> &Ident {
        self.segments
            .last()
            .expect("a path has at least one segment")
    }

    pub fn dotted(&self) -> String {
        self.segments
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            .join(".")
    }
}

/// `X.Y.Z`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub span: Span,
}

/// One factor of a unit or dimension product: a name raised to a power. A
/// factor after `/` has its power negated, so `m/s^2` is `m^1 s^-2`.
#[derive(Debug, Clone, PartialEq)]
pub struct Factor {
    pub name: Ident,
    pub power: i32,
    pub span: Span,
}

/// A product of powers of named units (`km/h`, `m/s^2`) or dimensions
/// (`Volume / Pressure`). An empty product is dimensionless (`1 / Time` has
/// one factor, `Time^-1`).
#[derive(Debug, Clone, PartialEq)]
pub struct Product {
    pub factors: Vec<Factor>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct File {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Extension(ExtensionDecl),
    Requires(Requirement),
    Dimension(DimensionDecl),
    Key(KeyDecl),
    Input(InputDecl),
    Derive(DeriveDecl),
    Check(CheckDecl),
    Report(ReportDecl),
    Fn(FnDecl),
}

/// `extension vehicle.braking 1.0.0`
#[derive(Debug, Clone, PartialEq)]
pub struct ExtensionDecl {
    pub name: Path,
    pub version: Version,
    pub span: Span,
}

/// `requires rlm >= 0.2.0` (several constraints separated by commas).
#[derive(Debug, Clone, PartialEq)]
pub struct Requirement {
    pub name: Path,
    pub constraints: Vec<(CmpOp, Version)>,
    pub span: Span,
}

/// `dimension Flow = Volume / Time`
#[derive(Debug, Clone, PartialEq)]
pub struct DimensionDecl {
    pub name: Ident,
    pub value: Product,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum KeyType {
    /// A value of this dimension, written in the contract with a unit.
    Value(Product),
    /// The contract names a node parameter that holds the value.
    /// `magnitude`: the parameter's absolute value is used (a deceleration
    /// written negative). `unit`: the parameter is a plain number in this
    /// unit rather than in SI base units.
    Param {
        dim: Product,
        magnitude: bool,
        unit: Option<Product>,
    },
    /// `table Velocity -> Acceleration`: a step function, each entry holding
    /// from its key up to the next.
    Table { from: Product, to: Product },
}

/// `key hazard.entry_speed : Velocity = default "doc"`
#[derive(Debug, Clone, PartialEq)]
pub struct KeyDecl {
    pub path: Path,
    pub ty: KeyType,
    pub default: Option<Expr>,
    pub doc: Option<String>,
    pub span: Span,
}

/// `input settle.response : Time = 0 s`
#[derive(Debug, Clone, PartialEq)]
pub struct InputDecl {
    pub path: Path,
    pub dim: Product,
    pub default: Option<Expr>,
    pub doc: Option<String>,
    pub span: Span,
}

/// `derive settle.duration : Time = expr` with its clauses.
#[derive(Debug, Clone, PartialEq)]
pub struct DeriveDecl {
    pub target: Path,
    pub dim: Product,
    pub body: Body,
    pub span: Span,
}

/// `check name: predicate` with its clauses.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckDecl {
    pub name: Ident,
    pub body: Body,
    pub span: Span,
}

/// `report name : Dim = expr [where ...]`
#[derive(Debug, Clone, PartialEq)]
pub struct ReportDecl {
    pub name: Ident,
    pub dim: Product,
    pub value: Expr,
    pub bindings: Vec<Binding>,
    pub span: Span,
}

/// `fn name(params) -> Dim = expr [where ...]`. A body is required: a
/// bodyless declaration is how Numbat binds to native code by name, which is
/// how its restricted prelude was escaped (E0).
#[derive(Debug, Clone, PartialEq)]
pub struct FnDecl {
    pub name: Ident,
    pub params: Vec<Param>,
    pub ret: Product,
    pub value: Expr,
    pub bindings: Vec<Binding>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: Ident,
    pub dim: Product,
}

/// The expression of a `derive` or `check` and its clauses, in the order
/// the grammar fixes: `where`, `worst_case`, `domain`.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub value: Expr,
    pub bindings: Vec<Binding>,
    pub worst_case: Vec<Monotone>,
    pub domain: Vec<Constraint>,
}

/// `name = expr` inside `where`.
#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub name: Ident,
    pub value: Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Increasing,
    Decreasing,
}

/// `increasing in a, b`
#[derive(Debug, Clone, PartialEq)]
pub struct Monotone {
    pub direction: Direction,
    pub vars: Vec<Path>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Constraint {
    /// `x in (lo, hi]`
    In {
        var: Path,
        interval: Interval,
        span: Span,
    },
    /// Any other predicate, `alarm_threshold > baseline_pressure`.
    Pred(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Interval {
    pub lo: Expr,
    pub hi: Expr,
    pub lo_closed: bool,
    pub hi_closed: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    /// A bare number. `text` is as written, so a trace can print it the way
    /// the author did.
    Number {
        value: f64,
        text: String,
    },
    /// A number with a unit: `30 km/h`.
    Quantity {
        value: f64,
        text: String,
        unit: Product,
    },
    /// A name: a key, input, parameter, binding or summation variable, or a
    /// field of one (`b.hi`).
    Path(Path),
    /// `min(a, b)`, `sqrt(x)` (also written `√(x)`), `braking_time(...)`,
    /// `a_ebd.bands(lo, hi)`.
    Call {
        callee: Path,
        args: Vec<Expr>,
    },
    Unary {
        op: UnOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    If {
        cond: Box<Expr>,
        then: Box<Expr>,
        els: Box<Expr>,
    },
    /// `sum over b in table_expr: body`
    Sum {
        var: Ident,
        over: Box<Expr>,
        body: Box<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    /// A number juxtaposed with a factor, `2 v0`. Kept apart from `Mul` so a
    /// trace prints it as written.
    ImplicitMul,
    Div,
    Pow,
    Cmp(CmpOp),
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

impl CmpOp {
    pub fn ascii(self) -> &'static str {
        match self {
            CmpOp::Lt => "<",
            CmpOp::Le => "<=",
            CmpOp::Gt => ">",
            CmpOp::Ge => ">=",
            CmpOp::Eq => "==",
            CmpOp::Ne => "!=",
        }
    }
}

impl BinOp {
    pub fn ascii(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::ImplicitMul => "juxt",
            BinOp::Div => "/",
            BinOp::Pow => "^",
            BinOp::Cmp(c) => c.ascii(),
            BinOp::And => "and",
            BinOp::Or => "or",
        }
    }
}

impl Product {
    /// `km h^-1`, the normalized product, for tests and messages.
    pub fn render(&self) -> String {
        self.factors
            .iter()
            .map(|f| {
                if f.power == 1 {
                    f.name.name.clone()
                } else {
                    format!("{}^{}", f.name.name, f.power)
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl Expr {
    /// A fully parenthesized prefix rendering, which makes precedence and
    /// associativity visible in a test assertion.
    pub fn sexpr(&self) -> String {
        match &self.kind {
            ExprKind::Number { text, .. } => text.clone(),
            ExprKind::Quantity { text, unit, .. } => format!("[{text} {}]", unit.render()),
            ExprKind::Path(p) => p.dotted(),
            ExprKind::Call { callee, args } => {
                let mut s = format!("({}", callee.dotted());
                for a in args {
                    s.push(' ');
                    s.push_str(&a.sexpr());
                }
                s.push(')');
                s
            }
            ExprKind::Unary { op, operand } => {
                let o = match op {
                    UnOp::Neg => "neg",
                    UnOp::Not => "not",
                };
                format!("({o} {})", operand.sexpr())
            }
            ExprKind::Binary { op, lhs, rhs } => {
                format!("({} {} {})", op.ascii(), lhs.sexpr(), rhs.sexpr())
            }
            ExprKind::If { cond, then, els } => {
                format!("(if {} {} {})", cond.sexpr(), then.sexpr(), els.sexpr())
            }
            ExprKind::Sum { var, over, body } => {
                format!("(sum {} {} {})", var.name, over.sexpr(), body.sexpr())
            }
        }
    }

    /// Visit this expression and every sub-expression, parents first.
    pub fn walk<'a>(&'a self, f: &mut dyn FnMut(&'a Expr)) {
        f(self);
        match &self.kind {
            ExprKind::Number { .. } | ExprKind::Quantity { .. } | ExprKind::Path(_) => {}
            ExprKind::Call { args, .. } => args.iter().for_each(|a| a.walk(f)),
            ExprKind::Unary { operand, .. } => operand.walk(f),
            ExprKind::Binary { lhs, rhs, .. } => {
                lhs.walk(f);
                rhs.walk(f);
            }
            ExprKind::If { cond, then, els } => {
                cond.walk(f);
                then.walk(f);
                els.walk(f);
            }
            ExprKind::Sum { over, body, .. } => {
                over.walk(f);
                body.walk(f);
            }
        }
    }
}
