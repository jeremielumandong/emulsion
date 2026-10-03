//! Sketch Sprint (SB3): a timed drawing session. Each panel gets the same
//! time; when it runs out the Stage moves on to a new panel (or the next
//! one), until the session's panels are drawn. A countdown floats over the
//! Stage with Pause, Resume and Stop. Every stroke committed during the
//! session is recorded with its time and the panel's drawing after it, so
//! the session can be played back sped up as a time-lapse GIF or movie,
//! through the shared GIF and FFmpeg encoders.
//!
//! The clock is any monotonic count of seconds passed in by the caller, so
//! the timing is tested with a fake clock.
use super::*;
use emulsion_core::project::PageId;
use emulsion_core::project::mileage::Mileage;
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
};
use std::path::PathBuf;
use std::time::Duration;

#[cfg(test)]
#[path = "storyboard_sprint_tests.rs"]
mod tests;

/// Time per panel offered, in seconds.
pub(crate) const PER_PANEL_CHOICES: [f64; 6] = [15., 30., 60., 120., 300., 600.];
/// Session lengths offered, in panels.
pub(crate) const PANEL_CHOICES: [usize; 5] = [4, 6, 10, 20, 40];
/// Most drawings a session keeps for the time-lapse; later strokes still
/// count but add no frames.
const MAX_FRAMES: usize = 3000;
/// A time-lapse plays the session in about this many seconds (never
/// slower than real time).
const TIMELAPSE_SECONDS: f64 = 20.;
/// Movie time-lapses run at this rate.
const TIMELAPSE_FPS: u32 = 12;
/// The last picture holds this long at the end of a time-lapse.
const HOLD_END: f64 = 1.5;
/// Each finished panel shows at least this long before the next.
const PANEL_HOLD: f64 = 0.6;

/// What a session is set up to do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SprintPlan {
    /// Seconds per panel.
    pub per_panel: f64,
    pub panels: usize,
    /// Draw on new panels; otherwise on the panels after the active one
    /// (new ones are added at the end of the board).
    pub new_panels: bool,
}

impl Default for SprintPlan {
    fn default() -> Self {
        Self {
            per_panel: 30.,
            panels: 10,
            new_panels: true,
        }
    }
}

impl SprintPlan {
    pub fn total(&self) -> f64 {
        self.per_panel * self.panels as f64
    }
}

/// What the clock says now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tick {
    Running,
    /// The panel's time is up: move to the next one.
    NextPanel,
    /// The session is over.
    Finished,
}

/// One recorded moment: a panel's drawing at a time in the session.
#[derive(Clone)]
pub(crate) struct SprintFrame {
    pub panel: PageId,
    /// Seconds of drawing time (pauses left out).
    pub at: f64,
    pub doc: Document,
}

/// A session in progress or just finished.
pub(crate) struct Sprint {
    pub plan: SprintPlan,
    started: f64,
    paused_at: Option<f64>,
    paused_for: f64,
    /// The panels drawn, in order; the last is being drawn.
    pub panels: Vec<PageId>,
    pub frames: Vec<SprintFrame>,
    pub strokes: usize,
    pub ink: Mileage,
    /// Drawing time when the session ended.
    pub finished: Option<f64>,
}

/// The session in numbers, for the summary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SprintSummary {
    pub panels: usize,
    pub seconds: f64,
    pub strokes: usize,
    pub ink: Mileage,
}

impl Sprint {
    /// Start at `now` on `first`, whose drawing is `doc`.
    pub fn new(plan: SprintPlan, now: f64, first: PageId, doc: Document) -> Self {
        Self {
            plan,
            started: now,
            paused_at: None,
            paused_for: 0.,
            panels: vec![first],
            frames: vec![SprintFrame {
                panel: first,
                at: 0.,
                doc,
            }],
            strokes: 0,
            ink: Mileage::default(),
            finished: None,
        }
    }

    /// Drawing time so far, pauses left out.
    pub fn elapsed(&self, now: f64) -> f64 {
        if let Some(at) = self.finished {
            return at;
        }
        let now = self.paused_at.unwrap_or(now);
        (now - self.started - self.paused_for).max(0.)
    }

    /// The panel being drawn, counting from 0.
    pub fn index(&self) -> usize {
        self.panels.len() - 1
    }

    /// Seconds left on the panel being drawn.
    pub fn remaining(&self, now: f64) -> f64 {
        (self.plan.per_panel * (self.index() + 1) as f64 - self.elapsed(now)).max(0.)
    }

    pub fn paused(&self) -> bool {
        self.paused_at.is_some()
    }

    pub fn pause(&mut self, now: f64) {
        if self.paused_at.is_none() && self.finished.is_none() {
            self.paused_at = Some(now);
        }
    }

    pub fn resume(&mut self, now: f64) {
        if let Some(at) = self.paused_at.take() {
            self.paused_for += (now - at).max(0.);
        }
    }

    /// What should happen at `now`.
    pub fn tick(&self, now: f64) -> Tick {
        if self.finished.is_some() || self.paused() {
            return Tick::Running;
        }
        let elapsed = self.elapsed(now);
        if elapsed >= self.plan.total() {
            Tick::Finished
        } else if elapsed >= self.plan.per_panel * (self.index() + 1) as f64 {
            Tick::NextPanel
        } else {
            Tick::Running
        }
    }

    /// Move on to `panel`, whose drawing is `doc`.
    pub fn advance(&mut self, now: f64, panel: PageId, doc: Document) {
        let at = self.elapsed(now);
        self.panels.push(panel);
        self.keep(SprintFrame { panel, at, doc });
    }

    /// A stroke of `length` committed on `panel` at `now`, leaving `doc`.
    pub fn record(&mut self, now: f64, panel: PageId, ink: Mileage, doc: Document) {
        if self.finished.is_some() || self.paused() {
            return;
        }
        self.strokes += 1;
        self.ink = self.ink + ink;
        let at = self.elapsed(now);
        self.keep(SprintFrame { panel, at, doc });
    }

    fn keep(&mut self, frame: SprintFrame) {
        if self.frames.len() < MAX_FRAMES {
            self.frames.push(frame);
        }
    }

    pub fn finish(&mut self, now: f64) {
        if self.finished.is_none() {
            let at = self.elapsed(now).min(self.plan.total());
            self.paused_at = None;
            self.finished = Some(at);
        }
    }

    pub fn summary(&self, now: f64) -> SprintSummary {
        SprintSummary {
            panels: self.panels.len(),
            seconds: self.elapsed(now),
            strokes: self.strokes,
            ink: self.ink,
        }
    }
}

/// How many times faster than real time a session of `total` seconds
/// plays as a time-lapse.
pub(crate) fn timelapse_speed(total: f64) -> f64 {
    (total / TIMELAPSE_SECONDS).max(1.)
}

/// Each recorded moment's time on screen in a time-lapse, in seconds,
/// from each moment's drawing time and panel: the gap to the next moment
/// sped up (at least a twentieth of a second so every stroke shows), a
/// pause on each finished panel, and a hold on the last picture.
pub(crate) fn timelapse_delays(moments: &[(f64, PageId)], speed: f64) -> Vec<f64> {
    moments
        .iter()
        .enumerate()
        .map(|(i, (at, panel))| match moments.get(i + 1) {
            Some((next, next_panel)) => {
                let gap = ((next - at) / speed).clamp(0.05, 1.);
                if next_panel == panel {
                    gap
                } else {
                    gap.max(PANEL_HOLD)
                }
            }
            None => HOLD_END,
        })
        .collect()
}

/// The recorded moment showing on each frame of a movie at `fps`, playing
/// each moment for its time-lapse delay.
pub(crate) fn timelapse_movie(moments: &[(f64, PageId)], speed: f64, fps: f64) -> Vec<usize> {
    let mut out = Vec::new();
    let mut clock = 0.;
    for (i, delay) in timelapse_delays(moments, speed).into_iter().enumerate() {
        clock += delay;
        while (out.len() as f64) < (clock * fps).round() {
            out.push(i);
        }
    }
    out
}

fn moments(frames: &[SprintFrame]) -> Vec<(f64, PageId)> {
    frames.iter().map(|f| (f.at, f.panel)).collect()
}

/// A drawing as a time-lapse picture.
fn picture(doc: &Document) -> Result<image::RgbaImage, String> {
    let level = super::animation::level_for(doc.width, doc.height);
    let flat = emulsion_raster::composite::flatten(&doc.composite_tree(), level);
    super::animation::rgba_image(&flat).ok_or_else(|| "A time-lapse frame failed.".into())
}

/// Write the session's time-lapse as a GIF.
pub(crate) fn write_timelapse_gif(
    frames: &[SprintFrame],
    total: f64,
    path: &std::path::Path,
) -> Result<(), String> {
    let delays = timelapse_delays(&moments(frames), timelapse_speed(total));
    let pictures = frames.iter().zip(delays).map(|(frame, delay)| {
        let ms = (delay * 1000.).round() as u32;
        picture(&frame.doc).map(|p| (p, image::Delay::from_numer_denom_ms(ms, 1)))
    });
    emulsion_io::frame_export::encode_gif_frames(path, pictures, 10)
}

/// Write the session's time-lapse as an H.264 movie.
pub(crate) fn write_timelapse_movie(
    frames: &[SprintFrame],
    total: f64,
    path: &std::path::Path,
) -> Result<(), String> {
    use emulsion_io::video_export::{Codec, Encode, encode};
    let first = picture(&frames.first().ok_or("Nothing was drawn.")?.doc)?;
    let size = ((first.width() + 1) & !1, (first.height() + 1) & !1);
    let order = timelapse_movie(
        &moments(frames),
        timelapse_speed(total),
        f64::from(TIMELAPSE_FPS),
    );
    let settings = Encode {
        codec: Codec::H264,
        width: size.0.max(2),
        height: size.1.max(2),
        rate: emulsion_core::timeline::FrameRate::whole(TIMELAPSE_FPS),
        quality: 75,
        audio: None,
    };
    let mut shown: Option<(usize, Vec<u8>)> = None;
    let white = image::Rgba([255, 255, 255, 255]);
    encode(
        path,
        &settings,
        order.len() as u64,
        |n| {
            let i = order[n as usize];
            if shown.as_ref().is_none_or(|(at, _)| *at != i) {
                let p = picture(&frames[i].doc).map_err(anyhow::Error::msg)?;
                let fitted =
                    emulsion_io::frame_export::fit(&p, (settings.width, settings.height), white);
                shown = Some((i, fitted.into_raw()));
            }
            Ok(shown.as_ref().unwrap().1.clone())
        },
        &mut |_, _| {},
        &std::sync::atomic::AtomicBool::new(false),
    )
    .map_err(|e| e.to_string())
}

fn clock() -> f64 {
    static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    EPOCH.get_or_init(Instant::now).elapsed().as_secs_f64()
}

fn mmss(seconds: f64) -> String {
    let s = seconds.ceil().max(0.) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

fn choice_label(seconds: f64) -> String {
    if seconds >= 60. {
        format!("{} min", seconds / 60.)
    } else {
        format!("{seconds} s")
    }
}

impl EditorView {
    /// Show the Sketch Sprint setup over the Stage.
    pub(crate) fn open_sketch_sprint(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor.storyboard().is_none() || self.extras.sprint.is_some() {
            return;
        }
        if self.board_open() {
            self.toggle_storyboard_board(window, cx);
        }
        self.extras.sprint_setup = Some(self.extras.sprint_setup.unwrap_or_default());
        cx.notify();
    }

    /// Start the session set up, on a fresh panel or the active one.
    pub(crate) fn start_sketch_sprint(&mut self, cx: &mut Context<Self>) {
        let Some(plan) = self.extras.sprint_setup.take() else {
            return;
        };
        self.start_sprint_at(plan, clock(), cx);
        if self.extras.sprint.is_some() {
            self.extras.sprint_ticker = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(200))
                        .await;
                    if this.update(cx, |e, cx| e.sprint_tick(clock(), cx)).is_err() {
                        break;
                    }
                }
            }));
        }
    }

    /// Start `plan` at `now` (seconds on the sprint clock).
    pub(crate) fn start_sprint_at(&mut self, plan: SprintPlan, now: f64, cx: &mut Context<Self>) {
        if plan.new_panels && !self.sprint_next_panel(true, cx) {
            return;
        }
        if self.active_panel_locked() {
            self.set_status("This panel is locked. Unlock it to sprint on it.", true, cx);
            return;
        }
        let page = self.editor.active_page();
        let doc = self.editor.doc.clone();
        self.extras.sprint = Some(Sprint::new(plan, now, page, doc));
        cx.notify();
    }

    /// Move to a new panel after the active one (or the next panel, when
    /// `fresh` is off and there is one). False when that failed.
    fn sprint_next_panel(&mut self, fresh: bool, cx: &mut Context<Self>) -> bool {
        let before = self.editor.active_page();
        let layout: Vec<PageId> = self.editor.page_list().iter().map(|m| m.id).collect();
        let next = layout
            .iter()
            .position(|id| *id == before)
            .and_then(|i| layout.get(i + 1))
            .copied();
        match next.filter(|_| !fresh) {
            Some(next) => self.select_page(next, cx),
            None => self.add_project_page(false, cx),
        }
        self.editor.active_page() != before
    }

    /// Follow the clock: change panels or end the session.
    pub(crate) fn sprint_tick(&mut self, now: f64, cx: &mut Context<Self>) {
        let Some(sprint) = &self.extras.sprint else {
            return;
        };
        match sprint.tick(now) {
            Tick::Running => {}
            Tick::NextPanel => {
                let fresh = sprint.plan.new_panels;
                if self.sprint_next_panel(fresh, cx) {
                    let (page, doc) = (self.editor.active_page(), self.editor.doc.clone());
                    if let Some(sprint) = &mut self.extras.sprint {
                        sprint.advance(now, page, doc);
                    }
                } else {
                    self.finish_sketch_sprint(now, cx);
                    return;
                }
            }
            Tick::Finished => {
                self.finish_sketch_sprint(now, cx);
                return;
            }
        }
        cx.notify();
    }

    pub(crate) fn pause_sketch_sprint(&mut self, pause: bool, cx: &mut Context<Self>) {
        if let Some(sprint) = &mut self.extras.sprint {
            if pause {
                sprint.pause(clock());
            } else {
                sprint.resume(clock());
            }
        }
        cx.notify();
    }

    /// End the session and show its summary.
    pub(crate) fn finish_sketch_sprint(&mut self, now: f64, cx: &mut Context<Self>) {
        self.extras.sprint_ticker = None;
        if let Some(sprint) = &mut self.extras.sprint {
            sprint.finish(now);
        }
        cx.notify();
    }

    /// Record a stroke for the running session.
    pub(crate) fn sprint_record(&mut self, page: PageId, ink: Mileage, cx: &mut Context<Self>) {
        let doc = self.editor.doc.clone();
        if let Some(sprint) = &mut self.extras.sprint {
            sprint.record(clock(), page, ink, doc);
            cx.notify();
        }
    }

    /// Write the finished session's time-lapse where the person chooses.
    pub(crate) fn export_sprint_timelapse(&mut self, movie: bool, cx: &mut Context<Self>) {
        let Some(sprint) = &self.extras.sprint else {
            return;
        };
        let frames = sprint.frames.clone();
        let total = sprint.elapsed(clock());
        let extension = if movie { "mp4" } else { "gif" };
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let rx = cx.prompt_for_new_path(
            &home,
            Some(&format!("{}-sketch-sprint.{extension}", self.name)),
        );
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut path))) = rx.await else {
                return;
            };
            path.set_extension(extension);
            this.update(cx, |e, cx| {
                e.set_status("Rendering the Sketch Sprint time-lapse…", false, cx)
            })
            .ok();
            let out = path.clone();
            let result = cx
                .background_spawn(async move {
                    if movie {
                        write_timelapse_movie(&frames, total, &out)
                    } else {
                        write_timelapse_gif(&frames, total, &out)
                    }
                })
                .await;
            this.update(cx, |e, cx| match result {
                Ok(()) => e.set_status(format!("Exported {}.", path.display()), false, cx),
                Err(error) => e.set_status(format!("Time-lapse failed: {error}"), true, cx),
            })
            .ok();
        })
        .detach();
    }

    /// The setup card, countdown or summary over the Stage.
    pub(super) fn sprint_overlay(&self, p: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        let card = |id: &'static str| {
            div()
                .id(id)
                .test_support()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .flex()
                .flex_col()
                .gap_2()
                .p_3()
                .rounded(px(8.))
                .bg(p.panel.opacity(0.96))
                .border_1()
                .border_color(p.line)
                .text_size(px(12.))
                .text_color(p.ink)
        };
        let button = |id: SharedString, label: String, on: bool| {
            Button::new(id)
                .label(label)
                .xsmall()
                .when(on, |b| b.primary())
                .when(!on, |b| b.outline())
        };
        fn place(card: impl IntoElement) -> AnyElement {
            div()
                .absolute()
                .top_2()
                .right_2()
                .child(card)
                .into_any_element()
        }
        if let Some(plan) = self.extras.sprint_setup {
            let mut per = div().flex().flex_wrap().gap_1();
            for seconds in PER_PANEL_CHOICES {
                per = per.child(
                    button(
                        format!("sprint-per-{seconds}").into(),
                        choice_label(seconds),
                        plan.per_panel == seconds,
                    )
                    .on_click(cx.listener(move |e, _, _, cx| {
                        if let Some(plan) = &mut e.extras.sprint_setup {
                            plan.per_panel = seconds;
                        }
                        cx.notify();
                    })),
                );
            }
            let mut count = div().flex().flex_wrap().gap_1();
            for n in PANEL_CHOICES {
                count = count.child(
                    button(
                        format!("sprint-panels-{n}").into(),
                        n.to_string(),
                        plan.panels == n,
                    )
                    .on_click(cx.listener(move |e, _, _, cx| {
                        if let Some(plan) = &mut e.extras.sprint_setup {
                            plan.panels = n;
                        }
                        cx.notify();
                    })),
                );
            }
            return Some(place(
                card("sketch-sprint-setup")
                    .w(px(320.))
                    .child(label("Sketch Sprint", p))
                    .child(mono("Time per panel", 10., p.muted))
                    .child(per)
                    .child(mono("Panels", 10., p.muted))
                    .child(count)
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(
                                button("sprint-new".into(), "New panels".into(), plan.new_panels)
                                    .on_click(cx.listener(|e, _, _, cx| {
                                        if let Some(plan) = &mut e.extras.sprint_setup {
                                            plan.new_panels = true;
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button(
                                    "sprint-existing".into(),
                                    "From this panel on".into(),
                                    !plan.new_panels,
                                )
                                .on_click(cx.listener(
                                    |e, _, _, cx| {
                                        if let Some(plan) = &mut e.extras.sprint_setup {
                                            plan.new_panels = false;
                                        }
                                        cx.notify();
                                    },
                                )),
                            ),
                    )
                    .child(mono(format!("{} in all", mmss(plan.total())), 10., p.muted))
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(
                                button("sprint-start".into(), "Start".into(), true)
                                    .on_click(cx.listener(|e, _, _, cx| e.start_sketch_sprint(cx))),
                            )
                            .child(
                                button("sprint-cancel".into(), "Cancel".into(), false).on_click(
                                    cx.listener(|e, _, _, cx| {
                                        e.extras.sprint_setup = None;
                                        cx.notify();
                                    }),
                                ),
                            ),
                    ),
            ));
        }
        let sprint = self.extras.sprint.as_ref()?;
        let now = clock();
        if sprint.finished.is_some() {
            let s = sprint.summary(now);
            let ink = match s.ink.comparison() {
                Some(fun) => format!("{} of line ({fun})", s.ink.label()),
                None => format!("{} of line", s.ink.label()),
            };
            return Some(place(
                card("sketch-sprint-summary")
                    .w(px(320.))
                    .child(label("Sketch Sprint done", p))
                    .child(format!(
                        "{} panels in {} · {} strokes",
                        s.panels,
                        mmss(s.seconds),
                        s.strokes
                    ))
                    .child(ink)
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_1()
                            .child(
                                button("sprint-gif".into(), "Time-lapse GIF…".into(), true)
                                    .on_click(cx.listener(|e, _, _, cx| {
                                        e.export_sprint_timelapse(false, cx)
                                    })),
                            )
                            .child(
                                button("sprint-movie".into(), "Time-lapse movie…".into(), false)
                                    .on_click(cx.listener(|e, _, _, cx| {
                                        e.export_sprint_timelapse(true, cx)
                                    })),
                            )
                            .child(
                                button("sprint-close".into(), "Close".into(), false).on_click(
                                    cx.listener(|e, _, _, cx| {
                                        e.extras.sprint = None;
                                        cx.notify();
                                    }),
                                ),
                            ),
                    ),
            ));
        }
        let paused = sprint.paused();
        let remaining = sprint.remaining(now);
        Some(place(
            card("sketch-sprint-countdown")
                .child(
                    div()
                        .id("sketch-sprint-time")
                        .test_support()
                        .text_size(px(28.))
                        .text_color(if remaining <= 5. { p.accent } else { p.ink })
                        .child(mmss(remaining)),
                )
                .child(mono(
                    format!(
                        "Panel {} of {}{}",
                        sprint.index() + 1,
                        sprint.plan.panels,
                        if paused { " · paused" } else { "" }
                    ),
                    10.,
                    p.muted,
                ))
                .child(
                    div()
                        .flex()
                        .gap_1()
                        .child(
                            button(
                                "sprint-pause".into(),
                                if paused { "Resume" } else { "Pause" }.into(),
                                false,
                            )
                            .on_click(
                                cx.listener(move |e, _, _, cx| e.pause_sketch_sprint(!paused, cx)),
                            ),
                        )
                        .child(button("sprint-stop".into(), "Stop".into(), false).on_click(
                            cx.listener(|e, _, _, cx| e.finish_sketch_sprint(clock(), cx)),
                        )),
                ),
        ))
    }
}
