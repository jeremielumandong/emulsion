//! Playing the storyboard animatic (T6, T8, V9): the player over the Stage
//! or Board, its transport bar and shortcuts, burn-in, sound and scrubbing,
//! and the audience window on any display.
//!
//! Time comes from `crate::playback`: the audio device's clock while the
//! board has sound (a cpal stream fed from the shared mixdown, mixed ahead on
//! a background thread), a monotonic clock otherwise or when there is no
//! device. Each tick shows the frame for the current time, dropping frames
//! rather than drifting. Pictures are the panel thumbnails at display size
//! (panels with layer keyframes are drawn per frame through
//! `animate_panel`), seen through the scene camera with the core's
//! `camera_view`; transitions blend them with the shared renderer and
//! burn-in is drawn with the core helper, on a background thread, so the
//! player and movie exports look the same.
use super::*;
use crate::playback::audience;
use crate::playback::audio_out::{AudioClock, Mix, Output};
use crate::playback::clock::{SystemClock, Timebase};
use crate::playback::player::{Player, Tick};
use emulsion_core::project::PageId;
use emulsion_core::storyboard::{CameraState, LayerMotion};
use emulsion_core::storyboard_animatic::{BurnIn, BurnInPosition, draw_burn_in};
use emulsion_core::storyboard_motion::camera_view;
use emulsion_core::timeline::{FrameRate, Timeline, TransitionKind, transition};
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
};
use std::collections::BTreeMap;
use std::time::Duration;

#[cfg(test)]
#[path = "storyboard_player_tests.rs"]
mod tests;

/// Longest side of played pictures, in pixels.
const MAX_PICTURE: u32 = 1920;
/// How often playback looks at the clock.
const TICK: Duration = Duration::from_millis(8);
/// How long Play waits for the first sound before starting anyway.
const SOUND_WAIT: Duration = Duration::from_millis(1500);
/// Shortest scrub grain.
const GRAIN: f64 = 0.08;
/// Burn-in text sizes offered, in percent of the frame height.
const BURN_SIZES: [f32; 5] = [3., 4., 5., 6., 8.];

/// A transport command; shortcuts, the transport bar and the audience
/// window all run these.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Playback {
    PlayPause,
    Stop,
    Step(i64),
    First,
    Last,
    MarkIn,
    MarkOut,
    ClearRange,
    ToggleLoop,
}

/// The audio output.
#[derive(Default)]
pub(crate) enum Device {
    #[default]
    Closed,
    Opening,
    Open(Rc<dyn Output>),
    Missing(String),
}

/// Opens the audio device; blocking, so it runs in the background.
pub(crate) type OpenDevice = fn() -> Result<Box<dyn Output + Send>, String>;

fn open_device() -> Result<Box<dyn Output + Send>, String> {
    // Tests never touch a real device.
    if cfg!(test) {
        return Err("No audio device in tests.".into());
    }
    crate::playback::audio_out::DeviceOutput::open().map(|d| Box::new(d) as _)
}

/// What a picture was made from, so it is only made again when that changes.
#[derive(Clone, Debug, PartialEq)]
struct FrameKey {
    to: SideKey,
    from: Option<(SideKey, TransitionKind, u32)>,
    lines: Vec<String>,
    burn_in: BurnIn,
}

/// One panel in a picture: where its pixels come from and the camera it is
/// seen through.
#[derive(Clone, Debug, PartialEq)]
struct SideKey {
    panel: PageId,
    source: SourceKey,
    /// The camera, unless at rest.
    camera: Option<[u64; 4]>,
}

#[derive(Clone, Debug, PartialEq)]
enum SourceKey {
    /// The panel's thumbnail, by address.
    Thumbnail(usize),
    /// Drawn with its layer keyframes at a frame into the panel.
    Animated {
        revision: u64,
        local: u64,
        motion: BTreeMap<NodeId, LayerMotion>,
    },
}

/// A panel's pixels for a picture: its thumbnail, or its keyframed
/// document to draw at the thumbnail's size.
enum SidePicture {
    Thumbnail(Arc<RenderImage>),
    Animated(Box<Document>),
}

/// One panel ready to compose: its pixels and, when the camera is not at
/// rest, the camera matrix in panel pixels with the panel's size.
struct Side {
    key: SideKey,
    picture: SidePicture,
    camera: Option<(glam::DAffine2, (u32, u32))>,
}

/// `side` as BGRA at `max` (the thumbnail size), through its camera.
fn side_pixels(side: &Side, max: u32) -> Option<(u32, u32, Vec<u8>)> {
    let (w, h, bytes) = match &side.picture {
        SidePicture::Thumbnail(image) => image_bytes(image)?,
        SidePicture::Animated(doc) => super::doc_thumb(doc, max),
    };
    let Some((m, (dw, dh))) = side.camera else {
        return Some((w, h, bytes));
    };
    // The camera works in panel pixels; the picture is smaller.
    let k = glam::dvec2(f64::from(w) / f64::from(dw), f64::from(h) / f64::from(dh));
    let m = glam::DAffine2::from_scale(k) * m * glam::DAffine2::from_scale(1. / k);
    Some((w, h, camera_view(&bytes, w, h, m, w, h, [255; 4])))
}

pub(crate) struct PlayerUi {
    /// The animatic shows over the Stage or Board, from Play until Stop.
    pub(crate) showing: bool,
    run: Option<Player>,
    clock: Box<dyn Timebase>,
    ticker: Option<Task<()>>,
    /// Play was pressed and waits for sound, since then.
    waiting: Option<(Instant, Arc<Mix>)>,
    /// The picture for the playhead, and what it was made from.
    pub(crate) picture: Option<Arc<RenderImage>>,
    made: Option<FrameKey>,
    /// The picture was composed here (not a shared thumbnail).
    owned: bool,
    pub(crate) composing: bool,
    pub(crate) burn_in: BurnIn,
    pub(crate) burn_in_on: bool,
    pub(crate) device: Device,
    pub(crate) open_device: OpenDevice,
    mix: Option<(Timeline, FrameRate, Arc<Mix>)>,
    /// The playhead last heard while scrubbing.
    heard: Option<u64>,
    /// The audience window is wanted, and its handle once open.
    audience_on: bool,
    audience: Option<AnyWindowHandle>,
    /// The view and time when Space went down, to tell a tap from a pan.
    space: Option<((f64, f64), Instant)>,
    /// Said under the transport, such as playing without a device.
    pub(crate) notice: Option<String>,
    /// Frames skipped to keep time in the last run.
    pub(crate) dropped: u64,
    /// Silent playback runs on this clock in tests.
    #[cfg(test)]
    pub(crate) fake_clock: Option<crate::playback::clock::FakeClock>,
}

impl Default for PlayerUi {
    fn default() -> Self {
        Self {
            showing: false,
            run: None,
            clock: Box::new(SystemClock::default()),
            ticker: None,
            waiting: None,
            picture: None,
            made: None,
            owned: false,
            composing: false,
            burn_in: BurnIn::default(),
            burn_in_on: true,
            device: Device::Closed,
            open_device,
            mix: None,
            heard: None,
            audience_on: false,
            audience: None,
            space: None,
            notice: None,
            dropped: 0,
            #[cfg(test)]
            fake_clock: None,
        }
    }
}

/// `kind` for pictures in BGRA order (only the fade colour cares).
fn bgra_kind(kind: TransitionKind) -> TransitionKind {
    match kind {
        TransitionKind::FadeToColor { color: [r, g, b] } => {
            TransitionKind::FadeToColor { color: [b, g, r] }
        }
        other => other,
    }
}

/// One animatic picture: `to`, entering over `from` when a transition plays,
/// with burn-in `lines`. Pictures are `w` × `h` BGRA; the transition
/// renderer and burn-in treat channels alike apart from the fade colour.
pub(crate) fn compose(
    to: &[u8],
    from: Option<(&[u8], TransitionKind, f32)>,
    w: u32,
    h: u32,
    lines: &[String],
    burn_in: &BurnIn,
) -> Vec<u8> {
    let mut out = match from {
        Some((from, kind, t)) => transition::blend(bgra_kind(kind), t, from, to, w, h),
        None => to.to_vec(),
    };
    draw_burn_in(&mut out, w, h, lines, burn_in);
    out
}

fn image_bytes(image: &RenderImage) -> Option<(u32, u32, Vec<u8>)> {
    let size = image.size(0);
    Some((
        size.width.0 as u32,
        size.height.0 as u32,
        image.as_bytes(0)?.to_vec(),
    ))
}

impl EditorView {
    fn playback_rate(&self) -> Option<FrameRate> {
        Some(self.editor.storyboard()?.settings.frame_rate)
    }

    fn playback_layout(&self) -> Vec<PageId> {
        self.editor.page_list().iter().map(|m| m.id).collect()
    }

    pub(crate) fn playback_playing(&self) -> bool {
        self.transport.playing || self.player.waiting.is_some()
    }

    /// Run a transport command. False when there is no storyboard, so the
    /// key can do what it does elsewhere.
    pub(crate) fn playback(&mut self, command: Playback, cx: &mut Context<Self>) -> bool {
        if self.editor.storyboard().is_none() {
            return false;
        }
        let total = self.timeline_length().max(1);
        match command {
            Playback::PlayPause => {
                if self.playback_playing() {
                    self.playback_pause(cx);
                } else {
                    self.playback_play(cx);
                }
            }
            Playback::Stop => self.playback_stop(cx),
            Playback::Step(delta) => {
                self.playback_pause(cx);
                let frame = self.transport.stepped(delta, total);
                self.timeline_seek(frame, cx);
            }
            Playback::First | Playback::Last => {
                self.playback_pause(cx);
                let (start, end) = self.transport.bounds(total);
                let frame = if command == Playback::First {
                    start
                } else {
                    end - 1
                };
                self.timeline_seek(frame, cx);
            }
            Playback::MarkIn => self.timeline_set_range(false, cx),
            Playback::MarkOut => self.timeline_set_range(true, cx),
            Playback::ClearRange => self.timeline_clear_range(cx),
            Playback::ToggleLoop => self.transport.looping = !self.transport.looping,
        }
        self.refresh_picture(cx);
        cx.notify();
        true
    }

    fn playback_play(&mut self, cx: &mut Context<Self>) {
        let Some(board) = self.editor.storyboard() else {
            return;
        };
        let rate = board.settings.frame_rate;
        let empty =
            board.animatic_frames(&self.playback_layout()) == 0 && board.timeline.end() == 0;
        let sound = emulsion_io::audio::mix::has_sound(&board.timeline, 0, u64::MAX);
        if empty {
            self.set_status("Add panels to play the animatic.", false, cx);
            return;
        }
        if !self.prepare_page_action(cx) {
            return;
        }
        self.player.showing = true;
        self.player.notice = None;
        if !sound {
            self.playback_begin(self.silent_clock(), cx);
            return;
        }
        match &self.player.device {
            Device::Open(output) => {
                let output = output.clone();
                let Some(mix) = self.playback_mix(cx) else {
                    return;
                };
                let from = self.transport.start_frame(self.timeline_length());
                let at = mix.sample_at(rate, from);
                if mix.ready(at) {
                    self.playback_begin(Box::new(AudioClock { output, mix }), cx);
                } else {
                    mix.want(at);
                    self.player.waiting = Some((Instant::now(), mix));
                    self.playback_ticker(cx);
                }
            }
            Device::Missing(reason) => {
                self.player.notice = Some(format!("{reason} Playing without sound."));
                self.playback_begin(self.silent_clock(), cx);
            }
            Device::Closed | Device::Opening => {
                // Play once the device answers, with or without it.
                self.playback_open_device(true, cx);
            }
        }
        cx.notify();
    }

    /// The clock for playing without sound.
    fn silent_clock(&self) -> Box<dyn Timebase> {
        #[cfg(test)]
        if let Some(clock) = &self.player.fake_clock {
            return Box::new(clock.clone());
        }
        Box::new(SystemClock::default())
    }

    /// Start a run on `clock`.
    fn playback_begin(&mut self, clock: Box<dyn Timebase>, cx: &mut Context<Self>) {
        let Some(rate) = self.playback_rate() else {
            return;
        };
        let total = self.timeline_length().max(1);
        self.player.waiting = None;
        self.player.clock.stop();
        self.player.clock = clock;
        self.player.dropped = 0;
        self.player.run = Some(Player::start(
            &mut self.transport,
            total,
            rate,
            self.player.clock.as_mut(),
        ));
        self.refresh_picture(cx);
        self.playback_ticker(cx);
        cx.notify();
    }

    fn playback_ticker(&mut self, cx: &mut Context<Self>) {
        if self.player.ticker.is_some() {
            return;
        }
        self.player.ticker = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                let more = this
                    .update(cx, |this, cx| this.animatic_tick(cx))
                    .unwrap_or(false);
                if !more {
                    break;
                }
            }
        }));
    }

    /// Advance playback to the clock. False once it has stopped.
    pub(crate) fn animatic_tick(&mut self, cx: &mut Context<Self>) -> bool {
        if let Some((since, mix)) = &self.player.waiting {
            let at = self.transport.frame;
            let rate = self.playback_rate().unwrap_or(FrameRate::whole(24));
            if mix.ready(mix.sample_at(rate, at)) || since.elapsed() > SOUND_WAIT {
                let mix = mix.clone();
                if let Device::Open(output) = &self.player.device {
                    let output = output.clone();
                    self.player.ticker = None;
                    self.playback_begin(Box::new(AudioClock { output, mix }), cx);
                    return false;
                }
                self.player.waiting = None;
            }
            return self.player.waiting.is_some();
        }
        let total = self.timeline_length().max(1);
        let Some(run) = self.player.run.as_mut() else {
            self.player.ticker = None;
            return false;
        };
        let tick = run.tick(&mut self.transport, total, self.player.clock.as_mut());
        self.player.dropped = run.dropped;
        match tick {
            Tick::Hold => true,
            Tick::Show(_) => {
                self.refresh_picture(cx);
                cx.notify();
                true
            }
            Tick::Ended | Tick::Idle => {
                self.player.run = None;
                self.player.ticker = None;
                self.playback_follow(cx);
                self.refresh_picture(cx);
                cx.notify();
                false
            }
        }
    }

    /// Pause where playback is; the animatic stays on screen.
    pub(crate) fn playback_pause(&mut self, cx: &mut Context<Self>) {
        if !self.playback_playing() {
            return;
        }
        Player::pause(&mut self.transport, self.player.clock.as_mut());
        self.player.run = None;
        self.player.ticker = None;
        self.player.waiting = None;
        self.playback_follow(cx);
        cx.notify();
    }

    /// Stop and go back to drawing, on the panel under the playhead.
    pub(crate) fn playback_stop(&mut self, cx: &mut Context<Self>) {
        self.playback_pause(cx);
        self.player.showing = false;
        self.player.audience_on = false;
        if let Some(handle) = self.player.audience.take() {
            cx.defer(move |cx| {
                cx.update_window(handle, |_, window, _| window.remove_window())
                    .ok();
            });
        }
        self.retire_picture(cx);
        self.player.picture = None;
        self.player.made = None;
        cx.notify();
    }

    /// The panel under the playhead becomes the active panel.
    fn playback_follow(&mut self, cx: &mut Context<Self>) {
        let frame = self.transport.frame;
        self.timeline_seek(frame, cx);
    }

    // ── Sound ──

    fn playback_open_device(&mut self, then_play: bool, cx: &mut Context<Self>) {
        if !matches!(self.player.device, Device::Closed) {
            return;
        }
        self.player.device = Device::Opening;
        let open = self.player.open_device;
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { open() }).await;
            this.update(cx, |this, cx| {
                this.player.device = match result {
                    Ok(output) => Device::Open(Rc::from(output as Box<dyn Output>)),
                    Err(error) => {
                        let error = if error.ends_with('.') {
                            error
                        } else {
                            format!("{error}.")
                        };
                        Device::Missing(error)
                    }
                };
                if then_play && this.player.showing && !this.playback_playing() {
                    this.playback_play(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Play the board's sound from timeline frame `from` without pictures
    /// (for the Panel Timer), or stop it with `None`.
    pub(crate) fn playback_sound(&mut self, from: Option<u64>, cx: &mut Context<Self>) {
        let Some(rate) = self.playback_rate() else {
            return;
        };
        let Device::Open(output) = &self.player.device else {
            if from.is_some() {
                self.playback_open_device(false, cx);
            }
            return;
        };
        let output = output.clone();
        match from {
            Some(frame) => {
                if let Some(mix) = self.playback_mix(cx) {
                    let at = mix.sample_at(rate, frame);
                    output.play(mix, at);
                }
            }
            None if !self.playback_playing() => output.stop(),
            None => {}
        }
    }

    /// The board's sound, mixed ahead for the open device; made again when
    /// the timeline changes.
    fn playback_mix(&mut self, cx: &mut Context<Self>) -> Option<Arc<Mix>> {
        let board = self.editor.storyboard()?;
        let rate = board.settings.frame_rate;
        if let Some((timeline, r, mix)) = &self.player.mix
            && *timeline == board.timeline
            && *r == rate
        {
            if let Some(error) = mix.error() {
                self.player.notice = Some(format!("Some sound could not play: {error}"));
            }
            return Some(mix.clone());
        }
        let timeline = board.timeline.clone();
        let len = emulsion_io::audio::mix::frame_sample(rate, timeline.end());
        let source = timeline.clone();
        let mix = Mix::new(
            emulsion_io::audio::RATE,
            len,
            Box::new(move |first, count| {
                emulsion_io::audio::mix::mix_samples(&source, rate, first, count)
                    .map_err(|e| format!("{e:#}"))
            }),
        );
        self.player.mix = Some((timeline, rate, mix.clone()));
        cx.notify();
        Some(mix)
    }

    /// Keep sound ready and scrub: when the playhead moves while stopped, a
    /// short grain plays there. Called whenever the Stage or Board renders.
    pub(crate) fn playback_sync(&mut self, cx: &mut Context<Self>) {
        let Some(board) = self.editor.storyboard() else {
            return;
        };
        let rate = board.settings.frame_rate;
        let sound = emulsion_io::audio::mix::has_sound(&board.timeline, 0, u64::MAX);
        let frame = self.transport.frame;
        if !sound {
            self.player.heard = Some(frame);
        } else if let Device::Open(output) = &self.player.device {
            let output = output.clone();
            if let Some(mix) = self.playback_mix(cx) {
                let at = mix.sample_at(rate, frame);
                mix.want(at);
                let moved = self.player.heard.is_some_and(|h| h != frame);
                if moved && !self.playback_playing() {
                    let len = (rate.frames_to_seconds(1).max(GRAIN) * f64::from(mix.rate)) as u64;
                    output.grain(mix, at, len);
                }
            }
            self.player.heard = Some(frame);
        } else {
            self.playback_open_device(false, cx);
        }
        if self.player.showing && !self.transport.playing {
            self.refresh_picture(cx);
        }
    }

    // ── Pictures ──

    /// Picture size for the area the animatic plays in.
    fn picture_size(&self) -> u32 {
        let side = self.canvas_bounds().map_or(640., |b| {
            f32::from(b.size.width).max(f32::from(b.size.height))
        });
        // Twice the layout size covers high-density screens.
        (((side * 2.) as u32).div_ceil(128) * 128).clamp(256, MAX_PICTURE)
    }

    /// Make the picture for the playhead, if what it shows changed. The
    /// panel pictures load in the background; until they do, the last
    /// picture stays up.
    pub(crate) fn refresh_picture(&mut self, cx: &mut Context<Self>) {
        if !self.player.showing {
            return;
        }
        let layout = self.playback_layout();
        let frame = self.transport.frame;
        let Some(board) = self.editor.storyboard() else {
            return;
        };
        let Some(at) = board.animatic_frame(&layout, frame) else {
            // Past the last panel (sound runs on): black.
            self.retire_picture(cx);
            self.player.picture = None;
            self.player.made = None;
            return;
        };
        let lines = if self.player.burn_in_on {
            board.burn_in_lines(&layout, frame, &self.player.burn_in)
        } else {
            Vec::new()
        };
        // Load the next panel ahead of its cut.
        let next = board
            .playing(&layout)
            .iter()
            .skip_while(|(id, _)| *id != at.panel)
            .nth(1)
            .map(|(id, _)| *id);
        let max = self.picture_size();
        let from_frame = (frame - at.local).saturating_sub(1);
        let from_local = at
            .blend
            .map(|(id, _, _)| u64::from(board.panels[&id].frames).saturating_sub(1));
        if let Some(next) = next {
            self.page_thumbnail(next, max, cx);
        }
        let Some(to) = self.player_side(at.panel, at.local, frame, max, cx) else {
            return;
        };
        let from = at.blend.and_then(|(id, kind, t)| {
            let side = self.player_side(id, from_local?, from_frame, max, cx)?;
            Some((side, kind, t))
        });
        let key = FrameKey {
            to: to.key.clone(),
            from: from
                .as_ref()
                .map(|(side, kind, t)| (side.key.clone(), *kind, t.to_bits())),
            lines: lines.clone(),
            burn_in: self.player.burn_in.clone(),
        };
        if self.player.made.as_ref() == Some(&key) {
            return;
        }
        if from.is_none()
            && lines.is_empty()
            && to.camera.is_none()
            && let SidePicture::Thumbnail(image) = &to.picture
        {
            self.retire_picture(cx);
            self.player.picture = Some(image.clone());
            self.player.owned = false;
            self.player.made = Some(key);
            cx.notify();
            return;
        }
        if self.player.composing {
            return;
        }
        let burn_in = self.player.burn_in.clone();
        self.player.composing = true;
        cx.spawn(async move |this, cx| {
            let bytes = cx
                .background_spawn(async move {
                    let (w, h, to) = side_pixels(&to, max)?;
                    // A neighbour still loading at another size cuts instead.
                    let from = from.and_then(|(side, kind, t)| {
                        let (fw, fh, bytes) = side_pixels(&side, max)?;
                        ((fw, fh) == (w, h)).then_some((bytes, kind, t))
                    });
                    let from = from.as_ref().map(|(b, kind, t)| (b.as_slice(), *kind, *t));
                    Some((w, h, compose(&to, from, w, h, &lines, &burn_in)))
                })
                .await;
            this.update(cx, |this, cx| {
                this.player.composing = false;
                if this.player.showing
                    && let Some((w, h, bytes)) = bytes
                {
                    this.retire_picture(cx);
                    this.player.picture = Some(Arc::new(viewport::bgra_image(w, h, bytes)));
                    this.player.owned = true;
                    this.player.made = Some(key);
                    // The playhead may have moved on meanwhile.
                    this.refresh_picture(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Panel `panel`, `local` frames in, as animatic frame `frame` shows
    /// it: its thumbnail, or (with layer keyframes) its document at that
    /// frame, through the scene camera there. `None` while the thumbnail
    /// loads.
    fn player_side(
        &mut self,
        panel: PageId,
        local: u64,
        frame: u64,
        max: u32,
        cx: &mut Context<Self>,
    ) -> Option<Side> {
        let layout = self.playback_layout();
        let board = self.editor.storyboard()?;
        let state: CameraState = board.camera_at(&layout, frame as f64);
        let page = self.editor.page(panel)?;
        let size = (page.doc.width, page.doc.height);
        let camera = (state != board.rest_camera()).then(|| (board.camera_matrix(state), size));
        let camera_key =
            camera.map(|_| [state.x, state.y, state.zoom, state.rotation].map(f64::to_bits));
        let motion = &board.panels.get(&panel)?.motion;
        if !motion.is_empty() {
            let doc = board.animate_panel(panel, &page.doc, local as f64).ok()?;
            return Some(Side {
                key: SideKey {
                    panel,
                    source: SourceKey::Animated {
                        revision: page.revision,
                        local,
                        motion: motion.clone(),
                    },
                    camera: camera_key,
                },
                picture: SidePicture::Animated(Box::new(doc)),
                camera,
            });
        }
        let image = self.page_thumbnail(panel, max, cx)?;
        Some(Side {
            key: SideKey {
                panel,
                source: SourceKey::Thumbnail(Arc::as_ptr(&image) as usize),
                camera: camera_key,
            },
            picture: SidePicture::Thumbnail(image),
            camera,
        })
    }

    /// Free the GPU copy of a composed picture that is being replaced.
    fn retire_picture(&mut self, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.player.owned) {
            return;
        }
        if let Some(image) = self.player.picture.clone() {
            cx.defer(move |cx| {
                for handle in cx.windows() {
                    let image = image.clone();
                    cx.update_window(handle, |_, window, _| {
                        window.drop_image(image).ok();
                    })
                    .ok();
                }
            });
        }
    }

    // ── Audience window ──

    /// Play on a full-screen window on `display` (the main one when `None`).
    pub(crate) fn playback_full_screen(
        &mut self,
        display: Option<DisplayId>,
        cx: &mut Context<Self>,
    ) {
        if self.editor.storyboard().is_none() {
            return;
        }
        if let Some(handle) = self.player.audience.take() {
            cx.defer(move |cx| {
                cx.update_window(handle, |_, window, _| window.remove_window())
                    .ok();
            });
        }
        self.player.audience_on = true;
        if !self.playback_playing() {
            self.playback_play(cx);
        }
        let owner = cx.entity();
        // A new window draws at once, and it reads the editor: open it
        // after this update.
        cx.defer(move |cx| {
            let opened = audience::open(
                &owner,
                display,
                |this: &EditorView, _: &App| {
                    this.player.audience_on.then(|| this.player.picture.clone())
                },
                |this: &mut EditorView, event: &KeyDownEvent, cx: &mut Context<EditorView>| {
                    this.playback_key(event, cx);
                },
                |this: &mut EditorView, cx: &mut Context<EditorView>| {
                    this.player.audience = None;
                    this.playback_stop(cx);
                },
                cx,
            );
            owner.update(cx, |this, cx| match opened {
                Ok(handle) if this.player.audience_on => this.player.audience = Some(handle),
                Ok(handle) => {
                    cx.defer(move |cx| {
                        cx.update_window(handle, |_, window, _| window.remove_window())
                            .ok();
                    });
                }
                Err(error) => {
                    this.player.audience_on = false;
                    this.set_status(
                        format!("Unable to open the player window: {error}"),
                        true,
                        cx,
                    );
                }
            });
        });
        cx.notify();
    }

    /// Keys on the audience window.
    fn playback_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let command = match event.keystroke.key.as_str() {
            "space" | "k" => Playback::PlayPause,
            "left" | "," => Playback::Step(-1),
            "right" | "." => Playback::Step(1),
            "home" => Playback::First,
            "end" => Playback::Last,
            _ => return,
        };
        self.playback(command, cx);
    }

    // ── Keys on the Stage ──

    /// Space went down on the canvas: remember the view, so letting go
    /// without panning plays or pauses.
    pub(crate) fn playback_space_down(&mut self) {
        if self.editor.storyboard().is_some() {
            self.player.space = Some((self.view.center, Instant::now()));
        }
    }

    /// Space came up: a quick tap (no pan) plays or pauses.
    pub(crate) fn playback_space_up(&mut self, cx: &mut Context<Self>) {
        if let Some((center, at)) = self.player.space.take()
            && center == self.view.center
            && at.elapsed() < Duration::from_millis(500)
            && self.drag.is_none()
        {
            self.playback(Playback::PlayPause, cx);
        }
    }

    /// The transport shortcuts on an element (the canvas or the Board).
    /// Keys fall through to whatever else they do on other documents, and
    /// Escape does only while the animatic is not showing.
    pub(crate) fn playback_actions<E: InteractiveElement>(el: E, cx: &mut Context<Self>) -> E {
        use crate::actions::*;
        fn run(this: &mut EditorView, command: Playback, cx: &mut Context<EditorView>) {
            if !this.playback(command, cx) {
                cx.propagate();
            }
        }
        el.on_action(cx.listener(|this, _: &PlayPause, _, cx| run(this, Playback::PlayPause, cx)))
            .on_action(
                cx.listener(|this, _: &PreviousFrame, _, cx| run(this, Playback::Step(-1), cx)),
            )
            .on_action(cx.listener(|this, _: &NextFrame, _, cx| run(this, Playback::Step(1), cx)))
            .on_action(cx.listener(|this, _: &FirstFrame, _, cx| run(this, Playback::First, cx)))
            .on_action(cx.listener(|this, _: &LastFrame, _, cx| run(this, Playback::Last, cx)))
            .on_action(cx.listener(|this, _: &SetPlayIn, _, cx| run(this, Playback::MarkIn, cx)))
            .on_action(cx.listener(|this, _: &SetPlayOut, _, cx| run(this, Playback::MarkOut, cx)))
            .on_action(
                cx.listener(|this, _: &ClearPlayRange, _, cx| run(this, Playback::ClearRange, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ToggleLoop, _, cx| run(this, Playback::ToggleLoop, cx)),
            )
            .on_action(cx.listener(|this, _: &StopPlayback, _, cx| {
                if this.player.showing {
                    this.playback(Playback::Stop, cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &ResetRotation, _, cx| {
                if this.player.showing {
                    this.playback(Playback::Stop, cx);
                } else {
                    cx.propagate();
                }
            }))
    }

    // ── On screen ──

    /// The animatic over the Stage or Board while it shows, and the
    /// transport bar.
    pub(crate) fn playback_layers(
        &mut self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        if self.editor.storyboard().is_none() {
            return Vec::new();
        }
        let mut out = Vec::new();
        if self.player.showing {
            out.push(
                div()
                    .id("storyboard-player")
                    .test_support()
                    .absolute()
                    .inset_0()
                    .bg(rgb(0x000000))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.playback(Playback::PlayPause, cx);
                    }))
                    .children(
                        self.player
                            .picture
                            .clone()
                            .map(|image| img(image).size_full().object_fit(ObjectFit::Contain)),
                    )
                    .children(self.reference_video_player())
                    .into_any_element(),
            );
        }
        // The Timeline hosts the transport while it is open.
        if !self.timeline_open() {
            out.push(
                div()
                    .absolute()
                    .bottom_2()
                    .right_2()
                    .child(self.transport_bar(p, cx))
                    .into_any_element(),
            );
        }
        out
    }

    /// The transport: play, stop, stepping, the playhead's timecode, loop,
    /// the play range and player options. The Timeline or Stage hosts it.
    pub(crate) fn transport_bar(&mut self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(board) = self.editor.storyboard() else {
            return div().into_any_element();
        };
        let rate = board.settings.frame_rate;
        let total = self.timeline_length();
        let playing = self.playback_playing();
        let t = &self.transport;
        let command = |id: &'static str, label: &'static str, tip: &'static str, run: Playback| {
            Button::new(id)
                .label(label)
                .tooltip(tip)
                .xsmall()
                .ghost()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.playback(run, cx);
                }))
        };
        let range = t.range.map(|(a, b)| {
            format!(
                "{} – {}",
                rate.timecode(a),
                rate.timecode(b.saturating_sub(1))
            )
        });
        let owner = cx.weak_entity();
        div()
            .id("storyboard-transport")
            .test_support()
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .py(px(2.))
            .rounded(px(6.))
            .bg(p.panel.opacity(0.92))
            .border_1()
            .border_color(p.line)
            .text_size(px(11.))
            .text_color(p.ink)
            .child(command(
                "transport-first",
                "⏮",
                "Go to the start (Home)",
                Playback::First,
            ))
            .child(command(
                "transport-back",
                "◀|",
                "Previous frame (,)",
                Playback::Step(-1),
            ))
            .child(
                Button::new("transport-play")
                    .label(if playing { "❚❚" } else { "▶" })
                    .tooltip(if playing {
                        "Pause (Space)"
                    } else {
                        "Play the animatic (Space)"
                    })
                    .xsmall()
                    .primary()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.playback(Playback::PlayPause, cx);
                    })),
            )
            .child(command(
                "transport-stop",
                "■",
                "Stop and go back to drawing (Esc)",
                Playback::Stop,
            ))
            .child(command(
                "transport-forward",
                "|▶",
                "Next frame (.)",
                Playback::Step(1),
            ))
            .child(command(
                "transport-last",
                "⏭",
                "Go to the end (End)",
                Playback::Last,
            ))
            .child(div().px_1().font_family(MONO_FONT).child(format!(
                "{} / {}",
                rate.timecode(t.frame),
                rate.timecode(total)
            )))
            .child(
                Button::new("transport-loop")
                    .label("Loop")
                    .tooltip("Play the range over and over (Alt+Shift+R)")
                    .xsmall()
                    .when(t.looping, |b| b.primary())
                    .when(!t.looping, |b| b.ghost())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.playback(Playback::ToggleLoop, cx);
                    })),
            )
            .child(command(
                "transport-in",
                "In",
                "Start the play range here (Shift+I)",
                Playback::MarkIn,
            ))
            .child(command(
                "transport-out",
                "Out",
                "End the play range here (Shift+O)",
                Playback::MarkOut,
            ))
            .when_some(range, |d, range| {
                d.child(
                    Button::new("transport-range")
                        .label(range)
                        .tooltip("Clear the play range (Alt+X)")
                        .xsmall()
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.playback(Playback::ClearRange, cx);
                        })),
                )
            })
            .child(
                Button::new("transport-options")
                    .label("Options ▾")
                    .tooltip("Burn-in, full screen and the Panel Timer")
                    .xsmall()
                    .ghost()
                    .dropdown_menu(move |menu, window, cx| {
                        Self::player_options(menu, owner.clone(), window, cx)
                    }),
            )
            .when_some(self.player.notice.clone(), |d, notice| {
                d.child(
                    div()
                        .id("transport-notice")
                        .test_support()
                        .px_1()
                        .text_color(p.muted)
                        .child(notice),
                )
            })
            .into_any_element()
    }

    /// The player options: burn-in, playing full screen and the Panel Timer.
    fn player_options(
        menu: PopupMenu,
        owner: WeakEntity<Self>,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let Some(editor) = owner.upgrade() else {
            return menu;
        };
        let (burn, on, captions) = {
            let e = editor.read(cx);
            let captions: Vec<String> = e
                .editor
                .storyboard()
                .map(|b| b.captions.iter().map(|c| c.name.clone()).collect())
                .unwrap_or_default();
            (e.player.burn_in.clone(), e.player.burn_in_on, captions)
        };
        let item = {
            let owner = owner.clone();
            move |label: String, checked: bool, edit: Box<dyn Fn(&mut PlayerUi)>| {
                let owner = owner.clone();
                PopupMenuItem::new(label)
                    .checked(checked)
                    .on_click(move |_, _, cx| {
                        owner
                            .update(cx, |this, cx| {
                                edit(&mut this.player);
                                // The next animatic and movie start with these.
                                let burn_in = this.player.burn_in.clone();
                                if cx.has_global::<crate::app_state::AppSettings>() {
                                    crate::app_state::update_settings(cx, |s| {
                                        s.storyboard_burn_in = burn_in
                                    });
                                }
                                this.player.made = None;
                                this.refresh_picture(cx);
                                cx.notify();
                            })
                            .ok();
                    })
            }
        };
        let screens = audience::screens(cx);
        let mut menu = menu
            .item(item(
                "Burn-in".into(),
                on,
                Box::new(|p| p.burn_in_on = !p.burn_in_on),
            ))
            .item(item(
                "Timecode".into(),
                burn.timecode,
                Box::new(|p| p.burn_in.timecode = !p.burn_in.timecode),
            ))
            .item(item(
                "Scene".into(),
                burn.scene,
                Box::new(|p| p.burn_in.scene = !p.burn_in.scene),
            ))
            .item(item(
                "Panel".into(),
                burn.panel,
                Box::new(|p| p.burn_in.panel = !p.burn_in.panel),
            ));
        let caption = burn.caption.clone();
        let size = burn.size;
        let position = burn.position;
        let item_c = item.clone();
        menu = menu
            .submenu("Caption", window, cx, move |mut menu, _, _| {
                menu = menu.item(item_c(
                    "None".into(),
                    caption.is_none(),
                    Box::new(|p| p.burn_in.caption = None),
                ));
                for name in &captions {
                    let chosen = caption.as_deref() == Some(name.as_str());
                    let value = name.clone();
                    menu = menu.item(item_c(
                        name.clone(),
                        chosen,
                        Box::new(move |p| p.burn_in.caption = Some(value.clone())),
                    ));
                }
                menu
            })
            .item(item(
                "At the top".into(),
                position == BurnInPosition::Top,
                Box::new(|p| p.burn_in.position = BurnInPosition::Top),
            ))
            .item(item(
                "At the bottom".into(),
                position == BurnInPosition::Bottom,
                Box::new(|p| p.burn_in.position = BurnInPosition::Bottom),
            ));
        let item_s = item.clone();
        menu = menu
            .submenu("Text size", window, cx, move |mut menu, _, _| {
                for s in BURN_SIZES {
                    menu = menu.item(item_s(
                        format!("{s}% of the height"),
                        (size - s).abs() < 0.01,
                        Box::new(move |p| p.burn_in.size = s),
                    ));
                }
                menu
            })
            .separator();
        for screen in screens {
            let owner = owner.clone();
            menu = menu.item(
                PopupMenuItem::new(format!("Play full screen on {}", screen.label)).on_click(
                    move |_, _, cx| {
                        owner
                            .update(cx, |this, cx| {
                                this.playback_full_screen(Some(screen.id), cx)
                            })
                            .ok();
                    },
                ),
            );
        }
        let timer = owner.clone();
        menu.separator().item(
            PopupMenuItem::new("Panel Timer…").on_click(move |_, window, cx| {
                timer
                    .update(cx, |this, cx| this.open_panel_timer(window, cx))
                    .ok();
            }),
        )
    }

    /// View menu entries for playing storyboards.
    pub(super) fn playback_view_items(
        menu: PopupMenu,
        editor: &Entity<EditorView>,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        if editor.read(cx).editor.storyboard().is_none() {
            return menu;
        }
        let looping = editor.read(cx).transport.looping;
        let owner = editor.downgrade();
        let run = |label: &str, command: Playback| {
            let owner = owner.clone();
            PopupMenuItem::new(label.to_string()).on_click(move |_, _, cx| {
                owner
                    .update(cx, |this, cx| {
                        this.playback(command, cx);
                    })
                    .ok();
            })
        };
        let full = owner.clone();
        let timer = owner.clone();
        menu.item(run("Play / Pause Animatic", Playback::PlayPause))
            .item(run("Stop Animatic", Playback::Stop))
            .item(run("Loop Playback", Playback::ToggleLoop).checked(looping))
            .item(
                PopupMenuItem::new("Play Animatic Full Screen").on_click(move |_, _, cx| {
                    full.update(cx, |this, cx| this.playback_full_screen(None, cx))
                        .ok();
                }),
            )
            .item(
                PopupMenuItem::new("Panel Timer…").on_click(move |_, window, cx| {
                    timer
                        .update(cx, |this, cx| this.open_panel_timer(window, cx))
                        .ok();
                }),
            )
            .separator()
    }
}
