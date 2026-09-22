use super::*;
use emulsion_core::NodeKind;
use gpui_kit::test::TestWindowExt;

#[gpui_kit::test]
fn layer_effects_preserve_editable_text_and_paths(cx: &mut TestAppContext) {
    let mut original = Document::new(64, 64);
    for node in [
        Node::text(
            0,
            "Text",
            emulsion_core::text::TextSpec {
                text: "A".into(),
                ..Default::default()
            },
            64,
            64,
        ),
        Node::path(
            0,
            "Path",
            Arc::new(emulsion_raster::vector::Path::from_svg("M 12 12 L 44 12 L 12 28 Z").unwrap()),
            Default::default(),
            64,
            64,
        ),
    ] {
        Command::AddNode {
            node: Box::new(node),
            slot: Slot::TOP,
        }
        .apply(&mut original)
        .unwrap();
    }
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1000.), gpui_kit::px(1200.)));
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    for node in &original.nodes {
        cx.update(|window, cx| {
            editor.update(cx, |e, cx| e.open_layer_styles_dialog(node.id, window, cx))
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
        });
        let point = cx.update(|window, _| window.find(("style-kind", 0usize)).bounds().center());
        cx.simulate_click(point, Default::default());
        cx.run_until_parked();
        let ok = cx.update(|window, _| window.find("style-dialog-ok").bounds().center());
        cx.simulate_click(ok, Default::default());
        cx.run_until_parked();
        cx.update(|_, cx| {
            let current = editor.read(cx).editor.doc.node(node.id).unwrap();
            assert_eq!(current.styles.len(), 1);
            match (&current.kind, &node.kind) {
                (NodeKind::Text { spec: after, .. }, NodeKind::Text { spec: before, .. }) => {
                    assert_eq!(after, before)
                }
                (NodeKind::Path { path: after, .. }, NodeKind::Path { path: before, .. }) => {
                    assert_eq!(after, before)
                }
                _ => panic!("effects must preserve editable layer content"),
            }
            editor.update(cx, |e, cx| e.undo(cx));
            assert_eq!(editor.read(cx).editor.doc, original);
        });
    }
}

#[gpui_kit::test]
fn advanced_blending_channels_and_ranges_are_undoable(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let id = original.nodes[0].id;
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1000.), gpui_kit::px(1200.)));
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| editor.update(cx, |e, cx| e.open_layer_styles_dialog(id, window, cx)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
    });
    let blend_if = cx.update(|window, _| window.find("blend-if-toggle").bounds().center());
    cx.simulate_click(blend_if, Default::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .find(("blend-if-gradient", false))
                .bounds()
                .size
                .width
                > px(0.)
        );
        assert!(window.find("blend-if-handle-true-3").bounds().size.height > px(0.));
    });
    let point = cx.update(|window, _| window.find(("blend-channel", 0usize)).bounds().center());
    cx.simulate_click(point, Default::default());
    cx.run_until_parked();
    let ok = cx.update(|window, _| window.find("style-dialog-ok").bounds().center());
    cx.simulate_click(ok, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(
            editor
                .read(cx)
                .editor
                .doc
                .node(id)
                .unwrap()
                .blending
                .channels,
            [false, true, true]
        );
        editor.update(cx, |e, cx| {
            e.undo(cx);
            e.set_blend_range(id, false, 0, 0.25, cx);
        });
        let range = editor
            .read(cx)
            .editor
            .doc
            .node(id)
            .unwrap()
            .blending
            .blend_if
            .source;
        assert_eq!(range.black, 0.25);
        assert_eq!(range.black_fade, 0.25);
        editor.update(cx, |e, cx| {
            e.undo(cx);
            e.set_blending(id, |options| options.fill_opacity = 0.3, cx);
        });
        assert_eq!(
            editor
                .read(cx)
                .editor
                .doc
                .node(id)
                .unwrap()
                .blending
                .fill_opacity,
            0.3
        );
        editor.update(cx, |e, cx| e.undo(cx));
        assert_eq!(editor.read(cx).editor.doc, original);
    });
}

#[gpui_kit::test]
fn layer_style_dialog_exposes_knockout_and_advanced_blending(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let id = original.nodes[0].id;
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1000.), gpui_kit::px(1200.)));
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| editor.update(cx, |e, cx| e.open_layer_styles_dialog(id, window, cx)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
    });
    for target in [
        ("blend-knockout", 1usize),
        ("advanced-blend-toggle", 0usize),
        ("advanced-blend-toggle", 3usize),
    ] {
        let point = cx.update(|window, _| window.find(target).bounds().center());
        cx.simulate_click(point, Default::default());
        cx.run_until_parked();
    }
    let ok = cx.update(|window, _| window.find("style-dialog-ok").bounds().center());
    cx.simulate_click(ok, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let blending = editor.read(cx).editor.doc.node(id).unwrap().blending;
        assert_eq!(
            blending.knockout,
            emulsion_raster::composite::Knockout::Shallow
        );
        assert!(!blending.blend_interior_effects_as_group);
        assert!(blending.layer_mask_hides_effects);
        editor.update(cx, |e, cx| e.undo(cx));
        assert_eq!(editor.read(cx).editor.doc, original);
    });
}
