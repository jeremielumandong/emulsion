//! Edit in an external editor (SB2), in the editor: send a panel to the
//! program chosen in Settings › Storyboard (or the system's app for the
//! file type), watch the file, and bring every save back as one Undo step.
//! The file work and the merge rules live in
//! `emulsion_io::storyboard_external_edit`; this holds the polling, the
//! conflict question and the status chip. One panel is out at a time;
//! stopping, starting another or closing the project removes the files.
use super::*;
use emulsion_core::project::PageId;
use emulsion_io::storyboard_external_edit::{
    EditFormat, ExternalEdit, FileStamp, Resolution, edit_root,
};
use gpui_kit::component::{Sizable, button::Button};
use std::path::Path;
use std::time::Duration;

/// How often the file is looked at.
const POLL: Duration = Duration::from_millis(400);

/// A panel out in another program.
pub(crate) struct ExternalUi {
    pub(crate) edit: ExternalEdit,
    /// A save that arrived while the panel also changed here, waiting for
    /// an answer.
    pub(crate) conflict: Option<Document>,
    /// A save is being read.
    reading: bool,
    _poll: Option<Task<()>>,
}

/// The name shown for the external editor setting: "Krita" for
/// `/usr/bin/krita`, "Photoshop" for `Adobe Photoshop.app`'s
/// `Photoshop`, or "external editor" for the system's choice.
pub(crate) fn editor_name(setting: &str) -> String {
    let stem = Path::new(setting.trim())
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = stem.trim_start_matches("Adobe ").trim();
    let mut chars = stem.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => "external editor".into(),
    }
}

/// Open `path` in `program`, or the system's app for it when empty.
fn launch(program: &str, path: &Path, cx: &App) -> Result<(), String> {
    let program = program.trim();
    if program.is_empty() {
        cx.open_with_system(path);
        return Ok(());
    }
    let mut command = if cfg!(target_os = "macos") && program.ends_with(".app") {
        let mut open = std::process::Command::new("open");
        open.arg("-a").arg(program);
        open
    } else {
        std::process::Command::new(program)
    };
    command
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not start {program}: {e}"))
}

impl EditorView {
    /// Send `panel` to the external editor and watch for saves.
    pub(crate) fn start_external_edit(&mut self, panel: PageId, cx: &mut Context<Self>) {
        let Some(board) = self.editor.storyboard() else {
            return;
        };
        if board.is_locked(panel) {
            self.set_status(
                "This panel is locked. Unlock it to edit it elsewhere.",
                true,
                cx,
            );
            return;
        }
        let Some(page) = self.editor.page(panel) else {
            return;
        };
        let root = edit_root(&board.project_id);
        let name = self
            .editor
            .page_list()
            .iter()
            .find(|m| m.id == panel)
            .map_or_else(|| format!("Panel {panel}"), |m| m.name.clone());
        let doc = page.doc.clone();
        let prefs = crate::app_state::settings(cx).storyboard.clone();
        let format = if prefs.external_editor_ora {
            EditFormat::Ora
        } else {
            EditFormat::Psd
        };
        let app = editor_name(&prefs.external_editor);
        let program = prefs.external_editor;
        // One panel at a time: the earlier one's files go now.
        self.extras.external = None;
        self.set_status(format!("Sending {name} to {app}…"), false, cx);
        cx.spawn(async move |this, cx| {
            let started = cx
                .background_spawn(async move {
                    ExternalEdit::start(panel, &name, doc, format, &root, &app)
                })
                .await;
            this.update(cx, |e, cx| match started {
                Ok(edit) => e.watch_external_edit(edit, &program, cx),
                Err(error) => e.set_status(format!("Could not write the panel: {error}"), true, cx),
            })
            .ok();
        })
        .detach();
    }

    /// Open the written file and start polling it.
    fn watch_external_edit(&mut self, edit: ExternalEdit, program: &str, cx: &mut Context<Self>) {
        if let Err(error) = launch(program, &edit.path, cx) {
            self.set_status(error, true, cx);
            return;
        }
        self.set_status(
            format!("Editing in {}. Each save comes back here.", edit.app),
            false,
            cx,
        );
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                if this.update(cx, |e, cx| e.poll_external_edit(cx)).is_err() {
                    break;
                }
            }
        });
        self.extras.external = Some(ExternalUi {
            edit,
            conflict: None,
            reading: false,
            _poll: Some(poll),
        });
        cx.notify();
    }

    /// Look at the file; read a settled save.
    pub(crate) fn poll_external_edit(&mut self, cx: &mut Context<Self>) {
        let Some(ui) = &mut self.extras.external else {
            return;
        };
        if ui.reading || ui.conflict.is_some() {
            return;
        }
        if !ui
            .edit
            .watch
            .poll(FileStamp::of(&ui.edit.path), Instant::now())
        {
            return;
        }
        ui.reading = true;
        let (format, path) = (ui.edit.format, ui.edit.path.clone());
        cx.spawn(async move |this, cx| {
            let read = cx.background_spawn(async move { format.read(&path) }).await;
            this.update(cx, |e, cx| e.external_save_read(read, cx)).ok();
        })
        .detach();
    }

    /// A save was read: bring it back, or ask when the panel also changed
    /// here.
    pub(crate) fn external_save_read(
        &mut self,
        read: Result<Document, String>,
        cx: &mut Context<Self>,
    ) {
        let Some(ui) = &mut self.extras.external else {
            return;
        };
        ui.reading = false;
        let doc = match read {
            Ok(doc) => doc,
            Err(error) => {
                let app = ui.edit.app.clone();
                self.set_status(
                    format!("Could not read the save from {app}: {error}"),
                    true,
                    cx,
                );
                return;
            }
        };
        if ui.edit.changed_here(&self.editor) {
            ui.conflict = Some(doc);
            cx.notify();
        } else {
            self.bring_back_external(doc, Resolution::TakeExternal, cx);
        }
    }

    /// Answer the conflict question.
    pub(crate) fn resolve_external_conflict(
        &mut self,
        resolution: Resolution,
        cx: &mut Context<Self>,
    ) {
        let Some(doc) = self
            .extras
            .external
            .as_mut()
            .and_then(|ui| ui.conflict.take())
        else {
            return;
        };
        self.bring_back_external(doc, resolution, cx);
    }

    fn bring_back_external(
        &mut self,
        doc: Document,
        resolution: Resolution,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            // Ask again once the current edit is finished.
            if let Some(ui) = &mut self.extras.external {
                ui.conflict = Some(doc);
            }
            return;
        }
        let Some(ui) = &mut self.extras.external else {
            return;
        };
        let app = ui.edit.app.clone();
        match ui.edit.bring_back(&mut self.editor, &doc, resolution) {
            Ok(true) => {
                self.after_change(cx);
                self.set_status(format!("Brought back the save from {app}."), false, cx);
            }
            Ok(false) => cx.notify(),
            Err(error) => self.set_status(error, true, cx),
        }
    }

    /// Stop watching and remove the files.
    pub(crate) fn stop_external_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(ui) = self.extras.external.take() {
            self.set_status(format!("Stopped editing in {}.", ui.edit.app), false, cx);
        }
        cx.notify();
    }

    /// The status chip: what is out where, with Stop, or the conflict
    /// question.
    pub(super) fn external_edit_chip(
        &self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let ui = self.extras.external.as_ref()?;
        let app = ui.edit.app.clone();
        let name = self
            .editor
            .page_list()
            .iter()
            .find(|m| m.id == ui.edit.panel)
            .map_or_else(String::new, |m| m.name.clone());
        let button =
            |id: &'static str, label: String| Button::new(id).label(label).xsmall().outline();
        let mut chip = div()
            .id("external-edit-chip")
            .test_support()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .px_3()
            .py_1()
            .rounded(px(6.))
            .bg(p.panel.opacity(0.95))
            .border_1()
            .border_color(p.accent)
            .text_size(px(12.))
            .text_color(p.ink);
        if ui.conflict.is_some() {
            chip = chip
                .child(format!("{name} changed here and in {app}."))
                .child(button("external-keep-both", "Keep both".into()).on_click(
                    cx.listener(|e, _, _, cx| {
                        e.resolve_external_conflict(Resolution::KeepBoth, cx)
                    }),
                ))
                .child(
                    button("external-take", format!("Take {app}'s")).on_click(cx.listener(
                        |e, _, _, cx| e.resolve_external_conflict(Resolution::TakeExternal, cx),
                    )),
                )
                .child(
                    button("external-keep-mine", "Keep mine".into()).on_click(cx.listener(
                        |e, _, _, cx| e.resolve_external_conflict(Resolution::KeepMine, cx),
                    )),
                );
        } else {
            chip = chip.child(format!("Editing {name} in {app}…")).child(
                button("external-stop", "Stop".into())
                    .on_click(cx.listener(|e, _, _, cx| e.stop_external_edit(cx))),
            );
        }
        Some(
            div()
                .absolute()
                .top(px(44.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(chip)
                .into_any_element(),
        )
    }
}
