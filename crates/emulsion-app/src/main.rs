//! Emulsion binary. `emulsion` opens the editor; `emulsion mcp-serve` runs the
//! stdio MCP server that a coding CLI attaches to.

use gpui_kit::component::{Root, Theme};
use gpui_kit::*;
use tracing_subscriber::EnvFilter;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with_writer(std::io::stderr)
        .init();

    let mut args = std::env::args().skip(1);
    if let Some(cmd) = args.next() {
        return match cmd.as_str() {
            "mcp-serve" => emulsion_mcp::serve_stdio(),
            "--version" | "-V" => {
                println!("emulsion {}", env!("CARGO_PKG_VERSION"));
                Ok(())
            }
            other => anyhow::bail!("unknown subcommand: {other}"),
        };
    }

    run_editor();
    Ok(())
}

fn run_editor() {
    gpui_kit::application().run(|cx| {
        gpui_kit::init(cx);
        emulsion_ui::theme::install(cx);
        // The design is square: no rounded corners except avatars and status dots.
        let theme = Theme::global_mut(cx);
        theme.radius = px(0.);
        theme.radius_lg = px(0.);

        cx.spawn(async move |cx| {
            let bounds = cx.update(|cx| Bounds::centered(None, size(px(1440.), px(900.)), cx));
            let opts = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Emulsion".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            cx.open_window(opts, |window, cx| {
                let shell = cx.new(emulsion_ui::shell::EditorShell::new);
                cx.new(|cx| Root::new(shell, window, cx))
            })
            .expect("failed to open window");
        })
        .detach();
    });
}
