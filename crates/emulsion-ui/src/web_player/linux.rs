use super::*;
use anyhow::Context;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, OpenOptionsExt},
        process::CommandExt,
    },
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Mutex, OnceLock,
        mpsc::{self, SyncSender},
    },
    thread,
};

static HELPER: OnceLock<&'static [u8]> = OnceLock::new();
/// The application embeds only this small adapter executable, not WebKit itself.
pub fn register_linux_helper(bytes: &'static [u8]) {
    let _ = HELPER.set(bytes);
}
const MAX_SIDE: u32 = 1920;
const MAX_PIXELS: u32 = 1920 * 1080;
const RUNTIME_HELP: &str = "Video playback needs the system WebKitGTK 4.1 runtime and GStreamer good/libav codecs (Arch: webkit2gtk-4.1 gst-plugins-good gst-libav; Ubuntu: libwebkit2gtk-4.1-0 gstreamer1.0-plugins-good gstreamer1.0-libav).";

#[derive(Default)]
struct Shared {
    frame: Option<Arc<RenderImage>>,
    error: Option<String>,
}

pub(super) struct PlatformPlayer {
    child: Child,
    sender: Option<SyncSender<String>>,
    shared: Arc<Mutex<Shared>>,
    directory: PathBuf,
    logical: (f32, f32),
    physical: (u32, u32),
}

fn dimensions(bounds: Bounds<Pixels>) -> ((f32, f32), (u32, u32)) {
    let w = f32::from(bounds.size.width).clamp(1.0, 100_000.0);
    let h = f32::from(bounds.size.height).clamp(1.0, 100_000.0);
    let scale = (MAX_SIDE as f32 / w.max(h))
        .min((MAX_PIXELS as f32 / (w * h)).sqrt())
        .min(1.0);
    (
        (w, h),
        (
            (w * scale).floor().max(1.0) as u32,
            (h * scale).floor().max(1.0) as u32,
        ),
    )
}

impl PlatformPlayer {
    pub(super) fn new(
        url: &str,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> anyhow::Result<Self> {
        let bytes = HELPER.get().filter(|bytes| !bytes.is_empty()).context("This build has no Linux video adapter. Rebuild with WebKitGTK 4.1 development headers installed.")?;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("emulsion-player-{}-{nonce}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let path = directory.join("player");
        let start = (|| -> anyhow::Result<Self> {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o700)
                .open(&path)?
                .write_all(bytes)?;
            let (logical, physical) = dimensions(bounds);
            let mut child = Command::new(&path)
                .args([url, &physical.0.to_string(), &physical.1.to_string()])
                .env("GDK_SCALE", "1")
                .env("GDK_DPI_SCALE", "1")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .process_group(0)
                .spawn()
                .with_context(|| RUNTIME_HELP)?;
            let mut output = child.stdout.take().expect("piped stdout");
            let mut input = child.stdin.take().expect("piped stdin");
            let mut errors = child.stderr.take().expect("piped stderr");
            let shared = Arc::new(Mutex::new(Shared::default()));
            let frames = shared.clone();
            thread::spawn(move || {
                loop {
                    let mut header = [0; 12];
                    if output.read_exact(&mut header).is_err() {
                        break;
                    }
                    let w = u32::from_le_bytes(header[4..8].try_into().unwrap());
                    let h = u32::from_le_bytes(header[8..12].try_into().unwrap());
                    if &header[..4] != b"EMP1"
                        || w == 0
                        || h == 0
                        || w > MAX_SIDE
                        || h > MAX_SIDE
                        || w.saturating_mul(h) > MAX_PIXELS
                    {
                        frames.lock().unwrap().error =
                            Some("The video player sent an invalid frame.".into());
                        return;
                    }
                    let mut pixels = vec![0; w as usize * h as usize * 4];
                    if output.read_exact(&mut pixels).is_err() {
                        break;
                    }
                    frames.lock().unwrap().frame =
                        Some(Arc::new(crate::viewport::bgra_image(w, h, pixels)));
                }
                let mut frames = frames.lock().unwrap();
                if frames.error.is_none() {
                    frames.error = Some(format!("The system video player stopped. {RUNTIME_HELP}"));
                }
            });
            let diagnostics = shared.clone();
            thread::spawn(move || {
                // Drain stderr continuously but retain at most 4 KiB of diagnostics.
                let mut retained = Vec::new();
                let mut buffer = [0; 1024];
                while let Ok(count) = errors.read(&mut buffer) {
                    if count == 0 {
                        break;
                    }
                    retained.extend_from_slice(
                        &buffer[..count.min(4096usize.saturating_sub(retained.len()))],
                    );
                    let message = String::from_utf8_lossy(&retained);
                    if message.contains("EMULSION_PLAYER_ERROR")
                        || message.contains("error while loading shared libraries")
                    {
                        diagnostics.lock().unwrap().error =
                            Some(format!("{RUNTIME_HELP}\n{}", message.trim()));
                    }
                }
            });
            let (sender, receiver) = mpsc::sync_channel::<String>(128);
            thread::spawn(move || {
                while let Ok(command) = receiver.recv() {
                    if input.write_all(command.as_bytes()).is_err() {
                        break;
                    }
                }
            });
            Ok(Self {
                child,
                sender: Some(sender),
                shared,
                directory: directory.clone(),
                logical,
                physical,
            })
        })();
        if start.is_err() {
            let _ = fs::remove_dir_all(&directory);
        }
        start
    }
    fn send(&self, command: String) {
        if let Some(sender) = &self.sender {
            let _ = sender.try_send(command);
        }
    }
    fn point(&self, x: f32, y: f32) -> (f32, f32) {
        (
            x.clamp(0.0, self.logical.0) * self.physical.0 as f32 / self.logical.0,
            y.clamp(0.0, self.logical.1) * self.physical.1 as f32 / self.logical.1,
        )
    }
    pub(super) fn update_bounds(&mut self, bounds: Bounds<Pixels>) {
        let (logical, physical) = dimensions(bounds);
        self.logical = logical;
        if self.physical != physical {
            self.physical = physical;
            self.send(format!("R {} {}\n", physical.0, physical.1));
        }
    }
    pub(super) fn latest_frame(&self) -> Option<Arc<RenderImage>> {
        self.shared.lock().unwrap().frame.clone()
    }
    pub(super) fn error(&self) -> Option<String> {
        self.shared.lock().unwrap().error.clone()
    }
    pub(super) fn pointer_move(&self, x: f32, y: f32) {
        let (x, y) = self.point(x, y);
        self.send(format!("M {x} {y}\n"));
    }
    pub(super) fn pointer_button(&self, button: u8, pressed: bool, x: f32, y: f32) {
        if !(1..=3).contains(&button) {
            return;
        }
        let (x, y) = self.point(x, y);
        self.send(format!("B {} {button} {x} {y}\n", u8::from(pressed)));
    }
    pub(super) fn scroll(&self, dx: f32, dy: f32, x: f32, y: f32) {
        let (x, y) = self.point(x, y);
        self.send(format!(
            "S {} {} {x} {y}\n",
            dx.clamp(-1000.0, 1000.0),
            dy.clamp(-1000.0, 1000.0)
        ));
    }
    pub(super) fn key(&self, key: &str, pressed: bool, modifiers: u32) {
        let key = match key {
            "space" => "space",
            "enter" => "Return",
            "backspace" => "BackSpace",
            "tab" => "Tab",
            "escape" => "Escape",
            "left" => "Left",
            "right" => "Right",
            "up" => "Up",
            "down" => "Down",
            "home" => "Home",
            "end" => "End",
            "delete" => "Delete",
            other => other,
        };
        if key.len() > 40
            || !key
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            return;
        }
        self.send(format!(
            "K {} {} {key}\n",
            u8::from(pressed),
            modifiers & 77
        ));
    }
}

impl Drop for PlatformPlayer {
    fn drop(&mut self) {
        self.sender.take();
        // Each helper has its own process group. Also stop the WebKit subprocesses
        // so switching slides or closing the presentation cannot leave audio playing.
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGTERM);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presentation_frames_have_a_bounded_pixel_budget() {
        use gpui_kit::gpui::{point, px, size};
        for (w, h) in [(3840., 2160.), (100000., 1.), (1., 100000.), (960., 540.)] {
            let (_, (w, h)) = dimensions(Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(w), px(h)),
            });
            assert!(w > 0 && h > 0 && w <= MAX_SIDE && h <= MAX_SIDE && w * h <= MAX_PIXELS);
        }
    }
}
