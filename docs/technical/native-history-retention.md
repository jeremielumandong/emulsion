# Native history retention and project recovery

Native format and history support remain at version 15. Projective metadata is
rejected with a typed unsupported-feature error; this change does not introduce
version 16 support.

The [conditional project-format-2 envelope](native-project-envelope.md) prevents
older project readers from silently discarding protected retired graphs. Files
without such retired content continue using outer version 1, independently of
the existing-field retired drawing-aid repair.

## Project API contract

`project::read` and `read_from` still return `Result<Project>`. They now fail with
`IoError::ProjectRecoveryRequired { report }` when opening would require omitting
legacy retired content or a missing retired archive. No partial project is
returned and audio/video extraction has not started at that point.

Callers that can visibly deliver recovery warnings use `read_with_report` or
`read_from_with_report`, returning `OpenedProject { project, report }`.
`ProjectReadReport::warnings()` returns display text. Typed diagnostics identify
an affected panel, archive entry, and board-version IDs, and distinguish missing
archives, omitted known-legacy archives, and unrepresented legacy live/working
snapshots. Only ZIP `FileNotFound` is absence. Present container failures,
unsafe/duplicate entries and aggregate budget failures are fatal.

`IoError::ProjectEntry` retains its original boxed error as `source`, so callers
can inspect a `TooNew` cause without parsing English text. New error variants
are a deliberate source-compatibility change for exhaustive external matches.

## Preservation checks

Before ordinary enum decoding, a bounded metadata visitor checks both envelope
identities/versions and guarded schema positions. It rejects Smart-only fields
on recognized non-Smart kinds, conflicting editable/original sources, and
projective mapping members. Explicit default/empty/null guarded fields cannot
silently disappear through Serde's ignored-field behavior. It skips unrelated
geometry and plane tables without constructing a second generic JSON tree.
Damaged JSON has a positive-only linear lexical protected-marker fallback. A
failed probe or malformed critical snapshot/node/kind shape is never affirmative
evidence of an old supported format. The lexical scanner retains only nesting
positions, and source regressions count scan work rather than relax timeouts.

Native/history version 13+, OriginalImage, PSD-compatible compositing/Background,
Invert and explicit enabled flags, and opaque editable source descriptors
require preservation. Present unknown/unclassifiable retired archives cannot be
discarded. Source resources remain unopened; even a future nested native source
inside an outer version-9 document can be retained byte-for-byte. Reserved
source/original entries prevent failed archives from entering legacy recovery
when their references are ambiguous.

The writer uses an exhaustive historical-persistence relation when deciding
whether to emit working state. Changes confined to allocator state, source
depth, image information, or active Smart caches/offsets can no longer disappear
because ordinary Document equality omits them. Live aids and caches that the
history schema deliberately derives/normalizes are treated separately.

A manifest fingerprint is only a candidate hint. Before substituting a working
or head-tip document, the reader compares shared authored metadata and canonical
raw RGBA8/16 source PNG samples against the historical candidate's actual writer
projection. Comparison occurs before PNG import can erase hidden RGB or round
low-alpha samples. Compression and ancillary chunks do not define identity.
Noncanonical/interlaced PNGs cannot verify this relation. OriginalImage digests,
encoded bytes and bound logical source samples compare exactly; opaque source
archives and links compare by content without opening them.

A standalone file can retain its decoded live document when a valid historical
candidate does not match. If an independent protected working record would be
lost instead, opening fails. Retired storage retains only a graph, so its live
state must match the retained head-tip writer representation and every working
record must exactly match that tip's normalized historical content. The exact
relation includes selection, ID allocation, source/mask fill and dimensions,
logical samples, Smart caches and offsets. Sparse planes compare equal fill plus
the union of populated tiles, clipped to intrinsic bounds. Verified shared tile
pairs are memoized; constant huge planes do not trigger full-area sample walks. It does not invent commits or alter
branches or IDs. Intentionally missing individual board-version commits after
branch deletion remain separate from missing whole retired archives.

Colors and drawing guides are nonhistorical live aids. Standalone substitution
preserves them. After retired live/source and working-state admission succeeds,
valid live aids are moved to one authoritative carrier on the retained graph's
head tip. Retrieving another retired snapshot overlays those current aids onto
its artwork without inventing a commit or changing graph identity. Retirement
captures the current editor's aids, including explicit clearing. Version imports
keep existing recipient aids; only a new retired carrier receives donor aids,
and imported off-head artwork does not duplicate that payload. Undo, redo,
restore and checkout retain the current aids. General document replacement and
transaction rollback keep their existing behavior.

Raw aid values are checked before legacy truncation or sanitization. Invalid
protected aids fail explicitly; permitted legacy sanitization produces a recovery
diagnostic. Writer structural checks run before output or atomic-write staging.
Later encoding, size or I/O failures can leave a partial private streaming buffer,
but path-based atomic replacement preserves the existing destination. Valid
nondefault aids no longer cause the interim blanket save refusal. These repairs
use existing fields; they neither create historical aid chronology nor make old
readers preserve aids they previously discarded.

## Conservative compatibility limits

- Primary report-aware UI/MCP routes can recover known legacy damage and missing
  retired entries with a visible warning. Strict secondary routes, including
  template packs, reject those same files until they gain a report contract.
- Present unknown-kind or unclassifiable retired entries and failed source-bearing
  old archives no longer qualify for silent discard. Standalone old histories
  with an unknown kind and no protected evidence retain their recovery path.
- Head-only retired admission may reject a working state represented by another
  historical commit. It does not claim that state is absent from every commit.
- Ordinary PNG encoding is many-to-one. A matching writer representation permits
  exact history restoration but cannot prove unique provenance or authenticity.
- The relation authenticates shared authored persistence, not whether derived
  Smart/vector caches were rendered correctly. Existing cache validation remains.
- Sparse fill is exact in history-to-history comparison. Live source PNGs,
  including OriginalImage bindings, encode logical samples rather than a sparse
  storage fill choice. Masks retain their explicit native fill descriptors.
- Arbitrary falsely-old unknown fields outside guarded schema positions,
  unreferenced archive files, and wholly removed descriptors are not guaranteed
  preserved. This is not general unknown-field preservation.

## Regression coverage

`native_admission` covers envelope ambiguity, guarded field positions, duplicate
and escaped keys, source conflicts, protected lexical fallback, and projective
mapping rejection in live/commit/working positions. `native_relation` covers raw
8/16-bit rows, low alpha, hidden RGB, noncanonical encodings, source allocation
independence, masks, protected resource bytes, saved caches, media and geometry.
`native_retention_tests` covers full/document-only opens, forged fingerprints,
working retention, strict/report file and reader parity, typed nested causes,
legacy recovery, retired aids and strict embedded template packages.

The implementation was prepared source-only. The coordinator must run the
following before claiming runtime validation:

```
cargo fmt --all --check
cargo test --locked -p emulsion-io native_admission -- --test-threads=1
cargo test --locked -p emulsion-io native_relation -- --test-threads=1
cargo test --locked -p emulsion-io native_retention_tests -- --test-threads=1
cargo test --locked -p emulsion-io -- --test-threads=1
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked -- --test-threads=1
```
