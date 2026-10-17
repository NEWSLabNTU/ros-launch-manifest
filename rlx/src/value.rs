//! Contract values against an extension's key types.
//!
//! A contract writes `entry_speed: 30 km/h` for a key an extension declared
//! `key hazard.entry_speed : Velocity`. The value is parsed with the same
//! lexer and expression parser as the extension (so `256ms`, `0.75 m/s^2`
//! and `30 km/h` read identically in both places), must be a literal, and
//! is converted to SI base units. Nothing loads extensions from contracts
//! yet; this is the API the evaluator (E4) and resolution (E7) call.
//!
//! The refusals, each with what to write instead:
//!
//! - a bare number for a dimensioned key (`entry_speed: 15`): the unit is
//!   what makes the number mean something, and guessing one is how a value
//!   ends up off by a factor of 3.6;
//! - a quantity of another dimension (`15 s` for a Velocity), naming both;
//! - a quantity for a `param` key, which takes the NAME of the node
//!   parameter holding the value;
//! - table keys that do not increase strictly.

use crate::{
    ast::{ExprKind, UnOp},
    chars,
    diag::{self, Diagnostic, Span, code},
    dim::Dim,
    lexer,
    parser::parse_expr,
    typeck::{Env, KeyTy},
    units,
};

/// A value in SI base units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quantity {
    pub si: f64,
    pub dim: Dim,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ContractValue {
    Quantity(Quantity),
    /// The name of the node parameter holding the value.
    Param(String),
}

fn first_error(diags: Vec<Diagnostic>) -> Diagnostic {
    diags
        .into_iter()
        .next()
        .expect("an error list is never empty")
}

/// A literal quantity, and whether it was written without a unit.
fn literal(text: &str) -> Result<(Quantity, bool), Diagnostic> {
    let src = chars::prepass(text.as_bytes()).map_err(first_error)?;
    let (tokens, diags) = lexer::lex(src);
    if !diags.is_empty() {
        return Err(first_error(diags));
    }
    let not_a_value = || {
        Diagnostic::new(
            code::VALUE,
            Span::new(0, text.len()),
            format!("`{text}` is not a value; write a number with a unit, such as `30 km/h`"),
        )
    };
    let e = parse_expr(&tokens).map_err(|_| not_a_value())?;
    let mut sign = 1.0;
    let mut e = &e;
    while let ExprKind::Unary {
        op: UnOp::Neg,
        operand,
    } = &e.kind
    {
        sign = -sign;
        e = operand;
    }
    match &e.kind {
        ExprKind::Number { value, .. } => Ok((
            Quantity {
                si: sign * value,
                dim: Dim::SCALAR,
            },
            true,
        )),
        ExprKind::Quantity { value, unit, .. } => {
            let (dim, scale) = units::product(unit).ok_or_else(not_a_value)?;
            Ok((
                Quantity {
                    si: sign * value * scale,
                    dim,
                },
                false,
            ))
        }
        _ => Err(not_a_value()),
    }
}

/// A ROS parameter name: a letter or `_`, then letters, digits, `_`, `.`.
fn is_param_name(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

impl Env {
    /// A quantity of dimension `want`.
    pub fn parse_quantity(&self, text: &str, want: Dim) -> Result<Quantity, Diagnostic> {
        let text = text.trim();
        let (q, bare) = literal(text)?;
        let span = Span::new(0, text.len());
        if bare && !want.is_scalar() {
            return Err(Diagnostic::new(
                code::VALUE,
                span,
                diag::fix_articles(&format!(
                    "`{text}` needs a unit: a {}, e.g. `{text} {}`",
                    self.name_of(want),
                    units::example(want)
                )),
            ));
        }
        if q.dim != want {
            return Err(Diagnostic::new(
                code::DIMENSION,
                span,
                diag::fix_articles(&format!(
                    "`{text}` is a {}, but a {} is expected",
                    self.name_of(q.dim),
                    self.name_of(want)
                )),
            ));
        }
        Ok(q)
    }

    /// A contract value for a key of type `ty` (not a table: see
    /// [`Env::parse_table`]).
    pub fn parse_value(&self, text: &str, ty: &KeyTy) -> Result<ContractValue, Diagnostic> {
        let text = text.trim();
        let span = Span::new(0, text.len());
        match ty {
            KeyTy::Value(d) => self.parse_quantity(text, *d).map(ContractValue::Quantity),
            KeyTy::Param { dim, .. } => {
                if is_param_name(text) {
                    Ok(ContractValue::Param(text.to_string()))
                } else {
                    Err(Diagnostic::new(
                        code::VALUE,
                        span,
                        format!(
                            "this key takes the name of the node parameter holding the {} \
                             (such as `target_acceleration`), found `{text}`",
                            self.name_of(*dim)
                        ),
                    ))
                }
            }
            KeyTy::Table { .. } => Err(Diagnostic::new(
                code::VALUE,
                span,
                "this key is a table and takes a map, `{ 0 km/h: 0.75 m/s^2, ... }`",
            )),
        }
    }

    /// The entries of a `table A -> B` key, as written: `(key, value)`
    /// pairs. Keys must increase strictly, since each entry holds from its
    /// key up to the next.
    pub fn parse_table(
        &self,
        entries: &[(&str, &str)],
        ty: &KeyTy,
    ) -> Result<Vec<(Quantity, Quantity)>, Diagnostic> {
        let KeyTy::Table { from, to } = ty else {
            return Err(Diagnostic::new(
                code::VALUE,
                Span::default(),
                "a map was given, but this key is not a table",
            ));
        };
        if entries.is_empty() {
            return Err(Diagnostic::new(
                code::VALUE,
                Span::default(),
                "a table needs at least one entry",
            ));
        }
        let context = |k: &str, mut d: Diagnostic| {
            d.message = format!("table entry `{k}`: {}", d.message);
            d
        };
        let mut out: Vec<(Quantity, Quantity)> = Vec::new();
        for &(k, v) in entries {
            let key = self.parse_quantity(k, *from).map_err(|d| context(k, d))?;
            let value = self.parse_quantity(v, *to).map_err(|d| context(k, d))?;
            if let Some((prev, _)) = out.last()
                && key.si <= prev.si
            {
                return Err(Diagnostic::new(
                    code::VALUE,
                    Span::default(),
                    format!(
                        "table keys must increase strictly, but `{}` does not exceed the key \
                         before it",
                        k.trim()
                    ),
                ));
            }
            out.push((key, value));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{check_source, dim::builtin};

    const EXT: &str = "extension t.t 0.1.0\n\
        dimension Flow = Volume / Time\n\
        dimension Compliance = Volume / Pressure\n\
        key hazard.entry_speed : Velocity\n\
        key hazard.min_rate : Flow\n\
        key hazard.compliance : Compliance\n\
        key hazard.soc : Fraction\n\
        key settle.decel : param Acceleration magnitude\n\
        key settle.a_ebd : table Velocity -> Acceleration\n\
        key hazard.alarm : param Pressure unit mmHg\n";

    fn env() -> Env {
        check_source(EXT.as_bytes()).unwrap().env
    }

    fn ty(env: &Env, k: &str) -> KeyTy {
        env.key(k).unwrap().ty.clone()
    }

    fn q(v: ContractValue) -> Quantity {
        match v {
            ContractValue::Quantity(q) => q,
            other => panic!("{other:?}"),
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-9 * b.abs()
    }

    #[test]
    fn converts() {
        let env = env();
        let v = q(env
            .parse_value("30 km/h", &ty(&env, "entry_speed"))
            .unwrap());
        assert!(close(v.si, 30.0 / 3.6));
        assert_eq!(Some(v.dim), builtin("Velocity"));
        let v = q(env.parse_value("1 mL/h", &ty(&env, "min_rate")).unwrap());
        assert!(close(v.si, 1e-6 / 3600.0));
        let v = q(env
            .parse_value("0.003 mL/mmHg", &ty(&env, "compliance"))
            .unwrap());
        assert!(close(v.si, 0.003e-6 / 133.322387415));
        let v = env
            .parse_quantity("300 mmHg", builtin("Pressure").unwrap())
            .unwrap();
        assert!(close(v.si, 39_996.716_224_5));
        // A dimensionless key takes a bare number.
        let v = q(env.parse_value("0.30", &ty(&env, "soc")).unwrap());
        assert!(close(v.si, 0.3));
    }

    #[test]
    fn refusals() {
        let env = env();
        let e = env.parse_value("15", &ty(&env, "entry_speed")).unwrap_err();
        assert_eq!(e.code, code::VALUE);
        assert_eq!(e.message, "`15` needs a unit: a Velocity, e.g. `15 m/s`");
        let e = env
            .parse_value("15 s", &ty(&env, "entry_speed"))
            .unwrap_err();
        assert_eq!(e.code, code::DIMENSION);
        assert_eq!(e.message, "`15 s` is a Time, but a Velocity is expected");
        let e = env
            .parse_value("2.5 m/s^2", &ty(&env, "decel"))
            .unwrap_err();
        assert_eq!(e.code, code::VALUE);
        assert!(e.message.contains("name of the node parameter"));
        let e = env
            .parse_value("fast", &ty(&env, "entry_speed"))
            .unwrap_err();
        assert_eq!(e.code, code::VALUE);
        let e = env.parse_value("1 m", &ty(&env, "a_ebd")).unwrap_err();
        assert_eq!(e.code, code::VALUE);
    }

    #[test]
    fn params() {
        let env = env();
        assert_eq!(
            env.parse_value("target_acceleration", &ty(&env, "decel")),
            Ok(ContractValue::Param("target_acceleration".into()))
        );
        assert_eq!(
            env.parse_value("occlusion_limit_mmhg", &ty(&env, "alarm")),
            Ok(ContractValue::Param("occlusion_limit_mmhg".into()))
        );
    }

    #[test]
    fn tables() {
        let env = env();
        let t = ty(&env, "a_ebd");
        let rows = env
            .parse_table(&[("0 km/h", "0.75 m/s^2"), ("100 km/h", "0.70 m/s^2")], &t)
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert!(close(rows[1].0.si, 100.0 / 3.6));
        assert!(close(rows[1].1.si, 0.70));
        let e = env
            .parse_table(&[("100 km/h", "0.7 m/s^2"), ("100 km/h", "0.6 m/s^2")], &t)
            .unwrap_err();
        assert!(e.message.contains("increase strictly"), "{}", e.message);
        let e = env.parse_table(&[("0 km/h", "0.75 s")], &t).unwrap_err();
        assert_eq!(e.code, code::DIMENSION);
        assert!(
            e.message.starts_with("table entry `0 km/h`"),
            "{}",
            e.message
        );
        assert!(env.parse_table(&[], &t).is_err());
    }
}
