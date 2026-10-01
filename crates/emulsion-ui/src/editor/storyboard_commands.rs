//! Storyboard commands that keyboard shortcuts reach. On the Board they act
//! on the selected panels; on the Stage, on the active panel. Each one is the
//! same routine as the Board's button or menu item.
use super::*;

/// A storyboard command a shortcut can run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BoardCommand {
    ToggleBoard,
    Add,
    SmartAdd,
    Duplicate,
    Delete,
    Previous,
    Next,
    ToggleLock,
    StartScene,
    Renumber,
    Copy,
    Paste,
}

impl EditorView {
    /// Run a storyboard command. Panel stepping also works on other
    /// multi-page projects; everything else needs a storyboard.
    pub(crate) fn storyboard_command(
        &mut self,
        command: BoardCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(command, BoardCommand::Previous | BoardCommand::Next) {
            self.step_panel(command == BoardCommand::Next, cx);
            return;
        }
        if self.editor.storyboard().is_none() {
            return;
        }
        // The Stage has no panel selection: commands act on the panel drawn.
        if !self.board_open() {
            self.set_board_selection(vec![self.editor.active_page()]);
        }
        match command {
            BoardCommand::ToggleBoard => self.toggle_storyboard_board(window, cx),
            BoardCommand::Add => self.board_add(false, cx),
            BoardCommand::SmartAdd => self.board_add(true, cx),
            BoardCommand::Duplicate => self.board_duplicate(cx),
            BoardCommand::Delete => self.board_delete(cx),
            BoardCommand::ToggleLock => {
                let lock = !self.board_selection_locked();
                self.board_lock_panels(lock, cx);
            }
            BoardCommand::StartScene => {
                let first = self.board_selection()[0];
                self.board_split(first, Level::Scene, cx);
            }
            BoardCommand::Renumber => self.board_renumber_dialog(None, window, cx),
            BoardCommand::Copy => {
                self.board_copy(cx);
            }
            BoardCommand::Paste => self.board_paste(cx),
            BoardCommand::Previous | BoardCommand::Next => unreachable!(),
        }
        cx.notify();
    }

    /// Paste the panel clipboard after the Board selection.
    pub(in crate::editor) fn storyboard_paste_panels(&mut self, cx: &mut Context<Self>) {
        self.board_paste(cx);
    }

    /// Make the previous or next page active; on the Board it is also the
    /// selection.
    fn step_panel(&mut self, next: bool, cx: &mut Context<Self>) {
        let order: Vec<PageId> = self.editor.page_list().iter().map(|m| m.id).collect();
        let Some(at) = order.iter().position(|id| *id == self.editor.active_page()) else {
            return;
        };
        let target = if next {
            order.get(at + 1)
        } else {
            at.checked_sub(1).and_then(|i| order.get(i))
        };
        let Some(&target) = target else {
            return;
        };
        self.select_page(target, cx);
        if self.board_open() {
            self.set_board_selection(vec![target]);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::open;
    use core::prelude::v1::test;
    use emulsion_core::project::{ProjectEditor, ProjectKind};
    use emulsion_core::storyboard::Panel;

    fn storyboard_editor(
        panels: usize,
        cx: &mut TestAppContext,
    ) -> (Entity<EditorView>, &mut VisualTestContext) {
        let (ws, cx) = open(cx, Document::new(64, 36));
        cx.simulate_resize(gpui_kit::size(px(1600.), px(1200.)));
        let mut project =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(64, 36)).unwrap();
        let blank = project.storyboard().unwrap().blank_panel().unwrap();
        let items = (2..=panels)
            .map(|n| (format!("Panel {n}"), Panel::new(0, 24)))
            .collect();
        project.insert_panels(Some(1), &blank, items, None).unwrap();
        let editor = cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.install_project(project, "Board".into(), window, cx)
            });
            let editor = ws.read(cx).editor.clone().unwrap();
            let focus = editor.read(cx).canvas_focus.clone();
            window.focus(&focus, cx);
            editor
        });
        cx.run_until_parked();
        (editor, cx)
    }

    fn pages(e: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<PageId> {
        cx.update(|_, cx| e.read(cx).editor.page_list().iter().map(|m| m.id).collect())
    }

    fn active(e: &Entity<EditorView>, cx: &mut VisualTestContext) -> PageId {
        cx.update(|_, cx| e.read(cx).editor.active_page())
    }

    #[gpui_kit::test]
    fn storyboard_shortcuts_run_board_commands_on_the_stage_and_the_board(cx: &mut TestAppContext) {
        let (e, cx) = storyboard_editor(3, cx);
        let ids = pages(&e, cx);
        cx.update(|_, cx| e.update(cx, |e, cx| e.select_page(ids[0], cx)));
        // Page Down / Page Up step through panels on the Stage.
        cx.simulate_keystrokes("pagedown pagedown");
        assert_eq!(active(&e, cx), ids[2]);
        cx.simulate_keystrokes("pageup");
        assert_eq!(active(&e, cx), ids[1]);
        // Add panel after the active one.
        cx.simulate_keystrokes("ctrl-alt-p");
        cx.run_until_parked();
        let after_add = pages(&e, cx);
        assert_eq!(after_add.len(), 4);
        assert_eq!(
            after_add.iter().position(|id| *id == active(&e, cx)),
            Some(2)
        );
        // Duplicate, then delete the duplicate.
        cx.simulate_keystrokes("ctrl-alt-j");
        cx.run_until_parked();
        assert_eq!(pages(&e, cx).len(), 5);
        cx.simulate_keystrokes("ctrl-shift-backspace");
        cx.run_until_parked();
        assert_eq!(pages(&e, cx).len(), 4);
        // Lock and unlock the active panel.
        cx.simulate_keystrokes("ctrl-alt-l");
        cx.run_until_parked();
        assert!(cx.update(|_, cx| e.read(cx).active_panel_locked()));
        cx.simulate_keystrokes("ctrl-alt-l");
        cx.run_until_parked();
        assert!(!cx.update(|_, cx| e.read(cx).active_panel_locked()));
        // Start a scene at the active panel.
        let scenes = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| e.read(cx).editor.storyboard().unwrap().scenes.len())
        };
        let before = scenes(cx);
        cx.simulate_keystrokes("ctrl-alt-n");
        cx.run_until_parked();
        assert_eq!(scenes(cx), before + 1);
        // The Board opens and closes from the keyboard, and Page Down moves
        // the Board selection too.
        cx.simulate_keystrokes("ctrl-alt-b");
        cx.run_until_parked();
        assert!(cx.update(|_, cx| e.read(cx).board_open()));
        cx.simulate_keystrokes("pagedown");
        cx.run_until_parked();
        let now = active(&e, cx);
        assert_eq!(cx.update(|_, cx| e.read(cx).board_selection()), vec![now]);
        // Copy and paste panels.
        cx.simulate_keystrokes("ctrl-alt-shift-c ctrl-alt-shift-v");
        cx.run_until_parked();
        assert_eq!(pages(&e, cx).len(), 5);
        cx.simulate_keystrokes("ctrl-alt-b");
        cx.run_until_parked();
        assert!(!cx.update(|_, cx| e.read(cx).board_open()));
    }

    #[gpui_kit::test]
    fn layers_paste_in_place_onto_another_panel(cx: &mut TestAppContext) {
        let (e, cx) = storyboard_editor(2, cx);
        let ids = pages(&e, cx);
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                let id = e
                    .editor
                    .execute(Command::AddNode {
                        node: Box::new(Node::raster(
                            0,
                            "Pose",
                            Arc::new(Raster::solid(8, 6, [0., 0., 1., 1.])),
                            Placement::at(5., 4.),
                        )),
                        slot: emulsion_core::command::Slot::TOP,
                    })
                    .unwrap()
                    .unwrap();
                e.after_change(cx);
                e.set_layer_selection(vec![id], Some(id));
                e.copy_pixels(cx);
            })
        });
        cx.update(|_, cx| e.update(cx, |e, cx| e.select_page(ids[1], cx)));
        cx.simulate_keystrokes("ctrl-shift-v");
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = e.read(cx);
            assert_eq!(e.editor.active_page(), ids[1]);
            let pasted = e
                .editor
                .doc
                .nodes
                .iter()
                .find(|n| n.name == "Pose")
                .expect("pasted on the second panel");
            let NodeKind::Raster { placement, .. } = &pasted.kind else {
                panic!()
            };
            assert_eq!((placement.x, placement.y), (5., 4.));
        });
    }
}
