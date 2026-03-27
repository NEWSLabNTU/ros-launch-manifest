//! Core manifest types.

use serde::Serialize;
use std::collections::BTreeMap;

/// Top-level manifest.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Manifest {
    pub version: u32,
    /// Manifest arguments. Value = default, None = required (must be provided by caller).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub args: BTreeMap<String, Option<String>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exclude_patterns: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub global_topics: BTreeMap<String, GlobalTopicDecl>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub nodes: BTreeMap<String, NodeDecl>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub topics: BTreeMap<String, TopicDecl>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub services: BTreeMap<String, ServiceDecl>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub actions: BTreeMap<String, ActionDecl>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub includes: BTreeMap<String, IncludeDecl>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub imports: BTreeMap<String, Vec<String>>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub exports: BTreeMap<String, Vec<String>>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub paths: BTreeMap<String, PathDecl>,
}

/// Global topic declaration (type + optional QoS).
#[derive(Debug, Clone, Serialize)]
pub struct GlobalTopicDecl {
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qos: Option<QosDecl>,
}

/// Node declaration.
#[derive(Debug, Clone, Default, Serialize)]
pub struct NodeDecl {
    #[serde(rename = "if", skip_serializing_if = "Option::is_none")]
    pub if_condition: Option<String>,
    #[serde(rename = "unless", skip_serializing_if = "Option::is_none")]
    pub unless_condition: Option<String>,
    #[serde(rename = "pub", skip_serializing_if = "BTreeMap::is_empty")]
    pub publishers: BTreeMap<String, EndpointProps>,
    #[serde(rename = "sub", skip_serializing_if = "BTreeMap::is_empty")]
    pub subscribers: BTreeMap<String, EndpointProps>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub srv: BTreeMap<String, SrvEndpointProps>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub cli: BTreeMap<String, EndpointProps>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub paths: BTreeMap<String, PathDecl>,
}

/// Publisher/subscriber endpoint properties (all optional).
#[derive(Debug, Clone, Default, Serialize)]
pub struct EndpointProps {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_rate_hz: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_rate_hz: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jitter_ms: Option<f64>,
    /// Sub endpoint: read-latest, not causal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<bool>,
    /// Sub endpoint: must receive at least once before operational.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
}

/// Service endpoint properties.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SrvEndpointProps {
    /// Max time from request to response (runtime monitoring only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_response_ms: Option<f64>,
}

/// Topic declaration.
#[derive(Debug, Clone, Serialize)]
pub struct TopicDecl {
    #[serde(rename = "if", skip_serializing_if = "Option::is_none")]
    pub if_condition: Option<String>,
    #[serde(rename = "unless", skip_serializing_if = "Option::is_none")]
    pub unless_condition: Option<String>,
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(rename = "pub", skip_serializing_if = "Vec::is_empty")]
    pub publishers: Vec<String>,
    #[serde(rename = "sub", skip_serializing_if = "Vec::is_empty")]
    pub subscribers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qos: Option<QosDecl>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_hz: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drop: Option<DropSpec>,
}

/// Service declaration.
#[derive(Debug, Clone, Serialize)]
pub struct ServiceDecl {
    #[serde(rename = "if", skip_serializing_if = "Option::is_none")]
    pub if_condition: Option<String>,
    #[serde(rename = "unless", skip_serializing_if = "Option::is_none")]
    pub unless_condition: Option<String>,
    #[serde(rename = "type")]
    pub srv_type: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub server: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub client: Vec<String>,
}

/// Action declaration.
#[derive(Debug, Clone, Serialize)]
pub struct ActionDecl {
    #[serde(rename = "if", skip_serializing_if = "Option::is_none")]
    pub if_condition: Option<String>,
    #[serde(rename = "unless", skip_serializing_if = "Option::is_none")]
    pub unless_condition: Option<String>,
    #[serde(rename = "type")]
    pub action_type: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub server: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub client: Vec<String>,
}

/// Include declaration (external manifest or inline scope).
#[derive(Debug, Clone, Serialize)]
pub enum IncludeDecl {
    /// External: loaded from separate manifest file.
    External { manifest: String },
    /// Inline: embedded manifest (from <group> block).
    Inline(Box<Manifest>),
}

/// QoS declaration.
#[derive(Debug, Clone, Default, Serialize)]
pub struct QosDecl {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reliability: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub durability: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifespan_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liveliness: Option<String>,
}

/// Named causal path.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PathDecl {
    #[serde(rename = "if", skip_serializing_if = "Option::is_none")]
    pub if_condition: Option<String>,
    #[serde(rename = "unless", skip_serializing_if = "Option::is_none")]
    pub unless_condition: Option<String>,
    /// Single endpoint name or list of endpoint names (from sub).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub input: Vec<String>,
    /// List of endpoint names (from pub).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub output: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_latency_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_latency_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_age_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tolerance_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drop: Option<DropSpec>,
}

/// Drop tolerance specification.
#[derive(Debug, Clone, Serialize)]
pub struct DropSpec {
    /// "N / W" format: max N drops per W messages.
    pub max_count: Option<DropCount>,
    /// Max consecutive drops.
    pub max_consecutive: Option<u32>,
}

/// Drop count: N drops per W messages.
#[derive(Debug, Clone, Serialize)]
pub struct DropCount {
    pub n: u32,
    pub w: u32,
}

impl DropCount {
    /// Per-message drop probability.
    pub fn drop_rate(&self) -> f64 {
        if self.w == 0 {
            0.0
        } else {
            self.n as f64 / self.w as f64
        }
    }

    /// Delivery rate (1 - drop_rate).
    pub fn delivery_rate(&self) -> f64 {
        1.0 - self.drop_rate()
    }
}

impl std::fmt::Display for DropCount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} / {}", self.n, self.w)
    }
}

/// Parse "N / W" string into DropCount.
impl std::str::FromStr for DropCount {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split('/').map(|p| p.trim()).collect();
        if parts.len() != 2 {
            return Err(format!("expected 'N / W' format, got '{s}'"));
        }
        let n: u32 = parts[0]
            .parse()
            .map_err(|_| format!("invalid drop count: '{}'", parts[0]))?;
        let w: u32 = parts[1]
            .parse()
            .map_err(|_| format!("invalid window size: '{}'", parts[1]))?;
        if w == 0 {
            return Err("window size must be > 0".into());
        }
        Ok(DropCount { n, w })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drop_count_parse() {
        let dc: DropCount = "5 / 100".parse().unwrap();
        assert_eq!(dc.n, 5);
        assert_eq!(dc.w, 100);
        assert!((dc.drop_rate() - 0.05).abs() < 1e-9);
        assert!((dc.delivery_rate() - 0.95).abs() < 1e-9);
    }

    #[test]
    fn test_drop_count_display() {
        let dc = DropCount { n: 3, w: 50 };
        assert_eq!(dc.to_string(), "3 / 50");
    }

    #[test]
    fn test_drop_count_invalid() {
        assert!("abc".parse::<DropCount>().is_err());
        assert!("5".parse::<DropCount>().is_err());
        assert!("5 / 0".parse::<DropCount>().is_err());
    }

    #[test]
    fn test_endpoint_props_default() {
        let ep = EndpointProps::default();
        assert!(ep.min_rate_hz.is_none());
        assert!(ep.state.is_none());
        assert!(ep.required.is_none());
    }
}
