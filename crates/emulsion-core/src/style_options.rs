//! Backward-compatible advanced layer-effect settings. Coordinates are document pixels.
use crate::styles::LayerStyle;
use emulsion_raster::BlendMode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GlobalLight {
    pub angle: f32,
    pub altitude: f32,
}
impl Default for GlobalLight {
    fn default() -> Self {
        Self {
            angle: 120.,
            altitude: 30.,
        }
    }
}
impl GlobalLight {
    pub fn valid(&self) -> bool {
        self.angle.is_finite() && self.altitude.is_finite() && (0. ..=90.).contains(&self.altitude)
    }
}
macro_rules! choices { ($name:ident,$first:ident $(,$rest:ident)*) => {
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="kebab-case")]
pub enum $name { #[default] $first, $($rest),* }
}; }
choices!(GradientKind, Linear, Radial, Angle, Reflected, Diamond);
choices!(FillType, Solid, Gradient, Pattern);
choices!(StrokePosition, Outside, Inside, Center);
choices!(BevelStyle, Inner, Outer, Emboss, Pillow, Stroke);
choices!(Technique, Smooth, ChiselHard, ChiselSoft);
choices!(GlowSource, Edge, Center);
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    pub position: f32,
    pub color: [u8; 4],
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GradientSettings {
    pub stops: Vec<GradientStop>,
    pub kind: GradientKind,
    /// Additional rotation in degrees (also available to gradient strokes).
    pub angle: f32,
    pub scale: f32,
    pub offset_x: f32,
    pub offset_y: f32,
    pub reverse: bool,
}
impl Default for GradientSettings {
    fn default() -> Self {
        Self {
            stops: vec![],
            kind: GradientKind::Linear,
            angle: 0.,
            scale: 100.,
            offset_x: 0.,
            offset_y: 0.,
            reverse: false,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PatternImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PatternSettings {
    pub image: Option<Arc<PatternImage>>,
    pub scale: f32,
    pub offset_x: f32,
    pub offset_y: f32,
    pub angle: f32,
}
impl Default for PatternSettings {
    fn default() -> Self {
        Self {
            image: None,
            scale: 100.,
            offset_x: 0.,
            offset_y: 0.,
            angle: 0.,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContourPoint {
    pub x: f32,
    pub y: f32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StyleOptions {
    pub id: u64,
    pub enabled: bool,
    pub blend: BlendMode,
    pub use_global_light: bool,
    pub spread: f32,
    pub choke: f32,
    pub noise: f32,
    pub contour: Vec<ContourPoint>,
    pub invert_contour: bool,
    pub fill: FillType,
    pub stroke_position: StrokePosition,
    pub gradient: GradientSettings,
    pub pattern: PatternSettings,
    pub glow_source: GlowSource,
    pub technique: Technique,
    pub bevel_style: BevelStyle,
    pub altitude: f32,
    pub soften: f32,
    pub highlight_opacity: f32,
    pub shadow_opacity: f32,
    pub highlight_blend: BlendMode,
    pub shadow_blend: BlendMode,
    pub texture_depth: f32,
    pub texture_invert: bool,
}
impl Default for StyleOptions {
    fn default() -> Self {
        Self {
            id: 0,
            enabled: true,
            blend: BlendMode::Normal,
            use_global_light: false,
            spread: 0.,
            choke: 0.,
            noise: 0.,
            contour: vec![],
            invert_contour: false,
            fill: FillType::Solid,
            stroke_position: StrokePosition::Outside,
            gradient: GradientSettings::default(),
            pattern: PatternSettings::default(),
            glow_source: GlowSource::Edge,
            technique: Technique::Smooth,
            bevel_style: BevelStyle::Inner,
            altitude: 30.,
            soften: 0.,
            highlight_opacity: 100.,
            shadow_opacity: 100.,
            highlight_blend: BlendMode::Normal,
            shadow_blend: BlendMode::Normal,
            texture_depth: 0.,
            texture_invert: false,
        }
    }
}
impl StyleOptions {
    pub fn for_style(_style: &LayerStyle) -> Self {
        Self::default()
    }
    pub fn valid(&self) -> bool {
        let finite = [
            self.spread,
            self.choke,
            self.noise,
            self.altitude,
            self.soften,
            self.highlight_opacity,
            self.shadow_opacity,
            self.texture_depth,
            self.gradient.angle,
            self.gradient.scale,
            self.gradient.offset_x,
            self.gradient.offset_y,
            self.pattern.scale,
            self.pattern.offset_x,
            self.pattern.offset_y,
            self.pattern.angle,
        ]
        .iter()
        .all(|x| x.is_finite());
        finite
            && [
                self.spread,
                self.choke,
                self.noise,
                self.highlight_opacity,
                self.shadow_opacity,
            ]
            .iter()
            .all(|x| (0. ..=100.).contains(x))
            && (0. ..=90.).contains(&self.altitude)
            && (0. ..=100.).contains(&self.soften)
            && (-1000. ..=1000.).contains(&self.texture_depth)
            && self.gradient.scale > 0.
            && self.pattern.scale > 0.
            && self.gradient.stops.len() <= 64
            && self
                .gradient
                .stops
                .iter()
                .all(|s| s.position.is_finite() && (0. ..=1.).contains(&s.position))
            && self.contour.len() <= 64
            && self.contour.iter().all(|p| {
                p.x.is_finite()
                    && p.y.is_finite()
                    && (0. ..=1.).contains(&p.x)
                    && (0. ..=1.).contains(&p.y)
            })
            && self.pattern.image.as_ref().is_none_or(|i| {
                i.width > 0
                    && i.height > 0
                    && i.width <= 2048
                    && i.height <= 2048
                    && (i.width as u64 * i.height as u64 * 4) == i.pixels.len() as u64
            })
    }
}
