use super::*;
use crate::raw_fixture;
use emulsion_core::Editor;

#[test]
fn liked_edits_save_export_and_reapply_as_presets() {
    let dir = std::env::temp_dir().join(format!("emulsion-mcp-presets-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("source.dng");
    raw_fixture::write_dng(&path);
    let mut editor = Editor::new(emulsion_io::raw::open(&path).unwrap(), None);
    let doc = editor.doc.clone();

    // Nothing to save before an edit, and arguments are checked.
    assert!(save(&doc, &json!({"name":"x","library":false,"export_xmp":true})).is_err());
    assert!(save(&doc, &json!({"name":" "})).is_err());
    assert!(save(&doc, &json!({"name":"x","bogus":1})).is_err());

    let p = crate::raw_tools::plan(
        &editor.doc,
        "apply_raw_look",
        &json!({"look":"cinematic-teal-orange"}),
    )
    .unwrap();
    assert!(!crate::exec::apply(&mut editor, p).is_error);
    let graded = editor.doc.raw.as_ref().unwrap().params;

    let xmp = dir.join("Teal Orange.xmp");
    let result = save(
        &editor.doc,
        &json!({"name":"Teal Orange","library":false,"export_xmp":xmp}),
    )
    .unwrap();
    let text: Value = serde_json::from_str(result.content[0]["text"].as_str().unwrap()).unwrap();
    assert!(text["library_preset"].is_null());
    assert!(xmp.is_file());
    assert!(
        save(
            &editor.doc,
            &json!({"name":"Teal Orange","library":false,"export_xmp":xmp})
        )
        .is_err()
    );
    assert!(
        save(
            &editor.doc,
            &json!({"name":"x","library":false,"export_xmp":"a.png"})
        )
        .is_err()
    );

    // Reapply the exported preset to the reset photo: the look returns, one undo step.
    editor.undo();
    let before = editor.doc.clone();
    let p = plan(&editor.doc, &json!({"path":xmp})).unwrap();
    assert!(!crate::exec::apply(&mut editor, p).is_error);
    let reapplied = editor.doc.raw.as_ref().unwrap().params;
    assert!((reapplied.contrast - graded.contrast).abs() < 0.006);
    assert!((reapplied.grading[0][1] - graded.grading[0][1]).abs() < 0.006);
    assert_eq!(
        reapplied.exposure,
        before.raw.as_ref().unwrap().params.exposure
    );
    editor.undo();
    assert_eq!(editor.doc, before);

    let p = plan(&editor.doc, &json!({"path":xmp,"strength":0})).unwrap();
    assert!(p.commands.is_empty());
    for args in [
        json!({}),
        json!({"name":"a","path":"b"}),
        json!({"name":"no such preset anywhere"}),
        json!({"path":xmp,"strength":3}),
    ] {
        assert!(plan(&editor.doc, &args).is_err(), "{args}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}
