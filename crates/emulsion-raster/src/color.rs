//! sRGB ↔ linear conversion and pixel packing.

use std::sync::LazyLock;

#[inline]
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[inline]
pub fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// 8-bit sRGB → linear f32.
pub static SRGB8_TO_LINEAR: LazyLock<[f32; 256]> = LazyLock::new(|| {
    let mut t = [0.0; 256];
    for (i, v) in t.iter_mut().enumerate() {
        *v = srgb_to_linear(i as f32 / 255.0);
    }
    t
});

const ENC_BITS: usize = 12;
const ENC_N: usize = 1 << ENC_BITS;

/// Linear [0,1] quantised to 12 bits → 8-bit sRGB. Accurate to ±1 code at
/// the darkest end, exact elsewhere.
static LINEAR_TO_SRGB8: LazyLock<Vec<u8>> = LazyLock::new(|| {
    (0..ENC_N)
        .map(|i| {
            let l = i as f32 / (ENC_N - 1) as f32;
            (linear_to_srgb(l) * 255.0 + 0.5).clamp(0.0, 255.0) as u8
        })
        .collect()
});

/// Linear [0,1] f32 → 8-bit sRGB via table. Dark values use the exact curve
/// so shadows do not band.
#[inline]
pub fn linear_to_srgb8(l: f32) -> u8 {
    let l = l.clamp(0.0, 1.0);
    if l < 0.01 {
        return (linear_to_srgb(l) * 255.0 + 0.5) as u8;
    }
    LINEAR_TO_SRGB8[(l * (ENC_N - 1) as f32 + 0.5) as usize]
}

/// Linear (16-bit steps) → 16-bit sRGB, built once. The toe below the
/// linear segment's knee is computed directly, where the table would
/// quantise too coarsely.
static LINEAR_TO_SRGB16: LazyLock<Vec<u16>> = LazyLock::new(|| {
    (0..=u16::MAX)
        .map(|i| f_to_u16(linear_to_srgb(i as f32 / 65535.0)))
        .collect()
});

#[inline]
pub fn linear_to_srgb16(l: f32) -> u16 {
    let l = l.clamp(0.0, 1.0);
    if l <= 0.003_130_8 {
        return f_to_u16(l * 12.92);
    }
    LINEAR_TO_SRGB16[(l * 65535.0 + 0.5) as usize]
}

#[inline]
pub fn u16_to_f(v: u16) -> f32 {
    v as f32 * (1.0 / 65535.0)
}

#[inline]
pub fn f_to_u16(v: f32) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16
}

#[inline]
pub fn px_to_f(p: [u16; 4]) -> [f32; 4] {
    [
        u16_to_f(p[0]),
        u16_to_f(p[1]),
        u16_to_f(p[2]),
        u16_to_f(p[3]),
    ]
}

#[inline]
pub fn f_to_px(p: [f32; 4]) -> [u16; 4] {
    [
        f_to_u16(p[0]),
        f_to_u16(p[1]),
        f_to_u16(p[2]),
        f_to_u16(p[3]),
    ]
}

/// Straight 8-bit sRGBA → premultiplied linear f32.
#[inline]
pub fn srgba8_to_premul(p: [u8; 4]) -> [f32; 4] {
    let a = p[3] as f32 / 255.0;
    let t = &*SRGB8_TO_LINEAR;
    [
        t[p[0] as usize] * a,
        t[p[1] as usize] * a,
        t[p[2] as usize] * a,
        a,
    ]
}

/// Premultiplied linear f32 → straight 8-bit sRGBA.
#[inline]
pub fn premul_to_srgba8(p: [f32; 4]) -> [u8; 4] {
    let a = p[3].clamp(0.0, 1.0);
    if a <= 0.0 {
        return [0, 0, 0, 0];
    }
    let inv = 1.0 / a;
    [
        linear_to_srgb8(p[0] * inv),
        linear_to_srgb8(p[1] * inv),
        linear_to_srgb8(p[2] * inv),
        (a * 255.0 + 0.5) as u8,
    ]
}

/// Straight 16-bit sRGBA → premultiplied linear f32.
#[inline]
pub fn srgba16_to_premul(p: [u16; 4]) -> [f32; 4] {
    let a = u16_to_f(p[3]);
    [
        srgb_to_linear(u16_to_f(p[0])) * a,
        srgb_to_linear(u16_to_f(p[1])) * a,
        srgb_to_linear(u16_to_f(p[2])) * a,
        a,
    ]
}

/// Premultiplied linear f32 → straight 16-bit sRGBA.
#[inline]
pub fn premul_to_srgba16(p: [f32; 4]) -> [u16; 4] {
    let a = p[3].clamp(0.0, 1.0);
    if a <= 0.0 {
        return [0, 0, 0, 0];
    }
    let inv = 1.0 / a;
    [
        linear_to_srgb16(p[0] * inv),
        linear_to_srgb16(p[1] * inv),
        linear_to_srgb16(p[2] * inv),
        f_to_u16(a),
    ]
}

/// Rec. 709 luminance of linear RGB.
#[inline]
pub fn luma(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb8_roundtrip_is_exact() {
        for v in 0..=255u8 {
            let l = SRGB8_TO_LINEAR[v as usize];
            assert_eq!(linear_to_srgb8(l), v, "code {v}");
        }
    }

    #[test]
    fn premul_roundtrip() {
        for p in [
            [255, 0, 0, 255],
            [10, 200, 30, 128],
            [0, 0, 0, 0],
            [255, 255, 255, 1],
        ] {
            let back = premul_to_srgba8(srgba8_to_premul(p));
            if p[3] == 0 {
                assert_eq!(back, [0, 0, 0, 0]);
                continue;
            }
            for c in 0..4 {
                let tol = if p[3] < 8 { 40 } else { 1 };
                assert!(
                    (back[c] as i32 - p[c] as i32).abs() <= tol,
                    "{p:?} -> {back:?}"
                );
            }
        }
    }
}
