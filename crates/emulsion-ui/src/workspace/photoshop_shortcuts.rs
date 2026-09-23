//! Handlers for the actions that exist so Photoshop's default shortcuts
//! reach the matching Emulsion feature.
use super::*;
use emulsion_raster::blend::BlendMode;

/// Each blend-mode action and the mode it sets.
macro_rules! blend_actions {
    ($d:expr, $cx:expr; $($action:ident => $mode:ident),* $(,)?) => {
        $d$(.on_action($cx.listener(|this, _: &$action, _, cx| {
            this.with_editor(cx, |e, cx| e.set_layer_blend(BlendMode::$mode, cx))
        })))*
    };
}

/// Each opacity action and its percentage.
macro_rules! opacity_actions {
    ($d:expr, $cx:expr; $($action:ident => $percent:expr),* $(,)?) => {
        $d$(.on_action($cx.listener(|this, _: &$action, _, cx| {
            this.with_editor(cx, |e, cx| e.opacity_shortcut($percent, cx))
        })))*
    };
}

/// Each adjustment action and its catalogue key.
macro_rules! adjustment_actions {
    ($d:expr, $cx:expr; $($action:ident => $key:expr),* $(,)?) => {
        $d$(.on_action($cx.listener(|this, _: &$action, _, cx| {
            this.with_editor(cx, |e, cx| e.quick_adjust($key, cx))
        })))*
    };
}

impl Workspace {
    pub(super) fn photoshop_actions(d: Stateful<Div>, cx: &Context<Self>) -> Stateful<Div> {
        let d =
            d.on_action(cx.listener(|this, _: &Reselect, _, cx| {
                this.with_editor(cx, |e, cx| e.reselect(cx))
            }))
            .on_action(cx.listener(|this, _: &FillBackground, _, cx| {
                this.with_editor(cx, |e, cx| e.fill_background(cx))
            }))
            .on_action(cx.listener(|this, _: &ToggleSnap, _, cx| {
                this.with_editor(cx, |e, cx| {
                    e.snap = !e.snap;
                    let state = if e.snap { "on" } else { "off" };
                    e.set_status(format!("Snap {state}."), false, cx);
                    cx.notify();
                })
            }))
            .on_action(cx.listener(|this, _: &ToggleClippingMask, _, cx| {
                this.with_editor(cx, |e, cx| e.toggle_clipping_mask(cx))
            }))
            .on_action(cx.listener(|this, _: &BringToFront, _, cx| {
                this.with_editor(cx, |e, cx| e.shift_selected_to_end(true, cx))
            }))
            .on_action(cx.listener(|this, _: &SendToBack, _, cx| {
                this.with_editor(cx, |e, cx| e.shift_selected_to_end(false, cx))
            }))
            .on_action(cx.listener(|this, _: &SelectLayerAbove, _, cx| {
                this.with_editor(cx, |e, cx| e.select_adjacent_layer(true, false, cx))
            }))
            .on_action(cx.listener(|this, _: &SelectLayerBelow, _, cx| {
                this.with_editor(cx, |e, cx| e.select_adjacent_layer(false, false, cx))
            }))
            .on_action(cx.listener(|this, _: &AddLayerAboveToSelection, _, cx| {
                this.with_editor(cx, |e, cx| e.select_adjacent_layer(true, true, cx))
            }))
            .on_action(cx.listener(|this, _: &AddLayerBelowToSelection, _, cx| {
                this.with_editor(cx, |e, cx| e.select_adjacent_layer(false, true, cx))
            }))
            .on_action(cx.listener(|this, _: &SelectTopLayer, _, cx| {
                this.with_editor(cx, |e, cx| e.select_edge_layer(true, cx))
            }))
            .on_action(cx.listener(|this, _: &SelectBottomLayer, _, cx| {
                this.with_editor(cx, |e, cx| e.select_edge_layer(false, cx))
            }))
            .on_action(cx.listener(|this, _: &SelectAllLayers, _, cx| {
                this.with_editor(cx, |e, cx| e.select_all_layers(cx))
            }))
            .on_action(cx.listener(|this, _: &AdjustDesaturate, _, cx| {
                this.with_editor(cx, |e, cx| e.quick_desaturate(cx))
            }))
            .on_action(cx.listener(|this, _: &FilterLensCorrection, _, cx| {
                this.with_editor(cx, |e, cx| e.apply_filter_key("lens_correction", cx))
            }))
            .on_action(cx.listener(|this, _: &BrushSofter, _, cx| {
                this.with_editor(cx, |e, cx| e.brush_hardness(false, cx))
            }))
            .on_action(cx.listener(|this, _: &BrushHarder, _, cx| {
                this.with_editor(cx, |e, cx| e.brush_hardness(true, cx))
            }))
            .on_action(cx.listener(|this, _: &ShowLayersPanel, window, cx| {
                this.with_editor(cx, |e, cx| e.show_layers_panel(window, cx))
            }))
            .on_action(cx.listener(|this, _: &FindLayers, window, cx| {
                this.with_editor(cx, |e, cx| e.find_layers(window, cx))
            }))
            .on_action(cx.listener(|this, _: &ShowInfoPanel, _, cx| {
                this.with_editor(cx, |e, cx| e.show_info_panel(cx))
            }))
            .on_action(cx.listener(|this, _: &ShowBrushSettings, _, cx| {
                this.with_editor(cx, |e, cx| e.show_brush_settings(cx))
            }))
            .on_action(cx.listener(|this, _: &TogglePanels, _, cx| {
                this.with_editor(cx, |e, cx| e.toggle_panel_dock(cx))
            }))
            .on_action(cx.listener(|this, _: &ToggleScreenMode, window, cx| {
                if this.editor.is_some() && !this.style_dialog_open(cx) {
                    window.toggle_fullscreen();
                }
            }))
            .on_action(cx.listener(|this, _: &ToolRemove, _, cx| {
                this.with_editor(cx, |e, cx| {
                    let removing = e.tool == crate::editor::Tool::Heal && e.tools.remove.enabled;
                    e.set_remove_mode(!removing, cx)
                })
            }))
            .on_action(cx.listener(|this, _: &ToolFreeformPen, _, cx| {
                use crate::editor::PenMode;
                this.with_editor(cx, |e, cx| {
                    let next = match (e.tool == crate::editor::Tool::Pen, e.tools.pen.mode) {
                        (true, PenMode::Free) => PenMode::Curvature,
                        (true, PenMode::Curvature) => PenMode::Pen,
                        _ => PenMode::Free,
                    };
                    e.set_pen_mode(next, cx)
                })
            }));
        let d = adjustment_actions!(d, cx;
            AdjustLevels => "levels",
            AdjustCurves => "curves",
            AdjustHueSaturation => "hue_saturation",
            AdjustColorBalance => "color_balance",
            AdjustBlackAndWhite => "black_and_white",
            AdjustInvert => "invert",
        );
        let d = opacity_actions!(d, cx;
            Opacity10 => 10, Opacity20 => 20, Opacity30 => 30, Opacity40 => 40,
            Opacity50 => 50, Opacity60 => 60, Opacity70 => 70, Opacity80 => 80,
            Opacity90 => 90, Opacity100 => 100,
        );
        blend_actions!(d, cx;
            BlendNormal => Normal,
            BlendDissolve => Dissolve,
            BlendDarken => Darken,
            BlendMultiply => Multiply,
            BlendColorBurn => ColorBurn,
            BlendLinearBurn => LinearBurn,
            BlendLighten => Lighten,
            BlendScreen => Screen,
            BlendColorDodge => ColorDodge,
            BlendLinearDodge => LinearDodge,
            BlendOverlay => Overlay,
            BlendSoftLight => SoftLight,
            BlendHardLight => HardLight,
            BlendVividLight => VividLight,
            BlendLinearLight => LinearLight,
            BlendPinLight => PinLight,
            BlendHardMix => HardMix,
            BlendDifference => Difference,
            BlendExclusion => Exclusion,
            BlendHue => Hue,
            BlendSaturation => Saturation,
            BlendColor => Color,
            BlendLuminosity => Luminosity,
        )
    }
}
