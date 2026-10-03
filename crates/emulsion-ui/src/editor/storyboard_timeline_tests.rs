//! Timeline workflows through the real editor: drawing panels and sound,
//! ripple, roll and retime drags as single Undo steps, transitions,
//! markers and snapping, clip placement, trims and fades, the sound library
//! and locked panels.
use super::super::storyboard_layout::StoryboardLayout;
use super::*;
use crate::tests::open;
use crate::workspace::Workspace;
use core::prelude::v1::test;
use emulsion_core::project::{ProjectEditor, ProjectKind};
use emulsion_core::storyboard::Panel;
use emulsion_core::timeline::{AudioAsset, Edge, TransitionKind};
use gpui_kit::test::TestWindowExt;

fn storyboard(frames: &[u32]) -> ProjectEditor {
    let mut p = ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(64, 36)).unwrap();
    let blank = p.storyboard().unwrap().blank_panel().unwrap();
    let items = (2..=frames.len())
        .map(|n| (format!("Panel {n}"), Panel::new(0, 24)))
        .collect();
    if frames.len() > 1 {
        p.insert_panels(Some(1), &blank, items, None).unwrap();
    }
    let ids: Vec<_> = p.page_list().iter().map(|m| m.id).collect();
    p.edit_storyboard(|b| {
        b.settings.frame_rate = FrameRate::whole(24);
        for (id, f) in ids.iter().zip(frames) {
            b.panels.get_mut(id).unwrap().frames = *f;
        }
        Ok(())
    })
    .unwrap();
    p
}

fn sound(name: &str, ms: u64) -> AudioAsset {
    AudioAsset {
        name: name.into(),
        format: "wav".into(),
        duration_ms: ms,
        sample_rate: 48_000,
        channels: 2,
        folder: String::new(),
        source: None,
    }
}

fn setup<'a>(
    cx: &'a mut TestAppContext,
    frames: &[u32],
) -> (
    Entity<Workspace>,
    Entity<EditorView>,
    Vec<PageId>,
    &'a mut VisualTestContext,
) {
    let (ws, cx) = open(cx, Document::new(64, 36));
    cx.simulate_resize(gpui_kit::size(px(1600.), px(1200.)));
    let project = storyboard(frames);
    let ids = project.page_list().iter().map(|m| m.id).collect();
    let editor = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(project, "Board".into(), window, cx)
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.timeline_ui.open = true;
            e.timeline_ui.zoom = 4.;
            e.timeline_ui.scroll = 0.;
            cx.notify();
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.render_frame(cx));
    (ws, editor, ids, cx)
}

fn frames(e: &Entity<EditorView>, ids: &[PageId], cx: &mut VisualTestContext) -> Vec<u32> {
    cx.update(|_, cx| {
        let b = e.read(cx).editor.storyboard().unwrap();
        ids.iter().map(|id| b.panels[id].frames).collect()
    })
}

fn timeline(e: &Entity<EditorView>, cx: &mut VisualTestContext) -> Timeline {
    cx.update(|_, cx| e.read(cx).editor.storyboard().unwrap().timeline.clone())
}

fn centre(b: Bounds<Pixels>) -> Point<Pixels> {
    point(
        b.origin.x + b.size.width / 2.,
        b.origin.y + b.size.height / 2.,
    )
}

fn status(e: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| {
        e.read(cx)
            .status
            .as_ref()
            .map(|(s, _)| s.to_string())
            .unwrap_or_default()
    })
}

/// Drag with modifiers through the timeline's gesture methods.
fn drag(
    e: &Entity<EditorView>,
    drag: TimelineDrag,
    from: Point<Pixels>,
    to: Point<Pixels>,
    modifiers: Modifiers,
    cx: &mut VisualTestContext,
) {
    cx.update(|window, cx| {
        e.update(cx, |e, cx| {
            e.timeline_begin(drag, from, modifiers, window, cx);
            e.timeline_move(to, modifiers, cx);
            e.timeline_end(cx);
        })
    });
    cx.run_until_parked();
}

fn undo(e: &Entity<EditorView>, cx: &mut VisualTestContext) {
    cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
    cx.run_until_parked();
}

#[test]
fn durations_parse_as_frames_seconds_or_timecode() {
    let r = FrameRate::whole(24);
    assert_eq!(parse_duration("36", r), Some(36));
    assert_eq!(parse_duration("36f", r), Some(36));
    assert_eq!(parse_duration("1.5s", r), Some(36));
    assert_eq!(parse_duration("00:00:01:12", r), Some(36));
    assert_eq!(parse_duration("1:12", r), Some(36));
    assert_eq!(parse_duration("abc", r), None);
    assert_eq!(snap_frame(31, &[30, 50], 2.), 30);
    assert_eq!(snap_frame(40, &[30, 50], 2.), 40);
    assert_eq!(volume_from_fraction(volume_fraction(-6.)), -6.);
}

#[gpui_kit::test]
fn timeline_shows_panels_audio_and_markers_and_selects_panels(cx: &mut TestAppContext) {
    let (_ws, e, ids, cx) = setup(cx, &[24, 24, 48]);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert!(e.timeline_audio_edit(
                |t, _| {
                    let id = t.add_asset(sound("Rain", 4000))?;
                    t.tracks.push(AudioTrack::new("Dialogue"));
                    t.place(
                        0,
                        AudioClip {
                            asset: id,
                            name: "Rain".into(),
                            start: 12,
                            frames: 48,
                            offset_ms: 0,
                            ..AudioClip::default()
                        },
                    )?;
                    t.tracks[0].markers.push(Marker {
                        frame: 30,
                        name: "Hit".into(),
                    });
                    Ok(())
                },
                cx,
            ));
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.render_frame(cx);
        assert!(window.find("storyboard-timeline").visible());
        assert!(window.find("timeline-ruler").visible());
        for id in &ids {
            assert!(window.find(("timeline-panel", *id)).visible());
        }
        // Thumbnails are sized by duration: 48 frames is twice 24.
        let a = window.find(("timeline-panel", ids[0])).bounds();
        let c = window.find(("timeline-panel", ids[2])).bounds();
        assert!((f32::from(c.size.width) - 2. * f32::from(a.size.width)).abs() < 2.);
        // A transitions menu at each cut after the first panel.
        assert!(window.try_find(("timeline-cut", ids[0])).is_none());
        assert!(window.find(("timeline-cut", ids[1])).visible());
        assert!(window.find("timeline-clip-0-0").visible());
        assert!(window.find("timeline-marker-0-0").visible());
        assert!(window.find(("timeline-track", 0usize)).visible());
        window.click(("timeline-panel", ids[1]), cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(e.read(cx).editor.active_page(), ids[1]);
        assert_eq!(e.read(cx).board_selection(), vec![ids[1]]);
    });
    // Clicking the ruler moves the playhead; the panel under it is active.
    cx.update(|window, cx| {
        let ruler = window.find("timeline-ruler").bounds();
        let at = point(ruler.origin.x + px(4. * 60.5), ruler.origin.y + px(5.));
        window.drag(at, at, cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(e.read(cx).transport.frame, 60);
        assert_eq!(e.read(cx).editor.active_page(), ids[2]);
    });
    // The play range: In at the playhead, then drag its handle.
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.transport.frame = 10;
            e.timeline_set_range(false, cx);
        })
    });
    cx.update(|window, cx| {
        window.render_frame(cx);
        let handle = centre(window.find("timeline-range-in").bounds());
        eprintln!(
            "DBG handle {:?} ruler {:?} scroll {} zoom {}",
            window.find("timeline-range-in").bounds(),
            window.find("timeline-ruler").bounds(),
            e.read(cx).timeline_ui.scroll,
            e.read(cx).timeline_ui.zoom
        );
        window.drag(handle, point(handle.x + px(20.), handle.y), cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let (a, b) = e.read(cx).transport.range.unwrap();
        assert!((14..=16).contains(&a) && b == 96, "{a}–{b}");
    });
    // View → Timeline closes it.
    cx.update(|_, cx| e.update(cx, |e, cx| e.toggle_storyboard_timeline(cx)));
    cx.update(|window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("storyboard-timeline").is_none());
    });
}

#[gpui_kit::test]
fn dragging_a_panel_edge_ripples_and_is_one_undo_step(cx: &mut TestAppContext) {
    let (_ws, e, ids, cx) = setup(cx, &[24, 24, 24]);
    let starts_before = cx.update(|_, cx| {
        let e = e.read(cx);
        e.editor
            .storyboard()
            .unwrap()
            .panel_starts(&e.timeline_layout())
    });
    cx.update(|window, cx| {
        let edge = centre(window.find(("timeline-panel-edge", ids[0])).bounds());
        // 48 px at 4 px a frame: twelve frames longer.
        window.drag(edge, point(edge.x + px(48.), edge.y), cx);
    });
    cx.run_until_parked();
    assert_eq!(frames(&e, &ids, cx), [36, 24, 24]);
    cx.update(|_, cx| {
        let e = e.read(cx);
        let starts = e
            .editor
            .storyboard()
            .unwrap()
            .panel_starts(&e.timeline_layout());
        // Later panels moved along.
        assert_eq!(starts[1].1, starts_before[1].1 + 12);
        assert_eq!(starts[2].1, starts_before[2].1 + 12);
        assert!(e.timeline_ui.drag.is_none() && e.timeline_ui.overlay.is_none());
    });
    undo(&e, cx);
    assert_eq!(frames(&e, &ids, cx), [24, 24, 24]);
    // Typed durations: frames, seconds or timecode.
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert!(e.timeline_set_duration(ids[1], "00:00:02:00", cx));
            assert!(!e.timeline_set_duration(ids[1], "soon", cx));
        })
    });
    assert_eq!(frames(&e, &ids, cx), [24, 48, 24]);
}

#[gpui_kit::test]
fn alt_drag_rolls_a_cut_and_shift_drag_scales_the_selection(cx: &mut TestAppContext) {
    let (_ws, e, ids, cx) = setup(cx, &[24, 24, 48]);
    let edge = cx.update(|window, _| centre(window.find(("timeline-panel-edge", ids[0])).bounds()));
    let alt = Modifiers {
        alt: true,
        ..Default::default()
    };
    // While dragging, the overlay shows the change and the board is
    // untouched.
    cx.update(|window, cx| {
        e.update(cx, |e, cx| {
            let drag = TimelineDrag::Edge {
                panel: ids[0],
                mode: EdgeMode::Roll,
            };
            e.timeline_begin(drag, edge, alt, window, cx);
            e.timeline_move(point(edge.x + px(24.), edge.y), alt, cx);
            assert!(
                e.timeline_ui
                    .overlay
                    .as_deref()
                    .unwrap()
                    .starts_with("Roll +6")
            );
            assert_eq!(e.editor.storyboard().unwrap().panels[&ids[0]].frames, 24);
            assert_eq!(e.timeline_board().unwrap().panels[&ids[0]].frames, 30);
            e.timeline_end(cx);
        })
    });
    assert_eq!(frames(&e, &ids, cx), [30, 18, 48]);
    undo(&e, cx);
    assert_eq!(frames(&e, &ids, cx), [24, 24, 48]);
    // Shift-drag the second panel's edge with the first two selected: their
    // total grows from 48 to 72, in proportion.
    cx.update(|_, cx| e.update(cx, |e, _| e.set_board_selection(vec![ids[0], ids[1]])));
    let edge = cx.update(|window, cx| {
        window.render_frame(cx);
        centre(window.find(("timeline-panel-edge", ids[1])).bounds())
    });
    let shift = Modifiers {
        shift: true,
        ..Default::default()
    };
    drag(
        &e,
        TimelineDrag::Edge {
            panel: ids[1],
            mode: EdgeMode::from_modifiers(shift),
        },
        edge,
        point(edge.x + px(96.), edge.y),
        shift,
        cx,
    );
    assert_eq!(frames(&e, &ids, cx), [36, 36, 48]);
    undo(&e, cx);
    assert_eq!(frames(&e, &ids, cx), [24, 24, 48]);
    // Fit selection to duration.
    cx.update(|_, cx| e.update(cx, |e, cx| assert!(e.timeline_fit_selection("4s", cx))));
    assert_eq!(frames(&e, &ids, cx), [48, 48, 48]);
}

#[gpui_kit::test]
fn transitions_are_set_at_cuts_resized_by_dragging_and_undone(cx: &mut TestAppContext) {
    let (_ws, e, ids, cx) = setup(cx, &[24, 24, 24]);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.timeline_transition_kind(ids[1], TransitionKind::Wipe { from: Edge::Left }, cx)
        })
    });
    let transition = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| e.read(cx).editor.storyboard().unwrap().panels[&ids[1]].transition)
    };
    // Half a second by default.
    assert_eq!(
        transition(cx),
        Transition {
            kind: TransitionKind::Wipe { from: Edge::Left },
            frames: 12
        }
    );
    let handle = cx.update(|window, cx| {
        window.render_frame(cx);
        assert!(window.find(("timeline-transition", ids[1])).visible());
        centre(window.find(("timeline-transition-edge", ids[1])).bounds())
    });
    // Shorten it to about six frames by dragging its end.
    cx.update(|window, cx| window.drag(handle, point(handle.x - px(24.), handle.y), cx));
    cx.run_until_parked();
    assert!((5..=7).contains(&transition(cx).frames));
    undo(&e, cx);
    assert_eq!(transition(cx).frames, 12);
    undo(&e, cx);
    assert!(transition(cx).is_cut());
    // A transition never outlasts its panel: shortening the panel shortens
    // it.
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.timeline_transition_kind(ids[1], TransitionKind::Dissolve, cx);
            e.timeline_set_duration(ids[1], "4", cx);
        })
    });
    assert_eq!(transition(cx).frames, 4);
    // The menu lists the kinds.
    cx.update(|window, cx| {
        window.render_frame(cx);
        window.click(("timeline-cut", ids[1]), cx);
    });
    cx.run_until_parked();
    cx.update(|window, _| assert!(window.find("popup-menu").visible()));
}

#[gpui_kit::test]
fn markers_snap_cuts_and_drags(cx: &mut TestAppContext) {
    let (_ws, e, ids, cx) = setup(cx, &[24, 24, 24]);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.transport.frame = 30;
            // Without a track, one is made.
            e.timeline_add_marker(None, cx);
            e.transport.frame = 0;
        })
    });
    let t = timeline(&e, cx);
    assert_eq!(t.tracks.len(), 1);
    assert_eq!(t.tracks[0].markers[0].frame, 30);
    cx.update(|_, cx| e.update(cx, |e, cx| e.timeline_snap_cuts(cx)));
    assert_eq!(frames(&e, &ids, cx), [30, 18, 24]);
    undo(&e, cx);
    assert_eq!(frames(&e, &ids, cx), [24, 24, 24]);
    // A ripple drag ending a frame short of the marker snaps onto it.
    let edge = cx.update(|window, cx| {
        window.render_frame(cx);
        centre(window.find(("timeline-panel-edge", ids[0])).bounds())
    });
    cx.update(|window, cx| window.drag(edge, point(edge.x + px(20.), edge.y), cx));
    cx.run_until_parked();
    assert_eq!(frames(&e, &ids, cx), [30, 24, 24]);
    // Ctrl drags freely.
    undo(&e, cx);
    let ctrl = Modifiers::secondary_key();
    drag(
        &e,
        TimelineDrag::Edge {
            panel: ids[0],
            mode: EdgeMode::Ripple,
        },
        edge,
        point(edge.x + px(20.), edge.y),
        ctrl,
        cx,
    );
    assert_eq!(frames(&e, &ids, cx), [29, 24, 24]);
    // Move, rename and delete the marker.
    let marker = cx.update(|window, cx| {
        window.render_frame(cx);
        centre(window.find("timeline-marker-0-0").bounds())
    });
    cx.update(|window, cx| window.drag(marker, point(marker.x + px(40.), marker.y), cx));
    cx.run_until_parked();
    assert_eq!(timeline(&e, cx).tracks[0].markers[0].frame, 40);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert!(e.timeline_rename_marker((0, 0), "Door slam", cx));
            assert!(!e.timeline_rename_marker((0, 0), "  ", cx));
        })
    });
    assert_eq!(timeline(&e, cx).tracks[0].markers[0].name, "Door slam");
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.timeline_ui.clip = None;
            e.timeline_ui.marker = Some((0, 0));
            e.timeline_delete_selected(cx);
        })
    });
    assert!(timeline(&e, cx).tracks[0].markers.is_empty());
    // With the timeline focused, M adds a marker at the playhead and Delete
    // removes the selected one; arrows step the playhead.
    cx.update(|window, cx| {
        let focus = e.update(cx, |e, cx| e.timeline_focus(cx));
        window.focus(&focus, cx);
    });
    cx.simulate_keystrokes("right right m");
    let t = timeline(&e, cx);
    assert_eq!(t.tracks[0].markers.len(), 1);
    assert_eq!(t.tracks[0].markers[0].frame, 2);
    cx.simulate_keystrokes("delete");
    assert!(timeline(&e, cx).tracks[0].markers.is_empty());
}

#[gpui_kit::test]
fn clips_place_move_trim_fade_and_rename(cx: &mut TestAppContext) {
    let (_ws, e, _ids, cx) = setup(cx, &[48, 48]);
    let asset = cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.timeline_add_track(cx);
            e.timeline_add_track(cx);
            e.library_add_assets(vec![Ok(sound("Line", 2000))], cx);
            e.timeline_ui.library.selected.unwrap()
        })
    });
    // Two seconds at 24 fps: 48 frames.
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert!(e.timeline_place_sound(asset, Some(0), 10, (0, 2000), cx));
            // Overlapping clips are refused.
            assert!(!e.timeline_place_sound(asset, Some(0), 40, (0, 1000), cx));
            assert!(e.timeline_place_sound(asset, Some(0), 80, (500, 1000), cx));
        })
    });
    let t = timeline(&e, cx);
    assert_eq!(t.tracks[0].clips.len(), 2);
    assert_eq!(
        (t.tracks[0].clips[0].start, t.tracks[0].clips[0].frames),
        (10, 48)
    );
    assert_eq!(t.tracks[0].clips[1].offset_ms, 500);
    // Trim the end inward by dragging.
    let end = cx.update(|window, cx| {
        window.render_frame(cx);
        centre(window.find("timeline-clip-end-0-0").bounds())
    });
    cx.update(|window, cx| window.drag(end, point(end.x - px(40.), end.y), cx));
    cx.run_until_parked();
    assert_eq!(timeline(&e, cx).tracks[0].clips[0].frames, 38);
    undo(&e, cx);
    assert_eq!(timeline(&e, cx).tracks[0].clips[0].frames, 48);
    // Trimming past the sound's end or into the next clip stops there.
    let rate = FrameRate::whole(24);
    let t = timeline(&e, cx);
    let longer = clip_edit(&t, rate, (0, 0), ClipPart::End, 200, 0).unwrap();
    assert_eq!(longer.tracks[0].clips[0].frames, 48);
    // Trimming the start keeps the sound in place.
    let later = clip_edit(&t, rate, (0, 0), ClipPart::Start, 22, 0).unwrap();
    let c = &later.tracks[0].clips[0];
    assert_eq!((c.start, c.frames, c.offset_ms), (22, 36, 500));
    // Fades stay inside the clip.
    let faded = clip_edit(&t, rate, (0, 0), ClipPart::FadeIn, 200, 0).unwrap();
    assert_eq!(faded.tracks[0].clips[0].fade_in, 48);
    let faded = clip_edit(&faded, rate, (0, 0), ClipPart::FadeOut, 40, 0).unwrap();
    assert_eq!(faded.tracks[0].clips[0].fade_out, 0);
    // Drag the first clip down onto the second track.
    let (body, lower) = cx.update(|window, _| {
        let body = window.find("timeline-clip-0-0").bounds();
        let lower = window.find(("timeline-track", 1usize)).bounds();
        (
            point(body.origin.x + px(60.), body.origin.y + px(30.)),
            lower,
        )
    });
    cx.update(|window, cx| {
        window.drag(body, point(body.x, centre(lower).y), cx);
    });
    cx.run_until_parked();
    let t = timeline(&e, cx);
    assert_eq!((t.tracks[0].clips.len(), t.tracks[1].clips.len()), (1, 1));
    assert_eq!(t.tracks[1].clips[0].start, 10);
    cx.update(|_, cx| assert_eq!(e.read(cx).timeline_ui.clip, Some((1, 0))));
    // Fade-in by dragging its handle; gain and rename (T11).
    let fade = cx.update(|window, cx| {
        window.render_frame(cx);
        centre(window.find("timeline-fade-in-1-0").bounds())
    });
    cx.update(|window, cx| window.drag(fade, point(fade.x + px(32.), fade.y), cx));
    cx.run_until_parked();
    assert!((7..=9).contains(&timeline(&e, cx).tracks[1].clips[0].fade_in));
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert!(e.timeline_clip_gain((1, 0), "-6 dB", cx));
            assert!(!e.timeline_clip_gain((1, 0), "-90", cx));
            assert!(e.timeline_rename_clip((1, 0), "Mia: hello", cx));
            e.timeline_toggle_track(1, true, cx);
        })
    });
    let t = timeline(&e, cx);
    assert_eq!(t.tracks[1].clips[0].gain_db, -6.);
    assert_eq!(t.tracks[1].clips[0].name, "Mia: hello");
    assert!(t.tracks[1].solo && !t.audible(0));
    // Tracks: rename, delete and the limit of sixteen.
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert!(e.timeline_rename_track(0, "Music", cx));
            e.timeline_delete_track(0, cx);
            for _ in 0..20 {
                e.timeline_add_track(cx);
            }
        })
    });
    let t = timeline(&e, cx);
    assert_eq!(t.tracks.len(), 16);
    assert_eq!(t.tracks[0].clips[0].name, "Mia: hello");
}

#[gpui_kit::test]
fn the_sound_library_keeps_folders_and_names(cx: &mut TestAppContext) {
    let (_ws, e, _ids, cx) = setup(cx, &[24]);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.timeline_ui.library.open = true;
            assert!(e.library_new_folder("Music", cx));
            assert!(!e.library_new_folder("../up", cx));
            // Imports land in the current folder.
            let mut a = sound("Theme", 3000);
            a.folder = e.timeline_ui.library.folder.clone();
            e.library_add_assets(
                vec![Ok(a), Ok(sound("Rain", 1000)), Err("Not a sound".into())],
                cx,
            );
        })
    });
    cx.run_until_parked();
    assert_eq!(status(&e, cx), "Not a sound");
    let t = timeline(&e, cx);
    let theme = *t.assets.iter().find(|(_, a)| a.name == "Theme").unwrap().0;
    let rain = *t.assets.iter().find(|(_, a)| a.name == "Rain").unwrap().0;
    assert_eq!(t.assets[&theme].folder, "Music");
    cx.update(|window, cx| {
        window.render_frame(cx);
        assert!(window.find("timeline-library").visible());
        assert!(window.find("library-folder-Music").visible());
        assert!(window.find(("library-sound", theme)).visible());
        // The selected sound shows in the preview with in and out points.
        assert!(window.find("library-preview").visible());
        assert!(window.find("library-preview-out").visible());
    });
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert!(e.library_rename_folder("Music", "Score", cx));
            assert!(e.library_rename_sound(theme, "Main theme", cx));
            assert!(e.library_move_sound(rain, "Score/Weather".into(), cx));
            assert_eq!(e.library_folders(), ["Score", "Score/Weather"]);
        })
    });
    let t = timeline(&e, cx);
    assert_eq!(t.assets[&theme].folder, "Score");
    assert_eq!(t.assets[&theme].name, "Main theme");
    undo(&e, cx);
    undo(&e, cx);
    assert_eq!(timeline(&e, cx).assets[&theme].name, "Theme");
    // Preview in and out points, then place on the selected track.
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.library_select_sound(theme, cx);
            e.timeline_ui.library.in_ms = 1000;
            e.timeline_ui.library.out_ms = 2000;
            e.transport.frame = 5;
            let (lo, hi) = (e.timeline_ui.library.in_ms, e.timeline_ui.library.out_ms);
            assert!(e.timeline_place_sound(theme, None, e.transport.frame, (lo, hi), cx));
            e.library_zoom(4., cx);
            assert_eq!(e.timeline_ui.library.zoom, 4.);
        })
    });
    let t = timeline(&e, cx);
    let clip = &t.tracks[0].clips[0];
    assert_eq!((clip.start, clip.frames, clip.offset_ms), (5, 24, 1000));
    // Drag a sound from the library onto the track.
    cx.update(|window, cx| {
        window.render_frame(cx);
        window.drag_to(("library-sound", theme), ("timeline-track", 0usize), cx);
    });
    cx.run_until_parked();
    assert_eq!(timeline(&e, cx).tracks[0].clips.len(), 2);
    undo(&e, cx);
    // Sounds in use cannot be deleted; unused ones can.
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert!(!e.library_delete_sound(theme, cx));
            e.library_delete_unused(cx);
        })
    });
    let t = timeline(&e, cx);
    assert!(t.assets.contains_key(&theme) && !t.assets.contains_key(&rain));
}

#[gpui_kit::test]
fn locked_panels_refuse_timing_edits(cx: &mut TestAppContext) {
    let (_ws, e, ids, cx) = setup(cx, &[24, 24, 24]);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert!(e.edit_board(
                |b| {
                    b.panels.get_mut(&ids[1]).unwrap().locked = true;
                    Ok(())
                },
                cx
            ))
        })
    });
    let edge = cx.update(|window, cx| {
        window.render_frame(cx);
        centre(window.find(("timeline-panel-edge", ids[1])).bounds())
    });
    cx.update(|window, cx| window.drag(edge, point(edge.x + px(40.), edge.y), cx));
    cx.run_until_parked();
    assert!(status(&e, cx).contains("locked"));
    assert_eq!(frames(&e, &ids, cx), [24, 24, 24]);
    // Rolling the cut before it would change it too.
    let edge = cx.update(|window, _| centre(window.find(("timeline-panel-edge", ids[0])).bounds()));
    let alt = Modifiers {
        alt: true,
        ..Default::default()
    };
    drag(
        &e,
        TimelineDrag::Edge {
            panel: ids[0],
            mode: EdgeMode::Roll,
        },
        edge,
        point(edge.x + px(20.), edge.y),
        alt,
        cx,
    );
    assert_eq!(frames(&e, &ids, cx), [24, 24, 24]);
    // A plain ripple of the panel before is fine: the locked panel moves
    // but keeps its duration.
    drag(
        &e,
        TimelineDrag::Edge {
            panel: ids[0],
            mode: EdgeMode::Ripple,
        },
        edge,
        point(edge.x + px(20.), edge.y),
        Modifiers::secondary_key(),
        cx,
    );
    assert_eq!(frames(&e, &ids, cx), [29, 24, 24]);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert!(!e.timeline_set_duration(ids[1], "48", cx));
            e.timeline_transition_kind(ids[1], TransitionKind::Dissolve, cx);
            assert!(
                e.editor.storyboard().unwrap().panels[&ids[1]]
                    .transition
                    .is_cut()
            );
            e.set_board_selection(ids.clone());
            assert!(!e.timeline_fit_selection("10s", cx));
        })
    });
    assert_eq!(frames(&e, &ids, cx), [29, 24, 24]);
}

#[gpui_kit::test]
fn the_timing_layout_opens_the_timeline(cx: &mut TestAppContext) {
    let (_ws, e, _ids, cx) = setup(cx, &[24, 24]);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.apply_storyboard_layout(StoryboardLayout::Drawing, cx);
            assert!(!e.timeline_open());
            e.apply_storyboard_layout(StoryboardLayout::Timing, cx);
            assert!(e.timeline_open() && !e.board_open());
            assert!(!e.sidebar_layout.upper_collapsed);
            assert_eq!(e.current_storyboard_layout(), StoryboardLayout::Timing);
            e.apply_storyboard_layout(StoryboardLayout::Overview, cx);
            assert!(!e.timeline_open() && e.board_open());
            e.apply_storyboard_layout(StoryboardLayout::Drawing, cx);
        })
    });
    cx.run_until_parked();
    // Ctrl+Alt+T toggles it.
    cx.update(|window, cx| {
        let focus = e.read(cx).canvas_focus.clone();
        window.focus(&focus, cx);
    });
    cx.simulate_keystrokes("ctrl-alt-t");
    cx.update(|_, cx| assert!(e.read(cx).timeline_open()));
}

#[gpui_kit::test]
fn envelope_keys_drag_on_the_clip_and_the_effects_editor_keys_eq(cx: &mut TestAppContext) {
    use emulsion_core::timeline::{ClipParam, EffectKey};
    let (_ws, e, _ids, cx) = setup(cx, &[48, 48]);
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert!(e.timeline_audio_edit(
                |t, _| {
                    let id = t.add_asset(sound("Line", 4000))?;
                    t.tracks.push(AudioTrack::new("Dialogue"));
                    let mut clip = AudioClip {
                        asset: id,
                        name: "Line".into(),
                        start: 12,
                        frames: 48,
                        ..AudioClip::default()
                    };
                    clip.envelope = vec![EffectKey::new(0, 0.), EffectKey::new(24, -12.)];
                    t.place(0, clip)
                },
                cx,
            ));
        })
    });
    cx.run_until_parked();
    let key = cx.update(|window, cx| {
        window.render_frame(cx);
        centre(window.find("timeline-envelope-key-0-0-1").bounds())
    });
    // 40 px right is 10 frames at 4 px a frame; down lowers the level.
    cx.update(|window, cx| window.drag(key, point(key.x + px(40.), key.y + px(8.)), cx));
    cx.run_until_parked();
    let moved = timeline(&e, cx).tracks[0].clips[0].envelope[1];
    assert_eq!(moved.frame, 34);
    assert!(moved.db < -12., "{}", moved.db);
    undo(&e, cx);
    assert_eq!(timeline(&e, cx).tracks[0].clips[0].envelope[1].frame, 24);

    // The Effects editor keys the low band at the playhead.
    cx.update(|window, cx| {
        e.update(cx, |e, cx| {
            e.timeline_seek(17, cx);
            e.open_clip_effects((0, 0), window, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.render_frame(cx);
        assert!(window.find("clip-effects").visible());
        window.click("clip-fx-low-key", cx);
    });
    cx.run_until_parked();
    let clip = timeline(&e, cx).tracks[0].clips[0].clone();
    assert_eq!(clip.eq.low.keys, vec![EffectKey::new(5, 0.)]);
    assert!(clip.has_effects());
    cx.update(|_, cx| e.update(cx, |e, cx| e.clip_fx_set((0, 0), ClipParam::Low, 6., cx)));
    cx.run_until_parked();
    assert_eq!(timeline(&e, cx).tracks[0].clips[0].eq.low.keys[0].db, 6.);
    undo(&e, cx);
    undo(&e, cx);
    assert!(timeline(&e, cx).tracks[0].clips[0].eq.low.keys.is_empty());
}
