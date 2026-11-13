//! Rules: links (play_launch phase 97, `docs/design/links.md`).
//!
//! - `link-unsourced` (warning; an error under `--strict`): a link whose
//!   `max_transport` states no `source:`. Every link is best effort until a
//!   guaranteed-link extension exists (L6), and a best-effort transport has
//!   no derivable worst case: its number is evidence only with a source.
//! - `deprecated-spelling` (warning; the `transport-inline` rule): a
//!   `max_transport` written on a subscriber instead of named through a
//!   link. Deprecated since v0.3.16 (it was an info, "legal for one release
//!   of overlap", from v0.3.4 on, and never scheduled), refused at the next
//!   release; `migrate` writes the link. A link states the number once,
//!   with its source, and `check --against <run>` holds it against runs.

use super::ValidationRule;
use crate::{CheckContext, graph::DataflowGraph};
use ros_launch_manifest_types::{Manifest, OptBound};

pub struct LinkSourceRule;

impl ValidationRule for LinkSourceRule {
    fn id(&self) -> &str {
        "link-unsourced"
    }

    fn check(&self, manifest: &Manifest, _graph: &DataflowGraph, ctx: &mut CheckContext) {
        for (name, link) in &manifest.links {
            if link.max_transport.source.is_none() {
                ctx.warning(
                    self.id(),
                    &format!("links.{name}.max_transport"),
                    format!(
                        "link '{name}': `max_transport` states no `source:`. A best-effort \
                         link's transport is platform behaviour no formula bounds; state the \
                         measurement it came from (`source: {{ measured: {{ max, n, margin, \
                         run }} }}`, `play_launch measure` proposes one)"
                    ),
                );
            }
        }
    }
}

pub struct TransportInlineRule;

impl ValidationRule for TransportInlineRule {
    fn id(&self) -> &str {
        "transport-inline"
    }

    fn check(&self, manifest: &Manifest, _graph: &DataflowGraph, ctx: &mut CheckContext) {
        for (n, node) in &manifest.nodes {
            for (ep, props) in &node.subscribers {
                if props.link.is_none()
                    && let Some(d) = props.max_transport.duration()
                {
                    ctx.warning(
                        "deprecated-spelling",
                        &format!("nodes.{n}.sub.{ep}.max_transport"),
                        format!(
                            "'{n}/{ep}': `max_transport: {d}` written on the subscriber is \
                             deprecated since v0.3.16 and refused at the next release. Name a \
                             link instead (`links.<name>: {{ max_transport }}`, then `link: \
                             <name>` here), so the number is stated once with its source; \
                             `play_launch migrate <contract> --write` writes it"
                        ),
                    );
                }
            }
        }
    }
}
