//! Emulsion binary. `emulsion [FILE]` opens the editor; `emulsion mcp-serve`
//! runs the stdio MCP server that a coding CLI attaches to.

use emulsion_ui::{Workspace, actions, theme};
use gpui_kit::component::{Root, Theme};
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
            "mcp-serve" => return emulsion_mcp::serve_stdio(),
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
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        theme::install(cx);
        actions::bind(cx);
        // The design is square: no rounded corners except avatars and dots.
        let t = Theme::global_mut(cx);
        t.radius = px(0.);
        t.radius_lg = px(0.);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        cx.spawn(async move |cx| {
            let bounds = cx.update(|cx| Bounds::centered(None, size(px(1440.), px(900.)), cx));
            let opts = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Emulsion".into()),
                    ..Default::default()
                }),
                app_id: Some("app.emulsion.Emulsion".into()),
                ..Default::default()
            };
            cx.open_window(opts, |window, cx| {
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
