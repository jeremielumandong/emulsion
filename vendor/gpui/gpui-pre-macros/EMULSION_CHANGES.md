# Emulsion changes to gpui-pre-macros 0.3.5

These changes remain under Apache-2.0. Original source notices are retained.

- `src/test.rs`: `#[gpui::test]` teardown calls `TestAppContext::quit` instead
  of `App::quit` inside `update`. `App::quit` only asks the platform to quit, so
  `on_quit` cleanup never ran and `App::shutdown` never cleared windows. Every
  test that opened a window kept its app (and each `VisualTestContext` leaked by
  `into_mut`) alive until the process exited, growing a long serial test run by
  tens of megabytes per test.

Archive and upstream revision information are in `../UPSTREAM.json`.
