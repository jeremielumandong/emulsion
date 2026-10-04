use super::*;
use ::core::prelude::v1::test;
use emulsion_raster::{Placement, Raster};
use std::{cell::RefCell, rc::Rc};
fn task(
    ws: &Entity<Workspace>,
    origin: Entity<EditorView>,
    action: Action,
    cx: &mut VisualTestContext,
) -> Result<Value, String> {
    let result = Rc::new(RefCell::new(None));
    let out = result.clone();
    cx.update(|window, cx| {
        let job = ws.update(cx, |ws, cx| {
            ws.smart_source_task(origin, action, window, cx)
        });
        cx.spawn(async move |_| {
            *out.borrow_mut() = Some(job.await);
        })
        .detach();
    });
    cx.run_until_parked();
    let completed = result.borrow_mut().take();
    completed.expect("source task completed")
}
#[gpui_kit::test]
fn smart_source_editor_nested_apply_stale_parent_and_undo(cx: &mut TestAppContext) {
    let mut doc = Document::new(100, 80);
    let id = Command::AddNode {
        node: Box::new(Node::raster(
            0,
            "Smart",
            Arc::new(Raster::solid(20, 10, [1., 0., 0., 1.])),
            Placement::at(10., 20.),
        )),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .unwrap()
    .unwrap();
    Command::ConvertToSmart { id }.apply(&mut doc).unwrap();
    let initial = doc.clone();
    let (ws, cx) = crate::tests::open(cx, doc);
    let parent = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    task(&ws, parent.clone(), Action::Open { node: id }, cx).unwrap();
    let child = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        assert_ne!(child, parent);
        child.update(cx, |e, cx| {
            e.execute(
                Command::Rename {
                    id: 1,
                    name: "Source edited".into(),
                },
                cx,
            );
            e.execute(Command::ConvertToSmart { id: 1 }, cx);
        });
    });
    task(&ws, child.clone(), Action::Open { node: 1 }, cx).unwrap();
    let grandchild = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        grandchild.update(cx, |e, cx| {
            e.execute(
                Command::Rename {
                    id: 1,
                    name: "Nested edit".into(),
                },
                cx,
            );
        })
    });
    task(&ws, grandchild, Action::Apply, cx).unwrap();
    task(&ws, child.clone(), Action::Apply, cx).unwrap();
    cx.update(|_, cx| {
        let p = parent.read(cx);
        let source = emulsion_io::smart_source::open(&p.editor.doc, id).unwrap();
        let nested = emulsion_io::smart_source::open(&source, 1).unwrap();
        assert_eq!(nested.nodes[0].name, "Nested edit");
        parent.update(cx, |e, cx| {
            e.undo(cx);
            assert_eq!(e.editor.doc, initial);
        });
    });
    assert!(
        task(&ws, child.clone(), Action::Apply, cx)
            .unwrap_err()
            .contains("changed")
    );
    cx.update(|_, cx| {
        assert_eq!(child.read(cx).editor.doc.nodes[0].name, "Source edited");
    });
}

#[gpui_kit::test]
fn smart_source_auto_refresh_recovers_after_unlock_and_respects_undo(cx: &mut TestAppContext) {
    let path = std::env::temp_dir().join(format!(
        "emulsion-ui-smart-watch-{}.ora",
        std::process::id()
    ));
    let mut source = Document::new(20, 10);
    Command::AddNode {
        node: Box::new(Node::raster(
            0,
            "Original",
            Arc::new(Raster::solid(20, 10, [1., 0., 0., 1.])),
            Placement::default(),
        )),
        slot: Slot::TOP,
    }
    .apply(&mut source)
    .unwrap();
    emulsion_io::ora::write(&source, &path).unwrap();
    let mut doc = source.clone();
    Command::ConvertToSmart { id: 1 }.apply(&mut doc).unwrap();
    let mut editor = emulsion_core::Editor::new(doc, None);
    emulsion_io::smart_source::relink(&mut editor, 1, &path, true).unwrap();
    let (ws, cx) = crate::tests::open(cx, editor.doc);
    let view = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| view.update(cx, |e, cx| e.start_smart_source_watch(cx)));
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(3));
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.execute(
                Command::SetLocked {
                    id: 1,
                    locked: true,
                },
                cx,
            );
        })
    });
    source.nodes[0].name = "External edited source".into();
    emulsion_io::ora::write(&source, &path).unwrap();
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(3));
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            assert!(e.smart.source_watch_error.is_some());
            assert_eq!(
                emulsion_io::smart_source::open(&e.editor.doc, 1)
                    .unwrap()
                    .nodes[0]
                    .name,
                "Original"
            );
            e.execute(
                Command::SetLocked {
                    id: 1,
                    locked: false,
                },
                cx,
            );
        })
    });
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(3));
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            assert!(e.smart.source_watch_error.is_none());
            assert_eq!(
                emulsion_io::smart_source::open(&e.editor.doc, 1)
                    .unwrap()
                    .nodes[0]
                    .name,
                "External edited source"
            );
            e.undo(cx);
            assert_eq!(
                emulsion_io::smart_source::open(&e.editor.doc, 1)
                    .unwrap()
                    .nodes[0]
                    .name,
                "Original"
            );
        })
    });
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(3));
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(
            emulsion_io::smart_source::open(&view.read(cx).editor.doc, 1)
                .unwrap()
                .nodes[0]
                .name,
            "Original"
        )
    });
    std::fs::remove_file(path).unwrap();
}

fn modal_fixture(name: &str) -> Document {
    let mut doc = Document::new(100, 80);
    Command::AddNode {
        node: Box::new(Node::raster(
            0,
            name,
            Arc::new(Raster::solid(20, 10, [1., 0., 0., 1.])),
            Placement::at(10., 20.),
        )),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .unwrap();
    doc
}

#[gpui_kit::test]
fn install_refusal_returns_false_and_preserves_modal_document_identity(cx: &mut TestAppContext) {
    let (ws, cx) = crate::tests::open(cx, modal_fixture("Active"));
    let active = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| {
        active.update(cx, |e, cx| {
            e.begin_photo_transform(true, cx);
            assert!(
                e.photo_transform_delta(glam::DAffine2::from_translation(glam::dvec2(4., 3.)), cx)
            );
        });
        let (preview, revision, saved) = {
            let e = active.read(cx);
            (
                e.editor.doc.clone(),
                e.editor.revision,
                e.editor.saved_revision(),
            )
        };
        ws.update(cx, |ws, cx| {
            assert!(!ws.install(
                modal_fixture("New document"),
                None,
                None,
                None,
                "New".into(),
                window,
                cx
            ));
            let project = emulsion_core::project::ProjectEditor::new_project(
                emulsion_core::project::ProjectKind::Design,
                modal_fixture("New project"),
            )
            .unwrap();
            assert!(!ws.install_project(project, "New project".into(), window, cx));
            assert_eq!(ws.editor.as_ref(), Some(&active));
            assert_eq!(ws.tabs.len(), 1);
        });
        active.update(cx, |e, cx| {
            assert_eq!(e.editor.doc, preview);
            assert_eq!(e.editor.doc.next_id, preview.next_id);
            assert_eq!(e.editor.revision, revision);
            assert_eq!(e.editor.saved_revision(), saved);
            assert!(e.smart.source_session.is_none());
            assert!(e.history.recovery.is_none());
            assert!(e.commit_photo_transform(cx));
            assert!(!e.photo_transform_active());
            assert_eq!(e.editor.history.len(), 1);
        });
    });
}

#[gpui_kit::test]
fn late_smart_source_completion_cannot_repurpose_another_tabs_modal_preview(
    cx: &mut TestAppContext,
) {
    let mut parent_doc = modal_fixture("Original Smart");
    let node = parent_doc.nodes[0].id;
    Command::ConvertToSmart { id: node }
        .apply(&mut parent_doc)
        .unwrap();
    // These values are exactly what the asynchronous source loader captures.
    let expected = parent_doc.node(node).unwrap().kind.clone();
    let loaded = emulsion_io::smart_source::open(&parent_doc, node).unwrap();
    let (ws, cx) = crate::tests::open(cx, parent_doc.clone());
    let origin = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    let page = cx.update(|_, cx| origin.read(cx).editor.active_page());
    let active = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            assert!(ws.install(
                modal_fixture("Different tab"),
                None,
                None,
                None,
                "Different tab".into(),
                window,
                cx
            ));
            ws.editor.clone().unwrap()
        })
    });
    cx.update(|window, cx| {
        active.update(cx, |e, cx| {
            e.begin_photo_transform(true, cx);
            assert!(
                e.photo_transform_delta(glam::DAffine2::from_translation(glam::dvec2(6., 2.)), cx)
            );
        });
        let (preview, revision, saved) = {
            let e = active.read(cx);
            (
                e.editor.doc.clone(),
                e.editor.revision,
                e.editor.saved_revision(),
            )
        };
        let error = ws
            .update(cx, |ws, cx| {
                ws.finish_open_smart_source(
                    &origin,
                    node,
                    expected.clone(),
                    page,
                    1,
                    "Original Smart".into(),
                    loaded.clone(),
                    window,
                    cx,
                )
            })
            .unwrap_err();
        assert!(error.contains("transform"));
        assert_eq!(ws.read(cx).editor.as_ref(), Some(&active));
        assert_eq!(ws.read(cx).tabs.len(), 2);
        assert_eq!(origin.read(cx).editor.doc, parent_doc);
        active.update(cx, |e, cx| {
            assert_eq!(e.editor.doc, preview);
            assert_eq!(e.editor.doc.next_id, preview.next_id);
            assert_eq!(e.editor.revision, revision);
            assert_eq!(
                e.editor.saved_revision(),
                saved,
                "refused completion cannot mark a provisional document saved"
            );
            assert!(e.smart.source_session.is_none());
            assert!(e.commit_photo_transform(cx));
            assert!(!e.photo_transform_active());
        });
        let opened = ws
            .update(cx, |ws, cx| {
                ws.finish_open_smart_source(
                    &origin,
                    node,
                    expected.clone(),
                    page,
                    1,
                    "Original Smart".into(),
                    loaded.clone(),
                    window,
                    cx,
                )
            })
            .unwrap();
        let child = ws.read(cx).editor.clone().unwrap();
        assert_ne!(child, active);
        assert_ne!(child, origin);
        assert_eq!(opened["source_tab_id"], child.entity_id().as_u64());
        let session = child.read(cx).smart.source_session.as_ref().unwrap();
        assert_eq!(session.parent.entity_id(), origin.entity_id());
        assert_eq!(session.node, node);
        assert_eq!(
            child.read(cx).editor.saved_revision(),
            child.read(cx).editor.revision
        );
    });
}

#[gpui_kit::test]
fn refused_recovery_keeps_ora_and_project_files_and_metadata(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let recovered_doc = modal_fixture("Recovered");
    let ora = directory.path().join("recover.ora");
    let emu = directory.path().join("recover.emu");
    emulsion_io::ora::write(&recovered_doc, &ora).unwrap();
    let project = emulsion_core::project::ProjectEditor::new_project(
        emulsion_core::project::ProjectKind::Design,
        recovered_doc,
    )
    .unwrap()
    .snapshot()
    .unwrap();
    emulsion_io::project::write(&project, &emu).unwrap();
    let (ws, cx) = crate::tests::open(cx, modal_fixture("Active"));
    let active = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        active.update(cx, |e, cx| {
            e.begin_photo_transform(true, cx);
            assert!(
                e.photo_transform_delta(glam::DAffine2::from_translation(glam::dvec2(3., 4.)), cx)
            );
        })
    });
    let preview = cx.update(|_, cx| active.read(cx).editor.doc.clone());
    for path in [ora, emu] {
        let bytes = std::fs::read(&path).unwrap();
        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.recovered.push((path.clone(), 123));
                ws.open_recovered(path.clone(), window, cx);
            })
        });
        cx.run_until_parked();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            bytes,
            "refused install must retain the durable recovery copy"
        );
        cx.update(|_, cx| {
            let workspace = ws.read(cx);
            assert_eq!(workspace.editor.as_ref(), Some(&active));
            assert_eq!(workspace.tabs.len(), 1);
            assert!(workspace.recovered.iter().any(|(p, _)| p == &path));
            assert!(workspace.busy.is_none());
            let e = active.read(cx);
            assert_eq!(e.editor.doc, preview);
            assert_eq!(e.editor.doc.next_id, preview.next_id);
            assert!(
                e.history.recovery.is_none(),
                "recovery metadata must not attach to the wrong tab"
            );
            assert!(e.photo_transform_active());
        });
    }
    cx.update(|_, cx| {
        active.update(cx, |e, cx| {
            assert!(e.commit_photo_transform(cx));
            assert!(!e.photo_transform_active());
            assert_eq!(e.editor.history.len(), 1);
        })
    });
}
