//! A stretch of a timeline: cut out what plays between two frames (audio and
//! video clips trimmed to it, markers within it), and replace a stretch with
//! another timeline's, rippling everything after it by the change in length.
//! Storyboard extract and merge use these to carry sound and reference
//! video with a range of scenes.
use super::FrameRate;
use super::audio::{AssetId, AudioClip, AudioTrack, MAX_TRACKS, Timeline};
use super::video::{MAX_VIDEO_TRACKS, VideoClip, VideoTrack};
use std::collections::HashMap;

/// Milliseconds `frames` frames last, rounded.
fn ms(frames: u64, rate: FrameRate) -> u64 {
    (rate.frames_to_seconds(frames) * 1000.).round() as u64
}

impl AudioClip {
    /// The part of the clip between timeline frames `from` and `to`, at the
    /// same frames: the head trimmed (its offset, fades and effect keys
    /// follow) and the tail cut. `None` when nothing of it is there.
    pub fn trimmed(&self, from: u64, to: u64, rate: FrameRate) -> Option<Self> {
        let (start, end) = (self.start.max(from), self.end().min(to));
        if start >= end {
            return None;
        }
        let head = start - self.start;
        let mut clip = self.clone();
        if head > 0 {
            clip.offset_ms += ms(head, rate);
            clip.fade_in = clip.fade_in.saturating_sub(head);
            clip.shift_effect_keys(head as i64);
        }
        let tail = self.end() - end;
        clip.fade_out = clip.fade_out.saturating_sub(tail);
        clip.start = start;
        clip.frames = end - start;
        clip.fade_in = clip.fade_in.min(clip.frames);
        clip.fade_out = clip.fade_out.min(clip.frames - clip.fade_in);
        Some(clip)
    }
}

impl VideoClip {
    /// The part of the clip between timeline frames `from` and `to`; see
    /// [`AudioClip::trimmed`].
    pub fn trimmed(&self, from: u64, to: u64, rate: FrameRate) -> Option<Self> {
        let (start, end) = (self.start.max(from), self.end().min(to));
        if start >= end {
            return None;
        }
        let mut clip = self.clone();
        clip.offset_ms += ms(start - self.start, rate);
        clip.start = start;
        clip.frames = end - start;
        Some(clip)
    }
}

/// Clips kept around a replaced stretch `from..from + old`: those before it
/// cut at `from`, those after it cut at its end and moved by `delta`.
fn around<C: Clone>(
    clips: &[C],
    from: u64,
    old: u64,
    delta: i64,
    trim: impl Fn(&C, u64, u64) -> Option<C>,
    start: impl Fn(&mut C) -> &mut u64,
) -> Vec<C> {
    let mut out = Vec::new();
    for clip in clips {
        out.extend(trim(clip, 0, from));
        if let Some(mut tail) = trim(clip, from + old, u64::MAX) {
            let s = start(&mut tail);
            *s = (*s as i64 + delta).max(0) as u64;
            out.push(tail);
        }
    }
    out
}

impl Timeline {
    /// What plays from `from` up to `to`, starting at frame 0: every track
    /// (empty ones too, so tracks keep their names and settings), clips
    /// trimmed to the stretch, markers inside it, and only the sounds and
    /// videos those clips use. Clips whose trimmed start lies past the end
    /// of their file are left out.
    pub fn excerpt(&self, from: u64, to: u64, rate: FrameRate) -> Timeline {
        let mut out = Timeline {
            tracks: Vec::new(),
            assets: self.assets.clone(),
            next_asset: self.next_asset,
            video: Vec::new(),
            videos: self.videos.clone(),
        };
        for track in &self.tracks {
            let mut t = AudioTrack {
                clips: Vec::new(),
                markers: Vec::new(),
                ..track.clone()
            };
            for clip in &track.clips {
                if let Some(mut c) = clip.trimmed(from, to, rate)
                    && self
                        .assets
                        .get(&c.asset)
                        .is_some_and(|a| c.offset_ms < a.duration_ms.max(1))
                {
                    c.start -= from;
                    t.clips.push(c);
                }
            }
            for marker in track
                .markers
                .iter()
                .filter(|m| (from..to).contains(&m.frame))
            {
                let mut m = marker.clone();
                m.frame -= from;
                t.markers.push(m);
            }
            out.tracks.push(t);
        }
        for track in &self.video {
            let mut t = VideoTrack::new(&track.name);
            for clip in &track.clips {
                if let Some(mut c) = clip.trimmed(from, to, rate)
                    && self
                        .videos
                        .get(&c.asset)
                        .is_some_and(|a| c.offset_ms < a.duration_ms.max(1))
                {
                    c.start -= from;
                    t.clips.push(c);
                }
            }
            out.video.push(t);
        }
        out.remove_unused_assets();
        out.remove_unused_videos();
        out
    }

    /// Replace the stretch `from..from + old` with the first `new` frames of
    /// `with` (a timeline starting at frame 0, such as an `excerpt`), and
    /// move everything after the stretch by `new - old` frames. Clips that
    /// cross either end of the stretch are cut there. Tracks match by name
    /// (new names add tracks); sounds and videos `with` uses are added unless
    /// this timeline already has the same file under the same ID.
    pub fn splice(
        &mut self,
        from: u64,
        old: u64,
        with: &Timeline,
        new: u64,
        rate: FrameRate,
    ) -> Result<(), String> {
        let delta = new as i64 - old as i64;
        let moved = |frame: u64| (frame as i64 + delta).max(0) as u64;
        for track in &mut self.tracks {
            track.clips = around(
                &track.clips,
                from,
                old,
                delta,
                |c, a, b| c.trimmed(a, b, rate),
                |c| &mut c.start,
            );
            track
                .markers
                .retain(|m| m.frame < from || m.frame >= from + old);
            for marker in &mut track.markers {
                if marker.frame >= from + old {
                    marker.frame = moved(marker.frame);
                }
            }
        }
        for track in &mut self.video {
            track.clips = around(
                &track.clips,
                from,
                old,
                delta,
                |c, a, b| c.trimmed(a, b, rate),
                |c| &mut c.start,
            );
        }
        let mut sounds: HashMap<AssetId, AssetId> = HashMap::new();
        let mut used = vec![false; self.tracks.len()];
        for track in &with.tracks {
            let index = match (0..self.tracks.len())
                .find(|i| !used[*i] && self.tracks[*i].name == track.name)
            {
                Some(i) => i,
                None if track.clips.is_empty() && track.markers.is_empty() => continue,
                None => {
                    if self.tracks.len() >= MAX_TRACKS {
                        return Err(format!("Use at most {MAX_TRACKS} audio tracks."));
                    }
                    self.tracks.push(AudioTrack {
                        clips: Vec::new(),
                        markers: Vec::new(),
                        ..track.clone()
                    });
                    used.push(false);
                    self.tracks.len() - 1
                }
            };
            used[index] = true;
            for clip in &track.clips {
                let Some(mut c) = clip.trimmed(0, new, rate) else {
                    continue;
                };
                let asset = with
                    .assets
                    .get(&c.asset)
                    .ok_or("A clip refers to a missing sound.")?;
                c.asset = match sounds.get(&c.asset) {
                    Some(id) => *id,
                    None => {
                        let id = match self.assets.get(&c.asset) {
                            Some(mine) if same_sound(mine, asset) => c.asset,
                            _ => self.add_asset(asset.clone())?,
                        };
                        sounds.insert(c.asset, id);
                        id
                    }
                };
                c.start += from;
                self.place(index, c)?;
            }
            for marker in track.markers.iter().filter(|m| m.frame < new) {
                let mut m = marker.clone();
                m.frame += from;
                let markers = &mut self.tracks[index].markers;
                let at = markers.partition_point(|x| x.frame <= m.frame);
                markers.insert(at, m);
            }
        }
        let mut videos: HashMap<AssetId, AssetId> = HashMap::new();
        let mut used = vec![false; self.video.len()];
        for track in &with.video {
            let index = match (0..self.video.len())
                .find(|i| !used[*i] && self.video[*i].name == track.name)
            {
                Some(i) => i,
                None if track.clips.is_empty() => continue,
                None => {
                    if self.video.len() >= MAX_VIDEO_TRACKS {
                        return Err(format!("Use at most {MAX_VIDEO_TRACKS} video tracks."));
                    }
                    self.video.push(VideoTrack::new(&track.name));
                    used.push(false);
                    self.video.len() - 1
                }
            };
            used[index] = true;
            for clip in &track.clips {
                let Some(mut c) = clip.trimmed(0, new, rate) else {
                    continue;
                };
                let asset = with
                    .videos
                    .get(&c.asset)
                    .ok_or("A clip refers to a missing video.")?;
                c.asset = match videos.get(&c.asset) {
                    Some(id) => *id,
                    None => {
                        let id = match self.videos.get(&c.asset) {
                            Some(mine)
                                if mine.name == asset.name
                                    && mine.format == asset.format
                                    && mine.duration_ms == asset.duration_ms =>
                            {
                                c.asset
                            }
                            _ => self.add_video(asset.clone())?,
                        };
                        videos.insert(c.asset, id);
                        id
                    }
                };
                c.start += from;
                self.place_video(index, c)?;
            }
        }
        self.remove_unused_videos();
        Ok(())
    }
}

/// The same sound file: what the library knows about it agrees.
pub(crate) fn same_sound(a: &super::AudioAsset, b: &super::AudioAsset) -> bool {
    a.name == b.name
        && a.format == b.format
        && a.duration_ms == b.duration_ms
        && a.sample_rate == b.sample_rate
        && a.channels == b.channels
}

#[cfg(test)]
mod tests {
    use super::super::{AudioAsset, Marker, VideoAsset};
    use super::*;

    fn sound(t: &mut Timeline) -> AssetId {
        t.add_asset(AudioAsset {
            name: "Rain".into(),
            format: "wav".into(),
            duration_ms: 100_000,
            sample_rate: 48_000,
            channels: 2,
            folder: String::new(),
            source: None,
        })
        .unwrap()
    }

    fn clip(asset: AssetId, start: u64, frames: u64) -> AudioClip {
        AudioClip {
            asset,
            name: "Rain".into(),
            start,
            frames,
            ..AudioClip::default()
        }
    }

    #[test]
    fn excerpts_trim_clips_and_keep_what_they_use() {
        let rate = FrameRate::whole(24);
        let mut t = Timeline::default();
        let rain = sound(&mut t);
        sound(&mut t);
        t.tracks.push(AudioTrack::new("FX"));
        let mut long = clip(rain, 10, 100);
        long.fade_in = 20;
        long.fade_out = 10;
        t.place(0, long).unwrap();
        t.tracks[0].markers = vec![
            Marker {
                frame: 5,
                name: "Before".into(),
            },
            Marker {
                frame: 40,
                name: "In".into(),
            },
        ];
        let part = t.excerpt(24, 48, rate);
        part.validate().unwrap();
        let c = &part.tracks[0].clips[0];
        assert_eq!((c.start, c.frames, c.offset_ms), (0, 24, 583));
        assert_eq!((c.fade_in, c.fade_out), (6, 0));
        assert_eq!(part.tracks[0].markers.len(), 1);
        assert_eq!(part.tracks[0].markers[0].frame, 16);
        assert_eq!(part.assets.len(), 1, "unused sounds stay behind");
        assert!(t.excerpt(200, 300, rate).tracks[0].clips.is_empty());
    }

    #[test]
    fn splicing_replaces_a_stretch_and_ripples_the_rest() {
        let rate = FrameRate::whole(24);
        let mut t = Timeline::default();
        let rain = sound(&mut t);
        t.tracks.push(AudioTrack::new("FX"));
        // One clip across the stretch 20..60, one after it.
        t.place(0, clip(rain, 10, 60)).unwrap();
        t.place(0, clip(rain, 80, 10)).unwrap();
        let mut theirs = t.excerpt(20, 60, rate);
        theirs.tracks[0].clips.clear();
        theirs.tracks[0].clips.push(clip(rain, 0, 30));
        theirs.tracks.push(AudioTrack::new("Voice"));
        let voice = theirs
            .add_asset(AudioAsset {
                name: "Line".into(),
                ..theirs.assets[&rain].clone()
            })
            .unwrap();
        theirs.place(1, clip(voice, 5, 100)).unwrap();
        let video = theirs
            .add_video(VideoAsset {
                name: "Ref".into(),
                format: "mp4".into(),
                duration_ms: 10_000,
                fps: 24.,
                width: 8,
                height: 8,
                has_audio: false,
                source: None,
            })
            .unwrap();
        theirs.video.push(VideoTrack::new("Video 1"));
        let ref_clip = VideoClip::new(video, &theirs.videos[&video], 0, rate);
        theirs.place_video(0, ref_clip).unwrap();
        // The stretch grows from 40 to 50 frames.
        t.splice(20, 40, &theirs, 50, rate).unwrap();
        t.validate().unwrap();
        let fx: Vec<_> = t.tracks[0]
            .clips
            .iter()
            .map(|c| (c.start, c.frames))
            .collect();
        assert_eq!(fx, [(10, 10), (20, 30), (70, 10), (90, 10)]);
        assert_eq!(t.tracks[0].clips[0].asset, rain, "the same sound is reused");
        let line = &t.tracks[1].clips[0];
        assert_eq!(
            (line.start, line.frames),
            (25, 45),
            "cut at the stretch's end"
        );
        assert_ne!(line.asset, rain);
        assert_eq!(t.video[0].clips[0].frames, 50);
        assert_eq!(t.assets.len(), 2);
    }
}
