use super::*;
use gpui_kit::test::TestWindowExt;

#[gpui_kit::test]
fn layer_effect_rows_toggle_individual_and_master_visibility_without_changing_pixels(
    cx: &mut TestAppContext,
) {
    let mut document = doc(&["Photo"], None);
    let id = document.nodes[0].id;
    let styles = emulsion_core::styles::LayerStyle::catalogue()[..2].to_vec();
    Command::SetLayerEffects {
        id,
        styles,
        options: vec![
            emulsion_core::style_options::StyleOptions {
                id: 7,
                ..Default::default()
            },
            emulsion_core::style_options::StyleOptions {
                id: 9,
                enabled: false,
                ..Default::default()
            },
        ],
    }
    .apply(&mut document)
    .unwrap();
    let original = document.clone();
    let (ws, cx) = open(cx, document);
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1200.), gpui_kit::px(1000.)));
    cx.run_until_parked();
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    let history = cx.update(|_, cx| editor.read(cx).editor.history.len());
    let position = cx.update(|window, _| {
        window
            .find(gpui_kit::SharedString::from(format!(
                "effect-visible-{id}-7"
            )))
            .bounds()
            .center()
    });
    cx.simulate_click(position, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        let node = e.editor.doc.node(id).unwrap();
        assert!(!node.style_options[0].enabled);
        assert!(!node.style_options[1].enabled);
        assert_eq!(node.kind, original.node(id).unwrap().kind);
        assert_eq!(e.editor.history.len(), history + 1);
        editor.update(cx, |e, cx| e.undo(cx));
    });
    cx.run_until_parked();
    let position = cx.update(|window, _| window.find(("effects-visible", id)).bounds().center());
    cx.simulate_click(position, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let node = editor.read(cx).editor.doc.node(id).unwrap();
        assert!(!node.effects_enabled);
        assert!(node.style_options[0].enabled);
        assert!(!node.style_options[1].enabled);
    });
    cx.simulate_click(position, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let node = editor.read(cx).editor.doc.node(id).unwrap();
        assert!(node.effects_enabled);
        assert_eq!(node.style_options, original.node(id).unwrap().style_options);
    });
    let before = cx.update(|_, cx| editor.read(cx).editor.history.len());
    let position = cx.update(|window, _| window.find(("effects-expand", id)).bounds().center());
    cx.simulate_click(position, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert!(editor.read(cx).layer_panel.effects_collapsed.contains(&id));
        assert_eq!(editor.read(cx).editor.history.len(), before);
    });
}

#[gpui_kit::test]
fn layer_style_modal_blocks_background_commands_and_tab_switch_cancels_preview(
    cx: &mut TestAppContext,
) {
    let original = doc(&["Original"], None);
    let id = original.nodes[0].id;
    let (ws, cx) = open(cx, original.clone());
    let source = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| {
        ws.update(cx, |w, cx| {
            w.install(
                Document::new(20, 20),
                None,
                None,
                None,
                "Target".into(),
                window,
                cx,
            )
        });
        ws.update(cx, |w, cx| w.activate_tab(0, window, cx));
        source.update(cx, |e, cx| {
            e.open_layer_styles_dialog(id, window, cx);
            e.execute(Command::SetOpacity { id, opacity: 0.4 }, cx);
            window.focus(&e.canvas_focus, cx);
        });
    });
    cx.run_until_parked();
    cx.dispatch_action(crate::actions::Undo);
    cx.dispatch_action(crate::actions::Redo);
    cx.dispatch_action(crate::actions::DeleteNode);
    cx.dispatch_action(crate::actions::DuplicateNode);
    cx.dispatch_action(crate::actions::NewLayer);
    cx.dispatch_action(crate::actions::PastePixels);
    cx.run_until_parked();
    cx.update(|window, cx| {
        let e = source.read(cx);
        assert_eq!(e.editor.doc.nodes.len(), 1);
        assert_eq!(e.editor.doc.node(id).unwrap().opacity, 0.4);
        assert!(e.editor.in_transaction());
        ws.update(cx, |w, cx| w.activate_tab(1, window, cx));
        let e = source.read(cx);
        assert!(e.styles_ui.dialog_for.is_none());
        assert!(!e.editor.in_transaction());
        assert_eq!(e.editor.doc, original);
        assert!(e.editor.history.is_empty());
    });
}

#[gpui_kit::test]
fn opening_new_document_and_home_cancel_layer_style_preview(cx: &mut TestAppContext) {
    let original = doc(&["Original"], None);
    let id = original.nodes[0].id;
    let (ws, cx) = open(cx, original.clone());
    let source = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| {
        source.update(cx, |e, cx| {
            e.open_layer_styles_dialog(id, window, cx);
            e.execute(Command::SetOpacity { id, opacity: 0.2 }, cx);
        });
        ws.update(cx, |w, cx| w.new_document_with(None, window, cx));
        assert_eq!(source.read(cx).editor.doc, original);
        assert!(source.read(cx).styles_ui.dialog_for.is_none());
        ws.update(cx, |w, cx| w.activate_tab(0, window, cx));
        source.update(cx, |e, cx| {
            e.open_layer_styles_dialog(id, window, cx);
            e.execute(Command::SetOpacity { id, opacity: 0.7 }, cx);
            window.focus(&e.canvas_focus, cx);
        });
    });
    cx.dispatch_action(crate::actions::ShowHome);
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(ws.read(cx).screen, crate::workspace::Screen::Home);
        assert_eq!(source.read(cx).editor.doc, original);
        assert!(!source.read(cx).editor.in_transaction());
    });
}
