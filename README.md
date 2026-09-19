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
