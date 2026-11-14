//! An edge's transport: the one precedence (play_launch phase 97 P1, design
//! `docs/design/links.md` L2).
//!
//! A topic hop's transport is charged on the edge that ENDS at a subscriber,
//! and only the subscriber states it (issue #44: the same topic can reach a
//! subscriber in the publisher's process and one across a network; the
//! topic-level value was removed in v0.3.15). Every reader of transport asks here: the shared
//! derivation (`derive`'s `TopicView::transport_ms`), the resolver's global
//! graph through that view, and the per-manifest `chain-latency` fallback
//! through [`crate::Manifest::topic_transport_ms`]. Before phase 97 the
//! fallback summed the topics' own values only, so a contract that states
//! transport per subscriber (the deprecated topic key's replacement) was
//! totalled with no transport at all there.

use std::collections::BTreeMap;

use crate::{
    OptBound,
    parse::ParseError,
    types::{IncludeKind, LinkDecl, Manifest},
};

impl Manifest {
    /// The transport a subscriber endpoint ref (`<node>/<endpoint>`, as a
    /// topic's `sub:` lists it) states for itself, when the node is
    /// declared in this manifest.
    pub fn sub_transport_ms(&self, sub_ref: &str) -> Option<f64> {
        let (node, ep) = sub_ref.rsplit_once('/')?;
        self.nodes
            .get(node)?
            .subscribers
            .get(ep)?
            .max_transport
            .ms()
    }

    /// Each edge of `topic` that this manifest can see, as `(subscriber
    /// ref, transport)`, in the order the topic lists its subscribers. A
    /// transport is stated on the subscriber (v0.3.15 removed the topic's
    /// own value).
    pub fn topic_edge_transports(&self, topic: &str) -> Vec<(String, Option<f64>)> {
        let Some(t) = self.topics.get(topic) else {
            return Vec::new();
        };
        t.subscribers
            .iter()
            .map(|s| (s.clone(), self.sub_transport_ms(s)))
            .collect()
    }

    /// The most transport any edge of `topic` is charged: what a flat,
    /// per-manifest sum charges for the hop. `None` when no edge states one.
    pub fn topic_transport_ms(&self, topic: &str) -> Option<f64> {
        self.topic_edge_transports(topic)
            .into_iter()
            .filter_map(|(_, ms)| ms)
            .reduce(f64::max)
    }
}

/// Put each named link's `max_transport` on the subscribers that name it
/// (a transport is the hop into one subscriber, phase 90 G3, so only a
/// subscriber names a link) (phase 97), so every reader of transport -- this module's
/// precedence, the model, the walk -- reads a link's number without knowing
/// links exist. A link is visible to the manifest that declares it and to
/// its inline includes (an include's own link of the same name shadows).
/// Refused: a name no visible link has, a `link` beside a written
/// `max_transport` (two numbers for one hop), and since v0.3.17 a
/// `max_transport` written on the subscriber at all (deprecated in v0.3.16;
/// a transport is a platform fact, stated once on a link with its source).
pub(crate) fn apply_links(
    m: &mut Manifest,
    outer: &BTreeMap<String, LinkDecl>,
) -> Result<(), ParseError> {
    let mut visible = outer.clone();
    visible.extend(m.links.iter().map(|(k, v)| (k.clone(), v.clone())));
    let find = |at: String, name: &str| -> Result<crate::Bound, ParseError> {
        visible
            .get(name)
            .map(|l| l.max_transport.clone())
            .ok_or_else(|| ParseError::Field {
                path: at,
                message: format!(
                    "no link named `{name}` -- declare it under `links:` (known: {})",
                    if visible.is_empty() {
                        "none".to_string()
                    } else {
                        visible.keys().cloned().collect::<Vec<_>>().join(", ")
                    }
                ),
            })
    };
    let both = |at: String| ParseError::Field {
        path: at,
        message: "`link` and `max_transport` both state this hop's transport -- keep the \
                  link (its `max_transport` is the one number) and delete this key"
            .to_string(),
    };
    for (n, node) in &mut m.nodes {
        for (ep, props) in &mut node.subscribers {
            if let Some(name) = &props.link {
                if props.max_transport.is_some() {
                    return Err(both(format!("nodes.{n}.sub.{ep}.max_transport")));
                }
                props.max_transport = Some(find(format!("nodes.{n}.sub.{ep}.link"), name)?);
            } else if props.max_transport.is_some() {
                return Err(ParseError::Field {
                    path: format!("nodes.{n}.sub.{ep}.max_transport"),
                    message: "Removed in v0.3.17 (deprecated since v0.3.16): a subscriber's \
                              transport is stated on the link its hop crosses. Write \
                              `links.<name>: { max_transport: <d> }` and `link: <name>` here; \
                              `play_launch migrate <contract> --write` does it"
                        .to_string(),
                });
            }
        }
    }
    for inc in m.includes.values_mut() {
        match &mut inc.kind {
            IncludeKind::Inline(inner) => apply_links(inner, &visible)?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::parse_manifest_str;

    /// Per-subscriber values (each subscriber's link): the hop is charged its
    /// largest subscriber's value, not nothing.
    #[test]
    fn per_subscriber_values_alone_are_read() {
        let m = parse_manifest_str(
            "version: 1\n\
             links:\n\
             \x20 far: { max_transport: 56ms }\n\
             \x20 near: { max_transport: 3ms }\n\
             nodes:\n\
             \x20 p: { pub: { out: {} } }\n\
             \x20 a: { sub: { in: { link: far } } }\n\
             \x20 b: { sub: { in: { link: near } } }\n\
             \x20 c: { sub: { in: {} } }\n\
             topics:\n\
             \x20 /t: { type: T, pub: [p/out], sub: [a/in, b/in, c/in] }\n",
        )
        .unwrap();
        assert_eq!(m.topic_transport_ms("/t"), Some(56.0));
        assert_eq!(m.topic_edge_transports("/t")[2], ("c/in".into(), None));
    }

    const LINKED: &str = "version: 1\n\
         links:\n\
         \x20 serial:\n\
         \x20   max_transport: { duration: 56ms, source: { measured: { max: 46.42ms, n: 12, margin: 0.2 } } }\n\
         nodes:\n\
         \x20 p: { pub: { out: {} } }\n\
         \x20 far: { sub: { in: { link: serial } } }\n\
         \x20 near: { sub: { in: {} } }\n\
         topics:\n\
         \x20 /t: { type: T, pub: [p/out], sub: [far/in, near/in] }\n";

    /// A subscriber naming a link reads the link's number and source, and
    /// the precedence reads it like a written one.
    #[test]
    fn a_named_link_is_the_subscribers_transport() {
        let m = parse_manifest_str(LINKED).unwrap();
        let far = &m.nodes["far"].subscribers["in"];
        assert_eq!(far.link.as_deref(), Some("serial"));
        assert_eq!(far.max_transport.as_ref().and_then(|b| b.ms()), Some(56.0));
        assert!(far.max_transport.as_ref().unwrap().source.is_some());
        assert_eq!(
            m.topic_edge_transports("/t"),
            vec![("far/in".into(), Some(56.0)), ("near/in".into(), None)]
        );
        let l = &m.links["serial"];
        assert_eq!(l.clock, crate::LinkClock::Unsynced);
    }

    #[test]
    fn an_unknown_link_and_a_second_number_are_refused() {
        let unknown = LINKED.replace("link: serial", "link: seriel");
        let e = parse_manifest_str(&unknown).unwrap_err().to_string();
        assert!(
            e.contains("nodes.far.sub.in.link") && e.contains("seriel"),
            "{e}"
        );
        let both = LINKED.replace("link: serial", "link: serial, max_transport: 5ms");
        let e = parse_manifest_str(&both).unwrap_err().to_string();
        assert!(e.contains("nodes.far.sub.in.max_transport"), "{e}");
        let on_pub = LINKED.replace("out: {}", "out: { link: serial }");
        let e = parse_manifest_str(&on_pub).unwrap_err().to_string();
        assert!(e.contains("pub.out") && e.contains("SUBSCRIBER"), "{e}");
    }

    #[test]
    fn a_link_states_its_bound() {
        let no_bound = LINKED.replace(
            "max_transport: { duration: 56ms, source: { measured: { max: 46.42ms, n: 12, margin: 0.2 } } }",
            "clock: synced",
        );
        let e = parse_manifest_str(&no_bound).unwrap_err().to_string();
        assert!(e.contains("links.serial.max_transport"), "{e}");
    }
}
