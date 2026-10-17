//! How the evaluator's trace prints a value.
//!
//! A value is printed in one unit chosen by its dimension and magnitude, so
//! a reviewer reads `256 ms` and `500 mm`, not `0.256 s` and `0.5 m`:
//!
//! | Dimension | Unit |
//! |---|---|
//! | Time | `ms` below 1 s, `s` below 120 s, `min` from there |
//! | Length | `mm` below 1 m, else `m` (never `km`, as the examples) |
//! | Mass | `g` below 1 kg, else `kg` |
//! | Volume | `mL` below 1 L, `L` below 1 m^3, else `m^3` |
//! | Energy | `J` below 1 Wh, `Wh` below 1 kWh, else `kWh` |
//! | Power | `W` below 1 kW, else `kW` |
//! | Pressure | `Pa`, `kPa` from 1 kPa, `MPa` from 1 MPa |
//! | Scalar | no unit |
//! | any other | coherent SI (`m/s`, `m/s^2`, `N`, `m^3/s`) |
//!
//! The number has four decimals below 10 and two from there, trailing zeros
//! dropped (`8.3333`, `2.5`, `4422.67`, `500`); outside [0.001, 10^6) it is
//! written in scientific notation with four decimals (`2.7778e-10`). An
//! input's trace line adds the text the contract wrote (`30 km/h`) when the
//! consumer passes it. Formatting uses Rust's float formatting only, so the
//! same value always prints the same text.

use crate::{
    dim::{Dim, builtin},
    units,
};

fn is(d: Dim, name: &str) -> bool {
    builtin(name) == Some(d)
}

/// The unit a value of `dim` is shown in, and the factor from SI to it.
fn choose(si: f64, dim: Dim) -> (f64, String) {
    let a = si.abs();
    let pick = |scale: f64, unit: &str| (scale, unit.to_string());
    let (scale, unit) = if dim.is_scalar() {
        pick(1.0, "")
    } else if is(dim, "Time") {
        if a != 0.0 && a < 1.0 {
            pick(1e-3, "ms")
        } else if a < 120.0 {
            pick(1.0, "s")
        } else {
            pick(60.0, "min")
        }
    } else if is(dim, "Length") {
        if a != 0.0 && a < 1.0 {
            pick(1e-3, "mm")
        } else {
            pick(1.0, "m")
        }
    } else if is(dim, "Mass") {
        if a != 0.0 && a < 1.0 {
            pick(1e-3, "g")
        } else {
            pick(1.0, "kg")
        }
    } else if is(dim, "Volume") {
        if a != 0.0 && a < 1e-3 {
            pick(1e-6, "mL")
        } else if a < 1.0 {
            pick(1e-3, "L")
        } else {
            pick(1.0, "m^3")
        }
    } else if is(dim, "Energy") {
        if a >= 3.6e6 {
            pick(3.6e6, "kWh")
        } else if a >= 3600.0 {
            pick(3600.0, "Wh")
        } else {
            pick(1.0, "J")
        }
    } else if is(dim, "Power") {
        if a >= 1e3 {
            pick(1e3, "kW")
        } else {
            pick(1.0, "W")
        }
    } else if is(dim, "Pressure") {
        if a >= 1e6 {
            pick(1e6, "MPa")
        } else if a >= 1e3 {
            pick(1e3, "kPa")
        } else {
            pick(1.0, "Pa")
        }
    } else {
        (1.0, units::example(dim))
    };
    (si / scale, unit)
}

/// The number alone, by the rule in the module docs.
pub fn number(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    let a = v.abs();
    if !(1e-3..1e6).contains(&a) {
        return format!("{v:.4e}");
    }
    let s = if a < 10.0 {
        format!("{v:.4}")
    } else {
        format!("{v:.2}")
    };
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

/// A value with its unit: `8.3333 m/s`, `4422.67 ms`, `0.84 mL`.
pub fn quantity(si: f64, dim: Dim) -> String {
    let (v, unit) = choose(si, dim);
    if unit.is_empty() {
        number(v)
    } else {
        format!("{} {unit}", number(v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(n: &str) -> Dim {
        builtin(n).unwrap()
    }

    #[test]
    fn rule() {
        assert_eq!(quantity(30.0 / 3.6, d("Velocity")), "8.3333 m/s");
        assert_eq!(quantity(4.422_666, d("Time")), "4.4227 s");
        assert_eq!(quantity(0.256, d("Time")), "256 ms");
        assert_eq!(quantity(0.5, d("Length")), "500 mm");
        assert_eq!(quantity(1529.754, d("Length")), "1529.75 m");
        assert_eq!(quantity(3029.0, d("Time")), "50.48 min");
        assert_eq!(quantity(8.4e-7, d("Volume")), "0.84 mL");
        assert_eq!(quantity(36_570.0, d("Energy")), "10.16 Wh");
        assert_eq!(quantity(0.75, d("Acceleration")), "0.75 m/s^2");
        assert_eq!(quantity(0.08, Dim::SCALAR), "0.08");
        assert_eq!(quantity(0.0, d("Velocity")), "0 m/s");
        let flow = d("Volume").per(d("Time"));
        assert_eq!(quantity(1e-6 / 3600.0, flow), "2.7778e-10 m^3/s");
    }
}
