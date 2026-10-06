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

/// Consume only the initial write's report. Watched saves must never reuse it.
fn external_edit_message(edit: &mut ExternalEdit) -> String {
    let report = edit.take_initial_write_report();
    let message = format!("Editing in {}. Each save comes back here.", edit.app);
    match super::export_ui::psd_export_notice(&edit.path, report) {
        Some(notice) => format!("{notice}\n{message}"),
        None => message,
    }
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
    fn watch_external_edit(
        &mut self,
        mut edit: ExternalEdit,
        program: &str,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = launch(program, &edit.path, cx) {
            self.set_status(error, true, cx);
            return;
        }
        self.set_status(external_edit_message(&mut edit), false, cx);
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

    /// The external edit's text and buttons: what is out where with Stop,
    /// or the conflict question. `panel` names them for a Board card (the
    /// card names the panel); `None` for the chip over the Stage or Board.
    fn external_edit_controls(
        &self,
        panel: Option<PageId>,
        cx: &mut Context<Self>,
    ) -> Option<Vec<AnyElement>> {
        let ui = self.extras.external.as_ref()?;
        if panel.is_some_and(|id| id != ui.edit.panel) {
            return None;
        }
        let app = ui.edit.app.clone();
        let name = self
            .editor
            .page_list()
            .iter()
            .find(|m| m.id == ui.edit.panel)
            .map_or_else(String::new, |m| m.name.clone());
        let button = |id: &'static str, label: String| {
            let id: ElementId = match panel {
                Some(panel) => (id, panel).into(),
                None => id.into(),
            };
            Button::new(id).label(label).xsmall().outline()
        };
        // On a card the click must not also select the card.
        let act = |f: fn(&mut Self, &mut Context<Self>)| {
            cx.listener(move |e: &mut Self, _: &ClickEvent, _: &mut Window, cx| {
                cx.stop_propagation();
                f(e, cx)
            })
        };
        Some(if ui.conflict.is_some() {
            vec![
                if panel.is_some() {
                    format!("Changed here and in {app}.")
                } else {
                    format!("{name} changed here and in {app}.")
                }
                .into_any_element(),
                button("external-keep-both", "Keep both".into())
                    .on_click(act(|e, cx| {
                        e.resolve_external_conflict(Resolution::KeepBoth, cx)
                    }))
                    .into_any_element(),
                button("external-take", format!("Take {app}'s"))
                    .on_click(act(|e, cx| {
                        e.resolve_external_conflict(Resolution::TakeExternal, cx)
                    }))
                    .into_any_element(),
                button("external-keep-mine", "Keep mine".into())
                    .on_click(act(|e, cx| {
                        e.resolve_external_conflict(Resolution::KeepMine, cx)
                    }))
                    .into_any_element(),
            ]
        } else {
            vec![
                if panel.is_some() {
                    format!("Editing in {app}…")
                } else {
                    format!("Editing {name} in {app}…")
                }
                .into_any_element(),
                button("external-stop", "Stop".into())
                    .on_click(act(|e, cx| e.stop_external_edit(cx)))
                    .into_any_element(),
            ]
        })
    }

    /// The status chip over the Stage or Board.
    pub(super) fn external_edit_chip(
        &self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let controls = self.external_edit_controls(None, cx)?;
        let chip = div()
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
            .text_color(p.ink)
            .children(controls);
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

    /// The edit's status on `panel`'s Board card, with Stop or the
    /// conflict question; `None` unless that panel is out.
    pub(crate) fn external_card_status(
        &self,
        panel: PageId,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let controls = self.external_edit_controls(Some(panel), cx)?;
        Some(
            div()
                .id(("board-external", panel))
                .test_support()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .flex()
                .flex_wrap()
                .items_center()
                .gap_1()
                .px_1()
                .rounded(px(3.))
                .border_1()
                .border_color(p.accent)
                .text_size(px(10.5))
                .text_color(p.ink)
                .children(controls)
                .into_any_element(),
        )
    }

    /// A mark on `panel`'s thumbnail in the panel strip while it is out,
    /// or waiting for the conflict question.
    pub(crate) fn external_strip_badge(&self, panel: PageId, p: &Palette) -> Option<AnyElement> {
        let ui = self.extras.external.as_ref()?;
        if ui.edit.panel != panel {
            return None;
        }
        let (text, tip) = if ui.conflict.is_some() {
            ("!", format!("Changed here and in {}", ui.edit.app))
        } else {
            ("✎", format!("Editing in {}…", ui.edit.app))
        };
        Some(
            div()
                .id(("strip-external", panel))
                .test_support()
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                })
                .absolute()
                .top_0()
                .left_0()
                .px_1()
                .rounded(px(3.))
                .bg(p.accent)
                .text_color(p.accent_fg)
                .text_size(px(10.))
                .child(text)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::storyboard_worksheets::tests::storyboard;
    use super::*;
    use core::prelude::v1::test;
    use emulsion_core::command::Slot;
    use emulsion_core::{Command, Node, NodeKind};
    use gpui_kit::test::TestWindowExt;

    /// `doc` with a layer `name` added on top, as another program saves it.
    fn with_layer(doc: &Document, name: &str) -> Document {
        let raster = emulsion_raster::Raster::solid(doc.width, doc.height, [0.2, 0.2, 0.2, 0.5]);
        let mut editor = emulsion_core::Editor::new(doc.clone(), None);
        editor
            .execute(Command::AddNode {
                node: Box::new(Node::new(
                    0,
                    name,
                    NodeKind::Raster {
                        raster: std::sync::Arc::new(raster),
                        placement: Default::default(),
                    },
                )),
                slot: Slot::TOP,
            })
            .unwrap();
        editor.doc
    }

    #[test]
    fn initial_external_edit_notices_are_file_specific_and_never_replayed() {
        let root = tempfile::tempdir().unwrap();
        for format in [EditFormat::Psd, EditFormat::Ora] {
            let mut doc = Document::new(16, 12);
            let mut node = Node::raster(
                1,
                "Ink",
                Arc::new(emulsion_raster::Raster::solid(16, 12, [0., 0., 0., 1.])),
                Default::default(),
            );
            node.mask = Some(Arc::new(emulsion_raster::Mask::empty(16, 12, 127)));
            doc.nodes.push(node);
            doc.next_id = 2;
            let mut edit =
                ExternalEdit::start(1, "Panel", doc, format, root.path(), "Krita").unwrap();
            let base = "Editing in Krita. Each save comes back here.";
            let message = external_edit_message(&mut edit);
            if format == EditFormat::Psd {
                assert!(message.starts_with("Exported flattened PSD appearance"));
                assert!(message.ends_with(base));
                assert!(message.contains(&edit.path.display().to_string()));
                assert_eq!(message.matches("flattened PSD appearance").count(), 1);
            } else {
                assert_eq!(message, base);
            }
            let stamp = FileStamp::of(&edit.path);
            let now = Instant::now();
            for i in 0..3 {
                assert!(!edit.watch.poll(
                    stamp,
                    now + emulsion_io::storyboard_external_edit::SETTLE * i
                ));
                assert_eq!(external_edit_message(&mut edit), base, "no repeated notice");
            }
        }
    }

    #[cfg(unix)]
    #[gpui_kit::test]
    fn external_start_completion_shows_actual_loss_once(cx: &mut TestAppContext) {
        let (e, cx) = storyboard(cx);
        let (panel, original, stamp, source) = cx.update(|_, cx| {
            // Exercise the real launch/completion path with an inert system
            // command, without opening a painting application in the test.
            let prefs = &mut cx
                .global_mut::<crate::app_state::AppSettings>()
                .0
                .storyboard;
            prefs.external_editor = "/usr/bin/true".into();
            prefs.external_editor_ora = false;
            e.update(cx, |e, _| {
                let panel = e.editor.active_page();
                let current = &e.editor.page(panel).unwrap().doc;
                let (w, h) = (current.width, current.height);
                let mut doc = Document::new(w, h);
                let mut node = Node::raster(
                    1,
                    "Ink",
                    Arc::new(emulsion_raster::Raster::solid(w, h, [0., 0., 0., 1.])),
                    Default::default(),
                );
                node.mask = Some(Arc::new(emulsion_raster::Mask::empty(w, h, 127)));
                doc.nodes.push(node);
                doc.next_id = 2;
                e.editor
                    .replace_panel_document(panel, doc.clone(), "Fixture")
                    .unwrap();
                (panel, doc, e.editor.stamp(), e.source.clone())
            })
        });
        cx.update(|_, cx| e.update(cx, |e, cx| e.start_external_edit(panel, cx)));
        cx.run_until_parked();
        let path = cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                let path = e.extras.external.as_ref().unwrap().edit.path.clone();
                let (message, error) = e.status.as_ref().unwrap();
                assert!(!*error);
                assert!(
                    message.starts_with("Exported flattened PSD appearance"),
                    "{message}"
                );
                assert!(
                    message.ends_with("Editing in True. Each save comes back here."),
                    "{message}"
                );
                assert!(message.contains(&path.display().to_string()));
                assert_eq!(message.matches("flattened PSD appearance").count(), 1);
                let message = message.clone();
                for _ in 0..3 {
                    e.poll_external_edit(cx);
                    assert_eq!(e.status.as_ref().unwrap().0, message);
                }
                assert!(
                    e.extras
                        .external
                        .as_mut()
                        .unwrap()
                        .edit
                        .take_initial_write_report()
                        .is_none()
                );
                assert_eq!(e.editor.page(panel).unwrap().doc, original);
                assert_eq!(e.editor.stamp(), stamp);
                assert_eq!(e.source, source);
                e.set_status("Later status", false, cx);
                e.poll_external_edit(cx);
                assert_eq!(e.status.as_ref().unwrap().0.as_ref(), "Later status");
                e.stop_external_edit(cx);
                path
            })
        });
        assert!(!path.exists());
    }

    #[gpui_kit::test]
    fn failed_external_editor_launch_does_not_show_export_success(cx: &mut TestAppContext) {
        let (e, cx) = storyboard(cx);
        let root = tempfile::tempdir().unwrap();
        let (panel, original, stamp, source) = cx.update(|_, cx| {
            let e = e.read(cx);
            let panel = e.editor.active_page();
            (
                panel,
                e.editor.page(panel).unwrap().doc.clone(),
                e.editor.stamp(),
                e.source.clone(),
            )
        });
        let edit = ExternalEdit::start(
            panel,
            "Panel",
            original.clone(),
            EditFormat::Psd,
            root.path(),
            "Missing editor",
        )
        .unwrap();
        let path = edit.path.clone();
        let missing_program = root.path().join("no-such-editor");
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.watch_external_edit(edit, missing_program.to_str().unwrap(), cx);
                let (message, error) = e.status.as_ref().unwrap();
                assert!(*error);
                assert!(message.contains("Could not start"));
                assert!(!message.contains("Editing in") && !message.contains("Exported"));
                assert!(e.extras.external.is_none());
                assert_eq!(e.editor.page(panel).unwrap().doc, original);
                assert_eq!(e.editor.stamp(), stamp);
                assert_eq!(e.source, source);
            })
        });
        assert!(
            !path.exists(),
            "unsuccessful hand-off cleans up its temporary file"
        );
    }

    #[gpui_kit::test]
    fn the_panel_out_shows_on_its_card_and_strip_with_stop_and_the_conflict(
        cx: &mut TestAppContext,
    ) {
        let (e, cx) = storyboard(cx);
        let ids: Vec<_> =
            cx.update(|_, cx| e.read(cx).editor.page_list().iter().map(|m| m.id).collect());
        let root = tempfile::tempdir().unwrap();
        let original = cx.update(|_, cx| e.read(cx).editor.page(ids[1]).unwrap().doc.clone());
        let edit = ExternalEdit::start(
            ids[1],
            "Panel 2",
            original.clone(),
            EditFormat::Psd,
            root.path(),
            "Krita",
        )
        .unwrap();
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.extras.external = Some(ExternalUi {
                    edit,
                    conflict: None,
                    reading: false,
                    _poll: None,
                });
                cx.notify();
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.find(("strip-external", ids[1])).visible());
            assert!(window.try_find(("strip-external", ids[0])).is_none());
        });
        cx.update(|window, cx| window.click("storyboard-view-toggle", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.find(("board-external", ids[1])).visible());
            assert!(window.find(("external-stop", ids[1])).visible());
            assert!(window.try_find(("board-external", ids[0])).is_none());
        });
        // A save comes back as one Undo step; the card stays.
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.external_save_read(Ok(with_layer(&original, "Shading")), cx)
            })
        });
        cx.run_until_parked();
        let doc = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| e.read(cx).editor.page(ids[1]).unwrap().doc.clone())
        };
        let names = |cx: &mut VisualTestContext| {
            doc(cx)
                .nodes
                .iter()
                .map(|n| n.name.clone())
                .collect::<Vec<_>>()
        };
        assert!(names(cx).contains(&"Shading".to_string()));
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.find(("board-external", ids[1])).visible());
        });
        cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
        assert!(doc(cx) == original, "one Undo takes the whole save back");
        cx.update(|_, cx| e.update(cx, |e, cx| e.redo(cx)));
        assert!(names(cx).contains(&"Shading".to_string()));
        // Changed here as well: the card asks.
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                let here = with_layer(&e.editor.page(ids[1]).unwrap().doc, "Mine");
                e.editor
                    .replace_panel_document(ids[1], here, "Draw")
                    .unwrap();
                e.external_save_read(Ok(with_layer(&original, "Theirs")), cx)
            })
        });
        cx.run_until_parked();
        let mine = doc(cx);
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.find(("external-keep-both", ids[1])).visible());
            assert!(window.find(("external-keep-mine", ids[1])).visible());
            assert!(window.find(("strip-external", ids[1])).visible());
            window.click(("external-take", ids[1]), cx);
        });
        cx.run_until_parked();
        assert!(names(cx).contains(&"Theirs".to_string()));
        assert!(!names(cx).contains(&"Mine".to_string()));
        cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
        assert!(doc(cx) == mine, "one Undo brings mine back");
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(
                window.find(("external-stop", ids[1])).visible(),
                "the question was answered"
            );
            // Stop on the card ends the edit and clears the card.
            window.click(("external-stop", ids[1]), cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(e.read(cx).extras.external.is_none());
            assert!(window.try_find(("board-external", ids[1])).is_none());
            assert!(window.try_find(("strip-external", ids[1])).is_none());
        });
    }
}
