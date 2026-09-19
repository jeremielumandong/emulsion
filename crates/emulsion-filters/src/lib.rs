//! `emulsion-filters` — pixel filters: blurs, sharpening, noise, high pass,
//! lens correction, and a few distort and stylize effects.
//!
//! A [`Filter`] is plain data with named parameters, like an adjustment.
//! [`apply_stack`] runs a stack over a layer's pixels and returns a new
//! raster that may be larger than the source, because blurs spread past
//! the layer's edges; the offset says where the result sits relative to
//! the source. Smart layers keep the source and the stack and re-run this
//! when a parameter changes.

use emulsion_raster::{IRect, Raster, color};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

pub const CRATE: &str = "emulsion-filters";

/// The most a filter may spread past the layer, in pixels.
pub const MAX_SPREAD: i32 = 250;

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
            Filter::Emboss { .. } => "Emboss",
            Filter::FindEdges => "Find edges",
            Filter::Pinch { .. } => "Pinch",
            Filter::Twirl { .. } => "Twirl",
            Filter::Wave { .. } => "Wave",
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
            Filter::Emboss { .. } => "emboss",
            Filter::FindEdges => "find_edges",
            Filter::Pinch { .. } => "pinch",
            Filter::Twirl { .. } => "twirl",
            Filter::Wave { .. } => "wave",
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
            (Filter::Emboss { angle, .. }, "angle") => angle,
            (Filter::Emboss { height, .. }, "height") => height,
            (Filter::Emboss { amount, .. }, "amount") => amount,
            (Filter::Pinch { amount }, "amount") => amount,
            (Filter::Twirl { angle }, "angle") => angle,
            (Filter::Wave { amplitude, .. }, "amplitude") => amplitude,
            (Filter::Wave { wavelength, .. }, "wavelength") => wavelength,
            _ => return false,
        };
        *slot = v;
        true
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
            Filter::Pinch { .. } | Filter::Twirl { .. } | Filter::LensCorrection { .. } => 0.0,
            _ => 0.0,
        };
        (s.ceil() as i32).clamp(0, MAX_SPREAD)
    }
}

/// Dense premultiplied linear image with its own origin.
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
    let r = (kernel.len() / 2) as i64;
    let w = img.w;
    let px: Vec<[f32; 4]> = (0..img.w * img.h)
        .into_par_iter()
        .map(|i| {
            let (x, y) = ((i % w) as i64, (i / w) as i64);
            let mut acc = [0.0f32; 4];
            for (k, wgt) in kernel.iter().enumerate() {
                let o = k as i64 - r;
                let p = if horizontal {
                    img.get(x + o, y)
                } else {
                    img.get(x, y + o)
                };
                for c in 0..4 {
                    acc[c] += p[c] * wgt;
                }
            }
            acc
        })
        .collect();
    Image { w, h: img.h, px }
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

fn apply_one(f: &Filter, img: &Image) -> Image {
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
    }
}

/// Run `stack` over `source`. Returns the filtered raster and where its
/// top-left sits relative to the source (negative when it spread out).
pub fn apply_stack(source: &Raster, stack: &[Filter]) -> (Raster, (i32, i32)) {
    if stack.is_empty() {
        return (source.clone(), (0, 0));
    }
    let spread: i32 = stack
        .iter()
        .map(Filter::spread)
        .sum::<i32>()
        .min(MAX_SPREAD);
    let (w, h) = (source.width() as usize, source.height() as usize);
    let px: Vec<[f32; 4]> = source
        .read_rect(source.bounds())
        .into_iter()
        .map(color::px_to_f)
        .collect();
    let mut img = Image { w, h, px }.pad(spread as usize);
    for f in stack {
        img = apply_one(f, &img);
    }
    let out: Vec<[u16; 4]> = img
        .px
        .into_iter()
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
        img = apply_one(f, &img);
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
}
