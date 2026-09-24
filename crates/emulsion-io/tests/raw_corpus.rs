//! Opt-in real-camera smoke/roundtrip tests. Sources are CC0; see the manifest.
//! EMULSION_RAW_CORPUS must contain the three named, hash-verified source files.
use emulsion_io::{
    raw::{DevelopParams, RawSource},
    raw_probe,
};
use std::{path::PathBuf, sync::Arc};

#[test]
#[ignore = "requires the CC0 raw-corpus.json downloads; runs full-resolution development"]
fn real_camera_open_edit_reopen_export() {
    let directory =
        PathBuf::from(std::env::var_os("EMULSION_RAW_CORPUS").expect("set EMULSION_RAW_CORPUS"));
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/raw-corpus.json")).unwrap();
    for fixture in fixtures.as_array().unwrap() {
        let name = fixture["file"].as_str().unwrap();
        let path = directory.join(name);
        let started = std::time::Instant::now();
        assert_eq!(
            emulsion_io::raw::source_digest(&path).unwrap(),
            fixture["sha256"].as_str().unwrap()
        );
        assert!(raw_probe::is_raw(&path).unwrap(), "{name} detection");
        let source = RawSource::load(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(source.metadata.model, fixture["model"].as_str().unwrap());
        assert!(source.metadata.bits_per_sample >= 12);
        let params = DevelopParams {
            exposure: -0.75,
            temperature: 0.15,
            highlights: 0.2,
            black_point: 0.002,
            brightness: 0.1,
            contrast: 0.1,
            saturation: -0.1,
            tone_curve: DevelopParams::MEDIUM_CONTRAST_CURVE,
            ..Default::default()
        };
        let raster = source
            .develop_with(&params)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let size = (raster.width(), raster.height());
        assert!(size.0 > 1000 && size.1 > 1000);
        let pixels = raster.to_srgba8();
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[0] > 10 && p[0] < 245)
        );
        let mut doc = emulsion_core::Document::new(size.0, size.1);
        emulsion_core::Command::AddNode {
            node: Box::new(emulsion_core::Node::raster(
                0,
                name,
                Arc::new(raster),
                Default::default(),
            )),
            slot: emulsion_core::command::Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
        doc.source_depth = 16;
        doc.raw = Some(emulsion_core::raw::RawDocument {
            schema_version: 1,
            node_id: doc.nodes[0].id,
            source: source.source.clone(),
            source_sha256: source.source_sha256.clone(),
            params,
            metadata: source.metadata.clone(),
        });
        drop(source);
        let project = directory.join(format!("{name}.test.ora"));
        let export = directory.join(format!("{name}.test.png"));
        emulsion_io::save(&doc, &project).unwrap();
        let reopened = emulsion_io::open(&project).unwrap();
        assert_eq!(reopened.raw, doc.raw);
        drop(doc);
        emulsion_io::export(
            &reopened,
            &export,
            emulsion_io::ExportOptions {
                depth: 8,
                jpeg_quality: 92,
            },
        )
        .unwrap();
        let exported = image::open(&export).unwrap().to_rgba8();
        assert_eq!(exported.dimensions(), size);
        assert_eq!(
            exported.as_raw(),
            &pixels,
            "preview/export differ for {name}"
        );
        assert_eq!(
            emulsion_io::raw::source_digest(&path).unwrap(),
            fixture["sha256"].as_str().unwrap()
        );
        eprintln!(
            "{name}: {size:?}, {:?}, {:?}, elapsed {:?}",
            reopened.raw.as_ref().unwrap().metadata.sensor,
            reopened.raw.as_ref().unwrap().metadata.compression,
            started.elapsed()
        );
        std::fs::remove_file(project).unwrap();
        std::fs::remove_file(export).unwrap();
    }
}
