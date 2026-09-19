//! Tier-0 suggestions from image statistics: no models, no network.

use emulsion_core::Document;
use emulsion_raster::composite::{flatten, level_size};
use emulsion_raster::{Adjustment, color};

#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    /// Chip text, e.g. "Stretch levels 12–231".
    pub label: String,
    /// Name the node gets when accepted.
    pub node_name: String,
    pub adjustment: Adjustment,
    /// What accepting does; adjustments add a node, actions run a tool.
    pub kind: Kind,
}

/// What a suggestion does when accepted.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Kind {
    /// Add `adjustment` as a node.
    #[default]
    Adjust,
    /// Run a named editor action: "lens_profile", "restore_faces",
    /// "remove_background", "select_subject".
    Action(String),
}

impl Suggestion {
    /// A suggestion that runs an editor action instead of adding a node.
    pub fn action(label: impl Into<String>, action: &str) -> Self {
        Suggestion {
            label: label.into(),
            node_name: action.to_string(),
            adjustment: Adjustment::Exposure {
                exposure: 0.0,
                offset: 0.0,
                gamma: 1.0,
            },
            kind: Kind::Action(action.to_string()),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    /// Encoded-luma percentiles in [0,1].
    pub p_low: f32,
    pub median: f32,
    pub p_high: f32,
    /// Fraction of pixels at or near white.
    pub clipped: f32,
    /// Mean encoded red minus blue.
    pub warmth: f32,
    /// Mean HSL-style saturation.
    pub saturation: f32,
}

/// Statistics of the composite at a small mip level.
pub fn stats(doc: &Document) -> Option<Stats> {
    let mut level = 0;
    while level_size(doc.width, doc.height, level)
        .0
        .max(level_size(doc.width, doc.height, level).1)
        > 384
    {
        level += 1;
    }
    let flat = flatten(&doc.composite_tree(), level);
    let px = flat.to_pixels();
    let mut hist = [0u32; 256];
    let (mut n, mut clipped, mut warm, mut sat) = (0u32, 0u32, 0f64, 0f64);
    for p in px {
        let f = color::px_to_f(p);
        if f[3] < 0.5 {
            continue;
        }
        let inv = 1.0 / f[3];
        let e = [0, 1, 2].map(|i| color::linear_to_srgb((f[i] * inv).clamp(0.0, 1.0)));
        let l = 0.2126 * e[0] + 0.7152 * e[1] + 0.0722 * e[2];
        hist[(l * 255.0).round().clamp(0.0, 255.0) as usize] += 1;
        if l > 0.985 {
            clipped += 1;
        }
        warm += (e[0] - e[2]) as f64;
        let (mx, mn) = (e[0].max(e[1]).max(e[2]), e[0].min(e[1]).min(e[2]));
        sat += if mx > 0.0 {
            ((mx - mn) / mx) as f64
        } else {
            0.0
        };
        n += 1;
    }
    if n < 64 {
        return None;
    }
    let pct = |q: f32| {
        let target = (q * n as f32) as u32;
        let mut acc = 0;
        for (i, c) in hist.iter().enumerate() {
            acc += c;
            if acc > target {
                return i as f32 / 255.0;
            }
        }
        1.0
    };
    Some(Stats {
        p_low: pct(0.005),
        median: pct(0.5),
        p_high: pct(0.995),
        clipped: clipped as f32 / n as f32,
        warmth: (warm / n as f64) as f32,
        saturation: (sat / n as f64) as f32,
    })
}

/// Up to four proposals, strongest first.
pub fn from_stats(s: &Stats) -> Vec<Suggestion> {
    let mut out = Vec::new();
    let span = s.p_high - s.p_low;
    if span < 0.85 && span > 0.05 {
        let (b, w) = (
            (s.p_low * 255.0).round(),
            (s.p_high * 255.0).round().max(s.p_low * 255.0 + 2.0),
        );
        out.push(Suggestion {
            label: format!("Stretch levels {b:.0}–{w:.0}"),
            node_name: "Levels (suggested)".into(),
            kind: Kind::Adjust,
            adjustment: Adjustment::Levels {
                in_black: b,
                in_white: w,
                gamma: 1.0,
                out_black: 0.0,
                out_white: 255.0,
            },
        });
    }
    if s.clipped > 0.02 {
        out.push(Suggestion {
            label: format!("Recover highlights ({:.0}% clipped)", s.clipped * 100.0),
            node_name: "Recover highlights".into(),
            kind: Kind::Adjust,
            adjustment: Adjustment::Exposure {
                exposure: -0.4,
                offset: 0.0,
                gamma: 1.0,
            },
        });
    } else if s.median < 0.3 {
        out.push(Suggestion {
            label: "Lift the shadows".into(),
            node_name: "Lift shadows".into(),
            kind: Kind::Adjust,
            adjustment: Adjustment::Levels {
                in_black: 0.0,
                in_white: 255.0,
                gamma: 1.35,
                out_black: 0.0,
                out_white: 255.0,
            },
        });
    }
    if s.warmth > 0.12 {
        out.push(Suggestion {
            label: "Cool the warm cast".into(),
            node_name: "Cool down".into(),
            kind: Kind::Adjust,
            adjustment: Adjustment::WhiteBalance {
                temperature: -20.0,
                tint: 0.0,
            },
        });
    } else if s.warmth < -0.12 {
        out.push(Suggestion {
            label: "Warm the cool cast".into(),
            node_name: "Warm up".into(),
            kind: Kind::Adjust,
            adjustment: Adjustment::WhiteBalance {
                temperature: 20.0,
                tint: 0.0,
            },
        });
    }
    if s.saturation < 0.12 && s.saturation > 0.01 {
        out.push(Suggestion {
            label: "Add saturation".into(),
            node_name: "Saturation (suggested)".into(),
            kind: Kind::Adjust,
            adjustment: Adjustment::HueSaturation {
                hue: 0.0,
                saturation: 20.0,
                lightness: 0.0,
            },
        });
    }
    out.truncate(4);
    out
}

/// Suggestions for `doc`, skipping any already accepted.
pub fn suggest(doc: &Document) -> Vec<Suggestion> {
    let Some(s) = stats(doc) else { return vec![] };
    from_stats(&s)
        .into_iter()
        .filter(|g| !doc.nodes.iter().any(|n| n.name == g.node_name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::command::Slot;
    use emulsion_core::{Command, Node};
    use emulsion_raster::{Placement, Raster};
    use std::sync::Arc;

    fn doc_with(pixels: impl Fn(u32, u32) -> [u8; 4]) -> Document {
        let (w, h) = (128u32, 128u32);
        let pixels = &pixels;
        let data: Vec<u8> = (0..h)
            .flat_map(|y| (0..w).flat_map(move |x| pixels(x, y)))
            .collect();
        let mut d = Document::new(w, h);
        Command::AddNode {
            node: Box::new(Node::raster(
                0,
                "img",
                Arc::new(Raster::from_srgba8(w, h, &data)),
                Placement::default(),
            )),
            slot: Slot::TOP,
        }
        .apply(&mut d)
        .unwrap();
        d
    }

    #[test]
    fn flat_dark_warm_image_gets_levels_shadows_and_cool() {
        // Values 40..=120, red-heavy.
        let d = doc_with(|x, _| {
            [
                (60 + x / 2) as u8,
                (40 + x / 3) as u8,
                (30 + x / 4) as u8,
                255,
            ]
        });
        let labels: Vec<String> = suggest(&d).into_iter().map(|s| s.label).collect();
        assert!(labels[0].starts_with("Stretch levels"), "{labels:?}");
        assert!(labels.contains(&"Lift the shadows".to_string()));
        assert!(labels.contains(&"Cool the warm cast".to_string()));
    }

    #[test]
    fn full_range_neutral_image_gets_nothing() {
        // A grey ramp through every code value: nothing to fix.
        let d = doc_with(|x, y| {
            let v = ((y * 128 + x) * 255 / (128 * 128 - 1)) as u8;
            [v, v, v, 255]
        });
        let s = stats(&d).unwrap();
        assert!(s.p_high - s.p_low > 0.95 && s.warmth.abs() < 0.01, "{s:?}");
        assert!(suggest(&d).is_empty(), "{:?}", suggest(&d));
    }

    #[test]
    fn accepted_suggestions_are_not_repeated() {
        let mut d = doc_with(|x, _| {
            [
                (60 + x / 2) as u8,
                (40 + x / 3) as u8,
                (30 + x / 4) as u8,
                255,
            ]
        });
        let first = suggest(&d)[0].clone();
        let mut n = Node::adjust(0, first.adjustment.clone());
        n.name = first.node_name.clone();
        Command::AddNode {
            node: Box::new(n),
            slot: Slot::TOP,
        }
        .apply(&mut d)
        .unwrap();
        assert!(suggest(&d).iter().all(|s| s.node_name != first.node_name));
    }
}
