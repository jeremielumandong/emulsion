//! Recording sound into the Timeline (T4, T13): **Record** on the Timeline
//! toolbar records the microphone chosen in Settings → Storyboard (or the
//! system default) from the playhead, with a level meter, while the
//! animatic plays; **Stop** imports the take into the sound library (as
//! `audio/{id}.wav` in the `.emu`, like any imported sound) and places it
//! on the chosen track, or a new one, as one Undo step. The Panel Timer
//! records through the same [`EditorView::place_recording`].
use super::storyboard_player::Playback;
use super::*;
use crate::playback::recorder::{Recorded, Recording};
use emulsion_core::timeline::{AudioClip, AudioTrack, audio::MAX_TRACKS};
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
use std::time::Duration;

/// The library folder recordings go into.
pub(crate) const RECORDINGS_FOLDER: &str = "Recordings";

/// A recording on the Timeline toolbar.
struct Active {
    recording: Recording,
    /// Where the take starts on the timeline.
    start: u64,
    track: Option<usize>,
    _ticker: Task<()>,
}

#[derive(Default)]
pub(crate) struct RecordingUi {
    active: Option<Active>,
    /// Record onto this track; a new track when `None`.
    pub(crate) target: Option<usize>,
    /// The meter's last reading, 0–1.
    level: f32,
}

/// The input device chosen in Settings, or `None` for the system default.
pub(crate) fn audio_input(cx: &App) -> Option<String> {
    crate::app_state::settings(cx)
        .storyboard
        .audio_input
        .clone()
}

/// "Recording N", numbered after the recordings already in the library.
pub(crate) fn recording_name(timeline: &emulsion_core::timeline::Timeline) -> String {
    let n = timeline
        .assets
        .values()
        .filter(|a| a.name.starts_with("Recording "))
        .count();
    format!("Recording {}", n + 1)
}

/// Place a recorded `asset` at `start`: on `track` when the take fits
/// there, otherwise on a new track. Returns the track and clip index.
pub(crate) fn place_take(
    timeline: &mut emulsion_core::timeline::Timeline,
    rate: emulsion_core::timeline::FrameRate,
    mut asset: emulsion_core::timeline::AudioAsset,
    start: u64,
    track: Option<usize>,
) -> Result<(usize, usize), String> {
    asset.name = recording_name(timeline);
    asset.folder = RECORDINGS_FOLDER.into();
    let clip = AudioClip {
        name: asset.name.clone(),
        start,
        frames: rate
            .seconds_to_frames(asset.duration_ms as f64 / 1000.)
            .max(1),
        ..AudioClip::default()
    };
    let id = timeline.add_asset(asset)?;
    let clip = AudioClip { asset: id, ..clip };
    let fits = |t: &AudioTrack| {
        !t.clips
            .iter()
            .any(|c| clip.start < c.end() && c.start < clip.end())
    };
    let track = match track.filter(|t| timeline.tracks.get(*t).is_some_and(fits)) {
        Some(track) => track,
        None => {
            if timeline.tracks.len() >= MAX_TRACKS {
                return Err(format!(
                    "The take overlaps a clip and there is no room for another track (at most {MAX_TRACKS})."
                ));
            }
            let mut n = timeline.tracks.len() + 1;
            while timeline
                .tracks
                .iter()
                .any(|t| t.name == format!("Voice {n}"))
            {
                n += 1;
            }
            timeline.tracks.push(AudioTrack::new(&format!("Voice {n}")));
            timeline.tracks.len() - 1
        }
    };
    timeline.place(track, clip)?;
    let index = timeline.tracks[track]
        .clips
        .iter()
        .position(|c| c.start == start)
        .unwrap_or(0);
    Ok((track, index))
}

impl EditorView {
    pub(crate) fn timeline_recording(&self) -> bool {
        self.recording_ui.active.is_some()
    }

    /// Record or stop.
    pub(crate) fn timeline_record_toggle(&mut self, cx: &mut Context<Self>) {
        if self.timeline_recording() {
            self.timeline_record_stop(cx);
        } else {
            self.timeline_record_start(cx);
        }
    }

    /// Start recording at the playhead; the animatic plays along.
    pub(crate) fn timeline_record_start(&mut self, cx: &mut Context<Self>) {
        if self.editor.storyboard().is_none() || self.timeline_recording() {
            return;
        }
        let recording = match Recording::start(audio_input(cx).as_deref()) {
            Ok(recording) => recording,
            Err(error) => {
                self.set_status(error, true, cx);
                return;
            }
        };
        let start = self.transport.frame;
        let total = self.timeline_length().max(1);
        if !self.playback_playing() && self.transport.start_frame(total) == start {
            self.playback(Playback::PlayPause, cx);
        }
        let ticker = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let alive = this.update(cx, |e, cx| {
                    let Some(active) = &e.recording_ui.active else {
                        return false;
                    };
                    if let Some(error) = active.recording.error() {
                        e.set_status(error, true, cx);
                        e.timeline_record_stop(cx);
                        return false;
                    }
                    let level = active.recording.meter.take();
                    e.recording_ui.level = level.max(e.recording_ui.level * 0.8);
                    cx.notify();
                    true
                });
                if !matches!(alive, Ok(true)) {
                    break;
                }
            }
        });
        self.set_status(format!("Recording from {}…", recording.device), false, cx);
        self.recording_ui.active = Some(Active {
            recording,
            start,
            track: self.recording_ui.target,
            _ticker: ticker,
        });
        cx.notify();
    }

    /// Stop recording and place the take.
    pub(crate) fn timeline_record_stop(&mut self, cx: &mut Context<Self>) {
        let Some(active) = self.recording_ui.active.take() else {
            return;
        };
        self.recording_ui.level = 0.;
        if self.playback_playing() {
            self.playback_pause(cx);
        }
        match active.recording.stop() {
            Ok(recorded) => self.place_recording(recorded, active.start, active.track, cx),
            Err(error) => self.set_status(error, true, cx),
        }
        cx.notify();
    }

    /// Import a recorded file into the sound library and place it at
    /// `start` on `track` (a new track when `None` or when it does not
    /// fit), as one Undo step. The recorded file goes when `recorded` is
    /// dropped, after the import has copied it.
    pub(crate) fn place_recording(
        &mut self,
        recorded: Recorded,
        start: u64,
        track: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        self.set_status("Adding the recording…", false, cx);
        cx.spawn(async move |this, cx| {
            let path = recorded.path.clone();
            let imported = cx
                .background_spawn(async move {
                    emulsion_io::audio::store::import(&path, RECORDINGS_FOLDER)
                        .map_err(|e| e.to_string())
                })
                .await;
            this.update(cx, |e, cx| {
                let asset = match imported {
                    Ok(asset) => asset,
                    Err(error) => {
                        e.set_status(error, true, cx);
                        return;
                    }
                };
                let mut placed = None;
                if e.timeline_audio_edit(
                    |t, rate| {
                        placed = Some(place_take(t, rate, asset, start, track)?);
                        Ok(())
                    },
                    cx,
                ) && let Some((track, clip)) = placed
                {
                    e.timeline_ui.track = Some(track);
                    e.timeline_ui.clip = Some((track, clip));
                    e.set_status(format!("Recorded {:.1} s.", recorded.seconds), false, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Record / Stop, the target track and the level meter.
    pub(super) fn timeline_record_controls(
        &mut self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let recording = self.recording_ui.active.as_ref();
        let elapsed = recording.map(|a| a.recording.elapsed());
        let tracks: Vec<String> = self.editor.storyboard().map_or(Vec::new(), |b| {
            b.timeline.tracks.iter().map(|t| t.name.clone()).collect()
        });
        let target = self.recording_ui.target.filter(|t| *t < tracks.len());
        let target_label = target.map_or_else(|| "New track".to_string(), |t| tracks[t].clone());
        let owner = cx.weak_entity();
        let level = self.recording_ui.level;
        div()
            .id("timeline-record")
            .test_support()
            .flex()
            .items_center()
            .gap_1()
            .child(
                Button::new("timeline-record-button")
                    .label(if recording.is_some() {
                        "■ Stop"
                    } else {
                        "● Record"
                    })
                    .tooltip("Record from the microphone at the playhead (Settings → Storyboard chooses the input)")
                    .xsmall()
                    .when(recording.is_some(), |b| b.danger())
                    .when(recording.is_none(), |b| b.ghost())
                    .on_click(cx.listener(|this, _, _, cx| this.timeline_record_toggle(cx))),
            )
            .child(
                Button::new("timeline-record-target")
                    .label(format!("into {target_label} ▾"))
                    .xsmall()
                    .ghost()
                    .dropdown_menu(move |menu, _, _| {
                        let pick = |label: String, value: Option<usize>| {
                            let owner = owner.clone();
                            PopupMenuItem::new(label)
                                .checked(value == target)
                                .on_click(move |_, _, cx| {
                                    owner
                                        .update(cx, |e, cx| {
                                            e.recording_ui.target = value;
                                            cx.notify();
                                        })
                                        .ok();
                                })
                        };
                        let mut menu = menu.item(pick("New track".into(), None));
                        for (i, name) in tracks.iter().enumerate() {
                            menu = menu.item(pick(name.clone(), Some(i)));
                        }
                        menu
                    }),
            )
            .when_some(elapsed, |d, seconds| {
                d.child(
                    div()
                        .id("timeline-record-meter")
                        .test_support()
                        .w(px(60.))
                        .h(px(6.))
                        .rounded(px(2.))
                        .bg(p.line)
                        .child(
                            div()
                                .h_full()
                                .rounded(px(2.))
                                .w(px(60. * level.sqrt()))
                                .bg(if level > 0.9 { p.accent } else { rgb(0x3EA15A).into() }),
                        ),
                )
                .child(
                    div()
                        .font_family(MONO_FONT)
                        .text_size(px(11.))
                        .text_color(p.muted)
                        .child(format!("{seconds:.1} s")),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use emulsion_core::timeline::{AudioAsset, FrameRate, Timeline};

    fn asset(ms: u64) -> AudioAsset {
        AudioAsset {
            name: "1-abc".into(),
            format: "wav".into(),
            duration_ms: ms,
            sample_rate: 48_000,
            channels: 1,
            folder: String::new(),
            source: None,
        }
    }

    #[test]
    fn takes_land_on_the_chosen_track_or_a_new_one() {
        let rate = FrameRate::whole(24);
        let mut t = Timeline::default();
        let (track, clip) = place_take(&mut t, rate, asset(2000), 10, None).unwrap();
        assert_eq!((track, clip), (0, 0));
        assert_eq!(t.tracks[0].name, "Voice 1");
        let c = &t.tracks[0].clips[0];
        assert_eq!(
            (c.start, c.frames, c.name.as_str()),
            (10, 48, "Recording 1")
        );
        let sound = &t.assets[&c.asset];
        assert_eq!(
            (sound.name.as_str(), sound.folder.as_str()),
            ("Recording 1", "Recordings")
        );
        // On the chosen track when it fits there.
        assert_eq!(
            place_take(&mut t, rate, asset(1000), 100, Some(0)).unwrap(),
            (0, 1)
        );
        // Overlapping a clip: onto a new track.
        assert_eq!(
            place_take(&mut t, rate, asset(1000), 20, Some(0)).unwrap(),
            (1, 0)
        );
        assert_eq!(t.tracks[1].clips[0].name, "Recording 3");
        t.validate().unwrap();
    }
}
