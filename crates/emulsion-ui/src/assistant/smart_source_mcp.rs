//! Live Smart source tools always act on the originating relay editor.
use super::*;
use emulsion_mcp::{ToolResult, smart_source_tools};
impl EditorView {
    pub(super) fn execute_smart_source_host_tool(
        &mut self,
        call: RelayCall,
        cx: &mut Context<Self>,
    ) {
        let generation = self.assistant.tool_generation;
        let action = match smart_source_tools::parse(&call.name, &call.arguments) {
            Ok(v) => v,
            Err(e) => {
                call.reply(ToolResult::error(e));
                self.complete_tool_work(generation, cx);
                return;
            }
        };
        let Some(workspace) = self
            .library_workspace
            .as_ref()
            .and_then(WeakEntity::upgrade)
        else {
            call.reply(ToolResult::error(
                "No live workspace is attached to this relay.",
            ));
            self.complete_tool_work(generation, cx);
            return;
        };
        let handle = workspace.read(cx).library_window;
        let owner = cx.entity();
        let weak = cx.weak_entity();
        let origin = owner.entity_id().as_u64();
        cx.defer(move |cx| {
            if owner.read(cx).assistant.tool_generation != generation {
                call.reply(ToolResult::error("The originating relay ended."));
                return;
            }
            let task = cx.update_window(handle, |_, window, cx| {
                workspace.update(cx, |ws, cx| ws.smart_source_task(owner, action, window, cx))
            });
            match task {
                Err(_) => {
                    call.reply(ToolResult::error(
                        "The originating workspace window closed.",
                    ));
                    weak.update(cx, |e, cx| e.complete_tool_work(generation, cx))
                        .ok();
                }
                Ok(task) => {
                    cx.spawn(async move |cx| {
                        let result = match task.await {
                            Ok(mut value) => {
                                value["origin_tab_id"] = serde_json::json!(origin);
                                ToolResult::text(value.to_string())
                            }
                            Err(e) => ToolResult::error(e),
                        };
                        call.reply(result);
                        weak.update(cx, |e, cx| e.complete_tool_work(generation, cx))
                            .ok();
                    })
                    .detach();
                }
            }
        });
    }
}
