# Native Invert Smart Filter

`Filter::Invert` is a parameter-free editable filter, serialized as
`{"kind":"invert"}` and available as `invert` through the filter catalogue and
MCP. The Filter menu exposes it under **Other**; the Smart layer Properties
catalogue and Repeat Last Filter use the same implementation.

The reference CPU kernel unpremultiplies RGB, converts linear channels to encoded
sRGB, complements each channel (`1 - c`), converts back to linear, and
premultiplies again. It preserves alpha, dimensions and source-relative origin.
Zero-alpha pixels remain unchanged. No intermediate 8-bit conversion is used.
The native raster still has its existing linear-premultiplied 16-bit precision;
this does not promise lossless round trips for arbitrary straight-sRGB inputs.

The existing Smart filter lifecycle owns source retention, raw cache rendering,
per-stage blending, stack-wide masks, source replacement, and Undo/Redo. Invert
adds no separate source or mask representation. GPU dispatch explicitly declines
Invert, including in explicit GPU mode, so the CPU reference is used.

## Persistence boundary

This unpublished native/history v14 batch includes Invert. Presence in any live,
working, or historical snapshot requires v14, including hidden layers and zero
filter opacity. Older ordinary documents retain their existing version; the
existing `Adjustment::Invert` does not trigger this gate. Both native read APIs
preflight filter metadata before raster allocation and reject downgraded Invert
records or unknown v14 filters instead of discarding the history.

This is a native prerequisite. It does not implement PSD `filterFX` or `FEid`
interchange, or establish editable Photoshop Smart Filter/mask compatibility.

## Focused verification

Run the normal repository checks, plus these focused filters in the shared
validation environment (UI tests serially):

- `cargo test --locked -p emulsion-filters invert`
- `cargo test --locked -p emulsion-core invert`
- `cargo test --locked -p emulsion-io --test native_invert_filters`
- `cargo test --locked -p emulsion-io --test native_photoshop_profile`
- `cargo test --locked -p emulsion-io native_features`
- `cargo test --locked -p emulsion-mcp invert`
- `cargo test --locked -p emulsion-ui invert -- --test-threads=1`
- `cargo test --locked -p emulsion-gpu invert_explicitly_rejects_gpu_dispatch`

The GPU dispatch test needs an actual test adapter; its no-adapter early return
is not hardware validation. CPU tests include an independent f64 transfer
reference, transparent and partial-alpha pixels, native 16-bit edge values,
repeated inversion, and order-sensitive filter stacks. Lifecycle and persistence
tests cover source edits, mask coverage, cache regeneration, Undo/Redo, reopen,
working/history-only features, downgrade attempts and unknown filter records.
