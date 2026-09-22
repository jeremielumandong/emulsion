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
