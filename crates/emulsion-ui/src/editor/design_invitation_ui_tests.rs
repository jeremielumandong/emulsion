//! Native invitation selection must stay isolated until an explicit apply action.
use super::tests::open_design;
use super::*;
use ::core::prelude::v1::test;
use emulsion_core::design::invitations::{FamilyId, VariantId};
use gpui_kit::{InputEvent as _, test::TestWindowExt};

#[gpui_kit::test]
fn invitation_purpose_family_layout_palette_and_cancel_keep_document_unchanged(
    cx: &mut TestAppContext,
) {
    let (view, cx) = open_design(cx);
    let (before, selected, ticket, catalog) = cx.update(|_, cx| {
        view.update(cx, |v, _| {
            let selected = v.editor.doc.nodes.last().unwrap().id;
            v.set_layer_selection(vec![selected], Some(selected));
            (
                v.editor.doc.clone(),
                selected,
                v.edit_ticket(),
                serde_json::to_value((&v.creative.catalog.brands, &v.creative.catalog.assets))
                    .unwrap(),
            )
        })
    });
    cx.update(|window, cx| {
        assert!(window.find("design-invitation-purposes").visible());
        assert!(window.find(("design-template", 0usize)).visible());
        window.click(("design-invitation-purpose", 1usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("design-invitation-families").visible());
        assert!(window.find(("design-invitation-family", 0usize)).visible());
        assert!(
            window
                .try_find(("design-invitation-family", 1usize))
                .is_some()
        );
        assert!(
            window
                .try_find(("design-invitation-family", 2usize))
                .is_none()
        );
        assert!(window.try_find(("design-template", 0usize)).is_none());
        window.click(("design-invitation-family", 0usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("design-invitation-choices").visible());
        assert!(window.find("design-template-large-preview").visible());
        for index in 0..3usize {
            assert!(window.find(("design-invitation-layout", index)).visible());
            assert!(window.find(("design-invitation-palette", index)).visible());
        }
        window.click(("design-invitation-layout", 1usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("design-invitation-palette", 2usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let v = view.read(cx);
        assert_eq!(v.editor.doc, before);
        assert_eq!(v.editor.page_list().len(), 1);
        assert_eq!(v.selected, Some(selected));
        assert_eq!(v.edit_ticket(), ticket);
        assert!(!v.editor.can_undo());
        window.click("design-template-cancel", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("design-template-dialog").is_none());
        window.click(("design-invitation-purpose", 2usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find(("design-invitation-family", 0usize)).visible());
        assert!(
            window
                .try_find(("design-invitation-family", 1usize))
                .is_some()
        );
        assert!(
            window
                .try_find(("design-invitation-family", 2usize))
                .is_none()
        );
        window.click(("design-invitation-family", 0usize), cx);
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("design-template-dialog").is_none());
        window.click(("design-invitation-purpose", 0usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        // Returning to All templates preserves the legacy catalog entry point.
        assert!(window.find(("design-template", 0usize)).visible());
        let v = view.read(cx);
        assert_eq!(v.editor.doc, before);
        assert_eq!(v.selected, Some(selected));
        assert_eq!(v.edit_ticket(), ticket);
        assert!(!v.editor.can_undo());
        assert_eq!(
            serde_json::to_value((&v.creative.catalog.brands, &v.creative.catalog.assets)).unwrap(),
            catalog
        );
    });
}

#[gpui_kit::test]
fn invitation_rapid_choices_reject_stale_async_projects_and_images(cx: &mut TestAppContext) {
    let (view, cx) = open_design(cx);
    let initial = Selection::for_family(FamilyId::GardenVows);
    let (preview, before, ticket) = cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            (
                v.open_template_preview(Source::Invitation(initial), window, cx),
                v.editor.doc.clone(),
                v.edit_ticket(),
            )
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        // Dispatch both choices against the same rendered callbacks. The usual
        // window.click helper repaints between every event and hides this race.
        for id in [
            ("design-invitation-layout", 1usize),
            ("design-invitation-palette", 2usize),
        ] {
            let position = window.find(id).bounds().center();
            window.dispatch_event(
                gpui_kit::MouseDownEvent {
                    position,
                    button: gpui_kit::MouseButton::Left,
                    modifiers: Default::default(),
                    click_count: 1,
                    first_mouse: false,
                }
                .to_platform_input(),
                cx,
            );
            window.dispatch_event(
                gpui_kit::MouseUpEvent {
                    position,
                    button: gpui_kit::MouseButton::Left,
                    modifiers: Default::default(),
                    click_count: 1,
                }
                .to_platform_input(),
                cx,
            );
        }
        assert_eq!(
            preview.read(cx).invitation,
            Some(Selection {
                variant: VariantId::GardenBorder,
                palette: 2,
                ..initial
            })
        );
    });
    cx.run_until_parked();
    let final_selection = Selection {
        variant: VariantId::WildflowerEditorial,
        palette: 2,
        ..initial
    };
    let previous_image = cx.update(|_, cx| {
        let previous = preview.read(cx).image.clone().unwrap();
        preview.update(cx, |p, cx| {
            let generation = p.project_generation;
            // Queue an old raster as well as multiple competing project loads.
            p.load_image(cx);
            p.choose_invitation(
                Selection {
                    variant: VariantId::GardenBorder,
                    ..initial
                },
                cx,
            );
            p.choose_invitation(
                Selection {
                    palette: 1,
                    ..initial
                },
                cx,
            );
            p.choose_invitation(final_selection, cx);
            assert_eq!(p.project_generation, generation + 3);
            assert!(p.project.is_none());
            assert!(p.image.is_none());
            assert!(p.loading);
        });
        previous
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(!window.has_image_atlas_entry(&previous_image));
        let p = preview.read(cx);
        assert_eq!(p.invitation, Some(final_selection));
        assert!(!p.loading);
        assert!(p.error.is_none());
        assert_eq!(p.index, 0);
        let expected = final_selection.create().unwrap();
        let project = p.project.as_ref().unwrap();
        assert_eq!(project.pages.len(), 3);
        for (actual, expected) in project.pages.iter().zip(expected.pages) {
            assert_eq!(actual.doc, expected.doc);
        }
        assert!(window.has_image_atlas_entry(p.image.as_ref().unwrap()));
        let v = view.read(cx);
        assert_eq!(v.editor.doc, before);
        assert_eq!(v.edit_ticket(), ticket);
        assert!(!v.editor.can_undo());
    });
    let weak = preview.downgrade();
    cx.update(|_, cx| {
        preview.update(cx, |p, cx| p.choose_invitation(initial, cx));
    });
    drop(preview);
    // Escape during a fresh load must release the preview, not apply its result.
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(weak.upgrade().is_none());
    cx.update(|window, cx| {
        assert!(window.try_find("design-template-dialog").is_none());
        assert_eq!(view.read(cx).editor.doc, before);
        assert_eq!(view.read(cx).edit_ticket(), ticket);
        assert!(!view.read(cx).editor.can_undo());
    });
}

#[gpui_kit::test]
fn invitation_page_navigation_clamps_stale_handlers_and_ignores_unready_projects(
    cx: &mut TestAppContext,
) {
    let (view, cx) = open_design(cx);
    let (preview, before) = cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            (
                v.open_template_preview(
                    Source::Invitation(Selection::for_family(FamilyId::GardenVows)),
                    window,
                    cx,
                ),
                v.editor.doc.clone(),
            )
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        preview.update(cx, |p, cx| {
            assert_eq!(p.project.as_ref().unwrap().pages.len(), 3);
            let generation = p.image_generation;
            p.change_page(false, cx);
            assert_eq!(p.index, 0);
            assert_eq!(p.image_generation, generation);
            p.change_page(true, cx);
            p.change_page(true, cx);
            assert_eq!(p.index, 2);
            let generation = p.image_generation;
            p.change_page(true, cx);
            assert_eq!(p.index, 2);
            assert_eq!(p.image_generation, generation);
            p.change_page(false, cx);
            assert_eq!(p.index, 1);

            let generation = p.image_generation;
            p.loading = true;
            p.change_page(false, cx);
            p.change_page(true, cx);
            assert_eq!(p.index, 1);
            p.loading = false;
            let project = p.project.take();
            p.change_page(false, cx);
            p.change_page(true, cx);
            assert_eq!(p.index, 1);
            p.project = project;
            p.applied = true;
            p.change_page(false, cx);
            p.change_page(true, cx);
            assert_eq!(p.index, 1);
            assert_eq!(p.image_generation, generation);
            p.applied = false;
        });
    });
    cx.run_until_parked();
    // Both callbacks remain enabled in the rendered middle-page frame. Repeated
    // raw clicks must stop at the boundary even before that frame is repainted.
    for (id, expected) in [("design-template-next", 2), ("design-template-previous", 0)] {
        cx.update(|_, cx| {
            preview.update(cx, |p, cx| {
                p.index = 1;
                p.load_image(cx);
            });
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            let position = window.find(id).bounds().center();
            for _ in 0..3 {
                window.dispatch_event(
                    gpui_kit::MouseDownEvent {
                        position,
                        button: gpui_kit::MouseButton::Left,
                        modifiers: Default::default(),
                        click_count: 1,
                        first_mouse: false,
                    }
                    .to_platform_input(),
                    cx,
                );
                window.dispatch_event(
                    gpui_kit::MouseUpEvent {
                        position,
                        button: gpui_kit::MouseButton::Left,
                        modifiers: Default::default(),
                        click_count: 1,
                    }
                    .to_platform_input(),
                    cx,
                );
            }
            assert_eq!(preview.read(cx).index, expected);
        });
        cx.run_until_parked();
    }
    cx.update(|_, cx| {
        assert_eq!(view.read(cx).editor.doc, before);
        assert_eq!(view.read(cx).editor.page_list().len(), 1);
        assert!(!view.read(cx).editor.can_undo());
        assert!(preview.read(cx).image.is_some());
    });
}

#[gpui_kit::test]
fn invitation_stale_apply_callbacks_wait_for_the_navigated_page_image(cx: &mut TestAppContext) {
    let (view, cx) = open_design(cx);
    let selection = Selection::for_family(FamilyId::GardenVows);
    let expected = selection.create().unwrap();
    for (action, expected_pages) in [
        ("design-template-add", 2),
        ("design-template-replace", 1),
        ("design-template-add-all", 4),
    ] {
        let (preview, before, ticket, stamp) = cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                (
                    v.open_template_preview(Source::Invitation(selection), window, cx),
                    v.editor.doc.clone(),
                    v.edit_ticket(),
                    v.editor.stamp(),
                )
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(preview.read(cx).image.is_some());
            // Both callbacks come from the same ready frame. The Next callback
            // clears its image, but the Apply callback is still enabled until
            // repaint. It must check readiness again before changing the design.
            let positions = [
                window.find("design-template-next").bounds().center(),
                window.find(action).bounds().center(),
            ];
            for position in positions {
                window.dispatch_event(
                    gpui_kit::MouseDownEvent {
                        position,
                        button: gpui_kit::MouseButton::Left,
                        modifiers: Default::default(),
                        click_count: 1,
                        first_mouse: false,
                    }
                    .to_platform_input(),
                    cx,
                );
                window.dispatch_event(
                    gpui_kit::MouseUpEvent {
                        position,
                        button: gpui_kit::MouseButton::Left,
                        modifiers: Default::default(),
                        click_count: 1,
                    }
                    .to_platform_input(),
                    cx,
                );
            }
            let p = preview.read(cx);
            assert_eq!(p.index, 1, "{action}");
            assert!(p.image.is_none(), "{action}");
            assert!(!p.applied, "{action} must wait for the current image");
            assert!(p.error.is_none(), "{action}");
            let v = view.read(cx);
            assert_eq!(v.editor.doc, before, "{action}");
            assert_eq!(v.editor.page_list().len(), 1, "{action}");
            assert_eq!(v.edit_ticket(), ticket, "{action}");
            assert_eq!(v.editor.stamp(), stamp, "{action}");
            assert!(!v.editor.can_undo(), "{action}");
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(preview.read(cx).image.is_some(), "{action}");
            window.click(action, cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.try_find("design-template-dialog").is_none());
            assert!(preview.read(cx).applied, "{action}");
            view.update(cx, |v, cx| {
                assert_eq!(v.editor.page_list().len(), expected_pages, "{action}");
                let expected_page = if action == "design-template-add-all" {
                    0
                } else {
                    1
                };
                assert_eq!(v.editor.doc, expected.pages[expected_page].doc, "{action}");
                assert!(v.editor.can_undo(), "{action}");
                v.undo(cx);
                assert_eq!(v.editor.page_list().len(), 1, "{action}");
                assert_eq!(v.editor.doc, before, "{action}");
                assert!(!v.editor.can_undo(), "{action}");
            });
        });
    }
}

#[gpui_kit::test]
fn invitation_matching_set_single_page_undo_save_reopen_png_and_pdf(cx: &mut TestAppContext) {
    let (view, cx) = open_design(cx);
    let selection = Selection {
        variant: VariantId::PartyTicket,
        palette: 1,
        ..Selection::for_family(FamilyId::ConfettiClub)
    };
    let expected = selection.create().unwrap();
    let before = cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.preview_invitation_family(selection, window, cx);
            v.editor.doc.clone()
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-template-add-all", cx));
    cx.run_until_parked();
    let project = cx.update(|_, cx| {
        view.update(cx, |v, cx| {
            // The original plus Invitation, Details and RSVP. Alternative fronts
            // and palettes must not be inserted as additional project pages.
            assert_eq!(v.editor.page_list().len(), 4);
            assert_eq!(v.editor.page(1).unwrap().doc, before);
            for (actual, expected) in v.editor.page_list().iter().skip(1).zip(&expected.pages) {
                assert_eq!(actual.name, expected.meta.name);
                assert_eq!(v.editor.page(actual.id).unwrap().doc, expected.doc);
            }
            assert!(v.selected.is_none());
            v.undo(cx);
            assert_eq!(v.editor.page_list().len(), 1);
            assert_eq!(v.editor.doc, before);
            v.redo(cx);
            assert_eq!(v.editor.page_list().len(), 4);
            v.editor.snapshot().unwrap()
        })
    });
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("matching-invitation-set.emu");
    emulsion_io::project::write(&project, &path).unwrap();
    let reopened = emulsion_io::project::read(&path).unwrap();
    assert_eq!(reopened.pages.len(), 4);
    for (saved, original) in reopened.pages.iter().zip(&project.pages) {
        assert_eq!(saved.meta, original.meta);
        assert_eq!(saved.doc, original.doc);
    }
    let matching_ids: Vec<_> = reopened
        .pages
        .iter()
        .skip(1)
        .map(|page| page.meta.id)
        .collect();
    for (format, name) in [
        (
            emulsion_io::project_export::Format::Png,
            "invitation-set.zip",
        ),
        (
            emulsion_io::project_export::Format::Pdf,
            "invitation-set.pdf",
        ),
    ] {
        let destination = dir.path().join(name);
        let report = emulsion_io::project_export::write(
            &reopened,
            &matching_ids,
            format,
            false,
            &destination,
        )
        .unwrap();
        assert_eq!(report.pages, 3);
        assert!(std::fs::metadata(destination).unwrap().len() > 0);
    }
    cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.undo(cx);
            assert_eq!(v.editor.page_list().len(), 1);
            v.preview_invitation_family(selection, window, cx);
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-template-next", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("design-template-add", cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |v, cx| {
            assert_eq!(v.editor.page_list().len(), 2);
            assert_eq!(v.editor.page(1).unwrap().doc, before);
            assert_eq!(v.editor.doc, expected.pages[1].doc);
            assert_eq!(v.editor.page_list()[1].name, expected.pages[1].meta.name);
            v.undo(cx);
            assert_eq!(v.editor.page_list().len(), 1);
            assert_eq!(v.editor.doc, before);
            v.redo(cx);
            assert_eq!(v.editor.page_list().len(), 2);
            assert_eq!(v.editor.doc, expected.pages[1].doc);
        });
    });
}

#[gpui_kit::test]
fn invitation_loading_keeps_choice_pager_and_footer_bounds_stable(cx: &mut TestAppContext) {
    let (view, cx) = open_design(cx);
    let preview = cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.open_template_preview(
                Source::Invitation(Selection::for_family(FamilyId::GardenVows)),
                window,
                cx,
            )
        })
    });
    cx.run_until_parked();
    for viewport in [size(px(1200.), px(900.)), size(px(900.), px(700.))] {
        cx.simulate_resize(viewport);
        cx.run_until_parked();
        cx.update(|window, cx| {
            let controls = [
                "design-template-dialog",
                "design-template-large-preview",
                "design-template-previous",
                "design-template-next",
                "design-template-add-all",
                "design-template-cancel",
                "design-template-replace",
                "design-template-add",
            ]
            .map(|id| (id, window.find(id).bounds()));
            let mut choices = Vec::new();
            for group in ["design-invitation-layout", "design-invitation-palette"] {
                for index in 0..3usize {
                    let id = (group, index);
                    choices.push((id, window.find(id).bounds()));
                }
            }
            let (project, image) = preview.update(cx, |p, cx| {
                assert!(!p.loading);
                assert!(p.project.is_some());
                assert!(p.image.is_some());
                p.loading = true;
                let previous = (p.project.take(), p.image.take());
                cx.notify();
                previous
            });
            // Render the loading state directly. No asynchronous load can finish
            // before these assertions and conceal a recentered dialog.
            window.render_frame(cx);
            window.render_frame(cx);
            assert!(preview.read(cx).loading);
            assert!(preview.read(cx).project.is_none());
            for (id, loaded) in &controls {
                assert_eq!(
                    window.find(*id).bounds(),
                    *loaded,
                    "{id} moved while loading at {viewport:?}"
                );
            }
            for (id, loaded) in &choices {
                assert_eq!(
                    window.find(*id).bounds(),
                    *loaded,
                    "{id:?} moved while loading at {viewport:?}"
                );
            }
            preview.update(cx, |p, cx| {
                p.project = project;
                p.image = image;
                p.loading = false;
                cx.notify();
            });
            window.render_frame(cx);
            window.render_frame(cx);
            for (id, loaded) in controls {
                assert_eq!(window.find(id).bounds(), loaded);
            }
            for (id, loaded) in choices {
                assert_eq!(window.find(id).bounds(), loaded);
            }
        });
    }
}

#[gpui_kit::test]
fn invitation_narrow_footer_stale_target_and_escape_are_safe(cx: &mut TestAppContext) {
    let (view, cx) = open_design(cx);
    cx.simulate_resize(size(px(900.), px(700.)));
    let before = cx.update(|window, cx| {
        view.update(cx, |v, cx| {
            v.preview_invitation_family(Selection::for_family(FamilyId::GardenVows), window, cx);
            v.editor.doc.clone()
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        for id in [
            "design-template-cancel",
            "design-template-add",
            "design-template-replace",
            "design-template-add-all",
        ] {
            let element = window.find(id);
            assert!(
                element.visible(),
                "{id} must be visible in a 900×700 window"
            );
            let bounds = element.bounds();
            assert!(
                bounds.origin.x >= px(0.) && bounds.right() <= px(900.),
                "{id} clipped horizontally: {bounds:?}"
            );
            assert!(
                bounds.origin.y >= px(0.) && bounds.bottom() <= px(700.),
                "{id} clipped vertically: {bounds:?}"
            );
        }
        view.update(cx, |v, _| v.operation_epoch += 1);
        window.click("design-template-add-all", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("design-template-error").visible());
        assert_eq!(view.read(cx).editor.doc, before);
        assert_eq!(view.read(cx).editor.page_list().len(), 1);
        assert!(!view.read(cx).editor.can_undo());
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("design-template-dialog").is_none());
        assert_eq!(view.read(cx).editor.doc, before);
        assert_eq!(view.read(cx).editor.page_list().len(), 1);
    });
}
