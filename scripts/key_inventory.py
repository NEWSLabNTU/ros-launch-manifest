#!/usr/bin/env python3
"""Generate docs/key-inventory.md: every contract key, who reads it, who writes it.

The grammar comes from types/src/field_table.rs (this checkout). For each
(context, key) row the script records:

  * kind and status, from the table;
  * since: the first release tag whose field_table.rs has the row (rows that
    were in the table when it was created are dated "<=" that tag);
  * readers: hits of the key's field-access patterns in the consumers'
    source (rlm's check, derive and sched crates; play_launch's resolver,
    its --explain tables and its runtime; nano-ros's code generation and the
    rest of its packages);
  * writers: occurrences of the key, in its context, in the contracts of
    play_launch, rlm, nano-ros and the safety island, walked with the same
    context rules the parser uses, plus the value shapes written.

Readers are found by grep, not by type analysis. A field name shared by
several structs (`on`, `reaction`, `drop`, `qos`, `all_of`, ...) is counted
for every context that spells it; the inventory marks those rows "shared"
and the analysis sections of the document resolve them by hand.

Usage:

    python3 scripts/key_inventory.py \
        --play-launch ~/repos/play_launch \
        --nano-ros <nano-ros checkout> \
        --island ~/repos/simple-autoware-safety-island \
        [--out docs/key-inventory.md]

Every input repository is read through `git ls-files` / `git show`, so build
trees and untracked files never count. Only the Python standard library and
PyYAML are needed.
"""

from __future__ import annotations

import argparse
import collections
import os
import re
import subprocess
import sys

import yaml

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TABLE = "types/src/field_table.rs"

# --------------------------------------------------------------------------
# The grammar


ROW = re.compile(
    r'\b(live|alias|removed|deprecated)\(\s*"([^"]+)",\s*Context::(\w+),\s*'
    r'(?:Kind::(\w+),\s*)?("(?:[^"\\]|\\.)*")(?:,\s*("(?:[^"\\]|\\.)*"))?',
    re.S,
)
LABEL = re.compile(r'Context::(\w+)\s*=>\s*"([^"]+)"')


def parse_table(src: str):
    body = src[src.index("pub const FIELDS") : src.index("\n];")]
    rows = []
    for fn, key, ctx, kind, text, doc in ROW.findall(body):
        text = text[1:-1].replace('\\"', '"')
        doc = doc[1:-1].replace('\\"', '"') if doc else ""
        rows.append(
            dict(
                status=fn,
                key=key,
                ctx=ctx,
                kind=kind or "",
                # `deprecated(key, ctx, replacement, doc)` (phase 90 G3): a
                # key still read for one release, its fact's place elsewhere.
                canonical=text if fn in ("alias", "deprecated") else "",
                doc=doc if fn == "deprecated" else ("" if fn == "alias" else text),
            )
        )
    return rows


def git(repo, *args):
    return subprocess.run(
        ["git", "-C", repo, *args], capture_output=True, text=True, check=False
    ).stdout


def since_map(rows):
    """First tag whose field table has each (ctx, key)."""
    tags = [t for t in git(HERE, "tag", "--sort=v:refname").split() if t.startswith("v")]
    first = {}
    oldest = None
    for t in tags:
        src = git(HERE, "show", f"{t}:{TABLE}")
        if "pub const FIELDS" not in src:
            continue
        if oldest is None:
            oldest = t
        for r in parse_table(src):
            first.setdefault((r["ctx"], r["key"]), t)
    out = {}
    for r in rows:
        t = first.get((r["ctx"], r["key"]))
        if t is None:
            out[(r["ctx"], r["key"])] = "unreleased"
        elif t == oldest:
            out[(r["ctx"], r["key"])] = f"<={t[1:]}"
        else:
            out[(r["ctx"], r["key"])] = t[1:]
    return out, oldest


# --------------------------------------------------------------------------
# Accepted forms (read from types/src/parse.rs by hand; one line per key
# that has more than one).

DUR = "duration"
BUDGET = "duration | {duration, source}"
FORMS = {
    ("Endpoint", "max_age"): "duration | {duration, param, source}",
    ("Endpoint", "max_transport"): BUDGET,
    ("Endpoint", "min_rate_hz"): "number",
    ("Endpoint", "max_rate_hz"): "number",
    ("SrvEndpoint", "max_response"): BUDGET,
    ("Topic", "max_transport"): BUDGET,
    ("Topic", "drop"): '"N / W" | {max_count, max_consecutive}',
    ("Path", "drop"): '"N / W" | {max_count, max_consecutive}',
    ("Path", "miss"): '"N / W" | {tolerate, consecutive, action}',
    ("Path", "max_latency"): BUDGET,
    ("Path", "max_jitter"): BUDGET,
    ("Path", "min_latency"): DUR,
    ("Path", "tolerance"): DUR,
    ("Path", "trigger"): "once | spontaneous | {timer: {...}} | {input: [...]}",
    ("Path", "input"): "list (legacy trigger)",
    ("TriggerTimer", "jitter"): BUDGET,
    ("Hazard", "ftti"): BUDGET,
    ("Hazard", "on"): "class | [classes]",
    ("Hazard", "guards"): "[name | {all_of: [...]}]",
    ("Hazard", "reaction"): "name",
    ("Mode", "reaction"): "name",
    ("OnViolation", "reaction"): "name",
    ("OnViolation", "within"): BUDGET,
    ("OnViolation", "on"): "class | [classes]",
    ("Mode", "window"): "duration | {duration, param, source}",
    ("SafeState", "settle"): "duration | {duration | decel, jerk ; response ; source}",
    ("Settle", "response"): BUDGET,
    ("Settle", "duration"): DUR,
    ("Manifest", "functions"): "per entry: [topics] | {all_of} | {of, when}",
    ("Budget", "source"): "requirement | measured | derived | analysed | {measured: {...}}",
    ("Settle", "source"): "requirement | measured | derived | analysed | {measured: {...}}",
}

# --------------------------------------------------------------------------
# Readers: field-access patterns per (context, key). Default is the field
# name with an optional unit suffix the model adds.

SHARED = "shared"  # marker: the pattern also matches another context's field


def default_pat(key):
    return [rf"\.{re.escape(key)}(?:_ms|_mps)?\b"]


PATTERNS = {
    ("Manifest", "version"): [r"\bmanifest\.version\b", r"\bmeta\.version\b"],
    ("Manifest", "rlm"): [r"\.rlm\b"],
    ("Manifest", "if"): [r"\.if_condition\b"],
    ("Manifest", "unless"): [r"\.unless_condition\b"],
    ("Manifest", "includes"): [r"\.includes\b", r"IncludeKind::"],
    ("Manifest", "external_topics"): [r"\.external_topics\b", r"\.externals\b"],
    ("Manifest", "severity_levels"): [r"\.severity_levels\b", r"severity_rank|severity_scale"],
    ("Node", "if"): [r"\.if_condition\b"],
    ("Node", "unless"): [r"\.unless_condition\b"],
    ("Node", "pub"): [r"\.publishers\b", r"\bpub_endpoints\b"],
    ("Node", "sub"): [r"\.subscribers\b", r"\bsub_endpoints\b"],
    ("Node", "srv"): [r"\.srv\b", r"\bsrv_endpoints\b"],
    ("Node", "cli"): [r"\.cli\b"],
    ("Node", "criticality"): [r"\.criticality\b", r"\bnode_criticality\b"],
    ("Node", "concurrency"): [r"\.concurrency\b", r"\bnode_concurrency\b"],
    ("Node", "params"): [r"\.params\b", r"\bnode_params\b"],
    ("Node", "paths"): [r"\.paths\b", r"\bnode_paths\b"],
    ("Endpoint", "max_age"): [r"\.max_age(?:_ms)?\b"],
    ("Endpoint", "state"): [r"\.state\b"],
    ("Endpoint", "required"): [r"\.required\b"],
    ("Endpoint", "buffer"): [r"\.buffer\b", r"\bBuffer(?:Contract)?::"],
    ("Topic", "if"): [r"\.if_condition\b"],
    ("Topic", "unless"): [r"\.unless_condition\b"],
    ("Topic", "type"): [r"\.msg_type\b"],
    ("Topic", "pub"): [r"\.publishers\b"],
    ("Topic", "sub"): [r"\.subscribers\b"],
    ("Topic", "external"): [r"\.external\b", r"\.externals\b", r"ExternalSide::"],
    ("ExternalTopic", "side"): [r"\.side\b", r"ExternalSide::"],
    ("ExternalTopic", "external"): [r"\.side\b"],
    ("ExternalTopic", "type"): [r"\.msg_type\b"],
    ("Service", "if"): [r"\.if_condition\b"],
    ("Service", "unless"): [r"\.unless_condition\b"],
    ("Service", "type"): [r"\.srv_type\b"],
    ("Service", "external"): [r"ExternalEndpointSide::", r"\.external\b"],
    ("Action", "if"): [r"\.if_condition\b"],
    ("Action", "unless"): [r"\.unless_condition\b"],
    ("Action", "type"): [r"\.action_type\b"],
    ("Action", "external"): [r"ExternalEndpointSide::", r"\.external\b"],
    ("IncludeExternal", "manifest"): [r"IncludeKind::External", r"\.external\(\)"],
    ("IncludeExternal", "if"): [r"\.if_condition\b"],
    ("IncludeExternal", "unless"): [r"\.unless_condition\b"],
    ("Arg", "type"): [r"ArgDecl::Bool\b", r"\.valid_values\(\)"],
    ("Arg", "choices"): [r"ArgDecl::Choices\b", r"\.valid_values\(\)"],
    ("Path", "if"): [r"\.if_condition\b"],
    ("Path", "unless"): [r"\.unless_condition\b"],
    ("Path", "trigger"): [r"\.trigger\b", r"EffectiveTrigger::"],
    ("Trigger", "timer"): [r"\bTrigger::Timer\b", r"\bEffectiveTrigger::Timer\b"],
    ("Trigger", "input"): [r"\bTrigger::Input\b", r"\bEffectiveTrigger::Input\b", r"\.effective_trigger\(\)"],
    ("TriggerTimer", "rate_hz"): [r"Trigger::Timer\s*\{[^}]*rate_hz"],
    ("TriggerTimer", "jitter"): [r"\btimer_jitter(?:_ms)?\b", r"Trigger::Timer\s*\{[^}]*jitter"],
    ("TriggerTimer", "param"): [r"\bTimerParam::Rate\b"],
    ("TriggerTimer", "period_param"): [r"\bTimerParam::Period\b"],
    ("Sync", "policy"): [r"\.policy\b", r"\bSyncPolicy::"],
    ("Drop", "max_count"): [r"\.max_count\b", r"\.max_drop_rate\b"],
    ("Miss", "action"): [r"\bMissAction::", r"\.action\b"],
    ("Param", "type"): [r"\bParamType::", r"\.ty\b"],
    ("Hazard", "guards"): [r"\.guards\b"],
    ("Hazard", "ftti"): [r"\.ftti(?:_ms)?\b"],
    ("Hazard", "entry_speed"): [r"\.entry_speed(?:_mps)?\b"],
    ("HazardGuard", "all_of"): [r"\.all_of\b"],
    ("OnViolation", "mechanism"): [r"\.mechanism\b", r"\bDetectMechanism::"],
    ("SafeState", "settle"): [r"\.settle(?:_ms)?\b"],
    ("Function", "of"): [r"\.group\b", r"\bf\.members\b"],
    ("Predicate", "equals"): [r"CompareOp::Equals\b", r"\bpred\.op\b"],
    ("Predicate", "not_equals"): [r"CompareOp::NotEquals\b", r"\bpred\.op\b"],
    ("Predicate", "lt"): [r"CompareOp::Lt\b", r"\bpred\.op\b"],
    ("Predicate", "le"): [r"CompareOp::Le\b", r"\bpred\.op\b"],
    ("Predicate", "gt"): [r"CompareOp::Gt\b", r"\bpred\.op\b"],
    ("Predicate", "ge"): [r"CompareOp::Ge\b", r"\bpred\.op\b"],
    ("Exit", "on"): [r"exit[\w.()?]*\.on\b", r"\bx\.on\b"],
    ("Exit", "to"): [r"exit[\w.()?]*\.to\b", r"\bx\.to\b"],
    ("Settle", "decel"): [r"\.decel\b"],
    ("Settle", "jerk"): [r"\.jerk\b"],
    ("Settle", "duration"): [r"settle-conflict", r"written_settle_ms"],
    ("Settle", "response"): [r"\.settle_response\b", r"\bresponse_ms\b"],
    ("Settle", "source"): [r"\bbudget_sources\b", r"\bBudgetSource\b"],
    ("Budget", "duration"): [r"\bbudget_sources\b", r"\bBound\b"],
    ("Budget", "param"): [r"\.param\(\)", r"ParamBinding::"],
    ("Budget", "source"): [r"\bbudget_sources\b", r"\bBudgetSource\b", r"SourceKind::"],
    ("Measured", "max"): [r"\.terms\b", r"MeasuredTerm"],
    ("Measured", "n"): [r"\.terms\b", r"MeasuredTerm"],
    ("Measured", "period"): [r"\.terms\b", r"MeasuredTerm"],
    ("Measured", "margin"): [r"\.margin\b"],
    ("Measured", "terms"): [r"\.terms\b"],
    ("MeasuredTerm", "max"): [r"\.terms\b", r"MeasuredTerm"],
    ("MeasuredTerm", "n"): [r"\.terms\b", r"MeasuredTerm"],
    ("MeasuredTerm", "period"): [r"\.terms\b", r"MeasuredTerm"],
    ("Budget", "supervise"): [r"\.supervise\b", r"\bsupervised\(\)", r"BudgetLevel::"],
    ("Settle", "supervise"): [r"\.supervise\b", r"\bsupervised\(\)"],
    ("Supervise", "reaction"): [r"SuperviseReaction::Reaction\b"],
    ("Supervise", "enters"): [r"SuperviseReaction::Enters\b"],
    ("Supervise", "mechanism"): [r"SuperviseMechanism::"],
    ("Manifest", "policy"): [r"\.policy\b"],
    ("Policy", "unsupervised_measured"): [r"\.unsupervised_measured\b"],
    ("Manifest", "supervision"): [r"\.supervision\b"],
    ("Supervision", "tick"): [r"\.tick\b"],
    ("Supervision", "switch"): [r"\.switch\b"],
    ("TimerRate", "value"): [r"\btimer_rate_source\b"],
    ("TimerRate", "source"): [r"\btimer_rate_source\b", r"SourceKind::Allocated\b"],
}

# Rows whose patterns are known to match a field of the same name in another
# context: the reader count is an upper bound, resolved in the notes.
SHARED_FIELD = {
    "if", "unless", "type", "pub", "sub", "qos", "drop", "max_transport",
    "on", "reaction", "when", "all_of", "description", "external", "rate_hz",
    "paths", "server", "client", "max", "n", "period", "param", "source",
    "duration", "input", "action", "consecutive", "state", "required",
    "policy", "mechanism", "tick",
}

# Rows a grep cannot attribute, resolved by reading the code.
NOTES = {
    ("Manifest", "rlm"): "read by the parser only: the grammar-version gate, by design",
    ("Manifest", "version"): "parsed; only nano-ros reads a `version` (its own files' field, not this one)",
    ("Manifest", "if"): "conditions are evaluated generically (`if_condition`) for every context",
    ("IncludeExternal", "manifest"): "`IncludeDecl::external()` has no caller; play_launch finds contracts beside launch files instead",
    ("Endpoint", "max_transport"): "G3: the one place; the hop into this subscriber",
    ("Topic", "max_transport"): "G3: deprecated; read for one release as the default of subscribers stating none; `migrate` copies it onto them",
    ("Path", "drop"): "G3: scope path only, the E2E budget (`drop-tightness`); refused on a node path, `migrate` moves it to the output topic",
    ("Topic", "drop"): "G3: read by rate rules, the loss detector interval, the runtime drop monitor",
    ("Manifest", "external_topics"): "G3: merged into the same `externals` set as `topics.<n>.external`",
    ("ExternalTopic", "side"): "never written: both writers use the deprecated alias `external`",
    ("Hazard", "reaction"): "G4: a mode name (a ladder) or a scope path (one rung), told apart by lookup",
    ("Mode", "reaction"): "G4: a scope path",
    ("OnViolation", "reaction"): "G4: a node path on the subscriber's node",
    ("Exit", "on"): "names a function, where `hazards.on` and `on_violation.on` name fault classes",
    ("Node", "criticality"): "G5: derived from hazards; a label stands only where none reaches the node",
    ("Topic", "rate_hz"): "a consequence (`derivable-rate`), written 236 times",
    ("SafeState", "settle"): "G6: `settle: <d>` is `{ duration: <d> }`; one map form",
    ("Settle", "duration"): "G6: the written settle; beside a profile, the fallback and compared by `settle-conflict`",
    ("Settle", "response"): "G6: the settle's, added to a written duration or a profile; omitted 0",
    ("Path", "input"): "legacy spelling of `trigger: { input }`; both feed `effective_trigger()`",
    ("Predicate", "lt"): "operators are read generically (`pred.op`) by `when-*` rules; nothing evaluates them at run time yet",
    ("Arg", "type"): "read through `ArgDecl::valid_values()` (satisfiability rule)",
    ("HazardGuard", "all_of"): "inline redundant group; same shape as `functions.<n>: { all_of }`",
}

CONSUMERS = [
    # (column, repo key, roots, excludes)
    ("rlm-check", "rlm", ["check/src"], []),
    ("rlm-derive", "rlm", ["derive/src", "sched/src"], ["derive/src/tests.rs"]),
    (
        "pl-check",
        "pl",
        ["src/ros-launch-resolve/resolve/src"],
        [
            "src/ros-launch-resolve/resolve/src/ros/model_builder.rs",
            "src/ros-launch-resolve/resolve/src/verbs/check.rs",
            "src/ros-launch-resolve/resolve/src/ros/budget_book.rs",
        ],
    ),
    (
        "pl-explain",
        "pl",
        [
            "src/ros-launch-resolve/resolve/src/verbs/check.rs",
            "src/ros-launch-resolve/resolve/src/ros/budget_book.rs",
        ],
        [],
    ),
    ("pl-runtime", "pl", ["src/play_launch/src"], []),
    ("nros-codegen", "nros", ["packages/cli"], []),
    ("nros-other", "nros", ["packages"], ["packages/cli"]),
]
SHORT = {
    "rlm-check": "chk",
    "rlm-derive": "drv",
    "pl-check": "pl",
    "pl-explain": "exp",
    "pl-runtime": "rt",
    "nros-codegen": "ncg",
    "nros-other": "nrt",
}

TEST_MOD = re.compile(r"^#\[cfg\(test\)\]", re.M)


def is_test_path(p):
    parts = p.split("/")
    return "tests" in parts or "benches" in parts or p.endswith("/tests.rs")


def load_sources(repo, roots, excludes):
    files = {}
    for p in git(repo, "ls-files", "--", *roots).split("\n"):
        if not p.endswith(".rs") or is_test_path(p):
            continue
        if any(p == e or p.startswith(e.rstrip("/") + "/") for e in excludes):
            continue
        try:
            with open(os.path.join(repo, p), encoding="utf-8") as f:
                text = f.read()
        except OSError:
            continue
        m = TEST_MOD.search(text)
        files[p] = text[: m.start()] if m else text
    return files


def count_hits(files, pats):
    rx = [re.compile(p) for p in pats]
    n = 0
    where = set()
    for path, text in files.items():
        c = sum(len(r.findall(text)) for r in rx)
        if c:
            n += c
            where.add(path)
    return n, sorted(where)


# --------------------------------------------------------------------------
# Writers: walk every contract with the parser's context rules.


class Loader(yaml.SafeLoader):
    """YAML 1.2 booleans only: `on:` must stay the string key it is."""


Loader.yaml_implicit_resolvers = {
    k: [(tag, rx) for tag, rx in v if tag != "tag:yaml.org,2002:bool"]
    for k, v in yaml.SafeLoader.yaml_implicit_resolvers.items()
}
Loader.add_implicit_resolver(
    "tag:yaml.org,2002:bool",
    re.compile(r"^(?:true|True|TRUE|false|False|FALSE)$"),
    list("tTfF"),
)

BUDGET_KEYS = {
    "Endpoint": {"max_age", "max_transport"},
    "SrvEndpoint": {"max_response"},
    "Topic": {"max_transport"},
    "Path": {"max_latency", "max_jitter"},
    "TriggerTimer": {"jitter"},
    "Hazard": {"ftti"},
    "OnViolation": {"within"},
    "Settle": {"response"},
}


def vals(x):
    return x.values() if isinstance(x, dict) else []


def shape(v):
    if isinstance(v, dict):
        return "map"
    if isinstance(v, list):
        return "list"
    if v is None:
        return "null"
    return "scalar"


class Walk:
    def __init__(self, rec):
        self.rec = rec

    def keys(self, ctx, d):
        for k, v in d.items():
            self.rec(ctx, str(k), v)

    def budgets(self, ctx, d):
        for k in BUDGET_KEYS.get(ctx, ()):
            v = d.get(k)
            if isinstance(v, dict):
                self.keys("Budget", v)
                self.source(v.get("source"))
                self.supervise(v.get("supervise"))

    def supervise(self, s):
        if isinstance(s, dict):
            self.keys("Supervise", s)

    def source(self, s):
        if isinstance(s, dict):
            m = s.get("measured")
            if isinstance(m, dict):
                self.keys("Measured", m)
                terms = m.get("terms")
                if isinstance(terms, dict):
                    for t in terms.values():
                        if isinstance(t, dict):
                            self.keys("MeasuredTerm", t)

    def predicate(self, p):
        if isinstance(p, dict):
            self.keys("Predicate", p)

    def manifest(self, d):
        self.keys("Manifest", d)
        if isinstance(d.get("policy"), dict):
            self.keys("Policy", d["policy"])
        if isinstance(d.get("supervision"), dict):
            self.keys("Supervision", d["supervision"])
        for a in vals(d.get("args")):
            if isinstance(a, dict):
                self.keys("Arg", a)
        for n in vals(d.get("nodes")):
            if isinstance(n, dict):
                self.node(n)
        for t in vals(d.get("topics")):
            if isinstance(t, dict):
                self.keys("Topic", t)
                self.qos(t)
                self.drop(t)
                self.budgets("Topic", t)
        for t in vals(d.get("external_topics")):
            if isinstance(t, dict):
                self.keys("ExternalTopic", t)
                self.qos(t)
        for s in vals(d.get("services")):
            if isinstance(s, dict):
                self.keys("Service", s)
        for s in vals(d.get("actions")):
            if isinstance(s, dict):
                self.keys("Action", s)
        for inc in vals(d.get("includes")):
            if isinstance(inc, dict):
                if "manifest" in inc:
                    self.keys("IncludeExternal", inc)
                else:
                    self.manifest(inc)
        for p in vals(d.get("paths")):
            if isinstance(p, dict):
                self.path(p)
        for h in vals(d.get("hazards")):
            if isinstance(h, dict):
                self.keys("Hazard", h)
                for g in h.get("guards") or []:
                    if isinstance(g, dict):
                        self.keys("HazardGuard", g)
                self.predicate(h.get("when"))
                self.budgets("Hazard", h)
        for f in vals(d.get("functions")):
            if isinstance(f, dict):
                self.keys("Function", f)
                self.predicate(f.get("when"))
        for m in vals(d.get("modes")):
            if isinstance(m, dict):
                self.keys("Mode", m)
                w = m.get("window")
                if isinstance(w, dict):
                    self.keys("Budget", w)  # G2: a window's map is a budget's
                    self.source(w.get("source"))
                if isinstance(m.get("exit"), dict):
                    self.keys("Exit", m["exit"])

    def qos(self, d):
        if isinstance(d.get("qos"), dict):
            self.keys("Qos", d["qos"])

    def drop(self, d):
        if isinstance(d.get("drop"), dict):
            self.keys("Drop", d["drop"])

    def node(self, n):
        self.keys("Node", n)
        for side in ("pub", "sub", "cli"):
            for e in vals(n.get(side)):
                if isinstance(e, dict):
                    self.keys("Endpoint", e)
                    self.qos(e)
                    self.budgets("Endpoint", e)
                    ov = e.get("on_violation")
                    if isinstance(ov, dict):
                        self.keys("OnViolation", ov)
                        self.budgets("OnViolation", ov)
        for e in vals(n.get("srv")):
            if isinstance(e, dict):
                self.keys("SrvEndpoint", e)
                self.budgets("SrvEndpoint", e)
        for p in vals(n.get("paths")):
            if isinstance(p, dict):
                self.path(p)
        if isinstance(n.get("concurrency"), dict):
            self.keys("Concurrency", n["concurrency"])
        for p in vals(n.get("params")):
            if isinstance(p, dict):
                self.keys("Param", p)

    def path(self, p):
        self.keys("Path", p)
        self.drop(p)
        self.budgets("Path", p)
        t = p.get("trigger")
        if isinstance(t, dict):
            self.keys("Trigger", t)
            tm = t.get("timer")
            if isinstance(tm, dict):
                self.keys("TriggerTimer", tm)
                self.budgets("TriggerTimer", tm)
                if isinstance(tm.get("rate_hz"), dict):
                    self.keys("TimerRate", tm["rate_hz"])
                    self.source(tm["rate_hz"].get("source"))
        if isinstance(p.get("sync"), dict):
            self.keys("Sync", p["sync"])
        if isinstance(p.get("miss"), dict):
            self.keys("Miss", p["miss"])
        ss = p.get("safe_state")
        if isinstance(ss, dict):
            self.keys("SafeState", ss)
            st = ss.get("settle")
            if isinstance(st, dict):
                self.keys("Settle", st)
                self.budgets("Settle", st)
                self.source(st.get("source"))
                self.supervise(st.get("supervise"))


def names(d, kind, acc=None):
    """Mode or path names declared anywhere in a manifest tree."""
    acc = set() if acc is None else acc
    if not isinstance(d, dict):
        return acc
    if kind == "mode":
        acc.update((d.get("modes") if isinstance(d.get("modes"), dict) else {}).keys())
    else:
        acc.update((d.get("paths") if isinstance(d.get("paths"), dict) else {}).keys())
        for n in vals(d.get("nodes")):
            if isinstance(n, dict):
                acc.update((n.get("paths") if isinstance(n.get("paths"), dict) else {}).keys())
    for inc in vals(d.get("includes")):
        if isinstance(inc, dict) and "manifest" not in inc:
            names(inc, kind, acc)
    return acc


WRITERS = [
    ("pl", "play_launch"),
    ("rlm", "rlm"),
    ("island", "island"),
    ("nros", "nano-ros"),
]


def contract_files(repo, manifest_keys):
    out = []
    for p in git(repo, "ls-files").split("\n"):
        if not (p.endswith(".yaml") or p.endswith(".yml")):
            continue
        full = os.path.join(repo, p)
        try:
            with open(full, encoding="utf-8") as f:
                docs = list(yaml.load_all(f, Loader=Loader))
        except Exception:
            continue
        for d in docs:
            if not isinstance(d, dict) or not d:
                continue
            ks = {str(k) for k in d}
            if not ks <= manifest_keys:
                continue
            if not ks & {"nodes", "topics", "paths", "hazards", "modes", "includes", "external_topics", "functions"}:
                continue
            # A ROS parameter file also has `nodes:`-free shapes; a contract's
            # nodes map to maps or null.
            out.append((p, d))
    return out


# --------------------------------------------------------------------------


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--play-launch", default=os.path.expanduser("~/repos/play_launch"))
    ap.add_argument("--nano-ros", required=True)
    ap.add_argument("--island", default=os.path.expanduser("~/repos/simple-autoware-safety-island"))
    ap.add_argument("--out", default=os.path.join(HERE, "docs/key-inventory.md"))
    a = ap.parse_args()
    repos = {"rlm": HERE, "pl": a.play_launch, "nros": a.nano_ros, "island": a.island}

    with open(os.path.join(HERE, TABLE), encoding="utf-8") as f:
        src = f.read()
    rows = parse_table(src)
    labels = dict(LABEL.findall(src))
    since, oldest = since_map(rows)

    # readers
    sources = {c: load_sources(repos[r], roots, ex) for c, r, roots, ex in CONSUMERS}
    readers = {}
    for r in rows:
        pats = PATTERNS.get((r["ctx"], r["key"]), default_pat(r["key"]))
        readers[(r["ctx"], r["key"])] = {
            c: count_hits(sources[c], pats) for c, *_ in CONSUMERS
        }

    # writers
    manifest_keys = {r["key"] for r in rows if r["ctx"] == "Manifest"}
    written = collections.defaultdict(lambda: collections.Counter())
    forms = collections.defaultdict(collections.Counter)
    files_by = collections.defaultdict(lambda: collections.defaultdict(set))
    values = collections.defaultdict(collections.Counter)
    unknown = collections.defaultdict(set)
    reaction_targets = collections.Counter()
    nfiles = collections.Counter()
    occ = collections.defaultdict(list)
    known = {(r["ctx"], r["key"]) for r in rows}
    for wkey, _ in WRITERS:
        for path, doc in contract_files(repos[wkey], manifest_keys):
            nfiles[wkey] += 1
            modes = names(doc, "mode")
            paths = names(doc, "path")

            def rec(ctx, key, v, wkey=wkey, path=path, modes=modes, paths=paths):
                if (ctx, key) not in known:
                    unknown[(ctx, key)].add(f"{wkey}:{path}")
                    return
                written[(ctx, key)][wkey] += 1
                forms[(ctx, key)][shape(v)] += 1
                files_by[(ctx, key)][wkey].add(path)
                occ[(ctx, key)].append((wkey, path, shape(v)))
                if key in ("reaction", "severity", "criticality", "source", "side", "external", "mechanism") and not isinstance(v, (dict, list)):
                    values[(ctx, key)][str(v)] += 1
                if key == "reaction" and isinstance(v, str):
                    what = (
                        "mode+path" if v in modes and v in paths
                        else "mode" if v in modes
                        else "path" if v in paths
                        else "unresolved"
                    )
                    reaction_targets[(ctx, what, wkey)] += 1
                    occ[(ctx, "reaction->" + what)].append((wkey, path, "scalar"))

            Walk(rec).manifest(doc)

    heads = {k: git(v, "rev-parse", "--short=10", "HEAD").strip() for k, v in repos.items()}
    md = render(rows, labels, since, oldest, readers, written, forms, values,
                unknown, reaction_targets, nfiles, heads, occ)
    with open(a.out, "w", encoding="utf-8") as f:
        f.write(md)
    print(f"wrote {a.out}", file=sys.stderr)


def esc(t):
    return t.replace("|", "\\|")


FOCUS = [
    ("Endpoint", "max_transport"),
    ("Topic", "max_transport"),
    ("Path", "drop"),
    ("Topic", "drop"),
    ("Manifest", "external_topics"),
    ("ExternalTopic", "external"),
    ("ExternalTopic", "side"),
    ("Topic", "external"),
    ("Hazard", "reaction->mode"),
    ("Hazard", "reaction->path"),
    ("Hazard", "reaction->unresolved"),
    ("Mode", "reaction->path"),
    ("OnViolation", "reaction->path"),
    ("Exit", "on"),
    ("Hazard", "severity"),
    ("Manifest", "severity_levels"),
    ("Node", "criticality"),
    ("Topic", "rate_hz"),
    ("SafeState", "settle"),
    ("Settle", "decel"),
    ("Settle", "duration"),
    ("Settle", "response"),
    ("Path", "input"),
    ("Trigger", "input"),
    ("HazardGuard", "all_of"),
    ("Function", "all_of"),
    ("Function", "of"),
    ("Mode", "window"),
    ("TriggerTimer", "param"),
    ("TriggerTimer", "period_param"),
    ("Budget", "param"),
    ("Path", "miss"),
    ("Miss", "tolerate"),
    ("Drop", "max_count"),
]


# Pairs of rows that state one fact in two places (read from the code; the
# counts beside them are generated).
TWO_PLACES = [
    (("Endpoint", "max_transport"), ("Topic", "max_transport"),
     "G3 done: the subscriber's is kept; the topic's is deprecated and `migrate` copies it onto each subscriber stating none"),
    (("Path", "drop"), ("Topic", "drop"),
     "G3 done: the topic's is the hop loss; a scope path's is the E2E budget (`drop-tightness`); a node path's is refused"),
    (("Manifest", "external_topics"), ("Topic", "external"),
     "both land in the model's one `externals` set (G3)"),
    (("ExternalTopic", "external"), ("ExternalTopic", "side"),
     "deprecated alias and its canonical key; only the alias is written"),
    (("Path", "input"), ("Trigger", "input"),
     "legacy and current spelling of an input trigger; `effective_trigger()` merges them (new)"),
    (("HazardGuard", "all_of"), ("Function", "all_of"),
     "an inline redundant group, and the same group named (new)"),
    (("SafeState", "settle"), ("Settle", "duration"),
     "G6 done: `settle: <d>` is shorthand for `{ duration: <d> }`, one fact in one map form"),
    (("Hazard", "severity"), ("Node", "criticality"),
     "criticality is derived from severity; a written label is a second copy (G5)"),
    (("TriggerTimer", "rate_hz"), ("Topic", "rate_hz"),
     "a topic's rate is derived from the timers (`derivable-rate`) (new, deferred)"),
    (("Topic", "qos"), ("Endpoint", "qos"),
     "topic default, endpoint override: kept, both sides share the profile"),
]

# Keys whose meaning depends on the context they sit in.
MULTI = [
    ("reaction", [("Hazard", "a mode (ladder) or a scope path (one rung)"),
                  ("Mode", "a scope path"),
                  ("OnViolation", "a path on the subscriber's own node")], "G4"),
    ("on", [("Hazard", "fault classes"), ("OnViolation", "fault classes"),
            ("Exit", "a function name")], "new"),
    ("param", [("TriggerTimer", "bare parameter on the path's node, Hz or s"),
               ("Budget", "bare on `max_age`'s node, or `<node>.<parameter>` on a window, s")],
     "G2 done: one `ParamBinding`"),
    ("duration", [("Budget", "the bound (a mode's window too)"),
                  ("Settle", "the written settle")], "G6 done"),
    ("rate_hz", [("Topic", "a consequence"), ("TriggerTimer", "a fact")], "deferred"),
    ("type", [("Topic", "message type"), ("Param", "ROS 2 parameter type"),
              ("Arg", "`bool` or free string")], "benign"),
    ("external", [("Topic", "pub | sub | both"), ("Service", "server | client | both"),
                  ("ExternalTopic", "alias of `side`")], "benign"),
]


def fmt_readers(rd):
    parts = [f"{SHORT[c]} {n}" for c, (n, _) in rd.items() if n]
    return ", ".join(parts) if parts else "**none**"


def fmt_writers(w):
    parts = [f"{k} {w[k]}" for k, _ in WRITERS if w.get(k)]
    return ", ".join(parts) if parts else "**none**"


def render(rows, labels, since, oldest, readers, written, forms, values,
           unknown, reaction_targets, nfiles, heads, occ):
    live = [r for r in rows if r["status"] == "live"]
    ctxs = []
    for r in rows:
        if r["ctx"] not in ctxs:
            ctxs.append(r["ctx"])
    unread = [r for r in live if all(n == 0 for n, _ in readers[(r["ctx"], r["key"])].values())]
    unwritten = [r for r in live if not written.get((r["ctx"], r["key"]))]
    dead = [r for r in unread if r in unwritten]
    L = []
    w = L.append
    w("# Contract key inventory")
    w("")
    w("**Generated by `scripts/key_inventory.py`. Do not edit by hand.** "
      "Regenerate with `python3 scripts/key_inventory.py --nano-ros <checkout>` "
      "(see the script's docstring for the other inputs).")
    w("")
    w("Inputs (commit): " + ", ".join(f"{k} `{v}`" for k, v in heads.items()) + ".")
    w("")
    w("## Counts")
    w("")
    w(f"- Rows in `{TABLE}`: {len(rows)} ({len(live)} live, "
      f"{sum(r['status']=='alias' for r in rows)} deprecated alias, "
      f"{sum(r['status']=='deprecated' for r in rows)} deprecated (one release), "
      f"{sum(r['status']=='removed' for r in rows)} removed).")
    w(f"- Distinct live key names: {len({r['key'] for r in live})}; contexts: {len(ctxs)}.")
    kinds = collections.Counter(r["kind"] for r in live)
    w("- Live rows by kind: " + ", ".join(f"{k} {n}" for k, n in sorted(kinds.items())) + ".")
    w(f"- Contracts walked: " + ", ".join(f"{lbl} {nfiles[k]}" for k, lbl in WRITERS) + ".")
    w(f"- Live rows no consumer pattern matches: {len(unread)}.")
    w(f"- Live rows no contract writes: {len(unwritten)}.")
    w(f"- Live rows both unread and unwritten: {len(dead)}.")
    w("")
    w("## Columns")
    w("")
    w("- **since**: first release tag whose `field_table.rs` has the row; "
      f"`<={oldest[1:] if oldest else '?'}` means the row was in the table when it "
      "was created (the key may be older; see `CHANGELOG.md`).")
    w("- **readers**: pattern hits, test code excluded. "
      "`chk` rlm `check/src` (the rules `play_launch check` runs); "
      "`drv` rlm `derive/src`, `sched/src`; "
      "`pl` play_launch `src/ros-launch-resolve/resolve/src` minus the model "
      "builder (a copy, not a read) and the explain files; "
      "`exp` play_launch `verbs/check.rs` and `ros/budget_book.rs` (the "
      "`--explain` tables); "
      "`rt` play_launch `src/play_launch/src` (launch, runtime enforcement); "
      "`ncg` nano-ros `packages/cli` (code generation, sizing); "
      "`nrt` the rest of nano-ros `packages` (runtime). "
      "nano-ros reads rlm at the tag its `Cargo.toml` pins, so a key added "
      "after that tag cannot have a nano-ros reader yet.")
    w("- A row marked *shared* has a field name another context also uses; its "
      "reader count is an upper bound. The analysis below resolves the ones "
      "that matter by reading the code.")
    w("- **writers**: occurrences in contracts, per repository "
      "(`pl` play_launch, `rlm`, `island`, `nros`); **forms** are the value "
      "shapes written (scalar / map / list).")
    w("")
    w("## Inventory")
    for c in ctxs:
        w("")
        w(f"### `{labels.get(c, c)}`")
        w("")
        w("| key | kind | status | since | accepted forms | readers | writers | forms written | note |")
        w("|---|---|---|---|---|---|---|---|---|")
        for r in rows:
            if r["ctx"] != c:
                continue
            k = (c, r["key"])
            status = {
                "alias": f"alias of `{r['canonical']}`",
                "deprecated": f"deprecated: `{r['canonical']}`",
            }.get(r["status"], r["status"])
            if r["status"] in ("live", "deprecated"):
                rd = fmt_readers(readers[k])
                if r["key"] in SHARED_FIELD and rd != "**none**":
                    rd += " (*shared*)"
                wr = fmt_writers(written.get(k, {}))
            else:
                rd = wr = ""
            fw = ", ".join(f"{s} {n}" for s, n in forms[k].most_common())
            w(f"| `{r['key']}` | {r['kind'].lower() if r['status']=='live' else ''} | {status} | "
              f"{since[k]} | {esc(FORMS.get(k, ''))} | {rd} | {wr} | {fw} | {esc(NOTES.get(k, ''))} |")
    w("")
    w("## Live rows no consumer pattern matches")
    w("")
    for r in unread:
        k = (r["ctx"], r["key"])
        w(f"- `{labels.get(r['ctx'])}.{r['key']}` ({r['kind'].lower()}), written: {fmt_writers(written.get(k, {}))}")
    w("")
    w("## Live rows no contract writes")
    w("")
    for r in unwritten:
        k = (r["ctx"], r["key"])
        w(f"- `{labels.get(r['ctx'])}.{r['key']}` ({r['kind'].lower()}), readers: {fmt_readers(readers[k])}")
    w("")
    w("## Observed values")
    w("")
    for k in sorted(values):
        vs = ", ".join(f"`{v}` {n}" for v, n in values[k].most_common(12))
        w(f"- `{labels.get(k[0])}.{k[1]}`: {vs}")
    w("")
    w("## What `reaction` names, as written")
    w("")
    w("Each written `reaction:` value resolved against the mode names and the "
      "path names declared in the same file (a name declared as both is "
      "`mode+path`; one declared in another file is `unresolved`).")
    w("")
    w("| context | names a | repo | count |")
    w("|---|---|---|---|")
    for (ctx, what, wkey), n in sorted(reaction_targets.items()):
        w(f"| `{labels.get(ctx)}` | {what} | {wkey} | {n} |")
    w("")
    w("## Keys written that the table does not accept")
    w("")
    w("Deliberately bad fixtures (unknown-key tests) land here too.")
    w("")
    if not unknown:
        w("None.")
    for k in sorted(unknown):
        fs = sorted(unknown[k])
        w(f"- `{labels.get(k[0], k[0])}.{k[1]}`: {len(fs)} file(s), e.g. `{fs[0]}`")
    w("")
    w("## One fact, two places")
    w("")
    w("| row A | written | row B | written | relation |")
    w("|---|---|---|---|---|")
    for a, b, why in TWO_PLACES:
        w(f"| `{labels.get(a[0])}.{a[1]}` | {fmt_writers(written.get(a, {}))} | "
          f"`{labels.get(b[0])}.{b[1]}` | {fmt_writers(written.get(b, {}))} | {why} |")
    w("")
    w("## One key, several meanings")
    w("")
    w("| key | context: meaning (written) | phase 90 |")
    w("|---|---|---|")
    for key, uses, unit in MULTI:
        cells = "; ".join(
            f"`{labels.get(c)}`: {m} ({fmt_writers(written.get((c, key), {}))})" for c, m in uses)
        w(f"| `{key}` | {esc(cells)} | {unit} |")
    w("")
    w("## Focus rows: who writes them, where, in which form")
    w("")
    w("Per repository: files / occurrences / value shapes. Files are listed "
      "when there are at most six.")
    w("")
    w("| row | repo | files | occurrences | shapes | files |")
    w("|---|---|---|---|---|---|")
    for ctx, key in FOCUS:
        lst = occ.get((ctx, key), [])
        label = f"`{labels.get(ctx, ctx)}.{key}`"
        if not lst:
            w(f"| {label} | - | 0 | 0 | | |")
            continue
        for wk, _ in WRITERS:
            mine = [o for o in lst if o[0] == wk]
            if not mine:
                continue
            files = sorted({o[1] for o in mine})
            shapes = collections.Counter(o[2] for o in mine)
            fl = ", ".join(f"`{f}`" for f in files) if len(files) <= 6 else ""
            w(f"| {label} | {wk} | {len(files)} | {len(mine)} | "
              + ", ".join(f"{a} {b}" for a, b in shapes.most_common()) + f" | {fl} |")
    w("")
    return "\n".join(L)


if __name__ == "__main__":
    main()
