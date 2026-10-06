//! The Shot Generator through the real editor: open it on a panel, add a
//! character, pose it and use the set as the panel's reference layer.
use super::*;
use crate::tests::open;
use crate::workspace::Workspace;
use core::prelude::v1::test;
use emulsion_core::project::{ProjectEditor, ProjectKind};
use emulsion_core::storyboard_shot::REFERENCE_LAYER;
use gpui_kit::test::TestWindowExt;

fn setup(
    cx: &mut TestAppContext,
) -> (
    Entity<Workspace>,
    Entity<EditorView>,
    &mut VisualTestContext,
) {
    let (ws, cx) = open(cx, Document::new(64, 36));
    cx.simulate_resize(gpui_kit::size(px(1600.), px(1200.)));
    let editor = cx.update(|window, cx| {
        let project =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(64, 36)).unwrap();
        ws.update(cx, |ws, cx| {
            ws.install_project(project, "Board".into(), window, cx)
        });
        ws.read(cx).editor.clone().unwrap()
    });
    settle(cx);
    (ws, editor, cx)
}

fn settle(cx: &mut VisualTestContext) {
    for _ in 0..4 {
        cx.run_until_parked();
        cx.update(|window, cx| window.render_frame(cx));
    }
    cx.run_until_parked();
}

fn characters(
    e: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Vec<emulsion_scene::Character> {
    cx.update(|_, cx| {
        let e = e.read(cx);
        let panel = e.editor.active_page();
        e.editor
            .panel_shot(panel)
            .map(|s| {
                s.set
                    .objects
                    .iter()
                    .filter_map(|o| o.character().cloned())
                    .collect()
            })
            .unwrap_or_default()
    })
}

#[gpui_kit::test]
fn shot_generator_builds_a_posed_reference_layer(cx: &mut TestAppContext) {
    let (_ws, e, cx) = setup(cx);
    // The Stage toolbar's 3D button opens it in place of the Stage.
    cx.update(|window, cx| window.click("stage-shot-generator", cx));
    settle(cx);
    cx.update(|window, cx| {
        assert!(e.read(cx).shot_generator_open());
        assert!(window.find("shot-generator").visible());
        assert!(window.find("shot-viewport").visible());
    });
    // Add an adult female mannequin: one Undo step.
    cx.update(|window, cx| window.click(("shot-add-character", 1usize), cx));
    settle(cx);
    let added = characters(&e, cx);
    assert_eq!(added.len(), 1);
    assert_eq!(
        added[0].body.kind,
        emulsion_scene::MannequinKind::AdultFemale
    );
    // The viewport rendered off the UI thread.
    cx.update(|_, cx| {
        let generator = e.read(cx).shot_generator.clone().unwrap();
        assert!(generator.read(cx).frame.is_some(), "a viewport frame");
    });
    // Choose the Wave pose for the selected character.
    let wave = emulsion_scene::PosePreset::ALL
        .iter()
        .position(|p| *p == emulsion_scene::PosePreset::Wave)
        .unwrap();
    cx.update(|window, cx| window.click(("shot-pose", wave), cx));
    settle(cx);
    assert_eq!(characters(&e, cx)[0].pose.name, "wave");
    // Use it as the panel's reference layer.
    cx.update(|window, cx| window.click("shot-use-reference", cx));
    settle(cx);
    let layer = cx.update(|_, cx| {
        let e = e.read(cx);
        let node = e
            .editor
            .doc
            .nodes
            .iter()
            .find(|n| n.name == REFERENCE_LAYER)
            .cloned()
            .expect("a reference layer");
        assert!(node.locked && node.opacity < 1.);
        let NodeKind::Raster { raster, .. } = &node.kind else {
            panic!("a raster layer");
        };
        assert_eq!((raster.width(), raster.height()), (64, 36));
        node.id
    });
    // Undo takes the pose back (the reference joined the pose step).
    cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
    settle(cx);
    assert_eq!(characters(&e, cx)[0].pose.name, "stand");
    cx.update(|_, cx| e.update(cx, |e, cx| e.redo(cx)));
    settle(cx);
    assert!(cx.update(|_, cx| e.read(cx).editor.doc.node(layer).is_some()));
    // Done goes back to the Stage.
    cx.update(|window, cx| {
        window.dispatch_action(Box::new(crate::actions::ToggleShotGenerator), cx)
    });
    settle(cx);
    cx.update(|window, cx| {
        assert!(!e.read(cx).shot_generator_open());
        assert!(window.find("storyboard-stage-toolbar").visible());
    });
}

#[gpui_kit::test]
fn describing_a_shot_and_the_explorer(cx: &mut TestAppContext) {
    let (_ws, e, cx) = setup(cx);
    cx.update(|_, cx| e.update(cx, |e, cx| e.toggle_shot_generator(cx)));
    settle(cx);
    let generator = cx.update(|_, cx| e.read(cx).shot_generator.clone().unwrap());
    cx.update(|_, cx| {
        generator.update(cx, |g, cx| {
            g.describe_text("wide shot of two people talking", cx)
        })
    });
    settle(cx);
    assert_eq!(characters(&e, cx).len(), 2);
    cx.update(|window, cx| window.click("shot-explore", cx));
    settle(cx);
    let (count, thumbs) = cx.update(|_, cx| {
        let g = generator.read(cx);
        let explorer = g.explorer.as_ref().expect("proposals");
        (
            explorer.proposals.len(),
            explorer.thumbs.iter().filter(|t| t.is_some()).count(),
        )
    });
    assert!(count >= 6);
    assert_eq!(thumbs, count, "every proposal has a thumbnail");
    let chosen =
        cx.update(|_, cx| generator.read(cx).explorer.as_ref().unwrap().proposals[2].camera);
    cx.update(|window, cx| window.click(("shot-proposal", 2usize), cx));
    settle(cx);
    let camera = cx.update(|_, cx| {
        let e = e.read(cx);
        e.editor
            .panel_shot(e.editor.active_page())
            .unwrap()
            .set
            .camera
    });
    assert_eq!(camera, chosen);
}

fn generator(
    e: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Entity<super::storyboard_shot_generator::ShotGenerator> {
    cx.update(|_, cx| e.read(cx).shot_generator.clone().unwrap())
}

fn object_position(
    e: &Entity<EditorView>,
    id: emulsion_scene::ObjectId,
    cx: &mut VisualTestContext,
) -> Option<glam::Vec3> {
    cx.update(|_, cx| {
        let e = e.read(cx);
        let shot = e.editor.panel_shot(e.editor.active_page())?;
        Some(shot.set.object(id)?.transform.position)
    })
}

#[gpui_kit::test]
fn gizmo_drags_move_along_an_axis_in_one_undo_step(cx: &mut TestAppContext) {
    use emulsion_core::shot_gizmo::Handle;
    let (_ws, e, cx) = setup(cx);
    cx.update(|_, cx| e.update(cx, |e, cx| e.toggle_shot_generator(cx)));
    settle(cx);
    let g = generator(&e, cx);
    cx.update(|_, cx| {
        g.update(cx, |g, cx| {
            let prop = emulsion_scene::Prop::builtin(emulsion_scene::PropKind::Box);
            g.add("Box", emulsion_scene::ObjectKind::Prop(prop), cx)
        })
    });
    settle(cx);
    let id = cx.update(|_, cx| g.read(cx).selected.unwrap());
    let from = object_position(&e, id, cx).unwrap();
    // The Move gizmo's X arrow, in window coordinates.
    let (start, end, size) = cx.update(|_, cx| {
        let g = g.read(cx);
        let (gizmo, view) = g.gizmo().expect("a gizmo on the selection");
        let arrow = gizmo
            .shapes(&view)
            .into_iter()
            .find(|s| s.handle == Handle::Axis(0))
            .unwrap();
        let (a, b) = (arrow.points[0], arrow.points[1]);
        let origin = g.bounds.get().unwrap().origin + g.picture_rect().unwrap().origin;
        let at = |p: glam::Vec2| point(origin.x + px(p.x), origin.y + px(p.y));
        (at(a.lerp(b, 0.8)), at(a.lerp(b, 0.8) + (b - a)), gizmo.size)
    });
    // Hovering lights the handle.
    cx.simulate_mouse_move(start, None, Modifiers::default());
    settle(cx);
    assert_eq!(cx.update(|_, cx| g.read(cx).hover), Some(Handle::Axis(0)));
    // Drag it one arrow length: the box slides along X only.
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    let mid = point((start.x + end.x) / 2., (start.y + end.y) / 2. + px(12.));
    cx.simulate_mouse_move(mid, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    settle(cx);
    let moved = object_position(&e, id, cx).unwrap();
    assert!(
        (moved.x - from.x - size).abs() < size * 0.05,
        "{from} → {moved}"
    );
    assert!((moved.y - from.y).abs() < 1e-5 && (moved.z - from.z).abs() < 1e-5);
    // One Undo step takes the whole drag back, and only it.
    cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
    settle(cx);
    assert_eq!(object_position(&e, id, cx), Some(from));
    // With the viewport focused, E, R and W switch tools and X the axes.
    cx.simulate_keystrokes("e");
    settle(cx);
    assert_eq!(
        cx.update(|_, cx| g.read(cx).tool),
        super::storyboard_shot_generator::ShotTool::Rotate
    );
    cx.simulate_keystrokes("r x");
    settle(cx);
    cx.update(|_, cx| {
        let g = g.read(cx);
        assert_eq!(g.tool, super::storyboard_shot_generator::ShotTool::Scale);
        assert!(g.gizmo_local);
    });
    cx.simulate_keystrokes("w");
    settle(cx);
    assert_eq!(
        cx.update(|_, cx| g.read(cx).tool),
        super::storyboard_shot_generator::ShotTool::Move
    );
}

#[gpui_kit::test]
fn the_stage_shows_layer_depth_parallax_like_the_player(cx: &mut TestAppContext) {
    use emulsion_core::storyboard::{CameraKey, SceneCamera};
    let (_ws, e, cx) = setup(cx);
    let layer = cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.editor
                .execute(emulsion_core::Command::AddNode {
                    node: Box::new(Node::raster(
                        0,
                        "Hills",
                        Arc::new(emulsion_raster::Raster::solid(20, 10, [0., 0.5, 0., 1.])),
                        emulsion_raster::Placement {
                            x: 10.,
                            y: 12.,
                            ..Default::default()
                        },
                    )),
                    slot: emulsion_core::command::Slot::TOP,
                })
                .unwrap();
            let layer = e.editor.doc.nodes.iter().map(|n| n.id).max().unwrap();
            let panel = e.editor.active_page();
            let scene = e.editor.storyboard().unwrap().panels[&panel].scene;
            let key = |frame: u64, x: f64| CameraKey {
                frame,
                x,
                y: 18.,
                zoom: 1.,
                rotation: 0.,
                easing: Default::default(),
                curve: None,
            };
            e.edit_board(
                |b| {
                    b.set_layer_depth(panel, layer, 2.)?;
                    b.cameras.insert(
                        scene,
                        SceneCamera {
                            keys: vec![key(0, 32.), key(10, 52.)],
                            shake: None,
                        },
                    );
                    Ok(())
                },
                cx,
            );
            e.timeline_seek(8, cx);
            layer
        })
    });
    settle(cx);
    let pixels =
        |doc: &Document| emulsion_raster::composite::flatten(&doc.composite_tree(), 0).to_srgba8();
    for camera_view in [false, true] {
        if camera_view {
            cx.update(|_, cx| e.update(cx, |e, cx| e.toggle_camera_view(cx)));
            settle(cx);
        }
        let (stage, player, drawn) = cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                assert!(e.layer_motion_shown(), "the Stage draws the panel moved");
                let panel = e.editor.active_page();
                let player = e
                    .player_drawing(panel, 8, 8, cx)
                    .unwrap()
                    .expect("the player's drawing");
                (e.render_doc().unwrap(), player, e.editor.doc.clone())
            })
        });
        assert_eq!(pixels(&stage), pixels(&player), "camera view {camera_view}");
        assert_ne!(
            stage.node(layer).map(|n| n.kind.clone()),
            drawn.node(layer).map(|n| n.kind.clone()),
            "the far layer moved"
        );
    }
    // At rest (the first frame) the panel shows as drawn.
    cx.update(|_, cx| e.update(cx, |e, cx| e.timeline_seek(0, cx)));
    settle(cx);
    assert!(cx.update(|_, cx| !e.read(cx).layer_motion_shown()));
}

#[gpui_kit::test]
fn the_camera_panel_shows_the_framing_the_camera_is_in(cx: &mut TestAppContext) {
    use emulsion_scene::{CameraAngle, ShotSide, ShotSize};
    let (_ws, e, cx) = setup(cx);
    cx.update(|_, cx| e.update(cx, |e, cx| e.toggle_shot_generator(cx)));
    settle(cx);
    let generator = generator(&e, cx);
    let highlights =
        |cx: &mut VisualTestContext| cx.update(|_, cx| generator.read(cx).camera_highlights());
    // Describing a shot highlights its size, angle and side.
    cx.update(|_, cx| {
        generator.update(cx, |g, cx| {
            g.describe_text("low-angle close-up of two people at a table", cx)
        })
    });
    settle(cx);
    assert_eq!(
        highlights(cx),
        (
            Some(ShotSize::CloseUp),
            Some(CameraAngle::Low),
            Some(ShotSide::Front)
        )
    );
    // Picking an angle reframes at the current size.
    let high = CameraAngle::ALL
        .iter()
        .position(|a| *a == CameraAngle::High)
        .unwrap();
    let before = cx.update(|_, cx| generator.read(cx).shot.set.camera);
    cx.update(|window, cx| window.click(("shot-angle", high), cx));
    settle(cx);
    assert_ne!(
        cx.update(|_, cx| generator.read(cx).shot.set.camera),
        before
    );
    assert_eq!(
        highlights(cx),
        (
            Some(ShotSize::CloseUp),
            Some(CameraAngle::High),
            Some(ShotSide::Front)
        )
    );
    // A size chip keeps the angle; a side chip reframes too.
    let left = ShotSide::ALL
        .iter()
        .position(|s| *s == ShotSide::Left)
        .unwrap();
    let ms = ShotSize::ALL
        .iter()
        .position(|s| *s == ShotSize::Medium)
        .unwrap();
    cx.update(|window, cx| window.click(("shot-size-frame", ms), cx));
    settle(cx);
    cx.update(|window, cx| window.click(("shot-side", left), cx));
    settle(cx);
    assert_eq!(
        highlights(cx),
        (
            Some(ShotSize::Medium),
            Some(CameraAngle::High),
            Some(ShotSide::Left)
        )
    );
    // A Shot Explorer proposal shows its own spec.
    cx.update(|window, cx| window.click("shot-explore", cx));
    settle(cx);
    let spec = cx.update(|_, cx| generator.read(cx).explorer.as_ref().unwrap().proposals[3].spec);
    cx.update(|window, cx| window.click(("shot-proposal", 3usize), cx));
    settle(cx);
    assert_eq!(
        highlights(cx),
        (Some(spec.size), Some(spec.angle), Some(spec.side))
    );
    // The framing survives the trip through the project.
    let saved = cx.update(|_, cx| {
        let e = e.read(cx);
        let shot = e.editor.panel_shot(e.editor.active_page()).unwrap();
        shot.set.current_shot().copied()
    });
    assert_eq!(saved, Some(spec));
    // Moving the camera freely clears the highlights.
    cx.update(|_, cx| {
        generator.update(cx, |g, cx| {
            g.shot.set.camera.position.x += 0.4;
            cx.notify();
        })
    });
    assert_eq!(highlights(cx), (None, None, None));
}
