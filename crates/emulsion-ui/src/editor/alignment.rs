//! Compact alignment controls shared by every spatial layer type.
use super::*;
use emulsion_core::command::{AlignTarget, Alignment};
use emulsion_core::layer_links::{Arrange, ArrangeTarget, Distribution};
use gpui_kit::component::button::Button;
use gpui_kit::component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::{Disableable, Sizable};

impl EditorView {
    fn alignment_busy(&self) -> bool {
        self.assistant.running
            || self.drag.is_some()
            || self.warp.is_some()
            || self.editor.in_transaction()
    }

    pub(crate) fn align_selected(
        &mut self,
        alignment: Alignment,
        target: AlignTarget,
        cx: &mut Context<Self>,
    ) {
        if self.alignment_busy() {
            self.set_status(
                "Finish the current edit before aligning artwork.",
                false,
                cx,
            );
            return;
        }
        let Some(id) = self.selected else {
            self.set_status("Select a layer or group to align.", false, cx);
            return;
        };
        if self.selected_layer_roots().len() > 1 {
            self.arrange_selected(
                Arrange::Align(alignment),
                match target {
                    AlignTarget::Canvas => ArrangeTarget::Canvas,
                    AlignTarget::Selection => ArrangeTarget::PixelSelection,
                },
                cx,
            );
            return;
        }
        self.execute(
            Command::AlignNode {
                id,
                alignment,
                target,
            },
            cx,
        );
    }

    pub(crate) fn arrange_selected(
        &mut self,
        operation: Arrange,
        target: ArrangeTarget,
        cx: &mut Context<Self>,
    ) {
        if self.alignment_busy() {
            self.set_status(
                "Finish the current edit before arranging artwork.",
                false,
                cx,
            );
            return;
        }
        let ids = self.selected_layer_roots();
        if ids.is_empty() {
            self.set_status("Select layers to arrange.", false, cx);
            return;
        }
        self.snap_lines.clear();
        self.execute(
            Command::ArrangeLayers {
                ids,
                operation,
                target,
            },
            cx,
        );
    }

    pub(crate) fn alignment_controls(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        let editor = cx.entity().downgrade();
        let disabled = self.selected.is_none() || self.alignment_busy();
        div()
            .id("move-align")
            .test_support()
            .flex_none()
            .child(
                Button::new("move-align-button")
                    .label("Align ▾")
                    .small()
                    .rounded_none()
                    .bg(p.soft_bg)
                    .text_color(p.ink)
                    .border_color(p.line)
                    .disabled(disabled)
                    .dropdown_menu(move |menu, window, cx| {
                        let Some(view) = editor.upgrade() else {
                            return menu;
                        };
                        let selected = view.read(cx).selected_layer_roots();
                        let has_selection = view
                            .read(cx)
                            .editor
                            .doc
                            .selection
                            .as_ref()
                            .is_some_and(|mask| !emulsion_raster::select::bounds(mask).is_empty());
                        let canvas_editor = editor.clone();
                        let canvas_selected = selected.clone();
                        let menu = menu.submenu("Canvas", window, cx, move |menu, _, _| {
                            alignment_items(
                                menu,
                                canvas_editor.clone(),
                                canvas_selected.clone(),
                                ArrangeTarget::Canvas,
                            )
                        });
                        let pixel_selected = selected.clone();
                        let menu = if has_selection {
                            let selection_editor = editor.clone();
                            menu.submenu("Pixel selection", window, cx, move |menu, _, _| {
                                alignment_items(
                                    menu,
                                    selection_editor.clone(),
                                    pixel_selected.clone(),
                                    ArrangeTarget::PixelSelection,
                                )
                            })
                        } else {
                            menu.item(
                                PopupMenuItem::new("Selection (make a selection first)")
                                    .disabled(true),
                            )
                        };
                        let layer_editor = editor.clone();
                        if selected.len() > 1 {
                            menu.submenu("Selected layers", window, cx, move |menu, _, _| {
                                alignment_items(
                                    menu,
                                    layer_editor.clone(),
                                    selected.clone(),
                                    ArrangeTarget::SelectedLayers,
                                )
                            })
                        } else {
                            menu.item(
                                PopupMenuItem::new("Selected layers (select two or more)")
                                    .disabled(true),
                            )
                        }
                    }),
            )
            .into_any_element()
    }
}

fn alignment_items(
    mut menu: PopupMenu,
    editor: WeakEntity<EditorView>,
    selected: Vec<NodeId>,
    target: ArrangeTarget,
) -> PopupMenu {
    for (label, alignment) in [
        ("Align left", Alignment::Left),
        ("Center horizontally", Alignment::HorizontalCenter),
        ("Align right", Alignment::Right),
        ("Align top", Alignment::Top),
        ("Center vertically", Alignment::VerticalCenter),
        ("Align bottom", Alignment::Bottom),
    ] {
        let editor = editor.clone();
        let selected = selected.clone();
        menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
            editor
                .update(cx, |view, cx| {
                    if view.selected_layer_roots() != selected {
                        view.set_status("The selected layer changed. Open Align again.", false, cx);
                        return;
                    }
                    if selected.len() == 1 && target != ArrangeTarget::SelectedLayers {
                        view.align_selected(
                            alignment,
                            if target == ArrangeTarget::Canvas {
                                AlignTarget::Canvas
                            } else {
                                AlignTarget::Selection
                            },
                            cx,
                        );
                    } else {
                        view.arrange_selected(Arrange::Align(alignment), target, cx);
                    }
                })
                .ok();
        }));
    }
    menu = menu.separator();
    for distribution in [
        Distribution::Left,
        Distribution::HorizontalCenter,
        Distribution::Right,
        Distribution::Top,
        Distribution::VerticalCenter,
        Distribution::Bottom,
        Distribution::HorizontalGap,
        Distribution::VerticalGap,
    ] {
        let editor = editor.clone();
        let selected = selected.clone();
        menu = menu.item(
            PopupMenuItem::new(distribution.label())
                .disabled(selected.len() < 3)
                .on_click(move |_, _, cx| {
                    editor
                        .update(cx, |view, cx| {
                            if view.selected_layer_roots() == selected {
                                view.arrange_selected(
                                    Arrange::Distribute(distribution),
                                    target,
                                    cx,
                                );
                            } else {
                                view.set_status(
                                    "The selected layers changed. Open Align again.",
                                    false,
                                    cx,
                                );
                            }
                        })
                        .ok();
                }),
        );
    }
    menu
}
