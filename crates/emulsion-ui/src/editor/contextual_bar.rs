//! Stable canvas-adjacent actions for the current tool and editing target.
use super::*;
use gpui_kit::component::ActiveTheme;

impl EditorView {
    pub(super) fn contextual_taskbar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let title = self.active_tool_name().to_string();
        let mask_actions = self.mask_taskbar_actions(cx);
        let tool_actions = self.contextual_tool_actions(cx);
        div()
            .id("contextual-taskbar")
            .test_support()
            .flex()
            .flex_none()
            .flex_wrap()
            .items_center()
            .justify_center()
            .gap_1()
            .px_2()
            .py_1()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .mr_2()
                    .child(title),
            )
            .children(mask_actions)
            .children(tool_actions)
            .into_any_element()
    }
}
