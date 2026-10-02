//! The Panel Timer (T9): time panels live while performing. Each tap of
//! Space or T ends the current panel; the take is then reviewed in a table
//! of old and new durations (editable) and applied as one Undo step, either
//! to the selected panels in order or as new blank panels after the
//! selection. A selected thumbnail sheet can be converted to panels first,
//! which are then timed in order. Sound can play while timing, and the
//! microphone can record (T4): the take lands on a new audio track from
//! the first timed panel when the timing is applied.
use super::*;
use crate::playback::recorder::{Recorded, Recording};
use emulsion_core::project::{PageId, ProjectEditor};
use emulsion_core::storyboard::{MAX_PANEL_FRAMES, Panel};
use emulsion_core::timeline::FrameRate;
use gpui_kit::component::{
    Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
};
use std::time::Duration;

#[cfg(test)]
#[path = "panel_timer_tests.rs"]
mod tests;

/// What a take times.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TimerTarget {
    /// These playing panels, in page order.
    Panels(Vec<PageId>),
    /// New blank panels after this one.
    New { after: PageId },
}

/// The taps of one take, in seconds from its start.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Take {
    started: Option<f64>,
    taps: Vec<f64>,
    /// The take ends by itself after this many panels.
    limit: Option<usize>,
}

impl Take {
    pub fn new(limit: Option<usize>) -> Self {
        Self {
            limit,
            ..Self::default()
        }
    }

    /// A tap at `now` (seconds on any clock): the first starts the take,
    /// each later one ends a panel. True once the take is complete.
    pub fn tap(&mut self, now: f64) -> bool {
        match self.started {
            None => self.started = Some(now),
            Some(start) => self.taps.push((now - start).max(0.)),
        }
        self.limit.is_some_and(|n| self.taps.len() >= n)
    }

    /// Panels timed so far.
    pub fn count(&self) -> usize {
        self.taps.len()
    }

    /// Each timed panel's duration in frames. Cuts land on the nearest frame
    /// of the running time, so rounding never adds up, and every panel
    /// keeps at least one frame.
    pub fn durations(&self, rate: FrameRate) -> Vec<u32> {
        let mut previous = 0u64;
        self.taps
            .iter()
            .map(|t| {
                let cut = rate.seconds_to_frames(*t).max(previous + 1);
                let frames = (cut - previous).min(u64::from(MAX_PANEL_FRAMES)) as u32;
                previous = cut;
                frames
            })
            .collect()
    }
}

/// Apply timed `frames` to `target` as one Undo step. Returns the panels
/// timed or made.
pub(crate) fn apply_timings(
    project: &mut ProjectEditor,
    target: &TimerTarget,
    frames: &[u32],
) -> Result<Vec<PageId>, String> {
    if frames.is_empty() {
        return Err("Tap at least once to time a panel.".into());
    }
    if let Some(bad) = frames.iter().find(|f| !(1..=MAX_PANEL_FRAMES).contains(*f)) {
        return Err(format!(
            "{bad} frames: panels are 1 to {MAX_PANEL_FRAMES} frames long."
        ));
    }
    match target {
        TimerTarget::Panels(ids) => {
            let n = frames.len().min(ids.len());
            project.edit_storyboard(|b| b.set_frames(&ids[..n], &frames[..n]))?;
            Ok(ids[..n].to_vec())
        }
        TimerTarget::New { after } => {
            let board = project
                .storyboard()
                .ok_or("This is not a storyboard project.")?;
            let blank = board.blank_panel()?;
            // Numbered on from the next panel name, as the Board names them.
            let scene = board.panels.get(after).map(|p| p.scene);
            let count = if board.naming.panels_per_scene {
                board
                    .panels
                    .values()
                    .filter(|p| Some(p.scene) == scene)
                    .count()
            } else {
                board.panels.len()
            };
            let items = frames
                .iter()
                .enumerate()
                .map(|(i, f)| (board.naming.panel_name(count + 1 + i), Panel::new(0, *f)))
                .collect();
            project.insert_panels(Some(*after), &blank, items, None)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Ready,
    Timing,
    Review,
}

pub(crate) struct PanelTimer {
    owner: WeakEntity<EditorView>,
    focus: FocusHandle,
    /// The selected playing panels, and a selected thumbnail sheet.
    selection: Vec<PageId>,
    sheet: Option<PageId>,
    /// Time the selection (otherwise make new panels).
    on_selection: bool,
    after: PageId,
    rate: FrameRate,
    phase: Phase,
    take: Take,
    epoch: Instant,
    /// Review rows: name, old duration and the new duration's input.
    rows: Vec<(String, Option<u32>, Entity<InputState>)>,
    sound: bool,
    /// Record the microphone while timing; the recording in progress, and
    /// the finished take waiting for Apply.
    record: bool,
    recording: Option<Recording>,
    recorded: Option<Recorded>,
    error: Option<String>,
    _ticker: Option<Task<()>>,
}

impl PanelTimer {
    fn new(e: &EditorView, owner: WeakEntity<EditorView>, focus: FocusHandle) -> Self {
        let mut timer = Self {
            owner,
            focus,
            selection: Vec::new(),
            sheet: None,
            on_selection: true,
            after: 0,
            rate: FrameRate::whole(24),
            phase: Phase::Ready,
            take: Take::default(),
            epoch: Instant::now(),
            rows: Vec::new(),
            sound: true,
            record: false,
            recording: None,
            recorded: None,
            error: None,
            _ticker: None,
        };
        timer.read_selection(e);
        timer.on_selection = timer.selection.len() > 1;
        timer
    }

    /// Take the selection from the editor.
    fn read_selection(&mut self, e: &EditorView) {
        let Some(board) = e.editor.storyboard() else {
            return;
        };
        let chosen = e.board_selection();
        self.rate = board.settings.frame_rate;
        self.after = *chosen.last().unwrap_or(&e.editor.active_page());
        self.sheet = chosen
            .iter()
            .copied()
            .find(|id| board.panels.get(id).is_some_and(|p| p.thumbnails.is_some()));
        self.selection = chosen
            .into_iter()
            .filter(|id| board.panels.get(id).is_some_and(|p| p.thumbnails.is_none()))
            .collect();
    }

    fn target(&self) -> TimerTarget {
        if self.on_selection {
            TimerTarget::Panels(self.selection.clone())
        } else {
            TimerTarget::New { after: self.after }
        }
    }

    fn now(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64()
    }

    /// A tap of the timing key at `now`.
    pub(crate) fn tap(&mut self, now: f64, window: &mut Window, cx: &mut Context<Self>) {
        match self.phase {
            Phase::Ready => {
                if self.on_selection && self.selection.is_empty() {
                    self.error = Some("Select the panels to time on the Board first.".into());
                    cx.notify();
                    return;
                }
                if self.record {
                    let device = super::storyboard_recording::audio_input(cx);
                    match Recording::start(device.as_deref()) {
                        Ok(recording) => self.recording = Some(recording),
                        Err(error) => {
                            self.error = Some(error);
                            cx.notify();
                            return;
                        }
                    }
                }
                let limit = self.on_selection.then_some(self.selection.len());
                self.take = Take::new(limit);
                self.take.tap(now);
                self.phase = Phase::Timing;
                self.error = None;
                self.start_sound(cx);
                // Redraw the running time while timing.
                self._ticker = Some(cx.spawn(async move |this, cx| {
                    loop {
                        cx.background_executor()
                            .timer(Duration::from_millis(100))
                            .await;
                        if this.update(cx, |_, cx| cx.notify()).is_err() {
                            break;
                        }
                    }
                }));
            }
            Phase::Timing => {
                if self.take.tap(now) {
                    self.review(window, cx);
                }
            }
            Phase::Review => {}
        }
        cx.notify();
    }

    fn start_sound(&self, cx: &mut Context<Self>) {
        if !self.sound {
            return;
        }
        let target = self.target();
        self.owner
            .update(cx, |e, cx| {
                let layout: Vec<PageId> = e.editor.page_list().iter().map(|m| m.id).collect();
                let start = e.editor.storyboard().and_then(|b| {
                    let starts = b.panel_starts(&layout);
                    match &target {
                        TimerTarget::Panels(ids) => starts
                            .iter()
                            .find(|(id, _)| Some(id) == ids.first())
                            .map(|(_, f)| *f),
                        TimerTarget::New { .. } => Some(e.transport.frame),
                    }
                });
                e.playback_sound(start, cx);
            })
            .ok();
    }

    fn stop_sound(&self, cx: &mut Context<Self>) {
        self.owner
            .update(cx, |e, cx| e.playback_sound(None, cx))
            .ok();
    }

    /// End the take and show the table.
    fn review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.stop_sound(cx);
        if let Some(recording) = self.recording.take() {
            match recording.stop() {
                Ok(recorded) => self.recorded = Some(recorded),
                Err(error) => self.error = Some(error),
            }
        }
        self._ticker = None;
        self.phase = Phase::Review;
        let durations = self.take.durations(self.rate);
        let (names, old) = self.row_labels(durations.len(), cx);
        self.rows = durations
            .iter()
            .zip(names.into_iter().zip(old))
            .map(|(f, (name, old))| {
                let input = cx.new(|cx| InputState::new(window, cx).default_value(f.to_string()));
                (name, old, input)
            })
            .collect();
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn row_labels(&self, n: usize, cx: &App) -> (Vec<String>, Vec<Option<u32>>) {
        let Some(owner) = self.owner.upgrade() else {
            return (vec![String::new(); n], vec![None; n]);
        };
        let e = owner.read(cx);
        match self.target() {
            TimerTarget::Panels(ids) => ids
                .iter()
                .take(n)
                .map(|id| {
                    let name = e
                        .editor
                        .page_list()
                        .iter()
                        .find(|m| m.id == *id)
                        .map_or_else(String::new, |m| m.name.clone());
                    let old = e
                        .editor
                        .storyboard()
                        .and_then(|b| b.panels.get(id))
                        .map(|p| p.frames);
                    (name, old)
                })
                .unzip(),
            TimerTarget::New { .. } => (1..=n).map(|i| (format!("New panel {i}"), None)).unzip(),
        }
    }

    /// The durations typed in the table.
    fn typed(&self, cx: &App) -> Result<Vec<u32>, String> {
        self.rows
            .iter()
            .map(|(name, _, input)| {
                input
                    .read(cx)
                    .value()
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| format!("{name}: type a whole number of frames."))
            })
            .collect()
    }

    pub(crate) fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let frames = match self.typed(cx) {
            Ok(frames) => frames,
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                return;
            }
        };
        let target = self.target();
        let recorded = self.recorded.take();
        let result = self
            .owner
            .update(cx, |e, cx| {
                if !e.prepare_page_action(cx) {
                    return Err("Finish the current edit first.".to_string());
                }
                let made = apply_timings(&mut e.editor, &target, &frames)?;
                e.after_change(cx);
                // The recording started with the first timed panel.
                let layout: Vec<PageId> = e.editor.page_list().iter().map(|m| m.id).collect();
                let start = e.editor.storyboard().and_then(|b| {
                    b.panel_starts(&layout)
                        .into_iter()
                        .find(|(id, _)| Some(id) == made.first())
                        .map(|(_, f)| f)
                });
                if let (Some(recorded), Some(start)) = (recorded, start) {
                    e.place_recording(recorded, start, None, cx);
                }
                let what = if matches!(target, TimerTarget::New { .. }) {
                    "Added"
                } else {
                    "Timed"
                };
                let n = made.len();
                e.set_status(
                    format!("{what} {n} panel{}.", if n == 1 { "" } else { "s" }),
                    false,
                    cx,
                );
                e.set_board_selection(made);
                Ok(())
            })
            .unwrap_or(Ok(()));
        match result {
            Ok(()) => window.close_dialog(cx),
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    fn retake(&mut self, cx: &mut Context<Self>) {
        self.recording = None;
        self.recorded = None;
        self.phase = Phase::Ready;
        self.take = Take::default();
        self.rows.clear();
        self.error = None;
        cx.notify();
    }

    fn convert_sheet(&mut self, cx: &mut Context<Self>) {
        let Some(sheet) = self.sheet else {
            return;
        };
        let result = self
            .owner
            .update(cx, |e, cx| {
                if !e.prepare_page_action(cx) {
                    return Err("Finish the current edit first.".to_string());
                }
                let made = e.editor.convert_thumbnails(sheet)?;
                e.after_change(cx);
                e.set_board_selection(made);
                Ok(())
            })
            .unwrap_or(Ok(()));
        match result {
            Ok(()) => {
                if let Some(owner) = self.owner.upgrade() {
                    self.read_selection(owner.read(cx));
                }
                self.on_selection = true;
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }

    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let plain = !event.keystroke.modifiers.modified();
        match (self.phase, key) {
            (Phase::Ready | Phase::Timing, "space" | "t") if plain => {
                let now = self.now();
                self.tap(now, window, cx);
            }
            (Phase::Timing, "escape" | "enter") => self.review(window, cx),
            (Phase::Ready | Phase::Review, "escape") => {
                self.stop_sound(cx);
                window.close_dialog(cx);
            }
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl Render for PanelTimer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let rate = self.rate;
        let timing = self.phase == Phase::Timing;
        let mode = |id: &'static str, label: String, on: bool, disabled: bool| {
            Button::new(id)
                .label(label)
                .small()
                .disabled(disabled || timing)
                .when(on, |b| b.primary())
                .when(!on, |b| b.outline())
        };
        let mut body = div()
            .id("panel-timer")
            .test_support()
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .gap_2()
            .text_size(px(12.))
            .text_color(p.ink)
            .on_key_down(cx.listener(Self::key))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        mode(
                            "timer-selection",
                            format!("Time the selected panels ({})", self.selection.len()),
                            self.on_selection,
                            self.selection.is_empty(),
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.on_selection = true;
                            this.retake(cx);
                        })),
                    )
                    .child(
                        mode(
                            "timer-new",
                            "Create new panels".into(),
                            !self.on_selection,
                            false,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.on_selection = false;
                            this.retake(cx);
                        })),
                    ),
            );
        body = match self.phase {
            Phase::Ready => body
                .child(if self.on_selection {
                    "Press Space or T to start, then tap at the end of each panel. The take ends after the last selected panel, or press Esc."
                } else {
                    "Press Space or T to start, then tap at the end of each new panel. Press Esc when done; new blank panels go after the selection."
                })
                .when_some(self.sheet, |d, _| {
                    d.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child("A thumbnail sheet is selected: convert it to panels to time them in order.")
                            .child(
                                Button::new("timer-convert")
                                    .label("Convert sheet to panels")
                                    .small()
                                    .outline()
                                    .on_click(cx.listener(|this, _, _, cx| this.convert_sheet(cx))),
                            ),
                    )
                })
                .child(
                    Checkbox::new("timer-sound")
                        .label("Play the sound while timing")
                        .checked(self.sound)
                        .on_click(cx.listener(|this, value: &bool, _, cx| {
                            this.sound = *value;
                            cx.notify();
                        })),
                )
                .child(
                    Checkbox::new("timer-record")
                        .label("Record from the microphone while timing (onto a new track)")
                        .checked(self.record)
                        .on_click(cx.listener(|this, value: &bool, _, cx| {
                            this.record = *value;
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("timer-start")
                        .label("Start (Space)")
                        .small()
                        .primary()
                        .on_click(cx.listener(|this, _, window, cx| {
                            let now = this.now();
                            this.tap(now, window, cx);
                            window.focus(&this.focus, cx);
                        })),
                ),
            Phase::Timing => {
                let elapsed = self.take.started.map_or(0., |s| self.now() - s);
                let panel = self.take.count() + 1;
                let of = match self.target() {
                    TimerTarget::Panels(ids) => format!(" of {}", ids.len()),
                    TimerTarget::New { .. } => String::new(),
                };
                body.child(
                    div()
                        .id("timer-running")
                        .test_support()
                        .text_size(px(20.))
                        .font_family(MONO_FONT)
                        .child(format!(
                            "Panel {panel}{of} · {}",
                            rate.timecode(rate.seconds_to_frames(elapsed))
                        )),
                )
                .child("Tap Space or T at each cut. Esc ends the take.")
                .when_some(self.recording.as_ref(), |d, r| {
                    d.child(
                        div()
                            .id("timer-recording")
                            .test_support()
                            .text_color(p.accent)
                            .child(format!("● Recording from {}", r.device)),
                    )
                })
                .child(
                    Button::new("timer-tap")
                        .label("Tap")
                        .small()
                        .primary()
                        .on_click(cx.listener(|this, _, window, cx| {
                            let now = this.now();
                            this.tap(now, window, cx);
                            window.focus(&this.focus, cx);
                        })),
                )
            }
            Phase::Review => {
                let mut table = div().flex().flex_col().gap_1().child(
                    div()
                        .flex()
                        .gap_2()
                        .text_color(p.muted)
                        .child(div().w(px(180.)).child("Panel"))
                        .child(div().w(px(110.)).child("Old"))
                        .child("New (frames)"),
                );
                for (name, old, input) in &self.rows {
                    let old = old.map_or_else(
                        || "—".to_string(),
                        |f| format!("{f} ({:.2} s)", rate.frames_to_seconds(u64::from(f))),
                    );
                    table = table.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().w(px(180.)).child(name.clone()))
                            .child(div().w(px(110.)).child(old))
                            .child(div().w(px(100.)).child(Input::new(input).small())),
                    );
                }
                let recorded = self.recorded.as_ref().map(|r| r.seconds);
                body.child(table)
                    .when_some(recorded, |d, seconds| {
                        d.child(format!(
                            "Recorded {seconds:.1} s of sound: Apply places it on a new audio track from the first timed panel."
                        ))
                    })
                    .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            Button::new("timer-apply")
                                .label("Apply")
                                .small()
                                .primary()
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.apply(window, cx)),
                                ),
                        )
                        .child(
                            Button::new("timer-retake")
                                .label("Retake")
                                .small()
                                .outline()
                                .on_click(cx.listener(|this, _, _, cx| this.retake(cx))),
                        ),
                )
            }
        };
        body.when_some(self.error.clone(), |d, error| {
            d.child(
                div()
                    .id("timer-error")
                    .test_support()
                    .text_color(p.accent)
                    .child(error),
            )
        })
    }
}

impl EditorView {
    /// Open the Panel Timer on the Board selection (or the active panel).
    pub(crate) fn open_panel_timer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor.storyboard().is_none() {
            return;
        }
        self.playback_stop(cx);
        let focus = cx.focus_handle();
        let timer = PanelTimer::new(self, cx.weak_entity(), focus.clone());
        let timer = cx.new(|_| timer);
        let closing = timer.downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let closing = closing.clone();
            dialog
                .title("Panel Timer")
                .width(px(560.))
                .keyboard(false)
                .on_close(move |_, _, cx| {
                    if let Some(timer) = closing.upgrade() {
                        timer.update(cx, |t, cx| t.stop_sound(cx));
                    }
                })
                .child(timer.clone())
        });
        window.focus(&focus, cx);
    }
}
