//! The history graph inside a native file.
//!
//! ```text
//! history/graph.json      commits, branches, and which planes each uses
//! history/tiles/r<n>      one 256×256 RGBA16 tile, little-endian, deflated
//! history/tiles/m<n>      one 256×256 8-bit mask tile, deflated
//! emulsion/paths/<n>.bin  compact geometry shared with the live document
//! ```
//!
//! Commits share pixels the way they do in memory: each distinct tile is
//! stored once however many commits or the current working copy use it, and each distinct plane
//! (raster, mask, selection) is listed once and referenced by index. On
//! reading, shared entries become shared buffers again, so unchanged nodes
//! still compare equal across commits and merges stay precise.
//!
//! The graph is optional. Files without it open with a fresh history, and
//! a damaged graph never stops the document itself from opening.

use crate::path_data::{PathData, PathPool, PathReader};
use crate::{IoError, Result};
use emulsion_core::NodeId;
use emulsion_core::document::Document;
use emulsion_core::graph::{Branch, Commit, CommitId, Graph, MAX_COMMITS};
use emulsion_core::node::{Node, NodeKind};
use emulsion_raster::blend::BlendSpace;
use emulsion_raster::image::{Pix, Plane, Tile};
use emulsion_raster::{Adjustment, BlendMode, Mask, Placement, Raster, TILE_PX, TileCoord};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Seek};
use std::sync::Arc;
use zip::ZipArchive;

// History snapshots retain native shape paints and stroke geometry as of version 5.
// Version 6 records the complete editable rich-text model in undo snapshots.
pub const HISTORY_VERSION: u32 = 6;
pub(crate) const GRAPH: &str = "history/graph.json";
const MAX_GRAPH_BYTES: u64 = crate::ora::MAX_NATIVE_MANIFEST_BYTES;

/// Tile pixels as bytes.
trait TileBytes: Pix {
    const SIZE: usize;
    const PREFIX: &'static str;
    fn put(px: &[Self], out: &mut Vec<u8>);
    fn get(b: &[u8]) -> Self;
}

impl TileBytes for [u16; 4] {
    const SIZE: usize = 8;
    const PREFIX: &'static str = "history/tiles/r";
    fn put(px: &[Self], out: &mut Vec<u8>) {
        for p in px {
            for c in p {
                out.extend_from_slice(&c.to_le_bytes());
            }
        }
    }
    fn get(b: &[u8]) -> Self {
        [0, 1, 2, 3].map(|i| u16::from_le_bytes([b[2 * i], b[2 * i + 1]]))
    }
}

impl TileBytes for u8 {
    const SIZE: usize = 1;
    const PREFIX: &'static str = "history/tiles/m";
    fn put(px: &[Self], out: &mut Vec<u8>) {
        out.extend_from_slice(px);
    }
    fn get(b: &[u8]) -> Self {
        b[0]
    }
}

#[derive(Serialize, Deserialize)]
struct HFile {
    format: String,
    version: u32,
    head: String,
    branches: BTreeMap<String, HBranch>,
    /// Hash of the file's emulsion.json when the live document is the head
    /// tip, so a reader can take the exact tip instead of re-decoded PNGs.
    live: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    working: Option<HWorking>,
    rasters: Vec<HPlane<[u16; 4]>>,
    masks: Vec<HPlane<u8>>,
    commits: Vec<HCommit>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    patterns: Vec<Arc<emulsion_core::style_options::PatternImage>>,
}

/// The current saved work, independent of deliberately created versions.
#[derive(Serialize, Deserialize)]
struct HWorking {
    live: String,
    doc: HDoc,
}

#[derive(Serialize, Deserialize)]
struct HBranch {
    tip: CommitId,
    base: CommitId,
}

#[derive(Serialize, Deserialize)]
struct HPlane<F> {
    width: u32,
    height: u32,
    fill: F,
    /// (tile x, tile y, blob number).
    tiles: Vec<(i32, i32, u32)>,
}

#[derive(Serialize, Deserialize)]
struct HCommit {
    id: CommitId,
    parents: Vec<CommitId>,
    name: String,
    time: u64,
    auto: bool,
    branch: String,
    doc: HDoc,
}

#[derive(Serialize, Deserialize)]
struct HDoc {
    width: u32,
    height: u32,
    resolution: f32,
    #[serde(default)]
    global_light: emulsion_core::style_options::GlobalLight,
    source_depth: u8,
    blend_space: BlendSpace,
    next_id: NodeId,
    selection: Option<u32>,
    nodes: Vec<HNode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    guides: Vec<emulsion_core::document::Guide>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    info: Option<emulsion_core::document::ImageInfo>,
}

#[derive(Serialize, Deserialize)]
struct HNode {
    id: NodeId,
    name: String,
    parent: Option<NodeId>,
    visible: bool,
    locked: bool,
    #[serde(default)]
    locks: emulsion_core::node::LayerLocks,
    #[serde(default)]
    color_label: emulsion_core::node::LayerColor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    link_group: Option<NodeId>,
    opacity: f32,
    blend: BlendMode,
    #[serde(default)]
    blending: emulsion_raster::composite::BlendingOptions,
    clip_to: Option<NodeId>,
    mask: Option<u32>,
    mask_enabled: bool,
    #[serde(default = "emulsion_core::node::default_mask_linked")]
    mask_linked: bool,
    #[serde(default = "emulsion_core::node::default_mask_transform")]
    mask_transform: [f64; 6],
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    styles: Vec<emulsion_core::styles::LayerStyle>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    style_options: Vec<emulsion_core::style_options::StyleOptions>,
    #[serde(default = "emulsion_core::node::default_effects_enabled")]
    effects_enabled: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pattern_refs: Vec<Option<u32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    origin: Option<String>,
    kind: HKind,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum HKind {
    Raster {
        raster: u32,
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
        path: PathData,
        style: emulsion_raster::vector::PathStyle,
    },
    Text {
        spec: emulsion_core::text::TextSpec,
    },
    Smart {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        editable: Option<emulsion_core::node::SmartEditable>,
        source: u32,
        cache: u32,
        offset: (i32, i32),
        filters: Vec<emulsion_filters::Filter>,
        placement: Placement,
    },
}

/// Planes and tiles seen so far, keyed by buffer address.
struct Pool<P: TileBytes> {
    planes: HashMap<usize, u32>,
    list: Vec<HPlane<P>>,
    tiles: HashMap<usize, u32>,
    blobs: Vec<Tile<P>>,
}

impl<P: TileBytes> Pool<P> {
    fn new() -> Self {
        Self {
            planes: HashMap::new(),
            list: Vec::new(),
            tiles: HashMap::new(),
            blobs: Vec::new(),
        }
    }

    fn add(&mut self, plane: &Arc<Plane<P>>) -> u32 {
        let key = Arc::as_ptr(plane) as usize;
        if let Some(i) = self.planes.get(&key) {
            return *i;
        }
        let mut tiles: Vec<(i32, i32, u32)> = plane
            .base_tiles()
            .map(|(c, t)| {
                let n = self.blobs.len() as u32;
                let blob = *self.tiles.entry(t.as_ptr() as usize).or_insert_with(|| {
                    self.blobs.push(t.clone());
                    n
                });
                (c.x, c.y, blob)
            })
            .collect();
        tiles.sort_unstable();
        let i = self.list.len() as u32;
        self.list.push(HPlane {
            width: plane.width(),
            height: plane.height(),
            fill: plane.fill(),
            tiles,
        });
        self.planes.insert(key, i);
        i
    }

    fn entries(&self) -> Vec<(String, Vec<u8>)> {
        use rayon::prelude::*;
        self.blobs
            .par_iter()
            .enumerate()
            .map(|(i, t)| {
                let mut b = Vec::with_capacity(TILE_PX * P::SIZE);
                P::put(t, &mut b);
                (format!("{}{i}", P::PREFIX), b)
            })
            .collect()
    }
}

/// FNV-1a, to tie the graph to the manifest it was written with.
pub(crate) fn fingerprint(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Encode `graph`. `live` is the manifest fingerprint when the saved
/// document equals the head branch's tip. Otherwise `working` preserves the
/// exact current document and its own fingerprint without creating a commit.
pub(crate) fn encode(
    graph: &Graph,
    live: Option<String>,
    working: Option<(&Document, String)>,
    paths: &mut PathPool,
) -> Result<Vec<(String, Vec<u8>)>> {
    let mut rasters = Pool::<[u16; 4]>::new();
    let mut masks = Pool::<u8>::new();
    let mut patterns = Vec::new();
    let mut pattern_ids = HashMap::new();
    let mut encode_doc = |d: &Document| -> Result<HDoc> {
        let nodes = d
            .nodes
            .iter()
            .map(|n| {
                let mut style_options = n.style_options.clone();
                let pattern_refs = style_options
                    .iter_mut()
                    .map(|option| {
                        option.pattern.image.take().map(|image| {
                            let key = Arc::as_ptr(&image) as usize;
                            *pattern_ids.entry(key).or_insert_with(|| {
                                let id = patterns.len() as u32;
                                patterns.push(image);
                                id
                            })
                        })
                    })
                    .collect();
                Ok(HNode {
                    id: n.id,
                    name: n.name.clone(),
                    parent: n.parent,
                    visible: n.visible,
                    locked: n.locked,
                    locks: n.locks,
                    color_label: n.color_label,
                    link_group: n.link_group,
                    opacity: n.opacity,
                    blend: n.blend,
                    blending: n.blending,
                    clip_to: n.clip_to,
                    mask: n.mask.as_ref().map(|m| masks.add(m)),
                    mask_enabled: n.mask_enabled,
                    mask_linked: n.mask_linked,
                    mask_transform: n.mask_transform,
                    styles: n.styles.clone(),
                    style_options,
                    effects_enabled: n.effects_enabled,
                    pattern_refs,
                    origin: n.origin.clone(),
                    kind: match &n.kind {
                        NodeKind::Raster { raster, placement } => HKind::Raster {
                            raster: rasters.add(raster),
                            placement: *placement,
                        },
                        NodeKind::Group { collapsed } => HKind::Group {
                            collapsed: *collapsed,
                        },
                        NodeKind::Adjust(a) => HKind::Adjust {
                            adjustment: a.clone(),
                        },
                        NodeKind::Fill { rgba } => HKind::Fill { rgba: *rgba },
                        NodeKind::Smart {
                            editable,
                            source,
                            filters,
                            placement,
                            cache,
                            offset,
                        } => HKind::Smart {
                            editable: editable.clone(),
                            source: rasters.add(source),
                            cache: rasters.add(cache),
                            offset: *offset,
                            filters: filters.clone(),
                            placement: *placement,
                        },
                        NodeKind::Path { path, style, .. } => HKind::Path {
                            path: paths.add(path)?,
                            style: *style,
                        },
                        NodeKind::Text { spec, .. } => HKind::Text {
                            spec: (**spec).clone(),
                        },
                    },
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(HDoc {
            width: d.width,
            height: d.height,
            resolution: d.resolution,
            global_light: d.global_light,
            source_depth: d.source_depth,
            blend_space: d.blend_space,
            next_id: d.next_id,
            selection: d.selection.as_ref().map(|s| masks.add(s)),
            nodes,
            guides: d.guides.clone(),
            info: d.info.clone(),
        })
    };
    let commits = graph
        .commits()
        .map(|c| {
            Ok(HCommit {
                id: c.id,
                parents: c.parents.clone(),
                name: c.name.clone(),
                time: c.time,
                auto: c.auto,
                branch: c.branch.clone(),
                doc: encode_doc(&c.doc)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let working = working
        .map(|(doc, live)| {
            Ok::<_, IoError>(HWorking {
                live,
                doc: encode_doc(doc)?,
            })
        })
        .transpose()?;
    let mut entries = rasters.entries();
    entries.extend(masks.entries());
    let file = HFile {
        format: "emulsion-history".into(),
        version: HISTORY_VERSION,
        head: graph.head().into(),
        branches: graph
            .branches()
            .iter()
            .map(|(k, b)| {
                (
                    k.clone(),
                    HBranch {
                        tip: b.tip,
                        base: b.base,
                    },
                )
            })
            .collect(),
        live,
        working,
        rasters: rasters.list,
        masks: masks.list,
        commits,
        patterns,
    };
    let json = serde_json::to_vec(&file).map_err(|e| IoError::Manifest(e.to_string()))?;
    if json.len() as u64 > MAX_GRAPH_BYTES {
        return Err(IoError::Manifest(format!("entry {GRAPH} is too large")));
    }
    entries.insert(0, (GRAPH.into(), json));
    Ok(entries)
}

fn read_tiles<R: Read + Seek, P: TileBytes>(
    zip: &mut ZipArchive<R>,
    planes: &[HPlane<P>],
) -> Result<Vec<Arc<Plane<P>>>> {
    let mut blobs: HashMap<u32, Tile<P>> = HashMap::new();
    let mut out = Vec::with_capacity(planes.len());
    for p in planes {
        crate::import::check_size(p.width, p.height)?;
        let mut tiles = Vec::with_capacity(p.tiles.len());
        for (x, y, blob) in &p.tiles {
            let t = match blobs.get(blob) {
                Some(t) => t.clone(),
                None => {
                    let name = format!("{}{blob}", P::PREFIX);
                    let bytes = crate::ora::read_entry(zip, &name, (TILE_PX * P::SIZE) as u64)?;
                    if bytes.len() != TILE_PX * P::SIZE {
                        return Err(IoError::Manifest(format!("{name} has the wrong size")));
                    }
                    // `as_chunks` needs a literal size; P::SIZE is generic.
                    #[allow(clippy::chunks_exact_to_as_chunks)]
                    let t: Tile<P> = bytes.chunks_exact(P::SIZE).map(P::get).collect();
                    blobs.insert(*blob, t.clone());
                    t
                }
            };
            tiles.push((TileCoord::new(*x, *y), t));
        }
        let plane = Plane::from_tiles(p.width, p.height, p.fill, tiles)
            .ok_or_else(|| IoError::Manifest("a history tile lies outside its image".into()))?;
        out.push(Arc::new(plane));
    }
    Ok(out)
}

/// What the graph says about the live document.
pub(crate) struct ReadGraph {
    pub graph: Graph,
    /// Manifest fingerprint recorded when the live document was the tip.
    pub live: Option<String>,
    pub working: Option<(String, Document)>,
}

/// Read the graph if the file has one. Every reference is checked.
pub(crate) fn read<R: Read + Seek>(zip: &mut ZipArchive<R>) -> Result<Option<ReadGraph>> {
    if zip.by_name(GRAPH).is_err() {
        return Ok(None);
    }
    let bytes = crate::ora::read_entry(zip, GRAPH, MAX_GRAPH_BYTES)?;
    // Skip other fields without constructing a second copy of all editable
    // path geometry as a generic JSON tree.
    #[derive(Deserialize)]
    struct Version {
        #[serde(default)]
        version: u32,
    }
    let probe: Version =
        serde_json::from_slice(&bytes).map_err(|e| IoError::Manifest(e.to_string()))?;
    let version = probe.version;
    if version > HISTORY_VERSION {
        return Err(IoError::TooNew(version));
    }
    let f: HFile = serde_json::from_slice(&bytes).map_err(|e| IoError::Manifest(e.to_string()))?;
    drop(bytes);
    if f.format != "emulsion-history" || version == 0 {
        return Err(IoError::Manifest("not an Emulsion history graph".into()));
    }
    if f.commits.len() > MAX_COMMITS {
        return Err(IoError::Manifest("too many commits".into()));
    }
    let rasters: Vec<Arc<Raster>> = read_tiles(zip, &f.rasters)?;
    let masks: Vec<Arc<Mask>> = read_tiles(zip, &f.masks)?;
    let raster = |i: u32| {
        rasters
            .get(i as usize)
            .cloned()
            .ok_or_else(|| IoError::Manifest(format!("raster {i} is missing")))
    };
    let mask = |i: u32, w: u32, h: u32| -> Result<Arc<Mask>> {
        let m = masks
            .get(i as usize)
            .cloned()
            .ok_or_else(|| IoError::Manifest(format!("mask {i} is missing")))?;
        if m.width() != w || m.height() != h {
            return Err(IoError::Manifest(format!(
                "mask {i} does not match its node"
            )));
        }
        Ok(m)
    };

    let mut paths = PathReader::default();
    let mut decode_doc = |h: HDoc| -> Result<Document> {
        crate::import::check_size(h.width, h.height)?;
        if h.nodes.len() > emulsion_core::document::MAX_NODES {
            return Err(IoError::Manifest("too many nodes".into()));
        }
        let mut doc = Document::new(h.width, h.height);
        doc.resolution = h.resolution;
        doc.global_light = h.global_light;
        doc.source_depth = if h.source_depth == 16 { 16 } else { 8 };
        doc.blend_space = h.blend_space;
        doc.guides = h.guides;
        doc.info = h.info;
        doc.selection = h
            .selection
            .map(|i| mask(i, h.width, h.height))
            .transpose()?;
        for mut n in h.nodes {
            if n.pattern_refs.len() > n.style_options.len() {
                return Err(IoError::Manifest(
                    "style pattern references do not match effects".into(),
                ));
            }
            for (option, reference) in n.style_options.iter_mut().zip(n.pattern_refs) {
                if let Some(id) = reference {
                    option.pattern.image =
                        Some(f.patterns.get(id as usize).cloned().ok_or_else(|| {
                            IoError::Manifest(format!("pattern {id} is missing"))
                        })?);
                }
            }
            let kind = match n.kind {
                HKind::Raster {
                    raster: i,
                    placement,
                } => NodeKind::Raster {
                    raster: raster(i)?,
                    placement,
                },
                HKind::Group { collapsed } => NodeKind::Group { collapsed },
                HKind::Adjust { adjustment } => NodeKind::Adjust(adjustment),
                HKind::Fill { rgba } => NodeKind::Fill { rgba },
                HKind::Smart {
                    editable,
                    source,
                    cache,
                    offset,
                    filters,
                    placement,
                } => NodeKind::Smart {
                    editable,
                    source: raster(source)?,
                    cache: raster(cache)?,
                    offset,
                    filters,
                    placement,
                },
                HKind::Text { spec } => {
                    let spec = spec.sanitized();
                    let cache = Arc::new(emulsion_core::text::rasterize(&spec, h.width, h.height));
                    NodeKind::Text {
                        spec: Arc::new(spec),
                        cache,
                    }
                }
                HKind::Path { path, style } => {
                    let path = paths.read(path, zip)?;
                    if path.anchor_count() > emulsion_raster::vector::MAX_ANCHORS {
                        return Err(IoError::Manifest("a path has too many anchors".into()));
                    }
                    let style = style.sanitized();
                    let cache = Arc::new(path.rasterize(&style, h.width, h.height));
                    NodeKind::Path { path, style, cache }
                }
            };
            let (mw, mh) = match &kind {
                NodeKind::Raster { raster, .. } => (raster.width(), raster.height()),
                NodeKind::Smart { source, .. } => (source.width(), source.height()),
                _ => (h.width, h.height),
            };
            let node_mask = n
                .mask
                .map(|i| {
                    if version < 3
                        && let NodeKind::Smart { placement, .. } = &kind
                        && let Some(old) = masks.get(i as usize)
                        && (old.width(), old.height()) != (mw, mh)
                        && (old.width(), old.height()) == (h.width, h.height)
                    {
                        let to_doc = placement.to_doc(mw, mh);
                        return Ok(Arc::new(Mask::from_fn(mw, mh, old.fill(), |x, y| {
                            let p = to_doc
                                .transform_point2(glam::dvec2(x as f64 + 0.5, y as f64 + 0.5));
                            if p.x < 0.0
                                || p.y < 0.0
                                || p.x >= old.width() as f64
                                || p.y >= old.height() as f64
                            {
                                old.fill()
                            } else {
                                old.get(p.x.floor() as u32, p.y.floor() as u32)
                            }
                        })));
                    }
                    mask(i, mw, mh)
                })
                .transpose()?;
            doc.nodes.push(Node {
                id: n.id,
                name: n.name,
                parent: n.parent,
                visible: n.visible,
                locked: n.locked,
                locks: n.locks,
                color_label: n.color_label,
                link_group: n.link_group,
                opacity: n.opacity,
                blend: n.blend,
                blending: n.blending,
                clip_to: n.clip_to,
                mask: node_mask,
                mask_enabled: n.mask_enabled,
                mask_linked: n.mask_linked,
                mask_transform: n.mask_transform,
                styles: n.styles,
                style_options: n.style_options,
                effects_enabled: n.effects_enabled,
                origin: n.origin,
                kind,
            });
        }
        let max_id = doc.nodes.iter().map(|n| n.id).max().unwrap_or(0);
        doc.next_id = h.next_id.max(max_id + 1);
        doc.validate()?;
        Ok(doc)
    };
    let mut commits = Vec::with_capacity(f.commits.len());
    for c in f.commits {
        let doc = decode_doc(c.doc)?;
        commits.push(Commit {
            id: c.id,
            parents: c.parents,
            name: c.name,
            time: c.time,
            auto: c.auto,
            branch: c.branch,
            doc,
        });
    }
    let working = f
        .working
        .map(|w| Ok::<_, IoError>((w.live, decode_doc(w.doc)?)))
        .transpose()?;
    let branches = f
        .branches
        .into_iter()
        .map(|(k, b)| {
            (
                k,
                Branch {
                    tip: b.tip,
                    base: b.base,
                },
            )
        })
        .collect();
    let graph = Graph::from_parts(commits, branches, f.head)
        .map_err(|e| IoError::Manifest(e.to_string()))?;
    Ok(Some(ReadGraph {
        graph,
        live: f.live,
        working,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    fn archive(
        entries: impl IntoIterator<Item = (String, Vec<u8>)>,
    ) -> ZipArchive<Cursor<Vec<u8>>> {
        let mut output = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            output
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            output.write_all(&bytes).unwrap();
        }
        ZipArchive::new(output.finish().unwrap()).unwrap()
    }

    fn path_graph() -> (Graph, emulsion_raster::vector::Path) {
        use emulsion_core::{Command, command::Slot};
        use emulsion_raster::vector::{Path, PathStyle};
        let path = Path::from_svg("M 2.123456789 3 C 4.25 1.125 27.5 7.75 29 20 Z M 31 22 L 37 28")
            .unwrap();
        let mut doc = Document::new(40, 32);
        Command::AddNode {
            node: Box::new(Node::path(
                0,
                "Outline",
                Arc::new(path.clone()),
                PathStyle::default(),
                doc.width,
                doc.height,
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
        let mut graph = Graph::new(doc.clone(), "Draw outline");
        doc.nodes[0].opacity = 0.5;
        assert!(graph.record(&doc, "Fade outline", false).is_some());
        (graph, path)
    }

    #[test]
    fn compact_paths_share_live_pool_and_restore_shared_geometry_across_commits() {
        let (original, geometry) = path_graph();
        let mut paths = PathPool::default();
        // The caller first collects the live document into this same pool.
        let live_path = serde_json::to_value(paths.add(&geometry).unwrap()).unwrap();
        let mut entries = encode(&original, None, None, &mut paths).unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(&entries[0].1).unwrap();
        assert_eq!(manifest["version"], HISTORY_VERSION);
        assert!(
            live_path.is_string(),
            "new history uses a compact blob reference"
        );
        for commit in manifest["commits"].as_array().unwrap() {
            assert_eq!(commit["doc"]["nodes"][0]["kind"]["path"], live_path);
        }
        let blobs: Vec<_> = paths.entries().collect();
        assert_eq!(
            blobs.len(),
            1,
            "live and all commits share one geometry blob"
        );
        entries.extend(blobs);
        let restored = read(&mut archive(entries)).unwrap().unwrap();
        let paths: Vec<_> = restored
            .graph
            .commits()
            .map(|commit| {
                let NodeKind::Path { path, .. } = &commit.doc.nodes[0].kind else {
                    panic!("editable path preserved");
                };
                assert_eq!(
                    path.as_ref(),
                    &geometry,
                    "anchor/handle coordinates survive exactly"
                );
                path.clone()
            })
            .collect();
        assert_eq!(paths.len(), original.len());
        assert!(
            Arc::ptr_eq(&paths[0], &paths[1]),
            "commits reuse the decoded path buffer"
        );
    }

    #[test]
    fn legacy_history_inline_paths_remain_readable_without_blobs() {
        let (original, geometry) = path_graph();
        let mut paths = PathPool::default();
        let mut entries = encode(&original, None, None, &mut paths).unwrap();
        let mut manifest: serde_json::Value = serde_json::from_slice(&entries[0].1).unwrap();
        manifest["version"] = serde_json::json!(1);
        for commit in manifest["commits"].as_array_mut().unwrap() {
            commit["doc"]["nodes"][0]["kind"]["path"] = serde_json::to_value(&geometry).unwrap();
        }
        entries[0].1 = serde_json::to_vec(&manifest).unwrap();
        // Deliberately omit the new pool's blobs: v1 geometry is self-contained.
        let restored = read(&mut archive(entries)).unwrap().unwrap();
        assert_eq!(restored.graph.len(), original.len());
        for (expected, actual) in original.commits().zip(restored.graph.commits()) {
            assert_eq!(actual.doc, expected.doc);
        }
    }

    #[test]
    fn history_larger_than_old_limit_preserves_graph() {
        let original = Graph::new(Document::new(8, 8), "Initial");
        let entries = encode(
            &original,
            Some("live-fingerprint".into()),
            None,
            &mut PathPool::default(),
        )
        .unwrap();
        let mut output = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .compression_level(Some(1));
        for (name, bytes) in entries {
            output.start_file(&name, options).unwrap();
            output.write_all(&bytes).unwrap();
            if name == GRAPH {
                // Exercise the old 64 MiB boundary without millions of nodes,
                // large image buffers, or a large compressed test fixture.
                let padding = [b' '; 64 * 1024];
                for _ in 0..1024 {
                    output.write_all(&padding).unwrap();
                }
            }
        }
        let mut archive = ZipArchive::new(output.finish().unwrap()).unwrap();
        assert!(archive.by_name(GRAPH).unwrap().size() > 64 << 20);
        let restored = read(&mut archive).unwrap().unwrap();
        assert_eq!(restored.live.as_deref(), Some("live-fingerprint"));
        assert_eq!(restored.graph.len(), original.len());
        assert_eq!(restored.graph.head(), original.head());
        assert_eq!(
            restored
                .graph
                .commit(restored.graph.head_branch().tip)
                .unwrap()
                .doc,
            original.commit(original.head_branch().tip).unwrap().doc
        );
    }

    #[test]
    fn future_history_version_rejected_before_schema() {
        let mut output = ZipWriter::new(Cursor::new(Vec::new()));
        output
            .start_file(GRAPH, SimpleFileOptions::default())
            .unwrap();
        // A future format need not have the fields expected by HFile.
        write!(output, "{{\"version\":{}}}", HISTORY_VERSION + 1).unwrap();
        let mut archive = ZipArchive::new(output.finish().unwrap()).unwrap();
        assert!(matches!(read(&mut archive), Err(IoError::TooNew(_))));
    }
}
