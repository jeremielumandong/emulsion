//! Brush controls next to the pointer or their toolbar trigger.
use super::*;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::{
    Disableable, Selectable, Sizable,
    button::{Button, ButtonVariants},
};

/// The small view observes the editor so open menus display live brush values.
struct BrushQuickControls {
    editor: WeakEntity<EditorView>,
    _observe: Subscription,
}

pub(super) fn menu(menu: PopupMenu, editor: &Entity<EditorView>, cx: &mut App) -> PopupMenu {
    editor.update(cx, |editor, cx| editor.prepare_presets(cx));
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
        let key = self.active_memory_key();
        let has_brush = key.is_some() && self.presets.current_id.is_some();
        let marks = self.active_brush_marks(cx);
        let mut memories =
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_xs().text_color(p.muted).child(match key {
                    Some("paint") => "Paint memories · size and opacity",
                    Some("smudge") => "Smudge memories · size and opacity",
                    Some("erase") => "Erase memories · size and opacity",
                    _ => "Brush memories · size and opacity",
                }));
        for (index, mark) in marks.into_iter().enumerate() {
            let active = mark.is_some_and(|m| {
                (m.size - brush.size).abs() < 0.01 && (m.opacity - brush.opacity).abs() < 0.001
            });
            let label = mark
                .map(|m| format!("{}: {:.0}px · {:.0}%", index + 1, m.size, m.opacity * 100.0))
                .unwrap_or_else(|| format!("{}: Empty", index + 1));
            memories = memories.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        Button::new(("brush-memory-recall", index))
                            .small()
                            .outline()
                            .flex_1()
                            .label(label)
                            .selected(active)
                            .disabled(!has_brush || mark.is_none())
                            .tooltip("Recall this size and opacity")
                            .on_click(cx.listener(move |editor, _, _, cx| {
                                editor.recall_brush_mark(index, cx)
                            })),
                    )
                    .child(
                        Button::new(("brush-memory-save", index))
                            .small()
                            .outline()
                            .label(if mark.is_some() { "Replace" } else { "Save" })
                            .disabled(!has_brush)
                            .tooltip("Save current size and opacity in this memory")
                            .on_click(cx.listener(move |editor, _, _, cx| {
                                editor.save_brush_mark(index, cx)
                            })),
                    )
                    .child(
                        Button::new(("brush-memory-remove", index))
                            .small()
                            .ghost()
                            .label("Clear")
                            .disabled(!has_brush || mark.is_none())
                            .on_click(cx.listener(move |editor, _, _, cx| {
                                editor.remove_brush_mark(index, cx)
                            })),
                    ),
            );
        }
        if !has_brush {
            memories = memories.child(
                div()
                    .text_xs()
                    .text_color(p.muted)
                    .child("Select a library brush to save memories."),
            );
        }
        let mut transfer = div().flex().items_center().gap_1();
        for (id, label, target) in [
            ("transfer-brush-paint", "Paint", PaintKind::Brush),
            ("transfer-brush-smudge", "Smudge", PaintKind::Smudge),
            ("transfer-brush-erase", "Erase", PaintKind::Eraser),
        ] {
            let current = self.tool == Tool::Brush && self.tools.paint == target;
            transfer = transfer.child(
                Button::new(id)
                    .small()
                    .outline()
                    .flex_1()
                    .label(label)
                    .selected(current)
                    .disabled(current)
                    .on_click(
                        cx.listener(move |editor, _, _, cx| editor.transfer_brush_to(target, cx)),
                    ),
            );
        }
        div()
            .id("brush-quick-controls")
            .test_support()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .w(rems(22.))
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
            .child(memories)
            .child(
                div()
                    .text_xs()
                    .text_color(p.muted)
                    .child("Use current brush with"),
            )
            .child(transfer)
            .when(
                self.tool == Tool::Brush
                    && matches!(self.tools.paint, PaintKind::Brush | PaintKind::Smudge),
                |controls| {
                    controls
                        .child(
                            div()
                                .text_xs()
                                .text_color(p.muted)
                                .child("Wet paint and smudge sampling"),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_1()
                                .child(
                                    Button::new("brush-sample-visible")
                                        .small()
                                        .outline()
                                        .flex_1()
                                        .label("Visible layers")
                                        .selected(self.tools.sample_merged)
                                        .on_click(cx.listener(|editor, _, _, cx| {
                                            editor.tools.sample_merged = true;
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    Button::new("brush-sample-current")
                                        .small()
                                        .outline()
                                        .flex_1()
                                        .label("Current layer")
                                        .selected(!self.tools.sample_merged)
                                        .on_click(cx.listener(|editor, _, _, cx| {
                                            editor.tools.sample_merged = false;
                                            cx.notify();
                                        })),
                                ),
                        )
                },
            )
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
