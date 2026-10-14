//! Contract migration: rewrite deprecated spellings to the current ones
//! (phase 90 G8).
//!
//! Every spelling the grammar retires gets a row in [`REWRITES`]: what it
//! finds, and the edit that replaces it. A rewrite reads the parsed document
//! to decide WHAT changes and edits the source TEXT to make the change, so
//! comments, key order, quoting and indentation survive: the output differs
//! from the input only in the tokens a rewrite names. Nothing is
//! re-serialized.
//!
//! Each rewrite is a pure function of one file. A rewrite that needs to tell
//! two meanings apart does the lookup the checker does, within the same file
//! (a hazard's `reaction:` is a mode when the file declares a mode of that
//! name, which is how the checker resolves it: modes are looked up in the
//! hazard's own scope, and a scope is one contract file).
//!
//! The consumer's `migrate` verb walks files and prints or writes the result;
//! this module owns the rewrites, because the grammar is what they encode.

use std::{collections::HashMap, ops::Range};

use yaml_rust2::{
    Yaml, YamlLoader,
    parser::{Event, MarkedEventReceiver, Parser},
    scanner::Marker,
};

/// One rewrite rule: a deprecated spelling and its replacement.
pub struct Rewrite {
    /// Stable id, printed beside every edit it makes.
    pub id: &'static str,
    /// The phase-90 unit that retired the spelling (`G4`, ...).
    pub unit: &'static str,
    /// The grammar release that introduced the replacement.
    pub since: &'static str,
    /// The old spelling, as a contract writes it.
    pub from: &'static str,
    /// The new spelling.
    pub to: &'static str,
    /// When the rewrite applies, for `--list`.
    pub when: &'static str,
    find: fn(&Doc) -> Vec<Edit>,
}

/// Every rewrite, applied in this order. G3, G6 and G7 add rows here.
pub const REWRITES: &[Rewrite] = &[
    Rewrite {
        id: "hazard-reaction-enters",
        unit: "G4",
        since: "0.1.59",
        from: "hazards.<h>.reaction: <mode>",
        to: "hazards.<h>.enters: <mode>",
        when: "the name is a mode declared in the same file (the checker's lookup: a mode first); a scope path stays `reaction:`",
        find: hazard_reaction_enters,
    },
    Rewrite {
        id: "exit-on-holds",
        unit: "G4",
        since: "0.1.59",
        from: "modes.<m>.exit.on: <function>",
        to: "modes.<m>.exit.holds: <function>",
        when: "always (`on` names fault classes only)",
        find: exit_on_holds,
    },
];

/// One textual edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// The [`Rewrite::id`] that made it.
    pub rewrite: &'static str,
    /// The dotted contract path of the key it rewrites.
    pub path: String,
    /// Byte range in the source.
    pub range: Range<usize>,
    /// The text that replaces `range`.
    pub replacement: String,
    /// 1-based line of `range.start`.
    pub line: usize,
}

/// The result of migrating one file.
#[derive(Debug, Clone)]
pub struct Migration {
    /// The migrated text; equal to the input when `edits` is empty.
    pub output: String,
    pub edits: Vec<Edit>,
    /// Places a rewrite applies but cannot edit safely (a key spelled in a
    /// way the textual edit does not handle). Rewrite these by hand.
    pub skipped: Vec<String>,
}

impl Migration {
    pub fn changed(&self) -> bool {
        !self.edits.is_empty()
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct MigrateError(pub String);

/// The document a rewrite reads: the parsed YAML, and where each key token
/// sits in the source.
pub struct Doc<'a> {
    pub src: &'a str,
    pub yaml: Yaml,
    keys: HashMap<Vec<String>, Range<usize>>,
    skipped: std::cell::RefCell<Vec<String>>,
}

impl Doc<'_> {
    /// An edit renaming the key at `path` to `new_key`, or `None` (recorded
    /// as skipped) when the key's token could not be located.
    fn rename_key(&self, rewrite: &'static str, path: &[&str], new_key: &str) -> Option<Edit> {
        let key: Vec<String> = path.iter().map(|s| s.to_string()).collect();
        let dotted = path.join(".");
        let Some(range) = self.keys.get(&key).cloned() else {
            self.skipped
                .borrow_mut()
                .push(format!("{rewrite}: `{dotted}` (key token not found)"));
            return None;
        };
        let token = &self.src[range.clone()];
        let replacement = match token.chars().next() {
            Some(q @ ('"' | '\'')) => format!("{q}{new_key}{q}"),
            _ => new_key.to_string(),
        };
        let line = self.src[..range.start].matches('\n').count() + 1;
        Some(Edit {
            rewrite,
            path: dotted,
            range,
            replacement,
            line,
        })
    }
}

fn key_str(k: &Yaml) -> Option<String> {
    match k {
        Yaml::String(s) => Some(s.clone()),
        Yaml::Integer(i) => Some(i.to_string()),
        Yaml::Real(r) => Some(r.clone()),
        Yaml::Boolean(b) => Some(b.to_string()),
        _ => None,
    }
}

/// G4: a hazard's `reaction:` that names a mode of the same file becomes
/// `enters:`.
fn hazard_reaction_enters(doc: &Doc) -> Vec<Edit> {
    let Some(hazards) = doc.yaml["hazards"].as_hash() else {
        return Vec::new();
    };
    let modes: Vec<String> = doc.yaml["modes"]
        .as_hash()
        .map(|h| h.keys().filter_map(key_str).collect())
        .unwrap_or_default();
    let mut out = Vec::new();
    for (k, v) in hazards {
        let Some(name) = key_str(k) else { continue };
        if !v["enters"].is_badvalue() {
            continue; // both written: a parse error the checker reports
        }
        let Some(r) = v["reaction"].as_str() else {
            continue;
        };
        if modes.iter().any(|m| m == r)
            && let Some(e) = doc.rename_key(
                "hazard-reaction-enters",
                &["hazards", &name, "reaction"],
                "enters",
            )
        {
            out.push(e);
        }
    }
    out
}

/// G4: `modes.<m>.exit.on` becomes `exit.holds`.
fn exit_on_holds(doc: &Doc) -> Vec<Edit> {
    let Some(modes) = doc.yaml["modes"].as_hash() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (k, v) in modes {
        let Some(name) = key_str(k) else { continue };
        let exit = &v["exit"];
        if exit.as_hash().is_none() || exit["on"].is_badvalue() || !exit["holds"].is_badvalue() {
            continue;
        }
        if let Some(e) = doc.rename_key("exit-on-holds", &["modes", &name, "exit", "on"], "holds") {
            out.push(e);
        }
    }
    out
}

/// Migrate one contract's text. An input that is not YAML is an error; an
/// input that parses as a contract must still parse after the rewrite, or
/// the migration is refused (a rewrite bug must not reach a file).
pub fn migrate_str(src: &str) -> Result<Migration, MigrateError> {
    let docs =
        YamlLoader::load_from_str(src).map_err(|e| MigrateError(format!("not YAML: {e}")))?;
    let yaml = docs.into_iter().next().unwrap_or(Yaml::Null);
    let doc = Doc {
        src,
        yaml,
        keys: key_index(src),
        skipped: Default::default(),
    };
    let mut edits: Vec<Edit> = REWRITES.iter().flat_map(|r| (r.find)(&doc)).collect();
    edits.sort_by_key(|e| e.range.start);
    for w in edits.windows(2) {
        if w[0].range.end > w[1].range.start {
            return Err(MigrateError(format!(
                "rewrites `{}` and `{}` overlap at `{}`",
                w[0].rewrite, w[1].rewrite, w[1].path
            )));
        }
    }
    let mut output = String::with_capacity(src.len() + 16 * edits.len());
    let mut at = 0;
    for e in &edits {
        output.push_str(&src[at..e.range.start]);
        output.push_str(&e.replacement);
        at = e.range.end;
    }
    output.push_str(&src[at..]);

    if !edits.is_empty()
        && crate::parse_manifest_str(src).is_ok()
        && let Err(e) = crate::parse_manifest_str(&output)
    {
        return Err(MigrateError(format!(
            "the migrated text no longer parses ({e}); nothing written"
        )));
    }
    let skipped = doc.skipped.into_inner();
    Ok(Migration {
        output,
        edits,
        skipped,
    })
}

/// Byte range of every mapping key token, by its path of key names
/// (sequence items by index). yaml-rust2's markers count characters, so the
/// index is translated to bytes, and a token is recorded only when the
/// source at that place spells the key (plain, or quoted without escapes).
fn key_index(src: &str) -> HashMap<Vec<String>, Range<usize>> {
    let mut rx = KeyReceiver {
        src,
        char_to_byte: src.char_indices().map(|(b, _)| b).collect(),
        frames: Vec::new(),
        keys: HashMap::new(),
    };
    let mut parser = Parser::new_from_str(src);
    let _ = parser.load(&mut rx, false);
    rx.keys
}

struct Frame {
    seg: String,
    map: bool,
    expect_key: bool,
    pending: Option<String>,
    idx: usize,
}

struct KeyReceiver<'a> {
    src: &'a str,
    char_to_byte: Vec<usize>,
    frames: Vec<Frame>,
    keys: HashMap<Vec<String>, Range<usize>>,
}

impl KeyReceiver<'_> {
    fn path(&self) -> Vec<String> {
        self.frames.iter().skip(1).map(|f| f.seg.clone()).collect()
    }

    fn child_segment(&mut self) -> String {
        match self.frames.last_mut() {
            Some(f) if f.map => f.pending.take().unwrap_or_default(),
            Some(f) => f.idx.to_string(),
            None => String::new(),
        }
    }

    fn value_done(&mut self) {
        if let Some(f) = self.frames.last_mut() {
            if f.map {
                f.pending = None;
                f.expect_key = true;
            } else {
                f.idx += 1;
            }
        }
    }

    fn token_range(&self, mark: &Marker, key: &str) -> Option<Range<usize>> {
        let start = *self.char_to_byte.get(mark.index())?;
        let rest = &self.src[start..];
        if rest.starts_with(key) {
            return Some(start..start + key.len());
        }
        let q = rest.chars().next()?;
        if (q == '"' || q == '\'')
            && rest[1..].starts_with(key)
            && rest[1 + key.len()..].starts_with(q)
        {
            return Some(start..start + key.len() + 2);
        }
        None
    }
}

impl MarkedEventReceiver for KeyReceiver<'_> {
    fn on_event(&mut self, ev: Event, mark: Marker) {
        match ev {
            Event::MappingStart(..) | Event::SequenceStart(..) => {
                let map = matches!(ev, Event::MappingStart(..));
                let seg = self.child_segment();
                self.frames.push(Frame {
                    seg,
                    map,
                    expect_key: map,
                    pending: None,
                    idx: 0,
                });
            }
            Event::MappingEnd | Event::SequenceEnd => {
                self.frames.pop();
                self.value_done();
            }
            Event::Scalar(value, ..) => {
                let is_key = self.frames.last().is_some_and(|f| f.map && f.expect_key);
                if is_key {
                    let mut path = self.path();
                    path.push(value.clone());
                    if let Some(r) = self.token_range(&mark, &value) {
                        self.keys.entry(path).or_insert(r);
                    }
                    let f = self.frames.last_mut().unwrap();
                    f.pending = Some(value);
                    f.expect_key = false;
                } else {
                    self.value_done();
                }
            }
            Event::Alias(_) => self.value_done(),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LADDER: &str = "\
version: 1
# the ladder -- comments survive
modes:
  driving:
    requires: [perception]
    fallback: [tor, stop]
  tor:
    window: 10s
    exit: { on: driver_took_over, to: manual }   # flow form
    reaction: sys.tor
  stop:
    reaction: sys.stop
  manual: {}
hazards:
  blind:
    severity: ASIL_D
    guards: [scan]
    reaction: driving    # a mode: becomes enters
  stale:
    guards: [odom]
    reaction: sys.stop   # a path: stays
";

    #[test]
    fn g4_rewrites_rename_only_the_keys() {
        let m = migrate_str(LADDER).unwrap();
        let ids: Vec<_> = m
            .edits
            .iter()
            .map(|e| (e.rewrite, e.path.as_str(), e.line))
            .collect();
        assert_eq!(
            ids,
            vec![
                ("exit-on-holds", "modes.tor.exit.on", 9),
                ("hazard-reaction-enters", "hazards.blind.reaction", 18),
            ]
        );
        let want = LADDER
            .replace("exit: { on: driver", "exit: { holds: driver")
            .replace("reaction: driving ", "enters: driving ");
        assert_eq!(m.output, want);
        assert!(m.skipped.is_empty());
        // Idempotent: a migrated file has nothing left to rewrite.
        assert!(!migrate_str(&m.output).unwrap().changed());
    }

    #[test]
    fn non_ascii_before_a_key_keeps_byte_offsets_right() {
        let src = "version: 1\n# déjà vu — em dash\nmodes:\n  a:\n    window: 1s\n    exit:\n      on: f\n      to: b\n  b: {}\n";
        let m = migrate_str(src).unwrap();
        assert_eq!(m.output, src.replace("      on: f", "      holds: f"));
    }

    #[test]
    fn quoted_keys_keep_their_quotes() {
        let src =
            "version: 1\nmodes:\n  a:\n    window: 1s\n    exit: { \"on\": f, to: b }\n  b: {}\n";
        let m = migrate_str(src).unwrap();
        assert_eq!(m.output, src.replace("\"on\"", "\"holds\""));
    }

    #[test]
    fn a_file_with_nothing_to_rewrite_is_unchanged() {
        let src = "version: 1\nhazards:\n  h:\n    enters: d\nmodes:\n  d: {}\n";
        let m = migrate_str(src).unwrap();
        assert!(!m.changed());
        assert_eq!(m.output, src);
    }

    #[test]
    fn every_rewrite_has_a_distinct_id() {
        let mut ids: Vec<_> = REWRITES.iter().map(|r| r.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), REWRITES.len());
    }
}
