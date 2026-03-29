//! YAML parser for manifest files using yaml-rust2.
//!
//! Parses YAML into typed [`Manifest`] AST. Handles both plain list
//! and map forms for endpoints (e.g., `pub: [a, b]` vs `pub: {a: {min_rate_hz: 10}}`).

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
        global_topics: parse_global_topics(doc)?,
        nodes: parse_nodes(doc, ctx)?,
        topics: parse_topics(doc, ctx)?,
        services: parse_services(doc, ctx)?,
        actions: parse_actions(doc, ctx)?,
        includes: parse_includes(doc, ctx)?,
        scope_pub: parse_groups(doc, "pub"),
        scope_sub: parse_groups(doc, "sub"),
        scope_srv: parse_groups(doc, "srv"),
        scope_cli: parse_groups(doc, "cli"),
        action_server: parse_groups(doc, "action_server"),
        action_client: parse_groups(doc, "action_client"),
        paths: parse_paths(doc, ctx)?,
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
        publishers: parse_endpoints(yaml, "pub", ctx)?,
        subscribers: parse_endpoints(yaml, "sub", ctx)?,
        srv: parse_srv_endpoints(yaml, "srv", ctx)?,
        cli: parse_endpoints(yaml, "cli", ctx)?,
        paths: parse_paths(yaml, ctx)?,
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

fn parse_endpoint_props(yaml: &Yaml, _ctx: &str) -> Result<EndpointProps, ParseError> {
    if yaml.is_null() || yaml.is_badvalue() {
        return Ok(EndpointProps::default());
    }
    Ok(EndpointProps {
        min_rate_hz: yaml_f64(yaml, "min_rate_hz"),
        max_rate_hz: yaml_f64(yaml, "max_rate_hz"),
        jitter_ms: yaml_f64(yaml, "jitter_ms"),
        state: yaml_bool(yaml, "state"),
        required: yaml_bool(yaml, "required"),
    })
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
                    max_response_ms: if v.is_null() || v.is_badvalue() {
                        None
                    } else {
                        yaml_f64(v, "max_response_ms")
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
            drop: None,
        });
    }
    Ok(TopicDecl {
        if_condition: yaml_string(yaml, "if"),
        unless_condition: yaml_string(yaml, "unless"),
        msg_type: yaml_string(yaml, "type")
            .ok_or_else(|| field_err(ctx, "type", "topic must have a type"))?,
        publishers: yaml_string_list(yaml, "pub"),
        subscribers: yaml_string_list(yaml, "sub"),
        qos: parse_qos(yaml),
        rate_hz: yaml_f64(yaml, "rate_hz"),
        drop: parse_drop_spec(yaml, "drop", ctx)?,
    })
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
/// Accepts both list form `args: [a, b]` and map form `args: { a:, b: }`.
fn parse_args(doc: &Yaml) -> Vec<String> {
    let section = &doc["args"];
    if section.is_badvalue() {
        return vec![];
    }
    match section {
        Yaml::Array(arr) => arr.iter().map(yaml_str_owned).collect(),
        Yaml::Hash(hash) => hash.keys().map(yaml_str_owned).collect(),
        _ => vec![],
    }
}

// ── Global Topics ──

fn parse_global_topics(doc: &Yaml) -> Result<BTreeMap<String, GlobalTopicDecl>, ParseError> {
    let mut out = BTreeMap::new();
    let section = &doc["global_topics"];
    if section.is_badvalue() {
        return Ok(out);
    }
    if let Some(hash) = section.as_hash() {
        for (k, v) in hash {
            let name = yaml_str_owned(k);
            out.insert(
                name,
                GlobalTopicDecl {
                    msg_type: yaml_string(v, "type").unwrap_or_default(),
                    qos: parse_qos(v),
                },
            );
        }
    }
    Ok(out)
}

// ── Import/Export Groups ──

fn parse_groups(doc: &Yaml, key: &str) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    let section = &doc[key];
    if section.is_badvalue() {
        return out;
    }
    if let Some(hash) = section.as_hash() {
        for (k, v) in hash {
            let name = yaml_str_owned(k);
            let members = match v {
                Yaml::Array(arr) => arr.iter().map(yaml_str_owned).collect(),
                _ => vec![],
            };
            out.insert(name, members);
        }
    }
    out
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
    Ok(PathDecl {
        if_condition: yaml_string(yaml, "if"),
        unless_condition: yaml_string(yaml, "unless"),
        input: parse_string_or_list(yaml, "input"),
        output: yaml_string_list(yaml, "output"),
        max_latency_ms: yaml_f64(yaml, "max_latency_ms"),
        min_latency_ms: yaml_f64(yaml, "min_latency_ms"),
        max_age_ms: yaml_f64(yaml, "max_age_ms"),
        correlation: yaml_string(yaml, "correlation"),
        tolerance_ms: yaml_f64(yaml, "tolerance_ms"),
        drop: parse_drop_spec(yaml, "drop", ctx)?,
    })
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

fn parse_qos(doc: &Yaml) -> Option<QosDecl> {
    let section = &doc["qos"];
    if section.is_badvalue() {
        return None;
    }
    Some(QosDecl {
        reliability: yaml_string(section, "reliability"),
        durability: yaml_string(section, "durability"),
        depth: yaml_u32(section, "depth"),
        history: yaml_string(section, "history"),
        lifespan_ms: yaml_u64(section, "lifespan_ms"),
        liveliness: yaml_string(section, "liveliness"),
    })
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

fn yaml_u64(doc: &Yaml, key: &str) -> Option<u64> {
    doc[key].as_i64().map(|i| i as u64)
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
    match &doc[key] {
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
pub:
  output: [talker/chatter]
"#;
        let m = parse_manifest_str(yaml).unwrap();
        assert_eq!(m.version, 1);
        assert_eq!(m.nodes.len(), 2);
        assert_eq!(m.topics.len(), 1);
        assert_eq!(m.scope_pub.len(), 1);

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
        assert_eq!(ndt.srv["trigger_node"].max_response_ms, Some(100.0));
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
        min_latency_ms: 10
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
        assert_eq!(loc.max_latency_ms, Some(50.0));
        assert_eq!(loc.min_latency_ms, Some(10.0));
        let drop = loc.drop.as_ref().unwrap();
        assert_eq!(drop.max_count.as_ref().unwrap().n, 10);
        assert_eq!(drop.max_count.as_ref().unwrap().w, 100);
        assert_eq!(drop.max_consecutive, Some(5));

        let debug = &ndt.paths["debug"];
        assert_eq!(debug.input, vec!["sensor_points"]);
        assert!(debug.max_latency_ms.is_none());
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
    pub:
      commands: [emergency_stop/stop_cmd]
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
                assert!(inner.scope_pub.contains_key("commands"));
            }
            _ => panic!("expected Inline"),
        }
    }

    #[test]
    fn test_scope_paths() {
        let yaml = r#"
version: 1
sub:
  raw_data: [cropbox_filter/input]
pub:
  detections: [centerpoint/objects]
paths:
  main:
    input: raw_data
    output: [detections]
    max_latency_ms: 50
    max_age_ms: 120
"#;
        let m = parse_manifest_str(yaml).unwrap();
        assert_eq!(m.scope_sub["raw_data"], vec!["cropbox_filter/input"]);
        assert_eq!(m.scope_pub["detections"], vec!["centerpoint/objects"]);

        let main = &m.paths["main"];
        assert_eq!(main.input, vec!["raw_data"]);
        assert_eq!(main.output, vec!["detections"]);
        assert_eq!(main.max_latency_ms, Some(50.0));
        assert_eq!(main.max_age_ms, Some(120.0));
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
        assert_eq!(path.tolerance_ms, Some(50.0));
    }

    #[test]
    fn test_global_topics() {
        let yaml = r#"
version: 1
global_topics:
  /tf:
    type: tf2_msgs/msg/TFMessage
    qos:
      reliability: reliable
      depth: 100
  /clock:
    type: rosgraph_msgs/msg/Clock
"#;
        let m = parse_manifest_str(yaml).unwrap();
        assert_eq!(m.global_topics.len(), 2);
        let tf = &m.global_topics["/tf"];
        assert_eq!(tf.msg_type, "tf2_msgs/msg/TFMessage");
        assert_eq!(tf.qos.as_ref().unwrap().depth, Some(100));
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
}
