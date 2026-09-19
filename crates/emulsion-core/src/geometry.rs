//! Canvas geometry: crop (with straighten) and image size.
//!
//! Pixel nodes only move: their placements change and their source pixels
//! are untouched, so cropping and resizing are lossless and fully reversible.
//! Document-space masks (on non-pixel nodes, and the selection) have no
//! placement, so they are resampled.

use crate::document::Document;
use crate::node::NodeKind;
use emulsion_raster::{IRect, Mask};
use glam::{DAffine2, dvec2};
use std::sync::Arc;

/// Resample a document-space mask: output pixel `p` reads `inv(p)`.
fn remap(m: &Mask, w: u32, h: u32, inv: DAffine2) -> Mask {
    let identity_shift = inv.matrix2 == glam::DMat2::IDENTITY
        && inv.translation.x.fract() == 0.0
        && inv.translation.y.fract() == 0.0;
    if identity_shift {
        let (dx, dy) = (inv.translation.x as i32, inv.translation.y as i32);
        let src = m.read_rect(IRect::new(dx, dy, w as i32, h as i32));
        // read_rect fills outside with the source fill; keep that fill.
        return Mask::from_pixels(w, h, m.fill(), &src);
    }
    let fill = m.fill() as f32;
    let get = |x: i64, y: i64| -> f32 {
        if x < 0 || y < 0 || x >= m.width() as i64 || y >= m.height() as i64 {
            fill
        } else {
            m.get(x as u32, y as u32) as f32
        }
    };
    Mask::from_fn(w, h, m.fill(), |x, y| {
        let p = inv.transform_point2(dvec2(x as f64 + 0.5, y as f64 + 0.5)) - dvec2(0.5, 0.5);
        let (fx, fy) = (p.x.floor(), p.y.floor());
        let (ax, ay) = ((p.x - fx) as f32, (p.y - fy) as f32);
        let (ix, iy) = (fx as i64, fy as i64);
        let top = get(ix, iy) + (get(ix + 1, iy) - get(ix, iy)) * ax;
        let bot = get(ix, iy + 1) + (get(ix + 1, iy + 1) - get(ix, iy + 1)) * ax;
        (top + (bot - top) * ay).round().clamp(0.0, 255.0) as u8
    })
}

/// Apply `to_new` (old document space → new document space) to everything.
fn transform_all(doc: &mut Document, w: u32, h: u32, to_new: DAffine2) {
    let angle = to_new
        .matrix2
        .x_axis
        .y
        .atan2(to_new.matrix2.x_axis.x)
        .to_degrees();
    let scale = to_new.matrix2.x_axis.length();
    let inv = to_new.inverse();
    for n in &mut doc.nodes {
        match &mut n.kind {
            NodeKind::Raster { raster, placement } => {
                // Placements rotate about the content centre: move the centre,
                // add the angle, scale the size.
                let (rw, rh) = (raster.width() as f64, raster.height() as f64);
                let c = dvec2(
                    placement.x + rw * placement.scale_x / 2.0,
                    placement.y + rh * placement.scale_y / 2.0,
                );
                let c2 = to_new.transform_point2(c);
                placement.scale_x *= scale;
                placement.scale_y *= scale;
                placement.rotation += angle;
                placement.x = c2.x - rw * placement.scale_x / 2.0;
                placement.y = c2.y - rh * placement.scale_y / 2.0;
            }
            NodeKind::Smart {
                placement, source, ..
            } => {
                let (rw, rh) = (source.width() as f64, source.height() as f64);
                let c = dvec2(
                    placement.x + rw * placement.scale_x / 2.0,
                    placement.y + rh * placement.scale_y / 2.0,
                );
                let c2 = to_new.transform_point2(c);
                placement.scale_x *= scale;
                placement.scale_y *= scale;
                placement.rotation += angle;
                placement.x = c2.x - rw * placement.scale_x / 2.0;
                placement.y = c2.y - rh * placement.scale_y / 2.0;
            }
            NodeKind::Path { path, style, cache } => {
                let mut p = (**path).clone();
                p.transform(to_new);
                style.width = (style.width as f64 * scale) as f32;
                *cache = Arc::new(p.rasterize(style, w, h));
                *path = Arc::new(p);
                if let Some(m) = &n.mask {
                    n.mask = Some(Arc::new(remap(m, w, h, inv)));
                }
            }
            NodeKind::Text { spec, cache } => {
                let mut s = (**spec).clone();
                let p = to_new.transform_point2(glam::dvec2(s.x as f64, s.y as f64));
                s.x = p.x as f32;
                s.y = p.y as f32;
                s.size = (s.size as f64 * scale) as f32;
                s.width = s.width.map(|w| (w as f64 * scale) as f32);
                s.letter_spacing = (s.letter_spacing as f64 * scale) as f32;
                let s = s.sanitized();
                *cache = Arc::new(crate::text::rasterize(&s, w, h));
                *spec = Arc::new(s);
                if let Some(m) = &n.mask {
                    n.mask = Some(Arc::new(remap(m, w, h, inv)));
                }
            }
            _ => {
                if let Some(m) = &n.mask {
                    n.mask = Some(Arc::new(remap(m, w, h, inv)));
                }
            }
        }
    }
    // Guides stay straight only when nothing rotates; otherwise they go.
    if angle.abs() < 1e-9 {
        for g in &mut doc.guides {
            let p = if g.vertical {
                dvec2(g.pos, 0.0)
            } else {
                dvec2(0.0, g.pos)
            };
            let q = to_new.transform_point2(p);
            g.pos = if g.vertical { q.x } else { q.y };
        }
    } else {
        doc.guides.clear();
    }
    if let Some(sel) = &doc.selection {
        doc.selection = Some(Arc::new(remap(sel, w, h, inv)));
    }
    doc.width = w;
    doc.height = h;
}

pub fn crop(doc: &mut Document, rect: IRect, rotation: f64) {
    let c = dvec2(doc.width as f64 / 2.0, doc.height as f64 / 2.0);
    let rot = DAffine2::from_translation(c)
        * DAffine2::from_angle(rotation.to_radians())
        * DAffine2::from_translation(-c);
    let to_new = DAffine2::from_translation(dvec2(-rect.x as f64, -rect.y as f64)) * rot;
    transform_all(doc, rect.w.max(1) as u32, rect.h.max(1) as u32, to_new);
}

pub fn resize(doc: &mut Document, width: u32, height: u32) {
    // Uniform scale by width keeps rotated placements exact.
    let s = width as f64 / doc.width as f64;
    transform_all(
        doc,
        width.max(1),
        height.max(1),
        DAffine2::from_scale(dvec2(s, s)),
    );
}

#[cfg(test)]
mod tests {
    use crate::command::Slot;
    use crate::{Command, Document, Node, NodeKind};
    use emulsion_raster::composite::flatten;
    use emulsion_raster::{IRect, Placement, Raster};
    use std::sync::Arc;

    fn doc() -> Document {
        let mut d = Document::new(200, 100);
        let r = Raster::from_fn(200, 100, [0; 4], |x, y| {
            if x < 100 {
                [65535, 0, 0, 65535]
            } else if y < 50 {
                [0, 65535, 0, 65535]
            } else {
                [0, 0, 65535, 65535]
            }
        });
        Command::AddNode {
            node: Box::new(Node::raster(0, "img", Arc::new(r), Placement::default())),
            slot: Slot::TOP,
        }
        .apply(&mut d)
        .unwrap();
        d
    }

    #[test]
    fn crop_moves_layers_without_resampling() {
        let mut d = doc();
        let before = match &d.nodes[0].kind {
            NodeKind::Raster { raster, .. } => raster.clone(),
            _ => unreachable!(),
        };
        Command::Crop {
            rect: IRect::new(90, 20, 40, 40),
            rotation: 0.0,
        }
        .apply(&mut d)
        .unwrap();
        assert_eq!((d.width, d.height), (40, 40));
        let NodeKind::Raster { raster, placement } = &d.nodes[0].kind else {
            panic!()
        };
        assert!(Arc::ptr_eq(raster, &before), "source pixels untouched");
        assert_eq!((placement.x, placement.y), (-90.0, -20.0));
        let flat = flatten(&d.composite_tree(), 0);
        assert!(flat.get(5, 5)[0] > 60000, "left of x=100 is red");
        assert!(flat.get(20, 5)[1] > 60000, "right, top is green");
    }

    #[test]
    fn crop_extends_canvas_and_resize_is_reversible() {
        let mut d = doc();
        Command::Crop {
            rect: IRect::new(-10, -10, 220, 120),
            rotation: 0.0,
        }
        .apply(&mut d)
        .unwrap();
        assert_eq!((d.width, d.height), (220, 120));
        Command::ImageSize {
            width: 110,
            height: 60,
        }
        .apply(&mut d)
        .unwrap();
        Command::ImageSize {
            width: 220,
            height: 120,
        }
        .apply(&mut d)
        .unwrap();
        let NodeKind::Raster { placement, .. } = &d.nodes[0].kind else {
            panic!()
        };
        assert!((placement.x - 10.0).abs() < 1e-9 && (placement.scale_x - 1.0).abs() < 1e-12);
    }

    #[test]
    fn straighten_rotates_placements_about_the_centre() {
        let mut d = doc();
        Command::Crop {
            rect: IRect::new(0, 0, 200, 100),
            rotation: 90.0,
        }
        .apply(&mut d)
        .unwrap();
        let NodeKind::Raster { placement, .. } = &d.nodes[0].kind else {
            panic!()
        };
        assert_eq!(placement.rotation, 90.0);
        assert!(
            (placement.x - 0.0).abs() < 1e-9,
            "centre stays at the canvas centre"
        );
    }

    #[test]
    fn selection_follows_the_crop() {
        let mut d = doc();
        let sel = emulsion_raster::select::rect(200, 100, 100.0, 0.0, 10.0, 10.0);
        Command::SetSelection {
            selection: Some(Arc::new(sel)),
        }
        .apply(&mut d)
        .unwrap();
        Command::Crop {
            rect: IRect::new(95, 0, 20, 20),
            rotation: 0.0,
        }
        .apply(&mut d)
        .unwrap();
        let s = d.selection.clone().unwrap();
        assert_eq!((s.width(), s.height()), (20, 20));
        assert_eq!(s.get(4, 5), 0);
        assert_eq!(s.get(5, 5), 255);
    }
}
