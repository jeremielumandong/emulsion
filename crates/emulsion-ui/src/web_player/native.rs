use super::*;
use wry::{
    WebView, WebViewBuilder,
    dpi::{LogicalPosition, LogicalSize},
};

pub(super) struct PlatformPlayer {
    view: WebView,
}

fn rect(bounds: Bounds<Pixels>) -> wry::Rect {
    wry::Rect {
        position: LogicalPosition::new(
            f64::from(f32::from(bounds.origin.x)),
            f64::from(f32::from(bounds.origin.y)),
        )
        .into(),
        size: LogicalSize::new(
            f64::from(f32::from(bounds.size.width).max(1.0)),
            f64::from(f32::from(bounds.size.height).max(1.0)),
        )
        .into(),
    }
}

impl PlatformPlayer {
    pub(super) fn new(
        url: &str,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        _cx: &mut App,
    ) -> anyhow::Result<Self> {
        use anyhow::Context;
        let allowed = url.to_owned();
        let view = WebViewBuilder::new()
            .with_url(url)
            .with_bounds(rect(bounds))
            .with_incognito(true)
            .with_devtools(false)
            .with_autoplay(true)
            .with_navigation_handler(move |uri| uri == allowed || uri.starts_with("https://www.youtube-nocookie.com/embed/") || uri.starts_with("https://www.youtube.com/embed/"))
            .with_permission_handler(|_| wry::PermissionResponse::Deny)
            .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
            .with_download_started_handler(|_, _| false)
            .build_as_child(window)
            .context("Unable to start the system video player. Windows requires the Microsoft Edge WebView2 Runtime; macOS uses its built-in WKWebView.")?;
        Ok(Self { view })
    }
    pub(super) fn update_bounds(&mut self, bounds: Bounds<Pixels>) {
        let _ = self.view.set_bounds(rect(bounds));
    }
    pub(super) fn latest_frame(&self) -> Option<Arc<RenderImage>> {
        None
    }
    pub(super) fn error(&self) -> Option<String> {
        None
    }
    // The native child view receives OS input directly.
    pub(super) fn pointer_move(&self, _x: f32, _y: f32) {}
    pub(super) fn pointer_button(&self, _button: u8, _pressed: bool, _x: f32, _y: f32) {}
    pub(super) fn scroll(&self, _dx: f32, _dy: f32, _x: f32, _y: f32) {}
    pub(super) fn key(&self, _key: &str, _pressed: bool, _modifiers: u32) {}
}
