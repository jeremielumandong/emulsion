use super::*;
use crate::editor::Tool;
use emulsion_core::NodeKind;

#[gpui_kit::test]
fn rotate_action_preserves_editable_path_and_undoes(cx: &mut TestAppContext) {
    let mut original = Document::new(64, 64);
    let id = Command::AddNode {
        node: Box::new(Node::path(
            0,
            "Drawing",
            Arc::new(emulsion_raster::vector::Path::from_svg("M 12 12 L 44 12 L 12 28 Z").unwrap()),
            Default::default(),
            64,
            64,
        )),
        slot: Slot::TOP,
    }
    .apply(&mut original)
    .unwrap()
    .unwrap();
    let (ws, cx) = open(cx, original.clone());
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.dispatch_action(crate::actions::RotateLayer90Cw);
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = e.read(cx);
        assert!(matches!(
            e.editor.doc.node(id).unwrap().kind,
            NodeKind::Path { .. }
        ));
        assert_ne!(e.editor.doc, original);
        assert_eq!(e.editor.history.len(), 1);
    });
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(e.read(cx).editor.doc, original));
}

#[gpui_kit::test]
fn selected_pixels_flip_and_rotate_each_undo_in_one_step(cx: &mut TestAppContext) {
    let mut original = doc(&["Source"], None);
    original.selection = Some(Arc::new(emulsion_raster::select::rect(
        256, 192, 20., 30., 40., 20.,
    )));
    let (ws, cx) = open(cx, original.clone());
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    for flip in [true, false] {
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                if flip {
                    e.flip_transform_selection(true, cx);
                } else {
                    e.rotate_transform_selection(90., cx);
                }
            })
        });
        cx.update(|_, cx| {
            let e = e.read(cx);
            assert_eq!(e.editor.doc.nodes.len(), 2);
            assert!(e.editor.doc.selection.is_none());
            let NodeKind::Raster { raster, placement } =
                &e.editor.doc.node(e.selected.unwrap()).unwrap().kind
            else {
                panic!()
            };
            assert_eq!((raster.width(), raster.height()), (40, 20));
            if flip {
                assert!(placement.flip_x);
            } else {
                assert_eq!(placement.rotation, 90.);
            }
        });
        cx.simulate_keystrokes("ctrl-z");
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(e.read(cx).editor.doc, original));
    }
}

#[gpui_kit::test]
fn free_transform_from_panel_scales_with_canvas_handles_and_undoes(cx: &mut TestAppContext) {
    let mut original = Document::new(256, 192);
    Command::AddNode {
        node: Box::new(Node::raster(
            0,
            "Object",
            Arc::new(Raster::solid(40, 20, [1.; 4])),
            Placement::at(40., 40.),
        )),
        slot: Slot::TOP,
    }
    .apply(&mut original)
    .unwrap();
    let (ws, cx) = open(cx, original.clone());
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| {
        let focus = e.read(cx).panel_focus.clone();
        window.focus(&focus, cx);
    });
    cx.simulate_keystrokes("ctrl-t");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(e.read(cx).tool, Tool::Move);
        assert!(e.read(cx).canvas_focus.is_focused(window));
    });
    let start = cx.update(|_, cx| e.read(cx).doc_to_window((80., 60.)).unwrap());
    let end = cx.update(|_, cx| e.read(cx).doc_to_window((120., 80.)).unwrap());
    cx.simulate_mouse_down(
        start,
        gpui_kit::MouseButton::Left,
        gpui_kit::Modifiers::none(),
    );
    cx.simulate_mouse_move(
        end,
        Some(gpui_kit::MouseButton::Left),
        gpui_kit::Modifiers::none(),
    );
    cx.simulate_mouse_up(
        end,
        gpui_kit::MouseButton::Left,
        gpui_kit::Modifiers::none(),
    );
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = e.read(cx);
        let NodeKind::Raster { placement, .. } = &e.editor.doc.nodes[0].kind else {
            panic!()
        };
        assert!((placement.scale_x - 2.).abs() < 0.05);
        assert!((placement.scale_y - 2.).abs() < 0.05);
    });
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(e.read(cx).editor.doc, original));
}
