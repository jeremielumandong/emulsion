//! Dismissing the general Photo flyout must not strand keyboard focus in an
//! unmounted filter chip. Drive the reported route with native pointer events.
use super::*;
use crate::app_state::AppSettings;
use crate::tests::open;
use core::prelude::v1::test;
use emulsion_core::project::{ProjectEditor, ProjectKind};
use emulsion_filters::Filter;
use gpui_kit::test::TestWindowExt;

fn document() -> Document {
    let mut document = Document::new(256, 192);
    document.nodes.push(Node::smart(
        1,
        "Photo",
        Arc::new(Raster::from_fn(64, 48, [0; 4], |x, _| {
            if x < 32 {
                [65535, 0, 0, 65535]
            } else {
                [0, 0, 65535, 65535]
            }
        })),
        vec![
            Filter::GaussianBlur { radius: 1. },
            Filter::BoxBlur { radius: 2. },
        ],
        Placement::at(80., 60.),
    ));
    document.next_id = 2;
    document
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

fn click(id: impl Into<ElementId>, cx: &mut VisualTestContext) {
    let at = cx.update(|window, _| {
        let element = window.find(id);
        assert!(element.visible());
        element.bounds().center()
    });
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
}

fn click_in_flyout(id: impl Into<ElementId>, cx: &mut VisualTestContext) {
    let id = id.into();
    // Scroll the real content, rather than clicking a clipped/offscreen row or
    // calling its handler directly. Both chrome sizes can overflow vertically.
    for _ in 0..20 {
        let visible = cx.update(|window, _| {
            let panel = window.find("photo-shortcut-panel").bounds();
            let target = window.find(id.clone());
            target.visible()
                && target.bounds().top() >= panel.top() + px(45.)
                && target.bounds().bottom() <= panel.bottom() - px(10.)
        });
        if visible {
            click(id, cx);
            return;
        }
        cx.update(|window, cx| {
            window.scroll(
                "photo-shortcut-panel",
                ScrollDelta::Pixels(point(px(0.), px(-160.))),
                cx,
            );
        });
        cx.run_until_parked();
    }
    panic!("flyout control {id:?} did not scroll into view");
}

#[gpui_kit::test]
fn photo_flyout_filter_remove_close_restores_gradient_save_and_undo(cx: &mut TestAppContext) {
    for compact in [false, true] {
        let original = document();
        let (workspace, cx) = open(cx, original.clone());
        cx.simulate_resize(size(px(1440.), px(1000.)));
        let folder = tempfile::tempdir().unwrap();
        let editor = cx.update(|window, cx| {
            cx.global_mut::<AppSettings>().0.compact_chrome = compact;
            workspace.update(cx, |workspace, _| {
                workspace.home_state.projects.catalog_root = Some(folder.path().join("catalog"));
            });
            window.refresh();
            workspace.read(cx).editor.clone().unwrap()
        });
        cx.run_until_parked();
        click(("layer-content", 1_u64), cx);
        press("v", cx);
        click(("photo-shortcut", 0_usize), cx);
        click_in_flyout("photo-layer-details", cx);
        click_in_flyout(("filter-del", 1_usize), cx);
        let edited = cx.update(|window, cx| {
            let e = editor.read(cx);
            let NodeKind::Smart {
                filters, source, ..
            } = &e.editor.doc.node(1).unwrap().kind
            else {
                panic!("the layer must remain editable");
            };
            assert_eq!(filters, &[Filter::GaussianBlur { radius: 1. }]);
            let NodeKind::Smart {
                source: original_source,
                ..
            } = &original.nodes[0].kind
            else {
                unreachable!();
            };
            assert!(Arc::ptr_eq(source, original_source));
            assert_eq!(e.tool, Tool::Move);
            assert!(e.sidebar_layout.flyout_open);
            assert!(
                !e.canvas_focus.is_focused(window),
                "exercise the lost-focus route"
            );
            assert_eq!(e.editor.history.len(), 1);
            assert!(e.editor.is_modified());
            e.editor.doc.clone()
        });
        click("photo-shortcut-close", cx);
        cx.update(|window, cx| {
            let e = editor.read(cx);
            assert!(!e.sidebar_layout.flyout_open);
            assert!(window.try_find("photo-shortcut-panel").is_none());
            assert!(e.canvas_focus.is_focused(window));
            assert_eq!(e.editor.doc, edited);
            assert_eq!(e.editor.history.len(), 1, "dismissal adds no edit");
        });
        // No canvas click, manual focus or dispatch_action may repair this path.
        press("g", cx);
        cx.update(|_, cx| {
            assert_eq!(editor.read(cx).tool, Tool::Brush);
            assert_eq!(editor.read(cx).tools.paint, PaintKind::Gradient);
        });
        press("ctrl-s", cx);
        assert!(
            cx.did_prompt_for_new_path(),
            "Save must reach the workspace"
        );
        let path = folder.path().join("removed-filter.ora");
        cx.simulate_new_path_selection(|_| Some(path.clone()));
        cx.run_until_parked();
        assert!(path.is_file());
        let reopened = emulsion_io::open(&path).unwrap();
        assert_eq!(reopened.nodes.len(), edited.nodes.len());
        let (
            NodeKind::Smart {
                filters, source, ..
            },
            NodeKind::Smart {
                filters: expected_filters,
                source: expected_source,
                ..
            },
        ) = (&reopened.nodes[0].kind, &edited.nodes[0].kind)
        else {
            panic!("Save must retain the Smart layer");
        };
        assert_eq!(filters, expected_filters);
        assert_eq!(source.to_srgba8(), expected_source.to_srgba8());
        cx.update(|_, cx| {
            assert!(!editor.read(cx).editor.is_modified());
            assert_eq!(editor.read(cx).editor.doc, edited);
        });
        press("ctrl-z", cx);
        cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, original));
        press("ctrl-shift-z", cx);
        cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc, edited));
        // Reopening and dismissing again must keep the same route and history.
        click(("photo-shortcut", 0_usize), cx);
        click("photo-shortcut-close", cx);
        press("v", cx);
        cx.update(|window, cx| {
            let e = editor.read(cx);
            assert_eq!(e.tool, Tool::Move);
            assert!(e.canvas_focus.is_focused(window));
            assert_eq!(e.editor.doc, edited);
            assert_eq!(e.editor.history.len(), 1);
        });
    }
}

#[gpui_kit::test]
fn photo_flyout_close_preserves_text_and_modal_focus(cx: &mut TestAppContext) {
    let original = document();
    let (workspace, cx) = open(cx, original.clone());
    cx.simulate_resize(size(px(1440.), px(1000.)));
    let editor = cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap());
    click(("layer-content", 1_u64), cx);
    press("v", cx);
    click(("photo-shortcut", 0_usize), cx);
    cx.update(|window, cx| editor.update(cx, |e, cx| e.rename_layer(window, cx)));
    cx.run_until_parked();
    let input = cx.update(|window, cx| window.focused_input(cx).unwrap());
    // default_value leaves the rename caret at the start. Explicitly select
    // the name so the pointer close must preserve both focus and selection.
    press("ctrl-a", cx);
    let rename = cx.update(|window, cx| {
        assert_eq!(window.focused_input(cx).as_ref(), Some(&input));
        assert_eq!(input.value(cx).as_str(), "Photo");
        let rename = editor.read(cx).renaming.as_ref().unwrap().1.clone();
        assert_eq!(rename.read(cx).selected_range(), 0..5);
        rename
    });
    click("photo-shortcut-close", cx);
    cx.update(|window, cx| {
        assert_eq!(window.focused_input(cx).as_ref(), Some(&input));
        assert_eq!(rename.read(cx).selected_range(), 0..5);
        assert!(!editor.read(cx).sidebar_layout.flyout_open);
    });
    cx.simulate_input("vg");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(input.value(cx).as_str(), "vg");
        assert_eq!(window.focused_input(cx).as_ref(), Some(&input));
        assert_eq!(editor.read(cx).tool, Tool::Move);
        assert_eq!(editor.read(cx).editor.doc, original);
    });
    press("ctrl-a", cx);
    cx.simulate_input("Photo");
    press("enter", cx);
    click(("photo-shortcut", 0_usize), cx);
    cx.update(|window, cx| {
        editor.update(cx, |e, cx| e.open_layer_styles_dialog(1, window, cx));
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.has_active_dialog(cx));
        let focused = window.focused(cx);
        // A modal occludes the close button; invoke only the close-boundary
        // guard here to prove it never steals a modal's ownership.
        editor.update(cx, |e, cx| e.close_canvas_panel(window, cx));
        assert_eq!(window.focused(cx), focused);
        assert!(!editor.read(cx).canvas_focus.is_focused(window));
        assert_eq!(editor.read(cx).editor.doc, original);
        assert!(editor.read(cx).editor.history.is_empty());
    });
}

#[gpui_kit::test]
fn photo_flyout_close_keeps_other_workspaces_focus_unchanged(cx: &mut TestAppContext) {
    let (workspace, cx) = open(cx, document());
    for kind in [
        None,
        Some(ProjectKind::Design),
        Some(ProjectKind::Storyboard),
    ] {
        cx.update(|window, cx| {
            if let Some(kind) = kind {
                let project = ProjectEditor::new_project(kind, document()).unwrap();
                workspace.update(cx, |workspace, cx| {
                    workspace.install_project(project, "Other workspace".into(), window, cx);
                });
            }
            let editor = workspace.read(cx).editor.clone().unwrap();
            editor.update(cx, |e, cx| {
                if kind.is_none() {
                    e.toggle_draw_mode(cx);
                }
                assert!(!e.is_photo_workflow());
                e.sidebar_layout.flyout_open = true;
                window.focus(&e.panel_focus, cx);
                let before = e.editor.doc.clone();
                e.close_canvas_panel(window, cx);
                assert!(!e.sidebar_layout.flyout_open);
                assert!(e.panel_focus.is_focused(window));
                assert_eq!(e.editor.doc, before);
                assert!(e.editor.history.is_empty());
            });
        });
        cx.run_until_parked();
    }
}
