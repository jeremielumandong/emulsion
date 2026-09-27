//! In-app playback using the operating system's web engine. No browser engine is bundled.
use gpui_kit::gpui::{App, Bounds, Pixels, RenderImage, Window};
use std::sync::Arc;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(any(target_os = "windows", target_os = "macos"))]
mod native;
#[cfg(target_os = "linux")]
use linux::PlatformPlayer;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use native::PlatformPlayer;

#[cfg(target_os = "linux")]
pub use linux::register_linux_helper;

pub struct Player(PlatformPlayer);

impl Player {
    pub fn new(
        url: &str,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> anyhow::Result<Self> {
        // The application serves a fixed, trusted player document on loopback. Never
        // turn document metadata into a general-purpose browser or a local-file viewer.
        anyhow::ensure!(
            trusted_url(url),
            "The embedded player requires a local presentation URL"
        );
        Ok(Self(PlatformPlayer::new(url, bounds, window, cx)?))
    }

    pub fn update_bounds(&mut self, bounds: Bounds<Pixels>) {
        self.0.update_bounds(bounds);
    }
    pub fn latest_frame(&self) -> Option<Arc<RenderImage>> {
        self.0.latest_frame()
    }
    pub fn error(&self) -> Option<String> {
        self.0.error()
    }
    pub fn pointer_move(&self, x: f32, y: f32) {
        self.0.pointer_move(x, y);
    }
    /// Buttons follow GDK numbering: left 1, middle 2, right 3.
    pub fn pointer_button(&self, button: u8, pressed: bool, x: f32, y: f32) {
        self.0.pointer_button(button, pressed, x, y);
    }
    pub fn scroll(&self, dx: f32, dy: f32, x: f32, y: f32) {
        self.0.scroll(dx, dy, x, y);
    }
    /// Modifiers: shift=1, control=4, alt=8, super=64.
    pub fn key(&self, key: &str, pressed: bool, modifiers: u32) {
        self.0.key(key, pressed, modifiers);
    }
}

fn trusted_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://127.0.0.1:") else {
        return false;
    };
    let Some((port, path)) = rest.split_once('/') else {
        return false;
    };
    port.parse::<u16>().is_ok_and(|port| port != 0) && !path.contains(['\r', '\n', '#'])
}

#[cfg(test)]
mod tests {
    #[test]
    fn player_only_accepts_loopback_documents() {
        assert!(super::trusted_url("http://127.0.0.1:12345/player/token"));
        for url in [
            "https://youtube.com/",
            "file:///etc/passwd",
            "http://127.0.0.1:80@evil.test/x",
            "http://127.0.0.1:0/",
            "http://127.0.0.1:9/x\ncommand",
        ] {
            assert!(!super::trusted_url(url));
        }
    }
}
