//! File › Import › Import script… for storyboards: a Fountain, Final Draft
//! or plain-text screenplay laid out as scenes and panels with captions,
//! previewed first (title, scenes, beats), one panel per beat or per scene,
//! after the active panel or at the end. The New storyboard dialog starts a
//! board from a script the same way. Importing is one Undo step.
use super::*;
use emulsion_core::project::{PageId, ProjectEditor};
use emulsion_io::script::{self, Script, storyboard::Split};
use gpui_kit::component::{
    Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
};
use std::path::Path;

/// "Fountain (.fountain, .spmd), Final Draft (.fdx) or text (.txt)".
const KINDS: &str = "Fountain (.fountain, .spmd), Final Draft (.fdx) or text (.txt)";

/// Read a script file, refusing other kinds of file.
pub(crate) fn read_script(path: &Path) -> Result<Script, String> {
    let known = path
        .extension()
        .is_some_and(|e| script::EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)));
    if !known {
        return Err(format!("Choose a {KINDS} script."));
    }
    script::read(path).map_err(|e| format!("{e:#}"))
}

/// A new storyboard holding only `script`'s panels, with no Undo history:
/// the blank panels it was created with are replaced.
pub(crate) fn project_from_script(
    mut project: ProjectEditor,
    script: &Script,
    split: Split,
) -> Result<ProjectEditor, String> {
    let blanks: Vec<PageId> = project.page_list().iter().map(|m| m.id).collect();
    let board = project
        .storyboard()
        .ok_or("Scripts start storyboards only.")?;
    let clip = script::storyboard::panels(script, board, split).map_err(|e| e.to_string())?;
    let new = project.paste_panels(blanks.last().copied(), &clip)?;
    project.remove_pages(&blanks)?;
    project.set_active_page(new[0])?;
    let snapshot = project
        .snapshot()
        .ok_or("Scripts start storyboards only.")?;
    ProjectEditor::open(snapshot, None)
}

/// "3 scenes · 12 beats".
fn summary(script: &Script) -> String {
    let plural = |n: usize, noun: &str| format!("{n} {noun}{}", if n == 1 { "" } else { "s" });
    format!(
        "{} · {}",
        plural(script.scenes.len(), "scene"),
        plural(script.beat_count(), "beat")
    )
}

pub(crate) struct ScriptImport {
    editor: WeakEntity<EditorView>,
    path: Option<PathBuf>,
    /// The read script, or why it could not be read; `None` while reading.
    script: Option<Result<Arc<Script>, String>>,
    split: Split,
    at_end: bool,
    message: Option<String>,
}

impl ScriptImport {
    fn choose(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(format!("Choose a {KINDS} script").into()),
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

    /// Read `path` off the UI thread for the preview.
    pub(crate) fn load(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.path = Some(path.clone());
        self.script = None;
        self.message = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_spawn({
                    let path = path.clone();
                    async move { read_script(&path).map(Arc::new) }
                })
                .await;
            this.update(cx, |this, cx| {
                if this.path.as_ref() == Some(&path) {
                    this.script = Some(read);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Import the previewed script; true when it landed.
    pub(crate) fn import(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(Ok(script)) = self.script.clone() else {
            return false;
        };
        let (split, at_end) = (self.split, self.at_end);
        let result = self.editor.update(cx, |editor, cx| {
            editor.import_script(&script, split, at_end, cx)
        });
        match result {
            Ok(Ok(_)) => true,
            Ok(Err(error)) => {
                self.message = Some(error);
                cx.notify();
                false
            }
            Err(_) => false,
        }
    }
}

impl Render for ScriptImport {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let option =
            |id: &'static str, text: &'static str, on: bool| chip(id, text, on, &p).test_support();
        let preview = match (&self.path, &self.script) {
            (None, _) => mono(format!("Choose a {KINDS} script."), 10.5, p.muted),
            (Some(_), None) => mono("Reading the script…", 10.5, p.muted),
            (Some(_), Some(Err(error))) => mono(error.clone(), 10.5, p.accent),
            (Some(path), Some(Ok(script))) => div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(script.title.clone().unwrap_or_else(|| {
                            path.file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned()
                        })),
                )
                .child(mono(summary(script), 10.5, p.muted)),
        };
        let ready = matches!(self.script, Some(Ok(_)));
        div()
            .id("script-import")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(10.))
            .text_size(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        Button::new("script-import-choose")
                            .label("Choose script…")
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
                    .id("script-import-preview")
                    .test_support()
                    .p(px(8.))
                    .rounded(px(4.))
                    .bg(p.soft_bg)
                    .child(preview),
            )
            .child(mono("Panels", 10., p.muted))
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        option(
                            "script-import-beat",
                            "One per action or dialogue block",
                            self.split == Split::Beat,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.split = Split::Beat;
                            cx.notify();
                        })),
                    )
                    .child(
                        option(
                            "script-import-scene",
                            "One per scene",
                            self.split == Split::Scene,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.split = Split::Scene;
                            cx.notify();
                        })),
                    ),
            )
            .child(mono("Insert", 10., p.muted))
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        option(
                            "script-import-after",
                            "After the active panel's scene",
                            !self.at_end,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.at_end = false;
                            cx.notify();
                        })),
                    )
                    .child(
                        option("script-import-end", "At the end", self.at_end).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.at_end = true;
                                cx.notify();
                            }),
                        ),
                    ),
            )
            .child(mono(
                "Each scene heading starts a scene; text goes to the Action, Dialogue and Slugging captions; DISSOLVE, FADE and WIPE become transitions.",
                10.,
                p.muted,
            ))
            .children(self.message.clone().map(|message| {
                div()
                    .id("script-import-message")
                    .test_support()
                    .aria_label(message.clone())
                    .text_color(p.accent)
                    .child(message)
            }))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        Button::new("script-import-cancel")
                            .label("Cancel")
                            .small()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("script-import-ok")
                            .label("Import")
                            .small()
                            .primary()
                            .disabled(!ready)
                            .on_click(cx.listener(|this, _, window, cx| {
                                if this.import(cx) {
                                    window.close_dialog(cx);
                                }
                            })),
                    ),
            )
    }
}

impl EditorView {
    /// File › Import › Import script…
    pub(crate) fn open_script_import(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<ScriptImport>> {
        if self.editor.storyboard().is_none() {
            self.set_status("Scripts import into storyboards.", false, cx);
            return None;
        }
        let editor = cx.entity().downgrade();
        let dialog = cx.new(|_| ScriptImport {
            editor,
            path: None,
            script: None,
            split: Split::Beat,
            at_end: false,
            message: None,
        });
        let shown = dialog.clone();
        window.open_dialog(cx, move |d, _, _| {
            d.title("Import script")
                .width(px(520.))
                .child(shown.clone())
        });
        Some(dialog)
    }

    /// Lay `script` out as panels after the active panel's scene, or at the
    /// end, as one Undo step; the new panels become the Board selection.
    pub(crate) fn import_script(
        &mut self,
        script: &Script,
        split: Split,
        at_end: bool,
        cx: &mut Context<Self>,
    ) -> Result<Vec<PageId>, String> {
        let board = self.editor.storyboard().ok_or("Open a storyboard first.")?;
        let clip = script::storyboard::panels(script, board, split).map_err(|e| e.to_string())?;
        if !self.prepare_page_action(cx) {
            return Err("Finish the current edit first.".into());
        }
        let after = if at_end {
            self.editor.page_list().last().map(|m| m.id)
        } else {
            Some(self.editor.active_page())
        };
        let new = self
            .editor
            .paste_panels(after, &clip)
            .inspect_err(|error| {
                self.set_status(error.clone(), true, cx);
            })?;
        self.after_change(cx);
        self.set_board_selection(new.clone());
        self.set_status(
            format!(
                "Imported {}: {} panel{} in {} scene{}.",
                script.title.as_deref().unwrap_or("the script"),
                new.len(),
                if new.len() == 1 { "" } else { "s" },
                clip.scenes.len(),
                if clip.scenes.len() == 1 { "" } else { "s" },
            ),
            false,
            cx,
        );
        Ok(new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::open;
    use core::prelude::v1::test;
    use emulsion_core::project::ProjectKind;

    const SCRIPT: &str = "Title: The Storm

INT. KITCHEN - NIGHT

Rain on the window.

MIA
Is anyone there?

EXT. GARDEN - DAWN

Birds.
";

    fn storyboard_editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
        let (ws, cx) = open(cx, Document::new(64, 36));
        let project =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(80, 40)).unwrap();
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
    fn a_previewed_script_imports_as_one_undo_step(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("storm.fountain");
        std::fs::write(&path, SCRIPT).unwrap();
        let (e, cx) = storyboard_editor(cx);
        let dialog = cx
            .update(|window, cx| e.update(cx, |e, cx| e.open_script_import(window, cx)))
            .unwrap();
        // Other files are refused before reading.
        let other = dir.path().join("notes.docx");
        std::fs::write(&other, "x").unwrap();
        cx.update(|_, cx| dialog.update(cx, |d, cx| d.load(other, cx)));
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert!(matches!(&dialog.read(cx).script, Some(Err(e)) if e.contains("Fountain")));
            assert!(!dialog.update(cx, |d, cx| d.import(cx)));
        });
        cx.update(|_, cx| dialog.update(cx, |d, cx| d.load(path, cx)));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let script = dialog.read(cx).script.clone().unwrap().unwrap();
            assert_eq!(script.title.as_deref(), Some("The Storm"));
            assert_eq!(summary(&script), "2 scenes · 3 beats");
            dialog.update(cx, |d, _| d.at_end = true);
            assert!(dialog.update(cx, |d, cx| d.import(cx)));
        });
        cx.update(|_, cx| {
            let e = e.read(cx);
            let pages = e.editor.page_list();
            assert_eq!(pages.len(), 4);
            let board = e.editor.storyboard().unwrap();
            let dialogue = board.caption("Dialogue").unwrap();
            assert_eq!(
                board.panels[&pages[2].id].captions[&dialogue].text,
                "MIA: Is anyone there?"
            );
            assert_eq!(e.board_selection().len(), 3);
        });
        cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
        assert_eq!(cx.update(|_, cx| e.read(cx).editor.page_list().len()), 1);
    }

    #[test]
    fn a_new_board_from_a_script_holds_only_its_panels() {
        let script = script::parse_fountain(SCRIPT);
        let project =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(80, 40)).unwrap();
        let project = project_from_script(project, &script, Split::Scene).unwrap();
        let board = project.storyboard().unwrap();
        assert_eq!(project.page_list().len(), 2);
        let names: Vec<_> = board.scenes.values().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["INT. KITCHEN - NIGHT", "EXT. GARDEN - DAWN"]);
        assert!(!project.can_undo());
        assert_eq!(project.active_page(), project.page_list()[0].id);
    }
}
