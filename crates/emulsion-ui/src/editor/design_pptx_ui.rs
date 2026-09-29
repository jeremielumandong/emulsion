//! Editable local presentation interchange and explicit-click web hyperlinks.
use super::*;
use gpui_kit::component::WindowExt;
impl EditorView {
    pub(super) fn export_design_pptx(&mut self, all: bool, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(project) = self.editor.snapshot() else {
            return;
        };
        let selected = if all {
            project.pages.iter().map(|p| p.meta.id).collect()
        } else {
            vec![project.active]
        };
        let dir = self
            .editor
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| ".".into())
            });
        let rx = cx.prompt_for_new_path(&dir, Some(&format!("{}.pptx", self.name)));
        cx.spawn(async move|this,cx|{
            let Ok(Ok(Some(mut path)))=rx.await else{return;};path.set_extension("pptx");let output=path.clone();
            this.update(cx,|this,cx|this.set_status("Exporting editable PowerPoint slides…",false,cx)).ok();
            let result=cx.background_spawn(async move{emulsion_io::pptx::write(&project,&selected,&output)}).await;
            this.update(cx,|this,cx|match result{Ok(report)=>{let count=report.warnings.len();this.diagram_import_notes(report.warnings);this.set_status(format!("Exported {} editable slide(s) to {}. {count} compatibility note(s); review Import / export notes in the Export menu.",report.pages,path.display()),count>0,cx);},Err(e)=>this.set_status(format!("PowerPoint export failed: {e}"),true,cx)}).ok();
        }).detach();
    }
    pub(super) fn show_design_hyperlink(
        &mut self,
        node: NodeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use emulsion_core::design_interactions::Action;
        if !self.prepare_page_action(cx) {
            return;
        }
        let original = self
            .editor
            .doc
            .design
            .interactions
            .get(&node)
            .cloned()
            .unwrap_or_default();
        let initial = original
            .iter()
            .find_map(|a| {
                if let Action::Url { url } = a {
                    Some(url.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(initial));
        let error = cx.new(|_| String::new());
        let owner = cx.weak_entity();
        let page = self.editor.active_page();
        window.open_dialog(cx,move|dialog,_,cx|{let input=input.clone();let owner=owner.clone();let err=error.clone();let original=original.clone();dialog.title("Object web link").width(px(480.)).child(div().flex().flex_col().gap_2().child("HTTP(S) URL · opens only when clicked during presentation").child(Input::new(&input).id("design-hyperlink-url")).child("Leave empty to remove this object's web link. Other saved actions are preserved.").child(error.read(cx).clone())).footer(crate::widgets::form_dialog_footer("Save link")).on_ok(move|_,_,cx|{
            let value=input.read(cx).value().trim().to_string();owner.update(cx,|this,cx|{let result=(||{
                if this.editor.active_page()!=page||this.editor.doc.design.interactions.get(&node).cloned().unwrap_or_default()!=original{return Err("Object actions changed while this dialog was open. Reopen it.".to_string());}
                let mut actions=original.clone();actions.retain(|a|!matches!(a,Action::Url{..}));if !value.is_empty(){actions.insert(0,Action::Url{url:value.clone()});}
                emulsion_core::design_interactions::author_with_trigger(&mut this.editor,node,actions,None,Some(emulsion_core::design_interactions::Trigger::Click))?;this.after_change(cx);Ok(())
            })();match result{Ok(())=>true,Err(e)=>{err.update(cx,|v,cx|{*v=e;cx.notify();});false}}}).unwrap_or(false)
        })});
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use gpui::TestAppContext;
    use gpui_kit::test::TestWindowExt;
    #[gpui_kit::test]
    fn design_hyperlink_dialog_validates_saves_and_undoes_without_navigation(
        cx: &mut TestAppContext,
    ) {
        let mut doc = Document::new(300, 200);
        let id = Command::AddNode {
            node: Box::new(Node::group(0, "Link")),
            slot: emulsion_core::command::Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        let original = doc.clone();
        let (ws, cx) = crate::tests::open(cx, doc);
        let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
        cx.update(|w, cx| editor.update(cx, |this, cx| this.show_design_hyperlink(id, w, cx)));
        cx.run_until_parked();
        cx.update(|w, cx| w.click("design-hyperlink-url", cx));
        cx.simulate_input("javascript:alert(1)");
        cx.update(|w, cx| w.click("ok", cx));
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, original));
        cx.update(|w, cx| w.click("design-hyperlink-url", cx));
        cx.simulate_keystrokes("ctrl-a");
        cx.simulate_input("https://example.com/design");
        cx.update(|w, cx| w.click("ok", cx));
        cx.run_until_parked();
        cx.update(|_,cx|editor.update(cx,|this,_|{assert!(matches!(&this.editor.doc.design.interactions[&id][0],emulsion_core::design_interactions::Action::Url{url}if url=="https://example.com/design"));this.editor.undo();assert_eq!(this.editor.doc,original);}));
    }
}
