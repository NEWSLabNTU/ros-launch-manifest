//! YAML parser for manifest files using yaml-rust2.
//!
//! Parses YAML into typed [`Manifest`] AST. Handles both plain list
//! and map forms for endpoints (e.g., `pub: [a, b]` vs `pub: {a: {min_rate_hz: 10}}`).

use crate::{
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
    reject_chains(doc, ctx)?;
    reject_unknown_keys(doc, ctx, Context::Manifest)?;
    Ok(Manifest {
        version: yaml_u32(doc, "version", ctx)?.unwrap_or(1),
        args: parse_args(doc, ctx)?,
        nodes: parse_nodes(doc, ctx)?,
        topics: parse_topics(doc, ctx)?,
        services: parse_services(doc, ctx)?,
        actions: parse_actions(doc, ctx)?,
        includes: parse_includes(doc, ctx)?,
        paths: parse_paths(doc, ctx)?,
        external_topics: parse_external_topics(doc, ctx)?,
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
    reject_unknown_keys(yaml, ctx, Context::Node)?;
    Ok(NodeDecl {
        if_condition: yaml_string(yaml, "if", ctx)?,
        unless_condition: yaml_string(yaml, "unless", ctx)?,
        lifecycle: yaml_bool(yaml, "lifecycle", ctx)?,
        publishers: parse_endpoints(yaml, "pub", ctx)?,
        subscribers: parse_endpoints(yaml, "sub", ctx)?,
        srv: parse_srv_endpoints(yaml, "srv", ctx)?,
        cli: parse_endpoints(yaml, "cli", ctx)?,
        paths: parse_paths(yaml, ctx)?,
        criticality: yaml_string(yaml, "criticality", ctx)?,
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
        min_rate_hz: yaml_f64(yaml, "min_rate_hz", ctx)?,
        max_rate_hz: yaml_f64(yaml, "max_rate_hz", ctx)?,
        max_age: yaml_duration(yaml, "max_age", "max_age_ms")?,
        state: yaml_bool(yaml, "state", ctx)?,
        required: yaml_bool(yaml, "required", ctx)?,
        qos: parse_qos(yaml, ctx)?,
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
                eps.insert(name, SrvEndpointProps::default());
            }
        }
        Yaml::Hash(hash) => {
            for (k, v) in hash {
                let name = yaml_str_owned(k);
                reject_unknown_keys(v, ctx, Context::SrvEndpoint)?;
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
        if let Some(manifest_path) = yaml_string(v, "manifest", ctx)? {
            reject_unknown_keys(v, &path, Context::IncludeExternal)?;
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
        max_latency: yaml_duration(yaml, "max_latency", "max_latency_ms")?,
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
            reject_unknown_keys(v, ctx, Context::TriggerTimer)?;
            let rate_hz = yaml_f64(v, "rate_hz", ctx)?
                .ok_or_else(|| field_err(ctx, "trigger.timer", "requires 'rate_hz'"))?;
            if rate_hz <= 0.0 {
                return Err(field_err(ctx, "trigger.timer.rate_hz", "must be > 0"));
            }
            Ok(Some(Trigger::Timer { rate_hz }))
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
    let max_interval = yaml_duration(section, "max_interval", "max_interval_ms")?;
    let timeout = yaml_duration(section, "timeout", "timeout_ms")?;
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
                let detail = match field_table::nearest(context, key) {
                    Some(near) => format!("did you mean `{near}`?"),
                    None => format!(
                        "accepted here: {}",
                        field_table::suggestions(context).join(", ")
                    ),
                };
                return Err(field_err(
                    ctx,
                    key,
                    &format!(
                        "unknown key in `{}` — {detail}. An unrecognised key used to be \
                         discarded in silence, which deleted whatever it was meant to \
                         declare",
                        context.label()
                    ),
                ));
            }
        }
    }
    Ok(())
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
            max_count: Some(count),
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
        .transpose()?;
    let max_consecutive = yaml_u32(section, "max_consecutive", ctx)?;

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
        lifespan: yaml_duration(section, "lifespan", "lifespan_ms")?,
        liveliness: yaml_string(section, "liveliness", ctx)?,
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
        _ => from_legacy_scalar(None, yaml_f64(doc, legacy, "")?, LegacyUnit::Millis)
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
    // Nested block: diagnostics name the block, not its parent.
    let ctx = &format_path(ctx, "miss");
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
    reject_unknown_keys(section, ctx, Context::Miss)?;
    let tolerate = yaml_string(section, "tolerate", ctx)?
        .map(|s| {
            s.parse::<DropCount>()
                .map_err(|e| field_err(ctx, "tolerate", &e.to_string()))
        })
        .transpose()?;
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
        consecutive: yaml_u32(section, "consecutive", ctx)?,
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
                "nodes:\n  n:\n    paths:\n      p:\n        output: [o]\n        drop: { max_count: 5 }\n",
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
            "nodes:\n  filter:\n    paths:\n      clean:\n        output: [o]\n        drop: { max_count: 5 }\n",
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("nodes.filter.paths.clean.drop.max_count"),
            "got: {err}"
        );
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
        tolerance_ms: 50
        max_latency_ms: 20
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
}
