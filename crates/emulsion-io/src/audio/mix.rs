//! Mixing a timeline's audio tracks down to stereo, for playback buffers and
//! the sound of exported movies, and writing the result as a WAV file.
use super::{CHANNELS, RATE, decode::decode_samples};
use anyhow::{Context, Result, bail};
use emulsion_core::timeline::{FrameRate, Timeline};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::AtomicBool;

/// The first 48 kHz sample of timeline frame `frame` at `rate`.
pub fn frame_sample(rate: FrameRate, frame: u64) -> u64 {
    let num = u128::from(rate.num.max(1));
    ((u128::from(frame) * u128::from(RATE) * u128::from(rate.den) + num / 2) / num) as u64
}

fn db(gain: f32) -> f32 {
    10f32.powf(gain / 20.)
}

/// Mix `count` stereo samples from sample `first` (see [`frame_sample`]) of
/// every audible track: track volume, clip gain and fades (interpolated per
/// sample between frames) and clip offsets. Interleaved stereo, not
/// clipped. Fails when a playing clip's sound has no file or cannot be
/// decoded.
pub fn mix_samples(
    timeline: &Timeline,
    rate: FrameRate,
    first: u64,
    count: usize,
) -> Result<Vec<f32>> {
    let mut out = vec![0f32; count * CHANNELS];
    let end = first + count as u64;
    // Timeline frames per sample.
    let frames_per_sample = rate.fps() / f64::from(RATE);
    for (index, track) in timeline.tracks.iter().enumerate() {
        if !timeline.audible(index) {
            continue;
        }
        let volume = db(track.volume_db);
        for clip in &track.clips {
            let (start, stop) = (
                frame_sample(rate, clip.start),
                frame_sample(rate, clip.end()),
            );
            let (a, b) = (start.max(first), stop.min(end));
            if a >= b {
                continue;
            }
            let asset = timeline
                .assets
                .get(&clip.asset)
                .with_context(|| format!("Clip “{}” has no sound", clip.name))?;
            let source = asset.source.as_deref().with_context(|| {
                format!("The sound “{}” has no file; import it again", asset.name)
            })?;
            let offset = clip.offset_ms * u64::from(RATE) / 1000;
            let pcm = decode_samples(source, a - start + offset, (b - a) as usize)?;
            // Gain at a frame boundary, cached per frame.
            let mut cached = (u64::MAX, 0f32, 0f32);
            for n in a..b {
                let position = n as f64 * frames_per_sample;
                let frame = (position.floor() as u64).clamp(clip.start, clip.end() - 1);
                if cached.0 != frame {
                    let g0 = clip.gain_at(frame);
                    let g1 = if frame + 1 < clip.end() {
                        clip.gain_at(frame + 1)
                    } else {
                        g0
                    };
                    cached = (frame, g0, g1);
                }
                let t = (position - frame as f64).clamp(0., 1.) as f32;
                let gain = (cached.1 + (cached.2 - cached.1) * t) * volume;
                let i = (n - a) as usize * CHANNELS;
                let o = (n - first) as usize * CHANNELS;
                out[o] += pcm[i] * gain;
                out[o + 1] += pcm[i + 1] * gain;
            }
        }
    }
    Ok(out)
}

/// Mix `frames` timeline frames from `start` (see [`mix_samples`]).
pub fn mix(timeline: &Timeline, rate: FrameRate, start: u64, frames: u64) -> Result<Vec<f32>> {
    let first = frame_sample(rate, start);
    let count = frame_sample(rate, start + frames) - first;
    mix_samples(timeline, rate, first, count as usize)
}

/// Whether any audible clip plays between frames `start` and `end`.
pub fn has_sound(timeline: &Timeline, start: u64, end: u64) -> bool {
    timeline.tracks.iter().enumerate().any(|(i, t)| {
        timeline.audible(i) && t.clips.iter().any(|c| c.start < end && start < c.end())
    })
}

/// Mix frames `start..end` into a 32-bit float stereo WAV at `path`, ten
/// seconds at a time.
pub fn write_wav(
    timeline: &Timeline,
    rate: FrameRate,
    start: u64,
    end: u64,
    path: &Path,
    cancel: &AtomicBool,
) -> Result<()> {
    let first = frame_sample(rate, start);
    let total = frame_sample(rate, end.max(start)) - first;
    let bytes = total * CHANNELS as u64 * 4;
    if bytes > u64::from(u32::MAX) - 64 {
        bail!("The sound is too long for one WAV file; export a shorter range")
    }
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    let block_align = (CHANNELS * 4) as u16;
    out.write_all(b"RIFF")?;
    out.write_all(&(36 + bytes as u32).to_le_bytes())?;
    out.write_all(b"WAVEfmt ")?;
    out.write_all(&16u32.to_le_bytes())?;
    out.write_all(&3u16.to_le_bytes())?; // IEEE float
    out.write_all(&(CHANNELS as u16).to_le_bytes())?;
    out.write_all(&RATE.to_le_bytes())?;
    out.write_all(&(RATE * u32::from(block_align)).to_le_bytes())?;
    out.write_all(&block_align.to_le_bytes())?;
    out.write_all(&32u16.to_le_bytes())?;
    out.write_all(b"data")?;
    out.write_all(&(bytes as u32).to_le_bytes())?;
    let chunk = u64::from(RATE) * 10;
    let mut at = 0;
    while at < total {
        crate::printing::canceled(cancel)?;
        let n = chunk.min(total - at);
        for s in mix_samples(timeline, rate, first + at, n as usize)? {
            out.write_all(&s.clamp(-1., 1.).to_le_bytes())?;
        }
        at += n;
    }
    out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::test_audio;
    use emulsion_core::timeline::{AudioAsset, AudioClip, AudioTrack};

    fn timeline(source: &Path) -> Timeline {
        let mut t = Timeline::default();
        let id = t
            .add_asset(AudioAsset {
                name: "Level".into(),
                format: "wav".into(),
                duration_ms: 4000,
                sample_rate: 48_000,
                channels: 2,
                folder: String::new(),
                source: Some(source.to_path_buf()),
            })
            .unwrap();
        t.tracks.push(AudioTrack::new("A"));
        t.tracks.push(AudioTrack::new("B"));
        let clip = AudioClip {
            asset: id,
            name: "Level".into(),
            start: 24,
            frames: 48,
            offset_ms: 0,
            gain_db: 0.,
            fade_in: 0,
            fade_out: 0,
        };
        t.place(0, clip.clone()).unwrap();
        t.place(1, AudioClip { start: 0, ..clip }).unwrap();
        t
    }

    /// Left-channel value at timeline frame `f` (its middle sample).
    fn at(pcm: &[f32], rate: FrameRate, f: u64) -> f32 {
        let s = (frame_sample(rate, f) + frame_sample(rate, f + 1)) / 2;
        pcm[s as usize * 2]
    }

    #[test]
    fn frames_map_to_samples() {
        assert_eq!(frame_sample(FrameRate::whole(24), 24), 48_000);
        assert_eq!(frame_sample(FrameRate::whole(25), 1), 1920);
        assert_eq!(frame_sample(FrameRate::ntsc(30), 30), 48_048);
    }

    #[test]
    fn tracks_mix_with_gain_fades_mute_and_solo() {
        if !test_audio::ffmpeg() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let src = test_audio::level(dir.path(), "level.wav", 0.25, 4.);
        let rate = FrameRate::whole(24);
        let mut t = timeline(&src);
        let pcm = mix(&t, rate, 0, 96).unwrap();
        assert_eq!(pcm.len(), 2 * 4 * 48_000);
        assert!((at(&pcm, rate, 10) - 0.25).abs() < 1e-3, "B alone");
        assert!((at(&pcm, rate, 30) - 0.5).abs() < 1e-3, "A and B");
        assert!(at(&pcm, rate, 80).abs() < 1e-6, "nothing plays");
        t.tracks[0].muted = true;
        let pcm = mix(&t, rate, 24, 24).unwrap();
        assert!((pcm[100] - 0.25).abs() < 1e-3, "muted A");
        t.tracks[0].muted = false;
        t.tracks[0].solo = true;
        let pcm = mix(&t, rate, 0, 48).unwrap();
        assert!(at(&pcm, rate, 10).abs() < 1e-6, "B is not soloed");
        assert!((at(&pcm, rate, 30) - 0.25).abs() < 1e-3);
        t.tracks[0].solo = false;
        t.tracks[1].muted = true;
        let clip = &mut t.tracks[0].clips[0];
        clip.gain_db = -6.0206;
        clip.fade_in = 8;
        clip.fade_out = 8;
        t.tracks[0].volume_db = 6.0206;
        let pcm = mix(&t, rate, 0, 96).unwrap();
        assert!(
            (at(&pcm, rate, 48) - 0.25).abs() < 1e-3,
            "gain and volume cancel"
        );
        let rising: Vec<f32> = (24..33).map(|f| at(&pcm, rate, f)).collect();
        assert!(rising.windows(2).all(|w| w[1] > w[0]), "{rising:?}");
        assert!(rising[0] < 0.02);
        // Smooth within a frame: neighbouring samples differ by little.
        let s = frame_sample(rate, 26) as usize * 2;
        assert!((pcm[s + 2] - pcm[s]).abs() < 1e-3);
        assert!(at(&pcm, rate, 71) < 0.02, "faded out at the end");
        // Offsets past the sound's end are silent.
        t.tracks[0].clips[0].offset_ms = 3900;
        let pcm = mix(&t, rate, 48, 1).unwrap();
        assert_eq!(pcm[pcm.len() - 2], 0.);
        let wav = dir.path().join("mix.wav");
        t.tracks[0].clips[0].offset_ms = 0;
        write_wav(&t, rate, 0, 48, &wav, &AtomicBool::new(false)).unwrap();
        let probe = crate::audio::probe::probe(&wav).unwrap();
        assert_eq!((probe.sample_rate, probe.channels), (48_000, 2));
        assert_eq!(probe.duration_ms, 2000);
        t.assets.values_mut().next().unwrap().source = None;
        assert!(mix(&t, rate, 0, 48).is_err());
        assert!(has_sound(&t, 0, 30) && !has_sound(&t, 80, 90));
    }
}
