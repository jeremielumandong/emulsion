# Retained original PNG Smart sources (native version 13)

A raster-backed Smart layer may retain `original_image: Option<Arc<OriginalImage>>`
beside its decoded `source`. Rendering, filters, placement, and masks continue to
use the native premultiplied linear RGBA16 source; rendering behavior is unchanged.
`SmartEditable` is unchanged. An original PNG and a layered/text/path/SVG editable
source cannot coexist on one Smart layer.

## Why encoded bytes are authoritative

Native premultiplication cannot reproduce every original straight-alpha RGBA8
sample. In particular, alpha-zero RGB and some samples with alpha 1–5 can differ
after decode and re-encode even when the native image is visually unchanged.
The admitted original PNG bytes therefore remain immutable and are never rebuilt
from a preview or from `source.to_srgba8()`.

An original stores two SHA-256 digests:

- The digest of its exact encoded bytes.
- The digest of its native decoded source: ASCII domain marker
  `Emulsion OriginalImage native-source v1` followed by NUL, big-endian u32 width
  and height, then in-bounds row-major premultiplied RGBA16 channel words in
  big-endian order. Sparse layout, tile padding, and allocation identity are
  excluded.

IO workers validate the encoded digest, strictly decode the PNG, and bind the
native digest to the current source before a native save or editable PSD export.
A stale or forged original fails native save. PSD export takes the existing
explicit whole-appearance fallback, with no embedded original bytes or placed
source records in that fallback.

## Native storage and compatibility

The native manifest's Smart kind gains an optional `original_image` descriptor:
`encoded_sha256`, `source_sha256`, `width`, and `height`. Its live `src` points
straight to `original-images/<encoded_sha256>.png`. This avoids the ordinary
native live-source PNG re-encode entirely. Other ORA applications still receive
the usual independent rendered preview.

History gains the same descriptor while retaining its exact existing RGBA16
source/cache tile references. Original resources are shared between the live
stack, working snapshot, and all history commits. History restoration validates
the original against its source tiles. Encoded-byte addressing deduplicates
identical resources; different hidden RGB values remain distinct even when their
native source digests are identical. Reopened references share one original Arc,
and history memory accounting counts each encoded allocation once.

Native `FORMAT_VERSION` and `HISTORY_VERSION` are 13. A file requires 13 if an
original is present in its live document or any saved snapshot, including an
original that exists only in history. Otherwise existing feature-dependent
versions 9, 10, 11, and 12 remain in use. Missing fields in older documents default
to no original. Falsely lower version declarations and future versions are
rejected. Native page archives carry their own feature versions. The outer
`.emu` package remains version 1 unless a retired panel's retained history needs
the [project-format-2 preservation contract](native-project-envelope.md).
Invalid original-bearing history is not silently discarded as recoverable
history damage.

## Bounded PNG admission

The shared IO-only codec admits noninterlaced PNG RGB8/RGBA8. The allowed ancillary
chunks are the validated sRGB/gAMA/cHRM subset and bounded-format pHYs metadata.
Unsupported profiles, APNG, unknown chunks, invalid CRCs/order, excess IDAT chunks,
trailing streams, missing zlib end markers, invalid Adler checksums, and scanline
byte-count mismatches fail admission. These are source-preservation constraints,
not support for every valid PNG encoding.

Before any original-source PNG decode, native reads collect references across
live/history and bound ZIP resource sizes and declared padded native allocations.
The encoded content digest and actual PNG header dimensions must match the
reference. Limits per native page/archive are:

- At most 1,024 distinct original resources.
- At most 64 MiB encoded per resource and 128 MiB encoded in aggregate.
- At most 16 million source pixels each and 32 million in aggregate.
- At most 128 MiB padded native tiles each and 256 MiB in aggregate.
- Dimensions 1–30,000 on each axis.

Canonical source validation also has a separate 32-million-pixel/256-MiB padded
work budget for distinct source allocations/serialized source planes. Repeated
history references to the same immutable source are hashed once; the cache keeps
the Arc alive so allocation addresses cannot be reused. Equivalent pixels in
separately allocated planes still consume this work budget even when one encoded
PNG resource is shared. History plane dimensions and this work budget are checked
before loading its tiles.

The existing project-container and nested-source archive limits still apply.

## Lifecycle and PSD scope

Transforms, crop, duplicate, layer masks, vector masks, filter stacks, and filter
masks retain an unchanged source's original. Explicit source replacement clears
it even if replacement pixels are equivalent. RAW development and derived RAW
export source replacement clear it. Component content overrides copy source,
editable descriptor, and original together; equality and source-edit-session
checks include original identity. Undo restores the shared original; Redo repeats
its invalidation.

Opening and applying a truly unchanged raster-backed source (including transient
selection-only changes) keeps the original and adds no edit. Source tabs capture
their own baseline, so renaming the parent layer or changing its fonts while the
tab is open does not create a false source edit; an actual child rename still
counts even when it happens to match the parent's new name. A real source edit
creates the existing layered native source archive and drops old PNG provenance.
This does not add editable PSD export of nested native documents or flatten a
nested document and claim that its editable source survived.

The PSD source-only adapter captures the exact admitted PNG bytes and re-emits
them after validation. Sources without retained originals still require exact
native → RGBA8 → native equality. Empty filter-stack cache equality remains exact.
Repeated Photoshop instance references to one source UUID remain unsupported:
native source editing is node-local. Independent native duplicate nodes export
distinct source UUIDs even if their retained byte resources deduplicate natively.

Tests include hidden/low-alpha samples, exact PNG bytes through PSD → native → PSD,
the independent MIT `smartobject-layer.psd` fixture, history-free/history/`.emu`
persistence, lifecycle/Undo, no-op Apply, resource limits, false versions, digest
and source-tile mismatches, resource deduplication, and absence of source payloads
from PSD fallback. These tests do not claim full editable Smart Filter interchange
or a current Photoshop application acceptance session.
