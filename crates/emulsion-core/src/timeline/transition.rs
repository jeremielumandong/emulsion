//! Transitions between two clips, and a CPU renderer that blends two RGBA8
//! frames at a point in a transition. Players and every export draw
//! transitions with this, so they always look the same.
use serde::{Deserialize, Serialize};

/// The side an incoming picture enters from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum TransitionKind {
    #[default]
    Cut,
    Dissolve,
    /// A straight edge sweeps across, revealing the incoming picture from
    /// `from`.
    Wipe {
        from: Edge,
    },
    /// A hand sweeps clockwise from twelve o'clock.
    Clock,
    /// A circle opens from the centre.
    Iris,
    /// The incoming picture slides in over the outgoing one from `from`.
    Slide {
        from: Edge,
    },
    /// Fade out to a colour, then in from it.
    FadeToColor {
        color: [u8; 3],
    },
}

/// How one clip enters from the one before it. Its frames are the first
/// frames of the incoming clip, so transitions never change timing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transition {
    #[serde(flatten)]
    pub kind: TransitionKind,
    #[serde(default)]
    pub frames: u32,
}

impl Transition {
    pub fn is_cut(&self) -> bool {
        self.kind == TransitionKind::Cut || self.frames == 0
    }
    /// Progress 0–1 at `frame` frames into the incoming clip, or `None`
    /// once the transition is over.
    pub fn progress(&self, frame: u64) -> Option<f32> {
        if self.is_cut() || frame >= u64::from(self.frames) {
            return None;
        }
        Some((frame as f32 + 0.5) / self.frames as f32)
    }
    pub fn label(&self) -> &'static str {
        match self.kind {
            TransitionKind::Cut => "Cut",
            TransitionKind::Dissolve => "Dissolve",
            TransitionKind::Wipe { .. } => "Wipe",
            TransitionKind::Clock => "Clock wipe",
            TransitionKind::Iris => "Iris",
            TransitionKind::Slide { .. } => "Slide",
            TransitionKind::FadeToColor { .. } => "Fade to colour",
        }
    }
}

/// How much of the incoming picture shows at pixel `(x, y)` of a `w` × `h`
/// frame, for the shaped wipes. Edges are one pixel soft.
fn reveal(kind: TransitionKind, t: f32, x: f32, y: f32, w: f32, h: f32) -> f32 {
    let soft = |d: f32| (d + 0.5).clamp(0., 1.);
    match kind {
        TransitionKind::Wipe { from } => {
            let (pos, len) = match from {
                Edge::Left => (x, w),
                Edge::Right => (w - x, w),
                Edge::Top => (y, h),
                Edge::Bottom => (h - y, h),
            };
            soft(t * len - pos)
        }
        TransitionKind::Clock => {
            let (dx, dy) = (x - w / 2., y - h / 2.);
            // Angle clockwise from twelve o'clock, 0–1.
            let a = (dx.atan2(-dy) / std::f32::consts::TAU).rem_euclid(1.);
            let r = dx.hypot(dy).max(1.);
            soft((t - a) * std::f32::consts::TAU * r)
        }
        TransitionKind::Iris => {
            let (dx, dy) = (x - w / 2., y - h / 2.);
            let max = (w / 2.).hypot(h / 2.);
            soft(t * max - dx.hypot(dy))
        }
        _ => 0.,
    }
}

/// Blend `from` into `to` (both RGBA8, `w` × `h`) at progress `t` (0–1).
pub fn blend(kind: TransitionKind, t: f32, from: &[u8], to: &[u8], w: u32, h: u32) -> Vec<u8> {
    let t = t.clamp(0., 1.);
    let n = (w as usize) * (h as usize) * 4;
    assert!(
        from.len() >= n && to.len() >= n,
        "frames must be w × h RGBA8"
    );
    let mix =
        |a: u8, b: u8, k: f32| (f32::from(a) + (f32::from(b) - f32::from(a)) * k).round() as u8;
    // Exact ends, whatever the shape's soft edge does.
    if t >= 1. {
        return to[..n].to_vec();
    }
    if t <= 0. && kind != TransitionKind::Cut {
        return from[..n].to_vec();
    }
    let mut out = vec![0u8; n];
    match kind {
        TransitionKind::Cut => out.copy_from_slice(&to[..n]),
        TransitionKind::Dissolve => {
            for i in 0..n {
                out[i] = mix(from[i], to[i], t);
            }
        }
        TransitionKind::FadeToColor { color } => {
            let (src, k) = if t < 0.5 {
                (from, t * 2.)
            } else {
                (to, (1. - t) * 2.)
            };
            for (i, px) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                for c in 0..3 {
                    px[c] = mix(src[i * 4 + c], color[c], k);
                }
                px[3] = mix(src[i * 4 + 3], 255, k);
            }
        }
        TransitionKind::Slide { from: edge } => {
            let (wi, hi) = (w as i64, h as i64);
            let shift = (1. - t) as f64;
            let (ox, oy) = match edge {
                Edge::Left => (-(shift * w as f64).round() as i64, 0),
                Edge::Right => ((shift * w as f64).round() as i64, 0),
                Edge::Top => (0, -(shift * h as f64).round() as i64),
                Edge::Bottom => (0, (shift * h as f64).round() as i64),
            };
            for y in 0..hi {
                for x in 0..wi {
                    let (sx, sy) = (x - ox, y - oy);
                    let src = if (0..wi).contains(&sx) && (0..hi).contains(&sy) {
                        &to[((sy * wi + sx) * 4) as usize..][..4]
                    } else {
                        &from[((y * wi + x) * 4) as usize..][..4]
                    };
                    out[((y * wi + x) * 4) as usize..][..4].copy_from_slice(src);
                }
            }
        }
        TransitionKind::Wipe { .. } | TransitionKind::Clock | TransitionKind::Iris => {
            let (fw, fh) = (w as f32, h as f32);
            for y in 0..h {
                for x in 0..w {
                    let k = reveal(kind, t, x as f32 + 0.5, y as f32 + 0.5, fw, fh);
                    let i = ((y * w + x) * 4) as usize;
                    for c in 0..4 {
                        out[i + c] = mix(from[i + c], to[i + c], k);
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, c: [u8; 4]) -> Vec<u8> {
        c.repeat((w * h) as usize)
    }
    fn at(frame: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * w + x) * 4) as usize;
        frame[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn transitions_progress_over_the_incoming_frames() {
        let t = Transition {
            kind: TransitionKind::Dissolve,
            frames: 4,
        };
        assert_eq!(t.progress(0), Some(0.125));
        assert_eq!(t.progress(4), None);
        assert!(Transition::default().is_cut());
        let json = serde_json::to_string(&Transition {
            kind: TransitionKind::Wipe { from: Edge::Left },
            frames: 12,
        })
        .unwrap();
        assert_eq!(json, r#"{"kind":"wipe","from":"left","frames":12}"#);
        let back: Transition = serde_json::from_str(&json).unwrap();
        assert_eq!(back.frames, 12);
    }

    #[test]
    fn every_kind_starts_on_the_outgoing_and_ends_on_the_incoming_picture() {
        let (w, h) = (20, 10);
        let (a, b) = (
            solid(w, h, [0, 0, 0, 255]),
            solid(w, h, [255, 255, 255, 255]),
        );
        let kinds = [
            TransitionKind::Dissolve,
            TransitionKind::Wipe { from: Edge::Left },
            TransitionKind::Wipe { from: Edge::Bottom },
            TransitionKind::Clock,
            TransitionKind::Iris,
            TransitionKind::Slide { from: Edge::Right },
            TransitionKind::Slide { from: Edge::Top },
            TransitionKind::FadeToColor { color: [255, 0, 0] },
        ];
        for kind in kinds {
            let start = blend(kind, 0., &a, &b, w, h);
            let end = blend(kind, 1., &a, &b, w, h);
            assert_eq!(start, a, "{kind:?} at 0");
            assert_eq!(end, b, "{kind:?} at 1");
        }
    }

    #[test]
    fn shaped_wipes_reveal_where_expected() {
        let (w, h) = (20, 10);
        let (a, b) = (
            solid(w, h, [0, 0, 0, 255]),
            solid(w, h, [255, 255, 255, 255]),
        );
        let wipe = blend(TransitionKind::Wipe { from: Edge::Left }, 0.5, &a, &b, w, h);
        assert_eq!(at(&wipe, w, 2, 5)[0], 255);
        assert_eq!(at(&wipe, w, 17, 5)[0], 0);
        let iris = blend(TransitionKind::Iris, 0.3, &a, &b, w, h);
        assert_eq!(at(&iris, w, 10, 5)[0], 255);
        assert_eq!(at(&iris, w, 0, 0)[0], 0);
        let clock = blend(TransitionKind::Clock, 0.25, &a, &b, w, h);
        // A quarter turn covers the top right, not the left.
        assert_eq!(at(&clock, w, 14, 2)[0], 255);
        assert_eq!(at(&clock, w, 3, 7)[0], 0);
        let slide = blend(
            TransitionKind::Slide { from: Edge::Right },
            0.5,
            &a,
            &b,
            w,
            h,
        );
        assert_eq!(at(&slide, w, 15, 5)[0], 255);
        assert_eq!(at(&slide, w, 4, 5)[0], 0);
        let fade = blend(
            TransitionKind::FadeToColor { color: [255, 0, 0] },
            0.5,
            &a,
            &b,
            w,
            h,
        );
        assert_eq!(at(&fade, w, 5, 5), [255, 0, 0, 255]);
    }
}
