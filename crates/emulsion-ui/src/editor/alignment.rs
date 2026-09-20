//! Compact alignment controls shared by every spatial layer type.
use super::*;
use emulsion_core::command::{AlignTarget, Alignment};
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
        self.snap_lines.clear();
        match self.editor.execute(Command::AlignNode {
            id,
            alignment,
            target,
        }) {
            Ok(_) => {
                self.status = None;
                self.after_change(cx);
            }
            Err(error) => self.set_status(error.to_string(), true, cx),
        }
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
                        let selected = view.read(cx).selected;
                        let has_selection = view
                            .read(cx)
                            .editor
                            .doc
                            .selection
                            .as_ref()
                            .is_some_and(|mask| !emulsion_raster::select::bounds(mask).is_empty());
                        let canvas_editor = editor.clone();
                        let menu = menu.submenu("Canvas", window, cx, move |menu, _, _| {
                            alignment_items(
                                menu,
                                canvas_editor.clone(),
                                selected,
                                AlignTarget::Canvas,
                            )
                        });
                        if has_selection {
                            let selection_editor = editor.clone();
                            menu.submenu("Selection", window, cx, move |menu, _, _| {
                                alignment_items(
                                    menu,
                                    selection_editor.clone(),
                                    selected,
                                    AlignTarget::Selection,
                                )
                            })
                        } else {
                            menu.item(
                                PopupMenuItem::new("Selection (make a selection first)")
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
    selected: Option<NodeId>,
    target: AlignTarget,
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
        menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
            editor
                .update(cx, |view, cx| {
                    if view.selected != selected {
                        view.set_status("The selected layer changed. Open Align again.", false, cx);
                        return;
                    }
                    view.align_selected(alignment, target, cx);
                })
                .ok();
        }));
    }
    menu
}
