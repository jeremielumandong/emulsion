//! Photo-only dock geometry and real panel routes. These clicks must not edit artwork.
use super::*;
use crate::app_state::AppSettings;
use crate::tests::open;
use core::prelude::v1::test;
use gpui_kit::test::TestWindowExt;

fn photo_document() -> Document {
    let mut doc = Document::new(256, 192);
    Command::AddNode {
        node: Box::new(Node::raster(
            0,
            "Photo",
            Arc::new(Raster::solid(256, 192, [0.2, 0.3, 0.4, 1.])),
            Placement::default(),
        )),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .unwrap();
    doc
}

fn open_panel_menu(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.click("sidebar-more", cx));
    cx.run_until_parked();
}

#[gpui_kit::test]
fn photo_dock_primary_tabs_and_overflow_open_real_panels_without_edits(cx: &mut TestAppContext) {
    let original = photo_document();
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(size(px(1280.), px(900.)));
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    for compact in [false, true] {
        cx.update(|window, cx| {
            cx.global_mut::<AppSettings>().0.compact_chrome = compact;
            window.refresh();
        });
        cx.run_until_parked();
        cx.update(|window, _| {
            for id in [
                "sidebar-enhance",
                "sidebar-assistant",
                "sidebar-info-top",
                "sidebar-reference",
            ] {
                assert!(
                    window.try_find(id).is_none(),
                    "Photo has one compact tab row: {id}"
                );
            }
        });
        for (id, tab, content) in [
            (
                "sidebar-properties",
                SidebarTab::Properties,
                "sidebar-properties-content",
            ),
            (
                "sidebar-adjustments",
                SidebarTab::Adjustments,
                "sidebar-adjustments-content",
            ),
            (
                "sidebar-history-top",
                SidebarTab::History,
                "sidebar-history-content",
            ),
        ] {
            cx.update(|window, cx| window.click(id, cx));
            cx.run_until_parked();
            cx.update(|window, cx| {
                assert!(editor.read(cx).sidebar_tab == tab);
                assert!(window.find(content).visible());
            });
        }
        // The separator after Photo's extra panels occupies menu index 2.
        for (index, tab) in [
            (0usize, SidebarTab::Enhance),
            (1, SidebarTab::Assistant),
            (3, SidebarTab::Character),
            (4, SidebarTab::Info),
            (5, SidebarTab::Reference),
            (6, SidebarTab::Navigator),
            (7, SidebarTab::Histogram),
            (8, SidebarTab::BrushSettings),
            (9, SidebarTab::BrushPresets),
            (10, SidebarTab::Recipes),
            (11, SidebarTab::Timeline),
            (12, SidebarTab::BlendingOptions),
        ] {
            open_panel_menu(cx);
            cx.update(|window, cx| window.within("popup-menu").click(index, cx));
            cx.run_until_parked();
            cx.update(|window, cx| {
                assert!(editor.read(cx).sidebar_tab == tab, "menu index {index}");
                assert!(window.find(("sidebar-content", tab as usize)).visible());
                assert!(window.try_find("popup-menu").is_none());
                assert_eq!(editor.read(cx).editor.doc, original);
                assert!(editor.read(cx).editor.history.is_empty());
            });
        }
    }
}

#[gpui_kit::test]
fn photo_dock_controls_tabs_and_shortcut_strip_fit_narrow_widths(cx: &mut TestAppContext) {
    let original = photo_document();
    let (ws, cx) = open(cx, original.clone());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    for compact in [false, true] {
        for width in [1280., 720., 480.] {
            cx.simulate_resize(size(px(width), px(700.)));
            cx.update(|window, cx| {
                cx.global_mut::<AppSettings>().0.compact_chrome = compact;
                editor.update(cx, |editor, cx| {
                    editor.sidebar_layout.width = Some(220.);
                    editor.show_sidebar_tab(SidebarTab::Properties, cx);
                });
                window.refresh();
            });
            cx.run_until_parked();
            cx.update(|window, cx| {
                let panel = window.find("node-panel").bounds();
                let controls = window.find("sidebar-dock-controls").bounds();
                let tabs = window.find("sidebar-primary-tabs").bounds();
                assert!(controls.bottom() <= tabs.top());
                assert!(panel.left() >= px(0.) && panel.right() <= px(width));
                for id in [
                    "sidebar-properties",
                    "sidebar-adjustments",
                    "sidebar-history-top",
                    "sidebar-more",
                    "sidebar-collapse",
                    "sidebar-section-toggle",
                ] {
                    let bounds = window.find(id).bounds();
                    assert!(
                        bounds.left() >= panel.left() && bounds.right() <= panel.right(),
                        "{id}: {bounds:?} outside {panel:?}"
                    );
                }
                assert!(window.find("editor-canvas-column").bounds().size.width >= px(280.));
                window.click("sidebar-collapse", cx);
            });
            cx.run_until_parked();
            cx.update(|window, cx| {
                let overlay = window.find("photo-shortcuts-overlay").bounds();
                let strip = window.find("photo-shortcut-strip").bounds();
                assert_eq!(strip.top(), overlay.top());
                assert_eq!(strip.right(), overlay.right());
                assert_eq!(strip.bottom(), overlay.bottom());
                let frame = window.find("photo-canvas-dock-frame").bounds();
                assert_eq!(overlay.origin, frame.origin);
                let canvas = editor
                    .read(cx)
                    .canvas_bounds()
                    .expect("Photo canvas is mounted");
                assert!(
                    canvas.right() <= strip.left(),
                    "the rail must not cover photo pixels"
                );
                assert!(canvas.top() >= strip.top() && canvas.bottom() <= strip.bottom());
                editor.update(cx, |editor, cx| editor.zoom_fit(cx));
                let view = editor.read(cx);
                for corner in [(0., 0.), (256., 192.)] {
                    assert!(
                        canvas.contains(&view.doc_to_window(corner).unwrap()),
                        "Fit keeps the whole photo within the canvas, away from the rail"
                    );
                }
                window.click(("photo-shortcut", 2usize), cx);
            });
            cx.run_until_parked();
            cx.update(|window, cx| {
                let strip = window.find("photo-shortcut-strip").bounds();
                let flyout = window.find("photo-shortcut-panel").bounds();
                assert!(flyout.left() >= px(0.));
                assert!(flyout.right() <= strip.left());
                assert_eq!(flyout.top(), strip.top());
                window.click("photo-shortcut-close", cx);
                assert_eq!(editor.read(cx).editor.doc, original);
                assert!(editor.read(cx).editor.history.is_empty());
            });
            cx.run_until_parked();
        }
    }
}

#[gpui_kit::test]
fn photo_layer_controls_are_available_without_expanding_optional_filters(cx: &mut TestAppContext) {
    let original = photo_document();
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(size(px(1280.), px(900.)));
    let editor = cx.update(|window, cx| {
        cx.global_mut::<AppSettings>().0.compact_chrome = true;
        let editor = ws.read(cx).editor.clone().unwrap();
        editor.update(cx, |editor, cx| {
            let id = editor.editor.doc.nodes[0].id;
            editor.set_layer_selection(vec![id], Some(id));
            editor.layer_panel.controls_open = false;
            cx.notify();
        });
        window.refresh();
        editor
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("layers-blend-mode").visible());
        for index in 0..4usize {
            assert!(window.find(("layer-lock", index)).visible());
        }
        assert!(!editor.read(cx).layer_panel.controls_open);
        assert_eq!(editor.read(cx).editor.doc, original);
        assert!(editor.read(cx).editor.history.is_empty());
    });
}

#[gpui_kit::test]
fn photo_properties_keep_unique_controls_without_alignment_or_blending_duplicates(
    cx: &mut TestAppContext,
) {
    let original = photo_document();
    let id = original.nodes[0].id;
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(size(px(1440.), px(1200.)));
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    for compact in [false, true] {
        for flyout in [false, true] {
            cx.update(|window, cx| {
                cx.global_mut::<AppSettings>().0.compact_chrome = compact;
                editor.update(cx, |editor, cx| {
                    editor.set_layer_selection(vec![id], Some(id));
                    editor.set_tool(Tool::Brush, cx);
                    editor.layer_panel.controls_open = false;
                    editor.show_sidebar_tab(SidebarTab::Properties, cx);
                });
                window.refresh();
            });
            cx.run_until_parked();
            if flyout {
                cx.update(|window, cx| window.click(("photo-shortcut", 0usize), cx));
                cx.run_until_parked();
            }
            cx.update(|window, cx| {
                let properties = window.within("sidebar-properties-content");
                for control in ["photo-align", "move-align", "photo-blending", "photo-blend"] {
                    assert!(
                        properties.try_find(control).is_none(),
                        "duplicate {control}"
                    );
                }
                for key in ["PhotoOpacity", "PhotoFillOpacity"] {
                    assert!(
                        properties
                            .try_find(SharedString::from(format!("{key}({id})")))
                            .is_none()
                    );
                }
                for control in [
                    "photo-zoom-fit",
                    "photo-mask",
                    "mask-add",
                    "photo-actions",
                    "photo-select-subject",
                    "photo-remove-background",
                ] {
                    assert!(properties.try_find(control).is_some(), "retained {control}");
                }
                assert!(window.find("layers-blend-mode").visible());
                for key in ["LayerOpacity", "LayerFillOpacity"] {
                    assert!(
                        window
                            .find(SharedString::from(format!("{key}({id})")))
                            .visible()
                    );
                }
                assert_eq!(editor.read(cx).sidebar_layout.flyout_open, flyout);
                assert_eq!(editor.read(cx).editor.doc, original);
                assert!(editor.read(cx).editor.history.is_empty());
            });
        }
    }
}

#[gpui_kit::test]
fn photo_properties_transform_fields_follow_move_but_flips_work_from_brush_and_move(
    cx: &mut TestAppContext,
) {
    let original = photo_document();
    let id = original.nodes[0].id;
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(size(px(1440.), px(1200.)));
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    for tool in [Tool::Brush, Tool::Move] {
        for horizontal in [true, false] {
            cx.update(|_, cx| {
                editor.update(cx, |editor, cx| {
                    editor.set_layer_selection(vec![id], Some(id));
                    editor.set_tool(tool, cx);
                    editor.show_sidebar_tab(SidebarTab::Properties, cx);
                });
            });
            cx.run_until_parked();
            cx.update(|window, cx| {
                let mut properties = window.within("sidebar-properties-content");
                for field in ["W", "H", "X", "Y", "Angle"] {
                    assert_eq!(
                        properties
                            .try_find(SharedString::from(format!("photo-transform-{field}")))
                            .is_some(),
                        tool == Tool::Move,
                        "{field} with {tool:?}"
                    );
                }
                for (control, label) in [
                    ("photo-flip-horizontal", "Flip horizontal"),
                    ("photo-flip-vertical", "Flip vertical"),
                ] {
                    assert!(properties.find(control).visible());
                    assert_eq!(properties.find(control).label(), Some(label));
                }
                assert_eq!(editor.read(cx).editor.doc, original);
                assert!(editor.read(cx).editor.history.is_empty());
                properties.click(
                    if horizontal {
                        "photo-flip-horizontal"
                    } else {
                        "photo-flip-vertical"
                    },
                    cx,
                );
            });
            cx.run_until_parked();
            cx.update(|_, cx| {
                let mut expected = original.clone();
                let NodeKind::Raster { placement, .. } = &mut expected.nodes[0].kind else {
                    panic!("pixel fixture");
                };
                placement.flip_x = horizontal;
                placement.flip_y = !horizontal;
                assert_eq!(editor.read(cx).editor.doc, expected);
                assert_eq!(editor.read(cx).editor.history.len(), 1);
                editor.update(cx, |editor, cx| editor.undo(cx));
                assert_eq!(editor.read(cx).editor.doc, original);
            });
            cx.run_until_parked();
        }
    }
    cx.update(|_, cx| {
        editor.update(cx, |editor, cx| {
            editor.execute(Command::SetLocked { id, locked: true }, cx);
            editor.set_tool(Tool::Move, cx);
        });
    });
    cx.run_until_parked();
    let locked = cx.update(|window, cx| {
        let mut properties = window.within("sidebar-properties-content");
        assert!(properties.try_find("photo-transform-W").is_none());
        assert!(properties.find("photo-flip-horizontal").visible());
        let locked = editor.read(cx).editor.doc.clone();
        properties.click("photo-flip-horizontal", cx);
        locked
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(editor.read(cx).editor.doc, locked);
        assert_eq!(
            editor.read(cx).editor.history.len(),
            1,
            "only the lock edits history"
        );
        editor.update(cx, |editor, cx| editor.undo(cx));
        assert_eq!(editor.read(cx).editor.doc, original);
    });
    cx.run_until_parked();
    cx.update(|window, _| {
        assert!(
            window
                .within("sidebar-properties-content")
                .find("photo-transform-W")
                .visible()
        );
    });
}

#[gpui_kit::test]
fn photo_flip_labels_fit_narrow_translated_and_scaled_properties(cx: &mut TestAppContext) {
    struct RestoreLocale(String);
    impl Drop for RestoreLocale {
        fn drop(&mut self) {
            rust_i18n::set_locale(&self.0);
        }
    }
    let _locale = RestoreLocale(rust_i18n::locale().to_string());
    let original = photo_document();
    let id = original.nodes[0].id;
    let (ws, cx) = open(cx, original.clone());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    for (width, rem, locale) in [
        (720., 16., "en"),
        (720., 16., "de"),
        (720., 16., "pt-BR"),
        (900., 20., "de"),
        (900., 20., "pt-BR"),
    ] {
        rust_i18n::set_locale(locale);
        cx.simulate_resize(size(px(width), px(1000.)));
        for flyout in [false, true] {
            cx.update(|window, cx| {
                cx.global_mut::<AppSettings>().0.compact_chrome = true;
                window.set_rem_size(px(rem));
                editor.update(cx, |editor, cx| {
                    editor.sidebar_layout.width = Some(220.);
                    editor.set_layer_selection(vec![id], Some(id));
                    editor.set_tool(Tool::Brush, cx);
                    editor.show_sidebar_tab(SidebarTab::Properties, cx);
                });
                window.refresh();
            });
            cx.run_until_parked();
            if flyout {
                cx.update(|window, cx| window.click(("photo-shortcut", 0usize), cx));
                cx.run_until_parked();
            }
            cx.update(|window, cx| {
                let bounds = window.find("sidebar-properties-content").bounds();
                for (control, label) in [
                    ("photo-flip-horizontal", t!("editor.photo_panels.flip_h")),
                    ("photo-flip-vertical", t!("editor.photo_panels.flip_v")),
                ] {
                    let button = window.find(control);
                    assert!(button.visible());
                    assert_eq!(button.label(), Some(label.as_ref()));
                    let b = button.bounds();
                    assert!(
                        b.left() >= bounds.left() && b.right() <= bounds.right() + px(1.),
                        "{locale}, rem {rem}, flyout {flyout}: {control}: {b:?} outside {bounds:?}"
                    );
                    assert!(b.left() >= px(0.) && b.right() <= px(width));
                }
                assert_eq!(editor.read(cx).editor.doc, original);
                assert!(editor.read(cx).editor.history.is_empty());
            });
        }
    }
}

#[gpui_kit::test]
fn paint_properties_preserve_alignment_blending_and_brush_flips_in_both_hosts(
    cx: &mut TestAppContext,
) {
    let original = photo_document();
    let id = original.nodes[0].id;
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(size(px(1440.), px(1200.)));
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    for flyout in [false, true] {
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.draw_mode = true;
                editor.set_layer_selection(vec![id], Some(id));
                editor.set_tool(Tool::Brush, cx);
                editor.show_sidebar_tab(SidebarTab::Properties, cx);
            });
        });
        cx.run_until_parked();
        if flyout {
            cx.update(|window, cx| window.click(("photo-shortcut", 0usize), cx));
            cx.run_until_parked();
        }
        cx.update(|window, cx| {
            let properties = window.within("sidebar-properties-content");
            for control in [
                "photo-align",
                "move-align",
                "photo-blending",
                "photo-blend",
                "photo-flip-horizontal",
                "photo-flip-vertical",
            ] {
                assert!(
                    properties.try_find(control).is_some(),
                    "Paint preserves {control}"
                );
            }
            assert_eq!(
                properties.find("move-align-button").label(),
                Some("Align ▾")
            );
            for key in ["PhotoOpacity", "PhotoFillOpacity"] {
                assert!(
                    properties
                        .try_find(SharedString::from(format!("{key}({id})")))
                        .is_some()
                );
            }
            assert_eq!(editor.read(cx).sidebar_layout.flyout_open, flyout);
            assert_eq!(editor.read(cx).editor.doc, original);
            assert!(editor.read(cx).editor.history.is_empty());
        });
    }
}

#[gpui_kit::test]
fn photo_move_alignment_remains_keyboard_accessible_in_narrow_options_overflow(
    cx: &mut TestAppContext,
) {
    let mut original = photo_document();
    let id = original.nodes[0].id;
    let NodeKind::Raster { placement, .. } = &mut original.nodes[0].kind else {
        panic!("pixel fixture");
    };
    placement.x = 30.;
    placement.y = 40.;
    let (ws, cx) = open(cx, original.clone());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    for narrow in [false, true] {
        cx.simulate_resize(size(px(if narrow { 480. } else { 1280. }), px(900.)));
        cx.update(|window, cx| {
            cx.global_mut::<AppSettings>().0.compact_chrome = true;
            editor.update(cx, |editor, cx| {
                editor.set_layer_selection(vec![id], Some(id));
                editor.set_tool(Tool::Move, cx);
                editor.show_sidebar_tab(SidebarTab::Properties, cx);
                editor.compact.bars[super::compact::Bar::Options as usize].scale =
                    if narrow { 2. } else { 1. };
                window.focus(&editor.canvas_focus, cx);
            });
            window.refresh();
        });
        cx.run_until_parked();
        // First dismiss without choosing a command, then reopen and execute
        // the same Canvas alignment through the real keyboard submenu path.
        for apply in [false, true] {
            if narrow {
                cx.update(|window, cx| {
                    assert!(
                        window
                            .within("editor-tool-options")
                            .try_find("move-align")
                            .is_none()
                    );
                    window.click("tool-options-more", cx);
                });
                cx.run_until_parked();
            }
            cx.update(|window, cx| {
                assert!(window.find("move-align").visible());
                assert_eq!(
                    window.find("move-align-button").label(),
                    Some("Align & distribute ▾")
                );
                window.click("move-align", cx);
            });
            cx.run_until_parked();
            let menu_bounds = cx.update(|window, cx| {
                let bounds = window.find("popup-menu").bounds();
                let mut menu = window.within("popup-menu");
                assert_eq!(menu.find(0usize).label(), Some("Canvas"));
                assert_eq!(
                    menu.find(1usize).label(),
                    Some("Selection (make a selection first)")
                );
                assert_eq!(
                    menu.find(2usize).label(),
                    Some("Selected layers (select two or more)")
                );
                assert_eq!(editor.read(cx).editor.doc, original);
                assert!(editor.read(cx).editor.history.is_empty());
                if apply {
                    menu.hover(0usize, cx);
                } else {
                    window.press("escape", cx);
                }
                bounds
            });
            cx.run_until_parked();
            if apply {
                // Constrained popups open their submenu to the left. Enter
                // in its actual direction instead of assuming a wide window.
                cx.update(|window, cx| {
                    let submenu = window.find("submenu").bounds();
                    window.press(
                        if submenu.left() < menu_bounds.left() {
                            "left"
                        } else {
                            "right"
                        },
                        cx,
                    );
                });
                cx.run_until_parked();
                cx.update(|window, cx| {
                    assert_eq!(
                        window.within("submenu").find(0usize).label(),
                        Some("Align left")
                    );
                    assert_eq!(window.within("submenu").find(0usize).selected(), Some(true));
                    window.press("enter", cx);
                });
                cx.run_until_parked();
            }
            cx.update(|window, cx| {
                assert!(window.try_find("popup-menu").is_none());
                assert!(window.try_find("submenu").is_none());
                if apply {
                    let mut expected = original.clone();
                    let NodeKind::Raster { placement, .. } = &mut expected.nodes[0].kind else {
                        panic!("pixel fixture");
                    };
                    placement.x = 0.;
                    assert_eq!(editor.read(cx).editor.doc, expected);
                    assert_eq!(editor.read(cx).editor.history.len(), 1);
                    editor.update(cx, |editor, cx| editor.undo(cx));
                }
                assert_eq!(editor.read(cx).editor.doc, original);
                if narrow && window.try_find("tool-options-overflow-content").is_some() {
                    window.press("escape", cx);
                }
            });
            cx.run_until_parked();
            cx.update(|window, _| {
                assert!(window.try_find("tool-options-overflow-content").is_none());
            });
        }
        // Dismissal must leave the canvas tool shortcuts usable, without
        // manually focusing the canvas to conceal a trapped popup focus scope.
        cx.simulate_keystrokes("b");
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(editor.read(cx).tool, Tool::Brush, "narrow={narrow}");
            assert_eq!(editor.read(cx).editor.doc, original);
            assert!(editor.read(cx).editor.history.is_empty());
        });
    }
}

#[gpui_kit::test]
fn photo_alignment_outside_click_preserves_editable_properties_focus(cx: &mut TestAppContext) {
    let original = photo_document();
    let id = original.nodes[0].id;
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(size(px(1440.), px(1100.)));
    let editor = cx.update(|window, cx| {
        cx.global_mut::<AppSettings>().0.compact_chrome = true;
        let editor = ws.read(cx).editor.clone().unwrap();
        editor.update(cx, |editor, cx| {
            editor.set_layer_selection(vec![id], Some(id));
            editor.set_tool(Tool::Move, cx);
            editor.show_sidebar_tab(SidebarTab::Properties, cx);
            window.focus(&editor.canvas_focus, cx);
        });
        window.refresh();
        editor
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("move-align", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.within("popup-menu").hover(0usize, cx);
        window.press("right", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("submenu").visible());
        window
            .within("sidebar-properties-content")
            .click("photo-transform-W", cx);
    });
    cx.run_until_parked();
    let input_focus = cx.update(|window, cx| {
        assert!(window.try_find("popup-menu").is_none());
        assert!(window.try_find("submenu").is_none());
        assert!(!editor.read(cx).canvas_focus.is_focused(window));
        assert_eq!(editor.read(cx).editor.doc, original);
        window
            .focused(cx)
            .expect("clicked transform input has focus")
    });
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("128");
    cx.run_until_parked();
    cx.update(|window, _| assert!(input_focus.is_focused(window)));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.update(|_, cx| {
        let mut expected = original.clone();
        let NodeKind::Raster { placement, .. } = &mut expected.nodes[0].kind else {
            panic!("pixel fixture");
        };
        placement.scale_x = 0.5;
        assert_eq!(editor.read(cx).editor.doc, expected);
        assert_eq!(editor.read(cx).editor.history.len(), 1);
        editor.update(cx, |editor, cx| editor.undo(cx));
        assert_eq!(editor.read(cx).editor.doc, original);
    });
}

#[gpui_kit::test]
fn draw_keeps_existing_primary_panels_and_floating_shortcut_card(cx: &mut TestAppContext) {
    let original = photo_document();
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(size(px(1280.), px(900.)));
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    for compact in [false, true] {
        cx.update(|window, cx| {
            cx.global_mut::<AppSettings>().0.compact_chrome = compact;
            editor.update(cx, |editor, cx| {
                editor.draw_mode = true;
                editor.show_sidebar_tab(SidebarTab::Properties, cx);
            });
            window.refresh();
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            for id in [
                "sidebar-enhance",
                "sidebar-assistant",
                "sidebar-info-top",
                "sidebar-reference",
            ] {
                assert!(window.find(id).visible(), "Draw preserves {id}");
            }
            let overlay = window.find("photo-shortcuts-overlay").bounds();
            let strip = window.find("photo-shortcut-strip").bounds();
            assert!(strip.top() > overlay.top());
            assert!(strip.right() < overlay.right());
            window.click("sidebar-assistant", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.find("sidebar-assistant-prompt").visible());
            assert!(editor.read(cx).sidebar_tab == SidebarTab::Assistant);
            assert_eq!(editor.read(cx).editor.doc, original);
            assert!(editor.read(cx).editor.history.is_empty());
        });
    }
}

#[gpui_kit::test]
fn photo_raw_original_remains_reachable_from_panel_menu(cx: &mut TestAppContext) {
    use emulsion_core::raw::{RawDocument, RawMetadata};
    let mut original = photo_document();
    original.raw = Some(RawDocument {
        schema_version: 1,
        node_id: original.nodes[0].id,
        source: std::env::temp_dir().join("emulsion-missing-photo-dock-fixture.dng"),
        source_sha256: "0".repeat(64),
        params: Default::default(),
        metadata: RawMetadata::default(),
    });
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(size(px(1280.), px(900.)));
    cx.run_until_parked();
    open_panel_menu(cx);
    cx.update(|window, cx| window.within("popup-menu").click(2usize, cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let editor = ws.read(cx).editor.as_ref().unwrap().read(cx);
        assert!(editor.sidebar_tab == SidebarTab::Develop);
        assert!(
            window
                .find(("sidebar-content", SidebarTab::Develop as usize))
                .visible()
        );
        assert_eq!(editor.editor.doc, original);
        assert!(editor.editor.history.is_empty());
    });
}

#[gpui_kit::test]
fn photo_short_windows_keep_layer_rows_visible_beside_scrollable_controls(cx: &mut TestAppContext) {
    let mut original = photo_document();
    for name in ["Detail", "Top"] {
        Command::AddNode {
            node: Box::new(Node::raster(
                0,
                name,
                Arc::new(Raster::solid(256, 192, [0.2, 0.3, 0.4, 1.])),
                Placement::default(),
            )),
            slot: Slot::TOP,
        }
        .apply(&mut original)
        .unwrap();
    }
    let (ws, cx) = open(cx, original.clone());
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    for compact in [false, true] {
        for (width, height) in [(1000., 720.), (800., 600.), (720., 540.)] {
            cx.simulate_resize(size(px(width), px(height)));
            cx.update(|window, cx| {
                cx.global_mut::<AppSettings>().0.compact_chrome = compact;
                editor.update(cx, |editor, cx| {
                    editor.layer_panel.controls_open = false;
                    editor.show_sidebar_tab(SidebarTab::History, cx);
                });
                window.refresh();
            });
            cx.run_until_parked();
            cx.update(|window, cx| {
                let list = window.find("sidebar-layers-list").bounds();
                let row = window.find(("row", 3u64));
                assert!(
                    row.visible(),
                    "top layer at {width}×{height}, compact={compact}"
                );
                assert!(
                    list.size.height >= row.bounds().size.height,
                    "a complete row stays usable at {width}×{height}, compact={compact}: {list:?}"
                );
                assert!(window.find("layers-blend-mode").visible());
                assert!(window.find("layers-delete").visible());
                assert!(window.find("sidebar-history-content").visible());
                assert_eq!(editor.read(cx).editor.doc, original);
                assert!(editor.read(cx).editor.history.is_empty());
            });
        }
    }
}
