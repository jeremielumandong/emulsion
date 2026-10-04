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
        if self.content
            && !matches!(
                node.kind,
                NodeKind::Text { .. } | NodeKind::Raster { .. } | NodeKind::Smart { .. }
            )
        {
            return Err("Content overrides support text, raster images and Smart Objects.".into());
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
    // Source replacement changes the local basis. Keep independent mask world
    // mapping even when content/appearance overrides are restored separately.
    let filter_world = crate::smart_filter_mask::to_document(next);
    let old_filter_world = crate::smart_filter_mask::to_document(old);
    let source_size = crate::photo_source::dimensions_from_node(next);
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
                    value.paragraphs = old.paragraphs.clone();
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
                    restore_paragraphs(old, value)?;
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
            NodeKind::Smart {
                placement,
                source: old_source,
                editable: old_editable,
                filters: old_filters,
                filter_styles: old_styles,
                filter_mask: old_filter_mask,
                ..
            },
            NodeKind::Smart {
                placement: position,
                source,
                editable,
                filters,
                filter_styles,
                filter_mask,
                cache,
                offset,
            },
        ) => {
            if !geometry && flags.appearance {
                *filters = old_filters.clone();
                *filter_styles = old_styles.clone();
                *filter_mask = old_filter_mask.clone();
                // The copied object is still in definition coordinates. Move
                // its retained coverage with the subsequent group alignment,
                // including dependent responsive reflow. The original link
                // flag and world mapping are restored in the final phase.
                if let Some(mask) = filter_mask {
                    mask.linked = true;
                }
            }
            if !geometry && flags.content {
                *source = old_source.clone();
                *editable = old_editable.clone();
            }
            if !geometry && (flags.content || flags.appearance) {
                let (rendered, origin) =
                    crate::smart::render_styled(source, filters, filter_styles);
                *cache = rendered;
                *offset = origin;
            }
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
                    paint.fill_paint = style.fill_paint;
                }
                if flags.stroke {
                    paint.stroke = style.stroke;
                    paint.stroke_paint = style.stroke_paint;
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
    if flags.appearance {
        // copy_group restores appearance in definition coordinates, before
        // replace_instance aligns the group. Rebinding to old document space
        // now would move a linked mask a second time during that alignment.
        // Keep the copied local descriptor for bounds and restore the old world
        // mapping only in the final, post-alignment geometry phase.
        if geometry {
            if let (
                Some(old_mask),
                NodeKind::Smart {
                    filter_mask: Some(mask),
                    ..
                },
            ) = (crate::smart_filter_mask::descriptor(old), &mut next.kind)
            {
                mask.linked = old_mask.linked;
            }
            crate::smart_filter_mask::preserve_world(next, old_filter_world);
        }
    } else if source_size != crate::photo_source::dimensions_from_node(next)
        || (geometry && crate::smart_filter_mask::descriptor(next).is_some_and(|mask| !mask.linked))
    {
        crate::smart_filter_mask::preserve_world(next, filter_world);
    }
    Ok(())
}
fn restore_paragraphs(
    old: &crate::text::TextSpec,
    next: &mut crate::text::TextSpec,
) -> Result<(), String> {
    if next.text == old.text {
        next.paragraphs = old.paragraphs.clone();
        return Ok(());
    }
    if next.paragraphs.is_empty() && old.paragraphs.is_empty() {
        return Ok(());
    }
    let starts = |text: &str| {
        std::iter::once(0)
            .chain(text.match_indices('\n').map(|(i, _)| i + 1))
            .collect::<Vec<_>>()
    };
    if !next.paragraphs.is_empty() {
        *next = crate::text::apply_paragraphs(next, 0..next.text.len(), Default::default())?;
        next.paragraphs.clear();
    }
    let original = starts(&old.text);
    for paragraph in &old.paragraphs {
        if let Ok(index) = original.binary_search(&paragraph.start)
            && let Some(start) = starts(&next.text).get(index).copied()
        {
            *next = crate::text::apply_paragraphs(next, start..start, paragraph.format)?;
        }
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

#[cfg(test)]
mod smart_filter_mask_override_tests {
    use super::*;
    use crate::smart::{Filter, FilterStyle};
    use crate::{MaskProperties, SmartFilterMask};
    use emulsion_raster::{BlendMode, Mask, Placement, Raster};
    use std::sync::Arc;
    fn node(width: u32) -> Node {
        let mut node = Node::smart(
            1,
            "Smart",
            Arc::new(Raster::solid(width, 4, [0.2, 0.3, 0.4, 1.])),
            vec![Filter::FindEdges],
            Placement::at(3., 2.),
        );
        let NodeKind::Smart { filter_mask, .. } = &mut node.kind else {
            unreachable!()
        };
        *filter_mask = Some(SmartFilterMask::new(Arc::new(Mask::empty(width, 4, 64))));
        node
    }
    fn assert_world(a: glam::DAffine2, b: glam::DAffine2) {
        for (x, y) in a.to_cols_array().into_iter().zip(b.to_cols_array()) {
            assert!((x - y).abs() < 1e-9);
        }
    }
    #[test]
    fn smart_appearance_override_retains_stack_styles_and_filter_mask() {
        let mut old = node(6);
        let mut next = node(8);
        let NodeKind::Smart {
            filter_mask: Some(mask),
            filter_styles,
            ..
        } = &mut old.kind
        else {
            unreachable!()
        };
        mask.enabled = false;
        mask.linked = false;
        mask.transform = [1., 0.2, 0., 1., -3., 1.];
        mask.properties = MaskProperties {
            density: 0.4,
            feather: 2.,
        };
        *filter_styles = vec![FilterStyle {
            opacity: 0.3,
            blend: BlendMode::Screen,
        }];
        let before = crate::smart_filter_mask::to_document(&old).unwrap();
        let raw = crate::smart_filter_mask::descriptor(&old)
            .unwrap()
            .pixels
            .clone();
        restore(
            &old,
            &mut next,
            Overrides {
                appearance: true,
                ..Default::default()
            },
            false,
        )
        .unwrap();
        restore(
            &old,
            &mut next,
            Overrides {
                appearance: true,
                ..Default::default()
            },
            true,
        )
        .unwrap();
        let descriptor = crate::smart_filter_mask::descriptor(&next).unwrap();
        assert!(Arc::ptr_eq(&descriptor.pixels, &raw));
        assert!(!descriptor.enabled && !descriptor.linked);
        assert_eq!(descriptor.properties.density, 0.4);
        assert_world(
            crate::smart_filter_mask::to_document(&next).unwrap(),
            before,
        );
        assert!(
            matches!(&next.kind,NodeKind::Smart{filter_styles,..} if filter_styles[0].opacity==0.3)
        );
    }
    #[test]
    fn smart_content_override_preserves_filter_mask_world_mapping_when_source_size_changes() {
        let old = node(6);
        let mut next = node(8);
        let raw = crate::smart_filter_mask::descriptor(&next)
            .unwrap()
            .pixels
            .clone();
        let before = crate::smart_filter_mask::to_document(&next).unwrap();
        restore(
            &old,
            &mut next,
            Overrides {
                content: true,
                ..Default::default()
            },
            false,
        )
        .unwrap();
        assert_eq!(
            crate::photo_source::dimensions_from_node(&next),
            Some((6, 4))
        );
        assert_world(
            crate::smart_filter_mask::to_document(&next).unwrap(),
            before,
        );
        assert!(Arc::ptr_eq(
            &crate::smart_filter_mask::descriptor(&next).unwrap().pixels,
            &raw
        ));
    }
}
