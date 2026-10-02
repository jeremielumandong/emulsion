//! Reference video for timelines, through FFmpeg: probing and importing
//! video files (optionally with their sound as a library sound), keeping
//! their bytes in the media cache while a document is open (and in `.emu`
//! packages as `video/{id}.{format}` when saved), and decoding single
//! pictures, scaled down, for scrubbing, playback and export. Neutral: works
//! on [`emulsion_core::timeline`] types only.
//!
//! * **Import** — [`import`]`(path, with_audio)` probes a file, copies it
//!   into the media cache and returns a [`VideoAsset`] with `source` set,
//!   and with `with_audio` the file's sound as an [`AudioAsset`]; add both
//!   with `Timeline::import_video`.
//! * **Pictures** — [`decode::picture`]`(source, asset, index, size, ahead)`
//!   decodes picture `index` (frame-accurate) scaled to fit `size`, keeping
//!   recent pictures in a small cache; `ahead` decodes the following
//!   pictures in the same run, for playback. Blocking: call it from a
//!   background thread.
//!
//! A missing FFmpeg is an error with [`crate::ffmpeg::MISSING`], never a
//! panic.
use crate::audio::store;
use anyhow::{Context, Result, bail};
pub use emulsion_core::timeline::{AudioAsset, VideoAsset};
use std::path::Path;
use std::time::Duration;

pub mod decode;
pub mod probe;

/// Video file extensions import accepts, lower case.
pub const EXTENSIONS: &[&str] = &["mp4", "mov", "m4v", "mkv", "webm", "avi"];

/// Most video bytes one package holds (beside, not inside, its sound budget).
pub const MAX_PACKAGE_VIDEO: u64 = 2 << 30;

/// The package entry holding video `id`.
pub fn entry_name(id: u64, format: &str) -> String {
    format!("video/{id}.{format}")
}

/// The format (lower-case extension) of a video file, when import takes it.
pub fn format_of(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    EXTENSIONS.contains(&ext.as_str()).then_some(ext)
}

/// A video file and, when asked for and it has one, its sound.
#[derive(Clone, Debug)]
pub struct Imported {
    pub video: VideoAsset,
    pub sound: Option<AudioAsset>,
}

/// Import a video file: probe it, copy its bytes into the media cache and
/// return the asset (named after the file) with `source` set. With
/// `with_audio`, the file's first sound stream is also extracted (as FLAC)
/// and imported as a library sound of the same name; a file without sound
/// gives none. The file is not changed.
pub fn import(path: &Path, with_audio: bool) -> Result<Imported> {
    let format = format_of(path).with_context(|| {
        format!(
            "Choose a video file ({})",
            EXTENSIONS
                .iter()
                .map(|e| format!(".{e}"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    let file =
        std::fs::File::open(path).with_context(|| format!("Cannot open {}", path.display()))?;
    if !file.metadata()?.is_file() {
        bail!("Choose a video file, not a folder")
    }
    let probe = probe::probe(path)?;
    let (source, _) = store::copy_to_cache(file, &format, MAX_PACKAGE_VIDEO)?;
    let name = store::name_of(path, "Video");
    let sound = if with_audio && probe.has_audio {
        let mut sound = extract_audio(&source, path)?;
        sound.name = name.clone();
        Some(sound)
    } else {
        None
    };
    Ok(Imported {
        video: VideoAsset {
            name,
            format,
            duration_ms: probe.duration_ms,
            fps: probe.fps,
            width: probe.width,
            height: probe.height,
            has_audio: probe.has_audio,
            source: Some(source),
        },
        sound,
    })
}

/// The first sound stream of video `source` as an imported FLAC sound.
fn extract_audio(source: &Path, original: &Path) -> Result<AudioAsset> {
    let temp = tempfile::tempdir()?;
    let flac = temp.path().join("sound.flac");
    let mut command = crate::ffmpeg::command("ffmpeg");
    command
        .args(["-nostdin", "-v", "error", "-y", "-i"])
        .arg(source)
        .args(["-map", "0:a:0", "-vn", "-sn", "-dn", "-c:a", "flac"])
        .arg(&flac);
    let run = crate::ffmpeg::capture(&mut command, 0, Duration::from_secs(600))?;
    let name = original.file_name().unwrap_or_default().to_string_lossy();
    if !run.success() {
        bail!(
            "Cannot read the sound of “{name}”: {}",
            crate::ffmpeg::last_line(&run.stderr)
        )
    }
    store::import(&flac, "")
}

#[cfg(test)]
pub(crate) mod test_video {
    use std::path::{Path, PathBuf};

    /// A `seconds`-long `w` × `h` test clip at `fps` as `name` in `dir`
    /// (lavfi `testsrc`, keyframes every 10 pictures), with a tone when
    /// `sound`. MP4 and MOV use MPEG-4 Part 2, which every FFmpeg has.
    pub fn clip(
        dir: &Path,
        name: &str,
        (w, h): (u32, u32),
        fps: u32,
        seconds: f64,
        sound: bool,
    ) -> PathBuf {
        let path = dir.join(name);
        let mut command = crate::ffmpeg::command("ffmpeg");
        command
            .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg(format!(
                "testsrc=size={w}x{h}:rate={fps}:duration={seconds}"
            ));
        if sound {
            command.args(["-f", "lavfi", "-i"]).arg(format!(
                "sine=frequency=440:sample_rate=48000:duration={seconds}"
            ));
        }
        command.args(["-g", "10", "-c:v", "mpeg4", "-q:v", "3"]);
        if sound {
            command.args(["-c:a", "aac", "-shortest"]);
        }
        let status = command.arg(&path).status().unwrap();
        assert!(status.success());
        path
    }

    /// A clip whose picture `n` is a flat grey of level `n * 10` (lossless
    /// RGB FFV1 in Matroska), to check which picture a decode returns.
    pub fn counter(dir: &Path, fps: u32, pictures: u32) -> PathBuf {
        let path = dir.join("counter.mkv");
        let status = crate::ffmpeg::command("ffmpeg")
            .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg(format!(
                "nullsrc=size=32x24:rate={fps},format=gbrp,geq=r='N*10':g='N*10':b='N*10'"
            ))
            .args(["-frames:v", &pictures.to_string(), "-g", "7"])
            .args(["-c:v", "ffv1", "-pix_fmt", "gbrp"])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::test_audio;

    #[test]
    fn import_copies_the_video_and_optionally_its_sound() {
        assert!(import(Path::new("/nope/a.txt"), false).is_err());
        assert!(import(Path::new("/nope/a.mp4"), false).is_err());
        assert_eq!(entry_name(3, "mov"), "video/3.mov");
        if !test_audio::ffmpeg() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let file = test_video::clip(dir.path(), "Ref take.mp4", (160, 120), 25, 2., true);
        let got = import(&file, true).unwrap();
        let video = &got.video;
        assert_eq!(video.name, "Ref take");
        assert_eq!(video.format, "mp4");
        assert_eq!((video.width, video.height), (160, 120));
        assert!((video.fps - 25.).abs() < 1e-6);
        assert!((1900..=2100).contains(&video.duration_ms), "{video:?}");
        assert!(video.has_audio);
        let source = video.source.clone().unwrap();
        assert!(source.starts_with(store::cache_root().unwrap()));
        assert_eq!(
            std::fs::read(source).unwrap(),
            std::fs::read(&file).unwrap()
        );
        let sound = got.sound.unwrap();
        assert_eq!(
            (sound.name.as_str(), sound.format.as_str()),
            ("Ref take", "flac")
        );
        assert!(sound.source.unwrap().is_file());
        // Without sound in the file, none comes in.
        let silent = test_video::clip(dir.path(), "silent.mov", (64, 48), 24, 1., false);
        let got = import(&silent, true).unwrap();
        assert!(!got.video.has_audio && got.sound.is_none());
    }
}
