//! Generate editable local pages from CSV, keeping the source page intact.
use super::*;
use crate::file_prompt::FilePrompts;
use emulsion_io::design_bulk;
use gpui_kit::component::{
    Sizable, WindowExt,
    button::Button,
    input::{Textarea, TextareaState},
};

impl EditorView {
    pub(crate) fn design_bulk_dialog(
        &mut self,
        initial: Option<(String, PathBuf)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(project) = self.editor.snapshot() else {
            return;
        };
        let all_pages: Vec<_> = project.pages.iter().map(|p| p.meta.id).collect();
        let active = project.active;
        let fields = match design_bulk::project_fields(&project, &all_pages)
            .or_else(|_| design_bulk::fields(&self.editor.doc))
        {
            Ok(fields) => fields,
            Err(error) => {
                self.set_status(error.to_string(), true, cx);
                return;
            }
        };
        let all = cx.new(|_| design_bulk::fields(&self.editor.doc).is_err());
        let base = initial
            .as_ref()
            .map(|(_, path)| path.clone())
            .or_else(|| {
                self.editor
                    .path
                    .as_ref()
                    .and_then(|p| p.parent())
                    .map(PathBuf::from)
            })
            .unwrap_or_else(|| PathBuf::from("."));
        let directory = cx.new(|cx| {
            InputState::new(window, cx).default_value(base.to_string_lossy().into_owned())
        });
        let available =
            emulsion_core::project::MAX_PAGES.saturating_sub(self.editor.page_list().len());
        if available == 0 {
            self.set_status(t!("editor.design_bulk_ui.page_limit"), true, cx);
            return;
        }
        let example = || {
            format!(
                "{}\n{}",
                fields
                    .iter()
                    .map(|f| format!("\"{}\"", f.replace('"', "\"\"")))
                    .collect::<Vec<_>>()
                    .join(","),
                vec!["Example"; fields.len()].join(",")
            )
        };
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(10)
                .default_value(initial.map(|(csv, _)| csv).unwrap_or_else(example))
        });
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        let stamp = self.editor.stamp();
        let help = t!(
            "editor.design_bulk_ui.help",
            fields = fields.join(", "),
            available = available
        );
        window.open_dialog(cx, move |dialog, _, cx| {
            let input = input.clone();
            let owner = owner.clone();
            let file_owner = owner.clone();
            let all_apply = all.clone();
            let change = all.clone();
            let directory = directory.clone();
            let stamp = stamp.clone();
            let all_pages = all_pages.clone();
            dialog
                .title(t!("editor.design_bulk_ui.title"))
                .width(px(880.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(help.clone())
                        .child(
                            Button::new("design-bulk-scope")
                                .label(if *all.read(cx) {
                                    t!("editor.design_bulk_ui.scope_all")
                                } else {
                                    t!("editor.design_bulk_ui.scope_current")
                                })
                                .small()
                                .outline()
                                .on_click(move |_, window, cx| {
                                    change.update(cx, |v, cx| {
                                        *v = !*v;
                                        cx.notify();
                                    });
                                    window.refresh();
                                }),
                        )
                        .child(t!("editor.design_bulk_ui.folder"))
                        .child(Input::new(&directory).id("design-bulk-directory"))
                        .child(
                            Button::new("design-bulk-import")
                                .label(t!("editor.design_bulk_ui.import_csv"))
                                .small()
                                .outline()
                                .on_click(move |_, window, cx| {
                                    file_owner
                                        .update(cx, |this, cx| this.design_bulk_import(window, cx))
                                        .ok();
                                }),
                        )
                        .child(
                            div()
                                .id("design-bulk-csv")
                                .test_support()
                                .child(Textarea::new(&input).h(rems(18.)).flex_shrink_0()),
                        ),
                )
                .footer(crate::widgets::form_dialog_footer(t!(
                    "editor.design_bulk_ui.create_pages"
                )))
                .on_ok(move |_, _, cx| {
                    let csv = input.read(cx).value().to_string();
                    let pages = if *all_apply.read(cx) {
                        all_pages.clone()
                    } else {
                        vec![active]
                    };
                    let base = PathBuf::from(directory.read(cx).value().trim());
                    let stamp = stamp.clone();
                    owner
                        .update(cx, |this, cx| {
                            if this.edit_ticket() != ticket || this.editor.stamp() != stamp {
                                this.set_status(
                                    t!("editor.design_bulk_ui.project_changed"),
                                    true,
                                    cx,
                                );
                                return false;
                            }
                            let Some(source) = this.editor.snapshot() else {
                                return false;
                            };
                            this.set_status(t!("editor.design_bulk_ui.creating"), false, cx);
                            cx.spawn(async move |this, cx| {
                                let result = cx
                                    .background_spawn(async move {
                                        design_bulk::generate_pages(
                                            &source,
                                            &pages,
                                            &csv,
                                            available,
                                            Some(&base),
                                        )
                                    })
                                    .await;
                                this.update(cx, |this, cx| {
                                    if this.edit_ticket() != ticket
                                        || this.editor.stamp() != stamp
                                        || this.editor.in_transaction()
                                    {
                                        this.set_status(
                                            t!("editor.design_bulk_ui.project_changed_generate"),
                                            true,
                                            cx,
                                        );
                                        return;
                                    }
                                    match result
                                        .map_err(|e| e.to_string())
                                        .and_then(|project| this.editor.import_pages(project))
                                    {
                                        Ok(ids) => {
                                            this.after_change(cx);
                                            this.set_tool(Tool::Move, cx);
                                            this.set_status(
                                                t!(
                                                    "editor.design_bulk_ui.created",
                                                    count = ids.len()
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
                            true
                        })
                        .unwrap_or(false)
                })
        });
    }

    fn design_bulk_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ticket = self.edit_ticket();
        let paths = cx.prompt_open_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t!("editor.design_bulk_ui.import_prompt").into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let result = cx
                .background_spawn(async move {
                    use std::io::Read;
                    let mut text = String::new();
                    std::fs::File::open(&path)?
                        .take(design_bulk::MAX_CSV_BYTES as u64 + 1)
                        .read_to_string(&mut text)?;
                    if text.len() > design_bulk::MAX_CSV_BYTES {
                        return Err(std::io::Error::other(t!(
                            "editor.design_bulk_ui.csv_too_large"
                        )));
                    }
                    Ok::<_, std::io::Error>((
                        text,
                        path.parent()
                            .unwrap_or_else(|| std::path::Path::new("."))
                            .to_path_buf(),
                    ))
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                if this.edit_ticket() != ticket {
                    this.set_status(t!("editor.design_bulk_ui.page_changed"), true, cx);
                    return;
                }
                match result {
                    Ok(text) => {
                        window.close_dialog(cx);
                        this.design_bulk_dialog(Some(text), window, cx);
                    }
                    Err(error) => this.set_status(error.to_string(), true, cx),
                }
            })
            .ok();
        })
        .detach();
    }
}
