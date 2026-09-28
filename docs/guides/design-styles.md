# Reusable appearance styles

Select one object and choose **Save as reusable style…** from its Object actions menu, or open **Brand → Saved styles → Save selection as style…**. Name the style. The selected object becomes its first linked consumer.

Saved styles contain typography, color, native fill/stroke paints, opacity, blending and layer effects. They preserve each target's text content, geometry, masks and object identity. A saved typography style samples the source's first character; publishing it preserves the source's mixed character formatting and applies the sampled style to other consumers. Text styles can transfer color to shapes, and shape colors can transfer to text; type-specific settings apply only to compatible objects.

- **Apply** styles selected objects and links them to the saved definition.
- **Manage → Update from selection** publishes the selected object's current appearance to every linked consumer on the active page. Local formatting edits remain local until this action is chosen.
- **Reset linked appearance** discards a selected object's local appearance edits and restores the saved definition.
- **Detach style** removes the link while preserving the current artwork.
- **Rename** changes the definition and its links. **Remove style; keep appearance** removes the definition and detaches its consumers.

Each action is one Undo step. Update/apply preflight all affected locks; a protected consumer prevents the whole operation rather than leaving a partial update. Styles and links survive native saves, recovery, duplicate and clipboard operations. Clipboard imports rename conflicting definitions instead of overwriting the destination style.

The library lists styles from every project page. Applying another page's style imports an independent definition into the active page. Subsequent updates propagate on that page; they do not automatically change other pages. There are up to 128 named styles per page. Named color/number variables and property bindings are separate capabilities.
