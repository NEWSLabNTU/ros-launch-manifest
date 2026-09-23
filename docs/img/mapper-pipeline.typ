// The mapper pipeline: contract facts in, one order out, two realizations.
// Steps 1-4 are the agnostic core (chain_aware_rank); 5-6 are a realizer's.
// Compile: typst compile mapper-pipeline.typ mapper-pipeline.svg --format svg

#set page(width: 680pt, height: auto, margin: 12pt)
#set text(font: "DejaVu Sans", size: 8.5pt)

#let code-font = "DejaVu Sans Mono"
#let facts-bg = rgb("#fff6e8")        // authored / derived facts
#let facts-line = rgb("#e37400")
#let core-bg = rgb("#e8f0fe")         // the shared, platform-free core
#let core-line = rgb("#4285f4")
#let posix-bg = rgb("#ede9fe")        // Linux realizer
#let posix-line = rgb("#5e35b1")
#let rtos-bg = rgb("#e4f4f6")         // RTOS realizer
#let rtos-line = rgb("#0097a7")
#let warn-bg = rgb("#fce8e6")
#let warn-line = rgb("#c5372c")
#let dim = rgb("#5f6368")

#let code(body) = text(font: code-font, size: 7.5pt, body)
#let stepbox(bg, line, title, body) = block(
  width: 100%, fill: bg, stroke: 0.8pt + line, radius: 3pt, inset: 6pt,
  stack(spacing: 3pt, text(size: 8pt, weight: "bold", title), text(size: 7.5pt, body)),
)
#let down = align(center, text(size: 10pt, fill: dim)[#sym.arrow.b])

#align(center, text(size: 13pt, weight: "bold")[From Contract Facts to Two Schedulers])
#v(4pt)
#align(center, text(size: 8.5pt, fill: dim)[
  One ranking core, no OS numbers in it; each consumer realizes the same order
  with the primitives its target actually has.
])
#v(8pt)

#stack(dir: ttb, spacing: 4pt,
  stepbox(facts-bg, facts-line)[Contract facts, per (node, path)][
    `effective_trigger` (a timer's `rate_hz`, or the inputs that release it)
    #sym.dot.c `max_latency` #sym.dot.c `criticality`, itself derived from the
    hazards a node guards #sym.dot.c `max_jitter`, `miss` #sym.dot.c
    `exec_ms`, filled only from a declared budget. Chains arrive already
    derived, one per scope path with a budget (`derive::resolve_chains`).
  ],
  down,

  block(width: 100%, stroke: (paint: core-line, thickness: 0.8pt, dash: "dashed"),
        radius: 3pt, inset: 6pt)[
    #text(size: 8pt, weight: "bold", fill: core-line)[
      Agnostic core -- `chain_aware_rank(&MapperInput) -> RankedPlan`]
    #v(4pt)
    #grid(columns: (1fr, 0.42fr), column-gutter: 7pt, align: (left, left),
      stack(spacing: 4pt,
        stepbox(core-bg, core-line)[1. Feasibility, per chain][
          `sampling_cost` = #sym.Sigma over boundaries (`period_ms` + `exec_ms`);
          `controllable` = `max_latency` - `sampling_cost`.
        ],
        stepbox(core-bg, core-line)[2. Order the chains][
          criticality descending, then controllable slack ascending, then name.
        ],
        stepbox(core-bg, core-line)[3. Rank inside a chain][
          walk sink to source: each segment drains toward the sink, each run of
          boundaries is rate-monotonic in place.
        ],
        stepbox(core-bg, core-line)[4. Rank the remainder][
          criticality bucket, then one ascending budget: a timer's period or an
          input path's deadline. No budget, no rank.
        ],
      ),
      stack(spacing: 4pt,
        stepbox(warn-bg, warn-line)[`ChainInfeasible`][
          slack #sym.lt.eq 0. Excluded from shaping; the diagnostic names the
          boundary to raise.
        ],
        stepbox(facts-bg, facts-line)[`ChainFeasibleWithoutWcet`][
          a boundary with no declared `exec_ms` was counted as zero. Feasible,
          but not from measured evidence.
        ],
        block(width: 100%, inset: (x: 2pt), text(size: 7pt, fill: dim)[
          Both are warnings on the plan, never a silent verdict. The same
          distinction is checked statically as `scope-sampling-feasibility`.
        ]),
      ),
    )
  ],
  down,

  stepbox(core-bg, core-line)[`RankedPlan` -- the order, and nothing else][
    `items` in priority order, highest first; `fine_group` (segment, boundary
    run, or bucket), `coarse_group` (the chain), `tie_group` (an exact tie, to
    be collapsed), `provenance`. No priority number, no policy, no affinity:
    a realizer decides those, and two realizers may decide differently without
    disagreeing about the order.
  ],
  down,

  grid(columns: (1fr, 1fr), column-gutter: 7pt,
    stack(spacing: 4pt,
      stepbox(posix-bg, posix-line)[5-6. POSIX realizer (this crate)][
        Dense priorities from `band.max` down. When the classes outnumber the
        band, adjacent runs merge tail-first, inside a `fine_group` before a
        chain, never across a criticality bucket or the chain divide;
        overflow clamps to `band.min` with `BandTooNarrow`. Ties may appear,
        inversions may not. Policy per node by the shared tie rule; typed
        `PosixPlacement` per tier.
      ],
      stepbox(posix-bg, posix-line)[Linux][
        `SCHED_FIFO` (or `SCHED_RR` for a slice-helped tie) applied to every
        thread of the node process; `SCHED_OTHER` for the unranked.
      ],
    ),
    stack(spacing: 4pt,
      stepbox(rtos-bg, rtos-line)[5-6. RTOS realizer (nano-ros)][
        The same `RankedPlan`, realized against a board's `SchedCaps`:
        priority count and numbering direction, EDF, sporadic reservation,
        preemption threshold, affinity. Every dimension resolves to native,
        backfilled, or a recorded degradation, and `fine_group` doubles as the
        executor grouping.
      ],
      stepbox(rtos-bg, rtos-line)[Zephyr, FreeRTOS, ThreadX, NuttX][
        One kernel task per tier at a derived priority; budget, deadline and
        release window enforced by the executor where the kernel has no
        primitive for them.
      ],
    ),
  ),
)
