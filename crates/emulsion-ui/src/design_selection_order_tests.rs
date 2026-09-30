//! Pointer selection and stacking order of overlapping Design objects.
use super::*;
use crate::editor::{EditorView, Tool};
use emulsion_core::{
    NodeId,
    project::{ProjectEditor, ProjectKind},
};
use emulsion_raster::{vector::PathStyle, vector_geometry::rectangle};
use gpui_kit::{Modifiers, MouseButton, test::TestWindowExt};

fn rectangle_node(doc: &mut Document, name: &str, rect: [f64; 4]) -> NodeId {
    Command::AddNode {
        node: Box::new(Node::path(
            0,
            name,
            Arc::new(rectangle(rect[0], rect[1], rect[2], rect[3])),
            PathStyle {
                fill: Some([60, 130, 220, 255]),
                stroke: None,
                ..Default::default()
            },
            600,
            400,
        )),
        slot: Slot::TOP,
    }
    .apply(doc)
    .unwrap()
    .unwrap()
}

fn design(cx: &mut TestAppContext, doc: Document) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (ws, cx) = open(cx, Document::new(600, 400));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1000.)));
    let editor = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Design, doc).unwrap(),
                "Stacking".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    (editor, cx)
}

#[gpui_kit::test]
fn design_layer_selection_wins_over_overlapping_objects_when_dragging(cx: &mut TestAppContext) {
    for cover_on_top in [false, true] {
        for multi in [false, true] {
            let mut doc = Document::new(600, 400);
            let small = rectangle_node(&mut doc, "Small", [120., 80., 160., 160.]);
            let large = rectangle_node(&mut doc, "Large", [20., 20., 540., 340.]);
            let other = rectangle_node(&mut doc, "Other", [400., 260., 50., 40.]);
            if !cover_on_top {
                Command::MoveNode {
                    id: large,
                    slot: Slot {
                        parent: None,
                        index: 0,
                    },
                }
                .apply(&mut doc)
                .unwrap();
            }
            let original = doc.clone();
            let (editor, cx) = design(cx, doc);
            cx.update(|_, cx| {
                editor.update(cx, |e, cx| {
                    e.set_tool(Tool::Move, cx);
                    e.select_layer_row(small, false, false, cx);
                    if multi {
                        e.select_layer_row(other, true, false, cx);
                    }
                })
            });
            cx.run_until_parked();
            let (start, end) = cx.update(|_, cx| {
                let e = editor.read(cx);
                (
                    e.doc_to_window((200., 160.)).unwrap(),
                    e.doc_to_window((230., 180.)).unwrap(),
                )
            });
            cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
            cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
            cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
            cx.run_until_parked();
            cx.update(|_, cx| {
                editor.update(cx, |e, cx| {
                    assert_eq!(e.editor.doc.node(large), original.node(large));
                    let bounds =
                        emulsion_core::geometry::node_bounds(&e.editor.doc, small).unwrap();
                    assert!(
                        (bounds.x - 150).abs() <= 1 && (bounds.y - 100).abs() <= 1,
                        "wrong drag target: {bounds:?}, cover={cover_on_top}, multi={multi}"
                    );
                    assert!(e.selected_layer_ids().contains(&small));
                    if multi {
                        assert_ne!(e.editor.doc.node(other), original.node(other));
                    }
                    e.undo(cx);
                    assert_eq!(e.editor.doc, original);
                })
            });
            // Explicit Alt picking still chooses the foremost overlapping object.
            if cover_on_top {
                cx.update(|_, cx| {
                    editor.update(cx, |e, cx| e.select_layer_row(small, false, false, cx))
                });
                cx.run_until_parked();
                cx.simulate_click(
                    start,
                    Modifiers {
                        alt: true,
                        ..Modifiers::none()
                    },
                );
                cx.run_until_parked();
                cx.update(|_, cx| assert_eq!(editor.read(cx).selected, Some(large)));
            }
        }
    }
}

#[gpui_kit::test]
fn design_toolbar_and_shortcuts_switch_between_select_and_hand(cx: &mut TestAppContext) {
    let (editor, cx) = design(cx, Document::new(600, 400));
    cx.update(|window, cx| {
        assert!(window.find("design-select").visible());
        assert!(window.find("design-hand").visible());
        window.click("design-hand", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.tool, Tool::Hand);
        assert!(!e.tools.rotate_view);
    });
    cx.update(|window, cx| window.click("design-select", cx));
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(editor.read(cx).tool, Tool::Move));

    // The Photo rail's bare-key shortcuts reach the Design canvas too.
    cx.update(|window, cx| editor.update(cx, |e, cx| window.focus(&e.canvas_focus, cx)));
    cx.simulate_keystrokes("h");
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(editor.read(cx).tool, Tool::Hand));
    cx.simulate_keystrokes("v");
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(editor.read(cx).tool, Tool::Move));
}

fn index_input(cx: &mut VisualTestContext, value: &str) {
    cx.update(|window, cx| window.click("design-layer-index-value", cx));
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    });
    cx.simulate_input(value);
    cx.run_until_parked();
    cx.update(|window, cx| window.click("ok", cx));
    cx.run_until_parked();
}

#[gpui_kit::test]
fn design_arrange_controls_and_index_preserve_artwork_and_undo(cx: &mut TestAppContext) {
    for nested in [false, true] {
        let mut doc = Document::new(600, 400);
        let ids: Vec<_> = (0..4)
            .map(|i| {
                rectangle_node(
                    &mut doc,
                    &format!("Object {i}"),
                    [50. + i as f64 * 80., 100., 60., 60.],
                )
            })
            .collect();
        let parent = if nested {
            Command::Group {
                ids: ids.clone(),
                name: "Group".into(),
            }
            .apply(&mut doc)
            .unwrap()
        } else {
            None
        };
        let original = doc.clone();
        let (editor, cx) = design(cx, doc);
        cx.update(|window, cx| {
            editor.update(cx, |e, cx| e.select_layer_row(ids[1], false, false, cx));
            window.click("design-position", cx);
        });
        cx.run_until_parked();
        for (button, expected) in [
            ("design-front", vec![ids[0], ids[2], ids[3], ids[1]]),
            ("design-back", vec![ids[1], ids[0], ids[2], ids[3]]),
            ("design-forward", vec![ids[0], ids[2], ids[1], ids[3]]),
            ("design-backward", vec![ids[1], ids[0], ids[2], ids[3]]),
        ] {
            cx.update(|window, cx| window.click(button, cx));
            cx.run_until_parked();
            cx.update(|_, cx| {
                editor.update(cx, |e, cx| {
                    assert_eq!(e.editor.doc.children(parent), expected, "{button}");
                    for id in &ids {
                        assert_eq!(
                            emulsion_core::geometry::node_bounds(&e.editor.doc, *id),
                            emulsion_core::geometry::node_bounds(&original, *id)
                        );
                    }
                    e.undo(cx);
                    assert_eq!(e.editor.doc, original);
                })
            });
            cx.run_until_parked();
        }
        cx.update(|window, cx| window.click("design-layer-index", cx));
        cx.run_until_parked();
        index_input(cx, "0");
        cx.update(|window, cx| {
            assert!(window.find("design-layer-index-value").visible());
            assert_eq!(editor.read(cx).editor.doc, original);
        });
        index_input(cx, "3");
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                assert_eq!(
                    e.editor.doc.children(parent),
                    vec![ids[0], ids[2], ids[1], ids[3]]
                );
                assert_eq!(e.selected, Some(ids[1]));
                e.undo(cx);
                assert_eq!(e.editor.doc, original);
                e.redo(cx);
                assert_eq!(
                    e.editor.doc.children(parent),
                    vec![ids[0], ids[2], ids[1], ids[3]]
                );
            })
        });
    }
}

#[gpui_kit::test]
fn design_text_frame_whitespace_is_selectable(cx: &mut TestAppContext) {
    let mut doc = Document::new(600, 400);
    doc.nodes.push(Node::text(
        1,
        "Text frame",
        emulsion_core::text::TextSpec {
            text: "Short".into(),
            x: 100.,
            y: 100.,
            size: 24.,
            width: Some(240.),
            height: Some(140.),
            ..Default::default()
        },
        600,
        400,
    ));
    let (editor, cx) = design(cx, doc);
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.set_layer_selection(Vec::new(), None);
            e.set_tool(Tool::Move, cx);
        })
    });
    cx.run_until_parked();
    let point = cx.update(|_, cx| editor.read(cx).doc_to_window((300., 220.)).unwrap());
    cx.simulate_click(point, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(editor.read(cx).selected, Some(1)));
}

#[gpui_kit::test]
fn design_multiple_selection_toolbar_groups_and_undoes(cx: &mut TestAppContext) {
    let mut doc = Document::new(600, 400);
    let a = rectangle_node(&mut doc, "First", [80., 80., 60., 60.]);
    let b = rectangle_node(&mut doc, "Second", [180., 80., 60., 60.]);
    let (editor, cx) = design(cx, doc.clone());
    let origin = cx.update(|_, cx| editor.read(cx).doc_to_window((0., 0.)).unwrap());
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.set_tool(Tool::Move, cx);
            e.set_layer_selection(vec![a, b], Some(b));
            cx.notify();
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(editor.read(cx).doc_to_window((0., 0.)).unwrap(), origin);
        window.click("context-design-group", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        let group = e.selected.unwrap();
        assert!(e.editor.doc.node(group).unwrap().is_group());
        assert_eq!(e.editor.doc.node(a).unwrap().parent, Some(group));
        assert_eq!(e.editor.doc.node(b).unwrap().parent, Some(group));
    });
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, doc));
}
