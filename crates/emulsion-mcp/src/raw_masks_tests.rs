use super::*;
use crate::raw_fixture;
use emulsion_core::Editor;

fn editor(tag: &str) -> (Editor, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("emulsion-mcp-masks-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("source.dng");
    raw_fixture::write_dng(&path);
    (
        Editor::new(emulsion_io::raw::open(&path).unwrap(), None),
        dir,
    )
}

fn masks(editor: &Editor) -> LocalEdits {
    current_edits(&editor.doc.raw.as_ref().unwrap().params).unwrap()
}

#[test]
fn view_coordinates_follow_crop_and_quarter_turns() {
    let p = DevelopParams {
        crop: [0.2, 0.1, 0.8, 0.9],
        ..DevelopParams::default()
    };
    assert_eq!(view_to_source(&p, [0., 0.]), [0.2, 0.1]);
    assert_eq!(view_to_source(&p, [1., 1.]), [0.8, 0.9]);
    let turned = DevelopParams {
        rotation: 1,
        ..DevelopParams::default()
    };
    // A clockwise quarter turn puts the source's top-left at the view's top-right.
    assert_eq!(view_to_source(&turned, [1., 0.]), [0., 0.]);
    assert_eq!(view_radius(&turned, [0.2, 0.4]), [0.4, 0.2]);
}

#[test]
fn manual_masks_combine_invert_adjust_and_undo() {
    let (mut editor, dir) = editor("manual");
    let before = editor.doc.clone();
    let p = plan(
        &editor.doc,
        &json!({"action":"add","name":"Edges","components":[
            {"shape":"radial","center":[0.5,0.5],"radius":0.4,"invert":true},
            {"shape":"luminance","range":[0.2,1.0],"operation":"intersect"}],
            "adjustments":{"exposure":-0.4,"highlights":-0.2,"shadows":0.1}}),
    )
    .unwrap();
    assert!(!crate::exec::apply(&mut editor, p).is_error);
    assert_eq!(editor.history.len(), 1);
    let edits = masks(&editor);
    let mask = &edits.masks[0];
    assert_eq!(mask.components.len(), 3);
    assert_eq!(mask.components[0].shape, Shape::All);
    assert_eq!(mask.components[1].operation, Operation::Subtract);
    assert_eq!(
        (mask.exposure, mask.highlights, mask.shadows),
        (-0.4, -0.2, 0.1)
    );
    // The inverted radial leaves the centre untouched and selects the corner.
    let bitmaps = develop_edits::bitmaps(&edits).unwrap();
    let bright = [0.5; 3];
    assert_eq!(
        develop_edits::weight(mask, [0.5, 0.5], bright, &bitmaps),
        0.
    );
    assert!(develop_edits::weight(mask, [0.02, 0.02], bright, &bitmaps) > 0.9);

    let listed = list(&editor.doc, &json!({"overlay":true})).unwrap();
    assert!(listed.content.iter().any(|b| b["type"] == "image"));

    let p = plan(
        &editor.doc,
        &json!({"action":"update","id":mask.id,"adjustments":{"exposure":-0.2},"name":"Soft edges"}),
    )
    .unwrap();
    assert!(!crate::exec::apply(&mut editor, p).is_error);
    assert_eq!(masks(&editor).masks[0].exposure, -0.2);
    assert_eq!(masks(&editor).masks[0].highlights, -0.2);

    for args in [
        json!({"action":"add","name":"x","components":[{"shape":"radial","radius":0}]}),
        json!({"action":"add","name":"x","components":[{"shape":"color","color":"plaid"}]}),
        json!({"action":"add","name":"x","components":[{"shape":"radial"},{"shape":"radial","invert":true}]}),
        json!({"action":"add","name":"x","components":[{"shape":"radial"}],"adjustments":{"clarity":1}}),
        json!({"action":"update","id":99}),
        json!({"action":"paint"}),
    ] {
        assert!(plan(&editor.doc, &args).is_err(), "{args}");
    }
    let p = plan(&editor.doc, &json!({"action":"clear"})).unwrap();
    assert!(!crate::exec::apply(&mut editor, p).is_error);
    assert!(
        editor
            .doc
            .raw
            .as_ref()
            .unwrap()
            .params
            .local_edits
            .is_none()
    );
    while !editor.history.is_empty() {
        editor.undo();
    }
    assert_eq!(editor.doc, before);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn strategies_are_idempotent_and_fall_back_without_models() {
    let (mut editor, dir) = editor("auto");
    let p = plan_auto(
        &editor.doc,
        &json!({"strategies":["vignette_focus","tonal_balance","color_separation","subject_pop"]}),
    )
    .unwrap();
    let message: Value = serde_json::from_str(&p.message).unwrap();
    assert!(!crate::exec::apply(&mut editor, p).is_error);
    let first = masks(&editor).masks.len();
    assert_eq!(first, 6, "{message}");
    if emulsion_ai::matte::available().is_none() {
        assert!(message["notes"].to_string().contains("approximation"));
    }
    let p = plan_auto(
        &editor.doc,
        &json!({"strategies":["vignette_focus"],"strength":0.5}),
    )
    .unwrap();
    assert!(!crate::exec::apply(&mut editor, p).is_error);
    let edits = masks(&editor);
    assert_eq!(edits.masks.len(), first);
    let vignette = edits
        .masks
        .iter()
        .find(|m| m.name == "Auto · Vignette focus")
        .unwrap();
    assert!((vignette.exposure + 0.15).abs() < 1e-6);

    // Eye masks need the face detector; auto skips them with a reason instead of failing.
    if emulsion_ai::face::detector_available().is_none() {
        let p = plan_auto(&editor.doc, &json!({"strategies":["eyes"]}));
        let skipped = match p {
            Ok(p) => serde_json::from_str::<Value>(&p.message).unwrap()["skipped"].clone(),
            Err(_) => json!(null),
        };
        assert!(skipped.to_string().contains("face detector"), "{skipped}");
        assert!(
            plan(
                &editor.doc,
                &json!({"action":"add","name":"e","components":[{"shape":"eyes"}]})
            )
            .is_err()
        );
    }
    let p = plan_auto(&editor.doc, &json!({"strategies":["auto"]})).unwrap();
    assert!(!crate::exec::apply(&mut editor, p).is_error);
    assert!(plan_auto(&editor.doc, &json!({"strategies":["glow"]})).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn no_model_fallbacks_follow_the_visible_frame() {
    if emulsion_ai::models::installed_for(emulsion_ai::models::Task::Sky).is_some()
        || emulsion_ai::matte::available().is_some()
    {
        return;
    }
    let (editor, dir) = editor("fallback");
    let bitmaps = Default::default();
    let bright = [0.6; 3];
    let weight = |parts: &[Component], xy| {
        let mask = Mask {
            id: 1,
            name: "t".into(),
            enabled: true,
            components: parts.to_vec(),
            exposure: 0.,
            contrast: 0.,
            saturation: 0.,
            temperature: 0.,
            tint: 0.,
            highlights: 0.,
            shadows: 0.,
        };
        develop_edits::weight(&mask, xy, bright, &bitmaps)
    };
    // Upside down, the visible top is the source bottom.
    let flipped = DevelopParams {
        rotation: 2,
        ..DevelopParams::default()
    };
    let sky = region("sky", &editor.doc, &flipped, &mut Regions::default()).unwrap();
    assert!(weight(&sky, [0.5, 0.9]) > weight(&sky, [0.5, 0.1]));
    // With an off-centre crop, the subject radial sits in the visible frame's centre.
    let cropped = DevelopParams {
        crop: [0.5, 0., 1., 1.],
        ..DevelopParams::default()
    };
    let subject = region("subject", &editor.doc, &cropped, &mut Regions::default()).unwrap();
    assert!(weight(&subject, [0.75, 0.5]) > 0.99);
    assert_eq!(weight(&subject, [0.3, 0.5]), 0.);
    std::fs::remove_dir_all(dir).unwrap();
}
