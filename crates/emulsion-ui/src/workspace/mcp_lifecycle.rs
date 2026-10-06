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
                if spec.is_project() {
                    let preferences = &crate::app_state::settings(cx).storyboard;
                    let session = spec.create_project_with(preferences)?;
                    if !self.install_project(session, spec.name, window, cx) {
                        return Err(
                            "Apply or cancel the active transform before creating a project."
                                .into(),
                        );
                    }
                } else {
                    let doc = spec.create()?;
                    if !self.install(doc, None, None, None, spec.name, window, cx) {
                        return Err(
                            "Apply or cancel the active transform before creating a document."
                                .into(),
                        );
                    }
                }
                if let Some(editor) = &self.editor {
                    editor.update(cx, |editor, cx| {
                        editor.home_canvas_kind = Some(spec.kind);
                        let draws = matches!(spec.kind, CanvasKind::Paint | CanvasKind::Storyboard);
                        if editor.draw_mode != draws {
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
        // A copy (a storyboard from a template) is never bound to its source.
        let copy = file.copy_as.clone();
        // Reopening a saved path activates its existing tab, including dirty projects.
        // install_project would otherwise replace that tab's entire session.
        if copy.is_none()
            && let Some(index) = self
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
        let mut import_notes = Vec::new();
        let psd_report = file.psd_report;
        match file.content {
            emulsion_mcp::workspace_tools::FileContent::Project(session, notes) => {
                warnings = notes;
                let name = copy.clone().unwrap_or_else(|| stem(&path));
                if !self.install_project(*session, name, window, cx) {
                    return Err(
                        "Apply or cancel the active transform before opening a project.".into(),
                    );
                }
                self.show_project_open_notes(warnings.clone(), false, cx);
            }
            emulsion_mcp::workspace_tools::FileContent::Document(opened) => {
                let emulsion_io::Opened {
                    doc,
                    graph,
                    history_error,
                } = *opened;
                let notice =
                    super::import_report::open_notice(history_error.as_deref(), psd_report);
                if let Some(error) = history_error {
                    warnings.push(format!("History could not be restored: {error}"));
                }
                if let Some((message, warning)) =
                    super::import_report::open_notice(None, psd_report)
                {
                    if warning {
                        warnings.push(message);
                    } else {
                        import_notes.push(message);
                    }
                }
                let kind = file.kind.or_else(|| self.home_project_kind(&path));
                if !self.install(
                    doc,
                    graph,
                    emulsion_io::is_native(&path).then(|| path.clone()),
                    Some(path.clone()),
                    stem(&path),
                    window,
                    cx,
                ) {
                    return Err(
                        "Apply or cancel the active transform before opening a document.".into(),
                    );
                }
                if let Some(editor) = &self.editor {
                    editor.update(cx, |editor, cx| {
                        editor.home_canvas_kind = kind;
                        if let Some(kind) = kind {
                            let paint = kind == emulsion_core::creation::CanvasKind::Paint;
                            if editor.draw_mode != paint {
                                editor.toggle_draw_mode(cx);
                            }
                        }
                        if let Some((message, warning)) = notice {
                            editor.set_status(message, warning, cx);
                        }
                    });
                }
            }
        }
        let mut result = self.mcp_tabs(origin, cx);
        result["opened_tab_id"] = result["active_tab_id"].clone();
        result["opened_path"] = if copy.is_some() {
            Value::Null
        } else {
            json!(path)
        };
        result["warnings"] = json!(warnings);
        result["import_notes"] = json!(import_notes);
        result["psd_import"] = psd_report
            .map(emulsion_mcp::workspace_tools::psd_report_value)
            .unwrap_or(Value::Null);
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
    use gpui_kit::test::TestWindowExt;
    #[gpui_kit::test]
    fn mcp_native_open_and_template_copy_display_all_decode_warnings(cx: &mut TestAppContext) {
        use emulsion_mcp::workspace_tools::{FileContent, FileRequest, load_file};
        let dir = tempfile::tempdir().unwrap();
        let (ws, cx) = crate::tests::open(cx, Document::new(8, 8));
        let origin = cx.update(|_, cx| ws.read(cx).editor.as_ref().unwrap().entity_id().as_u64());
        for missing in [false, true] {
            let (project, _, _) = Workspace::native_recovery_fixture(missing);
            let path = dir.path().join(format!("mcp-{missing}.emu"));
            emulsion_io::project::write(&project, &path).unwrap();
            let path = path.canonicalize().unwrap();
            let original = std::fs::read(&path).unwrap();
            let native_warnings = emulsion_io::project::read_with_report(&path)
                .unwrap()
                .report
                .warnings();
            for copy in [false, true] {
                let mut loaded = load_file(FileRequest {
                    path: path.clone(),
                    kind: None,
                    copy_as: copy.then(|| "Template copy".into()),
                })
                .unwrap();
                let FileContent::Project(_, notes) = &mut loaded.content else {
                    panic!("expected project")
                };
                assert_eq!(notes.as_slice(), native_warnings.as_slice());
                // Existing importer notes must survive alongside native recovery
                // notes through the same installation and response path.
                let mut expected = native_warnings.clone();
                if missing {
                    expected.push("Existing import warning".into());
                    *notes = expected.clone();
                }
                cx.update(|window, cx| {
                    ws.update(cx, |ws, cx| {
                        let result = ws.install_mcp_file(origin, loaded, window, cx).unwrap();
                        assert_eq!(result["warnings"], json!(expected));
                        let editor = ws.editor.as_ref().unwrap().read(cx);
                        assert_eq!(editor.editor.path, (!copy).then(|| path.clone()));
                        if missing {
                            let (message, warning) = editor.status.as_ref().unwrap();
                            assert!(*warning);
                            assert!(message.contains("2 warnings"));
                            assert_eq!(message.matches(native_warnings[0].as_str()).count(), 1);
                        } else {
                            assert!(!editor.status.as_ref().is_some_and(|(_, warning)| *warning));
                        }
                    });
                });
                cx.run_until_parked();
                if missing {
                    cx.update(|window, _| {
                        let message = window.find("editor-status-message");
                        assert!(message.visible());
                    });
                }
                if !copy {
                    // Reopening activates the existing session and does not
                    // replay warnings or clear its already visible status.
                    let loaded = load_file(FileRequest {
                        path: path.clone(),
                        kind: None,
                        copy_as: None,
                    })
                    .unwrap();
                    cx.update(|window, cx| {
                        ws.update(cx, |ws, cx| {
                            let editor = ws.editor.clone().unwrap();
                            let before = editor.read(cx).status.clone();
                            let result = ws.install_mcp_file(origin, loaded, window, cx).unwrap();
                            assert_eq!(result["reused_existing_tab"], true);
                            assert_eq!(result["warnings"], json!([]));
                            assert_eq!(editor.read(cx).status, before);
                        });
                    });
                }
            }
            assert_eq!(std::fs::read(path).unwrap(), original);
        }
    }

    #[gpui_kit::test]
    fn mcp_document_import_exposes_report_and_preserves_history_warning_priority(
        cx: &mut TestAppContext,
    ) {
        use emulsion_io::psd::{ImportProfileDecision, ReadReport};
        use emulsion_mcp::workspace_tools::{FileContent, LoadedFile};
        let (ws, cx) = crate::tests::open(cx, Document::new(8, 8));
        let origin = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap().entity_id().as_u64());
        cx.update(|window, cx| {
            for history_error in [None, Some("history damaged".to_string())] {
                for decision in [
                    ImportProfileDecision::SavedAppearance,
                    ImportProfileDecision::UniquePhotoshopSrgbV1,
                ] {
                    let report = ReadReport {
                        profile_decision: decision,
                        background_preserved: false,
                    };
                    let doc = Document::new(13, 17);
                    let expected = super::super::import_report::open_notice(
                        history_error.as_deref(),
                        Some(report),
                    )
                    .unwrap();
                    let path = std::env::temp_dir().join("import-report.psd");
                    let loaded = LoadedFile {
                        path: path.clone(),
                        copy_as: None,
                        kind: None,
                        content: FileContent::Document(Box::new(emulsion_io::Opened {
                            doc: doc.clone(),
                            graph: None,
                            history_error: history_error.clone(),
                        })),
                        psd_report: Some(report),
                    };
                    ws.update(cx, |ws, cx| {
                        let result = ws.install_mcp_file(origin, loaded, window, cx).unwrap();
                        assert_eq!(
                            result["psd_import"],
                            emulsion_mcp::workspace_tools::psd_report_value(report)
                        );
                        assert_eq!(
                            result["warnings"].as_array().unwrap().len(),
                            usize::from(history_error.is_some())
                                + usize::from(decision == ImportProfileDecision::SavedAppearance)
                        );
                        let editor = ws.editor.as_ref().unwrap().read(cx);
                        assert_eq!(editor.editor.doc, doc);
                        assert_eq!(editor.source, Some(path));
                        let (message, warning) = editor.status.as_ref().unwrap();
                        assert_eq!(message.as_ref(), expected.0.as_str());
                        assert_eq!(*warning, expected.1);
                    });
                }
            }
        });
    }

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
