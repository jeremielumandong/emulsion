//! Movie files through FFmpeg: raw RGBA frames are piped in one at a time
//! (so long movies never sit in memory), optionally muxed with a WAV
//! soundtrack, and encoded as H.264 MP4 or ProRes MOV, with progress and
//! cancel. The movie goes to a staging file beside the target that replaces
//! it only once complete. Neutral: any workspace renders its own frames.
use anyhow::{Context, Result, bail};
use emulsion_core::timeline::FrameRate;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Codec {
    /// H.264 in MP4: small files that play everywhere.
    #[default]
    H264,
    /// Apple ProRes 422 in QuickTime MOV: for editing.
    ProRes,
}

impl Codec {
    pub fn extension(self) -> &'static str {
        match self {
            Self::H264 => "mp4",
            Self::ProRes => "mov",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::H264 => "H.264 (MP4)",
            Self::ProRes => "ProRes 422 (MOV)",
        }
    }
}

/// Largest side of an encoded movie.
pub const MAX_SIDE: u32 = 8192;

/// What to encode.
#[derive(Clone, Debug, PartialEq)]
pub struct Encode {
    pub codec: Codec,
    /// Frame size; even numbers.
    pub width: u32,
    pub height: u32,
    pub rate: FrameRate,
    /// 1–100; higher is better and larger.
    pub quality: u8,
    /// A WAV file to use as the soundtrack.
    pub audio: Option<PathBuf>,
}

impl Encode {
    pub fn validate(&self) -> Result<()> {
        if self.width < 2
            || self.height < 2
            || self.width > MAX_SIDE
            || self.height > MAX_SIDE
            || !self.width.is_multiple_of(2)
            || !self.height.is_multiple_of(2)
        {
            bail!("Movie frames are 2–{MAX_SIDE} pixels a side, in even numbers")
        }
        self.rate.validate().map_err(anyhow::Error::msg)?;
        if !(1..=100).contains(&self.quality) {
            bail!("Quality is 1–100")
        }
        Ok(())
    }

    fn codec_args(&self) -> Vec<String> {
        let q = u32::from(self.quality);
        let mut args: Vec<String> = match self.codec {
            Codec::H264 => {
                // Quality 100 → CRF 14 (near lossless), 1 → CRF 51.
                let crf = 51 - (q * 37) / 100;
                [
                    "-c:v",
                    "libx264",
                    "-preset",
                    "medium",
                    "-crf",
                    &crf.to_string(),
                    "-pix_fmt",
                    "yuv420p",
                    "-movflags",
                    "+faststart",
                ]
                .map(String::from)
                .to_vec()
            }
            Codec::ProRes => {
                // Proxy, LT, 422 and HQ by quality.
                let profile = (q.saturating_sub(1) / 25).min(3);
                [
                    "-c:v",
                    "prores_ks",
                    "-profile:v",
                    &profile.to_string(),
                    "-pix_fmt",
                    "yuv422p10le",
                    "-vendor",
                    "apl0",
                ]
                .map(String::from)
                .to_vec()
            }
        };
        if self.audio.is_some() {
            let audio: &[&str] = match self.codec {
                Codec::H264 => &["-c:a", "aac", "-b:a", "192k"],
                Codec::ProRes => &["-c:a", "pcm_s16le"],
            };
            args.extend(audio.iter().map(|a| a.to_string()));
        }
        args.extend(["-f".to_string(), self.codec.extension().to_string()]);
        args
    }
}

/// Encode `frames` frames into `path`. `frame(n)` renders frame `n` as
/// straight RGBA8 of `width × height`; `progress(done, total)` is called
/// after each. Setting `cancel` stops the export and leaves any existing
/// file untouched.
pub fn encode(
    path: &Path,
    settings: &Encode,
    frames: u64,
    mut frame: impl FnMut(u64) -> Result<Vec<u8>>,
    progress: &mut dyn FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<()> {
    settings.validate()?;
    if frames == 0 {
        bail!("There are no frames to export")
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .context("Choose a file name for the movie")?;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = NEXT.fetch_add(1, Ordering::Relaxed);
    let staging = path.with_file_name(format!(".{name}.emulsion-tmp-{}-{seq}", std::process::id()));
    let result = run(&staging, settings, frames, &mut frame, progress, cancel)
        .and_then(|()| std::fs::rename(&staging, path).context("Cannot replace the movie file"));
    if result.is_err() {
        let _ = std::fs::remove_file(&staging);
    }
    result
}

fn run(
    staging: &Path,
    settings: &Encode,
    frames: u64,
    frame: &mut dyn FnMut(u64) -> Result<Vec<u8>>,
    progress: &mut dyn FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<()> {
    let (w, h) = (settings.width, settings.height);
    let mut command = crate::ffmpeg::command("ffmpeg");
    command
        .args([
            "-nostdin", "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba",
        ])
        .arg("-s")
        .arg(format!("{w}x{h}"))
        .arg("-framerate")
        .arg(format!("{}/{}", settings.rate.num, settings.rate.den))
        .args(["-i", "pipe:0"]);
    if let Some(audio) = &settings.audio {
        command.arg("-i").arg(audio);
    }
    command.args(["-map", "0:v:0"]);
    if settings.audio.is_some() {
        command.args(["-map", "1:a:0"]);
    }
    command
        .args(settings.codec_args())
        .arg(staging)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = crate::ffmpeg::spawn(&mut command)?;
    let errors = crate::ffmpeg::stderr_tail(&mut child);
    let mut stdin = child.stdin.take().context("Cannot write to FFmpeg")?;
    let expected = w as usize * h as usize * 4;
    let mut failure = None;
    for n in 0..frames {
        if cancel.load(Ordering::Relaxed) {
            failure = Some(anyhow::anyhow!("Export canceled"));
            break;
        }
        let pixels = match frame(n) {
            Ok(p) if p.len() == expected => p,
            Ok(_) => {
                failure = Some(anyhow::anyhow!("Frame {n} has the wrong size"));
                break;
            }
            Err(e) => {
                failure = Some(e);
                break;
            }
        };
        if stdin.write_all(&pixels).is_err() {
            // FFmpeg stopped; its error output says why.
            break;
        }
        progress(n + 1, frames);
    }
    drop(stdin);
    if failure.is_some() {
        let _ = child.kill();
    }
    let status = child.wait()?;
    let errors = errors.and_then(|h| h.join().ok()).unwrap_or_default();
    if let Some(e) = failure {
        return Err(e);
    }
    if !status.success() {
        bail!(
            "FFmpeg could not encode the movie: {}",
            crate::ffmpeg::last_line(&errors)
        )
    }
    Ok(())
}

/// Facts about a movie file from `ffprobe`: frame count of the first video
/// stream (counted), its size, duration in seconds and whether it has sound.
#[derive(Clone, Debug, PartialEq)]
pub struct MovieInfo {
    pub frames: u64,
    pub width: u32,
    pub height: u32,
    pub seconds: f64,
    pub audio: bool,
    pub codec: String,
}

/// Probe a movie (counts every frame, so it reads the whole file).
pub fn probe(path: &Path) -> Result<MovieInfo> {
    let out = crate::ffmpeg::spawn(
        crate::ffmpeg::command("ffprobe")
            .args(["-v", "error", "-count_frames", "-show_entries"])
            .args([
                "stream=codec_type,codec_name,width,height,nb_read_frames:format=duration",
                "-of",
                "json",
            ])
            .arg(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::null()),
    )?
    .wait_with_output()?;
    if !out.status.success() {
        bail!("Cannot read {} as a movie", path.display())
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let streams = v["streams"].as_array().cloned().unwrap_or_default();
    let video = streams
        .iter()
        .find(|s| s["codec_type"] == "video")
        .context("The movie has no picture")?;
    let num = |v: &serde_json::Value| {
        v.as_u64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            .unwrap_or(0)
    };
    Ok(MovieInfo {
        frames: num(&video["nb_read_frames"]),
        width: num(&video["width"]) as u32,
        height: num(&video["height"]) as u32,
        seconds: v["format"]["duration"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.),
        audio: streams.iter().any(|s| s["codec_type"] == "audio"),
        codec: video["codec_name"].as_str().unwrap_or("").into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(codec: Codec) -> Encode {
        Encode {
            codec,
            width: 64,
            height: 36,
            rate: FrameRate::whole(24),
            quality: 80,
            audio: None,
        }
    }

    #[test]
    fn settings_are_checked() {
        let mut s = settings(Codec::H264);
        s.validate().unwrap();
        s.width = 63;
        assert!(s.validate().is_err());
        s.width = 64;
        s.quality = 0;
        assert!(s.validate().is_err());
        assert!(
            settings(Codec::H264)
                .codec_args()
                .contains(&"libx264".into())
        );
        let mut pro = settings(Codec::ProRes);
        pro.audio = Some("a.wav".into());
        let args = pro.codec_args();
        assert!(args.contains(&"pcm_s16le".into()) && !args.contains(&"-b:a".into()));
        assert_eq!(args[args.len() - 1], "mov");
    }

    #[test]
    fn movies_encode_with_progress_and_cancel() {
        if !crate::audio::test_audio::ffmpeg() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.mp4");
        let red = [255u8, 0, 0, 255].repeat(64 * 36);
        let mut done = 0;
        encode(
            &path,
            &settings(Codec::H264),
            12,
            |_| Ok(red.clone()),
            &mut |d, _| done = d,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(done, 12);
        let info = probe(&path).unwrap();
        assert_eq!((info.frames, info.width, info.height), (12, 64, 36));
        assert_eq!(info.codec, "h264");
        assert!(!info.audio);
        let before = std::fs::read(&path).unwrap();
        let cancel = AtomicBool::new(false);
        let failed = encode(
            &path,
            &settings(Codec::ProRes),
            12,
            |n| {
                if n == 3 {
                    cancel.store(true, Ordering::Relaxed);
                }
                Ok(red.clone())
            },
            &mut |_, _| {},
            &cancel,
        );
        assert!(failed.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before, "untouched");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        let wrong = encode(
            &path,
            &settings(Codec::H264),
            2,
            |_| Ok(vec![0; 4]),
            &mut |_, _| {},
            &AtomicBool::new(false),
        );
        assert!(wrong.is_err());
    }
}
