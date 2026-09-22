use super::*;
use emulsion_core::Editor;
use std::path::PathBuf;

use crate::raw_fixture;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let mut nonce = [0; 16];
        getrandom::fill(&mut nonce).unwrap();
        let path = std::env::temp_dir().join(format!(
            "emulsion-mcp-raw-preview-{:032x}.dng",
            u128::from_le_bytes(nonce)
        ));
        raw_fixture::write_dng(&path);
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn image(result: ToolResult) -> image::RgbaImage {
    assert!(!result.is_error, "{:?}", result.content);
    let block = result
        .content
        .iter()
        .find(|v| v["type"] == "image")
        .unwrap();
    assert_eq!(block["mimeType"], "image/png");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(block["data"].as_str().unwrap())
        .unwrap();
    image::load_from_memory(&bytes).unwrap().into_rgba8()
}

fn developed(doc: &Document, params: DevelopParams) -> Document {
    let raw = doc.raw.as_ref().unwrap();
    let source = RawSource::load_verified(&raw.source, &raw.source_sha256).unwrap();
    let mut result = doc.clone();
    Command::DevelopRaw {
        id: raw.node_id,
        raster: Arc::new(source.develop_with(&params).unwrap()),
        params,
    }
    .apply(&mut result)
    .unwrap();
    result
}

#[test]
fn raw_preview_modes_show_exact_split_and_section_bypasses_without_edits() {
    let fixture = Fixture::new();
    let original_bytes = std::fs::read(&fixture.0).unwrap();
    let original = emulsion_io::raw::open(&fixture.0).unwrap();
    let params = DevelopParams {
        exposure: -0.7,
        brightness: 0.2,
        contrast: 0.3,
        saturation: -0.1,
        tone_curve: DevelopParams::STRONG_CONTRAST_CURVE,
        ..Default::default()
    };
    let edited_doc = developed(&original, params);
    let mut editor = Editor::new(edited_doc.clone(), None);
    let revision = editor.revision;
    let as_shot = image(crate::preview::view(&original, &json!({"max_size":64})).unwrap());
    let edited = image(crate::exec::execute(
        &mut editor,
        "get_raw_preview",
        &json!({"mode":"edited","max_size":64}),
    ));
    assert_ne!(edited, as_shot);
    assert_eq!(edited.dimensions(), (original.width, original.height));

    for position in [0.0, 0.25, 0.5, 1.0] {
        let split = image(crate::exec::execute(
            &mut editor,
            "get_raw_preview",
            &json!({"mode":"split","position":position,"max_size":64}),
        ));
        let boundary = (split.width() as f32 * position as f32).round() as u32;
        for (x, y, pixel) in split.enumerate_pixels() {
            let expected = if x < boundary { &as_shot } else { &edited };
            assert_eq!(
                pixel,
                expected.get_pixel(x, y),
                "position {position}, ({x},{y})"
            );
        }
    }

    for (mode, group) in [
        ("without_tone", RawSettingsGroup::Tone),
        ("without_curve", RawSettingsGroup::Curve),
    ] {
        let bypass = image(crate::exec::execute(
            &mut editor,
            "get_raw_preview",
            &json!({"mode":mode,"max_size":64}),
        ));
        let expected_doc = developed(
            &edited_doc,
            merge_settings(params, DevelopParams::default(), group),
        );
        let expected = image(crate::preview::view(&expected_doc, &json!({"max_size":64})).unwrap());
        assert_eq!(bypass, expected, "{mode}");
        assert_ne!(
            bypass, edited,
            "{mode} should visibly bypass the adjusted section"
        );
    }
    assert_eq!(editor.doc, edited_doc);
    assert_eq!(editor.revision, revision);
    assert_eq!(std::fs::read(&fixture.0).unwrap(), original_bytes);
}

#[test]
fn raw_clipping_proof_marks_output_highlights_and_leaves_history_unchanged() {
    let fixture = Fixture::new();
    let original = emulsion_io::raw::open(&fixture.0).unwrap();
    let doc = developed(
        &original,
        DevelopParams {
            exposure: 3.0,
            ..Default::default()
        },
    );
    let mut editor = Editor::new(doc.clone(), None);
    let revision = editor.revision;
    let edited = image(crate::exec::execute(
        &mut editor,
        "get_raw_preview",
        &json!({"mode":"edited"}),
    ));
    let clipped = image(crate::exec::execute(
        &mut editor,
        "get_raw_preview",
        &json!({"mode":"clipping"}),
    ));
    assert_ne!(clipped, edited);
    assert!(clipped.pixels().any(|p| p.0 == [255, 0, 0, 255]));
    assert!(clipped.pixels().all(|p| p.0 == [255, 0, 0, 255]
        || p.0 == [0, 0, 255, 255]
        || (p[0] == p[1] && p[1] == p[2])));
    assert_eq!(editor.doc, doc);
    assert_eq!(editor.revision, revision);
    editor.undo();
    assert_eq!(editor.doc, doc);
}

#[test]
fn raw_preview_rejects_unknown_and_invalid_arguments_and_non_raw_documents() {
    let fixture = Fixture::new();
    let doc = emulsion_io::raw::open(&fixture.0).unwrap();
    for args in [
        json!({"mode":"original"}),
        json!({"mode":null}),
        json!({"position":-0.1}),
        json!({"position":1.01}),
        json!({"position":"half"}),
        json!({"max_size":63}),
        json!({"max_size":1569}),
        json!({"max_size":64.5}),
        json!({"unknown":true}),
        json!([]),
    ] {
        assert!(preview(&doc, &args).is_err(), "{args}");
    }
    let non_raw = Document::new(12, 12);
    for mode in [
        "edited",
        "split",
        "without_tone",
        "without_curve",
        "clipping",
    ] {
        assert!(preview(&non_raw, &json!({"mode":mode})).is_err());
    }
}

#[test]
fn raw_workspace_preview_definitions_cover_all_four_tools() {
    let defs = definitions();
    assert_eq!(defs.len(), 4);
    for name in [
        "get_raw_preview",
        "set_raw_comparison",
        "list_raw_documents",
        "synchronize_raw",
    ] {
        let definition = defs.iter().find(|d| d.name == name).unwrap();
        assert_eq!(definition.input_schema["additionalProperties"], false);
    }
    assert_eq!(
        defs[0].input_schema["properties"]["max_size"]["maximum"],
        1568
    );
    assert_eq!(defs[1].input_schema["properties"]["position"]["maximum"], 1);
    assert_eq!(defs[3].input_schema["required"], json!(["targets"]));
}
