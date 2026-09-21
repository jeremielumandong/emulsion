//! Layer styles: effects drawn from a node's alpha — drop and inner
//! shadows, outer glow, stroke, colour and gradient overlays.
//!
//! Styles are data on the node. At composite time they become extra pixel
//! layers beside the node: shadow, glow and stroke below it, inner shadow
//! and overlays above it. Rendering them costs a blur over the node's
//! area, so results are memoized by the node's content, placement, mask
//! and styles; painting on the node invalidates its entry.

use crate::document::Document;
use crate::node::{Node, NodeKind};
use emulsion_raster::composite::{CompositeNode, NodeContent, flatten};
use emulsion_raster::{BlendMode, IRect, Placement, Raster, color};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum LayerStyle {
    DropShadow {
        color: [u8; 3],
        opacity: f32,
        angle: f32,
        distance: f32,
        size: f32,
    },
    InnerShadow {
        color: [u8; 3],
        opacity: f32,
        angle: f32,
        distance: f32,
        size: f32,
    },
    OuterGlow {
        color: [u8; 3],
        opacity: f32,
        size: f32,
    },
    Stroke {
        color: [u8; 3],
        opacity: f32,
        size: f32,
    },
    ColorOverlay {
        color: [u8; 3],
        opacity: f32,
    },
    GradientOverlay {
        from: [u8; 3],
        to: [u8; 3],
        angle: f32,
        opacity: f32,
    },
    BevelEmboss {
        highlight: [u8; 3],
        shadow: [u8; 3],
        opacity: f32,
        angle: f32,
        size: f32,
        depth: f32,
        contour: f32,
        texture: f32,
        texture_scale: f32,
    },
    InnerGlow {
        color: [u8; 3],
        opacity: f32,
        size: f32,
    },
    Satin {
        color: [u8; 3],
        opacity: f32,
        angle: f32,
        distance: f32,
        size: f32,
    },
    PatternOverlay {
        from: [u8; 3],
        to: [u8; 3],
        opacity: f32,
        scale: f32,
        angle: f32,
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
            u => format!("{:.0}{u}", self.value),
        }
    }
}

pub const MAX_STYLES: usize = 12;

impl LayerStyle {
    pub fn catalogue() -> Vec<LayerStyle> {
        vec![
            LayerStyle::DropShadow {
                color: [0, 0, 0],
                opacity: 60.0,
                angle: 120.0,
                distance: 8.0,
                size: 10.0,
            },
            LayerStyle::InnerShadow {
                color: [0, 0, 0],
                opacity: 50.0,
                angle: 120.0,
                distance: 6.0,
                size: 8.0,
            },
            LayerStyle::OuterGlow {
                color: [255, 230, 120],
                opacity: 70.0,
                size: 16.0,
            },
            LayerStyle::Stroke {
                color: [10, 10, 11],
                opacity: 100.0,
                size: 3.0,
            },
            LayerStyle::ColorOverlay {
                color: [217, 58, 30],
                opacity: 100.0,
            },
            LayerStyle::GradientOverlay {
                from: [10, 10, 11],
                to: [255, 255, 255],
                angle: 90.0,
                opacity: 100.0,
            },
            LayerStyle::BevelEmboss {
                highlight: [255; 3],
                shadow: [0; 3],
                opacity: 75.0,
                angle: 120.0,
                size: 6.0,
                depth: 150.0,
                contour: 100.0,
                texture: 0.0,
                texture_scale: 12.0,
            },
            LayerStyle::InnerGlow {
                color: [255, 240, 170],
                opacity: 70.0,
                size: 12.0,
            },
            LayerStyle::Satin {
                color: [30, 10, 50],
                opacity: 60.0,
                angle: 25.0,
                distance: 12.0,
                size: 10.0,
            },
            LayerStyle::PatternOverlay {
                from: [235; 3],
                to: [70; 3],
                opacity: 50.0,
                scale: 12.0,
                angle: 0.0,
            },
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            LayerStyle::DropShadow { .. } => "Drop shadow",
            LayerStyle::InnerShadow { .. } => "Inner shadow",
            LayerStyle::OuterGlow { .. } => "Outer glow",
            LayerStyle::Stroke { .. } => "Stroke",
            LayerStyle::ColorOverlay { .. } => "Color overlay",
            LayerStyle::GradientOverlay { .. } => "Gradient overlay",
            LayerStyle::BevelEmboss { .. } => "Bevel and emboss",
            LayerStyle::InnerGlow { .. } => "Inner glow",
            LayerStyle::Satin { .. } => "Satin",
            LayerStyle::PatternOverlay { .. } => "Pattern overlay",
        }
    }

    pub fn key(&self) -> &'static str {
        match self {
            LayerStyle::DropShadow { .. } => "drop_shadow",
            LayerStyle::InnerShadow { .. } => "inner_shadow",
            LayerStyle::OuterGlow { .. } => "outer_glow",
            LayerStyle::Stroke { .. } => "stroke",
            LayerStyle::ColorOverlay { .. } => "color_overlay",
            LayerStyle::GradientOverlay { .. } => "gradient_overlay",
            LayerStyle::BevelEmboss { .. } => "bevel_emboss",
            LayerStyle::InnerGlow { .. } => "inner_glow",
            LayerStyle::Satin { .. } => "satin",
            LayerStyle::PatternOverlay { .. } => "pattern_overlay",
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
            LayerStyle::Satin {
                opacity,
                angle,
                distance,
                size,
                ..
            }
            | LayerStyle::DropShadow {
                opacity,
                angle,
                distance,
                size,
                ..
            }
            | LayerStyle::InnerShadow {
                opacity,
                angle,
                distance,
                size,
                ..
            } => vec![
                p("opacity", "opacity", 0.0, 100.0, 1.0, *opacity, "%"),
                p("angle", "angle", -180.0, 180.0, 1.0, *angle, "°"),
                p("distance", "distance", 0.0, 100.0, 1.0, *distance, "px"),
                p("size", "size", 0.0, 60.0, 1.0, *size, "px"),
            ],
            LayerStyle::OuterGlow { opacity, size, .. }
            | LayerStyle::InnerGlow { opacity, size, .. } => vec![
                p("opacity", "opacity", 0.0, 100.0, 1.0, *opacity, "%"),
                p("size", "size", 0.0, 80.0, 1.0, *size, "px"),
            ],
            LayerStyle::Stroke { opacity, size, .. } => vec![
                p("opacity", "opacity", 0.0, 100.0, 1.0, *opacity, "%"),
                p("size", "size", 1.0, 40.0, 1.0, *size, "px"),
            ],
            LayerStyle::ColorOverlay { opacity, .. } => {
                vec![p("opacity", "opacity", 0.0, 100.0, 1.0, *opacity, "%")]
            }
            LayerStyle::BevelEmboss {
                opacity,
                angle,
                size,
                depth,
                contour,
                texture,
                texture_scale,
                ..
            } => vec![
                p("opacity", "opacity", 0.0, 100.0, 1.0, *opacity, "%"),
                p("angle", "light angle", -180.0, 180.0, 1.0, *angle, "\u{b0}"),
                p("size", "size", 1.0, 60.0, 1.0, *size, "px"),
                p("depth", "depth", 0.0, 500.0, 5.0, *depth, "%"),
                p("contour", "contour", 25.0, 400.0, 5.0, *contour, "%"),
                p("texture", "texture depth", 0.0, 100.0, 1.0, *texture, "%"),
                p(
                    "texture_scale",
                    "texture size",
                    2.0,
                    100.0,
                    1.0,
                    *texture_scale,
                    "px",
                ),
            ],
            LayerStyle::PatternOverlay {
                opacity,
                scale,
                angle,
                ..
            } => vec![
                p("opacity", "opacity", 0.0, 100.0, 1.0, *opacity, "%"),
                p("scale", "tile size", 2.0, 200.0, 1.0, *scale, "px"),
                p("angle", "angle", -180.0, 180.0, 1.0, *angle, "\u{b0}"),
            ],
            LayerStyle::GradientOverlay { angle, opacity, .. } => vec![
                p("angle", "angle", -180.0, 180.0, 1.0, *angle, "°"),
                p("opacity", "opacity", 0.0, 100.0, 1.0, *opacity, "%"),
            ],
        }
    }

    pub fn set_param(&mut self, key: &str, value: f32) -> bool {
        if !value.is_finite() {
            return false;
        }
        let Some(spec) = self.params().into_iter().find(|s| s.key == key) else {
            return false;
        };
        let v = value.clamp(spec.min, spec.max);
        let slot: &mut f32 = match (self, key) {
            (
                LayerStyle::DropShadow { opacity, .. }
                | LayerStyle::InnerShadow { opacity, .. }
                | LayerStyle::OuterGlow { opacity, .. }
                | LayerStyle::Stroke { opacity, .. }
                | LayerStyle::ColorOverlay { opacity, .. }
                | LayerStyle::GradientOverlay { opacity, .. }
                | LayerStyle::InnerGlow { opacity, .. }
                | LayerStyle::Satin { opacity, .. }
                | LayerStyle::BevelEmboss { opacity, .. }
                | LayerStyle::PatternOverlay { opacity, .. },
                "opacity",
            ) => opacity,
            (
                LayerStyle::DropShadow { angle, .. }
                | LayerStyle::InnerShadow { angle, .. }
                | LayerStyle::GradientOverlay { angle, .. }
                | LayerStyle::Satin { angle, .. }
                | LayerStyle::BevelEmboss { angle, .. }
                | LayerStyle::PatternOverlay { angle, .. },
                "angle",
            ) => angle,
            (
                LayerStyle::DropShadow { distance, .. }
                | LayerStyle::InnerShadow { distance, .. }
                | LayerStyle::Satin { distance, .. },
                "distance",
            ) => distance,
            (
                LayerStyle::DropShadow { size, .. }
                | LayerStyle::InnerShadow { size, .. }
                | LayerStyle::OuterGlow { size, .. }
                | LayerStyle::Stroke { size, .. }
                | LayerStyle::Satin { size, .. }
                | LayerStyle::InnerGlow { size, .. }
                | LayerStyle::BevelEmboss { size, .. },
                "size",
            ) => size,
            (LayerStyle::BevelEmboss { depth, .. }, "depth") => depth,
            (LayerStyle::BevelEmboss { contour, .. }, "contour") => contour,
            (LayerStyle::BevelEmboss { texture, .. }, "texture") => texture,
            (LayerStyle::BevelEmboss { texture_scale, .. }, "texture_scale") => texture_scale,
            (LayerStyle::PatternOverlay { scale, .. }, "scale") => scale,
            _ => return false,
        };
        *slot = v;
        true
    }

    /// Set the (first) colour; gradient overlays take `to` when `second`.
    pub fn set_color(&mut self, c: [u8; 3], second: bool) {
        match self {
            LayerStyle::DropShadow { color, .. }
            | LayerStyle::InnerShadow { color, .. }
            | LayerStyle::OuterGlow { color, .. }
            | LayerStyle::Stroke { color, .. }
            | LayerStyle::InnerGlow { color, .. }
            | LayerStyle::Satin { color, .. }
            | LayerStyle::ColorOverlay { color, .. } => *color = c,
            LayerStyle::GradientOverlay { from, to, .. }
            | LayerStyle::PatternOverlay { from, to, .. }
            | LayerStyle::BevelEmboss {
                highlight: from,
                shadow: to,
                ..
            } => {
                if second {
                    *to = c
                } else {
                    *from = c
                }
            }
        }
    }

    pub fn colors(&self) -> Vec<[u8; 3]> {
        match self {
            LayerStyle::DropShadow { color, .. }
            | LayerStyle::InnerShadow { color, .. }
            | LayerStyle::OuterGlow { color, .. }
            | LayerStyle::Stroke { color, .. }
            | LayerStyle::InnerGlow { color, .. }
            | LayerStyle::Satin { color, .. }
            | LayerStyle::ColorOverlay { color, .. } => vec![*color],
            LayerStyle::GradientOverlay { from, to, .. }
            | LayerStyle::PatternOverlay { from, to, .. }
            | LayerStyle::BevelEmboss {
                highlight: from,
                shadow: to,
                ..
            } => vec![*from, *to],
        }
    }

    fn spread(&self) -> i32 {
        match self {
            LayerStyle::DropShadow { distance, size, .. } => (distance + size * 3.0).ceil() as i32,
            LayerStyle::OuterGlow { size, .. } => (size * 3.0).ceil() as i32,
            LayerStyle::Stroke { size, .. } => size.ceil() as i32 + 1,
            _ => 0,
        }
    }
}

/// Rendered effects for one node.
pub struct Rendered {
    pub below: Option<(Arc<Raster>, IRect)>,
    pub above: Option<(Arc<Raster>, IRect)>,
}

type Key = (usize, usize, u64, String);
type Memo = Mutex<Vec<(Key, Arc<Rendered>)>>;

fn memo() -> &'static Memo {
    static M: std::sync::OnceLock<Memo> = std::sync::OnceLock::new();
    M.get_or_init(|| Mutex::new(Vec::new()))
}

fn key_for(n: &Node) -> Option<Key> {
    if n.styles.is_empty() {
        return None;
    }
    let (content, placement) = match &n.kind {
        NodeKind::Raster { raster, placement } => (Arc::as_ptr(raster) as usize, *placement),
        NodeKind::Smart {
            cache, placement, ..
        } => (Arc::as_ptr(cache) as usize, *placement),
        NodeKind::Path { cache, .. } | NodeKind::Text { cache, .. } => {
            (Arc::as_ptr(cache) as usize, Placement::default())
        }
        _ => return None,
    };
    let mask = n.mask.as_ref().map_or(0, |m| Arc::as_ptr(m) as usize);
    let mut ph = 0u64;
    for v in [
        placement.x,
        placement.y,
        placement.scale_x,
        placement.scale_y,
        placement.rotation,
    ] {
        ph = ph.rotate_left(13) ^ v.to_bits();
    }
    ph ^= (placement.flip_x as u64) << 1 | placement.flip_y as u64 | (n.mask_enabled as u64) << 2;
    for value in n.mask_transform {
        ph = ph.rotate_left(13) ^ value.to_bits();
    }
    Some((
        content,
        mask,
        ph,
        serde_json::to_string(&n.styles).unwrap_or_default(),
    ))
}

/// The node's document-space alpha over its bounds, from a solo render.
fn alpha_of(doc: &Document, n: &Node, pad: i32) -> Option<(Vec<f32>, IRect)> {
    let mut solo = Document::new(doc.width, doc.height);
    let mut node = n.clone();
    node.parent = None;
    node.clip_to = None;
    node.visible = true;
    node.opacity = 1.0;
    node.blend = BlendMode::Normal;
    node.styles.clear();
    node.blending = Default::default();
    solo.nodes.push(node);
    let full = flatten(&solo.composite_tree(), 0);
    let b = full.tile_bounds();
    if b.is_empty() {
        return None;
    }
    let r = IRect::new(b.x - pad, b.y - pad, b.w + 2 * pad, b.h + 2 * pad);
    let a: Vec<f32> = full
        .read_rect(r)
        .into_iter()
        .map(|p| color::u16_to_f(p[3]))
        .collect();
    Some((a, r))
}

fn blur(a: &[f32], w: usize, h: usize, radius: f32) -> Vec<f32> {
    if radius < 0.3 {
        return a.to_vec();
    }
    let sigma = (radius / 2.0).max(0.3);
    let r = (sigma * 3.0).ceil() as i64;
    let k: Vec<f32> = (-r..=r)
        .map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let sum: f32 = k.iter().sum();
    let k: Vec<f32> = k.iter().map(|v| v / sum).collect();
    let mut tmp = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut acc = 0.0;
            for (i, kv) in k.iter().enumerate() {
                let sx = x as i64 + i as i64 - r;
                if sx >= 0 && (sx as usize) < w {
                    acc += a[y * w + sx as usize] * kv;
                }
            }
            tmp[y * w + x] = acc;
        }
    }
    let mut out = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut acc = 0.0;
            for (i, kv) in k.iter().enumerate() {
                let sy = y as i64 + i as i64 - r;
                if sy >= 0 && (sy as usize) < h {
                    acc += tmp[sy as usize * w + x] * kv;
                }
            }
            out[y * w + x] = acc;
        }
    }
    out
}

fn shift(a: &[f32], w: usize, h: usize, dx: i32, dy: i32) -> Vec<f32> {
    let mut out = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let (sx, sy) = (x as i32 - dx, y as i32 - dy);
            if sx >= 0 && sy >= 0 && (sx as usize) < w && (sy as usize) < h {
                out[y * w + x] = a[sy as usize * w + sx as usize];
            }
        }
    }
    out
}

fn dilate(a: &[f32], w: usize, h: usize, radius: f32) -> Vec<f32> {
    let r = radius.ceil() as i32;
    let mut out = vec![0.0f32; w * h];
    let offsets: Vec<(i32, i32)> = (-r..=r)
        .flat_map(|dy| (-r..=r).map(move |dx| (dx, dy)))
        .filter(|(dx, dy)| ((dx * dx + dy * dy) as f32).sqrt() <= radius + 0.5)
        .collect();
    for y in 0..h {
        for x in 0..w {
            let mut m = 0.0f32;
            for (dx, dy) in &offsets {
                let (sx, sy) = (x as i32 + dx, y as i32 + dy);
                if sx >= 0 && sy >= 0 && (sx as usize) < w && (sy as usize) < h {
                    m = m.max(a[sy as usize * w + sx as usize]);
                }
            }
            out[y * w + x] = m;
        }
    }
    out
}

fn offset(angle: f32, distance: f32) -> (i32, i32) {
    // Photoshop measures the light's angle; the shadow falls opposite.
    let a = angle.to_radians();
    (
        (-a.cos() * distance).round() as i32,
        (a.sin() * distance).round() as i32,
    )
}

/// Render `n`'s styles. Memoized.
pub fn render(doc: &Document, n: &Node) -> Option<Arc<Rendered>> {
    let key = key_for(n)?;
    {
        let m = memo().lock();
        if let Some((_, r)) = m.iter().find(|(k, _)| *k == key) {
            return Some(r.clone());
        }
    }
    let pad = n
        .styles
        .iter()
        .map(LayerStyle::spread)
        .max()
        .unwrap_or(0)
        .clamp(0, 200);
    let (alpha, r) = alpha_of(doc, n, pad)?;
    let (w, h) = (r.w as usize, r.h as usize);
    let mut below = vec![[0.0f32; 4]; w * h];
    let mut above = vec![[0.0f32; 4]; w * h];
    let (mut any_below, mut any_above) = (false, false);
    let over = |dst: &mut [[f32; 4]], cov: &[f32], col: [f32; 3], k: f32| {
        for (d, c) in dst.iter_mut().zip(cov) {
            let a = (c * k).clamp(0.0, 1.0);
            if a <= 0.0 {
                continue;
            }
            for i in 0..3 {
                d[i] = col[i] * a + d[i] * (1.0 - a);
            }
            d[3] = a + d[3] * (1.0 - a);
        }
    };
    let lin = |c: [u8; 3]| [0, 1, 2].map(|i| color::srgb_to_linear(c[i] as f32 / 255.0));
    for s in &n.styles {
        match s {
            LayerStyle::DropShadow {
                color,
                opacity,
                angle,
                distance,
                size,
            } => {
                let (dx, dy) = offset(*angle, *distance);
                let cov = blur(&shift(&alpha, w, h, dx, dy), w, h, *size);
                over(&mut below, &cov, lin(*color), opacity / 100.0);
                any_below = true;
            }
            LayerStyle::OuterGlow {
                color,
                opacity,
                size,
            } => {
                let cov = blur(&dilate(&alpha, w, h, size / 3.0), w, h, *size);
                over(&mut below, &cov, lin(*color), opacity / 100.0);
                any_below = true;
            }
            LayerStyle::Stroke {
                color,
                opacity,
                size,
            } => {
                let cov = dilate(&alpha, w, h, *size);
                over(&mut below, &cov, lin(*color), opacity / 100.0);
                any_below = true;
            }
            LayerStyle::InnerShadow {
                color,
                opacity,
                angle,
                distance,
                size,
            } => {
                let (dx, dy) = offset(*angle, *distance);
                let inv: Vec<f32> = shift(&alpha, w, h, dx, dy)
                    .iter()
                    .map(|v| 1.0 - v)
                    .collect();
                let cov: Vec<f32> = blur(&inv, w, h, *size)
                    .iter()
                    .zip(&alpha)
                    .map(|(c, a)| c * a)
                    .collect();
                over(&mut above, &cov, lin(*color), opacity / 100.0);
                any_above = true;
            }
            LayerStyle::ColorOverlay { color, opacity } => {
                over(&mut above, &alpha, lin(*color), opacity / 100.0);
                any_above = true;
            }
            LayerStyle::InnerGlow {
                color,
                opacity,
                size,
            } => {
                let soft = blur(&alpha, w, h, *size);
                let cov: Vec<f32> = alpha
                    .iter()
                    .zip(soft)
                    .map(|(a, b)| a * (1.0 - b) * 2.0)
                    .collect();
                over(&mut above, &cov, lin(*color), opacity / 100.0);
                any_above = true;
            }
            LayerStyle::Satin {
                color,
                opacity,
                angle,
                distance,
                size,
            } => {
                let soft = blur(&alpha, w, h, *size);
                let (dx, dy) = offset(*angle, *distance);
                let left = shift(&soft, w, h, dx, dy);
                let right = shift(&soft, w, h, -dx, -dy);
                let cov: Vec<f32> = left
                    .iter()
                    .zip(right)
                    .zip(&alpha)
                    .map(|((a, b), mask)| (a - b).abs() * mask)
                    .collect();
                over(&mut above, &cov, lin(*color), opacity / 100.0);
                any_above = true;
            }
            LayerStyle::BevelEmboss {
                highlight,
                shadow,
                opacity,
                angle,
                size,
                depth,
                contour,
                texture,
                texture_scale,
            } => {
                // A softened alpha height field lights the object's edges; a
                // periodic relief adds an optional woven surface texture.
                let height = blur(&alpha, w, h, *size);
                let (ly, lx) = angle.to_radians().sin_cos();
                let mut light = vec![0.0; w * h];
                let mut dark = vec![0.0; w * h];
                let period = texture_scale.max(2.0);
                let frequency = std::f32::consts::TAU / period;
                for y in 0..h {
                    for x in 0..w {
                        let i = y * w + x;
                        let gx = height[y * w + (x + 1).min(w - 1)]
                            - height[y * w + x.saturating_sub(1)];
                        let gy = height[(y + 1).min(h - 1) * w + x]
                            - height[y.saturating_sub(1) * w + x];
                        let tx = (x as f32 + r.x as f32) * frequency;
                        let ty = (y as f32 + r.y as f32) * frequency;
                        let relief =
                            (tx.cos() * ty.sin() * lx - tx.sin() * ty.cos() * ly) * texture / 100.0;
                        let shade = ((gx * lx - gy * ly) * size.max(1.0) + relief) * depth / 100.0;
                        let strength = shade
                            .abs()
                            .clamp(0.0, 1.0)
                            .powf((contour / 100.0).max(0.25))
                            * alpha[i];
                        if shade >= 0.0 {
                            light[i] = strength;
                        } else {
                            dark[i] = strength;
                        }
                    }
                }
                over(&mut above, &dark, lin(*shadow), opacity / 100.0);
                over(&mut above, &light, lin(*highlight), opacity / 100.0);
                any_above = true;
            }
            LayerStyle::PatternOverlay {
                from,
                to,
                opacity,
                scale,
                angle,
            } => {
                // Two-colour checker tiles, anchored in document coordinates.
                let (sn, cs) = angle.to_radians().sin_cos();
                let period = scale.max(2.0);
                let colors = [lin(*from), lin(*to)];
                for y in 0..h {
                    for x in 0..w {
                        let i = y * w + x;
                        let px = x as f32 + r.x as f32;
                        let py = y as f32 + r.y as f32;
                        let u = ((px * cs - py * sn) / period).floor() as i64;
                        let v = ((px * sn + py * cs) / period).floor() as i64;
                        let col = colors[(u + v).rem_euclid(2) as usize];
                        let a = (alpha[i] * opacity / 100.0).clamp(0.0, 1.0);
                        let d = &mut above[i];
                        for k in 0..3 {
                            d[k] = col[k] * a + d[k] * (1.0 - a);
                        }
                        d[3] = a + d[3] * (1.0 - a);
                    }
                }
                any_above = true;
            }
            LayerStyle::GradientOverlay {
                from,
                to,
                angle,
                opacity,
            } => {
                let (c0, c1) = (lin(*from), lin(*to));
                let (s, c) = angle.to_radians().sin_cos();
                let (cw, ch) = (w as f32 / 2.0, h as f32 / 2.0);
                let extent = (cw * c.abs() + ch * s.abs()).max(1.0);
                for y in 0..h {
                    for x in 0..w {
                        let i = y * w + x;
                        let a = alpha[i] * opacity / 100.0;
                        if a <= 0.0 {
                            continue;
                        }
                        let t = ((((x as f32 - cw) * c - (y as f32 - ch) * s) / extent) * 0.5
                            + 0.5)
                            .clamp(0.0, 1.0);
                        let col = [0, 1, 2].map(|k| c0[k] + (c1[k] - c0[k]) * t);
                        let d = &mut above[i];
                        for k in 0..3 {
                            d[k] = col[k] * a + d[k] * (1.0 - a);
                        }
                        d[3] = a + d[3] * (1.0 - a);
                    }
                }
                any_above = true;
            }
        }
    }
    let to_raster = |px: Vec<[f32; 4]>| {
        let out: Vec<[u16; 4]> = px.into_iter().map(color::f_to_px).collect();
        Arc::new(Raster::from_pixels(w as u32, h as u32, [0; 4], &out))
    };
    let rendered = Arc::new(Rendered {
        below: any_below.then(|| (to_raster(below), r)),
        above: any_above.then(|| (to_raster(above), r)),
    });
    let mut m = memo().lock();
    if m.len() >= 24 {
        m.remove(0);
    }
    m.push((key, rendered.clone()));
    Some(rendered)
}

/// A composite node drawing a rendered effect raster at `r`.
pub fn effect_node(id: u64, raster: Arc<Raster>, r: IRect, n: &Node) -> CompositeNode {
    CompositeNode {
        id,
        visible: n.visible,
        opacity: n.opacity,
        blend: BlendMode::Normal,
        blending: emulsion_raster::composite::BlendingOptions {
            fill_opacity: 1.0,
            ..n.blending
        },
        mask: None,
        clip_to: None,
        content: NodeContent::Pixels {
            raster,
            placement: Placement::at(r.x as f64, r.y as f64),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, Slot};

    #[test]
    fn shadow_falls_outside_and_overlay_stays_inside() {
        let mut d = Document::new(100, 100);
        let square = Raster::from_fn(100, 100, [0; 4], |x, y| {
            if (30..70).contains(&x) && (30..70).contains(&y) {
                [0, 0, 65535, 65535]
            } else {
                [0; 4]
            }
        });
        let id = Command::AddNode {
            node: Box::new(Node::raster(
                0,
                "sq",
                Arc::new(square),
                Placement::default(),
            )),
            slot: Slot::TOP,
        }
        .apply(&mut d)
        .unwrap()
        .unwrap();
        d.node_mut(id).unwrap().styles = vec![
            LayerStyle::DropShadow {
                color: [0, 0, 0],
                opacity: 100.0,
                angle: 180.0,
                distance: 12.0,
                size: 0.0,
            },
            LayerStyle::ColorOverlay {
                color: [255, 0, 0],
                opacity: 100.0,
            },
        ];
        let flat = flatten(&d.composite_tree(), 0);
        let shadow = color::px_to_f(flat.get(76, 50));
        assert!(
            shadow[3] > 0.9 && shadow[0] < 0.01,
            "shadow to the right of the square: {shadow:?}"
        );
        let inside = color::px_to_f(flat.get(50, 50));
        assert!(
            inside[0] > 0.99 && inside[2] < 0.01,
            "overlay is red over the blue: {inside:?}"
        );
        assert_eq!(flat.get(10, 10), [0; 4]);
        // Memoized: the second render hits the cache.
        let n = d.node(id).unwrap();
        let a = render(&d, n).unwrap();
        let b = render(&d, n).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
    }

    fn shape_document() -> Document {
        let mut doc = Document::new(64, 64);
        let raster = Raster::from_fn(64, 64, [0; 4], |x, y| {
            if (12..52).contains(&x) && (12..52).contains(&y) {
                [12000, 22000, 32000, 65535]
            } else {
                [0; 4]
            }
        });
        doc.nodes.push(Node::raster(
            1,
            "shape",
            Arc::new(raster),
            Placement::default(),
        ));
        doc
    }

    #[test]
    fn new_effects_render_inside_shape_and_roundtrip_with_undo() {
        use crate::history::Editor;
        for style in LayerStyle::catalogue().into_iter().skip(6) {
            let doc = shape_document();
            let original = flatten(&doc.composite_tree(), 0).read_rect(IRect::new(0, 0, 64, 64));
            let mut editor = Editor::new(doc, None);
            editor
                .execute(Command::SetStyles {
                    id: 1,
                    styles: vec![style.clone()],
                })
                .unwrap();
            let rendered = flatten(&editor.doc.composite_tree(), 0);
            assert_ne!(
                rendered.read_rect(IRect::new(0, 0, 64, 64)),
                original,
                "{} changes shape",
                style.label()
            );
            assert_eq!(
                rendered.get(4, 4),
                [0; 4],
                "{} stays inside alpha",
                style.label()
            );
            assert_eq!(rendered.get(32, 32)[3], 65535);
            let encoded = serde_json::to_string(&style).unwrap();
            assert_eq!(serde_json::from_str::<LayerStyle>(&encoded).unwrap(), style);
            assert!(editor.undo());
            assert!(editor.doc.nodes[0].styles.is_empty());
            assert_eq!(
                flatten(&editor.doc.composite_tree(), 0).read_rect(IRect::new(0, 0, 64, 64)),
                original
            );
            assert!(editor.redo());
            assert_eq!(editor.doc.nodes[0].styles, vec![style]);
        }
    }

    #[test]
    fn effect_parameters_reject_nan_and_invalidate_memo() {
        let mut doc = shape_document();
        for mut style in LayerStyle::catalogue().into_iter().skip(6) {
            assert!(!style.set_param("opacity", f32::NAN));
            assert!(!style.set_param("opacity", f32::INFINITY));
            doc.nodes[0].styles = vec![style.clone()];
            let first = render(&doc, &doc.nodes[0]).unwrap();
            assert!(style.set_param("opacity", 0.0));
            doc.nodes[0].styles = vec![style];
            let second = render(&doc, &doc.nodes[0]).unwrap();
            assert!(!Arc::ptr_eq(&first, &second));
            assert_eq!(second.above.as_ref().unwrap().0.get(32, 32), [0; 4]);
        }
    }

    #[test]
    fn translucent_overlay_is_premultiplied_once() {
        let mut doc = shape_document();
        doc.nodes[0].styles = vec![LayerStyle::ColorOverlay {
            color: [255, 0, 0],
            opacity: 50.0,
        }];
        let effects = render(&doc, &doc.nodes[0]).unwrap();
        let pixel = effects.above.as_ref().unwrap().0.get(32, 32);
        assert!(
            (pixel[0] as i32 - 32768).abs() <= 1,
            "red should equal alpha for translucent red: {pixel:?}"
        );
        assert_eq!(pixel[0], pixel[3]);
    }

    #[test]
    fn bevel_texture_changes_interior_and_pattern_alternates() {
        let mut doc = shape_document();
        let mut bevel = LayerStyle::catalogue()[6].clone();
        doc.nodes[0].styles = vec![bevel.clone()];
        let plain = flatten(&doc.composite_tree(), 0).read_rect(IRect::new(24, 24, 16, 16));
        bevel.set_param("texture", 80.0);
        doc.nodes[0].styles = vec![bevel];
        assert_ne!(
            flatten(&doc.composite_tree(), 0).read_rect(IRect::new(24, 24, 16, 16)),
            plain
        );
        let mut pattern = LayerStyle::catalogue()[9].clone();
        pattern.set_param("opacity", 100.0);
        doc.nodes[0].styles = vec![pattern];
        let rendered = flatten(&doc.composite_tree(), 0);
        assert_ne!(rendered.get(25, 25), rendered.get(37, 25));
    }

    #[test]
    fn zero_fill_hides_content_and_preserves_style_shape() {
        let mut doc = shape_document();
        doc.nodes[0].blending.fill_opacity = 0.0;
        assert_eq!(flatten(&doc.composite_tree(), 0).get(32, 32), [0; 4]);
        doc.nodes[0].styles = vec![LayerStyle::ColorOverlay {
            color: [255, 0, 0],
            opacity: 100.0,
        }];
        let styled = flatten(&doc.composite_tree(), 0);
        assert_eq!(styled.get(32, 32), [65535, 0, 0, 65535]);
        assert_eq!(styled.get(4, 4), [0; 4]);
    }
}
