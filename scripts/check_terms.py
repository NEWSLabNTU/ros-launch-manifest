#!/usr/bin/env python3
"""Phase 98 N5: fail when a retired word reaches user-facing text.

Phase 98 renamed contract vocabulary to the words the standards use
(rlm `docs/design/standard-names.md`): a hazard's `severity:` became
`integrity:`, a rung a degradation level, a ladder a fallback sequence, a
mode's `window:` a `min_dwell:`, a scope path an event chain, and eleven
rule ids moved. An old contract is refused naming the new key (N4); this
gate keeps the old words out of what a user READS: the guides, the README,
the messages the binaries print and the web UI.

What is scanned (rlm's copy; play_launch keeps its own with its own
paths, `scripts/check_terms.py` there):

- Markdown: `README.md` and `docs/*.md`, every line.
- Rust: the string literals of every crate's `src/` (diagnostics and
  parse errors), with format placeholders (`{rung}`) removed first, since a
  placeholder is an identifier and never printed. `#[cfg(test)]` modules
  are skipped: a test may spell an old word to prove it is refused.

History keeps the words of its time, so `docs/design/`, the design issues
and the CHANGELOG are not scanned. A file that must name an old word (one
that maps it to the new one: `rule_names`, `migrate`, the field table's
`removed` rows, the model's read-side rewrite) is in `ALLOWED_FILES`; a
single line may carry `terms: allow` in a comment. Run by
`types/tests/retired_terms.rs`, so `cargo test` is the gate.

`--self-test` injects one old word into a copy of every scanned kind and
requires each to be reported: a negative control, run by `just check`.
"""

from __future__ import annotations

import argparse
import re
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# (pattern, the word to use instead). Case-insensitive, whole words.
RETIRED: list[tuple[str, str]] = [
    (r"severity_levels", "integrity_levels"),
    (r"severity_scale", "integrity_scale"),
    (r"severity:\s*(?:ASIL|QM|SIL|DAL)", "integrity: <level>"),
    (r"\brungs?\b", "degradation level"),
    (r"\bladders?\b", "fallback sequence"),
    (r"\bscope[ -]paths?\b", "event chain"),
    (r"\bseverity-unknown\b", "integrity-unknown"),
    (r"\bseverity-scale-conflict\b", "integrity-scale-conflict"),
    (r"\bladder-rung-budget\b", "degradation-level-time"),
    (r"\bladder-unterminated\b", "fallback-without-mrc"),
    (r"\bladder-window-floor\b", "dwell-at-mrc"),
    (r"\bcall-unbounded-floor\b", "call-unbounded-mrc"),
    (r"\bwindow-expiry\b", "dwell-expiry"),
    (r"\bwindow-param\b", "dwell-param"),
    (r"\bwindow-unbound\b", "dwell-unbound"),
    (r"\bscope-budget\b", "chain-latency"),
    (r"\bscope-sampling-feasibility\b", "chain-sampling-feasibility"),
    # Phase 98 N6, refused since v0.3.9 (R+1); the R+1 words are listed
    # here a release early, since nothing user-facing may teach a refused
    # spelling either.
    (r"\bfault-reaction-budget\b", "fault-reaction-time"),
    (r"\bdegradation-level-budget\b", "degradation-level-time"),
    (r"\bbudget-overflow\b", "latency-bound-overflow"),
    (r"\bon_demand\b", "aperiodic"),
    (r"\bspontaneous\b", "aperiodic"),
    # Phase 98 M7 (play_launch phase 99 T5, R+2): the scheduling unit is a
    # task. TIER IV's packages (`tier4_*`) and the company's name ("TIER
    # IV") are another word spelled the same; the pattern itself leaves
    # them out (a digit or `_` after the word is no word boundary, and the
    # lookahead skips the name), so no file is excluded for carrying them.
    (r"\btiers?\b(?![ \t]+IV\b)", "task"),
]
# Markdown only: a mode's `window:` key in an example block.
MD_ONLY: list[tuple[str, str]] = [
    (r"^\s+window:", "min_dwell:"),
    # Phase 98 N6 keys in an example block (refused since v0.3.9).
    (r"^\s*-?\s*(?:guards|ingress|on_demand|mapper):", "monitors / arrival_curve / aperiodic / priority_assignment"),
]

COMPILED = [(re.compile(p, re.IGNORECASE), new) for p, new in RETIRED]
MD_COMPILED = COMPILED + [(re.compile(p), new) for p, new in MD_ONLY]

MARKDOWN_GLOBS = ["README.md", "docs/*.md"]
RUST_DIRS = ["types/src", "model/src", "sched/src", "derive/src", "check/src", "rlx/src"]
HELP_DIRS: list[str] = []
JS_DIRS: list[str] = []
JS_SKIP = ("vendor",)

# Files that name an old word to map it to the new one. Paths relative to
# the repository root.
ALLOWED_FILES: set[str] = {
    # The map from each old spelling to its new one.
    "types/src/rule_names.rs",
    "types/src/migrate.rs",
    # A profile's `severity_scale:` is refused by its own message.
    "types/src/profile.rs",
    # Saved models of schema 1 to 4 are rewritten on read.
    "model/src/lib.rs",
    # The `system.toml` removed rows and `migrate` name `[tiers.*]` and
    # `group_tiers` to say where they went.
    "model/src/system_config.rs",
    # The legacy v1 schema's refusal names the key it removed.
    "sched/src/platform.rs",
    # `removed` rows name the old key to say where it went, and the
    # generated reference prints them.
    "types/src/field_table.rs",
    "docs/format-reference.md",
    # History.
    "docs/design-issues.md",
    "docs/key-inventory.md",
}

ALLOW_MARK = "terms: allow"


def rust_literals(text: str) -> list[tuple[int, str]]:
    """(1-based line of the literal's start, body) for each string literal,
    stopping at the first `#[cfg(test)]`."""
    out: list[tuple[int, str]] = []
    i, n, line = 0, len(text), 1
    while i < n:
        c = text[i]
        if c == "\n":
            line += 1
            i += 1
        elif text.startswith("#[cfg(test)]", i):
            break
        elif text.startswith("//", i):
            j = text.find("\n", i)
            i = n if j < 0 else j
        elif text.startswith("/*", i):
            j = text.find("*/", i + 2)
            j = n if j < 0 else j + 2
            line += text.count("\n", i, j)
            i = j
        elif c == "r" and re.match(r'r#*"', text[i:]) and not (
            i > 0 and (text[i - 1].isalnum() or text[i - 1] == "_")
        ):
            hashes = re.match(r"r(#*)\"", text[i:]).group(1)
            start = i + 2 + len(hashes)
            end = text.find('"' + hashes, start)
            end = n if end < 0 else end
            out.append((line, text[start:end]))
            line += text.count("\n", i, end)
            i = end + 1 + len(hashes)
        elif c == '"':
            j = i + 1
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            out.append((line, text[i + 1 : j]))
            line += text.count("\n", i, j)
            i = j + 1
        elif c == "'":
            # A char literal ('a', '\n', '"') or a lifetime ('a).
            m = re.match(r"'(?:\\.|\\u\{[0-9a-fA-F]+\}|[^\\'])'", text[i:])
            i += len(m.group(0)) if m else 1
        else:
            i += 1
    return out


def strip_placeholders(body: str) -> str:
    # `{rung}`, `{}`, `{x:.2}`; `{{` is a literal brace.
    return re.sub(r"\{[A-Za-z0-9_.:?#<>+\-]*\}", "", body.replace("{{", "").replace("}}", ""))


def js_literals(text: str) -> list[tuple[int, str]]:
    out: list[tuple[int, str]] = []
    for m in re.finditer(r"`(?:\\.|[^`\\])*`|'(?:\\.|[^'\\\n])*'|\"(?:\\.|[^\"\\\n])*\"", text):
        body = re.sub(r"\$\{[^}]*\}", "", m.group(0)[1:-1])
        out.append((text.count("\n", 0, m.start()) + 1, body))
    return out


def line_allowed(lines: list[str], lineno: int) -> bool:
    return 0 < lineno <= len(lines) and ALLOW_MARK in lines[lineno - 1]


def scan_body(rel: str, lines: list[str], start: int, body: str, patterns, hits: list[str]) -> None:
    for pat, new in patterns:
        for m in pat.finditer(body):
            lineno = start + body.count("\n", 0, m.start())
            if line_allowed(lines, lineno):
                continue
            hits.append(f"{rel}:{lineno}: retired word '{m.group(0).strip()}' -- write {new}")


def scan(root: Path) -> list[str]:
    hits: list[str] = []

    def rel(p: Path) -> str:
        return p.relative_to(root).as_posix()

    for glob in MARKDOWN_GLOBS:
        for p in sorted(root.glob(glob)):
            if rel(p) in ALLOWED_FILES:
                continue
            lines = p.read_text().splitlines()
            for k, text_line in enumerate(lines, 1):
                scan_body(rel(p), lines, k, text_line, MD_COMPILED, hits)

    for d in RUST_DIRS:
        for p in sorted((root / d).rglob("*.rs")):
            if rel(p) in ALLOWED_FILES or "/tests/" in rel(p):
                continue
            text = p.read_text()
            lines = text.splitlines()
            for start, body in rust_literals(text):
                scan_body(rel(p), lines, start, strip_placeholders(body), COMPILED, hits)

    for d in HELP_DIRS:
        for p in sorted((root / d).rglob("*.rs")):
            if rel(p) in ALLOWED_FILES:
                continue
            lines = p.read_text().splitlines()
            for k, text_line in enumerate(lines, 1):
                s = text_line.strip()
                if s.startswith("///"):
                    scan_body(rel(p), lines, k, s[3:], COMPILED, hits)

    for d in JS_DIRS:
        for p in sorted((root / d).rglob("*.js")):
            if rel(p) in ALLOWED_FILES or any(s in p.parts for s in JS_SKIP):
                continue
            text = p.read_text()
            lines = text.splitlines()
            for start, body in js_literals(text):
                scan_body(rel(p), lines, start, body, COMPILED, hits)

    # A literal spanning lines reports each match once.
    return sorted(set(hits))


def self_test() -> int:
    """Inject one old word per scanned kind; every one must be reported,
    and each kind's clean twin must not be."""
    cases = {
        "docs/x.md": ("Each rung is a mode.\n", "Each degradation level is a mode.\n"),
        "README.md": ("modes:\n  stop:\n    window: 10s\n", "modes:\n  stop:\n    min_dwell: 10s\n"),
        "types/src/a.rs": (
            'fn f(rung: &str) { eprintln!("hazard {rung}: scope-budget over"); }\n',
            'fn f(rung: &str) { eprintln!("hazard {rung}: chain-latency over"); }\n',
        ),
        "docs/y.md": ("hazards:\n  h:\n    guards: [a]\n", "hazards:\n  h:\n    monitors: [a]\n"),
        "derive/src/c.rs": (
            'const M: &str = "an on_demand publisher";\n',
            'const M: &str = "an aperiodic publisher";\n',
        ),
        "sched/src/t.rs": (
            'const M: &str = "the tier has no deadline";\n',
            'const M: &str = "the task has no deadline";\n',
        ),
        "docs/t.md": ("Each tier runs one executor.\n", "Each task runs one executor.\n"),
        # TIER IV's packages and name are another word: never a hit.
        "docs/t4.md": (
            "The tiers are bound.\n",
            "`tier4_system_launch` and `tier4_*` are TIER IV's; TIER IV ships them.\n",
        ),
        "check/src/b.rs": (
            'const M: &str = r#"the ladder ends"#;\n',
            'const M: &str = r#"the fallback sequence ends"#; // a ladder in a comment\n',
        ),
    }
    failed = 0
    for path, (bad, good) in cases.items():
        for body, want in ((bad, True), (good, False)):
            with tempfile.TemporaryDirectory(dir=ROOT / "tmp") as td:
                p = Path(td) / path
                p.parent.mkdir(parents=True, exist_ok=True)
                p.write_text(body)
                got = any(h.startswith(path + ":") for h in scan(Path(td)))
                if got != want:
                    failed += 1
                    print(f"check-terms self-test: {path}: {'missed' if want else 'false hit on'} {body!r}", file=sys.stderr)
    # A test module may spell an old word.
    with tempfile.TemporaryDirectory(dir=ROOT / "tmp") as td:
        p = Path(td) / "types/src/e.rs"
        p.parent.mkdir(parents=True)
        p.write_text('#[cfg(test)]\nmod tests { const X: &str = "rung"; }\n')
        if scan(Path(td)):
            failed += 1
            print("check-terms self-test: a #[cfg(test)] module was scanned", file=sys.stderr)
    if failed:
        return 1
    print("check-terms self-test: every injected word reported, no false hit")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--self-test", action="store_true")
    a = ap.parse_args()
    (ROOT / "tmp").mkdir(exist_ok=True)
    if a.self_test:
        return self_test()
    hits = scan(ROOT)
    for h in hits:
        print(h)
    if hits:
        print(f"check-terms: {len(hits)} retired word(s) in user-facing text (phase 98)", file=sys.stderr)
        return 1
    print("check-terms: no retired word in user-facing text")
    return 0


if __name__ == "__main__":
    sys.exit(main())
