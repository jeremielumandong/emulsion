# Cross-frame layout reuse experiment

This implements a bounded framework step toward incremental layout. It does not
retain GPUI element objects or skip their `render` methods. Emulsion enables it
**by default**, with a cold-layout comparison path in the same binary. The
standalone GPUI framework default remains cold layout.

Measurements of the initial geometry-only prototype and a manual editor panning observation are available
in [the release results](layout-reuse-release-results.md). Fixed geometry benefits;
the manual editor trial did not demonstrate a CPU saving. The subsequent
[intrinsic-text release results](intrinsic-text-release-results.md) show about 44%
less CPU drawing time than geometry-only retention for stable non-wrapping labels,
without establishing an editor-wide CPU saving. The later
[actual-editor navigation benchmark](canvas-navigation-release-results.md) finds
a clear gain from narrower application notifications but mixed additional gains
from layout reuse. Enabling it by default is a product choice, not a claim of
universal CPU savings.

## Behavior

Each window can retain Taffy nodes between frames. Requests reconcile against
allocation slots in request order. Styles are compared after rem/DPI conversion,
child relationships are reconciled with Taffy's reparenting APIs, and removed
slots are pruned. A slot represents reusable layout calculations, never input,
focus, or component identity. Element construction, painting, and input dispatch
continue normally. Existing `Entity::cached` behavior is unchanged.

Opaque measurement callbacks always invalidate. They may prepare state needed
by the current frame's paint method, so reusing only their previous size would be
incorrect. Eligible non-wrapping text now prepares fresh glyph/paint state before
layout and supplies a pure intrinsic size; unchanged sizes can reuse layout.
Truncating, clamped, wrapping, and custom measurements remain conservative.
Callback captures are dropped at the frame boundary in both modes. This also closes a context-retention gap in Taffy 0.13's `clear` API.

Old parent links are detached when a reused allocation becomes an independent
layout root. Absolute bounds and origins are recalculated each frame. Auto-sized
window roots still follow the original stretch behavior; it can dirty their root
layout on every frame even when descendant caches remain reusable.

## Try it

Open **Settings > Experimental > Reuse interface layout**. The switch applies
immediately to every open window and saves the choice for future launches. It
defaults to on for new settings and older settings without this field; an explicit
saved off choice remains off. New windows inherit the current
session's choice; switching modes preserves documents and undo history.

For repeatable launch comparisons, the existing environment variable still works:

```powershell
$env:EMULSION_RETAINED_LAYOUT = '1'
cargo run -p emulsion-app
```

`EMULSION_RETAINED_LAYOUT=1` or `=0` overrides the saved choice at startup only.
The Settings switch can still change it immediately during that session. Remove
the variable to use the saved preference on the next launch; use `=0` to explicitly
start with cold layout. Settings displays a note while a launch override is set.
The explicit GPUI API is `window.set_layout_reuse_enabled(true)` between frames.
`window.layout_reuse_stats()` returns the previous completed frame's allocations,
reused slots, changed styles/child lists, measured requests, retained nodes,
intrinsic size requests/reuse, and actual opaque measurement-callback calls.
These counters report reconciliation activity, not actual Taffy cache hits.
Pruning releases retired node contents; internal storage capacity can remain at
its high-water mark. `retained_nodes` counts live nodes, not allocated bytes.

## Comparative benchmark

```powershell
cargo bench -p emulsion-ui --features layout-bench --bench layout_reuse
```

For a quicker local build using the optimized development profile:

```powershell
cargo bench -p emulsion-ui --features layout-bench --bench layout_reuse --profile dev -- --sample-size 20 --warm-up-time 1 --measurement-time 3 --noplot
```

Cold mode runs first by default, followed by `retained_geometry_only` (the first
prototype) and `retained` (including the intrinsic-text specialization). Repeat with reversed order to check for ordering
bias; the selected order is printed in the run metadata:

```powershell
$env:EMULSION_LAYOUT_BENCH_RETAINED_FIRST = '1'
cargo bench -p emulsion-ui --features layout-bench --bench layout_reuse
Remove-Item Env:EMULSION_LAYOUT_BENCH_RETAINED_FIRST
```

The opt-in feature enables GPUI's existing `BenchAppContext`/`BenchReport` and
Criterion. It is not enabled by normal application or UI test builds. The cases compare cold, prior geometry-only retention, and full retention across
fixed geometry, changing native text, structural churn, stable non-wrapping labels,
changing non-wrapping labels, and a cached sidebar beside an active canvas.
The row fixtures use 256 rows. The canvas fixture asserts that its cached sidebar
stops rendering during canvas updates. They include element construction,
layout and CPU paint preparation; they exclude GPU submission. Native platform
text shaping is used for all text workloads. The output includes Criterion estimates,
frame distributions, and final-frame layout counters.

Do not extrapolate these synthetic results into process CPU percentage or compare
development-profile results to release numbers. Root rendering is still executed.
Retaining a layout tree also consumes memory between frames. Adoption needs real
Emulsion interaction measurements and platform validation, especially for text-
heavy and rapidly changing interfaces.

## Next framework milestones

1. Use these differential tests and benchmark cases to evaluate stable domain-
   keyed layout identities versus allocation-slot matching.
2. Define an explicit measured-content revision contract before caching text or
   custom measurement callbacks; rendering side effects must be separated first.
3. Preserve more stable subtrees only with explicit layout/paint/transform dirty
   dependencies, inherited-state invalidation, and focus/input correctness.
4. Consider paint damage tracking separately after geometry reuse is validated.

The immediate implementation avoids changing existing paint-range cache checks
or treating relocated hitboxes as valid. Those are separate correctness problems.

## Initial local measurement

One run on Windows x86_64 used the optimized **development** profile
(`debug_assertions=true`), 256 rows, 20 Criterion samples, 1 second of warmup,
and 3 seconds of requested measurement per case. Values below are Criterion's
per-iteration point estimates with 95% confidence intervals, in milliseconds;
they are not frame-time percentiles.

| Workload | Cold estimate (95% CI), ms | Retained estimate (95% CI), ms |
| --- | --- | --- |
| Fixed geometry | 2.6112 (2.5906 to 2.6330) | 2.7184 (2.0203 to 3.4519) |
| Changing text | 5.6689 (5.2374 to 6.1724) | 6.6038 (5.8530 to 7.2583) |
| Structural churn | 4.6420 (4.0669 to 5.2569) | 2.6348 (2.3998 to 2.9769) |

Other Cargo/rustc compilations ran concurrently. The modes also ran sequentially,
cold before retained, so changing machine load can bias the comparison. These
numbers establish that the benchmark runs; they do not demonstrate a CPU saving
or support enabling the experiment by default. Repeat on an otherwise idle machine
in release mode, including reversed mode order, before drawing performance conclusions.

The final-frame counters confirm slot reuse: retained mode allocated zero nodes
and reused 1,282 fixed-geometry nodes, 770 text nodes, and 842 churn nodes. Cold
mode allocated those node counts again. The text case still invalidated all 256
measurement callbacks; churn changed 506 styles and 254 child lists. These are
reconciliation counts, not proof of layout-cache hits. Raw output is in
`target/layout-reuse-benchmark.log` (a local, untracked artifact).

## Repeat after compilation finished

The same compiled benchmark executable was repeated after the UI tests and Cargo
builds finished, with the same development-profile settings. No Cargo/rustc
processes were observed around this repeat. The machine was not otherwise isolated,
and mode order was still cold then retained.

| Workload | Cold estimate (95% CI), ms | Retained estimate (95% CI), ms |
| --- | --- | --- |
| Fixed geometry | 2.8515 (2.7352 to 2.9544) | 1.6053 (1.5670 to 1.6445) |
| Changing text | 2.3885 (2.3389 to 2.4322) | 2.3116 (2.2679 to 2.3577) |
| Structural churn | 1.4913 (1.4655 to 1.5209) | 1.7667 (1.6720 to 1.8918) |

The point estimates suggest about 44% lower CPU draw duration for fixed geometry,
little change for changing text, and about 18% higher duration for structural
churn. This supported the original opt-in recommendation: comparing and reconciling
unstable trees can cost more than rebuilding them. It also motivates explicit
measurement revisions before trying to reuse text layout. These are synthetic
CPU drawing durations, not application CPU percentages or release-build results.

The repeat log is `target/layout-reuse-benchmark-repeat.log`. Criterion's `change`
lines compare each case against its previous run, not cold against retained;
use the paired mode estimates above for that comparison. The subsequent release runs are linked above; real editor workload validation
remains necessary to establish broader performance benefits.

## Validation

- `cargo check -p emulsion-app --offline --message-format=short`: passed with
  normal application features (the benchmark feature is not required).
- `cargo test -p emulsion-ui --lib --features layout-bench layout_reuse_tests -- --nocapture`:
  all eleven differential tests passed. They cover topology and pointer targets,
  resize/DPI/rem/text changes, measurement side effects and captured-state lifetime,
  node pruning, runtime toggles, independent roots, and cached view transitions.
  The intrinsic-text cases additionally compare fresh glyphs/decorations, font
  size and DPI changes, and transitions between measurement kinds. The mock font
  system cannot distinguish bold/italic glyph IDs; native shaping is exercised
  separately by the release benchmark.
- The complete compiled UI test binary ran serially in both modes, with
  `EMULSION_RETAINED_LAYOUT=0` and `=1`: **445 passed in each mode**, one existing
  ignored test, and one excluded stale landing-image dimensions assertion
  (`tests::splash_dismisses_and_the_landing_image_opens_for_editing`). Logs are
  `target/intrinsic-layout-suite-cold.log` and `target/intrinsic-layout-suite-retained.log`.
- `cargo clippy -p emulsion-ui --lib --tests --benches --features layout-bench --no-deps --message-format=short`:
  passed; only the existing explicit-lifetime warning in
  `photoshop_shortcut_tests.rs` remains. Changed Rust files pass rustfmt checks.
- All ten standalone renderer/presentation policy tests passed with Git Bash.
- The strict vendor checker still reports the checkout's 29 CRLF license-hash
  mismatches. All 29 upstream license hashes match after LF normalization.
  License staging tests pass in an isolated LF-normalized fixture with explicit
  Git Bash; the unchanged scripts fail directly on this Windows checkout because
  of Bash resolution/CRLF paths. No license texts were changed.

The UI test build included `layout-bench` to share compiled GPUI dependencies with
measurement; the normal-feature application check above independently verifies
that the feature remains optional. These checks establish correctness on this
Windows test setup, not real-window CPU/GPU savings or cross-platform coverage.
