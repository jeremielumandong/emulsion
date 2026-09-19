//! Blend modes.
//!
//! Every Photoshop mode, implemented with the W3C Compositing formulas:
//!
//! ```text
//! Cs' = (1 − αb)·Cs + αb·B(Cb, Cs)
//! co  = αs·Cs' + (1 − αs)·αb·Cb          (premultiplied result)
//! αo  = αs + αb·(1 − αs)
//! ```
//!
//! `B` runs on unpremultiplied colour in either linear light (default) or
//! sRGB-encoded values (the compatibility space most other editors use).

use crate::color::{linear_to_srgb, srgb_to_linear};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BlendMode {
    /// Groups only: children blend straight into the backdrop.
    PassThrough,
    #[default]
    Normal,
    Dissolve,
    Darken,
    Multiply,
    ColorBurn,
    LinearBurn,
    DarkerColor,
    Lighten,
    Screen,
    ColorDodge,
    LinearDodge,
    LighterColor,
    Overlay,
    SoftLight,
    HardLight,
    VividLight,
    LinearLight,
    PinLight,
    HardMix,
    Difference,
    Exclusion,
    Subtract,
    Divide,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

/// Where blend math runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BlendSpace {
    #[default]
    Linear,
    Srgb,
}

impl BlendMode {
    /// Modes in menu order, with separators between Photoshop's groups
    /// expressed as `None`.
    pub const MENU: &'static [Option<BlendMode>] = &[
        Some(BlendMode::Normal),
        Some(BlendMode::Dissolve),
        None,
        Some(BlendMode::Darken),
        Some(BlendMode::Multiply),
        Some(BlendMode::ColorBurn),
        Some(BlendMode::LinearBurn),
        Some(BlendMode::DarkerColor),
        None,
        Some(BlendMode::Lighten),
        Some(BlendMode::Screen),
        Some(BlendMode::ColorDodge),
        Some(BlendMode::LinearDodge),
        Some(BlendMode::LighterColor),
        None,
        Some(BlendMode::Overlay),
        Some(BlendMode::SoftLight),
        Some(BlendMode::HardLight),
        Some(BlendMode::VividLight),
        Some(BlendMode::LinearLight),
        Some(BlendMode::PinLight),
        Some(BlendMode::HardMix),
        None,
        Some(BlendMode::Difference),
        Some(BlendMode::Exclusion),
        Some(BlendMode::Subtract),
        Some(BlendMode::Divide),
        None,
        Some(BlendMode::Hue),
        Some(BlendMode::Saturation),
        Some(BlendMode::Color),
        Some(BlendMode::Luminosity),
    ];

    pub fn label(self) -> &'static str {
        use BlendMode::*;
        match self {
            PassThrough => "pass through",
            Normal => "normal",
            Dissolve => "dissolve",
            Darken => "darken",
            Multiply => "multiply",
            ColorBurn => "color burn",
            LinearBurn => "linear burn",
            DarkerColor => "darker color",
            Lighten => "lighten",
            Screen => "screen",
            ColorDodge => "color dodge",
            LinearDodge => "linear dodge",
            LighterColor => "lighter color",
            Overlay => "overlay",
            SoftLight => "soft light",
            HardLight => "hard light",
            VividLight => "vivid light",
            LinearLight => "linear light",
            PinLight => "pin light",
            HardMix => "hard mix",
            Difference => "difference",
            Exclusion => "exclusion",
            Subtract => "subtract",
            Divide => "divide",
            Hue => "hue",
            Saturation => "saturation",
            Color => "color",
            Luminosity => "luminosity",
        }
    }

    /// OpenRaster `composite-op`. Modes the ORA spec does not name use a
    /// vendor prefix; conforming readers fall back to `svg:src-over`.
    pub fn ora_op(self) -> &'static str {
        use BlendMode::*;
        match self {
            PassThrough | Normal => "svg:src-over",
            Multiply => "svg:multiply",
            Screen => "svg:screen",
            Overlay => "svg:overlay",
            Darken => "svg:darken",
            Lighten => "svg:lighten",
            ColorDodge => "svg:color-dodge",
            ColorBurn => "svg:color-burn",
            HardLight => "svg:hard-light",
            SoftLight => "svg:soft-light",
            Difference => "svg:difference",
            Exclusion => "svg:exclusion",
            Hue => "svg:hue",
            Saturation => "svg:saturation",
            Color => "svg:color",
            Luminosity => "svg:luminosity",
            LinearDodge => "svg:plus",
            Dissolve => "emulsion:dissolve",
            LinearBurn => "emulsion:linear-burn",
            DarkerColor => "emulsion:darker-color",
            LighterColor => "emulsion:lighter-color",
            VividLight => "emulsion:vivid-light",
            LinearLight => "emulsion:linear-light",
            PinLight => "emulsion:pin-light",
            HardMix => "emulsion:hard-mix",
            Subtract => "emulsion:subtract",
            Divide => "emulsion:divide",
        }
    }

    /// Parse an ORA `composite-op`, accepting the Krita names too.
    pub fn from_ora_op(op: &str) -> Option<BlendMode> {
        use BlendMode::*;
        let all = [
            Normal,
            Multiply,
            Screen,
            Overlay,
            Darken,
            Lighten,
            ColorDodge,
            ColorBurn,
            HardLight,
            SoftLight,
            Difference,
            Exclusion,
            Hue,
            Saturation,
            Color,
            Luminosity,
            LinearDodge,
            Dissolve,
            LinearBurn,
            DarkerColor,
            LighterColor,
            VividLight,
            LinearLight,
            PinLight,
            HardMix,
            Subtract,
            Divide,
        ];
        if let Some(m) = all.into_iter().find(|m| m.ora_op() == op) {
            return Some(m);
        }
        Some(match op {
            "krita:dissolve" => Dissolve,
            "krita:linear_burn" => LinearBurn,
            "krita:darker color" => DarkerColor,
            "krita:lighter color" => LighterColor,
            "krita:vivid_light" => VividLight,
            "krita:linear light" => LinearLight,
            "krita:pin_light" => PinLight,
            "krita:hard mix" | "krita:hard_mix_photoshop" => HardMix,
            "krita:subtract" => Subtract,
            "krita:divide" => Divide,
            "svg:add" => LinearDodge,
            _ => return None,
        })
    }
}

#[inline]
fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

#[inline]
fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut o = c;
    if n < 0.0 {
        for v in &mut o {
            *v = l + (*v - l) * l / (l - n).max(1e-9);
        }
    }
    if x > 1.0 {
        for v in &mut o {
            *v = l + (*v - l) * (1.0 - l) / (x - l).max(1e-9);
        }
    }
    o
}

#[inline]
fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color([c[0] + d, c[1] + d, c[2] + d])
}

#[inline]
fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

#[inline]
fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let mx = c[0].max(c[1]).max(c[2]);
    let mn = c[0].min(c[1]).min(c[2]);
    if mx - mn <= 1e-9 {
        return [0.0; 3];
    }
    let mut o = [0.0; 3];
    for i in 0..3 {
        o[i] = (c[i] - mn) * s / (mx - mn);
    }
    o
}

#[inline]
fn color_dodge(b: f32, s: f32) -> f32 {
    if b <= 0.0 {
        0.0
    } else if s >= 1.0 {
        1.0
    } else {
        (b / (1.0 - s)).min(1.0)
    }
}

#[inline]
fn color_burn(b: f32, s: f32) -> f32 {
    if b >= 1.0 {
        1.0
    } else if s <= 0.0 {
        0.0
    } else {
        1.0 - ((1.0 - b) / s).min(1.0)
    }
}

#[inline]
fn hard_light(b: f32, s: f32) -> f32 {
    if s <= 0.5 {
        b * 2.0 * s
    } else {
        let t = 2.0 * s - 1.0;
        b + t - b * t
    }
}

#[inline]
fn soft_light(b: f32, s: f32) -> f32 {
    if s <= 0.5 {
        b - (1.0 - 2.0 * s) * b * (1.0 - b)
    } else {
        let d = if b <= 0.25 {
            ((16.0 * b - 12.0) * b + 4.0) * b
        } else {
            b.sqrt()
        };
        b + (2.0 * s - 1.0) * (d - b)
    }
}

impl BlendMode {
    /// Whether `B` works channel by channel.
    pub fn is_separable(self) -> bool {
        !matches!(
            self,
            BlendMode::Hue
                | BlendMode::Saturation
                | BlendMode::Color
                | BlendMode::Luminosity
                | BlendMode::DarkerColor
                | BlendMode::LighterColor
        )
    }

    /// The blend function `B(Cb, Cs)` on unpremultiplied colour in [0,1].
    pub fn mix(self, cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
        use BlendMode::*;
        let sep = |f: &dyn Fn(f32, f32) -> f32| [f(cb[0], cs[0]), f(cb[1], cs[1]), f(cb[2], cs[2])];
        match self {
            PassThrough | Normal | Dissolve => cs,
            Darken => sep(&|b, s| b.min(s)),
            Multiply => sep(&|b, s| b * s),
            ColorBurn => sep(&color_burn),
            LinearBurn => sep(&|b, s| (b + s - 1.0).max(0.0)),
            Lighten => sep(&|b, s| b.max(s)),
            Screen => sep(&|b, s| b + s - b * s),
            ColorDodge => sep(&color_dodge),
            LinearDodge => sep(&|b, s| (b + s).min(1.0)),
            Overlay => sep(&|b, s| hard_light(s, b)),
            SoftLight => sep(&soft_light),
            HardLight => sep(&hard_light),
            VividLight => sep(&|b, s| {
                if s <= 0.5 {
                    color_burn(b, 2.0 * s)
                } else {
                    color_dodge(b, 2.0 * s - 1.0)
                }
            }),
            LinearLight => sep(&|b, s| (b + 2.0 * s - 1.0).clamp(0.0, 1.0)),
            PinLight => sep(&|b, s| {
                if s <= 0.5 {
                    b.min(2.0 * s)
                } else {
                    b.max(2.0 * s - 1.0)
                }
            }),
            HardMix => sep(&|b, s| if b + s >= 1.0 { 1.0 } else { 0.0 }),
            Difference => sep(&|b, s| (b - s).abs()),
            Exclusion => sep(&|b, s| b + s - 2.0 * b * s),
            Subtract => sep(&|b, s| (b - s).max(0.0)),
            Divide => sep(&|b, s| {
                if s <= 0.0 {
                    if b > 0.0 { 1.0 } else { 0.0 }
                } else {
                    (b / s).min(1.0)
                }
            }),
            DarkerColor => {
                if lum(cs) < lum(cb) {
                    cs
                } else {
                    cb
                }
            }
            LighterColor => {
                if lum(cs) > lum(cb) {
                    cs
                } else {
                    cb
                }
            }
            Hue => set_lum(set_sat(cs, sat(cb)), lum(cb)),
            Saturation => set_lum(set_sat(cb, sat(cs)), lum(cb)),
            Color => set_lum(cs, lum(cb)),
            Luminosity => set_lum(cb, lum(cs)),
        }
    }
}

/// Composite premultiplied `src` over premultiplied `dst` with `mode`.
///
/// `noise` in [0,1) drives Dissolve; pass any value for other modes.
#[inline]
pub fn blend_px(
    mode: BlendMode,
    space: BlendSpace,
    dst: [f32; 4],
    src: [f32; 4],
    noise: f32,
) -> [f32; 4] {
    let a_s = src[3];
    if a_s <= 0.0 {
        return dst;
    }
    match mode {
        BlendMode::Normal | BlendMode::PassThrough => {
            let k = 1.0 - a_s;
            return [
                src[0] + dst[0] * k,
                src[1] + dst[1] * k,
                src[2] + dst[2] * k,
                a_s + dst[3] * k,
            ];
        }
        BlendMode::Dissolve => {
            if noise >= a_s {
                return dst;
            }
            let inv = 1.0 / a_s;
            return [src[0] * inv, src[1] * inv, src[2] * inv, 1.0];
        }
        _ => {}
    }
    let a_b = dst[3];
    let inv_s = 1.0 / a_s;
    let cs = [
        (src[0] * inv_s).clamp(0.0, 1.0),
        (src[1] * inv_s).clamp(0.0, 1.0),
        (src[2] * inv_s).clamp(0.0, 1.0),
    ];
    let cb = if a_b > 0.0 {
        let inv_b = 1.0 / a_b;
        [
            (dst[0] * inv_b).clamp(0.0, 1.0),
            (dst[1] * inv_b).clamp(0.0, 1.0),
            (dst[2] * inv_b).clamp(0.0, 1.0),
        ]
    } else {
        [0.0; 3]
    };
    let b = match space {
        BlendSpace::Linear => mode.mix(cb, cs),
        BlendSpace::Srgb => {
            let e = |c: [f32; 3]| {
                [
                    linear_to_srgb(c[0]),
                    linear_to_srgb(c[1]),
                    linear_to_srgb(c[2]),
                ]
            };
            let m = mode.mix(e(cb), e(cs));
            [
                srgb_to_linear(m[0]),
                srgb_to_linear(m[1]),
                srgb_to_linear(m[2]),
            ]
        }
    };
    let mut o = [0.0; 4];
    for i in 0..3 {
        let mixed = (1.0 - a_b) * cs[i] + a_b * b[i];
        o[i] = a_s * mixed + (1.0 - a_s) * dst[i];
    }
    o[3] = a_s + a_b * (1.0 - a_s);
    o
}

/// Deterministic per-pixel noise in [0,1) for Dissolve.
#[inline]
pub fn dissolve_noise(x: i32, y: i32, seed: u64) -> f32 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ seed;
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    (h >> 40) as f32 / (1u64 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    const L: BlendSpace = BlendSpace::Linear;

    #[test]
    fn normal_is_src_over() {
        let o = blend_px(
            BlendMode::Normal,
            L,
            [0.2, 0.2, 0.2, 1.0],
            [0.25, 0.0, 0.0, 0.5],
            0.0,
        );
        assert!((o[0] - 0.35).abs() < 1e-6 && (o[3] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn opaque_modes_match_reference() {
        let b = [0.25, 0.5, 0.75];
        let s = [0.5, 0.5, 0.5];
        let cases: &[(BlendMode, [f32; 3])] = &[
            (BlendMode::Multiply, [0.125, 0.25, 0.375]),
            (BlendMode::Screen, [0.625, 0.75, 0.875]),
            (BlendMode::Darken, [0.25, 0.5, 0.5]),
            (BlendMode::Lighten, [0.5, 0.5, 0.75]),
            (BlendMode::Difference, [0.25, 0.0, 0.25]),
            (BlendMode::LinearDodge, [0.75, 1.0, 1.0]),
            (BlendMode::LinearBurn, [0.0, 0.0, 0.25]),
            (BlendMode::Subtract, [0.0, 0.0, 0.25]),
            (BlendMode::HardMix, [0.0, 1.0, 1.0]),
        ];
        for (m, want) in cases {
            let o = blend_px(*m, L, [b[0], b[1], b[2], 1.0], [s[0], s[1], s[2], 1.0], 0.0);
            for i in 0..3 {
                assert!(
                    (o[i] - want[i]).abs() < 1e-5,
                    "{m:?} ch{i}: {} vs {}",
                    o[i],
                    want[i]
                );
            }
        }
    }

    #[test]
    fn luminosity_keeps_backdrop_hue() {
        let o = BlendMode::Luminosity.mix([1.0, 0.0, 0.0], [0.5, 0.5, 0.5]);
        assert!(o[0] > o[1] && (lum(o) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn transparent_backdrop_shows_source() {
        for m in BlendMode::MENU.iter().flatten() {
            if *m == BlendMode::Dissolve {
                continue;
            }
            let o = blend_px(*m, L, [0.0; 4], [0.3, 0.2, 0.1, 1.0], 0.0);
            assert!(
                (o[0] - 0.3).abs() < 1e-5 && (o[3] - 1.0).abs() < 1e-6,
                "{m:?}"
            );
        }
    }

    #[test]
    fn ora_names_roundtrip() {
        for m in BlendMode::MENU.iter().flatten() {
            assert_eq!(BlendMode::from_ora_op(m.ora_op()), Some(*m));
        }
    }
}
