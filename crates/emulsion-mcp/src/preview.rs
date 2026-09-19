//! Bounded document-space previews, including exact image-to-document mapping.
use crate::server::ToolResult;
use base64::Engine as _;
use emulsion_core::Document;
use emulsion_raster::composite::{level_size, render_tile};
use emulsion_raster::{TileCoord, color};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug)]
struct ViewRegion {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl ViewRegion {
    fn parse(doc: &Document, args: &Value) -> Result<Self, ToolResult> {
        let all = Self {
            x: 0,
            y: 0,
            width: doc.width,
            height: doc.height,
        };
        let Some(value) = args.get("region") else {
            return Ok(all);
        };
        let error = || {
            ToolResult::error(
                "region must be [x, y, width, height] in whole document pixels, positive in size and entirely inside the canvas",
            )
        };
        let values = value
            .as_array()
            .filter(|v| v.len() == 4)
            .ok_or_else(error)?;
        let mut v = [0_u32; 4];
        for (dest, source) in v.iter_mut().zip(values) {
            *dest = source
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(error)?;
        }
        if v[2] == 0
            || v[3] == 0
            || v[0] as u64 + v[2] as u64 > doc.width as u64
            || v[1] as u64 + v[3] as u64 > doc.height as u64
        {
            return Err(error());
        }
        Ok(Self {
            x: v[0],
            y: v[1],
            width: v[2],
            height: v[3],
        })
    }
}

pub(crate) fn png_block(img: &image::RgbaImage) -> Result<Value, ToolResult> {
    let png = emulsion_io::export::png8(img.width(), img.height(), img.as_raw())
        .map_err(|e| ToolResult::error(e.to_string()))?;
    Ok(json!({"type": "image", "mimeType": "image/png",
        "data": base64::engine::general_purpose::STANDARD.encode(png)}))
}

pub(crate) fn view(doc: &Document, args: &Value) -> Result<ToolResult, ToolResult> {
    let rect = ViewRegion::parse(doc, args)?;
    if rect.width == 0 || rect.height == 0 {
        return Err(ToolResult::error("empty canvas"));
    }
    let max = match args.get("max_size") {
        None => 1024,
        Some(v) => v
            .as_u64()
            .filter(|v| (64..=1568).contains(v))
            .ok_or_else(|| ToolResult::error("max_size must be an integer from 64 to 1568"))?
            as u32,
    };
    let mut d = doc.clone();
    if let Some(value) = args.get("node") {
        let id = value
            .as_u64()
            .ok_or_else(|| ToolResult::error("node must be an integer"))?;
        if d.node(id).is_none() {
            return Err(ToolResult::error(format!("no node {id}")));
        }
        let keep = d.subtree(id);
        let snapshot = d.clone();
        for n in &mut d.nodes {
            if !(keep.contains(&n.id) || snapshot.is_ancestor(n.id, id)) {
                n.visible = false;
            }
        }
    }
    let ratio = (max as f64 / rect.width.max(rect.height) as f64).min(1.0);
    let w = ((rect.width as f64 * ratio).round() as u32).max(1);
    let h = ((rect.height as f64 * ratio).round() as u32).max(1);
    let mut level = 0;
    while rect.width.max(rect.height).div_ceil(1_u32 << level) > max * 2 {
        level += 1;
    }
    let scale = (1_u32 << level) as f64;
    let tree = d.composite_tree();
    let (lw, lh) = level_size(d.width, d.height, level);
    // One-pixel halo for interpolation. Render only intersecting mip tiles,
    // never a full-resolution allocation proportional to the requested area.
    let x0 = ((rect.x as f64 / scale).floor() as u32).saturating_sub(1);
    let y0 = ((rect.y as f64 / scale).floor() as u32).saturating_sub(1);
    let x1 = (((rect.x + rect.width) as f64 / scale).ceil() as u32 + 1).min(lw);
    let y1 = (((rect.y + rect.height) as f64 / scale).ceil() as u32 + 1).min(lh);
    let bw = x1 - x0;
    let bh = y1 - y0;
    let mut pixels = vec![[0_f32; 4]; bw as usize * bh as usize];
    const TILE: u32 = emulsion_raster::TILE;
    for ty in y0 / TILE..=(y1 - 1) / TILE {
        for tx in x0 / TILE..=(x1 - 1) / TILE {
            let tile = render_tile(&tree, level, TileCoord::new(tx as i32, ty as i32));
            for y in y0.max(ty * TILE)..y1.min((ty + 1) * TILE) {
                for x in x0.max(tx * TILE)..x1.min((tx + 1) * TILE) {
                    pixels[((y - y0) * bw + x - x0) as usize] =
                        tile[((y - ty * TILE) * TILE + x - tx * TILE) as usize];
                }
            }
        }
    }
    let img = image::RgbaImage::from_fn(w, h, |x, y| {
        let sx = ((rect.x as f64 + (x as f64 + 0.5) * rect.width as f64 / w as f64) / scale
            - 0.5
            - x0 as f64)
            .clamp(0.0, (bw - 1) as f64);
        let sy = ((rect.y as f64 + (y as f64 + 0.5) * rect.height as f64 / h as f64) / scale
            - 0.5
            - y0 as f64)
            .clamp(0.0, (bh - 1) as f64);
        let (ix, iy) = (sx.floor() as u32, sy.floor() as u32);
        let (fx, fy) = ((sx - ix as f64) as f32, (sy - iy as f64) as f32);
        let at = |x: u32, y: u32| pixels[(y.min(bh - 1) * bw + x.min(bw - 1)) as usize];
        let (a, b, c, e) = (
            at(ix, iy),
            at(ix + 1, iy),
            at(ix, iy + 1),
            at(ix + 1, iy + 1),
        );
        let p = std::array::from_fn(|i| {
            (a[i] * (1.0 - fx) + b[i] * fx) * (1.0 - fy) + (c[i] * (1.0 - fx) + e[i] * fx) * fy
        });
        image::Rgba(color::premul_to_srgba8(p))
    });
    let mapping = json!({
        "document_size": [doc.width, doc.height], "region": [rect.x, rect.y, rect.width, rect.height],
        "image_size": [w, h], "node": args.get("node"),
        "image_to_document": {"origin": [rect.x, rect.y], "scale": [rect.width as f64 / w as f64, rect.height as f64 / h as f64],
            "convention": "Image edge coordinates: document = origin + image * scale. Pixel (i,j) centre uses (i+0.5,j+0.5)."},
        "mip_level": level
    });
    Ok(ToolResult {
        content: vec![
            png_block(&img)?,
            json!({"type": "text", "text": mapping.to_string()}),
        ],
        is_error: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::{Command, Node, command::Slot};
    use emulsion_raster::{Placement, Raster};
    use std::sync::Arc;

    #[test]
    fn region_maps_native_pixels_and_transformed_node() {
        let mut doc = Document::new(1000, 800);
        Command::AddNode {
            node: Box::new(Node::raster(
                0,
                "red",
                Arc::new(Raster::solid(20, 10, [1.0, 0.0, 0.0, 1.0])),
                Placement::at(207.0, 103.0),
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
        let r = view(&doc, &json!({"region": [200, 100, 80, 40]})).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(r.content[0]["data"].as_str().unwrap())
            .unwrap();
        let img = image::load_from_memory(&bytes).unwrap().into_rgba8();
        assert_eq!(img.dimensions(), (80, 40));
        assert_eq!(img.get_pixel(7, 3).0, [255, 0, 0, 255]);
        assert_eq!(img.get_pixel(6, 3).0, [0, 0, 0, 0]);
        let meta: Value = serde_json::from_str(r.content[1]["text"].as_str().unwrap()).unwrap();
        assert_eq!(meta["image_to_document"]["origin"], json!([200, 100]));
        assert_eq!(meta["image_to_document"]["scale"], json!([1.0, 1.0]));
    }

    #[test]
    fn odd_region_downscale_mapping_and_document_edges() {
        let doc = Document::new(30000, 20000);
        let r = view(
            &doc,
            &json!({"region": [29199, 19399, 801, 601], "max_size": 128}),
        )
        .unwrap();
        let meta: Value = serde_json::from_str(r.content[1]["text"].as_str().unwrap()).unwrap();
        assert_eq!(meta["image_size"], json!([128, 96]));
        assert_eq!(meta["region"], json!([29199, 19399, 801, 601]));
        assert_eq!(
            meta["image_to_document"]["scale"],
            json!([801.0 / 128.0, 601.0 / 96.0])
        );
    }

    #[test]
    fn invalid_regions_and_sizes_are_errors() {
        let doc = Document::new(200, 100);
        for region in [
            json!([-1, 0, 10, 10]),
            json!([0, 0, 0, 10]),
            json!([190, 0, 20, 10]),
            json!([0, 0, 1.5, 2]),
            json!([0, 0, 10]),
            json!([0, 0, 4294967296_u64, 1]),
            Value::Null,
        ] {
            assert!(view(&doc, &json!({"region": region})).is_err());
        }
        assert!(view(&doc, &json!({"max_size": 0})).is_err());
        assert!(view(&doc, &json!({"node": "bad"})).is_err());
    }

    #[test]
    fn node_region_is_document_space_and_excludes_other_nodes() {
        let mut doc = Document::new(100, 100);
        for (name, color, placement) in [
            ("red", [1.0, 0.0, 0.0, 1.0], Placement::at(25.0, 30.0)),
            ("blue", [0.0, 0.0, 1.0, 1.0], Placement::default()),
        ] {
            Command::AddNode {
                node: Box::new(Node::raster(
                    0,
                    name,
                    Arc::new(Raster::solid(100, 100, color)),
                    placement,
                )),
                slot: Slot::TOP,
            }
            .apply(&mut doc)
            .unwrap();
        }
        let r = view(&doc, &json!({"node": 1, "region": [20,25,20,20]})).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(r.content[0]["data"].as_str().unwrap())
            .unwrap();
        let img = image::load_from_memory(&bytes).unwrap().into_rgba8();
        assert_eq!(img.get_pixel(0, 0).0, [0, 0, 0, 0]);
        assert_eq!(img.get_pixel(5, 5).0, [255, 0, 0, 255]);
    }
}
