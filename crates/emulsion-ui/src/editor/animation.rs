//! Animation assist and time-lapse.
//!
//! Animation: every top-level layer or group is one frame. The panel
//! plays them in turn (only the current frame shows; onion skin fades
//! the previous one in), steps through them, and exports a GIF. Nothing
//! is written to the document: the preview swaps the render tree only.
//!
//! Replay: the drawing played back from history, like Procreate's
//! time-lapse but with nothing recorded up front. Every commit on the head
//! branch (the file's root, each save and export) and every undo step still
//! held is a moment of the picture; replay renders them small, oldest
//! first, over the canvas, and can write them as a GIF. It therefore works
//! for anything drawn this session and for saved projects' commits, and
//! costs nothing until played.

use super::*;
use std::path::PathBuf;

#[derive(Default)]
pub struct AnimState {
    pub open: bool,
    pub playing: bool,
    pub fps: u32,
    pub frame: usize,
    pub onion: bool,
    playback_task: Option<Task<()>>,
    // ── Replay ──
    pub replay: Option<Replay>,
}

/// A replay in progress: which moments, which one shows, and its picture.
pub struct Replay {
    /// The picture at each moment, oldest first; the last is the present.
    docs: Vec<Document>,
    pub frame: usize,
    pub playing: bool,
    /// The rendered frame on screen, with its index.
    shown: Option<(usize, Arc<RenderImage>)>,
    /// A frame is being rendered off the UI thread.
    rendering: bool,
    render_task: Option<Task<()>>,
    playback_task: Option<Task<()>>,
    /// Bumped when playback starts, so an old loop stops itself.
    run: u64,
}

impl Replay {
    pub fn len(&self) -> usize {
        self.docs.len()
    }
}

/// Longest side of an exported or replayed frame.
const FRAME_PX: u32 = 800;
/// Most moments a replay shows; longer histories are thinned evenly.
const MAX_REPLAY_FRAMES: usize = 240;
/// Replay speed.
const REPLAY_FPS: u64 = 6;

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

/// The document with only top-level frame `i` visible (and, with `onion`,
/// the frame before it at a third of its opacity). A group is one frame
/// with everything inside it; children keep their own visibility.
fn frame_doc(doc: &Document, i: usize, onion: bool) -> Document {
    let mut d = doc.clone();
    let top = d.children(None);
    for n in d.nodes.iter_mut().filter(|n| n.parent.is_none()) {
        let k = top.iter().position(|id| *id == n.id).unwrap_or(usize::MAX);
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

/// A rendered picture as a GPUI image, over white where it is transparent.
fn display_image(r: &Raster) -> Arc<RenderImage> {
    let px = r.to_srgba8();
    let bgra: Vec<u8> = px
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|[r, g, b, a]| {
            let over = |c: u8| ((c as u32 * *a as u32 + 255 * (255 - *a as u32)) / 255) as u8;
            [over(*b), over(*g), over(*r), 255]
        })
        .collect();
    Arc::new(crate::viewport::bgra_image(r.width(), r.height(), bgra))
}

/// Write `frames` as a looping GIF, one frame at a time so a long replay
/// never holds every picture in memory at once. Frames go to a staging file
/// beside `path` that replaces it only once complete, so a failure leaves
/// any existing file untouched.
fn encode_gif(
    path: &std::path::Path,
    frames: impl IntoIterator<Item = Result<image::RgbaImage, String>>,
    fps: u32,
) -> Result<(), String> {
    use image::codecs::gif::{GifEncoder, Repeat};
    use std::io::Write as _;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let staging = path.with_file_name(format!(".{name}.emulsion-tmp-{}-{seq}", std::process::id()));
    let result = (|| {
        let mut out =
            std::io::BufWriter::new(std::fs::File::create(&staging).map_err(|e| e.to_string())?);
        {
            // Dropping the encoder writes the GIF trailer.
            let mut enc = GifEncoder::new(&mut out);
            enc.set_repeat(Repeat::Infinite)
                .map_err(|e| e.to_string())?;
            let delay = image::Delay::from_numer_denom_ms(1000, fps.max(1));
            for f in frames {
                enc.encode_frame(image::Frame::from_parts(f?, 0, 0, delay))
                    .map_err(|e| e.to_string())?;
            }
        }
        out.flush().map_err(|e| e.to_string())?;
        out.get_ref().sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&staging, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&staging);
    }
    result
}

impl EditorView {
    /// Hidden tabs retain their playback position, but run no preview timers.
    pub(crate) fn suspend_playback(&mut self, window: &mut Window, _cx: &mut Context<Self>) {
        self.anim.playing = false;
        self.anim.playback_task = None;
        if let Some(replay) = &mut self.anim.replay {
            replay.playing = false;
            replay.run = replay.run.wrapping_add(1);
            replay.playback_task = None;
            replay.render_task = None;
            replay.rendering = false;
            if let Some((_, image)) = replay.shown.take() {
                let _ = window.drop_image(image);
            }
        }
    }

    /// Restore the paused replay picture without restarting playback.
    pub(crate) fn resume_rendering(&mut self, cx: &mut Context<Self>) {
        self.replay_render_current(cx);
    }

    pub(crate) fn frame_count(&self) -> usize {
        self.editor.doc.children(None).len()
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
        self.anim.playback_task = None;
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
        self.anim.playing = on && self.visible;
        self.anim.playback_task = None;
        if self.anim.playing {
            let epoch = self.render_epoch;
            self.anim.playback_task = Some(cx.spawn(async move |this, cx| {
                loop {
                    let fps = this
                        .read_with(cx, |this, _| this.anim.fps.max(1))
                        .unwrap_or(8);
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(1000 / fps as u64))
                        .await;
                    let more = this.update(cx, |this, cx| {
                        if !this.visible
                            || this.render_epoch != epoch
                            || !this.anim.playing
                            || !this.anim.open
                        {
                            return false;
                        }
                        this.anim_step(1, cx);
                        true
                    });
                    if !matches!(more, Ok(true)) {
                        break;
                    }
                }
            }));
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
                    let frames = (0..n).map(|i| {
                        let d = frame_doc(&doc, i, false);
                        let r = emulsion_raster::composite::flatten(&d.composite_tree(), level);
                        rgba_image(&r).ok_or_else(|| "bad frame".to_string())
                    });
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

    // ── Replay ──────────────────────────────────────────────────────────

    /// Every moment of the picture history still knows, oldest first and
    /// ending with the present: the head branch's commits from the root,
    /// then the undo steps newer than the newest commit. Thinned evenly to
    /// `MAX_REPLAY_FRAMES`, always keeping the first and last.
    pub(crate) fn replay_docs(&self) -> Vec<Document> {
        let g = &self.editor.graph;
        let mut commits = Vec::new();
        let mut at = Some(g.head_branch().tip);
        while let Some(id) = at {
            let Some(c) = g.commit(id) else { break };
            commits.push(c.doc.clone());
            at = c.parents.first().copied();
        }
        commits.reverse();
        let mut docs: Vec<Document> = Vec::new();
        let push = |d: Document, docs: &mut Vec<Document>| {
            if docs.last() != Some(&d) {
                docs.push(d);
            }
        };
        for d in commits {
            push(d, &mut docs);
        }
        // `steps()` is newest first; each holds the picture before it ran.
        let steps: Vec<&emulsion_core::history::Step> = self.editor.history.steps().collect();
        for st in steps.into_iter().rev() {
            push(st.before.clone(), &mut docs);
        }
        push(self.editor.doc.clone(), &mut docs);
        if docs.len() > MAX_REPLAY_FRAMES {
            let n = docs.len();
            let keep: Vec<usize> = (0..MAX_REPLAY_FRAMES)
                .map(|i| i * (n - 1) / (MAX_REPLAY_FRAMES - 1))
                .collect();
            docs = docs
                .into_iter()
                .enumerate()
                .filter(|(i, _)| keep.binary_search(i).is_ok())
                .map(|(_, d)| d)
                .collect();
        }
        docs
    }

    /// Open the replay over the canvas and start playing from the first
    /// moment; while open, play again from wherever it stopped.
    pub(crate) fn replay_start(&mut self, cx: &mut Context<Self>) {
        if self.anim.replay.is_none() {
            let docs = self.replay_docs();
            if docs.len() < 2 {
                self.set_status(
                    "Nothing to replay yet: replay follows the history, so draw or edit first.",
                    true,
                    cx,
                );
                return;
            }
            self.anim.replay = Some(Replay {
                docs,
                frame: 0,
                playing: false,
                shown: None,
                rendering: false,
                render_task: None,
                playback_task: None,
                run: 0,
            });
        }
        self.replay_play(true, cx);
    }

    pub(crate) fn replay_close(&mut self, cx: &mut Context<Self>) {
        self.anim.replay = None;
        cx.notify();
    }

    /// Show moment `i` (rendering it off the UI thread) without playing.
    pub(crate) fn replay_seek(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(r) = &mut self.anim.replay else {
            return;
        };
        r.frame = i.min(r.len() - 1);
        r.playing = false;
        r.playback_task = None;
        r.run = r.run.wrapping_add(1);
        self.replay_render_current(cx);
    }

    fn replay_render_current(&mut self, cx: &mut Context<Self>) {
        if !self.visible {
            return;
        }
        let epoch = self.render_epoch;
        let Some(r) = &mut self.anim.replay else {
            return;
        };
        if r.rendering || r.shown.as_ref().is_some_and(|(i, _)| *i == r.frame) {
            cx.notify();
            return;
        }
        r.rendering = true;
        let (i, doc) = (r.frame, r.docs[r.frame].clone());
        r.render_task = Some(cx.spawn(async move |this, cx| {
            let img = cx
                .background_spawn(async move {
                    let level = level_for(doc.width, doc.height);
                    let r = emulsion_raster::composite::flatten(&doc.composite_tree(), level);
                    display_image(&r)
                })
                .await;
            this.update(cx, |this, cx| {
                if !this.visible || this.render_epoch != epoch {
                    return;
                }
                if let Some(r) = &mut this.anim.replay {
                    r.rendering = false;
                    r.shown = Some((i, img));
                    if r.frame != i {
                        this.replay_render_current(cx);
                    }
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// Play from the current moment to the end at `REPLAY_FPS`, rendering
    /// each frame as it comes; `false` pauses.
    pub(crate) fn replay_play(&mut self, on: bool, cx: &mut Context<Self>) {
        let epoch = self.render_epoch;
        let Some(r) = &mut self.anim.replay else {
            return;
        };
        r.playing = on && self.visible;
        r.playback_task = None;
        r.render_task = None;
        r.rendering = false;
        r.run = r.run.wrapping_add(1);
        let run = r.run;
        if !r.playing {
            cx.notify();
            return;
        }
        if r.frame + 1 >= r.len() {
            r.frame = 0;
            r.shown = None;
        }
        let n = r.len();
        r.playback_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let Ok(Some((i, doc))) = this.read_with(cx, |this, _| {
                    if !this.visible || this.render_epoch != epoch {
                        return None;
                    }
                    this.anim
                        .replay
                        .as_ref()
                        .filter(|r| r.playing && r.run == run)
                        .map(|r| (r.frame, r.docs[r.frame].clone()))
                }) else {
                    return;
                };
                let started = Instant::now();
                let img = cx
                    .background_spawn(async move {
                        let level = level_for(doc.width, doc.height);
                        let r = emulsion_raster::composite::flatten(&doc.composite_tree(), level);
                        display_image(&r)
                    })
                    .await;
                let more = this.update(cx, |this, cx| {
                    if !this.visible || this.render_epoch != epoch {
                        return false;
                    }
                    let Some(r) = &mut this.anim.replay else {
                        return false;
                    };
                    if !r.playing || r.run != run {
                        return false;
                    }
                    r.shown = Some((i, img));
                    if i + 1 >= n {
                        r.playing = false;
                        cx.notify();
                        return false;
                    }
                    r.frame = i + 1;
                    cx.notify();
                    true
                });
                if !matches!(more, Ok(true)) {
                    return;
                }
                let budget = std::time::Duration::from_millis(1000 / REPLAY_FPS);
                if let Some(rest) = budget.checked_sub(started.elapsed()) {
                    cx.background_executor().timer(rest).await;
                }
            }
        }));
        cx.notify();
    }

    /// Write the whole replay as a GIF where the person chooses.
    pub(crate) fn export_replay_gif(&mut self, cx: &mut Context<Self>) {
        let docs = match &self.anim.replay {
            Some(r) => r.docs.clone(),
            None => self.replay_docs(),
        };
        if docs.len() < 2 {
            self.set_status("Nothing to replay yet: draw or edit first.", true, cx);
            return;
        }
        let name = self.name.clone();
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let rx = cx.prompt_for_new_path(&home, Some(&format!("{name}-replay.gif")));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut path))) = rx.await else {
                return;
            };
            path.set_extension("gif");
            let out = path.clone();
            this.update(cx, |this, cx| {
                this.set_status(
                    format!("Rendering {} replay frames…", docs.len()),
                    false,
                    cx,
                )
            })
            .ok();
            let result = cx
                .background_spawn(async move {
                    let frames = docs.iter().map(|d| {
                        let level = level_for(d.width, d.height);
                        let r = emulsion_raster::composite::flatten(&d.composite_tree(), level);
                        rgba_image(&r).ok_or_else(|| "bad frame".to_string())
                    });
                    encode_gif(&out, frames, REPLAY_FPS as u32)
                })
                .await;
            this.update(cx, |this, cx| match result {
                Ok(()) => this.set_status(format!("Exported {}", path.display()), false, cx),
                Err(e) => this.set_status(format!("Replay export failed: {e}"), true, cx),
            })
            .ok();
        })
        .detach();
    }

    /// The replay over the canvas: the current moment, its position, and
    /// controls. None when no replay is open.
    pub(crate) fn replay_overlay(&self, p: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        let r = self.anim.replay.as_ref()?;
        let (n, i, playing) = (r.len(), r.frame, r.playing);
        let picture: AnyElement = match &r.shown {
            Some((_, picture)) => img(ImageSource::Render(picture.clone()))
                .max_w_full()
                .max_h_full()
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
            None => mono("rendering…", 11., p.chrome_fg).into_any_element(),
        };
        let bar = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(10.))
            .py(px(6.))
            .bg(p.chrome)
            .child(
                chip(
                    "replay-play",
                    if playing { "pause" } else { "play" },
                    playing,
                    p,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.replay_play(!playing, cx))),
            )
            .child(chip("replay-prev", "◀", false, p).on_click(
                cx.listener(move |this, _, _, cx| this.replay_seek(i.saturating_sub(1), cx)),
            ))
            .child(
                chip("replay-next", "▶", false, p)
                    .on_click(cx.listener(move |this, _, _, cx| this.replay_seek(i + 1, cx))),
            )
            .child(mono(format!("moment {}/{n}", i + 1), 10.5, p.chrome_fg))
            .child(div().flex_1())
            .child(
                chip("replay-gif", "export GIF", false, p)
                    .on_click(cx.listener(|this, _, _, cx| this.export_replay_gif(cx))),
            )
            .child(
                chip("replay-close", "close", false, p)
                    .on_click(cx.listener(|this, _, _, cx| this.replay_close(cx))),
            );
        Some(
            deferred(
                div()
                    .id("replay-overlay")
                    .test_support()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .bg(p.chrome.opacity(0.94))
                    .child(
                        div()
                            .id("replay-picture")
                            .flex_1()
                            .min_h_0()
                            .p(px(16.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(picture),
                    )
                    .child(bar),
            )
            .with_priority(1)
            .into_any_element(),
        )
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

#[cfg(test)]
mod tests {
    use super::encode_gif;

    #[test]
    fn a_failed_gif_export_leaves_the_existing_file_and_no_staging_file() {
        let dir = std::env::temp_dir().join(format!(
            "emulsion-gif-export-{}-{}",
            std::process::id(),
            emulsion_io::recent::now()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("replay.gif");
        std::fs::write(&path, b"previous export").unwrap();
        let frame = || {
            Ok(image::RgbaImage::from_pixel(
                4,
                4,
                image::Rgba([200, 30, 30, 255]),
            ))
        };

        let failed = encode_gif(&path, [frame(), Err("render failed".into())], 12);
        assert_eq!(failed, Err("render failed".into()));
        assert_eq!(std::fs::read(&path).unwrap(), b"previous export");
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            1,
            "staging file removed"
        );

        encode_gif(&path, [frame(), frame()], 12).unwrap();
        assert_eq!(image::open(&path).unwrap().width(), 4);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }
}
