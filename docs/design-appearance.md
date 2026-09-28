# Design appearance controls

Select an object on a Design page to show the Appearance row below the editor toolbar. Controls follow the selection's capabilities and use the same native commands as the full inspector.

- **Fill / gradient** edits vector fill color and alpha, solid/none/linear/radial paint, gradient end color and angle. Text and solid-color layers expose their color. Selected characters retain their individual formatting when changing text color.
- **Stroke** edits vector outline color/alpha, paint, width and inside/center/outside alignment.
- **Opacity** changes the whole selected object or each selected root. **Corners** adjusts native rectangular paths, preserving fill and stroke; arbitrary illustrations and responsive-frame boundaries are excluded.
- **Spacing** edits character spacing, line height and paragraph alignment. Selected character ranges receive spacing changes; paragraph settings apply to the text object.
- **Curve** keeps text editable and exposes Arc, Bulge and Flag with signed bend and horizontal/vertical distortion. Choosing None clears every warp setting.
- **Background** creates a rounded vector rectangle grouped behind the text, with adjustable color/alpha, horizontal/vertical padding and radius. The backdrop follows the text's rotation and scale. Apply again to refit it after changing the text. Remove restores the independent text object; Ungroup makes both objects independently editable.
- **Shadow**, **Outline** and **Effects** open the existing layer-style editor, including adjustable colors, opacity, offsets, blur and other effect-specific settings. Cancel restores the original effects.
- **Align**, distribution/spacing, **Group** and **Ungroup** use shared selection commands. **Object actions → Saved styles** opens reusable formatting; see [saved styles](design-styles.md).

Applying a form creates one Undo step; invalid values keep the form open and do not change the document. Background creation/removal is also one step. Text and paths remain native editable objects through save, duplication and export.

Ordinary opaque, horizontal warped text without a fixed height uses mapped glyph outlines in Vello, retaining canvas-resolution edges at zoom, and exports as SVG paths. Warped contours use bounded adaptive subdivision, with a 0.015 document-pixel target error and a 20,000-point ceiling per glyph. Unsupported text layouts, bitmap-only glyphs and incompatible blending still use the existing appearance-preserving fallback. SVG layer effects may require raster export; the new controls do not change that export limitation. Rectangular corner controls currently require axis-aligned rectangles; text backgrounds handle transformed text separately.

Regression coverage lives in `design_appearance_workflow_tests.rs` (real dialogs, validation and Undo), `editor/design_appearance_tests.rs` (native geometry, locks and project/SVG round trips), core text tests, and the engine's optional offscreen curved-text test at 100% and 300% zoom.
