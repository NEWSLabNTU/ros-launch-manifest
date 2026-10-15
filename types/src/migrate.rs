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
        id: "guard-all-of-function",
        unit: "G7",
        since: "0.1.59",
        from: "hazards.<h>.guards: [{ all_of: [...] }]",
        to: "functions.<h>_all_of: { all_of: [...] }, guards: [<h>_all_of]",
        when: "always (the inline group is removed; the same group named in `functions:`)",
        find: guard_all_of_function,
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
        let mut taken: HashSet<String> = y["functions"]
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
            let Some(guards) = h["guards"].as_vec() else {
                continue;
            };
            for (i, g) in guards.iter().enumerate() {
                if g.as_hash().is_none() || g["all_of"].is_badvalue() {
                    continue;
                }
                let gpath = with(&level, &["hazards", &hname, "guards", &i.to_string()]);
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
        let fpath = with(&level, &["functions"]);
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
                format!("{}functions:\n{}", " ".repeat(col), lines(ci)),
            ))
        } else {
            None
        };
        let Some((at, mut text)) = insert else {
            doc.skip(
                ID,
                &fpath.join("."),
                "no block `functions:` or `hazards:` to place the function in",
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
        // Every path reads the same trigger before and after.
        let (a, b) = (
            crate::parse_manifest_str(src).unwrap(),
            crate::parse_manifest_str(&m.output).unwrap(),
        );
        for (name, p) in &a.nodes["n"].paths {
            assert_eq!(
                format!("{:?}", p.effective_trigger()),
                format!("{:?}", b.nodes["n"].paths[name].effective_trigger()),
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
        assert!(after.external_topics.is_empty());
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
    guards:
      - /scan
      - blind_all_of
    reaction: s.stop
  lost:
    guards: [/odom, lost_all_of]
    reaction: s.stop
functions:
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
functions:
  pose: [/ndt]
  h_all_of: { all_of: [/a, /b] }
hazards:
  h:
    guards: [h_all_of]
    reaction: s.stop
"
        );
    }

    #[test]
    fn every_rewrite_has_a_distinct_id() {
        let mut ids: Vec<_> = REWRITES.iter().map(|r| r.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), REWRITES.len());
    }
}
