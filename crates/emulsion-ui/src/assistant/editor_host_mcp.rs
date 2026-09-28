//! Native editor controls retain the relay's original editor/window identity.
use super::*;
use emulsion_mcp::{
    ToolResult,
    editor_host_tools::{self, Action},
};
impl EditorView {
    pub(super) fn execute_photo_source_host_tool(
        &mut self,
        call: RelayCall,
        cx: &mut Context<Self>,
    ) {
        let generation = self.assistant.tool_generation;
        let revision = self.editor.revision;
        if emulsion_mcp::photo_source_tools::READ_ONLY.contains(&call.name.as_str()) {
            let result = emulsion_mcp::photo_source_tools::execute(
                &mut self.editor,
                &call.name,
                &call.arguments,
            )
            .unwrap();
            call.reply(result);
            self.complete_tool_work(generation, cx);
            return;
        }
        if self.raw.is_pending() || self.editor.in_transaction() {
            call.reply(ToolResult::error("Finish the active edit first."));
            self.complete_tool_work(generation, cx);
            return;
        }
        let ticket = self.edit_ticket();
        let doc = self.editor.doc.clone();
        let name = call.name.clone();
        let args = call.arguments.clone();
        let readonly = emulsion_mcp::photo_source_tools::READ_ONLY.contains(&name.as_str());
        let origin = cx.entity_id().as_u64();
        cx.spawn(async move|this,cx|{let (doc,mut result)=cx.background_spawn(async move{let mut trial=emulsion_core::Editor::new(doc,None);trial.revision=revision;let result=emulsion_mcp::photo_source_tools::execute(&mut trial,&name,&args).unwrap();(trial.doc,result)}).await;
            let applied=this.update(cx,|this,cx|{if this.assistant.tool_generation!=generation||this.edit_ticket()!=ticket{return Err("The originating source changed while loading. Retry against its current revision.".to_owned());}
if !result.is_error&&!readonly{this.editor.commit_design_document(doc,"Edit image source")?;this.after_change(cx);}Ok(this.editor.revision)});
            match applied{Ok(Ok(revision))=>{if let Some(text)=result.content.first_mut().and_then(|v|v.get_mut("text")) && let Some(mut data)=text.as_str().and_then(|v|serde_json::from_str::<serde_json::Value>(v).ok()).filter(|v|v.is_object()){data["revision"]=serde_json::json!(revision);data["origin_tab_id"]=serde_json::json!(origin);*text=serde_json::Value::String(data.to_string());}},Ok(Err(e))=>result=ToolResult::error(e),Err(_)=>result=ToolResult::error("The originating editor closed.")};call.reply(result);this.update(cx,|this,cx|this.complete_tool_work(generation,cx)).ok();
        }).detach();
    }
    pub(super) fn execute_print_host_tool(&mut self, call: RelayCall, cx: &mut Context<Self>) {
        let generation = self.assistant.tool_generation;
        let options = match emulsion_mcp::print_tools::parse(&call.name, &call.arguments) {
            Ok(v) => v,
            Err(e) => {
                call.reply(ToolResult::error(e));
                self.complete_tool_work(generation, cx);
                return;
            }
        };
        let requires_document =
            matches!(call.name.as_str(), "preview_print_job" | "submit_print_job");
        if requires_document
            && (self.raw.is_pending()
                || (self.editor.in_transaction()
                    && !(self.assistant.running
                        && !self.assistant.native_tool_steps
                        && self.editor.transaction_depth() == 1))
                || options
                    .expected_revision
                    .is_some_and(|v| v != self.editor.revision))
        {
            call.reply(ToolResult::error(
                "The document changed or has an active edit. Read editor state and retry.",
            ));
            self.complete_tool_work(generation, cx);
            return;
        }
        let title = self.name.to_string();
        let revision = self.editor.revision;
        let page = self.editor.active_page();
        let origin = cx.entity_id().as_u64();
        let docs = if !requires_document {
            Vec::new()
        } else if self.editor.kind().is_some() {
            self.editor
                .page_list()
                .iter()
                .map(|p| (p.name.clone(), self.editor.page(p.id).unwrap().doc.clone()))
                .collect()
        } else {
            vec![(title.clone(), self.editor.doc.clone())]
        };
        let name = call.name.clone();
        cx.spawn(async move|this,cx|{let mut result=cx.background_spawn(async move{emulsion_mcp::print_tools::execute(&name,options,&title,docs)}).await;result.content.push(serde_json::json!({"type":"text","text":serde_json::json!({"origin_tab_id":origin,"source_page":page,"source_revision":revision}).to_string()}));call.reply(result);this.update(cx,|this,cx|this.complete_tool_work(generation,cx)).ok();}).detach();
    }
    pub(super) fn execute_editor_host_tool(&mut self, call: RelayCall, cx: &mut Context<Self>) {
        let generation = self.assistant.tool_generation;
        let action = match editor_host_tools::parse(&call.name, &call.arguments) {
            Ok(v) => v,
            Err(e) => {
                call.reply(ToolResult::error(e));
                self.complete_tool_work(generation, cx);
                return;
            }
        };
        if !matches!(action, Action::Print) {
            let result = match self.editor_host_action(action, cx) {
                Ok(v) => ToolResult::text(v.to_string()),
                Err(e) => ToolResult::error(e),
            };
            call.reply(result);
            self.complete_tool_work(generation, cx);
            return;
        }
        if self.editor.in_transaction() || self.raw.is_pending() {
            call.reply(ToolResult::error("Finish the active edit first."));
            self.complete_tool_work(generation, cx);
            return;
        }
        let Some(workspace) = self
            .library_workspace
            .as_ref()
            .and_then(WeakEntity::upgrade)
        else {
            call.reply(ToolResult::error("No live workspace window."));
            self.complete_tool_work(generation, cx);
            return;
        };
        let handle = workspace.read(cx).library_window;
        let owner = cx.weak_entity();
        let origin = cx.entity_id().as_u64();
        let name = self.name.to_string();
        let active = self
            .editor
            .page_list()
            .iter()
            .position(|p| p.id == self.editor.active_page())
            .unwrap_or(0);
        let docs = if self.editor.kind().is_some() {
            self.editor
                .page_list()
                .iter()
                .map(|p| (p.name.clone(), self.editor.page(p.id).unwrap().doc.clone()))
                .collect()
        } else {
            vec![(name.clone(), self.editor.doc.clone())]
        };
        cx.defer(move |cx| {
            let valid = owner
                .upgrade()
                .is_some_and(|e| e.read(cx).assistant.tool_generation == generation);
            let result = if valid {
                cx.update_window(handle, |_, window, cx| {
                    crate::print_dialog::open(name, docs, active, window, cx)
                })
                .map(|_| {
                    ToolResult::text(
                        serde_json::json!({"origin_tab_id":origin,"dialog":"print","opened":true})
                            .to_string(),
                    )
                })
                .unwrap_or_else(|_| ToolResult::error("The original window closed."))
            } else {
                ToolResult::error("The originating relay ended.")
            };
            call.reply(result);
            owner
                .update(cx, |this, cx| this.complete_tool_work(generation, cx))
                .ok();
        });
    }
}
