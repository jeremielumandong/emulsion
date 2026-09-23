//! Brush controls next to the pointer or their toolbar trigger.
use super::*;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};

/// The small view observes the editor so open menus display live brush values.
struct BrushQuickControls {
    editor: WeakEntity<EditorView>,
    _observe: Subscription,
}

pub(super) fn menu(menu: PopupMenu, editor: &Entity<EditorView>, cx: &mut App) -> PopupMenu {
    let view = cx.new(|cx| BrushQuickControls {
        editor: editor.downgrade(),
        _observe: cx.observe(editor, |_, _, cx| cx.notify()),
    });
    let focus = editor.read(cx).canvas_focus.clone();
    let editor = editor.downgrade();
    menu.action_context(focus)
        .item(PopupMenuItem::element(move |_, _| view.clone()))
        .separator()
        .item(
            PopupMenuItem::new("All brush settings").on_click(move |_, _, cx| {
                editor
                    .update(cx, |editor, cx| {
                        editor.select_sidebar(SidebarTab::BrushSettings, cx);
                    })
                    .ok();
            }),
        )
}

impl Render for BrushQuickControls {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.editor
            .update(cx, |editor, cx| editor.brush_quick_controls(cx))
            .unwrap_or_else(|_| div().into_any_element())
    }
}

impl EditorView {
    fn brush_quick_controls(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::palette(cx);
        let brush = self.tools.brush;
        let mut controls = vec![
            self.opt_slider(
                SliderKey::QuickBrushSize,
                "Size",
                format!("{:.0}px", brush.size),
                ((brush.size - 1.0) / 499.0).sqrt(),
                (1.0, 500.0, 1.0),
                &p,
                cx,
            ),
            self.opt_slider(
                SliderKey::QuickBrushHardness,
                "Hardness",
                format!("{:.0}%", brush.hardness * 100.0),
                brush.hardness,
                (0.0, 100.0, 1.0),
                &p,
                cx,
            ),
        ];
        if self.tool != Tool::Heal {
            controls.push(self.opt_slider(
                SliderKey::QuickBrushOpacity,
                "Opacity",
                format!("{:.0}%", brush.opacity * 100.0),
                brush.opacity,
                (1.0, 100.0, 1.0),
                &p,
                cx,
            ));
        }
        controls.push(self.opt_slider(
            SliderKey::QuickBrushFlow,
            "Flow",
            format!("{:.0}%", brush.flow * 100.0),
            brush.flow,
            (1.0, 100.0, 1.0),
            &p,
            cx,
        ));
        div()
            .id("brush-quick-controls")
            .test_support()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .w(rems(18.))
            .text_sm()
            .text_color(p.ink)
            // A custom menu item normally closes on click. These controls stay
            // open for repeated adjustments and never pass input to the canvas.
            .on_click(cx.listener(|editor, _, _, cx| {
                // Click dispatch may consume mouse-up before the drag listener.
                if editor.dragging_quick_brush_slider() {
                    editor.drag_end(cx);
                }
                cx.stop_propagation();
            }))
            .on_mouse_move(cx.listener(|editor, event: &MouseMoveEvent, _, cx| {
                if editor.dragging_quick_brush_slider() {
                    editor.drag_move(event.position, cx);
                    cx.stop_propagation();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|editor, _, _, cx| {
                    if editor.dragging_quick_brush_slider() {
                        editor.drag_end(cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .child(div().child("Brush settings"))
            .children(controls)
            .child(
                div()
                    .text_xs()
                    .text_color(p.muted)
                    .child("Right-click canvas for these controls. [ ] resize the brush."),
            )
            .into_any_element()
    }

    fn dragging_quick_brush_slider(&self) -> bool {
        matches!(self.drag, Some(Drag::Slider { key, .. }) if key.is_quick_brush())
    }
}
