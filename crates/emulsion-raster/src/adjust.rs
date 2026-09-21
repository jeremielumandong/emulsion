//! Per-pixel adjustments.
//!
//! An [`Adjustment`] is plain data with named parameters, so the UI can show
//! it as sliders and the file format can store it. [`Adjustment::prepare`]
//! turns it into a [`Prepared`] operator, with lookup tables where the
//! adjustment is separable per channel.
//!
//! Adjustments that Photoshop defines on encoded values (levels, curves,
//! brightness / contrast, hue / saturation, colour balance, LUTs) run on
//! sRGB-encoded values; exposure and white balance run in linear light.

use crate::color::{linear_to_srgb, srgb_to_linear};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// A colour stop of a gradient map: position 0–1 and straight sRGB.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stop {
    pub pos: f32,
    pub color: [u8; 3],
}

/// A 3D LUT, `size³` entries with red varying fastest, values 0–65535.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cube {
    pub name: String,
    pub size: u32,
    pub data: Arc<Vec<[u16; 3]>>,
}

impl Cube {
    /// Parse an Adobe/Resolve `.cube` file.
    pub fn parse(text: &str) -> Result<Cube, String> {
        let mut name = String::new();
        let mut size = 0u32;
        let mut min = [0.0f32; 3];
        let mut max = [1.0f32; 3];
        let mut data: Vec<[u16; 3]> = Vec::new();
        for line in text.lines() {
            let l = line.trim();
            if l.is_empty() || l.starts_with('#') {
                continue;
            }
            let mut it = l.split_whitespace();
            let head = it.next().unwrap_or("");
            match head {
                "TITLE" => name = l[5..].trim().trim_matches('"').to_string(),
                "LUT_3D_SIZE" => {
                    size = it
                        .next()
                        .and_then(|v| v.parse().ok())
                        .ok_or("bad LUT_3D_SIZE")?
                }
                "LUT_1D_SIZE" => return Err("1D LUTs are not supported; use a 3D .cube".into()),
                "DOMAIN_MIN" | "DOMAIN_MAX" => {
                    let v: Vec<f32> = it.filter_map(|x| x.parse().ok()).collect();
                    if v.len() == 3 {
                        let t = if head == "DOMAIN_MIN" {
                            &mut min
                        } else {
                            &mut max
                        };
                        t.copy_from_slice(&v);
                    }
                }
                _ => {
                    let v: Vec<f32> = l
                        .split_whitespace()
                        .filter_map(|x| x.parse().ok())
                        .collect();
                    if v.len() == 3 {
                        data.push([0, 1, 2].map(|i| {
                            let t = ((v[i] - min[i]) / (max[i] - min[i]).max(1e-6)).clamp(0.0, 1.0);
                            (t * 65535.0).round() as u16
                        }));
                    }
                }
            }
        }
        if !(2..=128).contains(&size) {
            return Err("LUT_3D_SIZE must be 2–128".into());
        }
        if data.len() != (size * size * size) as usize {
            return Err(format!(
                "expected {} entries, found {}",
                size * size * size,
                data.len()
            ));
        }
        Ok(Cube {
            name,
            size,
            data: Arc::new(data),
        })
    }

    /// Trilinear lookup on encoded sRGB in 0–1.
    fn sample(&self, e: [f32; 3]) -> [f32; 3] {
        let n = self.size as usize;
        let s = (n - 1) as f32;
        let f = e.map(|v| v.clamp(0.0, 1.0) * s);
        let i0 = f.map(|v| (v.floor() as usize).min(n - 1));
        let i1 = i0.map(|i| (i + 1).min(n - 1));
        let t = [0, 1, 2].map(|k| f[k] - i0[k] as f32);
        let at = |r: usize, g: usize, b: usize| -> [f32; 3] {
            let p = self.data[r + g * n + b * n * n];
            p.map(|v| v as f32 / 65535.0)
        };
        let mut out = [0.0; 3];
        for (dr, wr) in [(i0[0], 1.0 - t[0]), (i1[0], t[0])] {
            for (dg, wg) in [(i0[1], 1.0 - t[1]), (i1[1], t[1])] {
                for (db, wb) in [(i0[2], 1.0 - t[2]), (i1[2], t[2])] {
                    let w = wr * wg * wb;
                    if w > 0.0 {
                        let c = at(dr, dg, db);
                        for k in 0..3 {
                            out[k] += c[k] * w;
                        }
                    }
                }
            }
        }
        out
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Adjustment {
    Exposure {
        exposure: f32,
        offset: f32,
        gamma: f32,
    },
    BrightnessContrast {
        brightness: f32,
        contrast: f32,
    },
    Levels {
        in_black: f32,
        in_white: f32,
        gamma: f32,
        out_black: f32,
        out_white: f32,
    },
    /// Points are (input, output) on 0–255 encoded values, sorted by input.
    /// Channel curves apply first, then the master curve.
    Curves {
        master: Vec<[f32; 2]>,
        red: Vec<[f32; 2]>,
        green: Vec<[f32; 2]>,
        blue: Vec<[f32; 2]>,
    },
    HueSaturation {
        hue: f32,
        saturation: f32,
        lightness: f32,
    },
    /// Cyan–red, magenta–green, yellow–blue for each tonal range, −100–100.
    ColorBalance {
        shadows: [f32; 3],
        midtones: [f32; 3],
        highlights: [f32; 3],
        preserve_luminosity: bool,
    },
    Vibrance {
        vibrance: f32,
        saturation: f32,
    },
    /// Per-hue weights, −200–300 like Photoshop; an optional tint.
    BlackAndWhite {
        reds: f32,
        yellows: f32,
        greens: f32,
        cyans: f32,
        blues: f32,
        magentas: f32,
        tint_hue: f32,
        tint_strength: f32,
    },
    PhotoFilter {
        hue: f32,
        saturation: f32,
        density: f32,
        preserve_luminosity: bool,
    },
    GradientMap {
        stops: Vec<Stop>,
        reverse: bool,
    },
    Grain {
        amount: f32,
        size: f32,
        monochrome: bool,
    },
    /// Darken the corners (or lighten them with a negative amount).
    Vignette {
        amount: f32,
        midpoint: f32,
        feather: f32,
        roundness: f32,
    },
    WhiteBalance {
        temperature: f32,
        tint: f32,
    },
    Threshold {
        level: f32,
    },
    Posterize {
        levels: f32,
    },
    Lut3D {
        cube: Cube,
        strength: f32,
    },
    Invert,
}

/// One slider's worth of metadata.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub value: f32,
    pub unit: &'static str,
}

impl ParamSpec {
    pub fn display(&self) -> String {
        match self.unit {
            "ev" => format!("{:+.2} ev", self.value),
            "°" => format!("{:+.0}°", self.value),
            "on" => if self.value >= 0.5 { "on" } else { "off" }.into(),
            "" if self.step < 1.0 => format!("{:.2}", self.value),
            u => format!("{:.0}{u}", self.value),
        }
    }
}

fn b2f(b: bool) -> f32 {
    if b { 1.0 } else { 0.0 }
}

/// The straight curve: identity.
pub fn straight_curve() -> Vec<[f32; 2]> {
    vec![[0.0, 0.0], [255.0, 255.0]]
}

impl Adjustment {
    /// Every kind with neutral parameters, for "add adjustment" menus.
    pub fn catalogue() -> Vec<Adjustment> {
        vec![
            Adjustment::Exposure {
                exposure: 0.0,
                offset: 0.0,
                gamma: 1.0,
            },
            Adjustment::BrightnessContrast {
                brightness: 0.0,
                contrast: 0.0,
            },
            Adjustment::Levels {
                in_black: 0.0,
                in_white: 255.0,
                gamma: 1.0,
                out_black: 0.0,
                out_white: 255.0,
            },
            Adjustment::Curves {
                master: straight_curve(),
                red: straight_curve(),
                green: straight_curve(),
                blue: straight_curve(),
            },
            Adjustment::HueSaturation {
                hue: 0.0,
                saturation: 0.0,
                lightness: 0.0,
            },
            Adjustment::ColorBalance {
                shadows: [0.0; 3],
                midtones: [0.0; 3],
                highlights: [0.0; 3],
                preserve_luminosity: true,
            },
            Adjustment::Vibrance {
                vibrance: 0.0,
                saturation: 0.0,
            },
            Adjustment::BlackAndWhite {
                reds: 40.0,
                yellows: 60.0,
                greens: 40.0,
                cyans: 60.0,
                blues: 20.0,
                magentas: 80.0,
                tint_hue: 42.0,
                tint_strength: 0.0,
            },
            Adjustment::PhotoFilter {
                hue: 30.0,
                saturation: 100.0,
                density: 0.0,
                preserve_luminosity: true,
            },
            Adjustment::GradientMap {
                stops: vec![
                    Stop {
                        pos: 0.0,
                        color: [0, 0, 0],
                    },
                    Stop {
                        pos: 1.0,
                        color: [255, 255, 255],
                    },
                ],
                reverse: false,
            },
            Adjustment::Grain {
                amount: 0.0,
                size: 1.5,
                monochrome: true,
            },
            Adjustment::Vignette {
                amount: 40.0,
                midpoint: 45.0,
                feather: 60.0,
                roundness: 30.0,
            },
            Adjustment::WhiteBalance {
                temperature: 0.0,
                tint: 0.0,
            },
            Adjustment::Threshold { level: 128.0 },
            Adjustment::Posterize { levels: 256.0 },
            Adjustment::Invert,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Adjustment::Exposure { .. } => "Exposure",
            Adjustment::BrightnessContrast { .. } => "Brightness / Contrast",
            Adjustment::Levels { .. } => "Levels",
            Adjustment::Curves { .. } => "Curves",
            Adjustment::HueSaturation { .. } => "Hue / Saturation",
            Adjustment::ColorBalance { .. } => "Color balance",
            Adjustment::Vibrance { .. } => "Vibrance",
            Adjustment::BlackAndWhite { .. } => "Black & white",
            Adjustment::PhotoFilter { .. } => "Photo filter",
            Adjustment::GradientMap { .. } => "Gradient map",
            Adjustment::Grain { .. } => "Grain",
            Adjustment::Vignette { .. } => "Vignette",
            Adjustment::WhiteBalance { .. } => "White balance",
            Adjustment::Threshold { .. } => "Threshold",
            Adjustment::Posterize { .. } => "Posterize",
            Adjustment::Lut3D { .. } => "LUT",
            Adjustment::Invert => "Invert",
        }
    }

    /// The name the assistant and files use: lowercase with underscores.
    pub fn key(&self) -> &'static str {
        match self {
            Adjustment::Exposure { .. } => "exposure",
            Adjustment::BrightnessContrast { .. } => "brightness_contrast",
            Adjustment::Levels { .. } => "levels",
            Adjustment::Curves { .. } => "curves",
            Adjustment::HueSaturation { .. } => "hue_saturation",
            Adjustment::ColorBalance { .. } => "color_balance",
            Adjustment::Vibrance { .. } => "vibrance",
            Adjustment::BlackAndWhite { .. } => "black_and_white",
            Adjustment::PhotoFilter { .. } => "photo_filter",
            Adjustment::GradientMap { .. } => "gradient_map",
            Adjustment::Grain { .. } => "grain",
            Adjustment::Vignette { .. } => "vignette",
            Adjustment::WhiteBalance { .. } => "white_balance",
            Adjustment::Threshold { .. } => "threshold",
            Adjustment::Posterize { .. } => "posterize",
            Adjustment::Lut3D { .. } => "lut",
            Adjustment::Invert => "invert",
        }
    }

    pub fn params(&self) -> Vec<ParamSpec> {
        let p = |key, label, min, max, step, value, unit| ParamSpec {
            key,
            label,
            min,
            max,
            step,
            value,
            unit,
        };
        match self {
            Adjustment::Exposure {
                exposure,
                offset,
                gamma,
            } => vec![
                p("exposure", "exposure", -5.0, 5.0, 0.01, *exposure, "ev"),
                p("offset", "offset", -0.5, 0.5, 0.001, *offset, ""),
                p("gamma", "gamma", 0.1, 3.0, 0.01, *gamma, ""),
            ],
            Adjustment::BrightnessContrast {
                brightness,
                contrast,
            } => vec![
                p(
                    "brightness",
                    "brightness",
                    -150.0,
                    150.0,
                    1.0,
                    *brightness,
                    "",
                ),
                p("contrast", "contrast", -50.0, 100.0, 1.0, *contrast, ""),
            ],
            Adjustment::Levels {
                in_black,
                in_white,
                gamma,
                out_black,
                out_white,
            } => vec![
                p("in_black", "input black", 0.0, 253.0, 1.0, *in_black, ""),
                p("in_white", "input white", 2.0, 255.0, 1.0, *in_white, ""),
                p("gamma", "midtones", 0.1, 9.99, 0.01, *gamma, ""),
                p("out_black", "output black", 0.0, 255.0, 1.0, *out_black, ""),
                p("out_white", "output white", 0.0, 255.0, 1.0, *out_white, ""),
            ],
            Adjustment::Curves { .. } => vec![],
            Adjustment::HueSaturation {
                hue,
                saturation,
                lightness,
            } => vec![
                p("hue", "hue", -180.0, 180.0, 1.0, *hue, "°"),
                p(
                    "saturation",
                    "saturation",
                    -100.0,
                    100.0,
                    1.0,
                    *saturation,
                    "",
                ),
                p("lightness", "lightness", -100.0, 100.0, 1.0, *lightness, ""),
            ],
            Adjustment::ColorBalance {
                shadows,
                midtones,
                highlights,
                preserve_luminosity,
            } => vec![
                p(
                    "shadows_cr",
                    "shadows cyan–red",
                    -100.0,
                    100.0,
                    1.0,
                    shadows[0],
                    "",
                ),
                p(
                    "shadows_mg",
                    "shadows magenta–green",
                    -100.0,
                    100.0,
                    1.0,
                    shadows[1],
                    "",
                ),
                p(
                    "shadows_yb",
                    "shadows yellow–blue",
                    -100.0,
                    100.0,
                    1.0,
                    shadows[2],
                    "",
                ),
                p(
                    "midtones_cr",
                    "midtones cyan–red",
                    -100.0,
                    100.0,
                    1.0,
                    midtones[0],
                    "",
                ),
                p(
                    "midtones_mg",
                    "midtones magenta–green",
                    -100.0,
                    100.0,
                    1.0,
                    midtones[1],
                    "",
                ),
                p(
                    "midtones_yb",
                    "midtones yellow–blue",
                    -100.0,
                    100.0,
                    1.0,
                    midtones[2],
                    "",
                ),
                p(
                    "highlights_cr",
                    "highlights cyan–red",
                    -100.0,
                    100.0,
                    1.0,
                    highlights[0],
                    "",
                ),
                p(
                    "highlights_mg",
                    "highlights magenta–green",
                    -100.0,
                    100.0,
                    1.0,
                    highlights[1],
                    "",
                ),
                p(
                    "highlights_yb",
                    "highlights yellow–blue",
                    -100.0,
                    100.0,
                    1.0,
                    highlights[2],
                    "",
                ),
                p(
                    "preserve_luminosity",
                    "preserve luminosity",
                    0.0,
                    1.0,
                    1.0,
                    b2f(*preserve_luminosity),
                    "on",
                ),
            ],
            Adjustment::Vibrance {
                vibrance,
                saturation,
            } => vec![
                p("vibrance", "vibrance", -100.0, 100.0, 1.0, *vibrance, ""),
                p(
                    "saturation",
                    "saturation",
                    -100.0,
                    100.0,
                    1.0,
                    *saturation,
                    "",
                ),
            ],
            Adjustment::BlackAndWhite {
                reds,
                yellows,
                greens,
                cyans,
                blues,
                magentas,
                tint_hue,
                tint_strength,
            } => vec![
                p("reds", "reds", -200.0, 300.0, 1.0, *reds, ""),
                p("yellows", "yellows", -200.0, 300.0, 1.0, *yellows, ""),
                p("greens", "greens", -200.0, 300.0, 1.0, *greens, ""),
                p("cyans", "cyans", -200.0, 300.0, 1.0, *cyans, ""),
                p("blues", "blues", -200.0, 300.0, 1.0, *blues, ""),
                p("magentas", "magentas", -200.0, 300.0, 1.0, *magentas, ""),
                p("tint_hue", "tint hue", 0.0, 360.0, 1.0, *tint_hue, "°"),
                p("tint_strength", "tint", 0.0, 100.0, 1.0, *tint_strength, ""),
            ],
            Adjustment::PhotoFilter {
                hue,
                saturation,
                density,
                preserve_luminosity,
            } => vec![
                p("hue", "filter hue", 0.0, 360.0, 1.0, *hue, "°"),
                p(
                    "saturation",
                    "filter saturation",
                    0.0,
                    100.0,
                    1.0,
                    *saturation,
                    "",
                ),
                p("density", "density", 0.0, 100.0, 1.0, *density, ""),
                p(
                    "preserve_luminosity",
                    "preserve luminosity",
                    0.0,
                    1.0,
                    1.0,
                    b2f(*preserve_luminosity),
                    "on",
                ),
            ],
            Adjustment::GradientMap { reverse, .. } => {
                vec![p("reverse", "reverse", 0.0, 1.0, 1.0, b2f(*reverse), "on")]
            }
            Adjustment::Vignette {
                amount,
                midpoint,
                feather,
                roundness,
            } => vec![
                p("amount", "amount", -100.0, 100.0, 1.0, *amount, ""),
                p("midpoint", "midpoint", 0.0, 100.0, 1.0, *midpoint, ""),
                p("feather", "feather", 1.0, 100.0, 1.0, *feather, ""),
                p("roundness", "roundness", 0.0, 100.0, 1.0, *roundness, ""),
            ],
            Adjustment::Grain {
                amount,
                size,
                monochrome,
            } => vec![
                p("amount", "amount", 0.0, 100.0, 1.0, *amount, ""),
                p("size", "size", 0.5, 8.0, 0.1, *size, "px"),
                p(
                    "monochrome",
                    "monochrome",
                    0.0,
                    1.0,
                    1.0,
                    b2f(*monochrome),
                    "on",
                ),
            ],
            Adjustment::WhiteBalance { temperature, tint } => vec![
                p(
                    "temperature",
                    "warmth",
                    -100.0,
                    100.0,
                    1.0,
                    *temperature,
                    "",
                ),
                p("tint", "tint", -100.0, 100.0, 1.0, *tint, ""),
            ],
            Adjustment::Threshold { level } => {
                vec![p("level", "level", 1.0, 255.0, 1.0, *level, "")]
            }
            Adjustment::Posterize { levels } => {
                vec![p("levels", "levels", 2.0, 256.0, 1.0, *levels, "")]
            }
            Adjustment::Lut3D { strength, .. } => {
                vec![p("strength", "strength", 0.0, 100.0, 1.0, *strength, "")]
            }
            Adjustment::Invert => vec![],
        }
    }

    /// Set a parameter by key, clamped to its range. Returns false for an
    /// unknown key.
    pub fn set_param(&mut self, key: &str, value: f32) -> bool {
        let Some(spec) = self.params().into_iter().find(|s| s.key == key) else {
            return false;
        };
        let v = value.clamp(spec.min, spec.max);
        let on = v >= 0.5;
        let slot: &mut f32 = match (self, key) {
            (Adjustment::Exposure { exposure, .. }, "exposure") => exposure,
            (Adjustment::Exposure { offset, .. }, "offset") => offset,
            (Adjustment::Exposure { gamma, .. }, "gamma") => gamma,
            (Adjustment::BrightnessContrast { brightness, .. }, "brightness") => brightness,
            (Adjustment::BrightnessContrast { contrast, .. }, "contrast") => contrast,
            (Adjustment::Levels { in_black, .. }, "in_black") => in_black,
            (Adjustment::Levels { in_white, .. }, "in_white") => in_white,
            (Adjustment::Levels { gamma, .. }, "gamma") => gamma,
            (Adjustment::Levels { out_black, .. }, "out_black") => out_black,
            (Adjustment::Levels { out_white, .. }, "out_white") => out_white,
            (Adjustment::HueSaturation { hue, .. }, "hue") => hue,
            (Adjustment::HueSaturation { saturation, .. }, "saturation") => saturation,
            (Adjustment::HueSaturation { lightness, .. }, "lightness") => lightness,
            (Adjustment::ColorBalance { shadows, .. }, "shadows_cr") => &mut shadows[0],
            (Adjustment::ColorBalance { shadows, .. }, "shadows_mg") => &mut shadows[1],
            (Adjustment::ColorBalance { shadows, .. }, "shadows_yb") => &mut shadows[2],
            (Adjustment::ColorBalance { midtones, .. }, "midtones_cr") => &mut midtones[0],
            (Adjustment::ColorBalance { midtones, .. }, "midtones_mg") => &mut midtones[1],
            (Adjustment::ColorBalance { midtones, .. }, "midtones_yb") => &mut midtones[2],
            (Adjustment::ColorBalance { highlights, .. }, "highlights_cr") => &mut highlights[0],
            (Adjustment::ColorBalance { highlights, .. }, "highlights_mg") => &mut highlights[1],
            (Adjustment::ColorBalance { highlights, .. }, "highlights_yb") => &mut highlights[2],
            (
                Adjustment::ColorBalance {
                    preserve_luminosity,
                    ..
                },
                "preserve_luminosity",
            ) => {
                *preserve_luminosity = on;
                return true;
            }
            (Adjustment::Vibrance { vibrance, .. }, "vibrance") => vibrance,
            (Adjustment::Vibrance { saturation, .. }, "saturation") => saturation,
            (Adjustment::BlackAndWhite { reds, .. }, "reds") => reds,
            (Adjustment::BlackAndWhite { yellows, .. }, "yellows") => yellows,
            (Adjustment::BlackAndWhite { greens, .. }, "greens") => greens,
            (Adjustment::BlackAndWhite { cyans, .. }, "cyans") => cyans,
            (Adjustment::BlackAndWhite { blues, .. }, "blues") => blues,
            (Adjustment::BlackAndWhite { magentas, .. }, "magentas") => magentas,
            (Adjustment::BlackAndWhite { tint_hue, .. }, "tint_hue") => tint_hue,
            (Adjustment::BlackAndWhite { tint_strength, .. }, "tint_strength") => tint_strength,
            (Adjustment::PhotoFilter { hue, .. }, "hue") => hue,
            (Adjustment::PhotoFilter { saturation, .. }, "saturation") => saturation,
            (Adjustment::PhotoFilter { density, .. }, "density") => density,
            (
                Adjustment::PhotoFilter {
                    preserve_luminosity,
                    ..
                },
                "preserve_luminosity",
            ) => {
                *preserve_luminosity = on;
                return true;
            }
            (Adjustment::GradientMap { reverse, .. }, "reverse") => {
                *reverse = on;
                return true;
            }
            (Adjustment::Grain { amount, .. }, "amount") => amount,
            (Adjustment::Grain { size, .. }, "size") => size,
            (Adjustment::Vignette { amount, .. }, "amount") => amount,
            (Adjustment::Vignette { midpoint, .. }, "midpoint") => midpoint,
            (Adjustment::Vignette { feather, .. }, "feather") => feather,
            (Adjustment::Vignette { roundness, .. }, "roundness") => roundness,
            (Adjustment::Grain { monochrome, .. }, "monochrome") => {
                *monochrome = on;
                return true;
            }
            (Adjustment::WhiteBalance { temperature, .. }, "temperature") => temperature,
            (Adjustment::WhiteBalance { tint, .. }, "tint") => tint,
            (Adjustment::Threshold { level }, "level") => level,
            (Adjustment::Posterize { levels }, "levels") => levels,
            (Adjustment::Lut3D { strength, .. }, "strength") => strength,
            _ => return false,
        };
        *slot = v;
        true
    }

    /// Levels that stretch the histogram so `clip` percent of pixels clip
    /// at each end (Photoshop's Auto uses 0.1 %).
    pub fn auto_levels(hist: &Histogram, clip: f32) -> Adjustment {
        let total: u64 = hist.luma.iter().map(|v| *v as u64).sum();
        let target = (total as f64 * (clip as f64 / 100.0)) as u64;
        let (mut lo, mut hi) = (0usize, 255usize);
        let mut acc = 0u64;
        for (i, v) in hist.luma.iter().enumerate() {
            acc += *v as u64;
            if acc > target {
                lo = i;
                break;
            }
        }
        acc = 0;
        for (i, v) in hist.luma.iter().enumerate().rev() {
            acc += *v as u64;
            if acc > target {
                hi = i;
                break;
            }
        }
        if hi <= lo + 1 {
            (lo, hi) = (0, 255);
        }
        Adjustment::Levels {
            in_black: lo as f32,
            in_white: hi as f32,
            gamma: 1.0,
            out_black: 0.0,
            out_white: 255.0,
        }
    }

    /// Build the render-time operator.
    pub fn prepare(&self) -> Prepared {
        match self {
            Adjustment::HueSaturation {
                hue,
                saturation,
                lightness,
            } => Prepared::HueSat {
                hue: hue / 360.0,
                sat: saturation / 100.0,
                light: lightness / 100.0,
            },
            Adjustment::ColorBalance {
                shadows,
                midtones,
                highlights,
                preserve_luminosity,
            } => Prepared::ColorBalance {
                s: shadows.map(|v| v / 100.0),
                m: midtones.map(|v| v / 100.0),
                h: highlights.map(|v| v / 100.0),
                preserve: *preserve_luminosity,
            },
            Adjustment::Vibrance {
                vibrance,
                saturation,
            } => Prepared::Vibrance {
                vib: vibrance / 100.0,
                sat: saturation / 100.0,
            },
            Adjustment::BlackAndWhite {
                reds,
                yellows,
                greens,
                cyans,
                blues,
                magentas,
                tint_hue,
                tint_strength,
            } => Prepared::BlackAndWhite {
                w: [*reds, *yellows, *greens, *cyans, *blues, *magentas].map(|v| v / 100.0),
                tint: (*tint_strength > 0.0)
                    .then(|| (hue_color(*tint_hue, 1.0), tint_strength / 100.0)),
            },
            Adjustment::PhotoFilter {
                hue,
                saturation,
                density,
                preserve_luminosity,
            } => Prepared::PhotoFilter {
                color: hue_color(*hue, saturation / 100.0),
                density: density / 100.0,
                preserve: *preserve_luminosity,
            },
            Adjustment::GradientMap { stops, reverse } => {
                let mut s = stops.clone();
                s.sort_by(|a, b| a.pos.total_cmp(&b.pos));
                if s.is_empty() {
                    s.push(Stop {
                        pos: 0.0,
                        color: [0, 0, 0],
                    });
                }
                // Bake to a 256-entry table on encoded luminance.
                let table: Vec<[f32; 3]> = (0..256)
                    .map(|i| {
                        let mut t = i as f32 / 255.0;
                        if *reverse {
                            t = 1.0 - t;
                        }
                        gradient_at(&s, t)
                    })
                    .collect();
                Prepared::GradientMap(table)
            }
            Adjustment::Grain {
                amount,
                size,
                monochrome,
            } => Prepared::Grain {
                amount: amount / 100.0,
                size: size.max(0.5),
                mono: *monochrome,
            },
            Adjustment::Vignette {
                amount,
                midpoint,
                feather,
                roundness,
            } => Prepared::Vignette {
                amount: (amount / 100.0).clamp(-1.0, 1.0),
                midpoint: (midpoint / 100.0).clamp(0.0, 1.0),
                feather: (feather / 100.0).clamp(0.01, 1.0),
                roundness: (roundness / 100.0).clamp(0.0, 1.0),
            },
            Adjustment::Threshold { level } => Prepared::Threshold(level / 255.0),
            Adjustment::Lut3D { cube, strength } => Prepared::Cube {
                cube: cube.clone(),
                strength: strength / 100.0,
            },
            _ => {
                let f = |ch: usize, l: f32| self.channel(ch, l);
                Prepared::Lut(Box::new([
                    build_lut(|l| f(0, l)),
                    build_lut(|l| f(1, l)),
                    build_lut(|l| f(2, l)),
                ]))
            }
        }
    }

    /// Channel-wise transfer on linear input for LUT-able kinds.
    fn channel(&self, ch: usize, l: f32) -> f32 {
        match self {
            Adjustment::Exposure {
                exposure,
                offset,
                gamma,
            } => {
                let v = l * 2f32.powf(*exposure) + offset;
                v.max(0.0).powf(1.0 / gamma.max(0.01))
            }
            Adjustment::BrightnessContrast {
                brightness,
                contrast,
            } => {
                // Photoshop's modern (non-legacy) curve approximated on encoded values.
                let e = linear_to_srgb(l);
                let b = brightness / 150.0;
                let e = if b >= 0.0 {
                    e + (1.0 - e) * b * e.sqrt().min(1.0)
                } else {
                    e * (1.0 + b)
                };
                let c = contrast / 100.0;
                let k = if c >= 0.0 { 1.0 + c * 2.0 } else { 1.0 + c };
                let e = ((e - 0.5) * k + 0.5).clamp(0.0, 1.0);
                srgb_to_linear(e)
            }
            Adjustment::Levels {
                in_black,
                in_white,
                gamma,
                out_black,
                out_white,
            } => {
                let e = linear_to_srgb(l) * 255.0;
                let t = ((e - in_black) / (in_white - in_black).max(1.0)).clamp(0.0, 1.0);
                let t = t.powf(1.0 / gamma.max(0.01));
                let o = (out_black + t * (out_white - out_black)) / 255.0;
                srgb_to_linear(o.clamp(0.0, 1.0))
            }
            Adjustment::Curves {
                master,
                red,
                green,
                blue,
            } => {
                let e = linear_to_srgb(l) * 255.0;
                let per = [red, green, blue][ch];
                let e = curve_at(master, curve_at(per, e));
                srgb_to_linear((e / 255.0).clamp(0.0, 1.0))
            }
            Adjustment::WhiteBalance { temperature, tint } => {
                let t = temperature / 100.0;
                let g = tint / 100.0;
                let gain = match ch {
                    0 => 1.0 + 0.3 * t,
                    1 => 1.0 - 0.25 * g,
                    _ => 1.0 - 0.3 * t,
                };
                l * gain.max(0.0)
            }
            Adjustment::Posterize { levels } => {
                let n = levels.round().clamp(2.0, 256.0);
                if n >= 256.0 {
                    return l;
                }
                let e = linear_to_srgb(l);
                let q = (e * (n - 1.0)).round() / (n - 1.0);
                srgb_to_linear(q)
            }
            Adjustment::Invert => srgb_to_linear(1.0 - linear_to_srgb(l)),
            _ => l,
        }
    }
}

/// Evaluate a curve (points on 0–255, sorted by x) at `x`, with monotone
/// cubic interpolation so the curve never overshoots between points.
pub fn curve_at(points: &[[f32; 2]], x: f32) -> f32 {
    let n = points.len();
    if n == 0 {
        return x;
    }
    if n == 1 {
        return points[0][1];
    }
    if x <= points[0][0] {
        return points[0][1];
    }
    if x >= points[n - 1][0] {
        return points[n - 1][1];
    }
    // Only the two tangents bounding the sample contribute to interpolation.
    let secant =
        |i: usize| (points[i + 1][1] - points[i][1]) / (points[i + 1][0] - points[i][0]).max(1e-6);
    let tangent = |i: usize| {
        if i == 0 {
            secant(0)
        } else if i == n - 1 {
            secant(n - 2)
        } else {
            let (a, b) = (secant(i - 1), secant(i));
            if a * b <= 0.0 {
                0.0
            } else {
                2.0 / (1.0 / a + 1.0 / b)
            }
        }
    };
    let i = (0..n - 1).find(|&i| x < points[i + 1][0]).unwrap_or(n - 2);
    let (x0, y0, x1, y1) = (
        points[i][0],
        points[i][1],
        points[i + 1][0],
        points[i + 1][1],
    );
    let h = (x1 - x0).max(1e-6);
    let t = (x - x0) / h;
    let (t2, t3) = (t * t, t * t * t);
    let (h00, h10, h01, h11) = (
        2.0 * t3 - 3.0 * t2 + 1.0,
        t3 - 2.0 * t2 + t,
        -2.0 * t3 + 3.0 * t2,
        t3 - t2,
    );
    (h00 * y0 + h10 * h * tangent(i) + h01 * y1 + h11 * h * tangent(i + 1)).clamp(0.0, 255.0)
}

fn gradient_at(stops: &[Stop], t: f32) -> [f32; 3] {
    let c = |s: &Stop| s.color.map(|v| v as f32 / 255.0);
    if t <= stops[0].pos {
        return c(&stops[0]);
    }
    for w in stops.windows(2) {
        if t <= w[1].pos {
            let f = ((t - w[0].pos) / (w[1].pos - w[0].pos).max(1e-6)).clamp(0.0, 1.0);
            let (a, b) = (c(&w[0]), c(&w[1]));
            return [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * f);
        }
    }
    c(stops.last().expect("non-empty"))
}

/// Encoded sRGB colour of a hue (degrees) at `sat` 0–1, full lightness.
fn hue_color(hue: f32, sat: f32) -> [f32; 3] {
    let c = hsl_to_rgb((hue / 360.0).rem_euclid(1.0), 1.0, 0.5);
    c.map(|v| 1.0 + (v - 1.0) * sat)
}

const LUT_N: usize = 4096;

fn build_lut(f: impl Fn(f32) -> f32) -> Vec<f32> {
    (0..LUT_N)
        .map(|i| f(i as f32 / (LUT_N - 1) as f32))
        .collect()
}

#[inline]
fn lut(t: &[f32], v: f32) -> f32 {
    let x = v.clamp(0.0, 1.0) * (LUT_N - 1) as f32;
    let i = x as usize;
    if i >= LUT_N - 1 {
        return t[LUT_N - 1];
    }
    let f = x - i as f32;
    t[i] + (t[i + 1] - t[i]) * f
}

#[inline]
fn enc(c: [f32; 3]) -> [f32; 3] {
    c.map(|v| linear_to_srgb(v.clamp(0.0, 1.0)))
}

#[inline]
fn dec(e: [f32; 3]) -> [f32; 3] {
    e.map(|v| srgb_to_linear(v.clamp(0.0, 1.0)))
}

#[inline]
fn luma_enc(e: [f32; 3]) -> f32 {
    0.299 * e[0] + 0.587 * e[1] + 0.114 * e[2]
}

/// Rescale `e` so its luma matches `target`.
fn keep_luma(e: [f32; 3], target: f32) -> [f32; 3] {
    let l = luma_enc(e);
    if l <= 1e-6 {
        return [target; 3];
    }
    e.map(|v| (v * target / l).clamp(0.0, 1.0))
}

#[inline]
fn grain_noise(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (y as u32).wrapping_mul(0xd825_5f9d)
        ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    (h & 0x00ff_ffff) as f32 / 16_777_216.0 - 0.5
}

/// Render-time operator.
pub enum Prepared {
    /// Several adjustments applied in order in one pass over the pixels
    /// (the compositor fuses adjacent plain adjustment nodes).
    Chain(Vec<std::sync::Arc<Prepared>>),
    Lut(Box<[Vec<f32>; 3]>),
    HueSat {
        hue: f32,
        sat: f32,
        light: f32,
    },
    ColorBalance {
        s: [f32; 3],
        m: [f32; 3],
        h: [f32; 3],
        preserve: bool,
    },
    Vibrance {
        vib: f32,
        sat: f32,
    },
    BlackAndWhite {
        w: [f32; 6],
        tint: Option<([f32; 3], f32)>,
    },
    PhotoFilter {
        color: [f32; 3],
        density: f32,
        preserve: bool,
    },
    GradientMap(Vec<[f32; 3]>),
    Grain {
        amount: f32,
        size: f32,
        mono: bool,
    },
    /// Darken (or lighten, negative amount) towards the corners.
    Vignette {
        amount: f32,
        midpoint: f32,
        feather: f32,
        roundness: f32,
    },
    Threshold(f32),
    Cube {
        cube: Cube,
        strength: f32,
    },
}

impl Prepared {
    /// Whether the result depends on the pixel position (grain, vignette).
    pub fn positional(&self) -> bool {
        match self {
            Prepared::Chain(ops) => ops.iter().any(|o| o.positional()),
            _ => matches!(self, Prepared::Grain { .. } | Prepared::Vignette { .. }),
        }
    }

    /// Apply to unpremultiplied linear RGB.
    #[inline]
    pub fn apply(&self, c: [f32; 3]) -> [f32; 3] {
        self.apply_at(c, 0, 0, 1, 1)
    }

    /// Apply to unpremultiplied linear RGB at document pixel (x, y).
    pub fn apply_at(&self, c: [f32; 3], x: i32, y: i32, width: u32, height: u32) -> [f32; 3] {
        match self {
            Prepared::Chain(ops) => ops
                .iter()
                .fold(c, |c, op| op.apply_at(c, x, y, width, height)),
            Prepared::Vignette {
                amount,
                midpoint,
                feather,
                roundness,
            } => {
                let (fw, fh) = (width.max(1) as f32, height.max(1) as f32);
                let nx = (x as f32 + 0.5) / fw * 2.0 - 1.0;
                let ny = (y as f32 + 0.5) / fh * 2.0 - 1.0;
                // Elliptical (follows the frame) blended towards circular.
                let r_e = (nx * nx + ny * ny).sqrt() / std::f32::consts::SQRT_2;
                let short = fw.min(fh);
                let r_c = ((nx * fw).powi(2) + (ny * fh).powi(2)).sqrt()
                    / short
                    / std::f32::consts::SQRT_2;
                let r = r_e + (r_c - r_e) * roundness;
                let t = ((r - midpoint) / feather).clamp(0.0, 1.0);
                let k = t * t * (3.0 - 2.0 * t);
                let f = if *amount >= 0.0 {
                    1.0 - amount * k * 0.92
                } else {
                    1.0 - amount * k * 0.6
                };
                c.map(|v| (v * f).clamp(0.0, 1.0))
            }
            Prepared::Lut(t) => [lut(&t[0], c[0]), lut(&t[1], c[1]), lut(&t[2], c[2])],
            Prepared::HueSat { hue, sat, light } => {
                let e = enc(c);
                let (mut h, mut s, mut l) = rgb_to_hsl(e);
                h = (h + hue).rem_euclid(1.0);
                s = if *sat >= 0.0 {
                    s + (1.0 - s) * sat * s.max(0.0001).sqrt().min(1.0)
                } else {
                    s * (1.0 + sat)
                };
                l = if *light >= 0.0 {
                    l + (1.0 - l) * light
                } else {
                    l * (1.0 + light)
                };
                dec(hsl_to_rgb(h, s.clamp(0.0, 1.0), l.clamp(0.0, 1.0)))
            }
            Prepared::ColorBalance { s, m, h, preserve } => {
                let e = enc(c);
                let l = luma_enc(e);
                // Tonal weights: shadows fade out by mid grey, highlights fade in.
                let ws = (1.0 - l * 2.0).clamp(0.0, 1.0).powi(2);
                let wh = (l * 2.0 - 1.0).clamp(0.0, 1.0).powi(2);
                let wm = 1.0 - ws - wh;
                let mut o = [0.0; 3];
                for i in 0..3 {
                    let shift = (s[i] * ws + m[i] * wm + h[i] * wh) * 0.35;
                    o[i] = (e[i] + shift).clamp(0.0, 1.0);
                }
                dec(if *preserve { keep_luma(o, l) } else { o })
            }
            Prepared::Vibrance { vib, sat } => {
                let e = enc(c);
                let mx = e[0].max(e[1]).max(e[2]);
                let mn = e[0].min(e[1]).min(e[2]);
                let s_now = if mx > 1e-6 { (mx - mn) / mx } else { 0.0 };
                // Vibrance lifts the least saturated colours most; skin tones
                // (orange hues) get half the effect.
                let (hh, ..) = rgb_to_hsl(e);
                let skin = if (0.02..0.12).contains(&hh) { 0.5 } else { 1.0 };
                let k = 1.0 + vib * (1.0 - s_now) * skin + sat;
                let l = luma_enc(e);
                let o = e.map(|v| (l + (v - l) * k.max(0.0)).clamp(0.0, 1.0));
                dec(o)
            }
            Prepared::BlackAndWhite { w, tint } => {
                let e = enc(c);
                let mx = e[0].max(e[1]).max(e[2]);
                let mn = e[0].min(e[1]).min(e[2]);
                let gray = if mx - mn < 1e-6 {
                    mx
                } else {
                    // Weight by hue sector, interpolating between the six sliders.
                    let (h, ..) = rgb_to_hsl(e);
                    let sector = h * 6.0;
                    let i = (sector.floor() as usize) % 6;
                    let f = sector - sector.floor();
                    let weight = w[i] * (1.0 - f) + w[(i + 1) % 6] * f;
                    (mn + (mx - mn) * weight).clamp(0.0, 1.0)
                };
                let o = match tint {
                    Some((col, k)) => [0, 1, 2]
                        .map(|i| gray + (gray * col[i] - gray) * k * 0.8 + (col[i] - 1.0) * 0.0),
                    None => [gray; 3],
                };
                dec(o.map(|v| v.clamp(0.0, 1.0)))
            }
            Prepared::PhotoFilter {
                color,
                density,
                preserve,
            } => {
                let e = enc(c);
                let l = luma_enc(e);
                let o = [0, 1, 2].map(|i| e[i] * (1.0 - density + density * color[i]));
                dec(if *preserve { keep_luma(o, l) } else { o })
            }
            Prepared::GradientMap(table) => {
                let l = luma_enc(enc(c));
                let i = ((l * 255.0).round() as usize).min(255);
                dec(table[i])
            }
            Prepared::Grain { amount, size, mono } => {
                let e = enc(c);
                let (gx, gy) = (
                    (x as f32 / size).floor() as i32,
                    (y as f32 / size).floor() as i32,
                );
                let l = luma_enc(e);
                // Grain shows most in the midtones, like film.
                let vis = 1.0 - (2.0 * l - 1.0).powi(2) * 0.6;
                let o = if *mono {
                    let n = grain_noise(gx, gy, 11) * amount * 0.5 * vis;
                    e.map(|v| (v + n).clamp(0.0, 1.0))
                } else {
                    [0, 1, 2].map(|i| {
                        (e[i] + grain_noise(gx, gy, 11 + i as u32) * amount * 0.5 * vis)
                            .clamp(0.0, 1.0)
                    })
                };
                dec(o)
            }
            Prepared::Threshold(level) => {
                let l = luma_enc(enc(c));
                if l >= *level { [1.0; 3] } else { [0.0; 3] }
            }
            Prepared::Cube { cube, strength } => {
                let e = enc(c);
                let o = cube.sample(e);
                dec([0, 1, 2].map(|i| e[i] + (o[i] - e[i]) * strength))
            }
        }
    }
}

/// Per-channel and luma histograms of encoded sRGB, 256 bins each.
#[derive(Clone, Debug, PartialEq)]
pub struct Histogram {
    pub r: [u32; 256],
    pub g: [u32; 256],
    pub b: [u32; 256],
    pub luma: [u32; 256],
    /// Pixels counted (alpha above zero).
    pub count: u64,
}

impl Histogram {
    /// From premultiplied linear pixels; transparent pixels are skipped.
    pub fn of(pixels: &[[f32; 4]]) -> Histogram {
        let mut h = Histogram {
            r: [0; 256],
            g: [0; 256],
            b: [0; 256],
            luma: [0; 256],
            count: 0,
        };
        for p in pixels {
            if p[3] <= 0.001 {
                continue;
            }
            let inv = 1.0 / p[3];
            let e = enc([p[0] * inv, p[1] * inv, p[2] * inv]);
            let bin = |v: f32| ((v * 255.0).round() as usize).min(255);
            h.r[bin(e[0])] += 1;
            h.g[bin(e[1])] += 1;
            h.b[bin(e[2])] += 1;
            h.luma[bin(luma_enc(e))] += 1;
            h.count += 1;
        }
        h
    }

    /// Bin heights scaled to 0–1 by the largest bin, for drawing.
    pub fn normalized(bins: &[u32; 256]) -> [f32; 256] {
        let mx = bins.iter().copied().max().unwrap_or(1).max(1) as f32;
        let mut out = [0.0; 256];
        for (o, b) in out.iter_mut().zip(bins) {
            *o = *b as f32 / mx;
        }
        out
    }
}

fn rgb_to_hsl(c: [f32; 3]) -> (f32, f32, f32) {
    let mx = c[0].max(c[1]).max(c[2]);
    let mn = c[0].min(c[1]).min(c[2]);
    let l = (mx + mn) / 2.0;
    if mx - mn < 1e-6 {
        return (0.0, 0.0, l);
    }
    let d = mx - mn;
    let s = if l > 0.5 {
        d / (2.0 - mx - mn)
    } else {
        d / (mx + mn)
    };
    let h = if mx == c[0] {
        (c[1] - c[2]) / d + if c[1] < c[2] { 6.0 } else { 0.0 }
    } else if mx == c[1] {
        (c[2] - c[0]) / d + 2.0
    } else {
        (c[0] - c[1]) / d + 4.0
    };
    (h / 6.0, s, l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    if s <= 0.0 {
        return [l; 3];
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let f = |mut t: f32| {
        t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [f(h + 1.0 / 3.0), f(h), f(h - 1.0 / 3.0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 3], b: [f32; 3], tol: f32) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() <= tol)
    }

    #[test]
    fn neutral_parameters_are_identity() {
        let samples = [
            [0.0, 0.0, 0.0],
            [0.18, 0.5, 0.9],
            [1.0, 1.0, 1.0],
            [0.01, 0.02, 0.03],
        ];
        for adj in Adjustment::catalogue() {
            // These are not identities by nature.
            if matches!(
                adj,
                Adjustment::Invert
                    | Adjustment::BlackAndWhite { .. }
                    | Adjustment::GradientMap { .. }
                    | Adjustment::Threshold { .. }
            ) {
                continue;
            }
            let p = adj.prepare();
            for c in samples {
                assert!(
                    close(p.apply(c), c, 2e-3),
                    "{} not neutral at {c:?}: {:?}",
                    adj.label(),
                    p.apply(c)
                );
            }
        }
    }

    #[test]
    fn exposure_one_stop_doubles() {
        let p = Adjustment::Exposure {
            exposure: 1.0,
            offset: 0.0,
            gamma: 1.0,
        }
        .prepare();
        assert!(close(p.apply([0.2, 0.1, 0.05]), [0.4, 0.2, 0.1], 1e-3));
    }

    #[test]
    fn invert_twice_is_identity() {
        let p = Adjustment::Invert.prepare();
        let c = [0.3, 0.6, 0.05];
        assert!(close(p.apply(p.apply(c)), c, 2e-3));
    }

    #[test]
    fn hue_180_swaps_red_to_cyan() {
        let p = Adjustment::HueSaturation {
            hue: 180.0,
            saturation: 0.0,
            lightness: 0.0,
        }
        .prepare();
        let o = p.apply([1.0, 0.0, 0.0]);
        assert!(close(o, [0.0, 1.0, 1.0], 1e-3), "{o:?}");
    }

    #[test]
    fn set_param_clamps_and_rejects_unknown() {
        let mut a = Adjustment::Exposure {
            exposure: 0.0,
            offset: 0.0,
            gamma: 1.0,
        };
        assert!(a.set_param("exposure", 99.0));
        assert_eq!(a.params()[0].value, 5.0);
        assert!(!a.set_param("nope", 1.0));
        let mut g = Adjustment::Grain {
            amount: 0.0,
            size: 1.0,
            monochrome: false,
        };
        assert!(g.set_param("monochrome", 1.0));
        assert_eq!(
            g,
            Adjustment::Grain {
                amount: 0.0,
                size: 1.0,
                monochrome: true
            }
        );
    }

    #[test]
    fn curves_interpolate_monotonically_and_pass_through_points() {
        let pts = vec![[0.0, 0.0], [64.0, 32.0], [128.0, 160.0], [255.0, 255.0]];
        for p in &pts {
            assert!((curve_at(&pts, p[0]) - p[1]).abs() < 1e-3);
        }
        let mut last = 0.0;
        for x in 0..=255 {
            let y = curve_at(&pts, x as f32);
            assert!(y >= last - 1e-4, "curve dips at {x}");
            last = y;
        }
        assert_eq!(curve_at(&straight_curve(), 100.0), 100.0);
        let s_curve = Adjustment::Curves {
            master: vec![[0.0, 0.0], [64.0, 48.0], [192.0, 208.0], [255.0, 255.0]],
            red: straight_curve(),
            green: straight_curve(),
            blue: straight_curve(),
        }
        .prepare();
        let dark = s_curve.apply([0.05; 3])[0];
        let light = s_curve.apply([0.6; 3])[0];
        assert!(dark < 0.05 && light > 0.6, "contrast: {dark} {light}");
    }

    #[test]
    fn curves_preserve_empty_singleton_and_endpoint_behavior() {
        assert_eq!(curve_at(&[], -3.0), -3.0);
        assert_eq!(curve_at(&[[10.0, 42.0]], 200.0), 42.0);
        let points = [[10.0, -20.0], [20.0, 300.0]];
        assert_eq!(curve_at(&points, 0.0), -20.0);
        assert_eq!(curve_at(&points, 30.0), 300.0);
        let repeated = [[0.0, 0.0], [64.0, 30.0], [64.0, 60.0], [255.0, 255.0]];
        assert_eq!(curve_at(&repeated, 64.0), 60.0);
    }

    #[test]
    fn curves_preserve_turning_points_and_plateaus_exactly() {
        let points = [
            [0.0, 255.0],
            [80.0, 20.0],
            [120.0, 20.0],
            [200.0, 230.0],
            [255.0, 0.0],
        ];
        // Bit patterns captured from the original full-tangent interpolation.
        for (x, expected) in [
            (0.0, 1132396544),
            (1.0, 1132201655),
            (40.0, 1121468416),
            (79.0, 1101043068),
            (80.0, 1101004800),
            (100.0, 1101004800),
            (120.0, 1101004800),
            (160.0, 1123680256),
            (200.0, 1130758144),
            (230.0, 1124235072),
            (255.0, 0),
        ] {
            assert_eq!(curve_at(&points, x).to_bits(), expected, "at {x}");
        }
    }

    #[test]
    fn colour_balance_vibrance_bw_filter_grain_threshold() {
        let warm = Adjustment::ColorBalance {
            shadows: [0.0; 3],
            midtones: [60.0, 0.0, -40.0],
            highlights: [0.0; 3],
            preserve_luminosity: true,
        }
        .prepare();
        let o = warm.apply([0.2; 3]);
        assert!(o[0] > o[2], "red up, blue down: {o:?}");
        let vib = Adjustment::Vibrance {
            vibrance: 100.0,
            saturation: 0.0,
        }
        .prepare();
        let o = vib.apply([0.3, 0.25, 0.2]);
        assert!(o[0] - o[2] > 0.1, "more saturated: {o:?}");
        let bw = Adjustment::BlackAndWhite {
            reds: 300.0,
            yellows: 60.0,
            greens: 40.0,
            cyans: 60.0,
            blues: -200.0,
            magentas: 80.0,
            tint_hue: 0.0,
            tint_strength: 0.0,
        }
        .prepare();
        let r = bw.apply([1.0, 0.0, 0.0]);
        let b = bw.apply([0.0, 0.0, 1.0]);
        assert!(
            r[0] == r[1] && r[1] == r[2] && r[0] > 0.9 && b[0] < 0.01,
            "{r:?} {b:?}"
        );
        let pf = Adjustment::PhotoFilter {
            hue: 30.0,
            saturation: 100.0,
            density: 50.0,
            preserve_luminosity: false,
        }
        .prepare();
        let o = pf.apply([0.5; 3]);
        assert!(o[0] > o[2], "warming: {o:?}");
        let g = Adjustment::Grain {
            amount: 50.0,
            size: 1.0,
            monochrome: true,
        }
        .prepare();
        assert!(g.positional());
        let a = g.apply_at([0.4; 3], 3, 7, 100, 100);
        let b2 = g.apply_at([0.4; 3], 40, 9, 100, 100);
        let v = Adjustment::Vignette {
            amount: 60.0,
            midpoint: 30.0,
            feather: 60.0,
            roundness: 0.0,
        }
        .prepare();
        let centre = v.apply_at([0.5; 3], 50, 50, 100, 100)[0];
        let corner = v.apply_at([0.5; 3], 2, 2, 100, 100)[0];
        assert!(
            (centre - 0.5).abs() < 1e-3 && corner < 0.3,
            "{centre} {corner}"
        );
        assert!(a != b2 && a[0] == a[1], "mono grain varies by position");
        let th = Adjustment::Threshold { level: 128.0 }.prepare();
        assert_eq!(th.apply([0.9; 3]), [1.0; 3]);
        assert_eq!(th.apply([0.05; 3]), [0.0; 3]);
        let post = Adjustment::Posterize { levels: 2.0 }.prepare();
        assert!(post.apply([0.1; 3])[0] < 0.01 && post.apply([0.6; 3])[0] > 0.99);
        let gm = Adjustment::GradientMap {
            stops: vec![
                Stop {
                    pos: 0.0,
                    color: [0, 0, 255],
                },
                Stop {
                    pos: 1.0,
                    color: [255, 255, 0],
                },
            ],
            reverse: false,
        }
        .prepare();
        assert!(gm.apply([0.0; 3])[2] > 0.99 && gm.apply([1.0; 3])[0] > 0.99);
    }

    #[test]
    fn cube_parses_and_identity_lut_is_neutral() {
        let mut text = String::from("TITLE \"ident\"\nLUT_3D_SIZE 3\n");
        for b in 0..3 {
            for g in 0..3 {
                for r in 0..3 {
                    text.push_str(&format!(
                        "{} {} {}\n",
                        r as f32 / 2.0,
                        g as f32 / 2.0,
                        b as f32 / 2.0
                    ));
                }
            }
        }
        let cube = Cube::parse(&text).unwrap();
        assert_eq!((cube.name.as_str(), cube.size), ("ident", 3));
        let p = Adjustment::Lut3D {
            cube,
            strength: 100.0,
        }
        .prepare();
        for c in [[0.1, 0.5, 0.9], [0.0; 3], [1.0; 3]] {
            assert!(close(p.apply(c), c, 3e-3), "{:?}", p.apply(c));
        }
        assert!(Cube::parse("LUT_3D_SIZE 2\n0 0 0\n").is_err());
    }

    #[test]
    fn auto_levels_stretches_the_histogram() {
        let px: Vec<[f32; 4]> = (0..1000)
            .map(|i| {
                let e = 0.25 + 0.5 * (i as f32 / 999.0);
                let l = srgb_to_linear(e);
                [l, l, l, 1.0]
            })
            .collect();
        let h = Histogram::of(&px);
        assert_eq!(h.count, 1000);
        let Adjustment::Levels {
            in_black, in_white, ..
        } = Adjustment::auto_levels(&h, 0.1)
        else {
            panic!()
        };
        assert!(
            (in_black - 64.0).abs() < 3.0 && (in_white - 191.0).abs() < 3.0,
            "{in_black} {in_white}"
        );
    }
}
