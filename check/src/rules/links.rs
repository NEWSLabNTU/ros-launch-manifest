//! Rules: links (play_launch phase 97, `docs/design/links.md`).
//!
//! - `link-unsourced` (warning; an error under `--strict`): a link whose
//!   `max_transport` states no `source:`. Every link is best effort until a
//!   guaranteed-link extension exists (L6), and a best-effort transport has
//!   no derivable worst case: its number is evidence only with a source.
//!
//! A `max_transport` written on a subscriber instead of named through a
//! link was the info `transport-inline` (v0.3.4), `deprecated-spelling`
//! (v0.3.16), and is a parse error since v0.3.17 (`transport::apply_links`).

use super::ValidationRule;
use crate::{CheckContext, graph::DataflowGraph};
use ros_launch_manifest_types::Manifest;

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
