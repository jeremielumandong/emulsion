//! Animation assist and time-lapse.
//!
//! Animation: every top-level layer or group is one frame. The panel
//! plays them in turn (only the current frame shows; onion skin fades
//! the previous one in), steps through them, and exports a GIF. Nothing
//! is written to the document: the preview swaps the render tree only.
//!
//! Time-lapse: while recording, a small frame of the whole picture is
//! saved after each change (at most one every couple of seconds) into
//! the data directory; export assembles them into a GIF.

use super::*;
use std::path::PathBuf;

#[derive(Default)]
pub struct AnimState {
    pub open: bool,
    pub playing: bool,
    pub fps: u32,
    pub frame: usize,
    pub onion: bool,
    tick_running: bool,
    // ── Time-lapse ──
    pub record: bool,
    rec_dir: Option<PathBuf>,
    pub captured: usize,
    last_capture: Option<Instant>,
    last_rev: u64,
}

/// Seconds between time-lapse captures.
const CAPTURE_GAP: f32 = 2.0;
/// Longest side of an exported or captured frame.
const FRAME_PX: u32 = 800;

/// Mip level at which the longest side fits `FRAME_PX`.
fn level_for(w: u32, h: u32) -> u32 {
    let mut l = 0;
    while (w.max(h) >> l) > FRAME_PX && l < 8 {
        l += 1;
    }
    l
}

fn rgba_image(r: &Raster) -> Option<image::RgbaImage> {
    image::RgbaImage::from_raw(r.width(), r.height(), r.to_srgba8())
}

/// The document with only frame `i` visible (and, with `onion`, the
/// frame before it at a third of its opacity).
fn frame_doc(doc: &Document, i: usize, onion: bool) -> Document {
    let mut d = doc.clone();
    for (k, n) in d.nodes.iter_mut().enumerate() {
        if k == i {
            n.visible = true;
        } else if onion && k + 1 == i {
            n.visible = true;
            n.opacity *= 0.33;
        } else {
            n.visible = false;
        }
    }
    d
}

fn encode_gif(
    path: &std::path::Path,
    frames: Vec<image::RgbaImage>,
    fps: u32,
) -> Result<(), String> {
    use image::codecs::gif::{GifEncoder, Repeat};
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = GifEncoder::new(std::io::BufWriter::new(file));
    enc.set_repeat(Repeat::Infinite)
        .map_err(|e| e.to_string())?;
    let delay = image::Delay::from_numer_denom_ms(1000, fps.max(1));
    enc.encode_frames(
        frames
            .into_iter()
            .map(|f| image::Frame::from_parts(f, 0, 0, delay)),
    )
    .map_err(|e| e.to_string())
}

impl EditorView {
    pub(crate) fn frame_count(&self) -> usize {
        self.editor.doc.nodes.len()
    }

    /// The document to render: the animation preview while the panel is
    /// open, else the document itself.
    pub(crate) fn render_doc(&self) -> Document {
        if self.anim.open && self.frame_count() > 0 {
            let i = self.anim.frame.min(self.frame_count() - 1);
            frame_doc(&self.editor.doc, i, self.anim.onion)
        } else {
            self.editor.doc.clone()
        }
    }

    /// Whether the render tree should be built from a preview document.
    pub(crate) fn previewing(&self) -> bool {
        self.anim.open && self.frame_count() > 0
    }

    /// Rebuild the render tree after the preview changed.
    fn anim_changed(&mut self, cx: &mut Context<Self>) {
        self.seen_rev = u64::MAX;
        self.tree_dirty = emulsion_core::Dirty::All;
        cx.notify();
    }

    pub(crate) fn toggle_animation(&mut self, cx: &mut Context<Self>) {
        self.anim.open = !self.anim.open;
        self.select_sidebar(
            if self.anim.open {
                SidebarTab::Timeline
            } else {
                SidebarTab::Properties
            },
            cx,
        );
        self.anim.playing = false;
        if self.anim.fps == 0 {
            self.anim.fps = 8;
        }
        self.anim_changed(cx);
    }

    pub(crate) fn anim_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let n = self.frame_count().max(1) as isize;
        self.anim.frame = ((self.anim.frame as isize + delta).rem_euclid(n)) as usize;
        self.anim_changed(cx);
    }

    pub(crate) fn anim_play(&mut self, on: bool, cx: &mut Context<Self>) {
        self.anim.playing = on;
        if on && !self.anim.tick_running {
            self.anim.tick_running = true;
            cx.spawn(async move |this, cx| {
                loop {
                    let fps = this
                        .read_with(cx, |this, _| this.anim.fps.max(1))
                        .unwrap_or(8);
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(1000 / fps as u64))
                        .await;
                    let more = this.update(cx, |this, cx| {
                        if !this.anim.playing || !this.anim.open {
                            this.anim.tick_running = false;
                            return false;
                        }
                        this.anim_step(1, cx);
                        true
                    });
                    if !matches!(more, Ok(true)) {
                        break;
                    }
                }
            })
            .detach();
        }
        cx.notify();
    }

    /// Render every frame and write an animated GIF where the person chooses.
    pub(crate) fn export_animation_gif(&mut self, cx: &mut Context<Self>) {
        let n = self.frame_count();
        if n == 0 {
            self.set_status("Nothing to animate: add layers, one per frame.", true, cx);
            return;
        }
        let doc = self.editor.doc.clone();
        let fps = self.anim.fps.max(1);
        let name = self.name.clone();
        let dir = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let rx = cx.prompt_for_new_path(&dir, Some(&format!("{name}-animation.gif")));
        self.set_status("Rendering frames…", false, cx);
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut path))) = rx.await else {
                this.update(cx, |this, _| this.status = None).ok();
                return;
            };
            path.set_extension("gif");
            let out = path.clone();
            let result = cx
                .background_spawn(async move {
                    let level = level_for(doc.width, doc.height);
                    let mut frames = Vec::with_capacity(n);
                    for i in 0..n {
                        let d = frame_doc(&doc, i, false);
                        let r = emulsion_raster::composite::flatten(&d.composite_tree(), level);
                        frames.push(rgba_image(&r).ok_or("bad frame")?);
                    }
                    encode_gif(&out, frames, fps)
                })
                .await;
            this.update(cx, |this, cx| match result {
                Ok(()) => this.set_status(format!("Exported {}", path.display()), false, cx),
                Err(e) => this.set_status(format!("GIF export failed: {e}"), true, cx),
            })
            .ok();
        })
        .detach();
    }

    // ── Time-lapse ──────────────────────────────────────────────────────

    pub(crate) fn toggle_timelapse(&mut self, cx: &mut Context<Self>) {
        self.anim.record = !self.anim.record;
        if self.anim.record && self.anim.rec_dir.is_none() {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let dir = emulsion_io::recent::data_dir()
                .join("timelapse")
                .join(format!("{}-{stamp}", self.name));
            let _ = std::fs::create_dir_all(&dir);
            self.anim.rec_dir = Some(dir);
        }
        self.set_status(
            if self.anim.record {
                "Time-lapse recording: a frame is kept after each change."
            } else {
                "Time-lapse paused."
            },
            false,
            cx,
        );
        cx.notify();
    }

    /// Called after every change: keep a frame now and then while recording.
    pub(crate) fn timelapse_tick(&mut self, cx: &mut Context<Self>) {
        if !self.anim.record || self.editor.in_transaction() {
            return;
        }
        if self.editor.revision == self.anim.last_rev {
            return;
        }
        if let Some(t) = self.anim.last_capture
            && t.elapsed().as_secs_f32() < CAPTURE_GAP
        {
            return;
        }
        let Some(dir) = self.anim.rec_dir.clone() else {
            return;
        };
        self.anim.last_rev = self.editor.revision;
        self.anim.last_capture = Some(Instant::now());
        self.anim.captured += 1;
        let path = dir.join(format!("{:05}.png", self.anim.captured));
        let doc = self.editor.doc.clone();
        cx.background_spawn(async move {
            let level = level_for(doc.width, doc.height);
            let r = emulsion_raster::composite::flatten(&doc.composite_tree(), level);
            if let Some(img) = rgba_image(&r) {
                let _ = img.save(&path);
            }
        })
        .detach();
    }

    /// Assemble the recorded frames into a GIF.
    pub(crate) fn export_timelapse_gif(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = self.anim.rec_dir.clone() else {
            self.set_status("Start a time-lapse recording first.", true, cx);
            return;
        };
        let name = self.name.clone();
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let rx = cx.prompt_for_new_path(&home, Some(&format!("{name}-timelapse.gif")));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut path))) = rx.await else {
                return;
            };
            path.set_extension("gif");
            let out = path.clone();
            let result = cx
                .background_spawn(async move {
                    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
                        .map_err(|e| e.to_string())?
                        .flatten()
                        .map(|e| e.path())
                        .filter(|p| p.extension().is_some_and(|e| e == "png"))
                        .collect();
                    files.sort();
                    if files.is_empty() {
                        return Err("no frames recorded yet".to_string());
                    }
                    let mut frames = Vec::with_capacity(files.len());
                    for f in files {
                        let img = image::open(&f).map_err(|e| e.to_string())?.to_rgba8();
                        frames.push(img);
                    }
                    encode_gif(&out, frames, 8)
                })
                .await;
            this.update(cx, |this, cx| match result {
                Ok(()) => this.set_status(format!("Exported {}", path.display()), false, cx),
                Err(e) => this.set_status(format!("Time-lapse export failed: {e}"), true, cx),
            })
            .ok();
        })
        .detach();
    }

    /// The animation strip under the scene graph.
    pub(crate) fn animation_panel(
        &mut self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.anim.open {
            return None;
        }
        let n = self.frame_count();
        let (playing, onion, fps) = (self.anim.playing, self.anim.onion, self.anim.fps.max(1));
        let frame = if n == 0 {
            "no frames".to_string()
        } else {
            format!("frame {}/{n}", self.anim.frame.min(n - 1) + 1)
        };
        Some(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(6.))
                .pb(px(8.))
                .font_family(MONO_FONT)
                .text_size(px(10.5))
                .text_color(p.muted)
                .child(label("Animation", p))
                .child(
                    chip("an-play", if playing { "stop" } else { "play" }, playing, p)
                        .on_click(cx.listener(move |this, _, _, cx| this.anim_play(!playing, cx))),
                )
                .child(
                    chip("an-prev", "◀", false, p)
                        .on_click(cx.listener(|this, _, _, cx| this.anim_step(-1, cx))),
                )
                .child(
                    chip("an-next", "▶", false, p)
                        .on_click(cx.listener(|this, _, _, cx| this.anim_step(1, cx))),
                )
                .child(mono(frame, 10.5, p.ink))
                .child(
                    chip("an-fps", format!("{fps} fps"), false, p).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.anim.fps = match fps {
                                4 => 8,
                                8 => 12,
                                12 => 24,
                                _ => 4,
                            };
                            cx.notify();
                        },
                    )),
                )
                .child(
                    chip("an-onion", "onion skin", onion, p).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.anim.onion = !onion;
                            this.anim_changed(cx);
                        },
                    )),
                )
                .child(
                    chip("an-gif", "export GIF", false, p)
                        .on_click(cx.listener(|this, _, _, cx| this.export_animation_gif(cx))),
                )
                .child(div().child("each top-level layer or group is one frame"))
                .into_any_element(),
        )
    }
}
