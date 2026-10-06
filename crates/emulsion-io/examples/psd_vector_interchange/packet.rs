//! Evidence orchestration only. The authored fixture and production admission
//! rules remain separate from this explicitly labeled format-precision control.
use super::*;
use emulsion_io::psd::AppearanceFallback;

fn file(out: &Path, name: &str) -> Result<Value> {
    Ok(json!({"file": name, "sha256": sha256(&std::fs::read(out.join(name))?)}))
}

fn cpu_pixels(doc: &Document) -> Vec<u8> {
    use emulsion_raster::{TILE, TileCoord, color, composite::render_tile_cpu};
    let tree = doc.composite_tree();
    let mut raster = Raster::transparent(doc.width, doc.height);
    for y in 0..doc.height.div_ceil(TILE) {
        for x in 0..doc.width.div_ceil(TILE) {
            let coordinate = TileCoord::new(x as i32, y as i32);
            raster.set_tile(
                coordinate,
                render_tile_cpu(&tree, 0, coordinate)
                    .into_iter()
                    .map(color::f_to_px)
                    .collect(),
            );
        }
    }
    raster.to_srgba8()
}

fn retained_scene(original: &Document, ids: &[u64]) -> Result<Document> {
    let mut doc = original.clone();
    doc.nodes.retain(|node| ids.contains(&node.id));
    doc.validate()?;
    for node in &doc.nodes {
        assert_eq!(Some(node), original.node(node.id));
    }
    Ok(doc)
}

// Independent standard 8.24 reconstruction, not a production encoder call.
// Keep document-space points plus the inverse layer-origin transform, as the
// format's editable import represents them. Never replace the original mask.
fn canonical_mask(doc: &Document, node: &Node) -> Result<(VectorMask, Vec<i32>)> {
    const SCALE: f64 = 16_777_216.0;
    let mut mask = node.vector_mask.clone().ok_or("Missing original cubic")?;
    let NodeKind::Raster { placement, .. } = &node.kind else {
        return Err("Cubic control requires a raster source".into());
    };
    let [a, b, c, d, tx, ty] = mask.transform;
    let mut path = (*mask.path).clone();
    let mut words = Vec::new();
    for subpath in &mut path.subpaths {
        for anchor in &mut subpath.anchors {
            for point in [&mut anchor.h_in, &mut anchor.p, &mut anchor.h_out] {
                let (x, y) = *point;
                let world = [
                    a * x + c * y + tx + placement.x,
                    b * x + d * y + ty + placement.y,
                ];
                let dimensions = [f64::from(doc.width), f64::from(doc.height)];
                let stored: [i32; 2] = std::array::from_fn(|axis| {
                    let normalized = world[axis] / dimensions[axis];
                    assert!((-16.0..16.0).contains(&normalized));
                    (normalized * SCALE).round() as i32
                });
                words.extend_from_slice(&[stored[1], stored[0]]);
                *point = (
                    f64::from(stored[0]) / SCALE * dimensions[0],
                    f64::from(stored[1]) / SCALE * dimensions[1],
                );
            }
        }
    }
    mask.path = Arc::new(path);
    mask.transform = [1.0, 0.0, 0.0, 1.0, -placement.x, -placement.y];
    Ok((mask, words))
}

fn write_case(
    out: &Path,
    id: &str,
    doc: &Document,
    expected: Option<AppearanceFallback>,
    control: bool,
) -> Result<Value> {
    let folder = out.join(id);
    std::fs::create_dir(&folder)?;
    doc.validate()?;
    assert!(!emulsion_io::psd::has_baked_raster_masks(doc));
    let before = doc.clone();
    emulsion_io::ora::write(doc, &folder.join("source-before.ora"))?;
    let layers = doc
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| reference(&folder, doc, node, index))
        .collect::<Result<Vec<_>>>()?;
    let pixels = cpu_pixels(doc);
    let composite = png(
        &folder,
        "native-composite.png".into(),
        (doc.width, doc.height),
        image::ColorType::Rgba8,
        &pixels,
    )?;
    let mut exports = Vec::new();
    for (extension, version) in [("psd", 1), ("psb", 2)] {
        let name = format!("scene.{extension}");
        let report = emulsion_io::psd::write_with_report(doc, &folder.join(&name))?;
        assert_eq!(
            report.appearance_fallback, expected,
            "{id}: admission changed"
        );
        assert!(
            !report.baked_raster_masks,
            "{id}: independent mask was baked"
        );
        assert_eq!(report.rounded_mask_densities, 0);
        assert_eq!(doc, &before, "{id}: export mutated native source");
        let (reopened, read_report) = emulsion_io::psd::read_with_report(&folder.join(&name))?;
        let reopened_pixels = cpu_pixels(&reopened);
        assert_eq!(pixels, reopened_pixels, "{id}: reopened appearance changed");
        if expected.is_some() {
            assert_eq!(reopened.nodes.len(), 1);
            assert!(reopened.nodes.iter().all(|node| node.vector_mask.is_none()));
        } else {
            assert_eq!(reopened.nodes.len(), doc.nodes.len());
            for node in &doc.nodes {
                let back = reopened
                    .nodes
                    .iter()
                    .find(|back| back.name == node.name)
                    .ok_or("Layered reopen lost a named source")?;
                assert_eq!(back.vector_mask.is_some(), node.vector_mask.is_some());
                if control {
                    assert_eq!(back.vector_mask, node.vector_mask);
                }
                if let NodeKind::Raster { raster, placement } = &node.kind {
                    let NodeKind::Raster {
                        raster: actual,
                        placement: at,
                    } = &back.kind
                    else {
                        return Err("Layered reopen changed source kind".into());
                    };
                    assert_eq!(actual.to_pixels(), raster.to_pixels());
                    assert_eq!(at, placement);
                }
            }
        }
        let mut entry = file(&folder, &name)?;
        entry["version"] = json!(version);
        entry["write_report"] = json!({
            "appearance_fallback": report.appearance_fallback.map(|value| format!("{value:?}")),
            "baked_raster_masks": report.baked_raster_masks,
            "rounded_mask_densities": report.rounded_mask_densities,
        });
        entry["read_report"] = json!(format!("{read_report:?}"));
        entry["reopened_vector_count"] = json!(
            reopened
                .nodes
                .iter()
                .filter(|node| node.vector_mask.is_some())
                .count()
        );
        entry["reopened_composite"] = png(
            &folder,
            format!("reopened-{extension}.png"),
            (doc.width, doc.height),
            image::ColorType::Rgba8,
            &reopened_pixels,
        )?;
        exports.push(entry);
    }
    emulsion_io::ora::write(doc, &folder.join("source-after.ora"))?;
    assert_eq!(
        doc, &before,
        "{id}: evidence generation mutated native source"
    );
    let sibling_order: Vec<Value> = std::iter::once(None)
        .chain(doc.nodes.iter().filter(|node| node.kind.is_group()).map(|node| Some(node.id)))
        .map(|parent| json!({
            "parent": parent.and_then(|id| doc.node(id)).map(|node| &node.name),
            "bottom_to_top": doc.children(parent).into_iter().map(|id| &doc.node(id).unwrap().name).collect::<Vec<_>>(),
        })).collect();
    std::fs::write(
        folder.join("manifest.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 2, "kind": "vector-mask-interchange-case", "case_id": id,
            "origin": if control { "canonical-8.24-control" } else { "unchanged-original" },
            "expected_appearance_fallback": expected.map(|value| format!("{value:?}")),
            "canvas": [doc.width, doc.height], "blend_space": doc.blend_space,
            "native_archives": [file(&folder, "source-before.ora")?, file(&folder, "source-after.ora")?],
            "native_document_unchanged": true, "layers": layers,
            "sibling_order": sibling_order, "composite": composite, "exports": exports,
        }))?,
    )?;
    Ok(json!({"id": id, "manifest": format!("{id}/manifest.json")}))
}

pub(super) fn write_packet(out: &Path) -> Result<()> {
    let original = fixture()?;
    let before = original.clone();
    let mut cases = vec![write_case(
        out,
        "original-complete-scene",
        &original,
        Some(AppearanceFallback::UnsupportedFeatures),
        false,
    )?];
    let background = original
        .nodes
        .iter()
        .find(|node| node.name == "Opaque background")
        .ok_or("Missing original backdrop")?
        .id;
    let cubic = original
        .nodes
        .iter()
        .find(|node| node.name == "Off-canvas cubic")
        .ok_or("Missing original cubic")?
        .id;
    let group = original
        .nodes
        .iter()
        .find(|node| node.name == "Editable vector group")
        .ok_or("Missing original group")?
        .id;
    let mut original_vector_names = Vec::new();
    for node in &original.nodes {
        if node.vector_mask.is_none() {
            continue;
        }
        original_vector_names.push(node.name.clone());
        let id = format!("original-root-{:02}", node.id);
        let ids = if node.id == group {
            let mut ids = original.children(Some(group));
            ids.push(group);
            ids
        } else {
            vec![background, node.id]
        };
        let scene = retained_scene(&original, &ids)?;
        cases.push(write_case(
            out,
            &id,
            &scene,
            (node.id == cubic).then_some(AppearanceFallback::BlendSpaceDifference),
            false,
        )?);
    }
    assert_eq!(original_vector_names.len(), 13);
    let cubic_original = retained_scene(&original, &[background, cubic])?;
    let (mask, words) = canonical_mask(&cubic_original, cubic_original.node(cubic).unwrap())?;
    let mut control = cubic_original.clone();
    control.node_mut(cubic).unwrap().vector_mask = Some(mask);
    let (again, again_words) = canonical_mask(&control, control.node(cubic).unwrap())?;
    assert_eq!(
        words, again_words,
        "Control changed expected serialized words"
    );
    assert_eq!(
        Some(&again),
        control.node(cubic).unwrap().vector_mask.as_ref()
    );
    let mut restored = control.clone();
    restored.node_mut(cubic).unwrap().vector_mask =
        cubic_original.node(cubic).unwrap().vector_mask.clone();
    assert_eq!(
        restored, cubic_original,
        "Control changed fields outside vector representation"
    );
    assert_ne!(
        cpu_pixels(&control),
        cpu_pixels(&cubic_original),
        "Control must expose changed native appearance"
    );
    assert_ne!(
        control
            .composite_mask(control.node(cubic).unwrap())?
            .unwrap()
            .to_gray8(),
        cubic_original
            .composite_mask(cubic_original.node(cubic).unwrap())?
            .unwrap()
            .to_gray8(),
        "Control must expose changed native coverage",
    );
    let words_file = "cubic-expected-signed-8.24-words.be";
    std::fs::write(
        out.join(words_file),
        words
            .iter()
            .flat_map(|word| word.to_be_bytes())
            .collect::<Vec<_>>(),
    )?;
    cases.push(write_case(
        out,
        "canonical-cubic-control",
        &control,
        None,
        true,
    )?);
    assert_eq!(
        original, before,
        "Packet orchestration changed the authored scene"
    );
    std::fs::write(
        out.join("manifest.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 2, "kind": "vector-mask-interchange-packet", "cases": cases,
            "original_vector_names": original_vector_names,
            "original_source_case": "original-complete-scene",
            "canonical_control": {"original_case": "original-root-03", "control_case": "canonical-cubic-control",
                "expected_words": file(out, words_file)?, "same_expected_8_24_words": true,
                "native_geometry_and_coverage_changed": true,
                "disclosure": "Original cubic preserves its original appearance through explicit fallback. The separate canonical control proves only the standard 8.24 record geometry; its native coverage and appearance differ from the authored cubic."},
            "density_packet": "density-boundaries/manifest.json",
            "scope": "12 original editable vector masks in admitted contexts, one separate canonical cubic control, and exact preservation of both original fallback scenes. No Adobe-application or feather-kernel parity claim.",
        }))?,
    )?;
    Ok(())
}
