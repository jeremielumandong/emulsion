//! The storyboard as an animatic: panels laid end to end in time (thumbnail
//! sheets left out), the transition into each, timing edits on the shared
//! timeline maths, and what the player and movie export show on a frame —
//! the panels to blend, the burn-in text and the area to render.
use crate::project::PageId;
use crate::storyboard::{Frame, Storyboard};
use crate::timeline::{self, TransitionKind};
use serde::{Deserialize, Serialize};

/// What one animatic frame shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimaticFrame {
    pub panel: PageId,
    /// Frames into the panel.
    pub local: u64,
    /// While a transition plays: the panel before, the kind and progress.
    pub blend: Option<(PageId, TransitionKind, f32)>,
}

/// Where burn-in text sits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BurnInPosition {
    Top,
    #[default]
    Bottom,
}

/// Text drawn over playback and exported movies.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BurnIn {
    pub timecode: bool,
    pub scene: bool,
    pub panel: bool,
    /// A caption field to show, by name.
    pub caption: Option<String>,
    pub position: BurnInPosition,
    /// Text height as a percentage of the frame height.
    pub size: f32,
}

impl Default for BurnIn {
    fn default() -> Self {
        Self {
            timecode: true,
            scene: true,
            panel: true,
            caption: None,
            position: BurnInPosition::Bottom,
            size: 4.,
        }
    }
}

impl BurnIn {
    pub fn is_empty(&self) -> bool {
        !self.timecode && !self.scene && !self.panel && self.caption.is_none()
    }
    pub fn validate(&self) -> Result<(), String> {
        if !self.size.is_finite() || !(1. ..=20.).contains(&self.size) {
            return Err("Burn-in text is 1–20% of the frame height.".into());
        }
        if self
            .caption
            .as_ref()
            .is_some_and(|c| c.chars().count() > 200)
        {
            return Err("Caption field names are up to 200 characters.".into());
        }
        Ok(())
    }
}

/// Draw burn-in `lines` (from [`Storyboard::burn_in_lines`]) onto a
/// straight-alpha RGBA8 frame of `w` × `h`: white text, centred, on a dark
/// band at the top or bottom, sized by `burn.size`. Long lines wrap to the
/// frame width. The player and every export use this, so burn-in always
/// looks the same.
pub fn draw_burn_in(rgba: &mut [u8], w: u32, h: u32, lines: &[String], burn: &BurnIn) {
    use crate::text::{Align, TextSpec};
    if lines.is_empty() || w == 0 || h == 0 || rgba.len() < w as usize * h as usize * 4 {
        return;
    }
    let size = (h as f32 * burn.size.clamp(1., 20.) / 100.).max(6.);
    let pad = (size * 0.35).round().max(2.);
    let spec = TextSpec {
        text: lines.join("\n"),
        size,
        line_height: 1.2,
        color: [255, 255, 255, 255],
        align: Align::Center,
        x: pad,
        y: pad,
        width: Some((w as f32 - 2. * pad).max(1.)),
        ..TextSpec::default()
    }
    .sanitized();
    let text_h = crate::text::layout(&spec).bounds().height.max(size * 1.2);
    let band = ((text_h + 2. * pad).ceil() as u32).min(h);
    let top = match burn.position {
        BurnInPosition::Top => 0,
        BurnInPosition::Bottom => h - band,
    };
    let text = crate::text::rasterize(&spec, w, band).to_srgba8();
    let stride = w as usize * 4;
    for y in 0..band as usize {
        let row = &mut rgba[(top as usize + y) * stride..][..stride];
        let src = &text[y * stride..][..stride];
        for (dst, src) in row
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(src.as_chunks::<4>().0)
        {
            // The band: 55% black over the picture.
            for c in &mut dst[..3] {
                *c = (f32::from(*c) * 0.45).round() as u8;
            }
            dst[3] = dst[3].max(140);
            let a = f32::from(src[3]) / 255.;
            if a > 0. {
                for i in 0..3 {
                    dst[i] = (f32::from(src[i]) * a + f32::from(dst[i]) * (1. - a)).round() as u8;
                }
                dst[3] = (255. * a + f32::from(dst[3]) * (1. - a)).round() as u8;
            }
        }
    }
}

/// How much of each panel playback and exports show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderArea {
    /// The camera frame: the panel as drawn.
    #[default]
    Camera,
    /// The camera frame plus the board's overscan.
    Overscan,
    /// Everything drawn on any panel, so no artwork is cut off.
    AllArtwork,
}

impl Storyboard {
    /// Panels that play, in page order, with their durations. Thumbnail
    /// sheets are left out.
    pub fn playing(&self, layout: &[PageId]) -> Vec<(PageId, u32)> {
        layout
            .iter()
            .filter_map(|id| {
                let panel = self.panels.get(id)?;
                panel.thumbnails.is_none().then_some((*id, panel.frames))
            })
            .collect()
    }

    /// The animatic's length in frames.
    pub fn animatic_frames(&self, layout: &[PageId]) -> u64 {
        self.playing(layout)
            .iter()
            .map(|(_, f)| u64::from(*f))
            .sum()
    }

    /// The first frame of each playing panel.
    pub fn panel_starts(&self, layout: &[PageId]) -> Vec<(PageId, u64)> {
        let playing = self.playing(layout);
        let durations: Vec<_> = playing.iter().map(|(_, f)| *f).collect();
        playing
            .iter()
            .map(|(id, _)| *id)
            .zip(timeline::starts(&durations))
            .collect()
    }

    /// What frame `frame` of the animatic shows.
    pub fn animatic_frame(&self, layout: &[PageId], frame: u64) -> Option<AnimaticFrame> {
        let playing = self.playing(layout);
        let durations: Vec<_> = playing.iter().map(|(_, f)| *f).collect();
        let (index, local) = timeline::locate(&durations, frame)?;
        let panel = playing[index].0;
        let blend = (index > 0)
            .then(|| {
                let transition = self.panels[&panel].transition;
                transition
                    .progress(local)
                    .map(|t| (playing[index - 1].0, transition.kind, t))
            })
            .flatten();
        Some(AnimaticFrame {
            panel,
            local,
            blend,
        })
    }

    /// Set the playing panels `ids` (in page order) to `frames` each, as
    /// timed by a performance (the Panel Timer) or typed in a table.
    pub fn set_frames(&mut self, ids: &[PageId], frames: &[u32]) -> Result<(), String> {
        if ids.len() != frames.len() {
            return Err("Give one duration per panel.".into());
        }
        for (id, f) in ids.iter().zip(frames) {
            self.panels
                .get_mut(id)
                .ok_or("Panel does not exist.")?
                .frames = *f;
        }
        Ok(())
    }

    /// Scale the durations of `ids` so they add up to `total` frames,
    /// keeping their proportions (retime and fit to duration).
    pub fn retime(&mut self, ids: &[PageId], total: u64) -> Result<(), String> {
        let current: Vec<u32> = ids
            .iter()
            .map(|id| {
                self.panels
                    .get(id)
                    .map(|p| p.frames)
                    .ok_or("Panel does not exist.")
            })
            .collect::<Result<_, _>>()?;
        let next = timeline::retime(&current, total, 1)?;
        self.set_frames(ids, &next)
    }

    /// Move the cut after `panel` by `delta` frames, taking the frames from
    /// the next playing panel (a roll edit). Returns the delta applied.
    pub fn roll(&mut self, layout: &[PageId], panel: PageId, delta: i64) -> Result<i64, String> {
        let playing = self.playing(layout);
        let index = playing
            .iter()
            .position(|(id, _)| *id == panel)
            .ok_or("That panel does not play.")?;
        let mut durations: Vec<_> = playing.iter().map(|(_, f)| *f).collect();
        let applied = timeline::roll(&mut durations, index, delta);
        let ids: Vec<_> = playing.iter().map(|(id, _)| *id).collect();
        self.set_frames(&ids, &durations)?;
        Ok(applied)
    }

    /// Move panel cuts onto the audio markers within `tolerance` frames.
    pub fn snap_to_markers(&mut self, layout: &[PageId], tolerance: u64) -> Result<(), String> {
        let playing = self.playing(layout);
        let durations: Vec<_> = playing.iter().map(|(_, f)| *f).collect();
        let next = timeline::snap_to_markers(&durations, &self.timeline.marker_frames(), tolerance);
        let ids: Vec<_> = playing.iter().map(|(id, _)| *id).collect();
        self.set_frames(&ids, &next)
    }

    /// The burn-in lines for `frame`, top to bottom.
    pub fn burn_in_lines(&self, layout: &[PageId], frame: u64, burn_in: &BurnIn) -> Vec<String> {
        let Some(at) = self.animatic_frame(layout, frame) else {
            return Vec::new();
        };
        let mut head = Vec::new();
        if burn_in.scene {
            let scene = &self.scenes[&self.panels[&at.panel].scene].name;
            head.push(format!("Scene {scene}"));
        }
        if burn_in.panel {
            let index = self
                .playing(layout)
                .iter()
                .position(|(id, _)| *id == at.panel)
                .unwrap_or(0);
            head.push(format!("Panel {}", index + 1));
        }
        if burn_in.timecode {
            head.push(self.settings.frame_rate.timecode(frame));
        }
        let mut lines = Vec::new();
        if !head.is_empty() {
            lines.push(head.join("   "));
        }
        if let Some(text) = burn_in
            .caption
            .as_deref()
            .and_then(|name| self.caption(name))
            .and_then(|field| self.panels[&at.panel].captions.get(&field))
        {
            lines.extend(
                text.text
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(str::to_string),
            );
        }
        lines
    }

    /// The area of each panel that playback and exports show, in panel
    /// pixels. `artwork` lists the bounds of everything drawn on every panel
    /// (used by `AllArtwork`).
    pub fn render_area(&self, area: RenderArea, artwork: impl IntoIterator<Item = Frame>) -> Frame {
        let (w, h) = (self.settings.width, self.settings.height);
        let camera = Frame::centred(w, h, 100.);
        match area {
            RenderArea::Camera => camera,
            RenderArea::Overscan => self.stage.stage_area(w, h),
            RenderArea::AllArtwork => artwork.into_iter().fold(camera, |a, b| {
                let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
                let (x1, y1) = ((a.x + a.w).max(b.x + b.w), (a.y + a.h).max(b.y + b.h));
                Frame {
                    x: x0,
                    y: y0,
                    w: x1 - x0,
                    h: y1 - y0,
                }
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storyboard::{Settings, ThumbnailGrid};
    use crate::timeline::{AudioTrack, Edge, Marker, Transition};

    fn board() -> (Storyboard, Vec<PageId>) {
        let ids = vec![1, 2, 3, 4];
        let mut b = Storyboard::new(Settings::new(640, 360), &ids);
        for (id, f) in ids.iter().zip([24, 12, 36, 48]) {
            b.panels.get_mut(id).unwrap().frames = f;
        }
        (b, ids)
    }

    #[test]
    fn panels_play_end_to_end_without_thumbnail_sheets() {
        let (mut b, ids) = board();
        b.panels.get_mut(&3).unwrap().thumbnails = Some(ThumbnailGrid::new(2, 2));
        assert_eq!(b.animatic_frames(&ids), 84);
        assert_eq!(b.panel_starts(&ids), [(1, 0), (2, 24), (4, 36)]);
        assert_eq!(b.animatic_frame(&ids, 30).unwrap().panel, 2);
        assert_eq!(b.animatic_frame(&ids, 36).unwrap().panel, 4);
        assert!(b.animatic_frame(&ids, 84).is_none());
    }

    #[test]
    fn transitions_blend_from_the_panel_before() {
        let (mut b, ids) = board();
        b.panels.get_mut(&2).unwrap().transition = Transition {
            kind: TransitionKind::Wipe { from: Edge::Left },
            frames: 4,
        };
        let f = b.animatic_frame(&ids, 25).unwrap();
        assert_eq!(f.panel, 2);
        let (from, kind, t) = f.blend.unwrap();
        assert_eq!((from, kind), (1, TransitionKind::Wipe { from: Edge::Left }));
        assert!((t - 0.375).abs() < 1e-6);
        assert!(b.animatic_frame(&ids, 28).unwrap().blend.is_none());
        // The first panel has nothing to come from.
        b.panels.get_mut(&1).unwrap().transition.frames = 4;
        b.panels.get_mut(&1).unwrap().transition.kind = TransitionKind::Dissolve;
        assert!(b.animatic_frame(&ids, 0).unwrap().blend.is_none());
    }

    #[test]
    fn timing_edits_retime_roll_and_snap() {
        let (mut b, ids) = board();
        b.retime(&[1, 2], 72).unwrap();
        assert_eq!((b.panels[&1].frames, b.panels[&2].frames), (48, 24));
        assert_eq!(b.roll(&ids, 1, 6).unwrap(), 6);
        assert_eq!((b.panels[&1].frames, b.panels[&2].frames), (54, 18));
        let mut track = AudioTrack::new("Dialogue");
        track.markers.push(Marker {
            frame: 50,
            name: "Line".into(),
        });
        b.timeline.tracks.push(track);
        b.snap_to_markers(&ids, 6).unwrap();
        assert_eq!(b.panels[&1].frames, 50);
        assert!(b.set_frames(&[1], &[]).is_err());
    }

    #[test]
    fn burn_in_and_render_area() {
        let (mut b, ids) = board();
        let action = b.caption("Action").unwrap();
        b.panels
            .get_mut(&2)
            .unwrap()
            .captions
            .insert(action, "Mia runs\nShe stops".into());
        let burn = BurnIn {
            caption: Some("action".into()),
            ..BurnIn::default()
        };
        let lines = b.burn_in_lines(&ids, 25, &burn);
        assert_eq!(lines[0], "Scene 1   Panel 2   00:00:01:01");
        assert!(lines[1].starts_with("Mia runs"));
        assert!(b.burn_in_lines(&ids, 999, &burn).is_empty());
        assert!(
            BurnIn {
                size: 0.,
                ..BurnIn::default()
            }
            .validate()
            .is_err()
        );
        let camera = b.render_area(RenderArea::Camera, []);
        assert_eq!((camera.w, camera.h), (640., 360.));
        let over = b.render_area(RenderArea::Overscan, []);
        assert!(over.w > 640. && over.x < 0.);
        let all = b.render_area(
            RenderArea::AllArtwork,
            [Frame {
                x: -50.,
                y: 10.,
                w: 20.,
                h: 20.,
            }],
        );
        assert_eq!((all.x, all.w), (-50., 690.));
    }

    #[test]
    fn burn_in_draws_a_band_with_text() {
        let (w, h) = (160u32, 90u32);
        let grey = [128u8, 128, 128, 255];
        let mut frame: Vec<u8> = grey.repeat((w * h) as usize);
        let burn = BurnIn {
            size: 10.,
            ..BurnIn::default()
        };
        draw_burn_in(&mut frame, w, h, &["Scene 1".into()], &burn);
        let px = |x: u32, y: u32| &frame[((y * w + x) * 4) as usize..][..4];
        assert_eq!(px(0, 0), grey, "the top is untouched");
        assert!(px(0, h - 1)[0] < 128, "the band darkens the bottom");
        let bright = (h / 2..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .filter(|&(x, y)| px(x, y)[0] > 200)
            .count();
        assert!(bright > 10, "white text is drawn");
        let mut top = grey.repeat((w * h) as usize);
        draw_burn_in(
            &mut top,
            w,
            h,
            &["A".into()],
            &BurnIn {
                position: BurnInPosition::Top,
                ..burn
            },
        );
        assert!(top[0] < 128 && top[((h - 1) * w * 4) as usize] == 128);
        let before = top.clone();
        draw_burn_in(&mut top, w, h, &[], &BurnIn::default());
        assert_eq!(top, before, "no lines draw nothing");
    }
}
