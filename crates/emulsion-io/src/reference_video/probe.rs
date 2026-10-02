//! What a video file holds, from `ffprobe`.
use anyhow::{Context, Result, bail};
use emulsion_core::timeline::video::MAX_VIDEO_SIDE;
use std::path::Path;
use std::time::Duration;

/// The first video stream of a file, and whether it has sound.
#[derive(Clone, Debug, PartialEq)]
pub struct Probe {
    pub duration_ms: u64,
    pub fps: f64,
    pub width: u32,
    pub height: u32,
    pub has_audio: bool,
    /// The video codec name, such as `h264`.
    pub codec: String,
}

/// Read the first video stream of `path`. Fails on files with no video, no
/// known duration or frame rate, or that FFmpeg cannot read.
pub fn probe(path: &Path) -> Result<Probe> {
    let mut command = crate::ffmpeg::command("ffprobe");
    command
        .args(["-v", "error", "-show_entries"])
        .args([
            "stream=codec_type,codec_name,width,height,avg_frame_rate,r_frame_rate,duration:format=duration",
            "-of",
            "json",
        ])
        .arg(path);
    let run = crate::ffmpeg::capture(&mut command, 1 << 20, Duration::from_secs(30))?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    match run.waited {
        crate::ffmpeg::Waited::Exited(_) if run.success() => {}
        crate::ffmpeg::Waited::Exited(_) => bail!(
            "Cannot read “{name}” as a video: {}",
            crate::ffmpeg::last_line(&run.stderr)
        ),
        _ => bail!("Reading “{name}” took too long"),
    }
    parse(&run.stdout).with_context(|| format!("“{name}” has no readable video"))
}

/// A rate such as `30000/1001` or `25`.
fn rate(text: &str) -> Option<f64> {
    let (num, den) = text.split_once('/').unwrap_or((text, "1"));
    let (num, den): (f64, f64) = (num.trim().parse().ok()?, den.trim().parse().ok()?);
    let fps = num / den;
    (fps.is_finite() && fps > 0.).then_some(fps)
}

fn parse(json: &[u8]) -> Result<Probe> {
    let value: serde_json::Value = serde_json::from_slice(json)?;
    let streams = value["streams"].as_array().cloned().unwrap_or_default();
    let stream = streams
        .iter()
        .find(|s| s["codec_type"] == "video")
        .context("The file has no video stream")?;
    let number = |v: &serde_json::Value| -> Option<f64> {
        v.as_str()
            .and_then(|s| s.parse().ok())
            .or_else(|| v.as_f64())
    };
    let seconds = number(&stream["duration"])
        .or_else(|| number(&value["format"]["duration"]))
        .filter(|s| s.is_finite() && *s > 0.)
        .context("The video has no duration")?;
    let fps = stream["avg_frame_rate"]
        .as_str()
        .and_then(rate)
        .or_else(|| stream["r_frame_rate"].as_str().and_then(rate))
        .filter(|f| (0.1..=1000.).contains(f))
        .context("The video has no frame rate")?;
    let width = stream["width"].as_u64().unwrap_or(0) as u32;
    let height = stream["height"].as_u64().unwrap_or(0) as u32;
    if !(1..=MAX_VIDEO_SIDE).contains(&width) || !(1..=MAX_VIDEO_SIDE).contains(&height) {
        bail!("The video has no usable picture size")
    }
    Ok(Probe {
        duration_ms: (seconds * 1000.).round().max(1.) as u64,
        fps,
        width,
        height,
        has_audio: streams.iter().any(|s| s["codec_type"] == "audio"),
        codec: stream["codec_name"].as_str().unwrap_or("").to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::test_audio;
    use crate::reference_video::test_video;

    #[test]
    fn parses_rates_sizes_and_sound() {
        let json = br#"{"streams":[{"codec_type":"audio","codec_name":"aac"},
            {"codec_type":"video","codec_name":"h264","width":1920,"height":1080,
             "avg_frame_rate":"30000/1001","r_frame_rate":"30000/1001"}],
            "format":{"duration":"2.500000"}}"#;
        let p = parse(json).unwrap();
        assert_eq!((p.duration_ms, p.width, p.height), (2500, 1920, 1080));
        assert!((p.fps - 29.97).abs() < 0.001 && p.has_audio);
        assert!(parse(br#"{"streams":[{"codec_type":"audio"}],"format":{}}"#).is_err());
        // An unknown average falls back to the stream's base rate.
        let json = br#"{"streams":[{"codec_type":"video","width":8,"height":8,
            "avg_frame_rate":"0/0","r_frame_rate":"24/1","duration":"1"}]}"#;
        assert_eq!(parse(json).unwrap().fps, 24.);
    }

    #[test]
    fn probes_a_generated_clip_and_rejects_others() {
        if !test_audio::ffmpeg() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let file = test_video::clip(dir.path(), "clip.mov", (96, 54), 24, 1.5, false);
        let p = probe(&file).unwrap();
        assert_eq!((p.width, p.height, p.fps), (96, 54, 24.));
        assert!((1450..=1550).contains(&p.duration_ms), "{p:?}");
        assert!(!p.has_audio);
        let sound = test_audio::tone(dir.path(), "tone.wav", 440, 0.5);
        assert!(probe(&sound).is_err());
    }
}
