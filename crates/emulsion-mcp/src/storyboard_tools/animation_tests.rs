use super::execute;
use super::tests::{board, call};
use emulsion_core::command::Slot;
use emulsion_core::project::ProjectEditor;
use emulsion_core::{Command, Node, NodeKind};
use serde_json::{Value, json};

fn refused(e: &mut ProjectEditor, name: &str, args: Value) -> String {
    let stamp = e.stamp();
    let result = execute(e, name, &args);
    assert!(result.is_error, "{name} {args} should fail");
    assert_eq!(e.stamp(), stamp, "{name} {args} changed the project");
    result.content[0]["text"].as_str().unwrap().to_string()
}

/// Three panels of 24 frames in one scene; returns the panel IDs and the
/// scene ID.
fn three(e: &mut ProjectEditor) -> (Vec<u64>, u64) {
    call(e, "set_storyboard_settings", json!({"panel_frames":24}));
    call(e, "add_storyboard_panels", json!({"panels":[{},{}]}));
    call(e, "update_storyboard_panel", json!({"panel":1,"frames":24}));
    let ids = super::layout(e);
    let scene = e.storyboard().unwrap().panels[&ids[0]].scene;
    (ids, scene)
}

/// A "Hero" layer on `panel`; returns its ID.
fn hero(e: &mut ProjectEditor, panel: u64) -> u64 {
    e.set_active_page(panel).unwrap();
    e.execute(Command::AddNode {
        node: Box::new(Node::new(0, "Hero", NodeKind::Fill { rgba: [200; 4] })),
        slot: Slot::TOP,
    })
    .unwrap()
    .unwrap()
}

#[test]
fn camera_moves_across_a_scene_with_eases_shake_and_undo() {
    let mut e = board();
    let (ids, scene) = three(&mut e);
    // A truck across the scene: rest at the start, right and closer by the
    // third panel, easing in and out.
    let camera = call(
        &mut e,
        "set_storyboard_camera_keys",
        json!({"scene":scene,"keys":[
            {"frame":0,"easing":"ease_in_out"},
            {"panel":ids[2],"frame":0,"x":48,"zoom":1.5,"curve":{"x1":0.2,"y1":0,"x2":0.2,"y2":1}}
        ]}),
    );
    let keys = camera["keys"].as_array().unwrap();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[1]["frame"], 48);
    assert_eq!(keys[1]["panel"], ids[2]);
    assert_eq!(keys[0]["x"], 32.0, "values left out keep the rest framing");
    assert_eq!(
        keys[1]["curve"],
        json!({"x1":0.2,"y1":0.0,"x2":0.2,"y2":1.0})
    );
    assert_eq!(camera["frames"], 72);
    // Seconds and timecode land on the same frames; a changed key keeps
    // what was not given.
    let camera = call(
        &mut e,
        "set_storyboard_camera_keys",
        json!({"scene":scene,"keys":[{"seconds":2,"rotation":5},{"timecode":"00:00:01:00","y":10}]}),
    );
    let keys = camera["keys"].as_array().unwrap();
    assert_eq!(keys.len(), 3);
    assert_eq!(
        (keys[1]["frame"].clone(), keys[1]["y"].clone()),
        (json!(24), json!(10.0))
    );
    assert_eq!(keys[2]["x"], 48.0);
    assert_eq!(keys[2]["rotation"], 5.0);
    // One Undo step per call.
    assert!(e.undo());
    let board = e.storyboard().unwrap();
    assert_eq!(board.cameras[&scene].keys.len(), 2);
    assert_eq!(board.cameras[&scene].keys[1].rotation, 0.);

    // Shake for an impact: a preset, adjusted, then removed.
    let camera = call(
        &mut e,
        "set_storyboard_camera_shake",
        json!({"scene":scene,"preset":"earthquake","amplitude":5}),
    );
    assert_eq!(camera["shake"]["amplitude"], 5.0);
    assert_eq!(camera["shake"]["frequency"], 9.0);
    let camera = call(
        &mut e,
        "set_storyboard_camera_shake",
        json!({"scene":scene,"remove":true}),
    );
    assert!(camera["shake"].is_null());

    let camera = call(
        &mut e,
        "delete_storyboard_camera_keys",
        json!({"scene":scene,"keys":[{"panel":ids[2],"frame":0}]}),
    );
    assert_eq!(camera["keys"].as_array().unwrap().len(), 1);
    let read = call(&mut e, "describe_storyboard_camera", json!({"scene":scene}));
    assert_eq!(read, camera);
    call(&mut e, "reset_storyboard_camera", json!({"scene":scene}));
    assert!(e.storyboard().unwrap().cameras.is_empty());
    assert!(e.undo());
    assert_eq!(e.storyboard().unwrap().cameras[&scene].keys.len(), 1);
}

#[test]
fn a_static_camera_holds_one_panel_and_cameras_copy_between_scenes() {
    let mut e = board();
    let (ids, scene) = three(&mut e);
    call(
        &mut e,
        "set_storyboard_camera_keys",
        json!({"scene":scene,"keys":[{"frame":0,"easing":"linear"},{"frame":72,"x":64}]}),
    );
    let before = e.storyboard().unwrap().clone();
    let layout = super::layout(&e);
    call(
        &mut e,
        "set_storyboard_static_camera",
        json!({"panel":ids[1],"zoom":2}),
    );
    let board = e.storyboard().unwrap();
    // The first panel keeps its pan; the second holds; the third resumes.
    for f in [0., 10., 23.] {
        assert_eq!(board.camera_at(&layout, f), before.camera_at(&layout, f));
    }
    let held = board.camera_at(&layout, 24.);
    assert_eq!(held.zoom, 2.);
    assert_eq!(board.camera_at(&layout, 47.), held);
    assert_eq!(
        board.camera_at(&layout, 48.),
        before.camera_at(&layout, 48.)
    );

    // A second scene takes a copy, stretched to its length.
    let group = call(
        &mut e,
        "start_storyboard_group",
        json!({"panel":ids[2],"level":"scene"}),
    )["group"]
        .as_u64()
        .unwrap();
    let camera = call(
        &mut e,
        "copy_storyboard_camera",
        json!({"from":scene,"to":group}),
    );
    let source = &e.storyboard().unwrap().cameras[&scene];
    let copied = &e.storyboard().unwrap().cameras[&group];
    // The first scene now plays 48 frames, the second 24: keys halve, and
    // keys that land on one frame merge.
    assert!(!copied.keys.is_empty() && copied.keys.len() <= source.keys.len());
    let last = camera["keys"].as_array().unwrap().last().unwrap()["frame"].clone();
    assert_eq!(last, json!(source.keys.last().unwrap().frame / 2));
    assert!(e.undo());
    assert!(!e.storyboard().unwrap().cameras.contains_key(&group));
}

#[test]
fn layer_keys_slide_and_fade_a_character_within_a_panel() {
    let mut e = board();
    let (ids, _) = three(&mut e);
    let hero = hero(&mut e, ids[0]);
    let motion = call(
        &mut e,
        "set_storyboard_layer_keys",
        json!({"panel":ids[0],"layer":hero,"tracks":[
            {"property":"x","keys":[{"frame":0,"value":-30,"easing":"ease_out"},{"seconds":0.5,"value":0}]},
            {"property":"opacity","keys":[{"frame":0,"value":0},{"frame":6,"value":1}]}
        ]}),
    );
    let layer = &motion["layers"][0];
    assert_eq!(layer["layer"], hero);
    assert_eq!(layer["name"], "Hero");
    assert_eq!(layer["tracks"][0]["keys"][1]["frame"], 12);
    let board = e.storyboard().unwrap();
    let doc = &e.page(ids[0]).unwrap().doc;
    let moved = board.animate_panel(ids[0], doc, 0.).unwrap();
    assert_eq!(moved.node(hero).unwrap().opacity, 0.);
    // One Undo step for the whole call.
    assert!(e.undo());
    assert!(e.storyboard().unwrap().panels[&ids[0]].motion.is_empty());
    assert!(e.redo());

    call(
        &mut e,
        "set_storyboard_layer_pivot",
        json!({"panel":ids[0],"layer":hero,"x":10,"y":20}),
    );
    let motion = call(
        &mut e,
        "delete_storyboard_layer_keys",
        json!({"panel":ids[0],"layer":hero,"property":"x","keys":[{"frame":0}]}),
    );
    assert_eq!(motion["layers"][0]["pivot"], json!([10.0, 20.0]));
    assert_eq!(
        motion["layers"][0]["tracks"][0]["keys"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    call(
        &mut e,
        "delete_storyboard_layer_keys",
        json!({"panel":ids[0],"layer":hero,"property":"opacity"}),
    );
    call(
        &mut e,
        "delete_storyboard_layer_keys",
        json!({"panel":ids[0],"layer":hero}),
    );
    let motion = call(
        &mut e,
        "set_storyboard_layer_pivot",
        json!({"panel":ids[0],"layer":hero,"clear":true}),
    );
    assert!(motion["layers"].as_array().unwrap().is_empty());

    // Effect parameters animate on adjustment layers, within their range.
    e.set_active_page(ids[0]).unwrap();
    let added = crate::exec::execute(&mut e, "add_adjustment", &json!({"kind":"white_balance"}));
    assert!(!added.is_error, "{}", added.content[0]["text"]);
    let adjust = e.doc.nodes.last().unwrap().id;
    let read = call(
        &mut e,
        "describe_storyboard_layer_motion",
        json!({"panel":ids[0]}),
    );
    let effect = &read["effects"][0];
    assert_eq!(effect["layer"], adjust);
    let key = effect["params"][0]["key"].as_str().unwrap().to_string();
    let max = effect["params"][0]["max"].as_f64().unwrap();
    call(
        &mut e,
        "set_storyboard_layer_keys",
        json!({"panel":ids[0],"layer":adjust,"tracks":[{"property":"effect","effect":key,"keys":[{"frame":0,"value":0},{"frame":10,"value":max}]}]}),
    );
    refused(
        &mut e,
        "set_storyboard_layer_keys",
        json!({"panel":ids[0],"layer":adjust,"tracks":[{"property":"effect","effect":key,"keys":[{"frame":0,"value":max + 1.}]}]}),
    );
    refused(
        &mut e,
        "set_storyboard_layer_keys",
        json!({"panel":ids[0],"layer":hero,"tracks":[{"property":"effect","effect":"warmth","keys":[{"frame":0,"value":1}]}]}),
    );
}

#[test]
fn layer_comps_list_capture_apply_rename_and_delete() {
    let mut e = board();
    let (ids, _) = three(&mut e);
    let hero = hero(&mut e, ids[0]);
    e.execute(Command::SetVisible {
        id: hero,
        visible: false,
    })
    .unwrap();
    call(
        &mut e,
        "capture_storyboard_layer_comp",
        json!({"panel":ids[0],"name":"Empty room"}),
    );
    e.execute(Command::SetVisible {
        id: hero,
        visible: true,
    })
    .unwrap();
    let listed = call(
        &mut e,
        "list_storyboard_layer_comps",
        json!({"panel":ids[0]}),
    );
    assert_eq!(listed["comps"][0]["hidden_names"], json!(["Hero"]));
    call(
        &mut e,
        "apply_storyboard_layer_comp",
        json!({"panel":ids[0],"name":"Empty room"}),
    );
    assert!(!e.page(ids[0]).unwrap().doc.node(hero).unwrap().visible);
    assert!(e.undo());
    assert!(e.page(ids[0]).unwrap().doc.node(hero).unwrap().visible);
    call(
        &mut e,
        "rename_storyboard_layer_comp",
        json!({"panel":ids[0],"name":"Empty room","new_name":"Before"}),
    );
    let listed = call(
        &mut e,
        "delete_storyboard_layer_comp",
        json!({"panel":ids[0],"name":"Before"}),
    );
    assert!(listed["comps"].as_array().unwrap().is_empty());
    assert!(e.undo());
    assert_eq!(
        e.storyboard().unwrap().panels[&ids[0]].comps[0].name,
        "Before"
    );
    refused(
        &mut e,
        "apply_storyboard_layer_comp",
        json!({"panel":ids[0],"name":"Night"}),
    );
}

#[test]
fn describe_storyboard_counts_what_is_animated_and_sync_switches() {
    let mut e = board();
    let (ids, scene) = three(&mut e);
    let hero = hero(&mut e, ids[1]);
    call(
        &mut e,
        "set_storyboard_layer_keys",
        json!({"panel":ids[1],"layer":hero,"tracks":[{"property":"y","keys":[{"frame":0,"value":5}]}]}),
    );
    call(
        &mut e,
        "set_storyboard_camera_shake",
        json!({"scene":scene,"preset":"handheld"}),
    );
    call(
        &mut e,
        "set_storyboard_keyframe_sync",
        json!({"mode":"keep"}),
    );
    let outline = call(&mut e, "describe_storyboard", json!({}));
    assert_eq!(
        outline["animation"],
        json!({"keyframe_sync":"keep","scenes_with_camera":1,"animated_panels":1,"panels_with_comps":0})
    );
    let scene_json = &outline["acts"][0]["sequences"][0]["scenes"][0];
    assert_eq!(scene_json["camera"], json!({"keys":0,"shake":true}));
    assert_eq!(scene_json["panels"][1]["animated_layers"], 1);
    assert!(scene_json["panels"][0].get("animated_layers").is_none());
    // Keep: a longer panel leaves its keys where they were.
    call(
        &mut e,
        "set_storyboard_layer_keys",
        json!({"panel":ids[1],"layer":hero,"tracks":[{"property":"y","keys":[{"frame":20,"value":0}]}]}),
    );
    call(
        &mut e,
        "update_storyboard_panel",
        json!({"panel":ids[1],"frames":48}),
    );
    let keys = &e.storyboard().unwrap().panels[&ids[1]].motion[&hero].tracks[0].keys;
    assert_eq!(keys[1].frame, 20);
}

#[test]
fn invalid_animation_calls_and_locked_panels_change_nothing() {
    let mut e = board();
    let (ids, scene) = three(&mut e);
    let hero = hero(&mut e, ids[0]);
    call(
        &mut e,
        "capture_storyboard_layer_comp",
        json!({"panel":ids[0],"name":"All"}),
    );
    for (name, args) in [
        (
            "set_storyboard_camera_keys",
            json!({"scene":999,"keys":[{"frame":0}]}),
        ),
        (
            "set_storyboard_camera_keys",
            json!({"scene":scene,"keys":[{"x":4}]}),
        ),
        (
            "set_storyboard_camera_keys",
            json!({"scene":scene,"keys":[{"frame":0,"seconds":1}]}),
        ),
        (
            "set_storyboard_camera_keys",
            json!({"scene":scene,"keys":[{"frame":500}]}),
        ),
        (
            "set_storyboard_camera_keys",
            json!({"scene":scene,"keys":[{"frame":0,"zoom":30}]}),
        ),
        (
            "set_storyboard_camera_keys",
            json!({"scene":scene,"keys":[{"frame":0,"curve":{"x1":2,"y1":0,"x2":0,"y2":1}}]}),
        ),
        (
            "set_storyboard_camera_keys",
            json!({"scene":scene,"keys":[{"frame":3},{"frame":3}]}),
        ),
        (
            "set_storyboard_camera_keys",
            json!({"scene":scene,"keys":[{"timecode":"bad timecode"}]}),
        ),
        (
            "delete_storyboard_camera_keys",
            json!({"scene":scene,"keys":[{"frame":4}]}),
        ),
        (
            "set_storyboard_camera_shake",
            json!({"scene":scene,"preset":"wobble"}),
        ),
        (
            "set_storyboard_camera_shake",
            json!({"scene":scene,"remove":true,"amplitude":3}),
        ),
        ("copy_storyboard_camera", json!({"from":scene,"to":scene})),
        ("set_storyboard_static_camera", json!({"panel":99})),
        (
            "set_storyboard_layer_keys",
            json!({"panel":ids[0],"layer":999,"tracks":[{"property":"x","keys":[{"frame":0,"value":1}]}]}),
        ),
        (
            "set_storyboard_layer_keys",
            json!({"panel":ids[0],"layer":hero,"tracks":[{"property":"opacity","keys":[{"frame":0,"value":2}]}]}),
        ),
        (
            "set_storyboard_layer_keys",
            json!({"panel":ids[0],"layer":hero,"tracks":[{"property":"x","keys":[{"frame":99,"value":1}]}]}),
        ),
        (
            "set_storyboard_layer_keys",
            json!({"panel":ids[0],"layer":hero,"tracks":[{"property":"x","effect":"warmth","keys":[{"frame":0,"value":1}]}]}),
        ),
        (
            "delete_storyboard_layer_keys",
            json!({"panel":ids[0],"layer":hero}),
        ),
        (
            "set_storyboard_layer_pivot",
            json!({"panel":ids[0],"layer":hero,"x":3}),
        ),
        (
            "rename_storyboard_layer_comp",
            json!({"panel":ids[0],"name":"None","new_name":"x"}),
        ),
        ("describe_storyboard_camera", json!({"scene":999})),
        ("describe_storyboard_layer_motion", json!({"panel":999})),
        ("set_storyboard_keyframe_sync", json!({"mode":"stretch"})),
    ] {
        refused(&mut e, name, args);
    }
    // Locked panels refuse layer keys, pivots and comps; locked scenes their
    // camera.
    call(
        &mut e,
        "set_storyboard_layer_keys",
        json!({"panel":ids[0],"layer":hero,"tracks":[{"property":"x","keys":[{"frame":0,"value":1}]}]}),
    );
    call(
        &mut e,
        "set_storyboard_locks",
        json!({"panels":[ids[0]],"locked":true}),
    );
    let text = refused(
        &mut e,
        "set_storyboard_layer_keys",
        json!({"panel":ids[0],"layer":hero,"tracks":[{"property":"x","keys":[{"frame":0,"value":9}]}]}),
    );
    assert!(text.contains("locked"), "{text}");
    for (name, args) in [
        (
            "delete_storyboard_layer_keys",
            json!({"panel":ids[0],"layer":hero}),
        ),
        (
            "set_storyboard_layer_pivot",
            json!({"panel":ids[0],"layer":hero,"clear":true}),
        ),
        (
            "capture_storyboard_layer_comp",
            json!({"panel":ids[0],"name":"Other"}),
        ),
        (
            "delete_storyboard_layer_comp",
            json!({"panel":ids[0],"name":"All"}),
        ),
    ] {
        refused(&mut e, name, args);
    }
    call(
        &mut e,
        "set_storyboard_locks",
        json!({"scenes":[scene],"locked":true}),
    );
    refused(
        &mut e,
        "set_storyboard_camera_keys",
        json!({"scene":scene,"keys":[{"frame":0,"x":1}]}),
    );
    refused(&mut e, "reset_storyboard_camera", json!({"scene":scene}));
}
