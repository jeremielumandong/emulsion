//! Layered scene export for animation production: every panel of the chosen
//! scenes as a layered OpenRaster or Photoshop file (groups, blend modes,
//! opacity and visibility kept by the shared ORA and PSD writers; review
//! layers left out, as in every export), and one
//! JSON file per scene describing its panels, timing, camera keys, layer
//! keyframes and layer comps. Files are named with the panel image tokens.
//! The schema is documented in `docs/guides/storyboard.md` (Layered scene
//! export).
use super::{Entry, PANEL_TOKENS, entries, expand, images::file_name, panel_token};
use anyhow::{Context, Result, bail};
use emulsion_core::{
    Document,
    project::{PageId, Project},
    storyboard::{GroupId, Storyboard},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

/// The schema name written into every scene file.
pub const SCHEMA: &str = "emulsion.storyboard.scene/1";

/// Tokens a scene file name may use.
pub const SCENE_TOKENS: &[&str] = &["project", "act", "seq", "scene"];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    #[default]
    Ora,
    Psd,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Ora => "ora",
            Self::Psd => "psd",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    pub format: Format,
    /// Panel file names, without extension, with the panel tokens.
    pub pattern: String,
    /// Scene file names, without extension: {project} {act} {seq} {scene}.
    pub scene_pattern: String,
    /// Scenes to export, by ID; empty exports every scene.
    pub scenes: Vec<GroupId>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            format: Format::Ora,
            pattern: "{seq}_{scene}_{panel}".into(),
            scene_pattern: "{seq}_{scene}".into(),
            scenes: Vec::new(),
        }
    }
}

impl Options {
    pub fn validate(&self) -> Result<()> {
        for (pattern, tokens) in [
            (&self.pattern, PANEL_TOKENS),
            (&self.scene_pattern, SCENE_TOKENS),
        ] {
            if pattern.trim().is_empty() {
                bail!("Enter a file name pattern, for example {{seq}}_{{scene}}")
            }
            super::validate_pattern(pattern, tokens)?;
        }
        Ok(())
    }
}

/// What an export wrote.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Written {
    /// Layered panel files, in board order.
    pub panels: Vec<PathBuf>,
    /// One JSON file per scene, in board order.
    pub scenes: Vec<PathBuf>,
}

/// One scene to write: its entries and the file names chosen.
struct Planned<'a> {
    entries: Vec<&'a Entry>,
    files: Vec<String>,
    json: String,
}

/// Write the chosen scenes of `project` into `dir` (created if needed).
/// Names are checked first: patterns that give two files one name write
/// nothing. `progress` hears (done, total) panel files.
pub fn write(
    project: &Project,
    name: &str,
    options: &Options,
    dir: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<Written> {
    options.validate()?;
    let board = super::board(project)?;
    let rate = board.settings.frame_rate;
    let all = entries(project)?;
    if let Some(missing) = options
        .scenes
        .iter()
        .find(|id| !board.scenes.contains_key(id))
    {
        bail!("No scene has ID {missing}")
    }
    let mut plan: Vec<Planned> = Vec::new();
    for entry in &all {
        if !options.scenes.is_empty() && !options.scenes.contains(&entry.scene_id) {
            continue;
        }
        match plan.last_mut() {
            Some(scene) if scene.entries[0].scene_id == entry.scene_id => scene.entries.push(entry),
            _ => plan.push(Planned {
                entries: vec![entry],
                files: Vec::new(),
                json: String::new(),
            }),
        }
    }
    if plan.is_empty() {
        bail!("Choose at least one scene to export")
    }
    let mut names = HashSet::new();
    let mut claim = |stem: String, extension: &str, what: &str| -> Result<String> {
        let stem = file_name(&stem);
        if stem.is_empty() {
            bail!("The pattern gives {what} an empty file name")
        }
        let file = format!("{stem}.{extension}");
        if !names.insert(file.to_lowercase()) {
            bail!(
                "Two files would both be named “{file}”. Add {{panel}} or {{index}} to the pattern."
            )
        }
        Ok(file)
    };
    for scene in &mut plan {
        let first = scene.entries[0];
        scene.json = claim(
            expand(&options.scene_pattern, |t| {
                panel_token(first, name, rate, t)
            })?,
            "json",
            &format!("scene {}", first.scene),
        )?;
        for entry in &scene.entries {
            scene.files.push(claim(
                expand(&options.pattern, |t| panel_token(entry, name, rate, t))?,
                options.format.extension(),
                &format!("panel {}", entry.index),
            )?);
        }
    }
    std::fs::create_dir_all(dir)
        .with_context(|| format!("Cannot create the folder {}", dir.display()))?;
    let layout: Vec<_> = project.pages.iter().map(|p| p.meta.id).collect();
    let total: usize = plan.iter().map(|s| s.entries.len()).sum();
    let mut written = Written::default();
    for scene in &plan {
        for (entry, file) in scene.entries.iter().zip(&scene.files) {
            crate::printing::canceled(cancel)?;
            let doc = page(project, entry.page)?;
            let path = dir.join(file);
            match options.format {
                Format::Ora => crate::ora::write(&doc, &path),
                Format::Psd => crate::psd::write(&doc, &path),
            }
            .with_context(|| format!("Cannot write {}", path.display()))?;
            written.panels.push(path);
            progress(written.panels.len(), total);
        }
        let value = scene_json(project, board, &layout, name, &scene.entries, &scene.files)?;
        let path = dir.join(&scene.json);
        let text = serde_json::to_string_pretty(&value)?;
        crate::write_atomic(&path, |file| {
            use std::io::Write;
            file.write_all(text.as_bytes())?;
            Ok(())
        })
        .with_context(|| format!("Cannot write {}", path.display()))?;
        written.scenes.push(path);
    }
    Ok(written)
}

/// Panel `id` as exports draw it: without review layers.
fn page(project: &Project, id: PageId) -> Result<std::borrow::Cow<'_, Document>> {
    Ok(emulsion_core::storyboard_review::printable(
        &project
            .pages
            .iter()
            .find(|p| p.meta.id == id)
            .context("Missing panel page")?
            .doc,
    ))
}

fn rate_json(board: &Storyboard) -> Value {
    let rate = board.settings.frame_rate;
    json!({"num":rate.num,"den":rate.den,"fps":rate.fps(),"drop_frame":rate.drop_frame()})
}

/// The scene file: see the schema in the Storyboard guide.
fn scene_json(
    project: &Project,
    board: &Storyboard,
    layout: &[PageId],
    name: &str,
    entries: &[&Entry],
    files: &[String],
) -> Result<Value> {
    let rate = board.settings.frame_rate;
    let first = entries[0];
    let start = first.start;
    let frames: u64 = entries.iter().map(|e| e.length()).sum();
    let camera = board.cameras.get(&first.scene_id);
    // Panel start and end frames show the camera without shake, as the
    // keys describe it; the shake is given once for the scene.
    let mut steady = board.clone();
    for camera in steady.cameras.values_mut() {
        camera.shake = None;
    }
    let state = |frame: u64| {
        let s = steady.camera_at(layout, frame as f64);
        json!({"x":s.x,"y":s.y,"zoom":s.zoom,"rotation":s.rotation})
    };
    let mut panels = Vec::new();
    for (i, (entry, file)) in entries.iter().zip(files).enumerate() {
        let doc = page(project, entry.page)?;
        let offset = entry.start - start;
        let length = entry.length();
        let last = i + 1 == entries.len();
        let keys: Vec<Value> = camera
            .into_iter()
            .flat_map(|c| &c.keys)
            .filter(|k| k.frame >= offset && (last || k.frame < offset + length))
            .map(|k| {
                json!({
                    "frame":k.frame - offset,
                    "scene_frame":k.frame,
                    "x":k.x,"y":k.y,"zoom":k.zoom,"rotation":k.rotation,
                    "easing":k.easing,"curve":k.curve,
                })
            })
            .collect();
        let layer_name = |id: emulsion_core::NodeId| doc.node(id).map(|n| n.name.clone());
        let keyframes: Vec<Value> = entry
            .panel
            .motion
            .iter()
            .map(|(id, motion)| {
                json!({
                    "layer_id":id,
                    "layer_name":layer_name(*id),
                    "pivot":motion.pivot,
                    "tracks":motion.tracks.iter().map(|t| json!({
                        "property":t.property,
                        "keys":t.keys,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        let comps: Vec<Value> = entry
            .panel
            .comps
            .iter()
            .map(|comp| {
                json!({
                    "name":comp.name,
                    "hidden":comp.hidden.iter().map(|id| json!({"id":id,"name":layer_name(*id)})).collect::<Vec<_>>(),
                })
            })
            .collect();
        let layers: Vec<Value> = doc
            .nodes
            .iter()
            .map(|n| {
                json!({
                    "id":n.id,
                    "name":n.name,
                    "parent":n.parent,
                    "kind":n.kind.tag(),
                    "visible":n.visible,
                    "opacity":n.opacity,
                    "blend":n.blend,
                    "clip_to":n.clip_to,
                })
            })
            .collect();
        let captions: serde_json::Map<String, Value> = board
            .captions
            .iter()
            .filter_map(|f| {
                let text = entry.panel.captions.get(&f.id)?;
                Some((f.name.clone(), json!(text.text)))
            })
            .collect();
        let end = entry.start + length;
        panels.push(json!({
            "id":entry.page,
            "name":entry.name,
            "number":entry.number,
            "index":entry.index,
            "file":file,
            "frames":length,
            "seconds":rate.frames_to_seconds(length),
            "start_frame":entry.start,
            "scene_frame":offset,
            "timecode_in":rate.timecode(entry.start),
            "timecode_out":rate.timecode(end),
            "thumbnail_sheet":entry.panel.thumbnails.is_some(),
            "transition":(!entry.panel.transition.is_cut()).then_some(entry.panel.transition),
            "shot":super::shot_label(&entry.panel),
            "angle":super::angle_label(&entry.panel),
            "captions":captions,
            "camera":camera.map(|_| json!({
                "start":state(entry.start),
                "end":state(end.saturating_sub(1).max(entry.start)),
                "keys":keys,
            })),
            "layers":layers,
            "layer_keyframes":keyframes,
            "layer_comps":comps,
        }));
    }
    Ok(json!({
        "schema":SCHEMA,
        "project":name,
        "project_id":board.project_id,
        "act":first.act,
        "sequence":first.sequence,
        "scene":first.scene,
        "scene_id":first.scene_id,
        "width":board.settings.width,
        "height":board.settings.height,
        "frame_rate":rate_json(board),
        "start_frame":start,
        "frames":frames,
        "timecode_in":rate.timecode(start),
        "timecode_out":rate.timecode(start + frames),
        "camera":camera.map(|c| json!({"keys":c.keys,"shake":c.shake})),
        "panels":panels,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storyboard_export::tests::project;
    use emulsion_core::command::Slot;
    use emulsion_core::motion::Easing;
    use emulsion_core::storyboard::{
        CameraKey, LayerComp, LayerMotion, LayerProperty, MotionKey, PropertyTrack, SceneCamera,
    };
    use emulsion_core::{Command, Node};
    use emulsion_raster::{BlendMode, Placement, Raster};
    use std::sync::Arc;

    /// The shared fixture with a group, a blend mode, a hidden layer, layer
    /// keys and a comp on panel 2, and a camera move in scene 1.
    fn animated() -> Project {
        let mut project = project();
        let second = project.pages[1].meta.id;
        let doc = &mut project.pages[1].doc;
        let group = Command::AddNode {
            node: Box::new(Node::group(0, "Characters")),
            slot: Slot::TOP,
        }
        .apply(doc)
        .unwrap()
        .unwrap();
        let mut hero = Node::raster(
            0,
            "Hero",
            Arc::new(Raster::solid(64, 36, [0.5, 0.1, 0.1, 0.8])),
            Placement::default(),
        );
        hero.blend = BlendMode::Multiply;
        hero.opacity = 0.5;
        let hero = Command::AddNode {
            node: Box::new(hero),
            slot: Slot::top_of(Some(group)),
        }
        .apply(doc)
        .unwrap()
        .unwrap();
        let mut ghost = Node::group(0, "Ghost");
        ghost.visible = false;
        Command::AddNode {
            node: Box::new(ghost),
            slot: Slot::TOP,
        }
        .apply(doc)
        .unwrap();
        let mut notes = Node::group(0, "Director notes");
        notes.review = true;
        Command::AddNode {
            node: Box::new(notes),
            slot: Slot::TOP,
        }
        .apply(doc)
        .unwrap();
        let hero_parent = doc.node(hero).unwrap().parent;
        let board = project.storyboard.as_mut().unwrap();
        let panel = board.panels.get_mut(&second).unwrap();
        panel.motion.insert(
            hero,
            LayerMotion {
                pivot: Some([10., 10.]),
                tracks: vec![PropertyTrack {
                    property: LayerProperty::X,
                    keys: vec![
                        MotionKey {
                            frame: 0,
                            value: 0.,
                            easing: Easing::EaseIn,
                            curve: None,
                        },
                        MotionKey {
                            frame: 20,
                            value: 30.,
                            easing: Easing::Linear,
                            curve: None,
                        },
                    ],
                }],
            },
        );
        panel.comps.push(LayerComp {
            name: "No hero".into(),
            hidden: vec![hero],
        });
        let scene = panel.scene;
        let rest = board.rest_camera();
        board.cameras.insert(
            scene,
            SceneCamera {
                keys: vec![
                    CameraKey::at(0, rest),
                    CameraKey::at(
                        60,
                        emulsion_core::storyboard::CameraState { zoom: 2., ..rest },
                    ),
                ],
                shake: None,
            },
        );
        assert_eq!(hero_parent, Some(group));
        project.validate().unwrap();
        project
    }

    #[test]
    fn scenes_export_as_layered_files_and_a_json_that_matches_the_board() {
        let project = animated();
        let board = project.storyboard.as_ref().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let mut calls = Vec::new();
        let written = write(
            &project,
            "Film",
            &Options::default(),
            dir.path(),
            &cancel,
            &mut |done, total| calls.push((done, total)),
        )
        .unwrap();
        assert_eq!(written.panels.len(), 3);
        assert_eq!(written.scenes.len(), 2);
        assert_eq!(calls.last(), Some(&(3, 3)));
        let names: Vec<_> = written
            .panels
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            [
                "Sequence 1_1_1.ora",
                "Sequence 1_1_2.ora",
                "Sequence 1_2_1.ora"
            ]
        );

        // The ORA opens back with its group, blend mode, opacity and the
        // hidden layer.
        let source = &project.pages[1].doc;
        let back = crate::ora::read(&written.panels[1]).unwrap();
        // Everything but the review layer.
        assert_eq!(back.nodes.len(), source.nodes.len() - 1);
        assert!(!back.nodes.iter().any(|n| n.name == "Director notes"));
        let hero = back.nodes.iter().find(|n| n.name == "Hero").unwrap();
        assert_eq!(hero.blend, BlendMode::Multiply);
        assert!((hero.opacity - 0.5).abs() < 1e-3);
        let parent = back.node(hero.parent.unwrap()).unwrap();
        assert!(parent.kind.is_group() && parent.name == "Characters");
        assert!(
            !back
                .nodes
                .iter()
                .find(|n| n.name == "Ghost")
                .unwrap()
                .visible
        );

        // The scene JSON describes the board.
        let text = std::fs::read_to_string(&written.scenes[0]).unwrap();
        let scene: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(scene["schema"], SCHEMA);
        assert_eq!(scene["project_id"], board.project_id.as_str());
        assert_eq!(scene["scene"], "1");
        assert_eq!(scene["frames"], 96);
        assert_eq!(scene["frame_rate"]["fps"], 24.);
        let panels = scene["panels"].as_array().unwrap();
        assert_eq!(panels.len(), 2);
        assert_eq!(panels[1]["file"], "Sequence 1_1_2.ora");
        assert_eq!(panels[1]["start_frame"], 48);
        assert_eq!(panels[1]["timecode_in"], "00:00:02:00");
        assert_eq!(panels[0]["captions"]["Dialogue"], "Wait,\nfor me!");
        // The camera key at frame 60 falls in the second panel, 12 frames in.
        let keys = panels[1]["camera"]["keys"].as_array().unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0]["frame"], 12);
        assert_eq!(keys[0]["scene_frame"], 60);
        assert_eq!(keys[0]["easing"], "ease_in_out");
        assert_eq!(panels[0]["camera"]["start"]["zoom"], 1.);
        let motion = &panels[1]["layer_keyframes"][0];
        assert_eq!(motion["layer_name"], "Hero");
        assert_eq!(motion["tracks"][0]["property"], "x");
        assert_eq!(motion["tracks"][0]["keys"][1]["value"], 30.);
        assert_eq!(motion["tracks"][0]["keys"][0]["easing"], "ease_in");
        assert_eq!(panels[1]["layer_comps"][0]["hidden"][0]["name"], "Hero");
        let layer_ids: Vec<_> = panels[1]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["id"].as_u64().unwrap())
            .collect();
        assert_eq!(
            layer_ids,
            source
                .nodes
                .iter()
                .filter(|n| !n.review)
                .map(|n| n.id)
                .collect::<Vec<_>>()
        );
        let layers = panels[1]["layers"].as_array().unwrap();
        let hero = layers.iter().find(|l| l["name"] == "Hero").unwrap();
        assert_eq!(hero["blend"], "multiply");
        let group = layers.iter().find(|l| l["name"] == "Characters").unwrap();
        assert_eq!(hero["parent"], group["id"]);
        assert_eq!(group["kind"], "grp");
        assert!(scene["panels"][0]["camera"].is_object());
        let other: Value =
            serde_json::from_str(&std::fs::read_to_string(&written.scenes[1]).unwrap()).unwrap();
        assert!(other["camera"].is_null());
    }

    #[test]
    fn psd_export_of_chosen_scenes_and_bad_patterns_write_nothing() {
        let project = animated();
        let board = project.storyboard.as_ref().unwrap();
        let second_scene = board.panels[&project.pages[2].meta.id].scene;
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let options = Options {
            format: Format::Psd,
            pattern: "{scene}-{panel:2}".into(),
            scene_pattern: "scene-{scene}".into(),
            scenes: vec![second_scene],
        };
        let written = write(
            &project,
            "Film",
            &options,
            dir.path(),
            &cancel,
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(written.panels, [dir.path().join("2-01.psd")]);
        assert_eq!(written.scenes, [dir.path().join("scene-2.json")]);
        let back = crate::psd::read(&written.panels[0]).unwrap();
        assert!(!back.nodes.is_empty());

        let out = dir.path().join("bad");
        for options in [
            Options {
                pattern: "{seq}".into(),
                ..Default::default()
            },
            Options {
                scene_pattern: "{panel}".into(),
                ..Default::default()
            },
            Options {
                scenes: vec![999],
                ..Default::default()
            },
        ] {
            assert!(write(&project, "Film", &options, &out, &cancel, &mut |_, _| {}).is_err());
        }
        assert!(!out.exists(), "nothing is written");
        assert!(
            write(
                &project,
                "Film",
                &Options::default(),
                &out,
                &AtomicBool::new(true),
                &mut |_, _| {}
            )
            .is_err()
        );
    }
}
