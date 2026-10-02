//! Sound for timelines, through FFmpeg: probing and importing sound files,
//! keeping their bytes in a media cache while a document is open (and in
//! `.emu` packages when saved), decoding to PCM, waveform peaks and mixing a
//! timeline down to stereo. Neutral: works on [`emulsion_core::timeline`]
//! types only.
//!
//! All PCM here is interleaved stereo `f32` at [`RATE`] (48 kHz).
//!
//! # Using it
//!
//! * **Import** — [`store::import`]`(path, folder)` probes a file, copies it
//!   into the media cache and returns an [`AudioAsset`] with `source` set;
//!   add it with `Timeline::add_asset`. [`store::EXTENSIONS`] lists what the
//!   file picker should offer. Packages store each asset as
//!   `audio/{id}.{format}` (see `project.rs`); opening a package fills
//!   `source` again.
//! * **Decode** — [`decode::decode`]`(source, start_ms, duration_ms)` or
//!   [`decode::decode_samples`]`(source, first_sample, count)`. Decoded audio
//!   is cached in 10-second blocks (bounded to 256 MiB), so repeated or
//!   sequential reads are cheap. Blocking: call from a background thread.
//! * **Waveforms** — [`waveform::waveform`]`(source)` never blocks: it
//!   returns [`waveform::State::Pending`] while a background thread decodes,
//!   then `Ready(Arc<Waveform>)`; poll it when repainting. Then
//!   [`waveform::Waveform::peaks`]`(start_ms, end_ms, buckets)` gives
//!   `[min, max]` per bucket at any zoom, instantly.
//!   [`waveform::waveform_blocking`] waits instead.
//! * **Mixdown** — [`mix::mix`]`(timeline, rate, start_frame, frames)` or
//!   [`mix::mix_samples`]`(timeline, rate, first_sample, count)` mix every
//!   audible track (mute/solo, track volume, clip gain, gain envelope and
//!   fades interpolated per sample, clip EQ through [`effects`], clip
//!   offsets). [`mix::frame_sample`] maps a timeline frame to its first
//!   sample. Blocking, like decode. The player and movie export both mix
//!   through here.
//! * **WAV** — [`wav::WavWriter`] writes float or 16-bit WAV a block at a
//!   time (the mixdown, and microphone recordings).
//!
//! A missing FFmpeg is an error with [`crate::ffmpeg::MISSING`], never a
//! panic.
pub use emulsion_core::timeline::AudioAsset;

pub mod decode;
pub mod effects;
pub mod mix;
pub mod probe;
pub mod store;
pub mod wav;
pub mod waveform;

/// Samples per second of all PCM here.
pub const RATE: u32 = 48_000;
/// Interleaved channels of all PCM here (stereo).
pub const CHANNELS: usize = 2;

#[cfg(test)]
pub(crate) mod test_audio {
    use std::path::{Path, PathBuf};

    /// Whether FFmpeg runs here; tests that need it return early without.
    pub fn ffmpeg() -> bool {
        let ok = crate::ffmpeg::available();
        if !ok {
            eprintln!("FFmpeg unavailable; skipping audio integration test");
        }
        ok
    }

    /// A `seconds`-long sine at `hz` (amplitude 0.5) as `name` in `dir`.
    pub fn tone(dir: &Path, name: &str, hz: u32, seconds: f64) -> PathBuf {
        let path = dir.join(name);
        let status = crate::ffmpeg::command("ffmpeg")
            .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg(format!(
                "sine=frequency={hz}:sample_rate=44100:duration={seconds}"
            ))
            .args(["-af", "volume=4", "-ac", "2"])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        path
    }

    /// A `seconds`-long constant (DC) signal of `level`, as 48 kHz WAV.
    pub fn level(dir: &Path, name: &str, level: f32, seconds: f64) -> PathBuf {
        let path = dir.join(name);
        let status = crate::ffmpeg::command("ffmpeg")
            .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg(format!(
                "aevalsrc={level}|{level}:sample_rate=48000:duration={seconds}"
            ))
            .args(["-c:a", "pcm_f32le"])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        path
    }
}
