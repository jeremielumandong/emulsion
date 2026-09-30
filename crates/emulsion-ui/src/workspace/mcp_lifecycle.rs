//! Workspace tools operate on stable tab entities and never discard dirty work.
use super::*;
use emulsion_mcp::workspace_tools::Action;
use serde_json::{Value, json};
impl Workspace {
    pub(crate) fn mcp_tabs(&self, origin: u64, cx: &App) -> Value {
        json!({"origin_tab_id":origin,"active_tab_id":self.editor.as_ref().map(|e|e.entity_id().as_u64()),"tabs":self.tabs.iter().map(|entity|{
            let e=entity.read(cx);
            json!({"tab_id":entity.entity_id().as_u64(),"name":e.name,"path":e.editor.path,"source":e.source,"project_kind":e.editor.kind(),"canvas_kind":e.home_canvas_kind,"unsaved":e.has_unsaved_changes(),"origin":entity.entity_id().as_u64()==origin})
        }).collect::<Vec<_>>()})
    }
    pub(crate) fn workspace_mcp_action(
        &mut self,
        origin: u64,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        if !self.tabs.iter().any(|e| e.entity_id().as_u64() == origin) {
            return Err(
                "The originating relay tab has closed. Start a relay in an open editor.".into(),
            );
        }
        if !matches!(action, Action::List)
            && self
                .editor
                .as_ref()
                .is_some_and(|e| e.read(cx).editor.in_transaction())
        {
            return Err(
                "Finish the active tab's edit gesture before changing workspace tabs.".into(),
            );
        }
        let mut closed = None;
        match action {
            Action::List => {}
            Action::Open(_) => {
                return Err("File opening requires the asynchronous workspace host".into());
            }
            Action::Create(spec) => {
                use emulsion_core::creation::CanvasKind;
                if matches!(spec.kind, CanvasKind::Design | CanvasKind::Diagram) {
                    let session = spec.create_project()?;
                    self.install_project(session, spec.name, window, cx);
                } else {
                    let doc = spec.create()?;
                    self.install(doc, None, None, None, spec.name, window, cx);
                }
                if let Some(editor) = &self.editor {
                    editor.update(cx, |editor, cx| {
                        editor.home_canvas_kind = Some(spec.kind);
                        if editor.draw_mode != (spec.kind == CanvasKind::Paint) {
                            editor.toggle_draw_mode(cx);
                        }
                    });
                }
            }
            Action::Select(id) => {
                let i = self
                    .tabs
                    .iter()
                    .position(|e| e.entity_id().as_u64() == id)
                    .ok_or("Tab no longer exists.")?;
                self.activate_tab(i, window, cx);
            }
            Action::Close(id) => {
                let i = self
                    .tabs
                    .iter()
                    .position(|e| e.entity_id().as_u64() == id)
                    .ok_or("Tab no longer exists.")?;
                let editor = self.tabs[i].read(cx);
                if editor.has_unsaved_changes() {
                    return Err("This tab has unsaved changes. Save it in its owning editor before closing; MCP never discards unsaved work.".into());
                }
                if editor.editor.in_transaction() || editor.raw.is_pending() {
                    return Err("Finish pending edits before closing this tab.".into());
                }
                if id != origin && editor.workspace_mcp_busy() {
                    return Err(
                        "Wait for this tab's active assistant work before closing it.".into(),
                    );
                }
                self.close_tab(i, window, cx);
                closed = Some(id);
            }
        }
        let mut value = self.mcp_tabs(origin, cx);
        value["closed_tab_id"] = json!(closed);
        Ok(value)
    }
    pub(crate) fn install_mcp_file(
        &mut self,
        origin: u64,
        file: emulsion_mcp::workspace_tools::LoadedFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        self.workspace_mcp_action(origin, Action::List, window, cx)?;
        if self.busy.is_some()
            || self
                .editor
                .as_ref()
                .is_some_and(|e| e.read(cx).editor.in_transaction())
        {
            return Err("Finish the active workspace operation before opening the file".into());
        }
        let path = file.path;
        // Reopening a saved path activates its existing tab, including dirty projects.
        // install_project would otherwise replace that tab's entire session.
        if let Some(index) = self
            .tabs
            .iter()
            .position(|e| e.read(cx).editor.path.as_ref() == Some(&path))
        {
            self.activate_tab(index, window, cx);
            let mut result = self.mcp_tabs(origin, cx);
            result["opened_tab_id"] = result["active_tab_id"].clone();
            result["opened_path"] = json!(path);
            result["reused_existing_tab"] = json!(true);
            result["warnings"] = json!([]);
            return Ok(result);
        }
        let mut warnings = Vec::new();
        match file.content {
            emulsion_mcp::workspace_tools::FileContent::Project(session, notes) => {
                warnings = notes;
                self.install_project(*session, stem(&path), window, cx);
                if let Some(editor) = &self.editor {
                    editor.update(cx, |editor, _| {
                        editor.diagram_import_notes(warnings.clone())
                    });
                }
            }
            emulsion_mcp::workspace_tools::FileContent::Document(opened) => {
                let emulsion_io::Opened {
                    doc,
                    graph,
                    history_error,
                } = *opened;
                if let Some(error) = history_error {
                    warnings.push(format!("History could not be restored: {error}"));
                }
                let kind = file.kind.or_else(|| self.home_project_kind(&path));
                self.install(
                    doc,
                    graph,
                    emulsion_io::is_native(&path).then(|| path.clone()),
                    Some(path.clone()),
                    stem(&path),
                    window,
                    cx,
                );
                if let Some(editor) = &self.editor {
                    editor.update(cx, |editor, cx| {
                        editor.home_canvas_kind = kind;
                        if let Some(kind) = kind {
                            let paint = kind == emulsion_core::creation::CanvasKind::Paint;
                            if editor.draw_mode != paint {
                                editor.toggle_draw_mode(cx);
                            }
                        }
                    });
                }
            }
        }
        let mut result = self.mcp_tabs(origin, cx);
        result["opened_tab_id"] = result["active_tab_id"].clone();
        result["opened_path"] = json!(path);
        result["warnings"] = json!(warnings);
        Ok(result)
    }

    pub(crate) fn publish_creative_catalog(
        &mut self,
        catalog: emulsion_io::creative_library::Catalog,
        cx: &mut Context<Self>,
    ) {
        if catalog.revision >= self.home_state.projects.catalog.revision {
            self.home_state.projects.catalog = catalog.clone();
        }
        for editor in &self.tabs {
            editor.update(cx, |editor, cx| {
                editor.install_catalog(catalog.clone());
                cx.notify();
            });
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use emulsion_core::{Command, Node, NodeKind, command::Slot};
    #[gpui_kit::test]
    fn workspace_mcp_creates_photo_and_paint_with_native_settings(cx: &mut TestAppContext) {
        let (ws, cx) = crate::tests::open(cx, Document::new(100, 100));
        let origin = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
        let origin_id = origin.entity_id().as_u64();
        cx.update(|window, cx| {
            let original = origin.read(cx).editor.doc.clone();
            for kind in ["photo", "paint"] {
                let action = emulsion_mcp::workspace_tools::parse(
                    "create_canvas",
                    &json!({
                        "kind":kind,"name":"Canvas","width":2,"height":1,"unit":"inches",
                        "resolution":144,"depth":8,"background":"transparent"
                    }),
                )
                .unwrap();
                ws.update(cx, |ws, cx| {
                    let state = ws
                        .workspace_mcp_action(origin_id, action, window, cx)
                        .unwrap();
                    assert_eq!(state["origin_tab_id"], origin_id);
                    let editor = ws.editor.as_ref().unwrap().read(cx);
                    assert_eq!(editor.editor.doc.width, 288);
                    assert_eq!(editor.editor.doc.height, 144);
                    assert_eq!(editor.draw_mode, kind == "paint");
                    assert_eq!(
                        editor.home_canvas_kind.unwrap().label().to_lowercase(),
                        kind
                    );
                });
            }
            assert_eq!(ws.read(cx).tabs.len(), 3);
            assert_eq!(origin.read(cx).editor.doc, original);
        });
    }
    #[gpui_kit::test]
    fn workspace_mcp_create_select_close_preserves_origin_and_unsaved_work(
        cx: &mut TestAppContext,
    ) {
        let (ws, cx) = crate::tests::open(cx, Document::new(100, 100));
        let origin = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
        let origin_id = origin.entity_id().as_u64();
        cx.update(|window, cx| {
            origin.update(cx, |v, _| {
                v.editor
                    .execute(Command::AddNode {
                        node: Box::new(Node::new(0, "Unsaved", NodeKind::Fill { rgba: [255; 4] })),
                        slot: Slot::TOP,
                    })
                    .unwrap();
            });
            let original = origin.read(cx).editor.doc.clone();
            ws.update(cx, |ws, cx| {
                let action = emulsion_mcp::workspace_tools::parse(
                    "create_design_project",
                    &json!({"kind":"design","name":"Campaign","width":800,"height":600,"pages":2}),
                )
                .unwrap();
                let result = ws
                    .workspace_mcp_action(origin_id, action, window, cx)
                    .unwrap();
                assert_eq!(result["origin_tab_id"], origin_id);
                let new_id = result["active_tab_id"].as_u64().unwrap();
                assert_ne!(origin_id, new_id);
                assert_eq!(
                    ws.editor
                        .as_ref()
                        .unwrap()
                        .read(cx)
                        .editor
                        .page_list()
                        .len(),
                    2
                );
                assert_eq!(origin.read(cx).editor.doc, original);
                assert!(
                    ws.workspace_mcp_action(origin_id, Action::Close(origin_id), window, cx)
                        .unwrap_err()
                        .contains("unsaved")
                );
                assert_eq!(ws.tabs.len(), 2);
                let result = ws
                    .workspace_mcp_action(origin_id, Action::Select(origin_id), window, cx)
                    .unwrap();
                assert_eq!(result["active_tab_id"], origin_id);
                assert_eq!(result["origin_tab_id"], origin_id);
                // An editor with no authored changes and an acknowledged saved
                // project is eligible for the existing native close path.
                let other = ws
                    .tabs
                    .iter()
                    .find(|e| e.entity_id().as_u64() == new_id)
                    .unwrap()
                    .clone();
                other.update(cx, |v, _| {
                    let stamp = v.editor.stamp();
                    v.editor.mark_project_saved(
                        std::env::temp_dir().join("mcp-workspace-saved.emu"),
                        &stamp,
                    );
                });
                let result = ws
                    .workspace_mcp_action(origin_id, Action::Close(new_id), window, cx)
                    .unwrap();
                assert_eq!(result["closed_tab_id"], new_id);
                assert_eq!(ws.tabs.len(), 1);
                assert_eq!(origin.read(cx).editor.doc, original);
                assert!(
                    ws.workspace_mcp_action(new_id, Action::List, window, cx)
                        .is_err()
                );
            });
        });
    }
}
