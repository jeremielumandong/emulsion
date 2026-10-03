use super::*;
use ::core::prelude::v1::test;
use emulsion_core::project::ProjectEditor;
use gpui_kit::test::TestWindowExt;

#[gpui_kit::test]
fn invitation_family_thumbnails_release_on_tab_switch_and_rebuild_on_return(
    cx: &mut TestAppContext,
) {
    let doc = Document::new(400, 300);
    let (workspace, cx) = crate::tests::open(cx, doc.clone());
    cx.simulate_resize(size(px(1200.), px(900.)));
    let (view, design_index) = cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.install_project(
                ProjectEditor::new_project(ProjectKind::Design, doc.clone()).unwrap(),
                "Invitation cache".into(),
                window,
                cx,
            );
            let view = workspace.editor.clone().unwrap();
            let index = workspace.tabs.iter().position(|tab| tab == &view).unwrap();
            (view, index)
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-family-purpose", 1usize), cx));
    cx.run_until_parked();
    let images = cx.update(|window, cx| {
        let thumbnail = window.find(("design-family-thumbnail", 0usize)).bounds();
        let card = window.find(("design-family-family", 0usize)).bounds();
        assert_eq!(thumbnail.size.height, px(174.));
        assert!(
            thumbnail.origin.x >= card.origin.x
                && thumbnail.origin.y >= card.origin.y
                && thumbnail.right() <= card.right()
                && thumbnail.bottom() <= card.bottom(),
            "Invitation thumbnail must fit inside its family card: {thumbnail:?}, {card:?}"
        );
        let v = view.read(cx);
        assert_eq!(v.design_ui.family_previews.len(), 2);
        let images: Vec<_> = v.design_ui.family_previews.values().cloned().collect();
        assert!(
            images
                .iter()
                .any(|image| window.has_image_atlas_entry(image))
        );
        images
    });
    let other_index = cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.install(
                Document::new(320, 240),
                None,
                None,
                None,
                "Other tab".into(),
                window,
                cx,
            );
            workspace.tabs.len() - 1
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let v = view.read(cx);
        assert!(!v.visible);
        assert!(v.design_ui.family_previews.is_empty());
        for image in &images {
            assert!(!window.has_image_atlas_entry(image));
        }
        workspace.update(cx, |workspace, cx| {
            workspace.activate_tab(design_index, window, cx)
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            assert!(v.visible);
            assert_eq!(v.design_ui.family_previews.len(), 2);
            assert_eq!(v.editor.doc, doc);
            // Start the other occasion and immediately hide this editor. The
            // finished worker must not repopulate the suspended display cache.
            v.design_ui.family_category = Some(Category::Birthday);
            v.load_family_previews(cx);
            assert!(v.design_ui.family_previews_loading);
        });
        workspace.update(cx, |workspace, cx| {
            workspace.activate_tab(other_index, window, cx)
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let v = view.read(cx);
        assert!(!v.visible);
        assert!(!v.design_ui.family_previews_loading);
        assert!(v.design_ui.family_previews.is_empty());
        assert_eq!(v.editor.doc, doc);
        assert!(!v.editor.can_undo());
        workspace.update(cx, |workspace, cx| {
            workspace.activate_tab(design_index, window, cx)
        });
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let v = view.read(cx);
        assert_eq!(v.design_ui.family_previews.len(), 2);
        assert!(
            v.design_ui
                .family_previews
                .contains_key(&FamilyId::ConfettiClub)
        );
        assert!(
            v.design_ui
                .family_previews
                .contains_key(&FamilyId::MidnightToast)
        );
        assert_eq!(v.editor.doc, doc);
    });
}

#[gpui_kit::test]
fn family_browser_thumbnails_preserve_square_portrait_and_wide_formats(cx: &mut TestAppContext) {
    let doc = Document::new(400, 300);
    let (workspace, cx) = crate::tests::open(cx, doc.clone());
    cx.simulate_resize(size(px(1200.), px(900.)));
    let view = cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.install_project(
                ProjectEditor::new_project(ProjectKind::Design, doc).unwrap(),
                "Family formats".into(),
                window,
                cx,
            );
            workspace.editor.clone().unwrap()
        })
    });
    cx.run_until_parked();
    for (index, category) in [Category::Social, Category::Posters, Category::Presentations]
        .into_iter()
        .enumerate()
    {
        cx.update(|window, cx| window.click(("design-family-purpose", index + 3), cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let v = view.read(cx);
            for family in template_families::families(category) {
                let size = v.design_ui.family_previews.get(&family.id).unwrap().size(0);
                let (width, height) = family.native_size();
                let actual_ratio = f64::from(size.width.0) / f64::from(size.height.0);
                let expected_ratio = f64::from(width) / f64::from(height);
                assert!(
                    (actual_ratio - expected_ratio).abs() < 0.01,
                    "{} thumbnail was stretched",
                    family.label
                );
            }
            assert_eq!(v.editor.page_list().len(), 1);
            assert!(!v.editor.can_undo());
        });
    }
}
