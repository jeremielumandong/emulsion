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
