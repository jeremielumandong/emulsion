# Conditional project format 2

Projects with protected artwork or embedded editable sources in removed-panel
history use project format 2. Older Emulsion versions refuse these files so they
cannot silently discard that history. Other projects keep format 1 when possible.

## Minimum reader version

The `.emu` envelope still contains the existing `project.json`, `storyboard.json`,
page and retired-panel ORA entries. No project-model field or new nested wire
shape is introduced by this gate. The supported envelope versions are 1 and 2.

`project::required_version(&Project)` computes the minimum from every retained
commit in every retired graph. It uses the same `ora::requires_preservation`
predicate as native retention: native/history feature version 13 or higher, or
an opaque `SmartEditable::Document` source even in a native-v9 document. This
covers hidden nodes, disabled or empty filter stacks, other branches, non-head
commits and retained commits not directly referenced by a board version.
Embedded source bytes are not opened to select the project version.

The retired-preservation version is a semantic constant independent of the
maximum supported envelope version. A later reader-version increase must not
automatically promote files that only need this existing preservation contract.
Future features must update the shared native-version/preservation predicate;
the project writer does not maintain a duplicate feature list.

Live pages and library drawings retain their independent native feature gates.
Recent features only in those locations do not require outer version 2. Ordinary
legacy retired drawings and native-v9-v12 masks without opaque sources remain
outer version 1. Any protected retired commit makes a mixed project version 2,
even if its current artwork and all live pages are legacy.

The envelope version is derived from the saved snapshot, not sticky loaded
state. Undoing removal can return a protected page to the live layout and permit
outer version 1. Redoing removal requires version 2 again. Removing the last
board-version reference and its retained graph can also permit version 1.

## Reader and recovery behavior

The shared header reader inspects the unsigned version before deserializing the
current manifest schema. An unsupported future version produces typed
`IoError::TooNew`, including when its current-schema fields are missing or its
kind is unknown. Zero, missing, negative, fractional, string and null versions
are invalid. Existing JSON, archive-safety and size checks remain in force.

File, stream, strict, report-aware and thumbnail readers use this boundary.
Strict reads still refuse undelivered recovery warnings; report-aware reads
still expose permitted legacy recovery. A mixed version-2 file does not turn
every legacy archive into protected content. Protected or unclassifiable retired
damage still fails rather than disappearing into legacy recovery.

Old readers checked outer version 1 before nested reads but swallowed retired
ORA errors, including nested `TooNew`. A nested version bump alone therefore
could not protect retired-only content. The new outer version uses the old
reader's existing “newer version of Emulsion; update Emulsion” refusal.

## Drawing aids and limits

The separate repair for current colors and drawing guides uses existing fields.
Legacy aid-only graphs stay format 1. Older-reader readability does **not** mean
older readers preserve those retired aids. This gate does not introduce aid
history, recover already lost state, or guarantee arbitrary unknown fields or
unreferenced resources. It does not add native-v16 support on its own.

The version calculation is read-only and leaves graph topology, allocators,
source identities and Undo state untouched. Path-based saves remain atomic on
failure. A generic streaming writer may emit partial output before a later
failure; its caller must propagate the error and must not publish that stream.

## Verification

`project::envelope_tests` exercises the v1 boundaries, protected feature matrix,
all-retained-commit selection, identity and source round trips, mixed recovery,
header errors, atomic failure, Undo/Redo and version removal.

The public-API `project_envelope_proof` example writes actual v1/v2 files for an
independently compiled older public project reader. Its fixtures cover legacy
retired drawings, opaque and disabled-filter retired sources, protected history
under a legacy head, and live-only protected content. The old executable must
be frozen before I/O integration and linked to the approved older build, not to
the new reader. Exact `TooNew(2)` is required; another error does not prove the
gate. A deliberately modified nested-only control may demonstrate old silent
omission, but cannot replace a genuine new-writer fixture.

Prepared source-only. Compilation, old/new reader execution, formatting and
aggregate validation belong to the coordinator; no runtime pass is claimed here.
