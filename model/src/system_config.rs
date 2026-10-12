//! R1-P1 — the integrator's system config (`system.toml`), the format this
//! crate owns for the shared layers (`docs/model-boundary.md`). A resolver
//! ingests it and fills the model's execution layer; realizers read the
//! model, not this file.
//!
//! This is a LENIENT reader: it understands the shared sections (`[system]`
//! defaults, `[host.<name>]`, `[deploy.<name>]` placement, `[tiers.*]`,
//! `[[component]]`, `[[transport]]`, `[[bridge]]`) and ignores every other
//! table by design. A realizer extends the same file with its own tables and
//! validates those itself. A few realizer-owned keys are still declared
//! below; each is parsed and ignored here.
//!
//! Placement: `[deploy.<name>].nodes = ["/fqn", …]` lists the node FQNs
//! deployed to that target. A single deploy block with no `nodes` list
//! means "every node" (the common single-image case); multiple blocks
//! require explicit lists (ambiguity is an error — fail-loud).

use indexmap::IndexMap;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    Autostart, Bridge, Deploy, Execution, NodeInstance, ParamSource, ParamValue, Transport,
};

#[derive(Debug, Default, Deserialize)]
pub struct SystemConfigToml {
    #[serde(default)]
    pub system: SystemDefaults,
    #[serde(default)]
    pub deploy: BTreeMap<String, DeployBlock>,
    /// `[host.<name>]` — a MACHINE, and the only thing that partitions nodes.
    ///
    /// `[deploy.*]` carries two meanings and the resolver
    /// below has to filter one of them out to work: `kind = "self"` is a
    /// machine that takes a SHARE of the nodes, `kind = "embedded"` is a whole
    /// board build that runs ALL of them. The comment in `apply_to_launch`
    /// calls that "a conflated axis", and the filter is the workaround.
    ///
    /// `[host.*]` is the un-conflated half. A block here is always a machine,
    /// so no `kind` is needed and none is accepted; placement reads these
    /// blocks alone when any exist. A realizer's own build-side table owns
    /// the other half.
    ///
    /// Additive on purpose: when no `[host.*]` exists the `[deploy.*]` path
    /// runs exactly as before, so every file written against the old schema
    /// keeps resolving.
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
    /// `[lifecycle] autostart = "none|configure|active"` — a realizer-owned
    /// key, parsed and ignored here (`docs/model-boundary.md`). A node's
    /// lifecycle autostart comes from its launch file and contract.
    #[serde(default)]
    pub lifecycle: Option<LifecycleBlock>,
    /// `[[component]]` rows — carry `group_tiers` (the
    /// group→tier bindings) keyed by the component/node `name`.
    #[serde(default, rename = "component")]
    pub components: Vec<ComponentBlock>,
    /// `[param_services]` — a realizer-owned key, parsed and ignored here
    /// (`docs/model-boundary.md`). It is one realizer's build switch for the
    /// runtime parameter services capability, spelled as a bare table and
    /// equivalent to `[system] features = ["param_services"]`.
    #[serde(default)]
    pub param_services: Option<ParamServicesBlock>,
}

/// The `[param_services]` capability block. Empty today; a table (not a
/// bool) so future knobs (e.g. persistence) can land without a schema
/// break.
#[derive(Debug, Default, Deserialize)]
pub struct ParamServicesBlock {}

#[derive(Debug, Default, Deserialize)]
pub struct LifecycleBlock {
    #[serde(default)]
    pub autostart: Option<String>,
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
    /// `[system] features` — a realizer-owned key, parsed and ignored here
    /// (`docs/model-boundary.md`): capability switches of one realizer's
    /// build.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
}

/// `[host.<name>]` — one machine in a multi-host system.
///
/// The half of `[deploy.*]` that is genuinely a placement. Deliberately tiny:
/// a machine is identified by its name, says which nodes it runs, and
/// optionally scopes itself to one launch file. Everything else a deploy block
/// carried — board, target, rmw, profile, features — describes a BUILD and
/// belongs to the realizer's own build table.
///
/// `deny_unknown_fields` for the same reason `DeployBlock` has it: a mistyped
/// key here silently unplaces a node, which surfaces far away as a realizer
/// that runs no nodes on that machine.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostBlock {
    /// Node FQNs this machine runs. Empty means "every node", which is only
    /// meaningful when this is the sole host.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<String>,
    /// Launch file this block applies to, relative to the bringup package.
    /// `None` means every launch file — same rule as `DeployBlock::launch`.
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
    /// Does this block govern `launch_file`? Same rule as
    /// [`DeployBlock::applies_to_launch`]: an unscoped block governs
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

/// One `[deploy.<name>]` block.
///
/// **This is the single definition of the `system.toml` deploy schema.**
/// A realizer that mirrored it in its own type drifted from it once: this
/// struct lacked `launch`, so serde silently dropped it and launch-scoped
/// blocks were counted against every launch file. A realizer re-exports this
/// type rather than redeclaring it (history: CHANGELOG).
///
/// `deny_unknown_fields` makes the next divergence a loud parse error instead
/// of a silently ignored key.
///
/// Of the fields below, placement (`nodes`, `launch`, and `kind`, which
/// tells a machine that takes a share of the nodes from a whole-system
/// build that runs all of them) and the session facts (`rmw`, `domain_id`,
/// `locator`) reach the model. `target`, `board`, `framework`, `profile`,
/// `optimize`, `features` and the opaque realizer sub-table describe a
/// BUILD: realizer-owned keys, parsed and ignored here
/// (`docs/model-boundary.md`).
// `Eq` dropped when the opaque realizer sub-table was added: `toml::Value` is
// only `PartialEq` (floats). `==` still works; no known consumer used
// `DeployBlock` as a set key or otherwise required total equality.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeployBlock {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub board: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framework: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rmw: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optimize: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
    /// R1-P1 placement — node FQNs deployed to this target.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<String>,
    /// Launch file this block applies to, relative to the bringup pkg
    /// (`multihost.launch.xml`). `None` means "every launch file".
    ///
    /// Realizers wrote this key before this struct declared it, and serde
    /// silently dropped it. Placement then counted launch-scoped blocks
    /// against launch files they were never meant to govern — see
    /// [`Self::applies_to_launch`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch: Option<String>,

    /// `[deploy.<name>.<realizer>]` — one realizer's own deploy
    /// configuration, carried verbatim and **not interpreted by this crate**.
    /// A realizer-owned key, parsed and ignored here
    /// (`docs/model-boundary.md`); the field's name is historical.
    ///
    /// This crate models launch and system STRUCTURE. A consumer's site
    /// configuration — where its RTOS SDK lives, which network stack it chose,
    /// where its config headers are, how it flashes a board — is not that
    /// vocabulary, and declaring those keys here would mean cutting a spec
    /// release every time a consumer adds one.
    ///
    /// Opaque on purpose. `DeployBlock` is `deny_unknown_fields` so that a
    /// mistyped key is an error rather than a silent drop; this field is the
    /// one sanctioned extension point, and the consumer applies its own schema
    /// (including its own `deny_unknown_fields`) to the value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nros: Option<toml::Value>,
}

impl DeployBlock {
    /// Whether this block governs `launch_file`.
    ///
    /// An unscoped block (`launch` absent) governs every launch file. A scoped
    /// one governs only its own — compared on the file NAME, since the key is
    /// written relative to the bringup pkg while callers hold a full path.
    /// An unknown caller launch (`None`) keeps every block, preserving the
    /// behaviour from before the key existed, for callers that cannot say
    /// which file they resolve.
    pub fn applies_to_launch(&self, launch_file: Option<&str>) -> bool {
        let (Some(scope), Some(current)) = (self.launch.as_deref(), launch_file) else {
            return true;
        };
        let base = |p: &str| p.rsplit(['/', '\\']).next().unwrap_or(p).to_string();
        base(scope) == base(current)
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
    /// clamped domains); ambiguity (multiple deploy blocks, none listing a
    /// node) is an `Err` — fail-loud, never silent partial placement.
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
    /// Deploy blocks scoped with `launch = "…"` are filtered out when they
    /// name a different file. Without this, a bringup with one unscoped block
    /// plus two scoped ones looked like "three deploy blocks" to every launch
    /// file, and the multi-block rule below demanded an explicit
    /// `nodes = [..]` for nodes the scoped blocks never governed — a hard
    /// error on a config that is correct.
    pub fn apply_to_launch(
        &self,
        execution: &mut Execution,
        node_fqns: &[&str],
        launch_file: Option<&str>,
    ) -> Result<Vec<String>, String> {
        let mut diags = Vec::new();

        // `[system] features` and `[param_services]` are NOT written to
        // `execution.features`: they are one realizer's build switches, and
        // the model carries only facts every realizer reads the same way
        // (docs/model-boundary.md, W6). They still parse, into nothing.

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

        // `[host.*]` placement — the un-conflated path. Hosts win for the
        // launch files they GOVERN, and only those.
        //
        // Scoped, not global, and the difference is what makes an incremental
        // migration possible. Hosts are usually `launch`-scoped (a multi-host
        // bringup) while the `[deploy.*]` blocks beside them are not, so a
        // "any host disables deploy" rule silently drops placement for every
        // OTHER launch file in the same package — measured on a real
        // workspace, where `execution.deploy` vanished from every non-multihost
        // model. For a launch file no host governs, the deploy path below runs
        // unchanged.
        //
        // Target is deliberately `None` for every placed node. A host says
        // WHERE a node runs; what it is built as comes from the realizer's own
        // build table, which supplies the concrete target. That is the same
        // conclusion the multi-board deploy path reached (see
        // `embedded_blocks_make_placement_board_agnostic`), reached here by
        // construction instead of by a special case.
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
                // The host's NAME is not written (`Deploy.extra` is no longer
                // written at all, docs/model-boundary.md W6): nothing reads it,
                // and the facts placement changes are the typed fields below.
                execution.deploy.insert(
                    (*fqn).to_string(),
                    Deploy {
                        target: None,
                        domain: clamp_domain(block.domain_id, &mut diags, hname).or_else(|| {
                            clamp_domain(self.system.domain_id, &mut diags, "[system]")
                        }),
                        locator: block
                            .locator
                            .clone()
                            .or_else(|| self.system.locator.clone()),
                        rmw: self.system.rmw.clone(),
                        extra: BTreeMap::new(),
                    },
                );
            }
            return Ok(diags);
        }

        if self.deploy.is_empty() {
            return Ok(diags);
        }

        // Placement resolution — over the blocks that GOVERN this launch file
        // AND that actually PARTITION nodes.
        //
        // `[deploy.*]` carries two different meanings, and only one of them is a
        // placement:
        //
        //   * `kind = "self"`  — a machine. Multiple of these PARTITION the
        //     nodes between them via their `nodes = [..]` lists.
        //   * `kind = "embedded"` — a BOARD BUILD of the whole system. Every
        //     such block runs every node; they are alternatives, not shares.
        //
        // Counting embedded blocks as placement domains asks "which of these
        // six machines runs /talker" about six builds of one image, and the
        // answer is "all of them". A system with native + zephyr + nuttx
        // blocks, two nodes and no `nodes = [..]` anywhere states this plainly
        // — and it could not re-resolve at all, failing `node '/ctrl_node' is
        // not placed`. It only looked healthy because its committed model
        // predated this resolver and was never regenerated.
        //
        // Same class as the launch-scope filter immediately above: placement
        // counting blocks it was never meant to govern. That one was a dropped
        // `launch` field; this one is a conflated axis.
        let partitioning: Vec<(&String, &DeployBlock)> = self
            .deploy
            .iter()
            .filter(|(_, b)| b.applies_to_launch(launch_file))
            .filter(|(_, b)| b.kind.as_deref() != Some("embedded"))
            .collect();
        // An embedded-only bringup still has a target worth naming — but only
        // when there is exactly ONE of them.
        //
        // With a single embedded block, falling back gives every node that
        // target, which is unambiguous and useful. With SEVERAL, the fallback
        // asks which of N whole-system board builds runs a given node, and the
        // answer is "all of them" — a node->target map cannot say that. The
        // first version of this fell back unconditionally and so turned two
        // embedded blocks into `node '/ctrl_node' is not placed`, which is the
        // very error this filter exists to prevent (a system declaring two
        // embedded builds of one image, e.g. two RTOSes on one board).
        //
        // So: emit NO placement instead. Consumers already treat a board the
        // map never mentions as unconstrained, which is exactly right here.
        let governing: Vec<(&String, &DeployBlock)> = self
            .deploy
            .iter()
            .filter(|(_, b)| b.applies_to_launch(launch_file))
            .collect();
        let in_scope: Vec<(&String, &DeployBlock)> = if !partitioning.is_empty() {
            partitioning
        } else if governing.len() == 1 {
            governing
        } else {
            return Ok(diags);
        };
        if in_scope.is_empty() {
            return Ok(diags);
        }
        let single = (in_scope.len() == 1).then(|| in_scope[0].0);
        for fqn in node_fqns {
            let (dname, block) = if let Some(k) = single {
                let b = &self.deploy[k];
                if !b.nodes.is_empty() && !b.nodes.iter().any(|n| n == fqn) {
                    continue; // explicitly not placed
                }
                (k.as_str(), b)
            } else {
                // The one placement source: an explicit `nodes = [..]` entry.
                //
                // There used to be a second — the node's `<node machine="…">`,
                // recorded as `execution.deploy[fqn].host` — but `machine=` is
                // ROS 1 roslaunch syntax that ROS 2's frontend rejects, so the
                // capture and the `Deploy.host` field are gone. Per-host
                // slicing now happens at RESOLVE time via an ordinary launch
                // argument + `if=` conditions; each per-host model then
                // contains only that host's nodes.
                match in_scope
                    .iter()
                    .find(|(_, b)| b.nodes.iter().any(|n| n == fqn))
                    .map(|(k, b)| (k.as_str(), *b))
                {
                    Some((k, b)) => (k, b),
                    None => {
                        return Err(format!(
                            "system config: node '{fqn}' is not placed — with multiple \
                             [deploy.*] blocks every node needs a `nodes = [..]` entry"
                        ));
                    }
                }
            };
            // `Deploy.target` and `Deploy.extra` are no longer written
            // (docs/model-boundary.md W6): the board, kind and build fields
            // of `[deploy.*]` describe one realizer's BUILD, and no realizer
            // reads the target. Placement stays: the node is in the map,
            // with the domain / locator / rmw its block states.
            execution.deploy.insert(
                (*fqn).to_string(),
                Deploy {
                    target: None,
                    // RFC-0004 ladder: deploy override > system default.
                    domain: clamp_domain(block.domain_id, &mut diags, dname)
                        .or_else(|| clamp_domain(self.system.domain_id, &mut diags, "[system]")),
                    locator: block
                        .locator
                        .clone()
                        .or_else(|| self.system.locator.clone()),
                    rmw: block.rmw.clone().or_else(|| self.system.rmw.clone()),
                    extra: BTreeMap::new(),
                },
            );
        }

        // Placement lists naming unknown nodes: diagnostic, not fatal
        // (conditional nodes may be absent in this variant).
        for (dname, block) in &self.deploy {
            for n in &block.nodes {
                if !node_fqns.contains(&n.as_str()) {
                    diags.push(format!(
                        "system config: [deploy.{dname}] places unknown node '{n}' \
                         (absent in this variant?)"
                    ));
                }
            }
        }
        Ok(diags)
    }

    /// The `[lifecycle] autostart` level, if declared. A realizer-owned key,
    /// parsed and ignored here (`docs/model-boundary.md`): nothing in this
    /// crate applies it, and a node's `lifecycle_autostart` comes from its
    /// launch file and contract. Kept as a read accessor for the realizer
    /// that owns the key. Unknown strings return `None`.
    pub fn lifecycle_autostart(&self) -> Option<Autostart> {
        match self.lifecycle.as_ref()?.autostart.as_deref()? {
            "active" => Some(Autostart::Active),
            "configure" => Some(Autostart::Configure),
            "none" => Some(Autostart::None),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOML: &str = r#"
[system]
rmw = "zenoh"
domain_id = 7
features = ["safety"]

[deploy.native]
nodes = ["/ctrl/control_node"]
profile = "release"

[deploy.imu]
board = "stm32f4"
rmw = "cyclonedds"
domain_id = 9
nodes = ["/sensing/imu_node"]
optimize = "size"

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
        // the realizer-owned [lifecycle] table.
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
        // lifecycle autostart accessor.
        assert_eq!(cfg.lifecycle_autostart(), Some(Autostart::Active));
    }

    /// A bringup with ONE unscoped deploy block plus launch-scoped siblings
    /// must not trip the multi-block placement rule.
    ///
    /// The shape is a real bringup's: three `[deploy.*]` blocks, no `nodes =
    /// [..]` anywhere, robot1/robot2 scoped to `multihost.launch.xml`.
    /// Resolving `system.launch.xml` used to fail with "node '/listener' is not
    /// placed" because `launch` was not a field on `DeployBlock` at all — serde
    /// dropped it and all three blocks counted. This struct is the ONE
    /// definition of the deploy schema, and `deny_unknown_fields` makes the
    /// next divergence loud. A key no field claims is now a parse error, not a
    /// silent drop — silently dropping `launch` is exactly how the original bug
    /// happened.
    #[test]
    fn deploy_block_rejects_unknown_keys_and_round_trips() {
        let toml = r#"
[system]
name = "demo"

[deploy.robot1]
kind = "self"
target = "x86_64-unknown-linux-gnu"
launch = "multihost.launch.xml"
nodes = ["/talker"]
"#;
        let cfg = parse_system_config(toml).expect("parses");
        let b = &cfg.deploy["robot1"];
        assert_eq!(b.launch.as_deref(), Some("multihost.launch.xml"));
        assert_eq!(b.nodes, vec!["/talker".to_string()]);

        // Serialize is required because a realizer WRITES system.toml through
        // this same type; absent options must not appear.
        let out = toml::to_string(b).expect("serializes");
        assert!(out.contains("launch = "), "{out}");
        assert!(
            !out.contains("board"),
            "absent options must be skipped:\n{out}"
        );

        // A key no field claims fails loudly.
        let bad = toml.replace("nodes", "nodez");
        let err = parse_system_config(&bad).expect_err("unknown key must be rejected");
        assert!(err.contains("nodez"), "error should name the key: {err}");
    }

    #[test]
    fn launch_scoped_deploy_blocks_do_not_govern_other_launch_files() {
        let toml = r#"
[system]
name = "demo"

[deploy.native]
kind = "self"
target = "x86_64-unknown-linux-gnu"

[deploy.robot1]
kind = "self"
target = "x86_64-unknown-linux-gnu"
launch = "multihost.launch.xml"

[deploy.robot2]
kind = "self"
target = "x86_64-unknown-linux-gnu"
launch = "multihost.launch.xml"
"#;
        let cfg = parse_system_config(toml).expect("parses");

        // The key must round-trip now, not be silently dropped.
        assert_eq!(
            cfg.deploy["robot1"].launch.as_deref(),
            Some("multihost.launch.xml"),
            "`launch` must be a declared field, not swallowed by serde"
        );

        // system.launch.xml: only `native` is in scope -> implicit placement.
        let mut e = Execution::default();
        cfg.apply_to_launch(&mut e, &["/talker", "/listener"], Some("system.launch.xml"))
            .expect("one in-scope block places implicitly");
        assert!(e.deploy.contains_key("/talker"));
        assert!(e.deploy.contains_key("/listener"));

        // A full path resolves the same — comparison is on the file name.
        let mut e2 = Execution::default();
        cfg.apply_to_launch(
            &mut e2,
            &["/talker"],
            Some("/abs/src/demo_bringup/launch/system.launch.xml"),
        )
        .expect("path and bare name agree");
        assert!(e2.deploy.contains_key("/talker"));

        // multihost.launch.xml: native + both robots are in scope -> genuinely
        // ambiguous without `nodes = [..]`, so the fail-loud rule still fires.
        let mut e3 = Execution::default();
        let err = cfg
            .apply_to_launch(&mut e3, &["/talker"], Some("multihost.launch.xml"))
            .expect_err("three in-scope blocks with no placement is still an error");
        assert!(err.contains("is not placed"), "{err}");

        // Callers that cannot name the launch file keep the old behaviour.
        let mut e4 = Execution::default();
        assert!(
            cfg.apply_to(&mut e4, &["/talker"]).is_err(),
            "an unknown launch file must not silently narrow the scope"
        );
    }

    /// A system that declares BOTH self machines and `kind = "embedded"`
    /// board builds is multi-board: the same nodes run on native AND on every
    /// embedded board. Because the model deploy is single-target per node,
    /// such placements must be board-AGNOSTIC (`target = None`), so a
    /// realizer that slices nodes per board (admitting a `None` node on every
    /// board) includes them for native and each embedded board alike. Pinning
    /// `native`'s `linux` target here left every embedded build with no
    /// nodes.
    #[test]
    fn embedded_blocks_make_placement_board_agnostic() {
        let toml = r#"
[system]
name = "demo"

[deploy.native]
kind = "self"
target = "x86_64-unknown-linux-gnu"

[deploy.freertos]
kind = "embedded"
board = "mps2-an385-freertos"

[deploy.nuttx]
kind = "embedded"
board = "nuttx-qemu-arm"
"#;
        let cfg = parse_system_config(toml).expect("parses");
        let mut e = Execution::default();
        cfg.apply_to_launch(&mut e, &["/talker", "/listener"], Some("system.launch.xml"))
            .expect("embedded blocks do not partition; native places implicitly");
        // Placed, and board-agnostic: no target is written at all since W6
        // (docs/model-boundary.md), so the consumer's own board decides.
        for n in ["/talker", "/listener"] {
            let d = e.deploy.get(n).unwrap_or_else(|| panic!("{n} placed"));
            assert!(
                d.target.is_none(),
                "{n}: no target is written, got {:?}",
                d.target
            );
        }
    }

    /// With `machine=` gone (ROS 1 roslaunch syntax; its `Deploy.host`
    /// fallback went with it), `nodes = [..]`
    /// is the ONLY placement source when multiple self blocks are in scope.
    /// A node in no list fails loud instead of riding a launch-derived host.
    #[test]
    fn nodes_lists_are_the_only_placement_with_multiple_self_blocks() {
        let toml = r#"
[system]
name = "demo"

[deploy.native]
kind = "self"

[deploy.robot1]
kind = "self"
launch = "multihost.launch.xml"
nodes = ["/talker"]
domain_id = 1

[deploy.robot2]
kind = "self"
launch = "multihost.launch.xml"
nodes = ["/listener"]
domain_id = 2
"#;
        let cfg = parse_system_config(toml).expect("parses");

        let mut e = Execution::default();
        cfg.apply_to_launch(
            &mut e,
            &["/talker", "/listener"],
            Some("multihost.launch.xml"),
        )
        .expect("nodes lists place both nodes");

        // Which block placed each node, seen through what it changes.
        assert_eq!(e.deploy["/talker"].domain, Some(1));
        assert_eq!(e.deploy["/listener"].domain, Some(2));

        // A node in no list is unplaced -> fail loud (the behaviour from
        // before the `machine=` fallback, restored because the
        // machine=-derived fact no longer exists).
        let err = cfg
            .apply_to_launch(&mut e, &["/ghost"], Some("multihost.launch.xml"))
            .expect_err("a node in no nodes list is not placed");
        assert!(err.contains("is not placed"), "{err}");
    }

    #[test]
    fn fills_execution_with_ladder_and_placement() {
        let cfg = parse_system_config(TOML).expect("parses");
        let mut e = Execution::default();
        let diags = cfg
            .apply_to(&mut e, &["/ctrl/control_node", "/sensing/imu_node"])
            .expect("applies");
        assert!(diags.is_empty(), "{diags:?}");
        // W6 (docs/model-boundary.md): build switches, build fields and the
        // board never reach the model; placement and the session ladder do.
        assert!(e.features.is_empty(), "{:?}", e.features);
        let ctrl = &e.deploy["/ctrl/control_node"];
        assert!(ctrl.target.is_none() && ctrl.extra.is_empty(), "{ctrl:?}");
        assert_eq!(ctrl.domain, Some(7), "system default rides the ladder");
        assert_eq!(ctrl.rmw.as_deref(), Some("zenoh"));
        let imu = &e.deploy["/sensing/imu_node"];
        assert!(imu.target.is_none() && imu.extra.is_empty(), "{imu:?}");
        assert_eq!(imu.domain, Some(9), "deploy override wins");
        assert_eq!(imu.rmw.as_deref(), Some("cyclonedds"));
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
             params_files = [\"talker:\\n  ros__parameters:\\n    x: 1\\n\"]\n\
             [deploy.native]\nkind = \"self\"\n",
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
             name = \"rust_params_param_talker\"\nparams = { rate = 7 }\n\
             [deploy.native]\nkind = \"self\"\n",
        )
        .expect("parses");
        let mut inst = NodeInstance::default();
        inst.pkg = Some("rust_param_talker_pkg".to_string());
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
             [[component]]\npkg = \"talker_pkg\"\nname = \"ghost\"\nparams = { a = 1 }\n\
             [deploy.native]\nkind = \"self\"\n",
        )
        .expect("parses");
        let mut nodes = IndexMap::new();
        for fqn in ["/talker_one", "/talker_two"] {
            let mut i = NodeInstance::default();
            i.pkg = Some("talker_pkg".to_string());
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
             [[component]]\nname = \"talker\"\nparams = { rate = 10 }\n\
             [deploy.native]\nkind = \"self\"\n",
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
             [[component]]\nname = \"ghost\"\nparams = { a = 1 }\n\
             [deploy.native]\nkind = \"self\"\n",
        )
        .expect("parses");
        let mut nodes = IndexMap::new();
        nodes.insert("/talker".to_string(), NodeInstance::default());
        let diags = cfg.apply_params_to_nodes(&mut nodes);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].contains("ghost"), "{diags:?}");
    }

    #[test]
    fn a_bare_param_services_section_parses_and_reaches_nothing() {
        let cfg = parse_system_config(
            "[system]\nrmw = \"zenoh\"\n[param_services]\n[deploy.native]\nkind = \"self\"\n",
        )
        .expect("parses");
        let mut e = Execution::default();
        cfg.apply_to(&mut e, &["/a"]).expect("applies");
        assert!(e.features.is_empty(), "{:?}", e.features);
    }

    #[test]
    fn system_features_parse_and_reach_nothing() {
        let cfg = parse_system_config(
            "[system]\nrmw = \"zenoh\"\nfeatures = [\"param_services\"]\n[param_services]\n[deploy.native]\nkind = \"self\"\n",
        )
        .expect("parses");
        let mut e = Execution::default();
        cfg.apply_to(&mut e, &["/a"]).expect("applies");
        assert!(e.features.is_empty(), "{:?}", e.features);
    }

    #[test]
    fn single_block_defaults_to_all_nodes() {
        let cfg = parse_system_config(
            "[system]\nrmw = \"zenoh\"\n[deploy.native]\nprofile = \"debug\"\n",
        )
        .expect("parses");
        let mut e = Execution::default();
        cfg.apply_to(&mut e, &["/a", "/b"]).expect("applies");
        assert_eq!(e.deploy.len(), 2);
        assert!(e.deploy["/a"].target.is_none());
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
        assert!(
            ex.deploy["/talker"].extra.is_empty(),
            "the host's name is not written"
        );
    }

    /// A host says WHERE, never WHAT. The build half lives in the realizer's
    /// own table, so a placed node is board-agnostic and the realizer's board
    /// decides — the conclusion the multi-board deploy path reached, here
    /// true by construction.
    #[test]
    fn a_host_placement_is_board_agnostic() {
        let (ex, _) = place(
            "[system]\nname = \"d\"\n\n[host.robot1]\nnodes = [\"/talker\"]\n",
            &["/talker"],
            None,
        );
        assert!(ex.deploy["/talker"].target.is_none());
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

    /// The migration property: hosts WIN outright, so a file part-way through
    /// migration does not have to keep two tables in agreement.
    #[test]
    fn hosts_are_the_only_source_when_present() {
        let (ex, _) = place(
            r#"
[system]
name = "d"

[deploy.old]
kind = "self"
board = "some-board"
nodes = ["/talker"]
domain_id = 5

[host.robot1]
nodes = ["/talker"]
domain_id = 1
"#,
            &["/talker"],
            None,
        );
        assert_eq!(
            ex.deploy["/talker"].domain,
            Some(1),
            "the deploy block must not contribute when a host exists"
        );
        assert!(ex.deploy["/talker"].target.is_none());
    }

    /// Absent `[host.*]`, every existing file resolves exactly as before.
    #[test]
    fn without_hosts_the_deploy_path_is_untouched() {
        let (ex, r) = place(
            r#"
[system]
name = "d"

[deploy.native]
kind = "self"
board = "linux-x86"
domain_id = 3
"#,
            &["/talker"],
            None,
        );
        assert!(r.is_ok(), "{r:?}");
        assert_eq!(ex.deploy["/talker"].domain, Some(3));
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

    /// The case a real migration hits, and the one an "any host wins" rule
    /// breaks: hosts scoped to ONE launch file, `[deploy.*]` governing the
    /// others. Measured on a real workspace, where the aggressive rule
    /// emptied `execution.deploy` for every non-multihost model.
    #[test]
    fn a_launch_no_host_governs_still_uses_deploy() {
        let src = r#"
[system]
name = "d"

[deploy.native]
kind = "self"
domain_id = 3

[host.robot1]
launch = "multihost.launch.xml"
nodes = ["/talker"]
domain_id = 1
"#;
        // The multihost launch: the host places it.
        let (ex, _) = place(src, &["/talker"], Some("multihost.launch.xml"));
        assert_eq!(ex.deploy["/talker"].domain, Some(1));

        // Any OTHER launch file: no host governs it, so the deploy block still
        // places, exactly as before hosts existed.
        let (ex2, r2) = place(src, &["/fibonacci_client"], Some("action.launch.xml"));
        assert!(r2.is_ok(), "{r2:?}");
        assert_eq!(
            ex2.deploy["/fibonacci_client"].domain,
            Some(3),
            "a launch file no host governs must fall back to [deploy.*]"
        );
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
mod deploy_consumer_block_tests {
    use super::DeployBlock;

    /// The sanctioned extension point: a realizer's own keys ride in the
    /// deploy block's opaque sub-table and are carried verbatim.
    #[test]
    fn a_consumer_block_is_carried_verbatim() {
        let b: DeployBlock = toml::from_str(
            r#"
            kind = "self"
            board = "mps2-an385-freertos"
            [nros]
            netstack = "lwip"
            sdk = { freertos = "{env:FREERTOS_DIR}" }
            "#,
        )
        .expect("parses");
        let nros = b.nros.expect("consumer block present");
        assert_eq!(nros.get("netstack").and_then(|v| v.as_str()), Some("lwip"));
        assert!(nros.get("sdk").and_then(|v| v.get("freertos")).is_some());
    }

    /// Absent is the common case and must stay cheap — every existing
    /// system.toml in the wild omits it.
    #[test]
    fn absent_consumer_block_is_none() {
        let b: DeployBlock = toml::from_str(r#"kind = "self""#).expect("parses");
        assert!(b.nros.is_none());
    }

    /// The extension point must NOT weaken the typo guard: an unknown key at
    /// the deploy level is still an error. That is why this is one named
    /// opaque field rather than a flattened catch-all.
    #[test]
    fn unknown_top_level_keys_are_still_rejected() {
        let e = toml::from_str::<DeployBlock>(r#"netstck = "lwip""#).unwrap_err();
        assert!(e.to_string().contains("netstck"), "got: {e}");
    }
}
