//! Decoding sound files to interleaved stereo `f32` at 48 kHz through
//! FFmpeg, in 10-second blocks kept in a bounded cache so players and
//! exports can read any range repeatedly without decoding it again.
use super::{CHANNELS, RATE};
use anyhow::{Result, bail};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Sample frames (one per channel pair) in a cached block.
pub const BLOCK: u64 = RATE as u64 * 10;
/// Most decoded bytes kept in memory.
pub const CACHE_BYTES: usize = 256 << 20;

type Block = Arc<Vec<f32>>;

struct Cache {
    /// Most recently used last.
    blocks: VecDeque<((PathBuf, u64), Block)>,
    bytes: usize,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: Mutex<Cache> = Mutex::new(Cache {
        blocks: VecDeque::new(),
        bytes: 0,
    });
    &CACHE
}

/// Block `index` of `source`: up to [`BLOCK`] stereo samples starting at
/// sample `index * BLOCK`; shorter (or empty) at the end of the file.
fn block(source: &Path, index: u64) -> Result<Block> {
    let key = (source.to_path_buf(), index);
    {
        let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(i) = cache.blocks.iter().position(|(k, _)| *k == key) {
            let entry = cache.blocks.remove(i).unwrap();
            let block = entry.1.clone();
            cache.blocks.push_back(entry);
            return Ok(block);
        }
    }
    let block = Arc::new(run(source, index * BLOCK, BLOCK)?);
    let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
    cache.bytes += block.len() * 4;
    cache.blocks.push_back((key, block.clone()));
    while cache.bytes > CACHE_BYTES {
        let Some((_, old)) = cache.blocks.pop_front() else {
            break;
        };
        cache.bytes -= old.len() * 4;
    }
    Ok(block)
}

/// Decode `count` stereo samples from sample `first` with one FFmpeg run.
fn run(source: &Path, first: u64, count: u64) -> Result<Vec<f32>> {
    if !source.is_file() {
        bail!("The sound file {} is missing", source.display())
    }
    let mut command = crate::ffmpeg::command("ffmpeg");
    command
        .args(["-nostdin", "-v", "error", "-ss"])
        .arg(format!("{:.6}", first as f64 / f64::from(RATE)))
        .arg("-i")
        .arg(source)
        .arg("-t")
        .arg(format!("{:.6}", count as f64 / f64::from(RATE)))
        .args(["-map", "0:a:0", "-vn", "-sn", "-dn", "-ac", "2", "-ar"])
        .arg(RATE.to_string())
        .args(["-f", "f32le", "pipe:1"]);
    let limit = count * CHANNELS as u64 * 4;
    let run = crate::ffmpeg::capture(&mut command, limit, Duration::from_secs(120))?;
    let name = source.file_name().unwrap_or_default().to_string_lossy();
    match run.waited {
        crate::ffmpeg::Waited::Exited(_) if run.success() => {}
        crate::ffmpeg::Waited::Exited(_) => bail!(
            "Cannot decode “{name}”: {}",
            crate::ffmpeg::last_line(&run.stderr)
        ),
        _ => bail!("Decoding “{name}” took too long"),
    }
    let bytes = run.stdout;
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect())
}

/// `count` interleaved stereo samples (`count * 2` values) of `source` from
/// sample `first` at 48 kHz, silent past the end of the sound.
pub fn decode_samples(source: &Path, first: u64, count: usize) -> Result<Vec<f32>> {
    let mut out = vec![0f32; count * CHANNELS];
    let end = first + count as u64;
    let mut at = first;
    while at < end {
        let index = at / BLOCK;
        let block = block(source, index)?;
        let block_start = index * BLOCK;
        let block_end = (block_start + BLOCK).min(end);
        let available = (block.len() / CHANNELS) as u64;
        let from = at - block_start;
        let to = (block_end - block_start).min(available);
        if from < to {
            let dst = ((at - first) as usize) * CHANNELS;
            let src = &block[from as usize * CHANNELS..to as usize * CHANNELS];
            out[dst..dst + src.len()].copy_from_slice(src);
        }
        if available < BLOCK {
            // The sound ends in this block.
            break;
        }
        at = block_end;
    }
    Ok(out)
}

/// `duration_ms` of `source` from `start_ms`, interleaved stereo at 48 kHz.
pub fn decode(source: &Path, start_ms: u64, duration_ms: u64) -> Result<Vec<f32>> {
    let rate = u64::from(RATE);
    let first = start_ms * rate / 1000;
    let count = (duration_ms * rate / 1000) as usize;
    decode_samples(source, first, count)
}

/// Drop every cached block of `source` (after its file changes or goes).
pub fn forget(source: &Path) {
    let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
    let mut freed = 0;
    cache.blocks.retain(|((path, _), block)| {
        let keep = path != source;
        if !keep {
            freed += block.len() * 4;
        }
        keep
    });
    cache.bytes -= freed;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::test_audio;

    #[test]
    fn decodes_ranges_across_blocks_and_pads_with_silence() {
        if !test_audio::ffmpeg() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = test_audio::level(dir.path(), "level.wav", 0.25, 12.);
        // Spans the first block boundary at 10 s.
        let pcm = decode_samples(&path, BLOCK - 100, 200).unwrap();
        assert_eq!(pcm.len(), 400);
        assert!(
            pcm.iter().all(|s| (s - 0.25).abs() < 1e-4),
            "{:?}",
            &pcm[..4]
        );
        let tail = decode(&path, 11_900, 200).unwrap();
        assert_eq!(tail.len(), 2 * 9600);
        assert!((tail[0] - 0.25).abs() < 1e-4);
        assert_eq!(*tail.last().unwrap(), 0., "silence past the end");
        assert!(decode(&dir.path().join("gone.wav"), 0, 10).is_err());
        forget(&path);
    }
}
