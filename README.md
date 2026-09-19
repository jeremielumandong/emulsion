# Emulsion

An image editor in Rust and [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui).
Nodes instead of layers, a branchable history, and optional AI that proposes edits instead of
asking for prompts. Every feature works without AI; local models, a coding-CLI subscription,
or a decision-model API key each make it better.

## Status

Phase 0: spikes and skeleton. Nothing here edits images yet.

## Build

Linux needs the GPUI system libraries (`wayland`, `xkbcommon`, `vulkan`, `fontconfig`,
`freetype` dev packages) and a Vulkan-capable GPU driver.

```sh
cargo run -p emulsion-app                 # open the editor shell
cargo run -p emulsion-app -- mcp-serve    # stdio MCP server (empty tool set for now)
cargo test --workspace
```

## Install (Linux)

```sh
scripts/install-appimage.sh --build   # build the AppImage and install it
scripts/install-appimage.sh --uninstall
```

This puts `Emulsion.AppImage` in `~/Applications`, an `emulsion` command in `~/.local/bin`, and a
desktop entry with icons and file associations. Settings and recent files are never touched.
`scripts/build-appimage.sh` only builds, into `target/appimage/`.

## Build (macOS)

```sh
scripts/build-macos.sh             # build and package for this Mac
scripts/build-macos.sh --no-build  # package an existing release binary
```

Requires Rust and either `rsvg-convert` (from librsvg) or ImageMagick for the icon.
The script creates a versioned `.app` and `.dmg` in `target/macos/`. The app is
signed ad hoc for local use; distributing it requires Developer ID signing and
notarization.

## Editor controls

The right dock keeps **Layers** visible, with its own scrolling list and
**+ Layer**, **Group**, **Duplicate**, and **Delete** actions. Below it:

- **Properties** edits the selected layer, including transforms, blending, and masks.
- **Adjustments** adds adjustment layers and filters, then opens their Properties.
- **Reference** keeps an attached image beside the canvas while selecting layers.

The **Panels** menu opens Navigator, Info, Recipes, Timeline, History, or Histogram
in the same dock. Active tool options stay above the canvas.
Leaving Recipes cancels an unapplied preview; leaving Timeline stops playback
and returns to the full document.

## Drawing from a reference

Choose **Add reference** in the editor's Reference panel or Ask bar, then select
an image. Its preview stays beside the canvas for manual drawing and painting.
Ask Emulsion to use it, for example: “Draw this character in manga style; keep
the pose and expression.” The assistant can inspect the attached image directly.
Use **Replace**, **Remove**, or **hide/show** in the panel to manage it.

References belong to the current open document session; reattach them after
reopening. They are separate from artwork and are not included in exports.

To render the manga guide's example locally, without an assistant provider:

```sh
cargo run --release -p emulsion-assistant --example playbook -- --guide manga --out target/manga-studies
```

This writes a PNG and an editable ORA for inspection.

## Rotating objects

Select a layer and use **Rotate object** in **Properties**. Enter an angle and
choose **Apply**, or use the 90° buttons. Positive angles turn clockwise.
Paths and text stay editable; groups rotate together around their content.
Each rotation can be undone. Locked nodes must be unlocked first.

## Layout

| Path | What lives there |
|---|---|
| `crates/emulsion-app` | The `emulsion` binary |
| `crates/emulsion-ui` | GPUI views and design tokens |
| `crates/emulsion-core` | Document model, Command API, history graph |
| `crates/emulsion-raster`, `-gpu`, `-filters`, `-color` | Pixels, compositing, adjustments, color |
| `crates/emulsion-tools` | Interactive tools |
| `crates/emulsion-io` | File formats |
| `crates/emulsion-recipes` | Recipes and film recipes |
| `crates/emulsion-ai`, `-mcp`, `-assistant` | Optional AI: local models, MCP server, coding-CLI bridge |
| `spikes/` | Phase 0 experiments and measured results |
| `docs/adr/` | Architecture decision records |

## License

Not chosen yet.
