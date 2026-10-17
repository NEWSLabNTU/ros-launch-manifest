//! Refusals that need the whole tree but no meaning: recursion and names a
//! quantity would read as units.
//!
//! Recursion is refused because rlx promises every expression terminates
//! (design 5.2). Without recursion, and with `sum over` ranging over a
//! finite table, termination is structural rather than something a checker
//! has to establish. The call graph is built over `fn` bodies and their
//! `where` bindings; any cycle, of length one or more, is refused.

use crate::{
    ast::*,
    diag::{Diagnostic, Span, code},
    units,
};
use std::collections::HashMap;

pub fn refuse(file: &File) -> Vec<Diagnostic> {
    let mut out = recursion(file);
    out.extend(shadowing(file));
    out.sort_by_key(|d| d.span.start);
    out
}

/// Calls in an expression to names that are `fn`s, with the call's span.
fn calls<'a>(e: &'a Expr, fns: &HashMap<&str, usize>, out: &mut Vec<(usize, Span)>) {
    e.walk(&mut |e: &'a Expr| {
        if let ExprKind::Call { callee, .. } = &e.kind
            && callee.segments.len() == 1
            && let Some(&i) = fns.get(callee.segments[0].name.as_str())
        {
            out.push((i, callee.span));
        }
    });
}

fn recursion(file: &File) -> Vec<Diagnostic> {
    let decls: Vec<&FnDecl> = file
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Fn(f) => Some(f),
            _ => None,
        })
        .collect();
    // First declaration wins for a duplicated name; duplicates are a naming
    // error for the later stages, not a recursion.
    let mut index: HashMap<&str, usize> = HashMap::new();
    for (i, f) in decls.iter().enumerate() {
        index.entry(f.name.name.as_str()).or_insert(i);
    }
    let edges: Vec<Vec<(usize, Span)>> = decls
        .iter()
        .map(|f| {
            let mut out = Vec::new();
            calls(&f.value, &index, &mut out);
            for b in &f.bindings {
                calls(&b.value, &index, &mut out);
            }
            out
        })
        .collect();

    // Each fn is reported in at most one cycle: the first one found from the
    // earliest fn on it, so one mutual recursion yields one diagnostic.
    let mut reported = vec![false; decls.len()];
    let mut diags = Vec::new();
    for start in 0..decls.len() {
        if reported[start] {
            continue;
        }
        let Some(cycle) = find_cycle(start, &edges) else {
            continue;
        };
        if cycle.iter().any(|&(f, _)| reported[f]) {
            continue;
        }
        for &(f, _) in &cycle {
            reported[f] = true;
        }
        let names: Vec<&str> = cycle
            .iter()
            .map(|&(f, _)| decls[f].name.name.as_str())
            .collect();
        let at = cycle[0].1;
        let msg = if cycle.len() == 1 {
            format!(
                "fn `{}` calls itself; rlx refuses recursion so that every expression \
                 terminates",
                names[0]
            )
        } else {
            let mut path = names.join(" -> ");
            path.push_str(" -> ");
            path.push_str(names[0]);
            format!(
                "fns {} call each other ({path}); rlx refuses recursion so that every \
                 expression terminates",
                names
                    .iter()
                    .map(|n| format!("`{n}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        diags.push(Diagnostic::new(code::RECURSION, at, msg));
    }
    diags
}

/// A cycle from `start` back to itself, as (fn, span of the call it makes to
/// the next fn on the cycle). Breadth-first, so the shortest cycle is named.
fn find_cycle(start: usize, edges: &[Vec<(usize, Span)>]) -> Option<Vec<(usize, Span)>> {
    // parent[n] = (previous fn, call span in it) on the shortest path start -> n.
    let mut parent: Vec<Option<(usize, Span)>> = vec![None; edges.len()];
    let mut queue = std::collections::VecDeque::from([start]);
    let mut seen = vec![false; edges.len()];
    seen[start] = true;
    while let Some(n) = queue.pop_front() {
        for &(m, span) in &edges[n] {
            if m == start {
                // Walk back from n to start, collecting the calls.
                let mut rev = vec![(n, span)];
                let mut cur = n;
                while cur != start {
                    let (p, s) = parent[cur].expect("visited nodes have a parent");
                    rev.push((p, s));
                    cur = p;
                }
                rev.reverse();
                return Some(rev);
            }
            if !seen[m] {
                seen[m] = true;
                parent[m] = Some((n, span));
                queue.push_back(m);
            }
        }
    }
    None
}

fn shadow(name: &Ident, what: &str, out: &mut Vec<Diagnostic>) {
    if let Some(unit) = units::unit_name(&name.name) {
        out.push(Diagnostic::new(
            code::SHADOW,
            name.span,
            format!(
                "{what} `{}` has the name of the unit `{}` ({unit}); after a number, as in \
                 `2 {}`, it would read as the unit, so choose another name",
                name.name, name.name, name.name
            ),
        ));
    }
}

fn shadow_expr(e: &Expr, out: &mut Vec<Diagnostic>) {
    e.walk(&mut |e| {
        if let ExprKind::Sum { var, .. } = &e.kind {
            shadow(var, "summation variable", out);
        }
    });
}

fn shadow_bindings(bs: &[Binding], out: &mut Vec<Diagnostic>) {
    for b in bs {
        shadow(&b.name, "binding", out);
        shadow_expr(&b.value, out);
    }
}

/// Names an expression can refer to must not be unit symbols: keys and
/// inputs (by their last segment), `fn` parameters, `where` bindings and
/// summation variables.
fn shadowing(file: &File) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for item in &file.items {
        match item {
            Item::Key(k) => {
                shadow(k.path.last(), "key", &mut out);
                if let Some(d) = &k.default {
                    shadow_expr(d, &mut out);
                }
            }
            Item::Input(i) => shadow(i.path.last(), "input", &mut out),
            Item::Derive(d) => {
                shadow_expr(&d.body.value, &mut out);
                shadow_bindings(&d.body.bindings, &mut out);
            }
            Item::Check(c) => {
                shadow_expr(&c.body.value, &mut out);
                shadow_bindings(&c.body.bindings, &mut out);
            }
            Item::Report(r) => {
                shadow_expr(&r.value, &mut out);
                shadow_bindings(&r.bindings, &mut out);
            }
            Item::Fn(f) => {
                for p in &f.params {
                    shadow(&p.name, "parameter", &mut out);
                }
                shadow_expr(&f.value, &mut out);
                shadow_bindings(&f.bindings, &mut out);
            }
            Item::Extension(_) | Item::Requires(_) | Item::Dimension(_) => {}
        }
    }
    out
}
