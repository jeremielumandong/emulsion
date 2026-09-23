//! Hidden documents retain editing state without presentation work or display caches.
use super::*;
use crate::editor::{EditorView, Tool};
use crate::viewport::{self, Key, View, Which};
use crate::workspace::Screen;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{Modifiers, MouseButton};
use std::time::Duration;

fn add_tab(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.install(
                doc(&["Second"], None),
                None,
                None,
                None,
                "Second".into(),
                window,
                cx,
            );
        });
    });
    cx.run_until_parked();
    cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap())
}

fn counts(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> (usize, usize) {
    cx.update(|_, cx| {
        let editor = editor.read(cx);
        (
            editor.canvas_view.read(cx).render_count,
            editor.sidebar_view.read(cx).render_count,
        )
    })
}

#[gpui_kit::test]
fn inactive_tabs_pause_animation_and_preserve_editing_state(cx: &mut TestAppContext) {
    for compact in [false, true] {
        let (workspace, cx) = open(cx, doc(&["Photo"], None));
        let first = cx.update(|window, cx| {
            cx.global_mut::<AppSettings>().0.compact_chrome = compact;
            cx.set_reduce_motion(false);
            let editor = workspace.read(cx).editor.clone().unwrap();
            let focus = editor.read(cx).canvas_focus.clone();
            window.focus(&focus, cx);
            window.refresh();
            editor
        });
        cx.run_until_parked();
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-shift-n"
        } else {
            "ctrl-shift-n"
        });
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-a"
        } else {
            "ctrl-a"
        });
        let snapshot = cx.update(|_, cx| {
            first.update(cx, |editor, cx| {
                editor.set_tool(Tool::Brush, cx);
                editor.view = View {
                    zoom: 1.25,
                    center: (117., 83.),
                    rotation: 0.,
                };
                assert!(editor.editor.doc.selection.is_some());
                assert!(editor.editor.history.can_undo());
                (
                    editor.editor.doc.clone(),
                    editor.editor.history.len(),
                    editor.view,
                    editor.selected,
                )
            })
        });
        cx.run_until_parked();
        let second = add_tab(&workspace, cx);
        let phase = cx.update(|_, cx| {
            let editor = first.read(cx);
            assert!(!editor.visible);
            assert!(editor.ants_task.is_none());
            assert!(second.read(cx).visible);
            editor.tools.ants_phase
        });
        let before = counts(&first, cx);
        cx.executor().advance_clock(Duration::from_millis(800));
        cx.run_until_parked();
        assert_eq!(counts(&first, cx), before, "hidden editor rendered");
        cx.update(|_, cx| assert_eq!(first.read(cx).tools.ants_phase, phase));
        if compact {
            let tab_center = cx.update(|window, _| {
                window
                    .find(("compact-document", first.entity_id()))
                    .bounds()
                    .center()
            });
            cx.simulate_mouse_down(tab_center, MouseButton::Left, Modifiers::none());
            cx.simulate_mouse_up(tab_center, MouseButton::Left, Modifiers::none());
        } else {
            cx.simulate_keystrokes("ctrl-shift-tab");
        }
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(workspace.read(cx).editor.as_ref(), Some(&first));
            let editor = first.read(cx);
            assert!(editor.visible);
            assert!(editor.ants_task.is_some());
            assert!(!second.read(cx).visible);
            assert_eq!(editor.editor.doc, snapshot.0);
            assert_eq!(editor.editor.history.len(), snapshot.1);
            assert_eq!(editor.view, snapshot.2);
            assert_eq!(editor.selected, snapshot.3);
            assert_eq!(editor.tool, Tool::Brush);
        });
        cx.executor().advance_clock(Duration::from_millis(400));
        cx.run_until_parked();
        cx.update(|_, cx| assert_ne!(first.read(cx).tools.ants_phase, phase));
    }
}

#[gpui_kit::test]
fn inactive_tabs_release_images_and_rebuild_on_return(cx: &mut TestAppContext) {
    let (workspace, cx) = open(cx, doc(&["Photo"], None));
    let first = cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap());
    let image = Arc::new(viewport::bgra_image(1, 1, vec![0, 0, 0, 255]));
    let weak_image = Arc::downgrade(&image);
    cx.update(|_, cx| {
        let editor = first.read(cx);
        editor.cache.borrow_mut().insert(
            Key {
                which: Which::Current,
                level: 0,
                x: 99,
                y: 99,
            },
            editor.render_gen,
            image,
        );
        assert!(editor.cache.borrow().resident_image_count() > 0);
    });
    let second = add_tab(&workspace, cx);
    cx.update(|_, cx| {
        let editor = first.read(cx);
        let cache = editor.cache.borrow();
        assert_eq!(cache.resident_image_count(), 0);
        assert_eq!(cache.pending_request_count(), 0);
        assert!(
            cache.to_drop.is_empty(),
            "hidden tab retained deferred image drops"
        );
    });
    assert!(
        weak_image.upgrade().is_none(),
        "hidden tab still owns the released image"
    );
    cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| workspace.activate_tab(0, window, cx))
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert!(
            first.read(cx).cache.borrow().resident_image_count() > 0,
            "resumed canvas did not rebuild its tiles"
        );
        assert_eq!(second.read(cx).cache.borrow().resident_image_count(), 0);
    });
}

#[gpui_kit::test]
fn workspace_pages_suspend_and_resume_the_selected_document(cx: &mut TestAppContext) {
    let (workspace, cx) = open(cx, doc(&["Photo"], None));
    let editor = cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap());
    for screen in [Screen::Home, Screen::Settings] {
        cx.update(|window, cx| {
            if screen == Screen::Home {
                window.dispatch_action(Box::new(actions::ShowHome), cx);
            } else {
                window.dispatch_action(Box::new(actions::ShowSettings), cx);
            }
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(workspace.read(cx).screen, screen);
            assert_eq!(workspace.read(cx).editor.as_ref(), Some(&editor));
            let editor = editor.read(cx);
            assert!(!editor.visible);
            assert!(editor.ants_task.is_none());
            assert_eq!(editor.cache.borrow().resident_image_count(), 0);
        });
        let before = counts(&editor, cx);
        cx.executor().advance_clock(Duration::from_millis(800));
        cx.run_until_parked();
        assert_eq!(counts(&editor, cx), before);
        cx.update(|window, cx| window.dispatch_action(Box::new(actions::ShowEditor), cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(workspace.read(cx).screen, Screen::Editor);
            assert!(editor.read(cx).visible);
        });
    }
}

#[gpui_kit::test]
fn closing_the_active_tab_resumes_only_its_replacement(cx: &mut TestAppContext) {
    let (workspace, cx) = open(cx, doc(&["Photo"], None));
    let first = cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap());
    let second = add_tab(&workspace, cx);
    cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| workspace.close_tab(1, window, cx))
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let workspace = workspace.read(cx);
        assert_eq!(workspace.tabs.len(), 1);
        assert_eq!(workspace.editor.as_ref(), Some(&first));
        assert!(first.read(cx).visible);
        let closed = second.read(cx);
        assert!(!closed.visible);
        assert!(closed.ants_task.is_none());
        assert_eq!(closed.cache.borrow().resident_image_count(), 0);
    });
}

#[gpui_kit::test]
fn hidden_timeline_and_replay_stay_paused_after_return(cx: &mut TestAppContext) {
    for replay in [false, true] {
        let (workspace, cx) = open(cx, doc(&["Frame 1", "Frame 2"], None));
        let editor = cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap());
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.toggle_animation(cx);
                if replay {
                    let id = editor.editor.doc.nodes[0].id;
                    for opacity in [0.8, 0.6, 0.4] {
                        editor.execute(Command::SetOpacity { id, opacity }, cx);
                    }
                    editor.replay_start(cx);
                    assert!(editor.anim.replay.as_ref().unwrap().playing);
                } else {
                    editor.anim_play(true, cx);
                    assert!(editor.anim.playing);
                }
            })
        });
        cx.run_until_parked();
        add_tab(&workspace, cx);
        let paused_frame = cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert!(!editor.visible);
            if replay {
                let replay = editor.anim.replay.as_ref().unwrap();
                assert!(!replay.playing);
                replay.frame
            } else {
                assert!(!editor.anim.playing);
                editor.anim.frame
            }
        });
        let before = counts(&editor, cx);
        cx.executor().advance_clock(Duration::from_secs(2));
        cx.run_until_parked();
        assert_eq!(counts(&editor, cx), before);
        cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert_eq!(
                if replay {
                    editor.anim.replay.as_ref().unwrap().frame
                } else {
                    editor.anim.frame
                },
                paused_frame
            );
        });
        cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| workspace.activate_tab(0, window, cx))
        });
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_secs(2));
        cx.run_until_parked();
        cx.update(|window, cx| {
            let editor = editor.read(cx);
            assert!(editor.visible);
            if replay {
                let replay = editor.anim.replay.as_ref().unwrap();
                assert!(!replay.playing);
                assert_eq!(replay.frame, paused_frame);
                assert!(window.find("replay-overlay").visible());
            } else {
                assert!(!editor.anim.playing);
                assert_eq!(editor.anim.frame, paused_frame);
            }
        });
    }
}

#[gpui_kit::test]
fn stale_tile_batches_cannot_repopulate_hidden_or_reactivated_tabs(cx: &mut TestAppContext) {
    let (workspace, cx) = open(cx, doc(&["Photo"], None));
    let editor = cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap());
    let (epoch, channel, request) = cx.update(|_, cx| {
        let editor = editor.read(cx);
        (
            editor.render_epoch,
            editor.channels.view,
            viewport::Request {
                key: Key {
                    which: Which::Current,
                    level: 0,
                    x: 99,
                    y: 99,
                },
                rev: editor.render_gen,
            },
        )
    });
    add_tab(&workspace, cx);
    cx.update(|_, cx| {
        editor.update(cx, |editor, cx| {
            assert!(!editor.visible);
            editor.install_tile_batch(
                epoch,
                channel,
                vec![(request, vec![0; 256 * 256 * 4])],
                Duration::from_millis(1),
                cx,
            );
            assert_eq!(editor.cache.borrow().resident_image_count(), 0);
            assert_eq!(editor.cache.borrow().pending_request_count(), 0);
        });
    });
    cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| workspace.activate_tab(0, window, cx));
        editor.update(cx, |editor, cx| {
            assert!(editor.visible);
            assert_ne!(editor.render_epoch, epoch);
            let before = editor.cache.borrow().resident_image_count();
            // A completion from before suspension must not release the newer
            // worker's in-flight marker after a quick switch back.
            editor.cache.borrow_mut().in_flight = true;
            editor.install_tile_batch(
                epoch,
                channel,
                vec![(request, vec![0; 256 * 256 * 4])],
                Duration::from_millis(1),
                cx,
            );
            {
                let cache = editor.cache.borrow();
                assert_eq!(cache.resident_image_count(), before);
                assert!(cache.in_flight);
            }
            editor.cache.borrow_mut().in_flight = false;
        });
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert!(editor.read(cx).cache.borrow().resident_image_count() > 0);
    });
}

#[gpui_kit::test]
fn closing_during_a_shape_gesture_prompts_before_discarding_the_edit(cx: &mut TestAppContext) {
    let (workspace, cx) = open(cx, doc(&["Photo"], None));
    let editor = cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_tool(Tool::Shape, cx);
            assert!(!editor.has_unsaved_changes());
        })
    });
    cx.run_until_parked();
    let (start, end) = cx.update(|_, cx| {
        let editor = editor.read(cx);
        (
            editor.doc_to_window((40., 40.)).unwrap(),
            editor.doc_to_window((140., 120.)).unwrap(),
        )
    });
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
    cx.update(|_, cx| {
        assert_eq!(editor.read(cx).editor.doc.nodes.len(), 1);
        assert!(!editor.read(cx).has_unsaved_changes());
    });
    cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| workspace.close_tab(0, window, cx))
    });
    cx.run_until_parked();
    assert!(
        cx.has_pending_prompt(),
        "closing must prompt for the completed gesture"
    );
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    cx.update(|_, cx| {
        assert_eq!(workspace.read(cx).tabs.len(), 1);
        assert_eq!(workspace.read(cx).editor.as_ref(), Some(&editor));
        let editor = editor.read(cx);
        assert!(editor.visible);
        assert!(editor.has_unsaved_changes());
        assert_eq!(editor.editor.doc.nodes.len(), 2);
        assert_eq!(
            editor.editor.history.len(),
            1,
            "gesture should commit exactly once"
        );
    });
}
