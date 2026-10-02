//! Library and template tools: the project library saved in the storyboard's
//! `.emu`, the personal library shared by every storyboard (kept in the
//! creative library), and storyboard templates. Project library edits and
//! placing are one Undo step each; the personal library and templates are
//! files on disk outside the project and its Undo.
use super::{def, group_id, panel_id};
use crate::ToolDef;
use emulsion_core::project::ProjectEditor;
use emulsion_core::storyboard::Storyboard;
use emulsion_core::storyboard_library::{ItemKind, LibraryItem, Placed, matches};
use emulsion_io::creative_library::{self as catalog, AssetKind, Catalog};
use emulsion_io::{storyboard_library as personal, template_pack};
use serde_json::{Value, json};
use std::path::Path;

fn scope() -> Value {
    json!({"enum":["project","personal"],"description":"project: the library saved in this storyboard's .emu (default). personal: the library on this computer shared by every storyboard."})
}

fn item() -> Value {
    json!({"type":"integer","minimum":1,"description":"Item ID from list_storyboard_library, in the given scope."})
}

fn tags() -> Value {
    json!({"type":"array","items":{"type":"string","minLength":1,"maxLength":200},"minItems":0,"maxItems":50,"description":"An empty list clears the tags."})
}

pub(super) fn definitions() -> Vec<ToolDef> {
    let name = json!({"type":"string","minLength":1,"maxLength":200});
    let query = json!({"type":"string","maxLength":200,"description":"Words that must all appear in the name or a tag (case-insensitive)."});
    vec![
        def(
            "list_storyboard_library",
            "List reusable drawings (characters, props, backgrounds) in the project library (saved in this storyboard) and the personal library (shared by every storyboard on this computer). Each item has an ID, name, tags and kind: `layers` (placed on top of a panel), `panel` (placed as a new panel) or `scene` (placed as a new scene). `animated` items bring layer keyframes, comps or camera moves; `panels` counts a scene item's panels. Project items also list their top-level layer names.",
            json!({"scope":{"enum":["project","personal","all"],"description":"Default all."},"query":query}),
            &[],
        ),
        def(
            "add_to_storyboard_library",
            "Add a drawing to a library: with `layers` (top-level or nested layer IDs from describe_document on that panel), a layers item holding copies of them at their positions in the frame; with `scene`, a scene item holding the whole scene (every panel's drawing, duration, captions, shot data, layer keyframes and comps, and the scene camera); otherwise a panel item holding the whole panel (default: the active panel) and, when it is animated, its duration, layer keyframes, comps and the scene camera's keys over it, re-timed to the panel. Adding to the project library is one Undo step; the personal library is saved on disk at once. Returns the item ID.",
            json!({
                "scope":scope(),
                "name":name,
                "tags":tags(),
                "panel":panel_id(),
                "scene":group_id(),
                "layers":{"type":"array","items":{"type":"integer","minimum":1},"minItems":1,"maxItems":500}
            }),
            &["name"],
        ),
        def(
            "place_storyboard_library_item",
            "Place a library item. A layers item goes on top of the active panel at its original position (fitted to the frame when it came from another resolution); a panel item becomes a new panel after the active one, in its scene, which becomes active (an animated one keeps its duration, keyframes and comps, and its camera keys join the scene camera over the new panel); a scene item becomes a new scene after the active panel's scene with all its panels, captions, keyframes, comps and camera, the first panel active. Frames keep their time at this board's frame rate. One Undo step. Select the panel first with select_project_page.",
            json!({"scope":scope(),"item":item()}),
            &["item"],
        ),
        def(
            "update_storyboard_library_item",
            "Rename a library item and, with `tags`, replace its tags. One Undo step in the project library.",
            json!({"scope":scope(),"item":item(),"name":name,"tags":tags()}),
            &["item", "name"],
        ),
        def(
            "remove_storyboard_library_item",
            "Delete a library item. Undo restores a project library item; a personal library item and its file are deleted from disk for every storyboard.",
            json!({"scope":scope(),"item":item()}),
            &["item"],
        ),
        def(
            "list_storyboard_templates",
            "List installed storyboard templates: ID, name, tags, author, and the resolution, frame rate and panel count they start with. Start a storyboard from one with create_storyboard_from_template.",
            json!({"query":query}),
            &[],
        ),
        def(
            "save_storyboard_template",
            "Save this storyboard as a template in the personal library: resolution, frame rate, caption fields, naming rules, Smart add layers, stage guides, palette, project library and its panels with their layers (version history is left out). It then appears in New canvas and list_storyboard_templates. Returns the template ID.",
            json!({
                "name":name,
                "tags":tags(),
                "author":{"type":"string","maxLength":2000},
                "license":{"type":"string","maxLength":4000},
                "description":{"type":"string","maxLength":8000}
            }),
            &["name"],
        ),
    ]
}

/// Installed storyboard templates in `catalog`.
pub fn templates(catalog: &Catalog) -> impl Iterator<Item = &catalog::Asset> {
    catalog
        .assets
        .iter()
        .filter(|a| a.kind == AssetKind::StoryboardTemplate)
}

/// The path of an installed storyboard template, for
/// `create_storyboard_from_template`.
pub fn template_path(root: &Path, id: u64) -> Result<std::path::PathBuf, String> {
    let catalog = catalog::load(root).map_err(|e| e.to_string())?;
    templates(&catalog)
        .find(|a| a.id == id)
        .map(|a| a.path.clone())
        .ok_or_else(|| "No storyboard template has that ID; see list_storyboard_templates.".into())
}

fn personal_scope(args: &Value) -> bool {
    args["scope"].as_str() == Some("personal")
}

fn tag_list(args: &Value) -> Option<Vec<String>> {
    args["tags"].as_array().map(|tags| {
        tags.iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect()
    })
}

fn kind_name(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Layers => "layers",
        ItemKind::Panel => "panel",
        ItemKind::Scene => "scene",
    }
}

fn placed(editor: &ProjectEditor, placed: Placed) -> Value {
    match placed {
        Placed::Layers(layers) => {
            json!({"placed":"layers","layers":layers,"active_panel":editor.active_page()})
        }
        Placed::Panel(panel) => json!({"placed":"panel","panel":panel,"active_panel":panel}),
        Placed::Scene { scene, panels } => {
            json!({"placed":"scene","scene":scene,"panels":panels,"active_panel":editor.active_page()})
        }
    }
}

pub(super) fn run(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    run_in(&catalog::root(), editor, board, name, args)
}

/// `run` with the personal library under `root`.
pub(super) fn run_in(
    root: &Path,
    editor: &mut ProjectEditor,
    board: &Storyboard,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    let load = || catalog::load(root).map_err(|e| e.to_string());
    let result = match name {
        "list_storyboard_library" => (|| {
            let query = args["query"].as_str().unwrap_or("");
            let scope = args["scope"].as_str().unwrap_or("all");
            let mut out = json!({});
            if scope != "personal" {
                out["project"] = board
                    .library
                    .items
                    .iter()
                    .filter(|i| i.matches(query))
                    .map(|i| {
                        let mut entry = json!({
                            "item":i.id,"name":i.name,"tags":i.tags,"kind":kind_name(i.kind),
                            "width":i.doc.width,"height":i.doc.height,
                            "layers":i.doc.nodes.iter().filter(|n| n.parent.is_none()).map(|n| &n.name).collect::<Vec<_>>(),
                        });
                        if i.is_animated() {
                            entry["animated"] = json!(true);
                        }
                        if i.kind == ItemKind::Scene {
                            entry["panels"] = json!(i.drawings().count());
                        }
                        entry
                    })
                    .collect();
            }
            if scope != "project" {
                out["personal"] = personal::items(&load()?)
                    .filter(|(a, _)| matches(&a.name, &a.tags, query))
                    .map(|(a, kind)| json!({"item":a.id,"name":a.name,"tags":a.tags,"kind":kind_name(kind)}))
                    .collect();
            }
            Ok(out)
        })(),
        "add_to_storyboard_library" => (|| {
            let panel = args["panel"].as_u64().unwrap_or(editor.active_page());
            let name = args["name"].as_str().unwrap();
            let tags = tag_list(args).unwrap_or_default();
            let layers: Option<Vec<u64>> = args["layers"]
                .as_array()
                .map(|ids| ids.iter().filter_map(Value::as_u64).collect());
            let scene = args["scene"].as_u64();
            if scene.is_some() && (layers.is_some() || args.get("panel").is_some()) {
                return Err("Use scene on its own, without panel or layers.".into());
            }
            let item = match (&layers, scene) {
                (Some(ids), _) => {
                    let page = editor.page(panel).ok_or("Panel does not exist.")?;
                    LibraryItem::drawing(
                        ItemKind::Layers,
                        emulsion_core::storyboard_library::capture_layers(&page.doc, ids)?,
                    )
                }
                (None, Some(scene)) => editor.capture_scene_item(scene)?,
                (None, None) => editor.capture_panel_item(panel)?,
            };
            let (kind, animated) = (item.kind, item.is_animated());
            let (scope, id) = if personal_scope(args) {
                let (_, id) =
                    personal::add_item(root, name, &tags, &item).map_err(|e| e.to_string())?;
                ("personal", id)
            } else {
                ("project", editor.add_library_entry(name, &tags, item)?)
            };
            Ok(json!({"scope":scope,"item":id,"kind":kind_name(kind),"animated":animated}))
        })(),
        "place_storyboard_library_item" => (|| {
            let id = args["item"].as_u64().unwrap();
            let result = if personal_scope(args) {
                let catalog = load()?;
                let (asset, _) = personal::items(&catalog)
                    .find(|(a, _)| a.id == id)
                    .ok_or("No personal library item has that ID.")?;
                let item = personal::load_item(asset).map_err(|e| e.to_string())?;
                editor.place_item(&item)?
            } else {
                editor.place_library_item(id)?
            };
            Ok(placed(editor, result))
        })(),
        "update_storyboard_library_item" => (|| {
            let id = args["item"].as_u64().unwrap();
            let name = args["name"].as_str().unwrap();
            let tags = tag_list(args);
            if personal_scope(args) {
                personal::rename(root, id, name, tags.as_deref()).map_err(|e| e.to_string())?;
            } else {
                editor.edit_storyboard(|b| b.library.rename(id, name, tags.as_deref()))?;
            }
            Ok(json!({"item":id}))
        })(),
        "remove_storyboard_library_item" => (|| {
            let id = args["item"].as_u64().unwrap();
            if personal_scope(args) {
                personal::remove(root, id).map_err(|e| e.to_string())?;
            } else {
                editor.edit_storyboard(|b| b.library.remove(id).map(|_| ()))?;
            }
            Ok(json!({"removed":id}))
        })(),
        "list_storyboard_templates" => (|| {
            let query = args["query"].as_str().unwrap_or("");
            let catalog = load()?;
            let mut list = Vec::new();
            for asset in templates(&catalog).filter(|a| matches(&a.name, &a.tags, query)) {
                let mut entry = json!({"template":asset.id,"name":asset.name,"tags":asset.tags,"author":asset.attribution});
                if let Some(board) = emulsion_io::project::read(&asset.path)
                    .ok()
                    .and_then(|p| p.storyboard)
                {
                    entry["width"] = json!(board.settings.width);
                    entry["height"] = json!(board.settings.height);
                    entry["frame_rate"] = json!(super::rate_label(board.settings.frame_rate));
                    entry["panels"] = json!(board.panels.len());
                }
                list.push(entry);
            }
            Ok(json!({"templates":list}))
        })(),
        "save_storyboard_template" => (|| {
            let project = editor
                .snapshot()
                .ok_or("Open a storyboard project first.")?;
            let mut manifest = template_pack::Manifest::new(
                template_pack::Kind::Storyboard,
                args["name"].as_str().unwrap().trim().into(),
            );
            manifest.tags = tag_list(args).unwrap_or_default();
            for (field, key) in [
                (&mut manifest.author, "author"),
                (&mut manifest.license, "license"),
                (&mut manifest.description, "description"),
            ] {
                *field = args[key].as_str().unwrap_or("").trim().into();
            }
            let pack = template_pack::pack(&project, &manifest, None).map_err(|e| e.to_string())?;
            let (_, id) = template_pack::install(root, pack).map_err(|e| e.to_string())?;
            Ok(json!({"template":id}))
        })(),
        _ => return None,
    };
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{board, call};
    use super::super::{execute, validate_args};
    use super::*;

    fn root(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "emulsion-mcp-storyboard-library-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    /// Run a tool with the personal library under `root`.
    fn call_in(
        root: &Path,
        e: &mut ProjectEditor,
        name: &str,
        args: Value,
    ) -> Result<Value, String> {
        validate_args(name, &args)?;
        let board = e.storyboard().unwrap().clone();
        run_in(root, e, &board, name, &args).unwrap()
    }

    /// Add a "Hero" layer to the active panel; returns its ID.
    fn hero(e: &mut ProjectEditor) -> u64 {
        use emulsion_core::{Command, Node, NodeKind, command::Slot};
        let node = Node::new(
            0,
            "Hero",
            NodeKind::Fill {
                rgba: [200, 40, 40, 255],
            },
        );
        e.execute(Command::AddNode {
            node: Box::new(node),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap()
    }

    #[test]
    fn project_items_add_place_rename_and_remove_as_single_steps() {
        let mut e = board();
        let hero = hero(&mut e);
        let added = call(
            &mut e,
            "add_to_storyboard_library",
            json!({"name":"Paper","tags":["set"],"layers":[hero]}),
        );
        assert_eq!(added["kind"], "layers");
        let item = added["item"].as_u64().unwrap();
        let panel = call(
            &mut e,
            "add_to_storyboard_library",
            json!({"name":"Opening"}),
        );
        assert_eq!(panel["kind"], "panel");
        let listed = call(
            &mut e,
            "list_storyboard_library",
            json!({"scope":"project","query":"set"}),
        );
        assert_eq!(listed["project"].as_array().unwrap().len(), 1);
        assert!(listed.get("personal").is_none());

        let layers = e.doc.nodes.len();
        let placed = call(
            &mut e,
            "place_storyboard_library_item",
            json!({"item":item}),
        );
        assert_eq!(placed["placed"], "layers");
        assert_eq!(e.doc.nodes.len(), layers + 1);
        assert!(e.undo());
        assert_eq!(e.doc.nodes.len(), layers);

        let pages = e.page_list().len();
        let placed = call(
            &mut e,
            "place_storyboard_library_item",
            json!({"item":panel["item"]}),
        );
        assert_eq!(e.page_list().len(), pages + 1);
        assert_eq!(e.active_page(), placed["panel"].as_u64().unwrap());
        assert!(e.undo());
        assert_eq!(e.page_list().len(), pages);

        call(
            &mut e,
            "update_storyboard_library_item",
            json!({"item":item,"name":"Sheet","tags":[]}),
        );
        let library = &e.storyboard().unwrap().library;
        assert_eq!(library.item(item).unwrap().name, "Sheet");
        assert!(library.item(item).unwrap().tags.is_empty());
        call(
            &mut e,
            "remove_storyboard_library_item",
            json!({"item":item}),
        );
        assert!(e.storyboard().unwrap().library.item(item).is_none());
        assert!(e.undo());
        assert!(e.storyboard().unwrap().library.item(item).is_some());
    }

    #[test]
    fn invalid_library_calls_change_nothing() {
        let mut e = board();
        hero(&mut e);
        call(
            &mut e,
            "add_to_storyboard_library",
            json!({"name":"Opening"}),
        );
        let stamp = e.stamp();
        for (name, args) in [
            ("add_to_storyboard_library", json!({})),
            ("add_to_storyboard_library", json!({"name":""})),
            ("add_to_storyboard_library", json!({"name":"x","panel":99})),
            (
                "add_to_storyboard_library",
                json!({"name":"x","layers":[999]}),
            ),
            (
                "add_to_storyboard_library",
                json!({"name":"x","scope":"team"}),
            ),
            ("place_storyboard_library_item", json!({"item":99})),
            (
                "update_storyboard_library_item",
                json!({"item":99,"name":"x"}),
            ),
            (
                "update_storyboard_library_item",
                json!({"item":1,"name":" "}),
            ),
            ("remove_storyboard_library_item", json!({"item":99})),
            ("list_storyboard_library", json!({"scope":"team"})),
        ] {
            assert!(execute(&mut e, name, &args).is_error, "{name} {args}");
            assert_eq!(e.stamp(), stamp, "{name}");
        }
    }

    #[test]
    fn personal_items_and_templates_live_outside_the_project() {
        let root = root("personal");
        let mut e = board();
        let layer = hero(&mut e);
        let added = call_in(
            &root,
            &mut e,
            "add_to_storyboard_library",
            json!({"scope":"personal","name":"Paper","layers":[layer],"tags":["prop"]}),
        )
        .unwrap();
        let id = added["item"].as_u64().unwrap();
        // Personal items are not project edits.
        let stamp = e.stamp();
        let listed = call_in(
            &root,
            &mut e,
            "list_storyboard_library",
            json!({"query":"PROP"}),
        )
        .unwrap();
        assert_eq!(listed["personal"][0]["name"], "Paper");
        assert_eq!(listed["project"].as_array().unwrap().len(), 0);

        // Another storyboard places it, as one Undo step.
        let mut other = board();
        let before = other.doc.nodes.len();
        call_in(
            &root,
            &mut other,
            "place_storyboard_library_item",
            json!({"scope":"personal","item":id}),
        )
        .unwrap();
        assert_eq!(other.doc.nodes.len(), before + 1);
        assert!(other.undo());
        assert_eq!(other.doc.nodes.len(), before);

        assert!(
            call_in(
                &root,
                &mut e,
                "update_storyboard_library_item",
                json!({"scope":"personal","item":id + 50,"name":"x"})
            )
            .is_err()
        );
        call_in(
            &root,
            &mut e,
            "update_storyboard_library_item",
            json!({"scope":"personal","item":id,"name":"Sheet"}),
        )
        .unwrap();
        call_in(
            &root,
            &mut e,
            "remove_storyboard_library_item",
            json!({"scope":"personal","item":id}),
        )
        .unwrap();
        assert_eq!(e.stamp(), stamp);
        let listed = call_in(
            &root,
            &mut e,
            "list_storyboard_library",
            json!({"scope":"personal"}),
        )
        .unwrap();
        assert!(listed["personal"].as_array().unwrap().is_empty());

        // Templates.
        let saved = call_in(
            &root,
            &mut e,
            "save_storyboard_template",
            json!({"name":"Pilot","tags":["tv"]}),
        )
        .unwrap();
        let template = saved["template"].as_u64().unwrap();
        let listed = call_in(
            &root,
            &mut e,
            "list_storyboard_templates",
            json!({"query":"tv"}),
        )
        .unwrap();
        assert_eq!(listed["templates"][0]["template"], template);
        assert_eq!(listed["templates"][0]["panels"], e.page_list().len());
        let path = template_path(&root, template).unwrap();
        // The template carries the board but not its identity: each
        // storyboard made from it gets a new project ID.
        let copy = emulsion_io::project::read(&path)
            .unwrap()
            .storyboard
            .unwrap();
        let board = e.storyboard().unwrap();
        assert_ne!(copy.project_id, board.project_id);
        assert_eq!(
            emulsion_core::storyboard::Storyboard {
                project_id: board.project_id.clone(),
                ..copy
            },
            *board
        );
        assert!(template_path(&root, template + 99).is_err());
        assert_eq!(e.stamp(), stamp);
        std::fs::remove_dir_all(root).unwrap();
    }
}
