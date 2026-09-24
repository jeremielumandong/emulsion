//! Photoshop's default shortcuts reach the matching Emulsion features.
use super::*;
use crate::editor::{EditorView, Tool};
use emulsion_core::NodeId;
use emulsion_raster::blend::BlendMode;

fn setup(cx: &mut TestAppContext) -> (Entity<EditorView>, Vec<NodeId>, &mut VisualTestContext) {
    let (ws, cx) = open(cx, doc(&["A", "B", "C"], None));
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    let ids = cx.update(|_, cx| e.read(cx).editor.doc.nodes.iter().map(|n| n.id).collect());
    cx.update(|window, cx| {
        e.update(cx, |e, cx| {
            e.set_tool(Tool::Move, cx);
            window.focus(&e.canvas_focus, cx);
        })
    });
    cx.run_until_parked();
    (e, ids, cx)
}

fn select(e: &Entity<EditorView>, id: NodeId, cx: &mut VisualTestContext) {
    cx.update(|_, cx| e.update(cx, |e, cx| e.select_layer_row(id, false, false, cx)));
    cx.run_until_parked();
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

#[gpui_kit::test]
fn layer_shortcuts_select_reorder_clip_and_blend(cx: &mut TestAppContext) {
    let (e, ids, cx) = setup(cx);
    let (a, b, c) = (ids[0], ids[1], ids[2]);
    let selected = |cx: &mut VisualTestContext| cx.update(|_, cx| e.read(cx).selected);

    select(&e, b, cx);
    press("alt-]", cx);
    assert_eq!(selected(cx), Some(c));
    press("alt-[ alt-[", cx);
    assert_eq!(selected(cx), Some(a));
    press("alt-.", cx);
    assert_eq!(selected(cx), Some(c));

    // Bring to Front and Send to Back are one undo step each.
    select(&e, a, cx);
    press("ctrl-shift-]", cx);
    assert_eq!(
        cx.update(|_, cx| e.read(cx).editor.doc.children(None)),
        vec![b, c, a]
    );
    press("ctrl-shift-[", cx);
    assert_eq!(
        cx.update(|_, cx| e.read(cx).editor.doc.children(None)),
        vec![a, b, c]
    );
    press("ctrl-z", cx);
    assert_eq!(
        cx.update(|_, cx| e.read(cx).editor.doc.children(None)),
        vec![b, c, a]
    );
    press("ctrl-z", cx);
    assert_eq!(
        cx.update(|_, cx| e.read(cx).editor.doc.children(None)),
        vec![a, b, c]
    );

    // Ctrl+Alt+G clips to the layer below, and again releases.
    select(&e, b, cx);
    let clip = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| e.read(cx).editor.doc.node(b).unwrap().clip_to)
    };
    press("ctrl-alt-g", cx);
    assert_eq!(clip(cx), Some(a));
    press("ctrl-alt-g", cx);
    assert_eq!(clip(cx), None);

    // Shift+Alt+letter blend modes and number-key opacity.
    press("alt-shift-m", cx);
    assert_eq!(
        cx.update(|_, cx| e.read(cx).editor.doc.node(b).unwrap().blend),
        BlendMode::Multiply
    );
    press("alt-shift-n", cx);
    assert_eq!(
        cx.update(|_, cx| e.read(cx).editor.doc.node(b).unwrap().blend),
        BlendMode::Normal
    );
    press("5", cx);
    assert_eq!(
        cx.update(|_, cx| e.read(cx).editor.doc.node(b).unwrap().opacity),
        0.5
    );

    // With a painting tool the number keys set the tool's opacity instead,
    // and Shift+[ / Shift+] step hardness by a quarter.
    press("b 3", cx);
    cx.update(|_, cx| {
        let e = e.read(cx);
        assert_eq!(e.tools.brush.opacity, 0.3);
        assert_eq!(e.editor.doc.node(b).unwrap().opacity, 0.5);
    });
    cx.update(|_, cx| e.update(cx, |e, _| e.tools.brush.hardness = 0.6));
    press("shift-]", cx);
    assert_eq!(cx.update(|_, cx| e.read(cx).tools.brush.hardness), 0.75);
    press("shift-[ shift-[", cx);
    assert_eq!(cx.update(|_, cx| e.read(cx).tools.brush.hardness), 0.25);
}

#[gpui_kit::test]
fn selection_and_adjustment_shortcuts_match_photoshop(cx: &mut TestAppContext) {
    let (e, ids, cx) = setup(cx);
    select(&e, ids[2], cx);
    let has_selection =
        |cx: &mut VisualTestContext| cx.update(|_, cx| e.read(cx).editor.doc.selection.is_some());
    press("ctrl-a", cx);
    assert!(has_selection(cx));
    press("ctrl-d", cx);
    assert!(!has_selection(cx));
    press("ctrl-shift-d", cx);
    assert!(has_selection(cx), "Reselect restores the last selection");

    // Ctrl+L adds an editable Levels layer; Ctrl+Shift+U a desaturating
    // Hue/Saturation layer.
    let count = |cx: &mut VisualTestContext| cx.update(|_, cx| e.read(cx).editor.doc.nodes.len());
    let before = count(cx);
    press("ctrl-l", cx);
    assert_eq!(count(cx), before + 1);
    press("ctrl-shift-u", cx);
    assert_eq!(count(cx), before + 2);
    cx.update(|_, cx| {
        let doc = &e.read(cx).editor.doc;
        let kinds: Vec<_> = doc
            .nodes
            .iter()
            .filter_map(|n| match &n.kind {
                emulsion_core::NodeKind::Adjust(a) => Some(a.key()),
                _ => None,
            })
            .collect();
        assert_eq!(kinds, ["levels", "hue_saturation"]);
    });
}
