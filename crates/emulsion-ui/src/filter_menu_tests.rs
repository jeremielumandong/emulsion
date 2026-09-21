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
        "cmd-f"
    } else {
        "ctrl-f"
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
            window.within("popup-menu").find(12usize).label(),
            Some("Liquify…")
        );
        window.within("popup-menu").click(12usize, cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.tool, crate::editor::Tool::Brush);
        assert_eq!(e.tools.paint, crate::editor::PaintKind::Liquify);
        assert_eq!(e.editor.doc, before);
    });
}
