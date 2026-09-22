//! Rule: detect structurally invalid entities after condition filtering.
//!
//! - Topic with 0 publishers: warning (no data source — may be wired by parent)
//! - Topic with 0 subscribers: warning (unused — may be an export)
//! - Service with 0 servers: error (calls will fail)
//! - Action with 0 servers: error (goals can't be processed)
//!
//! ...unless the missing side is declared EXTERNAL. A launch tree is routinely
//! a subset of a running system, and for a service that subset is normal
//! rather than exceptional: a client and its server are often two images. The
//! service error has no "may be wired by a parent" escape the way the topic
//! warnings do, so before `external:` existed on a service the only ways to
//! write a client-only manifest were to declare a server the image does not
//! run or to leave the service out of the contract.

use super::ValidationRule;
use crate::{CheckContext, graph::DataflowGraph};
use ros_launch_manifest_types::{ExternalEndpointSide, ExternalSide, Manifest, TopicDecl};

pub struct DanglingEntityRule;

impl ValidationRule for DanglingEntityRule {
    fn id(&self) -> &str {
        "dangling-entity"
    }

    fn check(&self, manifest: &Manifest, _graph: &DataflowGraph, ctx: &mut CheckContext) {
        // Topics
        for (name, topic) in &manifest.topics {
            if topic.publishers.is_empty()
                && !topic.subscribers.is_empty()
                && !topic_side_is_external(manifest, name, topic, ExternalSide::Pub)
            {
                ctx.warning(
                    self.id(),
                    &format!("topics.{name}"),
                    format!("topic '{name}' has no publishers (no data source)"),
                );
            }
            if !topic.publishers.is_empty()
                && topic.subscribers.is_empty()
                && !topic_side_is_external(manifest, name, topic, ExternalSide::Sub)
            {
                ctx.warning(
                    self.id(),
                    &format!("topics.{name}"),
                    format!("topic '{name}' has no subscribers (output unused)"),
                );
            }
        }

        // Services
        for (name, svc) in &manifest.services {
            if svc.server.is_empty() && !svc.client.is_empty() && !server_is_external(svc.external)
            {
                ctx.error(
                    self.id(),
                    &format!("services.{name}"),
                    format!("service '{name}' has no server (calls will fail)"),
                );
            }
        }

        // Actions
        for (name, act) in &manifest.actions {
            if act.server.is_empty() && !act.client.is_empty() && !server_is_external(act.external)
            {
                ctx.error(
                    self.id(),
                    &format!("actions.{name}"),
                    format!("action '{name}' has no server (goals can't be processed)"),
                );
            }
        }
    }
}

/// Whether `want` (`Pub` or `Sub`) was declared external for this topic, by
/// EITHER spelling of the same fact.
///
/// A topic carries its own `external:` field, and the manifest carries an
/// `external_topics:` block keyed by FQN; the two say the same thing about the
/// same topic, so a rule that reads one and not the other answers differently
/// depending on where the author wrote it. `Both` answers for either side.
///
/// Mirrors [`server_is_external`]: it is the SIDE that matters, never
/// `external.is_some()` — a topic whose CONSUMER is external still needs a
/// publisher somewhere in the tree.
fn topic_side_is_external(
    manifest: &Manifest,
    name: &str,
    topic: &TopicDecl,
    want: ExternalSide,
) -> bool {
    [topic.external, external_block_side(manifest, name)]
        .into_iter()
        .flatten()
        .any(|side| side == want || side == ExternalSide::Both)
}

/// The `external_topics:` entry for `name`, if the manifest has one.
///
/// `external_topics:` keys are FQNs while a `topics:` key may be written
/// relative to the declaring scope. Resolving that properly needs the scope's
/// namespace, which this crate never sees — the consumer's cross-scope pass is
/// where FQN resolution lives — so the only normalisation done here is the
/// leading slash, which is the difference an author writing both spellings in
/// one file actually hits.
fn external_block_side(manifest: &Manifest, name: &str) -> Option<ExternalSide> {
    if let Some(decl) = manifest.external_topics.get(name) {
        return Some(decl.side);
    }
    let alt = match name.strip_prefix('/') {
        Some(rest) => rest.to_string(),
        None => format!("/{name}"),
    };
    manifest.external_topics.get(&alt).map(|decl| decl.side)
}

/// Whether the SERVER side was declared external.
///
/// `Client` does not answer this question: a service whose CLIENT is external
/// still needs a server somewhere in the tree, and marking the client external
/// says nothing about the server. Only `Server` and `Both` excuse a missing
/// one, which is why this is a predicate rather than `external.is_some()`.
pub(super) fn server_is_external(side: Option<ExternalEndpointSide>) -> bool {
    matches!(
        side,
        Some(ExternalEndpointSide::Server | ExternalEndpointSide::Both)
    )
}
