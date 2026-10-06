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

#[gpui_kit::test]
fn unchanged_source_apply_preserves_original_after_parent_rename_and_font_edits(
    cx: &mut TestAppContext,
) {
    let mut doc = modal_fixture("Opened source name");
    let id = doc.nodes[0].id;
    Command::ConvertToSmart { id }.apply(&mut doc).unwrap();
    let original = Arc::new(emulsion_core::node::OriginalImage::new(
        Arc::new(vec![42; 128]),
        [1; 32],
        [2; 32],
    ));
    let emulsion_core::NodeKind::Smart { original_image, .. } = &mut doc.nodes[0].kind else {
        unreachable!()
    };
    *original_image = Some(original.clone());
    let (ws, cx) = crate::tests::open(cx, doc);
    let parent = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    task(&ws, parent.clone(), Action::Open { node: id }, cx).unwrap();
    let child = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    let (before, revision, history, ticket) = cx.update(|_, cx| {
        parent.update(cx, |e, cx| {
            e.execute(
                Command::Rename {
                    id,
                    name: "Renamed parent".into(),
                },
                cx,
            );
            let font = emulsion_core::design_fonts::EmbeddedFont::from_bytes(
                include_bytes!("../../../../assets/fonts/Geist.ttf").to_vec(),
            )
            .unwrap();
            let mut design = e.editor.doc.design.clone();
            design.fonts.insert(font.alias().into(), font);
            e.execute(
                Command::SetDesign {
                    design: Box::new(design),
                },
                cx,
            );
            (
                e.editor.doc.clone(),
                e.editor.revision,
                e.editor.history.len(),
                e.edit_ticket(),
            )
        })
    });
    cx.update(|_, cx| {
        child.update(cx, |e, _| {
            assert_eq!(
                e.smart.source_session.as_ref().unwrap().baseline.nodes[0].name,
                "Opened source name"
            );
            assert!(
                e.smart
                    .source_session
                    .as_ref()
                    .unwrap()
                    .baseline
                    .design
                    .fonts
                    .is_empty()
            );
            e.editor.doc.selection = Some(Arc::new(emulsion_raster::Mask::empty(20, 10, 127)));
        });
    });
    task(&ws, child.clone(), Action::Apply, cx).unwrap();
    cx.update(|_, cx| {
        let p = parent.read(cx);
        assert!(emulsion_io::smart_source::same_document_contents(
            &p.editor.doc,
            &before
        ));
        assert_eq!(p.editor.revision, revision);
        assert_eq!(p.editor.history.len(), history);
        assert_eq!(
            p.edit_ticket(),
            ticket,
            "a no-op must not invalidate other parent jobs"
        );
        let emulsion_core::NodeKind::Smart {
            original_image: Some(retained),
            editable: None,
            ..
        } = &p.editor.doc.node(id).unwrap().kind
        else {
            panic!("unchanged child lost the original PNG")
        };
        assert!(Arc::ptr_eq(retained, &original));
    });

    // A real child edit still counts when its new name happens to match the parent.
    cx.update(|_, cx| {
        let fonts = parent.read(cx).editor.doc.design.fonts.clone();
        child.update(cx, |e, cx| {
            let mut design = e.editor.doc.design.clone();
            design.fonts = fonts;
            e.execute(
                Command::SetDesign {
                    design: Box::new(design),
                },
                cx,
            );
            e.execute(
                Command::Rename {
                    id: 1,
                    name: "Renamed parent".into(),
                },
                cx,
            );
        });
        let regenerated = emulsion_io::smart_source::open(&parent.read(cx).editor.doc, id).unwrap();
        assert!(emulsion_io::smart_source::same_document_contents(
            &child.read(cx).editor.doc,
            &regenerated
        ));
    });
    task(&ws, child.clone(), Action::Apply, cx).unwrap();
    let applied_revision = cx.update(|_, cx| {
        let p = parent.read(cx);
        assert_eq!(p.editor.history.len(), history + 1);
        assert!(matches!(
            &p.editor.doc.node(id).unwrap().kind,
            emulsion_core::NodeKind::Smart {
                original_image: None,
                editable: Some(emulsion_core::node::SmartEditable::Document { .. }),
                ..
            }
        ));
        let source = emulsion_io::smart_source::open(&p.editor.doc, id).unwrap();
        assert_eq!(source.nodes[0].name, "Renamed parent");
        let c = child.read(cx);
        assert!(emulsion_io::smart_source::same_document_contents(
            &c.editor.doc,
            &c.smart.source_session.as_ref().unwrap().baseline,
        ));
        assert_eq!(c.editor.saved_revision(), c.editor.revision);
        p.editor.revision
    });
    task(&ws, child, Action::Apply, cx).unwrap();
    cx.update(|_, cx| {
        let p = parent.read(cx);
        assert_eq!(p.editor.revision, applied_revision);
        assert_eq!(p.editor.history.len(), history + 1);
        parent.update(cx, |e, cx| {
            e.undo(cx);
            assert!(emulsion_io::smart_source::same_document_contents(
                &e.editor.doc,
                &before
            ));
        });
    });
}

#[test]
fn source_session_content_comparison_ignores_only_selection() {
    let baseline = modal_fixture("Original");
    let mut selection = baseline.clone();
    selection.selection = Some(Arc::new(emulsion_raster::Mask::empty(100, 80, 127)));
    assert!(emulsion_io::smart_source::same_document_contents(
        &baseline, &selection
    ));
    for field in [
        "source_depth",
        "next_id",
        "info",
        "colors",
        "drawing_guides",
    ] {
        let mut candidate = baseline.clone();
        match field {
            "source_depth" => candidate.source_depth = 16,
            "next_id" => candidate.next_id += 1,
            "info" => candidate.info = Some(emulsion_core::document::ImageInfo::default()),
            "colors" => candidate.colors.push([1, 2, 3]),
            "drawing_guides" => candidate
                .drawing_guides
                .guides
                .push(emulsion_core::drawing_guides::GuideKind::Grid { size: 12. }),
            _ => unreachable!(),
        }
        assert_eq!(baseline, candidate, "history equality excludes {field}");
        assert!(
            !emulsion_io::smart_source::same_document_contents(&baseline, &candidate),
            "source content must retain {field}"
        );
    }
    let mut fonts = baseline.clone();
    let font = emulsion_core::design_fonts::EmbeddedFont::from_bytes(
        include_bytes!("../../../../assets/fonts/Geist.ttf").to_vec(),
    )
    .unwrap();
    fonts.design.fonts.insert(font.alias().into(), font);
    assert!(!emulsion_io::smart_source::same_document_contents(
        &baseline, &fonts
    ));
}

#[gpui_kit::test]
fn captured_smart_source_apply_is_rejected_after_later_filter_toggle(cx: &mut TestAppContext) {
    let mut doc = Document::new(40, 30);
    let pixels = Arc::new(Raster::solid(12, 8, [0.2, 0.3, 0.4, 1.]));
    doc.nodes.push(Node::smart(
        1,
        "Smart",
        pixels.clone(),
        vec![],
        Placement::default(),
    ));
    doc.next_id = 2;
    let (ws, cx) = crate::tests::open(cx, doc);
    let parent = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    task(&ws, parent.clone(), Action::Open { node: 1 }, cx).unwrap();
    let child = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        child.update(cx, |e, cx| {
            e.execute(
                Command::Rename {
                    id: 1,
                    name: "Edited source".into(),
                },
                cx,
            );
        })
    });
    let (captured, captured_rx) = async_channel::bounded(1);
    let (release, release_rx) = async_channel::bounded(1);
    cx.update(|_, cx| {
        child.update(cx, |e, _| {
            e.smart.apply_capture_barrier = Some((captured, release_rx));
        });
    });
    let result = Rc::new(RefCell::new(None));
    let out = result.clone();
    cx.update(|window, cx| {
        let job = ws.update(cx, |ws, cx| {
            ws.smart_source_task(child.clone(), Action::Apply, window, cx)
        });
        cx.spawn(async move |_| {
            *out.borrow_mut() = Some(job.await);
        })
        .detach();
    });
    cx.run_until_parked();
    captured_rx
        .try_recv()
        .expect("Apply captured its original source and parent tickets");
    assert!(result.borrow().is_none());
    cx.update(|_, cx| parent.update(cx, |e, cx| e.set_filters_enabled(1, false, cx)));
    cx.run_until_parked();
    assert!(
        result.borrow().is_none(),
        "Apply must remain paused while the toggle publishes"
    );
    let settled = cx.update(|_, cx| parent.read(cx).editor.doc.clone());
    assert!(matches!(
        settled.node(1).unwrap().kind,
        NodeKind::Smart {
            filters_enabled: false,
            ..
        }
    ));
    release.try_send(()).unwrap();
    cx.run_until_parked();
    let error = result
        .borrow_mut()
        .take()
        .expect("Apply completes")
        .expect_err("captured old parent ticket is stale");
    assert_eq!(
        error,
        "The parent changed during Apply. Retry against its current state."
    );
    cx.update(|_, cx| {
        let p = parent.read(cx);
        assert_eq!(p.editor.doc, settled);
        assert!(matches!(&p.editor.doc.node(1).unwrap().kind,
            NodeKind::Smart { filters_enabled:false, editable:None, source, cache, offset, .. }
            if Arc::ptr_eq(source,&pixels) && Arc::ptr_eq(cache,&pixels) && *offset==(0,0)));
    });
}

fn source_toggle_document() -> Document {
    use emulsion_core::{EmptyVectorCoverage, SmartFilterMask, VectorMask};
    use emulsion_filters::{Filter, FilterStyle};
    use emulsion_raster::Mask;
    let mut doc = Document::new(40, 30);
    let mut node = Node::smart(
        1,
        "Original source",
        Arc::new(Raster::solid(12, 8, [0.2, 0.3, 0.4, 1.])),
        vec![
            Filter::GaussianBlur { radius: 2. },
            Filter::BoxBlur { radius: 3. },
        ],
        Placement::at(4., 5.),
    );
    node.mask = Some(Arc::new(Mask::empty(12, 8, 191)));
    node.vector_mask = Some(VectorMask::empty(EmptyVectorCoverage::RevealAll));
    let NodeKind::Smart {
        original_image,
        filter_mask,
        ..
    } = &mut node.kind
    else {
        unreachable!()
    };
    *original_image = Some(Arc::new(emulsion_core::node::OriginalImage::new(
        Arc::new(vec![42; 128]),
        [1; 32],
        [2; 32],
    )));
    let mut mask = SmartFilterMask::new(Arc::new(Mask::empty(16, 12, 127)));
    mask.linked = false;
    mask.transform =
        emulsion_core::Mapping2::from_affine_columns([1., 0., 0., 1., -2., -2.]).unwrap();
    mask.properties.density = 0.7;
    *filter_mask = Some(mask);
    doc.nodes.push(node);
    doc.next_id = 2;
    Command::SetFilterStyles {
        id: 1,
        styles: vec![
            FilterStyle {
                opacity: 0.6,
                ..Default::default()
            },
            FilterStyle {
                enabled: false,
                ..Default::default()
            },
        ],
    }
    .apply(&mut doc)
    .unwrap();
    doc
}

fn exact_source_parent(actual: &Document, expected: &Document) {
    // Source, OriginalImage, all masks, authored filter state and parameters are
    // identity-sensitive document contents; cache/offset are intentionally not.
    assert!(emulsion_io::smart_source::same_document_contents(
        actual, expected
    ));
    assert_eq!(
        actual.selection.as_ref().map(Arc::as_ptr),
        expected.selection.as_ref().map(Arc::as_ptr)
    );
    let (
        NodeKind::Smart {
            cache: a,
            offset: oa,
            ..
        },
        NodeKind::Smart {
            cache: b,
            offset: ob,
            ..
        },
    ) = (
        &actual.node(1).unwrap().kind,
        &expected.node(1).unwrap().kind,
    )
    else {
        panic!("Smart parent retained")
    };
    assert!(Arc::ptr_eq(a, b));
    assert_eq!(oa, ob);
}

type HeldApplyResult = Rc<RefCell<Option<Result<Value, String>>>>;

fn captured_apply(
    ws: &Entity<Workspace>,
    child: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> (HeldApplyResult, async_channel::Sender<()>) {
    let (captured, captured_rx) = async_channel::bounded(1);
    let (release, release_rx) = async_channel::bounded(1);
    let result = Rc::new(RefCell::new(None));
    let out = result.clone();
    cx.update(|window, cx| {
        child.update(cx, |e, _| {
            assert!(e.smart.apply_capture_barrier.is_none());
            e.smart.apply_capture_barrier = Some((captured, release_rx));
        });
        let job = ws.update(cx, |ws, cx| {
            ws.smart_source_task(child.clone(), Action::Apply, window, cx)
        });
        cx.spawn(async move |_| {
            *out.borrow_mut() = Some(job.await);
        })
        .detach();
    });
    cx.run_until_parked();
    captured_rx
        .try_recv()
        .expect("Apply captured both documents and tickets");
    assert!(result.borrow().is_none());
    (result, release)
}

#[gpui_kit::test]
fn captured_source_apply_rejects_pending_and_cancelled_unpublished_filter_toggles(
    cx: &mut TestAppContext,
) {
    for item in [false, true] {
        for cancel_toggle in [false, true] {
            let original = source_toggle_document();
            let (ws, cx) = crate::tests::open(cx, original.clone());
            let parent = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
            task(&ws, parent.clone(), Action::Open { node: 1 }, cx).unwrap();
            let child = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
            let (
                parent_ticket,
                saved,
                next_revision,
                child_before,
                child_ticket,
                child_saved,
                session,
            ) = cx.update(|_, cx| {
                parent.update(cx, |e, cx| {
                    e.execute(
                        Command::Rename {
                            id: 1,
                            name: "Redo sentinel".into(),
                        },
                        cx,
                    );
                });
                let next_revision = parent.read(cx).editor.revision + 1;
                parent.update(cx, |e, cx| e.undo(cx));
                child.update(cx, |e, cx| {
                    e.execute(
                        Command::Rename {
                            id: 1,
                            name: "Edited source".into(),
                        },
                        cx,
                    );
                });
                let p = parent.read(cx);
                let c = child.read(cx);
                assert!(p.editor.history.is_empty() && p.editor.history.can_redo());
                exact_source_parent(&p.editor.doc, &original);
                (
                    p.edit_ticket(),
                    p.editor.saved_revision(),
                    next_revision,
                    c.editor.doc.clone(),
                    c.edit_ticket(),
                    c.editor.saved_revision(),
                    c.smart.source_session.clone().unwrap(),
                )
            });
            let (result, release_apply) = captured_apply(&ws, &child, cx);
            let (filter_ready, release_filter) = cx.update(|_, cx| {
                parent.update(cx, |e, cx| {
                    let barrier = e.smart.pause_next_render();
                    if item {
                        e.set_filter_enabled(1, 0, false, cx);
                    } else {
                        e.set_filters_enabled(1, false, cx);
                    }
                    assert_ne!(e.edit_ticket().0, parent_ticket.0);
                    assert_eq!(e.editor.revision, parent_ticket.1);
                    assert!(e.smart.has_pending());
                    exact_source_parent(&e.editor.doc, &original);
                    barrier
                })
            });
            cx.run_until_parked();
            filter_ready
                .try_recv()
                .expect("toggle render held before publication");
            let retired_ticket = cx.update(|_, cx| {
                parent.update(cx, |e, cx| {
                    if cancel_toggle {
                        assert!(e.tool_cancel(cx));
                        assert!(!e.smart.has_pending());
                        e.smart_source_ready()
                            .expect("readiness no longer rejects the captured Apply");
                    }
                    // With unchanged page, full document and revision, only the
                    // retired operation epoch can reject the canceled case.
                    assert_eq!(e.editor.active_page(), session.page);
                    assert_eq!(e.editor.revision, parent_ticket.1);
                    assert_ne!(e.edit_ticket().0, parent_ticket.0);
                    exact_source_parent(&e.editor.doc, &original);
                    e.edit_ticket()
                })
            });
            release_apply.try_send(()).unwrap();
            cx.run_until_parked();
            let error = result
                .borrow_mut()
                .take()
                .expect("Apply completed")
                .unwrap_err();
            assert_eq!(
                error,
                if cancel_toggle {
                    "The parent changed during Apply. Retry against its current state."
                } else {
                    "Finish active edits and exit preview before changing Smart sources."
                }
            );
            cx.update(|_, cx| {
                let p = parent.read(cx);
                exact_source_parent(&p.editor.doc, &original);
                assert_eq!(p.edit_ticket(), retired_ticket);
                assert_eq!(p.editor.saved_revision(), saved);
                assert!(p.editor.history.is_empty() && p.editor.history.can_redo());
                assert_eq!(p.smart.has_pending(), !cancel_toggle);
                let c = child.read(cx);
                assert!(emulsion_io::smart_source::same_document_contents(
                    &c.editor.doc,
                    &child_before
                ));
                assert_eq!(c.edit_ticket(), child_ticket);
                assert_eq!(c.editor.saved_revision(), child_saved);
                assert_eq!(c.editor.history.len(), 1);
                let retained = c.smart.source_session.as_ref().unwrap();
                assert_eq!(retained.expected, session.expected);
                assert!(emulsion_io::smart_source::same_document_contents(
                    &retained.baseline,
                    &session.baseline
                ));
            });
            release_filter.try_send(()).unwrap();
            cx.run_until_parked();
            cx.update(|_, cx| {
                parent.update(cx, |e, cx| {
                    assert!(!e.smart.has_pending());
                    if cancel_toggle {
                        exact_source_parent(&e.editor.doc, &original);
                        assert_eq!(e.edit_ticket(), retired_ticket);
                        assert!(e.editor.history.is_empty() && e.editor.history.can_redo());
                    } else {
                        let mut expected = original.clone();
                        let NodeKind::Smart {
                            source,
                            cache,
                            offset,
                            filters_enabled,
                            filter_styles,
                            ..
                        } = &mut expected.node_mut(1).unwrap().kind
                        else {
                            unreachable!()
                        };
                        if item {
                            filter_styles[0].enabled = false;
                        } else {
                            *filters_enabled = false;
                        }
                        *cache = source.clone();
                        *offset = (0, 0);
                        exact_source_parent(&e.editor.doc, &expected);
                        assert_eq!(e.editor.revision, next_revision);
                        assert_eq!(e.editor.history.len(), 1);
                        assert!(!e.editor.history.can_redo());
                        let step = e.editor.history.steps().next().unwrap();
                        assert_eq!(step.revision_before, parent_ticket.1);
                        assert_eq!(
                            step.name,
                            if item {
                                "Disable filter"
                            } else {
                                "Disable Smart Filters"
                            }
                        );
                        exact_source_parent(&step.before, &original);
                        e.undo(cx);
                        exact_source_parent(&e.editor.doc, &original);
                        assert_eq!(e.editor.revision, parent_ticket.1);
                        e.redo(cx);
                        exact_source_parent(&e.editor.doc, &expected);
                    }
                })
            });
        }
    }
}

#[gpui_kit::test]
fn same_state_filter_toggles_preserve_captured_source_apply_and_no_op_history(
    cx: &mut TestAppContext,
) {
    for changed_child in [false, true] {
        let original = source_toggle_document();
        let (ws, cx) = crate::tests::open(cx, original.clone());
        let parent = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
        task(&ws, parent.clone(), Action::Open { node: 1 }, cx).unwrap();
        let child = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
        let (ticket, saved, next_revision) = cx.update(|_, cx| {
            parent.update(cx, |e, cx| {
                e.execute(
                    Command::Rename {
                        id: 1,
                        name: "Redo sentinel".into(),
                    },
                    cx,
                );
                let next_revision = e.editor.revision + 1;
                e.undo(cx);
                (e.edit_ticket(), e.editor.saved_revision(), next_revision)
            })
        });
        if changed_child {
            cx.update(|_, cx| {
                child.update(cx, |e, cx| {
                    e.execute(
                        Command::Rename {
                            id: 1,
                            name: "Edited source".into(),
                        },
                        cx,
                    );
                })
            });
        }
        let (result, release) = captured_apply(&ws, &child, cx);
        cx.update(|_, cx| {
            parent.update(cx, |e, cx| {
                e.set_filters_enabled(1, true, cx);
                e.set_filter_enabled(1, 0, true, cx);
                e.set_filter_enabled(1, 1, false, cx);
                assert!(!e.smart.has_pending());
                assert_eq!(e.edit_ticket(), ticket);
                assert!(e.editor.history.is_empty() && e.editor.history.can_redo());
                exact_source_parent(&e.editor.doc, &original);
            })
        });
        // The per-editor barrier also releases when its test handle is dropped.
        drop(release);
        cx.run_until_parked();
        result
            .borrow_mut()
            .take()
            .expect("Apply completed")
            .unwrap();
        let settled = cx.update(|_, cx| {
            let p = parent.read(cx);
            assert_eq!(p.editor.saved_revision(), saved);
            if changed_child {
                assert_eq!(p.editor.revision, next_revision);
                assert_eq!(p.editor.history.len(), 1);
                assert!(!p.editor.history.can_redo());
                exact_source_parent(&p.editor.history.steps().next().unwrap().before, &original);
                let NodeKind::Smart {
                    source,
                    original_image,
                    editable,
                    filters,
                    filter_styles,
                    filters_enabled,
                    filter_mask,
                    cache,
                    offset,
                    ..
                } = &p.editor.doc.node(1).unwrap().kind
                else {
                    unreachable!()
                };
                let NodeKind::Smart {
                    source: old_source,
                    filters: old_filters,
                    filter_styles: old_styles,
                    filter_mask: old_mask,
                    ..
                } = &original.node(1).unwrap().kind
                else {
                    unreachable!()
                };
                assert!(!Arc::ptr_eq(source, old_source));
                assert!(original_image.is_none());
                assert!(matches!(
                    editable,
                    Some(emulsion_core::node::SmartEditable::Document { .. })
                ));
                assert_eq!(filters, old_filters);
                assert_eq!(filter_styles, old_styles);
                assert!(*filters_enabled);
                assert_eq!(filter_mask, old_mask);
                assert_eq!(
                    p.editor.doc.node(1).unwrap().mask.as_ref().map(Arc::as_ptr),
                    original.node(1).unwrap().mask.as_ref().map(Arc::as_ptr)
                );
                assert_eq!(
                    p.editor.doc.node(1).unwrap().vector_mask,
                    original.node(1).unwrap().vector_mask
                );
                let (expected_cache, expected_offset) = emulsion_core::smart::render_stack(
                    source,
                    filters,
                    filter_styles,
                    *filters_enabled,
                );
                assert_eq!(cache.to_pixels(), expected_cache.to_pixels());
                assert_eq!(*offset, expected_offset);
                assert_eq!(
                    emulsion_io::smart_source::open(&p.editor.doc, 1)
                        .unwrap()
                        .nodes[0]
                        .name,
                    "Edited source"
                );
            } else {
                exact_source_parent(&p.editor.doc, &original);
                assert_eq!(p.edit_ticket(), ticket);
                assert!(p.editor.history.is_empty() && p.editor.history.can_redo());
            }
            let c = child.read(cx);
            assert_eq!(c.editor.saved_revision(), c.editor.revision);
            assert!(emulsion_io::smart_source::same_document_contents(
                &c.editor.doc,
                &c.smart.source_session.as_ref().unwrap().baseline
            ));
            (
                p.editor.doc.clone(),
                p.edit_ticket(),
                p.editor.history.len(),
            )
        });
        task(&ws, child, Action::Apply, cx).unwrap();
        cx.update(|_, cx| {
            parent.update(cx, |e, cx| {
                exact_source_parent(&e.editor.doc, &settled.0);
                assert_eq!(e.edit_ticket(), settled.1);
                assert_eq!(e.editor.history.len(), settled.2);
                if changed_child {
                    e.undo(cx);
                    exact_source_parent(&e.editor.doc, &original);
                    assert_eq!(e.editor.revision, ticket.1);
                    e.redo(cx);
                    exact_source_parent(&e.editor.doc, &settled.0);
                } else {
                    assert!(e.editor.history.can_redo());
                }
            })
        });
    }
}
