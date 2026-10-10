# Library and Develop implementation ledger

This ledger tracks the requested desktop Library/Develop layout and the feature audit. A check records the bounded implementation described in the guide, with UI, shared renderer/persistence, MCP where applicable, and relevant validation. Remaining limits are tracked separately; a check does not imply every behavior of other photo editors is reproduced. It is not a claim of third-party rendering equivalence.

- [x] Module layout: central canvas, Navigator/presets/history left, histogram/tools/adjustments right, full-width filmstrip, hideable panels.
- [x] Bounded reusable RAW processing cache, cancellation, fit and detail previews.
- [x] Rendering process version with legacy preservation and explicit upgrade.
- [x] Parametric curves, primary calibration, global grading/blend/balance, incremental WB.
- [x] Numeric controls, reset/nudge, gesture undo.
- [x] Camera-profile import, compatibility and portable identity.
- [x] Chroma/luminance detail controls and full-resolution inspection.
- [x] Composed named local masks and overlays.
- [x] Nondestructive healing and cloning.
- [x] Direct crop/straighten/guided geometry.
- [x] Culling shortcuts/advance, compare/survey, metadata undo.
- [x] Catalog keywords/labels, paged import, offline records and root remapping.
- [x] Read-only sidecar fallback and offline edit proxies.
- [x] Display/proof color management and wider-gamut working/output path.
- [x] Measured indexed photo catalog and disk preview management.
- [x] Quality, memory and interaction benchmarks; separate packaged build.

Original files and the running application must remain untouched. Separate substantial research projects (sensor highlight reconstruction, AI sensor denoise, HDR/panorama/depth blur) remain explicitly outside a claim of completed parity.

Validation and remaining limits: [desktop validation report](../reports/library-develop-desktop-validation.md).
