//! Layer styles: effects drawn from a node's alpha — drop and inner
//! shadows, outer glow, stroke, colour and gradient overlays.
//!
//! Styles are data on the node. At composite time they become extra pixel
//! layers beside the node: shadow, glow and stroke below it, inner shadow
//! and overlays above it. Rendering them costs a blur over the node's
//! area, so results are memoized by the node's content, placement, mask
//! and styles; painting on the node invalidates its entry.

use crate::document::Document;
use crate::effect_render::*;
use crate::node::{Node, NodeKind};
use crate::style_options::*;
#[cfg(test)]
use emulsion_raster::composite::flatten;
use emulsion_raster::composite::{CompositeNode, NodeContent, render_tile};
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
            LayerStyle::DropShadow { distance, size, .. }
            | LayerStyle::InnerShadow { distance, size, .. }
            | LayerStyle::Satin { distance, size, .. } => (distance + size * 3.0).ceil() as i32,
            LayerStyle::OuterGlow { size, .. } | LayerStyle::InnerGlow { size, .. } => {
                (size * 3.0).ceil() as i32
            }
            LayerStyle::Stroke { size, .. } => size.ceil() as i32 + 1,
            _ => 0,
        }
    }
}

/// Rendered effects for one node.
pub struct Rendered {
    pub below: Vec<RenderedEffect>,
    pub above: Vec<RenderedEffect>,
}

pub struct RenderedEffect {
    pub raster: Arc<Raster>,
    pub rect: IRect,
    pub blend: BlendMode,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ColorSampling {
    SpatialGradient,
    CoverageGradient,
    Pattern,
}
type ColorMapping = ([u8; 3], [u8; 3], f32, f32, ColorSampling);

type Key = (usize, usize, u64, String);
#[path = "style_cache.rs"]
mod cache;
type Memo = Mutex<cache::Memo>;

fn memo() -> &'static Memo {
    static M: std::sync::OnceLock<Memo> = std::sync::OnceLock::new();
    M.get_or_init(|| Mutex::new(cache::Memo::default()))
}

#[cfg(test)]
pub(crate) fn evict_for_test(doc: &Document, node: &Node) {
    if let Some((local, _)) = translation_source(doc, node) {
        if let Some(key) = key_for(&local, &local.nodes[0]) {
            memo().lock().evict(&key);
        }
    } else if let Some(key) = key_for(doc, node) {
        memo().lock().evict(&key);
    }
}

fn has_enabled_effects(n: &Node) -> bool {
    n.effects_enabled
        && !n.styles.is_empty()
        && n.styles
            .iter()
            .enumerate()
            .any(|(index, _)| n.style_options.get(index).is_none_or(|o| o.enabled))
}

fn key_for(doc: &Document, n: &Node) -> Option<Key> {
    if !has_enabled_effects(n) {
        return None;
    }
    fn signature(n: &Node, root: bool) -> String {
        let (pointer, placement, extra) = match &n.kind {
            NodeKind::Raster { raster, placement } => {
                (Arc::as_ptr(raster) as usize, *placement, String::new())
            }
            NodeKind::Smart {
                cache,
                placement,
                offset,
                ..
            } => (
                Arc::as_ptr(cache) as usize,
                *placement,
                format!("{offset:?}"),
            ),
            NodeKind::Path { cache, .. } | NodeKind::Text { cache, .. } => (
                Arc::as_ptr(cache) as usize,
                Placement::default(),
                String::new(),
            ),
            NodeKind::Fill { rgba } => (0, Placement::default(), format!("{rgba:?}")),
            NodeKind::Adjust(adjustment) => (0, Placement::default(), format!("{adjustment:?}")),
            _ => (0, Placement::default(), String::new()),
        };
        let mut options = n.style_options.clone();
        let assets: Vec<_> = options
            .iter_mut()
            .map(|o| {
                o.id = 0; // Identity is for editing, not rendered appearance.
                o.pattern.image.take().map(|p| Arc::as_ptr(&p) as usize)
            })
            .collect();
        format!(
            "{pointer}:{placement:?}:{extra}:{:?}:{:?}:{:?}:{:?}:{}:{}:{:?}:{:?}:{:?}:{:?}:{:?}",
            n.styles,
            options,
            assets,
            n.mask_transform,
            n.mask_enabled,
            if root { 1.0 } else { n.opacity },
            if root { BlendMode::Normal } else { n.blend },
            if root { Default::default() } else { n.blending },
            n.mask.as_ref().map(|m| Arc::as_ptr(m) as usize),
            root || n.visible,
            if root { None } else { n.clip_to }
        )
    }
    let mut key = signature(n, true);
    if matches!(n.kind, NodeKind::Group { .. }) {
        let mut ids = vec![n.id];
        loop {
            let more: Vec<_> = doc
                .nodes
                .iter()
                .filter(|c| c.parent.is_some_and(|p| ids.contains(&p)) && !ids.contains(&c.id))
                .map(|c| c.id)
                .collect();
            if more.is_empty() {
                break;
            }
            ids.extend(more);
        }
        for child in doc
            .nodes
            .iter()
            .filter(|c| c.id != n.id && ids.contains(&c.id))
        {
            key.push_str(&format!(
                "{}:{:?}:{}:{}",
                child.id,
                child.parent,
                child.effects_enabled,
                signature(child, false)
            ));
        }
    }
    key.push_str(&format!(
        "{}:{}:{:?}:{:?}",
        doc.width, doc.height, doc.global_light, doc.blend_space
    ));
    Some((0, 0, 0, key))
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
    node.style_options.clear();
    node.blending = Default::default();
    solo.global_light = doc.global_light;
    solo.blend_space = doc.blend_space;
    solo.nodes.push(node);
    if matches!(n.kind, NodeKind::Group { .. }) {
        let mut ids = vec![n.id];
        loop {
            let more: Vec<_> = doc
                .nodes
                .iter()
                .filter(|child| {
                    child.parent.is_some_and(|p| ids.contains(&p)) && !ids.contains(&child.id)
                })
                .map(|c| c.id)
                .collect();
            if more.is_empty() {
                break;
            }
            ids.extend(more);
        }
        solo.nodes.extend(
            doc.nodes
                .iter()
                .filter(|child| child.id != n.id && ids.contains(&child.id))
                .cloned(),
        );
    }
    let canvas = IRect::new(0, 0, doc.width as i32, doc.height as i32);
    let candidate = if matches!(n.kind, NodeKind::Group { .. }) {
        canvas
    } else {
        crate::geometry::node_bounds(&solo, n.id)?.intersect(&canvas)
    };
    if candidate.is_empty() {
        return None;
    }
    let tile_size = emulsion_raster::TILE as i32;
    let x = candidate.x.div_euclid(tile_size) * tile_size;
    let y = candidate.y.div_euclid(tile_size) * tile_size;
    let candidate = IRect::new(
        x,
        y,
        ((candidate.right() + tile_size - 1) / tile_size) * tile_size - x,
        ((candidate.bottom() + tile_size - 1) / tile_size) * tile_size - y,
    )
    .intersect(&canvas);
    // Keep only alpha. Flattening an 8K layer and then reading its RGBA rectangle
    // used two extra full-size color buffers merely to discard RGB immediately.
    let mut coverage = vec![0.0f32; candidate.w as usize * candidate.h as usize];
    let tree = solo.composite_tree();
    use rayon::prelude::*;
    let b = coverage
        .par_chunks_mut(candidate.w as usize * tile_size as usize)
        .enumerate()
        .map(|(band, pixels)| {
            let top = candidate.y + band as i32 * tile_size;
            let height = pixels.len() / candidate.w as usize;
            let mut left = candidate.right();
            let mut right = candidate.x;
            let mut first = candidate.bottom();
            let mut bottom = candidate.y;
            for tx in candidate.x / tile_size..(candidate.right() + tile_size - 1) / tile_size {
                let tile = render_tile(
                    &tree,
                    0,
                    emulsion_raster::TileCoord::new(tx, top / tile_size),
                );
                let width = tile_size.min(candidate.right() - tx * tile_size) as usize;
                let offset = (tx * tile_size - candidate.x) as usize;
                for row in 0..height {
                    for col in 0..width {
                        let alpha = tile[row * tile_size as usize + col][3];
                        pixels[row * candidate.w as usize + offset + col] = alpha;
                        if alpha > 0.0 {
                            let px = tx * tile_size + col as i32;
                            let py = top + row as i32;
                            left = left.min(px);
                            right = right.max(px + 1);
                            first = first.min(py);
                            bottom = bottom.max(py + 1);
                        }
                    }
                }
            }
            if right > left && bottom > first {
                IRect::new(left, first, right - left, bottom - first)
            } else {
                IRect::default()
            }
        })
        .reduce(IRect::default, |a, b| a.union(&b));
    if b.is_empty() {
        return None;
    }
    let r = IRect::new(b.x - pad, b.y - pad, b.w + 2 * pad, b.h + 2 * pad);
    if r == candidate {
        return Some((coverage, r));
    }
    let mut a = vec![0.0f32; r.w as usize * r.h as usize];
    for y in b.y..b.bottom() {
        let src = ((y - candidate.y) * candidate.w + b.x - candidate.x) as usize;
        let dst = ((y - r.y) * r.w + b.x - r.x) as usize;
        a[dst..dst + b.w as usize].copy_from_slice(&coverage[src..src + b.w as usize]);
    }
    Some((a, r))
}

#[path = "effect_kernels.rs"]
mod effect_kernels;
use effect_kernels::{blur, dilate};

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

fn offset(angle: f32, distance: f32) -> (i32, i32) {
    // Photoshop measures the light's angle; the shadow falls opposite.
    let a = angle.to_radians();
    (
        (-a.cos() * distance).round() as i32,
        (a.sin() * distance).round() as i32,
    )
}

fn erode(a: &[f32], w: usize, h: usize, r: f32) -> Vec<f32> {
    let inv: Vec<_> = a.iter().map(|v| 1. - v).collect();
    dilate(&inv, w, h, r).iter().map(|v| 1. - v).collect()
}
fn adjusted(cov: &mut [f32], alpha: &[f32], w: usize, o: &StyleOptions, clip: bool) {
    for (i, v) in cov.iter_mut().enumerate() {
        let source = if clip && alpha[i] > 0. {
            *v / alpha[i]
        } else {
            *v
        };
        *v = contour(source, o) * (1. - o.noise / 100. * grain(i % w, i / w));
        if clip {
            *v *= alpha[i]
        }
    }
}
/// Build a translation-independent source canvas. Full source bounds keep
/// shadows and gradient geometry stable when the layer crosses document edges.
/// Document-anchored patterns/textures intentionally retain their old path.
fn translation_source(doc: &Document, n: &Node) -> Option<(Document, (i32, i32))> {
    for (index, style) in n.styles.iter().enumerate() {
        let option = n.style_options.get(index);
        if option.is_some_and(|o| !o.enabled) {
            continue;
        }
        match style {
            LayerStyle::PatternOverlay { .. } => return None,
            LayerStyle::Stroke { .. } if option.is_some_and(|o| o.fill == FillType::Pattern) => {
                return None;
            }
            LayerStyle::BevelEmboss { texture, .. }
                if *texture != 0.
                    || option
                        .is_some_and(|o| o.pattern.image.is_some() && o.texture_depth != 0.) =>
            {
                return None;
            }
            _ => {}
        }
    }
    let (width, height, placement) = match &n.kind {
        NodeKind::Raster { raster, placement } => (raster.width(), raster.height(), *placement),
        NodeKind::Smart {
            source,
            cache,
            placement,
            offset,
            ..
        } => (
            cache.width(),
            cache.height(),
            crate::smart::cache_placement(
                placement,
                (source.width(), source.height()),
                (cache.width(), cache.height()),
                *offset,
            ),
        ),
        _ => return None,
    };
    let transform = placement.to_doc(width, height);
    let corners = [
        glam::dvec2(0., 0.),
        glam::dvec2(width as f64, 0.),
        glam::dvec2(0., height as f64),
        glam::dvec2(width as f64, height as f64),
    ]
    .map(|p| transform.transform_point2(p));
    let mut lo = corners[0];
    let mut hi = lo;
    for p in corners {
        lo = lo.min(p);
        hi = hi.max(p);
    }
    if !lo.is_finite() || !hi.is_finite() {
        return None;
    }
    let x = lo.x.floor();
    let y = lo.y.floor();
    let w = hi.x.ceil() - x;
    let h = hi.y.ceil() - y;
    if x < i32::MIN as f64
        || x > i32::MAX as f64
        || y < i32::MIN as f64
        || y > i32::MAX as f64
        || w < 1.
        || h < 1.
        || w > crate::document::MAX_SIDE as f64
        || h > crate::document::MAX_SIDE as f64
        || w * h > crate::document::MAX_PIXELS as f64
    {
        return None;
    }
    let mut local = Document::new(w as u32, h as u32);
    local.global_light = doc.global_light;
    local.blend_space = doc.blend_space;
    let mut node = n.clone();
    node.parent = None;
    node.clip_to = None;
    match &mut node.kind {
        NodeKind::Raster { placement, .. } | NodeKind::Smart { placement, .. } => {
            placement.x -= x;
            placement.y -= y;
        }
        _ => unreachable!(),
    }
    local.nodes.push(node);
    Some((local, (x as i32, y as i32)))
}

/// Render independently blended effects, sharing their pixels across integer moves.
pub fn render(doc: &Document, n: &Node) -> Option<Arc<Rendered>> {
    if !has_enabled_effects(n) {
        return None;
    }
    if let Some((local, (x, y))) = translation_source(doc, n) {
        let rendered = render_cached(&local, &local.nodes[0])?;
        if x == 0 && y == 0 {
            return Some(rendered);
        }
        let shifted = |effect: &RenderedEffect| RenderedEffect {
            raster: effect.raster.clone(),
            rect: IRect::new(
                effect.rect.x.saturating_add(x),
                effect.rect.y.saturating_add(y),
                effect.rect.w,
                effect.rect.h,
            ),
            blend: effect.blend,
        };
        return Some(Arc::new(Rendered {
            below: rendered.below.iter().map(shifted).collect(),
            above: rendered.above.iter().map(shifted).collect(),
        }));
    }
    render_cached(doc, n)
}

fn render_cached(doc: &Document, n: &Node) -> Option<Arc<Rendered>> {
    let key = key_for(doc, n)?;
    if let Some(rendered) = memo().lock().get(&key) {
        return Some(rendered);
    }
    let pad = n
        .styles
        .iter()
        .map(|s| {
            s.spread()
                .max(if let LayerStyle::BevelEmboss { size, .. } = s {
                    (*size * 3.).ceil() as i32
                } else {
                    0
                })
        })
        .max()
        .unwrap_or(0)
        .clamp(0, 600);
    let (alpha, r) = alpha_of(doc, n, pad)?;
    let (w, h) = (r.w as usize, r.h as usize);
    let bounds = IRect::new(r.x + pad, r.y + pad, r.w - 2 * pad, r.h - 2 * pad);
    let mut result = Rendered {
        below: vec![],
        above: vec![],
    };
    let lin = |c: [u8; 3]| {
        [
            color::srgb_to_linear(c[0] as f32 / 255.),
            color::srgb_to_linear(c[1] as f32 / 255.),
            color::srgb_to_linear(c[2] as f32 / 255.),
            1.,
        ]
    };
    for (index, style) in n.styles.iter().enumerate() {
        let default = StyleOptions::default();
        let o = n.style_options.get(index).unwrap_or(&default);
        if !o.enabled || !o.valid() {
            continue;
        }
        let light_angle = |a: f32| {
            if o.use_global_light {
                doc.global_light.angle
            } else {
                a
            }
        };
        // Overlays borrow the source alpha; spatial effects populate their own coverage.
        let mut cov = Vec::new();
        let mut below = false;
        let opacity;
        let mut solid = [0.; 4];
        let mut colors: Option<ColorMapping> = None;
        match style {
            LayerStyle::DropShadow {
                color,
                opacity: op,
                angle,
                distance,
                size,
            } => {
                let (dx, dy) = offset(light_angle(*angle), *distance);
                let spread = *size * o.spread / 100.;
                cov = blur(
                    &shift(&dilate(&alpha, w, h, spread), w, h, dx, dy),
                    w,
                    h,
                    *size - spread,
                );
                adjusted(&mut cov, &alpha, w, o, false);
                below = true;
                opacity = *op;
                solid = lin(*color);
            }
            LayerStyle::InnerShadow {
                color,
                opacity: op,
                angle,
                distance,
                size,
            } => {
                let (dx, dy) = offset(light_angle(*angle), *distance);
                let inner = erode(&alpha, w, h, *size * o.choke / 100.);
                let inv: Vec<_> = shift(&inner, w, h, dx, dy).iter().map(|v| 1. - v).collect();
                cov = blur(&inv, w, h, *size * (1. - o.choke / 100.))
                    .iter()
                    .zip(&alpha)
                    .map(|(v, a)| v * a)
                    .collect();
                adjusted(&mut cov, &alpha, w, o, true);
                opacity = *op;
                solid = lin(*color);
            }
            LayerStyle::OuterGlow {
                color,
                opacity: op,
                size,
            }
            | LayerStyle::InnerGlow {
                color,
                opacity: op,
                size,
            } => {
                below = matches!(style, LayerStyle::OuterGlow { .. });
                let amount = if below { o.spread } else { o.choke } / 100.;
                let base = if below {
                    dilate(&alpha, w, h, *size * amount)
                } else {
                    erode(&alpha, w, h, *size * amount)
                };
                let soft = match o.technique {
                    Technique::Smooth => blur(&base, w, h, *size * (1. - amount)),
                    Technique::ChiselHard => {
                        if below {
                            dilate(&base, w, h, *size * (1. - amount))
                        } else {
                            erode(&base, w, h, *size * (1. - amount))
                        }
                    }
                    Technique::ChiselSoft => blur(&base, w, h, *size * (1. - amount) * 0.5),
                };
                cov = soft
                    .iter()
                    .zip(&alpha)
                    .map(|(v, a)| {
                        if below {
                            v * (1. - a)
                        } else {
                            a * if o.glow_source == GlowSource::Center {
                                *v
                            } else {
                                (1. - v) * 2.
                            }
                        }
                    })
                    .collect();
                adjusted(&mut cov, &alpha, w, o, !below);
                opacity = *op;
                solid = lin(*color);
                if o.fill == FillType::Gradient {
                    colors = Some((*color, [255; 3], 0., 8., ColorSampling::CoverageGradient));
                }
            }
            LayerStyle::Stroke {
                color,
                opacity: op,
                size,
            } => {
                let (outer, inner) = match o.stroke_position {
                    StrokePosition::Outside => (dilate(&alpha, w, h, *size), alpha.clone()),
                    StrokePosition::Inside => (alpha.clone(), erode(&alpha, w, h, *size)),
                    StrokePosition::Center => (
                        dilate(&alpha, w, h, *size / 2.),
                        erode(&alpha, w, h, *size / 2.),
                    ),
                };
                cov = outer
                    .iter()
                    .zip(inner)
                    .map(|(a, b)| (a - b).max(0.))
                    .collect();
                below = o.stroke_position == StrokePosition::Outside;
                opacity = *op;
                solid = lin(*color);
                if o.fill != FillType::Solid {
                    colors = Some((
                        *color,
                        [255; 3],
                        0.,
                        8.,
                        if o.fill == FillType::Gradient {
                            ColorSampling::SpatialGradient
                        } else {
                            ColorSampling::Pattern
                        },
                    ));
                }
            }
            LayerStyle::ColorOverlay { color, opacity: op } => {
                solid = lin(*color);
                opacity = *op;
            }
            LayerStyle::GradientOverlay {
                from,
                to,
                angle,
                opacity: op,
            } => {
                opacity = *op;
                colors = Some((*from, *to, *angle, 8., ColorSampling::SpatialGradient));
            }
            LayerStyle::PatternOverlay {
                from,
                to,
                opacity: op,
                scale,
                angle,
            } => {
                opacity = *op;
                colors = Some((*from, *to, *angle, *scale, ColorSampling::Pattern));
            }
            LayerStyle::Satin {
                color,
                opacity: op,
                angle,
                distance,
                size,
            } => {
                let soft = blur(&alpha, w, h, *size);
                let (dx, dy) = offset(*angle, *distance);
                let left = shift(&soft, w, h, dx, dy);
                let right = shift(&soft, w, h, -dx, -dy);
                cov = left.iter().zip(right).map(|(a, b)| (a - b).abs()).collect();
                adjusted(&mut cov, &alpha, w, o, false);
                cov.iter_mut().zip(&alpha).for_each(|(v, a)| *v *= a);
                solid = lin(*color);
                opacity = *op;
            }
            LayerStyle::BevelEmboss {
                highlight,
                shadow,
                opacity: op,
                angle,
                size,
                depth,
                contour: power,
                texture,
                texture_scale,
            } => {
                let base = match o.bevel_style {
                    BevelStyle::Inner => alpha.clone(),
                    BevelStyle::Outer => dilate(&alpha, w, h, *size),
                    BevelStyle::Emboss | BevelStyle::Pillow => dilate(&alpha, w, h, *size / 2.),
                    BevelStyle::Stroke => {
                        let out = dilate(&alpha, w, h, *size);
                        let inner = erode(&alpha, w, h, *size);
                        out.iter().zip(inner).map(|(a, b)| a - b).collect()
                    }
                };
                let mut height = match o.technique {
                    Technique::Smooth => blur(&base, w, h, *size),
                    Technique::ChiselHard => {
                        let mut v = vec![0.; w * h];
                        let steps = (*size).ceil().clamp(1., 64.) as usize;
                        for step in 0..steps {
                            let e = erode(&base, w, h, step as f32);
                            for (i, a) in e.iter().enumerate() {
                                v[i] += a / steps as f32
                            }
                        }
                        v
                    }
                    Technique::ChiselSoft => blur(&base, w, h, *size / 3.),
                };
                if o.soften > 0. {
                    height = blur(&height, w, h, o.soften)
                }
                let (ly, lx) = light_angle(*angle).to_radians().sin_cos();
                let altitude = if o.use_global_light {
                    doc.global_light.altitude
                } else {
                    o.altitude
                }
                .to_radians();
                let mut light = vec![0.; w * h];
                let mut dark = vec![0.; w * h];
                let texture_value = |x: f32, y: f32| {
                    if o.pattern.image.is_some() {
                        let p = pattern(x, y, &o.pattern, [0; 3], [255; 3], *texture_scale, 0.);
                        (p[0] * 0.2126 + p[1] * 0.7152 + p[2] * 0.0722) * o.texture_depth / 100.
                            * if o.texture_invert { -1. } else { 1. }
                    } else {
                        (x * std::f32::consts::TAU / texture_scale.max(2.)).sin()
                            * (y * std::f32::consts::TAU / texture_scale.max(2.)).sin()
                            * texture
                            / 100.
                    }
                };
                for y in 0..h {
                    for x in 0..w {
                        let i = y * w + x;
                        let px = x as f32 + r.x as f32;
                        let py = y as f32 + r.y as f32;
                        let gx = (height[y * w + (x + 1).min(w - 1)]
                            - height[y * w + x.saturating_sub(1)])
                            * size.max(1.)
                            + texture_value(px + 1., py)
                            - texture_value(px - 1., py);
                        let gy = (height[(y + 1).min(h - 1) * w + x]
                            - height[y.saturating_sub(1) * w + x])
                            * size.max(1.)
                            + texture_value(px, py + 1.)
                            - texture_value(px, py - 1.);
                        let slope = (gx * lx - gy * ly) * depth / 100.;
                        let mut shade = (slope * altitude.cos() + altitude.sin())
                            / (1. + gx * gx + gy * gy).sqrt()
                            - altitude.sin();
                        if o.bevel_style == BevelStyle::Pillow {
                            shade *= if alpha[i] > 0.5 { -1. } else { 1. }
                        }
                        let mask = match o.bevel_style {
                            BevelStyle::Inner => alpha[i],
                            BevelStyle::Outer => base[i] * (1. - alpha[i]),
                            _ => base[i],
                        };
                        let strength =
                            contour(shade.abs().clamp(0., 1.).powf((*power / 100.).max(0.25)), o)
                                * mask;
                        if shade >= 0. {
                            light[i] = strength
                        } else {
                            dark[i] = strength
                        }
                    }
                }
                let mut push = |coverage: Vec<f32>, c: [u8; 3], opacity: f32, blend: BlendMode| {
                    let c = lin(c);
                    let raster = Raster::from_fn(w as u32, h as u32, [0; 4], |x, y| {
                        let a =
                            (coverage[y as usize * w + x as usize] * opacity / 100.).clamp(0., 1.);
                        color::f_to_px([c[0] * a, c[1] * a, c[2] * a, a])
                    });
                    result.above.push(RenderedEffect {
                        raster: Arc::new(raster),
                        rect: r,
                        blend,
                    });
                };
                push(dark, *shadow, *op * o.shadow_opacity / 100., o.shadow_blend);
                push(
                    light,
                    *highlight,
                    *op * o.highlight_opacity / 100.,
                    o.highlight_blend,
                );
                continue;
            }
        }
        let cov = if cov.is_empty() {
            alpha.as_slice()
        } else {
            cov.as_slice()
        };
        let raster = Raster::from_fn(w as u32, h as u32, [0; 4], |x, y| {
            let i = y as usize * w + x as usize;
            let v = cov[i];
            let c = colors.map_or(solid, |(from, to, angle, scale, kind)| {
                let x = (i % w) as f32 + r.x as f32;
                let y = (i / w) as f32 + r.y as f32;
                if kind == ColorSampling::Pattern {
                    pattern(x, y, &o.pattern, from, to, scale, angle)
                } else {
                    gradient(
                        if kind == ColorSampling::CoverageGradient {
                            v
                        } else {
                            gradient_position(x, y, bounds, angle, &o.gradient)
                        },
                        &o.gradient,
                        from,
                        to,
                    )
                }
            });
            let a = (v * opacity / 100. * c[3]).clamp(0., 1.);
            color::f_to_px([c[0] * a, c[1] * a, c[2] * a, a])
        });
        let effect = RenderedEffect {
            raster: Arc::new(raster),
            rect: r,
            blend: o.blend,
        };
        if below {
            result.below.push(effect)
        } else {
            result.above.push(effect)
        }
    }
    let rendered = Arc::new(result);
    memo().lock().insert(key, &rendered, doc, n);
    Some(rendered)
}

fn rendered_bytes(r: &Rendered) -> usize {
    r.below
        .iter()
        .chain(&r.above)
        .map(|e| {
            // Planes allocate whole 256² tiles, including edge tiles and mip
            // levels. A one-pixel effect is not an eight-byte allocation.
            (0..=e.raster.max_level())
                .map(|level| {
                    let (w, h) = e.raster.tiles_at(level);
                    (w as usize)
                        .saturating_mul(h as usize)
                        .saturating_mul(emulsion_raster::TILE_PX)
                        .saturating_mul(std::mem::size_of::<[u16; 4]>())
                })
                .sum::<usize>()
        })
        .sum()
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
            assert_eq!(second.above.first().unwrap().raster.get(32, 32), [0; 4]);
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
        let effect = effects.above.first().unwrap();
        let pixel = effect
            .raster
            .get((32 - effect.rect.x) as u32, (32 - effect.rect.y) as u32);
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
