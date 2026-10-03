//! Single video pictures through FFmpeg, frame-accurate and scaled down,
//! with the most recent ones kept in a small cache so scrubbing back and
//! forth and playback reuse them.
//!
//! Picture `n` is found by seeking (`-ss` before `-i`, which FFmpeg makes
//! exact by decoding from the keyframe before it and dropping pictures
//! that start earlier) to a quarter of a picture before `n`'s start, so the
//! first picture out is `n` itself however the file's timestamps round.
use anyhow::{Result, bail};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Most decoded bytes kept in memory.
pub const CACHE_BYTES: usize = 96 << 20;
/// Most pictures one run decodes.
pub const MAX_RUN: u32 = 48;

/// One decoded picture: straight-alpha RGBA8, `width` × `height`.
#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The size a `video` picture is decoded at to fill at most `max`: its
/// shape kept, never larger than the video, at least 2 × 2.
pub fn fit_size(video: (u32, u32), max: (u32, u32)) -> (u32, u32) {
    let (vw, vh) = (f64::from(video.0.max(1)), f64::from(video.1.max(1)));
    let k = (f64::from(max.0) / vw).min(f64::from(max.1) / vh).min(1.);
    (
        ((vw * k).round() as u32).max(2),
        ((vh * k).round() as u32).max(2),
    )
}

type Key = (PathBuf, u64, (u32, u32));

struct Cache {
    /// Most recently used last.
    pictures: VecDeque<(Key, Arc<Picture>)>,
    bytes: usize,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: Mutex<Cache> = Mutex::new(Cache {
        pictures: VecDeque::new(),
        bytes: 0,
    });
    &CACHE
}

fn cached(key: &Key) -> Option<Arc<Picture>> {
    let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
    let i = cache.pictures.iter().position(|(k, _)| k == key)?;
    let entry = cache.pictures.remove(i)?;
    let picture = entry.1.clone();
    cache.pictures.push_back(entry);
    Some(picture)
}

fn keep(key: Key, picture: Arc<Picture>) {
    let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
    if cache.pictures.iter().any(|(k, _)| *k == key) {
        return;
    }
    cache.bytes += picture.rgba.len();
    cache.pictures.push_back((key, picture));
    while cache.bytes > CACHE_BYTES {
        let Some((_, old)) = cache.pictures.pop_front() else {
            break;
        };
        cache.bytes -= old.rgba.len();
    }
}

/// Whether picture `index` of `source` at `size` is in the cache.
pub fn is_cached(source: &Path, index: u64, size: (u32, u32)) -> bool {
    let key = (source.to_path_buf(), index, size);
    cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .pictures
        .iter()
        .any(|(k, _)| *k == key)
}

/// Picture `index` of the video at `source` (playing at `fps`), scaled to
/// exactly `size` (see [`fit_size`]). On a cache miss, `ahead` more
/// pictures after it are decoded in the same run and cached, for playback.
/// Past the end of the file, the last picture there is. Blocking.
pub fn picture(
    source: &Path,
    fps: f64,
    index: u64,
    size: (u32, u32),
    ahead: u32,
) -> Result<Arc<Picture>> {
    let key = (source.to_path_buf(), index, size);
    if let Some(hit) = cached(&key) {
        return Ok(hit);
    }
    let run = decode(source, fps, index, size, 1 + ahead.min(MAX_RUN - 1))?;
    let mut first = None;
    for (n, picture) in run.into_iter().enumerate() {
        let picture = Arc::new(picture);
        first.get_or_insert_with(|| picture.clone());
        keep((source.to_path_buf(), index + n as u64, size), picture);
    }
    match first {
        Some(picture) => Ok(picture),
        // Rounding put the index just past the last picture.
        None if index > 0 => picture(source, fps, index - 1, size, 0),
        None => bail!("The video has no pictures"),
    }
}

/// Decode `count` pictures from picture `index` with one FFmpeg run.
fn decode(
    source: &Path,
    fps: f64,
    index: u64,
    (w, h): (u32, u32),
    count: u32,
) -> Result<Vec<Picture>> {
    if !source.is_file() {
        bail!("The video file {} is missing", source.display())
    }
    if !(fps.is_finite() && fps > 0.) || w == 0 || h == 0 || w > 8192 || h > 8192 {
        bail!("Cannot decode a picture of that size")
    }
    let seek = ((index as f64 - 0.25) / fps).max(0.);
    let mut command = crate::ffmpeg::command("ffmpeg");
    command
        .args(["-nostdin", "-v", "error", "-ss"])
        .arg(format!("{seek:.6}"))
        .arg("-i")
        .arg(source)
        .args(["-map", "0:v:0", "-an", "-sn", "-dn", "-frames:v"])
        .arg(count.to_string())
        .arg("-vf")
        .arg(format!("scale={w}:{h}:flags=bilinear,format=rgba"))
        .args(["-f", "rawvideo", "pipe:1"]);
    let one = w as usize * h as usize * 4;
    let run = crate::ffmpeg::capture(
        &mut command,
        (one * count as usize) as u64,
        Duration::from_secs(60),
    )?;
    let name = source.file_name().unwrap_or_default().to_string_lossy();
    match run.waited {
        crate::ffmpeg::Waited::Exited(_) if run.success() => {}
        crate::ffmpeg::Waited::Exited(_) => bail!(
            "Cannot decode “{name}”: {}",
            crate::ffmpeg::last_line(&run.stderr)
        ),
        _ => bail!("Decoding “{name}” took too long"),
    }
    Ok(run
        .stdout
        .chunks_exact(one)
        .map(|rgba| Picture {
            width: w,
            height: h,
            rgba: rgba.to_vec(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::test_audio;
    use crate::reference_video::test_video;

    #[test]
    fn sizes_fit_without_growing() {
        assert_eq!(fit_size((1920, 1080), (480, 480)), (480, 270));
        assert_eq!(fit_size((100, 50), (480, 480)), (100, 50));
        assert_eq!(fit_size((4000, 1), (100, 100)), (100, 2));
    }

    #[test]
    fn pictures_are_frame_accurate_across_keyframes_and_cached() {
        if !test_audio::ffmpeg() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let file = test_video::counter(dir.path(), 25, 20);
        let level = |p: &Picture| p.rgba[0];
        // Every picture, including ones after a keyframe (every 7th).
        for index in [0u64, 1, 6, 7, 8, 13, 19] {
            let p = picture(&file, 25., index, (16, 12), 0).unwrap();
            assert_eq!((p.width, p.height, p.rgba.len()), (16, 12, 16 * 12 * 4));
            let want = (index * 10) as i32;
            assert!(
                (i32::from(level(&p)) - want).abs() <= 3,
                "picture {index}: level {} (want {want})",
                level(&p)
            );
            assert_eq!(p.rgba[3], 255);
        }
        // A run decodes ahead into the cache.
        let p = picture(&file, 25., 2, (8, 6), 4).unwrap();
        assert!((i32::from(level(&p)) - 20).abs() <= 3);
        assert!(is_cached(&file, 6, (8, 6)));
        let ahead = picture(&file, 25., 6, (8, 6), 0).unwrap();
        assert!((i32::from(level(&ahead)) - 60).abs() <= 3);
        // Past the end: the last picture.
        let last = picture(&file, 25., 20, (8, 6), 0).unwrap();
        assert!((i32::from(level(&last)) - 190).abs() <= 3);
        assert!(picture(Path::new("/nope.mp4"), 25., 0, (8, 6), 0).is_err());
    }

    #[test]
    fn decodes_a_testsrc_clip() {
        if !test_audio::ffmpeg() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let file = test_video::clip(dir.path(), "src.mp4", (160, 120), 24, 1., false);
        let p = picture(&file, 24., 12, fit_size((160, 120), (80, 80)), 0).unwrap();
        assert_eq!((p.width, p.height), (80, 60));
        // testsrc is colourful, not black.
        assert!(
            p.rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|px| px[0] > 100 || px[1] > 100)
        );
    }
}
