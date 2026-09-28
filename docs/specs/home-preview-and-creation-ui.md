# Home previews and document creation

## Problems addressed

Home cards were compressed into thumbnail strips as the gallery grew, hiding
filenames and actions. Repeated page/filter changes discarded decoded previews.
Home requested image sizes from the whole window rather than the actual card.
The New document gallery rebuilt template previews on category changes and had
no direct photo import. Opening a classified painting could inherit Photo mode.

## Behavior

- Home content keeps its natural height inside its scroll area. Cards retain
  thumbnails, filenames, project information, and contextual actions.
- Home requests thumbnail pixels from the card width and display scale, rounded
  to resize buckets. List rows request smaller previews. Two decodes run at most.
- Up to 96 recently requested Home previews are retained. Eviction uses recent
  page access; asynchronous results cannot repopulate an evicted entry. Saving
  invalidates the affected preview and its older requests. Project Reload clears
  cached previews. The existing disk cache and saved-photo processing remain.
- Template previews arrive independently with at most two pending renders per
  dialog. The selected template gets priority. An app-session cache retains up
  to 64 built-in thumbnails, shared across categories and dialog openings.
  Full editable documents are not cached. User template previews stay local to
  the dialog, so reopening reads the current source.
- One checked category menu replaces the template category button wall.
- Import photo is available in all four New document types, beside the starting
  options. It opens the native multiple-file picker without making an empty
  canvas. Cancellation leaves existing documents intact.
- Normal image opening follows the same persisted Photo/Paint classification as
  Home. Explicit Photo import uses Photo mode. This does not infer document type
  from its appearance or change saved classification.

## Validation

Focused headless checks cover card geometry with a populated gallery, cache
reuse/eviction, DPI sizing, import/cancel behavior across all creation types,
Paint reopening and explicit Photo opening, and existing Home/template flows.
Result: all 26 focused checks passed in the isolated validation checkout.
The seven changed source/test files match the workspace, apart from excluding
an unrelated concurrent Library-print routing line from the validation copy.
The shared release build uses the full workspace. These checks establish
behavior and bounded work; they do not establish an end-to-end latency benchmark
or a physical-window visual review.

The full shared workspace release build and AppImage packaging succeeded.
Artifact: `target/appimage/Emulsion-0.0.3-x86_64.AppImage`. Installation was
attempted, but the installer stopped because the installed Emulsion was still
running. The running session was left intact.

## Follow-up: compact splash and empty documents

The startup artwork now appears as a centered card up to 680 pixels wide, with
Home visible around it and 24-pixel margins on small windows. It preserves the
artwork's aspect ratio and existing timer/click/key dismissal; it does not change
the native editor window's bounds.

Design and Diagram galleries also offer a visible Blank document card above the
template choices. It opens the existing blank-canvas setup, clears the selected
template/name, and creates an empty page project of the chosen kind. The existing
Blank canvas toolbar option remains available.

Three focused interaction tests passed: splash geometry/dismissal, empty Design
and Diagram creation, and the existing template creation flow.

## Compact Recent history

Recent opens newest-first with at most 12 files from the last 14 days. Work
opened 14–29 days ago and work opened at least 30 days ago appear in separate,
collapsed sections. Each expanded section has its own 12-file page controls.
No entries are deleted or removed from their projects. Search, project folders,
type filters, pinned/unfinished views, Trash, and name sorting retain the full
filtered collection with its existing 48-file pagination. Choosing Recent in
the sidebar restores chronological order.

Collapsed age sections create no cards or thumbnail requests; all three open
sections together render at most 36 cards. Existing bounded thumbnail workers
and cache remain in use. Card footers show a compact sync status icon with an
accessible provider/status description. Clicking it opens sync actions; the
file context menu also offers connection, upload, pause/resume, and history as
appropriate. Full sync controls remain in file details.

Regression coverage checks exact 14/30-day boundaries, reverse input ordering,
12-file page transitions, collapsed preview loading, searching older files,
and cloud actions with unsaved work.
