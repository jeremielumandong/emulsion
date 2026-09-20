# Emulsion

An image editor in Rust and [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui).
Layers that are all nodes in one graph (pixels, adjustments, masks, text and paths alike), a
branchable history, and optional AI that proposes edits instead of asking for prompts. Every feature works without AI; local models, a coding-CLI subscription,
or a decision-model API key each make it better.

## Status

Actively developed and used daily. Editing, brushes, recipes, RAW and PSD, batch export, the
assistant relay and optional local AI models all work; see the sections below for controls.

## Build

Linux needs the GPUI system libraries (`wayland`, `xkbcommon`, `vulkan`, `fontconfig`,
`freetype` dev packages) and a working graphics driver. Hardware is preferred;
CPU-only VMs can use Mesa Lavapipe. Windows falls back to WARP when hardware is
unavailable. See [rendering and VM support](docs/rendering.md) for driver requirements
and software-renderer testing.

Image processing uses GPU compute selectively for expensive compositing and
noise reduction, with CPU fallback. See [GPU coverage and controls](docs/gpu-rendering.md).

```sh
cargo run -p emulsion-app                 # open the editor shell
cargo run -p emulsion-app -- mcp-serve    # stdio MCP server (empty tool set for now)
cargo test --workspace
```

GPUI's exact dependency versions are vendored under `vendor/gpui/` and selected
through Cargo path patches. See [maintaining GPUI](vendor/gpui/README.md).

Emulsion's original code is [MIT licensed](LICENSE). Vendored code retains its
upstream licenses; GPUI is primarily Apache-2.0. See
[third-party notices](THIRD_PARTY_NOTICES.md) for attribution and scope.

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

## Appearance

Use the Light/Dark controls in the top bar to choose Emulsion's built-in palette.
On Linux, the **◆ omarchy** control beside them takes the colours of the active
Omarchy theme and follows changes while Emulsion is running (within about a
second); while it is on it shows the theme's name. Choosing Light or Dark, or using
the theme toggle shortcut, switches back to Emulsion's own palette. On an Omarchy
desktop a first run starts in the theme's colours; an existing settings file is
left as it is. macOS and Windows keep the built-in theme controls. The package also
ships a symbolic icon (`emulsion-symbolic`) that themed panels and launchers can
recolour.

Emulsion reads `omarchy/current/theme/colors.toml` under `$XDG_STATE_HOME`
(default `~/.local/state`), with a fallback to `$XDG_CONFIG_HOME` (default
`~/.config`) for older installations. No hooks or system configuration changes
are needed. If no valid palette is available, the saved built-in mode is used;
a failed live reload retains the last valid palette until another valid one appears.
The canvas surround and transparency checkerboard keep Emulsion's neutral colors
for the selected light/dark mode; theme colors never alter document pixels.

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

## Opening files

Emulsion decodes these itself: its own `.ora`, Photoshop `.psd`/`.psb`, GIMP `.xcf`
(8-bit layers with names, offsets and opacity), PNG, JPEG, WebP, TIFF, BMP, GIF, SVG
and `.svgz`, JPEG XL, Targa, PNM/PAM, Windows icons, Radiance HDR, OpenEXR, DDS,
QOI, farbfeld, and camera RAW from Sony, Canon, Nikon, Adobe DNG, Fujifilm, Olympus,
Panasonic, Pentax and more. 16-bit and float sources keep their precision.

Like GIMP, the rest goes through a converter already on the machine when one is
installed: HEIC/HEIF (`heif-convert` from libheif), AVIF (`avifdec` from libavif),
PDF and PostScript first pages (`pdftoppm` from poppler), and everything ImageMagick
reads (PCX, Paint Shop Pro, XPM/XBM, SGI, Sun raster, FITS, DICOM, JPEG 2000, ICNS,
GIMP brushes and patterns). Unknown extensions are tried the same way. Without a
converter the error names what to install. The Flatpak sandbox cannot see host
converters, so this path applies to the AppImage and native builds.

```sh
cargo run --release -p emulsion-io --example open_any -- picture.heic layered.xcf
```

### Exporting

Export writes PNG, JPEG, WebP and TIFF (8 or 16-bit), layered PSD and layered
8-bit XCF, and behind **more formats…**: OpenEXR and Radiance HDR (float), BMP,
GIF, Targa, PPM, ICO, QOI and farbfeld, plus AVIF, HEIC, JPEG XL and one-page PDF
when `avifenc`, `heif-enc`, `cjxl` or ImageMagick is installed. Formats without
transparency composite over white. XCF carries the visible top-level layers with
their names and opacity, each rendered with its own adjustments, masks and styles
baked in; hidden layers are left out.

## Editor controls

The right dock keeps **Layers** visible, with its own scrolling list and
**+ Layer**, **Group**, **Duplicate**, and **Delete** actions. **+ Layer** starts
with **Empty layer (transparent)**, also on Ctrl-Shift-N; the rest of the menu
adds adjustments, a solid fill, a LUT or a group. **New transparent canvas** on
the home screen starts a document with one empty layer instead of a white
background. Below the layer list:

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
and returns to the full document. Timeline also holds **Replay drawing**: the
picture played back from its history (every step still undoable and every save),
over the canvas, with a GIF export. Nothing is recorded ahead of time.

Tool buttons and options support Tab navigation and Enter/Space activation.
Focused sliders accept arrow keys, Shift+arrow for larger steps, and Home/End
for their limits. Down opens a tool group's flyout; Escape dismisses it.
Selecting a tool returns keyboard focus to the canvas.

With Zoom selected, hold Shift (or Alt) to zoom out; the magnifier indicator
changes immediately. Hold Shift while drawing a shape for a square or circle.
Escape cancels an active tool gesture. Delete/Backspace removes the last point
of an unfinished polygon or magnetic selection instead of deleting the layer.

Additional canvas shortcuts: Shift-M ellipse marquee, Shift-L polygon lasso,
Alt-L magnetic lasso, Shift-W quick selection, Shift-B smudge, Shift-J liquify,
Shift-U ellipse, Q mask, and Shift-Q grade. U selects a rectangle. These keys
apply while the canvas has focus; text fields retain normal typing behavior.

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

## Cropping

The Crop tool's **delete cropped pixels** option is on by default, as in
Photoshop: applying a crop cuts every unrotated pixel layer and its mask down to
the new canvas in the same undo step. Turn it off to keep layers whole beyond
the edge, where the Move tool can bring them back. Rotated, scaled and smart
layers are never cut, and Canvas Size always keeps every pixel.

## Image size and canvas size

Click the dimensions beside the document name (for example **1920×1080 · 8 bit**)
to open the size panel, with **image size** (scale the picture, constrain
proportions, pixels or percent) and **canvas size** (grow or trim, anchored,
relative amounts, fill new edges). Ctrl-Alt-I and Ctrl-Alt-C open each directly,
and the Crop tool's options carry a **size…** chip.

## Rotating objects

Select a layer and use **Rotate object** in **Properties**. Enter an angle and
choose **Apply**, or use the 90° buttons. Positive angles turn clockwise.
Paths and text stay editable; groups rotate together around their content.
Each rotation can be undone. Locked layers must be unlocked first.

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
