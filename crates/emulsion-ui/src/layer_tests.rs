//! Adding layers; included as a child of the shared headless test module.
use super::*;
use crate::editor::EditorView;
use emulsion_core::NodeKind;

fn editor(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    cx.update(|_, cx| ws.read(cx).editor.clone().unwrap())
}

fn is_transparent_canvas_layer(node: &Node, w: u32, h: u32) -> bool {
    match &node.kind {
        NodeKind::Raster { raster, .. } => {
            raster.width() == w
                && raster.height() == h
                && (0..h).all(|y| (0..w).all(|x| raster.get(x, y)[3] == 0))
        }
        _ => false,
    }
}

#[gpui_kit::test]
fn ctrl_shift_n_adds_an_empty_transparent_layer_above_the_selection(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Bottom", "Top"], None));
    let e = editor(&ws, cx);
    let bottom = cx.update(|window, cx| {
        e.update(cx, |e, cx| {
            let bottom = e.editor.doc.nodes[0].id;
            e.selected = Some(bottom);
            window.focus(&e.canvas_focus, cx);
            bottom
        })
    });
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-shift-n"
    } else {
        "ctrl-shift-n"
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = e.read(cx);
        assert_eq!(e.editor.history.len(), 1);
        assert_eq!(e.editor.doc.nodes.len(), 3);
        let new = e.selected.expect("the new layer is selected");
        let node = e.editor.doc.node(new).unwrap();
        assert_eq!(node.name, "Layer 3");
        assert!(is_transparent_canvas_layer(node, 256, 192));
        // Just above the layer that was selected, not on top of the stack.
        let order = e.editor.doc.children(None);
        let b = order.iter().position(|i| *i == bottom).unwrap();
        assert_eq!(order[b + 1], new);
    });
}

#[gpui_kit::test]
fn a_new_transparent_canvas_has_one_empty_layer(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.update(|window, cx| ws.update(cx, |ws, cx| ws.new_document_with(None, window, cx)));
    cx.run_until_parked();
    let e = editor(&ws, cx);
    cx.update(|_, cx| {
        let e = e.read(cx);
        assert_eq!(e.editor.doc.nodes.len(), 1);
        assert!(is_transparent_canvas_layer(
            &e.editor.doc.nodes[0],
            e.editor.doc.width,
            e.editor.doc.height
        ));
    });
}
