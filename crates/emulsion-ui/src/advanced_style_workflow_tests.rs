//! Effect rows, advanced options and staged color changes through the inspector.
use super::*;
use emulsion_core::style_options::{GradientStop, StyleOptions};
use emulsion_core::styles::LayerStyle;
use gpui_kit::Focusable;
use gpui_kit::component::WindowExt;
use gpui_kit::test::TestWindowExt;

fn styled_doc() -> Document {
    let mut doc = Document::new(80, 60);
    let mut node = Node::new(
        1,
        "Shape",
        emulsion_core::NodeKind::Fill {
            rgba: [210, 130, 40, 255],
        },
    );
    node.styles = vec![
        LayerStyle::ColorOverlay {
            color: [100, 20, 30],
            opacity: 100.,
        },
        LayerStyle::Stroke {
            color: [0, 0, 0],
            opacity: 100.,
            size: 3.,
        },
    ];
    node.style_options = vec![
        StyleOptions {
            id: 10,
            noise: 12.,
            ..Default::default()
        },
        StyleOptions {
            id: 20,
            blend: emulsion_raster::BlendMode::Multiply,
            ..Default::default()
        },
    ];
    doc.nodes.push(node);
    doc
}
fn click(cx: &mut VisualTestContext, id: impl Into<gpui_kit::ElementId>) {
    let id = id.into();
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        window.click(id, cx);
    });
    cx.run_until_parked();
}

#[gpui_kit::test]
fn native_style_dialog_close_cancels_preview_and_allows_reopening(cx: &mut TestAppContext) {
    let original = styled_doc();
    let (ws, cx) = open(cx, original.clone());
    let view = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    for height in [720., 1500.] {
        cx.simulate_resize(gpui_kit::size(gpui_kit::px(1000.), gpui_kit::px(height)));
        for _ in 0..2 {
            cx.update(|window, cx| {
                view.update(cx, |e, cx| e.open_blending_options(1, window, cx));
                assert!(window.has_active_dialog(cx));
            });
            click(cx, "style-dialog-enabled-10");
            cx.update(|_, cx| {
                let e = view.read(cx);
                assert!(!e.editor.doc.nodes[0].style_options[0].enabled);
                assert!(e.editor.in_transaction());
                assert!(e.editor.history.is_empty());
            });
            let close = cx.update(|window, cx| {
                window.render_frame(cx);
                window.find("close").bounds().center()
            });
            cx.simulate_mouse_down(close, gpui_kit::MouseButton::Left, Default::default());
            cx.simulate_mouse_up(close, gpui_kit::MouseButton::Left, Default::default());
            cx.run_until_parked();
            cx.update(|window, cx| {
                assert!(!window.has_active_dialog(cx));
                let e = view.read(cx);
                assert!(e.styles_ui.dialog_for.is_none());
                assert!(!e.editor.in_transaction());
                assert!(e.editor.history.is_empty());
                assert_eq!(e.editor.doc, original);
            });
        }
    }
}

#[gpui_kit::test]
fn effect_enable_reorder_and_undo_keep_options_attached_to_stable_effect(cx: &mut TestAppContext) {
    let original = styled_doc();
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1000.), gpui_kit::px(1500.)));
    let view = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| {
        view.update(cx, |e, cx| e.open_blending_options(1, window, cx));
        assert!(window.has_active_dialog(cx));
        assert_eq!(view.read(cx).styles_ui.dialog_for, Some(1));
        window.render_frame(cx);
    });
    cx.run_until_parked();
    click(cx, "style-dialog-enabled-10");
    cx.update(|_, cx| {
        let e = view.read(cx);
        assert!(!e.editor.doc.nodes[0].style_options[0].enabled);
        assert_eq!(e.editor.history.len(), 0);
    });
    click(cx, "style-dialog-ok");
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, original));
    cx.update(|window, cx| {
        view.update(cx, |e, cx| e.open_blending_options(1, window, cx));
        assert!(window.has_active_dialog(cx));
        assert_eq!(view.read(cx).styles_ui.dialog_for, Some(1));
        window.render_frame(cx);
    });
    cx.run_until_parked();
    click(cx, "style-dialog-effect-10");
    click(cx, "style-1-10-down");
    cx.update(|_, cx| {
        let e = view.read(cx);
        let node = &e.editor.doc.nodes[0];
        assert_eq!(node.style_options[1].id, 10);
        assert_eq!(node.style_options[1].noise, 12.);
        assert_eq!(node.styles[1], original.nodes[0].styles[0]);
        assert_eq!(node.kind, original.nodes[0].kind);
    });
    click(cx, "style-dialog-cancel");
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, original));
}

#[gpui_kit::test]
fn native_effect_color_previews_in_dialog_and_cancel_or_ok_resolves_one_transaction(
    cx: &mut TestAppContext,
) {
    let original = styled_doc();
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1000.), gpui_kit::px(1200.)));
    let view = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    for commit in [false, true] {
        let at = cx.update(|window, cx| {
            window.render_frame(cx);
            let bounds = window.find(("row", 1u64)).bounds();
            gpui_kit::point(bounds.right() - gpui_kit::px(6.), bounds.center().y)
        });
        cx.simulate_event(gpui_kit::MouseDownEvent {
            button: gpui_kit::MouseButton::Left,
            position: at,
            click_count: 2,
            ..Default::default()
        });
        cx.simulate_event(gpui_kit::MouseUpEvent {
            button: gpui_kit::MouseButton::Left,
            position: at,
            click_count: 2,
            ..Default::default()
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.has_active_dialog(cx));
            assert_eq!(view.read(cx).styles_ui.dialog_for, Some(1));
            window.render_frame(cx);
            let left = window.find("style-dialog-effects").bounds();
            let right = window.find("style-dialog-settings").bounds();
            assert!(left.right() <= right.left());
        });
        click(cx, "style-dialog-effect-10");
        let picker = cx.update(|_, cx| {
            view.read(cx)
                .styles_ui
                .colors
                .get(&(1, 10, 0))
                .unwrap()
                .state
                .clone()
        });
        click(cx, "style-1-10-picker-0");
        cx.update(|window, cx| {
            let input = picker.read(cx).hex_input().clone();
            window.focus(&input.read(cx).focus_handle(cx), cx);
        });
        cx.simulate_keystrokes("ctrl-a");
        cx.simulate_input("#22bb44");
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = view.read(cx);
            assert_eq!(e.editor.doc.nodes[0].styles[0].colors()[0], [34, 187, 68]);
            assert_eq!(e.editor.history.len(), 0);
            assert!(e.editor.in_transaction());
        });
        click(cx, "style-color-ok");
        cx.update(|_, cx| {
            let e = view.read(cx);
            assert!(e.styles_ui.color_dialog_for.is_none());
            assert!(e.editor.in_transaction());
            assert_eq!(e.editor.history.len(), 0);
        });
        click(
            cx,
            if commit {
                "style-dialog-ok"
            } else {
                "style-dialog-cancel"
            },
        );
        cx.update(|_, cx| assert!(!view.read(cx).editor.in_transaction()));
        if commit {
            cx.update(|_, cx| assert_eq!(view.read(cx).editor.history.len(), 1));
            cx.simulate_keystrokes("ctrl-z");
            cx.run_until_parked();
        }
        cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, original));
    }
}

#[gpui_kit::test]
fn modal_effect_slider_drag_preserves_outer_transaction_until_cancel(cx: &mut TestAppContext) {
    let original = styled_doc();
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1000.), gpui_kit::px(1200.)));
    let view = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| {
        view.update(cx, |e, cx| e.open_blending_options(1, window, cx));
        assert!(window.has_active_dialog(cx));
        assert_eq!(view.read(cx).styles_ui.dialog_for, Some(1));
        window.render_frame(cx);
    });
    cx.run_until_parked();
    click(cx, "style-dialog-effect-10");
    let (from, to) = cx.update(|window, cx| {
        window.render_frame(cx);
        let bounds = window.find("Style(1, 0, \"opacity\")").bounds();
        (
            gpui_kit::point(bounds.left() + bounds.size.width * 0.7, bounds.center().y),
            gpui_kit::point(bounds.left() + bounds.size.width * 0.3, bounds.center().y),
        )
    });
    cx.simulate_mouse_down(from, gpui_kit::MouseButton::Left, Default::default());
    cx.simulate_mouse_move(to, Some(gpui_kit::MouseButton::Left), Default::default());
    cx.simulate_mouse_up(to, gpui_kit::MouseButton::Left, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = view.read(cx);
        let LayerStyle::ColorOverlay { opacity, .. } = e.editor.doc.nodes[0].styles[0] else {
            panic!("color overlay");
        };
        assert!(opacity > 20. && opacity < 40., "dragged opacity: {opacity}");
        assert!(!e.has_active_gesture());
        assert!(e.editor.in_transaction());
        assert_eq!(e.editor.history.len(), 0);
    });
    click(cx, "style-dialog-cancel");
    cx.update(|_, cx| {
        let e = view.read(cx);
        assert_eq!(e.editor.doc, original);
        assert!(!e.editor.in_transaction());
    });
}

#[gpui_kit::test]
fn style_copy_paste_and_saved_default_retain_advanced_gradient_options(cx: &mut TestAppContext) {
    let mut original = styled_doc();
    original.nodes[0].style_options[0].gradient.stops = vec![
        GradientStop {
            position: 0.,
            color: [10, 20, 30, 40],
        },
        GradientStop {
            position: 1.,
            color: [50, 60, 70, 80],
        },
    ];
    original.nodes.push(Node::new(
        2,
        "Target",
        emulsion_core::NodeKind::Fill { rgba: [255; 4] },
    ));
    let (ws, cx) = open(cx, original.clone());
    let view = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.set_layer_selection(vec![1], Some(1));
            e.copy_layer_style(cx);
            e.save_style_default(1, 0, cx);
            e.set_layer_selection(vec![2], Some(2));
            e.paste_layer_style(cx);
            assert_eq!(
                e.editor.doc.node(2).unwrap().style_options,
                original.nodes[0].style_options
            );
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
            e.add_style(
                2,
                LayerStyle::ColorOverlay {
                    color: [0; 3],
                    opacity: 100.,
                },
                cx,
            );
            assert_eq!(
                e.editor.doc.node(2).unwrap().style_options[0]
                    .gradient
                    .stops,
                original.nodes[0].style_options[0].gradient.stops
            );
        })
    });
}

fn drag_dialog_title(cx: &mut VisualTestContext, layer: usize, dx: f32, dy: f32) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
    });
    let surface = ["dialog-0", "dialog-1"][layer];
    let title = ["dialog-drag-handle-0", "dialog-drag-handle-1"][layer];
    let before = cx.debug_bounds(surface).unwrap();
    let start = cx.debug_bounds(title).unwrap().center();
    let delta = gpui_kit::point(gpui_kit::px(dx), gpui_kit::px(dy));
    let end = start + delta;
    cx.simulate_mouse_down(start, gpui_kit::MouseButton::Left, Default::default());
    cx.simulate_mouse_move(end, Some(gpui_kit::MouseButton::Left), Default::default());
    cx.simulate_mouse_up(end, gpui_kit::MouseButton::Left, Default::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.render_frame(cx);
    });
    assert_eq!(
        cx.debug_bounds(surface).unwrap().origin,
        before.origin + delta
    );
}

#[gpui_kit::test]
fn moving_style_and_color_dialogs_preserves_preview_and_nested_cancel_boundary(
    cx: &mut TestAppContext,
) {
    let original = styled_doc();
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1200.), gpui_kit::px(1200.)));
    let view = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    for escape in [false, true] {
        cx.update(|window, cx| view.update(cx, |e, cx| e.open_blending_options(1, window, cx)));
        drag_dialog_title(cx, 0, 35., 75.);
        click(cx, "style-dialog-enabled-20");
        click(cx, "style-dialog-effect-10");
        let picker = cx.update(|_, cx| {
            view.read(cx)
                .styles_ui
                .colors
                .get(&(1, 10, 0))
                .unwrap()
                .state
                .clone()
        });
        click(cx, "style-1-10-picker-0");
        let parent = cx.debug_bounds("dialog-0").unwrap();
        drag_dialog_title(cx, 1, 80., 70.);
        assert_eq!(cx.debug_bounds("dialog-0").unwrap().origin, parent.origin);
        cx.update(|window, cx| {
            let input = picker.read(cx).hex_input().clone();
            window.focus(&input.read(cx).focus_handle(cx), cx);
        });
        cx.simulate_keystrokes("ctrl-a");
        cx.simulate_input("#22bb44");
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = view.read(cx);
            assert_eq!(e.editor.doc.nodes[0].styles[0].colors()[0], [34, 187, 68]);
            assert_eq!(e.editor.history.len(), 0);
            assert!(e.editor.in_transaction());
            assert_eq!(e.styles_ui.color_dialog_for, Some((1, 10, 0)));
        });
        if escape {
            cx.simulate_keystrokes("escape");
            cx.run_until_parked();
        } else {
            click(cx, "style-color-cancel");
        }
        cx.update(|window, cx| {
            assert!(window.has_active_dialog(cx));
            let e = view.read(cx);
            assert!(e.styles_ui.color_dialog_for.is_none());
            assert_eq!(e.styles_ui.dialog_for, Some(1));
            assert!(e.editor.in_transaction());
            assert_eq!(e.editor.doc.nodes[0].styles[0].colors()[0], [100, 20, 30]);
            assert!(
                !e.editor.doc.nodes[0].style_options[1].enabled,
                "outer effect change survives nested cancellation"
            );
            assert_eq!(e.editor.history.len(), 0);
        });
        click(cx, "style-dialog-cancel");
        cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, original));
    }
}

#[gpui_kit::test]
fn switching_tabs_cancels_both_style_dialog_layers_and_restores_original(cx: &mut TestAppContext) {
    let original = styled_doc();
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1200.), gpui_kit::px(1200.)));
    let source = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| {
        ws.update(cx, |w, cx| {
            w.install(
                Document::new(20, 20),
                None,
                None,
                None,
                "Other tab".into(),
                window,
                cx,
            )
        });
        ws.update(cx, |w, cx| w.activate_tab(0, window, cx));
        source.update(cx, |e, cx| e.open_blending_options(1, window, cx));
    });
    click(cx, "style-dialog-enabled-20");
    click(cx, "style-dialog-effect-10");
    click(cx, "style-1-10-picker-0");
    let picker = cx.update(|_, cx| {
        source
            .read(cx)
            .styles_ui
            .colors
            .get(&(1, 10, 0))
            .unwrap()
            .state
            .clone()
    });
    cx.update(|window, cx| {
        let input = picker.read(cx).hex_input().clone();
        window.focus(&input.read(cx).focus_handle(cx), cx);
    });
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("#22bb44");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(source.read(cx).styles_ui.color_dialog_for, Some((1, 10, 0)));
        assert_eq!(
            source.read(cx).editor.doc.nodes[0].styles[0].colors()[0],
            [34, 187, 68]
        );
        ws.update(cx, |w, cx| w.activate_tab(1, window, cx));
        assert_eq!(ws.read(cx).active_tab(), Some(1));
        assert!(!window.has_active_dialog(cx));
        let e = source.read(cx);
        assert!(e.styles_ui.dialog_for.is_none());
        assert!(e.styles_ui.color_dialog_for.is_none());
        assert!(!e.editor.in_transaction());
        assert!(e.editor.history.is_empty());
        assert_eq!(e.editor.doc, original);
    });
}

#[gpui_kit::test]
fn native_movable_dialog_clamps_after_resize_resets_on_reopen_and_ignores_body_drags(
    cx: &mut TestAppContext,
) {
    use gpui_kit::{ParentElement as _, Styled as _};
    let (_ws, cx) = open(cx, styled_doc());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1000.), gpui_kit::px(800.)));
    cx.update(|window, cx| {
        window.open_dialog(cx, |dialog, _, _| dialog.title("Stationary").child("Body"));
        window.render_frame(cx);
        window.render_frame(cx);
    });
    cx.run_until_parked();
    let original = cx.debug_bounds("dialog-0").unwrap();
    let start = cx.debug_bounds("dialog-drag-handle-0").unwrap().center();
    let end = start + gpui_kit::point(gpui_kit::px(35.), gpui_kit::px(50.));
    cx.simulate_mouse_down(start, gpui_kit::MouseButton::Left, Default::default());
    cx.simulate_mouse_move(end, Some(gpui_kit::MouseButton::Left), Default::default());
    cx.simulate_mouse_up(end, gpui_kit::MouseButton::Left, Default::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.render_frame(cx);
    });
    assert_eq!(
        cx.debug_bounds("dialog-0").unwrap().origin,
        original.origin,
        "default dialog remains stationary"
    );
    cx.update(|window, cx| {
        window.close_dialog(cx);
        window.render_frame(cx);
        window.open_dialog(cx, |dialog, _, _| {
            dialog
                .movable(true)
                .title("Movable")
                .child(gpui_kit::div().h(gpui_kit::px(180.)).child("Body"))
        });
    });
    drag_dialog_title(cx, 0, 120., 160.);
    let moved = cx.debug_bounds("dialog-0").unwrap();
    let body = moved.origin + gpui_kit::point(gpui_kit::px(30.), gpui_kit::px(100.));
    let body_end = body + gpui_kit::point(gpui_kit::px(25.), gpui_kit::px(30.));
    cx.simulate_mouse_down(body, gpui_kit::MouseButton::Left, Default::default());
    cx.simulate_mouse_move(
        body_end,
        Some(gpui_kit::MouseButton::Left),
        Default::default(),
    );
    cx.simulate_mouse_up(body_end, gpui_kit::MouseButton::Left, Default::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.render_frame(cx);
    });
    assert_eq!(
        cx.debug_bounds("dialog-0").unwrap().origin,
        moved.origin,
        "body drag cannot move the dialog"
    );
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(400.), gpui_kit::px(300.)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
    });
    let surface = cx.debug_bounds("dialog-0").unwrap();
    let title = cx.debug_bounds("dialog-drag-handle-0").unwrap();
    assert!(surface.left() >= gpui_kit::px(0.) && surface.right() <= gpui_kit::px(400.));
    assert!(title.top() >= gpui_kit::px(0.) && title.bottom() <= gpui_kit::px(300.));
    cx.update(|window, cx| {
        window.close_dialog(cx);
        window.render_frame(cx);
        window.open_dialog(cx, |dialog, _, _| {
            dialog.movable(true).title("New opening").child("Body")
        });
        window.render_frame(cx);
        window.render_frame(cx);
    });
    cx.run_until_parked();
    assert_eq!(
        cx.debug_bounds("dialog-0").unwrap().origin,
        gpui_kit::point(gpui_kit::px(16.), gpui_kit::px(30.)),
        "new opening starts at normal position"
    );
}

#[gpui_kit::test]
fn adding_style_keeps_existing_effect_rows_stationary(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, styled_doc());
    let view = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| view.update(cx, |e, cx| e.open_blending_options(1, window, cx)));
    cx.run_until_parked();
    let before = cx.update(|window, cx| {
        window.render_frame(cx);
        window.find("style-dialog-effect-10").bounds()
    });
    let catalogue = LayerStyle::catalogue()
        .iter()
        .position(|s| s.key() == "drop_shadow")
        .unwrap();
    click(cx, ("style-kind", catalogue));
    cx.update(|window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("style-dialog-effect-10").bounds(), before);
        assert!(
            view.read(cx).editor.doc.nodes[0]
                .styles
                .iter()
                .any(|s| s.key() == "drop_shadow")
        );
    });
    click(cx, "style-dialog-cancel");
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, styled_doc()));
}

#[gpui_kit::test]
fn layer_context_style_dialog_returns_keyboard_focus_after_every_close(cx: &mut TestAppContext) {
    let original = styled_doc();
    let (workspace, cx) = open(cx, original.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1200.), gpui_kit::px(1100.)));
    let editor = cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap());
    for close in ["style-dialog-ok", "style-dialog-cancel", "close", "escape"] {
        cx.update(|window, cx| {
            editor.update(cx, |e, cx| e.set_tool(crate::editor::Tool::Hand, cx));
            window.render_frame(cx);
            window.right_click(("row", 1u64), cx);
        });
        cx.run_until_parked();
        let blending_options = cx.update(|window, _| {
            // The pointer can also open the lower Mask submenu as the long
            // context menu fits above this row. Target the verified top-level
            // action through its row scope instead of an ambiguous popup ID.
            let menu = window.within(("row", 1u64));
            let item = menu.find(11usize);
            assert_eq!(
                item.label(),
                Some(t!("editor.layer_menu.blending_options").as_ref()),
                "Actual layer context menu exposes Blending Options"
            );
            item.bounds().center()
        });
        // Use platform events in separate app updates for this modal-opening
        // click. ScopedWindow::click renders immediately after mouse-up, before
        // App flushes the menu's queued DismissEvent. That artificial frame can
        // refocus the still-open menu over the dialog it has just opened.
        cx.simulate_mouse_move(blending_options, None, Default::default());
        cx.simulate_click(blending_options, Default::default());
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.has_active_dialog(cx));
            assert!(!editor.read(cx).panel_focus.is_focused(window));
            assert!(
                gpui_kit::base::active_focus_trap(window, cx).is_some(),
                "{close}: opened dialog must own keyboard focus; contexts: {:?}",
                window.context_stack()
            );
        });
        click(cx, "style-dialog-enabled-10");
        cx.update(|window, cx| {
            assert!(
                !editor.read(cx).editor.doc.nodes[0].style_options[0].enabled,
                "{close}: the effect checkbox must change the live preview"
            );
            assert!(
                gpui_kit::base::active_focus_trap(window, cx).is_some(),
                "{close}: checkbox must preserve dialog keyboard focus; contexts: {:?}",
                window.context_stack()
            );
        });
        if close == "escape" {
            cx.simulate_keystrokes("escape");
            cx.run_until_parked();
        } else {
            click(cx, close);
        }
        cx.update(|window, cx| {
            assert!(!window.has_active_dialog(cx), "{close}");
            assert!(editor.read(cx).panel_focus.is_focused(window), "{close}");
            assert!(!editor.read(cx).editor.in_transaction(), "{close}");
        });
        cx.simulate_keystrokes("v");
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(editor.read(cx).tool, crate::editor::Tool::Move, "{close}"));
        if close == "style-dialog-ok" {
            cx.update(|_, cx| assert_ne!(editor.read(cx).editor.doc, original));
            cx.simulate_keystrokes("ctrl-z");
            cx.run_until_parked();
        }
        cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, original, "{close}"));
    }
}

#[gpui_kit::test]
fn photo_properties_style_opener_restores_shortcuts_after_every_close(cx: &mut TestAppContext) {
    use crate::editor::Tool;

    fn pointer_click(cx: &mut VisualTestContext, id: impl Into<gpui_kit::ElementId>) {
        let at = cx.update(|window, _| {
            let element = window.find(id);
            assert!(element.visible());
            element.bounds().center()
        });
        cx.simulate_mouse_move(at, None, Default::default());
        cx.simulate_click(at, Default::default());
        cx.run_until_parked();
    }

    for compact in [false, true] {
        let original = styled_doc();
        let (workspace, cx) = open(cx, original.clone());
        cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1100.)));
        let editor = cx.update(|window, cx| {
            cx.global_mut::<AppSettings>().0.compact_chrome = compact;
            let editor = workspace.read(cx).editor.clone().unwrap();
            editor.update(cx, |e, cx| e.set_tool(Tool::Hand, cx));
            window.refresh();
            editor
        });
        cx.run_until_parked();
        // This is the native route that leaves the direct inspector opener
        // visible: layer context menu -> Blending Options -> Cancel.
        cx.update(|window, cx| window.right_click(("row", 1u64), cx));
        cx.run_until_parked();
        let at = cx.update(|window, _| {
            let menu = window.within(("row", 1u64));
            let item = menu.find(11usize);
            assert_eq!(
                item.label(),
                Some(t!("editor.layer_menu.blending_options").as_ref())
            );
            item.bounds().center()
        });
        cx.simulate_mouse_move(at, None, Default::default());
        cx.simulate_click(at, Default::default());
        cx.run_until_parked();
        pointer_click(cx, "style-dialog-cancel");
        cx.update(|window, cx| {
            assert!(editor.read(cx).panel_focus.is_focused(window));
            assert!(window.find("open-layer-style").visible());
        });
        for close in ["style-dialog-ok", "style-dialog-cancel", "close", "escape"] {
            cx.simulate_keystrokes("h");
            cx.run_until_parked();
            cx.update(|_, cx| assert_eq!(editor.read(cx).tool, Tool::Hand));
            pointer_click(cx, "open-layer-style");
            cx.update(|window, cx| {
                assert!(window.has_active_dialog(cx));
                assert!(window.try_find("open-layer-style").is_none());
                assert!(!editor.read(cx).panel_focus.is_focused(window));
                assert!(gpui_kit::base::active_focus_trap(window, cx).is_some());
            });
            cx.update(|window, cx| {
                let pane = window.find("style-dialog-settings").bounds();
                let bounds = window
                    .within("layer-style-dialog")
                    .find(("advanced-blend-toggle", 1usize))
                    .bounds();
                let dy = if bounds.bottom() > pane.bottom() {
                    pane.bottom() - bounds.bottom()
                } else if bounds.top() < pane.top() {
                    pane.top() - bounds.top()
                } else {
                    return;
                };
                window.scroll(
                    "style-dialog-settings",
                    gpui_kit::ScrollDelta::Pixels(gpui_kit::point(gpui_kit::px(0.), dy)),
                    cx,
                );
            });
            cx.run_until_parked();
            let at = cx.update(|window, _| {
                let dialog = window.within("layer-style-dialog");
                let toggle = dialog.find(("advanced-blend-toggle", 1usize));
                assert!(toggle.visible());
                toggle.bounds().center()
            });
            cx.simulate_mouse_move(at, None, Default::default());
            cx.simulate_click(at, Default::default());
            cx.run_until_parked();
            cx.update(|window, cx| {
                let e = editor.read(cx);
                assert_eq!(
                    e.editor.doc.nodes[0].blending.blend_clipped_layers_as_group,
                    !original.nodes[0].blending.blend_clipped_layers_as_group,
                    "{close}: the pointer toggles the actual dialog control"
                );
                assert!(e.editor.in_transaction());
                assert!(gpui_kit::base::active_focus_trap(window, cx).is_some());
                assert!(!e.panel_focus.is_focused(window));
            });
            if close == "escape" {
                cx.simulate_keystrokes("escape");
                cx.run_until_parked();
            } else {
                pointer_click(cx, close);
            }
            cx.update(|window, cx| {
                assert!(!window.has_active_dialog(cx), "{close}");
                let e = editor.read(cx);
                assert!(e.panel_focus.is_focused(window), "{close}");
                assert!(!e.editor.in_transaction(), "{close}");
                assert_eq!(e.tool, Tool::Hand);
                assert_eq!(
                    e.editor.history.len(),
                    usize::from(close == "style-dialog-ok")
                );
                if close != "style-dialog-ok" {
                    assert_eq!(e.editor.doc, original, "{close}");
                }
            });
            // The native failure stranded both workspace and Photo shortcuts.
            // Do not click the canvas or explicitly focus an editor scope.
            cx.simulate_keystrokes("ctrl-shift-s");
            cx.run_until_parked();
            assert!(cx.did_prompt_for_new_path(), "{close}: Save As must route");
            cx.simulate_new_path_selection(|_| None);
            cx.run_until_parked();
            cx.simulate_keystrokes("v");
            cx.run_until_parked();
            cx.update(|_, cx| assert_eq!(editor.read(cx).tool, Tool::Move, "{close}"));
            if close == "style-dialog-ok" {
                cx.simulate_keystrokes("ctrl-z");
                cx.run_until_parked();
            }
            cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, original, "{close}"));
        }
    }
}
