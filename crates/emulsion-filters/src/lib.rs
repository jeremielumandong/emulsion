//! `emulsion-filters` — pixel filters: blurs, sharpening, noise, high pass,
//! lens correction, and a few distort and stylize effects.
//!
//! A [`Filter`] is plain data with named parameters, like an adjustment.
//! [`apply_stack`] runs a stack over a layer's pixels and returns a new
//! raster that may be larger than the source, because blurs spread past
//! the layer's edges; the offset says where the result sits relative to
//! the source. Smart layers keep the source and the stack and re-run this
//! when a parameter changes.

use emulsion_raster::blend::{BlendSpace, blend_px};
use emulsion_raster::{BlendMode, IRect, Raster, color};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};

pub const CRATE: &str = "emulsion-filters";

/// The most a filter may spread past the layer, in pixels.
pub const MAX_SPREAD: i32 = 250;

/// Optional image accelerator. Pixels are premultiplied, linear RGBA; filters
/// must preserve their dimensions. Returning `None` uses the CPU implementation.
pub trait FilterAccelerator: Send + Sync {
    fn apply(
        &self,
        filter: &Filter,
        width: usize,
        height: usize,
        pixels: &[[f32; 4]],
    ) -> Option<Vec<[f32; 4]>>;
}

static ACCELERATOR: OnceLock<Arc<dyn FilterAccelerator>> = OnceLock::new();

/// Install the application's accelerator once. CPU-only applications need not
/// call this; unsupported filters and failed GPU dispatches remain CPU-backed.
pub fn install_accelerator(accelerator: Arc<dyn FilterAccelerator>) {
    let _ = ACCELERATOR.set(accelerator);
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Filter {
    GaussianBlur {
        radius: f32,
    },
    BoxBlur {
        radius: f32,
    },
    MotionBlur {
        angle: f32,
        distance: f32,
    },
    /// Disk-shaped blur, like a lens out of focus.
    LensBlur {
        radius: f32,
    },
    UnsharpMask {
        amount: f32,
        radius: f32,
        threshold: f32,
    },
    SmartSharpen {
        amount: f32,
        radius: f32,
    },
    AddNoise {
        amount: f32,
        monochrome: bool,
    },
    ReduceNoise {
        strength: f32,
        detail: f32,
    },
    HighPass {
        radius: f32,
    },
    /// Barrel (+) or pincushion (−) distortion and corner darkening.
    LensCorrection {
        distortion: f32,
        vignette: f32,
    },
    /// A measured lens profile (lensfun): PanoTools distortion a, b, c;
    /// vignetting k1..k3; `scale` rescales the radius between the sensor
    /// the lens was calibrated on and this picture's. Radius 1 is half the
    /// shorter side. Each part has its own strength in percent.
    LensProfile {
        a: f32,
        b: f32,
        c: f32,
        k1: f32,
        k2: f32,
        k3: f32,
        scale: f32,
        distortion: f32,
        vignette: f32,
    },
    Emboss {
        angle: f32,
        height: f32,
        amount: f32,
    },
    FindEdges,
    Pinch {
        amount: f32,
    },
    Twirl {
        angle: f32,
    },
    Wave {
        amplitude: f32,
        wavelength: f32,
    },
    /// One-slider automatic enhancement: shadow lift, highlight recovery, a
    /// gentle S-curve, vibrance that spares skin, and mild local contrast,
    /// all adapted to the image's luma percentiles. `sky` deepens bright
    /// blue-cyan areas, more strongly toward the top of the frame.
    Enhance {
        amount: f32,
        sky: f32,
    },
    /// Large-radius local contrast on luminance only. The radius is 1–3% of
    /// the shorter side (growing with `softness`), so the look is the same at
    /// any resolution. Negative amounts soften.
    Structure {
        amount: f32,
        softness: f32,
    },
    /// Blurred bright areas screened back over the image. `radius` is a
    /// percentage of 5% of the shorter side; `threshold` is the perceptual
    /// brightness where the glow starts (with a soft knee).
    Glow {
        amount: f32,
        radius: f32,
        threshold: f32,
    },
    /// The Orton effect ("Mystical"): a brightened, blurred copy mixed back
    /// with multiply and screen. `radius` is a percentage of 8% of the
    /// shorter side.
    Orton {
        amount: f32,
        radius: f32,
    },
    /// Light rays from a sun at (`x`, `y`) percent of the width and height,
    /// streaking out of bright areas, plus a warm glow at the sun. `length`
    /// scales with the image, so rays look the same at any resolution.
    Sunrays {
        x: f32,
        y: f32,
        amount: f32,
        length: f32,
        warmth: f32,
    },
    /// Positive adds haze toward the estimated airlight, strongest at the
    /// top (`spread` sets how far down); negative removes haze with the
    /// dark-channel prior.
    Atmosphere {
        amount: f32,
        spread: f32,
    },
    /// Edge-preserving smoothing inside a soft skin-tone mask. `radius` maps
    /// to 0.2–1.5% of the shorter side; `detail` keeps that share of the
    /// fine texture.
    SkinSmooth {
        amount: f32,
        radius: f32,
        detail: f32,
    },
    /// Warm, low-sun toning weighted by luminance.
    GoldenHour {
        amount: f32,
    },
    /// Strong large-radius local contrast (2% of the shorter side), grit,
    /// deeper shadows and partial desaturation.
    Dramatic {
        amount: f32,
    },
}

/// Blending options for one editable smart filter.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FilterStyle {
    /// Filter result opacity, from 0 to 1.
    pub opacity: f32,
    /// How the filter result blends with the pixels entering this stage.
    pub blend: BlendMode,
}

impl Default for FilterStyle {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            blend: BlendMode::Normal,
        }
    }
}

impl FilterStyle {
    pub fn sanitized(mut self) -> Self {
        self.opacity = if self.opacity.is_finite() {
            self.opacity.clamp(0.0, 1.0)
        } else {
            1.0
        };
        if self.blend == BlendMode::PassThrough {
            self.blend = BlendMode::Normal;
        }
        self
    }
}

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
            "°" => format!("{:.0}°", self.value),
            "on" => if self.value >= 0.5 { "on" } else { "off" }.into(),
            "" if self.step < 1.0 => format!("{:.1}", self.value),
            u => format!("{:.0}{u}", self.value),
        }
    }
}

fn b2f(b: bool) -> f32 {
    if b { 1.0 } else { 0.0 }
}

impl Filter {
    pub fn catalogue() -> Vec<Filter> {
        vec![
            Filter::GaussianBlur { radius: 5.0 },
            Filter::BoxBlur { radius: 5.0 },
            Filter::MotionBlur {
                angle: 0.0,
                distance: 20.0,
            },
            Filter::LensBlur { radius: 8.0 },
            Filter::UnsharpMask {
                amount: 100.0,
                radius: 1.5,
                threshold: 0.0,
            },
            Filter::SmartSharpen {
                amount: 80.0,
                radius: 1.0,
            },
            Filter::AddNoise {
                amount: 10.0,
                monochrome: true,
            },
            Filter::ReduceNoise {
                strength: 5.0,
                detail: 50.0,
            },
            Filter::HighPass { radius: 10.0 },
            Filter::LensCorrection {
                distortion: 0.0,
                vignette: 0.0,
            },
            Filter::LensProfile {
                a: 0.0,
                b: 0.0,
                c: 0.0,
                k1: 0.0,
                k2: 0.0,
                k3: 0.0,
                scale: 1.0,
                distortion: 100.0,
                vignette: 100.0,
            },
            Filter::Emboss {
                angle: 135.0,
                height: 3.0,
                amount: 100.0,
            },
            Filter::FindEdges,
            Filter::Pinch { amount: 50.0 },
            Filter::Twirl { angle: 90.0 },
            Filter::Wave {
                amplitude: 10.0,
                wavelength: 60.0,
            },
            Filter::Enhance {
                amount: 50.0,
                sky: 0.0,
            },
            Filter::Structure {
                amount: 40.0,
                softness: 30.0,
            },
            Filter::Glow {
                amount: 40.0,
                radius: 20.0,
                threshold: 50.0,
            },
            Filter::Orton {
                amount: 40.0,
                radius: 15.0,
            },
            Filter::Sunrays {
                x: 70.0,
                y: 20.0,
                amount: 50.0,
                length: 60.0,
                warmth: 60.0,
            },
            Filter::Atmosphere {
                amount: 30.0,
                spread: 50.0,
            },
            Filter::SkinSmooth {
                amount: 50.0,
                radius: 30.0,
                detail: 30.0,
            },
            Filter::GoldenHour { amount: 50.0 },
            Filter::Dramatic { amount: 50.0 },
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Filter::GaussianBlur { .. } => "Gaussian blur",
            Filter::BoxBlur { .. } => "Box blur",
            Filter::MotionBlur { .. } => "Motion blur",
            Filter::LensBlur { .. } => "Lens blur",
            Filter::UnsharpMask { .. } => "Unsharp mask",
            Filter::SmartSharpen { .. } => "Smart sharpen",
            Filter::AddNoise { .. } => "Add noise",
            Filter::ReduceNoise { .. } => "Reduce noise",
            Filter::HighPass { .. } => "High pass",
            Filter::LensCorrection { .. } => "Lens correction",
            Filter::LensProfile { .. } => "Lens profile",
            Filter::Emboss { .. } => "Emboss",
            Filter::FindEdges => "Find edges",
            Filter::Pinch { .. } => "Pinch",
            Filter::Twirl { .. } => "Twirl",
            Filter::Wave { .. } => "Wave",
            Filter::Enhance { .. } => "Enhance",
            Filter::Structure { .. } => "Structure",
            Filter::Glow { .. } => "Glow",
            Filter::Orton { .. } => "Mystical",
            Filter::Sunrays { .. } => "Sunrays",
            Filter::Atmosphere { .. } => "Atmosphere",
            Filter::SkinSmooth { .. } => "Skin smoothing",
            Filter::GoldenHour { .. } => "Golden hour",
            Filter::Dramatic { .. } => "Dramatic",
        }
    }

    pub fn key(&self) -> &'static str {
        match self {
            Filter::GaussianBlur { .. } => "gaussian_blur",
            Filter::BoxBlur { .. } => "box_blur",
            Filter::MotionBlur { .. } => "motion_blur",
            Filter::LensBlur { .. } => "lens_blur",
            Filter::UnsharpMask { .. } => "unsharp_mask",
            Filter::SmartSharpen { .. } => "smart_sharpen",
            Filter::AddNoise { .. } => "add_noise",
            Filter::ReduceNoise { .. } => "reduce_noise",
            Filter::HighPass { .. } => "high_pass",
            Filter::LensCorrection { .. } => "lens_correction",
            Filter::LensProfile { .. } => "lens_profile",
            Filter::Emboss { .. } => "emboss",
            Filter::FindEdges => "find_edges",
            Filter::Pinch { .. } => "pinch",
            Filter::Twirl { .. } => "twirl",
            Filter::Wave { .. } => "wave",
            Filter::Enhance { .. } => "enhance",
            Filter::Structure { .. } => "structure",
            Filter::Glow { .. } => "glow",
            Filter::Orton { .. } => "orton",
            Filter::Sunrays { .. } => "sunrays",
            Filter::Atmosphere { .. } => "atmosphere",
            Filter::SkinSmooth { .. } => "skin_smooth",
            Filter::GoldenHour { .. } => "golden_hour",
            Filter::Dramatic { .. } => "dramatic",
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
            Filter::GaussianBlur { radius } | Filter::BoxBlur { radius } => {
                vec![p("radius", "radius", 0.0, 100.0, 0.1, *radius, "px")]
            }
            Filter::LensBlur { radius } => {
                vec![p("radius", "radius", 0.0, 40.0, 0.5, *radius, "px")]
            }
            Filter::MotionBlur { angle, distance } => vec![
                p("angle", "angle", -180.0, 180.0, 1.0, *angle, "°"),
                p("distance", "distance", 0.0, 200.0, 1.0, *distance, "px"),
            ],
            Filter::UnsharpMask {
                amount,
                radius,
                threshold,
            } => vec![
                p("amount", "amount", 0.0, 500.0, 1.0, *amount, "%"),
                p("radius", "radius", 0.1, 50.0, 0.1, *radius, "px"),
                p("threshold", "threshold", 0.0, 255.0, 1.0, *threshold, ""),
            ],
            Filter::SmartSharpen { amount, radius } => vec![
                p("amount", "amount", 0.0, 500.0, 1.0, *amount, "%"),
                p("radius", "radius", 0.1, 20.0, 0.1, *radius, "px"),
            ],
            Filter::AddNoise { amount, monochrome } => vec![
                p("amount", "amount", 0.0, 100.0, 0.5, *amount, "%"),
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
            Filter::ReduceNoise { strength, detail } => vec![
                p("strength", "strength", 0.0, 10.0, 0.5, *strength, ""),
                p("detail", "preserve detail", 0.0, 100.0, 1.0, *detail, "%"),
            ],
            Filter::HighPass { radius } => {
                vec![p("radius", "radius", 0.1, 100.0, 0.1, *radius, "px")]
            }
            Filter::LensProfile {
                distortion,
                vignette,
                ..
            } => vec![
                p(
                    "distortion",
                    "distortion",
                    0.0,
                    150.0,
                    1.0,
                    *distortion,
                    "%",
                ),
                p("vignette", "vignette", 0.0, 150.0, 1.0, *vignette, "%"),
            ],
            Filter::LensCorrection {
                distortion,
                vignette,
            } => vec![
                p(
                    "distortion",
                    "distortion",
                    -100.0,
                    100.0,
                    1.0,
                    *distortion,
                    "",
                ),
                p("vignette", "vignette", -100.0, 100.0, 1.0, *vignette, ""),
            ],
            Filter::Emboss {
                angle,
                height,
                amount,
            } => vec![
                p("angle", "angle", -180.0, 180.0, 1.0, *angle, "°"),
                p("height", "height", 1.0, 20.0, 1.0, *height, "px"),
                p("amount", "amount", 1.0, 500.0, 1.0, *amount, "%"),
            ],
            Filter::FindEdges => vec![],
            Filter::Pinch { amount } => {
                vec![p("amount", "amount", -100.0, 100.0, 1.0, *amount, "%")]
            }
            Filter::Twirl { angle } => vec![p("angle", "angle", -999.0, 999.0, 1.0, *angle, "°")],
            Filter::Wave {
                amplitude,
                wavelength,
            } => vec![
                p("amplitude", "amplitude", 0.0, 200.0, 1.0, *amplitude, "px"),
                p(
                    "wavelength",
                    "wavelength",
                    2.0,
                    500.0,
                    1.0,
                    *wavelength,
                    "px",
                ),
            ],
            Filter::Enhance { amount, sky } => vec![
                p("amount", "amount", 0.0, 100.0, 1.0, *amount, "%"),
                p("sky", "sky", 0.0, 100.0, 1.0, *sky, "%"),
            ],
            Filter::Structure { amount, softness } => vec![
                p("amount", "amount", -100.0, 100.0, 1.0, *amount, "%"),
                p("softness", "softness", 0.0, 100.0, 1.0, *softness, "%"),
            ],
            Filter::Glow {
                amount,
                radius,
                threshold,
            } => vec![
                p("amount", "amount", 0.0, 100.0, 1.0, *amount, "%"),
                p("radius", "radius", 1.0, 100.0, 1.0, *radius, "%"),
                p("threshold", "threshold", 0.0, 100.0, 1.0, *threshold, "%"),
            ],
            Filter::Orton { amount, radius } => vec![
                p("amount", "amount", 0.0, 100.0, 1.0, *amount, "%"),
                p("radius", "radius", 1.0, 100.0, 1.0, *radius, "%"),
            ],
            Filter::Sunrays {
                x,
                y,
                amount,
                length,
                warmth,
            } => vec![
                p("x", "sun x", 0.0, 100.0, 1.0, *x, "%"),
                p("y", "sun y", 0.0, 100.0, 1.0, *y, "%"),
                p("amount", "amount", 0.0, 100.0, 1.0, *amount, "%"),
                p("length", "length", 0.0, 100.0, 1.0, *length, "%"),
                p("warmth", "warmth", 0.0, 100.0, 1.0, *warmth, "%"),
            ],
            Filter::Atmosphere { amount, spread } => vec![
                p("amount", "amount", -100.0, 100.0, 1.0, *amount, "%"),
                p("spread", "spread", 0.0, 100.0, 1.0, *spread, "%"),
            ],
            Filter::SkinSmooth {
                amount,
                radius,
                detail,
            } => vec![
                p("amount", "amount", 0.0, 100.0, 1.0, *amount, "%"),
                p("radius", "radius", 1.0, 100.0, 1.0, *radius, "%"),
                p("detail", "detail", 0.0, 100.0, 1.0, *detail, "%"),
            ],
            Filter::GoldenHour { amount } | Filter::Dramatic { amount } => {
                vec![p("amount", "amount", 0.0, 100.0, 1.0, *amount, "%")]
            }
        }
    }

    pub fn set_param(&mut self, key: &str, value: f32) -> bool {
        let Some(spec) = self.params().into_iter().find(|s| s.key == key) else {
            return false;
        };
        let v = value.clamp(spec.min, spec.max);
        let slot: &mut f32 = match (self, key) {
            (
                Filter::GaussianBlur { radius }
                | Filter::BoxBlur { radius }
                | Filter::LensBlur { radius }
                | Filter::HighPass { radius },
                "radius",
            ) => radius,
            (Filter::MotionBlur { angle, .. }, "angle") => angle,
            (Filter::MotionBlur { distance, .. }, "distance") => distance,
            (
                Filter::UnsharpMask { amount, .. } | Filter::SmartSharpen { amount, .. },
                "amount",
            ) => amount,
            (
                Filter::UnsharpMask { radius, .. } | Filter::SmartSharpen { radius, .. },
                "radius",
            ) => radius,
            (Filter::UnsharpMask { threshold, .. }, "threshold") => threshold,
            (Filter::AddNoise { amount, .. }, "amount") => amount,
            (Filter::AddNoise { monochrome, .. }, "monochrome") => {
                *monochrome = v >= 0.5;
                return true;
            }
            (Filter::ReduceNoise { strength, .. }, "strength") => strength,
            (Filter::ReduceNoise { detail, .. }, "detail") => detail,
            (Filter::LensCorrection { distortion, .. }, "distortion") => distortion,
            (Filter::LensCorrection { vignette, .. }, "vignette") => vignette,
            (Filter::LensProfile { distortion, .. }, "distortion") => distortion,
            (Filter::LensProfile { vignette, .. }, "vignette") => vignette,
            (Filter::LensProfile { a, .. }, "a") => a,
            (Filter::LensProfile { b, .. }, "b") => b,
            (Filter::LensProfile { c, .. }, "c") => c,
            (Filter::LensProfile { k1, .. }, "k1") => k1,
            (Filter::LensProfile { k2, .. }, "k2") => k2,
            (Filter::LensProfile { k3, .. }, "k3") => k3,
            (Filter::LensProfile { scale, .. }, "scale") => scale,
            (Filter::Emboss { angle, .. }, "angle") => angle,
            (Filter::Emboss { height, .. }, "height") => height,
            (Filter::Emboss { amount, .. }, "amount") => amount,
            (Filter::Pinch { amount }, "amount") => amount,
            (Filter::Twirl { angle }, "angle") => angle,
            (Filter::Wave { amplitude, .. }, "amplitude") => amplitude,
            (Filter::Wave { wavelength, .. }, "wavelength") => wavelength,
            (
                Filter::Enhance { amount, .. }
                | Filter::Structure { amount, .. }
                | Filter::Glow { amount, .. }
                | Filter::Orton { amount, .. }
                | Filter::Sunrays { amount, .. }
                | Filter::Atmosphere { amount, .. }
                | Filter::SkinSmooth { amount, .. }
                | Filter::GoldenHour { amount }
                | Filter::Dramatic { amount },
                "amount",
            ) => amount,
            (Filter::Enhance { sky, .. }, "sky") => sky,
            (Filter::Structure { softness, .. }, "softness") => softness,
            (
                Filter::Glow { radius, .. }
                | Filter::Orton { radius, .. }
                | Filter::SkinSmooth { radius, .. },
                "radius",
            ) => radius,
            (Filter::Glow { threshold, .. }, "threshold") => threshold,
            (Filter::Sunrays { x, .. }, "x") => x,
            (Filter::Sunrays { y, .. }, "y") => y,
            (Filter::Sunrays { length, .. }, "length") => length,
            (Filter::Sunrays { warmth, .. }, "warmth") => warmth,
            (Filter::Atmosphere { spread, .. }, "spread") => spread,
            (Filter::SkinSmooth { detail, .. }, "detail") => detail,
            _ => return false,
        };
        *slot = v;
        true
    }

    /// Clamp every parameter to its `ParamSpec` range and replace non-finite
    /// values with the catalogue default, so filters loaded from files or
    /// commands cannot ask the kernels for unbounded work. In-range values
    /// are left unchanged.
    pub fn sanitized(&self) -> Filter {
        let mut out = self.clone();
        let default = Filter::catalogue()
            .into_iter()
            .find(|d| d.key() == self.key());
        for spec in self.params() {
            let value = if spec.value.is_finite() {
                spec.value
            } else {
                default
                    .as_ref()
                    .and_then(|d| d.params().into_iter().find(|s| s.key == spec.key))
                    .map_or(spec.min, |s| s.value)
            };
            out.set_param(spec.key, value);
        }
        if let Filter::LensProfile {
            a,
            b,
            c,
            k1,
            k2,
            k3,
            scale,
            ..
        } = &mut out
        {
            for v in [a, b, c, k1, k2, k3] {
                if !v.is_finite() {
                    *v = 0.0;
                }
            }
            if !scale.is_finite() {
                *scale = 1.0;
            }
        }
        out
    }

    /// How far this filter can push pixels past the layer's edge.
    pub fn spread(&self) -> i32 {
        let s = match self {
            Filter::GaussianBlur { radius } => radius * 3.0,
            Filter::BoxBlur { radius } | Filter::LensBlur { radius } => *radius,
            Filter::MotionBlur { distance, .. } => distance / 2.0,
            Filter::UnsharpMask { radius, .. } | Filter::SmartSharpen { radius, .. } => {
                radius * 2.0
            }
            Filter::Emboss { height, .. } => *height,
            Filter::Wave { amplitude, .. } => *amplitude,
            Filter::Pinch { .. }
            | Filter::Twirl { .. }
            | Filter::LensCorrection { .. }
            | Filter::LensProfile { .. } => 0.0,
            // Photo looks stay inside the layer: their blurs renormalise at
            // the edges instead of spreading.
            Filter::Enhance { .. }
            | Filter::Structure { .. }
            | Filter::Glow { .. }
            | Filter::Orton { .. }
            | Filter::Sunrays { .. }
            | Filter::Atmosphere { .. }
            | Filter::SkinSmooth { .. }
            | Filter::GoldenHour { .. }
            | Filter::Dramatic { .. } => 0.0,
            _ => 0.0,
        };
        (s.ceil() as i32).clamp(0, MAX_SPREAD)
    }
}

/// Dense premultiplied linear image with its own origin.
#[derive(Clone)]
struct Image {
    w: usize,
    h: usize,
    px: Vec<[f32; 4]>,
}

impl Image {
    fn get(&self, x: i64, y: i64) -> [f32; 4] {
        if x < 0 || y < 0 || x >= self.w as i64 || y >= self.h as i64 {
            [0.0; 4]
        } else {
            self.px[y as usize * self.w + x as usize]
        }
    }

    fn sample(&self, x: f32, y: f32) -> [f32; 4] {
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (ix, iy) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - ix, fy - iy);
        let (ix, iy) = (ix as i64, iy as i64);
        let a = self.get(ix, iy);
        let b = self.get(ix + 1, iy);
        let c = self.get(ix, iy + 1);
        let d = self.get(ix + 1, iy + 1);
        [0, 1, 2, 3].map(|k| {
            (a[k] * (1.0 - tx) + b[k] * tx) * (1.0 - ty) + (c[k] * (1.0 - tx) + d[k] * tx) * ty
        })
    }

    fn pad(&self, n: usize) -> Image {
        if n == 0 {
            return Image {
                w: self.w,
                h: self.h,
                px: self.px.clone(),
            };
        }
        let (w, h) = (self.w + 2 * n, self.h + 2 * n);
        let mut px = vec![[0.0; 4]; w * h];
        for y in 0..self.h {
            px[(y + n) * w + n..(y + n) * w + n + self.w]
                .copy_from_slice(&self.px[y * self.w..(y + 1) * self.w]);
        }
        Image { w, h, px }
    }

    fn map(&self, f: impl Fn(usize, usize, [f32; 4]) -> [f32; 4] + Sync) -> Image {
        let w = self.w;
        let px: Vec<[f32; 4]> = self
            .px
            .par_iter()
            .enumerate()
            .map(|(i, p)| f(i % w, i / w, *p))
            .collect();
        Image { w, h: self.h, px }
    }

    /// Remap every output pixel from a source position.
    fn warp(&self, f: impl Fn(f32, f32) -> (f32, f32) + Sync) -> Image {
        let w = self.w;
        let px: Vec<[f32; 4]> = (0..self.w * self.h)
            .into_par_iter()
            .map(|i| {
                let (x, y) = ((i % w) as f32 + 0.5, (i / w) as f32 + 0.5);
                let (sx, sy) = f(x, y);
                self.sample(sx, sy)
            })
            .collect();
        Image { w, h: self.h, px }
    }
}

fn gaussian_kernel(radius: f32) -> Vec<f32> {
    let sigma = (radius / 2.0).max(0.3);
    let r = (sigma * 3.0).ceil() as i32;
    let mut k: Vec<f32> = (-r..=r)
        .map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let s: f32 = k.iter().sum();
    for v in &mut k {
        *v /= s;
    }
    k
}

fn convolve_1d(img: &Image, kernel: &[f32], horizontal: bool) -> Image {
    let (w, h) = (img.w, img.h);
    let mut px = vec![[0.0f32; 4]; w * h];
    convolve_1d_into(img, kernel, horizontal, &mut px);
    Image { w, h, px }
}

fn convolve_1d_into(img: &Image, kernel: &[f32], horizontal: bool, px: &mut [[f32; 4]]) {
    let r = kernel.len() / 2;
    let (w, h) = (img.w, img.h);
    if horizontal {
        px.par_chunks_mut(w).enumerate().for_each(|(y, out)| {
            let row = &img.px[y * w..(y + 1) * w];
            for (x, o) in out.iter_mut().enumerate() {
                let lo = x.saturating_sub(r);
                let hi = (x + r).min(w - 1);
                let mut acc = [0.0f32; 4];
                for xx in lo..=hi {
                    let wgt = kernel[xx + r - x];
                    let p = row[xx];
                    acc[0] += p[0] * wgt;
                    acc[1] += p[1] * wgt;
                    acc[2] += p[2] * wgt;
                    acc[3] += p[3] * wgt;
                }
                *o = acc;
            }
        });
    } else {
        px.par_chunks_mut(w).enumerate().for_each(|(y, out)| {
            let lo = y.saturating_sub(r);
            let hi = (y + r).min(h - 1);
            for yy in lo..=hi {
                let wgt = kernel[yy + r - y];
                let row = &img.px[yy * w..(yy + 1) * w];
                for (o, p) in out.iter_mut().zip(row) {
                    o[0] += p[0] * wgt;
                    o[1] += p[1] * wgt;
                    o[2] += p[2] * wgt;
                    o[3] += p[3] * wgt;
                }
            }
        });
    }
}

fn gaussian(img: &Image, radius: f32) -> Image {
    if radius <= 0.05 {
        return img.pad(0);
    }
    let k = gaussian_kernel(radius);
    convolve_1d(&convolve_1d(img, &k, true), &k, false)
}

fn box_blur(img: &Image, radius: f32) -> Image {
    let r = radius.round() as usize;
    if r == 0 {
        return img.pad(0);
    }
    let k = vec![1.0 / (2 * r + 1) as f32; 2 * r + 1];
    convolve_1d(&convolve_1d(img, &k, true), &k, false)
}

fn luma(p: [f32; 4]) -> f32 {
    if p[3] <= 1e-6 {
        0.0
    } else {
        color::luma(p[0] / p[3], p[1] / p[3], p[2] / p[3])
    }
}

#[inline]
fn hash(x: usize, y: usize, seed: u32) -> f32 {
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

/// Encoded-space operation on unpremultiplied colour, alpha kept.
fn on_color(p: [f32; 4], f: impl Fn([f32; 3]) -> [f32; 3]) -> [f32; 4] {
    if p[3] <= 1e-6 {
        return p;
    }
    let c = f([p[0] / p[3], p[1] / p[3], p[2] / p[3]]);
    [
        c[0].clamp(0.0, 1.0) * p[3],
        c[1].clamp(0.0, 1.0) * p[3],
        c[2].clamp(0.0, 1.0) * p[3],
        p[3],
    ]
}

fn apply_one(f: &Filter, img: Image) -> Image {
    if let Some(px) = ACCELERATOR
        .get()
        .and_then(|accelerator| accelerator.apply(f, img.w, img.h, &img.px))
        .filter(|px| px.len() == img.px.len() && px.iter().flatten().all(|v| v.is_finite()))
    {
        return Image {
            w: img.w,
            h: img.h,
            px,
        };
    }
    apply_one_cpu_owned(f, img)
}

fn apply_one_cpu_owned(f: &Filter, mut img: Image) -> Image {
    if let Filter::GaussianBlur { radius } = f {
        if *radius <= 0.05 {
            return img;
        }
        let kernel = gaussian_kernel(*radius);
        let horizontal = convolve_1d(&img, &kernel, true);
        // The horizontal pass has consumed the input; reuse its storage
        // for the vertical result instead of keeping three full images alive.
        img.px.par_iter_mut().for_each(|p| *p = [0.0; 4]);
        convolve_1d_into(&horizontal, &kernel, false, &mut img.px);
        img
    } else {
        apply_one_cpu(f, &img)
    }
}

/// Run the reference CPU kernel on already padded premultiplied linear pixels.
/// This bypasses installed accelerators for numerical and performance checks.
#[doc(hidden)]
pub fn apply_pixels_cpu(
    filter: &Filter,
    width: usize,
    height: usize,
    pixels: Vec<[f32; 4]>,
) -> Option<Vec<[f32; 4]>> {
    if width == 0 || height == 0 || width.checked_mul(height)? != pixels.len() {
        return None;
    }
    Some(
        apply_one_cpu_owned(
            &filter.sanitized(),
            Image {
                w: width,
                h: height,
                px: pixels,
            },
        )
        .px,
    )
}

fn apply_one_cpu(f: &Filter, img: &Image) -> Image {
    match f {
        Filter::GaussianBlur { radius } => gaussian(img, *radius),
        Filter::BoxBlur { radius } => box_blur(img, *radius),
        Filter::MotionBlur { angle, distance } => {
            let n = (distance.round() as i64).max(1);
            let (s, c) = angle.to_radians().sin_cos();
            let w = img.w;
            let px: Vec<[f32; 4]> = (0..img.w * img.h)
                .into_par_iter()
                .map(|i| {
                    let (x, y) = ((i % w) as f32 + 0.5, (i / w) as f32 + 0.5);
                    let mut acc = [0.0f32; 4];
                    for k in 0..n {
                        let t = k as f32 - (n - 1) as f32 / 2.0;
                        let p = img.sample(x + c * t, y - s * t);
                        for ch in 0..4 {
                            acc[ch] += p[ch] / n as f32;
                        }
                    }
                    acc
                })
                .collect();
            Image { w, h: img.h, px }
        }
        Filter::LensBlur { radius } => {
            let r = radius.round().max(0.0) as i64;
            if r == 0 {
                return img.pad(0);
            }
            let offsets: Vec<(i64, i64)> = (-r..=r)
                .flat_map(|dy| (-r..=r).map(move |dx| (dx, dy)))
                .filter(|(dx, dy)| dx * dx + dy * dy <= r * r)
                .collect();
            let inv = 1.0 / offsets.len() as f32;
            let w = img.w;
            let px: Vec<[f32; 4]> = (0..img.w * img.h)
                .into_par_iter()
                .map(|i| {
                    let (x, y) = ((i % w) as i64, (i / w) as i64);
                    let mut acc = [0.0f32; 4];
                    for (dx, dy) in &offsets {
                        let p = img.get(x + dx, y + dy);
                        for ch in 0..4 {
                            acc[ch] += p[ch] * inv;
                        }
                    }
                    acc
                })
                .collect();
            Image { w, h: img.h, px }
        }
        Filter::UnsharpMask { amount, radius, .. } | Filter::SmartSharpen { amount, radius } => {
            let threshold = if let Filter::UnsharpMask { threshold, .. } = f {
                *threshold / 255.0
            } else {
                0.0
            };
            let soft = matches!(f, Filter::SmartSharpen { .. });
            let blur = gaussian(img, *radius);
            let k = amount / 100.0;
            let w = img.w;
            let px: Vec<[f32; 4]> = img
                .px
                .par_iter()
                .zip(&blur.px)
                .enumerate()
                .map(|(_, (p, b))| {
                    if p[3] <= 1e-6 {
                        return *p;
                    }
                    let mut o = *p;
                    let d_l = (luma(*p) - luma(*b)).abs();
                    if d_l < threshold {
                        return *p;
                    }
                    for ch in 0..3 {
                        let d = p[ch] - b[ch] * (p[3] / b[3].max(1e-6));
                        // Smart sharpen tames the halo on strong edges.
                        let gain = if soft { k / (1.0 + d.abs() * 6.0) } else { k };
                        o[ch] = (p[ch] + d * gain).clamp(0.0, p[3]);
                    }
                    o
                })
                .collect();
            let _ = w;
            Image {
                w: img.w,
                h: img.h,
                px,
            }
        }
        Filter::AddNoise { amount, monochrome } => {
            let a = amount / 100.0 * 0.6;
            img.map(|x, y, p| {
                on_color(p, |c| {
                    let n = hash(x, y, 1) * a;
                    [0, 1, 2].map(|i| {
                        let ni = if *monochrome {
                            n
                        } else {
                            hash(x, y, 1 + i as u32) * a
                        };
                        let e = color::linear_to_srgb(c[i]) + ni;
                        color::srgb_to_linear(e.clamp(0.0, 1.0))
                    })
                })
            })
        }
        Filter::ReduceNoise { strength, detail } => {
            // Bilateral: average neighbours whose colour is close.
            let sigma_c = (0.02 + strength / 10.0 * 0.25) * (1.0 - detail / 100.0 * 0.6);
            let r = 2i64;
            let w = img.w;
            let px: Vec<[f32; 4]> = (0..img.w * img.h)
                .into_par_iter()
                .map(|i| {
                    let (x, y) = ((i % w) as i64, (i / w) as i64);
                    let c = img.get(x, y);
                    if c[3] <= 1e-6 || *strength <= 0.0 {
                        return c;
                    }
                    let mut acc = [0.0f32; 4];
                    let mut ws = 0.0;
                    for dy in -r..=r {
                        for dx in -r..=r {
                            let p = img.get(x + dx, y + dy);
                            let dist: f32 =
                                (0..3).map(|k| (p[k] - c[k]).powi(2)).sum::<f32>().sqrt();
                            let wgt = (-(dist * dist) / (2.0 * sigma_c * sigma_c)).exp()
                                * (-((dx * dx + dy * dy) as f32) / 6.0).exp();
                            for k in 0..4 {
                                acc[k] += p[k] * wgt;
                            }
                            ws += wgt;
                        }
                    }
                    acc.map(|v| v / ws.max(1e-6))
                })
                .collect();
            Image { w, h: img.h, px }
        }
        Filter::HighPass { radius } => {
            let blur = gaussian(img, *radius);
            let px: Vec<[f32; 4]> = img
                .px
                .par_iter()
                .zip(&blur.px)
                .map(|(p, b)| {
                    if p[3] <= 1e-6 {
                        return *p;
                    }
                    let inv = 1.0 / p[3];
                    let binv = 1.0 / b[3].max(1e-6);
                    let c = [0, 1, 2].map(|k| {
                        let e = color::linear_to_srgb((p[k] * inv).clamp(0.0, 1.0))
                            - color::linear_to_srgb((b[k] * binv).clamp(0.0, 1.0))
                            + 0.5;
                        color::srgb_to_linear(e.clamp(0.0, 1.0)) * p[3]
                    });
                    [c[0], c[1], c[2], p[3]]
                })
                .collect();
            Image {
                w: img.w,
                h: img.h,
                px,
            }
        }
        Filter::LensProfile {
            a,
            b,
            c,
            k1,
            k2,
            k3,
            scale,
            distortion,
            vignette,
        } => {
            // PanoTools/lensfun: radius 1 is half the shorter side of the
            // calibration sensor; `scale` converts this picture's radius.
            let (cx, cy) = (img.w as f32 / 2.0, img.h as f32 / 2.0);
            let unit = (img.w.min(img.h) as f32 / 2.0).max(1.0);
            let q = if *scale > 0.0 { *scale } else { 1.0 };
            let kd = distortion / 100.0;
            let (a, b, c) = (a * kd, b * kd, c * kd);
            let d = 1.0 - a - b - c;
            let warped = if kd == 0.0 || (a == 0.0 && b == 0.0 && c == 0.0) {
                img.map(|_, _, p| p)
            } else {
                // Undistort: the corrected pixel at r_u shows the source at r_d.
                img.warp(|x, y| {
                    let (dx, dy) = (x - cx, y - cy);
                    let ru = (dx * dx + dy * dy).sqrt() / unit * q;
                    if ru <= 1e-6 {
                        return (x, y);
                    }
                    let rd = ru * (a * ru * ru * ru + b * ru * ru + c * ru + d);
                    let f = rd / ru;
                    (cx + dx * f, cy + dy * f)
                })
            };
            let kv = vignette / 100.0;
            if kv == 0.0 || (*k1 == 0.0 && *k2 == 0.0 && *k3 == 0.0) {
                return warped;
            }
            let (k1, k2, k3) = (k1 * kv, k2 * kv, k3 * kv);
            warped.map(|x, y, p| {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                let r2 = (dx * dx + dy * dy) / (unit * unit) * q * q;
                // pa model: measured = clean / (1 + k1 r² + k2 r⁴ + k3 r⁶); undo it.
                let cd = 1.0 + k1 * r2 + k2 * r2 * r2 + k3 * r2 * r2 * r2;
                let g = (1.0 / cd.max(0.05)).clamp(0.2, 4.0);
                on_color(p, |c| c.map(|ch| ch * g))
            })
        }
        Filter::LensCorrection {
            distortion,
            vignette,
        } => {
            let (cx, cy) = (img.w as f32 / 2.0, img.h as f32 / 2.0);
            let rmax = (cx * cx + cy * cy).sqrt().max(1.0);
            let k = distortion / 100.0 * 0.5;
            let v = vignette / 100.0;
            let warped = img.warp(|x, y| {
                let (dx, dy) = ((x - cx) / rmax, (y - cy) / rmax);
                let r2 = dx * dx + dy * dy;
                let f = 1.0 + k * r2;
                (cx + dx * f * rmax, cy + dy * f * rmax)
            });
            if v == 0.0 {
                return warped;
            }
            warped.map(|x, y, p| {
                let (dx, dy) = ((x as f32 + 0.5 - cx) / rmax, (y as f32 + 0.5 - cy) / rmax);
                let r = (dx * dx + dy * dy).sqrt();
                let g = (1.0 - v * r * r * 1.5).clamp(0.0, 2.0);
                on_color(p, |c| c.map(|ch| ch * g))
            })
        }
        Filter::Emboss {
            angle,
            height,
            amount,
        } => {
            let (s, c) = angle.to_radians().sin_cos();
            let (ox, oy) = (c * height, -s * height);
            let k = amount / 100.0;
            let w = img.w;
            let px: Vec<[f32; 4]> = (0..img.w * img.h)
                .into_par_iter()
                .map(|i| {
                    let (x, y) = ((i % w) as f32 + 0.5, (i / w) as f32 + 0.5);
                    let p = img.get((i % w) as i64, (i / w) as i64);
                    if p[3] <= 1e-6 {
                        return p;
                    }
                    let a = img.sample(x + ox, y + oy);
                    let b = img.sample(x - ox, y - oy);
                    let d = (luma(a) - luma(b)) * k;
                    let e = (0.5 + d).clamp(0.0, 1.0);
                    let l = color::srgb_to_linear(e) * p[3];
                    [l, l, l, p[3]]
                })
                .collect();
            Image { w, h: img.h, px }
        }
        Filter::FindEdges => {
            let w = img.w;
            let px: Vec<[f32; 4]> = (0..img.w * img.h)
                .into_par_iter()
                .map(|i| {
                    let (x, y) = ((i % w) as i64, (i / w) as i64);
                    let p = img.get(x, y);
                    if p[3] <= 1e-6 {
                        return p;
                    }
                    let l = |dx: i64, dy: i64| luma(img.get(x + dx, y + dy));
                    let gx =
                        l(1, -1) + 2.0 * l(1, 0) + l(1, 1) - l(-1, -1) - 2.0 * l(-1, 0) - l(-1, 1);
                    let gy =
                        l(-1, 1) + 2.0 * l(0, 1) + l(1, 1) - l(-1, -1) - 2.0 * l(0, -1) - l(1, -1);
                    let g = (gx * gx + gy * gy).sqrt().min(1.0);
                    let e = color::srgb_to_linear(1.0 - g) * p[3];
                    [e, e, e, p[3]]
                })
                .collect();
            Image { w, h: img.h, px }
        }
        Filter::Pinch { amount } => {
            let (cx, cy) = (img.w as f32 / 2.0, img.h as f32 / 2.0);
            let rmax = cx.min(cy).max(1.0);
            let k = amount / 100.0;
            img.warp(|x, y| {
                let (dx, dy) = (x - cx, y - cy);
                let r = (dx * dx + dy * dy).sqrt() / rmax;
                if r >= 1.0 || r <= 0.0 {
                    return (x, y);
                }
                let f = r.powf(1.0 + k * 0.9) / r;
                (cx + dx * f, cy + dy * f)
            })
        }
        Filter::Twirl { angle } => {
            let (cx, cy) = (img.w as f32 / 2.0, img.h as f32 / 2.0);
            let rmax = cx.min(cy).max(1.0);
            let a = angle.to_radians();
            img.warp(|x, y| {
                let (dx, dy) = (x - cx, y - cy);
                let r = (dx * dx + dy * dy).sqrt() / rmax;
                if r >= 1.0 {
                    return (x, y);
                }
                let t = a * (1.0 - r) * (1.0 - r);
                let (s, c) = t.sin_cos();
                (cx + dx * c - dy * s, cy + dx * s + dy * c)
            })
        }
        Filter::Wave {
            amplitude,
            wavelength,
        } => {
            let wl = wavelength.max(2.0);
            img.warp(|x, y| {
                (
                    x + (y / wl * std::f32::consts::TAU).sin() * amplitude,
                    y + (x / wl * std::f32::consts::TAU).cos() * amplitude * 0.5,
                )
            })
        }
        Filter::Enhance { amount, sky } => photo::enhance(img, *amount, *sky),
        Filter::Structure { amount, softness } => photo::structure(img, *amount, *softness),
        Filter::Glow {
            amount,
            radius,
            threshold,
        } => photo::glow(img, *amount, *radius, *threshold),
        Filter::Orton { amount, radius } => photo::orton(img, *amount, *radius),
        Filter::Sunrays {
            x,
            y,
            amount,
            length,
            warmth,
        } => photo::sunrays(img, [*x, *y], *amount, *length, *warmth),
        Filter::Atmosphere { amount, spread } => photo::atmosphere(img, *amount, *spread),
        Filter::SkinSmooth {
            amount,
            radius,
            detail,
        } => photo::skin_smooth(img, *amount, *radius, *detail),
        Filter::GoldenHour { amount } => photo::golden_hour(img, *amount),
        Filter::Dramatic { amount } => photo::dramatic(img, *amount),
    }
}

/// Photo looks: Enhance, Structure, Glow, Mystical (Orton), Sunrays,
/// Atmosphere, Skin smoothing, Golden hour and Dramatic.
///
/// Every kernel keeps the image's size and alpha, returns an unchanged copy
/// at amount 0, never spreads past the layer, and does O(n) or separable
/// work at full resolution; wide blurs and costly analyses run on a
/// downsampled copy and are bilinearly upsampled. Sizes given as a
/// percentage of the image scale with its shorter side, so a look matches at
/// any resolution. Blurs weight by alpha (premultiplied, then divided by the
/// blurred alpha) so transparent borders do not darken edges.
mod photo {
    use super::{Image, color, luma, on_color};
    use rayon::prelude::*;
    use std::sync::LazyLock;

    const LUT: usize = 4096;

    /// `linear_to_srgb` sampled on a square-root grid, where it is smooth.
    static TO_SRGB: LazyLock<Vec<f32>> = LazyLock::new(|| {
        (0..=LUT)
            .map(|i| color::linear_to_srgb((i as f32 / LUT as f32).powi(2)))
            .collect()
    });
    static TO_LINEAR: LazyLock<Vec<f32>> = LazyLock::new(|| {
        (0..=LUT)
            .map(|i| color::srgb_to_linear(i as f32 / LUT as f32))
            .collect()
    });

    #[inline]
    fn lookup(table: &[f32], t: f32) -> f32 {
        let t = if t.is_finite() {
            t.clamp(0.0, 1.0)
        } else {
            0.0
        } * LUT as f32;
        let i = (t as usize).min(LUT - 1);
        let f = t - i as f32;
        table[i] + (table[i + 1] - table[i]) * f
    }

    /// Fast `color::linear_to_srgb` for per-pixel work (error below 1e-5).
    #[inline]
    fn to_srgb(v: f32) -> f32 {
        lookup(&TO_SRGB, v.max(0.0).sqrt())
    }

    /// Fast `color::srgb_to_linear` for per-pixel work.
    #[inline]
    fn to_linear(e: f32) -> f32 {
        lookup(&TO_LINEAR, e)
    }

    #[inline]
    fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
        let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    fn shorter(img: &Image) -> f32 {
        img.w.min(img.h).max(1) as f32
    }

    #[inline]
    fn enc(c: [f32; 3]) -> [f32; 3] {
        c.map(|v| to_srgb(v.clamp(0.0, 1.0)))
    }

    #[inline]
    fn dec(e: [f32; 3]) -> [f32; 3] {
        e.map(|v| to_linear(v.clamp(0.0, 1.0)))
    }

    #[inline]
    fn luma3(c: [f32; 3]) -> f32 {
        color::luma(c[0], c[1], c[2])
    }

    /// Pull an out-of-range colour toward its own luma until it fits.
    fn fit_gamut(c: [f32; 3]) -> [f32; 3] {
        let c = c.map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 });
        let m = c[0].max(c[1]).max(c[2]);
        if m <= 1.0 {
            return c;
        }
        let l = luma3(c);
        if l >= 1.0 {
            return [1.0; 3];
        }
        let t = (1.0 - l) / (m - l);
        c.map(|v| (l + (v - l) * t).clamp(0.0, 1.0))
    }

    /// Give linear colour `c` the perceptual luma `target`, keeping its hue.
    fn relight(c: [f32; 3], target: f32) -> [f32; 3] {
        let target = to_linear(target.clamp(0.0, 1.0));
        let l = luma3(c);
        if l > 1e-5 {
            fit_gamut(c.map(|v| v * (target / l)))
        } else {
            [target; 3]
        }
    }

    /// Scale an encoded colour's distance from its luma.
    fn saturate(e: [f32; 3], k: f32) -> [f32; 3] {
        let l = luma3(e);
        e.map(|v| (l + (v - l) * k).clamp(0.0, 1.0))
    }

    /// HSV hue in degrees and saturation of an encoded colour.
    fn hue_sat(e: [f32; 3]) -> (f32, f32) {
        let mx = e[0].max(e[1]).max(e[2]);
        let mn = e[0].min(e[1]).min(e[2]);
        let d = mx - mn;
        if d <= 1e-6 {
            return (0.0, 0.0);
        }
        let h = if mx == e[0] {
            ((e[1] - e[2]) / d).rem_euclid(6.0)
        } else if mx == e[1] {
            (e[2] - e[0]) / d + 2.0
        } else {
            (e[0] - e[1]) / d + 4.0
        };
        (h * 60.0, d / mx.max(1e-6))
    }

    /// 1 inside the hue range `lo..hi`, easing to 0 over `feather` degrees.
    fn hue_band(h: f32, lo: f32, hi: f32, feather: f32) -> f32 {
        smoothstep(lo - feather, lo, h) * (1.0 - smoothstep(hi, hi + feather, h))
    }

    /// sRGB-encoded (perceptual) luma of each unpremultiplied pixel.
    fn perceptual_luma(img: &Image) -> Vec<f32> {
        img.px
            .par_iter()
            .map(|p| to_srgb(luma(*p).clamp(0.0, 1.0)))
            .collect()
    }

    /// Bilinear sample with pixel centres at integer coordinates, clamped to
    /// the edge.
    fn bilinear<const N: usize>(px: &[[f32; N]], w: usize, h: usize, x: f32, y: f32) -> [f32; N] {
        let x = if x.is_finite() {
            x.clamp(0.0, (w - 1) as f32)
        } else {
            0.0
        };
        let y = if y.is_finite() {
            y.clamp(0.0, (h - 1) as f32)
        } else {
            0.0
        };
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
        let (tx, ty) = (x - x0 as f32, y - y0 as f32);
        let (a, b) = (px[y0 * w + x0], px[y0 * w + x1]);
        let (c, d) = (px[y1 * w + x0], px[y1 * w + x1]);
        std::array::from_fn(|k| {
            (a[k] * (1.0 - tx) + b[k] * tx) * (1.0 - ty) + (c[k] * (1.0 - tx) + d[k] * tx) * ty
        })
    }

    /// Box-average `f`×`f` cells; partial cells at the edges average what
    /// they cover.
    fn downsample<const N: usize>(
        px: &[[f32; N]],
        w: usize,
        h: usize,
        f: usize,
    ) -> (Vec<[f32; N]>, usize, usize) {
        let (ws, hs) = (w.div_ceil(f), h.div_ceil(f));
        let out = (0..ws * hs)
            .into_par_iter()
            .map(|i| {
                let (x0, y0) = ((i % ws) * f, (i / ws) * f);
                let (x1, y1) = ((x0 + f).min(w), (y0 + f).min(h));
                let mut acc = [0.0f32; N];
                for y in y0..y1 {
                    for p in &px[y * w + x0..y * w + x1] {
                        for (a, v) in acc.iter_mut().zip(p) {
                            *a += v;
                        }
                    }
                }
                let inv = 1.0 / ((x1 - x0) * (y1 - y0)) as f32;
                acc.map(|v| v * inv)
            })
            .collect();
        (out, ws, hs)
    }

    /// Where full-resolution pixel centre `x` falls in an `f`-times smaller grid.
    #[inline]
    fn to_small(x: usize, f: usize) -> f32 {
        (x as f32 + 0.5) / f as f32 - 0.5
    }

    fn upsample<const N: usize>(
        small: &[[f32; N]],
        ws: usize,
        hs: usize,
        f: usize,
        w: usize,
        h: usize,
    ) -> Vec<[f32; N]> {
        let mut out = vec![[0.0f32; N]; w * h];
        out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
            let sy = to_small(y, f);
            for (x, o) in row.iter_mut().enumerate() {
                *o = bilinear(small, ws, hs, to_small(x, f), sy);
            }
        });
        out
    }

    /// Separable Gaussian whose kernel is renormalised at the borders.
    fn blur_direct<const N: usize>(
        px: &[[f32; N]],
        w: usize,
        h: usize,
        sigma: f32,
    ) -> Vec<[f32; N]> {
        let r = (sigma * 3.0).ceil().max(1.0) as usize;
        let k: Vec<f32> = (0..=r)
            .map(|i| (-((i * i) as f32) / (2.0 * sigma * sigma)).exp())
            .collect();
        let mut tmp = vec![[0.0f32; N]; w * h];
        tmp.par_chunks_mut(w).enumerate().for_each(|(y, out)| {
            let row = &px[y * w..(y + 1) * w];
            for (x, o) in out.iter_mut().enumerate() {
                let mut acc = [0.0f32; N];
                let mut total = 0.0;
                for (xx, p) in row
                    .iter()
                    .enumerate()
                    .take((x + r).min(w - 1) + 1)
                    .skip(x.saturating_sub(r))
                {
                    let wt = k[xx.abs_diff(x)];
                    total += wt;
                    for (a, v) in acc.iter_mut().zip(p) {
                        *a += v * wt;
                    }
                }
                *o = acc.map(|v| v / total);
            }
        });
        let mut out = vec![[0.0f32; N]; w * h];
        out.par_chunks_mut(w).enumerate().for_each(|(y, o)| {
            let mut total = 0.0;
            for yy in y.saturating_sub(r)..=(y + r).min(h - 1) {
                let wt = k[yy.abs_diff(y)];
                total += wt;
                for (a, p) in o.iter_mut().zip(&tmp[yy * w..(yy + 1) * w]) {
                    for (a, v) in a.iter_mut().zip(p) {
                        *a += v * wt;
                    }
                }
            }
            let inv = 1.0 / total;
            for a in o.iter_mut().flatten() {
                *a *= inv;
            }
        });
        out
    }

    /// Gaussian blur with standard deviation `sigma` pixels. Wide blurs run
    /// on a box-downsampled copy and are bilinearly upsampled, so the cost
    /// stays O(n) whatever the radius.
    fn blur<const N: usize>(px: &[[f32; N]], w: usize, h: usize, sigma: f32) -> Vec<[f32; N]> {
        if sigma.is_nan() || sigma <= 0.05 || w == 0 || h == 0 {
            return px.to_vec();
        }
        if sigma <= 4.0 {
            return blur_direct(px, w, h, sigma);
        }
        let f = ((sigma / 2.0) as usize).max(2);
        let (small, ws, hs) = downsample(px, w, h, f);
        let b = blur_direct(&small, ws, hs, sigma / f as f32);
        upsample(&b, ws, hs, f, w, h)
    }

    /// Blur a per-pixel plane weighted by alpha.
    fn blur_luma(img: &Image, l: &[f32], sigma: f32) -> Vec<f32> {
        let packed: Vec<[f32; 2]> = l
            .par_iter()
            .zip(&img.px)
            .map(|(l, p)| {
                let a = p[3].clamp(0.0, 1.0);
                [l * a, a]
            })
            .collect();
        blur(&packed, img.w, img.h, sigma)
            .into_par_iter()
            .zip(l.par_iter())
            .map(|(b, l)| {
                if b[1] > 1e-4 {
                    (b[0] / b[1]).clamp(0.0, 1.0)
                } else {
                    *l
                }
            })
            .collect()
    }

    /// Blur premultiplied pixels and return unpremultiplied linear colour.
    fn blur_unpremul(img: &Image, sigma: f32) -> Vec<[f32; 3]> {
        blur(&img.px, img.w, img.h, sigma)
            .into_par_iter()
            .map(unpremul_soft)
            .collect()
    }

    fn unpremul_soft(p: [f32; 4]) -> [f32; 3] {
        if p[3] > 1e-4 {
            [p[0] / p[3], p[1] / p[3], p[2] / p[3]].map(|v| v.clamp(0.0, 1.0))
        } else {
            [0.0; 3]
        }
    }

    /// Separable minimum over a (2r+1)² square.
    fn min_filter(v: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
        let mut tmp = vec![0.0f32; w * h];
        tmp.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
            let src = &v[y * w..(y + 1) * w];
            for (x, o) in row.iter_mut().enumerate() {
                *o = src[x.saturating_sub(r)..=(x + r).min(w - 1)]
                    .iter()
                    .copied()
                    .fold(f32::INFINITY, f32::min);
            }
        });
        let mut out = vec![f32::INFINITY; w * h];
        out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
            for yy in y.saturating_sub(r)..=(y + r).min(h - 1) {
                for (o, s) in row.iter_mut().zip(&tmp[yy * w..(yy + 1) * w]) {
                    *o = o.min(*s);
                }
            }
        });
        out
    }

    /// Guided filter (He et al.) with Gaussian windows: per-pixel smoothed
    /// `[a, b]` so the filtered value is `a * guide + b`.
    fn guided_coeffs(
        guide: &[f32],
        src: &[f32],
        w: usize,
        h: usize,
        sigma: f32,
        eps: f32,
    ) -> Vec<[f32; 2]> {
        let packed: Vec<[f32; 4]> = guide
            .par_iter()
            .zip(src)
            .map(|(&i, &p)| [i, p, i * p, i * i])
            .collect();
        let m = blur(&packed, w, h, sigma);
        let ab: Vec<[f32; 2]> = m
            .par_iter()
            .map(|m| {
                let var = (m[3] - m[0] * m[0]).max(0.0);
                let cov = m[2] - m[0] * m[1];
                let a = cov / (var + eps);
                [a, m[1] - a * m[0]]
            })
            .collect();
        blur(&ab, w, h, sigma)
    }

    /// Luminar-style "Accent AI": adaptive black/white points, shadow lift,
    /// highlight recovery, an S-curve, local contrast over 1% of the shorter
    /// side, vibrance that spares skin hues, and an optional sky boost.
    pub(super) fn enhance(img: &Image, amount: f32, sky: f32) -> Image {
        let a = (amount / 100.0).clamp(0.0, 1.0);
        let s = (sky / 100.0).clamp(0.0, 1.0);
        if a <= 0.0 && s <= 0.0 {
            return img.pad(0);
        }
        let (w, h) = (img.w, img.h);
        let l = perceptual_luma(img);
        let step = ((img.px.len() as f32 / 65_536.0).sqrt() as usize).max(1);
        let mut samples: Vec<f32> = (0..h)
            .step_by(step)
            .flat_map(|y| (0..w).step_by(step).map(move |x| y * w + x))
            .filter(|&i| img.px[i][3] > 0.01)
            .map(|i| l[i])
            .collect();
        if samples.is_empty() {
            return img.pad(0);
        }
        samples.sort_unstable_by(f32::total_cmp);
        let pct = |q: f32| samples[((samples.len() - 1) as f32 * q).round() as usize];
        let (lo, mid, hi) = (pct(0.005), pct(0.5), pct(0.995));
        let black = lo * a * 0.7;
        let white = 1.0 - (1.0 - hi) * a * 0.7;
        let span = (white - black).max(0.05);
        let lift = a * (0.1 + 0.3 * smoothstep(0.6, 0.2, mid));
        let recover = a * (0.1 + 0.3 * smoothstep(0.75, 0.97, hi));
        let contrast = a * 0.35;
        let local = a * 0.4;
        let base = if local > 0.0 {
            blur_luma(img, &l, (shorter(img) * 0.01).max(1.0))
        } else {
            l.clone()
        };
        let hf = h as f32;
        img.map(|x, y, p| {
            if p[3] <= 1e-6 {
                return p;
            }
            let i = y * w + x;
            let l0 = l[i];
            let mut t = ((l0 - black) / span).clamp(0.0, 1.0);
            t += lift * (t.sqrt() - t) * (1.0 - t) * (1.0 - t);
            t -= recover * (t - t * t) * t;
            t += contrast * (t * t * (3.0 - 2.0 * t) - t);
            let d = l0 - base[i];
            t += local * (4.0 * t * (1.0 - t)).clamp(0.0, 1.0) * d / (1.0 + d.abs() * 3.0);
            let top = 1.0 - 0.7 * (y as f32 + 0.5) / hf;
            on_color(p, |c| {
                let mut e = enc(relight(c, t));
                let (hue, sat) = hue_sat(e);
                let skin = hue_band(hue, 20.0, 50.0, 10.0);
                e = saturate(e, 1.0 + a * 0.45 * (1.0 - sat) * (1.0 - 0.7 * skin));
                if s > 0.0 {
                    let (hue, sat) = hue_sat(e);
                    let ws = s
                        * hue_band(hue, 180.0, 250.0, 20.0)
                        * smoothstep(0.08, 0.3, sat)
                        * smoothstep(0.3, 0.6, luma3(e))
                        * top;
                    e = saturate(e, 1.0 + 0.6 * ws).map(|v| v * (1.0 - 0.18 * ws));
                }
                fit_gamut(dec(e))
            })
        })
    }

    /// Local contrast on perceptual luma. The blur radius is 1–3% of the
    /// shorter side (larger with `softness`); detail is compressed against
    /// halos and weighted toward midtones so shadows and highlights do not
    /// clip. Negative amounts blend toward the blurred luma.
    pub(super) fn structure(img: &Image, amount: f32, softness: f32) -> Image {
        let k = (amount / 100.0).clamp(-1.0, 1.0);
        if k == 0.0 {
            return img.pad(0);
        }
        let radius = shorter(img) * (0.01 + 0.02 * (softness / 100.0).clamp(0.0, 1.0));
        let l = perceptual_luma(img);
        let b = blur_luma(img, &l, (radius / 2.0).max(1.0));
        let w = img.w;
        img.map(|x, y, p| {
            if p[3] <= 1e-6 {
                return p;
            }
            let i = y * w + x;
            let l0 = l[i];
            let d = l0 - b[i];
            let m = (4.0 * l0 * (1.0 - l0)).clamp(0.0, 1.0).sqrt();
            let nl = if k > 0.0 {
                l0 + k * 1.5 * m * d / (1.0 + d.abs() * 2.5)
            } else {
                l0 + k * m * d
            };
            on_color(p, |c| relight(c, nl))
        })
    }

    /// Screen a blur of the areas brighter than `threshold` back over the
    /// image. The blur radius is `radius`% of 5% of the shorter side.
    pub(super) fn glow(img: &Image, amount: f32, radius: f32, threshold: f32) -> Image {
        let a = (amount / 100.0).clamp(0.0, 1.0);
        if a <= 0.0 {
            return img.pad(0);
        }
        let r = (radius / 100.0).clamp(0.0, 1.0) * shorter(img) * 0.05;
        let t = (threshold / 100.0).clamp(0.0, 1.0);
        let knee = 0.1;
        let bright: Vec<[f32; 4]> = img
            .px
            .par_iter()
            .map(|p| {
                let l = to_srgb(luma(*p).clamp(0.0, 1.0));
                let wt = smoothstep(t - knee, t + knee, l);
                [p[0] * wt, p[1] * wt, p[2] * wt, p[3]]
            })
            .collect();
        let g = blur(&bright, img.w, img.h, (r / 2.0).max(0.5));
        drop(bright);
        let w = img.w;
        img.map(|x, y, p| {
            let gl = unpremul_soft(g[y * w + x]).map(|v| (v * 1.6 * a).min(1.0));
            on_color(p, |c| [0, 1, 2].map(|k| 1.0 - (1.0 - c[k]) * (1.0 - gl[k])))
        })
    }

    /// Orton effect: the blurred copy is screened with itself, mixed back
    /// half multiply, half screen, saturated, and blended at up to 80% so
    /// some sharpness remains. The blur radius is `radius`% of 8% of the
    /// shorter side.
    pub(super) fn orton(img: &Image, amount: f32, radius: f32) -> Image {
        let a = (amount / 100.0).clamp(0.0, 1.0);
        if a <= 0.0 {
            return img.pad(0);
        }
        let r = (radius / 100.0).clamp(0.0, 1.0) * shorter(img) * 0.08;
        let b = blur_unpremul(img, (r / 2.0).max(0.5));
        let w = img.w;
        let mix = a * 0.8;
        img.map(|x, y, p| {
            let be = enc(b[y * w + x]);
            on_color(p, |c| {
                let e = enc(c);
                let o = [0, 1, 2].map(|k| {
                    let bright = 1.0 - (1.0 - be[k]) * (1.0 - be[k]);
                    0.5 * e[k] * bright + 0.5 * (1.0 - (1.0 - e[k]) * (1.0 - be[k]))
                });
                let o = saturate(o, 1.25);
                dec([0, 1, 2].map(|k| e[k] + (o[k] - e[k]) * mix))
            })
        })
    }

    /// Radial blur of a bright-areas mask toward the sun at `sun` (percent of
    /// width and height), plus a glow at the sun, tinted and screened. Rays
    /// are computed with the longest side at most 768 pixels and upsampled;
    /// their reach scales with the image, so `length` means the same at any
    /// resolution.
    pub(super) fn sunrays(
        img: &Image,
        sun: [f32; 2],
        amount: f32,
        length: f32,
        warmth: f32,
    ) -> Image {
        let a = (amount / 100.0).clamp(0.0, 1.0);
        if a <= 0.0 {
            return img.pad(0);
        }
        let (w, h) = (img.w, img.h);
        let f = w.max(h).div_ceil(768).max(1);
        let (small, ws, hs) = downsample(&img.px, w, h, f);
        let mask: Vec<[f32; 1]> = small
            .par_iter()
            .map(|p| {
                let l = to_srgb(luma(*p).clamp(0.0, 1.0));
                [smoothstep(0.5, 0.85, l) * p[3].clamp(0.0, 1.0)]
            })
            .collect();
        drop(small);
        let sx = (sun[0] / 100.0).clamp(0.0, 1.0) * ws as f32 - 0.5;
        let sy = (sun[1] / 100.0).clamp(0.0, 1.0) * hs as f32 - 0.5;
        let short = ws.min(hs).max(1) as f32;
        let len = (length / 100.0).clamp(0.0, 1.0);
        let reach = 0.15 + 0.85 * len;
        let falloff = short * (0.3 + 1.2 * len);
        const STEPS: usize = 48;
        let rays: Vec<[f32; 1]> = (0..ws * hs)
            .into_par_iter()
            .map(|i| {
                let (px, py) = ((i % ws) as f32, (i / ws) as f32);
                let (dx, dy) = (sx - px, sy - py);
                let d = (dx * dx + dy * dy).sqrt();
                let (mut acc, mut total) = (0.0, 0.0);
                for k in 0..STEPS {
                    let t = k as f32 / STEPS as f32;
                    let wt = 1.0 - 0.5 * t;
                    let s = t * reach;
                    acc += bilinear(&mask, ws, hs, px + dx * s, py + dy * s)[0] * wt;
                    total += wt;
                }
                let ray = acc / total * (-d / falloff).exp();
                let glow =
                    0.9 * (-(d / (0.05 * short)).powi(2)).exp() + 0.3 * (-d / (0.2 * short)).exp();
                [(a * (1.6 * ray + glow)).min(1.0)]
            })
            .collect();
        let warm = (warmth / 100.0).clamp(0.0, 1.0);
        let tint = [1.0, 1.0 - 0.3 * warm, 1.0 - 0.65 * warm];
        img.map(|x, y, p| {
            let v = bilinear(&rays, ws, hs, to_small(x, f), to_small(y, f))[0];
            on_color(p, |c| {
                [0, 1, 2].map(|k| 1.0 - (1.0 - c[k]) * (1.0 - v * tint[k]))
            })
        })
    }

    /// Add haze (positive) or remove it with the dark-channel prior
    /// (negative). Analysis runs with the longest side at most 512 pixels;
    /// the dark-channel patch is 1.2% of the shorter side.
    pub(super) fn atmosphere(img: &Image, amount: f32, spread: f32) -> Image {
        let k = (amount / 100.0).clamp(-1.0, 1.0);
        if k == 0.0 {
            return img.pad(0);
        }
        let (w, h) = (img.w, img.h);
        let f = w.max(h).div_ceil(512).max(1);
        let (small, ws, hs) = downsample(&img.px, w, h, f);
        let es: Vec<[f32; 3]> = small.par_iter().map(|p| enc(unpremul_soft(*p))).collect();
        let opaque: Vec<bool> = small.iter().map(|p| p[3] > 0.01).collect();
        drop(small);
        let pr = ((ws.min(hs) as f32 * 0.012).round() as usize).max(1);
        let raw: Vec<f32> = es
            .iter()
            .zip(&opaque)
            .map(|(e, &o)| if o { e[0].min(e[1]).min(e[2]) } else { 1.0 })
            .collect();
        let dark = min_filter(&raw, ws, hs, pr);
        // Airlight: the mean colour of the brightest 0.1% of the dark channel.
        let mut idx: Vec<usize> = (0..ws * hs).filter(|&i| opaque[i]).collect();
        if idx.is_empty() {
            return img.pad(0);
        }
        idx.sort_unstable_by(|&i, &j| dark[j].total_cmp(&dark[i]));
        let top = (idx.len() / 1000).max(1);
        let mut air = [0.0f32; 3];
        for &i in &idx[..top] {
            for (a, v) in air.iter_mut().zip(es[i]) {
                *a += v;
            }
        }
        let air = air.map(|v| (v / top as f32).clamp(0.05, 1.0));
        if k > 0.0 {
            let dark: Vec<[f32; 1]> = dark
                .iter()
                .zip(&opaque)
                .map(|(d, &o)| [if o { d.clamp(0.0, 1.0) } else { 0.0 }])
                .collect();
            let dark = blur(&dark, ws, hs, pr as f32 * 2.0);
            let sp = (spread / 100.0).clamp(0.0, 1.0);
            let reach = 0.15 + 0.85 * sp;
            let amax = air[0].max(air[1]).max(air[2]);
            let fog = air.map(|v| v * 0.7 + amax * 0.3);
            let hf = h as f32;
            img.map(|x, y, p| {
                let dc = bilinear(&dark, ws, hs, to_small(x, f), to_small(y, f))[0];
                let yn = (y as f32 + 0.5) / hf;
                let v = (1.0 - smoothstep(0.0, reach, yn)) * 0.8 + 0.2 * sp;
                let wt = (k * 0.85 * v * (0.35 + 0.65 * dc)).clamp(0.0, 0.95);
                on_color(p, |c| {
                    let e = enc(c);
                    dec([0, 1, 2].map(|j| e[j] + (fog[j] - e[j]) * wt))
                })
            })
        } else {
            let omega = 0.95 * -k;
            let norm: Vec<f32> = es
                .iter()
                .zip(&opaque)
                .map(|(e, &o)| {
                    if o {
                        (0..3)
                            .map(|j| e[j] / air[j])
                            .fold(f32::INFINITY, f32::min)
                            .min(1.0)
                    } else {
                        1.0
                    }
                })
                .collect();
            let t: Vec<f32> = min_filter(&norm, ws, hs, pr)
                .into_iter()
                .map(|d| 1.0 - omega * d)
                .collect();
            let gray: Vec<f32> = es.iter().map(|e| luma3(*e)).collect();
            let coeffs = guided_coeffs(&gray, &t, ws, hs, pr as f32 * 2.0, 1e-3);
            let t: Vec<[f32; 1]> = coeffs
                .iter()
                .zip(&gray)
                .map(|(c, g)| [(c[0] * g + c[1]).clamp(0.1, 1.0)])
                .collect();
            img.map(|x, y, p| {
                let t = bilinear(&t, ws, hs, to_small(x, f), to_small(y, f))[0].clamp(0.1, 1.0);
                on_color(p, |c| {
                    let e = enc(c);
                    dec([0, 1, 2].map(|j| (e[j] - air[j]) / t + air[j]))
                })
            })
        }
    }

    /// Full-range BT.601 YCbCr of an encoded colour.
    fn to_ycc(e: [f32; 3]) -> [f32; 3] {
        [
            0.299 * e[0] + 0.587 * e[1] + 0.114 * e[2],
            0.5 - 0.168_736 * e[0] - 0.331_264 * e[1] + 0.5 * e[2],
            0.5 + 0.5 * e[0] - 0.418_688 * e[1] - 0.081_312 * e[2],
        ]
    }

    fn from_ycc(v: [f32; 3]) -> [f32; 3] {
        let (y, cb, cr) = (v[0], v[1] - 0.5, v[2] - 0.5);
        [
            y + 1.402 * cr,
            y - 0.344_136 * cb - 0.714_136 * cr,
            y + 1.772 * cb,
        ]
    }

    /// Soft skin-tone likelihood: Cb 77–127 and Cr 133–173 (of 255), easing
    /// out over about 8 levels, and not too dark.
    fn skin_likelihood(v: [f32; 3]) -> f32 {
        let fe = 8.0 / 255.0;
        let band = |x: f32, lo: f32, hi: f32| {
            smoothstep(lo - fe, lo + fe, x) * (1.0 - smoothstep(hi - fe, hi + fe, x))
        };
        band(v[1], 77.0 / 255.0, 127.0 / 255.0)
            * band(v[2], 133.0 / 255.0, 173.0 / 255.0)
            * smoothstep(0.08, 0.25, v[0])
    }

    /// Guided-filter smoothing of Y, Cb and Cr inside a blurred skin mask.
    /// The window is 0.2–1.5% of the shorter side; coefficients are computed
    /// on a downsampled copy (the "fast guided filter") and sampled
    /// bilinearly, then `detail` adds back that share of the fine texture.
    pub(super) fn skin_smooth(img: &Image, amount: f32, radius: f32, detail: f32) -> Image {
        let a = (amount / 100.0).clamp(0.0, 1.0);
        if a <= 0.0 {
            return img.pad(0);
        }
        let keep = (detail / 100.0).clamp(0.0, 1.0);
        let (w, h) = (img.w, img.h);
        let sigma = (shorter(img) * (0.002 + 0.013 * (radius / 100.0).clamp(0.0, 1.0))).max(1.0);
        let f = ((sigma / 2.0) as usize).max(1);
        let (small, ws, hs) = if f > 1 {
            downsample(&img.px, w, h, f)
        } else {
            (img.px.clone(), w, h)
        };
        let ss = sigma / f as f32;
        let ycc: Vec<[f32; 3]> = small
            .par_iter()
            .map(|p| to_ycc(enc(unpremul_soft(*p))))
            .collect();
        let mask: Vec<[f32; 1]> = ycc
            .iter()
            .zip(&small)
            .map(|(v, p)| [skin_likelihood(*v) * p[3].clamp(0.0, 1.0)])
            .collect();
        drop(small);
        let mask = blur(&mask, ws, hs, ss * 1.5);
        let eps = (0.02 + 0.05 * a).powi(2);
        let mut packed = vec![[0.0f32; 7]; ws * hs];
        for c in 0..3 {
            let ch: Vec<f32> = ycc.iter().map(|v| v[c]).collect();
            for (o, ab) in packed
                .iter_mut()
                .zip(guided_coeffs(&ch, &ch, ws, hs, ss, eps))
            {
                o[2 * c] = ab[0];
                o[2 * c + 1] = ab[1];
            }
        }
        for (o, m) in packed.iter_mut().zip(&mask) {
            o[6] = m[0];
        }
        img.map(|x, y, p| {
            if p[3] <= 1e-6 {
                return p;
            }
            let k = bilinear(&packed, ws, hs, to_small(x, f), to_small(y, f));
            let m = (k[6] * a).clamp(0.0, 1.0) * (1.0 - keep);
            if m <= 1e-4 {
                return p;
            }
            on_color(p, |c| {
                let v = to_ycc(enc(c));
                let out = [0, 1, 2].map(|j| {
                    let q = k[2 * j] * v[j] + k[2 * j + 1];
                    v[j] + (q - v[j]) * m
                });
                dec(from_ycc(out))
            })
        })
    }

    /// Warm highlights and midtones toward orange, lift shadows warm, and
    /// saturate warm hues a little, weighted by luminance.
    pub(super) fn golden_hour(img: &Image, amount: f32) -> Image {
        let a = (amount / 100.0).clamp(0.0, 1.0);
        if a <= 0.0 {
            return img.pad(0);
        }
        img.map(|_, _, p| {
            on_color(p, |c| {
                let e = enc(c);
                let l = luma3(e);
                let warm = a * (0.35 + 0.65 * smoothstep(0.1, 0.8, l));
                let sh = a * (1.0 - l).powi(3) * 0.05;
                let e = [
                    e[0] * (1.0 + 0.14 * warm) + sh,
                    e[1] * (1.0 + 0.03 * warm) + sh * 0.55,
                    e[2] * (1.0 - 0.2 * warm) + sh * 0.1,
                ]
                .map(|v| v.clamp(0.0, 1.0));
                let (hue, sat) = hue_sat(e);
                let e = saturate(e, 1.0 + 0.25 * a * hue_band(hue, 10.0, 60.0, 15.0) * sat);
                fit_gamut(dec(e))
            })
        })
    }

    /// Strong local contrast over 2% of the shorter side, fine grit, deeper
    /// shadows and up to 30% desaturation.
    pub(super) fn dramatic(img: &Image, amount: f32) -> Image {
        let a = (amount / 100.0).clamp(0.0, 1.0);
        if a <= 0.0 {
            return img.pad(0);
        }
        let short = shorter(img);
        let l = perceptual_luma(img);
        let big = blur_luma(img, &l, (short * 0.02).max(1.5));
        let fine = blur_luma(img, &l, (short * 0.0008).max(0.8));
        let w = img.w;
        img.map(|x, y, p| {
            if p[3] <= 1e-6 {
                return p;
            }
            let i = y * w + x;
            let l0 = l[i];
            let d = l0 - big[i];
            let g = l0 - fine[i];
            let mut t = l0 + a * 1.3 * d / (1.0 + 2.0 * d.abs()) + a * 0.5 * g;
            t = t.clamp(0.0, 1.0);
            t -= a * 0.35 * t * (1.0 - t) * (1.0 - t);
            on_color(p, |c| dec(saturate(enc(relight(c, t)), 1.0 - 0.3 * a)))
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn transfer_tables_match_exact_curves() {
            for i in 0..=20_000 {
                let v = i as f32 / 20_000.0;
                assert!((to_srgb(v) - color::linear_to_srgb(v)).abs() < 2e-5, "{v}");
                assert!(
                    (to_linear(v) - color::srgb_to_linear(v)).abs() < 2e-5,
                    "{v}"
                );
            }
            assert_eq!(to_srgb(f32::NAN), 0.0);
            assert_eq!(to_linear(2.0), 1.0);
        }
    }
}

/// Run `stack` over `source`. Returns the filtered raster and where its
/// top-left sits relative to the source (negative when it spread out).
pub fn apply_stack(source: &Raster, stack: &[Filter]) -> (Raster, (i32, i32)) {
    apply_stack_styled(source, stack, &[])
}

/// Run a smart-filter stack with per-stage opacity and blend mode. Missing
/// styles use Normal at 100%, preserving old documents exactly.
pub fn apply_stack_styled(
    source: &Raster,
    stack: &[Filter],
    styles: &[FilterStyle],
) -> (Raster, (i32, i32)) {
    if stack.is_empty() {
        return (source.clone(), (0, 0));
    }
    let stack: Vec<Filter> = stack.iter().map(Filter::sanitized).collect();
    let spread: i32 = stack
        .iter()
        .map(Filter::spread)
        .sum::<i32>()
        .min(MAX_SPREAD);
    let (w, h) = (source.width() as usize, source.height() as usize);
    let px: Vec<[f32; 4]> = source.rows_par(1, [0.0f32; 4], |row, dst| {
        for (p, o) in row.iter().zip(dst.iter_mut()) {
            *o = color::px_to_f(*p);
        }
    });
    let mut img = Image { w, h, px }.pad(spread as usize);
    for (index, f) in stack.iter().enumerate() {
        let style = styles.get(index).copied().unwrap_or_default().sanitized();
        let before = (style.opacity < 1.0 || style.blend != BlendMode::Normal).then(|| img.clone());
        img = apply_one(f, img);
        if let Some(before) = before {
            img.px
                .par_iter_mut()
                .zip(before.px.par_iter())
                .enumerate()
                .for_each(|(index, (filtered, base))| {
                    let source = filtered.map(|channel| channel * style.opacity);
                    *filtered = blend_px(
                        style.blend,
                        BlendSpace::Linear,
                        *base,
                        source,
                        index as f32 * 0.618_034,
                    );
                });
        }
    }
    let out: Vec<[u16; 4]> = img
        .px
        .par_iter()
        .map(|p| color::f_to_px(p.map(|v| v.clamp(0.0, 1.0))))
        .collect();
    (
        Raster::from_pixels(img.w as u32, img.h as u32, [0; 4], &out),
        (-spread, -spread),
    )
}

/// Filter only `region` of a layer (a live preview inside a selection).
pub fn apply_region(source: &Raster, stack: &[Filter], region: IRect) -> Raster {
    let region = region.intersect(&source.bounds());
    if region.is_empty() {
        return source.clone();
    }
    let px: Vec<[f32; 4]> = source
        .read_rect(region)
        .into_iter()
        .map(color::px_to_f)
        .collect();
    let mut img = Image {
        w: region.w as usize,
        h: region.h as usize,
        px,
    };
    for f in stack {
        img = apply_one(&f.sanitized(), img);
    }
    let out: Vec<[u16; 4]> = img
        .px
        .into_iter()
        .map(|p| color::f_to_px(p.map(|v| v.clamp(0.0, 1.0))))
        .collect();
    source.write_rect(region, &out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dot() -> Raster {
        Raster::from_fn(40, 40, [0; 4], |x, y| {
            if (18..22).contains(&x) && (18..22).contains(&y) {
                [65535; 4]
            } else {
                [0; 4]
            }
        })
    }

    #[test]
    fn out_of_range_params_are_sanitized_before_kernels() {
        let tiny = Raster::from_fn(4, 4, [0; 4], |_, _| [30000, 20000, 10000, 65535]);
        let start = std::time::Instant::now();
        let hostile = [
            Filter::LensBlur { radius: 1e5 },
            Filter::MotionBlur {
                angle: f32::NAN,
                distance: 1e9,
            },
            Filter::GaussianBlur { radius: f32::NAN },
            Filter::GaussianBlur {
                radius: f32::INFINITY,
            },
        ];
        let (out, _) = apply_stack(&tiny, &hostile);
        assert!(out.width() > 0);
        let region = apply_region(&tiny, &hostile, tiny.bounds());
        assert_eq!(region.width(), 4);
        assert!(start.elapsed() < std::time::Duration::from_secs(10));
        assert_eq!(
            Filter::LensBlur { radius: 1e5 }.sanitized(),
            Filter::LensBlur { radius: 40.0 }
        );
        assert_eq!(
            Filter::GaussianBlur { radius: f32::NAN }.sanitized(),
            Filter::GaussianBlur { radius: 5.0 }
        );
        for filter in Filter::catalogue() {
            assert_eq!(filter.sanitized(), filter, "in-range values are unchanged");
        }
    }

    #[test]
    fn owned_gaussian_matches_reference_at_edges_and_reuses_input() {
        for (w, h) in [(1, 1), (1, 9), (11, 1), (3, 5), (33, 27)] {
            for radius in [-1.0, 0.0, 0.05, 0.1, 1.0, 5.0, 20.0, f32::NAN] {
                let px = (0..w * h)
                    .map(|i| {
                        let a = (i % 7) as f32 / 6.0;
                        [a * 0.2, a * 0.5, a * 0.8, a]
                    })
                    .collect();
                let img = Image { w, h, px };
                let expected = gaussian(&img, radius);
                let allocation = img.px.as_ptr();
                let actual = apply_one_cpu_owned(&Filter::GaussianBlur { radius }, img);
                assert_eq!(actual.px.as_ptr(), allocation);
                assert_eq!(actual.px, expected.px, "{w}x{h}, radius {radius}");
            }
        }
    }

    #[test]
    fn gaussian_stack_and_region_match_borrowed_reference() {
        let source = dot();
        let stack = [
            Filter::GaussianBlur { radius: 2.0 },
            Filter::BoxBlur { radius: 1.0 },
            Filter::GaussianBlur { radius: 5.0 },
        ];
        let spread = stack.iter().map(Filter::spread).sum::<i32>();
        let mut reference = Image {
            w: 40,
            h: 40,
            px: source
                .read_rect(source.bounds())
                .into_iter()
                .map(color::px_to_f)
                .collect(),
        }
        .pad(spread as usize);
        for filter in &stack {
            reference = apply_one_cpu(filter, &reference);
        }
        let expected: Vec<_> = reference
            .px
            .iter()
            .map(|p| color::f_to_px(p.map(|v| v.clamp(0.0, 1.0))))
            .collect();
        let (actual, offset) = apply_stack(&source, &stack);
        assert_eq!(offset, (-spread, -spread));
        assert_eq!(actual.read_rect(actual.bounds()), expected);
        let region = IRect::new(5, 7, 13, 11);
        let mut reference = Image {
            w: 13,
            h: 11,
            px: source
                .read_rect(region)
                .into_iter()
                .map(color::px_to_f)
                .collect(),
        };
        for filter in &stack {
            reference = apply_one_cpu(filter, &reference);
        }
        let expected: Vec<_> = reference
            .px
            .iter()
            .map(|p| color::f_to_px(p.map(|v| v.clamp(0.0, 1.0))))
            .collect();
        let expected = source.write_rect(region, &expected);
        let actual = apply_region(&source, &stack, region);
        assert_eq!(
            actual.read_rect(actual.bounds()),
            expected.read_rect(expected.bounds())
        );
    }

    #[test]
    fn catalogue_params_round_trip() {
        for mut f in Filter::catalogue() {
            for spec in f.params() {
                assert!(
                    f.set_param(spec.key, spec.value),
                    "{} {}",
                    f.label(),
                    spec.key
                );
            }
            assert!(!f.set_param("nope", 1.0));
            let json = serde_json::to_string(&f).unwrap();
            let back: Filter = serde_json::from_str(&json).unwrap();
            assert_eq!(back, f);
        }
    }

    #[test]
    fn blur_spreads_past_the_edge_and_keeps_energy() {
        let src = dot();
        let (out, off) = apply_stack(&src, &[Filter::GaussianBlur { radius: 6.0 }]);
        assert!(out.width() > 40 && off.0 < 0);
        let sum = |r: &Raster| {
            r.read_rect(r.bounds())
                .iter()
                .map(|p| p[3] as f64)
                .sum::<f64>()
        };
        assert!(
            (sum(&out) / sum(&src) - 1.0).abs() < 0.02,
            "alpha is conserved"
        );
        // The centre is dimmer, the surroundings brighter.
        let c = (20 - off.0) as u32;
        assert!(out.get(c, c)[3] < 65535 && out.get(c + 5, c)[3] > 0);
    }

    #[test]
    fn sharpen_high_pass_edges_and_warps_run() {
        let src = Raster::from_fn(64, 64, [0; 4], |x, _| {
            if x < 32 {
                [20000, 20000, 20000, 65535]
            } else {
                [50000, 50000, 50000, 65535]
            }
        });
        let (sharp, off) = apply_stack(
            &src,
            &[Filter::UnsharpMask {
                amount: 200.0,
                radius: 2.0,
                threshold: 0.0,
            }],
        );
        // The result spread out; index it in source coordinates.
        let at = |x: i32, y: i32| sharp.get((x - off.0) as u32, (y - off.1) as u32);
        assert!(
            at(33, 32)[0] > 50000 && at(30, 32)[0] < 20000,
            "edge overshoots both ways: {:?} {:?}",
            at(33, 32),
            at(30, 32)
        );
        let (hp, _) = apply_stack(&src, &[Filter::HighPass { radius: 5.0 }]);
        let flat = color::px_to_f(hp.get(5, 5));
        assert!(
            (color::linear_to_srgb(flat[0]) - 0.5).abs() < 0.03,
            "flat areas go mid grey: {flat:?}"
        );
        let (edges, _) = apply_stack(&src, &[Filter::FindEdges]);
        assert!(
            edges.get(32, 32)[0] < edges.get(5, 5)[0],
            "edges dark, flat areas light"
        );
        for f in [
            Filter::Pinch { amount: 60.0 },
            Filter::Twirl { angle: 120.0 },
            Filter::Wave {
                amplitude: 5.0,
                wavelength: 20.0,
            },
            Filter::LensCorrection {
                distortion: 40.0,
                vignette: 50.0,
            },
            Filter::Emboss {
                angle: 135.0,
                height: 2.0,
                amount: 100.0,
            },
            Filter::MotionBlur {
                angle: 30.0,
                distance: 8.0,
            },
            Filter::LensBlur { radius: 3.0 },
            Filter::AddNoise {
                amount: 20.0,
                monochrome: false,
            },
            Filter::ReduceNoise {
                strength: 5.0,
                detail: 50.0,
            },
            Filter::BoxBlur { radius: 3.0 },
            Filter::SmartSharpen {
                amount: 100.0,
                radius: 1.0,
            },
        ] {
            let (o, _) = apply_stack(&src, std::slice::from_ref(&f));
            assert!(o.width() >= 64, "{}", f.label());
        }
        let region = apply_region(
            &src,
            &[Filter::GaussianBlur { radius: 3.0 }],
            IRect::new(0, 0, 64, 32),
        );
        assert_eq!(
            region.get(32, 50),
            src.get(32, 50),
            "outside the region is untouched"
        );
        assert_ne!(region.get(32, 10), src.get(32, 10));
    }

    #[test]
    fn smart_filter_style_controls_stage_opacity_and_blend() {
        let src = Raster::from_fn(24, 24, [0; 4], |x, _| {
            if x < 12 {
                [16000, 22000, 32000, 65535]
            } else {
                [50000, 42000, 25000, 65535]
            }
        });
        let filters = [Filter::HighPass { radius: 3.0 }];
        let (legacy, legacy_offset) = apply_stack(&src, &filters);
        let (normal, normal_offset) = apply_stack_styled(&src, &filters, &[FilterStyle::default()]);
        assert_eq!(legacy_offset, normal_offset);
        assert_eq!(legacy.to_srgba8(), normal.to_srgba8());

        let (hidden, hidden_offset) = apply_stack_styled(
            &src,
            &filters,
            &[FilterStyle {
                opacity: 0.0,
                blend: BlendMode::SoftLight,
            }],
        );
        for y in 0..src.height() {
            for x in 0..src.width() {
                assert_eq!(
                    hidden.get(
                        (x as i32 - hidden_offset.0) as u32,
                        (y as i32 - hidden_offset.1) as u32
                    ),
                    src.get(x, y)
                );
            }
        }
        let (soft, _) = apply_stack_styled(
            &src,
            &filters,
            &[FilterStyle {
                opacity: 0.65,
                blend: BlendMode::SoftLight,
            }],
        );
        assert_ne!(soft.to_srgba8(), legacy.to_srgba8());
    }

    const PHOTO_KEYS: [&str; 9] = [
        "enhance",
        "structure",
        "glow",
        "orton",
        "sunrays",
        "atmosphere",
        "skin_smooth",
        "golden_hour",
        "dramatic",
    ];

    fn photo_filter(key: &str) -> Filter {
        Filter::catalogue()
            .into_iter()
            .find(|f| f.key() == key)
            .unwrap()
    }

    fn with(key: &str, params: &[(&str, f32)]) -> Filter {
        let mut f = photo_filter(key);
        for (k, v) in params {
            assert!(f.set_param(k, *v), "{key} {k}");
        }
        f
    }

    /// Opaque image from unpremultiplied linear colour.
    fn opaque(w: usize, h: usize, f: impl Fn(usize, usize) -> [f32; 3]) -> Image {
        let px = (0..w * h)
            .map(|i| {
                let c = f(i % w, i / w);
                [c[0], c[1], c[2], 1.0]
            })
            .collect();
        Image { w, h, px }
    }

    /// Opaque image from sRGB-encoded colour.
    fn opaque_srgb(w: usize, h: usize, f: impl Fn(usize, usize) -> [f32; 3]) -> Image {
        opaque(w, h, |x, y| {
            f(x, y).map(|v| color::srgb_to_linear(v.clamp(0.0, 1.0)))
        })
    }

    fn encoded(p: [f32; 4]) -> [f32; 3] {
        [0, 1, 2].map(|k| color::linear_to_srgb((p[k] / p[3].max(1e-6)).clamp(0.0, 1.0)))
    }

    fn perceptual(p: [f32; 4]) -> f32 {
        color::linear_to_srgb(luma(p).clamp(0.0, 1.0))
    }

    fn std_dev(v: impl Iterator<Item = f32>) -> f32 {
        let v: Vec<f32> = v.collect();
        let mean = v.iter().sum::<f32>() / v.len() as f32;
        (v.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / v.len() as f32).sqrt()
    }

    fn random_image(w: usize, h: usize) -> Image {
        let px = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                let a = [0.0, 0.25, 1.0, 1.0, 0.7][(x + 3 * y) % 5];
                [0, 1, 2]
                    .map(|k| (hash(x, y, 7 + k as u32) + 0.5) * a)
                    .into_iter()
                    .chain([a])
                    .collect::<Vec<_>>()
                    .try_into()
                    .unwrap()
            })
            .collect();
        Image { w, h, px }
    }

    #[test]
    fn photo_filters_are_in_the_catalogue_with_expected_labels() {
        let labels: Vec<_> = PHOTO_KEYS.iter().map(|k| photo_filter(k).label()).collect();
        assert_eq!(
            labels,
            [
                "Enhance",
                "Structure",
                "Glow",
                "Mystical",
                "Sunrays",
                "Atmosphere",
                "Skin smoothing",
                "Golden hour",
                "Dramatic"
            ]
        );
        assert_eq!(Filter::catalogue().len(), 25);
        for key in PHOTO_KEYS {
            assert_eq!(photo_filter(key).spread(), 0, "{key}");
        }
        let json = serde_json::to_string(&photo_filter("skin_smooth")).unwrap();
        assert!(json.contains("\"kind\":\"skin-smooth\""), "{json}");
    }

    #[test]
    fn photo_filters_are_identity_at_zero_amount() {
        let img = random_image(23, 17);
        for key in PHOTO_KEYS {
            let f = with(key, &[("amount", 0.0)]);
            let out = apply_one_cpu(&f, &img);
            assert_eq!(out.px, img.px, "{key}");
        }
    }

    #[test]
    fn photo_filters_keep_size_alpha_and_finite_values() {
        for (w, h) in [(1, 1), (1, 9), (11, 1), (50, 37)] {
            let img = random_image(w, h);
            for key in PHOTO_KEYS {
                let base = photo_filter(key);
                let mut variants = vec![base.clone()];
                for pick_max in [false, true] {
                    let mut f = base.clone();
                    for spec in base.params() {
                        f.set_param(spec.key, if pick_max { spec.max } else { spec.min });
                    }
                    f.set_param("amount", if pick_max { 100.0 } else { -100.0 });
                    variants.push(f);
                }
                for f in variants {
                    let out = apply_one_cpu(&f, &img);
                    assert_eq!((out.w, out.h), (w, h), "{f:?}");
                    for (o, p) in out.px.iter().zip(&img.px) {
                        assert!(o.iter().all(|v| v.is_finite()), "{f:?}: {o:?}");
                        assert_eq!(o[3], p[3], "{f:?} alpha");
                        for v in &o[..3] {
                            assert!(*v >= 0.0 && *v <= o[3] + 1e-5, "{f:?}: {o:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn structure_raises_local_contrast_and_negative_softens() {
        let img = opaque_srgb(96, 96, |x, y| {
            let v = 0.5 + 0.15 * (x as f32 / 2.0).sin() * (y as f32 / 2.0).sin();
            [v, v * 0.9, v * 0.8]
        });
        let sd = |img: &Image| std_dev(img.px.iter().map(|p| perceptual(*p)));
        let before = sd(&img);
        let up = apply_one_cpu(&with("structure", &[("amount", 100.0)]), &img);
        let down = apply_one_cpu(&with("structure", &[("amount", -100.0)]), &img);
        assert!(sd(&up) > before * 1.1, "{} vs {before}", sd(&up));
        assert!(sd(&down) < before * 0.9, "{} vs {before}", sd(&down));
    }

    #[test]
    fn atmosphere_adds_and_removes_haze() {
        let hazy = opaque_srgb(80, 60, |x, y| {
            let c = [0, 1, 2].map(|k| hash(x / 4, y / 4, k) + 0.5);
            c.map(|v| v * 0.4 + 0.8 * 0.6)
        });
        let dark = |img: &Image| {
            img.px
                .iter()
                .map(|p| {
                    let e = encoded(*p);
                    e[0].min(e[1]).min(e[2])
                })
                .sum::<f32>()
                / img.px.len() as f32
        };
        let before = dark(&hazy);
        let more = apply_one_cpu(&with("atmosphere", &[("amount", 80.0)]), &hazy);
        let less = apply_one_cpu(&with("atmosphere", &[("amount", -80.0)]), &hazy);
        assert!(dark(&more) > before + 0.02, "{} vs {before}", dark(&more));
        assert!(dark(&less) < before - 0.05, "{} vs {before}", dark(&less));
    }

    #[test]
    fn glow_brightens_around_a_bright_spot() {
        let img = opaque(128, 128, |x, y| {
            if (60..68).contains(&x) && (60..68).contains(&y) {
                [1.0; 3]
            } else {
                [0.02; 3]
            }
        });
        let out = apply_one_cpu(&with("glow", &[("amount", 100.0), ("radius", 100.0)]), &img);
        let at = |img: &Image, x: usize, y: usize| img.px[y * 128 + x][1];
        assert!(at(&out, 71, 64) > at(&img, 71, 64) + 0.01);
        assert!(
            (at(&out, 5, 120) - at(&img, 5, 120)).abs() < 1e-3,
            "far away stays dark"
        );
    }

    #[test]
    fn sunrays_brighten_along_the_ray() {
        let img = opaque(128, 128, |x, y| {
            let (dx, dy) = (x as f32 - 64.0, y as f32 - 40.0);
            if dx * dx + dy * dy <= 100.0 {
                [1.0; 3]
            } else {
                [0.02; 3]
            }
        });
        let f = with("sunrays", &[("x", 50.0), ("y", 8.0), ("amount", 100.0)]);
        let out = apply_one_cpu(&f, &img);
        let gain = |x: usize, y: usize| out.px[y * 128 + x][0] - img.px[y * 128 + x][0];
        let (on, off) = (gain(64, 90), gain(121, 67));
        assert!(on > 0.02 && on > off * 2.0, "on {on}, off {off}");
    }

    #[test]
    fn skin_smoothing_touches_skin_only() {
        let noisy = |base: [f32; 3]| {
            opaque_srgb(64, 64, move |x, y| {
                let n = hash(x, y, 3) * 0.1;
                base.map(|v| v + n)
            })
        };
        let sd = |img: &Image| std_dev(img.px.iter().map(|p| perceptual(*p)));
        let f = with("skin_smooth", &[("amount", 100.0), ("detail", 0.0)]);
        let skin = noisy([224.0 / 255.0, 172.0 / 255.0, 140.0 / 255.0]);
        let smoothed = apply_one_cpu(&f, &skin);
        assert!(
            sd(&smoothed) < sd(&skin) * 0.7,
            "{} vs {}",
            sd(&smoothed),
            sd(&skin)
        );
        let blue = noisy([60.0 / 255.0, 90.0 / 255.0, 200.0 / 255.0]);
        assert_eq!(apply_one_cpu(&f, &blue).px, blue.px);
    }

    #[test]
    fn golden_hour_warms() {
        let img = opaque_srgb(16, 16, |x, _| [0.2 + x as f32 * 0.04; 3]);
        let out = apply_one_cpu(&photo_filter("golden_hour"), &img);
        let ratio = |img: &Image| {
            let (r, b) = img
                .px
                .iter()
                .fold((0.0, 0.0), |(r, b), p| (r + p[0], b + p[2]));
            r / b
        };
        assert!(ratio(&out) > ratio(&img) * 1.1);
    }

    #[test]
    fn enhance_widens_a_flat_image() {
        let img = opaque_srgb(64, 48, |x, y| {
            let v = 0.4 + 0.2 * (x + y) as f32 / 110.0;
            [v * 1.05, v, v * 0.9]
        });
        let range = |img: &Image| {
            let l: Vec<f32> = img.px.iter().map(|p| perceptual(*p)).collect();
            l.iter().copied().fold(f32::MIN, f32::max) - l.iter().copied().fold(f32::MAX, f32::min)
        };
        let out = apply_one_cpu(&with("enhance", &[("amount", 100.0)]), &img);
        assert!(
            range(&out) > range(&img) * 1.3,
            "{} vs {}",
            range(&out),
            range(&img)
        );
    }

    #[test]
    fn dramatic_desaturates() {
        let img = opaque_srgb(48, 48, |x, y| {
            [0.8, 0.3 + x as f32 * 0.005, 0.2 + y as f32 * 0.004]
        });
        let sat = |img: &Image| {
            img.px
                .iter()
                .map(|p| {
                    let e = encoded(*p);
                    let mx = e[0].max(e[1]).max(e[2]);
                    (mx - e[0].min(e[1]).min(e[2])) / mx.max(1e-6)
                })
                .sum::<f32>()
        };
        let out = apply_one_cpu(&with("dramatic", &[("amount", 100.0)]), &img);
        assert!(sat(&out) < sat(&img) * 0.9);
    }

    #[test]
    fn mystical_softens_edges() {
        let img = opaque_srgb(64, 64, |x, _| if x < 32 { [0.2; 3] } else { [0.7; 3] });
        let out = apply_one_cpu(&with("orton", &[("amount", 100.0), ("radius", 60.0)]), &img);
        let jump =
            |img: &Image| perceptual(img.px[10 * 64 + 32]) - perceptual(img.px[10 * 64 + 31]);
        assert!(
            jump(&out) < jump(&img) * 0.9,
            "{} vs {}",
            jump(&out),
            jump(&img)
        );
    }

    #[test]
    #[ignore = "manual 24-megapixel timing of the photo filters (run in release)"]
    fn benchmark_photo_filters_24mp() {
        let img = opaque(6000, 4000, |x, y| {
            [0, 1, 2].map(|k| (hash(x / 8, y / 8, k) + 0.5) * 0.8 + 0.1)
        });
        for key in PHOTO_KEYS {
            let f = photo_filter(key);
            let start = std::time::Instant::now();
            let out = apply_one_cpu(&f, &img);
            assert_eq!(out.px.len(), img.px.len());
            println!(
                "PHOTO_BENCH {key}: {:.0} ms",
                start.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
}
