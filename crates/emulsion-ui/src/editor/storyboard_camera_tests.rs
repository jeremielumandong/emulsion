//! The scene camera through the real editor: the Camera tool's handles and
//! commands (one Undo step each), shake, copy and paste between scenes, the
//! Timeline's camera row, and the player seeing through the camera.
use super::*;
use crate::tests::open;
use crate::workspace::Workspace;
use core::prelude::v1::test;
use emulsion_core::project::{ProjectEditor, ProjectKind};
use emulsion_core::storyboard::{Level, Panel};
use emulsion_core::timeline::FrameRate;
use gpui_kit::test::TestWindowExt;

/// Three 24-frame panels: 1 and 2 in scene 1, 3 in scene 2.
fn storyboard() -> ProjectEditor {
    let mut p = ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(64, 36)).unwrap();
    let blank = p.storyboard().unwrap().blank_panel().unwrap();
    let items = (2..=3)
        .map(|n| (format!("Panel {n}"), Panel::new(0, 24)))
        .collect();
    p.insert_panels(Some(1), &blank, items, None).unwrap();
    let ids: Vec<_> = p.page_list().iter().map(|m| m.id).collect();
    p.edit_storyboard(|b| {
        b.settings.frame_rate = FrameRate::whole(24);
        for id in &ids {
            b.panels.get_mut(id).unwrap().frames = 24;
        }
        b.split(&ids, ids[2], Level::Scene, Some("2"))?;
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
    Vec<PageId>,
    &mut VisualTestContext,
) {
    setup_with(cx, storyboard())
}

fn setup_with(
    cx: &mut TestAppContext,
    project: ProjectEditor,
) -> (
    Entity<Workspace>,
    Entity<EditorView>,
    Vec<PageId>,
    &mut VisualTestContext,
) {
    let (ws, cx) = open(cx, Document::new(64, 36));
    cx.simulate_resize(gpui_kit::size(px(1600.), px(1200.)));
    let ids = project.page_list().iter().map(|m| m.id).collect();
    let editor = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(project, "Board".into(), window, cx)
        });
        ws.read(cx).editor.clone().unwrap()
    });
    settle(cx);
    (ws, editor, ids, cx)
}

fn settle(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.render_frame(cx));
    }
    cx.run_until_parked();
}

fn scene_of(e: &Entity<EditorView>, panel: PageId, cx: &mut VisualTestContext) -> GroupId {
    cx.update(|_, cx| e.read(cx).editor.storyboard().unwrap().panels[&panel].scene)
}

fn camera(
    e: &Entity<EditorView>,
    scene: GroupId,
    cx: &mut VisualTestContext,
) -> Option<SceneCamera> {
    cx.update(|_, cx| {
        e.read(cx)
            .editor
            .storyboard()
            .unwrap()
            .cameras
            .get(&scene)
            .cloned()
    })
}

type Cameras = std::collections::BTreeMap<GroupId, SceneCamera>;

fn cameras(e: &Entity<EditorView>, cx: &mut VisualTestContext) -> Cameras {
    cx.update(|_, cx| e.read(cx).editor.storyboard().unwrap().cameras.clone())
}

/// The last change was one Undo step from `before`: Undo goes back to it
/// and Redo returns.
fn one_step(e: &Entity<EditorView>, before: &Cameras, cx: &mut VisualTestContext) {
    let after = cameras(e, cx);
    assert_ne!(&after, before, "something changed");
    undo(e, cx);
    assert_eq!(&cameras(e, cx), before, "one Undo step");
    cx.update(|_, cx| e.update(cx, |e, cx| e.redo(cx)));
    settle(cx);
    assert_eq!(cameras(e, cx), after);
}

fn undo(e: &Entity<EditorView>, cx: &mut VisualTestContext) {
    cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
    settle(cx);
}

fn seek(e: &Entity<EditorView>, frame: u64, cx: &mut VisualTestContext) {
    cx.update(|_, cx| e.update(cx, |e, cx| e.timeline_seek(frame, cx)));
    settle(cx);
}

fn screen(e: &Entity<EditorView>, d: (f64, f64), cx: &mut VisualTestContext) -> Point<Pixels> {
    cx.update(|_, cx| e.read(cx).doc_to_window(d).unwrap())
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[gpui_kit::test]
fn stage_handles_pan_zoom_and_turn_the_camera_one_step_each(cx: &mut TestAppContext) {
    let (_ws, e, ids, cx) = setup(cx);
    let scene = scene_of(&e, ids[0], cx);
    // Without the tool or a camera, the Stage draws no camera.
    cx.update(|_, cx| assert_eq!(e.read(cx).camera_paint(), CameraPaint::default()));
    cx.update(|window, cx| window.dispatch_action(Box::new(crate::actions::ToggleCameraTool), cx));
    settle(cx);
    cx.update(|window, cx| {
        assert!(window.find("storyboard-camera-bar").visible());
        let e = e.read(cx);
        assert!(e.camera_tool_on());
        let (corners, handles) = e.camera_paint().frame.unwrap();
        assert!(handles);
        assert_eq!((corners[0], corners[2]), ((0., 0.), (64., 36.)));
    });
    // Pan: drag inside the frame 8 px right, 4 down, at frame 6.
    seek(&e, 6, cx);
    let before = cameras(&e, cx);
    let (a, b) = (screen(&e, (20., 10.), cx), screen(&e, (28., 14.), cx));
    cx.update(|window, cx| window.drag(a, b, cx));
    settle(cx);
    let cam = camera(&e, scene, cx).unwrap();
    assert_eq!(cam.keys.len(), 1);
    let key = cam.keys[0];
    assert_eq!(key.frame, 6);
    assert!(
        near(key.x, 40.) && near(key.y, 22.) && near(key.zoom, 1.),
        "{key:?}"
    );
    one_step(&e, &before, cx);
    // Zoom: the corner dragged halfway to the centre doubles the zoom, at
    // the same key.
    let corner = cx.update(|_, cx| e.read(cx).camera_paint().frame.unwrap().0[0]);
    let centre = (40., 22.);
    let half = ((corner.0 + centre.0) / 2., (corner.1 + centre.1) / 2.);
    let (a, b) = (screen(&e, corner, cx), screen(&e, half, cx));
    let before = cameras(&e, cx);
    cx.update(|window, cx| window.drag(a, b, cx));
    settle(cx);
    let key = camera(&e, scene, cx).unwrap().keys[0];
    assert!((key.zoom - 2.).abs() < 0.05, "{key:?}");
    one_step(&e, &before, cx);
    // Turn: just outside a corner, swung a quarter turn about the centre.
    let corner = cx.update(|_, cx| e.read(cx).camera_paint().frame.unwrap().0[0]);
    let a = screen(&e, corner, cx);
    let c = screen(&e, centre, cx);
    let (ux, uy) = (f32::from(a.x - c.x), f32::from(a.y - c.y));
    let len = ux.hypot(uy);
    let out = point(a.x + px(ux / len * 16.), a.y + px(uy / len * 16.));
    // The same distance from the centre, a quarter turn on (y points down).
    let (dx, dy) = (out.x - c.x, out.y - c.y);
    let turned = point(c.x - dy, c.y + dx);
    let before = cameras(&e, cx);
    cx.update(|window, cx| window.drag(out, turned, cx));
    settle(cx);
    let key = camera(&e, scene, cx).unwrap().keys[0];
    assert!((key.rotation - 90.).abs() < 1., "{key:?}");
    one_step(&e, &before, cx);
    undo(&e, cx);
    assert!(near(camera(&e, scene, cx).unwrap().keys[0].rotation, 0.));
    undo(&e, cx);
    undo(&e, cx);
    assert_eq!(camera(&e, scene, cx), None);
    // A click off the frame does nothing and draws nothing.
    let before = cameras(&e, cx);
    let far = screen(&e, (-200., -200.), cx);
    cx.update(|window, cx| window.drag(far, point(far.x + px(10.), far.y), cx));
    settle(cx);
    assert_eq!(cameras(&e, cx), before);
    // Camera view masks around the frame.
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.toggle_camera_view(cx);
            assert!(e.camera_paint().mask.is_some());
            e.toggle_camera_tool(cx);
            assert!(!e.camera_tool_on());
            assert!(e.camera_paint().mask.is_none(), "no camera, no mask");
        })
    });
}

#[gpui_kit::test]
fn camera_keys_are_added_stepped_eased_held_and_reset(cx: &mut TestAppContext) {
    let (_ws, e, ids, cx) = setup(cx);
    let scene = scene_of(&e, ids[0], cx);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            // Panel 2 (frames 24–47) is in scene 1: keys count from its start.
            e.timeline_seek(30, cx);
            assert!(e.camera_add_key(cx));
            e.timeline_seek(3, cx);
            assert!(e.camera_add_key(cx));
        })
    });
    let before = cameras(&e, cx);
    cx.update(|_, cx| e.update(cx, |e, cx| assert!(e.camera_delete_key(cx))));
    settle(cx);
    // Two adds, and the delete removed the key at frame 3 again.
    let cam = camera(&e, scene, cx).unwrap();
    assert_eq!(cam.keys.iter().map(|k| k.frame).collect::<Vec<_>>(), [30]);
    one_step(&e, &before, cx);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.timeline_seek(10, cx);
            assert!(!e.camera_delete_key(cx), "no key at the playhead");
            assert!(e.camera_step_key(true, cx));
            assert_eq!(e.transport.frame, 30);
            assert_eq!(e.editor.active_page(), ids[1]);
            assert!(!e.camera_step_key(true, cx));
            assert!(e.camera_set_easing(Easing::Linear, cx));
            assert!(!e.camera_step_key(false, cx));
        })
    });
    let cam = camera(&e, scene, cx).unwrap();
    assert_eq!(cam.keys[0].easing, Easing::Linear);
    // Hold panel 1 still: keys at its first and last frames.
    seek(&e, 5, cx);
    let before = cameras(&e, cx);
    cx.update(|_, cx| e.update(cx, |e, cx| assert!(e.camera_static_panel(cx))));
    let cam = camera(&e, scene, cx).unwrap();
    assert_eq!(
        cam.keys.iter().map(|k| k.frame).collect::<Vec<_>>(),
        [0, 23, 30]
    );
    assert_eq!(cam.keys[0].x, cam.keys[1].x);
    one_step(&e, &before, cx);
    cx.update(|_, cx| e.update(cx, |e, cx| assert!(e.camera_step_key(false, cx))));
    cx.update(|_, cx| assert_eq!(e.read(cx).transport.frame, 0));
    // Reset removes the keys in one step.
    cx.update(|_, cx| e.update(cx, |e, cx| assert!(e.camera_reset(cx))));
    assert_eq!(camera(&e, scene, cx), None);
    undo(&e, cx);
    assert_eq!(camera(&e, scene, cx).unwrap().keys.len(), 3);
}

#[gpui_kit::test]
fn shake_and_copy_paste_between_scenes(cx: &mut TestAppContext) {
    let (_ws, e, ids, cx) = setup(cx);
    let (one, two) = (scene_of(&e, ids[0], cx), scene_of(&e, ids[2], cx));
    assert_ne!(one, two);
    seek(&e, 0, cx);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert!(e.camera_set_shake(Some(Shake::PRESETS[2].1), cx));
            assert!(!e.camera_set_shake(
                Some(Shake {
                    amplitude: -1.,
                    ..Shake::PRESETS[0].1
                }),
                cx
            ));
            assert!(e.camera_add_key(cx));
            // The key holds the steady camera, not a shaken one.
            let key = e.editor.storyboard().unwrap().cameras[&one].keys[0];
            assert!(near(key.x, 32.) && near(key.rotation, 0.), "{key:?}");
            assert!(e.camera_copy(cx));
        })
    });
    let copied = camera(&e, one, cx).unwrap();
    assert_eq!(copied.shake, Some(Shake::PRESETS[2].1));
    // Paste onto scene 2 (panel 3, from frame 48).
    seek(&e, 50, cx);
    let before = cameras(&e, cx);
    cx.update(|_, cx| e.update(cx, |e, cx| assert!(e.camera_paste(cx))));
    assert_eq!(camera(&e, two, cx), Some(copied));
    one_step(&e, &before, cx);
    // Shake settings, then no shake.
    assert_eq!(
        parse_shake("6, 0.5, 2", 9),
        Ok(Shake {
            amplitude: 6.,
            rotation: 0.5,
            frequency: 2.,
            seed: 9
        })
    );
    assert!(parse_shake("6, 0.5", 9).is_err());
    assert!(parse_shake("6, 90, 2, 1", 9).is_err(), "45° at most");
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert!(e.camera_set_shake(None, cx));
            assert!(e.camera_reset(cx));
        })
    });
    assert_eq!(camera(&e, two, cx), None, "an empty camera is removed");
    assert!(camera(&e, one, cx).is_some());
}

#[gpui_kit::test]
fn the_timeline_camera_row_retimes_keys_and_jumps_the_playhead(cx: &mut TestAppContext) {
    let (_ws, e, ids, cx) = setup(cx);
    let scene = scene_of(&e, ids[0], cx);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.timeline_seek(4, cx);
            e.camera_add_key(cx);
            e.timeline_seek(20, cx);
            e.camera_add_key(cx);
            e.timeline_ui.open = true;
            e.timeline_ui.zoom = 4.;
            e.timeline_ui.scroll = 0.;
            e.timeline_seek(40, cx);
            cx.notify();
        })
    });
    settle(cx);
    let before = cameras(&e, cx);
    let key = format!("timeline-camera-key-{scene}-0");
    cx.update(|window, cx| {
        assert!(window.find("timeline-camera").visible());
        let b = window.find(SharedString::from(key.clone())).bounds();
        let at = point(
            b.origin.x + b.size.width / 2.,
            b.origin.y + b.size.height / 2.,
        );
        // 24 px at 4 px a frame: six frames later.
        window.drag(at, point(at.x + px(24.), at.y), cx);
    });
    settle(cx);
    let frames: Vec<_> = camera(&e, scene, cx)
        .unwrap()
        .keys
        .iter()
        .map(|k| k.frame)
        .collect();
    assert_eq!(frames, [10, 20]);
    one_step(&e, &before, cx);
    // The press jumped the playhead to the key.
    cx.update(|_, cx| assert_eq!(e.read(cx).transport.frame, 4));
    // A key stops at its neighbour.
    cx.update(|window, cx| {
        let b = window.find(SharedString::from(key.clone())).bounds();
        let at = point(
            b.origin.x + b.size.width / 2.,
            b.origin.y + b.size.height / 2.,
        );
        window.drag(at, point(at.x + px(200.), at.y), cx);
    });
    settle(cx);
    assert_eq!(camera(&e, scene, cx).unwrap().keys[0].frame, 19);
    undo(&e, cx);
    undo(&e, cx);
    assert_eq!(camera(&e, scene, cx).unwrap().keys[0].frame, 4);
}

#[gpui_kit::test]
fn the_player_shows_the_camera_and_layer_keyframes(cx: &mut TestAppContext) {
    use emulsion_core::motion::Easing;
    use emulsion_core::storyboard::{
        CameraKey, CameraState, LayerMotion, LayerProperty, MotionKey, PropertyTrack,
    };
    // Panel 1: white with a black left half; panel 3: a black strip that
    // slides right.
    let strip = |w: u32| {
        let raster = emulsion_raster::Raster::from_fn(w, 36, [0; 4], |_, _| [0, 0, 0, 65535]);
        Node::raster(0, "Ink", Arc::new(raster), Placement::default())
    };
    let paper = || Node::new(0, "Paper", NodeKind::Fill { rgba: [255; 4] });
    let mut project = storyboard();
    let ids: Vec<_> = project.page_list().iter().map(|m| m.id).collect();
    for (page, w) in [(ids[0], 32), (ids[2], 8)] {
        project.set_active_page(page).unwrap();
        for node in [paper(), strip(w)] {
            project
                .execute(Command::AddNode {
                    node: Box::new(node),
                    slot: Slot::TOP,
                })
                .unwrap();
        }
    }
    let ink = project.page(ids[2]).unwrap().doc.nodes.last().unwrap().id;
    let key = |frame, value| MotionKey {
        frame,
        value,
        easing: Easing::Linear,
        curve: None,
    };
    project
        .edit_storyboard(|b| {
            let rest = b.rest_camera();
            let scene = b.panels[&ids[0]].scene;
            // Zoom 2 on the right (white) half of panel 1.
            b.cameras.insert(
                scene,
                SceneCamera {
                    keys: vec![CameraKey::at(
                        0,
                        CameraState {
                            x: 48.,
                            zoom: 2.,
                            ..rest
                        },
                    )],
                    shake: None,
                },
            );
            b.panels.get_mut(&ids[2]).unwrap().motion.insert(
                ink,
                LayerMotion {
                    pivot: None,
                    tracks: vec![PropertyTrack {
                        property: LayerProperty::X,
                        keys: vec![key(0, 0.), key(23, 56.)],
                    }],
                },
            );
            Ok(())
        })
        .unwrap();
    project.set_active_page(ids[0]).unwrap();
    let (_ws, e, _, cx) = setup_with(cx, project);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.player.burn_in_on = false;
            e.select_page(ids[0], cx);
            e.timeline_seek(0, cx);
            e.player.fake_clock = Some(Default::default());
            e.playback(super::storyboard_player::Playback::PlayPause, cx);
            e.playback(super::storyboard_player::Playback::PlayPause, cx);
        })
    });
    // The picture shows only white: the camera framed the right half.
    let pixels = |cx: &mut VisualTestContext| {
        for _ in 0..3 {
            settle(cx);
        }
        for _ in 0..20 {
            settle(cx);
            let done = cx.update(|_, cx| {
                let p = &e.read(cx).player;
                !p.composing && p.picture.is_some()
            });
            if done {
                break;
            }
        }
        cx.update(|_, cx| {
            let image = e.read(cx).player.picture.clone().unwrap();
            let size = image.size(0);
            (size.width.0 as usize, image.as_bytes(0).unwrap().to_vec())
        })
    };
    let (w, bytes) = pixels(cx);
    let h = bytes.len() / 4 / w;
    let at = |x: usize| bytes[((h / 2) * w + x) * 4];
    assert!(
        at(2) > 200 && at(w / 2) > 200 && at(w - 3) > 200,
        "all white"
    );
    // Panel 3 at its last frame: the strip has slid to the right edge.
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.playback(super::storyboard_player::Playback::Last, cx)
        })
    });
    let (w, bytes) = pixels(cx);
    let h = bytes.len() / 4 / w;
    let at = |x: usize| bytes[((h / 2) * w + x) * 4];
    assert!(at(2) > 200, "the strip left the left edge");
    assert!(at(w - 3) < 60, "and reached the right one");
}
