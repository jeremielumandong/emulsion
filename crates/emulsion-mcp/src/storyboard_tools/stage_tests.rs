use super::execute;
use super::tests::{board, call};
use emulsion_core::project::ProjectEditor;
use emulsion_core::raster::{BlendMode, Placement, Raster};
use emulsion_core::{Command, Document, Node, command::Slot};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;

fn refused(e: &mut ProjectEditor, name: &str, args: Value) -> String {
    let stamp = e.stamp();
    let active = e.active_page();
    let result = execute(e, name, &args);
    assert!(result.is_error, "{name} {args} should fail");
    assert_eq!(e.stamp(), stamp, "{name} {args} changed the project");
    assert_eq!(e.active_page(), active, "{name} {args} changed the panel");
    result.content[0]["text"].as_str().unwrap().to_string()
}

fn layer(doc: &mut Document, name: &str, size: (u32, u32), rgba: [f32; 4]) -> u64 {
    Command::AddNode {
        node: Box::new(Node::raster(
            0,
            name,
            Arc::new(Raster::solid(size.0, size.1, rgba)),
            Placement::default(),
        )),
        slot: Slot::TOP,
    }
    .apply(doc)
    .unwrap()
    .unwrap()
}

/// A layered PSD layout (a multiply shadow clipped to a figure), a flat PNG
/// reference and a native ORA set, at other sizes than the 64 × 36 board.
fn files(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("sb-stage-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut layout = Document::new(128, 72);
    let figure = layer(&mut layout, "Figure", (128, 72), [0.8, 0.2, 0.1, 1.]);
    let shadow = layer(&mut layout, "Shadow", (128, 72), [0.2, 0.2, 0.4, 1.]);
    let node = layout.nodes.iter_mut().find(|n| n.id == shadow).unwrap();
    node.blend = BlendMode::Multiply;
    node.clip_to = Some(figure);
    let psd = dir.join("Layout.psd");
    emulsion_io::psd::write(&layout, &psd).unwrap();
    let png = dir.join("Reference.png");
    image::RgbaImage::from_pixel(40, 40, image::Rgba([128, 128, 128, 255]))
        .save(&png)
        .unwrap();
    let mut set = Document::new(64, 36);
    layer(&mut set, "Set", (64, 36), [0.9, 0.9, 0.9, 1.]);
    let ora = dir.join("Set.ora");
    emulsion_io::save(&set, &ora).unwrap();
    (psd, png, ora)
}

#[test]
fn guides_and_palette_are_described_set_and_undone_in_one_step() {
    let mut e = board();
    let outline = call(&mut e, "describe_storyboard", json!({}));
    assert_eq!(outline["guides"]["action_safe"], 90.);
    assert_eq!(outline["guides"]["field_rects"], json!([]));
    assert_eq!(outline["palette"].as_array().unwrap().len(), 10);
    assert_eq!(outline["palette"][7], "#D63030");
    let set = call(
        &mut e,
        "set_storyboard_settings",
        json!({
            "guides":{"title_safe":0,"field_guide":true,"fields":4,"overscan":25},
            "palette":{"remove":["#FFFFFF"],"add":["#ff00ff","#FF00FF","#000000"]}
        }),
    );
    let guides = &set["guides"];
    assert_eq!(guides["action_safe"], 90., "omitted guides stay");
    assert_eq!(
        guides["action_safe_rect"],
        json!({"x":3.2,"y":1.8,"width":57.6,"height":32.4})
    );
    assert!(guides["title_safe_rect"].is_null(), "0% hides a guide");
    assert_eq!(guides["field_rects"].as_array().unwrap().len(), 4);
    assert_eq!(guides["field_rects"][0]["width"], 16.);
    assert_eq!(
        guides["stage_area"],
        json!({"x":-16.,"y":-9.,"width":96.,"height":54.})
    );
    let palette = set["palette"].as_array().unwrap();
    assert_eq!(palette.len(), 10, "white removed, magenta added once");
    assert_eq!(palette.last().unwrap(), "#FF00FF");
    assert!(!palette.contains(&json!("#FFFFFF")));
    assert_eq!(
        call(&mut e, "describe_storyboard", json!({}))["guides"]["fields"],
        4
    );
    let board = e.storyboard().unwrap();
    assert!(board.stage.field_guide && board.palette.len() == 10);
    call(
        &mut e,
        "set_storyboard_settings",
        json!({"palette":{"set":["#111111","#222222","#111111"]}}),
    );
    assert_eq!(e.storyboard().unwrap().palette, [[17; 3], [34; 3]]);
    call(
        &mut e,
        "set_storyboard_settings",
        json!({"palette":{"set":[]}}),
    );
    assert!(e.storyboard().unwrap().palette.is_empty());
    call(
        &mut e,
        "set_storyboard_settings",
        json!({"palette":{"reset":true}}),
    );
    assert_eq!(
        e.storyboard().unwrap().palette,
        emulsion_core::storyboard_stage::DEFAULT_PALETTE
    );
    // Each call is one step: three undos go back to the first change.
    for _ in 0..3 {
        assert!(e.undo());
    }
    let board = e.storyboard().unwrap();
    assert_eq!(board.palette.len(), 10);
    assert!(board.palette.contains(&[255, 0, 255]));
    assert!(e.undo());
    let board = e.storyboard().unwrap();
    assert_eq!(board.stage, Default::default());
    assert!(board.palette.contains(&[255; 3]));
}

#[test]
fn invalid_guides_and_palettes_change_nothing() {
    let mut e = board();
    let many: Vec<_> = (0..65).map(|i| format!("#0000{i:02X}")).collect();
    let full: Vec<_> = (0..64).map(|i| format!("#0000{i:02X}")).collect();
    for args in [
        json!({"guides":{"action_safe":120}}),
        json!({"guides":{"fields":1}}),
        json!({"guides":{"overscan":-5}}),
        json!({"guides":{"bleed":3}}),
        json!({"palette":{"add":["red"]}}),
        json!({"palette":{"add":["#12345"]}}),
        json!({"palette":{"add":[]}}),
        json!({"palette":{"remove":["#123456"]}}),
        json!({"palette":{"reset":true,"set":["#000000"]}}),
        json!({"palette":{"set":many}}),
        json!({"palette":{"set":full,"add":["#FFFFFF"]}}),
        // A good change in the same call does not land either.
        json!({"frame_rate":"25","palette":{"remove":["#123456"]}}),
    ] {
        refused(&mut e, "set_storyboard_settings", args);
    }
    assert!(!e.can_undo());
}

#[test]
fn files_become_panels_named_after_them_in_one_step() {
    let mut e = board();
    let (psd, png, ora) = files("panels");
    let result = call(
        &mut e,
        "import_storyboard_files",
        json!({"paths":[psd, png, ora],"after":1}),
    );
    let panels = result["panels"].as_array().unwrap();
    assert_eq!(panels.len(), 3);
    assert_eq!(panels[0]["name"], "Layout");
    assert_eq!(panels[1]["name"], "Reference");
    assert_eq!(panels[2]["layers"][0]["name"], "Set");
    let layers = panels[0]["layers"].as_array().unwrap();
    let shadow = layers.iter().find(|l| l["name"] == "Shadow").unwrap();
    let figure = layers.iter().find(|l| l["name"] == "Figure").unwrap();
    assert_eq!(shadow["blend"], BlendMode::Multiply.label());
    assert_eq!(shadow["clipped_to"], figure["id"]);
    let ids: Vec<u64> = panels
        .iter()
        .map(|p| p["panel"].as_u64().unwrap())
        .collect();
    assert_eq!(result["active_panel"], ids[0]);
    for id in &ids {
        let doc = &e.page(*id).unwrap().doc;
        assert_eq!((doc.width, doc.height), (64, 36), "fitted to the frame");
    }
    let board = e.storyboard().unwrap();
    assert_eq!(board.panels[&ids[0]].scene, board.panels[&1].scene);
    assert_eq!(super::layout(&e), [1, ids[0], ids[1], ids[2]]);
    e.snapshot().unwrap().validate().unwrap();
    assert!(e.undo());
    assert_eq!(super::layout(&e), [1]);
    // at_start puts the panel first.
    let first = call(
        &mut e,
        "import_storyboard_files",
        json!({"paths":[png],"at_start":true}),
    );
    assert_eq!(super::layout(&e)[0], first["panels"][0]["panel"]);
}

#[test]
fn files_become_layers_on_a_panel_in_one_step() {
    let mut e = board();
    call(&mut e, "add_storyboard_panels", json!({"panels":[{}]}));
    let second = e.active_page();
    let (psd, png, _) = files("layers");
    let before = e.page(1).unwrap().doc.nodes.len();
    let result = call(
        &mut e,
        "import_storyboard_files",
        json!({"paths":[png, psd],"into":"layers","panel":1}),
    );
    assert_eq!(result["panel"], 1);
    assert_eq!(e.active_page(), 1, "the panel is selected");
    let names: Vec<_> = result["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["Reference", "Figure", "Shadow"],
        "later files on top"
    );
    let shadow = &result["layers"][2];
    assert_eq!(shadow["blend"], BlendMode::Multiply.label());
    assert_eq!(shadow["clipped_to"], result["layers"][1]["id"]);
    let doc = &e.page(1).unwrap().doc;
    assert_eq!(doc.nodes.len(), before + 3);
    assert_eq!((doc.width, doc.height), (64, 36));
    assert!(e.page(second).unwrap().doc.nodes.len() == 1);
    assert!(e.undo());
    assert_eq!(e.page(1).unwrap().doc.nodes.len(), before, "one step");
    // A single file onto the active panel.
    e.set_active_page(second).unwrap();
    let placed = call(
        &mut e,
        "import_storyboard_files",
        json!({"paths":[png],"into":"layers"}),
    );
    assert_eq!(placed["panel"], second);
}

#[test]
fn bad_imports_change_nothing() {
    let mut e = board();
    let (psd, png, _) = files("refused");
    let missing = psd.with_file_name("Missing.psd");
    let text = psd.with_file_name("notes.txt");
    std::fs::write(&text, "not a picture").unwrap();
    for args in [
        json!({"paths":[]}),
        json!({"paths":["relative/Layout.psd"]}),
        json!({"paths":[text]}),
        json!({"paths":[png, missing]}),
        json!({"paths":[png],"into":"layers","panel":99}),
        json!({"paths":[png],"into":"layers","after":1}),
        json!({"paths":[png],"panel":1}),
        json!({"paths":[png],"after":99}),
        json!({"paths":[png],"after":1,"at_start":true}),
        json!({"paths":[png],"into":"sheet"}),
    ] {
        refused(&mut e, "import_storyboard_files", args);
    }
    call(
        &mut e,
        "set_storyboard_locks",
        json!({"panels":[1],"locked":true}),
    );
    refused(
        &mut e,
        "import_storyboard_files",
        json!({"paths":[psd],"into":"layers","panel":1}),
    );
}

#[test]
fn copy_page_nodes_pastes_in_place_across_panels() {
    let mut e = board();
    let drawn = crate::exec::execute(
        &mut e,
        "draw_shape",
        &json!({"shape":"rectangle","name":"Hero","x":10,"y":8,"width":12,"height":20,"mode":"shape"}),
    );
    assert!(!drawn.is_error, "{}", drawn.content[0]["text"]);
    let hero = e.doc.nodes.iter().find(|n| n.name == "Hero").unwrap().id;
    let bounds = emulsion_core::geometry::node_bounds(&e.doc, hero);
    let added = call(&mut e, "add_storyboard_panels", json!({"panels":[{}]}));
    let next = added["panels"][0].as_u64().unwrap();
    let copied = crate::project_tools::execute(
        &mut e,
        "copy_page_nodes",
        &json!({"from":1,"nodes":[hero],"to":next}),
    );
    assert!(!copied.is_error, "{}", copied.content[0]["text"]);
    assert_eq!(e.active_page(), next);
    let pasted = e.doc.nodes.iter().find(|n| n.name == "Hero").unwrap().id;
    assert_eq!(emulsion_core::geometry::node_bounds(&e.doc, pasted), bounds);
}
