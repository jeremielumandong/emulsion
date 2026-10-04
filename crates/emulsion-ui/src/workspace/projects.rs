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
                        let project =
                            emulsion_io::project::read(&source).map_err(|e| e.to_string())?;
                        ProjectEditor::open(project, (!recovered).then_some(source))
                    })
                    .await;
                this.update_in(cx, |this, window, cx| {
                    this.busy = None;
                    match result {
                        Ok(session) => {
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
                                    editor.update(cx, |editor, cx| {
                                        // Keep the only durable copy until a successful save.
                                        editor.history.recovery = Some(path.clone());
                                        editor.set_status(
                                            "Recovered all pages. Save the project to keep it.",
                                            false,
                                            cx,
                                        );
                                    });
                                }
                            }
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
