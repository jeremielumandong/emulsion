//! Enhance dialogue (AI9): a recorded or generated line cleaned up through
//! FFmpeg — rumble cut below 80 Hz, broadband noise reduced, sibilance
//! tamed, levels evened by a compressor and loudness set to −16 LUFS — into
//! a new 48 kHz WAV sound. The original sound is left alone; the board keeps
//! both. Runs locally like every FFmpeg job.
use super::AudioAsset;
use crate::ffmpeg::{FFMPEG, Waited, command};
use anyhow::{Result, bail};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// Integrated loudness enhanced dialogue is set to.
pub const TARGET_LUFS: f32 = -16.;

/// The filter chain, in order: high-pass, FFT denoise, de-esser,
/// compressor and loudness normalisation.
pub fn filters() -> String {
    format!(
        "highpass=f=80,afftdn=nf=-25,deesser=i=0.4,acompressor=threshold=-20dB:ratio=3:attack=5:release=120:makeup=2,loudnorm=I={TARGET_LUFS}:TP=-1.5:LRA=11"
    )
}

/// Enhance the sound at `source` into a new sound (named "Enhanced
/// dialogue"; the board names and files it). Blocking; `cancel` stops it.
pub fn enhance(source: &Path, cancel: &AtomicBool) -> Result<AudioAsset> {
    let dir = tempfile::tempdir()?;
    let out = dir.path().join("Enhanced dialogue.wav");
    let mut cmd = command("ffmpeg");
    cmd.args(["-nostdin", "-v", "error", "-y", "-i"])
        .arg(source)
        .arg("-af")
        .arg(filters())
        .args(["-ar", "48000", "-c:a", "pcm_s16le"])
        .arg(&out);
    let done = crate::ffmpeg::run(
        &mut cmd,
        FFMPEG,
        None,
        0,
        cancel,
        Some(Duration::from_secs(1800)),
    )?;
    match done.waited {
        Waited::Exited(status) if status.success() => {}
        Waited::Exited(_) => bail!(
            "FFmpeg could not enhance this sound. {}",
            crate::ffmpeg::last_line(&done.stderr)
        ),
        Waited::Canceled => bail!("Canceled."),
        Waited::TimedOut => bail!("Enhancing took too long."),
    }
    super::store::import(&out, "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::test_audio;

    #[test]
    fn enhancing_makes_a_new_sound_and_keeps_the_original() {
        if !test_audio::ffmpeg() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let line = test_audio::tone(dir.path(), "line.wav", 440, 1.5);
        let before = std::fs::read(&line).unwrap();
        let original = crate::audio::store::import(&line, "Recordings").unwrap();
        let source = original.source.clone().unwrap();
        let enhanced = enhance(&source, &AtomicBool::new(false)).unwrap();
        assert_eq!(enhanced.format, "wav");
        assert_eq!(enhanced.sample_rate, 48_000);
        assert!(
            (1300..=1700).contains(&enhanced.duration_ms),
            "{enhanced:?}"
        );
        assert_ne!(enhanced.source, original.source);
        assert_eq!(std::fs::read(&line).unwrap(), before);
        assert!(source.exists());
        // Cancel stops it.
        let err = enhance(&source, &AtomicBool::new(true)).unwrap_err();
        assert_eq!(err.to_string(), "Canceled.");
    }
}
