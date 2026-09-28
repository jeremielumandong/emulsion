use super::*;
use crate::{Node, NodeKind};

/// Explicit property groups retained when another instance publishes a change.
/// Appearance includes paint, typography, blending and effects; opacity and
/// visibility are separate so they can follow the source independently.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Overrides {
    pub content: bool,
    pub appearance: bool,
    pub geometry: bool,
    pub opacity: bool,
    pub visibility: bool,
    pub fill: bool,
    pub stroke: bool,
    pub stroke_width: bool,
    pub font_family: bool,
    pub font_size: bool,
    pub text_color: bool,
    pub position: bool,
    pub size: bool,
    pub effects: bool,
}
impl Overrides {
    pub fn is_empty(self) -> bool {
        self == Self::default()
    }
    pub fn validate(self, node: &Node) -> Result<(), String> {
        if (self.font_family || self.font_size || self.text_color)
            && !matches!(node.kind, NodeKind::Text { .. })
        {
            return Err("Typography overrides need a text object.".into());
        }
        if self.fill && !matches!(node.kind, NodeKind::Path { .. } | NodeKind::Fill { .. }) {
            return Err("Fill overrides need a path or fill object.".into());
        }
        if (self.stroke || self.stroke_width) && !matches!(node.kind, NodeKind::Path { .. }) {
            return Err("Stroke overrides need a path object.".into());
        }
        if self.content && !matches!(node.kind, NodeKind::Text { .. } | NodeKind::Raster { .. }) {
            return Err("Content overrides support text and raster images.".into());
        }
        if (self.geometry || self.position || self.size)
            && !matches!(
                node.kind,
                NodeKind::Text { .. }
                    | NodeKind::Path { .. }
                    | NodeKind::Raster { .. }
                    | NodeKind::Smart { .. }
            )
        {
            return Err("Geometry overrides apply to individual text, path or image objects; select a child of the group.".into());
        }
        Ok(())
    }
}
pub(super) fn restore(
    old: &Node,
    next: &mut Node,
    flags: Overrides,
    geometry: bool,
) -> Result<(), String> {
    flags.validate(old)?;
    if std::mem::discriminant(&old.kind) != std::mem::discriminant(&next.kind)
        && (flags.content
            || flags.appearance
            || flags.geometry
            || flags.fill
            || flags.stroke
            || flags.stroke_width
            || flags.font_family
            || flags.font_size
            || flags.text_color
            || flags.position
            || flags.size)
    {
        return Err(
            "An overridden member changed object type. Clear its overrides before publishing."
                .into(),
        );
    }
    if !geometry {
        if flags.opacity {
            next.opacity = old.opacity;
        }
        if flags.visibility {
            next.visible = old.visible;
        }
        if flags.appearance {
            next.blend = old.blend;
            next.blending = old.blending;
            next.styles = old.styles.clone();
            next.style_options = old.style_options.clone();
            next.effects_enabled = old.effects_enabled;
        }
        if flags.effects {
            next.styles = old.styles.clone();
            next.style_options = old.style_options.clone();
            next.effects_enabled = old.effects_enabled;
        }
    }
    match (&old.kind, &mut next.kind) {
        (NodeKind::Text { spec: old, .. }, NodeKind::Text { spec: next, .. }) => {
            let value = std::sync::Arc::make_mut(next);
            if geometry && flags.geometry {
                value.x = old.x;
                value.y = old.y;
                value.width = old.width;
                value.height = old.height;
                value.rotation = old.rotation;
                value.scale_x = old.scale_x;
                value.scale_y = old.scale_y;
                value.vertical = old.vertical;
                value.warp = old.warp;
                value.text_path = old.text_path.clone();
            } else if !geometry {
                if flags.content {
                    value.text = old.text.clone();
                    value.runs.clear();
                }
                if flags.appearance {
                    value.font = old.font.clone();
                    value.size = old.size;
                    value.color = old.color;
                    value.bold = old.bold;
                    value.italic = old.italic;
                    value.underline = old.underline;
                    value.strikethrough = old.strikethrough;
                    value.line_height = old.line_height;
                    value.align = old.align;
                    value.anti_alias = old.anti_alias;
                    value.letter_spacing = old.letter_spacing;
                    value.runs = if value.text == old.text {
                        old.runs.clone()
                    } else {
                        Vec::new()
                    };
                    if value.text != old.text {
                        value.apply_style(0..value.text.len(), |style| *style = old.style_at(0));
                    }
                }
            }
        }
        (
            NodeKind::Path { path, style, .. },
            NodeKind::Path {
                path: next,
                style: paint,
                ..
            },
        ) => {
            if geometry && flags.geometry {
                *next = path.clone();
            }
            if !geometry && flags.appearance {
                *paint = *style;
            }
        }
        (
            NodeKind::Raster { raster, placement },
            NodeKind::Raster {
                raster: next,
                placement: position,
            },
        ) => {
            if !geometry && flags.content {
                *next = raster.clone();
            }
            if geometry && flags.geometry {
                *position = *placement;
            }
        }
        (
            NodeKind::Smart { placement, .. },
            NodeKind::Smart {
                placement: position,
                ..
            },
        ) => {
            if geometry && flags.geometry {
                *position = *placement;
            }
        }
        (NodeKind::Fill { rgba }, NodeKind::Fill { rgba: paint })
            if !geometry && flags.appearance =>
        {
            *paint = *rgba;
        }
        _ => (),
    }
    match (&old.kind, &mut next.kind) {
        (NodeKind::Text { spec: old, .. }, NodeKind::Text { spec: next, .. }) => {
            let value = std::sync::Arc::make_mut(next);
            if geometry {
                if flags.position {
                    value.x = old.x;
                    value.y = old.y;
                }
                if flags.size {
                    value.width = old.width;
                    value.height = old.height;
                    value.scale_x = old.scale_x;
                    value.scale_y = old.scale_y;
                }
            } else {
                if flags.font_family {
                    value.font = old.font.clone();
                    for run in &mut value.runs {
                        run.style.font = old.font.clone();
                    }
                }
                if flags.font_size {
                    value.size = old.size;
                    for run in &mut value.runs {
                        run.style.size = old.size;
                    }
                }
                if flags.text_color {
                    value.color = old.color;
                    for run in &mut value.runs {
                        run.style.color = old.color;
                    }
                }
            }
        }
        (
            NodeKind::Path {
                path: old, style, ..
            },
            NodeKind::Path {
                path, style: paint, ..
            },
        ) => {
            if geometry && !flags.geometry && (flags.position || flags.size) {
                let before = emulsion_raster::vector_geometry::bounds(old)
                    .ok_or("Cannot retain geometry of an empty path.")?;
                let current = emulsion_raster::vector_geometry::bounds(path)
                    .ok_or("Cannot retain geometry of an empty path.")?;
                let (x, y) = if flags.position {
                    (before.0, before.1)
                } else {
                    (current.0, current.1)
                };
                let (sx, sy) = if flags.size {
                    if current.2 <= 1e-8 || current.3 <= 1e-8 {
                        return Err("Cannot retain size of a degenerate path.".into());
                    }
                    (before.2 / current.2, before.3 / current.3)
                } else {
                    (1., 1.)
                };
                std::sync::Arc::make_mut(path).transform(
                    glam::DAffine2::from_translation(glam::dvec2(x, y))
                        * glam::DAffine2::from_scale(glam::dvec2(sx, sy))
                        * glam::DAffine2::from_translation(glam::dvec2(-current.0, -current.1)),
                );
            }
            if !geometry {
                if flags.fill {
                    paint.fill = style.fill;
                }
                if flags.stroke {
                    paint.stroke = style.stroke;
                }
                if flags.stroke_width {
                    paint.width = style.width;
                }
            }
        }
        (NodeKind::Fill { rgba }, NodeKind::Fill { rgba: paint }) if !geometry && flags.fill => {
            *paint = *rgba
        }
        (
            NodeKind::Raster { placement: old, .. },
            NodeKind::Raster {
                placement: next, ..
            },
        )
        | (
            NodeKind::Smart { placement: old, .. },
            NodeKind::Smart {
                placement: next, ..
            },
        ) if geometry => {
            if flags.position {
                next.x = old.x;
                next.y = old.y;
            }
            if flags.size {
                next.scale_x = old.scale_x;
                next.scale_y = old.scale_y;
            }
        }
        _ => {}
    }
    Ok(())
}
pub(super) fn refresh(node: &mut Node, width: u32, height: u32) {
    match &mut node.kind {
        NodeKind::Text { spec, cache } => {
            *cache = crate::vector_cache::VectorRaster::text(spec.clone(), width, height)
        }
        NodeKind::Path { path, style, cache } => {
            *cache = crate::vector_cache::VectorRaster::path(path.clone(), *style, width, height)
        }
        _ => (),
    }
}
