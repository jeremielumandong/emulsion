//! Recovery saves the working document; only explicit versions grow the graph.
use super::*;
use gpui_kit::test::TestWindowExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

struct RecoveryFolder(PathBuf);
impl RecoveryFolder {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "emulsion-versioning-test-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn recovery(&self) -> PathBuf {
        self.0.join("working.ora")
    }
}
impl Drop for RecoveryFolder {
    fn drop(&mut self) {
        // Only test-created files inside this unique directory are removed.
        let _ = std::fs::remove_file(self.recovery());
        let _ = std::fs::remove_file(self.0.join("blocked"));
        let _ = std::fs::remove_dir(&self.0);
    }
}

#[gpui_kit::test]
fn recovery_updates_latest_document_without_creating_versions_or_losing_undo(
    cx: &mut TestAppContext,
) {
    let folder = RecoveryFolder::new();
    let original = doc(&["Photo"], None);
    let id = original.nodes[0].id;
    let (ws, cx) = open(cx, original.clone());
    let view = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    let versions = cx.update(|_, cx| view.read(cx).editor.graph.commits().count());
    for (index, opacity) in [0.8, 0.6, 0.4].into_iter().enumerate() {
        cx.update(|_, cx| {
            view.update(cx, |e, cx| {
                e.history.recovery = Some(folder.recovery());
                e.history.last_recovery = None;
                e.execute(Command::SetOpacity { id, opacity }, cx);
                e.autosave(cx);
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = view.read(cx);
            assert!(!e.history.recovery_busy);
            assert!(e.history.last_autosave.is_some());
            assert_eq!(e.history.recovery_rev, e.editor.revision);
            assert_eq!(e.editor.graph.commits().count(), versions);
            assert_eq!(e.editor.history.len(), index + 1);
            let recovered = emulsion_io::open_full(&folder.recovery()).unwrap();
            assert_eq!(
                (recovered.doc.width, recovered.doc.height),
                (e.editor.doc.width, e.editor.doc.height)
            );
            assert_eq!(recovered.doc.nodes.len(), e.editor.doc.nodes.len());
            assert_eq!(recovered.doc.node(id).unwrap().opacity, opacity);
            // Plane equality intentionally uses shared-buffer identity. Files
            // create fresh buffers, so verify exact pixel values instead.
            let emulsion_core::NodeKind::Raster { raster: actual, .. } =
                &recovered.doc.node(id).unwrap().kind
            else {
                panic!()
            };
            let emulsion_core::NodeKind::Raster {
                raster: expected, ..
            } = &e.editor.doc.node(id).unwrap().kind
            else {
                panic!()
            };
            for y in 0..e.editor.doc.height {
                for x in 0..e.editor.doc.width {
                    assert_eq!(
                        actual.get(x, y),
                        expected.get(x, y),
                        "recovery pixel {x},{y}"
                    );
                }
            }
            assert_eq!(recovered.graph.unwrap().commits().count(), versions);
        });
    }
    for opacity in [0.6, 0.8, 1.0] {
        cx.update(|_, cx| view.update(cx, |e, cx| e.undo(cx)));
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc.node(id).unwrap().opacity, opacity));
    }
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, original));
}

#[gpui_kit::test]
fn failed_recovery_does_not_claim_autosave_success_or_add_versions(cx: &mut TestAppContext) {
    let folder = RecoveryFolder::new();
    let blocked = folder.0.join("blocked");
    std::fs::write(&blocked, b"a file cannot be a recovery directory").unwrap();
    let original = doc(&["Photo"], None);
    let id = original.nodes[0].id;
    let (ws, cx) = open(cx, original);
    let view = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.history.recovery = Some(blocked.join("working.ora"));
            e.execute(Command::SetOpacity { id, opacity: 0.4 }, cx);
            e.autosave(cx);
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = view.read(cx);
        assert!(!e.history.recovery_busy);
        assert!(e.history.last_autosave.is_none());
        assert!(e.autosave_note().is_none());
        assert_eq!(e.history.recovery_rev, 0);
        assert_eq!(e.editor.graph.commits().count(), 1);
        assert_eq!(e.editor.history.len(), 1);
    });
}

#[gpui_kit::test]
fn create_named_version_from_history_keeps_edits_undoable(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let id = original.nodes[0].id;
    let (ws, cx) = open(cx, original.clone());
    let view = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            e.execute(Command::SetOpacity { id, opacity: 0.5 }, cx);
            e.open_history(cx);
        })
    });
    cx.run_until_parked();
    let button = cx.update(|window, _| window.find("create-version").bounds().center());
    cx.simulate_click(button, Default::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        let input = view
            .read(cx)
            .history
            .new_version
            .as_ref()
            .unwrap()
            .0
            .clone();
        input.update(cx, |input, cx| input.set_value("Warm grade", window, cx));
    });
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = view.read(cx);
        assert!(e.history.new_version.is_none());
        assert_eq!(e.editor.graph.commits().count(), 2);
        let version = e.editor.graph.commits().last().unwrap();
        assert_eq!(version.name, "Warm grade");
        assert!(!version.auto);
        assert_eq!(version.doc, e.editor.doc);
        assert_eq!(e.editor.history.len(), 1);
    });
    cx.update(|_, cx| view.update(cx, |e, cx| e.undo(cx)));
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = view.read(cx);
        assert_eq!(e.editor.doc, original);
        assert_eq!(
            e.editor.graph.commits().count(),
            2,
            "undoing an edit retains its named version"
        );
    });
}
