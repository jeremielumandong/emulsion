//! Handlers for the storyboard commands and Paste in Place, so their
//! shortcuts (and any the person rebinds) reach the active editor.
use super::*;
use crate::editor::BoardCommand;

/// Each storyboard action and the command it runs.
macro_rules! board_actions {
    ($d:expr, $cx:expr; $($action:ident => $command:ident),* $(,)?) => {
        $d$(.on_action($cx.listener(|this, _: &$action, window, cx| {
            this.with_editor(cx, |e, cx| {
                e.storyboard_command(BoardCommand::$command, window, cx)
            })
        })))*
    };
}

impl Workspace {
    pub(super) fn storyboard_actions(d: Stateful<Div>, cx: &Context<Self>) -> Stateful<Div> {
        let d = d
            .on_action(cx.listener(|this, _: &PasteInPlace, _, cx| {
                this.with_editor(cx, |e, cx| e.paste_in_place(cx))
            }))
            .on_action(cx.listener(|this, _: &NewReviewLayer, _, cx| {
                this.with_editor(cx, |e, cx| {
                    e.new_review_layer(cx);
                })
            }))
            .on_action(cx.listener(|this, _: &NextChange, _, cx| {
                this.with_editor(cx, |e, cx| e.step_change(true, cx))
            }))
            .on_action(cx.listener(|this, _: &PreviousChange, _, cx| {
                this.with_editor(cx, |e, cx| e.step_change(false, cx))
            }))
            .on_action(cx.listener(|this, _: &ToggleChangeMarks, _, cx| {
                this.with_editor(cx, |e, cx| e.toggle_change_marks(cx))
            }));
        board_actions!(d, cx;
            ToggleStoryboardBoard => ToggleBoard,
            AddPanel => Add,
            SmartAddPanel => SmartAdd,
            DuplicatePanel => Duplicate,
            DeletePanel => Delete,
            PreviousPanel => Previous,
            NextPanel => Next,
            TogglePanelLock => ToggleLock,
            StartScene => StartScene,
            RenumberPanels => Renumber,
            CopyPanels => Copy,
            PastePanels => Paste,
        )
    }
}
