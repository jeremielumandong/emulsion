//! Storyboard imports: layered files (PSD, PSB, ORA), images or SVG placed
//! on the active panel, or added as new panels, one per file; PDF and
//! AI (.ai) files add one panel per page. Files are read off the UI
//! thread with the same readers as File → Open; blend modes, masks and
//! clipping come through them. PDF pages convert through an external tool,
//! with a progress card that can cancel. Each import is one Undo step.
use super::*;
use crate::file_prompt::FilePrompts;
use emulsion_ai::jobs::Job;
use emulsion_io::pdf_import;
use std::path::Path;

/// The most files one import reads.
const MAX_FILES: usize = 100;

struct ReadDocument {
    doc: Document,
    notice: Option<(String, bool)>,
}

struct ReadPanels {
    documents: Vec<(String, Document)>,
    notices: Vec<(String, bool)>,
}

/// Retain every warning when later files succeed or have no profile evidence.
fn collect_panels(read: Vec<Result<ReadPanels, String>>) -> ReadPanels {
    let mut collected = ReadPanels {
        documents: Vec::new(),
        notices: Vec::new(),
    };
    for result in read {
        match result {
            Ok(panels) => {
                collected.documents.extend(panels.documents);
                collected.notices.extend(panels.notices);
            }
            Err(error) => collected.notices.push((error, true)),
        }
    }
    collected
}

/// Read each file once, keeping same-import source evidence with its document.
fn read_documents(paths: Vec<PathBuf>) -> Vec<(PathBuf, Result<ReadDocument, String>)> {
    paths
        .into_iter()
        .map(|path| {
            let doc = emulsion_io::open_full_with_report(&path)
                .map(|(opened, report)| ReadDocument {
                    notice: crate::workspace::import_report::source_notice(
                        &path,
                        opened.history_error.as_deref(),
                        report,
                    ),
                    doc: opened.doc,
                })
                .map_err(|e| format!("{}: {e}", path.display()));
            (path, doc)
        })
        .collect()
}

/// Each file as named panels, in order: one for a layered or image file, one
/// per page for a PDF or AI file. `job` hears the progress and can
/// cancel.
fn read_panels(paths: Vec<PathBuf>, job: &Job) -> Vec<Result<ReadPanels, String>> {
    let count = paths.len().max(1) as f32;
    let mut out = Vec::new();
    for (index, path) in paths.into_iter().enumerate() {
        if job.cancelled() {
            break;
        }
        let file = path.file_name().unwrap_or_default().to_string_lossy();
        job.set_stage(format!("reading {file}"));
        job.progress(index as f32 / count);
        out.push(if pdf_import::is_pdf(&path) {
            pdf_import::pages(&path, job.cancel_flag(), |page, pages| {
                job.set_stage(format!("converting {file}, page {page} of {pages}"));
                job.progress((index as f32 + (page - 1) as f32 / pages as f32) / count);
            })
            .map(|documents| ReadPanels {
                documents,
                notices: Vec::new(),
            })
            .map_err(|e| match e {
                emulsion_io::IoError::Unsupported(message) => format!("{file}: {message}"),
                e => format!("{file}: {e}"),
            })
        } else {
            read_documents(vec![path.clone()])
                .pop()
                .unwrap()
                .1
                .map(|loaded| ReadPanels {
                    documents: vec![(panel_name(&path), loaded.doc)],
                    notices: loaded.notice.into_iter().collect(),
                })
        });
    }
    job.progress(1.);
    out
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
        let rx = cx.prompt_open_paths(PathPromptOptions {
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
            "Import PSD, PSB, ORA, image, SVG, PDF or AI files as new panels",
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
                match doc.and_then(|loaded| {
                    this.editor
                        .place_layers(&loaded.doc)
                        .map(|ids| (ids, loaded.notice))
                }) {
                    Ok((ids, notice)) => {
                        this.set_layer_selection(ids.clone(), ids.last().copied());
                        this.after_change(cx);
                        let mut message = format!(
                            "Placed {} on this panel.",
                            path.file_name().unwrap_or_default().to_string_lossy()
                        );
                        let mut warning = false;
                        if let Some((note, is_warning)) = notice {
                            message.push(' ');
                            message.push_str(&note);
                            warning = is_warning;
                        }
                        this.set_status(message, warning, cx);
                    }
                    Err(error) => this.set_status(error, true, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    /// Add one panel per file after the active panel, in its scene, named
    /// after the files; a PDF or AI file adds one panel per page.
    /// Files that cannot be read are reported and skipped.
    pub(crate) fn add_files_as_panels(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if !self.storyboard_import_ready(paths.len(), cx) {
            return;
        }
        let ticket = self.edit_ticket();
        let after = self.editor.active_page();
        let job = Job::new();
        if paths.iter().any(|p| pdf_import::is_pdf(p)) {
            self.watch_job(job.clone(), "Importing pages as panels", cx);
        } else {
            self.set_status(format!("Reading {} file(s)…", paths.len()), false, cx);
        }
        cx.spawn(async move |this, cx| {
            let worker = job.clone();
            let read = cx
                .background_spawn(async move {
                    let read = read_panels(paths, &worker);
                    worker.finish();
                    read
                })
                .await;
            this.update(cx, |this, cx| {
                if job.cancelled() {
                    this.set_status("Import canceled.", false, cx);
                    return;
                }
                if this.edit_ticket() != ticket {
                    this.set_status(
                        "The storyboard changed while the files loaded. Import them again.",
                        false,
                        cx,
                    );
                    return;
                }
                let collected = collect_panels(read);
                let warning = collected.notices.iter().any(|(_, warning)| *warning);
                let notes: Vec<_> = collected
                    .notices
                    .into_iter()
                    .map(|(message, _)| message)
                    .collect();
                let documents = collected.documents;
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
                        this.set_status(message, warning, cx);
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

    #[test]
    fn later_successes_cannot_erase_saved_appearance_warnings_in_multi_import() {
        let fallback = crate::workspace::import_report::open_notice(
            None,
            Some(emulsion_io::psd::ReadReport {
                profile_decision: emulsion_io::psd::ImportProfileDecision::SavedAppearance,
                background_preserved: false,
            }),
        )
        .unwrap();
        let notice = format!("first.psd: {}", fallback.0);
        let collected = collect_panels(vec![
            Ok(ReadPanels {
                documents: vec![("first".into(), Document::new(2, 2))],
                notices: vec![(notice.clone(), fallback.1)],
            }),
            Ok(ReadPanels {
                documents: vec![("second".into(), Document::new(2, 2))],
                notices: Vec::new(),
            }),
            Ok(ReadPanels {
                documents: vec![("third".into(), Document::new(2, 2))],
                notices: vec![("third.psd: no comparison".into(), false)],
            }),
        ]);
        assert_eq!(collected.documents.len(), 3);
        assert_eq!(collected.notices[0], (notice, true));
        assert_eq!(collected.notices.len(), 2);
        assert!(collected.notices.iter().any(|(_, warning)| *warning));
    }

    #[test]
    fn layered_source_reader_keeps_the_same_import_notice_with_each_filename() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../emulsion-io/tests/fixtures/psd/blending/knockout-deep-nested-pt.psd");
        let (expected, report) = emulsion_io::open_full_with_report(&path).unwrap();
        let mut read = read_documents(vec![path.clone()]);
        let (source, loaded) = read.pop().unwrap();
        let loaded = loaded.unwrap();
        assert_eq!(source, path);
        crate::document_contents::assert_document_contents(
            &loaded.doc,
            &expected.doc,
            "layered source reader",
        );
        assert_eq!(
            loaded.notice,
            crate::workspace::import_report::source_notice(&path, None, report)
        );
    }

    fn layered_psd(dir: &Path) -> PathBuf {
        // Independently authored import fixture. Native export's appearance
        // guard is tested separately and must not be bypassed for this test.
        let path = dir.join("SC010 key art.psd");
        std::fs::write(
            &path,
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../emulsion-io/tests/fixtures/psd/storyboard/key-art.psd"
            )),
        )
        .unwrap();
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
        for (name, alpha) in [("Shade", 153u16), ("Light", 128u16)] {
            let NodeKind::Raster { raster, .. } = &named(name).kind else {
                panic!("the import fixture must retain editable pixel layers");
            };
            assert_eq!(raster.get(0, 0)[3], alpha * 257, "fractional source alpha");
        }
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
            let bounds = emulsion_core::geometry::node_bounds(doc, find("Paper").id)
                .unwrap()
                .unwrap();
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

    #[gpui_kit::test]
    fn pdf_pages_import_as_panels_in_file_then_page_order(cx: &mut TestAppContext) {
        if pdf_import::converter().is_none() {
            eprintln!("skipped: no PDF converter on PATH");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        // A two-page PDF from the project exporter.
        let mut design =
            ProjectEditor::new_project(ProjectKind::Design, Document::new(80, 40)).unwrap();
        design
            .add_page(Document::new(40, 80), "Second".into(), 0.)
            .unwrap();
        let project = design.snapshot().unwrap();
        let ids: Vec<_> = project.pages.iter().map(|p| p.meta.id).collect();
        let pdf = dir.path().join("Layouts.pdf");
        emulsion_io::project_export::write(
            &project,
            &ids,
            emulsion_io::project_export::Format::Pdf,
            false,
            &pdf,
        )
        .unwrap();
        let png = dir.path().join("Key.png");
        image::RgbaImage::from_pixel(16, 8, image::Rgba([10, 200, 30, 255]))
            .save(&png)
            .unwrap();
        let (e, cx) = storyboard_editor(cx);
        let first = cx.update(|_, cx| e.read(cx).editor.page_list()[0].id);
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.select_page(first, cx);
                e.add_files_as_panels(vec![pdf, png], cx)
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
            assert_eq!(
                names[1..4],
                ["Layouts page 1", "Layouts page 2", "Key"],
                "{:?}",
                e.status
            );
            // Pages are vector art fitted to the panel.
            let page = e.editor.page(e.editor.page_list()[1].id).unwrap();
            assert_eq!((page.doc.width, page.doc.height), (80, 40));
        });
        cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
        assert_eq!(cx.update(|_, cx| e.read(cx).editor.page_list().len()), 2);
    }
}
