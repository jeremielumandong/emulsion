#[path = "common/raw_fixture.rs"]
mod raw_fixture;

use emulsion_core::{Command, raw::DevelopParams};
use emulsion_io::raw::RawSource;
use std::sync::Arc;

#[test]
fn synthetic_raw_edit_reopen_export_and_missing_original() {
    let directory =
        std::env::temp_dir().join(format!("emulsion-raw-roundtrip-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    // Deliberately omit a RAW extension to exercise content-based import.
    let original = directory.join("sensor.data");
    let project = directory.join("edited.ora");
    let output = directory.join("edited.png");
    let missing_output = directory.join("missing.png");
    raw_fixture::write_dng(&original);
    let original_bytes = std::fs::read(&original).unwrap();
    let mut doc = emulsion_io::open(&original).unwrap();
    assert_eq!(doc.source_depth, 16);
    let raw = doc.raw.as_ref().unwrap();
    let id = raw.node_id;
    assert_eq!(raw.metadata.sensor, "Bayer");
    let source = RawSource::load(&original).unwrap();
    let params = DevelopParams {
        exposure: -1.0,
        temperature: 0.3,
        tint: -0.1,
        highlights: 0.2,
        shadows: 0.1,
        black_point: 0.005,
        brightness: 0.2,
        contrast: 0.15,
        saturation: -0.1,
        tone_curve: DevelopParams::MEDIUM_CONTRAST_CURVE,
        ..Default::default()
    };
    let edited = source.develop_with(&params).unwrap();
    let expected = edited.to_srgba8();
    assert_ne!(
        expected,
        source
            .develop_with(&DevelopParams::default())
            .unwrap()
            .to_srgba8()
    );
    Command::DevelopRaw {
        id,
        raster: Arc::new(edited),
        params,
    }
    .apply(&mut doc)
    .unwrap();
    assert_eq!(doc.raw.as_ref().unwrap().params, params);
    emulsion_io::save(&doc, &project).unwrap();
    assert_eq!(std::fs::read(&original).unwrap(), original_bytes);
    let reopened = emulsion_io::open(&project).unwrap();
    assert_eq!(reopened.raw, doc.raw);
    assert_eq!((reopened.width, reopened.height), (36, 24));
    emulsion_io::export(
        &reopened,
        &output,
        emulsion_io::ExportOptions {
            depth: 8,
            jpeg_quality: 92,
        },
    )
    .unwrap();
    let exported = image::open(&output).unwrap().to_rgba8();
    assert_eq!(exported.dimensions(), (36, 24));
    assert_eq!(exported.as_raw(), &expected);
    assert_eq!(std::fs::read(&original).unwrap(), original_bytes);
    drop(source);
    // Remove only this test-generated source. The native project keeps pixels.
    std::fs::remove_file(&original).unwrap();
    let offline = emulsion_io::open(&project).unwrap();
    assert_eq!(offline.raw, reopened.raw);
    assert_eq!(
        emulsion_raster::composite::flatten(&offline.composite_tree(), 0).to_srgba8(),
        expected
    );
    assert!(
        emulsion_io::export(
            &offline,
            &missing_output,
            emulsion_io::ExportOptions {
                depth: 8,
                jpeg_quality: 92
            }
        )
        .is_err()
    );
    assert!(!missing_output.exists());
    std::fs::remove_file(project).unwrap();
    std::fs::remove_file(output).unwrap();
    std::fs::remove_dir(directory).unwrap();
}
