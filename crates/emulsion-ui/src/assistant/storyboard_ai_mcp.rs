//! run_storyboard_ai from the assistant: panels and settings are read on the
//! UI thread, the models or provider run on a worker with the AI progress
//! card, and the result lands as one Undo step unless the request ended.
use super::*;
use emulsion_ai::jobs::Job;
use emulsion_mcp::ToolResult;
use emulsion_mcp::storyboard_tools::ai;

impl EditorView {
    pub(super) fn execute_storyboard_ai_tool(&mut self, call: RelayCall, cx: &mut Context<Self>) {
        let generation = self.assistant.tool_generation;
        let prepared = emulsion_mcp::storyboard_tools::validate_args(&call.name, &call.arguments)
            .and_then(|_| ai::prepare(&self.editor, &call.arguments));
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                call.reply(ToolResult::error(error));
                self.complete_tool_work(generation, cx);
                return;
            }
        };
        let job = Job::new();
        self.watch_job(job.clone(), "AI on storyboard panels", cx);
        cx.spawn(async move |this, cx| {
            let worker = job.clone();
            let computed = cx
                .background_spawn(async move { prepared.compute(&worker) })
                .await;
            this.update(cx, |this, cx| {
                let result = if this.assistant.tool_generation != generation {
                    ToolResult::error(
                        "the assistant request ended while this was computing; nothing was changed",
                    )
                } else {
                    match ai::finish(&mut this.editor, computed) {
                        Ok(value) => ToolResult::text(value.to_string()),
                        Err(error) => ToolResult::error(error),
                    }
                };
                if result.is_error {
                    cx.notify();
                } else {
                    this.after_change(cx);
                }
                call.reply(result);
                this.complete_tool_work(generation, cx);
            })
            .ok();
        })
        .detach();
    }
}
