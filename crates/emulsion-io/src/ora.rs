//! The native format: OpenRaster with an Emulsion manifest.
//!
//! Layout inside the zip:
//!
//! ```text
//! mimetype                  "image/openraster", first entry, stored
//! stack.xml                 standard ORA stack (raster layers and groups)
//! data/node-<id>.png        layer pixels as other ORA readers should see them
//! emulsion/src/node-<id>.png  source pixels of transformed layers
//! emulsion/mask-<id>.png    8-bit masks
//! emulsion.json             the full node stack (adjustments, placements, …)
//! mergedimage.png           full composite
//! Thumbnails/thumbnail.png  composite, at most 256 px
//! history/…                 the history graph (see [`crate::history`])
//! ```
//!
//! Emulsion reads `emulsion.json` when present and falls back to `stack.xml`,
//! so ORA files from Krita, MyPaint or GIMP open too. Other readers see the
//! raster layers and groups and the correct merged image; adjustment nodes
//! exist only in the manifest.
//!
//! Versioning: `version` increments whenever older builds could misread a
//! file. Builds reject any version above the one they know.

use crate::export::{png_gray, png8, png16};
use crate::import::{check_size, from_dynamic};
use crate::{IoError, Result, write_atomic};
use emulsion_core::graph::Graph;
use emulsion_core::node::{Node, NodeKind};
use emulsion_core::{Document, NodeId};
use emulsion_raster::blend::BlendSpace;
use emulsion_raster::composite::{CompositeNode, CompositeTree, NodeContent, flatten, level_size};
use emulsion_raster::{Adjustment, BlendMode, Mask, Placement, Raster};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Seek, Write};
use std::path::Path;
use std::sync::Arc;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub const FORMAT_VERSION: u32 = 1;
const MANIFEST: &str = "emulsion.json";
const MAX_MANIFEST_BYTES: u64 = 4 << 20;
const MAX_ENTRY_BYTES: u64 = 1 << 30;

#[derive(Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    width: u32,
    height: u32,
    resolution: f32,
    source_depth: u8,
    blend_space: BlendSpace,
    /// Bottom to top.
    nodes: Vec<MNode>,
    /// Ruler guides. Absent in files from before guides existed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    guides: Vec<emulsion_core::document::Guide>,
}

#[derive(Serialize, Deserialize)]
struct MNode {
    id: NodeId,
    name: String,
    parent: Option<NodeId>,
    visible: bool,
    locked: bool,
    opacity: f32,
    blend: BlendMode,
    clip_to: Option<NodeId>,
    mask: Option<String>,
    mask_enabled: bool,
    kind: MKind,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum MKind {
    Raster {
        src: String,
        width: u32,
        height: u32,
        placement: Placement,
    },
    Group {
        collapsed: bool,
    },
    Adjust {
        adjustment: Adjustment,
    },
    Fill {
        rgba: [u8; 4],
    },
    Path {
        path: emulsion_raster::vector::Path,
        style: emulsion_raster::vector::PathStyle,
        /// The rasterized path, for readers that only know layers.
        src: String,
    },
    Smart {
        /// Source pixels.
        src: String,
        width: u32,
        height: u32,
        filters: Vec<emulsion_filters::Filter>,
        placement: Placement,
    },
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn is_integer_translation(p: &Placement) -> bool {
    p.scale_x == 1.0
        && p.scale_y == 1.0
        && p.rotation == 0.0
        && !p.flip_x
        && !p.flip_y
        && p.x.fract() == 0.0
        && p.y.fract() == 0.0
}

/// Everything encoded before the zip is written.
struct Encoded {
    entries: Vec<(String, Vec<u8>)>,
    /// Per raster node: (stack.xml src, x, y).
    ora_layers: HashMap<NodeId, (String, i64, i64)>,
    manifest: Manifest,
}

fn raster_png(raster: &Raster, depth: u8) -> Result<Vec<u8>> {
    if depth == 16 {
        png16(raster.width(), raster.height(), &raster.to_srgba16())
    } else {
        png8(raster.width(), raster.height(), &raster.to_srgba8())
    }
}

/// Render a transformed raster node alone, cropped to its placed bounds, for
/// readers that only understand x/y offsets.
fn bake(doc: &Document, raster: &Arc<Raster>, placement: &Placement) -> (Raster, i64, i64) {
    let tree = CompositeTree {
        width: doc.width,
        height: doc.height,
        space: BlendSpace::Linear,
        nodes: vec![CompositeNode {
            id: 0,
            visible: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
            mask: None,
            clip_to: None,
            content: NodeContent::Pixels {
                raster: raster.clone(),
                placement: *placement,
            },
        }],
    };
    let full = flatten(&tree, 0);
    let b = placement
        .doc_bounds(raster.width(), raster.height())
        .intersect(&emulsion_raster::IRect::new(
            0,
            0,
            doc.width as i32,
            doc.height as i32,
        ));
    if b.is_empty() {
        return (Raster::transparent(1, 1), 0, 0);
    }
    let crop = Raster::from_fn(b.w as u32, b.h as u32, [0; 4], |x, y| {
        full.get(x + b.x as u32, y + b.y as u32)
    });
    (crop, b.x as i64, b.y as i64)
}

fn encode(doc: &Document) -> Result<Encoded> {
    enum Job<'a> {
        Png {
            path: String,
            raster: &'a Raster,
        },
        Baked {
            path: String,
            id: NodeId,
            raster: &'a Arc<Raster>,
            placement: Placement,
        },
        Mask {
            path: String,
            mask: &'a Mask,
        },
    }
    let mut jobs = Vec::new();
    let mut nodes = Vec::new();
    let mut ora_layers = HashMap::new();
    for n in &doc.nodes {
        let mask = n.mask.as_ref().map(|m| {
            let path = format!("emulsion/mask-{}.png", n.id);
            jobs.push(Job::Mask {
                path: path.clone(),
                mask: m,
            });
            path
        });
        let kind = match &n.kind {
            NodeKind::Raster { raster, placement } => {
                let data = format!("data/node-{}.png", n.id);
                let src = if is_integer_translation(placement) {
                    jobs.push(Job::Png {
                        path: data.clone(),
                        raster,
                    });
                    ora_layers.insert(n.id, (data.clone(), placement.x as i64, placement.y as i64));
                    data
                } else {
                    let src = format!("emulsion/src/node-{}.png", n.id);
                    jobs.push(Job::Png {
                        path: src.clone(),
                        raster,
                    });
                    jobs.push(Job::Baked {
                        path: data,
                        id: n.id,
                        raster,
                        placement: *placement,
                    });
                    src
                };
                MKind::Raster {
                    src,
                    width: raster.width(),
                    height: raster.height(),
                    placement: *placement,
                }
            }
            NodeKind::Group { collapsed } => MKind::Group {
                collapsed: *collapsed,
            },
            NodeKind::Adjust(a) => MKind::Adjust {
                adjustment: a.clone(),
            },
            NodeKind::Fill { rgba } => MKind::Fill { rgba: *rgba },
            NodeKind::Smart {
                source,
                filters,
                placement,
                cache,
                offset,
            } => {
                // Other readers get the filtered result, placed where it lands.
                let data = format!("data/node-{}.png", n.id);
                let src = format!("emulsion/src/node-{}.png", n.id);
                jobs.push(Job::Png {
                    path: src.clone(),
                    raster: source,
                });
                let cp = emulsion_core::smart::cache_placement(
                    placement,
                    (source.width(), source.height()),
                    (cache.width(), cache.height()),
                    *offset,
                );
                if is_integer_translation(&cp) {
                    jobs.push(Job::Png {
                        path: data.clone(),
                        raster: cache,
                    });
                    ora_layers.insert(n.id, (data, cp.x as i64, cp.y as i64));
                } else {
                    jobs.push(Job::Baked {
                        path: data,
                        id: n.id,
                        raster: cache,
                        placement: cp,
                    });
                }
                MKind::Smart {
                    src,
                    width: source.width(),
                    height: source.height(),
                    filters: filters.clone(),
                    placement: *placement,
                }
            }
            NodeKind::Path { path, style, cache } => {
                let data = format!("data/node-{}.png", n.id);
                jobs.push(Job::Png {
                    path: data.clone(),
                    raster: cache,
                });
                ora_layers.insert(n.id, (data.clone(), 0, 0));
                MKind::Path {
                    path: (**path).clone(),
                    style: *style,
                    src: data,
                }
            }
        };
        nodes.push(MNode {
            id: n.id,
            name: n.name.clone(),
            parent: n.parent,
            visible: n.visible,
            locked: n.locked,
            opacity: n.opacity,
            blend: n.blend,
            clip_to: n.clip_to,
            mask,
            mask_enabled: n.mask_enabled,
            kind,
        });
    }

    let depth = doc.source_depth;
    type Encoded1 = (String, Vec<u8>, Option<(NodeId, i64, i64)>);
    let results: Vec<Result<Encoded1>> = jobs
        .into_par_iter()
        .map(|job| match job {
            Job::Png { path, raster } => Ok((path, raster_png(raster, depth)?, None)),
            Job::Mask { path, mask } => Ok((
                path,
                png_gray(mask.width(), mask.height(), &mask.to_gray8())?,
                None,
            )),
            Job::Baked {
                path,
                id,
                raster,
                placement,
            } => {
                let (r, x, y) = bake(doc, raster, &placement);
                Ok((path, raster_png(&r, depth)?, Some((id, x, y))))
            }
        })
        .collect();
    let mut entries = Vec::new();
    for r in results {
        let (path, bytes, baked) = r?;
        if let Some((id, x, y)) = baked {
            ora_layers.insert(id, (path.clone(), x, y));
        }
        entries.push((path, bytes));
    }

    // Composite and thumbnail.
    let tree = doc.composite_tree();
    let merged = flatten(&tree, 0);
    entries.push((
        "mergedimage.png".into(),
        png8(doc.width, doc.height, &merged.to_srgba8())?,
    ));
    let mut level = 0;
    while level_size(doc.width, doc.height, level)
        .0
        .max(level_size(doc.width, doc.height, level).1)
        > 512
    {
        level += 1;
    }
    let small = flatten(&tree, level);
    let img = image::RgbaImage::from_raw(small.width(), small.height(), small.to_srgba8())
        .expect("sized buffer");
    let (tw, th) = fit(small.width(), small.height(), 256);
    let thumb = image::imageops::thumbnail(&img, tw, th);
    entries.push((
        "Thumbnails/thumbnail.png".into(),
        png8(tw, th, thumb.as_raw())?,
    ));

    let manifest = Manifest {
        format: "emulsion".into(),
        version: FORMAT_VERSION,
        width: doc.width,
        height: doc.height,
        resolution: doc.resolution,
        source_depth: doc.source_depth,
        blend_space: doc.blend_space,
        nodes,
        guides: doc.guides.clone(),
    };
    Ok(Encoded {
        entries,
        ora_layers,
        manifest,
    })
}

fn fit(w: u32, h: u32, max: u32) -> (u32, u32) {
    if w <= max && h <= max {
        return (w.max(1), h.max(1));
    }
    let s = max as f64 / w.max(h) as f64;
    (
        ((w as f64 * s).round() as u32).max(1),
        ((h as f64 * s).round() as u32).max(1),
    )
}

fn stack_xml(doc: &Document, layers: &HashMap<NodeId, (String, i64, i64)>) -> String {
    fn emit(
        doc: &Document,
        parent: Option<NodeId>,
        layers: &HashMap<NodeId, (String, i64, i64)>,
        out: &mut String,
        indent: usize,
    ) {
        // ORA lists the topmost element first.
        for id in doc.children(parent).into_iter().rev() {
            let n = doc.node(id).expect("child");
            let pad = "  ".repeat(indent);
            let vis = if n.visible { "visible" } else { "hidden" };
            match &n.kind {
                NodeKind::Raster { .. } | NodeKind::Path { .. } | NodeKind::Smart { .. } => {
                    let Some((src, x, y)) = layers.get(&id) else {
                        continue;
                    };
                    out.push_str(&format!(
                        "{pad}<layer name=\"{}\" src=\"{}\" x=\"{x}\" y=\"{y}\" opacity=\"{:.4}\" visibility=\"{vis}\" composite-op=\"{}\"/>\n",
                        esc(&n.name),
                        esc(src),
                        n.opacity,
                        n.blend.ora_op()
                    ));
                }
                NodeKind::Group { .. } => {
                    let isolation = if n.blend == BlendMode::PassThrough {
                        "auto"
                    } else {
                        "isolate"
                    };
                    out.push_str(&format!(
                        "{pad}<stack name=\"{}\" opacity=\"{:.4}\" visibility=\"{vis}\" composite-op=\"{}\" isolation=\"{isolation}\">\n",
                        esc(&n.name),
                        n.opacity,
                        n.blend.ora_op()
                    ));
                    emit(doc, Some(id), layers, out, indent + 1);
                    out.push_str(&format!("{pad}</stack>\n"));
                }
                NodeKind::Adjust(_) | NodeKind::Fill { .. } => {}
            }
        }
    }
    let mut out = String::from("<?xml version='1.0' encoding='UTF-8'?>\n");
    out.push_str(&format!(
        "<image version=\"0.0.6\" w=\"{}\" h=\"{}\" xres=\"{}\" yres=\"{}\">\n<stack>\n",
        doc.width, doc.height, doc.resolution as u32, doc.resolution as u32
    ));
    emit(doc, None, layers, &mut out, 1);
    out.push_str("</stack>\n</image>\n");
    out
}

/// Write `doc` to `path` in the native format.
pub fn write(doc: &Document, path: &Path) -> Result<()> {
    write_full(doc, None, path)
}

/// Write `doc` and, when given, its history graph.
pub fn write_full(doc: &Document, graph: Option<&Graph>, path: &Path) -> Result<()> {
    doc.validate()?;
    let enc = encode(doc)?;
    let xml = stack_xml(doc, &enc.ora_layers);
    let manifest =
        serde_json::to_vec_pretty(&enc.manifest).map_err(|e| IoError::Manifest(e.to_string()))?;
    let history = match graph {
        Some(g) => {
            let tip = g.commit(g.head_branch().tip).map(|c| &c.doc);
            let live = (tip == Some(doc)).then(|| crate::history::fingerprint(&manifest));
            crate::history::encode(g, live)?
        }
        None => Vec::new(),
    };
    write_atomic(path, |f| {
        let mut z = ZipWriter::new(std::io::BufWriter::new(f));
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        z.start_file("mimetype", stored)?;
        z.write_all(b"image/openraster")?;
        z.start_file("stack.xml", deflated)?;
        z.write_all(xml.as_bytes())?;
        z.start_file(MANIFEST, deflated)?;
        z.write_all(&manifest)?;
        // PNGs are already compressed.
        for (name, bytes) in &enc.entries {
            z.start_file(
                name.as_str(),
                stored.large_file(bytes.len() as u64 >= u32::MAX as u64),
            )?;
            z.write_all(bytes)?;
        }
        // Raw tiles compress well; favour speed.
        let fast = deflated.compression_level(Some(1));
        for (name, bytes) in &history {
            z.start_file(
                name.as_str(),
                fast.large_file(bytes.len() as u64 >= u32::MAX as u64),
            )?;
            z.write_all(bytes)?;
        }
        z.finish()?.flush()?;
        Ok(())
    })
}

pub(crate) fn read_entry<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    name: &str,
    max: u64,
) -> Result<Vec<u8>> {
    let mut f = zip
        .by_name(name)
        .map_err(|_| IoError::Manifest(format!("missing entry {name}")))?;
    if f.size() > max {
        return Err(IoError::Manifest(format!("entry {name} is too large")));
    }
    let mut out = Vec::with_capacity(f.size() as usize);
    f.by_ref().take(max).read_to_end(&mut out)?;
    Ok(out)
}

fn decode_png(bytes: &[u8]) -> Result<(Raster, u8)> {
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)?;
    let d = from_dynamic(img)?;
    Ok((d.raster, d.depth))
}

fn decode_mask(bytes: &[u8]) -> Result<Mask> {
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)?.into_luma8();
    check_size(img.width(), img.height())?;
    Ok(Mask::from_gray8(img.width(), img.height(), img.as_raw()))
}

/// A native document with its history graph.
pub struct Opened {
    pub doc: Document,
    pub graph: Option<Graph>,
    /// Set when the file's history could not be read; the document itself
    /// opened fine.
    pub history_error: Option<String>,
}

/// Read a native document and its history graph, if it has one.
pub fn read_full(path: &Path) -> Result<Opened> {
    let doc = read(path)?;
    let file = std::fs::File::open(path)?;
    let mut zip = ZipArchive::new(std::io::BufReader::new(file))?;
    let manifest = if zip.by_name(MANIFEST).is_ok() {
        Some(read_entry(&mut zip, MANIFEST, MAX_MANIFEST_BYTES)?)
    } else {
        None
    };
    match crate::history::read(&mut zip) {
        Ok(None) => Ok(Opened {
            doc,
            graph: None,
            history_error: None,
        }),
        Ok(Some(h)) => {
            // Use the exact tip (16-bit, buffers shared with older commits)
            // when this file's live stack was written from it.
            let same =
                h.live.is_some() && h.live == manifest.as_deref().map(crate::history::fingerprint);
            let tip = h
                .graph
                .commit(h.graph.head_branch().tip)
                .map(|c| c.doc.clone());
            let doc = match tip {
                Some(t) if same && t.width == doc.width && t.height == doc.height => t,
                _ => doc,
            };
            Ok(Opened {
                doc,
                graph: Some(h.graph),
                history_error: None,
            })
        }
        Err(e) => {
            tracing::warn!("history graph in {} is unreadable: {e}", path.display());
            Ok(Opened {
                doc,
                graph: None,
                history_error: Some(e.to_string()),
            })
        }
    }
}

/// Read a native document (or any ORA).
pub fn read(path: &Path) -> Result<Document> {
    let file = std::fs::File::open(path)?;
    let mut zip = ZipArchive::new(std::io::BufReader::new(file))?;
    if let Ok(m) = read_entry(&mut zip, "mimetype", 64)
        && m.trim_ascii() != b"image/openraster"
    {
        return Err(IoError::Unsupported("zip is not an OpenRaster file".into()));
    }
    let doc = if zip.by_name(MANIFEST).is_ok() {
        read_manifest(&mut zip)?
    } else {
        read_stack(&mut zip)?
    };
    doc.validate()?;
    Ok(doc)
}

fn read_manifest<R: Read + Seek>(zip: &mut ZipArchive<R>) -> Result<Document> {
    let bytes = read_entry(zip, MANIFEST, MAX_MANIFEST_BYTES)?;
    // Check the version before trusting the rest of the shape.
    let probe: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| IoError::Manifest(e.to_string()))?;
    let version = probe.get("version").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    if version > FORMAT_VERSION {
        return Err(IoError::TooNew(version));
    }
    if version == 0 {
        return Err(IoError::Manifest("missing version".into()));
    }
    let m: Manifest =
        serde_json::from_value(probe).map_err(|e| IoError::Manifest(e.to_string()))?;
    if m.format != "emulsion" {
        return Err(IoError::Manifest(format!("unknown format {:?}", m.format)));
    }
    check_size(m.width, m.height)?;
    if m.nodes.len() > emulsion_core::document::MAX_NODES {
        return Err(IoError::Manifest("too many nodes".into()));
    }

    // Read compressed bytes sequentially, decode in parallel.
    let mut blobs: HashMap<String, Vec<u8>> = HashMap::new();
    for n in &m.nodes {
        if let MKind::Raster { src, .. } | MKind::Smart { src, .. } = &n.kind
            && !blobs.contains_key(src)
        {
            blobs.insert(src.clone(), read_entry(zip, src, MAX_ENTRY_BYTES)?);
        }
        if let Some(mask) = &n.mask
            && !blobs.contains_key(mask)
        {
            blobs.insert(mask.clone(), read_entry(zip, mask, MAX_ENTRY_BYTES)?);
        }
    }
    let rasters: HashMap<String, Result<(Raster, u8)>> = m
        .nodes
        .par_iter()
        .filter_map(|n| match &n.kind {
            MKind::Raster { src, .. } | MKind::Smart { src, .. } => {
                Some((src.clone(), decode_png(&blobs[src])))
            }
            _ => None,
        })
        .collect();
    let masks: HashMap<String, Result<Mask>> = m
        .nodes
        .par_iter()
        .filter_map(|n| n.mask.as_ref().map(|p| (p.clone(), decode_mask(&blobs[p]))))
        .collect();

    let mut doc = Document::new(m.width, m.height);
    doc.resolution = m.resolution;
    doc.source_depth = if m.source_depth == 16 { 16 } else { 8 };
    doc.blend_space = m.blend_space;
    doc.guides = m.guides.clone();
    let mut raster_cache: HashMap<String, Arc<Raster>> = HashMap::new();
    for n in m.nodes {
        let kind = match n.kind {
            MKind::Raster {
                src,
                width,
                height,
                placement,
            } => {
                let r = match raster_cache.get(&src) {
                    Some(r) => r.clone(),
                    None => {
                        let (r, _) = rasters[&src]
                            .as_ref()
                            .map_err(|e| IoError::Manifest(format!("{src}: {e}")))?;
                        let r = Arc::new(r.clone());
                        raster_cache.insert(src.clone(), r.clone());
                        r
                    }
                };
                if r.width() != width || r.height() != height {
                    return Err(IoError::Manifest(format!(
                        "{src} is {}×{}, manifest says {width}×{height}",
                        r.width(),
                        r.height()
                    )));
                }
                NodeKind::Raster {
                    raster: r,
                    placement,
                }
            }
            MKind::Group { collapsed } => NodeKind::Group { collapsed },
            MKind::Adjust { adjustment } => NodeKind::Adjust(adjustment),
            MKind::Fill { rgba } => NodeKind::Fill { rgba },
            MKind::Smart {
                src,
                width,
                height,
                filters,
                placement,
            } => {
                let r = match raster_cache.get(&src) {
                    Some(r) => r.clone(),
                    None => {
                        let (r, _) = rasters[&src]
                            .as_ref()
                            .map_err(|e| IoError::Manifest(format!("{src}: {e}")))?;
                        let r = Arc::new(r.clone());
                        raster_cache.insert(src.clone(), r.clone());
                        r
                    }
                };
                if r.width() != width || r.height() != height {
                    return Err(IoError::Manifest(format!(
                        "{src} is {}×{}, manifest says {width}×{height}",
                        r.width(),
                        r.height()
                    )));
                }
                if filters.len() > 32 {
                    return Err(IoError::Manifest("too many filters".into()));
                }
                let (cache, offset) = emulsion_core::smart::render(&r, &filters);
                NodeKind::Smart {
                    source: r,
                    filters,
                    placement,
                    cache,
                    offset,
                }
            }
            MKind::Path { path, style, .. } => {
                if path.anchor_count() > emulsion_raster::vector::MAX_ANCHORS {
                    return Err(IoError::Manifest("a path has too many anchors".into()));
                }
                let style = style.sanitized();
                let cache = Arc::new(path.rasterize(&style, m.width, m.height));
                NodeKind::Path {
                    path: Arc::new(path),
                    style,
                    cache,
                }
            }
        };
        let mask = match &n.mask {
            None => None,
            Some(p) => {
                let mk = masks[p]
                    .as_ref()
                    .map_err(|e| IoError::Manifest(format!("{p}: {e}")))?;
                let (ew, eh) = match &kind {
                    NodeKind::Raster { raster, .. } => (raster.width(), raster.height()),
                    _ => (m.width, m.height),
                };
                if mk.width() != ew || mk.height() != eh {
                    return Err(IoError::Manifest(format!(
                        "mask {p} does not match its node's size"
                    )));
                }
                Some(Arc::new(mk.clone()))
            }
        };
        doc.nodes.push(Node {
            id: n.id,
            name: n.name,
            parent: n.parent,
            visible: n.visible,
            locked: n.locked,
            opacity: n.opacity,
            blend: n.blend,
            clip_to: n.clip_to,
            mask,
            mask_enabled: n.mask_enabled,
            kind,
        });
        doc.next_id = doc.next_id.max(n.id + 1);
    }
    Ok(doc)
}

/// Fallback for ORA files from other editors.
fn read_stack<R: Read + Seek>(zip: &mut ZipArchive<R>) -> Result<Document> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let xml = read_entry(zip, "stack.xml", MAX_MANIFEST_BYTES)?;
    let mut reader = Reader::from_reader(xml.as_slice());
    reader.config_mut().trim_text(true);

    struct Item {
        name: String,
        attrs: HashMap<String, String>,
        children: Vec<Item>,
        is_stack: bool,
    }
    let mut size = None;
    let mut stack: Vec<Item> = Vec::new();
    let mut root: Option<Item> = None;
    let mut buf = Vec::new();
    let attrs_of = |e: &quick_xml::events::BytesStart| -> Result<HashMap<String, String>> {
        let mut m = HashMap::new();
        for a in e.attributes() {
            let a = a.map_err(|e| IoError::Xml(e.to_string()))?;
            let k = a.key.as_ref().to_string();
            let v = a
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map_err(|e| IoError::Xml(e.to_string()))?
                .into_owned();
            m.insert(k, v);
        }
        Ok(m)
    };
    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| IoError::Xml(e.to_string()))?
        {
            Event::Start(e) | Event::Empty(e) if e.name().as_ref() == "image" => {
                let a = attrs_of(&e)?;
                let w = a.get("w").and_then(|v| v.parse().ok()).unwrap_or(0);
                let h = a.get("h").and_then(|v| v.parse().ok()).unwrap_or(0);
                size = Some((w, h));
            }
            Event::Start(e) if e.name().as_ref() == "stack" => {
                let a = attrs_of(&e)?;
                stack.push(Item {
                    name: a.get("name").cloned().unwrap_or_default(),
                    attrs: a,
                    children: vec![],
                    is_stack: true,
                });
            }
            Event::End(e) if e.name().as_ref() == "stack" => {
                let done = stack
                    .pop()
                    .ok_or_else(|| IoError::Xml("unbalanced stack".into()))?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(done),
                    None => root = Some(done),
                }
            }
            Event::Empty(e) | Event::Start(e) if e.name().as_ref() == "layer" => {
                let a = attrs_of(&e)?;
                let item = Item {
                    name: a.get("name").cloned().unwrap_or_default(),
                    attrs: a,
                    children: vec![],
                    is_stack: false,
                };
                stack
                    .last_mut()
                    .ok_or_else(|| IoError::Xml("layer outside stack".into()))?
                    .children
                    .push(item);
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    let (w, h) = size.ok_or_else(|| IoError::Xml("missing <image>".into()))?;
    check_size(w, h)?;
    let root = root.ok_or_else(|| IoError::Xml("missing root stack".into()))?;

    // Gather layer PNGs.
    fn srcs(item: &Item, out: &mut Vec<String>) {
        for c in &item.children {
            if c.is_stack {
                srcs(c, out);
            } else if let Some(s) = c.attrs.get("src") {
                out.push(s.clone());
            }
        }
    }
    let mut paths = Vec::new();
    srcs(&root, &mut paths);
    let mut blobs = HashMap::new();
    for p in &paths {
        if !blobs.contains_key(p) {
            blobs.insert(p.clone(), read_entry(zip, p, MAX_ENTRY_BYTES)?);
        }
    }
    let decoded: HashMap<String, Result<(Raster, u8)>> = blobs
        .par_iter()
        .map(|(k, v)| (k.clone(), decode_png(v)))
        .collect();

    let mut doc = Document::new(w, h);
    fn build(
        doc: &mut Document,
        item: &Item,
        parent: Option<NodeId>,
        decoded: &HashMap<String, Result<(Raster, u8)>>,
    ) -> Result<()> {
        // ORA lists top first; the document is bottom first.
        for c in item.children.iter().rev() {
            let id = doc.alloc_id();
            let opacity = c
                .attrs
                .get("opacity")
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(1.0)
                .clamp(0.0, 1.0);
            let visible = c
                .attrs
                .get("visibility")
                .map(|v| v != "hidden")
                .unwrap_or(true);
            let op = c
                .attrs
                .get("composite-op")
                .map(String::as_str)
                .unwrap_or("svg:src-over");
            let mut blend = BlendMode::from_ora_op(op).unwrap_or(BlendMode::Normal);
            let kind = if c.is_stack {
                if blend == BlendMode::Normal
                    && c.attrs.get("isolation").map(String::as_str) != Some("isolate")
                {
                    blend = BlendMode::PassThrough;
                }
                NodeKind::Group { collapsed: false }
            } else {
                let src = c.attrs.get("src").cloned().unwrap_or_default();
                let (r, depth) = decoded
                    .get(&src)
                    .ok_or_else(|| IoError::Xml(format!("missing {src}")))?
                    .as_ref()
                    .map_err(|e| IoError::Manifest(format!("{src}: {e}")))?;
                if *depth == 16 {
                    doc.source_depth = 16;
                }
                let x = c
                    .attrs
                    .get("x")
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let y = c
                    .attrs
                    .get("y")
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(0.0);
                NodeKind::Raster {
                    raster: Arc::new(r.clone()),
                    placement: Placement::at(x, y),
                }
            };
            let mut n = Node::new(
                id,
                if c.name.is_empty() {
                    format!("Layer {id}")
                } else {
                    c.name.clone()
                },
                kind,
            );
            n.parent = parent;
            n.opacity = opacity;
            n.visible = visible;
            n.blend = blend;
            doc.nodes.push(n);
            if c.is_stack {
                build(doc, c, Some(id), decoded)?;
            }
        }
        Ok(())
    }
    build(&mut doc, &root, None, &decoded)?;
    doc.normalize();
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::Command;
    use emulsion_core::command::Slot;

    fn sample_doc() -> Document {
        let mut d = Document::new(300, 200);
        // Pixels come from an 8-bit source, as every 8-bit import does.
        let rgba: Vec<u8> = (0..200u32)
            .flat_map(|y| {
                (0..300u32).flat_map(move |x| [(x % 256) as u8, (y + 30) as u8, 150, 255])
            })
            .collect();
        let bg = Raster::from_srgba8(300, 200, &rgba);
        let add = |d: &mut Document, n: Node| {
            Command::AddNode {
                node: Box::new(n),
                slot: Slot::TOP,
            }
            .apply(d)
            .unwrap()
            .unwrap()
        };
        let bg = add(
            &mut d,
            Node::raster(0, "background & sky", Arc::new(bg), Placement::default()),
        );
        let mut spot = Node::raster(
            0,
            "spot",
            Arc::new(Raster::from_srgba8(
                40,
                40,
                &[255u8, 0, 0, 128].repeat(40 * 40),
            )),
            Placement::at(20.0, 30.0),
        );
        spot.blend = BlendMode::Multiply;
        spot.mask = Some(Arc::new(Mask::from_fn(40, 40, 255, |x, _| (x * 6) as u8)));
        let spot = add(&mut d, spot);
        let mut scaled = Node::raster(
            0,
            "scaled",
            Arc::new(Raster::solid(64, 64, [0.0, 0.0, 1.0, 1.0])),
            Placement::default(),
        );
        if let NodeKind::Raster { placement, .. } = &mut scaled.kind {
            *placement = Placement {
                x: 100.0,
                y: 50.0,
                scale_x: 0.5,
                scale_y: 0.5,
                rotation: 30.0,
                flip_x: true,
                flip_y: false,
            };
        }
        let scaled = add(&mut d, scaled);
        let g = Command::Group {
            ids: vec![spot, scaled],
            name: "group".into(),
        }
        .apply(&mut d)
        .unwrap()
        .unwrap();
        Command::SetOpacity {
            id: g,
            opacity: 0.8,
        }
        .apply(&mut d)
        .unwrap();
        Command::SetClip {
            id: scaled,
            clip_to: Some(spot),
        }
        .apply(&mut d)
        .unwrap();
        let a = add(
            &mut d,
            Node::adjust(
                0,
                Adjustment::Exposure {
                    exposure: 0.5,
                    offset: 0.0,
                    gamma: 1.0,
                },
            ),
        );
        Command::SetVisible {
            id: a,
            visible: false,
        }
        .apply(&mut d)
        .unwrap();
        let _ = bg;
        d
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("emulsion-io-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn native_roundtrip_preserves_everything() {
        let mut d = sample_doc();
        d.guides = vec![
            emulsion_core::document::Guide {
                vertical: true,
                pos: 150.5,
            },
            emulsion_core::document::Guide {
                vertical: false,
                pos: 40.0,
            },
        ];
        let p = tmp("roundtrip.ora");
        write(&d, &p).unwrap();
        let back = read(&p).unwrap();
        assert_eq!(back.guides, d.guides);
        assert_eq!(back.nodes.len(), d.nodes.len());
        for (a, b) in d.nodes.iter().zip(&back.nodes) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.name, b.name);
            assert_eq!(a.parent, b.parent);
            assert_eq!(a.visible, b.visible);
            assert_eq!(a.opacity, b.opacity);
            assert_eq!(a.blend, b.blend);
            assert_eq!(a.clip_to, b.clip_to);
            assert_eq!(a.mask.is_some(), b.mask.is_some());
            match (&a.kind, &b.kind) {
                (
                    NodeKind::Raster {
                        raster: ra,
                        placement: pa,
                    },
                    NodeKind::Raster {
                        raster: rb,
                        placement: pb,
                    },
                ) => {
                    assert_eq!(pa, pb);
                    assert_eq!(ra.to_srgba8(), rb.to_srgba8(), "pixels survive");
                }
                (x, y) => assert_eq!(x, y),
            }
        }
        // The composite is identical after a round trip.
        let a = flatten(&d.composite_tree(), 0).to_srgba8();
        let b = flatten(&back.composite_tree(), 0).to_srgba8();
        assert_eq!(a, b);
    }

    #[test]
    fn sixteen_bit_roundtrip_is_within_one_code() {
        let mut d = Document::new(64, 64);
        d.source_depth = 16;
        let r = Raster::from_fn(64, 64, [0; 4], |x, y| {
            [(x * 1000) as u16, (y * 1000) as u16, 777, 65535]
        });
        Command::AddNode {
            node: Box::new(Node::raster(
                0,
                "deep",
                Arc::new(r.clone()),
                Placement::default(),
            )),
            slot: Slot::TOP,
        }
        .apply(&mut d)
        .unwrap();
        let p = tmp("deep.ora");
        write(&d, &p).unwrap();
        let back = read(&p).unwrap();
        assert_eq!(back.source_depth, 16);
        let NodeKind::Raster { raster, .. } = &back.nodes[0].kind else {
            panic!()
        };
        let (a, b) = (r.to_srgba16(), raster.to_srgba16());
        assert!(a.iter().zip(&b).all(|(x, y)| x.abs_diff(*y) <= 1));
    }

    #[test]
    fn plain_ora_readers_path_works() {
        // Strip the manifest and read through stack.xml alone.
        let d = sample_doc();
        let p = tmp("plain.ora");
        write(&d, &p).unwrap();
        let mut src = ZipArchive::new(std::fs::File::open(&p).unwrap()).unwrap();
        let q = tmp("plain-stripped.ora");
        let mut out = ZipWriter::new(std::fs::File::create(&q).unwrap());
        for i in 0..src.len() {
            let mut f = src.by_index(i).unwrap();
            if f.name() == MANIFEST {
                continue;
            }
            let name = f.name().to_string();
            let mut bytes = Vec::new();
            f.read_to_end(&mut bytes).unwrap();
            out.start_file(name, SimpleFileOptions::default()).unwrap();
            out.write_all(&bytes).unwrap();
        }
        out.finish().unwrap();
        let back = read(&q).unwrap();
        // Adjustment nodes are manifest-only; rasters and the group survive.
        assert_eq!(back.nodes.len(), 4);
        let g = back.nodes.iter().find(|n| n.is_group()).unwrap();
        assert_eq!(g.name, "group");
        assert_eq!(back.children(Some(g.id)).len(), 2);
        let spot = back.nodes.iter().find(|n| n.name == "spot").unwrap();
        assert_eq!(spot.blend, BlendMode::Multiply);
    }

    #[test]
    fn rejects_newer_versions_and_bad_sizes() {
        let d = sample_doc();
        let p = tmp("newer.ora");
        write(&d, &p).unwrap();
        // Rewrite the manifest with a future version.
        let mut src = ZipArchive::new(std::fs::File::open(&p).unwrap()).unwrap();
        let q = tmp("newer2.ora");
        let mut out = ZipWriter::new(std::fs::File::create(&q).unwrap());
        for i in 0..src.len() {
            let mut f = src.by_index(i).unwrap();
            let name = f.name().to_string();
            let mut bytes = Vec::new();
            f.read_to_end(&mut bytes).unwrap();
            if name == MANIFEST {
                let mut v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                v["version"] = serde_json::json!(FORMAT_VERSION + 1);
                bytes = serde_json::to_vec(&v).unwrap();
            }
            out.start_file(name, SimpleFileOptions::default()).unwrap();
            out.write_all(&bytes).unwrap();
        }
        out.finish().unwrap();
        assert!(matches!(read(&q), Err(IoError::TooNew(_))));
    }

    #[test]
    fn mimetype_is_first_and_stored() {
        let p = tmp("mime.ora");
        write(&sample_doc(), &p).unwrap();
        let mut z = ZipArchive::new(std::fs::File::open(&p).unwrap()).unwrap();
        let f = z.by_index(0).unwrap();
        assert_eq!(f.name(), "mimetype");
        assert_eq!(f.compression(), CompressionMethod::Stored);
    }

    #[test]
    fn history_round_trips_with_shared_buffers() {
        use emulsion_core::Editor;
        use std::collections::HashMap;
        let doc = sample_doc();
        let first = doc.nodes[0].id;
        let mut e = Editor::new(doc, None);
        e.execute(Command::SetOpacity {
            id: first,
            opacity: 0.5,
        })
        .unwrap();
        e.branch("warm").unwrap();
        e.execute(Command::Rename {
            id: first,
            name: "warm sky".into(),
        })
        .unwrap();
        e.commit("Warmer", false);
        e.checkout("main").unwrap();
        e.execute(Command::SetVisible {
            id: first,
            visible: false,
        })
        .unwrap();
        e.commit("Saved", false);
        let path = tmp("history.ora");
        write_full(&e.doc, Some(&e.graph), &path).unwrap();

        let o = read_full(&path).unwrap();
        assert!(o.history_error.is_none());
        let g = o.graph.expect("graph");
        assert_eq!(g.len(), e.graph.len());
        assert_eq!(g.head(), "main");
        assert_eq!(g.branches().keys().collect::<Vec<_>>(), ["main", "warm"]);
        // The live document is the exact head tip, so it shares buffers.
        let tip = &g.commit(g.head_branch().tip).unwrap().doc;
        assert!(o.doc == *tip);
        // Commits that shared a raster still share it after reading.
        let rasters: Vec<_> = g
            .commits()
            .filter_map(|c| match &c.doc.nodes[0].kind {
                NodeKind::Raster { raster, .. } => Some(Arc::as_ptr(raster)),
                _ => None,
            })
            .collect();
        assert!(rasters.windows(2).all(|w| w[0] == w[1]));
        // And the reopened graph merges exactly like the original.
        let mut e2 = emulsion_core::Editor::with_graph(o.doc, Some(path.clone()), g);
        let emulsion_core::graph::MergeOutcome::Merged(m) =
            e2.merge("warm", &HashMap::new()).unwrap()
        else {
            panic!("clean merge expected");
        };
        let n = m.node(first).unwrap();
        assert!(n.name == "warm sky" && !n.visible && n.opacity == 0.5);
    }

    #[test]
    fn damaged_history_still_opens_the_document() {
        let doc = sample_doc();
        let e = emulsion_core::Editor::new(doc.clone(), None);
        let path = tmp("damaged-history.ora");
        write_full(&doc, Some(&e.graph), &path).unwrap();
        // Rewrite the zip with a broken graph.
        let bytes = std::fs::read(&path).unwrap();
        let mut zin = ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut out = ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for i in 0..zin.len() {
            let mut f = zin.by_index(i).unwrap();
            let name = f.name().to_string();
            let mut data = Vec::new();
            f.read_to_end(&mut data).unwrap();
            if name == crate::history::GRAPH {
                data = br#"{"format":"emulsion-history","version":1,"head":"main","branches":{},"live":null,"rasters":[],"masks":[],"commits":[]}"#.to_vec();
            }
            out.start_file(name, SimpleFileOptions::default()).unwrap();
            out.write_all(&data).unwrap();
        }
        std::fs::write(&path, out.finish().unwrap().into_inner()).unwrap();
        let o = read_full(&path).unwrap();
        assert!(o.graph.is_none());
        assert!(o.history_error.is_some());
        assert_eq!(o.doc.nodes.len(), doc.nodes.len());
    }
}
