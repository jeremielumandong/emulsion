# Native text lists and decorations

Select a text object and open **Aa → Character and paragraph**. **Underline** and **Strikethrough** apply to selected characters, or the entire layer when no character range is selected. These attributes also travel with saved typography styles and reusable components. They remain native geometry in the Vello canvas and supported SVG/PDF exports, following text scaling, rotation, rich colors, baseline shifts and paragraph clipping. CPU previews and effects use the same shaped decoration geometry.

**Bullets**, **Numbered**, and **Remove list** act on the selected text layer. They insert or replace editable line prefixes while preserving character formatting and indentation. Blank lines stay blank and restart numbering. Switching between bullet and numbered lists replaces existing prefixes rather than accumulating them. Each action is independently undoable. Lists remain ordinary editable text in projects, clipboard content, templates and exports.

MCP `add_text` and `set_text` accept `list: "bullet" | "numbered" | "none"`, `underline: boolean`, and `strikethrough: boolean`. `format_text_range` accepts the two decoration booleans for character ranges. Text inspection returns both attributes in base and run styles. Malformed input is rejected before document mutation.

List controls currently affect all nonempty lines in the layer. They do not provide nested outline numbering, automatic continuation on Enter, or hanging indents for wrapped lines. Decorations use font-size-derived thickness and offsets; custom decoration styles are not yet exposed.
