//! Animation tools: a camera per scene (keys timed across the scene's panels
//! with x/y/zoom/rotation, eases or bezier curves, and shake), layer
//! keyframes per panel (offsets, scale, rotation, skew, opacity and effect
//! parameters about a pivot), layer comps, and what keyframes do when panel
//! durations change. Every change is one Undo step through
//! `edit_storyboard` (applying a comp through the panel's own history);
//! invalid input changes nothing. describe_storyboard counts what is
//! animated through `panel_json`, `scene_json` and `summary_json`.
use super::{def, group_id, layout, panel_id};
use crate::ToolDef;
use emulsion_core::motion::{self, Curve, Easing, KeyView};
use emulsion_core::project::{PageId, ProjectEditor};
use emulsion_core::storyboard::{
    CameraKey, CameraState, GroupId, KeyframeSync, LayerMotion, LayerProperty, MotionKey, Panel,
    PropertyTrack, SceneCamera, Shake, Storyboard,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Most keys one call may set or delete.
const MAX_KEYS: usize = 500;
/// The longest stretch a key time may name (24 hours at 120 fps).
const MAX_FRAMES: u64 = 24 * 3600 * 120;
const EASINGS: [&str; 5] = ["linear", "ease_in", "ease_out", "ease_in_out", "step"];
const PROPERTIES: [&str; 9] = [
    "x", "y", "scale_x", "scale_y", "rotation", "skew_x", "skew_y", "opacity", "effect",
];
const PRESETS: [&str; 3] = ["handheld", "bumpy_ride", "earthquake"];

fn scene_id() -> Value {
    let mut scene = group_id();
    scene["description"] = json!("Scene ID from describe_storyboard.");
    scene
}

fn layer_id() -> Value {
    json!({"type":"integer","minimum":1,"description":"Layer ID from describe_document on that panel (or describe_storyboard_layer_motion)."})
}

/// When a key falls: one of frame, seconds or timecode.
fn time_fields(within: &str) -> serde_json::Map<String, Value> {
    let mut fields = serde_json::Map::new();
    fields.insert("frame".into(), json!({"type":"integer","minimum":0,"maximum":MAX_FRAMES,"description":format!("Frames from the start of the {within}. Use one of frame, seconds or timecode.")}));
    fields.insert(
        "seconds".into(),
        json!({"type":"number","minimum":0,"maximum":86400,"description":format!("Seconds from the start of the {within}, rounded to whole frames.")}),
    );
    fields.insert(
        "timecode".into(),
        json!({"type":"string","minLength":11,"maxLength":16,"description":format!("Time from the start of the {within} as HH:MM:SS:FF.")}),
    );
    fields
}

fn easing() -> Value {
    json!({"enum":EASINGS,"description":"How the move to the next key eases: ease_in_out for natural starts and stops, linear for constant speed, step to hold then cut."})
}

fn curve() -> Value {
    let handle =
        |what: &str| json!({"type":"number","minimum":-10,"maximum":10,"description":what});
    json!({
        "type":"object",
        "additionalProperties":false,
        "properties":{"x1":handle("0–1."),"y1":handle("-10 to 10."),"x2":handle("0–1."),"y2":handle("-10 to 10.")},
        "required":["x1","y1","x2","y2"],
        "description":"Bezier ease through two handles, like CSS cubic-bezier(x1, y1, x2, y2); replaces the easing for the move to the next key."
    })
}

fn camera_keys() -> Value {
    let mut key = time_fields("scene (or of `panel`)");
    for (name, schema) in [
        (
            "panel",
            json!({"type":"integer","minimum":1,"description":"Count the time from this panel's start instead of the scene's; it must play in the scene."}),
        ),
        (
            "x",
            json!({"type":"number","minimum":-1e6,"maximum":1e6,"description":"Centre of the shot, panel pixels. Omitted values keep the camera's value at that time."}),
        ),
        ("y", json!({"type":"number","minimum":-1e6,"maximum":1e6})),
        (
            "zoom",
            json!({"type":"number","minimum":0.05,"maximum":20,"description":"1 shows the whole frame; 2 shows half of it, closer."}),
        ),
        (
            "rotation",
            json!({"type":"number","minimum":-1e6,"maximum":1e6,"description":"Degrees."}),
        ),
        ("easing", easing()),
        ("curve", curve()),
    ] {
        key.insert(name.into(), schema);
    }
    json!({"type":"array","minItems":1,"maxItems":MAX_KEYS,"items":{"type":"object","properties":key,"additionalProperties":false}})
}

fn key_times(within: &str) -> Value {
    let mut time = time_fields(within);
    if within.starts_with("scene") {
        time.insert(
            "panel".into(),
            json!({"type":"integer","minimum":1,"description":"Count the time from this panel's start."}),
        );
    }
    json!({"type":"array","minItems":1,"maxItems":MAX_KEYS,"items":{"type":"object","properties":time,"additionalProperties":false}})
}

fn property() -> Value {
    json!({"enum":PROPERTIES,"description":"x/y: offset in panel pixels; scale_x/scale_y: 1 is unchanged; rotation and skew in degrees; opacity 0–1 multiplies the layer's own; effect: an adjustment layer's parameter named by `effect`."})
}

fn effect() -> Value {
    json!({"type":"string","minLength":1,"maxLength":64,"description":"With property effect: the parameter key, from describe_storyboard_layer_motion `effects`."})
}

pub(super) fn definitions() -> Vec<ToolDef> {
    let name = json!({"type":"string","minLength":1,"maxLength":200});
    let mut layer_key = time_fields("panel");
    layer_key.insert(
        "value".into(),
        json!({"type":"number","minimum":-1e6,"maximum":1e6}),
    );
    layer_key.insert("easing".into(), easing());
    layer_key.insert("curve".into(), curve());
    let track = json!({
        "type":"object",
        "additionalProperties":false,
        "properties":{
            "property":property(),
            "effect":effect(),
            "keys":{"type":"array","minItems":1,"maxItems":MAX_KEYS,"items":{"type":"object","properties":layer_key,"required":["value"],"additionalProperties":false}},
            "replace":{"type":"boolean","description":"Replace the track's keys instead of merging by frame."}
        },
        "required":["property","keys"]
    });
    let number =
        |what: &str| json!({"type":"number","minimum":-1e6,"maximum":1e6,"description":what});
    vec![
        def(
            "describe_storyboard_camera",
            "Read a scene's camera: where the scene starts in the animatic and how long it plays, its playing panels with their start within the scene, the camera at rest (centre of the frame, zoom 1), the keys (frame within the scene, seconds, timecode, the panel they fall in, x/y centre in panel pixels, zoom, rotation, easing or bezier curve) and the shake.",
            json!({"scene":scene_id()}),
            &["scene"],
        ),
        def(
            "set_storyboard_camera_keys",
            "Set camera keys on a scene: pans, trucks and zooms that run across the scene's panels. Each key's time is within the scene (or from `panel`'s start); a key at a frame that has one changes it, values left out keep the camera's value there. `replace` replaces every key. Easing applies to the move to the next key. Locked scenes refuse. One Undo step. Returns the camera.",
            json!({"scene":scene_id(),"keys":camera_keys(),"replace":{"type":"boolean"}}),
            &["scene", "keys"],
        ),
        def(
            "delete_storyboard_camera_keys",
            "Delete camera keys at the given times (within the scene, or from `panel`'s start). Every time must hold a key. One Undo step.",
            json!({"scene":scene_id(),"keys":key_times("scene (or of `panel`)")}),
            &["scene", "keys"],
        ),
        def(
            "reset_storyboard_camera",
            "Reset a scene's camera to rest: removes its keys and shake. One Undo step.",
            json!({"scene":scene_id()}),
            &["scene"],
        ),
        def(
            "set_storyboard_static_camera",
            "Hold the camera still through one panel at the given framing (values left out keep the camera's framing at the panel's start); the scene's camera resumes after the panel. One Undo step.",
            json!({"panel":panel_id(),"x":number("Centre x, panel pixels."),"y":number("Centre y, panel pixels."),"zoom":{"type":"number","minimum":0.05,"maximum":20},"rotation":number("Degrees.")}),
            &["panel"],
        ),
        def(
            "set_storyboard_camera_shake",
            "Shake a scene's camera on top of its keys, for impacts and handheld energy: a preset (handheld, bumpy_ride, earthquake), adjusted by any of amplitude (px), rotation (degrees), frequency (wobbles a second) and seed; or `remove`. One Undo step.",
            json!({
                "scene":scene_id(),
                "preset":{"enum":PRESETS},
                "amplitude":{"type":"number","minimum":0,"maximum":10000},
                "rotation":{"type":"number","minimum":0,"maximum":45},
                "frequency":{"type":"number","minimum":0,"maximum":60},
                "seed":{"type":"integer","minimum":0},
                "remove":{"type":"boolean"}
            }),
            &["scene"],
        ),
        def(
            "copy_storyboard_camera",
            "Copy one scene's camera (keys and shake) to another scene, replacing its camera. With `fit` (default true) the keys stretch to the other scene's length; otherwise they keep their frames. One Undo step.",
            json!({"from":scene_id(),"to":scene_id(),"fit":{"type":"boolean"}}),
            &["from", "to"],
        ),
        def(
            "describe_storyboard_layer_motion",
            "Read a panel's layer animation: duration, lock, keyframe sync, each animated layer (ID, name, pivot, tracks of keys with frame within the panel, seconds, value, easing or curve), the layers' animatable effect parameters (`effects`: key, range) and the layer comps (name, hidden layers).",
            json!({"panel":panel_id()}),
            &["panel"],
        ),
        def(
            "set_storyboard_layer_keys",
            "Set keyframes on a layer of a panel: slide, scale, turn, skew or fade a character within one panel instead of drawing many next-frame panels. Keys are timed within the panel (0 to its duration); a key at a frame that has one changes it; `replace` replaces the track. Values: x/y offsets in px, scale 1 = unchanged, degrees, opacity 0–1, effect parameters within their range. Locked panels refuse. One Undo step.",
            json!({"panel":panel_id(),"layer":layer_id(),"tracks":{"type":"array","minItems":1,"maxItems":9,"items":track}}),
            &["panel", "layer", "tracks"],
        ),
        def(
            "delete_storyboard_layer_keys",
            "Delete layer keyframes: with `keys`, the keys at those times on `property`'s track; with only `property`, the whole track; with neither, all of the layer's animation. Locked panels refuse. One Undo step.",
            json!({"panel":panel_id(),"layer":layer_id(),"property":property(),"effect":effect(),"keys":key_times("panel")}),
            &["panel", "layer"],
        ),
        def(
            "set_storyboard_layer_pivot",
            "Set the point (panel pixels) a layer turns, scales and skews about, or `clear` it to use the layer's centre. Locked panels refuse. One Undo step.",
            json!({"panel":panel_id(),"layer":layer_id(),"x":number("Panel pixels."),"y":number("Panel pixels."),"clear":{"type":"boolean"}}),
            &["panel", "layer"],
        ),
        def(
            "list_storyboard_layer_comps",
            "List a panel's layer comps: saved sets of hidden layers for alternate looks (day/night, with or without a prop), with the layer IDs and names each hides.",
            json!({"panel":panel_id()}),
            &["panel"],
        ),
        def(
            "capture_storyboard_layer_comp",
            "Save which layers of a panel are hidden now as a layer comp, replacing a comp of that name. Hide and show layers first with set_visibility. One Undo step.",
            json!({"panel":panel_id(),"name":name.clone()}),
            &["panel", "name"],
        ),
        def(
            "apply_storyboard_layer_comp",
            "Show and hide a panel's layers as a layer comp saved them. One Undo step on the panel; locked panels refuse.",
            json!({"panel":panel_id(),"name":name.clone()}),
            &["panel", "name"],
        ),
        def(
            "rename_storyboard_layer_comp",
            "Rename a panel's layer comp. One Undo step.",
            json!({"panel":panel_id(),"name":name.clone(),"new_name":name.clone()}),
            &["panel", "name", "new_name"],
        ),
        def(
            "delete_storyboard_layer_comp",
            "Delete a panel's layer comp; the layers keep their visibility. One Undo step.",
            json!({"panel":panel_id(),"name":name}),
            &["panel", "name"],
        ),
        def(
            "set_storyboard_keyframe_sync",
            "Choose what keyframes do when a panel's duration changes: `scale` (layer keys and the scene camera's keys stretch with the panel, the default) or `keep` (they keep their frames). One Undo step.",
            json!({"mode":{"enum":["scale","keep"]}}),
            &["mode"],
        ),
    ]
}

/// The keyed camera at `frame` within the scene, before shake.
fn camera_state(camera: &SceneCamera, frame: f64, rest: CameraState) -> CameraState {
    let channel = |get: fn(&CameraKey) -> f64, rest: f64| {
        motion::sample_by(&camera.keys, frame, |k| KeyView {
            time: k.frame as f64,
            value: get(k),
            easing: k.easing,
            curve: k.curve,
        })
        .unwrap_or(rest)
    };
    CameraState {
        x: channel(|k| k.x, rest.x),
        y: channel(|k| k.y, rest.y),
        zoom: channel(|k| k.zoom, rest.zoom),
        rotation: channel(|k| k.rotation, rest.rotation),
    }
}

/// At most one of `keys` in `args`.
fn one_of<'a>(args: &'a Value, keys: &[&'a str]) -> Result<Option<&'a str>, String> {
    let given: Vec<_> = keys
        .iter()
        .copied()
        .filter(|k| args.get(*k).is_some())
        .collect();
    match given[..] {
        [] => Ok(None),
        [key] => Ok(Some(key)),
        _ => Err(format!("Use only one of {}.", given.join(", "))),
    }
}

/// A time from frame, seconds or timecode in `args`.
fn time(board: &Storyboard, args: &Value) -> Result<u64, String> {
    let rate = board.settings.frame_rate;
    Ok(match one_of(args, &["frame", "seconds", "timecode"])? {
        None => return Err("Give each key a frame, seconds or timecode.".into()),
        Some("frame") => args["frame"].as_u64().unwrap(),
        Some("seconds") => rate.seconds_to_frames(args["seconds"].as_f64().unwrap()),
        Some(_) => {
            let text = args["timecode"].as_str().unwrap_or_default();
            rate.parse_timecode(text).ok_or_else(|| {
                format!("'{text}' is not a timecode at this frame rate (HH:MM:SS:FF).")
            })?
        }
    })
}

/// A scene's playing panels: ID, start within the scene and duration.
fn scene_panels(board: &Storyboard, layout: &[PageId], scene: GroupId) -> Vec<(PageId, u64, u64)> {
    let mut at = 0;
    board
        .playing(layout)
        .into_iter()
        .filter(|(id, _)| board.panels[id].scene == scene)
        .map(|(id, frames)| {
            let span = (id, at, u64::from(frames));
            at += u64::from(frames);
            span
        })
        .collect()
}

fn scene_length(board: &Storyboard, layout: &[PageId], scene: GroupId) -> u64 {
    scene_panels(board, layout, scene)
        .last()
        .map_or(0, |(_, start, frames)| start + frames)
}

fn scene_arg(board: &Storyboard, value: &Value) -> Result<GroupId, String> {
    let id = value.as_u64().unwrap();
    if !board.scenes.contains_key(&id) {
        return Err(format!(
            "No scene has ID {id}; see describe_storyboard for scene IDs."
        ));
    }
    Ok(id)
}

/// A scene whose camera may change.
fn open_scene(board: &Storyboard, value: &Value) -> Result<GroupId, String> {
    let scene = scene_arg(board, value)?;
    if board.scenes[&scene].locked {
        return Err("That scene is locked. Unlock it first.".into());
    }
    Ok(scene)
}

/// A camera key time within `scene`: from the scene start, or from `panel`'s.
fn scene_time(
    board: &Storyboard,
    layout: &[PageId],
    scene: GroupId,
    args: &Value,
) -> Result<u64, String> {
    let base = match args["panel"].as_u64() {
        None => 0,
        Some(id) => {
            scene_panels(board, layout, scene)
                .into_iter()
                .find(|(p, _, _)| *p == id)
                .ok_or_else(|| format!("Panel {id} does not play in that scene."))?
                .1
        }
    };
    let frame = base + time(board, args)?;
    let length = scene_length(board, layout, scene);
    if frame > length {
        return Err(format!(
            "Frame {frame} is past the end of the scene ({length} frames)."
        ));
    }
    Ok(frame)
}

fn easing_of(args: &Value) -> Result<Option<Easing>, String> {
    args.get("easing")
        .map(|v| serde_json::from_value(v.clone()).map_err(|e| e.to_string()))
        .transpose()
}

fn curve_of(args: &Value) -> Option<Curve> {
    serde_json::from_value(args.get("curve")?.clone()).ok()
}

fn easing_json(easing: Easing, curve: Option<Curve>) -> (Value, Value) {
    (
        serde_json::to_value(easing).unwrap_or(Value::Null),
        curve.map_or(Value::Null, |c| json!(c)),
    )
}

fn camera_json(board: &Storyboard, layout: &[PageId], scene: GroupId) -> Value {
    let rate = board.settings.frame_rate;
    let panels = scene_panels(board, layout, scene);
    let camera = board.cameras.get(&scene).cloned().unwrap_or_default();
    let (start, frames) = board.scene_span(layout, scene).unwrap_or((0, 0));
    let rest = board.rest_camera();
    let keys: Vec<_> = camera
        .keys
        .iter()
        .map(|k| {
            let (easing, curve) = easing_json(k.easing, k.curve);
            let panel = panels
                .iter()
                .find(|(_, s, f)| k.frame >= *s && k.frame < s + f)
                .or(panels.last())
                .map(|(id, _, _)| *id);
            let mut key = json!({
                "frame":k.frame,
                "seconds":rate.frames_to_seconds(k.frame),
                "timecode":rate.timecode(k.frame),
                "panel":panel,
                "x":k.x,"y":k.y,"zoom":k.zoom,"rotation":k.rotation,
                "easing":easing,
            });
            if !curve.is_null() {
                key["curve"] = curve;
            }
            key
        })
        .collect();
    let shake = camera.shake.map(|s| {
        json!({"amplitude":s.amplitude,"rotation":s.rotation,"frequency":s.frequency,"seed":s.seed})
    });
    json!({
        "scene":scene,
        "name":board.scenes[&scene].name,
        "locked":board.scenes[&scene].locked,
        "start":start,
        "frames":frames,
        "timecode":rate.timecode(start),
        "panels":panels.iter().map(|(id, s, f)| json!({"panel":id,"start":s,"frames":f})).collect::<Vec<_>>(),
        "rest":{"x":rest.x,"y":rest.y,"zoom":rest.zoom,"rotation":rest.rotation},
        "keys":keys,
        "shake":shake,
    })
}

/// Store `camera` for `scene`, dropping it when empty.
fn put_camera(b: &mut Storyboard, scene: GroupId, camera: SceneCamera) {
    if camera.is_empty() {
        b.cameras.remove(&scene);
    } else {
        b.cameras.insert(scene, camera);
    }
}

fn state_of(key: &CameraKey) -> CameraState {
    CameraState {
        x: key.x,
        y: key.y,
        zoom: key.zoom,
        rotation: key.rotation,
    }
}

fn set_camera_keys(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    args: &Value,
) -> Result<Value, String> {
    let layout = layout(editor);
    let scene = open_scene(board, &args["scene"])?;
    let rest = board.rest_camera();
    let current = board.cameras.get(&scene).cloned().unwrap_or_default();
    let mut keys: BTreeMap<u64, CameraKey> = if args["replace"] == true {
        BTreeMap::new()
    } else {
        current.keys.iter().map(|k| (k.frame, *k)).collect()
    };
    let mut seen = std::collections::HashSet::new();
    for item in args["keys"].as_array().unwrap() {
        let frame = scene_time(board, &layout, scene, item)?;
        if !seen.insert(frame) {
            return Err(format!("Two keys fall on frame {frame}."));
        }
        let mut key = keys
            .get(&frame)
            .copied()
            .unwrap_or_else(|| CameraKey::at(frame, camera_state(&current, frame as f64, rest)));
        let mut state = state_of(&key);
        for (field, slot) in [
            ("x", &mut state.x),
            ("y", &mut state.y),
            ("zoom", &mut state.zoom),
            ("rotation", &mut state.rotation),
        ] {
            if let Some(v) = item[field].as_f64() {
                *slot = v;
            }
        }
        key = CameraKey {
            easing: key.easing,
            curve: key.curve,
            ..CameraKey::at(frame, state)
        };
        if let Some(easing) = easing_of(item)? {
            key.easing = easing;
            key.curve = None;
        }
        if let Some(curve) = curve_of(item) {
            key.curve = Some(curve);
        }
        keys.insert(frame, key);
    }
    let camera = SceneCamera {
        keys: keys.into_values().collect(),
        shake: current.shake,
    };
    editor.edit_storyboard(|b| {
        put_camera(b, scene, camera);
        Ok(())
    })?;
    Ok(camera_json(editor.storyboard().unwrap(), &layout, scene))
}

fn delete_camera_keys(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    args: &Value,
) -> Result<Value, String> {
    let layout = layout(editor);
    let scene = open_scene(board, &args["scene"])?;
    let mut camera = board.cameras.get(&scene).cloned().unwrap_or_default();
    for item in args["keys"].as_array().unwrap() {
        let frame = scene_time(board, &layout, scene, item)?;
        let at = camera
            .keys
            .iter()
            .position(|k| k.frame == frame)
            .ok_or_else(|| format!("The camera has no key at frame {frame}."))?;
        camera.keys.remove(at);
    }
    editor.edit_storyboard(|b| {
        put_camera(b, scene, camera);
        Ok(())
    })?;
    Ok(camera_json(editor.storyboard().unwrap(), &layout, scene))
}

fn static_camera(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    args: &Value,
) -> Result<Value, String> {
    let layout = layout(editor);
    let id = args["panel"].as_u64().unwrap();
    let panel = board
        .panels
        .get(&id)
        .ok_or_else(|| format!("Panel {id} does not exist."))?;
    let scene = open_scene(board, &json!(panel.scene))?;
    let (_, start, frames) = scene_panels(board, &layout, scene)
        .into_iter()
        .find(|(p, _, _)| *p == id)
        .ok_or_else(|| format!("Panel {id} is a thumbnail sheet and does not play."))?;
    let end = start + frames;
    let rest = board.rest_camera();
    let old = board.cameras.get(&scene).cloned().unwrap_or_default();
    let mut state = camera_state(&old, start as f64, rest);
    for (field, slot) in [
        ("x", &mut state.x),
        ("y", &mut state.y),
        ("zoom", &mut state.zoom),
        ("rotation", &mut state.rotation),
    ] {
        if let Some(v) = args[field].as_f64() {
            *slot = v;
        }
    }
    let mut keys: BTreeMap<u64, CameraKey> = old
        .keys
        .iter()
        .filter(|k| k.frame < start || k.frame >= end)
        .map(|k| (k.frame, *k))
        .collect();
    // Resume the scene's camera after the panel, when more of it plays.
    if end < scene_length(board, &layout, scene) {
        keys.entry(end).or_insert_with(|| {
            let mut key = CameraKey::at(end, camera_state(&old, end as f64, rest));
            key.easing = old
                .keys
                .iter()
                .rev()
                .find(|k| k.frame < end)
                .map_or(key.easing, |k| k.easing);
            key
        });
    }
    if start > 0 {
        // Earlier panels keep their framing; the hold arrives on the cut.
        let hold = keys.entry(start - 1).or_insert_with(|| {
            CameraKey::at(start - 1, camera_state(&old, (start - 1) as f64, rest))
        });
        hold.easing = Easing::Step;
        hold.curve = None;
    }
    let mut key = CameraKey::at(start, state);
    key.easing = Easing::Step;
    keys.insert(start, key);
    let camera = SceneCamera {
        keys: keys.into_values().collect(),
        shake: old.shake,
    };
    editor.edit_storyboard(|b| {
        put_camera(b, scene, camera);
        Ok(())
    })?;
    Ok(camera_json(editor.storyboard().unwrap(), &layout, scene))
}

fn shake(editor: &mut ProjectEditor, board: &Storyboard, args: &Value) -> Result<Value, String> {
    let layout = layout(editor);
    let scene = open_scene(board, &args["scene"])?;
    let mut camera = board.cameras.get(&scene).cloned().unwrap_or_default();
    if args["remove"] == true {
        if ["preset", "amplitude", "rotation", "frequency", "seed"]
            .iter()
            .any(|k| args.get(*k).is_some())
        {
            return Err("Use remove on its own.".into());
        }
        camera.shake = None;
    } else {
        let mut shake = match args["preset"].as_str() {
            Some(name) => {
                let at = PRESETS.iter().position(|p| *p == name).unwrap();
                Shake::PRESETS[at].1
            }
            None => camera.shake.unwrap_or(Shake::PRESETS[0].1),
        };
        for (field, slot) in [
            ("amplitude", &mut shake.amplitude),
            ("rotation", &mut shake.rotation),
            ("frequency", &mut shake.frequency),
        ] {
            if let Some(v) = args[field].as_f64() {
                *slot = v;
            }
        }
        if let Some(seed) = args["seed"].as_u64() {
            shake.seed = seed;
        }
        camera.shake = Some(shake);
    }
    editor.edit_storyboard(|b| {
        put_camera(b, scene, camera);
        Ok(())
    })?;
    Ok(camera_json(editor.storyboard().unwrap(), &layout, scene))
}

fn copy_camera(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    args: &Value,
) -> Result<Value, String> {
    let layout = layout(editor);
    let from = scene_arg(board, &args["from"])?;
    let to = open_scene(board, &args["to"])?;
    if from == to {
        return Err("Choose two different scenes.".into());
    }
    let mut camera = board.cameras.get(&from).cloned().unwrap_or_default();
    let (a, b) = (
        scene_length(board, &layout, from),
        scene_length(board, &layout, to),
    );
    if args["fit"] != false && a > 0 && a != b {
        let k = b as f64 / a as f64;
        let mut last = None;
        camera.keys.retain_mut(|key| {
            key.frame = (key.frame as f64 * k).round() as u64;
            let keep = last.is_none_or(|l| key.frame > l);
            last = Some(key.frame);
            keep
        });
    }
    editor.edit_storyboard(|board| {
        put_camera(board, to, camera);
        Ok(())
    })?;
    Ok(camera_json(editor.storyboard().unwrap(), &layout, to))
}

/// A panel and its data.
fn panel_arg<'a>(board: &'a Storyboard, value: &Value) -> Result<(PageId, &'a Panel), String> {
    let id = value.as_u64().unwrap();
    board
        .panels
        .get(&id)
        .map(|p| (id, p))
        .ok_or_else(|| format!("Panel {id} does not exist."))
}

/// A panel whose layer animation may change, and a layer on it.
fn open_layer(
    editor: &ProjectEditor,
    board: &Storyboard,
    args: &Value,
) -> Result<(PageId, u64), String> {
    let (id, panel) = panel_arg(board, &args["panel"])?;
    if panel.locked || board.is_locked(id) {
        return Err("That panel is locked. Unlock it first.".into());
    }
    let layer = args["layer"].as_u64().unwrap();
    let doc = &editor.page(id).ok_or("Panel does not exist.")?.doc;
    if doc.node(layer).is_none() {
        return Err(format!(
            "Panel {id} has no layer {layer}; see describe_document on that panel."
        ));
    }
    Ok((id, layer))
}

/// The property a track argument names, checked against the layer.
fn property_of(
    editor: &ProjectEditor,
    panel: PageId,
    layer: u64,
    args: &Value,
) -> Result<LayerProperty, String> {
    let name = args["property"].as_str().unwrap();
    if name != "effect" && args.get("effect").is_some() {
        return Err("effect goes with property effect.".into());
    }
    Ok(match name {
        "x" => LayerProperty::X,
        "y" => LayerProperty::Y,
        "scale_x" => LayerProperty::ScaleX,
        "scale_y" => LayerProperty::ScaleY,
        "rotation" => LayerProperty::Rotation,
        "skew_x" => LayerProperty::SkewX,
        "skew_y" => LayerProperty::SkewY,
        "opacity" => LayerProperty::Opacity,
        _ => {
            let key = args["effect"]
                .as_str()
                .ok_or("property effect needs `effect`, the parameter key.")?;
            let node = editor.page(panel).and_then(|e| e.doc.node(layer)).unwrap();
            if !node.params().iter().any(|p| p.key == key) {
                let keys: Vec<_> = node.params().iter().map(|p| p.key).collect();
                return Err(if keys.is_empty() {
                    format!("Layer {layer} has no effect parameters to animate.")
                } else {
                    format!(
                        "Layer {layer} has no parameter '{key}'. Parameters: {}",
                        keys.join(", ")
                    )
                });
            }
            LayerProperty::Effect(key.into())
        }
    })
}

fn effect_range(
    editor: &ProjectEditor,
    panel: PageId,
    layer: u64,
    key: &str,
) -> Option<(f64, f64)> {
    let node = editor.page(panel)?.doc.node(layer)?;
    node.params()
        .into_iter()
        .find(|p| p.key == key)
        .map(|p| (f64::from(p.min), f64::from(p.max)))
}

fn property_name(property: &LayerProperty) -> (&'static str, Option<&str>) {
    match property {
        LayerProperty::X => ("x", None),
        LayerProperty::Y => ("y", None),
        LayerProperty::ScaleX => ("scale_x", None),
        LayerProperty::ScaleY => ("scale_y", None),
        LayerProperty::Rotation => ("rotation", None),
        LayerProperty::SkewX => ("skew_x", None),
        LayerProperty::SkewY => ("skew_y", None),
        LayerProperty::Opacity => ("opacity", None),
        LayerProperty::Effect(key) => ("effect", Some(key)),
    }
}

/// A key time within a panel of `frames`.
fn panel_time(board: &Storyboard, frames: u32, args: &Value) -> Result<u64, String> {
    let frame = time(board, args)?;
    if frame > u64::from(frames) {
        return Err(format!(
            "Frame {frame} is past the end of the panel ({frames} frames)."
        ));
    }
    Ok(frame)
}

fn motion_json(editor: &ProjectEditor, board: &Storyboard, id: PageId) -> Value {
    let rate = board.settings.frame_rate;
    let panel = &board.panels[&id];
    let doc = editor.page(id).map(|e| &e.doc);
    let name = |layer: u64| doc.and_then(|d| d.node(layer)).map(|n| n.name.clone());
    let layers: Vec<_> = panel
        .motion
        .iter()
        .map(|(layer, motion)| {
            let tracks: Vec<_> = motion
                .tracks
                .iter()
                .map(|t| {
                    let (property, effect) = property_name(&t.property);
                    let keys: Vec<_> = t
                        .keys
                        .iter()
                        .map(|k| {
                            let (easing, curve) = easing_json(k.easing, k.curve);
                            let mut key = json!({"frame":k.frame,"seconds":rate.frames_to_seconds(k.frame),"value":k.value,"easing":easing});
                            if !curve.is_null() {
                                key["curve"] = curve;
                            }
                            key
                        })
                        .collect();
                    let mut track = json!({"property":property,"keys":keys});
                    if let Some(effect) = effect {
                        track["effect"] = json!(effect);
                    }
                    track
                })
                .collect();
            json!({"layer":layer,"name":name(*layer),"pivot":motion.pivot,"tracks":tracks})
        })
        .collect();
    let effects: Vec<_> = doc
        .into_iter()
        .flat_map(|d| &d.nodes)
        .filter(|n| !n.params().is_empty())
        .map(|n| {
            json!({"layer":n.id,"name":n.name,"params":n.params().iter().map(|p| json!({"key":p.key,"min":p.min,"max":p.max,"value":p.value})).collect::<Vec<_>>()})
        })
        .collect();
    json!({
        "panel":id,
        "frames":panel.frames,
        "seconds":rate.frames_to_seconds(u64::from(panel.frames)),
        "locked":board.is_locked(id),
        "keyframe_sync":board.keyframe_sync,
        "layers":layers,
        "effects":effects,
        "comps":comps_json(editor, board, id),
    })
}

fn comps_json(editor: &ProjectEditor, board: &Storyboard, id: PageId) -> Value {
    let doc = editor.page(id).map(|e| &e.doc);
    board.panels[&id]
        .comps
        .iter()
        .map(|c| {
            let names: Vec<_> = c
                .hidden
                .iter()
                .filter_map(|l| doc.and_then(|d| d.node(*l)).map(|n| n.name.clone()))
                .collect();
            json!({"name":c.name,"hidden":c.hidden,"hidden_names":names})
        })
        .collect()
}

fn set_layer_keys(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    args: &Value,
) -> Result<Value, String> {
    let (id, layer) = open_layer(editor, board, args)?;
    let frames = board.panels[&id].frames;
    let mut motion = board.panels[&id]
        .motion
        .get(&layer)
        .cloned()
        .unwrap_or_default();
    let mut changed = Vec::new();
    for item in args["tracks"].as_array().unwrap() {
        let property = property_of(editor, id, layer, item)?;
        if changed.contains(&property) {
            return Err("List each property once.".into());
        }
        let range = match &property {
            LayerProperty::Effect(key) => effect_range(editor, id, layer, key),
            _ => None,
        };
        let existing = motion.track(&property).filter(|_| item["replace"] != true);
        let mut keys: BTreeMap<u64, MotionKey> = existing
            .map(|t| t.keys.iter().map(|k| (k.frame, *k)).collect())
            .unwrap_or_default();
        let mut seen = std::collections::HashSet::new();
        for k in item["keys"].as_array().unwrap() {
            let frame = panel_time(board, frames, k)?;
            if !seen.insert(frame) {
                return Err(format!("Two keys fall on frame {frame}."));
            }
            let value = k["value"].as_f64().unwrap();
            if let Some((min, max)) = range
                && !(min..=max).contains(&value)
            {
                return Err(format!("{} runs from {min} to {max}.", property.label()));
            }
            let mut key = keys.get(&frame).copied().unwrap_or(MotionKey {
                frame,
                value,
                easing: Easing::EaseInOut,
                curve: None,
            });
            key.value = value;
            if let Some(easing) = easing_of(k)? {
                key.easing = easing;
                key.curve = None;
            }
            if let Some(curve) = curve_of(k) {
                key.curve = Some(curve);
            }
            keys.insert(frame, key);
        }
        let track = PropertyTrack {
            property: property.clone(),
            keys: keys.into_values().collect(),
        };
        match motion.tracks.iter_mut().find(|t| t.property == property) {
            Some(t) => *t = track,
            None => motion.tracks.push(track),
        }
        changed.push(property);
    }
    editor.edit_storyboard(|b| {
        b.panels.get_mut(&id).unwrap().motion.insert(layer, motion);
        Ok(())
    })?;
    Ok(motion_json(editor, editor.storyboard().unwrap(), id))
}

fn delete_layer_keys(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    args: &Value,
) -> Result<Value, String> {
    let (id, layer) = open_layer(editor, board, args)?;
    let frames = board.panels[&id].frames;
    let Some(mut motion) = board.panels[&id].motion.get(&layer).cloned() else {
        return Err(format!("Layer {layer} has no keyframes on panel {id}."));
    };
    match args.get("property") {
        None if args.get("keys").is_some() || args.get("effect").is_some() => {
            return Err("Name the property whose keys to delete.".into());
        }
        None => motion.tracks.clear(),
        Some(_) => {
            let property = property_of(editor, id, layer, args)?;
            let at = motion
                .tracks
                .iter()
                .position(|t| t.property == property)
                .ok_or_else(|| format!("Layer {layer} has no {} keys.", property.label()))?;
            match args["keys"].as_array() {
                None => {
                    motion.tracks.remove(at);
                }
                Some(times) => {
                    let track = &mut motion.tracks[at];
                    for t in times {
                        let frame = panel_time(board, frames, t)?;
                        let k = track
                            .keys
                            .iter()
                            .position(|k| k.frame == frame)
                            .ok_or_else(|| format!("No key at frame {frame}."))?;
                        track.keys.remove(k);
                    }
                    if track.keys.is_empty() {
                        motion.tracks.remove(at);
                    }
                }
            }
        }
    }
    editor.edit_storyboard(|b| {
        let panel = b.panels.get_mut(&id).unwrap();
        if motion.tracks.is_empty() && motion.pivot.is_none() {
            panel.motion.remove(&layer);
        } else {
            panel.motion.insert(layer, motion);
        }
        Ok(())
    })?;
    Ok(motion_json(editor, editor.storyboard().unwrap(), id))
}

fn set_pivot(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    args: &Value,
) -> Result<Value, String> {
    let (id, layer) = open_layer(editor, board, args)?;
    let mut motion = board.panels[&id]
        .motion
        .get(&layer)
        .cloned()
        .unwrap_or_default();
    match (
        args["clear"] == true,
        args["x"].as_f64(),
        args["y"].as_f64(),
    ) {
        (true, None, None) => motion.pivot = None,
        (false, Some(x), Some(y)) => motion.pivot = Some([x, y]),
        _ => return Err("Give x and y, or clear on its own.".into()),
    }
    editor.edit_storyboard(|b| {
        let panel = b.panels.get_mut(&id).unwrap();
        if motion == LayerMotion::default() {
            panel.motion.remove(&layer);
        } else {
            panel.motion.insert(layer, motion);
        }
        Ok(())
    })?;
    Ok(motion_json(editor, editor.storyboard().unwrap(), id))
}

/// A panel's comp by name.
fn comp_arg(board: &Storyboard, args: &Value) -> Result<(PageId, String), String> {
    let (id, panel) = panel_arg(board, &args["panel"])?;
    let name = args["name"].as_str().unwrap().trim().to_string();
    if !panel.comps.iter().any(|c| c.name == name) {
        let names: Vec<_> = panel.comps.iter().map(|c| c.name.as_str()).collect();
        return Err(format!(
            "Panel {id} has no layer comp '{name}'. Comps: {}",
            if names.is_empty() {
                "none".into()
            } else {
                names.join(", ")
            }
        ));
    }
    Ok((id, name))
}

/// Run an animation tool; `None` when `name` is not one.
pub(super) fn run(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    let result = match name {
        "describe_storyboard_camera" => {
            scene_arg(board, &args["scene"]).map(|scene| camera_json(board, &layout(editor), scene))
        }
        "set_storyboard_camera_keys" => set_camera_keys(editor, board, args),
        "delete_storyboard_camera_keys" => delete_camera_keys(editor, board, args),
        "reset_storyboard_camera" => (|| {
            let scene = open_scene(board, &args["scene"])?;
            editor.edit_storyboard(|b| {
                b.cameras.remove(&scene);
                Ok(())
            })?;
            Ok(camera_json(
                editor.storyboard().unwrap(),
                &layout(editor),
                scene,
            ))
        })(),
        "set_storyboard_static_camera" => static_camera(editor, board, args),
        "set_storyboard_camera_shake" => shake(editor, board, args),
        "copy_storyboard_camera" => copy_camera(editor, board, args),
        "describe_storyboard_layer_motion" => {
            panel_arg(board, &args["panel"]).map(|(id, _)| motion_json(editor, board, id))
        }
        "set_storyboard_layer_keys" => set_layer_keys(editor, board, args),
        "delete_storyboard_layer_keys" => delete_layer_keys(editor, board, args),
        "set_storyboard_layer_pivot" => set_pivot(editor, board, args),
        "list_storyboard_layer_comps" => panel_arg(board, &args["panel"])
            .map(|(id, _)| json!({"panel":id,"comps":comps_json(editor, board, id)})),
        "capture_storyboard_layer_comp" => (|| {
            let (id, _) = panel_arg(board, &args["panel"])?;
            editor.capture_comp(id, args["name"].as_str().unwrap())?;
            Ok(json!({"panel":id,"comps":comps_json(editor, editor.storyboard().unwrap(), id)}))
        })(),
        "apply_storyboard_layer_comp" => (|| {
            let (id, name) = comp_arg(board, args)?;
            editor.apply_comp(id, &name)?;
            Ok(json!({"panel":id,"applied":name}))
        })(),
        "rename_storyboard_layer_comp" => (|| {
            let (id, name) = comp_arg(board, args)?;
            let new_name = args["new_name"].as_str().unwrap().trim().to_string();
            editor.edit_storyboard(|b| {
                let comps = &mut b.panels.get_mut(&id).unwrap().comps;
                if new_name != name && comps.iter().any(|c| c.name == new_name) {
                    return Err(format!("Panel {id} already has a comp '{new_name}'."));
                }
                comps.iter_mut().find(|c| c.name == name).unwrap().name = new_name;
                Ok(())
            })?;
            Ok(json!({"panel":id,"comps":comps_json(editor, editor.storyboard().unwrap(), id)}))
        })(),
        "delete_storyboard_layer_comp" => (|| {
            let (id, name) = comp_arg(board, args)?;
            editor.edit_storyboard(|b| {
                b.panels
                    .get_mut(&id)
                    .unwrap()
                    .comps
                    .retain(|c| c.name != name);
                Ok(())
            })?;
            Ok(json!({"panel":id,"comps":comps_json(editor, editor.storyboard().unwrap(), id)}))
        })(),
        "set_storyboard_keyframe_sync" => (|| {
            let mode: KeyframeSync =
                serde_json::from_value(args["mode"].clone()).map_err(|e| e.to_string())?;
            editor.edit_storyboard(|b| {
                b.keyframe_sync = mode;
                Ok(())
            })?;
            Ok(json!({"keyframe_sync":mode}))
        })(),
        _ => return None,
    };
    Some(result)
}

/// describe_storyboard: a panel's animation counts, when it has any.
pub(super) fn panel_json(panel: &Panel, entry: &mut Value) {
    if !panel.motion.is_empty() {
        entry["animated_layers"] = json!(panel.motion.len());
    }
    if !panel.comps.is_empty() {
        entry["comps"] = json!(panel.comps.len());
    }
}

/// describe_storyboard: a scene's camera, when it has one.
pub(super) fn scene_json(board: &Storyboard, scene: GroupId, entry: &mut Value) {
    if let Some(camera) = board.cameras.get(&scene) {
        entry["camera"] = json!({"keys":camera.keys.len(),"shake":camera.shake.is_some()});
    }
}

/// describe_storyboard: how much of the board is animated.
pub(super) fn summary_json(board: &Storyboard) -> Value {
    json!({
        "keyframe_sync":board.keyframe_sync,
        "scenes_with_camera":board.cameras.len(),
        "animated_panels":board.panels.values().filter(|p| !p.motion.is_empty()).count(),
        "panels_with_comps":board.panels.values().filter(|p| !p.comps.is_empty()).count(),
    })
}
