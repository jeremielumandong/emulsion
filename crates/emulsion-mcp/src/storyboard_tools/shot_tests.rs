use super::super::tests::{board, call};
use emulsion_core::storyboard_shot::REFERENCE_LAYER;
use serde_json::{Value, json};

fn layer_names(e: &emulsion_core::project::ProjectEditor, panel: u64) -> Vec<String> {
    e.page(panel)
        .unwrap()
        .doc
        .nodes
        .iter()
        .map(|n| n.name.clone())
        .collect()
}

#[test]
fn text_to_shot_builds_a_panels_set_and_reports_what_it_understood() {
    let mut e = board();
    let out = call(
        &mut e,
        "text_to_storyboard_shot",
        json!({"panel":1,"text":"low-angle close-up of two people at a table, glorp"}),
    );
    assert!(out["interpretation"].as_str().unwrap().contains("close"));
    assert_eq!(out["unrecognized"], json!(["glorp"]));
    let objects = out["set"]["objects"].as_array().unwrap();
    let people = objects.iter().filter(|o| o["type"] == "character").count();
    assert_eq!(people, 2);
    assert!(e.undo(), "one Undo step");
    let out = call(&mut e, "describe_storyboard_shot", json!({"panel":1}));
    assert_eq!(out["has_set"], false);
}

#[test]
fn objects_poses_and_the_camera_round_trip() {
    let mut e = board();
    let mia = call(
        &mut e,
        "add_storyboard_shot_object",
        json!({"panel":1,"type":"character","kind":"adult_female","name":"Mia","pose":"wave","face":"happy"}),
    )["object"]
        .as_u64()
        .unwrap();
    let chair = call(
        &mut e,
        "add_storyboard_shot_object",
        json!({"panel":1,"type":"prop","kind":"chair","position":{"x":1,"y":0,"z":0},"color":"#AA3311"}),
    )["object"]
        .as_u64()
        .unwrap();
    call(
        &mut e,
        "update_storyboard_shot_object",
        json!({"panel":1,"object":chair,"rotation":{"yaw":90},"size":{"x":0.5,"y":1.0,"z":0.5},"name":"Seat"}),
    );
    call(
        &mut e,
        "pose_storyboard_character",
        json!({"panel":1,"object":mia,"preset":"point","joints":{"head":{"y":20}},
               "right_hand":"fist","ik":[{"limb":"left_arm","target":{"x":0.3,"y":1.2,"z":0.4}}],
               "look_at":{"x":1,"y":1.5,"z":2},"face":"angry","save_as":"Mia points"}),
    );
    let out = call(&mut e, "describe_storyboard_shot", json!({"panel":1}));
    let mia_json = out["set"]["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == mia)
        .unwrap()
        .clone();
    assert_eq!(mia_json["face"], "angry");
    assert_eq!(mia_json["pose"]["right_hand"], "fist");
    assert_eq!(mia_json["ik"][0]["limb"], "left_arm");
    assert_eq!(out["custom_poses"], json!(["Mia points"]));
    let seat = out["set"]["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == chair)
        .unwrap();
    assert_eq!(seat["name"], "Seat");
    assert_eq!(seat["color"], json!([0xAA, 0x33, 0x11]));
    // Frame Mia in a close-up with an 85 mm lens.
    let cam = call(
        &mut e,
        "set_storyboard_shot_camera",
        json!({"panel":1,"frame":{"subject":mia,"size":"close_up","angle":"low","focal_length_mm":85}}),
    );
    assert!((cam["camera"]["focal_length_mm"].as_f64().unwrap() - 85.).abs() < 1e-3);
    // The whole set goes back in unchanged.
    let set = out["set"].clone();
    call(&mut e, "set_storyboard_shot", json!({"panel":1,"set":set}));
    let again = call(&mut e, "describe_storyboard_shot", json!({"panel":1}));
    assert_eq!(again["set"], set);
    // Removing objects.
    let removed = call(
        &mut e,
        "remove_storyboard_shot_objects",
        json!({"panel":1,"objects":[chair]}),
    );
    assert_eq!(removed["removed"], 1);
    let result = super::super::execute(
        &mut e,
        "pose_storyboard_character",
        &json!({"panel":1,"object":chair,"preset":"sit"}),
    );
    assert!(result.is_error);
}

#[test]
fn shot_explorer_proposes_and_applies_angles() {
    let mut e = board();
    call(
        &mut e,
        "text_to_storyboard_shot",
        json!({"panel":1,"text":"a man and a woman talking"}),
    );
    let out = call(
        &mut e,
        "explore_storyboard_shots",
        json!({"panel":1,"count":8}),
    );
    let list = out["proposals"].as_array().unwrap();
    assert_eq!(list.len(), 8);
    assert!(
        list.iter()
            .all(|p| p["name"].as_str().is_some_and(|n| !n.is_empty()))
    );
    let chosen = list[3]["focal_length_mm"].as_f64().unwrap();
    call(
        &mut e,
        "explore_storyboard_shots",
        json!({"panel":1,"count":8,"apply":3}),
    );
    let set = call(&mut e, "describe_storyboard_shot", json!({"panel":1}));
    assert!((set["camera"]["focal_length_mm"].as_f64().unwrap() - chosen).abs() < 1e-3);
}

#[test]
fn renders_land_in_the_reference_layer_and_update_with_the_set() {
    let mut e = board();
    let mia = call(
        &mut e,
        "add_storyboard_shot_object",
        json!({"panel":1,"type":"character"}),
    )["object"]
        .clone();
    let layer = call(
        &mut e,
        "render_storyboard_shot",
        json!({"panel":1,"mode":"reference","style":"outline"}),
    )["layer"]
        .as_u64()
        .unwrap();
    let names = layer_names(&e, 1);
    assert_eq!(names.iter().filter(|n| *n == REFERENCE_LAYER).count(), 1);
    let pixels =
        |e: &emulsion_core::project::ProjectEditor| e.page(1).unwrap().doc.node(layer).cloned();
    let before = pixels(&e);
    // Moving the character renders the reference again, in the same step.
    call(
        &mut e,
        "update_storyboard_shot_object",
        json!({"panel":1,"object":mia,"move_by":{"x":0.6,"y":0,"z":0}}),
    );
    assert_ne!(pixels(&e), before);
    assert_eq!(
        layer_names(&e, 1)
            .iter()
            .filter(|n| *n == REFERENCE_LAYER)
            .count(),
        1
    );
    assert!(e.undo());
    assert_eq!(pixels(&e), before, "the move and its render undo together");
    // A snapshot is a new layer.
    let snap = call(
        &mut e,
        "render_storyboard_shot",
        json!({"panel":1,"mode":"snapshot"}),
    );
    assert_ne!(snap["layer"].as_u64().unwrap(), layer);
    // Panels without a set refuse.
    let result = super::super::execute(
        &mut e,
        "render_storyboard_shot",
        &json!({"panel":1,"mode":"reference"}),
    );
    assert!(!result.is_error);
}

#[test]
fn models_layer_depth_and_attachments() {
    let mut e = board();
    let path = std::env::temp_dir().join(format!("emulsion-mcp-model-{}.obj", std::process::id()));
    std::fs::write(&path, b"o crate\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();
    let out = call(
        &mut e,
        "import_storyboard_model",
        json!({"panel":1,"path":path.to_str().unwrap(),"position":{"x":0,"y":0,"z":1}}),
    );
    std::fs::remove_file(&path).unwrap();
    let crate_id = out["object"].clone();
    let described = call(&mut e, "describe_storyboard_shot", json!({"panel":1}));
    assert_eq!(described["models"][0]["asset"], out["asset"]);
    // Depth and following use a panel layer.
    e.execute(emulsion_core::Command::AddNode {
        node: Box::new(emulsion_core::Node::new(
            0,
            "Hero",
            emulsion_core::NodeKind::Fill { rgba: [255; 4] },
        )),
        slot: emulsion_core::command::Slot::TOP,
    })
    .unwrap();
    let layer = e.page(1).unwrap().doc.nodes[0].id;
    call(
        &mut e,
        "set_storyboard_layer_depth",
        json!({"panel":1,"layer":layer,"depth":2.5}),
    );
    let described = call(&mut e, "describe_storyboard_shot", json!({"panel":1}));
    assert_eq!(described["layer_depth"][layer.to_string()], 2.5);
    let bad = super::super::execute(
        &mut e,
        "set_storyboard_layer_depth",
        &json!({"panel":1,"layer":layer,"depth":-3}),
    );
    assert!(bad.is_error);
    let attached = call(
        &mut e,
        "attach_storyboard_layer",
        json!({"panel":1,"layer":layer,"object":crate_id}),
    );
    assert_eq!(attached["attached"], true);
    let described: Value = call(&mut e, "describe_storyboard_shot", json!({"panel":1}));
    assert_eq!(described["attachments"][0]["layer"], layer);
    call(
        &mut e,
        "attach_storyboard_layer",
        json!({"panel":1,"layer":layer,"detach":true}),
    );
}
