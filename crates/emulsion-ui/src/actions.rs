//! Actions and key bindings.
//!
//! Contexts: `Workspace` (whole window, modifier shortcuts), `Canvas` (bare
//! keys that must not fire while typing), `NodePanel` (delete and friends).
//! Text inputs sit deeper than all three, so their own bindings win.

use gpui_kit::*;

gpui_kit::actions!(
    emulsion,
    [
        NewDocument,
        Open,
        Save,
        SaveAs,
        Export,
        Undo,
        Redo,
        ZoomIn,
        ZoomOut,
        ZoomFit,
        Zoom100,
        RotateCw,
        RotateCcw,
        ResetRotation,
        ToggleRulers,
        ToggleTheme,
        ShowHome,
        ShowEditor,
        DeleteNode,
        DuplicateNode,
        GroupNodes,
        Ungroup,
        MoveNodeUp,
        MoveNodeDown,
        ToggleNodeVisible,
        Ask,
        ShowSettings,
        Suggestion1,
        Suggestion2,
        Suggestion3,
        Suggestion4,
        Quit,
    ]
);

pub fn bind(cx: &mut App) {
    let ws = Some("Workspace");
    let canvas = Some("Canvas");
    let panel = Some("NodePanel");
    cx.bind_keys([
        KeyBinding::new("ctrl-n", NewDocument, ws),
        KeyBinding::new("ctrl-o", Open, ws),
        KeyBinding::new("ctrl-s", Save, ws),
        KeyBinding::new("ctrl-shift-s", SaveAs, ws),
        KeyBinding::new("ctrl-shift-e", Export, ws),
        KeyBinding::new("ctrl-z", Undo, ws),
        KeyBinding::new("ctrl-shift-z", Redo, ws),
        KeyBinding::new("ctrl-y", Redo, ws),
        KeyBinding::new("ctrl-=", ZoomIn, ws),
        KeyBinding::new("ctrl-+", ZoomIn, ws),
        KeyBinding::new("ctrl-shift-=", ZoomIn, ws),
        KeyBinding::new("ctrl--", ZoomOut, ws),
        KeyBinding::new("ctrl-0", ZoomFit, ws),
        KeyBinding::new("ctrl-1", Zoom100, ws),
        KeyBinding::new("ctrl-r", ToggleRulers, ws),
        KeyBinding::new("ctrl-j", DuplicateNode, ws),
        KeyBinding::new("ctrl-g", GroupNodes, ws),
        KeyBinding::new("ctrl-shift-g", Ungroup, ws),
        KeyBinding::new("ctrl-]", MoveNodeUp, ws),
        KeyBinding::new("ctrl-[", MoveNodeDown, ws),
        KeyBinding::new("ctrl-,", ToggleNodeVisible, ws),
        KeyBinding::new("ctrl-q", Quit, ws),
        KeyBinding::new("ctrl-k", Ask, ws),
        KeyBinding::new("alt-1", Suggestion1, ws),
        KeyBinding::new("alt-2", Suggestion2, ws),
        KeyBinding::new("alt-3", Suggestion3, ws),
        KeyBinding::new("alt-4", Suggestion4, ws),
        KeyBinding::new("r", RotateCw, canvas),
        KeyBinding::new("shift-r", RotateCcw, canvas),
        KeyBinding::new("escape", ResetRotation, canvas),
        KeyBinding::new("delete", DeleteNode, canvas),
        KeyBinding::new("backspace", DeleteNode, canvas),
        KeyBinding::new("delete", DeleteNode, panel),
        KeyBinding::new("backspace", DeleteNode, panel),
    ]);
}
