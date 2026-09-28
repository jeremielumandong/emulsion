# Native text lists, paragraph spacing and decorations

Select a text object and open **Aa → Character and paragraph**. **Underline** and **Strikethrough** apply to selected characters, or the entire layer when no character range is selected. These attributes travel with saved typography styles and reusable components. They remain native geometry in the Vello canvas and supported SVG/PDF exports, following text scaling, rotation, rich colors, baseline shifts and paragraph clipping. CPU previews and effects use the same shaped decoration geometry.

**Bullets**, **Numbered**, and **Remove list** target selected paragraphs or the paragraph at the active text caret. With no text selection or caret, they target the entire layer. **Lists and paragraph spacing…** additionally controls nesting level (0–8), left indent, hanging marker offset, spacing before and after, paragraph alignment, and optional numbering restart. Multiple selected paragraphs restart only on the first paragraph. Nested decimal numbering follows the parent sequence; bullet symbols vary with nesting level.

Wrapped list content aligns to the content indent. Markers remain ordinary editable source text, while paragraph metadata controls layout. Long markers receive enough space to avoid colliding with their content. CPU ink, Vello glyphs, caret positions, decorations and vector exports share the same paragraph layout. Enter continues a list; Enter on an empty item outdents one level or exits the list. Undo restores both text and formatting. Projects, clipboard and templates retain paragraph metadata and rich character styles.

Paragraph formatting supports up to 1,000 paragraphs per text layer. Indentation and spacing are local document pixels; levels add 1.5 em of indentation. The controls currently apply to horizontal text. Decorations use font-size-derived thickness and offsets; custom decoration line styles are not exposed.

## MCP

`inspect_text_paragraphs` accepts a text node and returns paragraph indices, source text, byte starts and paragraph formatting. `format_text_paragraphs` accepts `node`, optional inclusive zero-based `first`/`last` indices, and a partial `format` object. Supported fields are `list` (none/bullet/numbered), `level`, `indent`, `hanging`, `space_before`, `space_after`, `align`, and `restart`. Omitted fields inherit the first targeted paragraph. `align: null` inherits layer alignment; `restart: null` continues the sequence. Invalid ranges, unsupported fields and protected targets fail before mutation.

The existing `add_text`/`set_text` `list` option retains its legacy whole-layer editable-prefix behavior. Use `format_text_paragraphs` for native hanging indents and nested lists. `add_text`/`set_text` also accept `underline` and `strikethrough`; `format_text_range` applies these decorations to character ranges. Text inspection includes paragraph metadata and decoration attributes.
