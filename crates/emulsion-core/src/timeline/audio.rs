//! Audio on a timeline: a library of assets (sound files kept with the
//! document), tracks of clips placed in frames, with gain, fades, mute and
//! solo, and markers for timing other clips to.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

pub type AssetId = u64;

pub const MAX_TRACKS: usize = 16;
pub const MAX_CLIPS: usize = 4096;
pub const MAX_MARKERS: usize = 4096;
pub const MAX_ASSETS: usize = 2048;
pub const MAX_GAIN_DB: f32 = 24.;
pub const MIN_GAIN_DB: f32 = -60.;

pub(super) fn check_name(name: &str, what: &str) -> Result<(), String> {
    if name.trim().is_empty() || name.chars().count() > 200 || name.chars().any(char::is_control) {
        return Err(format!("{what} names must be 1–200 characters."));
    }
    Ok(())
}

/// A stored file's format: a short file extension.
pub(super) fn check_format(format: &str, what: &str) -> Result<(), String> {
    if format.is_empty() || format.len() > 8 || !format.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(format!("{what} formats are short file extensions."));
    }
    Ok(())
}

fn check_gain(db: f32) -> Result<(), String> {
    if !db.is_finite() || !(MIN_GAIN_DB..=MAX_GAIN_DB).contains(&db) {
        return Err(format!("Gain is {MIN_GAIN_DB}–{MAX_GAIN_DB} dB."));
    }
    Ok(())
}

/// A sound file kept with the document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AudioAsset {
    pub name: String,
    /// File extension of the stored bytes (wav, mp3, m4a, flac, ogg…).
    pub format: String,
    pub duration_ms: u64,
    pub sample_rate: u32,
    pub channels: u16,
    /// Library folder, `/`-separated; empty for the top level.
    #[serde(default)]
    pub folder: String,
    /// Where the bytes are on disk while the document is open. Filled when a
    /// file is imported or a document is opened; never saved.
    #[serde(skip)]
    pub source: Option<PathBuf>,
}

/// A stretch of an asset placed on a track.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AudioClip {
    pub asset: AssetId,
    pub name: String,
    /// First timeline frame.
    pub start: u64,
    /// Length on the timeline, in frames.
    pub frames: u64,
    /// Where in the asset the clip starts, in milliseconds.
    #[serde(default)]
    pub offset_ms: u64,
    #[serde(default)]
    pub gain_db: f32,
    #[serde(default)]
    pub fade_in: u64,
    #[serde(default)]
    pub fade_out: u64,
    /// Gain envelope keys in dB, added to `gain_db` (see `effects.rs`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub envelope: Vec<super::effects::EffectKey>,
    /// Three-band EQ; flat leaves the sound alone.
    #[serde(default, skip_serializing_if = "super::effects::Equalizer::is_flat")]
    pub eq: super::effects::Equalizer,
}

impl AudioClip {
    pub fn end(&self) -> u64 {
        self.start + self.frames
    }
    /// Linear gain at `frame` (a timeline frame inside the clip): the
    /// clip's gain and gain envelope, shaped by its fades.
    pub fn gain_at(&self, frame: u64) -> f32 {
        if frame < self.start || frame >= self.end() {
            return 0.;
        }
        let into = frame - self.start;
        let left = self.end() - frame;
        let envelope = super::effects::sample_keys(&self.envelope, into as f64).unwrap_or(0.);
        let mut k = 10f32.powf((self.gain_db + envelope) / 20.);
        if self.fade_in > 0 && into < self.fade_in {
            k *= into as f32 / self.fade_in as f32;
        }
        if self.fade_out > 0 && left <= self.fade_out {
            k *= (left - 1) as f32 / self.fade_out as f32;
        }
        k
    }
}

/// A named point on a track, for timing panels to sound.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub frame: u64,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AudioTrack {
    pub name: String,
    #[serde(default)]
    pub volume_db: f32,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub solo: bool,
    /// In time order, never overlapping.
    #[serde(default)]
    pub clips: Vec<AudioClip>,
    #[serde(default)]
    pub markers: Vec<Marker>,
}

impl AudioTrack {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            volume_db: 0.,
            muted: false,
            solo: false,
            clips: Vec::new(),
            markers: Vec::new(),
        }
    }

    /// Whether no clip overlaps frames `start..end`.
    pub fn has_room(&self, start: u64, end: u64) -> bool {
        !self.clips.iter().any(|c| start < c.end() && c.start < end)
    }
}

/// Every audio track and the assets their clips play.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Timeline {
    #[serde(default)]
    pub tracks: Vec<AudioTrack>,
    #[serde(default)]
    pub assets: BTreeMap<AssetId, AudioAsset>,
    #[serde(default)]
    pub next_asset: AssetId,
    /// Reference video tracks (see `video.rs`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub video: Vec<super::video::VideoTrack>,
    /// The videos their clips show; IDs come from `next_asset`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub videos: BTreeMap<AssetId, super::video::VideoAsset>,
}

impl Timeline {
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
            && self.assets.is_empty()
            && self.video.is_empty()
            && self.videos.is_empty()
    }

    /// Add an asset; returns its ID.
    pub fn add_asset(&mut self, asset: AudioAsset) -> Result<AssetId, String> {
        if self.assets.len() >= MAX_ASSETS {
            return Err(format!("A document holds at most {MAX_ASSETS} sounds."));
        }
        self.next_asset = self.next_asset.max(1);
        let id = self.next_asset;
        self.next_asset += 1;
        self.assets.insert(id, asset);
        Ok(id)
    }

    /// Whether a track plays: not muted, and soloed when any track is.
    pub fn audible(&self, track: usize) -> bool {
        let any_solo = self.tracks.iter().any(|t| t.solo);
        self.tracks
            .get(track)
            .is_some_and(|t| !t.muted && (!any_solo || t.solo))
    }

    /// Every marker frame on every track, sorted.
    pub fn marker_frames(&self) -> Vec<u64> {
        let mut out: Vec<u64> = self
            .tracks
            .iter()
            .flat_map(|t| t.markers.iter().map(|m| m.frame))
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Place a clip on a track, refusing overlaps; keeps clips in order.
    pub fn place(&mut self, track: usize, clip: AudioClip) -> Result<(), String> {
        let t = self
            .tracks
            .get_mut(track)
            .ok_or("No track has that index.")?;
        if !t.has_room(clip.start, clip.end()) {
            return Err("Clips on one track cannot overlap.".into());
        }
        t.clips.push(clip);
        t.clips.sort_by_key(|c| c.start);
        Ok(())
    }

    /// The last frame any clip plays or shows.
    pub fn end(&self) -> u64 {
        self.tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(AudioClip::end))
            .max()
            .unwrap_or(0)
            .max(self.video_end())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.tracks.len() > MAX_TRACKS {
            return Err(format!("Use at most {MAX_TRACKS} audio tracks."));
        }
        if self.assets.len() > MAX_ASSETS {
            return Err(format!("A document holds at most {MAX_ASSETS} sounds."));
        }
        for (id, asset) in &self.assets {
            if *id == 0 || *id >= self.next_asset {
                return Err("Sound IDs must be allocated.".into());
            }
            check_name(&asset.name, "Sound")?;
            check_format(&asset.format, "Sound")?;
            if asset.folder.chars().count() > 400
                || asset.folder.chars().any(char::is_control)
                || asset.folder.split('/').any(|p| p == "..")
            {
                return Err("Sound folders are up to 400 characters.".into());
            }
            if asset.sample_rate == 0 || asset.channels == 0 {
                return Err("Sounds need a sample rate and channels.".into());
            }
        }
        let mut clips = 0;
        for track in &self.tracks {
            check_name(&track.name, "Track")?;
            check_gain(track.volume_db)?;
            if track.markers.len() > MAX_MARKERS {
                return Err(format!("A track holds at most {MAX_MARKERS} markers."));
            }
            for marker in &track.markers {
                check_name(&marker.name, "Marker")?;
            }
            clips += track.clips.len();
            let mut end = 0;
            for clip in &track.clips {
                check_name(&clip.name, "Clip")?;
                check_gain(clip.gain_db)?;
                clip.validate_effects()?;
                let asset = self
                    .assets
                    .get(&clip.asset)
                    .ok_or("A clip refers to a missing sound.")?;
                if clip.frames == 0 || clip.fade_in + clip.fade_out > clip.frames {
                    return Err("Clips need a length, and fades must fit inside it.".into());
                }
                if clip.offset_ms >= asset.duration_ms.max(1) {
                    return Err("A clip starts after its sound ends.".into());
                }
                if clip.start < end {
                    return Err("Clips on one track must be in order and not overlap.".into());
                }
                end = clip.end();
            }
        }
        if clips > MAX_CLIPS {
            return Err(format!("Use at most {MAX_CLIPS} audio clips."));
        }
        self.validate_video()
    }

    /// Drop assets no clip uses. Returns how many were removed.
    pub fn remove_unused_assets(&mut self) -> usize {
        let used: HashSet<_> = self
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.asset))
            .collect();
        let before = self.assets.len();
        self.assets.retain(|id, _| used.contains(id));
        before - self.assets.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset() -> AudioAsset {
        AudioAsset {
            name: "Rain".into(),
            format: "wav".into(),
            duration_ms: 10_000,
            sample_rate: 48_000,
            channels: 2,
            folder: "Ambience".into(),
            source: None,
        }
    }

    fn clip(asset: AssetId, start: u64, frames: u64) -> AudioClip {
        AudioClip {
            asset,
            name: "Rain".into(),
            start,
            frames,
            offset_ms: 0,
            ..AudioClip::default()
        }
    }

    #[test]
    fn clips_place_in_order_without_overlap_and_validate() {
        let mut t = Timeline::default();
        let id = t.add_asset(asset()).unwrap();
        t.tracks.push(AudioTrack::new("Dialogue"));
        t.place(0, clip(id, 48, 24)).unwrap();
        t.place(0, clip(id, 0, 24)).unwrap();
        assert!(t.place(0, clip(id, 60, 24)).is_err());
        assert_eq!(t.tracks[0].clips[0].start, 0);
        assert_eq!(t.end(), 72);
        t.validate().unwrap();
        t.tracks[0].clips[0].fade_in = 30;
        assert!(t.validate().is_err());
        t.tracks[0].clips[0].fade_in = 0;
        t.tracks[0].clips[0].asset = 99;
        assert!(t.validate().is_err());
        let json = serde_json::to_string(&t).unwrap();
        assert!(!json.contains("source"));
    }

    #[test]
    fn mute_solo_fades_and_markers() {
        let mut t = Timeline {
            tracks: vec![AudioTrack::new("A"), AudioTrack::new("B")],
            ..Timeline::default()
        };
        assert!(t.audible(0) && t.audible(1));
        t.tracks[1].solo = true;
        assert!(!t.audible(0) && t.audible(1));
        t.tracks[1].muted = true;
        assert!(!t.audible(1));
        let mut c = clip(1, 10, 20);
        c.fade_in = 5;
        c.fade_out = 5;
        assert_eq!(c.gain_at(10), 0.);
        assert!((c.gain_at(17) - 1.).abs() < 1e-6);
        assert!(c.gain_at(26) < 1.);
        assert_eq!(c.gain_at(29), 0.);
        assert_eq!(c.gain_at(30), 0.);
        c.gain_db = -6.;
        assert!((c.gain_at(17) - 0.501).abs() < 0.01);
        t.tracks[0].markers = vec![
            Marker {
                frame: 30,
                name: "Hit".into(),
            },
            Marker {
                frame: 10,
                name: "Line".into(),
            },
        ];
        assert_eq!(t.marker_frames(), [10, 30]);
    }

    #[test]
    fn unused_assets_are_removed() {
        let mut t = Timeline::default();
        let used = t.add_asset(asset()).unwrap();
        t.add_asset(asset()).unwrap();
        t.tracks.push(AudioTrack::new("A"));
        t.place(0, clip(used, 0, 10)).unwrap();
        assert_eq!(t.remove_unused_assets(), 1);
        assert!(t.assets.contains_key(&used));
    }
}
