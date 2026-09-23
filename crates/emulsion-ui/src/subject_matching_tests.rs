use super::*;
use crate::editor::Tool;
use emulsion_core::NodeKind;
use emulsion_raster::composite::flatten;
use emulsion_raster::{BlendMode, Mask};
use gpui_kit::test::TestWindowExt;

#[gpui_kit::test]
fn blending_subject_stack_is_clipped_masked_and_one_undo_step(cx: &mut TestAppContext) {
    let mut document = doc(&["Background", "Subject"], None);
    let subject = document.nodes[1].id;
    document.nodes[1].mask = Some(Arc::new(Mask::from_fn(256, 192, 0, |x, _| {
        if x < 128 { 255 } else { 0 }
    })));
    let original = document.clone();
    let before = flatten(&document.composite_tree(), 0);
    let (ws, cx) = open(cx, document);
    let editor = cx.update(|_, cx| {
        let editor = ws.read(cx).editor.clone().unwrap();
        editor.update(cx, |e, cx| {
            e.set_layer_selection(vec![subject], Some(subject));
            e.set_tool(Tool::Move, cx);
        });
        editor
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("context-match-subject", cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            assert_eq!(e.editor.doc.nodes.len(), 5);
            assert_eq!(e.editor.history.len(), 1);
            for node in &e.editor.doc.nodes[2..] {
                assert_eq!(node.clip_to, Some(subject));
                assert_eq!(node.mask.as_ref().unwrap().get(200, 90), 255);
            }
            let levels = e.editor.doc.nodes[2].id;
            let saturation = e.editor.doc.nodes[3].id;
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
            e.redo(cx);
            e.execute(
                Command::SetParam {
                    id: levels,
                    key: "out_black".into(),
                    value: 75.,
                },
                cx,
            );
            e.execute(
                Command::SetParam {
                    id: saturation,
                    key: "saturation".into(),
                    value: -60.,
                },
                cx,
            );
            let after = flatten(&e.editor.doc.composite_tree(), 0);
            assert_ne!(after.get(64, 90), before.get(64, 90));
            assert_eq!(after.get(200, 90), before.get(200, 90));
            assert_eq!(e.editor.doc.node(subject), original.node(subject));
        });
    });
}

#[gpui_kit::test]
fn blending_checks_cover_whole_image_even_with_selection(cx: &mut TestAppContext) {
    let mut document = doc(&["Photo"], None);
    document.selection = Some(Arc::new(emulsion_raster::select::rect(
        256, 192, 10., 10., 20., 20.,
    )));
    let (ws, cx) = open(cx, document);
    let editor = cx.update(|_, cx| {
        let editor = ws.read(cx).editor.clone().unwrap();
        editor.update(cx, |e, cx| e.set_tool(Tool::Grade, cx));
        editor
    });
    for (button, kind) in [
        ("context-check-brightness", "brightness"),
        ("context-check-saturation", "saturation"),
        ("context-check-color", "color"),
    ] {
        cx.run_until_parked();
        cx.update(|window, cx| window.click(button, cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                let node = e.editor.doc.node(e.selected.unwrap()).unwrap();
                assert!(node.mask.is_none());
                assert!(node.clip_to.is_none());
                assert!(node.parent.is_none());
                match (&node.kind, kind) {
                    (
                        NodeKind::Adjust(emulsion_raster::Adjustment::BlackAndWhite { .. }),
                        "brightness",
                    ) => {}
                    (
                        NodeKind::Adjust(emulsion_raster::Adjustment::SelectiveColor {
                            colors,
                            relative,
                        }),
                        "saturation",
                    ) => {
                        assert!(!relative);
                        assert_eq!(colors[0][3], -100.);
                        assert_eq!(colors[8][3], 100.);
                    }
                    (NodeKind::Fill { rgba }, "color") => {
                        assert_eq!(*rgba, [128, 128, 128, 255]);
                        assert_eq!(node.blend, BlendMode::Luminosity);
                    }
                    _ => panic!("wrong check layer"),
                }
                e.undo(cx);
                assert_eq!(e.editor.doc.nodes.len(), 1);
            });
        });
    }
}
