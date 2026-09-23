//! Selective Color exposes one range while retaining the other ranges.
use super::*;
use emulsion_core::NodeKind;
use emulsion_raster::adjust::{Adjustment, SELECTIVE_COLOR_KEYS};
use gpui_kit::test::TestWindowExt;

#[gpui_kit::test]
fn selective_color_properties_preserve_ranges_and_undo_preset(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1600.), gpui_kit::px(1100.)));
    let editor = cx.update(|_, cx| {
        let editor = ws.read(cx).editor.clone().unwrap();
        editor.update(cx, |e, cx| e.quick_adjust("selective_color", cx));
        editor
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("selective-absolute", cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            let id = e.selected.unwrap();
            let NodeKind::Adjust(mut adjustment) = e.editor.doc.node(id).unwrap().kind.clone()
            else {
                panic!("Selective Color layer");
            };
            assert!(matches!(
                adjustment,
                Adjustment::SelectiveColor {
                    relative: false,
                    ..
                }
            ));
            adjustment.set_param("reds_cyan", 23.0);
            adjustment.set_param("blacks_black", -41.0);
            e.execute(
                Command::SetAdjustment {
                    id,
                    adjustment: adjustment.clone(),
                },
                cx,
            );
            for (range, keys) in SELECTIVE_COLOR_KEYS.iter().enumerate() {
                e.adjust_ui.selective_range = range;
                let params = e.adjust_visible_params(&adjustment);
                assert_eq!(params.len(), 4);
                assert_eq!(
                    params.iter().map(|param| param.key).collect::<Vec<_>>(),
                    keys.to_vec()
                );
            }
            e.adjust_ui.selective_range = 0;
            cx.notify();
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("selective-saturation-check", cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            let id = e.selected.unwrap();
            assert_eq!(
                e.editor.doc.node(id).unwrap().kind,
                NodeKind::Adjust(Adjustment::selective_color_saturation_check())
            );
            e.undo(cx);
            let NodeKind::Adjust(Adjustment::SelectiveColor { colors, relative }) =
                &e.editor.doc.node(id).unwrap().kind
            else {
                panic!("Selective Color")
            };
            assert!(!relative);
            assert_eq!(colors[0][0], 23.0);
            assert_eq!(colors[8][3], -41.0);
        });
    });
}
