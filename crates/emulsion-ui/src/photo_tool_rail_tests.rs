//! Docked Photo tools keep the user's column choice and remain reachable on short windows.
use super::*;
use crate::editor::{
    EditorView, Tool,
    rail::{DRAW_GROUPS, GROUPS},
};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{Bounds, Pixels, ScrollDelta, Window, point, px, size};

fn photo(
    cx: &mut TestAppContext,
    original: Document,
) -> (
    Entity<Workspace>,
    Entity<EditorView>,
    &mut VisualTestContext,
) {
    let (workspace, cx) = open(cx, original);
    cx.simulate_resize(size(px(1440.), px(1200.)));
    let editor = cx.update(|window, cx| {
        cx.global_mut::<AppSettings>().0.compact_chrome = true;
        window.refresh();
        workspace.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    (workspace, editor, cx)
}

fn assert_inside(inner: Bounds<Pixels>, outer: Bounds<Pixels>) {
    assert!(
        inner.left() >= outer.left() - px(1.),
        "{inner:?} outside {outer:?}"
    );
    assert!(
        inner.right() <= outer.right() + px(1.),
        "{inner:?} outside {outer:?}"
    );
    assert!(
        inner.top() >= outer.top() - px(1.),
        "{inner:?} outside {outer:?}"
    );
    assert!(
        inner.bottom() <= outer.bottom() + px(1.),
        "{inner:?} outside {outer:?}"
    );
}

fn scroll_to_tool(name: &'static str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let viewport = window.find("tool-rail-scroll").bounds();
        let target = window.within("tool-rail").find(name).bounds();
        window.scroll(
            "tool-rail-scroll",
            ScrollDelta::Pixels(point(px(0.), viewport.center().y - target.center().y)),
            cx,
        );
    });
    cx.run_until_parked();
    cx.update(|window, _| {
        let target = window.within("tool-rail").find(name);
        assert!(target.visible(), "{name} must be reachable by scrolling");
        assert_inside(target.bounds(), window.find("tool-rail-scroll").bounds());
    });
}

#[gpui_kit::test]
fn photo_rail_column_choice_and_tool_order_survive_short_narrow_windows(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let (_workspace, editor, cx) = photo(cx, original.clone());
    for columns in [1usize, 2, 1] {
        cx.update(|window, cx| {
            if usize::from(editor.read(cx).workspace_snapshot().tool_columns) != columns {
                window.click("tool-columns-toggle", cx);
            }
        });
        cx.run_until_parked();
        for (width, height) in [
            (1440., 1200.),
            (1188., 848.),
            (900., 650.),
            (720., 540.),
            (1440., 1200.),
        ] {
            cx.simulate_resize(size(px(width), px(height)));
            cx.run_until_parked();
            cx.update(|window, cx| {
                let rail = window.find("tool-rail").bounds();
                let viewport = window.find("tool-rail-scroll").bounds();
                let content = window.find("tool-rail-items").bounds();
                let swatches = window.find("tool-rail-swatches").bounds();
                let toolbar = window.find("canvas-toolbar-tools").bounds();
                let rows = GROUPS.len().div_ceil(columns);
                assert_eq!(rail.size.width, px(columns as f32 * 30. - 2.));
                assert_eq!(content.size.height, px(rows as f32 * 30. - 2.));
                assert_inside(viewport, toolbar);
                assert_inside(swatches, toolbar);
                assert!(swatches.top() >= viewport.bottom());
                if viewport.size.height < content.size.height {
                    let close = window.find("toolbar-close-tools").bounds();
                    assert!(
                        close.top() - swatches.bottom() < px(30.),
                        "scrolling must not waste a tool row at {width}×{height}: \
                        shell={toolbar:?}, viewport={viewport:?}, \
                        swatches={swatches:?}, close={close:?}"
                    );
                }
                assert!(window.find("fg-swatch").visible());
                assert!(window.find("quick-mask-toggle").visible());
                for (index, group) in GROUPS.iter().enumerate() {
                    let target = window.within("tool-rail").find(group[0].name).bounds();
                    assert_eq!(target.size, size(px(28.), px(28.)));
                    assert_eq!(
                        target.left(),
                        content.left() + px((index % columns) as f32 * 30.)
                    );
                    assert_eq!(
                        target.top(),
                        content.top() + px((index / columns) as f32 * 30.)
                    );
                }
                if columns == 2 {
                    for (name, column, row) in [
                        ("Move", 0, 0),
                        ("Rectangular marquee", 1, 0),
                        ("Lasso", 0, 1),
                        ("Quick select (AI)", 1, 1),
                    ] {
                        let target = window.within("tool-rail").find(name).bounds();
                        assert_eq!(
                            target.origin,
                            content.origin + point(px(column as f32 * 30.), px(row as f32 * 30.))
                        );
                    }
                }
                let editor = editor.read(cx);
                assert_eq!(
                    usize::from(editor.workspace_snapshot().tool_columns),
                    columns
                );
                assert_eq!(editor.editor.doc, original);
                assert_eq!(editor.editor.history.len(), 0);
            });
        }
    }
}

#[gpui_kit::test]
fn photo_rail_scroll_clicks_flyouts_and_swatches_preserve_artwork(cx: &mut TestAppContext) {
    let mut original = doc(&["Photo"], None);
    // Selecting Mask intentionally creates a mask unless the layer already has one.
    original.nodes[0].mask = Some(Arc::new(emulsion_raster::Mask::white(256, 192)));
    let (_workspace, editor, cx) = photo(cx, original.clone());
    cx.simulate_resize(size(px(720.), px(540.)));
    cx.run_until_parked();
    for columns in [1u8, 2] {
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_tool(Tool::Move, cx);
                editor.rail = Default::default();
            });
            if editor.read(cx).workspace_snapshot().tool_columns != columns {
                window.click("tool-columns-toggle", cx);
            }
        });
        cx.run_until_parked();
        for group in GROUPS {
            let tool = group[0];
            scroll_to_tool(tool.name, cx);
            cx.update(|window, cx| window.within("tool-rail").click(tool.name, cx));
            cx.run_until_parked();
            cx.update(|window, cx| {
                assert_eq!(
                    window.within("tool-rail").find(tool.name).selected(),
                    Some(true)
                );
                assert_eq!(editor.read(cx).editor.doc, original);
                assert_eq!(editor.read(cx).editor.history.len(), 0);
            });
        }
        // A grouped tool near the bottom opens outside the scrolling mask and is clickable.
        scroll_to_tool("Hand", cx);
        cx.update(|window, cx| window.within("tool-rail").right_click("Hand", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            let flyout = window.find(("rail-flyout", 17usize));
            assert!(flyout.visible());
            assert_inside(
                flyout.bounds(),
                Bounds::new(point(px(0.), px(0.)), window.viewport_size()),
            );
            assert!(flyout.bounds().right() > window.find("tool-rail-scroll").bounds().right());
            window.click(("rail-flyout-item", 17usize * 16 + 1), cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(editor.read(cx).tool, Tool::Hand);
            assert!(editor.read(cx).tools.rotate_view);
            assert_eq!(
                window.within("tool-rail").find("Rotate View").selected(),
                Some(true)
            );
            assert!(window.try_find(("rail-flyout", 17usize)).is_none());
            window.click("fg-swatch", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(editor.read(cx).tools.picker);
            window.click("fg-swatch", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(!editor.read(cx).tools.picker);
            window.click("quick-mask-toggle", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(editor.read(cx).tools.quick_mask);
            window.click("quick-mask-toggle", cx);
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert!(!editor.tools.quick_mask);
            assert_eq!(editor.editor.doc, original);
            assert_eq!(editor.editor.history.len(), 0);
        });
    }
}

fn assert_legacy_rail(window: &mut Window) {
    assert!(window.find("tool-rail").visible());
    assert!(window.try_find("tool-rail-items").is_none());
}

#[gpui_kit::test]
fn photo_rail_scrolling_does_not_replace_custom_or_other_workspaces(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let (workspace, editor, cx) = photo(cx, original.clone());
    cx.simulate_resize(size(px(900.), px(650.)));
    cx.run_until_parked();
    let factory = cx.update(|_, cx| editor.read(cx).workspace_snapshot());
    for (edge, overlay, draw) in [
        ("floating", false, false),
        ("top", false, false),
        ("bottom", false, false),
        ("left", true, false),
        ("left", false, true),
    ] {
        let mut layout = factory.clone();
        layout.toolbars_overlay = Some(overlay);
        layout.draw_mode = draw;
        layout
            .toolbar_placements
            .iter_mut()
            .find(|bar| bar.id == "tools")
            .unwrap()
            .edge = edge.into();
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| editor.apply_workspace_layout(&layout, cx))
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_legacy_rail(window);
            let content = window.find("tool-rail-scroll").bounds();
            let horizontal = matches!(edge, "top" | "bottom");
            let slots = ((f32::from(if horizontal {
                content.size.width
            } else {
                content.size.height
            }) + 2.)
                / 30.)
                .round() as usize;
            let groups = if draw { DRAW_GROUPS } else { GROUPS };
            for (index, group) in groups.iter().enumerate() {
                let target = window.within("tool-rail").find(group[0].name).bounds();
                let (column, row) = if horizontal {
                    (index % slots, index / slots)
                } else {
                    (index / slots, index % slots)
                };
                assert_eq!(
                    target.origin,
                    content.origin + point(px(column as f32 * 30.), px(row as f32 * 30.))
                );
            }
            assert_eq!(editor.read(cx).workspace_snapshot().tool_columns, 1);
            assert_eq!(editor.read(cx).editor.doc, original);
            assert_eq!(editor.read(cx).editor.history.len(), 0);
        });
    }
    let mut layout = factory.clone();
    layout.tool_ids = vec!["Move".into(), "Brush".into(), "Hand".into()];
    cx.update(|_, cx| editor.update(cx, |editor, cx| editor.apply_workspace_layout(&layout, cx)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("custom-tool-rail").visible());
        assert!(window.try_find("tool-rail-items").is_none());
        assert_eq!(
            editor.read(cx).workspace_snapshot().tool_ids,
            layout.tool_ids
        );
    });
    // A right-docked Photo rail uses the same bounded column preference.
    layout = factory;
    layout
        .toolbar_placements
        .iter_mut()
        .find(|bar| bar.id == "tools")
        .unwrap()
        .edge = "right".into();
    cx.update(|_, cx| editor.update(cx, |editor, cx| editor.apply_workspace_layout(&layout, cx)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("tool-rail-items").visible());
        assert_eq!(window.find("tool-rail").bounds().size.width, px(28.));
        assert_eq!(editor.read(cx).editor.doc, original);
        assert_eq!(editor.read(cx).editor.history.len(), 0);
    });
    let storyboard = emulsion_core::project::ProjectEditor::new_project(
        emulsion_core::project::ProjectKind::Storyboard,
        Document::new(64, 36),
    )
    .unwrap();
    cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.install_project(storyboard, "Storyboard".into(), window, cx);
        });
    });
    cx.run_until_parked();
    cx.update(|window, _| assert_legacy_rail(window));
}

#[gpui_kit::test]
fn photo_rail_and_sidebar_swatch_toggles_keep_real_outside_dismissal(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let (_workspace, editor, cx) = photo(cx, original.clone());
    for compact in [false, true] {
        for draw in [false, true] {
            cx.update(|window, cx| {
                cx.global_mut::<AppSettings>().0.compact_chrome = compact;
                let mut layout = editor.read(cx).workspace_snapshot();
                layout.draw_mode = draw;
                editor.update(cx, |editor, cx| editor.apply_workspace_layout(&layout, cx));
                window.refresh();
            });
            cx.run_until_parked();
            if compact {
                cx.update(|window, cx| window.click("sidebar-color-tab", cx));
                cx.run_until_parked();
            }
            let scopes = if compact {
                &["tool-rail-swatches", "sidebar-color-content"][..]
            } else {
                &["tool-rail-swatches"][..]
            };
            for &scope in scopes {
                for expected in [true, false] {
                    cx.update(|window, cx| window.within(scope).click("fg-swatch", cx));
                    cx.run_until_parked();
                    cx.update(|_, cx| {
                        assert_eq!(
                            editor.read(cx).tools.picker,
                            expected,
                            "{scope}, compact={compact}, draw={draw}"
                        );
                    });
                }
            }
            if compact {
                // Both swatches can be mounted at once; opening through one must
                // not make the other reopen a just-dismissed picker on this click.
                cx.update(|window, cx| window.within("tool-rail-swatches").click("fg-swatch", cx));
                cx.run_until_parked();
                cx.update(|window, cx| {
                    assert!(editor.read(cx).tools.picker);
                    window
                        .within("sidebar-color-content")
                        .click("fg-swatch", cx);
                });
                cx.run_until_parked();
                cx.update(|_, cx| assert!(!editor.read(cx).tools.picker));
            }
            cx.update(|window, cx| window.within("tool-rail-swatches").click("fg-swatch", cx));
            cx.run_until_parked();
            cx.update(|window, cx| {
                assert!(editor.read(cx).tools.picker);
                window.within("tool-rail").click("Move", cx);
            });
            cx.run_until_parked();
            cx.update(|_, cx| {
                let editor = editor.read(cx);
                assert!(!editor.tools.picker);
                assert_eq!(editor.editor.doc, original);
                assert_eq!(editor.editor.history.len(), 0);
            });
        }
    }
}
