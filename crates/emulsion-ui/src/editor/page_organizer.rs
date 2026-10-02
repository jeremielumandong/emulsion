//! A page overview with ID-based selection and atomic project operations.
use super::*;
use emulsion_core::project::{PageId, ProjectStamp};
use emulsion_io::project_export::Format;
use gpui_kit::component::{
    Disableable, Sizable,
    button::{Button, ButtonVariants},
};
use std::{collections::HashSet, ops::Range};

#[derive(Default)]
pub(super) struct PageOrganizerUi {
    pub(super) open: bool,
    selected: HashSet<PageId>,
    anchor: Option<PageId>,
    focus: Option<PageId>,
    scroll: UniformListScrollHandle,
}
#[derive(Clone)]
struct DraggedPages {
    owner: u64,
    stamp: ProjectStamp,
    ids: Vec<PageId>,
}
impl Render for DraggedPages {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        div()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(p.ink)
            .text_color(p.paper)
            .child(SharedString::from(t!(
                "editor.page_organizer.drag_pages",
                count = self.ids.len()
            )))
    }
}
impl EditorView {
    pub(super) fn reconcile_page_selection(&mut self) {
        let state = &mut self.pages_ui.organizer;
        state.selected.retain(|id| self.editor.page(*id).is_some());
        if state
            .anchor
            .is_some_and(|id| self.editor.page(id).is_none())
        {
            state.anchor = None;
        }
        if state.focus.is_some_and(|id| self.editor.page(id).is_none()) {
            state.focus = Some(self.editor.active_page());
        }
    }
    pub(super) fn selected_project_pages(&self) -> Vec<PageId> {
        self.editor
            .page_list()
            .iter()
            .filter_map(|p| {
                self.pages_ui
                    .organizer
                    .selected
                    .contains(&p.id)
                    .then_some(p.id)
            })
            .collect()
    }
    pub(super) fn open_page_organizer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_design() || !self.prepare_page_action(cx) {
            return;
        }
        self.close_font_picker(cx);
        self.reconcile_page_selection();
        if !self.pages_ui.organizer.open {
            let id = self.editor.active_page();
            self.pages_ui.organizer.selected = HashSet::from([id]);
            self.pages_ui.organizer.anchor = Some(id);
            self.pages_ui.organizer.focus = Some(id);
            self.pages_ui.organizer.open = true;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }
    fn close_page_organizer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_active_drag(window);
        self.pages_ui.organizer.open = false;
        window.focus(&self.canvas_focus, cx);
        cx.notify();
    }
    fn select_organizer_page(
        &mut self,
        id: PageId,
        toggle: bool,
        range: bool,
        cx: &mut Context<Self>,
    ) {
        if self.editor.page(id).is_none() {
            return;
        }
        let pages = self.editor.page_list();
        let state = &mut self.pages_ui.organizer;
        if range {
            let start = state
                .anchor
                .and_then(|a| pages.iter().position(|p| p.id == a));
            let end = pages.iter().position(|p| p.id == id).unwrap();
            if !toggle {
                state.selected.clear();
            }
            if let Some(start) = start {
                state
                    .selected
                    .extend(pages[start.min(end)..=start.max(end)].iter().map(|p| p.id));
            } else {
                state.selected.insert(id);
                state.anchor = Some(id);
            }
        } else if toggle {
            if !state.selected.remove(&id) {
                state.selected.insert(id);
            }
            state.anchor = Some(id);
        } else {
            state.selected = HashSet::from([id]);
            state.anchor = Some(id);
        }
        state.focus = Some(id);
        cx.notify();
    }
    fn duplicate_organizer_pages(&mut self, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let selected = self.selected_project_pages();
        match self.editor.duplicate_pages(&selected) {
            Ok(ids) => {
                self.pages_ui.organizer.selected = ids.iter().copied().collect();
                self.pages_ui.organizer.anchor = ids.first().copied();
                self.pages_ui.organizer.focus = Some(self.editor.active_page());
                self.after_change(cx);
                self.set_status(
                    t!("editor.page_organizer.duplicated", count = ids.len()),
                    false,
                    cx,
                );
            }
            Err(error) => self.set_status(error, true, cx),
        }
    }
    fn delete_organizer_pages(&mut self, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let selected = self.selected_project_pages();
        match self.editor.remove_pages(&selected) {
            Ok(()) => {
                let active = self.editor.active_page();
                self.pages_ui.organizer.selected = HashSet::from([active]);
                self.pages_ui.organizer.anchor = Some(active);
                self.pages_ui.organizer.focus = Some(active);
                self.after_change(cx);
                self.set_status(
                    t!("editor.page_organizer.deleted", count = selected.len()),
                    false,
                    cx,
                );
            }
            Err(error) => self.set_status(error, true, cx),
        }
    }
    fn move_organizer_pages(&mut self, selected: &[PageId], slot: usize, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        match self.editor.move_pages(selected, slot) {
            Ok(()) => {
                self.pages_ui.organizer.selected = selected.iter().copied().collect();
                self.after_change(cx);
            }
            Err(error) => self.set_status(error, true, cx),
        }
    }
    fn step_organizer_pages(&mut self, forward: bool, cx: &mut Context<Self>) {
        let selected = self.selected_project_pages();
        let layout = self.editor.page_list();
        let first = layout.iter().position(|p| selected.contains(&p.id));
        let last = layout.iter().rposition(|p| selected.contains(&p.id));
        let slot = if forward {
            last.filter(|i| i + 1 < layout.len()).map(|i| i + 2)
        } else {
            first.filter(|i| *i > 0).map(|i| i - 1)
        };
        if let Some(slot) = slot {
            self.move_organizer_pages(&selected, slot, cx);
        }
    }
    fn drop_organizer_pages(&mut self, drag: &DraggedPages, slot: usize, cx: &mut Context<Self>) {
        if !self.visible
            || !self.pages_ui.organizer.open
            || drag.owner != cx.entity_id().as_u64()
            || drag.stamp != self.editor.stamp()
        {
            return;
        }
        self.move_organizer_pages(&drag.ids, slot, cx);
    }
    fn organizer_key(
        &mut self,
        event: &KeyDownEvent,
        columns: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        if matches!(key, "enter" | "space") && !self.focus.is_focused(window) {
            return;
        }
        let mods = event.keystroke.modifiers;
        let command = mods.control || mods.platform;
        match key {
            "escape" => {
                if !cx.stop_active_drag(window) {
                    self.close_page_organizer(window, cx);
                }
            }
            "a" if command => {
                self.pages_ui.organizer.selected =
                    self.editor.page_list().iter().map(|p| p.id).collect();
                cx.notify();
            }
            "d" if command => self.duplicate_organizer_pages(cx),
            "z" if command => {
                if mods.shift {
                    self.redo(cx);
                } else {
                    self.undo(cx);
                }
            }
            "y" if command => self.redo(cx),
            "delete" | "backspace" => self.delete_organizer_pages(cx),
            "left" | "right" if mods.alt => self.step_organizer_pages(key == "right", cx),
            "left" | "right" | "up" | "down" => {
                let pages = self.editor.page_list();
                let current = self
                    .pages_ui
                    .organizer
                    .focus
                    .unwrap_or(self.editor.active_page());
                let index = pages.iter().position(|p| p.id == current).unwrap_or(0);
                let next = match key {
                    "left" => index.saturating_sub(1),
                    "right" => (index + 1).min(pages.len() - 1),
                    "up" => index.saturating_sub(columns),
                    _ => (index + columns).min(pages.len() - 1),
                };
                let id = pages[next].id;
                self.select_organizer_page(id, command, mods.shift, cx);
                self.pages_ui
                    .organizer
                    .scroll
                    .scroll_to_item(next / columns, ScrollStrategy::Top);
            }
            "space" => {
                if let Some(id) = self.pages_ui.organizer.focus {
                    self.select_organizer_page(id, true, false, cx);
                }
            }
            "enter" => {
                if let Some(id) = self.pages_ui.organizer.focus {
                    self.select_page(id, cx);
                    self.close_page_organizer(window, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }
    fn organizer_tile(
        &mut self,
        index: usize,
        width: f32,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let meta = self.editor.page_list()[index].clone();
        let id = meta.id;
        let selected = self.pages_ui.organizer.selected.contains(&id);
        let focused = self.pages_ui.organizer.focus == Some(id);
        let active = self.editor.active_page() == id;
        let doc = &self.editor.page(id).unwrap().doc;
        let dimensions = format!("{} × {}", doc.width, doc.height);
        let image = self.page_thumbnail(id, 192, cx);
        let ids = if selected {
            self.selected_project_pages()
        } else {
            vec![id]
        };
        let drag = DraggedPages {
            owner: cx.entity_id().as_u64(),
            stamp: self.editor.stamp(),
            ids,
        };
        let accent = p.accent;
        div()
            .id(("organizer-page", id))
            .test_support()
            .aria_label(if selected {
                t!(
                    "editor.page_organizer.page_aria_selected",
                    number = index + 1,
                    name = meta.name
                )
            } else {
                t!(
                    "editor.page_organizer.page_aria",
                    number = index + 1,
                    name = meta.name
                )
            })
            .w(px(width))
            .h(px(220.))
            .flex_none()
            .p_2()
            .flex()
            .flex_col()
            .gap_2()
            .bg(if selected { p.soft_bg } else { p.panel })
            .rounded_md()
            .border_2()
            .border_color(if selected || focused {
                p.accent
            } else {
                p.line
            })
            .cursor_pointer()
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                window.focus(&this.focus, cx);
                this.select_organizer_page(
                    id,
                    event.modifiers().control || event.modifiers().platform,
                    event.modifiers().shift,
                    cx,
                );
                if event.click_count() >= 2 {
                    this.select_page(id, cx);
                    this.close_page_organizer(window, cx);
                }
                cx.stop_propagation();
            }))
            .on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
            .drag_over::<DraggedPages>(move |style, _, _, _| {
                style.border_color(accent).border_l(px(6.))
            })
            .on_drop(cx.listener(move |this, drag: &DraggedPages, _, cx| {
                this.drop_organizer_pages(drag, index, cx)
            }))
            .child(
                div()
                    .h(px(140.))
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(p.stage)
                    .rounded_sm()
                    .children(
                        image.map(|image| img(image).size_full().object_fit(ObjectFit::Contain)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        Button::new(("organizer-select", id))
                            .xsmall()
                            .ghost()
                            .label(if selected { "✓" } else { "○" })
                            .accessibility_label(if selected {
                                t!("editor.page_organizer.deselect_page", number = index + 1)
                            } else {
                                t!("editor.page_organizer.select_page", number = index + 1)
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                window.focus(&this.focus, cx);
                                this.select_organizer_page(id, true, false, cx);
                                cx.stop_propagation();
                            })),
                    )
                    .child(div().flex_1().min_w_0().text_sm().truncate().child(format!(
                        "{} · {}",
                        index + 1,
                        meta.name
                    ))),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_xs()
                    .text_color(p.muted)
                    .child(dimensions)
                    .child(if active {
                        t!("editor.page_organizer.editing")
                    } else {
                        "".into()
                    }),
            )
            .into_any_element()
    }
    pub(super) fn page_organizer(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.reconcile_page_selection();
        let count = self.editor.page_list().len();
        let selected = self.selected_project_pages();
        let n = selected.len();
        let width = (f32::from(window.viewport_size().width) - 64.).max(180.);
        let columns = ((width / 202.).floor() as usize).clamp(1, 10);
        let tile_width = ((width - 12. * (columns - 1) as f32) / columns as f32).clamp(160., 270.);
        let scroll = self.pages_ui.organizer.scroll.clone();
        let first = self
            .editor
            .page_list()
            .iter()
            .position(|p| selected.contains(&p.id));
        let last = self
            .editor
            .page_list()
            .iter()
            .rposition(|p| selected.contains(&p.id));
        let rows = count.div_ceil(columns);
        let list = uniform_list(
            "page-organizer-grid",
            rows + 1,
            cx.processor(move |this, range: Range<usize>, _, cx| {
                let p = theme::palette(cx);
                range
                    .map(|row| {
                        if row == rows {
                            return div()
                                .h(px(232.))
                                .p_2()
                                .child(
                                    div()
                                        .id("organizer-drop-end")
                                        .test_support()
                                        .h(px(52.))
                                        .w_full()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .border_1()
                                        .border_color(p.line)
                                        .rounded_md()
                                        .text_sm()
                                        .text_color(p.muted)
                                        .child(t!("editor.page_organizer.drop_end"))
                                        .drag_over::<DraggedPages>(move |style, _, _, _| {
                                            style.bg(p.soft_bg).border_color(p.accent)
                                        })
                                        .on_drop(cx.listener(
                                            move |this, drag: &DraggedPages, _, cx| {
                                                this.drop_organizer_pages(drag, count, cx)
                                            },
                                        )),
                                )
                                .into_any_element();
                        }
                        div()
                            .h(px(232.))
                            .px_2()
                            .flex()
                            .gap_3()
                            .children(
                                (row * columns..((row + 1) * columns).min(count))
                                    .map(|index| this.organizer_tile(index, tile_width, &p, cx))
                                    .collect::<Vec<_>>(),
                            )
                            .into_any_element()
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&scroll)
        .flex_1()
        .min_h_0();
        let heading = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .p_3()
            .bg(p.panel)
            .border_b_1()
            .border_color(p.line)
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t!("editor.page_organizer.pages")),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(p.muted)
                    .child(SharedString::from(t!(
                        "editor.page_organizer.selected_total",
                        n = n,
                        count = count
                    ))),
            )
            .child(
                Button::new("organizer-select-all")
                    .small()
                    .ghost()
                    .label(t!("editor.page_organizer.select_all"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.pages_ui.organizer.selected =
                            this.editor.page_list().iter().map(|p| p.id).collect();
                        cx.notify();
                    })),
            )
            .child(
                Button::new("organizer-select-none")
                    .small()
                    .ghost()
                    .label(t!("editor.page_organizer.clear"))
                    .disabled(n == 0)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.pages_ui.organizer.selected.clear();
                        cx.notify();
                    })),
            )
            .child(
                Button::new("organizer-close")
                    .small()
                    .outline()
                    .label(t!("editor.page_organizer.back"))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.close_page_organizer(window, cx)),
                    ),
            );
        let mut controls = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .bg(p.panel)
            .child(
                Button::new("organizer-duplicate")
                    .small()
                    .outline()
                    .label(t!("editor.page_organizer.duplicate"))
                    .disabled(n == 0)
                    .on_click(cx.listener(|this, _, _, cx| this.duplicate_organizer_pages(cx))),
            )
            .child(
                Button::new("organizer-delete")
                    .small()
                    .outline()
                    .label(t!("editor.page_organizer.delete"))
                    .disabled(n == 0 || n == count)
                    .tooltip(t!("editor.page_organizer.delete_tip"))
                    .on_click(cx.listener(|this, _, _, cx| this.delete_organizer_pages(cx))),
            )
            .child(
                Button::new("organizer-earlier")
                    .small()
                    .outline()
                    .label(t!("editor.page_organizer.earlier"))
                    .disabled(first.is_none_or(|i| i == 0))
                    .tooltip("Alt+Left")
                    .on_click(cx.listener(|this, _, _, cx| this.step_organizer_pages(false, cx))),
            )
            .child(
                Button::new("organizer-later")
                    .small()
                    .outline()
                    .label(t!("editor.page_organizer.later"))
                    .disabled(last.is_none_or(|i| i + 1 == count))
                    .tooltip("Alt+Right")
                    .on_click(cx.listener(|this, _, _, cx| this.step_organizer_pages(true, cx))),
            )
            .child(
                Button::new("organizer-undo")
                    .small()
                    .ghost()
                    .label(t!("edit.undo"))
                    .disabled(!self.editor.can_undo())
                    .on_click(cx.listener(|this, _, _, cx| this.undo(cx))),
            )
            .child(
                Button::new("organizer-redo")
                    .small()
                    .ghost()
                    .label(t!("edit.redo"))
                    .disabled(!self.editor.can_redo())
                    .on_click(cx.listener(|this, _, _, cx| this.redo(cx))),
            );
        for (index, format) in [Format::Png, Format::Pdf].into_iter().enumerate() {
            let label = if format == Format::Png {
                "PNG ZIP"
            } else {
                "PDF"
            };
            controls = controls.child(
                Button::new(("organizer-export", index))
                    .small()
                    .outline()
                    .label(t!(
                        "editor.page_organizer.export_selected",
                        format = label,
                        n = n
                    ))
                    .disabled(n == 0 || self.pages_ui.export_pending)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let ids = this.selected_project_pages();
                        this.export_project_selection(format, ids, cx);
                    })),
            );
        }
        div()
            .id("page-organizer")
            .test_support()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .bg(p.stage)
            .key_context("DesignPageOrganizer")
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                this.organizer_key(event, columns, window, cx);
            }))
            .child(heading)
            .child(controls)
            .child(
                div()
                    .px_3()
                    .py_2()
                    .text_xs()
                    .text_color(p.muted)
                    .child(t!("editor.page_organizer.hint")),
            )
            .child(
                div().px_3().pb_2().child(
                    Button::new("organizer-bleed")
                        .xsmall()
                        .ghost()
                        .label(if self.pages_ui.include_bleed {
                            t!("editor.page_organizer.bleed_on")
                        } else {
                            t!("editor.page_organizer.bleed_off")
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.pages_ui.include_bleed = !this.pages_ui.include_bleed;
                            cx.notify();
                        })),
                ),
            )
            .child(list)
            .into_any_element()
    }
}
#[cfg(test)]
#[path = "page_organizer_tests.rs"]
mod tests;
