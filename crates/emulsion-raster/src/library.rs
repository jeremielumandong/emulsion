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
    "Manga",
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
    let mut presets = vec![
        // ── Manga: nibs, liners, tones and effects ──
        p(
            "Maru pen",
            "Manga",
            "Mapping nib: very fine, thickens only under pressure; for eyes, hair and hatching",
            Brush {
                size: 4.0,
                hardness: 0.75,
                spacing: 0.06,
                size_pressure: 1.0,
                speed_thins: 0.9,
                taper_start: 6.0,
                taper_end: 8.0,
                stabilizer: 0.4,
                grain_strength: 1.0,
                grain_scale: 5.0,
                flow: 0.8,
                ..d
            },
        ),
        p(
            "Kabura pen",
            "Manga",
            "Turnip nib: even medium line with slight swell; for backgrounds and lettering",
            Brush {
                size: 6.0,
                hardness: 1.0,
                spacing: 0.06,
                size_pressure: 0.35,
                speed_thins: 0.3,
                taper_start: 4.0,
                taper_end: 4.0,
                stabilizer: 0.5,
                ..d
            },
        ),
        p(
            "Milli pen 0.3",
            "Manga",
            "Technical liner, constant 2 px",
            Brush {
                size: 2.0,
                hardness: 1.0,
                spacing: 0.1,
                stabilizer: 0.6,
                ..d
            },
        ),
        p(
            "Milli pen 0.8",
            "Manga",
            "Technical liner, constant 5 px; panel borders",
            Brush {
                size: 5.0,
                hardness: 1.0,
                spacing: 0.1,
                stabilizer: 0.7,
                ..d
            },
        ),
        p(
            "Fude brush",
            "Manga",
            "Brush pen for bold expressive lines and shadows",
            Brush {
                size: 22.0,
                hardness: 0.95,
                spacing: 0.05,
                size_pressure: 1.0,
                speed_thins: 1.0,
                taper_start: 30.0,
                taper_end: 50.0,
                stabilizer: 0.5,
                ..d
            },
        ),
        p(
            "Speed lines",
            "Manga",
            "Hairline that fades out; flick for motion and focus lines",
            Brush {
                size: 5.0,
                hardness: 1.0,
                spacing: 0.05,
                size_pressure: 1.0,
                speed_thins: 1.0,
                taper_start: 2.0,
                taper_end: 160.0,
                stabilizer: 0.7,
                ..d
            },
        ),
        p(
            "Screentone 20%",
            "Manga",
            "Light dot tone fixed to the page",
            Brush {
                size: 120.0,
                hardness: 1.0,
                spacing: 0.1,
                grain: GrainKind::Halftone,
                grain_scale: 6.0,
                grain_strength: 0.2,
                ..d
            },
        ),
        p(
            "Screentone 40%",
            "Manga",
            "Medium dot tone",
            Brush {
                size: 120.0,
                hardness: 1.0,
                spacing: 0.1,
                grain: GrainKind::Halftone,
                grain_scale: 6.0,
                grain_strength: 0.4,
                ..d
            },
        ),
        p(
            "Screentone 60%",
            "Manga",
            "Dark dot tone for shadows",
            Brush {
                size: 120.0,
                hardness: 1.0,
                spacing: 0.1,
                grain: GrainKind::Halftone,
                grain_scale: 6.0,
                grain_strength: 0.6,
                ..d
            },
        ),
        p(
            "Hatching",
            "Manga",
            "Parallel 45° lines fixed to the page",
            Brush {
                size: 80.0,
                hardness: 1.0,
                spacing: 0.1,
                grain: GrainKind::Hatch,
                grain_scale: 7.0,
                grain_strength: 1.0,
                ..d
            },
        ),
        p(
            "Cross hatch",
            "Manga",
            "Two crossing line directions for deeper shade",
            Brush {
                size: 80.0,
                hardness: 1.0,
                spacing: 0.1,
                grain: GrainKind::CrossHatch,
                grain_scale: 8.0,
                grain_strength: 1.0,
                ..d
            },
        ),
        p(
            "Blue pencil",
            "Manga",
            "Non-photo blue rough; set the colour to #A4C8FF and sketch loosely",
            Brush {
                size: 5.0,
                hardness: 0.5,
                flow: 0.4,
                opacity: 0.8,
                spacing: 0.1,
                grain: GrainKind::Paper,
                grain_scale: 3.0,
                grain_strength: 0.7,
                size_pressure: 0.5,
                flow_pressure: 0.6,
                speed_thins: 0.4,
                ..d
            },
        ),
        p(
            "Sketch pencil",
            "Manga",
            "Loose graphite for roughs and construction lines",
            Brush {
                size: 6.0,
                hardness: 0.4,
                flow: 0.35,
                opacity: 0.85,
                spacing: 0.1,
                grain: GrainKind::Paper,
                grain_scale: 3.0,
                grain_strength: 0.8,
                size_pressure: 0.5,
                flow_pressure: 0.7,
                speed_thins: 0.5,
                ..d
            },
        ),
        p(
            "White ink",
            "Manga",
            "Opaque correction and highlights; set the colour to white",
            Brush {
                size: 4.0,
                hardness: 1.0,
                spacing: 0.06,
                size_pressure: 0.6,
                taper_start: 3.0,
                taper_end: 3.0,
                stabilizer: 0.4,
                ..d
            },
        ),
        p(
            "Ink wash",
            "Manga",
            "Diluted ink for grey washes; layers darken",
            Brush {
                size: 60.0,
                hardness: 0.3,
                flow: 0.25,
                opacity: 0.5,
                spacing: 0.08,
                wetness: 0.4,
                edge_darken: 0.5,
                blend: BrushBlend::Multiply,
                ..d
            },
        ),
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
                grain_strength: 0.8,
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
                grain_scale: 3.5,
                grain_strength: 0.85,
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
                grain: GrainKind::Chalk,
                grain_scale: 4.0,
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
                grain_scale: 2.5,
                grain_strength: 0.5,
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
                grain_strength: 0.85,
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
                grain_scale: 6.0,
                grain_strength: 0.95,
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
                grain: GrainKind::Chalk,
                grain_scale: 4.0,
                grain_strength: 0.9,
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
                hardness: 0.5,
                flow: 0.7,
                spacing: 0.12,
                grain: GrainKind::Chalk,
                grain_scale: 5.0,
                grain_strength: 0.85,
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
                grain_strength: 0.7,
                blend: BrushBlend::Multiply,
                edge_darken: 0.9,
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
                edge_darken: 0.7,
                grain: GrainKind::Paper,
                grain_scale: 6.0,
                grain_strength: 0.45,
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
                edge_darken: 0.6,
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
                grain_scale: 2.5,
                grain_strength: 0.9,
                wetness: 0.35,
                color_jitter: 0.06,
                relief: 0.7,
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
                grain_scale: 3.0,
                grain_strength: 0.75,
                wetness: 0.25,
                size_pressure: 0.5,
                follow_path: true,
                relief: 0.6,
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
                grain_strength: 0.9,
                relief: 1.0,
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
                grain_strength: 0.5,
                wetness: 0.1,
                relief: 0.3,
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
    ];
    // Original presets exercising the authored dynamics; no third-party artwork.
    let mut marker = Brush {
        size: 36.,
        grain: GrainKind::Paper,
        grain_strength: 0.5,
        flow: 0.65,
        ..d
    };
    marker.advanced.grain.mode = crate::paint::GrainMode::Moving;
    marker.advanced.path.lateral_jitter = 0.035;
    marker.advanced.rendering = crate::paint::RenderingMode::Accumulating;
    presets.push(p(
        "Moving paper marker",
        "Marker",
        "Original paper texture moving with each stamp",
        marker,
    ));
    let mut linen = Brush {
        size: 64.,
        roundness: 0.3,
        follow_path: true,
        grain: GrainKind::CrossHatch,
        grain_scale: 3.,
        grain_strength: 0.7,
        ..d
    };
    linen.advanced.grain.rotation = 25.;
    linen.advanced.stabilization.stages = 3;
    linen.advanced.stabilization.amount = 0.3;
    presets.push(p(
        "Woven roller",
        "Chalk",
        "Original crossed-fiber roller with staged smoothing",
        linen,
    ));
    let mut spray = Brush {
        size: 12.,
        spacing: 0.4,
        hardness: 0.7,
        flow: 0.35,
        ..d
    };
    spray.advanced.shape.count = 5;
    spray.advanced.shape.count_jitter = 0.7;
    spray.advanced.path.lateral_jitter = 1.5;
    spray.advanced.path.linear_jitter = 0.5;
    spray.advanced.dynamics.opacity_jitter = 0.5;
    presets.push(p(
        "Scattered pigment",
        "Airbrush",
        "Seeded scattered dabs with independent coverage variation",
        spray,
    ));
    let mut stamp = Brush {
        size: 42.,
        roundness: 0.22,
        spacing: 0.7,
        hardness: 0.95,
        flow: 0.8,
        ..d
    };
    stamp.advanced.shape.rotation_jitter = 1.;
    stamp.advanced.color.stamp_hue = 0.04;
    stamp.advanced.color.stamp_lightness = 0.12;
    presets.push(p(
        "Turning petals",
        "Oil",
        "Original rotating elliptical stamps with gentle pigment variation",
        stamp,
    ));
    presets
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
