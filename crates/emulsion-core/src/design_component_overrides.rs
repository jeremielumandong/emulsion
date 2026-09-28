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
}
impl Overrides {
    pub fn is_empty(self) -> bool {
        self == Self::default()
    }
    pub fn validate(self, node: &Node) -> Result<(), String> {
        if self.content && !matches!(node.kind, NodeKind::Text { .. } | NodeKind::Raster { .. }) {
            return Err("Content overrides support text and raster images.".into());
        }
        if self.geometry
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
        && (flags.content || flags.appearance || flags.geometry)
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
