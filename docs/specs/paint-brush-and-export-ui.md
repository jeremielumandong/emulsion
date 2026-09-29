# Paint brushes, document tabs, and export

The canvas shelf and Paint Properties were both presenting complete brush
collections, while a floating Brushes shortcut opened the same settings again.
Paint also put document tabs into the menu row. The header export opened an
inline options strip, but File → Export skipped it and opened a save prompt
using the last format (PNG by default).

## Interaction contract

- Photo and Paint document tabs occupy a dedicated row, outside the floating
  toolbar area. Layout presets do not relocate document navigation.
- The brush shelf contains at most four quick choices: the current brush,
  pinned brushes, and recent brushes, with builtins filling unused slots.
  Browse opens the existing stroke-preview gallery; pinning remains available.
  All pinned brushes remain accessible in the gallery.
- Paint Properties summarizes the active brush, size, and opacity. Choose brush
  opens the gallery; Edit settings opens the dedicated settings panel.
- Brush settings uses GPUI TabBar for Brushes, Tip, Texture, Dynamics, and
  Drawing, with an overflow menu. Local arrow/Home/End handling and scroll
  reveal fill the vendored TabBar's keyboard navigation gap. Library and set
  selection use checked GPUI popup menus instead of wrapping category buttons.
  Brush rows retain their virtualized list, with left-aligned names and explicit
  selected styling. Save and Import share a small command toolbar.
- The full settings panel has one host: dock or canvas flyout. The flyout's
  title bar owns its close control. Moving it does not change the document.
- Header Export and File → Export open the same GPUI Dialog. Format, bit depth,
  size, profile, and resolution are grouped vertically. JPEG quality and other
  format-specific controls appear as needed. Export opens the native save
  prompt only after the user chooses settings; Cancel/Escape leaves the
  document unchanged. The canvas does not resize when the dialog opens.
- Design/Diagram enter the same dialog. Their existing page, vector,
  presentation, and selected-object exports remain in its Project export menu;
  choosing one dismisses the common dialog before its specialized flow opens.

## Component choice

Uses the vendored GPUI Kit components and the already backported Base Toolbar.
The upstream component guidance for [Tabs](https://gpui-kit.com/component/tabs/)
and [Dialog](https://gpui-kit.com/component/dialog/) informs these controls.
This change does not upgrade GPUI or the rendering backend. Export encoding,
project formats, brush definitions, and saved layout schemas are unchanged.

## Verification

Interaction coverage exercises brush selection and saved libraries, moving the
settings panel between dock and flyout, the Paint tab row at narrow widths,
and both export entry points, format persistence, file-prompt confirmation,
cancellation, and unchanged document/undo state.

Validation result: 38 focused UI checks passed in a clean HEAD snapshot with
all 15 changed UI source/test files copied byte-for-byte. This includes the
keyboard navigation/scrolling regression, existing brush library workflows,
both export entry points, narrow dialog footer, specialized project export,
and compact document-tab interactions. The shared test build was blocked by
concurrent Library test edits; those files were left intact. Headless interaction
checks do not constitute a physical-window or cross-platform visual review.
