# Files, folders and environment

Where Emulsion keeps its settings, shortcuts, recovery copies and downloads,
how to capture its logs, and which environment variables change its behaviour.
Feature documentation lives in the [README](../README.md); this page only
covers what is on disk and in the environment.

## Command line

```
usage: emulsion [FILE]
       emulsion mcp-serve
       emulsion --version
```

That text is what `emulsion --help` (or `-h`) prints. The forms are:

| Invocation | Effect |
| --- | --- |
| `emulsion` | Open the editor on the Home screen. |
| `emulsion FILE` | Open the editor with `FILE` loaded. |
| `emulsion mcp-serve` | Run the stdio MCP server that a coding CLI attaches to; no window. |
| `emulsion --version` / `-V` | Print `emulsion <version>` and exit. |
| `emulsion --help` / `-h` | Print the usage text above and exit. |

Any other argument starting with `-` is an error (`unknown option`). Only the
first positional argument is read; further arguments are ignored. The desktop
entry launches `emulsion %F`, so a multi-file selection in a file manager opens
only the first file.

## Data directory

Everything Emulsion stores for you lives in one folder, resolved in this order:

1. `$XDG_DATA_HOME/emulsion` when `XDG_DATA_HOME` is set to an absolute path
   (on every platform, including Windows).
2. Windows only: `%LOCALAPPDATA%\Emulsion\.local\share\emulsion` when that folder
   already exists (an earlier Windows layout; existing installations keep it).
3. `<home>/.local/share/emulsion`, where `<home>` is the user's profile folder
   (`~` on Linux and macOS, the native profile on Windows even when `HOME` is
   unset).
4. If no absolute home folder can be determined, `<temp dir>/emulsion`.

The working directory is never used, so launching from a shortcut, a file
association or a terminal all share the same folder.

AI model files resolve their folder separately (`crates/emulsion-ai`):
`$EMULSION_MODELS_DIR` if set and non-empty, else `$XDG_DATA_HOME/emulsion/models`
if `XDG_DATA_HOME` is set and non-empty, else `$HOME/.local/share/emulsion/models`.
This differs from the list above in two ways: it does not check the legacy
Windows `%LOCALAPPDATA%` folder, and when `HOME` is unset it falls back to `.`
(the working directory) rather than the profile folder. On Windows, where `HOME`
is usually unset, set `EMULSION_MODELS_DIR` or `XDG_DATA_HOME` if you need the
models folder to be in a predictable place.

## What is stored there

Paths are relative to the data directory. "Copy" means copying to another
machine or another user account.

| Path | Purpose | Safe to delete? | Safe to copy between machines? |
| --- | --- | --- | --- |
| `settings.json` | All preferences: assistant provider and CLI path, model alias, workspace layouts and presets, layer-style defaults, theme choices, starred Home files, and the API keys `jev_api_key`, `openai_image_key` and `google_image_key`. Written as pretty JSON, owner-only (`0600`) on Unix. | Yes; the next launch starts from defaults. You lose saved keys and layouts. | Yes, but it contains API keys in plain text and absolute paths (starred files, CLI path). Review before sharing. |
| `settings.json.bak`, `recent.json.bak` | Written only when the original fails to parse: the unreadable bytes are kept here (owner-only) so a later save cannot silently destroy them. | Yes. | Not needed. |
| `recent.json` | The Recent list on the Home screen (up to 24 entries: path, open time, summary). Entries whose file no longer exists are dropped on load. | Yes; the list starts empty. | Only useful if the same absolute paths exist there. |
| `recent-imports.json` | Windows only: which older per-launch-folder recent lists have already been merged in. | Yes; the merge may repeat. | No. |
| `keymap.toml` | Your keyboard shortcut overrides, one `[workspace]`, `[canvas]` and `[panel]` table. Created from a commented template when you open it from Settings. Read once at launch, so restart after editing. | Yes; defaults return. | Yes; it is plain TOML. macOS defaults add `cmd-` variants automatically. |
| `sessions/` | Scratch folders (`<pid>-<timestamp>-<serial>`) for each assistant conversation: the generated MCP configuration for the CLI, a scoped Codex home, and so on. Removed when the conversation ends; stale ones are removed at the next launch. | Yes, while Emulsion is not running. | No; contents are per-run and hold the relay token. |
| `autosave/` | Crash-recovery copies: `<name>-<pid>-<time>-<editor>-<generation>.ora`, a full project with its history graph. Written at most once a minute while a document is modified and unsaved, and deleted after a successful save or when the work is discarded. The Home screen lists copies left by other sessions and offers Open or Discard. | Yes, but any copy still listed on Home is unsaved work. | Yes; they are ordinary `.ora` projects. |
| `thumbs/` | Cache of file preview thumbnails, `<hash>.png`, keyed by the file's path, size, modification time, sidecar content and requested size. | Yes; it is rebuilt. | Not needed. |
| `recipes/` | Recipes you saved or imported, `<name>.recipe.toml`. | Yes; you lose those recipes. Built-in library recipes are not stored here. | Yes; plain TOML. |
| `luts/` | `.cube` files that the assistant fetched from a URL through the `lut_file` argument. A URL is fetched once; the file is reused after that. | Yes; a later URL import downloads again. | Yes. |
| `brush-library.json`, `brushes/assets/`, `brushes/sources/`, `brushes/textures/` | The brush library manifest with its imported shape/grain PNGs (`assets/<sha256>.png`), the original imported brush packages (`sources/<sha256>.archive`), and textures saved by id (`textures/<id>.png`). | Deleting removes imported and edited brushes; built-in brushes remain. | Yes, but copy the manifest and the `brushes/` folder together. |
| `raw-camera-defaults/` | Explicit per-camera RAW development defaults, one `<sha256>.json` per camera make and model. | Yes; those cameras return to as-shot defaults. | Yes. |
| `lensfun/` | The lensfun lens-profile database, downloaded from GitHub when requested from the Settings models screen or the assistant (about 5 MB of XML, CC-BY-SA 3.0). Counts as installed once at least half the files are present. | Yes; it can be downloaded again. | Yes. |
| `models/<id>/` | Local AI model files, downloaded only when asked for. Sizes are checked after download. Location can be overridden with `EMULSION_MODELS_DIR`. | Yes; they are downloaded again on demand. | Yes, but they are large. |

API keys are stored unencrypted. Emulsion restricts `settings.json` to your
user on Unix, but anyone who can read the file, any backup of it, or a copy you
send with a bug report can read the keys. Treat `settings.json` as a credential
file: exclude it from shared archives, or remove the `*_key` values first.

## Sidecars next to your files

Opening a RAW photo directly and saving writes its development settings beside
the original as `<file>.emulsion-raw.json`; the RAW file itself is never
changed. What the sidecar holds, how it is restored, and what happens when it
is renamed or mismatched are described in the README under
[Developing RAW photos](../README.md#developing-raw-photos). When you back up
or move a photo, keep its sidecar with it.

## Logs

Emulsion writes its log to standard error only; there is no log file. The
level is controlled by `RUST_LOG` using the `tracing_subscriber` `EnvFilter`
syntax; when `RUST_LOG` is unset or invalid the level is `info`. Assistant
sessions, the tool relay, GPU selection and file errors are logged here.

Linux and macOS, from a terminal:

```sh
RUST_LOG=debug emulsion 2> emulsion.log
```

A narrower filter keeps the file small, for example
`RUST_LOG=info,emulsion_assistant=debug`. When Emulsion is started from a
launcher rather than a terminal, stderr goes wherever the desktop session
sends it (for example `journalctl --user` on systemd desktops).

Windows: release builds are built as a GUI executable with no console window,
so nothing is shown when you double-click `emulsion.exe`. The source notes that
inherited pipes still work, which is how `mcp-serve` communicates, so
redirecting from a terminal (`emulsion.exe 2> emulsion.log` in cmd, or
`$env:RUST_LOG='debug'; .\emulsion.exe 2> emulsion.log` in PowerShell) is
expected to capture the log; this has not been verified for this page. The
reliable alternative is a debug build, which keeps its console:

```powershell
$env:RUST_LOG = 'debug'
cargo run --locked -p emulsion-app 2> emulsion.log
```

Before attaching a log to a bug report, remove file paths and any credential
that may appear in it.

## Environment variables

Set these before starting Emulsion; they are read at launch.

| Variable | Values | Effect | Source |
| --- | --- | --- | --- |
| `RUST_LOG` | `EnvFilter` directive, e.g. `debug`, `info,emulsion_mcp=trace` | Log level and per-module filters; default `info`. The name is `tracing_subscriber`'s default and does not appear literally in Emulsion's source. | `crates/emulsion-app/src/main.rs` (`EnvFilter::try_from_default_env`) |
| `XDG_DATA_HOME` | Absolute path | Data directory becomes `$XDG_DATA_HOME/emulsion` on every platform. | `crates/emulsion-io/src/recent.rs` |
| `EMULSION_GPU` | `cpu`, `force`, `software`, unset | `cpu` disables GPU image compute; `force` prefers GPU for every supported operation regardless of measured cost; `software` does the same on a CPU adapter for shader validation. GPUI window rendering is unaffected. See [GPU image processing](gpu-rendering.md#controls). | `crates/emulsion-gpu/src/lib.rs`, `context.rs` |
| `EMULSION_GPU_BRUSHES` | `1`, `persistent` | Opt in to experimental GPU brush composition (`1`), or the persistent GPU brush backend (`persistent`). Ignored when GPU compute is unavailable. | `crates/emulsion-gpu/src/lib.rs` |
| `EMULSION_MODELS_DIR` | Absolute path | Folder for AI model downloads instead of `<data dir>/models`. | `crates/emulsion-ai/src/models.rs` |
| `EMULSION_RETAINED_LAYOUT` | `1`, `0` | Launch override for the Settings "layout reuse" option: `1` forces it on, `0` off, without changing the saved preference. Any other value is ignored. | `crates/emulsion-ui/src/app_state.rs` |
| `OPENAI_API_KEY` | Key | Used for OpenAI image generation in preference to `openai_image_key` in `settings.json`. | `crates/emulsion-io/src/settings.rs` |
| `GEMINI_API_KEY`, `GOOGLE_API_KEY` | Key | Used for Google image generation in preference to `google_image_key`; `GEMINI_API_KEY` is checked first. | `crates/emulsion-io/src/settings.rs` |
| `TYPESAFE_API_KEY` | Key | Used for Jev in preference to `jev_api_key` in `settings.json`. | `crates/emulsion-io/src/settings.rs` |
| `CODEX_HOME` | Path | Where your own Codex sign-in and config are read from when the assistant builds its scoped Codex home (default `~/.codex`). | `crates/emulsion-assistant/src/launch.rs` |
| `XDG_CONFIG_HOME` | Path | Where your own OpenCode config is read from (default `~/.config`); also the fallback root for the Omarchy theme below. | `crates/emulsion-assistant/src/launch.rs`, `crates/emulsion-ui/src/theme/omarchy.rs` |
| `XDG_STATE_HOME` | Absolute path | Linux: first root searched for `omarchy/current/theme/colors.toml` (default `~/.local/state`), then `XDG_CONFIG_HOME`. See [Appearance](../README.md#appearance). | `crates/emulsion-ui/src/theme/omarchy.rs`, `crates/emulsion-io/src/settings.rs` |
| `GPUI_FORCE_SOFTWARE_RENDERING` | `1` | Force the software window renderer (Linux/wgpu and Windows); fails rather than falling back to hardware. Independent of `EMULSION_GPU`. See [Rendering and virtual machines](rendering.md). | `vendor/gpui/gpui-pre-wgpu/src/wgpu_context.rs` |
| `EMULSION_APPIMAGE` | Absolute path | Installer and launcher wrapper: where the AppImage is installed (default `~/Applications/Emulsion.AppImage`). | `scripts/install-appimage.sh` |
| `EMULSION_KEEP_BACKUPS` | Integer | Installer: how many `.bak-*` copies of a previous AppImage to keep (default 2). | `scripts/install-appimage.sh` |
| `EMULSION_TOOLS_DIR` | Path | AppImage build script: cache for appimagetool and the runtime (default `~/.cache/emulsion/tools`). | `scripts/build-appimage.sh` |

`EMULSION_RELAY` and `EMULSION_TOKEN` are set by Emulsion itself in the
environment of the `mcp-serve` child it launches (the relay address on
`127.0.0.1` and a per-session token); `EMULSION_EXE` is read by the app to
locate the `mcp-serve` executable instead of its own path and is set by the test
harness. Do not set any of the three by hand.

Test and benchmark variables, each documented where it is used:
`EMULSION_REQUIRE_GPU_TESTS` in [GPU image processing](gpu-rendering.md#verification);
`EMULSION_RAW_CORPUS` in the [RAW corpus notes](../crates/emulsion-io/tests/fixtures/RAW-CORPUS.md);
`EMULSION_NIKON_HE_FILE` in [Nikon HE support](nikon-he.md). The remaining
`EMULSION_*` names in the source (`EMULSION_DRAWING_WORKFLOW_ARTIFACT`,
`EMULSION_COMPOSITOR_BENCH_CASES`, `EMULSION_LAYOUT_BENCH_RETAINED_FIRST`,
`EMULSION_NAVIGATION_BENCH_TARGETED_FIRST`, `EMULSION_TEST_EXPECTED_DATA_DIR`,
`EMULSION_TEST_CONVERTER_CHILD`) are internal to tests and benches.

## Installer options

`scripts/install-appimage.sh` installs the newest AppImage from
`target/appimage` into `~/Applications`, writes a launcher command at
`~/.local/bin/emulsion`, a desktop entry with file associations, and icons.

| Argument | Effect |
| --- | --- |
| *(none)* | Install the newest `target/appimage/Emulsion-*.AppImage`. |
| `path/to/Emulsion-x.y.z-x86_64.AppImage` | Install that file instead. Cannot be combined with `--build`. |
| `--build` | Run `scripts/build-appimage.sh` first, then install the result. |
| `--force` | Reinstall even when the installed file is byte-identical. |
| `--stop-running` | Stop a running installed Emulsion (and its assistant sessions) before replacing it. Without this flag the install refuses, because a running copy may hold unsaved edits. Development builds under `target/` are never stopped. |
| `--uninstall` | Remove the AppImage, its backups, the launcher command, the desktop entry and the icons. |
| `-h`, `--help` | Print the script's header comment. |

The installer never touches `settings.json`, `recent.json` or anything else in
the data directory except `sessions/`, which it deletes because those folders
are regenerated on demand. Uninstalling leaves the whole data directory in
place. The previous AppImage is kept as `Emulsion.AppImage.bak-<date>`, with
`EMULSION_KEEP_BACKUPS` copies retained.

<!-- Sources rechecked 2026-09-24:
crates/emulsion-app/src/main.rs:6,14-18,20-46; packaging/linux/app.emulsion.Emulsion.desktop:7;
crates/emulsion-io/src/recent.rs:7,18-44,46-47,96-140,142-148;
crates/emulsion-ai/src/models.rs:1-8,239-258; crates/emulsion-io/src/lib.rs:237-282;
crates/emulsion-io/src/settings.rs:97-165,167-186,228-230,239-260,287-302;
crates/emulsion-ui/src/actions.rs:485-502,504-524,527-552,556-574; crates/emulsion-ui/src/settings_screen.rs:578-585;
crates/emulsion-ui/src/app_state.rs:28-35,54-64; crates/emulsion-assistant/src/storage.rs:23-42,49-73,87;
crates/emulsion-ui/src/editor/history.rs:8-10,110-111,162-164,198-227,229-252,266,294-303;
crates/emulsion-ui/src/workspace.rs:2158-2184; crates/emulsion-ui/src/home.rs:1245-1270;
crates/emulsion-io/src/thumb.rs:147-151; crates/emulsion-recipes/src/store.rs:42,91,113;
crates/emulsion-ui/src/editor/recipes.rs:66-68; crates/emulsion-mcp/src/exec.rs:3657-3677;
crates/emulsion-io/src/brush_library.rs:59-68,782-785; crates/emulsion-io/src/brushset.rs:409-411;
crates/emulsion-io/src/raw_settings.rs:387-400,415; crates/emulsion-io/src/lensfun.rs:17-19,79-115;
crates/emulsion-gpu/src/lib.rs:23-53,106; crates/emulsion-gpu/src/context.rs:59-61;
crates/emulsion-assistant/src/launch.rs:99-101,142-144,331; crates/emulsion-ui/src/theme/omarchy.rs:140-153;
crates/emulsion-mcp/src/relay.rs:23-24,83-86,122-127,267-271; crates/emulsion-mcp/src/server.rs:160-166;
crates/emulsion-ui/src/assistant.rs:640,655,663-666,676; crates/emulsion-ui/src/tests.rs:168;
scripts/install-appimage.sh:14-16,26-28,35-38,50-60,66,113-126,134-147,150-153,176-179,187-190,196-203;
scripts/build-appimage.sh:20-26; README.md:199-200,227-248,295,320-327 (working-tree copy);
vendor/gpui/gpui-pre-wgpu/src/wgpu_context.rs:336-337; tracing-subscriber EnvFilter::DEFAULT_ENV = "RUST_LOG";
docs/gpu-rendering.md:62-73,86-96; docs/rendering.md:36-67; docs/nikon-he.md:19-24;
crates/emulsion-io/tests/raw_corpus.rs:10-13; crates/emulsion-io/tests/nikon_he.rs:5-8;
.github/workflows/ci.yml:112-116,134 -->
