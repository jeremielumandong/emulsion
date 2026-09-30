use super::{doc, open};
use crate::workspace::Screen;
use gpui_kit::{TestAppContext, test::TestWindowExt};

struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "emulsion-library-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
    fn pngs(&self) -> Vec<std::path::PathBuf> {
        (0..4)
            .map(|i| {
                let p = self.0.join(format!("photo-{i}.png"));
                image::RgbaImage::from_pixel(4, 4, image::Rgba([30, 60, 90, 255]))
                    .save(&p)
                    .unwrap();
                p
            })
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[gpui_kit::test]
fn library_grid_loupe_filmstrip_preserve_selection_and_output(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let paths = fixture.pngs();
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1400.), gpui_kit::px(1000.)));
    cx.update(|_, cx| {
        ws.update(cx, |ws, cx| {
            ws.load_batch(fixture.0.clone(), paths.clone(), cx);
            ws.screen = Screen::Batch;
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("batch-item", 1usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let batch = &ws.read(cx).batch;
        assert_eq!(batch.current, Some(1));
        assert_eq!(batch.items.iter().filter(|i| i.selected).count(), 1);
        assert!(window.find("library-filmstrip").visible());
        assert!(window.find("library-selection-actions").visible());
        window.click("library-loupe-view", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("library-preview").visible());
        window.click("library-next", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(ws.read(cx).batch.current, Some(2));
        window.click("library-grid-view", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find(("batch-item", 2usize)).visible());
        window.click("library-sort", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let batch = &ws.read(cx).batch;
        assert_eq!(batch.items[0].path, paths[3]);
        assert_eq!(batch.current, Some(1));
        assert!(batch.items[1].selected);
        assert_eq!(batch.out_dir, Some(fixture.0.join("emulsion-export")));
    });
}

#[gpui_kit::test]
fn library_shift_and_control_selection_and_list_keep_active_photo(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let paths = fixture.pngs();
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1500.), gpui_kit::px(1000.)));
    cx.update(|_, cx| {
        ws.update(cx, |ws, cx| {
            ws.load_batch(fixture.0.clone(), paths, cx);
            ws.screen = Screen::Batch;
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("library-reject").is_none());
        window.click(("batch-item", 0usize), cx);
    });
    cx.run_until_parked();
    let position = cx.update(|window, _| window.find(("batch-item", 2usize)).bounds().center());
    cx.simulate_click(
        position,
        gpui_kit::Modifiers {
            shift: true,
            ..Default::default()
        },
    );
    cx.run_until_parked();
    cx.update(|window, cx| {
        let batch = &ws.read(cx).batch;
        assert_eq!(
            batch.items.iter().map(|i| i.selected).collect::<Vec<_>>(),
            vec![true, true, true, false]
        );
        assert_eq!(batch.current, Some(2));
        assert!(window.find("library-reject").visible());
    });
    let position = cx.update(|window, _| window.find(("batch-item", 1usize)).bounds().center());
    cx.simulate_click(
        position,
        gpui_kit::Modifiers {
            control: true,
            ..Default::default()
        },
    );
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(
            ws.read(cx)
                .batch
                .items
                .iter()
                .map(|i| i.selected)
                .collect::<Vec<_>>(),
            vec![true, false, true, false]
        );
        window.click("library-list-view", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find(("batch-item", 3usize)).visible());
        assert_eq!(ws.read(cx).batch.current, Some(1));
    });
}

#[gpui_kit::test]
fn library_raw_bw_save_and_export_use_same_settings(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let path = fixture.0.join("photo.dng");
    crate::raw_test_fixture::write_dng(&path);
    let second = fixture.0.join("second.dng");
    crate::raw_test_fixture::write_dng(&second);
    let original = std::fs::read(&path).unwrap();
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1500.), gpui_kit::px(1500.)));
    cx.update(|_, cx| {
        ws.update(cx, |ws, cx| {
            ws.load_batch(fixture.0.clone(), vec![path.clone(), second.clone()], cx);
            ws.screen = Screen::Batch;
            ws.batch.format = "png".into();
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("batch-item", 0usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("library-develop").visible());
        window.click("library-raw-bw", cx);
    });
    cx.update(|_, cx| ws.update(cx, |ws, cx| ws.run_batch(cx)));
    cx.update(|_, cx| {
        assert!(
            ws.read(cx).batch.running.is_none(),
            "unsaved edits cannot silently export older pixels"
        )
    });
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(500));
    cx.run_until_parked();
    let doc = emulsion_io::raw::open(&path).unwrap();
    assert_eq!(doc.raw.as_ref().unwrap().params.saturation, -1.);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    cx.update(|window, cx| window.click("library-compare-view", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("library-preview").visible());
        window.click("batch-all", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("library-footer-sync", cx));
    cx.run_until_parked();
    assert_eq!(
        emulsion_io::raw::open(&second)
            .unwrap()
            .raw
            .unwrap()
            .params
            .saturation,
        -1.
    );

    cx.update(|_, cx| ws.update(cx, |ws, cx| ws.run_batch(cx)));
    cx.run_until_parked();
    let exported = image::open(fixture.0.join("emulsion-export/photo.png"))
        .unwrap()
        .to_rgba8();
    let expected = emulsion_raster::composite::flatten(&doc.composite_tree(), 0).to_srgba8();
    assert_eq!(exported.as_raw(), &expected);
}

fn library_tool(
    ws: &gpui_kit::Entity<crate::workspace::Workspace>,
    cx: &mut gpui_kit::VisualTestContext,
    root: &std::path::Path,
    name: &str,
    args: serde_json::Value,
) -> emulsion_mcp::ToolResult {
    let output = std::rc::Rc::new(std::cell::RefCell::new(None));
    let result = output.clone();
    cx.update(|_, cx| {
        ws.update(cx, |ws, cx| {
            let task = ws.library_mcp_at(name, &args, root.to_path_buf(), cx);
            cx.spawn(async move |_, _| {
                *result.borrow_mut() = Some(task.await);
            })
            .detach();
        })
    });
    for _ in 0..2000 {
        cx.run_until_parked();
        if let Some(result) = output.borrow_mut().take() {
            return result;
        }
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
    }
    panic!("Library tool did not finish: {name}");
}
fn tool_json(result: emulsion_mcp::ToolResult) -> serde_json::Value {
    assert!(!result.is_error, "{:?}", result.content);
    serde_json::from_str(result.content[0]["text"].as_str().unwrap()).unwrap()
}

#[gpui_kit::test]
fn library_add_and_refresh_preserve_selection_and_saved_edits(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let paths = fixture.pngs();
    let root = fixture.0.join("catalog");
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    // Initialize the same isolated catalog used by every command below.
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "get_library",
        serde_json::json!({}),
    ));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1700.), gpui_kit::px(1100.)));
    cx.update(|_, cx| {
        ws.update(cx, |ws, cx| {
            ws.screen = Screen::Batch;
            ws.splash = false;
            cx.notify();
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("library-add-photos").visible());
        assert!(window.find("library-refresh").visible());
        ws.update(cx, |ws, cx| {
            ws.library_import_photos_from(root.clone(), paths[..2].to_vec(), None, cx)
        });
    });
    cx.run_until_parked();
    let imported = tool_json(library_tool(
        &ws,
        cx,
        &root,
        "get_library",
        serde_json::json!({}),
    ));
    assert_eq!(imported["total"], 2, "{imported}");
    cx.update(|window, cx| window.click(("batch-item", 0usize), cx));
    cx.run_until_parked();
    let active = paths[0].canonicalize().unwrap();
    let draft = emulsion_core::raw::DevelopParams {
        exposure: 0.75,
        ..Default::default()
    };
    let digest = emulsion_io::raw::source_digest(&active).unwrap();
    emulsion_io::raw_settings::save_photo_settings(&active, &digest, draft).unwrap();
    cx.update(|_, cx| {
        ws.update(cx, |ws, cx| {
            ws.library_import_photos_from(root.clone(), vec![paths[2].clone()], None, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let batch = &ws.read(cx).batch;
        assert_eq!(batch.items.len(), 3);
        assert_eq!(batch.items[batch.current.unwrap()].path, active);
        assert_eq!(batch.items.iter().filter(|i| i.selected).count(), 1);
    });
    // A catalog change made outside this workspace must appear on explicit refresh.
    emulsion_io::creative_library::update(&root, |catalog| {
        emulsion_io::photo_catalog::import(catalog, &paths[3..], false)
    })
    .unwrap();
    cx.update(|_, cx| ws.update(cx, |ws, cx| ws.library_refresh_from(root.clone(), cx)));
    cx.run_until_parked();
    cx.update(|_, cx| {
        let batch = &ws.read(cx).batch;
        assert_eq!(batch.items.len(), 4);
        assert_eq!(batch.items[batch.current.unwrap()].path, active);
        assert_eq!(batch.items.iter().filter(|i| i.selected).count(), 1);
        assert_eq!(
            emulsion_io::raw_settings::adjacent_settings(&active, &digest).unwrap(),
            draft
        );
    });
    assert_eq!(
        emulsion_io::creative_library::load(&root)
            .unwrap()
            .assets
            .len(),
        4
    );
}

#[gpui_kit::test]
fn library_remove_selected_photos_persists_without_deleting_files(cx: &mut TestAppContext) {
    use serde_json::json;
    let fixture = Fixture::new();
    let paths = fixture.pngs();
    let root = fixture.0.join("catalog");
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1700.), gpui_kit::px(1100.)));
    cx.update(|_, cx| ws.update(cx, |ws, _| ws.screen = Screen::Batch));
    cx.run_until_parked();
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "import_library",
        json!({"folder":fixture.0}),
    ));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "select_library_photos",
        json!({"paths":[paths[0],paths[1]],"active":paths[0]}),
    ));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("library-remove-photos").visible());
        // Exercise the button's command against this test's isolated catalog.
        ws.update(cx, |ws, cx| ws.library_remove_photos_from(root.clone(), cx));
    });
    cx.run_until_parked();
    let catalog = emulsion_io::creative_library::load(&root).unwrap();
    assert_eq!(catalog.assets.len(), 2);
    for path in &paths[..2] {
        assert!(
            !catalog
                .assets
                .iter()
                .any(|a| a.path == path.canonicalize().unwrap())
        );
    }
    cx.update(|_, cx| {
        let batch = &ws.read(cx).batch;
        assert_eq!(batch.items.len(), 2);
        assert!(batch.current.is_none());
    });
    for path in paths {
        assert_eq!(
            image::open(path).unwrap().to_rgba8().get_pixel(0, 0).0,
            [30, 60, 90, 255]
        );
    }
}

#[gpui_kit::test]
fn library_mcp_catalog_selection_metadata_filter_and_collection_roundtrip(cx: &mut TestAppContext) {
    use serde_json::json;
    let fixture = Fixture::new();
    let photos = fixture.pngs();
    let root = fixture.0.join("catalog");
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.run_until_parked();
    let imported = tool_json(library_tool(
        &ws,
        cx,
        &root,
        "import_library",
        json!({"folder":fixture.0}),
    ));
    assert_eq!(imported["total"], 4);
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "select_library_photos",
        json!({"paths":[photos[0],photos[2]],"active":photos[2]}),
    ));
    let selected = tool_json(library_tool(
        &ws,
        cx,
        &root,
        "get_library",
        json!({"limit":2}),
    ));
    assert_eq!(selected["active"], json!(photos[2]));
    assert_eq!(selected["next_offset"], 2);
    assert_eq!(selected["selected"].as_array().unwrap().len(), 2);
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "edit_library_metadata",
        json!({"paths":[photos[0],photos[2]],"rating":4,"flag":"rejected","color_label":3,"keywords":["Nature","Trip"]}),
    ));
    let catalog = emulsion_io::creative_library::load(&root).unwrap();
    let asset = catalog.assets.iter().find(|a| a.path == photos[2]).unwrap();
    assert_eq!(asset.rating, 4);
    assert!(asset.rejected && !asset.flagged);
    assert_eq!(asset.color_label, 3);
    assert_eq!(asset.tags, vec!["Nature", "Trip"]);
    let collection = tool_json(library_tool(
        &ws,
        cx,
        &root,
        "library_collection",
        json!({"action":"create","name":"Field work","paths":[photos[2]]}),
    ));
    let id = collection["collection_id"].as_u64().unwrap();
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "library_collection",
        json!({"action":"add","id":id,"paths":[photos[0]]}),
    ));
    let filtered = tool_json(library_tool(
        &ws,
        cx,
        &root,
        "set_library_view",
        json!({"collection":id,"minimum_rating":4,"flag":"rejected","query":"nature","sort":"capture_time","reverse":true,"mode":"list","inspector":"keywords"}),
    ));
    assert_eq!(filtered["total"], 2);
    assert_eq!(filtered["view"], "list");
    assert_eq!(filtered["inspector"], "keywords");
    let bad = library_tool(
        &ws,
        cx,
        &root,
        "select_library_photos",
        json!({"paths":[photos[1]]}),
    );
    assert!(bad.is_error);
    let unchanged = tool_json(library_tool(&ws, cx, &root, "get_library", json!({})));
    assert_eq!(unchanged["active"], json!(photos[2]));
}

#[gpui_kit::test]
fn library_mcp_raw_develop_preview_sync_presets_undo_and_export(cx: &mut TestAppContext) {
    use serde_json::json;
    let fixture = Fixture::new();
    let first = fixture.0.join("a.dng");
    let second = fixture.0.join("b.dng");
    crate::raw_test_fixture::write_dng(&first);
    crate::raw_test_fixture::write_dng(&second);
    let original = std::fs::read(&first).unwrap();
    let root = fixture.0.join("catalog");
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.run_until_parked();
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "import_library",
        json!({"folder":fixture.0}),
    ));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "select_library_photos",
        json!({"paths":[first,second],"active":first}),
    ));
    let developed = tool_json(library_tool(
        &ws,
        cx,
        &root,
        "develop_library",
        json!({"action":"adjust","settings":{"exposure":1.1,"whites":0.1,"vibrance":0.2,"texture":0.15,"clarity":0.1,"dehaze":0.1,"vignette":-0.1,"sharpening":0.2,"noise_reduction":0.15}}),
    ));
    assert_eq!(developed["develop"]["dirty"], false);
    let saved = emulsion_io::raw::open(&first).unwrap().raw.unwrap().params;
    assert_eq!(saved.exposure, 1.1);
    assert_eq!(saved.noise_reduction, 0.15);
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "develop_library",
        json!({"action":"sync","group":"tone"}),
    ));
    assert_eq!(
        emulsion_io::raw::open(&second)
            .unwrap()
            .raw
            .unwrap()
            .params
            .exposure,
        1.1
    );
    let preset = fixture.0.join("saved.emulsion-preset.json");
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "develop_library",
        json!({"action":"save_preset","path":preset}),
    ));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "develop_library",
        json!({"action":"reset"}),
    ));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "develop_library",
        json!({"action":"undo"}),
    ));
    assert_eq!(
        emulsion_io::raw::open(&first).unwrap().raw.unwrap().params,
        saved
    );
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "develop_library",
        json!({"action":"preset","preset":"black_and_white"}),
    ));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "develop_library",
        json!({"action":"load_preset","path":preset}),
    ));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "set_library_view",
        json!({"mode":"compare"}),
    ));
    let preview = library_tool(&ws, cx, &root, "get_library_preview", json!({}));
    assert!(!preview.is_error, "{:?}", preview.content);
    assert_eq!(
        preview
            .content
            .iter()
            .filter(|v| v["type"] == "image")
            .count(),
        2
    );
    let out = fixture.0.join("exports");
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "export_library",
        json!({"out_dir":out,"format":"png"}),
    ));
    assert!(out.join("a.png").is_file());
    assert!(out.join("b.png").is_file());
    let doc = emulsion_io::raw::open(&first).unwrap();
    // Compare the native 16-bit export paths; an 8-bit preview conversion rounds
    // at a different boundary and may differ by one channel level.
    let photo_export = fixture.0.join("photo-reference.png");
    emulsion_io::export::export(
        &doc,
        &photo_export,
        emulsion_io::export::ExportOptions::for_doc(&doc),
    )
    .unwrap();
    assert_eq!(
        image::open(out.join("a.png")).unwrap().to_rgba16(),
        image::open(photo_export).unwrap().to_rgba16(),
    );
    assert_eq!(std::fs::read(&first).unwrap(), original);
    // External sidecar replacement must be reported, never silently overwritten.
    let source = emulsion_io::raw::RawSource::load(&first).unwrap();
    emulsion_io::raw_settings::save_source_settings(&source, Default::default()).unwrap();
    let conflict = library_tool(
        &ws,
        cx,
        &root,
        "develop_library",
        json!({"action":"adjust","settings":{"exposure":0.6}}),
    );
    assert!(conflict.is_error);
    let dirty = tool_json(library_tool(&ws, cx, &root, "get_library", json!({})));
    assert_eq!(dirty["develop"]["dirty"], true);
    assert!(
        library_tool(
            &ws,
            cx,
            &root,
            "export_library",
            json!({"out_dir":out,"format":"png"})
        )
        .is_error
    );
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "develop_library",
        json!({"action":"reload"}),
    ));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "develop_library",
        json!({"action":"auto"}),
    ));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "develop_library",
        json!({"action":"as_shot"}),
    ));
    let opened = tool_json(library_tool(
        &ws,
        cx,
        &root,
        "open_library_photo",
        json!({}),
    ));
    assert_eq!(opened["opened"], true);
    assert!(opened["document_id"].is_u64());
}

#[gpui_kit::test]
fn library_raw_thumbnails_load_without_selecting_a_photo(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let path = fixture.0.join("sensor.dng");
    crate::raw_test_fixture::write_dng(&path);
    // This fixture has no embedded JPEG: the Library must develop a thumbnail.
    assert!(emulsion_io::thumb::batch_thumbnail(&path, 300).is_err());
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1500.), gpui_kit::px(1000.)));
    cx.update(|_, cx| {
        ws.update(cx, |ws, cx| {
            ws.load_batch(fixture.0.clone(), vec![path], cx);
            ws.screen = Screen::Batch;
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find(("batch-item", 0usize)).visible());
        let b = &ws.read(cx).batch;
        assert!(b.items[0].thumb.is_some(), "RAW thumbnail never loaded");
        let bounds = window.find(("batch-thumbnail", 0usize)).bounds();
        assert!(
            bounds.size.width > gpui_kit::px(100.) && bounds.size.height > gpui_kit::px(80.),
            "RAW thumbnail has no display area: {bounds:?}"
        );
        assert!(b.current.is_none(), "browsing must not select a photo");
    });
}

#[gpui_kit::test]
fn library_checkbox_keeps_multiselection_and_supports_keyboard(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1500.), gpui_kit::px(1000.)));
    cx.update(|_, cx| {
        ws.update(cx, |ws, cx| {
            ws.load_batch(fixture.0.clone(), fixture.pngs(), cx);
            ws.screen = Screen::Batch;
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("batch-item", 0usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("batch-tick", 1usize), cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        let b = &ws.read(cx).batch;
        assert_eq!(
            b.items.iter().map(|i| i.selected).collect::<Vec<_>>(),
            vec![true, true, false, false]
        );
        assert_eq!(b.current, Some(0));
    });
    // Kit checkboxes preserve focus on pointer clicks; Tab reaches the control.
    let mut focused = false;
    for _ in 0..100 {
        cx.update(|window, cx| window.focus_next(cx));
        cx.run_until_parked();
        focused =
            cx.update(|window, _| window.find(("batch-tick", 1usize)).focused() == Some(true));
        if focused {
            break;
        }
    }
    assert!(focused, "checkbox must be in the Tab order");
    let keystroke = gpui_kit::Keystroke::parse("space").unwrap();
    cx.simulate_event(gpui_kit::KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.simulate_event(gpui_kit::KeyUpEvent { keystroke });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let b = &ws.read(cx).batch;
        assert_eq!(
            b.items.iter().map(|i| i.selected).collect::<Vec<_>>(),
            vec![true, false, false, false]
        );
        assert_eq!(b.current, Some(0));
    });
}

#[gpui_kit::test]
fn library_failed_thumbnail_can_retry_after_file_is_repaired(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let path = fixture.0.join("sensor.dng");
    std::fs::write(&path, b"incomplete RAW").unwrap();
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1500.), gpui_kit::px(1000.)));
    cx.update(|_, cx| {
        ws.update(cx, |ws, cx| {
            ws.load_batch(fixture.0.clone(), vec![path.clone()], cx);
            ws.screen = Screen::Batch;
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(ws.read(cx).batch.items[0].thumb.is_none());
        assert!(window.find(("batch-thumb-retry", 0usize)).visible());
    });
    let state = tool_json(library_tool(
        &ws,
        cx,
        &fixture.0.join("catalog"),
        "get_library",
        serde_json::json!({}),
    ));
    assert_eq!(state["files"][0]["thumbnail"]["status"], "error");
    assert!(
        state["files"][0]["thumbnail"]["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
    );
    crate::raw_test_fixture::write_dng(&path);
    cx.update(|window, cx| window.click(("batch-thumb-retry", 0usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(ws.read(cx).batch.items[0].thumb.is_some());
        assert!(window.try_find(("batch-thumb-retry", 0usize)).is_none());
        assert!(ws.read(cx).batch.current.is_none());
    });
}

/// Run manually with EMULSION_LIBRARY_RAW_SAMPLES pointing at a local photo folder.
#[gpui_kit::test]
#[ignore = "requires local camera RAW samples"]
fn library_real_camera_thumbnails(cx: &mut TestAppContext) {
    let folder = std::path::PathBuf::from(
        std::env::var_os("EMULSION_LIBRARY_RAW_SAMPLES").expect("sample folder"),
    );
    let paths = std::fs::read_dir(&folder)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| emulsion_io::raw::is_raw(p))
        .collect::<Vec<_>>();
    assert!(!paths.is_empty());
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(2000.), gpui_kit::px(1500.)));
    cx.update(|_, cx| {
        ws.update(cx, |ws, cx| {
            ws.load_batch(folder, paths, cx);
            ws.screen = Screen::Batch;
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        for item in &ws.read(cx).batch.items {
            assert!(item.thumb.is_some(), "{} did not load", item.path.display());
        }
    });
    let fixture = Fixture::new();
    let count = cx.update(|_, cx| ws.read(cx).batch.items.len());
    for index in 0..count {
        cx.update(|window, cx| window.click(("batch-item", index), cx));
        cx.run_until_parked();
        let state = tool_json(library_tool(
            &ws,
            cx,
            &fixture.0.join("catalog"),
            "get_library",
            serde_json::json!({}),
        ));
        assert!(
            !state["develop"]["settings"].is_null(),
            "RAW selection failed: {state}"
        );
    }
}

#[gpui_kit::test]
fn library_import_primes_bounded_thumbnails_before_grid_layout(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let paths = (0..4)
        .map(|i| {
            let path = fixture.0.join(format!("sensor-{i}.dng"));
            crate::raw_test_fixture::write_dng(&path);
            path
        })
        .collect();
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    // Stay in Photo: no Library layout callback can seed the preview queue.
    cx.update(|_, cx| ws.update(cx, |ws, cx| ws.load_batch(fixture.0.clone(), paths, cx)));
    cx.run_until_parked();
    cx.update(|_, cx| {
        let b = &ws.read(cx).batch;
        assert!(b.items[0].thumb.is_some() && b.items[1].thumb.is_some());
        assert!(b.items[2].thumb.is_none() && b.items[3].thumb.is_none());
        assert!(b.current.is_none());
    });
}

#[gpui_kit::test]
fn library_preserves_unchanged_thumbnails_on_sort_reentry_and_external_edit(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let paths = fixture.pngs();
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1500.), gpui_kit::px(1000.)));
    cx.update(|_, cx| {
        ws.update(cx, |ws, cx| {
            ws.load_batch(fixture.0.clone(), paths.clone(), cx);
            ws.screen = Screen::Batch;
        })
    });
    cx.run_until_parked();
    let thumbs = cx.update(|_, cx| {
        ws.read(cx)
            .batch
            .items
            .iter()
            .map(|i| (i.path.clone(), i.thumb.clone().unwrap()))
            .collect::<std::collections::HashMap<_, _>>()
    });
    cx.update(|window, cx| window.click("library-sort", cx));
    cx.run_until_parked();
    cx.update(|_, cx| ws.update(cx, |ws, cx| ws.refresh_batch_recipes(cx)));
    cx.run_until_parked();
    cx.update(|_, cx| {
        for item in &ws.read(cx).batch.items {
            assert!(std::sync::Arc::ptr_eq(
                item.thumb.as_ref().unwrap(),
                &thumbs[&item.path]
            ));
        }
    });
    image::RgbaImage::from_pixel(20, 20, image::Rgba([90, 10, 60, 255]))
        .save(&paths[0])
        .unwrap();
    std::fs::File::options()
        .write(true)
        .open(&paths[0])
        .unwrap()
        .set_modified(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(123))
        .unwrap();
    cx.update(|_, cx| ws.update(cx, |ws, cx| ws.refresh_batch_recipes(cx)));
    cx.run_until_parked();
    cx.update(|_, cx| {
        for item in &ws.read(cx).batch.items {
            assert_eq!(
                std::sync::Arc::ptr_eq(item.thumb.as_ref().unwrap(), &thumbs[&item.path]),
                item.path != paths[0]
            );
        }
    });
}

#[gpui_kit::test]
fn library_autosave_keeps_unedited_thumbnails(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let mut paths = fixture.pngs();
    let raw = fixture.0.join("raw.dng");
    crate::raw_test_fixture::write_dng(&raw);
    paths.push(raw);
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1500.), gpui_kit::px(1500.)));
    cx.update(|_, cx| {
        ws.update(cx, |ws, cx| {
            ws.load_batch(fixture.0.clone(), paths, cx);
            ws.screen = Screen::Batch;
        })
    });
    cx.run_until_parked();
    let thumbs = cx.update(|_, cx| {
        ws.read(cx).batch.items[..4]
            .iter()
            .map(|i| i.thumb.clone().unwrap())
            .collect::<Vec<_>>()
    });
    cx.update(|window, cx| window.click(("batch-item", 4usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("library-raw-bw", cx));
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(500));
    cx.run_until_parked();
    cx.update(|_, cx| {
        for (item, thumb) in ws.read(cx).batch.items[..4].iter().zip(&thumbs) {
            assert!(
                std::sync::Arc::ptr_eq(item.thumb.as_ref().unwrap(), thumb),
                "autosave reloaded an unrelated photo"
            );
        }
    });
}

#[gpui_kit::test]
fn library_kit_filter_checkboxes_apply_requested_values(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let mut paths = fixture.pngs();
    let raw = fixture.0.join("raw.dng");
    crate::raw_test_fixture::write_dng(&raw);
    paths.push(raw);
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1500.), gpui_kit::px(1000.)));
    cx.update(|_, cx| {
        ws.update(cx, |ws, cx| {
            ws.load_batch(fixture.0.clone(), paths, cx);
            ws.screen = Screen::Batch;
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("library-filter-raw", cx));
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(ws.read(cx).batch.items.len(), 1));
    cx.update(|window, cx| window.click("library-filter-raw", cx));
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(ws.read(cx).batch.items.len(), 5));
}

#[gpui_kit::test]
fn library_rendered_develop_presets_snapshots_and_output_share_mcp(cx: &mut TestAppContext) {
    use serde_json::json;
    let fixture = Fixture::new();
    let paths = fixture.pngs();
    let original = std::fs::read(&paths[0]).unwrap();
    let root = fixture.0.join("catalog");
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1700.), gpui_kit::px(1100.)));
    cx.update(|_, cx| ws.update(cx, |ws, _| ws.screen = Screen::Batch));
    cx.run_until_parked();
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "import_library",
        json!({"folder":fixture.0}),
    ));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "select_library_photos",
        json!({"paths":[paths[0]],"active":paths[0]}),
    ));
    let result = tool_json(library_tool(
        &ws,
        cx,
        &root,
        "develop_library",
        json!({"action":"adjust","settings":{"exposure":0.5,"crop":[0.,0.,0.5,1.]}}),
    ));
    assert_eq!(result["develop"]["dirty"], false);
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "develop_library",
        json!({"action":"snapshot","name":"Crop"}),
    ));
    let preset = fixture.0.join("film.xmp");
    std::fs::write(&preset,r#"<rdf:Description xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:Exposure2012="1.25" crs:CameraProfile="VSCO custom profile"/>"#).unwrap();
    let imported = tool_json(library_tool(
        &ws,
        cx,
        &root,
        "develop_library",
        json!({"action":"load_preset","path":preset}),
    ));
    assert_eq!(imported["develop"]["settings"]["exposure"], 1.25);
    assert!(
        imported["preset_import"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("VSCO"))
    );
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "set_library_view",
        json!({"mode":"develop"}),
    ));
    cx.update(|window, cx| window.click(("library-left-section", 1usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.click("library-snapshot-save", cx);
        // A second event before the write finishes must not create another entry.
        window.click("library-snapshot-save", cx);
    });
    cx.run_until_parked();
    let digest = emulsion_io::raw::source_digest(&paths[0]).unwrap();
    let snapshots = emulsion_io::raw_settings::photo_history(&paths[0], &digest)
        .unwrap()
        .1;
    assert_eq!(snapshots.len(), 2);
    assert_eq!(snapshots["Snapshot 1"].exposure, 1.25);
    cx.update(|window, cx| window.click("library-snapshot-restore:Crop", cx));
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(500));
    cx.run_until_parked();
    let restored = emulsion_io::raw_settings::adjacent_settings(&paths[0], &digest).unwrap();
    assert_eq!(restored.exposure, 0.5);
    assert_eq!(
        emulsion_io::raw_settings::photo_history(&paths[0], &digest)
            .unwrap()
            .1
            .len(),
        2
    );
    cx.update(|window, cx| window.click("library-snapshot-delete:Snapshot 1", cx));
    cx.run_until_parked();
    let snapshots = emulsion_io::raw_settings::photo_history(&paths[0], &digest)
        .unwrap()
        .1;
    assert_eq!(snapshots.len(), 1);
    assert!(snapshots.contains_key("Crop"));
    assert_eq!(
        emulsion_io::raw_settings::adjacent_settings(&paths[0], &digest).unwrap(),
        restored
    );
    cx.update(|window, _| {
        assert!(
            window
                .try_find("library-snapshot-delete:Snapshot 1")
                .is_none()
        );
        assert!(window.find("library-snapshot-restore:Crop").visible());
    });
    let output = fixture.0.join("out");
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "export_library",
        json!({"out_dir":output,"format":"tif","settings":{"long_edge":2,"depth":16,"metadata":"camera"}}),
    ));
    let exported = image::open(output.join("photo-0.tif")).unwrap();
    assert_eq!((exported.width(), exported.height()), (1, 2));
    assert_eq!(std::fs::read(&paths[0]).unwrap(), original);
    for section in [
        "basic", "crop", "curve", "mixer", "grading", "masks", "kelvin", "history", "enhance",
    ] {
        tool_json(library_tool(
            &ws,
            cx,
            &root,
            "set_library_view",
            json!({"develop_section":section}),
        ));
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn library_assistant_host_does_not_create_a_photo_tab(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.tabs.clear();
            ws.editor = None;
            ws.screen = Screen::Batch;
            ws.library_ask(window, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let ws = ws.read(cx);
        assert!(ws.tabs.is_empty());
        assert!(ws.editor.is_none());
        let host = ws.batch.assistant_host.as_ref().unwrap().read(cx);
        assert!(host.library_only);
        assert!(host.library_workspace.is_some());
        assert!(host.ask.is_some());
    });
}

#[gpui_kit::test]
fn library_preset_import_details_are_collapsed_bounded_and_available_to_mcp(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let paths = fixture.pngs();
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1700.), gpui_kit::px(1500.)));
    cx.update(|_, cx| {
        ws.update(cx, |ws, cx| {
            ws.load_batch(fixture.0.clone(), paths, cx);
            ws.screen = Screen::Batch;
            ws.record_preset_import(
                1,
                (0..40)
                    .map(|i| format!("Active unsupported adjustment {i}"))
                    .collect(),
                cx,
            );
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("batch-item", 0usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let note = &ws.read(cx).batch.note.as_ref().unwrap().0;
        assert!(note.starts_with("Imported 1 preset."));
        assert!(note.len() < 200);
        assert!(!note.contains("Active unsupported"));
        assert!(window.try_find("library-preset-import-notes").is_none());
        window.click("library-preset-import-details", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(
            window
                .find("library-preset-import-notes")
                .bounds()
                .size
                .height
                <= gpui_kit::px(160.)
        );
        window.click("library-preset-import-details", cx);
    });
    cx.run_until_parked();
    let state = tool_json(library_tool(
        &ws,
        cx,
        &fixture.0.join("catalog"),
        "get_library",
        serde_json::json!({}),
    ));
    assert_eq!(state["preset_import_notes"].as_array().unwrap().len(), 40);
    cx.update(|window, _| assert!(window.try_find("library-preset-import-notes").is_none()));
}

#[gpui_kit::test]
fn library_desktop_layout_and_local_edits_share_mcp(cx: &mut TestAppContext) {
    use serde_json::json;
    let fixture = Fixture::new();
    let photos = fixture.pngs();
    let root = fixture.0.join("catalog");
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1600.), gpui_kit::px(1100.)));
    cx.update(|_, cx| ws.update(cx, |ws, _| ws.screen = Screen::Batch));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "import_library",
        json!({"folder":fixture.0}),
    ));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "select_library_photos",
        json!({"paths":[photos[0]],"active":photos[0]}),
    ));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "set_library_view",
        json!({"mode":"develop","develop_section":"detail","auto_advance":true}),
    ));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let left = window.find("library-navigation").bounds();
        let right = window.find("library-settings-panel").bounds();
        let film = window.find("library-filmstrip").bounds();
        let preview = window.find("library-preview").bounds();
        assert!(left.origin.x < preview.origin.x);
        assert!(right.origin.x > preview.origin.x);
        assert!(film.origin.y >= preview.origin.y + preview.size.height);
        assert!(film.size.width > preview.size.width);
        let _ = cx;
    });
    let edits = json!({"version":1,"masks":[{"id":1,"name":"Center","enabled":true,"components":[{"operation":"add","shape":{"type":"radial","center":[0.5,0.5],"radius":[0.5,0.5],"feather":0.5}}],"exposure":1.,"contrast":0.,"saturation":0.,"temperature":0.,"tint":0.}],"spots":[]});
    let state = tool_json(library_tool(
        &ws,
        cx,
        &root,
        "develop_library",
        json!({"action":"local_edits","edits":edits}),
    ));
    assert!(state["develop"]["settings"]["local_edits"].is_array());
    let params = emulsion_io::raw_settings::adjacent_settings(
        &photos[0],
        &emulsion_io::raw::source_digest(&photos[0]).unwrap(),
    )
    .unwrap();
    assert!(params.local_edits.is_some());
    let source = emulsion_io::photo_develop::PhotoSource::load(&photos[0]).unwrap();
    assert_ne!(
        source.develop_with(&params).unwrap().get(2, 2),
        source.develop_with(&Default::default()).unwrap().get(2, 2)
    );
}

#[gpui_kit::test]
fn library_classic_chrome_keeps_tools_and_footer_visible(cx: &mut TestAppContext) {
    use gpui_kit::{ScrollDelta, point, px, size};
    let fixture = Fixture::new();
    let paths = fixture.pngs();
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.simulate_resize(size(px(1440.), px(900.)));
    cx.update(|_, cx| {
        ws.update(cx, |ws, cx| {
            ws.load_batch(fixture.0.clone(), paths, cx);
            ws.screen = Screen::Batch;
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("library-navigator").visible());
        window.click(("batch-item", 0usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("library-module", 1usize), cx));
    cx.run_until_parked();
    for (w, h) in [(1440., 900.), (1280., 720.)] {
        cx.simulate_resize(size(px(w), px(h)));
        cx.run_until_parked();
        cx.update(|window, cx| {
            let histogram = window.find("library-histogram-panel").bounds();
            let tools = window.find("library-editing-toolstrip").bounds();
            let footer = window.find("library-develop-footer").bounds();
            let strip = window.find("library-filmstrip").bounds();
            assert!(histogram.bottom() <= tools.top());
            assert!(tools.bottom() < footer.top());
            assert!(footer.bottom() <= strip.top());
            assert!(window.find("library-footer-reset").visible());
            let row = window.find(("library-basic-row", 0usize)).bounds();
            assert!(row.size.height <= px(28.));
            window.scroll(
                "library-adjustment-scroll",
                ScrollDelta::Pixels(point(px(0.), px(-2000.))),
                cx,
            );
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.find("library-rgb-histogram").visible());
            assert!(window.find("library-footer-sync").visible());
            window.click(("library-editing-tool", 5usize), cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.try_find(("develop-geometry-tool", 5usize)).is_some());
            assert_eq!(ws.read(cx).batch.current, Some(0));
            window.click(("library-module", 0usize), cx);
        });
        cx.run_until_parked();
        let state = tool_json(library_tool(
            &ws,
            cx,
            &fixture.0.join("catalog"),
            "get_library",
            serde_json::json!({}),
        ));
        assert_eq!(state["layout"]["canvas_tool"], 0);

        cx.update(|window, cx| {
            assert!(window.find(("batch-item", 0usize)).visible());
            window.click(("library-module", 1usize), cx);
        });
        cx.run_until_parked();
        tool_json(library_tool(
            &ws,
            cx,
            &fixture.0.join("catalog"),
            "set_library_view",
            serde_json::json!({"mode":"develop","develop_section":"basic"}),
        ));
    }
    let state = tool_json(library_tool(
        &ws,
        cx,
        &fixture.0.join("catalog"),
        "set_library_view",
        serde_json::json!({"mode":"develop","canvas_tool":"radial"}),
    ));
    assert_eq!(state["layout"]["canvas_tool"], 8);
    assert_eq!(state["layout"]["develop_section"], 5);
    let bad = library_tool(
        &ws,
        cx,
        &fixture.0.join("catalog"),
        "set_library_view",
        serde_json::json!({"mode":"grid","canvas_tool":"unknown","panels_hidden":true}),
    );
    assert!(bad.is_error);
    let state = tool_json(library_tool(
        &ws,
        cx,
        &fixture.0.join("catalog"),
        "get_library",
        serde_json::json!({}),
    ));
    assert_eq!(state["view"], "develop");
    assert_eq!(state["layout"]["canvas_tool"], 8);
    assert_eq!(state["layout"]["panels_hidden"], false);
    cx.update(|window, cx| window.click("library-tool-done", cx));
    cx.run_until_parked();
    let state = tool_json(library_tool(
        &ws,
        cx,
        &fixture.0.join("catalog"),
        "get_library",
        serde_json::json!({}),
    ));
    assert_eq!(state["layout"]["canvas_tool"], 0);
}

#[gpui_kit::test]
fn library_rotation_exits_detail_and_straightening_is_undoable(cx: &mut TestAppContext) {
    use gpui_kit::{point, px, size};
    use serde_json::json;
    let fixture = Fixture::new();
    let path = fixture.0.join("square.png");
    image::RgbaImage::from_fn(32, 32, |x, y| {
        image::Rgba([x as u8 * 7, y as u8 * 7, 80, 255])
    })
    .save(&path)
    .unwrap();
    let original = std::fs::read(&path).unwrap();
    let root = fixture.0.join("catalog");
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.simulate_resize(size(px(1440.), px(1000.)));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "import_library",
        json!({"folder":fixture.0}),
    ));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "select_library_photos",
        json!({"paths":[path],"active":path}),
    ));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "develop_library",
        json!({"action":"adjust","settings":{"exposure":0.5}}),
    ));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "set_library_view",
        json!({"mode":"develop","detail_region":[0.2,0.8]}),
    ));
    cx.update(|_, cx| ws.update(cx, |ws, _| ws.screen = Screen::Batch));
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("library-rotate", 3usize), cx));
    cx.run_until_parked();
    let state = tool_json(library_tool(&ws, cx, &root, "get_library", json!({})));
    assert_eq!(state["develop"]["settings"]["rotation"], 3);
    assert!(state["layout"]["detail_region"].is_null());
    cx.update(|window, cx| window.click("library-rotation-toggle", cx));
    cx.run_until_parked();
    let steps = state["develop"]["undo_steps"].as_u64().unwrap();
    let (start, end) = cx.update(|window, _| {
        assert!(window.find("library-rotation-panel").visible());
        let slider = window.within("library-rotation-angle");
        let bounds = slider.find("slider-bar-container").bounds();
        (
            point(bounds.left() + bounds.size.width * 0.60, bounds.center().y),
            point(bounds.left() + bounds.size.width * 0.75, bounds.center().y),
        )
    });
    cx.simulate_mouse_down(start, gpui_kit::MouseButton::Left, Default::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(end, Some(gpui_kit::MouseButton::Left), Default::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(end, Some(gpui_kit::MouseButton::Left), Default::default());
    cx.run_until_parked();
    cx.simulate_mouse_up(end, gpui_kit::MouseButton::Left, Default::default());
    cx.run_until_parked();
    let state = tool_json(library_tool(&ws, cx, &root, "get_library", json!({})));
    let angle = state["develop"]["settings"]["straighten"].as_f64().unwrap();
    assert!(
        angle > 15. && angle < 30.,
        "drag should reach roughly 22.5°, got {angle}"
    );
    assert_eq!(
        state["develop"]["undo_steps"],
        steps + 1,
        "one undo step per slider gesture"
    );
    cx.update(|window, cx| window.click("library-rotation-reset", cx));
    cx.run_until_parked();
    let state = tool_json(library_tool(&ws, cx, &root, "get_library", json!({})));
    assert_eq!(state["develop"]["settings"]["rotation"], 0);
    assert_eq!(state["develop"]["settings"]["straighten"], 0.);
    assert_eq!(state["develop"]["settings"]["exposure"], 0.5);
    let undone = tool_json(library_tool(
        &ws,
        cx,
        &root,
        "develop_library",
        json!({"action":"undo"}),
    ));
    assert_eq!(undone["develop"]["settings"]["rotation"], 3);
    assert_eq!(undone["develop"]["settings"]["straighten"], angle);
    cx.update(|window, cx| window.click("library-rotation-done", cx));
    cx.run_until_parked();
    cx.update(|window, _| assert!(window.try_find("library-rotation-panel").is_none()));
    let source = emulsion_io::photo_develop::PhotoSource::load(&path).unwrap();
    let saved = emulsion_io::raw_settings::adjacent_settings(&path, &source.source_sha256).unwrap();
    assert_eq!(saved.rotation, 3);
    assert!((saved.straighten as f64 - angle).abs() < 0.0001);
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[gpui_kit::test]
fn library_rotation_curve_and_profile_controls_share_mcp_state(cx: &mut TestAppContext) {
    use gpui_kit::{point, px, size};
    use serde_json::json;
    let fixture = Fixture::new();
    let path = fixture.0.join("wide.png");
    image::RgbaImage::from_fn(12, 8, |x, y| {
        image::Rgba([(x * 17) as u8, (y * 29) as u8, 80, 255])
    })
    .save(&path)
    .unwrap();
    let original = std::fs::read(&path).unwrap();
    let root = fixture.0.join("catalog");
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.simulate_resize(size(px(1440.), px(1000.)));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "import_library",
        json!({"folder":fixture.0}),
    ));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "select_library_photos",
        json!({"paths":[path],"active":path}),
    ));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "set_library_view",
        json!({"mode":"develop","develop_section":"curve"}),
    ));
    // Initialize the fixture catalog before rendering Library can load the global catalog.
    cx.update(|_, cx| ws.update(cx, |ws, _| ws.screen = Screen::Batch));
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("library-rotate", 1usize), cx));
    cx.run_until_parked();
    let state = tool_json(library_tool(&ws, cx, &root, "get_library", json!({})));
    assert_eq!(state["develop"]["settings"]["rotation"], 1);
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(600));
    cx.run_until_parked();
    let source = emulsion_io::photo_develop::PhotoSource::load(&path).unwrap();
    let params =
        emulsion_io::raw_settings::adjacent_settings(&path, &source.source_sha256).unwrap();
    let rendered = source.develop_with(&params).unwrap();
    assert_eq!((rendered.width(), rendered.height()), (8, 12));
    // Rotation updates only this photo's saved thumbnail, with no change to originals.
    cx.update(|_, cx| {
        let batch = &ws.read(cx).batch;
        let thumb = batch
            .items
            .iter()
            .find(|i| i.path == path)
            .unwrap()
            .thumb
            .as_ref()
            .unwrap();
        let size = thumb.size(0);
        assert!(size.width.0 < size.height.0);
        assert_eq!(size.width.0 * 3, size.height.0 * 2);
    });
    let bounds = cx.update(|window, _| window.find("library-curve-graph").bounds());
    let at = |x: f32, y: f32| {
        bounds.origin + point(bounds.size.width * x, bounds.size.height * (1. - y))
    };
    cx.update(|window, cx| window.drag(at(0.5, 0.5), at(0.5, 0.7), cx));
    cx.run_until_parked();
    cx.update(|window, _| {
        assert_eq!(
            window.find("library-curve-graph").bounds(),
            bounds,
            "Editing must not move the curve graph"
        )
    });
    let state = tool_json(library_tool(&ws, cx, &root, "get_library", json!({})));
    assert_eq!(state["develop"]["settings"]["smooth_point_curves"][0], true);
    assert_eq!(
        state["develop"]["undo_steps"], 2,
        "rotation and one curve gesture"
    );
    assert!(
        state["develop"]["settings"]["point_curves"][0]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p[1].as_f64().unwrap() > 0.65 && p[1].as_f64().unwrap() < 0.75)
    );
    cx.update(|window, cx| window.click("develop-profile-toggle", cx));
    cx.run_until_parked();
    cx.update(|window, _| assert!(window.find("library-profile-browser").visible()));
    let preview = library_tool(
        &ws,
        cx,
        &root,
        "library_profiles",
        json!({"action":"preview"}),
    );
    assert!(!preview.is_error, "{:?}", preview.content);
    assert!(preview.content.len() > 1);
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[gpui_kit::test]
fn library_hdr_preview_merge_and_dialog_controls(cx: &mut TestAppContext) {
    use serde_json::json;
    let fixture = Fixture::new();
    let paths = fixture.pngs();
    let root = fixture.0.join("catalog");
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "import_library",
        json!({"folder":fixture.0}),
    ));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "select_library_photos",
        json!({"paths":[paths[0],paths[1]],"active":paths[0]}),
    ));
    cx.update(|_, cx| ws.update(cx, |ws, _| ws.screen = Screen::Batch));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("library-hdr-merge", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("hdr-align").visible());
        assert!(window.find("hdr-auto-tone").visible());
        assert!(window.find("hdr-merge").visible());
        window.click("hdr-cancel", cx);
    });
    cx.run_until_parked();
    let options = json!({"align":false,"exposure_ev":[-1,1]});
    let result = library_tool(
        &ws,
        cx,
        &root,
        "merge_library_hdr",
        json!({"paths":[paths[0],paths[1]],"preview":true,"options":options}),
    );
    assert!(!result.is_error, "{:?}", result.content);
    assert!(result.content.len() > 1);
    let output = fixture.0.join("merged.tif");
    let result = library_tool(
        &ws,
        cx,
        &root,
        "merge_library_hdr",
        json!({"paths":[paths[0],paths[1]],"output":output,"options":options}),
    );
    assert!(!result.is_error, "{:?}", result.content);
    assert!(emulsion_io::photo_hdr::load(&output).unwrap().is_some());
    let state = tool_json(library_tool(&ws, cx, &root, "get_library", json!({})));
    assert_eq!(state["total"], 5);
    assert_eq!(state["hdr_busy"], false);
    let overwrite = library_tool(
        &ws,
        cx,
        &root,
        "merge_library_hdr",
        json!({"paths":[paths[0],paths[1]],"output":output,"options":options}),
    );
    assert!(overwrite.is_error);
}

#[gpui_kit::test]
fn library_guided_perspective_saves_and_undoes_shared_settings(cx: &mut TestAppContext) {
    use serde_json::json;
    let fixture = Fixture::new();
    let path = fixture.0.join("guides.png");
    image::RgbaImage::from_pixel(32, 32, image::Rgba([80, 100, 120, 255]))
        .save(&path)
        .unwrap();
    let original = std::fs::read(&path).unwrap();
    let root = fixture.0.join("catalog");
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "import_library",
        json!({"folder":fixture.0}),
    ));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "select_library_photos",
        json!({"paths":[path],"active":path}),
    ));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "develop_library",
        json!({
            "action":"guided_perspective", "guides":[[[0.2,0.1],[0.3,0.9]],[[0.8,0.1],[0.7,0.9]]]
        }),
    ));
    let state = tool_json(library_tool(&ws, cx, &root, "get_library", json!({})));
    assert!(
        state["develop"]["settings"]["perspective"][1]
            .as_f64()
            .unwrap()
            .abs()
            > 0.01
    );
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "develop_library",
        json!({"action":"undo"}),
    ));
    let state = tool_json(library_tool(&ws, cx, &root, "get_library", json!({})));
    assert_eq!(state["develop"]["settings"]["perspective"], json!([0., 0.]));
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[gpui_kit::test]
fn library_depth_and_rgb_gamut_edits_persist_and_undo(cx: &mut TestAppContext) {
    use serde_json::json;
    let fixture = Fixture::new();
    let path = fixture.pngs().remove(0);
    let root = fixture.0.join("catalog");
    let original = std::fs::read(&path).unwrap();
    let map = emulsion_raster::Mask::from_pixels(4, 4, 0, &[128; 16]);
    let digest = emulsion_io::photo_develop::save_mask(&map).unwrap();
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "import_library",
        json!({"folder":fixture.0}),
    ));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "select_library_photos",
        json!({"paths":[path],"active":path}),
    ));
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "develop_library",
        json!({"action":"adjust","settings":{"wide_gamut":true,"depth_map":digest,"depth_blur":0.015,"depth_focus":0.7,"depth_range":0.08}}),
    ));
    let state = tool_json(library_tool(&ws, cx, &root, "get_library", json!({})));
    assert_eq!(state["develop"]["settings"]["wide_gamut"], true);
    assert!(
        (state["develop"]["settings"]["depth_focus"]
            .as_f64()
            .unwrap()
            - 0.7)
            .abs()
            < 1e-5
    );
    let source = emulsion_io::photo_develop::PhotoSource::load(&path).unwrap();
    let saved = emulsion_io::raw_settings::adjacent_settings(&path, &source.source_sha256).unwrap();
    assert_eq!(saved.depth_map, Some(digest));
    assert!(saved.wide_gamut);
    tool_json(library_tool(
        &ws,
        cx,
        &root,
        "develop_library",
        json!({"action":"undo"}),
    ));
    let state = tool_json(library_tool(&ws, cx, &root, "get_library", json!({})));
    assert_eq!(state["develop"]["settings"]["depth_blur"], 0.);
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[gpui_kit::test]
fn library_neutral_picker_preserves_geometry_and_saves_recipe(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let path = fixture.0.join("neutral.dng");
    crate::raw_test_fixture::write_dng(&path);
    let source = emulsion_io::photo_develop::PhotoSource::load(&path).unwrap();
    let params = emulsion_core::raw::DevelopParams {
        rotation: 1,
        crop: [0.1, 0.1, 0.9, 0.9],
        ..Default::default()
    };
    source.save(params).unwrap();
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1500.), gpui_kit::px(1100.)));
    cx.update(|window, cx| ws.update(cx, |ws, cx| ws.open_path(path.clone(), window, cx)));
    cx.run_until_parked();
    tool_json(library_tool(
        &ws,
        cx,
        &fixture.0.join("catalog"),
        "set_library_view",
        serde_json::json!({"canvas_tool":"white_balance"}),
    ));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let position = window.find("library-preview").bounds().center();
        window.drag(position, position, cx);
    });
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(2));
    cx.run_until_parked();
    let state = tool_json(library_tool(
        &ws,
        cx,
        &fixture.0.join("catalog"),
        "get_library",
        serde_json::json!({}),
    ));
    assert_eq!(state["layout"]["canvas_tool"], 0, "{state}");
    let sampled: emulsion_core::raw::DevelopParams =
        serde_json::from_value(state["develop"]["settings"].clone()).unwrap();
    assert!(sampled.wb_override.is_some());
    assert_eq!(sampled.crop, params.crop);
    assert_eq!(sampled.rotation, params.rotation);
    assert_eq!(
        emulsion_io::raw_settings::adjacent_settings(&path, &source.source_sha256).unwrap(),
        sampled
    );
}
