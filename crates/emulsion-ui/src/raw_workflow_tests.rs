//! RAW recipes live in the document; pending development never overrides undo.
use super::*;
use emulsion_core::raw::{DevelopParams, RawDocument, RawMetadata};

use crate::raw_test_fixture as raw_fixture;

#[gpui_kit::test]
fn raw_curve_graph_drags_points_and_keeps_monotonic_limits(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt;
    let mut before = raw_document();
    before.raw.as_mut().unwrap().params.smooth_curve = false;
    let (ws, cx) = open(cx, before.clone());
    let ed = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1200.), gpui_kit::px(1600.)));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("raw-curve", cx));
    cx.run_until_parked();
    let bounds = cx.update(|_, cx| {
        ed.read(cx)
            .raw_curve_bounds()
            .expect("RAW curve graph laid out")
    });
    let at = |x: f32, y: f32| {
        bounds.origin + gpui_kit::point(bounds.size.width * x, bounds.size.height * (1. - y))
    };
    cx.update(|window, cx| window.drag(at(0.5, 0.5), at(0.5, 0.65), cx));
    cx.update(|_, cx| {
        let params = ed.read(cx).raw_params().unwrap();
        assert!(
            params.smooth_curve,
            "dragging upgrades a legacy curve to smooth interpolation"
        );
        let curve = params.tone_curve;
        assert!((curve[2] - 0.65).abs() < 0.02, "{curve:?}");
        assert_eq!(curve[1], 0.25);
        assert_eq!(curve[3], 0.75);
    });
    cx.update(|window, cx| window.drag(at(0.5, 0.65), at(0.5, 0.95), cx));
    cx.update(|_, cx| {
        ed.update(cx, |e, cx| {
            assert_eq!(e.raw_params().unwrap().tone_curve[2], 0.75);
            e.undo(cx);
            assert!(!e.raw.is_pending());
            assert_eq!(e.editor.doc, before);
        })
    });
}

#[gpui_kit::test]
fn raw_image_rotation_turns_canvas_and_preserves_recipe_through_undo(cx: &mut TestAppContext) {
    let fixture = SidecarFixture::new();
    let original = emulsion_io::open(&fixture.0).unwrap();
    let (width, height) = (original.width, original.height);
    let (ws, cx) = open(cx, original.clone());
    let ed = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        ed.update(cx, |e, _| {
            e.draw_mode = false;
            let id = e.editor.doc.raw.as_ref().unwrap().node_id;
            e.set_layer_selection(vec![id], Some(id));
        })
    });
    cx.dispatch_action(crate::actions::RotateLayer90Cw);
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = ed.read(cx);
        assert_eq!((e.editor.doc.width, e.editor.doc.height), (height, width));
        assert_eq!(e.editor.doc.raw, original.raw);
        assert_eq!(e.editor.history.len(), 1);
        let id = original.raw.as_ref().unwrap().node_id;
        assert_eq!(
            emulsion_core::geometry::node_bounds(&e.editor.doc, id),
            Some(emulsion_raster::IRect::new(
                0,
                0,
                height as i32,
                width as i32
            ))
        );
    });
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(ed.read(cx).editor.doc, original));
}

struct SidecarFixture(std::path::PathBuf);

impl SidecarFixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "emulsion-ui-sidecar-{}-{}.dng",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        raw_fixture::write_dng(&path);
        Self(path)
    }

    fn sidecar(&self) -> std::path::PathBuf {
        emulsion_io::raw_settings::sidecar_path(&self.0).unwrap()
    }
}

impl Drop for SidecarFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.sidecar());
        let _ = std::fs::remove_file(&self.0);
    }
}

#[gpui_kit::test]
fn opening_raw_shows_histogram_alongside_develop_controls(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt;

    let fixture = SidecarFixture::new();
    let document = emulsion_io::open(&fixture.0).unwrap();
    let before = document.clone();
    let (ws, cx) = open(cx, document);
    let ed = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("raw-histogram").visible());
        assert!(window.find("raw-adjust").visible());
        window.click("raw-curve", cx);
    });
    cx.run_until_parked();
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1200.), gpui_kit::px(1600.)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("raw-histogram").visible());
        assert!(window.find("raw-curve-linear").visible());
        ed.update(cx, |e, cx| {
            assert!(
                e.histogram(cx).is_some(),
                "histogram finishes in the background"
            );
            assert_eq!(e.editor.doc, before);
            assert!(e.editor.history.is_empty());
        });
    });
}

#[gpui_kit::test]
fn raw_ctrl_s_saves_sidecar_and_reopen_restores_edits(cx: &mut TestAppContext) {
    let fixture = SidecarFixture::new();
    let original = std::fs::read(&fixture.0).unwrap();
    let (ws, cx) = open(cx, emulsion_io::open(&fixture.0).unwrap());
    let ed = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        ed.update(cx, |e, cx| {
            e.source = Some(fixture.0.clone());
            e.raw_slider("exposure", 125.0, cx);
            e.raw_slider("temperature", 20.0, cx);
        })
    });
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(250));
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-s");
    cx.run_until_parked();
    assert!(!cx.did_prompt_for_new_path());
    assert!(fixture.sidecar().exists());
    let restored = emulsion_io::open(&fixture.0).unwrap();
    cx.update(|_, cx| {
        let e = ed.read(cx);
        assert!(!e.editor.is_modified());
        assert!(e.editor.path.is_none());
        assert_eq!(e.source.as_ref(), Some(&fixture.0));
        assert_eq!(
            restored.raw.as_ref().unwrap().params,
            e.editor.doc.raw.as_ref().unwrap().params
        );
        assert_ne!(
            restored.raw.as_ref().unwrap().params,
            DevelopParams::default()
        );
    });
    assert_eq!(std::fs::read(&fixture.0).unwrap(), original);
    // Save as remains a project operation, even for pure RAW development.
    cx.simulate_keystrokes("ctrl-shift-s");
    assert!(cx.did_prompt_for_new_path());
    cx.simulate_new_path_selection(|_| None);
    cx.run_until_parked();
}

#[gpui_kit::test]
fn raw_sidecar_save_does_not_mark_concurrent_project_edit_saved(cx: &mut TestAppContext) {
    let fixture = SidecarFixture::new();
    let (ws, cx) = open(cx, emulsion_io::open(&fixture.0).unwrap());
    let ed = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.simulate_keystrokes("ctrl-s");
    cx.update(|_, cx| {
        ed.update(cx, |e, cx| {
            let id = e.editor.doc.raw.as_ref().unwrap().node_id;
            e.execute(Command::SetOpacity { id, opacity: 0.5 }, cx);
        })
    });
    cx.run_until_parked();
    assert!(fixture.sidecar().exists());
    cx.update(|_, cx| assert!(ed.read(cx).editor.is_modified()));
    cx.simulate_keystrokes("ctrl-s");
    assert!(
        cx.did_prompt_for_new_path(),
        "project edits cannot be saved as just a RAW recipe"
    );
    cx.simulate_new_path_selection(|_| None);
    cx.run_until_parked();
}

#[gpui_kit::test]
fn raw_sidecar_save_failure_keeps_edits_dirty(cx: &mut TestAppContext) {
    let fixture = SidecarFixture::new();
    let (ws, cx) = open(cx, emulsion_io::open(&fixture.0).unwrap());
    let ed = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| ed.update(cx, |e, cx| e.raw_slider("exposure", 100.0, cx)));
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(250));
    cx.run_until_parked();
    std::fs::write(fixture.sidecar(), b"unrelated file").unwrap();
    cx.simulate_keystrokes("ctrl-s");
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = ed.read(cx);
        assert!(e.editor.is_modified());
        assert!(e.status.as_ref().unwrap().0.contains("Save failed"));
    });
    assert_eq!(std::fs::read(fixture.sidecar()).unwrap(), b"unrelated file");
}

#[gpui_kit::test]
fn raw_synchronization_applies_only_selected_group_and_is_undoable(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt;
    let path =
        std::env::temp_dir().join(format!("emulsion-raw-ui-sync-{}.dng", std::process::id()));
    raw_fixture::write_dng(&path);
    let target_doc = emulsion_io::open(&path).unwrap();
    let before_target = target_doc.clone();
    let mut source_doc = raw_document();
    source_doc.raw.as_mut().unwrap().params.brightness = 0.25;
    source_doc.raw.as_mut().unwrap().params.temperature = 0.3;
    let (ws, cx) = open(cx, source_doc.clone());
    let source = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    let target = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install(
                target_doc,
                None,
                None,
                Some(path.clone()),
                "Destination".into(),
                window,
                cx,
            );
            let target = ws.editor.clone().unwrap();
            ws.editor = Some(source.clone());
            ws.synchronize_raw(window, cx);
            target
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.click("raw-sync-tone", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.click("raw-sync-select-all", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.click("raw-sync-apply", cx);
    });
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(250));
    cx.run_until_parked();
    cx.update(|_, cx| {
        let params = target.read(cx).editor.doc.raw.as_ref().unwrap().params;
        assert_eq!(params.brightness, 0.25);
        assert_eq!(params.exposure, 0.5);
        assert_eq!(
            params.temperature,
            before_target.raw.as_ref().unwrap().params.temperature
        );
        assert_eq!(source.read(cx).editor.doc, source_doc);
        target.update(cx, |e, cx| e.undo(cx));
        assert_eq!(target.read(cx).editor.doc, before_target);
    });
    std::fs::remove_file(path).unwrap();
}

fn raw_document() -> Document {
    let mut d = doc(&["Camera photo"], None);
    d.source_depth = 16;
    d.raw = Some(RawDocument {
        schema_version: 1,
        node_id: d.nodes[0].id,
        source: std::env::temp_dir().join("emulsion-missing-raw-workflow-fixture.dng"),
        source_sha256: "0".repeat(64),
        params: DevelopParams {
            exposure: 0.5,
            ..Default::default()
        },
        metadata: RawMetadata {
            make: "Test".into(),
            model: "Bayer".into(),
            ..Default::default()
        },
    });
    d
}

#[gpui_kit::test]
fn raw_before_after_handle_drags_without_edits_and_escape_restores_view(cx: &mut TestAppContext) {
    use emulsion_raster::composite::flatten;
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{MouseButton, point, px};

    let path = std::env::temp_dir().join(format!(
        "emulsion-ui-raw-split-{}-{}.dng",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    raw_fixture::write_dng(&path);
    let mut document = emulsion_io::open(&path).unwrap();
    let baseline = flatten(&document.composite_tree(), 0).to_srgba8();
    let raw = document.raw.as_ref().unwrap().clone();
    let params = DevelopParams {
        exposure: 1.5,
        brightness: 0.15,
        ..Default::default()
    };
    let pixels = emulsion_io::raw::RawSource::load_verified(&raw.source, &raw.source_sha256)
        .unwrap()
        .develop_with(&params)
        .unwrap();
    Command::DevelopRaw {
        id: raw.node_id,
        raster: Arc::new(pixels),
        params,
    }
    .apply(&mut document)
    .unwrap();
    let edited = flatten(&document.composite_tree(), 0).to_srgba8();
    assert_ne!(baseline, edited);
    let before = document.clone();
    // Opening an already edited project makes its saved history baseline equal to
    // the current photo. RAW comparison must still use the original RAW recipe.
    let (ws, cx) = open(cx, document);
    let ed = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.simulate_resize(gpui_kit::size(px(1400.), px(1600.)));
    cx.update(|_, cx| {
        ed.update(cx, |e, cx| {
            e.selected = Some(raw.node_id);
            e.select_sidebar(crate::editor::SidebarTab::Properties, cx);
        })
    });
    cx.run_until_parked();
    let revision = cx.update(|_, cx| ed.read(cx).editor.revision);
    cx.update(|window, cx| {
        assert!(window.find("raw-before-after").visible());
        window.click("raw-before-after", cx);
    });
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(250));
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = ed.read(cx);
        assert!(e.raw_split_active());
        assert_eq!(
            flatten(&e.raw_split_tree().unwrap(), 0).to_srgba8(),
            baseline
        );
        assert_eq!(flatten(&e.tree, 0).to_srgba8(), edited);
        assert_eq!(e.compare, 0.5);
    });

    let bounds = cx.update(|_, cx| ed.read(cx).canvas_bounds.get().unwrap());
    let anchor = cx.update(|_, cx| ed.read(cx).doc_to_window((10., 10.)).unwrap());
    for (fraction, expected) in [(0.75, 0.75), (-0.1, 0.0), (1.1, 1.0), (0.4, 0.4)] {
        let start = cx.update(|window, _| {
            assert!(window.find("raw-compare-handle").visible());
            window.find("raw-compare-handle").bounds().center()
        });
        let end = point(bounds.origin.x + bounds.size.width * fraction, start.y);
        cx.simulate_mouse_down(start, MouseButton::Left, Default::default());
        cx.simulate_mouse_move(end, Some(MouseButton::Left), Default::default());
        cx.simulate_mouse_up(end, MouseButton::Left, Default::default());
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = ed.read(cx);
            assert!(
                (e.compare - expected).abs() < 0.001,
                "drag to {fraction}: expected {expected}, got {} (start {start:?}, end {end:?})",
                e.compare
            );
            assert!(
                e.raw_split_active(),
                "edge positions must keep the handle available"
            );
            assert_eq!(e.editor.doc, before);
            assert_eq!(e.editor.revision, revision);
            assert!(e.editor.history.is_empty());
            assert!(!e.editor.is_modified());
            assert!(!e.editor.in_transaction());
            assert_eq!(
                e.doc_to_window((10., 10.)).unwrap(),
                anchor,
                "drag must not pan"
            );
        });
    }
    for (key, expected) in [("home", 0.), ("right", 0.02), ("end", 1.), ("left", 0.98)] {
        cx.simulate_keystrokes(key);
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert!(
                (ed.read(cx).compare - expected).abs() < 0.001,
                "key {key}: expected {expected}, got {}",
                ed.read(cx).compare
            )
        });
    }
    // Escape also ends an in-progress divider drag.
    let start = cx.update(|window, _| window.find("raw-compare-handle").bounds().center());
    cx.simulate_mouse_down(start, MouseButton::Left, Default::default());
    cx.simulate_keystrokes("escape");
    cx.simulate_mouse_move(start, Some(MouseButton::Left), Default::default());
    cx.simulate_mouse_up(start, MouseButton::Left, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = ed.read(cx);
        assert!(!e.raw_split_requested());
        assert!(e.raw_split_tree().is_none());
        assert_eq!(e.compare, 0.);
        assert_eq!(flatten(&e.tree, 0).to_srgba8(), edited);
        assert_eq!(e.editor.doc, before);
        assert_eq!(e.editor.revision, revision);
        assert!(e.editor.history.is_empty());
    });
    std::fs::remove_file(path).unwrap();
}

#[gpui_kit::test]
fn raw_pending_edits_are_cancelled_by_undo_without_changing_recipe(cx: &mut TestAppContext) {
    let d = raw_document();
    let before = d.clone();
    let (ws, cx) = open(cx, d);
    let ed = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        ed.update(cx, |e, cx| {
            e.raw_slider("exposure", 150.0, cx);
            assert!(e.raw.is_pending());
            assert_eq!(e.editor.doc.raw.as_ref().unwrap().params.exposure, 0.5);
            e.undo(cx);
            assert!(!e.raw.is_pending());
        })
    });
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(500));
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(ed.read(cx).editor.doc, before));
}

#[gpui_kit::test]
fn raw_source_layer_identity_survives_reopen_and_smart_filters(cx: &mut TestAppContext) {
    let mut d = raw_document();
    let raw_id = d.raw.as_ref().unwrap().node_id;
    Command::ConvertToSmart { id: raw_id }
        .apply(&mut d)
        .unwrap();
    let (ws, cx) = open(cx, d);
    let ed = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        ed.update(cx, |e, cx| {
            // Native reopen has no source filename hint, but the RAW panel persists.
            assert!(e.source.is_none());
            let p = theme::palette(cx);
            assert!(e.raw_panel(raw_id, &p, cx).is_some());
            e.raw_slider("temperature", 30.0, cx);
            assert!(e.raw.is_pending());
            e.execute(Command::RemoveNode { id: raw_id }, cx);
            assert!(!e.raw.is_pending());
            assert!(e.editor.doc.raw.is_none());
            e.undo(cx);
            assert!(e.editor.doc.raw.is_some());
            assert!(e.raw_panel(raw_id, &p, cx).is_some());
        })
    });
}

#[gpui_kit::test]
fn save_during_raw_development_does_not_silently_save_old_settings(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, raw_document());
    let ed = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        ed.update(cx, |e, cx| e.raw_slider("exposure", 100.0, cx));
        ws.update(cx, |ws, cx| {
            ws.write(
                ed.clone(),
                std::env::temp_dir().join("raw-pending-must-not-save.ora"),
                cx,
            )
        });
        let e = ed.read(cx);
        assert!(e.raw.is_pending());
        assert!(!e.history.save_busy);
        assert!(
            e.status
                .as_ref()
                .unwrap()
                .0
                .contains("RAW development is still running")
        );
    });
    cx.update(|_, cx| ed.update(cx, |e, _| e.cancel_raw_develop()));
}

#[gpui_kit::test]
fn raw_extended_controls_are_drafts_and_undo_cancels_them(cx: &mut TestAppContext) {
    let before = raw_document();
    let (ws, cx) = open(cx, before.clone());
    let ed = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        ed.update(cx, |e, cx| {
            for (name, value) in [
                ("black_point", 10.0),
                ("brightness", 25.0),
                ("contrast", 30.0),
                ("saturation", -20.0),
                ("curve1", 20.0),
            ] {
                e.raw_slider(name, value, cx);
            }
            let params = e.raw_params().unwrap();
            assert_eq!(params.black_point, 0.1);
            assert_eq!(params.brightness, 0.25);
            assert_eq!(params.contrast, 0.3);
            assert_eq!(params.saturation, -0.2);
            assert_eq!(params.tone_curve[1], 0.2);
            assert_eq!(e.editor.doc, before);
            e.raw_slider("curve1", 99.0, cx);
            assert_eq!(
                e.raw_params().unwrap().tone_curve[1],
                0.5,
                "curve stays monotonic"
            );
            e.undo(cx);
            assert!(!e.raw.is_pending());
            assert_eq!(e.editor.doc, before);
        })
    });
}

#[gpui_kit::test]
fn raw_settings_picker_cancel_clears_busy_without_changing_document(cx: &mut TestAppContext) {
    let before = raw_document();
    let (ws, cx) = open(cx, before.clone());
    let ed = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| ed.update(cx, |e, cx| e.raw_settings_file(true, true, cx)));
    assert!(cx.did_prompt_for_new_path());
    cx.update(|_, cx| assert!(ed.read(cx).raw_settings_is_busy()));
    cx.simulate_new_path_selection(|_| None);
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert!(!ed.read(cx).raw_settings_is_busy());
        assert_eq!(ed.read(cx).editor.doc, before);
    });
    cx.update(|_, cx| ed.update(cx, |e, cx| e.raw_settings_file(false, false, cx)));
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|_| None);
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert!(!ed.read(cx).raw_settings_is_busy());
        assert_eq!(ed.read(cx).editor.doc, before);
    });
}

#[gpui_kit::test]
fn raw_settings_picker_rejects_stale_document_before_writing(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, raw_document());
    let ed = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| {
        ed.update(cx, |e, cx| {
            e.raw_settings_file(true, true, cx);
            e.undo(cx);
        })
    });
    cx.simulate_new_path_selection(|_| {
        Some(std::env::temp_dir().join("stale-raw-settings-must-not-be-written.json"))
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = ed.read(cx);
        assert!(!e.raw_settings_is_busy());
        assert!(e.status.as_ref().unwrap().0.contains("document changed"));
    });
}

#[gpui_kit::test]
fn raw_real_dng_preview_and_clipping_buttons_leave_history_unchanged_and_escape_restores_view(
    cx: &mut TestAppContext,
) {
    use emulsion_raster::composite::flatten;
    use gpui_kit::test::TestWindowExt;

    let path = std::env::temp_dir().join(format!(
        "emulsion-ui-raw-preview-{}-{}.dng",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    raw_fixture::write_dng(&path);
    let mut document = emulsion_io::open(&path).unwrap();
    let raw = document.raw.as_ref().unwrap().clone();
    let params = DevelopParams {
        exposure: 1.5,
        brightness: 0.15,
        contrast: 0.2,
        ..Default::default()
    };
    let pixels = emulsion_io::raw::RawSource::load_verified(&raw.source, &raw.source_sha256)
        .unwrap()
        .develop_with(&params)
        .unwrap();
    Command::DevelopRaw {
        id: raw.node_id,
        raster: Arc::new(pixels),
        params,
    }
    .apply(&mut document)
    .unwrap();
    let before = document.clone();
    let edited = flatten(&document.composite_tree(), 0).to_srgba8();
    let (ws, cx) = open(cx, document);
    let ed = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1400.), gpui_kit::px(1600.)));
    cx.update(|_, cx| {
        ed.update(cx, |e, cx| {
            e.selected = Some(raw.node_id);
            e.select_sidebar(crate::editor::SidebarTab::Properties, cx);
        })
    });
    cx.run_until_parked();
    let revision = cx.update(|_, cx| ed.read(cx).editor.revision);

    for control in ["raw-tone-preview", "raw-clipping"] {
        let prior_generation = cx.update(|_, cx| ed.read(cx).render_gen);
        cx.update(|window, cx| {
            if !window.find(control).visible() {
                window.scroll(
                    ("sidebar-content", ed.read(cx).sidebar_tab as usize),
                    gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
                        gpui_kit::px(0.),
                        gpui_kit::px(-160.),
                    )),
                    cx,
                );
            }
            assert!(
                window.find(control).visible(),
                "{control} must be discoverable in RAW properties"
            );
            window.click(control, cx);
        });
        cx.run_until_parked();
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(250));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = ed.read(cx);
            assert!(
                !e.raw.is_pending(),
                "preview should finish for generated 36×24 DNG"
            );
            assert_eq!(e.editor.doc, before);
            assert_eq!(e.editor.revision, revision);
            assert!(e.editor.history.is_empty());
            assert!(!e.editor.is_modified());
            assert_ne!(
                flatten(&e.tree, 0).to_srgba8(),
                edited,
                "{control} must actually change the displayed photo"
            );
            assert_ne!(
                e.render_gen, prior_generation,
                "preview must invalidate viewport cache"
            );
        });
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = ed.read(cx);
            assert_eq!(
                flatten(&e.tree, 0).to_srgba8(),
                edited,
                "Escape must restore the edited viewport"
            );
            assert_eq!(e.editor.doc, before);
            assert_eq!(e.editor.revision, revision);
            assert!(e.editor.history.is_empty());
            assert!(!e.raw.is_pending());
        });
    }
    std::fs::remove_file(&path).unwrap();
}
