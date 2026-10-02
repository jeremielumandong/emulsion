//! A neutral edit: what editing software exchanges through EDL, FCP 7 XML
//! and OpenTimelineIO. Picture and sound clips placed on tracks by record
//! (sequence) frames, each cut from a source by its source frames, with how
//! a picture clip enters from the one before it and named markers. Every
//! time is in frames at the edit's own rate; record frames count from the
//! sequence start, so `start` only says where the sequence's timecode
//! begins (often 01:00:00:00).
use super::{FrameRate, TransitionKind};
use serde::{Deserialize, Serialize};

/// How a clip enters from the clip before it on its track. It starts at
/// the cut and runs over the incoming clip's first frames (so the outgoing
/// clip needs that many frames of media after its out point).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditTransition {
    pub kind: TransitionKind,
    pub frames: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EditClip {
    /// The clip name; exported clips carry their panel's or sound's name.
    pub name: String,
    /// Where its media is: an absolute path, or empty when the edit does
    /// not say.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub media: String,
    /// Track index from 0 (V1, A1…).
    pub track: usize,
    pub source_in: u64,
    pub source_out: u64,
    pub record_in: u64,
    pub record_out: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<EditTransition>,
    /// Sound clips: gain in dB when the edit says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gain_db: Option<f32>,
}

impl EditClip {
    pub fn frames(&self) -> u64 {
        self.record_out.saturating_sub(self.record_in)
    }
    /// The media file's name without its extension, if the edit names a
    /// file.
    pub fn media_stem(&self) -> Option<&str> {
        let name = self.media.rsplit(['/', '\\']).next()?;
        let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
        (!stem.is_empty()).then_some(stem)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EditMarker {
    /// Record frame.
    pub frame: u64,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edit {
    pub name: String,
    pub rate: FrameRate,
    /// The sequence's first timecode, as a frame number at `rate`.
    pub start: u64,
    /// Picture clips, in record order within each track.
    pub video: Vec<EditClip>,
    /// Sound clips, in record order within each track.
    pub audio: Vec<EditClip>,
    pub markers: Vec<EditMarker>,
}

impl Edit {
    pub fn new(name: &str, rate: FrameRate) -> Self {
        Self {
            name: name.into(),
            rate,
            start: 0,
            video: Vec::new(),
            audio: Vec::new(),
            markers: Vec::new(),
        }
    }

    /// One hour, the usual first timecode of an edited sequence.
    pub fn one_hour(rate: FrameRate) -> u64 {
        rate.parse_timecode("01:00:00:00").unwrap_or(0)
    }

    /// The last record frame of any clip.
    pub fn end(&self) -> u64 {
        self.video
            .iter()
            .chain(&self.audio)
            .map(|c| c.record_out)
            .max()
            .unwrap_or(0)
    }

    /// Sort both lists by track, then record frame.
    pub fn sort(&mut self) {
        for list in [&mut self.video, &mut self.audio] {
            list.sort_by_key(|c| (c.track, c.record_in));
        }
        self.markers.sort_by_key(|m| m.frame);
    }

    /// Reject clips with no length or running backwards, and absurd sizes.
    pub fn validate(&self) -> Result<(), String> {
        self.rate.validate()?;
        if self.video.len() + self.audio.len() > 20_000 || self.markers.len() > 20_000 {
            return Err("The edit has too many clips or markers.".into());
        }
        for clip in self.video.iter().chain(&self.audio) {
            if clip.record_out <= clip.record_in {
                return Err(format!("The clip “{}” has no length.", clip.name));
            }
            if clip.source_out < clip.source_in || clip.track > 64 {
                return Err(format!("The clip “{}” is not valid.", clip.name));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems_ignore_folders_and_extensions() {
        let mut clip = EditClip {
            media: "/a/b/Panel_1_p3.png".into(),
            ..EditClip::default()
        };
        assert_eq!(clip.media_stem(), Some("Panel_1_p3"));
        clip.media = r"C:\x\Rain.wav".into();
        assert_eq!(clip.media_stem(), Some("Rain"));
        clip.media.clear();
        assert_eq!(clip.media_stem(), None);
        assert_eq!(Edit::one_hour(FrameRate::whole(24)), 86_400);
        assert_eq!(Edit::one_hour(FrameRate::ntsc(30)), 107_892);
    }
}
