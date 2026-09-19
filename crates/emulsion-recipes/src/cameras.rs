//! Camera and film presets: instant film, disposables, colour negative
//! stocks, black and white, cross-processing and early digital, in the
//! spirit of the "shoot it like it was 1998" phone apps. Each is an
//! ordinary recipe, so it previews, applies and edits like any other.

use crate::effects::{Frame, LeakSide};
use crate::{Grain, GrainSize, Recipe, Strength, WhiteBalance};

fn base(name: &str, sim: &str, tags: &[&str], notes: &str) -> Recipe {
    Recipe {
        name: name.into(),
        author: "Emulsion camera set".into(),
        license: "CC0".into(),
        notes: notes.into(),
        tags: tags.iter().map(|t| t.to_string()).collect(),
        film_simulation: sim.into(),
        ..Recipe::default()
    }
}

fn grain(strength: Strength, size: GrainSize) -> Grain {
    Grain { strength, size }
}

fn wb(preset: &str, red: i32, blue: i32) -> WhiteBalance {
    WhiteBalance {
        preset: preset.into(),
        kelvin: None,
        red,
        blue,
    }
}

/// The built-in camera looks.
pub fn presets() -> Vec<Recipe> {
    vec![
        Recipe {
            highlight: -1.0,
            shadow: -1.0,
            color: -1.0,
            white_balance: wb("daylight", 3, -1),
            fade: 28.0,
            vignette: 22.0,
            split_shadows: [0.0, 8.0, 18.0],
            split_highlights: [14.0, 6.0, -8.0],
            grain: grain(Strength::Weak, GrainSize::Small),
            frame: Frame::Polaroid,
            ..base(
                "Polaroid SX-70",
                "nostalgic-negative",
                &["instant", "camera", "frame"],
                "Warm, soft instant print with lifted blacks, a cool shadow cast and the classic white frame.",
            )
        },
        Recipe {
            highlight: -2.0,
            shadow: 1.0,
            color: 1.0,
            white_balance: wb("daylight", 1, -3),
            fade: 14.0,
            vignette: 30.0,
            split_shadows: [-6.0, 0.0, 14.0],
            split_highlights: [6.0, 4.0, -4.0],
            frame: Frame::Polaroid,
            ..base(
                "Instax Mini",
                "provia",
                &["instant", "camera", "frame"],
                "Brighter, punchier instant film: cool shadows, milky highlights, deep bottom border.",
            )
        },
        Recipe {
            highlight: 1.0,
            shadow: 2.0,
            color: 2.0,
            sharpness: -2.0,
            white_balance: wb("daylight", 4, -4),
            grain: grain(Strength::Strong, GrainSize::Large),
            vignette: 40.0,
            light_leak: 35.0,
            leak_color: [255, 130, 60],
            leak_side: LeakSide::Right,
            dust: 15.0,
            date_stamp: true,
            date: "'98 7 12".into(),
            ..base(
                "Disposable camera",
                "classic-negative",
                &["film", "camera", "date"],
                "Plastic lens, on-camera flash energy, heavy grain, a leak from the film door and the orange date stamp.",
            )
        },
        Recipe {
            highlight: 0.0,
            shadow: 1.0,
            color: 2.0,
            white_balance: wb("daylight", 3, -2),
            grain: grain(Strength::Weak, GrainSize::Small),
            split_shadows: [4.0, 0.0, -6.0],
            split_highlights: [10.0, 6.0, -10.0],
            vignette: 10.0,
            ..base(
                "Kodak Gold 200",
                "classic-negative",
                &["film", "colour negative"],
                "Warm consumer colour negative: golden highlights, gentle contrast, a little grain.",
            )
        },
        Recipe {
            highlight: -1.0,
            shadow: 0.0,
            color: 1.0,
            white_balance: wb("daylight", -1, 1),
            grain: grain(Strength::Weak, GrainSize::Small),
            split_shadows: [0.0, 6.0, 4.0],
            split_highlights: [2.0, 0.0, -4.0],
            ..base(
                "Fuji Superia 400",
                "pro-neg-hi",
                &["film", "colour negative"],
                "Cooler, greener consumer stock with a slight magenta highlight roll-off.",
            )
        },
        Recipe {
            highlight: -2.0,
            shadow: -1.0,
            color: -2.0,
            white_balance: wb("daylight", 2, 0),
            grain: grain(Strength::Weak, GrainSize::Small),
            fade: 8.0,
            split_highlights: [6.0, 3.0, 0.0],
            ..base(
                "Kodak Portra 400",
                "pro-neg-std",
                &["film", "colour negative", "portrait"],
                "Soft, low-saturation skin-tone film with pastel highlights.",
            )
        },
        Recipe {
            highlight: -1.0,
            shadow: 1.0,
            color: 2.0,
            white_balance: wb("tungsten", 0, 0),
            grain: grain(Strength::Weak, GrainSize::Large),
            split_shadows: [-10.0, -4.0, 18.0],
            split_highlights: [20.0, 4.0, -6.0],
            vignette: 15.0,
            ..base(
                "CineStill 800T",
                "eterna",
                &["film", "cinema", "night"],
                "Tungsten-balanced cinema stock: teal shadows, red halation-like highlights, made for neon.",
            )
        },
        Recipe {
            highlight: 1.0,
            shadow: 2.0,
            grain: grain(Strength::Strong, GrainSize::Large),
            vignette: 20.0,
            ..base(
                "Ilford HP5 Plus",
                "acros",
                &["film", "black and white"],
                "Classic 400-speed black and white: gritty grain, rich mid-greys.",
            )
        },
        Recipe {
            highlight: 2.0,
            shadow: 3.0,
            grain: grain(Strength::Strong, GrainSize::Small),
            vignette: 25.0,
            ..base(
                "Kodak Tri-X 400",
                "acros-r",
                &["film", "black and white"],
                "Punchy reportage black and white with deep blacks and tight grain.",
            )
        },
        Recipe {
            highlight: 2.0,
            shadow: 2.0,
            color: 3.0,
            white_balance: wb("daylight", -1, 2),
            grain: grain(Strength::Weak, GrainSize::Large),
            split_shadows: [-10.0, 4.0, 12.0],
            split_highlights: [10.0, 12.0, -14.0],
            vignette: 50.0,
            light_leak: 12.0,
            leak_color: [255, 120, 80],
            leak_side: LeakSide::Left,
            ..base(
                "Lomo cross-process",
                "velvia",
                &["film", "lomo", "cross process"],
                "Slide film in the wrong chemistry: cyan shadows, yellow highlights, saturated, heavy vignette.",
            )
        },
        Recipe {
            highlight: -1.0,
            shadow: -2.0,
            color: -1.0,
            white_balance: wb("daylight", 2, -3),
            grain: grain(Strength::Strong, GrainSize::Large),
            fade: 36.0,
            split_shadows: [6.0, 10.0, -4.0],
            split_highlights: [12.0, 8.0, -14.0],
            vignette: 18.0,
            dust: 40.0,
            frame: Frame::Paper,
            ..base(
                "Old print",
                "nostalgic-negative",
                &["old", "frame", "print"],
                "A faded, yellowed paper print with dust, scratches and worn corners.",
            )
        },
        Recipe {
            highlight: 0.0,
            shadow: 1.0,
            color: -1.0,
            white_balance: wb("daylight", 1, 2),
            grain: grain(Strength::Weak, GrainSize::Large),
            fade: 20.0,
            split_shadows: [-4.0, 8.0, 12.0],
            split_highlights: [8.0, 4.0, -6.0],
            vignette: 12.0,
            dust: 25.0,
            ..base(
                "Expired film",
                "classic-negative",
                &["film", "expired"],
                "Stock kept too long: colour casts drifting green-cyan, lost density, mottled grain.",
            )
        },
        Recipe {
            negative: true,
            grain: grain(Strength::Weak, GrainSize::Small),
            frame: Frame::Film,
            ..base(
                "Colour negative strip",
                "provia",
                &["film", "negative", "frame"],
                "The picture as it sits on the developed roll: inverted, under an orange mask, with sprocket holes.",
            )
        },
        Recipe {
            highlight: 2.0,
            shadow: 1.0,
            color: 1.0,
            sharpness: 3.0,
            white_balance: wb("daylight", 0, 3),
            noise_reduction: -2.0,
            split_shadows: [0.0, 0.0, 10.0],
            split_highlights: [4.0, 8.0, 12.0],
            dust: 0.0,
            date_stamp: true,
            date: "2004 8 21".into(),
            ..base(
                "Early digicam",
                "astia",
                &["digital", "camera", "date"],
                "A 3-megapixel CCD compact: cool blue-white highlights, crunchy sharpening, the date in the corner.",
            )
        },
        Recipe {
            highlight: -1.0,
            shadow: 1.0,
            color: 1.0,
            white_balance: wb("daylight", 2, -1),
            grain: grain(Strength::Weak, GrainSize::Small),
            vignette: 28.0,
            split_shadows: [2.0, 4.0, 8.0],
            split_highlights: [10.0, 6.0, -6.0],
            date_stamp: true,
            ..base(
                "Point and shoot '98",
                "classic-chrome",
                &["film", "camera", "date"],
                "A compact with a decent lens on drugstore film: today's date burned in like it always was.",
            )
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_validate_and_compile_with_effects() {
        let all = presets();
        assert!(all.len() >= 14);
        let mut names: Vec<&str> = all.iter().map(|r| r.name.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), all.len(), "unique names");
        for r in &all {
            r.validate().unwrap_or_else(|e| panic!("{}: {e}", r.name));
            let (_, small) = r.compile(None);
            let (_, sized) = r.compile_for(None, 320, 200);
            assert!(!small.is_empty(), "{} compiles to stages", r.name);
            if r.has_effects() {
                assert!(sized.len() > small.len(), "{} adds pixel layers", r.name);
            } else {
                assert_eq!(sized.len(), small.len());
            }
        }
        let polaroid = all.iter().find(|r| r.name == "Polaroid SX-70").unwrap();
        let (_, kids) = polaroid.compile_for(None, 320, 200);
        assert!(kids.iter().any(|n| n.name.ends_with("Frame")));
        let disposable = all.iter().find(|r| r.name == "Disposable camera").unwrap();
        let (_, kids) = disposable.compile_for(None, 320, 200);
        assert!(
            kids.iter().any(|n| n.name.ends_with("Date"))
                && kids.iter().any(|n| n.name.ends_with("Light leak"))
        );
    }
}
