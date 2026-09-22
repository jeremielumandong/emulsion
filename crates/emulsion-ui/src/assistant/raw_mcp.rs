//! Live RAW tools: retain workspace ownership, serialize work, reject stale results.
use super::*;
use emulsion_io::raw_settings::{RawSettingsGroup, merge_settings};
use emulsion_mcp::{ToolResult, raw_preview::Comparison};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SyncRequest {
    targets: Vec<u64>,
    #[serde(default = "all")]
    group: String,
}
fn all() -> String {
    "all".into()
}

impl EditorView {
    pub(crate) fn raw_documents_result(&self, cx: &Context<Self>) -> ToolResult {
        let mut documents = Vec::new();
        let mut add = |view: &EditorView, id: u64, current: bool| {
            if let Some(raw) = &view.editor.doc.raw {
                documents.push(
                    json!({"id":id,"name":view.name,"current":current,"camera":raw.metadata,
                    "pending":view.raw.is_pending(),"params":raw.params}),
                );
            }
        };
        add(self, cx.entity_id().as_u64(), true);
        for peer in &self.raw_peers {
            if let Some(peer) = peer.upgrade() {
                add(peer.read(cx), peer.entity_id().as_u64(), false);
            }
        }
        ToolResult::text(json!({"documents":documents}).to_string())
    }

    pub(super) fn execute_raw_host_tool(&mut self, call: RelayCall, cx: &mut Context<Self>) {
        let generation = self.assistant.tool_generation;
        if call.name == "list_raw_documents" {
            let result = if call.arguments.as_object().is_some_and(|a| a.is_empty()) {
                self.raw_documents_result(cx)
            } else {
                ToolResult::error("list_raw_documents takes no arguments")
            };
            call.reply(result);
            self.complete_tool_work(generation, cx);
            return;
        }
        if call.name == "set_raw_comparison" {
            let pending = Comparison::parse(&call.arguments).and_then(|request| {
                self.raw_comparison_wait(&request, cx)
                    .map_err(ToolResult::error)
            });
            let (wait, revision) = match pending {
                Ok(value) => value,
                Err(error) => {
                    call.reply(error);
                    self.complete_tool_work(generation, cx);
                    return;
                }
            };
            cx.spawn(async move |this, cx| {
                let _ = wait.recv().await;
                this.update(cx, |this, cx| {
                    let result = if this.assistant.tool_generation != generation {
                        ToolResult::error("Assistant request ended during RAW comparison")
                    } else {
                        match this.raw_comparison_result(revision) {
                            Ok(()) => ToolResult::text(
                                "RAW comparison updated; saved edits and history unchanged.",
                            ),
                            Err(error) => ToolResult::error(error),
                        }
                    };
                    call.reply(result);
                    this.complete_tool_work(generation, cx);
                })
                .ok();
            })
            .detach();
            return;
        }
        let prepared = self.prepare_raw_sync(&call.arguments, cx);
        let targets = match prepared {
            Ok(targets) => targets,
            Err(error) => {
                call.reply(error);
                self.complete_tool_work(generation, cx);
                return;
            }
        };
        let source_ticket = self.edit_ticket();
        cx.spawn(async move |this,cx| {
            let mut results = Vec::new();
            let mut failed = false;
            for (target, doc, ticket, params) in targets {
                let id = target.entity_id().as_u64();
                let current = this.update(cx, |source,_| {
                    source.assistant.tool_generation == generation && source.edit_ticket() == source_ticket
                        && !source.raw.is_pending() && source.raw_peers.iter().any(|p| p.entity_id() == target.entity_id())
                }).unwrap_or(false);
                if !current {
                    failed = true;
                    results.push(json!({"id":id,"error":"Source changed, request ended, or target closed; skipped"}));
                    continue;
                }
                let planned = cx.background_spawn(async move {
                    exec::plan_heavy(&doc,"develop_raw",&json!({"settings":params}))
                }).await;
                // Revalidate the source and membership after expensive development as well.
                let current = this.update(cx, |source,_| {
                    source.assistant.tool_generation == generation && source.edit_ticket() == source_ticket
                        && !source.raw.is_pending() && source.raw_peers.iter().any(|p| p.entity_id() == target.entity_id())
                }).unwrap_or(false);
                let result = if !current {
                    ToolResult::error("Source changed, request ended, or target closed; result discarded")
                } else {
                    target.update(cx, |target,cx| {
                        if !target.edit_is_current(ticket) || target.raw.is_pending() || target.editor.in_transaction() || target.assistant.tool_busy {
                            return ToolResult::error("Target changed or is busy; result discarded");
                        }
                        match planned {
                            Err(error) => error,
                            Ok(plan) => {
                                let result = exec::apply(&mut target.editor,plan);
                                target.after_change(cx);
                                result
                            }
                        }
                    }).unwrap_or_else(|_| ToolResult::error("Target closed"))
                };
                failed |= result.is_error;
                results.push(json!({"id":id,"is_error":result.is_error,"content":result.content}));
            }
            let result = ToolResult {content:vec![json!({"type":"text","text":json!({"results":results}).to_string()})],is_error:failed};
            call.reply(result);
            this.update(cx, |this,cx| this.complete_tool_work(generation,cx)).ok();
        }).detach();
    }

    fn prepare_raw_sync(
        &self,
        args: &Value,
        cx: &Context<Self>,
    ) -> Result<Vec<RawTarget>, ToolResult> {
        if !args.is_object() {
            return Err(ToolResult::error("Arguments must be an object"));
        }
        let request: SyncRequest =
            serde_json::from_value(args.clone()).map_err(|e| ToolResult::error(e.to_string()))?;
        let group = match request.group.as_str() {
            "all" => RawSettingsGroup::All,
            "white_balance" => RawSettingsGroup::WhiteBalance,
            "tone" => RawSettingsGroup::Tone,
            "curve" => RawSettingsGroup::Curve,
            _ => {
                return Err(ToolResult::error(
                    "group must be all, white_balance, tone or curve",
                ));
            }
        };
        if request.targets.is_empty() || request.targets.len() > 64 {
            return Err(ToolResult::error(
                "Choose between 1 and 64 target document IDs",
            ));
        }
        if self.raw.is_pending() {
            return Err(ToolResult::error("Finish pending source RAW edits first"));
        }
        let source = self
            .editor
            .doc
            .raw
            .as_ref()
            .ok_or_else(|| ToolResult::error("Source has no editable RAW"))?;
        let mut seen = std::collections::HashSet::new();
        let mut targets = Vec::new();
        for id in request.targets {
            if !seen.insert(id) {
                return Err(ToolResult::error("Duplicate target document ID"));
            }
            let target = self
                .raw_peers
                .iter()
                .find(|p| p.entity_id().as_u64() == id)
                .and_then(WeakEntity::upgrade)
                .ok_or_else(|| {
                    ToolResult::error(format!(
                        "Document {id} is not another open tab; call list_raw_documents"
                    ))
                })?;
            let view = target.read(cx);
            let raw =
                view.editor.doc.raw.as_ref().ok_or_else(|| {
                    ToolResult::error(format!("Document {id} has no editable RAW"))
                })?;
            if view.raw.is_pending() || view.editor.in_transaction() || view.assistant.tool_busy {
                return Err(ToolResult::error(format!(
                    "Document {id} has an edit in progress"
                )));
            }
            if source.params.wb_override.is_some()
                && matches!(
                    group,
                    RawSettingsGroup::All | RawSettingsGroup::WhiteBalance
                )
                && !(source
                    .metadata
                    .make
                    .trim()
                    .eq_ignore_ascii_case(raw.metadata.make.trim())
                    && source
                        .metadata
                        .model
                        .trim()
                        .eq_ignore_ascii_case(raw.metadata.model.trim()))
            {
                return Err(ToolResult::error(format!(
                    "Document {id}: sampled white balance requires the same camera model"
                )));
            }
            targets.push((
                target.downgrade(),
                view.editor.doc.clone(),
                view.edit_ticket(),
                merge_settings(raw.params, source.params, group),
            ));
        }
        Ok(targets)
    }
}

type RawTarget = (
    WeakEntity<EditorView>,
    Document,
    (u64, u64),
    emulsion_core::raw::DevelopParams,
);
