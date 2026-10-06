//! The animatic player through the real editor: transport shortcuts, the
//! clock driving the playhead, loop and range, burn-in on the picture, sound
//! on the audio clock, scrubbing and playing on without a device.
use super::*;
use crate::playback::audio_out::FakeOutput;
use crate::playback::clock::FakeClock;
use crate::tests::open;
use core::prelude::v1::test;
use emulsion_core::project::{ProjectEditor, ProjectKind};
use emulsion_core::storyboard::Panel;
use emulsion_core::timeline::{AudioAsset, AudioClip, AudioTrack, Edge, Transition};
use gpui_kit::test::TestWindowExt;

fn storyboard(panels: usize) -> ProjectEditor {
    let mut p = ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(64, 36)).unwrap();
    let blank = p.storyboard().unwrap().blank_panel().unwrap();
    let items = (2..=panels)
        .map(|n| (format!("Panel {n}"), Panel::new(0, 24)))
        .collect();
    p.insert_panels(Some(1), &blank, items, None).unwrap();
    p.edit_storyboard(|b| {
        b.panels.get_mut(&1).unwrap().frames = 24;
        Ok(())
    })
    .unwrap();
    p
}

fn setup(
    cx: &mut TestAppContext,
    project: ProjectEditor,
) -> (Entity<EditorView>, FakeClock, &mut VisualTestContext) {
    let (ws, cx) = open(cx, Document::new(64, 36));
    cx.simulate_resize(gpui_kit::size(px(1600.), px(1200.)));
    let clock = FakeClock::default();
    let fake = clock.clone();
    let editor = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(project, "Board".into(), window, cx)
        });
        let editor = ws.read(cx).editor.clone().unwrap();
        editor.update(cx, |e, _| e.player.fake_clock = Some(fake));
        let focus = editor.read(cx).canvas_focus.clone();
        window.focus(&focus, cx);
        editor
    });
    settle(cx);
    (editor, clock, cx)
}

fn settle(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.render_frame(cx));
    }
    cx.run_until_parked();
}

fn tick(e: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|_, cx| e.update(cx, |e, cx| e.animatic_tick(cx)))
}

fn transport(e: &Entity<EditorView>, cx: &mut VisualTestContext) -> crate::playback::Transport {
    cx.update(|_, cx| e.read(cx).transport.clone())
}

/// A quick tap of Space on the Stage: down, then up.
fn tap_space(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("space");
    cx.simulate_event(KeyUpEvent {
        keystroke: Keystroke::parse("space").unwrap(),
    });
    settle(cx);
}

fn action(cx: &mut VisualTestContext, action: impl Action) {
    cx.update(|window, cx| window.dispatch_action(Box::new(action), cx));
}

#[test]
fn pictures_blend_transitions_and_carry_burn_in() {
    let (w, h) = (40u32, 30u32);
    let black: Vec<u8> = [0, 0, 0, 255].repeat((w * h) as usize);
    let white: Vec<u8> = [255; 4].repeat((w * h) as usize);
    let half = compose(
        &white,
        Some((&black, TransitionKind::Dissolve, 0.5)),
        w,
        h,
        &[],
        &BurnIn::default(),
    );
    assert_eq!(&half[..4], &[128, 128, 128, 255]);
    // Fade colours are RGB; pictures are BGRA.
    let red = compose(
        &white,
        Some((
            &black,
            TransitionKind::FadeToColor { color: [255, 0, 0] },
            0.5,
        )),
        w,
        h,
        &[],
        &BurnIn::default(),
    );
    assert_eq!(&red[..4], &[0, 0, 255, 255]);
    let burned = compose(
        &white,
        None,
        w,
        h,
        &["00:00:01:00".into()],
        &BurnIn {
            size: 10.,
            ..BurnIn::default()
        },
    );
    assert_eq!(&burned[..4], &[255; 4], "the top is the picture");
    let bottom = ((h - 1) * w * 4) as usize;
    assert!(burned[bottom] < 200, "the burn-in band darkens the bottom");
}

#[gpui_kit::test]
fn the_clock_drives_the_playhead_and_shortcuts_move_it(cx: &mut TestAppContext) {
    let (e, clock, cx) = setup(cx, storyboard(3));
    let ids: Vec<PageId> = cx.update(|_, cx| e.read(cx).playback_layout());
    // A tap of Space on the Stage plays.
    tap_space(cx);
    assert!(transport(&e, cx).playing);
    assert!(cx.update(|_, cx| e.read(cx).player.showing));
    clock.advance(1.5);
    assert!(tick(&e, cx));
    assert_eq!(transport(&e, cx).frame, 36);
    // A slow display drops frames rather than falling behind.
    clock.advance(0.5);
    tick(&e, cx);
    assert_eq!(transport(&e, cx).frame, 48);
    assert!(cx.update(|_, cx| e.read(cx).player.dropped) > 0);
    // Space again pauses; the panel under the playhead becomes active.
    tap_space(cx);
    let t = transport(&e, cx);
    assert!(!t.playing && t.frame == 48);
    assert_eq!(cx.update(|_, cx| e.read(cx).editor.active_page()), ids[2]);
    // Frame steps, Home and End.
    action(cx, crate::actions::PreviousFrame);
    assert_eq!(transport(&e, cx).frame, 47);
    assert_eq!(cx.update(|_, cx| e.read(cx).editor.active_page()), ids[1]);
    action(cx, crate::actions::NextFrame);
    assert_eq!(transport(&e, cx).frame, 48);
    action(cx, crate::actions::LastFrame);
    assert_eq!(transport(&e, cx).frame, 71);
    action(cx, crate::actions::FirstFrame);
    assert_eq!(transport(&e, cx).frame, 0);
    // Escape on the Stage stops and goes back to drawing.
    cx.simulate_keystrokes("escape");
    settle(cx);
    assert!(!cx.update(|_, cx| e.read(cx).player.showing));
}

#[gpui_kit::test]
fn the_play_range_bounds_playback_and_loops(cx: &mut TestAppContext) {
    let (e, clock, cx) = setup(cx, storyboard(3));
    cx.update(|_, cx| e.update(cx, |e, cx| e.timeline_seek(10, cx)));
    action(cx, crate::actions::SetPlayIn);
    cx.update(|_, cx| e.update(cx, |e, cx| e.timeline_seek(29, cx)));
    action(cx, crate::actions::SetPlayOut);
    assert_eq!(transport(&e, cx).range, Some((10, 30)));
    action(cx, crate::actions::PlayPause);
    assert_eq!(
        transport(&e, cx).frame,
        10,
        "from the last frame, start over"
    );
    clock.advance(25. / 24.);
    assert!(!tick(&e, cx), "playback ends at the range end");
    assert_eq!(transport(&e, cx).frame, 29);
    action(cx, crate::actions::ToggleLoop);
    assert!(transport(&e, cx).looping);
    action(cx, crate::actions::PlayPause);
    clock.advance(21. / 24.);
    assert!(tick(&e, cx));
    let t = transport(&e, cx);
    assert!(
        t.playing && t.frame == 10,
        "looping goes back to the range start"
    );
    action(cx, crate::actions::ClearPlayRange);
    assert_eq!(transport(&e, cx).range, None);
}

#[gpui_kit::test]
fn the_picture_shows_the_animatic_with_burn_in(cx: &mut TestAppContext) {
    let mut project = storyboard(2);
    project
        .edit_storyboard(|b| {
            b.panels.get_mut(&2).unwrap().transition = Transition {
                kind: TransitionKind::Wipe { from: Edge::Left },
                frames: 6,
            };
            Ok(())
        })
        .unwrap();
    let (e, clock, cx) = setup(cx, project);
    action(cx, crate::actions::PlayPause);
    clock.advance(26. / 24.);
    tick(&e, cx);
    settle(cx);
    // Pictures load and compose in the background.
    for _ in 0..3 {
        cx.update(|_, cx| e.update(cx, |e, cx| e.refresh_picture(cx)));
        settle(cx);
    }
    cx.update(|_, cx| {
        let e = e.read(cx);
        let made = e.player.made.clone().expect("a picture was made");
        assert_eq!(made.to.panel, 2);
        assert!(made.from.is_some(), "the wipe from panel 1 plays");
        assert_eq!(made.lines[0], "Scene 1   Panel 2   00:00:01:02");
        let picture = e.player.picture.clone().unwrap();
        let (w, h, bytes) = image_bytes(&picture).unwrap();
        let px = |x: u32, y: u32| bytes[((y * w + x) * 4) as usize];
        assert!(px(w / 2, h - 1) < 200, "burn-in drawn at the bottom");
    });
    // Without burn-in the panel picture shows as it is.
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.player.burn_in_on = false;
            e.playback(Playback::Step(10), cx);
        })
    });
    settle(cx);
    cx.update(|_, cx| {
        let e = e.read(cx);
        assert!(!e.player.owned && e.player.made.as_ref().unwrap().lines.is_empty());
    });
}

fn with_sound(project: &mut ProjectEditor) {
    project
        .edit_storyboard(|b| {
            let asset = b.timeline.add_asset(AudioAsset {
                name: "Line".into(),
                format: "wav".into(),
                duration_ms: 3000,
                sample_rate: 48_000,
                channels: 2,
                folder: String::new(),
                source: None,
            })?;
            b.timeline.tracks.push(AudioTrack::new("Dialogue"));
            b.timeline.place(
                0,
                AudioClip {
                    asset,
                    name: "Line".into(),
                    start: 0,
                    frames: 72,
                    offset_ms: 0,
                    ..AudioClip::default()
                },
            )
        })
        .unwrap();
}

#[gpui_kit::test]
fn without_an_audio_device_playback_runs_silently_on_the_clock(cx: &mut TestAppContext) {
    let mut project = storyboard(3);
    with_sound(&mut project);
    let (e, clock, cx) = setup(cx, project);
    action(cx, crate::actions::PlayPause);
    settle(cx);
    cx.update(|_, cx| {
        let e = e.read(cx);
        assert!(matches!(e.player.device, Device::Missing(_)));
        assert!(
            e.player
                .notice
                .as_deref()
                .is_some_and(|n| n.contains("without sound"))
        );
        assert!(e.transport.playing);
    });
    clock.advance(0.5);
    tick(&e, cx);
    assert_eq!(transport(&e, cx).frame, 12);
}

#[gpui_kit::test]
fn sound_drives_the_clock_and_scrubbing_plays_grains(cx: &mut TestAppContext) {
    let mut project = storyboard(3);
    with_sound(&mut project);
    let (e, clock, cx) = setup(cx, project);
    let out = FakeOutput::new(emulsion_io::audio::RATE);
    let device = out.clone();
    cx.update(|_, cx| e.update(cx, |e, _| e.player.device = Device::Open(Rc::new(device))));
    // Moving the playhead while stopped plays a grain there.
    cx.update(|_, cx| e.update(cx, |e, cx| e.timeline_seek(12, cx)));
    settle(cx);
    cx.update(|_, cx| e.update(cx, |e, cx| e.timeline_seek(20, cx)));
    settle(cx);
    assert_eq!(out.state(), Some(false), "a scrub grain");
    // Playing waits for the first sound, then follows the device.
    action(cx, crate::actions::PlayPause);
    for _ in 0..200 {
        if out.state() == Some(true) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
        tick(&e, cx);
    }
    assert_eq!(out.state(), Some(true), "playing on the audio clock");
    // The system clock does not move the picture; the device does.
    clock.advance(10.);
    tick(&e, cx);
    assert_eq!(transport(&e, cx).frame, 20);
    out.pull(48_000);
    tick(&e, cx);
    assert_eq!(transport(&e, cx).frame, 44);
    action(cx, crate::actions::PlayPause);
    assert_eq!(out.state(), None, "pausing stops the sound");
}

/// Playing from the transport's Play button: the button, Space, Escape and
/// Stop must all still reach the player afterwards, and Save must work.
#[gpui_kit::test]
fn playing_from_the_button_can_be_paused_stopped_and_saved(cx: &mut TestAppContext) {
    let (e, clock, cx) = setup(cx, storyboard(3));
    let showing = |e: &Entity<EditorView>, cx: &mut VisualTestContext| {
        cx.update(|_, cx| e.read(cx).player.showing)
    };
    cx.update(|window, cx| window.click("transport-play", cx));
    settle(cx);
    assert!(transport(&e, cx).playing, "the Play button starts playback");
    clock.advance(0.5);
    tick(&e, cx);
    // Clicking the same button again pauses.
    cx.update(|window, cx| window.click("transport-play", cx));
    settle(cx);
    assert!(!transport(&e, cx).playing, "the Play button pauses");
    // Space resumes and pauses even though the button has focus.
    tap_space(cx);
    assert!(
        transport(&e, cx).playing,
        "Space resumes after clicking Play"
    );
    tap_space(cx);
    assert!(
        !transport(&e, cx).playing,
        "Space pauses after clicking Play"
    );
    // Escape stops and leaves the player.
    cx.simulate_keystrokes("escape");
    settle(cx);
    assert!(!showing(&e, cx), "Escape stops after clicking Play");
    // Play again, then the Stop button.
    cx.update(|window, cx| window.click("transport-play", cx));
    settle(cx);
    cx.update(|window, cx| window.click("transport-stop", cx));
    settle(cx);
    assert!(!transport(&e, cx).playing && !showing(&e, cx), "Stop stops");
    // Saving afterwards writes the project.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.emu");
    // Save also remembers this project in Home. Isolate that catalog so later
    // Home tests cannot inherit this fixture's temporary board.emu entry.
    cx.update(|_, cx| {
        let workspace = e
            .read(cx)
            .library_workspace
            .as_ref()
            .unwrap()
            .upgrade()
            .unwrap();
        workspace.update(cx, |workspace, _| {
            workspace.home_state.projects.catalog_root = Some(dir.path().join("catalog"));
        });
        e.update(cx, |e, _| e.editor.path = Some(path.clone()));
    });
    action(cx, crate::actions::Save);
    for _ in 0..50 {
        settle(cx);
        if path.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(path.exists(), "Save after playback writes the file");
}

/// Saving with Ctrl+S and exporting the PDF while the animatic plays.
#[gpui_kit::test]
fn saving_and_exporting_work_while_the_animatic_plays(cx: &mut TestAppContext) {
    let (e, clock, cx) = setup(cx, storyboard(3));
    cx.update(|window, cx| window.click("transport-play", cx));
    settle(cx);
    clock.advance(0.7);
    tick(&e, cx);
    assert!(transport(&e, cx).playing);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.emu");
    // Save also remembers this project in Home. Isolate that catalog so later
    // Home tests cannot inherit this fixture's temporary board.emu entry.
    cx.update(|_, cx| {
        let workspace = e
            .read(cx)
            .library_workspace
            .as_ref()
            .unwrap()
            .upgrade()
            .unwrap();
        workspace.update(cx, |workspace, _| {
            workspace.home_state.projects.catalog_root = Some(dir.path().join("catalog"));
        });
        e.update(cx, |e, _| e.editor.path = Some(path.clone()));
    });
    cx.simulate_keystrokes("ctrl-s");
    for _ in 0..50 {
        settle(cx);
        if path.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(path.exists(), "Ctrl+S while playing saves");
    cx.update(|window, cx| e.update(cx, |e, cx| e.storyboard_print(true, window, cx)));
    settle(cx);
    let status = cx.update(|_, cx| e.read(cx).status.clone());
    cx.update(|window, _| {
        assert!(
            window.try_find("storyboard-pdf-options").is_some(),
            "Export PDF while playing opens the dialog (status: {status:?})"
        );
    });
}

#[gpui_kit::test]
fn outgoing_checked_pose_failure_retains_picture_instead_of_publishing_a_cut(
    cx: &mut TestAppContext,
) {
    use emulsion_core::motion::Easing;
    use emulsion_core::storyboard::{LayerMotion, LayerProperty, MotionKey, PropertyTrack};
    use emulsion_core::{Command, Node, NodeKind, SmartPlacement, command::Slot};
    use emulsion_raster::{Placement, Raster, projective::Projective2};
    let mut project = storyboard(2);
    project.set_active_page(1).unwrap();
    let mut node = Node::smart(
        0,
        "Outgoing Smart",
        Arc::new(Raster::solid(4, 4, [1., 0., 0., 1.])),
        vec![],
        Placement::default(),
    );
    let NodeKind::Smart { placement, .. } = &mut node.kind else {
        unreachable!()
    };
    *placement = SmartPlacement::Projective(Projective2::IDENTITY);
    let outgoing = project
        .execute(Command::AddNode {
            node: Box::new(node),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    project.set_active_page(2).unwrap();
    let incoming = project
        .execute(Command::AddNode {
            node: Box::new(Node::raster(
                0,
                "Incoming",
                Arc::new(Raster::solid(4, 4, [0., 1., 0., 1.])),
                Placement::default(),
            )),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap();
    project
        .edit_storyboard(|board| {
            board.panels.get_mut(&1).unwrap().motion.insert(
                outgoing,
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
                                frame: 23,
                                value: 0.,
                                easing: Easing::Linear,
                                curve: None,
                            },
                        ],
                    }],
                },
            );
            let next = board.panels.get_mut(&2).unwrap();
            next.transition = Transition {
                kind: TransitionKind::Dissolve,
                frames: 6,
            };
            // An evaluated incoming document avoids thumbnail-loading races, so the
            // failure is specifically the outgoing checked transition pose.
            next.motion.insert(
                incoming,
                LayerMotion {
                    pivot: None,
                    tracks: vec![PropertyTrack {
                        property: LayerProperty::Opacity,
                        keys: vec![MotionKey {
                            frame: 0,
                            value: 1.,
                            easing: Easing::Linear,
                            curve: None,
                        }],
                    }],
                },
            );
            Ok(())
        })
        .unwrap();
    let (view, _, cx) = setup(cx, project);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            assert!(view.player_side(1, 0, 0, 256, cx).unwrap().is_some());
            assert!(view.player_side(1, 23, 23, 256, cx).is_err());
            assert!(
                view.player_side(u64::MAX, 0, 0, 256, cx).unwrap().is_none(),
                "missing side remains distinct from an evaluation error"
            );
            let previous = Arc::new(crate::viewport::bgra_image(1, 1, vec![0, 0, 255, 255]));
            view.player.picture = Some(previous.clone());
            view.player.owned = false;
            view.player.made = None;
            view.player.showing = true;
            view.player.burn_in_on = false;
            view.transport.frame = 26;
            let before = view.editor.doc.clone();
            let revision = view.editor.revision;
            view.refresh_picture(cx);
            assert!(Arc::ptr_eq(
                view.player.picture.as_ref().unwrap(),
                &previous
            ));
            assert!(
                view.player.made.is_none(),
                "failed transition cannot become a newly accepted cut"
            );
            assert!(!view.player.composing);
            assert!(view.player.notice.is_some());
            assert!(view.status.as_ref().is_some_and(|(_, error)| *error));
            assert_eq!(view.editor.doc, before);
            assert_eq!(view.editor.revision, revision);
        })
    });
}
