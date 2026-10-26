//! Contract values read from the launch tree (play_launch phase 94; design
//! `docs/design/contract-values.md`).
//!
//! A quantity of a contract may READ a number instead of restating it:
//!
//! ```text
//! value  := term (('+' | '-') term)*
//! term   := factor (('*' | '/') factor)*
//! factor := number unit? | ref unit | '$(let' name ')' | '(' value ')'
//! ref    := '$(param' [node] name ')' | '$(var' name ['@' node] ')'
//! ```
//!
//! The parser keeps each reference-bearing value as an expression
//! ([`ValueRef`]); the consumer that holds the launch tree evaluates it with
//! a [`Lookup`] and re-parses the contract with the numbers in place
//! ([`resolve_manifest_str`]), so no rule, model field or downstream reader
//! ever sees an expression. Units and dimensions are rlx's ([`rlx::units`],
//! [`rlx::dim`]): one unit table for the extension language and the core
//! grammar.
//!
//! # Decisions where the design is silent
//!
//! - **A count takes a reference without a unit.** "A reference takes a
//!   unit, always" cannot hold for a count (`max_consecutive`), which has
//!   none; there the reference is a plain number. Everywhere else a missing
//!   unit is a parse error.
//! - **`$(let)` takes no unit** after it: the entry carries its own, and a
//!   second one would be a conversion the author did not mean.
//! - **A value outside its key's range** (a negative duration, a rate that
//!   is not positive, a count that is not a whole number) is
//!   `error[value-range]`, naming the value and where it came from.
//! - **A configuration no launch file sets** is `error[var-unset]`.
//! - **The dimension check is the consumer's**, not the parser's: its
//!   message names the value and its source, which only the launch tree
//!   knows, and a wrong dimension must not drop the whole file the way a
//!   grammar error does.

use crate::types::Manifest;
use rlx::dim::Dim;
use serde::Serialize;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Dimensions the core grammar uses
// ---------------------------------------------------------------------------

/// A Time.
pub const TIME: Dim = Dim([0, 0, 1, 0, 0, 0, 0]);
/// A Frequency.
pub const FREQUENCY: Dim = Dim([0, 0, -1, 0, 0, 0, 0]);
/// A plain number (a count).
pub const SCALAR: Dim = Dim::SCALAR;

/// The name messages use for a dimension: the first built-in name of the
/// vector (`Frequency`, not `AngularVelocity`), else its base spelling.
pub fn dim_name(d: Dim) -> String {
    if d.is_scalar() {
        return "plain number".to_string();
    }
    rlx::dim::BUILTIN
        .iter()
        .find(|(_, v)| Dim(*v) == d)
        .map(|(n, _)| n.to_string())
        .unwrap_or_else(|| d.render_base())
}

fn article(word: &str) -> &'static str {
    match word.chars().next() {
        Some('A' | 'E' | 'I' | 'O' | 'U' | 'a' | 'e' | 'i' | 'o' | 'u') => "an",
        _ => "a",
    }
}

/// `a Time`, `an Acceleration`, `a plain number`.
pub fn a_dim(d: Dim) -> String {
    let n = dim_name(d);
    format!("{} {n}", article(&n))
}

// ---------------------------------------------------------------------------
// The expression
// ---------------------------------------------------------------------------

/// A unit as written after a number or a reference, with rlx's dimension and
/// scale to SI.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitText {
    pub text: String,
    pub dim: Dim,
    pub scale: f64,
}

/// Where a two-argument `$(param <node> <name>)` looks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamNode {
    /// One argument: the node the key sits under (`nodes.<n>.…`), or none
    /// when the key is not under a node.
    Here(Option<String>),
    /// A node key of this contract.
    Key(String),
    /// A fully qualified node name anywhere in the launch tree.
    Fqn(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
}

impl Op {
    fn symbol(self) -> &'static str {
        match self {
            Op::Add => "+",
            Op::Sub => "-",
            Op::Mul => "*",
            Op::Div => "/",
        }
    }
}

/// A value expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// A literal, with its unit when written.
    Num { value: f64, unit: Option<UnitText> },
    /// `$(param [node] name) unit`.
    Param {
        node: ParamNode,
        name: String,
        unit: Option<UnitText>,
        /// The reference as written, unit included.
        written: String,
    },
    /// `$(var name [@node]) unit`.
    Var {
        name: String,
        at: Option<String>,
        unit: Option<UnitText>,
        written: String,
    },
    /// `$(let name)`.
    Let(String),
    Bin {
        op: Op,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
}

impl Expr {
    /// Whether the expression reads the launch tree (a `$(param)` or a
    /// `$(var)`), directly. A `$(let)` reads it when its entry does.
    pub fn reads_launch(&self) -> bool {
        match self {
            Expr::Param { .. } | Expr::Var { .. } => true,
            Expr::Num { .. } | Expr::Let(_) => false,
            Expr::Bin { lhs, rhs, .. } => lhs.reads_launch() || rhs.reads_launch(),
        }
    }

    /// The `$(let)` names the expression reads, in order.
    pub fn lets(&self) -> Vec<&str> {
        let mut out = Vec::new();
        fn walk<'a>(e: &'a Expr, out: &mut Vec<&'a str>) {
            match e {
                Expr::Let(n) => out.push(n),
                Expr::Bin { lhs, rhs, .. } => {
                    walk(lhs, out);
                    walk(rhs, out);
                }
                _ => {}
            }
        }
        walk(self, &mut out);
        out
    }

    /// Every `$(param)` and `$(var)` without a unit, as written.
    pub fn unitless_refs(&self) -> Vec<String> {
        let mut out = Vec::new();
        fn walk(e: &Expr, out: &mut Vec<String>) {
            match e {
                Expr::Param {
                    unit: None,
                    written,
                    ..
                }
                | Expr::Var {
                    unit: None,
                    written,
                    ..
                } => out.push(written.clone()),
                Expr::Bin { lhs, rhs, .. } => {
                    walk(lhs, out);
                    walk(rhs, out);
                }
                _ => {}
            }
        }
        walk(self, &mut out);
        out
    }
}

/// Whether a scalar's text is a reference-bearing value: it contains `$(`.
/// A value without one is the literal it always was.
pub fn is_reference(text: &str) -> bool {
    text.contains("$(")
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    /// `$(...)`: the words inside.
    Ref(Vec<String>, String),
    Op(char),
    LParen,
    RParen,
}

fn lex(text: &str) -> Result<Vec<Tok>, String> {
    let b = text.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i] as char;
        if c.is_whitespace() {
            i += 1;
        } else if c == '$' {
            if b.get(i + 1) != Some(&b'(') {
                return Err(format!("`$` at column {} does not start `$(`", i + 1));
            }
            let Some(close) = text[i..].find(')') else {
                return Err(format!(
                    "`{}` is not closed: a reference ends with `)`",
                    &text[i..]
                ));
            };
            let inner = &text[i + 2..i + close];
            let words = inner.split_whitespace().map(str::to_string).collect();
            out.push(Tok::Ref(words, text[i..=i + close].to_string()));
            i += close + 1;
        } else if c.is_ascii_digit() || (c == '.' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            let start = i;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                i += 1;
            }
            // An exponent only when digits follow: `5e3`, `1.0e-6`, never the
            // `e` of a unit.
            if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
                let mut j = i + 1;
                if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
                    j += 1;
                }
                if j < b.len() && b[j].is_ascii_digit() {
                    while j < b.len() && b[j].is_ascii_digit() {
                        j += 1;
                    }
                    i = j;
                }
            }
            let s = &text[start..i];
            let v: f64 = s.parse().map_err(|_| format!("`{s}` is not a number"))?;
            out.push(Tok::Num(v));
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push(Tok::Ident(text[start..i].to_string()));
        } else if matches!(c, '+' | '-' | '*' | '/' | '^') {
            out.push(Tok::Op(c));
            i += 1;
        } else if c == '(' {
            out.push(Tok::LParen);
            i += 1;
        } else if c == ')' {
            out.push(Tok::RParen);
            i += 1;
        } else {
            return Err(format!("`{c}` has no meaning in a value"));
        }
    }
    Ok(out)
}

struct P {
    toks: Vec<Tok>,
    at: usize,
}

fn is_name(s: &str) -> bool {
    let mut c = s.chars();
    c.next()
        .is_some_and(|f| f.is_ascii_alphabetic() || f == '_')
        && c.all(|x| x.is_ascii_alphanumeric() || x == '_' || x == '.')
}

fn is_node(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|x| x.is_ascii_alphanumeric() || matches!(x, '_' | '/' | '-' | '.'))
}

impl P {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.at).cloned();
        self.at += 1;
        t
    }

    fn value(&mut self) -> Result<Expr, String> {
        let mut lhs = self.term()?;
        while let Some(Tok::Op(c @ ('+' | '-'))) = self.peek() {
            let op = if *c == '+' { Op::Add } else { Op::Sub };
            self.at += 1;
            let rhs = self.term()?;
            lhs = Expr::Bin {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn term(&mut self) -> Result<Expr, String> {
        let mut lhs = self.factor()?;
        while let Some(Tok::Op(c @ ('*' | '/'))) = self.peek() {
            let op = if *c == '*' { Op::Mul } else { Op::Div };
            self.at += 1;
            let rhs = self.factor()?;
            lhs = Expr::Bin {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    /// A unit after a number or a reference, when one follows: a unit
    /// symbol, then `/` or `*` continuing only into another unit symbol
    /// (rlx's rule: in `2000 mm/s * T` the unit is `mm/s`).
    fn unit(&mut self) -> Result<Option<UnitText>, String> {
        let Some(Tok::Ident(first)) = self.peek().cloned() else {
            return Ok(None);
        };
        let Some(u) = rlx::units::lookup(&first) else {
            return Err(format!(
                "`{first}` is not a unit (rlx's unit table: {})",
                rlx::units::symbols().collect::<Vec<_>>().join(", ")
            ));
        };
        self.at += 1;
        let mut text = first.clone();
        let power = |p: &mut P, text: &mut String| -> Result<i32, String> {
            if p.peek() != Some(&Tok::Op('^')) {
                return Ok(1);
            }
            p.at += 1;
            let neg = if p.peek() == Some(&Tok::Op('-')) {
                p.at += 1;
                true
            } else {
                false
            };
            match p.next() {
                Some(Tok::Num(n)) if n.fract() == 0.0 && n > 0.0 => {
                    let n = n as i32;
                    text.push_str(&format!("^{}{n}", if neg { "-" } else { "" }));
                    Ok(if neg { -n } else { n })
                }
                _ => Err("a unit's power is a whole number (`m/s^2`)".to_string()),
            }
        };
        let pw = power(self, &mut text)?;
        let mut dim = Dim(u.dim).powi(pw);
        let mut scale = u.scale.powi(pw);
        while let (Some(Tok::Op(c @ ('/' | '*'))), Some(Tok::Ident(next))) =
            (self.toks.get(self.at), self.toks.get(self.at + 1))
        {
            let Some(u2) = rlx::units::lookup(next) else {
                break;
            };
            let div = *c == '/';
            text.push(*c);
            text.push_str(next);
            self.at += 2;
            let mut p2 = power(self, &mut text)?;
            if div {
                p2 = -p2;
            }
            dim = dim.times(Dim(u2.dim).powi(p2));
            scale *= u2.scale.powi(p2);
        }
        Ok(Some(UnitText { text, dim, scale }))
    }

    fn factor(&mut self) -> Result<Expr, String> {
        match self.next() {
            Some(Tok::Op('-')) => match self.next() {
                Some(Tok::Num(v)) => {
                    let unit = self.unit()?;
                    Ok(Expr::Num { value: -v, unit })
                }
                _ => Err("`-` is only a sign on a number here".to_string()),
            },
            Some(Tok::Num(value)) => {
                let unit = self.unit()?;
                Ok(Expr::Num { value, unit })
            }
            Some(Tok::LParen) => {
                let e = self.value()?;
                match self.next() {
                    Some(Tok::RParen) => Ok(e),
                    _ => Err("a `(` is not closed".to_string()),
                }
            }
            Some(Tok::Ref(words, raw)) => self.reference(words, raw),
            Some(Tok::Ident(name)) => Err(format!(
                "`{name}` is a bare name: a value reads a parameter as `$(param {name}) <unit>`, \
                 a launch configuration as `$(var {name}) <unit>`, a contract name as \
                 `$(let {name})`"
            )),
            Some(Tok::Op(c)) => Err(format!("`{c}` where a value belongs")),
            Some(Tok::RParen) => Err("`)` where a value belongs".to_string()),
            None => Err("the value ends where an operand belongs".to_string()),
        }
    }

    fn reference(&mut self, words: Vec<String>, raw: String) -> Result<Expr, String> {
        let w: Vec<&str> = words.iter().map(String::as_str).collect();
        match w.as_slice() {
            ["param", name] | ["param", _, name] if !is_name(name) => Err(format!(
                "`{raw}`: `{name}` is not a parameter name (letters, digits, `_`, dotted \
                 sections)"
            )),
            ["param", name] => {
                let unit = self.unit()?;
                Ok(Expr::Param {
                    node: ParamNode::Here(None),
                    name: name.to_string(),
                    written: written(&raw, &unit),
                    unit,
                })
            }
            ["param", node, name] => {
                if !is_node(node) {
                    return Err(format!("`{raw}`: `{node}` is not a node name"));
                }
                let unit = self.unit()?;
                Ok(Expr::Param {
                    node: if node.starts_with('/') {
                        ParamNode::Fqn(node.to_string())
                    } else {
                        ParamNode::Key(node.to_string())
                    },
                    name: name.to_string(),
                    written: written(&raw, &unit),
                    unit,
                })
            }
            ["var", name] | ["var", name, _] if !is_name(name) => Err(format!(
                "`{raw}`: `{name}` is not a launch configuration name"
            )),
            ["var", name] => {
                let unit = self.unit()?;
                Ok(Expr::Var {
                    name: name.to_string(),
                    at: None,
                    written: written(&raw, &unit),
                    unit,
                })
            }
            ["var", name, at] => {
                let Some(node) = at.strip_prefix('@').filter(|n| is_node(n)) else {
                    return Err(format!(
                        "`{raw}`: the point a configuration is read at is written `@<node>` \
                         (`$(var {name} @gate)`)"
                    ));
                };
                let unit = self.unit()?;
                Ok(Expr::Var {
                    name: name.to_string(),
                    at: Some(node.to_string()),
                    written: written(&raw, &unit),
                    unit,
                })
            }
            ["let", name] => {
                if !is_name(name) {
                    return Err(format!("`{raw}`: `{name}` is not a name"));
                }
                if let Some(Tok::Ident(u)) = self.peek()
                    && rlx::units::is_unit(u)
                {
                    return Err(format!(
                        "`{raw} {u}`: a `$(let)` carries the unit its entry was given; write \
                         `{raw}` alone"
                    ));
                }
                Ok(Expr::Let(name.to_string()))
            }
            [kind, ..] if matches!(*kind, "param" | "var" | "let") => Err(format!(
                "`{raw}` is malformed: write `$(param [<node>] <name>)`, `$(var <name> \
                 [@<node>])` or `$(let <name>)`"
            )),
            _ => Err(format!(
                "`{raw}` is not a reference a contract reads: `$(param …)`, `$(var …)` or \
                 `$(let …)` (`$(env …)` is not read: it differs per host)"
            )),
        }
    }
}

fn written(raw: &str, unit: &Option<UnitText>) -> String {
    match unit {
        Some(u) => format!("{raw} {}", u.text),
        None => raw.to_string(),
    }
}

/// Parse a value expression.
pub fn parse_expr(text: &str) -> Result<Expr, String> {
    let toks = lex(text)?;
    if toks.is_empty() {
        return Err("an empty value".to_string());
    }
    let mut p = P { toks, at: 0 };
    let e = p.value()?;
    if let Some(t) = p.peek() {
        return Err(match t {
            Tok::Ident(u) => format!(
                "`{u}` follows a complete value: a unit only follows a number or a `$(param)`/`$(var)`"
            ),
            Tok::Num(_) | Tok::Ref(..) => "two values with no operator between them".to_string(),
            _ => "the value continues past its end".to_string(),
        });
    }
    Ok(e)
}

/// The static check of one expression against the key it is written for:
/// every `$(param)`/`$(var)` has a unit unless the key is a count, every
/// `$(let)` names an entry before this one.
pub fn check_static(
    expr: &Expr,
    want: Dim,
    known_lets: &[String],
    written: &str,
) -> Result<(), String> {
    if !want.is_scalar()
        && let Some(r) = expr.unitless_refs().first()
    {
        return Err(format!(
            "`{r}` needs a unit: a parameter carries none, and the value is {} \
             (`{r} {}`)",
            a_dim(want),
            rlx::units::example(want)
        ));
    }
    for name in expr.lets() {
        if !known_lets.iter().any(|l| l == name) {
            return Err(format!(
                "`$(let {name})` in `{written}` names no earlier entry of `let:`{}",
                if known_lets.is_empty() {
                    String::new()
                } else {
                    format!(" (entries before it: {})", known_lets.join(", "))
                }
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// What the parser keeps
// ---------------------------------------------------------------------------

/// A `let:` entry (root only), in order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LetDecl {
    pub name: String,
    /// The value as written.
    pub written: String,
    #[serde(skip)]
    pub expr: Expr,
}

/// A reference-bearing value of a contract: where it is and what it reads.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ValueRef {
    /// The dotted YAML path of the scalar (`nodes.n.paths.p.trigger.timer.
    /// rate_hz`, `nodes.n.sub.a.max_age.duration`).
    pub path: String,
    /// The value as written.
    pub written: String,
    #[serde(skip)]
    pub expr: Expr,
    /// The dimension the key takes.
    #[serde(skip)]
    pub want: Dim,
    /// The node key the value sits under (`nodes.<n>.…`), for a one-argument
    /// `$(param)`.
    pub node: Option<String>,
    /// The evaluated value, once a consumer has resolved the contract.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved: Option<Resolved>,
}

impl ValueRef {
    /// The key the value is written for: the path's last segment, or its
    /// parent's for the value of a braces form (`max_age` for
    /// `max_age.duration`).
    pub fn key(&self) -> &str {
        let mut it = self.path.rsplit('.');
        let last = it.next().unwrap_or("");
        match last {
            "duration" | "value" => it.next().unwrap_or(last),
            _ => last,
        }
    }
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

/// What a lookup answers for one reference.
#[derive(Debug, Clone, PartialEq)]
pub enum Read {
    /// The number, where it came from (a source label: `bringup.launch.xml`,
    /// `mrm_handler.param.yaml (/**)`, `command line`), and the node it was
    /// read on (a `$(param)`'s FQN).
    Value {
        raw: f64,
        at: String,
        node: Option<String>,
    },
    /// No launch tree to read: the value is unknown.
    Unknown,
    /// A reference the launch tree cannot answer, with the rule id.
    Error { code: &'static str, message: String },
}

/// Which reference a lookup answers, for its messages.
#[derive(Debug, Clone, Copy)]
pub struct RefAt<'a> {
    /// The dotted path of the value that reads it (`let.<name>` for an
    /// entry of `let:`).
    pub path: &'a str,
    /// The reference as written, unit included (`$(param timeout) s`).
    pub written: &'a str,
}

impl RefAt<'_> {
    /// The path without the value key of a braces form
    /// (`hazards.h.ftti` for `hazards.h.ftti.duration`).
    pub fn key_path(&self) -> &str {
        self.path
            .strip_suffix(".duration")
            .or_else(|| self.path.strip_suffix(".value"))
            .unwrap_or(self.path)
    }
}

/// The launch tree, as references read it. Implemented by the consumer.
pub trait Lookup {
    /// `$(param …)`: the effective value of `name` on `node`.
    fn param(&self, at: &RefAt, node: &ParamNode, name: &str) -> Read;
    /// `$(var …)`: a launch configuration, at the end of the contract's
    /// launch file or, with `node`, where that node was declared.
    fn var(&self, at: &RefAt, name: &str, node: Option<&str>) -> Read;
}

/// No launch tree: every `$(param)` and `$(var)` is unknown. A `$(let)` of
/// literals still evaluates.
pub struct NoLaunchTree;

impl Lookup for NoLaunchTree {
    fn param(&self, _: &RefAt, _: &ParamNode, _: &str) -> Read {
        Read::Unknown
    }
    fn var(&self, _: &RefAt, _: &str, _: Option<&str>) -> Read {
        Read::Unknown
    }
}

/// What one reference read.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadKind {
    Param { node: String, name: String },
    Var { name: String },
    Let { name: String },
}

/// One read of an evaluated value, for provenance.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReadRecord {
    pub kind: ReadKind,
    /// The reference as written (`$(param update_rate) Hz`).
    pub written: String,
    /// The value read, with the unit written (`10 Hz`).
    pub value: String,
    /// Where the value came from.
    pub at: String,
}

/// An evaluated value: SI units, and every read behind it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Resolved {
    pub si: f64,
    #[serde(skip)]
    pub dim: Dim,
    pub reads: Vec<ReadRecord>,
}

/// A value that could not be evaluated.
#[derive(Debug, Clone, PartialEq)]
pub struct ValueError {
    /// The dotted path of the value (`let.<name>` for an entry of `let:`).
    pub path: String,
    /// The rule id (`param-dimension`, `param-unset`, ...).
    pub code: &'static str,
    pub message: String,
}

/// The evaluation of every value of a contract.
#[derive(Debug, Clone, Default)]
pub struct Resolution {
    /// By path: each value that evaluated.
    pub values: BTreeMap<String, Resolved>,
    /// Paths whose value is unknown (no launch tree, or an input that is).
    pub unknown: Vec<String>,
    pub errors: Vec<ValueError>,
}

fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{v:.6}");
        let s = s.trim_end_matches('0').trim_end_matches('.');
        s.to_string()
    }
}

/// A value in a unit for a message: `10 Hz`, `0.5`.
fn shown(raw: f64, unit: &Option<UnitText>) -> String {
    match unit {
        Some(u) => format!("{} {}", fmt_num(raw), u.text),
        None => fmt_num(raw),
    }
}

/// An SI value for a message, in a unit of its dimension.
pub fn shown_si(si: f64, d: Dim) -> String {
    if d == TIME {
        if si.abs() < 1.0 {
            return format!("{} ms", fmt_num(si * 1000.0));
        }
        return format!("{} s", fmt_num(si));
    }
    let u = rlx::units::example(d);
    if u.is_empty() {
        fmt_num(si)
    } else {
        format!("{} {u}", fmt_num(si))
    }
}

/// The static dimension of an expression, or a message naming the operands
/// that do not add.
fn dim_of(e: &Expr, lets: &BTreeMap<String, Dim>) -> Result<Dim, String> {
    Ok(match e {
        Expr::Num { unit, .. } | Expr::Param { unit, .. } | Expr::Var { unit, .. } => {
            unit.as_ref().map_or(Dim::SCALAR, |u| u.dim)
        }
        Expr::Let(n) => *lets
            .get(n)
            .ok_or_else(|| format!("`$(let {n})` has no value"))?,
        Expr::Bin { op, lhs, rhs } => {
            let (a, b) = (dim_of(lhs, lets)?, dim_of(rhs, lets)?);
            match op {
                Op::Mul => a.times(b),
                Op::Div => a.per(b),
                Op::Add | Op::Sub => {
                    if a != b {
                        return Err(format!(
                            "`{}` cannot apply to {} and {}",
                            op.symbol(),
                            a_dim(a),
                            a_dim(b)
                        ));
                    }
                    a
                }
            }
        }
    })
}

/// The evaluator's state over one contract: the `let:` entries evaluated so
/// far.
struct Eval<'a> {
    lookup: &'a dyn Lookup,
    /// The path of the value being evaluated.
    path: std::cell::RefCell<String>,
    lets: BTreeMap<String, Option<Resolved>>,
    let_dims: BTreeMap<String, Dim>,
}

/// `Ok(None)`: unknown.
type Out = Result<Option<(f64, Vec<ReadRecord>)>, (&'static str, String)>;

impl Eval<'_> {
    fn eval(&self, e: &Expr, node: &Option<String>) -> Out {
        match e {
            Expr::Num { value, unit } => Ok(Some((
                value * unit.as_ref().map_or(1.0, |u| u.scale),
                Vec::new(),
            ))),
            Expr::Param {
                node: pn,
                name,
                unit,
                written,
            } => {
                let pn = match pn {
                    ParamNode::Here(None) => ParamNode::Here(node.clone()),
                    other => other.clone(),
                };
                match self.lookup.param(
                    &RefAt {
                        path: &self.path.borrow(),
                        written,
                    },
                    &pn,
                    name,
                ) {
                    Read::Value { raw, at, node } => Ok(Some((
                        raw * unit.as_ref().map_or(1.0, |u| u.scale),
                        vec![ReadRecord {
                            kind: ReadKind::Param {
                                node: node.unwrap_or_default(),
                                name: name.clone(),
                            },
                            written: written.clone(),
                            value: shown(raw, unit),
                            at,
                        }],
                    ))),
                    Read::Unknown => Ok(None),
                    Read::Error { code, message } => Err((code, message)),
                }
            }
            Expr::Var {
                name,
                at,
                unit,
                written,
            } => match self.lookup.var(
                &RefAt {
                    path: &self.path.borrow(),
                    written,
                },
                name,
                at.as_deref(),
            ) {
                Read::Value { raw, at, .. } => Ok(Some((
                    raw * unit.as_ref().map_or(1.0, |u| u.scale),
                    vec![ReadRecord {
                        kind: ReadKind::Var { name: name.clone() },
                        written: written.clone(),
                        value: shown(raw, unit),
                        at,
                    }],
                ))),
                Read::Unknown => Ok(None),
                Read::Error { code, message } => Err((code, message)),
            },
            Expr::Let(n) => match self.lets.get(n) {
                Some(Some(r)) => {
                    let mut reads = vec![ReadRecord {
                        kind: ReadKind::Let { name: n.clone() },
                        written: format!("$(let {n})"),
                        value: shown_si(r.si, r.dim),
                        at: format!("let.{n}"),
                    }];
                    reads.extend(r.reads.iter().cloned());
                    Ok(Some((r.si, reads)))
                }
                _ => Ok(None),
            },
            Expr::Bin { op, lhs, rhs } => {
                let a = self.eval(lhs, node)?;
                let b = self.eval(rhs, node)?;
                let (Some((a, mut ra)), Some((b, rb))) = (a, b) else {
                    return Ok(None);
                };
                ra.extend(rb);
                let v = match op {
                    Op::Add => a + b,
                    Op::Sub => a - b,
                    Op::Mul => a * b,
                    Op::Div => {
                        if b == 0.0 {
                            return Err(("value-range", "the value divides by zero".to_string()));
                        }
                        a / b
                    }
                };
                Ok(Some((v, ra)))
            }
        }
    }

    /// The reads behind an expression that has a value, for a message:
    /// `(10 Hz, mrm_handler.param.yaml)`.
    fn values_note(&self, e: &Expr, node: &Option<String>) -> String {
        match self.eval(e, node) {
            Ok(Some((_, reads))) if !reads.is_empty() => {
                let parts: Vec<String> = reads
                    .iter()
                    .filter(|r| !matches!(r.kind, ReadKind::Let { .. }))
                    .map(|r| format!("{}, {}", r.value, r.at))
                    .collect();
                if parts.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", parts.join("; "))
                }
            }
            _ => String::new(),
        }
    }
}

/// Evaluate one expression for a key of dimension `want`.
fn evaluate_one(
    ev: &Eval,
    expr: &Expr,
    written: &str,
    key: &str,
    want: Dim,
    node: &Option<String>,
) -> Result<Option<Resolved>, (&'static str, String)> {
    let got =
        dim_of(expr, &ev.let_dims).map_err(|m| ("param-dimension", format!("`{written}`: {m}")))?;
    if got != want {
        let inverse = got.powi(-1) == want;
        let suggestion = if inverse {
            let inner = match expr {
                Expr::Bin {
                    op: Op::Add | Op::Sub,
                    ..
                } => format!("({written})"),
                _ => written.to_string(),
            };
            format!(" Did you mean `1 / {inner}`?")
        } else {
            String::new()
        };
        return Err((
            "param-dimension",
            format!(
                "`{key}` is {}, and `{written}` is {}{}.{suggestion}",
                a_dim(want),
                a_dim(got),
                ev.values_note(expr, node)
            ),
        ));
    }
    Ok(ev.eval(expr, node)?.map(|(si, reads)| Resolved {
        si,
        dim: got,
        reads,
    }))
}

/// The range a core key's value must lie in.
fn range_check(r: &Resolved, key: &str, written: &str) -> Result<(), String> {
    let why = if !r.si.is_finite() {
        Some("is not finite")
    } else if r.dim == TIME && r.si < 0.0 {
        Some("is negative")
    } else if r.dim == FREQUENCY && r.si <= 0.0 {
        Some("is not a positive rate")
    } else if r.dim.is_scalar() && (r.si < 0.0 || (r.si - r.si.round()).abs() > 1e-9) {
        Some("is not a whole count")
    } else {
        None
    };
    match why {
        None => Ok(()),
        Some(w) => {
            let from: Vec<String> = r
                .reads
                .iter()
                .filter(|x| !matches!(x.kind, ReadKind::Let { .. }))
                .map(|x| format!("{} = {}, {}", x.written, x.value, x.at))
                .collect();
            Err(format!(
                "`{key}: {written}` evaluates to {}, which {w}{}",
                shown_si(r.si, r.dim),
                if from.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", from.join("; "))
                }
            ))
        }
    }
}

impl Manifest {
    /// Evaluate every reference-bearing value of this contract against
    /// `lookup`: the `let:` entries in order, then each value. The contract
    /// is unchanged; [`resolve_manifest_str`] puts the numbers in place.
    pub fn resolve_values(&self, lookup: &dyn Lookup) -> Resolution {
        let mut ev = Eval {
            lookup,
            path: std::cell::RefCell::new(String::new()),
            lets: BTreeMap::new(),
            let_dims: BTreeMap::new(),
        };
        let mut out = Resolution::default();
        for l in &self.lets {
            let path = format!("let.{}", l.name);
            *ev.path.borrow_mut() = path.clone();
            match dim_of(&l.expr, &ev.let_dims) {
                Ok(d) => {
                    ev.let_dims.insert(l.name.clone(), d);
                    match ev.eval(&l.expr, &None) {
                        Ok(Some((si, reads))) => {
                            ev.lets
                                .insert(l.name.clone(), Some(Resolved { si, dim: d, reads }));
                        }
                        Ok(None) => {
                            ev.lets.insert(l.name.clone(), None);
                            out.unknown.push(path);
                        }
                        Err((code, message)) => {
                            ev.lets.insert(l.name.clone(), None);
                            out.errors.push(ValueError {
                                path,
                                code,
                                message: format!("`let.{}: {}`: {message}", l.name, l.written),
                            });
                        }
                    }
                }
                Err(m) => {
                    ev.lets.insert(l.name.clone(), None);
                    out.errors.push(ValueError {
                        path,
                        code: "param-dimension",
                        message: format!("`let.{}: {}`: {m}", l.name, l.written),
                    });
                }
            }
        }
        for r in &self.value_refs {
            // A `$(let)` whose entry failed already has its error.
            let failed_let = r.expr.lets().iter().any(|n| !ev.let_dims.contains_key(*n));
            if failed_let {
                out.unknown.push(r.path.clone());
                continue;
            }
            *ev.path.borrow_mut() = r.path.clone();
            match evaluate_one(&ev, &r.expr, &r.written, r.key(), r.want, &r.node) {
                Ok(Some(v)) => match range_check(&v, r.key(), &r.written) {
                    Ok(()) => {
                        out.values.insert(r.path.clone(), v);
                    }
                    Err(message) => out.errors.push(ValueError {
                        path: r.path.clone(),
                        code: "value-range",
                        message,
                    }),
                },
                Ok(None) => out.unknown.push(r.path.clone()),
                Err((code, message)) => out.errors.push(ValueError {
                    path: r.path.clone(),
                    code,
                    message,
                }),
            }
        }
        out
    }

    /// Evaluate one value written for a key this crate does not type (an
    /// extension's), against the contract's own `let:` entries and
    /// `lookup`. `node` is the node key the value sits under, for a
    /// one-argument `$(param)`. `Ok(None)`: unknown.
    pub fn evaluate_text(
        &self,
        text: &str,
        key: &str,
        want: Dim,
        node: Option<&str>,
        lookup: &dyn Lookup,
    ) -> Result<Option<Resolved>, ValueError> {
        let err = |code, message| ValueError {
            path: key.to_string(),
            code,
            message,
        };
        let expr = parse_expr(text).map_err(|m| err("value-syntax", format!("`{text}`: {m}")))?;
        let names: Vec<String> = self.lets.iter().map(|l| l.name.clone()).collect();
        check_static(&expr, want, &names, text).map_err(|m| err("value-syntax", m))?;
        let mut ev = Eval {
            lookup,
            path: std::cell::RefCell::new(String::new()),
            lets: BTreeMap::new(),
            let_dims: BTreeMap::new(),
        };
        for l in &self.lets {
            *ev.path.borrow_mut() = format!("let.{}", l.name);
            if let Ok(d) = dim_of(&l.expr, &ev.let_dims) {
                ev.let_dims.insert(l.name.clone(), d);
                let v = ev
                    .eval(&l.expr, &None)
                    .ok()
                    .flatten()
                    .map(|(si, reads)| Resolved { si, dim: d, reads });
                ev.lets.insert(l.name.clone(), v);
            }
        }
        *ev.path.borrow_mut() = key.to_string();
        evaluate_one(&ev, &expr, text, key, want, &node.map(str::to_string))
            .map_err(|(code, m)| err(code, m))
    }
}

// ---------------------------------------------------------------------------
// Two passes
// ---------------------------------------------------------------------------

/// Parse a contract and put the values `lookup` gives in place: the first
/// parse collects the expressions, they are evaluated, and the contract is
/// parsed again with the numbers in the slots they name. Each
/// [`ValueRef`] then carries its [`Resolved`] value. A value that is unknown
/// or could not be evaluated leaves its key absent, as an undeclared one,
/// and the errors are returned for the consumer to report.
pub fn resolve_manifest_str(
    source: &str,
    lookup: &dyn Lookup,
) -> Result<(crate::parse::ParseResult, Resolution), crate::parse::ParseError> {
    let first = crate::parse::parse_manifest_str_with_values(source, None)?;
    if first.value_refs.is_empty() && first.lets.is_empty() {
        return Ok((
            crate::parse::ParseResult {
                manifest: first,
                source: source.to_string(),
                spans: crate::span::SpanIndex::build(source),
            },
            Resolution::default(),
        ));
    }
    let resolution = first.resolve_values(lookup);
    let mut manifest =
        crate::parse::parse_manifest_str_with_values(source, Some(&resolution.values))?;
    for r in &mut manifest.value_refs {
        r.resolved = resolution.values.get(&r.path).cloned();
    }
    Ok((
        crate::parse::ParseResult {
            manifest,
            source: source.to_string(),
            spans: crate::span::SpanIndex::build(source),
        },
        resolution,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(t: &str) -> Expr {
        parse_expr(t).unwrap_or_else(|e| panic!("{t}: {e}"))
    }

    fn lets() -> BTreeMap<String, Dim> {
        BTreeMap::new()
    }

    #[test]
    fn parses_the_grammar() {
        assert!(matches!(
            p("$(param update_rate) Hz"),
            Expr::Param { node: ParamNode::Here(None), ref name, unit: Some(ref u), .. }
                if name == "update_rate" && u.text == "Hz"
        ));
        assert!(matches!(
            p("$(param mrm_handler timeout) s"),
            Expr::Param { node: ParamNode::Key(ref n), .. } if n == "mrm_handler"
        ));
        assert!(matches!(
            p("$(param /safety/lidar_driver rate_hz) Hz"),
            Expr::Param { node: ParamNode::Fqn(ref n), .. } if n == "/safety/lidar_driver"
        ));
        assert!(matches!(
            p("$(var timeout @gate) s"),
            Expr::Var { at: Some(ref a), .. } if a == "gate"
        ));
        assert!(matches!(p("$(let odd_speed)"), Expr::Let(ref n) if n == "odd_speed"));
        // Precedence: `1 / x Hz` is 1/(x Hz), `a + b * c` multiplies first.
        let e = p("2 / $(param update_rate) Hz");
        assert_eq!(dim_of(&e, &lets()).unwrap(), TIME);
        let e = p("$(param target_acceleration) m/s^2");
        assert_eq!(
            dim_of(&e, &lets()).unwrap(),
            rlx::dim::builtin("Acceleration").unwrap()
        );
        let e = p("$(param j) m/s^3 * 2");
        assert_eq!(
            dim_of(&e, &lets()).unwrap(),
            rlx::dim::builtin("Jerk").unwrap()
        );
        let e = p("(500ms + 1 s) / 2");
        assert_eq!(dim_of(&e, &lets()).unwrap(), TIME);
        assert!(matches!(p("1.0e-3 s"), Expr::Num { value, .. } if value == 1.0e-3));
        assert!(matches!(p("-2.5 m/s^2"), Expr::Num { value, .. } if value == -2.5));
    }

    #[test]
    fn refuses_malformed_values() {
        for (text, needle) in [
            ("$(param)", "malformed"),
            ("$(param a b c)", "malformed"),
            ("$(env HOME)", "`$(env …)` is not read"),
            ("$(var x gate)", "`@<node>`"),
            ("$(param x", "not closed"),
            ("$(param x) furlongs", "not a unit"),
            ("$(let a) ms", "carries the unit"),
            ("update_rate", "bare name"),
            ("$(param a) Hz $(param b) Hz", "no operator"),
            ("(1 s", "not closed"),
            ("1 s +", "operand"),
            ("1 s % 2", "no meaning"),
        ] {
            let e = parse_expr(text).unwrap_err();
            assert!(e.contains(needle), "{text}: {e}");
        }
    }

    #[test]
    fn static_checks_units_and_lets() {
        let e = p("$(param timeout)");
        let m = check_static(&e, TIME, &[], "$(param timeout)").unwrap_err();
        assert!(m.contains("needs a unit"), "{m}");
        assert!(m.contains("`$(param timeout) s`"), "{m}");
        // A count takes a bare reference.
        check_static(&e, SCALAR, &[], "$(param timeout)").unwrap();
        let e = p("$(let later)");
        let m = check_static(&e, TIME, &["a".into()], "$(let later)").unwrap_err();
        assert!(m.contains("names no earlier entry"), "{m}");
    }

    struct Tree;
    impl Lookup for Tree {
        fn param(&self, _: &RefAt, node: &ParamNode, name: &str) -> Read {
            match (node, name) {
                (ParamNode::Here(Some(n)), "update_rate") if n == "h" => Read::Value {
                    raw: 10.0,
                    at: "h.param.yaml".into(),
                    node: Some("/h".into()),
                },
                (ParamNode::Here(None), n) => Read::Error {
                    code: "param-node-missing",
                    message: format!("no node for {n}"),
                },
                _ => Read::Error {
                    code: "param-unset",
                    message: format!("{name} unset"),
                },
            }
        }
        fn var(&self, _: &RefAt, name: &str, _: Option<&str>) -> Read {
            match name {
                "lidar_hz" => Read::Value {
                    raw: 100.0,
                    at: "command line".into(),
                    node: None,
                },
                _ => Read::Error {
                    code: "var-unset",
                    message: name.to_string(),
                },
            }
        }
    }

    fn manifest(src: &str) -> Manifest {
        crate::parse_manifest_str(src).unwrap()
    }

    #[test]
    fn evaluates_with_units_and_provenance() {
        let m = manifest(
            "let:\n  rate: $(var lidar_hz) Hz\n  odd: 8.33 m/s\n\
             nodes:\n  h:\n    pub:\n      o: {}\n    sub:\n      i: { max_age: 3 / $(let rate) }\n\
             \x20   paths:\n      t:\n        trigger: { timer: { rate_hz: $(param update_rate) Hz } }\n\
             \x20       output: [o]\n",
        );
        let r = m.resolve_values(&Tree);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        let age = &r.values["nodes.h.sub.i.max_age"];
        assert!((age.si - 0.03).abs() < 1e-12);
        assert!(
            age.reads
                .iter()
                .any(|x| matches!(&x.kind, ReadKind::Var { name } if name == "lidar_hz"))
        );
        assert!(
            age.reads
                .iter()
                .any(|x| matches!(&x.kind, ReadKind::Let { name } if name == "rate"))
        );
        let rate = &r.values["nodes.h.paths.t.trigger.timer.rate_hz"];
        assert_eq!(rate.si, 10.0);
        assert_eq!(rate.reads[0].at, "h.param.yaml");
        assert_eq!(rate.reads[0].value, "10 Hz");
    }

    #[test]
    fn a_wrong_dimension_suggests_the_inverse() {
        let m = manifest("nodes:\n  h:\n    sub:\n      i: { max_age: $(param update_rate) Hz }\n");
        let r = m.resolve_values(&Tree);
        assert_eq!(r.errors.len(), 1, "{:?}", r.errors);
        assert_eq!(r.errors[0].code, "param-dimension");
        assert_eq!(
            r.errors[0].message,
            "`max_age` is a Time, and `$(param update_rate) Hz` is a Frequency (10 Hz, \
             h.param.yaml). Did you mean `1 / $(param update_rate) Hz`?"
        );
        // Not an inverse: no suggestion.
        let m = manifest("nodes:\n  h:\n    sub:\n      i: { max_age: $(param x) m/s }\n");
        let r = m.resolve_values(&NoLaunchTree);
        assert_eq!(r.errors[0].code, "param-dimension");
        assert!(
            !r.errors[0].message.contains("Did you mean"),
            "{}",
            r.errors[0].message
        );
        // Adding two dimensions.
        let m = manifest(
            "let:\n  a: 1 s\nnodes:\n  h:\n    sub:\n      i: { max_age: $(let a) + 2 Hz }\n",
        );
        let r = m.resolve_values(&NoLaunchTree);
        assert_eq!(r.errors[0].code, "param-dimension");
        assert!(
            r.errors[0]
                .message
                .contains("cannot apply to a Time and a Frequency")
        );
    }

    #[test]
    fn lookup_errors_and_unknowns_pass_through() {
        let m = manifest(
            "hazards:\n  z: { guards: [/a], ftti: $(param timeout) s }\n\
             nodes:\n  h:\n    sub:\n      i: { max_age: $(param nope) s }\n",
        );
        let r = m.resolve_values(&Tree);
        let codes: Vec<&str> = r.errors.iter().map(|e| e.code).collect();
        assert_eq!(codes, ["param-unset", "param-node-missing"]);
        let r = m.resolve_values(&NoLaunchTree);
        assert!(r.errors.is_empty());
        assert_eq!(r.unknown.len(), 2);
    }

    #[test]
    fn range_is_checked() {
        let m = manifest(
            "let:\n  a: 1 s\nnodes:\n  h:\n    sub:\n      i: { max_age: $(let a) - 2 s }\n",
        );
        let r = m.resolve_values(&NoLaunchTree);
        assert_eq!(r.errors[0].code, "value-range");
        assert!(
            r.errors[0].message.contains("is negative"),
            "{}",
            r.errors[0].message
        );
    }

    #[test]
    fn evaluates_an_extension_value() {
        let m = manifest("let:\n  a: 2 m/s^2\n");
        let acc = rlx::dim::builtin("Acceleration").unwrap();
        let v = m
            .evaluate_text("$(let a) * 1.5", "decel", acc, None, &NoLaunchTree)
            .unwrap()
            .unwrap();
        assert_eq!(v.si, 3.0);
        let e = m
            .evaluate_text("$(param d) m/s", "decel", acc, Some("n"), &NoLaunchTree)
            .unwrap_err();
        assert_eq!(e.code, "param-dimension");
    }
}
