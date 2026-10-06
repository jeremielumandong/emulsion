//! Defer workspace mutations until the originating editor lease is released.
use super::*;
use emulsion_mcp::{ToolResult, workspace_tools};
impl EditorView {
    pub(crate) fn workspace_mcp_busy(&self) -> bool {
        self.assistant.running || self.assistant.tool_busy || !self.assistant.tool_queue.is_empty()
    }
    pub(super) fn execute_workspace_host_tool(&mut self, call: RelayCall, cx: &mut Context<Self>) {
        let generation = self.assistant.tool_generation;
        let action = match workspace_tools::parse(&call.name, &call.arguments) {
            Ok(action) => action,
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
        let owner = cx.weak_entity();
        let origin = cx.entity_id().as_u64();
        if let workspace_tools::Action::Open(request) = action {
            cx.spawn(async move |this, cx| {
                let loaded = cx.background_spawn(async move { workspace_tools::load_file(request) }).await;
                let result = match loaded {
                    Err(error) => ToolResult::error(error),
                    Ok(file) => {
                        let valid = this.update(cx, |e, _| e.assistant.tool_generation == generation
                            && e.library_workspace.as_ref().is_some_and(|w| w.entity_id() == workspace.entity_id())).unwrap_or(false);
                        if !valid {
                            ToolResult::error("The originating relay ended or moved while opening the file; no tab was added")
                        } else {
                            match cx.update_window(handle, |_, window, cx| workspace.update(cx, |ws, cx| ws.install_mcp_file(origin, file, window, cx))) {
                                Ok(Ok(value)) => ToolResult::text(value.to_string()),
                                Ok(Err(error)) => ToolResult::error(error),
                                Err(_) => ToolResult::error("The original workspace window closed"),
                            }
                        }
                    }
                };
                call.reply(result);
                this.update(cx, |this, cx| this.complete_tool_work(generation, cx)).ok();
            }).detach();
            return;
        }
        cx.defer(move |cx| {
            let valid = owner.upgrade().is_some_and(|e| {
                let e = e.read(cx);
                e.assistant.tool_generation == generation
                    && e.library_workspace
                        .as_ref()
                        .is_some_and(|w| w.entity_id() == workspace.entity_id())
            });
            let result = if !valid {
                ToolResult::error(
                    "The originating relay ended or moved before the workspace action ran.",
                )
            } else {
                match cx.update_window(handle, |_, window, cx| {
                    workspace.update(cx, |ws, cx| {
                        ws.workspace_mcp_action(origin, action, window, cx)
                    })
                }) {
                    Ok(Ok(value)) => ToolResult::text(value.to_string()),
                    Ok(Err(e)) => ToolResult::error(e),
                    Err(_) => ToolResult::error("The original workspace window closed."),
                }
            };
            call.reply(result);
            owner
                .update(cx, |this, cx| this.complete_tool_work(generation, cx))
                .ok();
        });
    }
    pub(super) fn execute_creative_catalog_tool(
        &mut self,
        call: RelayCall,
        cx: &mut Context<Self>,
    ) {
        let generation = self.assistant.tool_generation;
        let workspace = self.library_workspace.clone();
        let origin = cx.entity_id().as_u64();
        cx.spawn(async move|this,cx| {
            let name=call.name.clone();let args=call.arguments.clone();
            let (mut result,catalog)=cx.background_spawn(async move {
                let root=emulsion_io::creative_library::root();
                let result=emulsion_mcp::creative_catalog_tools::execute(&root,&name,&args);
                let catalog=emulsion_io::creative_library::load(&root).ok();(result,catalog)
            }).await;
            if let (Some(workspace),Some(catalog))=(workspace,catalog) {workspace.update(cx,|workspace,cx|workspace.publish_creative_catalog(catalog,cx)).ok();}
            result.content.push(serde_json::json!({"type":"text","text":serde_json::json!({"origin_tab_id":origin}).to_string()}));
            call.reply(result);this.update(cx,|this,cx|this.complete_tool_work(generation,cx)).ok();
        }).detach();
    }
}

impl EditorView {
    pub(super) fn execute_design_brand_tool(&mut self, call: RelayCall, cx: &mut Context<Self>) {
        let generation = self.assistant.tool_generation;
        let ticket = self.edit_ticket();
        if self.editor.in_transaction() {
            call.reply(ToolResult::error(
                "Finish the current edit before applying brand assets.",
            ));
            self.complete_tool_work(generation, cx);
            return;
        }
        let doc = self.editor.doc.clone();
        let stamp = self.editor.stamp();
        cx.spawn(async move|this,cx|{let name=call.name.clone();let args=call.arguments.clone();let (result,doc)=cx.background_spawn(async move{let mut editor=match emulsion_core::Editor::try_new(doc.clone(),None){Ok(editor)=>editor,Err(error)=>return (ToolResult::error(error.to_string()),doc)};let result=emulsion_mcp::design_brand_tools::execute(&mut editor,&name,&args).unwrap_or_else(||ToolResult::error("Unknown brand tool"));(result,editor.doc)}).await;
            let result=this.update(cx,|this,cx|{
                if this.assistant.tool_generation!=generation||!this.edit_is_current(ticket)||this.editor.stamp()!=stamp {return ToolResult::error("The originating document changed while loading brand assets. Nothing was applied; inspect and retry.");}
                if !result.is_error && this.editor.doc!=doc {if let Err(e)=this.editor.commit_design_document(doc,"Apply brand asset"){return ToolResult::error(e);}this.after_change(cx);}result
            }).unwrap_or_else(|_|ToolResult::error("The originating editor closed before brand assets could be applied."));
            call.reply(result);this.update(cx,|this,cx|this.complete_tool_work(generation,cx)).ok();
        }).detach();
    }
}
