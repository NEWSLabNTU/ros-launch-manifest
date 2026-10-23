//! `docs/extension-reference.md` is generated from the extensions this
//! repository publishes (phase 91 E11): the bundled ones and the reference
//! ones, each with its keys, what it derives, checks and reports, and its
//! source. The format reference lists the core grammar; this lists the keys
//! extensions add. Regenerate with
//! `UPDATE_EXTENSION_REFERENCE=1 cargo test -p ros-launch-manifest-rlx --test extension_reference`.

use rlx::{
    ast::{Direction, Item, KeyType, Product},
    check_source,
};
use ros_launch_manifest_types::extension::{BUNDLED, REFERENCE};

fn slice(src: &str, p: &Product) -> String {
    src[p.span.start..p.span.end].trim().to_string()
}

fn key_type(src: &str, ty: &KeyType) -> String {
    match ty {
        KeyType::Value(p) => slice(src, p),
        KeyType::Param {
            dim,
            magnitude,
            unit,
        } => {
            let mut s = format!("param {}", slice(src, dim));
            if *magnitude {
                s.push_str(" magnitude");
            }
            if let Some(u) = unit {
                s.push_str(&format!(" unit {}", slice(src, u)));
            }
            s
        }
        KeyType::Table { from, to } => format!("table {} -> {}", slice(src, from), slice(src, to)),
    }
}

fn worst(w: &[rlx::ast::Monotone]) -> String {
    if w.is_empty() {
        return "none stated".to_string();
    }
    w.iter()
        .map(|m| {
            format!(
                "{} in {}",
                match m.direction {
                    Direction::Increasing => "increasing",
                    Direction::Decreasing => "decreasing",
                },
                m.vars
                    .iter()
                    .map(|v| format!("`{}`", v.dotted()))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn section(file: &str, src: &str, status: &str) -> String {
    let checked = check_source(src.as_bytes()).unwrap_or_else(|d| panic!("{file}: {d:?}"));
    let id = rlx::eval::identity(&checked);
    let mut out = format!("## `{} {}`\n\n{status}\n\n", id.name, id.version);
    let mut keys = Vec::new();
    let mut inputs = Vec::new();
    let mut produces = Vec::new();
    for item in &checked.file().items {
        match item {
            Item::Requires(r) => out.push_str(&format!(
                "Requires: `{} {}`.\n\n",
                r.name.dotted(),
                r.constraints
                    .iter()
                    .map(|(op, v)| format!("{} {}.{}.{}", op.ascii(), v.major, v.minor, v.patch))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            Item::Key(k) => keys.push(format!(
                "| `{}` | {} | {} | {} |",
                k.path.dotted(),
                key_type(src, &k.ty),
                if k.default.is_some() { "yes" } else { "no" },
                k.doc.clone().unwrap_or_default()
            )),
            Item::Input(i) => inputs.push(format!(
                "| `{}` | {} | {} |",
                i.path.dotted(),
                slice(src, &i.dim),
                if i.default.is_some() { "yes" } else { "no" }
            )),
            Item::Derive(d) => produces.push(format!(
                "| derive | `{}` | {} | {} |",
                d.target.dotted(),
                slice(src, &d.dim),
                worst(&d.body.worst_case)
            )),
            Item::Check(c) => produces.push(format!(
                "| check | `{}` | verdict | {} |",
                c.name.name,
                worst(&c.body.worst_case)
            )),
            Item::Report(r) => produces.push(format!(
                "| report | `{}` | {} | no verdict |",
                r.name.name,
                slice(src, &r.dim)
            )),
            _ => {}
        }
    }
    out.push_str(
        "Keys a contract writes:\n\n| key | type | default | meaning |\n|---|---|---|---|\n",
    );
    out.push_str(&keys.join("\n"));
    out.push_str("\n\n");
    if !inputs.is_empty() {
        out.push_str("Core values it reads:\n\n| input | dimension | default |\n|---|---|---|\n");
        out.push_str(&inputs.join("\n"));
        out.push_str("\n\n");
    }
    out.push_str("What it produces:\n\n| item | name | result | worst case |\n|---|---|---|---|\n");
    out.push_str(&produces.join("\n"));
    out.push_str(&format!(
        "\n\nsha256 of the canonical form: `{}`. Source (`types/extensions/{file}`):\n\n```\n{}```\n\n",
        id.sha256, src
    ));
    out
}

fn render() -> String {
    let mut out = String::from(
        "# Extensions -- Reference\n\n\
         **Generated from `types/extensions/`. Do not edit by hand** -- run \
         `UPDATE_EXTENSION_REFERENCE=1 cargo test -p ros-launch-manifest-rlx --test \
         extension_reference` after changing an extension.\n\n\
         The core grammar is [format-reference.md](format-reference.md); a key not listed \
         there is a parse error unless the contract `use:`s an extension that declares it in \
         that context (`hazards.<name>` or `safe_state.settle`). This page lists the \
         extensions this repository publishes. How to write one: \
         [extension-guide.md](extension-guide.md).\n\n\
         - **Bundled** extensions ship inside the checker and resolve for any `use:` entry \
         with no file on disk.\n\
         - **Reference** extensions are the worked examples of \
         [design/extension-examples.md](design/extension-examples.md) as files. They are not \
         bundled: a contract that wants one ships a copy in its package's `rlm/` directory. \
         They are illustrative: \
         [design/reference-extensions-verification.md](design/reference-extensions-verification.md) \
         compares each with the standard it models and lists what each leaves out.\n\n",
    );
    for (file, src) in BUNDLED {
        out.push_str(&section(
            file,
            src,
            "**Bundled.** The keys were core grammar from rlm v0.1.46 to v0.1.71. play_launch 0.19 \
             loaded this extension implicitly, with a warning, for a contract that writes them \
             without `use:`; later releases refuse such a contract \
             (`deprecated-implicit-extension`).",
        ));
    }
    for (file, src) in REFERENCE {
        out.push_str(&section(
            &format!("reference/{file}"),
            src,
            "**Reference, illustrative.**",
        ));
    }
    out.trim_end().to_string() + "\n"
}

#[test]
fn the_extension_reference_matches_the_extensions() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/extension-reference.md");
    let want = render();
    if std::env::var_os("UPDATE_EXTENSION_REFERENCE").is_some() {
        std::fs::write(&path, &want).unwrap();
    }
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        have == want,
        "docs/extension-reference.md is stale: run UPDATE_EXTENSION_REFERENCE=1 cargo test -p \
         ros-launch-manifest-rlx --test extension_reference"
    );
}

/// Every rlx example of `docs/extension-guide.md` loads, and (with `smt`)
/// proves every worst case it states: the guide teaches files that work.
#[test]
fn the_guides_examples_load_and_prove() {
    let guide = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/extension-guide.md"),
    )
    .unwrap();
    let blocks: Vec<&str> = guide
        .split("```rlx\n")
        .skip(1)
        .map(|b| b.split("```").next().unwrap())
        .collect();
    assert!(!blocks.is_empty());
    for b in blocks {
        let checked = check_source(b.as_bytes()).unwrap_or_else(|d| panic!("{d:?}\n{b}"));
        #[cfg(feature = "smt")]
        {
            let claims = rlx::prove::prove_all(&checked, &rlx::prove::Options::default());
            assert!(!claims.is_empty());
            for c in claims {
                assert_eq!(c.verdict, rlx::prove::Verdict::Proved, "{}", c.report());
            }
        }
        #[cfg(not(feature = "smt"))]
        let _ = checked;
    }
}

/// A key typed by a dimension takes a quantity with its unit; a plain
/// number is refused, as the guide says, so no unit is guessed.
#[test]
fn a_plain_number_is_not_a_quantity() {
    let src = BUNDLED[0].1;
    let checked = check_source(src.as_bytes()).unwrap();
    let ty = checked.env.key("hazard.entry_speed").unwrap().ty.clone();
    assert!(checked.env.parse_value("30 km/h", &ty).is_ok());
    assert!(checked.env.parse_value("8.33", &ty).is_err());
}
