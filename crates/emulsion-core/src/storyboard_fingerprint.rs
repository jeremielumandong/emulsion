//! Content fingerprints of storyboard panels: a SHA-256 over what a panel
//! shows and says (its name, its layers and their pixels, vectors and
//! settings, and its panel data), so two copies can be compared without
//! keeping either. Saving and opening a project gives the same fingerprint;
//! anything an artist changes gives another. Extract and merge use it to
//! find panels changed since an extract was made, and change tracking can
//! use it to find edited panels.
//!
//! Left out on purpose: which scene the panel is in, panel and layer locks,
//! layer colour labels, the selection, ruler and drawing guides and the
//! painted-colour history. None of those change what the panel shows.
use crate::Document;
use crate::node::{Node, NodeKind};
use crate::storyboard::Panel;
use emulsion_raster::image::{Pix, Plane};
use emulsion_raster::{IRect, TILE};
use serde::Serialize;
use sha2::{Digest, Sha256};

/// The fingerprint of one panel: its page `name`, its document and its
/// storyboard data, as 64 lowercase hex digits.
pub fn panel_fingerprint(name: &str, doc: &Document, panel: &Panel) -> String {
    let mut h = Sha256::new();
    h.update(b"emulsion-panel-1\0");
    text(&mut h, name.trim());
    // Grouping and locks do not change the panel's content.
    let data = Panel {
        scene: 0,
        locked: false,
        ..panel.clone()
    };
    json(&mut h, &data);
    document(&mut h, doc);
    hex(h)
}

/// The fingerprint of a document's content alone.
pub fn document_fingerprint(doc: &Document) -> String {
    let mut h = Sha256::new();
    h.update(b"emulsion-document-1\0");
    document(&mut h, doc);
    hex(h)
}

/// The fingerprint of any serializable data, such as one part of a panel
/// (its captions, timing or keys) that change tracking compares alone.
pub fn data_fingerprint(value: &impl Serialize) -> String {
    let mut h = Sha256::new();
    json(&mut h, value);
    hex(h)
}

fn hex(h: Sha256) -> String {
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn text(h: &mut Sha256, s: &str) {
    h.update((s.len() as u64).to_le_bytes());
    h.update(s.as_bytes());
}

/// Serde JSON is deterministic for these types (maps are ordered and floats
/// print exactly), so it stands in for a field-by-field hash.
fn json(h: &mut Sha256, value: &impl Serialize) {
    match serde_json::to_vec(value) {
        Ok(bytes) => {
            h.update((bytes.len() as u64).to_le_bytes());
            h.update(&bytes);
        }
        Err(_) => h.update(u64::MAX.to_le_bytes()),
    }
}

fn document(h: &mut Sha256, doc: &Document) {
    h.update(doc.width.to_le_bytes());
    h.update(doc.height.to_le_bytes());
    h.update((doc.nodes.len() as u64).to_le_bytes());
    for node in &doc.nodes {
        layer(h, node);
    }
}

fn layer(h: &mut Sha256, node: &Node) {
    h.update(node.id.to_le_bytes());
    text(h, &node.name);
    json(h, &node.parent);
    h.update([u8::from(node.visible)]);
    // Opacity is stored as text in layered files; a thousandth is finer
    // than any opacity control.
    h.update(((node.opacity.clamp(0., 1.) * 1000.).round() as u32).to_le_bytes());
    json(h, &node.blend);
    json(h, &node.blending);
    json(h, &node.clip_to);
    h.update([u8::from(node.mask_enabled), u8::from(node.mask_linked)]);
    match &node.mask {
        Some(mask) => {
            h.update([1]);
            plane(h, mask, |p| [*p]);
            json(h, &node.mask_transform);
            if !node.mask_properties.is_default() {
                json(h, &node.mask_properties);
            }
        }
        None => h.update([0]),
    }
    if let Some(mask) = &node.vector_mask {
        h.update(b"vector_mask");
        json(h, &*mask.path);
        json(h, &mask.transform);
        json(h, &mask.properties);
        json(h, &mask.empty_coverage);
        h.update([
            u8::from(mask.enabled),
            u8::from(mask.linked),
            u8::from(mask.inverted),
        ]);
    }
    h.update([u8::from(node.effects_enabled)]);
    json(h, &node.styles);
    json(h, &node.style_options);
    match &node.kind {
        NodeKind::Raster { raster, placement } => {
            h.update(b"raster");
            json(h, placement);
            plane(h, raster, pixel_bytes);
        }
        NodeKind::Group { .. } => h.update(b"group"),
        NodeKind::Adjust(adjustment) => {
            h.update(b"adjust");
            json(h, adjustment);
        }
        NodeKind::Fill { rgba } => {
            h.update(b"fill");
            h.update(rgba);
        }
        NodeKind::Path { path, style, .. } => {
            h.update(b"path");
            json(h, path.as_ref());
            json(h, style);
        }
        NodeKind::Text { spec, .. } => {
            h.update(b"text");
            json(h, spec.as_ref());
        }
        NodeKind::Strokes { strokes, .. } => {
            h.update(b"strokes");
            json(h, strokes.as_ref());
        }
        NodeKind::Smart {
            editable,
            source,
            filters,
            filter_styles,
            filter_mask,
            placement,
            ..
        } => {
            h.update(b"smart");
            json(h, editable);
            json(h, filters);
            json(h, filter_styles);
            if let Some(mask) = filter_mask {
                h.update(b"smart_filter_mask");
                json(h, &mask.enabled);
                json(h, &mask.linked);
                json(h, &mask.transform);
                json(h, &mask.properties);
                plane(h, &mask.pixels, |p| [*p]);
            }
            json(h, placement);
            plane(h, source, pixel_bytes);
        }
    }
}

fn pixel_bytes(p: &[u16; 4]) -> [u8; 8] {
    let mut out = [0; 8];
    for (i, c) in p.iter().enumerate() {
        out[i * 2..i * 2 + 2].copy_from_slice(&c.to_le_bytes());
    }
    out
}

/// A plane's pixels by tile, in tile order, skipping tiles that only hold
/// the fill: the same image hashes the same however sparsely it is stored.
fn plane<P: Pix, const N: usize>(h: &mut Sha256, plane: &Plane<P>, bytes: impl Fn(&P) -> [u8; N]) {
    h.update(plane.width().to_le_bytes());
    h.update(plane.height().to_le_bytes());
    h.update(bytes(&plane.fill()));
    let mut coords: Vec<_> = plane.base_tiles().map(|(c, _)| *c).collect();
    coords.sort_by_key(|c| (c.y, c.x));
    let t = TILE as i32;
    let fill = plane.fill();
    for c in coords {
        let rect = IRect::new(c.x * t, c.y * t, t, t).intersect(&plane.bounds());
        if rect.is_empty() {
            continue;
        }
        let px = plane.read_rect(rect);
        if px.iter().all(|p| *p == fill) {
            continue;
        }
        h.update(c.x.to_le_bytes());
        h.update(c.y.to_le_bytes());
        let mut buf = Vec::with_capacity(px.len() * N);
        for p in &px {
            buf.extend_from_slice(&bytes(p));
        }
        h.update(&buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Slot;
    use crate::{Command, Node};
    use emulsion_raster::{Placement, Raster};
    use std::sync::Arc;

    fn doc() -> Document {
        let mut doc = Document::new(300, 20);
        Command::AddNode {
            node: Box::new(Node::raster(
                0,
                "Ink",
                Arc::new(Raster::solid(300, 20, [0.2, 0.1, 0., 0.5])),
                Placement::default(),
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
        doc
    }

    #[test]
    fn fingerprints_follow_content_not_storage_or_grouping() {
        let doc = doc();
        let panel = Panel::new(3, 24);
        let a = panel_fingerprint("Panel 1", &doc, &panel);
        assert_eq!(a.len(), 64);
        // Same content, other scene and lock: same fingerprint.
        let moved = Panel {
            scene: 9,
            locked: true,
            ..panel.clone()
        };
        assert_eq!(panel_fingerprint("Panel 1", &doc, &moved), a);
        // A copy stored densely hashes the same as a shared one.
        let mut dense = doc.clone();
        if let NodeKind::Raster { raster, .. } = &mut dense.nodes[0].kind {
            let px = raster.to_pixels();
            *raster = Arc::new(Raster::from_pixels(300, 20, [0; 4], &px));
        }
        assert_eq!(document_fingerprint(&dense), document_fingerprint(&doc));
        // Name, timing, captions and pixels all count.
        assert_ne!(panel_fingerprint("Panel 2", &doc, &panel), a);
        let longer = Panel {
            frames: 25,
            ..panel.clone()
        };
        assert_ne!(panel_fingerprint("Panel 1", &doc, &longer), a);
        let mut painted = doc.clone();
        if let NodeKind::Raster { raster, .. } = &mut painted.nodes[0].kind {
            *raster = Arc::new(raster.write_rect(IRect::new(290, 5, 1, 1), &[[9, 9, 9, 9]]));
        }
        assert_ne!(panel_fingerprint("Panel 1", &painted, &panel), a);
        let mut hidden = doc.clone();
        hidden.nodes[0].visible = false;
        assert_ne!(document_fingerprint(&hidden), document_fingerprint(&doc));
    }
}
