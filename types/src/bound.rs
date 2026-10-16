//! One bound type (phase 90 G2).
//!
//! A contract writes a bound about fifteen ways: `max_latency`, `max_age`,
//! `max_transport`, `ftti`, a mode's `window`, a settle's `response`, a
//! drop count, a rate floor. Read by what they mean, every one of them is
//! the same statement: a value with a unit (a duration, a rate or a count),
//! what it limits (its [`BoundKind`]), from which side ([`Limit`]), where the
//! number comes from (`source:`), and optionally the parameter of the image
//! that must carry it (`param:`). [`Bound`] is that statement, and every
//! bound key parses into one.
//!
//! The keys keep their names: a key is a SPELLING of a bound in one place
//! of the grammar, and [`BOUND_KEYS`] lists each spelling with the kind it
//! parses to. That table is what the format reference prints under its
//! "Bounds" heading.

use crate::{
    duration::Duration,
    field_table::Context,
    types::{BudgetLevel, BudgetSource, DropCount, ParamRef, Supervision},
};
use serde::{Serialize, Serializer, ser::SerializeMap};

/// What a bound limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundKind {
    /// A path's (or scope path's) latency, input to output.
    Latency,
    /// The age of a sample at receive (`now - header.stamp`).
    Age,
    /// The transport of one hop, publisher to subscriber.
    Transport,
    /// A spread: a path's latency variation, a timer's release lateness.
    Jitter,
    /// The time something takes to answer: a service, the plant's settle,
    /// the vehicle's response to a braking command.
    Response,
    /// A message rate.
    Rate,
    /// A number of events: drops or misses, in a row or per window.
    Count,
    /// A time by which something must have happened (`ftti`, `within`).
    Deadline,
    /// A least time: a transitional rung stays at least this long (`window`).
    LeastTime,
}

impl BoundKind {
    pub const ALL: [BoundKind; 9] = [
        BoundKind::Latency,
        BoundKind::Age,
        BoundKind::Transport,
        BoundKind::Jitter,
        BoundKind::Response,
        BoundKind::Rate,
        BoundKind::Count,
        BoundKind::Deadline,
        BoundKind::LeastTime,
    ];

    /// The name the format reference prints.
    pub fn as_str(self) -> &'static str {
        match self {
            BoundKind::Latency => "latency",
            BoundKind::Age => "age",
            BoundKind::Transport => "transport",
            BoundKind::Jitter => "jitter",
            BoundKind::Response => "response",
            BoundKind::Rate => "rate",
            BoundKind::Count => "count",
            BoundKind::Deadline => "deadline",
            BoundKind::LeastTime => "least time",
        }
    }
}

impl std::fmt::Display for BoundKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which side a bound limits: an upper bound (`max_latency`, `ftti`) or a
/// lower one (`min_latency`, `min_rate_hz`, a window's least time).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Limit {
    /// At most the value.
    Max,
    /// At least the value.
    Min,
}

impl Limit {
    pub fn as_str(self) -> &'static str {
        match self {
            Limit::Max => "at most",
            Limit::Min => "at least",
        }
    }
}

/// A bound's value, with its unit.
#[derive(Debug, Clone, PartialEq)]
pub enum Quantity {
    /// A duration (`149ms`).
    Duration(Duration),
    /// A rate, Hz.
    Hz(f64),
    /// A number of events (`max_consecutive: 3`).
    Count(u32),
    /// A number of events per window of events (`"2 / 100"`).
    CountPer(DropCount),
}

impl Serialize for Quantity {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Quantity::Duration(d) => d.serialize(s),
            Quantity::Hz(v) => v.serialize(s),
            Quantity::Count(n) => n.serialize(s),
            Quantity::CountPer(c) => c.serialize(s),
        }
    }
}

/// The parameter of the image that must carry a bound's value.
///
/// One representation for the three ways a contract binds a parameter: a
/// bare name on the bound's own node (`max_age: { param: <name> }`, and a
/// timer's `trigger: { timer: { param | period_param } }`, whose
/// [`crate::TimerParam`] says whether the parameter holds Hz or seconds),
/// and `<node>.<parameter>` across the contract (`window: { param }`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamBinding {
    /// A parameter of the bound's own node (`max_age: { param: <name> }`).
    Own(String),
    /// A parameter named across the contract (`window: { param:
    /// <node>.<parameter> }`).
    Node(ParamRef),
}

impl ParamBinding {
    /// The parameter's name on its node.
    pub fn name(&self) -> &str {
        match self {
            ParamBinding::Own(n) => n,
            ParamBinding::Node(r) => &r.name,
        }
    }
    /// The parameter's node, when the binding names one.
    pub fn node(&self) -> Option<&str> {
        match self {
            ParamBinding::Own(_) => None,
            ParamBinding::Node(r) => Some(&r.node),
        }
    }
    /// The [`ParamRef`] of a `<node>.<parameter>` binding.
    pub fn as_ref(&self) -> Option<&ParamRef> {
        match self {
            ParamBinding::Own(_) => None,
            ParamBinding::Node(r) => Some(r),
        }
    }
}

impl std::fmt::Display for ParamBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParamBinding::Own(n) => f.write_str(n),
            ParamBinding::Node(r) => write!(f, "{r}"),
        }
    }
}

impl Serialize for ParamBinding {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

/// One bound, as any bound key states it.
#[derive(Debug, Clone, PartialEq)]
pub struct Bound {
    /// What it limits.
    pub kind: BoundKind,
    /// From which side.
    pub limit: Limit,
    /// The value, with its unit.
    pub value: Quantity,
    /// Where the number comes from (`source:` in the map form); `None` when
    /// the contract does not say.
    pub source: Option<BudgetSource>,
    /// The parameter that must carry the value (`param:` in the map form).
    pub param: Option<ParamBinding>,
    /// The runtime supervision of the value (`supervise:` in the map form,
    /// phase 93); `None` when the contract states none.
    pub supervise: Option<Supervision>,
}

impl Bound {
    /// A bound with only a value: what a bare spelling (`max_latency:
    /// 149ms`) states.
    pub fn new(kind: BoundKind, limit: Limit, value: Quantity) -> Self {
        Bound {
            kind,
            limit,
            value,
            source: None,
            param: None,
            supervise: None,
        }
    }

    /// What the budget's evidence amounts to (phase 93): see
    /// [`BudgetLevel::of`].
    pub fn level(&self) -> Option<BudgetLevel> {
        BudgetLevel::of(self.source.as_ref(), self.supervise.as_ref())
    }

    /// The value, when it is a duration.
    pub fn duration(&self) -> Option<Duration> {
        match self.value {
            Quantity::Duration(d) => Some(d),
            _ => None,
        }
    }

    /// The value in milliseconds, when it is a duration.
    pub fn ms(&self) -> Option<f64> {
        self.duration().map(Duration::as_millis_f64)
    }

    /// The value, when it is a rate.
    pub fn hz(&self) -> Option<f64> {
        match self.value {
            Quantity::Hz(v) => Some(v),
            _ => None,
        }
    }

    /// The value, when it is a plain count.
    pub fn count(&self) -> Option<u32> {
        match self.value {
            Quantity::Count(n) => Some(n),
            _ => None,
        }
    }

    /// The value, when it is a count per window.
    pub fn count_per(&self) -> Option<&DropCount> {
        match &self.value {
            Quantity::CountPer(c) => Some(c),
            _ => None,
        }
    }
}

/// A bound serializes as its contract spelling: the bare value, or the map
/// form `{ duration, param, source, supervise }` when it states more than
/// the value.
impl Serialize for Bound {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if self.source.is_none() && self.param.is_none() && self.supervise.is_none() {
            return self.value.serialize(s);
        }
        let mut m = s.serialize_map(None)?;
        m.serialize_entry("duration", &self.value)?;
        if let Some(p) = &self.param {
            m.serialize_entry("param", p)?;
        }
        if let Some(src) = &self.source {
            m.serialize_entry("source", src)?;
        }
        if let Some(sv) = &self.supervise {
            m.serialize_entry("supervise", sv)?;
        }
        m.end()
    }
}

/// Reading an optional bound: what a consumer of `Option<Bound>` asks.
pub trait OptBound {
    /// The value, when the bound is stated and is a duration.
    fn duration(&self) -> Option<Duration>;
    /// The value in milliseconds, when the bound is stated and is a duration.
    fn ms(&self) -> Option<f64>;
    /// The value, when the bound is stated and is a rate.
    fn hz(&self) -> Option<f64>;
    /// The value, when the bound is stated and is a plain count.
    fn count(&self) -> Option<u32>;
    /// The value, when the bound is stated and is a count per window.
    fn count_per(&self) -> Option<&DropCount>;
    /// The source, when the bound is stated and states one.
    fn source(&self) -> Option<&BudgetSource>;
    /// The parameter binding, when the bound is stated and states one.
    fn param(&self) -> Option<&ParamBinding>;
    /// The supervision, when the bound is stated and states one (phase 93).
    fn supervise(&self) -> Option<&Supervision>;
}

impl OptBound for Option<Bound> {
    fn duration(&self) -> Option<Duration> {
        self.as_ref().and_then(Bound::duration)
    }
    fn ms(&self) -> Option<f64> {
        self.as_ref().and_then(Bound::ms)
    }
    fn hz(&self) -> Option<f64> {
        self.as_ref().and_then(Bound::hz)
    }
    fn count(&self) -> Option<u32> {
        self.as_ref().and_then(Bound::count)
    }
    fn count_per(&self) -> Option<&DropCount> {
        self.as_ref().and_then(Bound::count_per)
    }
    fn source(&self) -> Option<&BudgetSource> {
        self.as_ref().and_then(|b| b.source.as_ref())
    }
    fn param(&self) -> Option<&ParamBinding> {
        self.as_ref().and_then(|b| b.param.as_ref())
    }
    fn supervise(&self) -> Option<&Supervision> {
        self.as_ref().and_then(|b| b.supervise.as_ref())
    }
}

/// What a bound key accepts beyond its bare value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// The bare value only.
    Bare,
    /// The bare value, or the map form `{ duration, source, supervise }`.
    Map,
    /// The bare value, or the map form `{ duration, source, supervise,
    /// param }`.
    MapParam,
    /// A settle (phase 90 G6): the bare duration is shorthand for `{
    /// duration }`, and the map is `{ duration | decel, jerk ; response ;
    /// source }`: a written duration or a braking profile it is derived
    /// from (or both, compared by `settle-conflict`), the vehicle's response
    /// added to either, and where the written duration comes from.
    Settle,
}

/// One spelling of a bound: a key in one context, and what it parses to.
#[derive(Debug, Clone, Copy)]
pub struct BoundKey {
    pub key: &'static str,
    pub context: Context,
    pub kind: BoundKind,
    pub limit: Limit,
    /// The unit the value carries: `duration`, `Hz`, `count` or
    /// `count / window`.
    pub unit: &'static str,
    pub form: Form,
    /// What the parameter binding names, for [`Form::MapParam`].
    pub param: &'static str,
}

const fn bk(
    key: &'static str,
    context: Context,
    kind: BoundKind,
    limit: Limit,
    unit: &'static str,
    form: Form,
) -> BoundKey {
    BoundKey {
        key,
        context,
        kind,
        limit,
        unit,
        form,
        param: "",
    }
}

/// Every bound key of the grammar, with the kind it parses to. The format
/// reference's "Bounds" section is this table.
pub const BOUND_KEYS: &[BoundKey] = &[
    bk(
        "max_latency",
        Context::Path,
        BoundKind::Latency,
        Limit::Max,
        "duration",
        Form::Map,
    ),
    bk(
        "min_latency",
        Context::Path,
        BoundKind::Latency,
        Limit::Min,
        "duration",
        Form::Bare,
    ),
    bk(
        "max_jitter",
        Context::Path,
        BoundKind::Jitter,
        Limit::Max,
        "duration",
        Form::Map,
    ),
    bk(
        "jitter",
        Context::TriggerTimer,
        BoundKind::Jitter,
        Limit::Max,
        "duration",
        Form::Map,
    ),
    bk(
        "min_rate_hz",
        Context::Endpoint,
        BoundKind::Rate,
        Limit::Min,
        "Hz",
        Form::Bare,
    ),
    bk(
        "max_rate_hz",
        Context::Endpoint,
        BoundKind::Rate,
        Limit::Max,
        "Hz",
        Form::Bare,
    ),
    BoundKey {
        param: "a parameter of the subscriber's node, seconds (`max-age-param`)",
        ..bk(
            "max_age",
            Context::Endpoint,
            BoundKind::Age,
            Limit::Max,
            "duration",
            Form::MapParam,
        )
    },
    bk(
        "max_transport",
        Context::Endpoint,
        BoundKind::Transport,
        Limit::Max,
        "duration",
        Form::Map,
    ),
    bk(
        "max_transport",
        Context::Topic,
        BoundKind::Transport,
        Limit::Max,
        "duration",
        Form::Map,
    ),
    bk(
        "max_response",
        Context::SrvEndpoint,
        BoundKind::Response,
        Limit::Max,
        "duration",
        Form::Map,
    ),
    bk(
        "within",
        Context::OnViolation,
        BoundKind::Deadline,
        Limit::Max,
        "duration",
        Form::Map,
    ),
    bk(
        "ftti",
        Context::Hazard,
        BoundKind::Deadline,
        Limit::Max,
        "duration",
        Form::Map,
    ),
    bk(
        "detection",
        Context::Hazard,
        BoundKind::Latency,
        Limit::Max,
        "duration",
        Form::Map,
    ),
    BoundKey {
        param: "`<node>.<parameter>`, seconds (`window-param`)",
        ..bk(
            "window",
            Context::Mode,
            BoundKind::LeastTime,
            Limit::Min,
            "duration",
            Form::MapParam,
        )
    },
    bk(
        "settle",
        Context::SafeState,
        BoundKind::Response,
        Limit::Max,
        "duration",
        Form::Settle,
    ),
    bk(
        "duration",
        Context::Settle,
        BoundKind::Response,
        Limit::Max,
        "duration",
        Form::Bare,
    ),
    bk(
        "response",
        Context::Settle,
        BoundKind::Response,
        Limit::Max,
        "duration",
        Form::Map,
    ),
    bk(
        "max_count",
        Context::Drop,
        BoundKind::Count,
        Limit::Max,
        "count / window",
        Form::Bare,
    ),
    bk(
        "max_consecutive",
        Context::Drop,
        BoundKind::Count,
        Limit::Max,
        "count",
        Form::Bare,
    ),
    bk(
        "tolerate",
        Context::Miss,
        BoundKind::Count,
        Limit::Max,
        "count / window",
        Form::Bare,
    ),
    bk(
        "consecutive",
        Context::Miss,
        BoundKind::Count,
        Limit::Max,
        "count",
        Form::Bare,
    ),
];

/// The spelling `key` in `context`, when it is a bound key.
pub fn bound_key(context: Context, key: &str) -> Option<&'static BoundKey> {
    BOUND_KEYS
        .iter()
        .find(|b| b.context == context && b.key == key)
}

/// A [`Bound`] for the spelling `key` in `context`, holding `value`: what
/// the parser builds for that key, and what a consumer building a contract
/// in code writes (`spelled(Context::Path, "max_latency", d.into())`).
///
/// # Panics
/// When `key` is not a bound key in `context` (a row of [`BOUND_KEYS`]).
pub fn spelled(context: Context, key: &str, value: impl Into<Quantity>) -> Bound {
    let b = bound_key(context, key)
        .unwrap_or_else(|| panic!("`{key}` in {} is not a bound key", context.label()));
    Bound::new(b.kind, b.limit, value.into())
}

impl From<Duration> for Quantity {
    fn from(d: Duration) -> Self {
        Quantity::Duration(d)
    }
}

impl From<f64> for Quantity {
    /// A rate, Hz.
    fn from(v: f64) -> Self {
        Quantity::Hz(v)
    }
}

impl From<u32> for Quantity {
    fn from(n: u32) -> Self {
        Quantity::Count(n)
    }
}

impl From<DropCount> for Quantity {
    fn from(c: DropCount) -> Self {
        Quantity::CountPer(c)
    }
}

impl Bound {
    /// Whether this is a budget: a duration that something is charged
    /// against or must fit in. Every duration bound is one except a lower
    /// latency bound (`min_latency`), which is a floor a measurement meets.
    pub fn is_budget(&self) -> bool {
        self.duration().is_some() && !(self.kind == BoundKind::Latency && self.limit == Limit::Min)
    }
}

impl crate::types::Manifest {
    /// Every bound this manifest states, keyed by its dotted contract path
    /// from this manifest's root (`nodes.h.paths.call_mrm.max_latency`,
    /// `modes.tor.window`), in a stable order: hazards, modes, nodes (their
    /// paths, then `sub`, `pub`, `cli` and `srv` endpoints), topics, scope
    /// paths. An inline include's bounds are its own manifest's.
    pub fn bounds(&self) -> Vec<(String, &Bound)> {
        use crate::types::{EndpointProps, PathDecl};
        let mut out: Vec<(String, &Bound)> = Vec::new();
        fn put<'a>(out: &mut Vec<(String, &'a Bound)>, at: &str, key: &str, b: &'a Option<Bound>) {
            if let Some(b) = b {
                out.push((format!("{at}.{key}"), b));
            }
        }
        fn path<'a>(out: &mut Vec<(String, &'a Bound)>, at: &str, p: &'a PathDecl) {
            put(out, at, "max_latency", &p.max_latency);
            put(out, at, "min_latency", &p.min_latency);
            put(out, at, "max_jitter", &p.max_jitter);
            if let Some(crate::types::Trigger::Timer { jitter, .. }) = &p.trigger {
                put(out, at, "trigger.timer.jitter", jitter);
            }
            if let Some(d) = &p.drop {
                put(out, at, "drop.max_count", &d.max_count);
                put(out, at, "drop.max_consecutive", &d.max_consecutive);
            }
            if let Some(m) = &p.miss {
                put(out, at, "miss.tolerate", &m.tolerate);
                put(out, at, "miss.consecutive", &m.consecutive);
            }
            if let Some(ss) = &p.safe_state {
                if ss.settle_map {
                    put(out, at, "safe_state.settle.duration", &ss.settle);
                    put(out, at, "safe_state.settle.response", &ss.settle_response);
                } else {
                    put(out, at, "safe_state.settle", &ss.settle);
                }
            }
        }
        fn endpoint<'a>(out: &mut Vec<(String, &'a Bound)>, at: &str, e: &'a EndpointProps) {
            put(out, at, "min_rate_hz", &e.min_rate_hz);
            put(out, at, "max_rate_hz", &e.max_rate_hz);
            put(out, at, "max_age", &e.max_age);
            put(out, at, "max_transport", &e.max_transport);
            if let Some(ov) = &e.on_violation {
                put(out, at, "on_violation.within", &ov.within);
            }
        }
        for (h, d) in &self.hazards {
            put(&mut out, &format!("hazards.{h}"), "ftti", &d.ftti);
            put(&mut out, &format!("hazards.{h}"), "detection", &d.detection);
        }
        for (m, d) in &self.modes {
            put(&mut out, &format!("modes.{m}"), "window", &d.window);
        }
        for (n, nd) in &self.nodes {
            for (p, pd) in &nd.paths {
                path(&mut out, &format!("nodes.{n}.paths.{p}"), pd);
            }
            for (side, eps) in [
                ("sub", &nd.subscribers),
                ("pub", &nd.publishers),
                ("cli", &nd.cli),
            ] {
                for (ep, props) in eps {
                    endpoint(&mut out, &format!("nodes.{n}.{side}.{ep}"), props);
                }
            }
            for (ep, props) in &nd.srv {
                put(
                    &mut out,
                    &format!("nodes.{n}.srv.{ep}"),
                    "max_response",
                    &props.max_response,
                );
            }
        }
        for (t, d) in &self.topics {
            let at = format!("topics.{t}");
            put(&mut out, &at, "max_transport", &d.max_transport);
            if let Some(dr) = &d.drop {
                put(&mut out, &at, "drop.max_count", &dr.max_count);
                put(&mut out, &at, "drop.max_consecutive", &dr.max_consecutive);
            }
        }
        for (p, pd) in &self.paths {
            path(&mut out, &format!("paths.{p}"), pd);
        }
        out
    }

    /// Where each budget comes from (phase 10 B7): the `source:` of every
    /// bound written in its map form (`max_latency: { duration: 149ms,
    /// source: ... }`), keyed as [`Manifest::bounds`] keys it. A bound
    /// written as a bare value states no source and has no entry.
    ///
    /// [`Manifest::bounds`]: crate::types::Manifest::bounds
    ///
    /// A timer's `rate_hz` written in its map form (`rate_hz: { value,
    /// source }`, phase 93) is listed too, as `...trigger.timer.rate_hz`:
    /// it is not a bound, but its source is stated the same way.
    pub fn budget_sources(&self) -> std::collections::BTreeMap<String, &BudgetSource> {
        let mut out: std::collections::BTreeMap<String, &BudgetSource> = self
            .bounds()
            .into_iter()
            .filter_map(|(k, b)| b.source.as_ref().map(|s| (k, s)))
            .collect();
        for (n, nd) in &self.nodes {
            for (p, pd) in &nd.paths {
                if let Some(s) = &pd.timer_rate_source {
                    out.insert(format!("nodes.{n}.paths.{p}.trigger.timer.rate_hz"), s);
                }
            }
        }
        for (p, pd) in &self.paths {
            if let Some(s) = &pd.timer_rate_source {
                out.insert(format!("paths.{p}.trigger.timer.rate_hz"), s);
            }
        }
        out
    }

    /// Every supervised bound (phase 93), keyed as [`Manifest::bounds`]
    /// keys it.
    ///
    /// [`Manifest::bounds`]: crate::types::Manifest::bounds
    pub fn supervised(&self) -> Vec<(String, &Bound, &Supervision)> {
        self.bounds()
            .into_iter()
            .filter_map(|(k, b)| b.supervise.as_ref().map(|s| (k, b, s)))
            .collect()
    }

    /// Every supervised settle (v0.1.62), keyed
    /// `nodes.<n>.paths.<p>.safe_state.settle` or
    /// `paths.<p>.safe_state.settle`, with its safe state: the whole settle
    /// (written duration or derived profile, plus the response) is
    /// supervised.
    pub fn supervised_settles(&self) -> Vec<(String, &crate::types::SafeState, &Supervision)> {
        let mut out = Vec::new();
        for (n, nd) in &self.nodes {
            for (p, pd) in &nd.paths {
                if let Some(ss) = &pd.safe_state
                    && let Some(sv) = &ss.settle_supervise
                {
                    out.push((format!("nodes.{n}.paths.{p}.safe_state.settle"), ss, sv));
                }
            }
        }
        for (p, pd) in &self.paths {
            if let Some(ss) = &pd.safe_state
                && let Some(sv) = &ss.settle_supervise
            {
                out.push((format!("paths.{p}.safe_state.settle"), ss, sv));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field_table;

    /// Every bound spelling is a live key of its context: the Bounds table
    /// names no key the grammar refuses.
    #[test]
    fn every_bound_key_is_a_key_of_its_context() {
        for b in BOUND_KEYS {
            assert!(
                field_table::lookup(b.context, b.key).is_some(),
                "`{}` is not a key of {}",
                b.key,
                b.context.label()
            );
        }
    }

    /// A contract writing every bound key parses each into the bound its
    /// row of [`BOUND_KEYS`] gives, and [`Manifest::bounds`] lists them all.
    ///
    /// [`Manifest::bounds`]: crate::types::Manifest::bounds
    #[test]
    fn every_bound_key_parses_to_its_row() {
        let m = crate::parse_manifest_str(
            "hazards:\n  h: { guards: [/a], ftti: { duration: 1s, source: requirement }, detection: 1s }\n\
             modes:\n  m: { requires: [/a], window: { duration: 10s, param: n.t } }\n\
             topics:\n  /a: { type: std_msgs/msg/Empty, max_transport: 2ms, \
             drop: { max_count: 1 / 10, max_consecutive: 2 } }\n\
             nodes:\n  n:\n    pub:\n      o: { min_rate_hz: 10, max_rate_hz: 20 }\n\
             \x20   sub:\n      a: { max_age: { duration: 500ms, param: t }, max_transport: 3ms, \
             on_violation: { reaction: r, within: 20ms } }\n\
             \x20   srv:\n      s: { max_response: 5ms }\n\
             \x20   paths:\n      r:\n        input: [a]\n        output: [o]\n\
             \x20       max_latency: 50ms\n        min_latency: 1ms\n        max_jitter: 4ms\n\
             \x20       miss: { tolerate: 2 / 100, consecutive: 1 }\n\
             \x20       safe_state: { emits: o, settle: { decel: d, jerk: j, duration: 2s, \
             response: 300ms } }\n\
             \x20     t:\n        trigger: { timer: { rate_hz: 10, jitter: 5ms } }\n\
             \x20       output: [o]\n        safe_state: { emits: o, settle: 1s }\n",
        )
        .unwrap();
        let bounds = m.bounds();
        let spelled: Vec<(&str, &str)> = bounds
            .iter()
            .map(|(k, _)| {
                let (ctx, key) = k.rsplit_once('.').unwrap();
                (ctx, key)
            })
            .collect();
        for (path, b) in &bounds {
            let key = path.rsplit('.').next().unwrap();
            let row = BOUND_KEYS
                .iter()
                .find(|r| r.key == key && r.kind == b.kind && r.limit == b.limit);
            assert!(
                row.is_some(),
                "{path}: {:?} {:?} matches no row",
                b.kind,
                b.limit
            );
        }
        // Every row's key appears (settle twice: scalar and profile form).
        for row in BOUND_KEYS {
            assert!(
                spelled.iter().any(|(_, k)| *k == row.key),
                "`{}` was not parsed into a bound",
                row.key
            );
        }
        assert_eq!(bounds.len(), 21, "{spelled:?}");
        let get = |k: &str| bounds.iter().find(|(p, _)| p == k).unwrap().1;
        assert_eq!(
            get("modes.m.window").param,
            Some(ParamBinding::Node(ParamRef {
                node: "n".into(),
                name: "t".into()
            }))
        );
        assert_eq!(
            get("nodes.n.sub.a.max_age").param,
            Some(ParamBinding::Own("t".into()))
        );
        assert_eq!(get("nodes.n.pub.o.min_rate_hz").hz(), Some(10.0));
        assert_eq!(get("topics./a.drop.max_consecutive").count(), Some(2));
        assert_eq!(
            get("nodes.n.paths.r.miss.tolerate").count_per(),
            Some(&DropCount { n: 2, w: 100 })
        );
        let src = m.budget_sources();
        assert_eq!(src.keys().collect::<Vec<_>>(), ["hazards.h.ftti"]);
    }
}
