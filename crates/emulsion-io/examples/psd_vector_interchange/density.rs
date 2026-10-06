//! A separate packet: the original thirteen vector descriptors stay intact.
use super::*;
use emulsion_core::graph::Graph;
use emulsion_raster::blend::BlendSpace;

fn fixture() -> Result<Document> {
    let mut doc = Document::new(128, 80);
    doc.blend_space = BlendSpace::PhotoshopSrgbV1;
    add(
        &mut doc,
        Node::raster(
            0,
            "Density background",
            Arc::new(Raster::from_srgba8(
                128,
                80,
                &[24, 34, 49, 255].repeat(128 * 80),
            )),
            Placement::default(),
        ),
        None,
    )?;
    let lower = 0.9f32;
    let upper = f32::from_bits(lower.to_bits() + 1);
    for (index, (name, raster_density, vector_density)) in [
        ("Raster below half", lower, None),
        ("Raster above half", upper, None),
        ("Carried lower raster upper vector", lower, Some(upper)),
        ("Carried upper raster lower vector", upper, Some(lower)),
        ("Exact half carried", 0.5, Some(0.5)),
        ("Byte grid carried", 153.0 / 255.0, Some(204.0 / 255.0)),
    ]
    .into_iter()
    .enumerate()
    {
        let mut node = raster(
            name,
            (52, 38),
            ((index % 3 * 40) as f64, (index / 3 * 36) as f64),
            20 + index as u32,
        );
        carrier(&mut node, false);
        node.mask_properties.density = raster_density;
        if let Some(value) = vector_density {
            attach(&mut node, rectangle());
            node.vector_mask.as_mut().unwrap().properties.density = value;
        }
        add(&mut doc, node, None)?;
    }
    Ok(doc)
}

fn assert_history(actual: &Graph, before: &Graph) {
    assert_eq!(actual.head(), before.head());
    assert_eq!(actual.branches(), before.branches());
    assert_eq!(actual.len(), before.len());
    for (a, b) in actual.commits().zip(before.commits()) {
        assert_eq!(
            (a.id, &a.parents, &a.name, a.time, a.auto, &a.branch),
            (b.id, &b.parents, &b.name, b.time, b.auto, &b.branch)
        );
        // Includes source/mask Arc identity and exact native density values.
        assert_eq!(a.doc, b.doc, "Export changed a native history snapshot");
    }
}

fn file(out: &Path, name: &str) -> Result<Value> {
    Ok(json!({"file": name, "sha256": sha256(&std::fs::read(out.join(name))?)}))
}

pub(super) fn write_packet(out: &Path) -> Result<()> {
    std::fs::create_dir(out)?;
    let doc = fixture()?;
    doc.validate()?;
    assert!(!emulsion_io::psd::needs_appearance_fallback(&doc));
    assert!(!emulsion_io::psd::has_baked_raster_masks(&doc));
    let before = doc.clone();
    let mut initial = doc.clone();
    for node in &mut initial.nodes {
        node.mask_properties.density = 1.0;
        if let Some(vector) = &mut node.vector_mask {
            vector.properties.density = 1.0;
        }
    }
    let mut graph = Graph::new(initial, "Before authored densities");
    assert!(
        graph
            .record(&doc, "Exact native boundary densities", false)
            .is_some()
    );
    let graph_before = graph.clone();
    emulsion_io::ora::write_full(&doc, Some(&graph), &out.join("source-before.ora"))?;

    // This reference uses the exported byte-grid values only. The native
    // document/history and their unrounded f32 bits remain separate evidence.
    let mut represented = doc.clone();
    for node in &mut represented.nodes {
        if node.mask.is_some() {
            node.mask_properties.density =
                f32::from(density_byte(node.mask_properties.density)) / 255.0;
        }
        if let Some(vector) = &mut node.vector_mask {
            vector.properties.density = f32::from(density_byte(vector.properties.density)) / 255.0;
        }
    }
    let mut layers = Vec::new();
    for (index, (native, encoded)) in doc.nodes.iter().zip(&represented.nodes).enumerate() {
        let mut entry = reference(out, &represented, encoded, index)?;
        if native.mask.is_some() {
            density_metadata(&mut entry["raster_mask"], native.mask_properties.density);
        }
        if let Some(vector) = &native.vector_mask {
            density_metadata(&mut entry["vector_mask"], vector.properties.density);
        }
        if let NodeKind::Raster { raster, .. } = &native.kind {
            let name = format!("layer-{index:02}-native.rgba16be");
            let words: Vec<u8> = raster
                .to_pixels()
                .into_iter()
                .flatten()
                .flat_map(u16::to_be_bytes)
                .collect();
            std::fs::write(out.join(&name), words)?;
            entry["native_source"] = file(out, &name)?;
        }
        layers.push(entry);
    }
    let represented_pixels = flatten(&represented.composite_tree(), 0).to_srgba8();
    let native_pixels = flatten(&doc.composite_tree(), 0).to_srgba8();
    assert_ne!(
        represented_pixels, native_pixels,
        "Fixture must expose rounding in the preview"
    );
    let composite = png(
        out,
        "represented-composite.png".into(),
        (doc.width, doc.height),
        image::ColorType::Rgba8,
        &represented_pixels,
    )?;
    let native_composite = png(
        out,
        "native-unrounded-composite.png".into(),
        (doc.width, doc.height),
        image::ColorType::Rgba8,
        &native_pixels,
    )?;
    let mut exports = Vec::new();
    for (extension, version) in [("psd", 1), ("psb", 2)] {
        let name = format!("density-boundaries.{extension}");
        let report = emulsion_io::psd::write_with_report(&doc, &out.join(&name))?;
        assert_eq!(report.appearance_fallback, None, "Density packet fell back");
        assert!(!report.baked_raster_masks, "Independent masks were baked");
        assert_eq!(report.rounded_mask_densities, 8);
        assert_eq!(doc, before, "Density export changed native document/source");
        assert_history(&graph, &graph_before);
        let mut entry = file(out, &name)?;
        entry["version"] = json!(version);
        entry["write_report"] = json!({
            "appearance_fallback": report.appearance_fallback.map(|value| format!("{value:?}")),
            "baked_raster_masks": report.baked_raster_masks,
            "rounded_mask_densities": report.rounded_mask_densities,
        });
        exports.push(entry);
    }
    emulsion_io::ora::write_full(&doc, Some(&graph), &out.join("source-after.ora"))?;
    assert_eq!(doc, before);
    assert_history(&graph, &graph_before);
    std::fs::write(
        out.join("manifest.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1, "kind": "independent-density-boundary-interchange",
            "blend_space": doc.blend_space,
            "canvas": [doc.width, doc.height], "layers": layers, "exports": exports,
            "sibling_order": [{"parent": null, "bottom_to_top": doc.children(None).into_iter().map(|id| &doc.node(id).unwrap().name).collect::<Vec<_>>()}],
            "native_archives": [file(out, "source-before.ora")?, file(out, "source-after.ora")?],
            "native_document_unchanged": true, "native_history_commits": graph.len(),
            "composite": composite, "native_unrounded_composite": native_composite,
            "component_reference": "Byte-grid represented document; native densities are recorded as exact f32 bits",
        }))?,
    )?;
    Ok(())
}
