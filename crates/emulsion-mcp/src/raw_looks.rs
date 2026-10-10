//! Mood-driven RAW grading: measure the developed photo, then apply an
//! adaptive look from a library modelled on the preset styles photographers
//! buy today. Every look is ordinary, editable RAW development settings, so
//! the result stays fully tweakable with `develop_raw` and undoes in one step.
use crate::{
    exec::Planned,
    server::{ToolDef, ToolResult},
};
use emulsion_core::{
    Document, NodeKind,
    raw::{DevelopParams, PointCurve},
};
use emulsion_io::{develop_presets, film_library, photo_develop::PhotoSource};
use serde::Serialize;
use serde_json::{Value, json};

const BANDS: [&str; 8] = [
    "red", "orange", "yellow", "green", "aqua", "blue", "purple", "magenta",
];
const CENTERS: [f32; 8] = [0., 30., 60., 120., 180., 240., 270., 300.];
const R: usize = 0;
const O: usize = 1;
const Y: usize = 2;
const G: usize = 3;
const A: usize = 4;
const B: usize = 5;
const P: usize = 6;
const M: usize = 7;

fn error(message: impl ToString) -> ToolResult {
    ToolResult::error(message.to_string())
}

// ---------------------------------------------------------------- analysis

#[derive(Clone, Debug, Default, Serialize)]
pub struct Tone {
    /// Display-encoded luminance percentiles 1/5/50/95/99.
    pub percentiles: [f32; 5],
    pub key: &'static str,
    /// p95 − p5; below ~0.45 reads flat, above ~0.85 contrasty.
    pub spread: f32,
    pub clipped_highlights_pct: f32,
    pub crushed_shadows_pct: f32,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Cast {
    /// Positive means the near-neutrals read warm, negative cool.
    pub warmth: f32,
    /// Positive means magenta, negative green.
    pub magenta: f32,
    pub neutral_pct: f32,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Analysis {
    pub tone: Tone,
    pub mean_saturation: f32,
    pub cast: Cast,
    /// Share of colourful pixels per HSL mixer band, percent.
    pub hue_share: std::collections::BTreeMap<&'static str, f32>,
    pub skin_pct: f32,
    /// Mean HSV hue/saturation/value of detected skin; flattering skin sits near 18–28°.
    pub skin: Option<[f32; 3]>,
    /// Hue (degrees) carrying the most colour, the anchor for colour harmonies.
    pub dominant_hue: Option<f32>,
    pub sky_pct: f32,
    pub foliage_pct: f32,
    pub scene: Vec<&'static str>,
}

impl Analysis {
    pub fn has(&self, tag: &str) -> bool {
        self.scene.contains(&tag)
    }
}

fn encode(v: f32) -> f32 {
    let v = v.clamp(0., 1.);
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1. / 2.4) - 0.055
    }
}

fn hsv([r, g, b]: [f32; 3]) -> [f32; 3] {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d < 1e-6 {
        0.
    } else if max == r {
        60. * ((g - b) / d).rem_euclid(6.)
    } else if max == g {
        60. * ((b - r) / d + 2.)
    } else {
        60. * ((r - g) / d + 4.)
    };
    [h, if max > 1e-6 { d / max } else { 0. }, max]
}

fn percentile(sorted: &[f32], p: f32) -> f32 {
    sorted[((sorted.len() - 1) as f32 * p).round() as usize]
}

/// Measure a grid of linear-light premultiplied pixels laid out row-major.
pub fn analyze_pixels(width: u32, height: u32, sample: impl Fn(u32, u32) -> [u16; 4]) -> Analysis {
    let step = (width.max(height) / 256).max(1);
    let (mut lumas, mut sats) = (Vec::new(), Vec::new());
    let (mut bands, mut colourful) = ([0f32; 8], 0f32);
    let (mut neutral, mut neutral_n) = ([0f64; 3], 0usize);
    let (mut skin, mut sky, mut foliage, mut clipped, mut crushed) = (0, 0, 0, 0, 0);
    let mut skin_sum = [0f32; 3];
    let mut band_vectors = [[0f32; 2]; 8];
    for y in (0..height).step_by(step as usize) {
        for x in (0..width).step_by(step as usize) {
            let p = sample(x, y);
            if p[3] == 0 {
                continue;
            }
            let alpha = p[3] as f32 / 65535.;
            let linear = [0, 1, 2].map(|c| (p[c] as f32 / 65535. / alpha).clamp(0., 1.));
            let display = linear.map(encode);
            let luma = display[0] * 0.2126 + display[1] * 0.7152 + display[2] * 0.0722;
            let [h, s, v] = hsv(display);
            lumas.push(luma);
            sats.push(s);
            if display.iter().any(|v| *v >= 0.995) {
                clipped += 1;
            }
            if display.iter().all(|v| *v <= 0.02) {
                crushed += 1;
            }
            if s < 0.18 && (0.15..0.92).contains(&luma) {
                for c in 0..3 {
                    neutral[c] += linear[c].max(1e-4) as f64;
                }
                neutral_n += 1;
            }
            if s >= 0.15 && v > 0.12 {
                let nearest = (0..8)
                    .min_by(|&a, &b| {
                        let d = |i: usize| ((h - CENTERS[i] + 180.).rem_euclid(360.) - 180.).abs();
                        d(a).total_cmp(&d(b))
                    })
                    .unwrap();
                bands[nearest] += s;
                let radians = h.to_radians();
                band_vectors[nearest][0] += s * radians.cos();
                band_vectors[nearest][1] += s * radians.sin();
                colourful += s;
            }
            // Skin across complexions: orange hues, moderate saturation, not too dark.
            if (5.0..45.).contains(&h) && (0.15..0.65).contains(&s) && (0.25..0.97).contains(&v) {
                skin += 1;
                skin_sum[0] += h;
                skin_sum[1] += s;
                skin_sum[2] += v;
            }
            if (180.0..250.).contains(&h) && s > 0.12 && luma > 0.35 && y < height / 2 {
                sky += 1;
            }
            if (65.0..160.).contains(&h) && s > 0.18 && v > 0.1 {
                foliage += 1;
            }
        }
    }
    let n = lumas.len().max(1) as f32;
    if lumas.is_empty() {
        return Analysis::default();
    }
    let pct = |count: usize| (count as f32 / n * 1000.).round() / 10.;
    let round = |v: f32| (v * 1000.).round() / 1000.;
    lumas.sort_by(f32::total_cmp);
    let percentiles = [0.01, 0.05, 0.5, 0.95, 0.99].map(|p| round(percentile(&lumas, p)));
    let spread = round(percentiles[3] - percentiles[1]);
    let median = percentiles[2];
    let mean_saturation = round(sats.iter().sum::<f32>() / n);
    let cast = if neutral_n * 20 >= lumas.len() {
        let [r, g, b] = neutral.map(|v| (v / neutral_n as f64) as f32);
        Cast {
            warmth: round((r / b).log2() / 1.4),
            magenta: round(((r * b).sqrt() / g).log2() / 0.4),
            neutral_pct: pct(neutral_n),
        }
    } else {
        Cast {
            neutral_pct: pct(neutral_n),
            ..Cast::default()
        }
    };
    let hue_share = BANDS
        .iter()
        .zip(bands)
        .map(|(name, v)| (*name, (v / colourful.max(1e-6) * 1000.).round() / 10.))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut a = Analysis {
        tone: Tone {
            percentiles,
            key: if median < 0.3 {
                "low"
            } else if median > 0.62 {
                "high"
            } else {
                "normal"
            },
            spread,
            clipped_highlights_pct: pct(clipped),
            crushed_shadows_pct: pct(crushed),
        },
        mean_saturation,
        cast,
        skin_pct: pct(skin),
        skin: (skin > 0).then(|| skin_sum.map(|v| round(v / skin as f32))),
        dominant_hue: (colourful > 0.).then(|| {
            let band = (0..8)
                .max_by(|&a, &b| bands[a].total_cmp(&bands[b]))
                .unwrap();
            let [x, y] = band_vectors[band];
            y.atan2(x).to_degrees().rem_euclid(360.).round()
        }),
        sky_pct: pct(sky),
        foliage_pct: pct(foliage),
        hue_share,
        scene: Vec::new(),
    };
    let warm_share = a.hue_share["red"] + a.hue_share["orange"] + a.hue_share["yellow"];
    let night = median < 0.2 && percentiles[4] > 0.7;
    for (tag, on) in [
        ("portrait", a.skin_pct >= 6.),
        (
            "landscape",
            a.sky_pct + a.foliage_pct >= 25. && a.skin_pct < 6.,
        ),
        ("night", night),
        ("low_key", median < 0.3),
        ("high_key", median > 0.62),
        (
            "golden_light",
            !night && a.cast.warmth > 0.12 && warm_share > 55.,
        ),
        ("flat", spread < 0.45),
        (
            "contrasty",
            spread > 0.85 || a.tone.clipped_highlights_pct > 1.,
        ),
        ("monochrome", mean_saturation < 0.04),
        ("muted", (0.04..0.12).contains(&mean_saturation)),
        ("colorful", mean_saturation > 0.35),
    ] {
        if on {
            a.scene.push(tag);
        }
    }
    a
}

/// What the palette says emotionally, from common colour psychology: warm
/// hues read as energy and warmth, cool hues as calm, low saturation as quiet.
pub fn palette_mood(a: &Analysis) -> Vec<&'static str> {
    let mut mood = Vec::new();
    let share = |bands: &[&str]| bands.iter().map(|b| a.hue_share[b]).sum::<f32>();
    if a.hue_share.is_empty() || a.has("monochrome") {
        return vec!["timeless", "graphic"];
    }
    let warm = share(&["red", "orange", "yellow"]);
    let cool = share(&["green", "aqua", "blue", "purple"]);
    if warm > cool * 1.5 {
        mood.extend(["warm", "energetic", "inviting"]);
    } else if cool > warm * 1.5 {
        mood.extend(["cool", "calm", "serene"]);
    } else {
        mood.push("balanced warm/cool");
    }
    if let Some(hue) = a.dominant_hue {
        mood.push(match hue {
            h if !(15.0..345.).contains(&h) => "red: passion, power",
            h if h < 45. => "orange: warmth, comfort",
            h if h < 75. => "yellow: joy, optimism",
            h if h < 165. => "green: nature, freshness",
            h if h < 200. => "aqua: clarity, coolness",
            h if h < 255. => "blue: calm, trust, melancholy",
            h if h < 290. => "purple: mystery, romance",
            _ => "magenta: playfulness, romance",
        });
    }
    if a.has("muted") {
        mood.push("quiet, understated");
    } else if a.has("colorful") {
        mood.push("lively, bold");
    }
    if a.has("low_key") {
        mood.push("dramatic, intimate");
    } else if a.has("high_key") {
        mood.push("light, optimistic");
    }
    mood
}

pub fn analyze_document(doc: &Document) -> Result<Analysis, ToolResult> {
    let raw = doc
        .raw
        .as_ref()
        .ok_or_else(|| error("No editable RAW source; open a supported camera RAW file first"))?;
    let node = doc
        .node(raw.node_id)
        .ok_or_else(|| error("RAW node is missing"))?;
    let NodeKind::Raster { raster, .. } = &node.kind else {
        return Err(error("RAW node has no developed pixels"));
    };
    Ok(analyze_pixels(raster.width(), raster.height(), |x, y| {
        raster.get(x, y)
    }))
}

// ---------------------------------------------------------------- looks

pub struct Look {
    pub key: &'static str,
    pub label: &'static str,
    /// Words people use for the feeling; matched against free-text requests.
    pub moods: &'static [&'static str],
    pub description: &'static str,
    /// The preset genre on the market this look is modelled on.
    pub comparable_to: &'static str,
    /// Scene tags from analysis that suit or fight the look.
    pub suits: &'static [&'static str],
    pub avoid: &'static [&'static str],
    /// Neutralise a measured colour cast before grading.
    pub neutralize_cast: bool,
    /// Colour harmony used when the request leaves it to the look.
    pub harmony: Harmony,
    pub build: fn(&mut DevelopParams),
}

fn curve(p: &mut DevelopParams, channel: usize, points: &[[f32; 2]]) {
    p.point_curves[channel] = PointCurve::try_from(points.to_vec()).expect("static look curve");
}

pub const LOOKS: &[Look] = &[
    Look {
        key: "natural-pro",
        label: "Clean Natural",
        moods: &[
            "natural",
            "clean",
            "true",
            "realistic",
            "balanced",
            "commercial",
            "simple",
            "auto",
            "best",
        ],
        description: "True-to-life colour with gentle pop: modest contrast, clean whites, lively but believable colour.",
        comparable_to: "clean editorial/commercial base presets",
        suits: &["flat", "muted"],
        avoid: &[],
        neutralize_cast: true,
        harmony: Harmony::None,
        build: |p| {
            p.contrast = 0.08;
            p.whites = 0.06;
            p.blacks = -0.06;
            p.vibrance = 0.14;
            p.clarity = 0.06;
            p.texture = 0.06;
            curve(p, 0, &[[0., 0.], [0.25, 0.235], [0.75, 0.765], [1., 1.]]);
        },
    },
    Look {
        key: "portrait-film-warm",
        label: "Warm Portrait Film",
        moods: &[
            "passion", "intimate", "love", "warm", "film", "portrait", "wedding", "soft",
            "romantic", "pastel", "analog", "skin", "people",
        ],
        description: "Soft negative-film rendering: lifted blacks, creamy skin, olive greens, gently warm highlights.",
        comparable_to: "warm portrait negative-film emulation presets (Portra-style)",
        suits: &["portrait", "golden_light", "high_key"],
        avoid: &["night", "monochrome"],
        neutralize_cast: true,
        harmony: Harmony::Complementary,
        build: |p| {
            p.contrast = -0.08;
            p.saturation = -0.08;
            p.vibrance = 0.1;
            p.clarity = -0.06;
            curve(
                p,
                0,
                &[[0., 0.045], [0.25, 0.245], [0.75, 0.775], [1., 0.97]],
            );
            p.hsl[R] = [0.03, -0.1, 0.];
            p.hsl[O] = [0.03, -0.05, 0.1];
            p.hsl[Y] = [-0.1, -0.15, 0.05];
            p.hsl[G] = [-0.3, -0.35, -0.05];
            p.hsl[A] = [0., -0.2, 0.];
            p.hsl[B] = [-0.1, -0.15, 0.05];
            p.grading = [[200., 0.12, 0.], [35., 0.05, 0.], [45., 0.18, 0.]];
            p.calibration[0] = [0.05, 0.1];
            p.calibration[2] = [-0.05, 0.1];
        },
    },
    Look {
        key: "clean-portrait",
        label: "Clean Skin Portrait",
        moods: &[
            "portrait",
            "skin",
            "flattering",
            "beauty",
            "studio",
            "headshot",
            "fresh",
            "glow",
        ],
        description: "Flattering, luminous skin with true colour; softened micro-contrast and a touch of warmth.",
        comparable_to: "professional headshot and beauty retouch presets",
        suits: &["portrait"],
        avoid: &["landscape", "night", "monochrome"],
        neutralize_cast: true,
        harmony: Harmony::None,
        build: |p| {
            p.contrast = 0.05;
            p.whites = 0.05;
            p.vibrance = 0.1;
            p.clarity = -0.12;
            p.texture = -0.12;
            p.hsl[R] = [0.02, -0.06, 0.05];
            p.hsl[O] = [0.02, -0.06, 0.12];
            p.hsl[Y] = [-0.05, -0.1, 0.05];
            p.grading = [[0., 0., 0.], [0., 0., 0.], [40., 0.08, 0.]];
            curve(p, 0, &[[0., 0.015], [0.25, 0.24], [0.75, 0.77], [1., 1.]]);
        },
    },
    Look {
        key: "bright-airy",
        label: "Bright & Airy",
        moods: &[
            "bright",
            "airy",
            "light",
            "dreamy",
            "pastel",
            "soft",
            "wedding",
            "lifestyle",
            "happy",
            "fresh",
            "minimal",
        ],
        description: "Luminous and soft: open shadows, creamy highlights, muted greens and powder blues.",
        comparable_to: "light-and-airy wedding/lifestyle presets",
        suits: &["high_key", "portrait", "flat"],
        avoid: &["night", "low_key", "contrasty"],
        neutralize_cast: true,
        harmony: Harmony::Analogous,
        build: |p| {
            p.contrast = -0.15;
            p.whites = 0.15;
            p.blacks = 0.15;
            p.parametric = [0.12, 0.08, 0.04, 0.];
            p.saturation = -0.06;
            p.vibrance = 0.1;
            p.clarity = -0.1;
            curve(p, 0, &[[0., 0.06], [0.25, 0.28], [0.75, 0.8], [1., 0.98]]);
            p.hsl[O] = [0.02, -0.05, 0.12];
            p.hsl[Y] = [-0.1, -0.2, 0.1];
            p.hsl[G] = [-0.25, -0.35, 0.1];
            p.hsl[A] = [0., -0.25, 0.1];
            p.hsl[B] = [-0.05, -0.15, 0.12];
            p.grading = [[0., 0., 0.], [0., 0., 0.], [50., 0.08, 0.]];
        },
    },
    Look {
        key: "moody-dark",
        label: "Dark & Moody",
        moods: &[
            "mysterious",
            "mystery",
            "sad",
            "moody",
            "dark",
            "dramatic",
            "matte",
            "earthy",
            "emotional",
            "brooding",
            "autumn",
            "forest",
            "rain",
            "melancholy",
        ],
        description: "Deep, earthy and matte: faded blacks, rolled-off highlights, desaturated greens and blues, cool shadows.",
        comparable_to: "dark-and-moody earthy presets",
        suits: &["landscape", "low_key", "flat", "muted"],
        avoid: &["portrait", "high_key"],
        neutralize_cast: true,
        harmony: Harmony::Complementary,
        build: |p| {
            p.contrast = 0.12;
            p.whites = -0.12;
            p.blacks = -0.1;
            p.parametric = [0., -0.06, -0.08, -0.15];
            p.saturation = -0.15;
            p.vibrance = 0.05;
            p.clarity = 0.1;
            p.dehaze = 0.05;
            p.vignette = -0.15;
            curve(p, 0, &[[0., 0.07], [0.25, 0.2], [0.6, 0.58], [1., 0.92]]);
            p.hsl[O] = [0., 0.05, -0.05];
            p.hsl[Y] = [-0.15, -0.3, -0.1];
            p.hsl[G] = [-0.3, -0.45, -0.25];
            p.hsl[A] = [0., -0.4, -0.15];
            p.hsl[B] = [0., -0.3, -0.2];
            p.grading = [[210., 0.14, 0.], [30., 0.04, 0.], [40., 0.1, 0.]];
        },
    },
    Look {
        key: "cinematic-teal-orange",
        label: "Cinematic Teal & Orange",
        moods: &[
            "cinematic",
            "movie",
            "film",
            "teal",
            "orange",
            "blockbuster",
            "epic",
            "dramatic",
            "urban",
            "travel",
            "hollywood",
        ],
        description: "Blockbuster colour contrast: teal shadows and skies, warm skin and highlights, rich filmic S-curve.",
        comparable_to: "cinematic teal-and-orange film-grade presets/LUTs",
        suits: &["portrait", "contrasty", "night", "colorful"],
        avoid: &["monochrome"],
        neutralize_cast: true,
        harmony: Harmony::Complementary,
        build: |p| {
            p.contrast = 0.12;
            p.blacks = -0.05;
            p.saturation = -0.05;
            p.vibrance = 0.12;
            p.clarity = 0.1;
            p.dehaze = 0.05;
            p.vignette = -0.12;
            curve(p, 0, &[[0., 0.035], [0.25, 0.215], [0.75, 0.8], [1., 0.98]]);
            p.hsl[R] = [0.08, 0., 0.];
            p.hsl[O] = [0., 0.1, 0.05];
            p.hsl[Y] = [-0.25, -0.2, 0.];
            p.hsl[G] = [0.6, -0.5, -0.1];
            p.hsl[A] = [0.1, 0.1, -0.05];
            p.hsl[B] = [-0.25, 0.1, -0.1];
            p.grading = [[195., 0.3, -0.03], [0., 0., 0.], [38., 0.22, 0.]];
            p.grading_balance = -0.1;
            p.calibration[2] = [-0.1, 0.2];
        },
    },
    Look {
        key: "golden-hour",
        label: "Golden Hour Glow",
        moods: &[
            "joyful",
            "energetic",
            "cheerful",
            "happy",
            "golden",
            "sunset",
            "sunrise",
            "warm",
            "glow",
            "summer",
            "sunny",
            "cozy",
            "nostalgic",
            "travel",
        ],
        description: "Sun-kissed warmth: glowing amber highlights, rich oranges, softened haze; keeps the warmth of the light.",
        comparable_to: "golden-hour / summer glow presets",
        suits: &["golden_light", "portrait", "landscape"],
        avoid: &["night", "monochrome"],
        neutralize_cast: false,
        harmony: Harmony::Analogous,
        build: |p| {
            p.contrast = 0.05;
            p.vibrance = 0.15;
            p.clarity = -0.05;
            p.dehaze = -0.05;
            p.vignette = -0.1;
            curve(
                p,
                0,
                &[[0., 0.025], [0.25, 0.235], [0.75, 0.78], [1., 0.99]],
            );
            p.hsl[R] = [0.03, 0.05, 0.];
            p.hsl[O] = [0., 0.15, 0.05];
            p.hsl[Y] = [-0.12, 0.1, 0.05];
            p.hsl[G] = [-0.2, -0.15, 0.];
            p.hsl[B] = [-0.1, -0.15, 0.];
            p.global_grading = [40., 0.12, 0.];
            p.grading = [[25., 0.08, 0.], [0., 0., 0.], [45., 0.25, 0.03]];
        },
    },
    Look {
        key: "vivid-landscape",
        label: "Vivid Landscape",
        moods: &[
            "vivid",
            "landscape",
            "nature",
            "punchy",
            "colorful",
            "colourful",
            "saturated",
            "bold",
            "travel",
            "crisp",
            "vibrant",
        ],
        description: "Crisp and saturated: deep blue skies, lush greens, strong local contrast and clarity.",
        comparable_to: "vivid landscape/travel presets",
        suits: &["landscape", "flat", "muted"],
        avoid: &["portrait", "colorful"],
        neutralize_cast: true,
        harmony: Harmony::None,
        build: |p| {
            p.contrast = 0.15;
            p.whites = 0.1;
            p.blacks = -0.1;
            p.saturation = 0.05;
            p.vibrance = 0.25;
            p.clarity = 0.15;
            p.texture = 0.15;
            p.dehaze = 0.1;
            p.vignette = -0.05;
            curve(p, 0, &[[0., 0.], [0.25, 0.22], [0.75, 0.79], [1., 1.]]);
            p.hsl[Y] = [-0.05, 0.05, 0.];
            p.hsl[G] = [0.1, 0.1, -0.05];
            p.hsl[A] = [0., 0.1, -0.05];
            p.hsl[B] = [-0.05, 0.15, -0.15];
        },
    },
    Look {
        key: "rich-slide-film",
        label: "Rich Slide Film",
        moods: &[
            "film",
            "rich",
            "retro",
            "classic",
            "slide",
            "saturated",
            "kodachrome",
            "vintage",
            "deep",
        ],
        description: "Classic transparency-film richness: deep reds and blues, dense shadows, warm-neutral highlights.",
        comparable_to: "classic slide-film emulation presets (Kodachrome-style)",
        suits: &["landscape", "flat", "muted"],
        avoid: &["monochrome", "colorful"],
        neutralize_cast: true,
        harmony: Harmony::None,
        build: |p| {
            p.contrast = 0.18;
            p.blacks = -0.1;
            p.saturation = 0.08;
            p.vibrance = 0.05;
            curve(p, 0, &[[0., 0.], [0.25, 0.21], [0.75, 0.8], [1., 0.99]]);
            p.hsl[R] = [0.02, 0.15, -0.1];
            p.hsl[Y] = [-0.1, 0.1, 0.];
            p.hsl[G] = [-0.1, -0.1, -0.1];
            p.hsl[B] = [-0.1, 0.1, -0.2];
            p.grading = [[220., 0.05, 0.], [0., 0., 0.], [50., 0.08, 0.]];
        },
    },
    Look {
        key: "vintage-faded",
        label: "Vintage Faded Film",
        moods: &[
            "sepia",
            "vintage",
            "retro",
            "faded",
            "nostalgic",
            "70s",
            "analog",
            "matte",
            "old",
            "instant",
            "memory",
        ],
        description: "Nostalgic print: milky blacks, creamy yellow highlights, cool shadows and softened colour.",
        comparable_to: "vintage/faded film and instant-print presets",
        suits: &["golden_light", "high_key", "colorful"],
        avoid: &["monochrome"],
        neutralize_cast: true,
        harmony: Harmony::Complementary,
        build: |p| {
            p.contrast = -0.05;
            p.saturation = -0.15;
            p.clarity = -0.1;
            p.vignette = -0.15;
            curve(p, 0, &[[0., 0.1], [0.3, 0.3], [0.75, 0.74], [1., 0.9]]);
            curve(p, 3, &[[0., 0.06], [0.5, 0.49], [1., 0.9]]);
            p.hsl[O] = [0., 0.05, 0.];
            p.hsl[G] = [-0.3, -0.3, 0.];
            p.hsl[B] = [-0.15, -0.25, 0.];
            p.grading = [[220., 0.1, 0.], [0., 0., 0.], [50., 0.15, 0.]];
        },
    },
    Look {
        key: "nordic-cool",
        label: "Nordic Cool Matte",
        moods: &[
            "peaceful",
            "tranquil",
            "modern",
            "cool",
            "cold",
            "nordic",
            "scandinavian",
            "minimal",
            "winter",
            "calm",
            "clean",
            "blue",
            "serene",
            "icy",
        ],
        description: "Quiet and cool: restrained colour, cool shadows, muted yellows and greens, soft matte finish.",
        comparable_to: "Scandinavian/minimal cool-tone presets",
        suits: &["landscape", "high_key", "flat"],
        avoid: &["golden_light", "portrait"],
        neutralize_cast: true,
        harmony: Harmony::Analogous,
        build: |p| {
            p.contrast = -0.05;
            p.saturation = -0.2;
            p.vibrance = 0.05;
            p.clarity = 0.05;
            curve(p, 0, &[[0., 0.04], [0.25, 0.25], [0.75, 0.76], [1., 0.98]]);
            p.hsl[O] = [0., -0.1, 0.];
            p.hsl[Y] = [0., -0.4, 0.];
            p.hsl[G] = [0.3, -0.4, 0.];
            p.hsl[B] = [0., -0.1, 0.1];
            p.global_grading = [210., 0.06, 0.];
            p.grading = [[210., 0.12, 0.], [0., 0., 0.], [190., 0.04, 0.]];
        },
    },
    Look {
        key: "urban-muted",
        label: "Urban Muted",
        moods: &[
            "urban",
            "street",
            "gritty",
            "muted",
            "editorial",
            "desaturated",
            "documentary",
            "city",
            "raw",
            "edgy",
            "fashion",
        ],
        description: "Gritty editorial: pulled-back colour, strong texture and clarity, faded blacks and a cool cast in shadows.",
        comparable_to: "street/urban desaturated editorial presets",
        suits: &["contrasty", "colorful", "muted"],
        avoid: &["monochrome", "high_key"],
        neutralize_cast: true,
        harmony: Harmony::None,
        build: |p| {
            p.contrast = 0.2;
            p.saturation = -0.3;
            p.vibrance = 0.05;
            p.clarity = 0.22;
            p.texture = 0.15;
            p.dehaze = 0.08;
            p.vignette = -0.15;
            curve(p, 0, &[[0., 0.05], [0.25, 0.22], [0.75, 0.78], [1., 0.97]]);
            p.hsl[O] = [0., 0.1, 0.];
            p.grading = [[200., 0.08, 0.], [0., 0., 0.], [40., 0.05, 0.]];
        },
    },
    Look {
        key: "night-neon",
        label: "Night City Neon",
        moods: &[
            "mysterious",
            "night",
            "neon",
            "city",
            "cyberpunk",
            "vibrant",
            "nightlife",
            "electric",
            "lights",
            "blue",
            "purple",
        ],
        description: "Electric night: inky blue shadows, saturated neon purples, magentas and cyans, controlled sodium yellows.",
        comparable_to: "neon/cyberpunk night city presets",
        suits: &["night", "low_key", "colorful"],
        avoid: &["high_key", "portrait"],
        neutralize_cast: false,
        harmony: Harmony::SplitComplementary,
        build: |p| {
            p.contrast = 0.15;
            p.blacks = -0.1;
            p.saturation = 0.05;
            p.vibrance = 0.2;
            p.clarity = 0.1;
            p.dehaze = 0.1;
            p.vignette = -0.15;
            p.hsl[O] = [-0.1, 0.05, 0.];
            p.hsl[Y] = [-0.2, -0.15, 0.];
            p.hsl[G] = [0.5, -0.4, 0.];
            p.hsl[A] = [0.1, 0.2, 0.];
            p.hsl[B] = [0., 0.15, 0.];
            p.hsl[P] = [-0.1, 0.2, 0.];
            p.hsl[M] = [0., 0.2, 0.];
            p.grading = [[230., 0.2, 0.], [0., 0., 0.], [320., 0.08, 0.]];
        },
    },
    Look {
        key: "classic-bw",
        label: "Classic Black & White",
        moods: &[
            "black",
            "white",
            "bw",
            "b&w",
            "monochrome",
            "mono",
            "grayscale",
            "greyscale",
            "timeless",
            "classic",
            "documentary",
        ],
        description: "Timeless monochrome with a full tonal range, rich blacks and crisp mid-tone separation.",
        comparable_to: "classic B&W darkroom presets",
        suits: &["monochrome", "contrasty", "muted"],
        avoid: &["colorful"],
        neutralize_cast: false,
        harmony: Harmony::None,
        build: |p| {
            p.saturation = -1.;
            p.contrast = 0.15;
            p.whites = 0.08;
            p.blacks = -0.08;
            p.clarity = 0.12;
            p.vignette = -0.08;
            curve(p, 0, &[[0., 0.], [0.25, 0.22], [0.75, 0.79], [1., 1.]]);
        },
    },
    Look {
        key: "noir-bw",
        label: "Noir High-Contrast B&W",
        moods: &[
            "noir",
            "black",
            "white",
            "bw",
            "dramatic",
            "gritty",
            "hard",
            "mono",
            "monochrome",
            "street",
            "moody",
        ],
        description: "Hard-hitting monochrome: deep blacks, bright whites, strong clarity and a heavy vignette.",
        comparable_to: "high-contrast noir/street B&W presets",
        suits: &["contrasty", "low_key", "night", "monochrome"],
        avoid: &["high_key", "flat"],
        neutralize_cast: false,
        harmony: Harmony::None,
        build: |p| {
            p.saturation = -1.;
            p.contrast = 0.35;
            p.whites = 0.15;
            p.blacks = -0.2;
            p.clarity = 0.25;
            p.texture = 0.1;
            p.vignette = -0.25;
            curve(p, 0, &[[0., 0.], [0.25, 0.17], [0.75, 0.84], [1., 1.]]);
        },
    },
];

/// Colour-wheel relationships used by colourists, anchored on the photo's
/// subject colour (skin when people are present, else the dominant hue).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Harmony {
    None,
    Complementary,
    SplitComplementary,
    Analogous,
    Triadic,
    Monochromatic,
}

impl Harmony {
    fn parse(value: &str) -> Option<Option<Self>> {
        Some(match value {
            "auto" => None,
            "none" => Some(Self::None),
            "complementary" => Some(Self::Complementary),
            "split_complementary" => Some(Self::SplitComplementary),
            "analogous" => Some(Self::Analogous),
            "triadic" => Some(Self::Triadic),
            "monochromatic" => Some(Self::Monochromatic),
            _ => return None,
        })
    }
    fn hues(self, anchor: f32) -> Vec<f32> {
        let offsets: &[f32] = match self {
            Self::None => &[],
            // 180° reads as a video-game filter; 160–175° reads cinematic.
            Self::Complementary => &[0., 170.],
            Self::SplitComplementary => &[0., 150., 210.],
            Self::Analogous => &[-30., 0., 30.],
            Self::Triadic => &[0., 120., 240.],
            Self::Monochromatic => &[0.],
        };
        offsets
            .iter()
            .map(|o| (anchor + o).rem_euclid(360.))
            .collect()
    }
}

fn hue_delta(from: f32, to: f32) -> f32 {
    (to - from + 180.).rem_euclid(360.) - 180.
}

/// Warmth of a hue on the colour wheel: 1 at orange, -1 at blue.
fn warmth(hue: f32) -> f32 {
    (hue - 30.).to_radians().cos()
}

/// Pull the palette into a colour harmony: colours on the scheme gain richness,
/// colours off it are nudged toward the nearest scheme hue and muted (about
/// -40, as colourists do), and the split-tone wheels carry the scheme with
/// warm hues in highlights and cool hues in shadows, kept conservative.
fn apply_harmony(p: &mut DevelopParams, harmony: Harmony, anchor: f32) -> Option<String> {
    let hues = harmony.hues(anchor);
    if hues.is_empty() || p.saturation <= -1. {
        return None;
    }
    for (band, center) in CENTERS.iter().enumerate() {
        let distance = hues
            .iter()
            .map(|h| hue_delta(*center, *h))
            .min_by(|a, b| a.abs().total_cmp(&b.abs()))
            .unwrap();
        let hsl = &mut p.hsl[band];
        if distance.abs() <= 20. {
            hsl[1] = (hsl[1] + 0.1).min(1.);
        } else {
            hsl[0] = (hsl[0] + distance.clamp(-25., 25.) / 45.).clamp(-1., 1.);
            let mute = if harmony == Harmony::Monochromatic {
                0.6
            } else {
                ((distance.abs() - 20.) / 100.).min(0.4)
            };
            hsl[1] = (hsl[1] - mute).max(-1.);
        }
    }
    let mut by_warmth = hues.clone();
    by_warmth.sort_by(|a, b| warmth(*b).total_cmp(&warmth(*a)));
    let (warm, cool) = (by_warmth[0], *by_warmth.last().unwrap());
    match harmony {
        Harmony::Monochromatic => {
            p.grading = [[0.; 3]; 3];
            p.global_grading = [anchor, 0.15, p.global_grading[2]];
            p.saturation = (p.saturation - 0.15).max(-0.9);
        }
        _ => {
            p.grading[2] = [warm, 0.12, p.grading[2][2]];
            p.grading[0] = [cool, 0.1, p.grading[0][2]];
            p.grading[1] = match by_warmth.get(1).filter(|_| hues.len() == 3) {
                Some(mid) => [*mid, 0.04, p.grading[1][2]],
                None => [0., 0., p.grading[1][2]],
            };
            p.global_grading[1] = 0.;
        }
    }
    let names: Vec<_> = hues.iter().map(|h| format!("{h:.0}°")).collect();
    Some(format!(
        "{harmony:?} harmony on {}: on-palette colours enriched, off-palette colours shifted toward it and muted; highlights toned {warm:.0}°, shadows {cool:.0}°",
        names.join("/")
    ))
}

/// Graduated sky filter a landscape photographer would add: darker, richer sky.
/// Its exact shape marks it as the look's, so re-applying replaces it.
const SKY_GRAD_CENTER: [f32; 2] = [0.5, 0.35];
const SKY_GRAD_RADIUS: [f32; 2] = [1., 0.3];

fn is_sky_grad(m: &emulsion_core::raw::LocalAdjustment) -> bool {
    m.linear && !m.inverted && m.center == SKY_GRAD_CENTER && m.radius == SKY_GRAD_RADIUS
}

fn add_sky_grad(p: &mut DevelopParams, mono: bool) -> bool {
    let Some(slot) = p
        .masks
        .iter_mut()
        .find(|m| !m.enabled && m.bitmap.is_none())
    else {
        return false;
    };
    *slot = emulsion_core::raw::LocalAdjustment {
        enabled: true,
        bitmap: None,
        linear: true,
        inverted: false,
        center: SKY_GRAD_CENTER,
        radius: SKY_GRAD_RADIUS,
        feather: 1.,
        exposure: -0.4,
        saturation: if mono { 0. } else { 0.1 },
        temperature: 0.,
    };
    true
}

/// Classic B&W contrast filters, applied as primary calibration before the
/// monochrome conversion so neutrals stay put and only coloured areas move.
fn bw_filter(p: &mut DevelopParams, a: &Analysis) -> &'static str {
    // B&W mixer (R, O, Y, G, A, B, P, M), from the style guide's mixer table.
    let (mixer, note) = if a.has("portrait") {
        (
            [0.15, 0.12, 0.05, 0., 0., -0.05, 0., -0.05],
            "B&W mix lightens reds and oranges for luminous skin",
        )
    } else if a.sky_pct >= 5. {
        (
            [0.05, 0.08, 0.12, -0.05, -0.12, -0.2, -0.05, -0.05],
            "B&W mix darkens blues and aquas for a dramatic sky",
        )
    } else if a.foliage_pct >= 10. {
        (
            [0., 0.05, 0.15, -0.1, -0.05, -0.05, 0., 0.],
            "B&W mix brightens yellows and darkens greens for foliage depth",
        )
    } else {
        (
            [0.05, 0.05, 0.1, 0., -0.05, -0.1, 0., -0.05],
            "B&W mix like a yellow filter: natural tonal separation",
        )
    };
    if p.gray_mixer == [0.; 8] {
        p.gray_mixer = mixer;
    }
    note
}

/// Settings a look owns. Applying a look replaces these, so looks never stack;
/// exposure, white balance, geometry, detail and local masks are preserved.
pub fn clear_look(p: &mut DevelopParams) {
    let d = DevelopParams::default();
    p.hsl = d.hsl;
    p.grading = d.grading;
    p.global_grading = d.global_grading;
    p.grading_balance = d.grading_balance;
    p.grading_blending = d.grading_blending;
    p.calibration = d.calibration;
    p.shadow_tint = d.shadow_tint;
    p.point_curves = d.point_curves;
    p.smooth_point_curves = d.smooth_point_curves;
    p.tone_curve = d.tone_curve;
    p.parametric = d.parametric;
    p.parametric_splits = d.parametric_splits;
    p.grain = d.grain;
    p.gray_mixer = d.gray_mixer;
    for mask in &mut p.masks {
        if is_sky_grad(mask) {
            *mask = Default::default();
        }
    }
    for (field, default) in look_scalars(p).into_iter().zip(look_scalar_defaults()) {
        *field = default;
    }
}

fn look_scalars(p: &mut DevelopParams) -> [&mut f32; 9] {
    [
        &mut p.contrast,
        &mut p.whites,
        &mut p.blacks,
        &mut p.saturation,
        &mut p.vibrance,
        &mut p.clarity,
        &mut p.texture,
        &mut p.dehaze,
        &mut p.vignette,
    ]
}
fn look_scalar_defaults() -> [f32; 9] {
    look_scalars(&mut DevelopParams::default()).map(|v| *v)
}

/// Scale a styled look's departure from `base` by `strength` (0 = base).
pub fn blend(base: &DevelopParams, styled: &DevelopParams, strength: f32) -> DevelopParams {
    if strength <= 0. {
        return *base;
    }
    let mix = |a: f32, b: f32| a + (b - a) * strength;
    let mut out = *styled;
    let mut base_copy = *base;
    let mut styled_copy = *styled;
    for ((o, b), s) in look_scalars(&mut out)
        .into_iter()
        .zip(look_scalars(&mut base_copy))
        .zip(look_scalars(&mut styled_copy))
    {
        *o = mix(*b, *s).clamp(-1., 1.);
    }
    for i in 0..8 {
        for c in 0..3 {
            out.hsl[i][c] = mix(base.hsl[i][c], styled.hsl[i][c]).clamp(-1., 1.);
        }
    }
    for i in 0..3 {
        out.grading[i][1] = mix(base.grading[i][1], styled.grading[i][1]).clamp(0., 1.);
        out.grading[i][2] = mix(base.grading[i][2], styled.grading[i][2]).clamp(-1., 1.);
        for c in 0..2 {
            out.calibration[i][c] =
                mix(base.calibration[i][c], styled.calibration[i][c]).clamp(-1., 1.);
        }
    }
    for i in 0..4 {
        out.parametric[i] = mix(base.parametric[i], styled.parametric[i]).clamp(-1., 1.);
    }
    out.global_grading[1] = mix(base.global_grading[1], styled.global_grading[1]).clamp(0., 1.);
    out.global_grading[2] = mix(base.global_grading[2], styled.global_grading[2]).clamp(-1., 1.);
    out.grading_balance = mix(base.grading_balance, styled.grading_balance).clamp(-1., 1.);
    out.shadow_tint = mix(base.shadow_tint, styled.shadow_tint).clamp(-1., 1.);
    out.grain[0] = mix(base.grain[0], styled.grain[0]).clamp(0., 1.);
    for i in 0..8 {
        out.gray_mixer[i] = mix(base.gray_mixer[i], styled.gray_mixer[i]).clamp(-1., 1.);
    }
    out.highlights = mix(base.highlights, styled.highlights).clamp(-1., 1.);
    out.shadows = mix(base.shadows, styled.shadows).clamp(-1., 1.);
    let mut floor = 0f32;
    for i in 0..5 {
        // Keep the five-point curve monotonic when strength extrapolates.
        floor = mix(base.tone_curve[i], styled.tone_curve[i]).clamp(floor, 1.);
        out.tone_curve[i] = floor;
    }
    for (o, b) in out.point_curves.iter_mut().zip(base.point_curves) {
        // Looks start from identity curves, so ease each point toward y = x.
        if b.len == 0 {
            for point in &mut o.points[..o.len as usize] {
                point[1] = mix(point[0], point[1]).clamp(0., 1.);
            }
        }
    }
    out
}

/// Fit the look to the photo, the way a retoucher trims a preset after applying it.
///
/// `measured` is false when an earlier look already shifted the pixels being
/// measured; corrections that steer toward a measured target are then skipped.
fn adapt(p: &mut DevelopParams, a: &Analysis, measured: bool) -> Vec<String> {
    let mut notes: Vec<String> = Vec::new();
    let mono = p.saturation <= -1.;
    if mono {
        notes.push(bw_filter(p, a).into());
    }
    if a.has("portrait") && !mono {
        // Skin is steered in the HSL orange/red bands, which are targeted; the
        // midtone grading wheel would tint every face, so it stays light.
        for band in [R, O] {
            p.hsl[band][0] = p.hsl[band][0].clamp(-0.05, 0.05);
            p.hsl[band][1] = p.hsl[band][1].min(0.05);
            p.hsl[band][2] = p.hsl[band][2].max(0.);
        }
        p.grading[1][1] = p.grading[1][1].min(0.05);
        p.clarity = p.clarity.min(0.05);
        p.texture = p.texture.min(0.);
        notes.push(
            "protected skin: limited red/orange shifts, light midtone grading, softened clarity"
                .into(),
        );
        if let (true, Some([hue, sat, value])) = (measured, a.skin) {
            let shift = (hue_delta(hue, 23.) / 45.).clamp(-0.12, 0.12);
            if shift.abs() >= 0.02 {
                p.hsl[O][0] = shift;
                p.hsl[R][0] = shift * 0.5;
                notes.push(format!(
                    "skin hue {hue:.0}° moved toward the flattering 18–28° range ({} cast removed)",
                    if shift > 0. {
                        "red/magenta"
                    } else {
                        "yellow/green"
                    }
                ));
            }
            if sat > 0.48 {
                p.hsl[O][1] = (p.hsl[O][1] - ((sat - 0.42) * 1.2).min(0.25)).max(-1.);
                notes.push("calmed oversaturated skin in the orange band".into());
            } else if sat < 0.2 {
                p.hsl[O][1] = (p.hsl[O][1] + 0.08).min(1.);
                notes.push("added life to pale skin in the orange band".into());
            }
            if value < 0.5 {
                p.hsl[O][2] = (p.hsl[O][2] + 0.12).min(0.25);
                notes.push("brightened skin with orange luminance".into());
            }
        }
    }
    if a.has("landscape") && !mono {
        if a.sky_pct >= 5. {
            p.hsl[B][2] = (p.hsl[B][2] - 0.12).max(-1.);
            p.hsl[B][1] = (p.hsl[B][1] + 0.06).min(1.);
            p.hsl[A][0] = (p.hsl[A][0] - 0.1).max(-1.);
            notes.push("deepened sky: darker, richer blues with aqua pulled toward blue".into());
        }
        if a.foliage_pct >= 10. {
            // Digital greens read neon: green saturation -10..-20 per the style guide.
            p.hsl[G][1] = (p.hsl[G][1] - 0.12).max(-1.);
            p.hsl[Y][1] = (p.hsl[Y][1] - 0.05).max(-1.);
            p.hsl[G][2] = (p.hsl[G][2] - 0.05).max(-1.);
            notes.push("natural foliage: tamed neon greens and yellow-greens".into());
        }
    }
    if a.has("contrasty") {
        for v in [
            &mut p.contrast,
            &mut p.whites,
            &mut p.dehaze,
            &mut p.clarity,
        ] {
            if *v > 0. {
                *v *= 0.5;
            }
        }
        notes.push(
            "halved added contrast/whites/dehaze: the scene already has a wide tonal range".into(),
        );
    } else if a.has("flat") {
        p.contrast = (p.contrast + 0.08).min(1.);
        notes.push("added contrast for a flat scene".into());
    }
    if a.has("colorful") && !mono {
        for v in [&mut p.saturation, &mut p.vibrance] {
            if *v > 0. {
                *v *= 0.5;
            }
        }
        notes.push("halved added saturation/vibrance: colours are already strong".into());
    } else if a.has("muted") && !mono {
        p.vibrance = (p.vibrance + 0.06).min(1.);
        notes.push("added vibrance for muted colour".into());
    }
    if a.has("night") {
        for v in [&mut p.dehaze, &mut p.clarity, &mut p.texture] {
            if *v > 0. {
                *v *= 0.5;
            }
        }
        notes.push("reduced dehaze/clarity/texture to avoid amplifying low-light noise".into());
    }
    notes
}

/// Hard limits from the film-preset style guide (peva3/Lightroom-Presets
/// STYLEGUIDE.md), applied to every look so no edit falls into the known traps.
fn style_guide(p: &mut DevelopParams, a: &Analysis) -> Vec<String> {
    let mut notes = Vec::new();
    let mono = p.saturation <= -1.;
    if p.grain[0] > 0. {
        let mut changed = p.sharpening > 10. / 150.;
        p.sharpening = p.sharpening.min(10. / 150.);
        for v in [&mut p.clarity, &mut p.texture, &mut p.dehaze] {
            if *v > 0. {
                *v = 0.;
                changed = true;
            }
        }
        if changed {
            notes.push("grain: sharpening held at 10 and no positive clarity/texture/dehaze, so grain stays organic".into());
        }
    }
    if p.clarity > 0. && p.texture > 0. && p.dehaze > 0. {
        let largest = p.clarity.max(p.texture).max(p.dehaze);
        for v in [&mut p.clarity, &mut p.texture, &mut p.dehaze] {
            if *v < largest {
                *v *= 0.5;
            }
        }
        notes.push(
            "one local-contrast band leads: eased the other two of clarity/texture/dehaze".into(),
        );
    }
    let capped = [
        (&mut p.clarity, 0.3),
        (&mut p.texture, 0.4),
        (&mut p.dehaze, 0.3),
    ]
    .into_iter()
    .fold(false, |hit, (v, cap)| {
        let was = *v;
        *v = v.clamp(-cap, cap);
        hit || *v != was
    });
    if capped {
        notes.push("held clarity/texture/dehaze within safe limits".into());
    }
    let lifted =
        p.tone_curve[0] > 0.03 || (p.point_curves[0].len > 0 && p.point_curve_output(0, 0.) > 0.03);
    if lifted && p.blacks > 0. {
        p.blacks = 0.;
        notes.push("faded curve: blacks anchored at 0 to avoid a double-faded grey wash".into());
    }
    if !mono && (p.vibrance - p.saturation).abs() > 0.1 {
        p.vibrance = p.saturation + (p.vibrance - p.saturation).clamp(-0.1, 0.1);
        notes.push("kept vibrance within 10 of saturation to avoid a selective-colour look".into());
    }
    for band in &mut p.hsl {
        band[1] = band[1].clamp(-0.6, 0.6);
    }
    for wheel in &mut p.grading {
        wheel[1] = wheel[1].min(0.3);
    }
    p.global_grading[1] = p.global_grading[1].min(0.3);
    if a.has("portrait") {
        p.grading[1][1] = p.grading[1][1].min(0.1);
    }
    if p.shadows > 0. && p.grading[0][2] < 0. {
        p.grading[0][2] = 0.;
        notes.push("shadow wheel no longer darkens shadows that are being opened".into());
    }
    notes
}

/// Classic film stocks from the bundled library that suit the photo.
pub fn film_recommendations(a: &Analysis) -> Vec<Value> {
    let table: [(&str, &[&str], &str); 9] = [
        (
            "portrait",
            &["Kodak Portra 400", "Fujifilm Pro 400H", "Kodak Portra 160"],
            "flattering skin and soft contrast",
        ),
        (
            "golden_light",
            &["Kodak Gold 200", "Kodak Portra 800"],
            "warm consumer and portrait stocks for golden light",
        ),
        (
            "landscape",
            &["Fuji Velvia 50", "Kodak Ektar 100", "Fuji Provia 100F"],
            "saturated, fine-grain landscape stocks",
        ),
        (
            "night",
            &["Cinestill 800T"],
            "tungsten cinema stock with halation for night",
        ),
        (
            "monochrome",
            &["Kodak Tri-X 400", "Ilford HP5 Plus", "Fujifilm Acros"],
            "classic black-and-white stocks",
        ),
        (
            "high_key",
            &["Fujifilm Pro 400H", "Kodak Portra 160"],
            "pastel, airy stocks for bright scenes",
        ),
        (
            "muted",
            &["Kodak Ultramax 400", "Fujifilm Classic Chrome"],
            "everyday stocks that add life to muted colour",
        ),
        (
            "flat",
            &["Kodak Ektar 100", "Kodak Ektachrome E100"],
            "punchier stocks for flat light",
        ),
        (
            "contrasty",
            &["Kodak Portra 400", "Fujifilm Classic Chrome"],
            "gentle stocks that tame contrast",
        ),
    ];
    let mut out = Vec::new();
    let mut seen = Vec::new();
    for (tag, names, why) in table {
        if !a.has(tag) {
            continue;
        }
        for name in names {
            if let Some(p) = film_library::find(name).filter(|p| !seen.contains(&p.name)) {
                seen.push(p.name);
                out.push(json!({"name": p.name, "category": p.category, "description": p.description, "why": why}));
            }
        }
    }
    if out.is_empty() {
        for name in [
            "Kodak Portra 400",
            "Kodak Gold 200",
            "Fuji Superia X-TRA 400",
        ] {
            if let Some(p) = film_library::find(name) {
                out.push(json!({"name": p.name, "category": p.category, "description": p.description, "why": "versatile all-rounders"}));
            }
        }
    }
    out.truncate(5);
    out
}

/// A look from Emulsion's adaptive set or the bundled film library.
#[derive(Clone, Copy)]
enum Chosen {
    Look(&'static Look),
    Film(&'static film_library::FilmPreset),
}

impl Chosen {
    fn key(&self) -> &'static str {
        match self {
            Self::Look(l) => l.key,
            Self::Film(f) => f.name,
        }
    }
    fn label(&self) -> &'static str {
        match self {
            Self::Look(l) => l.label,
            Self::Film(f) => f.name,
        }
    }
}

#[derive(Serialize)]
pub struct Recommendation {
    pub key: &'static str,
    pub label: &'static str,
    pub score: f32,
    pub reason: String,
}

pub fn recommend(a: &Analysis) -> Vec<Recommendation> {
    let mut out: Vec<_> = LOOKS
        .iter()
        .map(|look| {
            let suits: Vec<_> = look.suits.iter().filter(|t| a.has(t)).copied().collect();
            let fights: Vec<_> = look.avoid.iter().filter(|t| a.has(t)).copied().collect();
            // The neutral look is the safe default when nothing else fits.
            let base = if look.key == "natural-pro" { 0.4 } else { 0.3 };
            let mut score = base + 0.25 * suits.len() as f32 - 0.35 * fights.len() as f32;
            // Only offer monochrome uninvited when the photo is close to it already.
            if look.key.ends_with("-bw") && !a.has("monochrome") {
                score -= 0.3;
            }
            let reason = match (suits.is_empty(), fights.is_empty()) {
                (true, true) => "neutral fit for this photo".to_string(),
                (false, true) => format!("suits {}", suits.join(", ")),
                (true, false) => format!("fights {}", fights.join(", ")),
                (false, false) => {
                    format!("suits {}; fights {}", suits.join(", "), fights.join(", "))
                }
            };
            Recommendation {
                key: look.key,
                label: look.label,
                score: (score * 100.).round() / 100.,
                reason,
            }
        })
        .collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out
}

/// Resolve an exact key/label, or match mood words like "warm moody film".
fn resolve(request: &str, a: &Analysis) -> Result<(Chosen, String), ToolResult> {
    let request = request.trim().to_lowercase();
    let ranked = recommend(a);
    let rank = |key: &str| ranked.iter().find(|r| r.key == key).map_or(0., |r| r.score);
    if request.is_empty() || request == "auto" {
        let best = &ranked[0];
        let look = LOOKS.iter().find(|l| l.key == best.key).unwrap();
        return Ok((
            Chosen::Look(look),
            format!("auto-selected from photo analysis: {}", best.reason),
        ));
    }
    if let Some(look) = LOOKS
        .iter()
        .find(|l| l.key == request || l.label.to_lowercase() == request)
    {
        return Ok((Chosen::Look(look), "requested by name".into()));
    }
    if let Some(film) = film_library::find(&request) {
        return Ok((
            Chosen::Film(film),
            format!("film library preset ({})", film.category),
        ));
    }
    let words: Vec<_> = request
        .split(|c: char| !c.is_alphanumeric() && c != '&')
        .filter(|w| !w.is_empty())
        .collect();
    // A film named by its words ("portra 400", "tri-x"): the shortest such name
    // is the base stock rather than a push/pull or expired variant.
    let named = film_library::all()
        .iter()
        .filter(|f| {
            let name = f.name.to_lowercase();
            let tokens: Vec<_> = name
                .split(|c: char| !c.is_alphanumeric() && c != '&')
                .collect();
            words
                .iter()
                .all(|w| tokens.contains(w) || (w.len() > 3 && name.contains(*w)))
        })
        .min_by_key(|f| f.name.len());
    let best = LOOKS
        .iter()
        .map(|look| {
            let hits = words
                .iter()
                .filter(|w| look.moods.contains(w) || look.key.split('-').any(|k| k == **w))
                .count();
            (look, hits)
        })
        .filter(|(_, hits)| *hits > 0)
        .max_by(|(a, x), (b, y)| x.cmp(y).then(rank(a.key).total_cmp(&rank(b.key))));
    match (named, best) {
        (Some(film), Some((_, hits))) if hits < words.len() => Ok((
            Chosen::Film(film),
            format!(
                "film library preset matching \"{request}\" ({})",
                film.category
            ),
        )),
        (_, Some((look, _))) => Ok((Chosen::Look(look), format!("matched mood \"{request}\""))),
        (Some(film), None) => Ok((
            Chosen::Film(film),
            format!(
                "film library preset matching \"{request}\" ({})",
                film.category
            ),
        )),
        (None, None) => match film_library::search(&request, None, 1).first() {
            Some(film) => Ok((
                Chosen::Film(film),
                format!(
                    "closest film library match for \"{request}\": {}",
                    film.description
                ),
            )),
            None => Err(error(format!(
                "No look matches \"{request}\". Use \"auto\", a key from list_raw_looks ({}), a film name such as \"Kodak Portra 400\" (search with list_raw_looks query), or mood words such as warm, moody, cinematic, airy, vintage, vivid, cool, noir.",
                LOOKS.iter().map(|l| l.key).collect::<Vec<_>>().join(", ")
            ))),
        },
    }
}

// ---------------------------------------------------------------- tools

pub fn describe_analysis(doc: &Document, args: &Value) -> Result<ToolResult, ToolResult> {
    if args.as_object().is_none_or(|o| !o.is_empty()) {
        return Err(error("analyze_raw takes no arguments"));
    }
    let a = analyze_document(doc)?;
    let current = doc.raw.as_ref().map(|r| r.params).unwrap_or_default();
    let mut hints = Vec::new();
    if a.tone.clipped_highlights_pct > 0.5 {
        hints.push("highlights clip: raise highlights (recovery) or lower whites/exposure");
    }
    if a.tone.crushed_shadows_pct > 2. {
        hints.push("shadows crush: raise shadows or blacks");
    }
    if let (true, Some([hue, sat, _])) = (a.has("portrait"), a.skin)
        && (!(15.0..=30.).contains(&hue) || !(0.2..=0.5).contains(&sat))
    {
        hints.push("skin sits outside the flattering range (hue 18–28°, moderate saturation): correct it in the HSL orange/red bands, not the midtone grading wheel");
    }
    if a.cast.neutral_pct >= 5. && (a.cast.warmth.abs() > 0.08 || a.cast.magenta.abs() > 0.08) {
        hints.push("near-neutrals carry a colour cast; see suggested_white_balance (skip it for intentional sunset/tungsten mood)");
    }
    let suggested_wb = (a.cast.neutral_pct >= 5.).then(|| {
        json!({
            "temperature": (current.temperature - a.cast.warmth * 0.7).clamp(-1., 1.),
            "tint": (current.tint + a.cast.magenta * 0.7).clamp(-1., 1.),
        })
    });
    Ok(ToolResult::text(
        json!({
            "analysis": a,
            "hints": hints,
            "suggested_white_balance": suggested_wb,
            "recommended_looks": recommend(&a).into_iter().take(4).collect::<Vec<_>>(),
            "palette_mood": palette_mood(&a),
            "film_recommendations": film_recommendations(&a),
            "harmony_anchor_hue": match (a.has("portrait"), a.skin) {
                (true, Some([hue, ..])) => Some(hue),
                _ => a.dominant_hue,
            },
            "measured": "current developed RAW pixels, display-encoded; percentages are of sampled pixels",
            "next": "Pair with get_view to judge content and mood; apply_raw_look with a key, mood words or \"auto\"; fine-tune with develop_raw.",
        })
        .to_string(),
    ))
}

pub fn list(args: &Value) -> Result<ToolResult, ToolResult> {
    let object = args
        .as_object()
        .ok_or_else(|| error("Arguments must be an object"))?;
    if let Some(key) = object
        .keys()
        .find(|k| !["query", "category", "limit"].contains(&k.as_str()))
    {
        return Err(error(format!("Unknown argument '{key}'")));
    }
    let query = args.get("query").and_then(Value::as_str).unwrap_or("");
    let category = args.get("category").and_then(Value::as_str);
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(20)
        .clamp(1, 100) as usize;
    let words: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    let looks: Vec<_> = LOOKS
        .iter()
        .filter(|l| {
            category.is_none()
                && (words.is_empty()
                    || words
                        .iter()
                        .any(|w| l.moods.contains(&w.as_str()) || l.key.contains(w.as_str())))
        })
        .map(|l| {
            json!({"key":l.key,"label":l.label,"moods":l.moods,"description":l.description,
                "comparable_to":l.comparable_to,"best_for":l.suits,"avoid":l.avoid})
        })
        .collect();
    let film: Vec<_> = if words.is_empty() && category.is_none() {
        Vec::new()
    } else {
        film_library::search(query, category, limit)
            .into_iter()
            .map(|p| json!({"name": p.name, "category": p.category, "description": p.description}))
            .collect()
    };
    let categories: Vec<_> = film_library::categories()
        .into_iter()
        .map(|(c, n)| json!({"category": c, "presets": n}))
        .collect();
    Ok(ToolResult::text(json!({"looks":looks, "film_library": film,
        "film_categories": categories,
        "note":"looks are Emulsion's adaptive looks. film_library holds 451 bundled film and creative presets (peva3/Lightroom-Presets, MIT); search them with query (film names, moods, genres) or category, and apply either kind by name with apply_raw_look. Film presets are translated to Emulsion's renderer, so they can differ from the original presets."}).to_string()))
}

pub fn plan(doc: &Document, args: &Value) -> Result<Planned, ToolResult> {
    let object = args
        .as_object()
        .ok_or_else(|| error("Arguments must be an object"))?;
    for key in object.keys() {
        if !["look", "strength", "correct", "harmony"].contains(&key.as_str()) {
            return Err(error(format!("Unknown argument '{key}'")));
        }
    }
    let request = match args.get("look") {
        None => "auto",
        Some(v) => v.as_str().ok_or_else(|| error("look must be a string"))?,
    };
    let strength = match args.get("strength") {
        None => 1.,
        Some(v) => {
            v.as_f64()
                .filter(|v| (0.0..=1.5).contains(v))
                .ok_or_else(|| error("strength must be a number from 0 to 1.5"))? as f32
        }
    };
    let harmony = match args.get("harmony") {
        None => None,
        Some(v) => v.as_str().and_then(Harmony::parse).ok_or_else(|| {
            error("harmony must be auto, none, complementary, split_complementary, analogous, triadic, or monochromatic")
        })?,
    };
    let correct = match args.get("correct") {
        None => true,
        Some(v) => v
            .as_bool()
            .ok_or_else(|| error("correct must be a boolean"))?,
    };
    let raw = doc
        .raw
        .as_ref()
        .ok_or_else(|| error("No editable RAW source; open a supported camera RAW file first"))?;
    raw.validate().map_err(error)?;
    let analysis = analyze_document(doc)?;
    let (chosen, why) = resolve(request, &analysis)?;
    // The film preset's own values, relative to a neutral photo.
    let film = match chosen {
        Chosen::Film(f) => Some(f.load(DevelopParams::default()).map_err(error)?),
        Chosen::Look(_) => None,
    };
    let film_sets_wb = film.as_ref().is_some_and(|f| {
        f.params.kelvin.is_some() || f.params.temperature != 0. || f.params.tint != 0.
    });
    let neutralize_cast = match chosen {
        Chosen::Look(l) => l.neutralize_cast,
        Chosen::Film(_) => !film_sets_wb,
    };

    let current = raw.params;
    let mut cleared = current;
    clear_look(&mut cleared);
    let source = PhotoSource::load_verified(&raw.source, &raw.source_sha256).map_err(error)?;
    let mut corrections = Vec::new();
    let grade_free = cleared == current;
    let mut base = cleared;
    if correct {
        base = source.auto_adjust(&cleared).map_err(error)?;
        if analysis.has("night") || analysis.has("low_key") {
            // Keep the darkness that makes a night or low-key photo.
            base.brightness *= 0.5;
            base.exposure = current.exposure + (base.exposure - current.exposure) * 0.5;
        }
        base.highlights = if analysis.has("contrasty") {
            0.35
        } else {
            0.15
        };
        base.shadows = if analysis.tone.percentiles[1] < 0.06 {
            0.25
        } else {
            0.08
        };
        corrections.push(format!(
            "auto tone: exposure {:+.2}, highlight recovery {:.2}, shadow lift {:.2}",
            base.exposure, base.highlights, base.shadows
        ));
        let cast = &analysis.cast;
        if neutralize_cast
            && grade_free
            && !analysis.has("golden_light")
            && !analysis.has("night")
            && cast.neutral_pct >= 5.
            && (cast.warmth.abs() > 0.08 || cast.magenta.abs() > 0.08)
        {
            base.temperature = (current.temperature - cast.warmth * 0.6).clamp(-1., 1.);
            base.tint = (current.tint + cast.magenta * 0.6).clamp(-1., 1.);
            corrections.push(format!(
                "neutralised colour cast: temperature {:+.2}, tint {:+.2}",
                base.temperature, base.tint
            ));
        }
    }
    let mut styled = base;
    match (chosen, &film) {
        (Chosen::Look(look), _) => (look.build)(&mut styled),
        (Chosen::Film(_), Some(film)) => {
            let pure = film.params;
            styled = develop_presets::apply(&base, &pure);
            // Preset exposure and incremental balance are offsets on the corrected photo.
            styled.exposure = (base.exposure + pure.exposure).clamp(-5., 5.);
            styled.brightness = (base.brightness + pure.brightness).clamp(-1., 1.);
            styled.black_point = (base.black_point + pure.black_point).clamp(0., 0.25);
            if pure.kelvin.is_none() {
                styled.kelvin = base.kelvin;
                styled.wb_override = base.wb_override;
                styled.temperature = (base.temperature + pure.temperature).clamp(-1., 1.);
                styled.tint = (base.tint + pure.tint).clamp(-1., 1.);
            }
            if pure.highlights == 0. {
                styled.highlights = base.highlights;
            }
            if pure.shadows == 0. {
                styled.shadows = base.shadows;
            }
            styled.masks = base.masks;
        }
        (Chosen::Film(_), None) => unreachable!(),
    }
    let harmony = harmony.unwrap_or(match chosen {
        Chosen::Look(look) => look.harmony,
        // Film stocks carry their own colour; add a scheme only when asked.
        Chosen::Film(_) => Harmony::None,
    });
    // Anchor on the subject: skin when people are present, else the dominant colour.
    let anchor = match (
        analysis.has("portrait"),
        analysis.skin,
        analysis.dominant_hue,
    ) {
        (true, Some([hue, ..]), _) => Some(hue),
        (_, _, hue) => hue,
    };
    let mut adaptations: Vec<String> = anchor
        .and_then(|anchor| apply_harmony(&mut styled, harmony, anchor))
        .into_iter()
        .collect();
    adaptations.extend(adapt(&mut styled, &analysis, grade_free));
    if correct && styled.saturation > -1. && base.exposure - current.exposure > 0.5 {
        // Hunt effect: brighter images read more colourful, so ease saturation.
        let ease = (0.05 * (base.exposure - current.exposure - 0.5)).min(0.15);
        styled.saturation = (styled.saturation - ease).max(-0.9);
        adaptations.push(format!(
            "brightened by {:+.1} stop: eased saturation {ease:.2} (Hunt effect)",
            base.exposure - current.exposure
        ));
    }
    adaptations.extend(style_guide(&mut styled, &analysis));
    let mono = styled.saturation <= -1.;
    if correct && analysis.sky_pct >= 8. && add_sky_grad(&mut styled, mono) {
        adaptations.push("graduated filter darkens the sky by 0.4 stop".into());
    }
    let result = blend(&base, &styled, strength);
    result.validate().map_err(error)?;
    let mut planned = crate::raw_tools::develop(doc, result, Some(source))?;
    planned.message = json!({
        "look": chosen.key(),
        "label": chosen.label(),
        "source": match chosen { Chosen::Look(_) => "emulsion adaptive look", Chosen::Film(f) => f.category },
        "film_notes": film.as_ref().map(|f| f.warnings.iter().skip(1).collect::<Vec<_>>()),
        "why": why,
        "harmony": harmony,
        "strength": strength,
        "corrections": corrections,
        "adaptations": adaptations,
        "scene": analysis.scene,
        "alternatives": recommend(&analysis).into_iter().filter(|r| r.key != chosen.key()).take(3).collect::<Vec<_>>(),
        "film_alternatives": film_recommendations(&analysis).into_iter().filter(|f| f["name"] != chosen.key()).take(3).collect::<Vec<_>>(),
        "settings": result,
        "undo_steps": if planned.commands.is_empty() { 0 } else { 1 },
        "next": "Inspect with get_raw_preview (split) and get_view; refine with develop_raw (e.g. strength via a re-apply, white balance, HSL, grading).",
    })
    .to_string();
    Ok(planned)
}

pub fn definitions() -> Vec<ToolDef> {
    let def = |name: &str, description: &str, properties: Value| ToolDef {
        name: name.into(),
        description: description.into(),
        input_schema: json!({"type":"object","additionalProperties":false,"properties":properties,"required":[]}),
    };
    vec![
        def(
            "analyze_raw",
            "Measure the developed RAW photo: tonal percentiles, key, contrast spread, highlight/shadow clipping, colour cast of near-neutrals, saturation, hue distribution, skin/sky/foliage share, scene tags (portrait, landscape, night, golden_light, flat, contrasty, muted, colorful…), suggested white balance and ranked look recommendations. Read-only. Use with get_view before choosing a grade.",
            json!({}),
        ),
        def(
            "list_raw_looks",
            "List Emulsion's adaptive mood looks (key, mood words, description, preset genre, scenes suited/avoided) and search the bundled film library of 451 film and creative presets (Kodak Portra, Fuji Velvia, CineStill, Ilford, slide, B&W, cinematic, alternative process, genre, seasonal, decade, geographic, photographer styles). query matches film names, moods and descriptions; category narrows the library (film_categories lists them). Read-only.",
            json!({"query":{"type":"string"},"category":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":100,"default":20}}),
        ),
        def(
            "apply_raw_look",
            "Grade the RAW with an adaptive look in one undo step. look is \"auto\" (chosen from photo analysis), a key from list_raw_looks, a film library preset name (\"Kodak Portra 400\", \"Cinestill 800T\", \"tri-x\"), or mood words (\"warm moody film\", \"bright airy\", \"cinematic\"). correct=true first auto-balances exposure, highlights, shadows and a measured colour cast; false keeps the current exposure and white balance. The look is then fitted to the photo with retoucher rules: colour harmony (complementary, split-complementary, analogous, triadic, monochromatic) anchored on the subject; skin hue, saturation and brightness steered in the HSL orange/red bands; deeper skies, natural foliage and a graduated sky filter for landscapes; B&W colour filters chosen by scene; contrast and saturation trimmed to the scene; and the film style guide's limits (grain keeps sharpening at 10 with no added clarity/texture/dehaze, one local-contrast band leads, grading wheels at most 30 and midtones 10 on portraits, vibrance within 10 of saturation, no double-faded blacks, HSL within 60, saturation eased when brightening). Scaled by strength (0–1.5; 1 is the full look). Replaces any previous look's colour/curve/HSL/grading settings rather than stacking; geometry, detail, lens and masks are preserved. Result is plain editable RAW settings.",
            json!({
                "look":{"type":"string","default":"auto"},
                "strength":{"type":"number","minimum":0,"maximum":1.5,"default":1},
                "correct":{"type":"boolean","default":true},
                "harmony":{"type":"string","enum":["auto","none","complementary","split_complementary","analogous","triadic","monochromatic"],"default":"auto","description":"Colour-wheel scheme anchored on the subject (skin, else dominant hue). auto uses the look's own scheme."}
            }),
        ),
    ]
}

#[cfg(test)]
#[path = "raw_looks_tests.rs"]
mod tests;
