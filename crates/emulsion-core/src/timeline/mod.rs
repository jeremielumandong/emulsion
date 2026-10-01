//! Time on a track: frame rates and SMPTE timecode, a sequence of clip
//! durations with ripple, roll, retime and snapping edits, audio tracks with
//! clips and markers, and transitions between clips. Neutral: no workspace
//! types, so any workspace can lay out its own clips on it.
use serde::{Deserialize, Serialize};

pub mod audio;
pub mod transition;

pub use audio::{AudioAsset, AudioClip, AudioTrack, Marker, Timeline};
pub use transition::{Edge, Transition, TransitionKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameRate {
    pub num: u32,
    pub den: u32,
}

impl FrameRate {
    pub const PRESETS: [Self; 9] = [
        Self::ntsc(24),
        Self::whole(24),
        Self::whole(25),
        Self::ntsc(30),
        Self::whole(30),
        Self::whole(48),
        Self::whole(50),
        Self::ntsc(60),
        Self::whole(60),
    ];
    pub const fn whole(fps: u32) -> Self {
        Self { num: fps, den: 1 }
    }
    /// The NTSC rate just below `fps`, such as 23.976 for 24.
    pub const fn ntsc(fps: u32) -> Self {
        Self {
            num: fps * 1000,
            den: 1001,
        }
    }
    pub fn fps(self) -> f64 {
        f64::from(self.num) / f64::from(self.den)
    }
    pub fn validate(self) -> Result<(), String> {
        if !matches!(self.den, 1 | 1001) || self.num == 0 || !(1. ..=120.).contains(&self.fps()) {
            return Err("Frame rate must be 1–120 fps, whole or NTSC (×1000/1001).".into());
        }
        Ok(())
    }
    pub fn frames_to_ms(self, frames: u32) -> f64 {
        f64::from(frames) * 1000. / self.fps()
    }
    pub fn label(self) -> String {
        if self.den == 1 {
            format!("{} fps", self.num)
        } else {
            format!("{:.3} fps", self.fps())
        }
    }
}

impl FrameRate {
    /// Frames per timecode second: the nominal whole rate (30 for 29.97).
    pub fn timebase(self) -> u64 {
        u64::from(self.num).div_ceil(u64::from(self.den))
    }
    /// Whether timecode drops frame numbers to stay on the clock
    /// (29.97 and 59.94).
    pub fn drop_frame(self) -> bool {
        self.den == 1001 && self.timebase().is_multiple_of(30)
    }
    pub fn frames_to_seconds(self, frames: u64) -> f64 {
        frames as f64 * f64::from(self.den) / f64::from(self.num)
    }
    /// The nearest frame to `seconds`.
    pub fn seconds_to_frames(self, seconds: f64) -> u64 {
        (seconds.max(0.) * f64::from(self.num) / f64::from(self.den)).round() as u64
    }

    /// SMPTE timecode for frame `frame`: `HH:MM:SS:FF`, or `HH:MM:SS;FF` with
    /// drop-frame numbering at 29.97 and 59.94.
    pub fn timecode(self, frame: u64) -> String {
        let base = self.timebase();
        let mut n = frame;
        let separator = if self.drop_frame() {
            // Drop 2 (or 4) frame numbers every minute except each tenth.
            let drop = base / 15;
            let per_ten = base * 600 - drop * 9;
            let per_minute = base * 60 - drop;
            let tens = n / per_ten;
            let rest = n % per_ten;
            n += drop * 9 * tens;
            if rest > drop {
                n += drop * ((rest - drop) / per_minute);
            }
            ';'
        } else {
            ':'
        };
        let ff = n % base;
        let seconds = n / base;
        format!(
            "{:02}:{:02}:{:02}{separator}{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60,
            ff
        )
    }

    /// The frame a timecode names; accepts `:` or `;` before the frames.
    pub fn parse_timecode(self, text: &str) -> Option<u64> {
        let parts: Vec<u64> = text
            .trim()
            .split([':', ';'])
            .map(|p| p.parse().ok())
            .collect::<Option<_>>()?;
        let [h, m, s, f] = parts[..] else {
            return None;
        };
        let base = self.timebase();
        if m >= 60 || s >= 60 || f >= base {
            return None;
        }
        let minutes = h * 60 + m;
        let mut frame = (minutes * 60 + s) * base + f;
        if self.drop_frame() {
            let drop = base / 15;
            frame = frame.checked_sub(drop * (minutes - minutes / 10))?;
        }
        Some(frame)
    }
}

/// Start frame of each duration, then the total.
pub fn starts(durations: &[u32]) -> Vec<u64> {
    let mut out = Vec::with_capacity(durations.len() + 1);
    let mut at = 0u64;
    for d in durations {
        out.push(at);
        at += u64::from(*d);
    }
    out.push(at);
    out
}

/// Which duration holds `frame`, and how far into it.
pub fn locate(durations: &[u32], frame: u64) -> Option<(usize, u64)> {
    let mut at = 0u64;
    for (i, d) in durations.iter().enumerate() {
        let end = at + u64::from(*d);
        if frame < end {
            return Some((i, frame - at));
        }
        at = end;
    }
    None
}

/// Move the boundary after `index` by `delta` frames, keeping the total:
/// one side grows as the other shrinks, neither below one frame. Returns the
/// delta applied.
pub fn roll(durations: &mut [u32], index: usize, delta: i64) -> i64 {
    if index + 1 >= durations.len() {
        return 0;
    }
    let (a, b) = (i64::from(durations[index]), i64::from(durations[index + 1]));
    let delta = delta.clamp(1 - a, b - 1);
    durations[index] = (a + delta) as u32;
    durations[index + 1] = (b - delta) as u32;
    delta
}

/// Scale durations so they add up to `total` frames, keeping their
/// proportions (largest remainders get the spare frames) and at least
/// `min` frames each. Fails when `total` cannot fit.
pub fn retime(durations: &[u32], total: u64, min: u32) -> Result<Vec<u32>, String> {
    let n = durations.len() as u64;
    if n == 0 {
        return Ok(Vec::new());
    }
    let min = u64::from(min.max(1));
    if total < n * min {
        return Err(format!("{n} clips need at least {} frames.", n * min));
    }
    let current: u64 = durations.iter().map(|d| u64::from(*d)).sum();
    let exact: Vec<f64> = durations
        .iter()
        .map(|d| {
            let share = if current == 0 {
                1. / n as f64
            } else {
                f64::from(*d) / current as f64
            };
            share * total as f64
        })
        .collect();
    let mut out: Vec<u64> = exact.iter().map(|e| e.floor() as u64).collect();
    let mut left = total - out.iter().sum::<u64>();
    let mut order: Vec<usize> = (0..out.len()).collect();
    order.sort_by(|a, b| {
        (exact[*b] - exact[*b].floor())
            .total_cmp(&(exact[*a] - exact[*a].floor()))
            .then(a.cmp(b))
    });
    for &i in &order {
        if left == 0 {
            break;
        }
        out[i] += 1;
        left -= 1;
    }
    // Lift short clips to the minimum, taking frames from the longest.
    while let Some(short) = out.iter().position(|d| *d < min) {
        let long = (0..out.len()).max_by_key(|i| out[*i]).unwrap();
        out[long] -= 1;
        out[short] += 1;
    }
    out.into_iter()
        .map(|d| u32::try_from(d).map_err(|_| "Duration is too long.".to_string()))
        .collect()
}

/// Move each boundary to the nearest marker within `tolerance` frames, as
/// long as every duration keeps at least one frame. The total changes only
/// if the last boundary (the end) snaps.
pub fn snap_to_markers(durations: &[u32], markers: &[u64], tolerance: u64) -> Vec<u32> {
    let mut bounds = starts(durations);
    let n = bounds.len();
    for i in 1..n {
        let b = bounds[i];
        let nearest = markers
            .iter()
            .copied()
            .filter(|m| m.abs_diff(b) <= tolerance)
            .min_by_key(|m| m.abs_diff(b));
        if let Some(m) = nearest {
            let after = if i + 1 < n { bounds[i + 1] } else { u64::MAX };
            if m > bounds[i - 1] && m < after {
                bounds[i] = m;
            }
        }
    }
    bounds.windows(2).map(|w| (w[1] - w[0]) as u32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timecode_counts_whole_and_drop_frame_rates() {
        let r24 = FrameRate::whole(24);
        assert_eq!(r24.timecode(0), "00:00:00:00");
        assert_eq!(r24.timecode(24 * 3661 + 5), "01:01:01:05");
        assert_eq!(r24.parse_timecode("01:01:01:05"), Some(24 * 3661 + 5));
        assert_eq!(r24.parse_timecode("00:00:00:24"), None);
        let df = FrameRate::ntsc(30);
        assert!(df.drop_frame() && !FrameRate::ntsc(24).drop_frame());
        // After 00:00:59;29 comes 00:01:00;02.
        assert_eq!(df.timecode(1799), "00:00:59;29");
        assert_eq!(df.timecode(1800), "00:01:00;02");
        // Every tenth minute keeps its first frames.
        assert_eq!(df.timecode(17982), "00:10:00;00");
        for frame in [0, 1799, 1800, 17982, 107_892, 123_456] {
            assert_eq!(df.parse_timecode(&df.timecode(frame)), Some(frame));
        }
        assert_eq!(FrameRate::ntsc(24).timecode(24), "00:00:01:00");
        assert_eq!(r24.seconds_to_frames(1.5), 36);
        assert!((r24.frames_to_seconds(36) - 1.5).abs() < 1e-9);
    }

    #[test]
    fn durations_locate_roll_and_retime() {
        let d = [10, 20, 30];
        assert_eq!(starts(&d), [0, 10, 30, 60]);
        assert_eq!(locate(&d, 0), Some((0, 0)));
        assert_eq!(locate(&d, 29), Some((1, 19)));
        assert_eq!(locate(&d, 60), None);
        let mut r = d;
        assert_eq!(roll(&mut r, 0, 5), 5);
        assert_eq!(r, [15, 15, 30]);
        assert_eq!(roll(&mut r, 1, 100), 29);
        assert_eq!(r, [15, 44, 1]);
        assert_eq!(roll(&mut r, 2, 3), 0);
        let fitted = retime(&d, 120, 1).unwrap();
        assert_eq!(fitted, [20, 40, 60]);
        let odd = retime(&[1, 1, 1], 10, 1).unwrap();
        assert_eq!(odd.iter().sum::<u32>(), 10);
        assert!(odd.iter().all(|d| (3..=4).contains(d)));
        assert!(retime(&d, 2, 1).is_err());
    }

    #[test]
    fn boundaries_snap_to_nearby_markers() {
        let d = [24, 24, 24];
        let snapped = snap_to_markers(&d, &[20, 50, 100], 5);
        assert_eq!(snapped, [20, 30, 22]);
        assert_eq!(snap_to_markers(&d, &[], 5), d);
    }
}
