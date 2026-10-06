//! Shared Move controls must preserve Design picking and other workspaces' gestures.
use super::*;
use crate::tests::open;
use ::core::prelude::v1::test;
use emulsion_core::project::{ProjectEditor, ProjectKind};
use emulsion_raster::{vector::PathStyle, vector_geometry::rectangle};
use gpui_kit::test::TestWindowExt;

fn rectangle_node(doc: &mut Document, rect: [f64; 4]) -> NodeId {
    Command::AddNode {
        node: Box::new(Node::path(
            0,
            "Artwork",
            Arc::new(rectangle(rect[0], rect[1], rect[2], rect[3])),
            PathStyle {
                fill: Some([60, 130, 220, 255]),
                stroke: None,
                ..Default::default()
            },
            doc.width,
            doc.height,
        )),
        slot: Slot::TOP,
    }
    .apply(doc)
    .unwrap()
    .unwrap()
}

fn setup(
    cx: &mut TestAppContext,
    kind: ProjectKind,
    doc: Document,
) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (workspace, cx) = open(cx, Document::new(600, 400));
    cx.simulate_resize(size(px(1440.), px(1000.)));
    let view = cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.install_project(
                ProjectEditor::new_project(kind, doc).unwrap(),
                "Transform controls".into(),
                window,
                cx,
            )
        });
        workspace.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        view.update(cx, |e, cx| {
            e.set_layer_selection(Vec::new(), None);
            e.set_tool(Tool::Move, cx);
            e.snap = false;
            e.view.zoom = 1.;
            e.view.center = (300., 200.);
            e.fit_pending = false;
            window.focus(&e.canvas_focus, cx);
            cx.notify();
        })
    });
    cx.run_until_parked();
    (view, cx)
}

fn at(view: &Entity<EditorView>, cx: &mut VisualTestContext, p: (f64, f64)) -> Point<Pixels> {
    cx.update(|_, cx| view.read(cx).doc_to_window(p).unwrap())
}

fn click(position: Point<Pixels>, modifiers: Modifiers, cx: &mut VisualTestContext) {
    cx.simulate_mouse_down(position, MouseButton::Left, modifiers);
    cx.simulate_mouse_up(position, MouseButton::Left, modifiers);
    cx.run_until_parked();
}

fn select(view: &Entity<EditorView>, ids: &[NodeId], cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.set_layer_selection(ids.to_vec(), ids.last().copied());
            cx.notify();
        })
    });
    cx.run_until_parked();
}

#[gpui_kit::test]
fn rotation_controls_design_first_selection_does_not_toggle(cx: &mut TestAppContext) {
    let mut original = Document::new(600, 400);
    let first = rectangle_node(&mut original, [100., 120., 120., 100.]);
    let second = rectangle_node(&mut original, [340., 120., 120., 100.]);
    let (view, cx) = setup(cx, ProjectKind::Design, original.clone());
    let stamp = cx.update(|_, cx| view.read(cx).editor.stamp());
    for (id, position) in [(first, (160., 170.)), (second, (400., 170.))] {
        let position = at(&view, cx, position);
        cx.simulate_mouse_down(position, MouseButton::Left, Modifiers::none());
        cx.update(|_, cx| {
            let e = view.read(cx);
            assert!(e.has_transform_controls());
            assert_eq!(e.selected_layer_ids(), vec![id]);
            assert_eq!(e.transform_control_mode, TransformControlMode::Resize);
        });
        cx.simulate_mouse_up(position, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = view.read(cx);
            assert_eq!(e.transform_control_mode, TransformControlMode::Resize);
            assert_eq!(e.editor.doc, original);
            assert_eq!(e.editor.stamp(), stamp);
            assert!(e.editor.history.is_empty());
            assert!(!e.editor.in_transaction());
            assert!(e.drag.is_none());
        });
        // The next object's first pick must reset the previous object's mode.
        click(position, Modifiers::none(), cx);
        cx.update(|_, cx| {
            assert_eq!(
                view.read(cx).transform_control_mode,
                TransformControlMode::Rotate
            )
        });
    }
}

#[gpui_kit::test]
fn rotation_controls_design_shift_and_alt_keep_selection_semantics(cx: &mut TestAppContext) {
    let mut original = Document::new(600, 400);
    let first = rectangle_node(&mut original, [100., 120., 100., 100.]);
    let second = rectangle_node(&mut original, [340., 120., 100., 100.]);
    let group = Command::Group {
        ids: vec![first],
        name: "Editable group".into(),
    }
    .apply(&mut original)
    .unwrap()
    .unwrap();
    let (view, cx) = setup(cx, ProjectKind::Design, original.clone());
    select(&view, &[group], cx);
    let first_point = at(&view, cx, (150., 170.));
    let second_point = at(&view, cx, (390., 170.));
    let shift = Modifiers {
        shift: true,
        ..Modifiers::none()
    };
    let alt = Modifiers {
        alt: true,
        ..Modifiers::none()
    };
    let stamp = cx.update(|_, cx| view.read(cx).editor.stamp());

    click(second_point, shift, cx);
    cx.update(|_, cx| {
        let e = view.read(cx);
        assert_eq!(e.selected_layer_ids().len(), 2);
        assert!(e.layer_is_selected(group) && e.layer_is_selected(second));
        assert_eq!(e.transform_control_mode, TransformControlMode::Resize);
    });
    click(second_point, shift, cx);
    cx.update(|_, cx| {
        let e = view.read(cx);
        assert_eq!(e.selected_layer_ids(), vec![group]);
        assert_eq!(e.transform_control_mode, TransformControlMode::Resize);
    });
    click(first_point, alt, cx);
    cx.update(|_, cx| {
        let e = view.read(cx);
        assert_eq!(
            e.selected_layer_ids(),
            vec![first],
            "Alt picks inside the group"
        );
        assert_eq!(e.transform_control_mode, TransformControlMode::Resize);
    });
    // Alt on an already selected child also must not become a mode toggle.
    click(first_point, alt, cx);
    cx.update(|_, cx| {
        let e = view.read(cx);
        assert_eq!(e.selected_layer_ids(), vec![first]);
        assert_eq!(e.transform_control_mode, TransformControlMode::Resize);
        assert_eq!(e.editor.doc, original);
        assert_eq!(e.editor.stamp(), stamp);
        assert!(e.editor.history.is_empty());
        assert!(!e.editor.in_transaction());
        assert!(e.drag.is_none());
    });
}

#[gpui_kit::test]
fn rotation_controls_design_group_and_multi_clicks_preserve_targets(cx: &mut TestAppContext) {
    for grouped in [false, true] {
        let mut original = Document::new(600, 400);
        let first = rectangle_node(&mut original, [100., 120., 100., 100.]);
        let second = rectangle_node(&mut original, [340., 120., 100., 100.]);
        let targets = if grouped {
            vec![
                Command::Group {
                    ids: vec![first, second],
                    name: "Group".into(),
                }
                .apply(&mut original)
                .unwrap()
                .unwrap(),
            ]
        } else {
            vec![first, second]
        };
        let (view, cx) = setup(cx, ProjectKind::Design, original.clone());
        select(&view, &targets, cx);
        let point = at(&view, cx, (150., 170.));
        let stamp = cx.update(|_, cx| view.read(cx).editor.stamp());
        for expected in [TransformControlMode::Rotate, TransformControlMode::Resize] {
            click(point, Modifiers::none(), cx);
            cx.update(|_, cx| {
                let e = view.read(cx);
                assert_eq!(e.transform_control_mode, expected, "grouped={grouped}");
                assert_eq!(e.selected_layer_ids(), targets);
                assert_eq!(e.editor.doc, original);
                assert_eq!(e.editor.stamp(), stamp);
                assert!(e.editor.history.is_empty());
                assert!(!e.editor.in_transaction());
                assert!(e.drag.is_none());
            });
        }
    }
}

#[gpui_kit::test]
fn rotation_controls_design_locked_targets_never_start_a_transform(cx: &mut TestAppContext) {
    for target_kind in ["single", "group", "multi"] {
        for position_lock in [false, true] {
            let mut original = Document::new(600, 400);
            let first = rectangle_node(&mut original, [100., 120., 100., 100.]);
            let second = rectangle_node(&mut original, [340., 120., 100., 100.]);
            let targets = match target_kind {
                "group" => vec![
                    Command::Group {
                        ids: vec![first, second],
                        name: "Locked member".into(),
                    }
                    .apply(&mut original)
                    .unwrap()
                    .unwrap(),
                ],
                "multi" => vec![first, second],
                _ => vec![first],
            };
            let locked = original.node_mut(first).unwrap();
            if position_lock {
                locked.locks.position = true;
            } else {
                locked.locked = true;
            }
            let (view, cx) = setup(cx, ProjectKind::Design, original.clone());
            select(&view, &targets, cx);
            let stamp = cx.update(|_, cx| {
                view.update(cx, |e, cx| {
                    e.transform_control_mode = TransformControlMode::Rotate;
                    assert!(e.transformable().is_none());
                    cx.notify();
                    e.editor.stamp()
                })
            });
            cx.run_until_parked();
            // Hit an unlocked member of collective targets, so their locks must
            // be checked across the whole selection rather than only the hit node.
            let point = at(
                &view,
                cx,
                if target_kind == "single" {
                    (150., 170.)
                } else {
                    (390., 170.)
                },
            );
            cx.simulate_mouse_down(point, MouseButton::Left, Modifiers::none());
            cx.update(|_, cx| {
                let e = view.read(cx);
                assert!(e.drag.is_none(), "{target_kind}, position={position_lock}");
                assert!(!e.editor.in_transaction());
            });
            let end = point + point_offset(32., 17.);
            cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
            cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
            cx.run_until_parked();
            cx.update(|_, cx| {
                let e = view.read(cx);
                assert_eq!(e.editor.doc, original);
                assert_eq!(e.editor.stamp(), stamp);
                assert!(e.editor.history.is_empty());
                assert!(!e.editor.in_transaction());
                assert!(e.drag.is_none());
            });
        }
    }
}

fn point_offset(x: f32, y: f32) -> Point<Pixels> {
    point(px(x), px(y))
}

#[gpui_kit::test]
fn rotation_controls_design_page_switch_resets_mode_without_editing(cx: &mut TestAppContext) {
    let mut original = Document::new(600, 400);
    let id = rectangle_node(&mut original, [100., 120., 120., 100.]);
    let (view, cx) = setup(cx, ProjectKind::Design, original.clone());
    let (first, second, stamp) = cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            let first = e.editor.active_page();
            let second = e.editor.duplicate_page(first).unwrap();
            e.after_change(cx);
            e.set_layer_selection(vec![id], Some(id));
            e.transform_control_mode = TransformControlMode::Rotate;
            (first, second, e.editor.stamp())
        })
    });
    cx.run_until_parked();
    for destination in [first, second] {
        cx.update(|window, cx| window.click(("project-page", destination), cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |e, cx| {
                assert_eq!(e.editor.active_page(), destination);
                assert_eq!(e.transform_control_mode, TransformControlMode::Resize);
                assert_eq!(e.editor.doc, original);
                assert_eq!(e.editor.stamp(), stamp);
                assert!(!e.editor.in_transaction());
                assert!(e.drag.is_none());
                e.set_layer_selection(vec![id], Some(id));
                e.transform_control_mode = TransformControlMode::Rotate;
                cx.notify();
            })
        });
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn rotation_controls_design_double_click_still_opens_image_frame_crop(cx: &mut TestAppContext) {
    use emulsion_core::design::{Element, frame, frame_parts, place_in_frame};
    let mut editor = emulsion_core::Editor::new(Document::new(600, 400), None);
    let group = frame(&editor.doc, Element::Rectangle)
        .paste(&mut editor, Slot::TOP, (0., 0.))
        .unwrap()[0];
    place_in_frame(
        &mut editor,
        group,
        Arc::new(Raster::solid(400, 100, [0.2, 0.3, 0.4, 1.])),
    )
    .unwrap();
    let original = editor.doc;
    let boundary = frame_parts(&original, group).unwrap().0;
    let bounds = emulsion_core::geometry::node_bounds(&original, boundary)
        .unwrap()
        .unwrap();
    let center = (
        bounds.x as f64 + bounds.w as f64 / 2.,
        bounds.y as f64 + bounds.h as f64 / 2.,
    );
    let (view, cx) = setup(cx, ProjectKind::Design, original.clone());
    for mode in [TransformControlMode::Resize, TransformControlMode::Rotate] {
        select(&view, &[group], cx);
        let stamp = cx.update(|_, cx| {
            view.update(cx, |e, cx| {
                e.transform_control_mode = mode;
                cx.notify();
                e.editor.stamp()
            })
        });
        cx.run_until_parked();
        let center = at(&view, cx, center);
        cx.simulate_event(MouseDownEvent {
            position: center,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 2,
            first_mouse: false,
        });
        cx.simulate_mouse_up(center, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        cx.update(|window, cx| {
            let e = view.read(cx);
            assert!(e.frame_crop_active(), "mode={mode:?}");
            assert!(window.find("design-frame-crop-editor").visible());
            assert_eq!(e.editor.doc, original);
            assert_eq!(e.editor.stamp(), stamp);
            assert!(e.drag.is_none());
            assert!(!e.editor.in_transaction());
        });
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = view.read(cx);
            assert!(!e.frame_crop_active());
            assert_eq!(e.editor.doc, original);
            assert_eq!(e.editor.stamp(), stamp);
            assert!(e.editor.history.is_empty());
        });
    }
}

#[gpui_kit::test]
fn rotation_controls_diagram_and_storyboard_ignore_stale_mode_and_have_no_round_nub(
    cx: &mut TestAppContext,
) {
    for kind in [ProjectKind::Diagram, ProjectKind::Storyboard] {
        let (original, id) = if kind == ProjectKind::Diagram {
            let mut builder = emulsion_core::diagram::Builder::new(600, 400).unwrap();
            let id = builder
                .add_shape(
                    emulsion_core::diagram::ShapeKind::Process,
                    [180., 130., 200., 120.],
                    "Artwork",
                )
                .unwrap();
            (builder.finish().unwrap(), id)
        } else {
            let mut doc = Document::new(600, 400);
            let id = rectangle_node(&mut doc, [180., 130., 200., 120.]);
            (doc, id)
        };
        let (view, cx) = setup(cx, kind, original.clone());
        select(&view, &[id], cx);
        let stamp = cx.update(|_, cx| view.read(cx).editor.stamp());
        for mode in [TransformControlMode::Resize, TransformControlMode::Rotate] {
            cx.update(|window, cx| {
                view.update(cx, |e, cx| {
                    e.transform_control_mode = mode;
                    assert!(!e.has_transform_controls(), "{kind:?}");
                    let quad = e.transform_screen_quad().unwrap();
                    for (index, (x, y)) in quad.into_iter().enumerate() {
                        assert_eq!(
                            e.handle_hit(point(px(x as f32), px(y as f32))),
                            Some(transform::Handle::Corner(index)),
                            "{kind:?}, {mode:?}"
                        );
                    }
                    let (_, nub) =
                        transform_controls::rotation_handle(quad, e.canvas_bounds().unwrap())
                            .expect("room for a hypothetical Photo/Design rotation nub");
                    let nub = point(px(nub.0 as f32), px(nub.1 as f32));
                    assert_eq!(e.handle_hit(nub), None, "{kind:?}");
                    assert_eq!(e.transform_cursor(nub), CursorStyle::Arrow);
                    let overlay = e.overlay(1.);
                    assert!(overlay.transform.is_some());
                    assert_eq!(overlay.transform_mode, None, "{kind:?}");
                    cx.notify();
                });
                assert!(window.try_find("transform-mode-resize").is_none());
                assert!(window.try_find("transform-mode-rotate").is_none());
            });
            cx.run_until_parked();
            let center = at(&view, cx, (280., 190.));
            for _ in 0..2 {
                click(center, Modifiers::none(), cx);
                cx.update(|_, cx| {
                    let e = view.read(cx);
                    assert_eq!(e.transform_control_mode, mode, "{kind:?}");
                    assert_eq!(e.selected_layer_ids(), vec![id]);
                    assert_eq!(e.editor.doc, original);
                    assert_eq!(e.editor.stamp(), stamp);
                    assert!(e.editor.history.is_empty());
                    assert!(!e.editor.in_transaction());
                    assert!(e.drag.is_none());
                });
            }
        }
    }
}

#[gpui_kit::test]
fn rotation_controls_design_all_tools_keep_modes_in_direct_row_and_nub_clear_of_toolbar(
    cx: &mut TestAppContext,
) {
    let mut original = Document::new(600, 400);
    let id = rectangle_node(&mut original, [200., 130., 200., 140.]);
    let (view, cx) = setup(cx, ProjectKind::Design, original.clone());
    select(&view, &[id], cx);
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.show_design_section(design_ui::Section::Tools, cx);
        })
    });
    cx.run_until_parked();
    let stamp = cx.update(|_, cx| view.read(cx).editor.stamp());
    for width in [1440., 600.] {
        cx.simulate_resize(size(px(width), px(1000.)));
        cx.run_until_parked();
        for rotation in [0., 37., 90.] {
            cx.update(|_, cx| {
                view.update(cx, |e, cx| {
                    e.view.center = (300., 200.);
                    e.view.zoom = 1.;
                    e.view.rotation = rotation;
                    e.fit_pending = false;
                    cx.notify();
                })
            });
            cx.run_until_parked();
            cx.update(|window, cx| {
                window.scroll(
                    "design-direct-controls-scroll",
                    ScrollDelta::Pixels(point(px(-10000.), px(0.))),
                    cx,
                );
            });
            cx.run_until_parked();
            for (button, expected) in [
                ("transform-mode-rotate", TransformControlMode::Rotate),
                ("transform-mode-resize", TransformControlMode::Resize),
            ] {
                cx.update(|window, cx| {
                    let row = window.find("design-direct-controls-scroll").bounds();
                    for id in ["transform-mode-resize", "transform-mode-rotate"] {
                        let control = window.find(id);
                        let bounds = control.bounds();
                        assert!(control.visible(), "{width}, {rotation}, {id}");
                        assert!(
                            bounds.left() >= row.left()
                                && bounds.right() <= row.right()
                                && bounds.top() >= row.top()
                                && bounds.bottom() <= row.bottom(),
                            "{width}, {rotation}, {id}: {bounds:?} outside {row:?}"
                        );
                    }
                    window.click(button, cx);
                });
                cx.run_until_parked();
                cx.update(|window, cx| {
                    let toolbar = window.find("design-selection-toolbar").bounds();
                    view.update(cx, |e, _| {
                        assert!(e.design_full_tools());
                        assert_eq!(e.transform_control_mode, expected);
                        let quad = e.transform_screen_quad().unwrap();
                        let bounds = e.canvas_bounds().unwrap();
                        let (_, nub) = e.rotation_handle_for_frame(quad, bounds)
                            .expect("All tools leaves room for a side rotation handle");
                        assert_eq!(
                            e.rotation_handle_for_frame(quad, bounds),
                            transform_controls::rotation_handle_with_obstacle(
                                quad, bounds, true, Some(toolbar),
                            )
                        );
                        let position = point(px(nub.0 as f32), px(nub.1 as f32));
                        assert_eq!(e.handle_hit(position), Some(transform::Handle::Rotate));
                        assert_eq!(e.transform_cursor(position), CursorStyle::Crosshair);
                        // Include the complete 9-pixel hit radius, not just the
                        // visible 5-pixel round handle, when checking chrome.
                        let target = Bounds::new(position - point(px(9.), px(9.)), size(px(18.), px(18.)));
                        assert!(
                            target.right() <= toolbar.left() || target.left() >= toolbar.right()
                                || target.bottom() <= toolbar.top() || target.top() >= toolbar.bottom(),
                            "{width}, {rotation}: rotation target {target:?} overlaps toolbar {toolbar:?}"
                        );
                        let overlay = e.overlay(1.);
                        assert_eq!(overlay.transform_mode, Some(expected));
                        assert!(overlay.transform_side_handle);
                        assert_eq!(overlay.transform_obstacle, Some(toolbar));
                        assert_eq!(e.editor.doc, original);
                        assert_eq!(e.editor.stamp(), stamp);
                        assert!(e.editor.history.is_empty());
                        assert!(!e.editor.in_transaction());
                    });
                });
            }
        }
    }
}

#[test]
fn rotation_controls_design_side_geometry_handles_rotation_and_clipped_edges() {
    let bounds = Bounds::new(point(px(0.), px(0.)), size(px(400.), px(300.)));
    let upright = [(100., 100.), (260., 100.), (260., 200.), (100., 200.)];
    assert_eq!(
        transform_controls::side_rotation_handle(upright, bounds),
        Some(((260., 150.), (284., 150.)))
    );
    let clipped = [(220., 100.), (390., 100.), (390., 200.), (220., 200.)];
    assert_eq!(
        transform_controls::side_rotation_handle(clipped, bounds),
        Some(((220., 150.), (196., 150.))),
        "A right-clipped target uses the left edge, never the toolbar strip"
    );
    for degrees in [15_f64, 37., 90., 135., 179.] {
        let angle = degrees.to_radians();
        let (sin, cos) = angle.sin_cos();
        let quad = upright.map(|(x, y)| {
            let (x, y) = (x - 180., y - 150.);
            (180. + x * cos - y * sin, 150. + x * sin + y * cos)
        });
        let (mid, nub) = transform_controls::side_rotation_handle(quad, bounds)
            .expect("Rotated artwork has an in-bounds handle clear of both toolbar strips");
        let top = quad.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
        let bottom = quad.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
        assert!(
            nub.1 - 9. >= top && nub.1 + 9. <= bottom,
            "{degrees}: {nub:?}"
        );
        assert!(((nub.0 - mid.0).hypot(nub.1 - mid.1) - 24.).abs() < 1e-8);
        assert!(bounds.contains(&point(px(nub.0 as f32 - 9.), px(nub.1 as f32 - 9.))));
        assert!(bounds.contains(&point(px(nub.0 as f32 + 9.), px(nub.1 as f32 + 9.))));
    }
    let too_short = [(100., 100.), (260., 100.), (260., 110.), (100., 110.)];
    assert_eq!(
        transform_controls::side_rotation_handle(too_short, bounds),
        None,
        "No nub is preferable to putting its hit area under the floating toolbar"
    );
}

#[test]
fn rotation_controls_design_oversized_artwork_avoids_bottom_clamped_toolbar() {
    let canvas = Bounds::new(point(px(0.), px(0.)), size(px(500.), px(700.)));
    let quad = [(100., 20.), (200., 20.), (200., 1320.), (100., 1320.)];
    let toolbar = Bounds::new(point(px(12.), px(662.)), size(px(276.), px(32.)));
    let legacy = transform_controls::side_rotation_handle(quad, canvas).unwrap();
    assert_eq!(legacy, ((200., 670.), (224., 670.)));
    assert!(toolbar.contains(&point(px(legacy.1.0 as f32), px(legacy.1.1 as f32))));
    assert_eq!(
        transform_controls::rotation_handle_with_obstacle(quad, canvas, true, Some(toolbar)),
        None,
        "Both visible side targets are underneath the bottom-clamped floating toolbar"
    );
}

#[gpui_kit::test]
fn rotation_controls_design_rendered_toolbar_blocks_oversized_artwork_nub(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx, ProjectKind::Design, Document::new(600, 400));
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.show_design_section(design_ui::Section::Tools, cx)
        })
    });
    cx.run_until_parked();
    let (original, stamp) = cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            let bounds = e.canvas_bounds().unwrap();
            let width = f64::from(f32::from(bounds.size.width));
            let height = f64::from(f32::from(bounds.size.height));
            let id = e
                .execute(
                    Command::AddNode {
                        node: Box::new(Node::path(
                            0,
                            "Oversized artwork",
                            Arc::new(rectangle(100., 20., 100., 2. * (height - 50.))),
                            PathStyle {
                                fill: Some([60, 130, 220, 255]),
                                stroke: None,
                                ..Default::default()
                            },
                            e.editor.doc.width,
                            e.editor.doc.height,
                        )),
                        slot: Slot::TOP,
                    },
                    cx,
                )
                .unwrap();
            e.set_layer_selection(vec![id], Some(id));
            e.view.center = (width / 2., height / 2.);
            e.view.zoom = 1.;
            e.view.rotation = 0.;
            e.fit_pending = false;
            cx.notify();
            (e.editor.doc.clone(), e.editor.stamp())
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let toolbar = window.find("design-selection-toolbar").bounds();
        view.update(cx, |e, _| {
            let bounds = e.canvas_bounds().unwrap();
            let quad = e.transform_screen_quad().unwrap();
            assert!((quad[0].1 - f64::from(f32::from(bounds.top())) - 20.).abs() < 1e-6);
            let (_, legacy) = transform_controls::side_rotation_handle(quad, bounds).unwrap();
            let legacy = point(px(legacy.0 as f32), px(legacy.1 as f32));
            assert!(
                toolbar.contains(&legacy),
                "legacy target {legacy:?}, toolbar {toolbar:?}"
            );
            assert_eq!(e.rotation_control_obstacle(), Some(toolbar));
            assert_eq!(e.transform_toolbar_bounds.get(), Some(toolbar));
            assert_eq!(
                e.handle_hit(legacy),
                None,
                "The obscured round nub is no longer interactive"
            );
            if let Some((_, nub)) = e.rotation_handle_for_frame(quad, bounds) {
                let target = Bounds::new(
                    point(px(nub.0 as f32 - 9.), px(nub.1 as f32 - 9.)),
                    size(px(18.), px(18.)),
                );
                assert!(
                    target.right() <= toolbar.left()
                        || target.left() >= toolbar.right()
                        || target.bottom() <= toolbar.top()
                        || target.top() >= toolbar.bottom(),
                    "replacement target {target:?}, toolbar {toolbar:?}"
                );
            }
            let overlay = e.overlay(1.);
            assert_eq!(overlay.transform_obstacle, Some(toolbar));
            assert!(overlay.transform_side_handle);
            assert_eq!(e.editor.doc, original);
            assert_eq!(e.editor.stamp(), stamp);
            assert!(!e.editor.in_transaction());
            assert!(e.drag.is_none());
        });
    });
}

#[gpui_kit::test]
fn rotation_controls_design_keep_narrow_font_controls_visible_and_clickable(
    cx: &mut TestAppContext,
) {
    let mut original = Document::new(600, 400);
    let id = Command::AddNode {
        node: Box::new(Node::text(
            0,
            "Heading",
            emulsion_core::text::TextSpec {
                text: "Selected heading".into(),
                x: 100.,
                y: 100.,
                size: 32.,
                ..Default::default()
            },
            600,
            400,
        )),
        slot: Slot::TOP,
    }
    .apply(&mut original)
    .unwrap()
    .unwrap();
    let (view, cx) = setup(cx, ProjectKind::Design, original.clone());
    select(&view, &[id], cx);
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.show_design_section(design_ui::Section::Templates, cx)
        })
    });
    cx.run_until_parked();
    let stamp = cx.update(|_, cx| view.read(cx).editor.stamp());
    for width in [480., 600., 899., 900.] {
        cx.simulate_resize(size(px(width), px(700.)));
        cx.run_until_parked();
        cx.update(|window, cx| {
            let scroll = window.find("design-direct-controls-scroll").bounds();
            let toolbar = window.find("design-direct-controls").bounds();
            let drawer = window.find("design-drawer").bounds();
            assert!(drawer.top() >= toolbar.bottom());
            for control in [
                "design-selection-target",
                "design-text-font",
                "design-text-size",
            ] {
                let bounds = window.find(control).bounds();
                assert!(
                    bounds.left() >= scroll.left() && bounds.right() <= scroll.right(),
                    "{width} {control}: {bounds:?} outside {scroll:?}"
                );
            }
            // Transform modes remain in the same scroll row after the existing
            // text controls; adding them must not displace font and size inputs.
            let size_bounds = window.find("design-text-size").bounds();
            let mode_bounds = window.find("transform-mode-resize").bounds();
            assert!(mode_bounds.left() >= size_bounds.right());
            window.click("design-text-font", cx);
        });
        cx.run_until_parked();
        cx.update(|window, _| assert!(window.find("font-picker").visible()));
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = view.read(cx);
            assert_eq!(e.editor.doc, original);
            assert_eq!(e.editor.stamp(), stamp);
            assert!(e.editor.history.is_empty());
            assert!(!e.editor.in_transaction());
            assert_eq!(e.transform_control_mode, TransformControlMode::Resize);
        });
    }
}
