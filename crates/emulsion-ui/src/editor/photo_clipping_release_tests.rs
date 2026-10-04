use super::*;
use crate::tests::open;
use core::prelude::v1::test;
use gpui_kit::test::TestWindowExt;

fn stack() -> Document {
    let mut doc = Document::new(16, 16);
    for id in 1..=6 {
        let mut node = Node::new(id, format!("Layer {id}"), NodeKind::Fill { rgba: [255; 4] });
        node.clip_to = match id {
            2 => Some(1),
            3 => Some(2),
            4 => Some(1), // Imported direct link to the same resolved base.
            6 => Some(5),
            _ => None,
        };
        doc.nodes.push(node);
    }
    doc.next_id = 7;
    doc.validate().unwrap();
    doc
}

#[test]
fn photo_clip_release_resolves_chains_direct_links_and_stops_at_group_boundary() {
    let doc = stack();
    assert_eq!(photo_clip_release_ids(&doc, 2), vec![2, 3, 4]);
    assert_eq!(photo_clip_release_ids(&doc, 3), vec![3, 4]);
    assert_eq!(photo_clip_release_ids(&doc, 4), vec![4]);
    assert!(photo_clip_release_ids(&doc, 1).is_empty());
    assert!(photo_clip_release_ids(&doc, 99).is_empty());
    assert_eq!(photo_clip_release_ids(&doc, 6), vec![6]);
    let mut other_group = doc.clone();
    other_group.nodes[3].clip_to = Some(2);
    other_group.nodes[2].clip_to = None;
    assert_eq!(photo_clip_release_ids(&other_group, 2), vec![2]);
}

#[gpui_kit::test]
fn photo_clip_release_is_atomic_undoable_and_native_persistent(cx: &mut TestAppContext) {
    let original = stack();
    let (workspace, cx) = open(cx, original.clone());
    let view = cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            assert!(e.is_photo_workflow());
            for selected in [4, 3, 2] {
                e.set_layer_selection(vec![selected], Some(selected));
                e.toggle_clipping_mask(cx);
                for id in 2..=4 {
                    assert_eq!(
                        e.editor.doc.node(id).unwrap().clip_to,
                        if id >= selected {
                            None
                        } else {
                            original.node(id).unwrap().clip_to
                        }
                    );
                }
                assert_eq!(e.editor.doc.node(6), original.node(6));
                assert_eq!(e.selected, Some(selected));
                assert_eq!(e.editor.history.len(), 1);
                let changed = e.editor.doc.clone();
                e.undo(cx);
                assert_eq!(e.editor.doc, original);
                e.redo(cx);
                assert_eq!(e.editor.doc, changed);
                let dir = tempfile::tempdir().unwrap();
                let file = dir.path().join("clip-release.ora");
                emulsion_io::ora::write(&changed, &file).unwrap();
                let reopened = emulsion_io::ora::read(&file).unwrap();
                for node in &changed.nodes {
                    assert_eq!(reopened.node(node.id).unwrap().clip_to, node.clip_to);
                }
                e.undo(cx);
            }
        })
    });
}

#[gpui_kit::test]
fn photo_clip_release_rejects_locked_members_multiselection_and_busy_state(
    cx: &mut TestAppContext,
) {
    let mut original = stack();
    original.node_mut(4).unwrap().locked = true;
    let (workspace, cx) = open(cx, original.clone());
    let view = cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.set_layer_selection(vec![2], Some(2));
            assert!(e.clipping_mask_commands(2).is_none());
            e.toggle_clipping_mask(cx);
            assert_eq!(e.editor.doc, original);
            assert_eq!(e.editor.history.len(), 0);
            e.editor.doc.node_mut(4).unwrap().locked = false;
            let unlocked = e.editor.doc.clone();
            e.set_layer_selection(vec![2, 3], Some(2));
            e.toggle_clipping_mask(cx);
            assert_eq!(e.editor.doc, unlocked);
            e.set_layer_selection(vec![2], Some(2));
            e.editor.begin("Unrelated preview");
            e.toggle_clipping_mask(cx);
            assert_eq!(e.editor.doc, unlocked);
            e.editor.cancel();
            e.set_layer_selection(vec![1], Some(1));
            e.toggle_clipping_mask(cx);
            assert_eq!(e.editor.doc, unlocked, "bottom layer cannot clip");
            assert_eq!(e.editor.history.len(), 0);
        })
    });
}

#[gpui_kit::test]
fn photo_clipping_shortcut_and_context_menu_share_group_release(cx: &mut TestAppContext) {
    let original = stack();
    let (workspace, cx) = open(cx, original.clone());
    cx.simulate_resize(size(px(1200.), px(1200.)));
    let view = cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| {
        view.update(cx, |e, cx| {
            e.set_layer_selection(vec![3], Some(3));
            window.focus(&e.panel_focus, cx);
        })
    });
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-alt-g"
    } else {
        "ctrl-alt-g"
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert!(view.read(cx).editor.doc.node(3).unwrap().clip_to.is_none());
        assert!(view.read(cx).editor.doc.node(4).unwrap().clip_to.is_none());
        view.update(cx, |e, cx| e.undo(cx));
    });
    cx.run_until_parked();
    let row = cx.update(|window, _| window.find(("row", 3u64)).bounds().center());
    cx.simulate_mouse_down(row, MouseButton::Right, Default::default());
    cx.run_until_parked();
    // Stable existing Layer menu ordering: clipping follows Select Pixels.
    let release =
        cx.update(|window, _| window.within("popup-menu").find(24usize).bounds().center());
    cx.simulate_click(release, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert!(view.read(cx).editor.doc.node(3).unwrap().clip_to.is_none());
        assert!(view.read(cx).editor.doc.node(4).unwrap().clip_to.is_none());
        view.update(cx, |e, cx| e.undo(cx));
        assert_eq!(view.read(cx).editor.doc, original);
    });
}

#[gpui_kit::test]
fn photo_clip_release_handles_nested_groups_adjustments_and_preserves_paint(
    cx: &mut TestAppContext,
) {
    let mut original = stack();
    // Group children are not sibling members; releasing the group preserves them.
    let group_id = Command::Group {
        ids: vec![3, 4],
        name: "Nested".into(),
    }
    .apply(&mut original)
    .unwrap()
    .unwrap();
    original.node_mut(group_id).unwrap().clip_to = Some(2);
    original.node_mut(6).unwrap().kind = NodeKind::Adjust(emulsion_raster::Adjustment::Exposure {
        exposure: 1.,
        offset: 0.,
        gamma: 1.,
    });
    original.validate().unwrap();
    let (workspace, cx) = open(cx, original.clone());
    let view = cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.set_layer_selection(vec![2], Some(2));
            e.toggle_clipping_mask(cx);
            assert!(e.editor.doc.node(2).unwrap().clip_to.is_none());
            assert!(e.editor.doc.node(group_id).unwrap().clip_to.is_none());
            for id in [3, 4] {
                assert_eq!(e.editor.doc.node(id), original.node(id));
            }
            e.undo(cx);
            e.set_layer_selection(vec![6], Some(6));
            e.toggle_clipping_mask(cx);
            assert!(e.editor.doc.node(6).unwrap().clip_to.is_none());
            assert!(matches!(
                e.editor.doc.node(6).unwrap().kind,
                NodeKind::Adjust(_)
            ));
            e.undo(cx);
            e.draw_mode = true;
            e.set_layer_selection(vec![2], Some(2));
            e.toggle_clipping_mask(cx);
            assert!(e.editor.doc.node(2).unwrap().clip_to.is_none());
            assert_eq!(e.editor.doc.node(group_id), original.node(group_id));
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
        })
    });
}

#[gpui_kit::test]
fn photo_clip_release_repeated_toggle_and_design_scope(cx: &mut TestAppContext) {
    let original = stack();
    let (workspace, cx) = open(cx, original.clone());
    let view = cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.set_layer_selection(vec![3], Some(3));
            e.toggle_clipping_mask(cx);
            let released = e.editor.doc.clone();
            e.toggle_clipping_mask(cx);
            assert_eq!(e.editor.doc.node(3).unwrap().clip_to, Some(2));
            assert!(e.editor.doc.node(4).unwrap().clip_to.is_none());
            e.undo(cx);
            assert_eq!(e.editor.doc, released);
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
            e.editor = emulsion_core::project::ProjectEditor::new_project(
                emulsion_core::project::ProjectKind::Design,
                original.clone(),
            )
            .unwrap();
            assert!(!e.is_photo_workflow());
            e.toggle_clipping_mask(cx);
            assert!(e.editor.doc.node(3).unwrap().clip_to.is_none());
            assert_eq!(e.editor.doc.node(4), original.node(4));
        })
    });
}
