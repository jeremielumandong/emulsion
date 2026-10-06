//! Project-aware relay dispatch: stable page ownership and asynchronous file IO.
use super::*;
use emulsion_mcp::{ToolResult, project_tools};
use serde_json::json;

impl EditorView {
    pub(super) fn execute_project_host_tool(&mut self, call: RelayCall, cx: &mut Context<Self>) {
        let generation = self.assistant.tool_generation;
        if !project_tools::READ_ONLY.contains(&call.name.as_str())
            && (self.raw.is_pending() || self.editor.in_transaction())
        {
            call.reply(ToolResult::error(
                "Finish the active edit or RAW development before changing project pages.",
            ));
            self.complete_tool_work(generation, cx);
            return;
        }
        if matches!(call.name.as_str(), "undo" | "redo") {
            if !call.arguments.as_object().is_some_and(|a| a.is_empty()) {
                call.reply(ToolResult::error("Undo and Redo take no arguments"));
            } else {
                let changed = if call.name == "undo" {
                    self.editor.undo()
                } else {
                    self.editor.redo()
                };
                if changed {
                    self.after_change(cx);
                }
                call.reply(ToolResult::text(
                    json!({"changed":changed,"active_page":self.editor.active_page()}).to_string(),
                ));
            }
            self.complete_tool_work(generation, cx);
            return;
        }
        let mut name = call.name.clone();
        let mut args = call.arguments.clone();
        if name == "save_document" {
            name = "save_project".into();
            if args.get("path").is_none() {
                if let Some(path) = &self.editor.path {
                    args["path"] = json!(path);
                } else {
                    call.reply(ToolResult::error(
                        "Pass a .emu path to save every project page.",
                    ));
                    self.complete_tool_work(generation, cx);
                    return;
                }
            }
        }
        if let Err(error) = project_tools::validate_args(&name, &args) {
            call.reply(ToolResult::error(error));
            self.complete_tool_work(generation, cx);
            return;
        }
        if name == "generate_design_pages" {
            let Some(project) = self.editor.snapshot() else {
                call.reply(ToolResult::error("Open a Design project first."));
                self.complete_tool_work(generation, cx);
                return;
            };
            let stamp = self.editor.stamp();
            let ticket = self.edit_ticket();
            cx.spawn(async move|this,cx| {
                let generated=cx.background_spawn(async move {project_tools::generate_data_snapshot(&project,&args)}).await;
                this.update(cx,|this,cx| {
                    let result=if this.assistant.tool_generation!=generation || !this.edit_is_current(ticket) || this.editor.stamp()!=stamp || this.editor.in_transaction() {
                        ToolResult::error("Project changed or request ended during generation; no pages were inserted.")
                    } else {
                        match generated.and_then(|project|this.editor.import_pages(project)) {
                            Ok(pages)=>{this.after_change(cx);ToolResult::text(json!({"active_page":this.editor.active_page(),"result":{"pages":pages}}).to_string())},
                            Err(error)=>ToolResult::error(error),
                        }
                    };
                    call.reply(result);this.complete_tool_work(generation,cx);
                }).ok();
            }).detach();
            return;
        }
        if name == "import_project_pages" {
            if self.editor.kind().is_none() {
                call.reply(ToolResult::error(
                    "Open a Design or Diagram project before importing pages.",
                ));
                self.complete_tool_work(generation, cx);
                return;
            }
            let ticket = self.edit_ticket();
            let stamp = self.editor.stamp();
            cx.spawn(async move |this,cx| {
                let loaded=cx.background_spawn(async move {project_tools::load_pages(&args)}).await;
                this.update(cx, |this,cx| {
                    let result=if this.assistant.tool_generation != generation || !this.edit_is_current(ticket) || this.editor.stamp()!=stamp || this.editor.in_transaction() {
                        ToolResult::error("Project changed or request ended during import; nothing was inserted. Retry after inspecting the project.")
                    } else {
                        match loaded.and_then(|(project,warnings)| this.editor.import_pages(project).map(|pages|(pages,warnings))) {
                            Ok((pages,warnings)) => {this.after_change(cx); ToolResult::text(json!({"pages":pages,"warnings":warnings,"active_page":this.editor.active_page()}).to_string())},
                            Err(error)=>ToolResult::error(error),
                        }
                    };
                    call.reply(result);
                    this.complete_tool_work(generation,cx);
                }).ok();
            }).detach();
            return;
        }
        if project_tools::IO_TOOLS.contains(&name.as_str()) {
            let Some(project) = self.editor.snapshot() else {
                call.reply(ToolResult::error("Open a Design or Diagram project first."));
                self.complete_tool_work(generation, cx);
                return;
            };
            let stamp = self.editor.stamp();
            let save = name == "save_project";
            let path = PathBuf::from(args["path"].as_str().unwrap());
            cx.spawn(async move |this, cx| {
                let written = cx
                    .background_spawn(async move {
                        project_tools::write_snapshot(&project, &name, &args)
                    })
                    .await;
                let success = written.is_ok();
                let notice = completed_save_notice(save, &written);
                let result = match written {
                    Ok(value) => ToolResult::text(value.to_string()),
                    Err(error) => ToolResult::error(error),
                };
                // Report actual file publication even if the originating editor closes.
                call.reply(result);
                this.update(cx, |this, cx| {
                    if save && success {
                        this.editor.mark_project_saved(path, &stamp);
                        if let Some(notice) = notice {
                            // Successful tool cards omit their result text, and turn
                            // completion replaces the footer with a cost summary.
                            // Keep the notice in this turn's visible transcript too.
                            if this.assistant.tool_generation == generation
                                && let Some(turn) = &mut this.assistant.turn
                            {
                                if !turn.text.is_empty() {
                                    turn.text.push_str("\n\n");
                                }
                                turn.text.push_str(&notice);
                                turn.text_break = true;
                            }
                            this.set_status(notice, false, cx);
                        }
                        cx.notify();
                    }
                    this.complete_tool_work(generation, cx);
                })
                .ok();
            })
            .detach();
            return;
        }
        let before = self.editor.stamp();
        let active = self.editor.active_page();
        let result = project_tools::execute(&mut self.editor, &name, &args);
        if self.editor.stamp() != before || self.editor.active_page() != active {
            self.after_change(cx);
        }
        call.reply(result);
        self.complete_tool_work(generation, cx);
    }
}

/// Only a successfully published native save can disclose its saved format.
/// This consumes the writer's result, not a later snapshot of the open editor.
fn completed_save_notice(save: bool, written: &Result<Value, String>) -> Option<String> {
    if !save {
        return None;
    }
    let version = written.as_ref().ok()?["project_format_version"].as_u64()?;
    crate::workspace::save_notice::project_notice(u32::try_from(version).ok()?)
}

#[cfg(test)]
#[path = "project_save_tests.rs"]
mod tests;
