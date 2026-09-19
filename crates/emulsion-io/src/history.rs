//! The history graph inside a native file.
//!
//! ```text
//! history/graph.json      commits, branches, and which planes each uses
//! history/tiles/r<n>      one 256×256 RGBA16 tile, little-endian, deflated
//! history/tiles/m<n>      one 256×256 8-bit mask tile, deflated
//! ```
//!
//! Commits share pixels the way they do in memory: each distinct tile is
//! stored once however many commits use it, and each distinct plane
//! (raster, mask, selection) is listed once and referenced by index. On
//! reading, shared entries become shared buffers again, so unchanged nodes
//! still compare equal across commits and merges stay precise.
//!
//! The graph is optional. Files without it open with a fresh history, and
//! a damaged graph never stops the document itself from opening.

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

pub const HISTORY_VERSION: u32 = 1;
pub(crate) const GRAPH: &str = "history/graph.json";
const MAX_GRAPH_BYTES: u64 = 64 << 20;

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
    rasters: Vec<HPlane<[u16; 4]>>,
    masks: Vec<HPlane<u8>>,
    commits: Vec<HCommit>,
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
    source_depth: u8,
    blend_space: BlendSpace,
    next_id: NodeId,
    selection: Option<u32>,
    nodes: Vec<HNode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    guides: Vec<emulsion_core::document::Guide>,
}

#[derive(Serialize, Deserialize)]
struct HNode {
    id: NodeId,
    name: String,
    parent: Option<NodeId>,
    visible: bool,
    locked: bool,
    opacity: f32,
    blend: BlendMode,
    clip_to: Option<NodeId>,
    mask: Option<u32>,
    mask_enabled: bool,
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
        path: emulsion_raster::vector::Path,
        style: emulsion_raster::vector::PathStyle,
    },
    Smart {
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
/// document equals the head branch's tip.
pub(crate) fn encode(graph: &Graph, live: Option<String>) -> Result<Vec<(String, Vec<u8>)>> {
    let mut rasters = Pool::<[u16; 4]>::new();
    let mut masks = Pool::<u8>::new();
    let commits = graph
        .commits()
        .map(|c| {
            let d = &c.doc;
            let nodes = d
                .nodes
                .iter()
                .map(|n| HNode {
                    id: n.id,
                    name: n.name.clone(),
                    parent: n.parent,
                    visible: n.visible,
                    locked: n.locked,
                    opacity: n.opacity,
                    blend: n.blend,
                    clip_to: n.clip_to,
                    mask: n.mask.as_ref().map(|m| masks.add(m)),
                    mask_enabled: n.mask_enabled,
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
                            source,
                            filters,
                            placement,
                            cache,
                            offset,
                        } => HKind::Smart {
                            source: rasters.add(source),
                            cache: rasters.add(cache),
                            offset: *offset,
                            filters: filters.clone(),
                            placement: *placement,
                        },
                        NodeKind::Path { path, style, .. } => HKind::Path {
                            path: (**path).clone(),
                            style: *style,
                        },
                    },
                })
                .collect();
            HCommit {
                id: c.id,
                parents: c.parents.clone(),
                name: c.name.clone(),
                time: c.time,
                auto: c.auto,
                branch: c.branch.clone(),
                doc: HDoc {
                    width: d.width,
                    height: d.height,
                    resolution: d.resolution,
                    source_depth: d.source_depth,
                    blend_space: d.blend_space,
                    next_id: d.next_id,
                    selection: d.selection.as_ref().map(|s| masks.add(s)),
                    nodes,
                    guides: d.guides.clone(),
                },
            }
        })
        .collect();
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
        rasters: rasters.list,
        masks: masks.list,
        commits,
    };
    let json = serde_json::to_vec(&file).map_err(|e| IoError::Manifest(e.to_string()))?;
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
}

/// Read the graph if the file has one. Every reference is checked.
pub(crate) fn read<R: Read + Seek>(zip: &mut ZipArchive<R>) -> Result<Option<ReadGraph>> {
    if zip.by_name(GRAPH).is_err() {
        return Ok(None);
    }
    let bytes = crate::ora::read_entry(zip, GRAPH, MAX_GRAPH_BYTES)?;
    let probe: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| IoError::Manifest(e.to_string()))?;
    let version = probe.get("version").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    if version > HISTORY_VERSION {
        return Err(IoError::TooNew(version));
    }
    let f: HFile = serde_json::from_value(probe).map_err(|e| IoError::Manifest(e.to_string()))?;
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

    let mut commits = Vec::with_capacity(f.commits.len());
    for c in f.commits {
        let h = c.doc;
        crate::import::check_size(h.width, h.height)?;
        if h.nodes.len() > emulsion_core::document::MAX_NODES {
            return Err(IoError::Manifest("too many nodes".into()));
        }
        let mut doc = Document::new(h.width, h.height);
        doc.resolution = h.resolution;
        doc.source_depth = if h.source_depth == 16 { 16 } else { 8 };
        doc.blend_space = h.blend_space;
        doc.guides = h.guides;
        doc.selection = h
            .selection
            .map(|i| mask(i, h.width, h.height))
            .transpose()?;
        for n in h.nodes {
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
                    source,
                    cache,
                    offset,
                    filters,
                    placement,
                } => NodeKind::Smart {
                    source: raster(source)?,
                    cache: raster(cache)?,
                    offset,
                    filters,
                    placement,
                },
                HKind::Path { path, style } => {
                    if path.anchor_count() > emulsion_raster::vector::MAX_ANCHORS {
                        return Err(IoError::Manifest("a path has too many anchors".into()));
                    }
                    let style = style.sanitized();
                    let cache = Arc::new(path.rasterize(&style, h.width, h.height));
                    NodeKind::Path {
                        path: Arc::new(path),
                        style,
                        cache,
                    }
                }
            };
            let (mw, mh) = match &kind {
                NodeKind::Raster { raster, .. } => (raster.width(), raster.height()),
                _ => (h.width, h.height),
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
                mask: n.mask.map(|i| mask(i, mw, mh)).transpose()?,
                mask_enabled: n.mask_enabled,
                kind,
            });
        }
        let max_id = doc.nodes.iter().map(|n| n.id).max().unwrap_or(0);
        doc.next_id = h.next_id.max(max_id + 1);
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
    }))
}
