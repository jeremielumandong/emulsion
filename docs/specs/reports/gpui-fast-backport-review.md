# GPUI Fast backport review

Current status (2026-09-29): the optimized scene sorter is enabled directly in
production `Scene::finish`. See [production integration](#production-integration)
for the integration and validation. Earlier sections retain the experiment's
historical results and limitations.

Reviewed 2026-09-28 against Emulsion `3ab79c3` and its working tree, using
GPUI Kit 0.6.4 / gpui-pre 0.3.5. Upstream was pinned to
[`4b59e246c9120ccb02a0ed20de24785b5a6d8de6`](https://github.com/longbridge/gpui-fast/tree/4b59e246c9120ccb02a0ed20de24785b5a6d8de6).
The initial review was documentation-only. The implementation follow-up below
records the selective backports subsequently applied; unrelated working-tree
changes are preserved.

## Initial recommendation

Adopt selected changes in our existing vendor tree, beginning with the balanced
paint-order bounds tree. It is independently portable and showed a useful local
microbenchmark improvement while producing the same orderings. Evaluate bounds
replay next. Treat automatic view retention as a separate framework experiment,
with an explicit fallback and application-wide invalidation tests.

These changes target interface construction, layout and paint preparation. They
do not establish faster Vello artwork rendering, RAW development, image decoding,
video playback or export.

## Current upstream versus its earlier history

The repository changed substantially in
[`1304f17`](https://github.com/longbridge/gpui-fast/commit/1304f176f6a0fa660ad5641e8dff4180b16eefea).
Its current implementation places retained layout, text measurement, view
dependencies and ordering in `crates/gpui/src/fast/`. The next dependency change,
[`4b59e24`](https://github.com/longbridge/gpui-fast/commit/4b59e246c9120ccb02a0ed20de24785b5a6d8de6),
distinguishes notifying an entity from updating it.

Several optimizations described in the earlier README are historical candidates,
not features still present at this pinned head: compact paint-operation records,
prehashed global element IDs, boxed listener storage, per-run glyph rendering
setup, the macOS sized-font cache and deferred truncation wrappers. The public
`memo` and `.key()` additions were also removed in that reorganization. Do not
base an integration on the earlier README or its deleted performance guide.

## Candidate assessment

| Change | Current status and Emulsion applicability | Adoption priority |
| --- | --- | --- |
| Balanced bounds tree | Active in upstream's `fast/bounds_tree.rs`. Our existing `bounds_tree.rs` is byte-identical to the parent of its original balancing commit. It controls primitive order without changing layout, text, input or GPU APIs. | First backport candidate; benchmarked below. |
| Reuse bounds orderings | Active in the same upstream module. Reuses orderings only while the recorded bounds and relevant preceding intersections permit it; bounded search falls back to rebuilding. Does not reuse texture pixels or document content. | Second, after changed-frame/scene regressions and memory measurement. |
| Carry text measurements across frames | Active in `fast/text.rs`, coupled to retained identity/layout. Emulsion already handles intrinsic non-wrapping text and refreshes paint state independently. | Do not replace our text/layout code wholesale; merge only with differential glyph, truncation, DPI and decoration checks. |
| Automatic retained views | Active in `fast/retained.rs`, `dependencies.rs` and `splice.rs`. Avoids more work than our allocation-slot layout reuse, but retains hitboxes, listeners, dispatch and other frame state. | Separate experimental workstream; not an immediate backport. |
| Defer unused truncation wrappers | Historical [`a2b646f`](https://github.com/longbridge/gpui-fast/commit/a2b646fba78479ffbeb772afb4365d621b1532a5). Our measured-text path still acquires a wrapper before checking whether it truncates. A small selective adaptation can avoid font lookup/pool work. | Low-risk follow-up, preserving our existing intrinsic-text and kerning/truncation fixes. No local timing measured yet. |
| Reuse macOS sized native fonts | Historical [`a9a9533`](https://github.com/longbridge/gpui-fast/commit/a9a95339581041dab3933e5718ce913338c3c01c). Our macOS text system still creates sized fonts during line shaping. | Evaluate separately on macOS, including fallback fonts, emoji, animated sizes and ligature boundaries. |
| Per-run glyph setup | Historical [`07f880a`](https://github.com/longbridge/gpui-fast/commit/07f880a56b183851dd77a29f4acfc816d2bb2f0c). Hoists repeated rendering decisions from individual glyphs. | Profile first; upstream's own commit reports no clear whole-frame improvement. |
| Compact paint-operation records | Historical [`bb6045a`](https://github.com/longbridge/gpui-fast/commit/bb6045aed03c20c0b5883c3f9d7634c967d8ba3f). Avoids duplicate primitive storage but changes sorting/replay representation. | Later; preserve our custom external-texture surfaces and their lifetimes. |
| Prehashed IDs / boxed listeners | Historical [`ffbebf2`](https://github.com/longbridge/gpui-fast/commit/ffbebf2218585cc25f055a6981373694f05e65d7) / [`59ac6d4`](https://github.com/longbridge/gpui-fast/commit/59ac6d4). Alters identity internals or interaction storage; prehashing removed `DerefMut` in that revision. | Lower priority until measured in Emulsion; focus, accessibility and event dispatch need regression coverage. |

## Existing work to preserve

- `gpui-pre/src/taffy.rs`, `window.rs` and `elements/text.rs`: our switchable
  allocation-slot layout reuse, intrinsic text specialization, fresh measured
  callbacks, context cleanup and cold comparison path.
- `elements/list.rs`: retained height estimates needed to scroll through large
  layer/effect lists before every row has been measured.
- `scene.rs` / `window.rs` and the platform crates: external textures, shared
  graphics devices, native surfaces and frame-completion behavior.
- Presentation policy: software-renderer pacing, fallback and recovery fixes.
- `editor/render_regions.rs`: separate canvas/sidebar notifications and explicit
  sidebar caching; navigation updates the panels that depend on the viewport.
- Current text measurement fixes: text that fits its shaped width must not become
  incorrectly truncated, and a previously truncated measurement must not poison
  a later unconstrained measurement.

Automatic retention is not just a faster implementation of our existing switch.
Upstream explicitly documents that a view reading an entity notified without an
update is not necessarily rebuilt. Mutating a shared cell through `read()` needs
an `update()` for dependent readers to observe it. State outside entities/globals/
tracked scroll state also needs explicit notification. Emulsion uses shared
`Rc<RefCell<...>>` canvas, SVG and GPU caches and mutable canvas bounds, so those
dependencies and asynchronous completions must be audited before adoption.
This identifies an integration risk, not a confirmed rendering bug.
See the pinned [retention contract](https://github.com/longbridge/gpui-fast/blob/4b59e246c9120ccb02a0ed20de24785b5a6d8de6/docs/retained-mode.md).

## Local ordering experiment

Compared three implementations in one optimized executable:

1. Emulsion's actual current bounds-tree source.
2. The original balanced-only implementation at
   [`887a2c1`](https://github.com/longbridge/gpui-fast/commit/887a2c12e97144fa27b851662c826b0138384be2).
3. The active balanced-plus-replay implementation at the pinned upstream head.

The harness imports the real `gpui::Bounds`, `Point`, `Size` and `Half` types
from our compiled release GPUI dependency. It includes the three tree modules
without altering them. Each implementation was checked against a brute-force
intersection/order oracle through static, repeated, moved, reversed/deleted and
replaced histories. Five workload sizes/patterns and an additional negative,
touching and zero-size rectangle case passed for all three implementations.

Linux x86_64, AMD Ryzen 7 8700G, Rust 1.98.1, `rustc -O`. Each table entry is the
median of 15 samples of 10 frame fills, after two warmup samples. Execution order
rotates among the three implementations. The moving cases shift every 17th
rectangle on successive frames. This was a shared development machine, not an
otherwise idle performance lab.

| Workload | Current, ms | Balanced only, ms | Balanced + replay, ms |
| --- | ---: | ---: | ---: |
| Grid: 10,000 rectangles, unchanged | 3.9203 | 2.5114 | 0.0343 |
| Grid: approximately 6% moving | 4.0263 | 2.8609 | 2.7461 |
| Rows: 1,500 rectangles, unchanged | 1.1561 | 0.1877 | 0.0052 |
| Rows: approximately 6% moving | 1.2164 | 0.2059 | 0.2062 |
| Overlapping: 10,000 rectangles, unchanged | 1.9490 | 0.7786 | 0.0345 |
| Overlapping: approximately 6% moving | 1.8853 | 0.8159 | 0.8827 |
| Random: 5,000 rectangles, unchanged | 4.3386 | 2.9056 | 0.0170 |
| Random: approximately 6% moving | 4.0025 | 2.9166 | 2.9831 |
| Small: 32 rectangles, unchanged | 0.0010 | 0.0009 | 0.0001 |
| Small: approximately 6% moving | 0.0011 | 0.0011 | 0.0002 |

These timings measure bounds insertion/order assignment only. They exclude
element building, layout, text shaping, GPU submission and Vello document work.
The nearly empty work on unchanged replay frames is not a 99% application speedup.
Replay can cost more than balancing alone on changing workloads and retains
previous/current bounds history; memory and actual scene behavior need measurement.

Review artifacts on the development machine:

- `/tmp/emulsion-gpui-fast-review-results/bounds_compare.rs`
- `/tmp/emulsion-gpui-fast-review-results/bounds-final-results.txt`
- `/tmp/emulsion-gpui-fast-review-20260928/` (pinned source checkout)

The original Emulsion source and the pre-balancing upstream source share SHA-256
`c3da5abcd110191eea8b7b73a88bb8d1fd89251549e889c9cd42d1a7857a1b4c`.
The balanced-only source has SHA-256
`ac88381ee57209cd38f3035bd7081322c793522e1345c2e6e3361969e52dafec`.

## Backport acceptance

Keep each optimization separately reviewable. Preserve upstream Apache notices,
add our modified-file notice, and record exact provenance in
`vendor/gpui/gpui-pre/EMULSION_CHANGES.md` when code is adopted. Do not replace the
vendored GPUI family or regenerate Cargo.lock merely to apply these changes.

For the first backport, run ordering/scene replay tests plus the existing
`layout_reuse_tests` and `canvas_invalidation_tests`, then text, scrolling,
tool-selection, popup, drag/drop and external-texture checks. Exercise both cold
and retained layout settings. Compare the existing release editor-navigation and
layout benchmarks, and actual Library, Layers, Design and Photo interactions at
the same size/DPI; report CPU frame time and memory, not synthetic percentages
as a promised app-wide gain. Native Windows/macOS runs remain necessary for a
cross-platform release. No such full integration or platform result is claimed
by this source review.


## Implementation follow-up

Implemented in the existing GPUI 0.3.5 vendor package, without changing dependency
versions: balanced bounds insertion, bounds/order-only replay, and deferred line
wrapper acquisition. Our replay additionally caps each history at 16,384 bounds;
exceeding that limit rebuilds any replayed prefix before discarding history and
continues balanced insertion. The upstream 32,768-comparison budget limits replay
search work. Large or changing scenes retain the balanced-tree fallback.

Existing text fitting, intrinsic text/Taffy reuse, cached views, canvas external
textures, and renderer policies remain in place. No automatic view retention or
entity-notification changes were imported. Exact provenance and memory limits are
in `vendor/gpui/gpui-pre/EMULSION_CHANGES.md`.

A repeat of the bounds-only benchmark with the actual bounded backport compared
the original implementation, balanced insertion alone, and the shipped replay.
For moving scenes (roughly 6% of bounds moved each frame), median original →
backport CPU times were 3.983 → 2.706 ms for the 10,000-bound grid,
1.222 → 0.205 ms for 1,500 row bounds, 2.037 → 0.963 ms for 10,000 overlapping
bounds, and 3.841 → 3.043 ms for 5,000 random bounds. Repeated static histories
replayed in 0.006–0.059 ms across those workloads. Timings used 15 rotated samples
of 10 frames after warmup; concurrent builds add noise. These are paint-order
microbenchmarks, not whole-application FPS measurements.

Validation used an isolated checkout of `3ab79c3` plus this backport, excluding
concurrent Photo/Diagram edits in the shared working tree. The production GPUI
and UI test target compiled on Linux. Eight bounds-tree tests passed against the
actual backported source, including randomized brute-force ordering comparisons,
changed-frame replay, tree depth, and history-limit fallback. The two Scene
regressions passed against the newly compiled GPUI, including changed paint/clips,
nested cached layers, and releasing/replacing external texture handles. The
13-test `layout_reuse` filter passed (including three-way cold/geometry-only/
intrinsic text parity and narrow/wide truncation recovery).

The vendor inventory, three license-staging tests, ten renderer-policy tests, and
Rust formatting checks also passed. Native graphics rendering, real-font visual
comparison, Windows/macOS execution, and application-wide release performance
measurements remain outside this headless validation.

Repeat the application regressions with:

```sh
cargo test --locked -p emulsion-ui --lib layout_reuse
cargo test --locked -p emulsion-ui --lib gpui_fast
cargo test --locked -p emulsion-ui --lib canvas_invalidation
EMULSION_RETAINED_LAYOUT=1 cargo test --locked -p emulsion-ui --lib -- --test-threads=1
EMULSION_RETAINED_LAYOUT=0 cargo test --locked -p emulsion-ui --lib -- --test-threads=1
python3 scripts/check-gpui-vendor.py
python3 scripts/test-license-staging.py
bash scripts/test-renderer-policy.sh
```

Full UI results were identical with `EMULSION_RETAINED_LAYOUT=1` and `=0`:
**641 passed, 17 failed, 3 ignored** per run. All 17 failing test names also fail
in the earlier `target/debug/deps/emulsion_ui-a08bcda92303b9d7` binary (built
before this bounds backport). The focused rerun of those names returned
0 passed / 17 failed, so the suite is not described as green.

Failures cover the missing Print shortcut category, the old layer opacity
locator, template counts/category IDs, narrow diagram chrome, workspace
destination expectations, selective-color and shape-property controls, library
navigation, brush settings, and the curves editor. For Print, extracting and
running `shortcut_group` from unmodified `3ab79c3` additionally reproduces `Other`
for `("Print", "workspace")`. These failures were left outside this framework
backport; concurrent Photo/Diagram UI work is preserved.

Machine-local full-run logs are `/tmp/emulsion-gpui-fast-ui-retained.log`,
`/tmp/emulsion-gpui-fast-ui-cold.log`, and
`/tmp/emulsion-gpui-fast-baseline-failures.log`. The separate final Scene test
source was also compiled directly against the new GPUI artifacts and both tests
passed, including successive texture replacement across replayed frames.


## September 29 update and scene-sorting benchmark

Reviewed the local `C:/development/github/gpui-fast` checkout at
`7ab23f46f2ba3a040ceb27d387383a2896bc5ae1` against Emulsion
`2f56e0984d0e402828ef09b96eacfadc0aaf76de`. This is the supplied local
checkout's head; no remote fetch was performed.

The newer work is compatible with selective backports, but is not a drop-in
replacement for Emulsion's vendor tree. Changes since the earlier review include:

- The bounds tree now uses a spatial grid and coarse changed-bounds filtering.
  This is a separate candidate to benchmark, retaining Emulsion's history cap.
- Scene sorting uses reusable index/gather buffers and groups sprites by atlas
  texture before tile ID. The experiment below evaluates this candidate.
- Text-style resolution, glyph lookup, native macOS fonts, path tessellation,
  dispatch, IDs, and interactivity have additional caching/allocation changes.
  These have not been benchmarked in Emulsion in this follow-up.
- Automatic view retention now distinguishes queried inputs, updated/notified
  entities, global existence, and innermost hover/scroll invalidation. Cached
  nested views can be spliced. The latest text fix (`391a90b`) clears a carried
  measurement when changed text must be laid out afresh. These changes need a
  coordinated retention integration and differential application testing.
- The new Windows renderer fast loop explicitly skips
  `PrimitiveBatch::Surfaces`. Emulsion's `draw_surfaces` composites application
  textures. Copying that loop unchanged would omit the canvas surfaces. Any
  renderer backport must preserve this path and device-recovery behavior.

### What was measured

Three algorithms run in one executable: an exact copy of Emulsion's original
`Scene::finish`, stable sorting with only the new atlas grouping key, and an
adaptation of gpui-fast's index/gather sorter from
`8111e627725c1868930141bfba3c1663acc8e978`. The original copy is checked against
actual production `Scene::finish`; candidate output is checked against the
independent stable sorter with the intended texture keys.

The adaptation detects already-sorted input before allocating, uses `usize`
indices, and gathers only `Copy` primitives. Paths and surfaces retain their
original stable sort to avoid cloning vertex allocations or retaining external
textures in scratch storage. It is an experiment under `benches/support`, not an
exact whole-fork comparison. No new sorting or retention code is enabled in the
production vendor tree.

Windows 11, AMD Ryzen 7 8700G, Rust 1.98.1. All compared sorting algorithms are
compiled together at `opt-level=3` with release-style assertion/overflow settings,
using real GPUI primitive types. The linked GPUI library is also optimized at
level 3; its dev-profile checks remain enabled. No application UI, GPU submission,
layout, shaping, or document rendering is included in the timed region.

For each of 28 workloads: five warmup samples, then 21 samples of 64 frames,
rotating the order of the three algorithms each sample. Input vectors are reset
outside the timer; scratch buffers survive between frames. The seed is fixed.
Two complete runs were taken after compilation finished, with ordinary desktop
services still active. Values below are the mean of the two run medians in
microseconds per frame; negative change means less sorting time. Small workloads
include timer overhead, and the raw files retain candidate p10/p90 values.

| Workload | Original, us | Candidate, us | Sorting time change | Batches, original -> candidate |
| --- | ---: | ---: | ---: | ---: |
| Shuffled quads, 2,000 | 71.95 | 30.71 | -57.3% | 1 -> 1 |
| Reverse-order quads, 2,000 | 5.32 | 7.62 | +43.1% | 1 -> 1 |
| Shuffled quads, 10,000 | 450.13 | 337.90 | -24.9% | 1 -> 1 |
| Shuffled glyphs, one atlas, 10,000 | 425.16 | 277.17 | -34.8% | 1 -> 1 |
| Ordered glyphs, four atlases, 10,000 | 9.19 | 200.98 | +2086.7% | 10000 -> 1252 |
| Shuffled glyphs, four atlases, 10,000 | 422.58 | 448.16 | +6.1% | 7640 -> 256 |

The gain is workload-dependent. Reusable scratch removes the observed one
transient sort allocation per frame after warmup, but a 10,000-primitive unsorted
scene additionally retains 1,680,000 bytes for quads or 1,200,000 bytes for glyphs.
These counts exclude allocator overhead and existing scene storage. Buffers keep
their high-water capacity; they are not a bound on total memory. Allocations are
counted in separate calls with counters disabled during timing.

Atlas grouping reduces many draw batches but can add substantial CPU sorting
work to an already ordered scene. Batch count is not a measured GPU speedup.
The reverse-order and large mixed-atlas regressions prevent recommending this
sorter as an unconditional production update. Keep the benchmark for a subsequent
renderer-inclusive comparison on captured Emulsion scenes before enabling it.
The earlier balanced-bounds/replay backport remains unchanged.

### Reproduction and validation

```sh
python scripts/bench-gpui-scene.py
```

The runner builds only the production GPUI package, compiles both sorting
algorithms with identical optimization settings, runs the four scene regressions,
and saves two CSV runs plus metadata under `target/performance/gpui-scene/`.
It avoids rebuilding the entire application merely to measure these algorithms.
The Cargo `scene_finish` bench target is also available, but different Cargo
profiles can change the comparison; use the command above for these figures.

All four scene tests passed, covering paint/clips/cached layers, texture release,
stable equal-key ordering across reused frames, and atlas batching across all
three sprite kinds with an overlapping quad between background and foreground
sprites. All benchmark output comparisons passed; targeted Rust formatting and
`git diff --check` passed. This is not a full application or native-renderer test.

The vendor inventory check reports 29 existing license hash mismatches on this
Windows checkout: every file matches both its recorded upstream hash after CRLF
normalization and its committed Git blob. No license or production vendor files
were changed to mask that existing checkout issue.

Saved artifacts: [first run](data/gpui-fast-scene-20260929/run-1.csv),
[second run](data/gpui-fast-scene-20260929/run-2.csv),
[build/source metadata](data/gpui-fast-scene-20260929/metadata.json), and
[regression results](data/gpui-fast-scene-20260929/tests.log).


## Sorting regression follow-up

The benchmark candidate now fixes the previously observed reverse-order and
large mixed-atlas regressions. It remains isolated from production while full
application/renderer validation is outstanding.

The revised algorithm:

- Packs draw order, texture index, tile ID and original position into a 16-byte
  record. Comparisons/passes no longer repeatedly fetch keys from large primitives.
- Uses stable radix passes for larger inputs, skipping constant bytes and
  reducing the prefix-sum range when only a few bits vary. Small inputs use the
  standard unstable sort with the original position as a stable tie breaker.
- Resolves the sorted permutation in place. Scratch owns only integer records,
  with no duplicate primitive arrays, vertex allocations or texture references.
- Reverses strictly descending keys directly. Equal keys use stable sorting so
  insertion order is preserved.
- Leaves sprites already ordered by the original `(order, tile)` key alone.
  Atlas regrouping occurs only when sorting is already needed. This deliberately
  gives up the previous ordered-atlas batch reduction to avoid charging CPU time
  for a renderer benefit that has not been measured.

The benchmark now compares four variants in the same process: the production
algorithm, texture grouping alone, the initial candidate preserved in
`benches/support/scene_sort_initial.rs`, and the revised candidate. All use the
same compiler settings. The expanded 52-case matrix includes equal keys,
reverse order with ties, nearly ordered input, sparse draw-order IDs and
already-grouped atlases, in addition to the original 28 cases. Sizes remain
32, 256, 2,000 and 10,000 primitives. Two complete runs use 21 rotating samples
of 64 frames after five warmup samples per case.

Values are the mean of the two run medians, in microseconds per frame. The
initial candidate column is measured alongside the fix, not copied from the
older run; this avoids interpreting different run conditions as a code gain.

| Workload | Production algorithm | Initial candidate | Revised candidate | Revised vs production |
| --- | ---: | ---: | ---: | ---: |
| Reverse-order quads, 2,000 | 5.370 | 7.655 | 5.127 | -4.5% |
| Shuffled quads, 2,000 | 71.661 | 30.515 | 22.451 | -68.7% |
| Shuffled quads, 10,000 | 544.114 | 390.570 | 404.234 | -25.7% |
| Shuffled glyphs, one atlas, 10,000 | 485.686 | 378.089 | 165.177 | -66.0% |
| Ordered glyphs, four atlases, 10,000 | 10.574 | 255.367 | 9.428 | -10.8% |
| Shuffled glyphs, four atlases, 10,000 | 476.125 | 505.022 | 204.274 | -57.1% |
| Nearly ordered quads, 256 | 1.255 | 2.984 | 1.350 | +7.6% |

The revised sorter was faster than production in 51 of 52 cases. The remaining
256-quad nearly ordered case cost about **0.095 us more** (7.6%); this small
regression is not represented as a universal speedup. The 10,000 shuffled-quad
case is approximately 3.5% slower than the initial candidate, while still 25.7%
faster than production, in exchange for substantially less scratch memory.

For 10,000 unsorted primitives, retained scratch is now **320,000 bytes**, down
from 1,680,000 for quads or 1,200,000 for glyphs (81% and 73% reductions).
Already-ordered and strictly reversed scenes allocate no new scratch. Warm
sorting in all measured quad/glyph cases allocates nothing. Buffer capacities
still retain their high-water size; this is not a total scene-memory limit.

For the shuffled four-atlas 10,000-glyph case, batches still fall from 7,640 to
256. For ordered four-atlas glyphs, they remain at the production count of
10,000 rather than the initial candidate's 1,252. CPU sorting and draw-batch
counts are measured separately; GPU time and application FPS remain unmeasured.

Seven regressions pass, including all 720 six-item permutations, reverse order
with equal keys, full-width 32-bit draw/texture/tile IDs, repeated equal keys,
ordered-sprite preservation, all three sprite kinds, overlapping paint order,
cached scene state and external-texture release. Every benchmark output also
matches its independent stable-sort oracle. Rust formatting and diff whitespace
checks pass. No production vendor files were changed by this follow-up.

Reproduce with:

```sh
python scripts/bench-gpui-scene.py --output target/performance/gpui-scene-fixed
```

Saved artifacts: [first run](data/gpui-fast-scene-fixed-20260929/run-1.csv),
[second run](data/gpui-fast-scene-fixed-20260929/run-2.csv),
[source/build metadata](data/gpui-fast-scene-fixed-20260929/metadata.json), and
[regression results](data/gpui-fast-scene-fixed-20260929/tests.log).

## Further optimization passes

The follow-up candidate is preserved in
`benches/support/scene_sort_previous.rs` so subsequent gains are measured against
it in the same executable. The production GPUI implementation remains unchanged.

The optimization passes addressed distinct costs:

1. Carry one primitive through each permutation cycle instead of swapping two
   large primitives at every step.
2. Read individual 32-bit radix fields instead of shifting packed 128-bit keys.
3. Reverse descending input with ties directly, then restore the original order
   within each equal-key run.
4. Use 8-byte records for ordinary primitives and 16-byte records for sprites.
5. Choose comparison sorting for small inputs with many varying key bytes, and
   specialize radix field access. This fixes sparse-ID regressions exposed by
   the fourth pass.

Four further experiments changed the already-ordered sprite scan: direct tuple
comparisons, packed 64-bit keys, adjacent windows, and groups of four independent
comparisons. None consistently removed the largest ordered-glyph regression,
so those experiments were reverted. The final candidate retains the five
changes above and the simple original ordering check. The
[nine-pass experiment summary](data/gpui-fast-scene-optimized-20260929/optimization-passes.json)
records the tested matrices and regression screening results, including rejected
experiments. Intermediate matrices and harness versions differ; their absolute
times should not be compared across passes.

The matrix now includes mixed quad/glyph scenes and full-width texture/tile IDs.
Validation covers 16 workloads at 32, 128, 256, 512, 2,000, 10,000 and 16,384
primitives, with three seeds. Each case uses 25 timed samples of 64 frames after
five warmup samples. Both execution positions and physical scene buffers rotate
evenly among the five algorithms. Allocation counters remain disabled during
timing, and every output is checked against its independent stable-sort oracle.
These are synthetic CPU measurements on a normal Windows desktop, without fixed
CPU clocks or thermal controls; they do not establish application FPS or GPU
performance.

Final values below are the arithmetic mean of three run medians, in microseconds
per frame. The previous candidate is measured alongside the new candidate in
each run, rather than taken from earlier results under different conditions.

| Workload | Production | Previous candidate | Optimized candidate | Change vs previous |
| --- | ---: | ---: | ---: | ---: |
| Nearly ordered quads, 256 | 1.448 | 1.458 | 0.709 | -51.4% |
| Shuffled quads, 10,000 | 654.735 | 453.758 | 319.493 | -29.6% |
| Reverse-order quads with ties, 10,000 | 934.907 | 387.145 | 116.928 | -69.8% |
| Nearly ordered quads, 10,000 | 390.992 | 141.196 | 103.606 | -26.6% |
| Shuffled glyphs, one atlas, 10,000 | 551.986 | 214.298 | 122.789 | -42.7% |
| Shuffled glyphs, four atlases, 10,000 | 515.976 | 218.257 | 134.060 | -38.6% |
| Shuffled mixed primitives, 10,000 total | 492.848 | 182.076 | 93.326 | -48.7% |
| Reverse-order quads, 16,384 | 132.796 | 131.943 | 136.229 | +3.2% |
| Ordered glyphs, four atlases, 16,384 | 37.271 | 41.058 | 40.817 | -0.6% |

Mean sorting time is lower than production in **107 of 112 cases** and lower
than the previous candidate in **89 of 112 cases**. These counts include small
differences and are not claims of statistical significance. The previously
regressing nearly ordered 256-quad case now takes about half as long as either
baseline.

The remaining production regressions are explicit:

- Reverse-order quads at 10,000 items cost **5.259 us more (5.8%)**; at 16,384,
  **3.433 us more (2.6%)**. The additional equal-key handling has not established
  a win for strictly descending input.
- Ordered four-atlas glyphs at 16,384 items cost **3.546 us more (9.5%)**. The
  previous candidate has a similar cost; the extra scan experiments did not
  consistently remove it.
- Reverse-order and equal-key quads at 2,000 items cost 0.034 and 0.040 us more,
  respectively.

Only the 10,000 reverse-order quads and 16,384 ordered glyphs exceed the
screening threshold of both 5% and 0.05 us. This threshold is an engineering
screen, not a significance test. Overall CPU sorting is substantially improved,
but the candidate remains an experiment pending application/renderer validation.

Scratch for 10,000 unsorted quads falls from 320,000 to **160,000 bytes**. Glyph
scratch remains **320,000 bytes**. Separate record layouts have a tradeoff: a
mixed 10,000-item scene (5,000 quads and 5,000 glyphs) retains **240,000 bytes**,
versus 160,000 with the previous shared buffer. Scratch retains high-water
capacities. Paths and surfaces continue to use their existing stable sort.
All **336 final case/run measurements** report zero warm candidate allocations.

The seven correctness tests include all 720 six-item permutations, stable equal
keys, full-width identifiers, reuse across frame sizes, all three sprite kinds,
overlapping paint order, cached scene state and texture release. Reused-frame
tests now also exercise both sides of the 128- and 1,024-item dispatch boundaries.

Reproduce the final matrix with:

```sh
python scripts/bench-gpui-scene.py --runs 3 --seed-step 7919 --sizes 32,128,256,512,2000,10000,16384 --output target/performance/gpui-scene-final-validation
```

For focused follow-up work, `--workloads atlas4-ordered,atlas4-random` selects a
subset without changing the timing procedure.

Saved artifacts: [run 1](data/gpui-fast-scene-optimized-20260929/run-1.csv),
[run 2](data/gpui-fast-scene-optimized-20260929/run-2.csv),
[run 3](data/gpui-fast-scene-optimized-20260929/run-3.csv),
[aggregated results](data/gpui-fast-scene-optimized-20260929/summary.json),
[source/build metadata](data/gpui-fast-scene-optimized-20260929/metadata.json), and
[regression tests](data/gpui-fast-scene-optimized-20260929/tests.log).
All recorded source hashes match the final working files. Targeted Rust
formatting, Python parsing and diff whitespace checks pass. No production GPUI
vendor files were modified.

## Production integration

The optimized sorter now runs unconditionally from the vendored GPUI
`Scene::finish` implementation. There is no setting, environment switch or
benchmark feature needed to enable it. `src/scene/sort.rs` contains the validated
cached-key, adaptive radix/comparison and carry-permutation implementation;
`Scene` owns the reusable integer buffers. The key-sorting helpers match the
validated candidate exactly. Only their imports, visibility and ownership wiring
changed for production.

`Scene::clear` preserves scratch capacity but still releases the frame's paint
operations, paths, primitives and textures. Frame swaps carry each Scene's own
scratch with it; dropping a Scene frees its buffers. Legacy-ordered sprites keep
their sequence, equal keys remain stable, and paths/surfaces retain their
existing sort and native-renderer behavior. The production patch does not adopt
the upstream Windows renderer that omits Emulsion's external-texture surfaces.
Atlas regrouping stays within a draw order; GPUI's `Window::paint_layer` contract
also requires geometry grouped in an explicit painting layer to be non-overlapping.

The application scene regressions now call production `Scene::finish` directly.
An additional regression swaps frame scenes and varies primitive counts while
checking shadows, quads, underlines and path vertices against independent stable
sorts. Existing tests cover all three sprite types, atlas batching, overlap
order, full-width IDs, equal-key stability, cached paint and external textures.

The benchmark now times the linked production method. The frozen optimized
candidate remains an output oracle outside timing. Its `production_*` CSV
columns compare against the copied legacy sorter and two historical candidates.
The runner matches the linked GPUI library's assertion/overflow settings for all
timed alternatives. Earlier experiment runs used release-style settings in the
standalone candidates, so compare timings within each run, not across those
historical and production result sets.

Validation of the integrated implementation:

- `cargo build --locked -p emulsion-app --bin emulsion --example renderer_smoke`
  passed, producing the application and native-renderer test executable.
- All eight production Scene regressions passed, including the added frame-swap
  and shared-scratch test. The standalone runner links the actual GPUI library.
- `cargo clippy --locked -p gpui-pre --lib --no-deps -- -D warnings` passed.
- UI all-target Clippy passed with `-D warnings -A dead_code`. The unmodified
  strict command stops on the existing Windows-only `write_device_pdf` dead-code
  warning; the application build also reports the existing `shared_generation`
  warning. No unrelated source changes were made to suppress them.
- All ten native renderer-policy tests passed (Windows rendering, wgpu adapter
  selection, and unchanged-scene presentation), compiled directly from the three
  sources used by `scripts/test-renderer-policy.sh`.
- Vendor inventory still reports the 29 existing Windows CRLF license hashes.
  Every normalized license and committed Git blob matches the recorded upstream
  hash. License/notice manifest coverage passes, but Bash license-staging tests
  fail on CRLF manifest entries/fixtures in this checkout. No license texts,
  recorded hashes, or packaging scripts were changed.

Both final production benchmark runs passed all 112 output comparisons, including
comparison with the frozen optimized candidate. All **224 case/run measurements**
have zero warm production allocations. Mean sorting time is lower than the old
sorter in **111 of 112 cases**; this includes small differences and is not a
statistical-significance claim. Representative means of the two run medians:

| Workload | Old sorter (us) | Production finish (us) | Change |
| --- | ---: | ---: | ---: |
| Shuffled quads, 10,000 | 480.456 | 236.930 | -50.7% |
| Shuffled glyphs, one atlas, 10,000 | 437.929 | 100.071 | -77.1% |
| Shuffled glyphs, four atlases, 10,000 | 437.877 | 126.664 | -71.1% |
| Shuffled mixed primitives, 10,000 total | 417.863 | 91.473 | -78.1% |

The remaining slower case is 16,384 legacy-ordered four-atlas glyphs: **3.438 us
more (9.4%)**. The algorithm was integrated unchanged; differences from earlier
candidate runs do not establish that the reverse-input tradeoff has disappeared
on all machines/build profiles. The prior CPU/GPU/FPS limitations still apply.

The hardware renderer smoke test initialized the Radeon RX 7700 XT successfully,
but its hidden window did not complete the two draw/present callbacks before
the watchdog. An unsandboxed retry removed the initial desktop-access error but
also timed out with the hidden window. This is **not a passed presentation test**.
Visible-window hardware/software smoke testing remains pending user approval.

Workspace formatting, targeted vendor formatting, and diff whitespace checks
passed. All recorded production benchmark source hashes match the final files.

Reproduce production measurements with:

```sh
python scripts/bench-gpui-scene.py --runs 2 --seed-step 7919 --sizes 32,128,256,512,2000,10000,16384 --output target/performance/gpui-scene-production-validation
```

Artifacts: [run 1](data/gpui-fast-scene-production-20260929/run-1.csv),
[run 2](data/gpui-fast-scene-production-20260929/run-2.csv),
[summary](data/gpui-fast-scene-production-20260929/summary.json),
[source/build metadata](data/gpui-fast-scene-production-20260929/metadata.json),
[production scene tests](data/gpui-fast-scene-production-20260929/tests.log), and
[hidden-window smoke log](data/gpui-fast-scene-production-20260929/renderer-hardware.stderr.log).
