//! The storyboard Timeline: a dock under the Stage or Board with a time
//! ruler, the panel track (thumbnails sized by duration, scenes, transitions
//! at the cuts), audio tracks with clips and markers, the playhead and the
//! play range. Timing edits (ripple, roll, proportional retime,
//! transitions, markers and clips) use the shared timeline maths in core
//! and land through `edit_storyboard`, one Undo step per gesture or command.
//! The playhead and play range are the editor's `transport`, which the
//! player advances. Rendering lives in `storyboard_timeline_view.rs`, the
//! sound library in `storyboard_audio_library.rs`.
use super::*;
use crate::widgets::TrackBounds;
use emulsion_core::project::PageId;
use emulsion_core::storyboard::Storyboard;
use emulsion_core::timeline::{
    self as tl, AudioClip, AudioTrack, FrameRate, Marker, Timeline, Transition, audio::MAX_TRACKS,
};
use std::borrow::Cow;
use std::cell::Cell;

#[path = "storyboard_timeline_view.rs"]
mod view;

#[cfg(test)]
#[path = "storyboard_timeline_tests.rs"]
mod tests;

pub(crate) const MIN_ZOOM: f32 = 0.05;
pub(crate) const MAX_ZOOM: f32 = 48.;
/// Pointer distance within which drags snap, in screen pixels.
const SNAP_PX: f32 = 8.;
const DEFAULT_HEIGHT: f32 = 300.;

/// What an edge drag does to timing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EdgeMode {
    /// Change the panel's duration; later panels move (plain drag).
    Ripple,
    /// Move the cut, trading frames with the next panel (Alt-drag).
    Roll,
    /// Scale the selection's total duration proportionally (Shift-drag).
    Scale,
}

impl EdgeMode {
    pub(crate) fn from_modifiers(m: Modifiers) -> Self {
        if m.alt {
            Self::Roll
        } else if m.shift {
            Self::Scale
        } else {
            Self::Ripple
        }
    }
}

/// The part of an audio clip being dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClipPart {
    Body,
    Start,
    End,
    FadeIn,
    FadeOut,
}

/// A pointer gesture in progress.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TimelineDrag {
    Scrub,
    RangeIn,
    RangeOut,
    Edge {
        panel: PageId,
        mode: EdgeMode,
    },
    TransitionLength {
        panel: PageId,
    },
    Clip {
        track: usize,
        index: usize,
        part: ClipPart,
    },
    Marker {
        track: usize,
        index: usize,
    },
    Volume {
        track: usize,
    },
    ScrollBar {
        /// Pointer x minus the thumb's left edge, in pixels.
        grab: f32,
    },
    /// The dock's top edge.
    Resize {
        y: f32,
        height: f32,
    },
    /// The library preview's in or out point.
    PreviewPoint {
        out: bool,
    },
}

/// One timing change, applied to the board as a single Undo step (or to a
/// copy, to preview a drag).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TimingEdit {
    /// Set a panel's duration; the panels after it move (ripple).
    Duration { panel: PageId, frames: u32 },
    /// Move the cut after `panel` by `delta` frames (roll).
    Roll { panel: PageId, delta: i64 },
    /// Scale these panels to `total` frames together (retime, fit).
    Retime { panels: Vec<PageId>, total: u64 },
    /// The transition into a panel.
    Transition {
        panel: PageId,
        transition: Transition,
    },
    /// Move cuts onto audio markers within `tolerance` frames.
    SnapToMarkers { tolerance: u64 },
    /// Audio tracks, clips, markers and the sound library.
    Audio(Timeline),
}

impl TimingEdit {
    pub(crate) fn apply(&self, board: &mut Storyboard, layout: &[PageId]) -> Result<(), String> {
        match self {
            Self::Duration { panel, frames } => {
                if *frames == 0 {
                    return Err("A panel lasts at least one frame.".into());
                }
                board
                    .panels
                    .get_mut(panel)
                    .ok_or("Panel does not exist.")?
                    .frames = *frames;
            }
            Self::Roll { panel, delta } => {
                board.roll(layout, *panel, *delta)?;
            }
            Self::Retime { panels, total } => board.retime(panels, *total)?,
            Self::Transition { panel, transition } => {
                let p = board.panels.get_mut(panel).ok_or("Panel does not exist.")?;
                p.transition = *transition;
                p.transition.frames = transition.frames.min(p.frames);
            }
            Self::SnapToMarkers { tolerance } => board.snap_to_markers(layout, *tolerance)?,
            Self::Audio(timeline) => board.timeline = timeline.clone(),
        }
        // A transition plays inside its panel, so it shortens with it.
        for panel in board.panels.values_mut() {
            panel.transition.frames = panel.transition.frames.min(panel.frames);
        }
        Ok(())
    }
}

pub(crate) struct TimelineUi {
    /// The dock shows under the Stage or Board.
    pub(crate) open: bool,
    /// Screen pixels per frame.
    pub(crate) zoom: f32,
    /// Pixels scrolled from frame 0.
    pub(crate) scroll: f32,
    /// The ruler counts SMPTE timecode; otherwise frames.
    pub(crate) timecode: bool,
    /// Drags snap to cuts, markers and the playhead.
    pub(crate) snap: bool,
    pub(crate) height: f32,
    /// The selected audio track, clip and marker.
    pub(crate) track: Option<usize>,
    pub(crate) clip: Option<(usize, usize)>,
    pub(crate) marker: Option<(usize, usize)>,
    pub(crate) drag: Option<TimelineDrag>,
    /// The frame under the pointer when the drag began: dragged things
    /// move by the pointer's travel from here.
    pub(crate) origin: f64,
    /// The play range when the drag began.
    range_origin: Option<(u64, u64)>,
    /// What the drag would do, previewed until release.
    pub(crate) pending: Option<TimingEdit>,
    /// The duration overlay while dragging.
    pub(crate) overlay: Option<String>,
    /// The ruler lane, which every lane lines up with.
    pub(crate) lanes: TrackBounds,
    /// Each audio track's lane, for dragging clips between tracks.
    pub(crate) rows: Rc<RefCell<Vec<Option<Bounds<Pixels>>>>>,
    pub(crate) volume_tracks: Vec<TrackBounds>,
    pub(crate) scrollbar: TrackBounds,
    pub(crate) focus: Option<FocusHandle>,
    pub(crate) library: super::storyboard_audio_library::LibraryUi,
}

impl Default for TimelineUi {
    fn default() -> Self {
        Self {
            open: false,
            zoom: 4.,
            scroll: 0.,
            timecode: true,
            snap: true,
            height: DEFAULT_HEIGHT,
            track: None,
            clip: None,
            marker: None,
            drag: None,
            origin: 0.,
            range_origin: None,
            pending: None,
            overlay: None,
            lanes: Rc::new(Cell::new(None)),
            rows: Rc::default(),
            volume_tracks: Vec::new(),
            scrollbar: Rc::new(Cell::new(None)),
            focus: None,
            library: Default::default(),
        }
    }
}

/// A frame as the ruler shows it.
pub(crate) fn frame_label(rate: FrameRate, frame: u64, timecode: bool) -> String {
    if timecode {
        rate.timecode(frame)
    } else {
        frame.to_string()
    }
}

/// A duration as timecode and frames, such as `00:00:01:12 (36 f)`.
pub(crate) fn duration_label(rate: FrameRate, frames: u64) -> String {
    format!("{} ({frames} f)", rate.timecode(frames))
}

/// A typed duration: frames (`36` or `36f`), seconds (`1.5s`) or timecode
/// (`00:00:01:12`, or the trailing parts such as `1:12`).
pub(crate) fn parse_duration(text: &str, rate: FrameRate) -> Option<u64> {
    let text = text.trim();
    if text.contains([':', ';']) {
        let parts = text.split([':', ';']).count();
        if parts > 4 {
            return None;
        }
        let padded = format!("{}{text}", "00:".repeat(4 - parts));
        return rate.parse_timecode(&padded);
    }
    if let Some(seconds) = text.strip_suffix('s') {
        let v: f64 = seconds.trim().parse().ok()?;
        return (v.is_finite() && v >= 0.).then(|| rate.seconds_to_frames(v));
    }
    text.strip_suffix('f').unwrap_or(text).trim().parse().ok()
}

/// The nearest of `candidates` within `tolerance` of `frame`, or `frame`.
pub(crate) fn snap_frame(frame: i64, candidates: &[u64], tolerance: f64) -> i64 {
    candidates
        .iter()
        .map(|c| *c as i64)
        .filter(|c| (c - frame).abs() as f64 <= tolerance)
        .min_by_key(|c| (c - frame).abs())
        .unwrap_or(frame)
}

/// Frames a clip may last from its offset to the end of its sound.
fn clip_room(timeline: &Timeline, clip: &AudioClip, rate: FrameRate) -> u64 {
    let ms = timeline
        .assets
        .get(&clip.asset)
        .map_or(0, |a| a.duration_ms.saturating_sub(clip.offset_ms));
    rate.seconds_to_frames(ms as f64 / 1000.).max(1)
}

/// Keep fades inside the clip.
fn fit_fades(clip: &mut AudioClip) {
    clip.fade_in = clip.fade_in.min(clip.frames);
    clip.fade_out = clip.fade_out.min(clip.frames - clip.fade_in);
}

/// A clip moved, trimmed or faded by the pointer at `frame` (and to
/// `target` track when moved). `None` when the change cannot fit.
pub(crate) fn clip_edit(
    timeline: &Timeline,
    rate: FrameRate,
    (track, index): (usize, usize),
    part: ClipPart,
    frame: i64,
    target: usize,
) -> Option<Timeline> {
    let mut next = timeline.clone();
    let clips = &timeline.tracks.get(track)?.clips;
    let clip = clips.get(index)?.clone();
    let prev_end = index
        .checked_sub(1)
        .map_or(0, |i| clips[i].end())
        .min(clip.start);
    let next_start = clips.get(index + 1).map_or(u64::MAX, |c| c.start);
    let mut edited = clip.clone();
    match part {
        ClipPart::Body => {
            edited.start = frame.max(0) as u64;
            next.tracks[track].clips.remove(index);
            next.place(target, edited).ok()?;
            return Some(next);
        }
        ClipPart::Start => {
            let revealed = rate.seconds_to_frames(clip.offset_ms as f64 / 1000.);
            let low = prev_end.max(clip.start.saturating_sub(revealed)) as i64;
            let start = frame.clamp(low, clip.end() as i64 - 1) as u64;
            let shift_ms = |frames: u64| (rate.frames_to_seconds(frames) * 1000.).round() as u64;
            edited.offset_ms = if start >= clip.start {
                clip.offset_ms + shift_ms(start - clip.start)
            } else {
                clip.offset_ms.saturating_sub(shift_ms(clip.start - start))
            };
            edited.start = start;
            edited.frames = clip.end() - start;
            let asset_ms = timeline.assets.get(&clip.asset)?.duration_ms;
            if edited.offset_ms >= asset_ms {
                return None;
            }
        }
        ClipPart::End => {
            let room = clip_room(timeline, &clip, rate);
            let high = (clip.start + room).min(next_start) as i64;
            let end = frame.clamp(clip.start as i64 + 1, high) as u64;
            edited.frames = end - clip.start;
        }
        ClipPart::FadeIn => {
            let max = clip.frames - clip.fade_out;
            edited.fade_in = (frame - clip.start as i64).clamp(0, max as i64) as u64;
        }
        ClipPart::FadeOut => {
            let max = clip.frames - clip.fade_in;
            edited.fade_out = (clip.end() as i64 - frame).clamp(0, max as i64) as u64;
        }
    }
    fit_fades(&mut edited);
    next.tracks[track].clips[index] = edited;
    Some(next)
}

impl EditorView {
    pub(crate) fn timeline_open(&self) -> bool {
        self.timeline_ui.open && self.editor.storyboard().is_some()
    }

    pub(crate) fn toggle_storyboard_timeline(&mut self, cx: &mut Context<Self>) {
        if self.editor.storyboard().is_none() {
            return;
        }
        self.timeline_ui.open = !self.timeline_ui.open;
        cx.notify();
    }

    pub(super) fn timeline_focus(&mut self, cx: &mut Context<Self>) -> FocusHandle {
        self.timeline_ui
            .focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone()
    }

    fn timeline_layout(&self) -> Vec<PageId> {
        self.editor.page_list().iter().map(|m| m.id).collect()
    }

    pub(crate) fn timeline_rate(&self) -> FrameRate {
        self.editor
            .storyboard()
            .map_or(FrameRate::whole(24), |b| b.settings.frame_rate)
    }

    /// The board as the timeline shows it: with the dragged change applied.
    pub(crate) fn timeline_board(&self) -> Option<Cow<'_, Storyboard>> {
        let board = self.editor.storyboard()?;
        let Some(edit) = &self.timeline_ui.pending else {
            return Some(Cow::Borrowed(board));
        };
        let mut preview = board.clone();
        match edit.apply(&mut preview, &self.timeline_layout()) {
            Ok(()) => Some(Cow::Owned(preview)),
            Err(_) => Some(Cow::Borrowed(board)),
        }
    }

    /// Frames the timeline spans: the animatic or the last sound, whichever
    /// ends later.
    pub(crate) fn timeline_length(&self) -> u64 {
        self.editor.storyboard().map_or(0, |b| {
            b.animatic_frames(&self.timeline_layout())
                .max(b.timeline.end())
        })
    }

    /// Apply a timing change as one Undo step.
    pub(crate) fn timeline_commit(&mut self, edit: TimingEdit, cx: &mut Context<Self>) -> bool {
        if !self.prepare_page_action(cx) {
            return false;
        }
        let layout = self.timeline_layout();
        let ok = self.edit_board(|b| edit.apply(b, &layout), cx);
        self.clamp_timeline_selection();
        ok
    }

    /// Change the audio timeline as one Undo step.
    pub(crate) fn timeline_audio_edit(
        &mut self,
        edit: impl FnOnce(&mut Timeline, FrameRate) -> Result<(), String>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(board) = self.editor.storyboard() else {
            return false;
        };
        let mut timeline = board.timeline.clone();
        if let Err(error) = edit(&mut timeline, board.settings.frame_rate) {
            self.set_status(error, true, cx);
            return false;
        }
        self.timeline_commit(TimingEdit::Audio(timeline), cx)
    }

    /// Drop selections that no longer exist (after Undo or a delete).
    fn clamp_timeline_selection(&mut self) {
        let Some(board) = self.editor.storyboard() else {
            return;
        };
        let tracks = &board.timeline.tracks;
        let ui = &mut self.timeline_ui;
        ui.track = ui.track.filter(|t| *t < tracks.len());
        ui.clip = ui
            .clip
            .filter(|(t, c)| tracks.get(*t).is_some_and(|t| *c < t.clips.len()));
        ui.marker = ui
            .marker
            .filter(|(t, m)| tracks.get(*t).is_some_and(|t| *m < t.markers.len()));
    }

    // ── Geometry ──

    /// The frame under window x `x`, unrounded.
    pub(crate) fn timeline_frame_at(&self, x: Pixels) -> f64 {
        let origin = self
            .timeline_ui
            .lanes
            .get()
            .map_or(0., |b| f32::from(b.origin.x));
        ((f32::from(x) - origin + self.timeline_ui.scroll) / self.timeline_ui.zoom).max(0.) as f64
    }

    pub(crate) fn timeline_lane_width(&self) -> f32 {
        self.timeline_ui
            .lanes
            .get()
            .map_or(800., |b| f32::from(b.size.width))
    }

    /// Keep the scroll inside the timeline (with a little room after it).
    pub(crate) fn clamp_timeline_scroll(&mut self) {
        let content = (self.timeline_length() as f32 + 48.) * self.timeline_ui.zoom;
        let max = (content - self.timeline_lane_width() * 0.5).max(0.);
        self.timeline_ui.scroll = self.timeline_ui.scroll.clamp(0., max);
    }

    /// Zoom by `factor`, keeping window x `anchor` (or the playhead) still.
    pub(crate) fn timeline_zoom_by(
        &mut self,
        factor: f32,
        anchor: Option<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let ui = &self.timeline_ui;
        let origin = ui.lanes.get().map_or(0., |b| f32::from(b.origin.x));
        let at = anchor.map_or(self.transport.frame as f32 * ui.zoom - ui.scroll, |x| {
            f32::from(x) - origin
        });
        let frame = (at + ui.scroll) / ui.zoom;
        let zoom = (ui.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.timeline_ui.zoom = zoom;
        self.timeline_ui.scroll = frame * zoom - at;
        self.clamp_timeline_scroll();
        cx.notify();
    }

    /// Show the whole timeline.
    pub(crate) fn timeline_zoom_fit(&mut self, cx: &mut Context<Self>) {
        let frames = self.timeline_length().max(24) as f32;
        self.timeline_ui.zoom =
            ((self.timeline_lane_width() - 24.) / frames).clamp(MIN_ZOOM, MAX_ZOOM);
        self.timeline_ui.scroll = 0.;
        cx.notify();
    }

    /// Frames drags snap to, apart from `except`: cuts, markers, the
    /// playhead and the play range.
    fn timeline_snaps(&self, except: Option<u64>) -> Vec<u64> {
        let Some(board) = self.editor.storyboard() else {
            return Vec::new();
        };
        let layout = self.timeline_layout();
        let durations: Vec<u32> = board.playing(&layout).iter().map(|(_, f)| *f).collect();
        let mut out = tl::starts(&durations);
        out.extend(board.timeline.marker_frames());
        out.push(self.transport.frame);
        if let Some((a, b)) = self.transport.range {
            out.extend([a, b]);
        }
        out.retain(|f| Some(*f) != except);
        out
    }

    fn timeline_snap(&self, frame: i64, except: Option<u64>, modifiers: Modifiers) -> i64 {
        // Ctrl/Cmd turns snapping off for one drag.
        if !self.timeline_ui.snap || modifiers.secondary() {
            return frame;
        }
        let tolerance = f64::from(SNAP_PX / self.timeline_ui.zoom);
        snap_frame(frame, &self.timeline_snaps(except), tolerance)
    }

    // ── Playhead and range ──

    /// Move the playhead; the panel under it becomes the active panel.
    pub(crate) fn timeline_seek(&mut self, frame: u64, cx: &mut Context<Self>) {
        let total = self.timeline_length().max(1);
        self.transport.seek(frame, total);
        if !self.transport.playing
            && let Some(at) = self
                .editor
                .storyboard()
                .and_then(|b| b.animatic_frame(&self.timeline_layout(), self.transport.frame))
            && at.panel != self.editor.active_page()
        {
            self.select_page(at.panel, cx);
        }
        cx.notify();
    }

    /// Set the play range's in or out point at the playhead.
    pub(crate) fn timeline_set_range(&mut self, out: bool, cx: &mut Context<Self>) {
        let total = self.timeline_length().max(1);
        let frame = self.transport.frame;
        let (a, b) = self.transport.range.unwrap_or((0, total));
        self.transport.range = Some(if out {
            (a.min(frame), (frame + 1).max(a + 1))
        } else {
            (frame, b.max(frame + 1))
        });
        cx.notify();
    }

    pub(crate) fn timeline_clear_range(&mut self, cx: &mut Context<Self>) {
        self.transport.range = None;
        cx.notify();
    }

    // ── Pointer gestures ──

    /// Start a drag. Edge drags on locked panels are refused up front.
    pub(crate) fn timeline_begin(
        &mut self,
        drag: TimelineDrag,
        position: Point<Pixels>,
        modifiers: Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = self.timeline_focus(cx);
        window.focus(&focus, cx);
        if let TimelineDrag::Edge { panel, mode } = &drag
            && let Some(reason) = self.edge_refusal(*panel, *mode)
        {
            self.set_status(reason, true, cx);
            return;
        }
        if let TimelineDrag::TransitionLength { panel } = &drag
            && self
                .editor
                .storyboard()
                .is_some_and(|b| b.is_locked(*panel))
        {
            self.set_status("That panel is locked. Unlock it to change it.", true, cx);
            return;
        }
        self.timeline_ui.drag = Some(drag);
        self.timeline_ui.origin = self.timeline_frame_at(position.x);
        self.timeline_ui.range_origin = self.transport.range;
        self.timeline_ui.pending = None;
        self.timeline_ui.overlay = None;
        self.timeline_move(position, modifiers, cx);
    }

    /// Why an edge drag cannot change timing, if it cannot.
    fn edge_refusal(&self, panel: PageId, mode: EdgeMode) -> Option<String> {
        let board = self.editor.storyboard()?;
        let layout = self.timeline_layout();
        let playing = board.playing(&layout);
        let at = playing.iter().position(|(id, _)| *id == panel)?;
        let touched: Vec<PageId> = match mode {
            EdgeMode::Ripple => vec![panel],
            EdgeMode::Roll => playing[at..playing.len().min(at + 2)]
                .iter()
                .map(|(id, _)| *id)
                .collect(),
            EdgeMode::Scale => self.timeline_scale_set(panel),
        };
        if mode == EdgeMode::Roll && at + 1 >= playing.len() {
            return Some("The last panel has no cut after it to roll.".into());
        }
        touched
            .iter()
            .any(|id| board.is_locked(*id))
            .then(|| "That panel is locked. Unlock it to change its timing.".into())
    }

    /// Panels a Shift-drag on `panel`'s edge scales: the selection when it
    /// holds the panel, otherwise the panel alone. Thumbnail sheets do not
    /// play, so they are left out.
    fn timeline_scale_set(&self, panel: PageId) -> Vec<PageId> {
        let selection = self.board_selection();
        let Some(board) = self.editor.storyboard() else {
            return vec![panel];
        };
        if !selection.contains(&panel) {
            return vec![panel];
        }
        selection
            .into_iter()
            .filter(|id| board.panels.get(id).is_some_and(|p| p.thumbnails.is_none()))
            .collect()
    }

    pub(crate) fn timeline_move(
        &mut self,
        position: Point<Pixels>,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.timeline_ui.drag.clone() else {
            return;
        };
        let raw = self.timeline_frame_at(position.x);
        // `anchor` moved by the pointer's travel since the drag began.
        let travel = raw - self.timeline_ui.origin;
        let shift = |anchor: u64| (anchor as f64 + travel).round() as i64;
        let Some(board) = self.editor.storyboard() else {
            return;
        };
        let rate = board.settings.frame_rate;
        let layout = self.timeline_layout();
        match drag {
            TimelineDrag::Scrub => {
                self.timeline_seek(raw.floor() as u64, cx);
                return;
            }
            TimelineDrag::RangeIn | TimelineDrag::RangeOut => {
                let total = self.timeline_length().max(1);
                let (a, b) = self.transport.range.unwrap_or((0, total));
                let (a0, b0) = self.timeline_ui.range_origin.unwrap_or((0, total));
                let anchor = if drag == TimelineDrag::RangeIn {
                    a0
                } else {
                    b0
                };
                let frame = self
                    .timeline_snap(shift(anchor), Some(anchor), modifiers)
                    .max(0) as u64;
                self.transport.range = Some(if drag == TimelineDrag::RangeIn {
                    (frame.min(b - 1), b)
                } else {
                    (a, frame.clamp(a + 1, total))
                });
                self.timeline_ui.overlay = self.transport.range.map(|(a, b)| {
                    format!(
                        "In {} · Out {} · {}",
                        rate.timecode(a),
                        rate.timecode(b),
                        duration_label(rate, b - a)
                    )
                });
            }
            TimelineDrag::Edge { panel, mode } => {
                let starts = board.panel_starts(&layout);
                let Some(start) = starts.iter().find(|(id, _)| *id == panel).map(|(_, s)| *s)
                else {
                    return;
                };
                let frames = u64::from(board.panels[&panel].frames);
                let cut = start + frames;
                let target = self.timeline_snap(shift(cut), Some(cut), modifiers);
                let delta = target - cut as i64;
                let (edit, text) = match mode {
                    EdgeMode::Ripple => {
                        let next = (target - start as i64).max(1) as u32;
                        (
                            TimingEdit::Duration {
                                panel,
                                frames: next,
                            },
                            format!(
                                "Duration {} · {:+} f",
                                duration_label(rate, u64::from(next)),
                                i64::from(next) - frames as i64
                            ),
                        )
                    }
                    EdgeMode::Roll => {
                        let mut durations: Vec<u32> =
                            board.playing(&layout).iter().map(|(_, f)| *f).collect();
                        let at = board
                            .playing(&layout)
                            .iter()
                            .position(|(id, _)| *id == panel)
                            .unwrap_or(0);
                        let applied = tl::roll(&mut durations, at, delta);
                        (
                            TimingEdit::Roll {
                                panel,
                                delta: applied,
                            },
                            format!(
                                "Roll {applied:+} f · {} | {}",
                                rate.timecode(u64::from(durations[at])),
                                durations
                                    .get(at + 1)
                                    .map_or(String::new(), |d| rate.timecode(u64::from(*d)))
                            ),
                        )
                    }
                    EdgeMode::Scale => {
                        let panels = self.timeline_scale_set(panel);
                        let before: u64 = panels
                            .iter()
                            .map(|id| u64::from(board.panels[id].frames))
                            .sum();
                        let total = (before as i64 + delta).max(panels.len() as i64) as u64;
                        let text = format!(
                            "{} panel{} · {} ({:+.0}%)",
                            panels.len(),
                            if panels.len() == 1 { "" } else { "s" },
                            duration_label(rate, total),
                            (total as f64 / before.max(1) as f64 - 1.) * 100.
                        );
                        (TimingEdit::Retime { panels, total }, text)
                    }
                };
                self.timeline_ui.pending = Some(edit);
                self.timeline_ui.overlay = Some(text);
            }
            TimelineDrag::TransitionLength { panel } => {
                let starts = board.panel_starts(&layout);
                let Some(start) = starts.iter().find(|(id, _)| *id == panel).map(|(_, s)| *s)
                else {
                    return;
                };
                let p = &board.panels[&panel];
                let end = start + u64::from(p.transition.frames);
                let frames = (shift(end) - start as i64).clamp(1, i64::from(p.frames)) as u32;
                let mut transition = p.transition;
                transition.frames = frames;
                self.timeline_ui.pending = Some(TimingEdit::Transition { panel, transition });
                self.timeline_ui.overlay = Some(format!(
                    "{} · {}",
                    transition.label(),
                    duration_label(rate, u64::from(frames))
                ));
            }
            TimelineDrag::Clip { track, index, part } => {
                let timeline = &board.timeline;
                let Some(clip) = timeline.tracks.get(track).and_then(|t| t.clips.get(index)) else {
                    return;
                };
                let (frames, original) = (clip.frames as i64, clip.start);
                let mut target_track = track;
                let at = match part {
                    ClipPart::Body => {
                        let start = shift(original);
                        // Snap the start, or else the end, to cuts and markers.
                        let snapped = self.timeline_snap(start, Some(original), modifiers);
                        let at = if snapped != start {
                            snapped
                        } else {
                            self.timeline_snap(
                                start + frames,
                                Some(original + frames as u64),
                                modifiers,
                            ) - frames
                        };
                        target_track = self.timeline_track_at(position.y).unwrap_or(track);
                        at
                    }
                    ClipPart::Start => {
                        self.timeline_snap(shift(original), Some(original), modifiers)
                    }
                    ClipPart::End => {
                        self.timeline_snap(shift(clip.end()), Some(clip.end()), modifiers)
                    }
                    ClipPart::FadeIn => shift(original + clip.fade_in),
                    ClipPart::FadeOut => shift(clip.end() - clip.fade_out),
                };
                if let Some(next) =
                    clip_edit(timeline, rate, (track, index), part, at, target_track)
                {
                    let text = match part {
                        ClipPart::Body => {
                            format!("Start {}", frame_label(rate, at.max(0) as u64, true))
                        }
                        ClipPart::Start | ClipPart::End => format!(
                            "Clip {}",
                            duration_label(rate, next.tracks[track].clips[index].frames)
                        ),
                        ClipPart::FadeIn => format!(
                            "Fade in {}",
                            duration_label(rate, next.tracks[track].clips[index].fade_in)
                        ),
                        ClipPart::FadeOut => format!(
                            "Fade out {}",
                            duration_label(rate, next.tracks[track].clips[index].fade_out)
                        ),
                    };
                    self.timeline_ui.pending = Some(TimingEdit::Audio(next));
                    self.timeline_ui.overlay = Some(text);
                }
            }
            TimelineDrag::Marker { track, index } => {
                let mut next = board.timeline.clone();
                let Some(marker) = next
                    .tracks
                    .get_mut(track)
                    .and_then(|t| t.markers.get_mut(index))
                else {
                    return;
                };
                let original = marker.frame;
                let at = self
                    .timeline_snap(shift(original), Some(original), modifiers)
                    .max(0) as u64;
                marker.frame = at;
                self.timeline_ui.overlay = Some(format!("{} · {}", marker.name, rate.timecode(at)));
                self.timeline_ui.pending = Some(TimingEdit::Audio(next));
            }
            TimelineDrag::Volume { track } => {
                let Some(fraction) = self
                    .timeline_ui
                    .volume_tracks
                    .get(track)
                    .and_then(|bounds| crate::widgets::track_fraction(bounds, position.x))
                else {
                    return;
                };
                let mut next = board.timeline.clone();
                let Some(t) = next.tracks.get_mut(track) else {
                    return;
                };
                t.volume_db = volume_from_fraction(fraction);
                self.timeline_ui.overlay = Some(format!("{} · {:+.1} dB", t.name, t.volume_db));
                self.timeline_ui.pending = Some(TimingEdit::Audio(next));
            }
            TimelineDrag::ScrollBar { grab } => {
                if let Some(bar) = self.timeline_ui.scrollbar.get() {
                    let content = (self.timeline_length() as f32 + 48.) * self.timeline_ui.zoom;
                    let width = f32::from(bar.size.width).max(1.);
                    let left = f32::from(position.x - bar.origin.x) - grab;
                    self.timeline_ui.scroll = left / width * content.max(width);
                    self.clamp_timeline_scroll();
                }
            }
            TimelineDrag::Resize { y, height } => {
                self.timeline_ui.height = (height + y - f32::from(position.y)).clamp(140., 720.);
            }
            TimelineDrag::PreviewPoint { out } => {
                self.library_preview_drag(out, position.x);
            }
        }
        cx.notify();
    }

    /// The audio track whose lane holds window y `y`.
    fn timeline_track_at(&self, y: Pixels) -> Option<usize> {
        self.timeline_ui
            .rows
            .borrow()
            .iter()
            .position(|b| b.is_some_and(|b| y >= b.origin.y && y < b.origin.y + b.size.height))
    }

    /// Finish the drag: the previewed change lands as one Undo step.
    pub(crate) fn timeline_end(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.timeline_ui.drag.take() else {
            return;
        };
        self.timeline_ui.overlay = None;
        let Some(edit) = self.timeline_ui.pending.take() else {
            cx.notify();
            return;
        };
        // A moved clip stays selected where it landed.
        let moved = match (&drag, &edit) {
            (TimelineDrag::Clip { .. }, TimingEdit::Audio(next)) => {
                self.editor.storyboard().and_then(|b| {
                    let old: Vec<_> = b.timeline.tracks.iter().map(|t| &t.clips).collect();
                    next.tracks.iter().enumerate().find_map(|(t, track)| {
                        track
                            .clips
                            .iter()
                            .position(|c| old.get(t).is_none_or(|o| !o.contains(c)))
                            .map(|c| (t, c))
                    })
                })
            }
            _ => None,
        };
        if self.timeline_commit(edit, cx)
            && let Some(clip) = moved
        {
            self.timeline_ui.clip = Some(clip);
            self.timeline_ui.track = Some(clip.0);
        }
        cx.notify();
    }

    // ── Commands ──

    /// Set a panel's duration from typed text (frames, seconds or
    /// timecode); later panels move.
    pub(crate) fn timeline_set_duration(
        &mut self,
        panel: PageId,
        text: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let rate = self.timeline_rate();
        match parse_duration(text, rate).and_then(|f| u32::try_from(f).ok()) {
            Some(frames) if frames >= 1 => {
                self.timeline_commit(TimingEdit::Duration { panel, frames }, cx)
            }
            _ => {
                self.set_status(
                    "Type a duration in frames (36), seconds (1.5s) or timecode (00:00:01:12).",
                    true,
                    cx,
                );
                false
            }
        }
    }

    /// Fit the selected panels to a typed total duration, keeping their
    /// proportions.
    pub(crate) fn timeline_fit_selection(&mut self, text: &str, cx: &mut Context<Self>) -> bool {
        let rate = self.timeline_rate();
        let Some(total) = parse_duration(text, rate).filter(|f| *f > 0) else {
            self.set_status(
                "Type a duration in frames (240), seconds (10s) or timecode (00:00:10:00).",
                true,
                cx,
            );
            return false;
        };
        let first = self.board_selection()[0];
        let panels = self.timeline_scale_set(first);
        self.timeline_commit(TimingEdit::Retime { panels, total }, cx)
    }

    /// Move panel cuts onto nearby audio markers (within half a second).
    pub(crate) fn timeline_snap_cuts(&mut self, cx: &mut Context<Self>) {
        let rate = self.timeline_rate();
        if self
            .editor
            .storyboard()
            .is_some_and(|b| b.timeline.marker_frames().is_empty())
        {
            self.set_status("Add markers to an audio track first.", true, cx);
            return;
        }
        let tolerance = rate.timebase() / 2;
        if self.timeline_commit(TimingEdit::SnapToMarkers { tolerance }, cx) {
            self.set_status("Cuts snapped to markers.", false, cx);
        }
    }

    pub(crate) fn timeline_set_transition(
        &mut self,
        panel: PageId,
        transition: Transition,
        cx: &mut Context<Self>,
    ) -> bool {
        self.timeline_commit(TimingEdit::Transition { panel, transition }, cx)
    }

    /// Change a transition's kind, giving a cut a default length (half a
    /// second, within the panel).
    pub(crate) fn timeline_transition_kind(
        &mut self,
        panel: PageId,
        kind: tl::TransitionKind,
        cx: &mut Context<Self>,
    ) {
        let Some(board) = self.editor.storyboard() else {
            return;
        };
        let Some(p) = board.panels.get(&panel) else {
            return;
        };
        let half = (board.settings.frame_rate.timebase() / 2).max(1) as u32;
        let frames = if kind == tl::TransitionKind::Cut {
            0
        } else if p.transition.frames == 0 {
            half.min(p.frames)
        } else {
            p.transition.frames
        };
        self.timeline_set_transition(panel, Transition { kind, frames }, cx);
    }

    // ── Audio tracks and markers ──

    pub(crate) fn timeline_add_track(&mut self, cx: &mut Context<Self>) {
        let ok = self.timeline_audio_edit(
            |t, _| {
                if t.tracks.len() >= MAX_TRACKS {
                    return Err(format!("Use at most {MAX_TRACKS} audio tracks."));
                }
                let mut n = t.tracks.len() + 1;
                while t.tracks.iter().any(|tr| tr.name == format!("Audio {n}")) {
                    n += 1;
                }
                t.tracks.push(AudioTrack::new(&format!("Audio {n}")));
                Ok(())
            },
            cx,
        );
        if ok {
            self.timeline_ui.track = self
                .editor
                .storyboard()
                .map(|b| b.timeline.tracks.len() - 1);
        }
    }

    pub(crate) fn timeline_delete_track(&mut self, track: usize, cx: &mut Context<Self>) {
        if self.timeline_audio_edit(
            |t, _| {
                if track >= t.tracks.len() {
                    return Err("No track has that index.".into());
                }
                t.tracks.remove(track);
                Ok(())
            },
            cx,
        ) {
            self.timeline_ui.track = None;
            self.timeline_ui.clip = None;
            self.timeline_ui.marker = None;
        }
    }

    pub(crate) fn timeline_rename_track(
        &mut self,
        track: usize,
        name: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let name = name.trim().to_string();
        self.timeline_audio_edit(
            move |t, _| {
                t.tracks
                    .get_mut(track)
                    .ok_or("No track has that index.")?
                    .name = name;
                Ok(())
            },
            cx,
        )
    }

    pub(crate) fn timeline_toggle_track(
        &mut self,
        track: usize,
        solo: bool,
        cx: &mut Context<Self>,
    ) {
        self.timeline_audio_edit(
            |t, _| {
                let t = t.tracks.get_mut(track).ok_or("No track has that index.")?;
                if solo {
                    t.solo = !t.solo;
                } else {
                    t.muted = !t.muted;
                }
                Ok(())
            },
            cx,
        );
    }

    /// Add a marker at the playhead on `track` (the selected track, or the
    /// first, made if there is none).
    pub(crate) fn timeline_add_marker(&mut self, track: Option<usize>, cx: &mut Context<Self>) {
        let frame = self.transport.frame;
        let track = track.or(self.timeline_ui.track).unwrap_or(0);
        let mut added = None;
        self.timeline_audio_edit(
            |t, _| {
                if t.tracks.is_empty() {
                    t.tracks.push(AudioTrack::new("Audio 1"));
                }
                let tr = t.tracks.get_mut(track).ok_or("No track has that index.")?;
                let name = format!("Marker {}", tr.markers.len() + 1);
                tr.markers.push(Marker { frame, name });
                tr.markers.sort_by_key(|m| m.frame);
                added = tr.markers.iter().rposition(|m| m.frame == frame);
                Ok(())
            },
            cx,
        );
        if let Some(index) = added {
            self.timeline_ui.track = Some(track);
            self.timeline_ui.marker = Some((track, index));
        }
    }

    pub(crate) fn timeline_rename_marker(
        &mut self,
        (track, index): (usize, usize),
        name: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let name = name.trim().to_string();
        self.timeline_audio_edit(
            move |t, _| {
                t.tracks
                    .get_mut(track)
                    .and_then(|t| t.markers.get_mut(index))
                    .ok_or("That marker no longer exists.")?
                    .name = name;
                Ok(())
            },
            cx,
        )
    }

    pub(crate) fn timeline_delete_marker(
        &mut self,
        (track, index): (usize, usize),
        cx: &mut Context<Self>,
    ) {
        if self.timeline_audio_edit(
            |t, _| {
                let markers = &mut t
                    .tracks
                    .get_mut(track)
                    .ok_or("No track has that index.")?
                    .markers;
                if index >= markers.len() {
                    return Err("That marker no longer exists.".into());
                }
                markers.remove(index);
                Ok(())
            },
            cx,
        ) {
            self.timeline_ui.marker = None;
        }
    }

    // ── Clips ──

    pub(crate) fn timeline_rename_clip(
        &mut self,
        (track, index): (usize, usize),
        name: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let name = name.trim().to_string();
        self.timeline_audio_edit(
            move |t, _| {
                t.tracks
                    .get_mut(track)
                    .and_then(|t| t.clips.get_mut(index))
                    .ok_or("That clip no longer exists.")?
                    .name = name;
                Ok(())
            },
            cx,
        )
    }

    pub(crate) fn timeline_clip_gain(
        &mut self,
        (track, index): (usize, usize),
        text: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(db) = text
            .trim()
            .trim_end_matches("dB")
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|v| v.is_finite())
        else {
            self.set_status("Type the clip gain in dB, such as -6.", true, cx);
            return false;
        };
        self.timeline_audio_edit(
            move |t, _| {
                t.tracks
                    .get_mut(track)
                    .and_then(|t| t.clips.get_mut(index))
                    .ok_or("That clip no longer exists.")?
                    .gain_db = db;
                Ok(())
            },
            cx,
        )
    }

    pub(crate) fn timeline_delete_clip(
        &mut self,
        (track, index): (usize, usize),
        cx: &mut Context<Self>,
    ) {
        if self.timeline_audio_edit(
            |t, _| {
                let clips = &mut t
                    .tracks
                    .get_mut(track)
                    .ok_or("No track has that index.")?
                    .clips;
                if index >= clips.len() {
                    return Err("That clip no longer exists.".into());
                }
                clips.remove(index);
                Ok(())
            },
            cx,
        ) {
            self.timeline_ui.clip = None;
        }
    }

    /// Delete the selected clip, or else the selected marker.
    pub(crate) fn timeline_delete_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(clip) = self.timeline_ui.clip {
            self.timeline_delete_clip(clip, cx);
        } else if let Some(marker) = self.timeline_ui.marker {
            self.timeline_delete_marker(marker, cx);
        }
    }

    /// Keys while the timeline has focus that no shortcut takes: arrows
    /// step the playhead a frame (Shift: a second) and M adds a marker.
    /// Returns whether the key was used.
    pub(crate) fn timeline_key(
        &mut self,
        key: &str,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) -> bool {
        if modifiers.secondary() || modifiers.alt {
            return false;
        }
        let step = if modifiers.shift {
            self.timeline_rate().timebase()
        } else {
            1
        };
        match key {
            "left" => self.timeline_seek(self.transport.frame.saturating_sub(step), cx),
            "right" => self.timeline_seek(self.transport.frame + step, cx),
            "m" if !modifiers.shift => self.timeline_add_marker(None, cx),
            _ => return false,
        }
        true
    }
}

/// Track volume on the slider: −60 dB at the left, +24 dB at the right.
pub(crate) fn volume_from_fraction(fraction: f32) -> f32 {
    let (lo, hi) = (tl::audio::MIN_GAIN_DB, tl::audio::MAX_GAIN_DB);
    ((lo + (hi - lo) * fraction.clamp(0., 1.)) * 2.).round() / 2.
}

pub(crate) fn volume_fraction(db: f32) -> f32 {
    let (lo, hi) = (tl::audio::MIN_GAIN_DB, tl::audio::MAX_GAIN_DB);
    ((db - lo) / (hi - lo)).clamp(0., 1.)
}
