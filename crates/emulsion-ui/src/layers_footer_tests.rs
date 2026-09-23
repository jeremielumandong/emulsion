use super::*;

#[gpui_kit::test]
fn footer_buttons_add_group_and_delete_layers(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt;
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.run_until_parked();
    cx.update(|window, cx| window.click("layers-new", cx));
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc.nodes.len(), 2));
    cx.update(|window, cx| window.click("layers-group", cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.editor.doc.nodes.len(), 3);
        let group = e.editor.doc.node(e.selected.unwrap()).unwrap();
        assert!(group.is_group());
        assert!(e.editor.doc.children(Some(group.id)).is_empty());
    });
    cx.update(|window, cx| window.click("layers-delete", cx));
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc.nodes.len(), 2));
}

#[gpui_kit::test]
fn fx_entry_adds_effect_inside_cancellable_dialog(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let id = original.nodes[0].id;
    let (ws, cx) = open(cx, original.clone());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| {
        editor.update(cx, |e, cx| {
            e.open_layer_effect_kind(id, "drop_shadow", window, cx)
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let e = editor.read(cx);
        assert_eq!(e.styles_ui.dialog_for, Some(id));
        let styles = &e.editor.doc.node(id).unwrap().styles;
        assert_eq!(styles.len(), 1);
        assert_eq!(styles[0].key(), "drop_shadow");
        assert_eq!(e.styles_ui.expanded, Some((id, 0)));
        // Picking the same effect again selects it rather than adding a copy.
        editor.update(cx, |e, cx| {
            e.open_layer_effect_kind(id, "drop_shadow", window, cx)
        });
        assert_eq!(editor.read(cx).editor.doc.node(id).unwrap().styles.len(), 1);
        editor.update(cx, |e, cx| e.close_style_dialog(false, cx));
        assert!(
            editor
                .read(cx)
                .editor
                .doc
                .node(id)
                .unwrap()
                .styles
                .is_empty()
        );
    });
}
