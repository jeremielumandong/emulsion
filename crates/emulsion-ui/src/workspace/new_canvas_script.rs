//! New storyboard › Start from a script…: create the board with the chosen
//! size and preferences, then lay a Fountain, Final Draft or plain-text
//! script out on it, one panel per action or dialogue block. The script's
//! title names the project.
use super::*;
use crate::editor::storyboard_script::{project_from_script, read_script};
use crate::file_prompt::FilePrompts;
use emulsion_io::script::{Script, storyboard::Split};

impl NewCanvas {
    pub(super) fn script_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        Button::new("new-canvas-script")
            .label("Start from a script…")
            .small()
            .outline()
            .disabled(self.submitted)
            .tooltip(
                "A Fountain, Final Draft or text script, one panel per action or dialogue block",
            )
            .on_click(cx.listener(|this, _, window, cx| this.choose_script(window, cx)))
    }

    fn choose_script(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_open_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose a Fountain (.fountain), Final Draft (.fdx) or text script".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let read = cx.background_spawn(async move { read_script(&path) }).await;
            this.update_in(cx, |this, window, cx| {
                if this.start_from_script(read, window, cx) {
                    window.close_dialog(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Create the storyboard from the dialog's choices and `script`; true
    /// when it opened.
    pub(super) fn start_from_script(
        &mut self,
        script: Result<Script, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.submitted {
            return false;
        }
        let preferences = crate::app_state::settings(cx).storyboard.clone();
        let result = script.and_then(|script| {
            let mut spec = self.draft(cx)?;
            if let Some(title) = script
                .title
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
            {
                spec.name = title.chars().take(200).collect();
            }
            let project = spec.create_project_with(&preferences)?;
            Ok((spec, project_from_script(project, &script, Split::Beat)?))
        });
        match result {
            Ok((spec, project)) => {
                let doc = project.doc.clone();
                self.install_created(spec, doc, Some(project), window, cx)
            }
            Err(error) => {
                self.notice = Some(error);
                cx.notify();
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::open;
    use ::core::prelude::v1::test;

    #[gpui_kit::test]
    fn a_new_storyboard_starts_from_a_script_named_after_its_title(cx: &mut TestAppContext) {
        let (ws, cx) = open(cx, Document::new(64, 36));
        let script = emulsion_io::script::parse_fountain(
            "Title: Night Shift\n\nINT. LAB - NIGHT\n\nSparks fly.\n\nEXT. ROOF - DAWN\n\nShe waits.\n",
        );
        let opened = cx.update(|window, cx| {
            let view = cx.new(|cx| {
                let mut view = NewCanvas::new(ws.downgrade(), None, window, cx);
                view.pick_kind(CanvasKind::Storyboard, window, cx);
                view
            });
            // An unreadable script is explained in the dialog.
            assert!(!view.update(cx, |v, cx| v.start_from_script(
                Err("Not a script.".into()),
                window,
                cx
            )));
            assert_eq!(view.read(cx).notice.as_deref(), Some("Not a script."));
            view.update(cx, |v, cx| v.start_from_script(Ok(script), window, cx))
        });
        assert!(opened);
        cx.run_until_parked();
        cx.update(|_, cx| {
            let workspace = ws.read(cx);
            assert_eq!(workspace.tabs.len(), 2);
            let editor = workspace.editor.as_ref().unwrap().read(cx);
            assert_eq!(editor.name, "Night Shift");
            let pages = editor.editor.page_list();
            assert_eq!(pages.len(), 2, "only the script's panels");
            let board = editor.editor.storyboard().unwrap();
            let names: Vec<_> = board.scenes.values().map(|s| s.name.as_str()).collect();
            assert_eq!(names, ["INT. LAB - NIGHT", "EXT. ROOF - DAWN"]);
            let action = board.caption("Action").unwrap();
            assert_eq!(
                board.panels[&pages[1].id].captions[&action].text,
                "She waits."
            );
            assert!(!editor.editor.can_undo());
        });
    }
}
