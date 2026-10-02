//! Layer comps (L8) in the Panel inspector: named sets of hidden layers per
//! panel. Save current as… records which layers are hidden now
//! (`capture_comp`), Apply shows and hides the layers as saved
//! (`apply_comp`), and Rename and Delete edit the board. Each is one Undo
//! step; locked panels refuse.
use super::*;
use emulsion_core::project::PageId;
use gpui_kit::component::{
    Disableable, Sizable,
    button::{Button, ButtonVariants},
};

#[cfg(test)]
#[path = "storyboard_comps_tests.rs"]
mod tests;

impl EditorView {
    fn comps_refused(&mut self, panel: PageId, cx: &mut Context<Self>) -> bool {
        if self.editor.storyboard().is_some_and(|b| b.is_locked(panel)) {
            self.set_status("That panel is locked. Unlock it to change it.", true, cx);
            return true;
        }
        false
    }

    fn comp_result(&mut self, result: Result<(), String>, cx: &mut Context<Self>) -> bool {
        match result {
            Ok(()) => {
                self.after_change(cx);
                true
            }
            Err(error) => {
                self.set_status(error, true, cx);
                false
            }
        }
    }

    /// Save which of the active panel's layers are hidden as `name`,
    /// replacing a comp of that name.
    pub(crate) fn save_layer_comp(&mut self, name: &str, cx: &mut Context<Self>) -> bool {
        let panel = self.editor.active_page();
        if self.comps_refused(panel, cx) || !self.prepare_page_action(cx) {
            return false;
        }
        let result = self.editor.capture_comp(panel, name);
        self.comp_result(result, cx)
    }

    /// Show and hide the active panel's layers as comp `name` saved them.
    pub(crate) fn apply_layer_comp(&mut self, name: &str, cx: &mut Context<Self>) -> bool {
        let panel = self.editor.active_page();
        if self.comps_refused(panel, cx) || !self.prepare_page_action(cx) {
            return false;
        }
        let result = self.editor.apply_comp(panel, name);
        let done = self.comp_result(result, cx);
        if done {
            self.set_status(format!("Layer comp “{name}” applied."), false, cx);
        }
        done
    }

    pub(crate) fn rename_layer_comp(
        &mut self,
        old: &str,
        new: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let panel = self.editor.active_page();
        if self.comps_refused(panel, cx) {
            return false;
        }
        let new = new.trim().to_string();
        self.edit_board(
            |b| {
                let comps = &mut b
                    .panels
                    .get_mut(&panel)
                    .ok_or("Panel does not exist.")?
                    .comps;
                if comps.iter().any(|c| c.name == new && c.name != old) {
                    return Err(format!("This panel already has a layer comp “{new}”."));
                }
                comps
                    .iter_mut()
                    .find(|c| c.name == old)
                    .ok_or("That layer comp no longer exists.")?
                    .name = new;
                Ok(())
            },
            cx,
        )
    }

    pub(crate) fn delete_layer_comp(&mut self, name: &str, cx: &mut Context<Self>) -> bool {
        let panel = self.editor.active_page();
        if self.comps_refused(panel, cx) {
            return false;
        }
        self.edit_board(
            |b| {
                let comps = &mut b
                    .panels
                    .get_mut(&panel)
                    .ok_or("Panel does not exist.")?
                    .comps;
                let before = comps.len();
                comps.retain(|c| c.name != name);
                if comps.len() == before {
                    return Err("That layer comp no longer exists.".into());
                }
                Ok(())
            },
            cx,
        )
    }

    pub(crate) fn save_comp_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let count = self
            .editor
            .storyboard()
            .and_then(|b| b.panels.get(&self.editor.active_page()))
            .map_or(0, |p| p.comps.len());
        self.timeline_text_dialog(
            "Save layer comp",
            "Name: which layers are hidden now is saved under it (an existing comp of that name is replaced)",
            format!("Comp {}", count + 1),
            "Save",
            |this, text, cx| this.save_layer_comp(&text, cx),
            window,
            cx,
        );
    }

    fn rename_comp_dialog(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let old = name.clone();
        self.timeline_text_dialog(
            "Rename layer comp",
            "New name",
            name,
            "Rename",
            move |this, text, cx| this.rename_layer_comp(&old, &text, cx),
            window,
            cx,
        );
    }

    /// The Panel inspector's Layer comps section.
    pub(crate) fn layer_comps_section(
        &self,
        panel: PageId,
        locked: bool,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let comps: Vec<_> = self
            .editor
            .storyboard()
            .and_then(|b| b.panels.get(&panel))
            .map(|p| p.comps.clone())
            .unwrap_or_default();
        let hidden: Vec<NodeId> = self
            .editor
            .doc
            .nodes
            .iter()
            .filter(|n| !n.visible)
            .map(|n| n.id)
            .collect();
        let mut root =
            div()
                .id("storyboard-layer-comps")
                .test_support()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(label("Layer comps", p))
                        .child(div().flex_1())
                        .child(
                            Button::new("storyboard-comp-save")
                                .label("Save current as…")
                                .tooltip("Save which layers are hidden now as a named comp")
                                .xsmall()
                                .outline()
                                .disabled(locked)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.save_comp_dialog(window, cx)
                                })),
                        ),
                );
        if comps.is_empty() {
            root = root.child(mono(
                "No comps yet. Hide and show layers, then save the look.",
                10.,
                p.muted,
            ));
        }
        for (index, comp) in comps.into_iter().enumerate() {
            let current = {
                let mut a = comp.hidden.clone();
                a.sort_unstable();
                let mut b = hidden.clone();
                b.sort_unstable();
                a == b
            };
            let (apply, rename, delete) = (comp.name.clone(), comp.name.clone(), comp.name.clone());
            root = root.child(
                div()
                    .id(("storyboard-comp", index))
                    .test_support()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_color(if current { p.accent } else { p.ink })
                            .child(comp.name.clone()),
                    )
                    .child(
                        Button::new(("storyboard-comp-apply", index))
                            .label("Apply")
                            .tooltip("Show and hide this panel's layers as saved")
                            .xsmall()
                            .ghost()
                            .disabled(locked)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.apply_layer_comp(&apply, cx);
                            })),
                    )
                    .child(
                        Button::new(("storyboard-comp-rename", index))
                            .label("Rename…")
                            .xsmall()
                            .ghost()
                            .disabled(locked)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.rename_comp_dialog(rename.clone(), window, cx)
                            })),
                    )
                    .child(
                        Button::new(("storyboard-comp-delete", index))
                            .label("Delete")
                            .xsmall()
                            .ghost()
                            .disabled(locked)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.delete_layer_comp(&delete, cx);
                            })),
                    ),
            );
        }
        root.into_any_element()
    }
}
