# Native Smart Filter enabled state

Smart layers have two independent controls:

- `NodeKind::Smart.filters_enabled` enables the entire stack
- `FilterStyle.enabled` enables one stage

Both default to true. Disabling a stack retains its mixed item flags, filter
parameters, opacity/blend settings, source and OriginalImage, and all masks.
An empty disabled stack is retained authored state. Adding a stage enables the
new stage without enabling its parent. Opacity zero remains an enabled stage.

## Rendering and geometry

`smart::render_stack` computes the authoritative unmasked cache. A disabled
stack, or a stack without enabled stages, aliases the source Arc at offset zero.
Disabled stages contribute neither pixels nor spread. Enabled zero-opacity
stages retain the existing spread behavior. Stage indices are not compacted:
Dissolve continues using each stage's original index in its deterministic seed.

The stack mask remains separate. Bypassed stacks return the source without
projecting dormant coverage. Masks retain their raw planes and source-local
matrices, including extents beyond the current cache. Ordinary raster/vector
masks, stack-mask visibility and layer visibility remain independent.

Every native recomputation uses the stack-aware helper, including RAW/source
replacement, component overrides, native loading and background UI renders.
Direct history/cache installation canonicalizes bypass results to source/zero
rather than restoring stale expanded geometry. Active historical caches are
restored without rerendering.

## Persistence

Native and history readers support version 15. Any false root or item flag
requires version 15, including hidden, empty, working-copy and history-only
states. Writers omit true flags and retain the prior minimum version for
all-enabled documents. Legacy missing flags and trailing styles default to true
and ordinary blending options. Extra orphan styles and malformed booleans fail.

Both read APIs preflight the live manifest and history before plane allocation.
Downgraded feature-bearing archives cannot recover by dropping history. A scoped
lexical fallback detects explicit root state and immediate filter-style enabled
fields in otherwise malformed legacy history. Generic mask/effect enabled
fields do not trigger that detection.

Nested native archives retain independent versions and opaque bytes. Their
metadata is checked when opened for source editing. Original PNG bytes and
source/encoded digest checks retain their existing contract.

## Authoring and asynchronous work

Properties panels expose root and item toggles; parameters and blending remain
editable while disabled. The layer-list Smart Filters header also exposes root
state. Pending controls display the complete requested stack, including the
latest throttled parameter value.

Accepted filter requests retire older RAW/source/MCP operation tickets without
clearing filter intent. Independent filter queues carry their shared operation
epoch forward when another filter publishes. A later generic edit job retires
older filter results. Source/page changes and modifying MCP calls refuse to
snapshot pending filter edits; RAW scheduling likewise waits for the user to
finish them. Native Save remains blocked until the pending state settles.

Filter publication retains generation, history, exact-node, lock and transaction
checks. Standalone toggles cannot preview inside an unrelated gesture. Escape
cancels pending filter work and cancels only a filter gesture's own transaction.

MCP exposes optional strict `enabled` booleans on `add_filter` and `set_filter`,
and required `enabled` on `set_filters_enabled`. Omission preserves existing
item state; new stages default true. `describe_document` reports both levels.
Heavy tools render from a snapshot and publish one atomic cache/state command.

This native capability does not implement PSD Smart Filter descriptors or
establish filter parity with other editors. Until an export adapter encodes these flags,
a disabled empty stack must not qualify for source-only editable export.
