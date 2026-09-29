//! Native EditorView benchmark. Runs only against an isolated, disposable store.

extern "C" fn isolate() {
    // SAFETY: the loader calls this before main and worker thread creation.
    unsafe {
        std::env::remove_var("TYPESAFE_API_KEY");
        std::env::set_var(
            "XDG_DATA_HOME",
            std::env::temp_dir().join(format!("emulsion-canvas-bench-{}", std::process::id())),
        );
    }
}

#[used]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".init_array"))]
#[cfg_attr(
    target_vendor = "apple",
    unsafe(link_section = "__DATA,__mod_init_func")
)]
#[cfg_attr(windows, unsafe(link_section = ".CRT$XCU"))]
static ISOLATE: extern "C" fn() = isolate;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();
    let path = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    emulsion_ui::editor::canvas_benchmark::run(path.as_deref())
}
