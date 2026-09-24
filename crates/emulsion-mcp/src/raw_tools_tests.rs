use super::*;
use crate::raw_fixture;
use emulsion_core::Editor;

#[test]
fn raw_discovery_and_development_preserve_the_layer_stack_and_original() {
    let dir =
        std::env::temp_dir().join(format!("emulsion-mcp-raw-workflow-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("source.dng");
    raw_fixture::write_dng(&path);
    let original = std::fs::read(&path).unwrap();
    let mut editor = Editor::new(emulsion_io::raw::open(&path).unwrap(), None);
    let before = editor.doc.clone();
    let node_id = before.raw.as_ref().unwrap().node_id;
    let description = crate::exec::describe(&editor);
    assert_eq!(description["raw"]["node_id"], node_id);
    assert_eq!(description["raw"]["settings"]["exposure"], 0.0);
    assert_eq!(editor.doc, before);
    assert_eq!(editor.history.len(), 0);

    let inspected = crate::exec::execute(&mut editor, "describe_raw", &json!({}));
    assert!(!inspected.is_error);
    let developed = crate::exec::execute(
        &mut editor,
        "develop_raw",
        &json!({"settings":{"exposure":0.5,"contrast":0.2,"saturation":-1.0}}),
    );
    assert!(!developed.is_error, "{developed:?}");
    assert_eq!(editor.doc.nodes.len(), before.nodes.len());
    assert_eq!(editor.doc.nodes[0].id, node_id);
    assert_eq!(editor.doc.nodes[0].name, before.nodes[0].name);
    let params = editor.doc.raw.as_ref().unwrap().params;
    assert_eq!(params.exposure, 0.5);
    assert_eq!(params.contrast, 0.2);
    assert_eq!(params.saturation, -1.0);
    assert_eq!(
        params.temperature,
        before.raw.as_ref().unwrap().params.temperature
    );
    assert_eq!(
        crate::exec::describe(&editor)["raw"]["settings"],
        json!(params)
    );
    let view = crate::exec::execute(&mut editor, "get_view", &json!({}));
    assert!(!view.is_error);
    assert!(view.content.iter().any(|block| block["type"] == "image"));
    assert_eq!(editor.history.len(), 1);
    editor.undo();
    assert_eq!(editor.doc, before);
    assert_eq!(std::fs::read(&path).unwrap(), original);

    // Camera provenance alone is not evidence of an editable RAW recipe.
    editor.doc.raw = None;
    assert!(!editor.doc.raw_originals.is_empty());
    assert!(crate::exec::describe(&editor)["raw"].is_null());
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn raw_tools_roundtrip_validation_and_deferred_save() {
    let dir = std::env::temp_dir().join(format!("emulsion-mcp-raw-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("source.dng");
    raw_fixture::write_dng(&path);
    let mut editor = Editor::new(emulsion_io::raw::open(&path).unwrap(), None);
    let before = editor.doc.clone();
    let mut locked = Editor::new(before.clone(), None);
    locked
        .execute(Command::SetLocked {
            id: before.raw.as_ref().unwrap().node_id,
            locked: true,
        })
        .unwrap();
    let locked_before = locked.doc.clone();
    let p = plan(
        &locked.doc,
        "develop_raw",
        &json!({"settings":{"exposure":0.5}}),
    )
    .unwrap();
    assert!(crate::exec::apply(&mut locked, p).is_error);
    assert_eq!(locked.doc, locked_before);
    for args in [
        json!({"settings":{"exposure":4}}),
        json!({"settings":{"unknown":1}}),
        json!({"settings":{"tone_curve":[0.,0.7,0.4,0.8,1.]}}),
        json!({"settings":{"tint":null}}),
        json!({"settings":{"wb_override":[1.,1.]}}),
    ] {
        assert!(plan(&editor.doc, "develop_raw", &args).is_err());
        assert_eq!(before, editor.doc);
    }
    let p = plan(&editor.doc,"develop_raw",&json!({"settings":{"exposure":0.5,"brightness":0.2,"saturation":-0.1},"curve_preset":"medium"})).unwrap();
    assert_eq!(p.commands.len(), 1);
    assert!(!crate::exec::apply(&mut editor, p).is_error);
    let edited = editor.doc.clone();
    assert_eq!(
        edited.raw.as_ref().unwrap().params.tone_curve,
        DevelopParams::MEDIUM_CONTRAST_CURVE
    );
    editor.undo();
    assert_eq!(editor.doc, before);
    editor.redo();
    assert_eq!(editor.doc, edited);
    let sidecar = dir.join("settings.json");
    let args = json!({"action":"save_sidecar","path":sidecar});
    let p = plan(&editor.doc, "raw_settings", &args).unwrap();
    assert!(!sidecar.exists());
    editor.undo();
    assert!(crate::exec::apply(&mut editor, p).is_error);
    assert!(!sidecar.exists());
    editor.redo();
    let p = plan(&editor.doc, "raw_settings", &args).unwrap();
    assert!(!crate::exec::apply(&mut editor, p).is_error);
    let p = plan(&editor.doc, "reset_raw", &json!({})).unwrap();
    assert!(!crate::exec::apply(&mut editor, p).is_error);
    let p = plan(
        &editor.doc,
        "raw_settings",
        &json!({"action":"load_sidecar","path":sidecar,"group":"tone"}),
    )
    .unwrap();
    assert!(!crate::exec::apply(&mut editor, p).is_error);
    let params = editor.doc.raw.as_ref().unwrap().params;
    assert_eq!(params.exposure, 0.5);
    assert_eq!(params.tone_curve, DevelopParams::LINEAR_CURVE);
    assert!(
        plan(
            &editor.doc,
            "pick_raw_white_balance",
            &json!({"x":-1,"y":0})
        )
        .is_err()
    );
    assert!(
        plan(
            &editor.doc,
            "pick_raw_white_balance",
            &json!({"x":9000,"y":0})
        )
        .is_err()
    );
    let p = plan(&editor.doc, "auto_develop_raw", &json!({})).unwrap();
    assert!(!crate::exec::apply(&mut editor, p).is_error);
    let p = plan(
        &editor.doc,
        "pick_raw_white_balance",
        &json!({"x":18,"y":12}),
    )
    .unwrap();
    assert!(!crate::exec::apply(&mut editor, p).is_error);
    assert!(
        editor
            .doc
            .raw
            .as_ref()
            .unwrap()
            .params
            .wb_override
            .is_some()
    );
    assert!(describe(&editor.doc, &json!({})).is_ok());
    let preset = dir.join("preset.json");
    let p = plan(
        &editor.doc,
        "raw_settings",
        &json!({"action":"save_preset","path":preset}),
    )
    .unwrap();
    assert!(!crate::exec::apply(&mut editor, p).is_error);
    let mut other_camera = editor.doc.clone();
    other_camera.raw.as_mut().unwrap().metadata.model = "Different camera".into();
    assert!(
        plan(
            &other_camera,
            "raw_settings",
            &json!({"action":"load_preset","path":preset,"group":"white_balance"})
        )
        .is_err()
    );
    assert!(
        plan(
            &other_camera,
            "raw_settings",
            &json!({"action":"load_preset","path":preset,"group":"tone"})
        )
        .is_ok()
    );
    let source_bytes = std::fs::read(&path).unwrap();
    let p = plan(
        &editor.doc,
        "raw_settings",
        &json!({"action":"save_sidecar","path":path}),
    )
    .unwrap();
    assert!(crate::exec::apply(&mut editor, p).is_error);
    assert_eq!(std::fs::read(&path).unwrap(), source_bytes);
    let moved = dir.join("moved.dng");
    std::fs::copy(&path, &moved).unwrap();
    let p = plan(&editor.doc, "relink_raw", &json!({"path":moved})).unwrap();
    assert!(!crate::exec::apply(&mut editor, p).is_error);
    assert_eq!(
        editor.doc.raw.as_ref().unwrap().source,
        std::fs::canonicalize(&moved).unwrap()
    );
    std::fs::remove_file(sidecar).unwrap();
    std::fs::remove_file(preset).unwrap();
    std::fs::remove_file(moved).unwrap();
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn save_document_uses_adjacent_raw_sidecar_without_replacing_original() {
    let dir = std::env::temp_dir().join(format!("emulsion-mcp-save-raw-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("source.dng");
    raw_fixture::write_dng(&path);
    let original = std::fs::read(&path).unwrap();
    let mut editor = Editor::new(emulsion_io::raw::open(&path).unwrap(), None);
    let result = crate::exec::execute(
        &mut editor,
        "develop_raw",
        &json!({"settings":{"exposure":0.75,"temperature":0.2}}),
    );
    assert!(!result.is_error);
    assert!(editor.is_modified());
    let sidecar = emulsion_io::raw_settings::suggested_sidecar_path(&editor.doc).unwrap();
    assert!(!sidecar.exists());
    assert!(!crate::exec::execute(&mut editor, "save_document", &json!({})).is_error);
    assert!(sidecar.exists());
    assert!(editor.path.is_none());
    assert!(!editor.is_modified());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let reopened = emulsion_io::open(&path).unwrap();
    assert_eq!(
        reopened.raw.as_ref().unwrap().params,
        editor.doc.raw.as_ref().unwrap().params
    );
    editor.undo();
    assert!(editor.is_modified());
    editor.redo();
    assert!(!editor.is_modified());

    let mut versioned = Editor::new(editor.doc.clone(), None);
    let result = crate::exec::execute(
        &mut versioned,
        "develop_raw",
        &json!({"settings":{"exposure":0.5}}),
    );
    assert!(!result.is_error);
    versioned.create_version("Alternate exposure");
    assert!(crate::exec::execute(&mut versioned, "save_document", &json!({})).is_error);

    let saved_bytes = std::fs::read(&sidecar).unwrap();
    assert!(!crate::exec::execute(&mut editor, "add_layer", &json!({"name":"Extra"})).is_error);
    assert!(crate::exec::execute(&mut editor, "save_document", &json!({})).is_error);
    assert!(editor.is_modified());
    assert_eq!(std::fs::read(&sidecar).unwrap(), saved_bytes);
    assert!(crate::exec::execute(&mut editor, "save_document", &json!({"path":path})).is_error);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let project = dir.join("project.ora");
    assert!(!crate::exec::execute(&mut editor, "save_document", &json!({"path":project})).is_error);
    assert_eq!(editor.path.as_ref(), Some(&project));
    assert!(!crate::exec::execute(&mut editor, "save_document", &json!({})).is_error);
    assert_eq!(std::fs::read(&sidecar).unwrap(), saved_bytes);
    std::fs::remove_file(project).unwrap();
    std::fs::remove_file(sidecar).unwrap();
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn raw_schemas_and_missing_source_errors() {
    for def in definitions() {
        assert_eq!(def.input_schema["additionalProperties"], false);
    }
    let doc = Document::new(2, 2);
    assert!(describe(&doc, &json!({})).is_err());
    for name in HEAVY {
        assert!(crate::tools::HEAVY.contains(name));
        assert!(plan(&doc, name, &json!({})).is_err());
    }
}
