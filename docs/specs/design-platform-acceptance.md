# Design platform acceptance

Run this checklist on the packaged build on Windows and macOS. Linux test results
are recorded in the completion worklist; they do not establish runtime results on
another operating system. Keep a copy of the original project when testing file
exchange. The native format now stores portable fonts and local media in shared
archive entries; older builds reject the newer format rather than silently losing
those resources.

1. Open an existing Design project and each supplied template category. Create a
   page, add native text and an image, select a different object, remove the page,
   and Undo/Redo. Verify narrow and wide editor windows keep controls reachable.
2. Apply a shadow and multistop gradient, copy/paste text, and zoom through 66.7%,
   100% and 400%. Check crisp text foreground and single active tool highlighting.
3. Insert a responsive starter. Preview phone/tablet/desktop widths; text must
   wrap without squeezed glyphs. Edit container breakpoints and child sizing;
   exiting preview must restore the authored size and history.
4. Create a component with a variant. Edit an instance, publish source changes,
   reset selected overrides and Undo. Import/publish a project variable between
   pages and verify one Undo restores all affected pages.
5. Import a local font and local audio/video, save, close and reopen. Copy the
   project to a machine without the font installed; appearance should remain.
   Check media play/pause/seek/trim/loop and a supported local codec.
6. Play an official YouTube embed while online. A missing platform web runtime or
   unsupported codec must show an actionable error without closing the editor.
   Emulsion uses platform web runtimes; the package does not include Chromium.
7. Start a presentation with notes, overlays, hover/drag actions, component states,
   motion and text reveal. Fullscreen audience view must contain only the slide.
   Verify Escape, page navigation, presenter timer and returning to editing.
8. Export selection PNG/SVG/PDF, standalone HTML, animated SVG and rendered-frame
   Lottie. Inspect diagnostics before comparing output. Open HTML in a browser,
   test actions/media and hide controls using its fullscreen command. Lottie is
   sampled image-layer export, not editable native vector interchange.
9. Bind CSV text/image fields and generate a two-page record set. Verify links
   target pages in the same record. A missing image or malformed row must reject
   the batch without adding partial pages. Undo removes the complete batch.
10. Connect MCP to the visible editor. Inspect tool state, invoke an undoable
    authoring action, and verify relay identity stays with its originating editor
    after switching tabs. Test print preview with a configured printer; submit an
    actual print job only when desired. Driver installation remains OS setup.

Record OS/version, package identity, GPU, scaling, system web runtime and media
codec when reporting a failure. Do not include credentials or private document
content in diagnostics. macOS and Windows packaging/runtime confirmation remains
separate from Linux acceptance.
