//! A hand-written recursive-descent parser for both layers.
//!
//! It stops at the first syntax error. Error recovery would let one typo
//! produce a cascade, and an extension is small enough that fixing one error
//! and re-running costs nothing.
//!
//! Besides the tree, the parser records a [`Role`] for each token whose
//! Unicode rendering depends on where it stands (`in` is `∈` only as
//! membership, `/` stays `/` inside a unit), which is the one thing the
//! source-level formatter cannot see from the tokens alone.

use crate::{
    ast::*,
    diag::{Diagnostic, Span, code},
    lexer::{Kw, Tok, Token},
    units,
};

/// What a token means where it stands, for the formatter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Role {
    #[default]
    Plain,
    /// `in` as membership (a domain interval, a `sum over` binder).
    Membership,
    /// An operator inside a unit or dimension product (`km/h`), which keeps
    /// its ASCII spelling in Unicode rendering: `km÷h` reads worse.
    Unit,
    /// `sqrt` as the callee of a call, rendered `√`.
    SqrtCall,
}

pub struct Parsed {
    pub file: File,
    pub roles: Vec<Role>,
}

pub fn parse(tokens: &[Token]) -> Result<Parsed, Diagnostic> {
    let mut p = Parser {
        toks: tokens,
        pos: 0,
        roles: vec![Role::Plain; tokens.len()],
    };
    let file = p.file()?;
    Ok(Parsed {
        file,
        roles: p.roles,
    })
}

type PResult<T> = Result<T, Diagnostic>;

struct Parser<'a> {
    toks: &'a [Token],
    pos: usize,
    roles: Vec<Role>,
}

impl Parser<'_> {
    // -- token plumbing -----------------------------------------------------

    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn peek_at(&self, n: usize) -> &Tok {
        let i = (self.pos + n).min(self.toks.len() - 1);
        &self.toks[i].tok
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span
    }

    /// Span of the token just consumed.
    fn prev_span(&self) -> Span {
        self.toks[self.pos.saturating_sub(1)].span
    }

    fn bump(&mut self) -> &Token {
        let t = &self.toks[self.pos];
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn at(&self, t: &Tok) -> bool {
        self.peek() == t
    }

    fn at_kw(&self, k: Kw) -> bool {
        *self.peek() == Tok::Kw(k)
    }

    fn eat(&mut self, t: &Tok) -> bool {
        if self.at(t) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn eat_kw(&mut self, k: Kw) -> bool {
        self.eat(&Tok::Kw(k))
    }

    fn error(&self, expected: &str) -> Diagnostic {
        Diagnostic::new(
            code::PARSE,
            self.span(),
            format!("expected {expected}, found {}", self.peek().describe()),
        )
    }

    fn expect(&mut self, t: &Tok) -> PResult<Span> {
        if self.at(t) {
            Ok(self.bump().span)
        } else {
            Err(self.error(&format!("`{}`", t.ascii())))
        }
    }

    fn expect_kw(&mut self, k: Kw) -> PResult<Span> {
        if self.at_kw(k) {
            Ok(self.bump().span)
        } else {
            Err(self.error(&format!("`{}`", k.as_str())))
        }
    }

    fn ident(&mut self, what: &str) -> PResult<Ident> {
        match self.peek().clone() {
            Tok::Ident(name) => {
                let span = self.bump().span;
                Ok(Ident { name, span })
            }
            Tok::Kw(k) => Err(Diagnostic::new(
                code::PARSE,
                self.span(),
                format!(
                    "expected {what}, found `{}`, which is a reserved word",
                    k.as_str()
                ),
            )),
            _ => Err(self.error(what)),
        }
    }

    fn path(&mut self, what: &str) -> PResult<Path> {
        let first = self.ident(what)?;
        let mut span = first.span;
        let mut segments = vec![first];
        while self.at(&Tok::Dot) {
            self.bump();
            let seg = self.ident("a name after `.`")?;
            span = span.to(seg.span);
            segments.push(seg);
        }
        Ok(Path { segments, span })
    }

    fn version(&mut self) -> PResult<Version> {
        match self.peek().clone() {
            Tok::Version(text) => {
                let span = self.bump().span;
                let parts: Vec<u64> = text
                    .split('.')
                    .map(|p| p.parse::<u64>())
                    .collect::<Result<_, _>>()
                    .map_err(|_| {
                        Diagnostic::new(code::PARSE, span, format!("version `{text}` is too large"))
                    })?;
                Ok(Version {
                    major: parts[0],
                    minor: parts[1],
                    patch: parts[2],
                    span,
                })
            }
            _ => Err(self.error("a version `X.Y.Z`")),
        }
    }

    // -- items --------------------------------------------------------------

    fn file(&mut self) -> PResult<File> {
        let mut items = Vec::new();
        if !self.at_kw(Kw::Extension) {
            return Err(Diagnostic::new(
                code::PARSE,
                self.span(),
                format!(
                    "an rlx file starts with `extension <name> <version>`, found {}",
                    self.peek().describe()
                ),
            ));
        }
        while !self.at(&Tok::Eof) {
            let start = self.span();
            let item = match self.peek() {
                Tok::Kw(Kw::Extension) => {
                    if !items.is_empty() {
                        return Err(Diagnostic::new(
                            code::PARSE,
                            start,
                            "a file declares one extension, in its first item",
                        ));
                    }
                    self.bump();
                    let name = self.path("the extension's name")?;
                    let version = self.version()?;
                    Item::Extension(ExtensionDecl {
                        name,
                        version,
                        span: start.to(self.prev_span()),
                    })
                }
                Tok::Kw(Kw::Requires) => self.requires()?,
                Tok::Kw(Kw::Dimension) => self.dimension()?,
                Tok::Kw(Kw::Key) => self.key()?,
                Tok::Kw(Kw::Input) => self.input()?,
                Tok::Kw(Kw::Derive) => self.derive()?,
                Tok::Kw(Kw::Check) => self.check()?,
                Tok::Kw(Kw::Report) => self.report()?,
                Tok::Kw(Kw::Fn) => self.fn_decl()?,
                _ => {
                    return Err(self.error(
                        "an item (`requires`, `dimension`, `key`, `input`, `derive`, `check`, \
                         `report` or `fn`)",
                    ));
                }
            };
            items.push(item);
        }
        Ok(File { items })
    }

    fn requires(&mut self) -> PResult<Item> {
        let start = self.bump().span;
        let name = self.path("the name of what is required")?;
        let mut constraints = Vec::new();
        loop {
            let op = self
                .cmp_op()
                .ok_or_else(|| self.error("a comparison (`>=`, `<`, ...)"))?;
            self.bump();
            constraints.push((op, self.version()?));
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        Ok(Item::Requires(Requirement {
            name,
            constraints,
            span: start.to(self.prev_span()),
        }))
    }

    fn dimension(&mut self) -> PResult<Item> {
        let start = self.bump().span;
        let name = self.ident("the dimension's name")?;
        self.expect(&Tok::Eq)?;
        let value = self.dim_product()?;
        Ok(Item::Dimension(DimensionDecl {
            name,
            value,
            span: start.to(self.prev_span()),
        }))
    }

    fn doc(&mut self) -> Option<String> {
        if let Tok::Str(s) = self.peek().clone() {
            self.bump();
            Some(s)
        } else {
            None
        }
    }

    fn key(&mut self) -> PResult<Item> {
        let start = self.bump().span;
        let path = self.path("the key's context path")?;
        self.expect(&Tok::Colon)?;
        let ty = if self.eat_kw(Kw::Param) {
            let dim = self.dim_product()?;
            let magnitude = self.eat_contextual("magnitude");
            let unit = if self.eat_contextual("unit") {
                Some(self.unit_product_required()?)
            } else {
                None
            };
            KeyType::Param {
                dim,
                magnitude,
                unit,
            }
        } else if self.eat_kw(Kw::Table) {
            let from = self.dim_product()?;
            self.expect(&Tok::Arrow)?;
            let to = self.dim_product()?;
            KeyType::Table { from, to }
        } else {
            KeyType::Value(self.dim_product()?)
        };
        let default = if self.eat(&Tok::Eq) {
            Some(self.expr()?)
        } else {
            None
        };
        let doc = self.doc();
        Ok(Item::Key(KeyDecl {
            path,
            ty,
            default,
            doc,
            span: start.to(self.prev_span()),
        }))
    }

    /// `magnitude` and `unit` are words only after `param <Dim>`; reserving
    /// them would take two useful names from every expression.
    fn eat_contextual(&mut self, word: &str) -> bool {
        if matches!(self.peek(), Tok::Ident(s) if s == word) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn input(&mut self) -> PResult<Item> {
        let start = self.bump().span;
        let path = self.path("the input's path")?;
        self.expect(&Tok::Colon)?;
        let dim = self.dim_product()?;
        let default = if self.eat(&Tok::Eq) {
            Some(self.expr()?)
        } else {
            None
        };
        let doc = self.doc();
        Ok(Item::Input(InputDecl {
            path,
            dim,
            default,
            doc,
            span: start.to(self.prev_span()),
        }))
    }

    fn derive(&mut self) -> PResult<Item> {
        let start = self.bump().span;
        let target = self.path("the derived bound's path")?;
        self.expect(&Tok::Colon)?;
        let dim = self.dim_product()?;
        self.expect(&Tok::Eq)?;
        let body = self.body()?;
        Ok(Item::Derive(DeriveDecl {
            target,
            dim,
            body,
            span: start.to(self.prev_span()),
        }))
    }

    fn check(&mut self) -> PResult<Item> {
        let start = self.bump().span;
        let name = self.ident("the check's name")?;
        self.expect(&Tok::Colon)?;
        let body = self.body()?;
        Ok(Item::Check(CheckDecl {
            name,
            body,
            span: start.to(self.prev_span()),
        }))
    }

    fn report(&mut self) -> PResult<Item> {
        let start = self.bump().span;
        let name = self.ident("the report's name")?;
        self.expect(&Tok::Colon)?;
        let dim = self.dim_product()?;
        self.expect(&Tok::Eq)?;
        let value = self.expr()?;
        let bindings = self.where_clause()?;
        Ok(Item::Report(ReportDecl {
            name,
            dim,
            value,
            bindings,
            span: start.to(self.prev_span()),
        }))
    }

    fn fn_decl(&mut self) -> PResult<Item> {
        let start = self.bump().span;
        let name = self.ident("the function's name")?;
        self.expect(&Tok::LParen)?;
        let mut params = Vec::new();
        if !self.at(&Tok::RParen) {
            loop {
                let pname = self.ident("a parameter name")?;
                self.expect(&Tok::Colon)?;
                let dim = self.dim_product()?;
                params.push(Param { name: pname, dim });
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
        }
        self.expect(&Tok::RParen)?;
        self.expect(&Tok::Arrow)?;
        let ret = self.dim_product()?;
        if !self.at(&Tok::Eq) {
            return Err(Diagnostic::new(
                code::PARSE,
                self.span(),
                format!(
                    "expected `=` and the function's body, found {}; every `fn` has a body \
                     written in rlx",
                    self.peek().describe()
                ),
            ));
        }
        self.bump();
        let value = self.expr()?;
        let bindings = self.where_clause()?;
        Ok(Item::Fn(FnDecl {
            name,
            params,
            ret,
            value,
            bindings,
            span: start.to(self.prev_span()),
        }))
    }

    fn body(&mut self) -> PResult<Body> {
        let value = self.expr()?;
        let bindings = self.where_clause()?;
        let worst_case = self.worst_case()?;
        let domain = self.domain()?;
        Ok(Body {
            value,
            bindings,
            worst_case,
            domain,
        })
    }

    fn where_clause(&mut self) -> PResult<Vec<Binding>> {
        let mut out = Vec::new();
        if !self.eat_kw(Kw::Where) {
            return Ok(out);
        }
        loop {
            let name = self.ident("a binding name")?;
            self.expect(&Tok::Eq)?;
            let value = self.expr()?;
            out.push(Binding { name, value });
            if !self.eat_kw(Kw::And) {
                break;
            }
        }
        Ok(out)
    }

    fn worst_case(&mut self) -> PResult<Vec<Monotone>> {
        let mut out = Vec::new();
        if !self.eat_kw(Kw::WorstCase) {
            return Ok(out);
        }
        self.expect(&Tok::Colon)?;
        loop {
            let start = self.span();
            let direction = if self.eat_kw(Kw::Increasing) {
                Direction::Increasing
            } else if self.eat_kw(Kw::Decreasing) {
                Direction::Decreasing
            } else {
                return Err(self.error("`increasing` or `decreasing`"));
            };
            self.expect_kw(Kw::In)?;
            let mut vars = vec![self.path("a name")?];
            while self.eat(&Tok::Comma) {
                vars.push(self.path("a name")?);
            }
            out.push(Monotone {
                direction,
                vars,
                span: start.to(self.prev_span()),
            });
            // Groups are separated by `;` or by starting the next group on
            // its own line; both appear in the examples.
            if self.eat(&Tok::Semi) {
                continue;
            }
            if !(self.at_kw(Kw::Increasing) || self.at_kw(Kw::Decreasing)) {
                break;
            }
        }
        Ok(out)
    }

    fn domain(&mut self) -> PResult<Vec<Constraint>> {
        let mut out = Vec::new();
        if !self.eat_kw(Kw::Domain) {
            return Ok(out);
        }
        self.expect(&Tok::Colon)?;
        loop {
            out.push(self.constraint()?);
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        Ok(out)
    }

    fn constraint(&mut self) -> PResult<Constraint> {
        // `path in <interval>` is told from a predicate by looking past the
        // path for `in`; no predicate contains `in`.
        let save = self.pos;
        if matches!(self.peek(), Tok::Ident(_)) {
            let var = self.path("a name")?;
            if self.at_kw(Kw::In) {
                self.roles[self.pos] = Role::Membership;
                self.bump();
                let interval = self.interval()?;
                let span = var.span.to(interval.span);
                return Ok(Constraint::In {
                    var,
                    interval,
                    span,
                });
            }
            self.pos = save;
        }
        Ok(Constraint::Pred(self.expr()?))
    }

    fn interval(&mut self) -> PResult<Interval> {
        let start = self.span();
        let lo_closed = if self.eat(&Tok::LBracket) {
            true
        } else if self.eat(&Tok::LParen) {
            false
        } else {
            return Err(self.error("an interval, `(lo, hi]` or `[lo, hi]`"));
        };
        let lo = self.expr()?;
        self.expect(&Tok::Comma)?;
        let hi = self.expr()?;
        let hi_closed = if self.eat(&Tok::RBracket) {
            true
        } else if self.eat(&Tok::RParen) {
            false
        } else {
            return Err(self.error("`)` or `]` closing the interval"));
        };
        Ok(Interval {
            lo,
            hi,
            lo_closed,
            hi_closed,
            span: start.to(self.prev_span()),
        })
    }

    // -- products (units and dimensions) -------------------------------------

    fn mark_unit(&mut self) {
        self.roles[self.pos] = Role::Unit;
    }

    /// `^ -? INT` after a factor, or nothing (power 1).
    fn power_suffix(&mut self) -> PResult<i32> {
        if !self.at(&Tok::Caret) {
            return Ok(1);
        }
        self.mark_unit();
        self.bump();
        let neg = if self.at(&Tok::Minus) {
            self.mark_unit();
            self.bump();
            true
        } else {
            false
        };
        match self.peek().clone() {
            Tok::Number(t) if t.bytes().all(|b| b.is_ascii_digit()) => {
                let span = self.bump().span;
                let n: i32 = t.parse().map_err(|_| {
                    Diagnostic::new(code::PARSE, span, format!("exponent `{t}` is too large"))
                })?;
                Ok(if neg { -n } else { n })
            }
            _ => Err(self.error("an integer exponent")),
        }
    }

    /// A dimension: `Velocity`, `Volume / Pressure`, `Length * Time^-1`,
    /// `1 / Time`.
    fn dim_product(&mut self) -> PResult<Product> {
        let start = self.span();
        let mut factors = Vec::new();
        if matches!(self.peek(), Tok::Number(t) if t == "1") && *self.peek_at(1) == Tok::Slash {
            self.bump();
        } else {
            let name = self.ident("a dimension")?;
            let power = self.power_suffix()?;
            let span = name.span.to(self.prev_span());
            factors.push(Factor { name, power, span });
        }
        while matches!(self.peek(), Tok::Star | Tok::Slash) {
            let div = self.at(&Tok::Slash);
            self.mark_unit();
            self.bump();
            let name = self.ident("a dimension")?;
            let power = self.power_suffix()?;
            let span = name.span.to(self.prev_span());
            factors.push(Factor {
                name,
                power: if div { -power } else { power },
                span,
            });
        }
        Ok(Product {
            factors,
            span: start.to(self.prev_span()),
        })
    }

    /// Is the token at `pos + n` a unit symbol standing as a unit? A unit
    /// symbol followed by `(` is a call (`min(a, b)`) and followed by `.` is
    /// a path, so neither is a unit.
    fn unit_at(&self, n: usize) -> bool {
        matches!(self.peek_at(n), Tok::Ident(s) if units::is_unit(s))
            && !matches!(self.peek_at(n + 1), Tok::LParen | Tok::Dot)
    }

    /// A unit after a number: `km/h`, `m/s^2`, `N*m`. It continues over `/`
    /// or `*` only when a unit symbol follows, so in `2000 mm/s * T` the
    /// unit is `mm/s` and `T` is a variable.
    fn unit_product(&mut self) -> PResult<Product> {
        let start = self.span();
        let mut factors = Vec::new();
        let mut div = false;
        loop {
            let name = self.ident("a unit")?;
            let power = self.power_suffix()?;
            let span = name.span.to(self.prev_span());
            factors.push(Factor {
                name,
                power: if div { -power } else { power },
                span,
            });
            if matches!(self.peek(), Tok::Star | Tok::Slash) && self.unit_at(1) {
                div = self.at(&Tok::Slash);
                self.mark_unit();
                self.bump();
            } else {
                break;
            }
        }
        Ok(Product {
            factors,
            span: start.to(self.prev_span()),
        })
    }

    /// `unit <unit>` in a key type, where a unit is required.
    fn unit_product_required(&mut self) -> PResult<Product> {
        if !self.unit_at(0) {
            return Err(self.error("a unit (`mmHg`, `km/h`, ...)"));
        }
        self.unit_product()
    }

    // -- expressions ---------------------------------------------------------
    //
    // Lowest to highest: if / sum (prefix forms whose last operand extends as
    // far as it can), or, and, not, comparison (non-associative), + -, * /,
    // unary minus, implicit multiplication and quantities, ^ (right
    // associative), postfix (call), atoms.

    pub fn expr(&mut self) -> PResult<Expr> {
        let e = self.or()?;
        // Nothing in the grammar puts a name, a number or `(` straight after
        // a complete expression, so this is a missing operator. Saying so
        // beats "expected an item", which is what the caller would report.
        if matches!(self.peek(), Tok::Ident(_) | Tok::Number(_) | Tok::LParen) {
            return Err(Diagnostic::new(
                code::PARSE,
                self.span(),
                format!(
                    "expected an operator before {}; only a number is multiplied by \
                     juxtaposition (`2 v0`), so write `*` between other factors",
                    self.peek().describe()
                ),
            ));
        }
        Ok(e)
    }

    fn binary(lhs: Expr, op: BinOp, rhs: Expr) -> Expr {
        let span = lhs.span.to(rhs.span);
        Expr {
            kind: ExprKind::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            },
            span,
        }
    }

    fn or(&mut self) -> PResult<Expr> {
        let mut lhs = self.and()?;
        while self.eat_kw(Kw::Or) {
            let rhs = self.and()?;
            lhs = Self::binary(lhs, BinOp::Or, rhs);
        }
        Ok(lhs)
    }

    /// `and` followed by `name =` is the next `where` binding, never a
    /// conjunction (`y = z` is not an expression), so `where a = p and b =
    /// q` needs no other separator.
    fn and(&mut self) -> PResult<Expr> {
        let mut lhs = self.not()?;
        while self.at_kw(Kw::And)
            && !(matches!(self.peek_at(1), Tok::Ident(_)) && *self.peek_at(2) == Tok::Eq)
        {
            self.bump();
            let rhs = self.not()?;
            lhs = Self::binary(lhs, BinOp::And, rhs);
        }
        Ok(lhs)
    }

    fn not(&mut self) -> PResult<Expr> {
        if self.at_kw(Kw::Not) {
            let start = self.bump().span;
            let operand = self.not()?;
            let span = start.to(operand.span);
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnOp::Not,
                    operand: Box::new(operand),
                },
                span,
            });
        }
        self.comparison()
    }

    fn cmp_op(&self) -> Option<CmpOp> {
        Some(match self.peek() {
            Tok::Lt => CmpOp::Lt,
            Tok::Le => CmpOp::Le,
            Tok::Gt => CmpOp::Gt,
            Tok::Ge => CmpOp::Ge,
            Tok::EqEq => CmpOp::Eq,
            Tok::Ne => CmpOp::Ne,
            _ => return None,
        })
    }

    fn comparison(&mut self) -> PResult<Expr> {
        let lhs = self.additive()?;
        let Some(op) = self.cmp_op() else {
            return Ok(lhs);
        };
        self.bump();
        let rhs = self.additive()?;
        if self.cmp_op().is_some() {
            return Err(Diagnostic::new(
                code::PARSE,
                self.span(),
                "comparisons do not chain; write `a <= b and b <= c`",
            ));
        }
        Ok(Self::binary(lhs, BinOp::Cmp(op), rhs))
    }

    fn additive(&mut self) -> PResult<Expr> {
        let mut lhs = self.multiplicative()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => break,
            };
            self.bump();
            let rhs = self.multiplicative()?;
            lhs = Self::binary(lhs, op, rhs);
        }
        Ok(lhs)
    }

    fn multiplicative(&mut self) -> PResult<Expr> {
        let mut lhs = self.unary()?;
        loop {
            let op = match self.peek() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                _ => break,
            };
            self.bump();
            let rhs = self.unary()?;
            lhs = Self::binary(lhs, op, rhs);
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> PResult<Expr> {
        if self.at(&Tok::Minus) {
            let start = self.bump().span;
            let operand = self.unary()?;
            let span = start.to(operand.span);
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnOp::Neg,
                    operand: Box::new(operand),
                },
                span,
            });
        }
        self.implicit()
    }

    /// A number, then: a unit (a quantity), `^` (a power of the number), or
    /// the start of a factor (implicit multiplication, `2 v0`, `2 b.value`,
    /// `2 (x + y)`). Only a bare number may be juxtaposed, and only once:
    /// `a b` and `2 3` are errors rather than products, so a missing
    /// operator is never read as multiplication.
    fn implicit(&mut self) -> PResult<Expr> {
        let Tok::Number(text) = self.peek().clone() else {
            return self.power();
        };
        let nspan = self.bump().span;
        let value = parse_number(&text, nspan)?;
        if self.unit_at(0) {
            let unit = self.unit_product()?;
            let span = nspan.to(unit.span);
            return Ok(Expr {
                kind: ExprKind::Quantity { value, text, unit },
                span,
            });
        }
        let num = Expr {
            kind: ExprKind::Number { value, text },
            span: nspan,
        };
        if self.at(&Tok::Caret) {
            return self.power_tail(num);
        }
        if matches!(self.peek(), Tok::Ident(_) | Tok::LParen) {
            let rhs = self.power()?;
            return Ok(Self::binary(num, BinOp::ImplicitMul, rhs));
        }
        Ok(num)
    }

    fn power(&mut self) -> PResult<Expr> {
        let base = self.postfix()?;
        self.power_tail(base)
    }

    /// `^` binds tighter than unary minus on its left (`-a^2` is `-(a^2)`,
    /// as in mathematics) and takes a signed operand on its right (`a^-1`),
    /// right-associatively (`a^b^c` is `a^(b^c)`). The exponent is an atom,
    /// never a juxtaposition: `a^2 m` is an error, not `a^(2 m)`.
    fn power_tail(&mut self, base: Expr) -> PResult<Expr> {
        if !self.eat(&Tok::Caret) {
            return Ok(base);
        }
        let exp = self.exponent()?;
        Ok(Self::binary(base, BinOp::Pow, exp))
    }

    fn exponent(&mut self) -> PResult<Expr> {
        if self.at(&Tok::Minus) {
            let start = self.bump().span;
            let operand = self.exponent()?;
            let span = start.to(operand.span);
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnOp::Neg,
                    operand: Box::new(operand),
                },
                span,
            });
        }
        self.power()
    }

    fn postfix(&mut self) -> PResult<Expr> {
        match self.peek().clone() {
            Tok::Ident(_) => {
                let callee_tok = self.pos;
                let path = self.path("a name")?;
                if self.at(&Tok::LParen) {
                    if path.segments.len() == 1 && path.segments[0].name == "sqrt" {
                        self.roles[callee_tok] = Role::SqrtCall;
                    }
                    self.bump();
                    let mut args = Vec::new();
                    if !self.at(&Tok::RParen) {
                        loop {
                            args.push(self.expr()?);
                            if !self.eat(&Tok::Comma) {
                                break;
                            }
                        }
                    }
                    let end = self.expect(&Tok::RParen)?;
                    let span = path.span.to(end);
                    return Ok(Expr {
                        kind: ExprKind::Call { callee: path, args },
                        span,
                    });
                }
                let span = path.span;
                Ok(Expr {
                    kind: ExprKind::Path(path),
                    span,
                })
            }
            Tok::Number(text) => {
                let span = self.bump().span;
                let value = parse_number(&text, span)?;
                Ok(Expr {
                    kind: ExprKind::Number { value, text },
                    span,
                })
            }
            Tok::LParen => {
                let start = self.bump().span;
                let mut e = self.expr()?;
                let end = self.expect(&Tok::RParen)?;
                e.span = start.to(end);
                Ok(e)
            }
            Tok::Kw(Kw::If) => {
                let start = self.bump().span;
                let cond = self.expr()?;
                self.expect_kw(Kw::Then)?;
                let then = self.expr()?;
                self.expect_kw(Kw::Else)?;
                let els = self.expr()?;
                let span = start.to(els.span);
                Ok(Expr {
                    kind: ExprKind::If {
                        cond: Box::new(cond),
                        then: Box::new(then),
                        els: Box::new(els),
                    },
                    span,
                })
            }
            Tok::Kw(Kw::Sum) => {
                let start = self.bump().span;
                self.expect_kw(Kw::Over)?;
                let var = self.ident("the summation variable")?;
                if self.at_kw(Kw::In) {
                    self.roles[self.pos] = Role::Membership;
                }
                self.expect_kw(Kw::In)?;
                let over = self.expr()?;
                self.expect(&Tok::Colon)?;
                let body = self.expr()?;
                let span = start.to(body.span);
                Ok(Expr {
                    kind: ExprKind::Sum {
                        var,
                        over: Box::new(over),
                        body: Box::new(body),
                    },
                    span,
                })
            }
            _ => Err(self.error("an expression")),
        }
    }
}

fn parse_number(text: &str, span: Span) -> PResult<f64> {
    text.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| {
            Diagnostic::new(
                code::PARSE,
                span,
                format!("number `{text}` is out of range"),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;

    fn expr(src: &str) -> String {
        let (toks, d) = lex(src);
        assert!(d.is_empty(), "{d:?}");
        let mut p = Parser {
            toks: &toks,
            pos: 0,
            roles: vec![Role::Plain; toks.len()],
        };
        let e = p.expr().unwrap_or_else(|d| panic!("{src}: {d}"));
        assert_eq!(*p.peek(), Tok::Eof, "{src}: trailing input");
        e.sexpr()
    }

    fn expr_err(src: &str) -> String {
        let (toks, _) = lex(src);
        let mut p = Parser {
            toks: &toks,
            pos: 0,
            roles: vec![Role::Plain; toks.len()],
        };
        match p.expr() {
            Err(d) => d.message,
            Ok(_) if *p.peek() != Tok::Eof => format!("trailing {}", p.peek().describe()),
            Ok(e) => panic!("{src} parsed as {}", e.sexpr()),
        }
    }

    #[test]
    fn precedence() {
        assert_eq!(expr("a + b * c"), "(+ a (* b c))");
        assert_eq!(expr("a - b - c"), "(- (- a b) c)");
        assert_eq!(expr("-a^2"), "(neg (^ a 2))");
        assert_eq!(expr("a^b^c"), "(^ a (^ b c))");
        assert_eq!(expr("a^-1"), "(^ a (neg 1))");
        assert_eq!(expr("a / b * c"), "(* (/ a b) c)");
        assert_eq!(expr("a < b and not c or d"), "(or (and (< a b) (not c)) d)");
    }

    #[test]
    fn juxtaposition_and_quantities() {
        assert_eq!(expr("2 v0 / j"), "(/ (juxt 2 v0) j)");
        assert_eq!(expr("a / 2 j"), "(/ a (juxt 2 j))");
        assert_eq!(expr("2 b.value"), "(juxt 2 b.value)");
        assert_eq!(expr("2 x^2"), "(juxt 2 (^ x 2))");
        assert_eq!(expr("30 km/h"), "[30 km h^-1]");
        assert_eq!(expr("0.75 m/s^2"), "[0.75 m s^-2]");
        assert_eq!(expr("0.75 m/s²"), "[0.75 m s^-2]");
        assert_eq!(expr("2000 mm/s * T + C"), "(+ (* [2000 mm s^-1] T) C)");
        assert_eq!(expr("5 min(a, b)"), "(juxt 5 (min a b))");
        assert_eq!(expr("8 * (d - 14 mm)"), "(* 8 (- d [14 mm]))");
        assert_eq!(expr("-2 v0"), "(neg (juxt 2 v0))");
        assert_eq!(expr("2^3"), "(^ 2 3)");
    }

    #[test]
    fn prefix_forms_extend_right() {
        assert_eq!(expr("if a > b then x else y + 1"), "(if (> a b) x (+ y 1))");
        assert_eq!(
            expr("t + sum over b in s.bands(0 m/s, v): (b.hi - b.lo) / b.value"),
            "(+ t (sum b (s.bands [0 m s^-1] v) (/ (- b.hi b.lo) b.value)))"
        );
        assert_eq!(expr("√(2 v0 / j)"), "(sqrt (/ (juxt 2 v0) j))");
    }

    #[test]
    fn refusals() {
        assert!(expr_err("a < b < c").contains("do not chain"));
        for src in ["a b", "2 3", "a^2 m", "30 km/h v", "(a)(b)"] {
            assert!(expr_err(src).contains("expected an operator"), "{src}");
        }
    }
}
