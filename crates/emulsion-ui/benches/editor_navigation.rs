//! Real EditorView pan benchmark. Run with:
//! cargo bench -p emulsion-ui --features layout-bench --bench editor_navigation
//! Set EMULSION_NAVIGATION_BENCH_TARGETED_FIRST=1 to reverse notification order.
//! Set EMULSION_LAYOUT_BENCH_RETAINED_FIRST=1 to reverse layout mode order.

use std::time::Duration;

// As in the UI test harness, isolate before main or any benchmark workers start.
extern "C" fn init_benchmark_environment() {
    // SAFETY: the loader invokes this before main/thread creation.
    unsafe {
        std::env::remove_var("TYPESAFE_API_KEY");
        std::env::set_var(
            "XDG_DATA_HOME",
            std::env::temp_dir().join(format!("emulsion-navigation-bench-{}", std::process::id())),
        );
    }
}

#[used]
#[cfg_attr(
    any(target_os = "linux", target_os = "android", target_os = "freebsd"),
    unsafe(link_section = ".init_array")
)]
#[cfg_attr(
    target_vendor = "apple",
    unsafe(link_section = "__DATA,__mod_init_func")
)]
#[cfg_attr(windows, unsafe(link_section = ".CRT$XCU"))]
static INIT_BENCHMARK_ENVIRONMENT: extern "C" fn() = init_benchmark_environment;

fn main() {
    let mut criterion = criterion::Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3))
        .configure_from_args();
    emulsion_ui::editor::navigation_benchmark::benchmark(&mut criterion);
    criterion.final_summary();
}
