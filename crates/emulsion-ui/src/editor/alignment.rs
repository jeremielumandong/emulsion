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
            self.set_status(t!("editor.alignment.finish_before_align"), false, cx);
            return;
        }
        let Some(id) = self.selected else {
            self.set_status(t!("editor.alignment.select_to_align"), false, cx);
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
            self.set_status(t!("editor.alignment.finish_before_arrange"), false, cx);
            return;
        }
        let ids = self.selected_layer_roots();
        if ids.is_empty() {
            self.set_status(t!("editor.alignment.select_to_arrange"), false, cx);
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
                    .label(if self.is_photo_workflow() {
                        format!("{} ▾", t!("editor.photo_panels.align"))
                    } else {
                        t!("editor.alignment.align_menu").into_owned()
                    })
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
                        // PopupMenu only restores focus when it still owns it;
                        // an outside click into an input keeps that new focus.
                        // Root and target submenus are built independently, so
                        // give each the same live Photo return context.
                        let focus = view
                            .read(cx)
                            .is_photo_workflow()
                            .then(|| view.read(cx).photo_options_menu_focus(cx));
                        let menu =
                            menu.when_some(focus.clone(), |menu, focus| menu.action_context(focus));
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
                        let canvas_focus = focus.clone();
                        let menu = menu.submenu(
                            t!("editor.alignment.canvas"),
                            window,
                            cx,
                            move |menu, _, _| {
                                alignment_items(
                                    menu,
                                    canvas_editor.clone(),
                                    canvas_selected.clone(),
                                    ArrangeTarget::Canvas,
                                    canvas_focus.clone(),
                                )
                            },
                        );
                        let pixel_selected = selected.clone();
                        let menu = if has_selection {
                            let selection_editor = editor.clone();
                            let selection_focus = focus.clone();
                            menu.submenu(
                                t!("editor.alignment.pixel_selection"),
                                window,
                                cx,
                                move |menu, _, _| {
                                    alignment_items(
                                        menu,
                                        selection_editor.clone(),
                                        pixel_selected.clone(),
                                        ArrangeTarget::PixelSelection,
                                        selection_focus.clone(),
                                    )
                                },
                            )
                        } else {
                            menu.item(
                                PopupMenuItem::new(t!("editor.alignment.selection_disabled"))
                                    .disabled(true),
                            )
                        };
                        let layer_editor = editor.clone();
                        if selected.len() > 1 {
                            menu.submenu(
                                t!("editor.alignment.selected_layers"),
                                window,
                                cx,
                                move |menu, _, _| {
                                    alignment_items(
                                        menu,
                                        layer_editor.clone(),
                                        selected.clone(),
                                        ArrangeTarget::SelectedLayers,
                                        focus.clone(),
                                    )
                                },
                            )
                        } else {
                            menu.item(
                                PopupMenuItem::new(t!("editor.alignment.selected_layers_disabled"))
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
    focus: Option<FocusHandle>,
) -> PopupMenu {
    menu = menu.when_some(focus, |menu, focus| menu.action_context(focus));
    for (label, alignment) in [
        (t!("editor.alignment.align_left"), Alignment::Left),
        (
            t!("editor.alignment.center_horizontally"),
            Alignment::HorizontalCenter,
        ),
        (t!("editor.alignment.align_right"), Alignment::Right),
        (t!("editor.alignment.align_top"), Alignment::Top),
        (
            t!("editor.alignment.center_vertically"),
            Alignment::VerticalCenter,
        ),
        (t!("editor.alignment.align_bottom"), Alignment::Bottom),
    ] {
        let editor = editor.clone();
        let selected = selected.clone();
        menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
            editor
                .update(cx, |view, cx| {
                    if view.selected_layer_roots() != selected {
                        view.set_status(t!("editor.alignment.layer_changed"), false, cx);
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
            PopupMenuItem::new(distribution_label(distribution))
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
                                view.set_status(t!("editor.alignment.layers_changed"), false, cx);
                            }
                        })
                        .ok();
                }),
        );
    }
    menu
}

/// The menu label for a distribution, in the interface language.
fn distribution_label(distribution: Distribution) -> std::borrow::Cow<'static, str> {
    match distribution {
        Distribution::Left => t!("editor.alignment.distribute_left"),
        Distribution::HorizontalCenter => t!("editor.alignment.distribute_horizontal_centers"),
        Distribution::Right => t!("editor.alignment.distribute_right"),
        Distribution::Top => t!("editor.alignment.distribute_top"),
        Distribution::VerticalCenter => t!("editor.alignment.distribute_vertical_centers"),
        Distribution::Bottom => t!("editor.alignment.distribute_bottom"),
        Distribution::HorizontalGap => t!("editor.alignment.distribute_horizontal_spacing"),
        Distribution::VerticalGap => t!("editor.alignment.distribute_vertical_spacing"),
    }
}
