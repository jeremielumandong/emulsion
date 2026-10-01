//! Waveform peaks for drawing sounds on a timeline or in a clip preview.
//! A sound is decoded once, on a background thread, into a fine summary
//! (min and max of each 1/200 s, mono); any zoom level is then read from the
//! summary instantly.
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

/// Summary buckets per second.
pub const BUCKETS_PER_SECOND: u32 = 200;
/// Decode rate for the summary (mono).
const SUMMARY_RATE: u32 = 12_000;
/// Most summaries kept.
const MAX_CACHED: usize = 512;

/// The min/max summary of one sound.
#[derive(Clone, Debug, PartialEq)]
pub struct Waveform {
    /// `[min, max]` of each 1/[`BUCKETS_PER_SECOND`] s, mono, −1 to 1.
    pub summary: Vec<[f32; 2]>,
}

impl Waveform {
    pub fn duration_ms(&self) -> f64 {
        self.summary.len() as f64 * 1000. / f64::from(BUCKETS_PER_SECOND)
    }

    /// `[min, max]` for each of `buckets` equal slices of `start_ms` to
    /// `end_ms` (zero where there is no sound). The usual call draws one
    /// bucket per pixel column.
    pub fn peaks(&self, start_ms: f64, end_ms: f64, buckets: usize) -> Vec<[f32; 2]> {
        let mut out = vec![[0f32; 2]; buckets];
        if buckets == 0 || end_ms <= start_ms || !start_ms.is_finite() || !end_ms.is_finite() {
            return out;
        }
        let per_ms = f64::from(BUCKETS_PER_SECOND) / 1000.;
        let step = (end_ms - start_ms) / buckets as f64;
        for (i, peak) in out.iter_mut().enumerate() {
            let a = (start_ms + step * i as f64) * per_ms;
            let b = (start_ms + step * (i + 1) as f64) * per_ms;
            let lo = a.floor().max(0.) as usize;
            // Always cover at least one summary bucket, even zoomed far in.
            let hi = (b.ceil() as usize).max(lo + 1).min(self.summary.len());
            if lo >= hi {
                continue;
            }
            let mut p = [f32::MAX, f32::MIN];
            for s in &self.summary[lo..hi] {
                p[0] = p[0].min(s[0]);
                p[1] = p[1].max(s[1]);
            }
            *peak = p;
        }
        out
    }
}

/// Where a background summary is.
#[derive(Clone, Debug)]
pub enum State {
    /// Being decoded; ask again later (for example on the next repaint).
    Pending,
    Ready(Arc<Waveform>),
    Failed(String),
}

fn cache() -> &'static Mutex<HashMap<PathBuf, State>> {
    static CACHE: std::sync::OnceLock<Mutex<HashMap<PathBuf, State>>> = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The summary of `source`, without blocking: starts a background decode
/// the first time and returns `Pending` until it is done.
pub fn waveform(source: &Path) -> State {
    let mut map = cache().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(state) = map.get(source) {
        return state.clone();
    }
    if map.len() >= MAX_CACHED {
        map.retain(|_, s| matches!(s, State::Pending));
    }
    map.insert(source.to_path_buf(), State::Pending);
    let path = source.to_path_buf();
    std::thread::spawn(move || {
        let state = match summarize(&path) {
            Ok(w) => State::Ready(Arc::new(w)),
            Err(e) => State::Failed(e.to_string()),
        };
        cache()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(path, state);
    });
    State::Pending
}

/// The summary of `source`, waiting for it (cached like [`waveform`]).
pub fn waveform_blocking(source: &Path) -> Result<Arc<Waveform>> {
    if let Some(State::Ready(w)) = cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(source)
    {
        return Ok(w.clone());
    }
    let w = Arc::new(summarize(source)?);
    cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(source.to_path_buf(), State::Ready(w.clone()));
    Ok(w)
}

/// Decode all of `source` as a stream and summarize it, never holding the
/// PCM in memory.
fn summarize(source: &Path) -> Result<Waveform> {
    if !source.is_file() {
        bail!("The sound file {} is missing", source.display())
    }
    let mut command = crate::ffmpeg::command("ffmpeg");
    command
        .args(["-nostdin", "-v", "error", "-i"])
        .arg(source)
        .args(["-map", "0:a:0", "-vn", "-sn", "-dn", "-ac", "1", "-ar"])
        .arg(SUMMARY_RATE.to_string())
        .args(["-f", "f32le", "pipe:1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = crate::ffmpeg::spawn(&mut command)?;
    let errors = crate::ffmpeg::stderr_tail(&mut child);
    let mut stdout = child.stdout.take().context("No FFmpeg output")?;
    let per_bucket = (SUMMARY_RATE / BUCKETS_PER_SECOND) as usize;
    let mut summary = Vec::new();
    let mut current = [f32::MAX, f32::MIN];
    let mut filled = 0;
    let mut buf = vec![0u8; 64 << 10];
    let mut carry = Vec::with_capacity(4);
    loop {
        let n = stdout.read(&mut buf)?;
        if n == 0 {
            break;
        }
        carry.extend_from_slice(&buf[..n]);
        let whole = carry.len() / 4 * 4;
        for b in carry[..whole].as_chunks::<4>().0 {
            let s = f32::from_le_bytes(*b).clamp(-1., 1.);
            current = [current[0].min(s), current[1].max(s)];
            filled += 1;
            if filled == per_bucket {
                summary.push(current);
                current = [f32::MAX, f32::MIN];
                filled = 0;
            }
        }
        carry.drain(..whole);
    }
    if filled > 0 {
        summary.push(current);
    }
    let status = child.wait()?;
    let errors = errors.and_then(|h| h.join().ok()).unwrap_or_default();
    if !status.success() {
        let name = source.file_name().unwrap_or_default().to_string_lossy();
        bail!(
            "Cannot decode “{name}”: {}",
            crate::ffmpeg::last_line(&errors)
        )
    }
    Ok(Waveform { summary })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::test_audio;

    #[test]
    fn peaks_cover_any_zoom() {
        let w = Waveform {
            summary: vec![[-0.5, 0.5], [-0.1, 0.2], [0., 0.], [-1., 0.9]],
        };
        assert_eq!(w.duration_ms(), 20.);
        assert_eq!(w.peaks(0., 20., 2), [[-0.5, 0.5], [-1., 0.9]]);
        // Zoomed in further than the summary: neighbours repeat.
        assert_eq!(w.peaks(0., 5., 2), [[-0.5, 0.5], [-0.5, 0.5]]);
        assert_eq!(w.peaks(40., 60., 1), [[0., 0.]], "past the end");
        assert!(w.peaks(10., 0., 3).iter().all(|p| *p == [0., 0.]));
    }

    #[test]
    fn summaries_decode_in_the_background() {
        if !test_audio::ffmpeg() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = test_audio::tone(dir.path(), "tone.ogg", 440, 2.);
        let ready = loop {
            match waveform(&path) {
                State::Pending => std::thread::sleep(std::time::Duration::from_millis(20)),
                State::Ready(w) => break w,
                State::Failed(e) => panic!("{e}"),
            }
        };
        assert!(
            (ready.duration_ms() - 2000.).abs() < 50.,
            "{}",
            ready.duration_ms()
        );
        let peaks = ready.peaks(0., 1000., 10);
        assert!(
            peaks
                .iter()
                .all(|p| p[1] > 0.4 && p[1] < 0.6 && p[0] < -0.4),
            "{peaks:?}"
        );
        assert!(Arc::ptr_eq(&waveform_blocking(&path).unwrap(), &ready));
        let gone = dir.path().join("gone.wav");
        assert!(waveform_blocking(&gone).is_err());
    }
}
