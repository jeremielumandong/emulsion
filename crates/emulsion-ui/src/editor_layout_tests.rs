//! The familiar desktop arrangement must leave usable canvas and layer space.
use super::*;
use gpui_kit::test::TestWindowExt;

#[gpui_kit::test]
fn history_and_paths_docks_work_without_changing_the_document(cx: &mut TestAppContext) {
    let mut original = doc(&["Photo"], None);
    let path_id = Command::AddNode {
        node: Box::new(Node::path(
            0,
            "Triangle",
            Arc::new(emulsion_raster::vector::Path::from_svg("M 10 10 L 80 10 L 20 60 Z").unwrap()),
            Default::default(),
            256,
            192,
        )),
        slot: Slot::TOP,
    }
    .apply(&mut original)
    .unwrap()
    .unwrap();
    let (ws, cx) = open(cx, original.clone());
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.execute(
                Command::SetOpacity {
                    id: path_id,
                    opacity: 0.5,
                },
                cx,
            );
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("history-initial", cx));
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(e.read(cx).editor.doc, original));
    cx.update(|window, cx| window.click("history-redo", cx));
    cx.run_until_parked();
    let edited = cx.update(|_, cx| e.read(cx).editor.doc.clone());
    assert_ne!(edited, original);
    cx.update(|window, cx| window.click("dock-paths", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("path-row", path_id), cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("sidebar-paths-content").visible());
        assert!(window.find("sidebar-history-content").visible());
        let e = e.read(cx);
        assert_eq!(e.selected, Some(path_id));
        assert_eq!(e.tool, crate::editor::Tool::Pen);
        assert_eq!(e.editor.doc, edited);
    });
    cx.update(|window, cx| window.click("dock-layers", cx));
    cx.run_until_parked();
    cx.update(|window, _| assert!(window.find(("row", path_id)).visible()));
}

#[gpui_kit::test]
fn editor_keeps_tools_left_layers_right_and_options_above_canvas(cx: &mut TestAppContext) {
    let (_ws, cx) = open(cx, doc(&["Photo", "Paint", "Details"], None));
    for (width, height) in [(1000., 720.), (800., 600.), (720., 540.)] {
        cx.simulate_resize(gpui_kit::size(gpui_kit::px(width), gpui_kit::px(height)));
        cx.run_until_parked();
        cx.update(|window, _| {
            let rail = window.find("tool-rail").bounds();
            let scroll = window.find("tool-rail-scroll").bounds();
            let swatches = window.find("tool-rail-swatches").bounds();
            assert!(scroll.origin.y >= rail.origin.y);
            assert!(scroll.origin.y + scroll.size.height <= swatches.origin.y + gpui_kit::px(1.));
            assert!(
                swatches.origin.y + swatches.size.height
                    <= rail.origin.y + rail.size.height + gpui_kit::px(1.)
            );
            assert!(window.find("tool-rail-swatches").visible());
            let options = window.find("editor-tool-options").bounds();
            let area = window.find("editor-work-area").bounds();
            let canvas = window.find("editor-canvas-column").bounds();
            let dock = window.find("node-panel").bounds();
            let layers = window.find("sidebar-layers-dock").bounds();
            let inspector = window
                .find((
                    "sidebar-content",
                    crate::editor::SidebarTab::History as usize,
                ))
                .bounds();
            assert!(
                rail.size.width <= gpui_kit::px(64.),
                "tools should use a narrow rail"
            );
            assert!(canvas.origin.x >= rail.origin.x + rail.size.width);
            assert!(dock.origin.x >= canvas.origin.x + canvas.size.width);
            assert!(
                canvas.size.width >= gpui_kit::px(300.),
                "canvas width at {width}×{height}: {canvas:?}"
            );
            assert!(
                canvas.size.height >= gpui_kit::px(300.),
                "canvas height at {width}×{height}: {canvas:?}"
            );
            assert!(options.origin.y + options.size.height <= area.origin.y + gpui_kit::px(1.));
            assert!(
                layers.size.height > inspector.size.height,
                "Layers should be the main dock at {width}×{height}: {layers:?}, {inspector:?}"
            );
            assert!(inspector.origin.y + inspector.size.height <= layers.origin.y);
            assert!(window.find("sidebar-history-content").visible());
            assert!(window.find("dock-layers").visible());
            assert!(window.find("dock-channels").visible());
            assert!(window.find("dock-paths").visible());
            assert!(window.find(("row", 3u64)).visible());
            assert!(window.find("sidebar-properties").visible());
        });
    }
}

#[gpui_kit::test]
fn grouped_tools_remain_clickable_outside_the_scrolling_rail(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(800.), gpui_kit::px(600.)));
    cx.run_until_parked();
    for _ in 0..2 {
        cx.update(|window, cx| window.click("Rectangular marquee", cx));
        cx.run_until_parked();
    }
    // The active trigger closes the open menu, and can reopen it.
    cx.update(|window, cx| window.click("Rectangular marquee", cx));
    cx.run_until_parked();
    cx.update(|window, _| assert!(window.try_find(("rail-flyout", 1usize)).is_none()));
    cx.update(|window, cx| window.click("Rectangular marquee", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let popup = window.find(("rail-flyout", 1usize));
        assert!(popup.visible());
        let rail = window.find("tool-rail").bounds();
        assert!(
            popup.bounds().origin.x + popup.bounds().size.width
                > rail.origin.x + rail.size.width + gpui_kit::px(100.)
        );
        window.click(("rail-flyout-item", 17usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let e = ws.read(cx).editor.as_ref().unwrap().read(cx);
        assert_eq!(e.tool, crate::editor::Tool::Select);
        assert_eq!(e.tools.select, crate::editor::SelectShape::Ellipse);
        assert!(window.try_find(("rail-flyout", 1usize)).is_none());
        assert_eq!(e.editor.history.len(), 0);
    });
    cx.update(|window, cx| window.click("Elliptical marquee", cx));
    cx.run_until_parked();
    cx.update(|window, _| assert!(window.find(("rail-flyout", 1usize)).visible()));
    cx.simulate_click(
        gpui_kit::point(gpui_kit::px(700.), gpui_kit::px(590.)),
        gpui_kit::Modifiers::none(),
    );
    cx.run_until_parked();
    cx.update(|window, _| assert!(window.try_find(("rail-flyout", 1usize)).is_none()));
}
