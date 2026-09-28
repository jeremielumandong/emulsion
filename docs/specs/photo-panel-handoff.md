# Photo panel handoff

Reference: `Emulsion Editor v2.dc.html` from the Modern AI-Powered Design App handoff, plus the five Photo panel reference images. The native GPUI implementation uses the existing bundled Geist / Geist Mono fonts, palette, accent, light/dark mode and corner preferences.

## Layout and controls

- Canvas-side rail: Properties, Brushes, History, Character, Assistant. One flyout opens at a time. Panels use the handoff's 300 px width at default interface scale, 40 px header, 52 px right offset, rounded border, shadow and scrollable content. The width is constrained on narrow windows.
- Dock and flyout selections are independent. Opening a flyout preserves the dock's current panel unless it would mount the same editing controls twice; in that case the dock shows RAW Develop or Adjustments. Moving a flyout to the dock transfers its selected panel. Layers and Color remain available. Paint retains its existing brush settings workflow.
- Properties: actual cached document preview; Fit, 100% and zoom steps; editable transform fields; flip actions; collapsible alignment, blending, mask and quick actions. Smart filter and adjustment parameters remain available under Layer controls. Selected text displays its Character controls.
- Brushes: actual rendered stroke preview, four filled round sliders, compact numeric spacing/smoothing/angle/roundness fields, pressure/tilt toggles, searchable three-column preset gallery, and the full brush library. Twelve presets per page; at most twelve preset render jobs pending and forty-eight cached previews. Rendering happens in the background. Catalog revision and theme changes invalidate previews; stale results are discarded. Brush sliders have independent measured tracks from toolbar and popup controls.
- History: chronological states, active-state marker, Undo, Redo and Versions. Selecting a prior state uses the existing undo path.
- Character: font selection, compact numeric fields, formatting icon buttons, text color, paragraph alignment and lists, warp and path options. The existing selected-character styling and undo operations are reused.
- Assistant: recent conversation, inline composer, working/error state and Stop. Pending tool requests retain explicit Apply/Skip controls and the existing approval implementation. Advanced Assistant options remain accessible.

## Behavior and validation

Opening, closing and docking panels must not modify the document or add undo entries. Brush setting changes affect the next stroke, not existing pixels. Numeric brush fields reject non-finite values and clamp to the supported range. Layer and text edits continue to use document commands and transactions. RAW opens on Develop, and Develop remains usable beside the Properties flyout.

The reference contains illustrative controls that do not have corresponding native operations. This implementation presents supported operations with accurate labels and units: text sizes remain pixels, line height remains a multiple, pressure controls flow, and mask feathering remains the existing explicit 6 px action. It does not add inert density, font-matching or AI comparison buttons.

Regression coverage: Photo flyout geometry/routing, shared dock/paint behavior, brush numeric input validation, RAW Develop alongside Properties, text styling/undo, and mask workflows. No GPUI or renderer dependency migration is required for this change.

## Verification — 2026-09-28

The native test build succeeds. All 39 targeted shared-panel, RAW, text-property and mask workflow tests pass, including narrow-window panel geometry, numeric-field focus, independent Fill slider undo, and RAW controls remaining visible alongside the Properties flyout. Changed panel files pass formatting checks, and the diff has no whitespace errors.

Strict UI lint still reports ten existing errors in the concurrent Library and Diagram code; none are in the Photo panel changes. The separate Home-to-Library navigation test is also affected by the concurrent Library navigation changes and is excluded from this targeted run. These checks validate native layout and behavior; a pixel comparison of the running app against the handoff has not been performed.
