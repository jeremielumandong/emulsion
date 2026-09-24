# Troubleshooting

Find the symptom below; when a fix needs evidence, capture the log as described in [files-and-environment.md#logs](files-and-environment.md#logs).

## Window does not open, is black, or crashes on a VM or machine without a GPU

- **Likely cause:** no usable hardware graphics adapter and no software fallback. On Linux, GPUI can fall back to a CPU Vulkan driver only when one is installed; on Windows it falls back to WARP; macOS has no software fallback.
- **What to do:** On Linux, install the distribution's software Vulkan driver (`mesa-vulkan-drivers` on Ubuntu/Debian, `vulkan-swrast` on Arch) and make sure an X11 or Wayland display server and the Vulkan loader are present. On Windows, WARP is selected automatically when hardware is unavailable. To test the software path explicitly, start with `GPUI_FORCE_SOFTWARE_RENDERING=1` (only the exact value `1` counts); this fails instead of silently choosing hardware, and the log records the selected adapter name and whether it is software-rendered. Unset the variable for normal use.
- **Read more:** [rendering.md](rendering.md), [README › Build](../README.md#build).

## Image processing is slow or produces wrong pixels

- **Likely cause:** GPU compute is used for expensive compositing and filters; a driver or device problem affects those operations. A failed shader is disabled for the session and a lost device disables compute until restart.
- **What to do:** Start with `EMULSION_GPU=cpu` to disable image compute entirely (the interface still draws normally). Other values: unset selects hardware compute with performance routing, `force` prefers GPU for every supported operation, `software` forces a CPU adapter for shader validation.
- **Read more:** [gpu-rendering.md › Controls](gpu-rendering.md#controls).

## GPU brush painting shows artifacts

- **Likely cause:** the experimental GPU brush paths, enabled with `EMULSION_GPU_BRUSHES=1` or `EMULSION_GPU_BRUSHES=persistent`. Both are off by default; brush composition uses the CPU otherwise.
- **What to do:** Unset `EMULSION_GPU_BRUSHES` and restart.
- **Read more:** [gpu-rendering.md › Controls](gpu-rendering.md#controls), [gpu-brush-performance.md](gpu-brush-performance.md).

## "no decoder for .heic" (or .avif, .pdf, …) or "could not be converted"

- **Likely cause:** HEIC/HEIF/HIF, AVIF, PDF/PS/EPS/AI and formats Emulsion does not decode itself are opened through a converter found on `PATH`. The error `no decoder for .<ext>; install one of <tools> and Emulsion will open it through that` names the tools it looked for; `<file> could not be converted (<tool>: <message>)` means a tool ran and failed.
- **What to do:** Install the converter for the format: `heif-convert` (libheif) for HEIC/HEIF/HIF, `avifdec` (libavif) for AVIF (`heif-convert` is tried next), `pdftoppm` (poppler) for PDF/PostScript first pages, and ImageMagick's `magick` or `convert` for everything else and unknown extensions. Make sure the binary is on `PATH`. The Flatpak sandbox cannot see host converters; use the AppImage or a native build.
- **Read more:** [README › Opening files](../README.md#opening-files).

## AVIF, HEIC, JPEG XL or PDF is missing from Export, or "no encoder for …"

- **Likely cause:** these formats are written by external tools, and **more formats…** lists only those for which an installed tool is found. The error `no encoder for .<ext>; install one of <tools> to export it` names the candidates; `.<ext> could not be written (<tool>: <message>)` means a tool ran and failed.
- **What to do:** Install `avifenc` for AVIF, `heif-enc` for HEIC, `cjxl` for JPEG XL, or ImageMagick's `magick`/`convert`, which is tried for all four formats and is the only writer for PDF.
- **Read more:** [README › Exporting](../README.md#exporting).

## Save or export says RAW development is still running

- **Likely cause:** the RAW preview is still being developed; Save reports `RAW development is still running. Save when the preview finishes updating.` and Export reports the same with `Export`.
- **What to do:** Wait until the preview finishes updating, then save or export again.
- **Read more:** [README › Developing RAW photos](../README.md#developing-raw-photos).

## "could not restore … sidecar" when opening a RAW photo

- **Likely cause:** the `<filename>.emulsion-raw.json` sidecar beside the photo is invalid or belongs to a different original (`this sidecar belongs to a different RAW original (SHA-256 mismatch)`). Emulsion refuses to open with a bad sidecar rather than silently dropping saved edits.
- **What to do:** If you renamed the photo, rename the sidecar to match (`DSC_1234.NEF` → `DSC_1234.NEF.emulsion-raw.json`). Otherwise restore a valid matching sidecar, or move it aside to open the original without the saved edits.
- **Read more:** [README › Developing RAW photos](../README.md#developing-raw-photos).

## RAW project opens but the RAW layer cannot be edited or exported

- **Likely cause:** an `.ora` project links to the original RAW by path and SHA-256; it does not embed it. The project opens without the original, but changing RAW settings or exporting the linked layer needs it. A modified original is refused with `RAW original has changed (SHA-256 mismatch); restore the original file before developing or exporting`.
- **What to do:** Use **Locate original…** in the RAW panel to reconnect a moved, identical file. Keep the original unmodified. Painting directly on the RAW layer detaches its recipe; paint on another layer instead.
- **Read more:** [README › Developing RAW photos](../README.md#developing-raw-photos).

## "RAW memory budget is in use" or a very large RAW is refused

- **Likely cause:** RAW decoding shares a 128,000,000-pixel budget (`MAX_RAW_PIXELS`) across open tabs and exports. When the budget is taken, the error is `RAW memory budget is in use; wait for development/export to finish or close another RAW document`. A single sensor larger than the budget is refused with `image is <w>×<h>, larger than Emulsion supports`.
- **What to do:** Wait for other RAW development or export jobs to finish, or close other RAW documents, then retry. Files above the budget cannot be opened.
- **Read more:** [README › Developing RAW photos](../README.md#developing-raw-photos).

## Assistant CLI is not detected

- **Likely cause:** Settings shows `<Provider> was not found. Install it with: <command>` when the binary is not found. Emulsion looks for `claude` (Claude Code, `npm install -g @anthropic-ai/claude-code`), `codex` (Codex, `npm install -g @openai/codex`), `opencode` (OpenCode, `npm install -g opencode-ai`) or `kimi` (Kimi Code, `npm install -g @moonshot-ai/kimi-cli`). It searches an explicit path first, then `PATH`, then `/usr/local/bin`, `/opt/homebrew/bin` and, under the home directory, `.local/bin`, `.claude/local`, `.npm-global/bin`, `.local/share/mise/shims`, `.volta/bin`, `.bun/bin`, `.opencode/bin` and `.cargo/bin`. Windows adds `AppData\Roaming\npm`, `AppData\Local\Programs\claude`, `%APPDATA%\npm`, the `nodejs` folder under `ProgramFiles`, `ProgramFiles(x86)` and `LOCALAPPDATA`, and `NVM_SYMLINK`, `VOLTA_HOME\bin` and `FNM_MULTISHELL_PATH`; an extensionless npm shim resolves to its `.exe`/`.com`/`.cmd`/`.bat`/`.ps1` sibling.
- **What to do:** Install the CLI with the command shown, or enter its full path in the Settings **path** field and choose **use path**; **detect again** repeats the search. The path is stored as `cli_path` in `settings.json`.
- **Read more:** [README › An assistant that works on your canvas](../README.md#an-assistant-that-works-on-your-canvas), [README › Build (Windows)](../README.md#build-windows).

## Google image generation reports `free_tier` with `limit: 0`

- **Likely cause:** the API key's Google project has no generation quota for the model. **Save & test** checks key and model access, not generation quota.
- **What to do:** Check billing, prepaid credits and model quotas for that project in Google AI Studio. Waiting for a retry timer does not increase a zero quota.
- **Read more:** [README › Image generation](../README.md#image-generation).

## Cloud API key is saved but a different key is used

- **Likely cause:** environment variables take precedence over keys saved in Settings: `OPENAI_API_KEY` for OpenAI, `GEMINI_API_KEY` then `GOOGLE_API_KEY` for Google. A non-empty variable is used and the saved key is ignored.
- **What to do:** Unset or correct the variable in the environment Emulsion starts from, or rely on it and leave the saved key empty.
- **Read more:** [files-and-environment.md#environment-variables](files-and-environment.md#environment-variables), [README › Image generation](../README.md#image-generation).

## Omarchy theme is not applied

- **Likely cause:** Emulsion (Linux only) reads `omarchy/current/theme/colors.toml` under `$XDG_STATE_HOME` (default `~/.local/state`), then under `$XDG_CONFIG_HOME` (default `~/.config`); a relative XDG value is ignored. Without a valid palette the saved built-in Light/Dark mode is used, and a failed live reload keeps the last valid palette. Choosing Light or Dark, or the theme toggle shortcut, switches back to the built-in palette.
- **What to do:** Check that the file exists at one of those paths and parses, then turn the **◆ omarchy** control in the top bar on again.
- **Read more:** [README › Appearance](../README.md#appearance).

## Shortcuts differ from the README

- **Likely cause:** `keymap.toml` in Emulsion's data directory overrides the defaults; an override replaces every default binding of the same action in that context, and a later binding for the same keys wins.
- **What to do:** Open **Settings › Shortcuts** (Ctrl+K) to see the effective bindings. Use **open keymap file** to edit the overrides (defaults are listed there, commented out) and **reload** to apply them; the panel reports `Reloaded: N custom binding(s).`
- **Read more:** [README › Editor controls](../README.md#editor-controls).

## Several files selected in a file manager, only one opens

- **Likely cause:** the command line is `emulsion [FILE]`: one file path, or `mcp-serve`, `--version` or `--help`. Extra arguments are not read.
- **What to do:** Open files one at a time, or open the rest from inside Emulsion.
- **Read more:** [files-and-environment.md#command-line](files-and-environment.md#command-line).

## Where are my logs

- **Likely cause:** Emulsion writes its log to standard error; the default level is `info` and the filter follows the standard `RUST_LOG`-style environment filter.
- **What to do:** Start Emulsion from a terminal and read stderr, or redirect it to a file.
- **Read more:** [files-and-environment.md#logs](files-and-environment.md#logs).

<!-- Sources rechecked 2026-09-24: docs/rendering.md:3-34,63-67; docs/gpu-rendering.md:62-78,111-117; vendor/gpui/gpui-pre-wgpu/src/wgpu_context.rs:336-337; crates/emulsion-gpu/src/lib.rs:22-55; crates/emulsion-io/src/external.rs:53-70,80-123,140-147,189-240,256-290,302-353; crates/emulsion-io/src/export.rs:136-152; crates/emulsion-ui/src/workspace.rs:1126-1128,1227-1230; crates/emulsion-io/src/raw_settings.rs:201,224,241-243; crates/emulsion-io/src/raw.rs:24-35,52-58,141; crates/emulsion-io/src/lib.rs:57-62; crates/emulsion-ui/src/editor/raw_panel.rs:544,672; crates/emulsion-assistant/src/provider.rs:47-206; crates/emulsion-ui/src/settings_screen.rs:257-267,384-392,552-562; crates/emulsion-io/src/settings.rs:115,197,238-259; crates/emulsion-io/src/recent.rs:21-27; crates/emulsion-ui/src/theme/omarchy.rs:140-183; crates/emulsion-ui/src/actions.rs:500-561; crates/emulsion-app/src/main.rs:15-45; README.md:127-136,190-193,224-230,300-338,344-349,431-446 -->
