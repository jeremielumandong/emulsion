# Native vector editing and precision

Selected vector shapes expose **Edit points, join, split, skew, envelope…** in Properties. The Pen tool remains the direct canvas editor for anchors and Bézier handles. Numeric point editing accepts document coordinates and zero-based subpath/anchor indices. Join connects one open contour's end to another's start; Split opens a closed contour or divides an open contour at an interior anchor.

The Compound, Union, Subtract, Intersect and Exclude controls operate on selected sibling paths. Compound retains curve handles; Boolean operations produce editable contours. The first selected path retains its identity and style, and other operands are removed in the same Undo step. Clipping-stack operands and protected objects are rejected before any change.

Skew retains Bézier handles. Envelope uses a four-corner bilinear mapping; Perspective uses a true projective mapping. Both warp operations sample curves into editable points and reject invalid corners. The MCP mesh operation supports a rectangular 2–16 by 2–16 control grid. Undo restores the original curves. The maximum native path size remains 20,000 anchors.

**Create editable stroke outline** creates a separate native path and keeps the original source. It includes caps, joins, dashes, and centered/inside/outside alignment. It currently requires a solid stroke; it does not reproduce the source layer's masks or effects on the new outline. It is a copy operation, so hide or remove the original explicitly if desired.

Fill and stroke **Edit gradient stops…** dialogs support 2–16 ordered colors, opacity, linear angle, radial gradients, and equal-position stops for hard transitions. The representation stays native and persists through save, clipboard, reusable appearances, and Undo. Opaque gradients use native Vello paths at canvas zoom; translucent stops use the existing compositing fallback. SVG keeps gradients and vector geometry, with additional intermediate stops for consumers that ignore linear color interpolation.

**Select matching objects** finds visible objects by type, fill, stroke, or opacity. MCP also supports matching text font attributes. Hidden objects and content under hidden ancestors are excluded.

## Image tracing

For an image, open **Trace to vector…** from Design object actions. Choose dark foreground or alpha silhouette, threshold, optional inversion, and a sampling resolution of 16–512 pixels on the longest side. Update the preview before creating the vector. Work runs locally and preview generation runs outside the UI thread. The committed geometry is the geometry previewed; a changed source requires a fresh dialog.

The result is a monochrome editable path with holes and the source placement preserved. The original image remains intact. This is a threshold contour tracer, not multicolor illustration reconstruction or curve fitting. It samples image pixels and Smart filter results; layer masks, blending and layer effects are excluded. Complex results above 20,000 points require a lower resolution or different threshold.

## Rulers and exact placement

**Rulers and precise placement…** selects pixels, millimeters, inches, or points and a custom ruler origin. Physical units use the document print resolution. Changing units or origin does not move artwork. Settings persist per page and are undoable.

Optional X/Y fields position one object's top-left relative to the ruler origin in the selected units. The gap field arranges two or more independent objects in geometric order with exact horizontal or vertical edge spacing. Negative gaps overlap objects. Paths use geometric bounds; frames use their boundary; other objects use artwork bounds. Dependent moves are validated together, so a locked object cannot leave a partially applied spacing operation.

Existing width/height and transform controls remain available in pixels. Units affect rulers and the new numeric placement/gap dialog; they do not reinterpret authored document geometry or existing pixel-valued controls.
