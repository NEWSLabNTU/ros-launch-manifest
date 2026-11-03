//! An edge's transport: the one precedence (play_launch phase 97 P1, design
//! `docs/design/links.md` L2).
//!
//! A topic hop's transport is charged on the edge that ENDS at a subscriber:
//! the subscriber's own value if it states one, else the topic's (issue #44:
//! the same topic can reach a subscriber in the publisher's process and one
//! across a network). Every reader of transport asks here: the shared
//! derivation (`derive`'s `TopicView::transport_ms`), the resolver's global
//! graph through that view, and the per-manifest `chain-latency` fallback
//! through [`crate::Manifest::topic_transport_ms`]. Before phase 97 the
//! fallback summed the topics' own values only, so a contract that states
//! transport per subscriber (the deprecated topic key's replacement) was
//! totalled with no transport at all there.

use crate::{OptBound, types::Manifest};

/// The transport charged for the edge into a subscriber: its own value,
/// else the topic's. `None`: the hop states none (it contributes 0 to a
/// total, and the rules that need one say so).
pub fn edge_transport_ms(sub_own: Option<f64>, topic_own: Option<f64>) -> Option<f64> {
    sub_own.or(topic_own)
}

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
    /// topic with no subscriber listed yields one entry with an empty ref
    /// and the topic's own value, so a topic-level value is still read.
    pub fn topic_edge_transports(&self, topic: &str) -> Vec<(String, Option<f64>)> {
        let Some(t) = self.topics.get(topic) else {
            return Vec::new();
        };
        let own = t.max_transport.ms();
        if t.subscribers.is_empty() {
            return vec![(String::new(), own)];
        }
        t.subscribers
            .iter()
            .map(|s| (s.clone(), edge_transport_ms(self.sub_transport_ms(s), own)))
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

#[cfg(test)]
mod tests {
    use crate::parse_manifest_str;

    /// Two subscribers of one topic, one overriding: the override wins on
    /// its edge, the topic's value on the other, and the hop's charge is
    /// the larger.
    #[test]
    fn a_subscriber_override_wins_on_its_edge_only() {
        let m = parse_manifest_str(
            "version: 1\n\
             nodes:\n\
             \x20 p: { pub: { out: {} } }\n\
             \x20 fast: { sub: { in: { max_transport: 0ms } } }\n\
             \x20 slow: { sub: { in: {} } }\n\
             topics:\n\
             \x20 /t: { type: T, pub: [p/out], sub: [fast/in, slow/in], max_transport: 10ms }\n",
        )
        .unwrap();
        assert_eq!(
            m.topic_edge_transports("/t"),
            vec![
                ("fast/in".into(), Some(0.0)),
                ("slow/in".into(), Some(10.0))
            ]
        );
        assert_eq!(m.topic_transport_ms("/t"), Some(10.0));
    }

    /// Per-subscriber values only (the topic key is deprecated): the hop is
    /// charged its largest subscriber's value, not nothing.
    #[test]
    fn per_subscriber_values_alone_are_read() {
        let m = parse_manifest_str(
            "version: 1\n\
             nodes:\n\
             \x20 p: { pub: { out: {} } }\n\
             \x20 a: { sub: { in: { max_transport: 56ms } } }\n\
             \x20 b: { sub: { in: { max_transport: 3ms } } }\n\
             \x20 c: { sub: { in: {} } }\n\
             topics:\n\
             \x20 /t: { type: T, pub: [p/out], sub: [a/in, b/in, c/in] }\n",
        )
        .unwrap();
        assert_eq!(m.topic_transport_ms("/t"), Some(56.0));
        assert_eq!(m.topic_edge_transports("/t")[2], ("c/in".into(), None));
    }
}
