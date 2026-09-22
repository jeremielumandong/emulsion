//! Sampling and coverage helpers shared by layer effects.
use crate::style_options::*;
use emulsion_raster::{IRect, color};
pub(crate) fn contour(v: f32, o: &StyleOptions) -> f32 {
    let v = v.clamp(0., 1.);
    let lower = o
        .contour
        .iter()
        .filter(|p| p.x <= v)
        .max_by(|a, b| a.x.total_cmp(&b.x));
    let upper = o
        .contour
        .iter()
        .filter(|p| p.x >= v)
        .min_by(|a, b| a.x.total_cmp(&b.x));
    let out = match (lower, upper) {
        (Some(a), Some(b)) => a.y + (b.y - a.y) * (v - a.x) / (b.x - a.x).max(0.00001),
        (Some(a), None) => a.y,
        (None, Some(b)) => b.y,
        _ => v,
    };
    if o.invert_contour { 1. - out } else { out }
}
fn rgba(c: [u8; 4]) -> [f32; 4] {
    static LUT: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    let lut = LUT.get_or_init(|| std::array::from_fn(|i| color::srgb_to_linear(i as f32 / 255.)));
    [
        lut[c[0] as usize],
        lut[c[1] as usize],
        lut[c[2] as usize],
        c[3] as f32 / 255.,
    ]
}
pub(crate) fn grain(x: usize, y: usize) -> f32 {
    let mut v = (x as u32).wrapping_mul(374761393) ^ (y as u32).wrapping_mul(668265263);
    v = (v ^ (v >> 13)).wrapping_mul(1274126177);
    (v ^ (v >> 16)) as f32 / u32::MAX as f32
}
pub(crate) fn gradient(t: f32, g: &GradientSettings, from: [u8; 3], to: [u8; 3]) -> [f32; 4] {
    let t = if g.reverse { 1. - t } else { t }.clamp(0., 1.);
    let lower = g
        .stops
        .iter()
        .filter(|s| s.position <= t)
        .max_by(|a, b| a.position.total_cmp(&b.position));
    let upper = g
        .stops
        .iter()
        .filter(|s| s.position >= t)
        .min_by(|a, b| a.position.total_cmp(&b.position));
    match (lower, upper) {
        (Some(a), Some(b)) => {
            let u = (t - a.position) / (b.position - a.position).max(0.00001);
            let a = rgba(a.color);
            let b = rgba(b.color);
            std::array::from_fn(|i| a[i] + (b[i] - a[i]) * u)
        }
        (Some(a), None) => rgba(a.color),
        (None, Some(b)) => rgba(b.color),
        _ => {
            let a = rgba([from[0], from[1], from[2], 255]);
            let b = rgba([to[0], to[1], to[2], 255]);
            std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
        }
    }
}
pub(crate) fn gradient_position(x: f32, y: f32, b: IRect, angle: f32, g: &GradientSettings) -> f32 {
    let (s, c) = (angle + g.angle).to_radians().sin_cos();
    let px = x - (b.x as f32 + b.w as f32 / 2.) - g.offset_x;
    let py = y - (b.y as f32 + b.h as f32 / 2.) - g.offset_y;
    let extent =
        ((b.w as f32 * c.abs() + b.h as f32 * s.abs()) / 2.).max(1.) * g.scale.max(0.01) / 100.;
    let u = (px * c - py * s) / extent;
    let v = (px * s + py * c) / extent;
    match g.kind {
        GradientKind::Linear => u * 0.5 + 0.5,
        GradientKind::Radial => (u * u + v * v).sqrt(),
        GradientKind::Angle => v.atan2(u) / std::f32::consts::TAU + 0.5,
        GradientKind::Reflected => u.abs(),
        GradientKind::Diamond => u.abs() + v.abs(),
    }
    .clamp(0., 1.)
}
pub(crate) fn pattern(
    x: f32,
    y: f32,
    p: &PatternSettings,
    from: [u8; 3],
    to: [u8; 3],
    legacy_scale: f32,
    angle: f32,
) -> [f32; 4] {
    let (s, c) = (angle + p.angle).to_radians().sin_cos();
    let x = x - p.offset_x;
    let y = y - p.offset_y;
    let u = (x * c - y * s) * 100. / p.scale.max(0.01);
    let v = (x * s + y * c) * 100. / p.scale.max(0.01);
    let rgba = if let Some(img) = &p.image {
        let ix = (u.floor() as i64).rem_euclid(img.width as i64) as usize;
        let iy = (v.floor() as i64).rem_euclid(img.height as i64) as usize;
        let i = (iy * img.width as usize + ix) * 4;
        [
            img.pixels[i],
            img.pixels[i + 1],
            img.pixels[i + 2],
            img.pixels[i + 3],
        ]
    } else {
        let cell = ((u / legacy_scale.max(1.)).floor() as i64
            + (v / legacy_scale.max(1.)).floor() as i64)
            .rem_euclid(2);
        let c = if cell == 0 { from } else { to };
        [c[0], c[1], c[2], 255]
    };
    self::rgba(rgba)
}
