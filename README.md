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
