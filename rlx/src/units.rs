//! Unit symbols, their dimensions and their SI scale.
//!
//! The parser has to know which identifiers are units, because the examples
//! write both `30 km/h` (a quantity) and `2 v0` (two times `v0`) with nothing
//! but the name to tell them apart. The rule: after a bare number, an
//! identifier on this list is a unit (and the unit may continue with `/`,
//! `*` and powers over further units); any other identifier is a factor of
//! an implicit multiplication. The type checker (E3) reads each unit's
//! dimension from this table, and the contract value parser its scale.
//!
//! The list is deliberately short and explicit rather than "every SI prefix
//! on every SI unit": each entry is a name a contract or extension author
//! might write, and each entry is also a name that can no longer be a
//! variable in a quantity position. Single letters whose unit is rare in
//! this domain and whose name is a common variable are LEFT OUT for that
//! reason: `T` (tesla), `C` (coulomb), `F` (farad), `H` (henry), `S`
//! (siemens), `t` (tonne), `d` (day). The machinery example binds `T` and
//! `C` as variables. To keep the remaining ambiguity from biting silently,
//! a binding, parameter or key named like a unit is refused (`rlx-shadow`).
//!
//! # Scales
//!
//! A scale is the factor to SI base units, as an `f64`: `km/h` is
//! `1000/3600` m/s. f64 rather than exact rationals because the evaluator
//! (design 5.2) computes in IEEE-754 doubles anyway, and two of the scales
//! (`mmHg`, `deg`) are irrational or defined by a measured constant. A
//! composed unit multiplies scales in the order written, which is a fixed
//! order, so the same text always gives the same bits.
//!
//! **No affine units.** Degrees Celsius and Fahrenheit carry an offset, so
//! `20 °C * 2` has no single meaning; they are not on the list, and the
//! symbols `°`, `℃` and `℉` are refused by the character rules with a hint to
//! write a temperature difference in `K`.

use crate::dim::Dim;

pub struct Unit {
    pub symbol: &'static str,
    pub name: &'static str,
    /// Exponents over [`crate::dim::BASE`].
    pub dim: [i32; 7],
    /// Factor to SI base units.
    pub scale: f64,
}

const L: [i32; 7] = [1, 0, 0, 0, 0, 0, 0];
const M: [i32; 7] = [0, 1, 0, 0, 0, 0, 0];
const T: [i32; 7] = [0, 0, 1, 0, 0, 0, 0];
const VOL: [i32; 7] = [3, 0, 0, 0, 0, 0, 0];
const FREQ: [i32; 7] = [0, 0, -1, 0, 0, 0, 0];
const FORCE: [i32; 7] = [1, 1, -2, 0, 0, 0, 0];
const ENERGY: [i32; 7] = [2, 1, -2, 0, 0, 0, 0];
const POWER: [i32; 7] = [2, 1, -3, 0, 0, 0, 0];
const PRESSURE: [i32; 7] = [-1, 1, -2, 0, 0, 0, 0];
const CURRENT: [i32; 7] = [0, 0, 0, 1, 0, 0, 0];
const VOLTAGE: [i32; 7] = [2, 1, -3, -1, 0, 0, 0];
const SCALAR: [i32; 7] = [0; 7];

const fn u(symbol: &'static str, name: &'static str, dim: [i32; 7], scale: f64) -> Unit {
    Unit {
        symbol,
        name,
        dim,
        scale,
    }
}

const UNITS: &[Unit] = &[
    // length
    u("m", "metre", L, 1.0),
    u("mm", "millimetre", L, 1e-3),
    u("cm", "centimetre", L, 1e-2),
    u("km", "kilometre", L, 1e3),
    u("um", "micrometre", L, 1e-6),
    u("nm", "nanometre", L, 1e-9),
    // time
    u("s", "second", T, 1.0),
    u("ms", "millisecond", T, 1e-3),
    u("us", "microsecond", T, 1e-6),
    u("ns", "nanosecond", T, 1e-9),
    u("min", "minute", T, 60.0),
    u("h", "hour", T, 3600.0),
    // mass: the SI base unit is the kilogram
    u("g", "gram", M, 1e-3),
    u("mg", "milligram", M, 1e-6),
    u("kg", "kilogram", M, 1.0),
    // volume
    u("L", "litre", VOL, 1e-3),
    u("mL", "millilitre", VOL, 1e-6),
    // frequency
    u("Hz", "hertz", FREQ, 1.0),
    u("kHz", "kilohertz", FREQ, 1e3),
    // force
    u("N", "newton", FORCE, 1.0),
    u("kN", "kilonewton", FORCE, 1e3),
    // energy
    u("J", "joule", ENERGY, 1.0),
    u("kJ", "kilojoule", ENERGY, 1e3),
    u("Wh", "watt-hour", ENERGY, 3600.0),
    u("kWh", "kilowatt-hour", ENERGY, 3.6e6),
    // power
    u("W", "watt", POWER, 1.0),
    u("mW", "milliwatt", POWER, 1e-3),
    u("kW", "kilowatt", POWER, 1e3),
    // pressure
    u("Pa", "pascal", PRESSURE, 1.0),
    u("kPa", "kilopascal", PRESSURE, 1e3),
    u("MPa", "megapascal", PRESSURE, 1e6),
    u("bar", "bar", PRESSURE, 1e5),
    u("mbar", "millibar", PRESSURE, 1e2),
    // The conventional millimetre of mercury, 133.322387415 Pa exactly.
    u("mmHg", "millimetre of mercury", PRESSURE, 133.322_387_415),
    // electrical
    u("A", "ampere", CURRENT, 1.0),
    u("mA", "milliampere", CURRENT, 1e-3),
    u("V", "volt", VOLTAGE, 1.0),
    u("mV", "millivolt", VOLTAGE, 1e-3),
    // other base units and angles (dimensionless, see crate::dim)
    u("K", "kelvin", [0, 0, 0, 0, 1, 0, 0], 1.0),
    u("mol", "mole", [0, 0, 0, 0, 0, 1, 0], 1.0),
    u("cd", "candela", [0, 0, 0, 0, 0, 0, 1], 1.0),
    u("rad", "radian", SCALAR, 1.0),
    u("deg", "degree", SCALAR, std::f64::consts::PI / 180.0),
];

/// The SI base unit symbol for each base dimension, for spelling an example
/// unit in a message.
pub const BASE_SYMBOLS: [&str; 7] = ["m", "kg", "s", "A", "K", "mol", "cd"];

pub fn lookup(name: &str) -> Option<&'static Unit> {
    UNITS.iter().find(|u| u.symbol == name)
}

/// Is this identifier a unit symbol?
pub fn is_unit(name: &str) -> bool {
    lookup(name).is_some()
}

/// The unit's spoken name, for messages.
pub fn unit_name(name: &str) -> Option<&'static str> {
    lookup(name).map(|u| u.name)
}

/// Every unit symbol.
pub fn symbols() -> impl Iterator<Item = &'static str> {
    UNITS.iter().map(|u| u.symbol)
}

/// Dimension and scale of a unit product (`km/h`, `m/s^2`), or `None` when
/// a factor is not a unit symbol. The parser guarantees every factor of a
/// quantity's unit is one.
pub fn product(p: &crate::ast::Product) -> Option<(Dim, f64)> {
    let mut dim = Dim::SCALAR;
    let mut scale = 1.0;
    for f in &p.factors {
        let u = lookup(&f.name.name)?;
        dim = dim.times(Dim(u.dim).powi(f.power));
        scale *= u.scale.powi(f.power);
    }
    Some((dim, scale))
}

/// A unit to show in a message for a dimension: a listed unit of scale one
/// when there is one (`N`, `Pa`), else the SI base spelling (`m/s^2`).
pub fn example(d: Dim) -> String {
    if d.is_scalar() {
        return String::new();
    }
    if let Some(u) = UNITS.iter().find(|u| u.scale == 1.0 && Dim(u.dim) == d) {
        return u.symbol.to_string();
    }
    crate::dim::render_with(d, &BASE_SYMBOLS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dim::builtin, lexer::lex, parser::parse_expr};

    /// Dimension and SI value of a literal like `30 km/h`.
    fn si(text: &str) -> (Dim, f64) {
        let (toks, d) = lex(text);
        assert!(d.is_empty());
        let e = parse_expr(&toks).unwrap();
        match e.kind {
            crate::ast::ExprKind::Quantity { value, unit, .. } => {
                let (dim, scale) = product(&unit).unwrap();
                (dim, value * scale)
            }
            _ => panic!("{text} is not a quantity"),
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-9 * b.abs().max(1e-300)
    }

    /// Every unit spelling the seven examples use, with its SI value.
    #[test]
    fn example_units_convert() {
        let cases: &[(&str, &str, f64)] = &[
            ("30 km/h", "Velocity", 30.0 / 3.6),
            ("160 km/h", "Velocity", 160.0 / 3.6),
            ("100 km/h", "Velocity", 100.0 / 3.6),
            ("70 m/s", "Velocity", 70.0),
            ("2.5 m/s^2", "Acceleration", 2.5),
            ("0.75 m/s^2", "Acceleration", 0.75),
            ("1.5 m/s^3", "Jerk", 1.5),
            ("256ms", "Time", 0.256),
            ("10s", "Time", 10.0),
            ("30min", "Time", 1800.0),
            ("2 s", "Time", 2.0),
            ("30 mm", "Length", 0.03),
            ("520 mm", "Length", 0.52),
            ("1600 m", "Length", 1600.0),
            ("2000 mm/s", "Velocity", 2.0),
            ("300 mmHg", "Pressure", 300.0 * 133.322387415),
            ("11 bar", "Pressure", 1.1e6),
            ("600 W", "Power", 600.0),
            ("222 Wh", "Energy", 222.0 * 3600.0),
        ];
        for &(text, dim, want) in cases {
            let (d, v) = si(text);
            assert_eq!(Some(d), builtin(dim), "{text}");
            assert!(close(v, want), "{text}: {v} != {want}");
        }
        let vol = builtin("Volume").unwrap();
        let time = builtin("Time").unwrap();
        let pressure = builtin("Pressure").unwrap();
        // Dimensions the examples declare themselves.
        let (d, v) = si("1 mL/h");
        assert_eq!(d, vol.per(time));
        assert!(close(v, 1e-6 / 3600.0));
        let (d, v) = si("1200 mL/h");
        assert_eq!(d, vol.per(time));
        assert!(close(v, 1200e-6 / 3600.0));
        let (d, v) = si("0.003 mL/mmHg");
        assert_eq!(d, vol.per(pressure));
        assert!(close(v, 0.003e-6 / 133.322387415));
        let (d, v) = si("0.5 bar/s");
        assert_eq!(d, pressure.per(time));
        assert!(close(v, 0.5e5));
        let (d, v) = si("90 deg");
        assert!(d.is_scalar());
        assert!(close(v, std::f64::consts::FRAC_PI_2));
    }

    #[test]
    fn examples_spell_units() {
        assert_eq!(example(builtin("Velocity").unwrap()), "m/s");
        assert_eq!(example(builtin("Pressure").unwrap()), "Pa");
        assert_eq!(example(builtin("Jerk").unwrap()), "m/s^3");
    }
}
