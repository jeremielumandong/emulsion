#[path = "common/raw_fixture.rs"]
mod raw_fixture;

use emulsion_core::{Command, raw::DevelopParams};
use emulsion_io::{
    export::{ExportOptions, ExportWorkflow, export_with_workflow},
    raw::RawSource,
};
use std::sync::Arc;

#[test]
fn full_size_workflow_matches_saved_raw_preview_at_both_depths() {
    let directory = std::env::temp_dir().join(format!(
        "emulsion-export-raw-workflow-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let original = directory.join("sensor.dng");
    raw_fixture::write_dng(&original);
    let original_bytes = std::fs::read(&original).unwrap();
    let mut doc = emulsion_io::open(&original).unwrap();
    let id = doc.raw.as_ref().unwrap().node_id;
    let source = RawSource::load(&original).unwrap();
    let params = DevelopParams {
        exposure: -0.75,
        temperature: 0.2,
        tint: -0.1,
        black_point: 0.005,
        brightness: 0.15,
        contrast: 0.2,
        saturation: -0.2,
        tone_curve: DevelopParams::MEDIUM_CONTRAST_CURVE,
        wb_override: Some([2.0, 1.0, 1.5, 1.0]),
        ..Default::default()
    };
    let raster = source.develop_with(&params).unwrap();
    let expected8 = raster.to_srgba8();
    let expected16 = raster.to_srgba16();
    Command::DevelopRaw {
        id,
        raster: Arc::new(raster),
        params,
    }
    .apply(&mut doc)
    .unwrap();
    let project = directory.join("edited.ora");
    emulsion_io::save(&doc, &project).unwrap();
    let reopened = emulsion_io::open(&project).unwrap();
    assert_eq!(reopened.raw.as_ref().unwrap().params, params);
    for depth in [8, 16] {
        let output = directory.join(format!("edited-{depth}.png"));
        export_with_workflow(
            &reopened,
            &output,
            ExportOptions {
                depth,
                jpeg_quality: 92,
            },
            ExportWorkflow::default(),
        )
        .unwrap();
        let image = image::open(&output).unwrap();
        if depth == 8 {
            assert_eq!(image.to_rgba8().as_raw(), &expected8);
        } else {
            assert_eq!(image.to_rgba16().as_raw(), &expected16);
        }
        std::fs::remove_file(output).unwrap();
    }
    assert_eq!(std::fs::read(&original).unwrap(), original_bytes);
    drop(source);
    std::fs::remove_file(original).unwrap();
    std::fs::remove_file(project).unwrap();
    std::fs::remove_dir(directory).unwrap();
}
