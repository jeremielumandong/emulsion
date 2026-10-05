//! Shared canvas transform-control gestures through the real offscreen editor. Keep
//! presentation-only changes separate from document/history mutations.
use super::*;
use crate::tests::open;
use core::prelude::v1::test;
use emulsion_core::project::{ProjectEditor, ProjectKind};
use gpui_kit::test::TestWindowExt;

fn artwork() -> Document {
    let mut doc = Document::new(512, 384);
    Command::AddNode {
        node: Box::new(Node::raster(
            0,
            "Photo",
            Arc::new(Raster::solid(96, 64, [0.8, 0.2, 0.1, 1.])),
            Placement::at(208., 160.),
        )),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .unwrap();
    doc
}

#[derive(Clone, Copy, Debug)]
enum CanvasWorkspace {
    Photo,
    Paint,
    Design,
}

const CANVAS_WORKSPACES: [CanvasWorkspace; 3] = [
    CanvasWorkspace::Photo,
    CanvasWorkspace::Paint,
    CanvasWorkspace::Design,
];

fn setup(cx: &mut TestAppContext, doc: Document) -> (Entity<EditorView>, &mut VisualTestContext) {
    setup_workspace(cx, CanvasWorkspace::Photo, doc)
}

fn setup_workspace(
    cx: &mut TestAppContext,
    kind: CanvasWorkspace,
    doc: Document,
) -> (Entity<EditorView>, &mut VisualTestContext) {
    let selected = doc.nodes[0].id;
    let (workspace, cx) = open(cx, doc.clone());
    cx.simulate_resize(size(px(1440.), px(1000.)));
    let view = cx.update(|window, cx| {
        if matches!(kind, CanvasWorkspace::Design) {
            workspace.update(cx, |w, cx| {
                w.install_project(
                    ProjectEditor::new_project(ProjectKind::Design, doc).unwrap(),
                    "Design".into(),
                    window,
                    cx,
                )
            });
        }
        let view = workspace.read(cx).editor.clone().unwrap();
        view.update(cx, |e, cx| {
            if matches!(kind, CanvasWorkspace::Paint) {
                let mut layout = e.workspace_snapshot();
                layout.draw_mode = true;
                e.apply_workspace_layout(&layout, cx);
            }
            assert!(e.has_transform_controls());
            assert_eq!(
                e.is_photo_workflow(),
                matches!(kind, CanvasWorkspace::Photo)
            );
            e.set_layer_selection(vec![selected], Some(selected));
            e.set_tool(Tool::Move, cx);
            e.snap = false;
            e.view.zoom = 1.;
            e.view.center = (256., 192.);
            window.focus(&e.canvas_focus, cx);
            cx.notify();
        });
        view
    });
    cx.run_until_parked();
    (view, cx)
}

fn at(view: &Entity<EditorView>, cx: &mut VisualTestContext, p: (f64, f64)) -> Point<Pixels> {
    cx.update(|_, cx| view.read(cx).doc_to_window(p).unwrap())
}

fn screen(p: (f64, f64)) -> Point<Pixels> {
    point(px(p.0 as f32), px(p.1 as f32))
}

fn click(at: Point<Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

fn placement(e: &EditorView) -> Placement {
    let NodeKind::Raster { placement, .. } = &e.editor.doc.nodes[0].kind else {
        panic!("editable raster placement");
    };
    *placement
}

#[gpui_kit::test]
fn rotation_controls_plain_click_toggles_on_release_preserving_document_and_redo(
    cx: &mut TestAppContext,
) {
    for kind in CANVAS_WORKSPACES {
        for modified in [false, true] {
            let (view, cx) = setup_workspace(cx, kind, artwork());
            let (before, redo, revision, saved, history) = cx.update(|_, cx| {
                view.update(cx, |e, cx| {
                    // A newly created Design project starts unsaved. Establish
                    // a clean saved revision before testing both dirty baselines.
                    if matches!(kind, CanvasWorkspace::Design) {
                        let stamp = e.editor.stamp();
                        e.editor
                            .mark_project_saved("rotation-controls.emu".into(), &stamp);
                    }
                    let id = e.selected.unwrap();
                    if modified {
                        e.execute(Command::SetOpacity { id, opacity: 0.8 }, cx);
                    }
                    let before = e.editor.doc.clone();
                    e.execute(Command::SetOpacity { id, opacity: 0.4 }, cx);
                    let redo = e.editor.doc.clone();
                    e.undo(cx);
                    assert_eq!(e.editor.doc, before);
                    assert!(e.editor.history.can_redo());
                    assert_eq!(e.editor.is_modified(), modified);
                    (
                        before,
                        redo,
                        e.editor.revision,
                        e.editor.saved_revision(),
                        e.editor.history.len(),
                    )
                })
            });
            let center = at(&view, cx, (256., 192.));
            for expected in [TransformControlMode::Rotate, TransformControlMode::Resize] {
                let previous = cx.update(|_, cx| view.read(cx).transform_control_mode);
                cx.simulate_mouse_down(center, MouseButton::Left, Modifiers::none());
                cx.update(|_, cx| {
                    let e = view.read(cx);
                    assert_eq!(e.transform_control_mode, previous, "not on mouse-down");
                    assert_eq!(e.editor.doc, before);
                });
                cx.simulate_mouse_up(center, MouseButton::Left, Modifiers::none());
                cx.run_until_parked();
                cx.update(|_, cx| {
                    let e = view.read(cx);
                    assert_eq!(e.transform_control_mode, expected);
                    assert_eq!(e.editor.doc, before);
                    assert_eq!(e.editor.revision, revision);
                    assert_eq!(e.editor.saved_revision(), saved);
                    assert_eq!(e.editor.history.len(), history);
                    assert!(e.editor.history.can_redo());
                    assert_eq!(e.editor.is_modified(), modified);
                    assert!(!e.editor.in_transaction());
                    assert!(e.drag.is_none());
                });
            }
            press("ctrl-shift-z", cx);
            cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, redo));
        }
    }
}

#[gpui_kit::test]
fn rotation_controls_click_slop_is_screen_pixels_and_out_and_back_drag_cannot_toggle(
    cx: &mut TestAppContext,
) {
    for kind in CANVAS_WORKSPACES {
        let original = artwork();
        let (view, cx) = setup_workspace(cx, kind, original.clone());
        let modified = cx.update(|_, cx| view.read(cx).editor.is_modified());
        for zoom in [0.5, 2.] {
            cx.update(|_, cx| {
                view.update(cx, |e, cx| {
                    e.view.zoom = zoom;
                    e.view.rotation = 31.;
                    e.transform_control_mode = TransformControlMode::Resize;
                    cx.notify();
                })
            });
            cx.run_until_parked();
            let start = at(&view, cx, (256., 192.));
            let slight = start + point(px(3.), px(0.));
            cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
            cx.simulate_mouse_move(slight, Some(MouseButton::Left), Modifiers::none());
            cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, original));
            cx.simulate_mouse_up(slight, MouseButton::Left, Modifiers::none());
            cx.update(|_, cx| {
                assert_eq!(
                    view.read(cx).transform_control_mode,
                    TransformControlMode::Rotate
                );
                assert_eq!(view.read(cx).editor.doc, original);
            });

            let far = start + point(px(6.), px(0.));
            cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
            cx.simulate_mouse_move(far, Some(MouseButton::Left), Modifiers::none());
            cx.update(|_, cx| {
                assert_ne!(
                    view.read(cx).editor.doc,
                    original,
                    "past the drag threshold"
                );
            });
            cx.simulate_mouse_move(start, Some(MouseButton::Left), Modifiers::none());
            cx.simulate_mouse_up(start, MouseButton::Left, Modifiers::none());
            cx.run_until_parked();
            cx.update(|_, cx| {
                let e = view.read(cx);
                assert_eq!(e.transform_control_mode, TransformControlMode::Rotate);
                assert_eq!(e.editor.doc, original);
                assert!(e.editor.history.is_empty());
                assert_eq!(e.editor.is_modified(), modified);
                assert!(!e.editor.in_transaction());
            });
        }
    }
}

#[gpui_kit::test]
fn rotation_controls_modified_clicks_and_multi_clicks_never_toggle(cx: &mut TestAppContext) {
    for kind in CANVAS_WORKSPACES {
        let original = artwork();
        let (view, cx) = setup_workspace(cx, kind, original.clone());
        let center = at(&view, cx, (256., 192.));
        for modifiers in [
            Modifiers {
                shift: true,
                ..Modifiers::none()
            },
            Modifiers {
                control: true,
                ..Modifiers::none()
            },
            Modifiers {
                platform: true,
                ..Modifiers::none()
            },
            Modifiers {
                alt: true,
                ..Modifiers::none()
            },
        ] {
            cx.simulate_mouse_down(center, MouseButton::Left, modifiers);
            cx.simulate_mouse_up(center, MouseButton::Left, modifiers);
            cx.update(|_, cx| {
                let e = view.read(cx);
                assert_eq!(e.transform_control_mode, TransformControlMode::Resize);
                assert_eq!(e.editor.doc, original);
                assert!(!e.editor.in_transaction());
            });
        }
        for count in [2, 3] {
            cx.simulate_event(MouseDownEvent {
                position: center,
                button: MouseButton::Left,
                modifiers: Modifiers::none(),
                click_count: count,
                first_mouse: false,
            });
            cx.simulate_mouse_up(center, MouseButton::Left, Modifiers::none());
            cx.update(|_, cx| {
                assert_eq!(
                    view.read(cx).transform_control_mode,
                    TransformControlMode::Resize
                );
                assert_eq!(view.read(cx).editor.doc, original);
                assert!(view.read(cx).editor.history.is_empty());
            });
        }
        // A modifier added after the press must also prevent a mode toggle.
        cx.simulate_mouse_down(center, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(
            center,
            MouseButton::Left,
            Modifiers {
                shift: true,
                ..Modifiers::none()
            },
        );
        cx.update(|_, cx| {
            assert_eq!(
                view.read(cx).transform_control_mode,
                TransformControlMode::Resize
            )
        });
    }
}

#[gpui_kit::test]
fn rotation_controls_abandoned_press_focus_loss_and_late_release_do_not_toggle(
    cx: &mut TestAppContext,
) {
    let original = artwork();
    let (view, cx) = setup(cx, original.clone());
    let center = at(&view, cx, (256., 192.));
    cx.simulate_mouse_down(center, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(center, None, Modifiers::none());
    cx.simulate_mouse_up(center, MouseButton::Left, Modifiers::none());
    cx.update(|_, cx| {
        let e = view.read(cx);
        assert_eq!(e.transform_control_mode, TransformControlMode::Resize);
        assert!(e.drag.is_none());
        assert!(!e.editor.in_transaction());
    });
    // Release coordinates matter even if the system delivers no intervening move.
    cx.simulate_mouse_down(center, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(
        center + point(px(6.), px(0.)),
        MouseButton::Left,
        Modifiers::none(),
    );
    cx.update(|_, cx| {
        assert_eq!(
            view.read(cx).transform_control_mode,
            TransformControlMode::Resize
        )
    });
    cx.simulate_mouse_down(center, MouseButton::Left, Modifiers::none());
    press("escape", cx);
    cx.simulate_mouse_up(center, MouseButton::Left, Modifiers::none());
    cx.update(|_, cx| {
        assert_eq!(
            view.read(cx).transform_control_mode,
            TransformControlMode::Resize
        )
    });
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    cx.simulate_mouse_down(center, MouseButton::Left, Modifiers::none());
    cx.deactivate_window();
    cx.run_until_parked();
    cx.simulate_mouse_up(center, MouseButton::Left, Modifiers::none());
    cx.update(|_, cx| {
        let e = view.read(cx);
        assert_eq!(e.transform_control_mode, TransformControlMode::Resize);
        assert_eq!(e.editor.doc, original);
        assert!(e.editor.history.is_empty());
        assert!(!e.editor.in_transaction());
        assert!(e.drag.is_none());
    });
}

#[gpui_kit::test]
fn rotation_controls_layer_component_and_tool_changes_reset_to_resize(cx: &mut TestAppContext) {
    let mut original = artwork();
    let id = original.nodes[0].id;
    original.nodes[0].mask = Some(Arc::new(emulsion_raster::Mask::empty(96, 64, 255)));
    let (view, cx) = setup(cx, original.clone());
    let center = at(&view, cx, (256., 192.));
    click(center, cx);
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            assert_eq!(e.transform_control_mode, TransformControlMode::Rotate);
            e.set_layer_selection(vec![id], Some(id));
            assert_eq!(
                e.transform_control_mode,
                TransformControlMode::Resize,
                "same layer reselected"
            );
            e.transform_control_mode = TransformControlMode::Rotate;
            e.set_mask_edit_target(MaskEditTarget::RasterMask, cx);
            assert_eq!(
                e.transform_control_mode,
                TransformControlMode::Resize,
                "content to mask"
            );
            e.transform_control_mode = TransformControlMode::Rotate;
            e.set_mask_edit_target(MaskEditTarget::Content, cx);
            assert_eq!(
                e.transform_control_mode,
                TransformControlMode::Resize,
                "mask to content"
            );
            e.transform_control_mode = TransformControlMode::Rotate;
            e.set_tool(Tool::Hand, cx);
            assert_eq!(e.transform_control_mode, TransformControlMode::Resize);
            e.set_tool(Tool::Move, cx);
            assert_eq!(e.transform_control_mode, TransformControlMode::Resize);
            e.transform_control_mode = TransformControlMode::Rotate;
            e.set_layer_selection(Vec::new(), None);
            assert_eq!(e.transform_control_mode, TransformControlMode::Resize);
            assert_eq!(e.editor.doc, original);
            assert!(e.editor.history.is_empty());
        })
    });
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.set_layer_selection(vec![id], Some(id));
            e.set_mask_edit_target(MaskEditTarget::RasterMask, cx);
            e.transform_control_mode = TransformControlMode::Rotate;
            e.remove_mask(cx);
            assert!(e.editor.doc.node(id).unwrap().mask.is_none());
            assert_eq!(e.tools.mask_edit_target, MaskEditTarget::Content);
            assert_eq!(e.transform_control_mode, TransformControlMode::Resize);
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
            e.redo(cx);
            assert!(e.editor.doc.node(id).unwrap().mask.is_none());
            e.execute(
                Command::SetMask {
                    id,
                    mask: original.node(id).unwrap().mask.clone(),
                },
                cx,
            );
            e.set_mask_edit_target(MaskEditTarget::RasterMask, cx);
            e.transform_control_mode = TransformControlMode::Rotate;
            // Undoing mask creation must use after_change's missing-component
            // cleanup, just as explicit removal does.
            e.undo(cx);
            assert!(e.editor.doc.node(id).unwrap().mask.is_none());
            assert_eq!(e.tools.mask_edit_target, MaskEditTarget::Content);
            assert_eq!(e.transform_control_mode, TransformControlMode::Resize);
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
            assert!(e.editor.history.is_empty());
        });
    });
}

#[gpui_kit::test]
fn rotation_controls_text_double_click_still_edits_text(cx: &mut TestAppContext) {
    for kind in CANVAS_WORKSPACES {
        let mut original = Document::new(512, 384);
        Command::AddNode {
            node: Box::new(Node::text(
                0,
                "Type",
                emulsion_core::text::TextSpec {
                    text: "Hello world".into(),
                    x: 160.,
                    y: 160.,
                    size: 24.,
                    ..Default::default()
                },
                512,
                384,
            )),
            slot: Slot::TOP,
        }
        .apply(&mut original)
        .unwrap();
        let (view, cx) = setup_workspace(cx, kind, original.clone());
        let at = cx.update(|_, cx| {
            view.update(cx, |e, _| {
                e.transform_control_mode = TransformControlMode::Rotate;
                let NodeKind::Text { spec, .. } = &e.editor.doc.nodes[0].kind else {
                    panic!("text");
                };
                let caret = emulsion_core::text::layout(spec).caret(8);
                let p = spec.transform().transform_point2(glam::dvec2(
                    caret.x as f64,
                    (caret.y + caret.height * 0.5) as f64,
                ));
                e.doc_to_window((p.x, p.y)).unwrap()
            })
        });
        cx.simulate_event(MouseDownEvent {
            position: at,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 2,
            first_mouse: false,
        });
        cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
        cx.update(|_, cx| {
            assert_eq!(view.read(cx).tool, Tool::Type);
            assert_eq!(
                view.read(cx).transform_control_mode,
                TransformControlMode::Resize
            );
        });
        cx.simulate_input("earth");
        cx.update(|_, cx| {
            let NodeKind::Text { spec, .. } = &view.read(cx).editor.doc.nodes[0].kind else {
                panic!("text");
            };
            assert_eq!(spec.text, "Hello earth");
        });
        press("escape", cx);
        cx.update(|_, cx| {
            assert_eq!(view.read(cx).editor.doc, original);
            assert!(view.read(cx).editor.history.is_empty());
        });
    }
}

#[gpui_kit::test]
fn rotation_controls_explicit_rotate_pointer_enter_is_one_undo_and_redo(cx: &mut TestAppContext) {
    let original = artwork();
    let (view, cx) = setup(cx, original.clone());
    cx.dispatch_action(crate::actions::TransformRotate);
    cx.run_until_parked();
    let (start, end) = cx.update(|_, cx| {
        let e = view.read(cx);
        assert!(e.photo_transform_active());
        assert_eq!(e.transform_control_mode, TransformControlMode::Rotate);
        let quad = e.transform_screen_quad().unwrap();
        let center = ((quad[0].0 + quad[2].0) / 2., (quad[0].1 + quad[2].1) / 2.);
        (
            screen(quad[0]),
            screen((
                center.0 - (quad[0].1 - center.1),
                center.1 + quad[0].0 - center.0,
            )),
        )
    });
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.update(|_, cx| assert!(matches!(view.read(cx).drag, Some(Drag::Transform(grab)) if grab.handle == transform::Handle::Rotate)));
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    cx.update(|_, cx| {
        let e = view.read(cx);
        let p = placement(e);
        assert!(
            (p.rotation - 90.).abs() < 0.01,
            "rotation was {}",
            p.rotation
        );
        assert!((p.scale_x - 1.).abs() < 0.001);
        assert!((p.scale_y - 1.).abs() < 0.001);
        assert!(e.photo_transform_active());
        assert!(e.editor.history.is_empty());
    });
    press("enter", cx);
    let committed = cx.update(|_, cx| {
        let e = view.read(cx);
        assert!(!e.photo_transform_active());
        assert!(!e.editor.in_transaction());
        assert_eq!(e.editor.history.len(), 1);
        e.editor.doc.clone()
    });
    press("ctrl-z", cx);
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, original));
    press("ctrl-shift-z", cx);
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, committed));
    cx.dispatch_action(crate::actions::TransformScale);
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = view.read(cx);
        assert!(e.photo_transform_active());
        assert_eq!(e.transform_control_mode, TransformControlMode::Resize);
    });
    press("escape", cx);
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, committed));
}

#[gpui_kit::test]
fn rotation_controls_rotate_escape_restores_redo_and_ignores_late_release(cx: &mut TestAppContext) {
    let original = artwork();
    let (view, cx) = setup(cx, original.clone());
    let redo = cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.execute(
                Command::SetOpacity {
                    id: e.selected.unwrap(),
                    opacity: 0.4,
                },
                cx,
            );
            let redo = e.editor.doc.clone();
            e.undo(cx);
            redo
        })
    });
    cx.dispatch_action(crate::actions::TransformRotate);
    cx.run_until_parked();
    let (start, end) = cx.update(|_, cx| {
        let e = view.read(cx);
        let quad = e.transform_screen_quad().unwrap();
        (screen(quad[0]), screen(quad[1]))
    });
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
    cx.update(|_, cx| assert_ne!(view.read(cx).editor.doc, original));
    press("escape", cx);
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = view.read(cx);
        assert_eq!(e.editor.doc, original);
        assert!(e.editor.history.is_empty());
        assert!(e.editor.history.can_redo());
        assert!(!e.editor.is_modified());
        assert!(!e.editor.in_transaction());
        assert!(!e.photo_transform_active());
        assert!(e.drag.is_none());
    });
    press("ctrl-shift-z", cx);
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, redo));
}

#[gpui_kit::test]
fn rotation_controls_nub_corners_edges_and_cursors_share_rotated_screen_geometry(
    cx: &mut TestAppContext,
) {
    for kind in CANVAS_WORKSPACES {
        let mut original = artwork();
        let NodeKind::Raster { placement, .. } = &mut original.nodes[0].kind else {
            panic!("raster");
        };
        placement.rotation = 37.;
        let (view, cx) = setup_workspace(cx, kind, original.clone());
        for zoom in [0.5, 2.] {
            cx.update(|_, cx| {
                view.update(cx, |e, cx| {
                    e.view.zoom = zoom;
                    e.view.rotation = -19.;
                    cx.notify();
                })
            });
            cx.run_until_parked();
            let hover = cx.update(|_, cx| {
                view.update(cx, |e, _| {
                    let q = e.transform_screen_quad().unwrap();
                    let bounds = e.canvas_bounds().unwrap();
                    let (_, nub) = transform_controls::rotation_handle(q, bounds).unwrap();
                    let center = ((q[0].0 + q[2].0) / 2., (q[0].1 + q[2].1) / 2.);
                    e.transform_control_mode = TransformControlMode::Resize;
                    assert_eq!(e.handle_hit(screen(nub)), Some(transform::Handle::Rotate));
                    assert_eq!(e.transform_cursor(screen(nub)), CursorStyle::Crosshair);
                    for (i, corner) in q.iter().enumerate() {
                        assert_eq!(
                            e.handle_hit(screen(*corner)),
                            Some(transform::Handle::Corner(i))
                        );
                        assert!(matches!(
                            e.transform_cursor(screen(*corner)),
                            CursorStyle::ResizeLeftRight
                                | CursorStyle::ResizeUpDown
                                | CursorStyle::ResizeUpLeftDownRight
                                | CursorStyle::ResizeUpRightDownLeft
                        ));
                        let radial = (corner.0 - center.0, corner.1 - center.1);
                        let length = radial.0.hypot(radial.1);
                        let exterior = screen((
                            corner.0 + radial.0 / length * 18.,
                            corner.1 + radial.1 / length * 18.,
                        ));
                        assert_eq!(e.handle_hit(exterior), Some(transform::Handle::Rotate));
                        assert_eq!(e.transform_cursor(exterior), CursorStyle::Crosshair);
                        let interior = screen((
                            corner.0 - radial.0 / length * 12.,
                            corner.1 - radial.1 / length * 12.,
                        ));
                        assert_eq!(
                            e.handle_hit(interior),
                            None,
                            "inside the frame is not exterior rotation"
                        );
                    }
                    let edge = screen(((q[0].0 + q[1].0) / 2., (q[0].1 + q[1].1) / 2.));
                    assert_eq!(e.handle_hit(edge), Some(transform::Handle::Edge(0)));
                    e.transform_control_mode = TransformControlMode::Rotate;
                    for corner in q {
                        assert_eq!(
                            e.handle_hit(screen(corner)),
                            Some(transform::Handle::Rotate)
                        );
                        assert_eq!(e.transform_cursor(screen(corner)), CursorStyle::Crosshair);
                    }
                    assert_eq!(
                        e.handle_hit(edge),
                        None,
                        "rotation mode has no edge resize handles"
                    );
                    assert_eq!(e.transform_cursor(edge), CursorStyle::Arrow);
                    screen(nub)
                })
            });
            cx.simulate_mouse_move(hover, None, Modifiers::none());
            cx.run_until_parked();
            cx.update(|window, cx| {
                let e = view.read(cx);
                assert_eq!(e.editor.doc, original);
                assert!(e.editor.history.is_empty());
                assert!(window.find("transform-rotation-cursor").visible());
            });
        }
    }
}

#[test]
fn rotation_controls_nub_falls_back_inside_narrow_bounds_and_hides_when_none_fit() {
    let bounds = Bounds::new(point(px(0.), px(0.)), size(px(200.), px(120.)));
    let quad = [(60., 10.), (140., 10.), (140., 70.), (60., 70.)];
    let (midpoint, nub) = transform_controls::rotation_handle(quad, bounds).unwrap();
    assert_eq!(midpoint, (140., 40.));
    assert_eq!(
        nub,
        (164., 40.),
        "right edge is visible when top edge is clipped"
    );
    assert!(((nub.0 - midpoint.0).hypot(nub.1 - midpoint.1) - 24.).abs() < 0.001);
    let no_room = Bounds::new(point(px(60.), px(10.)), size(px(80.), px(60.)));
    assert_eq!(transform_controls::rotation_handle(quad, no_room), None);
    assert_eq!(
        transform_controls::rotation_handle([(100., 50.); 4], bounds),
        None
    );
}

#[gpui_kit::test]
fn rotation_controls_direct_nub_drag_rotates_without_scaling_and_commits_once(
    cx: &mut TestAppContext,
) {
    for kind in CANVAS_WORKSPACES {
        let original = artwork();
        let (view, cx) = setup_workspace(cx, kind, original.clone());
        let (start, end) = cx.update(|_, cx| {
            let e = view.read(cx);
            assert_eq!(e.transform_control_mode, TransformControlMode::Resize);
            let quad = e.transform_screen_quad().unwrap();
            let (_, nub) =
                transform_controls::rotation_handle(quad, e.canvas_bounds().unwrap()).unwrap();
            let center = ((quad[0].0 + quad[2].0) / 2., (quad[0].1 + quad[2].1) / 2.);
            (
                screen(nub),
                screen((center.0 - (nub.1 - center.1), center.1 + nub.0 - center.0)),
            )
        });
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        cx.update(|_, cx| {
        let e = view.read(cx);
        assert!(matches!(e.drag, Some(Drag::Transform(grab)) if grab.handle == transform::Handle::Rotate));
        assert_eq!(e.transform_cursor(end), CursorStyle::Crosshair);
    });
        cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
        cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        let committed = cx.update(|_, cx| {
            let e = view.read(cx);
            let p = placement(e);
            assert!((p.rotation - 90.).abs() < 0.01);
            assert!((p.scale_x - 1.).abs() < 0.001);
            assert!((p.scale_y - 1.).abs() < 0.001);
            assert_eq!(e.editor.history.len(), 1);
            assert!(!e.editor.in_transaction());
            assert!(!e.photo_transform_active());
            e.editor.doc.clone()
        });
        press("ctrl-z", cx);
        cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, original));
        press("ctrl-shift-z", cx);
        cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, committed));
    }
}

#[gpui_kit::test]
fn rotation_controls_distort_and_warp_retire_affine_rotation_controls(cx: &mut TestAppContext) {
    let original = artwork();
    let (view, cx) = setup(cx, original.clone());
    let center = at(&view, cx, (256., 192.));
    click(center, cx);
    cx.update(|_, cx| {
        assert_eq!(
            view.read(cx).transform_control_mode,
            TransformControlMode::Rotate
        );
    });
    cx.dispatch_action(crate::actions::TransformDistort);
    cx.run_until_parked();
    let corner = cx.update(|_, cx| {
        let e = view.read(cx);
        assert_eq!(e.transform_control_mode, TransformControlMode::Resize);
        assert!(!e.photo_transform_active());
        let corner = screen(e.transform_screen_quad().unwrap()[0]);
        assert_eq!(e.handle_hit(corner), Some(transform::Handle::Corner(0)));
        corner
    });
    cx.simulate_mouse_down(
        corner,
        MouseButton::Left,
        Modifiers {
            control: true,
            ..Modifiers::none()
        },
    );
    cx.update(|_, cx| {
        assert!(matches!(
            view.read(cx).drag,
            Some(Drag::Distort { corner: 0, .. })
        ));
    });
    press("escape", cx);
    cx.simulate_mouse_up(corner, MouseButton::Left, Modifiers::none());
    click(center, cx);
    let (corner, exterior) = cx.update(|_, cx| {
        let e = view.read(cx);
        assert_eq!(e.transform_control_mode, TransformControlMode::Rotate);
        let corner = screen(e.transform_screen_quad().unwrap()[0]);
        let exterior = corner - point(px(12.), px(12.));
        assert_eq!(e.transform_cursor(corner), CursorStyle::Crosshair);
        assert_eq!(e.transform_cursor(exterior), CursorStyle::Crosshair);
        (corner, exterior)
    });
    cx.dispatch_action(crate::actions::TransformWarp);
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = view.read(cx);
        assert!(e.warp.is_some());
        assert_eq!(e.transform_control_mode, TransformControlMode::Resize);
        assert_eq!(e.transform_cursor(corner), CursorStyle::Arrow);
        assert_eq!(e.transform_cursor(exterior), CursorStyle::Arrow);
        assert_eq!(e.editor.doc, original);
        assert!(e.editor.history.is_empty());
    });
    press("escape", cx);
    cx.update(|_, cx| {
        assert!(view.read(cx).warp.is_none());
        assert_eq!(view.read(cx).editor.doc, original);
    });
}

#[gpui_kit::test]
fn rotation_controls_photo_paint_workspace_switch_resets_then_supports_same_controls(
    cx: &mut TestAppContext,
) {
    let original = artwork();
    let (view, cx) = setup(cx, original.clone());
    for draw_mode in [true, false] {
        let center = at(&view, cx, (256., 192.));
        click(center, cx);
        cx.update(|_, cx| {
            view.update(cx, |e, cx| {
                assert_eq!(e.transform_control_mode, TransformControlMode::Rotate);
                let mut layout = e.workspace_snapshot();
                layout.draw_mode = draw_mode;
                e.apply_workspace_layout(&layout, cx);
                assert_eq!(e.draw_mode, draw_mode);
                assert!(e.has_transform_controls());
                assert_eq!(e.transform_control_mode, TransformControlMode::Resize);
            });
        });
        cx.run_until_parked();
        let center = at(&view, cx, (256., 192.));
        click(center, cx);
        cx.update(|_, cx| {
            view.update(cx, |e, _| {
                assert_eq!(e.transform_control_mode, TransformControlMode::Rotate);
                let overlay = e.overlay(1.);
                assert!(overlay.transform.is_some());
                assert_eq!(overlay.transform_mode, Some(TransformControlMode::Rotate));
                let quad = e.transform_screen_quad().unwrap();
                for corner in quad {
                    assert_eq!(
                        e.handle_hit(screen(corner)),
                        Some(transform::Handle::Rotate)
                    );
                    assert_eq!(e.transform_cursor(screen(corner)), CursorStyle::Crosshair);
                }
                let (_, nub) =
                    transform_controls::rotation_handle(quad, e.canvas_bounds().unwrap()).unwrap();
                assert_eq!(e.handle_hit(screen(nub)), Some(transform::Handle::Rotate));
                assert_eq!(e.editor.doc, original);
                assert!(e.editor.history.is_empty());
            });
        });
        click(center, cx);
    }
}

#[gpui_kit::test]
fn rotation_controls_explicit_modal_mode_switch_keeps_preview_and_single_undo_baseline(
    cx: &mut TestAppContext,
) {
    let original = artwork();
    let (view, cx) = setup(cx, original.clone());
    cx.dispatch_action(crate::actions::TransformRotate);
    cx.run_until_parked();
    let rotated = cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            assert!(e.photo_transform_active());
            assert_eq!(e.editor.transaction_depth(), 1);
            e.rotate_transform_selection(30., cx);
            assert_ne!(e.editor.doc, original);
            e.editor.doc.clone()
        })
    });
    cx.dispatch_action(crate::actions::TransformScale);
    cx.run_until_parked();
    let (start, end) = cx.update(|_, cx| {
        let e = view.read(cx);
        assert!(e.photo_transform_active());
        assert_eq!(e.transform_control_mode, TransformControlMode::Resize);
        assert_eq!(e.editor.transaction_depth(), 1);
        assert_eq!(
            e.editor.doc, rotated,
            "switching does not restart or commit the preview"
        );
        assert!(e.editor.history.is_empty());
        assert!(!e.status.as_ref().is_some_and(|(_, error)| *error));
        let quad = e.transform_screen_quad().unwrap();
        (
            screen(quad[2]),
            screen((
                quad[0].0 + (quad[2].0 - quad[0].0) * 1.25,
                quad[0].1 + (quad[2].1 - quad[0].1) * 1.25,
            )),
        )
    });
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    cx.dispatch_action(crate::actions::TransformRotate);
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = view.read(cx);
        assert_eq!(e.transform_control_mode, TransformControlMode::Rotate);
        assert_eq!(e.editor.transaction_depth(), 1);
        assert!(e.editor.history.is_empty());
        assert_ne!(e.editor.doc, rotated);
        assert!(!e.status.as_ref().is_some_and(|(_, error)| *error));
    });
    press("enter", cx);
    let committed = cx.update(|_, cx| {
        let e = view.read(cx);
        assert!(!e.photo_transform_active());
        assert!(!e.editor.in_transaction());
        assert_eq!(e.editor.history.len(), 1);
        e.editor.doc.clone()
    });
    press("ctrl-z", cx);
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, original));
    press("ctrl-shift-z", cx);
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, committed));
}

#[test]
fn rotation_controls_nub_reserves_top_and_left_ruler_hit_targets() {
    let bounds = Bounds::new(point(px(0.), px(0.)), size(px(200.), px(160.)));
    let quad = [(60., 36.), (140., 36.), (140., 96.), (60., 96.)];
    let (midpoint, nub) = transform_controls::rotation_handle(quad, bounds).unwrap();
    assert_eq!(midpoint, (140., 66.));
    assert_eq!(
        nub,
        (164., 66.),
        "top nub at y=12 overlaps the ruler despite being inside canvas"
    );
    assert!(nub.1 >= super::snap::RULER_PX as f64 + 9.);

    // With a rotated frame, its first edge can face the left ruler instead.
    let bounds = Bounds::new(point(px(0.), px(0.)), size(px(160.), px(200.)));
    let quad = [(36., 140.), (36., 60.), (96., 60.), (96., 140.)];
    let (midpoint, nub) = transform_controls::rotation_handle(quad, bounds).unwrap();
    assert_eq!(midpoint, (66., 60.));
    assert_eq!(nub, (66., 36.), "left nub at x=12 must also fall back");
    assert!(nub.0 >= super::snap::RULER_PX as f64 + 9.);
}

#[gpui_kit::test]
fn rotation_controls_more_popup_escape_preserves_modal_until_canvas_escape(
    cx: &mut TestAppContext,
) {
    let original = artwork();
    let (view, cx) = setup(cx, original.clone());
    cx.simulate_resize(size(px(480.), px(900.)));
    cx.update(|window, cx| {
        cx.global_mut::<crate::app_state::AppSettings>()
            .0
            .compact_chrome = true;
        view.update(cx, |e, cx| {
            e.show_sidebar_tab(SidebarTab::Properties, cx);
            e.compact.bars[super::compact::Bar::Options as usize].scale = 2.;
            window.focus(&e.canvas_focus, cx);
        });
        window.refresh();
    });
    cx.run_until_parked();
    press("ctrl-t", cx);
    let preview = cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            assert!(e.photo_transform_active());
            assert_eq!(e.transform_control_mode, TransformControlMode::Resize);
            e.rotate_transform_selection(15., cx);
            assert_ne!(e.editor.doc, original);
            e.editor.doc.clone()
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(
            window
                .within("editor-tool-options")
                .try_find("transform-mode-rotate")
                .is_none()
        );
        window.click("tool-options-more", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(
            window
                .within("tool-options-overflow-content")
                .find("transform-mode-rotate")
                .visible()
        );
        window.click("transform-mode-rotate", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("tool-options-overflow-content").visible());
        assert_eq!(
            view.read(cx).transform_control_mode,
            TransformControlMode::Rotate
        );
        assert!(view.read(cx).photo_transform_active());
    });
    press("escape", cx);
    cx.update(|window, cx| {
        let e = view.read(cx);
        assert!(window.try_find("tool-options-overflow-content").is_none());
        assert!(
            e.photo_transform_active(),
            "first Escape only dismisses More"
        );
        assert_eq!(e.editor.doc, preview);
        assert_eq!(e.editor.transaction_depth(), 1);
        assert!(e.editor.history.is_empty());
    });
    // No manual refocus: dismissal must restore the canvas shortcut context.
    press("escape", cx);
    cx.update(|_, cx| {
        let e = view.read(cx);
        assert!(!e.photo_transform_active());
        assert!(!e.editor.in_transaction());
        assert_eq!(e.editor.doc, original);
        assert!(e.editor.history.is_empty());
        assert!(!e.editor.is_modified());
    });
}

#[gpui_kit::test]
fn rotation_controls_paint_brush_strokes_stay_painting_and_keep_undo(cx: &mut TestAppContext) {
    let original = artwork();
    let (view, cx) = setup_workspace(cx, CanvasWorkspace::Paint, original.clone());
    let center = at(&view, cx, (256., 192.));
    click(center, cx);
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            assert_eq!(e.transform_control_mode, TransformControlMode::Rotate);
            e.set_paint(PaintKind::Brush, cx);
            e.tools.brush = emulsion_raster::paint::Brush {
                size: 12.,
                hardness: 1.,
                ..Default::default()
            };
            e.tools.quick_shape = false;
            e.set_fg([0, 0, 255, 255], cx);
            assert_eq!(e.transform_control_mode, TransformControlMode::Resize);
        });
    });
    cx.run_until_parked();
    let end = at(&view, cx, (276., 192.));
    cx.simulate_mouse_down(center, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    let painted = cx.update(|window, cx| {
        let e = view.read(cx);
        assert_eq!(e.tool, Tool::Brush);
        assert_eq!(e.transform_control_mode, TransformControlMode::Resize);
        assert_eq!(placement(e), Placement::at(208., 160.));
        let NodeKind::Raster { raster, .. } = &e.editor.doc.nodes[0].kind else {
            panic!("raster");
        };
        assert_eq!(raster.get(55, 32), [0, 0, 65535, 65535]);
        assert_eq!(e.editor.history.len(), 1);
        assert!(!e.editor.in_transaction());
        assert!(window.try_find("transform-rotation-cursor").is_none());
        e.editor.doc.clone()
    });
    press("ctrl-z", cx);
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, original));
    press("ctrl-shift-z", cx);
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, painted));
}

#[gpui_kit::test]
fn rotation_controls_paint_and_design_explicit_rotate_keep_per_gesture_history_and_escape(
    cx: &mut TestAppContext,
) {
    for kind in [CanvasWorkspace::Paint, CanvasWorkspace::Design] {
        let original = artwork();
        let (view, cx) = setup_workspace(cx, kind, original.clone());
        cx.dispatch_action(crate::actions::TransformRotate);
        cx.run_until_parked();
        let (start, end) = cx.update(|_, cx| {
            let e = view.read(cx);
            assert_eq!(
                e.transform_control_mode,
                TransformControlMode::Rotate,
                "{kind:?}"
            );
            assert!(!e.photo_transform_active());
            assert!(!e.editor.in_transaction());
            assert!(e.editor.history.is_empty());
            let quad = e.transform_screen_quad().unwrap();
            let center = ((quad[0].0 + quad[2].0) / 2., (quad[0].1 + quad[2].1) / 2.);
            (
                screen(quad[0]),
                screen((
                    center.0 - (quad[0].1 - center.1),
                    center.1 + quad[0].0 - center.0,
                )),
            )
        });
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        cx.update(|_, cx| {
            let e = view.read(cx);
            assert!(matches!(e.drag, Some(Drag::Transform(grab)) if grab.handle == transform::Handle::Rotate));
            assert_eq!(e.editor.transaction_depth(), 1);
        });
        cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
        cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        let committed = cx.update(|_, cx| {
            let e = view.read(cx);
            let p = placement(e);
            assert!((p.rotation - 90.).abs() < 0.01);
            assert!((p.scale_x - 1.).abs() < 0.001);
            assert!((p.scale_y - 1.).abs() < 0.001);
            assert!(!e.photo_transform_active());
            assert!(
                !e.editor.in_transaction(),
                "non-Photo still commits at pointer release"
            );
            assert_eq!(e.editor.history.len(), 1);
            e.editor.doc.clone()
        });
        press("ctrl-z", cx);
        cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, original));
        press("ctrl-shift-z", cx);
        cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, committed));
        cx.dispatch_action(crate::actions::TransformRotate);
        cx.run_until_parked();
        let (start, end) = cx.update(|_, cx| {
            let quad = view.read(cx).transform_screen_quad().unwrap();
            (screen(quad[0]), screen(quad[1]))
        });
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
        cx.update(|_, cx| assert_ne!(view.read(cx).editor.doc, committed));
        press("escape", cx);
        cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = view.read(cx);
            assert_eq!(e.editor.doc, committed);
            assert_eq!(e.editor.history.len(), 1);
            assert!(!e.editor.in_transaction());
            assert!(e.drag.is_none());
        });
        cx.dispatch_action(crate::actions::TransformScale);
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = view.read(cx);
            assert_eq!(e.transform_control_mode, TransformControlMode::Resize);
            assert!(!e.photo_transform_active());
            let corner = screen(e.transform_screen_quad().unwrap()[0]);
            assert_eq!(e.handle_hit(corner), Some(transform::Handle::Corner(0)));
            assert_eq!(e.editor.doc, committed);
            assert_eq!(e.editor.history.len(), 1);
        });
    }
}
