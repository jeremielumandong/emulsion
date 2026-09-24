# Canvas navigation release measurements - 2026-09-23

Targeted navigation notifications reduce CPU-side pan-update duration by
**41-42% in the roomy editor and 22-28% in compact** across both release benchmark
orders and layout modes. This application optimization is enabled normally.
Layout reuse remains opt-in: its additional effect on this workload was mixed,
including a 5.9% slower compact comparison in the reverse-order run.

## Change

Continuous hand-tool panning, wheel pan/zoom, canvas rotation, and pinch redraw
the canvas without notifying the editor owner. The cached Layers/Properties
sidebar remains reusable. Visible Info and Navigator panels still redraw because
their displayed values depend on the viewport. Collapsed panels stay cached.
Uncached ancestors refresh the displayed zoom and rotation controls.

Document changes retain their existing broader invalidation. This optimization
works with both cold layout and the retained-layout experiment.

## Measurement

- Windows 10.0.26200 x86_64; Ryzen 7 8700G; 16 logical processors; Rust 1.98.1.
- Release profile, thin LTO, one codegen unit. Same executable for both orders.
- Actual `EditorView` inside GPUI Component `Root`, with the real Layers sidebar;
  no outer Workspace shell. A 256x192 document contains 64 raster layers sharing
  pixel data. The warmed view pans two pixels and back through the actual drag
  handler. Both chrome layouts and both layout modes are measured.
- Native text and the GPUI Kit bundled asset source participate. The benchmark
  measures CPU-side input/update/render work and foreground work it schedules,
  including layout and paint preparation. It excludes GPU presentation, OS input
  delivery, and cold or large-document rasterization.
- Each case: 50 Criterion samples, 2 seconds warmup, 5 seconds requested
  measurement. The second run reverses both notification and layout mode order.
- App data is isolated before startup. In-memory settings disable external
  services, tablet hooks, autosave, and selection timers only in this fixture.
- Untimed setup warms both pan endpoints until two consecutive round trips
  leave the sidebar cached. Setup fails if it cannot converge within eight trips.
  This drains initial compact toolbar-measurement frames before timing. The
  measured assertion still requires zero sidebar renders for targeted pan.
- One-second monitoring detected no Cargo, rustc, Emulsion, or UI-test processes
  during either completed run. Other desktop activity and clock frequency were
  not controlled.

The comparison mode runs the same current drag handler and then adds the former
editor-owner notification. It reproduces broad sidebar invalidation in the same
binary, but includes an extra canvas notification compared with the historical
implementation. Thus this is a controlled comparison of notification scopes,
not a before/after executable comparison or process CPU-percentage measurement.

Binary hash, source revision, exact values, 95% confidence intervals, and monitoring
metadata are preserved in [the JSON results](canvas-navigation-release-results.json).
This was a dirty development checkout.

## Targeted notifications

Criterion per-update point estimates in milliseconds; negative changes are faster.

| Order | Chrome | Layout | Owner notification ms | Targeted ms | Change |
| --- | --- | --- | ---: | ---: | ---: |
| legacy-first | roomy | cold | 3.0660 | 1.7777 | -42.0% |
| legacy-first | roomy | retained | 3.0031 | 1.7707 | -41.0% |
| legacy-first | compact | cold | 2.4116 | 1.7269 | -28.4% |
| legacy-first | compact | retained | 2.3265 | 1.7170 | -26.2% |
| targeted-first | roomy | cold | 3.0659 | 1.7888 | -41.7% |
| targeted-first | roomy | retained | 2.9480 | 1.7149 | -41.8% |
| targeted-first | compact | cold | 2.2103 | 1.5988 | -27.7% |
| targeted-first | compact | retained | 2.1726 | 1.6935 | -22.1% |

Every targeted measured update redrew the canvas without rebuilding the sidebar.
The owner-notification mode rebuilt the sidebar on every update. Final batches
reported 150 pans / 150 canvas renders / 0 sidebar renders for targeted mode,
and 100 / 100 / 100 for owner mode. Different iteration counts are Criterion's
calibration choice; timings above are per update.

## Should layout reuse become the default?

The navigation fix has a repeatable benefit. The extra benefit of layout reuse
after that fix is not consistent:

| Order | Chrome | Retained vs cold, targeted navigation |
| --- | --- | ---: |
| legacy-first | roomy | -0.4% |
| legacy-first | compact | -0.6% |
| targeted-first | roomy | -4.1% |
| targeted-first | compact | +5.9% |

The compact cold estimate also varied between orders (1.7269 vs 1.5988 ms), so
this is not proof of a universal retained-layout regression. It is evidence
against treating the earlier synthetic 44% text improvement as an editor-wide
saving. Keep layout reuse opt-in while trying representative documents and real
windows. These measurements do not assess memory growth, GPU time, or other
platforms. The current application and framework defaults remain cold layout.

You can now toggle it live under **Settings > Experimental > Reuse interface
layout**. The default is off; the choice is saved. To force a startup mode for a
release comparison:

```powershell
$env:EMULSION_RETAINED_LAYOUT = '1'
cargo run --release -p emulsion-app
```

Set the variable to `0` for cold layout at startup; remove it to use the saved preference. The navigation fix applies
in either mode after rebuilding.

## Validation and reproduction

- All 12 canvas-invalidation tests passed, including five new event regressions.
  They exercise wheel pan/zoom, pinch, drag pan/rotation, current zoom/rotation
  labels, stationary-pointer Info updates, actual Navigator painted geometry,
  and manual/automatic sidebar collapse in both layouts where applicable.
- Full UI suite passed **450 tests in each layout mode**, one existing ignored
  test, one excluded stale landing-image dimensions assertion:
  `tests::splash_dismisses_and_the_landing_image_opens_for_editing`.
- Normal-feature application check and UI clippy (library, tests, benchmarks)
  passed. Existing explicit-lifetime and Windows GPUI unused-import warnings
  remain. Changed small Rust modules pass rustfmt; scoped diff whitespace checks
  pass. Existing unrelated edits in `editor.rs` were preserved.
- All eight release benchmark smoke cases and both full timing runs passed.

```powershell
cargo bench -p emulsion-ui --features layout-bench --bench editor_navigation -- --sample-size 50 --warm-up-time 2 --measurement-time 5 --noplot --save-baseline navigation-legacy-first
$env:EMULSION_NAVIGATION_BENCH_TARGETED_FIRST = '1'
$env:EMULSION_LAYOUT_BENCH_RETAINED_FIRST = '1'
cargo bench -p emulsion-ui --features layout-bench --bench editor_navigation -- --sample-size 50 --warm-up-time 2 --measurement-time 5 --noplot --save-baseline navigation-targeted-first
Remove-Item Env:EMULSION_NAVIGATION_BENCH_TARGETED_FIRST
Remove-Item Env:EMULSION_LAYOUT_BENCH_RETAINED_FIRST
```

The recorded runs invoked the compiled executable after all builds/tests ended.
Raw logs are `target/performance/navigation-{legacy-first,targeted-first}-20260923.log`.
Validation logs are `target/navigation-{invalidation-tests,app-check,clippy,bench-smoke}.log`
and `target/navigation-suite-{cold,retained}.log`. The earlier failed smoke attempts
were fixture setup/cleanup checks and supplied no timing results to this report.
