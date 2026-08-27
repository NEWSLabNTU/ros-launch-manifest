//! YAML parser for manifest files using yaml-rust2.
//!
//! Parses YAML into typed [`Manifest`] AST. Handles both plain list
//! and map forms for endpoints (e.g., `pub: [a, b]` vs `pub: {a: {min_rate_hz: 10}}`).

use crate::duration::Duration;
use crate::{span::SpanIndex, types::*};
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
    parse_manifest_yaml(&docs[0], "")
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
    Ok(Manifest {
        version: yaml_u32(doc, "version").unwrap_or(1),
        args: parse_args(doc),
        exclude_patterns: yaml_string_list(doc, "exclude_patterns"),
        nodes: parse_nodes(doc, ctx)?,
        topics: parse_topics(doc, ctx)?,
        services: parse_services(doc, ctx)?,
        actions: parse_actions(doc, ctx)?,
        includes: parse_includes(doc, ctx)?,
        paths: parse_paths(doc, ctx)?,
        external_topics: parse_external_topics(doc, ctx)?,
        chains: parse_chains(doc, ctx)?,
    })
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
    Ok(NodeDecl {
        if_condition: yaml_string(yaml, "if"),
        unless_condition: yaml_string(yaml, "unless"),
        lifecycle: yaml_bool(yaml, "lifecycle"),
        publishers: parse_endpoints(yaml, "pub", ctx)?,
        subscribers: parse_endpoints(yaml, "sub", ctx)?,
        srv: parse_srv_endpoints(yaml, "srv", ctx)?,
        cli: parse_endpoints(yaml, "cli", ctx)?,
        paths: parse_paths(yaml, ctx)?,
        criticality: yaml_string(yaml, "criticality"),
        concurrency: parse_concurrency(yaml, ctx)?,
    })
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
                eps.insert(name, EndpointProps::default());
            }
        }
        Yaml::Hash(hash) => {
            for (k, v) in hash {
                let name = yaml_str_owned(k);
                let path = format_path(ctx, &format!("{key}.{name}"));
                let props = parse_endpoint_props(v, &path)?;
                eps.insert(name, props);
            }
        }
        _ => {
            return Err(field_err(ctx, key, "expected list or mapping"));
        }
    }
    Ok(eps)
}

fn parse_endpoint_props(yaml: &Yaml, ctx: &str) -> Result<EndpointProps, ParseError> {
    if yaml.is_null() || yaml.is_badvalue() {
        return Ok(EndpointProps::default());
    }
    let props = EndpointProps {
        min_rate_hz: yaml_f64(yaml, "min_rate_hz"),
        max_rate_hz: yaml_f64(yaml, "max_rate_hz"),
        jitter: yaml_duration(yaml, "jitter", "jitter_ms")?,
        max_age: yaml_duration(yaml, "max_age", "max_age_ms")?,
        state: yaml_bool(yaml, "state"),
        required: yaml_bool(yaml, "required"),
        qos: parse_qos(yaml)?,
        max_transport: yaml_duration(yaml, "max_transport", "max_transport_ms")?,
        buffer: parse_buffer(yaml, ctx)?,
    };
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
fn parse_buffer(doc: &Yaml, ctx: &str) -> Result<Option<Buffer>, ParseError> {
    let raw = match yaml_string(doc, "buffer") {
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
                eps.insert(name, SrvEndpointProps::default());
            }
        }
        Yaml::Hash(hash) => {
            for (k, v) in hash {
                let name = yaml_str_owned(k);
                let props = SrvEndpointProps {
                    max_response: if v.is_null() || v.is_badvalue() {
                        None
                    } else {
                        yaml_duration(v, "max_response", "max_response_ms")?
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
        if_condition: yaml_string(yaml, "if"),
        unless_condition: yaml_string(yaml, "unless"),
        msg_type: yaml_string(yaml, "type")
            .ok_or_else(|| field_err(ctx, "type", "topic must have a type"))?,
        publishers: yaml_string_list(yaml, "pub"),
        subscribers: yaml_string_list(yaml, "sub"),
        qos: parse_qos(yaml)?,
        rate_hz: yaml_f64(yaml, "rate_hz"),
        max_transport: yaml_duration(yaml, "max_transport", "max_transport_ms")?,
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
                msg_type: yaml_string(v, "type"),
                qos: parse_qos(v)?,
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
    let raw = match yaml_string(doc, key) {
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
        out.insert(
            name,
            ServiceDecl {
                if_condition: yaml_string(v, "if"),
                unless_condition: yaml_string(v, "unless"),
                srv_type: yaml_string(v, "type").unwrap_or_default(),
                server: yaml_string_list(v, "server"),
                client: yaml_string_list(v, "client"),
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
        out.insert(
            name,
            ActionDecl {
                if_condition: yaml_string(v, "if"),
                unless_condition: yaml_string(v, "unless"),
                action_type: yaml_string(v, "type").unwrap_or_default(),
                server: yaml_string_list(v, "server"),
                client: yaml_string_list(v, "client"),
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
        if let Some(manifest_path) = yaml_string(v, "manifest") {
            out.insert(
                name,
                IncludeDecl::External {
                    manifest: manifest_path,
                },
            );
        } else {
            // Inline scope
            let inner = parse_manifest_yaml(v, &path)?;
            out.insert(name, IncludeDecl::Inline(Box::new(inner)));
        }
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
fn parse_args(doc: &Yaml) -> BTreeMap<String, ArgDecl> {
    let mut out = BTreeMap::new();
    let section = &doc["args"];
    if section.is_badvalue() {
        return out;
    }
    match section {
        Yaml::Array(arr) => {
            for item in arr {
                out.insert(yaml_str_owned(item), ArgDecl::String);
            }
        }
        Yaml::Hash(hash) => {
            for (k, v) in hash {
                let name = yaml_str_owned(k);
                let decl = parse_arg_decl(v);
                out.insert(name, decl);
            }
        }
        _ => {}
    }
    out
}

fn parse_arg_decl(yaml: &Yaml) -> ArgDecl {
    if yaml.is_null() || yaml.is_badvalue() {
        return ArgDecl::String;
    }
    // Check for { type: bool }
    if let Some(type_str) = yaml_string(yaml, "type") {
        if type_str == "bool" {
            return ArgDecl::Bool;
        }
        // "string" or any other type → free string
        return ArgDecl::String;
    }
    // Check for { choices: [a, b, c] }
    let choices = yaml_string_list(yaml, "choices");
    if !choices.is_empty() {
        return ArgDecl::Choices(choices);
    }
    ArgDecl::String
}

// ── Paths ──

fn parse_paths(doc: &Yaml, ctx: &str) -> Result<BTreeMap<String, PathDecl>, ParseError> {
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
        let path = parse_path_decl(v, &path_ctx)?;
        out.insert(name, path);
    }
    Ok(out)
}

fn parse_path_decl(yaml: &Yaml, ctx: &str) -> Result<PathDecl, ParseError> {
    let input = parse_string_or_list(yaml, "input");
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
        if_condition: yaml_string(yaml, "if"),
        unless_condition: yaml_string(yaml, "unless"),
        input,
        output: yaml_string_list(yaml, "output"),
        max_latency: yaml_duration(yaml, "max_latency", "max_latency_ms")?,
        correlation: yaml_string(yaml, "correlation"),
        tolerance: yaml_duration(yaml, "tolerance", "tolerance_ms")?,
        drop: parse_drop_spec(yaml, "drop", ctx)?,
        trigger,
        sync,
        max_jitter: yaml_duration_new(yaml, "max_jitter")?,
        min_latency: yaml_duration_new(yaml, "min_latency")?,
        miss: parse_miss_spec(yaml, ctx)?,
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
            let rate_hz = yaml_f64(v, "rate_hz")
                .ok_or_else(|| field_err(ctx, "trigger.timer", "requires 'rate_hz'"))?;
            if rate_hz <= 0.0 {
                return Err(field_err(ctx, "trigger.timer.rate_hz", "must be > 0"));
            }
            Ok(Some(Trigger::Timer { rate_hz }))
        }
        "input" => {
            let endpoints = yaml_direct_string_list(v);
            Ok(Some(Trigger::Input(endpoints)))
        }
        other => Err(field_err(
            ctx,
            "trigger",
            &format!("invalid trigger key '{other}', expected 'timer' or 'input'"),
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
    let policy_str = yaml_string(section, "policy").ok_or_else(|| {
        field_err(
            ctx,
            "sync.policy",
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
                "sync.policy",
                &format!(
                    "invalid policy '{other}', expected 'exact', 'approximate', or 'timeout_any'"
                ),
            ));
        }
    };
    let max_interval = yaml_duration(section, "max_interval", "max_interval_ms")?;
    let timeout = yaml_duration(section, "timeout", "timeout_ms")?;
    match policy {
        SyncPolicy::TimeoutAny => {
            if timeout.is_none() {
                return Err(field_err(
                    ctx,
                    "sync.timeout",
                    "required when policy is 'timeout_any'",
                ));
            }
        }
        SyncPolicy::Exact | SyncPolicy::Approximate => {
            if max_interval.is_none() {
                return Err(field_err(
                    ctx,
                    "sync.max_interval",
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

// ── Chains ──

/// Parse the top-level `chains:` section (Vocabulary v2, Phase 44.1 §4).
fn parse_chains(doc: &Yaml, ctx: &str) -> Result<BTreeMap<String, ChainDecl>, ParseError> {
    let mut out = BTreeMap::new();
    let section = &doc["chains"];
    if section.is_badvalue() {
        return Ok(out);
    }
    let hash = section
        .as_hash()
        .ok_or_else(|| field_err(ctx, "chains", "expected mapping"))?;
    for (k, v) in hash {
        let name = yaml_str_owned(k);
        let path = format_path(ctx, &format!("chains.{name}"));
        let chain = parse_chain_decl(v, &path)?;
        out.insert(name, chain);
    }
    Ok(out)
}

fn parse_chain_decl(yaml: &Yaml, ctx: &str) -> Result<ChainDecl, ParseError> {
    let semantics_str = yaml_string(yaml, "semantics")
        .ok_or_else(|| field_err(ctx, "semantics", "required: 'reaction' or 'age'"))?;
    let semantics = match semantics_str.as_str() {
        "reaction" => ChainSemantics::Reaction,
        "age" => ChainSemantics::Age,
        other => {
            return Err(field_err(
                ctx,
                "semantics",
                &format!("invalid semantics '{other}', expected 'reaction' or 'age'"),
            ));
        }
    };
    // ChainDecl's latency is required, so absence is an error rather than None.
    let max_latency = yaml_duration(yaml, "max_latency", "max_latency_ms")?
        .ok_or_else(|| field_err(ctx, "max_latency", "required"))?;
    let segments = parse_chain_segments(yaml, ctx)?;
    Ok(ChainDecl {
        semantics,
        max_latency,
        segments,
    })
}

fn parse_chain_segments(yaml: &Yaml, ctx: &str) -> Result<Vec<ChainSegment>, ParseError> {
    let section = &yaml["segments"];
    let arr = section
        .as_vec()
        .ok_or_else(|| field_err(ctx, "segments", "expected a non-empty list"))?;
    if arr.is_empty() {
        return Err(field_err(ctx, "segments", "must be non-empty"));
    }
    let mut segments = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        let seg_ctx = format_path(ctx, &format!("segments[{i}]"));
        segments.push(parse_chain_segment(item, &seg_ctx)?);
    }
    // Local shape only (Phase 44.1 §4) — cross-file link resolution
    // (does every `via` exist? is it produced/consumed correctly?) is
    // the checker's `chain-link` rule (W2).
    if !matches!(segments.first(), Some(ChainSegment::Path { .. })) {
        return Err(field_err(
            ctx,
            "segments",
            "first segment must be a path segment ({ scope, path })",
        ));
    }
    if !matches!(segments.last(), Some(ChainSegment::Path { .. })) {
        return Err(field_err(
            ctx,
            "segments",
            "last segment must be a path segment ({ scope, path })",
        ));
    }
    for w in segments.windows(2) {
        if matches!(w[0], ChainSegment::Via { .. }) && matches!(w[1], ChainSegment::Via { .. }) {
            return Err(field_err(
                ctx,
                "segments",
                "two adjacent 'via' segments are not allowed",
            ));
        }
    }
    Ok(segments)
}

fn parse_chain_segment(yaml: &Yaml, ctx: &str) -> Result<ChainSegment, ParseError> {
    let has_via = !yaml["via"].is_badvalue();
    let has_scope = !yaml["scope"].is_badvalue();
    let has_path = !yaml["path"].is_badvalue();
    if has_via && (has_scope || has_path) {
        return Err(field_err(
            ctx,
            "segment",
            "'via' cannot be combined with 'scope'/'path'",
        ));
    }
    if has_via {
        let via =
            yaml_string(yaml, "via").ok_or_else(|| field_err(ctx, "via", "expected a string"))?;
        return Ok(ChainSegment::Via { via });
    }
    let scope = yaml_string(yaml, "scope")
        .ok_or_else(|| field_err(ctx, "scope", "path segment requires 'scope'"))?;
    let path = yaml_string(yaml, "path")
        .ok_or_else(|| field_err(ctx, "path", "path segment requires 'path'"))?;
    Ok(ChainSegment::Path { scope, path })
}

// ── Drop ──

fn parse_drop_spec(doc: &Yaml, key: &str, ctx: &str) -> Result<Option<DropSpec>, ParseError> {
    let section = &doc[key];
    if section.is_badvalue() {
        return Ok(None);
    }
    // Shorthand: "5 / 100"
    if let Some(s) = section.as_str() {
        let count: DropCount = s.parse().map_err(|e: String| field_err(ctx, key, &e))?;
        return Ok(Some(DropSpec {
            max_count: Some(count),
            max_consecutive: None,
        }));
    }
    // Full form: { max_count: "5 / 100", max_consecutive: 3 }
    let max_count = yaml_string(section, "max_count")
        .map(|s| {
            s.parse::<DropCount>()
                .map_err(|e| field_err(ctx, &format!("{key}.max_count"), &e.to_string()))
        })
        .transpose()?;
    let max_consecutive = yaml_u32(section, "max_consecutive");

    Ok(Some(DropSpec {
        max_count,
        max_consecutive,
    }))
}

// ── QoS ──

/// Returns `Err` rather than `None` on a malformed duration: phase 59's whole
/// argument is that a wrong unit must not pass quietly, and swallowing the
/// error here to keep an `Option` return would do exactly that.
fn parse_qos(doc: &Yaml) -> Result<Option<QosDecl>, ParseError> {
    let section = &doc["qos"];
    if section.is_badvalue() {
        return Ok(None);
    }
    Ok(Some(QosDecl {
        reliability: yaml_string(section, "reliability"),
        durability: yaml_string(section, "durability"),
        depth: yaml_u32(section, "depth"),
        history: yaml_string(section, "history"),
        lifespan: yaml_duration(section, "lifespan", "lifespan_ms")?,
        liveliness: yaml_string(section, "liveliness"),
        deadline: yaml_duration_new(section, "deadline")?,
        lease_duration: yaml_duration_new(section, "lease_duration")?,
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

fn yaml_string(doc: &Yaml, key: &str) -> Option<String> {
    doc[key].as_str().map(|s| s.to_string())
}

/// Read a duration field: the phase-59 spelling, or the deprecated name.
///
/// This is the entry point serde cannot provide. The contract reader is
/// hand-rolled so it can produce source spans for checker diagnostics, and
/// that same hand-rolling lets it do something the serde adapters cannot: see
/// WHICH spelling was used, and therefore **reject a bare number under the new
/// name**. That rejection is the point of phase 59, and this is the only path
/// where it is fully enforceable.
///
/// The conversion itself is `duration::from_legacy_scalar`, shared with the
/// serde adapters so the two cannot drift.
fn yaml_duration(
    doc: &Yaml,
    canonical: &str,
    legacy: &str,
) -> Result<Option<Duration>, ParseError> {
    use crate::duration::{LegacyUnit, from_legacy_scalar};

    let no_unit = |shown: String| {
        field_err(
            "",
            canonical,
            &format!(
                "`{canonical}: {shown}` has no unit — write `{canonical}: {shown}ms` \
                 (or ns/us/s). The unit is required so a value cannot be 1000x wrong \
                 and still parse"
            ),
        )
    };

    match &doc[canonical] {
        Yaml::String(text) => from_legacy_scalar(Some(text), None, LegacyUnit::Millis)
            .map_err(|e| field_err("", canonical, &e.to_string())),
        Yaml::Integer(i) => Err(no_unit(i.to_string())),
        Yaml::Real(r) => Err(no_unit(r.clone())),
        // Deprecated spelling keeps its implied unit exactly, so an
        // un-migrated contract cannot change meaning.
        _ => from_legacy_scalar(None, yaml_f64(doc, legacy), LegacyUnit::Millis)
            .map_err(|e| field_err("", legacy, &e.to_string())),
    }
}

/// A duration field introduced AFTER the unit-suffix migration, so it has no
/// deprecated `_ms` spelling to fall back to and the unit is simply required.
///
/// Written as its own helper rather than calling `yaml_duration(doc, k, k)`
/// because "the legacy name is the same as the canonical name" is a confusing
/// way to say "there is no legacy name".
fn yaml_duration_new(doc: &Yaml, key: &str) -> Result<Option<Duration>, ParseError> {
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
    // Shorthand: `miss: "2 / 100"` — tolerance only, default action.
    if let Some(text) = section.as_str() {
        let count: DropCount = text
            .parse()
            .map_err(|e: String| field_err(ctx, "miss", &e))?;
        return Ok(Some(MissSpec {
            tolerate: Some(count),
            consecutive: None,
            action: None,
        }));
    }
    let tolerate = yaml_string(section, "tolerate")
        .map(|s| {
            s.parse::<DropCount>()
                .map_err(|e| field_err(ctx, "miss.tolerate", &e.to_string()))
        })
        .transpose()?;
    let action = match yaml_string(section, "action").as_deref() {
        None => None,
        Some("continue") => Some(MissAction::Continue),
        Some("skip_next") => Some(MissAction::SkipNext),
        Some("abort") => Some(MissAction::Abort),
        Some(other) => {
            return Err(field_err(
                ctx,
                "miss.action",
                &format!(
                    "unknown action '{other}' — expected `continue`, `skip_next` or `abort`"
                ),
            ));
        }
    };
    Ok(Some(MissSpec {
        tolerate,
        consecutive: yaml_u32(section, "consecutive"),
        action,
    }))
}

/// `concurrency:` — which of a node's paths may not run concurrently.
///
/// An ABSENT section and an empty `exclusive:` list mean opposite things, so
/// the distinction is preserved rather than normalised away: absent means every
/// path serializes (the conservative default both `rclcpp` and nano-ros
/// already take), while `exclusive: []` says they may all run concurrently.
fn parse_concurrency(doc: &Yaml, ctx: &str) -> Result<Option<ConcurrencyDecl>, ParseError> {
    let section = &doc["concurrency"];
    if section.is_badvalue() {
        return Ok(None);
    }
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

fn yaml_f64(doc: &Yaml, key: &str) -> Option<f64> {
    match &doc[key] {
        Yaml::Real(s) => s.parse().ok(),
        Yaml::Integer(i) => Some(*i as f64),
        _ => None,
    }
}

fn yaml_u32(doc: &Yaml, key: &str) -> Option<u32> {
    doc[key].as_i64().map(|i| i as u32)
}

fn yaml_bool(doc: &Yaml, key: &str) -> Option<bool> {
    doc[key].as_bool()
}

fn yaml_string_list(doc: &Yaml, key: &str) -> Vec<String> {
    match &doc[key] {
        Yaml::Array(arr) => arr.iter().map(yaml_str_owned).collect(),
        _ => vec![],
    }
}

/// Parse a field that can be a single string or a list of strings.
fn parse_string_or_list(doc: &Yaml, key: &str) -> Vec<String> {
    yaml_direct_string_list(&doc[key])
}

/// Interpret a YAML value directly (not indexed under a key) as a single
/// string or a list of strings. Used for nested trigger sub-values like
/// `trigger: { input: [a, b] }`, where `v` is already the array/string.
fn yaml_direct_string_list(y: &Yaml) -> Vec<String> {
    match y {
        Yaml::String(s) => vec![s.clone()],
        Yaml::Array(arr) => arr.iter().map(yaml_str_owned).collect(),
        _ => vec![],
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

    /// Phase 59: both spellings parse to the same value, and the new one
    /// refuses a bare number.
    ///
    /// The migration's safety argument is that an un-migrated contract behaves
    /// EXACTLY as before, so these must agree to the nanosecond.
    #[test]
    fn duration_field_accepts_both_spellings_and_refuses_bare_numbers() {
        use yaml_rust2::YamlLoader;
        let load = |t: &str| YamlLoader::load_from_str(t).unwrap().remove(0);

        let old = yaml_duration(&load("max_latency_ms: 12"), "max_latency", "max_latency_ms")
            .unwrap()
            .unwrap();
        let new = yaml_duration(&load("max_latency: 12ms"), "max_latency", "max_latency_ms")
            .unwrap()
            .unwrap();
        assert_eq!(old, new, "both spellings must mean the same duration");
        assert_eq!(new.as_millis_f64(), 12.0);

        // A bare number under the NEW name is the 1000x error. This is the one
        // path that can refuse it, because it can see which name was used —
        // the serde adapters cannot.
        let err = yaml_duration(&load("max_latency: 12"), "max_latency", "max_latency_ms")
            .unwrap_err()
            .to_string();
        assert!(err.contains("has no unit"), "{err}");
        assert!(err.contains("max_latency: 12ms"), "{err}");

        // Absent under either name is None, not an error.
        assert!(
            yaml_duration(&load("other: 1"), "max_latency", "max_latency_ms")
                .unwrap()
                .is_none()
        );
    }

    /// The new spelling wins when both appear: an author who wrote it meant
    /// it, and preferring the old would make a migration a silent no-op.
    #[test]
    fn the_new_spelling_wins_over_the_deprecated_one() {
        use yaml_rust2::YamlLoader;
        let doc = YamlLoader::load_from_str("max_latency: 5ms\nmax_latency_ms: 99")
            .unwrap()
            .remove(0);
        let v = yaml_duration(&doc, "max_latency", "max_latency_ms")
            .unwrap()
            .unwrap();
        assert_eq!(v.as_millis_f64(), 5.0);
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
        max_response_ms: 100
"#;
        let m = parse_manifest_str(yaml).unwrap();
        let ndt = &m.nodes["ndt"];

        assert_eq!(ndt.subscribers["sensor_points"].min_rate_hz, Some(10.0));
        assert_eq!(ndt.subscribers["initial_pose"].required, Some(true));
        assert_eq!(ndt.subscribers["regularization_pose"].state, Some(true));
        assert_eq!(ndt.publishers["ndt_pose"].min_rate_hz, Some(10.0));
        assert!(ndt.publishers.contains_key("exe_time_ms"));
        assert_eq!(
            ndt.srv["trigger_node"]
                .max_response
                .map(|d| d.as_millis_f64()),
            Some(100.0)
        );
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
        max_latency_ms: 50
        drop:
          max_count: 10 / 100
          max_consecutive: 5
      debug:
        input: sensor_points
        output: [exe_time_ms]
"#;
        let m = parse_manifest_str(yaml).unwrap();
        let ndt = &m.nodes["ndt"];
        assert_eq!(ndt.paths.len(), 2);

        let loc = &ndt.paths["localization"];
        assert_eq!(loc.input, vec!["sensor_points"]);
        assert_eq!(loc.output, vec!["ndt_pose"]);
        assert_eq!(loc.max_latency.map(|d| d.as_millis_f64()), Some(50.0));
        let drop = loc.drop.as_ref().unwrap();
        assert_eq!(drop.max_count.as_ref().unwrap().n, 10);
        assert_eq!(drop.max_count.as_ref().unwrap().w, 100);
        assert_eq!(drop.max_consecutive, Some(5));

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
        assert_eq!(drop.max_count.as_ref().unwrap().n, 1);
        assert_eq!(drop.max_count.as_ref().unwrap().w, 100);

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
  lidar:
    manifest: tier4_perception_launch/lidar_perception.launch.yaml
  safety:
    nodes:
      emergency_stop:
        pub: [stop_cmd]
        sub: [diagnostics]
"#;
        let m = parse_manifest_str(yaml).unwrap();
        assert_eq!(m.includes.len(), 2);

        match &m.includes["lidar"] {
            IncludeDecl::External { manifest } => {
                assert_eq!(
                    manifest,
                    "tier4_perception_launch/lidar_perception.launch.yaml"
                );
            }
            _ => panic!("expected External"),
        }

        match &m.includes["safety"] {
            IncludeDecl::Inline(inner) => {
                assert!(inner.nodes.contains_key("emergency_stop"));
            }
            _ => panic!("expected Inline"),
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
    max_latency_ms: 50
"#;
        let m = parse_manifest_str(yaml).unwrap();

        let main = &m.paths["main"];
        assert_eq!(main.input, vec!["raw_data"]);
        assert_eq!(main.output, vec!["detections"]);
        assert_eq!(main.max_latency.map(|d| d.as_millis_f64()), Some(50.0));
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
        correlation: timestamp
        tolerance_ms: 50
        max_latency_ms: 20
"#;
        let m = parse_manifest_str(yaml).unwrap();
        let path = &m.nodes["fusion"].paths["fusion"];
        assert_eq!(path.input, vec!["lidar_objects", "camera_objects"]);
        assert_eq!(path.correlation.as_deref(), Some("timestamp"));
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
        max_transport_ms: 0
  remote_viz:
    sub:
      pointcloud:
        max_transport_ms: 10
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
        assert_eq!(
            perception_sub.max_transport.map(|d| d.as_millis_f64()),
            Some(0.0)
        );

        let remote_sub = &m.nodes["remote_viz"].subscribers["pointcloud"];
        assert!(remote_sub.qos.is_none());
        assert_eq!(
            remote_sub.max_transport.map(|d| d.as_millis_f64()),
            Some(10.0)
        );
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
        assert_eq!(drop.max_count.as_ref().unwrap().n, 5);
        assert_eq!(drop.max_count.as_ref().unwrap().w, 100);
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
        assert_eq!(boxes.max_jitter.unwrap().as_millis_f64(), 4.0);
        assert_eq!(boxes.min_latency.unwrap().as_millis_f64(), 6.0);

        let miss = boxes.miss.as_ref().expect("miss: parsed");
        let tol = miss.tolerate.as_ref().expect("tolerate: parsed");
        assert_eq!((tol.n, tol.w), (2, 100));
        assert_eq!(miss.consecutive, Some(1));
        assert_eq!(miss.action, Some(MissAction::SkipNext));

        let ex = &d.concurrency.as_ref().expect("concurrency: parsed").exclusive;
        assert_eq!(ex, &vec![vec!["to_boxes".to_string(), "to_masks".to_string()]]);

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
        assert!(err.contains("kill_it") && err.contains("skip_next"), "got: {err}");
    }

    /// `miss: "2 / 100"` is the same shorthand `drop:` accepts — same
    /// arithmetic, different event, so the author learns one spelling.
    #[test]
    fn miss_accepts_the_drop_shorthand() {
        let yaml = "version: 1\nnodes:\n  n:\n    paths:\n      p:\n        \
                    output: [o]\n        miss: 3 / 50\n";
        let m = parse_manifest_str(yaml).unwrap();
        let miss = m.nodes["n"].paths["p"].miss.as_ref().unwrap();
        let tol = miss.tolerate.as_ref().unwrap();
        assert_eq!((tol.n, tol.w), (3, 50));
        assert_eq!(miss.action, None, "shorthand states tolerance only");
    }

    /// ABSENT and EMPTY mean opposite things and must not be normalised
    /// together: no `concurrency:` means every path serializes (the
    /// conservative default), while `exclusive: []` says they may all run
    /// concurrently.
    #[test]
    fn absent_concurrency_differs_from_an_empty_exclusive_list() {
        let absent = parse_manifest_str(
            "version: 1\nnodes:\n  n:\n    paths:\n      p: { output: [o] }\n",
        )
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
}
