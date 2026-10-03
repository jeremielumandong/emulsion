//! Reference video on a timeline: video files kept with the document (beside
//! the sounds, sharing their ID allocator), tracks of clips placed in frames
//! with a trim, opacity, visibility and lock, which picture shows on a
//! frame, and where a reference picture sits over a frame (fitted over it
//! with opacity, or a picture-in-picture inset). Clips stay at their
//! timeline frames when panels are retimed, as audio clips do.
use super::FrameRate;
use super::audio::{AssetId, AudioAsset, AudioClip, AudioTrack, MAX_TRACKS, Timeline};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

pub const MAX_VIDEO_TRACKS: usize = 4;
pub const MAX_VIDEO_CLIPS: usize = 1024;
pub const MAX_VIDEOS: usize = 256;
/// Largest video picture side accepted, in pixels.
pub const MAX_VIDEO_SIDE: u32 = 16_384;

fn yes() -> bool {
    true
}

fn is_true(v: &bool) -> bool {
    *v
}

fn is_one(v: &f32) -> bool {
    *v == 1.
}

fn one() -> f32 {
    1.
}

/// A video file kept with the document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VideoAsset {
    pub name: String,
    /// File extension of the stored bytes (mp4, mov, mkv, webm…).
    pub format: String,
    pub duration_ms: u64,
    /// Pictures per second of the first video stream.
    pub fps: f64,
    pub width: u32,
    pub height: u32,
    /// Whether the file also has sound.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub has_audio: bool,
    /// Where the bytes are on disk while the document is open. Filled when a
    /// file is imported or a document is opened; never saved.
    #[serde(skip)]
    pub source: Option<PathBuf>,
}

impl VideoAsset {
    /// The number of pictures in the file.
    pub fn frame_count(&self) -> u64 {
        ((self.duration_ms as f64 / 1000. * self.fps).floor() as u64).max(1)
    }

    /// The picture showing `ms` into the file: the last one starting at or
    /// before it.
    pub fn frame_index(&self, ms: f64) -> u64 {
        let index = (ms.max(0.) / 1000. * self.fps + 1e-6).floor() as u64;
        index.min(self.frame_count() - 1)
    }
}

/// A stretch of a video placed on a track.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VideoClip {
    pub asset: AssetId,
    pub name: String,
    /// First timeline frame.
    pub start: u64,
    /// Length on the timeline, in frames.
    pub frames: u64,
    /// Where in the video the clip starts (its in point), in milliseconds.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub offset_ms: u64,
    /// 0–1, how strongly the picture shows over the panels.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub opacity: f32,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub visible: bool,
    /// Locked clips refuse moves, trims and deletion.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub locked: bool,
}

fn is_zero(v: &u64) -> bool {
    *v == 0
}

impl VideoClip {
    /// The whole of `asset` from timeline frame `start`.
    pub fn new(id: AssetId, asset: &VideoAsset, start: u64, rate: FrameRate) -> Self {
        Self {
            asset: id,
            name: asset.name.clone(),
            start,
            frames: rate
                .seconds_to_frames(asset.duration_ms as f64 / 1000.)
                .max(1),
            offset_ms: 0,
            opacity: 1.,
            visible: true,
            locked: false,
        }
    }
    pub fn end(&self) -> u64 {
        self.start + self.frames
    }
    /// Milliseconds into the video that timeline frame `frame` shows.
    pub fn source_ms(&self, frame: u64, rate: FrameRate) -> f64 {
        self.offset_ms as f64 + rate.frames_to_seconds(frame.saturating_sub(self.start)) * 1000.
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VideoTrack {
    pub name: String,
    /// In time order, never overlapping.
    #[serde(default)]
    pub clips: Vec<VideoClip>,
}

impl VideoTrack {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            clips: Vec::new(),
        }
    }
}

/// The reference picture a timeline frame shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoAt {
    pub track: usize,
    pub clip: usize,
    pub asset: AssetId,
    /// The picture of the asset.
    pub index: u64,
    pub opacity: f32,
}

/// How a reference picture sits over the animatic frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoPlacement {
    /// Fitted over the whole frame, with the clip's opacity.
    #[default]
    Overlay,
    /// A small inset at the bottom right.
    PictureInPicture,
}

/// The rectangle `[x, y, w, h]` a `vw` × `vh` picture covers within a
/// `fw` × `fh` frame: fitted inside it (letterboxed) as an overlay, or a
/// third of its width inset from the bottom-right corner.
pub fn placement_rect(placement: VideoPlacement, fw: f64, fh: f64, vw: f64, vh: f64) -> [f64; 4] {
    let (vw, vh) = (vw.max(1.), vh.max(1.));
    let (bw, bh, margin) = match placement {
        VideoPlacement::Overlay => (fw, fh, 0.),
        VideoPlacement::PictureInPicture => (fw / 3., fh / 3., fw.min(fh) * 0.03),
    };
    let k = (bw / vw).min(bh / vh);
    let (w, h) = (vw * k, vh * k);
    match placement {
        VideoPlacement::Overlay => [(fw - w) / 2., (fh - h) / 2., w, h],
        VideoPlacement::PictureInPicture => [fw - w - margin, fh - h - margin, w, h],
    }
}

/// Draw a straight-alpha RGBA8 `src` (`sw` × `sh`) over a straight-alpha
/// `dst` frame (`fw` × `fh`, any channel order shared with `src`) at
/// `rect` (from [`placement_rect`]), scaled by nearest pixel, at `opacity`.
pub fn draw_picture(
    dst: &mut [u8],
    (fw, fh): (u32, u32),
    src: &[u8],
    (sw, sh): (u32, u32),
    rect: [f64; 4],
    opacity: f32,
) {
    if sw == 0
        || sh == 0
        || src.len() < sw as usize * sh as usize * 4
        || dst.len() < fw as usize * fh as usize * 4
    {
        return;
    }
    let [rx, ry, rw, rh] = rect;
    if rw <= 0. || rh <= 0. {
        return;
    }
    let x0 = rx.round().clamp(0., f64::from(fw)) as u32;
    let y0 = ry.round().clamp(0., f64::from(fh)) as u32;
    let x1 = (rx + rw).round().clamp(0., f64::from(fw)) as u32;
    let y1 = (ry + rh).round().clamp(0., f64::from(fh)) as u32;
    let opacity = opacity.clamp(0., 1.);
    for y in y0..y1 {
        let sy = (((f64::from(y) + 0.5 - ry) / rh * f64::from(sh)) as u32).min(sh - 1);
        for x in x0..x1 {
            let sx = (((f64::from(x) + 0.5 - rx) / rw * f64::from(sw)) as u32).min(sw - 1);
            let s = &src[(sy * sw + sx) as usize * 4..][..4];
            let d = &mut dst[(y * fw + x) as usize * 4..][..4];
            let a = f32::from(s[3]) / 255. * opacity;
            if a <= 0. {
                continue;
            }
            // Straight alpha over straight alpha.
            let da = f32::from(d[3]) / 255. * (1. - a);
            let out = a + da;
            for i in 0..3 {
                d[i] = ((f32::from(s[i]) * a + f32::from(d[i]) * da) / out).round() as u8;
            }
            d[3] = (255. * out).round() as u8;
        }
    }
}

impl Timeline {
    /// Add a video; returns its ID (shared with sounds).
    pub fn add_video(&mut self, asset: VideoAsset) -> Result<AssetId, String> {
        if self.videos.len() >= MAX_VIDEOS {
            return Err(format!("A document holds at most {MAX_VIDEOS} videos."));
        }
        self.next_asset = self.next_asset.max(1);
        let id = self.next_asset;
        self.next_asset += 1;
        self.videos.insert(id, asset);
        Ok(id)
    }

    /// Place a video clip on a video track, refusing overlaps; keeps clips
    /// in order. Returns its index.
    pub fn place_video(&mut self, track: usize, clip: VideoClip) -> Result<usize, String> {
        let t = self
            .video
            .get_mut(track)
            .ok_or("No video track has that index.")?;
        if t.clips
            .iter()
            .any(|c| clip.start < c.end() && c.start < clip.end())
        {
            return Err("Clips on one track cannot overlap.".into());
        }
        let at = t.clips.partition_point(|c| c.start < clip.start);
        t.clips.insert(at, clip);
        Ok(at)
    }

    /// Bring in an imported video as a clip at `start`: on `track`, or the
    /// first video track with room (a new one when none has). With `sound`
    /// (the video's own audio, imported as a sound), the sound joins the
    /// library and a clip of it lines up under the video on the first
    /// audio track with room. Returns the video clip's track and index.
    pub fn import_video(
        &mut self,
        asset: VideoAsset,
        sound: Option<AudioAsset>,
        start: u64,
        rate: FrameRate,
        track: Option<usize>,
    ) -> Result<(usize, usize), String> {
        let id = self.add_video(asset)?;
        let clip = VideoClip::new(id, &self.videos[&id], start, rate);
        let fits = |t: &VideoTrack| {
            !t.clips
                .iter()
                .any(|c| clip.start < c.end() && c.start < clip.end())
        };
        let track = match track {
            Some(t) => t,
            None => match self.video.iter().position(fits) {
                Some(t) => t,
                None => {
                    if self.video.len() >= MAX_VIDEO_TRACKS {
                        return Err(format!(
                            "Every video track has a clip there; use at most {MAX_VIDEO_TRACKS} video tracks."
                        ));
                    }
                    self.video
                        .push(VideoTrack::new(&format!("Video {}", self.video.len() + 1)));
                    self.video.len() - 1
                }
            },
        };
        let (frames, name) = (clip.frames, clip.name.clone());
        let index = self.place_video(track, clip)?;
        if let Some(sound) = sound {
            let sound_id = self.add_asset(sound)?;
            let audio = AudioClip {
                asset: sound_id,
                name,
                start,
                frames,
                ..AudioClip::default()
            };
            let free = self.tracks.iter().position(|t| {
                !t.clips
                    .iter()
                    .any(|c| audio.start < c.end() && c.start < audio.end())
            });
            let at = match free {
                Some(t) => t,
                None if self.tracks.len() < MAX_TRACKS => {
                    self.tracks
                        .push(AudioTrack::new(&format!("Audio {}", self.tracks.len() + 1)));
                    self.tracks.len() - 1
                }
                None => return Err("Every audio track has a clip there.".into()),
            };
            self.place(at, audio)?;
        }
        Ok((track, index))
    }

    /// The reference picture timeline frame `frame` shows: the visible clip
    /// there on the first track that has one.
    pub fn video_at(&self, frame: u64, rate: FrameRate) -> Option<VideoAt> {
        self.video.iter().enumerate().find_map(|(t, track)| {
            let i = track
                .clips
                .iter()
                .position(|c| c.visible && c.start <= frame && frame < c.end())?;
            let clip = &track.clips[i];
            let asset = self.videos.get(&clip.asset)?;
            Some(VideoAt {
                track: t,
                clip: i,
                asset: clip.asset,
                index: asset.frame_index(clip.source_ms(frame, rate)),
                opacity: clip.opacity,
            })
        })
    }

    /// Frames a video clip may last from its offset to the end of its video.
    pub fn video_room(&self, clip: &VideoClip, rate: FrameRate) -> u64 {
        let ms = self
            .videos
            .get(&clip.asset)
            .map_or(0, |a| a.duration_ms.saturating_sub(clip.offset_ms));
        rate.seconds_to_frames(ms as f64 / 1000.).max(1)
    }

    /// The last frame any video clip shows.
    pub fn video_end(&self) -> u64 {
        self.video
            .iter()
            .flat_map(|t| t.clips.iter().map(VideoClip::end))
            .max()
            .unwrap_or(0)
    }

    /// Drop videos no clip uses (a video has no library of its own, so its
    /// last clip takes it out of the document). Returns how many went.
    pub fn remove_unused_videos(&mut self) -> usize {
        let used: HashSet<_> = self
            .video
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.asset))
            .collect();
        let before = self.videos.len();
        self.videos.retain(|id, _| used.contains(id));
        before - self.videos.len()
    }

    pub(super) fn validate_video(&self) -> Result<(), String> {
        if self.video.len() > MAX_VIDEO_TRACKS {
            return Err(format!("Use at most {MAX_VIDEO_TRACKS} video tracks."));
        }
        if self.videos.len() > MAX_VIDEOS {
            return Err(format!("A document holds at most {MAX_VIDEOS} videos."));
        }
        for (id, asset) in &self.videos {
            if *id == 0 || *id >= self.next_asset || self.assets.contains_key(id) {
                return Err("Video IDs must be allocated.".into());
            }
            super::audio::check_name(&asset.name, "Video")?;
            super::audio::check_format(&asset.format, "Video")?;
            if !asset.fps.is_finite() || !(0.1..=1000.).contains(&asset.fps) {
                return Err("Videos are 0.1–1000 frames per second.".into());
            }
            if !(1..=MAX_VIDEO_SIDE).contains(&asset.width)
                || !(1..=MAX_VIDEO_SIDE).contains(&asset.height)
            {
                return Err(format!("Videos are 1–{MAX_VIDEO_SIDE} pixels a side."));
            }
        }
        let mut clips = 0;
        for track in &self.video {
            super::audio::check_name(&track.name, "Track")?;
            clips += track.clips.len();
            let mut end = 0;
            for clip in &track.clips {
                super::audio::check_name(&clip.name, "Clip")?;
                let asset = self
                    .videos
                    .get(&clip.asset)
                    .ok_or("A clip refers to a missing video.")?;
                if clip.frames == 0 {
                    return Err("Clips need a length.".into());
                }
                if clip.offset_ms >= asset.duration_ms.max(1) {
                    return Err("A clip starts after its video ends.".into());
                }
                if !clip.opacity.is_finite() || !(0. ..=1.).contains(&clip.opacity) {
                    return Err("Clip opacity is 0–1.".into());
                }
                if clip.start < end {
                    return Err("Clips on one track must be in order and not overlap.".into());
                }
                end = clip.end();
            }
        }
        if clips > MAX_VIDEO_CLIPS {
            return Err(format!("Use at most {MAX_VIDEO_CLIPS} video clips."));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset() -> VideoAsset {
        VideoAsset {
            name: "Reference".into(),
            format: "mp4".into(),
            duration_ms: 4000,
            fps: 25.,
            width: 320,
            height: 240,
            has_audio: true,
            source: None,
        }
    }

    fn sound() -> AudioAsset {
        AudioAsset {
            name: "Reference".into(),
            format: "flac".into(),
            duration_ms: 4000,
            sample_rate: 48_000,
            channels: 2,
            folder: String::new(),
            source: None,
        }
    }

    #[test]
    fn imported_video_lands_on_a_free_track_with_its_sound() {
        let rate = FrameRate::whole(24);
        let mut t = Timeline::default();
        let (track, index) = t
            .import_video(asset(), Some(sound()), 10, rate, None)
            .unwrap();
        assert_eq!((track, index), (0, 0));
        assert_eq!(t.video[0].name, "Video 1");
        let clip = &t.video[0].clips[0];
        assert_eq!((clip.start, clip.frames), (10, 96));
        // Video and sound share one ID allocator.
        assert_eq!(t.videos.len(), 1);
        assert_eq!(t.assets.len(), 1);
        assert_ne!(t.videos.keys().next(), t.assets.keys().next());
        let audio = &t.tracks[0].clips[0];
        assert_eq!((audio.start, audio.frames), (10, 96));
        t.validate().unwrap();
        // A second import over the first goes on a new track.
        let (track, _) = t.import_video(asset(), None, 20, rate, None).unwrap();
        assert_eq!(track, 1);
        assert_eq!(t.video_end(), 116);
        assert!(t.end() >= 116);
        // Asked for a busy track, it refuses.
        assert!(t.import_video(asset(), None, 20, rate, Some(0)).is_err());
        let json = serde_json::to_string(&t).unwrap();
        assert!(!json.contains("visible") && !json.contains("opacity"));
        let back: Timeline = serde_json::from_str(&json).unwrap();
        assert_eq!(back.video, t.video);
    }

    #[test]
    fn frames_map_to_pictures_and_hidden_clips_show_nothing() {
        let rate = FrameRate::whole(24);
        let mut t = Timeline::default();
        t.import_video(asset(), None, 24, rate, None).unwrap();
        assert!(t.video_at(23, rate).is_none());
        let at = t.video_at(24, rate).unwrap();
        assert_eq!((at.track, at.clip, at.index), (0, 0, 0));
        // One second in at 25 fps is picture 25; trims shift it.
        assert_eq!(t.video_at(48, rate).unwrap().index, 25);
        t.video[0].clips[0].offset_ms = 1000;
        assert_eq!(t.video_at(48, rate).unwrap().index, 50);
        // Never past the last picture.
        assert_eq!(t.video_at(24 + 95, rate).unwrap().index, 99);
        t.video[0].clips[0].visible = false;
        assert!(t.video_at(48, rate).is_none());
        assert_eq!(asset().frame_index(39.999), 0);
        assert_eq!(asset().frame_index(40.), 1);
    }

    #[test]
    fn validation_and_unused_videos() {
        let rate = FrameRate::whole(24);
        let mut t = Timeline::default();
        t.import_video(asset(), None, 0, rate, None).unwrap();
        t.validate().unwrap();
        t.video[0].clips[0].opacity = 2.;
        assert!(t.validate().is_err());
        t.video[0].clips[0].opacity = 0.5;
        t.video[0].clips[0].offset_ms = 5000;
        assert!(t.validate().is_err());
        t.video[0].clips[0].offset_ms = 0;
        t.videos.values_mut().next().unwrap().fps = 0.;
        assert!(t.validate().is_err());
        t.videos.values_mut().next().unwrap().fps = 25.;
        t.video[0].clips.clear();
        assert_eq!(t.remove_unused_videos(), 1);
        assert!(t.videos.is_empty());
    }

    #[test]
    fn pictures_fit_or_inset_and_draw_with_opacity() {
        let r = placement_rect(VideoPlacement::Overlay, 160., 90., 40., 40.);
        assert_eq!(r, [35., 0., 90., 90.]);
        let p = placement_rect(VideoPlacement::PictureInPicture, 300., 300., 100., 50.);
        assert!((p[2] - 100.).abs() < 1e-9 && p[0] + p[2] < 300. && p[1] + p[3] < 300.);
        let mut dst = vec![0u8; 4 * 4 * 4];
        let src = [255u8, 0, 0, 255].repeat(4);
        draw_picture(&mut dst, (4, 4), &src, (2, 2), [0., 0., 2., 2.], 0.5);
        // Over nothing, the colour stays and only the alpha fades.
        assert_eq!(&dst[..4], &[255, 0, 0, 128]);
        let mut opaque = [0u8, 0, 255, 255].repeat(4);
        draw_picture(&mut opaque, (2, 2), &src, (2, 2), [0., 0., 2., 2.], 0.5);
        assert_eq!(&opaque[..4], &[128, 0, 128, 255]);
        assert_eq!(&dst[8..12], &[0, 0, 0, 0]);
    }
}
