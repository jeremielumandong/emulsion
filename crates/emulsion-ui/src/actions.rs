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
        ToolHand,
        ToolMove,
        ToolPen,
        ToolType,
        ToolMarquee,
        ToolLasso,
        ToolWand,
        ToolBrush,
        ToolEraser,
        ToolBucket,
        ToolGradient,
        ToolHeal,
        ToolClone,
        ToolCrop,
        ToolShape,
        ToolEyedropper,
        ToolZoom,
        ToolEllipseMarquee,
        ToolPolygonLasso,
        ToolMagneticLasso,
        ToolQuickSelect,
        ToolSmudge,
        ToolLiquify,
        ToolEllipse,
        ToolMask,
        ToolGrade,
        ImageSizeDialog,
        CanvasSizeDialog,
        NextTab,
        PrevTab,
        CloseTab,
        SwapColors,
        DefaultColors,
        BrushSmaller,
        BrushLarger,
        CommitTool,
        SelectAll,
        Deselect,
        InvertSelection,
        FillSelection,
        ContentAwareFill,
        CopyPixels,
        CutPixels,
        PastePixels,
        ClearPixels,
        CanvasDelete,
        FreeTransform,
        NudgeLeft,
        NudgeRight,
        NudgeUp,
        NudgeDown,
        NudgeLeftLarge,
        NudgeRightLarge,
        NudgeUpLarge,
        NudgeDownLarge,
        ShowSettings,
        Suggestion1,
        Suggestion2,
        Suggestion3,
        Suggestion4,
        Quit,
    ]
);

/// Every action a key can trigger, by name, so a keymap file can refer
/// to them.
macro_rules! make_binding {
    ($name:expr, $keys:expr, $ctx:expr; $($a:ident),* $(,)?) => {
        match $name {
            $(stringify!($a) => Some(KeyBinding::new($keys, $a, $ctx)),)*
            _ => None,
        }
    };
}

/// A binding for the action called `name`, or None for an unknown name.
pub fn binding(name: &str, keys: &str, ctx: Option<&str>) -> Option<KeyBinding> {
    make_binding!(name, keys, ctx;
        NewDocument, Open, Save, SaveAs, Export, Undo, Redo, ZoomIn, ZoomOut, ZoomFit,
        Zoom100, RotateCw, RotateCcw, ResetRotation, ToggleRulers, ToggleTheme, ShowHome,
        ShowEditor, DeleteNode, DuplicateNode, GroupNodes, Ungroup, MoveNodeUp, MoveNodeDown,
        ToggleNodeVisible, Ask, ToolHand, ToolMove, ToolPen, ToolType, ToolMarquee, ToolLasso,
        ToolWand, ToolBrush, ToolEraser, ToolBucket, ToolGradient, ToolHeal, ToolClone,
        ToolCrop, ToolShape, ToolEyedropper, ToolZoom, ImageSizeDialog, CanvasSizeDialog, NextTab, PrevTab, CloseTab, SwapColors, DefaultColors, BrushSmaller, BrushLarger, CommitTool,
        SelectAll, Deselect, InvertSelection, FillSelection, ContentAwareFill, ShowSettings,
        CopyPixels, CutPixels, PastePixels, ClearPixels, CanvasDelete, FreeTransform,
        ToolEllipseMarquee, ToolPolygonLasso, ToolMagneticLasso, ToolQuickSelect, ToolSmudge, ToolLiquify, ToolEllipse, ToolMask, ToolGrade,
        NudgeLeft, NudgeRight, NudgeUp, NudgeDown,
        NudgeLeftLarge, NudgeRightLarge, NudgeUpLarge, NudgeDownLarge,
        Suggestion1, Suggestion2, Suggestion3, Suggestion4, Quit,
    )
}

/// The binding contexts a keymap file may name.
pub const CONTEXTS: [(&str, &str); 3] = [
    ("workspace", "Workspace"),
    ("canvas", "Canvas"),
    ("panel", "NodePanel"),
];

/// Default shortcuts: (context key, action, keystrokes).
pub const DEFAULTS: &[(&str, &str, &str)] = &[
    ("workspace", "NewDocument", "ctrl-n"),
    ("workspace", "Open", "ctrl-o"),
    ("workspace", "Save", "ctrl-s"),
    ("workspace", "SaveAs", "ctrl-shift-s"),
    ("workspace", "Export", "ctrl-shift-e"),
    ("workspace", "Undo", "ctrl-z"),
    ("workspace", "Redo", "ctrl-shift-z"),
    ("workspace", "Redo", "ctrl-y"),
    ("workspace", "ZoomIn", "ctrl-="),
    ("workspace", "ZoomIn", "ctrl-+"),
    ("workspace", "ZoomIn", "ctrl-shift-="),
    ("workspace", "ZoomOut", "ctrl--"),
    ("workspace", "ZoomFit", "ctrl-0"),
    ("workspace", "Zoom100", "ctrl-1"),
    ("workspace", "ToggleRulers", "ctrl-r"),
    ("workspace", "DuplicateNode", "ctrl-j"),
    ("workspace", "GroupNodes", "ctrl-g"),
    ("workspace", "Ungroup", "ctrl-shift-g"),
    ("workspace", "MoveNodeUp", "ctrl-]"),
    ("workspace", "MoveNodeDown", "ctrl-["),
    ("workspace", "ToggleNodeVisible", "ctrl-,"),
    ("workspace", "Quit", "ctrl-q"),
    ("workspace", "Ask", "ctrl-k"),
    ("workspace", "Suggestion1", "alt-1"),
    ("workspace", "Suggestion2", "alt-2"),
    ("workspace", "Suggestion3", "alt-3"),
    ("workspace", "Suggestion4", "alt-4"),
    ("workspace", "SelectAll", "ctrl-a"),
    ("workspace", "Deselect", "ctrl-d"),
    ("workspace", "InvertSelection", "ctrl-shift-i"),
    ("canvas", "ToolHand", "h"),
    ("canvas", "ToolPen", "p"),
    ("canvas", "ToolType", "t"),
    ("canvas", "ToolMove", "v"),
    ("canvas", "ToolMarquee", "m"),
    ("canvas", "ToolLasso", "l"),
    ("canvas", "ToolWand", "w"),
    ("canvas", "ToolBrush", "b"),
    ("canvas", "ToolEraser", "e"),
    ("canvas", "ToolBucket", "g"),
    ("canvas", "ToolGradient", "shift-g"),
    ("canvas", "ToolHeal", "j"),
    ("canvas", "ToolClone", "s"),
    ("canvas", "ToolCrop", "c"),
    ("canvas", "ToolShape", "u"),
    ("canvas", "ToolEyedropper", "i"),
    ("canvas", "ToolZoom", "z"),
    ("canvas", "ToolEllipseMarquee", "shift-m"),
    ("canvas", "ToolPolygonLasso", "shift-l"),
    ("canvas", "ToolMagneticLasso", "alt-l"),
    ("canvas", "ToolQuickSelect", "shift-w"),
    ("canvas", "ToolSmudge", "shift-b"),
    ("canvas", "ToolLiquify", "shift-j"),
    ("canvas", "ToolEllipse", "shift-u"),
    ("canvas", "ToolMask", "q"),
    ("canvas", "ToolGrade", "shift-q"),
    ("workspace", "ImageSizeDialog", "ctrl-alt-i"),
    ("workspace", "CanvasSizeDialog", "ctrl-alt-c"),
    ("workspace", "NextTab", "ctrl-tab"),
    ("workspace", "PrevTab", "ctrl-shift-tab"),
    ("workspace", "CloseTab", "ctrl-w"),
    ("canvas", "SwapColors", "x"),
    ("canvas", "DefaultColors", "d"),
    ("canvas", "BrushSmaller", "["),
    ("canvas", "BrushLarger", "]"),
    ("canvas", "CommitTool", "enter"),
    ("canvas", "FillSelection", "alt-backspace"),
    ("canvas", "ContentAwareFill", "shift-backspace"),
    ("canvas", "CopyPixels", "ctrl-c"),
    ("canvas", "CutPixels", "ctrl-x"),
    ("canvas", "PastePixels", "ctrl-v"),
    ("canvas", "FreeTransform", "ctrl-t"),
    ("canvas", "NudgeLeft", "left"),
    ("canvas", "NudgeRight", "right"),
    ("canvas", "NudgeUp", "up"),
    ("canvas", "NudgeDown", "down"),
    ("canvas", "NudgeLeftLarge", "shift-left"),
    ("canvas", "NudgeRightLarge", "shift-right"),
    ("canvas", "NudgeUpLarge", "shift-up"),
    ("canvas", "NudgeDownLarge", "shift-down"),
    ("canvas", "RotateCw", "r"),
    ("canvas", "RotateCcw", "shift-r"),
    ("canvas", "ResetRotation", "escape"),
    ("canvas", "CanvasDelete", "delete"),
    ("canvas", "CanvasDelete", "backspace"),
    ("panel", "DeleteNode", "delete"),
    ("panel", "DeleteNode", "backspace"),
];

fn platform_defaults() -> Vec<(String, String, String)> {
    let mut out: Vec<_> = DEFAULTS
        .iter()
        .map(|(c, a, k)| (c.to_string(), a.to_string(), k.to_string()))
        .collect();
    if cfg!(target_os = "macos") {
        out.extend(DEFAULTS.iter().filter_map(|(c, a, k)| {
            k.strip_prefix("ctrl-")
                .map(|keys| (c.to_string(), a.to_string(), format!("cmd-{keys}")))
        }));
    }
    out
}

/// Where the person's overrides live.
pub fn keymap_path() -> std::path::PathBuf {
    emulsion_io::recent::data_dir().join("keymap.toml")
}

/// A starter file with every default, commented, for editing.
pub fn keymap_template() -> String {
    let mut out = String::from(
        r#"# Emulsion shortcuts. Uncomment a line and change its keys; a later
# binding for the same keys wins. Several keys: ["ctrl-z", "f1"].
# Contexts: [workspace] modifier shortcuts, [canvas] bare keys while
# the canvas has focus, [panel] the scene graph.

"#,
    );
    for (ctx, _) in CONTEXTS {
        out.push_str(&format!("[{ctx}]\n"));
        for (c, action, keys) in platform_defaults() {
            if c == ctx {
                out.push_str(&format!("# {action} = \"{keys}\"\n"));
            }
        }
        out.push('\n');
    }
    out
}

/// Overrides from the keymap file: (context key, action, keystrokes).
pub fn user_bindings() -> Vec<(String, String, String)> {
    let Ok(text) = std::fs::read_to_string(keymap_path()) else {
        return Vec::new();
    };
    let Ok(v) = toml::from_str::<toml::Table>(&text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (ctx, _) in CONTEXTS {
        let Some(table) = v.get(ctx).and_then(|t| t.as_table()) else {
            continue;
        };
        for (action, keys) in table {
            match keys {
                toml::Value::String(k) => out.push((ctx.to_string(), action.clone(), k.clone())),
                toml::Value::Array(a) => {
                    for k in a.iter().filter_map(|k| k.as_str()) {
                        out.push((ctx.to_string(), action.clone(), k.to_string()));
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// Defaults with the person's overrides applied (an override replaces
/// every default of the same action in that context).
pub fn effective() -> Vec<(String, String, String)> {
    let user = user_bindings();
    let mut out: Vec<(String, String, String)> = platform_defaults()
        .into_iter()
        .filter(|(c, a, _)| !user.iter().any(|(uc, ua, _)| uc == c && ua == a))
        .collect();
    out.extend(user);
    out
}

fn context_name(key: &str) -> Option<&'static str> {
    CONTEXTS.iter().find(|(k, _)| *k == key).map(|(_, n)| *n)
}

pub fn bind(cx: &mut App) {
    let mut bindings = Vec::new();
    for (ctx, action, keys) in effective() {
        if let Some(b) = binding(&action, &keys, context_name(&ctx)) {
            bindings.push(b);
        }
    }
    cx.bind_keys(bindings);
}

#[cfg(test)]
mod tests {
    use super::{DEFAULTS, binding, context_name, keymap_template};

    #[test]
    fn every_default_names_a_real_action_and_the_template_parses() {
        for (ctx, action, keys) in DEFAULTS {
            assert!(
                binding(action, keys, context_name(ctx)).is_some(),
                "{action} is not an action"
            );
        }
        let t = keymap_template();
        if let Err(e) = toml::from_str::<toml::Table>(&t) {
            panic!("{e}");
        }
        assert!(t.contains("# Undo = \"ctrl-z\""));
    }

    #[test]
    fn clipboard_bindings_are_canvas_scoped_and_mac_aliases_keep_control() {
        let defaults = super::platform_defaults();
        for (action, key) in [
            ("CopyPixels", "c"),
            ("CutPixels", "x"),
            ("PastePixels", "v"),
            ("FreeTransform", "t"),
        ] {
            assert!(defaults.contains(&("canvas".into(), action.into(), format!("ctrl-{key}"))));
            if cfg!(target_os = "macos") {
                assert!(defaults.contains(&("canvas".into(), action.into(), format!("cmd-{key}"))));
            }
            assert!(
                defaults
                    .iter()
                    .filter(|(_, a, _)| a == action)
                    .all(|(c, _, _)| c == "canvas")
            );
        }
        assert!(defaults.contains(&("panel".into(), "DeleteNode".into(), "backspace".into())));
        assert!(defaults.contains(&("canvas".into(), "CanvasDelete".into(), "backspace".into())));
    }

    #[test]
    fn platform_shortcuts_have_no_context_collisions() {
        let mut seen = std::collections::HashMap::new();
        for (context, action, key) in super::platform_defaults() {
            if let Some(previous) = seen.insert((context.clone(), key.clone()), action.clone()) {
                assert_eq!(previous, action, "{context}: {key} triggers two actions");
            }
        }
    }

    #[test]
    fn nudge_bindings_are_canvas_scoped_and_available_to_keymaps() {
        let defaults = super::platform_defaults();
        for (action, key) in [
            ("NudgeLeft", "left"),
            ("NudgeRight", "right"),
            ("NudgeUp", "up"),
            ("NudgeDown", "down"),
            ("NudgeLeftLarge", "shift-left"),
            ("NudgeRightLarge", "shift-right"),
            ("NudgeUpLarge", "shift-up"),
            ("NudgeDownLarge", "shift-down"),
        ] {
            let matches: Vec<_> = defaults.iter().filter(|(_, a, _)| a == action).collect();
            assert_eq!(matches.len(), 1, "{action} must have one scoped default");
            assert_eq!(matches[0], &("canvas".into(), action.into(), key.into()));
            assert!(super::binding(action, key, Some("Canvas")).is_some());
        }
    }
}
