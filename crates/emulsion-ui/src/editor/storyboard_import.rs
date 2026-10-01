//! Storyboard imports: layered files (PSD, PSB, ORA) or images placed on the
//! active panel, or added as new panels, one per file. Files are read off the
//! UI thread with the same readers as File → Open; blend modes, masks and
//! clipping come through them. Each import is one Undo step.
use super::*;
use std::path::Path;

/// The most files one import reads.
const MAX_FILES: usize = 100;

/// Read each file as a layered document.
fn read_documents(paths: Vec<PathBuf>) -> Vec<(PathBuf, Result<Document, String>)> {
    paths
        .into_iter()
        .map(|path| {
            let doc = emulsion_io::open_full(&path)
                .map(|opened| opened.doc)
                .map_err(|e| format!("{}: {e}", path.display()));
            (path, doc)
        })
        .collect()
}

/// A panel name from a file name: "SC010 key art.psd" → "SC010 key art".
fn panel_name(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Imported".into())
}

impl EditorView {
    fn prompt_storyboard_files(
        &mut self,
        multiple: bool,
        prompt: &str,
        then: fn(&mut Self, Vec<PathBuf>, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple,
            prompt: Some(prompt.to_string().into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            this.update(cx, |this, cx| then(this, paths, cx)).ok();
        })
        .detach();
    }

    /// File → Import → Import into panel…
    pub(super) fn import_into_panel(&mut self, cx: &mut Context<Self>) {
        self.prompt_storyboard_files(
            false,
            "Import a PSD, PSB, ORA or image into this panel",
            Self::place_files_in_panel,
            cx,
        );
    }

    /// File → Import → Import as panels…
    pub(super) fn import_as_panels(&mut self, cx: &mut Context<Self>) {
        self.prompt_storyboard_files(
            true,
            "Import PSD, PSB, ORA or image files as new panels",
            Self::add_files_as_panels,
            cx,
        );
    }

    /// Whether files can be imported now; reports why not.
    fn storyboard_import_ready(&mut self, count: usize, cx: &mut Context<Self>) -> bool {
        if self.editor.storyboard().is_none() || count == 0 {
            return false;
        }
        if count > MAX_FILES {
            self.set_status(
                format!("Import up to {MAX_FILES} files at a time."),
                true,
                cx,
            );
            return false;
        }
        self.prepare_page_action(cx)
    }

    /// Place the layers of the first file on top of the active panel, fitted
    /// to the frame.
    pub(crate) fn place_files_in_panel(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if !self.storyboard_import_ready(paths.len(), cx) {
            return;
        }
        if self.active_panel_locked() {
            self.set_status(
                "This panel is locked. Unlock it to import into it.",
                true,
                cx,
            );
            return;
        }
        let ticket = self.edit_ticket();
        let page = self.editor.active_page();
        let paths: Vec<_> = paths.into_iter().take(1).collect();
        self.set_status("Reading the file…", false, cx);
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_spawn(async move { read_documents(paths) })
                .await;
            this.update(cx, |this, cx| {
                if this.edit_ticket() != ticket || this.editor.active_page() != page {
                    this.set_status(
                        "The panel changed while the file loaded. Import it again.",
                        false,
                        cx,
                    );
                    return;
                }
                let Some((path, doc)) = read.into_iter().next() else {
                    return;
                };
                match doc.and_then(|doc| this.editor.place_layers(&doc)) {
                    Ok(ids) => {
                        this.set_layer_selection(ids.clone(), ids.last().copied());
                        this.after_change(cx);
                        this.set_status(
                            format!(
                                "Placed {} on this panel.",
                                path.file_name().unwrap_or_default().to_string_lossy()
                            ),
                            false,
                            cx,
                        );
                    }
                    Err(error) => this.set_status(error, true, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    /// Add one panel per file after the active panel, in its scene, named
    /// after the files. Files that cannot be read are reported and skipped.
    pub(crate) fn add_files_as_panels(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if !self.storyboard_import_ready(paths.len(), cx) {
            return;
        }
        let ticket = self.edit_ticket();
        let after = self.editor.active_page();
        self.set_status(format!("Reading {} file(s)…", paths.len()), false, cx);
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_spawn(async move { read_documents(paths) })
                .await;
            this.update(cx, |this, cx| {
                if this.edit_ticket() != ticket {
                    this.set_status(
                        "The storyboard changed while the files loaded. Import them again.",
                        false,
                        cx,
                    );
                    return;
                }
                let mut notes = Vec::new();
                let documents: Vec<_> = read
                    .into_iter()
                    .filter_map(|(path, doc)| match doc {
                        Ok(doc) => Some((panel_name(&path), doc)),
                        Err(error) => {
                            notes.push(error);
                            None
                        }
                    })
                    .collect();
                if documents.is_empty() {
                    this.set_status(notes.join(" "), true, cx);
                    return;
                }
                match this.editor.import_panels(Some(after), documents) {
                    Ok(new) => {
                        this.after_change(cx);
                        let mut message = format!("Imported {} panel(s).", new.len());
                        for note in &notes {
                            message.push(' ');
                            message.push_str(note);
                        }
                        this.set_board_selection(new);
                        this.set_status(message, !notes.is_empty(), cx);
                    }
                    Err(error) => this.set_status(error, true, cx),
                }
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::open;
    use core::prelude::v1::test;
    use emulsion_core::project::{ProjectEditor, ProjectKind};
    use emulsion_raster::BlendMode;

    fn layered_psd(dir: &Path) -> PathBuf {
        let mut doc = Document::new(40, 20);
        let mut add = |name: &str, rgba: [f32; 4], blend: BlendMode, clip: Option<NodeId>| {
            let mut node = Node::raster(
                0,
                name,
                Arc::new(Raster::solid(40, 20, rgba)),
                Placement::default(),
            );
            node.blend = blend;
            let id = Command::AddNode {
                node: Box::new(node),
                slot: Slot::TOP,
            }
            .apply(&mut doc)
            .unwrap()
            .unwrap();
            if clip.is_some() {
                Command::SetClip { id, clip_to: clip }
                    .apply(&mut doc)
                    .unwrap();
            }
            id
        };
        add("Paper", [0.9, 0.9, 0.85, 1.], BlendMode::Normal, None);
        let base = add("Shade", [0.1, 0.1, 0.3, 0.6], BlendMode::Multiply, None);
        add(
            "Light",
            [0.4, 0.35, 0.2, 0.5],
            BlendMode::Screen,
            Some(base),
        );
        let path = dir.join("SC010 key art.psd");
        emulsion_io::psd::write(&doc, &path).unwrap();
        path
    }

    fn storyboard_editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
        let (ws, cx) = open(cx, Document::new(64, 36));
        let mut project =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(80, 40)).unwrap();
        let blank = project.storyboard().unwrap().blank_panel().unwrap();
        project
            .insert_panels(
                Some(1),
                &blank,
                vec![(
                    "Panel 2".into(),
                    emulsion_core::storyboard::Panel::new(0, 24),
                )],
                None,
            )
            .unwrap();
        let editor = cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.install_project(project, "Board".into(), window, cx)
            });
            ws.read(cx).editor.clone().unwrap()
        });
        cx.run_until_parked();
        (editor, cx)
    }

    #[gpui_kit::test]
    fn layered_psd_imports_into_the_panel_with_blend_modes_and_clipping(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let psd = layered_psd(dir.path());
        // The PSD reader keeps the clipping and blend modes.
        let read = emulsion_io::open_full(&psd).unwrap().doc;
        let named = |name: &str| read.nodes.iter().find(|n| n.name == name).unwrap();
        assert_eq!(named("Light").clip_to, Some(named("Shade").id));
        let (e, cx) = storyboard_editor(cx);
        let before = cx.update(|_, cx| e.read(cx).editor.doc.nodes.len());
        cx.update(|_, cx| e.update(cx, |e, cx| e.place_files_in_panel(vec![psd], cx)));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = e.read(cx);
            let doc = &e.editor.doc;
            assert_eq!(doc.nodes.len(), before + 3, "{:?}", e.status);
            let find = |name: &str| doc.nodes.iter().find(|n| n.name == name).unwrap();
            assert_eq!(find("Shade").blend, BlendMode::Multiply);
            assert_eq!(find("Light").blend, BlendMode::Screen);
            assert_eq!(find("Light").clip_to, Some(find("Shade").id));
            // Fitted to the 80×40 frame, from 40×20.
            let bounds = emulsion_core::geometry::node_bounds(doc, find("Paper").id).unwrap();
            assert_eq!((bounds.w, bounds.h), (80, 40));
        });
        // One Undo step takes the whole file back off.
        cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
        assert_eq!(cx.update(|_, cx| e.read(cx).editor.doc.nodes.len()), before);
    }

    #[gpui_kit::test]
    fn files_import_as_new_panels_named_after_them_after_the_active_panel(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let psd = layered_psd(dir.path());
        let png = dir.path().join("SC020 wide.png");
        image::RgbaImage::from_pixel(16, 8, image::Rgba([10, 200, 30, 255]))
            .save(&png)
            .unwrap();
        let missing = dir.path().join("gone.png");
        let (e, cx) = storyboard_editor(cx);
        let first = cx.update(|_, cx| e.read(cx).editor.page_list()[0].id);
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.select_page(first, cx);
                e.add_files_as_panels(vec![psd, png, missing], cx)
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = e.read(cx);
            let names: Vec<_> = e
                .editor
                .page_list()
                .iter()
                .map(|m| m.name.clone())
                .collect();
            assert_eq!(names.len(), 4, "{names:?}");
            assert_eq!(names[1], "SC010 key art");
            assert_eq!(names[2], "SC020 wide");
            let (message, error) = e.status.clone().unwrap();
            assert!(message.contains("Imported 2 panel(s)"), "{message}");
            assert!(message.contains("gone.png") && error);
            assert_eq!(e.board_selection().len(), 2);
        });
        // The whole import is one Undo step.
        cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
        assert_eq!(cx.update(|_, cx| e.read(cx).editor.page_list().len()), 2);
    }
}
