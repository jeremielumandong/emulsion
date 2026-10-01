//! File → Export Storyboard: the PDF board (laid out by a profile in the
//! print dialog, which also prints it), panel images named by a pattern, and
//! the captions CSV. Files are written off the UI thread from a snapshot.
use super::*;
use emulsion_core::project::{PageId, Project};
use emulsion_io::storyboard_export::{self as story, PANEL_TOKENS, Scope, images};
use gpui_kit::component::{
    Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
use std::sync::{Arc, atomic::AtomicBool};

/// A menu entry of the image export dialog: its label and what it sets.
type MenuChoice = (String, fn(&mut ImageExport));

/// Export Panel Images: the naming pattern, format, layers and panels.
pub(crate) struct ImageExport {
    editor: WeakEntity<EditorView>,
    project: Arc<Project>,
    name: String,
    selection: Vec<PageId>,
    pattern: Entity<InputState>,
    options: images::Options,
    scope: Scope,
    busy: bool,
    message: Option<String>,
    _subs: Vec<Subscription>,
}

impl ImageExport {
    fn new(
        editor: WeakEntity<EditorView>,
        project: Project,
        name: String,
        selection: Vec<PageId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let options = images::Options::default();
        let pattern =
            cx.new(|cx| InputState::new(window, cx).default_value(options.pattern.clone()));
        let subs = vec![cx.subscribe(&pattern, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })];
        Self {
            editor,
            project: Arc::new(project),
            name,
            selection,
            pattern,
            options,
            scope: Scope::All,
            busy: false,
            message: None,
            _subs: subs,
        }
    }

    /// The options as entered, validated.
    fn draft(&self, cx: &App) -> anyhow::Result<images::Options> {
        let mut options = self.options.clone();
        options.pattern = self.pattern.read(cx).value().trim().to_string();
        options.validate()?;
        Ok(options)
    }

    /// The first file name the pattern gives, as an example.
    fn example(&self, cx: &App) -> anyhow::Result<String> {
        let options = self.draft(cx)?;
        let entries = story::select(story::entries(&self.project)?, &self.scope)?;
        let rate = story::board(&self.project)?.settings.frame_rate;
        let stem = story::expand(&options.pattern, |token| match token {
            "layer" => Some("Background".into()),
            _ => story::panel_token(&entries[0], &self.name, rate, token),
        })?;
        let stem = if options.per_layer && !options.pattern.contains("{layer") {
            format!("{stem}_Background")
        } else {
            stem
        };
        Ok(format!("{stem}.{}", options.format.extension()))
    }

    /// Write the images into `dir` on a worker.
    pub(crate) fn export_to(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        let options = match self.draft(cx) {
            Ok(options) => options,
            Err(e) => {
                self.message = Some(e.to_string());
                cx.notify();
                return;
            }
        };
        let (project, name, scope) = (self.project.clone(), self.name.clone(), self.scope.clone());
        self.busy = true;
        self.message = Some("Exporting panel images…".into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    images::write(
                        &project,
                        &name,
                        &scope,
                        &options,
                        &dir,
                        &AtomicBool::new(false),
                    )
                    .map(|files| (files.len(), dir))
                })
                .await;
            this.update(cx, |this, cx| {
                this.busy = false;
                this.message = Some(match result {
                    Ok((count, dir)) => format!("Wrote {count} images to {}", dir.display()),
                    Err(e) => format!("Export failed: {e}"),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn choose_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let request = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Export panel images into this folder".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = request.await
                && let Some(dir) = paths.into_iter().next()
            {
                this.update(cx, |this, cx| this.export_to(dir, cx)).ok();
            }
        })
        .detach();
    }

    fn menu(
        &self,
        id: &'static str,
        label: String,
        choices: Vec<MenuChoice>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let owner = cx.weak_entity();
        Button::new(id)
            .label(label)
            .small()
            .outline()
            .dropdown_caret(true)
            .disabled(self.busy)
            .dropdown_menu(move |mut menu, _, _| {
                for (name, apply) in &choices {
                    let (owner, apply) = (owner.clone(), *apply);
                    menu = menu.item(PopupMenuItem::new(name.clone()).on_click(move |_, _, cx| {
                        owner
                            .update(cx, |this, cx| {
                                apply(this);
                                cx.notify();
                            })
                            .ok();
                    }));
                }
                menu
            })
            .into_any_element()
    }
}

impl Render for ImageExport {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let example = self.example(cx);
        let format = match self.options.format {
            images::Format::Png => "PNG · transparency kept",
            images::Format::Jpeg => "JPEG · on white",
        };
        let scope = match &self.scope {
            Scope::All => "All panels".to_string(),
            Scope::Panels(ids) => format!("Selected panels ({})", ids.len()),
            Scope::Scene(id) => story::board(&self.project)
                .ok()
                .and_then(|b| b.scenes.get(id))
                .map_or_else(|| "Scene".into(), |s| format!("Scene {}", s.name)),
        };
        let scenes: Vec<_> = story::entries(&self.project)
            .map(|entries| {
                let mut seen = std::collections::HashSet::new();
                entries
                    .into_iter()
                    .filter(|e| seen.insert(e.scene_id))
                    .map(|e| (e.scene_id, e.scene))
                    .collect()
            })
            .unwrap_or_default();
        let owner = cx.weak_entity();
        let selection = self.selection.clone();
        let tokens = PANEL_TOKENS
            .iter()
            .map(|t| format!("{{{t}}}"))
            .collect::<Vec<_>>()
            .join(" ");
        div()
            .id("storyboard-image-export")
            .test_support()
            .flex()
            .flex_col()
            .gap_2()
            .text_size(px(12.))
            .text_color(p.ink)
            .child("File name pattern")
            .child(Input::new(&self.pattern).small().disabled(self.busy))
            .child(div().text_color(p.muted).child(format!(
                "Tokens: {tokens}, and {{layer}} per layer. {{index:3}} pads numbers with zeros."
            )))
            .child(
                div()
                    .id("storyboard-image-example")
                    .test_support()
                    .text_color(if example.is_ok() { p.muted } else { rgb(0xc98535).into() })
                    .child(match &example {
                        Ok(name) => format!("First file: {name}"),
                        Err(e) => e.to_string(),
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(self.menu(
                        "storyboard-image-format",
                        format.into(),
                        vec![
                            ("PNG · transparency kept".into(), |s| {
                                s.options.format = images::Format::Png
                            }),
                            ("JPEG · on white".into(), |s| {
                                s.options.format = images::Format::Jpeg
                            }),
                        ],
                        cx,
                    ))
                    .child(self.menu(
                        "storyboard-image-layers",
                        if self.options.per_layer {
                            "One image per layer".into()
                        } else {
                            "One image per panel".into()
                        },
                        vec![
                            ("One image per panel".into(), |s| s.options.per_layer = false),
                            ("One image per layer".into(), |s| s.options.per_layer = true),
                        ],
                        cx,
                    ))
                    .child(
                        Button::new("storyboard-image-scope")
                            .label(scope)
                            .small()
                            .outline()
                            .dropdown_caret(true)
                            .disabled(self.busy)
                            .dropdown_menu(move |mut menu, _, _| {
                                let mut choices = vec![("All panels".to_string(), Scope::All)];
                                choices.push((
                                    format!("Selected panels ({})", selection.len()),
                                    Scope::Panels(selection.clone()),
                                ));
                                for (id, name) in &scenes {
                                    choices.push((format!("Scene {name}"), Scope::Scene(*id)));
                                }
                                for (label, scope) in choices {
                                    let owner = owner.clone();
                                    menu = menu.item(PopupMenuItem::new(label).on_click(
                                        move |_, _, cx| {
                                            owner
                                                .update(cx, |this, cx| {
                                                    this.scope = scope.clone();
                                                    cx.notify();
                                                })
                                                .ok();
                                        },
                                    ));
                                }
                                menu
                            }),
                    ),
            )
            .when(self.options.per_layer, |d| {
                d.child(div().text_color(p.muted).child(
                    "Each visible top-level layer becomes its own image, with layers clipped to it.",
                ))
            })
            .when_some(self.message.clone(), |d, m| {
                d.child(div().id("storyboard-image-message").test_support().child(m))
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("storyboard-image-close")
                            .label("Close")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("storyboard-image-export-button")
                            .label("Export to folder…")
                            .primary()
                            .disabled(self.busy || example.is_err() || self.editor.upgrade().is_none())
                            .on_click(cx.listener(|this, _, window, cx| this.choose_folder(window, cx))),
                    ),
            )
    }
}

impl EditorView {
    /// A storyboard snapshot to export, or `None` with the reason shown.
    fn storyboard_snapshot(&mut self, cx: &mut Context<Self>) -> Option<Project> {
        self.finish_gpu_stroke(cx);
        self.editor.storyboard()?;
        if self.editor.in_transaction() {
            self.set_status("Finish the current edit before exporting.", false, cx);
            return None;
        }
        self.editor.snapshot()
    }

    /// File → Export Storyboard PDF…, and Print on a storyboard: the print
    /// dialog with storyboard layout profiles.
    pub(crate) fn storyboard_print(
        &mut self,
        pdf: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.storyboard_snapshot(cx) else {
            return;
        };
        let selection = self.board_selection();
        if let Err(e) = crate::print_dialog::open_storyboard(
            self.name.clone(),
            project,
            selection,
            pdf,
            window,
            cx,
        ) {
            self.set_status(e.to_string(), true, cx);
        }
    }

    /// File → Export Panel Images…
    pub(crate) fn storyboard_images_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.storyboard_snapshot(cx) else {
            return;
        };
        let (editor, name, selection) =
            (cx.weak_entity(), self.name.clone(), self.board_selection());
        let view = cx.new(|cx| ImageExport::new(editor, project, name, selection, window, cx));
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title("Export panel images")
                .width(px(560.))
                .child(view.clone())
        });
    }

    /// File → Export Captions CSV…
    pub(crate) fn storyboard_csv(&mut self, cx: &mut Context<Self>) {
        let Some(project) = self.storyboard_snapshot(cx) else {
            return;
        };
        let request = cx.prompt_for_new_path(
            &std::env::current_dir().unwrap_or_default(),
            Some(&format!("{}.csv", self.name)),
        );
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut path))) = request.await else {
                return;
            };
            path.set_extension("csv");
            let result = cx
                .background_spawn(async move {
                    story::csv::write(&project, &Scope::All, &path).map(|rows| (rows, path))
                })
                .await;
            this.update(cx, |this, cx| match result {
                Ok((rows, path)) => this.set_status(
                    format!("Exported {rows} panels to {}", path.display()),
                    false,
                    cx,
                ),
                Err(e) => this.set_status(format!("CSV export failed: {e}"), true, cx),
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use emulsion_core::project::{ProjectEditor, ProjectKind};
    use gpui_kit::test::TestWindowExt;

    fn project() -> Project {
        let mut editor =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(32, 18)).unwrap();
        let blank = editor.storyboard().unwrap().blank_panel().unwrap();
        let first = editor.active_page();
        editor
            .insert_panels(
                Some(first),
                &blank,
                vec![(
                    "Panel 2".into(),
                    emulsion_core::storyboard::Panel::new(0, 24),
                )],
                None,
            )
            .unwrap();
        editor.snapshot().unwrap()
    }

    #[gpui_kit::test]
    fn image_export_shows_the_first_name_and_writes_files(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            theme::install(cx);
        });
        let (view, cx) = cx.add_window_view(|window, cx| {
            ImageExport::new(
                WeakEntity::new_invalid(),
                project(),
                "Film".into(),
                vec![],
                window,
                cx,
            )
        });
        let dir = tempfile::tempdir().unwrap();
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.find("storyboard-image-example").visible());
            let example = view.read(cx).example(cx).unwrap();
            assert_eq!(example, "Sequence 1_1_1.png");
            view.update(cx, |v, cx| {
                v.pattern
                    .update(cx, |f, cx| f.set_value("{index:3}-{nope}", window, cx));
                assert!(v.example(cx).is_err());
                v.pattern
                    .update(cx, |f, cx| f.set_value("{index:3}", window, cx));
                v.options.per_layer = true;
                assert_eq!(v.example(cx).unwrap(), "001_Background.png");
                v.options.per_layer = false;
                v.options.format = images::Format::Jpeg;
                v.export_to(dir.path().join("out"), cx);
            });
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let message = view.read(cx).message.clone().unwrap();
            assert!(message.starts_with("Wrote 2 images"), "{message}");
        });
        assert!(dir.path().join("out/001.jpg").is_file());
        assert!(dir.path().join("out/002.jpg").is_file());
    }

    #[gpui_kit::test]
    fn file_menu_and_print_open_storyboard_exports(cx: &mut TestAppContext) {
        let (ws, cx) = crate::tests::open(cx, Document::new(32, 18));
        cx.simulate_resize(gpui_kit::size(px(1600.), px(1200.)));
        let board = ProjectEditor::open(project(), None).unwrap();
        let editor = cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.install_project(board, "Film".into(), window, cx)
            });
            ws.read(cx).editor.clone().unwrap()
        });
        cx.run_until_parked();
        cx.update(|window, cx| editor.update(cx, |e, cx| e.storyboard_print(true, window, cx)));
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.find("storyboard-pdf-options").visible());
            window.close_dialog(cx);
        });
        // Print on a storyboard offers the same layouts.
        cx.update(|window, cx| window.dispatch_action(Box::new(crate::actions::Print), cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.find("storyboard-pdf-options").visible());
            window.close_dialog(cx);
        });
        cx.update(|window, cx| editor.update(cx, |e, cx| e.storyboard_images_dialog(window, cx)));
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.find("storyboard-image-export").visible());
            window.close_dialog(cx);
        });
    }
}
