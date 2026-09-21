use super::*;
use crate::editor::{PaintKind, Tool};
use emulsion_core::{Document, Node, NodeKind};
use emulsion_raster::select;

fn shape() -> Document {
    let mut doc = Document::new(256, 192);
    let mut node = Node::new(
        1,
        "Ellipse",
        NodeKind::Fill {
            rgba: [220, 40, 30, 255],
        },
    );
    node.mask = Some(Arc::new(select::ellipse(256, 192, 40., 30., 80., 60.)));
    doc.nodes.push(node);
    doc.next_id = 2;
    doc
}

#[gpui_kit::test]
fn bucket_and_color_drop_recolor_existing_shape_without_an_extra_layer(cx: &mut TestAppContext) {
    let original = shape();
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
    let inside = cx.update(|_, cx| e.read(cx).doc_to_window((80., 60.)).unwrap());
    cx.simulate_click(inside, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert_eq!(e.editor.doc.nodes.len(), 1);
            assert!(matches!(
                e.editor.doc.nodes[0].kind,
                NodeKind::Fill {
                    rgba: [20, 180, 70, 255]
                }
            ));
            assert!(Arc::ptr_eq(
                e.editor.doc.nodes[0].mask.as_ref().unwrap(),
                original.nodes[0].mask.as_ref().unwrap()
            ));
            assert_eq!(e.selected, Some(1));
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
            e.color_drop([30, 50, 210, 255], inside, cx);
            assert!(matches!(
                e.editor.doc.nodes[0].kind,
                NodeKind::Fill {
                    rgba: [30, 50, 210, 255]
                }
            ));
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
            e.fill_selection(cx);
            assert_eq!(e.editor.doc.nodes.len(), 1);
            assert!(matches!(
                e.editor.doc.nodes[0].kind,
                NodeKind::Fill {
                    rgba: [20, 180, 70, 255]
                }
            ));
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
        })
    });
}

#[gpui_kit::test]
fn partial_shape_fill_keeps_unselected_color_mask_and_undo_restores_editability(
    cx: &mut TestAppContext,
) {
    let mut original = shape();
    original.selection = Some(Arc::new(select::rect(256, 192, 40., 30., 40., 60.)));
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
            assert_eq!(node.id, 1);
            assert!(Arc::ptr_eq(
                node.mask.as_ref().unwrap(),
                original.nodes[0].mask.as_ref().unwrap()
            ));
            let NodeKind::Raster { raster, .. } = &node.kind else {
                panic!("partial fill rasterizes only the selected shape");
            };
            assert_eq!(raster.get(60, 60), [0, 0, 65535, 65535]);
            assert_eq!(
                raster.get(100, 60),
                emulsion_raster::color::f_to_px(emulsion_raster::color::srgba8_to_premul([
                    220, 40, 30, 255
                ]))
            );
            assert_eq!(node.mask.as_ref().unwrap().get(10, 10), 0);
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
            assert!(!e.editor.history.can_undo());
        })
    });
}

#[gpui_kit::test]
fn fill_mask_edits_existing_mask_and_pixel_locks_do_not_create_layers(cx: &mut TestAppContext) {
    let mut original = shape();
    original.nodes[0].locks.pixels = true;
    let (ws, cx) = open(cx, original.clone());
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.set_layer_selection(vec![1], Some(1));
            e.set_fg([0, 0, 255, 255], cx);
            e.fill_selection(cx);
            assert_eq!(
                e.editor.doc, original,
                "locked shape is not replaced by a new layer"
            );
            e.set_tool(Tool::Mask, cx);
            e.set_paint(PaintKind::Bucket, cx);
            assert!(e.tools.mask_edit, "bucket retains the selected mask target");
            e.set_fg([0, 0, 0, 255], cx);
        })
    });
    cx.run_until_parked();
    let inside = cx.update(|_, cx| e.read(cx).doc_to_window((80., 60.)).unwrap());
    cx.simulate_click(inside, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert_eq!(e.editor.doc.nodes.len(), 1);
            assert!(matches!(
                e.editor.doc.nodes[0].kind,
                NodeKind::Fill {
                    rgba: [220, 40, 30, 255]
                }
            ));
            assert_eq!(e.editor.doc.nodes[0].mask.as_ref().unwrap().get(80, 60), 0);
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
            e.set_paint(PaintKind::Bucket, cx);
            e.set_mask_edit(true, cx);
            e.editor.doc.nodes[0].locked = true;
            let locked = e.editor.doc.clone();
            e.fill_selection(cx);
            assert_eq!(e.editor.doc, locked);
        })
    });
}
