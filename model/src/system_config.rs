//! R1-P1 — the integrator's system config (`system.toml`), the format this
//! crate owns for the shared layers (`docs/model-boundary.md`). A resolver
//! ingests it and fills the model's execution layer; realizers read the
//! model, not this file.
//!
//! This is a LENIENT reader: it understands the shared sections (`[system]`
//! defaults, `[host.<name>]`, `[tiers.*]`, `[[component]]`, `[[transport]]`,
//! `[[bridge]]`) and ignores every other table by design. A realizer extends
//! the same file with its own tables and validates those itself.
//!
//! Placement: `[host.<name>].nodes = ["/fqn", …]` lists the node FQNs that
//! machine runs. A single host with no `nodes` list means "every node";
//! multiple hosts require explicit lists (ambiguity is an error — fail-loud).
//! With no host in scope, nothing is placed and every node runs everywhere
//! the model is realized.

use indexmap::IndexMap;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Bridge, Deploy, Execution, NodeInstance, ParamSource, ParamValue, Transport};

#[derive(Debug, Default, Deserialize)]
pub struct SystemConfigToml {
    #[serde(default)]
    pub system: SystemDefaults,
    /// `[host.<name>]` — a MACHINE, and the only thing that places nodes.
    ///
    /// A block is always a machine, so no `kind` is needed and none is
    /// accepted. What a node is BUILT as is a realizer's own table, never
    /// this one.
    #[serde(default)]
    pub host: BTreeMap<String, HostBlock>,
    #[serde(default, rename = "transport")]
    pub transports: Vec<TransportBlock>,
    #[serde(default, rename = "bridge")]
    pub bridges: Vec<BridgeBlock>,
    /// `[tiers.<name>]` scheduling tiers. The block is the shared-schema
    /// `sched::TierDef` verbatim — `spin_period_us` + per-RTOS sub-tables —
    /// so it deserializes straight into `execution.tiers`. (A system may
    /// instead supply tiers through a scheduling platform file; one that
    /// authors them inline here has them consumed by the resolver.)
    #[serde(default)]
    pub tiers: BTreeMap<String, ros_launch_manifest_sched::TierDef>,
    /// `[[component]]` rows — carry `group_tiers` (the
    /// group→tier bindings) keyed by the component/node `name`.
    #[serde(default, rename = "component")]
    pub components: Vec<ComponentBlock>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ComponentBlock {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub class: Option<String>,
    #[serde(default)]
    pub pkg: Option<String>,
    /// `group_tiers = { <group> = <tier> }` — RFC-0047 group→tier binding.
    #[serde(default)]
    pub group_tiers: BTreeMap<String, String>,
    /// `params = { <name> = <value> }` — deployment-time parameter values for
    /// this component's node, equivalent to an inline `<param>` in the launch
    /// file.
    ///
    /// Some parameters are a property of the DEPLOYMENT rather than of the
    /// launch description — a `qos_overrides.…` entry chosen per system is the
    /// motivating case. Before this existed the only
    /// way to express one was to type it into the resolved model by hand, so
    /// re-resolving silently dropped it: the model was both the artifact and
    /// the only record of the intent.
    #[serde(default)]
    pub params: BTreeMap<String, toml::Value>,
    /// `params_files = ["<yaml content>", …]` — parameter FILE contents for
    /// this component's node, same shape as `NodeInstance::params_files` and
    /// the `<param from=…>` launch form. Applied in order, before `params`.
    #[serde(default)]
    pub params_files: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct SystemDefaults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rmw: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,
}

/// `[host.<name>]` — one machine in a multi-host system.
///
/// Deliberately tiny: a machine is identified by its name, says which nodes
/// it runs, and optionally scopes itself to one launch file. Board, target,
/// profile and the like describe a BUILD and belong to a realizer's own table.
///
/// `deny_unknown_fields` because a mistyped key here silently unplaces a node,
/// which surfaces far away as a realizer that runs no nodes on that machine.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostBlock {
    /// Node FQNs this machine runs. Empty means "every node", which is only
    /// meaningful when this is the sole host.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<String>,
    /// Launch file this block applies to, relative to the bringup package.
    /// `None` means every launch file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch: Option<String>,
    /// Session domain, if this machine overrides the system default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain_id: Option<u32>,
    /// Session locator, if this machine overrides the system default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,
}

impl HostBlock {
    /// Does this block govern `launch_file`? An unscoped block governs
    /// everything, a scoped one compares on basename, and a caller asking
    /// about no particular file keeps every block.
    #[must_use]
    pub fn applies_to_launch(&self, launch_file: Option<&str>) -> bool {
        match (&self.launch, launch_file) {
            (None, _) => true,
            (Some(_), None) => true,
            (Some(mine), Some(theirs)) => {
                let base = |p: &str| p.rsplit('/').next().unwrap_or(p).to_string();
                base(mine) == base(theirs)
            }
        }
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct TransportBlock {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub ip: Option<String>,
    #[serde(default)]
    pub mac: Option<String>,
    #[serde(default)]
    pub gateway: Option<String>,
    #[serde(default)]
    pub interfaces: Vec<String>,
    #[serde(default)]
    pub ssid: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub baudrate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rmw: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,
    #[serde(default)]
    pub domain: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
pub struct BridgeBlock {
    pub name: String,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub topics: Vec<String>,
    #[serde(default)]
    pub bidirectional: bool,
}

/// Parse the integrator `system.toml` (lenient subset).
pub fn parse_system_config(input: &str) -> Result<SystemConfigToml, String> {
    toml::from_str(input).map_err(|e| e.to_string())
}

fn clamp_domain(d: Option<u32>, diags: &mut Vec<String>, ctx: &str) -> Option<u8> {
    match d {
        None => None,
        Some(v) if v <= 232 => Some(v as u8),
        Some(v) => {
            diags.push(format!(
                "system config: {ctx}: domain_id {v} out of ROS range 0..=232 — ignored"
            ));
            None
        }
    }
}

impl SystemConfigToml {
    /// Fill an [`Execution`] layer from this config for the given node
    /// FQNs. Returns human-readable diagnostics (unknown placement nodes,
    /// clamped domains); ambiguity (multiple hosts, none listing a node) is
    /// an `Err` — fail-loud, never silent partial placement.
    /// Project `[[component]] params` / `params_files` onto the STRUCTURE
    /// layer.
    ///
    /// `apply_to` only reaches `Execution`; parameters are a per-node property
    /// and live in `structure.nodes`, so this is a separate entry point rather
    /// than a widening of that signature.
    ///
    /// Components match nodes by BARE NAME, the same rule `group_tiers` uses,
    /// and an unmatched component is a diagnostic rather than an error — a
    /// conditional node may legitimately be absent from this variant.
    ///
    /// Precedence follows ROS and the launch path: `params_files` apply in
    /// order, then inline `params` win. Values already present on the node
    /// (from the launch file) are NOT overwritten — the launch description is
    /// the more specific statement.
    pub fn apply_params_to_nodes(&self, nodes: &mut IndexMap<String, NodeInstance>) -> Vec<String> {
        let mut diags = Vec::new();
        for c in &self.components {
            let Some(name) = &c.name else { continue };
            if c.params.is_empty() && c.params_files.is_empty() {
                continue;
            }
            let fqn = nodes
                .keys()
                .find(|f| f.rsplit('/').next().unwrap_or(f) == name.as_str())
                .cloned();
            // Fall back to the component's PACKAGE when the instance name does
            // not match a launch node name. A consolidated workspace gives
            // `[[component]] name` a workspace-unique spelling
            // (`rust_params_param_talker`) while the launch file keeps the plain
            // node name (`param_talker`), so bare-name matching silently finds
            // nothing — every per-node projection then no-ops without saying so.
            // Only an UNAMBIGUOUS pkg counts: two instances of one package are
            // exactly the case the instance name exists to tell apart.
            let fqn = fqn.or_else(|| {
                let pkg = c.pkg.as_deref()?;
                let mut it = nodes
                    .iter()
                    .filter(|(_, inst)| inst.pkg.as_deref() == Some(pkg))
                    .map(|(f, _)| f.clone());
                let first = it.next()?;
                it.next().is_none().then_some(first)
            });
            let Some(fqn) = fqn else {
                diags.push(format!(
                    "system config: [[component]] '{name}' declares params but has no \
                     matching launch node (absent in this variant?)"
                ));
                continue;
            };
            let Some(inst) = nodes.get_mut(&fqn) else {
                continue;
            };
            for content in &c.params_files {
                if !inst.params_files.iter().any(|f| f == content) {
                    inst.params_files.push(content.clone());
                    inst.param_sources.push(ParamSource::File {
                        content: content.clone(),
                    });
                }
            }
            for (k, v) in &c.params {
                let Some(pv) = toml_to_param_value(v) else {
                    diags.push(format!(
                        "system config: [[component]] '{name}' param '{k}' has an \
                         unsupported type; expected bool, integer, float, string or \
                         string list"
                    ));
                    continue;
                };
                // The launch file is more specific — do not overwrite it.
                if inst.params.contains_key(k) {
                    continue;
                }
                inst.params.insert(k.clone(), pv.clone());
                inst.param_sources.push(ParamSource::Inline {
                    name: k.clone(),
                    value: pv,
                });
            }
        }
        diags
    }

    pub fn apply_to(
        &self,
        execution: &mut Execution,
        node_fqns: &[&str],
    ) -> Result<Vec<String>, String> {
        self.apply_to_launch(execution, node_fqns, None)
    }

    /// [`Self::apply_to`], told WHICH launch file is being resolved.
    ///
    /// Hosts scoped with `launch = "…"` are filtered out when they name a
    /// different file. Without this, a bringup with one unscoped host plus two
    /// scoped ones looked like "three hosts" to every launch file, and the
    /// multi-host rule below demanded an explicit `nodes = [..]` for nodes the
    /// scoped hosts never governed — a hard error on a config that is
    /// correct.
    pub fn apply_to_launch(
        &self,
        execution: &mut Execution,
        node_fqns: &[&str],
        launch_file: Option<&str>,
    ) -> Result<Vec<String>, String> {
        let mut diags = Vec::new();

        // `[tiers.*]` → the execution tier table (verbatim schema).
        execution.tiers = self.tiers.clone();

        // `[[component]].group_tiers` → `execution.bindings`
        // (`<node FQN>/<group>` → tier). Map the component `name` to its
        // resolved node FQN (bare-name match against the launch nodes); an
        // unmatched component is a diagnostic, not fatal (conditional nodes
        // may be absent in this variant).
        for c in &self.components {
            let Some(name) = &c.name else { continue };
            if c.group_tiers.is_empty() {
                continue;
            }
            let fqn = node_fqns
                .iter()
                .find(|f| f.rsplit('/').next().unwrap_or(f) == name.as_str());
            match fqn {
                Some(fqn) => {
                    for (group, tier) in &c.group_tiers {
                        execution
                            .bindings
                            .insert(format!("{fqn}/{group}"), tier.clone());
                    }
                }
                None => diags.push(format!(
                    "system config: [[component]] '{name}' has group_tiers but no \
                     matching launch node (absent in this variant?)"
                )),
            }
        }

        for t in &self.transports {
            execution.transports.push(Transport {
                kind: t.kind.clone().unwrap_or_else(|| "ethernet".to_string()),
                id: t.id.clone(),
                ip: t.ip.clone(),
                mac: t.mac.clone(),
                gateway: t.gateway.clone(),
                interfaces: t.interfaces.clone(),
                ssid: t.ssid.clone(),
                password: t.password.clone(),
                device: t.device.clone(),
                baudrate: t.baudrate,
                rmw: t.rmw.clone(),
                locator: t.locator.clone(),
                domain: clamp_domain(t.domain, &mut diags, "transport"),
            });
        }
        for b in &self.bridges {
            execution.bridges.push(Bridge {
                name: b.name.clone(),
                from: b.from.clone(),
                to: b.to.clone(),
                topics: b.topics.clone(),
                bidirectional: b.bidirectional,
            });
        }

        // `[host.*]` placement, for the launch files a host GOVERNS. A launch
        // file no host governs places nothing: the map stays empty and every
        // node runs wherever the model is realized.
        //
        // A host says WHERE a node runs. What it is built as comes from a
        // realizer's own build table, so placement carries no board.
        let hosts_in_scope: Vec<(&String, &HostBlock)> = self
            .host
            .iter()
            .filter(|(_, b)| b.applies_to_launch(launch_file))
            .collect();
        if !hosts_in_scope.is_empty() {
            let in_scope = hosts_in_scope;
            let single = (in_scope.len() == 1).then(|| in_scope[0].0);
            for fqn in node_fqns {
                let (hname, block) = if let Some(k) = single {
                    let b = &self.host[k];
                    if !b.nodes.is_empty() && !b.nodes.iter().any(|n| n == fqn) {
                        continue; // explicitly not placed
                    }
                    (k.as_str(), b)
                } else {
                    match in_scope
                        .iter()
                        .find(|(_, b)| b.nodes.iter().any(|n| n == fqn))
                        .map(|(k, b)| (k.as_str(), *b))
                    {
                        Some((k, b)) => (k, b),
                        None => {
                            return Err(format!(
                                "system config: node '{fqn}' is not placed — with multiple \
                                 [host.*] blocks every node needs a `nodes = [..]` entry"
                            ));
                        }
                    }
                };
                // The host's NAME is not written: nothing reads it, and the
                // facts placement changes are the typed fields below.
                execution.deploy.insert(
                    (*fqn).to_string(),
                    Deploy {
                        domain: clamp_domain(block.domain_id, &mut diags, hname).or_else(|| {
                            clamp_domain(self.system.domain_id, &mut diags, "[system]")
                        }),
                        locator: block
                            .locator
                            .clone()
                            .or_else(|| self.system.locator.clone()),
                        rmw: self.system.rmw.clone(),
                    },
                );
            }
        }
        Ok(diags)
    }
}

/// TOML scalar → [`ParamValue`]. Returns `None` for shapes the model has no
/// representation for (tables, mixed arrays), which the caller reports as a
/// diagnostic rather than dropping silently.
fn toml_to_param_value(v: &toml::Value) -> Option<ParamValue> {
    Some(match v {
        toml::Value::Boolean(b) => ParamValue::Bool(*b),
        toml::Value::Integer(i) => ParamValue::Int(*i),
        toml::Value::Float(f) => ParamValue::Float(*f),
        toml::Value::String(s) => ParamValue::Str(s.clone()),
        toml::Value::Array(a) => {
            let mut out = Vec::with_capacity(a.len());
            for e in a {
                out.push(e.as_str()?.to_string());
            }
            ParamValue::StrList(out)
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOML: &str = r#"
[system]
rmw = "zenoh"
domain_id = 7
features = ["safety"]

[host.native]
nodes = ["/ctrl/control_node"]

[host.imu]
domain_id = 9
nodes = ["/sensing/imu_node"]

[[transport]]
kind = "ethernet"
id = "eth0"
ip = "10.0.2.50/24"
mac = "02:00:00:00:00:01"
domain = 7

[[bridge]]
name = "uplink"
from = "eth0"
to = "can0"
topics = ["/perception/objects"]
"#;

    #[test]
    fn reads_tiers_lifecycle_and_group_tier_bindings() {
        // A system.toml with inline [tiers.*], [[component]] group_tiers and
        // a `[lifecycle]` table this crate does not declare (ignored).
        let toml = r#"
[system]
rmw = "zenoh"
[lifecycle]
autostart = "active"
[[component]]
pkg = "ctrl_pkg"
class = "ctrl_pkg::Ctrl"
name = "control_node"
group_tiers = { ctrl = "high" }
[[component]]
pkg = "telem_pkg"
class = "telem_pkg::Telem"
name = "telem_node"
group_tiers = { telem = "low" }
[tiers.high]
spin_period_us = 1000
[tiers.high.posix]
priority = 80
[tiers.high.zephyr]
priority = 5
[tiers.low]
spin_period_us = 10000
[tiers.low.posix]
priority = 10
"#;
        let cfg = parse_system_config(toml).expect("parses");
        let mut e = Execution::default();
        let diags = cfg
            .apply_to(&mut e, &["/control_node", "/telem_node"])
            .expect("applies");
        assert!(diags.is_empty(), "{diags:?}");
        // tiers rode in verbatim.
        assert_eq!(e.tiers.len(), 2);
        assert_eq!(e.tiers["high"].spin_period.unwrap().as_micros(), 1_000);
        assert_eq!(e.tiers["high"].posix.as_ref().unwrap().priority, 80);
        assert_eq!(e.tiers["high"].zephyr.as_ref().unwrap().priority, 5);
        // group_tiers → bindings keyed by FQN.
        assert_eq!(e.bindings["/control_node/ctrl"], "high");
        assert_eq!(e.bindings["/telem_node/telem"], "low");
    }

    #[test]
    fn fills_execution_with_ladder_and_placement() {
        let cfg = parse_system_config(TOML).expect("parses");
        let mut e = Execution::default();
        let diags = cfg
            .apply_to(&mut e, &["/ctrl/control_node", "/sensing/imu_node"])
            .expect("applies");
        assert!(diags.is_empty(), "{diags:?}");
        // Placement and the session ladder reach the model.
        let ctrl = &e.deploy["/ctrl/control_node"];
        assert_eq!(ctrl.domain, Some(7), "system default rides the ladder");
        assert_eq!(ctrl.rmw.as_deref(), Some("zenoh"));
        let imu = &e.deploy["/sensing/imu_node"];
        assert_eq!(imu.domain, Some(9), "host override wins");
        assert_eq!(imu.rmw.as_deref(), Some("zenoh"));
        assert_eq!(e.transports[0].mac.as_deref(), Some("02:00:00:00:00:01"));
        assert_eq!(e.bridges[0].name, "uplink");
    }

    #[test]
    fn unplaced_node_with_multiple_blocks_fails_loud() {
        let cfg = parse_system_config(TOML).expect("parses");
        let mut e = Execution::default();
        let err = cfg
            .apply_to(&mut e, &["/ctrl/control_node", "/ghost"])
            .unwrap_err();
        assert!(err.contains("'/ghost' is not placed"), "{err}");
    }

    #[test]
    fn component_params_and_files_project_onto_the_node() {
        let cfg = parse_system_config(
            "[system]\nrmw = \"zenoh\"\n\
             [[component]]\nname = \"talker\"\n\
             params = { qos = \"best_effort\", rate = 10 }\n\
             params_files = [\"talker:\\n  ros__parameters:\\n    x: 1\\n\"]\n",
        )
        .expect("parses");
        let mut nodes = IndexMap::new();
        nodes.insert("/talker".to_string(), NodeInstance::default());
        let diags = cfg.apply_params_to_nodes(&mut nodes);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let n = &nodes["/talker"];
        assert_eq!(
            n.params.get("qos"),
            Some(&ParamValue::Str("best_effort".into()))
        );
        assert_eq!(n.params.get("rate"), Some(&ParamValue::Int(10)));
        assert_eq!(n.params_files.len(), 1);
        // one File source + two Inline sources
        assert_eq!(n.param_sources.len(), 3);
    }

    #[test]
    fn component_matches_by_pkg_when_the_instance_name_differs() {
        // A consolidated workspace renames `[[component]] name` for uniqueness
        // while the launch file keeps the plain node name. Bare-name matching
        // finds nothing; the pkg is what still ties them together.
        let cfg = parse_system_config(
            "[system]\nrmw = \"zenoh\"\n\
             [[component]]\npkg = \"rust_param_talker_pkg\"\n\
             name = \"rust_params_param_talker\"\nparams = { rate = 7 }\n",
        )
        .expect("parses");
        let inst = NodeInstance {
            pkg: Some("rust_param_talker_pkg".to_string()),
            ..Default::default()
        };
        let mut nodes = IndexMap::new();
        nodes.insert("/param_talker".to_string(), inst);
        let diags = cfg.apply_params_to_nodes(&mut nodes);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(
            nodes["/param_talker"].params.get("rate"),
            Some(&ParamValue::Int(7))
        );
    }

    #[test]
    fn an_ambiguous_pkg_does_not_match() {
        // Two instances of one package are exactly what the instance name is
        // for — guessing between them would be worse than the diagnostic.
        let cfg = parse_system_config(
            "[system]\nrmw = \"zenoh\"\n\
             [[component]]\npkg = \"talker_pkg\"\nname = \"ghost\"\nparams = { a = 1 }\n",
        )
        .expect("parses");
        let mut nodes = IndexMap::new();
        for fqn in ["/talker_one", "/talker_two"] {
            let i = NodeInstance {
                pkg: Some("talker_pkg".to_string()),
                ..Default::default()
            };
            nodes.insert(fqn.to_string(), i);
        }
        let diags = cfg.apply_params_to_nodes(&mut nodes);
        assert_eq!(
            diags.len(),
            1,
            "expected the unmatched diagnostic: {diags:?}"
        );
        assert!(nodes["/talker_one"].params.is_empty());
        assert!(nodes["/talker_two"].params.is_empty());
    }

    #[test]
    fn launch_params_win_over_component_params() {
        let cfg = parse_system_config(
            "[system]\nrmw = \"zenoh\"\n\
             [[component]]\nname = \"talker\"\nparams = { rate = 10 }\n",
        )
        .expect("parses");
        let mut inst = NodeInstance::default();
        inst.params.insert("rate".into(), ParamValue::Int(99));
        let mut nodes = IndexMap::new();
        nodes.insert("/talker".to_string(), inst);
        cfg.apply_params_to_nodes(&mut nodes);
        // The launch description is the more specific statement.
        assert_eq!(
            nodes["/talker"].params.get("rate"),
            Some(&ParamValue::Int(99))
        );
    }

    #[test]
    fn unmatched_component_with_params_is_a_diagnostic_not_a_panic() {
        let cfg = parse_system_config(
            "[system]\nrmw = \"zenoh\"\n\
             [[component]]\nname = \"ghost\"\nparams = { a = 1 }\n",
        )
        .expect("parses");
        let mut nodes = IndexMap::new();
        nodes.insert("/talker".to_string(), NodeInstance::default());
        let diags = cfg.apply_params_to_nodes(&mut nodes);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].contains("ghost"), "{diags:?}");
    }

    /// Tables this crate does not declare are ignored by design
    /// (`docs/model-boundary.md`): a realizer extends the same file with its
    /// own and validates them itself. `[deploy.*]`, `[lifecycle]`,
    /// `[param_services]` and `[system] features` were declared here once and
    /// are ignored like any other since v0.1.57 — they place nothing and
    /// reach no field of the model.
    #[test]
    fn tables_this_crate_does_not_declare_reach_nothing() {
        let cfg = parse_system_config(
            "[system]\nrmw = \"zenoh\"\nfeatures = [\"param_services\"]\n\
             [param_services]\n[lifecycle]\nautostart = \"active\"\n\
             [deploy.native]\nkind = \"self\"\nnodes = [\"/a\"]\ndomain_id = 4\n\
             [deploy.native.nros]\nnetstack = \"lwip\"\n\
             [image.fw]\nboard = \"x\"\n",
        )
        .expect("an undeclared table is not an error");
        let mut e = Execution::default();
        cfg.apply_to(&mut e, &["/a", "/b"]).expect("applies");
        assert!(e.deploy.is_empty(), "no host, no placement: {:?}", e.deploy);
        let yaml = serde_yaml_ng::to_string(&e).unwrap();
        assert!(!yaml.contains("features"), "{yaml}");
    }

    /// With no host in scope for a launch file, nothing is placed: the map is
    /// empty and every node runs wherever the model is realized.
    #[test]
    fn a_launch_no_host_governs_places_nothing() {
        let src = r#"
[system]
name = "d"

[host.robot1]
launch = "multihost.launch.xml"
nodes = ["/talker"]
domain_id = 1
"#;
        let (ex, _) = place(src, &["/talker"], Some("multihost.launch.xml"));
        assert_eq!(ex.deploy["/talker"].domain, Some(1));
        let (ex2, r2) = place(src, &["/fibonacci_client"], Some("action.launch.xml"));
        assert!(r2.is_ok(), "{r2:?}");
        assert!(ex2.deploy.is_empty(), "{:?}", ex2.deploy);
    }

    // ---- [host.*] placement ----

    fn place(
        toml_src: &str,
        fqns: &[&str],
        launch: Option<&str>,
    ) -> (Execution, Result<Vec<String>, String>) {
        let cfg: SystemConfigToml = toml::from_str(toml_src).expect("parse");
        let mut ex = Execution::default();
        let r = cfg.apply_to_launch(&mut ex, fqns, launch);
        (ex, r)
    }

    #[test]
    fn hosts_partition_nodes_between_machines() {
        let (ex, r) = place(
            r#"
[system]
name = "d"

[host.robot1]
nodes = ["/talker"]
domain_id = 1

[host.robot2]
nodes = ["/listener"]
domain_id = 2
"#,
            &["/talker", "/listener"],
            None,
        );
        assert!(r.is_ok(), "{r:?}");
        assert_eq!(ex.deploy.len(), 2);
        assert_eq!(ex.deploy["/talker"].domain, Some(1));
        assert_eq!(ex.deploy["/listener"].domain, Some(2));
    }

    #[test]
    fn an_unplaced_node_is_an_error_not_a_silent_drop() {
        let (_, r) = place(
            r#"
[system]
name = "d"

[host.a]
nodes = ["/x"]

[host.b]
nodes = ["/y"]
"#,
            &["/x", "/z"],
            None,
        );
        let e = r.expect_err("an unplaced node must fail loudly");
        assert!(e.contains("/z") && e.contains("not placed"), "{e}");
    }

    #[test]
    fn a_host_scoped_to_a_launch_file_only_governs_that_one() {
        let src = r#"
[system]
name = "d"

[host.robot1]
launch = "multihost.launch.xml"
nodes = ["/talker"]
"#;
        let (ex, _) = place(src, &["/talker"], Some("multihost.launch.xml"));
        assert_eq!(ex.deploy.len(), 1);
        let (ex2, _) = place(src, &["/talker"], Some("other.launch.xml"));
        assert!(ex2.deploy.is_empty(), "an out-of-scope host must not place");
    }

    #[test]
    fn a_mistyped_host_key_is_an_error() {
        let e = toml::from_str::<HostBlock>("noeds = [\"/x\"]\n")
            .expect_err("deny_unknown_fields must reject a typo that would unplace a node");
        assert!(
            e.to_string().contains("noeds") || e.to_string().contains("unknown"),
            "{e}"
        );
    }
}
