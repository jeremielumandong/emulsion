use super::*;
use crate::editor::Tool;
use emulsion_core::NodeKind;

#[gpui_kit::test]
fn sparse_raster_handles_resize_ink_without_replacing_source_and_undo(cx: &mut TestAppContext) {
    let source = Arc::new(Raster::from_fn(400, 300, [0; 4], |x, y| {
        if (60..100).contains(&x) && (70..100).contains(&y) {
            [65535; 4]
        } else {
            [0; 4]
        }
    }));
    let mut original = Document::new(400, 300);
    let id = Command::AddNode {
        node: Box::new(Node::raster(
            0,
            "Sparse",
            source.clone(),
            Default::default(),
        )),
        slot: Slot::TOP,
    }
    .apply(&mut original)
    .unwrap()
    .unwrap();
    let (ws, cx) = open(cx, original.clone());
    let view = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| {
        view.update(cx, |e, cx| {
            e.set_layer_selection(vec![id], Some(id));
            e.set_tool(Tool::Move, cx);
            window.focus(&e.canvas_focus, cx);
        })
    });
    cx.run_until_parked();
    let (start, end) = cx.update(|_, cx| {
        let e = view.read(cx);
        assert_eq!(
            e.transform_box().unwrap(),
            [(60., 70.), (100., 70.), (100., 100.), (60., 100.)]
        );
        (
            e.doc_to_window((100., 100.)).unwrap(),
            e.doc_to_window((140., 130.)).unwrap(),
        )
    });
    cx.simulate_mouse_down(start, gpui_kit::MouseButton::Left, Default::default());
    cx.simulate_mouse_move(end, Some(gpui_kit::MouseButton::Left), Default::default());
    cx.simulate_mouse_up(end, gpui_kit::MouseButton::Left, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = view.read(cx);
        let q = e.transform_box().unwrap();
        for (actual, expected) in
            q.into_iter()
                .zip([(60., 70.), (140., 70.), (140., 130.), (60., 130.)])
        {
            assert!((actual.0 - expected.0).abs() < 0.1 && (actual.1 - expected.1).abs() < 0.1);
        }
        let NodeKind::Raster { raster, .. } = &e.editor.doc.node(id).unwrap().kind else {
            panic!()
        };
        assert!(Arc::ptr_eq(raster, &source));
        assert_eq!(e.editor.history.len(), 1);
    });
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, original));
}

#[gpui_kit::test]
fn vertical_type_shortcut_creates_upright_editable_text(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, Document::new(400, 400));
    let view = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| {
        let focus = view.read(cx).canvas_focus.clone();
        window.focus(&focus, cx);
    });
    cx.simulate_keystrokes("shift-t");
    cx.run_until_parked();
    let position = cx.update(|_, cx| view.read(cx).doc_to_window((80., 40.)).unwrap());
    cx.simulate_click(position, Default::default());
    cx.simulate_keystrokes("A B C ctrl-enter");
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = view.read(cx);
        let NodeKind::Text { spec, .. } = &e.editor.doc.nodes[0].kind else {
            panic!("editable text expected")
        };
        assert_eq!(spec.text, "ABC");
        assert!(spec.vertical);
        assert_eq!(spec.rotation, 0.);
        let bounds = emulsion_core::text::bounds(spec);
        assert!(bounds.h > bounds.w);
        assert_eq!(e.editor.history.len(), 1);
    });
}

#[gpui_kit::test]
fn text_free_transform_scales_editable_horizontal_and_vertical_type(cx: &mut TestAppContext) {
    let mut original = Document::new(600, 600);
    Command::AddNode {
        node: Box::new(Node::text(
            0,
            "Type",
            emulsion_core::text::TextSpec {
                text: "Ab".into(),
                x: 60.,
                y: 60.,
                size: 32.,
                ..Default::default()
            },
            600,
            600,
        )),
        slot: Slot::TOP,
    }
    .apply(&mut original)
    .unwrap();
    let (ws, cx) = open(cx, original.clone());
    let view = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    for vertical in [false, true] {
        cx.update(|window, cx| {
            view.update(cx, |e, cx| {
                let id = e.editor.doc.nodes[0].id;
                let NodeKind::Text { spec, .. } = &e.editor.doc.nodes[0].kind else {
                    panic!()
                };
                let mut spec = (**spec).clone();
                spec.vertical = vertical;
                e.execute(
                    Command::SetText {
                        id,
                        spec: Box::new(spec),
                    },
                    cx,
                );
                e.selected = Some(id);
                window.focus(&e.panel_focus, cx);
            })
        });
        let before = cx.update(|_, cx| view.read(cx).editor.doc.clone());
        cx.simulate_keystrokes("ctrl-t");
        cx.run_until_parked();
        let (start, end) = cx.update(|_, cx| {
            let e = view.read(cx);
            let q = e.transform_box().expect("text handles");
            (
                e.doc_to_window(q[2]).unwrap(),
                e.doc_to_window((
                    q[0].0 + 2. * (q[2].0 - q[0].0),
                    q[0].1 + 2. * (q[2].1 - q[0].1),
                ))
                .unwrap(),
            )
        });
        cx.simulate_mouse_down(start, gpui_kit::MouseButton::Left, Default::default());
        cx.simulate_mouse_move(end, Some(gpui_kit::MouseButton::Left), Default::default());
        cx.simulate_mouse_up(end, gpui_kit::MouseButton::Left, Default::default());
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = view.read(cx);
            let NodeKind::Text { spec, .. } = &e.editor.doc.nodes[0].kind else {
                panic!("text must stay editable")
            };
            assert_eq!(spec.text, "Ab");
            assert_eq!(spec.vertical, vertical);
            assert!((spec.scale_x - 2.).abs() < 0.05);
            assert!((spec.scale_y - 2.).abs() < 0.05);
        });
        cx.simulate_keystrokes("ctrl-z");
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, before));
    }
}

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

#[gpui_kit::test]
fn masked_solid_raster_frame_tracks_mask_enabled_without_cropping_source(cx: &mut TestAppContext) {
    let raster = Arc::new(Raster::solid(300, 200, [1.; 4]));
    let mut node = Node::raster(0, "Masked shape", raster.clone(), Placement::default());
    node.mask = Some(Arc::new(emulsion_raster::select::rect(
        300, 200, 40., 60., 50., 20.,
    )));
    let mut doc = Document::new(300, 200);
    let id = Command::AddNode {
        node: Box::new(node),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .unwrap()
    .unwrap();
    let (ws, cx) = open(cx, doc);
    let view = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.set_layer_selection(vec![id], Some(id));
            e.set_tool(Tool::Move, cx);
            assert_eq!(
                e.transform_box().unwrap(),
                [(40., 60.), (90., 60.), (90., 80.), (40., 80.)]
            );
            e.flip_transform_selection(true, cx);
            let q = e.transform_box().unwrap();
            assert_eq!(q, [(90., 60.), (40., 60.), (40., 80.), (90., 80.)]);
            let NodeKind::Raster { raster: stored, .. } = &e.editor.doc.node(id).unwrap().kind
            else {
                panic!()
            };
            assert!(Arc::ptr_eq(stored, &raster));
            e.undo(cx);
            e.execute(Command::SetMaskEnabled { id, enabled: false }, cx);
            assert_eq!(
                e.transform_box().unwrap(),
                [(0., 0.), (300., 0.), (300., 200.), (0., 200.)]
            );
        })
    });
}
