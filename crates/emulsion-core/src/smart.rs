//! Smart layers: source pixels plus a filter stack, rendered into a cache.
//!
//! Filters may spread past the source (blurs), so the cache can be larger
//! than the source; `offset` says where the cache's top-left sits in
//! source pixels (zero or negative). The cache is placed so that source
//! pixel (0, 0) lands exactly where it did before filtering, whatever the
//! placement's rotation or scale.

pub use emulsion_filters::{Filter, FilterStyle};
use emulsion_filters::{apply_stack, apply_stack_styled};
use emulsion_raster::{Placement, Raster};
use glam::dvec2;
use std::sync::Arc;

/// Render the stack. Returns the cache and its offset in source pixels.
pub fn render(source: &Raster, filters: &[Filter]) -> (Arc<Raster>, (i32, i32)) {
    let (r, off) = apply_stack(source, filters);
    (Arc::new(r), off)
}

pub fn render_styled(
    source: &Raster,
    filters: &[Filter],
    styles: &[FilterStyle],
) -> (Arc<Raster>, (i32, i32)) {
    let (r, off) = apply_stack_styled(source, filters, styles);
    (Arc::new(r), off)
}

/// Whether any authored stage contributes. Opacity zero is still an enabled stage.
pub fn has_active_filters(filters: &[Filter], styles: &[FilterStyle], enabled: bool) -> bool {
    enabled
        && filters
            .iter()
            .enumerate()
            .any(|(index, _)| styles.get(index).is_none_or(|style| style.enabled))
}

/// Render authoritative, unmasked Smart pixels. A bypass aliases the source and
/// discards all derived expansion; retained descriptors and masks stay untouched.
pub fn render_stack(
    source: &Arc<Raster>,
    filters: &[Filter],
    styles: &[FilterStyle],
    enabled: bool,
) -> (Arc<Raster>, (i32, i32)) {
    if !has_active_filters(filters, styles, enabled) {
        return (source.clone(), (0, 0));
    }
    render_styled(source, filters, styles)
}

/// The placement to draw a cache of `cw × ch` with, given the source's
/// placement and size and the cache's offset.
pub fn cache_placement(
    p: &Placement,
    (sw, sh): (u32, u32),
    (cw, ch): (u32, u32),
    offset: (i32, i32),
) -> Placement {
    if offset == (0, 0) && (sw, sh) == (cw, ch) {
        return *p;
    }
    let want = p.to_doc(sw, sh).transform_point2(dvec2(0.0, 0.0));
    let mut q = *p;
    let got = q
        .to_doc(cw, ch)
        .transform_point2(dvec2(-offset.0 as f64, -offset.1 as f64));
    q.x += want.x - got.x;
    q.y += want.y - got.y;
    q
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_placement_keeps_source_origin_fixed() {
        let p = Placement {
            x: 100.0,
            y: 50.0,
            scale_x: 1.5,
            scale_y: 1.5,
            rotation: 30.0,
            flip_x: false,
            flip_y: false,
        };
        let q = cache_placement(&p, (200, 100), (240, 140), (-20, -20));
        let a = p.to_doc(200, 100).transform_point2(dvec2(0.0, 0.0));
        let b = q.to_doc(240, 140).transform_point2(dvec2(20.0, 20.0));
        assert!((a - b).length() < 1e-6, "{a} vs {b}");
        // And an arbitrary source pixel too, since scale and rotation are shared.
        let a = p.to_doc(200, 100).transform_point2(dvec2(150.0, 70.0));
        let b = q.to_doc(240, 140).transform_point2(dvec2(170.0, 90.0));
        assert!((a - b).length() < 1e-6);
    }

    #[test]
    fn invert_stack_mask_source_edit_and_undo_use_existing_smart_lifecycle() {
        use crate::{Command, Document, Editor, Node, NodeKind, SmartFilterMask};
        use emulsion_raster::Mask;
        let source = Arc::new(Raster::from_fn(3, 1, [0; 4], |x, _| {
            [x as u16 * 12000, 2000, 4000, 32768]
        }));
        let placement = Placement::at(4.0, 5.0);
        let mut doc = Document::new(10, 8);
        doc.nodes
            .push(Node::smart(1, "Source", source.clone(), vec![], placement));
        doc.next_id = 2;
        let mut editor = Editor::new(doc, None);
        editor
            .execute(Command::SetFilters {
                id: 1,
                filters: vec![Filter::Invert],
            })
            .unwrap();
        let filtered = editor.doc.clone();
        let NodeKind::Smart {
            cache,
            offset,
            source: retained,
            placement: actual,
            ..
        } = &filtered.nodes[0].kind
        else {
            panic!()
        };
        assert_eq!(*offset, (0, 0));
        assert_eq!(actual.require_legacy("legacy fixture").unwrap(), placement);
        assert!(Arc::ptr_eq(retained, &source));
        assert_ne!(
            cache.read_rect(cache.bounds()),
            source.read_rect(source.bounds())
        );
        editor
            .execute(Command::SetSmartFilterMask {
                id: 1,
                mask: Some(SmartFilterMask::new(Arc::new(Mask::from_gray8(
                    3,
                    1,
                    &[0, 128, 255],
                )))),
            })
            .unwrap();
        let masked = editor.doc.clone();
        let effective = crate::smart_filter_mask::effective_pixels(&masked.nodes[0])
            .unwrap()
            .unwrap();
        for (x, coverage) in [0u32, 128, 255].into_iter().enumerate() {
            let s = source.get(x as u32, 0);
            let f = cache.get(x as u32, 0);
            let expected = std::array::from_fn(|ch| {
                ((u32::from(s[ch]) * (255 - coverage) + u32::from(f[ch]) * coverage + 127) / 255)
                    as u16
            });
            assert_eq!(effective.get(x as u32, 0), expected);
        }
        editor
            .execute(Command::SetFilterStyles {
                id: 1,
                styles: vec![FilterStyle {
                    opacity: 0.0,
                    ..Default::default()
                }],
            })
            .unwrap();
        let NodeKind::Smart {
            cache: disabled,
            filters,
            ..
        } = &editor.doc.nodes[0].kind
        else {
            panic!()
        };
        assert_eq!(filters, &[Filter::Invert]);
        assert_eq!(
            disabled.read_rect(disabled.bounds()),
            source.read_rect(source.bounds())
        );
        assert!(editor.undo());
        assert_eq!(editor.doc, masked);
        let replacement = Arc::new(Raster::from_fn(3, 1, [0; 4], |x, _| {
            [2000, x as u16 * 16000, 8000, 40000]
        }));
        crate::photo_source::replace(&mut editor, 1, replacement.clone()).unwrap();
        let edited = editor.doc.clone();
        let NodeKind::Smart {
            source: retained,
            cache: updated,
            filters,
            filter_mask,
            offset,
            placement: actual,
            ..
        } = &edited.nodes[0].kind
        else {
            panic!()
        };
        assert!(Arc::ptr_eq(retained, &replacement));
        assert!(!Arc::ptr_eq(cache, updated));
        assert_eq!(filters, &[Filter::Invert]);
        assert_eq!(*offset, (0, 0));
        assert_eq!(actual.require_legacy("legacy fixture").unwrap(), placement);
        let descriptor = crate::smart_filter_mask::descriptor(&masked.nodes[0]);
        assert_eq!(filter_mask.as_ref(), descriptor);
        let (expected, _) = render(&replacement, &[Filter::Invert]);
        assert_eq!(
            updated.read_rect(updated.bounds()),
            expected.read_rect(expected.bounds())
        );
        assert!(editor.undo());
        assert_eq!(editor.doc, masked);
        assert!(editor.redo());
        assert_eq!(editor.doc, edited);
        editor
            .execute(Command::SetFilters {
                id: 1,
                filters: vec![],
            })
            .unwrap();
        assert_eq!(
            crate::smart_filter_mask::descriptor(&editor.doc.nodes[0]),
            descriptor
        );
        let NodeKind::Smart { cache: cleared, .. } = &editor.doc.nodes[0].kind else {
            panic!()
        };
        assert_eq!(
            cleared.read_rect(cleared.bounds()),
            replacement.read_rect(replacement.bounds())
        );
        assert!(editor.undo());
        assert_eq!(editor.doc, edited);
        assert!(editor.undo());
        assert_eq!(editor.doc, masked);
        assert!(editor.undo());
        assert_eq!(editor.doc, filtered);
    }

    #[test]
    fn render_reports_spread() {
        let src = Raster::solid(20, 20, [1.0, 0.0, 0.0, 1.0]);
        let (cache, off) = render(&src, &[Filter::GaussianBlur { radius: 4.0 }]);
        assert!(cache.width() > 20 && off.0 < 0);
        let (same, off0) = render(&src, &[]);
        assert_eq!((same.width(), off0), (20, (0, 0)));
    }
}

/// Restore source layers without baking Smart filters. Text remains editable
/// when the combined transform can be represented without shear.
pub fn restore_source(
    node: &crate::node::Node,
    width: u32,
    height: u32,
) -> Result<crate::node::NodeKind, &'static str> {
    use crate::node::{NodeKind, SmartEditable};
    let NodeKind::Smart {
        source,
        placement,
        editable,
        ..
    } = &node.kind
    else {
        return Err("select a Smart Object");
    };
    node.require_affine_capability("restore Smart source")
        .map_err(|_| "restore Smart source is unavailable while projective metadata is retained")?;
    let placement = placement
        .require_legacy("restore Smart source")
        .map_err(|_| "projective Smart source")?;
    let transform = placement.to_doc(source.width(), source.height());
    Ok(match editable {
        Some(SmartEditable::Document { .. }) => {
            return Err("Open Edit Source to edit the nested source document");
        }
        Some(SmartEditable::Svg { .. }) => {
            return Err(
                "SVG source is retained as a scalable object; rasterize explicitly to edit pixels",
            );
        }
        None => NodeKind::Raster {
            raster: source.clone(),
            placement,
        },
        Some(SmartEditable::Path { path, style }) => {
            let mut path = (**path).clone();
            path.transform(transform);
            let path = Arc::new(path);
            let cache =
                crate::vector_cache::VectorRaster::path(path.clone(), *style, width, height);
            NodeKind::Path {
                path,
                style: *style,
                cache,
            }
        }
        Some(SmartEditable::Text { spec }) => {
            let mut spec = (**spec).clone();
            let combined = transform * spec.transform();
            let x = combined.matrix2.x_axis;
            let y = combined.matrix2.y_axis;
            let sx = x.length();
            let sy = y.length();
            if sx < 1e-6 || sy < 1e-6 || x.dot(y).abs() > sx * sy * 1e-5 {
                return Err(
                    "this sheared transform cannot become editable text; rasterize it instead",
                );
            }
            spec.x = combined.translation.x as f32;
            spec.y = combined.translation.y as f32;
            spec.rotation = x.y.atan2(x.x).to_degrees() as f32;
            spec.scale_x = sx as f32;
            spec.scale_y = (sy * combined.matrix2.determinant().signum()) as f32;
            let spec = Arc::new(spec);
            let cache = crate::vector_cache::VectorRaster::text(spec.clone(), width, height);
            NodeKind::Text { spec, cache }
        }
    })
}

#[cfg(test)]
mod editable_source_tests {
    use super::*;
    use crate::command::Slot;
    use crate::text::TextSpec;
    use crate::{Command, Document, Node, NodeKind};

    #[test]
    fn smart_text_roundtrip_preserves_editability_transform_and_undo() {
        let mut editor = crate::history::Editor::new(Document::new(180, 100), None);
        let id = editor
            .execute(Command::AddNode {
                node: Box::new(Node::text(
                    0,
                    "Type",
                    TextSpec {
                        text: "Editable".into(),
                        size: 18.0,
                        x: 12.0,
                        y: 10.0,
                        ..Default::default()
                    },
                    180,
                    100,
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let original = editor.doc.node(id).unwrap().kind.clone();
        editor.execute(Command::ConvertToSmart { id }).unwrap();
        assert!(matches!(
            editor.doc.node(id).unwrap().kind,
            NodeKind::Smart {
                editable: Some(_),
                ..
            }
        ));
        editor
            .execute(Command::SetPlacement {
                id,
                placement: Placement {
                    scale_x: 1.5,
                    scale_y: 0.8,
                    x: 3.0,
                    ..Default::default()
                },
            })
            .unwrap();
        editor.execute(Command::ConvertToLayers { id }).unwrap();
        let NodeKind::Text { spec, .. } = &editor.doc.node(id).unwrap().kind else {
            panic!("text remains editable")
        };
        assert_eq!(spec.text, "Editable");
        assert_eq!((spec.scale_x, spec.scale_y), (1.5, 0.8));
        assert!(spec.x.is_finite() && spec.y.is_finite());
        assert!(editor.undo());
        assert!(matches!(
            editor.doc.node(id).unwrap().kind,
            NodeKind::Smart { .. }
        ));
        assert!(editor.undo());
        assert!(editor.undo());
        assert_eq!(editor.doc.node(id).unwrap().kind, original);
    }

    #[test]
    fn rasterize_text_preserves_pixels_and_can_be_undone() {
        let mut editor = crate::history::Editor::new(Document::new(100, 60), None);
        let id = editor
            .execute(Command::AddNode {
                node: Box::new(Node::text(
                    0,
                    "Type",
                    TextSpec {
                        text: "Text".into(),
                        size: 20.0,
                        ..Default::default()
                    },
                    100,
                    60,
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let before =
            emulsion_raster::composite::flatten(&editor.doc.composite_tree(), 0).to_srgba8();
        editor.execute(Command::Rasterize { id }).unwrap();
        assert!(matches!(
            editor.doc.node(id).unwrap().kind,
            NodeKind::Raster { .. }
        ));
        assert_eq!(
            emulsion_raster::composite::flatten(&editor.doc.composite_tree(), 0).to_srgba8(),
            before
        );
        assert!(editor.undo());
        assert!(matches!(
            editor.doc.node(id).unwrap().kind,
            NodeKind::Text { .. }
        ));
    }

    #[test]
    fn smart_source_uses_complete_local_text_bounds_and_restores_position() {
        let spec = TextSpec {
            text: "Outside the canvas".into(),
            size: 24.0,
            x: -18.0,
            y: -8.0,
            ..Default::default()
        };
        let bounds = crate::text::bounds(&spec);
        let mut doc = Document::new(80, 40);
        let id = Command::AddNode {
            node: Box::new(Node::text(0, "Offcanvas", spec.clone(), 80, 40)),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        Command::ConvertToSmart { id }.apply(&mut doc).unwrap();
        let NodeKind::Smart {
            source,
            placement,
            editable: Some(crate::node::SmartEditable::Text { spec: local }),
            ..
        } = &doc.node(id).unwrap().kind
        else {
            panic!("smart text")
        };
        let placement = placement.require_legacy("legacy fixture").unwrap();
        assert_eq!(
            (source.width(), source.height()),
            ((bounds.w + 4) as u32, (bounds.h + 4) as u32)
        );
        assert_eq!(
            (placement.x, placement.y),
            ((bounds.x - 2) as f64, (bounds.y - 2) as f64)
        );
        assert!(
            source.width() > doc.width,
            "offcanvas glyphs remain in Smart source"
        );
        assert_eq!(
            (local.x + placement.x as f32, local.y + placement.y as f32),
            (spec.x, spec.y)
        );
        Command::ConvertToLayers { id }.apply(&mut doc).unwrap();
        let NodeKind::Text { spec: restored, .. } = &doc.node(id).unwrap().kind else {
            panic!("restored text")
        };
        assert_eq!(**restored, spec);
    }

    #[test]
    fn smart_path_uses_local_bounds_and_restores_original_geometry() {
        use emulsion_raster::vector::{Path, PathStyle};
        let path = Arc::new(Path::from_svg("M -10 12 L 24 12 L 24 32 Z").unwrap());
        let style = PathStyle {
            stroke: None,
            fill: Some([30, 60, 90, 255]),
            ..Default::default()
        };
        let bounds = path.bounds(&style);
        let mut doc = Document::new(300, 200);
        let id = Command::AddNode {
            node: Box::new(Node::path(0, "Path", path.clone(), style, 300, 200)),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        Command::ConvertToSmart { id }.apply(&mut doc).unwrap();
        let NodeKind::Smart {
            source, placement, ..
        } = &doc.node(id).unwrap().kind
        else {
            panic!("smart path")
        };
        let placement = placement.require_legacy("legacy fixture").unwrap();
        assert_eq!(
            (source.width(), source.height()),
            (bounds.w as u32, bounds.h as u32)
        );
        assert_eq!(
            (placement.x, placement.y),
            (bounds.x as f64, bounds.y as f64)
        );
        assert!(source.width() < 60 && source.height() < 40);
        Command::ConvertToLayers { id }.apply(&mut doc).unwrap();
        let NodeKind::Path { path: restored, .. } = &doc.node(id).unwrap().kind else {
            panic!("restored path")
        };
        assert_eq!(**restored, *path);
    }
}
