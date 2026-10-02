//! Editorial interchange for storyboards. File → Export Edit… writes the
//! animatic as an EDL, Final Cut Pro 7 XML or OpenTimelineIO edit with each
//! panel's media (stills or movies) in a folder beside it, off the UI
//! thread with progress and Cancel. File → Import → Conform to Edit… reads
//! an edit back, shows what would change (dry run), lets the person choose
//! whether another frame rate keeps times or frame counts, and applies it
//! as one Undo step, then shows what changed.
use super::*;
use emulsion_core::project::Project;
use emulsion_core::storyboard_conform::{ConformReport, RateChoice};
use emulsion_core::timeline::Edit;
use emulsion_io::editorial::{self, ExportOptions, Format, MediaKind};
use gpui_kit::component::{
    Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub(crate) struct EditExport {
    project: Arc<Project>,
    name: String,
    pub(crate) options: ExportOptions,
    busy: bool,
    cancel: Arc<AtomicBool>,
    done: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
    pub(crate) message: Option<String>,
}

impl EditExport {
    fn new(project: Project, name: String) -> Self {
        Self {
            project: Arc::new(project),
            name,
            options: ExportOptions::default(),
            busy: false,
            cancel: Arc::new(AtomicBool::new(false)),
            done: Arc::new(AtomicU64::new(0)),
            total: Arc::new(AtomicU64::new(0)),
            message: None,
        }
    }

    fn choose_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let extension = self.options.format.extension();
        let request = cx.prompt_for_new_path(
            &std::env::current_dir().unwrap_or_default(),
            Some(&format!("{}.{extension}", self.name)),
        );
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(mut path))) = request.await {
                path.set_extension(extension);
                this.update(cx, |this, cx| this.export_to(path, cx)).ok();
            }
        })
        .detach();
    }

    /// Export to `path` on a worker, showing progress until it ends.
    pub(crate) fn export_to(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let (project, name, options) = (
            self.project.clone(),
            self.name.clone(),
            self.options.clone(),
        );
        let (cancel, done, total) = (self.cancel.clone(), self.done.clone(), self.total.clone());
        cancel.store(false, Ordering::Relaxed);
        done.store(0, Ordering::Relaxed);
        total.store(0, Ordering::Relaxed);
        self.busy = true;
        self.message = Some("Exporting…".into());
        cx.notify();
        let task = cx.background_spawn(async move {
            let mut progress = |d: u64, t: u64| {
                done.store(d, Ordering::Relaxed);
                total.store(t, Ordering::Relaxed);
            };
            editorial::export(&project, &name, &options, &path, &mut progress, &cancel)
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(200))
                    .await;
                let busy = this
                    .update(cx, |this, cx| {
                        cx.notify();
                        this.busy
                    })
                    .unwrap_or(false);
                if !busy {
                    break;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.busy = false;
                this.message = Some(match result {
                    Ok(report) => {
                        let mut text = format!(
                            "Exported {} panels and {} sound clips to {}, media in {}.",
                            report.clips,
                            report.sound_clips,
                            report.path.display(),
                            report.media_folder.display()
                        );
                        for warning in report.warnings {
                            text.push('\n');
                            text.push_str(&warning);
                        }
                        text
                    }
                    Err(e) => format!("Export failed: {e:#}"),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for EditExport {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let progress = self.busy.then(|| {
            let (d, t) = (
                self.done.load(Ordering::Relaxed),
                self.total.load(Ordering::Relaxed),
            );
            if t == 0 {
                "Preparing…".to_string()
            } else {
                format!("Panel {d} of {t}")
            }
        });
        let busy = self.busy;
        let formats = Format::ALL.into_iter().map(|f| {
            chip(
                SharedString::from(format!("edit-export-format-{}", f.extension())),
                f.label(),
                self.options.format == f,
                &p,
            )
            .test_support()
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.busy {
                    this.options.format = f;
                    cx.notify();
                }
            }))
        });
        let media = [
            (MediaKind::Still, "edit-export-still", "Stills (PNG)"),
            (MediaKind::Movie, "edit-export-movie", "Movies (ProRes MOV)"),
        ]
        .into_iter()
        .map(|(kind, id, label)| {
            chip(id, label, self.options.media == kind, &p)
                .test_support()
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.busy {
                        this.options.media = kind;
                        cx.notify();
                    }
                }))
        });
        let widths = [(1920u32, "1920 px"), (1280, "1280 px"), (0, "Full size")]
            .into_iter()
            .map(|(w, label)| {
                chip(
                    SharedString::from(format!("edit-export-width-{w}")),
                    label,
                    self.options.width == w,
                    &p,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.busy {
                        this.options.width = w;
                        cx.notify();
                    }
                }))
            });
        let note = match self.options.format {
            Format::Edl => {
                "One picture track and four sound tracks; no levels, reference video or wipes other than from the left or top."
            }
            Format::Xmeml => {
                "For Final Cut Pro 7, Premiere Pro, DaVinci Resolve and Avid: panels, transitions, sound with levels, reference video and markers."
            }
            Format::Otio => {
                "For any OpenTimelineIO tool: panels, transitions, sound, reference video and markers."
            }
        };
        div()
            .id("edit-export")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(10.))
            .text_size(px(12.))
            .text_color(p.ink)
            .child(mono("Format", 10., p.muted))
            .child(div().flex().flex_wrap().gap(px(6.)).children(formats))
            .child(mono(note, 10., p.muted))
            .child(mono("Panel media", 10., p.muted))
            .child(div().flex().flex_wrap().gap(px(6.)).children(media))
            .child(div().flex().flex_wrap().gap(px(6.)).children(widths))
            .child(mono(
                "Media, sounds and reference videos go into a folder named after the edit, beside it. Clips keep the panel names, so the edit can be conformed back. Movies need FFmpeg.",
                10.,
                p.muted,
            ))
            .when_some(progress.or(self.message.clone()), |d, m| {
                d.child(
                    div()
                        .id("edit-export-message")
                        .test_support()
                        .whitespace_normal()
                        .child(m),
                )
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .when(busy, |d| {
                        d.child(
                            Button::new("edit-export-cancel")
                                .label("Cancel export")
                                .small()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.cancel.store(true, Ordering::Relaxed);
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        Button::new("edit-export-close")
                            .label("Close")
                            .small()
                            .disabled(busy)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("edit-export-button")
                            .label("Export…")
                            .small()
                            .primary()
                            .disabled(busy)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.choose_path(window, cx)),
                            ),
                    ),
            )
    }
}

pub(crate) struct ConformDialog {
    editor: WeakEntity<EditorView>,
    path: Option<PathBuf>,
    /// The read edit, or why it could not be read; `None` while reading.
    edit: Option<Result<Arc<Edit>, String>>,
    pub(crate) choice: RateChoice,
    /// The dry run for the current edit and choice.
    pub(crate) preview: Option<Result<ConformReport, String>>,
    /// What the last Apply changed.
    pub(crate) applied: Option<ConformReport>,
}

impl ConformDialog {
    fn choose(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose an EDL, Final Cut XML or OpenTimelineIO edit".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            if let Some(path) = paths.into_iter().next() {
                this.update(cx, |this, cx| this.load(path, cx)).ok();
            }
        })
        .detach();
    }

    /// Read `path` off the UI thread, then preview the conform.
    pub(crate) fn load(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let Some(rate) = self.editor.upgrade().and_then(|e| {
            e.read(cx)
                .editor
                .storyboard()
                .map(|b| b.settings.frame_rate)
        }) else {
            return;
        };
        self.path = Some(path.clone());
        self.edit = None;
        self.preview = None;
        self.applied = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_spawn({
                    let path = path.clone();
                    async move {
                        editorial::read(&path, rate)
                            .map(Arc::new)
                            .map_err(|e| format!("{e:#}"))
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                if this.path.as_ref() == Some(&path) {
                    this.edit = Some(read);
                    this.refresh(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Dry-run the conform for the current edit and rate choice.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.preview = match &self.edit {
            Some(Ok(edit)) => {
                let (edit, choice) = (edit.clone(), self.choice);
                self.editor
                    .update(cx, |e, _| e.editor.conform_storyboard(&edit, choice, false))
                    .ok()
            }
            Some(Err(e)) => Some(Err(e.clone())),
            None => None,
        };
        cx.notify();
    }

    /// Apply the previewed conform; true when it landed.
    pub(crate) fn apply(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(Ok(edit)) = self.edit.clone() else {
            return false;
        };
        let choice = self.choice;
        let result = self
            .editor
            .update(cx, |e, cx| e.conform_to_edit(&edit, choice, cx));
        match result {
            Ok(Ok(report)) => {
                self.applied = Some(report);
                self.refresh(cx);
                true
            }
            Ok(Err(error)) => {
                self.preview = Some(Err(error));
                cx.notify();
                false
            }
            Err(_) => false,
        }
    }
}

impl Render for ConformDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let preview = match (&self.path, &self.edit, &self.preview) {
            (None, _, _) => mono(
                "Choose an EDL (.edl), Final Cut XML (.xml) or OpenTimelineIO (.otio) edit.",
                10.5,
                p.muted,
            ),
            (Some(_), None, _) => mono("Reading the edit…", 10.5, p.muted),
            (Some(_), _, Some(Err(error))) => mono(error.clone(), 10.5, p.accent),
            (Some(_), _, Some(Ok(report))) => div()
                .whitespace_normal()
                .child(format!("Conforming would change:\n{}", report.summary())),
            (Some(_), _, None) => mono("Checking the edit…", 10.5, p.muted),
        };
        let rate_differs = matches!(&self.preview, Some(Ok(r)) if r.rate_differs);
        let ready = matches!(&self.preview, Some(Ok(r)) if !r.is_noop());
        let rate_option = |id: &'static str, text: &'static str, choice: RateChoice| {
            chip(id, text, self.choice == choice, &p)
                .test_support()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.choice = choice;
                    this.refresh(cx);
                }))
        };
        div()
            .id("conform-edit")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(10.))
            .text_size(px(12.))
            .text_color(p.ink)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        Button::new("conform-edit-choose")
                            .label("Choose edit…")
                            .small()
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| this.choose(cx))),
                    )
                    .child(
                        div().flex_1().min_w_0().truncate().child(
                            self.path
                                .as_ref()
                                .and_then(|p| p.file_name())
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                        ),
                    ),
            )
            .child(
                div()
                    .id("conform-edit-preview")
                    .test_support()
                    .p(px(8.))
                    .rounded(px(4.))
                    .bg(p.soft_bg)
                    .child(preview),
            )
            .when(rate_differs, |d| {
                d.child(mono("Frame rate", 10., p.muted)).child(
                    div()
                        .flex()
                        .gap(px(6.))
                        .child(rate_option(
                            "conform-edit-convert",
                            "Convert: keep times",
                            RateChoice::Convert,
                        ))
                        .child(rate_option(
                            "conform-edit-keep",
                            "Keep frame counts",
                            RateChoice::Keep,
                        )),
                )
            })
            .child(mono(
                "Clips match panels by name, or by the media files Export Edit wrote. Panels take the edit's durations, order and transitions; panels the edit moves join the scene they land in; sound clips that play this board's sounds replace its sound clips.",
                10.,
                p.muted,
            ))
            .children(self.applied.as_ref().map(|report| {
                div()
                    .id("conform-edit-applied")
                    .test_support()
                    .whitespace_normal()
                    .text_color(p.accent)
                    .child(format!("Conformed (Undo reverts it):\n{}", report.summary()))
            }))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        Button::new("conform-edit-close")
                            .label("Close")
                            .small()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("conform-edit-apply")
                            .label("Apply")
                            .small()
                            .primary()
                            .disabled(!ready)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.apply(cx);
                            })),
                    ),
            )
    }
}

impl EditorView {
    /// File → Export Edit (EDL, Final Cut XML, OpenTimelineIO)…
    pub(crate) fn edit_export_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<EditExport>> {
        let project = self.storyboard_snapshot(cx)?;
        let name = self.name.clone();
        let view = cx.new(|_| EditExport::new(project, name));
        let shown = view.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title("Export edit")
                .width(px(560.))
                .child(shown.clone())
        });
        Some(view)
    }

    /// File → Import → Conform to Edit…
    pub(crate) fn open_conform_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<ConformDialog>> {
        if self.editor.storyboard().is_none() {
            self.set_status("Edits conform storyboards.", false, cx);
            return None;
        }
        let editor = cx.entity().downgrade();
        let dialog = cx.new(|_| ConformDialog {
            editor,
            path: None,
            edit: None,
            choice: RateChoice::Convert,
            preview: None,
            applied: None,
        });
        let shown = dialog.clone();
        window.open_dialog(cx, move |d, _, _| {
            d.title("Conform to edit")
                .width(px(560.))
                .child(shown.clone())
        });
        Some(dialog)
    }

    /// Conform the storyboard to `edit` as one Undo step.
    pub(crate) fn conform_to_edit(
        &mut self,
        edit: &Edit,
        choice: RateChoice,
        cx: &mut Context<Self>,
    ) -> Result<ConformReport, String> {
        if !self.prepare_page_action(cx) {
            return Err("Finish the current edit first.".into());
        }
        let report = self
            .editor
            .conform_storyboard(edit, choice, true)
            .inspect_err(|error| self.set_status(error.clone(), true, cx))?;
        self.after_change(cx);
        self.set_status(
            format!(
                "Conformed to {}: {} panels matched, {} retimed, {} moved.",
                if edit.name.is_empty() {
                    "the edit"
                } else {
                    &edit.name
                },
                report.matched,
                report.retimed.len(),
                report.moved.len()
            ),
            false,
            cx,
        );
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::open;
    use core::prelude::v1::test;
    use emulsion_core::project::{ProjectEditor, ProjectKind};
    use emulsion_core::storyboard::Panel;
    use gpui_kit::test::TestWindowExt;

    fn storyboard_editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
        let (ws, cx) = open(cx, Document::new(64, 36));
        let mut project =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(32, 18)).unwrap();
        let blank = project.storyboard().unwrap().blank_panel().unwrap();
        let first = project.active_page();
        project
            .insert_panels(
                Some(first),
                &blank,
                vec![
                    ("Panel 2".into(), Panel::new(0, 24)),
                    ("Panel 3".into(), Panel::new(0, 12)),
                ],
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
    fn an_exported_edit_conforms_back_with_a_dry_run_and_one_undo_step(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Board.otio");
        let (e, cx) = storyboard_editor(cx);
        let export = cx
            .update(|window, cx| e.update(cx, |e, cx| e.edit_export_dialog(window, cx)))
            .unwrap();
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.find("edit-export").visible());
            export.update(cx, |v, cx| {
                v.options.format = Format::Otio;
                v.options.width = 64;
                v.export_to(path.clone(), cx);
            });
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            let message = export.read(cx).message.clone().unwrap();
            assert!(message.starts_with("Exported 3 panels"), "{message}");
            window.close_dialog(cx);
        });
        // The editor swaps the last two panels.
        let rate = emulsion_core::storyboard::FrameRate::whole(24);
        let mut edit = editorial::read(&path, rate).unwrap();
        let (b, c) = (edit.video[1].clone(), edit.video[2].clone());
        let c_frames = c.frames();
        edit.video[1] = emulsion_core::timeline::EditClip {
            record_in: b.record_in,
            record_out: b.record_in + c_frames,
            ..c
        };
        edit.video[2] = emulsion_core::timeline::EditClip {
            record_in: b.record_in + c_frames,
            record_out: b.record_in + c_frames + b.frames(),
            ..b
        };
        let cut = dir.path().join("Cut.otio");
        std::fs::write(&cut, editorial::otio::write(&edit)).unwrap();
        let dialog = cx
            .update(|window, cx| e.update(cx, |e, cx| e.open_conform_dialog(window, cx)))
            .unwrap();
        cx.update(|_, cx| dialog.update(cx, |d, cx| d.load(cut, cx)));
        cx.run_until_parked();
        let names = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| {
                e.read(cx)
                    .editor
                    .page_list()
                    .iter()
                    .map(|m| m.name.clone())
                    .collect::<Vec<_>>()
            })
        };
        let before = names(cx);
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.find("conform-edit-preview").visible());
            let preview = dialog.read(cx).preview.clone().unwrap().unwrap();
            assert_eq!(preview.matched, 3);
            assert_eq!(preview.moved.len(), 1);
            assert!(dialog.update(cx, |d, cx| d.apply(cx)));
            assert!(dialog.read(cx).applied.is_some());
            window.render_frame(cx);
            assert!(window.find("conform-edit-applied").visible());
        });
        let after = names(cx);
        assert_eq!(
            after,
            [before[0].clone(), before[2].clone(), before[1].clone()]
        );
        cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
        assert_eq!(names(cx), before);
    }
}
