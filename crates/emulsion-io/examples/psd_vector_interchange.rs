//! Self-authored vector-mask PSD/PSB evidence: originals, fallbacks and controls.
//!
//! cargo run -p emulsion-io --example psd_vector_interchange -- NEW_DIRECTORY
//! python3 scripts/verify_psd_vector_interchange.py NEW_DIRECTORY
//! python3 scripts/verify_psd_density_interchange.py NEW_DIRECTORY/density-boundaries
//!
//! Fresh independent geometry is checked separately from the saved composite.
//! These fixtures do not establish Adobe-application or feather-kernel parity.

use emulsion_core::{
    Command, Document, EmptyVectorCoverage, MaskProperties, Node, NodeKind, VectorMask,
    command::Slot,
};
use emulsion_raster::{
    Mask, Placement, Raster,
    composite::flatten,
    vector::{Anchor, Path as VectorPath, SubPath},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{error::Error, path::Path, sync::Arc};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[path = "psd_vector_interchange/density.rs"]
mod density;
#[path = "psd_vector_interchange/packet.rs"]
mod packet;

fn density_byte(value: f32) -> u8 {
    // The writer widens the stored native f32 before multiplication. In f32,
    // 0.9 * 255 invents a 229.5 tie and produces the wrong byte (230, not 229).
    (f64::from(value) * 255.0).round() as u8
}

fn density_metadata(target: &mut Value, value: f32) {
    target["density_f32_bits"] = json!(value.to_bits());
    let byte = density_byte(value);
    target["density_byte"] = json!((byte != 255).then_some(byte));
}

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
fn raster(name: &str, size: (u32, u32), at: (f64, f64), seed: u32) -> Node {
    let pixels: Vec<u8> = (0..size.1)
        .flat_map(|y| {
            (0..size.0).flat_map(move |x| {
                [
                    ((x * 17 + seed * 31) % 256) as u8,
                    ((y * 13 + seed * 47) % 256) as u8,
                    ((x * 7 + y * 11 + seed * 53) % 256) as u8,
                    (64 + (x * 3 + y * 5 + seed * 19) % 192) as u8,
                ]
            })
        })
        .collect();
    Node::raster(
        0,
        name,
        Arc::new(Raster::from_srgba8(size.0, size.1, &pixels)),
        Placement::at(at.0, at.1),
    )
}
fn rectangle() -> VectorPath {
    VectorPath {
        subpaths: vec![SubPath {
            closed: true,
            anchors: [(8.0, 6.0), (40.0, 6.0), (40.0, 26.0), (8.0, 26.0)]
                .into_iter()
                .map(Anchor::corner)
                .collect(),
        }],
    }
}
fn attach(node: &mut Node, path: VectorPath) {
    node.vector_mask = Some(VectorMask {
        path: Arc::new(path),
        empty_coverage: EmptyVectorCoverage::HideAll,
        ..VectorMask::default()
    });
}
fn carrier(node: &mut Node, parameters: bool) {
    node.mask = Some(Arc::new(Mask::from_fn(142, 95, 255, |x, y| {
        if (x / 9 + y / 7) % 3 == 0 {
            0
        } else {
            ((x * 23 + y * 37) % 256) as u8
        }
    })));
    node.mask_transform = emulsion_core::Mapping2::Affine(glam::DAffine2::from_cols_array(&[
        1.0, 0.0, 0.0, 1.0, -16.0, -11.0,
    ]));
    node.mask_linked = false;
    if parameters {
        node.mask_properties.density = 204.0 / 255.0;
    }
}
fn fixture() -> Result<Document> {
    // Unequal axes catch an x/y swap or normalization by layer dimensions.
    let mut doc = Document::new(128, 80);
    // This interchange scene is authored for the explicit PSD-compatible
    // profile. Legacy Linear alpha over its colored backdrop may correctly
    // require appearance fallback; changing that guard is not fixture setup.
    doc.blend_space = emulsion_raster::blend::BlendSpace::PhotoshopSrgbV1;
    add(
        &mut doc,
        Node::raster(
            0,
            "Opaque background",
            Arc::new(Raster::from_srgba8(
                128,
                80,
                &[24, 34, 49, 255].repeat(128 * 80),
            )),
            Placement::default(),
        ),
        None,
    )?;
    let mut golden = raster("Golden rectangle", (52, 38), (8.0, 4.0), 1);
    attach(&mut golden, rectangle());
    add(&mut doc, golden, None)?;
    let mut cubic = raster("Off-canvas cubic", (140, 100), (-6.0, -5.0), 2);
    attach(
        &mut cubic,
        VectorPath::from_svg("M -2 2 C 8 -24 110 -12 136 18 L 120 74 L 2 68 Z")?,
    );
    cubic.vector_mask.as_mut().unwrap().transform = [1.0, 0.0, 0.0, 1.0, 2.0, 1.0];
    // Smooth/corner state is independently checked, not inferred from handles.
    Arc::make_mut(&mut cubic.vector_mask.as_mut().unwrap().path).subpaths[0].anchors[1].smooth =
        true;
    add(&mut doc, cubic, None)?;
    let mut open = raster("Open affine contour", (58, 43), (43.0, 19.0), 3);
    attach(&mut open, VectorPath::from_svg("M 8 8 L 46 11 L 30 35")?);
    open.vector_mask.as_mut().unwrap().transform = [0.875, 0.125, -0.125, 1.0, 2.5, -1.25];
    add(&mut doc, open, None)?;
    for (index, name) in ["Vector inverted", "Vector unlinked", "Vector disabled"]
        .into_iter()
        .enumerate()
    {
        let mut node = raster(name, (52, 38), (8.0, 4.0), 4 + index as u32);
        attach(&mut node, rectangle());
        let vector = node.vector_mask.as_mut().unwrap();
        vector.inverted = index == 0;
        vector.linked = index != 1;
        vector.enabled = index != 2;
        add(&mut doc, node, None)?;
    }
    for (index, (name, coverage, inverted)) in [
        ("Empty reveal", EmptyVectorCoverage::RevealAll, false),
        ("Empty hide", EmptyVectorCoverage::HideAll, false),
        ("Empty hide inverted", EmptyVectorCoverage::HideAll, true),
    ]
    .into_iter()
    .enumerate()
    {
        let mut node = raster(name, (27, 19), (83.0, 53.0), 7 + index as u32);
        let mut vector = VectorMask::empty(coverage);
        vector.inverted = inverted;
        node.vector_mask = Some(vector);
        add(&mut doc, node, None)?;
    }
    for (index, name) in [
        "Vector density only",
        "Vector feather only",
        "Independent raster and vector",
    ]
    .into_iter()
    .enumerate()
    {
        let mut node = raster(name, (52, 38), (8.0, 4.0), 10 + index as u32);
        attach(&mut node, rectangle());
        carrier(&mut node, index == 2);
        let vector = node.vector_mask.as_mut().unwrap();
        vector.properties = MaskProperties {
            density: if index != 1 { 153.0 / 255.0 } else { 1.0 },
            feather: if index != 0 { 2.25 } else { 0.0 },
        };
        add(&mut doc, node, None)?;
    }
    let mut group = Node::group(0, "Editable vector group");
    attach(&mut group, rectangle());
    group.vector_mask.as_mut().unwrap().transform = [1.0, 0.0, 0.0, 1.0, 68.0, 37.0];
    group.vector_mask.as_mut().unwrap().linked = false;
    let id = add(&mut doc, group, None)?;
    add(
        &mut doc,
        raster("Group source", (51, 35), (70.0, 39.0), 14),
        Some(id),
    )?;
    Ok(doc)
}
fn png(
    out: &Path,
    name: String,
    size: (u32, u32),
    mode: image::ColorType,
    pixels: &[u8],
) -> Result<Value> {
    image::save_buffer(out.join(&name), pixels, size.0, size.1, mode)?;
    Ok(
        json!({"png": name, "mode": if mode == image::ColorType::L8 {"L"} else {"RGBA"},
        "size": [size.0, size.1], "sha256": sha256(pixels)}),
    )
}
fn mask_png(out: &Path, index: usize, suffix: &str, mask: &Mask) -> Result<Value> {
    png(
        out,
        format!("layer-{index:02}-{suffix}.png"),
        (mask.width(), mask.height()),
        image::ColorType::L8,
        &mask.to_gray8(),
    )
}
fn reference(out: &Path, doc: &Document, node: &Node, index: usize) -> Result<Value> {
    let mut result = json!({"name": node.name, "kind": if node.kind.is_group() {"group"} else {"raster"},
        "parent": node.parent.and_then(|id| doc.node(id)).map(|n| &n.name)});
    let (origin, size) = if let NodeKind::Raster { raster, placement } = &node.kind {
        let mut source = png(
            out,
            format!("layer-{index:02}-source.png"),
            (raster.width(), raster.height()),
            image::ColorType::Rgba8,
            &raster.to_srgba8(),
        )?;
        source["bounds"] = json!([
            placement.x as i32,
            placement.y as i32,
            placement.x as i32 + raster.width() as i32,
            placement.y as i32 + raster.height() as i32
        ]);
        result["source"] = source;
        (
            (placement.x, placement.y),
            (raster.width(), raster.height()),
        )
    } else {
        ((0.0, 0.0), (doc.width, doc.height))
    };
    if let Some(mask) = &node.mask {
        let mut raster = mask_png(out, index, "raster-source", mask)?;
        let (left, top) = (
            (origin.0
                + node
                    .mask_transform
                    .affine()
                    .expect("affine fixture mapping")
                    .translation
                    .x) as i32,
            (origin.1
                + node
                    .mask_transform
                    .affine()
                    .expect("affine fixture mapping")
                    .translation
                    .y) as i32,
        );
        raster["bounds"] = json!([
            left,
            top,
            left + mask.width() as i32,
            top + mask.height() as i32
        ]);
        raster["fill"] = json!(mask.fill());
        raster["enabled"] = json!(node.mask_enabled);
        raster["linked"] = json!(node.mask_linked);
        density_metadata(&mut raster, node.mask_properties.density);
        raster["feather"] =
            json!((node.mask_properties.feather != 0.0).then_some(node.mask_properties.feather));
        result["raster_mask"] = raster;
    }
    if let Some(vector) = &node.vector_mask {
        let [a, b, c, d, tx, ty] = vector.transform;
        let point = |p: (f64, f64)| {
            [
                a * p.0 + c * p.1 + tx + origin.0,
                b * p.0 + d * p.1 + ty + origin.1,
            ]
        };
        let paths: Vec<Value> = vector.path.subpaths.iter().map(|s| json!({
            "closed": s.closed,
            "anchors": s.anchors.iter().map(|p| json!({"smooth": p.smooth,
                "preceding": point(p.h_in), "anchor": point(p.p), "leaving": point(p.h_out)})).collect::<Vec<_>>()
        })).collect();
        let full = doc
            .vector_mask_for_inspection(node)?
            .ok_or("Missing native vector mask")?;
        let mut sharp = node.clone();
        let v = sharp.vector_mask.as_mut().unwrap();
        v.inverted = false;
        v.properties = MaskProperties::default();
        let sharp_mask = doc
            .vector_mask_for_inspection(&sharp)?
            .ok_or("Missing sharp geometry")?;
        let mut effective = node.clone();
        effective.mask = None;
        effective.vector_mask.as_mut().unwrap().properties.feather = 0.0;
        let effective_mask = doc
            .composite_mask(&effective)?
            .unwrap_or_else(|| Arc::new(Mask::empty(size.0, size.1, 255)));
        let mut combined = node.clone();
        combined.mask_properties.feather = 0.0;
        combined.vector_mask.as_mut().unwrap().properties.feather = 0.0;
        let combined_mask = doc
            .composite_mask(&combined)?
            .unwrap_or_else(|| Arc::new(Mask::empty(size.0, size.1, 255)));
        result["vector_mask"] = json!({
            "enabled": vector.enabled, "linked": vector.linked, "inverted": vector.inverted,
            "initial_fill": u8::from(vector.empty_coverage == EmptyVectorCoverage::RevealAll),
            "density_f32_bits": vector.properties.density.to_bits(),
            "density_byte": (density_byte(vector.properties.density) != 255).then_some(density_byte(vector.properties.density)),
            "feather": (vector.properties.feather != 0.0).then_some(vector.properties.feather),
            "transform": vector.transform, "paths": paths,
            "viewport": [origin.0 as i32, origin.1 as i32, origin.0 as i32 + size.0 as i32, origin.1 as i32 + size.1 as i32],
            "sharp_geometry": mask_png(out, index, "sharp-geometry", &sharp_mask)?,
            "effective_no_feather": mask_png(out, index, "effective-no-feather", &effective_mask)?,
            "combined_no_feather": mask_png(out, index, "combined-no-feather", &combined_mask)?,
            "native_full_inspection": mask_png(out, index, "native-full-inspection", &full)?,
        });
    }
    Ok(result)
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
    packet::write_packet(&out)?;
    density::write_packet(&out.join("density-boundaries"))?;
    println!(
        "Wrote original/fallback/control PSD/PSB evidence and separate density packet to {}",
        out.display()
    );
    Ok(())
}
