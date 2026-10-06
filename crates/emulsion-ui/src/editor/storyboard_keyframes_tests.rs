//! Layer keyframe workflows through the real editor: Set key, auto-key
//! through the Move and Transform drags, the Stage's animated picture,
//! timeline key drags and Delete, the ease curve editor, motion path and
//! pivot handles, effect keys and keyframe sync. Each edit is one Undo step;
//! locked panels refuse.
use super::*;
use crate::tests::open;
use crate::workspace::Workspace;
use core::prelude::v1::test;
use emulsion_core::project::{ProjectEditor, ProjectKind};
use emulsion_core::storyboard::Panel;
use emulsion_core::timeline::FrameRate;
use emulsion_raster::Placement;
use gpui_kit::test::TestWindowExt;

const KEYS: &str = "keyframe";

fn storyboard() -> ProjectEditor {
    let mut p = ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(64, 36)).unwrap();
    let blank = p.storyboard().unwrap().blank_panel().unwrap();
    p.insert_panels(
        Some(1),
        &blank,
        vec![("Panel 2".to_string(), Panel::new(0, 24))],
        None,
    )
    .unwrap();
    let ids: Vec<_> = p.page_list().iter().map(|m| m.id).collect();
    p.edit_storyboard(|b| {
        b.settings.frame_rate = FrameRate::whole(24);
        for id in &ids {
            b.panels.get_mut(id).unwrap().frames = 24;
        }
        Ok(())
    })
    .unwrap();
    p
}

fn setup(
    cx: &mut TestAppContext,
) -> (
    Entity<Workspace>,
    Entity<EditorView>,
    NodeId,
    &mut VisualTestContext,
) {
    let (ws, cx) = open(cx, Document::new(64, 36));
    cx.simulate_resize(gpui_kit::size(px(1600.), px(3000.)));
    let editor = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(storyboard(), "Board".into(), window, cx)
        });
        ws.read(cx).editor.clone().unwrap()
    });
    let id = cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            let id = e
                .execute(
                    Command::AddNode {
                        node: Box::new(Node::raster(
                            0,
                            "Hero",
                            Arc::new(Raster::solid(16, 16, [1., 0., 0., 1.])),
                            Placement::at(8., 8.),
                        )),
                        slot: Slot::TOP,
                    },
                    cx,
                )
                .unwrap();
            e.set_layer_selection(vec![id], Some(id));
            e.tool = Tool::Move;
            // Six frames into the active (second) panel.
            e.transport.frame = 30;
            cx.notify();
            id
        })
    });
    settle(cx);
    (ws, editor, id, cx)
}

fn settle(cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.run_until_parked();
        cx.update(|window, cx| window.render_frame(cx));
    }
    cx.run_until_parked();
}

fn motion(e: &Entity<EditorView>, cx: &mut VisualTestContext) -> Motion {
    cx.update(|_, cx| {
        let e = e.read(cx);
        let board = e.editor.storyboard().unwrap();
        board.panels[&e.editor.active_page()].motion.clone()
    })
}

fn value(m: &Motion, id: NodeId, p: LayerProperty, frame: f64) -> f64 {
    m.get(&id).map_or(p.initial(), |l| l.value(&p, frame))
}

fn undo(e: &Entity<EditorView>, cx: &mut VisualTestContext) {
    cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
    settle(cx);
}

fn lock(e: &Entity<EditorView>, on: bool, cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            let panel = e.editor.active_page();
            assert!(e.edit_board(
                |b| {
                    b.panels.get_mut(&panel).unwrap().locked = on;
                    Ok(())
                },
                cx,
            ));
        })
    });
}

fn placement(e: &Entity<EditorView>, id: NodeId, cx: &mut VisualTestContext) -> Placement {
    cx.update(
        |_, cx| match &e.read(cx).editor.doc.node(id).unwrap().kind {
            NodeKind::Raster { placement, .. } => *placement,
            _ => panic!("raster"),
        },
    )
}

#[test]
fn keyframe_motion_values_decompose_back_into_the_matrix() {
    let layer = |values: &[(LayerProperty, f64)]| {
        let mut l = LayerMotion::default();
        for (p, v) in values {
            put_key(&mut l, p, 0, *v);
        }
        l
    };
    let pivot = dvec2(30., 20.);
    let source = layer(&[
        (LayerProperty::X, 12.),
        (LayerProperty::Y, -4.),
        (LayerProperty::Rotation, 370.),
        (LayerProperty::ScaleX, 1.5),
        (LayerProperty::ScaleY, 0.75),
        (LayerProperty::SkewX, 10.),
        (LayerProperty::SkewY, 5.),
    ]);
    let m = source.transform(0., pivot);
    let values = decompose(m, pivot, 5., 360.);
    let mut back = layer(&values);
    put_key(&mut back, &LayerProperty::SkewY, 0, 5.);
    for (p, v) in &values {
        assert!((source.value(p, 0.) - v).abs() < 1e-6, "{p:?}: {v}");
    }
    assert!(back.transform(0., pivot).abs_diff_eq(m, 1e-9));
    // Keys stay in frame order, one per frame; moves replace.
    let mut l = layer(&[(LayerProperty::X, 1.)]);
    put_key(&mut l, &LayerProperty::X, 10, 2.);
    put_key(&mut l, &LayerProperty::X, 5, 3.);
    assert_eq!(key_frames(&l, None), [0, 5, 10]);
    move_keys(&mut l, Some(&LayerProperty::X), 10, 5);
    assert_eq!(key_frames(&l, None), [0, 5]);
    assert_eq!(l.value(&LayerProperty::X, 5.), 2.);
    delete_keys(&mut l, None, 0);
    delete_keys(&mut l, None, 5);
    assert!(l.tracks.is_empty());
}

#[gpui_kit::test]
fn keyframe_set_key_records_the_pose_at_the_playhead_in_one_step(cx: &mut TestAppContext) {
    let (_ws, e, id, cx) = setup(cx);
    let _ = KEYS;
    cx.update(|window, cx| {
        assert!(window.find("stage-set-key").visible());
        window.click("stage-set-key", cx);
    });
    settle(cx);
    let m = motion(&e, cx);
    assert_eq!(m[&id].tracks.len(), LayerProperty::TRANSFORMS.len());
    assert!(
        m[&id]
            .tracks
            .iter()
            .all(|t| t.keys.len() == 1 && t.keys[0].frame == 6)
    );
    assert_eq!(value(&m, id, LayerProperty::Opacity, 6.), 1.);
    undo(&e, cx);
    assert!(motion(&e, cx).is_empty());
    // Locked panels refuse.
    lock(&e, true, cx);
    cx.update(|_, cx| e.update(cx, |e, cx| assert!(!e.set_layer_key(cx))));
    assert!(motion(&e, cx).is_empty());
}

#[gpui_kit::test]
fn keyframe_auto_key_turns_move_and_transform_drags_into_keys(cx: &mut TestAppContext) {
    let (_ws, e, id, cx) = setup(cx);
    let rest = placement(&e, id, cx);
    // Move: the layer's rest pose stays; the keys hold the offset.
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.toggle_auto_key(cx);
            e.snap_bypass = true;
            e.arm_layer_gesture();
            e.begin_move(e.layer_motion_point((12., 12.)), cx);
            let Some(Drag::Move(g)) = e.drag else {
                panic!("move")
            };
            e.move_drag(g, e.layer_motion_point((32., 27.)), cx);
            e.drag_end(cx);
        })
    });
    settle(cx);
    assert_eq!(placement(&e, id, cx), rest);
    let m = motion(&e, cx);
    assert_eq!(value(&m, id, LayerProperty::X, 6.), 20.);
    assert_eq!(value(&m, id, LayerProperty::Y, 6.), 15.);
    assert!(
        m[&id].track(&LayerProperty::Rotation).is_none(),
        "only what changed"
    );
    // The Stage shows the animated layer: its pixels moved.
    cx.update(|_, cx| {
        let e = e.read(cx);
        assert!(e.layer_motion_shown());
        let shown = e.render_doc().unwrap();
        let b = emulsion_core::geometry::node_bounds(&shown, id)
            .unwrap()
            .unwrap();
        assert_eq!((b.x, b.y), (28, 23));
        // The Move tool's box follows it.
        let corner = e.transform_box().unwrap()[0];
        assert!((corner.0 - 28.).abs() < 1e-6 && (corner.1 - 23.).abs() < 1e-6);
    });
    undo(&e, cx);
    assert!(motion(&e, cx).is_empty());
    assert_eq!(placement(&e, id, cx), rest);
    // Transform: a quarter turn becomes a rotation key about the pivot.
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.arm_layer_gesture();
            let (_, w, h, start) = e.transformable().unwrap();
            e.editor.begin("Rotate");
            let turned = Placement {
                rotation: 90.,
                ..start
            };
            e.execute(
                Command::SetPlacement {
                    id,
                    placement: turned,
                },
                cx,
            );
            e.drag = Some(Drag::Transform(super::super::transform::Grab {
                collective: false,
                current: turned,
                mask: None,
                id,
                start,
                handle: super::super::transform::Handle::Rotate,
                start_doc: (0., 0.),
                size: (w, h),
            }));
            e.drag_end(cx);
        })
    });
    settle(cx);
    assert_eq!(placement(&e, id, cx), rest);
    let m = motion(&e, cx);
    assert!((value(&m, id, LayerProperty::Rotation, 6.) - 90.).abs() < 1e-6);
    assert!(value(&m, id, LayerProperty::X, 6.).abs() < 1e-6);
    assert!((value(&m, id, LayerProperty::ScaleX, 6.) - 1.).abs() < 1e-6);
    undo(&e, cx);
    assert!(motion(&e, cx).is_empty());
    // Without auto-key, a drag moves the layer itself.
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.toggle_auto_key(cx);
            e.arm_layer_gesture();
            e.begin_move((12., 12.), cx);
            let Some(Drag::Move(g)) = e.drag else {
                panic!("move")
            };
            e.move_drag(g, (22., 12.), cx);
            e.drag_end(cx);
        })
    });
    assert!(motion(&e, cx).is_empty());
    assert_eq!(placement(&e, id, cx).x, rest.x + 10.);
}

/// Keys at frames 0 and 10 on X (0 → 40), set as one step.
fn two_keys(e: &Entity<EditorView>, id: NodeId, cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            let panel = e.editor.active_page();
            assert!(e.edit_motion(
                panel,
                |m| {
                    let l = m.entry(id).or_default();
                    put_key(l, &LayerProperty::X, 0, 0.);
                    put_key(l, &LayerProperty::X, 10, 40.);
                    Ok(())
                },
                cx,
            ));
            e.timeline_ui.open = true;
            e.timeline_ui.zoom = 4.;
            e.timeline_ui.scroll = 0.;
            e.layer_keys.open_panels.insert(panel);
            e.layer_keys.open_layers.insert((panel, id));
            cx.notify();
        })
    });
    settle(cx);
}

#[gpui_kit::test]
fn keyframe_timeline_drag_retimes_and_delete_removes_in_one_step(cx: &mut TestAppContext) {
    let (_ws, e, id, cx) = setup(cx);
    two_keys(&e, id, cx);
    let panel = cx.update(|_, cx| e.read(cx).editor.active_page());
    let diamond = format!("timeline-key-{panel}-{id}-X-10");
    cx.update(|window, cx| {
        let b = window.find(SharedString::from(diamond.clone())).bounds();
        let at = point(
            b.origin.x + b.size.width / 2.,
            b.origin.y + b.size.height / 2.,
        );
        // 20 px at 4 px a frame: five frames later.
        window.drag(at, point(at.x + px(20.), at.y), cx);
    });
    settle(cx);
    let frames = |e: &Entity<EditorView>, cx: &mut VisualTestContext| {
        key_frames(&motion(e, cx)[&id], Some(&LayerProperty::X))
    };
    assert_eq!(frames(&e, cx), [0, 15]);
    cx.update(|_, cx| {
        let sel = e.read(cx).layer_keys.selected.clone().unwrap();
        assert_eq!((sel.frame, sel.property), (15, Some(LayerProperty::X)));
    });
    undo(&e, cx);
    assert_eq!(frames(&e, cx), [0, 10]);
    // Click selects; Delete removes, as one step.
    cx.update(|window, cx| {
        window.click(SharedString::from(diamond.clone()), cx);
    });
    settle(cx);
    cx.update(|window, cx| {
        window.dispatch_action(Box::new(crate::actions::DeleteNode), cx);
    });
    settle(cx);
    assert_eq!(frames(&e, cx), [0]);
    undo(&e, cx);
    assert_eq!(frames(&e, cx), [0, 10]);
    // Locked panels refuse.
    lock(&e, true, cx);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.layer_keys.selected = Some(KeyRef {
                panel,
                layer: id,
                property: None,
                frame: 10,
            });
            e.timeline_delete_selected(cx);
        })
    });
    assert_eq!(frames(&e, cx), [0, 10]);
}

#[gpui_kit::test]
fn keyframe_curve_editor_applies_a_preset_and_a_dragged_curve(cx: &mut TestAppContext) {
    let (_ws, e, id, cx) = setup(cx);
    two_keys(&e, id, cx);
    let panel = cx.update(|_, cx| e.read(cx).editor.active_page());
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.layer_keys.selected = Some(KeyRef {
                panel,
                layer: id,
                property: Some(LayerProperty::X),
                frame: 0,
            });
            cx.notify();
        })
    });
    settle(cx);
    let first = |e: &Entity<EditorView>, cx: &mut VisualTestContext| {
        motion(e, cx)[&id].track(&LayerProperty::X).unwrap().keys[0]
    };
    // Drag the graph: the first handle moves to the top left.
    cx.update(|window, cx| {
        let graph = window.find("layer-key-ease-graph").bounds();
        let from = point(
            graph.origin.x + graph.size.width * 0.25,
            graph.origin.y + graph.size.height * 0.625,
        );
        let to = point(
            graph.origin.x + graph.size.width * 0.1,
            graph.origin.y + graph.size.height * 0.25,
        );
        window.drag(from, to, cx);
    });
    settle(cx);
    let key = first(&e, cx);
    let curve = key.curve.expect("a custom curve");
    assert!(curve.x1 < 0.2 && curve.y1 > 0.9, "{curve:?}");
    assert_eq!((curve.x2, curve.y2), (0.75, 0.75));
    // The curve drives the value between the keys.
    let expected = 40. * curve.sample(0.5);
    assert!((value(&motion(&e, cx), id, LayerProperty::X, 5.) - expected).abs() < 1e-6);
    undo(&e, cx);
    assert!(first(&e, cx).curve.is_none());
    // A preset from the menu, one step.
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert!(e.ease_selected_key(Easing::EaseIn, None, cx));
        })
    });
    assert_eq!(first(&e, cx).easing, Easing::EaseIn);
    undo(&e, cx);
    assert_eq!(first(&e, cx).easing, Easing::Linear);
}

#[gpui_kit::test]
fn keyframe_motion_path_handle_drag_edits_its_key_in_one_step(cx: &mut TestAppContext) {
    let (_ws, e, id, cx) = setup(cx);
    two_keys(&e, id, cx);
    cx.update(|_, cx| {
        let e = e.read(cx);
        let (lines, handles) = e.motion_path_overlay();
        // Pivot at the layer's centre (16, 16); keys at x + 0 and x + 40.
        assert_eq!(handles, [(16., 16.), (56., 16.)]);
        assert_eq!(lines[0].len(), 24, "one point per frame");
    });
    cx.update(|window, cx| {
        e.update(cx, |e, cx| {
            let b = e.canvas_bounds().unwrap();
            let screen = |d: (f64, f64)| {
                let s = e.view.doc_to_screen(d, &b);
                point(px(s.0 as f32), px(s.1 as f32))
            };
            let (at, to) = (screen((56., 16.)), screen((50., 26.)));
            let down = MouseDownEvent {
                button: MouseButton::Left,
                position: at,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            };
            assert!(e.motion_handle_down(&down, cx));
            assert!(e.motion_drag_move(to, cx));
            assert!(e.motion_drag_end(cx));
            let _ = window;
        })
    });
    settle(cx);
    let m = motion(&e, cx);
    assert!((value(&m, id, LayerProperty::X, 10.) - 34.).abs() < 1.01);
    assert!((value(&m, id, LayerProperty::Y, 10.) - 10.).abs() < 1.01);
    undo(&e, cx);
    let m = motion(&e, cx);
    assert_eq!(value(&m, id, LayerProperty::X, 10.), 40.);
    assert!(m[&id].track(&LayerProperty::Y).is_none());
    // The pivot drags too (Alt over a key point).
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.transport.frame = 24;
            let b = e.canvas_bounds().unwrap();
            let screen = |d: (f64, f64)| {
                let s = e.view.doc_to_screen(d, &b);
                point(px(s.0 as f32), px(s.1 as f32))
            };
            let (at, to) = (screen((16., 16.)), screen((20., 30.)));
            let down = MouseDownEvent {
                button: MouseButton::Left,
                position: at,
                modifiers: Modifiers {
                    alt: true,
                    ..Default::default()
                },
                click_count: 1,
                first_mouse: false,
            };
            assert!(e.motion_handle_down(&down, cx));
            e.motion_drag_move(to, cx);
            e.motion_drag_end(cx);
        })
    });
    let pivot = motion(&e, cx)[&id].pivot.unwrap();
    assert!((pivot[0] - 20.).abs() <= 1. && (pivot[1] - 30.).abs() <= 1.);
}

#[gpui_kit::test]
fn keyframe_effect_keys_animate_adjustment_parameters(cx: &mut TestAppContext) {
    let (_ws, e, _, cx) = setup(cx);
    let adjust = cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            let id = e
                .execute(
                    Command::AddNode {
                        node: Box::new(Node::new(
                            0,
                            "Exposure",
                            NodeKind::Adjust(emulsion_raster::Adjustment::Exposure {
                                exposure: 0.,
                                offset: 0.,
                                gamma: 1.,
                            }),
                        )),
                        slot: Slot::TOP,
                    },
                    cx,
                )
                .unwrap();
            e.set_layer_selection(vec![id], Some(id));
            id
        })
    });
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert!(e.toggle_property_key(
                adjust,
                LayerProperty::Effect("exposure".into()),
                Some(1.5),
                cx
            ));
        })
    });
    let m = motion(&e, cx);
    let property = LayerProperty::Effect("exposure".into());
    assert_eq!(value(&m, adjust, property.clone(), 6.), 1.5);
    cx.update(|_, cx| {
        let shown = e.read(cx).render_doc().unwrap();
        let NodeKind::Adjust(a) = &shown.node(adjust).unwrap().kind else {
            panic!("adjustment")
        };
        assert!(
            a.params()
                .iter()
                .any(|p| p.key == "exposure" && p.value == 1.5)
        );
    });
    undo(&e, cx);
    assert!(motion(&e, cx).is_empty());
}

#[gpui_kit::test]
fn keyframe_sync_option_keeps_or_stretches_keys_in_one_step(cx: &mut TestAppContext) {
    let (_ws, e, id, cx) = setup(cx);
    two_keys(&e, id, cx);
    let panel = cx.update(|_, cx| e.read(cx).editor.active_page());
    let sync = |e: &Entity<EditorView>, cx: &mut VisualTestContext| {
        cx.update(|_, cx| e.read(cx).editor.storyboard().unwrap().keyframe_sync)
    };
    assert_eq!(sync(&e, cx), KeyframeSync::Scale);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert!(e.set_keyframe_sync(KeyframeSync::Keep, cx));
            assert!(e.timeline_set_duration(panel, "48", cx));
        })
    });
    assert_eq!(sync(&e, cx), KeyframeSync::Keep);
    assert_eq!(key_frames(&motion(&e, cx)[&id], None), [0, 10]);
    undo(&e, cx);
    undo(&e, cx);
    assert_eq!(sync(&e, cx), KeyframeSync::Scale);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert!(e.timeline_set_duration(panel, "48", cx))
        })
    });
    assert_eq!(key_frames(&motion(&e, cx)[&id], None), [0, 20]);
}

#[gpui_kit::test]
fn checked_storyboard_pose_failure_preserves_the_accepted_stage_frame(cx: &mut TestAppContext) {
    use emulsion_core::SmartPlacement;
    use emulsion_raster::projective::Projective2;
    let mut doc = Document::new(64, 36);
    let mut node = Node::smart(
        1,
        "Retained Smart",
        Arc::new(Raster::solid(4, 4, [1., 0., 0., 1.])),
        vec![],
        Placement::default(),
    );
    let NodeKind::Smart { placement, .. } = &mut node.kind else {
        unreachable!()
    };
    *placement = SmartPlacement::Projective(Projective2::IDENTITY);
    doc.nodes.push(node);
    doc.next_id = 2;
    doc.validate().unwrap();
    let mut project = ProjectEditor::new_project(ProjectKind::Storyboard, doc).unwrap();
    project
        .edit_storyboard(|board| {
            board.panels.get_mut(&1).unwrap().motion.insert(
                1,
                LayerMotion {
                    pivot: Some([0., 0.]),
                    tracks: vec![PropertyTrack {
                        property: LayerProperty::ScaleX,
                        keys: vec![
                            MotionKey {
                                frame: 0,
                                value: 1.,
                                easing: Easing::Linear,
                                curve: None,
                            },
                            MotionKey {
                                frame: 1,
                                value: 0.,
                                easing: Easing::Linear,
                                curve: None,
                            },
                        ],
                    }],
                },
            );
            Ok(())
        })
        .unwrap();
    let (ws, cx) = open(cx, Document::new(64, 36));
    cx.update(|window, cx| {
        assert!(ws.update(cx, |ws, cx| ws.install_project(
            project,
            "Checked storyboard".into(),
            window,
            cx
        )));
        let view = ws.read(cx).editor.clone().unwrap();
        view.update(cx, |view, cx| {
            view.transport.frame = 0;
            assert!(view.layer_motion_doc().unwrap().is_some());
            view.sync_trees(cx);
            let scene = view.tree.clone();
            let before = view.editor.doc.clone();
            let revision = view.editor.revision;
            let history = view.editor.history.len();
            view.transport.frame = 1;
            assert!(
                view.layer_motion_doc().is_err(),
                "a singular animated map is an error, not an absent pose"
            );
            assert!(view.render_doc().is_err());
            view.sync_trees(cx);
            assert!(Arc::ptr_eq(&scene, &view.tree));
            assert!(view.status.as_ref().is_some_and(|(_, error)| *error));
            // The expensive-tree entry point must fail before marking a worker
            // active, and must keep the same accepted scene as synchronous work.
            view.build_tree_async(cx);
            assert!(view.tree_building.is_none());
            assert!(Arc::ptr_eq(&scene, &view.tree));
            assert_eq!(view.editor.doc, before);
            assert_eq!(
                (view.editor.revision, view.editor.history.len()),
                (revision, history)
            );
            view.transport.frame = 0;
            assert!(
                view.render_doc().is_ok(),
                "the previous valid frame still evaluates"
            );
            view.editor
                .edit_storyboard(|board| {
                    board.panels.get_mut(&1).unwrap().motion.clear();
                    Ok(())
                })
                .unwrap();
            assert!(
                view.layer_motion_doc().unwrap().is_none(),
                "no authored motion remains a successful absence"
            );
            assert!(view.render_doc().is_ok());
        });
    });
}
