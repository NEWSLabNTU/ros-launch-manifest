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

use std::{
    collections::{HashMap, HashSet},
    ops::Range,
};

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
        to: "hazards.<h>.leaves: <mode> (`enters:` from v0.1.59 to v0.3.18)",
        when: "the name is a mode declared in the same file (the checker's lookup: a mode first); an event chain stays `reaction:`",
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
    Rewrite {
        id: "path-input-trigger",
        unit: "G3",
        since: "0.1.59",
        from: "paths.<p>.input: [...]",
        to: "paths.<p>.trigger: { input: [...] }",
        when: "no `trigger:` is written; beside a `trigger:` the legacy list is deleted (the trigger is what is read)",
        find: path_input_trigger,
    },
    Rewrite {
        id: "external-topics-to-topics",
        unit: "G3",
        since: "0.1.59",
        from: "external_topics.<t>: { side|external, type, qos }",
        to: "topics.<t>: { external, type, qos }",
        when: "always; an entry whose topic is also under `topics:` gains `external:` (and `type:` when the topic has none) there",
        find: external_topics_to_topics,
    },
    Rewrite {
        id: "max-transport-link",
        unit: "104 S1",
        since: "0.3.16",
        from: "topics.<t>.max_transport: <d>; nodes.<n>.sub.<ep>.max_transport: <d>",
        to: "links.<name>: { max_transport: <d> } and `link: <name>` on the subscriber (one link per topic, named after it, for each listed subscriber that states none; one per subscriber, `<node>_<ep>`, for a subscriber's own)",
        when: "a topic's: every listed subscriber is declared in the same file (one with its own transport or link keeps it), else left for a hand edit. A subscriber's: no `link:` beside it. The value is copied as written, so a link the value gives no `source:` is `link-unsourced` until one is added",
        find: max_transport_links,
    },
    Rewrite {
        id: "tolerance-sync",
        unit: "104 S1",
        since: "0.3.16",
        from: "nodes.<n>.paths.<p>.tolerance: <d>",
        to: "nodes.<n>.paths.<p>.sync.max_interval: <d>",
        when: "the path has a `sync:` block whose `max_interval` is absent or written the same; without `sync:` it is left for a hand edit, since adding one turns the path's derived output rate from the SUM of its inputs into their MIN",
        find: tolerance_sync,
    },
    Rewrite {
        id: "node-path-drop-topic",
        unit: "G3",
        since: "0.1.60",
        from: "nodes.<n>.paths.<p>.drop: <spec>",
        to: "topics.<t>.drop: <spec>, <t> the topic whose `pub:` lists the path's one output",
        when: "the path has exactly one output and one topic of the same file publishes it, with no drop of its own or the same one; several outputs are left for a hand edit (which topic loses the messages is the author's call)",
        find: node_path_drop_topic,
    },
    Rewrite {
        id: "guard-all-of-function",
        unit: "G7",
        since: "0.1.59",
        from: "hazards.<h>.guards: [{ all_of: [...] }]",
        to: "capabilities.<h>_all_of: { all_of: [...] }, guards: [<h>_all_of]",
        when: "always (the inline group is removed; the same group named in `functions:`)",
        find: guard_all_of_function,
    },
    Rewrite {
        id: "entry-speed-unit",
        unit: "E8",
        since: "0.2.0",
        from: "hazards.<h>.entry_speed: <number>",
        to: "hazards.<h>.entry_speed: <number> m/s",
        when: "the value is a plain number (the core key's metres per second); `vehicle.braking` reads a velocity with its unit",
        find: entry_speed_unit,
    },
    Rewrite {
        id: "use-implicit",
        unit: "E8",
        since: "0.2.0",
        from: "`vehicle.braking`'s keys (`entry_speed`, `settle: { decel, jerk }`) without `use:`; a hazard `severity:` with no profile and no `severity_levels:`",
        to: "use: { vehicle.braking: \"^1\" } / use: { iso26262: \"^1\" }, the entries that apply, added to the level's `use:` (created when absent)",
        when: "the checker would load the extension or the scale implicitly; `iso26262` is not added beside a used name that may be a profile (an undotted name that is not a reference profile is skipped for a hand edit)",
        find: use_implicit,
    },
    Rewrite {
        id: "ext-param-read",
        unit: "94",
        since: "0.2.4",
        from: "settle: { decel: <parameter> }, an extension key typed `param` holding a bare parameter name",
        to: "settle: { decel: $(param <parameter>) m/s^2 }, the read form with the unit the key reads the parameter in",
        when: "the key is a `param` key of a bundled extension (`vehicle.braking`'s `decel` and `jerk`) and its value is a bare parameter name; a key of an extension the file does not carry is left (its type is the extension's)",
        find: ext_param_read,
    },
    Rewrite {
        id: "severity-integrity",
        unit: "98 N1",
        since: "0.3.3",
        from: "hazards.<h>.severity: <level>, severity_levels: [...]",
        to: "hazards.<h>.integrity: <level>, integrity_levels: [...]",
        when: "always (the key holds an integrity level -- an ASIL, SIL or DAL -- not ISO 26262's S0-S3 severity class)",
        find: severity_integrity,
    },
    Rewrite {
        id: "window-min-dwell",
        unit: "98 S4",
        since: "0.3.3",
        from: "modes.<m>.window: <d>",
        to: "modes.<m>.min_dwell: <d>",
        when: "always (a least time in a mode before the next is the switched-systems dwell time)",
        find: window_min_dwell,
    },
    Rewrite {
        id: "guards-monitors",
        unit: "98 N6 M2",
        since: "0.3.7",
        from: "hazards.<h>.guards: [...]",
        to: "hazards.<h>.monitors: [...]",
        when: "always (the topics watched for a fault are its monitored signals; in formal methods a guard is a transition's condition)",
        find: guards_monitors,
    },
    Rewrite {
        id: "ingress-arrival-curve",
        unit: "98 N6 M4",
        since: "0.3.7",
        from: "nodes.<n>.sub.<e>.ingress: { rate_hz, burst }",
        to: "nodes.<n>.sub.<e>.arrival_curve: { rate_hz, burst }",
        when: "always (a token bucket bounding what arrives is network calculus's arrival curve)",
        find: ingress_arrival_curve,
    },
    Rewrite {
        id: "aperiodic",
        unit: "98 N6 M5",
        since: "0.3.7",
        from: "nodes.<n>.pub.<e>.on_demand: true; a path's `trigger: spontaneous`",
        to: "nodes.<n>.pub.<e>.aperiodic: true; `trigger: aperiodic`",
        when: "always (an activation with no period and no minimum separation is aperiodic in the periodic / sporadic / aperiodic task model)",
        find: aperiodic,
    },
    Rewrite {
        id: "functions-capabilities",
        unit: "104 S3",
        since: "0.3.19",
        from: "functions.<f>: {...}",
        to: "capabilities.<f>: {...}",
        when: "always (what a mode requires and a fault removes is a capability; IEC 61508's safety function is detect-and-react)",
        find: functions_capabilities,
    },
    Rewrite {
        id: "hazard-enters-leaves",
        unit: "104 S3",
        since: "0.3.19",
        from: "hazards.<h>.enters: <mode>",
        to: "hazards.<h>.leaves: <mode>",
        when: "always (the hazard takes the mode away and the system leaves it by its fallback sequence; `supervise.enters` names the level entered)",
        find: hazard_enters_leaves,
    },
    Rewrite {
        id: "call-reaction-enters",
        unit: "104 S3",
        since: "0.3.19",
        from: "cli.<e>.on_timeout.reaction / on_timeout.then.reaction / on_rejected.reaction: <mode>",
        to: "the same with `enters: <mode>`",
        when: "the name is a mode declared at the same manifest level (the checker's lookup); an event chain stays `reaction:`",
        find: call_reaction_enters,
    },
    Rewrite {
        id: "rule-id-renamed",
        unit: "98",
        since: "0.3.3",
        from: "acknowledged: [{ rule: <old id>, at: <path with an old key>, ... }]",
        to: "acknowledged: [{ rule: <new id>, at: <path with the new key>, ... }]",
        when: "the id is the old side of a rename in `rule_names::RENAMED_RULES`, or the path names a key of `rule_names::RENAMED_KEYS` where the grammar puts it (the rule emits under the new names only)",
        find: rule_id_renamed,
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
    /// Keys whose mapping is a flow mapping (`{ ... }`).
    flow: HashSet<Vec<String>>,
    /// Byte offset where each value node starts, by its path (a sequence
    /// item's last segment is its index).
    nodes: HashMap<Vec<String>, usize>,
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

impl Doc<'_> {
    fn skip(&self, rewrite: &str, path: &str, why: &str) {
        self.skipped
            .borrow_mut()
            .push(format!("{rewrite}: `{path}` ({why})"));
    }

    fn key(&self, path: &[String]) -> Option<Range<usize>> {
        self.keys.get(path).cloned()
    }

    fn line_of(&self, pos: usize) -> usize {
        self.src[..pos].matches('\n').count() + 1
    }

    fn line_start(&self, pos: usize) -> usize {
        self.src[..pos].rfind('\n').map_or(0, |i| i + 1)
    }

    fn line_end(&self, pos: usize) -> usize {
        self.src[pos..]
            .find('\n')
            .map_or(self.src.len(), |i| pos + i)
    }

    /// The key at `range` is the first token on its line (a block mapping
    /// key not behind a `- `).
    fn starts_line(&self, range: &Range<usize>) -> bool {
        self.src[self.line_start(range.start)..range.start]
            .chars()
            .all(|c| c == ' ')
    }

    /// Where the value of the key at `range` starts: past the `:` and any
    /// blanks. At a line end (or a comment) when the value is a block.
    fn value_start(&self, range: &Range<usize>) -> usize {
        let b = self.src.as_bytes();
        let mut i = range.end;
        while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
            i += 1;
        }
        if i < b.len() && b[i] == b':' {
            i += 1;
        }
        while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
            i += 1;
        }
        i
    }

    /// The end of a value written on its key's line (a scalar, a quoted
    /// scalar, or a flow collection, which may span lines). Equal to
    /// `start` when the value is a block on the following lines.
    fn inline_value_end(&self, start: usize, in_flow: bool) -> usize {
        let b = self.src.as_bytes();
        if start >= b.len() || b[start] == b'\n' || b[start] == b'#' {
            return start;
        }
        match b[start] {
            b'[' | b'{' => {
                let mut depth = 0usize;
                let mut i = start;
                while i < b.len() {
                    match b[i] {
                        b'[' | b'{' => depth += 1,
                        b']' | b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                return i + 1;
                            }
                        }
                        b'"' | b'\'' => i = quoted_end(b, i) - 1,
                        b'#' if i > 0 && (b[i - 1] == b' ' || b[i - 1] == b'\n') => {
                            i = self.line_end(i) - 1;
                        }
                        _ => {}
                    }
                    i += 1;
                }
                b.len()
            }
            b'"' | b'\'' => quoted_end(b, start),
            _ => {
                let mut i = start;
                while i < b.len() && b[i] != b'\n' {
                    if b[i] == b'#' && b[i - 1] == b' ' {
                        break;
                    }
                    if in_flow && matches!(b[i], b',' | b']' | b'}') {
                        break;
                    }
                    i += 1;
                }
                while i > start && (b[i - 1] == b' ' || b[i - 1] == b'\t') {
                    i -= 1;
                }
                i
            }
        }
    }

    /// The end of the block value of the key at `range`: just past the
    /// newline of the last line indented deeper than the key (or a `- ` item
    /// at the key's own indent). Trailing blank and comment lines are not
    /// part of it.
    fn block_end(&self, range: &Range<usize>) -> usize {
        let col = range.start - self.line_start(range.start);
        let mut end = (self.line_end(range.start) + 1).min(self.src.len());
        let mut at = end;
        while at < self.src.len() {
            let le = self.line_end(at);
            let line = &self.src[at..le];
            let body = line.trim_start_matches(' ');
            let ind = line.len() - body.len();
            let next = (le + 1).min(self.src.len());
            if body.is_empty() || body.starts_with('#') {
                at = next;
                continue;
            }
            if ind > col || (ind == col && (body == "-" || body.starts_with("- "))) {
                end = next;
                at = next;
                continue;
            }
            break;
        }
        end
    }

    /// The indent of the first content line inside the block of the key at
    /// `range`, when it is a block mapping on the following lines.
    fn child_indent(&self, range: &Range<usize>) -> Option<usize> {
        let end = self.block_end(range);
        let mut at = (self.line_end(range.start) + 1).min(self.src.len());
        while at < end {
            let le = self.line_end(at);
            let line = &self.src[at..le];
            let body = line.trim_start_matches(' ');
            if !body.is_empty() && !body.starts_with('#') {
                return Some(line.len() - body.len());
            }
            at = (le + 1).min(self.src.len());
        }
        None
    }

    /// The range that deletes the entry whose key is at `range` from its
    /// mapping: its whole lines in a block mapping, `key: value, ` in a flow
    /// mapping. `None` when neither shape applies.
    fn entry_range(&self, path: &[String]) -> Option<Range<usize>> {
        let r = self.key(path)?;
        if self.flow.contains(path) {
            let b = self.src.as_bytes();
            let ve = self.inline_value_end(self.value_start(&r), true);
            let mut i = ve;
            while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n') {
                i += 1;
            }
            if i < b.len() && b[i] == b',' {
                i += 1;
                while i < b.len() && matches!(b[i], b' ' | b'\t') {
                    i += 1;
                }
                return Some(r.start..i);
            }
            // The last entry: take the comma before it instead.
            let mut j = r.start;
            while j > 0 && matches!(b[j - 1], b' ' | b'\t' | b'\n') {
                j -= 1;
            }
            if j > 0 && b[j - 1] == b',' {
                return Some(j - 1..ve);
            }
            return Some(r.start..ve);
        }
        if !self.starts_line(&r) {
            return None;
        }
        Some(self.line_start(r.start)..self.block_end(&r))
    }

    fn edit(&self, rewrite: &'static str, path: &str, range: Range<usize>, text: String) -> Edit {
        Edit {
            rewrite,
            path: path.to_string(),
            line: self.line_of(range.start),
            range,
            replacement: text,
        }
    }
}

/// The end (exclusive) of a quoted scalar starting at `start`.
fn quoted_end(b: &[u8], start: usize) -> usize {
    let q = b[start];
    let mut i = start + 1;
    while i < b.len() {
        if q == b'"' && b[i] == b'\\' {
            i += 2;
            continue;
        }
        if b[i] == q {
            if q == b'\'' && i + 1 < b.len() && b[i + 1] == b'\'' {
                i += 2;
                continue;
            }
            return i + 1;
        }
        i += 1;
    }
    b.len()
}

/// A string as a flow-context scalar: plain when that reads back as the
/// same string, double-quoted otherwise.
fn flow_scalar(s: &str) -> String {
    let plain = !s.is_empty()
        && !s.starts_with([
            '-', '?', ':', '&', '*', '!', '|', '>', '\'', '"', '%', '@', '`', ' ',
        ])
        && !s.ends_with(' ')
        && !s.contains([',', '[', ']', '{', '}', '#', '\n'])
        && !s.contains(": ")
        && !matches!(
            s,
            "true" | "false" | "null" | "~" | "yes" | "no" | "on" | "off" | "True" | "False"
        )
        && s.parse::<f64>().is_err();
    if plain {
        s.to_string()
    } else {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn flow_list(items: &[String]) -> String {
    format!(
        "[{}]",
        items
            .iter()
            .map(|s| flow_scalar(s))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// The strings of a scalar or a list of scalars.
fn yaml_strings(v: &Yaml) -> Option<Vec<String>> {
    match v {
        Yaml::Array(a) => a.iter().map(key_str).collect(),
        other => key_str(other).map(|s| vec![s]),
    }
}

/// Every manifest level in the document: the root, and each inline include
/// (recursively), with its key path.
fn manifest_levels(doc: &Doc) -> Vec<(Vec<String>, Yaml)> {
    fn walk(path: Vec<String>, y: &Yaml, out: &mut Vec<(Vec<String>, Yaml)>) {
        out.push((path.clone(), y.clone()));
        if let Some(inc) = y["includes"].as_hash() {
            for (k, v) in inc {
                if let Some(name) = key_str(k)
                    && v.as_hash().is_some()
                    && v["manifest"].is_badvalue()
                {
                    let mut p = path.clone();
                    p.extend(["includes".to_string(), name]);
                    walk(p, v, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(Vec::new(), &doc.yaml, &mut out);
    out
}

fn with(path: &[String], tail: &[&str]) -> Vec<String> {
    let mut p = path.to_vec();
    p.extend(tail.iter().map(|s| s.to_string()));
    p
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
        if !v["enters"].is_badvalue() || !v["leaves"].is_badvalue() {
            continue; // both written: a parse error the checker reports
        }
        let Some(r) = v["reaction"].as_str() else {
            continue;
        };
        if modes.iter().any(|m| m == r)
            && let Some(e) = doc.rename_key(
                "hazard-reaction-enters",
                &["hazards", &name, "reaction"],
                "leaves",
            )
        {
            out.push(e);
        }
    }
    out
}

/// Phase 98 N1: `hazards.<h>.severity` becomes `integrity` and
/// `severity_levels` becomes `integrity_levels`, at every manifest level
/// (the root and each inline include). A level that writes both spellings
/// is refused by the parser and left for a hand edit.
fn severity_integrity(doc: &Doc) -> Vec<Edit> {
    const ID: &str = "severity-integrity";
    let mut out = Vec::new();
    let mut rename = |path: Vec<String>, new_key: &str| {
        let refs: Vec<&str> = path.iter().map(String::as_str).collect();
        out.extend(doc.rename_key(ID, &refs, new_key));
    };
    for (level, y) in manifest_levels(doc) {
        if !y["severity_levels"].is_badvalue() {
            let path = with(&level, &["severity_levels"]);
            if y["integrity_levels"].is_badvalue() {
                rename(path, "integrity_levels");
            } else {
                doc.skip(
                    ID,
                    &path.join("."),
                    "both spellings written: delete one by hand",
                );
            }
        }
        let Some(hazards) = y["hazards"].as_hash() else {
            continue;
        };
        for (k, h) in hazards {
            let Some(name) = key_str(k) else { continue };
            if h["severity"].is_badvalue() {
                continue;
            }
            let path = with(&level, &["hazards", &name, "severity"]);
            if h["integrity"].is_badvalue() {
                rename(path, "integrity");
            } else {
                doc.skip(
                    ID,
                    &path.join("."),
                    "both spellings written: delete one by hand",
                );
            }
        }
    }
    out
}

/// Phase 98: an `acknowledged:` entry naming a renamed rule by its old id
/// names the new one (`rule_names::RENAMED_RULES`). Root level only: an
/// acknowledgement is a root key.
fn rule_id_renamed(doc: &Doc) -> Vec<Edit> {
    const ID: &str = "rule-id-renamed";
    let Some(list) = doc.yaml["acknowledged"].as_vec() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (i, entry) in list.iter().enumerate() {
        let item = vec!["acknowledged".to_string(), i.to_string()];
        let renames = [
            entry["rule"].as_str().and_then(|r| {
                crate::rule_names::renamed(r)
                    .map(|_| ("rule", r, crate::rule_names::current(r).into_owned()))
            }),
            entry["at"].as_str().and_then(|a| {
                let now = crate::rule_names::current_path(a);
                (now != a).then_some(("at", a, now))
            }),
        ];
        for (key, old, new) in renames.into_iter().flatten() {
            let path = with(&item, &[key]);
            let dotted = path.join(".");
            let Some(&start) = doc.nodes.get(&path) else {
                doc.skip(ID, &dotted, "value token not found");
                continue;
            };
            // The item is a flow mapping when its value opens with `{`.
            let in_flow = doc.flow.contains(&item)
                || doc
                    .nodes
                    .get(&item)
                    .is_some_and(|&p| doc.src[p..].starts_with('{'));
            let end = doc.inline_value_end(start, in_flow);
            let token = &doc.src[start..end];
            if token.trim_matches(|c| c == '"' || c == '\'') != old {
                doc.skip(
                    ID,
                    &dotted,
                    "value spelled in a form the edit does not handle",
                );
                continue;
            }
            let text = match token.chars().next() {
                Some(q @ ('"' | '\'')) => format!("{q}{new}{q}"),
                _ => new,
            };
            out.push(doc.edit(ID, &dotted, start..end, text));
        }
    }
    out
}

/// Rename the key `old` to `new` in each mapping of `maps`, leaving one
/// that writes both for a hand edit (the parser refuses it).
fn rename_in(
    doc: &Doc,
    id: &'static str,
    maps: Vec<(Vec<String>, Yaml)>,
    old: &str,
    new: &str,
) -> Vec<Edit> {
    let mut out = Vec::new();
    for (mpath, m) in maps {
        if m[old].is_badvalue() {
            continue;
        }
        let path = with(&mpath, &[old]);
        if !m[new].is_badvalue() {
            doc.skip(
                id,
                &path.join("."),
                "both spellings written: delete one by hand",
            );
            continue;
        }
        let refs: Vec<&str> = path.iter().map(String::as_str).collect();
        out.extend(doc.rename_key(id, &refs, new));
    }
    out
}

/// Every `nodes.<n>.<side>.<e>` mapping, at every manifest level.
fn endpoint_maps(doc: &Doc, side: &str) -> Vec<(Vec<String>, Yaml)> {
    let mut out = Vec::new();
    for (level, y) in manifest_levels(doc) {
        let Some(nodes) = y["nodes"].as_hash() else {
            continue;
        };
        for (k, n) in nodes {
            let Some(node) = key_str(k) else { continue };
            let Some(eps) = n[side].as_hash() else {
                continue;
            };
            for (ek, e) in eps {
                if let Some(ep) = key_str(ek) {
                    out.push((with(&level, &["nodes", &node, side, &ep]), e.clone()));
                }
            }
        }
    }
    out
}

/// S3 (v0.3.19): `functions:` becomes `capabilities:` at every manifest
/// level.
fn functions_capabilities(doc: &Doc) -> Vec<Edit> {
    let maps = manifest_levels(doc);
    rename_in(doc, "functions-capabilities", maps, "functions", "capabilities")
}

/// Every `hazards.<h>` mapping, at every manifest level.
fn hazard_maps(doc: &Doc) -> Vec<(Vec<String>, Yaml)> {
    let mut maps = Vec::new();
    for (level, y) in manifest_levels(doc) {
        if let Some(hazards) = y["hazards"].as_hash() {
            for (k, h) in hazards {
                if let Some(name) = key_str(k) {
                    maps.push((with(&level, &["hazards", &name]), h.clone()));
                }
            }
        }
    }
    maps
}

/// S3 (v0.3.19): a hazard's `enters:` becomes `leaves:`.
fn hazard_enters_leaves(doc: &Doc) -> Vec<Edit> {
    rename_in(doc, "hazard-enters-leaves", hazard_maps(doc), "enters", "leaves")
}

/// S3 (v0.3.19): a client's `reaction:` naming a mode of the same manifest
/// level becomes `enters:`, in `on_timeout`, `on_timeout.then` and
/// `on_rejected`.
fn call_reaction_enters(doc: &Doc) -> Vec<Edit> {
    const ID: &str = "call-reaction-enters";
    let mut out = Vec::new();
    for (level, y) in manifest_levels(doc) {
        let modes: HashSet<String> = y["modes"]
            .as_hash()
            .map(|h| h.keys().filter_map(key_str).collect())
            .unwrap_or_default();
        if modes.is_empty() {
            continue;
        }
        let Some(nodes) = y["nodes"].as_hash() else {
            continue;
        };
        for (k, n) in nodes {
            let Some(node) = key_str(k) else { continue };
            let Some(eps) = n["cli"].as_hash() else {
                continue;
            };
            for (ek, e) in eps {
                let Some(ep) = key_str(ek) else { continue };
                for (tail, m) in [
                    (vec!["on_timeout"], &e["on_timeout"]),
                    (vec!["on_timeout", "then"], &e["on_timeout"]["then"]),
                    (vec!["on_rejected"], &e["on_rejected"]),
                ] {
                    let Some(r) = m["reaction"].as_str() else {
                        continue;
                    };
                    if !modes.contains(r) {
                        continue;
                    }
                    let mut path = with(&level, &["nodes", &node, "cli", &ep]);
                    path.extend(tail.iter().map(|s| s.to_string()));
                    path.push("reaction".to_string());
                    if !m["enters"].is_badvalue() {
                        doc.skip(ID, &path.join("."), "both spellings written: delete one by hand");
                        continue;
                    }
                    let refs: Vec<&str> = path.iter().map(String::as_str).collect();
                    out.extend(doc.rename_key(ID, &refs, "enters"));
                }
            }
        }
    }
    out
}

/// Phase 98 N6 M2: `hazards.<h>.guards` becomes `monitors`.
fn guards_monitors(doc: &Doc) -> Vec<Edit> {
    let mut maps = Vec::new();
    for (level, y) in manifest_levels(doc) {
        if let Some(hazards) = y["hazards"].as_hash() {
            for (k, h) in hazards {
                if let Some(name) = key_str(k) {
                    maps.push((with(&level, &["hazards", &name]), h.clone()));
                }
            }
        }
    }
    rename_in(doc, "guards-monitors", maps, "guards", "monitors")
}

/// Phase 98 N6 M4: a subscriber's `ingress` becomes `arrival_curve`.
fn ingress_arrival_curve(doc: &Doc) -> Vec<Edit> {
    rename_in(
        doc,
        "ingress-arrival-curve",
        endpoint_maps(doc, "sub"),
        "ingress",
        "arrival_curve",
    )
}

/// Phase 98 N6 M5: a publisher's `on_demand` becomes `aperiodic`, and a
/// path's `trigger: spontaneous` becomes `trigger: aperiodic`.
fn aperiodic(doc: &Doc) -> Vec<Edit> {
    const ID: &str = "aperiodic";
    let mut out = rename_in(doc, ID, endpoint_maps(doc, "pub"), "on_demand", "aperiodic");
    for (level, y) in manifest_levels(doc) {
        let mut maps: Vec<(Vec<String>, Yaml)> =
            vec![(with(&level, &["paths"]), y["paths"].clone())];
        if let Some(nodes) = y["nodes"].as_hash() {
            for (k, v) in nodes {
                if let Some(n) = key_str(k) {
                    maps.push((with(&level, &["nodes", &n, "paths"]), v["paths"].clone()));
                }
            }
        }
        for (mpath, paths) in maps {
            let Some(paths) = paths.as_hash() else {
                continue;
            };
            for (k, p) in paths {
                let Some(name) = key_str(k) else { continue };
                if p["trigger"].as_str() != Some("spontaneous") {
                    continue;
                }
                let item = with(&mpath, &[&name]);
                let path = with(&item, &["trigger"]);
                let dotted = path.join(".");
                let Some(&start) = doc.nodes.get(&path) else {
                    doc.skip(ID, &dotted, "value token not found");
                    continue;
                };
                let in_flow = doc.flow.contains(&item)
                    || doc
                        .nodes
                        .get(&item)
                        .is_some_and(|&p| doc.src[p..].starts_with('{'));
                let end = doc.inline_value_end(start, in_flow);
                let token = &doc.src[start..end];
                if token.trim_matches(|c| c == '"' || c == '\'') != "spontaneous" {
                    doc.skip(
                        ID,
                        &dotted,
                        "value spelled in a form the edit does not handle",
                    );
                    continue;
                }
                let text = match token.chars().next() {
                    Some(q @ ('"' | '\'')) => format!("{q}aperiodic{q}"),
                    _ => "aperiodic".to_string(),
                };
                out.push(doc.edit(ID, &dotted, start..end, text));
            }
        }
    }
    out
}

/// Phase 98 S4: `modes.<m>.window` becomes `min_dwell`, at every manifest
/// level. A mode that writes both is refused by the parser and left.
fn window_min_dwell(doc: &Doc) -> Vec<Edit> {
    const ID: &str = "window-min-dwell";
    let mut out = Vec::new();
    for (level, y) in manifest_levels(doc) {
        let Some(modes) = y["modes"].as_hash() else {
            continue;
        };
        for (k, m) in modes {
            let Some(name) = key_str(k) else { continue };
            if m["window"].is_badvalue() {
                continue;
            }
            let path = with(&level, &["modes", &name, "window"]);
            if !m["min_dwell"].is_badvalue() {
                doc.skip(
                    ID,
                    &path.join("."),
                    "both spellings written: delete one by hand",
                );
                continue;
            }
            let refs: Vec<&str> = path.iter().map(String::as_str).collect();
            out.extend(doc.rename_key(ID, &refs, "min_dwell"));
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

/// G3: a path's legacy `input:` list becomes `trigger: { input: [...] }`.
/// Beside a written `trigger:` the list is deleted: the parser already
/// requires it to agree with a `trigger: { input }`, and beside any other
/// trigger it is read by nothing.
fn path_input_trigger(doc: &Doc) -> Vec<Edit> {
    const ID: &str = "path-input-trigger";
    let mut out = Vec::new();
    for (level, y) in manifest_levels(doc) {
        let mut maps: Vec<(Vec<String>, Yaml)> =
            vec![(with(&level, &["paths"]), y["paths"].clone())];
        if let Some(nodes) = y["nodes"].as_hash() {
            for (k, v) in nodes {
                if let Some(n) = key_str(k) {
                    maps.push((with(&level, &["nodes", &n, "paths"]), v["paths"].clone()));
                }
            }
        }
        for (mpath, paths) in maps {
            let Some(paths) = paths.as_hash() else {
                continue;
            };
            for (k, p) in paths {
                let Some(name) = key_str(k) else { continue };
                if p["input"].is_badvalue() {
                    continue;
                }
                let kpath = with(&mpath, &[&name, "input"]);
                let dotted = kpath.join(".");
                let Some(r) = doc.key(&kpath) else {
                    doc.skip(ID, &dotted, "key token not found");
                    continue;
                };
                if !p["trigger"].is_badvalue() {
                    match doc.entry_range(&kpath) {
                        Some(del) => out.push(doc.edit(ID, &dotted, del, String::new())),
                        None => doc.skip(
                            ID,
                            &dotted,
                            "cannot delete the legacy list beside `trigger:`",
                        ),
                    }
                    continue;
                }
                let Some(items) = yaml_strings(&p["input"]).filter(|i| !i.is_empty()) else {
                    doc.skip(
                        ID,
                        &dotted,
                        "an empty or non-scalar list: delete it by hand",
                    );
                    continue;
                };
                let in_flow = doc.flow.contains(&kpath);
                let vs = doc.value_start(&r);
                let ve = doc.inline_value_end(vs, in_flow);
                if ve > vs {
                    let text = &doc.src[vs..ve];
                    let list = if text.starts_with('[') {
                        text.to_string()
                    } else {
                        format!("[{text}]")
                    };
                    out.push(doc.edit(
                        ID,
                        &dotted,
                        r.start..ve,
                        format!("trigger: {{ input: {list} }}"),
                    ));
                } else if !in_flow && doc.starts_line(&r) {
                    // A block list: the items become a flow list on the key's line.
                    let mut end = doc.block_end(&r);
                    if end > 0 && doc.src.as_bytes()[end - 1] == b'\n' {
                        end -= 1;
                    }
                    out.push(doc.edit(
                        ID,
                        &dotted,
                        r.start..end,
                        format!("trigger: {{ input: {} }}", flow_list(&items)),
                    ));
                } else {
                    doc.skip(
                        ID,
                        &dotted,
                        "a value shape the textual edit does not handle",
                    );
                }
            }
        }
    }
    out
}

/// G3: the `external_topics:` section moves under `topics:`. Without a
/// `topics:` section the section key is renamed and each entry's `side:`
/// becomes `external:`. Otherwise each entry is appended to the `topics:`
/// block (re-indented), or, when that topic is already there, `external:`
/// (and a `type:` the topic lacks) is added to it; the section is then
/// deleted. Anything else is left for a hand edit, the whole section at
/// once, so a half-moved section is never written.
fn external_topics_to_topics(doc: &Doc) -> Vec<Edit> {
    const ID: &str = "external-topics-to-topics";
    let mut out = Vec::new();
    for (level, y) in manifest_levels(doc) {
        let Some(ext) = y["external_topics"].as_hash() else {
            continue;
        };
        let spath = with(&level, &["external_topics"]);
        let sdot = spath.join(".");
        let Some(sr) = doc.key(&spath) else {
            doc.skip(ID, &sdot, "key token not found");
            continue;
        };
        if ext.is_empty() {
            match doc.entry_range(&spath) {
                Some(del) => out.push(doc.edit(ID, &sdot, del, String::new())),
                None => doc.skip(ID, &sdot, "cannot delete the empty section"),
            }
            continue;
        }
        let names: Vec<String> = ext.keys().filter_map(key_str).collect();
        // `side:` -> `external:` inside each entry, as edits on the source.
        let side_edits = || -> Option<Vec<(Range<usize>, &'static str)>> {
            let mut v = Vec::new();
            for n in &names {
                let sp = with(&spath, &[n, "side"]);
                let e = &y["external_topics"][n.as_str()];
                if !e["side"].is_badvalue() {
                    if !e["external"].is_badvalue() {
                        return None; // both spellings: renaming would duplicate a key
                    }
                    let r = doc.key(&sp)?;
                    let tok = &doc.src[r.clone()];
                    v.push((
                        r,
                        if tok.starts_with('"') {
                            "\"external\""
                        } else if tok.starts_with('\'') {
                            "'external'"
                        } else {
                            "external"
                        },
                    ));
                }
            }
            Some(v)
        };
        let Some(sides) = side_edits() else {
            doc.skip(
                ID,
                &sdot,
                "an entry writing both `side:` and `external:`, or a `side:` key token not found",
            );
            continue;
        };
        let topics = &y["topics"];
        // A topic must have a type; an external entry need not. An entry
        // with none moves only into a topic that has one.
        let untyped: Vec<&String> = names
            .iter()
            .filter(|n| {
                key_str(&y["external_topics"][n.as_str()]["type"]).is_none()
                    && topics.as_hash().is_none_or(|th| {
                        !th.keys().filter_map(key_str).any(|t| {
                            t == **n
                                || (level.is_empty()
                                    && t.trim_start_matches('/') == n.trim_start_matches('/'))
                        })
                    })
            })
            .collect();
        if !untyped.is_empty() {
            doc.skip(
                ID,
                &sdot,
                &format!(
                    "{} states no `type:`, which a topic needs: add it, then migrate",
                    untyped
                        .iter()
                        .map(|n| format!("`{n}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );
            continue;
        }
        if topics.is_badvalue() {
            // No `topics:` here: the section becomes it.
            let tok = &doc.src[sr.clone()];
            let rep = if tok.starts_with('"') {
                "\"topics\""
            } else if tok.starts_with('\'') {
                "'topics'"
            } else {
                "topics"
            };
            out.push(doc.edit(ID, &sdot, sr.clone(), rep.to_string()));
            for (r, t) in sides {
                out.push(doc.edit(ID, &sdot, r, t.to_string()));
            }
            continue;
        }
        let tpath = with(&level, &["topics"]);
        let (Some(tr), Some(th)) = (doc.key(&tpath), topics.as_hash()) else {
            doc.skip(ID, &sdot, "`topics:` is not a mapping");
            continue;
        };
        if doc.flow.contains(&spath)
            || doc.flow.contains(&tpath)
            || !doc.starts_line(&sr)
            || !doc.starts_line(&tr)
        {
            doc.skip(ID, &sdot, "a flow-style section: move it by hand");
            continue;
        }
        let Some(tind) = doc.child_indent(&tr) else {
            doc.skip(ID, &sdot, "`topics:` has no block entries");
            continue;
        };
        // `x` and `/x` name one topic at the root namespace.
        let same = |a: &str, b: &str| {
            a == b || (level.is_empty() && a.trim_start_matches('/') == b.trim_start_matches('/'))
        };
        let mut edits = Vec::new();
        let mut appended = String::new();
        let mut ok = true;
        for n in &names {
            let e = &y["external_topics"][n.as_str()];
            let side = if e["side"].is_badvalue() {
                &e["external"]
            } else {
                &e["side"]
            };
            let Some(side) = key_str(side) else {
                ok = false;
                break;
            };
            let existing = th.keys().filter_map(key_str).find(|t| same(t, n));
            if let Some(t) = existing {
                let tv = &topics[t.as_str()];
                let tk = with(&tpath, &[&t]);
                let Some(tkr) = doc.key(&tk) else {
                    ok = false;
                    break;
                };
                if !tv["external"].is_badvalue()
                    && key_str(&tv["external"]).as_deref() != Some(side.as_str())
                {
                    ok = false; // two sides: by hand
                    break;
                }
                if !e["qos"].is_badvalue() {
                    ok = false; // merging a QoS block: by hand
                    break;
                }
                let mut add = Vec::new();
                if tv["external"].is_badvalue() {
                    add.push(format!("external: {side}"));
                }
                if let Some(ty) = key_str(&e["type"]) {
                    match key_str(&tv["type"]) {
                        Some(tt) if tt == ty => {}
                        Some(_) => {
                            ok = false; // the checker's `consistency` error: by hand
                            break;
                        }
                        None => add.push(format!("type: {}", flow_scalar(&ty))),
                    }
                }
                if add.is_empty() {
                    continue;
                }
                if doc.flow.contains(&with(&tk, &["type"])) || tv.as_hash().is_none() {
                    // `t: { ... }` or `t: {}` on one line: insert after the brace.
                    let vs = doc.value_start(&tkr);
                    if doc.src.as_bytes().get(vs) != Some(&b'{') {
                        ok = false;
                        break;
                    }
                    let empty = tv.as_hash().is_some_and(|h| h.is_empty());
                    let text = if empty {
                        format!(" {} ", add.join(", "))
                    } else {
                        format!(" {},", add.join(", "))
                    };
                    edits.push(doc.edit(ID, &sdot, vs + 1..vs + 1, text));
                } else {
                    let Some(ci) = doc.child_indent(&tkr) else {
                        ok = false;
                        break;
                    };
                    let at = (doc.line_end(tkr.start) + 1).min(doc.src.len());
                    let pad = " ".repeat(ci);
                    let text: String = add.iter().map(|a| format!("{pad}{a}\n")).collect();
                    edits.push(doc.edit(ID, &sdot, at..at, text));
                }
            } else {
                // Move the entry's lines under `topics:`.
                let ep = with(&spath, &[n]);
                let (Some(er), Some(range)) = (doc.key(&ep), doc.entry_range(&ep)) else {
                    ok = false;
                    break;
                };
                let eind = er.start - doc.line_start(er.start);
                let mut text = String::new();
                let mut at = range.start;
                for (r, t) in sides.iter().filter(|(r, _)| range.contains(&r.start)) {
                    text.push_str(&doc.src[at..r.start]);
                    text.push_str(t);
                    at = r.end;
                }
                text.push_str(&doc.src[at..range.end]);
                for line in text.split_inclusive('\n') {
                    let body = line.trim_start_matches(' ');
                    let ind = line.len() - body.len();
                    if body.trim().is_empty() {
                        appended.push_str(line.trim_start_matches(' '));
                    } else if ind >= eind {
                        appended.push_str(&" ".repeat(tind + ind - eind));
                        appended.push_str(body);
                    } else {
                        appended.push_str(line);
                    }
                }
                if !appended.ends_with('\n') {
                    appended.push('\n');
                }
            }
        }
        if !ok {
            doc.skip(ID, &sdot, "an entry that cannot be moved mechanically (two sides, two types, a QoS to merge, or a flow-style entry): move it by hand");
            continue;
        }
        let Some(del) = doc.entry_range(&spath) else {
            doc.skip(ID, &sdot, "cannot delete the section");
            continue;
        };
        if !appended.is_empty() {
            let at = doc.block_end(&tr);
            let mut text = appended;
            if at > 0 && doc.src.as_bytes()[at - 1] != b'\n' {
                text.insert(0, '\n');
            }
            edits.push(doc.edit(ID, &sdot, at..at, text));
        }
        // A blank line that separated the section from what follows goes too.
        let mut del = del;
        let b = doc.src.as_bytes();
        if del.end < b.len() && b[del.end] == b'\n' && (del.start == 0 || b[del.start - 1] == b'\n')
        {
            let before_blank = del.start >= 2 && b[del.start - 2] == b'\n';
            if before_blank {
                del.end += 1;
            }
        }
        edits.push(doc.edit(ID, &sdot, del, String::new()));
        out.extend(edits);
    }
    out
}

impl Doc<'_> {
    /// The value text of the key at `path` when it is written on the key's
    /// line (a scalar or a flow collection).
    fn inline_value(&self, path: &[String]) -> Option<String> {
        let r = self.key(path)?;
        let vs = self.value_start(&r);
        let ve = self.inline_value_end(vs, self.flow.contains(path));
        (ve > vs).then(|| self.src[vs..ve].to_string())
    }

    /// An insertion that adds `entry` (`key: value`) to the mapping whose
    /// key is at `path`: before the closing brace of a flow mapping, or as
    /// a new last line of a block mapping. `None` when the value has
    /// neither shape (a null value, a block mapping with no entries).
    fn add_entry(&self, path: &[String], entry: &str) -> Option<(usize, String)> {
        let r = self.key(path)?;
        let vs = self.value_start(&r);
        let b = self.src.as_bytes();
        if b.get(vs) == Some(&b'{') {
            let ve = self.inline_value_end(vs, true);
            let mut j = ve - 1; // the closing brace
            while j > vs + 1 && matches!(b[j - 1], b' ' | b'\t' | b'\n') {
                j -= 1;
            }
            return Some(if j == vs + 1 {
                (vs + 1, format!(" {entry} "))
            } else {
                (j, format!(", {entry}"))
            });
        }
        if vs < b.len() && b[vs] != b'\n' && b[vs] != b'#' {
            return None; // a scalar
        }
        if !self.starts_line(&r) {
            return None;
        }
        let ci = self.child_indent(&r)?;
        let at = self.block_end(&r);
        let mut text = format!("{}{entry}\n", " ".repeat(ci));
        if at > 0 && b[at - 1] != b'\n' {
            text.insert(0, '\n');
        }
        Some((at, text))
    }
}

/// A YAML value in flow style: what a moved block value is written as.
fn flow_value(v: &Yaml) -> Option<String> {
    match v {
        Yaml::Hash(h) => {
            let mut parts = Vec::new();
            for (k, v) in h {
                parts.push(format!("{}: {}", flow_scalar(&key_str(k)?), flow_value(v)?));
            }
            Some(format!("{{ {} }}", parts.join(", ")))
        }
        Yaml::Array(a) => Some(format!(
            "[{}]",
            a.iter()
                .map(flow_value)
                .collect::<Option<Vec<_>>>()?
                .join(", ")
        )),
        Yaml::String(s) => Some(flow_scalar(s)),
        other => key_str(other),
    }
}

/// play_launch phase 104 S1: a path's `tolerance:` is the same quantity as
/// `sync.max_interval`, so it folds into an existing `sync:`. Only there:
/// writing a `sync:` where none was changes the derived rate (the SUM of
/// the inputs becomes their MIN), which no rewrite may do silently.
fn tolerance_sync(doc: &Doc) -> Vec<Edit> {
    const ID: &str = "tolerance-sync";
    let mut out = Vec::new();
    for (level, y) in manifest_levels(doc) {
        let Some(nodes) = y["nodes"].as_hash() else {
            continue;
        };
        for (nk, node) in nodes {
            let Some(nname) = key_str(nk) else { continue };
            let Some(paths) = node["paths"].as_hash() else {
                continue;
            };
            for (pk, p) in paths {
                let Some(pname) = key_str(pk) else { continue };
                if p["tolerance"].is_badvalue() {
                    continue;
                }
                let ppath = with(&level, &["nodes", &nname, "paths", &pname]);
                let kpath = with(&ppath, &["tolerance"]);
                let dotted = kpath.join(".");
                let Some(value) = doc.inline_value(&kpath) else {
                    doc.skip(ID, &dotted, "a block-form value: move it by hand");
                    continue;
                };
                if p["sync"].is_badvalue() {
                    doc.skip(
                        ID,
                        &dotted,
                        &format!(
                            "the path has no `sync:`, so no synchroniser waits and the key \
                             bounded nothing. If it does synchronise, add `sync: {{ policy: \
                             approximate, max_interval: {value} }}` (its derived output rate \
                             becomes the MIN of its inputs, not their SUM); otherwise delete it"
                        ),
                    );
                    continue;
                }
                let spath = with(&ppath, &["sync"]);
                let mut mine = Vec::new();
                if p["sync"]["max_interval"].is_badvalue() {
                    match doc.add_entry(&spath, &format!("max_interval: {value}")) {
                        Some((at, text)) => mine.push(doc.edit(ID, &dotted, at..at, text)),
                        None => {
                            doc.skip(ID, &dotted, "`sync:` takes no entry here: move it by hand");
                            continue;
                        }
                    }
                } else {
                    let mi = doc.inline_value(&with(&spath, &["max_interval"]));
                    if mi.as_deref().map(str::trim) != Some(value.trim()) {
                        doc.skip(
                            ID,
                            &dotted,
                            &format!(
                                "`sync.max_interval` is {} and `tolerance` is {value}: one \
                                 quantity, two numbers. Keep the one that is right and delete \
                                 `tolerance`",
                                mi.unwrap_or_else(|| "written in block form".to_string())
                            ),
                        );
                        continue;
                    }
                }
                let Some(del) = doc.entry_range(&kpath) else {
                    doc.skip(ID, &dotted, "cannot delete the key");
                    continue;
                };
                mine.push(doc.edit(ID, &dotted, del, String::new()));
                out.extend(mine);
            }
        }
    }
    out
}

/// A link name derived from `base`, not yet taken in `taken`.
fn fresh_link_name(base: &str, taken: &mut HashSet<String>) -> String {
    let base: String = base
        .trim_start_matches('/')
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut name = base.clone();
    let mut i = 2;
    while taken.contains(&name) {
        name = format!("{base}_{i}");
        i += 1;
    }
    taken.insert(name.clone());
    name
}

/// G3 and play_launch phase 104 S1: a transport is stated through a link.
///
/// A topic's `max_transport:` (refused since v0.3.15) becomes one link,
/// named after the topic, that each subscriber the topic lists names when
/// it states no transport of its own: exactly what the topic-level value
/// meant. Every listed subscriber must be declared in the same file, or the
/// topic is left for a hand edit. A subscriber's own `max_transport:`
/// (deprecated since v0.3.16) becomes a link of its own, `<node>_<ep>`, and
/// the key becomes `link:`. Both go in one rewrite because both add to the
/// level's one `links:` mapping, and two insertions at one place overlap.
fn max_transport_links(doc: &Doc) -> Vec<Edit> {
    const ID: &str = "max-transport-link";
    let mut out = Vec::new();
    for (level, y) in manifest_levels(doc) {
        let mut taken: HashSet<String> = y["links"]
            .as_hash()
            .map(|h| h.keys().filter_map(key_str).collect())
            .unwrap_or_default();
        // (link name, value text, the dotted path it came from)
        let mut links: Vec<(String, String, String)> = Vec::new();
        let mut edits = Vec::new();

        // A topic's value: one link, named on each listed subscriber.
        if let Some(topics) = y["topics"].as_hash() {
            for (k, t) in topics {
                let Some(tname) = key_str(k) else { continue };
                if t["max_transport"].is_badvalue() {
                    continue;
                }
                let kpath = with(&level, &["topics", &tname, "max_transport"]);
                let dotted = kpath.join(".");
                let Some(value) = doc.inline_value(&kpath) else {
                    doc.skip(ID, &dotted, "a block-form value: move it by hand");
                    continue;
                };
                let subs = yaml_strings(&t["sub"]).unwrap_or_default();
                if subs.is_empty() {
                    doc.skip(
                        ID,
                        &dotted,
                        &format!(
                            "`{tname}` lists no subscriber in this file: the value applies to \
                             subscribers declared in another contract of the launch (or to \
                             none). Write `links.<name>: {{ max_transport: {value} }}`, name it \
                             with `link: <name>` on each of them, then delete this key"
                        ),
                    );
                    continue;
                }
                let mut name: Option<String> = None;
                let mut mine = Vec::new();
                let mut why: Option<String> = None;
                for sref in &subs {
                    let Some((node, ep)) = sref.rsplit_once('/') else {
                        why = Some(format!("`{sref}` is not `<node>/<endpoint>`"));
                        break;
                    };
                    let props = &y["nodes"][node]["sub"][ep];
                    if y["nodes"][node]["sub"].as_hash().is_none() || props.is_badvalue() {
                        why = Some(format!(
                            "subscriber `{sref}` is not declared under `nodes.{node}.sub` in \
                             this file (another contract, or a list-form `sub:`)"
                        ));
                        break;
                    }
                    if !props["max_transport"].is_badvalue() || !props["link"].is_badvalue() {
                        continue; // its own value wins, as it did
                    }
                    let lname = name
                        .get_or_insert_with(|| fresh_link_name(&tname, &mut taken))
                        .clone();
                    let epath = with(&level, &["nodes", node, "sub", ep]);
                    match doc.add_entry(&epath, &format!("link: {lname}")) {
                        Some((at, text)) => mine.push(doc.edit(ID, &dotted, at..at, text)),
                        None => {
                            why = Some(format!(
                                "subscriber `{sref}` has no mapping to add the key to (a null \
                                 value): write it as `{ep}: {{ link: <name> }}`"
                            ));
                            break;
                        }
                    }
                }
                if let Some(why) = why {
                    if let Some(n) = name {
                        taken.remove(&n);
                    }
                    doc.skip(ID, &dotted, &why);
                    continue;
                }
                let Some(del) = doc.entry_range(&kpath) else {
                    doc.skip(ID, &dotted, "cannot delete the topic's key");
                    continue;
                };
                mine.push(doc.edit(ID, &dotted, del, String::new()));
                if let Some(n) = name {
                    links.push((n, value, dotted));
                }
                edits.extend(mine);
            }
        }

        // A subscriber's own value: a link of its own.
        if let Some(nodes) = y["nodes"].as_hash() {
            for (nk, node) in nodes {
                let Some(nname) = key_str(nk) else { continue };
                let Some(subs) = node["sub"].as_hash() else {
                    continue;
                };
                for (ek, props) in subs {
                    let Some(ep) = key_str(ek) else { continue };
                    if props["max_transport"].is_badvalue() || !props["link"].is_badvalue() {
                        continue; // none, or both (refused by the parser)
                    }
                    let kpath = with(&level, &["nodes", &nname, "sub", &ep, "max_transport"]);
                    let dotted = kpath.join(".");
                    let (Some(kr), Some(value)) = (doc.key(&kpath), doc.inline_value(&kpath))
                    else {
                        doc.skip(
                            ID,
                            &dotted,
                            "a block-form value: move it into a link by hand",
                        );
                        continue;
                    };
                    let vs = doc.value_start(&kr);
                    let ve = doc.inline_value_end(vs, doc.flow.contains(&kpath));
                    let lname = fresh_link_name(&format!("{nname}_{ep}"), &mut taken);
                    edits.push(doc.edit(ID, &dotted, kr, "link".to_string()));
                    edits.push(doc.edit(ID, &dotted, vs..ve, lname.clone()));
                    links.push((lname, value, dotted));
                }
            }
        }

        if links.is_empty() {
            out.extend(edits);
            continue;
        }
        let entries: Vec<String> = links
            .iter()
            .map(|(n, v, _)| format!("{n}: {{ max_transport: {v} }}"))
            .collect();
        let at_dotted = links[0].2.clone();
        let lpath = with(&level, &["links"]);
        let insertion = if y["links"].as_hash().is_some() {
            let mut first: Option<usize> = None;
            let mut text = String::new();
            for e in &entries {
                match doc.add_entry(&lpath, e) {
                    Some((at, t))
                        if first.is_none_or(|f| f == at)
                            && !(t.starts_with(' ') && t.ends_with(' ')) =>
                    {
                        first = Some(at);
                        text.push_str(&t);
                    }
                    _ => {
                        first = None;
                        break;
                    }
                }
            }
            first.map(|at| (at, text))
        } else if level.is_empty() {
            let b = doc.src.as_bytes();
            let mut text = String::new();
            if !b.is_empty() && b[b.len() - 1] != b'\n' {
                text.push('\n');
            }
            text.push_str("\nlinks:\n");
            for e in &entries {
                text.push_str(&format!("  {e}\n"));
            }
            Some((b.len(), text))
        } else {
            doc.add_entry(&level, &format!("links: {{ {} }}", entries.join(", ")))
        };
        let Some((at, text)) = insertion else {
            for (_, _, d) in &links {
                doc.skip(
                    ID,
                    d,
                    "the level's `links:` cannot take an entry (an empty flow mapping, or no \
                     block entries): write the link by hand",
                );
            }
            continue;
        };
        edits.push(doc.edit(ID, &at_dotted, at..at, text));
        out.extend(edits);
    }
    out
}

/// G3: a node path's `drop:` (read by nothing) moves to the topic its one
/// output publishes, where the rate rule, the loss detector and the
/// runtime drop monitor read it.
fn node_path_drop_topic(doc: &Doc) -> Vec<Edit> {
    const ID: &str = "node-path-drop-topic";
    let mut out = Vec::new();
    for (level, y) in manifest_levels(doc) {
        let Some(nodes) = y["nodes"].as_hash() else {
            continue;
        };
        for (nk, nv) in nodes {
            let Some(node) = key_str(nk) else { continue };
            let Some(paths) = nv["paths"].as_hash() else {
                continue;
            };
            for (pk, p) in paths {
                let Some(pname) = key_str(pk) else { continue };
                if p["drop"].is_badvalue() {
                    continue;
                }
                let kpath = with(&level, &["nodes", &node, "paths", &pname, "drop"]);
                let dotted = kpath.join(".");
                let outputs = yaml_strings(&p["output"]).unwrap_or_default();
                if outputs.len() != 1 {
                    doc.skip(
                        ID,
                        &dotted,
                        &if outputs.is_empty() {
                            "the path has no output: delete it, or state the loss on the topic \
                             that drops by hand"
                                .to_string()
                        } else {
                            format!(
                                "the path has {} outputs ({}): state the loss on the topic of \
                                 the output that drops (`topics.<t>.drop`) by hand",
                                outputs.len(),
                                outputs.join(", ")
                            )
                        },
                    );
                    continue;
                }
                let ep_ref = format!("{node}/{}", outputs[0]);
                let carriers: Vec<String> = y["topics"]
                    .as_hash()
                    .map(|th| {
                        th.iter()
                            .filter(|(_, tv)| {
                                yaml_strings(&tv["pub"]).is_some_and(|ps| ps.contains(&ep_ref))
                            })
                            .filter_map(|(tk, _)| key_str(tk))
                            .collect()
                    })
                    .unwrap_or_default();
                let [topic] = carriers.as_slice() else {
                    doc.skip(
                        ID,
                        &dotted,
                        &format!(
                            "{} topic(s) of this file list `{ep_ref}` under `pub:`: state the \
                             loss on the topic it publishes by hand",
                            carriers.len()
                        ),
                    );
                    continue;
                };
                let tv = &y["topics"][topic.as_str()];
                let Some(del) = doc.entry_range(&kpath) else {
                    doc.skip(ID, &dotted, "cannot delete the path's key");
                    continue;
                };
                if !tv["drop"].is_badvalue() {
                    if tv["drop"] == p["drop"] {
                        out.push(doc.edit(ID, &dotted, del, String::new()));
                    } else {
                        doc.skip(
                            ID,
                            &dotted,
                            &format!(
                                "topic `{topic}` states a different `drop:`: reconcile by hand"
                            ),
                        );
                    }
                    continue;
                }
                let Some(value) = doc.inline_value(&kpath).or_else(|| flow_value(&p["drop"]))
                else {
                    doc.skip(ID, &dotted, "a value the textual edit cannot carry");
                    continue;
                };
                let tpath = with(&level, &["topics", topic]);
                let Some((at, text)) = doc.add_entry(&tpath, &format!("drop: {value}")) else {
                    doc.skip(
                        ID,
                        &dotted,
                        &format!("topic `{topic}` is not a mapping to add to"),
                    );
                    continue;
                };
                out.push(doc.edit(ID, &dotted, at..at, text));
                out.push(doc.edit(ID, &dotted, del, String::new()));
            }
        }
    }
    out
}

/// G7: an inline `{ all_of: [...] }` guard becomes a function of the scope,
/// guarded by name. The function is named `<hazard>_all_of` (`_2`, `_3`, ...
/// for a second group, or when the name is taken), appended to the
/// `functions:` block, or to a new `functions:` section after `hazards:`.
fn guard_all_of_function(doc: &Doc) -> Vec<Edit> {
    const ID: &str = "guard-all-of-function";
    let mut out = Vec::new();
    for (level, y) in manifest_levels(doc) {
        let Some(hazards) = y["hazards"].as_hash() else {
            continue;
        };
        // `capabilities:` since v0.3.19; `functions:` read for one release.
        let fkey = if y["capabilities"].is_badvalue() && !y["functions"].is_badvalue() {
            "functions"
        } else {
            "capabilities"
        };
        let mut taken: HashSet<String> = y[fkey]
            .as_hash()
            .map(|h| h.keys().filter_map(key_str).collect())
            .unwrap_or_default();
        if let Some(t) = y["topics"].as_hash() {
            taken.extend(t.keys().filter_map(key_str));
        }
        let mut new_fns: Vec<(String, Vec<String>)> = Vec::new();
        let mut edits = Vec::new();
        for (k, h) in hazards {
            let Some(hname) = key_str(k) else { continue };
            // `monitors:` since phase 98 N6; `guards:` read for one release.
            let key = if h["monitors"].is_badvalue() {
                "guards"
            } else {
                "monitors"
            };
            let Some(guards) = h[key].as_vec() else {
                continue;
            };
            for (i, g) in guards.iter().enumerate() {
                if g.as_hash().is_none() || g["all_of"].is_badvalue() {
                    continue;
                }
                let gpath = with(&level, &["hazards", &hname, key, &i.to_string()]);
                let dotted = format!(
                    "{}.guards[{i}]",
                    with(&level, &["hazards", &hname]).join(".")
                );
                let Some(members) = yaml_strings(&g["all_of"]) else {
                    doc.skip(ID, &dotted, "members are not names");
                    continue;
                };
                let Some(&start) = doc.nodes.get(&gpath) else {
                    doc.skip(ID, &dotted, "item not found");
                    continue;
                };
                let b = doc.src.as_bytes();
                let (start, end) = if b.get(start) == Some(&b'{') {
                    (start, doc.inline_value_end(start, true))
                } else {
                    // `- all_of: [...]` as a block-mapping item of one key.
                    let akey = with(&gpath, &["all_of"]);
                    let Some(ar) = doc.key(&akey) else {
                        doc.skip(ID, &dotted, "item not found");
                        continue;
                    };
                    let vs = doc.value_start(&ar);
                    let ve = doc.inline_value_end(vs, false);
                    if ve == vs || g.as_hash().is_some_and(|h| h.len() != 1) {
                        doc.skip(ID, &dotted, "a multi-line group: rewrite by hand");
                        continue;
                    }
                    (ar.start, ve)
                };
                let mut name = format!("{hname}_all_of");
                let mut n = 2;
                while taken.contains(&name) {
                    name = format!("{hname}_all_of_{n}");
                    n += 1;
                }
                taken.insert(name.clone());
                edits.push(doc.edit(ID, &dotted, start..end, name.clone()));
                new_fns.push((name, members));
            }
        }
        if new_fns.is_empty() {
            continue;
        }
        let fpath = with(&level, &[fkey]);
        let hpath = with(&level, &["hazards"]);
        let lines = |ind: usize| -> String {
            new_fns
                .iter()
                .map(|(n, m)| format!("{}{n}: {{ all_of: {} }}\n", " ".repeat(ind), flow_list(m)))
                .collect()
        };
        let insert = if let Some(fr) = doc.key(&fpath) {
            match (
                doc.child_indent(&fr),
                doc.flow.contains(&fpath) || !doc.starts_line(&fr),
            ) {
                (Some(ci), false) => Some((doc.block_end(&fr), lines(ci))),
                _ => None,
            }
        } else if let Some(hr) = doc.key(&hpath).filter(|r| doc.starts_line(r)) {
            let col = hr.start - doc.line_start(hr.start);
            let ci = doc.child_indent(&hr).unwrap_or(col + 2);
            Some((
                doc.block_end(&hr),
                format!("{}capabilities:\n{}", " ".repeat(col), lines(ci)),
            ))
        } else {
            None
        };
        let Some((at, mut text)) = insert else {
            doc.skip(
                ID,
                &fpath.join("."),
                "no block `capabilities:` or `hazards:` to place the capability in",
            );
            continue;
        };
        if at > 0 && doc.src.as_bytes()[at - 1] != b'\n' {
            text.insert(0, '\n');
        }
        edits.push(doc.edit(ID, &fpath.join("."), at..at, text));
        out.extend(edits);
    }
    out
}

/// E8: a plain-number `entry_speed` (the core key's m/s, v0.1.46-v0.1.71)
/// gains its unit, as `vehicle.braking` reads a velocity.
fn entry_speed_unit(doc: &Doc) -> Vec<Edit> {
    const ID: &str = "entry-speed-unit";
    let mut out = Vec::new();
    for (level, y) in manifest_levels(doc) {
        let Some(hazards) = y["hazards"].as_hash() else {
            continue;
        };
        for (k, h) in hazards {
            let Some(name) = key_str(k) else { continue };
            let number = match &h["entry_speed"] {
                Yaml::Real(r) => r.clone(),
                Yaml::Integer(i) => i.to_string(),
                _ => continue,
            };
            let kpath = with(&level, &["hazards", &name, "entry_speed"]);
            let dotted = kpath.join(".");
            let Some(r) = doc.key(&kpath) else {
                doc.skip(ID, &dotted, "key token not found");
                continue;
            };
            let vs = doc.value_start(&r);
            let ve = doc.inline_value_end(vs, doc.flow.contains(&kpath));
            if doc.src[vs..ve].trim() != number {
                doc.skip(ID, &dotted, "the value is not written as a plain number");
                continue;
            }
            out.push(doc.edit(ID, &dotted, vs..ve, format!("{number} m/s")));
        }
    }
    out
}

/// E8: the `use:` entries a level needs so that nothing is loaded
/// implicitly: `vehicle.braking` when it writes the extension's keys, and
/// `iso26262` when a hazard states a severity on the implicit scale.
fn use_implicit(doc: &Doc) -> Vec<Edit> {
    const ID: &str = "use-implicit";
    let reference: Vec<String> = crate::profile::reference()
        .into_iter()
        .map(|p| p.name)
        .collect();
    let mut out = Vec::new();
    for (level, y) in manifest_levels(doc) {
        let used: Vec<String> = y["use"]
            .as_hash()
            .map(|h| h.keys().filter_map(key_str).collect())
            .unwrap_or_default();
        let dotted_level = if level.is_empty() {
            "use".to_string()
        } else {
            with(&level, &["use"]).join(".")
        };
        let hazards = y["hazards"].as_hash();
        let mut add: Vec<&str> = Vec::new();
        let vehicle_keys = hazards
            .is_some_and(|hs| hs.values().any(|h| !h["entry_speed"].is_badvalue()))
            || y["nodes"].as_hash().is_some_and(|ns| {
                ns.values().any(|n| {
                    n["paths"].as_hash().is_some_and(|ps| {
                        ps.values().any(|p| {
                            let st = &p["safe_state"]["settle"];
                            st.as_hash().is_some()
                                && (!st["decel"].is_badvalue() || !st["jerk"].is_badvalue())
                        })
                    })
                })
            });
        let ext = crate::extension::VEHICLE_BRAKING;
        if vehicle_keys && !used.iter().any(|u| u == ext) {
            // Another used extension may declare the key itself (`rail.ebd`
            // has an `entry_speed`), and then nothing loads implicitly:
            // which it is needs the extension, which a file does not hold.
            match used.iter().find(|u| u.contains('.')) {
                Some(other) => doc.skip(
                    ID,
                    &dotted_level,
                    &format!(
                        "uses `{other}`, which may declare `entry_speed`, `decel` or `jerk`: add \
                         `{ext}` by hand if the checker reports `deprecated-implicit-extension`"
                    ),
                ),
                None => add.push(ext),
            }
        }
        let integrity = hazards.is_some_and(|hs| {
            hs.values()
                .any(|h| !h["integrity"].is_badvalue() || !h["severity"].is_badvalue())
        });
        if integrity && y["integrity_levels"].is_badvalue() && y["severity_levels"].is_badvalue() {
            let profile = used.iter().find(|u| reference.contains(u));
            let maybe = used
                .iter()
                .find(|u| !u.contains('.') && !reference.contains(u));
            match (profile, maybe) {
                (Some(_), _) => {}
                (None, Some(m)) => doc.skip(
                    ID,
                    &dotted_level,
                    &format!("uses `{m}`, which may be a profile: add `iso26262` by hand if not"),
                ),
                (None, None) => add.push("iso26262"),
            }
        }
        if add.is_empty() {
            continue;
        }
        let upath = with(&level, &["use"]);
        let entry = |n: &str| format!("{n}: \"^1\"");
        let edit = if let Some(ur) = doc.key(&upath) {
            if doc.src.as_bytes().get(doc.value_start(&ur)) == Some(&b'{') {
                // `use: { a: "^1" }`: before the closing brace.
                let vs = doc.value_start(&ur);
                let ve = doc.inline_value_end(vs, false);
                let b = doc.src.as_bytes();
                if b.get(vs) != Some(&b'{') || ve < 2 || b[ve - 1] != b'}' {
                    doc.skip(ID, &dotted_level, "a `use:` that is not a map");
                    continue;
                }
                let mut at = ve - 1;
                while at > vs + 1 && b[at - 1] == b' ' {
                    at -= 1;
                }
                let empty = doc.src[vs + 1..at].trim().is_empty();
                let text: Vec<String> = add.iter().map(|n| entry(n)).collect();
                let text = if empty {
                    format!(" {} ", text.join(", "))
                } else {
                    format!(", {}", text.join(", "))
                };
                doc.edit(ID, &dotted_level, at..at, text)
            } else {
                let Some(ci) = doc.child_indent(&ur) else {
                    doc.skip(ID, &dotted_level, "an empty `use:`");
                    continue;
                };
                let at = doc.block_end(&ur);
                let mut text: String = add
                    .iter()
                    .map(|n| format!("{}{}\n", " ".repeat(ci), entry(n)))
                    .collect();
                if at > 0 && doc.src.as_bytes()[at - 1] != b'\n' {
                    text.insert(0, '\n');
                }
                doc.edit(ID, &dotted_level, at..at, text)
            }
        } else {
            // A new `use:` beside the level's first key: after the header
            // (`version:`, `rlm:`) at the root, else before the level's
            // first key.
            let first = doc
                .keys
                .iter()
                .filter(|(p, _)| p.len() == level.len() + 1 && p.starts_with(&level))
                .map(|(p, r)| (r.clone(), p.clone()))
                .min_by_key(|(r, _)| r.start);
            let Some((fr, _)) = first else {
                doc.skip(
                    ID,
                    &dotted_level,
                    "the level has no key to place `use:` beside",
                );
                continue;
            };
            if !doc.starts_line(&fr) {
                doc.skip(ID, &dotted_level, "a flow-mapping level");
                continue;
            }
            let col = fr.start - doc.line_start(fr.start);
            let header = ["version", "rlm"]
                .iter()
                .filter_map(|k| doc.key(&with(&level, &[k])))
                .filter(|r| doc.starts_line(r))
                .max_by_key(|r| r.start);
            let at = match header {
                Some(hr) => doc.block_end(&hr),
                None => doc.line_start(fr.start),
            };
            let mut text = format!("{}use:\n", " ".repeat(col));
            for n in &add {
                text.push_str(&format!("{}{}\n", " ".repeat(col + 2), entry(n)));
            }
            if at > 0 && doc.src.as_bytes()[at - 1] != b'\n' {
                text.insert(0, '\n');
            }
            doc.edit(ID, &dotted_level, at..at, text)
        };
        out.push(edit);
    }
    out
}

/// The `param` keys of the bundled extensions: (extension, context
/// (`hazard` / `settle`), key, the unit the parameter is read in).
fn bundled_param_keys() -> Vec<(String, String, String, String)> {
    let mut out = Vec::new();
    for (file, src) in crate::extension::BUNDLED {
        let Ok(checked) = rlx::check_source(src.as_bytes()) else {
            continue;
        };
        let ext = file.trim_end_matches(".rlx").to_string();
        for k in &checked.env.keys {
            let rlx::typeck::KeyTy::Param { dim, unit, .. } = &k.ty else {
                continue;
            };
            let Some((ctx, key)) = k.path.split_once('.') else {
                continue;
            };
            let unit = unit
                .as_ref()
                .map_or_else(|| rlx::units::example(*dim), |u| u.text.clone());
            out.push((ext.clone(), ctx.to_string(), key.to_string(), unit));
        }
    }
    out
}

/// Phase 94: `decel: target_acceleration` reads the parameter by bare name;
/// the read form says the unit (`decel: $(param target_acceleration)
/// m/s^2`), as every other value read from the launch tree does.
fn ext_param_read(doc: &Doc) -> Vec<Edit> {
    const ID: &str = "ext-param-read";
    let keys = bundled_param_keys();
    let is_name = |s: &str| {
        let mut c = s.chars();
        c.next()
            .is_some_and(|f| f.is_ascii_alphabetic() || f == '_')
            && c.all(|x| x.is_ascii_alphanumeric() || x == '_' || x == '.')
    };
    let mut out = Vec::new();
    // (path of the map, context word)
    let mut maps: Vec<(Vec<String>, &'static str)> = Vec::new();
    for (level, y) in manifest_levels(doc) {
        if let Some(hs) = y["hazards"].as_hash() {
            for k in hs.keys().filter_map(key_str) {
                maps.push((with(&level, &["hazards", &k]), "hazard"));
            }
        }
        let mut paths: Vec<Vec<String>> = Vec::new();
        if let Some(ns) = y["nodes"].as_hash() {
            for (nk, n) in ns {
                let Some(nk) = key_str(nk) else { continue };
                if let Some(ps) = n["paths"].as_hash() {
                    for pk in ps.keys().filter_map(key_str) {
                        paths.push(with(&level, &["nodes", &nk, "paths", &pk]));
                    }
                }
            }
        }
        if let Some(ps) = y["paths"].as_hash() {
            for pk in ps.keys().filter_map(key_str) {
                paths.push(with(&level, &["paths", &pk]));
            }
        }
        for p in paths {
            maps.push((with(&p, &["safe_state", "settle"]), "settle"));
        }
    }
    for (map, ctx) in maps {
        let mut y = &doc.yaml;
        for seg in &map {
            y = &y[seg.as_str()];
        }
        let Some(h) = y.as_hash() else { continue };
        for (k, v) in h {
            let Some(written) = key_str(k) else { continue };
            let (qual, bare) = match written.split_once('/') {
                Some((q, b)) => (Some(q), b),
                None => (None, written.as_str()),
            };
            let Some((_, _, _, unit)) = keys
                .iter()
                .find(|(e, c, kk, _)| c == ctx && kk == bare && qual.is_none_or(|q| q == e))
            else {
                continue;
            };
            let Some(name) = v.as_str().filter(|t| is_name(t)) else {
                continue;
            };
            let kpath = {
                let mut p = map.clone();
                p.push(written.clone());
                p
            };
            let dotted = kpath.join(".");
            let Some(r) = doc.key(&kpath) else {
                doc.skip(ID, &dotted, "key token not found");
                continue;
            };
            let vs = doc.value_start(&r);
            let ve = doc.inline_value_end(vs, doc.flow.contains(&kpath));
            if doc.src[vs..ve].trim() != name {
                doc.skip(ID, &dotted, "the value is not written as a plain name");
                continue;
            }
            out.push(doc.edit(ID, &dotted, vs..ve, format!("$(param {name}) {unit}")));
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
    let idx = key_index(src);
    let doc = Doc {
        src,
        yaml,
        keys: idx.keys,
        flow: idx.flow,
        nodes: idx.nodes,
        skipped: Default::default(),
    };
    let mut edits: Vec<Edit> = REWRITES.iter().flat_map(|r| (r.find)(&doc)).collect();
    edits.sort_by_key(|e| (e.range.start, e.range.end));
    for w in edits.windows(2) {
        if w[0].range.end > w[1].range.start
            || (w[0].range.start == w[1].range.start
                && w[0].range.is_empty() == w[1].range.is_empty())
        {
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
struct KeyIndex {
    keys: HashMap<Vec<String>, Range<usize>>,
    flow: HashSet<Vec<String>>,
    nodes: HashMap<Vec<String>, usize>,
}

fn key_index(src: &str) -> KeyIndex {
    let mut rx = KeyReceiver {
        src,
        char_to_byte: src.char_indices().map(|(b, _)| b).collect(),
        frames: Vec::new(),
        keys: HashMap::new(),
        flow: HashSet::new(),
        nodes: HashMap::new(),
    };
    let mut parser = Parser::new_from_str(src);
    let _ = parser.load(&mut rx, false);
    KeyIndex {
        keys: rx.keys,
        flow: rx.flow,
        nodes: rx.nodes,
    }
}

struct Frame {
    seg: String,
    map: bool,
    flow: bool,
    expect_key: bool,
    pending: Option<String>,
    idx: usize,
}

struct KeyReceiver<'a> {
    src: &'a str,
    char_to_byte: Vec<usize>,
    frames: Vec<Frame>,
    keys: HashMap<Vec<String>, Range<usize>>,
    flow: HashSet<Vec<String>>,
    nodes: HashMap<Vec<String>, usize>,
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
                let start = self.char_to_byte.get(mark.index()).copied();
                let flow = start
                    .and_then(|b| self.src.as_bytes().get(b))
                    .is_some_and(|c| matches!(c, b'{' | b'['));
                if let Some(b) = start {
                    let mut path = self.path();
                    path.push(seg.clone());
                    self.nodes.entry(path).or_insert(b);
                }
                self.frames.push(Frame {
                    seg,
                    map,
                    flow,
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
                        if self.frames.last().is_some_and(|f| f.flow) {
                            self.flow.insert(path.clone());
                        }
                        self.keys.entry(path).or_insert(r);
                    }
                    let f = self.frames.last_mut().unwrap();
                    f.pending = Some(value);
                    f.expect_key = false;
                } else {
                    if let Some(b) = self.char_to_byte.get(mark.index()).copied() {
                        let seg = match self.frames.last() {
                            Some(f) if f.map => f.pending.clone().unwrap_or_default(),
                            Some(f) => f.idx.to_string(),
                            None => String::new(),
                        };
                        let mut path = self.path();
                        path.push(seg);
                        self.nodes.entry(path).or_insert(b);
                    }
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
    min_dwell: 10s
    exit: { on: driver_took_over, to: manual }   # flow form
    reaction: sys.tor
  stop:
    reaction: sys.stop
  manual: {}
hazards:
  blind:
    integrity: ASIL_D
    monitors: [scan]
    reaction: driving    # a mode: becomes enters
  stale:
    monitors: [odom]
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
                ("use-implicit", "use", 2),
                ("exit-on-holds", "modes.tor.exit.on", 9),
                ("hazard-reaction-enters", "hazards.blind.reaction", 18),
            ]
        );
        // E8: `severity: ASIL_D` on the implicit scale names it.
        let want = LADDER
            .replace("version: 1\n", "version: 1\nuse:\n  iso26262: \"^1\"\n")
            .replace("exit: { on: driver", "exit: { holds: driver")
            .replace("reaction: driving ", "leaves: driving ");
        assert_eq!(m.output, want);
        assert!(m.skipped.is_empty());
        // Idempotent: a migrated file has nothing left to rewrite.
        assert!(!migrate_str(&m.output).unwrap().changed());
    }

    /// S3 (v0.3.19): `functions:` is `capabilities:`, a hazard's `enters:`
    /// is `leaves:`, and a client's `reaction:` naming a mode is `enters:`;
    /// a client's event chain stays `reaction:`.
    #[test]
    fn s3_renames_capabilities_leaves_and_call_enters() {
        let src = "version: 1
functions:
  loc: [/loc/pose]
modes:
  driving: { requires: [loc], fallback: [stop] }
  stop: { requires: [], reaction: sys.stop }
hazards:
  blind:
    monitors: [loc]
    enters: driving
nodes:
  n:
    cli:
      op:
        call: blocking
        timeout: 1s
        on_timeout: { reaction: stop }
        on_rejected:
          reaction: sys.stop
      op2:
        call: blocking
        timeout: 1s
        on_timeout: { retry: { attempts: 2, backoff: 1ms }, then: { reaction: stop } }
";
        let m = migrate_str(src).unwrap();
        let ids: Vec<_> = m.edits.iter().map(|e| (e.rewrite, e.path.as_str())).collect();
        assert_eq!(
            ids,
            vec![
                ("functions-capabilities", "functions"),
                ("hazard-enters-leaves", "hazards.blind.enters"),
                ("call-reaction-enters", "nodes.n.cli.op.on_timeout.reaction"),
                ("call-reaction-enters", "nodes.n.cli.op2.on_timeout.then.reaction"),
            ]
        );
        let want = src
            .replace("functions:\n", "capabilities:\n")
            .replace("    enters: driving", "    leaves: driving")
            .replace("on_timeout: { reaction: stop }", "on_timeout: { enters: stop }")
            .replace("then: { reaction: stop }", "then: { enters: stop }");
        assert_eq!(m.output, want);
        let parsed = crate::parse::parse_manifest_str(&m.output).unwrap();
        assert!(parsed.deprecated_spellings.is_empty());
        assert!(parsed.nodes["n"].cli["op"].on_timeout.as_ref().unwrap().enters);
        assert!(!parsed.nodes["n"].cli["op"].on_rejected_enters);
        assert!(!migrate_str(&m.output).unwrap().changed());
        // The old file reads, each old key noted for the consumer's warning.
        let old = crate::parse::parse_manifest_str(src).unwrap();
        let noted: Vec<_> = old
            .deprecated_spellings
            .iter()
            .map(|d| (d.at.as_str(), d.old, d.new))
            .collect();
        assert_eq!(
            noted,
            vec![
                ("hazards.blind.enters", "enters", "leaves"),
                ("functions", "functions", "capabilities"),
            ]
        );
    }

    #[test]
    fn non_ascii_before_a_key_keeps_byte_offsets_right() {
        let src = "version: 1\n# déjà vu — em dash\nmodes:\n  a:\n    min_dwell: 1s\n    exit:\n      on: f\n      to: b\n  b: {}\n";
        let m = migrate_str(src).unwrap();
        assert_eq!(m.output, src.replace("      on: f", "      holds: f"));
    }

    #[test]
    fn quoted_keys_keep_their_quotes() {
        let src = "version: 1\nmodes:\n  a:\n    min_dwell: 1s\n    exit: { \"on\": f, to: b }\n  b: {}\n";
        let m = migrate_str(src).unwrap();
        assert_eq!(m.output, src.replace("\"on\"", "\"holds\""));
    }

    #[test]
    fn a_file_with_nothing_to_rewrite_is_unchanged() {
        let src = "version: 1\nhazards:\n  h:\n    leaves: d\nmodes:\n  d: {}\n";
        let m = migrate_str(src).unwrap();
        assert!(!m.changed());
        assert_eq!(m.output, src);
    }

    fn ids(m: &Migration) -> Vec<&'static str> {
        m.edits.iter().map(|e| e.rewrite).collect()
    }

    #[test]
    fn g3_path_input_becomes_a_trigger_in_every_value_shape() {
        let src = "\
version: 1
nodes:
  n:
    paths:
      a:
        input: x        # a scalar
        output: [o]
      b:
        input: [x, y]
        output: [o]
      c: { input: x, output: [o] }
      d:
        input:
          - x
          - \"$(var ns)/y\"
        output: [o]
      e:
        input: [x]
        trigger: { input: [x] }
        output: [o]
paths:
  e2e: { output: [/o], input: [/i] }
includes:
  inner:
    paths:
      p:
        input: /i
        output: [/o]
";
        let m = migrate_str(src).unwrap();
        assert!(m.skipped.is_empty(), "{:?}", m.skipped);
        assert_eq!(ids(&m), vec!["path-input-trigger"; 7]);
        let want = src
            .replace(
                "input: x        # a scalar",
                "trigger: { input: [x] }        # a scalar",
            )
            .replace(
                "        input: [x, y]\n",
                "        trigger: { input: [x, y] }\n",
            )
            .replace(
                "c: { input: x, output",
                "c: { trigger: { input: [x] }, output",
            )
            .replace(
                "        input:\n          - x\n          - \"$(var ns)/y\"\n",
                "        trigger: { input: [x, $(var ns)/y] }\n",
            )
            .replace("        input: [x]\n        trigger", "        trigger")
            .replace(
                "{ output: [/o], input: [/i] }",
                "{ output: [/o], trigger: { input: [/i] } }",
            )
            .replace("        input: /i\n", "        trigger: { input: [/i] }\n");
        assert_eq!(m.output, want);
        assert!(!migrate_str(&m.output).unwrap().changed());
        // The source is refused since v0.3.15; the migrated file reads every
        // path as input-triggered.
        let b = crate::parse_manifest_str(&m.output).unwrap();
        for (name, p) in &b.nodes["n"].paths {
            assert!(
                matches!(p.effective_trigger(), crate::EffectiveTrigger::Input(_)),
                "{name}"
            );
        }
    }

    #[test]
    fn g3_external_topics_become_the_topics_section_when_there_is_none() {
        let src = "\
version: 1
nodes:
  c: { sub: [d] }
external_topics:
  /in:
    side: pub
    type: std_msgs/msg/String   # kept
  /out: { external: sub, type: std_msgs/msg/String }
";
        let m = migrate_str(src).unwrap();
        assert_eq!(
            m.output,
            src.replace("external_topics:", "topics:")
                .replace("side: pub", "external: pub")
        );
        assert!(!migrate_str(&m.output).unwrap().changed());
    }

    #[test]
    fn g3_external_topics_move_under_topics_and_merge_into_a_declared_topic() {
        let src = "\
version: 1
nodes:
  c:
    sub: [d, e]
topics:
  /in:
    type: std_msgs/msg/String
    sub: [c/d]
  /other: { type: std_msgs/msg/String, sub: [c/e] }

external_topics:
  /in:
    external: pub
    type: std_msgs/msg/String
  /other:
    side: pub
  /far:
    side: sub   # nobody here reads it
    type: std_msgs/msg/Empty
    qos: { depth: 1 }

services: {}
";
        let m = migrate_str(src).unwrap();
        assert!(m.skipped.is_empty(), "{:?}", m.skipped);
        let want = "\
version: 1
nodes:
  c:
    sub: [d, e]
topics:
  /in:
    external: pub
    type: std_msgs/msg/String
    sub: [c/d]
  /other: { external: pub, type: std_msgs/msg/String, sub: [c/e] }
  /far:
    external: sub   # nobody here reads it
    type: std_msgs/msg/Empty
    qos: { depth: 1 }

services: {}
";
        assert_eq!(m.output, want);
        let after = crate::parse_manifest_str(&m.output).unwrap();
        assert_eq!(
            after.topics["/far"].external,
            Some(crate::ExternalSide::Sub)
        );
        assert!(!migrate_str(&m.output).unwrap().changed());
    }

    #[test]
    fn g3_external_topics_that_disagree_with_topics_are_left_for_a_hand_edit() {
        let src = "\
version: 1
topics:
  /in:
    type: a/msg/A
external_topics:
  /in:
    external: pub
    type: b/msg/B
";
        let m = migrate_str(src).unwrap();
        assert!(!m.changed());
        assert_eq!(m.skipped.len(), 1, "{:?}", m.skipped);
    }

    #[test]
    fn g3_an_untyped_external_topic_is_left_for_a_hand_edit() {
        let m = migrate_str("version: 1\nexternal_topics:\n  /in: { external: pub }\n").unwrap();
        assert!(!m.changed());
        assert!(
            m.skipped[0].contains("`/in` states no `type:`"),
            "{:?}",
            m.skipped
        );
    }

    #[test]
    fn g7_inline_all_of_guards_become_functions() {
        let src = "\
version: 1
hazards:
  blind:
    guards:
      - /scan
      - all_of: [/ndt, /gnss]
    reaction: s.stop
  lost:
    guards: [/odom, { all_of: [/a, /b] }]
    reaction: s.stop
modes: {}
";
        // The removed spelling does not parse; the migrated text does.
        assert!(crate::parse_manifest_str(src).is_err());
        let m = migrate_str(src).unwrap();
        assert!(m.skipped.is_empty(), "{:?}", m.skipped);
        let want = "\
version: 1
hazards:
  blind:
    monitors:
      - /scan
      - blind_all_of
    reaction: s.stop
  lost:
    monitors: [/odom, lost_all_of]
    reaction: s.stop
capabilities:
  blind_all_of: { all_of: [/ndt, /gnss] }
  lost_all_of: { all_of: [/a, /b] }
modes: {}
";
        assert_eq!(m.output, want);
        let after = crate::parse_manifest_str(&m.output).unwrap();
        assert!(after.functions["blind_all_of"].group.all_of);
        assert!(!migrate_str(&m.output).unwrap().changed());
    }

    #[test]
    fn g7_a_function_goes_into_an_existing_functions_block() {
        let src = "\
version: 1
functions:
  pose: [/ndt]
hazards:
  h:
    guards: [{ all_of: [/a, /b] }]
    reaction: s.stop
";
        let m = migrate_str(src).unwrap();
        assert_eq!(
            m.output,
            "\
version: 1
capabilities:
  pose: [/ndt]
  h_all_of: { all_of: [/a, /b] }
hazards:
  h:
    monitors: [h_all_of]
    reaction: s.stop
"
        );
    }

    const TRANSPORT: &str = "\
version: 1
nodes:
  near:
    sub:
      objects: { max_transport: 0ms }   # its own wins
  far:
    sub:
      objects: { min_rate_hz: 10 }
  empty:
    sub:
      objects: {}
  block:
    sub:
      objects:
        min_rate_hz: 10
        # a comment inside stays
topics:
  /objects:
    type: T
    sub: [near/objects, far/objects, empty/objects, block/objects]
    max_transport: { duration: 10ms, source: requirement }
  /flow: { type: T, max_transport: 3ms }
";

    #[test]
    fn s1_a_topic_transport_becomes_one_link_and_an_inline_one_its_own() {
        let m = migrate_str(TRANSPORT).unwrap();
        assert!(ids(&m).iter().all(|i| *i == "max-transport-link"));
        let want = "\
version: 1
nodes:
  near:
    sub:
      objects: { link: near_objects }   # its own wins
  far:
    sub:
      objects: { min_rate_hz: 10, link: objects }
  empty:
    sub:
      objects: { link: objects }
  block:
    sub:
      objects:
        min_rate_hz: 10
        link: objects
        # a comment inside stays
topics:
  /objects:
    type: T
    sub: [near/objects, far/objects, empty/objects, block/objects]
  /flow: { type: T, max_transport: 3ms }

links:
  objects: { max_transport: { duration: 10ms, source: requirement } }
  near_objects: { max_transport: 0ms }
";
        assert_eq!(m.output, want);
        // `/flow` lists no subscriber here: the value may belong to another
        // contract's subscribers, so it is left, and said so.
        assert_eq!(m.skipped.len(), 1, "{:?}", m.skipped);
        assert!(m.skipped[0].contains("/flow") && m.skipped[0].contains("lists no subscriber"));
        // The transport each subscriber paid is its link's.
        let after = crate::parse_manifest_str(
            &m.output
                .replace("  /flow: { type: T, max_transport: 3ms }\n", ""),
        )
        .unwrap();
        for (n, paid) in [
            ("near", 0.0),
            ("far", 10.0),
            ("empty", 10.0),
            ("block", 10.0),
        ] {
            let sub = &after.nodes[n].subscribers["objects"];
            let link = sub.link.as_deref().expect("a link");
            assert_eq!(
                after.links[link].max_transport.ms(),
                Some(paid),
                "{n}: the transport it paid is its link's"
            );
        }
    }

    #[test]
    fn s1_inline_transports_join_an_existing_links_block_under_fresh_names() {
        let src = "\
version: 1
links:
  a_in: { max_transport: { duration: 1ms, source: requirement } }
nodes:
  a:
    sub:
      in:
        max_transport: 2ms
  b:
    sub:
      in: { max_transport: 4ms }
      other: { link: a_in }
";
        let m = migrate_str(src).unwrap();
        assert_eq!(
            m.output,
            "\
version: 1
links:
  a_in: { max_transport: { duration: 1ms, source: requirement } }
  a_in_2: { max_transport: 2ms }
  b_in: { max_transport: 4ms }
nodes:
  a:
    sub:
      in:
        link: a_in_2
  b:
    sub:
      in: { link: b_in }
      other: { link: a_in }
"
        );
        crate::parse_manifest_str(&m.output).unwrap();
    }

    const TOLERANCE: &str = "\
version: 1
nodes:
  n:
    paths:
      folded:
        sync: { policy: approximate }
        tolerance: 50ms
      same:
        sync: { policy: approximate, max_interval: 20ms }
        tolerance: 20ms
      differs:
        sync: { policy: approximate, max_interval: 20ms }
        tolerance: 30ms
      bare:
        tolerance: 50ms
";

    #[test]
    fn s1_tolerance_folds_into_an_existing_sync_only() {
        let m = migrate_str(TOLERANCE).unwrap();
        assert!(ids(&m).iter().all(|i| *i == "tolerance-sync"));
        assert_eq!(
            m.output,
            "\
version: 1
nodes:
  n:
    paths:
      folded:
        sync: { policy: approximate, max_interval: 50ms }
      same:
        sync: { policy: approximate, max_interval: 20ms }
      differs:
        sync: { policy: approximate, max_interval: 20ms }
        tolerance: 30ms
      bare:
        tolerance: 50ms
"
        );
        assert_eq!(m.skipped.len(), 2, "{:?}", m.skipped);
        assert!(
            m.skipped
                .iter()
                .any(|s| s.contains("differs") && s.contains("two numbers"))
        );
        // No `sync:`: adding one changes the derived rate, so it is the
        // author's call, and the message says what each choice means.
        assert!(
            m.skipped
                .iter()
                .any(|s| s.contains("bare") && s.contains("SUM") && s.contains("MIN"))
        );
    }

    #[test]
    fn s1_tolerance_is_refused_naming_max_interval() {
        let e = crate::parse_manifest_str(TOLERANCE)
            .unwrap_err()
            .to_string();
        assert!(e.contains("max_interval") && e.contains("v0.3.16"), "{e}");
    }

    #[test]
    fn g3b_a_subscriber_in_another_file_leaves_the_topic_alone() {
        let src = "version: 1\ntopics:\n  /t:\n    type: T\n    sub: [elsewhere/in]\n    max_transport: 5ms\n";
        let m = migrate_str(src).unwrap();
        assert!(!m.changed());
        assert!(m.skipped[0].contains("elsewhere/in"), "{:?}", m.skipped);
    }

    const NODE_DROP: &str = "\
version: 1
nodes:
  a:
    pub:
      out: {}
    paths:
      main:
        output: [out]
        drop:
          max_count: 5 / 100
          max_consecutive: 3
  b:
    pub:
      x: {}
      y: {}
    paths:
      main:
        output: [x, y]
        drop: 1 / 10
  c:
    pub:
      z: {}
    paths:
      main: { output: [z], drop: 2 / 100 }
topics:
  a_out:
    type: T
    pub: [a/out]
  c_z: { type: T, pub: [c/z] }
paths:
  e2e:
    output: [a_out]
    drop: 1 / 10
";

    #[test]
    fn g3b_a_node_path_drop_moves_to_its_one_output_topic() {
        let m = migrate_str(NODE_DROP).unwrap();
        assert!(ids(&m).iter().all(|i| *i == "node-path-drop-topic"));
        let want = "\
version: 1
nodes:
  a:
    pub:
      out: {}
    paths:
      main:
        output: [out]
  b:
    pub:
      x: {}
      y: {}
    paths:
      main:
        output: [x, y]
        drop: 1 / 10
  c:
    pub:
      z: {}
    paths:
      main: { output: [z] }
topics:
  a_out:
    type: T
    pub: [a/out]
    drop: { max_count: 5 / 100, max_consecutive: 3 }
  c_z: { type: T, pub: [c/z], drop: 2 / 100 }
paths:
  e2e:
    output: [a_out]
    drop: 1 / 10
";
        assert_eq!(m.output, want);
        assert_eq!(m.skipped.len(), 1, "{:?}", m.skipped);
        assert!(m.skipped[0].contains("2 outputs (x, y)"), "{:?}", m.skipped);
        // The input does not parse (a node-path drop is refused); the output
        // would, but for `b`, which is a hand edit.
        assert!(crate::parse_manifest_str(NODE_DROP).is_err());
        let fixed = m.output.replace("        drop: 1 / 10\n  c:", "  c:");
        let after = crate::parse_manifest_str(&fixed).unwrap();
        assert!(after.topics["a_out"].drop.is_some());
        assert!(after.topics["c_z"].drop.is_some());
    }

    #[test]
    fn every_rewrite_has_a_distinct_id() {
        let mut ids: Vec<_> = REWRITES.iter().map(|r| r.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), REWRITES.len());
    }
}

#[cfg(test)]
mod phase94_tests {
    use super::*;

    /// A bare parameter name on a bundled `param` key becomes the read form,
    /// block and qualified spellings included; a value already a reference,
    /// a quantity, or a key of an extension the file does not carry is left.
    #[test]
    fn ext_param_read_rewrites_bare_names_only() {
        let src = "\
version: 1
use: { vehicle.braking: \"^1\", my.ext: \"^1\" }
nodes:
  op:
    paths:
      p:
        output: [cmd]
        safe_state:
          emits: cmd
          settle:
            vehicle.braking/decel: target_acceleration   # qualified
            jerk: $(param target_jerk) m/s^3
            my.ext/decel: other
      q:
        output: [cmd]
        safe_state: { emits: cmd, settle: { decel: 2.5 m/s^2, jerk: target_jerk } }
";
        let m = migrate_str(src).unwrap();
        let paths: Vec<&str> = m.edits.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "nodes.op.paths.p.safe_state.settle.vehicle.braking/decel",
                "nodes.op.paths.q.safe_state.settle.jerk",
            ]
        );
        assert!(
            m.output.contains(
                "vehicle.braking/decel: $(param target_acceleration) m/s^2   # qualified"
            )
        );
        assert!(m.output.contains("jerk: $(param target_jerk) m/s^3 } }"));
        assert!(m.output.contains("my.ext/decel: other"));
        assert!(!migrate_str(&m.output).unwrap().changed());
    }
}

#[cfg(test)]
mod e8_tests {
    use super::*;

    fn run(src: &str) -> Migration {
        migrate_str(src).unwrap()
    }

    /// The vehicle keys without `use:`: the extension is added after
    /// `version:`, the entry speed gains its unit, and the severity's scale
    /// is named; a second run changes nothing.
    #[test]
    fn e8_names_what_the_checker_would_load_implicitly() {
        let src = "\
version: 1
# braking
hazards:
  blind:
    integrity: ASIL_B
    entry_speed: 8.33   # m/s
    monitors: [scan]
nodes:
  op:
    paths:
      p:
        output: [cmd]
        safe_state: { emits: cmd, settle: { decel: target_acceleration, jerk: target_jerk } }
";
        let m = run(src);
        assert_eq!(
            m.output,
            "\
version: 1
use:
  vehicle.braking: \"^1\"
  iso26262: \"^1\"
# braking
hazards:
  blind:
    integrity: ASIL_B
    entry_speed: 8.33 m/s   # m/s
    monitors: [scan]
nodes:
  op:
    paths:
      p:
        output: [cmd]
        safe_state: { emits: cmd, settle: { decel: $(param target_acceleration) m/s^2, jerk: $(param target_jerk) m/s^3 } }
"
        );
        let ids: Vec<&str> = m.edits.iter().map(|e| e.rewrite).collect();
        assert_eq!(
            ids,
            [
                "use-implicit",
                "entry-speed-unit",
                "ext-param-read",
                "ext-param-read"
            ]
        );
        assert!(!run(&m.output).changed());
    }

    /// An existing `use:` is extended in either form; a used profile, or
    /// `integrity_levels:`, means the scale is named already.
    #[test]
    fn e8_extends_a_use_in_both_forms_and_respects_a_named_scale() {
        let flow =
            "version: 1\nuse: { machine.iso13855: \"^1\" }\nhazards:\n  h:\n    integrity: SIL_2\n";
        assert_eq!(
            run(flow).output,
            "version: 1\nuse: { machine.iso13855: \"^1\", iso26262: \"^1\" }\nhazards:\n  h:\n    integrity: SIL_2\n"
        );
        // Another extension may declare `entry_speed` itself: reported,
        // not guessed.
        let rail = "version: 1\nuse: { rail.ebd: \"^0.1\", en50129: \"^1\" }\nhazards:\n  h:\n    integrity: SIL_4\n    entry_speed: 160 km/h\n";
        let m = run(rail);
        assert!(!m.changed());
        assert!(m.skipped[0].contains("rail.ebd"), "{:?}", m.skipped);
        let block = "version: 1\nuse:\n  iec61508: \"^1\"\nhazards:\n  h:\n    integrity: SIL_2\n    entry_speed: 3 m/s\n";
        assert_eq!(
            run(block).output,
            "version: 1\nuse:\n  iec61508: \"^1\"\n  vehicle.braking: \"^1\"\nhazards:\n  h:\n    integrity: SIL_2\n    entry_speed: 3 m/s\n"
        );
        let levels =
            "version: 1\nintegrity_levels: [low, high]\nhazards:\n  h:\n    integrity: high\n";
        assert!(!run(levels).changed());
        // An undotted name that is not a reference profile may be one.
        let local = "version: 1\nuse: { site_scale: \"^1\" }\nhazards:\n  h:\n    integrity: S2\n";
        let m = run(local);
        assert!(!m.changed());
        assert!(m.skipped[0].contains("may be a profile"), "{:?}", m.skipped);
    }

    /// A file with no root `version:` gets `use:` before its first key;
    /// one with a header gets it after the header's last key.
    #[test]
    fn e8_places_use_before_the_first_key_without_version() {
        let src = "# c\nhazards:\n  h:\n    integrity: ASIL_A\n";
        assert_eq!(
            run(src).output,
            "# c\nuse:\n  iso26262: \"^1\"\nhazards:\n  h:\n    integrity: ASIL_A\n"
        );
        let src = "version: 1\nrlm: v0.1.49\n\nhazards:\n  h:\n    integrity: ASIL_A\n";
        assert_eq!(
            run(src).output,
            "version: 1\nrlm: v0.1.49\nuse:\n  iso26262: \"^1\"\n\nhazards:\n  h:\n    integrity: ASIL_A\n"
        );
    }

    /// Phase 98 S4: a mode's `window:` is `min_dwell:`, block or flow, at
    /// the root and in an inline include.
    #[test]
    fn s4_renames_window_to_min_dwell() {
        let src = "version: 1\nmodes:\n  tor:\n    window: { duration: 10s, param: h.t }\n  slow: { window: 2s, requires: [] }\nincludes:\n  sub:\n    modes:\n      m:\n        window: 1s\n";
        let m = run(src);
        assert_eq!(
            m.output,
            "version: 1\nmodes:\n  tor:\n    min_dwell: { duration: 10s, param: h.t }\n  slow: { min_dwell: 2s, requires: [] }\nincludes:\n  sub:\n    modes:\n      m:\n        min_dwell: 1s\n"
        );
        assert!(
            m.edits.iter().all(|e| e.rewrite == "window-min-dwell"),
            "{:?}",
            m.edits
        );
        assert!(!run(&m.output).changed());
    }

    /// Phase 98 N6: `guards`, `ingress`, `on_demand` and
    /// `trigger: spontaneous` become `monitors`, `arrival_curve`,
    /// `aperiodic` and `trigger: aperiodic`, block and flow, in an inline
    /// include too; a mapping writing both spellings is left.
    #[test]
    fn n6_renames_reach_every_level_and_form() {
        let src = "version: 1\nhazards:\n  h: { guards: [a], ftti: 1s }\nnodes:\n  n:\n    pub:\n      cmd: { on_demand: true }\n    sub:\n      in:\n        ingress: { rate_hz: 10, burst: 2 }\n    paths:\n      p: { trigger: spontaneous, output: [cmd] }\n      q:\n        trigger: \"spontaneous\"\nincludes:\n  sub:\n    hazards:\n      g:\n        guards: [b]\n        monitors: [c]\n";
        let m = run(src);
        assert_eq!(
            m.output,
            "version: 1\nhazards:\n  h: { monitors: [a], ftti: 1s }\nnodes:\n  n:\n    pub:\n      cmd: { aperiodic: true }\n    sub:\n      in:\n        arrival_curve: { rate_hz: 10, burst: 2 }\n    paths:\n      p: { trigger: aperiodic, output: [cmd] }\n      q:\n        trigger: \"aperiodic\"\nincludes:\n  sub:\n    hazards:\n      g:\n        guards: [b]\n        monitors: [c]\n"
        );
        assert_eq!(m.edits.len(), 5, "{:?}", m.edits);
        assert_eq!(m.skipped.len(), 1, "{:?}", m.skipped);
        assert!(
            m.skipped[0].contains("includes.sub.hazards.g.guards"),
            "{:?}",
            m.skipped
        );
    }

    /// Phase 98: an acknowledgement of a renamed rule follows the rename,
    /// in block and flow form, quoted or not; other ids are left.
    #[test]
    fn an_acknowledged_old_rule_id_is_rewritten() {
        let src = "version: 1\nacknowledged:\n  - rule: severity-scale-conflict\n    at: integrity_levels\n    reason: kept\n  - { rule: \"severity-unknown\", at: x, reason: y }\n  - { rule: reaction-unguarded, at: modes.m.window, reason: w }\n";
        let m = run(src);
        assert_eq!(
            m.output,
            "version: 1\nacknowledged:\n  - rule: integrity-scale-conflict\n    at: integrity_levels\n    reason: kept\n  - { rule: \"integrity-unknown\", at: x, reason: y }\n  - { rule: reaction-unguarded, at: modes.m.min_dwell, reason: w }\n"
        );
        assert_eq!(m.edits.len(), 3, "{:?} {:?}", m.edits, m.skipped);
        assert!(!run(&m.output).changed());
    }

    /// Phase 98 N1: the integrity level's old key, at the root and in an
    /// inline include, beside `use-implicit` which reads either spelling.
    #[test]
    fn n1_renames_severity_to_integrity() {
        let src = "version: 1\nuse: { iso26262: \"^1\" }\nseverity_levels: [QM, ASIL_A, ASIL_B, ASIL_C, ASIL_D]\nhazards:\n  h:\n    severity: ASIL_B   # kept\nincludes:\n  sub:\n    use: { iso26262: \"^1\" }\n    hazards:\n      g: { severity: QM }\n";
        let m = run(src);
        assert_eq!(
            m.output,
            "version: 1\nuse: { iso26262: \"^1\" }\nintegrity_levels: [QM, ASIL_A, ASIL_B, ASIL_C, ASIL_D]\nhazards:\n  h:\n    integrity: ASIL_B   # kept\nincludes:\n  sub:\n    use: { iso26262: \"^1\" }\n    hazards:\n      g: { integrity: QM }\n"
        );
        assert!(
            m.edits.iter().all(|e| e.rewrite == "severity-integrity"),
            "{:?}",
            m.edits
        );
        assert!(!run(&m.output).changed());
        // Both spellings: the parser refuses it, migrate leaves it.
        let both = "version: 1\nhazards:\n  h:\n    severity: QM\n    integrity: QM\n";
        let m = run(both);
        assert!(m.edits.iter().all(|e| e.rewrite != "severity-integrity"));
        assert!(
            m.skipped.iter().any(|s| s.contains("both spellings")),
            "{:?}",
            m.skipped
        );
    }
}
