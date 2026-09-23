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
        SynchronizeRaw,
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
        ToggleDrawMode,
        ToggleQuickMask,
        ToggleTheme,
        ShowHome,
        ShowEditor,
        ShowBatch,
        ShowAbout,
        DeleteNode,
        NewLayer,
        DuplicateNode,
        GroupNodes,
        Ungroup,
        RenameLayer,
        MergeLayers,
        MergeVisible,
        FlattenImage,
        LinkLayers,
        UnlinkLayers,
        CopyLayerStyle,
        PasteLayerStyle,
        ApplyLayerMask,
        MoveNodeUp,
        MoveNodeDown,
        NextBlendMode,
        PreviousBlendMode,
        ToggleNodeVisible,
        Ask,
        ToolHand,
        ToolRotateView,
        RepeatFilter,
        ToolMove,
        ToolPen,
        ToolType,
        ToolVerticalType,
        ConvertToSmartObject,
        ConvertSmartToLayers,
        RasterizeLayer,
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
        AutoTone,
        AutoContrast,
        AutoColor,
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
        FillBackground,
        ContentAwareFill,
        CopyPixels,
        CutPixels,
        PastePixels,
        ClearPixels,
        CanvasDelete,
        FreeTransform,
        TransformScale,
        TransformRotate,
        TransformDistort,
        TransformWarp,
        RotateLayer180,
        RotateLayer90Cw,
        RotateLayer90Ccw,
        FlipLayerHorizontal,
        FlipLayerVertical,
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
        Reselect,
        ToggleSnap,
        ToggleClippingMask,
        BringToFront,
        SendToBack,
        SelectLayerAbove,
        SelectLayerBelow,
        AddLayerAboveToSelection,
        AddLayerBelowToSelection,
        SelectTopLayer,
        SelectBottomLayer,
        SelectAllLayers,
        AdjustLevels,
        AdjustCurves,
        AdjustHueSaturation,
        AdjustColorBalance,
        AdjustBlackAndWhite,
        AdjustInvert,
        AdjustDesaturate,
        FilterLensCorrection,
        BrushSofter,
        BrushHarder,
        Opacity10,
        Opacity20,
        Opacity30,
        Opacity40,
        Opacity50,
        Opacity60,
        Opacity70,
        Opacity80,
        Opacity90,
        Opacity100,
        ShowLayersPanel,
        FindLayers,
        ShowInfoPanel,
        ShowBrushSettings,
        TogglePanels,
        ToggleScreenMode,
        ToolRemove,
        ToolFreeformPen,
        BlendNormal,
        BlendDissolve,
        BlendDarken,
        BlendMultiply,
        BlendColorBurn,
        BlendLinearBurn,
        BlendLighten,
        BlendScreen,
        BlendColorDodge,
        BlendLinearDodge,
        BlendOverlay,
        BlendSoftLight,
        BlendHardLight,
        BlendVividLight,
        BlendLinearLight,
        BlendPinLight,
        BlendHardMix,
        BlendDifference,
        BlendExclusion,
        BlendHue,
        BlendSaturation,
        BlendColor,
        BlendLuminosity,
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
        NewDocument, Open, Save, SaveAs, Export, SynchronizeRaw, Undo, Redo, ZoomIn, ZoomOut, ZoomFit,
        Zoom100, RotateCw, RotateCcw, ResetRotation, ToggleRulers, ToggleDrawMode, ToggleQuickMask, ToggleTheme, ShowHome,
        ShowEditor, ShowBatch, ShowAbout, DeleteNode, NewLayer, DuplicateNode, GroupNodes, Ungroup, RenameLayer, MergeLayers, MergeVisible, FlattenImage, LinkLayers, UnlinkLayers, CopyLayerStyle, PasteLayerStyle, ApplyLayerMask, MoveNodeUp, MoveNodeDown,
        ToggleNodeVisible, NextBlendMode, PreviousBlendMode, Ask, ToolHand, ToolRotateView, RepeatFilter, ToolMove, ToolPen, ToolType, ToolVerticalType, ConvertToSmartObject, ConvertSmartToLayers, RasterizeLayer, ToolMarquee, ToolLasso,
        ToolWand, ToolBrush, ToolEraser, ToolBucket, ToolGradient, ToolHeal, ToolClone,
        ToolCrop, ToolShape, ToolEyedropper, ToolZoom, AutoTone, AutoContrast, AutoColor, ImageSizeDialog, CanvasSizeDialog, NextTab, PrevTab, CloseTab, SwapColors, DefaultColors, BrushSmaller, BrushLarger, CommitTool,
        SelectAll, Deselect, InvertSelection, FillSelection, FillBackground, ContentAwareFill, ShowSettings,
        CopyPixels, CutPixels, PastePixels, ClearPixels, CanvasDelete, FreeTransform,
        TransformScale, TransformRotate, TransformDistort, TransformWarp,
        RotateLayer180, RotateLayer90Cw, RotateLayer90Ccw, FlipLayerHorizontal, FlipLayerVertical,
        ToolEllipseMarquee, ToolPolygonLasso, ToolMagneticLasso, ToolQuickSelect, ToolSmudge, ToolLiquify, ToolEllipse, ToolMask, ToolGrade,
        NudgeLeft, NudgeRight, NudgeUp, NudgeDown,
        NudgeLeftLarge, NudgeRightLarge, NudgeUpLarge, NudgeDownLarge,
        Suggestion1, Suggestion2, Suggestion3, Suggestion4, Quit,
        Reselect, ToggleSnap, ToggleClippingMask, BringToFront, SendToBack,
        SelectLayerAbove, SelectLayerBelow, AddLayerAboveToSelection, AddLayerBelowToSelection,
        SelectTopLayer, SelectBottomLayer, SelectAllLayers,
        AdjustLevels, AdjustCurves, AdjustHueSaturation, AdjustColorBalance, AdjustBlackAndWhite,
        AdjustInvert, AdjustDesaturate, FilterLensCorrection, BrushSofter, BrushHarder,
        Opacity10, Opacity20, Opacity30, Opacity40, Opacity50,
        Opacity60, Opacity70, Opacity80, Opacity90, Opacity100,
        ShowLayersPanel, FindLayers, ShowInfoPanel, ShowBrushSettings, TogglePanels, ToggleScreenMode,
        ToolRemove, ToolFreeformPen,
        BlendNormal, BlendDissolve, BlendDarken, BlendMultiply, BlendColorBurn, BlendLinearBurn,
        BlendLighten, BlendScreen, BlendColorDodge, BlendLinearDodge, BlendOverlay, BlendSoftLight,
        BlendHardLight, BlendVividLight, BlendLinearLight, BlendPinLight, BlendHardMix,
        BlendDifference, BlendExclusion, BlendHue, BlendSaturation, BlendColor, BlendLuminosity,
    )
}

/// The binding contexts a keymap file may name.
pub const CONTEXTS: [(&str, &str); 3] = [
    ("workspace", "Workspace"),
    ("canvas", "Canvas"),
    ("panel", "NodePanel"),
];

/// Default shortcuts: (context key, action, keystrokes).
///
/// These follow Photoshop's default Windows shortcuts wherever Emulsion has
/// the feature, so Photoshop users keep their muscle memory; on macOS every
/// `ctrl-` binding also gets a `cmd-` twin.
pub const DEFAULTS: &[(&str, &str, &str)] = &[
    // ── File ──
    ("workspace", "NewDocument", "ctrl-n"),
    ("workspace", "Open", "ctrl-o"),
    ("workspace", "CloseTab", "ctrl-w"),
    ("workspace", "Save", "ctrl-s"),
    ("workspace", "SaveAs", "ctrl-shift-s"),
    // Photoshop: Export As, and Save for Web (Legacy).
    ("workspace", "Export", "ctrl-alt-shift-w"),
    ("workspace", "Export", "ctrl-alt-shift-s"),
    ("workspace", "Quit", "ctrl-q"),
    // ── Edit ──
    ("workspace", "Undo", "ctrl-z"),
    // Photoshop's Step Backward (legacy undo).
    ("workspace", "Undo", "ctrl-alt-z"),
    ("workspace", "Redo", "ctrl-shift-z"),
    ("workspace", "Redo", "ctrl-y"),
    // Photoshop: Preferences, and Keyboard Shortcuts.
    ("workspace", "ShowSettings", "ctrl-k"),
    ("workspace", "ShowSettings", "ctrl-alt-shift-k"),
    ("canvas", "CopyPixels", "ctrl-c"),
    ("canvas", "CutPixels", "ctrl-x"),
    ("canvas", "PastePixels", "ctrl-v"),
    // Paste in Place: pixels copied here already paste where they came from.
    ("canvas", "PastePixels", "ctrl-shift-v"),
    ("canvas", "FreeTransform", "ctrl-t"),
    ("canvas", "FillSelection", "alt-backspace"),
    ("canvas", "FillBackground", "ctrl-backspace"),
    ("canvas", "ContentAwareFill", "shift-backspace"),
    ("canvas", "ContentAwareFill", "shift-f5"),
    ("canvas", "CanvasDelete", "delete"),
    ("canvas", "CanvasDelete", "backspace"),
    ("canvas", "CommitTool", "enter"),
    ("canvas", "ResetRotation", "escape"),
    ("canvas", "NudgeLeft", "left"),
    ("canvas", "NudgeRight", "right"),
    ("canvas", "NudgeUp", "up"),
    ("canvas", "NudgeDown", "down"),
    ("canvas", "NudgeLeftLarge", "shift-left"),
    ("canvas", "NudgeRightLarge", "shift-right"),
    ("canvas", "NudgeUpLarge", "shift-up"),
    ("canvas", "NudgeDownLarge", "shift-down"),
    // ── Image ──
    ("workspace", "AdjustLevels", "ctrl-l"),
    ("workspace", "AdjustCurves", "ctrl-m"),
    ("workspace", "AdjustHueSaturation", "ctrl-u"),
    ("workspace", "AdjustColorBalance", "ctrl-b"),
    ("workspace", "AdjustBlackAndWhite", "ctrl-alt-shift-b"),
    ("workspace", "AdjustInvert", "ctrl-i"),
    ("workspace", "AdjustDesaturate", "ctrl-shift-u"),
    ("workspace", "AutoTone", "ctrl-shift-l"),
    ("workspace", "AutoContrast", "ctrl-alt-shift-l"),
    ("workspace", "AutoColor", "ctrl-shift-b"),
    ("workspace", "ImageSizeDialog", "ctrl-alt-i"),
    ("workspace", "CanvasSizeDialog", "ctrl-alt-c"),
    // ── Layer ──
    ("workspace", "NewLayer", "ctrl-shift-n"),
    ("workspace", "NewLayer", "ctrl-alt-shift-n"),
    ("workspace", "DuplicateNode", "ctrl-j"),
    ("workspace", "GroupNodes", "ctrl-g"),
    ("workspace", "Ungroup", "ctrl-shift-g"),
    ("workspace", "ToggleClippingMask", "ctrl-alt-g"),
    ("workspace", "MoveNodeUp", "ctrl-]"),
    ("workspace", "BringToFront", "ctrl-shift-]"),
    ("workspace", "MoveNodeDown", "ctrl-["),
    ("workspace", "SendToBack", "ctrl-shift-["),
    ("workspace", "MergeLayers", "ctrl-e"),
    ("workspace", "MergeVisible", "ctrl-shift-e"),
    ("workspace", "SelectLayerAbove", "alt-]"),
    ("workspace", "SelectLayerBelow", "alt-["),
    ("workspace", "AddLayerAboveToSelection", "alt-shift-]"),
    ("workspace", "AddLayerBelowToSelection", "alt-shift-["),
    ("workspace", "SelectTopLayer", "alt-."),
    ("workspace", "SelectBottomLayer", "alt-,"),
    ("workspace", "ToggleNodeVisible", "ctrl-,"),
    ("panel", "RenameLayer", "f2"),
    ("panel", "DeleteNode", "delete"),
    ("panel", "DeleteNode", "backspace"),
    ("panel", "CopyPixels", "ctrl-c"),
    ("panel", "CutPixels", "ctrl-x"),
    ("panel", "PastePixels", "ctrl-v"),
    ("panel", "PastePixels", "ctrl-shift-v"),
    ("panel", "SelectAll", "ctrl-a"),
    ("panel", "FreeTransform", "ctrl-t"),
    // Photoshop's Shift+Plus / Shift+Minus blend-mode cycling.
    ("canvas", "NextBlendMode", "shift-="),
    ("canvas", "PreviousBlendMode", "shift--"),
    ("panel", "NextBlendMode", "shift-="),
    ("panel", "PreviousBlendMode", "shift--"),
    // Photoshop's Shift+Alt+letter layer blend modes.
    ("canvas", "BlendNormal", "alt-shift-n"),
    ("canvas", "BlendDissolve", "alt-shift-i"),
    ("canvas", "BlendDarken", "alt-shift-k"),
    ("canvas", "BlendMultiply", "alt-shift-m"),
    ("canvas", "BlendColorBurn", "alt-shift-b"),
    ("canvas", "BlendLinearBurn", "alt-shift-a"),
    ("canvas", "BlendLighten", "alt-shift-g"),
    ("canvas", "BlendScreen", "alt-shift-s"),
    ("canvas", "BlendColorDodge", "alt-shift-d"),
    ("canvas", "BlendLinearDodge", "alt-shift-w"),
    ("canvas", "BlendOverlay", "alt-shift-o"),
    ("canvas", "BlendSoftLight", "alt-shift-f"),
    ("canvas", "BlendHardLight", "alt-shift-h"),
    ("canvas", "BlendVividLight", "alt-shift-v"),
    ("canvas", "BlendLinearLight", "alt-shift-j"),
    ("canvas", "BlendPinLight", "alt-shift-z"),
    ("canvas", "BlendHardMix", "alt-shift-l"),
    ("canvas", "BlendDifference", "alt-shift-e"),
    ("canvas", "BlendExclusion", "alt-shift-x"),
    ("canvas", "BlendHue", "alt-shift-u"),
    ("canvas", "BlendSaturation", "alt-shift-t"),
    ("canvas", "BlendColor", "alt-shift-c"),
    ("canvas", "BlendLuminosity", "alt-shift-y"),
    ("panel", "BlendNormal", "alt-shift-n"),
    ("panel", "BlendDissolve", "alt-shift-i"),
    ("panel", "BlendDarken", "alt-shift-k"),
    ("panel", "BlendMultiply", "alt-shift-m"),
    ("panel", "BlendColorBurn", "alt-shift-b"),
    ("panel", "BlendLinearBurn", "alt-shift-a"),
    ("panel", "BlendLighten", "alt-shift-g"),
    ("panel", "BlendScreen", "alt-shift-s"),
    ("panel", "BlendColorDodge", "alt-shift-d"),
    ("panel", "BlendLinearDodge", "alt-shift-w"),
    ("panel", "BlendOverlay", "alt-shift-o"),
    ("panel", "BlendSoftLight", "alt-shift-f"),
    ("panel", "BlendHardLight", "alt-shift-h"),
    ("panel", "BlendVividLight", "alt-shift-v"),
    ("panel", "BlendLinearLight", "alt-shift-j"),
    ("panel", "BlendPinLight", "alt-shift-z"),
    ("panel", "BlendHardMix", "alt-shift-l"),
    ("panel", "BlendDifference", "alt-shift-e"),
    ("panel", "BlendExclusion", "alt-shift-x"),
    ("panel", "BlendHue", "alt-shift-u"),
    ("panel", "BlendSaturation", "alt-shift-t"),
    ("panel", "BlendColor", "alt-shift-c"),
    ("panel", "BlendLuminosity", "alt-shift-y"),
    // Photoshop's number keys: tool opacity with a painting tool, layer
    // opacity otherwise. 1 is 10 %, 0 is 100 %.
    ("canvas", "Opacity10", "1"),
    ("canvas", "Opacity20", "2"),
    ("canvas", "Opacity30", "3"),
    ("canvas", "Opacity40", "4"),
    ("canvas", "Opacity50", "5"),
    ("canvas", "Opacity60", "6"),
    ("canvas", "Opacity70", "7"),
    ("canvas", "Opacity80", "8"),
    ("canvas", "Opacity90", "9"),
    ("canvas", "Opacity100", "0"),
    // ── Select ──
    ("canvas", "SelectAll", "ctrl-a"),
    ("workspace", "Deselect", "ctrl-d"),
    ("workspace", "Reselect", "ctrl-shift-d"),
    ("workspace", "InvertSelection", "ctrl-shift-i"),
    ("workspace", "SelectAllLayers", "ctrl-alt-a"),
    // ── Filter ──
    // Photoshop 2021+: Last Filter moved to Ctrl+Alt+F; Ctrl+F searches.
    ("canvas", "RepeatFilter", "ctrl-alt-f"),
    ("canvas", "ToolLiquify", "ctrl-shift-x"),
    ("workspace", "FilterLensCorrection", "ctrl-shift-r"),
    // ── View ──
    ("workspace", "ZoomIn", "ctrl-="),
    ("workspace", "ZoomIn", "ctrl-+"),
    ("workspace", "ZoomIn", "ctrl-shift-="),
    ("workspace", "ZoomOut", "ctrl--"),
    ("workspace", "ZoomFit", "ctrl-0"),
    ("workspace", "Zoom100", "ctrl-1"),
    ("workspace", "ToggleRulers", "ctrl-r"),
    ("workspace", "ToggleSnap", "ctrl-shift-;"),
    ("canvas", "ToggleScreenMode", "f"),
    ("canvas", "TogglePanels", "tab"),
    ("canvas", "RotateCw", "alt-r"),
    ("canvas", "RotateCcw", "shift-r"),
    // ── Window ──
    ("workspace", "ToggleDrawMode", "ctrl-alt-shift-d"),
    ("workspace", "ShowBrushSettings", "f5"),
    ("workspace", "ShowLayersPanel", "f7"),
    ("workspace", "ShowInfoPanel", "f8"),
    ("workspace", "NextTab", "ctrl-tab"),
    ("workspace", "PrevTab", "ctrl-shift-tab"),
    // ── Find ── Ctrl+F searches, as in Photoshop 2021+ and most apps.
    ("workspace", "FindLayers", "ctrl-f"),
    // ── Assistant ── F1 is the Help key; Omarchy's Hyprland binds neither it
    // nor Ctrl+F.
    ("workspace", "Ask", "f1"),
    ("workspace", "Ask", "alt-f1"),
    ("workspace", "Suggestion1", "alt-1"),
    ("workspace", "Suggestion2", "alt-2"),
    ("workspace", "Suggestion3", "alt-3"),
    ("workspace", "Suggestion4", "alt-4"),
    // ── Tools ── Shift+letter steps through the letter's group.
    ("canvas", "ToolMove", "v"),
    ("canvas", "ToolMarquee", "m"),
    ("canvas", "ToolEllipseMarquee", "shift-m"),
    ("canvas", "ToolLasso", "l"),
    ("canvas", "ToolPolygonLasso", "shift-l"),
    ("canvas", "ToolMagneticLasso", "alt-l"),
    ("canvas", "ToolWand", "w"),
    ("canvas", "ToolQuickSelect", "shift-w"),
    ("canvas", "ToolCrop", "c"),
    ("canvas", "ToolEyedropper", "i"),
    ("canvas", "ToolHeal", "j"),
    ("canvas", "ToolRemove", "shift-j"),
    ("canvas", "ToolBrush", "b"),
    ("canvas", "ToolSmudge", "shift-b"),
    ("canvas", "ToolClone", "s"),
    ("canvas", "ToolEraser", "e"),
    ("canvas", "ToolGradient", "g"),
    ("canvas", "ToolBucket", "shift-g"),
    ("canvas", "ToolPen", "p"),
    ("canvas", "ToolFreeformPen", "shift-p"),
    ("canvas", "ToolType", "t"),
    ("canvas", "ToolVerticalType", "shift-t"),
    ("canvas", "ToolShape", "u"),
    ("canvas", "ToolEllipse", "shift-u"),
    ("canvas", "ToolHand", "h"),
    ("canvas", "ToolRotateView", "r"),
    ("canvas", "ToolZoom", "z"),
    // Photoshop: Q toggles Quick Mask. The Mask tool stays bindable.
    ("canvas", "ToggleQuickMask", "q"),
    ("canvas", "ToolGrade", "shift-q"),
    ("canvas", "DefaultColors", "d"),
    ("canvas", "SwapColors", "x"),
    ("canvas", "BrushSmaller", "["),
    ("canvas", "BrushLarger", "]"),
    ("canvas", "BrushSofter", "shift-["),
    ("canvas", "BrushHarder", "shift-]"),
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
    // Let embedded sliders receive arrows instead of navigating their menu.
    for key in ["left", "right", "up", "down"] {
        bindings.push(KeyBinding::new(
            key,
            gpui_kit::NoAction,
            Some("PopupMenu > Slider"),
        ));
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
    fn clipboard_bindings_are_editor_scoped_and_mac_aliases_keep_control() {
        let defaults = super::platform_defaults();
        for (action, key) in [
            ("SelectAll", "a"),
            ("CopyPixels", "c"),
            ("CutPixels", "x"),
            ("PastePixels", "v"),
            ("FreeTransform", "t"),
        ] {
            assert!(defaults.contains(&("canvas".into(), action.into(), format!("ctrl-{key}"))));
            if cfg!(target_os = "macos") {
                assert!(defaults.contains(&("canvas".into(), action.into(), format!("cmd-{key}"))));
            }
            {
                assert!(defaults.contains(&("panel".into(), action.into(), format!("ctrl-{key}"))));
                if cfg!(target_os = "macos") {
                    assert!(defaults.contains(&(
                        "panel".into(),
                        action.into(),
                        format!("cmd-{key}")
                    )));
                }
            }
            assert!(
                defaults
                    .iter()
                    .filter(|(_, a, _)| a == action)
                    .all(|(c, _, _)| c == "canvas" || c == "panel")
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
    fn photoshop_default_shortcuts_reach_the_matching_actions() {
        let defaults = super::platform_defaults();
        for (ctx, action, keys) in [
            // File
            ("workspace", "NewDocument", "ctrl-n"),
            ("workspace", "Open", "ctrl-o"),
            ("workspace", "CloseTab", "ctrl-w"),
            ("workspace", "Save", "ctrl-s"),
            ("workspace", "SaveAs", "ctrl-shift-s"),
            ("workspace", "Export", "ctrl-alt-shift-w"),
            ("workspace", "Quit", "ctrl-q"),
            // Edit
            ("workspace", "Undo", "ctrl-z"),
            ("workspace", "Undo", "ctrl-alt-z"),
            ("workspace", "Redo", "ctrl-shift-z"),
            ("canvas", "CutPixels", "ctrl-x"),
            ("canvas", "CopyPixels", "ctrl-c"),
            ("canvas", "PastePixels", "ctrl-v"),
            ("canvas", "PastePixels", "ctrl-shift-v"),
            ("canvas", "FillSelection", "alt-backspace"),
            ("canvas", "FillBackground", "ctrl-backspace"),
            ("canvas", "ContentAwareFill", "shift-f5"),
            ("canvas", "FreeTransform", "ctrl-t"),
            ("workspace", "ShowSettings", "ctrl-k"),
            ("workspace", "ShowSettings", "ctrl-alt-shift-k"),
            // Image
            ("workspace", "AdjustLevels", "ctrl-l"),
            ("workspace", "AdjustCurves", "ctrl-m"),
            ("workspace", "AdjustHueSaturation", "ctrl-u"),
            ("workspace", "AdjustColorBalance", "ctrl-b"),
            ("workspace", "AdjustBlackAndWhite", "ctrl-alt-shift-b"),
            ("workspace", "AdjustInvert", "ctrl-i"),
            ("workspace", "AdjustDesaturate", "ctrl-shift-u"),
            ("workspace", "AutoTone", "ctrl-shift-l"),
            ("workspace", "AutoContrast", "ctrl-alt-shift-l"),
            ("workspace", "AutoColor", "ctrl-shift-b"),
            ("workspace", "ImageSizeDialog", "ctrl-alt-i"),
            ("workspace", "CanvasSizeDialog", "ctrl-alt-c"),
            // Layer
            ("workspace", "NewLayer", "ctrl-shift-n"),
            ("workspace", "DuplicateNode", "ctrl-j"),
            ("workspace", "GroupNodes", "ctrl-g"),
            ("workspace", "Ungroup", "ctrl-shift-g"),
            ("workspace", "ToggleClippingMask", "ctrl-alt-g"),
            ("workspace", "MoveNodeUp", "ctrl-]"),
            ("workspace", "BringToFront", "ctrl-shift-]"),
            ("workspace", "MoveNodeDown", "ctrl-["),
            ("workspace", "SendToBack", "ctrl-shift-["),
            ("workspace", "MergeLayers", "ctrl-e"),
            ("workspace", "MergeVisible", "ctrl-shift-e"),
            ("workspace", "SelectLayerAbove", "alt-]"),
            ("workspace", "SelectLayerBelow", "alt-["),
            ("canvas", "BlendMultiply", "alt-shift-m"),
            ("canvas", "BlendScreen", "alt-shift-s"),
            ("canvas", "BlendNormal", "alt-shift-n"),
            ("canvas", "Opacity50", "5"),
            ("canvas", "Opacity100", "0"),
            // Select
            ("canvas", "SelectAll", "ctrl-a"),
            ("workspace", "Deselect", "ctrl-d"),
            ("workspace", "Reselect", "ctrl-shift-d"),
            ("workspace", "InvertSelection", "ctrl-shift-i"),
            ("workspace", "SelectAllLayers", "ctrl-alt-a"),
            // Filter
            ("canvas", "RepeatFilter", "ctrl-alt-f"),
            ("canvas", "ToolLiquify", "ctrl-shift-x"),
            ("workspace", "FilterLensCorrection", "ctrl-shift-r"),
            // View and Window
            ("workspace", "ZoomIn", "ctrl-="),
            ("workspace", "ZoomOut", "ctrl--"),
            ("workspace", "ZoomFit", "ctrl-0"),
            ("workspace", "Zoom100", "ctrl-1"),
            ("workspace", "ToggleRulers", "ctrl-r"),
            ("workspace", "ToggleSnap", "ctrl-shift-;"),
            ("canvas", "ToggleScreenMode", "f"),
            ("canvas", "TogglePanels", "tab"),
            ("workspace", "ShowBrushSettings", "f5"),
            ("workspace", "ShowLayersPanel", "f7"),
            ("workspace", "ShowInfoPanel", "f8"),
            ("workspace", "FindLayers", "ctrl-f"),
            ("workspace", "Ask", "f1"),
            ("workspace", "Ask", "alt-f1"),
            // Tools
            ("canvas", "ToolMove", "v"),
            ("canvas", "ToolMarquee", "m"),
            ("canvas", "ToolLasso", "l"),
            ("canvas", "ToolWand", "w"),
            ("canvas", "ToolCrop", "c"),
            ("canvas", "ToolEyedropper", "i"),
            ("canvas", "ToolHeal", "j"),
            ("canvas", "ToolRemove", "shift-j"),
            ("canvas", "ToolBrush", "b"),
            ("canvas", "ToolClone", "s"),
            ("canvas", "ToolEraser", "e"),
            ("canvas", "ToolGradient", "g"),
            ("canvas", "ToolBucket", "shift-g"),
            ("canvas", "ToolPen", "p"),
            ("canvas", "ToolFreeformPen", "shift-p"),
            ("canvas", "ToolType", "t"),
            ("canvas", "ToolShape", "u"),
            ("canvas", "ToolHand", "h"),
            ("canvas", "ToolRotateView", "r"),
            ("canvas", "ToolZoom", "z"),
            ("canvas", "DefaultColors", "d"),
            ("canvas", "SwapColors", "x"),
            ("canvas", "ToggleQuickMask", "q"),
            ("canvas", "BrushSmaller", "["),
            ("canvas", "BrushLarger", "]"),
            ("canvas", "BrushSofter", "shift-["),
            ("canvas", "BrushHarder", "shift-]"),
        ] {
            assert!(
                defaults.contains(&(ctx.into(), action.into(), keys.into())),
                "{keys} should run {action} in {ctx}"
            );
            assert!(
                defaults
                    .iter()
                    .filter(|(_, _, k)| k == keys)
                    .all(|(_, a, _)| a == action),
                "{keys} must only run {action}"
            );
        }
        // A workspace shortcut must not mean something else on the canvas
        // or in the layers panel, which sit inside the workspace.
        for (_, action, keys) in defaults.iter().filter(|(c, _, _)| c == "workspace") {
            assert!(
                defaults
                    .iter()
                    .filter(|(c, _, k)| c != "workspace" && k == keys)
                    .all(|(_, a, _)| a == action),
                "{keys} is shadowed on the canvas or panel"
            );
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
