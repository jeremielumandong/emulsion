//! The Shot Generator (phase 11): each panel's 3D set of posable
//! mannequins, props, lights and a camera, built from words or object by
//! object, framed by shot size and angle, explored for angles, and rendered
//! into the panel as a locked reference layer or an editable snapshot. Layer
//! depth (parallax) and layers that follow the set are here too. Every
//! change is one Undo step; a reference layer set to update itself is
//! rendered again in the same step.
use super::{def, panel_id};
use crate::ToolDef;
use emulsion_core::project::{PageId, ProjectEditor};
use emulsion_core::storyboard_shot::{LENSES, PanelShot, label_of as title};
use emulsion_scene::{
    Bone, BuiltinProp, CameraAngle, Character, FacePreset, HandShape, Light, LightKind, Limb,
    MannequinKind, ModelRef, ObjectId, ObjectKind, PosePreset, Prop, PropKind, RenderStyle, Rgb,
    Scene, ShotSide, ShotSize, ShotSpec, explore_shots, frame_shot,
};
use glam::Vec3;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn names<T: serde::Serialize>(all: impl IntoIterator<Item = T>) -> Value {
    json!(all.into_iter().map(|v| json!(v)).collect::<Vec<_>>())
}

fn enum_of<T: serde::Serialize>(all: impl IntoIterator<Item = T>, description: &str) -> Value {
    json!({"type":"string","enum":names(all),"description":description})
}

fn point(description: &str) -> Value {
    let n = json!({"type":"number","minimum":-100000,"maximum":100000});
    json!({"type":"object","additionalProperties":false,"properties":{"x":n,"y":n,"z":n},"required":["x","y","z"],"description":description})
}

fn object_id() -> Value {
    json!({"type":"integer","minimum":1,"description":"Object id from describe_storyboard_shot."})
}

fn layer_id() -> Value {
    json!({"type":"integer","minimum":1,"description":"Layer (node) id on the panel, from describe_storyboard_layers or list_layers."})
}

fn frame_schema() -> Value {
    json!({
        "type":"object",
        "additionalProperties":false,
        "description":"Frame a subject by shot size: the camera keeps its lens (unless focal_length_mm is given) and moves so the subject fills the frame from the crown to the size's cut line.",
        "properties":{
            "subject":object_id(),
            "size":enum_of(ShotSize::ALL, "ECU extreme_close_up (eyes), close_up (head and shoulders), medium_close_up (to mid-chest), medium (to the waist), medium_wide (cowboy, to the knees), wide (whole figure), extreme_wide (small in the set)."),
            "angle":enum_of(CameraAngle::ALL, "Camera angle; over_the_shoulder and two_shot use `secondary` (the nearest other character when left out)."),
            "side":enum_of(ShotSide::ALL, "Which side of the subject, relative to where it faces."),
            "secondary":object_id(),
            "focus":enum_of(Bone::ALL.iter().copied(), "Frame one body part of a character (hand_r, head, foot_l…) instead of the figure."),
            "focal_length_mm":{"type":"number","minimum":4,"maximum":1200},
            "group":{"type":"boolean","description":"Widen to every visible character."}
        },
        "required":["subject","size"]
    })
}

pub(super) fn definitions() -> Vec<ToolDef> {
    let rotation = json!({"type":"object","additionalProperties":false,"properties":{"yaw":{"type":"number"},"pitch":{"type":"number"},"roll":{"type":"number"}},"description":"Degrees. Yaw 0 faces +Z (toward the default camera); positive yaw turns toward +X."});
    let body = json!({
        "type":"object","additionalProperties":false,
        "description":"Mannequin sliders; omitted ones stay.",
        "properties":{
            "kind":enum_of(MannequinKind::ALL, "Body type."),
            "height":{"type":"number","minimum":0.5,"maximum":2.5,"description":"Metres."},
            "build":{"type":"number","minimum":0,"maximum":1,"description":"Slim (0) to heavy (1)."},
            "head_size":{"type":"number","minimum":0.7,"maximum":1.5},
            "leg_length":{"type":"number","minimum":0.8,"maximum":1.2},
            "arm_length":{"type":"number","minimum":0.8,"maximum":1.2},
            "shoulder_width":{"type":"number","minimum":0.7,"maximum":1.4},
            "hip_width":{"type":"number","minimum":0.7,"maximum":1.4}
        }
    });
    let joints = json!({"type":"object","additionalProperties":{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"},"z":{"type":"number"}},"additionalProperties":false},"description":"Joint angles in degrees by bone name (mannequins: hips, spine, chest, neck, head, shoulder_l, upper_arm_l, lower_arm_l, hand_l, thumb1_l…, upper_leg_r, lower_leg_r, foot_r) or, for an imported rig, by joint name. x bends forward/back, y twists, z swings sideways; right-side bones mirror y and z. Values are clamped to joint limits."});
    vec![
        def(
            "describe_storyboard_shot",
            "Read-only. A panel's Shot Generator set: every object (id, name, type, transform, colour; characters with body sliders, pose, hand shapes, IK targets, look-at and face; props with their kind and size or imported model and its joint names; lights), the camera (position, yaw, pitch, roll, focal length, film back, horizontal field of view), the environment, the reference layer's settings and layer, layers that follow the set and layer depths, plus the project's imported models and custom poses. Units are metres, Y is up, the ground is y = 0. `set` is the full set JSON that set_storyboard_shot accepts.",
            json!({"panel":panel_id()}),
            &["panel"],
        ),
        def(
            "set_storyboard_shot",
            "Create, replace or remove a panel's Shot Generator set, or change its environment and reference-layer settings. `set` replaces the whole set with set JSON (as describe_storyboard_shot returns it; missing fields take defaults, ids must be unique) — the way to build a rich set in one call. Without a set, the panel gets an empty set with key and fill lights when it has none.",
            json!({
                "panel":panel_id(),
                "set":{"type":"object","additionalProperties":{},"description":"Full set JSON: {objects:[{id,name,type:character|prop|light,…}], camera:{…}, environment:{…}}."},
                "remove":{"type":"boolean","description":"Remove the panel's set (its layers stay)."},
                "environment":{"type":"object","additionalProperties":false,"properties":{
                    "ambient":{"type":"number","minimum":0,"maximum":1},
                    "show_ground":{"type":"boolean"},"show_grid":{"type":"boolean"},"show_horizon":{"type":"boolean"},
                    "grid_spacing":{"type":"number","minimum":0.05,"maximum":100},
                    "sky_color":{"type":"string","description":"#RRGGBB"},"ground_color":{"type":"string","description":"#RRGGBB"}
                }},
                "reference":{"type":"object","additionalProperties":false,"properties":{
                    "style":enum_of(RenderStyle::ALL, "toon (shaded with lines), clay (grey shading with lines), outline (lines only, for tracing), silhouette."),
                    "opacity":{"type":"number","minimum":0.05,"maximum":1},
                    "auto_update":{"type":"boolean","description":"Render the reference layer again whenever the set changes."},
                    "shadows":{"type":"boolean","description":"Key lights cast shadows (toon and clay styles); on by default."}
                }}
            }),
            &["panel"],
        ),
        def(
            "add_storyboard_shot_object",
            "Add a character (posable mannequin), a built-in prop or a directional light to a panel's set (made if the panel has none). Returns the new object's id.",
            json!({
                "panel":panel_id(),
                "type":{"type":"string","enum":["character","prop","light"]},
                "name":{"type":"string","maxLength":200},
                "kind":{"type":"string","description":"Characters: adult_male, adult_female, adult_neutral, child. Props: box, cylinder, sphere, wall, floor, door, window_frame, table, chair, bed, sofa, car, tree, lamp_post, stairs. Lights: key, fill, rim."},
                "position":point("Metres; default the origin."),
                "yaw":{"type":"number","description":"Degrees; 0 faces the default camera (+Z)."},
                "pose":enum_of(PosePreset::ALL, "Characters: a pose preset."),
                "face":enum_of(FacePreset::ALL, "Characters: a face preset."),
                "size":point("Props: width (x), height (y) and depth (z) in metres."),
                "intensity":{"type":"number","minimum":0,"maximum":4,"description":"Lights."},
                "color":{"type":"string","description":"#RRGGBB"},
                "casts_shadows":{"type":"boolean","description":"Whether it casts a shadow (on a key light: whether that light casts shadows); default true."}
            }),
            &["panel", "type"],
        ),
        def(
            "update_storyboard_shot_object",
            "Change an object of a panel's set: name, position, rotation, scale, visibility, colour; a character's body sliders; a prop's size; a light's intensity. Omitted values stay.",
            json!({
                "panel":panel_id(),
                "object":object_id(),
                "name":{"type":"string","maxLength":200},
                "position":point("Metres."),
                "move_by":point("Move by this many metres."),
                "rotation":rotation,
                "scale":point("Scale per axis."),
                "visible":{"type":"boolean"},
                "casts_shadows":{"type":"boolean","description":"Whether it casts a shadow (on a key light: whether that light casts shadows)."},
                "color":{"type":"string","description":"#RRGGBB"},
                "body":body,
                "size":point("Props: width (x), height (y) and depth (z) in metres."),
                "intensity":{"type":"number","minimum":0,"maximum":4}
            }),
            &["panel", "object"],
        ),
        def(
            "remove_storyboard_shot_objects",
            "Remove objects from a panel's set. Layers that followed them stop following.",
            json!({"panel":panel_id(),"objects":{"type":"array","items":object_id(),"minItems":1,"maxItems":256}}),
            &["panel", "objects"],
        ),
        def(
            "pose_storyboard_character",
            "Pose a character (or an imported rig): a pose preset or a custom pose of the project, joint angles on top, hand shapes, two-bone IK targets for hands and feet (world points the wrist or ankle reaches), a look-at point for the head, and a face. Optionally bake IK and look-at into the pose, mirror it, or save it as a custom pose.",
            json!({
                "panel":panel_id(),
                "object":object_id(),
                "preset":enum_of(PosePreset::ALL, "Built-in pose."),
                "custom_pose":{"type":"string","description":"A custom pose saved in the project (describe_storyboard_shot lists them)."},
                "mirror":{"type":"boolean","description":"Mirror the pose left to right."},
                "joints":joints,
                "left_hand":enum_of(HandShape::ALL, "Hand shape."),
                "right_hand":enum_of(HandShape::ALL, "Hand shape."),
                "ik":{"type":"array","maxItems":4,"items":{"type":"object","additionalProperties":false,"properties":{"limb":enum_of(Limb::ALL, "Limb."),"target":point("World point for the wrist or ankle."),"clear":{"type":"boolean","description":"Remove this limb's target."}},"required":["limb"]}},
                "look_at":{"description":"World point {x, y, z} the head turns toward, or null to look ahead."},
                "face":enum_of(FacePreset::ALL, "Face preset."),
                "bake":{"type":"boolean","description":"Bake IK targets and look-at into the joint angles."},
                "save_as":{"type":"string","maxLength":100,"description":"Save the resulting pose under this name in the project."}
            }),
            &["panel", "object"],
        ),
        def(
            "set_storyboard_shot_camera",
            "Set a panel's shot camera: lens (focal length in mm on a Super 35 film back shaped like the board; common primes 14–200), position, yaw, pitch, roll (dutch), height, aim at a point, or frame a subject by shot size, angle and side. Framing runs after the other values.",
            json!({
                "panel":panel_id(),
                "focal_length_mm":{"type":"number","minimum":4,"maximum":1200},
                "position":point("Metres."),
                "height":{"type":"number","description":"Camera height above the ground in metres."},
                "yaw":{"type":"number"},"pitch":{"type":"number"},"roll":{"type":"number"},
                "look_at":point("Aim the camera at this world point."),
                "frame":frame_schema()
            }),
            &["panel"],
        ),
        def(
            "text_to_storyboard_shot",
            "Build a panel's set from words with the offline shot parser, e.g. \"low-angle close-up of two people at a table\", \"wide shot of a woman running down a street\", \"over-the-shoulder of Mia talking to Tom, 85mm\". It understands shot sizes, angles, sides, lenses, numbers of people and names, actions (poses), moods (faces), props, places and simple relations, and replaces the panel's set. Returns what it understood and the words it did not know — refine with the other tools (add props, pose, frame).",
            json!({"panel":panel_id(),"text":{"type":"string","minLength":1,"maxLength":500}}),
            &["panel", "text"],
        ),
        def(
            "explore_storyboard_shots",
            "Shot Explorer: propose varied camera setups (size × angle × side, each with a suited lens) on a subject of a panel's set, the same list every time. With `apply`, the chosen proposal becomes the panel's camera.",
            json!({
                "panel":panel_id(),
                "subject":object_id(),
                "count":{"type":"integer","minimum":1,"maximum":48,"description":"Default 12."},
                "apply":{"type":"integer","minimum":0,"description":"Index of the proposal to use."}
            }),
            &["panel"],
        ),
        def(
            "render_storyboard_shot",
            "Render a panel's set through its camera at the panel's resolution: `reference` puts it in the locked, reduced-opacity \"Shot Generator\" layer just above the paper (replacing the one before), `snapshot` adds an editable bitmap layer at the top to draw on. Returns the layer id.",
            json!({
                "panel":panel_id(),
                "mode":{"type":"string","enum":["reference","snapshot"]},
                "style":enum_of(RenderStyle::ALL, "Render style for this and later references.")
            }),
            &["panel", "mode"],
        ),
        def(
            "import_storyboard_model",
            "Import a glTF (.glb, or .gltf with embedded buffers) or OBJ model file into the project and place it in a panel's set. The file is stored in the project. Skinned glTF rigs are posable by joint name with pose_storyboard_character. Returns the object id and joint names.",
            json!({
                "panel":panel_id(),
                "path":{"type":"string","description":"Absolute path of the model file."},
                "position":point("Metres; default the origin.")
            }),
            &["panel", "path"],
        ),
        def(
            "set_storyboard_layer_depth",
            "Layer depth for parallax (L6): a panel layer's distance behind the panel plane in multiples of the camera's distance to it (0 is the panel plane, 1 twice as far, negative values nearer, down to -0.9). Scene camera moves then move far layers less than near ones; at rest the panel looks as drawn.",
            json!({"panel":panel_id(),"layer":layer_id(),"depth":{"type":"number","minimum":-0.9,"maximum":100}}),
            &["panel", "layer", "depth"],
        ),
        def(
            "attach_storyboard_layer",
            "Make a panel layer follow an object (or one of a character's bones) of the panel's set: when the set or camera changes, the layer moves with the object's point on screen and scales with its distance. With `at` (a panel pixel), the layer is laid on the surface the set camera sees there, with its angle: a pixel layer is warped in perspective onto that plane (a hidden \"(flat)\" copy keeps the drawing) and stays on it as the object moves or turns or the camera changes; `normal` with `point` and `object` does the same for a surface you know. `detach` stops it.",
            json!({
                "panel":panel_id(),
                "layer":layer_id(),
                "object":object_id(),
                "bone":enum_of(Bone::ALL.iter().copied(), "A character's bone (e.g. head, hand_r)."),
                "point":point("A world point on the object to follow instead of its origin."),
                "normal":point("The surface's outward normal at `point`: lay the layer on that plane."),
                "at":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2,"description":"Panel pixel [x, y]: lay the layer on the surface the set camera sees there (object, point and normal are picked)."},
                "detach":{"type":"boolean"}
            }),
            &["panel", "layer"],
        ),
    ]
}

fn parse<T: DeserializeOwned>(value: &Value, what: &str) -> Result<T, String> {
    serde_json::from_value(value.clone()).map_err(|_| format!("Unknown {what}: {value}"))
}

/// A point given as {x, y, z} (or [x, y, z]).
fn vec3(value: &Value) -> Result<Option<Vec3>, String> {
    if value.is_null() {
        return Ok(None);
    }
    let parts: Vec<Option<f64>> = match value.as_array() {
        Some(a) => a.iter().map(Value::as_f64).collect(),
        None => ["x", "y", "z"].iter().map(|k| value[*k].as_f64()).collect(),
    };
    match parts[..] {
        [Some(x), Some(y), Some(z)] if [x, y, z].iter().all(|v| v.is_finite()) => {
            Ok(Some(Vec3::new(x as f32, y as f32, z as f32)))
        }
        _ => Err("Give points as {x, y, z} in metres.".into()),
    }
}

fn color(value: &Value) -> Result<Option<Rgb>, String> {
    let Some(text) = value.as_str() else {
        return Ok(None);
    };
    let digits = text.trim().trim_start_matches('#');
    let v = u32::from_str_radix(digits, 16)
        .ok()
        .filter(|_| digits.len() == 6)
        .ok_or_else(|| format!("{text} is not a colour; use #RRGGBB."))?;
    let [_, r, g, b] = v.to_be_bytes();
    Ok(Some(Rgb([r, g, b])))
}

fn number(args: &Value, key: &str) -> Option<f32> {
    args[key].as_f64().map(|v| v as f32)
}

fn panel(editor: &ProjectEditor, args: &Value) -> Result<PageId, String> {
    let id = args["panel"].as_u64().ok_or("Give a panel.")?;
    if editor
        .storyboard()
        .is_none_or(|b| !b.panels.contains_key(&id))
    {
        return Err("Panel does not exist.".into());
    }
    Ok(id)
}

fn object(args: &Value) -> Result<ObjectId, String> {
    args["object"]
        .as_u64()
        .map(ObjectId)
        .ok_or_else(|| "Give an object.".into())
}

/// Edit a panel's set as one step and render a self-updating reference
/// again in the same step.
fn edit(
    editor: &mut ProjectEditor,
    panel: PageId,
    label: &str,
    f: impl FnOnce(
        &mut PanelShot,
        &mut emulsion_core::storyboard_shot::ShotLibrary,
    ) -> Result<(), String>,
) -> Result<(), String> {
    if editor.edit_panel_shot(panel, label, f)? {
        editor.update_shot_reference(panel)?;
    }
    Ok(())
}

fn describe(editor: &ProjectEditor, panel: PageId) -> Result<Value, String> {
    let board = editor.storyboard().ok_or("Open a storyboard.")?;
    let p = &board.panels[&panel];
    let library = &board.shot_library;
    let assets = library.assets();
    let models: Vec<Value> = library
        .models
        .iter()
        .map(|(id, m)| {
            json!({
                "asset":id,
                "name":m.name,
                "format":m.format,
                "bytes":m.data.len(),
                "joints":assets.get(id).map(|a| a.joint_names()).unwrap_or_default(),
            })
        })
        .collect();
    let depth: BTreeMap<String, f64> = p.depth.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    let mut out = json!({
        "panel":panel,
        "has_set":p.shot.is_some(),
        "layer_depth":depth,
        "models":models,
        "custom_poses":library.poses.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
        "pose_presets":names(PosePreset::ALL),
        "lenses_mm":LENSES,
    });
    if let Some(shot) = &p.shot {
        let camera = &shot.set.camera;
        out["set"] = serde_json::to_value(&shot.set).map_err(|e| e.to_string())?;
        out["camera"] = json!({
            "focal_length_mm":camera.focal_length_mm,
            "horizontal_fov_deg":camera.horizontal_fov_deg(),
            "height_m":camera.position.y,
        });
        out["reference"] = json!({
            "style":shot.reference.style,
            "opacity":shot.reference.opacity,
            "auto_update":shot.reference.auto_update,
            "shadows":shot.reference.shadows,
            "layer":shot.layer,
        });
        out["attachments"] = json!(
            shot.attachments
                .iter()
                .map(|(layer, a)| {
                    json!({"layer":layer,"object":a.object,"bone":a.bone,"on_surface":a.surface.is_some()})
                })
                .collect::<Vec<_>>()
        );
    }
    Ok(out)
}

fn set_shot(editor: &mut ProjectEditor, panel: PageId, args: &Value) -> Result<Value, String> {
    if args["remove"].as_bool() == Some(true) {
        editor.remove_panel_shot(panel)?;
        return Ok(json!({"panel":panel,"has_set":false}));
    }
    let set = match args.get("set").filter(|v| !v.is_null()) {
        Some(value) => Some(Scene::from_json(&value.to_string()).map_err(|e| e.to_string())?),
        None => None,
    };
    let env = &args["environment"];
    let reference = &args["reference"];
    edit(editor, panel, "Shot Generator set", |shot, _| {
        if let Some(set) = set {
            shot.set = set;
            shot.attachments.clear();
        }
        let e = &mut shot.set.environment;
        if let Some(v) = number(env, "ambient") {
            e.ambient = v.clamp(0., 1.);
        }
        if let Some(v) = number(env, "grid_spacing") {
            e.grid_spacing = v;
        }
        for (key, flag) in [
            ("show_ground", &mut e.show_ground),
            ("show_grid", &mut e.show_grid),
            ("show_horizon", &mut e.show_horizon),
        ] {
            if let Some(v) = env[key].as_bool() {
                *flag = v;
            }
        }
        if let Some(c) = color(&env["sky_color"])? {
            e.sky_color = c;
        }
        if let Some(c) = color(&env["ground_color"])? {
            e.ground_color = c;
        }
        if !reference["style"].is_null() {
            shot.reference.style = parse(&reference["style"], "style")?;
        }
        if let Some(v) = number(reference, "opacity") {
            shot.reference.opacity = v;
        }
        if let Some(v) = reference["shadows"].as_bool() {
            shot.reference.shadows = v;
        }
        if let Some(v) = reference["auto_update"].as_bool() {
            shot.reference.auto_update = v;
        }
        Ok(())
    })?;
    describe(editor, panel)
}

fn add_object(editor: &mut ProjectEditor, panel: PageId, args: &Value) -> Result<Value, String> {
    let position = vec3(&args["position"])?.unwrap_or(Vec3::ZERO);
    let yaw = number(args, "yaw").unwrap_or(0.);
    let kind = &args["kind"];
    let given = args["name"].as_str();
    let kind = match args["type"].as_str() {
        Some("character") => {
            let body: MannequinKind = if kind.is_null() {
                MannequinKind::AdultNeutral
            } else {
                parse(kind, "body type")?
            };
            let mut c = Character::of(body);
            if !args["pose"].is_null() {
                c = c.with_pose(parse(&args["pose"], "pose")?);
            }
            if !args["face"].is_null() {
                c.face = parse(&args["face"], "face")?;
            }
            (ObjectKind::Character(c), body.label().to_string())
        }
        Some("prop") => {
            let k: PropKind = if kind.is_null() {
                PropKind::Box
            } else {
                parse(kind, "prop")?
            };
            let mut prop = BuiltinProp::new(k);
            prop.size = vec3(&args["size"])?;
            (ObjectKind::Prop(Prop::Builtin(prop)), title(k.name()))
        }
        Some("light") => {
            let k: LightKind = if kind.is_null() {
                LightKind::Key
            } else {
                parse(kind, "light")?
            };
            let light = Light {
                kind: k,
                intensity: number(args, "intensity").unwrap_or(1.),
            };
            (
                ObjectKind::Light(light),
                format!("{} light", title(json!(k).as_str().unwrap_or("key"))),
            )
        }
        _ => return Err("Add a character, prop or light.".into()),
    };
    let mut id = ObjectId(0);
    let color = color(&args["color"])?;
    edit(editor, panel, "Add to set", |shot, _| {
        let (kind, fallback) = kind;
        id = shot.set.add(given.unwrap_or(&fallback), kind);
        shot.set.set_position(id, position);
        shot.set.set_rotation_euler(id, yaw, 0., 0.);
        let o = shot.set.object_mut(id).unwrap();
        if let Some(c) = color {
            o.color = c;
        }
        if let Some(v) = args["casts_shadows"].as_bool() {
            o.casts_shadows = v;
        }
        Ok(())
    })?;
    Ok(json!({"panel":panel,"object":id.0}))
}

fn update_object(editor: &mut ProjectEditor, panel: PageId, args: &Value) -> Result<Value, String> {
    let id = object(args)?;
    let position = vec3(&args["position"])?;
    let move_by = vec3(&args["move_by"])?;
    let scale = vec3(&args["scale"])?;
    let size = vec3(&args["size"])?;
    let color = color(&args["color"])?;
    edit(editor, panel, "Change set object", |shot, _| {
        let set = &mut shot.set;
        let o = set.object(id).ok_or("That object is not in the set.")?;
        let (yaw, pitch, roll) = o.transform.rotation.to_yaw_pitch_roll();
        if let Some(p) = position {
            set.set_position(id, p);
        }
        if let Some(d) = move_by {
            set.translate(id, d);
        }
        let r = &args["rotation"];
        if r.is_object() {
            let get = |k: &str, v: f32| number(r, k).unwrap_or(v);
            set.set_rotation_euler(id, get("yaw", yaw), get("pitch", pitch), get("roll", roll));
        }
        if let Some(s) = scale {
            set.set_scale(id, s);
        }
        let o = set.object_mut(id).unwrap();
        if let Some(name) = args["name"].as_str() {
            o.name = name.chars().take(200).collect();
        }
        if let Some(v) = args["visible"].as_bool() {
            o.visible = v;
        }
        if let Some(v) = args["casts_shadows"].as_bool() {
            o.casts_shadows = v;
        }
        if let Some(c) = color {
            o.color = c;
        }
        match &mut o.kind {
            ObjectKind::Character(c) => {
                let b = &args["body"];
                if !b["kind"].is_null() {
                    let kind: MannequinKind = parse(&b["kind"], "body type")?;
                    c.body = emulsion_scene::MannequinParams { kind, ..c.body };
                }
                if let Some(h) = number(b, "height") {
                    c.body.height = Some(h);
                }
                for (key, slot) in [
                    ("build", &mut c.body.build),
                    ("head_size", &mut c.body.head_size),
                    ("leg_length", &mut c.body.leg_length),
                    ("arm_length", &mut c.body.arm_length),
                    ("shoulder_width", &mut c.body.shoulder_width),
                    ("hip_width", &mut c.body.hip_width),
                ] {
                    if let Some(v) = number(b, key) {
                        *slot = v;
                    }
                }
                c.body = c.body.clamped();
            }
            ObjectKind::Prop(Prop::Builtin(p)) => {
                if size.is_some() {
                    p.size = size;
                }
            }
            ObjectKind::Light(l) => {
                if let Some(v) = number(args, "intensity") {
                    l.intensity = v;
                }
            }
            ObjectKind::Prop(Prop::Model(_)) => {}
        }
        Ok(())
    })?;
    Ok(json!({"panel":panel,"object":id.0}))
}

fn pose(editor: &mut ProjectEditor, panel: PageId, args: &Value) -> Result<Value, String> {
    let id = object(args)?;
    let mut ik = Vec::new();
    for item in args["ik"].as_array().into_iter().flatten() {
        let limb: Limb = parse(&item["limb"], "limb")?;
        let clear = item["clear"].as_bool() == Some(true);
        ik.push((limb, if clear { None } else { vec3(&item["target"])? }));
    }
    let look_at = match args.get("look_at") {
        None => None,
        Some(v) => Some(vec3(v)?),
    };
    let mut saved = None;
    edit(editor, panel, "Pose character", |shot, library| {
        let set = &mut shot.set;
        let o = set.object_mut(id).ok_or("That object is not in the set.")?;
        match &mut o.kind {
            ObjectKind::Prop(Prop::Model(ModelRef {
                joint_rotations, ..
            })) => {
                for (joint, r) in args["joints"].as_object().into_iter().flatten() {
                    let r = rotation(r);
                    joint_rotations.insert(joint.clone(), r);
                }
                return Ok(());
            }
            ObjectKind::Character(_) => {}
            _ => return Err("Only characters and imported rigs can be posed.".into()),
        }
        let c = set.character_mut(id).unwrap();
        if !args["preset"].is_null() {
            let preset: PosePreset = parse(&args["preset"], "pose preset")?;
            c.pose = preset.pose();
        }
        if let Some(name) = args["custom_pose"].as_str() {
            c.pose = library
                .poses
                .iter()
                .find(|p| p.name == name)
                .ok_or_else(|| format!("No custom pose “{name}”."))?
                .clone();
        }
        if args["mirror"].as_bool() == Some(true) {
            c.pose = c.pose.mirrored();
        }
        for (bone, r) in args["joints"].as_object().into_iter().flatten() {
            let bone = Bone::from_name(bone).ok_or_else(|| format!("Unknown bone {bone}."))?;
            c.pose.set(bone, rotation(r));
        }
        if !args["left_hand"].is_null() {
            c.pose.left_hand = parse::<HandShape>(&args["left_hand"], "hand shape")?;
        }
        if !args["right_hand"].is_null() {
            c.pose.right_hand = parse::<HandShape>(&args["right_hand"], "hand shape")?;
        }
        if !args["face"].is_null() {
            c.face = parse::<FacePreset>(&args["face"], "face")?;
        }
        if let Some(look) = look_at {
            c.look_at = look;
        }
        for (limb, target) in &ik {
            match target {
                Some(t) => {
                    set.set_ik_target(id, *limb, *t);
                }
                None => set
                    .character_mut(id)
                    .unwrap()
                    .ik
                    .retain(|t| t.limb != *limb),
            }
        }
        if args["bake"].as_bool() == Some(true) {
            set.bake_pose(id);
        }
        if let Some(name) = args["save_as"].as_str() {
            let pose = shot.set.character(id).unwrap().pose.clone();
            library.save_pose(name, &pose)?;
            saved = Some(name.to_string());
        }
        Ok(())
    })?;
    Ok(json!({"panel":panel,"object":id.0,"saved_pose":saved}))
}

fn rotation(value: &Value) -> emulsion_scene::JointRotation {
    let get = |k: &str| value[k].as_f64().unwrap_or(0.) as f32;
    emulsion_scene::JointRotation::new(get("x"), get("y"), get("z"))
}

fn shot_spec(set: &Scene, frame: &Value) -> Result<ShotSpec, String> {
    let subject = ObjectId(frame["subject"].as_u64().ok_or("Frame a subject.")?);
    let size: ShotSize = parse(&frame["size"], "shot size")?;
    let mut spec = ShotSpec::new(subject, size);
    if !frame["angle"].is_null() {
        spec.angle = parse(&frame["angle"], "angle")?;
    }
    if !frame["side"].is_null() {
        spec.side = parse(&frame["side"], "side")?;
    }
    if !frame["focus"].is_null() {
        spec.focus = Some(parse(&frame["focus"], "body part")?);
    }
    spec.secondary = frame["secondary"].as_u64().map(ObjectId);
    if spec.angle.needs_secondary() && spec.secondary.is_none() {
        let here = set.object(subject).map(|o| o.transform.position);
        spec.secondary = set
            .objects
            .iter()
            .filter(|o| o.id != subject && o.character().is_some())
            .min_by(|a, b| {
                let d = |o: &&emulsion_scene::SceneObject| {
                    here.map_or(0., |h| (o.transform.position - h).length_squared())
                };
                d(a).total_cmp(&d(b))
            })
            .map(|o| o.id);
    }
    spec.focal_length_mm = number(frame, "focal_length_mm");
    spec.group = frame["group"].as_bool() == Some(true);
    Ok(spec)
}

fn set_camera(editor: &mut ProjectEditor, panel: PageId, args: &Value) -> Result<Value, String> {
    let position = vec3(&args["position"])?;
    let look_at = vec3(&args["look_at"])?;
    let assets = editor.shot_assets();
    let aspect = editor.storyboard().map_or(16. / 9., |b| b.aspect());
    edit(editor, panel, "Shot camera", |shot, _| {
        let camera = &mut shot.set.camera;
        if let Some(f) = number(args, "focal_length_mm") {
            camera.focal_length_mm = f;
        }
        if let Some(p) = position {
            camera.position = p;
        }
        if let Some(h) = number(args, "height") {
            camera.position.y = h;
        }
        for (key, slot) in [
            ("yaw", &mut camera.yaw),
            ("pitch", &mut camera.pitch),
            ("roll", &mut camera.roll),
        ] {
            if let Some(v) = number(args, key) {
                *slot = v;
            }
        }
        if let Some(target) = look_at {
            camera.look_at(target);
        }
        if args["frame"].is_object() {
            let spec = shot_spec(&shot.set, &args["frame"])?;
            let camera =
                frame_shot(&shot.set, &assets, &spec, aspect).map_err(|e| e.to_string())?;
            shot.set.apply_shot(spec, camera);
        }
        Ok(())
    })?;
    let camera = editor.panel_shot(panel).unwrap().set.camera;
    Ok(json!({"panel":panel,"camera":camera,"horizontal_fov_deg":camera.horizontal_fov_deg()}))
}

fn explore(editor: &mut ProjectEditor, panel: PageId, args: &Value) -> Result<Value, String> {
    let shot = editor
        .panel_shot(panel)
        .ok_or("That panel has no Shot Generator set.")?;
    let subject = match args["subject"].as_u64() {
        Some(id) => ObjectId(id),
        None => shot
            .set
            .character_ids()
            .first()
            .copied()
            .or_else(|| {
                shot.set
                    .objects
                    .iter()
                    .find(|o| matches!(o.kind, ObjectKind::Prop(_)))
                    .map(|o| o.id)
            })
            .ok_or("Add a character or prop to explore shots of.")?,
    };
    let count = args["count"].as_u64().unwrap_or(12).clamp(1, 48) as usize;
    let aspect = editor.storyboard().map_or(16. / 9., |b| b.aspect());
    let proposals = explore_shots(&shot.set, &editor.shot_assets(), subject, count, aspect)
        .map_err(|e| e.to_string())?;
    let list: Vec<Value> = proposals
        .iter()
        .enumerate()
        .map(|(i, p)| {
            json!({
                "index":i,
                "name":p.name,
                "size":p.spec.size,
                "angle":p.spec.angle,
                "side":p.spec.side,
                "focal_length_mm":p.camera.focal_length_mm,
            })
        })
        .collect();
    if let Some(i) = args["apply"].as_u64() {
        let chosen = proposals
            .get(i as usize)
            .ok_or("There is no proposal with that index.")?;
        let camera = chosen.camera;
        edit(editor, panel, "Use shot", |shot, _| {
            shot.set.camera = camera;
            Ok(())
        })?;
    }
    Ok(json!({"panel":panel,"subject":subject.0,"proposals":list,"applied":args["apply"]}))
}

fn render_panel(editor: &mut ProjectEditor, panel: PageId, args: &Value) -> Result<Value, String> {
    if !args["style"].is_null() {
        let style: RenderStyle = parse(&args["style"], "style")?;
        editor.edit_panel_shot(panel, "Reference style", |shot, _| {
            shot.reference.style = style;
            Ok(())
        })?;
    }
    let layer = match args["mode"].as_str() {
        Some("snapshot") => {
            let (image, _) = editor.render_panel_shot(panel)?;
            editor.snapshot_shot(panel, &image)?
        }
        _ => editor.update_shot_reference(panel)?,
    };
    Ok(json!({"panel":panel,"layer":layer}))
}

fn import_model(editor: &mut ProjectEditor, panel: PageId, args: &Value) -> Result<Value, String> {
    let path = std::path::Path::new(args["path"].as_str().ok_or("Give the model's path.")?);
    let size = std::fs::metadata(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .len();
    if size as usize > emulsion_scene::limits::MAX_ASSET_BYTES {
        return Err("Models are at most 64 MB.".into());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("model");
    let position = vec3(&args["position"])?.unwrap_or(Vec3::ZERO);
    let id = editor.import_shot_model(panel, file_name, bytes, position)?;
    let shot = editor.panel_shot(panel).unwrap();
    let asset = match &shot.set.object(id).unwrap().kind {
        ObjectKind::Prop(Prop::Model(m)) => m.asset.clone(),
        _ => String::new(),
    };
    if shot.reference.auto_update && shot.layer.is_some() {
        editor.update_shot_reference(panel)?;
    }
    let joints = editor
        .shot_assets()
        .get(&asset)
        .map(|m| m.joint_names())
        .unwrap_or_default();
    Ok(json!({"panel":panel,"object":id.0,"asset":asset,"joints":joints}))
}

fn attach(editor: &mut ProjectEditor, panel: PageId, args: &Value) -> Result<Value, String> {
    let layer = args["layer"].as_u64().ok_or("Give a layer.")?;
    if args["detach"].as_bool() == Some(true) {
        editor.detach_layer_from_shot(panel, layer)?;
        return Ok(json!({"panel":panel,"layer":layer,"attached":false}));
    }
    let bone = if args["bone"].is_null() {
        None
    } else {
        Some(parse::<Bone>(&args["bone"], "bone")?)
    };
    let (id, bone, point, normal) = match args["at"].as_array() {
        Some(at) => {
            let [Some(x), Some(y)] = [0, 1].map(|i| at.get(i).and_then(Value::as_f64)) else {
                return Err("Give `at` as [x, y] panel pixels.".into());
            };
            let hit = editor.pick_panel_shot(panel, x as f32, y as f32)?;
            (hit.object, hit.bone(), Some(hit.point), Some(hit.normal))
        }
        None => (
            object(args)?,
            bone,
            vec3(&args["point"])?,
            vec3(&args["normal"])?,
        ),
    };
    editor.attach_layer_to_shot(panel, layer, id, bone, point, normal)?;
    let surface = normal.is_some();
    let flat = editor
        .panel_shot(panel)
        .and_then(|s| s.attachments.get(&layer)?.surface)
        .map(|f| f.flat);
    Ok(
        json!({"panel":panel,"layer":layer,"attached":true,"object":id.0,"on_surface":surface,"flat_layer":flat}),
    )
}

pub(super) fn run(
    editor: &mut ProjectEditor,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    type Tool = fn(&mut ProjectEditor, PageId, &Value) -> Result<Value, String>;
    let tool: Tool = match name {
        "describe_storyboard_shot" => |e, p, _| describe(e, p),
        "set_storyboard_shot" => set_shot,
        "add_storyboard_shot_object" => add_object,
        "update_storyboard_shot_object" => update_object,
        "remove_storyboard_shot_objects" => |e, p, a| {
            let ids: Vec<ObjectId> = a["objects"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_u64().map(ObjectId))
                .collect();
            edit(e, p, "Remove from set", |shot, _| {
                for id in &ids {
                    shot.set
                        .remove(*id)
                        .ok_or_else(|| format!("Object {} is not in the set.", id.0))?;
                }
                Ok(())
            })?;
            Ok(json!({"panel":p,"removed":ids.len()}))
        },
        "pose_storyboard_character" => pose,
        "set_storyboard_shot_camera" => set_camera,
        "text_to_storyboard_shot" => |e, p, a| {
            let text = a["text"].as_str().ok_or("Describe the shot.")?;
            let described = e.describe_panel_shot(p, text)?;
            if described.stale {
                e.update_shot_reference(p)?;
            }
            let mut out = describe(e, p)?;
            out["interpretation"] = json!(described.interpretation);
            out["unrecognized"] = json!(described.unrecognized);
            Ok(out)
        },
        "explore_storyboard_shots" => explore,
        "render_storyboard_shot" => render_panel,
        "import_storyboard_model" => import_model,
        "set_storyboard_layer_depth" => |e, p, a| {
            let layer = a["layer"].as_u64().ok_or("Give a layer.")?;
            let depth = a["depth"].as_f64().ok_or("Give a depth.")?;
            if e.page(p).is_none_or(|page| page.doc.node(layer).is_none()) {
                return Err("That layer is not on the panel.".into());
            }
            e.edit_storyboard(|b| b.set_layer_depth(p, layer, depth))?;
            Ok(json!({"panel":p,"layer":layer,"depth":depth}))
        },
        "attach_storyboard_layer" => attach,
        _ => return None,
    };
    Some(panel(editor, args).and_then(|p| tool(editor, p, args)))
}

#[cfg(test)]
#[path = "shot_tests.rs"]
mod tests;
