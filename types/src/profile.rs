//! Profiles (phase 91 E6): a severity scale and its map to scheduling
//! criticality, as data a contract names in `use:`.
//!
//! ```yaml
//! profile: iec61508
//! version: 1.0.0
//! requires: { rlm: ">=0.1.71" }
//! severity_scale: [none, SIL_1, SIL_2, SIL_3, SIL_4]
//! criticality: { none: null, SIL_1: low, SIL_2: medium, SIL_3: high, SIL_4: high }
//! functions: |          # optional: rlx `fn` items (ISO 26262 decomposition)
//!   fn ...
//! ```
//!
//! A contract that uses a profile draws `hazards.<h>.severity` from its
//! scale, and a node a hazard reaches takes the criticality the profile's
//! map names, instead of the `ceil(3 rank / n)` split [`crate::severity`]
//! applies to a `severity_levels:` scale. The map is the profile author's,
//! stated level by level, so a standard whose levels do not split into
//! thirds (ISO 13849's five PLs, IEC 62304's three classes) says so.
//!
//! The parser refuses what would otherwise be read some other way: an
//! unknown key, a level the map does not name or a name the scale lacks, a
//! criticality outside `low | medium | high | null`, and a map that falls as
//! the severity rises. `functions:` is kept as text; the consumer
//! type-checks it with rlx, which this crate does not depend on.
//!
//! The reference profiles ([`REFERENCE`]) ship in this crate, so a checker
//! has them without a ROS package: `iso26262`, `iec61508`, `iec61511`,
//! `do178c`, `iec62304`, `iso13849`, `en50129`.

use yaml_rust2::{Yaml, YamlLoader};

use crate::{
    VersionReq,
    parse::{is_dotted_name, is_semver},
    severity::CRITICALITY_LEVELS,
};

/// One profile, as parsed.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    /// A dotted lowercase name (`iso26262`), the name `use:` gives.
    pub name: String,
    /// `MAJOR.MINOR.PATCH`.
    pub version: String,
    /// `requires: { rlm: "<req>" }`, when written.
    pub requires_rlm: Option<VersionReq>,
    /// The severity levels, ascending.
    pub scale: Vec<String>,
    /// For each level of [`Profile::scale`], in order, the criticality it
    /// derives: one of [`CRITICALITY_LEVELS`], or `None` for no requirement.
    pub criticality: Vec<Option<&'static str>>,
    /// rlx `fn` items, as written.
    pub functions: Option<String>,
}

impl Profile {
    /// The rank of `level` on the scale, compared without case (as the
    /// checker compares a hazard's severity).
    pub fn rank(&self, level: &str) -> Option<usize> {
        self.scale
            .iter()
            .position(|s| s.eq_ignore_ascii_case(level))
    }

    /// The criticality the level of `rank` derives.
    pub fn criticality_of(&self, rank: usize) -> Option<&'static str> {
        self.criticality.get(rank).copied().flatten()
    }

    /// The profile's `functions:` as an rlx file: the items under a header
    /// naming the profile, so rlx's identity and diagnostics name it.
    pub fn functions_source(&self) -> Option<String> {
        self.functions
            .as_ref()
            .map(|f| format!("extension {} {}\n\n{f}", self.name, self.version))
    }
}

/// The reference profiles: (file name, source).
pub const REFERENCE: [(&str, &str); 7] = [
    (
        "iso26262.profile.yaml",
        include_str!("../profiles/iso26262.profile.yaml"),
    ),
    (
        "iec61508.profile.yaml",
        include_str!("../profiles/iec61508.profile.yaml"),
    ),
    (
        "iec61511.profile.yaml",
        include_str!("../profiles/iec61511.profile.yaml"),
    ),
    (
        "do178c.profile.yaml",
        include_str!("../profiles/do178c.profile.yaml"),
    ),
    (
        "iec62304.profile.yaml",
        include_str!("../profiles/iec62304.profile.yaml"),
    ),
    (
        "iso13849.profile.yaml",
        include_str!("../profiles/iso13849.profile.yaml"),
    ),
    (
        "en50129.profile.yaml",
        include_str!("../profiles/en50129.profile.yaml"),
    ),
];

/// The file-name suffix a profile is found by beside rlx files.
pub const FILE_SUFFIX: &str = ".profile.yaml";

const KEYS: [&str; 6] = [
    "profile",
    "version",
    "requires",
    "severity_scale",
    "criticality",
    "functions",
];

fn scalar(y: &Yaml) -> Option<String> {
    match y {
        Yaml::String(s) | Yaml::Real(s) => Some(s.clone()),
        Yaml::Integer(i) => Some(i.to_string()),
        _ => None,
    }
}

/// Parse a profile; the error says what is wrong and where.
pub fn parse_profile(text: &str) -> Result<Profile, String> {
    let docs = YamlLoader::load_from_str(text).map_err(|e| format!("not YAML: {e}"))?;
    let [doc] = docs.as_slice() else {
        return Err("a profile is one YAML document".into());
    };
    let Some(map) = doc.as_hash() else {
        return Err("a profile is a map (`profile:`, `version:`, ...)".into());
    };
    for k in map.keys() {
        let k = k.as_str().unwrap_or("");
        if !KEYS.contains(&k) {
            return Err(format!(
                "unknown key `{k}`; a profile has {}",
                KEYS.map(|k| format!("`{k}`")).join(", ")
            ));
        }
    }
    let name = scalar(&doc["profile"]).ok_or("`profile:` (its name) is required")?;
    if !is_dotted_name(&name) {
        return Err(format!(
            "`profile: {name}`: a name is dotted lowercase words (`iso26262`)"
        ));
    }
    let version = scalar(&doc["version"]).ok_or("`version:` is required")?;
    if !is_semver(&version) {
        return Err(format!("`version: {version}`: MAJOR.MINOR.PATCH"));
    }
    let requires_rlm = match &doc["requires"] {
        Yaml::BadValue => None,
        Yaml::Hash(h) => {
            let mut rlm = None;
            for (k, v) in h {
                match k.as_str() {
                    Some("rlm") => {
                        let t = scalar(v).ok_or("`requires.rlm` is a version requirement")?;
                        rlm = Some(VersionReq::parse(&t).ok_or_else(|| {
                            format!("`requires.rlm: {t}` is not a version requirement")
                        })?);
                    }
                    other => {
                        return Err(format!(
                            "`requires.{}`: a profile requires only `rlm`",
                            other.unwrap_or("?")
                        ));
                    }
                }
            }
            rlm
        }
        _ => return Err("`requires:` is a map (`{ rlm: \">=0.1.71\" }`)".into()),
    };
    let scale: Vec<String> = match &doc["severity_scale"] {
        Yaml::Array(a) => a
            .iter()
            .map(|y| scalar(y).ok_or("`severity_scale` holds level names"))
            .collect::<Result<_, _>>()?,
        _ => return Err("`severity_scale:` (a list of levels, ascending) is required".into()),
    };
    if scale.len() < 2 {
        return Err("`severity_scale` needs at least two levels".into());
    }
    for (i, s) in scale.iter().enumerate() {
        if scale[..i].iter().any(|p| p.eq_ignore_ascii_case(s)) {
            return Err(format!("`severity_scale` names `{s}` twice"));
        }
    }
    let Yaml::Hash(cmap) = &doc["criticality"] else {
        return Err("`criticality:` (each level's criticality) is required".into());
    };
    let mut criticality: Vec<Option<Option<&'static str>>> = vec![None; scale.len()];
    for (k, v) in cmap {
        let level = scalar(k).unwrap_or_default();
        let Some(rank) = scale.iter().position(|s| *s == level) else {
            return Err(format!(
                "`criticality.{level}`: `{level}` is not on the scale [{}]",
                scale.join(", ")
            ));
        };
        let crit = match v {
            Yaml::Null => None,
            other => {
                let t = scalar(other).unwrap_or_default();
                Some(
                    *CRITICALITY_LEVELS
                        .iter()
                        .find(|c| **c == t)
                        .ok_or_else(|| {
                            format!("`criticality.{level}: {t}`: one of low, medium, high, or null")
                        })?,
                )
            }
        };
        criticality[rank] = Some(crit);
    }
    let criticality: Vec<Option<&'static str>> = criticality
        .into_iter()
        .zip(&scale)
        .map(|(c, s)| c.ok_or_else(|| format!("`criticality` does not name `{s}`")))
        .collect::<Result<_, _>>()?;
    let pos = |c: Option<&str>| {
        c.map_or(0, |c| {
            1 + CRITICALITY_LEVELS.iter().position(|l| *l == c).unwrap_or(0)
        })
    };
    for i in 1..scale.len() {
        if pos(criticality[i]) < pos(criticality[i - 1]) {
            return Err(format!(
                "`criticality` falls from `{}` ({}) to `{}` ({}): a higher severity may not \
                 derive a lower criticality",
                scale[i - 1],
                criticality[i - 1].unwrap_or("null"),
                scale[i],
                criticality[i].unwrap_or("null")
            ));
        }
    }
    let functions = match &doc["functions"] {
        Yaml::BadValue => None,
        Yaml::String(s) => Some(s.clone()),
        _ => return Err("`functions:` is rlx text (`functions: |`)".into()),
    };
    Ok(Profile {
        name,
        version,
        requires_rlm,
        scale,
        criticality,
        functions,
    })
}

/// The reference profiles, parsed.
pub fn reference() -> Vec<Profile> {
    REFERENCE
        .iter()
        .map(|(f, s)| parse_profile(s).unwrap_or_else(|e| panic!("{f}: {e}")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::severity::{DEFAULT_SEVERITY_LEVELS, criticality_map};

    #[test]
    fn every_reference_profile_parses_and_requires_this_release() {
        let all = reference();
        assert_eq!(all.len(), 7);
        for p in &all {
            assert!(
                REFERENCE
                    .iter()
                    .any(|(f, _)| *f == format!("{}{FILE_SUFFIX}", p.name)),
                "{} is not in a file of its name",
                p.name
            );
            let req = p.requires_rlm.as_ref().expect("requires rlm");
            assert!(req.matches(crate::GRAMMAR_VERSION), "{}: {req}", p.name);
            assert_eq!(
                p.criticality_of(0),
                None,
                "{}: rank 0 derives nothing",
                p.name
            );
        }
    }

    /// The gate's premise: under `iso26262` every derived criticality is
    /// what the default scale already derives.
    #[test]
    fn iso26262_is_the_default_scale_and_map() {
        let p = reference()
            .into_iter()
            .find(|p| p.name == "iso26262")
            .unwrap();
        assert_eq!(p.scale, DEFAULT_SEVERITY_LEVELS.map(String::from));
        let default: Vec<_> = criticality_map(&DEFAULT_SEVERITY_LEVELS)
            .into_iter()
            .map(|(_, c)| c)
            .collect();
        assert_eq!(p.criticality, default);
    }

    #[test]
    fn refusals_name_the_mistake() {
        let base = "profile: t\nversion: 1.0.0\nseverity_scale: [n, a, b]\n";
        let err = |extra: &str| parse_profile(&format!("{base}{extra}")).unwrap_err();
        assert!(err("criticality: { n: null, a: low }\n").contains("does not name `b`"));
        assert!(
            err("criticality: { n: null, a: low, b: high, c: high }\n")
                .contains("`c` is not on the scale")
        );
        assert!(
            err("criticality: { n: null, a: urgent, b: high }\n")
                .contains("one of low, medium, high")
        );
        assert!(err("criticality: { n: null, a: high, b: low }\n").contains("falls from `a`"));
        assert!(
            err("criticality: { n: null, a: low, b: high }\nseverity: x\n")
                .contains("unknown key `severity`")
        );
        assert!(
            err("criticality: { n: null, a: low, b: high }\nrequires: { rosidl: \"^1\" }\n")
                .contains("only `rlm`")
        );
        assert!(parse_profile("profile: T\nversion: 1.0.0\nseverity_scale: [a, b]\ncriticality: { a: null, b: high }\n").unwrap_err().contains("dotted lowercase"));
        assert!(parse_profile("profile: t\nversion: 1\nseverity_scale: [a, b]\ncriticality: { a: null, b: high }\n").unwrap_err().contains("MAJOR.MINOR.PATCH"));
        assert!(parse_profile("profile: t\nversion: 1.0.0\nseverity_scale: [a, A]\ncriticality: { a: null, A: high }\n").unwrap_err().contains("twice"));
        // A map may stay flat: equal neighbours are not a fall.
        assert!(
            parse_profile(&format!(
                "{base}criticality: {{ n: null, a: high, b: high }}\n"
            ))
            .is_ok()
        );
    }
}
