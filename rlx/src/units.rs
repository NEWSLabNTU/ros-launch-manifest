//! Unit symbols, as names only.
//!
//! The parser has to know which identifiers are units, because the examples
//! write both `30 km/h` (a quantity) and `2 v0` (two times `v0`) with nothing
//! but the name to tell them apart. The rule: after a bare number, an
//! identifier on this list is a unit (and the unit may continue with `/`,
//! `*` and powers over further units); any other identifier is a factor of
//! an implicit multiplication. What each unit MEANS (its dimension and
//! scale) is the unit checker's business (E3), which keys on these names.
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

const UNITS: &[(&str, &str)] = &[
    // length
    ("m", "metre"),
    ("mm", "millimetre"),
    ("cm", "centimetre"),
    ("km", "kilometre"),
    ("um", "micrometre"),
    ("nm", "nanometre"),
    // time
    ("s", "second"),
    ("ms", "millisecond"),
    ("us", "microsecond"),
    ("ns", "nanosecond"),
    ("min", "minute"),
    ("h", "hour"),
    // mass
    ("g", "gram"),
    ("mg", "milligram"),
    ("kg", "kilogram"),
    // volume
    ("L", "litre"),
    ("mL", "millilitre"),
    // frequency
    ("Hz", "hertz"),
    ("kHz", "kilohertz"),
    // force
    ("N", "newton"),
    ("kN", "kilonewton"),
    // energy
    ("J", "joule"),
    ("kJ", "kilojoule"),
    ("Wh", "watt-hour"),
    ("kWh", "kilowatt-hour"),
    // power
    ("W", "watt"),
    ("mW", "milliwatt"),
    ("kW", "kilowatt"),
    // pressure
    ("Pa", "pascal"),
    ("kPa", "kilopascal"),
    ("MPa", "megapascal"),
    ("bar", "bar"),
    ("mbar", "millibar"),
    ("mmHg", "millimetre of mercury"),
    // electrical
    ("A", "ampere"),
    ("mA", "milliampere"),
    ("V", "volt"),
    ("mV", "millivolt"),
    // other base units and angles
    ("K", "kelvin"),
    ("mol", "mole"),
    ("cd", "candela"),
    ("rad", "radian"),
    ("deg", "degree"),
];

/// Is this identifier a unit symbol?
pub fn is_unit(name: &str) -> bool {
    UNITS.iter().any(|(u, _)| *u == name)
}

/// The unit's spoken name, for messages.
pub fn unit_name(name: &str) -> Option<&'static str> {
    UNITS.iter().find(|(u, _)| *u == name).map(|(_, n)| *n)
}

/// Every unit symbol, for E3's table to check it covers each one.
pub fn symbols() -> impl Iterator<Item = &'static str> {
    UNITS.iter().map(|(u, _)| *u)
}
