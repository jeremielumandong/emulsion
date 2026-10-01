//! What a sound file holds, from `ffprobe`.
use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// The first audio stream of a file.
#[derive(Clone, Debug, PartialEq)]
pub struct Probe {
    pub duration_ms: u64,
    pub sample_rate: u32,
    pub channels: u16,
    /// The codec name, such as `pcm_s16le` or `mp3`.
    pub codec: String,
}

/// Read the first audio stream of `path`. Fails on files with no audio, no
/// known duration, or that FFmpeg cannot read.
pub fn probe(path: &Path) -> Result<Probe> {
    let mut command = crate::ffmpeg::command("ffprobe");
    command
        .args(["-v", "error", "-select_streams", "a:0", "-show_entries"])
        .args([
            "stream=codec_name,sample_rate,channels,duration:format=duration",
            "-of",
            "json",
        ])
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = crate::ffmpeg::spawn(&mut command)?;
    let errors = crate::ffmpeg::stderr_tail(&mut child);
    let mut stdout = child.stdout.take().context("No ffprobe output")?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut std::io::Read::take(&mut stdout, 1 << 20), &mut out)
            .map(|_| out)
    });
    let waited = crate::ffmpeg::wait(
        &mut child,
        &AtomicBool::new(false),
        Some(Duration::from_secs(30)),
    )?;
    let out = reader.join().ok().and_then(Result::ok).unwrap_or_default();
    let errors = errors.and_then(|h| h.join().ok()).unwrap_or_default();
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    match waited {
        crate::ffmpeg::Waited::Exited(status) if status.success() => {}
        crate::ffmpeg::Waited::Exited(_) => {
            bail!(
                "Cannot read “{name}” as a sound file: {}",
                crate::ffmpeg::last_line(&errors)
            )
        }
        _ => bail!("Reading “{name}” took too long"),
    }
    parse(&out).with_context(|| format!("“{name}” has no readable audio"))
}

fn parse(json: &[u8]) -> Result<Probe> {
    let value: serde_json::Value = serde_json::from_slice(json)?;
    let stream = value["streams"]
        .get(0)
        .context("The file has no audio stream")?;
    let number = |v: &serde_json::Value| -> Option<f64> {
        v.as_str()
            .and_then(|s| s.parse().ok())
            .or_else(|| v.as_f64())
    };
    let seconds = number(&stream["duration"])
        .or_else(|| number(&value["format"]["duration"]))
        .filter(|s| s.is_finite() && *s > 0.)
        .context("The sound has no duration")?;
    let sample_rate = number(&stream["sample_rate"]).unwrap_or(0.) as u32;
    let channels = stream["channels"].as_u64().unwrap_or(0) as u16;
    if sample_rate == 0 || channels == 0 {
        bail!("The audio stream has no sample rate or channels")
    }
    Ok(Probe {
        duration_ms: (seconds * 1000.).round().max(1.) as u64,
        sample_rate,
        channels,
        codec: stream["codec_name"].as_str().unwrap_or("").to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::test_audio;

    #[test]
    fn parses_stream_and_format_durations() {
        let json = br#"{"streams":[{"codec_name":"opus","sample_rate":"48000","channels":1}],
            "format":{"duration":"2.500000"}}"#;
        let p = parse(json).unwrap();
        assert_eq!((p.duration_ms, p.sample_rate, p.channels), (2500, 48000, 1));
        assert!(parse(br#"{"streams":[],"format":{}}"#).is_err());
    }

    #[test]
    fn probes_generated_sounds_and_rejects_non_audio() {
        if !test_audio::ffmpeg() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let wav = test_audio::tone(dir.path(), "tone.wav", 440, 1.5);
        let p = probe(&wav).unwrap();
        assert_eq!((p.sample_rate, p.channels), (44100, 2));
        assert!((1490..=1510).contains(&p.duration_ms), "{p:?}");
        let flac = test_audio::tone(dir.path(), "tone.flac", 440, 0.5);
        assert_eq!(probe(&flac).unwrap().codec, "flac");
        let text = dir.path().join("notes.wav");
        std::fs::write(&text, b"not a sound").unwrap();
        assert!(probe(&text).is_err());
    }
}
