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
//! emulsion/paths/*.bin      exact editable geometry, shared with history
//! mergedimage.png           full composite
//! Thumbnails/thumbnail.png  composite, at most 256 px
//! history/…                 the history graph (see [`crate::history`])
//! ```
//!
//! Emulsion reads `emulsion.json` when present and falls back to `stack.xml`,
//! so ORA files from Krita, MyPaint or GIMP open too. Other readers see the
//! raster layers and groups when representable. Documents with masks,
//! clipping, adjustments, fills, or styles expose a named merged appearance
//! layer to other readers; their editable originals remain in the manifest.
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

pub const FORMAT_VERSION: u32 = 3;
const MANIFEST: &str = "emulsion.json";
// Editable geometry can be large, especially in legacy pretty-printed files.
// Keep the much smaller generic ORA XML limit separate.
pub(crate) const MAX_NATIVE_MANIFEST_BYTES: u64 = 512 << 20;
const MAX_STACK_BYTES: u64 = 4 << 20;
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
    /// Camera metadata from the source photograph.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    info: Option<emulsion_core::document::ImageInfo>,
}

#[derive(Serialize, Deserialize)]
struct MNode {
    id: NodeId,
    name: String,
    parent: Option<NodeId>,
    visible: bool,
    locked: bool,
    #[serde(default)]
    locks: emulsion_core::node::LayerLocks,
    #[serde(default)]
    color_label: emulsion_core::node::LayerColor,
    opacity: f32,
    blend: BlendMode,
    #[serde(default)]
    blending: emulsion_raster::composite::BlendingOptions,
    clip_to: Option<NodeId>,
    mask: Option<String>,
    #[serde(default = "default_mask_fill")]
    mask_fill: u8,
    mask_enabled: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    styles: Vec<emulsion_core::styles::LayerStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    origin: Option<String>,
    kind: MKind,
}

fn default_mask_fill() -> u8 {
    255
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
        path: crate::path_data::PathData,
        style: emulsion_raster::vector::PathStyle,
        /// The rasterized path, for readers that only know layers.
        src: String,
    },
    Text {
        spec: emulsion_core::text::TextSpec,
        /// The rasterized text, for readers that only know layers.
        src: String,
    },
    Smart {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        editable: Option<emulsion_core::node::SmartEditable>,
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
            blending: Default::default(),
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

fn encode(doc: &Document, paths: &mut crate::path_data::PathPool) -> Result<Encoded> {
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
                editable,
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
                    editable: editable.clone(),
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
                    path: paths.add(path)?,
                    style: *style,
                    src: data,
                }
            }
            NodeKind::Text { spec, cache } => {
                let data = format!("data/node-{}.png", n.id);
                jobs.push(Job::Png {
                    path: data.clone(),
                    raster: cache,
                });
                ora_layers.insert(n.id, (data.clone(), 0, 0));
                MKind::Text {
                    spec: (**spec).clone(),
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
            locks: n.locks,
            color_label: n.color_label,
            opacity: n.opacity,
            blend: n.blend,
            blending: n.blending,
            clip_to: n.clip_to,
            mask,
            mask_fill: n.mask.as_ref().map_or(255, |m| m.fill()),
            mask_enabled: n.mask_enabled,
            styles: n.styles.clone(),
            origin: n.origin.clone(),
            kind,
        });
    }

    let depth = doc.source_depth;
    type Encoded1 = (String, Vec<u8>, Option<(NodeId, i64, i64)>);
    // The layer PNGs and the merged composite are independent; encode
    // them side by side.
    type Extras = Result<Vec<(String, Vec<u8>)>>;
    let (results, merged): (Vec<Result<Encoded1>>, Extras) = rayon::join(
        || {
            jobs.into_par_iter()
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
                .collect()
        },
        || {
            // Composite and thumbnail. When the picture is one untouched
            // layer, the merged image would only repeat that layer's PNG
            // (a fifth of the file for a 16-bit photo), so it is left out;
            // stack.xml already points readers at the layer itself.
            let tree = doc.composite_tree();
            let mut out = Vec::new();
            if !merged_is_redundant(doc) {
                let merged = flatten(&tree, 0);
                out.push((
                    "mergedimage.png".to_string(),
                    png8(doc.width, doc.height, &merged.to_srgba8())?,
                ));
            }
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
            out.push((
                "Thumbnails/thumbnail.png".to_string(),
                png8(tw, th, thumb.as_raw())?,
            ));
            Ok(out)
        },
    );
    let mut entries = Vec::new();
    for r in results {
        let (path, bytes, baked) = r?;
        if let Some((id, x, y)) = baked {
            ora_layers.insert(id, (path.clone(), x, y));
        }
        entries.push((path, bytes));
    }
    entries.extend(merged?);

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
        info: doc.info.clone(),
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

/// Is the composite exactly the one and only layer's own pixels?
fn merged_is_redundant(doc: &Document) -> bool {
    let [n] = doc.nodes.as_slice() else {
        return false;
    };
    let NodeKind::Raster { raster, placement } = &n.kind else {
        return false;
    };
    n.visible
        && n.opacity >= 1.0
        && n.blend == emulsion_raster::BlendMode::Normal
        && n.mask.is_none()
        && n.styles.is_empty()
        && n.blending == Default::default()
        && *placement == Placement::default()
        && raster.width() == doc.width
        && raster.height() == doc.height
}

fn stack_xml(doc: &Document, layers: &HashMap<NodeId, (String, i64, i64)>) -> String {
    // ORA cannot encode these operations. Other applications must see the
    // complete picture, while Emulsion keeps every editable node in its
    // native manifest. Name the fallback explicitly instead of pretending
    // the standard stack contains the editable original layers.
    if doc.nodes.iter().any(|n| {
        matches!(n.kind, NodeKind::Adjust(_) | NodeKind::Fill { .. })
            || n.clip_to.is_some()
            || n.mask.is_some()
            || !n.styles.is_empty()
            || n.blending != Default::default()
    }) {
        return format!(
            "<?xml version='1.0' encoding='UTF-8'?>\n<image version=\"0.0.6\" w=\"{}\" h=\"{}\" xres=\"{}\" yres=\"{}\"><stack><layer name=\"Appearance (editable layers in Emulsion)\" src=\"mergedimage.png\" x=\"0\" y=\"0\" opacity=\"1\" visibility=\"visible\" composite-op=\"svg:src-over\"/></stack></image>\n",
            doc.width, doc.height, doc.resolution as u32, doc.resolution as u32
        );
    }
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
                NodeKind::Raster { .. }
                | NodeKind::Path { .. }
                | NodeKind::Text { .. }
                | NodeKind::Smart { .. } => {
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
    let mut paths = crate::path_data::PathPool::default();
    let enc = encode(doc, &mut paths)?;
    let xml = stack_xml(doc, &enc.ora_layers);
    let manifest =
        serde_json::to_vec(&enc.manifest).map_err(|e| IoError::Manifest(e.to_string()))?;
    if manifest.len() as u64 > MAX_NATIVE_MANIFEST_BYTES {
        return Err(IoError::Manifest(format!(
            "entry {MANIFEST} exceeds the supported {} MiB limit",
            MAX_NATIVE_MANIFEST_BYTES >> 20
        )));
    }
    let history = match graph {
        Some(g) => {
            let tip = g.commit(g.head_branch().tip).map(|c| &c.doc);
            let live = (tip == Some(doc)).then(|| crate::history::fingerprint(&manifest));
            let working = live
                .is_none()
                .then(|| (doc, crate::history::fingerprint(&manifest)));
            crate::history::encode(g, live, working, &mut paths)?
        }
        None => Vec::new(),
    };
    write_atomic(path, |f| {
        let mut z = ZipWriter::new(std::io::BufWriter::new(f));
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .compression_level(Some(6));
        z.start_file("mimetype", stored)?;
        z.write_all(b"image/openraster")?;
        z.start_file("stack.xml", deflated)?;
        z.write_all(xml.as_bytes())?;
        z.start_file(MANIFEST, deflated)?;
        z.write_all(&manifest)?;
        // Fast PNG encoding leaves useful redundancy; ZIP compression is lossless.
        for (name, bytes) in &enc.entries {
            z.start_file(
                name.as_str(),
                deflated.large_file(bytes.len() as u64 >= u32::MAX as u64),
            )?;
            z.write_all(bytes)?;
        }
        for (name, bytes) in paths.entries() {
            z.start_file(name, deflated)?;
            z.write_all(&bytes)?;
        }
        // Raw tiles favour speed; the small history index uses stronger compression.
        let fast = deflated.compression_level(Some(1));
        for (name, bytes) in &history {
            z.start_file(
                name.as_str(),
                (if name == crate::history::GRAPH {
                    deflated
                } else {
                    fast
                })
                .large_file(bytes.len() as u64 >= u32::MAX as u64),
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
        Some(crate::history::fingerprint(&read_entry(
            &mut zip,
            MANIFEST,
            MAX_NATIVE_MANIFEST_BYTES,
        )?))
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
            // Recover the exact current work (16-bit, shared buffers) only
            // when its fingerprint matches this file's live stack. Older files
            // and documents saved directly at a version use the tip instead.
            let same = h.live.is_some() && h.live == manifest;
            let tip = h
                .graph
                .commit(h.graph.head_branch().tip)
                .map(|c| c.doc.clone());
            let exact = h
                .working
                .filter(|(fingerprint, _)| Some(fingerprint) == manifest.as_ref())
                .map(|(_, doc)| doc)
                .or_else(|| same.then_some(tip).flatten());
            let doc = match exact {
                Some(t) if t.width == doc.width && t.height == doc.height => t,
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
    let bytes = read_entry(zip, MANIFEST, MAX_NATIVE_MANIFEST_BYTES)?;
    // Probe only the version: a generic Value tree duplicates every path
    // coordinate and costs far more memory than the editable geometry itself.
    #[derive(Deserialize)]
    struct VersionProbe {
        #[serde(default)]
        version: u32,
    }
    let version = serde_json::from_slice::<VersionProbe>(&bytes)
        .map_err(|e| IoError::Manifest(e.to_string()))?
        .version;
    if version > FORMAT_VERSION {
        return Err(IoError::TooNew(version));
    }
    if version == 0 {
        return Err(IoError::Manifest("missing version".into()));
    }
    let m: Manifest =
        serde_json::from_slice(&bytes).map_err(|e| IoError::Manifest(e.to_string()))?;
    drop(bytes);
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
    doc.info = m.info.clone();
    let mut raster_cache: HashMap<String, Arc<Raster>> = HashMap::new();
    let mut paths = crate::path_data::PathReader::default();
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
                editable,
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
                    editable,
                    source: r,
                    filters,
                    placement,
                    cache,
                    offset,
                }
            }
            MKind::Path { path, style, .. } => {
                let path = paths.read(path, zip)?;
                if path.anchor_count() > emulsion_raster::vector::MAX_ANCHORS {
                    return Err(IoError::Manifest("a path has too many anchors".into()));
                }
                let style = style.sanitized();
                let cache = Arc::new(path.rasterize(&style, m.width, m.height));
                NodeKind::Path { path, style, cache }
            }
            MKind::Text { spec, .. } => {
                let spec = spec.sanitized();
                let cache = Arc::new(emulsion_core::text::rasterize(&spec, m.width, m.height));
                NodeKind::Text {
                    spec: Arc::new(spec),
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
                    NodeKind::Smart { source, .. } => (source.width(), source.height()),
                    _ => (m.width, m.height),
                };
                let mk = Mask::from_pixels(mk.width(), mk.height(), n.mask_fill, &mk.to_gray8());
                // v1/v2 UI created document-space masks on smart nodes, but
                // raster-to-smart conversion retained source-sized masks.
                // Source dimensions take precedence in the ambiguous equal-
                // size case, matching the renderer used by those versions.
                let mk = if m.version < 3
                    && (mk.width(), mk.height()) != (ew, eh)
                    && let NodeKind::Smart { placement, .. } = &kind
                    && (mk.width(), mk.height()) == (m.width, m.height)
                {
                    let to_doc = placement.to_doc(ew, eh);
                    Mask::from_fn(ew, eh, mk.fill(), |x, y| {
                        let p =
                            to_doc.transform_point2(glam::dvec2(x as f64 + 0.5, y as f64 + 0.5));
                        if p.x < 0.0
                            || p.y < 0.0
                            || p.x >= mk.width() as f64
                            || p.y >= mk.height() as f64
                        {
                            mk.fill()
                        } else {
                            mk.get(p.x.floor() as u32, p.y.floor() as u32)
                        }
                    })
                } else {
                    mk
                };
                if mk.width() != ew || mk.height() != eh {
                    return Err(IoError::Manifest(format!(
                        "mask {p} does not match its node's size"
                    )));
                }
                Some(Arc::new(mk))
            }
        };
        doc.nodes.push(Node {
            id: n.id,
            name: n.name,
            parent: n.parent,
            visible: n.visible,
            locked: n.locked,
            locks: n.locks,
            color_label: n.color_label,
            opacity: n.opacity,
            blend: n.blend,
            blending: n.blending,
            clip_to: n.clip_to,
            mask,
            mask_enabled: n.mask_enabled,
            styles: n.styles,
            origin: n.origin,
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

    let xml = read_entry(zip, "stack.xml", MAX_STACK_BYTES)?;
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

    fn rewrite_archive(path: &Path, mut edit: impl FnMut(&str, Vec<u8>) -> Option<Vec<u8>>) {
        let mut src = ZipArchive::new(std::io::Cursor::new(std::fs::read(path).unwrap())).unwrap();
        let mut dst = ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for i in 0..src.len() {
            let mut entry = src.by_index(i).unwrap();
            let name = entry.name().to_string();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            if let Some(bytes) = edit(&name, bytes) {
                dst.start_file(name, SimpleFileOptions::default()).unwrap();
                dst.write_all(&bytes).unwrap();
            }
        }
        std::fs::write(path, dst.finish().unwrap().into_inner()).unwrap();
    }

    fn masked_smart_document() -> Document {
        let mut doc = Document::new(12, 10);
        let mut node = Node::raster(
            0,
            "Small masked source",
            Arc::new(Raster::solid(4, 4, [1.0, 0.0, 0.0, 1.0])),
            Placement::at(3.0, 2.0),
        );
        node.mask = Some(Arc::new(Mask::from_fn(4, 4, 0, |x, _| {
            if x < 2 { 255 } else { 0 }
        })));
        let id = emulsion_core::Command::AddNode {
            node: Box::new(node),
            slot: emulsion_core::command::Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        emulsion_core::Command::ConvertToSmart { id }
            .apply(&mut doc)
            .unwrap();
        doc
    }

    #[test]
    fn masked_smart_native_and_legacy_source_masks_reopen_with_history() {
        let doc = masked_smart_document();
        let editor = emulsion_core::Editor::new(doc.clone(), None);
        let path = tmp("smart-mask-history.ora");
        write_full(&doc, Some(&editor.graph), &path).unwrap();
        let reopened = read_full(&path).unwrap();
        assert!(reopened.history_error.is_none());
        assert!(reopened.graph.is_some());
        assert_eq!(
            flatten(&doc.composite_tree(), 0).to_srgba8(),
            flatten(&reopened.doc.composite_tree(), 0).to_srgba8()
        );
        assert_eq!(reopened.doc.nodes[0].mask.as_ref().unwrap().fill(), 0);
        // Old ConvertToSmart wrote exactly this smaller source-sized mask.
        rewrite_archive(&path, |name, bytes| {
            if name == MANIFEST {
                let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                value["version"] = 2.into();
                Some(serde_json::to_vec(&value).unwrap())
            } else {
                Some(bytes)
            }
        });
        let legacy = read_full(&path).unwrap();
        assert!(legacy.history_error.is_none());
        assert!(legacy.graph.is_some());
        assert_eq!(legacy.doc.nodes[0].mask.as_ref().unwrap().width(), 4);
        assert_eq!(
            flatten(&doc.composite_tree(), 0).to_srgba8(),
            flatten(&legacy.doc.composite_tree(), 0).to_srgba8()
        );
    }

    #[test]
    fn advanced_blending_round_trips_with_history_and_legacy_defaults() {
        let mut doc = masked_smart_document();
        doc.nodes[0].blending.fill_opacity = 0.35;
        doc.nodes[0].blending.channels = [true, false, true];
        doc.nodes[0].blending.blend_if.source.black_fade = 0.2;
        let expected = doc.nodes[0].blending;
        let editor = emulsion_core::Editor::new(doc.clone(), None);
        let path = tmp("advanced-blending.ora");
        write_full(&doc, Some(&editor.graph), &path).unwrap();
        let reopened = read_full(&path).unwrap();
        assert_eq!(reopened.doc.nodes[0].blending, expected);
        assert!(reopened.history_error.is_none());
        for commit in reopened.graph.unwrap().commits() {
            assert_eq!(commit.doc.nodes[0].blending, expected);
        }
        assert_eq!(
            flatten(&doc.composite_tree(), 0).to_srgba8(),
            flatten(&reopened.doc.composite_tree(), 0).to_srgba8()
        );
        assert!(stack_xml(&doc, &HashMap::new()).contains("Appearance"));
        rewrite_archive(&path, |name, bytes| {
            if name == MANIFEST {
                let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                for node in value["nodes"].as_array_mut().unwrap() {
                    node.as_object_mut().unwrap().remove("blending");
                }
                Some(serde_json::to_vec(&value).unwrap())
            } else {
                Some(bytes)
            }
        });
        assert_eq!(
            read_full(&path).unwrap().doc.nodes[0].blending,
            Default::default()
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn legacy_document_sized_smart_masks_migrate_into_source_coordinates() {
        let doc = masked_smart_document();
        let path = tmp("smart-legacy-canvas-mask.ora");
        write(&doc, &path).unwrap();
        let mask = Mask::from_fn(12, 10, 0, |x, _| if x == 4 { 255 } else { 0 });
        rewrite_archive(&path, |name, bytes| {
            if name == MANIFEST {
                let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                value["version"] = 2.into();
                Some(serde_json::to_vec(&value).unwrap())
            } else if name.starts_with("emulsion/mask-") {
                Some(png_gray(12, 10, &mask.to_gray8()).unwrap())
            } else {
                Some(bytes)
            }
        });
        let restored = read(&path).unwrap();
        let mask = restored.nodes[0].mask.as_ref().unwrap();
        assert_eq!((mask.width(), mask.height()), (4, 4));
        assert_eq!(mask.get(0, 0), 0);
        assert_eq!(mask.get(1, 0), 255);
        assert_eq!(mask.get(2, 0), 0);
    }

    #[test]
    fn legacy_smart_canvas_masks_in_history_are_migrated_without_losing_commits() {
        let mut doc = masked_smart_document();
        doc.nodes[0].mask = Some(Arc::new(Mask::white(4, 4)));
        let editor = emulsion_core::Editor::new(doc.clone(), None);
        let path = tmp("smart-legacy-history-canvas-mask.ora");
        write_full(&doc, Some(&editor.graph), &path).unwrap();
        rewrite_archive(&path, |name, bytes| {
            if name == MANIFEST {
                let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                value["version"] = 2.into();
                Some(serde_json::to_vec(&value).unwrap())
            } else if name.starts_with("emulsion/mask-") {
                Some(png_gray(12, 10, &[255; 120]).unwrap())
            } else if name == crate::history::GRAPH {
                let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                value["version"] = 2.into();
                for mask in value["masks"].as_array_mut().unwrap() {
                    mask["width"] = 12.into();
                    mask["height"] = 10.into();
                }
                Some(serde_json::to_vec(&value).unwrap())
            } else {
                Some(bytes)
            }
        });
        let restored = read_full(&path).unwrap();
        assert!(
            restored.history_error.is_none(),
            "{:?}",
            restored.history_error
        );
        let graph = restored.graph.unwrap();
        assert_eq!(graph.commits().count(), editor.graph.commits().count());
        for commit in graph.commits() {
            let mask = commit.doc.nodes[0].mask.as_ref().unwrap();
            assert_eq!((mask.width(), mask.height()), (4, 4));
            assert_eq!(mask.get(0, 0), 255);
            commit.doc.validate().unwrap();
        }
    }

    #[test]
    fn standard_ora_fallback_keeps_appearance_and_native_keeps_editability() {
        let mut doc = masked_smart_document();
        let fill = Node::new(
            0,
            "Fill",
            NodeKind::Fill {
                rgba: [0, 100, 200, 128],
            },
        );
        emulsion_core::Command::AddNode {
            node: Box::new(fill),
            slot: emulsion_core::command::Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
        emulsion_core::Command::AddNode {
            node: Box::new(Node::adjust(
                0,
                Adjustment::Exposure {
                    exposure: 1.0,
                    offset: 0.0,
                    gamma: 1.0,
                },
            )),
            slot: emulsion_core::command::Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
        let path = tmp("standard-fidelity.ora");
        write(&doc, &path).unwrap();
        let native = read(&path).unwrap();
        assert_eq!(native.nodes.len(), 3);
        assert!(matches!(native.nodes[0].kind, NodeKind::Smart { .. }));
        assert!(matches!(native.nodes[2].kind, NodeKind::Adjust(_)));
        rewrite_archive(&path, |name, bytes| {
            if name == MANIFEST || name.starts_with("emulsion/") {
                None
            } else {
                Some(bytes)
            }
        });
        let standard = read(&path).unwrap();
        assert_eq!(standard.nodes.len(), 1);
        assert!(standard.nodes[0].name.contains("Appearance"));
        let expected = flatten(&doc.composite_tree(), 0).to_srgba8();
        let actual = flatten(&standard.composite_tree(), 0).to_srgba8();
        assert!(
            expected
                .iter()
                .zip(actual)
                .all(|(&a, b)| a.abs_diff(b) <= 1)
        );
    }

    #[test]
    fn legacy_manifest_above_four_mib_keeps_editable_paths() {
        let mut doc = Document::new(8, 8);
        let path =
            Arc::new(emulsion_raster::vector::Path::from_svg("M 1 1 L 7 1 L 1 7 Z").unwrap());
        Command::AddNode {
            node: Box::new(Node::path(
                0,
                "Drawing",
                path.clone(),
                Default::default(),
                8,
                8,
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
        let saved = tmp("large-legacy-manifest.ora");
        write(&doc, &saved).unwrap();
        let mut input = ZipArchive::new(std::fs::File::open(&saved).unwrap()).unwrap();
        let mut archive = ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for index in 0..input.len() {
            let mut entry = input.by_index(index).unwrap();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            if entry.name() == MANIFEST {
                assert!(!data.contains(&b'\n'), "new manifests use compact JSON");
                let mut legacy: serde_json::Value = serde_json::from_slice(&data).unwrap();
                legacy["version"] = serde_json::json!(1);
                legacy["nodes"][0]["kind"]["path"] = serde_json::to_value(path.as_ref()).unwrap();
                data = serde_json::to_vec(&legacy).unwrap();
                // Valid legacy formatting above the former 4 MiB limit.
                data.extend(std::iter::repeat_n(b' ', (4 << 20) + 1));
            }
            archive
                .start_file(
                    entry.name(),
                    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
                )
                .unwrap();
            archive.write_all(&data).unwrap();
        }
        std::fs::write(&saved, archive.finish().unwrap().into_inner()).unwrap();
        let loaded = read_full(&saved).unwrap();
        assert!(loaded.history_error.is_none());
        let NodeKind::Path { path: reopened, .. } = &loaded.doc.nodes[0].kind else {
            panic!("large native files must retain editable geometry");
        };
        assert_eq!(reopened.as_ref(), path.as_ref());
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
    fn rotated_text_and_paths_roundtrip_as_editable_native_content() {
        use emulsion_core::text::TextSpec;
        use emulsion_raster::vector::{Path, PathStyle};
        let legacy: TextSpec =
            serde_json::from_value(serde_json::json!({"text": "Old document"})).unwrap();
        assert_eq!(
            legacy.rotation, 0.0,
            "older text metadata defaults to no rotation"
        );
        let mut doc = Document::new(240, 180);
        let path_id = Command::AddNode {
            node: Box::new(Node::path(
                0,
                "Arrow",
                Arc::new(
                    Path::from_svg("M 35 45 L 95 45 L 95 30 L 125 60 L 95 90 L 95 75 L 35 75 Z")
                        .unwrap(),
                ),
                PathStyle {
                    stroke: None,
                    fill: Some([20, 60, 180, 255]),
                    width: 0.0,
                },
                240,
                180,
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        let text_id = Command::AddNode {
            node: Box::new(Node::text(
                0,
                "Editable label",
                TextSpec {
                    text: "Turn".into(),
                    x: 135.0,
                    y: 55.0,
                    size: 24.0,
                    ..TextSpec::default()
                },
                240,
                180,
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        Command::RotateNode {
            id: path_id,
            degrees: 30.0,
        }
        .apply(&mut doc)
        .unwrap();
        Command::RotateNode {
            id: text_id,
            degrees: -25.0,
        }
        .apply(&mut doc)
        .unwrap();
        let NodeKind::Text { spec, .. } = &doc.node(text_id).unwrap().kind else {
            panic!()
        };
        assert!((spec.rotation.rem_euclid(360.0) - 335.0).abs() < 1e-5);
        let file = tmp("rotated-editable-content.ora");
        write(&doc, &file).unwrap();
        let mut restored = read(&file).unwrap();
        for id in [path_id, text_id] {
            match (
                &doc.node(id).unwrap().kind,
                &restored.node(id).unwrap().kind,
            ) {
                (
                    NodeKind::Path {
                        path: a, style: sa, ..
                    },
                    NodeKind::Path {
                        path: b, style: sb, ..
                    },
                ) => {
                    assert_eq!(a, b);
                    assert_eq!(sa, sb);
                }
                (NodeKind::Text { spec: a, .. }, NodeKind::Text { spec: b, .. }) => {
                    assert_eq!(a, b)
                }
                _ => panic!("native content must remain editable after reopening"),
            }
        }
        assert_eq!(
            flatten(&doc.composite_tree(), 0).to_srgba8(),
            flatten(&restored.composite_tree(), 0).to_srgba8()
        );
        let NodeKind::Text { spec, .. } = &restored.node(text_id).unwrap().kind else {
            panic!()
        };
        let mut edited = (**spec).clone();
        edited.text = "Still editable".into();
        Command::SetText {
            id: text_id,
            spec: Box::new(edited),
        }
        .apply(&mut restored)
        .unwrap();
        let NodeKind::Text { spec, .. } = &restored.node(text_id).unwrap().kind else {
            panic!()
        };
        assert_eq!(spec.text, "Still editable");
        assert!((spec.rotation.rem_euclid(360.0) - 335.0).abs() < 1e-5);
        let NodeKind::Path { path, style, .. } = &restored.node(path_id).unwrap().kind else {
            panic!()
        };
        let mut edited = (**path).clone();
        edited.subpaths[0].anchors[0].p.0 += 2.0;
        let expected = edited.subpaths[0].anchors[0].p;
        let style = *style;
        Command::SetPath {
            id: path_id,
            path: Arc::new(edited),
            style,
        }
        .apply(&mut restored)
        .unwrap();
        let NodeKind::Path { path, .. } = &restored.node(path_id).unwrap().kind else {
            panic!()
        };
        assert_eq!(path.subpaths[0].anchors[0].p, expected);
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
        // A representable stack remains layered for ordinary ORA readers.
        // Advanced operations have a separate appearance-fallback regression.
        let mut d = sample_doc();
        d.nodes.retain(|n| !matches!(n.kind, NodeKind::Adjust(_)));
        for node in &mut d.nodes {
            node.mask = None;
            node.clip_to = None;
        }
        d.validate().unwrap();
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
        // Rasters, transforms, blend modes, and group structure survive.
        assert_eq!(back.nodes.len(), 4);
        let g = back.nodes.iter().find(|n| n.is_group()).unwrap();
        assert_eq!(g.name, "group");
        assert_eq!(back.children(Some(g.id)).len(), 2);
        let spot = back.nodes.iter().find(|n| n.name == "spot").unwrap();
        assert_eq!(spot.blend, BlendMode::Multiply);
        let expected = flatten(&d.composite_tree(), 0).to_srgba8();
        let actual = flatten(&back.composite_tree(), 0).to_srgba8();
        assert!(
            expected
                .iter()
                .zip(actual)
                .all(|(&a, b)| a.abs_diff(b) <= 2)
        );
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
    fn working_snapshot_preserves_unsaved_version_edits_without_growing_history() {
        use emulsion_core::{Editor, text::TextSpec};
        use emulsion_raster::{IRect, TileCoord};
        let mut doc = Document::new(520, 64);
        let original = Arc::new(Raster::from_fn(520, 64, [0; 4], |x, _| {
            [1234 + x as u16, 2345, 3456, 54321]
        }));
        doc.nodes.push(Node::raster(
            1,
            "Pixels",
            original.clone(),
            Placement::default(),
        ));
        doc.nodes.push(Node::text(
            2,
            "Caption",
            TextSpec {
                text: "Before".into(),
                size: 18.0,
                ..Default::default()
            },
            520,
            64,
        ));
        doc.next_id = 3;
        let mut editor = Editor::new(doc, None);
        let version_count = editor.graph.len();
        let changed =
            Arc::new(original.write_rect(IRect::new(0, 0, 1, 1), &[[1111, 2222, 3333, 44444]]));
        editor
            .execute(Command::ReplacePixels {
                id: 1,
                raster: changed.clone(),
                dirty: IRect::new(0, 0, 1, 1),
                label: "Paint".into(),
            })
            .unwrap();
        let text = TextSpec {
            text: "Current work".into(),
            color: [191, 40, 80, 255],
            size: 18.0,
            ..Default::default()
        };
        editor
            .execute(Command::SetText {
                id: 2,
                spec: Box::new(text.clone()),
            })
            .unwrap();
        let selection = Arc::new(Mask::from_fn(520, 64, 0, |x, y| {
            if x < 30 && y < 20 { 123 } else { 0 }
        }));
        editor
            .execute(Command::SetSelection {
                selection: Some(selection.clone()),
            })
            .unwrap();
        let path = tmp("working-snapshot.ora");
        let mut tile_sizes = None;
        for _ in 0..3 {
            write_full(&editor.doc, Some(&editor.graph), &path).unwrap();
            // Compressed JSON size can vary with tile ordering. Compare the
            // stored tile payload instead: repeated saves must not accumulate it.
            let mut archive = ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
            let mut sizes: Vec<_> = (0..archive.len())
                .filter_map(|i| {
                    let entry = archive.by_index(i).unwrap();
                    entry
                        .name()
                        .starts_with("history/tiles/")
                        .then_some(entry.size())
                })
                .collect();
            sizes.sort_unstable();
            assert_eq!(
                tile_sizes.get_or_insert_with(|| sizes.clone()),
                &sizes,
                "saving replaces the working snapshot"
            );
            let opened = read_full(&path).unwrap();
            assert!(opened.history_error.is_none());
            let graph = opened.graph.unwrap();
            assert_eq!(graph.len(), version_count);
            let NodeKind::Raster { raster, .. } = &opened.doc.node(1).unwrap().kind else {
                panic!("raster");
            };
            for y in 0..64 {
                for x in 0..520 {
                    assert_eq!(raster.get(x, y), changed.get(x, y));
                }
            }
            let NodeKind::Text { spec, .. } = &opened.doc.node(2).unwrap().kind else {
                panic!("editable text");
            };
            assert_eq!(**spec, text);
            let restored_selection = opened.doc.selection.as_ref().unwrap();
            for y in 0..64 {
                for x in 0..520 {
                    assert_eq!(restored_selection.get(x, y), selection.get(x, y));
                }
            }
            let tip = &graph.commit(graph.head_branch().tip).unwrap().doc;
            let NodeKind::Raster {
                raster: checkpoint, ..
            } = &tip.node(1).unwrap().kind
            else {
                panic!("checkpoint raster");
            };
            assert_eq!(checkpoint.get(0, 0), original.get(0, 0));
            // The untouched second tile is shared by the working copy and the checkpoint.
            let tile = |image: &Raster| {
                image
                    .base_tiles()
                    .find(|(coord, _)| **coord == TileCoord::new(1, 0))
                    .unwrap()
                    .1
                    .as_ptr()
            };
            assert_eq!(tile(raster), tile(checkpoint));
            editor = Editor::with_graph(opened.doc, Some(path.clone()), graph);
        }
        // An external manifest edit must never resurrect a stale working copy.
        rewrite_archive(&path, |name, data| {
            if name == MANIFEST {
                let mut manifest: serde_json::Value = serde_json::from_slice(&data).unwrap();
                manifest["nodes"][1]["name"] = serde_json::json!("Externally renamed");
                Some(serde_json::to_vec(&manifest).unwrap())
            } else {
                Some(data)
            }
        });
        let opened = read_full(&path).unwrap();
        assert!(opened.history_error.is_none());
        assert_eq!(opened.doc.nodes[1].name, "Externally renamed");
        assert_eq!(opened.graph.unwrap().len(), version_count);
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

    #[test]
    fn smart_text_retains_editable_source_in_live_document_and_versions() {
        use emulsion_core::{Command, text::TextSpec};
        let mut editor = emulsion_core::history::Editor::new(Document::new(100, 60), None);
        let id = editor
            .execute(Command::AddNode {
                node: Box::new(Node::text(
                    0,
                    "Type",
                    TextSpec {
                        text: "Editable".into(),
                        size: 14.0,
                        ..Default::default()
                    },
                    100,
                    60,
                )),
                slot: emulsion_core::command::Slot::TOP,
            })
            .unwrap()
            .unwrap();
        editor.execute(Command::ConvertToSmart { id }).unwrap();
        editor.create_version("Smart type");
        let path = tmp("smart-editable-text.ora");
        crate::save_full(&editor.doc, &editor.graph, &path).unwrap();
        let mut opened = read_full(&path).unwrap();
        assert!(
            matches!(&opened.doc.node(id).unwrap().kind, NodeKind::Smart { editable: Some(emulsion_core::node::SmartEditable::Text { spec }), .. } if spec.text == "Editable")
        );
        let saved = opened.graph.as_ref().unwrap().commits().last().unwrap();
        assert!(
            matches!(&saved.doc.node(id).unwrap().kind, NodeKind::Smart { editable: Some(emulsion_core::node::SmartEditable::Text { spec }), .. } if spec.text == "Editable")
        );
        Command::ConvertToLayers { id }
            .apply(&mut opened.doc)
            .unwrap();
        assert!(
            matches!(&opened.doc.node(id).unwrap().kind, NodeKind::Text { spec, .. } if spec.text == "Editable")
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn layer_locks_and_color_survive_native_and_version_roundtrip() {
        use emulsion_core::{
            Command,
            node::{LayerColor, LayerLocks},
        };
        let mut editor = emulsion_core::history::Editor::new(Document::new(4, 4), None);
        let id = editor
            .execute(Command::AddNode {
                node: Box::new(Node::raster(
                    0,
                    "Locked",
                    Arc::new(Raster::transparent(4, 4)),
                    Placement::default(),
                )),
                slot: emulsion_core::command::Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let locks = LayerLocks {
            transparency: true,
            pixels: true,
            position: true,
        };
        editor
            .execute(Command::SetLayerLocks { id, locks })
            .unwrap();
        editor
            .execute(Command::SetColorLabel {
                id,
                color: LayerColor::Violet,
            })
            .unwrap();
        editor.create_version("Labeled locks");
        let path = tmp("layer-lock-label.ora");
        crate::save_full(&editor.doc, &editor.graph, &path).unwrap();
        let opened = read_full(&path).unwrap();
        assert_eq!(opened.doc.node(id).unwrap().locks, locks);
        assert_eq!(opened.doc.node(id).unwrap().color_label, LayerColor::Violet);
        let version = opened.graph.unwrap();
        let last = version.commits().last().unwrap();
        assert_eq!(last.doc.node(id).unwrap().locks, locks);
        assert_eq!(last.doc.node(id).unwrap().color_label, LayerColor::Violet);
        let _ = std::fs::remove_file(path);
    }
}
