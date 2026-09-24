# Photographer workflow recipes

*Snapshot from 2026-09-20. For current behavior see [README › Developing RAW photos](../README.md#developing-raw-photos).*

## First release: capture and reuse current adjustments

1. Capture a selected adjustment layer or flat adjustment group into versioned recipe data. Preserve values, order, names, visibility, opacity and blend modes. Embed LUT values so the recipe travels with its dependencies.
2. Add **Save edits as recipe** to the Recipes panel. Let the photographer name/tag the recipe and select included stages, including whether exposure and white balance should transfer. Separate **Save new** from **Update existing** to avoid accidental replacement.
3. Use the existing library, preview/cancel, editable one-step application and batch export paths for both captured workflows and existing film recipes. Expose capture through MCP with the same validation.
4. Explain unsupported content. This release captures adjustments; it does not transfer source pixels, Smart filter stacks, local masks, clipping, retouching, cropping, or RAW development. Imported film fields that are stored but not rendered must be visible as limitations.
5. Verify exact rendered equivalence after save/reload on the same source, preservation of editable stages, excluded settings, portable LUTs, collision handling, undo, preview/cancel and batch output on new photos.

Capture stores the selected group's current state. It does not replay the edit history. A new photo can still need its own exposure or white balance correction. Captured stages remain editable in their own recipe group.

## Using saved workflows

1. Put the adjustments you want to reuse into a flat adjustment group, or select a single adjustment layer.
2. Open **Panels → Recipes → Save edits as recipe…**. Name the recipe, add comma-separated tags and notes, and deselect any stages that should stay specific to this photo.
3. Choose **Save new**. To replace an existing saved recipe, enter its exact name and choose **Update existing**. Saving does not change the document or add an undo step.
4. On another photo, click the recipe to preview it. **Apply** keeps its editable group as one undo step; **Cancel** restores the photo. Open the applied group's layers to refine individual settings.
5. Open **Batch**, choose a folder and the saved recipe, then export the selected photos. Returning to Batch refreshes newly saved or updated recipes. Exports preserve existing files by using numbered filenames when necessary.
6. Use **Export bundle** to share saved recipes, including embedded LUT data. Import the bundle on another installation.

The assistant can use `save_recipe` with `node`, `name`, optional `tags`, `notes`, `exclude_nodes`, and explicit `overwrite`. `list_recipes` describes the saved stages; `apply_recipe` and `batch_export` reuse them.

Legacy film recipes continue to work. Their stored sharpness, noise reduction and Color Chrome FX Blue fields are reported when the renderer cannot apply them. Captured native adjustment stages do not rely on those approximate film fields.

## Verification — 2026-09-20

- `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` passed.
- Release workspace tests passed all 299 non-UI tests. After correcting the new visibility test to open the Recipes panel through its normal UI path, `cargo test --release -p emulsion-ui --lib` passed all 121 UI tests. Total: **420 passed**, with the existing external Claude CLI integration test intentionally ignored.
- New regressions cover capture/serialized pixel equivalence, editable stages, embedded LUT portability, stage exclusions, unsupported content, malformed imports, save/update collisions, visible form rendering, stale draft rejection, preview replacement/cancel/failure, one-step undo, Batch catalog refresh, stale background previews, and safe batch outputs.
- Paid model calls and manual visual review are not part of this verification.
- `scripts/build-macos.sh` passed and produced `target/macos/Emulsion.app` and `target/macos/Emulsion-0.0.1-arm64.dmg`.

## Follow-on releases

- Selective copy/sync across photos, per-photo overrides and comparison previews.
- Persistent RAW development parameters, safe background result ownership, clipping warnings and calibrated color-management controls.
- Filmstrip, ratings/rejects, comparison and keyboard culling.
- Optional delivery presets: format, dimensions, quality, color profile, watermark and filename templates.
- Explicit rules for transferring local edits, including normalized geometry and per-photo subject masks; no automatic reuse of pixel-coordinate retouching.

These follow-ons require separate implementation and validation. The first release establishes reusable native adjustment recipes while retaining existing film recipes.
