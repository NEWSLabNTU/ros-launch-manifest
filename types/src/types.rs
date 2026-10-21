//! Core manifest types.

use serde::Serialize;
use std::collections::BTreeMap;

/// Argument declaration with optional type constraint.
#[derive(Debug, Clone, Default, Serialize)]
pub enum ArgDecl {
    /// Free string — no constraint (default).
    #[default]
    String,
    /// Boolean — only "true" or "false".
    Bool,
    /// Enum — one of the listed values.
    Choices(Vec<String>),
}

impl ArgDecl {
    /// Valid values for this arg type. None = unconstrained (free string).
    pub fn valid_values(&self) -> Option<Vec<&str>> {
        match self {
            ArgDecl::String => None,
            ArgDecl::Bool => Some(vec!["true", "false"]),
            ArgDecl::Choices(choices) => Some(choices.iter().map(|s| s.as_str()).collect()),
        }
    }
}

/// Top-level manifest.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Manifest {
    pub version: u32,
    /// The grammar release the file needs, as written in its `rlm:` header
    /// (`v0.1.49`, `0.1.49` or `>=0.1.49`; since v0.1.49). Checked against
    /// [`crate::GRAMMAR_VERSION`] before the body is read; `None` when the file
    /// states none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rlm: Option<String>,
    /// The extensions this contract uses (phase 91 E4), `use: { <name>:
    /// "<requirement>" }`, in the order written. A contract that uses one
    /// may write the keys it declares in the contexts it extends; without
    /// `use:` an unknown key stays an error.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub uses: Vec<UseDecl>,
    /// Manifest arguments — all mandatory. Values provided by scope args from record.json.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub args: BTreeMap<String, ArgDecl>,
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
    pub paths: BTreeMap<String, PathDecl>,
    /// Topics this manifest's subtree consumes from / produces to systems
    /// outside the loaded manifest tree (Issue #51). Each entry marks the
    /// listed FQN as expected-external on the named side; the
    /// `dangling-entity` rule skips that side when it would otherwise
    /// fire. An internal `pub:`/`sub:` declaration anywhere in the merged
    /// tree overrides the external mark.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub external_topics: BTreeMap<String, ExternalTopicDecl>,
    /// Hazards (phase 71): the one place a fault-tolerant time interval is
    /// written. FDTI and FRTI are derived from the guards' detectors and the
    /// reaction's route; the FTTI is physics and belongs to the hazard.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub hazards: BTreeMap<String, HazardDecl>,
    /// The severity scale `hazards.<h>.severity` is drawn from, ascending.
    /// Declared rather than baked in, so a team working to IEC 61508 or
    /// DO-178C names its own; absent means ISO 26262's
    /// `[QM, ASIL_A, ASIL_B, ASIL_C, ASIL_D]`. The first entry is "no
    /// safety requirement" and derives no criticality.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub severity_levels: Vec<String>,
    /// Named guard groups (phase 75). A function is what a set of topics
    /// together provide; a mode requires functions, and a hazard may guard
    /// one by name instead of repeating its members.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub functions: BTreeMap<String, FunctionDecl>,
    /// Operational modes (phase 75): what each requires, and the ordered
    /// ladder to fall to when it is lost.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub modes: BTreeMap<String, ModeDecl>,
    /// Project policy (phase 93): how the consumer treats a measured budget
    /// nothing supervises. `None` when the file states none (the default:
    /// a warning).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy: Option<Policy>,
    /// The platform terms of a supervision's detection delay (phase 93),
    /// stated once per contract. `None` when the file states none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supervision: Option<SupervisionPlatform>,
    // Where each budget comes from (phase 10 B7) is the `source` of its
    // [`crate::Bound`] (phase 90 G2); [`Manifest::budget_sources`] lists
    // them by dotted contract path.
}

/// `source:` on a bound (phase 10 B7): where the number comes from.
///
/// A measured budget is the largest value some runs observed, plus a margin;
/// it is NOT an upper bound, and a consumer must not present a total that
/// contains one as if it were.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BudgetSource {
    pub kind: SourceKind,
    /// `measured` only, and optional there: the observations the budget was
    /// sized from, which the consumer reproduces (`budget-rule`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub measured: Option<Measurement>,
    /// `derived` only, and optional there (phase 91 E1): what derived the
    /// value. An extension writes it when it fills a core bound; a contract
    /// may write one by hand, and the consumer reads the bound the same way
    /// either way. A bare `source: derived` carries none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub derived: Option<Box<Derivation>>,
}

/// The record of a derived value (phase 91 E1): the extension that
/// computed it, its identity, the inputs it read and the steps it took.
/// The consumer prints it (`derived by`) and never recomputes it; checking
/// that the record matches the extension is the extension system's job
/// (E4, E7).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Derivation {
    /// The extension, a dotted name (`vehicle.braking`).
    pub by: String,
    /// Its version, `MAJOR.MINOR.PATCH`.
    pub version: String,
    /// The sha256 of the extension's canonical ASCII form, 64 lowercase hex
    /// digits; `None` when the record does not say.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// The inputs read, name to value as written (`entry_speed: 8.33 m/s`),
    /// in the order written.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<(String, String)>,
    /// The steps, one line each, in order (`v_r = a^2/(2j) = 2.0833 m/s`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub trace: Vec<String>,
}

/// The provenances a budget can have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// Chosen: a hazard analysis, a decision, a configured parameter.
    Requirement,
    /// The observed maximum of runs plus a margin. Not a bound.
    Measured,
    /// Computed from other numbers of the contract or the image's parameters.
    Derived,
    /// An upper bound from an analysis (WCET, response time, link schedule).
    Analysed,
    /// Chosen inside a range the consumer derives from the rest of the
    /// contract (phase 93): a timer's period between what its executor can
    /// carry and what the hazards' intervals allow. The consumer refuses a
    /// value outside the range.
    Allocated,
}

impl SourceKind {
    pub const ALL: [SourceKind; 5] = [
        SourceKind::Requirement,
        SourceKind::Measured,
        SourceKind::Derived,
        SourceKind::Analysed,
        SourceKind::Allocated,
    ];

    /// The contract spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::Requirement => "requirement",
            SourceKind::Measured => "measured",
            SourceKind::Derived => "derived",
            SourceKind::Analysed => "analysed",
            SourceKind::Allocated => "allocated",
        }
    }
}

/// `supervise:` on a budget (phase 93): the runtime checks the budget while
/// the system runs, and a miss starts a fallback. The user states the
/// expectation and the fallback; the consumer derives the detection delay
/// and checks that the overrun case still fits the hazard's interval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Supervision {
    /// What a miss starts.
    #[serde(flatten)]
    pub reaction: SuperviseReaction,
    /// What checks the budget.
    pub mechanism: SuperviseMechanism,
    /// A settle's supervision only (v0.1.62): the input the platform sees
    /// standstill on, as `<node>/<subscriber>` of this manifest. The
    /// consumer derives how late standstill is seen from it (the input's
    /// period + its `max_transport`) and adds that to the supervised
    /// deadline. Refused on any other budget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observes: Option<String>,
}

/// What a supervised budget's miss starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SuperviseReaction {
    /// `reaction: <path>`: a path of the budget's own node (as
    /// `on_violation.reaction` names one), else a scope path of the manifest.
    /// The consumer resolves which.
    Reaction(String),
    /// `enters: <mode>`: the mode's `reaction:` scope path runs.
    Enters(String),
}

impl SuperviseReaction {
    /// The name written, path or mode.
    pub fn name(&self) -> &str {
        match self {
            SuperviseReaction::Reaction(n) | SuperviseReaction::Enters(n) => n,
        }
    }
}

/// What checks a supervised budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SuperviseMechanism {
    /// Checked in the executor's spin loop: catches a slow callback once it
    /// returns, not one that never does.
    Executor,
    /// An independent timer and thread: catches a slow callback and a stuck
    /// one.
    Timer,
}

impl SuperviseMechanism {
    pub const ALL: [SuperviseMechanism; 2] =
        [SuperviseMechanism::Executor, SuperviseMechanism::Timer];

    /// The contract spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            SuperviseMechanism::Executor => "executor",
            SuperviseMechanism::Timer => "timer",
        }
    }
}

impl std::fmt::Display for SuperviseMechanism {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a budget's evidence amounts to (phase 93). Derived from its
/// `source:` and `supervise:`, never written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BudgetLevel {
    /// A measured budget nothing supervises: an observation, not a bound.
    Unsupervised,
    /// The runtime checks the budget and a miss starts a fallback.
    Supervised,
    /// An analysed upper bound.
    Bound,
}

impl BudgetLevel {
    /// `source: analysed` -> bound; `supervise:` stated -> supervised;
    /// `source: measured` alone -> unsupervised; anything else (a
    /// requirement, a derived or allocated value, no source) -> `None`: not
    /// evidence about the implementation.
    pub fn of(source: Option<&BudgetSource>, supervise: Option<&Supervision>) -> Option<Self> {
        match (source.map(|s| s.kind), supervise) {
            (Some(SourceKind::Analysed), _) => Some(BudgetLevel::Bound),
            (_, Some(_)) => Some(BudgetLevel::Supervised),
            (Some(SourceKind::Measured), None) => Some(BudgetLevel::Unsupervised),
            _ => None,
        }
    }

    /// The name `--explain` prints.
    pub fn as_str(self) -> &'static str {
        match self {
            BudgetLevel::Unsupervised => "unsupervised",
            BudgetLevel::Supervised => "supervised",
            BudgetLevel::Bound => "bound",
        }
    }
}

impl std::fmt::Display for BudgetLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `policy:` (phase 93): project policy for the consumer's rules.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Policy {
    /// The severity of a measured budget nothing supervises
    /// (`measured-unsupervised`). `None`: the default, a warning.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unsupervised_measured: Option<PolicySeverity>,
}

/// A severity a policy sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PolicySeverity {
    #[default]
    Warn,
    Error,
}

/// `supervision:` (phase 93): the platform terms of a supervision's
/// detection delay, until platform profiles carry them.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SupervisionPlatform {
    /// The period the monitor's clock is read at: the executor's tick, the
    /// supervision timer's resolution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tick: Option<crate::duration::Duration>,
    /// The interrupt plus the thread switch a `timer` mechanism adds before
    /// its reaction runs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub switch: Option<crate::duration::Duration>,
}

impl std::fmt::Display for SourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The observations behind a measured budget: one or more terms, each sized
/// as `period + max x (1 + margin)` rounded up to the next millisecond, and
/// summed. A term with a `period` is a timer tick: the period is the
/// parameter the image runs and takes no margin; only the lateness beyond it
/// (`max`) does.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Measurement {
    /// The margin over the observed maximum, as a fraction (`0.2` = 20 %).
    pub margin: f64,
    pub terms: Vec<MeasuredTerm>,
}

/// One measured term of a budget.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MeasuredTerm {
    /// The term's name in the `terms:` map; `None` for the single-term form.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The largest value observed (for a timer tick: the largest lateness
    /// beyond `period`).
    pub max: crate::duration::Duration,
    /// How many samples the maximum is over.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u64>,
    /// A timer tick's period, charged without margin.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period: Option<crate::duration::Duration>,
}

impl MeasuredTerm {
    /// `period + max x (1 + margin)`, rounded up to the next millisecond.
    pub fn budget_ms(&self, margin: f64) -> f64 {
        let raw = self.period.map_or(0.0, |p| p.as_millis_f64())
            + self.max.as_millis_f64() * (1.0 + margin);
        // A product such as 50 x 1.2 lands a hair above 60 in binary; it is
        // 60, not 61.
        (raw - 1e-9).ceil()
    }
}

impl Measurement {
    /// The budget the rule gives: the sum of the terms' budgets.
    pub fn budget_ms(&self) -> f64 {
        self.terms.iter().map(|t| t.budget_ms(self.margin)).sum()
    }
}

/// The parameter that sets a timer's period (phase 10 B1), on the path's
/// own node: `trigger: { timer: { rate_hz: 10, param: update_rate } }` (a
/// rate in Hz) or `period_param: <name>` (a period in seconds). The binding
/// is the one a bound's `param` is (phase 90 G2): a
/// [`crate::ParamBinding::Own`], since the parameter is the path's node's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimerParam {
    /// The parameter holds the rate, Hz.
    Rate(crate::bound::ParamBinding),
    /// The parameter holds the period, seconds.
    Period(crate::bound::ParamBinding),
}

impl TimerParam {
    /// The parameter's name on the path's node.
    pub fn name(&self) -> &str {
        self.binding().name()
    }

    /// The parameter binding, whichever unit it holds.
    pub fn binding(&self) -> &crate::bound::ParamBinding {
        match self {
            TimerParam::Rate(b) | TimerParam::Period(b) => b,
        }
    }
}

/// One operational mode (phase 75, `docs/design/operational-modes.md`).
///
/// A mode is available while every function it `requires` holds. Losing one
/// walks `fallback` in order and takes the first rung that is still
/// available; the rung's `reaction` is the scope path reaching its safe
/// state. Autoware's diagnostic graph is exactly this shape — `and`/`or`
/// over diag units, modes over functions — and `mrm_handler` walks the
/// ladder at runtime.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ModeDecl {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Function (or bare topic) names this mode needs.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
    /// Modes to fall to, in order, when this one is lost.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fallback: Vec<String>,
    /// The scope path that reaches this mode's safe state, when it is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reaction: Option<String>,
    /// Requirement values that apply IN THIS MODE, pinned over the scalar
    /// declared elsewhere (`contract-axes.md` §3.3's mitigation cashed in:
    /// no requirement becomes a map).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<ModeOverride>,
    /// A timed, TRANSITIONAL rung (v0.1.46): the system stays in this mode
    /// `window` (a least-time bound, phase 90 G2) and then takes the next
    /// rung of the ladder, even while this one is still available. A takeover request is the
    /// case it exists for. A windowed rung is not a safe state, so it cannot
    /// be the floor of a ladder (`ladder-window-floor` in the consumer).
    ///
    /// `window: 10s` (a bare duration) parses as a window with no parameter
    /// -- UNBOUND: nothing in the image is tied to the number, which the
    /// consumer reports (`window-unbound`) rather than refuses. The map form
    /// `{ duration, param: <node>.<parameter>, source }` is a budget's
    /// (phase 90 G2); its `param` is a [`crate::ParamBinding::Node`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<crate::bound::Bound>,
    /// The one way out of a windowed rung that is not the next rung
    /// (v0.1.46): while this mode is active, the named function holding ends
    /// the ladder successfully in `to`. Only a windowed rung may carry one --
    /// a parse error otherwise -- which keeps general transitions out of the
    /// grammar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit: Option<ModeExit>,
}

/// A parameter named across the contract: `<node>.<parameter>`, split at
/// the FIRST dot. A ROS node name cannot contain a dot and a parameter name
/// can (`turning_hazard_on.emergency`), so the split is unambiguous.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParamRef {
    /// The node's name as the contract's `nodes:` map spells it.
    pub node: String,
    /// The parameter name, as the node's `params:` map spells it.
    pub name: String,
}

impl std::fmt::Display for ParamRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.node, self.name)
    }
}

/// `modes.<m>.exit` (v0.1.46): `{ holds: <function>, to: <mode> }`.
///
/// v0.1.59 renamed `on:` to `holds:`, so that `on` names fault classes only
/// (`hazards.<h>.on`, `on_violation.on`). `on:` still parses for one release
/// and sets [`Self::spelled_on`], which the consumer reports as deprecated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModeExit {
    /// The function whose holding ends the ladder -- typically a value
    /// function (`of:` + `when:`), such as "the control mode is MANUAL".
    pub holds: String,
    /// `true` when the contract wrote the deprecated `on:` instead of
    /// `holds:`.
    #[serde(skip)]
    pub spelled_on: bool,
    /// The mode the ladder ends in. Outside every `fallback:` list: an exit
    /// is not a rung (`mode-exit-target` in the consumer).
    pub to: String,
}

/// A named function (phase 75): a guard group, lost when its topics are,
/// and -- the `of:` + `when:` form, v0.1.46 -- also lost BY VALUE, when the
/// predicate holds on a sample of one of its topics.
///
/// The three older shapes (a bare topic, a list, `{ all_of: [...] }`) carry
/// no predicate: they are lost only by silence. A value function is lost by
/// either, because silence also means "not known to be in the state the
/// predicate excludes". Which fault removes which kind is the consumer's
/// ladder selection: a `reported` fault removes the value functions on its
/// topic, an omission, late or loss fault removes both kinds.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FunctionDecl {
    pub group: GuardGroup,
    /// Present only for the `{ of: [...], when: {...} }` form.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<ValuePredicate>,
}

/// A predicate on ONE scalar field of a topic's message (v0.1.46):
/// `when: { field: autonomous, equals: false }`.
///
/// Deliberately the smallest thing that names a value fault: one field (a
/// dotted path reaches into nested messages), one operator, one constant.
/// There are no conjunctions -- a second condition is a second function. The
/// contract describes the predicate the application evaluates; it never
/// generates it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ValuePredicate {
    /// The field, a dotted path from the message root (`autonomous`,
    /// `status.mode`). Checked against the `.msg` definition when the
    /// consumer can resolve it (`when-field-unknown`).
    pub field: String,
    pub op: CompareOp,
    pub value: PredicateValue,
}

/// The operator of a [`ValuePredicate`]. A closed set, written as the key
/// that carries the constant: `equals: false`, `lt: 16.7`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompareOp {
    Equals,
    NotEquals,
    Lt,
    Le,
    Gt,
    Ge,
}

impl CompareOp {
    /// Every operator, in the order the grammar documents them.
    pub const ALL: [CompareOp; 6] = [
        CompareOp::Equals,
        CompareOp::NotEquals,
        CompareOp::Lt,
        CompareOp::Le,
        CompareOp::Gt,
        CompareOp::Ge,
    ];

    /// The key the operator is written as.
    pub fn key(self) -> &'static str {
        match self {
            CompareOp::Equals => "equals",
            CompareOp::NotEquals => "not_equals",
            CompareOp::Lt => "lt",
            CompareOp::Le => "le",
            CompareOp::Gt => "gt",
            CompareOp::Ge => "ge",
        }
    }

    /// The operator as the mathematical symbol, for messages.
    pub fn symbol(self) -> &'static str {
        match self {
            CompareOp::Equals => "==",
            CompareOp::NotEquals => "!=",
            CompareOp::Lt => "<",
            CompareOp::Le => "<=",
            CompareOp::Gt => ">",
            CompareOp::Ge => ">=",
        }
    }

    /// An ordering operator needs a number: `lt: false` is meaningless.
    pub fn is_ordering(self) -> bool {
        matches!(
            self,
            CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge
        )
    }
}

/// The constant a [`ValuePredicate`] compares against.
///
/// An identifier in upper snake case (`MANUAL`, `NO_COMMAND`) is a MESSAGE
/// CONSTANT NAME, resolved against the `.msg` that declares the field; any
/// other string is a string literal. This is the ROS convention for
/// constants, so the rule costs nothing in a real message definition.
#[derive(Debug, Clone, PartialEq)]
pub enum PredicateValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Constant(String),
    Str(String),
}

impl PredicateValue {
    /// Classify a YAML string: upper snake case is a constant name.
    pub fn from_text(s: &str) -> Self {
        let mut chars = s.chars();
        let is_constant = chars.next().is_some_and(|c| c.is_ascii_uppercase())
            && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
        if is_constant {
            PredicateValue::Constant(s.to_string())
        } else {
            PredicateValue::Str(s.to_string())
        }
    }

    /// The value as a number, when it is one.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            PredicateValue::Int(i) => Some(*i as f64),
            PredicateValue::Float(f) => Some(*f),
            _ => None,
        }
    }
}

impl std::fmt::Display for PredicateValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PredicateValue::Bool(b) => write!(f, "{b}"),
            PredicateValue::Int(i) => write!(f, "{i}"),
            PredicateValue::Float(x) => write!(f, "{x}"),
            PredicateValue::Constant(c) => write!(f, "{c}"),
            PredicateValue::Str(s) => write!(f, "{s:?}"),
        }
    }
}

impl Serialize for PredicateValue {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            PredicateValue::Bool(b) => s.serialize_bool(*b),
            PredicateValue::Int(i) => s.serialize_i64(*i),
            PredicateValue::Float(x) => s.serialize_f64(*x),
            PredicateValue::Constant(c) | PredicateValue::Str(c) => s.serialize_str(c),
        }
    }
}

/// One `modes.<m>.overrides` entry: a requirement named by its contract
/// path, and the value it takes in this mode.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModeOverride {
    /// `paths.<name>.max_latency`, `nodes.<n>.sub.<e>.min_rate_hz`, …
    pub target: String,
    /// The value, as written. Typed at use against the target's own field.
    pub value: String,
}

/// A hazard: an outcome the system must react to in time (phase 71,
/// `docs/design/fault-reaction-primitives.md`).
#[derive(Debug, Clone, Default, Serialize)]
pub struct HazardDecl {
    /// Severity label from the team's HARA (`ASIL_D`, `SIL_3`, …). Consumed,
    /// never computed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// What is watched. Each group is one fault: a single topic, or an
    /// `all_of` set whose fault is the loss of EVERY member (a redundant
    /// pair). Any group faulting is the hazard.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub guards: Vec<GuardGroup>,
    /// Which fault classes the guards report, as a set: `on: omission` and
    /// `on: [omission, late]` both parse (phase 82). EMPTY means the key was
    /// omitted, which a consumer reads as "every class" -- the strictest
    /// reading, so declaring less never buys slack. An explicit `on: []` is
    /// a parse error, so empty is never ambiguous. Order is as written,
    /// duplicates dropped. `reported` names an application detector's
    /// output topic -- the fault is whatever that node checks.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub on: Vec<FaultKind>,
    /// The fault-tolerant time interval: fault → hazardous event, absent
    /// reaction. The only new number; `FDTI + FRTI` must fit inside it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ftti: Option<crate::bound::Bound>,
    /// Fault to detection, when that is physics rather than a detector's
    /// timeout (phase 91 E1): an occlusion builds pressure for minutes
    /// before any threshold can see it. The consumer charges it BEFORE the
    /// detection it derives from the detectors, so stating it never buys
    /// slack. Usually derived by an extension (`source: { derived: ... }`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detection: Option<crate::bound::Bound>,
    /// The check that is this hazard's requirement when the requirement is
    /// not a time (phase 91 E1): a safety distance, an end of authority.
    /// A hazard states `ftti`, `deadline_by`, or both.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline_by: Option<CheckRef>,
    /// Keys a used extension declares in `hazard` (phase 91 E4), as
    /// written; empty without `use:`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ext: Vec<ExtKey>,
    /// The mode this hazard enters (v0.1.59): the top of the fallback ladder
    /// it walks to a safe state. Exactly one of `enters` and `reaction` is
    /// written; the parser refuses both.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enters: Option<String>,
    /// The scope path whose route reaches the safe state: a one-rung
    /// reaction. Before v0.1.59 a hazard also wrote the MODE it enters here;
    /// that spelling still parses for one release, and the consumer tells
    /// the two apart by lookup (a mode of the hazard's scope first) and
    /// reports it as deprecated, naming `enters:`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reaction: Option<String>,
    /// The VALUE that is the fault (v0.1.46), for a hazard whose guard is a
    /// detector's output: `on: reported` says some node checks something,
    /// and this names what. Only meaningful with `reported` among the fault
    /// classes (`when-requires-reported` in the consumer).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<ValuePredicate>,
    /// The speed the vehicle may be travelling at when the fault occurs, in
    /// METRES PER SECOND (v0.1.46): the operating domain's speed bound. A
    /// plain number, positive. A `safe_state` whose settle is a braking
    /// profile derives its settle time from it, per hazard.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_speed: Option<f64>,
}

impl HazardDecl {
    /// The name the hazard's reaction starts from: the mode it `enters`, or
    /// the `reaction` it names. The consumer resolves it the one way it
    /// always has (a mode of the hazard's scope first, else a scope path),
    /// so the deprecated `reaction: <mode>` spelling keeps its meaning.
    pub fn entry(&self) -> Option<&String> {
        self.enters.as_ref().or(self.reaction.as_ref())
    }

    /// The key [`Self::entry`] was written under, for diagnostic paths:
    /// `enters` or `reaction`.
    pub fn entry_key(&self) -> &'static str {
        if self.enters.is_some() {
            "enters"
        } else {
            "reaction"
        }
    }
}

/// One entry of `use:` (phase 91 E4): an extension and the versions the
/// contract accepts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UseDecl {
    /// The extension, a dotted name (`vehicle.braking`).
    pub name: String,
    /// The requirement as written (`^1`, `~0.2`, `=1.0.0`, `>=1.0.0`).
    pub req: VersionReq,
}

/// A version requirement with cargo's meaning (phase 91 E4): `^1.2.3`
/// accepts `>=1.2.3, <2.0.0` (`<0.3.0` for `^0.2.3`, `<0.0.4` for `^0.0.3`),
/// `~1.2.3` accepts `>=1.2.3, <1.3.0`, `=1.2.3` that version only, `>=1.2.3`
/// that version or any later one. Missing components read as 0, so `^1` is
/// `^1.0.0` and `^0.1` is `^0.1.0`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(into = "String")]
pub struct VersionReq {
    pub op: ReqOp,
    pub major: u64,
    pub minor: Option<u64>,
    pub patch: Option<u64>,
}

/// The operator of a [`VersionReq`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReqOp {
    Caret,
    Tilde,
    Exact,
    AtLeast,
}

impl VersionReq {
    /// Parse `^1`, `~0.2.1`, `=1.0.0`, `>=1.0.0`; `None` when it is none of
    /// these.
    pub fn parse(text: &str) -> Option<VersionReq> {
        let t = text.trim();
        let (op, rest) = if let Some(r) = t.strip_prefix(">=") {
            (ReqOp::AtLeast, r)
        } else if let Some(r) = t.strip_prefix('^') {
            (ReqOp::Caret, r)
        } else if let Some(r) = t.strip_prefix('~') {
            (ReqOp::Tilde, r)
        } else {
            (ReqOp::Exact, t.strip_prefix('=')?)
        };
        let parts: Vec<&str> = rest.trim().split('.').collect();
        if parts.is_empty()
            || parts.len() > 3
            || parts
                .iter()
                .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
        {
            return None;
        }
        let num = |i: usize| parts.get(i).map(|p| p.parse::<u64>().ok());
        let major = num(0)??;
        let minor = match num(1) {
            None => None,
            Some(v) => Some(v?),
        };
        let patch = match num(2) {
            None => None,
            Some(v) => Some(v?),
        };
        if op == ReqOp::Exact && patch.is_none() {
            return None;
        }
        Some(VersionReq {
            op,
            major,
            minor,
            patch,
        })
    }

    /// Whether `version` (`MAJOR.MINOR.PATCH`) satisfies the requirement.
    pub fn matches(&self, version: &str) -> bool {
        let v: Vec<u64> = match version
            .split('.')
            .map(|p| p.parse::<u64>())
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(v) if v.len() == 3 => v,
            _ => return false,
        };
        let have = (v[0], v[1], v[2]);
        let lo = (self.major, self.minor.unwrap_or(0), self.patch.unwrap_or(0));
        match self.op {
            ReqOp::Exact => have == lo,
            ReqOp::AtLeast => have >= lo,
            ReqOp::Tilde => {
                let hi = match self.minor {
                    Some(m) => (self.major, m + 1, 0),
                    None => (self.major + 1, 0, 0),
                };
                have >= lo && have < hi
            }
            ReqOp::Caret => {
                let hi = if self.major > 0 || self.minor.is_none() {
                    (self.major + 1, 0, 0)
                } else if self.minor.unwrap_or(0) > 0 || self.patch.is_none() {
                    (0, self.minor.unwrap_or(0) + 1, 0)
                } else {
                    (0, 0, self.patch.unwrap_or(0) + 1)
                };
                have >= lo && have < hi
            }
        }
    }
}

impl std::fmt::Display for VersionReq {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let op = match self.op {
            ReqOp::Caret => "^",
            ReqOp::Tilde => "~",
            ReqOp::Exact => "=",
            ReqOp::AtLeast => ">=",
        };
        write!(f, "{op}{}", self.major)?;
        if let Some(m) = self.minor {
            write!(f, ".{m}")?;
        }
        if let Some(p) = self.patch {
            write!(f, ".{p}")?;
        }
        Ok(())
    }
}

impl From<VersionReq> for String {
    fn from(r: VersionReq) -> String {
        r.to_string()
    }
}

/// A key an extension declares, as the contract wrote it (phase 91 E4).
/// The contract layer cannot type it -- only the extension knows its
/// dimension -- so it keeps the text and the consumer converts it with the
/// extension's value checker.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExtKey {
    pub key: String,
    pub value: ExtValue,
}

/// The value of an [`ExtKey`]: a scalar's text (`30 km/h`, `target_jerk`,
/// `15ms`) or a map's entries in order (a table: `{ 0 km/h: 0.75 m/s^2 }`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ExtValue {
    Scalar(String),
    Map(Vec<(String, String)>),
}

/// `<extension>/<check>`: a check an extension declares (phase 91 E1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CheckRef {
    /// The extension, a dotted name (`machine.iso13855`).
    pub extension: String,
    /// The check's name inside it (`safety_distance`).
    pub check: String,
}

impl std::fmt::Display for CheckRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.extension, self.check)
    }
}

/// One guard: a topic, or a redundant set that faults only when all of its
/// members do.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GuardGroup {
    pub members: Vec<String>,
    /// `true` for `all_of:`; `false` for a single topic (the degenerate
    /// all-of-one, spelled without the wrapper).
    pub all_of: bool,
}

/// What a detector detects. AADL EMV2's error-type library, restricted to
/// what the middleware can observe plus what a declared detector reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultKind {
    /// No message at all — liveliness lease expired.
    Omission,
    /// A message arrived, too late — QoS deadline or `max_age` violated.
    Late,
    /// Consecutive losses past `drop.max_consecutive`.
    Loss,
    /// An application detector published on the guarded topic; the fault is
    /// whatever it checks (covariance, plausibility, a value out of range).
    Reported,
    /// A supervised budget was overrun (phase 93): what a `supervise:`
    /// detects, for traces and verdicts. Not written in an `on:` list.
    Overrun,
}

/// Where the runtime observer reads a violation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectMechanism {
    /// The subscription's QoS event callback (`on_deadline_missed`,
    /// `on_liveliness_changed`).
    #[default]
    Qos,
    /// A `diagnostic_updater` status on `/diagnostics`.
    Diagnostics,
    /// The reaction path's own trigger — the node detects for itself.
    Application,
}

/// The reaction edge (phase 71): declared on the SUBSCRIBER that detects,
/// because that is the party that times out and reacts — AUTOSAR WdgM's
/// "EXPIRED → configured reaction", the `cmd_vel` timeout of every mobile
/// base.
#[derive(Debug, Clone, Default, Serialize)]
pub struct OnViolation {
    /// Which fault classes trigger this reaction.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub on: Vec<FaultKind>,
    /// A path on THIS node whose trigger includes the subscription.
    pub reaction: String,
    /// This hop's share of the fault reaction time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub within: Option<crate::bound::Bound>,
    #[serde(default)]
    pub mechanism: DetectMechanism,
}

/// What a reaction path produces (phase 71): the topic it commands the safe
/// state on, and how long the plant takes to reach it — the one term of the
/// reaction time the software cannot derive. Expected to be measured.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SafeState {
    pub emits: String,
    /// A written settle time: `settle: 2s`, shorthand for `settle: {
    /// duration: 2s }` (phase 90 G6), whose map form may also state its
    /// `source:`. With a [`SettleProfile`] beside it, the written duration is
    /// what the consumer falls back to when the hazard states no entry
    /// speed, and what it compares the derivation against
    /// (`settle-conflict`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settle: Option<crate::bound::Bound>,
    /// The settle DERIVED from a braking profile (v0.1.46):
    /// `settle: { decel: <param>, jerk: <param> }`, the parameters by name
    /// on this path's node, and the entry speed from the hazard.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settle_profile: Option<SettleProfile>,
    /// The vehicle's response (phase 10 B2; phase 90 G6 made it the
    /// settle's, not the profile's): `settle: { ..., response: <d> }`, the
    /// time from the braking command to the deceleration the plant then
    /// holds. Added to the settle, whether the settle is a written duration
    /// or derived from a profile; omitted means 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settle_response: Option<crate::bound::Bound>,
    /// The settle was written in its map form (`settle: { ... }`), so its
    /// bounds are keyed `settle.duration` and `settle.response`; the scalar
    /// `settle: 2s` is keyed `settle`.
    #[serde(skip)]
    pub settle_map: bool,
    /// `settle: { ..., supervise: { reaction | enters, mechanism, observes }
    /// }` (v0.1.62): the WHOLE settle is supervised, from the safe state's
    /// command to standstill: the derived profile (or the written
    /// duration) plus the response. The platform arms due = command +
    /// settle + how late it sees standstill, and a vehicle not at
    /// standstill by then starts the reaction. v0.1.61 kept a supervised
    /// written duration on its own bound; it lives here now, for either
    /// form.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settle_supervise: Option<Supervision>,
    /// Keys a used extension declares in `settle` (phase 91 E4), as
    /// written; empty without `use:`. With any, the settle may state
    /// neither a duration nor a profile: an extension derives it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub settle_ext: Vec<ExtKey>,
}

impl SafeState {
    /// The vehicle's response in milliseconds: 0 when not written.
    pub fn response_ms(&self) -> f64 {
        use crate::bound::OptBound;
        self.settle_response.ms().unwrap_or(0.0)
    }

    /// The written settle in milliseconds, with the response added: what a
    /// written duration charges (`None` when no duration is written).
    pub fn written_settle_ms(&self) -> Option<f64> {
        use crate::bound::OptBound;
        self.settle.ms().map(|d| d + self.response_ms())
    }
}

/// A braking profile named by the parameters that implement it (v0.1.46).
///
/// The node ramps its deceleration at `jerk` up to `decel` and holds it until
/// standstill. Both are read as magnitudes, so Autoware's signed spellings
/// (`target_acceleration: -2.5`, `target_jerk: -1.5`) name them unchanged.
/// [`SettleProfile::settle_s`] is the arithmetic.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SettleProfile {
    /// The parameter holding the deceleration, m/s^2.
    pub decel: String,
    /// The parameter holding the jerk the deceleration ramps at, m/s^3.
    pub jerk: String,
}

impl SettleProfile {
    /// Seconds from the braking command to standstill, from entry speed
    /// `v0` (m/s), deceleration `decel` (m/s^2) and jerk `jerk` (m/s^3),
    /// both taken as magnitudes.
    ///
    /// With a = |decel| and j = |jerk|, the ramp to full deceleration takes
    /// a/j and sheds `v_r = a^2 / (2 j)`. A vehicle slower than that stops
    /// inside the ramp: `t = sqrt(2 v0 / j)`. Otherwise it finishes at full
    /// deceleration: `t = a/j + (v0 - v_r) / a`. `None` when either
    /// magnitude is zero or a value is not finite: no profile, no number.
    pub fn settle_s(v0: f64, decel: f64, jerk: f64) -> Option<f64> {
        let (a, j) = (decel.abs(), jerk.abs());
        if !(v0.is_finite() && a.is_finite() && j.is_finite()) || a == 0.0 || j == 0.0 || v0 < 0.0 {
            return None;
        }
        let v_r = a * a / (2.0 * j);
        Some(if v0 <= v_r {
            (2.0 * v0 / j).sqrt()
        } else {
            a / j + (v0 - v_r) / a
        })
    }
}

/// Node declaration.
#[derive(Debug, Clone, Default, Serialize)]
pub struct NodeDecl {
    #[serde(rename = "if", skip_serializing_if = "Option::is_none")]
    pub if_condition: Option<String>,
    #[serde(rename = "unless", skip_serializing_if = "Option::is_none")]
    pub unless_condition: Option<String>,
    /// True if this is a ROS 2 lifecycle (managed) node. Rate, latency,
    /// and age contracts on this node's endpoints are runtime-gated on
    /// the node being in the Active state — publishers don't publish
    /// and subscribers don't run callbacks in Unconfigured/Inactive.
    /// Static checking is unaffected; this is a signal for runtime
    /// monitors to skip checks until the node is Active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<bool>,
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
    /// Platform-agnostic scheduling criticality — a CLOSED set,
    /// `high` | `medium` | `low`, rejected with a parse error otherwise
    /// (phase 70: `urgent` used to schedule a node as if nothing had been
    /// declared, via a debug log). No priority numbers; a `SchedMapper`
    /// reads it as a rank.
    ///
    /// Since phase 72 the label is a CONSEQUENCE rather than a free
    /// declaration: a node that feeds, detects or reacts to a hazard takes
    /// that hazard's severity (max over hazards, mapped by
    /// [`crate::severity::criticality_of`]), and `sched_derive` reads the
    /// derivation BEFORE any label. Since v0.1.59 a label on a node a hazard
    /// reaches is an error in the consumer (`derivable-criticality` when it
    /// agrees, `criticality-mismatch` when it does not; the fix is to delete
    /// it). The key stays live for the underivable case: a node no hazard
    /// reaches keeps its label, and it is the scheduler mapper's input there.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub criticality: Option<String>,
    /// Which of this node's paths may NOT run concurrently (phase 67).
    ///
    /// Absent means **every path of this node serializes**, which is what both
    /// realizations already default to — `rclcpp`'s implicit per-node callback
    /// group is `MutuallyExclusive`, and embedded executors commonly default
    /// the same way. So nothing is written unless an author claims MORE
    /// concurrency than the safe answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<ConcurrencyDecl>,
    /// The parameters this node declares, keyed by parameter name.
    ///
    /// Names and ROS 2 types only. The contract describes the node, and a
    /// capacity for a string or an array describes the board it is built
    /// for, so size bounds are deliberately NOT here. A map of
    /// `{ type: ... }` rather than a bare type so later fields (`read_only`,
    /// a description) are additive.
    ///
    /// Three states, and they stay distinct all the way to the model:
    ///
    /// - `None` -- no `params:` key: "not stated". The node keeps today's
    ///   behaviour everywhere, and a consumer that needs every node's
    ///   declarations (a store sized from them) must refuse, not guess.
    /// - `Some(empty)` -- `params: {}`: "declares no parameters". A statement,
    ///   so a launch value addressed to this node is undeclared.
    /// - `Some(names)` -- the parameters it declares.
    ///
    /// Serialized only when `Some`, so `{}` round trips as `{}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<BTreeMap<String, ParamDecl>>,
}

/// One `nodes.<n>.params.<name>` entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParamDecl {
    /// The ROS 2 parameter type. Required: a declaration without one says
    /// nothing a consumer could size or check.
    #[serde(rename = "type")]
    pub ty: ParamType,
}

/// The ROS 2 parameter types (`rcl_interfaces/msg/ParameterType`, less
/// `NOT_SET`). A closed set: an unknown spelling is a parse error, never a
/// skip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamType {
    Bool,
    Integer,
    Double,
    String,
    ByteArray,
    BoolArray,
    IntegerArray,
    DoubleArray,
    StringArray,
}

impl ParamType {
    /// Every type, in `rcl_interfaces` order.
    pub const ALL: [ParamType; 9] = [
        ParamType::Bool,
        ParamType::Integer,
        ParamType::Double,
        ParamType::String,
        ParamType::ByteArray,
        ParamType::BoolArray,
        ParamType::IntegerArray,
        ParamType::DoubleArray,
        ParamType::StringArray,
    ];

    /// The contract spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            ParamType::Bool => "bool",
            ParamType::Integer => "integer",
            ParamType::Double => "double",
            ParamType::String => "string",
            ParamType::ByteArray => "byte_array",
            ParamType::BoolArray => "bool_array",
            ParamType::IntegerArray => "integer_array",
            ParamType::DoubleArray => "double_array",
            ParamType::StringArray => "string_array",
        }
    }
}

impl std::fmt::Display for ParamType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ParamType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ParamType::ALL
            .into_iter()
            .find(|t| t.as_str() == s)
            .ok_or(())
    }
}

/// Publisher/subscriber endpoint properties (all optional).
#[derive(Debug, Clone, Default, Serialize)]
pub struct EndpointProps {
    /// A rate bound: the publisher's promise, or the subscriber's need.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_rate_hz: Option<crate::bound::Bound>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_rate_hz: Option<crate::bound::Bound>,
    /// Sub endpoint: max data age at receive (now - header.stamp), runtime-checked.
    /// Its `param` (phase 10 B1, `max_age: { duration: 500ms, param: <name> }`)
    /// is the parameter of this node, in seconds, whose resolved launch value
    /// must equal it (`max-age-param` in the consumer): a
    /// [`crate::ParamBinding::Own`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_age: Option<crate::bound::Bound>,
    /// Sub endpoint: read-latest, not causal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<bool>,
    /// Sub endpoint: must receive at least once before operational.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    /// Per-endpoint QoS override. Field-level merge with topic-level
    /// `qos:` — fields declared here override the topic default; fields
    /// not declared here inherit from the topic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qos: Option<QosDecl>,
    /// Sub endpoint: per-subscriber transport latency override (ms).
    /// Used as the edge weight from publisher to this subscriber in
    /// scope-path critical-path computation. Inherits topic-level
    /// `max_transport_ms` when not declared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_transport: Option<crate::bound::Bound>,
    /// Sub endpoint: buffering discipline for a `state: true` subscription
    /// (Vocabulary v2, Phase 44.1 §3). `latest` (default) — staleness is
    /// the failure mode; `queue` — backlog is the failure mode, drained
    /// batch-wise by the consuming timer. Only meaningful alongside
    /// `state: true`; parse-time error otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffer: Option<Buffer>,
    /// The reaction this subscriber owes when its assumption is violated
    /// (phase 71). Subscribers only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_violation: Option<OnViolation>,
    /// Publisher only: this endpoint publishes when asked (a service call,
    /// an operator's command) and promises NO rate (since v0.1.49). Not the
    /// same as leaving `min_rate_hz` out, which says nothing:
    /// `on_demand: true` is a statement a consumer acts on -- no minimum
    /// rate is derived for the topic, no rate monitor is generated for the
    /// endpoint, and a subscriber that requires a rate of it is an error.
    /// The parser refuses it beside `min_rate_hz` and on any side but
    /// `pub:`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_demand: Option<bool>,
    /// Subscriber only: what this endpoint is made to ABSORB (since v0.1.64,
    /// nano-ros RFC-0074 / issue 0760) -- a token bucket of `rate_hz` with
    /// `burst` messages back to back. Not a QoS policy and not a rate the
    /// subscriber needs (`min_rate_hz` is that): it bounds the work the
    /// transport may hand the device, and a consumer enforces it on the
    /// device's read path and at the router. Both keys are required, so an
    /// unstated burst can never read as unbounded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ingress: Option<IngressDecl>,
    /// Subscriber only (since v0.1.68, nano-ros RFC-0108 D1): the least time
    /// between two arrivals at this input. With it the callback is a
    /// SPORADIC task (T_min); without it, or a rate bound, or a `budget`, an
    /// event input's arrivals are unbounded and no response-time analysis
    /// can bound the interference its callback adds. Not the same statement
    /// as `max_rate_hz`: a rate bounds arrivals per second on average, a
    /// minimum inter-arrival bounds every pair. Refused under `pub:`/`cli:`,
    /// and when it exceeds `1 / min_rate_hz` (the input would have to arrive
    /// both at least that often and at most that often).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_interarrival: Option<crate::duration::Duration>,
    /// Subscriber only (since v0.1.68, nano-ros RFC-0108 D1): the execution
    /// budget Q the callback may use per `budget_period` P. With it the
    /// callback is SERVED: put behind a server (sporadic, deferrable or
    /// constant-bandwidth) and analysed as a periodic task (Q, P). Requires
    /// `budget_period`; refused when Q exceeds P.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget: Option<crate::duration::Duration>,
    /// The server period P of [`Self::budget`] (since v0.1.68). Required by,
    /// and only meaningful beside, `budget`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_period: Option<crate::duration::Duration>,
    /// Client only (since v0.1.69, nano-ros RFC-0108 D8.3): whether the
    /// code waits for the response inside the callback that sends the
    /// request (`blocking`) or sends it and drains the response on a later
    /// release (`async`). Absent means not stated: a consumer that checks
    /// blocking calls across preemption levels checks only a stated one,
    /// and reports an unstated one. Refused under `pub:`/`sub:`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call: Option<CallMode>,
}

/// How a client waits for its response (`cli.<e>.call`, and
/// `actions.<a>.call`; since v0.1.69).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CallMode {
    /// The callback that sends the request waits for the response. Under a
    /// preemptive executor the server must run at a more urgent level, or
    /// the wait is a priority inversion (a less urgent server) or a
    /// deadlock (a server at the client's own level).
    Blocking,
    /// The request is sent and the response is taken by a later callback;
    /// the sender never waits.
    Async,
}

/// A subscriber's ingress bound, `sub.<e>.ingress: { rate_hz, burst }`
/// (since v0.1.64). See [`EndpointProps::ingress`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct IngressDecl {
    /// Sustained messages per second the endpoint absorbs. Finite and > 0.
    pub rate_hz: f64,
    /// Messages that may arrive back to back before pacing applies. >= 1.
    pub burst: u32,
}

/// Buffering discipline for a `state: true` subscriber endpoint
/// (Vocabulary v2, Phase 44.1 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Buffer {
    /// Read-latest; staleness is the failure mode (today's implicit
    /// semantics — the default when `buffer:` is unset).
    Latest,
    /// Bounded queue, drained batch-wise by the consuming timer callback;
    /// backlog is the failure mode.
    Queue,
}

/// Service endpoint properties.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SrvEndpointProps {
    /// Max time from request to response (runtime monitoring only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_response: Option<crate::bound::Bound>,
    /// The least time between two requests (since v0.1.68, nano-ros
    /// RFC-0108 D1). See [`EndpointProps::min_interarrival`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_interarrival: Option<crate::duration::Duration>,
    /// The execution budget per `budget_period` (since v0.1.68). See
    /// [`EndpointProps::budget`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget: Option<crate::duration::Duration>,
    /// The server period of `budget` (since v0.1.68).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_period: Option<crate::duration::Duration>,
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
    /// Worst-case transport latency (ms) for this topic hop.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_transport: Option<crate::bound::Bound>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drop: Option<DropSpec>,
    /// Per-topic external override. When set, the matching side
    /// (`pub`/`sub`/`both`) is treated as expected-external — the
    /// `dangling-entity` rule skips it. Equivalent to a
    /// per-topic-scoped entry in the manifest's deprecated `external_topics:`
    /// block (phase 90 G3: this is the one place).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external: Option<ExternalSide>,
}

/// Side of a topic that is provided/consumed by an external system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ExternalSide {
    /// Producer is external. The local manifest tree may declare
    /// subscribers but no internal publisher is required.
    Pub,
    /// Consumer is external. The local manifest tree may declare
    /// publishers but no internal subscriber is required.
    Sub,
    /// Both producer and consumer are external — passthrough we don't
    /// model on either side.
    Both,
}

/// Side of a service or action that is provided/consumed by an external
/// system.
///
/// The sibling of [`ExternalSide`], and a separate type rather than a reuse:
/// a topic's sides are `pub`/`sub` and a service's are `server`/`client`, so
/// one enum would have to accept `pub` on a service, where it means nothing.
/// The vocabulary a contract is written in is the thing being checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ExternalEndpointSide {
    /// The server is external. The local manifest tree may declare clients
    /// and no internal server is required -- the pattern a client-only image
    /// has whenever its server is a separate process.
    Server,
    /// The client is external. Nothing in the tree calls this service; some
    /// other system does.
    Client,
    /// Both sides are external -- declared here only so a type or a QoS can
    /// be stated about a service this tree merely passes through.
    Both,
}

/// External-topic declaration (Issue #51). Marks an FQN as
/// expected-external from the declaring scope's subtree.
///
/// Deprecated in v0.1.59 (phase 90 G3): the `external_topics:` section is the
/// second place for what `topics.<name>.external` states. It still parses for
/// one release; the checker warns and `migrate` moves each entry under
/// `topics:`.
#[derive(Debug, Clone, Serialize)]
pub struct ExternalTopicDecl {
    /// Side that is externally provided. Required.
    #[serde(rename = "external")]
    pub side: ExternalSide,
    /// Optional ROS message type. When set, cross-checked against any
    /// internal `topics:` declaration of the same FQN by the
    /// `consistency` rule.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub msg_type: Option<String>,
    /// Optional QoS profile (offered by the external producer or
    /// requested by the external consumer). Cross-checked under the
    /// `qos-match` rule.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qos: Option<QosDecl>,
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
    /// Per-service external override, the counterpart of
    /// [`TopicDecl::external`]. When set, the matching side is treated as
    /// expected-external and `dangling-entity` skips it.
    ///
    /// Without this a client-only manifest could not be written at all: a
    /// service with clients and no server is an ERROR, not a warning, so the
    /// author's only options were to declare a server the image does not run
    /// or to leave the service out of the contract entirely.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external: Option<ExternalEndpointSide>,
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
    /// Per-action external override. Same meaning and same reason as
    /// [`ServiceDecl::external`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external: Option<ExternalEndpointSide>,
    /// How every client listed in `client` waits for its result (since
    /// v0.1.69). See [`EndpointProps::call`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call: Option<CallMode>,
}

/// Include declaration: an inline scope.
///
/// Carries its own condition, like every other declaration in this grammar
/// (issue #0043): a launch file puts `if=` on the `<group>` an inline include
/// comes from, so the contract has to be able to say the same thing.
/// [`crate::filter_manifest`] drops a whole include whose condition is false,
/// and the refs pointing into it, the way it drops a node.
///
/// The inline form IS a nested manifest, so `if:` sits at that manifest's
/// root. A root condition on a *standalone* manifest is refused by the parser
/// — there is no include entry there for it to belong to. The file-reference
/// form (`includes.<name>: { manifest: <file> }`) was removed in v0.1.59
/// (phase 90 G7): nothing loaded the file, so it declared nothing.
#[derive(Debug, Clone, Serialize)]
pub struct IncludeDecl {
    #[serde(rename = "if", skip_serializing_if = "Option::is_none")]
    pub if_condition: Option<String>,
    #[serde(rename = "unless", skip_serializing_if = "Option::is_none")]
    pub unless_condition: Option<String>,
    #[serde(flatten)]
    pub kind: IncludeKind,
}

impl IncludeDecl {
    /// An include with no condition on it.
    pub fn new(kind: IncludeKind) -> Self {
        Self {
            if_condition: None,
            unless_condition: None,
            kind,
        }
    }

    /// The inline manifest. An `Option` because the include had two forms
    /// before v0.1.59; it is always `Some` now.
    pub fn inline(&self) -> Option<&Manifest> {
        match &self.kind {
            IncludeKind::Inline(inner) => Some(inner),
        }
    }
}

/// The form of an [`IncludeDecl`]: since v0.1.59 only the inline one (the
/// file-reference form `External { manifest }` was removed, phase 90 G7).
///
/// Serialized untagged, and flattened into the entry by [`IncludeDecl`], so
/// the serialized shape is the shape the parser reads back.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum IncludeKind {
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
    #[serde(
        default,
        alias = "lifespan_ms",
        deserialize_with = "crate::duration::compat::opt_millis",
        skip_serializing_if = "Option::is_none"
    )]
    pub lifespan: Option<crate::duration::Duration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liveliness: Option<String>,
    /// `deadline` QoS — the maximum time DDS will allow between messages
    /// before it reports a violation (phase 67).
    ///
    /// The runtime-enforced counterpart of a subscriber's `min_rate_hz`, and
    /// one of only two ROS 2 QoS policies that turn a declared timing
    /// requirement into a callback. Enforcement is opt-in on the node's side:
    /// `rclcpp`'s `QosOverridingOptions()` defaults to "no overrides allowed",
    /// so a derived value can be applied only where the author opted in, and
    /// must be REPORTED rather than silently dropped where they did not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<crate::duration::Duration>,
    /// `liveliness` lease duration (phase 67).
    ///
    /// `liveliness` alone names a policy kind; the lease is the actual
    /// fault-detection interval, and is the closest thing this vocabulary has
    /// to ISO 26262's FDTI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_duration: Option<crate::duration::Duration>,
}

impl QosDecl {
    /// Compute the effective QoS for an endpoint by overlaying the
    /// endpoint's per-field overrides on top of the topic-level default.
    /// Each field follows: `endpoint.<f> ?? topic.<f>`. Fields unspecified
    /// on both sides remain `None` and are skipped by `qos-match`.
    pub fn effective(topic: Option<&QosDecl>, endpoint: Option<&QosDecl>) -> QosDecl {
        let pick_str = |f: fn(&QosDecl) -> Option<&String>| -> Option<String> {
            endpoint.and_then(f).or_else(|| topic.and_then(f)).cloned()
        };
        QosDecl {
            reliability: pick_str(|q| q.reliability.as_ref()),
            durability: pick_str(|q| q.durability.as_ref()),
            depth: endpoint
                .and_then(|q| q.depth)
                .or_else(|| topic.and_then(|q| q.depth)),
            history: pick_str(|q| q.history.as_ref()),
            lifespan: endpoint
                .and_then(|q| q.lifespan)
                .or_else(|| topic.and_then(|q| q.lifespan)),
            liveliness: pick_str(|q| q.liveliness.as_ref()),
            deadline: endpoint
                .and_then(|q| q.deadline)
                .or_else(|| topic.and_then(|q| q.deadline)),
            lease_duration: endpoint
                .and_then(|q| q.lease_duration)
                .or_else(|| topic.and_then(|q| q.lease_duration)),
        }
    }
}

/// Named causal path.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PathDecl {
    #[serde(rename = "if", skip_serializing_if = "Option::is_none")]
    pub if_condition: Option<String>,
    #[serde(rename = "unless", skip_serializing_if = "Option::is_none")]
    pub unless_condition: Option<String>,
    /// The legacy `input:` list, deprecated in v0.1.59 (phase 90 G3) for
    /// `trigger: { input: [...] }`; read through
    /// [`PathDecl::effective_trigger`]. Node paths: endpoint names. Scope
    /// paths: topic names (relative/absolute).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub input: Vec<String>,
    /// Node paths: endpoint names. Scope paths: topic names (relative/absolute).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub output: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_latency: Option<crate::bound::Bound>,
    /// Maximum `header.stamp` spread between the inputs of a fan-in path
    /// that the callback will still treat as one set. A window the
    /// synchroniser may wait for, so `sync-budget` checks it against
    /// `max_latency`.
    #[serde(
        default,
        alias = "tolerance_ms",
        deserialize_with = "crate::duration::compat::opt_millis",
        skip_serializing_if = "Option::is_none"
    )]
    pub tolerance: Option<crate::duration::Duration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drop: Option<DropSpec>,
    /// Explicit closed-taxonomy cause of this path's output (Vocabulary
    /// v2, Phase 44.1 §1). When absent, [`PathDecl::effective_trigger`]
    /// applies the legacy derivation from `input:`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<Trigger>,
    /// Fan-in synchronization policy for an input-triggered path with
    /// ≥2 endpoints (Vocabulary v2, Phase 44.1 §2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync: Option<Sync>,
    /// Tolerable variation in this path's contribution to end-to-end latency
    /// (phase 67).
    ///
    /// A *spread*, not a second budget: control stability depends on how much
    /// latency VARIES, not only on its maximum, which is why AADL treats
    /// latency jitter as a first-class analysis output rather than a
    /// by-product. Checking it needs a lower bound as well as an upper one —
    /// see [`PathDecl::min_latency`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_jitter: Option<crate::bound::Bound>,
    /// Best-case latency for this path (phase 67).
    ///
    /// Optional, and expected to be *measured* rather than authored in most
    /// cases — a measurement of the running system yields per-invocation
    /// distributions. It exists because every other bound this vocabulary
    /// carries is an upper one, so `max_jitter` would otherwise be
    /// unfalsifiable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_latency: Option<crate::bound::Bound>,
    /// What a missed deadline costs and what to do about it (phase 67).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub miss: Option<MissSpec>,
    /// What this path produces when it is a reaction (phase 71).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safe_state: Option<SafeState>,
    /// The parameter that sets this path's timer period (phase 10 B1),
    /// written inside `trigger: { timer: { ... } }` and kept here so the
    /// [`Trigger::Timer`] variant every consumer matches stays unchanged
    /// (`timer-param` in the consumer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timer_param: Option<TimerParam>,
    /// Where the timer's `rate_hz` comes from (phase 93), written in its
    /// map form `rate_hz: { value, source }` and kept here for the same
    /// reason as [`Self::timer_param`]. `None` for the bare number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timer_rate_source: Option<BudgetSource>,
}

impl PathDecl {
    /// The release jitter of this path's timer trigger (`trigger: { timer: {
    /// rate_hz, jitter } }`, v0.1.49), or `None` for any other trigger or a
    /// timer that states none. Kept off [`EffectiveTrigger`] on purpose:
    /// that enum's `Timer` variant is matched by every consumer, and a
    /// period is all most of them need.
    pub fn timer_jitter(&self) -> Option<&crate::bound::Bound> {
        match &self.trigger {
            Some(Trigger::Timer { jitter, .. }) => jitter.as_ref(),
            _ => None,
        }
    }

    /// Derive the trigger actually in effect for this path (Vocabulary
    /// v2, Phase 44.1 §1): an explicit `trigger:` always wins; otherwise
    /// a non-empty legacy `input:` list derives an input trigger (today's
    /// 75+ contracts parse identically under this rule); otherwise the
    /// path is [`EffectiveTrigger::Unclassified`] — no trigger fact is
    /// derived, never silently assumed to be a timer.
    pub fn effective_trigger(&self) -> EffectiveTrigger {
        if let Some(trigger) = &self.trigger {
            return match trigger {
                Trigger::Timer { rate_hz, .. } => EffectiveTrigger::Timer { rate_hz: *rate_hz },
                Trigger::Input(endpoints) => EffectiveTrigger::Input(endpoints.clone()),
                Trigger::Once => EffectiveTrigger::Once,
                Trigger::Spontaneous => EffectiveTrigger::Spontaneous,
            };
        }
        if !self.input.is_empty() {
            return EffectiveTrigger::Input(self.input.clone());
        }
        EffectiveTrigger::Unclassified
    }
}

/// Explicit closed taxonomy of what causes a path's output (Vocabulary
/// v2, Phase 44.1 §1). YAML forms:
///
/// ```yaml
/// trigger: { timer: { rate_hz: 10 } }
/// trigger: { input: [a, b] }
/// trigger: once
/// trigger: spontaneous
/// ```
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Trigger {
    /// Periodic self-clocked callback. `rate_hz` is a REAL scheduling
    /// input (mapper fact); must be > 0.
    ///
    /// `jitter` is the timer's RELEASE jitter (since v0.1.49): how
    /// late a tick may be released after its nominal instant, so that a
    /// message the path reads on its tick waits up to `1/rate_hz + jitter`.
    /// A fact about the executor and the board, measured, never derived;
    /// less than one period. Absent means the ticks are released on time
    /// (the reading before v0.1.49).
    Timer {
        rate_hz: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        jitter: Option<crate::bound::Bound>,
    },
    /// Output caused by these input endpoint/topic names. Rate is
    /// inherited from upstream, not authored here.
    Input(Vec<String>),
    /// Published once (startup latch); scheduling-irrelevant.
    Once,
    /// Caused outside the graph (operator, network, hardware); event-like,
    /// no upstream inheritance.
    Spontaneous,
}

/// The trigger actually in effect for a path — either the explicit
/// [`Trigger`] or the legacy-derived / unclassified fallback (Vocabulary
/// v2, Phase 44.1 §1). See [`PathDecl::effective_trigger`].
#[derive(Debug, Clone, PartialEq)]
pub enum EffectiveTrigger {
    Timer {
        rate_hz: f64,
    },
    Input(Vec<String>),
    Once,
    Spontaneous,
    /// No explicit `trigger:` and no (non-empty) legacy `input:` list —
    /// trigger facts are not derivable. Never silently assumed to be a
    /// timer.
    Unclassified,
}

/// Fan-in synchronization policy on an input-triggered path with ≥2
/// endpoints (Vocabulary v2, Phase 44.1 §2).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Sync {
    pub policy: SyncPolicy,
    /// Match window for `exact`/`approximate` (required for those
    /// policies).
    #[serde(
        default,
        alias = "max_interval_ms",
        deserialize_with = "crate::duration::compat::opt_millis",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_interval: Option<crate::duration::Duration>,
    /// Collect-until-timeout duration for `timeout_any` (required for
    /// that policy).
    #[serde(
        default,
        alias = "timeout_ms",
        deserialize_with = "crate::duration::compat::opt_millis",
        skip_serializing_if = "Option::is_none"
    )]
    pub timeout: Option<crate::duration::Duration>,
}

/// Fan-in sync matching policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncPolicy {
    /// message_filters ExactTime.
    Exact,
    /// message_filters ApproximateTime.
    Approximate,
    /// Collect-until-timeout, publish partial set.
    TimeoutAny,
}

/// Drop tolerance specification. Both are count bounds (phase 90 G2).
#[derive(Debug, Clone, Serialize)]
pub struct DropSpec {
    /// "N / W" format: max N drops per W messages
    /// ([`crate::Quantity::CountPer`]).
    pub max_count: Option<crate::bound::Bound>,
    /// Max consecutive drops ([`crate::Quantity::Count`]).
    pub max_consecutive: Option<crate::bound::Bound>,
}

/// Drop count: N drops per W messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DropCount {
    pub n: u32,
    pub w: u32,
}

/// Deadline-miss tolerance and handling (phase 67).
///
/// Deliberately **not** [`DropSpec`], though it borrows its spelling. A dropped
/// message and a late message are different failures with different
/// consequences, and a controller tolerates them differently; sharing the
/// author-facing shape helps, sharing the type would force one event's fields
/// onto the other.
#[derive(Debug, Clone, Serialize, Default)]
pub struct MissSpec {
    /// "N / W" format: at most N missed deadlines per W releases — the
    /// weakly-hard (m,K) model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tolerate: Option<crate::bound::Bound>,
    /// Maximum consecutive misses. Distinct from `tolerate`: a controller can
    /// survive a scattered 2-in-100 and be destabilised by 2 in a row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consecutive: Option<crate::bound::Bound>,
    /// What to do when a deadline IS missed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<MissAction>,
}

/// What happens to a job that misses its deadline.
///
/// The three from the weakly-hard literature. They are **not equally
/// realizable on Linux**, which is a fact about the platform rather than the
/// vocabulary: `Continue` is what CBS already does, while `SkipNext` and
/// `Abort` describe an obligation on the node — `rclcpp` offers no hook to
/// interrupt a callback mid-execution. A realizer that cannot deliver the
/// requested action must say so rather than silently substituting `Continue`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MissAction {
    /// Run the job to completion past its deadline.
    #[default]
    Continue,
    /// Discard the next release rather than the overrunning job.
    SkipNext,
    /// Abandon the overrunning job at its deadline.
    Abort,
}

/// Which of a node's paths may not run concurrently (phase 67).
///
/// An **exclusion relation**, from which callback groups are derived — a
/// maximal mutually exclusive set IS a group. The group is a consequence, so
/// it is never authored; a realizer may infer groups from causal coupling
/// and treat a declaration as an override.
#[derive(Debug, Clone, Serialize, Default)]
pub struct ConcurrencyDecl {
    /// Sets of path names that must not run concurrently with one another.
    ///
    /// An empty declaration is meaningful and is NOT the same as omitting the
    /// field: `concurrency: { exclusive: [] }` says every path of this node may
    /// run concurrently, while an absent `concurrency:` means none may.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclusive: Vec<Vec<String>>,
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

    /// The arithmetic of the island's contract comment block, reproduced:
    /// decel 2.5, jerk 1.5, so v_r = 2.0833 m/s. 3.0 m/s is past the ramp
    /// and gives 2033.33 ms; phase 7's observed 4.23 m/s gives 2525.33 ms.
    #[test]
    fn settle_arithmetic_reproduces_the_islands_numbers() {
        let ms = |v0: f64| SettleProfile::settle_s(v0, -2.5, -1.5).unwrap() * 1000.0;
        assert!((ms(3.0) - 2033.333).abs() < 0.01, "{}", ms(3.0));
        assert!((ms(4.23) - 2525.333).abs() < 0.01, "{}", ms(4.23));
        // Below v_r the vehicle stops inside the ramp: sqrt(2 v0 / j).
        let slow = SettleProfile::settle_s(1.5, 2.5, 1.5).unwrap();
        assert!((slow - (2.0f64 * 1.5 / 1.5).sqrt()).abs() < 1e-12);
        // The two branches meet at v_r: continuous, no jump.
        let v_r = 2.5 * 2.5 / 3.0;
        let a = SettleProfile::settle_s(v_r, 2.5, 1.5).unwrap();
        let b = SettleProfile::settle_s(v_r + 1e-9, 2.5, 1.5).unwrap();
        assert!((a - b).abs() < 1e-6);
        // No profile, no number.
        assert_eq!(SettleProfile::settle_s(3.0, 0.0, 1.5), None);
        assert_eq!(SettleProfile::settle_s(3.0, 2.5, f64::NAN), None);
    }
}
