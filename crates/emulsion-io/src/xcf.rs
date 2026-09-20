//! GIMP's native XCF through `xcf-rs`, both ways. Reading: every 8-bit
//! RGB/RGBA layer with its name, offset, opacity and visibility becomes a
//! pixel layer. Writing: every visible top-level layer is rendered by itself
//! (adjustments, text, paths and styles baked in) and stored as an 8-bit
//! RGBA layer with its name and opacity, so GIMP opens the picture layered. Greyscale,
//! indexed and high-precision files, layer masks and groups are beyond the
//! crate; those fall back to a flattened import through an installed
//! converter (see `external`).

use crate::{IoError, Result};
use emulsion_core::command::Slot;
use emulsion_core::{Command, Document, Node};
use emulsion_raster::{Placement, Raster};
use std::path::Path;
use std::sync::Arc;
use xcf_rs::data::property::{PropertyIdentifier, PropertyPayload};

pub fn is_xcf(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("xcf"))
}

/// Layer facts GIMP stores as a property list.
struct Props {
    offset: (i32, i32),
    opacity: f32,
    visible: bool,
}

fn props(layer: &xcf_rs::data::layer::Layer) -> Props {
    let mut p = Props {
        offset: (0, 0),
        opacity: 1.0,
        visible: true,
    };
    let be_u32 = |b: &[u8]| u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
    for prop in &layer.properties {
        let raw: &[u8] = match &prop.payload {
            PropertyPayload::Unknown(b) => b,
            PropertyPayload::OffsetsLayer(x, y) => {
                p.offset = (*x as i32, *y as i32);
                continue;
            }
            _ => continue,
        };
        match prop.kind {
            PropertyIdentifier::PropOffsets if raw.len() >= 8 => {
                p.offset = (be_u32(&raw[0..4]) as i32, be_u32(&raw[4..8]) as i32);
            }
            PropertyIdentifier::PropOpacity if raw.len() >= 4 => {
                p.opacity = be_u32(raw).min(255) as f32 / 255.0;
            }
            PropertyIdentifier::PropFloatOpacity if raw.len() >= 4 => {
                p.opacity = f32::from_bits(be_u32(raw)).clamp(0.0, 1.0);
            }
            PropertyIdentifier::PropVisible if raw.len() >= 4 => {
                p.visible = be_u32(raw) != 0;
            }
            _ => {}
        }
    }
    p
}

/// Read `path` as a layered document.
pub fn read(path: &Path) -> Result<Document> {
    // The crate panics on property ids it has no name for (files from a
    // GIMP newer than it knows); a panic here is just "unsupported".
    let xcf = std::panic::catch_unwind(|| xcf_rs::data::xcf::Xcf::open(path))
        .map_err(|_| IoError::Unsupported("XCF: unknown property (newer GIMP?)".into()))?
        .map_err(|e| IoError::Unsupported(format!("XCF: {e:?}")))?;
    let (w, h) = (xcf.header.width, xcf.header.height);
    crate::import::check_size(w, h)?;
    let mut doc = Document::new(w, h);
    doc.source_depth = 8;
    // GIMP lists layers top to bottom; the stack wants bottom first.
    for (i, layer) in xcf.layers.iter().enumerate().rev() {
        let (lw, lh) = layer.dimensions();
        if lw == 0 || lh == 0 {
            continue;
        }
        crate::import::check_size(lw, lh)?;
        let px = layer.raw_rgba_buffer();
        let mut bytes = Vec::with_capacity(px.len() * 4);
        for p in px.iter() {
            bytes.extend_from_slice(&p.0);
        }
        if bytes.len() < (lw * lh * 4) as usize {
            return Err(IoError::Unsupported("XCF: short layer data".into()));
        }
        let p = props(layer);
        let name = if layer.name.is_empty() {
            format!("Layer {}", xcf.layers.len() - i)
        } else {
            layer.name.clone()
        };
        let mut node = Node::raster(
            0,
            name,
            Arc::new(Raster::from_srgba8(
                lw,
                lh,
                &bytes[..(lw * lh * 4) as usize],
            )),
            Placement::at(p.offset.0 as f64, p.offset.1 as f64),
        );
        node.opacity = p.opacity;
        node.visible = p.visible;
        Command::AddNode {
            node: Box::new(node),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .map_err(|e| IoError::Unsupported(format!("XCF: {e}")))?;
    }
    if doc.nodes.is_empty() {
        return Err(IoError::Unsupported("XCF has no pixel layers".into()));
    }
    doc.validate()
        .map_err(|e| IoError::Unsupported(format!("XCF: {e}")))?;
    Ok(doc)
}

/// Write `doc` as a layered 8-bit XCF (GIMP 2.10+ format, version 11).
/// Hidden layers are left out, since the writer cannot mark them hidden.
pub fn write(doc: &Document, path: &Path) -> Result<()> {
    use xcf_rs::create::XcfCreator;
    use xcf_rs::data::color::ColorType;
    use xcf_rs::data::layer::Layer;
    use xcf_rs::data::pixeldata::PixelData;
    use xcf_rs::data::property::{Property, PropertyIdentifier, PropertyPayload};
    use xcf_rs::data::rgba::RgbaPixel;
    use xcf_rs::{LayerColorType, LayerColorValue};

    let (w, h) = (doc.width, doc.height);
    let mut xcf = XcfCreator::new(11, w, h, ColorType::Rgb);
    xcf.add_properties(&vec![]);
    // GIMP stores the top layer first.
    let mut layers = Vec::new();
    for id in doc.children(None).into_iter().rev() {
        let Some(n) = doc.node(id) else { continue };
        if !n.visible {
            continue;
        }
        let px = render_alone(doc, id).to_srgba8();
        let pixels: Vec<RgbaPixel> = px
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| RgbaPixel(*p))
            .collect();
        layers.push(Layer {
            width: w,
            height: h,
            kind: LayerColorType {
                kind: LayerColorValue::Rgb,
                alpha: true,
            },
            name: n.name.clone(),
            pixels: PixelData {
                width: w,
                height: h,
                pixels,
            },
            properties: vec![
                Property {
                    kind: PropertyIdentifier::PropOffsets,
                    length: 8,
                    payload: PropertyPayload::OffsetsLayer(0, 0),
                },
                Property {
                    kind: PropertyIdentifier::PropOpacity,
                    length: 4,
                    payload: PropertyPayload::OpacityLayer(RgbaPixel([
                        0,
                        0,
                        0,
                        (n.opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
                    ])),
                },
                Property {
                    kind: PropertyIdentifier::PropVisible,
                    length: 4,
                    payload: PropertyPayload::VisibleLayer(),
                },
            ],
        });
    }
    if layers.is_empty() {
        // Nothing visible: one empty layer keeps the file valid.
        layers.push(Layer {
            width: w,
            height: h,
            kind: LayerColorType {
                kind: LayerColorValue::Rgb,
                alpha: true,
            },
            name: "Background".into(),
            pixels: PixelData {
                width: w,
                height: h,
                pixels: vec![RgbaPixel([0; 4]); (w * h) as usize],
            },
            properties: vec![],
        });
    }
    xcf.add_layers(&layers);
    let tmp = path.with_extension("xcf.emulsion-tmp");
    xcf.save(&tmp)
        .map_err(|e| IoError::Unsupported(format!("XCF: {e:?}")))?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// One top-level layer rendered by itself in document space, with its
/// blend mode, mask, clipping and children (for a group) applied.
fn render_alone(doc: &Document, id: emulsion_core::NodeId) -> Raster {
    let mut d = doc.clone();
    let keep: std::collections::HashSet<_> = d.subtree(id).into_iter().chain([id]).collect();
    for n in d.nodes.iter_mut() {
        if keep.contains(&n.id) {
            if n.id == id {
                n.opacity = 1.0;
            }
        } else {
            n.visible = false;
        }
    }
    emulsion_raster::composite::flatten(&d.composite_tree(), 0)
}
