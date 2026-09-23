// Sampling cost, slack, and what the ranking does with what is left.
// The same chain as chain-anatomy.typ, against two declared budgets.
// Compile: typst compile chain-feasibility.typ chain-feasibility.svg --format svg

#set page(width: 680pt, height: auto, margin: 12pt)
#set text(font: "DejaVu Sans", size: 8.5pt)

#let code-font = "DejaVu Sans Mono"
#let boundary-bg = rgb("#fef7cd")
#let boundary-line = rgb("#e0c000")
#let slack-bg = rgb("#4285f4")
#let over-bg = rgb("#fce8e6")
#let over-line = rgb("#c5372c")
#let rank-bg = rgb("#e6f4ea")
#let rank-line = rgb("#34a853")
#let dim = rgb("#5f6368")

#let code(body) = text(font: code-font, size: 7.5pt, body)
#let ms = 2.6pt                       // one millisecond of budget
#let bar(width, fill, line, label, light: false) = box(
  width: width, height: 17pt, fill: fill, stroke: 0.7pt + line,
  align(center + horizon, text(size: 7pt, fill: if light { white } else { black }, label)),
)
#let rowlabel(body) = box(width: 96pt, align(left + horizon, text(size: 7.5pt, weight: "bold", body)))
#let note(body) = block(inset: (left: 96pt, top: 2pt), text(size: 7pt, fill: dim, body))

#align(center, text(size: 13pt, weight: "bold")[Sampling Cost, Slack, and the Order])
#v(4pt)
#align(center, text(size: 8.5pt, fill: dim)[
  Each boundary crossed costs one period of waiting plus its own execution.
  That sum is architectural: no priority assignment shortens it.
])
#v(8pt)

#stack(dir: ttb, spacing: 7pt,
  // Feasible: 200 ms
  stack(dir: ltr, spacing: 0pt,
    rowlabel[budget 200 ms],
    bar(33.3 * ms, boundary-bg, boundary-line)[33.3],
    bar(100 * ms, boundary-bg, boundary-line)[100],
    bar(66.7 * ms, slack-bg, slack-bg, light: true)[slack 66.7 ms],
  ),
  note[
    `sampling_cost` = 33.3 + 100 = 133.3 ms, one period per boundary the route
    crosses. `controllable` = 200 - 133.3 = 66.7 ms is the whole of what
    priority assignment can shape, and what the segments must fit inside.
  ],

  // Infeasible: 120 ms
  stack(dir: ltr, spacing: 0pt,
    rowlabel[budget 120 ms],
    bar(33.3 * ms, boundary-bg, boundary-line)[33.3],
    bar(86.7 * ms, boundary-bg, boundary-line)[100 ...],
    bar(13.3 * ms, over-bg, over-line)[],
  ),
  note[
    `controllable` #sym.lt.eq 0: `ChainInfeasible`. The chain is excluded from
    shaping and its members keep their local-fact priorities; the diagnostic
    names the boundary whose rate or budget has to move. The static twin is
    `scope-sampling-feasibility`, which fires before `scope-budget` for the
    same reason.
  ],

  // The order
  stack(dir: ltr, spacing: 3pt,
    rowlabel[the order],
    ..("motor", "planner", "detector", "preproc", "camera").map(n =>
      box(fill: rank-bg, stroke: 0.7pt + rank-line, radius: 2pt,
          inset: (x: 5pt, y: 3pt), code(n))),
    box(inset: (x: 3pt), align(horizon, text(size: 7pt, fill: dim)[then])),
    box(fill: rgb("#f1f3f4"), stroke: 0.7pt + rgb("#9aa0a6"), radius: 2pt,
        inset: (x: 5pt, y: 3pt), code[logger]),
    box(inset: (x: 3pt), align(horizon, text(size: 7pt, fill: dim)[and])),
    box(fill: rgb("#f1f3f4"), stroke: 0.7pt + rgb("#9aa0a6"), radius: 2pt,
        inset: (x: 5pt, y: 3pt), code[map_loader: unranked]),
  ),
  note[
    Highest priority first. Within the chain the walk is sink to source, so
    data in flight drains to the actuator before fresh work enters upstream; a
    run of adjacent boundaries is re-ordered rate-monotonically inside its walk
    position. The remainder follows by criticality bucket, then by one ascending
    budget (a timer's period or an input path's `max_latency`); a path with no
    derivable budget never ranks.
  ],
)
