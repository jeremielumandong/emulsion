//! Complete Pen paths through actual Options pointer clicks, then keep using
//! canvas and workspace shortcuts without a corrective click or manual focus.
use super::*;
use crate::app_state::AppSettings;
use crate::tests::open;
use core::prelude::v1::test;
use emulsion_core::{EmptyVectorCoverage, VectorMask};
use emulsion_raster::vector::{Anchor, Path, SubPath};
use gpui_kit::component::WindowExt;
use gpui_kit::test::TestWindowExt;

fn document() -> Document {
    let mut document = Document::new(256, 192);
    let mut node = Node::raster(
        1,
        "Photo",
        Arc::new(Raster::solid(256, 192, [0.2, 0.3, 0.4, 1.])),
        Placement::default(),
    );
    node.mask = Some(Arc::new(Mask::white(256, 192)));
    let mut mask = VectorMask::empty(EmptyVectorCoverage::RevealAll);
    mask.path = Arc::new(Path {
        subpaths: vec![SubPath {
            anchors: [(12., 12.), (22., 12.), (18., 22.)]
                .into_iter()
                .map(Anchor::corner)
                .collect(),
            closed: true,
        }],
    });
    node.vector_mask = Some(mask);
    document.nodes.push(node);
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

fn completion_shortcuts(cx: &mut TestAppContext, close: bool, compact: bool, narrow: bool) {
    for vector in [false, true] {
        let original = document();
        let (workspace, cx) = open(cx, original.clone());
        cx.simulate_resize(size(px(1440.), px(1100.)));
        let folder = tempfile::tempdir().unwrap();
        let editor = cx.update(|window, cx| {
            cx.global_mut::<AppSettings>().0.compact_chrome = compact;
            workspace.update(cx, |workspace, _| {
                workspace.home_state.projects.catalog_root = Some(folder.path().join("catalog"));
            });
            let editor = workspace.read(cx).editor.clone().unwrap();
            editor.update(cx, |e, cx| {
                e.snap = false;
                e.set_tool(Tool::Hand, cx);
            });
            window.refresh();
            editor
        });
        cx.run_until_parked();
        click(
            (
                if vector {
                    "layer-vector-mask"
                } else {
                    "layer-content"
                },
                1_u64,
            ),
            cx,
        );
        if !vector {
            press("p", cx);
        }
        cx.update(|_, cx| {
            let e = editor.read(cx);
            assert_eq!(e.tool, Tool::Pen, "vector thumbnail activates Pen");
            assert_eq!(
                e.tools.mask_edit_target,
                if vector {
                    MaskEditTarget::VectorMask
                } else {
                    MaskEditTarget::Content
                }
            );
        });
        let points = [(60., 50.), (180., 50.), (110., 120.)];
        for point in points {
            let at = cx.update(|_, cx| editor.read(cx).doc_to_window(point).unwrap());
            cx.simulate_click(at, Modifiers::none());
            cx.run_until_parked();
        }
        cx.update(|_, cx| {
            let e = editor.read(cx);
            assert_eq!(e.tools.pen.building.as_ref().unwrap().anchors.len(), 3);
            assert_eq!(e.editor.doc, original);
            assert!(e.editor.history.is_empty());
        });
        let control = if close { "pen-close" } else { "pen-finish" };
        if narrow {
            cx.simulate_resize(size(px(400.), px(900.)));
            cx.run_until_parked();
            cx.update(|window, _| {
                assert!(
                    window
                        .within("editor-tool-options")
                        .try_find(control)
                        .is_none()
                );
            });
            click("tool-options-more", cx);
            cx.update(|window, _| {
                assert!(
                    window
                        .within("tool-options-overflow-content")
                        .find(control)
                        .visible()
                );
            });
        }
        click(control, cx);
        let completed = cx.update(|window, cx| {
            let e = editor.read(cx);
            assert!(
                e.canvas_focus.is_focused(window),
                "completion must retain a live focus scope"
            );
            assert!(window.try_find("tool-options-overflow-content").is_none());
            assert!(e.tools.pen.building.is_none());
            assert_eq!(
                e.tools.mask_edit_target,
                if vector {
                    MaskEditTarget::VectorMask
                } else {
                    MaskEditTarget::Content
                }
            );
            assert_eq!(e.editor.history.len(), 1, "completion is still one edit");
            assert!(e.editor.is_modified());
            assert_eq!(e.editor.doc.node(1).unwrap().kind, original.nodes[0].kind);
            assert!(Arc::ptr_eq(
                e.editor.doc.node(1).unwrap().mask.as_ref().unwrap(),
                original.nodes[0].mask.as_ref().unwrap(),
            ));
            let path = if vector {
                assert_eq!(e.editor.doc.nodes.len(), 1);
                let path = &e
                    .editor
                    .doc
                    .node(1)
                    .unwrap()
                    .vector_mask
                    .as_ref()
                    .unwrap()
                    .path;
                assert_eq!(path.subpaths.len(), 2);
                assert_eq!(
                    path.subpaths[0],
                    original.nodes[0]
                        .vector_mask
                        .as_ref()
                        .unwrap()
                        .path
                        .subpaths[0]
                );
                path
            } else {
                assert_eq!(e.editor.doc.nodes.len(), 2);
                assert_eq!(e.editor.doc.node(1).unwrap(), &original.nodes[0]);
                let NodeKind::Path { path, .. } =
                    &e.editor.doc.node(e.selected.unwrap()).unwrap().kind
                else {
                    panic!("content completion must create an editable Path");
                };
                path
            };
            let subpath = path.subpaths.last().unwrap();
            assert_eq!(subpath.closed, close);
            assert_eq!(subpath.anchors.len(), 3);
            for (anchor, expected) in subpath.anchors.iter().zip(points) {
                assert!((anchor.p.0 - expected.0).abs() < 0.1);
                assert!((anchor.p.1 - expected.1).abs() < 0.1);
            }
            e.editor.doc.clone()
        });
        // These must be actual keystrokes: dispatch_action would bypass the
        // missing focus route that caused the native failure.
        press("v", cx);
        cx.update(|_, cx| assert_eq!(editor.read(cx).tool, Tool::Move));
        press("g", cx);
        cx.update(|_, cx| {
            assert_eq!(editor.read(cx).tool, Tool::Brush);
            assert_eq!(editor.read(cx).tools.paint, PaintKind::Gradient);
            assert_eq!(editor.read(cx).editor.doc, completed);
        });
        press("ctrl-s", cx);
        assert!(
            cx.did_prompt_for_new_path(),
            "Save must reach the workspace"
        );
        let path = folder.path().join("pen-completed.ora");
        cx.simulate_new_path_selection(|_| Some(path.clone()));
        cx.run_until_parked();
        assert!(path.is_file());
        let reopened = emulsion_io::open(&path).unwrap();
        assert_eq!(
            (reopened.width, reopened.height),
            (completed.width, completed.height)
        );
        assert_eq!(reopened.nodes.len(), completed.nodes.len());
        for expected in &completed.nodes {
            let actual = reopened.node(expected.id).unwrap();
            assert_eq!(actual.name, expected.name);
            assert_eq!(actual.vector_mask, expected.vector_mask);
            assert_eq!(
                actual.mask.as_ref().map(|mask| mask.to_gray8()),
                expected.mask.as_ref().map(|mask| mask.to_gray8()),
            );
            match (&actual.kind, &expected.kind) {
                (
                    NodeKind::Raster {
                        raster: a,
                        placement: ap,
                    },
                    NodeKind::Raster {
                        raster: b,
                        placement: bp,
                    },
                ) => {
                    assert_eq!(ap, bp);
                    assert_eq!(a.to_srgba8(), b.to_srgba8());
                }
                (
                    NodeKind::Path {
                        path: a, style: sa, ..
                    },
                    NodeKind::Path {
                        path: b, style: sb, ..
                    },
                ) => {
                    assert_eq!(a, b);
                    assert_eq!(sa, sb);
                }
                _ => panic!("Save must retain the editable node type"),
            }
        }
        cx.update(|_, cx| {
            assert_eq!(editor.read(cx).editor.path.as_ref(), Some(&path));
            assert!(!editor.read(cx).editor.is_modified());
        });
        press("ctrl-shift-s", cx);
        assert!(
            cx.did_prompt_for_new_path(),
            "Save As must remain reachable"
        );
        cx.simulate_new_path_selection(|_| None);
        cx.run_until_parked();
        press("ctrl-z", cx);
        cx.update(|_, cx| {
            let e = editor.read(cx);
            assert_eq!(
                e.editor.doc.nodes, original.nodes,
                "one Undo restores the original geometry"
            );
            assert!(e.editor.is_modified());
        });
        press("ctrl-shift-z", cx);
        cx.update(|_, cx| assert_eq!(editor.read(cx).editor.doc.nodes, completed.nodes));
    }
}

#[gpui_kit::test]
fn photo_pen_close_options_restore_shortcuts_save_and_undo(cx: &mut TestAppContext) {
    for compact in [false, true] {
        completion_shortcuts(cx, true, compact, false);
    }
}

#[gpui_kit::test]
fn photo_pen_finish_options_restore_shortcuts_save_and_undo(cx: &mut TestAppContext) {
    for compact in [false, true] {
        completion_shortcuts(cx, false, compact, false);
    }
}

#[gpui_kit::test]
fn photo_pen_overflow_completion_dismisses_popup_and_restores_shortcuts(cx: &mut TestAppContext) {
    for close in [false, true] {
        completion_shortcuts(cx, close, true, true);
    }
}

#[gpui_kit::test]
fn photo_pen_options_focus_restore_preserves_text_and_modal_ownership(cx: &mut TestAppContext) {
    let original = document();
    let (workspace, cx) = open(cx, original.clone());
    cx.simulate_resize(size(px(1440.), px(1100.)));
    let editor = cx.update(|window, cx| {
        let editor = workspace.read(cx).editor.clone().unwrap();
        editor.update(cx, |e, cx| {
            e.set_tool(Tool::Pen, cx);
            e.set_layer_selection(vec![1], Some(1));
            e.rename_layer(window, cx);
        });
        editor
    });
    cx.run_until_parked();
    let input = cx.update(|window, cx| window.focused_input(cx).unwrap());
    // default_value leaves the rename caret at the start. Explicitly select
    // the existing name and ensure the guard preserves both focus and selection.
    press("ctrl-a", cx);
    cx.update(|window, cx| {
        assert_eq!(input.value(cx).as_str(), "Photo");
        let rename = editor.read(cx).renaming.as_ref().unwrap().1.clone();
        assert_eq!(rename.read(cx).selected_range(), 0..5);
        editor.update(cx, |e, cx| {
            e.restore_photo_canvas_after_pen_options(window, cx)
        });
        assert_eq!(window.focused_input(cx).as_ref(), Some(&input));
        assert_eq!(rename.read(cx).selected_range(), 0..5);
    });
    cx.simulate_input("vg");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(input.value(cx).as_str(), "vg");
        assert_eq!(window.focused_input(cx).as_ref(), Some(&input));
        assert_eq!(editor.read(cx).tool, Tool::Pen);
        assert_eq!(editor.read(cx).editor.doc, original);
    });
    // Restore the original name before committing the text field so the modal
    // check starts from the same document and does not include a rename edit.
    press("ctrl-a", cx);
    cx.simulate_input("Photo");
    press("enter", cx);
    cx.update(|window, cx| {
        editor.update(cx, |e, cx| e.open_layer_styles_dialog(1, window, cx));
    });
    cx.run_until_parked();
    let focused = cx.update(|window, cx| window.focused(cx));
    cx.update(|window, cx| {
        assert!(window.has_active_dialog(cx));
        editor.update(cx, |e, cx| {
            e.restore_photo_canvas_after_pen_options(window, cx)
        });
        assert_eq!(window.focused(cx), focused);
        assert!(!editor.read(cx).canvas_focus.is_focused(window));
        assert_eq!(editor.read(cx).editor.doc, original);
    });
}

#[gpui_kit::test]
fn photo_pen_options_focus_restore_leaves_other_workspaces_unchanged(cx: &mut TestAppContext) {
    use emulsion_core::project::{ProjectEditor, ProjectKind};
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
                window.focus(&e.panel_focus, cx);
                let before = e.editor.doc.clone();
                e.restore_photo_canvas_after_pen_options(window, cx);
                assert!(e.panel_focus.is_focused(window));
                assert_eq!(e.editor.doc, before);
                assert!(e.editor.history.is_empty());
            });
        });
        cx.run_until_parked();
    }
}
