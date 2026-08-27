//! Rule: nudge `chains:` toward the scope path that derives the same route.
//!
//! A chain names a route hop by hop. The route is already computable from the
//! facts a contract must declare anyway — a path's `trigger` says what causes
//! it, its `output` says what it publishes, and the topic graph joins the two —
//! so an authored `segments:` list is a second copy of something the tool
//! already knows, and two copies can disagree. `chain-link` exists solely to
//! catch that disagreement, which is the clearest possible sign the second copy
//! should not exist.
//!
//! Measured on `rt_workspace`: five lines of `segments:` restating a route the
//! graph defines, in a contract where roughly a third of the content is
//! irreducible.
//!
//! # Why this only fires now
//!
//! Until phase 68 the mapper read authored chains and nothing else, so deleting
//! `segments:` did not move the derivation to another source — it removed the
//! only source. With derived routes reaching the mapper, a scope path produces
//! the same scheduling decisions, verified by provenance rather than by the
//! numbers agreeing:
//!
//! ```text
//! derived(chain_aware: points_to_cmd segment drain 2/2) -> prio 39
//! ```
//!
//! # Why Info, and why `topics.rate_hz` is NOT included
//!
//! Info follows the `deprecated-unit-suffix` and `explicit-trigger` precedent:
//! an un-migrated contract is valid, not broken, and a warning would punish
//! people for a migration they have not been asked to do yet.
//!
//! `topics.<t>.rate_hz` is the other field the design lists as derivable — it
//! propagates from the timer that starts the chain — but that propagation is
//! **not implemented**, so nothing would reconstruct the value an author
//! deleted. Deprecating a field whose replacement does not exist would be
//! worse than leaving it alone, so this rule deliberately says nothing about
//! it. The same goes for the one thing a scope path still cannot express,
//! `semantics`, which is why the message stops short of telling an author to
//! delete a chain that declares `age`.

use super::ValidationRule;
use crate::{CheckContext, graph::DataflowGraph};
use ros_launch_manifest_types::{ChainSemantics, Manifest};

pub struct DerivableChainRule;

impl ValidationRule for DerivableChainRule {
    fn id(&self) -> &str {
        "derivable-chain"
    }

    fn check(&self, manifest: &Manifest, _graph: &DataflowGraph, ctx: &mut CheckContext) {
        for (chain_name, chain) in &manifest.chains {
            // `reaction` is what a derived route means today. A chain asking
            // for `age` is saying something a scope path cannot yet say, so
            // telling its author to migrate would be telling them to lose a
            // requirement.
            if chain.semantics != ChainSemantics::Reaction {
                continue;
            }
            let hops = chain.segments.len();
            ctx.emit(
                self.id(),
                crate::Severity::Info,
                &format!("chains.{chain_name}"),
                format!(
                    "chain '{chain_name}' hand-writes a route ({hops} segment(s)) that is \
                     derivable from the `trigger`/`output` facts its nodes already declare. \
                     A scope path states the same requirement without the route:\n\
                     \x20 paths:\n\
                     \x20   {chain_name}:\n\
                     \x20     input: <first topic>\n\
                     \x20     output: [<last topic>]\n\
                     \x20     max_latency: {}\n\
                     The mapper derives the same schedule from either spelling; `chains:` is \
                     on the way out because a written route is a second copy of the graph, and \
                     `chain-link` exists only to catch the two disagreeing.",
                    chain.max_latency
                ),
            );
        }
    }
}
