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

## Build (Windows)

Install Rust and Visual Studio Build Tools with the **Desktop development with C++**
workload (including the Windows SDK). Use the MSVC Rust toolchain; the repository's
`rust-toolchain.toml` selects the Rust version.

```powershell
.\scripts\build-windows.ps1                       # release build
.\scripts\build-windows.ps1 -Configuration Debug  # development build
.\scripts\build-windows.ps1 -Package              # release build + NSIS installer
```

The script works from any working directory and builds the `emulsion` executable
under `target/` (normally `target/release/emulsion.exe` or `target/debug/emulsion.exe`).
Cargo errors stop the script. Release builds launch without a console window;
debug builds retain the console for diagnostics.

For packaging, install `cargo-packager` with
`cargo install cargo-packager --version 0.11.8 --locked`. Like AgentOps, `-Package`
uses cargo-packager's NSIS backend (downloaded automatically on first use) and
creates `target/windows/emulsion_<version>_x64-setup.exe` on x64 Windows.
The installer installs for the current user, supplies shortcuts and an
uninstaller, and bundles runtime DLLs beside the application. The app and
installer use the Emulsion icon. These local builds are unsigned.

Close a running copy from `target/release/` before rebuilding, since Windows
locks executable files while they are in use. Coding assistant detection supports
native Windows executables and npm launchers, including Claude Code in
`%USERPROFILE%\.local\bin` and Codex in `%APPDATA%\npm`.

## Build (macOS)

```sh
scripts/build-macos.sh             # build and package for this Mac
scripts/build-macos.sh --no-build  # package an existing release binary
```

Requires Rust and either `rsvg-convert` (from librsvg) or ImageMagick for the icon.
The script creates a versioned `.app` and `.dmg` in `target/macos/`. The app is
signed ad hoc for local use; distributing it requires Developer ID signing and
notarization.

## Compact project files

Saving an ORA stores each unique editable path once, shared by the current
artwork and its history. Coordinates remain full precision; redundant corner
handles are omitted and reconstructed exactly. Layer previews and history are
kept, with lossless ZIP compression.

Older projects open normally. Newly saved projects use native format version 2
and need this updated Emulsion build for native editing; other OpenRaster
readers can still use the included layer previews.

To create and verify a separate compact copy:

```sh
cargo run --release -p emulsion-io --example compact_native -- input.ora compact-copy.ora
```

## Editor controls

The right dock keeps **Layers** visible, with its own scrolling list and
**+ Layer**, **Group**, **Duplicate**, and **Delete** actions. Below it:

- **Properties** edits the selected layer, including transforms, blending, and masks.
- **Adjustments** adds adjustment layers and filters, then opens their Properties.
- **Reference** keeps an attached image beside the canvas while selecting layers.

Choose **Grade** in the left toolbar for colour and tone work. Its top bar adds
Exposure, Curves, Color Balance, or Hue/Saturation as editable adjustment layers;
**All adjustments** opens the full catalogue. Selecting Grade with an adjustment
layer selected opens its existing controls in Properties.

The **Panels** menu opens Navigator, Info, Recipes, Timeline, History, or Histogram
in the same dock. Active tool options stay above the canvas.
Leaving Recipes cancels an unapplied preview; leaving Timeline stops playback
and returns to the full document.

## Batch controls

Choose a folder in the top bar, then select photos in the left pane. The centre
shows the current photo; the right dock groups **Recipe** and **Export settings**.
Open the recipe chooser to search by name or tag, and expand **Category** when
you need a filter. Choosing a recipe closes the list. Set the format and output
folder in the dock, then use **Export** in the top bar.

## Image generation

Press **Ctrl-K** and choose **Assistant**, **Local SD**, **OpenAI**, or **Google**.
Assistant keeps the editing and drawing workflow. The image providers create
a new raster layer; with an active selection they fill that area using the
surrounding canvas as context. Enter your prompt and press **Enter**. Generated
layers are labelled with their model and can be hidden or undone.

Configure providers under **Settings → Image generation**:

- **Local SD:** start A1111 / Forge with `./webui.sh --api`; use
  `http://127.0.0.1:7860` as the base address.
- **OpenAI:** save an API key, or set `OPENAI_API_KEY`. The default model is
  `gpt-image-2.5-sunburst`.
- **Google:** save a Gemini API key, or set `GEMINI_API_KEY` (`GOOGLE_API_KEY`
  is also accepted). The default model is `gemini-3.1-flash-image`.

Each provider keeps its own model and credentials. Environment keys take
precedence over saved keys. **Save & test** checks key/model access without
generating an image; it does not verify generation quota. If Google reports
`free_tier` with `limit: 0`, check billing, prepaid credits, and model quotas
for the API key's project in Google AI Studio. Waiting for a retry timer will
not increase a zero quota. Cloud generation sends prompts and fill context to the chosen
provider and uses separately billed API access, not a chat/CLI subscription.
Google fill follows mask instructions; Emulsion masks the result locally so
pixels outside the selection stay untouched. Cancelling discards the result;
it does not guarantee cancellation of the provider's processing or charges.

The Select tool's **generate** field uses the default provider from Settings.
Attached Reference images are used by **Assistant**; image modes currently
use the prompt and selected canvas context.

Provider documentation: [OpenAI Images](https://developers.openai.com/api/docs/guides/image-generation),
[Gemini Images](https://ai.google.dev/gemini-api/docs/generate-content/image-generation).

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
