//! Emulsion binary. Images open in the viewer; `--edit` opens the editor.
//! `emulsion mcp-serve`
//! runs the stdio MCP server that a coding CLI attaches to.

// Ship a Windows GUI executable without a console. Inherited pipes still work
// for mcp-serve; debug builds retain their console for development diagnostics.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use emulsion_ui::{Workspace, actions, app_state, theme};
use gpui_kit::component::Root;
use gpui_kit::*;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

mod launch;
mod memory;

fn main() -> anyhow::Result<()> {
    let memory_configured = memory::configure();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with_writer(std::io::stderr)
        .init();
    if !memory_configured {
        tracing::warn!("Large image allocation policy could not be configured");
    }

    let (files, edit) = match launch::parse(std::env::args_os().skip(1))? {
        launch::Launch::Mcp => {
            if let Err(error) = std::thread::Builder::new()
                .name("image-compute".into())
                .spawn(emulsion_gpu::initialize)
            {
                tracing::warn!(%error, "Compute initialization thread unavailable; using CPU");
            }
            return emulsion_mcp::serve_stdio();
        }
        launch::Launch::Version => {
            println!("emulsion {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        launch::Launch::Help => {
            println!(
                "usage: emulsion [FILE...]\n       emulsion --edit [FILE...]\n       emulsion mcp-serve\n       emulsion --version\n\nSupported image files open in the lightweight viewer. Use --edit to open the full editor."
            );
            return Ok(());
        }
        launch::Launch::Gui { files, edit } => (files, edit),
    };
    run_application(files, edit);
    // The window owners have released their sessions. Allow child shutdown and
    // workspace removal to finish before background threads are terminated.
    emulsion_assistant::storage::wait_for_cleanup(std::time::Duration::from_secs(5));
    Ok(())
}

fn run_application(files: Vec<PathBuf>, edit: bool) {
    let viewer =
        !edit && !files.is_empty() && files.iter().all(|p| emulsion_ui::image_viewer::supports(p));
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::install(cx);
            if viewer {
                app_state::install_viewer(cx);
            } else {
                app_state::install(cx);
            }
            // Apply the saved palette and corner style before the first frame.
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
                let groups = if viewer || files.is_empty() {
                    vec![files]
                } else {
                    files.into_iter().map(|path| vec![path]).collect()
                };
                for files in groups {
                    let bounds = cx.update(|cx| {
                        Bounds::centered(
                            None,
                            if viewer {
                                size(px(1000.), px(720.))
                            } else {
                                size(px(1440.), px(900.))
                            },
                            cx,
                        )
                    });
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
                        if viewer {
                            let view = cx.new(|cx| {
                                emulsion_ui::image_viewer::ImageViewer::new(files, window, cx)
                            });
                            cx.new(|cx| Root::new(view, window, cx))
                        } else {
                            let ws = cx.new(|cx| {
                                if files.is_empty() {
                                    Workspace::new(window, cx)
                                } else {
                                    Workspace::new_for_file(window, cx)
                                }
                            });
                            if let Some(path) = files.into_iter().next() {
                                ws.update(cx, |ws, cx| ws.open_path(path, window, cx));
                            }
                            cx.new(|cx| Root::new(ws, window, cx))
                        }
                    })
                    .expect("failed to open window");
                }
            })
            .detach();
        });
}

#[cfg(test)]
mod asset_tests {
    use gpui_kit::AssetSource;

    #[test]
    fn packaged_navigation_icons_are_available_offline() {
        let assets = gpui_kit::assets::AllAssets;
        for icon in [
            "clock",
            "pin",
            "image",
            "workflow",
            "layout-grid",
            "list",
            "library",
            "brush",
            "layout-template",
            "folder-plus",
            "panel-left",
        ] {
            let path = format!("icons/{icon}.svg");
            let bytes = assets.load(&path).unwrap().unwrap();
            assert!(
                std::str::from_utf8(&bytes).unwrap().contains("<svg"),
                "{path}"
            );
        }
    }
}
