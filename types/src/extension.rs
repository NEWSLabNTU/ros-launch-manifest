//! Extensions bundled with the checker (phase 91 E8), and the vehicle keys
//! that moved out of the core grammar into one of them.
//!
//! `hazards.<h>.entry_speed` and `settle: { decel, jerk }` were core keys
//! from v0.1.46 to v0.1.71, with the braking arithmetic in the consumer.
//! From v0.2.0 they are keys of the bundled `vehicle.braking` extension,
//! and the arithmetic is its rlx `derive settle.duration`. For one minor
//! release a contract may still write them without `use:`: the parser
//! collects them as extension keys ([`LEGACY_KEYS`]), and the consumer
//! loads `vehicle.braking` implicitly with a `deprecated-implicit-extension`
//! warning. The release after requires the `use:` line, which `migrate`
//! adds.

use crate::field_table::Context;

/// The extensions bundled with the checker: (file name, source). A
/// consumer offers each as a candidate for a `use:` entry, after any file
/// on disk of the same name and version.
pub const BUNDLED: [(&str, &str); 1] = [(
    "vehicle.braking.rlx",
    include_str!("../extensions/vehicle.braking.rlx"),
)];

/// The reference extensions (phase 91 E9): the worked examples of
/// `docs/design/extension-examples.md` as files, (file name, source). They
/// are not bundled: a contract that wants one ships a copy in its package's
/// `rlm/` directory. Each is marked illustrative in its header.
pub const REFERENCE: [(&str, &str); 5] = [
    (
        "iso13855.rlx",
        include_str!("../extensions/reference/iso13855.rlx"),
    ),
    (
        "occlusion.rlx",
        include_str!("../extensions/reference/occlusion.rlx"),
    ),
    (
        "descent.rlx",
        include_str!("../extensions/reference/descent.rlx"),
    ),
    ("ebd.rlx", include_str!("../extensions/reference/ebd.rlx")),
    ("pst.rlx", include_str!("../extensions/reference/pst.rlx")),
];

/// The bundled extension the legacy keys belong to.
pub const VEHICLE_BRAKING: &str = "vehicle.braking";

/// The keys a contract may write without `use:` for one release, each the
/// key of [`VEHICLE_BRAKING`] in that context.
pub const LEGACY_KEYS: [(Context, &str); 3] = [
    (Context::Hazard, "entry_speed"),
    (Context::Settle, "decel"),
    (Context::Settle, "jerk"),
];

/// Whether `key` (bare) is a legacy vehicle key of `context`.
pub fn is_legacy_key(context: Context, key: &str) -> bool {
    LEGACY_KEYS.iter().any(|(c, k)| *c == context && *k == key)
}
