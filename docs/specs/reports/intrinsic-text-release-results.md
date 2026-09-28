# Intrinsic text release measurements - 2026-09-23

*Snapshot from 2026-09-23. For current behavior see [layout-reuse-experiment.md](layout-reuse-experiment.md).*

The new text specialization reduces CPU drawing duration for stable, non-wrapping
labels by **43.7-44.0% compared with the previous geometry-only retention**, in
both benchmark orders. Changing non-wrapping text improves by **3.3-5.5%**.
These are synthetic release results, not an editor-wide CPU reduction.
Measured on 2026-09-23 comparing cold and retained layout in the same binary;
the setting is on by default, see [layout-reuse-experiment.md](layout-reuse-experiment.md).

## What changed

Eligible text prepares fresh glyphs and decorations each frame, then gives Taffy
its numeric intrinsic size. Equal snapped sizes preserve cached layout. Only
non-wrapping text without overflow/truncation or line clamps qualifies. Wrapping
text and arbitrary measurement callbacks keep their existing behavior.

Two additional fixes apply in normal rendering: text decoration vectors reserve
space based on the actual run count, and the Layers footer embeds its trash icon
instead of repeatedly requesting an asset absent from the default bundle. Their
individual performance effect was not measured: all three comparison modes use
these fixes.

## Setup

- Windows 10.0.26200 x86_64; AMD Ryzen 7 8700G; 16 logical processors.
- Rust 1.98.1; release profile, thin LTO, one codegen unit.
- Same executable in both orders: cold, geometry-only retention, full retention;
  then the reverse. Six workloads, three modes, 36 measured cases overall.
- Each case: 50 Criterion samples, 2 seconds warmup, 5 seconds requested measurement.
- Native text shaping; 256 rows in row workloads. Includes element construction,
  layout, and CPU paint preparation; excludes GPU submission.
- One-second process sampling detected no Cargo, rustc, Emulsion, or UI-test
  processes during the completed repeat. The desktop was not otherwise isolated.
  An earlier interrupted attempt overlapped compilation and is excluded.
- This was a dirty checkout. Binary SHA-256, source revision, exact estimates,
  and 95% confidence intervals are preserved in the
  [machine-readable results](intrinsic-text-release-results.json).

## Results

Times are Criterion per-iteration point estimates in milliseconds. Negative
changes mean less time. Geometry-only is the previous retained-layout prototype;
full retention includes the intrinsic-text specialization. These percentages
must not be added to the earlier fixed-geometry improvement: they concern
different workloads and different baselines.

| Order | Workload | Cold ms | Geometry-only ms | Full retention ms | New vs geometry-only | New vs cold |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| cold-first | fixed_geometry | 2.3507 | 0.9085 | 0.9135 | +0.6% | -61.1% |
| cold-first | changing_text | 1.5895 | 1.5542 | 1.5551 | +0.1% | -2.2% |
| cold-first | structural_churn | 0.9649 | 0.9453 | 0.9499 | +0.5% | -1.6% |
| cold-first | stable_text | 1.1230 | 1.1009 | 0.6166 | -44.0% | -45.1% |
| cold-first | changing_nowrap_text | 1.4263 | 1.3824 | 1.3057 | -5.5% | -8.5% |
| cold-first | cached_sidebar_canvas | 0.0455 | 0.0429 | 0.0431 | +0.5% | -5.3% |
| retained-first | fixed_geometry | 1.8810 | 0.9089 | 0.9050 | -0.4% | -51.9% |
| retained-first | changing_text | 1.5670 | 1.5500 | 1.5507 | +0.0% | -1.0% |
| retained-first | structural_churn | 0.9638 | 0.9361 | 0.9371 | +0.1% | -2.8% |
| retained-first | stable_text | 1.1321 | 1.1143 | 0.6272 | -43.7% | -44.6% |
| retained-first | changing_nowrap_text | 1.4159 | 1.4048 | 1.3591 | -3.3% | -4.0% |
| retained-first | cached_sidebar_canvas | 0.0451 | 0.0439 | 0.0440 | +0.1% | -2.5% |

The cold fixed-geometry case varied noticeably between orders (2.35 vs 1.88 ms;
first-run 95% interval 2.17-2.62 ms). Do not treat its larger first-run percentage
as an additional optimization. Geometry-only and full-retention timings were
nearly identical in that workload, as expected for a text-specific change.

For stable labels, the last full-retention frame reused all 256 intrinsic sizes.
The old path invoked opaque measurement callbacks 2,048 times; the new path invoked
none. Text is still shaped/refreshed through GPUI's text system each frame, and
Taffy can still dispatch numeric intrinsic measurements. These counters do not
mean zero text work or directly count layout-cache hits.

Changing non-wrapping labels reused zero intrinsic sizes in the last frame, but
still avoided repeated opaque callbacks. Wrapping text sees essentially no
additional benefit. The canvas fixture asserts zero sidebar renders during
canvas updates in every mode: existing entity caching already avoids most work
there, so the text specialization adds little to that workload.

## Validation

- All 11 differential layout tests passed, including glyph/decoration freshness,
  font-size and DPI changes, callback lifetimes, measurement-kind transitions,
  topology, and pointer dispatch. Mock fonts do not distinguish bold/italic
  glyph IDs; native shaping is exercised by the benchmark.
- Full UI suite, serial, both `EMULSION_RETAINED_LAYOUT=0` and `=1`: **445 passed
  in each mode**, one existing ignored test, one excluded stale landing-image
  dimensions assertion (`tests::splash_dismisses_and_the_landing_image_opens_for_editing`).
- Normal-feature application check, UI clippy with tests/benches, targeted rustfmt,
  and diff whitespace checks passed. Clippy reports the existing explicit-lifetime
  warning in `photoshop_shortcut_tests.rs`; the release build reports the existing
  unused import in the Windows GPUI backend.
- All 18 release benchmark smoke cases passed before timing.
- Local logs: `target/intrinsic-layout-{tests,app-check,clippy,bench-smoke}.log`,
  `target/intrinsic-layout-suite-{cold,retained}.log`, and
  `target/performance/intrinsic-{cold-first,retained-first}-20260923.log`.

## Repeat or try in the editor

```powershell
cargo bench -p emulsion-ui --features layout-bench --bench layout_reuse -- --sample-size 50 --warm-up-time 2 --measurement-time 5 --noplot --save-baseline intrinsic-cold-first
$env:EMULSION_LAYOUT_BENCH_RETAINED_FIRST = '1'
cargo bench -p emulsion-ui --features layout-bench --bench layout_reuse -- --sample-size 50 --warm-up-time 2 --measurement-time 5 --noplot --save-baseline intrinsic-retained-first
Remove-Item Env:EMULSION_LAYOUT_BENCH_RETAINED_FIRST

$env:EMULSION_RETAINED_LAYOUT = '1'
cargo run --release -p emulsion-app
```

You can switch immediately in **Settings > Experimental > Reuse interface layout**.
Remove `Env:EMULSION_RETAINED_LAYOUT` to use your saved preference on the next launch.
Run timing measurements with other builds/tests stopped. The local measurements
above invoked the compiled release benchmark directly after compilation and
validation, avoiding build overlap.

No real-window CPU/GPU or memory measurement was performed for this extension.
The [earlier manual panning trial](layout-reuse-release-results.md) did not show
a process CPU benefit for geometry-only reuse. Repeat controlled editor workloads
before changing the default. Eager text preparation can also do work for text
that later becomes hidden; retained numeric contexts and layout nodes consume
memory between frames. This change does not retain element objects or implement
paint damage tracking. See the [experiment design](layout-reuse-experiment.md).
