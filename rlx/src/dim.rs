//! Dimensions: vectors of integer exponents over the seven SI base
//! dimensions.
//!
//! A dimension is what a value measures (a Velocity), not how it is written
//! (`km/h` or `m/s`); units carry the scale, dimensions only the exponents.
//! Dimensionless is the zero vector, spelled `Scalar` (or `1` in a
//! dimension expression). Angles are dimensionless too, as in SI: `rad` and
//! `deg` are scaled units of `Scalar`. None of the examples needs angles to
//! be distinct, and making them a base dimension would make `2 pi rad / s`
//! and `Frequency` disagree for no checking gain. `Angle`, `Fraction` and
//! `Scalar` are therefore three names for one vector.

use std::fmt::Write;

/// The base dimensions, in vector order.
pub const BASE: [&str; 7] = [
    "Length",
    "Mass",
    "Time",
    "Current",
    "Temperature",
    "Amount",
    "LuminousIntensity",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Dim(pub [i32; 7]);

impl Dim {
    pub const SCALAR: Dim = Dim([0; 7]);

    pub fn base(i: usize) -> Dim {
        let mut v = [0; 7];
        v[i] = 1;
        Dim(v)
    }

    pub fn is_scalar(self) -> bool {
        self == Dim::SCALAR
    }

    pub fn times(self, o: Dim) -> Dim {
        let mut v = self.0;
        for (a, b) in v.iter_mut().zip(o.0) {
            *a += b;
        }
        Dim(v)
    }

    pub fn per(self, o: Dim) -> Dim {
        let mut v = self.0;
        for (a, b) in v.iter_mut().zip(o.0) {
            *a -= b;
        }
        Dim(v)
    }

    pub fn powi(self, n: i32) -> Dim {
        Dim(self.0.map(|e| e * n))
    }

    /// Half of every exponent, when all are even.
    pub fn sqrt(self) -> Option<Dim> {
        if self.0.iter().all(|e| e % 2 == 0) {
            Some(Dim(self.0.map(|e| e / 2)))
        } else {
            None
        }
    }

    /// The base-exponent spelling: `Length/Time^2`, `Mass/(Length*Time^2)`,
    /// `1/Time`, `Scalar`.
    pub fn render_base(self) -> String {
        render_with(self, &BASE)
    }
}

/// Render over a set of seven base spellings (dimension names or the SI
/// base unit symbols).
pub fn render_with(d: Dim, names: &[&str; 7]) -> String {
    if d.is_scalar() {
        return "Scalar".to_string();
    }
    let term = |name: &str, e: i32| {
        if e == 1 {
            name.to_string()
        } else {
            format!("{name}^{e}")
        }
    };
    let num: Vec<String> = (0..7)
        .filter(|&i| d.0[i] > 0)
        .map(|i| term(names[i], d.0[i]))
        .collect();
    let den: Vec<String> = (0..7)
        .filter(|&i| d.0[i] < 0)
        .map(|i| term(names[i], -d.0[i]))
        .collect();
    let mut s = if num.is_empty() {
        "1".to_string()
    } else {
        num.join("*")
    };
    match den.len() {
        0 => {}
        1 => {
            let _ = write!(s, "/{}", den[0]);
        }
        _ => {
            let _ = write!(s, "/({})", den.join("*"));
        }
    }
    s
}

/// Built-in named dimensions, as (name, exponents over [`BASE`]). The
/// order matters: when a vector has several names, messages use the first
/// (so `1/Time` is `Frequency`, not `AngularVelocity`, and the zero vector
/// is `Scalar`). Names an example declares itself (`Flow`, `Compliance`,
/// `PressureRate`) are deliberately absent, since redefining a built-in is
/// refused.
pub const BUILTIN: &[(&str, [i32; 7])] = &[
    ("Scalar", [0, 0, 0, 0, 0, 0, 0]),
    ("Fraction", [0, 0, 0, 0, 0, 0, 0]),
    ("Angle", [0, 0, 0, 0, 0, 0, 0]),
    ("Length", [1, 0, 0, 0, 0, 0, 0]),
    ("Mass", [0, 1, 0, 0, 0, 0, 0]),
    ("Time", [0, 0, 1, 0, 0, 0, 0]),
    ("Current", [0, 0, 0, 1, 0, 0, 0]),
    ("Temperature", [0, 0, 0, 0, 1, 0, 0]),
    ("Amount", [0, 0, 0, 0, 0, 1, 0]),
    ("LuminousIntensity", [0, 0, 0, 0, 0, 0, 1]),
    ("Area", [2, 0, 0, 0, 0, 0, 0]),
    ("Volume", [3, 0, 0, 0, 0, 0, 0]),
    ("Velocity", [1, 0, -1, 0, 0, 0, 0]),
    ("Acceleration", [1, 0, -2, 0, 0, 0, 0]),
    ("Jerk", [1, 0, -3, 0, 0, 0, 0]),
    ("Frequency", [0, 0, -1, 0, 0, 0, 0]),
    ("AngularVelocity", [0, 0, -1, 0, 0, 0, 0]),
    ("AngularAcceleration", [0, 0, -2, 0, 0, 0, 0]),
    ("Force", [1, 1, -2, 0, 0, 0, 0]),
    ("Pressure", [-1, 1, -2, 0, 0, 0, 0]),
    ("Energy", [2, 1, -2, 0, 0, 0, 0]),
    ("Power", [2, 1, -3, 0, 0, 0, 0]),
    ("Momentum", [1, 1, -1, 0, 0, 0, 0]),
    ("Density", [-3, 1, 0, 0, 0, 0, 0]),
    ("Charge", [0, 0, 1, 1, 0, 0, 0]),
    ("Voltage", [2, 1, -3, -1, 0, 0, 0]),
    ("Resistance", [2, 1, -3, -2, 0, 0, 0]),
];

pub fn builtin(name: &str) -> Option<Dim> {
    BUILTIN
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, v)| Dim(*v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn algebra_and_rendering() {
        let l = Dim::base(0);
        let t = Dim::base(2);
        let v = l.per(t);
        assert_eq!(Some(v), builtin("Velocity"));
        assert_eq!(v.per(t).render_base(), "Length/Time^2");
        assert_eq!(
            builtin("Pressure").unwrap().render_base(),
            "Mass/(Length*Time^2)"
        );
        assert_eq!(t.powi(-1).render_base(), "1/Time");
        assert_eq!(t.powi(2).sqrt(), Some(t));
        assert_eq!(t.sqrt(), None);
        assert_eq!(Dim::SCALAR.render_base(), "Scalar");
    }
}
