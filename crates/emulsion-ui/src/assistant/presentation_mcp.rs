//! Live presentation commands are ordered with other assistant work and resolve
//! their audience window after releasing the editor entity's current lease.
use super::*;
use emulsion_mcp::{ToolResult, design_motion_tools::parse_host_action};

impl EditorView {
    pub(super) fn execute_presentation_host_tool(
        &mut self,
        call: RelayCall,
        cx: &mut Context<Self>,
    ) {
        let generation = self.assistant.tool_generation;
        let action = match parse_host_action(&call.name, &call.arguments) {
            Ok(action) => action,
            Err(error) => {
                call.reply(ToolResult::error(error));
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
                "Presentation requires an editor attached to an Emulsion workspace window",
            ));
            self.complete_tool_work(generation, cx);
            return;
        };
        let handle = workspace.read(cx).library_window;
        let workspace = workspace.downgrade();
        let ticket = self.edit_ticket();
        let presentation_ticket = self.presentation_runtime_ticket();
        let owner = cx.weak_entity();
        cx.defer(move |cx| {
            // Keep ownership outside the update closure so closed windows or
            // entities still receive an explicit error and release the queue.
            let result = cx.update_window(handle, |_, window, cx| {
                owner.update(cx, |this, cx| {
                    if this.assistant.tool_generation != generation || this.edit_ticket() != ticket || this.presentation_runtime_ticket() != presentation_ticket {
                        return Err("Document changed or assistant request ended before presentation command ran".into());
                    }
                    if !this.visible || this.library_workspace.as_ref().map(|workspace| workspace.entity_id()) != Some(workspace.entity_id()) {
                        return Err("The presentation editor is no longer visible in its original workspace".into());
                    }
                    this.presentation_host_action(action, window, cx)
                })
            });
            let result = match result {
                Ok(Ok(Ok(value))) => ToolResult::text(value.to_string()),
                Ok(Ok(Err(error))) => ToolResult::error(error),
                _ => ToolResult::error("The presentation window or editor closed before the command completed"),
            };
            call.reply(result);
            owner.update(cx, |this, cx| this.complete_tool_work(generation, cx)).ok();
        });
    }
}
