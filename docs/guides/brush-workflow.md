# Brush Library and Brush Studio

Open **Brush library** from the Presets sidebar. Selecting a brush keeps the current Paint, Smudge, or Erase operation. Browsing sets or searching leaves the active brush unchanged. All document editors share one persistent catalog.

## Organize and exchange

- Select a library and set on the left. Search matches brush names, descriptions, and set names across libraries. Recent and Pinned are separate views.
- Click a brush to use it; Ctrl-click or Shift-click toggles multiple selections. Double-click or choose **Brush Studio** to edit. With the library focused, Up/Down selects and Enter edits.
- Create, rename, duplicate, and delete brushes, sets, and libraries. Built-in collections cannot be deleted. Move brushes with the destination menu or Move up/down; use the set/library menus to move or reorder collections.
- Pin selected brushes or combine two in catalog order. Uncombine creates independent copies and keeps the combined original.
- **Export** writes selected brushes, or the visible collection when nothing is selected. **Export set or library** includes the entire chosen collection and its hierarchy. Native `.embrushes` packages retain settings, source images, original settings, reset points, and dual components.
- **Import** parses in the background and presents compatibility warnings before publishing. See [format support](brush-import-formats.md) for external conversion limits.
- Changes are published only after the store commits successfully. Undo reverses the last library change while its revision is still current. If another change intervened, Undo refuses to overwrite it. Reload reads changes made by another process.

## Edit a brush

Studio has category navigation, numeric/slider settings, and a drawing pad. Drag the divider to resize the settings and pad. Draw a test stroke; changing parameters replays the recorded samples using the same CPU brush renderer as the canvas. The pad can paint, smudge, and erase, with color and background choices. Its Undo/Redo actions affect test strokes; document-editing shortcuts are blocked while the brush workspace is open.

**Done** publishes the validated draft. **Cancel** discards it, including changes to reset points; neither operation paints the document. A failed save keeps the draft open. **Save as new brush** preserves the draft as a separate definition in the latest catalog rather than overwriting a concurrently edited original.

Shape and Grain accept source images and can invert or rotate them. Grain can produce a seamless mirrored tile. Original generated diamond and paper sources are available; procedural grain options remain available too. Image work runs outside rendering, and stale requests cannot overwrite a subsequently selected component or reset source.

Dual brushes have independent primary/secondary settings and sources. Choose their composition mode in Studio. Normal overlays the components, Multiply intersects their coverage, and Screen unions coverage while screening pigment. These are Emulsion's defined compositing modes, not a claim of identical Procreate brush physics.

The categories expose implemented stroke-path variation, staged smoothing, taper, shape count and orientation, grain coordinates, glazing/accumulation, wet pickup and reservoir behavior, color variation, speed/pressure/tilt dynamics, size/opacity bounds, metadata, original settings, and a custom reset point.

## Quick settings and pen input

The brush-settings popup has four paired size/opacity memories per brush and painting tool. Save or replace a slot, recall it, or clear it. Tool switching restores size/opacity independently; transfer buttons copy the active brush to Paint, Smudge, or Erase.

On Windows, native pen messages provide pressure and tilt when available. Mouse input remains usable. Hardware/driver validation is still required; automated tests cannot substitute for a physical tablet.

Interactive wet painting defaults to **Sample visible layers**, preserving the existing application behavior. Choose **Sample current layer** to restrict pickup. This is explicitly different from MCP's opt-in `sample_merged`, which samples the target and lower layers while excluding upper layers; MCP defaults to current-layer sampling.

## Rendering and validation

The CPU implementation is the reference for advanced and dual brushes. These brushes bypass unsupported GPU paths. The drawing pad uses a fixed seed for reproducible replay, and library previews are generated lazily with a bounded image cache.

Run `cargo run -p emulsion-io --example brush_benchmark --offline -- target/brush-validation` for 4K CPU stroke-update measurements and preview swatches. It does not measure display latency, native input latency, or GPU performance. No 60 Hz guarantee is implied by the renderer's existence.

3D material brushes and cloud synchronization are outside this implementation. External formats are conversions, and native export does not produce Procreate-readable archives.

## MCP automation

The brush workflow is also available through eight MCP tools for catalog management, Studio edits, source images, import/export, memories, and standalone previews. Painting accepts timed pressure/tilt samples, deterministic seeds, and primary/secondary overrides. See [Brush MCP guide](brush-mcp.md) for revision handling and examples.
