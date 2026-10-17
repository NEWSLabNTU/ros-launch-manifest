//! rlx, the extension language of launch manifests: its front end.
//!
//! An extension adds domain vocabulary to a contract (a vehicle's braking
//! profile, a light curtain's safety distance) and derives core bounds from
//! it. The design is `docs/design/extension-system.md`; this crate is unit E2
//! of play_launch phase 91: the character rules, the tokenizer and parser
//! for both layers (the extension format and the expression core), the
//! purely syntactic refusals, and the formatter; and unit E3: dimensions
//! ([`dim`]), units with their scales ([`units`]), the load-time checker
//! ([`typeck`]) and contract values ([`value`]). Evaluation (E4) builds on
//! the same tree, [`ast`].
//!
//! # Pipeline
//!
//! [`check_source`] runs, in order, stopping after the first stage that
//! refuses anything:
//!
//! 1. the file-wide character rules ([`chars::prepass`]): UTF-8, NFC, no
//!    bidi, invisible or control characters anywhere;
//! 2. the tokenizer ([`lexer::lex`]), which also applies the identifier and
//!    symbol rules outside comments and strings;
//! 3. the parser ([`parser::parse`]), first error only;
//! 4. the tree refusals ([`refuse::refuse`]): recursion, and names a
//!    quantity would read as units;
//! 5. the load-time type check ([`typeck::check`]): names, dimensions and
//!    kinds in every branch (E3).
//!
//! [`value`] checks a contract's value against a key's type, with the same
//! lexer and parser, and converts it to SI base units.
//!
//! # Grammar
//!
//! Derived from the six rlx files in `docs/design/extension-examples.md`.
//! Newlines are not significant; `--` starts a comment to the end of the
//! line. Reserved words: `extension requires key input derive check report
//! fn dimension where and or not if then else sum over in worst_case domain
//! increasing decreasing param table`. `magnitude` and `unit` are words only
//! after `param <Dim>`.
//!
//! ```text
//! file       = "extension" path VERSION item*
//! item       = "requires" path cmp VERSION ("," cmp VERSION)*
//!            | "dimension" IDENT "=" dim
//!            | "key" path ":" keytype ["=" expr] [STRING]
//!            | "input" path ":" dim ["=" expr] [STRING]
//!            | "derive" path ":" dim "=" body
//!            | "check" IDENT ":" body
//!            | "report" IDENT ":" dim "=" expr [where]
//!            | "fn" IDENT "(" [param ("," param)*] ")" "->" dim "=" expr [where]
//! keytype    = "param" dim ["magnitude"] ["unit" unit]
//!            | "table" dim "->" dim
//!            | dim
//! param      = IDENT ":" dim
//! body       = expr [where] [worst_case] [domain]
//! where      = "where" IDENT "=" expr ("and" IDENT "=" expr)*
//! worst_case = "worst_case" ":" mono ([";"] mono)*
//! mono       = ("increasing" | "decreasing") "in" path ("," path)*
//! domain     = "domain" ":" constraint ("," constraint)*
//! constraint = path "in" interval | expr
//! interval   = ("(" | "[") expr "," expr (")" | "]")
//! dim        = (IDENT [pow] | "1") (("*" | "/") IDENT [pow])*
//! unit       = UNIT [pow] (("*" | "/") UNIT [pow])*
//! pow        = "^" ["-"] INT
//!
//! expr       = or
//! or         = and ("or" and)*
//! and        = not ("and" not)*          -- not when "and" IDENT "=" follows
//! not        = "not" not | cmp
//! cmp        = add [cmpop add]           -- does not chain
//! add        = mul (("+" | "-") mul)*
//! mul        = unary (("*" | "/") unary)*
//! unary      = "-" unary | implicit
//! implicit   = NUMBER unit               -- a quantity: 30 km/h
//!            | NUMBER "^" exponent
//!            | NUMBER power              -- implicit multiplication: 2 v0
//!            | power
//! power      = postfix ["^" exponent]
//! exponent   = "-" exponent | power      -- right associative
//! postfix    = path ["(" [expr ("," expr)*] ")"]
//!            | NUMBER | "(" expr ")"
//!            | "if" expr "then" expr "else" expr
//!            | "sum" "over" IDENT "in" expr ":" expr
//! ```
//!
//! # Decisions where the examples are silent
//!
//! - **Units versus variables.** `2 v0` multiplies, `30 km` is a quantity,
//!   and only the name distinguishes them. After a bare number, a name on
//!   the unit list ([`units`]) is a unit; any other name is a factor. A
//!   unit continues over `/` and `*` only into another unit name, so in
//!   `2000 mm/s * T` the unit is `mm/s`. The list leaves out single letters
//!   that are common variables (`T`, `C`, ...), and naming a key, input,
//!   parameter, binding or summation variable after a unit is refused
//!   (`rlx-shadow`) so the rule never silently picks the unit.
//! - **Implicit multiplication** is only a bare number followed by a factor
//!   (a name, a call, a parenthesis), once. It binds tighter than `*` and `/`
//!   (`a / 2 j` is `a / (2 j)`, as a physicist reads it) and looser than `^`
//!   (`2 x^2` is `2 (x^2)`). `a b` and `2 3` are errors, so a missing
//!   operator is never read as a product.
//! - **`^`** binds tighter than unary minus (`-a^2` is `-(a^2)`), is right
//!   associative, and its exponent is an atom with an optional sign, never a
//!   quantity or a juxtaposition.
//! - **`if` and `sum over`** take everything to their right (`if c then a
//!   else b + 1` is `if c then a else (b + 1)`); parenthesize to stop them.
//! - **`and`** separates `where` bindings and is also the boolean
//!   conjunction; it is a separator exactly when `name =` follows it.
//! - **Clauses** come in the order `where`, `worst_case`, `domain` (the
//!   order every example uses). `worst_case` groups are separated by `;` or
//!   by simply starting the next `increasing`/`decreasing`; domain
//!   constraints by commas.
//! - **Comparisons do not chain** (`a < b < c` is refused) and equality is
//!   `==`; `=` only binds.
//! - **`√`** is the twin of the name `sqrt` and must be followed directly
//!   by `(`. `²`/`³` are the twins of `^2`/`^3`.
//! - **Versions** are exactly `X.Y.Z`; pre-release tags are not accepted.
//! - **Strings** are one line, with escapes `\"` and `\\` only.
//! - **The file** begins with its one `extension` header; other items come
//!   in any order.
//!
//! # Canonical form
//!
//! See [`fmt`]: ASCII operators, keywords and `sqrt`; comments, strings and
//! identifiers exactly as written; no trailing whitespace; one final line
//! feed. A Greek identifier therefore survives into the canonical form,
//! which is the one place "the canonical form is ASCII" (design 5.3) cannot
//! hold literally.

pub mod ast;
pub mod chars;
pub mod diag;
pub mod dim;
pub mod display;
pub mod eval;
pub mod fmt;
pub mod lexer;
pub mod parser;
pub mod refuse;
pub mod typeck;
pub mod units;
pub mod value;

pub use diag::{Diagnostic, Span};
pub use fmt::Style;

/// A file that passed the syntactic stages (1 to 4 above).
pub struct Syntax<'a> {
    pub source: &'a str,
    pub tokens: Vec<lexer::Token>,
    pub file: ast::File,
    roles: Vec<parser::Role>,
}

impl Syntax<'_> {
    /// The file in `style` (see [`fmt`]).
    pub fn format(&self, style: Style) -> String {
        fmt::format(self.source, &self.tokens, &self.roles, style)
    }
}

/// A file that passed every check, the load-time type check included.
pub struct Checked<'a> {
    pub syntax: Syntax<'a>,
    /// What the extension declares, for contract values and evaluation.
    pub env: typeck::Env,
}

impl Checked<'_> {
    pub fn file(&self) -> &ast::File {
        &self.syntax.file
    }

    pub fn format(&self, style: Style) -> String {
        self.syntax.format(style)
    }
}

/// Run the character rules, tokenizer, parser and tree refusals.
pub fn parse_source(bytes: &[u8]) -> Result<Syntax<'_>, Vec<Diagnostic>> {
    let source = chars::prepass(bytes)?;
    let (tokens, diags) = lexer::lex(source);
    if !diags.is_empty() {
        return Err(diags);
    }
    let parsed = parser::parse(&tokens).map_err(|d| vec![d])?;
    let diags = refuse::refuse(&parsed.file);
    if !diags.is_empty() {
        return Err(diags);
    }
    Ok(Syntax {
        source,
        tokens,
        file: parsed.file,
        roles: parsed.roles,
    })
}

/// Every check: [`parse_source`], then the load-time type check
/// ([`typeck::check`]).
pub fn check_source(bytes: &[u8]) -> Result<Checked<'_>, Vec<Diagnostic>> {
    let syntax = parse_source(bytes)?;
    let env = typeck::check(&syntax.file)?;
    Ok(Checked { syntax, env })
}

/// Format a file. Only the syntactic stages must pass: the roles that
/// decide how a token renders come from the parse, and a file with a
/// dimension error is still worth formatting.
pub fn format_source(bytes: &[u8], style: Style) -> Result<String, Vec<Diagnostic>> {
    Ok(parse_source(bytes)?.format(style))
}
