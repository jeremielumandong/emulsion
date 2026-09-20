//! Emulsion binary. `emulsion [FILE]` opens the editor; `emulsion mcp-serve`
//! runs the stdio MCP server that a coding CLI attaches to.

// Ship a Windows GUI executable without a console. Inherited pipes still work
// for mcp-serve; debug builds retain their console for development diagnostics.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use emulsion_ui::{Workspace, actions, app_state, theme};
use gpui_kit::component::Root;
use gpui_kit::*;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with_writer(std::io::stderr)
        .init();

    let mut args = std::env::args().skip(1);
    let file = match args.next() {
        None => None,
        Some(cmd) => match cmd.as_str() {
            "mcp-serve" => {
                if let Err(error) = std::thread::Builder::new()
                    .name("image-compute".into())
                    .spawn(emulsion_gpu::initialize)
                {
                    tracing::warn!(%error, "Compute initialization thread unavailable; using CPU");
                }
                return emulsion_mcp::serve_stdio();
            }
            "--version" | "-V" => {
                println!("emulsion {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" | "-h" => {
                println!(
                    "usage: emulsion [FILE]\n       emulsion mcp-serve\n       emulsion --version"
                );
                return Ok(());
            }
            other if other.starts_with('-') => anyhow::bail!("unknown option: {other}"),
            path => Some(PathBuf::from(path)),
        },
    };

    run_editor(file);
    Ok(())
}

fn run_editor(file: Option<PathBuf>) {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::install(cx);
            app_state::install(cx);
            // Device discovery and shader work must not block window creation.
            cx.background_executor()
                .spawn(async { emulsion_gpu::initialize() })
                .detach();
            // Also squares gpui-kit's corners: the design has none but avatars and dots.
            theme::apply_saved(cx);
            #[cfg(target_os = "linux")]
            theme::watch_omarchy(cx);
            actions::bind(cx);
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            cx.spawn(async move |cx| {
                let bounds = cx.update(|cx| Bounds::centered(None, size(px(1440.), px(900.)), cx));
                // The title bar is ours on every platform: it moves the window,
                // and on Linux and Windows carries minimise, maximise and close.
                let opts = WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Emulsion".into()),
                        ..gpui_kit::component::TitleBar::title_bar_options()
                    }),
                    app_owns_titlebar_drag: true,
                    window_decorations: Some(if cfg!(target_os = "linux") {
                        WindowDecorations::Client
                    } else {
                        WindowDecorations::Server
                    }),
                    app_id: Some("app.emulsion.Emulsion".into()),
                    ..Default::default()
                };
                cx.open_window(opts, |window, cx| {
                    if let Some(specs) = window.gpu_specs() {
                        tracing::info!(
                            device = %specs.device_name,
                            software = specs.is_software_emulated,
                            "graphics renderer initialized"
                        );
                    }
                    let ws = cx.new(|cx| Workspace::new(window, cx));
                    if let Some(path) = file {
                        ws.update(cx, |ws, cx| ws.open_path(path, window, cx));
                    }
                    cx.new(|cx| Root::new(ws, window, cx))
                })
                .expect("failed to open window");
            })
            .detach();
        });
}
