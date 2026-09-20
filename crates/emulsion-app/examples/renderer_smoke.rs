//! Real-window renderer smoke test, independent of editor state and user files.
//!
//! Run with `GPUI_FORCE_SOFTWARE_RENDERING=1` and `--require-software` under
//! Xvfb + Mesa lavapipe on Linux, or in an interactive Windows session for WARP.
//! Without the flag, it also exercises automatic adapter selection. This checks
//! window creation and two draw/present cycles, not screenshot pixel correctness.

use gpui_kit::{prelude::*, *};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

static RENDERS: AtomicUsize = AtomicUsize::new(0);
static COMPLETED: AtomicBool = AtomicBool::new(false);

struct RendererSmoke;

impl Render for RendererSmoke {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        if RENDERS.fetch_add(1, Ordering::SeqCst) == 0 {
            // Frame callbacks run at the next platform frame boundary. Nesting
            // a second callback lets the refreshed scene actually reach present
            // before success, rather than quitting during initial view creation.
            window.on_next_frame(|window, _| {
                window.refresh();
                window.on_next_frame(|_, cx| {
                    assert!(RENDERS.load(Ordering::SeqCst) >= 2, "second draw missing");
                    COMPLETED.store(true, Ordering::SeqCst);
                    cx.quit();
                });
            });
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .bg(rgb(0x182030))
            .text_color(rgb(0xffffff))
            .child("Emulsion renderer smoke")
            .child(
                div()
                    .size(px(96.))
                    .rounded_xl()
                    .border_2()
                    .border_color(rgb(0xffffff))
                    .bg(rgb(0x397ac4))
                    .shadow_lg(),
            )
    }
}

fn main() -> anyhow::Result<()> {
    let mut require_software = false;
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--require-software" => require_software = true,
            _ => anyhow::bail!("unknown argument: {argument}; expected --require-software"),
        }
    }
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();
    // An independent watchdog also catches a blocked renderer/event loop. The
    // process exits normally when main returns; there is no need to join it.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(30));
        eprintln!("renderer smoke timed out before successful shutdown");
        std::process::exit(1);
    });

    gpui_kit::application().run(move |cx| {
        let bounds = Bounds::centered(None, size(px(400.), px(300.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            move |window, cx| {
                let specs = window
                    .gpu_specs()
                    .expect("renderer did not report an adapter");
                eprintln!("renderer smoke adapter: {specs:?}");
                assert!(
                    !require_software || specs.is_software_emulated,
                    "software renderer required, but a hardware adapter was selected"
                );
                cx.new(|_| RendererSmoke)
            },
        )
        .expect("renderer smoke could not open a real window");
        cx.activate(true);
    });
    anyhow::ensure!(
        COMPLETED.load(Ordering::SeqCst),
        "renderer exited before completing two draw/present cycles"
    );
    println!("Renderer smoke passed: two draw/present cycles completed.");
    Ok(())
}
