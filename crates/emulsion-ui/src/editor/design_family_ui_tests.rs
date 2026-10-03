//! Families share one isolated chooser, but their pages are purposeful sets.
use super::tests::open_design;
use super::*;
use ::core::prelude::v1::test;
use gpui_kit::test::TestWindowExt;

const NEW_CATEGORIES: [Category; 3] =
    [Category::Social, Category::Posters, Category::Presentations];

#[gpui_kit::test]
fn family_categories_choices_and_cancel_preserve_the_canvas(cx: &mut TestAppContext) {
    let (view, cx) = open_design(cx);
    let before = cx.update(|_, cx| view.read(cx).editor.doc.clone());
    for (index, category) in NEW_CATEGORIES.into_iter().enumerate() {
        cx.update(|window, cx| window.click(("design-family-purpose", index + 3), cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            for index in 0..2usize {
                assert!(window.try_find(("design-family-family", index)).is_some());
            }
            assert!(window.try_find(("design-family-family", 2usize)).is_none());
            window.click(("design-family-family", 0usize), cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            for index in 0..3usize {
                assert!(window.find(("design-family-layout", index)).visible());
                assert!(window.find(("design-family-palette", index)).visible());
            }
            let is_set = category != Category::Posters;
            assert_eq!(window.try_find("design-template-add-all").is_some(), is_set);
            assert_eq!(window.try_find("design-template-next").is_some(), is_set);
            window.click(("design-family-layout", 2usize), cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click(("design-family-palette", 1usize), cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(view.read(cx).editor.doc, before);
            assert!(!view.read(cx).editor.can_undo());
            window.click("design-template-cancel", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.try_find("design-template-dialog").is_none());
            assert_eq!(view.read(cx).editor.doc, before);
            assert_eq!(view.read(cx).editor.page_list().len(), 1);
        });
    }
}

#[gpui_kit::test]
fn family_sets_preserve_formats_and_editability_through_undo_and_save(cx: &mut TestAppContext) {
    let (view, cx) = open_design(cx);
    let before = cx.update(|_, cx| view.read(cx).editor.doc.clone());
    let dir = tempfile::tempdir().unwrap();
    for family in template_families::FAMILIES
        .iter()
        .filter(|f| NEW_CATEGORIES.contains(&f.occasion))
    {
        let selection = Selection {
            variant: family.variants[2].id,
            palette: 2,
            ..family.selection()
        };
        let expected = selection.create().unwrap();
        cx.update(|window, cx| {
            view.update(cx, |v, cx| v.preview_template_family(selection, window, cx));
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.click(
                if expected.pages.len() == 1 {
                    "design-template-add"
                } else {
                    "design-template-add-all"
                },
                cx,
            );
        });
        cx.run_until_parked();
        let project = cx.update(|window, cx| {
            assert!(window.try_find("design-template-dialog").is_none());
            view.update(cx, |v, cx| {
                // Only the selected layout and its semantic companion pages are inserted.
                assert_eq!(v.editor.page_list().len(), 1 + family.page_labels().len());
                assert_eq!(v.editor.page(1).unwrap().doc, before);
                for (meta, expected) in v.editor.page_list().iter().skip(1).zip(&expected.pages) {
                    let actual = &v.editor.page(meta.id).unwrap().doc;
                    assert_eq!(actual, &expected.doc);
                    assert_eq!((actual.width, actual.height), family.native_size());
                    assert_eq!(meta.name, expected.meta.name);
                    assert!(
                        actual
                            .nodes
                            .iter()
                            .any(|node| matches!(node.kind, NodeKind::Text { .. }))
                    );
                }
                v.undo(cx);
                assert_eq!(v.editor.page_list().len(), 1);
                assert_eq!(v.editor.doc, before);
                v.redo(cx);
                assert_eq!(v.editor.page_list().len(), expected.pages.len() + 1);
                v.editor.snapshot().unwrap()
            })
        });
        let path = dir.path().join(format!("{}.emu", family.label));
        emulsion_io::project::write(&project, &path).unwrap();
        let reopened = emulsion_io::project::read(&path).unwrap();
        assert_eq!(reopened.pages.len(), project.pages.len());
        for (saved, original) in reopened.pages.iter().zip(&project.pages) {
            assert_eq!(saved.meta, original.meta);
            assert_eq!(saved.doc, original.doc);
        }
        cx.update(|_, cx| view.update(cx, |v, cx| v.undo(cx)));
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn family_loading_uses_the_selected_format_and_latest_choices(cx: &mut TestAppContext) {
    let (view, cx) = open_design(cx);
    cx.simulate_resize(size(px(900.), px(700.)));
    for category in NEW_CATEGORIES {
        let family = template_families::families(category).next().unwrap();
        let initial = family.selection();
        let final_selection = Selection {
            variant: family.variants[2].id,
            palette: 2,
            ..initial
        };
        let preview = cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                v.open_template_preview(Source::Family(initial), window, cx)
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            let controls = [
                "design-template-large-preview",
                "design-template-cancel",
                "design-template-add",
            ]
            .map(|id| (id, window.find(id).bounds()));
            preview.update(cx, |p, cx| {
                p.choose_family_selection(
                    Selection {
                        variant: family.variants[1].id,
                        ..initial
                    },
                    cx,
                );
                p.choose_family_selection(final_selection, cx);
                assert!(!p.ready_to_apply());
                assert!(p.project.is_none());
                assert_eq!(p.index, 0);
            });
            window.render_frame(cx);
            for (id, old_bounds) in controls {
                let bounds = window.find(id).bounds();
                assert_eq!(bounds, old_bounds, "{category:?}: {id} moved while loading");
                assert!(bounds.origin.x >= px(0.) && bounds.right() <= px(900.));
                assert!(bounds.origin.y >= px(0.) && bounds.bottom() <= px(700.));
            }
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            let p = preview.read(cx);
            assert_eq!(p.family_selection, Some(final_selection));
            assert!(p.ready_to_apply());
            let project = p.project.as_ref().unwrap();
            assert_eq!(project.pages.len(), family.page_labels().len());
            let expected = final_selection.create().unwrap();
            for (actual, expected) in project.pages.iter().zip(&expected.pages) {
                assert_eq!(actual.doc, expected.doc);
            }
            window.click("design-template-cancel", cx);
        });
        cx.run_until_parked();
    }
}
