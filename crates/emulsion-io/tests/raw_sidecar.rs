#[path = "common/raw_fixture.rs"]
mod raw_fixture;

use emulsion_core::{Command, Document, Node, NodeKind, raw::DevelopParams};
use emulsion_io::{raw::RawSource, raw_settings};
use std::{path::PathBuf, sync::Arc};

struct Fixture(PathBuf);
impl Fixture {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("emulsion-sidecar-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        raw_fixture::write_dng(&path.join("camera.dng"));
        Self(path)
    }
    fn original(&self) -> PathBuf {
        self.0.join("camera.dng")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for file in std::fs::read_dir(&self.0).unwrap() {
            let _ = std::fs::remove_file(file.unwrap().path());
        }
        let _ = std::fs::remove_dir(&self.0);
    }
}
fn pixels(doc: &Document) -> Vec<u8> {
    emulsion_raster::composite::flatten(&doc.composite_tree(), 0).to_srgba8()
}

#[test]
fn original_reopens_saved_recipe_but_native_project_keeps_its_own() {
    let fixture = Fixture::new("roundtrip");
    let original = fixture.original();
    let digest = emulsion_io::raw::source_digest(&original).unwrap();
    let mut doc = emulsion_io::open(&original).unwrap();
    assert_eq!(doc.raw.as_ref().unwrap().params, DevelopParams::default());
    assert!(raw_settings::sidecar_only(&doc));
    let unedited = pixels(&doc);
    let project = fixture.0.join("unedited.ora");
    emulsion_io::save(&doc, &project).unwrap();
    let params = DevelopParams {
        exposure: -0.7,
        temperature: 0.3,
        tint: -0.1,
        wb_override: Some([2.1, 1.0, 1.4, 1.0]),
        tone_curve: DevelopParams::MEDIUM_CONTRAST_CURVE,
        ..Default::default()
    };
    let source = RawSource::load(&original).unwrap();
    Command::DevelopRaw {
        id: doc.raw.as_ref().unwrap().node_id,
        raster: Arc::new(source.develop_with(&params).unwrap()),
        params,
    }
    .apply(&mut doc)
    .unwrap();
    assert!(raw_settings::sidecar_only(&doc));
    let expected = pixels(&doc);
    assert_ne!(expected, unedited);
    let sidecar = raw_settings::suggested_sidecar_path(&doc).unwrap();
    raw_settings::save_sidecar(&doc, &sidecar).unwrap();
    let restored = emulsion_io::open(&original).unwrap();
    assert_eq!(restored.raw.as_ref().unwrap().params, params);
    assert_eq!(pixels(&restored), expected);
    assert!(raw_settings::sidecar_only(&restored));
    let native = emulsion_io::open(&project).unwrap();
    assert_eq!(
        native.raw.as_ref().unwrap().params,
        DevelopParams::default()
    );
    assert_eq!(pixels(&native), unedited);
    assert_eq!(emulsion_io::raw::source_digest(&original).unwrap(), digest);
    // Saving again replaces the recipe atomically, not the original.
    raw_settings::save_sidecar(&native, &sidecar).unwrap();
    assert_eq!(pixels(&emulsion_io::open(&original).unwrap()), unedited);
}

#[test]
fn invalid_and_mismatched_adjacent_recipes_are_actionable_errors() {
    let fixture = Fixture::new("invalid");
    let original = fixture.original();
    let doc = emulsion_io::open(&original).unwrap();
    let path = raw_settings::suggested_sidecar_path(&doc).unwrap();
    raw_settings::save_sidecar(&doc, &path).unwrap();
    let valid = std::fs::read(&path).unwrap();
    for bytes in [
        b"broken JSON".to_vec(),
        {
            let mut json: serde_json::Value = serde_json::from_slice(&valid).unwrap();
            json["source_sha256"] = serde_json::Value::String("0".repeat(64));
            serde_json::to_vec(&json).unwrap()
        },
        {
            let mut json: serde_json::Value = serde_json::from_slice(&valid).unwrap();
            json["params"]["exposure"] = serde_json::json!(99);
            serde_json::to_vec(&json).unwrap()
        },
    ] {
        std::fs::write(&path, &bytes).unwrap();
        let error = emulsion_io::open(&original).unwrap_err().to_string();
        assert!(error.contains("could not restore"), "{error}");
        assert!(error.contains("move it aside"), "{error}");
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn sidecar_save_eligibility_rejects_unrepresented_project_edits() {
    let fixture = Fixture::new("eligibility");
    let doc = emulsion_io::open(&fixture.original()).unwrap();
    assert!(raw_settings::sidecar_only(&doc));
    let changes: [fn(&mut Document); 10] = [
        |d| d.width -= 1,
        |d| d.nodes[0].opacity = 0.5,
        |d| d.nodes[0].name = "Renamed".into(),
        |d| d.nodes[0].locked = true,
        |d| d.nodes[0].visible = false,
        |d| d.resolution = 300.0,
        |d| {
            d.guides.push(emulsion_core::document::Guide {
                vertical: true,
                pos: 1.0,
            })
        },
        |d| {
            if let NodeKind::Raster { placement, .. } = &mut d.nodes[0].kind {
                placement.rotation = 90.0;
            }
        },
        |d| {
            d.nodes
                .push(Node::new(99, "Extra", NodeKind::Fill { rgba: [255; 4] }))
        },
        |d| d.raw = None,
    ];
    for change in changes {
        let mut edited = doc.clone();
        change(&mut edited);
        assert!(!raw_settings::sidecar_only(&edited));
    }
}
