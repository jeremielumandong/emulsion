# Live layout setting comparison - 2026-09-23

The same running release editor (PID 32040) was sampled while the user
panned the canvas manually for about 30 seconds with layout reuse off, then on.
The saved setting was checked before and after both samples. The switch applies
live; the process was not restarted. The installed executable matched the workspace
release binary (SHA-256 `52d81e44cd87c4514e2a279943fb65e028ebc7b47cfbe7349e2cdaec0a596c16`).

| Setting | Mean CPU | Peak sampled CPU | Final private MiB | Final working set MiB |
| --- | --- | --- | --- | --- |
| Off | 5.86% | 6.54% | 1188.5 | 923.6 |
| On | 5.82% | 6.57% | 1188.4 | 923.5 |

Mean CPU changed by -0.044 percentage points (-0.75% relative).
This difference is too small to establish an improvement in a single manual pair.
Memory was essentially unchanged. CPU is process CPU time normalized across all
16 logical processors; peaks are approximately one-second samples, not instantaneous
peaks. GPU memory, frame counts/times, exact input rate, and viewport matching were
not measured. These are observations of the current document/session, not a
controlled benchmark or evidence for changing the application-wide default.

The user's saved choice remains on. No source default was changed.
Raw samples and summaries are in `target/performance/`:

- `20260923-213629-526-live-toggle-off-pan-1.csv` and `.json`
- `20260923-213746-266-live-toggle-on-pan-1.csv` and `.json`

Full numbers: [JSON results](live-layout-toggle-results.json).
Sampler: `scripts/measure-editor-process.ps1`.
