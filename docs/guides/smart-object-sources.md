# Smart Object source editing and links

Smart Objects can retain a complete native layered source document. In Photo Properties, choose **Edit source**; in Design, use **Object actions → Edit Smart source**. Existing pixel, text and path Smart Objects open as editable native source tabs. Supported SVG geometry opens as native paths; unsupported SVG features produce an explicit error and leave the retained original unchanged.

The source tab has **Apply to parent** and **Return to parent** controls. Save (Ctrl+S on Windows/Linux, Cmd+S on macOS) applies its current layers to the originating Smart Object. Apply retains the parent's placement, mask, opacity, effects and editable filter stack and creates one parent Undo step. It does not write an external file. Edit history inside the source tab remains independent. Source tabs can nest to eight levels; apply the deepest source first, then its parent source. Closing a dirty source tab uses the normal unsaved-work prompt. Save As writes an independent document and detaches that tab from its parent session after the save succeeds.

A source session remembers the original tab, page and source identity. Changed/undone/deleted sources, locked parents and closed parent tabs reject Apply. The source tab remains available so edits can be saved separately. Reopening the same source selects its existing session rather than creating a duplicate. If the parent has changed pages, switch it back to the originating page before applying.

## External links

The Source menu provides **Link / relink file**, **Refresh linked source**, **Auto refresh linked file**, **Keep embedded source / unlink**, **Save layered source as**, and **Write linked .ora file**. Relink is an explicit replacement of the embedded source and can be undone. Supported local inputs are native `.ora`, single-page `.emu`, editable SVG, and supported image formats.

Links retain an absolute local path, SHA-256 fingerprint and refresh policy in the native document. The embedded source is always saved alongside the link. Moving a project to a different machine does not discard its artwork: a missing path leaves the embedded source available and can be relinked. Files are not downloaded or copied to their old location automatically.

Auto refresh polls the current page of each open editor, on a background worker, every three seconds. Each turn visits at most eight files in a rotating batch. File metadata avoids repeatedly hashing unchanged files. Explicit Refresh always verifies the file contents, including changes that preserve size and timestamp. No directory watcher or platform-specific service is required. When a linked file changes, the embedded source and cache update in a native Undo step. Undo of an automatic refresh remains in place until the file changes again or Refresh is requested.

Applying local source edits marks the embedded source modified. If both the external file and embedded source change, refresh refuses to discard local edits. Save a separate source, or use the explicitly labelled **Discard local source edits and refresh** action. Missing files, decode failures and lock conflicts retain the previous embedded artwork and report an error.

Only **Write linked .ora file** overwrites an existing external file. It checks the stored fingerprint and refuses an externally modified file. Document Undo does not undo disk writes. Image/SVG/project originals are protected: use **Save layered source as** to create and link a new `.ora` file; that action refuses existing paths. Normal Apply never writes the link target.

## Native persistence and resource limits

Native archive/history version 9 stores source archives as content-addressed ZIP resources (`sources/<sha256>.ora` and `history/sources/<sha256>.ora`). History snapshots share each identical source archive rather than repeating byte arrays in JSON. Read validates resource sizes and hashes. Clipboard/duplicate/component operations retain the source descriptor with the native Smart Object. Source raster caches remain disposable; native layers and portable fonts stay inside the embedded archive.

Each source input/archive is limited to 64 MiB; an editable source document supports up to 64 megapixels and 2,000 layers. A source resource pool is limited to 512 MiB. Source IO, native encoding, cache compositing and external refresh run off the UI thread. Parent composite/export uses the Smart Object's rendered cache; retaining editable vector/text layers is not a promise that every enclosing flattened export remains vector artwork.

## MCP

All tools require the live editor host and retain the originating relay tab identity:

| Tool | Parameters |
| --- | --- |
| `inspect_smart_source` | `node`; returns dimensions, external descriptor, revision and nested session identity without filesystem IO |
| `open_smart_source` | `node`; opens/selects a source tab and returns its ID |
| `apply_smart_source` | none; applies the originating source-editor tab to its parent |
| `link_smart_source` | `node`, `path`, optional `auto_refresh` |
| `refresh_smart_source` | `node`, optional `discard_local` (default false) |
| `set_smart_source_auto_refresh` | `node`, `enabled` |
| `unlink_smart_source` | `node`; retains embedded layers |
| `save_smart_source_as` | `node`, new `.ora` `path`; explicit new-file write |
| `write_linked_smart_source` | `node`; explicit fingerprint-checked overwrite of a `.ora` link |

Unknown/null arguments are rejected. Asynchronous results check the original page/edit ticket before committing. If an explicitly requested file save succeeds while its tab changes or closes, the result reports that disk save completed and that link metadata could not be applied.

Regression coverage includes layered nesting/native round trips, source Apply/Undo, external edits/refresh, local conflict refusal, missing-source fallback, existing-path/lock rejection, link persistence, history resource storage, strict MCP parsing and native nested source-tab Apply/stale-parent handling. Platform GUI acceptance remains part of the Windows/macOS checks; the source IO itself uses cross-platform Rust APIs.
