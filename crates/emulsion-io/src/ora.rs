//! The native format: OpenRaster with an Emulsion manifest.
//!
//! Layout inside the zip:
//!
//! ```text
//! mimetype                  "image/openraster", first entry, stored
//! stack.xml                 standard ORA stack (raster layers and groups)
//! data/node-<id>.png        layer pixels as other ORA readers should see them
//! emulsion/src/node-<id>.png  source pixels of transformed layers
//! emulsion/mask-<id>.png    8-bit layer masks
//! emulsion/filter-mask-<id>.png  independent 8-bit Smart Filter masks
//! emulsion.json             the full node stack (adjustments, placements, …)
//! emulsion/paths/*.bin      exact editable geometry, shared with history
//! original-images/*.png     validated exact PNG sources, shared with history
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
use crate::mapping_data::{ComponentOwner, MappingData, PlacementData};
use crate::{IoError, Result, write_atomic};
use emulsion_core::graph::Graph;
use emulsion_core::mapping::{Mapping2, SmartPlacement};
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

// Native shape paints and stroke geometry must not be silently ignored by older readers.
// Version 6 preserves rich text runs, paragraph frames, warp and path text.
// Older builds must reject these files instead of silently flattening those attributes.
// Version 7 retains diagram graphs, conditional rules, design constraints and motion.
// Version 8 externalizes portable font and local-media resources by content digest.
// Version 9 retains nested Smart source archives as content-addressed resources.
// Version 10 retains persistent mask properties and independent raw mask extents.
// Version 11 retains independent editable native vector masks.
// Version 12 retains dedicated Smart Filter masks, including dormant descriptors.
// Version 13 preserves original Smart PNG resources bound to exact native source pixels.
// Version 14 retains Photoshop sRGB v1, explicit Background identity, and Invert Smart Filters.
// Version 15 preserves independent Smart Filter stack and item enabled state.
// Version 16 retains Smart projective placements and component mappings.
pub const FORMAT_VERSION: u32 = 16;
const MANIFEST: &str = "emulsion.json";
// Editable geometry can be large, especially in legacy pretty-printed files.
// Keep the much smaller generic ORA XML limit separate.
pub(crate) const MAX_NATIVE_MANIFEST_BYTES: u64 = 512 << 20;
const MAX_STACK_BYTES: u64 = 4 << 20;
const MAX_ENTRY_BYTES: u64 = 1 << 30;

#[derive(Serialize, Deserialize)]
struct Manifest {
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    media_resources: std::collections::BTreeMap<emulsion_core::NodeId, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    fonts: Vec<String>,
    #[serde(
        default,
        skip_serializing_if = "emulsion_core::design_metadata::Design::is_default"
    )]
    design: emulsion_core::design_metadata::Design,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    diagram: Option<Arc<emulsion_core::diagram::Diagram>>,
    format: String,
    version: u32,
    width: u32,
    height: u32,
    resolution: f32,
    #[serde(default)]
    global_light: emulsion_core::style_options::GlobalLight,
    source_depth: u8,
    blend_space: BlendSpace,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    psd_background: Option<NodeId>,
    /// Bottom to top.
    nodes: Vec<MNode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    patterns: Vec<MPattern>,
    /// Ruler guides. Absent in files from before guides existed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    guides: Vec<emulsion_core::document::Guide>,
    /// Camera metadata from the source photograph.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    info: Option<emulsion_core::document::ImageInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    raw: Option<emulsion_core::raw::RawDocument>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    raw_originals: Vec<std::path::PathBuf>,
    /// Colours painted with, most recent first. Absent in older files.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    colors: Vec<[u8; 3]>,
    /// Drawing Assist guides, ruler and guide sets. Absent in older files.
    #[serde(default, skip_serializing_if = "is_default")]
    drawing_guides: emulsion_core::drawing_guides::DrawingGuides,
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

#[derive(Serialize, Deserialize)]
struct MPattern {
    src: String,
    width: u32,
    height: u32,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    link_group: Option<NodeId>,
    opacity: f32,
    blend: BlendMode,
    #[serde(default)]
    blending: emulsion_raster::composite::BlendingOptions,
    clip_to: Option<NodeId>,
    mask: Option<String>,
    #[serde(default = "default_mask_fill")]
    mask_fill: u8,
    mask_enabled: bool,
    #[serde(default = "emulsion_core::node::default_mask_linked")]
    mask_linked: bool,
    #[serde(default)]
    mask_transform: MappingData,
    #[serde(default, skip_serializing_if = "is_default")]
    mask_properties: emulsion_core::MaskProperties,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    vector_mask: Option<crate::path_data::VectorMaskData>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    styles: Vec<emulsion_core::styles::LayerStyle>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    style_options: Vec<emulsion_core::style_options::StyleOptions>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pattern_refs: Vec<Option<usize>>,
    #[serde(default = "emulsion_core::node::default_effects_enabled")]
    effects_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    origin: Option<String>,
    /// A non-printing storyboard review layer.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    review: bool,
    kind: MKind,
}

fn default_mask_fill() -> u8 {
    255
}

#[expect(
    clippy::large_enum_variant,
    reason = "Intentional inline native wire metadata; raster and mask pixels stay in separate resources."
)]
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum MKind {
    Raster {
        src: String,
        width: u32,
        height: u32,
        placement: PlacementData,
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
    Strokes {
        strokes: emulsion_raster::strokes::StrokeSet,
        /// The rasterized strokes, for readers that only know layers.
        src: String,
    },
    Smart {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        editable: Option<emulsion_core::node::SmartEditable>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_document: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        original_image: Option<crate::original_image_data::OriginalImageRef>,
        /// Source pixels.
        src: String,
        width: u32,
        height: u32,
        filters: Vec<emulsion_filters::Filter>,
        #[serde(default)]
        filter_styles: Vec<emulsion_filters::FilterStyle>,
        #[serde(
            default = "crate::native_features::enabled_by_default",
            skip_serializing_if = "crate::native_features::is_enabled"
        )]
        filters_enabled: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filter_mask: Option<Box<crate::filter_mask_data::FilterMaskData<String>>>,
        placement: PlacementData,
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
    patterns: Vec<Arc<emulsion_core::style_options::PatternImage>>,
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
        space: doc.blend_space,
        knockout_background: None,
        nodes: vec![CompositeNode {
            id: 0,
            visible: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
            blending: Default::default(),
            mask: None,
            clip_to: None,
            clip_rect: None,
            content: NodeContent::Pixels {
                raster: raster.clone().into(),
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

/// Projected appearance stays document-bounded; the retained source and encoded
/// originals are separate jobs and never rewritten by this preview.
fn bake_smart(
    doc: &Document,
    raster: &Arc<Raster>,
    placement: SmartPlacement,
) -> Result<(Raster, i64, i64)> {
    let SmartPlacement::Projective(forward) = placement else {
        return Ok(bake(
            doc,
            raster,
            &placement.require_legacy("ORA Smart preview")?,
        ));
    };
    let content = NodeContent::projective_pixels(raster.clone().into(), forward)
        .map_err(|e| IoError::Manifest(format!("invalid ORA Smart preview: {e}")))?;
    let NodeContent::ProjectivePixels(pixels) = &content else {
        unreachable!("checked projective content");
    };
    let bounds = pixels
        .mapping()
        .bounds()
        .intersect(&emulsion_raster::IRect::new(
            0,
            0,
            doc.width as i32,
            doc.height as i32,
        ));
    if bounds.is_empty() {
        return Ok((Raster::transparent(1, 1), 0, 0));
    }
    let tree = CompositeTree {
        width: doc.width,
        height: doc.height,
        space: doc.blend_space,
        knockout_background: None,
        nodes: vec![CompositeNode {
            id: 0,
            visible: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
            blending: Default::default(),
            mask: None,
            clip_to: None,
            clip_rect: None,
            content,
        }],
    };
    let full = flatten(&tree, 0);
    let crop = Raster::from_fn(bounds.w as u32, bounds.h as u32, [0; 4], |x, y| {
        full.get(x + bounds.x as u32, y + bounds.y as u32)
    });
    Ok((crop, i64::from(bounds.x), i64::from(bounds.y)))
}

fn encode(
    doc: &Document,
    paths: &mut crate::path_data::PathPool,
    originals: &mut crate::original_image_data::OriginalImagePool,
) -> Result<Encoded> {
    let mut sources = crate::smart_source_data::SourcePool::default();
    enum Job<'a> {
        SmartPreview {
            path: String,
            id: NodeId,
            raster: Arc<Raster>,
            placement: SmartPlacement,
        },
        VectorPreview {
            path: String,
            id: NodeId,
            raster: &'a Raster,
        },
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
    let mut patterns: Vec<Arc<emulsion_core::style_options::PatternImage>> = Vec::new();
    let mut pattern_pointers: HashMap<usize, usize> = HashMap::new();
    let mut pattern_hashes: HashMap<(u32, u32, String), Vec<usize>> = HashMap::new();
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
                    placement: PlacementData::from_raster(*placement)?,
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
                original_image,
                filters,
                filter_styles,
                filters_enabled,
                filter_mask,
                placement,
                ..
            } => {
                // Other readers get the filtered result, placed where it lands.
                let data = format!("data/node-{}.png", n.id);
                let original_image =
                    originals.reference(original_image, source, editable.is_some())?;
                let src = if let Some(original) = &original_image {
                    original.path()
                } else {
                    let src = format!("emulsion/src/node-{}.png", n.id);
                    jobs.push(Job::Png {
                        path: src.clone(),
                        raster: source,
                    });
                    src
                };
                let grid = emulsion_core::smart_support::output_grid(n)?;
                let cp = match placement {
                    SmartPlacement::Legacy(legacy) => {
                        SmartPlacement::Legacy(emulsion_core::smart::cache_placement(
                            legacy,
                            (source.width(), source.height()),
                            grid.size,
                            grid.offset,
                        ))
                    }
                    SmartPlacement::Projective(projective) => SmartPlacement::Projective(
                        Mapping2::Projective(*projective)
                            .with_source_offset(grid.offset)
                            .and_then(Mapping2::to_projective)
                            .map_err(|e| IoError::Manifest(e.to_string()))?,
                    ),
                };
                jobs.push(Job::SmartPreview {
                    path: data,
                    id: n.id,
                    raster: emulsion_core::smart_filter_mask::effective_pixels_with_space(
                        n,
                        doc.blend_space,
                    )?
                    .expect("Smart node has effective pixels"),
                    placement: cp,
                });
                let filter_mask = filter_mask
                    .as_ref()
                    .map(|mask| {
                        let path = format!("emulsion/filter-mask-{}.png", n.id);
                        jobs.push(Job::Mask {
                            path: path.clone(),
                            mask: &mask.pixels,
                        });
                        crate::filter_mask_data::FilterMaskData::encode(mask, path).map(Box::new)
                    })
                    .transpose()?;
                MKind::Smart {
                    original_image,
                    source_document: sources.reference(editable),
                    editable: crate::smart_source_data::SourcePool::stripped(editable),
                    src,
                    width: source.width(),
                    height: source.height(),
                    filters: filters.clone(),
                    filter_styles: filter_styles.clone(),
                    filters_enabled: *filters_enabled,
                    filter_mask,
                    placement: PlacementData::from_smart(*placement)?,
                }
            }
            NodeKind::Path { path, style, cache } => {
                let data = format!("data/node-{}.png", n.id);
                jobs.push(Job::VectorPreview {
                    path: data.clone(),
                    id: n.id,
                    raster: cache.pixels(),
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
                jobs.push(Job::VectorPreview {
                    path: data.clone(),
                    id: n.id,
                    raster: cache.pixels(),
                });
                ora_layers.insert(n.id, (data.clone(), 0, 0));
                MKind::Text {
                    spec: (**spec).clone(),
                    src: data,
                }
            }
            NodeKind::Strokes { strokes, cache } => {
                let data = format!("data/node-{}.png", n.id);
                jobs.push(Job::VectorPreview {
                    path: data.clone(),
                    id: n.id,
                    raster: cache.pixels(),
                });
                ora_layers.insert(n.id, (data.clone(), 0, 0));
                MKind::Strokes {
                    strokes: (**strokes).clone(),
                    src: data,
                }
            }
        };
        let mut style_options = n.style_options.clone();
        let pattern_refs = style_options
            .iter_mut()
            .map(|option| {
                option.pattern.image.take().map(|image| {
                    let pointer = Arc::as_ptr(&image) as usize;
                    if let Some(index) = pattern_pointers.get(&pointer) {
                        return *index;
                    }
                    let hash = (
                        image.width,
                        image.height,
                        crate::history::fingerprint(&image.pixels),
                    );
                    let candidates = pattern_hashes.entry(hash).or_default();
                    let index = candidates
                        .iter()
                        .copied()
                        .find(|i| patterns[*i].pixels == image.pixels)
                        .unwrap_or_else(|| {
                            let index = patterns.len();
                            patterns.push(image);
                            candidates.push(index);
                            index
                        });
                    pattern_pointers.insert(pointer, index);
                    index
                })
            })
            .collect();
        nodes.push(MNode {
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
            mask,
            mask_fill: n.mask.as_ref().map_or(255, |m| m.fill()),
            mask_enabled: n.mask_enabled,
            mask_linked: n.mask_linked,
            mask_transform: MappingData::from_raster_mask(
                n.mask_transform,
                if matches!(&n.kind, NodeKind::Smart { .. }) {
                    ComponentOwner::Smart
                } else {
                    ComponentOwner::Other
                },
            )?,
            mask_properties: n.mask_properties,
            vector_mask: n
                .vector_mask
                .as_ref()
                .map(|mask| crate::path_data::VectorMaskData::encode(mask, paths))
                .transpose()?,
            styles: n.styles.clone(),
            style_options,
            pattern_refs,
            effects_enabled: n.effects_enabled,
            origin: n.origin.clone(),
            review: n.review,
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
                    Job::SmartPreview {
                        path,
                        id,
                        raster,
                        placement,
                    } => {
                        if let SmartPlacement::Legacy(legacy) = placement
                            && is_integer_translation(&legacy)
                        {
                            Ok((
                                path,
                                raster_png(&raster, depth)?,
                                Some((id, legacy.x as i64, legacy.y as i64)),
                            ))
                        } else {
                            let (r, x, y) = bake_smart(doc, &raster, placement)?;
                            Ok((path, raster_png(&r, depth)?, Some((id, x, y))))
                        }
                    }
                    Job::VectorPreview { path, id, raster } => {
                        // Native readers rebuild vectors from their editable
                        // geometry. ORA readers need only the occupied pixels
                        // and their stack.xml offset, not a canvas-sized PNG.
                        let bounds = raster.coverage_bounds();
                        let (bytes, x, y) = if bounds.is_empty() {
                            (raster_png(&Raster::transparent(1, 1), depth)?, 0, 0)
                        } else if bounds == raster.bounds() {
                            (raster_png(raster, depth)?, 0, 0)
                        } else {
                            let crop = Raster::from_fn(
                                bounds.w as u32,
                                bounds.h as u32,
                                [0; 4],
                                |x, y| raster.get(x + bounds.x as u32, y + bounds.y as u32),
                            );
                            (
                                raster_png(&crop, depth)?,
                                i64::from(bounds.x),
                                i64::from(bounds.y),
                            )
                        };
                        Ok((path, bytes, Some((id, x, y))))
                    }
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
            let tree = doc.try_composite_tree()?;
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
    entries.extend(sources.entries("sources")?);

    let mut fonts = crate::font_data::FontPool::default();
    let (design, font_refs) = fonts.detach(&doc.design);
    entries.extend(fonts.entries("fonts")?);
    let mut media = crate::media_data::MediaPool::default();
    let (design, media_resources) = media.detach(&design);
    entries.extend(media.entries("media")?);
    let manifest = Manifest {
        media_resources,
        fonts: font_refs,
        diagram: doc.diagram.clone(),
        design,
        format: "emulsion".into(),
        version: required_version(doc),
        width: doc.width,
        height: doc.height,
        resolution: doc.resolution,
        global_light: doc.global_light,
        source_depth: doc.source_depth,
        blend_space: doc.blend_space,
        psd_background: doc.psd_background,
        nodes,
        patterns: patterns
            .iter()
            .enumerate()
            .map(|(i, image)| MPattern {
                src: format!("emulsion/patterns/{i}.rgba"),
                width: image.width,
                height: image.height,
            })
            .collect(),
        guides: doc.guides.clone(),
        info: doc.info.clone(),
        raw: doc.raw.clone(),
        raw_originals: doc.raw_originals.clone(),
        colors: doc.colors.clone(),
        drawing_guides: doc.drawing_guides.clone(),
    };
    Ok(Encoded {
        patterns,
        entries,
        ora_layers,
        manifest,
    })
}

/// Minimum native/history reader version needed for every editable feature,
/// including disabled and empty mask components. Embedded Smart source archives
/// are opaque resources with independent native versions: preserve their bytes
/// and saved appearance here, and validate their version when opened for editing.
pub(crate) fn required_version(doc: &Document) -> u32 {
    if doc.nodes.iter().any(Node::has_projective_metadata) {
        16
    } else if doc
        .nodes
        .iter()
        .any(crate::native_features::has_disabled_filters)
    {
        15
    } else if crate::native_features::requires_v14(doc.blend_space, doc.psd_background)
        || doc.nodes.iter().any(crate::native_features::has_invert)
    {
        14
    } else if doc.nodes.iter().any(|node| {
        matches!(
            &node.kind,
            NodeKind::Smart {
                original_image: Some(_),
                ..
            }
        )
    }) {
        13
    } else if doc.nodes.iter().any(has_filter_mask) {
        12
    } else if doc.nodes.iter().any(|n| n.vector_mask.is_some()) {
        11
    } else if requires_mask_v10(doc) {
        10
    } else {
        9
    }
}

/// Whether losing this authored document would violate the protected native
/// retention contract. Embedded documents already existed in version 9, but
/// their opaque bytes may contain features newer than the enclosing document.
/// Do not inspect, decode or downgrade those bytes to select the outer gate.
pub(crate) fn requires_preservation(doc: &Document) -> bool {
    required_version(doc) >= 13
        || doc.nodes.iter().any(|node| {
            matches!(
                &node.kind,
                NodeKind::Smart {
                    editable: Some(emulsion_core::node::SmartEditable::Document { .. }),
                    ..
                }
            )
        })
}

/// Descriptor presence is semantic, even while disabled or the stack is empty.
pub(crate) fn has_filter_mask(node: &Node) -> bool {
    matches!(
        &node.kind,
        NodeKind::Smart {
            filter_mask: Some(_),
            ..
        }
    )
}

pub(crate) fn requires_mask_v10(doc: &Document) -> bool {
    doc.nodes.iter().any(|n| {
        let legacy_extent = match &n.kind {
            NodeKind::Raster { raster, .. } => (raster.width(), raster.height()),
            NodeKind::Smart { source, .. } => (source.width(), source.height()),
            _ => (doc.width, doc.height),
        };
        !is_default(&n.mask_properties)
            || n.mask
                .as_ref()
                .is_some_and(|mask| (mask.width(), mask.height()) != legacy_extent)
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
        && !n.has_mask()
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
    if doc
        .nodes
        .iter()
        .any(crate::native_features::has_disabled_filters)
        || crate::native_features::requires_v14(doc.blend_space, doc.psd_background)
        || doc.nodes.iter().any(|n| {
            matches!(n.kind, NodeKind::Adjust(_) | NodeKind::Fill { .. })
                || n.clip_to.is_some()
                || n.has_mask()
                || has_filter_mask(n)
                || !n.styles.is_empty()
                || n.blending != Default::default()
        })
    {
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
                | NodeKind::Strokes { .. }
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
    ensure_not_raw_original(doc, path)?;
    if let Some(graph) = graph {
        for commit in graph.commits() {
            ensure_not_raw_original(&commit.doc, path)?;
        }
    }
    write_atomic(path, |file| write_to(doc, graph, file))
}

/// Write a page archive into a project container without temporary files.
pub(crate) fn write_to<W: Write + Seek>(
    doc: &Document,
    graph: Option<&Graph>,
    writer: W,
) -> Result<()> {
    doc.validate()?;
    let mut paths = crate::path_data::PathPool::default();
    let mut originals = crate::original_image_data::OriginalImagePool::default();
    let mut enc = encode(doc, &mut paths, &mut originals)?;
    // The archive version covers all saved snapshots, even after a feature
    // was reset or removed in the live document. Older readers must reject it
    // rather than silently discard editable history.
    if let Some(graph) = graph {
        enc.manifest.version = graph
            .commits()
            .map(|commit| required_version(&commit.doc))
            .fold(enc.manifest.version, u32::max);
    }
    referenced_patterns(&enc.manifest)?;
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
            let live = tip
                .is_some_and(|tip| {
                    matches!(
                        crate::native_relation::history_persistence_matches(doc, tip),
                        crate::native_relation::LiveRelation::Consistent
                    )
                })
                .then(|| crate::history::fingerprint(&manifest));
            let working = live
                .is_none()
                .then(|| (doc, crate::history::fingerprint(&manifest)));
            crate::history::encode(g, live, working, &mut paths, &mut originals)?
        }
        None => Vec::new(),
    };
    enc.entries.extend(originals.entries());
    {
        let mut z = ZipWriter::new(std::io::BufWriter::new(writer));
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
        // Keep the existing lossless compression levels, but compress independent
        // entries on workers before assembling them in deterministic order.
        let path_entries: Vec<_> = paths.entries().collect();
        let fast = deflated.compression_level(Some(1));
        let mut entries: Vec<(&str, &[u8], SimpleFileOptions)> = enc
            .entries
            .iter()
            .map(|(name, bytes)| (name.as_str(), bytes.as_slice(), deflated))
            .collect();
        entries.extend(enc.patterns.iter().zip(&enc.manifest.patterns).map(
            |(pattern, metadata)| (metadata.src.as_str(), pattern.pixels.as_slice(), deflated),
        ));
        entries.extend(
            path_entries
                .iter()
                .map(|(name, bytes)| (name.as_str(), bytes.as_slice(), deflated)),
        );
        entries.extend(history.iter().map(|(name, bytes)| {
            (
                name.as_str(),
                bytes.as_slice(),
                if name == crate::history::GRAPH {
                    deflated
                } else {
                    fast
                },
            )
        }));
        write_entries(&mut z, &entries)?;
        z.finish()?.flush()?;
        Ok(())
    }
}

/// Bound the extra compressed buffers while allowing layer images and history
/// tiles to use multiple cores. A single oversized entry streams directly.
fn write_entries<W: Write + Seek>(
    writer: &mut ZipWriter<W>,
    entries: &[(&str, &[u8], SimpleFileOptions)],
) -> Result<()> {
    const BATCH_BYTES: usize = 64 << 20;
    let mut start = 0;
    while start < entries.len() {
        let mut end = start + 1;
        let mut bytes = entries[start].1.len();
        while end < entries.len()
            && end - start < 64
            && entries[end].1.len() <= BATCH_BYTES.saturating_sub(bytes)
        {
            bytes += entries[end].1.len();
            end += 1;
        }
        let batch = &entries[start..end];
        if batch.len() == 1 {
            let (name, bytes, options) = batch[0];
            writer.start_file(
                name,
                options.large_file(bytes.len() as u64 >= u32::MAX as u64),
            )?;
            writer.write_all(bytes)?;
        } else {
            let compressed: Vec<Result<_>> = batch
                .par_iter()
                .map(|(name, bytes, options)| {
                    let mut part = ZipWriter::new(std::io::Cursor::new(Vec::new()));
                    part.start_file(
                        *name,
                        options.large_file(bytes.len() as u64 >= u32::MAX as u64),
                    )?;
                    part.write_all(bytes)?;
                    Ok(ZipArchive::new(part.finish()?)?)
                })
                .collect();
            for part in compressed {
                writer.merge_archive(part?)?;
            }
        }
        start = end;
    }
    Ok(())
}

/// Enforce original preservation at the write boundary, including baked RAWs.
pub(crate) fn ensure_not_raw_original(doc: &Document, path: &Path) -> Result<()> {
    let destination = std::fs::canonicalize(path).ok();
    for source in doc
        .raw_originals
        .iter()
        .chain(doc.raw.iter().map(|raw| &raw.source))
    {
        if source == path
            || destination.as_ref().is_some_and(|destination| {
                std::fs::canonicalize(source).ok().as_ref() == Some(destination)
            })
        {
            return Err(IoError::Unsupported("Saving or exporting cannot overwrite a linked original photograph. Choose a different output file.".into()));
        }
    }
    Ok(())
}

pub(crate) fn read_entry<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    name: &str,
    max: u64,
) -> Result<Vec<u8>> {
    let mut f = zip.by_name(name)?;
    if f.size() > max {
        return Err(IoError::Manifest(format!("entry {name} is too large")));
    }
    let mut out = Vec::with_capacity(f.size() as usize);
    f.by_ref().take(max).read_to_end(&mut out)?;
    Ok(out)
}

fn png_dimensions(bytes: &[u8]) -> Result<(u32, u32)> {
    use image::ImageDecoder as _;
    let decoder = image::codecs::png::PngDecoder::new(std::io::Cursor::new(bytes))?;
    let size = decoder.dimensions();
    check_size(size.0, size.1)?;
    Ok(size)
}

fn decode_png(bytes: &[u8]) -> Result<(Raster, u8)> {
    png_dimensions(bytes)?;
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)?;
    let d = from_dynamic(img)?;
    Ok((d.raster, d.depth))
}

fn decode_mask(bytes: &[u8], fill: u8) -> Result<Mask> {
    png_dimensions(bytes)?;
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)?.into_luma8();
    check_size(img.width(), img.height())?;
    Ok(Mask::from_pixels(
        img.width(),
        img.height(),
        fill,
        img.as_raw(),
    ))
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
    read_from(std::io::BufReader::new(std::fs::File::open(path)?))
}

/// Private evidence is kept until the caller chooses a representation. In
/// particular retired admission never observes an already-substituted document.
#[derive(Default)]
struct LiveEvidence {
    source_entries: Vec<(NodeId, String)>,
    auxiliary_error: Option<IoError>,
}

/// Retired storage keeps one exact live aid payload on its head tip. Validate
/// at admission/output boundaries, before compatibility readers can sanitize it.
pub(crate) fn validate_retired_live_aids(
    colors: &[[u8; 3]],
    drawing_guides: &emulsion_core::drawing_guides::DrawingGuides,
) -> Result<()> {
    if colors.len() > emulsion_core::document::MAX_PROJECT_COLORS {
        return Err(IoError::NativePreservation {
            code: crate::NativeFailureCode::RetiredLiveInvalidColors,
            location: "retired.live.colors".into(),
            detail: format!(
                "{} colors exceed the supported {} entries",
                colors.len(),
                emulsion_core::document::MAX_PROJECT_COLORS
            ),
        });
    }
    drawing_guides
        .validate()
        .map_err(|detail| IoError::NativePreservation {
            code: crate::NativeFailureCode::RetiredLiveInvalidDrawingGuides,
            location: "retired.live.drawing_guides".into(),
            detail,
        })
}

struct DecodedNative {
    live: Document,
    evidence: LiveEvidence,
    history: Option<crate::history::ReadGraph>,
    history_error: Option<IoError>,
    manifest: Option<String>,
    facts: crate::native_admission::ArchiveFacts,
}

pub(crate) struct NativeReadFailure {
    pub facts: crate::native_admission::ArchiveFacts,
    pub stage: &'static str,
    pub source: IoError,
}

fn decode_native<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
) -> std::result::Result<DecodedNative, NativeReadFailure> {
    let inspection = crate::native_admission::inspect_archive(zip);
    let facts = inspection.facts;
    let failure = |stage, source| NativeReadFailure {
        facts: facts.clone(),
        stage,
        source,
    };
    if let Some(error) = inspection.error {
        return Err(failure("metadata admission", error));
    }
    let strict_history = crate::native_features::preflight_archive(zip)
        .map_err(|e| failure("feature admission", e))?;
    preflight_archive_metadata(zip, strict_history || facts.standalone_strict)
        .map_err(|e| failure("geometry metadata admission", e))?;
    let mut originals = crate::original_image_data::preflight_archive(zip)
        .map_err(|e| failure("original resource admission", e))?;
    let mut paths = crate::path_data::PathReader::default();
    let mut evidence = LiveEvidence::default();
    let live = read_document(zip, &mut paths, &mut originals, &mut evidence)
        .map_err(|e| failure("live document", e))?;
    let manifest = if facts.native_version.is_some() {
        Some(crate::history::fingerprint(
            &read_entry(zip, MANIFEST, MAX_NATIVE_MANIFEST_BYTES)
                .map_err(|e| failure("live fingerprint", e))?,
        ))
    } else {
        None
    };
    let (history, history_error) = match crate::history::read(zip, &mut paths, &mut originals) {
        Ok(history) => (history, None),
        Err(error) if facts.standalone_strict || originals.requires_valid_history() => {
            return Err(failure("history", error));
        }
        Err(error) => (None, Some(error)),
    };
    Ok(DecodedNative {
        live,
        evidence,
        history,
        history_error,
        manifest,
        facts,
    })
}

fn relation_error(
    relation: crate::native_relation::LiveRelation,
    mismatch: crate::NativeFailureCode,
    unverified: crate::NativeFailureCode,
    location: &str,
) -> Option<IoError> {
    use crate::native_relation::LiveRelation;
    let (code, detail) = match relation {
        LiveRelation::Consistent => return None,
        LiveRelation::Mismatch(detail) => (mismatch, detail),
        LiveRelation::Unverified(detail) => (unverified, detail),
    };
    Some(IoError::NativePreservation {
        code,
        location: location.into(),
        detail,
    })
}

pub(crate) fn read_from<R: Read + Seek>(reader: R) -> Result<Opened> {
    read_selected(reader, false)
}

pub(crate) fn read_project_from<R: Read + Seek>(reader: R) -> Result<Opened> {
    read_selected(reader, true)
}

fn read_selected<R: Read + Seek>(reader: R, strict_history: bool) -> Result<Opened> {
    let mut zip = ZipArchive::new(reader)?;
    let decoded = decode_native(&mut zip).map_err(|e| e.source)?;
    let DecodedNative {
        mut live,
        evidence,
        history,
        mut history_error,
        manifest,
        facts,
    } = decoded;
    if strict_history && let Some(error) = history_error.take() {
        return Err(error);
    }
    let Some(history) = history else {
        return Ok(Opened {
            doc: live,
            graph: None,
            history_error: history_error.map(|e| e.to_string()),
        });
    };
    let tip = history
        .graph
        .commit(history.graph.head_branch().tip)
        .map(|c| &c.doc);
    let mut selected_working = false;
    let candidate = if let Some((fingerprint, working)) = &history.working
        && Some(fingerprint) == manifest.as_ref()
        && matches!(
            crate::native_relation::live_representation_matches(
                &live,
                &evidence.source_entries,
                working,
                &mut zip
            )?,
            crate::native_relation::LiveRelation::Consistent
        ) {
        selected_working = true;
        Some(working)
    } else if history.live.is_some() && history.live == manifest {
        if let Some(tip) = tip
            && matches!(
                crate::native_relation::live_representation_matches(
                    &live,
                    &evidence.source_entries,
                    tip,
                    &mut zip
                )?,
                crate::native_relation::LiveRelation::Consistent
            )
        {
            Some(tip)
        } else {
            None
        }
    } else {
        None
    };
    let mut history_error = None;
    if !selected_working && let Some((_, working)) = &history.working {
        let relation = tip.map_or_else(
            || {
                crate::native_relation::LiveRelation::Unverified(
                    "the retained head tip is missing".into(),
                )
            },
            |tip| crate::native_relation::history_matches(working, tip),
        );
        if let Some(error) = relation_error(
            relation,
            crate::NativeFailureCode::WorkingNotRepresented,
            crate::NativeFailureCode::WorkingUnverified,
            "history.working",
        ) {
            if facts.standalone_strict || strict_history {
                return Err(error);
            }
            history_error = Some(error.to_string());
        }
    }
    if let Some(candidate) = candidate {
        // These aids deliberately have no historical wire representation.
        let colors = std::mem::take(&mut live.colors);
        let drawing_guides = std::mem::take(&mut live.drawing_guides);
        live = candidate.clone();
        live.colors = colors;
        live.drawing_guides = drawing_guides;
    }
    Ok(Opened {
        doc: live,
        graph: Some(history.graph),
        history_error,
    })
}

pub(crate) struct RetiredNative {
    pub graph: Option<Graph>,
    pub diagnostics: Vec<crate::project::ProjectReadDiagnosticCode>,
}

pub(crate) fn read_retired_from<R: Read + Seek>(
    reader: R,
) -> std::result::Result<RetiredNative, NativeReadFailure> {
    // Container failures are deliberately outside legacy recovery.
    let mut zip = ZipArchive::new(reader).map_err(|error| NativeReadFailure {
        facts: crate::native_admission::ArchiveFacts {
            retention: crate::native_admission::Retention::Indeterminate,
            standalone_strict: true,
            native_version: None,
        },
        stage: "container",
        source: error.into(),
    })?;
    let mut decoded = decode_native(&mut zip)?;
    let fail = |stage, source| NativeReadFailure {
        facts: decoded.facts.clone(),
        stage,
        source,
    };
    let mut diagnostics = Vec::new();
    let Some(history) = &decoded.history else {
        if decoded.facts.protected() {
            return Err(fail(
                "history",
                decoded
                    .history_error
                    .unwrap_or_else(|| IoError::NativePreservation {
                        code: crate::NativeFailureCode::RetiredLiveNotRepresented,
                        location: "live".into(),
                        detail: "a retired archive without a graph cannot retain its live document"
                            .into(),
                    }),
            ));
        }
        return Ok(RetiredNative {
            graph: None,
            diagnostics: vec![
                crate::project::ProjectReadDiagnosticCode::OmittedLegacyRetiredArchive,
            ],
        });
    };
    let tip = history
        .graph
        .commit(history.graph.head_branch().tip)
        .ok_or_else(|| fail("history", IoError::Manifest("missing head tip".into())))?;
    let relation = crate::native_relation::live_representation_matches(
        &decoded.live,
        &decoded.evidence.source_entries,
        &tip.doc,
        &mut zip,
    )
    .map_err(|error| fail("live relation", error))?;
    if let Some(error) = relation_error(
        relation,
        crate::NativeFailureCode::RetiredLiveNotRepresented,
        crate::NativeFailureCode::RetiredLiveUnverified,
        "retired.live",
    ) {
        if decoded.facts.protected() {
            return Err(fail("live relation", error));
        }
        if !diagnostics
            .contains(&crate::project::ProjectReadDiagnosticCode::UnrepresentedLegacyRetiredLive)
        {
            diagnostics
                .push(crate::project::ProjectReadDiagnosticCode::UnrepresentedLegacyRetiredLive);
        }
    }
    if let Some((_, working)) = &history.working
        && let Some(error) = relation_error(
            crate::native_relation::history_matches(working, &tip.doc),
            crate::NativeFailureCode::RetiredWorkingNotRepresented,
            crate::NativeFailureCode::RetiredWorkingUnverified,
            "retired.history.working",
        )
    {
        if decoded.facts.protected() {
            return Err(fail("working relation", error));
        }
        diagnostics
            .push(crate::project::ProjectReadDiagnosticCode::UnrepresentedLegacyRetiredWorking);
    }
    if let Some(error) = decoded.evidence.auxiliary_error.take() {
        if decoded.facts.protected() {
            return Err(fail("live aids", error));
        }
        diagnostics.push(crate::project::ProjectReadDiagnosticCode::SanitizedLegacyRetiredAids);
    }
    // Authored/source and exact working comparisons must precede this overlay:
    // decoded history documents deliberately have default nonhistorical aids.
    let graph = decoded.history.map(|mut history| {
        history.graph.set_retired_live_aids(
            std::mem::take(&mut decoded.live.colors),
            std::mem::take(&mut decoded.live.drawing_guides),
        );
        history.graph
    });
    Ok(RetiredNative { graph, diagnostics })
}

/// Read a native document (or any ORA).
pub fn read(path: &Path) -> Result<Document> {
    let file = std::fs::File::open(path)?;
    let mut zip = ZipArchive::new(std::io::BufReader::new(file))?;
    if let Some(error) = crate::native_admission::inspect_archive(&mut zip).error {
        return Err(error);
    }
    let strict_history = crate::native_features::preflight_archive(&mut zip)?;
    preflight_archive_metadata(&mut zip, strict_history)?;
    let mut originals = crate::original_image_data::preflight_archive(&mut zip)?;
    read_document(
        &mut zip,
        &mut crate::path_data::PathReader::default(),
        &mut originals,
        &mut LiveEvidence::default(),
    )
}

fn read_document<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    paths: &mut crate::path_data::PathReader,
    originals: &mut crate::original_image_data::OriginalImagePool,
    evidence: &mut LiveEvidence,
) -> Result<Document> {
    if let Ok(m) = read_entry(zip, "mimetype", 64)
        && m.trim_ascii() != b"image/openraster"
    {
        return Err(IoError::Unsupported("zip is not an OpenRaster file".into()));
    }
    let doc = if zip.by_name(MANIFEST).is_ok() {
        read_manifest(zip, paths, originals, evidence)?
    } else {
        read_stack(zip)?
    };
    doc.validate()?;
    Ok(doc)
}

/// Preflight references before inflating binary pattern assets. Shared entries
/// count once, while unrelated referenced images share one aggregate budget.
fn referenced_patterns(manifest: &Manifest) -> Result<Vec<usize>> {
    let mut references = std::collections::BTreeSet::new();
    for node in &manifest.nodes {
        if node.pattern_refs.len() > node.style_options.len() {
            return Err(IoError::Manifest(
                "pattern references do not match effects".into(),
            ));
        }
        references.extend(node.pattern_refs.iter().flatten().copied());
    }
    let mut sources = HashMap::new();
    let mut total = 0u64;
    for index in &references {
        let metadata = manifest
            .patterns
            .get(*index)
            .ok_or_else(|| IoError::Manifest("missing pattern reference".into()))?;
        if metadata.width == 0
            || metadata.height == 0
            || metadata.width > 2048
            || metadata.height > 2048
        {
            return Err(IoError::Manifest(
                "pattern image dimensions exceed supported size".into(),
            ));
        }
        let dimensions = (metadata.width, metadata.height);
        if let Some(previous) = sources.insert(metadata.src.as_str(), dimensions) {
            if previous != dimensions {
                return Err(IoError::Manifest("pattern dimensions disagree".into()));
            }
        } else {
            total += metadata.width as u64 * metadata.height as u64 * 4;
            if total > MAX_NATIVE_MANIFEST_BYTES {
                return Err(IoError::Manifest(
                    "referenced pattern images exceed the 512 MiB asset budget".into(),
                ));
            }
        }
    }
    Ok(references.into_iter().collect())
}

fn wire_support_metadata<'a>(
    node: &'a MNode,
    raster_plane: Option<emulsion_core::smart_support::MaskPlaneMetadata>,
    filter_plane: Option<emulsion_core::smart_support::MaskPlaneMetadata>,
) -> Result<Option<emulsion_core::smart_support::SmartSupportMetadata<'a>>> {
    use emulsion_core::smart_support::{
        ComponentMaskMetadata, MaskPlaneMetadata, SmartSupportMetadata,
    };
    let owner = if matches!(&node.kind, MKind::Smart { .. }) {
        ComponentOwner::Smart
    } else {
        ComponentOwner::Other
    };
    let transform = node.mask_transform.into_raster_mask(owner)?;
    if let MKind::Raster {
        width,
        height,
        placement,
        ..
    } = &node.kind
    {
        check_size(*width, *height)?;
        placement.into_raster()?;
    }
    let MKind::Smart {
        width,
        height,
        placement,
        filters,
        filter_styles,
        filters_enabled,
        filter_mask,
        ..
    } = &node.kind
    else {
        return Ok(None);
    };
    check_size(*width, *height)?;
    if filters.len() > 32 {
        return Err(IoError::Manifest("too many filters".into()));
    }
    let filter_mask = filter_mask
        .as_ref()
        .map(|mask| {
            mask.validate_resource()?;
            Ok::<_, IoError>(ComponentMaskMetadata {
                plane: Some(filter_plane.unwrap_or(MaskPlaneMetadata {
                    size: (mask.width, mask.height),
                    has_detail: false,
                })),
                transform: mask.transform.into_filter_mask()?,
                properties: mask.properties,
                enabled: mask.enabled,
                linked: mask.linked,
            })
        })
        .transpose()?;
    Ok(Some(SmartSupportMetadata {
        source_size: (*width, *height),
        placement: placement.into_smart(),
        filters,
        styles: filter_styles,
        filters_enabled: *filters_enabled,
        retained_cache: None,
        raster_mask: ComponentMaskMetadata {
            plane: raster_plane,
            transform,
            properties: node.mask_properties,
            enabled: node.mask_enabled,
            linked: node.mask_linked,
        },
        filter_mask,
        has_vector_mask: node.vector_mask.is_some(),
    }))
}

/// Maps, owners, source dimensions and predicted filter/cache support precede
/// resource lookup. PNG headers then supply intrinsic mask dimensions without
/// guessing whether the decoded sparse plane contains detail.
fn preflight_manifest_metadata<R: Read + Seek>(
    m: &Manifest,
    zip: &mut ZipArchive<R>,
) -> Result<()> {
    use emulsion_core::smart_support::{MaskPlaneMetadata, preflight_pending_mask_resources};
    check_size(m.width, m.height)?;
    if m.nodes.len() > emulsion_core::document::MAX_NODES {
        return Err(IoError::Manifest("too many nodes".into()));
    }
    for node in &m.nodes {
        if let Some(metadata) = wire_support_metadata(node, None, None)? {
            if metadata.features().any() && m.version < 16 {
                return Err(IoError::Manifest(
                    "projective Smart mappings require native version 16".into(),
                ));
            }
            preflight_pending_mask_resources(metadata)
                .map_err(|e| IoError::Manifest(e.to_string()))?;
        }
    }
    for node in &m.nodes {
        let Some(metadata) = wire_support_metadata(node, None, None)? else {
            continue;
        };
        if !metadata.features().any() {
            continue;
        }
        let plane = node
            .mask
            .as_ref()
            .map(|path| {
                use image::ImageDecoder as _;
                let bytes = read_entry(zip, path, MAX_ENTRY_BYTES)?;
                let decoder = image::codecs::png::PngDecoder::new(std::io::Cursor::new(bytes))?;
                let size = decoder.dimensions();
                check_size(size.0, size.1)?;
                if decoder.color_type() != image::ColorType::L8 {
                    return Err(IoError::Manifest(
                        "raster-mask PNG must be grayscale".into(),
                    ));
                }
                Ok::<_, IoError>(MaskPlaneMetadata {
                    size,
                    has_detail: false,
                })
            })
            .transpose()?;
        preflight_pending_mask_resources(
            wire_support_metadata(node, plane, None)?.expect("Smart metadata"),
        )
        .map_err(|e| IoError::Manifest(e.to_string()))?;
    }
    Ok(())
}

fn preflight_archive_metadata<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    strict_history: bool,
) -> Result<()> {
    crate::history::preflight_metadata(zip, strict_history)?;
    match zip.by_name(MANIFEST) {
        Ok(_) => {}
        Err(zip::result::ZipError::FileNotFound) => return Ok(()),
        Err(error) => return Err(error.into()),
    }
    let bytes = read_entry(zip, MANIFEST, MAX_NATIVE_MANIFEST_BYTES)?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|e| IoError::Manifest(e.to_string()))?;
    preflight_manifest_metadata(&manifest, zip)
}

fn read_manifest<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    paths: &mut crate::path_data::PathReader,
    originals: &mut crate::original_image_data::OriginalImagePool,
    evidence: &mut LiveEvidence,
) -> Result<Document> {
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
    let mut m: Manifest =
        serde_json::from_slice(&bytes).map_err(|e| IoError::Manifest(e.to_string()))?;
    drop(bytes);
    if m.format != "emulsion" {
        return Err(IoError::Manifest(format!("unknown format {:?}", m.format)));
    }
    crate::native_features::check_version(version, m.blend_space, m.psd_background)?;
    check_size(m.width, m.height)?;
    if m.nodes.len() > emulsion_core::document::MAX_NODES {
        return Err(IoError::Manifest("too many nodes".into()));
    }

    preflight_manifest_metadata(&m, zip)?;
    // Validate every descriptor, reference and PNG header before materializing
    // any planes. Sharing compressed resources does not weaken dimension checks.
    let mut filter_blobs = HashMap::new();
    for node in &m.nodes {
        if let MKind::Smart {
            filters,
            filter_styles,
            filters_enabled,
            ..
        } = &node.kind
        {
            crate::native_features::check_filters_version(version, filters)?;
            crate::native_features::check_enabled_version(
                version,
                filters,
                filter_styles,
                *filters_enabled,
            )?;
        }
        if let MKind::Smart {
            filter_mask: Some(mask),
            ..
        } = &node.kind
        {
            if version < 12 {
                return Err(IoError::Manifest(
                    "Smart Filter masks require native version 12".into(),
                ));
            }
            mask.validate_resource()?;
            let bytes = match filter_blobs.entry(mask.pixels.clone()) {
                std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(read_entry(zip, &mask.pixels, MAX_ENTRY_BYTES)?)
                }
            };
            mask.validate_png(bytes)?;
        }
    }

    // Validate all referenced sizes and the aggregate budget before allocating any
    // image bytes. Unused table entries are not part of the live document.
    let references = referenced_patterns(&m)?;
    let mut pattern_cache: HashMap<String, Arc<emulsion_core::style_options::PatternImage>> =
        HashMap::new();
    let mut patterns = HashMap::new();
    for index in references {
        let metadata = &m.patterns[index];
        let expected = metadata.width as usize * metadata.height as usize * 4;
        let pattern = if let Some(image) = pattern_cache.get(&metadata.src) {
            image.clone()
        } else {
            let pixels = read_entry(zip, &metadata.src, expected as u64)?;
            if pixels.len() != expected {
                return Err(IoError::Manifest(
                    "pattern image has the wrong length".into(),
                ));
            }
            let image = Arc::new(emulsion_core::style_options::PatternImage {
                width: metadata.width,
                height: metadata.height,
                pixels,
            });
            pattern_cache.insert(metadata.src.clone(), image.clone());
            image
        };
        patterns.insert(index, pattern);
    }

    for node in &m.nodes {
        if let MKind::Raster { src, .. }
        | MKind::Smart {
            src,
            original_image: None,
            ..
        } = &node.kind
        {
            evidence.source_entries.push((node.id, src.clone()));
        }
    }

    // Record raw validity before the compatibility reader sanitizes/truncates
    // aids. Protected retired admission must never present that loss as exact.
    evidence.auxiliary_error = validate_retired_live_aids(&m.colors, &m.drawing_guides).err();

    // Read compressed bytes sequentially, decode in parallel.
    let mut blobs: HashMap<String, Vec<u8>> = HashMap::new();
    for n in &m.nodes {
        if let MKind::Raster { src, .. }
        | MKind::Smart {
            original_image: None,
            src,
            ..
        } = &n.kind
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
    // Source descriptors do not authorize decoding a differently sized PNG.
    // Check every available native source header before decoding any pixels.
    for node in &m.nodes {
        if let MKind::Raster {
            src, width, height, ..
        }
        | MKind::Smart {
            src,
            width,
            height,
            original_image: None,
            ..
        } = &node.kind
            && png_dimensions(&blobs[src])? != (*width, *height)
        {
            return Err(IoError::Manifest(format!(
                "{src} dimensions differ from its source descriptor"
            )));
        }
    }
    let rasters: HashMap<String, Result<(Raster, u8)>> = m
        .nodes
        .par_iter()
        .filter_map(|n| match &n.kind {
            MKind::Raster { src, .. }
            | MKind::Smart {
                original_image: None,
                src,
                ..
            } => Some((src.clone(), decode_png(&blobs[src]))),
            _ => None,
        })
        .collect();
    let masks: HashMap<(String, u8), Result<Mask>> = m
        .nodes
        .par_iter()
        .filter_map(|n| {
            n.mask.as_ref().map(|p| {
                (
                    (p.clone(), n.mask_fill),
                    decode_mask(&blobs[p], n.mask_fill),
                )
            })
        })
        .collect();

    let mut doc = Document::new(m.width, m.height);
    doc.resolution = m.resolution;
    doc.global_light = m.global_light;
    doc.diagram = m.diagram.clone();
    doc.design = m.design.clone();
    crate::font_data::FontPool::default().restore(&mut doc.design, &m.fonts, zip, "fonts")?;
    crate::media_data::MediaPool::default().restore(
        &mut doc.design,
        &m.media_resources,
        zip,
        "media",
    )?;
    doc.source_depth = if m.source_depth == 16 { 16 } else { 8 };
    doc.blend_space = m.blend_space;
    doc.psd_background = m.psd_background;
    doc.guides = m.guides.clone();
    doc.info = m.info.clone();
    doc.raw = m.raw.clone();
    doc.raw_originals = m.raw_originals.clone();
    doc.colors = std::mem::take(&mut m.colors);
    doc.colors
        .truncate(emulsion_core::document::MAX_PROJECT_COLORS);
    // Guides are a drawing aid: drop malformed ones rather than the file.
    if m.drawing_guides.validate().is_ok() {
        doc.drawing_guides = std::mem::take(&mut m.drawing_guides);
    }
    let mut filter_masks: HashMap<(String, u8), Arc<Mask>> = HashMap::new();
    for node in &m.nodes {
        if let MKind::Smart {
            filter_mask: Some(mask),
            ..
        } = &node.kind
        {
            let key = (mask.pixels.clone(), mask.fill);
            if let std::collections::hash_map::Entry::Vacant(entry) = filter_masks.entry(key) {
                entry.insert(mask.decode_png(&filter_blobs[&mask.pixels])?);
            }
        }
    }
    drop(filter_blobs);
    for node in &m.nodes {
        use emulsion_core::smart_support::{MaskPlaneMetadata, preflight_stack_support};
        let raster_plane = node
            .mask
            .as_ref()
            .map(|path| {
                let mask = masks
                    .get(&(path.clone(), node.mask_fill))
                    .ok_or_else(|| IoError::Manifest("missing raster mask".into()))?
                    .as_ref()
                    .map_err(|e| IoError::Manifest(e.to_string()))?;
                Ok::<_, IoError>(MaskPlaneMetadata::from_mask(mask))
            })
            .transpose()?;
        let filter_plane = match &node.kind {
            MKind::Smart {
                filter_mask: Some(mask),
                ..
            } => Some(MaskPlaneMetadata::from_mask(
                filter_masks
                    .get(&(mask.pixels.clone(), mask.fill))
                    .ok_or_else(|| IoError::Manifest("missing Smart Filter mask".into()))?,
            )),
            _ => None,
        };
        if let Some(metadata) = wire_support_metadata(node, raster_plane, filter_plane)? {
            preflight_stack_support(metadata).map_err(|e| IoError::Manifest(e.to_string()))?;
        }
    }
    let mut raster_cache: HashMap<String, Arc<Raster>> = HashMap::new();
    let mut sources = crate::smart_source_data::SourcePool::default();
    for mut n in m.nodes {
        if m.version < 11 && n.vector_mask.is_some() {
            return Err(IoError::Manifest(
                "vector masks require native version 11".into(),
            ));
        }
        if n.pattern_refs.len() > n.style_options.len() {
            return Err(IoError::Manifest(
                "pattern references do not match effects".into(),
            ));
        }
        for (option, reference) in n.style_options.iter_mut().zip(n.pattern_refs) {
            if let Some(index) = reference {
                option.pattern.image = Some(
                    patterns
                        .get(&index)
                        .cloned()
                        .ok_or_else(|| IoError::Manifest("missing pattern reference".into()))?,
                );
            }
        }
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
                    placement: placement.into_raster()?,
                }
            }
            MKind::Group { collapsed } => NodeKind::Group { collapsed },
            MKind::Adjust { adjustment } => NodeKind::Adjust(adjustment),
            MKind::Fill { rgba } => NodeKind::Fill { rgba },
            MKind::Smart {
                editable,
                source_document,
                original_image,
                src,
                width,
                height,
                filters,
                filter_styles,
                filters_enabled,
                filter_mask,
                placement,
            } => {
                let (original_image, original_source) = if let Some(reference) = original_image {
                    if version < 13
                        || reference.path() != src
                        || (reference.width, reference.height) != (width, height)
                    {
                        return Err(IoError::Manifest(
                            "Invalid version or source path for original PNG".into(),
                        ));
                    }
                    let (original, raster) =
                        originals.restore(&reference, None, editable.is_some())?;
                    (Some(original), Some(raster))
                } else {
                    (None, None)
                };
                let r = if let Some(source) = original_source {
                    source
                } else {
                    match raster_cache.get(&src) {
                        Some(r) => r.clone(),
                        None => {
                            let (r, _) = rasters[&src]
                                .as_ref()
                                .map_err(|e| IoError::Manifest(format!("{src}: {e}")))?;
                            let r = Arc::new(r.clone());
                            raster_cache.insert(src.clone(), r.clone());
                            r
                        }
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
                let (cache, offset) = emulsion_core::smart::render_stack(
                    &r,
                    &filters,
                    &filter_styles,
                    filters_enabled,
                );
                NodeKind::Smart {
                    editable: sources.restore(editable, source_document, zip, "sources")?,
                    original_image,
                    source: r,
                    filters,
                    filter_styles,
                    filters_enabled,
                    filter_mask: filter_mask
                        .map(|mask| {
                            let pixels = filter_masks
                                .get(&(mask.pixels.clone(), mask.fill))
                                .cloned()
                                .ok_or_else(|| {
                                    IoError::Manifest("missing Smart Filter mask".into())
                                })?;
                            (*mask).decode(pixels)
                        })
                        .transpose()?,
                    placement: placement.into_smart(),
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
                let cache = emulsion_core::vector_cache::VectorRaster::path(
                    path.clone(),
                    style,
                    m.width,
                    m.height,
                );
                NodeKind::Path { path, style, cache }
            }
            MKind::Text { spec, .. } => {
                let spec = Arc::new(spec.sanitized());
                let cache = emulsion_core::vector_cache::VectorRaster::text(
                    spec.clone(),
                    m.width,
                    m.height,
                );
                NodeKind::Text { spec, cache }
            }
            MKind::Strokes { strokes, .. } => {
                strokes
                    .validate()
                    .map_err(|e| IoError::Manifest(format!("vector strokes: {e}")))?;
                let strokes = Arc::new(strokes);
                let cache = emulsion_core::vector_cache::VectorRaster::strokes(
                    strokes.clone(),
                    m.width,
                    m.height,
                );
                NodeKind::Strokes { strokes, cache }
            }
        };
        let mask = match &n.mask {
            None => None,
            Some(p) => {
                let mk = masks[&(p.clone(), n.mask_fill)]
                    .as_ref()
                    .map_err(|e| IoError::Manifest(format!("{p}: {e}")))?;
                let (ew, eh) = match &kind {
                    NodeKind::Raster { raster, .. } => (raster.width(), raster.height()),
                    NodeKind::Smart { source, .. } => (source.width(), source.height()),
                    _ => (m.width, m.height),
                };
                let mk = mk.clone();
                // v1/v2 UI created document-space masks on smart nodes, but
                // raster-to-smart conversion retained source-sized masks.
                // Source dimensions take precedence in the ambiguous equal-
                // size case, matching the renderer used by those versions.
                let mk = if m.version < 3
                    && (mk.width(), mk.height()) != (ew, eh)
                    && let NodeKind::Smart { placement, .. } = &kind
                    && (mk.width(), mk.height()) == (m.width, m.height)
                {
                    let to_doc = placement
                        .require_legacy("legacy native mask conversion")?
                        .to_doc(ew, eh);
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
                // Since v10 every node retains bounded intrinsic mask pixels
                // through geometry and source-kind changes. mask_transform
                // maps the raw plane into the node's current local space.
                if m.version < 10 && (mk.width() != ew || mk.height() != eh) {
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
            link_group: n.link_group,
            opacity: n.opacity,
            blend: n.blend,
            blending: n.blending,
            clip_to: n.clip_to,
            mask,
            mask_enabled: n.mask_enabled,
            mask_linked: n.mask_linked,
            mask_transform: n.mask_transform.into_raster_mask(
                if matches!(&kind, NodeKind::Smart { .. }) {
                    ComponentOwner::Smart
                } else {
                    ComponentOwner::Other
                },
            )?,
            mask_properties: n.mask_properties,
            vector_mask: n
                .vector_mask
                .map(|mask| mask.decode(paths, zip))
                .transpose()?,
            styles: n.styles,
            style_options: n.style_options,
            effects_enabled: n.effects_enabled,
            origin: n.origin,
            review: n.review,
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
    use emulsion_core::DocumentError;
    use emulsion_core::document::{MAX_DEPTH, MAX_NODES};
    let mut size = None;
    let mut stack: Vec<Item> = Vec::new();
    let mut root: Option<Item> = None;
    let mut buf = Vec::new();
    // Enforce the document limits while parsing, so a hostile stack.xml
    // cannot build a tree too deep to recurse over (or drop) or too big to
    // hold. The root stack is not a node; its children have depth 0.
    let mut nodes = 0usize;
    let mut count_node = |open: usize| -> Result<()> {
        nodes += 1;
        if nodes > MAX_NODES {
            return Err(DocumentError::TooManyNodes(MAX_NODES).into());
        }
        if open > MAX_DEPTH + 1 {
            return Err(DocumentError::TooDeep(MAX_DEPTH).into());
        }
        Ok(())
    };
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
                if !stack.is_empty() {
                    count_node(stack.len())?;
                }
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
                count_node(stack.len())?;
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
    // One decoded raster per distinct src, shared by every layer using it.
    let decoded: HashMap<String, Result<(Arc<Raster>, u8)>> = blobs
        .par_iter()
        .map(|(k, v)| (k.clone(), decode_png(v).map(|(r, d)| (Arc::new(r), d))))
        .collect();

    let mut doc = Document::new(w, h);
    fn build(
        doc: &mut Document,
        item: &Item,
        parent: Option<NodeId>,
        decoded: &HashMap<String, Result<(Arc<Raster>, u8)>>,
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
                    raster: r.clone(),
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

    #[test]
    fn smart_filter_mask_box_preserves_manifest_json_shape_and_legacy_omission() {
        let descriptor = serde_json::json!({
            "pixels": "emulsion/filter-mask-7.png",
            "width": 8,
            "height": 6,
            "fill": 127,
            "enabled": false,
            "linked": false,
            "transform": [1.0, 0.125, -0.25, 1.0, -0.5, 0.75],
            "properties": { "density": 0.625, "feather": 1.25 }
        });
        let legacy = serde_json::json!({
            "type": "smart",
            "src": "emulsion/src/node-7.png",
            "width": 8,
            "height": 6,
            "filters": [],
            "filter_styles": [],
            "placement": serde_json::to_value(Placement::default()).unwrap()
        });
        let mut with_mask = legacy.clone();
        with_mask["filter_mask"] = descriptor;
        for expected in [legacy, with_mask] {
            let decoded: MKind = serde_json::from_value(expected.clone()).unwrap();
            assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
        }
    }

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

    #[test]
    fn raw_recipe_and_original_link_round_trip_in_document_and_history() {
        use emulsion_core::raw::{DevelopParams, RawDocument, RawMetadata};
        let mut doc = sample_doc();
        let id = doc.nodes[0].id;
        doc.raw = Some(RawDocument {
            schema_version: 1,
            node_id: id,
            source: "originals/camera.dng".into(),
            source_sha256: "ab".repeat(32),
            params: DevelopParams::default(),
            metadata: RawMetadata {
                make: "Test".into(),
                model: "Bayer 14".into(),
                compression: "lossless".into(),
                bits_per_sample: 14,
                ..Default::default()
            },
        });
        let original = doc.raw.clone();
        doc.raw_originals = vec![std::path::PathBuf::from("originals/camera.dng")];
        let mut editor = emulsion_core::Editor::new(doc, None);
        let pixels = match &editor.doc.nodes[0].kind {
            NodeKind::Raster { raster, .. } => {
                Arc::new(Raster::solid(raster.width(), raster.height(), [0.5; 4]))
            }
            _ => panic!("raster source"),
        };
        editor
            .execute(Command::DevelopRaw {
                id,
                raster: pixels,
                params: Box::new(DevelopParams {
                    exposure: -1.0,
                    temperature: 0.3,
                    ..Default::default()
                }),
            })
            .unwrap();
        editor.commit("Developed", false);
        let path = tmp("raw-recipe-history.ora");
        write_full(&editor.doc, Some(&editor.graph), &path).unwrap();
        let reopened = read_full(&path).unwrap();
        assert!(reopened.history_error.is_none());
        assert_eq!(reopened.doc.raw, editor.doc.raw);
        assert_eq!(reopened.doc.raw_originals, editor.doc.raw_originals);
        let graph = reopened.graph.unwrap();
        assert!(graph.commits().any(|commit| commit.doc.raw == original));
        assert!(
            graph
                .commits()
                .any(|commit| commit.doc.raw == editor.doc.raw)
        );
    }

    #[test]
    fn raw_original_stays_protected_after_painting_and_native_reopen() {
        use emulsion_core::raw::{DevelopParams, RawDocument, RawMetadata};
        let original = tmp("protected-original.tif");
        let relocated = tmp("relocated-original.tif");
        let native = tmp("protected-original-edits.ora");
        std::fs::write(&original, b"unchanged RAW original").unwrap();
        std::fs::write(&relocated, b"unchanged RAW original").unwrap();
        let mut doc = Document::new(2, 2);
        doc.nodes.push(Node::raster(
            1,
            "RAW",
            Arc::new(Raster::solid(2, 2, [0.5; 4])),
            Default::default(),
        ));
        doc.next_id = 2;
        doc.raw = Some(RawDocument {
            schema_version: 1,
            node_id: 1,
            source: original.clone(),
            source_sha256: "a".repeat(64),
            params: DevelopParams::default(),
            metadata: RawMetadata::default(),
        });
        assert!(write(&doc, &original).is_err());
        let mut editor = emulsion_core::Editor::new(doc, None);
        editor
            .execute(Command::RelinkRaw {
                source: relocated.clone(),
            })
            .unwrap();
        editor.undo();
        let mut doc = editor.doc;
        assert!(doc.raw_originals.contains(&relocated));
        Command::ReplacePixels {
            id: 1,
            raster: Arc::new(Raster::solid(2, 2, [1.0; 4])),
            dirty: emulsion_raster::IRect::new(0, 0, 2, 2),
            label: "Paint".into(),
        }
        .apply(&mut doc)
        .unwrap();
        assert!(doc.raw.is_none());
        assert!(doc.raw_originals.contains(&original));
        assert!(write(&doc, &original).is_err());
        write(&doc, &native).unwrap();
        let reopened = read_full(&native).unwrap().doc;
        assert!(ensure_not_raw_original(&reopened, &original).is_err());
        assert!(write(&reopened, &original).is_err());
        assert!(write(&reopened, &relocated).is_err());
        for path in [&original, &relocated] {
            assert!(
                crate::export::export(
                    &reopened,
                    path,
                    crate::export::ExportOptions::for_doc(&reopened)
                )
                .is_err()
            );
            assert_eq!(std::fs::read(path).unwrap(), b"unchanged RAW original");
        }
        assert_eq!(std::fs::read(&original).unwrap(), b"unchanged RAW original");
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

    fn mask_properties_document() -> Document {
        let mut doc = Document::new(7, 5);
        doc.source_depth = 16;
        let mut node = Node::raster(
            1,
            "Persistent mask",
            Arc::new(Raster::solid(7, 5, [1.0, 0.0, 0.0, 1.0])),
            Placement::default(),
        );
        node.mask = Some(Arc::new(Mask::from_fn(7, 5, 0, |x, y| {
            if (2..5).contains(&x) && (1..4).contains(&y) {
                255
            } else {
                0
            }
        })));
        node.mask_linked = false;
        node.mask_transform = Mapping2::Affine(glam::DAffine2::from_cols_array(&[
            1.0, 0.0, 0.0, 1.0, 0.5, 0.0,
        ]));
        doc.nodes.push(node);
        doc.next_id = 2;
        doc
    }

    fn assert_mask_properties_document(actual: &Document, expected: &Document) {
        let actual_node = &actual.nodes[0];
        let expected_node = &expected.nodes[0];
        assert_eq!(actual_node.mask_properties, expected_node.mask_properties);
        assert_eq!(actual_node.mask_enabled, expected_node.mask_enabled);
        assert_eq!(actual_node.mask_linked, expected_node.mask_linked);
        assert_eq!(actual_node.mask_transform, expected_node.mask_transform);
        let actual_mask = actual_node.mask.as_ref().unwrap();
        let expected_mask = expected_node.mask.as_ref().unwrap();
        assert_eq!(actual_mask.fill(), expected_mask.fill());
        assert_eq!(actual_mask.to_gray8(), expected_mask.to_gray8());
        let (
            NodeKind::Raster { raster: actual, .. },
            NodeKind::Raster {
                raster: expected, ..
            },
        ) = (&actual_node.kind, &expected_node.kind)
        else {
            panic!("raster source preserved");
        };
        assert_eq!(actual.to_srgba16(), expected.to_srgba16());
    }

    #[test]
    fn persistent_mask_properties_roundtrip_raw_pixels_history_and_working_copy() {
        use emulsion_core::MaskProperties;
        let original = mask_properties_document();
        let raw = original.nodes[0].mask.as_ref().unwrap().clone();
        let mut graph = Graph::new(original.clone(), "Original mask");
        let mut modified = original.clone();
        modified.nodes[0].mask_properties = MaskProperties {
            density: 0.4,
            feather: 1.75,
        };
        assert!(graph.record(&modified, "Mask properties", false).is_some());
        let mut working = modified.clone();
        working.nodes[0].mask_properties.density = 0.75;
        working.nodes[0].mask_enabled = false;
        let path = tmp("persistent-mask-properties.ora");
        for doc in [&modified, &working] {
            for with_history in [false, true] {
                write_full(doc, with_history.then_some(&graph), &path).unwrap();
                let reopened = read_full(&path).unwrap();
                assert!(reopened.history_error.is_none());
                assert_mask_properties_document(&reopened.doc, doc);
                assert_eq!(
                    flatten(&reopened.doc.composite_tree(), 0).to_srgba8(),
                    flatten(&doc.composite_tree(), 0).to_srgba8(),
                );
                if with_history {
                    let restored = reopened.graph.unwrap();
                    assert_eq!(restored.len(), graph.len());
                    let mut previous_mask = None;
                    for (actual, expected) in restored.commits().zip(graph.commits()) {
                        assert_mask_properties_document(&actual.doc, &expected.doc);
                        let mask = actual.doc.nodes[0].mask.as_ref().unwrap();
                        if let Some(previous) = &previous_mask {
                            assert!(Arc::ptr_eq(previous, mask), "raw masks remain shared");
                        }
                        previous_mask = Some(mask.clone());
                    }
                } else {
                    assert!(reopened.graph.is_none());
                }
            }
        }
        assert!(Arc::ptr_eq(&raw, modified.nodes[0].mask.as_ref().unwrap()));
        assert_eq!(
            raw.to_gray8(),
            working.nodes[0].mask.as_ref().unwrap().to_gray8()
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn mask_properties_format_version_tracks_live_and_serialized_history() {
        let default = mask_properties_document();
        let mut modified = default.clone();
        modified.nodes[0].mask_properties = emulsion_core::MaskProperties {
            density: 0.4,
            feather: 2.0,
        };
        let default_graph = Graph::new(default.clone(), "Default");
        let mut property_graph = Graph::new(modified.clone(), "Properties");
        property_graph
            .record(&default, "Reset properties", false)
            .unwrap();
        let path = tmp("mask-properties-format-version.ora");
        for (doc, graph, native_version, history_version) in [
            (&default, None, 9, None),
            (&default, Some(&default_graph), 9, Some(9)),
            (&modified, None, 10, None),
            (&modified, Some(&default_graph), 10, Some(10)),
            (&default, Some(&property_graph), 10, Some(10)),
        ] {
            write_full(doc, graph, &path).unwrap();
            let mut zip = ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
            let manifest: serde_json::Value = serde_json::from_slice(
                &read_entry(&mut zip, MANIFEST, MAX_NATIVE_MANIFEST_BYTES).unwrap(),
            )
            .unwrap();
            assert_eq!(manifest["version"], native_version);
            assert_eq!(
                manifest["nodes"][0].get("mask_properties").is_some(),
                requires_mask_v10(doc),
            );
            if let Some(version) = history_version {
                let history: serde_json::Value = serde_json::from_slice(
                    &read_entry(&mut zip, crate::history::GRAPH, MAX_NATIVE_MANIFEST_BYTES)
                        .unwrap(),
                )
                .unwrap();
                assert_eq!(history["version"], version);
            }
            drop(zip);
            let reopened = read_full(&path).unwrap();
            assert!(reopened.history_error.is_none());
            assert_mask_properties_document(&reopened.doc, doc);
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn legacy_mask_properties_default_in_native_and_history_without_pixel_changes() {
        let doc = mask_properties_document();
        let graph = Graph::new(doc.clone(), "Legacy mask");
        let path = tmp("legacy-mask-properties.ora");
        write_full(&doc, Some(&graph), &path).unwrap();
        rewrite_archive(&path, |name, bytes| {
            if name != MANIFEST && name != crate::history::GRAPH {
                return Some(bytes);
            }
            let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            value["version"] = 9.into();
            let remove = |nodes: &mut serde_json::Value| {
                for node in nodes.as_array_mut().unwrap() {
                    node.as_object_mut().unwrap().remove("mask_properties");
                }
            };
            if name == MANIFEST {
                remove(&mut value["nodes"]);
            } else {
                for commit in value["commits"].as_array_mut().unwrap() {
                    remove(&mut commit["doc"]["nodes"]);
                }
            }
            Some(serde_json::to_vec(&value).unwrap())
        });
        let reopened = read_full(&path).unwrap();
        assert!(reopened.history_error.is_none());
        assert_mask_properties_document(&reopened.doc, &doc);
        assert_eq!(
            flatten(&reopened.doc.composite_tree(), 0).to_srgba8(),
            flatten(&doc.composite_tree(), 0).to_srgba8(),
        );
        for commit in reopened.graph.unwrap().commits() {
            assert_eq!(commit.doc.nodes[0].mask_properties, Default::default());
            assert_mask_properties_document(&commit.doc, &doc);
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn invalid_persistent_mask_properties_fail_safely_in_native_and_history() {
        let doc = mask_properties_document();
        let graph = Graph::new(doc.clone(), "Valid mask");
        let path = tmp("invalid-mask-properties.ora");
        for properties in [
            serde_json::json!({"density": -0.1, "feather": 0.0}),
            serde_json::json!({"density": 1.1, "feather": 0.0}),
            serde_json::json!({"density": 0.5, "feather": -1.0}),
            serde_json::json!({"density": 0.5, "feather": emulsion_core::MAX_MASK_FEATHER + 1.0}),
            serde_json::json!({"density": null, "feather": 0.0}),
        ] {
            for target in [MANIFEST, crate::history::GRAPH] {
                write_full(&doc, Some(&graph), &path).unwrap();
                rewrite_archive(&path, |name, bytes| {
                    if name != target {
                        return Some(bytes);
                    }
                    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                    if name == MANIFEST {
                        value["nodes"][0]["mask_properties"] = properties.clone();
                    } else {
                        value["commits"][0]["doc"]["nodes"][0]["mask_properties"] =
                            properties.clone();
                    }
                    Some(serde_json::to_vec(&value).unwrap())
                });
                if target == MANIFEST {
                    assert!(
                        read_full(&path).is_err(),
                        "invalid live properties: {properties}"
                    );
                } else {
                    let reopened = read_full(&path).unwrap();
                    assert!(
                        reopened.history_error.is_some(),
                        "invalid history: {properties}"
                    );
                    assert!(reopened.graph.is_none());
                    assert_mask_properties_document(&reopened.doc, &doc);
                }
            }
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn invalid_mask_properties_are_rejected_before_native_or_history_serialization() {
        let valid = mask_properties_document();
        for (density, feather) in [
            (f32::NAN, 0.0),
            (f32::INFINITY, 0.0),
            (1.0, f32::NAN),
            (1.0, f32::INFINITY),
            (1.0, emulsion_core::MAX_MASK_FEATHER + 1.0),
        ] {
            let mut invalid = valid.clone();
            invalid.nodes[0].mask_enabled = false;
            invalid.nodes[0].mask_properties = emulsion_core::MaskProperties { density, feather };
            assert!(write_to(&invalid, None, std::io::Cursor::new(Vec::new())).is_err());
            let graph = Graph::new(invalid, "Invalid snapshot");
            assert!(write_to(&valid, Some(&graph), std::io::Cursor::new(Vec::new())).is_err());
        }
    }

    #[test]
    fn persistent_mask_properties_standard_ora_fallback_and_previews_match_appearance() {
        let path = tmp("mask-properties-ora-fallback.ora");
        for enabled in [true, false] {
            let mut doc = mask_properties_document();
            doc.nodes[0].mask_properties = emulsion_core::MaskProperties {
                density: 0.5,
                feather: 1.75,
            };
            doc.nodes[0].mask_enabled = enabled;
            let expected = flatten(&doc.composite_tree(), 0).to_srgba8();
            assert!(!merged_is_redundant(&doc));
            write(&doc, &path).unwrap();
            let mut zip = ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
            let xml = read_entry(&mut zip, "stack.xml", MAX_STACK_BYTES).unwrap();
            let xml = String::from_utf8(xml).unwrap();
            assert!(xml.contains("Appearance (editable layers in Emulsion)"));
            assert!(xml.contains("mergedimage.png"));
            let manifest: serde_json::Value = serde_json::from_slice(
                &read_entry(&mut zip, MANIFEST, MAX_NATIVE_MANIFEST_BYTES).unwrap(),
            )
            .unwrap();
            assert_eq!(manifest["version"], 10);
            for name in ["mergedimage.png", "Thumbnails/thumbnail.png"] {
                let bytes = read_entry(&mut zip, name, MAX_ENTRY_BYTES).unwrap();
                assert_eq!(
                    image::load_from_memory(&bytes)
                        .unwrap()
                        .to_rgba8()
                        .into_raw(),
                    expected
                );
            }
            drop(zip);
            rewrite_archive(&path, |name, bytes| (name != MANIFEST).then_some(bytes));
            let generic = read(&path).unwrap();
            assert_eq!(generic.nodes.len(), 1);
            assert!(generic.nodes[0].name.contains("Appearance"));
            assert_eq!(flatten(&generic.composite_tree(), 0).to_srgba8(), expected);
        }
        std::fs::remove_file(path).unwrap();
    }

    fn document_space_mask_document() -> Document {
        let mut doc = Document::new(12, 8);
        doc.source_depth = 16;
        let mut group = Node::group(1, "Intrinsic group mask");
        group.mask = Some(Arc::new(Mask::from_fn(12, 8, 0, |x, y| {
            if (2..10).contains(&x) && (1..7).contains(&y) {
                255
            } else {
                0
            }
        })));
        group.mask_linked = false;
        group.mask_transform = Mapping2::Affine(glam::DAffine2::from_cols_array(&[
            1.0, 0.0, 0.15, 1.0, 0.25, -0.5,
        ]));
        let mut child = Node::raster(
            2,
            "Retained source",
            Arc::new(Raster::solid(12, 8, [1.0, 0.0, 0.0, 1.0])),
            Placement::default(),
        );
        child.parent = Some(group.id);
        // Native stacks store each subtree child-before-parent.
        doc.nodes = vec![child, group];
        doc.next_id = 3;
        doc.validate().unwrap();
        doc
    }

    fn assert_document_space_mask_roundtrip(actual: &Document, expected: &Document) {
        assert_eq!(
            (actual.width, actual.height),
            (expected.width, expected.height)
        );
        assert_eq!(actual.nodes.len(), expected.nodes.len());
        let node = actual.node(1).unwrap();
        let original = expected.node(1).unwrap();
        let mask = node.mask.as_ref().unwrap();
        let raw = original.mask.as_ref().unwrap();
        assert_eq!((mask.width(), mask.height()), (raw.width(), raw.height()));
        assert_eq!(mask.fill(), raw.fill());
        assert_eq!(mask.to_gray8(), raw.to_gray8());
        assert_eq!(node.mask_transform, original.mask_transform);
        assert_eq!(node.mask_properties, original.mask_properties);
        assert_eq!(node.mask_enabled, original.mask_enabled);
        assert_eq!(node.mask_linked, original.mask_linked);
        let (
            NodeKind::Raster {
                raster: actual_source,
                placement: actual_placement,
            },
            NodeKind::Raster {
                raster: expected_source,
                placement: expected_placement,
            },
        ) = (
            &actual.node(2).unwrap().kind,
            &expected.node(2).unwrap().kind,
        )
        else {
            panic!("retained child source");
        };
        assert_eq!(actual_source.to_srgba16(), expected_source.to_srgba16());
        assert_eq!(actual_placement, expected_placement);
        assert_eq!(
            flatten(&actual.composite_tree(), 0).to_srgba8(),
            flatten(&expected.composite_tree(), 0).to_srgba8(),
        );
    }

    #[test]
    fn document_space_mask_properties_after_crop_resize_roundtrip_raw_extents_and_history() {
        let path = tmp("docspace-mask-crop-resize.ora");
        for properties in [
            emulsion_core::MaskProperties::default(),
            emulsion_core::MaskProperties {
                density: 0.4,
                feather: 1.75,
            },
        ] {
            for enabled in [true, false] {
                let mut doc = document_space_mask_document();
                doc.node_mut(1).unwrap().mask_properties = properties;
                doc.node_mut(1).unwrap().mask_enabled = enabled;
                let raw = doc.node(1).unwrap().mask.as_ref().unwrap().clone();
                let initial_transform = doc
                    .node(1)
                    .unwrap()
                    .mask_transform
                    .require_affine("legacy fixture")
                    .unwrap();
                let mut graph = Graph::new(doc.clone(), "Original canvas");
                let crop_transform = glam::DAffine2::from_translation(glam::dvec2(-2.0, -1.0));
                for (command, transform) in [
                    (
                        Command::Crop {
                            rect: emulsion_raster::IRect::new(2, 1, 6, 4),
                            rotation: 0.0,
                        },
                        crop_transform * initial_transform,
                    ),
                    (
                        Command::ImageSize {
                            width: 18,
                            height: 12,
                        },
                        glam::DAffine2::from_scale(glam::dvec2(3.0, 3.0))
                            * crop_transform
                            * initial_transform,
                    ),
                ] {
                    command.apply(&mut doc).unwrap();
                    let mask = doc.node(1).unwrap().mask.as_ref().unwrap();
                    assert!(
                        Arc::ptr_eq(&raw, mask),
                        "geometry retains the raw mask plane"
                    );
                    assert_eq!((mask.width(), mask.height()), (12, 8));
                    assert_eq!(doc.node(1).unwrap().mask_properties, properties);
                    assert_eq!(
                        doc.node(1).unwrap().mask_transform,
                        Mapping2::Affine(transform)
                    );
                    assert_ne!((mask.width(), mask.height()), (doc.width, doc.height));
                    doc.validate().unwrap();
                    graph.record(&doc, "Canvas geometry", false).unwrap();
                    for with_history in [false, true] {
                        write_full(&doc, with_history.then_some(&graph), &path).unwrap();
                        let opened = read_full(&path).unwrap();
                        assert!(opened.history_error.is_none());
                        assert_document_space_mask_roundtrip(&opened.doc, &doc);
                        if with_history {
                            let restored = opened.graph.unwrap();
                            assert_eq!(restored.len(), graph.len());
                            let mut previous = None;
                            for (actual, expected) in restored.commits().zip(graph.commits()) {
                                assert_document_space_mask_roundtrip(&actual.doc, &expected.doc);
                                let mask = actual.doc.node(1).unwrap().mask.as_ref().unwrap();
                                if let Some(prior) = &previous {
                                    assert!(
                                        Arc::ptr_eq(prior, mask),
                                        "history shares raw mask pixels"
                                    );
                                }
                                previous = Some(mask.clone());
                            }
                        }
                    }
                }
            }
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn independent_document_space_mask_extents_require_v10_in_live_and_history() {
        let original = document_space_mask_document();
        let mut cropped = original.clone();
        Command::Crop {
            rect: emulsion_raster::IRect::new(2, 1, 6, 4),
            rotation: 0.0,
        }
        .apply(&mut cropped)
        .unwrap();
        assert_eq!(cropped.node(1).unwrap().mask_properties, Default::default());
        assert!(!requires_mask_v10(&original));
        assert!(requires_mask_v10(&cropped));
        let mut graph = Graph::new(cropped.clone(), "Independent mask extent");
        graph.record(&original, "Restore canvas", false).unwrap();
        let path = tmp("independent-mask-extents-version.ora");
        for (doc, history, version) in [
            (&original, None, 9),
            (&cropped, None, 10),
            (&original, Some(&graph), 10),
        ] {
            write_full(doc, history, &path).unwrap();
            let mut zip = ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
            let manifest: serde_json::Value = serde_json::from_slice(
                &read_entry(&mut zip, MANIFEST, MAX_NATIVE_MANIFEST_BYTES).unwrap(),
            )
            .unwrap();
            assert_eq!(manifest["version"], version);
            assert!(
                manifest["nodes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|node| node["id"] == 1)
                    .unwrap()
                    .get("mask_properties")
                    .is_none()
            );
            if history.is_some() {
                let history: serde_json::Value = serde_json::from_slice(
                    &read_entry(&mut zip, crate::history::GRAPH, MAX_NATIVE_MANIFEST_BYTES)
                        .unwrap(),
                )
                .unwrap();
                assert_eq!(history["version"], 10);
            }
            drop(zip);
            let opened = read_full(&path).unwrap();
            assert!(opened.history_error.is_none());
            assert_document_space_mask_roundtrip(&opened.doc, doc);
        }
        // Legacy versions never permitted this dimensional mismatch. Keep
        // rejecting malformed old archives rather than silently reinterpreting them.
        write(&cropped, &path).unwrap();
        rewrite_archive(&path, |name, bytes| {
            if name != MANIFEST {
                return Some(bytes);
            }
            let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            value["version"] = 9.into();
            Some(serde_json::to_vec(&value).unwrap())
        });
        assert!(read_full(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }

    fn cropped_convertible_mask_document(kind: &str, enabled: bool) -> Document {
        let mut doc = Document::new(48, 32);
        doc.source_depth = 16;
        let mut node = match kind {
            "fill" => Node::new(
                1,
                "Fill",
                NodeKind::Fill {
                    rgba: [255, 0, 0, 255],
                },
            ),
            "text" => Node::text(
                1,
                "Text",
                emulsion_core::text::TextSpec {
                    text: "Mask".into(),
                    size: 16.0,
                    x: 10.0,
                    y: 7.0,
                    color: [255, 0, 0, 255],
                    ..Default::default()
                },
                48,
                32,
            ),
            "path" => Node::path(
                1,
                "Path",
                Arc::new(emulsion_raster::vector::Path::from_svg("M 6 5 H 43 V 28 H 6 Z").unwrap()),
                emulsion_raster::vector::PathStyle {
                    fill: Some([255, 0, 0, 255]),
                    stroke: None,
                    ..Default::default()
                },
                48,
                32,
            ),
            _ => unreachable!(),
        };
        node.mask = Some(Arc::new(Mask::from_fn(48, 32, 0, |x, y| {
            if (9..39).contains(&x) && (6..26).contains(&y) {
                255
            } else {
                0
            }
        })));
        node.mask_properties = emulsion_core::MaskProperties {
            density: 0.45,
            feather: 2.0,
        };
        node.mask_transform = Mapping2::Affine(glam::DAffine2::from_cols_array(&[
            1.0, 0.0, 0.1, 1.0, 1.25, -0.5,
        ]));
        node.mask_enabled = enabled;
        node.mask_linked = false;
        doc.nodes.push(node);
        doc.next_id = 2;
        Command::Crop {
            rect: emulsion_raster::IRect::new(8, 4, 28, 20),
            rotation: 0.0,
        }
        .apply(&mut doc)
        .unwrap();
        doc
    }

    fn assert_independent_mask_roundtrip(actual: &Document, expected: &Document) {
        let actual_node = &actual.nodes[0];
        let expected_node = &expected.nodes[0];
        assert_eq!(
            std::mem::discriminant(&actual_node.kind),
            std::mem::discriminant(&expected_node.kind)
        );
        let actual_mask = actual_node.mask.as_ref().unwrap();
        let expected_mask = expected_node.mask.as_ref().unwrap();
        assert_eq!(
            (actual_mask.width(), actual_mask.height()),
            (expected_mask.width(), expected_mask.height())
        );
        assert_eq!(actual_mask.to_gray8(), expected_mask.to_gray8());
        assert_eq!(actual_mask.fill(), expected_mask.fill());
        assert_eq!(actual_node.mask_transform, expected_node.mask_transform);
        assert_eq!(actual_node.mask_properties, expected_node.mask_properties);
        assert_eq!(actual_node.mask_enabled, expected_node.mask_enabled);
        assert_eq!(actual_node.mask_linked, expected_node.mask_linked);
        assert_eq!(
            emulsion_core::transform::mask_to_document(actual_node)
                .unwrap()
                .require_affine("legacy fixture")
                .unwrap()
                .to_cols_array(),
            emulsion_core::transform::mask_to_document(expected_node)
                .unwrap()
                .require_affine("legacy fixture")
                .unwrap()
                .to_cols_array()
        );
        assert_eq!(
            flatten(&actual.composite_tree(), 0).to_srgba8(),
            flatten(&expected.composite_tree(), 0).to_srgba8()
        );
    }

    #[test]
    fn cropped_fill_text_path_mask_properties_convert_and_roundtrip_native_history() {
        let path = tmp("converted-intrinsic-mask.ora");
        for kind in ["fill", "text", "path"] {
            for smart in [false, true] {
                for enabled in [true, false] {
                    let mut doc = cropped_convertible_mask_document(kind, enabled);
                    let raw = doc.nodes[0].mask.as_ref().unwrap().clone();
                    let properties = doc.nodes[0].mask_properties;
                    let world = emulsion_core::transform::mask_to_document(&doc.nodes[0])
                        .unwrap()
                        .require_affine("legacy fixture")
                        .unwrap();
                    let appearance = flatten(&doc.composite_tree(), 0).to_srgba8();
                    let mut graph = Graph::new(doc.clone(), "Cropped editable source");
                    if !smart || kind == "fill" {
                        Command::Rasterize { id: 1 }.apply(&mut doc).unwrap();
                    }
                    if smart {
                        Command::ConvertToSmart { id: 1 }.apply(&mut doc).unwrap();
                    }
                    assert!(Arc::ptr_eq(&raw, doc.nodes[0].mask.as_ref().unwrap()));
                    assert_eq!(doc.nodes[0].mask_properties, properties);
                    let converted_world = emulsion_core::transform::mask_to_document(&doc.nodes[0])
                        .unwrap()
                        .require_affine("legacy fixture")
                        .unwrap();
                    for (actual, expected) in converted_world
                        .to_cols_array()
                        .into_iter()
                        .zip(world.to_cols_array())
                    {
                        assert!(
                            (actual - expected).abs() < 1e-9,
                            "world mask affine retained"
                        );
                    }
                    assert_eq!(
                        flatten(&doc.composite_tree(), 0).to_srgba8(),
                        appearance,
                        "conversion appearance: {kind}, smart={smart}, enabled={enabled}"
                    );
                    doc.validate().unwrap();
                    graph.record(&doc, "Converted source", false).unwrap();
                    for with_history in [false, true] {
                        write_full(&doc, with_history.then_some(&graph), &path).unwrap();
                        let opened = read_full(&path).unwrap();
                        assert!(opened.history_error.is_none());
                        assert_independent_mask_roundtrip(&opened.doc, &doc);
                        if with_history {
                            let restored = opened.graph.unwrap();
                            assert_eq!(restored.len(), graph.len());
                            let mut previous = None;
                            for (actual, expected) in restored.commits().zip(graph.commits()) {
                                assert_independent_mask_roundtrip(&actual.doc, &expected.doc);
                                let mask = actual.doc.nodes[0].mask.as_ref().unwrap();
                                if let Some(prior) = &previous {
                                    assert!(Arc::ptr_eq(prior, mask));
                                }
                                previous = Some(mask.clone());
                            }
                        }
                    }
                }
            }
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn independent_raster_smart_mask_extents_require_v10_without_nondefault_properties() {
        let path = tmp("pixel-mask-intrinsic-extents-version.ora");
        for smart in [false, true] {
            let mut doc = cropped_convertible_mask_document("fill", true);
            Command::Rasterize { id: 1 }.apply(&mut doc).unwrap();
            if smart {
                Command::ConvertToSmart { id: 1 }.apply(&mut doc).unwrap();
            }
            doc.nodes[0].mask_properties = Default::default();
            assert!(
                requires_mask_v10(&doc),
                "independent extent itself requires v10"
            );
            let graph = Graph::new(doc.clone(), "Independent pixel mask");
            write_full(&doc, Some(&graph), &path).unwrap();
            let mut zip = ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
            for name in [MANIFEST, crate::history::GRAPH] {
                let value: serde_json::Value = serde_json::from_slice(
                    &read_entry(&mut zip, name, MAX_NATIVE_MANIFEST_BYTES).unwrap(),
                )
                .unwrap();
                assert_eq!(value["version"], 10);
            }
            drop(zip);
            let opened = read_full(&path).unwrap();
            assert!(opened.history_error.is_none());
            assert_independent_mask_roundtrip(&opened.doc, &doc);
            rewrite_archive(&path, |name, bytes| {
                if name != MANIFEST {
                    return Some(bytes);
                }
                let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                value["version"] = 9.into();
                Some(serde_json::to_vec(&value).unwrap())
            });
            assert!(
                read_full(&path).is_err(),
                "v9 keeps source-sized pixel-mask invariants"
            );
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn smart_rasterize_expanded_cache_mask_properties_roundtrip_raw_plane_and_world_affine() {
        let path = tmp("smart-rasterize-independent-mask.ora");
        for enabled in [true, false] {
            let mut doc = cropped_convertible_mask_document("path", enabled);
            Command::ConvertToSmart { id: 1 }.apply(&mut doc).unwrap();
            Command::SetFilters {
                id: 1,
                filters: vec![emulsion_filters::Filter::GaussianBlur { radius: 1.5 }],
            }
            .apply(&mut doc)
            .unwrap();
            let NodeKind::Smart {
                offset,
                source,
                cache,
                ..
            } = &doc.nodes[0].kind
            else {
                panic!("smart");
            };
            assert_ne!(*offset, (0, 0));
            assert_ne!(
                (source.width(), source.height()),
                (cache.width(), cache.height())
            );
            let raw = doc.nodes[0].mask.as_ref().unwrap().clone();
            let properties = doc.nodes[0].mask_properties;
            let world = emulsion_core::transform::mask_to_document(&doc.nodes[0])
                .unwrap()
                .require_affine("legacy fixture")
                .unwrap();
            let appearance = flatten(&doc.composite_tree(), 0).to_srgba8();
            let mut graph = Graph::new(doc.clone(), "Expanded Smart cache");
            Command::Rasterize { id: 1 }.apply(&mut doc).unwrap();
            assert!(Arc::ptr_eq(&raw, doc.nodes[0].mask.as_ref().unwrap()));
            assert_eq!(doc.nodes[0].mask_properties, properties);
            for (actual, expected) in emulsion_core::transform::mask_to_document(&doc.nodes[0])
                .unwrap()
                .require_affine("legacy fixture")
                .unwrap()
                .to_cols_array()
                .into_iter()
                .zip(world.to_cols_array())
            {
                assert!((actual - expected).abs() < 1e-9);
            }
            assert_eq!(flatten(&doc.composite_tree(), 0).to_srgba8(), appearance);
            graph.record(&doc, "Rasterized Smart cache", false).unwrap();
            for with_history in [false, true] {
                write_full(&doc, with_history.then_some(&graph), &path).unwrap();
                let opened = read_full(&path).unwrap();
                assert!(opened.history_error.is_none());
                assert_independent_mask_roundtrip(&opened.doc, &doc);
                if with_history {
                    let restored = opened.graph.unwrap();
                    for (actual, expected) in restored.commits().zip(graph.commits()) {
                        assert_independent_mask_roundtrip(&actual.doc, &expected.doc);
                    }
                }
            }
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn native_layer_links_and_independent_mask_transform_roundtrip() {
        let mut doc = Document::new(12, 10);
        for id in 1..=2 {
            let mut node = Node::raster(
                id,
                "Linked",
                Arc::new(Raster::solid(4, 4, [1.; 4])),
                Placement::default(),
            );
            node.link_group = Some(19);
            node.mask = Some(Arc::new(Mask::from_fn(4, 4, 0, |x, _| {
                if x < 2 { 255 } else { 0 }
            })));
            node.mask_linked = false;
            node.mask_transform =
                Mapping2::Affine(glam::DAffine2::from_cols_array(&[1., 0., 0.25, 1., 2., 1.]));
            doc.nodes.push(node);
        }
        doc.next_id = 3;
        let editor = emulsion_core::Editor::new(doc.clone(), None);
        let path = tmp("layer-links-mask-affine.ora");
        write_full(&doc, Some(&editor.graph), &path).unwrap();
        let reopened = read_full(&path).unwrap();
        assert!(reopened.history_error.is_none());
        assert_eq!(reopened.graph.as_ref().unwrap().len(), editor.graph.len());
        assert_eq!(reopened.doc.nodes.len(), doc.nodes.len());
        for (actual, expected) in reopened.doc.nodes.iter().zip(&doc.nodes) {
            assert_eq!(actual.id, expected.id);
            assert_eq!(actual.name, expected.name);
            assert_eq!(actual.link_group, expected.link_group);
            assert_eq!(actual.mask_linked, expected.mask_linked);
            assert_eq!(actual.mask_enabled, expected.mask_enabled);
            assert_eq!(actual.mask_transform, expected.mask_transform);
            let (
                NodeKind::Raster {
                    raster: a,
                    placement: ap,
                },
                NodeKind::Raster {
                    raster: b,
                    placement: bp,
                },
            ) = (&actual.kind, &expected.kind)
            else {
                panic!("raster layers");
            };
            assert_eq!(ap, bp);
            for y in 0..4 {
                for x in 0..4 {
                    assert_eq!(a.get(x, y), b.get(x, y));
                    assert_eq!(
                        actual.mask.as_ref().unwrap().get(x, y),
                        expected.mask.as_ref().unwrap().get(x, y)
                    );
                }
            }
        }
        // Plain native loading also retains metadata without the history sidecar.
        write_full(&doc, None, &path).unwrap();
        let plain = read_full(&path).unwrap();
        assert_eq!(
            plain.doc.nodes[0].mask_transform,
            doc.nodes[0].mask_transform
        );
        assert_eq!(plain.doc.nodes[0].link_group, Some(19));
        assert!(!plain.doc.nodes[0].mask_linked);
        assert_eq!(
            flatten(&plain.doc.composite_tree(), 0).to_srgba8(),
            flatten(&doc.composite_tree(), 0).to_srgba8()
        );
        rewrite_archive(&path, |name, bytes| {
            if name != MANIFEST {
                return Some(bytes);
            }
            let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            for node in value["nodes"].as_array_mut().unwrap() {
                let node = node.as_object_mut().unwrap();
                node.remove("link_group");
                node.remove("mask_linked");
                node.remove("mask_transform");
            }
            Some(serde_json::to_vec(&value).unwrap())
        });
        let legacy = read_full(&path).unwrap();
        assert!(legacy.doc.nodes.iter().all(|n| n.link_group.is_none()
            && n.mask_linked
            && n.mask_transform == Mapping2::IDENTITY));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn extended_effect_assets_and_global_light_persist_without_repeating_history_assets() {
        use emulsion_core::style_options::*;
        let mut editor = emulsion_core::Editor::new(Document::new(8, 8), None);
        let id = editor
            .execute(Command::AddNode {
                node: Box::new(Node::raster(
                    0,
                    "Pattern",
                    Arc::new(Raster::solid(4, 4, [1.; 4])),
                    Placement::default(),
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let image = Arc::new(PatternImage {
            width: 2,
            height: 1,
            pixels: vec![1, 2, 3, 4, 201, 202, 203, 204],
        });
        let mut option = StyleOptions {
            id: 91,
            enabled: false,
            blend: BlendMode::Multiply,
            ..Default::default()
        };
        option.pattern.image = Some(image.clone());
        option.gradient.stops = vec![
            GradientStop {
                position: 0.25,
                color: [2, 3, 4, 5],
            },
            GradientStop {
                position: 0.8,
                color: [91, 92, 93, 94],
            },
        ];
        option.contour = vec![ContourPoint { x: 0., y: 1. }, ContourPoint { x: 1., y: 0. }];
        let styles = vec![
            emulsion_core::styles::LayerStyle::catalogue()
                .pop()
                .unwrap(),
        ];
        editor
            .execute(Command::SetLayerEffects {
                id,
                styles,
                options: vec![option.clone()],
            })
            .unwrap();
        let light = GlobalLight {
            angle: 51.,
            altitude: 42.,
        };
        editor.execute(Command::SetGlobalLight { light }).unwrap();
        editor
            .execute(Command::SetEffectsEnabled { id, enabled: false })
            .unwrap();
        editor.commit("Pattern version", false).unwrap();
        editor
            .execute(Command::Rename {
                id,
                name: "Working copy".into(),
            })
            .unwrap();
        let path = tmp("extended-effect-assets.ora");
        write_full(&editor.doc, Some(&editor.graph), &path).unwrap();
        let mut archive = ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let graph_json: serde_json::Value = serde_json::from_slice(
            &read_entry(
                &mut archive,
                "history/graph.json",
                MAX_NATIVE_MANIFEST_BYTES,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(graph_json["patterns"].as_array().unwrap().len(), 1);
        drop(archive);
        let opened = read_full(&path).unwrap();
        assert!(opened.history_error.is_none());
        assert_eq!(opened.doc.global_light, light);
        assert!(!opened.doc.node(id).unwrap().effects_enabled);
        assert_eq!(
            opened.doc.node(id).unwrap().style_options,
            vec![option.clone()]
        );
        let graph = opened.graph.unwrap();
        let checkpoint = &graph.commit(graph.head_branch().tip).unwrap().doc;
        let a = opened.doc.node(id).unwrap().style_options[0]
            .pattern
            .image
            .as_ref()
            .unwrap();
        let b = checkpoint.node(id).unwrap().style_options[0]
            .pattern
            .image
            .as_ref()
            .unwrap();
        assert!(Arc::ptr_eq(a, b), "history shares imported image data");
        assert_eq!(a.pixels, image.pixels);
        write_full(&editor.doc, None, &path).unwrap();
        assert_eq!(
            read_full(&path)
                .unwrap()
                .doc
                .node(id)
                .unwrap()
                .style_options,
            vec![option]
        );
        rewrite_archive(&path, |name, bytes| {
            if name != MANIFEST {
                return Some(bytes);
            }
            let mut json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            json.as_object_mut().unwrap().remove("global_light");
            for node in json["nodes"].as_array_mut().unwrap() {
                node.as_object_mut().unwrap().remove("style_options");
                node.as_object_mut().unwrap().remove("pattern_refs");
                node.as_object_mut().unwrap().remove("effects_enabled");
            }
            Some(serde_json::to_vec(&json).unwrap())
        });
        let legacy = read_full(&path).unwrap();
        assert_eq!(legacy.doc.global_light, GlobalLight::default());
        assert!(legacy.doc.node(id).unwrap().style_options.is_empty());
        assert!(legacy.doc.node(id).unwrap().effects_enabled);
        assert_eq!(legacy.doc.node(id).unwrap().styles.len(), 1);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn native_patterns_use_one_binary_asset_and_restore_shared_images() {
        use emulsion_core::style_options::{PatternImage, StyleOptions};
        let image = Arc::new(PatternImage {
            width: 2,
            height: 1,
            pixels: vec![0, 1, 2, 3, 251, 252, 253, 254],
        });
        let mut doc = Document::new(4, 4);
        for id in 1..=3 {
            let mut node = Node::raster(
                id,
                "Pattern",
                Arc::new(Raster::solid(4, 4, [1.; 4])),
                Placement::default(),
            );
            node.styles = vec![
                emulsion_core::styles::LayerStyle::catalogue()
                    .pop()
                    .unwrap(),
            ];
            let mut option = StyleOptions {
                id: 1,
                ..Default::default()
            };
            option.pattern.image = Some(if id == 3 {
                Arc::new((*image).clone())
            } else {
                image.clone()
            });
            node.style_options = vec![option];
            doc.nodes.push(node);
        }
        doc.next_id = 4;
        let path = tmp("binary-pattern-pool.ora");
        write_full(&doc, None, &path).unwrap();
        let mut zip = ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let json: serde_json::Value = serde_json::from_slice(
            &read_entry(&mut zip, MANIFEST, MAX_NATIVE_MANIFEST_BYTES).unwrap(),
        )
        .unwrap();
        assert_eq!(json["patterns"].as_array().unwrap().len(), 1);
        for node in json["nodes"].as_array().unwrap() {
            assert!(node["style_options"][0]["pattern"]["image"].is_null());
            assert_eq!(node["pattern_refs"][0], 0);
        }
        assert_eq!(
            read_entry(&mut zip, "emulsion/patterns/0.rgba", 8).unwrap(),
            image.pixels
        );
        drop(zip);
        let reopened = read_full(&path).unwrap();
        let a = reopened.doc.nodes[0].style_options[0]
            .pattern
            .image
            .as_ref()
            .unwrap();
        for node in &reopened.doc.nodes {
            let b = node.style_options[0].pattern.image.as_ref().unwrap();
            assert!(Arc::ptr_eq(a, b));
            assert_eq!(b.pixels, image.pixels);
        }
        assert_eq!(
            flatten(&reopened.doc.composite_tree(), 0).to_srgba8(),
            flatten(&doc.composite_tree(), 0).to_srgba8()
        );
        rewrite_archive(&path, |name, bytes| {
            if name != MANIFEST {
                return Some(bytes);
            }
            let mut json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            json["patterns"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({
                    "src": "unused-missing-asset.rgba", "width": u32::MAX, "height": u32::MAX
                }));
            Some(serde_json::to_vec(&json).unwrap())
        });
        assert_eq!(
            read_full(&path).unwrap().doc.nodes[0].style_options[0]
                .pattern
                .image
                .as_ref()
                .unwrap()
                .pixels,
            image.pixels,
            "unused assets are not loaded or size-validated"
        );
        // Version-three files embedded pattern bytes directly in each option.
        rewrite_archive(&path, |name, bytes| {
            if name.starts_with("emulsion/patterns/") {
                return None;
            }
            if name != MANIFEST {
                return Some(bytes);
            }
            let mut json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            json["version"] = 3.into();
            json.as_object_mut().unwrap().remove("patterns");
            for node in json["nodes"].as_array_mut().unwrap() {
                node.as_object_mut().unwrap().remove("pattern_refs");
                node["style_options"][0]["pattern"]["image"] =
                    serde_json::to_value(&*image).unwrap();
            }
            Some(serde_json::to_vec(&json).unwrap())
        });
        assert_eq!(
            read_full(&path).unwrap().doc.nodes[0].style_options[0]
                .pattern
                .image
                .as_ref()
                .unwrap()
                .pixels,
            image.pixels
        );
        write_full(&doc, None, &path).unwrap();
        rewrite_archive(&path, |name, bytes| {
            if name == "emulsion/patterns/0.rgba" {
                Some(vec![0])
            } else {
                Some(bytes)
            }
        });
        assert!(
            read_full(&path).is_err(),
            "truncated pattern data is rejected"
        );
        write_full(&doc, None, &path).unwrap();
        rewrite_archive(&path, |name, bytes| {
            if name != MANIFEST {
                return Some(bytes);
            }
            let mut json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let template = json["nodes"][0].clone();
            let mut nodes = Vec::new();
            let mut patterns = Vec::new();
            for index in 0..33 {
                let mut node = template.clone();
                node["id"] = (index + 1).into();
                node["pattern_refs"] = serde_json::json!([index]);
                nodes.push(node);
                patterns.push(serde_json::json!({"src": format!("not-decoded-{index}.rgba"), "width": 2048, "height": 2048}));
            }
            json["nodes"] = nodes.into();
            json["patterns"] = patterns.into();
            Some(serde_json::to_vec(&json).unwrap())
        });
        let error = match read_full(&path) {
            Ok(_) => panic!("oversized pattern pool accepted"),
            Err(error) => error.to_string(),
        };
        assert!(
            error.contains("asset budget"),
            "aggregate size is rejected before any absent asset is read: {error}"
        );
        let _ = std::fs::remove_file(path);
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
    fn vector_stroke_layers_round_trip_with_a_flat_preview() {
        use emulsion_raster::strokes::{Stroke, StrokePoint, StrokeSet};
        let mut doc = Document::new(40, 20);
        let mut pencil = Stroke::new([20, 40, 60, 255], 3.);
        pencil.points = vec![
            StrokePoint::new(4., 10.),
            StrokePoint {
                width: 0.5,
                opacity: 0.7,
                ..StrokePoint::new(36., 10.)
            },
        ];
        let strokes = Arc::new(StrokeSet {
            strokes: vec![pencil],
            fills: Vec::new(),
        });
        Command::AddNode {
            node: Box::new(Node::strokes(0, "Pencil", strokes.clone(), 40, 20)),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
        let saved = tmp("vector-strokes.ora");
        write(&doc, &saved).unwrap();
        let loaded = read_full(&saved).unwrap();
        let NodeKind::Strokes {
            strokes: back,
            cache,
        } = &loaded.doc.nodes[0].kind
        else {
            panic!("strokes must stay editable")
        };
        assert_eq!(**back, *strokes);
        assert!(cache.pixels().get(10, 10)[3] > 0);
        // Readers that only know layers see the drawing as a PNG layer.
        let mut zip = ZipArchive::new(std::fs::File::open(&saved).unwrap()).unwrap();
        assert!(
            zip.by_name(&format!("data/node-{}.png", doc.nodes[0].id))
                .is_ok()
        );
        std::fs::remove_file(saved).unwrap();
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
        d.colors = vec![[200, 30, 10], [0, 0, 0]];
        d.drawing_guides
            .set_primary(emulsion_core::drawing_guides::GuideKind::Curvilinear {
                center: (10.0, 20.0),
                radius: 30.0,
                five: true,
            });
        d.drawing_guides.save_set("Fish-eye").unwrap();
        d.drawing_guides.ruler = Some(emulsion_core::drawing_guides::Ruler::centered(64.0, 48.0));
        let p = tmp("roundtrip.ora");
        write(&d, &p).unwrap();
        let back = read(&p).unwrap();
        assert_eq!(back.guides, d.guides);
        assert_eq!(back.colors, d.colors);
        assert_eq!(back.drawing_guides, d.drawing_guides);
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
    fn shape_paints_and_strokes_survive_undo_and_native_history() {
        use emulsion_core::Editor;
        use emulsion_raster::vector::{
            Path, PathPaint, PathStyle, PatternKind, StrokeAlignment, StrokeCap, StrokeJoin,
        };
        let legacy: PathStyle = serde_json::from_value(serde_json::json!({
            "stroke": [0, 0, 0, 255], "width": 3.0, "fill": [255, 0, 0, 255]
        }))
        .unwrap();
        assert_eq!(legacy.fill_paint, PathPaint::Solid);
        assert_eq!(legacy.alignment, StrokeAlignment::Center);
        assert_eq!(legacy.dash_count, 0);
        let path = Arc::new(Path::from_svg("M 8 8 L 48 8 L 48 40 L 8 40 Z").unwrap());
        let mut doc = Document::new(64, 48);
        doc.nodes
            .push(Node::path(1, "Shape", path.clone(), legacy, 64, 48));
        doc.normalize();
        let mut editor = Editor::new(doc, None);
        let styles = [
            PathStyle {
                fill_paint: PathPaint::LinearGradient {
                    end: [0, 100, 255, 128],
                    angle: 37.0,
                },
                stroke_paint: PathPaint::RadialGradient {
                    end: [255, 255, 255, 255],
                },
                alignment: StrokeAlignment::Inside,
                cap: StrokeCap::Square,
                join: StrokeJoin::Bevel,
                miter_limit: 7.0,
                dash: [4.0, 2.0, 1.0, 2.0, 0.0, 0.0],
                dash_count: 4,
                dash_offset: 1.5,
                ..legacy
            },
            PathStyle {
                fill_paint: PathPaint::Pattern {
                    kind: PatternKind::Dots,
                    secondary: [20, 80, 50, 255],
                    size: 8.0,
                },
                stroke_paint: PathPaint::Pattern {
                    kind: PatternKind::Stripes,
                    secondary: [200, 80, 50, 255],
                    size: 6.0,
                },
                alignment: StrokeAlignment::Outside,
                ..legacy
            },
        ];
        for (index, style) in styles.into_iter().enumerate() {
            editor
                .execute(Command::SetPath {
                    id: 1,
                    path: path.clone(),
                    style,
                })
                .unwrap();
            let expected = editor.doc.clone();
            assert!(editor.undo());
            assert!(editor.redo());
            assert_eq!(editor.doc, expected);
            editor.commit(format!("Shape style {index}"), false);
        }
        let file = tmp("shape-style-history.ora");
        write_full(&editor.doc, Some(&editor.graph), &file).unwrap();
        let reopened = read_full(&file).unwrap();
        assert!(reopened.history_error.is_none());
        assert_eq!(reopened.doc, editor.doc);
        let graph = reopened.graph.unwrap();
        for commit in editor.graph.commits() {
            assert_eq!(graph.commit(commit.id).unwrap().doc, commit.doc);
        }
        assert_eq!(
            flatten(&reopened.doc.composite_tree(), 0).to_srgba8(),
            flatten(&editor.doc.composite_tree(), 0).to_srgba8()
        );
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
                    ..Default::default()
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

    /// A foreign ORA archive holding `stack` and one 2×2 PNG at `a.png`.
    fn foreign_ora(stack: &str) -> ZipArchive<std::io::Cursor<Vec<u8>>> {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let mut out = ZipWriter::new(std::io::Cursor::new(Vec::new()));
        out.start_file("stack.xml", SimpleFileOptions::default())
            .unwrap();
        out.write_all(stack.as_bytes()).unwrap();
        out.start_file("a.png", SimpleFileOptions::default())
            .unwrap();
        out.write_all(&png).unwrap();
        ZipArchive::new(out.finish().unwrap()).unwrap()
    }

    #[test]
    fn foreign_stack_limits_are_enforced_while_parsing() {
        let deep = format!("<image w=\"2\" h=\"2\">{}", "<stack>".repeat(100_000));
        assert!(matches!(
            read_stack(&mut foreign_ora(&deep)),
            Err(IoError::Invalid(emulsion_core::DocumentError::TooDeep(_)))
        ));
        let wide = format!(
            "<image w=\"2\" h=\"2\"><stack>{}</stack></image>",
            "<layer src=\"a.png\"/>".repeat(emulsion_core::document::MAX_NODES + 1)
        );
        assert!(matches!(
            read_stack(&mut foreign_ora(&wide)),
            Err(IoError::Invalid(
                emulsion_core::DocumentError::TooManyNodes(_)
            ))
        ));
        // Within the limits, layers naming one src share its pixels.
        let shared = format!(
            "<image w=\"2\" h=\"2\"><stack>{}<stack>{}</stack></stack></image>",
            "<layer src=\"a.png\"/>".repeat(3),
            "<layer src=\"a.png\"/>".repeat(2)
        );
        let doc = read_stack(&mut foreign_ora(&shared)).unwrap();
        let rasters: Vec<_> = doc
            .nodes
            .iter()
            .filter_map(|n| match &n.kind {
                NodeKind::Raster { raster, .. } => Some(raster.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(rasters.len(), 5);
        assert!(rasters.iter().all(|r| Arc::ptr_eq(r, &rasters[0])));
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
    fn cropped_vector_previews_keep_standard_ora_placement() {
        use emulsion_core::text::TextSpec;
        use emulsion_raster::vector::{Path, PathStyle};

        let mut doc = Document::new(160, 120);
        doc.source_depth = 16;
        for node in [
            Node::path(
                0,
                "Shape",
                Arc::new(Path::from_svg("M 42 31 L 67 31 L 67 53 L 42 53 Z").unwrap()),
                PathStyle {
                    fill: Some([30, 90, 180, 255]),
                    stroke: None,
                    ..Default::default()
                },
                160,
                120,
            ),
            Node::text(
                0,
                "Label",
                TextSpec {
                    text: "Art".into(),
                    x: 85.0,
                    y: 65.0,
                    size: 18.0,
                    ..Default::default()
                },
                160,
                120,
            ),
            Node::text(0, "Empty", TextSpec::default(), 160, 120),
        ] {
            Command::AddNode {
                node: Box::new(node),
                slot: Slot::TOP,
            }
            .apply(&mut doc)
            .unwrap();
        }
        let mut paths = crate::path_data::PathPool::default();
        let encoded = encode(
            &doc,
            &mut paths,
            &mut crate::original_image_data::OriginalImagePool::default(),
        )
        .unwrap();
        for node in &doc.nodes {
            let raster = match &node.kind {
                NodeKind::Path { cache, .. } => cache.pixels(),
                NodeKind::Text { cache, .. } => cache.pixels(),
                _ => unreachable!(),
            };
            let bounds = raster.coverage_bounds();
            let (name, x, y) = &encoded.ora_layers[&node.id];
            let bytes = &encoded.entries.iter().find(|(p, _)| p == name).unwrap().1;
            let image = image::load_from_memory(bytes).unwrap();
            if bounds.is_empty() {
                assert_eq!((image.width(), image.height(), *x, *y), (1, 1, 0, 0));
            } else {
                assert_eq!(
                    (image.width(), image.height()),
                    (bounds.w as u32, bounds.h as u32)
                );
                assert_eq!((*x, *y), (i64::from(bounds.x), i64::from(bounds.y)));
                assert!(image.width() < doc.width);
                assert!(image.height() < doc.height);
            }
        }
        let path = tmp("cropped-vector-previews.ora");
        write(&doc, &path).unwrap();
        rewrite_archive(&path, |name, bytes| {
            (name != MANIFEST && !name.starts_with("emulsion/")).then_some(bytes)
        });
        let standard = read(&path).unwrap();
        assert_eq!(standard.nodes.len(), doc.nodes.len());
        let expected = flatten(&doc.composite_tree(), 0).to_srgba8();
        let actual = flatten(&standard.composite_tree(), 0).to_srgba8();
        assert_eq!(expected.len(), actual.len());
        assert!(
            expected
                .iter()
                .zip(actual)
                .all(|(&a, b)| a.abs_diff(b) <= 1)
        );
    }

    #[test]
    fn parallel_archive_entries_preserve_order_bytes_and_compression() {
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .compression_level(Some(1));
        // Cross the worker batch boundary and exercise the single-entry tail.
        let data: Vec<_> = (0..65)
            .map(|i| (format!("history/tile-{i}.bin"), vec![i as u8; 4096]))
            .collect();
        let entries: Vec<_> = data
            .iter()
            .enumerate()
            .map(|(i, (name, bytes))| {
                (
                    name.as_str(),
                    bytes.as_slice(),
                    if i % 2 == 0 { stored } else { deflated },
                )
            })
            .collect();
        let mut writer = ZipWriter::new(std::io::Cursor::new(Vec::new()));
        writer.start_file("mimetype", stored).unwrap();
        writer.write_all(b"image/openraster").unwrap();
        write_entries(&mut writer, &entries).unwrap();
        let mut archive = ZipArchive::new(writer.finish().unwrap()).unwrap();
        assert_eq!(archive.len(), 66);
        for (i, (name, expected)) in data.iter().enumerate() {
            let mut entry = archive.by_index(i + 1).unwrap();
            assert_eq!(entry.name(), name);
            assert_eq!(
                entry.compression(),
                if i % 2 == 0 {
                    CompressionMethod::Stored
                } else {
                    CompressionMethod::Deflated
                }
            );
            let mut actual = Vec::new();
            entry.read_to_end(&mut actual).unwrap();
            assert_eq!(&actual, expected);
        }
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
        // External edits invalidate exact working restoration. Legacy recovery
        // keeps the live artwork and reports the unrepresented selection.
        let expected_history_error = IoError::NativePreservation {
            code: crate::NativeFailureCode::WorkingNotRepresented,
            location: "history.working".into(),
            detail: "document.selection.presence".into(),
        }
        .to_string();
        assert_eq!(
            opened.history_error.as_deref(),
            Some(expected_history_error.as_str())
        );
        assert_eq!(opened.doc.nodes[1].name, "Externally renamed");
        assert_eq!(opened.doc.source_depth, 8);
        assert!(opened.doc.selection.is_none());
        let NodeKind::Text { spec, .. } = &opened.doc.node(2).unwrap().kind else {
            panic!("editable text");
        };
        assert_eq!(**spec, text);
        let NodeKind::Raster { raster, .. } = &opened.doc.node(1).unwrap().kind else {
            panic!("raster");
        };
        assert_eq!(raster.to_srgba8(), changed.to_srgba8());
        let graph = opened.graph.unwrap();
        assert_eq!(graph.len(), version_count);
        let tip = &graph.commit(graph.head_branch().tip).unwrap().doc;
        let NodeKind::Raster {
            raster: checkpoint, ..
        } = &tip.node(1).unwrap().kind
        else {
            panic!("checkpoint raster");
        };
        for y in 0..64 {
            for x in 0..520 {
                assert_eq!(checkpoint.get(x, y), original.get(x, y));
            }
        }
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
