# Brush files and conversion limits

Emulsion stores its catalog in `brush-library.json` in its application data directory. Existing `brush-presets.json` files are read without truncating rows and are left untouched after migration. Brush identities do not depend on their names or positions.

| Format | Supported content | Limits |
| --- | --- | --- |
| `.embrushes` | Selected brushes, ordered sets/libraries (including empty sets), current settings, original baselines, reset points, metadata, dual components and PNG source assets | Brush exports import into the selected set; set exports create a set in the selected library; library exports create an independent library. This is Emulsion's portable format, not a Procreate export. |
| `.brush`, `.brushset` | Scalar settings with Emulsion equivalents, shape and grain PNGs | Conversion is approximate. Unsupported property names are reported; external source archives are preserved. Imported brush sets become named sets. |
| `.brushlibrary` | ZIP layouts containing set/brush folders with `Brush.archive`, or nested `.brush` / `.brushset` archives | Creates a library and recovers set names from folder/archive labels. Proprietary ordering metadata is not interpreted. Alternate layouts fail explicitly. Only synthetic structural fixtures have been tested; no claim of complete Procreate 5.4 archive compatibility. |
| `.abr` | 8-bit sampled tips in versions 1, 2, 6.1 and 6.2; uncompressed or PackBits scanlines; version 2 UTF-16 names | Photoshop dynamics/descriptors and procedural brush records are not converted. Other versions, depths and compression modes fail explicitly. Tested against synthetic binary fixtures; real brush packs require additional compatibility validation. |

Procreate's handbook documents the library/set/brush hierarchy and accepted file extensions, but does not specify its binary archive layout. [Brush libraries](https://help.procreate.com/procreate/handbook/brushes/brush-library)

The ABR field layout was cross-checked against the published Krita reader; Emulsion's parser uses bounded slices and rejects invalid dimensions and scanlines. [Krita ABR reader](https://github.com/KDE/krita/blob/master/libs/brush/kis_abr_brush_collection.cpp)

Imported source PNGs use SHA-256 filenames under `brushes/assets`. Runtime texture IDs are resolved separately, including legacy IDs. Original external archives are retained under `brushes/sources`, with their content hash in brush provenance. Missing textures are reported on load and their references are retained for recovery. Portable export fails if a required source is missing, including sources used only by original or reset-point snapshots.

Catalog writes take an OS file lock, compare the expected revision and replace a synced temporary file. A failed write leaves the caller's live catalog unchanged. A stale writer receives a conflict instead of overwriting a newer catalog. Corrupt catalog or migration input is reported and cannot be silently replaced by a subsequent save.
