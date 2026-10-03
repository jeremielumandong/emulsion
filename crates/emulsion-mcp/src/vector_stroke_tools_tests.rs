use super::*;
use emulsion_core::Document;
use emulsion_core::project::{ProjectEditor, ProjectKind};

fn editor() -> Editor {
    Editor::new(Document::new(200, 120), None)
}

fn call(e: &mut Editor, name: &str, args: Value) -> Value {
    let result = crate::exec::execute(e, name, &args);
    assert!(!result.is_error, "{name}: {}", result.content[0]["text"]);
    serde_json::from_str(result.content[0]["text"].as_str().unwrap()).unwrap()
}

/// The call fails and leaves the document and its history as they were.
fn refused(e: &mut Editor, name: &str, args: Value) -> String {
    let (doc, undo) = (e.doc.clone(), e.history.can_undo());
    let result = crate::exec::execute(e, name, &args);
    assert!(result.is_error, "{name} {args} should fail");
    assert_eq!(e.doc, doc, "{name} {args} changed the document");
    assert_eq!(e.history.can_undo(), undo, "{name} {args} added history");
    result.content[0]["text"].as_str().unwrap().to_string()
}

fn set(e: &Editor, id: NodeId) -> StrokeSet {
    let NodeKind::Strokes { strokes, .. } = &e.doc.node(id).unwrap().kind else {
        panic!("not a stroke layer")
    };
    (**strokes).clone()
}

fn alpha(e: &Editor, id: NodeId, x: u32, y: u32) -> u16 {
    let NodeKind::Strokes { cache, .. } = &e.doc.node(id).unwrap().kind else {
        panic!("not a stroke layer")
    };
    cache.pixels().get(x, y)[3]
}

fn tapered() -> Value {
    json!({"points":[{"x":20,"y":60,"width":0.1},{"x":100,"y":60,"width":1},{"x":180,"y":60,"width":0.1}]})
}

/// An empty vector layer, with the snapshot to compare Undo against.
fn vector(e: &mut Editor) -> NodeId {
    call(e, "add_vector_layer", json!({"name":"Ink"}))["node"]
        .as_u64()
        .unwrap()
}

fn pixel_layer(e: &mut Editor) -> NodeId {
    assert!(!crate::exec::execute(e, "add_layer", &json!({})).is_error);
    *e.doc.children(None).last().unwrap()
}

/// Undo restores `before` in exactly one step.
fn undoes_to(e: &mut Editor, before: &Document) {
    assert!(e.undo());
    assert_eq!(&e.doc, before);
}

#[test]
fn tools_are_registered_and_describe_is_read_only() {
    let names: Vec<_> = crate::tools::definitions()
        .into_iter()
        .map(|d| d.name)
        .collect();
    for def in definitions() {
        assert!(names.contains(&def.name), "{} is not registered", def.name);
    }
    assert!(crate::tools::is_read_only("describe_vector_strokes"));
    assert!(!crate::tools::is_read_only("draw_vector_strokes"));
}

#[test]
fn add_vector_layer_creates_an_editable_layer_in_one_step() {
    let mut e = editor();
    let before = e.doc.clone();
    let out = call(
        &mut e,
        "add_vector_layer",
        json!({"name":"Pencil","line_width":8,"color":"#ff000080","strokes":[tapered()]}),
    );
    let id = out["node"].as_u64().unwrap();
    assert_eq!(out["stroke_count"], 1);
    let s = set(&e, id);
    assert_eq!(s.strokes[0].color, [255, 0, 0, 128]);
    assert_eq!(s.strokes[0].width, 8.);
    let doc = crate::exec::describe(&e);
    let layer = doc["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == id)
        .unwrap();
    assert_eq!(layer["kind"], "strokes");
    assert_eq!(layer["strokes"], 1);
    undoes_to(&mut e, &before);
    // Above an existing node.
    let lower = vector(&mut e);
    let upper = call(&mut e, "add_vector_layer", json!({"above":lower}))["node"]
        .as_u64()
        .unwrap();
    let order = e.doc.children(None);
    assert!(
        order.iter().position(|n| *n == upper).unwrap()
            > order.iter().position(|n| *n == lower).unwrap()
    );
    for bad in [
        json!({"color":"red"}),
        json!({"strokes":[{"points":[{"x":1,"y":1}],"line_width":0}]}),
        json!({"strokes":[{"points":[{"x":1,"y":1,"width":9}]}]}),
        json!({"strokes":[{"points":[{"x":1,"y":1},{"x":2,"y":2}],"closed":true}]}),
        json!({"strokes":[{"points":[]}]}),
        json!({"above":999}),
        json!({"surprise":true}),
    ] {
        refused(&mut e, "add_vector_layer", bad);
    }
}

#[test]
fn drawing_tapered_strokes_varies_width_and_undoes_in_one_step() {
    let mut e = editor();
    let id = vector(&mut e);
    let before = e.doc.clone();
    let out = call(
        &mut e,
        "draw_vector_strokes",
        json!({"node":id,"line_width":10,"strokes":[tapered(),
            {"points":[{"x":20,"y":100,"opacity":0.2},{"x":180,"y":100}],"color":"#0000ff","line_width":4}]}),
    );
    assert_eq!(out["added"], json!([0, 1]));
    let s = set(&e, id);
    assert_eq!(s.strokes[0].points[1].width, 1.);
    assert_eq!(s.strokes[1].color, [0, 0, 255, 255]);
    // Full width in the middle, thin at the ends; opacity fades in.
    assert!(alpha(&e, id, 100, 64) > 60000);
    assert_eq!(alpha(&e, id, 25, 64), 0);
    assert!(alpha(&e, id, 25, 100) < alpha(&e, id, 170, 100));
    undoes_to(&mut e, &before);
    for bad in [
        json!({"node":id,"strokes":[]}),
        json!({"node":id,"strokes":[{"points":[{"x":"a","y":1}]}]}),
        json!({"node":id,"strokes":[{"points":[{"x":1,"y":1,"opacity":2}]}]}),
        json!({"node":id,"strokes":[{"points":[{"x":1,"y":1}],"colour":"#000000"}]}),
        json!({"node":id,"color":"#12345","strokes":[tapered()]}),
        json!({"node":999,"strokes":[tapered()]}),
    ] {
        refused(&mut e, "draw_vector_strokes", bad);
    }
    // Not a vector layer, or locked.
    let raster = pixel_layer(&mut e);
    assert!(
        refused(
            &mut e,
            "draw_vector_strokes",
            json!({"node":raster,"strokes":[tapered()]})
        )
        .contains("not a vector stroke layer")
    );
    e.execute(Command::SetLocked { id, locked: true }).unwrap();
    assert!(
        refused(
            &mut e,
            "draw_vector_strokes",
            json!({"node":id,"strokes":[tapered()]})
        )
        .contains("locked")
    );
}

#[test]
fn shapes_become_strokes() {
    let mut e = editor();
    let id = vector(&mut e);
    let before = e.doc.clone();
    let out = call(
        &mut e,
        "draw_vector_shapes",
        json!({"node":id,"line_width":3,"shapes":[
            {"shape":"line","points":[{"x":10,"y":10,"width":0.2},{"x":60,"y":10}]},
            {"shape":"rectangle","x":10,"y":20,"width":50,"height":30,"color":"#ff0000"},
            {"shape":"ellipse","x":80,"y":20,"width":60,"height":40,"line_width":6},
            {"shape":"polyline","points":[{"x":150,"y":20},{"x":190,"y":20},{"x":170,"y":60}],"closed":true}
        ]}),
    );
    assert_eq!(out["added"], json!([0, 1, 2, 3]));
    let s = set(&e, id);
    assert_eq!(s.strokes[0].points.len(), 2);
    assert!(!s.strokes[0].closed);
    assert_eq!(s.strokes[1].points.len(), 4);
    assert!(s.strokes[1].closed && s.strokes[2].closed && s.strokes[3].closed);
    assert_eq!(s.strokes[1].color, [255, 0, 0, 255]);
    assert_eq!(s.strokes[2].width, 6.);
    assert!(s.strokes[2].points.len() >= 16);
    // The rectangle's edges and the ellipse's rim are drawn.
    assert!(alpha(&e, id, 35, 20) > 60000 && alpha(&e, id, 35, 35) == 0);
    assert!(alpha(&e, id, 80, 40) > 60000 && alpha(&e, id, 110, 40) == 0);
    undoes_to(&mut e, &before);
    for shape in [
        json!({"shape":"line","points":[{"x":1,"y":1},{"x":2,"y":2},{"x":3,"y":3}]}),
        json!({"shape":"line","points":[{"x":1,"y":1}]}),
        json!({"shape":"rectangle","x":1,"y":1,"width":10}),
        json!({"shape":"rectangle","points":[{"x":1,"y":1}],"x":1,"y":1,"width":10,"height":10}),
        json!({"shape":"polyline","points":[{"x":1,"y":1},{"x":2,"y":2}],"x":4}),
        json!({"shape":"star","x":1,"y":1,"width":10,"height":10}),
    ] {
        refused(
            &mut e,
            "draw_vector_shapes",
            json!({"node":id,"shapes":[{"shape":"line","points":[{"x":0,"y":0},{"x":5,"y":5}]}, shape]}),
        );
    }
}

#[test]
fn describe_pages_strokes_and_points_on_request() {
    let mut e = editor();
    let id = vector(&mut e);
    let strokes: Vec<Value> = (0..5)
        .map(|i| json!({"points":[{"x":10,"y":10 + i * 20},{"x":100,"y":10 + i * 20,"width":0.5}]}))
        .collect();
    call(
        &mut e,
        "draw_vector_strokes",
        json!({"node":id,"strokes":strokes}),
    );
    let page = call(
        &mut e,
        "describe_vector_strokes",
        json!({"node":id,"limit":2}),
    );
    assert_eq!(page["stroke_count"], 5);
    assert_eq!(page["point_count"], 10);
    assert_eq!(page["next_offset"], 2);
    let first = &page["strokes"][0];
    assert_eq!(first["index"], 0);
    assert_eq!(first["color"], "#000000ff");
    assert_eq!(first["line_width"], 4.0);
    assert_eq!(first["point_count"], 2);
    assert_eq!(first["width"], json!([0.5, 1.0]));
    assert!(first["bounds"].is_array() && first.get("points").is_none());
    let last = call(
        &mut e,
        "describe_vector_strokes",
        json!({"node":id,"offset":4,"include_points":true}),
    );
    assert_eq!(last["next_offset"], Value::Null);
    assert_eq!(
        last["strokes"][0]["points"][1],
        json!({"x":100.0,"y":90.0,"width":0.5,"opacity":1.0})
    );
    let pixel = pixel_layer(&mut e);
    refused(&mut e, "describe_vector_strokes", json!({"node":pixel}));
    refused(
        &mut e,
        "describe_vector_strokes",
        json!({"node":id,"limit":0}),
    );
}

fn two_lines(e: &mut Editor) -> NodeId {
    let id = vector(e);
    call(
        e,
        "draw_vector_strokes",
        json!({"node":id,"strokes":[
            {"points":[{"x":10,"y":20},{"x":20,"y":24},{"x":30,"y":20},{"x":40,"y":24},{"x":50,"y":20}]},
            {"points":[{"x":10,"y":60},{"x":30,"y":60},{"x":50,"y":60}]}
        ]}),
    );
    id
}

#[test]
fn edit_recolours_rewidths_smooths_simplifies_and_transforms() {
    let mut e = editor();
    let id = two_lines(&mut e);
    let before = e.doc.clone();
    call(
        &mut e,
        "edit_vector_strokes",
        json!({"node":id,"strokes":[1],"color":"#00ff00","line_width":9,"simplify":0.5}),
    );
    let s = set(&e, id);
    assert_eq!(s.strokes[1].color, [0, 255, 0, 255]);
    assert_eq!(s.strokes[1].width, 9.);
    assert_eq!(s.strokes[1].points.len(), 2, "straight line simplified");
    assert_eq!(
        s.strokes[0],
        set_of(&before, id).strokes[0],
        "others untouched"
    );
    undoes_to(&mut e, &before);
    call(
        &mut e,
        "edit_vector_strokes",
        json!({"node":id,"strokes":[0],"smooth":1,"smooth_iterations":3}),
    );
    let smoothed = set(&e, id).strokes[0].clone();
    assert!(smoothed.points[1].y < 24. && smoothed.points[0].y == 20.);
    undoes_to(&mut e, &before);
    // Translate everything; rotate stroke 1 a half turn about its centre.
    call(
        &mut e,
        "edit_vector_strokes",
        json!({"node":id,"dx":5,"dy":-10}),
    );
    assert_eq!(set(&e, id).strokes[1].points[0].x, 15.);
    assert_eq!(set(&e, id).strokes[1].points[0].y, 50.);
    undoes_to(&mut e, &before);
    call(
        &mut e,
        "edit_vector_strokes",
        json!({"node":id,"strokes":[1],"rotation":180,"scale":2}),
    );
    let p = set(&e, id).strokes[1].points[0];
    assert!((p.x - 70.).abs() < 1e-9 && (p.y - 60.).abs() < 1e-9);
    assert_eq!(set(&e, id).strokes[1].width, 8.);
    undoes_to(&mut e, &before);
    call(
        &mut e,
        "edit_vector_strokes",
        json!({"node":id,"strokes":[1],"scale":0.5,"origin":{"x":10,"y":60}}),
    );
    assert_eq!(set(&e, id).strokes[1].points[2].x, 30.);
    undoes_to(&mut e, &before);
    for bad in [
        json!({"node":id}),
        json!({"node":id,"strokes":[2],"color":"#000000"}),
        json!({"node":id,"strokes":[],"color":"#000000"}),
        json!({"node":id,"smooth":2}),
        json!({"node":id,"smooth_iterations":3}),
        json!({"node":id,"origin":{"x":1,"y":1},"dx":3}),
        json!({"node":id,"scale":0}),
        json!({"node":id,"color":"#00ff00","line_width":5000}),
    ] {
        refused(&mut e, "edit_vector_strokes", bad);
    }
}

fn set_of(doc: &Document, id: NodeId) -> StrokeSet {
    let NodeKind::Strokes { strokes, .. } = &doc.node(id).unwrap().kind else {
        panic!("not a stroke layer")
    };
    (**strokes).clone()
}

#[test]
fn delete_and_outline_strokes() {
    let mut e = editor();
    let id = two_lines(&mut e);
    let before = e.doc.clone();
    let out = call(
        &mut e,
        "delete_vector_strokes",
        json!({"node":id,"strokes":[0]}),
    );
    assert_eq!(out["stroke_count"], 1);
    assert_eq!(set(&e, id).strokes[0].points[0].y, 60.);
    undoes_to(&mut e, &before);
    let painted = alpha(&e, id, 30, 60);
    let out = call(
        &mut e,
        "outline_vector_strokes",
        json!({"node":id,"strokes":[1]}),
    );
    assert_eq!(out["fill_count"], 1);
    assert_eq!(out["stroke_count"], 1);
    assert!(painted > 60000 && alpha(&e, id, 30, 60) > 50000);
    undoes_to(&mut e, &before);
    for name in ["delete_vector_strokes", "outline_vector_strokes"] {
        refused(&mut e, name, json!({"node":id,"strokes":[5]}));
        refused(&mut e, name, json!({"node":id}));
        refused(&mut e, name, json!({"node":id,"strokes":[-1]}));
    }
}

#[test]
fn erasing_cuts_long_segments_and_opens_closed_strokes() {
    let mut e = editor();
    let id = vector(&mut e);
    call(
        &mut e,
        "draw_vector_shapes",
        json!({"node":id,"shapes":[
            {"shape":"line","points":[{"x":10,"y":20},{"x":190,"y":20}]},
            {"shape":"rectangle","x":20,"y":50,"width":100,"height":50}
        ]}),
    );
    let before = e.doc.clone();
    // A vertical swipe through the middle of the line and the rectangle.
    let out = call(
        &mut e,
        "erase_vector_strokes",
        json!({"node":id,"points":[{"x":100,"y":10},{"x":100,"y":110}],"radius":5}),
    );
    assert_eq!(out["changed"], true);
    let s = set(&e, id);
    assert_eq!(s.strokes.len(), 4, "line in two, rectangle in two");
    assert!(s.strokes.iter().all(|s| !s.closed));
    assert_eq!(alpha(&e, id, 100, 20), 0);
    assert!(alpha(&e, id, 90, 20) > 60000 && alpha(&e, id, 110, 20) > 60000);
    assert_eq!(alpha(&e, id, 100, 50), 0);
    // The rectangle's other edges, including the closing one, remain.
    assert!(alpha(&e, id, 20, 75) > 60000 && alpha(&e, id, 60, 100) > 60000);
    undoes_to(&mut e, &before);
    // Missing everything changes nothing and adds no history.
    let doc = e.doc.clone();
    let out = call(
        &mut e,
        "erase_vector_strokes",
        json!({"node":id,"points":[{"x":150,"y":80}],"radius":3}),
    );
    assert_eq!(out["changed"], false);
    assert_eq!(e.doc, doc);
    for bad in [
        json!({"node":id,"points":[],"radius":3}),
        json!({"node":id,"points":[{"x":1,"y":1}],"radius":0}),
        json!({"node":id,"points":[{"x":1}],"radius":3}),
        json!({"node":id,"points":[{"x":0,"y":0},{"x":1000000,"y":0}],"radius":0.5}),
    ] {
        refused(&mut e, "erase_vector_strokes", bad);
    }
}

#[test]
fn retouch_changes_line_weight_under_the_brush() {
    let mut e = editor();
    let id = vector(&mut e);
    call(
        &mut e,
        "draw_vector_strokes",
        json!({"node":id,"line_width":4,"strokes":[
            {"points":[{"x":10,"y":30},{"x":190,"y":30}]},
            {"points":[{"x":10,"y":80},{"x":190,"y":80}]}
        ]}),
    );
    let before = e.doc.clone();
    assert_eq!(alpha(&e, id, 100, 34), 0);
    // A short swipe over the middle of both lines, limited to the first.
    call(
        &mut e,
        "retouch_vector_strokes",
        json!({"node":id,"points":[{"x":95,"y":30},{"x":105,"y":30},{"x":100,"y":80}],"mode":"thicker","radius":20,"amount":1,"strokes":[0]}),
    );
    let s = set(&e, id);
    assert!(s.strokes[0].points.iter().any(|p| p.width > 1.5));
    assert_eq!(s.strokes[0].points[0].width, 1., "ends stay thin");
    assert_eq!(s.strokes[1], set_of(&before, id).strokes[1]);
    assert!(alpha(&e, id, 100, 34) > 0);
    undoes_to(&mut e, &before);
    call(
        &mut e,
        "retouch_vector_strokes",
        json!({"node":id,"points":[{"x":100,"y":80}],"mode":"fainter","radius":10}),
    );
    assert!(set(&e, id).strokes[1].points.iter().any(|p| p.opacity < 1.));
    undoes_to(&mut e, &before);
    let doc = e.doc.clone();
    let out = call(
        &mut e,
        "retouch_vector_strokes",
        json!({"node":id,"points":[{"x":100,"y":55}],"mode":"smooth","radius":5}),
    );
    assert_eq!(out["changed"], false);
    assert_eq!(e.doc, doc);
    for bad in [
        json!({"node":id,"points":[{"x":1,"y":1}],"mode":"bolder","radius":5}),
        json!({"node":id,"points":[{"x":1,"y":1}],"mode":"thicker","radius":5,"amount":2}),
        json!({"node":id,"points":[{"x":1,"y":1}],"mode":"thicker","radius":5,"strokes":[7]}),
        json!({"node":id,"points":[{"x":1,"y":1}],"mode":"thicker"}),
    ] {
        refused(&mut e, "retouch_vector_strokes", bad);
    }
}

#[test]
fn locked_storyboard_panels_refuse_every_vector_edit() {
    let mut p = ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(64, 36)).unwrap();
    let id = vector(&mut p);
    call(
        &mut p,
        "draw_vector_strokes",
        json!({"node":id,"strokes":[{"points":[{"x":5,"y":5},{"x":40,"y":20}]}]}),
    );
    let panel = p.active_page();
    let locked = crate::storyboard_tools::execute(
        &mut p,
        "set_storyboard_locks",
        &json!({"panels":[panel],"locked":true}),
    );
    assert!(!locked.is_error, "{}", locked.content[0]["text"]);
    assert!(p.is_read_only());
    let one = json!([0]);
    for (name, args) in [
        ("add_vector_layer", json!({})),
        (
            "draw_vector_strokes",
            json!({"node":id,"strokes":[tapered()]}),
        ),
        (
            "draw_vector_shapes",
            json!({"node":id,"shapes":[{"shape":"rectangle","x":1,"y":1,"width":5,"height":5}]}),
        ),
        ("edit_vector_strokes", json!({"node":id,"dx":3})),
        ("delete_vector_strokes", json!({"node":id,"strokes":one})),
        ("outline_vector_strokes", json!({"node":id,"strokes":one})),
        (
            "erase_vector_strokes",
            json!({"node":id,"points":[{"x":20,"y":12}],"radius":4}),
        ),
        (
            "retouch_vector_strokes",
            json!({"node":id,"points":[{"x":20,"y":12}],"mode":"thicker","radius":4}),
        ),
    ] {
        let stamp = p.stamp();
        let message = refused(&mut p, name, args);
        assert!(message.contains("read-only"), "{name}: {message}");
        assert_eq!(p.stamp(), stamp);
    }
    // Reading still works.
    let page = call(&mut p, "describe_vector_strokes", json!({"node":id}));
    assert_eq!(page["stroke_count"], 1);
}
