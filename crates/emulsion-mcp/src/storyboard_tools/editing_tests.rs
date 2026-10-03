use super::execute;
use super::tests::{board, call};
use emulsion_core::project::ProjectEditor;
use serde_json::{Value, json};

fn layout(e: &ProjectEditor) -> Vec<u64> {
    super::layout(e)
}

fn scene_of(e: &ProjectEditor, panel: u64) -> u64 {
    e.storyboard().unwrap().panels[&panel].scene
}

fn refused(e: &mut ProjectEditor, name: &str, args: Value) -> String {
    let stamp = e.stamp();
    let result = execute(e, name, &args);
    assert!(result.is_error, "{name} {args} should fail");
    assert_eq!(e.stamp(), stamp, "{name} {args} changed the project");
    result.content[0]["text"].as_str().unwrap().to_string()
}

/// Four panels in two scenes: [1, 2] and [3, 4]. Returns the second scene.
fn two_scenes(e: &mut ProjectEditor) -> u64 {
    call(e, "add_storyboard_panels", json!({"panels":[{}]}));
    let added = call(
        e,
        "add_storyboard_panels",
        json!({"after":layout(e)[1],"start":"scene","group_name":"Street","panels":[{},{}]}),
    );
    scene_of(e, added["panels"][0].as_u64().unwrap())
}

#[test]
fn locks_protect_panels_and_scenes_until_unlocked() {
    let mut e = board();
    let street = two_scenes(&mut e);
    let ids = layout(&e);
    call(
        &mut e,
        "set_storyboard_locks",
        json!({"panels":[ids[0]],"scenes":[street],"locked":true}),
    );
    let outline = call(&mut e, "describe_storyboard", json!({}));
    let scenes = &outline["acts"][0]["sequences"][0]["scenes"];
    assert_eq!(scenes[0]["panels"][0]["locked"], true);
    assert_eq!(scenes[1]["locked"], true);
    for panel in [ids[0], ids[2]] {
        refused(
            &mut e,
            "update_storyboard_panel",
            json!({"panel":panel,"frames":3}),
        );
    }
    assert!(
        refused(
            &mut e,
            "set_storyboard_locks",
            json!({"panels":[ids[3]],"locked":true})
        )
        .contains("locked scene")
    );
    refused(&mut e, "join_storyboard_group", json!({"group":street}));
    refused(
        &mut e,
        "move_storyboard_panels",
        json!({"panels":[ids[0]],"at_start":true}),
    );
    // Locked panels refuse drawing as well.
    e.set_active_page(ids[0]).unwrap();
    assert!(e.is_read_only());
    // Undo restores both locks in one step.
    assert!(e.undo());
    let board = e.storyboard().unwrap();
    assert!(!board.is_locked(ids[0]) && !board.is_locked(ids[2]));
    call(
        &mut e,
        "update_storyboard_panel",
        json!({"panel":ids[2],"frames":3}),
    );
}

#[test]
fn smart_add_carries_the_listed_layers_into_the_next_panel() {
    let mut e = board();
    call(
        &mut e,
        "set_storyboard_settings",
        json!({"smart_add_layers":["Set"],"naming":{"panel_prefix":"P","panel_digits":2}}),
    );
    for name in ["Set", "Hero"] {
        let drawn = crate::exec::execute(
            &mut e,
            "draw_shape",
            &json!({"shape":"rectangle","name":name,"x":4,"y":4,"width":10,"height":10,"mode":"shape"}),
        );
        assert!(!drawn.is_error, "{}", drawn.content[0]["text"]);
    }
    call(
        &mut e,
        "update_storyboard_panel",
        json!({"panel":1,"size":"wide"}),
    );
    let added = call(&mut e, "smart_add_storyboard_panel", json!({"after":1}));
    let id = added["panel"].as_u64().unwrap();
    assert_eq!(added["carried_layers"], json!(["Set"]));
    assert_eq!(e.active_page(), id);
    assert!(!e.doc.nodes.iter().any(|n| n.name == "Hero"));
    let outline = call(&mut e, "describe_storyboard", json!({}));
    assert_eq!(outline["smart_add_layers"], json!(["Set"]));
    assert_eq!(outline["naming"]["panel_prefix"], "P");
    let panel = &outline["acts"][0]["sequences"][0]["scenes"][0]["panels"][1];
    assert_eq!(panel["size"], "wide");
    assert_eq!(panel["name"], "P02");
    assert!(e.undo());
    assert_eq!(layout(&e), [1]);
    // An empty list clears it.
    call(
        &mut e,
        "set_storyboard_settings",
        json!({"smart_add_layers":[]}),
    );
    assert!(e.storyboard().unwrap().smart_add_layers.is_empty());
}

#[test]
fn panels_move_across_scenes_and_groups_join_and_renumber() {
    let mut e = board();
    let street = two_scenes(&mut e);
    let ids = layout(&e);
    let first = scene_of(&e, ids[0]);
    // Into the next scene by name, at its start.
    call(
        &mut e,
        "move_storyboard_panels",
        json!({"panels":[ids[1]],"after":ids[0],"scene_name":"street"}),
    );
    assert_eq!(scene_of(&e, ids[1]), street);
    // Only a scene: to its end.
    call(
        &mut e,
        "move_storyboard_panels",
        json!({"panels":[ids[0]],"scene":street}),
    );
    assert_eq!(layout(&e), [ids[1], ids[2], ids[3], ids[0]]);
    assert!(!e.storyboard().unwrap().scenes.contains_key(&first));
    assert!(e.undo());
    assert!(e.undo());
    assert_eq!(layout(&e), ids[..]);
    assert_eq!(scene_of(&e, ids[1]), first);
    // Join the second scene into the first.
    let joined = call(&mut e, "join_storyboard_group", json!({"group":street}));
    assert_eq!(joined["group"], first);
    assert!(
        e.storyboard()
            .unwrap()
            .panels
            .values()
            .all(|p| p.scene == first)
    );
    assert!(e.undo());
    assert_eq!(scene_of(&e, ids[3]), street);
    // Renumber the second scene only, then everything.
    call(
        &mut e,
        "set_storyboard_settings",
        json!({"naming":{"scene_prefix":"SC","scene_start":10,"scene_step":10,"scene_digits":3}}),
    );
    let renamed = call(
        &mut e,
        "renumber_storyboard",
        json!({"groups":[street],"panels":false}),
    );
    assert_eq!(renamed["renamed"], 1);
    let board = e.storyboard().unwrap();
    assert_eq!(board.scenes[&street].name, "SC020");
    assert_ne!(board.scenes[&first].name, "SC010");
    call(&mut e, "renumber_storyboard", json!({}));
    let names: Vec<_> = e.page_list().iter().map(|m| m.name.clone()).collect();
    assert_eq!(names, ["Panel 1", "Panel 2", "Panel 1", "Panel 2"]);
    assert_eq!(e.storyboard().unwrap().scenes[&first].name, "SC010");
    assert!(e.undo());
    assert_eq!(e.storyboard().unwrap().scenes[&street].name, "SC020");
}

#[test]
fn thumbnail_sheets_report_cells_and_convert_to_panels() {
    let mut e = board();
    let sheet = call(
        &mut e,
        "set_storyboard_thumbnail_sheet",
        json!({"panel":1,"columns":2,"rows":2,"gap":2,"margin":2}),
    );
    let cells = sheet["thumbnail_sheet"]["cells"].as_array().unwrap();
    assert_eq!(cells.len(), 4);
    assert_eq!(cells[0]["height"], 15);
    let outline = call(&mut e, "describe_storyboard", json!({}));
    let panel = &outline["acts"][0]["sequences"][0]["scenes"][0]["panels"][0];
    assert_eq!(panel["thumbnail_sheet"]["cells"], json!(cells));
    assert_eq!(outline["total_frames"], 0);
    // Draw a thumbnail in the first cell, then convert.
    let cell = &cells[0];
    let drawn = crate::exec::execute(
        &mut e,
        "draw_shape",
        &json!({"shape":"ellipse","name":"Mia","x":cell["x"],"y":cell["y"],"width":cell["width"],"height":cell["height"],"mode":"shape"}),
    );
    assert!(!drawn.is_error, "{}", drawn.content[0]["text"]);
    let converted = call(&mut e, "convert_storyboard_thumbnails", json!({"panel":1}));
    let ids: Vec<u64> = serde_json::from_value(converted["panels"].clone()).unwrap();
    assert_eq!(layout(&e), ids);
    let first = e.page(ids[0]).unwrap();
    assert_eq!((first.doc.width, first.doc.height), (64, 36));
    assert!(first.doc.nodes.iter().any(|n| n.name == "Mia"));
    e.snapshot().unwrap().validate().unwrap();
    assert!(e.undo());
    assert_eq!(layout(&e), [1]);
    call(
        &mut e,
        "set_storyboard_thumbnail_sheet",
        json!({"panel":1,"clear":true}),
    );
    assert!(e.storyboard().unwrap().panels[&1].thumbnails.is_none());
}

#[test]
fn caption_fields_are_added_updated_and_removed() {
    let mut e = board();
    let added = call(
        &mut e,
        "add_storyboard_caption_field",
        json!({"name":"Camera","multiline":false,"position":0}),
    );
    assert_eq!(added["caption_fields"][0]["name"], "Camera");
    call(
        &mut e,
        "update_storyboard_panel",
        json!({"panel":1,"captions":{"camera":"PAN LEFT"}}),
    );
    call(
        &mut e,
        "update_storyboard_caption_field",
        json!({"field":"camera","name":"Lens","print":false,"position":2}),
    );
    let outline = call(&mut e, "describe_storyboard", json!({}));
    let field = &outline["caption_fields"][2];
    assert_eq!(
        (&field["name"], &field["print"], &field["multiline"]),
        (&json!("Lens"), &json!(false), &json!(false))
    );
    assert_eq!(
        outline["acts"][0]["sequences"][0]["scenes"][0]["panels"][0]["captions"]["Lens"],
        "PAN LEFT"
    );
    call(
        &mut e,
        "remove_storyboard_caption_field",
        json!({"field":"Lens"}),
    );
    assert!(e.storyboard().unwrap().caption("Lens").is_none());
    assert!(e.storyboard().unwrap().panels[&1].captions.is_empty());
    // One step restores the field and its text.
    assert!(e.undo());
    let board = e.storyboard().unwrap();
    assert_eq!(
        board.panels[&1].captions[&board.caption("Lens").unwrap()].text,
        "PAN LEFT"
    );
}

#[test]
fn captions_format_by_character_offsets_or_matching_text() {
    let mut e = board();
    call(
        &mut e,
        "update_storyboard_panel",
        json!({"panel":1,"captions":{"Action":"Café: Mia waves at Mia"}}),
    );
    let formatted = call(
        &mut e,
        "format_storyboard_caption",
        json!({"panel":1,"field":"action","match":"Mia","bold":true,"color":"#cc0000"}),
    );
    assert_eq!(
        formatted["ranges"],
        json!([{"start":6,"end":9},{"start":19,"end":22}])
    );
    // Offsets count characters: "é" is one.
    call(
        &mut e,
        "format_storyboard_caption",
        json!({"panel":1,"field":"Action","start":0,"end":4,"italic":true,"size":60}),
    );
    let outline = call(&mut e, "describe_storyboard", json!({}));
    let panel = &outline["acts"][0]["sequences"][0]["scenes"][0]["panels"][0];
    assert_eq!(panel["captions"]["Action"], "Café: Mia waves at Mia");
    let runs = panel["formatting"]["Action"].as_array().unwrap();
    assert_eq!((&runs[0]["start"], &runs[0]["end"]), (&json!(0), &json!(4)));
    assert_eq!(runs[0]["style"]["italic"], true);
    assert_eq!(runs[1]["start"], 6);
    assert_eq!(runs[1]["style"]["bold"], true);
    assert_eq!(runs[1]["style"]["color"], "#cc0000");
    // Plain-string updates with the same text keep the formatting.
    call(
        &mut e,
        "update_storyboard_panel",
        json!({"panel":1,"captions":{"Action":"Café: Mia waves at Mia"},"tag":1}),
    );
    let board = e.storyboard().unwrap();
    let action = board.caption("Action").unwrap();
    assert!(!board.panels[&1].captions[&action].runs.is_empty());
    assert!(e.undo());
    assert!(e.undo());
    let board = e.storyboard().unwrap();
    let caption = &board.panels[&1].captions[&action];
    // Byte 7: "é" takes two bytes.
    assert!(caption.style_at(7).bold && !caption.style_at(0).italic);
}

#[test]
fn find_and_replace_rename_characters_and_skip_locked_panels() {
    let mut e = board();
    call(
        &mut e,
        "add_storyboard_panels",
        json!({"panels":[{"captions":{"Action":"Mia runs.","Dialogue":"MIA: Wait!"}},{"captions":{"Action":"Mia stops."}}]}),
    );
    let ids = layout(&e);
    call(
        &mut e,
        "set_storyboard_locks",
        json!({"panels":[ids[2]],"locked":true}),
    );
    let found = call(
        &mut e,
        "find_in_storyboard_captions",
        json!({"query":"mia","whole_word":true}),
    );
    assert_eq!(found["count"], 3);
    assert_eq!(found["matches"][1]["field"], "Dialogue");
    assert_eq!(found["matches"][2]["locked"], true);
    let only_action = call(
        &mut e,
        "find_in_storyboard_captions",
        json!({"query":"Mia","field":"Action","match_case":true}),
    );
    assert_eq!(only_action["count"], 2);
    let stamp = e.stamp();
    let replaced = call(
        &mut e,
        "replace_in_storyboard_captions",
        json!({"query":"mia","replacement":"Tom"}),
    );
    assert_eq!(replaced, json!({"replaced":2,"locked_panels_skipped":1}));
    let after = call(
        &mut e,
        "find_in_storyboard_captions",
        json!({"query":"Tom"}),
    );
    assert_eq!(after["count"], 2);
    assert!(e.undo());
    assert_eq!(e.stamp(), stamp);
}

#[test]
fn panels_and_scenes_copy_within_the_project_and_import_from_files() {
    let mut e = board();
    let street = two_scenes(&mut e);
    let ids = layout(&e);
    call(
        &mut e,
        "update_storyboard_panel",
        json!({"panel":ids[2],"captions":{"Action":"Bus passes"}}),
    );
    // A whole scene comes back as a new scene after it.
    let copied = call(&mut e, "copy_storyboard_panels", json!({"scenes":[street]}));
    let new: Vec<u64> = serde_json::from_value(copied["panels"].clone()).unwrap();
    assert_eq!(layout(&e)[4..], new[..]);
    let board = e.storyboard().unwrap();
    assert_ne!(board.panels[&new[0]].scene, street);
    assert_eq!(board.scenes[&board.panels[&new[0]].scene].name, "Street");
    assert!(e.undo());
    assert_eq!(layout(&e), ids);
    // A whole scene can also join the scene it lands in.
    let joined = call(
        &mut e,
        "copy_storyboard_panels",
        json!({"scenes":[street],"at_start":true,"new_scenes":false}),
    );
    assert_eq!(
        scene_of(&e, joined["panels"][0].as_u64().unwrap()),
        scene_of(&e, ids[0])
    );
    assert!(e.undo());

    // Another saved storyboard, at another resolution.
    let path = std::env::temp_dir().join(format!("sb-import-{}.emu", std::process::id()));
    emulsion_io::project::write(&e.snapshot().unwrap(), &path).unwrap();
    let mut target = ProjectEditor::new_project(
        emulsion_core::project::ProjectKind::Storyboard,
        emulsion_core::Document::new(128, 72),
    )
    .unwrap();
    let path_text = path.to_string_lossy();
    refused(
        &mut target,
        "import_storyboard_panels",
        json!({"path":path_text,"scenes":["Nowhere"]}),
    );
    let imported = call(
        &mut target,
        "import_storyboard_panels",
        json!({"path":path_text,"scenes":["street"]}),
    );
    std::fs::remove_file(&path).ok();
    assert_eq!(imported["scenes"], json!(["Street"]));
    let new: Vec<u64> = serde_json::from_value(imported["panels"].clone()).unwrap();
    assert_eq!(new.len(), 2);
    let doc = &target.page(new[0]).unwrap().doc;
    assert_eq!((doc.width, doc.height), (128, 72));
    let board = target.storyboard().unwrap();
    let action = board.caption("Action").unwrap();
    assert_eq!(board.panels[&new[0]].captions[&action].text, "Bus passes");
    assert!(target.undo());
    assert_eq!(layout(&target), [1]);
}

#[test]
fn invalid_phase_two_calls_change_nothing() {
    let mut e = board();
    let street = two_scenes(&mut e);
    call(
        &mut e,
        "update_storyboard_panel",
        json!({"panel":1,"captions":{"Action":"Mia runs","Slugging":"MIA'S FLAT"}}),
    );
    let first = scene_of(&e, 1);
    for (name, args) in [
        ("set_storyboard_locks", json!({"locked":true})),
        ("set_storyboard_locks", json!({"panels":[99],"locked":true})),
        ("set_storyboard_locks", json!({"scenes":[99],"locked":true})),
        (
            "set_storyboard_settings",
            json!({"naming":{"scene_step":0}}),
        ),
        ("set_storyboard_settings", json!({"naming":{"bogus":1}})),
        ("set_storyboard_settings", json!({"smart_add_layers":[" "]})),
        ("smart_add_storyboard_panel", json!({"after":99})),
        ("move_storyboard_panels", json!({"panels":[1]})),
        ("move_storyboard_panels", json!({"panels":[1],"after":1})),
        (
            "move_storyboard_panels",
            json!({"panels":[1],"scene_name":"Nowhere"}),
        ),
        (
            "move_storyboard_panels",
            json!({"panels":[1],"after":4,"scene":first}),
        ),
        (
            "move_storyboard_panels",
            json!({"panels":[1],"scene":street,"scene_name":"Street"}),
        ),
        ("join_storyboard_group", json!({"group":99})),
        ("join_storyboard_group", json!({"group":first})),
        (
            "renumber_storyboard",
            json!({"scenes":false,"panels":false}),
        ),
        ("renumber_storyboard", json!({"groups":[99]})),
        ("set_storyboard_thumbnail_sheet", json!({"panel":1})),
        (
            "set_storyboard_thumbnail_sheet",
            json!({"panel":1,"columns":9,"rows":1}),
        ),
        (
            "set_storyboard_thumbnail_sheet",
            json!({"panel":1,"columns":8,"rows":8}),
        ),
        (
            "set_storyboard_thumbnail_sheet",
            json!({"panel":1,"clear":true,"rows":2}),
        ),
        ("convert_storyboard_thumbnails", json!({"panel":1})),
        ("copy_storyboard_panels", json!({})),
        ("copy_storyboard_panels", json!({"panels":[99]})),
        ("import_storyboard_panels", json!({"path":"relative.emu"})),
        (
            "import_storyboard_panels",
            json!({"path":"/nonexistent/board.emu"}),
        ),
        ("add_storyboard_caption_field", json!({"name":"action"})),
        ("update_storyboard_caption_field", json!({"field":"Action"})),
        (
            "update_storyboard_caption_field",
            json!({"field":"Action","name":"Notes"}),
        ),
        (
            "update_storyboard_caption_field",
            json!({"field":"Mood","print":false}),
        ),
        ("remove_storyboard_caption_field", json!({"field":"Mood"})),
        (
            "format_storyboard_caption",
            json!({"panel":1,"field":"Action"}),
        ),
        (
            "format_storyboard_caption",
            json!({"panel":1,"field":"Action","bold":true,"start":2}),
        ),
        (
            "format_storyboard_caption",
            json!({"panel":1,"field":"Action","bold":true,"start":0,"end":99}),
        ),
        (
            "format_storyboard_caption",
            json!({"panel":1,"field":"Action","bold":true,"match":"Tom"}),
        ),
        (
            "format_storyboard_caption",
            json!({"panel":1,"field":"Notes","bold":true}),
        ),
        (
            "format_storyboard_caption",
            json!({"panel":1,"field":"Action","color":"red"}),
        ),
        ("find_in_storyboard_captions", json!({"query":""})),
        ("replace_in_storyboard_captions", json!({"query":"Mia"})),
        (
            "replace_in_storyboard_captions",
            json!({"query":"Mia","replacement":"x","field":"Mood"}),
        ),
        (
            "replace_in_storyboard_captions",
            json!({"query":"Mia","replacement":"a\nb","field":"Slugging"}),
        ),
    ] {
        refused(&mut e, name, args);
    }
    // A locked panel refuses formatting and blocks removing a field it uses.
    call(
        &mut e,
        "set_storyboard_locks",
        json!({"panels":[1],"locked":true}),
    );
    refused(
        &mut e,
        "format_storyboard_caption",
        json!({"panel":1,"field":"Action","bold":true}),
    );
    refused(
        &mut e,
        "remove_storyboard_caption_field",
        json!({"field":"Action"}),
    );
}
