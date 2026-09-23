//! User-selected tools share the standard rail's activation semantics.
use super::*;
use gpui_kit::component::{
    Disableable, Sizable,
    button::{Button, ButtonVariants},
};

#[derive(Clone)]
struct DraggedTool(&'static str);

impl Render for DraggedTool {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        div()
            .p_2()
            .bg(p.panel)
            .text_color(p.ink)
            .border_1()
            .border_color(p.line)
            .child(self.0)
    }
}

fn find_tool(id: &str) -> Option<rail::RailItem> {
    rail::GROUPS
        .iter()
        .flat_map(|group| group.iter())
        .find(|item| item.name == id)
        .copied()
}

impl EditorView {
    fn insert_toolbox_tool(&mut self, id: &str, before: Option<&str>, cx: &mut Context<Self>) {
        if find_tool(id).is_none() || before == Some(id) {
            return;
        }
        self.compact.tool_ids.retain(|name| name != id);
        let index = before
            .and_then(|name| self.compact.tool_ids.iter().position(|item| item == name))
            .unwrap_or(self.compact.tool_ids.len());
        self.compact.tool_ids.insert(index, id.to_owned());
        self.rail.flyout = None;
        cx.notify();
    }

    pub(super) fn toolbox_customizer(&mut self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let accent = p.accent;
        let mut chosen = div()
            .id("toolbox-selected")
            .max_h(rems(18.))
            .overflow_y_scroll()
            .test_support()
            .flex()
            .flex_col()
            .gap_1()
            .w_full()
            .min_h(rems(3.))
            .p_2()
            .border_1()
            .border_color(p.line)
            .drag_over::<DraggedTool>(move |style, _, _, _| style.bg(accent.opacity(0.12)))
            .on_drop(cx.listener(|this, item: &DraggedTool, _, cx| {
                this.insert_toolbox_tool(item.0, None, cx);
                cx.stop_propagation();
            }));
        if self.compact.tool_ids.is_empty() {
            chosen = chosen
                .child("Using the standard toolbox. Add or drag tools here to create your own.");
        }
        for (index, id) in self.compact.tool_ids.clone().iter().enumerate() {
            let Some(item) = find_tool(id) else {
                continue;
            };
            let name = item.name;
            let len = self.compact.tool_ids.len();
            chosen = chosen.child(
                div()
                    .id(SharedString::from(format!("toolbox-row-{name}")))
                    .test_support()
                    .flex()
                    .items_center()
                    .gap_1()
                    .on_drag(DraggedTool(name), |item, _, _, cx| cx.new(|_| item.clone()))
                    .drag_over::<DraggedTool>(move |style, _, _, _| {
                        style.border_t_2().border_color(accent)
                    })
                    .on_drop(cx.listener(move |this, item: &DraggedTool, _, cx| {
                        this.insert_toolbox_tool(item.0, Some(name), cx);
                        cx.stop_propagation();
                    }))
                    .child(rail::tool_icon(item.glyph).size_4().text_color(p.ink))
                    .child(div().flex_1().child(name))
                    .child(
                        Button::new(SharedString::from(format!("toolbox-up-{name}")))
                            .label("Up")
                            .xsmall()
                            .ghost()
                            .disabled(index == 0)
                            .tooltip(format!("Move {name} earlier"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(index) =
                                    this.compact.tool_ids.iter().position(|id| id == name)
                                    && index > 0
                                {
                                    this.compact.tool_ids.swap(index, index - 1);
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("toolbox-down-{name}")))
                            .label("Down")
                            .xsmall()
                            .ghost()
                            .disabled(index + 1 == len)
                            .tooltip(format!("Move {name} later"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(index) =
                                    this.compact.tool_ids.iter().position(|id| id == name)
                                    && index + 1 < this.compact.tool_ids.len()
                                {
                                    this.compact.tool_ids.swap(index, index + 1);
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("toolbox-remove-{name}")))
                            .label("Remove")
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.compact.tool_ids.retain(|id| id != name);
                                cx.notify();
                            })),
                    ),
            );
        }
        let mut available = div()
            .id("toolbox-available")
            .max_h(rems(18.))
            .overflow_y_scroll()
            .test_support()
            .flex()
            .flex_wrap()
            .gap_1();
        for item in rail::GROUPS.iter().flat_map(|group| group.iter()) {
            let name = item.name;
            let included = self.compact.tool_ids.iter().any(|id| id == name);
            available = available.child(
                div()
                    .id(SharedString::from(format!("toolbox-source-{name}")))
                    .test_support()
                    .on_drag(DraggedTool(name), |item, _, _, cx| cx.new(|_| item.clone()))
                    .child(
                        Button::new(SharedString::from(format!("toolbox-add-{name}")))
                            .label(name)
                            .small()
                            .outline()
                            .disabled(included)
                            .tooltip(format!(
                                "Add {name} to your toolbox, or drag it into position"
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.insert_toolbox_tool(name, None, cx)
                            })),
                    ),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .text_sm()
            .text_color(p.ink)
            .child("Your toolbox")
            .child(
                "Click or drag available tools to add them. Drag to reorder, or use Up and Down.",
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .items_start()
                    .child(div().flex_1().min_w_0().child("Your tools").child(chosen))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child("Available tools")
                            .child(available),
                    ),
            )
            .child(
                Button::new("toolbox-standard")
                    .label("Use standard toolbox")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.compact.tool_ids.clear();
                        this.rail.flyout = None;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    pub(super) fn custom_tool_rail(
        &mut self,
        horizontal: bool,
        available_length: f32,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let items: Vec<_> = self
            .compact
            .tool_ids
            .iter()
            .filter_map(|id| find_tool(id))
            .collect();
        let mut slots =
            (((available_length + 0.125) / 1.875).floor() as usize).clamp(1, items.len().max(1));
        if self.compact.tool_columns >= 2 {
            slots = slots.min(items.len().max(1).div_ceil(2));
        }
        let tracks = items.len().max(1).div_ceil(slots);
        let length = rems(slots as f32 * 1.875 - 0.125);
        let breadth = rems(tracks as f32 * 1.875 - 0.125);
        let mut tools = div()
            .id("custom-tool-rail")
            .test_support()
            .flex()
            .flex_none()
            .flex_wrap()
            .gap(rems(0.125))
            .when(horizontal, |d| d.flex_row().w(length).h(breadth))
            .when(!horizontal, |d| d.flex_col().h(length).w(breadth));
        for item in items {
            let active = self.rail_item_active(&item);
            tools = tools.child(
                Button::new(SharedString::from(format!("custom-tool-{}", item.name)))
                    .small()
                    .ghost()
                    .w(rems(1.75))
                    .h(rems(1.75))
                    .accessibility_label(item.name)
                    .tooltip(format!("{} ({})", item.name, item.key))
                    .when(active, |button| {
                        button.bg(p.soft_bg).border_1().border_color(p.accent)
                    })
                    .child(rail::tool_icon(item.glyph).size_4().text_color(p.ink))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.activate_tool_item(item, cx);
                        window.focus(&this.canvas_focus, cx);
                    })),
            );
        }
        tools.into_any_element()
    }
}
