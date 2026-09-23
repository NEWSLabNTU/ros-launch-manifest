// Chain anatomy: what a derived route is made of.
// Shows: one causal route through the graph, its decomposition into segments
// and boundaries, and the parts of the graph the route does not cover.
// Compile: typst compile chain-anatomy.typ chain-anatomy.svg --format svg

#set page(width: 680pt, height: auto, margin: 12pt)
#set text(font: "DejaVu Sans", size: 8.5pt)

#let code-font = "DejaVu Sans Mono"
#let boundary-bg = rgb("#fef7cd")     // yellow - timer-released, a clock to wait for
#let boundary-line = rgb("#e0c000")
#let segment-bg = rgb("#e8f0fe")      // blue - input-triggered, runs on arrival
#let segment-line = rgb("#4285f4")
#let off-bg = rgb("#f1f3f4")          // grey - in the graph, not on the route
#let off-line = rgb("#9aa0a6")
#let route = rgb("#0097a7")
#let dim = rgb("#5f6368")

#let code(body) = text(font: code-font, size: 7.5pt, body)
#let nodebox(bg, line, name, sub) = box(
  fill: bg, stroke: 0.8pt + line, radius: 3pt, inset: (x: 5pt, y: 4pt),
  align(center, stack(spacing: 2pt, code(name), text(size: 6.5pt, fill: dim, sub))),
)
#let edge(label) = align(center + horizon, stack(spacing: 1pt,
  text(size: 6.5pt, fill: dim, label),
  text(size: 9pt, fill: route, weight: "bold")[#sym.arrow.r],
))
#let band(bg, line, title, sub) = box(
  width: 100%, fill: bg, stroke: (top: 1.2pt + line), inset: (x: 4pt, y: 3pt),
  align(center, stack(spacing: 1pt,
    text(size: 7pt, weight: "bold")[#title],
    text(size: 6.5pt, fill: dim, sub))),
)

#align(center, text(size: 13pt, weight: "bold")[Chain Anatomy: Segments and Boundaries])
#v(4pt)
#align(center, text(size: 8.5pt, fill: dim)[
  A chain is one causal route through the graph, derived from the `trigger:` and
  `output:` facts. Timer-released paths are boundaries; maximal runs of
  input-triggered paths are segments.
])
#v(6pt)

// Legend
#align(center)[
  #box(width: 12pt, height: 8pt, fill: boundary-bg, stroke: 0.5pt + boundary-line, radius: 2pt, baseline: 1pt)
  #h(2pt) #text(size: 7pt)[boundary (timer)] #h(12pt)
  #box(width: 12pt, height: 8pt, fill: segment-bg, stroke: 0.5pt + segment-line, radius: 2pt, baseline: 1pt)
  #h(2pt) #text(size: 7pt)[segment (on input)] #h(12pt)
  #box(width: 12pt, height: 8pt, fill: off-bg, stroke: 0.5pt + off-line, radius: 2pt, baseline: 1pt)
  #h(2pt) #text(size: 7pt)[in the graph, off the route] #h(12pt)
  #text(size: 9pt, fill: route, weight: "bold")[#sym.arrow.r]
  #h(2pt) #text(size: 7pt)[the route]
]
#v(8pt)

// The route, and under it the decomposition the mapper walks.
#align(center, block(width: 96%)[
  #grid(
    columns: (1fr, 0.5fr, 1fr, 0.5fr, 1fr, 0.5fr, 1fr, 0.5fr, 1fr),
    align: center + horizon, row-gutter: 5pt,
    nodebox(boundary-bg, boundary-line)[camera][timer 30 Hz],
    edge[image],
    nodebox(segment-bg, segment-line)[preproc][on input],
    edge[features],
    nodebox(segment-bg, segment-line)[detector][on input],
    edge[objects],
    nodebox(boundary-bg, boundary-line)[planner][timer 10 Hz],
    edge[cmd],
    nodebox(segment-bg, segment-line)[motor][on input],

    band(boundary-bg, boundary-line)[Boundary][period 33.3 ms],
    [],
    grid.cell(colspan: 3, band(segment-bg, segment-line)[Segment][
      two paths, run-to-completion on arrival, ranked drain-to-sink]),
    [],
    band(boundary-bg, boundary-line)[Boundary][period 100 ms],
    [],
    band(segment-bg, segment-line)[Segment][one path],
  )
])
#v(8pt)

// What the route does not cover.
#align(center, block(width: 96%, fill: off-bg, stroke: 0.5pt + off-line, radius: 3pt, inset: 7pt)[
  #set text(size: 7.5pt)
  #grid(columns: (auto, 1fr), column-gutter: 7pt, row-gutter: 4pt, align: (left, left),
    code[lidar #sym.arrow.r planner],
    [a second input to a boundary: it waits for the same tick, and adds no
     period of its own to this route],
    code[detector #sym.arrow.r logger],
    [a consumer off the route. It never ranks inside the chain; it falls to the
     non-chain remainder, bucketed by criticality and ordered by its own budget],
    code[map_loader #sym.arrow.r planner],
    [`once`-triggered, read as state (`state: true`). It carries no causality,
     so the derivation does not walk it and the node never ranks],
  )
])
#v(6pt)
#align(center, text(size: 7pt, fill: dim)[
  A node on several chains keeps the strongest rank it earns anywhere
  (`RankItem` projection, `chain_aware_mapper.rs`). The route itself is derived
  in `derive/src/graph.rs`, never authored.
])
