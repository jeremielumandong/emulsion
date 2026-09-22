//! Smart layers: source pixels plus a filter stack, rendered into a cache.
//!
//! Filters may spread past the source (blurs), so the cache can be larger
//! than the source; `offset` says where the cache's top-left sits in
//! source pixels (zero or negative). The cache is placed so that source
//! pixel (0, 0) lands exactly where it did before filtering, whatever the
//! placement's rotation or scale.

use emulsion_filters::{Filter, FilterStyle, apply_stack, apply_stack_styled};
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
    let transform = placement.to_doc(source.width(), source.height());
    if node.mask.is_some() && !placement.is_identity() && editable.is_some() {
        return Err("remove the transformed Smart Object mask before restoring editable layers");
    }
    Ok(match editable {
        None => NodeKind::Raster {
            raster: source.clone(),
            placement: *placement,
        },
        Some(SmartEditable::Path { path, style }) => {
            let mut path = (**path).clone();
            path.transform(transform);
            let cache = Arc::new(path.rasterize(style, width, height));
            NodeKind::Path {
                path: Arc::new(path),
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
            let cache = Arc::new(crate::text::rasterize(&spec, width, height));
            NodeKind::Text {
                spec: Arc::new(spec),
                cache,
            }
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
