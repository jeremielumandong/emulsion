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
