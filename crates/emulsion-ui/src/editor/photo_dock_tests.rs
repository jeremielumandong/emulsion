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
