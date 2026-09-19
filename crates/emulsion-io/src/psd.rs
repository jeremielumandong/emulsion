//! Photoshop documents through `ag-psd`: layers, groups, opacity, blend
//! modes, visibility and masks come across in both directions. Reading
//! turns every pixel layer into a raster node; writing rasterises what
//! Photoshop has no equivalent for (adjustments, text, paths, smart
//! layers) into ordinary layers and adds a flattened composite so any
//! viewer shows the picture.

use crate::{IoError, Result, write_atomic};
use ag_psd::psd::{BlendMode as PsdBlend, ColorMode, Layer, LayerMaskData, PixelData, Psd};
use ag_psd::psd::{ReadOptions, WriteOptions};
use emulsion_core::command::Slot;
use emulsion_core::{Command, Document, Node, NodeId, NodeKind};
use emulsion_raster::composite::flatten;
use emulsion_raster::{BlendMode, Mask, Placement, Raster};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

pub fn is_psd(path: &Path) -> bool {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|e| e == "psd" || e == "psb")
}

fn blend_in(b: Option<PsdBlend>) -> BlendMode {
    match b.unwrap_or(PsdBlend::Normal) {
        PsdBlend::PassThrough => BlendMode::PassThrough,
        PsdBlend::Normal => BlendMode::Normal,
        PsdBlend::Dissolve => BlendMode::Dissolve,
        PsdBlend::Darken => BlendMode::Darken,
        PsdBlend::Multiply => BlendMode::Multiply,
        PsdBlend::ColorBurn => BlendMode::ColorBurn,
        PsdBlend::LinearBurn => BlendMode::LinearBurn,
        PsdBlend::DarkerColor => BlendMode::DarkerColor,
        PsdBlend::Lighten => BlendMode::Lighten,
        PsdBlend::Screen => BlendMode::Screen,
        PsdBlend::ColorDodge => BlendMode::ColorDodge,
        PsdBlend::LinearDodge => BlendMode::LinearDodge,
        PsdBlend::LighterColor => BlendMode::LighterColor,
        PsdBlend::Overlay => BlendMode::Overlay,
        PsdBlend::SoftLight => BlendMode::SoftLight,
        PsdBlend::HardLight => BlendMode::HardLight,
        PsdBlend::VividLight => BlendMode::VividLight,
        PsdBlend::LinearLight => BlendMode::LinearLight,
        PsdBlend::PinLight => BlendMode::PinLight,
        PsdBlend::HardMix => BlendMode::HardMix,
        PsdBlend::Difference => BlendMode::Difference,
        PsdBlend::Exclusion => BlendMode::Exclusion,
        PsdBlend::Subtract => BlendMode::Subtract,
        PsdBlend::Divide => BlendMode::Divide,
        PsdBlend::Hue => BlendMode::Hue,
        PsdBlend::Saturation => BlendMode::Saturation,
        PsdBlend::Color => BlendMode::Color,
        PsdBlend::Luminosity => BlendMode::Luminosity,
        #[allow(unreachable_patterns)]
        _ => BlendMode::Normal,
    }
}

fn blend_out(b: BlendMode) -> PsdBlend {
    match b {
        BlendMode::PassThrough => PsdBlend::PassThrough,
        BlendMode::Normal => PsdBlend::Normal,
        BlendMode::Dissolve => PsdBlend::Dissolve,
        BlendMode::Darken => PsdBlend::Darken,
        BlendMode::Multiply => PsdBlend::Multiply,
        BlendMode::ColorBurn => PsdBlend::ColorBurn,
        BlendMode::LinearBurn => PsdBlend::LinearBurn,
        BlendMode::DarkerColor => PsdBlend::DarkerColor,
        BlendMode::Lighten => PsdBlend::Lighten,
        BlendMode::Screen => PsdBlend::Screen,
        BlendMode::ColorDodge => PsdBlend::ColorDodge,
        BlendMode::LinearDodge => PsdBlend::LinearDodge,
        BlendMode::LighterColor => PsdBlend::LighterColor,
        BlendMode::Overlay => PsdBlend::Overlay,
        BlendMode::SoftLight => PsdBlend::SoftLight,
        BlendMode::HardLight => PsdBlend::HardLight,
        BlendMode::VividLight => PsdBlend::VividLight,
        BlendMode::LinearLight => PsdBlend::LinearLight,
        BlendMode::PinLight => PsdBlend::PinLight,
        BlendMode::HardMix => PsdBlend::HardMix,
        BlendMode::Difference => PsdBlend::Difference,
        BlendMode::Exclusion => PsdBlend::Exclusion,
        BlendMode::Subtract => PsdBlend::Subtract,
        BlendMode::Divide => PsdBlend::Divide,
        BlendMode::Hue => PsdBlend::Hue,
        BlendMode::Saturation => PsdBlend::Saturation,
        BlendMode::Color => PsdBlend::Color,
        BlendMode::Luminosity => PsdBlend::Luminosity,
        #[allow(unreachable_patterns)]
        _ => PsdBlend::Normal,
    }
}

/// Grey values from a mask's pixel block, whichever layout ag-psd used.
fn mask_bytes(px: &PixelData) -> Vec<u8> {
    let n = (px.width * px.height) as usize;
    if px.data.len() == n * 4 {
        px.data.as_chunks::<4>().0.iter().map(|c| c[0]).collect()
    } else if px.data.len() >= n {
        px.data[..n].to_vec()
    } else {
        vec![255; n]
    }
}

/// A layer mask in the layer's own pixel space (`lw`×`lh`, at `lx`,`ly`).
fn mask_in(m: &LayerMaskData, lx: i64, ly: i64, lw: u32, lh: u32) -> Option<Arc<Mask>> {
    let px = m.image_data.as_ref().or(m.canvas.as_ref())?;
    let fill = m.default_color.unwrap_or(255.0).clamp(0.0, 255.0) as u8;
    let bytes = mask_bytes(px);
    let (mx, my) = (m.left.unwrap_or(0.0) as i64, m.top.unwrap_or(0.0) as i64);
    let mut out = vec![fill; (lw * lh) as usize];
    for y in 0..px.height as i64 {
        let dy = my + y - ly;
        if dy < 0 || dy >= lh as i64 {
            continue;
        }
        for x in 0..px.width as i64 {
            let dx = mx + x - lx;
            if dx < 0 || dx >= lw as i64 {
                continue;
            }
            out[(dy * lw as i64 + dx) as usize] = bytes[(y * px.width as i64 + x) as usize];
        }
    }
    Some(Arc::new(Mask::from_pixels(lw, lh, fill, &out)))
}

fn add(doc: &mut Document, node: Node, parent: Option<NodeId>) -> Result<NodeId> {
    Command::AddNode {
        node: Box::new(node),
        slot: Slot::top_of(parent),
    }
    .apply(doc)
    .map_err(|e| IoError::Unsupported(format!("PSD: {e}")))?
    .ok_or_else(|| IoError::Unsupported("PSD: node not added".into()))
}

fn add_layers(doc: &mut Document, layers: &[Layer], parent: Option<NodeId>) -> Result<()> {
    // ag-psd lists layers bottom to top, as the file does.
    for l in layers {
        let name = l
            .additional_info
            .name
            .clone()
            .unwrap_or_else(|| "Layer".into());
        let mut node = if let Some(children) = &l.children {
            let mut g = Node::group(0, name);
            g.blend = blend_in(l.blend_mode);
            let id = finish_node(doc, g, l, parent)?;
            add_layers(doc, children, Some(id))?;
            continue;
        } else {
            let px = l.image_data.as_ref().or(l.canvas.as_ref());
            let (left, top) = (l.left.unwrap_or(0.0), l.top.unwrap_or(0.0));
            let (raster, lw, lh) = match px {
                Some(p)
                    if p.width > 0
                        && p.height > 0
                        && p.data.len() >= (p.width * p.height * 4) as usize =>
                {
                    crate::import::check_size(p.width, p.height)?;
                    (
                        Raster::from_srgba8(
                            p.width,
                            p.height,
                            &p.data[..(p.width * p.height * 4) as usize],
                        ),
                        p.width,
                        p.height,
                    )
                }
                _ => (Raster::transparent(1, 1), 1, 1),
            };
            let mut n = Node::raster(0, name, Arc::new(raster), Placement::at(left, top));
            if let Some(m) = &l.additional_info.mask {
                n.mask = mask_in(m, left as i64, top as i64, lw, lh);
                n.mask_enabled = n.mask.is_some() && !m.disabled.unwrap_or(false);
            }
            n
        };
        node.blend = blend_in(l.blend_mode);
        finish_node(doc, node, l, parent)?;
    }
    Ok(())
}

fn finish_node(
    doc: &mut Document,
    mut node: Node,
    l: &Layer,
    parent: Option<NodeId>,
) -> Result<NodeId> {
    node.visible = !l.hidden.unwrap_or(false);
    node.opacity = l.opacity.unwrap_or(1.0).clamp(0.0, 1.0) as f32;
    node.locked = l.transparency_protected.unwrap_or(false) && false;
    add(doc, node, parent)
}

/// Open a Photoshop file as a layered document.
pub fn read(path: &Path) -> Result<Document> {
    let bytes = std::fs::read(path)?;
    let opts = ReadOptions {
        skip_thumbnail: Some(true),
        skip_composite_image_data: Some(true),
        skip_linked_files_data: Some(true),
        use_image_data: Some(true),
        ..Default::default()
    };
    let psd =
        ag_psd::read_psd(&bytes, &opts).map_err(|e| IoError::Unsupported(format!("PSD: {e:?}")))?;
    let (w, h) = (psd.width as u32, psd.height as u32);
    crate::import::check_size(w, h)?;
    if !matches!(
        psd.color_mode,
        None | Some(ColorMode::Rgb) | Some(ColorMode::Grayscale)
    ) {
        return Err(IoError::Unsupported(format!(
            "PSD colour mode {:?} (only RGB and greyscale open)",
            psd.color_mode
        )));
    }
    let mut doc = Document::new(w, h);
    doc.source_depth = 8;
    match &psd.children {
        Some(layers) if !layers.is_empty() => add_layers(&mut doc, layers, None)?,
        _ => {
            // A flat file: the composite is the only picture.
            let px = psd
                .image_data
                .as_ref()
                .or(psd.canvas.as_ref())
                .ok_or_else(|| {
                    IoError::Unsupported("PSD has neither layers nor a composite".into())
                })?;
            let r = Raster::from_srgba8(
                px.width,
                px.height,
                &px.data[..(px.width * px.height * 4) as usize],
            );
            add(
                &mut doc,
                Node::raster(0, "Background", Arc::new(r), Placement::default()),
                None,
            )?;
        }
    }
    doc.validate()
        .map_err(|e| IoError::Unsupported(format!("PSD: {e}")))?;
    Ok(doc)
}

/// Render one node by itself in document space (for nodes Photoshop has
/// no equivalent for, and for transformed rasters).
fn render_alone(doc: &Document, id: NodeId) -> Raster {
    let mut d = doc.clone();
    let keep: std::collections::HashSet<NodeId> = {
        // The node and its ancestors stay visible; everything else hides.
        let mut set = std::collections::HashSet::new();
        let mut cur = Some(id);
        while let Some(c) = cur {
            set.insert(c);
            cur = d.node(c).and_then(|n| n.parent);
        }
        set
    };
    let hide: Vec<NodeId> = d
        .nodes
        .iter()
        .filter(|n| !keep.contains(&n.id) && !is_descendant(&d, n.id, id))
        .map(|n| n.id)
        .collect();
    for n in d.nodes.iter_mut() {
        if keep.contains(&n.id) {
            n.visible = true;
            n.opacity = 1.0;
            n.blend = if n.kind.is_group() {
                BlendMode::PassThrough
            } else {
                BlendMode::Normal
            };
        } else if hide.contains(&n.id) {
            n.visible = false;
        }
    }
    flatten(&d.composite_tree(), 0)
}

fn is_descendant(doc: &Document, node: NodeId, of: NodeId) -> bool {
    let mut cur = doc.node(node).and_then(|n| n.parent);
    while let Some(c) = cur {
        if c == of {
            return true;
        }
        cur = doc.node(c).and_then(|n| n.parent);
    }
    false
}

/// Trim a document-space raster to its opaque bounds; (left, top, pixels).
fn trimmed(r: &Raster) -> (f64, f64, PixelData) {
    let (w, h) = (r.width(), r.height());
    let data = r.to_srgba8();
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0u32, 0u32);
    for y in 0..h {
        for x in 0..w {
            if data[((y * w + x) * 4 + 3) as usize] != 0 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x1 <= x0 || y1 <= y0 {
        return (
            0.0,
            0.0,
            PixelData {
                width: 1,
                height: 1,
                data: vec![0; 4],
            },
        );
    }
    let (tw, th) = (x1 - x0, y1 - y0);
    let mut out = Vec::with_capacity((tw * th * 4) as usize);
    for y in y0..y1 {
        let s = ((y * w + x0) * 4) as usize;
        out.extend_from_slice(&data[s..s + (tw * 4) as usize]);
    }
    (
        x0 as f64,
        y0 as f64,
        PixelData {
            width: tw,
            height: th,
            data: out,
        },
    )
}

fn layer_for(doc: &Document, n: &Node) -> Layer {
    let mut l = Layer {
        blend_mode: Some(blend_out(n.blend)),
        opacity: Some(n.opacity as f64),
        hidden: Some(!n.visible),
        ..Default::default()
    };
    l.additional_info.name = Some(n.name.clone());
    match &n.kind {
        NodeKind::Group { .. } => {
            let kids: Vec<Layer> = doc
                .children(Some(n.id))
                .into_iter()
                .filter_map(|id| doc.node(id))
                .map(|c| layer_for(doc, c))
                .collect();
            l.children = Some(kids);
        }
        NodeKind::Raster { raster, placement }
            if placement.scale_x == 1.0
                && placement.scale_y == 1.0
                && placement.rotation == 0.0
                && !placement.flip_x
                && !placement.flip_y =>
        {
            l.left = Some(placement.x.round());
            l.top = Some(placement.y.round());
            l.right = Some(placement.x.round() + raster.width() as f64);
            l.bottom = Some(placement.y.round() + raster.height() as f64);
            l.image_data = Some(PixelData {
                width: raster.width(),
                height: raster.height(),
                data: raster.to_srgba8(),
            });
            if let Some(m) = &n.mask {
                let (mw, mh) = (m.width(), m.height());
                let bytes: Vec<u8> = m.read_rect(m.bounds());
                let rgba: Vec<u8> = bytes.iter().flat_map(|v| [*v, *v, *v, 255]).collect();
                l.additional_info.mask = Some(LayerMaskData {
                    left: Some(placement.x.round()),
                    top: Some(placement.y.round()),
                    right: Some(placement.x.round() + mw as f64),
                    bottom: Some(placement.y.round() + mh as f64),
                    default_color: Some(255.0),
                    disabled: Some(!n.mask_enabled),
                    image_data: Some(PixelData {
                        width: mw,
                        height: mh,
                        data: rgba,
                    }),
                    ..Default::default()
                });
            }
        }
        _ => {
            // Rasterise in place: masks and transforms are baked in.
            let (x, y, px) = trimmed(&render_alone(doc, n.id));
            l.left = Some(x);
            l.top = Some(y);
            l.right = Some(x + px.width as f64);
            l.bottom = Some(y + px.height as f64);
            l.image_data = Some(px);
        }
    }
    l
}

/// Write the document as a layered PSD (PSB above 30 000 px).
pub fn write(doc: &Document, path: &Path) -> Result<()> {
    let flat = flatten(&doc.composite_tree(), 0);
    let children: Vec<Layer> = doc
        .children(None)
        .into_iter()
        .filter_map(|id| doc.node(id))
        .map(|n| layer_for(doc, n))
        .collect();
    let psd = Psd {
        width: doc.width as f64,
        height: doc.height as f64,
        channels: Some(4.0),
        bits_per_channel: Some(8.0),
        color_mode: Some(ColorMode::Rgb),
        children: Some(children),
        image_data: Some(PixelData {
            width: doc.width,
            height: doc.height,
            data: flat.to_srgba8(),
        }),
        ..Default::default()
    };
    let psb = doc.width > 30_000
        || doc.height > 30_000
        || path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("psb"));
    let opts = WriteOptions {
        generate_thumbnail: Some(false),
        trim_image_data: Some(false),
        psb: Some(psb),
        compress: Some(true),
        ..Default::default()
    };
    let bytes = ag_psd::write_psd(&psd, &opts);
    write_atomic(path, |f| {
        f.write_all(&bytes)?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn psd_round_trips_layers_groups_masks_and_blend() {
        let mut d = Document::new(64, 48);
        let bg = Raster::solid(64, 48, [0.2, 0.4, 0.6, 1.0]);
        add(
            &mut d,
            Node::raster(0, "Background", Arc::new(bg), Placement::default()),
            None,
        )
        .unwrap();
        let g = add(&mut d, Node::group(0, "Bits"), None).unwrap();
        let mut top = Node::raster(
            0,
            "Red square",
            Arc::new(Raster::solid(10, 10, [1.0, 0.0, 0.0, 1.0])),
            Placement::at(20.0, 15.0),
        );
        top.opacity = 0.5;
        top.blend = BlendMode::Multiply;
        let mut mask = vec![255u8; 100];
        for v in mask.iter_mut().take(50) {
            *v = 0;
        }
        top.mask = Some(Arc::new(Mask::from_pixels(10, 10, 255, &mask)));
        top.mask_enabled = true;
        add(&mut d, top, Some(g)).unwrap();
        let mut hidden = Node::raster(
            0,
            "Hidden",
            Arc::new(Raster::solid(4, 4, [0.0, 1.0, 0.0, 1.0])),
            Placement::at(1.0, 1.0),
        );
        hidden.visible = false;
        add(&mut d, hidden, None).unwrap();

        let dir = std::env::temp_dir().join(format!("emulsion-psd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rt.psd");
        write(&d, &path).unwrap();
        let back = read(&path).unwrap();
        assert_eq!((back.width, back.height), (64, 48));
        let roots = back.children(None);
        assert_eq!(roots.len(), 3, "{roots:?}");
        let names: Vec<String> = roots
            .iter()
            .map(|id| back.node(*id).unwrap().name.clone())
            .collect();
        assert_eq!(names, ["Background", "Bits", "Hidden"]);
        let group = back.node(roots[1]).unwrap();
        assert!(group.kind.is_group());
        let kids = back.children(Some(group.id));
        assert_eq!(kids.len(), 1);
        let sq = back.node(kids[0]).unwrap();
        assert_eq!(sq.name, "Red square");
        assert!((sq.opacity - 0.5).abs() < 0.01);
        assert_eq!(sq.blend, BlendMode::Multiply);
        let NodeKind::Raster { raster, placement } = &sq.kind else {
            panic!("raster");
        };
        assert_eq!((raster.width(), raster.height()), (10, 10));
        assert_eq!((placement.x, placement.y), (20.0, 15.0));
        assert!(raster.get(5, 5)[0] > 60000);
        let m = sq.mask.as_ref().expect("mask");
        assert_eq!(m.get(0, 0), 0);
        assert_eq!(m.get(9, 9), 255);
        assert!(sq.mask_enabled);
        assert!(!back.node(roots[2]).unwrap().visible);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
