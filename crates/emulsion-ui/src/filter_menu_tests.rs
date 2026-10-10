//! Image and Filter menus use real commands, and filter rendering is one undo.
use super::*;
use crate::editor::EditorView;
use emulsion_core::NodeKind;
use emulsion_filters::Filter;
use gpui_kit::test::TestWindowExt;

fn setup(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    let d = doc(
        &["Photo"],
        Some(Raster::from_fn(24, 24, [0; 4], |x, _| {
            if x < 12 {
                [65535, 0, 0, 65535]
            } else {
                [0, 0, 65535, 65535]
            }
        })),
    );
    let id = d.nodes[0].id;
    let (ws, cx) = open(cx, d);
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| editor.update(cx, |e, _| e.selected = Some(id)));
    cx.run_until_parked();
    (editor, cx)
}

#[gpui_kit::test]
fn filter_menu_blur_converts_and_filters_in_one_undo(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    cx.update(|window, cx| {
        window.click("filter-menu", cx);
        assert_eq!(
            window.within("popup-menu").find(0usize).label(),
            Some("Repeat last filter")
        );
        assert_eq!(
            window.within("popup-menu").find(2usize).label(),
            Some("Blur")
        );
        window.within("popup-menu").hover(2usize, cx);
        window.press("right", cx);
        assert_eq!(
            window.within("submenu").find(0usize).label(),
            Some("Gaussian blur")
        );
        window.press("enter", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            let NodeKind::Smart {
                filters,
                source,
                cache,
                ..
            } = &e.editor.doc.nodes[0].kind
            else {
                panic!("smart filter layer")
            };
            assert_eq!(filters, &[Filter::GaussianBlur { radius: 5. }]);
            assert_ne!(
                source.get(12, 12),
                cache.get(12, 12),
                "filter changes pixels"
            );
            assert_eq!(
                e.editor.history.len(),
                1,
                "conversion and filter share one undo"
            );
            e.undo(cx);
            assert_eq!(e.editor.doc, before);
        })
    });
}

#[gpui_kit::test]
fn invert_menu_converts_repeats_and_undoes_without_parameter_controls(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    cx.update(|window, cx| {
        window.click("filter-menu", cx);
        assert_eq!(
            window.within("popup-menu").find(7usize).label(),
            Some("Other")
        );
        window.within("popup-menu").hover(7usize, cx);
        window.press("right", cx);
        assert_eq!(
            window.within("submenu").find(1usize).label(),
            Some("Invert")
        );
        window.within("submenu").click(1usize, cx);
    });
    cx.run_until_parked();
    let inverted = cx.update(|_, cx| {
        let e = editor.read(cx);
        let NodeKind::Smart {
            filters,
            source,
            cache,
            offset,
            ..
        } = &e.editor.doc.nodes[0].kind
        else {
            panic!("smart filter layer")
        };
        assert_eq!(filters, &[Filter::Invert]);
        assert!(filters[0].params().is_empty());
        assert_eq!(*offset, (0, 0));
        assert_eq!(cache.get(0, 0), [0, 65535, 65535, 65535]);
        assert_eq!(source.get(0, 0), [65535, 0, 0, 65535]);
        assert_eq!(e.editor.history.len(), 1);
        e.editor.doc.clone()
    });
    cx.update(|_, cx| editor.update(cx, |e, cx| e.repeat_last_filter(cx)));
    cx.run_until_parked();
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            let NodeKind::Smart {
                filters,
                source,
                cache,
                ..
            } = &e.editor.doc.nodes[0].kind
            else {
                panic!()
            };
            assert_eq!(filters, &[Filter::Invert, Filter::Invert]);
            assert_eq!(
                cache.read_rect(cache.bounds()),
                source.read_rect(source.bounds())
            );
            e.undo(cx);
            assert_eq!(e.editor.doc, inverted);
            e.undo(cx);
            assert_eq!(e.editor.doc, before);
        })
    });
}

#[gpui_kit::test]
fn repeat_filter_shortcut_retains_tuned_parameters_and_is_undoable(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    cx.update(|_, cx| editor.update(cx, |e, cx| e.quick_filter("Gaussian blur", cx)));
    cx.run_until_parked();
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            let id = e.selected.unwrap();
            e.set_filter_param(id, 0, "radius", 2., true, cx);
        })
    });
    cx.run_until_parked();
    let before = cx.update(|window, cx| {
        let e = editor.read(cx);
        let before = e.editor.doc.clone();
        let focus = e.canvas_focus.clone();
        window.focus(&focus, cx);
        before
    });
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-alt-f"
    } else {
        "ctrl-alt-f"
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            let NodeKind::Smart { filters, .. } = &e.editor.doc.nodes[0].kind else {
                panic!("smart layer")
            };
            assert_eq!(
                filters,
                &[
                    Filter::GaussianBlur { radius: 2. },
                    Filter::GaussianBlur { radius: 2. }
                ]
            );
            e.undo(cx);
            assert_eq!(e.editor.doc, before);
        })
    });
}

#[gpui_kit::test]
fn image_adjustments_menu_adds_editable_adjustment_with_undo(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    cx.update(|window, cx| {
        window.click("image-menu", cx);
        window.within("popup-menu").hover(0usize, cx);
        window.press("right", cx);
        assert_eq!(
            window.within("submenu").find(0usize).label(),
            Some("Exposure")
        );
        window.press("enter", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            assert_eq!(e.editor.doc.nodes.len(), 2);
            assert!(matches!(
                e.editor.doc.node(e.selected.unwrap()).unwrap().kind,
                NodeKind::Adjust(_)
            ));
            assert_eq!(e.editor.history.len(), 1);
            e.undo(cx);
            assert_eq!(e.editor.doc, before);
        })
    });
}

#[gpui_kit::test]
fn image_blend_profiles_are_distinct_discoverable_and_undoable(cx: &mut TestAppContext) {
    use emulsion_raster::blend::BlendSpace;
    let (editor, cx) = setup(cx);
    let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    assert_eq!(before.blend_space, BlendSpace::Linear);
    assert_eq!(before.psd_background, None);
    for (index, space) in [
        (0usize, BlendSpace::Srgb),
        (2usize, BlendSpace::PhotoshopSrgbV1),
    ] {
        cx.update(|window, cx| {
            window.click("image-menu", cx);
            assert_eq!(
                window.within("popup-menu").find(6usize).label(),
                Some("Blend space")
            );
            window.within("popup-menu").hover(6usize, cx);
            window.press("right", cx);
            for (index, label) in ["sRGB (legacy)", "Linear light", "PSD-compatible sRGB v1"]
                .into_iter()
                .enumerate()
            {
                assert_eq!(window.within("submenu").find(index).label(), Some(label));
            }
            window.within("submenu").click(index, cx);
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                assert_eq!(e.editor.doc.blend_space, space);
                assert_eq!(e.editor.doc.psd_background, None);
                assert_eq!(e.editor.doc.nodes, before.nodes);
                assert_eq!(e.editor.history.len(), 1);
                let after = e.editor.doc.clone();
                e.undo(cx);
                assert_eq!(e.editor.doc, before);
                e.redo(cx);
                assert_eq!(e.editor.doc, after);
                e.undo(cx);
            })
        });
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn image_background_menu_sets_clears_and_undoes_explicit_identity(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    let id = before.nodes[0].id;
    cx.update(|window, cx| {
        window.click("image-menu", cx);
        assert_eq!(
            window.within("popup-menu").find(8usize).label(),
            Some("Set PSD Background")
        );
        assert_eq!(
            window.within("popup-menu").find(9usize).label(),
            Some("Clear PSD Background")
        );
        // Clearing without an assignment is disabled.
        window.within("popup-menu").click(9usize, cx);
        assert_eq!(editor.read(cx).editor.doc, before);
        assert_eq!(editor.read(cx).editor.history.len(), 0);
        window.press("escape", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.click("image-menu", cx);
        window.within("popup-menu").click(8usize, cx);
    });
    cx.run_until_parked();
    let assigned = cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            assert_eq!(e.editor.doc.psd_background, Some(id));
            assert_eq!(e.editor.doc.blend_space, before.blend_space);
            assert_eq!(e.editor.doc.nodes, before.nodes);
            assert_eq!(e.editor.history.len(), 1);
            let assigned = e.editor.doc.clone();
            e.undo(cx);
            assert_eq!(e.editor.doc, before);
            e.redo(cx);
            assert_eq!(e.editor.doc, assigned);
            assigned
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.click("image-menu", cx);
        // Reassigning the same identity is disabled, with no new undo step.
        window.within("popup-menu").click(8usize, cx);
        assert_eq!(editor.read(cx).editor.doc, assigned);
        assert_eq!(editor.read(cx).editor.history.len(), 1);
        window.press("escape", cx);
        editor.update(cx, |e, cx| {
            e.selected = None;
            cx.notify();
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.click("image-menu", cx);
        // Clear works without selecting the Background layer.
        window.within("popup-menu").click(9usize, cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            assert_eq!(e.editor.doc, before);
            assert_eq!(e.editor.history.len(), 2);
            e.undo(cx);
            assert_eq!(e.editor.doc, assigned);
            e.redo(cx);
            assert_eq!(e.editor.doc, before);
        });
    });
}

#[gpui_kit::test]
fn filters_reject_locked_layers_and_pending_results_after_undo(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            let id = e.selected.unwrap();
            e.editor.doc.node_mut(id).unwrap().locked = true;
            let before = e.editor.doc.clone();
            e.quick_filter("Gaussian blur", cx);
            assert_eq!(e.editor.doc, before);
            assert_eq!(e.editor.history.len(), 0);
            e.editor.doc.node_mut(id).unwrap().locked = false;
            e.quick_filter("Gaussian blur", cx);
            e.undo(cx);
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert!(matches!(
            e.editor.doc.nodes[0].kind,
            NodeKind::Raster { .. }
        ));
        assert_eq!(e.editor.history.len(), 0);
    });
}

#[gpui_kit::test]
fn image_and_filter_menus_fit_compact_window(cx: &mut TestAppContext) {
    let (_, cx) = setup(cx);
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(800.), gpui_kit::px(600.)));
    cx.run_until_parked();
    cx.update(|window, _| {
        for id in ["image-menu", "filter-menu", "doc-size"] {
            let control = window.find(id);
            assert!(control.visible());
            assert!(f32::from(control.bounds().right()) <= 800.);
        }
    });
}

#[gpui_kit::test]
fn filter_menu_liquify_opens_existing_pixel_tool(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx);
    let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    cx.update(|window, cx| {
        window.click("filter-menu", cx);
        assert_eq!(
            window.within("popup-menu").find(15usize).label(),
            Some("Liquify…")
        );
        window.within("popup-menu").click(15usize, cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.tool, crate::editor::Tool::Brush);
        assert_eq!(e.tools.paint, crate::editor::PaintKind::Liquify);
        assert_eq!(e.editor.doc, before);
    });
}

#[gpui_kit::test]
fn app_icon_and_name_lead_the_menu_row(cx: &mut TestAppContext) {
    let (_, cx) = setup(cx);
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1280.), gpui_kit::px(800.)));
    cx.run_until_parked();
    cx.update(|window, _| {
        let app = window.find("app-menu").bounds();
        let file = window.find("file-menu").bounds();
        assert!(app.right() <= file.left(), "{app:?} before {file:?}");
    });
}
