use super::*;
use ::core::prelude::v1::test;
use emulsion_core::project::ProjectEditor;
use gpui_kit::test::TestWindowExt;

fn setup(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    let doc = Document::new(600, 400);
    let (workspace, cx) = crate::tests::open(cx, doc.clone());
    // Keep the whole catalog visible in the catalog test; smaller drawers are
    // covered separately, with the same cards in the scroll container.
    cx.simulate_resize(size(px(1400.), px(2400.)));
    let view = cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.install_project(
                ProjectEditor::new_project(ProjectKind::Design, doc).unwrap(),
                "Typography".into(),
                window,
                cx,
            );
        });
        let view = workspace.read(cx).editor.clone().unwrap();
        view.update(cx, |view, cx| {
            view.show_design_section(Section::Text, cx);
        });
        view
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.render_frame(cx));
    (view, cx)
}

fn search(view: &Entity<EditorView>, cx: &mut VisualTestContext, query: &str) {
    cx.update(|window, cx| {
        let input = view.read(cx).design_ui.search.clone().unwrap();
        input.update(cx, |input, cx| input.set_value(query, window, cx));
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.render_frame(cx));
}

#[gpui_kit::test]
fn typography_drawer_shows_ten_cards_and_search_keeps_catalog_indices(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    let pairs = emulsion_core::design::typography_pairs();
    assert_eq!(pairs.len(), 10);
    cx.update(|window, _| {
        let drawer = window.find("design-drawer").bounds();
        let mut previous_bottom = drawer.top();
        for pair in &pairs {
            let card = window.find(("design-type-pair", pair.index));
            assert!(card.visible(), "{}", pair.name);
            let bounds = card.bounds();
            assert!(bounds.left() >= drawer.left() && bounds.right() <= drawer.right());
            assert!(bounds.top() >= previous_bottom, "Cards must not overlap");
            previous_bottom = bounds.bottom();
        }
    });
    let before = cx.update(|_, cx| view.read(cx).editor.doc.clone());
    for query in [
        "  DAILY   dispatch  ",
        "gEiSt",
        "Editorial",
        "no-match-9864",
        "",
    ] {
        search(&view, cx, query);
        cx.update(|window, cx| {
            for pair in &pairs {
                assert_eq!(
                    window.try_find(("design-type-pair", pair.index)).is_some(),
                    pair.matches_query(query),
                    "{} with query {query:?}",
                    pair.name,
                );
            }
            assert_eq!(view.read(cx).editor.doc, before);
        });
    }
}

#[gpui_kit::test]
fn typography_previews_match_native_fragment_rendering(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    let previews = cx.update(|_, cx| {
        let ui = &view.read(cx).design_ui;
        assert!(!ui.pair_loading);
        assert_eq!(
            ui.pair_generation,
            Some(emulsion_core::text::font_generation())
        );
        assert_eq!(ui.pair_previews.len(), 10);
        ui.pair_previews.clone()
    });
    for pair in emulsion_core::design::typography_pairs() {
        let mut editor = emulsion_core::Editor::new(Document::new(640, 280), None);
        emulsion_core::design::typography_pair(&editor.doc, pair.index)
            .unwrap()
            .paste(&mut editor, Slot::TOP, (0., 0.))
            .unwrap();
        let (width, height, expected) = super::super::history::doc_thumb(&editor.doc, 480).unwrap();
        let preview = &previews[&pair.index];
        assert_eq!(preview.size(0).width.0, width as i32);
        assert_eq!(preview.size(0).height.0, height as i32);
        assert_eq!(preview.as_bytes(0).unwrap(), expected);
        assert!(
            expected
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] != 0)
        );
        assert!(
            expected
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] == 0)
        );
    }
    assert_ne!(previews[&0].as_bytes(0), previews[&2].as_bytes(0));
}

#[gpui_kit::test]
fn typography_search_card_inserts_selected_editable_group_with_one_undo(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    let pair = emulsion_core::design::typography_pairs().pop().unwrap();
    assert_ne!(pair.index, 0);
    search(&view, cx, pair.name);
    let original = cx.update(|window, cx| {
        assert!(window.try_find(("design-type-pair", 0usize)).is_none());
        let original = view.read(cx).editor.doc.clone();
        window.click(("design-type-pair", pair.index), cx);
        original
    });
    cx.run_until_parked();
    let inserted = cx.update(|window, cx| {
        let editor = view.read(cx);
        editor.editor.doc.validate().unwrap();
        let group = editor.selected.unwrap();
        assert_eq!(editor.selected_layer_ids(), vec![group]);
        assert_eq!(editor.tool, Tool::Move);
        let node = editor.editor.doc.node(group).unwrap();
        assert!(node.is_group());
        assert_eq!(node.name, format!("{} font combination", pair.name));
        assert_eq!(editor.editor.doc.nodes.len(), original.nodes.len() + 3);
        assert_eq!(editor.editor.doc.children(Some(group)).len(), 2);
        for (name, style, sample) in [
            ("Heading", &pair.heading, pair.heading_sample),
            ("Body", &pair.body, pair.body_sample),
        ] {
            let text = editor
                .editor
                .doc
                .nodes
                .iter()
                .find(|n| n.name == name)
                .unwrap();
            let NodeKind::Text { spec, .. } = &text.kind else {
                panic!("Typography content must remain editable text");
            };
            assert_eq!(spec.font, style.font);
            assert_eq!(spec.text, sample);
        }
        let inserted = editor.editor.doc.clone();
        window.click("design-undo", cx);
        inserted
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc, original);
        window.click("design-redo", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc, inserted);
        window.click(("design-type-pair", pair.index), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let editor = view.read(cx);
        editor.editor.doc.validate().unwrap();
        assert_eq!(editor.editor.doc.nodes.len(), original.nodes.len() + 6);
        window.click("design-undo", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, inserted));
}

#[gpui_kit::test]
fn typography_cards_fit_narrow_drawer_and_reopen_without_losing_search(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    let pair = emulsion_core::design::typography_pairs().remove(2);
    search(&view, cx, pair.name);
    for width in [480., 800., 1099., 1100.] {
        cx.simulate_resize(size(px(width), px(700.)));
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
            let drawer = window.find("design-drawer").bounds();
            let card = window.find(("design-type-pair", pair.index));
            let bounds = card.bounds();
            assert!(card.visible());
            assert!(drawer.left() >= px(0.) && drawer.right() <= px(width));
            assert!(bounds.left() >= drawer.left() && bounds.right() <= drawer.right());
            assert!(bounds.top() >= drawer.top() && bounds.bottom() <= drawer.bottom());
            assert!(view.read(cx).editor.doc.nodes.is_empty());
        });
    }
    cx.update(|window, cx| window.click("design-drawer-close", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("design-drawer").is_none());
        window.click(("design-section", 2usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find(("design-type-pair", 0usize)).is_none());
        assert!(window.find(("design-type-pair", pair.index)).visible());
        window.click(("design-type-pair", pair.index), cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc.nodes.len(), 3));
}

#[gpui_kit::test]
fn typography_preview_atlas_is_released_on_font_refresh_and_document_hide(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    let original = cx.update(|window, cx| {
        let image = view.read(cx).design_ui.pair_previews[&0].clone();
        assert!(window.has_image_atlas_entry(&image));
        image
    });
    emulsion_core::text::refresh_fonts();
    cx.update(|_, cx| view.update(cx, |_, cx| cx.notify()));
    cx.run_until_parked();
    let replacement = cx.update(|window, cx| {
        window.render_frame(cx);
        assert!(!window.has_image_atlas_entry(&original));
        let replacement = view.read(cx).design_ui.pair_previews[&0].clone();
        assert!(!Arc::ptr_eq(&original, &replacement));
        assert!(window.has_image_atlas_entry(&replacement));
        // Hide through the real tab lifecycle while another batch is pending.
        // Its completion must not repopulate the hidden editor's preview map.
        emulsion_core::text::refresh_fonts();
        view.update(cx, |view, cx| {
            view.load_pair_previews(cx);
            assert!(view.design_ui.pair_loading);
        });
        let workspace = view
            .read(cx)
            .library_workspace
            .as_ref()
            .unwrap()
            .upgrade()
            .unwrap();
        workspace.update(cx, |workspace, cx| {
            assert_ne!(workspace.active_tab(), Some(0));
            workspace.activate_tab(0, window, cx);
        });
        assert!(!window.has_image_atlas_entry(&replacement));
        replacement
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let editor = view.read(cx);
        assert!(!editor.visible);
        assert!(!editor.design_ui.pair_loading);
        assert!(editor.design_ui.pair_previews.is_empty());
        assert_eq!(editor.design_ui.pair_generation, None);
        assert!(!window.has_image_atlas_entry(&replacement));
    });
}
