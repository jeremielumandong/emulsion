//! Short-window previews keep their actions and scope visible outside the body.
use super::tests::open_design;
use super::*;
use ::core::prelude::v1::test;
use emulsion_core::design::template_families::FamilyId;
use gpui_kit::{Bounds, Pixels, Size, test::TestWindowExt};

fn assert_inside(bounds: Bounds<Pixels>, container: Bounds<Pixels>, id: &str) {
    assert!(
        bounds.left() >= container.left() && bounds.right() <= container.right(),
        "{id} clipped horizontally: {bounds:?}, container {container:?}"
    );
    assert!(
        bounds.top() >= container.top() && bounds.bottom() <= container.bottom(),
        "{id} clipped vertically: {bounds:?}, container {container:?}"
    );
}

fn assert_footer(window: &mut Window, viewport: Size<Pixels>, is_set: bool) -> Bounds<Pixels> {
    let footer = window.find("design-template-footer");
    assert!(footer.visible());
    let footer = footer.bounds();
    assert_inside(
        footer,
        Bounds::new(point(px(0.), px(0.)), viewport),
        "footer",
    );
    for id in [
        "design-template-cancel",
        "design-template-add",
        "design-template-replace",
        "design-template-replace-note",
    ] {
        let element = window.find(id);
        assert!(
            element.visible(),
            "{id} must remain visible at {viewport:?}"
        );
        assert_inside(element.bounds(), footer, id);
    }
    assert_eq!(window.try_find("design-template-add-all").is_some(), is_set);
    if is_set {
        let add_all = window.find("design-template-add-all");
        assert!(add_all.visible());
        assert_inside(add_all.bounds(), footer, "add-all");
    }
    footer
}

fn assert_initial_preview(window: &mut Window, viewport: Size<Pixels>, footer: Bounds<Pixels>) {
    let preview = window.find("design-template-large-preview");
    assert!(preview.visible());
    let bounds = preview.bounds();
    assert!(
        bounds.size.height >= px(120.),
        "preview too small: {bounds:?}"
    );
    assert_inside(
        bounds,
        Bounds::new(point(px(0.), px(0.)), viewport),
        "preview",
    );
    assert!(
        bounds.bottom() <= footer.top(),
        "preview overlaps pinned actions"
    );
}

#[gpui_kit::test]
fn template_family_footer_stays_pinned_at_short_and_narrow_sizes(cx: &mut TestAppContext) {
    let (view, cx) = open_design(cx);
    let before = cx.update(|_, cx| view.read(cx).editor.doc.clone());
    for (index, viewport) in [
        size(px(770.), px(560.)),
        size(px(610.), px(510.)),
        size(px(480.), px(500.)),
    ]
    .into_iter()
    .enumerate()
    {
        cx.simulate_resize(viewport);
        let preview = cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                v.open_template_preview(
                    Source::Family(Selection::for_family(FamilyId::GardenVows)),
                    window,
                    cx,
                )
            })
        });
        cx.run_until_parked();
        let footer = cx.update(|window, cx| {
            let footer = assert_footer(window, viewport, true);
            assert_initial_preview(window, viewport, footer);
            let (project, image) = preview.update(cx, |p, cx| {
                p.loading = true;
                let previous = (p.project.take(), p.image.take());
                cx.notify();
                previous
            });
            // Hold the loading frame so background rendering cannot hide a
            // missed footer subscription or layout shift before the assertion.
            window.render_frame(cx);
            window.render_frame(cx);
            assert_eq!(assert_footer(window, viewport, true), footer);
            assert!(!preview.read(cx).ready_to_apply());
            for id in [
                "design-template-add",
                "design-template-replace",
                "design-template-add-all",
            ] {
                // Buttons do not expose disabled accessibility metadata here.
                // Exercise native input and verify that no scope can apply.
                window.click(id, cx);
                assert!(window.try_find("design-template-dialog").is_some());
                assert!(!preview.read(cx).applied, "{id} applied while loading");
                assert_eq!(view.read(cx).editor.doc, before, "{id}");
                assert_eq!(view.read(cx).editor.page_list().len(), 1, "{id}");
                assert!(!view.read(cx).editor.can_undo(), "{id}");
            }
            preview.update(cx, |p, cx| {
                p.project = project;
                p.image = image;
                p.loading = false;
                cx.notify();
            });
            window.render_frame(cx);
            window.render_frame(cx);
            assert!(preview.read(cx).ready_to_apply());
            window.scroll(
                "design-template-large-preview",
                ScrollDelta::Pixels(point(px(0.), px(-2000.))),
                cx,
            );
            footer
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(assert_footer(window, viewport, true), footer);
            for id in ["design-template-previous", "design-template-next"] {
                let pager = window.find(id);
                assert!(pager.visible(), "{id} must be reachable by scrolling");
                assert!(pager.bounds().bottom() <= footer.top());
                assert_inside(
                    pager.bounds(),
                    Bounds::new(point(px(0.), px(0.)), viewport),
                    id,
                );
            }
            assert_eq!(view.read(cx).editor.doc, before);
            assert!(!view.read(cx).editor.can_undo());
            // This must become clickable again after the held loading frame,
            // proving that the footer observes the preview's readiness change.
            window.click("design-template-add", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.try_find("design-template-dialog").is_none());
            assert!(preview.read(cx).applied);
            view.update(cx, |v, cx| {
                assert_eq!(v.editor.page_list().len(), 2);
                assert_eq!(v.editor.page(1).unwrap().doc, before);
                v.undo(cx);
                assert_eq!(v.editor.doc, before);
                assert_eq!(v.editor.page_list().len(), 1);
                assert!(!v.editor.can_undo());
                v.open_template_preview(
                    Source::Family(Selection::for_family(FamilyId::GardenVows)),
                    window,
                    cx,
                );
            });
            // Cancellation must also remain available before rendering finishes.
            if index % 2 == 0 {
                window.click("design-template-cancel", cx);
            }
        });
        if index % 2 != 0 {
            cx.simulate_keystrokes("escape");
        }
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.try_find("design-template-dialog").is_none());
            assert!(window.try_find("design-template-footer").is_none());
            assert_eq!(view.read(cx).editor.doc, before);
            assert_eq!(view.read(cx).editor.page_list().len(), 1);
        });
    }
}

#[gpui_kit::test]
fn template_builtin_and_local_footers_fit_short_windows(cx: &mut TestAppContext) {
    let (view, cx) = open_design(cx);
    let mut local =
        ProjectEditor::new_project(ProjectKind::Design, Document::new(260, 180)).unwrap();
    local
        .add_page(
            Template::Announcement.create(320, 240).unwrap(),
            "Second design".into(),
            2.,
        )
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("local.emu");
    emulsion_io::project::write(&local.snapshot().unwrap(), &path).unwrap();
    for viewport in [size(px(610.), px(510.)), size(px(480.), px(500.))] {
        cx.simulate_resize(viewport);
        for (source, is_set) in [
            (Source::Builtin(Template::Announcement, (240, 320)), false),
            (Source::Local(path.clone()), true),
        ] {
            let (before, preview) = cx.update(|window, cx| {
                view.update(cx, |v, cx| {
                    let preview = v.open_template_preview(source.clone(), window, cx);
                    (v.editor.doc.clone(), preview)
                })
            });
            cx.run_until_parked();
            let expected = cx.update(|window, cx| {
                let footer = assert_footer(window, viewport, is_set);
                assert_initial_preview(window, viewport, footer);
                let p = preview.read(cx);
                assert!(p.ready_to_apply());
                let expected = p.project.as_ref().unwrap().pages[0].doc.clone();
                window.click("design-template-add", cx);
                expected
            });
            cx.run_until_parked();
            cx.update(|window, cx| {
                assert!(window.try_find("design-template-dialog").is_none());
                assert!(preview.read(cx).applied);
                view.update(cx, |v, cx| {
                    assert_eq!(v.editor.page_list().len(), 2);
                    assert_eq!(v.editor.doc, expected);
                    assert_eq!(v.editor.page(1).unwrap().doc, before);
                    v.undo(cx);
                    assert_eq!(v.editor.doc, before);
                    assert!(!v.editor.can_undo());
                    v.open_template_preview(source, window, cx);
                });
            });
            cx.run_until_parked();
            cx.update(|window, cx| window.click("design-template-cancel", cx));
            cx.run_until_parked();
            cx.update(|window, cx| {
                assert!(window.try_find("design-template-dialog").is_none());
                assert_eq!(view.read(cx).editor.doc, before);
                assert_eq!(view.read(cx).editor.page_list().len(), 1);
                assert!(!view.read(cx).editor.can_undo());
            });
        }
    }
}

#[gpui_kit::test]
fn template_pinned_footer_applies_only_the_requested_scope(cx: &mut TestAppContext) {
    let (view, cx) = open_design(cx);
    let selection = Selection::for_family(FamilyId::GardenVows);
    let expected = selection.create().unwrap();
    for viewport in [size(px(610.), px(510.)), size(px(480.), px(500.))] {
        cx.simulate_resize(viewport);
        for (action, pages) in [
            ("design-template-add", 2),
            ("design-template-replace", 1),
            ("design-template-add-all", 1 + expected.pages.len()),
        ] {
            let before = cx.update(|window, cx| {
                view.update(cx, |v, cx| {
                    v.open_template_preview(Source::Family(selection), window, cx);
                    v.editor.doc.clone()
                })
            });
            cx.run_until_parked();
            cx.update(|window, cx| {
                assert_footer(window, viewport, true);
                window.click(action, cx);
            });
            cx.run_until_parked();
            cx.update(|window, cx| {
                assert!(window.try_find("design-template-dialog").is_none());
                view.update(cx, |v, cx| {
                    assert_eq!(v.editor.page_list().len(), pages, "{action}");
                    assert_eq!(v.editor.doc, expected.pages[0].doc, "{action}");
                    if action != "design-template-replace" {
                        assert_eq!(v.editor.page(1).unwrap().doc, before, "{action}");
                    }
                    v.undo(cx);
                    assert_eq!(v.editor.page_list().len(), 1);
                    assert_eq!(v.editor.doc, before);
                    assert!(!v.editor.can_undo());
                });
            });
        }
    }
}
