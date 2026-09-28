//! Named styles reuse native appearance across objects and project pages.
use super::*;
use emulsion_core::design_styles;
use gpui_kit::component::{
    Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};

impl EditorView {
    fn style_result(&mut self, result: Result<(), String>, message: &str, cx: &mut Context<Self>) {
        match result {
            Ok(()) => {
                self.after_change(cx);
                self.set_status(message, false, cx);
            }
            Err(error) => self.set_status(error, true, cx),
        }
    }

    pub(super) fn save_design_style_dialog(
        &mut self,
        rename: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let source = self.selected_layer_roots();
        if rename.is_none() && source.len() != 1 {
            self.set_status(
                "Select one object to save its appearance as a style.",
                true,
                cx,
            );
            return;
        }
        let initial = rename.clone().unwrap_or_else(|| {
            self.editor
                .doc
                .node(source[0])
                .map_or("New style", |node| node.name.as_str())
                .to_string()
        });
        let field = cx.new(|cx| InputState::new(window, cx).default_value(initial));
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        window.open_dialog(cx, move |dialog, _, _| {
            let field = field.clone();
            let owner = owner.clone();
            let source = source.clone();
            let rename = rename.clone();
            dialog.title(if rename.is_some() { "Rename saved style" } else { "Save reusable style" })
                .width(px(420.))
                .child(div().flex().flex_col().gap_2().child("Style name")
                    .child(Input::new(&field).id("design-style-name"))
                    .child("Styles preserve typography, colors, strokes, gradients and effects. Content and geometry stay independent."))
                .footer(crate::widgets::form_dialog_footer("Save style"))
                .on_ok(move |_, _, cx| {
                    let name = field.read(cx).value().to_string();
                    owner.update(cx, |this, cx| {
                        if this.edit_ticket() != ticket {
                            this.set_status("The page changed. Open the style dialog again.", true, cx);
                            return false;
                        }
                        let result = match &rename {
                            Some(old) => design_styles::rename(&mut this.editor, old, &name),
                            None => design_styles::create(&mut this.editor, source[0], &name),
                        };
                        let success = result.is_ok();
                        this.style_result(result, "Saved reusable style.", cx);
                        success
                    }).unwrap_or(false)
                })
        });
    }

    pub(super) fn design_saved_style_controls(
        &self,
        query: &str,
        p: &Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        let roots = self.selected_layer_roots();
        let has_links = roots
            .iter()
            .any(|id| self.editor.doc.design.style_links.contains_key(id));
        let mut panel = div().flex().flex_col().gap_2()
            .child("Saved styles")
            .child(Button::new("design-style-create").label("Save selection as style…").small().outline()
                .disabled(roots.len() != 1)
                .on_click(cx.listener(|this, _, window, cx| this.save_design_style_dialog(None, window, cx))))
            .when(has_links, |panel| panel
                .child(Button::new("design-style-reset").label("Reset linked appearance").small().outline()
                    .on_click(cx.listener(|this, _, _, cx| {
                        if !this.prepare_page_action(cx) { return; }
                        let ids = this.selected_layer_roots();
                        let result = design_styles::reset(&mut this.editor, &ids);
                        this.style_result(result, "Restored the saved appearance.", cx);
                    })))
                .child(Button::new("design-style-detach").label("Detach style").small().ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        if !this.prepare_page_action(cx) { return; }
                        let ids = this.selected_layer_roots();
                        let result = design_styles::detach(&mut this.editor, &ids);
                        this.style_result(result, "Style detached; appearance preserved.", cx);
                    }))))
            .child(div().text_xs().text_color(p.muted).child("Apply a style to link objects on this page. Update from selection publishes your edits to those links. Styles from another page are copied into this page."));
        let mut index = 0usize;
        for page in self.editor.page_list() {
            let Some(editor) = self.editor.page(page.id) else {
                continue;
            };
            for (name, style) in &editor.doc.design.saved_styles {
                if !format!("{} {}", page.name, name)
                    .to_lowercase()
                    .contains(query)
                {
                    continue;
                }
                let current = page.id == self.editor.active_page();
                let name = name.clone();
                let apply_name = name.clone();
                let style = style.clone();
                let owner = cx.weak_entity();
                let can_update = roots.len() == 1;
                let links = editor
                    .doc
                    .design
                    .style_links
                    .values()
                    .filter(|linked| *linked == &name)
                    .count();
                let row = div().flex().flex_col().gap_1().p_2().border_1().border_color(p.line).rounded_md()
                    .child(name.clone())
                    .child(div().text_xs().text_color(p.muted).child(format!("{} · {links} linked", page.name)))
                    .child(div().flex().gap_1()
                        .child(Button::new(("design-style-apply", index)).label("Apply").small().outline().disabled(roots.is_empty())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.prepare_page_action(cx) { return; }
                                let ids = this.selected_layer_roots();
                                let result = design_styles::apply(&mut this.editor, &ids, &apply_name, &style).map(|_| ());
                                this.style_result(result, "Applied linked style. Undo restores the previous appearance.", cx);
                            })))
                        .when(current, |row| row.child(Button::new(("design-style-manage", index)).label("Manage").small().ghost()
                            .dropdown_menu(move |menu, _, _| {
                                let update_owner = owner.clone();
                                let rename_owner = owner.clone();
                                let remove_owner = owner.clone();
                                let update_name = name.clone();
                                let rename_name = name.clone();
                                let remove_name = name.clone();
                                menu.item(PopupMenuItem::new("Update from selection").disabled(!can_update).on_click(move |_, _, cx| {
                                    update_owner.update(cx, |this, cx| {
                                        if !this.prepare_page_action(cx) { return; }
                                        let ids = this.selected_layer_roots();
                                        if ids.len() != 1 { return; }
                                        let result = design_styles::update(&mut this.editor, &update_name, ids[0]);
                                        this.style_result(result, "Updated linked appearances on this page.", cx);
                                    }).ok();
                                }))
                                .item(PopupMenuItem::new("Rename…").on_click(move |_, window, cx| {
                                    rename_owner.update(cx, |this, cx| this.save_design_style_dialog(Some(rename_name.clone()), window, cx)).ok();
                                }))
                                .item(PopupMenuItem::new("Remove style; keep appearance").on_click(move |_, _, cx| {
                                    remove_owner.update(cx, |this, cx| {
                                        if !this.prepare_page_action(cx) { return; }
                                        let result = design_styles::remove(&mut this.editor, &remove_name);
                                        this.style_result(result, "Removed style; objects keep their appearance.", cx);
                                    }).ok();
                                }))
                            }))));
                panel = panel.child(row);
                index += 1;
            }
        }
        if index == 0 {
            panel = panel.child("Save a selected object's appearance to start your style library.");
        }
        panel.into_any_element()
    }
}
