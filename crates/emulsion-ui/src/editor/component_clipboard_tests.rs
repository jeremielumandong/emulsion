//! Unsupported component clipboard commands must stop before whole-layer
//! clipboard routing. These tests use GPUI's simulated clipboard.
use super::*;
use crate::app_state::AppSettings;
use ::core::prelude::v1::test;
use emulsion_core::project::{ProjectEditor, ProjectKind};
use emulsion_raster::vector::PathStyle;
use gpui_kit::{InputEvent as _, TestAppContext, VisualTestContext, test::TestWindowExt};

const TARGETS: [MaskEditTarget; 3] = [
    MaskEditTarget::RasterMask,
    MaskEditTarget::VectorMask,
    MaskEditTarget::SmartFilterMask,
];

fn document() -> Document {
    let mut doc = super::super::smart_filter_mask_tests::document();
    let path = Node::path(
        2,
        "Path owner",
        Arc::new(emulsion_raster::vector_geometry::rectangle(
            20., 20., 70., 60.,
        )),
        PathStyle {
            fill: Some([80, 130, 210, 255]),
            stroke: None,
            ..Default::default()
        },
        doc.width,
        doc.height,
    );
    let text = Node::text(
        3,
        "Text owner",
        emulsion_core::text::TextSpec {
            text: "Mask owner".into(),
            size: 18.,
            x: 20.,
            y: 30.,
            ..Default::default()
        },
        doc.width,
        doc.height,
    );
    doc.nodes.extend([path, text]);
    doc.next_id = 4;
    for node in &mut doc.nodes {
        node.mask = Some(Arc::new(Mask::from_fn(doc.width, doc.height, 0, |x, y| {
            ((x + y) % 200 + 40) as u8
        })));
        node.mask_enabled = false;
        node.mask_linked = false;
        node.vector_mask = Some(emulsion_core::VectorMask::empty(
            emulsion_core::EmptyVectorCoverage::RevealAll,
        ));
        let vector = node.vector_mask.as_mut().unwrap();
        vector.enabled = false;
        vector.linked = false;
        if let NodeKind::Smart {
            filter_mask: Some(mask),
            ..
        } = &mut node.kind
        {
            mask.enabled = false;
            mask.linked = false;
        }
    }
    doc
}

fn setup(
    cx: &mut TestAppContext,
    doc: Document,
    design: bool,
) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (workspace, cx) = crate::tests::open(cx, doc.clone());
    cx.simulate_resize(size(px(1440.), px(1200.)));
    let editor = cx.update(|window, cx| {
        if design {
            workspace.update(cx, |workspace, cx| {
                workspace.install_project(
                    ProjectEditor::new_project(ProjectKind::Design, doc).unwrap(),
                    "Clipboard targets".into(),
                    window,
                    cx,
                );
            });
        }
        let editor = workspace.read(cx).editor.clone().unwrap();
        editor.update(cx, |e, cx| {
            e.set_layer_selection(vec![1], Some(1));
            e.set_tool(Tool::Move, cx);
            window.focus(&e.canvas_focus, cx);
        });
        editor
    });
    cx.run_until_parked();
    (editor, cx)
}

fn seed_clipboard(editor: &Entity<EditorView>, stale: bool, cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.tools.mask_edit_target = MaskEditTarget::Content;
            e.copy_pixels(cx);
            assert!(cx.has_global::<ClipboardOrigin>());
            assert!(matches!(e.status, Some((_, false))));
        });
        let mut item = if stale {
            // A stale native origin plus undecodable foreign pixels must not
            // be read/decoded or replace the component-specific refusal.
            ClipboardItem::new_image(&Image::from_bytes(
                ImageFormat::Png,
                b"foreign invalid image".to_vec(),
            ))
        } else {
            cx.read_from_clipboard().unwrap()
        };
        item.entries.extend(
            ClipboardItem::new_string_with_metadata("clipboard sentinel".into(), "metadata".into())
                .entries,
        );
        cx.write_to_clipboard(item);
    });
}

struct Snapshot {
    doc: Document,
    revision: u64,
    modified: bool,
    history: Vec<(String, Document, u64)>,
    redo: bool,
    selected: Option<NodeId>,
    target: MaskEditTarget,
    clipboard: ClipboardItem,
    origin: ClipboardOrigin,
}

fn snapshot(editor: &Entity<EditorView>, cx: &App) -> Snapshot {
    let e = editor.read(cx);
    let origin = cx.global::<ClipboardOrigin>();
    Snapshot {
        doc: e.editor.doc.clone(),
        revision: e.editor.revision,
        modified: e.editor.is_modified(),
        history: e
            .editor
            .history
            .steps()
            .map(|step| (step.name.clone(), step.before.clone(), step.revision_before))
            .collect(),
        redo: e.editor.can_redo(),
        selected: e.selected,
        target: e.tools.mask_edit_target,
        clipboard: cx.read_from_clipboard().unwrap(),
        origin: ClipboardOrigin {
            image_id: origin.image_id,
            editor_id: origin.editor_id,
            page_id: origin.page_id,
            rect: origin.rect,
            objects: origin.objects.clone(),
        },
    }
}

fn unchanged(editor: &Entity<EditorView>, before: &Snapshot, cx: &App) {
    let after = snapshot(editor, cx);
    assert_eq!(
        after.doc, before.doc,
        "all node, mask and selection data survives"
    );
    assert_eq!(after.revision, before.revision);
    assert_eq!(after.modified, before.modified);
    assert_eq!(after.history, before.history);
    assert_eq!(after.redo, before.redo);
    assert_eq!(after.selected, before.selected);
    assert_eq!(after.target, before.target);
    assert_eq!(after.clipboard, before.clipboard);
    assert_eq!(after.origin.image_id, before.origin.image_id);
    assert_eq!(after.origin.editor_id, before.origin.editor_id);
    assert_eq!(after.origin.page_id, before.origin.page_id);
    assert_eq!(after.origin.rect, before.origin.rect);
    match (&after.origin.objects, &before.origin.objects) {
        (Some(after), Some(before)) => {
            assert_eq!(after.nodes, before.nodes);
            assert_eq!(after.roots, before.roots);
            assert_eq!(after.design, before.design);
            assert_eq!(after.diagram, before.diagram);
            assert_eq!(after.raw_originals, before.raw_originals);
        }
        (None, None) => {}
        _ => panic!("native clipboard payload changed"),
    }
    for (after, before) in after.doc.nodes.iter().zip(&before.doc.nodes) {
        assert_eq!(
            after.mask.as_ref().map(Arc::as_ptr),
            before.mask.as_ref().map(Arc::as_ptr),
            "raw layer-mask allocation survives"
        );
        assert_eq!(
            after
                .vector_mask
                .as_ref()
                .map(|mask| Arc::as_ptr(&mask.path)),
            before
                .vector_mask
                .as_ref()
                .map(|mask| Arc::as_ptr(&mask.path)),
            "raw vector-mask path allocation survives"
        );
        if let (
            NodeKind::Smart {
                source: a,
                cache: ca,
                filter_mask: ma,
                ..
            },
            NodeKind::Smart {
                source: b,
                cache: cb,
                filter_mask: mb,
                ..
            },
        ) = (&after.kind, &before.kind)
        {
            assert!(Arc::ptr_eq(a, b));
            assert!(Arc::ptr_eq(ca, cb));
            assert_eq!(
                ma.as_ref().map(|mask| Arc::as_ptr(&mask.pixels)),
                mb.as_ref().map(|mask| Arc::as_ptr(&mask.pixels)),
                "raw Smart filter-mask allocation survives"
            );
        }
    }
}

fn refused(editor: &Entity<EditorView>, before: &Snapshot, cx: &App) {
    unchanged(editor, before, cx);
    let status = editor.read(cx).status.as_ref().unwrap();
    assert!(status.1);
    assert!(status.0.contains("not supported yet"));
    assert!(status.0.contains("content thumbnail"));
}

fn press(key: &str, cx: &mut VisualTestContext) {
    let keys = if cfg!(target_os = "macos") {
        key.replace("ctrl-", "cmd-")
    } else {
        key.to_owned()
    };
    cx.simulate_keystrokes(&keys);
    cx.run_until_parked();
}

#[gpui_kit::test]
fn component_clipboard_keys_and_host_preserve_masks_and_stale_clipboard(cx: &mut TestAppContext) {
    for design in [false, true] {
        for selection in [false, true] {
            let mut doc = document();
            if selection {
                doc.selection = Some(Arc::new(select::rect(160, 120, 40., 36., 25., 25.)));
            } else {
                // The same routing applies to enabled/linked and to
                // disabled/unlinked masks; neither state selects content.
                for node in &mut doc.nodes {
                    node.mask_enabled = true;
                    node.mask_linked = true;
                    let vector = node.vector_mask.as_mut().unwrap();
                    vector.enabled = true;
                    vector.linked = true;
                    if let NodeKind::Smart {
                        filter_mask: Some(mask),
                        ..
                    } = &mut node.kind
                    {
                        mask.enabled = true;
                        mask.linked = true;
                    }
                }
            }
            let (editor, cx) = setup(cx, doc, design);
            seed_clipboard(&editor, selection, cx);
            for target in TARGETS {
                for panel in [false, true] {
                    let before = cx.update(|window, cx| {
                        editor.update(cx, |e, cx| {
                            e.tools.mask_edit_target = target;
                            window.focus(
                                if panel {
                                    &e.panel_focus
                                } else {
                                    &e.canvas_focus
                                },
                                cx,
                            );
                        });
                        snapshot(&editor, cx)
                    });
                    for key in ["ctrl-c", "ctrl-x", "ctrl-v", "ctrl-shift-v"] {
                        press(key, cx);
                        cx.update(|_, cx| refused(&editor, &before, cx));
                    }
                }
                cx.update(|_, cx| {
                    let before = snapshot(&editor, cx);
                    for action in ["copy", "cut", "paste"] {
                        let result = editor.update(cx, |e, cx| {
                            e.editor_host_action(
                                emulsion_mcp::editor_host_tools::Action::Clipboard(action.into()),
                                cx,
                            )
                        });
                        assert!(result.unwrap_err().contains("content thumbnail"));
                        refused(&editor, &before, cx);
                    }
                });
            }
        }
    }
}

#[gpui_kit::test]
fn component_clipboard_cut_cannot_remove_native_text_or_path_owners(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, document(), false);
    seed_clipboard(&editor, false, cx);
    for owner in [2, 3] {
        for target in [MaskEditTarget::RasterMask, MaskEditTarget::VectorMask] {
            let before = cx.update(|window, cx| {
                editor.update(cx, |e, cx| {
                    e.set_layer_selection(vec![owner], Some(owner));
                    e.tools.mask_edit_target = target;
                    window.focus(&e.panel_focus, cx);
                });
                snapshot(&editor, cx)
            });
            press("ctrl-x", cx);
            cx.update(|_, cx| refused(&editor, &before, cx));
            cx.update(|_, cx| {
                let result = editor.update(cx, |e, cx| e.clipboard_host("cut", cx));
                assert!(result.unwrap_err().contains("content thumbnail"));
                refused(&editor, &before, cx);
            });
        }
    }
}

#[gpui_kit::test]
fn component_clipboard_stale_targets_and_text_only_clipboard_never_fall_back(
    cx: &mut TestAppContext,
) {
    let mut doc = document();
    doc.nodes[0].mask = None;
    doc.nodes[0].vector_mask = None;
    if let NodeKind::Smart { filter_mask, .. } = &mut doc.nodes[0].kind {
        *filter_mask = None;
    }
    let (editor, cx) = setup(cx, doc, false);
    seed_clipboard(&editor, false, cx);
    cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("text sentinel".into())));
    for selected in [Some(1), Some(999), None] {
        for target in TARGETS {
            let before = cx.update(|_, cx| {
                editor.update(cx, |e, _| {
                    e.selected = selected;
                    e.tools.mask_edit_target = target;
                });
                snapshot(&editor, cx)
            });
            cx.dispatch_action(crate::actions::CopyPixels);
            cx.update(|_, cx| refused(&editor, &before, cx));
            cx.dispatch_action(crate::actions::CutPixels);
            cx.update(|_, cx| refused(&editor, &before, cx));
            cx.dispatch_action(crate::actions::PastePixels);
            cx.update(|_, cx| refused(&editor, &before, cx));
            cx.dispatch_action(crate::actions::PasteInPlace);
            cx.update(|_, cx| refused(&editor, &before, cx));
        }
    }
}

#[gpui_kit::test]
fn component_clipboard_keeps_pending_edit_guards_and_existing_redo(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, document(), false);
    seed_clipboard(&editor, false, cx);
    cx.update(|_, cx| {
        editor.update(cx, |e, cx| {
            e.execute(
                Command::Rename {
                    id: 1,
                    name: "First edit".into(),
                },
                cx,
            );
            e.execute(
                Command::Rename {
                    id: 1,
                    name: "Redo survives".into(),
                },
                cx,
            );
            e.undo(cx);
            e.tools.mask_edit_target = MaskEditTarget::VectorMask;
        });
        let before = snapshot(&editor, cx);
        assert!(before.redo);
        editor.update(cx, |e, cx| {
            e.assistant.running = true;
            e.copy_pixels(cx);
            assert_eq!(
                e.status.as_ref().unwrap().0.as_ref(),
                t!("editor.clipboard.finish_edit")
            );
            e.cut_pixels(cx);
            e.paste_pixels(cx);
            e.paste_in_place(cx);
            // The assistant's host route may bypass its own busy flag, but
            // still cannot reinterpret a component as owning-layer content.
            assert!(
                e.clipboard_host("cut", cx)
                    .unwrap_err()
                    .contains("content thumbnail")
            );
            e.assistant.running = false;
        });
        refused(&editor, &before, cx);
        editor.update(cx, |e, cx| {
            e.editor.begin("Pending edit");
            e.copy_pixels(cx);
            e.cut_pixels(cx);
            e.paste_pixels(cx);
            e.paste_in_place(cx);
            assert!(
                e.clipboard_host("cut", cx)
                    .unwrap_err()
                    .contains("Finish the active edit")
            );
            assert!(e.editor.in_transaction());
            e.editor.cancel();
        });
        unchanged(&editor, &before, cx);
        editor.update(cx, |e, cx| e.redo(cx));
        assert_eq!(
            editor.read(cx).editor.doc.node(1).unwrap().name,
            "Redo survives"
        );
    });
}

#[gpui_kit::test]
fn component_clipboard_menus_keep_active_thumbnail_and_disable_layer_actions(
    cx: &mut TestAppContext,
) {
    for design in [false, true] {
        let (editor, cx) = setup(cx, document(), design);
        seed_clipboard(&editor, false, cx);
        cx.update(|window, cx| {
            cx.global_mut::<AppSettings>().0.compact_chrome = true;
            window.refresh();
        });
        for target in TARGETS {
            cx.update(|_, cx| editor.update(cx, |e, _| e.tools.mask_edit_target = target));
            cx.run_until_parked();
            for menu in ["edit", "canvas"] {
                let (before, status) = cx.update(|_, cx| {
                    assert_eq!(editor.read(cx).tools.mask_edit_target, target);
                    (snapshot(&editor, cx), editor.read(cx).status.clone())
                });
                open_clipboard_menu(&editor, menu, cx);
                let labels = [t!("edit.cut"), t!("edit.copy"), t!("edit.paste")];
                for label in labels
                    .iter()
                    .map(|label| label.as_ref())
                    .chain((menu == "edit").then_some("Paste in Place"))
                {
                    // Popup rows do not expose aria_disabled in snapshots.
                    // Click the real row instead: a disabled item must not
                    // dispatch even the guarded action or change its status.
                    click_clipboard_menu_item(label, cx);
                    cx.update(|_, cx| {
                        unchanged(&editor, &before, cx);
                        assert_eq!(editor.read(cx).status, status, "{menu} {label} dispatched");
                    });
                }
                press("escape", cx);
            }
        }
        // Prove that the same menu lookup and pointer path can dispatch an
        // enabled content action, rather than merely missing every row.
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                e.set_mask_edit_target(MaskEditTarget::Content, cx)
            });
        });
        for menu in ["edit", "canvas"] {
            let before = cx.update(|_, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string("enabled Copy sentinel".into()));
                snapshot(&editor, cx)
            });
            open_clipboard_menu(&editor, menu, cx);
            click_clipboard_menu_item(&t!("edit.copy"), cx);
            cx.update(|_, cx| {
                let after = snapshot(&editor, cx);
                assert_eq!(after.doc, before.doc);
                assert_eq!(after.revision, before.revision);
                assert_eq!(after.modified, before.modified);
                assert_eq!(after.history, before.history);
                assert_eq!(after.redo, before.redo);
                assert_eq!(after.selected, before.selected);
                assert_eq!(after.target, MaskEditTarget::Content);
                assert_ne!(after.clipboard, before.clipboard, "{menu} Copy must dispatch");
                assert!(after.clipboard.entries.iter().any(|entry| {
                    matches!(entry, ClipboardEntry::Image(image) if image.id == after.origin.image_id)
                }));
                assert_eq!(after.clipboard.text(), None);
                assert!(matches!(editor.read(cx).status, Some((_, false))));
            });
        }
        // A new Design project starts with its inspector closed. Open the
        // actual pane before expecting its layer-name hit targets.
        if design {
            cx.update(|window, cx| {
                assert!(window.find("design-inspector-toggle").visible());
                window.click("design-inspector-toggle", cx);
            });
            cx.run_until_parked();
        }
        // The row center can land on a mask thumbnail. Right-click its name
        // explicitly to verify the distinct layer-content context.
        for id in [1_u64, 2] {
            cx.update(|window, cx| {
                assert!(window.find(("layer-name", id)).visible());
                window.right_click(("layer-name", id), cx);
            });
            cx.run_until_parked();
            cx.update(|_, cx| {
                let e = editor.read(cx);
                assert_eq!(e.selected, Some(id));
                assert_eq!(e.tools.mask_edit_target, MaskEditTarget::Content);
                assert_eq!(e.layer_panel.mask_context, None);
            });
            press("escape", cx);
        }
    }
}

fn open_clipboard_menu(editor: &Entity<EditorView>, menu: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| match menu {
        "edit" => window.click("edit-menu-button", cx),
        _ => {
            let position = editor.read(cx).canvas_bounds.get().unwrap().center();
            window.dispatch_event(
                MouseDownEvent {
                    position,
                    button: MouseButton::Right,
                    modifiers: Modifiers::none(),
                    click_count: 1,
                    first_mouse: false,
                }
                .to_platform_input(),
                cx,
            );
        }
    });
    cx.run_until_parked();
}

fn click_clipboard_menu_item(label: &str, cx: &mut VisualTestContext) {
    let position = cx.update(|window, _| {
        let rows: Vec<_> = gpui_kit::base::test_support::snapshots(window)
            .into_iter()
            .filter(|item| {
                item.visible()
                    && item.label() == Some(label)
                    && item.path().contains(&ElementId::from("popup-menu"))
            })
            .collect();
        assert_eq!(rows.len(), 1, "one visible menu item {label}");
        rows[0].bounds().center()
    });
    cx.simulate_click(position, Modifiers::none());
    cx.run_until_parked();
}

#[gpui_kit::test]
fn component_target_does_not_capture_text_input_copy_cut_or_paste(cx: &mut TestAppContext) {
    let (editor, cx) = setup(cx, document(), false);
    let original = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
    cx.update(|window, cx| {
        editor.update(cx, |e, cx| {
            e.tools.mask_edit_target = MaskEditTarget::SmartFilterMask;
            e.start_rename(1, window, cx);
        });
    });
    cx.run_until_parked();
    press("ctrl-a ctrl-c", cx);
    cx.update(|_, cx| {
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some("Smart")
        );
    });
    press("ctrl-x", cx);
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.renaming.as_ref().unwrap().1.read(cx).value().as_ref(), "");
        assert_eq!(e.editor.doc, original);
    });
    press("ctrl-v", cx);
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(
            e.renaming.as_ref().unwrap().1.read(cx).value().as_ref(),
            "Smart"
        );
        assert_eq!(e.tools.mask_edit_target, MaskEditTarget::SmartFilterMask);
        assert_eq!(e.editor.doc, original);
        assert!(e.editor.history.is_empty());
    });
}

#[gpui_kit::test]
fn content_clipboard_still_copies_cuts_and_pastes_editable_owners(cx: &mut TestAppContext) {
    for design in [false, true] {
        let (editor, cx) = setup(cx, document(), design);
        let original = cx.update(|window, cx| {
            editor.update(cx, |e, cx| {
                e.set_layer_selection(vec![2], Some(2));
                window.focus(&e.canvas_focus, cx);
            });
            editor.read(cx).editor.doc.clone()
        });
        press("ctrl-c", cx);
        cx.update(|_, cx| {
            assert_eq!(editor.read(cx).editor.doc, original);
            assert_eq!(
                cx.global::<ClipboardOrigin>()
                    .objects
                    .as_ref()
                    .unwrap()
                    .roots,
                vec![2]
            );
        });
        press("ctrl-x", cx);
        cx.update(|_, cx| assert!(editor.read(cx).editor.doc.node(2).is_none()));
        press("ctrl-shift-v", cx);
        cx.update(|_, cx| {
            let e = editor.read(cx);
            assert_eq!(e.editor.doc.nodes.len(), original.nodes.len());
            let pasted = e.editor.doc.node(e.selected.unwrap()).unwrap();
            assert!(matches!(pasted.kind, NodeKind::Path { .. }));
            assert!(Arc::ptr_eq(
                pasted.mask.as_ref().unwrap(),
                original.node(2).unwrap().mask.as_ref().unwrap(),
            ));
            assert_eq!(pasted.vector_mask, original.node(2).unwrap().vector_mask);
        });
    }
}
