//! YAML parser for manifest files using yaml-rust2.
//!
//! Parses YAML into typed [`Manifest`] AST. Handles both plain list
//! and map forms for endpoints (e.g., `pub: [a, b]` vs `pub: {a: {min_rate_hz: 10}}`).

use crate::{
    bound::{Bound, ParamBinding, Quantity, spelled},
    duration::Duration,
    field_table::{self, Context},
    span::SpanIndex,
    types::*,
};
use std::{collections::BTreeMap, path::Path};
use yaml_rust2::{Yaml, YamlLoader};

/// Parse error with optional source location.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("YAML syntax error: {0}")]
    YamlSyntax(String),
    #[error("at '{path}': {message}")]
    Field { path: String, message: String },
}

/// Result of parsing a manifest, including source text and span index
/// for diagnostic reporting.
#[derive(Debug, Clone)]
pub struct ParseResult {
    pub manifest: Manifest,
    /// Original source text (for codespan-reporting).
    pub source: String,
    /// Span index mapping YAML key paths to byte ranges.
    pub spans: SpanIndex,
}

/// Parse a manifest file from disk.
pub fn parse_manifest(path: &Path) -> Result<Manifest, ParseError> {
    let content = std::fs::read_to_string(path)?;
    parse_manifest_str(&content)
}

/// Parse a manifest file from disk, returning source and spans.
pub fn parse_manifest_with_spans(path: &Path) -> Result<ParseResult, ParseError> {
    let content = std::fs::read_to_string(path)?;
    parse_manifest_str_with_spans(&content)
}

/// Parse a manifest from a YAML string.
pub fn parse_manifest_str(source: &str) -> Result<Manifest, ParseError> {
    let docs =
        YamlLoader::load_from_str(source).map_err(|e| ParseError::YamlSyntax(e.to_string()))?;
    if docs.is_empty() {
        return Ok(Manifest {
            version: 1,
            ..Default::default()
        });
    }
    // The grammar the file needs, BEFORE any key of
    // the body is read -- a file written for a newer grammar would otherwise
    // be refused for its first new key, which reads as the author's typo.
    check_grammar_header(&docs[0])?;
    with_unknown_keys_collected(|| parse_manifest_yaml(&docs[0], ""))
}

/// A release spelled `0.1.49`, `v0.1.49`, `>=0.1.49` or `>= v0.1.49`, as
/// `(major, minor, patch)`.
pub fn parse_grammar_version(raw: &str) -> Option<(u64, u64, u64)> {
    let v = raw.trim();
    let v = v.strip_prefix(">=").unwrap_or(v).trim_start();
    let v = v.strip_prefix('v').unwrap_or(v);
    let mut it = v.split('.').map(|p| p.parse::<u64>().ok());
    let (Some(Some(a)), Some(Some(b)), Some(Some(c)), None) =
        (it.next(), it.next(), it.next(), it.next())
    else {
        return None;
    };
    Some((a, b, c))
}

/// Refuse a file whose `rlm:` header names a grammar newer than this crate's
/// ([`crate::GRAMMAR_VERSION`]), naming both releases. A checker older than the
/// header key itself (before v0.1.49) refuses `rlm:` as an unknown key instead,
/// the fallback the header cannot avoid.
fn check_grammar_header(doc: &Yaml) -> Result<(), ParseError> {
    let v = &doc["rlm"];
    if v.is_badvalue() {
        return Ok(());
    }
    let ours = parse_grammar_version(crate::GRAMMAR_VERSION).expect("GRAMMAR_VERSION parses");
    let raw = match v {
        Yaml::String(s) => s.clone(),
        other => {
            return Err(field_err(
                "",
                "rlm",
                &format!(
                    "expected the grammar release this file needs as a string, like `rlm: \
                     v{}`; found {}",
                    crate::GRAMMAR_VERSION,
                    yaml_kind(other)
                ),
            ));
        }
    };
    let Some(needed) = parse_grammar_version(&raw) else {
        return Err(field_err(
            "",
            "rlm",
            &format!(
                "`{raw}` is not a grammar release: write `v<major>.<minor>.<patch>` (or with \
                 `>=`), like `rlm: v{}`",
                crate::GRAMMAR_VERSION
            ),
        ));
    };
    if needed > ours {
        return Err(field_err(
            "",
            "rlm",
            &format!(
                "this contract needs rlm >= v{}.{}.{}; this checker reads rlm v{}. Upgrade the \
                 checker -- its grammar is older than the file's, so a key of the file may be \
                 one it does not know",
                needed.0,
                needed.1,
                needed.2,
                crate::GRAMMAR_VERSION
            ),
        ));
    }
    Ok(())
}

/// Parse a manifest from a YAML string, returning source and spans.
pub fn parse_manifest_str_with_spans(source: &str) -> Result<ParseResult, ParseError> {
    let manifest = parse_manifest_str(source)?;
    let spans = SpanIndex::build(source);
    Ok(ParseResult {
        manifest,
        source: source.to_string(),
        spans,
    })
}

fn parse_manifest_yaml(doc: &Yaml, ctx: &str) -> Result<Manifest, ParseError> {
    reject_chains(doc, ctx)?;
    reject_unknown_keys(doc, ctx, Context::Manifest)?;
    // `if:`/`unless:` at a manifest root are the INLINE include's condition
    // (issue #0043), and an inline include is the only manifest that has one:
    // a standalone file is not pulled in by anything, so a condition there
    // selects nothing. Phase 69 established that a key which cannot act is an
    // error, not something to discard quietly.
    if ctx.is_empty() {
        for key in ["if", "unless"] {
            if !doc[key].is_badvalue() {
                return Err(field_err(
                    ctx,
                    key,
                    "a condition at a manifest root belongs to the `includes:` entry that \
                     pulls the file in — write it there (`includes.<name>.{manifest: …, \
                     if: …}`). At the root of a standalone manifest it would select nothing",
                ));
            }
        }
    }
    let manifest = Manifest {
        version: yaml_u32(doc, "version", ctx)?.unwrap_or(1),
        rlm: if ctx.is_empty() {
            doc["rlm"].as_str().map(str::to_string)
        } else {
            None
        },
        args: parse_args(doc, ctx)?,
        nodes: parse_nodes(doc, ctx)?,
        topics: parse_topics(doc, ctx)?,
        services: parse_services(doc, ctx)?,
        actions: parse_actions(doc, ctx)?,
        includes: parse_includes(doc, ctx)?,
        paths: parse_paths(doc, ctx, false)?,
        external_topics: parse_external_topics(doc, ctx)?,
        hazards: parse_hazards(doc, ctx)?,
        severity_levels: yaml_string_list(doc, "severity_levels", ctx)?,
        functions: parse_functions(doc, ctx)?,
        modes: parse_modes(doc, ctx)?,
    };
    Ok(manifest)
}

/// A duration bound (phase 90 G2): the key `key` of `context`, read into
/// the [`Bound`] its row of [`crate::BOUND_KEYS`] gives. A bare duration
/// (`max_latency: 149ms`), or -- for a key whose form allows it -- the map
/// form `{ duration, source }` (phase 10 B7), which also takes `param` on a
/// key that can be bound to a parameter (`max_age`).
fn yaml_bound(
    doc: &Yaml,
    key: &str,
    ctx: &str,
    context: Context,
) -> Result<Option<Bound>, ParseError> {
    let form = crate::bound::bound_key(context, key)
        .unwrap_or_else(|| panic!("`{key}` in {} is not a bound key", context.label()))
        .form;
    let v = &doc[key];
    let Yaml::Hash(_) = v else {
        return Ok(yaml_duration(doc, key)?.map(|d| spelled(context, key, Quantity::Duration(d))));
    };
    if form == crate::bound::Form::Bare {
        // What `yaml_duration` says about a map where a duration belongs.
        return Ok(yaml_duration(doc, key)?.map(|d| spelled(context, key, Quantity::Duration(d))));
    }
    let bctx = &format_path(ctx, key);
    reject_unknown_keys(v, bctx, Context::Budget)?;
    let d = yaml_duration(v, "duration")
        .map_err(|e| reframe(e, bctx))?
        .ok_or_else(|| {
            field_err(
                bctx,
                "duration",
                "required: the budget itself, with a unit (`149ms`)",
            )
        })?;
    let param = yaml_string(v, "param", bctx)?;
    if param.is_some() && form != crate::bound::Form::MapParam {
        return Err(field_err(
            bctx,
            "param",
            &format!(
                "`{key}` cannot be bound to a parameter; `param:` binds `max_age` (a subscriber's \
                 staleness bound), a timer (`trigger: {{ timer: {{ param }} }}`) and a window"
            ),
        ));
    }
    let mut b = spelled(context, key, Quantity::Duration(d));
    b.param = param.map(ParamBinding::Own);
    if !v["source"].is_badvalue() {
        b.source = Some(parse_source(&v["source"], bctx)?);
    }
    Ok(Some(b))
}

/// A rate bound (phase 90 G2): a number of Hz under `key` of `context`.
fn yaml_rate_bound(
    doc: &Yaml,
    key: &str,
    ctx: &str,
    context: Context,
) -> Result<Option<Bound>, ParseError> {
    Ok(yaml_f64(doc, key, ctx)?.map(|v| spelled(context, key, Quantity::Hz(v))))
}

/// `source:` (phase 10 B7): `requirement | measured | derived | analysed`,
/// or `{ measured: { max, n, period, margin } }`, or `{ measured: { margin,
/// terms: { <name>: { max, n, period } } } }`.
fn parse_source(v: &Yaml, ctx: &str) -> Result<BudgetSource, ParseError> {
    let kinds = || {
        SourceKind::ALL
            .iter()
            .map(|k| k.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let kind_of = |name: &str| {
        SourceKind::ALL
            .into_iter()
            .find(|k| k.as_str() == name)
            .ok_or_else(|| {
                field_err(
                    ctx,
                    "source",
                    &format!("`{name}` is not a source -- accepted: {}", kinds()),
                )
            })
    };
    match v {
        Yaml::String(s) => Ok(BudgetSource {
            kind: kind_of(s)?,
            measured: None,
        }),
        Yaml::Hash(h) if h.len() == 1 => {
            let (k, body) = h.iter().next().expect("len == 1");
            let kind = kind_of(&yaml_str_owned(k))?;
            if kind != SourceKind::Measured {
                return Err(field_err(
                    ctx,
                    "source",
                    &format!("only `measured` carries observations; write `source: {kind}`"),
                ));
            }
            let mctx = &format_path(ctx, "source.measured");
            reject_unknown_keys(body, mctx, Context::Measured)?;
            let margin = yaml_f64(body, "margin", mctx)?.ok_or_else(|| {
                field_err(
                    mctx,
                    "margin",
                    "required: the margin over the observed maximum, a fraction (`0.2`)",
                )
            })?;
            if !(margin.is_finite() && margin >= 0.0) {
                return Err(field_err(mctx, "margin", "must be a fraction >= 0"));
            }
            let term =
                |t: &Yaml, tctx: &str, name: Option<String>| -> Result<MeasuredTerm, ParseError> {
                    let max = yaml_duration(t, "max")
                        .map_err(|e| reframe(e, tctx))?
                        .ok_or_else(|| {
                            field_err(tctx, "max", "required: the largest value observed")
                        })?;
                    let n = match &t["n"] {
                        Yaml::BadValue | Yaml::Null => None,
                        Yaml::Integer(i) if *i > 0 => Some(*i as u64),
                        other => return Err(type_err(tctx, "n", "a positive sample count", other)),
                    };
                    Ok(MeasuredTerm {
                        name,
                        max,
                        n,
                        period: yaml_duration(t, "period").map_err(|e| reframe(e, tctx))?,
                    })
                };
            let terms = match (&body["terms"], body["max"].is_badvalue()) {
                (Yaml::BadValue, true) => {
                    return Err(field_err(
                        mctx,
                        "max",
                        "required: the observed maximum (`max`), or several terms (`terms`)",
                    ));
                }
                (Yaml::BadValue, false) => vec![term(body, mctx, None)?],
                (Yaml::Hash(th), true) => {
                    let tctx = &format_path(mctx, "terms");
                    let mut out = Vec::new();
                    for (tk, tv) in th {
                        let name = yaml_str_owned(tk);
                        let one = &format_path(tctx, &name);
                        reject_unknown_keys(tv, one, Context::MeasuredTerm)?;
                        out.push(term(tv, one, Some(name))?);
                    }
                    if out.is_empty() {
                        return Err(field_err(mctx, "terms", "names no term"));
                    }
                    out
                }
                (Yaml::Hash(_), false) => {
                    return Err(field_err(
                        mctx,
                        "terms",
                        "state one maximum (`max`) or several terms (`terms`), not both",
                    ));
                }
                (other, _) => {
                    return Err(type_err(
                        mctx,
                        "terms",
                        "a map of name -> { max, n, period }",
                        other,
                    ));
                }
            };
            Ok(BudgetSource {
                kind,
                measured: Some(Measurement { margin, terms }),
            })
        }
        other => Err(type_err(
            ctx,
            "source",
            "a source name or `{ measured: { max, n, margin } }`",
            other,
        )),
    }
}

/// A guard group in the two shapes phase 71 established: a bare list is
/// any-of (losing any member is the fault), `{ all_of: [...] }` is a
/// redundant set (lost only when every member is). The redundant set is a
/// function's shape only: inline in `hazards.<h>.guards` it was removed in
/// v0.1.59 (phase 90 G7), and [`parse_hazard_guard`] refuses it.
fn parse_guard_group(v: &Yaml, ctx: &str, key: &str) -> Result<GuardGroup, ParseError> {
    match v {
        Yaml::String(t) => Ok(GuardGroup {
            members: vec![t.clone()],
            all_of: false,
        }),
        Yaml::Array(_) => Ok(GuardGroup {
            members: yaml_direct_string_list(v, key, ctx)?,
            all_of: false,
        }),
        Yaml::Hash(_) => {
            reject_unknown_keys(v, ctx, Context::Function)?;
            let members = yaml_string_list(v, "all_of", ctx)?;
            if members.len() < 2 {
                return Err(field_err(
                    ctx,
                    "all_of",
                    "a redundant group needs at least two members — a single topic is \
                     written bare",
                ));
            }
            Ok(GuardGroup {
                members,
                all_of: true,
            })
        }
        other => Err(type_err(
            ctx,
            key,
            "a topic name, a list of them, or `{ all_of: [...] }`",
            other,
        )),
    }
}

/// One `hazards.<h>.guards[i]`: a topic, a list of them, or a function's
/// name. A mapping is refused: its only key was the inline `all_of`, removed
/// in v0.1.59 (phase 90 G7) for the same group named in `functions:`, and the
/// removed row's message says so.
fn parse_hazard_guard(v: &Yaml, ctx: &str, key: &str) -> Result<GuardGroup, ParseError> {
    if let Yaml::Hash(_) = v {
        reject_unknown_keys(v, ctx, Context::HazardGuard)?;
        return Err(type_err(
            ctx,
            key,
            "a topic name, a list of them, or the name of a function",
            v,
        ));
    }
    parse_guard_group(v, ctx, key)
}

fn parse_functions(doc: &Yaml, ctx: &str) -> Result<BTreeMap<String, FunctionDecl>, ParseError> {
    let mut out = BTreeMap::new();
    let section = &doc["functions"];
    if section.is_badvalue() {
        return Ok(out);
    }
    let hash = section
        .as_hash()
        .ok_or_else(|| type_err(ctx, "functions", "a mapping", section))?;
    for (k, v) in hash {
        let name = yaml_str_owned(k);
        let fctx = format_path(ctx, &format!("functions.{name}"));
        out.insert(name, parse_function(v, &fctx)?);
    }
    Ok(out)
}

/// One `functions.<name>`: the three guard-group shapes phase 75 accepts, or
/// (v0.1.46) a VALUE function, `{ of: [...], when: {...} }`.
///
/// The map form is closed three ways. `all_of` is a redundant group and
/// takes no predicate: "lost by value when every member is" is not a thing
/// any detector evaluates. `of` without `when` is the bare list spelled
/// twice, and `when` without `of` reads no topic -- both refused, so each
/// meaning has one spelling.
fn parse_function(v: &Yaml, ctx: &str) -> Result<FunctionDecl, ParseError> {
    let Yaml::Hash(_) = v else {
        return Ok(FunctionDecl {
            group: parse_guard_group(v, ctx, "functions.<name>")?,
            when: None,
        });
    };
    reject_unknown_keys(v, ctx, Context::Function)?;
    let has = |key: &str| !v[key].is_badvalue();
    match (has("all_of"), has("of"), has("when")) {
        (true, false, false) => Ok(FunctionDecl {
            group: parse_guard_group(v, ctx, "functions.<name>")?,
            when: None,
        }),
        (true, _, _) => Err(field_err(
            ctx,
            "all_of",
            "a redundant group takes no `of:` or `when:` -- a value function is `{ of: [...], \
             when: {...} }`, lost by value on ANY member, and a redundant group is lost only by \
             silence",
        )),
        (false, true, true) => {
            let members = parse_string_or_list(v, "of", ctx)?;
            if members.is_empty() {
                return Err(field_err(
                    ctx,
                    "of",
                    "a value function reads at least one topic",
                ));
            }
            Ok(FunctionDecl {
                group: GuardGroup {
                    members,
                    all_of: false,
                },
                when: parse_predicate(v, ctx)?,
            })
        }
        (false, true, false) => Err(field_err(
            ctx,
            "of",
            "`of:` without `when:` is the plain list written twice -- write the members bare \
             (`functions.<name>: [...]`), or add the `when:` that loses the function by value",
        )),
        (false, false, true) => Err(field_err(
            ctx,
            "when",
            "a predicate needs the topics it reads -- write `{ of: [<topic>], when: {...} }`",
        )),
        (false, false, false) => Err(field_err(
            ctx,
            "all_of",
            "an empty mapping -- a function is a topic, a list, `{ all_of: [...] }` or \
             `{ of: [...], when: {...} }`",
        )),
    }
}

/// `when: { field: <name>, <op>: <constant> }` (v0.1.46), on a hazard or a
/// value function. Exactly one operator key; an ordering operator needs a
/// number. `None` when the key is absent.
fn parse_predicate(doc: &Yaml, ctx: &str) -> Result<Option<ValuePredicate>, ParseError> {
    let section = &doc["when"];
    if section.is_badvalue() {
        return Ok(None);
    }
    let pctx = &format_path(ctx, "when");
    let Yaml::Hash(_) = section else {
        return Err(type_err(
            ctx,
            "when",
            "a mapping `{ field: <name>, <op>: <constant> }`",
            section,
        ));
    };
    reject_unknown_keys(section, pctx, Context::Predicate)?;
    let field = yaml_string(section, "field", pctx)?.ok_or_else(|| {
        field_err(
            pctx,
            "field",
            "required: the message field the predicate reads, a dotted path from the root",
        )
    })?;
    if field.is_empty() || field.split('.').any(str::is_empty) {
        return Err(field_err(
            pctx,
            "field",
            &format!("`{field}` is not a field path -- dot-separated names, none empty"),
        ));
    }
    let ops: Vec<CompareOp> = CompareOp::ALL
        .into_iter()
        .filter(|op| !section[op.key()].is_badvalue())
        .collect();
    let accepted = CompareOp::ALL.map(CompareOp::key).join(", ");
    let op = match ops.as_slice() {
        [one] => *one,
        [] => {
            return Err(field_err(
                pctx,
                "field",
                &format!("no operator -- write exactly one of {accepted} with the constant"),
            ));
        }
        many => {
            let named: Vec<&str> = many.iter().map(|o| o.key()).collect();
            return Err(field_err(
                pctx,
                named[1],
                &format!(
                    "{} operators ({}) -- a predicate has exactly one. There are no \
                     conjunctions: a second condition is a second function",
                    many.len(),
                    named.join(", ")
                ),
            ));
        }
    };
    let raw = &section[op.key()];
    let value = match raw {
        Yaml::Boolean(b) => PredicateValue::Bool(*b),
        Yaml::Integer(i) => PredicateValue::Int(*i),
        Yaml::Real(r) => PredicateValue::Float(
            r.parse()
                .map_err(|_| type_err(pctx, op.key(), "a number", raw))?,
        ),
        Yaml::String(t) => PredicateValue::from_text(t),
        other => {
            return Err(type_err(
                pctx,
                op.key(),
                "a scalar constant: a bool, a number, a message constant name or a string",
                other,
            ));
        }
    };
    if op.is_ordering() && value.as_f64().is_none() {
        return Err(field_err(
            pctx,
            op.key(),
            &format!(
                "`{}: {value}` -- an ordering operator compares numbers; use `equals` or \
                 `not_equals` for anything else",
                op.key()
            ),
        ));
    }
    Ok(Some(ValuePredicate { field, op, value }))
}

/// `modes.<m>.window` (v0.1.46): a bare duration (unbound), or the map
/// form of a budget, `{ duration, param: <node>.<parameter>, source }`
/// (phase 90 G2: a least-time bound, no context of its own).
fn parse_window(doc: &Yaml, ctx: &str) -> Result<Option<Bound>, ParseError> {
    let section = &doc["window"];
    let positive = |d: crate::duration::Duration, at: &str| {
        if d.as_millis_f64() > 0.0 {
            Ok(d)
        } else {
            Err(field_err(
                ctx,
                at,
                "a window of zero is no window -- the rung would be left the moment it was \
                 entered",
            ))
        }
    };
    let window = |d| spelled(Context::Mode, "window", Quantity::Duration(d));
    match section {
        Yaml::BadValue => Ok(None),
        Yaml::String(_) | Yaml::Integer(_) | Yaml::Real(_) => {
            let d = yaml_duration(doc, "window")
                .map_err(|e| reframe(e, ctx))?
                .expect("a scalar window parses or errors");
            Ok(Some(window(positive(d, "window")?)))
        }
        Yaml::Hash(_) => {
            let wctx = &format_path(ctx, "window");
            reject_unknown_keys(section, wctx, Context::Budget)?;
            let d = yaml_duration(section, "duration")
                .map_err(|e| reframe(e, wctx))?
                .ok_or_else(|| {
                    field_err(
                        wctx,
                        "duration",
                        "required: how long the rung may last, with a unit (`10s`)",
                    )
                })?;
            let param = yaml_string(section, "param", wctx)?
                .map(|raw| parse_param_ref(&raw, wctx, "param"))
                .transpose()?;
            let source = if section["source"].is_badvalue() {
                None
            } else {
                Some(parse_source(&section["source"], wctx)?)
            };
            let mut b = window(positive(d, "window.duration")?);
            b.param = param.map(ParamBinding::Node);
            b.source = source;
            Ok(Some(b))
        }
        other => Err(type_err(
            ctx,
            "window",
            "a duration or `{ duration, param }`",
            other,
        )),
    }
}

/// `<node>.<parameter>`, split at the first dot.
fn parse_param_ref(raw: &str, ctx: &str, key: &str) -> Result<ParamRef, ParseError> {
    match raw.split_once('.') {
        Some((node, name)) if !node.is_empty() && !name.is_empty() => Ok(ParamRef {
            node: node.to_string(),
            name: name.to_string(),
        }),
        _ => Err(field_err(
            ctx,
            key,
            &format!(
                "`{raw}` does not name a parameter -- write `<node>.<parameter>`, the node as \
                 `nodes:` spells it (`mrm_handler.takeover_request_timeout`)"
            ),
        )),
    }
}

/// `modes.<m>.exit` (v0.1.46): `{ holds: <function>, to: <mode> }`, both
/// required. `on:` is the pre-0.1.59 spelling of `holds:` (deprecated: it
/// parses, and [`ModeExit::spelled_on`] records it for the consumer's
/// warning); writing both is an error.
fn parse_exit(doc: &Yaml, ctx: &str) -> Result<Option<ModeExit>, ParseError> {
    let section = &doc["exit"];
    if section.is_badvalue() {
        return Ok(None);
    }
    let ectx = &format_path(ctx, "exit");
    let Yaml::Hash(_) = section else {
        return Err(type_err(
            ctx,
            "exit",
            "a mapping `{ holds: <function>, to: <mode> }`",
            section,
        ));
    };
    reject_unknown_keys(section, ectx, Context::Exit)?;
    let holds = yaml_string(section, "holds", ectx)?;
    let on = yaml_string(section, "on", ectx)?;
    if holds.is_some() && on.is_some() {
        return Err(field_err(
            ectx,
            "on",
            "`on:` is the old spelling of `holds:` -- write `holds:` alone",
        ));
    }
    let spelled_on = on.is_some();
    let holds = holds.or(on).ok_or_else(|| {
        field_err(
            ectx,
            "holds",
            "required: the function whose holding ends the ladder",
        )
    })?;
    let to = yaml_string(section, "to", ectx)?.ok_or_else(|| {
        field_err(
            ectx,
            "to",
            "required: the mode the ladder ends in when the exit is taken",
        )
    })?;
    Ok(Some(ModeExit {
        holds,
        spelled_on,
        to,
    }))
}

/// `yaml_duration` reports with an empty context; put the real one back.
fn reframe(e: ParseError, ctx: &str) -> ParseError {
    match e {
        ParseError::Field { path, message } => ParseError::Field {
            path: format_path(ctx, &path),
            message,
        },
        other => other,
    }
}

fn parse_modes(doc: &Yaml, ctx: &str) -> Result<BTreeMap<String, ModeDecl>, ParseError> {
    let mut out = BTreeMap::new();
    let section = &doc["modes"];
    if section.is_badvalue() {
        return Ok(out);
    }
    let hash = section
        .as_hash()
        .ok_or_else(|| type_err(ctx, "modes", "a mapping", section))?;
    for (k, v) in hash {
        let name = yaml_str_owned(k);
        let mctx = format_path(ctx, &format!("modes.{name}"));
        reject_unknown_keys(v, &mctx, Context::Mode)?;
        let overrides = parse_mode_overrides(v, &mctx)?;
        let window = parse_window(v, &mctx)?;
        let exit = parse_exit(v, &mctx)?;
        // An exit is the one way out of a TIMED rung. On a rung with no
        // window it would be a general mode transition, which the grammar
        // deliberately does not have (`operational-modes.md`: transitions
        // are not folded in).
        if exit.is_some() && window.is_none() {
            return Err(field_err(
                &mctx,
                "exit",
                "an exit is only for a windowed rung -- add the `window:` this rung waits \
                 out, or drop the exit. A general mode transition is not in the grammar",
            ));
        }
        out.insert(
            name,
            ModeDecl {
                description: yaml_string(v, "description", &mctx)?,
                requires: parse_string_or_list(v, "requires", &mctx)?,
                fallback: parse_string_or_list(v, "fallback", &mctx)?,
                reaction: yaml_string(v, "reaction", &mctx)?,
                overrides,
                window,
                exit,
            },
        );
    }
    Ok(out)
}

/// `overrides:` is a nested mapping mirroring the contract's own shape, and
/// it is flattened to `(dotted target, value)` pairs here — the checker
/// resolves the target when it runs the mode. Flattening at parse time
/// keeps the type one shape rather than a partial mirror of every
/// declaration struct.
fn parse_mode_overrides(doc: &Yaml, ctx: &str) -> Result<Vec<ModeOverride>, ParseError> {
    let section = &doc["overrides"];
    if section.is_badvalue() {
        return Ok(Vec::new());
    }
    let ctx = &format_path(ctx, "overrides");
    let mut out = Vec::new();
    fn walk(
        v: &Yaml,
        prefix: &str,
        ctx: &str,
        out: &mut Vec<ModeOverride>,
    ) -> Result<(), ParseError> {
        let hash = v
            .as_hash()
            .ok_or_else(|| type_err(ctx, prefix, "a mapping", v))?;
        for (k, val) in hash {
            let key = yaml_str_owned(k);
            let target = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            match val {
                Yaml::Hash(_) => walk(val, &target, ctx, out)?,
                Yaml::String(s) => out.push(ModeOverride {
                    target,
                    value: s.clone(),
                }),
                Yaml::Integer(_) | Yaml::Real(_) | Yaml::Boolean(_) => out.push(ModeOverride {
                    target,
                    value: yaml_str_owned(val),
                }),
                other => {
                    return Err(type_err(
                        ctx,
                        &target,
                        "a scalar value or a nested mapping",
                        other,
                    ));
                }
            }
        }
        Ok(())
    }
    walk(section, "", ctx, &mut out)?;
    Ok(out)
}

fn parse_fault_kind(raw: &str, ctx: &str, key: &str) -> Result<FaultKind, ParseError> {
    match raw {
        "omission" => Ok(FaultKind::Omission),
        "late" => Ok(FaultKind::Late),
        "loss" => Ok(FaultKind::Loss),
        "reported" => Ok(FaultKind::Reported),
        other => Err(field_err(
            ctx,
            key,
            &format!(
                "`{other}` is not a fault class — accepted: omission, late, loss, reported \
                 (AADL EMV2's error types, restricted to what a subscriber can observe)"
            ),
        )),
    }
}

fn parse_hazards(doc: &Yaml, ctx: &str) -> Result<BTreeMap<String, HazardDecl>, ParseError> {
    let mut out = BTreeMap::new();
    let section = &doc["hazards"];
    if section.is_badvalue() {
        return Ok(out);
    }
    let hash = section
        .as_hash()
        .ok_or_else(|| type_err(ctx, "hazards", "a mapping", section))?;
    for (k, v) in hash {
        let name = yaml_str_owned(k);
        let hctx = format_path(ctx, &format!("hazards.{name}"));
        reject_unknown_keys(v, &hctx, Context::Hazard)?;
        // A guard is a topic, an `all_of` set, or (phase 75) the NAME of a
        // function declared at scope level — resolved by the loader, which
        // is the layer that knows the scope's functions.
        let mut guards = Vec::new();
        match &v["guards"] {
            Yaml::BadValue | Yaml::Null => {}
            Yaml::Array(items) => {
                for (i, item) in items.iter().enumerate() {
                    let gctx = format_path(&hctx, &format!("guards[{i}]"));
                    guards.push(parse_hazard_guard(item, &gctx, &format!("guards[{i}]"))?);
                }
            }
            other => return Err(type_err(&hctx, "guards", "a list", other)),
        }
        // Phase 82: a set. A scalar is the set of one; `on: []` is refused
        // because an empty set would read as "omitted" (= every class),
        // the opposite of what an empty list looks like it says.
        if matches!(&v["on"], Yaml::Array(a) if a.is_empty()) {
            return Err(field_err(
                &hctx,
                "on",
                "an empty set of fault classes; state at least one fault class or omit the key \
                 (omitted means every class)",
            ));
        }
        let mut on: Vec<FaultKind> = Vec::new();
        for raw in parse_string_or_list(v, "on", &hctx)? {
            let kind = parse_fault_kind(&raw, &hctx, "on")?;
            if !on.contains(&kind) {
                on.push(kind);
            }
        }
        let entry_speed = yaml_f64(v, "entry_speed", &hctx)?;
        if let Some(s) = entry_speed
            && !(s.is_finite() && s > 0.0)
        {
            return Err(field_err(
                &hctx,
                "entry_speed",
                &format!(
                    "`entry_speed: {s}` -- a speed in m/s, positive. It is the operating \
                     domain's speed bound, and a braking settle is derived from it"
                ),
            ));
        }
        // v0.1.59: a hazard either enters a mode (the ladder) or reacts by
        // one scope path, never both. `reaction:` naming a mode is the old
        // spelling of `enters:`; the consumer tells the two apart by lookup.
        let enters = yaml_string(v, "enters", &hctx)?;
        let reaction = yaml_string(v, "reaction", &hctx)?;
        if enters.is_some() && reaction.is_some() {
            return Err(field_err(
                &hctx,
                "enters",
                "a hazard either enters a mode (`enters: <mode>`, the fallback ladder) or \
                 reacts by one scope path (`reaction: <path>`), not both -- keep one",
            ));
        }
        out.insert(
            name,
            HazardDecl {
                severity: yaml_string(v, "severity", &hctx)?,
                description: yaml_string(v, "description", &hctx)?,
                guards,
                on,
                ftti: yaml_bound(v, "ftti", &hctx, Context::Hazard)?,
                enters,
                reaction,
                when: parse_predicate(v, &hctx)?,
                entry_speed,
            },
        );
    }
    Ok(out)
}

fn parse_on_violation(doc: &Yaml, ctx: &str) -> Result<Option<OnViolation>, ParseError> {
    let section = &doc["on_violation"];
    if section.is_badvalue() {
        return Ok(None);
    }
    let ctx = &format_path(ctx, "on_violation");
    reject_unknown_keys(section, ctx, Context::OnViolation)?;
    let on = parse_string_or_list(section, "on", ctx)?
        .iter()
        .map(|s| parse_fault_kind(s, ctx, "on"))
        .collect::<Result<Vec<_>, _>>()?;
    let reaction = yaml_string(section, "reaction", ctx)?.ok_or_else(|| {
        field_err(
            ctx,
            "reaction",
            "required: the path on this node that runs when the assumption is violated",
        )
    })?;
    let mechanism = match yaml_string(section, "mechanism", ctx)?.as_deref() {
        None | Some("qos") => DetectMechanism::Qos,
        Some("diagnostics") => DetectMechanism::Diagnostics,
        Some("application") => DetectMechanism::Application,
        Some(other) => {
            return Err(field_err(
                ctx,
                "mechanism",
                &format!(
                    "`{other}` is not a detection mechanism — accepted: qos, diagnostics, \
                     application"
                ),
            ));
        }
    };
    Ok(Some(OnViolation {
        on,
        reaction,
        within: yaml_bound(section, "within", ctx, Context::OnViolation)?,
        mechanism,
    }))
}

fn parse_safe_state(doc: &Yaml, ctx: &str) -> Result<Option<SafeState>, ParseError> {
    let section = &doc["safe_state"];
    if section.is_badvalue() {
        return Ok(None);
    }
    let ctx = &format_path(ctx, "safe_state");
    reject_unknown_keys(section, ctx, Context::SafeState)?;
    let emits = yaml_string(section, "emits", ctx)?.ok_or_else(|| {
        field_err(
            ctx,
            "emits",
            "required: the endpoint this reaction commands the safe state on",
        )
    })?;
    // Phase 90 G6: `settle: <d>` is shorthand for `settle: { duration: <d> }`;
    // the map is `{ duration | decel, jerk ; response ; source }`.
    let (settle, settle_profile, settle_response, settle_map) = match &section["settle"] {
        Yaml::Hash(_) => {
            let sctx = &format_path(ctx, "settle");
            let map = &section["settle"];
            reject_unknown_keys(map, sctx, Context::Settle)?;
            let profile = if map["decel"].is_badvalue() && map["jerk"].is_badvalue() {
                None
            } else {
                let param = |key: &str| {
                    yaml_string(map, key, sctx)?.ok_or_else(|| {
                        field_err(
                            sctx,
                            key,
                            "required: a braking profile names both its deceleration and its \
                             jerk parameter",
                        )
                    })
                };
                let decel = param("decel")?;
                let jerk = param("jerk")?;
                Some(SettleProfile { decel, jerk })
            };
            let mut literal =
                yaml_bound(map, "duration", sctx, Context::Settle).map_err(|e| reframe(e, sctx))?;
            if literal.is_none() && profile.is_none() {
                return Err(field_err(
                    sctx,
                    "duration",
                    "required: a settle states its duration (`settle: { duration: 2s }`, or \
                     `settle: 2s`) or a braking profile it is derived from (`decel` and `jerk`)",
                ));
            }
            if !map["source"].is_badvalue() {
                let Some(b) = literal.as_mut() else {
                    return Err(field_err(
                        sctx,
                        "source",
                        "says where a written `duration` comes from, and none is written: a \
                         profile's settle is derived from its parameters. A measured or \
                         analysed `response` states its source in its own map form, \
                         `response: { duration, source }`",
                    ));
                };
                b.source = Some(parse_source(&map["source"], sctx)?);
            }
            let response = yaml_bound(map, "response", sctx, Context::Settle)?;
            (literal, profile, response, true)
        }
        _ => (
            yaml_bound(section, "settle", ctx, Context::SafeState)?,
            None,
            None,
            false,
        ),
    };
    Ok(Some(SafeState {
        emits,
        settle,
        settle_profile,
        settle_response,
        settle_map,
    }))
}

// ── Nodes ──

fn parse_nodes(doc: &Yaml, ctx: &str) -> Result<BTreeMap<String, NodeDecl>, ParseError> {
    let mut nodes = BTreeMap::new();
    let section = &doc["nodes"];
    if section.is_badvalue() {
        return Ok(nodes);
    }
    let hash = section
        .as_hash()
        .ok_or_else(|| field_err(ctx, "nodes", "expected mapping"))?;
    for (k, v) in hash {
        let name = yaml_str_owned(k);
        let path = format_path(ctx, &format!("nodes.{name}"));
        let node = parse_node_decl(v, &path)?;
        // Validate endpoint uniqueness
        validate_endpoint_uniqueness(&node, &path)?;
        nodes.insert(name, node);
    }
    Ok(nodes)
}

fn parse_node_decl(yaml: &Yaml, ctx: &str) -> Result<NodeDecl, ParseError> {
    if yaml.is_null() || yaml.is_badvalue() {
        return Ok(NodeDecl::default());
    }
    reject_unknown_keys(yaml, ctx, Context::Node)?;
    Ok(NodeDecl {
        if_condition: yaml_string(yaml, "if", ctx)?,
        unless_condition: yaml_string(yaml, "unless", ctx)?,
        lifecycle: yaml_bool(yaml, "lifecycle", ctx)?,
        publishers: parse_endpoints(yaml, "pub", ctx)?,
        subscribers: parse_endpoints(yaml, "sub", ctx)?,
        srv: parse_srv_endpoints(yaml, "srv", ctx)?,
        cli: parse_endpoints(yaml, "cli", ctx)?,
        paths: parse_paths(yaml, ctx, true)?,
        criticality: parse_criticality(yaml, ctx)?,
        concurrency: parse_concurrency(yaml, ctx)?,
        params: parse_params(yaml, ctx)?,
    })
}

/// `params:` -- the parameters a node declares: a map of parameter name to
/// `{ type: <ROS 2 type> }`.
///
/// The names are the author's, so only the value under each is held to the
/// grammar table ([`Context::Param`]). Every failure is an error rather than
/// a skipped entry: a declaration that parses as absent sizes and checks
/// nothing, in silence -- the defect phases 69 and 70 closed for every other
/// key.
///
/// `None` when the key is absent ("not stated"), `Some(empty)` for
/// `params: {}` ("declares no parameters"). The two are different statements
/// and a consumer that sizes a store from every node's declarations tells
/// them apart, so they must not collapse here.
fn parse_params(doc: &Yaml, ctx: &str) -> Result<Option<BTreeMap<String, ParamDecl>>, ParseError> {
    let mut out = BTreeMap::new();
    let hash = match &doc["params"] {
        Yaml::BadValue => return Ok(None),
        Yaml::Hash(h) => h,
        other => {
            return Err(type_err(
                ctx,
                "params",
                "a mapping of parameter name to `{ type: ... }`",
                other,
            ));
        }
    };
    let accepted = ParamType::ALL.map(ParamType::as_str).join(", ");
    for (k, v) in hash {
        let name = yaml_str_owned(k);
        if name.is_empty() {
            return Err(field_err(
                ctx,
                "params",
                "a parameter name must be a non-empty scalar",
            ));
        }
        let key = format!("params.{name}");
        let path = format_path(ctx, &key);
        match v {
            Yaml::Hash(_) => {}
            Yaml::Null | Yaml::BadValue => {
                return Err(field_err(
                    &path,
                    "type",
                    &format!("missing -- a declared parameter needs `{{ type: <{accepted}> }}`"),
                ));
            }
            Yaml::String(s) if s.parse::<ParamType>().is_ok() => {
                return Err(field_err(
                    ctx,
                    &key,
                    &format!(
                        "expected a mapping -- write `{{ type: {s} }}`. The map form is what \
                         leaves room for later fields"
                    ),
                ));
            }
            other => {
                return Err(type_err(ctx, &key, "a mapping `{ type: ... }`", other));
            }
        }
        reject_unknown_keys(v, &path, Context::Param)?;
        let Some(raw) = yaml_string(v, "type", &path)? else {
            return Err(field_err(
                &path,
                "type",
                &format!("missing -- a declared parameter needs `{{ type: <{accepted}> }}`"),
            ));
        };
        let ty = raw.parse::<ParamType>().map_err(|()| {
            field_err(
                &path,
                "type",
                &format!(
                    "`{raw}` is not a ROS 2 parameter type -- accepted: {accepted}. An unknown \
                     type is never skipped: a declaration without a type sizes and checks nothing"
                ),
            )
        })?;
        out.insert(name, ParamDecl { ty });
    }
    Ok(Some(out))
}

/// Parse endpoints: either a list `[a, b]` or a map `{a: {min_rate_hz: 10}, b: null}`.
fn parse_endpoints(
    doc: &Yaml,
    key: &str,
    ctx: &str,
) -> Result<BTreeMap<String, EndpointProps>, ParseError> {
    let mut eps = BTreeMap::new();
    let section = &doc[key];
    if section.is_badvalue() {
        return Ok(eps);
    }
    match section {
        Yaml::Array(arr) => {
            for item in arr {
                let name = yaml_str_owned(item);
                reject_endpoint_path(ctx, key, &name)?;
                eps.insert(name, EndpointProps::default());
            }
        }
        Yaml::Hash(hash) => {
            for (k, v) in hash {
                let name = yaml_str_owned(k);
                reject_endpoint_path(ctx, key, &name)?;
                let path = format_path(ctx, &format!("{key}.{name}"));
                let props = parse_endpoint_props(v, &path)?;
                if key != "pub" && props.on_demand.is_some() {
                    return Err(field_err(
                        &path,
                        "on_demand",
                        "`on_demand` is a publisher's statement (it publishes when asked and \
                         promises no rate); a subscriber that needs no rate simply states no \
                         `min_rate_hz`",
                    ));
                }
                eps.insert(name, props);
            }
        }
        _ => {
            return Err(field_err(ctx, key, "expected list or mapping"));
        }
    }
    Ok(eps)
}

/// Refuse a `/` in an endpoint key (`pub:`, `sub:`, `srv:`, `cli:`).
///
/// An endpoint key is a LOCAL name: the consumer prefixes the node's FQN to
/// it, so `sub: { /abs/topic: ... }` became the endpoint `/node//abs/topic`,
/// which matches no topic and was dropped in silence (the Autoware Safety
/// Island's contract had to explain this in its head comment). A relative
/// `a/b` is refused too: a `topics:` list names an endpoint as
/// `node/endpoint` and splits at the last `/`, so the key cannot carry one.
/// Where the endpoint is connected is the launch file's remap or the
/// `topics:` entry's business, never the key's.
fn reject_endpoint_path(ctx: &str, section: &str, name: &str) -> Result<(), ParseError> {
    if !name.contains('/') {
        return Ok(());
    }
    Err(field_err(
        ctx,
        &format!("{section}.{name}"),
        &format!(
            "endpoint key `{name}` contains `/`: an endpoint key is a local name, and the \
             node's FQN is prefixed to it (`{name}` would become `<node>/{name}`). Use the \
             endpoint's local name; remap the topic in the launch file or wire it under \
             `topics:`"
        ),
    ))
}

fn parse_endpoint_props(yaml: &Yaml, ctx: &str) -> Result<EndpointProps, ParseError> {
    if yaml.is_null() || yaml.is_badvalue() {
        return Ok(EndpointProps::default());
    }
    // `jitter:` on an endpoint was removed (phase 68). It was declared, copied
    // into the model, and read by NOTHING — no check, no mapper, no runtime
    // monitor, on either side of the toolchain. Jitter is a property of a
    // route, not of a single endpoint: what destabilises a controller is how
    // much the END-TO-END latency varies, and one publisher's spread does not
    // determine that. `max_jitter` on a path or scope path is the requirement
    // that replaced it, and `jitter-feasibility` checks it against the
    // sampling jitter the route already carries.
    if !yaml["jitter"].is_badvalue() || !yaml["jitter_ms"].is_badvalue() {
        return Err(field_err(
            ctx,
            "jitter",
            "`jitter:` on an endpoint was removed — nothing ever read it, and \
             jitter is a property of a ROUTE rather than of one endpoint. \
             Declare it on the path or scope path whose end-to-end variation \
             you mean:\n\
             \x20 paths:\n\
             \x20   <path name>:\n\
             \x20     max_jitter: <the tolerable spread>\n\
             `jitter-feasibility` then checks it against the sampling jitter \
             the route already carries — one whole period per clock boundary \
             crossed, which no priority assignment can reduce.",
        ));
    }
    reject_unknown_keys(yaml, ctx, Context::Endpoint)?;
    let props = EndpointProps {
        min_rate_hz: yaml_rate_bound(yaml, "min_rate_hz", ctx, Context::Endpoint)?,
        max_rate_hz: yaml_rate_bound(yaml, "max_rate_hz", ctx, Context::Endpoint)?,
        max_age: yaml_bound(yaml, "max_age", ctx, Context::Endpoint)?,
        state: yaml_bool(yaml, "state", ctx)?,
        required: yaml_bool(yaml, "required", ctx)?,
        qos: parse_qos(yaml, ctx)?,
        max_transport: yaml_bound(yaml, "max_transport", ctx, Context::Endpoint)?,
        buffer: parse_buffer(yaml, ctx)?,
        on_violation: parse_on_violation(yaml, ctx)?,
        on_demand: yaml_bool(yaml, "on_demand", ctx)?,
    };
    if props.on_demand == Some(true) && props.min_rate_hz.is_some() {
        return Err(field_err(
            ctx,
            "on_demand",
            "`on_demand: true` says this publisher promises no rate, and `min_rate_hz` promises \
             one: state one or the other",
        ));
    }
    if props.buffer.is_some() && props.state != Some(true) {
        return Err(field_err(
            ctx,
            "buffer",
            "'buffer' is only meaningful on a subscriber with 'state: true'",
        ));
    }
    Ok(props)
}

/// Parse the `buffer:` discriminator (Vocabulary v2, Phase 44.1 §3).
/// `criticality` is a closed set. It used to accept any string, and the one
/// reader (`sched_derive::parse_criticality`) answered an unknown value with a
/// debug log and `None` — so `criticality: urgent` scheduled a node exactly as
/// if nothing had been declared, in silence. Same defect as an unknown key or
/// a wrong type: a declaration deleted without a word.
fn parse_criticality(doc: &Yaml, ctx: &str) -> Result<Option<String>, ParseError> {
    let Some(raw) = yaml_string(doc, "criticality", ctx)? else {
        return Ok(None);
    };
    match raw.as_str() {
        "high" | "medium" | "low" => Ok(Some(raw)),
        other => Err(field_err(
            ctx,
            "criticality",
            &format!(
                "`{other}` is not a criticality — accepted: high, medium, low. An unknown \
                 value used to be read as no declaration at all"
            ),
        )),
    }
}

fn parse_buffer(doc: &Yaml, ctx: &str) -> Result<Option<Buffer>, ParseError> {
    let raw = match yaml_string(doc, "buffer", ctx)? {
        Some(s) => s,
        None => return Ok(None),
    };
    match raw.as_str() {
        "latest" => Ok(Some(Buffer::Latest)),
        "queue" => Ok(Some(Buffer::Queue)),
        other => Err(field_err(
            ctx,
            "buffer",
            &format!("invalid buffer '{other}', expected 'latest' or 'queue'"),
        )),
    }
}

fn parse_srv_endpoints(
    doc: &Yaml,
    key: &str,
    ctx: &str,
) -> Result<BTreeMap<String, SrvEndpointProps>, ParseError> {
    let mut eps = BTreeMap::new();
    let section = &doc[key];
    if section.is_badvalue() {
        return Ok(eps);
    }
    match section {
        Yaml::Array(arr) => {
            for item in arr {
                let name = yaml_str_owned(item);
                reject_endpoint_path(ctx, key, &name)?;
                eps.insert(name, SrvEndpointProps::default());
            }
        }
        Yaml::Hash(hash) => {
            for (k, v) in hash {
                let name = yaml_str_owned(k);
                reject_endpoint_path(ctx, key, &name)?;
                reject_unknown_keys(v, ctx, Context::SrvEndpoint)?;
                let props = SrvEndpointProps {
                    max_response: if v.is_null() || v.is_badvalue() {
                        None
                    } else {
                        yaml_bound(
                            v,
                            "max_response",
                            &format_path(ctx, &name),
                            Context::SrvEndpoint,
                        )?
                    },
                };
                eps.insert(name, props);
            }
        }
        _ => {
            return Err(field_err(ctx, key, "expected list or mapping"));
        }
    }
    Ok(eps)
}

fn validate_endpoint_uniqueness(node: &NodeDecl, ctx: &str) -> Result<(), ParseError> {
    let mut seen = std::collections::HashSet::new();
    for name in node
        .publishers
        .keys()
        .chain(node.subscribers.keys())
        .chain(node.srv.keys())
        .chain(node.cli.keys())
    {
        if !seen.insert(name.as_str()) {
            return Err(field_err(
                ctx,
                name,
                "duplicate endpoint name across pub/sub/srv/cli",
            ));
        }
    }
    Ok(())
}

// ── Topics ──

fn parse_topics(doc: &Yaml, ctx: &str) -> Result<BTreeMap<String, TopicDecl>, ParseError> {
    let mut topics = BTreeMap::new();
    let section = &doc["topics"];
    if section.is_badvalue() {
        return Ok(topics);
    }
    let hash = section
        .as_hash()
        .ok_or_else(|| field_err(ctx, "topics", "expected mapping"))?;
    for (k, v) in hash {
        let name = yaml_str_owned(k);
        let path = format_path(ctx, &format!("topics.{name}"));
        let topic = parse_topic_decl(v, &path)?;
        topics.insert(name, topic);
    }
    Ok(topics)
}

fn parse_topic_decl(yaml: &Yaml, ctx: &str) -> Result<TopicDecl, ParseError> {
    reject_unknown_keys(yaml, ctx, Context::Topic)?;
    // Shorthand: just a type string
    if let Some(s) = yaml.as_str() {
        return Ok(TopicDecl {
            if_condition: None,
            unless_condition: None,
            msg_type: s.to_string(),
            publishers: vec![],
            subscribers: vec![],
            qos: None,
            rate_hz: None,
            max_transport: None,
            drop: None,
            external: None,
        });
    }
    Ok(TopicDecl {
        if_condition: yaml_string(yaml, "if", ctx)?,
        unless_condition: yaml_string(yaml, "unless", ctx)?,
        msg_type: yaml_string(yaml, "type", ctx)?
            .ok_or_else(|| field_err(ctx, "type", "topic must have a type"))?,
        publishers: yaml_string_list(yaml, "pub", ctx)?,
        subscribers: yaml_string_list(yaml, "sub", ctx)?,
        qos: parse_qos(yaml, ctx)?,
        rate_hz: yaml_f64(yaml, "rate_hz", ctx)?,
        max_transport: yaml_bound(yaml, "max_transport", ctx, Context::Topic)?,
        drop: parse_drop_spec(yaml, "drop", ctx)?,
        external: parse_external_side(yaml, "external", ctx)?,
    })
}

/// Parse the manifest-level `external_topics:` block.
fn parse_external_topics(
    doc: &Yaml,
    ctx: &str,
) -> Result<BTreeMap<String, ExternalTopicDecl>, ParseError> {
    let mut out = BTreeMap::new();
    let section = &doc["external_topics"];
    if section.is_badvalue() {
        return Ok(out);
    }
    let hash = section
        .as_hash()
        .ok_or_else(|| field_err(ctx, "external_topics", "expected mapping"))?;
    for (k, v) in hash {
        let name = yaml_str_owned(k);
        let path = format_path(ctx, &format!("external_topics.{name}"));
        reject_unknown_keys(v, &path, Context::ExternalTopic)?;
        // Accept either `side:` or `external:` as the side selector.
        // Try whichever is present; if both are present, `side:` wins.
        let side_present = !v["side"].is_badvalue();
        let external_present = !v["external"].is_badvalue();
        let side = if side_present {
            parse_external_side(v, "side", &path)?
        } else if external_present {
            parse_external_side(v, "external", &path)?
        } else {
            None
        };
        let side = side.ok_or_else(|| {
            field_err(
                &path,
                "side",
                "external_topics entry must have a 'side: pub|sub|both' (or 'external:') field",
            )
        })?;
        out.insert(
            name,
            ExternalTopicDecl {
                side,
                msg_type: yaml_string(v, "type", ctx)?,
                qos: parse_qos(v, &path)?,
            },
        );
    }
    Ok(out)
}

/// Parse an `ExternalSide` enum from a string-valued YAML field.
fn parse_external_side(
    doc: &Yaml,
    key: &str,
    ctx: &str,
) -> Result<Option<ExternalSide>, ParseError> {
    let raw = match yaml_string(doc, key, ctx)? {
        Some(s) => s,
        None => return Ok(None),
    };
    match raw.as_str() {
        "pub" => Ok(Some(ExternalSide::Pub)),
        "sub" => Ok(Some(ExternalSide::Sub)),
        "both" => Ok(Some(ExternalSide::Both)),
        _ => Err(field_err(
            ctx,
            key,
            &format!("invalid external side '{raw}', expected 'pub', 'sub', or 'both'"),
        )),
    }
}

/// Parse an `external:` selector on a service or action.
///
/// Separate from [`parse_external_side`] because the vocabulary differs:
/// `server`/`client` here, `pub`/`sub` there. Sharing one parser would accept
/// `external: pub` on a service, which names nothing.
fn parse_external_endpoint_side(
    doc: &Yaml,
    key: &str,
    ctx: &str,
) -> Result<Option<ExternalEndpointSide>, ParseError> {
    let raw = match yaml_string(doc, key, ctx)? {
        Some(s) => s,
        None => return Ok(None),
    };
    match raw.as_str() {
        "server" => Ok(Some(ExternalEndpointSide::Server)),
        "client" => Ok(Some(ExternalEndpointSide::Client)),
        "both" => Ok(Some(ExternalEndpointSide::Both)),
        _ => Err(field_err(
            ctx,
            key,
            &format!("invalid external side '{raw}', expected 'server', 'client', or 'both'"),
        )),
    }
}

// ── Services & Actions ──

fn parse_services(doc: &Yaml, ctx: &str) -> Result<BTreeMap<String, ServiceDecl>, ParseError> {
    let mut out = BTreeMap::new();
    let section = &doc["services"];
    if section.is_badvalue() {
        return Ok(out);
    }
    let hash = section
        .as_hash()
        .ok_or_else(|| field_err(ctx, "services", "expected mapping"))?;
    for (k, v) in hash {
        let name = yaml_str_owned(k);
        reject_unknown_keys(v, ctx, Context::Service)?;
        out.insert(
            name,
            ServiceDecl {
                if_condition: yaml_string(v, "if", ctx)?,
                unless_condition: yaml_string(v, "unless", ctx)?,
                srv_type: yaml_string(v, "type", ctx)?.unwrap_or_default(),
                server: yaml_string_list(v, "server", ctx)?,
                client: yaml_string_list(v, "client", ctx)?,
                external: parse_external_endpoint_side(v, "external", ctx)?,
            },
        );
    }
    Ok(out)
}

fn parse_actions(doc: &Yaml, ctx: &str) -> Result<BTreeMap<String, ActionDecl>, ParseError> {
    let mut out = BTreeMap::new();
    let section = &doc["actions"];
    if section.is_badvalue() {
        return Ok(out);
    }
    let hash = section
        .as_hash()
        .ok_or_else(|| field_err(ctx, "actions", "expected mapping"))?;
    for (k, v) in hash {
        let name = yaml_str_owned(k);
        reject_unknown_keys(v, ctx, Context::Action)?;
        out.insert(
            name,
            ActionDecl {
                if_condition: yaml_string(v, "if", ctx)?,
                unless_condition: yaml_string(v, "unless", ctx)?,
                action_type: yaml_string(v, "type", ctx)?.unwrap_or_default(),
                server: yaml_string_list(v, "server", ctx)?,
                client: yaml_string_list(v, "client", ctx)?,
                external: parse_external_endpoint_side(v, "external", ctx)?,
            },
        );
    }
    Ok(out)
}

// ── Includes ──

fn parse_includes(doc: &Yaml, ctx: &str) -> Result<BTreeMap<String, IncludeDecl>, ParseError> {
    let mut out = BTreeMap::new();
    let section = &doc["includes"];
    if section.is_badvalue() {
        return Ok(out);
    }
    let hash = section
        .as_hash()
        .ok_or_else(|| field_err(ctx, "includes", "expected mapping"))?;
    for (k, v) in hash {
        let name = yaml_str_owned(k);
        let path = format_path(ctx, &format!("includes.{name}"));
        // The file-reference form `{ manifest: <file> }` was removed in
        // v0.1.59 (phase 90 G7): the removal message names the inline form.
        if !v["manifest"].is_badvalue() {
            let removed = field_table::lookup(Context::IncludeExternal, "manifest")
                .expect("the removed row is in the table");
            return Err(field_err(&path, "manifest", removed.doc));
        }
        // `if:`/`unless:` (issue #0043) sit at the inline manifest's root.
        let if_condition = yaml_string(v, "if", &path)?;
        let unless_condition = yaml_string(v, "unless", &path)?;
        let kind = IncludeKind::Inline(Box::new(parse_manifest_yaml(v, &path)?));
        out.insert(
            name,
            IncludeDecl {
                if_condition,
                unless_condition,
                kind,
            },
        );
    }
    Ok(out)
}

// ── Args ──

/// Parse `args:` section. All args are mandatory — values from scope table.
///
/// Accepts:
/// - List form: `args: [a, b]` → all free strings
/// - Map form with null values: `args: { a:, b: }` → all free strings
/// - Map form with type: `args: { x: { type: bool } }` → typed
/// - Map form with choices: `args: { x: { choices: [a, b] } }` → enum
fn parse_args(doc: &Yaml, ctx: &str) -> Result<BTreeMap<String, ArgDecl>, ParseError> {
    let mut out = BTreeMap::new();
    let section = &doc["args"];
    match section {
        Yaml::BadValue | Yaml::Null => {}
        Yaml::Array(arr) => {
            for name in yaml_scalar_list(arr, "args", ctx)? {
                out.insert(name, ArgDecl::String);
            }
        }
        Yaml::Hash(hash) => {
            for (k, v) in hash {
                let name = yaml_str_owned(k);
                let decl = parse_arg_decl(v, &format_path(ctx, &format!("args.{name}")))?;
                out.insert(name, decl);
            }
        }
        other => return Err(type_err(ctx, "args", "a list of names or a mapping", other)),
    }
    Ok(out)
}
fn parse_arg_decl(yaml: &Yaml, ctx: &str) -> Result<ArgDecl, ParseError> {
    if yaml.is_null() || yaml.is_badvalue() {
        return Ok(ArgDecl::String);
    }
    // Check for { type: bool }
    if let Some(type_str) = yaml_string(yaml, "type", ctx)? {
        if type_str == "bool" {
            return Ok(ArgDecl::Bool);
        }
        // "string" or any other type → free string
        return Ok(ArgDecl::String);
    }
    // Check for { choices: [a, b, c] }
    let choices = yaml_string_list(yaml, "choices", ctx)?;
    if !choices.is_empty() {
        return Ok(ArgDecl::Choices(choices));
    }
    Ok(ArgDecl::String)
}

// ── Paths ──

/// The message of a node path's `drop:` (removed in v0.1.60, phase 90 G3).
pub const NODE_PATH_DROP_REMOVED: &str = "Removed in v0.1.60 (phase 90 G3) -- a node path's \
     `drop:` was read by nothing (no rule, monitor or code generator), while the same loss \
     stated on the topic is: the rate rule, the loss detector's interval and the runtime drop \
     monitor read `topics.<t>.drop`. State it on the topic the path outputs, \
     `topics: { <t>: { drop: ... } }`; `play_launch migrate` moves it when the path has one \
     output. On a scope path `drop:` stays: the end-to-end loss budget.";

/// `paths:` of a manifest level (`node == false`: scope paths) or of a
/// node (`node == true`).
fn parse_paths(
    doc: &Yaml,
    ctx: &str,
    node: bool,
) -> Result<BTreeMap<String, PathDecl>, ParseError> {
    let mut out = BTreeMap::new();
    let section = &doc["paths"];
    if section.is_badvalue() {
        return Ok(out);
    }
    let hash = section
        .as_hash()
        .ok_or_else(|| field_err(ctx, "paths", "expected mapping"))?;
    for (k, v) in hash {
        let name = yaml_str_owned(k);
        let path_ctx = format_path(ctx, &format!("paths.{name}"));
        if node && !v["drop"].is_badvalue() {
            return Err(field_err(&path_ctx, "drop", NODE_PATH_DROP_REMOVED));
        }
        let path = parse_path_decl(v, &path_ctx)?;
        out.insert(name, path);
    }
    Ok(out)
}

fn parse_path_decl(yaml: &Yaml, ctx: &str) -> Result<PathDecl, ParseError> {
    // `correlation:` was removed (phase 70). Parsed, exported to the causal
    // graph, lowered to a model enum — and branched on by NOTHING: no check,
    // no mapper, no monitor. `sync:` is what states how a fan-in path treats
    // its inputs, and `sync-feasibility` / `sync-budget` read that.
    if !yaml["correlation"].is_badvalue() {
        return Err(field_err(
            ctx,
            "correlation",
            "`correlation:` was removed — nothing ever read it. A fan-in path \
             states its policy with `sync:`, which IS checked:\n\
             \x20 sync: { policy: approximate, max_interval: <window> }   # was `timestamp`\n\
             \x20 (no `sync:` at all)                                       # was `latest`\n\
             `tolerance:` stays: it is the stamp spread the callback accepts, \
             and `sync-budget` checks it against `max_latency`.",
        ));
    }
    reject_unknown_keys(yaml, ctx, Context::Path)?;
    let input = parse_string_or_list(yaml, "input", ctx)?;
    let trigger = parse_trigger(yaml, ctx)?;

    // Vocabulary v2 §1: explicit trigger.input and legacy input: may both
    // be present; they must agree (as sets) or it's a parse error.
    if let Some(Trigger::Input(trigger_eps)) = &trigger
        && !input.is_empty()
    {
        let mut a: Vec<&str> = trigger_eps.iter().map(String::as_str).collect();
        let mut b: Vec<&str> = input.iter().map(String::as_str).collect();
        a.sort_unstable();
        b.sort_unstable();
        if a != b {
            return Err(field_err(
                ctx,
                "trigger.input",
                "disagrees with the legacy 'input:' list on this path",
            ));
        }
    }

    let sync = parse_sync(yaml, ctx)?;

    let path = PathDecl {
        if_condition: yaml_string(yaml, "if", ctx)?,
        unless_condition: yaml_string(yaml, "unless", ctx)?,
        input,
        output: yaml_string_list(yaml, "output", ctx)?,
        max_latency: yaml_bound(yaml, "max_latency", ctx, Context::Path)?,
        tolerance: yaml_duration(yaml, "tolerance")?,
        drop: parse_drop_spec(yaml, "drop", ctx)?,
        trigger,
        sync,
        max_jitter: yaml_bound(yaml, "max_jitter", ctx, Context::Path)?,
        min_latency: yaml_bound(yaml, "min_latency", ctx, Context::Path)?,
        miss: parse_miss_spec(yaml, ctx)?,
        safe_state: parse_safe_state(yaml, ctx)?,
        timer_param: parse_timer_param(yaml, ctx)?,
    };

    // Vocabulary v2 §2: sync only meaningful with an input trigger with
    // >=2 endpoints — cheap and unambiguous to check at parse time.
    if path.sync.is_some() {
        let ok = matches!(
            path.effective_trigger(),
            EffectiveTrigger::Input(eps) if eps.len() >= 2
        );
        if !ok {
            return Err(field_err(
                ctx,
                "sync",
                "'sync:' requires an input trigger with at least 2 endpoints",
            ));
        }
    }

    Ok(path)
}

// ── Trigger ──

/// Parse `trigger:` (Vocabulary v2, Phase 44.1 §1). Bare-string forms
/// (`once`, `spontaneous`) or single-key mapping forms (`timer:`,
/// `input:`).
fn parse_trigger(doc: &Yaml, ctx: &str) -> Result<Option<Trigger>, ParseError> {
    let section = &doc["trigger"];
    if section.is_badvalue() {
        return Ok(None);
    }
    if let Some(s) = section.as_str() {
        return match s {
            "once" => Ok(Some(Trigger::Once)),
            "spontaneous" => Ok(Some(Trigger::Spontaneous)),
            other => Err(field_err(
                ctx,
                "trigger",
                &format!(
                    "invalid trigger '{other}', expected 'once', 'spontaneous', \
                     or a mapping with 'timer' or 'input'"
                ),
            )),
        };
    }
    let hash = section.as_hash().ok_or_else(|| {
        field_err(
            ctx,
            "trigger",
            "expected 'once', 'spontaneous', or a mapping with 'timer' or 'input'",
        )
    })?;
    if hash.len() != 1 {
        return Err(field_err(
            ctx,
            "trigger",
            "expected exactly one of 'timer' or 'input'",
        ));
    }
    let (k, v) = hash.iter().next().expect("len == 1");
    let key = yaml_str_owned(k);
    match key.as_str() {
        "timer" => {
            reject_unknown_keys(v, ctx, Context::TriggerTimer)?;
            let rate_hz = yaml_f64(v, "rate_hz", ctx)?
                .ok_or_else(|| field_err(ctx, "trigger.timer", "requires 'rate_hz'"))?;
            if rate_hz <= 0.0 {
                return Err(field_err(ctx, "trigger.timer.rate_hz", "must be > 0"));
            }
            let jitter = yaml_bound(
                v,
                "jitter",
                &format_path(ctx, "trigger.timer"),
                Context::TriggerTimer,
            )?;
            if let Some(j) = jitter.as_ref().and_then(Bound::duration)
                && j.as_millis_f64() >= 1000.0 / rate_hz
            {
                return Err(field_err(
                    ctx,
                    "trigger.timer.jitter",
                    &format!(
                        "a release jitter of {}ms is a whole period ({}ms at {rate_hz} Hz) or \
                         more: a tick that late is a missed tick, not a late one",
                        j.as_millis_f64(),
                        1000.0 / rate_hz
                    ),
                ));
            }
            Ok(Some(Trigger::Timer { rate_hz, jitter }))
        }
        "input" => {
            let endpoints = yaml_direct_string_list(v, "input", ctx)?;
            Ok(Some(Trigger::Input(endpoints)))
        }
        other => Err(field_err(
            ctx,
            "trigger",
            &format!("invalid trigger key '{other}', expected 'timer' or 'input'"),
        )),
    }
}

/// `trigger: { timer: { param | period_param } }` (phase 10 B1): the
/// parameter of the path's node that sets the timer's period.
fn parse_timer_param(doc: &Yaml, ctx: &str) -> Result<Option<TimerParam>, ParseError> {
    let timer = &doc["trigger"]["timer"];
    if !timer.is_hash() {
        return Ok(None);
    }
    let tctx = &format_path(ctx, "trigger.timer");
    match (
        yaml_string(timer, "param", tctx)?,
        yaml_string(timer, "period_param", tctx)?,
    ) {
        (None, None) => Ok(None),
        (Some(r), None) => Ok(Some(TimerParam::Rate(ParamBinding::Own(r)))),
        (None, Some(p)) => Ok(Some(TimerParam::Period(ParamBinding::Own(p)))),
        (Some(_), Some(_)) => Err(field_err(
            tctx,
            "period_param",
            "a timer is bound to ONE parameter: its rate (`param`, Hz) or its period \
             (`period_param`, seconds), not both",
        )),
    }
}

// ── Sync ──

/// Parse `sync:` (Vocabulary v2, Phase 44.1 §2).
fn parse_sync(doc: &Yaml, ctx: &str) -> Result<Option<Sync>, ParseError> {
    let section = &doc["sync"];
    if section.is_badvalue() {
        return Ok(None);
    }
    // Nested block: diagnostics name the block, not its parent.
    let ctx = &format_path(ctx, "sync");
    reject_unknown_keys(section, ctx, Context::Sync)?;
    let policy_str = yaml_string(section, "policy", ctx)?.ok_or_else(|| {
        field_err(
            ctx,
            "policy",
            "required: 'exact', 'approximate', or 'timeout_any'",
        )
    })?;
    let policy = match policy_str.as_str() {
        "exact" => SyncPolicy::Exact,
        "approximate" => SyncPolicy::Approximate,
        "timeout_any" => SyncPolicy::TimeoutAny,
        other => {
            return Err(field_err(
                ctx,
                "policy",
                &format!(
                    "invalid policy '{other}', expected 'exact', 'approximate', or 'timeout_any'"
                ),
            ));
        }
    };
    let max_interval = yaml_duration(section, "max_interval")?;
    let timeout = yaml_duration(section, "timeout")?;
    match policy {
        SyncPolicy::TimeoutAny => {
            if timeout.is_none() {
                return Err(field_err(
                    ctx,
                    "timeout",
                    "required when policy is 'timeout_any'",
                ));
            }
        }
        SyncPolicy::Exact | SyncPolicy::Approximate => {
            if max_interval.is_none() {
                return Err(field_err(
                    ctx,
                    "max_interval",
                    "required when policy is 'exact' or 'approximate'",
                ));
            }
        }
    }
    Ok(Some(Sync {
        policy,
        max_interval,
        timeout,
    }))
}

// ── Chains (removed) ──

/// Reject a manifest that still carries the retired `chains:` section.
///
/// `chains:` named a route hop by hop. The route is derivable from the facts a
/// contract must declare anyway — a path's `trigger:` says what causes it, its
/// `output:` says what it publishes, and the topic graph joins the two — so an
/// authored `segments:` list was a second copy of something the tool already
/// knows, and `chain-link` existed solely to catch the two disagreeing.
///
/// This is a hard error, not a silent drop, for the reason phase 47 made
/// `record.json` a clap failure rather than a warning: a contract whose
/// end-to-end budget is quietly ignored still resolves, still produces a
/// schedule, and the missing requirement shows up as a missed deadline on a
/// running system rather than as a message.
///
/// # On `semantics: age`
///
/// A scope path has no `semantics:` field, so this looks like it drops a
/// requirement. It does not: **nothing ever branched on `ChainSemantics`** —
/// no check, no mapper, no arithmetic. `age` and `reaction` produced identical
/// results, which made it a write-only field of exactly the kind
/// `contract-axes.md` §2 is about. The fact that actually expresses staleness
/// today is a subscriber's `max_age:`, which the `lifespan-age` rule reads.
/// Reject a key the schema does not define in this context.
///
/// Before phase 69 an unrecognised key was silently discarded, so
/// `max_latencyy: 5ms` deleted a budget and `rate_hzz: 100` deleted the
/// declaration that the `derivable-rate` rule reads — silencing the one
/// diagnostic that pointed at the mistake. Measured across the whole
/// 42-file corpus, exactly ONE key was affected (`srv.<ep>.request`), which
/// is why this is a hard error rather than a deprecation window.
///
/// Contexts whose keys are chosen by the author (`nodes:`, `topics:`,
/// `pub:`, …) have no [`Context`] variant, so they cannot reach this
/// function at all — the exemption is structural, not an omission.
///
/// A [`Status::Removed`] key is rejected here too, but only when no
/// dedicated check has already fired: `chains:` and `jitter:` are caught
/// earlier by handlers that explain the migration, and this is the backstop
/// for the ones that are not (`segments:`).
fn reject_unknown_keys(doc: &Yaml, ctx: &str, context: Context) -> Result<(), ParseError> {
    let Some(hash) = doc.as_hash() else {
        return Ok(());
    };
    for k in hash.keys() {
        let Some(key) = k.as_str() else {
            continue;
        };
        match field_table::lookup(context, key) {
            Some(f) if f.status == field_table::Status::Removed => {
                return Err(field_err(ctx, key, f.doc));
            }
            Some(_) => continue,
            None => {
                let near = field_table::nearest_all(context, key);
                let detail = match near.as_slice() {
                    [] => format!(
                        "accepted here: {}",
                        field_table::suggestions(context).join(", ")
                    ),
                    [one] => format!("did you mean `{one}`?"),
                    many => format!(
                        "did you mean one of {}?",
                        many.iter()
                            .map(|k| format!("`{k}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                };
                let err = field_err(
                    ctx,
                    key,
                    &format!(
                        "unknown key in `{}` — {detail}. An unrecognised key used to be \
                         discarded in silence, which deleted whatever it was meant to \
                         declare",
                        context.label()
                    ),
                );
                // Phase 85 I10: inside a manifest parse, keep going and
                // report every unknown key of the file in one refusal.
                if let Some(err) = collect_unknown_key(err) {
                    return Err(err);
                }
            }
        }
    }
    Ok(())
}

thread_local! {
    /// Unknown-key refusals gathered during one [`parse_manifest_str`] call
    /// (phase 85 I10). `None` outside one: [`reject_unknown_keys`] then
    /// returns the first refusal, as before.
    static UNKNOWN_KEYS: std::cell::RefCell<Option<Vec<ParseError>>> =
        const { std::cell::RefCell::new(None) };
}

/// Hold `err` for the end of the parse when a collection is open; hand it
/// back to be returned at once when none is.
fn collect_unknown_key(err: ParseError) -> Option<ParseError> {
    UNKNOWN_KEYS.with(|u| match u.borrow_mut().as_mut() {
        Some(list) => {
            list.push(err);
            None
        }
        None => Some(err),
    })
}

/// Run `parse` collecting every unknown key, and fold them into ONE refusal
/// (phase 85 I10). A file with two typos used to take two runs to find
/// both. An unknown key is skipped by the parser (it reads known keys only),
/// so parsing on past one is safe; any other error still stops the parse
/// and is reported after the unknown keys found before it. One unknown key
/// alone reads exactly as it always did.
fn with_unknown_keys_collected<T>(
    parse: impl FnOnce() -> Result<T, ParseError>,
) -> Result<T, ParseError> {
    let outer = UNKNOWN_KEYS.with(|u| u.borrow_mut().replace(Vec::new()));
    let result = parse();
    let found = UNKNOWN_KEYS
        .with(|u| std::mem::replace(&mut *u.borrow_mut(), outer))
        .unwrap_or_default();
    let mut found = found.into_iter();
    let Some(first) = found.next() else {
        return result;
    };
    // The "used to be discarded" sentence is said once, by the first.
    let rest: Vec<String> = found
        .map(|e| {
            e.to_string().replace(
                ". An unrecognised key used to be discarded in silence, which deleted \
                     whatever it was meant to declare",
                "",
            )
        })
        .chain(result.err().map(|e| e.to_string()))
        .collect();
    if rest.is_empty() {
        return Err(first);
    }
    let ParseError::Field { path, message } = first else {
        return Err(first);
    };
    Err(ParseError::Field {
        path,
        message: format!(
            "{message}. Also refused in this file ({} more): {}",
            rest.len(),
            rest.join("; ")
        ),
    })
}

fn reject_chains(doc: &Yaml, ctx: &str) -> Result<(), ParseError> {
    if doc["chains"].is_badvalue() {
        return Ok(());
    }
    Err(field_err(
        ctx,
        "chains",
        "`chains:`/`segments:` were removed — a written route is a second copy \
         of the graph. State the same requirement as a scope path, which names \
         only its two ends and lets the route be derived from the `trigger:` \
         and `output:` facts the nodes already declare:\n\
         \x20 paths:\n\
         \x20   <chain name>:\n\
         \x20     trigger: { input: [<first topic>] }\n\
         \x20     output: [<last topic>]\n\
         \x20     max_latency: <the chain's max_latency>\n\
         A `semantics:` line can be dropped: nothing ever branched on it, and \
         a subscriber's `max_age:` is what states staleness today.",
    ))
}

// ── Drop ──

/// An `N / W` count bound.
fn count_bound(context: Context, key: &str, c: DropCount) -> Bound {
    spelled(context, key, Quantity::CountPer(c))
}

fn parse_drop_spec(doc: &Yaml, key: &str, ctx: &str) -> Result<Option<DropSpec>, ParseError> {
    let section = &doc[key];
    if section.is_badvalue() {
        return Ok(None);
    }
    // Nested block: diagnostics name the block, not its parent.
    let ctx = &format_path(ctx, key);
    // Shorthand: "5 / 100"
    if let Some(s) = section.as_str() {
        let count: DropCount = s.parse().map_err(|e: String| field_err(ctx, key, &e))?;
        return Ok(Some(DropSpec {
            max_count: Some(count_bound(Context::Drop, "max_count", count)),
            max_consecutive: None,
        }));
    }
    // Full form: { max_count: "5 / 100", max_consecutive: 3 }
    reject_unknown_keys(section, ctx, Context::Drop)?;
    let max_count = yaml_string(section, "max_count", ctx)?
        .map(|s| {
            s.parse::<DropCount>()
                .map_err(|e| field_err(ctx, "max_count", &e.to_string()))
        })
        .transpose()?
        .map(|c| count_bound(Context::Drop, "max_count", c));
    let max_consecutive = yaml_u32(section, "max_consecutive", ctx)?
        .map(|n| spelled(Context::Drop, "max_consecutive", Quantity::Count(n)));

    Ok(Some(DropSpec {
        max_count,
        max_consecutive,
    }))
}

// ── QoS ──

/// Returns `Err` rather than `None` on a malformed duration: phase 59's whole
/// argument is that a wrong unit must not pass quietly, and swallowing the
/// error here to keep an `Option` return would do exactly that.
fn parse_qos(doc: &Yaml, ctx: &str) -> Result<Option<QosDecl>, ParseError> {
    let section = &doc["qos"];
    if section.is_badvalue() {
        return Ok(None);
    }
    // Nested block: diagnostics name the block, not its parent.
    let ctx = &format_path(ctx, "qos");
    reject_unknown_keys(section, ctx, Context::Qos)?;
    Ok(Some(QosDecl {
        reliability: yaml_string(section, "reliability", ctx)?,
        durability: yaml_string(section, "durability", ctx)?,
        depth: yaml_u32(section, "depth", ctx)?,
        history: yaml_string(section, "history", ctx)?,
        lifespan: yaml_duration(section, "lifespan")?,
        liveliness: yaml_string(section, "liveliness", ctx)?,
        deadline: yaml_duration(section, "deadline")?,
        lease_duration: yaml_duration(section, "lease_duration")?,
    }))
}

// ── YAML helpers ──

fn yaml_str_owned(y: &Yaml) -> String {
    match y {
        Yaml::String(s) => s.clone(),
        Yaml::Integer(i) => i.to_string(),
        Yaml::Real(s) => s.clone(),
        Yaml::Boolean(b) => b.to_string(),
        _ => String::new(),
    }
}

/// What a YAML node IS, for a message that says what was found.
fn yaml_kind(y: &Yaml) -> &'static str {
    match y {
        Yaml::String(_) => "a quoted string",
        Yaml::Integer(_) => "an integer",
        Yaml::Real(_) => "a float",
        Yaml::Boolean(_) => "a boolean",
        Yaml::Array(_) => "a list",
        Yaml::Hash(_) => "a mapping",
        Yaml::Null => "null",
        Yaml::Alias(_) => "an alias",
        Yaml::BadValue => "nothing",
    }
}

/// A value of the wrong TYPE is an error, not `None` (phase 70).
///
/// Every one of these helpers used to answer "not a string" / "not a number"
/// with `None`, so `max_count: 5` (an integer where a `"N / W"` string is
/// expected), `rate_hz: "100"` (a quoted number), `lifecycle: "true"` and a
/// bare-scalar `output:` all parsed as ABSENT — deleting a declaration in
/// silence, which is exactly what phase 69's unknown-key check exists to
/// prevent one level up. A wrong type is the same defect with a different
/// spelling.
fn type_err(ctx: &str, key: &str, expected: &str, found: &Yaml) -> ParseError {
    field_err(
        ctx,
        key,
        &format!(
            "expected {expected}, got {}. A value of the wrong type used to be read as \
             absent, which deleted the declaration in silence",
            yaml_kind(found)
        ),
    )
}

fn yaml_string(doc: &Yaml, key: &str, ctx: &str) -> Result<Option<String>, ParseError> {
    match &doc[key] {
        Yaml::BadValue | Yaml::Null => Ok(None),
        Yaml::String(s) => Ok(Some(s.clone())),
        other => Err(type_err(ctx, key, "a string", other)),
    }
}
fn yaml_duration(doc: &Yaml, key: &str) -> Result<Option<Duration>, ParseError> {
    use crate::duration::{LegacyUnit, from_legacy_scalar};
    let no_unit = |shown: String| {
        field_err(
            "",
            key,
            &format!(
                "`{key}: {shown}` has no unit — write `{key}: {shown}ms` (or ns/us/s). \
                 The unit is required so a value cannot be 1000x wrong and still parse"
            ),
        )
    };
    match &doc[key] {
        Yaml::String(text) => from_legacy_scalar(Some(text), None, LegacyUnit::Millis)
            .map_err(|e| field_err("", key, &e.to_string())),
        Yaml::Integer(i) => Err(no_unit(i.to_string())),
        Yaml::Real(r) => Err(no_unit(r.clone())),
        _ => Ok(None),
    }
}

/// `miss:` — deadline-miss tolerance and handling (phase 67).
///
/// Mirrors `parse_drop_spec`'s author-facing shape, including the "N / W"
/// shorthand, because it is the same arithmetic on a different event. The
/// types stay distinct: a dropped message and a late message are different
/// failures.
fn parse_miss_spec(doc: &Yaml, ctx: &str) -> Result<Option<MissSpec>, ParseError> {
    let section = &doc["miss"];
    if section.is_badvalue() {
        return Ok(None);
    }
    // Nested block: diagnostics name the block, not its parent.
    let ctx = &format_path(ctx, "miss");
    // Shorthand: `miss: "2 / 100"` — tolerance only, default action.
    if let Some(text) = section.as_str() {
        let count: DropCount = text
            .parse()
            .map_err(|e: String| field_err(ctx, "miss", &e))?;
        return Ok(Some(MissSpec {
            tolerate: Some(count_bound(Context::Miss, "tolerate", count)),
            consecutive: None,
            action: None,
        }));
    }
    reject_unknown_keys(section, ctx, Context::Miss)?;
    let tolerate = yaml_string(section, "tolerate", ctx)?
        .map(|s| {
            s.parse::<DropCount>()
                .map_err(|e| field_err(ctx, "tolerate", &e.to_string()))
        })
        .transpose()?
        .map(|c| count_bound(Context::Miss, "tolerate", c));
    let action = match yaml_string(section, "action", ctx)?.as_deref() {
        None => None,
        Some("continue") => Some(MissAction::Continue),
        Some("skip_next") => Some(MissAction::SkipNext),
        Some("abort") => Some(MissAction::Abort),
        Some(other) => {
            return Err(field_err(
                ctx,
                "action",
                &format!("unknown action '{other}' — expected `continue`, `skip_next` or `abort`"),
            ));
        }
    };
    Ok(Some(MissSpec {
        tolerate,
        consecutive: yaml_u32(section, "consecutive", ctx)?
            .map(|n| spelled(Context::Miss, "consecutive", Quantity::Count(n))),
        action,
    }))
}

/// `concurrency:` — which of a node's paths may not run concurrently.
///
/// An ABSENT section and an empty `exclusive:` list mean opposite things, so
/// the distinction is preserved rather than normalised away: absent means every
/// path serializes (the conservative default `rclcpp` and embedded executors
/// already take), while `exclusive: []` says they may all run concurrently.
fn parse_concurrency(doc: &Yaml, ctx: &str) -> Result<Option<ConcurrencyDecl>, ParseError> {
    let section = &doc["concurrency"];
    if section.is_badvalue() {
        return Ok(None);
    }
    reject_unknown_keys(section, ctx, Context::Concurrency)?;
    let mut exclusive: Vec<Vec<String>> = Vec::new();
    if let Yaml::Array(groups) = &section["exclusive"] {
        for (i, group) in groups.iter().enumerate() {
            match group {
                Yaml::Array(names) => exclusive.push(names.iter().map(yaml_str_owned).collect()),
                _ => {
                    return Err(field_err(
                        ctx,
                        &format!("concurrency.exclusive[{i}]"),
                        "expected a list of path names, e.g. `- [to_boxes, to_masks]`",
                    ));
                }
            }
        }
    }
    Ok(Some(ConcurrencyDecl { exclusive }))
}

fn yaml_f64(doc: &Yaml, key: &str, ctx: &str) -> Result<Option<f64>, ParseError> {
    match &doc[key] {
        Yaml::BadValue | Yaml::Null => Ok(None),
        Yaml::Real(r) => r
            .parse()
            .map(Some)
            .map_err(|_| type_err(ctx, key, "a number", &doc[key])),
        Yaml::Integer(i) => Ok(Some(*i as f64)),
        Yaml::String(_) => Err(field_err(
            ctx,
            key,
            "expected a number, got a quoted string — remove the quotes. A quoted \
             number used to be read as absent, which deleted the declaration in silence",
        )),
        other => Err(type_err(ctx, key, "a number", other)),
    }
}
fn yaml_u32(doc: &Yaml, key: &str, ctx: &str) -> Result<Option<u32>, ParseError> {
    match &doc[key] {
        Yaml::BadValue | Yaml::Null => Ok(None),
        Yaml::Integer(i) if *i >= 0 && *i <= u32::MAX as i64 => Ok(Some(*i as u32)),
        Yaml::Integer(_) => Err(field_err(ctx, key, "expected a non-negative integer")),
        other => Err(type_err(ctx, key, "a non-negative integer", other)),
    }
}
fn yaml_bool(doc: &Yaml, key: &str, ctx: &str) -> Result<Option<bool>, ParseError> {
    match &doc[key] {
        Yaml::BadValue | Yaml::Null => Ok(None),
        Yaml::Boolean(b) => Ok(Some(*b)),
        Yaml::String(_) => Err(field_err(
            ctx,
            key,
            "expected a boolean, got a quoted string — write `true` or `false` without quotes",
        )),
        other => Err(type_err(ctx, key, "a boolean", other)),
    }
}
/// Every element must be a scalar; `yaml_str_owned` used to turn a nested
/// mapping or list into `""` and carry on.
fn yaml_scalar_list(arr: &[Yaml], key: &str, ctx: &str) -> Result<Vec<String>, ParseError> {
    arr.iter()
        .enumerate()
        .map(|(i, y)| match y {
            Yaml::String(_) | Yaml::Integer(_) | Yaml::Real(_) | Yaml::Boolean(_) => {
                Ok(yaml_str_owned(y))
            }
            other => Err(type_err(ctx, &format!("{key}[{i}]"), "a scalar", other)),
        })
        .collect()
}
fn yaml_string_list(doc: &Yaml, key: &str, ctx: &str) -> Result<Vec<String>, ParseError> {
    match &doc[key] {
        Yaml::BadValue | Yaml::Null => Ok(vec![]),
        Yaml::Array(arr) => yaml_scalar_list(arr, key, ctx),
        Yaml::String(s) => Err(field_err(
            ctx,
            key,
            &format!(
                "expected a list, got a bare scalar — write `{key}: [{s}]`. A scalar here \
                 used to be read as an empty list, which deleted the declaration in silence"
            ),
        )),
        other => Err(type_err(ctx, key, "a list", other)),
    }
}
/// `input:` and `trigger.input` accept a bare scalar as a one-element list.
fn parse_string_or_list(doc: &Yaml, key: &str, ctx: &str) -> Result<Vec<String>, ParseError> {
    yaml_direct_string_list(&doc[key], key, ctx)
}
fn yaml_direct_string_list(y: &Yaml, key: &str, ctx: &str) -> Result<Vec<String>, ParseError> {
    match y {
        Yaml::BadValue | Yaml::Null => Ok(vec![]),
        Yaml::String(s) => Ok(vec![s.clone()]),
        Yaml::Array(arr) => yaml_scalar_list(arr, key, ctx),
        other => Err(type_err(ctx, key, "a list or a single name", other)),
    }
}

fn field_err(ctx: &str, field: &str, message: &str) -> ParseError {
    ParseError::Field {
        path: format_path(ctx, field),
        message: message.to_string(),
    }
}

fn format_path(ctx: &str, field: &str) -> String {
    if ctx.is_empty() {
        field.to_string()
    } else {
        format!("{ctx}.{field}")
    }
}

#[cfg(test)]
mod tests {
    use crate::bound::OptBound;

    /// Phase 69: the three typos that motivated the field table. Each one
    /// used to parse cleanly and silently discard what it was meant to
    /// declare.
    #[test]
    fn a_typo_is_an_error_and_names_the_key_it_meant() {
        let cases = [
            (
                "nodes:\n  n:\n    paths:\n      p:\n        max_latencyy: 5ms\n",
                "max_latency",
            ),
            (
                "topics:\n  /t:\n    type: std_msgs/msg/String\n    rate_hzz: 100\n",
                "rate_hz",
            ),
            ("nodes:\n  n:\n    lifecycl: true\n", "lifecycle"),
        ];
        for (yaml, expected) in cases {
            let err = super::parse_manifest_str(yaml)
                .expect_err("a typo must not parse")
                .to_string();
            assert!(
                err.contains("unknown key") && err.contains(expected),
                "expected a suggestion of `{expected}`, got: {err}"
            );
        }
    }

    /// A key with no near neighbour gets the accepted list instead of a
    /// confident wrong guess.
    #[test]
    fn an_unrelated_key_is_rejected_without_a_misleading_suggestion() {
        let err = super::parse_manifest_str("nodes:\n  n:\n    bogus_field: 1\n")
            .expect_err("an unknown key must not parse")
            .to_string();
        assert!(err.contains("accepted here"), "got: {err}");
        assert!(!err.contains("did you mean"), "got: {err}");
    }

    /// `segments:` was named in the `chains:` migration message but had no
    /// check of its own, so a stray one stayed silent after phase 68 removed
    /// it. The table is that backstop.
    #[test]
    fn a_removed_key_with_no_dedicated_check_is_still_rejected() {
        let err =
            super::parse_manifest_str("paths:\n  p:\n    output: [/b]\n    segments: [a, b]\n")
                .expect_err("`segments:` must not parse")
                .to_string();
        assert!(err.contains("Removed in phase 68"), "got: {err}");
    }

    /// The dedicated migration messages must keep winning over the generic
    /// one — they name the replacement, which the generic message cannot.
    #[test]
    fn a_removed_key_with_a_dedicated_check_keeps_its_own_message() {
        let err = super::parse_manifest_str("chains:\n  c:\n    segments: []\n")
            .expect_err("`chains:` must not parse")
            .to_string();
        assert!(
            err.contains("State the same requirement as a scope path"),
            "got: {err}"
        );

        let err =
            super::parse_manifest_str("nodes:\n  n:\n    pub:\n      out:\n        jitter_ms: 5\n")
                .expect_err("`jitter_ms:` must not parse")
                .to_string();
        assert!(err.contains("property of a ROUTE"), "got: {err}");
    }

    /// A value of the wrong TYPE is an error (phase 70), not an absent field.
    /// Each of these used to parse clean with the declaration deleted.
    #[test]
    fn a_wrong_type_is_rejected_not_read_as_absent() {
        let cases: &[(&str, &str)] = &[
            // an integer where a "N / W" string is expected
            (
                "topics:\n  t:\n    type: T\n    drop: { max_count: 5 }\n",
                "expected a string, got an integer",
            ),
            // a quoted number
            (
                "topics:\n  t:\n    type: T\n    rate_hz: \"100\"\n",
                "remove the quotes",
            ),
            // a quoted boolean
            ("nodes:\n  n:\n    lifecycle: \"true\"\n", "without quotes"),
            // a bare scalar where a list is expected
            (
                "nodes:\n  n:\n    paths:\n      p:\n        output: o\n",
                "write `output: [o]`",
            ),
            // a mapping where a string is expected
            (
                "nodes:\n  n:\n    criticality: { level: high }\n",
                "got a mapping",
            ),
            // a negative depth
            (
                "topics:\n  t:\n    type: T\n    qos: { depth: -1 }\n",
                "non-negative",
            ),
        ];
        for (yaml, needle) in cases {
            let err = super::parse_manifest_str(yaml)
                .expect_err(&format!("must not parse:\n{yaml}"))
                .to_string();
            assert!(err.contains(needle), "for:\n{yaml}\ngot: {err}");
        }
    }

    /// The error names WHERE, not just what.
    #[test]
    fn a_type_error_carries_its_path() {
        let err = super::parse_manifest_str(
            "paths:\n  clean:\n    output: [o]\n    drop: { max_count: 5 }\n",
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("paths.clean.drop.max_count"), "got: {err}");
    }

    /// The one bare scalar that is still accepted: `input:` as a single name.
    #[test]
    fn input_still_accepts_a_single_name() {
        let m = super::parse_manifest_str(
            "nodes:\n  n:\n    sub:\n      a: {}\n    pub:\n      o: {}\n    paths:\n      p:\n        input: a\n        output: [o]\n",
        )
        .unwrap();
        assert_eq!(m.nodes["n"].paths["p"].input, vec!["a"]);
    }

    /// Phase 75: functions, modes and per-mode overrides parse, and a
    /// hazard may guard a function by name.
    #[test]
    fn functions_modes_and_overrides_parse() {
        let yaml = r#"
version: 1
functions:
  pose_estimation: { all_of: [/loc/ndt, /loc/gnss] }
  trajectory: [/planning/trajectory]
  scan: /sensing/scan
modes:
  autonomous:
    description: full autonomy
    requires: [pose_estimation, trajectory]
    fallback: [comfortable_stop, emergency_stop]
  comfortable_stop:
    requires: [pose_estimation]
    reaction: system.comfortable_stop
    overrides:
      paths:
        lidar_to_brake: { max_latency: 100ms }
      nodes:
        detector:
          sub: { scan: { min_rate_hz: 10 } }
  emergency_stop:
    requires: []
    reaction: system.emergency_stop
hazards:
  lost_pose:
    guards: [pose_estimation]
    ftti: 2s
    reaction: autonomous
"#;
        let m = parse_manifest_str(yaml).unwrap();
        assert!(m.functions["pose_estimation"].group.all_of);
        assert_eq!(m.functions["pose_estimation"].group.members.len(), 2);
        assert!(!m.functions["trajectory"].group.all_of);
        assert_eq!(m.functions["scan"].group.members, vec!["/sensing/scan"]);
        let auto = &m.modes["autonomous"];
        assert_eq!(auto.requires, vec!["pose_estimation", "trajectory"]);
        assert_eq!(auto.fallback, vec!["comfortable_stop", "emergency_stop"]);
        let cs = &m.modes["comfortable_stop"];
        assert_eq!(cs.reaction.as_deref(), Some("system.comfortable_stop"));
        let targets: Vec<&str> = cs.overrides.iter().map(|o| o.target.as_str()).collect();
        assert!(
            targets.contains(&"paths.lidar_to_brake.max_latency"),
            "{targets:?}"
        );
        assert!(
            targets.contains(&"nodes.detector.sub.scan.min_rate_hz"),
            "{targets:?}"
        );
        assert_eq!(
            cs.overrides
                .iter()
                .find(|o| o.target.ends_with("max_latency"))
                .unwrap()
                .value,
            "100ms"
        );
        // A hazard guards the function by NAME; the loader resolves it.
        assert_eq!(
            m.hazards["lost_pose"].guards[0].members,
            vec!["pose_estimation"]
        );
    }

    /// A file states the grammar it needs, and a
    /// newer one is refused before the body is read, naming both releases.
    #[test]
    fn a_contract_states_the_grammar_it_needs() {
        let ours = crate::GRAMMAR_VERSION;
        for ok in [
            format!("v{ours}"),
            ours.to_string(),
            format!(">= v{ours}"),
            "v0.1.0".into(),
        ] {
            let m = super::parse_manifest_str(&format!("version: 1\nrlm: \"{ok}\"\n"))
                .unwrap_or_else(|e| panic!("{ok}: {e}"));
            assert_eq!(m.rlm.as_deref(), Some(ok.as_str()));
        }
        // Newer than this grammar: refused even though the body also carries
        // a key this grammar does not know -- the header is read first.
        let err = super::parse_manifest_str(
            "version: 1\nrlm: v99.0.0\nhazards:\n  h:\n    not_a_key_yet: 1\n",
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains(&format!(
                "this contract needs rlm >= v99.0.0; this checker reads rlm v{ours}"
            )),
            "{err}"
        );
        assert!(!err.contains("not_a_key_yet"), "{err}");
        for (bad, needle) in [
            ("rlm: latest\n", "is not a grammar release"),
            ("rlm: 1\n", "as a string"),
        ] {
            let err = super::parse_manifest_str(bad).unwrap_err().to_string();
            assert!(err.contains(needle), "{bad}: {err}");
        }
    }

    /// `GRAMMAR_VERSION` is the release this crate is: equal to the newest
    /// released heading of `CHANGELOG.md`, or its successor while an
    /// `## Unreleased` section is on top.
    #[test]
    fn the_grammar_version_is_the_release() {
        let log = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../CHANGELOG.md"),
        )
        .expect("CHANGELOG.md");
        let headings: Vec<&str> = log.lines().filter(|l| l.starts_with("## ")).collect();
        let released = headings
            .iter()
            .find_map(|h| h.strip_prefix("## v"))
            .and_then(|h| h.split_whitespace().next())
            .and_then(super::parse_grammar_version)
            .expect("a released heading");
        let ours = super::parse_grammar_version(crate::GRAMMAR_VERSION).unwrap();
        if headings.first() == Some(&"## Unreleased") {
            assert!(ours > released, "{ours:?} must be after {released:?}");
        } else {
            assert_eq!(ours, released, "GRAMMAR_VERSION must be the newest release");
        }
    }

    /// `on_demand: true` is a publisher's
    /// statement that it promises no rate; beside a `min_rate_hz`, or on a
    /// subscriber, it is refused.
    #[test]
    fn on_demand_is_a_publishers_statement() {
        let m = super::parse_manifest_str(
            "nodes:\n  op:\n    pub:\n      limit: { on_demand: true }\n",
        )
        .expect("parses");
        assert_eq!(m.nodes["op"].publishers["limit"].on_demand, Some(true));
        for (yaml, needle) in [
            (
                "nodes:\n  op:\n    pub:\n      limit: { on_demand: true, min_rate_hz: 10 }\n",
                "state one or the other",
            ),
            (
                "nodes:\n  op:\n    sub:\n      limit: { on_demand: true }\n",
                "is a publisher's statement",
            ),
            (
                "nodes:\n  op:\n    pub:\n      limit: { on_demand: yes_please }\n",
                "on_demand",
            ),
        ] {
            let err = super::parse_manifest_str(yaml)
                .expect_err(&format!("must not parse:\n{yaml}"))
                .to_string();
            assert!(err.contains(needle), "`{needle}` not in: {err}");
        }
    }

    #[test]
    fn mode_keys_are_closed_and_overrides_reject_a_list() {
        for (yaml, needle) in [
            (
                "modes:\n  m:\n    requiress: [a]\n",
                "did you mean `requires`?",
            ),
            (
                "modes:\n  m:\n    overrides: { paths: [a] }\n",
                "a scalar value or a nested mapping",
            ),
            ("functions:\n  f: { all_of: [/only] }\n", "at least two"),
            ("modes: [a, b]\n", "expected a mapping"),
        ] {
            let err = super::parse_manifest_str(yaml)
                .expect_err(&format!("must not parse:\n{yaml}"))
                .to_string();
            assert!(err.contains(needle), "for:\n{yaml}\ngot: {err}");
        }
    }

    /// Phase 71: the fault-reaction vocabulary parses, and every closed set
    /// refuses an unknown member.
    #[test]
    fn hazards_on_violation_and_safe_state_parse() {
        let yaml = r#"
version: 1
hazards:
  drive_blind:
    severity: ASIL_D
    description: vehicle continues with no obstacle data
    guards:
      - /safety/scan
      - pose
    on: omission
    ftti: 500ms
    reaction: safety.stop
nodes:
  brake:
    sub:
      obstacles:
        max_age: 60ms
        on_violation:
          on: [late, omission]
          reaction: emergency_stop
          within: 20ms
          mechanism: diagnostics
    pub:
      brake_cmd: {}
    paths:
      emergency_stop:
        trigger: { input: [obstacles] }
        output: [brake_cmd]
        max_latency: 5ms
        safe_state: { emits: brake_cmd, settle: 200ms }
"#;
        let m = parse_manifest_str(yaml).unwrap();
        let h = &m.hazards["drive_blind"];
        assert_eq!(h.guards.len(), 2);
        assert!(!h.guards[0].all_of && h.guards[0].members == vec!["/safety/scan"]);
        // A redundant set is a function, guarded by name (phase 90 G7).
        assert!(!h.guards[1].all_of && h.guards[1].members == vec!["pose"]);
        assert_eq!(h.on, vec![FaultKind::Omission]);
        assert_eq!(h.ftti.ms().unwrap(), 500.0);
        assert_eq!(h.reaction.as_deref(), Some("safety.stop"));
        let ov = m.nodes["brake"].subscribers["obstacles"]
            .on_violation
            .as_ref()
            .unwrap();
        assert_eq!(ov.on, vec![FaultKind::Late, FaultKind::Omission]);
        assert_eq!(ov.reaction, "emergency_stop");
        assert_eq!(ov.within.ms().unwrap(), 20.0);
        assert_eq!(ov.mechanism, DetectMechanism::Diagnostics);
        let ss = m.nodes["brake"].paths["emergency_stop"]
            .safe_state
            .as_ref()
            .unwrap();
        assert_eq!(ss.emits, "brake_cmd");
        assert_eq!(ss.settle.ms().unwrap(), 200.0);
    }

    /// Phase 82: `hazards.<h>.on` is a set. A scalar is the set of one, a
    /// list is the set as written (duplicates dropped), and an omitted key
    /// is the empty set -- which consumers read as every class.
    #[test]
    fn hazard_on_is_a_set_of_fault_classes() {
        let on = |yaml: &str| {
            super::parse_manifest_str(yaml).unwrap().hazards["h"]
                .on
                .clone()
        };
        assert_eq!(
            on("hazards:\n  h:\n    on: omission\n"),
            vec![FaultKind::Omission]
        );
        assert_eq!(
            on("hazards:\n  h:\n    on: [omission, late]\n"),
            vec![FaultKind::Omission, FaultKind::Late]
        );
        assert_eq!(
            on("hazards:\n  h:\n    on: [late, late, reported]\n"),
            vec![FaultKind::Late, FaultKind::Reported]
        );
        assert!(on("hazards:\n  h:\n    ftti: 5ms\n").is_empty());
    }

    #[test]
    fn fault_reaction_closed_sets_refuse_unknown_members() {
        let cases: &[(&str, &str)] = &[
            ("hazards:\n  h:\n    on: crash\n", "not a fault class"),
            (
                "hazards:\n  h:\n    on: [omission, crash]\n",
                "not a fault class",
            ),
            (
                "hazards:\n  h:\n    on: []\n",
                "state at least one fault class or omit the key",
            ),
            (
                "hazards:\n  h:\n    on: { late: 1 }\n",
                "a list or a single name",
            ),
            (
                "nodes:\n  n:\n    sub:\n      a:\n        on_violation: { on: [value], reaction: r }\n",
                "not a fault class",
            ),
            (
                "nodes:\n  n:\n    sub:\n      a:\n        on_violation: { on: [late], reaction: r, mechanism: magic }\n",
                "not a detection mechanism",
            ),
            (
                "nodes:\n  n:\n    sub:\n      a:\n        on_violation: { on: [late] }\n",
                "required",
            ),
            (
                "hazards:\n  h:\n    guards: [{ all_of: [/a, /b] }]\n",
                "Removed in v0.1.59",
            ),
            (
                "hazards:\n  h:\n    guards: [{ any: [/a] }]\n",
                "unknown key",
            ),
            ("hazards:\n  h:\n    fttii: 5ms\n", "did you mean `ftti`"),
            (
                "nodes:\n  n:\n    paths:\n      p:\n        output: [o]\n        safe_state: { settle: 1ms }\n",
                "required",
            ),
        ];
        for (yaml, needle) in cases {
            let err = super::parse_manifest_str(yaml)
                .expect_err(&format!("must not parse:\n{yaml}"))
                .to_string();
            assert!(err.contains(needle), "for:\n{yaml}\ngot: {err}");
        }
    }

    /// `correlation:` (phase 70) keeps a dedicated message too: the generic
    /// check would say "removed", and only this one names `sync:`.
    #[test]
    fn a_removed_correlation_names_sync_as_the_replacement() {
        let err = super::parse_manifest_str(
            "nodes:\n  n:\n    paths:\n      p:\n        output: [o]\n        correlation: timestamp\n",
        )
        .expect_err("`correlation:` must not parse")
        .to_string();
        assert!(err.contains("sync:"), "got: {err}");
        let err = super::parse_manifest_str("exclude_patterns: [/rosout]\n")
            .expect_err("`exclude_patterns:` must not parse")
            .to_string();
        assert!(err.contains("Removed in phase 70"), "got: {err}");
    }

    /// An author-chosen key must never be measured against the schema. A
    /// node named `max_latency` is legal, and so is a topic named after
    /// anything at all.
    #[test]
    fn author_chosen_keys_are_not_measured_against_the_schema() {
        let m = super::parse_manifest_str(
            "nodes:\n  max_latency:\n    pub: [out]\ntopics:\n  bogus_field:\n    type: std_msgs/msg/String\n",
        )
        .expect("author-chosen names are not schema keys");
        assert!(m.nodes.contains_key("max_latency"));
        assert!(m.topics.contains_key("bogus_field"));
    }

    /// A duration needs a unit, absent is `None`, and the retired `_ms`
    /// spelling (phase 70) is a parse error that names the replacement —
    /// not an alias, not an info-level nudge.
    #[test]
    fn duration_refuses_bare_numbers_and_the_retired_suffix() {
        use yaml_rust2::YamlLoader;
        let load = |t: &str| YamlLoader::load_from_str(t).unwrap().remove(0);

        let v = yaml_duration(&load("max_latency: 12ms"), "max_latency")
            .unwrap()
            .unwrap();
        assert_eq!(v.as_millis_f64(), 12.0);

        // A bare number is the 1000x error.
        let err = yaml_duration(&load("max_latency: 12"), "max_latency")
            .unwrap_err()
            .to_string();
        assert!(err.contains("has no unit"), "{err}");
        assert!(err.contains("max_latency: 12ms"), "{err}");

        // Absent is None, not an error.
        assert!(
            yaml_duration(&load("other: 1"), "max_latency")
                .unwrap()
                .is_none()
        );

        // The old name is gone, and the error says what to write instead.
        let err = super::parse_manifest_str(
            "nodes:\n  n:\n    paths:\n      p:\n        output: [o]\n        max_latency_ms: 5\n",
        )
        .expect_err("`max_latency_ms:` must not parse")
        .to_string();
        assert!(err.contains("Removed in phase 70"), "got: {err}");
        assert!(err.contains("max_latency: <n>ms"), "got: {err}");
    }

    /// `criticality` is a closed set (phase 70). `urgent` used to schedule a
    /// node exactly as if nothing had been declared.
    #[test]
    fn criticality_is_a_closed_set() {
        let err = super::parse_manifest_str("nodes:\n  n:\n    criticality: urgent\n")
            .expect_err("must not parse")
            .to_string();
        assert!(err.contains("accepted: high, medium, low"), "got: {err}");
        for ok in ["high", "medium", "low"] {
            super::parse_manifest_str(&format!("nodes:\n  n:\n    criticality: {ok}\n")).unwrap();
        }
    }

    use super::*;

    #[test]
    fn test_minimal_manifest() {
        let yaml = r#"
version: 1
nodes:
  talker:
    pub: [chatter]
  listener:
    sub: [chatter]
topics:
  chatter:
    type: std_msgs/msg/String
    pub: [talker/chatter]
    sub: [listener/chatter]
"#;
        let m = parse_manifest_str(yaml).unwrap();
        assert_eq!(m.version, 1);
        assert_eq!(m.nodes.len(), 2);
        assert_eq!(m.topics.len(), 1);

        let talker = &m.nodes["talker"];
        assert!(talker.publishers.contains_key("chatter"));
        assert!(talker.subscribers.is_empty());

        let topic = &m.topics["chatter"];
        assert_eq!(topic.msg_type, "std_msgs/msg/String");
        assert_eq!(topic.publishers, vec!["talker/chatter"]);
        assert_eq!(topic.subscribers, vec!["listener/chatter"]);
    }

    #[test]
    fn test_endpoint_properties() {
        let yaml = r#"
version: 1
nodes:
  ndt:
    sub:
      sensor_points:
        min_rate_hz: 10
      initial_pose:
        required: true
      regularization_pose:
        state: true
    pub:
      ndt_pose:
        min_rate_hz: 10
      exe_time_ms:
    srv:
      trigger_node:
        max_response: 100ms
"#;
        let m = parse_manifest_str(yaml).unwrap();
        let ndt = &m.nodes["ndt"];

        assert_eq!(
            ndt.subscribers["sensor_points"].min_rate_hz.hz(),
            Some(10.0)
        );
        assert_eq!(ndt.subscribers["initial_pose"].required, Some(true));
        assert_eq!(ndt.subscribers["regularization_pose"].state, Some(true));
        assert_eq!(ndt.publishers["ndt_pose"].min_rate_hz.hz(), Some(10.0));
        assert!(ndt.publishers.contains_key("exe_time_ms"));
        assert_eq!(ndt.srv["trigger_node"].max_response.ms(), Some(100.0));
    }

    #[test]
    fn test_lifecycle_node() {
        let yaml = r#"
version: 1
nodes:
  lidar_driver:
    lifecycle: true
    pub:
      pointcloud:
        min_rate_hz: 10
  regular_node:
    pub:
      data: {}
"#;
        let m = parse_manifest_str(yaml).unwrap();
        assert_eq!(m.nodes["lidar_driver"].lifecycle, Some(true));
        assert_eq!(m.nodes["regular_node"].lifecycle, None);
    }

    #[test]
    fn test_node_criticality() {
        let yaml = r#"
version: 1
nodes:
  control_node:
    criticality: high
  regular_node:
    pub:
      data: {}
"#;
        let m = parse_manifest_str(yaml).unwrap();
        assert_eq!(m.nodes["control_node"].criticality.as_deref(), Some("high"));
        assert_eq!(m.nodes["regular_node"].criticality, None);
    }

    #[test]
    fn test_paths() {
        let yaml = r#"
version: 1
nodes:
  ndt:
    sub: [sensor_points]
    pub: [ndt_pose, exe_time_ms]
    paths:
      localization:
        input: sensor_points
        output: [ndt_pose]
        max_latency: 50ms
      debug:
        input: sensor_points
        output: [exe_time_ms]
paths:
  localization:
    input: sensor_points
    output: [ndt_pose]
    drop:
      max_count: 10 / 100
      max_consecutive: 5
"#;
        let m = parse_manifest_str(yaml).unwrap();
        let ndt = &m.nodes["ndt"];
        assert_eq!(ndt.paths.len(), 2);

        let loc = &ndt.paths["localization"];
        assert_eq!(loc.input, vec!["sensor_points"]);
        assert_eq!(loc.output, vec!["ndt_pose"]);
        assert_eq!(loc.max_latency.ms(), Some(50.0));
        let drop = m.paths["localization"].drop.as_ref().unwrap();
        assert_eq!(drop.max_count.count_per().unwrap().n, 10);
        assert_eq!(drop.max_count.count_per().unwrap().w, 100);
        assert_eq!(drop.max_consecutive.count(), Some(5));

        let debug = &ndt.paths["debug"];
        assert_eq!(debug.input, vec!["sensor_points"]);
        assert!(debug.max_latency.is_none());
    }

    #[test]
    fn test_topic_with_contract() {
        let yaml = r#"
version: 1
topics:
  pointcloud:
    type: sensor_msgs/msg/PointCloud2
    pub: [lidar_driver/pointcloud]
    sub: [cropbox_filter/input]
    qos:
      reliability: best_effort
      depth: 1
    rate_hz: 10
    drop: 1 / 100
  debug_output: sensor_msgs/msg/PointCloud2
"#;
        let m = parse_manifest_str(yaml).unwrap();
        assert_eq!(m.topics.len(), 2);

        let pc = &m.topics["pointcloud"];
        assert_eq!(pc.msg_type, "sensor_msgs/msg/PointCloud2");
        assert_eq!(pc.rate_hz, Some(10.0));
        let qos = pc.qos.as_ref().unwrap();
        assert_eq!(qos.reliability.as_deref(), Some("best_effort"));
        assert_eq!(qos.depth, Some(1));
        let drop = pc.drop.as_ref().unwrap();
        assert_eq!(drop.max_count.count_per().unwrap().n, 1);
        assert_eq!(drop.max_count.count_per().unwrap().w, 100);

        // Shorthand form
        let debug = &m.topics["debug_output"];
        assert_eq!(debug.msg_type, "sensor_msgs/msg/PointCloud2");
        assert!(debug.publishers.is_empty());
    }

    #[test]
    fn test_includes() {
        let yaml = r#"
version: 1
includes:
  safety:
    nodes:
      emergency_stop:
        pub: [stop_cmd]
        sub: [diagnostics]
"#;
        let m = parse_manifest_str(yaml).unwrap();
        assert_eq!(m.includes.len(), 1);
        assert!(
            m.includes["safety"]
                .inline()
                .expect("expected Inline")
                .nodes
                .contains_key("emergency_stop")
        );
    }

    /// Phase 90 G7: the file-reference include is removed, and its key
    /// reaches the removal message, which names the inline form.
    #[test]
    fn test_include_file_reference_is_removed() {
        for yaml in [
            "version: 1\nincludes:\n  lidar:\n    manifest: lidar.yaml\n",
            "version: 1\nincludes:\n  lidar:\n    manifest: lidar.yaml\n    if: use_lidar\n",
        ] {
            let e = parse_manifest_str(yaml).unwrap_err().to_string();
            assert!(e.contains("includes.lidar.manifest"), "{e}");
            assert!(
                e.contains("Removed in v0.1.59") && e.contains("inline"),
                "{e}"
            );
        }
    }

    #[test]
    fn test_includes_carry_conditions() {
        // Issue #0043: the inline form IS a nested manifest, so the
        // condition is at its root.
        let yaml = r#"
version: 1
includes:
  lidar:
    if: use_lidar
    nodes:
      cloud:
        pub: [points]
  safety:
    unless: "false"
    nodes:
      emergency_stop:
        pub: [stop_cmd]
"#;
        let m = parse_manifest_str(yaml).unwrap();
        assert_eq!(m.includes.len(), 2);

        let lidar = &m.includes["lidar"];
        assert_eq!(lidar.if_condition.as_deref(), Some("use_lidar"));
        assert_eq!(lidar.unless_condition, None);
        assert!(lidar.inline().is_some());

        let safety = &m.includes["safety"];
        assert_eq!(safety.if_condition, None);
        assert_eq!(safety.unless_condition.as_deref(), Some("false"));
        assert!(
            safety
                .inline()
                .expect("expected Inline")
                .nodes
                .contains_key("emergency_stop")
        );
    }

    #[test]
    fn test_root_condition_on_standalone_manifest_refused() {
        // A condition at the root of a file nobody includes selects nothing.
        for key in ["if", "unless"] {
            let yaml = format!("version: 1\n{key}: use_lidar\nnodes:\n  a:\n    pub: [out]\n");
            let err = parse_manifest_str(&yaml).unwrap_err().to_string();
            assert!(
                err.contains("belongs to the `includes:` entry"),
                "{key}: unexpected message: {err}"
            );
        }
    }

    #[test]
    fn test_scope_paths() {
        let yaml = r#"
version: 1
paths:
  main:
    input: raw_data
    output: [detections]
    max_latency: 50ms
"#;
        let m = parse_manifest_str(yaml).unwrap();

        let main = &m.paths["main"];
        assert_eq!(main.input, vec!["raw_data"]);
        assert_eq!(main.output, vec!["detections"]);
        assert_eq!(main.max_latency.ms(), Some(50.0));
    }

    #[test]
    fn test_multi_input_path() {
        let yaml = r#"
version: 1
nodes:
  fusion:
    sub: [lidar_objects, camera_objects]
    pub: [fused]
    paths:
      fusion:
        input: [lidar_objects, camera_objects]
        output: [fused]
        tolerance: 50ms
        max_latency: 20ms
"#;
        let m = parse_manifest_str(yaml).unwrap();
        let path = &m.nodes["fusion"].paths["fusion"];
        assert_eq!(path.input, vec!["lidar_objects", "camera_objects"]);
        assert_eq!(path.tolerance.map(|d| d.as_millis_f64()), Some(50.0));
    }

    #[test]
    fn test_endpoint_uniqueness_violation() {
        let yaml = r#"
version: 1
nodes:
  relay:
    pub: [data]
    sub: [data]
"#;
        let result = parse_manifest_str(yaml);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("duplicate endpoint name"), "got: {err}");
    }

    #[test]
    fn test_services_and_actions() {
        let yaml = r#"
version: 1
services:
  configure:
    type: std_srvs/srv/SetBool
    server: [driver/configure]
    client: [controller/configure]
actions:
  navigate:
    type: nav2_msgs/action/NavigateToPose
    server: [navigator/navigate]
    client: [planner/navigate]
"#;
        let m = parse_manifest_str(yaml).unwrap();
        let svc = &m.services["configure"];
        assert_eq!(svc.srv_type, "std_srvs/srv/SetBool");
        assert_eq!(svc.server, vec!["driver/configure"]);

        let act = &m.actions["navigate"];
        assert_eq!(act.action_type, "nav2_msgs/action/NavigateToPose");
        assert_eq!(act.client, vec!["planner/navigate"]);
    }

    #[test]
    fn test_empty_manifest() {
        let m = parse_manifest_str("").unwrap();
        assert_eq!(m.version, 1);
        assert!(m.nodes.is_empty());
    }

    #[test]
    fn test_endpoint_qos_override() {
        let yaml = r#"
version: 1
nodes:
  lidar:
    pub:
      pointcloud:
        qos:
          reliability: best_effort
          depth: 5
  perception:
    sub:
      pointcloud:
        qos: { reliability: reliable }
        max_transport: 0ms
  remote_viz:
    sub:
      pointcloud:
        max_transport: 10ms
"#;
        let m = parse_manifest_str(yaml).unwrap();
        let lidar_pub = &m.nodes["lidar"].publishers["pointcloud"];
        let lidar_qos = lidar_pub.qos.as_ref().unwrap();
        assert_eq!(lidar_qos.reliability.as_deref(), Some("best_effort"));
        assert_eq!(lidar_qos.depth, Some(5));
        assert_eq!(lidar_pub.max_transport, None);

        let perception_sub = &m.nodes["perception"].subscribers["pointcloud"];
        let perception_qos = perception_sub.qos.as_ref().unwrap();
        assert_eq!(perception_qos.reliability.as_deref(), Some("reliable"));
        assert_eq!(perception_sub.max_transport.ms(), Some(0.0));

        let remote_sub = &m.nodes["remote_viz"].subscribers["pointcloud"];
        assert!(remote_sub.qos.is_none());
        assert_eq!(remote_sub.max_transport.ms(), Some(10.0));
    }

    #[test]
    fn test_endpoint_effective_qos_overlay() {
        use crate::types::QosDecl;
        let topic = QosDecl {
            reliability: Some("reliable".into()),
            durability: Some("transient_local".into()),
            depth: Some(10),
            ..Default::default()
        };
        let endpoint = QosDecl {
            reliability: Some("best_effort".into()),
            ..Default::default()
        };
        let eff = QosDecl::effective(Some(&topic), Some(&endpoint));
        // Endpoint override wins for reliability.
        assert_eq!(eff.reliability.as_deref(), Some("best_effort"));
        // Topic-level fields are inherited.
        assert_eq!(eff.durability.as_deref(), Some("transient_local"));
        assert_eq!(eff.depth, Some(10));

        // Empty endpoint qos = full inherit.
        let eff = QosDecl::effective(Some(&topic), Some(&QosDecl::default()));
        assert_eq!(eff.reliability.as_deref(), Some("reliable"));

        // No topic-level: endpoint stands alone.
        let eff = QosDecl::effective(None, Some(&endpoint));
        assert_eq!(eff.reliability.as_deref(), Some("best_effort"));
        assert_eq!(eff.durability, None);

        // Neither side: all None.
        let eff = QosDecl::effective(None, None);
        assert_eq!(eff.reliability, None);
        assert_eq!(eff.durability, None);
    }

    #[test]
    fn test_external_topics_block() {
        use crate::types::ExternalSide;
        let yaml = r#"
version: 1
external_topics:
  /tf:
    external: pub
    type: tf2_msgs/msg/TFMessage
  /vehicle/engage:
    external: pub
  /debug/marker:
    external: sub
  /passthrough/relay:
    external: both
    type: std_msgs/msg/String
    qos: { reliability: best_effort }
"#;
        let m = parse_manifest_str(yaml).unwrap();
        assert_eq!(m.external_topics.len(), 4);

        let tf = &m.external_topics["/tf"];
        assert_eq!(tf.side, ExternalSide::Pub);
        assert_eq!(tf.msg_type.as_deref(), Some("tf2_msgs/msg/TFMessage"));

        let engage = &m.external_topics["/vehicle/engage"];
        assert_eq!(engage.side, ExternalSide::Pub);
        assert!(engage.msg_type.is_none());

        let marker = &m.external_topics["/debug/marker"];
        assert_eq!(marker.side, ExternalSide::Sub);

        let passthrough = &m.external_topics["/passthrough/relay"];
        assert_eq!(passthrough.side, ExternalSide::Both);
        let qos = passthrough.qos.as_ref().unwrap();
        assert_eq!(qos.reliability.as_deref(), Some("best_effort"));
    }

    #[test]
    fn test_topic_external_flag() {
        use crate::types::ExternalSide;
        let yaml = r#"
version: 1
nodes:
  consumer:
    sub: [data]
topics:
  /sensor/data:
    type: std_msgs/msg/String
    external: pub
    sub: [consumer/data]
"#;
        let m = parse_manifest_str(yaml).unwrap();
        let topic = &m.topics["/sensor/data"];
        assert_eq!(topic.external, Some(ExternalSide::Pub));
    }

    #[test]
    fn test_external_invalid_side_errors() {
        let yaml = r#"
version: 1
external_topics:
  /tf: { external: maybe }
"#;
        let err = parse_manifest_str(yaml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("invalid external side") || msg.contains("'maybe'"),
            "got: {msg}"
        );
    }

    #[test]
    fn test_drop_shorthand() {
        let yaml = r#"
version: 1
topics:
  pointcloud:
    type: sensor_msgs/msg/PointCloud2
    drop: 5 / 100
"#;
        let m = parse_manifest_str(yaml).unwrap();
        let drop = m.topics["pointcloud"].drop.as_ref().unwrap();
        assert_eq!(drop.max_count.count_per().unwrap().n, 5);
        assert_eq!(drop.max_count.count_per().unwrap().w, 100);
        assert!(drop.max_consecutive.is_none());
    }

    // ── phase 67 vocabulary ──

    const P67: &str = r#"
version: 1
nodes:
  detector:
    sub: { image: { min_rate_hz: 30 } }
    pub: { boxes: {}, masks: {} }
    concurrency:
      exclusive:
        - [to_boxes, to_masks]
    paths:
      to_boxes:
        trigger: { input: [image] }
        output: [boxes]
        max_latency: 20ms
        min_latency: 6ms
        max_jitter: 4ms
        miss:
          tolerate: 2 / 100
          consecutive: 1
          action: skip_next
      to_masks:
        trigger: { input: [image] }
        output: [masks]
        max_latency: 35ms
topics:
  /image:
    type: sensor_msgs/msg/Image
    qos:
      reliability: reliable
      deadline: 33ms
      liveliness: automatic
      lease_duration: 200ms
"#;

    /// The whole phase-67 vocabulary must survive the HAND-ROLLED parser,
    /// which is the live path (`parse_manifest_with_spans`). A field added to
    /// the struct but not to `parse.rs` would be silently `None` in production
    /// while any serde round-trip test still passed.
    #[test]
    fn phase67_fields_reach_the_model_through_the_real_parser() {
        let m = parse_manifest_str(P67).unwrap();
        let d = &m.nodes["detector"];

        let boxes = &d.paths["to_boxes"];
        assert_eq!(boxes.max_jitter.ms().unwrap(), 4.0);
        assert_eq!(boxes.min_latency.ms().unwrap(), 6.0);

        let miss = boxes.miss.as_ref().expect("miss: parsed");
        let tol = miss.tolerate.count_per().expect("tolerate: parsed");
        assert_eq!((tol.n, tol.w), (2, 100));
        assert_eq!(miss.consecutive.count(), Some(1));
        assert_eq!(miss.action, Some(MissAction::SkipNext));

        let ex = &d
            .concurrency
            .as_ref()
            .expect("concurrency: parsed")
            .exclusive;
        assert_eq!(
            ex,
            &vec![vec!["to_boxes".to_string(), "to_masks".to_string()]]
        );

        let qos = m.topics["/image"].qos.as_ref().expect("qos parsed");
        assert_eq!(qos.deadline.unwrap().as_millis_f64(), 33.0);
        assert_eq!(qos.lease_duration.unwrap().as_millis_f64(), 200.0);

        // A path that declares none of it stays absent, not defaulted.
        let masks = &d.paths["to_masks"];
        assert!(masks.max_jitter.is_none() && masks.min_latency.is_none());
        assert!(masks.miss.is_none());
    }

    /// New duration fields have no deprecated `_ms` spelling, so the unit is
    /// required — a bare number is the mistake the type exists to catch.
    #[test]
    fn a_new_duration_field_rejects_a_bare_number() {
        for field in ["max_jitter", "min_latency"] {
            let yaml = format!(
                "version: 1\nnodes:\n  n:\n    paths:\n      p:\n        \
                 output: [o]\n        {field}: 4\n"
            );
            let err = parse_manifest_str(&yaml).unwrap_err().to_string();
            assert!(
                err.contains("no unit") && err.contains(field),
                "{field} should demand a unit, got: {err}"
            );
        }
    }

    /// An unknown `miss.action` is an error, not a silent `continue`. A
    /// vocabulary where a misspelt action quietly becomes the default is worse
    /// than one that offers fewer actions.
    #[test]
    fn an_unknown_miss_action_is_rejected() {
        let yaml = "version: 1\nnodes:\n  n:\n    paths:\n      p:\n        \
                    output: [o]\n        miss: { action: kill_it }\n";
        let err = parse_manifest_str(yaml).unwrap_err().to_string();
        assert!(
            err.contains("kill_it") && err.contains("skip_next"),
            "got: {err}"
        );
    }

    /// `miss: "2 / 100"` is the same shorthand `drop:` accepts — same
    /// arithmetic, different event, so the author learns one spelling.
    #[test]
    fn miss_accepts_the_drop_shorthand() {
        let yaml = "version: 1\nnodes:\n  n:\n    paths:\n      p:\n        \
                    output: [o]\n        miss: 3 / 50\n";
        let m = parse_manifest_str(yaml).unwrap();
        let miss = m.nodes["n"].paths["p"].miss.as_ref().unwrap();
        let tol = miss.tolerate.count_per().unwrap();
        assert_eq!((tol.n, tol.w), (3, 50));
        assert_eq!(miss.action, None, "shorthand states tolerance only");
    }

    /// ABSENT and EMPTY mean opposite things and must not be normalised
    /// together: no `concurrency:` means every path serializes (the
    /// conservative default), while `exclusive: []` says they may all run
    /// concurrently.
    #[test]
    fn absent_concurrency_differs_from_an_empty_exclusive_list() {
        let absent =
            parse_manifest_str("version: 1\nnodes:\n  n:\n    paths:\n      p: { output: [o] }\n")
                .unwrap();
        assert!(absent.nodes["n"].concurrency.is_none());

        let empty = parse_manifest_str(
            "version: 1\nnodes:\n  n:\n    concurrency: { exclusive: [] }\n    \
             paths:\n      p: { output: [o] }\n",
        )
        .unwrap();
        let c = empty.nodes["n"].concurrency.as_ref().expect("present");
        assert!(c.exclusive.is_empty());
    }

    // -- v0.1.46: the four keys of the takeover scenario --------------------

    fn err_of(yaml: &str) -> String {
        match parse_manifest_str(yaml) {
            Err(e) => e.to_string(),
            Ok(_) => panic!("expected a parse error for:\n{yaml}"),
        }
    }

    /// The demo contract's shapes, all four keys at once.
    #[test]
    fn the_four_takeover_keys_parse() {
        let yaml = r#"
version: 1
functions:
  hpc_alive: [/system/operation_mode/availability]
  hpc_in_odd:
    of: [/system/operation_mode/availability]
    when: { field: autonomous, equals: false }
  driver_took_over: { of: /vehicle/status/control_mode, when: { field: mode, equals: MANUAL } }
hazards:
  odd_exit:
    guards: [/system/operation_mode/availability]
    on: reported
    when: { field: autonomous, equals: false }
    entry_speed: 8.33
    ftti: 30s
    reaction: l3_engaged
modes:
  l3_engaged:
    requires: [hpc_alive, hpc_in_odd]
    fallback: [takeover_request, comfortable_stop, emergency_stop]
  takeover_request:
    requires: [hpc_alive]
    reaction: island.tor
    window: { duration: 10s, param: mrm_handler.takeover_request_timeout }
    exit: { holds: driver_took_over, to: manual }
  comfortable_stop:
    requires: [hpc_alive]
    reaction: island.comfortable_stop
    window: 30s
  manual: {}
nodes:
  op:
    paths:
      on_timer:
        trigger: { timer: { rate_hz: 30 } }
        output: [cmd]
        safe_state:
          emits: cmd
          settle: { decel: target_acceleration, jerk: target_jerk }
      measured:
        output: [cmd2]
        safe_state: { emits: cmd2, settle: { decel: a, jerk: j, duration: 2034ms } }
"#;
        let m = parse_manifest_str(yaml).unwrap();

        let odd = &m.functions["hpc_in_odd"];
        assert_eq!(
            odd.group.members,
            vec!["/system/operation_mode/availability"]
        );
        assert!(!odd.group.all_of);
        let w = odd.when.as_ref().unwrap();
        assert_eq!(
            (w.field.as_str(), w.op, &w.value),
            (
                "autonomous",
                CompareOp::Equals,
                &PredicateValue::Bool(false)
            )
        );
        assert!(m.functions["hpc_alive"].when.is_none());
        let drv = m.functions["driver_took_over"].when.as_ref().unwrap();
        assert_eq!(drv.value, PredicateValue::Constant("MANUAL".into()));

        let h = &m.hazards["odd_exit"];
        assert_eq!(h.entry_speed, Some(8.33));
        assert_eq!(h.when.as_ref().unwrap().field, "autonomous");

        let tor = &m.modes["takeover_request"];
        let win = tor.window.as_ref().unwrap();
        assert_eq!(win.ms(), Some(10_000.0));
        assert_eq!(win.kind, crate::BoundKind::LeastTime);
        assert_eq!(
            win.param,
            Some(crate::ParamBinding::Node(ParamRef {
                node: "mrm_handler".into(),
                name: "takeover_request_timeout".into()
            }))
        );
        assert_eq!(
            tor.exit,
            Some(ModeExit {
                holds: "driver_took_over".into(),
                spelled_on: false,
                to: "manual".into()
            })
        );
        // The scalar form is a window with no parameter: unbound.
        let bare = m.modes["comfortable_stop"].window.as_ref().unwrap();
        assert_eq!((bare.ms(), &bare.param), (Some(30_000.0), &None));

        let ss = m.nodes["op"].paths["on_timer"].safe_state.as_ref().unwrap();
        assert_eq!(ss.settle, None);
        assert_eq!(
            ss.settle_profile,
            Some(SettleProfile {
                decel: "target_acceleration".into(),
                jerk: "target_jerk".into(),
            })
        );
        let both = m.nodes["op"].paths["measured"].safe_state.as_ref().unwrap();
        assert_eq!(both.settle.ms(), Some(2034.0));
        assert!(both.settle_profile.is_some());
    }

    /// A literal settle is unchanged by the new map form.
    #[test]
    fn a_literal_settle_still_parses_as_before() {
        let m = parse_manifest_str(
            "version: 1\nnodes:\n  n:\n    paths:\n      p:\n        output: [o]\n        \
             safe_state: { emits: o, settle: 2034ms }\n",
        )
        .unwrap();
        let ss = m.nodes["n"].paths["p"].safe_state.as_ref().unwrap();
        assert_eq!(ss.settle.ms(), Some(2034.0));
        assert_eq!(ss.settle_profile, None);
    }

    #[test]
    fn a_predicate_has_exactly_one_operator_and_a_fitting_constant() {
        let h = |when: &str| {
            format!(
                "version: 1\nhazards:\n  h:\n    guards: [/t]\n    on: reported\n    when: {when}\n"
            )
        };
        let e = err_of(&h("{ field: autonomous }"));
        assert!(e.contains("no operator"), "{e}");
        let e = err_of(&h("{ field: speed, gt: 1.0, lt: 5.0 }"));
        assert!(
            e.contains("exactly one") && e.contains("second function"),
            "{e}"
        );
        let e = err_of(&h("{ field: autonomous, lt: false }"));
        assert!(e.contains("ordering operator compares numbers"), "{e}");
        let e = err_of(&h("{ equals: false }"));
        assert!(
            e.contains("hazards.h.when.field") && e.contains("required"),
            "{e}"
        );
        let e = err_of(&h("{ field: a..b, equals: 1 }"));
        assert!(e.contains("not a field path"), "{e}");
        let e = err_of(&h("{ field: a, eq: 1 }"));
        assert!(
            e.contains("unknown key in `when`") && e.contains("equals"),
            "{e}"
        );
        let e = err_of(&h("autonomous == false"));
        assert!(e.contains("expected a mapping"), "{e}");
        let e = err_of(&h("{ field: a, equals: [1, 2] }"));
        assert!(e.contains("a scalar constant"), "{e}");
        // The value classes.
        let v = |when: &str| {
            parse_manifest_str(&h(when)).unwrap().hazards["h"]
                .when
                .clone()
                .unwrap()
                .value
        };
        assert_eq!(v("{ field: v, ge: 16.7 }"), PredicateValue::Float(16.7));
        assert_eq!(v("{ field: v, le: 3 }"), PredicateValue::Int(3));
        assert_eq!(
            v("{ field: s, equals: NOT_READY }"),
            PredicateValue::Constant("NOT_READY".into())
        );
        assert_eq!(
            v("{ field: s, not_equals: ready }"),
            PredicateValue::Str("ready".into())
        );
    }

    #[test]
    fn a_function_map_is_closed() {
        let f = |body: &str| format!("version: 1\nfunctions:\n  f: {body}\n");
        let e = err_of(&f("{ of: [/t] }"));
        assert!(e.contains("without `when:`"), "{e}");
        let e = err_of(&f("{ when: { field: a, equals: 1 } }"));
        assert!(e.contains("needs the topics it reads"), "{e}");
        let e = err_of(&f("{ all_of: [/a, /b], when: { field: a, equals: 1 } }"));
        assert!(e.contains("redundant group takes no"), "{e}");
        let e = err_of(&f("{ of: [], when: { field: a, equals: 1 } }"));
        assert!(e.contains("at least one topic"), "{e}");
        let e = err_of(&f("{ any_of: [/a] }"));
        assert!(e.contains("unknown key in `functions.<name>`"), "{e}");
        // The phase-75 shapes are unchanged.
        let m = parse_manifest_str(&f("{ all_of: [/a, /b] }")).unwrap();
        assert!(m.functions["f"].group.all_of && m.functions["f"].when.is_none());
    }

    #[test]
    fn a_window_is_positive_and_names_its_parameter() {
        let m = |mode: &str| format!("version: 1\nmodes:\n  m:\n{mode}");
        let e = err_of(&m("    window: 10\n"));
        assert!(e.contains("modes.m.window") && e.contains("no unit"), "{e}");
        let e = err_of(&m("    window: 0s\n"));
        assert!(e.contains("a window of zero is no window"), "{e}");
        let e = err_of(&m("    window: { param: h.p }\n"));
        assert!(
            e.contains("modes.m.window.duration") && e.contains("required"),
            "{e}"
        );
        let e = err_of(&m(
            "    window: { duration: 10s, param: takeover_request_timeout }\n",
        ));
        assert!(e.contains("<node>.<parameter>"), "{e}");
        let e = err_of(&m("    window: { duration: 10s, parameter: h.p }\n"));
        assert!(e.contains("accepted here: duration, param"), "{e}");
        // A parameter name may itself contain dots; the node is the first
        // segment.
        let ok = parse_manifest_str(&m(
            "    window: { duration: 500ms, param: mrm_handler.turning_hazard_on.emergency }\n",
        ))
        .unwrap();
        let p = ok.modes["m"].window.clone().unwrap().param.unwrap();
        assert_eq!(
            (p.node().unwrap(), p.name()),
            ("mrm_handler", "turning_hazard_on.emergency")
        );
    }

    #[test]
    fn an_exit_needs_a_window_and_both_ends() {
        let m = |mode: &str| format!("version: 1\nmodes:\n  m:\n{mode}");
        let e = err_of(&m("    exit: { holds: driver, to: manual }\n"));
        assert!(e.contains("only for a windowed rung"), "{e}");
        let e = err_of(&m("    window: 10s\n    exit: { holds: driver }\n"));
        assert!(
            e.contains("modes.m.exit.to") && e.contains("required"),
            "{e}"
        );
        let e = err_of(&m("    window: 10s\n    exit: { to: manual }\n"));
        assert!(
            e.contains("modes.m.exit.holds") && e.contains("required"),
            "{e}"
        );
        let e = err_of(&m("    window: 10s\n    exit: manual\n"));
        assert!(e.contains("expected a mapping"), "{e}");
    }

    /// v0.1.59 (phase 90 G4): `exit.on` became `exit.holds`, so `on` names
    /// fault classes only. The old spelling parses for one release and is
    /// marked for the consumer's warning; both at once is an error.
    #[test]
    fn exit_on_is_the_old_spelling_of_holds() {
        let m = |exit: &str| format!("version: 1\nmodes:\n  m:\n    window: 10s\n{exit}");
        let new = parse_manifest_str(&m("    exit: { holds: driver, to: manual }\n")).unwrap();
        let old = parse_manifest_str(&m("    exit: { on: driver, to: manual }\n")).unwrap();
        let (n, o) = (
            new.modes["m"].exit.clone().unwrap(),
            old.modes["m"].exit.clone().unwrap(),
        );
        assert_eq!((n.holds.as_str(), n.spelled_on), ("driver", false));
        assert_eq!((o.holds.as_str(), o.spelled_on), ("driver", true));
        assert_eq!((n.to, o.to), ("manual".to_string(), "manual".to_string()));
        let e = err_of(&m("    exit: { holds: a, on: a, to: manual }\n"));
        assert!(e.contains("old spelling of `holds:`"), "{e}");
    }

    /// v0.1.59 (phase 90 G4): a hazard enters a mode or reacts by one scope
    /// path, never both. `reaction:` keeps parsing whatever it names; the
    /// consumer tells a mode from a path by lookup.
    #[test]
    fn a_hazard_enters_a_mode_or_reacts_by_a_path_not_both() {
        let h = |body: &str| format!("version: 1\nhazards:\n  h:\n{body}");
        let m = parse_manifest_str(&h("    enters: driving\n")).unwrap();
        let d = &m.hazards["h"];
        assert_eq!(
            (d.enters.as_deref(), d.reaction.as_deref(), d.entry_key()),
            (Some("driving"), None, "enters")
        );
        assert_eq!(d.entry().map(String::as_str), Some("driving"));
        let m = parse_manifest_str(&h("    reaction: safety.stop\n")).unwrap();
        let d = &m.hazards["h"];
        assert_eq!(
            (d.entry().map(String::as_str), d.entry_key()),
            (Some("safety.stop"), "reaction")
        );
        let e = err_of(&h("    enters: driving\n    reaction: safety.stop\n"));
        assert!(e.contains("not both"), "{e}");
    }

    #[test]
    fn entry_speed_is_a_positive_number_in_metres_per_second() {
        let h = |v: &str| format!("version: 1\nhazards:\n  h:\n    entry_speed: {v}\n");
        assert_eq!(
            parse_manifest_str(&h("16.7")).unwrap().hazards["h"].entry_speed,
            Some(16.7)
        );
        assert_eq!(
            parse_manifest_str(&h("8")).unwrap().hazards["h"].entry_speed,
            Some(8.0)
        );
        let e = err_of(&h("0"));
        assert!(e.contains("positive"), "{e}");
        let e = err_of(&h("-3.0"));
        assert!(e.contains("positive"), "{e}");
        let e = err_of(&h("30km/h"));
        assert!(e.contains("hazards.h.entry_speed"), "{e}");
    }

    /// Phase 90 G6: `settle: <d>` is `settle: { duration: <d> }`, the map
    /// takes a response beside a written duration as beside a profile, and
    /// `source:` says where a written duration comes from.
    #[test]
    fn g6_settle_takes_one_map_form() {
        let ss = |settle: &str| {
            let m = parse_manifest_str(&format!(
                "version: 1\nnodes:\n  n:\n    paths:\n      p:\n        output: [o]\n        \
                 safe_state: {{ emits: o, settle: {settle} }}\n"
            ))
            .unwrap();
            m.nodes["n"].paths["p"].safe_state.clone().unwrap()
        };
        let bare = ss("2s");
        let map = ss("{ duration: 2s }");
        assert_eq!(bare.settle, map.settle);
        assert_eq!((bare.settle_map, map.settle_map), (false, true));
        assert_eq!(
            map.written_settle_ms(),
            Some(2000.0),
            "omitted response is 0"
        );
        let both = ss("{ duration: 2s, response: 300ms, source: measured }");
        assert_eq!(both.written_settle_ms(), Some(2300.0));
        assert_eq!(both.settle_profile, None);
        assert_eq!(
            both.settle.source().map(|s| s.kind),
            Some(crate::SourceKind::Measured)
        );
        let prof = ss("{ decel: a, jerk: j, response: 300ms }");
        assert_eq!(prof.settle, None);
        assert_eq!(prof.response_ms(), 300.0);
        assert!(prof.settle_profile.is_some());
        let keys: Vec<String> = parse_manifest_str(
            "version: 1\nnodes:\n  n:\n    paths:\n      p:\n        output: [o]\n        \
             safe_state: { emits: o, settle: { duration: 2s, response: 300ms, source: measured } }\n",
        )
        .unwrap()
        .bounds()
        .into_iter()
        .map(|(k, _)| k)
        .collect();
        assert_eq!(
            keys,
            [
                "nodes.n.paths.p.safe_state.settle.duration",
                "nodes.n.paths.p.safe_state.settle.response"
            ]
        );
    }

    #[test]
    fn g6_a_settle_map_states_a_duration_or_a_profile() {
        let p = |settle: &str| {
            format!(
                "version: 1\nnodes:\n  n:\n    paths:\n      p:\n        output: [o]\n        \
                 safe_state: {{ emits: o, settle: {settle} }}\n"
            )
        };
        let e = err_of(&p("{ response: 300ms }"));
        assert!(
            e.contains("safe_state.settle.duration") && e.contains("braking profile"),
            "{e}"
        );
        let e = err_of(&p("{ decel: a, jerk: j, source: measured }"));
        assert!(
            e.contains("safe_state.settle.source") && e.contains("derived"),
            "{e}"
        );
    }

    /// Phase 90 G3: a node path's `drop:` is refused, naming the topic form;
    /// a scope path's stays.
    #[test]
    fn g3_a_node_path_drop_is_refused() {
        let e = err_of(
            "version: 1\nnodes:\n  n:\n    paths:\n      p:\n        output: [o]\n        \
             drop: 1 / 10\n",
        );
        assert!(
            e.contains("nodes.n.paths.p.drop")
                && e.contains("topics.<t>.drop")
                && e.contains("play_launch migrate"),
            "{e}"
        );
        let m =
            parse_manifest_str("version: 1\npaths:\n  e2e:\n    output: [o]\n    drop: 1 / 10\n")
                .unwrap();
        assert!(m.paths["e2e"].drop.is_some());
    }

    /// Phase 90 G3: the topic-level `max_transport` still parses for one
    /// release (deprecated, not removed).
    #[test]
    fn g3_a_topic_max_transport_still_parses() {
        let m = parse_manifest_str("version: 1\ntopics:\n  /t: { type: T, max_transport: 5ms }\n")
            .unwrap();
        assert_eq!(m.topics["/t"].max_transport.ms(), Some(5.0));
        assert_eq!(
            crate::field_table::lookup(crate::field_table::Context::Topic, "max_transport")
                .map(|f| f.status),
            Some(crate::field_table::Status::Deprecated)
        );
    }

    #[test]
    fn a_settle_profile_names_both_parameters_and_nothing_else() {
        let p = |settle: &str| {
            format!(
                "version: 1\nnodes:\n  n:\n    paths:\n      p:\n        output: [o]\n        \
                 safe_state: {{ emits: o, settle: {settle} }}\n"
            )
        };
        let e = err_of(&p("{ decel: target_acceleration }"));
        assert!(
            e.contains("safe_state.settle.jerk") && e.contains("required"),
            "{e}"
        );
        let e = err_of(&p("{ decel: a, jerk: j, entry_speed: 3.0 }"));
        assert!(e.contains("unknown key in `safe_state.settle`"), "{e}");
        let e = err_of(&p("{ decel: a, jerk: j, duration: 2034 }"));
        assert!(e.contains("no unit"), "{e}");
    }

    /// An endpoint key is a local name. A `/`
    /// in it is refused in every endpoint section and both spellings, with
    /// the message saying where the topic is connected instead.
    #[test]
    fn an_endpoint_key_with_a_slash_is_refused() {
        for (section, body) in [
            ("pub", "{ /abs/topic: {} }"),
            ("sub", "{ /abs/topic: { min_rate_hz: 10 } }"),
            ("sub", "[/abs/topic]"),
            ("cli", "{ ns/call: {} }"),
            ("srv", "{ /abs/serve: {} }"),
        ] {
            let e = err_of(&format!(
                "version: 1\nnodes:\n  n:\n    {section}: {body}\n"
            ));
            assert!(e.contains("contains `/`"), "{section} {body}: {e}");
            assert!(e.contains("an endpoint key is a local name"), "{e}");
            assert!(
                e.contains("remap the topic in the launch file or wire it under"),
                "{e}"
            );
            assert!(e.contains(&format!("at 'nodes.n.{section}.")), "{e}");
        }
        // A local name is unaffected.
        let m = parse_manifest_str("version: 1\nnodes:\n  n:\n    pub: { scan: {} }\n").unwrap();
        assert!(m.nodes["n"].publishers.contains_key("scan"));
    }

    /// Every unknown key of a file in one
    /// refusal, and `max_rate` suggests `max_rate_hz`.
    #[test]
    fn every_unknown_key_is_named_at_once() {
        let yaml = "version: 1\nnodes:\n  n:\n    pub:\n      a: { max_rate: 10 }\n    \
                    paths:\n      p: { max_latencyy: 5ms }\ntopics:\n  /t: { type: std_msgs/msg/String, rate_hzz: 1 }\n";
        let e = err_of(yaml);
        assert!(e.contains("at 'nodes.n.pub.a.max_rate'"), "{e}");
        assert!(e.contains("did you mean `max_rate_hz`?"), "{e}");
        assert!(e.contains("Also refused in this file (2 more)"), "{e}");
        assert!(
            e.contains("max_latencyy") && e.contains("`max_latency`"),
            "{e}"
        );
        assert!(e.contains("rate_hzz") && e.contains("`rate_hz`"), "{e}");
        // One unknown key reads as it always did.
        let one = err_of("version: 1\ntopics:\n  /t: { type: std_msgs/msg/String, rate_hzz: 1 }\n");
        assert!(!one.contains("Also refused"), "{one}");
        // The collection does not leak into the next parse.
        assert!(parse_manifest_str("version: 1\n").is_ok());
    }

    /// Phase 10 B1, B2, B7: a timer and a `max_age` bound to their
    /// parameters, a settle's response, and a budget's source in each form.
    #[test]
    fn phase10_budget_keys_parse() {
        let m = parse_manifest_str(
            r#"
version: 1
hazards:
  h:
    guards: [/a]
    ftti: { duration: 10s, source: requirement }
    reaction: r
modes:
  tor:
    requires: []
    window: { duration: 10s, param: n.timeout, source: requirement }
nodes:
  n:
    params:
      update_rate: { type: integer }
      period_s: { type: double }
      timeout: { type: double }
      response: { type: double }
    sub:
      a:
        max_age: { duration: 500ms, param: timeout, source: requirement }
        max_transport:
          duration: 56ms
          source: { measured: { max: 46.42ms, n: 12, margin: 0.2 } }
        on_violation:
          reaction: react
          within: { duration: 149ms, source: derived }
    paths:
      tick:
        trigger: { timer: { rate_hz: 10, param: update_rate, jitter: { duration: 18ms, source: measured } } }
        output: [b]
      slow:
        trigger: { timer: { rate_hz: 2, period_param: period_s } }
        output: [b]
      react:
        trigger: { input: [a] }
        output: [b]
        max_latency:
          duration: 149ms
          source:
            measured:
              margin: 0.2
              terms:
                tick: { period: 100ms, max: 14.84ms, n: 1641 }
                work: { max: 25.04ms, n: 13 }
        safe_state:
          emits: b
          settle: { decel: d, jerk: j, response: { duration: 256ms, source: { measured: { max: 213.11ms, n: 3, margin: 0.2 } } } }
    pub:
      b: {}
"#,
        )
        .unwrap();
        let n = &m.nodes["n"];
        assert_eq!(
            n.paths["tick"].timer_param,
            Some(TimerParam::Rate(ParamBinding::Own("update_rate".into())))
        );
        assert_eq!(
            n.paths["slow"].timer_param,
            Some(TimerParam::Period(ParamBinding::Own("period_s".into())))
        );
        assert_eq!(
            n.paths["tick"].timer_jitter().and_then(crate::Bound::ms),
            Some(18.0)
        );
        let a = &n.subscribers["a"];
        assert_eq!(a.max_age.ms(), Some(500.0));
        assert_eq!(
            a.max_age.param(),
            Some(&crate::ParamBinding::Own("timeout".into()))
        );
        assert_eq!(a.max_transport.ms(), Some(56.0));
        let react = &n.paths["react"];
        assert_eq!(react.max_latency.ms(), Some(149.0));
        let ss = react.safe_state.as_ref().unwrap();
        assert!(ss.settle_profile.is_some());
        assert_eq!(ss.settle_response.ms(), Some(256.0));

        let sources = m.budget_sources();
        let src = |k: &str| *sources.get(k).unwrap_or_else(|| panic!("{k}"));
        assert_eq!(src("hazards.h.ftti").kind, SourceKind::Requirement);
        assert_eq!(src("modes.tor.window").kind, SourceKind::Requirement);
        assert_eq!(src("nodes.n.sub.a.max_age").kind, SourceKind::Requirement);
        assert_eq!(
            src("nodes.n.sub.a.on_violation.within").kind,
            SourceKind::Derived
        );
        let jit = src("nodes.n.paths.tick.trigger.timer.jitter");
        assert_eq!(
            (jit.kind, jit.measured.is_none()),
            (SourceKind::Measured, true)
        );
        let link = src("nodes.n.sub.a.max_transport")
            .measured
            .as_ref()
            .unwrap();
        assert_eq!(link.budget_ms(), 56.0, "46.42 x 1.2 = 55.70 -> 56");
        let route = src("nodes.n.paths.react.max_latency")
            .measured
            .as_ref()
            .unwrap();
        assert_eq!(route.terms.len(), 2);
        assert_eq!(route.terms[0].name.as_deref(), Some("tick"));
        assert_eq!(
            route.budget_ms(),
            149.0,
            "100 + 14.84 x 1.2 -> 118, 25.04 x 1.2 -> 31"
        );
        let resp = src("nodes.n.paths.react.safe_state.settle.response")
            .measured
            .as_ref()
            .unwrap();
        assert_eq!(resp.budget_ms(), 256.0);
        assert_eq!(m.budget_sources().len(), 8);
    }

    /// The new keys refuse what they cannot mean.
    #[test]
    fn phase10_budget_keys_refuse_nonsense() {
        let base = |body: &str| {
            parse_manifest_str(&format!(
                "version: 1\nnodes:\n  n:\n    sub:\n      a: {{}}\n    paths:\n      p:\n{body}"
            ))
        };
        let err = |body: &str| base(body).unwrap_err().to_string();
        let e = err("        trigger: { timer: { rate_hz: 10, param: r, period_param: p } }\n");
        assert!(e.contains("not both"), "{e}");
        let e = err(
            "        trigger: { input: [a] }\n        max_latency: { duration: 5ms, param: x }\n",
        );
        assert!(e.contains("cannot be bound to a parameter"), "{e}");
        let e = err("        trigger: { input: [a] }\n        max_latency: { source: measured }\n");
        assert!(e.contains("required: the budget itself"), "{e}");
        let e = err(
            "        trigger: { input: [a] }\n        max_latency: { duration: 5ms, source: guessed }\n",
        );
        assert!(e.contains("is not a source"), "{e}");
        let e = err(
            "        trigger: { input: [a] }\n        max_latency: { duration: 5ms, source: { derived: x } }\n",
        );
        assert!(e.contains("only `measured` carries observations"), "{e}");
        let e = err(
            "        trigger: { input: [a] }\n        max_latency: { duration: 5ms, source: { measured: { max: 4ms } } }\n",
        );
        assert!(e.contains("margin"), "{e}");
        let e = err(
            "        trigger: { input: [a] }\n        max_latency: { duration: 5ms, source: { measured: { max: 4ms, margin: 0.2, terms: { a: { max: 1ms } } } } }\n",
        );
        assert!(e.contains("not both"), "{e}");
        let e = err(
            "        trigger: { input: [a] }\n        max_latency: { duration: 5ms, source: { measured: { margin: 0.2, terms: { a: { mx: 1ms } } } } }\n",
        );
        assert!(e.contains("unknown key"), "{e}");
        // The bare forms are unchanged, and record no source.
        let m = base(
            "        trigger: { timer: { rate_hz: 10, jitter: 2ms } }\n        max_latency: 5ms\n",
        )
        .unwrap();
        assert!(m.budget_sources().is_empty());
        assert_eq!(m.nodes["n"].paths["p"].timer_param, None);
    }
}
