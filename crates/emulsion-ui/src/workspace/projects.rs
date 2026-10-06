//! Open a complete project before installing any of its pages in the workspace.
use super::*;
use emulsion_core::project::ProjectEditor;

impl Workspace {
    pub(crate) fn open_diagram_path(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let is_pack = emulsion_io::template_pack::is_pack(&path);
        self.add_tab_then(window, cx, move |this, window, cx| {
            this.start_busy(crate::busy_card::Busy::new(if is_pack {
                "Installing creative package"
            } else {
                "Opening editable diagram"
            }), window, cx);
            cx.spawn_in(window, async move |this, cx| {
                let source = path.clone();
                let result = cx.background_spawn(async move {
                    if is_pack {
                        let pack = emulsion_io::template_pack::read(&source)?;
                        let project = pack.project.clone();
                        emulsion_io::template_pack::install(&emulsion_io::creative_library::root(), pack)?;
                        Ok(emulsion_io::drawio::Imported { project, warnings: Vec::new() })
                    } else if emulsion_io::pptx::is_pptx(&source) {
                        emulsion_io::pptx::read(&source).map(|p|emulsion_io::drawio::Imported{project:p.project,warnings:p.warnings})
                    } else if source.extension().is_some_and(|e|e.eq_ignore_ascii_case("json"))
                        && emulsion_io::lottie::is_lottie_path(&source) {
                        let (doc,report)=emulsion_io::lottie::read(&source)?;
                        let project=ProjectEditor::new_project(emulsion_core::project::ProjectKind::Design,doc).map_err(emulsion_io::IoError::Unsupported)?.snapshot().ok_or_else(||emulsion_io::IoError::Unsupported("Missing imported project".into()))?;
                        Ok(emulsion_io::drawio::Imported{project,warnings:report.diagnostics})
                    } else {
                        emulsion_io::diagram_import::read(&source)
                    }
                }).await;
                this.update_in(cx, |this, window, cx| {
                    this.busy = None;
                    match result.map_err(|e| e.to_string()).and_then(|imported| {
                        ProjectEditor::open(imported.project, None).map(|session| (session, imported.warnings))
                    }) {
                        Ok((session, warnings)) => {
                            this.recents = recent::push(&path, format!("{} · {} pages", session.kind().unwrap().label(), session.page_list().len()));
                            if !this.install_project(session, stem(&path), window, cx) { return; }
                            if let Some(editor) = &this.editor {
                                editor.update(cx, |e, cx| {
                                    let message = if !warnings.is_empty() {
                                        format!("Imported with {} notes. Review Import / export notes in the Export menu.", warnings.len())
                                    } else if is_pack {
                                        "Package installed in your local library. This is an editable copy.".into()
                                    } else {
                                        "Imported editable project. Save as an Emulsion project to retain all page history.".into()
                                    };
                                    e.diagram_import_notes(warnings.clone());
                                    e.set_status(message, !warnings.is_empty(), cx);
                                });
                            }
                        }
                        Err(error) => this.error = Some(format!("Could not import {}: {error}", path.display()).into()),
                    }
                    cx.notify();
                }).ok();
            }).detach();
        });
    }

    pub(crate) fn install_project(
        &mut self,
        session: ProjectEditor,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(editor) = &self.editor
            && !editor.update(cx, |e, cx| e.photo_transform_ready(cx))
        {
            cx.notify();
            return false;
        }
        let path = session.path.clone();
        if !self.install(
            session.doc.clone(),
            Some(session.graph.clone()),
            path.clone(),
            path,
            name,
            window,
            cx,
        ) {
            return false;
        }
        if let Some(editor) = &self.editor {
            editor.update(cx, |editor, cx| editor.install_project_session(session, cx));
        }
        true
    }

    /// Deliver every note from this open only after the editor is installed.
    /// Recovery completion must not replace a partial-history warning with a
    /// success message. The full list remains available in the Export menu.
    pub(super) fn show_project_open_notes(
        &self,
        warnings: Vec<String>,
        recovered: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(editor) = &self.editor {
            editor.update(cx, |editor, cx| {
                let notice = project_open_notice(&warnings, recovered);
                editor.diagram_import_notes(warnings);
                if let Some((message, warning)) = notice {
                    editor.set_status(message, warning, cx);
                }
            });
        }
    }

    pub(crate) fn open_project_path(
        &mut self,
        path: PathBuf,
        recovered: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !recovered
            && let Some(index) = self
                .tabs
                .iter()
                .position(|tab| tab.read(cx).editor.path.as_ref() == Some(&path))
        {
            self.activate_tab(index, window, cx);
            return;
        }
        self.add_tab_then(window, cx, move |this, window, cx| {
            this.start_busy(
                crate::busy_card::Busy::new(if recovered {
                    "Recovering your project"
                } else {
                    "Opening your project"
                }),
                window,
                cx,
            );
            this.error = None;
            cx.spawn_in(window, async move |this, cx| {
                let source = path.clone();
                let result = cx
                    .background_spawn(async move {
                        let opened = emulsion_io::project::read_with_report(&source)
                            .map_err(|e| e.to_string())?;
                        let session =
                            ProjectEditor::open(opened.project, (!recovered).then_some(source))?;
                        Ok::<_, String>((session, opened.report))
                    })
                    .await;
                this.update_in(cx, |this, window, cx| {
                    this.busy = None;
                    match result {
                        Ok((session, report)) => {
                            // A second concurrent open may have finished first.
                            if !recovered
                                && let Some(index) = this.tabs.iter().position(|tab| {
                                    tab.read(cx).editor.path.as_ref() == Some(&path)
                                })
                            {
                                this.activate_tab(index, window, cx);
                                return;
                            }
                            if !recovered {
                                this.recents = recent::push(
                                    &path,
                                    format!(
                                        "{} · {} pages",
                                        session.kind().unwrap().label(),
                                        session.page_list().len()
                                    ),
                                );
                            }
                            let name = if recovered {
                                recovered_name(&path)
                            } else {
                                stem(&path)
                            };
                            if !this.install_project(session, name, window, cx) {
                                return;
                            }
                            if !recovered {
                                this.shared_check_on_open(cx);
                            }
                            if recovered {
                                this.recovered.retain(|(p, _)| p != &path);
                                if let Some(editor) = &this.editor {
                                    editor.update(cx, |editor, _| {
                                        // Keep the only durable copy until a successful save.
                                        editor.history.recovery = Some(path.clone());
                                    });
                                }
                            }
                            this.show_project_open_notes(report.warnings(), recovered, cx);
                        }
                        Err(error) => {
                            this.error =
                                Some(format!("Could not open {}: {error}", path.display()).into())
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        });
    }
}

fn project_open_notice(warnings: &[String], recovered: bool) -> Option<(String, bool)> {
    if let Some(first) = warnings.first() {
        let action = if recovered { "Recovered" } else { "Opened" };
        let count = warnings.len();
        let noun = if count == 1 { "warning" } else { "warnings" };
        let save = if recovered {
            " Save the project to keep it."
        } else {
            ""
        };
        Some((
            format!(
                "{action} with {count} {noun}: {first} Review Import / export notes in the Export menu for all details.{save}",
            ),
            true,
        ))
    } else if recovered {
        Some((
            "Recovered all pages. Save the project to keep it.".into(),
            false,
        ))
    } else {
        None
    }
}

// Shared by the actual UI open, MCP installation, and local-template tests.
// Write a real project, retaining version references but optionally omitting
// their retired graph. No mocked report or private I/O decoder is involved.
#[cfg(test)]
impl Workspace {
    pub(crate) fn native_recovery_fixture(
        missing: bool,
    ) -> (emulsion_core::project::Project, u64, u64) {
        use emulsion_core::{project::ProjectKind, storyboard::Panel};
        let mut editor =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(32, 18)).unwrap();
        let blank = editor.storyboard().unwrap().blank_panel().unwrap();
        let retired = editor
            .insert_panels(
                Some(1),
                &blank,
                vec![("Retired".into(), Panel::new(0, 24))],
                None,
            )
            .unwrap()[0];
        let version = editor.create_board_version("Before removal").unwrap();
        editor.set_active_page(1).unwrap();
        editor.remove_page(retired).unwrap();
        let mut project = editor.snapshot().unwrap();
        if missing {
            project
                .storyboard
                .as_mut()
                .unwrap()
                .versions
                .retired
                .clear();
        }
        (project, retired, version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use gpui_kit::test::TestWindowExt;

    #[gpui_kit::test]
    fn native_open_and_recovery_keep_visible_diagnostics_after_install(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (ws, cx) = crate::tests::open(cx, Document::new(8, 8));
        for missing in [false, true] {
            for recovered in [false, true] {
                let (project, retired, version) = Workspace::native_recovery_fixture(missing);
                let path = dir.path().join(format!("native-{missing}-{recovered}.emu"));
                emulsion_io::project::write(&project, &path).unwrap();
                let before = std::fs::read(&path).unwrap();
                let report = emulsion_io::project::read_with_report(&path)
                    .unwrap()
                    .report;
                assert_eq!(report.is_empty(), !missing);
                if missing {
                    let diagnostic = &report.diagnostics[0];
                    assert_eq!(report.diagnostics.len(), 1);
                    assert_eq!(
                        diagnostic.code,
                        emulsion_io::project::ProjectReadDiagnosticCode::MissingRetiredArchive
                    );
                    assert_eq!(diagnostic.panel_id, retired);
                    assert_eq!(diagnostic.entry, format!("history/board/{retired}.ora"));
                    assert_eq!(diagnostic.affected_version_ids, vec![version]);
                }
                let warnings = report.warnings();
                cx.update(|window, cx| {
                    ws.update(cx, |ws, cx| {
                        ws.open_project_path(path.clone(), recovered, window, cx)
                    });
                });
                cx.run_until_parked();
                cx.update(|window, cx| {
                    if missing {
                        let message = window.find("editor-status-message");
                        assert!(message.visible());
                    }
                    let workspace = ws.read(cx);
                    assert!(workspace.error.is_none());
                    let editor = workspace.editor.as_ref().unwrap().read(cx);
                    assert_eq!(editor.editor.page_list().len(), 1);
                    assert_eq!(editor.editor.board_versions().len(), 1);
                    assert_eq!(editor.editor.path, (!recovered).then(|| path.clone()));
                    if recovered {
                        assert_eq!(editor.history.recovery, Some(path.clone()));
                    }
                    if missing || recovered {
                        let status = editor.status.as_ref().unwrap();
                        assert_eq!(status.1, missing);
                        if missing {
                            assert_eq!(status.0.matches(warnings[0].as_str()).count(), 1);
                            assert!(!status.0.contains("Recovered all pages"));
                        } else {
                            assert_eq!(
                                status.0.as_ref(),
                                "Recovered all pages. Save the project to keep it."
                            );
                        }
                    } else {
                        assert!(!editor.status.as_ref().is_some_and(|(_, warning)| *warning));
                    }
                });
                assert_eq!(
                    std::fs::read(&path).unwrap(),
                    before,
                    "opening never saves over recovery evidence"
                );
            }
        }
    }

    #[gpui_kit::test]
    fn failed_native_open_never_installs_a_partial_project(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("corrupt.emu");
        std::fs::write(&path, b"not an archive").unwrap();
        let doc = Document::new(13, 17);
        let (ws, cx) = crate::tests::open(cx, doc.clone());
        let original = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| ws.open_project_path(path, false, window, cx))
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let workspace = ws.read(cx);
            assert!(workspace.error.is_some());
            assert_eq!(workspace.tabs.len(), 1);
            assert_eq!(workspace.editor.as_ref().unwrap(), &original);
            assert_eq!(original.read(cx).editor.doc, doc);
        });
    }
}
