//! Photo's familiar shell exposes real editing controls without changing the
//! document or the layouts used by the other workspaces.
use super::*;
use crate::editor::{EditorView, PaintKind, SelectShape, SidebarTab, SliderKey, Tool};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{px, size};

fn setup(
    cx: &mut TestAppContext,
    width: f32,
    height: f32,
) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (workspace, cx) = open(cx, doc(&["Photo"], None));
    cx.simulate_resize(size(px(width), px(height)));
    let editor = cx.update(|window, cx| {
        cx.global_mut::<AppSettings>().0.compact_chrome = true;
        window.refresh();
        workspace.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    (editor, cx)
}

#[gpui_kit::test]
fn photo_options_span_the_window_above_document_tabs(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, 1280., 900.);
    cx.update(|window, cx| {
        let options = window.find("canvas-toolbar-options").bounds();
        let tabs = window.find("document-tab-bar").bounds();
        let panel = window.find("node-panel").bounds();
        let canvas = window.find("canvas").bounds();
        assert!(options.bottom() <= tabs.top() + px(1.));
        assert!(options.right() >= panel.right() - px(1.));
        assert!(tabs.right() <= panel.left() + px(1.));
        assert!(panel.top() <= tabs.top() + px(1.));
        assert!(tabs.bottom() <= canvas.top());
        assert!(editor.read(cx).editor.history.is_empty());
    });
}

#[gpui_kit::test]
fn photo_brush_options_prioritize_mode_opacity_and_flow_with_reachable_overflow(
    cx: &mut TestAppContext,
) {
    let (editor, cx) = setup(cx, 1280., 900.);
    let original = cx.update(|window, cx| {
        editor.update(cx, |e, cx| {
            e.set_paint(PaintKind::Brush, cx);
            window.focus(&e.canvas_focus, cx);
        });
        editor.read(cx).editor.doc.clone()
    });
    cx.run_until_parked();
    cx.update(|window, _| {
        let bar = window.find("canvas-toolbar-options").bounds();
        let controls = ["ToolSize", "brush-blend-mode", "ToolOpacity", "ToolFlow"].map(|id| {
            assert!(window.find(id).visible(), "{id}");
            window.find(id).bounds()
        });
        assert!(
            controls
                .windows(2)
                .all(|pair| pair[0].right() <= pair[1].left())
        );
        assert!(
            controls
                .iter()
                .all(|bounds| bounds.top() >= bar.top() && bounds.bottom() <= bar.bottom())
        );
    });
    cx.simulate_resize(size(px(640.), px(700.)));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("tool-options-more", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("ToolFlow").visible());
        let flow = window.find("ToolFlow").bounds();
        window.drag(
            flow.center(),
            flow.origin + gpui_kit::point(flow.size.width * 0.75, flow.size.height / 2.),
            cx,
        );
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert!(e.tools.brush.flow > 0.65 && e.tools.brush.flow < 0.85);
        assert_eq!(e.editor.doc, original);
        assert!(e.editor.history.is_empty());
    });
    // Resizing an open More popup must move controls back to the header
    // without leaving a second slider registered against the same track.
    cx.simulate_resize(size(px(1280.), px(900.)));
    cx.run_until_parked();
    cx.update(|window, _| {
        assert!(
            window
                .within("editor-tool-options")
                .find("ToolFlow")
                .visible()
        );
        assert!(window.find("tool-options-overflow-content").visible());
        assert!(
            window
                .within("tool-options-overflow-content")
                .try_find("ToolFlow")
                .is_none()
        );
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("tool-options-overflow-content").is_none());
        editor.update(cx, |e, cx| e.set_paint(PaintKind::Eraser, cx));
        editor.update(cx, |e, cx| e.set_paint(PaintKind::Brush, cx));
        assert!(editor.read(cx).tools.brush.flow > 0.65);
    });
}

#[gpui_kit::test]
fn photo_options_return_focus_to_canvas_when_resize_removes_more(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, 500., 700.);
    let original = cx.update(|window, cx| {
        editor.update(cx, |e, cx| {
            e.set_hand_mode(false, cx);
            window.focus(&e.canvas_focus, cx);
        });
        editor.read(cx).editor.doc.clone()
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("tool-options-more", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("tool-options-overflow-content").visible());
        assert!(
            window
                .within("tool-options-overflow-content")
                .find("reset-view-rotation")
                .visible()
        );
        window.press("tab", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| assert!(!editor.read(cx).canvas_focus.is_focused(window)));
    cx.simulate_resize(size(px(1280.), px(900.)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("tool-options-more").is_none());
        assert!(window.try_find("tool-options-overflow-content").is_none());
        assert!(editor.read(cx).canvas_focus.is_focused(window));
    });
    cx.simulate_keystrokes("b");
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.tool, Tool::Brush);
        assert_eq!(e.editor.doc, original);
        assert!(e.editor.history.is_empty());
    });
}

#[gpui_kit::test]
fn photo_selection_modes_and_feather_lead_subtool_shortcuts(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, 900., 750.);
    let original = cx.update(|_, cx| {
        editor.update(cx, |e, cx| e.set_select(SelectShape::Rect, cx));
        editor.read(cx).editor.doc.clone()
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("photo-selection-operations").visible());
        assert!(window.find("Feather").visible());
        window.click("cm-add", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(
            editor.read(cx).tools.combine,
            emulsion_raster::select::Combine::Add
        );
        window.click("tool-options-more", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("sel-ell", cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.tools.select, SelectShape::Ellipse);
        assert_eq!(e.editor.doc, original);
        assert!(e.editor.history.is_empty());
    });
}

#[gpui_kit::test]
fn photo_selection_options_fit_narrow_scaled_and_translated_headers(cx: &mut TestAppContext) {
    struct RestoreLocale(String);
    impl Drop for RestoreLocale {
        fn drop(&mut self) {
            rust_i18n::set_locale(&self.0);
        }
    }
    let _locale = RestoreLocale(rust_i18n::locale().to_string());
    let (editor, cx) = setup(cx, 640., 800.);
    cx.update(|_, cx| editor.update(cx, |e, cx| e.set_select(SelectShape::Rect, cx)));
    for (width, rem, scale, locale) in [
        (640., 16., 1., "en"),
        (480., 20., 1.25, "de"),
        (900., 20., 1.5, "fr"),
    ] {
        rust_i18n::set_locale(locale);
        cx.simulate_resize(size(px(width), px(800.)));
        cx.update(|window, cx| {
            window.set_rem_size(px(rem));
            editor.update(cx, |e, cx| {
                let mut layout = e.workspace_snapshot();
                layout
                    .toolbar_placements
                    .iter_mut()
                    .find(|bar| bar.id == "options")
                    .unwrap()
                    .scale = scale;
                e.apply_workspace_layout(&layout, cx);
            });
            window.refresh();
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            let bar = window.find("canvas-toolbar-options").bounds();
            for id in [
                "tool-options-more",
                "toolbar-close-options",
                "photo-selection-operations",
            ] {
                if let Some(control) = window.try_find(id) {
                    let b = control.bounds();
                    assert!(
                        b.left() >= bar.left() && b.right() <= bar.right() + px(1.),
                        "{locale}: {id}: {b:?}, bar {bar:?}"
                    );
                }
            }
            window.click("tool-options-more", cx);
        });
        cx.run_until_parked();
        cx.update(|window, _| {
            let more = window.find("tool-options-overflow-content").bounds();
            assert!(more.left() >= px(0.) && more.right() <= px(width));
        });
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn photo_adjustment_properties_open_on_live_parameters_and_keep_single_step_undo(
    cx: &mut TestAppContext,
) {
    let (editor, cx) = setup(cx, 1280., 900.);
    let (id, key) = cx.update(|_, cx| {
        editor.update(cx, |e, cx| e.quick_adjust("exposure", cx));
        let e = editor.read(cx);
        let id = e.selected.unwrap();
        let emulsion_core::NodeKind::Adjust(adjustment) = &e.editor.doc.node(id).unwrap().kind
        else {
            panic!("adjustment")
        };
        (id, adjustment.params()[0].key)
    });
    cx.run_until_parked();
    let before = cx.update(|window, cx| {
        assert!(editor.read(cx).sidebar_tab == SidebarTab::Properties);
        assert!(window.find("photo-adjustment-properties").visible());
        assert!(window.try_find("photo-layer-details").is_none());
        let slider = window.find(gpui_kit::SharedString::from(format!(
            "{:?}",
            SliderKey::Param(id, key)
        )));
        assert!(slider.visible());
        (
            editor.read(cx).editor.doc.clone(),
            editor.read(cx).editor.history.len(),
            slider.bounds(),
        )
    });
    cx.update(|window, cx| {
        window.drag(
            before.2.center(),
            before.2.origin
                + gpui_kit::point(before.2.size.width * 0.75, before.2.size.height / 2.),
            cx,
        )
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(editor.read(cx).editor.history.len(), before.1 + 1);
        assert_ne!(editor.read(cx).editor.doc, before.0);
        editor.update(cx, |e, cx| e.undo(cx));
        assert_eq!(editor.read(cx).editor.doc, before.0);
    });
}

#[gpui_kit::test]
fn photo_palette_preserves_user_accent_and_leaves_paint_unchanged(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, 1280., 900.);
    cx.update(|_, cx| {
        let base = theme::palette(cx);
        let photo = editor.read(cx).workspace_palette(cx);
        assert_eq!(photo.accent, base.accent);
        assert_eq!(photo.dark, base.dark);
        assert_ne!(photo.panel, base.panel);
        editor.update(cx, |e, _| e.draw_mode = true);
        assert!(!editor.read(cx).is_photo_workflow());
        assert_eq!(editor.read(cx).workspace_palette(cx), base);
        editor.update(cx, |e, _| {
            e.draw_mode = false;
            e.library_only = true;
        });
        assert!(!editor.read(cx).is_photo_workflow());
        assert_eq!(editor.read(cx).workspace_palette(cx), base);
    });
}
