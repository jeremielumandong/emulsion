//! Reproducible source-only Smart Object PSD/PSB and native-history evidence.
//!
//! cargo run -p emulsion-io --example psd_smart_source_interchange -- NEW_DIRECTORY
//! python3 scripts/verify_psd_smart_source_interchange.py NEW_DIRECTORY
//!
//! Uses only the pinned MIT psd-tools fixture. The original embedded PNG is
//! retained byte-for-byte; neither layer previews nor a saved merged composite
//! prove original-source preservation. No Photoshop application/Smart Filter
//! interoperability is claimed by this source-only check.

use emulsion_core::{
    Document, EmptyVectorCoverage, NodeKind, VectorMask, graph::Graph, node::OriginalImage,
};
use emulsion_raster::{Mask, Placement, Raster, vector::Path as VectorPath};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{error::Error, path::Path, sync::Arc};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const FIXTURE_SHA256: &str = "a34abf773a64f21b59d5a854f7d394e7ac41e402d0ce546ddd86d19c764a63a2";
const PNG_SHA256: &str = "582ae5daa72a495b9d8fcee8593d5253f62ea268c357d0816b3bd00b5065267c";
const SOURCE_POINTS: [(f64, f64); 4] = [(2.0, 3.0), (29.0, 3.0), (29.0, 28.0), (2.0, 28.0)];

fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn smart(doc: &Document) -> (&Arc<Raster>, &Arc<OriginalImage>, Placement) {
    assert_eq!(doc.nodes.len(), 1);
    let NodeKind::Smart {
        source,
        original_image: Some(original),
        placement,
        editable,
        filters,
        filter_styles,
        filters_enabled,
        filter_mask,
        cache,
        offset,
    } = &doc.nodes[0].kind
    else {
        panic!("The public reader must retain the embedded Smart source")
    };
    assert!(
        editable.is_none()
            && *filters_enabled
            && filters.is_empty()
            && filter_styles.is_empty()
            && filter_mask.is_none()
    );
    assert_eq!(*offset, (0, 0));
    assert_eq!(
        (source.width(), source.height()),
        (cache.width(), cache.height())
    );
    assert_eq!(source.fill(), cache.fill());
    assert_eq!(source.to_pixels(), cache.to_pixels());
    (source, original, placement.legacy().unwrap())
}
fn native_pixels(source: &Raster) -> Vec<u8> {
    source
        .to_pixels()
        .into_iter()
        .flatten()
        .flat_map(u16::to_be_bytes)
        .collect()
}
fn native_digest(source: &Raster) -> String {
    let mut hash = Sha256::new();
    hash.update(b"Emulsion OriginalImage native-source v1\0");
    hash.update(source.width().to_be_bytes());
    hash.update(source.height().to_be_bytes());
    hash.update(native_pixels(source));
    hex(&hash.finalize())
}
fn same_document(actual: &Document, expected: &Document) {
    // Document equality intentionally compares pixel/source Arcs by identity.
    // A fresh native read owns new allocations: verify every payload exactly
    // before aligning those identities for the remaining document metadata.
    let (source, original, _) = smart(actual);
    let (expected_source, expected_original, _) = smart(expected);
    assert_eq!(
        (source.width(), source.height()),
        (expected_source.width(), expected_source.height())
    );
    assert_eq!(source.fill(), expected_source.fill());
    assert_eq!(source.to_pixels(), expected_source.to_pixels());
    assert_eq!(original.bytes(), expected_original.bytes());
    assert_eq!(
        original.encoded_sha256(),
        expected_original.encoded_sha256()
    );
    assert_eq!(original.source_sha256(), expected_original.source_sha256());
    assert_eq!(
        actual.nodes[0].mask.as_ref().map(|mask| (
            (mask.width(), mask.height()),
            mask.fill(),
            mask.to_gray8()
        )),
        expected.nodes[0].mask.as_ref().map(|mask| (
            (mask.width(), mask.height()),
            mask.fill(),
            mask.to_gray8()
        ))
    );
    let mut aligned = actual.clone();
    aligned.nodes[0].mask = expected.nodes[0].mask.clone();
    if let NodeKind::Smart {
        source,
        original_image,
        ..
    } = &mut aligned.nodes[0].kind
    {
        *source = expected_source.clone();
        *original_image = Some(expected_original.clone());
    }
    assert_eq!(aligned, *expected, "Native document metadata changed");
}
fn same_graph(actual: &Graph, expected: &Graph) {
    assert_eq!(actual.head(), expected.head());
    assert_eq!(actual.branches(), expected.branches());
    assert_eq!(actual.len(), expected.len());
    for (a, b) in actual.commits().zip(expected.commits()) {
        assert_eq!(
            (a.id, &a.parents, &a.name, a.time, a.auto, &a.branch),
            (b.id, &b.parents, &b.name, b.time, b.auto, &b.branch)
        );
        same_document(&a.doc, &b.doc);
    }
}
fn file(out: &Path, name: &str) -> Result<Value> {
    Ok(json!({"file": name, "sha256": sha256(&std::fs::read(out.join(name))?)}))
}
fn check_roundtrip(actual: &Document, expected: &Document) {
    let (source, original, placement) = smart(actual);
    let (expected_source, expected_original, expected_placement) = smart(expected);
    assert_eq!(native_digest(source), native_digest(expected_source));
    assert_eq!(original.bytes(), expected_original.bytes());
    assert_eq!(placement, expected_placement);
    let (node, target) = (&actual.nodes[0], &expected.nodes[0]);
    assert_eq!(
        node.mask.as_ref().map(|m| m.to_gray8()),
        target.mask.as_ref().map(|m| m.to_gray8())
    );
    assert_eq!(
        node.mask
            .as_ref()
            .map(|m| (m.width(), m.height(), m.fill())),
        target
            .mask
            .as_ref()
            .map(|m| (m.width(), m.height(), m.fill()))
    );
    assert_eq!(
        (
            node.mask_enabled,
            node.mask_linked,
            node.mask_transform,
            node.mask_properties
        ),
        (
            target.mask_enabled,
            target.mask_linked,
            target.mask_transform,
            target.mask_properties
        )
    );
    let (vector, wanted) = (
        node.vector_mask.as_ref().unwrap(),
        target.vector_mask.as_ref().unwrap(),
    );
    assert_eq!(
        (
            vector.enabled,
            vector.linked,
            vector.inverted,
            vector.properties,
            vector.empty_coverage
        ),
        (
            wanted.enabled,
            wanted.linked,
            wanted.inverted,
            wanted.properties,
            wanted.empty_coverage
        )
    );
    assert_eq!(vector.path.subpaths.len(), 1);
    assert!(vector.path.subpaths[0].closed);
    assert_eq!(vector.path.subpaths[0].anchors.len(), SOURCE_POINTS.len());
    let [a, b, c, d, tx, ty] = vector.transform;
    for (anchor, (x, y)) in vector.path.subpaths[0].anchors.iter().zip(SOURCE_POINTS) {
        assert!(!anchor.smooth);
        for p in [anchor.h_in, anchor.p, anchor.h_out] {
            assert!((a * p.0 + c * p.1 + tx - x).abs() <= 64.0 / f64::from(1 << 24));
            assert!((b * p.0 + d * p.1 + ty - y).abs() <= 48.0 / f64::from(1 << 24));
        }
    }
}
fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let out = std::path::PathBuf::from(args.next().ok_or("Pass a new output directory")?);
    if args.next().is_some() {
        return Err("Unexpected argument".into());
    }
    // Deliberately refuses even an existing empty directory: never overwrite evidence.
    std::fs::create_dir(&out)?;
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psd");
    let fixture = fixture_dir.join("smartobject-layer.psd");
    let fixture_bytes = std::fs::read(&fixture)?;
    assert_eq!(
        sha256(&fixture_bytes),
        FIXTURE_SHA256,
        "Pinned MIT fixture changed"
    );
    let imported = emulsion_io::psd::read(&fixture)?;
    let imported_before = imported.clone();
    let (source, original, _) = smart(&imported);
    assert_eq!((source.width(), source.height()), (32, 32));
    assert_eq!(sha256(original.bytes()), PNG_SHA256);
    assert_eq!(native_digest(source), hex(original.source_sha256()));
    let decoded = image::load_from_memory(original.bytes())?.into_rgba8();
    let hidden_rgb = decoded
        .pixels()
        .filter(|p| p[3] == 0 && p.0[..3].iter().any(|v| *v != 0))
        .count();
    let partial_alpha = decoded.pixels().filter(|p| p[3] > 0 && p[3] < 255).count();
    assert!(hidden_rgb > 0 && partial_alpha > 0);
    assert_ne!(
        source.to_srgba8(),
        *decoded.as_raw(),
        "Fixture must detect an original re-encode"
    );
    std::fs::write(out.join("input-smartobject-layer.psd"), fixture_bytes)?;
    std::fs::copy(
        fixture_dir.join("LICENSE.psd-tools"),
        out.join("LICENSE.psd-tools"),
    )?;
    std::fs::write(out.join("original.png"), original.bytes().as_slice())?;
    std::fs::write(out.join("source-native.rgba16be"), native_pixels(source))?;
    image::save_buffer(
        out.join("native-reencoded.png"),
        &source.to_srgba8(),
        32,
        32,
        image::ColorType::Rgba8,
    )?;
    let mask = Arc::new(Mask::from_fn(36, 38, 255, |x, y| {
        ((x * 7 + y * 11) % 256) as u8
    }));
    image::save_buffer(
        out.join("ordinary-raster-mask.png"),
        &mask.to_gray8(),
        36,
        38,
        image::ColorType::L8,
    )?;
    let mut cases = Vec::new();
    for (label, enabled, linked, inverted, raster_enabled, raster_linked) in [
        ("enabled-unlinked", true, false, false, true, true),
        ("disabled-linked", false, true, false, true, false),
        ("inverted-unlinked", true, false, true, false, true),
    ] {
        let mut doc = imported.clone();
        doc.width = 64;
        doc.height = 48;
        let node = &mut doc.nodes[0];
        node.name = format!("Smart source {label}");
        if let NodeKind::Smart { placement, .. } = &mut node.kind {
            *placement = emulsion_core::SmartPlacement::Legacy(Placement::at(11.0, 7.0));
        }
        node.mask = Some(mask.clone());
        // Deliberately differ from vector flags to catch cross-mask conflation.
        node.mask_enabled = raster_enabled;
        node.mask_linked = raster_linked;
        node.mask_transform = emulsion_core::Mapping2::Affine(glam::DAffine2::from_cols_array(&[
            1.0, 0.0, 0.0, 1.0, -2.0, -3.0,
        ]));
        node.vector_mask = Some(VectorMask {
            path: Arc::new(VectorPath::from_svg("M 2 3 L 29 3 L 29 28 L 2 28 Z")?),
            empty_coverage: EmptyVectorCoverage::HideAll,
            enabled,
            linked,
            inverted,
            ..VectorMask::default()
        });
        doc.validate()?;
        let baseline = doc.clone();
        let mut graph = Graph::new(imported.clone(), "Pinned original import");
        assert!(
            graph
                .record(&doc, "Translated source and independent masks", false)
                .is_some()
        );
        let graph_before = graph.clone();
        let before_name = format!("{label}-before.ora");
        let after_name = format!("{label}-after.ora");
        emulsion_io::ora::write_full(&doc, Some(&graph), &out.join(&before_name))?;
        let opened = emulsion_io::ora::read_full(&out.join(&before_name))?;
        assert!(opened.history_error.is_none());
        same_document(&opened.doc, &doc);
        let reopened_graph = opened.graph.as_ref().ok_or("Native history missing")?;
        same_graph(reopened_graph, &graph);
        let mut exports = Vec::new();
        for (extension, version) in [("psd", 1), ("psb", 2)] {
            let name = format!("{label}.{extension}");
            let path = out.join(&name);
            let report = emulsion_io::psd::write_with_report(&opened.doc, &path)?;
            assert_eq!(
                report.appearance_fallback, None,
                "Source-only export fell back"
            );
            assert!(!report.baked_raster_masks, "Ordinary mask was baked");
            check_roundtrip(&emulsion_io::psd::read(&path)?, &opened.doc);
            same_document(&opened.doc, &baseline);
            same_graph(reopened_graph, &graph_before);
            let mut entry = file(&out, &name)?;
            entry["version"] = json!(version);
            entry["appearance_fallback"] = Value::Null;
            entry["baked_raster_masks"] = json!(false);
            exports.push(entry);
        }
        emulsion_io::ora::write_full(&opened.doc, Some(reopened_graph), &out.join(&after_name))?;
        assert_eq!(doc, baseline, "Native IO changed the input document/source");
        same_graph(&graph, &graph_before);
        cases.push(json!({
            "name": label, "layer_name": doc.nodes[0].name,
            "placement": [11, 7], "source_bounds": [11, 7, 43, 39],
            "raster_mask": {"file": "ordinary-raster-mask.png", "pixel_sha256": sha256(&mask.to_gray8()),
                "size": [36, 38], "bounds": [9, 4, 45, 42], "fill": 255,
                "transform": [1, 0, 0, 1, -2, -3], "enabled": raster_enabled, "linked": raster_linked},
            "vector_mask": {"enabled": enabled, "linked": linked, "inverted": inverted,
                "initial_fill": 0, "source_points": SOURCE_POINTS,
                "document_points": SOURCE_POINTS.map(|(x, y)| [x + 11.0, y + 7.0])},
            "native_before": file(&out, &before_name)?, "native_after": file(&out, &after_name)?,
            "native_history_commits": graph.len(), "exports": exports,
        }));
    }
    assert_eq!(imported, imported_before, "Original import changed");
    std::fs::write(
        out.join("manifest.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1, "kind": "source-only-smart-interchange", "canvas": [64, 48],
            "scope": "Exact embedded PNG, placed identity, ordinary raster/vector masks and native history; no Photoshop application or Smart Filter support claim",
            "input": file(&out, "input-smartobject-layer.psd")?,
            "provenance": "psd-tools MIT fixture at d68bf46c7140a1f8c74be9c10b4e21103e820761; LICENSE.psd-tools",
            "source": {"file": "original.png", "encoded_sha256": PNG_SHA256, "size": [32, 32],
                "native_file": "source-native.rgba16be", "native_source_sha256": native_digest(source),
                "native_reencoded_png": "native-reencoded.png", "hidden_rgb_pixels": hidden_rgb,
                "partial_alpha_pixels": partial_alpha},
            "input_document_source_history_unchanged": true, "cases": cases,
        }))?,
    )?;
    println!(
        "Wrote source-only Smart PSD/PSB, exact original PNG, native history and manifest to {}",
        out.display()
    );
    Ok(())
}
