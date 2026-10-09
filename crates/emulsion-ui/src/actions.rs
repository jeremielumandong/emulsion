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
        ConfirmExport,
        Print,
        SynchronizeRaw,
        DevelopOriginal,
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
        PanelDelete,
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
        AddVectorMaskRevealAll,
        AddVectorMaskHideAll,
        DrawVectorMask,
        EditVectorMask,
        CloseVectorMaskPath,
        ToggleVectorMask,
        InvertVectorMask,
        LinkVectorMask,
        RemoveVectorMask,
        VectorMaskToSelection,
        RasterizeVectorMask,
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
        ToolRectangularMarquee,
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
        ToolVectorShape,
        ToolContourEditor,
        ToolPencilRetouch,
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
        DuplicateTransform,
        TransformAgain,
        TransformAgainWithCopy,
        TransformScale,
        TransformRotate,
        TransformDistort,
        TransformWarp,
        RotateLayer180,
        RotateLayer90Cw,
        RotateLayer90Ccw,
        FlipLayerHorizontal,
        FlipLayerVertical,
        DiagramAddLeft,
        DiagramAddRight,
        DiagramAddUp,
        DiagramAddDown,
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
        Flow10,
        Flow20,
        Flow30,
        Flow40,
        Flow50,
        Flow60,
        Flow70,
        Flow80,
        Flow90,
        Flow100,
        ShowLayersPanel,
        FindLayers,
        FindReplaceCaptions,
        CheckCaptionSpelling,
        PasteInPlace,
        ToggleStoryboardBoard,
        AddPanel,
        SmartAddPanel,
        DuplicatePanel,
        DeletePanel,
        PreviousPanel,
        NextPanel,
        TogglePanelLock,
        StartScene,
        RenumberPanels,
        CopyPanels,
        PastePanels,
        NewReviewLayer,
        PreviousChange,
        NextChange,
        ToggleChangeMarks,
        ShowSharedProject,
        CheckSharedChanges,
        ToggleLightTable,
        ToggleCameraView,
        ToggleShotGenerator,
        ToggleCameraTool,
        AddCameraKey,
        DeleteCameraKey,
        PreviousCameraKey,
        NextCameraKey,
        PlayPause,
        StopPlayback,
        PreviousFrame,
        NextFrame,
        FirstFrame,
        LastFrame,
        SetPlayIn,
        SetPlayOut,
        ClearPlayRange,
        ToggleLoop,
        ToggleTimeline,
        AddTimelineMarker,
        FlipViewHorizontal,
        FlipViewVertical,
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
    // Panel descendants can be editable controls. Never let inherited Photo
    // tool keys (including user remaps/chords) consume text or menu navigation.
    let ctx = if ctx == context_name("panel") && name == "DeleteNode" {
        // Photo inherits these keys through PanelDelete below. Other panel
        // workspaces retain their existing DeleteNode handlers and semantics.
        Some("NodePanel && !Photo")
    } else if ctx == context_name("photo_panel")
        && (matches!(name, "DeleteNode" | "PanelDelete") || is_tool_choice(name))
    {
        Some("NodePanel && Photo && !Input && !CanvasText && !PopupMenu && !Slider")
    } else {
        ctx
    };
    make_binding!(name, keys, ctx;
        NewDocument, Open, Save, SaveAs, Export, Print, SynchronizeRaw, DevelopOriginal, Undo, Redo, ZoomIn, ZoomOut, ZoomFit,
        Zoom100, RotateCw, RotateCcw, ResetRotation, ToggleRulers, ToggleDrawMode, ToggleQuickMask, ToggleTheme, ShowHome,
        ShowEditor, ShowBatch, ShowAbout, DeleteNode, PanelDelete, NewLayer, DuplicateNode, GroupNodes, Ungroup, RenameLayer, MergeLayers, MergeVisible, FlattenImage, LinkLayers, UnlinkLayers, CopyLayerStyle, PasteLayerStyle, ApplyLayerMask, MoveNodeUp, MoveNodeDown,
        AddVectorMaskRevealAll, AddVectorMaskHideAll, DrawVectorMask, EditVectorMask, CloseVectorMaskPath, ToggleVectorMask, InvertVectorMask, LinkVectorMask, RemoveVectorMask, VectorMaskToSelection, RasterizeVectorMask,
        ToggleNodeVisible, NextBlendMode, PreviousBlendMode, Ask, ToolHand, ToolRotateView, RepeatFilter, ToolMove, ToolPen, ToolType, ToolVerticalType, ConvertToSmartObject, ConvertSmartToLayers, RasterizeLayer, ToolMarquee, ToolLasso,
        ToolWand, ToolBrush, ToolEraser, ToolBucket, ToolGradient, ToolHeal, ToolClone,
        ToolCrop, ToolShape, ToolEyedropper, ToolZoom, AutoTone, AutoContrast, AutoColor, ImageSizeDialog, CanvasSizeDialog, NextTab, PrevTab, CloseTab, SwapColors, DefaultColors, BrushSmaller, BrushLarger, CommitTool,
        SelectAll, Deselect, InvertSelection, FillSelection, FillBackground, ContentAwareFill, ShowSettings,
        CopyPixels, CutPixels, PastePixels, ClearPixels, CanvasDelete, FreeTransform,
        DuplicateTransform, TransformAgain, TransformAgainWithCopy,
        TransformScale, TransformRotate, TransformDistort, TransformWarp,
        RotateLayer180, RotateLayer90Cw, RotateLayer90Ccw, FlipLayerHorizontal, FlipLayerVertical,
        ToolRectangularMarquee, ToolEllipseMarquee, ToolPolygonLasso, ToolMagneticLasso, ToolQuickSelect, ToolSmudge, ToolLiquify, ToolEllipse, ToolMask, ToolGrade,
        ToolVectorShape, ToolContourEditor, ToolPencilRetouch,
        DiagramAddLeft, DiagramAddRight, DiagramAddUp, DiagramAddDown,
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
        Flow10, Flow20, Flow30, Flow40, Flow50, Flow60, Flow70, Flow80, Flow90, Flow100,
        ShowLayersPanel, FindLayers, FindReplaceCaptions, CheckCaptionSpelling, ShowInfoPanel,
        PasteInPlace, ToggleStoryboardBoard, AddPanel, SmartAddPanel, DuplicatePanel, DeletePanel,
        PreviousPanel, NextPanel, TogglePanelLock, StartScene, RenumberPanels, CopyPanels, PastePanels, ToggleLightTable, ToggleCameraView, ToggleShotGenerator,
        NewReviewLayer, PreviousChange, NextChange, ToggleChangeMarks, ShowSharedProject, CheckSharedChanges,
        ToggleCameraTool, AddCameraKey, DeleteCameraKey, PreviousCameraKey, NextCameraKey,
        PlayPause, StopPlayback, PreviousFrame, NextFrame, FirstFrame, LastFrame, SetPlayIn, SetPlayOut, ClearPlayRange, ToggleLoop,
        ToggleTimeline, AddTimelineMarker,
        FlipViewHorizontal, FlipViewVertical, ShowBrushSettings, TogglePanels, ToggleScreenMode,
        ToolRemove, ToolFreeformPen,
        BlendNormal, BlendDissolve, BlendDarken, BlendMultiply, BlendColorBurn, BlendLinearBurn,
        BlendLighten, BlendScreen, BlendColorDodge, BlendLinearDodge, BlendOverlay, BlendSoftLight,
        BlendHardLight, BlendVividLight, BlendLinearLight, BlendPinLight, BlendHardMix,
        BlendDifference, BlendExclusion, BlendHue, BlendSaturation, BlendColor, BlendLuminosity,
    )
}

/// Storyboard commands, listed under their own heading in Settings.
pub const STORYBOARD_ACTIONS: &[&str] = &[
    "ToggleStoryboardBoard",
    "AddPanel",
    "SmartAddPanel",
    "DuplicatePanel",
    "DeletePanel",
    "PreviousPanel",
    "NextPanel",
    "TogglePanelLock",
    "StartScene",
    "RenumberPanels",
    "CopyPanels",
    "PastePanels",
    "NewReviewLayer",
    "PreviousChange",
    "NextChange",
    "ToggleChangeMarks",
    "ShowSharedProject",
    "CheckSharedChanges",
    "ToggleLightTable",
    "ToggleCameraView",
    "ToggleShotGenerator",
    "ToggleCameraTool",
    "AddCameraKey",
    "DeleteCameraKey",
    "PreviousCameraKey",
    "NextCameraKey",
    "PlayPause",
    "StopPlayback",
    "PreviousFrame",
    "NextFrame",
    "FirstFrame",
    "LastFrame",
    "SetPlayIn",
    "SetPlayOut",
    "ClearPlayRange",
    "ToggleLoop",
    "ToggleTimeline",
    "AddTimelineMarker",
    "FindReplaceCaptions",
    "CheckCaptionSpelling",
];

/// The binding contexts a keymap file may name.
pub const CONTEXTS: [(&str, &str); 5] = [
    ("workspace", "Workspace"),
    ("canvas", "Canvas"),
    ("photo_canvas", "Canvas && Photo"),
    ("panel", "NodePanel"),
    ("photo_panel", "NodePanel && Photo"),
];

/// Default shortcuts: (context key, action, keystrokes).
///
/// These follow the industry-standard Windows shortcuts wherever Emulsion has
/// the feature, so people keep their muscle memory; on macOS every
/// `ctrl-` binding also gets a `cmd-` twin.
pub const DEFAULTS: &[(&str, &str, &str)] = &[
    // ── File ──
    ("workspace", "NewDocument", "ctrl-n"),
    ("workspace", "Open", "ctrl-o"),
    ("workspace", "CloseTab", "ctrl-w"),
    ("workspace", "Save", "ctrl-s"),
    ("workspace", "SaveAs", "ctrl-shift-s"),
    ("workspace", "Print", "ctrl-p"),
    // Export As, and Save for Web (Legacy).
    ("workspace", "Export", "ctrl-alt-shift-w"),
    ("workspace", "Export", "ctrl-alt-shift-s"),
    ("workspace", "Quit", "ctrl-q"),
    // ── Edit ──
    ("workspace", "Undo", "ctrl-z"),
    // Step Backward (legacy undo).
    ("workspace", "Undo", "ctrl-alt-z"),
    ("workspace", "Redo", "ctrl-shift-z"),
    ("workspace", "Redo", "ctrl-y"),
    // Preferences, and Keyboard Shortcuts.
    ("workspace", "ShowSettings", "ctrl-k"),
    ("workspace", "ShowSettings", "ctrl-alt-shift-k"),
    ("canvas", "CopyPixels", "ctrl-c"),
    ("canvas", "CutPixels", "ctrl-x"),
    ("canvas", "PastePixels", "ctrl-v"),
    // Paste in Place: at the copied position, on any page or
    // storyboard panel.
    ("canvas", "PasteInPlace", "ctrl-shift-v"),
    ("canvas", "FreeTransform", "ctrl-t"),
    // Photo takes Ctrl+Alt+T only while an editor surface owns focus. The
    // workspace Timeline default remains available in other workflows, and
    // effective_with_overrides preserves explicitly saved overlapping keys.
    ("photo_canvas", "DuplicateTransform", "ctrl-alt-t"),
    ("photo_canvas", "TransformAgain", "ctrl-shift-t"),
    ("photo_canvas", "TransformAgainWithCopy", "ctrl-alt-shift-t"),
    ("canvas", "FillSelection", "alt-backspace"),
    ("canvas", "FillBackground", "ctrl-backspace"),
    ("canvas", "ContentAwareFill", "shift-backspace"),
    ("canvas", "ContentAwareFill", "shift-f5"),
    ("canvas", "CanvasDelete", "delete"),
    ("canvas", "CanvasDelete", "backspace"),
    ("canvas", "CommitTool", "enter"),
    ("canvas", "ResetRotation", "escape"),
    ("canvas", "DiagramAddLeft", "ctrl-alt-left"),
    ("canvas", "DiagramAddRight", "ctrl-alt-right"),
    ("canvas", "DiagramAddUp", "ctrl-alt-up"),
    ("canvas", "DiagramAddDown", "ctrl-alt-down"),
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
    ("panel", "PasteInPlace", "ctrl-shift-v"),
    ("panel", "SelectAll", "ctrl-a"),
    ("panel", "FreeTransform", "ctrl-t"),
    ("photo_panel", "DuplicateTransform", "ctrl-alt-t"),
    ("photo_panel", "TransformAgain", "ctrl-shift-t"),
    ("photo_panel", "TransformAgainWithCopy", "ctrl-alt-shift-t"),
    // Shift+Plus / Shift+Minus blend-mode cycling.
    ("canvas", "NextBlendMode", "shift-="),
    ("canvas", "PreviousBlendMode", "shift--"),
    ("panel", "NextBlendMode", "shift-="),
    ("panel", "PreviousBlendMode", "shift--"),
    // Shift+Alt+letter layer blend modes.
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
    // Number keys: tool opacity with a painting tool, layer
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
    // Photo paint flow uses Shift plus the same rapid percentage digits.
    ("photo_canvas", "Flow10", "shift-1"),
    ("photo_canvas", "Flow20", "shift-2"),
    ("photo_canvas", "Flow30", "shift-3"),
    ("photo_canvas", "Flow40", "shift-4"),
    ("photo_canvas", "Flow50", "shift-5"),
    ("photo_canvas", "Flow60", "shift-6"),
    ("photo_canvas", "Flow70", "shift-7"),
    ("photo_canvas", "Flow80", "shift-8"),
    ("photo_canvas", "Flow90", "shift-9"),
    ("photo_canvas", "Flow100", "shift-0"),
    // Linux may normalize shifted number-row keys to their printed symbols.
    ("photo_canvas", "Flow10", "!"),
    ("photo_canvas", "Flow20", "@"),
    ("photo_canvas", "Flow30", "#"),
    ("photo_canvas", "Flow40", "$"),
    ("photo_canvas", "Flow50", "%"),
    ("photo_canvas", "Flow60", "^"),
    ("photo_canvas", "Flow70", "&"),
    ("photo_canvas", "Flow80", "*"),
    ("photo_canvas", "Flow90", "("),
    ("photo_canvas", "Flow100", ")"),
    ("photo_canvas", "BrushSofter", "{"),
    ("photo_canvas", "BrushHarder", "}"),
    // ── Select ──
    ("canvas", "SelectAll", "ctrl-a"),
    ("workspace", "Deselect", "ctrl-d"),
    ("workspace", "Reselect", "ctrl-shift-d"),
    ("workspace", "InvertSelection", "ctrl-shift-i"),
    ("workspace", "SelectAllLayers", "ctrl-alt-a"),
    // ── Filter ──
    // Last Filter is Ctrl+Alt+F; Ctrl+F searches.
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
    // ── Find ── Ctrl+F searches, as in most apps.
    ("workspace", "FindLayers", "ctrl-f"),
    // Storyboard captions; Ctrl+H is Find and Replace in most apps.
    ("workspace", "FindReplaceCaptions", "ctrl-h"),
    // Spelling sits next to Find and Replace.
    ("workspace", "CheckCaptionSpelling", "ctrl-alt-h"),
    // ── Storyboard ── Ctrl+Alt keeps clear of the standard editing shortcuts; the
    // panel keys follow the layer ones (Ctrl+J duplicates, and so on).
    ("workspace", "ToggleStoryboardBoard", "ctrl-alt-b"),
    ("workspace", "AddPanel", "ctrl-alt-p"),
    ("workspace", "SmartAddPanel", "ctrl-alt-shift-p"),
    ("workspace", "DuplicatePanel", "ctrl-alt-j"),
    ("workspace", "DeletePanel", "ctrl-shift-backspace"),
    ("workspace", "TogglePanelLock", "ctrl-alt-l"),
    ("workspace", "StartScene", "ctrl-alt-n"),
    ("workspace", "RenumberPanels", "ctrl-alt-shift-r"),
    ("workspace", "CopyPanels", "ctrl-alt-shift-c"),
    ("workspace", "PastePanels", "ctrl-alt-shift-v"),
    // Review: a non-printing review layer, and stepping through the panels
    // changed since a version (brackets step, as for layers).
    ("workspace", "NewReviewLayer", "ctrl-alt-r"),
    ("workspace", "PreviousChange", "ctrl-alt-["),
    ("workspace", "NextChange", "ctrl-alt-]"),
    ("workspace", "ToggleChangeMarks", "ctrl-alt-shift-m"),
    // Shared projects through cloud sync: the Shared Project dialog, and
    // checking the cloud for other artists' saves.
    ("workspace", "ShowSharedProject", "ctrl-alt-y"),
    ("workspace", "CheckSharedChanges", "ctrl-alt-shift-y"),
    // The Stage: the light table of neighbouring panels.
    ("workspace", "ToggleLightTable", "ctrl-alt-o"),
    ("workspace", "ToggleCameraView", "ctrl-alt-k"),
    // The Shot Generator's 3D set for the active panel (G for Generator).
    ("workspace", "ToggleShotGenerator", "ctrl-alt-shift-g"),
    // The scene camera on the Stage: the Camera tool and its keys.
    ("canvas", "ToggleCameraTool", "ctrl-alt-e"),
    ("canvas", "AddCameraKey", "ctrl-alt-shift-e"),
    ("canvas", "DeleteCameraKey", "ctrl-alt-shift-backspace"),
    ("canvas", "PreviousCameraKey", "ctrl-alt-shift-,"),
    ("canvas", "NextCameraKey", "ctrl-alt-shift-."),
    // The animatic player, on the Stage and the Board (storyboards only;
    // elsewhere the keys do what they did). Comma and full stop step frames
    // as in storyboard tools; Shift+I/O and Alt+X set and clear the play range
    // as in video editors. On the Stage a quick tap of Space plays (holding it
    // still pans) and Escape stops; on the Board, Space and Escape.
    ("canvas", "PreviousFrame", ","),
    ("canvas", "NextFrame", "."),
    ("canvas", "FirstFrame", "home"),
    ("canvas", "LastFrame", "end"),
    ("canvas", "SetPlayIn", "shift-i"),
    ("canvas", "SetPlayOut", "shift-o"),
    ("canvas", "ClearPlayRange", "alt-x"),
    ("canvas", "ToggleLoop", "alt-shift-r"),
    ("panel", "PlayPause", "space"),
    ("panel", "StopPlayback", "escape"),
    ("panel", "PreviousFrame", ","),
    ("panel", "NextFrame", "."),
    ("panel", "FirstFrame", "home"),
    ("panel", "LastFrame", "end"),
    ("panel", "SetPlayIn", "shift-i"),
    ("panel", "SetPlayOut", "shift-o"),
    ("panel", "ClearPlayRange", "alt-x"),
    ("panel", "ToggleLoop", "alt-shift-r"),
    // The Timeline dock. Plain M also adds a marker while the Timeline has
    // focus; the bound default keeps M itself for the Marquee tool.
    ("workspace", "ToggleTimeline", "ctrl-alt-t"),
    ("panel", "AddTimelineMarker", "ctrl-alt-m"),
    // Page Up and Page Down step through panels on the Stage and the Board.
    ("canvas", "PreviousPanel", "pageup"),
    ("canvas", "NextPanel", "pagedown"),
    ("panel", "PreviousPanel", "pageup"),
    ("panel", "NextPanel", "pagedown"),
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
    // Vector drawing: N steps Line → Rectangle → Ellipse → Polyline.
    ("canvas", "ToolVectorShape", "n"),
    ("canvas", "ToolContourEditor", "a"),
    ("canvas", "ToolPencilRetouch", "shift-a"),
    ("canvas", "ToolHand", "h"),
    ("canvas", "ToolRotateView", "r"),
    ("canvas", "ToolZoom", "z"),
    // Q toggles Quick Mask. The Mask tool stays bindable.
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
    defaults_for_platform(cfg!(target_os = "macos"))
}

fn defaults_for_platform(macos: bool) -> Vec<(String, String, String)> {
    let mut out: Vec<_> = DEFAULTS
        .iter()
        .map(|(c, a, k)| (c.to_string(), a.to_string(), k.to_string()))
        .collect();
    if macos {
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
# binding in the same context for the same keys wins. Several keys:
# ["ctrl-z", "f1"]. Use [] to remove an action's defaults in that context.
# Contexts: [workspace] modifier shortcuts, [canvas] bare keys while
# the canvas has focus, [panel] the scene graph, [photo_canvas] and
# [photo_panel] Photo-only keys for those surfaces. More focused contexts
# take precedence; within one context, bindings are loaded in action-name order.
# Photo panels inherit tool-choice keys from [canvas] and [photo_canvas],
# including remaps and []. Explicit [workspace], [panel] or [photo_panel]
# choices win there; panel editing/navigation and focused inputs keep their keys.
# Photo inherits panel DeleteNode keys as target-aware PanelDelete, including
# remaps/[]; [photo_panel] can override it. DeleteNode explicitly deletes layers.
# Photo's Ctrl+Alt+T (Cmd+Option+T on macOS) starts DuplicateTransform.
# ToggleTimeline keeps that default elsewhere. An explicit saved binding
# in an applicable context takes priority over the new Photo transform defaults;
# remapping or unbinding a transform action also removes its applicable defaults.

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
/// An empty keystroke is an explicit unbinding, not an absent override.
pub fn user_bindings() -> Vec<(String, String, String)> {
    let Ok(text) = std::fs::read_to_string(keymap_path()) else {
        return Vec::new();
    };
    parse_user_bindings(&text)
}

fn parse_user_bindings(text: &str) -> Vec<(String, String, String)> {
    let Ok(v) = toml::from_str::<toml::Table>(text) else {
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
                    if a.is_empty() {
                        out.push((ctx.to_string(), action.clone(), String::new()));
                    }
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
/// every default of the same action in that context). New Photo transform
/// defaults also yield to explicit overrides in applicable ancestor contexts.
pub fn effective() -> Vec<(String, String, String)> {
    effective_with_overrides(platform_defaults(), &user_bindings())
}

/// Pure keymap composition, shared by installation, Settings and tests. Keep
/// explicit user bindings in their original order: GPUI resolves overlapping
/// user scopes by context depth, then the last binding at that depth.
fn effective_with_overrides(
    defaults: Vec<(String, String, String)>,
    user: &[(String, String, String)],
) -> Vec<(String, String, String)> {
    let mut out: Vec<_> = defaults
        .into_iter()
        .filter(|(context, action, keys)| {
            !user.iter().any(|(user_context, user_action, user_keys)| {
                (user_context == context && user_action == action)
                    || (is_photo_transform_default(context, action)
                        && applies_to_photo_context(user_context, context)
                        && (user_action == action || same_keystrokes(user_keys, keys)))
            })
        })
        .collect();
    // Empty overrides have already removed their defaults; never install a
    // zero-keystroke binding or show an empty shortcut in Settings.
    out.extend(
        user.iter()
            .filter(|(_, _, keys)| !keys.trim().is_empty())
            .cloned(),
    );
    inherit_photo_panel_delete(&mut out, user);
    inherit_photo_panel_tools(&mut out, user);
    out
}

/// Photo's keyboard delete follows the active edit target. Derive it from
/// the effective panel DeleteNode keys so saved remaps and [] retain their
/// meaning, while an explicit Photo-only assignment stays authoritative.
fn inherit_photo_panel_delete(
    bindings: &mut Vec<(String, String, String)>,
    user: &[(String, String, String)],
) {
    if user.iter().any(|(scope, action, _)| {
        scope == "photo_panel" && matches!(action.as_str(), "DeleteNode" | "PanelDelete")
    }) {
        return;
    }
    let contexts = [
        KeyContext::parse("Workspace").unwrap(),
        KeyContext::parse("NodePanel").unwrap(),
    ];
    let keymap = Keymap::new(
        bindings
            .iter()
            .filter_map(|(c, a, k)| binding(a, k, context_name(c)))
            .collect(),
    );
    let inherited: Vec<_> = bindings
        .iter()
        .filter(|(context, action, keys)| {
            if context != "panel" || action != "DeleteNode" {
                return false;
            }
            let Some(input) = parse_keystrokes(keys) else {
                return false;
            };
            let (matches, _) = keymap.bindings_for_input(&input, &contexts);
            if !matches
                .first()
                .is_some_and(|b| b.action().name().rsplit("::").next() == Some("DeleteNode"))
            {
                return false;
            }
            // A Photo-specific binding also owns its chord prefixes: inheritance
            // must not delay an explicitly assigned single key or consume a chord.
            !user.iter().any(|(scope, _, existing)| {
                scope == "photo_panel"
                    && parse_keystrokes(existing).is_some_and(|existing| {
                        !existing.is_empty()
                            && (input.starts_with(&existing) || existing.starts_with(&input))
                    })
            })
        })
        .map(|(_, _, keys)| ("photo_panel".into(), "PanelDelete".into(), keys.clone()))
        .collect();
    // Keep every explicit user binding in its original order and context.
    bindings.splice(0..0, inherited);
}

/// Only tool choices cross the canvas/panel focus boundary. In particular,
/// opacity/flow, deletion, transforms, clipboard and navigation never do.
fn is_tool_choice(action: &str) -> bool {
    matches!(
        action,
        "ToolMove"
            | "ToolHand"
            | "ToolRotateView"
            | "ToolZoom"
            | "ToolEyedropper"
            | "ToolMarquee"
            | "ToolRectangularMarquee"
            | "ToolEllipseMarquee"
            | "ToolLasso"
            | "ToolPolygonLasso"
            | "ToolMagneticLasso"
            | "ToolWand"
            | "ToolQuickSelect"
            | "ToolCrop"
            | "ToolHeal"
            | "ToolRemove"
            | "ToolClone"
            | "ToolBrush"
            | "ToolSmudge"
            | "ToolLiquify"
            | "ToolEraser"
            | "ToolGradient"
            | "ToolBucket"
            | "ToolPen"
            | "ToolFreeformPen"
            | "ToolType"
            | "ToolVerticalType"
            | "ToolShape"
            | "ToolEllipse"
            | "ToolVectorShape"
            | "ToolContourEditor"
            | "ToolPencilRetouch"
            | "ToolMask"
            | "ToolGrade"
    )
}

fn inherit_photo_panel_tools(
    bindings: &mut Vec<(String, String, String)>,
    user: &[(String, String, String)],
) {
    let canvas_contexts = [
        KeyContext::parse("Workspace").unwrap(),
        KeyContext::parse("Canvas Photo").unwrap(),
    ];
    let canvas_keymap = Keymap::new(
        bindings
            .iter()
            .filter_map(|(context, action, keys)| binding(action, keys, context_name(context)))
            .collect(),
    );
    let inherited: Vec<_> = bindings
        .iter()
        .filter(|(context, action, keys)| {
            if !matches!(context.as_str(), "canvas" | "photo_canvas") || !is_tool_choice(action) {
                return false;
            }
            // A Photo-specific source remap/unbinding replaces the inherited
            // generic choice. An explicit destination choice always wins, even [].
            if user.iter().any(|(scope, name, _)| {
                name == action
                    && (applies_to_photo_context(scope, "photo_panel")
                        || (context == "canvas" && scope == "photo_canvas"))
            }) {
                return false;
            }
            let Some(input) = parse_keystrokes(keys) else {
                return false;
            };
            let Some(first) = input.first() else {
                return false;
            };
            // These are owned by panel controls even where they use raw key events
            // rather than registered actions. Do not import a canvas remap over them.
            if matches!(
                first.key.as_str(),
                "0" | "1"
                    | "2"
                    | "3"
                    | "4"
                    | "5"
                    | "6"
                    | "7"
                    | "8"
                    | "9"
                    | "!"
                    | "@"
                    | "#"
                    | "$"
                    | "%"
                    | "^"
                    | "&"
                    | "*"
                    | "("
                    | ")"
                    | "delete"
                    | "backspace"
                    | "left"
                    | "right"
                    | "up"
                    | "down"
                    | "home"
                    | "end"
                    | "pageup"
                    | "pagedown"
                    | "space"
                    | "enter"
                    | "escape"
                    | "tab"
            ) {
                return false;
            }
            // Preserve the effective canvas winner, including a non-tool action
            // remapped over a default tool key, rather than resurrecting that key.
            let (matches, _) = canvas_keymap.bindings_for_input(&input, &canvas_contexts);
            if !matches
                .first()
                .is_some_and(|b| b.action().name().rsplit("::").next() == Some(action.as_str()))
            {
                return false;
            }
            // Existing panel/workspace bindings also own chord prefixes. Adding
            // a longer tool chord must not delay or shadow their first keystroke.
            !bindings.iter().any(|(scope, _, existing)| {
                applies_to_photo_context(scope, "photo_panel")
                    && parse_keystrokes(existing).is_some_and(|existing| {
                        !existing.is_empty()
                            && (input.starts_with(&existing) || existing.starts_with(&input))
                    })
            })
        })
        .map(|(_, action, keys)| ("photo_panel".into(), action.clone(), keys.clone()))
        .collect();
    // Derived defaults stay before explicit bindings so the user tail and
    // existing same-depth ordering remain unchanged.
    bindings.splice(0..0, inherited);
}

fn parse_keystrokes(keys: &str) -> Option<Vec<Keystroke>> {
    keys.split_whitespace()
        .map(Keystroke::parse)
        .collect::<Result<_, _>>()
        .ok()
}

fn is_photo_transform_default(context: &str, action: &str) -> bool {
    matches!(context, "photo_canvas" | "photo_panel")
        && matches!(
            action,
            "DuplicateTransform" | "TransformAgain" | "TransformAgainWithCopy"
        )
}

fn applies_to_photo_context(user_context: &str, photo_context: &str) -> bool {
    user_context == "workspace"
        || user_context == photo_context
        || matches!(
            (user_context, photo_context),
            ("canvas", "photo_canvas") | ("panel", "photo_panel")
        )
}

fn same_keystrokes(left: &str, right: &str) -> bool {
    let parse = |keys: &str| {
        keys.split_whitespace()
            .map(Keystroke::parse)
            .collect::<Result<Vec<_>, _>>()
    };
    match (parse(left), parse(right)) {
        (Ok(left), Ok(right)) => !left.is_empty() && left == right,
        _ => false,
    }
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
    // A focused embedded player owns playback keys. Prevent the surrounding
    // canvas/workspace shortcuts from consuming them before its key callbacks.
    for key in [
        "left", "right", "up", "down", "space", "escape", "home", "end", "k", "j", "l", "f", "m",
        "c", "t", "i", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", ",", ".", "shift-,",
        "shift-.",
    ] {
        bindings.push(KeyBinding::new(
            key,
            gpui_kit::NoAction,
            Some("EmbeddedVideo"),
        ));
    }
    // The Shot Generator's focused viewport owns its gizmo keys: W, E and R
    // pick Move, Rotate and Scale, X switches local and world axes.
    for key in ["w", "e", "r", "x"] {
        bindings.push(KeyBinding::new(
            key,
            gpui_kit::NoAction,
            Some("ShotViewport"),
        ));
    }
    // Crop preview owns its keys before any Workspace or user-remapped
    // action can mutate the authored document. NoAction still delivers the
    // raw key event to the crop controls (Enter, Escape, arrows and zoom).
    for (_, _, keys) in effective() {
        bindings.push(KeyBinding::new(
            &keys,
            gpui_kit::NoAction,
            Some("FrameCrop"),
        ));
    }
    bindings.push(KeyBinding::new(
        "escape",
        gpui_kit::NoAction,
        Some("DesignAssetLoading"),
    ));
    // The page grid owns editing shortcuts, including user remaps, so a key
    // cannot change hidden canvas objects. Keep document/file navigation usable.
    for (_, action, keys) in effective() {
        if !matches!(
            action.as_str(),
            "Save"
                | "SaveAs"
                | "NextTab"
                | "PrevTab"
                | "CloseTab"
                | "Quit"
                | "Open"
                | "NewDocument"
                | "ShowHome"
                | "ShowSettings"
                | "ShowAbout"
                | "Ask"
        ) {
            bindings.push(KeyBinding::new(
                &keys,
                gpui_kit::NoAction,
                Some("DesignPageOrganizer"),
            ));
        }
    }
    for key in [
        "escape",
        "enter",
        "space",
        "delete",
        "backspace",
        "left",
        "right",
        "up",
        "down",
        "shift-left",
        "shift-right",
        "shift-up",
        "shift-down",
        "alt-left",
        "alt-right",
        "ctrl-a",
        "cmd-a",
        "ctrl-d",
        "cmd-d",
        "ctrl-z",
        "cmd-z",
        "ctrl-shift-z",
        "cmd-shift-z",
        "ctrl-y",
        "cmd-y",
    ] {
        bindings.push(KeyBinding::new(
            key,
            gpui_kit::NoAction,
            Some("DesignPageOrganizer"),
        ));
    }

    cx.bind_keys(bindings);
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULTS, binding, context_name, defaults_for_platform, effective_with_overrides,
        keymap_template, parse_user_bindings,
    };
    use gpui_kit::{KeyContext, Keymap, Keystroke};

    const PHOTO_TRANSFORM_ACTIONS: [(&str, &str); 3] = [
        ("DuplicateTransform", "ctrl-alt-t"),
        ("TransformAgain", "ctrl-shift-t"),
        ("TransformAgainWithCopy", "ctrl-alt-shift-t"),
    ];

    fn owned_bindings(bindings: &[(&str, &str, &str)]) -> Vec<(String, String, String)> {
        bindings
            .iter()
            .map(|(context, action, keys)| {
                (context.to_string(), action.to_string(), keys.to_string())
            })
            .collect()
    }

    fn resolved_action(
        bindings: &[(String, String, String)],
        surface: &str,
        keys: &str,
    ) -> Option<String> {
        let keymap = Keymap::new(
            bindings
                .iter()
                .filter_map(|(context, action, keys)| binding(action, keys, context_name(context)))
                .collect(),
        );
        let contexts = [
            KeyContext::parse("Workspace").unwrap(),
            KeyContext::parse(surface).unwrap(),
        ];
        let keys: Vec<_> = keys
            .split_whitespace()
            .map(|key| Keystroke::parse(key).unwrap())
            .collect();
        let (matches, pending) = keymap.bindings_for_input(&keys, &contexts);
        assert!(!pending);
        matches.first().map(|binding| {
            binding
                .action()
                .name()
                .rsplit("::")
                .next()
                .unwrap()
                .to_string()
        })
    }

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
            ("canvas", "PasteInPlace", "ctrl-shift-v"),
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
        // Keep cross-workflow shortcuts consistent except the deliberate
        // Photo-only DuplicateTransform / Timeline overlap.
        for (_, action, keys) in defaults.iter().filter(|(c, _, _)| c == "workspace") {
            assert!(
                defaults
                    .iter()
                    .filter(|(c, _, k)| c != "workspace" && k == keys)
                    .all(|(c, a, _)| {
                        a == action
                            || (action == "ToggleTimeline"
                                && a == "DuplicateTransform"
                                && matches!(c.as_str(), "photo_canvas" | "photo_panel"))
                    }),
                "{keys} is shadowed on the canvas or panel"
            );
        }
    }

    #[test]
    fn photo_repeat_transform_defaults_resolve_only_on_photo_editor_surfaces() {
        for macos in [false, true] {
            let effective = effective_with_overrides(defaults_for_platform(macos), &[]);
            let modifiers: &[&str] = if macos { &["ctrl", "cmd"] } else { &["ctrl"] };
            for modifier in modifiers {
                for (action, control_keys) in PHOTO_TRANSFORM_ACTIONS {
                    let keys = control_keys.replacen("ctrl", modifier, 1);
                    for surface in ["Canvas Photo", "NodePanel Photo"] {
                        assert_eq!(
                            resolved_action(&effective, surface, &keys).as_deref(),
                            Some(action),
                            "{surface}: {keys}"
                        );
                    }
                    for surface in ["Canvas", "NodePanel", "CanvasText", "Input", "Dialog"] {
                        let expected = (action == "DuplicateTransform").then_some("ToggleTimeline");
                        assert_eq!(
                            resolved_action(&effective, surface, &keys).as_deref(),
                            expected,
                            "{surface}: {keys} must not install a Photo transform action"
                        );
                    }
                }
                for surface in ["Canvas Photo", "NodePanel Photo"] {
                    assert_eq!(
                        resolved_action(&effective, surface, &format!("{modifier}-t")).as_deref(),
                        Some("FreeTransform")
                    );
                }
            }
            assert_eq!(
                resolved_action(&effective, "Canvas Photo", "t").as_deref(),
                Some("ToolType")
            );
            assert_eq!(
                resolved_action(&effective, "Canvas Photo", "shift-t").as_deref(),
                Some("ToolVerticalType")
            );
        }
    }

    #[test]
    fn saved_overlapping_bindings_win_over_new_photo_defaults() {
        for (context, surfaces) in [
            ("workspace", &["Canvas Photo", "NodePanel Photo"][..]),
            ("canvas", &["Canvas Photo"][..]),
            ("photo_canvas", &["Canvas Photo"][..]),
            ("panel", &["NodePanel Photo"][..]),
            ("photo_panel", &["NodePanel Photo"][..]),
        ] {
            for (_, keys) in PHOTO_TRANSFORM_ACTIONS {
                let user = owned_bindings(&[(context, "ToggleTimeline", keys)]);
                let effective = effective_with_overrides(defaults_for_platform(false), &user);
                for surface in surfaces {
                    assert_eq!(
                        resolved_action(&effective, surface, keys).as_deref(),
                        Some("ToggleTimeline"),
                        "explicit {context} binding must win on {surface}"
                    );
                }
                // The Timeline default still exists outside Photo; existing
                // same-action workspace remapping behavior is unchanged.
                if context != "workspace" || keys == "ctrl-alt-t" {
                    assert_eq!(
                        resolved_action(&effective, "Canvas", "ctrl-alt-t").as_deref(),
                        Some("ToggleTimeline")
                    );
                }
            }
        }
    }

    #[test]
    fn sibling_surface_overrides_do_not_remove_unrelated_photo_defaults() {
        for (context, unaffected_surface) in [
            ("canvas", "NodePanel Photo"),
            ("photo_canvas", "NodePanel Photo"),
            ("panel", "Canvas Photo"),
            ("photo_panel", "Canvas Photo"),
        ] {
            for (action, keys) in PHOTO_TRANSFORM_ACTIONS {
                let user = owned_bindings(&[(context, "ToggleTimeline", keys)]);
                let effective = effective_with_overrides(defaults_for_platform(false), &user);
                assert_eq!(
                    resolved_action(&effective, unaffected_surface, keys).as_deref(),
                    Some(action)
                );
            }
        }
    }

    #[test]
    fn photo_transform_remaps_replace_all_applicable_action_defaults() {
        for (context, affected_contexts) in [
            ("workspace", &["photo_canvas", "photo_panel"][..]),
            ("canvas", &["photo_canvas"][..]),
            ("photo_canvas", &["photo_canvas"][..]),
            ("panel", &["photo_panel"][..]),
            ("photo_panel", &["photo_panel"][..]),
        ] {
            for (action, _) in PHOTO_TRANSFORM_ACTIONS {
                for replacement in ["f10", ""] {
                    let user = owned_bindings(&[(context, action, replacement)]);
                    let effective = effective_with_overrides(defaults_for_platform(true), &user);
                    for affected_context in affected_contexts {
                        assert!(effective.iter().all(|(c, a, keys)| {
                            c != affected_context || a != action || keys == replacement
                        }));
                    }
                    assert!(effective.iter().all(|(_, _, keys)| !keys.is_empty()));
                    if !replacement.is_empty() {
                        assert!(effective.contains(&user[0]));
                    }
                }
            }
        }
    }

    #[test]
    fn explicit_empty_arrays_survive_parsing_and_never_become_key_bindings() {
        let user = parse_user_bindings(
            r#"
[workspace]
Undo = []
[canvas]
TransformAgain = []
[photo_panel]
DuplicateTransform = []
TransformAgainWithCopy = ["f3", "f4"]
"#,
        );
        for (context, action) in [
            ("workspace", "Undo"),
            ("canvas", "TransformAgain"),
            ("photo_panel", "DuplicateTransform"),
        ] {
            assert!(user.contains(&(context.into(), action.into(), String::new())));
        }
        assert_eq!(user.len(), 5);
        let effective = effective_with_overrides(defaults_for_platform(true), &user);
        assert!(effective.iter().all(|(_, _, keys)| !keys.is_empty()));
        assert!(!effective.iter().any(|(_, action, _)| action == "Undo"));
        assert!(!effective.iter().any(|(context, action, _)| {
            context == "photo_canvas" && action == "TransformAgain"
        }));
        assert!(!effective.iter().any(|(context, action, _)| {
            context == "photo_panel" && action == "DuplicateTransform"
        }));
        for keys in ["f3", "f4"] {
            assert_eq!(
                resolved_action(&effective, "NodePanel Photo", keys).as_deref(),
                Some("TransformAgainWithCopy")
            );
        }
        assert!(keymap_template().contains("Use []"));
        assert!(keymap_template().contains("[photo_panel]"));
    }

    #[test]
    fn mac_control_and_command_overrides_claim_chords_independently() {
        for (explicit_keys, matching_keys, unaffected_keys) in [
            ("alt-CTRL-t", "ctrl-alt-t", "cmd-alt-t"),
            ("alt-SUPER-t", "cmd-alt-t", "ctrl-alt-t"),
        ] {
            let user = owned_bindings(&[("workspace", "ToggleTimeline", explicit_keys)]);
            let effective = effective_with_overrides(defaults_for_platform(true), &user);
            for surface in ["Canvas Photo", "NodePanel Photo"] {
                assert_eq!(
                    resolved_action(&effective, surface, matching_keys).as_deref(),
                    Some("ToggleTimeline")
                );
                assert_eq!(
                    resolved_action(&effective, surface, unaffected_keys).as_deref(),
                    Some("DuplicateTransform")
                );
            }
        }
    }

    #[test]
    fn explicit_user_collisions_keep_context_depth_and_last_binding_precedence() {
        // Two explicit bindings are not suppressed by the default migration.
        // The deeper canvas binding wins even when the workspace one is last.
        let user = owned_bindings(&[
            ("photo_canvas", "DuplicateTransform", "ctrl-alt-t"),
            ("workspace", "ToggleTimeline", "ctrl-alt-t"),
        ]);
        let effective = effective_with_overrides(defaults_for_platform(false), &user);
        assert!(effective.ends_with(&user));
        assert_eq!(
            resolved_action(&effective, "Canvas Photo", "ctrl-alt-t").as_deref(),
            Some("DuplicateTransform")
        );
        assert_eq!(
            resolved_action(&effective, "NodePanel Photo", "ctrl-alt-t").as_deref(),
            Some("ToggleTimeline")
        );
        for actions in [
            ["DuplicateTransform", "ToggleTimeline"],
            ["ToggleTimeline", "DuplicateTransform"],
        ] {
            let user = owned_bindings(&[
                ("photo_canvas", actions[0], "ctrl-alt-t"),
                ("photo_canvas", actions[1], "ctrl-alt-t"),
            ]);
            let effective = effective_with_overrides(defaults_for_platform(false), &user);
            assert!(effective.ends_with(&user));
            assert_eq!(
                resolved_action(&effective, "Canvas Photo", "ctrl-alt-t").as_deref(),
                Some(actions[1])
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
    #[test]
    fn photo_panel_tool_defaults_are_scoped_and_do_not_mirror_canvas_edits() {
        let keys = effective_with_overrides(defaults_for_platform(false), &[]);
        for (key, action) in [
            ("v", "ToolMove"),
            ("g", "ToolGradient"),
            ("shift-m", "ToolEllipseMarquee"),
            ("shift-u", "ToolEllipse"),
        ] {
            assert_eq!(
                resolved_action(&keys, "NodePanel Photo", key).as_deref(),
                Some(action)
            );
            assert_eq!(resolved_action(&keys, "NodePanel", key), None);
            assert_eq!(
                resolved_action(&keys, "Canvas Photo", key).as_deref(),
                Some(action)
            );
        }
        for key in [
            "1",
            "shift-1",
            "[",
            "left",
            "d",
            "x",
            "q",
            "shift-backspace",
        ] {
            assert_eq!(
                resolved_action(&keys, "NodePanel Photo", key),
                None,
                "{key}"
            );
        }
        for (key, action) in [
            ("delete", "PanelDelete"),
            ("ctrl-c", "CopyPixels"),
            ("ctrl-t", "FreeTransform"),
            ("space", "PlayPause"),
        ] {
            assert_eq!(
                resolved_action(&keys, "NodePanel Photo", key).as_deref(),
                Some(action)
            );
        }
    }

    #[test]
    fn photo_panel_inherits_tool_remaps_and_explicit_source_unbindings() {
        for scope in ["canvas", "photo_canvas"] {
            let user = parse_user_bindings(&format!(
                "[{scope}]\nToolMove = [\"k\", \"f12\"]\nToolGradient = []"
            ));
            let keys = effective_with_overrides(defaults_for_platform(false), &user);
            for key in ["k", "f12"] {
                assert_eq!(
                    resolved_action(&keys, "NodePanel Photo", key).as_deref(),
                    Some("ToolMove")
                );
            }
            assert_eq!(resolved_action(&keys, "NodePanel Photo", "v"), None);
            assert_eq!(resolved_action(&keys, "NodePanel Photo", "g"), None);
        }
        let user = owned_bindings(&[("canvas", "DeleteNode", "v")]);
        let keys = effective_with_overrides(defaults_for_platform(false), &user);
        assert_eq!(
            resolved_action(&keys, "NodePanel Photo", "v"),
            None,
            "an overridden canvas tool key cannot be resurrected on the panel"
        );
    }

    #[test]
    fn photo_panel_tool_mirrors_yield_to_destination_remaps_unbindings_and_chords() {
        for scope in ["workspace", "panel", "photo_panel"] {
            for override_key in ["k", ""] {
                let user = owned_bindings(&[(scope, "ToolMove", override_key)]);
                let keys = effective_with_overrides(defaults_for_platform(false), &user);
                assert_eq!(resolved_action(&keys, "NodePanel Photo", "v"), None);
                if !override_key.is_empty() {
                    assert_eq!(
                        resolved_action(&keys, "NodePanel Photo", "k").as_deref(),
                        Some("ToolMove")
                    );
                }
            }
            let user = owned_bindings(&[(scope, "ToggleNodeVisible", "g")]);
            let keys = effective_with_overrides(defaults_for_platform(false), &user);
            assert_eq!(
                resolved_action(&keys, "NodePanel Photo", "g").as_deref(),
                Some("ToggleNodeVisible")
            );
            let user = owned_bindings(&[(scope, "ToggleNodeVisible", "g k")]);
            let keys = effective_with_overrides(defaults_for_platform(false), &user);
            assert!(
                !keys
                    .iter()
                    .any(|(c, a, _)| c == "photo_panel" && a == "ToolGradient")
            );
        }
        for reserved in [
            "delete",
            "backspace",
            "ctrl-c",
            "ctrl-a",
            "space",
            "left",
            "1",
            "shift-2",
            "ctrl-j",
            "ctrl-t k",
        ] {
            let user = owned_bindings(&[("canvas", "ToolMove", reserved)]);
            let keys = effective_with_overrides(defaults_for_platform(false), &user);
            assert!(
                !keys
                    .iter()
                    .any(|(c, a, _)| c == "photo_panel" && a == "ToolMove"),
                "{reserved}"
            );
        }
    }

    #[test]
    fn photo_panel_tool_bindings_exclude_editable_and_menu_descendants() {
        let bindings = effective_with_overrides(defaults_for_platform(false), &[]);
        let keymap = Keymap::new(
            bindings
                .iter()
                .filter_map(|(c, a, k)| binding(a, k, context_name(c)))
                .collect(),
        );
        for child in ["Input", "CanvasText", "PopupMenu", "Slider"] {
            let contexts = [
                KeyContext::parse("Workspace").unwrap(),
                KeyContext::parse("NodePanel Photo").unwrap(),
                KeyContext::parse(child).unwrap(),
            ];
            for key in ["v", "g", "shift-m", "shift-u", "ctrl-shift-x"] {
                let (matches, pending) =
                    keymap.bindings_for_input(&[Keystroke::parse(key).unwrap()], &contexts);
                assert!(!pending);
                assert!(matches.is_empty(), "{child}: {key}");
            }
        }
    }
    #[test]
    fn photo_panel_delete_inherits_legacy_keys_without_changing_other_surfaces() {
        let defaults = effective_with_overrides(defaults_for_platform(false), &[]);
        for key in ["delete", "backspace"] {
            assert_eq!(
                resolved_action(&defaults, "NodePanel Photo", key).as_deref(),
                Some("PanelDelete")
            );
            assert_eq!(
                resolved_action(&defaults, "NodePanel", key).as_deref(),
                Some("DeleteNode")
            );
            assert_eq!(
                resolved_action(&defaults, "Canvas Photo", key).as_deref(),
                Some("CanvasDelete")
            );
        }
        for replacement in ["f12", ""] {
            let user = owned_bindings(&[("panel", "DeleteNode", replacement)]);
            let effective = effective_with_overrides(defaults_for_platform(false), &user);
            for surface in ["NodePanel", "NodePanel Photo"] {
                for key in ["delete", "backspace"] {
                    assert_eq!(resolved_action(&effective, surface, key), None);
                }
                if !replacement.is_empty() {
                    let action = if surface == "NodePanel Photo" {
                        "PanelDelete"
                    } else {
                        "DeleteNode"
                    };
                    assert_eq!(
                        resolved_action(&effective, surface, replacement).as_deref(),
                        Some(action)
                    );
                }
            }
        }
    }

    #[test]
    fn photo_panel_delete_yields_to_explicit_photo_overrides_and_empty_arrays() {
        for action in ["DeleteNode", "PanelDelete"] {
            for replacement in ["f12", ""] {
                let user = owned_bindings(&[("photo_panel", action, replacement)]);
                let effective = effective_with_overrides(defaults_for_platform(false), &user);
                for key in ["delete", "backspace"] {
                    assert_eq!(resolved_action(&effective, "NodePanel Photo", key), None);
                    assert_eq!(
                        resolved_action(&effective, "NodePanel", key).as_deref(),
                        Some("DeleteNode")
                    );
                }
                if !replacement.is_empty() {
                    assert_eq!(
                        resolved_action(&effective, "NodePanel Photo", replacement).as_deref(),
                        Some(action)
                    );
                }
                assert!(effective.ends_with(&user[..usize::from(!replacement.is_empty())]));
            }
        }
        let user = owned_bindings(&[("photo_panel", "ToggleNodeVisible", "delete")]);
        let effective = effective_with_overrides(defaults_for_platform(false), &user);
        assert_eq!(
            resolved_action(&effective, "NodePanel Photo", "delete").as_deref(),
            Some("ToggleNodeVisible")
        );
        assert_eq!(
            resolved_action(&effective, "NodePanel Photo", "backspace").as_deref(),
            Some("PanelDelete")
        );
    }

    #[test]
    fn photo_panel_target_delete_never_consumes_editable_or_menu_input() {
        for replacement in ["delete", "k", "k k"] {
            let user = owned_bindings(&[("panel", "DeleteNode", replacement)]);
            let effective = effective_with_overrides(defaults_for_platform(false), &user);
            let keymap = Keymap::new(
                effective
                    .iter()
                    .filter_map(|(c, a, k)| binding(a, k, context_name(c)))
                    .collect(),
            );
            for child in ["Input", "CanvasText", "PopupMenu", "Slider"] {
                let contexts = [
                    KeyContext::parse("Workspace").unwrap(),
                    KeyContext::parse("NodePanel Photo").unwrap(),
                    KeyContext::parse(child).unwrap(),
                ];
                let input: Vec<_> = replacement
                    .split_whitespace()
                    .map(|k| Keystroke::parse(k).unwrap())
                    .collect();
                let (matches, pending) = keymap.bindings_for_input(&input, &contexts);
                assert!(!pending);
                assert!(matches.is_empty(), "{child}: {replacement}");
            }
        }
    }
}
