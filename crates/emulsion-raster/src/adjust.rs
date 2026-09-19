//! Per-pixel adjustments.
//!
//! An [`Adjustment`] is plain data with named parameters, so the UI can show
//! it as sliders and the file format can store it. [`Adjustment::prepare`]
//! turns it into a [`Prepared`] operator with lookup tables for rendering.
//!
//! Adjustments that Photoshop defines on encoded values (levels, brightness /
//! contrast, hue / saturation) run on sRGB-encoded values; exposure and white
//! balance run in linear light.

use crate::color::{linear_to_srgb, srgb_to_linear};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Adjustment {
    Exposure { exposure: f32, offset: f32, gamma: f32 },
    BrightnessContrast { brightness: f32, contrast: f32 },
    Levels { in_black: f32, in_white: f32, gamma: f32, out_black: f32, out_white: f32 },
    HueSaturation { hue: f32, saturation: f32, lightness: f32 },
    WhiteBalance { temperature: f32, tint: f32 },
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
            "" if self.step < 1.0 => format!("{:.2}", self.value),
            u => format!("{:.0}{u}", self.value),
        }
    }
}

impl Adjustment {
    /// Every kind with neutral parameters, for "add adjustment" menus.
    pub fn catalogue() -> Vec<Adjustment> {
        vec![
            Adjustment::Exposure { exposure: 0.0, offset: 0.0, gamma: 1.0 },
            Adjustment::BrightnessContrast { brightness: 0.0, contrast: 0.0 },
            Adjustment::Levels { in_black: 0.0, in_white: 255.0, gamma: 1.0, out_black: 0.0, out_white: 255.0 },
            Adjustment::HueSaturation { hue: 0.0, saturation: 0.0, lightness: 0.0 },
            Adjustment::WhiteBalance { temperature: 0.0, tint: 0.0 },
            Adjustment::Invert,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Adjustment::Exposure { .. } => "Exposure",
            Adjustment::BrightnessContrast { .. } => "Brightness / Contrast",
            Adjustment::Levels { .. } => "Levels",
            Adjustment::HueSaturation { .. } => "Hue / Saturation",
            Adjustment::WhiteBalance { .. } => "White balance",
            Adjustment::Invert => "Invert",
        }
    }

    pub fn params(&self) -> Vec<ParamSpec> {
        let p = |key, label, min, max, step, value, unit| ParamSpec { key, label, min, max, step, value, unit };
        match *self {
            Adjustment::Exposure { exposure, offset, gamma } => vec![
                p("exposure", "exposure", -5.0, 5.0, 0.01, exposure, "ev"),
                p("offset", "offset", -0.5, 0.5, 0.001, offset, ""),
                p("gamma", "gamma", 0.1, 3.0, 0.01, gamma, ""),
            ],
            Adjustment::BrightnessContrast { brightness, contrast } => vec![
                p("brightness", "brightness", -150.0, 150.0, 1.0, brightness, ""),
                p("contrast", "contrast", -50.0, 100.0, 1.0, contrast, ""),
            ],
            Adjustment::Levels { in_black, in_white, gamma, out_black, out_white } => vec![
                p("in_black", "input black", 0.0, 253.0, 1.0, in_black, ""),
                p("in_white", "input white", 2.0, 255.0, 1.0, in_white, ""),
                p("gamma", "midtones", 0.1, 9.99, 0.01, gamma, ""),
                p("out_black", "output black", 0.0, 255.0, 1.0, out_black, ""),
                p("out_white", "output white", 0.0, 255.0, 1.0, out_white, ""),
            ],
            Adjustment::HueSaturation { hue, saturation, lightness } => vec![
                p("hue", "hue", -180.0, 180.0, 1.0, hue, "°"),
                p("saturation", "saturation", -100.0, 100.0, 1.0, saturation, ""),
                p("lightness", "lightness", -100.0, 100.0, 1.0, lightness, ""),
            ],
            Adjustment::WhiteBalance { temperature, tint } => vec![
                p("temperature", "warmth", -100.0, 100.0, 1.0, temperature, ""),
                p("tint", "tint", -100.0, 100.0, 1.0, tint, ""),
            ],
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
            (Adjustment::WhiteBalance { temperature, .. }, "temperature") => temperature,
            (Adjustment::WhiteBalance { tint, .. }, "tint") => tint,
            _ => return false,
        };
        *slot = v;
        true
    }

    /// Build the render-time operator.
    pub fn prepare(&self) -> Prepared {
        match *self {
            Adjustment::HueSaturation { hue, saturation, lightness } => Prepared::HueSat {
                hue: hue / 360.0,
                sat: saturation / 100.0,
                light: lightness / 100.0,
            },
            _ => {
                let f = |ch: usize, l: f32| self.channel(ch, l);
                Prepared::Lut(Box::new([build_lut(|l| f(0, l)), build_lut(|l| f(1, l)), build_lut(|l| f(2, l))]))
            }
        }
    }

    /// Channel-wise transfer on linear input for LUT-able kinds.
    fn channel(&self, ch: usize, l: f32) -> f32 {
        match *self {
            Adjustment::Exposure { exposure, offset, gamma } => {
                let v = l * 2f32.powf(exposure) + offset;
                v.max(0.0).powf(1.0 / gamma.max(0.01))
            }
            Adjustment::BrightnessContrast { brightness, contrast } => {
                // Photoshop's modern (non-legacy) curve approximated on encoded values.
                let e = linear_to_srgb(l);
                let b = brightness / 150.0;
                let e = if b >= 0.0 { e + (1.0 - e) * b * e.sqrt().min(1.0) } else { e * (1.0 + b) };
                let c = contrast / 100.0;
                let k = if c >= 0.0 { 1.0 + c * 2.0 } else { 1.0 + c };
                let e = ((e - 0.5) * k + 0.5).clamp(0.0, 1.0);
                srgb_to_linear(e)
            }
            Adjustment::Levels { in_black, in_white, gamma, out_black, out_white } => {
                let e = linear_to_srgb(l) * 255.0;
                let t = ((e - in_black) / (in_white - in_black).max(1.0)).clamp(0.0, 1.0);
                let t = t.powf(1.0 / gamma.max(0.01));
                let o = (out_black + t * (out_white - out_black)) / 255.0;
                srgb_to_linear(o.clamp(0.0, 1.0))
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
            Adjustment::Invert => srgb_to_linear(1.0 - linear_to_srgb(l)),
            Adjustment::HueSaturation { .. } => l,
        }
    }
}

const LUT_N: usize = 4096;

fn build_lut(f: impl Fn(f32) -> f32) -> Vec<f32> {
    (0..LUT_N).map(|i| f(i as f32 / (LUT_N - 1) as f32)).collect()
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

/// Render-time operator.
pub enum Prepared {
    Lut(Box<[Vec<f32>; 3]>),
    HueSat { hue: f32, sat: f32, light: f32 },
}

impl Prepared {
    /// Apply to unpremultiplied linear RGB.
    #[inline]
    pub fn apply(&self, c: [f32; 3]) -> [f32; 3] {
        match self {
            Prepared::Lut(t) => [lut(&t[0], c[0]), lut(&t[1], c[1]), lut(&t[2], c[2])],
            Prepared::HueSat { hue, sat, light } => {
                let e = [linear_to_srgb(c[0].clamp(0.0, 1.0)), linear_to_srgb(c[1].clamp(0.0, 1.0)), linear_to_srgb(c[2].clamp(0.0, 1.0))];
                let (mut h, mut s, mut l) = rgb_to_hsl(e);
                h = (h + hue).rem_euclid(1.0);
                s = if *sat >= 0.0 { s + (1.0 - s) * sat * s.max(0.0001).sqrt().min(1.0) } else { s * (1.0 + sat) };
                l = if *light >= 0.0 { l + (1.0 - l) * light } else { l * (1.0 + light) };
                let o = hsl_to_rgb(h, s.clamp(0.0, 1.0), l.clamp(0.0, 1.0));
                [srgb_to_linear(o[0]), srgb_to_linear(o[1]), srgb_to_linear(o[2])]
            }
        }
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
    let s = if l > 0.5 { d / (2.0 - mx - mn) } else { d / (mx + mn) };
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
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
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
        let samples = [[0.0, 0.0, 0.0], [0.18, 0.5, 0.9], [1.0, 1.0, 1.0], [0.01, 0.02, 0.03]];
        for adj in Adjustment::catalogue() {
            if adj == Adjustment::Invert {
                continue;
            }
            let p = adj.prepare();
            for c in samples {
                assert!(close(p.apply(c), c, 2e-3), "{} not neutral at {c:?}: {:?}", adj.label(), p.apply(c));
            }
        }
    }

    #[test]
    fn exposure_one_stop_doubles() {
        let p = Adjustment::Exposure { exposure: 1.0, offset: 0.0, gamma: 1.0 }.prepare();
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
        let p = Adjustment::HueSaturation { hue: 180.0, saturation: 0.0, lightness: 0.0 }.prepare();
        let o = p.apply([1.0, 0.0, 0.0]);
        assert!(close(o, [0.0, 1.0, 1.0], 1e-3), "{o:?}");
    }

    #[test]
    fn set_param_clamps_and_rejects_unknown() {
        let mut a = Adjustment::Exposure { exposure: 0.0, offset: 0.0, gamma: 1.0 };
        assert!(a.set_param("exposure", 99.0));
        assert_eq!(a.params()[0].value, 5.0);
        assert!(!a.set_param("nope", 1.0));
    }
}
