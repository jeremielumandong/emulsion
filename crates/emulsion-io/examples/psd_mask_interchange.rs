//! Generate small editable raster-mask interchange fixtures and reference pixels.
//!
//! cargo run -p emulsion-io --example psd_mask_interchange -- NEW_OUTPUT_DIRECTORY
//! python3 scripts/verify-psd-raster-masks.py NEW_OUTPUT_DIRECTORY
//!
//! The independent verifier checks stored records, not Photoshop rendering parity.

use emulsion_core::{Command, Document, MaskProperties, Node, NodeKind, command::Slot};
use emulsion_raster::{Mask, Placement, Raster, composite::flatten};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{error::Error, path::Path, sync::Arc};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn add(doc: &mut Document, node: Node, parent: Option<u64>) -> Result<u64> {
    Ok(Command::AddNode {
        node: Box::new(node),
        slot: Slot::top_of(parent),
    }
    .apply(doc)?
    .ok_or("Node was not added")?)
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn raster(name: &str, width: u32, height: u32, x: f64, y: f64, seed: u32) -> Node {
    let pixels: Vec<u8> = (0..height)
        .flat_map(|row| {
            (0..width).flat_map(move |col| {
                [
                    ((col * 11 + seed * 37) % 256) as u8,
                    ((row * 17 + seed * 53) % 256) as u8,
                    ((col * 7 + row * 13 + seed * 19) % 256) as u8,
                    (96 + (col * 3 + row * 5 + seed * 7) % 160) as u8,
                ]
            })
        })
        .collect();
    Node::raster(
        0,
        name,
        Arc::new(Raster::from_srgba8(width, height, &pixels)),
        Placement::at(x, y),
    )
}

struct MaskSpec {
    size: (u32, u32),
    offset: (f64, f64),
    fill: u8,
    enabled: bool,
    linked: bool,
    parameters: bool,
}

fn attach_mask(node: &mut Node, spec: MaskSpec) {
    node.mask = Some(Arc::new(Mask::from_fn(
        spec.size.0,
        spec.size.1,
        spec.fill,
        |x, y| match (x / 5 + y / 4) % 4 {
            0 => 0,
            1 => 255,
            _ => ((x * 29 + y * 31) % 256) as u8,
        },
    )));
    node.mask_transform = [1.0, 0.0, 0.0, 1.0, spec.offset.0, spec.offset.1];
    node.mask_enabled = spec.enabled;
    node.mask_linked = spec.linked;
    if spec.parameters {
        node.mask_properties = MaskProperties {
            density: 153.0 / 255.0,
            feather: 2.25,
        };
    }
}

fn reference(out: &Path, doc: &Document, node: &Node, index: usize) -> Result<Value> {
    let mut entry = json!({
        "name": node.name,
        "kind": if node.kind.is_group() { "group" } else { "raster" },
        "parent": node.parent.and_then(|id| doc.node(id)).map(|parent| &parent.name),
    });
    let (x, y) = if let NodeKind::Raster { raster, placement } = &node.kind {
        let name = format!("layer-{index:02}-source.png");
        let pixels = raster.to_srgba8();
        image::save_buffer(
            out.join(&name),
            &pixels,
            raster.width(),
            raster.height(),
            image::ColorType::Rgba8,
        )?;
        entry["source"] = json!({
            "png": name,
            "mode": "RGBA",
            "sha256": sha256(&pixels),
            "size": [raster.width(), raster.height()],
            "bounds": [placement.x as i32, placement.y as i32,
                placement.x as i32 + raster.width() as i32,
                placement.y as i32 + raster.height() as i32],
        });
        (placement.x, placement.y)
    } else {
        (0.0, 0.0)
    };
    if let Some(mask) = &node.mask {
        let name = format!("layer-{index:02}-mask.png");
        let pixels = mask.to_gray8();
        image::save_buffer(
            out.join(&name),
            &pixels,
            mask.width(),
            mask.height(),
            image::ColorType::L8,
        )?;
        let left = (x + node.mask_transform[4]) as i32;
        let top = (y + node.mask_transform[5]) as i32;
        let properties = node.mask_properties;
        let density =
            (properties.density != 1.0).then_some((properties.density * 255.0).round() as u8);
        let feather = (properties.feather != 0.0).then_some(properties.feather);
        let flags = u8::from(!node.mask_linked)
            | (u8::from(!node.mask_enabled) << 1)
            | (u8::from(density.is_some() || feather.is_some()) << 4);
        entry["mask"] = json!({
            "png": name,
            "mode": "L",
            "sha256": sha256(&pixels),
            "size": [mask.width(), mask.height()],
            "bounds": [left, top, left + mask.width() as i32, top + mask.height() as i32],
            "fill": mask.fill(),
            "enabled": node.mask_enabled,
            "linked": node.mask_linked,
            "density_byte": density,
            "feather": feather,
            "flags_byte": flags,
            "channel_id": -2,
        });
    }
    Ok(entry)
}

fn fixture() -> Result<Document> {
    let mut doc = Document::new(96, 64);
    add(
        &mut doc,
        Node::raster(
            0,
            "Opaque background",
            Arc::new(Raster::from_srgba8(
                96,
                64,
                &[24, 34, 49, 255].repeat(96 * 64),
            )),
            Placement::default(),
        ),
        None,
    )?;
    for (index, (name, enabled, linked, fill, parameters)) in [
        ("Enabled linked black outside", true, true, 0, true),
        ("Disabled unlinked white outside", false, false, 255, true),
        ("Enabled unlinked white outside", true, false, 255, false),
        ("Disabled linked black outside", false, true, 0, false),
    ]
    .into_iter()
    .enumerate()
    {
        let mut node = raster(name, 25, 21, 5.0 + index as f64 * 20.0, 8.0, index as u32);
        attach_mask(
            &mut node,
            MaskSpec {
                // First plane crosses all four canvas edges and is much larger
                // than its layer, so clipping it cannot pass the byte check.
                size: if index == 0 { (110, 80) } else { (31, 27) },
                offset: if index == 0 {
                    (-12.0, -16.0)
                } else {
                    (-3.0, 4.0)
                },
                enabled,
                linked,
                fill,
                parameters,
            },
        );
        add(&mut doc, node, None)?;
    }
    for (index, (name, enabled, linked, fill, parameters)) in [
        ("Enabled unlinked group", true, false, 255, true),
        ("Disabled linked group", false, true, 0, false),
    ]
    .into_iter()
    .enumerate()
    {
        let mut group = Node::group(0, name);
        attach_mask(
            &mut group,
            MaskSpec {
                size: (47, 33),
                offset: (index as f64 * 46.0 - 3.0, 29.0),
                enabled,
                linked,
                fill,
                parameters,
            },
        );
        let id = add(&mut doc, group, None)?;
        add(
            &mut doc,
            raster(
                &format!("Group child {}", index + 1),
                37,
                22,
                4.0 + index as f64 * 46.0,
                37.0,
                8 + index as u32,
            ),
            Some(id),
        )?;
        add(
            &mut doc,
            raster(
                &format!("Group accent {}", index + 1),
                14,
                11,
                13.0 + index as f64 * 46.0,
                43.0,
                12 + index as u32,
            ),
            Some(id),
        )?;
    }
    Ok(doc)
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let out = std::path::PathBuf::from(args.next().ok_or("Pass a new output directory")?);
    if args.next().is_some() {
        return Err("Unexpected argument".into());
    }
    std::fs::create_dir_all(&out)?;
    if std::fs::read_dir(&out)?.next().is_some() {
        return Err("Choose a new or empty output directory".into());
    }
    let doc = fixture()?;
    doc.validate()?;
    assert!(!emulsion_io::psd::needs_appearance_fallback(&doc));
    assert!(!emulsion_io::psd::has_baked_raster_masks(&doc));
    let layers = doc
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| reference(&out, &doc, node, index))
        .collect::<Result<Vec<_>>>()?;
    let sibling_order = std::iter::once(None)
        .chain(
            doc.nodes
                .iter()
                .filter(|node| node.kind.is_group())
                .map(|node| Some(node.id)),
        )
        .map(|parent| {
            json!({
                "parent": parent.and_then(|id| doc.node(id)).map(|node| &node.name),
                "bottom_to_top": doc.children(parent).into_iter()
                    .filter_map(|id| doc.node(id)).map(|node| &node.name).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    let composite = flatten(&doc.composite_tree(), 0).to_srgba8();
    assert!(
        composite
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255)
    );
    image::save_buffer(
        out.join("native-composite.png"),
        &composite,
        doc.width,
        doc.height,
        image::ColorType::Rgba8,
    )?;
    emulsion_io::ora::write(&doc, &out.join("editable-source.ora"))?;
    emulsion_io::psd::write(&doc, &out.join("editable-masks.psd"))?;
    emulsion_io::psd::write(&doc, &out.join("editable-masks.psb"))?;
    // Native-only companion for checking the PSD export warning in the UI.
    // The original editable fixture and its independent-reader manifest stay unchanged.
    let mut baked = doc.clone();
    baked
        .nodes
        .iter_mut()
        .find(|node| node.mask.is_some())
        .ok_or("Fixture has no masked node")?
        .mask_transform[4] += 0.5;
    assert!(emulsion_io::psd::has_baked_raster_masks(&baked));
    assert!(!emulsion_io::psd::needs_appearance_fallback(&baked));
    emulsion_io::ora::write(&baked, &out.join("baked-mask-source.ora"))?;
    let manifest = json!({
        "schema": 1,
        "description": "Editable raster mask records; no Photoshop or feather-kernel parity claim",
        "canvas": [doc.width, doc.height],
        "native": "editable-source.ora",
        "composite": {"png": "native-composite.png", "mode": "RGBA", "sha256": sha256(&composite), "size": [doc.width, doc.height]},
        "exports": [{"file": "editable-masks.psd", "version": 1}, {"file": "editable-masks.psb", "version": 2}],
        "layers": layers,
        "sibling_order": sibling_order,
    });
    std::fs::write(
        out.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    println!(
        "Wrote PSD, PSB, native ORA, source PNGs and manifest to {}",
        out.display()
    );
    println!("baked-mask-source.ora has a half-pixel mask translation for export-warning QA");
    Ok(())
}
