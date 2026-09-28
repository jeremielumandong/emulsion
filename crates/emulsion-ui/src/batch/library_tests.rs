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
        Self(path)
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
    cx.update(|window, cx| window.click("library-selection-sync", cx));
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
    assert_eq!(
        image::open(out.join("a.png")).unwrap().to_rgba8().as_raw(),
        &emulsion_raster::composite::flatten(&doc.composite_tree(), 0).to_srgba8()
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
