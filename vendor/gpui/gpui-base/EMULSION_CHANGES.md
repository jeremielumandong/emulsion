# Emulsion patches to gpui-base 0.6.4

## Toolbar behavior backport — 2026-09-28

`src/toolbar.rs` is copied from the Apache-2.0 gpui-base 0.7.0 release,
with an added provenance comment. `src/lib.rs` exports Toolbar and ToolbarGroup.
Focus wrapping is adjusted to walk back through the toolbar's own tab stops
instead of traversing the entire window: Home can have more than 100 unrelated
card controls, exceeding the upstream traversal bound. The bounded walk still
skips disabled controls and preserves single-item behavior.
The source archive was downloaded from crates.io and verified against its
published SHA-256:

`64bb52b29a8dcdc8d3f595b8b0839c076f237572ab601ba4577d3da8ae085728`

Source: https://github.com/longbridge/gpui-kit/tree/v0.7.0/crates/base/src/toolbar.rs

This brings semantic groups and bounded Left/Right focus traversal to application
command rows while retaining Emulsion's patched GPUI 0.3.5 renderer. It is not a
full GPUI Kit 0.7.0 upgrade. The original package license remains applicable.
