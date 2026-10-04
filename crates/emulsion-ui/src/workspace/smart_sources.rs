//! Nested source tabs retain their original parent identity, even after tab switches.
use super::*;
use crate::editor::smart_source_ui::SourceSession;
use emulsion_mcp::smart_source_tools::Action;
use serde_json::{Value, json};
fn ready(e: &EditorView) -> Result<(), String> {
    e.smart_source_ready()
}
impl Workspace {
    /// Complete a source-open against both its original parent and the current
    /// installation target. Refusal must never repurpose an unrelated active tab.
    #[allow(clippy::too_many_arguments)]
    fn finish_open_smart_source(
        &mut self,
        origin: &Entity<EditorView>,
        node: emulsion_core::NodeId,
        expected: emulsion_core::NodeKind,
        page: emulsion_core::project::PageId,
        depth: usize,
        name: String,
        document: Document,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        if !self.tabs.contains(origin) {
            return Err("The source's parent tab closed.".into());
        }
        let parent = origin.read(cx);
        ready(parent)?;
        if parent.editor.active_page() != page
            || !crate::editor::smart_source_ui::same_source(
                parent.editor.doc.node(node).map(|n| &n.kind),
                &expected,
            )
        {
            return Err("The Smart source changed while opening. Retry.".into());
        }
        if let Some(active) = &self.editor
            && !active.update(cx, |e, cx| e.photo_transform_ready(cx))
        {
            return Err(
                "Apply or cancel the active transform before opening the Smart source.".into(),
            );
        }
        if let Some(index) = self.tabs.iter().position(|tab| {
            tab.read(cx).smart.source_session.as_ref().is_some_and(|s| {
                s.parent.entity_id() == origin.entity_id() && s.node == node && s.page == page
            })
        }) {
            let id = self.tabs[index].entity_id().as_u64();
            self.activate_tab(index, window, cx);
            return Ok(
                json!({"source_tab_id":id,"origin_tab_id":origin.entity_id().as_u64(),"existing":true}),
            );
        }
        if !self.install(
            document,
            None,
            None,
            None,
            format!("Source · {name}"),
            window,
            cx,
        ) {
            return Err(
                "The Smart source could not be installed; the active tab was retained.".into(),
            );
        }
        let child = self.editor.as_ref().expect("successful install").clone();
        child.update(cx, |e, cx| {
            e.smart.source_session = Some(SourceSession {
                parent: origin.downgrade(),
                node,
                page,
                expected,
                depth,
            });
            let revision = e.editor.revision;
            e.editor.mark_sidecar_saved(revision);
            cx.notify();
        });
        Ok(
            json!({"source_tab_id":child.entity_id().as_u64(),"origin_tab_id":origin.entity_id().as_u64(),"depth":depth}),
        )
    }

    pub(crate) fn smart_source_task(
        &mut self,
        origin: Entity<EditorView>,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<Value, String>> {
        cx.spawn_in(window,async move|this,cx|{
            if let Action::Inspect{node}=action{
                return origin.read_with(cx,|e,_|e.inspect_smart_source(node));
            }
            if let Action::Open{node}=action{
                let (doc,expected,page,depth,name)=this.update(cx,|ws,cx|{
                    if !ws.tabs.contains(&origin){return Err("The source's parent tab closed.".to_owned());}
                    let e=origin.read(cx);ready(e)?;
                    emulsion_core::smart_source::ensure_editable(&e.editor.doc,node)?;
                    let depth=e.smart.source_session.as_ref().map_or(1,|s|s.depth+1);
                    if depth>emulsion_core::smart_source::MAX_SOURCE_DEPTH{return Err("Smart source editors may nest up to eight levels.".into());}
                    Ok((e.editor.doc.clone(),e.editor.doc.node(node).unwrap().kind.clone(),e.editor.active_page(),depth,e.editor.doc.node(node).unwrap().name.clone()))
                }).map_err(|e|e.to_string())??;
                let document=cx.background_spawn(async move{emulsion_io::smart_source::open(&doc,node).map_err(|e|e.to_string())}).await?;
                return this.update_in(cx, |ws, window, cx| {
                    ws.finish_open_smart_source(&origin, node, expected, page, depth, name, document, window, cx)
                }).map_err(|e| e.to_string())?;
            }
            if matches!(action,Action::Apply){
                let (source,child_ticket,session,parent,parent_doc,parent_ticket)=this.update(cx,|ws,cx|{
                    if !ws.tabs.contains(&origin){return Err("The source tab closed.".to_owned());}
                    let e=origin.read(cx);ready(e)?;
                    let session=e.smart.source_session.clone().ok_or("This tab is not a Smart source editor.")?;
                    let parent=session.parent.upgrade().filter(|p|ws.tabs.contains(p)).ok_or("The parent tab closed. Save this source separately to retain it.")?;
                    let p=parent.read(cx);ready(p)?;
                    if p.editor.active_page()!=session.page{return Err("Switch the parent to the original page before applying its source.".into());}
                    if !crate::editor::smart_source_ui::same_source(p.editor.doc.node(session.node).map(|n|&n.kind),&session.expected){return Err("The parent's source changed or was undone. Reopen its source; this tab remains available to save separately.".into());}
                    Ok((e.editor.doc.clone(),e.edit_ticket(),session,parent.clone(),p.editor.doc.clone(),p.edit_ticket()))
                }).map_err(|e|e.to_string())??;
                let node=session.node;
                let updated=cx.background_spawn(async move{let mut trial=emulsion_core::Editor::new(parent_doc,None);emulsion_io::smart_source::apply(&mut trial,node,&source).map_err(|e|e.to_string())?;Ok::<_,String>(trial.doc)}).await?;
                return this.update(cx,|ws,cx|{
                    if !ws.tabs.contains(&origin)||!ws.tabs.contains(&parent){return Err("A source or parent tab closed before Apply completed.".to_owned());}
                    if origin.read(cx).edit_ticket()!=child_ticket{return Err("The source changed during Apply. Save again to apply the latest edits.".into());}
                    let expected=parent.update(cx,|p,cx|{ready(p)?;if p.edit_ticket()!=parent_ticket||p.editor.active_page()!=session.page{return Err("The parent changed during Apply. Retry against its current state.".to_owned());}p.editor.commit_design_document(updated,"Edit Smart Object source")?;p.after_change(cx);Ok(p.editor.doc.node(node).unwrap().kind.clone())})?;
                    origin.update(cx,|e,cx|{if let Some(s)=&mut e.smart.source_session{s.expected=expected;}let revision=e.editor.revision;e.editor.mark_sidecar_saved(revision);e.set_status("Source applied to parent. External files were not changed.",false,cx);});
                    Ok(json!({"source_tab_id":origin.entity_id().as_u64(),"parent_tab_id":parent.entity_id().as_u64(),"node":node,"applied":true,"external_written":false}))
                }).map_err(|e|e.to_string())?;
            }
            let node=action.node().ok_or("Missing Smart source target.")?;
            let (doc,ticket,page)=this.update(cx,|ws,cx|{if !ws.tabs.contains(&origin){return Err("The originating tab closed.".to_owned());}let e=origin.read(cx);ready(e)?;emulsion_core::smart_source::ensure_editable(&e.editor.doc,node)?;Ok((e.editor.doc.clone(),e.edit_ticket(),e.editor.active_page()))}).map_err(|e|e.to_string())??;
            let writes=matches!(action,Action::Write{..}|Action::SaveAs{..});
            let updated=cx.background_spawn(async move{let mut trial=emulsion_core::Editor::new(doc,None);emulsion_mcp::smart_source_tools::execute(&mut trial,&action)?;Ok::<_,String>(trial.doc)}).await?;
            this.update(cx,|ws,cx|{
                if !ws.tabs.contains(&origin){return Err(if writes{"The explicit external save completed, but its editor closed before link metadata could be updated."}else{"The originating tab closed."}.to_owned());}
                origin.update(cx,|e,cx|{ready(e)?;if e.edit_ticket()!=ticket||e.editor.active_page()!=page{return Err(if writes{"External save completed from the captured source. The page changed; inspect/relink before writing again."}else{"The source changed during loading. Retry refresh/relink."}.to_owned());}e.editor.commit_design_document(updated,"Update Smart source")?;e.smart.source_watch_facts.remove(&node);e.after_change(cx);e.set_status("Smart source updated.",false,cx);let mut result=e.inspect_smart_source(node)?;result["external_written"]=json!(writes);Ok(result)})
            }).map_err(|e|e.to_string())?
        })
    }
}

#[cfg(test)]
#[path = "smart_source_tests.rs"]
mod tests;
