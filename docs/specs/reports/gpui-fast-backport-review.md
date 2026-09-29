# GPUI Fast backport review

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
