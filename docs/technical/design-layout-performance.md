# Responsive layout performance

Ordinary Design edits reflow only affected responsive frame roots. A node, sibling order, or frame settings change marks its outermost responsive ancestor; nested container breakpoints are resolved through that ancestor. Inserting, removing, or reparenting frames also refreshes newly independent roots.

Canvas size, resolution, portable fonts, variable bindings, diagram synchronization, and movement links retain the full layout pass because their dependencies can cross frame boundaries. All frame settings and the resulting document are still validated. Locked dependent artwork can reject the entire operation, and Undo retains the complete previous document.

Geometry transforms prepare copies of affected nodes rather than cloning the whole document for each child. Nodes are published only after every transform and mask adjustment succeeds, so a later unsupported transform cannot leave earlier objects partly changed.

Run the bounded release workload with:

```sh
cargo run --locked --release -p emulsion-io --example design_layout_bench -- 1000
```

The workload contains 1,000 editable objects in 10 responsive grids and measures repeated spacing changes in one frame. It emits median, p95, and maximum elapsed milliseconds, validates the result, and checks repeated layout is idempotent. The optional object count accepts 10–4,000. Results depend on the machine and concurrent build/render activity; this is a reproducible local workload, not a guarantee of interaction speed in every document.

Core regressions compare incremental results against a full pass for nested frames, container breakpoints, variables, movement links, canvas resizing, reorder/insert/remove operations, and protected descendants. Transform regressions cover linked masks and failure after a preceding candidate has already been prepared.

## Linux measurement, 2026-09-28

Same release workload on this workspace machine, with concurrent compiler work:

| 1,000 objects / 10 frames / 20 measured edits | Median | p95 / maximum |
| --- | ---: | ---: |
| Before affected-root and candidate-transform changes | 40.47 ms | 61.92 ms |
| After changes | 16.29 ms | 17.84 ms |

Both runs validated the resulting document and repeated-layout idempotence. This
is about a 60% reduction in measured median edit time for this workload; it does
not represent every document, renderer cost, or another operating system.

## macOS measurement, 2026-09-28

Measured revision `1f3465b` on an Apple M1 MacBook Pro (8 CPU cores, 16 GB RAM),
macOS 26.3 (25D125), native arm64, Rust 1.98.1. The unmodified workload used the
locked release build above. Emulsion was closed and all Rust compilation finished
before measurement. AC power was connected and low power mode was disabled.
Normal desktop activity remained: a process snapshot during the larger workload
showed Docker and macOS media analysis using CPU. This was not an idle-machine
laboratory run, and thermal status was unavailable.

Each size had one discarded warmup process followed by five measured processes.
Each process discards its first edit and times 20 further edits. The table reports
the median of the five per-run medians, the range of those medians, and the worst
individual edit across the runs. The benchmark's per-run p95 uses sample index 19
of 20, so it equals that run's maximum; it is not a pooled 100-edit percentile.
Fixture creation and final correctness checks are outside the edit timings.

| Workload | Median of run medians | Range of run medians | Worst edit |
| --- | ---: | ---: | ---: |
| 1,000 objects / 10 frames | 16.08 ms | 15.94–22.91 ms | 41.74 ms |
| 4,000 objects / 40 frames | 88.53 ms | 80.77–106.57 ms | 118.89 ms |

The first four 1,000-object runs had medians of 15.94–16.33 ms; the slower fifth
run is retained. The 1,000-object aggregate is close to the documented Linux
post-change result of 16.29 ms, but different hardware, revisions, and background
load prevent treating this as a platform speed comparison. No pre-change Mac
baseline was measured, so these results do not establish a Mac speedup or
regression. The 4,000-object result shows that large-document layout remains a
significant interaction cost. These timings exclude rendering, GPU presentation,
and input delivery, and do not establish a frame-rate guarantee.

Every measured process passed document validation and repeated-layout idempotence.
The release correctness checks also passed: 33 layout tests (including incremental
versus full reflow) and 7 transform tests:

```sh
cargo test --locked --release -p emulsion-core --lib design_layout::
cargo test --locked --release -p emulsion-core --lib transform::
```

After building, reproduce the process sequence with no concurrent compilation:

```sh
for objects in 1000 4000; do
    target/release/examples/design_layout_bench "$objects" > /dev/null
    for run in 1 2 3 4 5; do
        target/release/examples/design_layout_bench "$objects"
    done
done
```

Raw per-run measurements and environment metadata:
[Mac results](../specs/reports/design-layout-macos-results.json).

## Single-object movement optimization on latest code

After pulling `782da4d`, the unmodified photo, diagram, paint and layout workloads
were rebuilt and measured separately. See the [latest Mac baseline](../specs/reports/macos-performance-suite.md).
The earlier measurements above remain attributed to `1f3465b`.

A macOS sampling profile identified repeated movement-link expansion during
responsive child sizing and positioning. The sample included fixture setup as
well as edits, so it is not used as a percentage breakdown of edit time. Both
paths repeatedly scanned and hashed all document nodes even when the target was
one unlinked leaf. `movement_roots` now returns that object immediately after
checking it exists. Valid document trees permit descendants only under groups;
groups and linked objects retain the complete expansion algorithm. Lock checks,
atomic transform publication, document validation and Undo remain in place.

The identical, unmodified release benchmark was compared against a saved binary
of `782da4d` on the same M1 Mac. Each size had one warmup process per binary,
followed by five before/after pairs with alternating execution order. Each
process measured 20 edits. No compilation or other benchmark ran concurrently.
Normal desktop background activity remained.

| Objects / frames | Before: median of run medians | After: median of run medians | Median time reduction | Worst edit before / after |
| --- | ---: | ---: | ---: | ---: |
| 1,000 / 10 | 16.20 ms | 3.77 ms | 76.8% | 17.42 / 4.27 ms |
| 4,000 / 40 | 80.79 ms | 30.84 ms | 61.8% | 84.37 / 34.11 ms |

All 20 measured processes passed document validation and repeated-layout
idempotence. The latest core suite passed 343 tests with one existing ignored
test (`cargo test --locked -p emulsion-core --lib`). The added regression checks
missing IDs, an unlinked child under a linked parent, linked leaves and links
held by group descendants. Existing lock, transform, layout and Undo regressions
also passed. Scoped rustfmt and diff whitespace checks passed.

This change reduces CPU layout edit time in the measured workload; no improvement
to RAW processing, painting, diagram GPU rendering or native-window frame rate
is inferred. The larger layout still takes about 31 ms per edit before rendering.
[Raw paired results and binary hashes](../specs/reports/design-layout-macos-optimization.json).
