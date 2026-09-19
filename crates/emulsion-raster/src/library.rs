//! The built-in brush library: named brushes by medium.
//!
//! Every entry is a complete [`Brush`], so the same list serves the brush
//! panel and the assistant's painting tools.

use crate::paint::{Brush, BrushBlend, GrainKind};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BrushPreset {
    pub name: String,
    pub category: String,
    /// One line on what it is for.
    pub note: String,
    pub brush: Brush,
}

pub const CATEGORIES: &[&str] = &[
    "Ink",
    "Pencil",
    "Chalk",
    "Marker",
    "Watercolour",
    "Oil",
    "Airbrush",
    "Eraser",
    "Smudge",
];

fn p(name: &str, category: &str, note: &str, brush: Brush) -> BrushPreset {
    BrushPreset {
        name: name.into(),
        category: category.into(),
        note: note.into(),
        brush: brush.sanitized(),
    }
}

/// Every built-in brush, grouped by category in `CATEGORIES` order.
pub fn library() -> Vec<BrushPreset> {
    let d = Brush::default();
    vec![
        // ── Ink: crisp, opaque, pressure- and speed-sensitive lines ──
        p(
            "Fine liner",
            "Ink",
            "Even 3 px line for outlines and hatching",
            Brush {
                size: 3.0,
                hardness: 1.0,
                spacing: 0.08,
                stabilizer: 0.3,
                ..d
            },
        ),
        p(
            "G-pen",
            "Ink",
            "Manga nib: thick to thin with pressure or speed, tapered ends",
            Brush {
                size: 9.0,
                hardness: 0.95,
                spacing: 0.06,
                size_pressure: 0.9,
                speed_thins: 0.8,
                taper_start: 14.0,
                taper_end: 18.0,
                stabilizer: 0.45,
                ..d
            },
        ),
        p(
            "Brush pen",
            "Ink",
            "Long tapers, big swell; for gestures and lettering",
            Brush {
                size: 18.0,
                hardness: 0.9,
                spacing: 0.05,
                size_pressure: 1.0,
                speed_thins: 1.0,
                taper_start: 40.0,
                taper_end: 60.0,
                stabilizer: 0.55,
                ..d
            },
        ),
        p(
            "Technical pen",
            "Ink",
            "Rigid 1.5 px line, heavily stabilised",
            Brush {
                size: 1.5,
                hardness: 1.0,
                spacing: 0.1,
                stabilizer: 0.7,
                ..d
            },
        ),
        p(
            "Dry ink",
            "Ink",
            "Broken, textured ink for rough shading",
            Brush {
                size: 10.0,
                hardness: 0.9,
                spacing: 0.07,
                grain: GrainKind::Speckle,
                grain_scale: 2.0,
                grain_strength: 0.8,
                size_pressure: 0.6,
                speed_thins: 0.5,
                ..d
            },
        ),
        // ── Pencil: soft graphite that shows the paper ──
        p(
            "HB pencil",
            "Pencil",
            "Light, precise graphite",
            Brush {
                size: 4.0,
                hardness: 0.6,
                flow: 0.35,
                opacity: 0.9,
                spacing: 0.1,
                grain: GrainKind::Paper,
                grain_scale: 3.0,
                grain_strength: 0.7,
                size_pressure: 0.4,
                flow_pressure: 0.7,
                speed_thins: 0.4,
                ..d
            },
        ),
        p(
            "2B soft",
            "Pencil",
            "Darker, wider tone; builds up with passes",
            Brush {
                size: 9.0,
                hardness: 0.3,
                flow: 0.45,
                spacing: 0.1,
                grain: GrainKind::Paper,
                grain_scale: 4.0,
                grain_strength: 0.8,
                size_pressure: 0.5,
                flow_pressure: 0.6,
                speed_thins: 0.4,
                ..d
            },
        ),
        p(
            "6B side",
            "Pencil",
            "The side of a soft pencil: broad, grainy tone",
            Brush {
                size: 30.0,
                hardness: 0.0,
                flow: 0.3,
                spacing: 0.15,
                roundness: 0.4,
                follow_path: true,
                grain: GrainKind::Paper,
                grain_scale: 5.0,
                grain_strength: 0.9,
                flow_pressure: 0.7,
                ..d
            },
        ),
        p(
            "Mechanical",
            "Pencil",
            "Thin, even, barely textured",
            Brush {
                size: 2.0,
                hardness: 0.9,
                flow: 0.6,
                spacing: 0.1,
                grain: GrainKind::Paper,
                grain_scale: 2.0,
                grain_strength: 0.3,
                stabilizer: 0.3,
                ..d
            },
        ),
        p(
            "Coloured pencil",
            "Pencil",
            "Waxy layered colour",
            Brush {
                size: 7.0,
                hardness: 0.5,
                flow: 0.3,
                spacing: 0.1,
                grain: GrainKind::Paper,
                grain_scale: 3.0,
                grain_strength: 0.75,
                color_jitter: 0.1,
                flow_pressure: 0.6,
                ..d
            },
        ),
        // ── Chalk and charcoal ──
        p(
            "Chalk",
            "Chalk",
            "Coarse tooth; skips over the paper",
            Brush {
                size: 24.0,
                hardness: 0.7,
                flow: 0.7,
                spacing: 0.12,
                roundness: 0.7,
                grain: GrainKind::Chalk,
                grain_scale: 6.0,
                grain_strength: 1.0,
                flow_pressure: 0.5,
                ..d
            },
        ),
        p(
            "Charcoal stick",
            "Chalk",
            "Dark, dusty, wide",
            Brush {
                size: 34.0,
                hardness: 0.4,
                flow: 0.55,
                spacing: 0.12,
                roundness: 0.5,
                follow_path: true,
                grain: GrainKind::Chalk,
                grain_scale: 7.0,
                grain_strength: 0.9,
                size_pressure: 0.3,
                flow_pressure: 0.6,
                ..d
            },
        ),
        p(
            "Conté",
            "Chalk",
            "Square-edged crayon for figure drawing",
            Brush {
                size: 14.0,
                hardness: 0.85,
                flow: 0.8,
                spacing: 0.1,
                roundness: 0.3,
                angle: 35.0,
                grain: GrainKind::Paper,
                grain_scale: 4.0,
                grain_strength: 0.7,
                size_pressure: 0.3,
                ..d
            },
        ),
        p(
            "Pastel",
            "Chalk",
            "Soft, blends what is already there",
            Brush {
                size: 40.0,
                hardness: 0.2,
                flow: 0.5,
                spacing: 0.12,
                grain: GrainKind::Canvas,
                grain_scale: 5.0,
                grain_strength: 0.6,
                wetness: 0.35,
                ..d
            },
        ),
        // ── Markers: multiply so overlaps darken ──
        p(
            "Chisel marker",
            "Marker",
            "Wide flat tip; strokes darken where they cross",
            Brush {
                size: 28.0,
                hardness: 1.0,
                flow: 1.0,
                opacity: 0.6,
                spacing: 0.05,
                roundness: 0.25,
                angle: 45.0,
                blend: BrushBlend::Multiply,
                ..d
            },
        ),
        p(
            "Soft marker",
            "Marker",
            "Alcohol-marker bleed with soft edges",
            Brush {
                size: 40.0,
                hardness: 0.5,
                flow: 0.9,
                opacity: 0.5,
                spacing: 0.06,
                blend: BrushBlend::Multiply,
                ..d
            },
        ),
        p(
            "Highlighter",
            "Marker",
            "Translucent flat colour",
            Brush {
                size: 36.0,
                hardness: 0.95,
                opacity: 0.35,
                spacing: 0.05,
                roundness: 0.3,
                angle: 90.0,
                blend: BrushBlend::Multiply,
                ..d
            },
        ),
        p(
            "Fine marker",
            "Marker",
            "Small round marker tip",
            Brush {
                size: 6.0,
                hardness: 0.95,
                opacity: 0.85,
                spacing: 0.06,
                blend: BrushBlend::Multiply,
                ..d
            },
        ),
        // ── Watercolour: translucent, wet, edges pool ──
        p(
            "Wash",
            "Watercolour",
            "Broad translucent wash; pigment pools at the edges and settles in the paper",
            Brush {
                size: 90.0,
                hardness: 0.15,
                flow: 0.18,
                opacity: 0.55,
                spacing: 0.08,
                wetness: 0.5,
                grain: GrainKind::Paper,
                grain_scale: 8.0,
                grain_strength: 0.6,
                blend: BrushBlend::Multiply,
                edge_darken: 0.8,
                color_jitter: 0.06,
                ..d
            },
        ),
        p(
            "Wet blend",
            "Watercolour",
            "Bleeds into neighbouring colour",
            Brush {
                size: 50.0,
                hardness: 0.1,
                flow: 0.35,
                opacity: 0.7,
                spacing: 0.08,
                wetness: 0.75,
                color_jitter: 0.08,
                blend: BrushBlend::Multiply,
                edge_darken: 0.6,
                grain: GrainKind::Paper,
                grain_scale: 6.0,
                grain_strength: 0.35,
                ..d
            },
        ),
        p(
            "Dry brush",
            "Watercolour",
            "Little paint, lots of paper",
            Brush {
                size: 40.0,
                hardness: 0.6,
                flow: 0.5,
                opacity: 0.8,
                spacing: 0.08,
                roundness: 0.45,
                follow_path: true,
                grain: GrainKind::Speckle,
                grain_scale: 3.0,
                grain_strength: 0.9,
                ..d
            },
        ),
        p(
            "Detail round",
            "Watercolour",
            "Small round for edges and detail",
            Brush {
                size: 10.0,
                hardness: 0.4,
                flow: 0.5,
                opacity: 0.8,
                spacing: 0.08,
                wetness: 0.3,
                size_pressure: 0.7,
                speed_thins: 0.4,
                blend: BrushBlend::Multiply,
                edge_darken: 0.5,
                grain: GrainKind::Paper,
                grain_scale: 4.0,
                grain_strength: 0.3,
                ..d
            },
        ),
        // ── Oil and acrylic: opaque, mixing, bristly ──
        p(
            "Flat bristle",
            "Oil",
            "Loaded flat brush, bristle marks along the stroke",
            Brush {
                size: 44.0,
                hardness: 0.85,
                flow: 0.9,
                spacing: 0.05,
                roundness: 0.28,
                follow_path: true,
                grain: GrainKind::Bristle,
                grain_scale: 3.0,
                grain_strength: 0.75,
                wetness: 0.35,
                color_jitter: 0.06,
                ..d
            },
        ),
        p(
            "Round oil",
            "Oil",
            "Round loaded brush",
            Brush {
                size: 26.0,
                hardness: 0.7,
                flow: 0.95,
                spacing: 0.06,
                grain: GrainKind::Bristle,
                grain_scale: 4.0,
                grain_strength: 0.55,
                wetness: 0.25,
                size_pressure: 0.5,
                follow_path: true,
                ..d
            },
        ),
        p(
            "Impasto",
            "Oil",
            "Thick paint that drags the colour under it",
            Brush {
                size: 36.0,
                hardness: 0.9,
                flow: 1.0,
                spacing: 0.04,
                roundness: 0.5,
                follow_path: true,
                wetness: 0.6,
                grain: GrainKind::Bristle,
                grain_scale: 2.0,
                grain_strength: 0.85,
                ..d
            },
        ),
        p(
            "Acrylic",
            "Oil",
            "Fast-drying opaque colour, slight texture",
            Brush {
                size: 30.0,
                hardness: 0.8,
                flow: 1.0,
                spacing: 0.07,
                grain: GrainKind::Canvas,
                grain_scale: 3.0,
                grain_strength: 0.25,
                wetness: 0.1,
                ..d
            },
        ),
        p(
            "Fan blender",
            "Oil",
            "Softens and blends without adding much colour",
            Brush {
                size: 60.0,
                hardness: 0.0,
                flow: 0.4,
                spacing: 0.1,
                roundness: 0.4,
                follow_path: true,
                wetness: 0.9,
                ..d
            },
        ),
        // ── Airbrush ──
        p(
            "Airbrush",
            "Airbrush",
            "Soft build-up spray",
            Brush {
                size: 120.0,
                hardness: 0.0,
                flow: 0.08,
                spacing: 0.06,
                flow_pressure: 0.8,
                ..d
            },
        ),
        p(
            "Fine spray",
            "Airbrush",
            "Small soft spray for shading",
            Brush {
                size: 30.0,
                hardness: 0.0,
                flow: 0.15,
                spacing: 0.06,
                flow_pressure: 0.8,
                ..d
            },
        ),
        p(
            "Speckle spray",
            "Airbrush",
            "Scattered droplets",
            Brush {
                size: 6.0,
                hardness: 1.0,
                flow: 0.9,
                spacing: 0.3,
                scatter: 6.0f32.min(1.0),
                size_jitter: 0.8,
                ..d
            },
        ),
        // ── Erasers ──
        p(
            "Hard eraser",
            "Eraser",
            "Clean removal",
            Brush {
                size: 30.0,
                hardness: 1.0,
                spacing: 0.08,
                ..d
            },
        ),
        p(
            "Soft eraser",
            "Eraser",
            "Feathered removal",
            Brush {
                size: 60.0,
                hardness: 0.0,
                flow: 0.5,
                spacing: 0.08,
                ..d
            },
        ),
        p(
            "Kneaded eraser",
            "Eraser",
            "Lifts tone gradually, shows the paper",
            Brush {
                size: 40.0,
                hardness: 0.3,
                flow: 0.25,
                spacing: 0.1,
                grain: GrainKind::Paper,
                grain_scale: 4.0,
                grain_strength: 0.6,
                ..d
            },
        ),
        // ── Smudge ──
        p(
            "Blend",
            "Smudge",
            "Soft, long smear",
            Brush {
                size: 40.0,
                hardness: 0.2,
                flow: 0.5,
                spacing: 0.06,
                wetness: 0.9,
                ..d
            },
        ),
        p(
            "Smear",
            "Smudge",
            "Short hard smear, like a finger",
            Brush {
                size: 24.0,
                hardness: 0.8,
                flow: 0.8,
                spacing: 0.06,
                wetness: 0.6,
                ..d
            },
        ),
        p(
            "Bristle smudge",
            "Smudge",
            "Drags colour in streaks",
            Brush {
                size: 40.0,
                hardness: 0.7,
                flow: 0.7,
                spacing: 0.06,
                roundness: 0.3,
                follow_path: true,
                wetness: 0.8,
                grain: GrainKind::Canvas,
                grain_scale: 3.0,
                grain_strength: 0.5,
                ..d
            },
        ),
    ]
}

/// Find a built-in brush by name, case-insensitively.
pub fn find(name: &str) -> Option<BrushPreset> {
    let n = name.trim().to_lowercase();
    library().into_iter().find(|p| p.name.to_lowercase() == n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_is_well_formed() {
        let lib = library();
        let mut names = std::collections::HashSet::new();
        for b in &lib {
            assert!(names.insert(b.name.clone()), "duplicate {}", b.name);
            assert!(
                CATEGORIES.contains(&b.category.as_str()),
                "{} has category {}",
                b.name,
                b.category
            );
            assert_eq!(b.brush, b.brush.sanitized(), "{} is out of range", b.name);
        }
        for c in CATEGORIES {
            assert!(lib.iter().any(|b| b.category == *c), "empty category {c}");
        }
        assert!(find("g-PEN").is_some());
    }
}
