use super::*;
use crate::editor::PaintKind;
use emulsion_core::{Document, Node, NodeKind};
use emulsion_raster::vector::{PathPaint, PathStyle};
use emulsion_raster::{Mask, select};

fn vector_shape(fill: Option<[u8; 4]>, paint: PathPaint) -> Document {
    let mut doc = Document::new(128, 96);
    doc.nodes.push(Node::path(
        1,
        "Rectangle",
        Arc::new(emulsion_raster::vector_geometry::rectangle(
            20., 20., 80., 50.,
        )),
        PathStyle {
            fill,
            fill_paint: paint,
            stroke: Some([20, 20, 20, 255]),
            width: 4.,
            ..Default::default()
        },
        128,
        96,
    ));
    doc.next_id = 2;
    doc
}

#[gpui_kit::test]
fn vector_bucket_and_color_drop_preserve_geometry_stroke_and_layer(cx: &mut TestAppContext) {
    let original = vector_shape(None, PathPaint::Solid);
    let (ws, cx) = open(cx, original.clone());
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.set_layer_selection(vec![1], Some(1));
            e.set_paint(PaintKind::Bucket, cx);
            e.set_fg([20, 180, 70, 255], cx);
        })
    });
    cx.run_until_parked();
    let outside = cx.update(|_, cx| e.read(cx).doc_to_window((10., 10.)).unwrap());
    cx.simulate_click(outside, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(e.read(cx).editor.doc, original));
    let inside = cx.update(|_, cx| e.read(cx).doc_to_window((60., 45.)).unwrap());
    cx.simulate_click(inside, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert_eq!(e.editor.doc.nodes.len(), 1);
            let NodeKind::Path { path, style, .. } = &e.editor.doc.nodes[0].kind else {
                panic!("vector preserved");
            };
            let NodeKind::Path {
                path: old_path,
                style: old_style,
                ..
            } = &original.nodes[0].kind
            else {
                unreachable!()
            };
            assert!(Arc::ptr_eq(path, old_path));
            assert_eq!(style.fill, Some([20, 180, 70, 255]));
            assert_eq!(style.stroke, old_style.stroke);
            assert_eq!(style.width, old_style.width);
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
            e.color_drop([30, 50, 210, 255], inside, cx);
            let NodeKind::Path { style, .. } = &e.editor.doc.nodes[0].kind else {
                panic!("drop preserves vector");
            };
            assert_eq!(style.fill, Some([30, 50, 210, 255]));
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
        })
    });
}

#[gpui_kit::test]
fn partial_vector_fill_keeps_gradient_stroke_mask_and_one_undo(cx: &mut TestAppContext) {
    let mut original = vector_shape(
        Some([255, 0, 0, 255]),
        PathPaint::LinearGradient {
            end: [0, 255, 0, 255],
            angle: 0.,
        },
    );
    original.selection = Some(Arc::new(select::rect(128, 96, 20., 20., 40., 50.)));
    original.nodes[0].mask = Some(Arc::new(Mask::from_fn(128, 96, 255, |x, _| {
        if x < 35 { 0 } else { 180 }
    })));
    let NodeKind::Path {
        cache: old_cache,
        path,
        style,
        ..
    } = &original.nodes[0].kind
    else {
        unreachable!()
    };
    let old_cache = old_cache.clone();
    let target = path.rasterize(
        &PathStyle {
            fill: Some([0, 0, 255, 255]),
            fill_paint: PathPaint::Solid,
            ..*style
        },
        128,
        96,
    );
    let (ws, cx) = open(cx, original.clone());
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.set_layer_selection(vec![1], Some(1));
            e.set_fg([0, 0, 255, 255], cx);
            e.fill_selection(cx);
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert_eq!(e.editor.doc.nodes.len(), 1);
            let node = &e.editor.doc.nodes[0];
            let NodeKind::Raster { raster, .. } = &node.kind else {
                panic!("partial fill rasterizes same layer");
            };
            assert_eq!(raster.get(45, 45), target.get(45, 45));
            assert_eq!(
                raster.get(80, 45),
                old_cache.get(80, 45),
                "unselected gradient survives"
            );
            assert_eq!(
                raster.get(30, 45),
                old_cache.get(30, 45),
                "hidden masked pixels survive"
            );
            assert_eq!(raster.get(18, 45), old_cache.get(18, 45), "stroke survives");
            assert_eq!(raster.get(5, 5)[3], 0);
            assert!(Arc::ptr_eq(
                node.mask.as_ref().unwrap(),
                original.nodes[0].mask.as_ref().unwrap()
            ));
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
            assert!(!e.editor.history.can_undo());
        })
    });
}

#[gpui_kit::test]
fn full_selection_keeps_vector_and_pixel_locked_fill_does_not_create_layer(
    cx: &mut TestAppContext,
) {
    let mut original = vector_shape(Some([255, 0, 0, 255]), PathPaint::Solid);
    original.selection = Some(Arc::new(Mask::white(128, 96)));
    let (ws, cx) = open(cx, original.clone());
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.set_layer_selection(vec![1], Some(1));
            e.set_fg([0, 0, 255, 255], cx);
            e.fill_selection(cx);
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            let NodeKind::Path { style, .. } = &e.editor.doc.nodes[0].kind else {
                panic!("full selection retains editable shape");
            };
            assert_eq!(style.fill, Some([0, 0, 255, 255]));
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
            e.editor.doc.nodes[0].locks.pixels = true;
            let locked = e.editor.doc.clone();
            e.fill_selection(cx);
            assert_eq!(e.editor.doc, locked);
        })
    });
}
