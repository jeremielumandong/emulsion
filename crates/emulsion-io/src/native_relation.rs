//! Compare the state actually retained by the two native representations.
//!
//! Live PNGs are a directional, lossy writer projection of history planes.
//! Their *straight* samples must be checked before import discards hidden RGB
//! or rounds low-alpha values. This is neither provenance authentication nor
//! verification that a saved filter cache was correctly rendered.

use crate::{IoError, Result};
use emulsion_core::{Document, Node, NodeId, NodeKind};
use emulsion_raster::{Mask, Raster, color, image::Pix, image::Plane};
use std::{
    collections::{HashMap, HashSet},
    io::{Cursor, Read, Seek},
    sync::Arc,
};
use zip::ZipArchive;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LiveRelation {
    Consistent,
    Mismatch(String),
    Unverified(String),
}

type Compared = std::result::Result<(), LiveRelation>;

fn same<T: PartialEq + ?Sized>(a: &T, b: &T, location: &str) -> Compared {
    if a == b {
        Ok(())
    } else {
        Err(LiveRelation::Mismatch(location.into()))
    }
}

fn depth(value: u8) -> u8 {
    if value == 16 { 16 } else { 8 }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Live,
    History,
    HistoryPersistence,
}

type BytePair = (Arc<Vec<u8>>, Arc<Vec<u8>>);

#[derive(Default)]
struct Comparisons {
    rasters: Vec<(Arc<Raster>, Arc<Raster>, String)>,
    original_samples: Vec<(Arc<Raster>, Arc<Raster>, String)>,
    masks: Vec<(Arc<Mask>, Arc<Mask>, String)>,
    pngs: Vec<(NodeId, Arc<Raster>, String)>,
    // Authored resource comparisons are memoized only after comparing bytes.
    // Both allocations are retained; Arc addresses are never equality proof.
    bytes: HashMap<(usize, usize), BytePair>,
}

/// Evidence is captured from the same manifest that decoded `live`, excluding
/// OriginalImage sources, whose validated bytes and exact planes are compared
/// separately. Only the working/head candidates should be offered by callers.
pub(crate) fn live_representation_matches<R: Read + Seek>(
    live: &Document,
    source_entries: &[(NodeId, String)],
    candidate: &Document,
    zip: &mut ZipArchive<R>,
) -> Result<LiveRelation> {
    let mut comparisons = Comparisons::default();
    if let Err(relation) = document(live, candidate, Mode::Live, &mut comparisons) {
        return Ok(relation);
    }
    if let Err(relation) = comparisons.exact_planes() {
        return Ok(relation);
    }
    let mut entries = HashMap::with_capacity(source_entries.len());
    for (id, entry) in source_entries {
        if entries.insert(*id, entry.as_str()).is_some() {
            return Ok(LiveRelation::Unverified(format!(
                "node {id}: ambiguous live source evidence"
            )));
        }
    }
    // Keys never prove content equality. Insert only after verifying every
    // row, and retain the allocation for the lifetime of the verified key.
    let mut verified: HashMap<(&str, usize, u8), Arc<Raster>> = HashMap::new();
    for (id, raster, location) in comparisons.pngs {
        let Some(entry) = entries.get(&id).copied() else {
            return Ok(LiveRelation::Unverified(format!(
                "{location}: no live source evidence"
            )));
        };
        let encoding = depth(live.source_depth);
        let key = (entry, Arc::as_ptr(&raster) as usize, encoding);
        if verified.contains_key(&key) {
            continue;
        }
        // One bounded encoded entry, a PNG scanline and decoder state. Never
        // allocate a second decoded raster/document or open a nested archive.
        let bytes = crate::ora::read_entry(zip, entry, 1 << 30)?;
        let relation = png_rows_match(&bytes, &raster, encoding, &location)?;
        if relation != LiveRelation::Consistent {
            return Ok(relation);
        }
        verified.insert(key, raster);
    }
    Ok(LiveRelation::Consistent)
}

/// Exact normalized history content, including selection, ID allocation,
/// sparse fills and retained effective Smart caches. No PNG projection.
pub(crate) fn history_matches(a: &Document, b: &Document) -> LiveRelation {
    history_relation(a, b, Mode::History)
}

/// The writer must not use Document::PartialEq to decide whether working state
/// exists. Compare all content retained by HDoc, excluding only live aids and
/// normalizing the caches the historical reader deliberately derives/discards.
pub(crate) fn history_persistence_matches(a: &Document, b: &Document) -> LiveRelation {
    history_relation(a, b, Mode::HistoryPersistence)
}

fn history_relation(a: &Document, b: &Document, mode: Mode) -> LiveRelation {
    let mut comparisons = Comparisons::default();
    match document(a, b, mode, &mut comparisons).and_then(|()| comparisons.exact_planes()) {
        Ok(()) => LiveRelation::Consistent,
        Err(relation) => relation,
    }
}

macro_rules! fields {
    ($other:expr, $location:expr; $($field:ident),+ $(,)?) => {
        $(same($field, &$other.$field, &format!("{}.{}", $location, stringify!($field)))?;)+
    };
}

fn document(a: &Document, b: &Document, mode: Mode, work: &mut Comparisons) -> Compared {
    // Deliberately exhaustive: a new Document field requires an explicit
    // persistence decision here, even when its existing PartialEq omits it.
    let Document {
        width,
        height,
        resolution,
        diagram,
        design,
        global_light,
        source_depth,
        blend_space,
        psd_background,
        nodes,
        next_id,
        selection,
        guides,
        info,
        raw,
        raw_originals,
        colors,
        drawing_guides,
    } = a;
    fields!(b, "document";
        width, height, resolution, diagram, global_light, blend_space,
        psd_background, guides, info, raw, raw_originals);
    same(
        &depth(*source_depth),
        &depth(b.source_depth),
        "document.source_depth",
    )?;
    design_metadata(design, &b.design)?;
    if mode != Mode::Live {
        fields!(b, "document"; next_id);
        work.mask(selection, &b.selection, "document.selection")?;
        // Both history readers supply default aids. Keeping these exact here
        // also prevents this helper overlooking nonhistorical state supplied
        // by a caller. Live-to-history explicitly excludes/overlays the aids.
        if mode == Mode::History {
            fields!(b, "document"; colors, drawing_guides);
        }
    }
    same(&nodes.len(), &b.nodes.len(), "document.nodes.length")?;
    for (index, (a, b)) in nodes.iter().zip(&b.nodes).enumerate() {
        node(a, b, mode, work, &format!("node[{index}]({})", a.id))?;
    }
    Ok(())
}

fn design_metadata(
    a: &emulsion_core::design_metadata::Design,
    b: &emulsion_core::design_metadata::Design,
) -> Compared {
    use emulsion_core::design_metadata::Design;
    // EmbeddedFont's derived equality includes its registration OnceLock.
    // Compare only its authored descriptors and bytes. The other fields have
    // value equality; notably LocalMedia and PatternImage compare byte Vecs.
    let Design {
        page_background,
        data_bindings,
        fonts,
        variable_libraries,
        variables,
        variable_bindings,
        interaction_triggers,
        interactions,
        overlays,
        local_media,
        keyframes,
        precision,
        speaker_notes,
        page_transition,
        transition_ms,
        saved_styles,
        style_links,
        components,
        component_links,
        media,
        charts,
        frames,
        constraints,
        duration_ms,
        fps,
        motion,
    } = a;
    fields!(b, "document.design";
        page_background, data_bindings, variable_libraries, variables,
        variable_bindings, interaction_triggers, interactions, overlays,
        local_media, keyframes, precision, speaker_notes, page_transition,
        transition_ms, saved_styles, style_links, components, component_links,
        media, charts, frames, constraints, duration_ms, fps, motion);
    same(&fonts.len(), &b.fonts.len(), "document.design.fonts.length")?;
    for ((key, a), (other_key, b)) in fonts.iter().zip(&b.fonts) {
        same(key, other_key, "document.design.fonts.key")?;
        same(a.alias(), b.alias(), "document.design.fonts.alias")?;
        same(a.family(), b.family(), "document.design.fonts.family")?;
        same(a.bytes(), b.bytes(), "document.design.fonts.bytes")?;
    }
    Ok(())
}

fn node(a: &Node, b: &Node, mode: Mode, work: &mut Comparisons, at: &str) -> Compared {
    let Node {
        id,
        name,
        parent,
        visible,
        locked,
        locks,
        color_label,
        link_group,
        opacity,
        blend,
        blending,
        clip_to,
        mask,
        vector_mask,
        mask_enabled,
        mask_properties,
        mask_linked,
        mask_transform,
        styles,
        style_options,
        effects_enabled,
        origin,
        review,
        kind,
    } = a;
    fields!(b, at;
        id, name, parent, visible, locked, locks, color_label, link_group,
        opacity, blend, blending, clip_to, vector_mask, mask_enabled,
        mask_properties, mask_linked, mask_transform, styles, style_options,
        effects_enabled, origin, review);
    work.mask(mask, &b.mask, &format!("{at}.mask"))?;
    kind_metadata(kind, &b.kind, *id, mode, work, at)
}

fn kind_metadata(
    a: &NodeKind,
    b: &NodeKind,
    id: NodeId,
    mode: Mode,
    work: &mut Comparisons,
    at: &str,
) -> Compared {
    let wrong_kind = || LiveRelation::Mismatch(format!("{at}.kind"));
    // Match one side exhaustively, with every field named on both sides. A
    // future kind cannot slip through a tuple-match wildcard.
    match a {
        NodeKind::Raster { raster, placement } => {
            let NodeKind::Raster {
                raster: other,
                placement: other_placement,
            } = b
            else {
                return Err(wrong_kind());
            };
            same(placement, other_placement, &format!("{at}.placement"))?;
            work.source(raster, other, id, mode, &format!("{at}.raster"))?;
        }
        NodeKind::Group { collapsed } => {
            let NodeKind::Group { collapsed: other } = b else {
                return Err(wrong_kind());
            };
            same(collapsed, other, &format!("{at}.collapsed"))?;
        }
        NodeKind::Adjust(adjustment) => {
            let NodeKind::Adjust(other) = b else {
                return Err(wrong_kind());
            };
            same(adjustment, other, &format!("{at}.adjustment"))?;
        }
        NodeKind::Fill { rgba } => {
            let NodeKind::Fill { rgba: other } = b else {
                return Err(wrong_kind());
            };
            same(rgba, other, &format!("{at}.rgba"))?;
        }
        NodeKind::Path { path, style, cache } => {
            let NodeKind::Path {
                path: other,
                style: other_style,
                cache: other_cache,
            } = b
            else {
                return Err(wrong_kind());
            };
            same(path.as_ref(), other.as_ref(), &format!("{at}.path"))?;
            same(style, other_style, &format!("{at}.style"))?;
            vector_cache(cache, other_cache, mode, work, at)?;
        }
        NodeKind::Text { spec, cache } => {
            let NodeKind::Text {
                spec: other,
                cache: other_cache,
            } = b
            else {
                return Err(wrong_kind());
            };
            same(spec.as_ref(), other.as_ref(), &format!("{at}.text"))?;
            vector_cache(cache, other_cache, mode, work, at)?;
        }
        NodeKind::Strokes { strokes, cache } => {
            let NodeKind::Strokes {
                strokes: other,
                cache: other_cache,
            } = b
            else {
                return Err(wrong_kind());
            };
            same(strokes.as_ref(), other.as_ref(), &format!("{at}.strokes"))?;
            vector_cache(cache, other_cache, mode, work, at)?;
        }
        NodeKind::Smart {
            editable,
            source,
            original_image,
            filters,
            filter_styles,
            filters_enabled,
            filter_mask,
            placement,
            cache,
            offset,
        } => {
            let NodeKind::Smart {
                editable: other_editable,
                source: other_source,
                original_image: other_original,
                filters: other_filters,
                filter_styles: other_styles,
                filters_enabled: other_enabled,
                filter_mask: other_mask,
                placement: other_placement,
                cache: other_cache,
                offset: other_offset,
            } = b
            else {
                return Err(wrong_kind());
            };
            work.editable(editable, other_editable, &format!("{at}.editable"))?;
            same(filters, other_filters, &format!("{at}.filters"))?;
            same(filter_styles, other_styles, &format!("{at}.filter_styles"))?;
            same(
                filters_enabled,
                other_enabled,
                &format!("{at}.filters_enabled"),
            )?;
            same(placement, other_placement, &format!("{at}.placement"))?;
            work.filter_mask(filter_mask, other_mask, at)?;
            match (original_image, other_original) {
                (None, None) => {
                    work.source(source, other_source, id, mode, &format!("{at}.source"))?
                }
                (Some(a), Some(b)) => {
                    same(
                        a.encoded_sha256(),
                        b.encoded_sha256(),
                        &format!("{at}.original_image.encoded_sha256"),
                    )?;
                    same(
                        a.source_sha256(),
                        b.source_sha256(),
                        &format!("{at}.original_image.source_sha256"),
                    )?;
                    work.bytes(a.bytes(), b.bytes(), &format!("{at}.original_image.bytes"))?;
                    // The pools already validated the descriptor and bound
                    // source. Original PNG encoding is independent of depth.
                    let comparisons = if mode != Mode::Live {
                        &mut work.rasters
                    } else {
                        // OriginalImage binds logical native samples, not
                        // the history plane's sparse-storage fill choice.
                        &mut work.original_samples
                    };
                    comparisons.push((
                        source.clone(),
                        other_source.clone(),
                        format!("{at}.original_image.source"),
                    ));
                }
                _ => {
                    return Err(LiveRelation::Mismatch(format!(
                        "{at}.original_image.presence"
                    )));
                }
            }
            if mode != Mode::Live
                && (mode == Mode::History
                    || emulsion_core::smart::has_active_filters(
                        filters,
                        filter_styles,
                        *filters_enabled,
                    ))
            {
                same(offset, other_offset, &format!("{at}.offset"))?;
                work.rasters
                    .push((cache.clone(), other_cache.clone(), format!("{at}.cache")));
            }
        }
    }
    Ok(())
}

fn vector_cache(
    a: &emulsion_core::vector_cache::VectorRaster,
    b: &emulsion_core::vector_cache::VectorRaster,
    mode: Mode,
    work: &mut Comparisons,
    at: &str,
) -> Compared {
    if mode == Mode::History {
        same(&a.size(), &b.size(), &format!("{at}.cache.size"))?;
        // HKind never saves vector pixels; both decoded caches start cold.
        // If a caller supplies materialized caches, compare them without ever
        // invoking rendering. One-sided materialization cannot be verified.
        match (a.rendered_pixels(), b.rendered_pixels()) {
            (None, None) => {}
            (Some(a), Some(b)) => work
                .rasters
                .push((a.clone(), b.clone(), format!("{at}.cache"))),
            _ => {
                return Err(LiveRelation::Unverified(format!(
                    "{at}.cache: only one vector cache is materialized"
                )));
            }
        }
    }
    Ok(())
}

impl Comparisons {
    fn bytes(&mut self, a: &Arc<Vec<u8>>, b: &Arc<Vec<u8>>, at: &str) -> Compared {
        let key = (Arc::as_ptr(a) as usize, Arc::as_ptr(b) as usize);
        if let std::collections::hash_map::Entry::Vacant(entry) = self.bytes.entry(key) {
            same(a.as_slice(), b.as_slice(), at)?;
            entry.insert((a.clone(), b.clone()));
        }
        Ok(())
    }

    fn editable(
        &mut self,
        a: &Option<emulsion_core::node::SmartEditable>,
        b: &Option<emulsion_core::node::SmartEditable>,
        at: &str,
    ) -> Compared {
        use emulsion_core::node::SmartEditable;
        let (a, b) = match (a, b) {
            (None, None) => return Ok(()),
            (Some(a), Some(b)) => (a, b),
            _ => return Err(LiveRelation::Mismatch(format!("{at}.presence"))),
        };
        let wrong_kind = || LiveRelation::Mismatch(format!("{at}.kind"));
        match a {
            SmartEditable::Document { archive, external } => {
                let SmartEditable::Document {
                    archive: other_archive,
                    external: other_external,
                } = b
                else {
                    return Err(wrong_kind());
                };
                same(external, other_external, &format!("{at}.external"))?;
                // Opaque current/future archives stay closed, even if a byte
                // string would not itself be a readable native document.
                self.bytes(archive, other_archive, &format!("{at}.archive"))?;
            }
            SmartEditable::Svg { xml } => {
                let SmartEditable::Svg { xml: other } = b else {
                    return Err(wrong_kind());
                };
                same(xml.as_ref(), other.as_ref(), &format!("{at}.xml"))?;
            }
            SmartEditable::Text { spec } => {
                let SmartEditable::Text { spec: other } = b else {
                    return Err(wrong_kind());
                };
                same(spec.as_ref(), other.as_ref(), &format!("{at}.text"))?;
            }
            SmartEditable::Path { path, style } => {
                let SmartEditable::Path {
                    path: other_path,
                    style: other_style,
                } = b
                else {
                    return Err(wrong_kind());
                };
                same(path.as_ref(), other_path.as_ref(), &format!("{at}.path"))?;
                same(style, other_style, &format!("{at}.style"))?;
            }
        }
        Ok(())
    }

    fn source(
        &mut self,
        a: &Arc<Raster>,
        b: &Arc<Raster>,
        id: NodeId,
        mode: Mode,
        at: &str,
    ) -> Compared {
        same(
            &(a.width(), a.height()),
            &(b.width(), b.height()),
            &format!("{at}.dimensions"),
        )?;
        if mode != Mode::Live {
            self.rasters.push((a.clone(), b.clone(), at.into()));
        } else {
            // Ordinary PNG does not serialize sparse fill. Compare logical
            // in-bounds writer samples; exact history equality checks fill.
            self.pngs.push((id, b.clone(), at.into()));
        }
        Ok(())
    }

    fn mask(&mut self, a: &Option<Arc<Mask>>, b: &Option<Arc<Mask>>, at: &str) -> Compared {
        match (a, b) {
            (None, None) => Ok(()),
            (Some(a), Some(b)) => {
                self.masks.push((a.clone(), b.clone(), at.into()));
                Ok(())
            }
            _ => Err(LiveRelation::Mismatch(format!("{at}.presence"))),
        }
    }

    fn filter_mask(
        &mut self,
        a: &Option<emulsion_core::SmartFilterMask>,
        b: &Option<emulsion_core::SmartFilterMask>,
        at: &str,
    ) -> Compared {
        match (a, b) {
            (None, None) => Ok(()),
            (Some(a), Some(b)) => {
                let emulsion_core::SmartFilterMask {
                    pixels,
                    enabled,
                    linked,
                    transform,
                    properties,
                } = a;
                fields!(b, &format!("{at}.filter_mask"); enabled, linked, transform, properties);
                self.masks.push((
                    pixels.clone(),
                    b.pixels.clone(),
                    format!("{at}.filter_mask.pixels"),
                ));
                Ok(())
            }
            _ => Err(LiveRelation::Mismatch(format!("{at}.filter_mask.presence"))),
        }
    }

    fn exact_planes(&self) -> Compared {
        plane_pairs(&self.rasters, true)?;
        plane_pairs(&self.original_samples, false)?;
        plane_pairs(&self.masks, true)
    }
}

type PlanePair<P> = (Arc<Plane<P>>, Arc<Plane<P>>, String);

fn plane_pairs<P: Pix + Eq + std::hash::Hash>(
    pairs: &[PlanePair<P>],
    exact_fill: bool,
) -> Compared {
    sparse_plane_pairs(pairs, exact_fill, &mut |_| {})
}

fn sparse_plane_pairs<P: Pix + Eq + std::hash::Hash>(
    pairs: &[PlanePair<P>],
    exact_fill: bool,
    samples: &mut impl FnMut(usize),
) -> Compared {
    use emulsion_raster::TILE;
    // All planes and their tiles stay retained by `pairs`: pointer keys may
    // memoize only comparisons already verified by samples, never provenance.
    let mut verified = HashSet::new();
    let mut verified_tiles = HashSet::new();
    for (a, b, at) in pairs {
        let key = (Arc::as_ptr(a) as usize, Arc::as_ptr(b) as usize);
        if verified.contains(&key) {
            continue;
        }
        same(
            &(a.width(), a.height()),
            &(b.width(), b.height()),
            &format!("{at}.dimensions"),
        )?;
        if exact_fill {
            same(&a.fill(), &b.fill(), &format!("{at}.fill"))?;
        }
        let (columns, rows) = a.tiles_at(0);
        let in_bounds =
            |c: &emulsion_raster::TileCoord| c.x >= 0 && c.y >= 0 && c.x < columns && c.y < rows;
        let left_tiles = || a.base_tiles().filter(|(c, _)| in_bounds(c));
        let only_b = || {
            b.base_tiles().filter(|(coordinate, _)| {
                in_bounds(coordinate) && a.base_tile(**coordinate).is_none()
            })
        };
        let covered = left_tiles().count() + only_b().count();
        if a.fill() != b.fill() && covered < columns as usize * rows as usize {
            return Err(LiveRelation::Mismatch(format!("{at}.unoccupied_samples")));
        }
        // Equal fills already prove every wholly unoccupied tile, regardless
        // of intrinsic image area. Visit only the union of populated tiles.
        for coordinate in left_tiles().map(|(c, _)| c).chain(only_b().map(|(c, _)| c)) {
            let x0 = coordinate.x as u32 * TILE;
            let y0 = coordinate.y as u32 * TILE;
            let width = TILE.min(a.width() - x0);
            let height = TILE.min(a.height() - y0);
            let left = a.base_tile(*coordinate);
            let right = b.base_tile(*coordinate);
            let tile_key = (
                left.map(|t| t.as_ptr() as usize),
                right.map(|t| t.as_ptr() as usize),
                a.fill(),
                b.fill(),
                width,
                height,
            );
            if verified_tiles.contains(&tile_key) {
                continue;
            }
            for y in 0..height {
                for x in 0..width {
                    let index = (y * TILE + x) as usize;
                    let left = left.map_or(a.fill(), |tile| tile[index]);
                    let right = right.map_or(b.fill(), |tile| tile[index]);
                    samples(1);
                    if left != right {
                        return Err(LiveRelation::Mismatch(format!(
                            "{at}.samples[{},{}]",
                            x0 + x,
                            y0 + y
                        )));
                    }
                }
            }
            verified_tiles.insert(tile_key);
        }
        verified.insert(key);
    }
    Ok(())
}

fn png_rows_match(bytes: &[u8], candidate: &Raster, depth: u8, at: &str) -> Result<LiveRelation> {
    let malformed =
        |error: png::DecodingError| IoError::Manifest(format!("{at}: live PNG: {error}"));
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::IDENTITY);
    // Ancillary text and profiles are not sample identity, and need not be
    // decompressed/retained merely to compare raw writer output.
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    decoder.set_limits(png::Limits { bytes: 64 << 20 });
    let mut reader = decoder.read_info().map_err(malformed)?;
    let info = reader.info();
    if info.interlaced
        || info.color_type != png::ColorType::Rgba
        || info.animation_control.is_some()
        || info.frame_control.is_some()
    {
        return Ok(LiveRelation::Unverified(format!(
            "{at}: live PNG is not noninterlaced RGBA"
        )));
    }
    let bytes_per_sample = match info.bit_depth {
        png::BitDepth::Eight if depth == 8 => 1usize,
        png::BitDepth::Sixteen if depth == 16 => 2usize,
        _ => {
            return Ok(LiveRelation::Unverified(format!(
                "{at}: live PNG depth differs from document source_depth"
            )));
        }
    };
    if (info.width, info.height) != (candidate.width(), candidate.height()) {
        return Ok(LiveRelation::Mismatch(format!("{at}.png.dimensions")));
    }
    crate::import::check_size(info.width, info.height)?;
    let row_bytes = usize::try_from(info.width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .and_then(|samples| samples.checked_mul(bytes_per_sample))
        .ok_or_else(|| IoError::Manifest(format!("{at}: PNG row size overflow")))?;
    for y in 0..candidate.height() {
        let Some(row) = reader.next_row().map_err(malformed)? else {
            return Err(IoError::Manifest(format!("{at}: truncated live PNG rows")));
        };
        let row = row.data();
        if row.len() != row_bytes {
            return Err(IoError::Manifest(format!(
                "{at}: invalid live PNG row length"
            )));
        }
        for x in 0..candidate.width() {
            let pixel = color::px_to_f(candidate.get(x, y));
            let matches = if bytes_per_sample == 1 {
                let offset = x as usize * 4;
                // Same scalar conversion as encode_srgba8_row (which has
                // exhaustive channel/alpha-boundary equivalence tests).
                row[offset..offset + 4] == color::premul_to_srgba8(pixel)
            } else {
                let expected = color::premul_to_srgba16(pixel);
                let offset = x as usize * 8;
                (0..4).all(|channel| {
                    let i = offset + channel * 2;
                    u16::from_be_bytes([row[i], row[i + 1]]) == expected[channel]
                })
            };
            if !matches {
                return Ok(LiveRelation::Mismatch(format!("{at}.png.samples[{x},{y}]")));
            }
        }
    }
    if reader.next_row().map_err(malformed)?.is_some() {
        return Err(IoError::Manifest(format!("{at}: excess live PNG rows")));
    }
    reader.finish().map_err(malformed)?;
    Ok(LiveRelation::Consistent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::node::{OriginalImage, SmartEditable};
    use emulsion_raster::Placement;
    use std::io::Write;

    fn raster_doc(raster: Raster, source_depth: u8) -> Document {
        let mut doc = Document::new(raster.width(), raster.height());
        doc.source_depth = source_depth;
        doc.nodes.push(Node::raster(
            1,
            "source",
            Arc::new(raster),
            Placement::default(),
        ));
        doc.next_id = 2;
        doc
    }

    fn smart_doc() -> Document {
        let mut doc = Document::new(2, 1);
        let source = Arc::new(Raster::empty(2, 1, [2, 1, 0, 7]));
        doc.nodes.push(Node::new(
            1,
            "smart",
            NodeKind::Smart {
                editable: None,
                source: source.clone(),
                original_image: None,
                filters: vec![emulsion_filters::Filter::Invert],
                filter_styles: vec![Default::default()],
                filters_enabled: true,
                filter_mask: Some(emulsion_core::SmartFilterMask::new(Arc::new(Mask::empty(
                    2, 1, 83,
                )))),
                placement: emulsion_core::mapping::SmartPlacement::Legacy(Placement::default()),
                cache: source,
                offset: (0, 0),
            },
        ));
        doc.next_id = 2;
        doc
    }

    fn png(
        width: u32,
        height: u32,
        bits: png::BitDepth,
        color: png::ColorType,
        samples: &[u8],
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, width, height);
            encoder.set_depth(bits);
            encoder.set_color(color);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(samples).unwrap();
            writer.finish().unwrap();
        }
        bytes
    }

    fn projected(raster: &Raster, depth: u8) -> Vec<u8> {
        if depth == 16 {
            let samples: Vec<u8> = raster
                .to_srgba16()
                .iter()
                .flat_map(|v| v.to_be_bytes())
                .collect();
            png(
                raster.width(),
                raster.height(),
                png::BitDepth::Sixteen,
                png::ColorType::Rgba,
                &samples,
            )
        } else {
            png(
                raster.width(),
                raster.height(),
                png::BitDepth::Eight,
                png::ColorType::Rgba,
                &raster.to_srgba8(),
            )
        }
    }

    fn zip_sources(entries: &[(&str, &[u8])]) -> ZipArchive<Cursor<Vec<u8>>> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        ZipArchive::new(zip.finish().unwrap()).unwrap()
    }

    fn compare_live(live: &Document, candidate: &Document, bytes: &[u8]) -> LiveRelation {
        live_representation_matches(
            live,
            &[(1, "source.png".into())],
            candidate,
            &mut zip_sources(&[("source.png", bytes)]),
        )
        .unwrap()
    }

    #[test]
    fn huge_sparse_planes_require_work_only_for_occupied_samples() {
        let mut pairs = Vec::new();
        for i in 0..128u8 {
            pairs.push((
                Arc::new(Mask::empty(20_000, 20_000, i)),
                Arc::new(Mask::empty(20_000, 20_000, i)),
                format!("mask{i}"),
            ));
        }
        let mut samples = 0usize;
        assert!(sparse_plane_pairs(&pairs, true, &mut |n| samples += n).is_ok());
        assert_eq!(samples, 0);
        // A padding-only difference does not change intrinsic content, and a
        // redundant populated fill tile remains equivalent to a sparse tile.
        let mut tile = vec![7u8; emulsion_raster::TILE_PX];
        tile[2] = 13;
        let a = Arc::new(
            Mask::from_tiles(
                2,
                1,
                7,
                [(emulsion_raster::TileCoord::new(0, 0), tile.into())],
            )
            .unwrap(),
        );
        let b = Arc::new(Mask::empty(2, 1, 7));
        let pairs = vec![
            (a.clone(), b.clone(), "padding".into()),
            (
                Arc::new(a.as_ref().clone()),
                Arc::new(b.as_ref().clone()),
                "shared tiles".into(),
            ),
        ];
        let mut samples = 0usize;
        assert!(sparse_plane_pairs(&pairs, true, &mut |n| samples += n).is_ok());
        assert_eq!(samples, 2, "verified shared tile samples are memoized");
    }

    #[test]
    fn live_rows_follow_actual_writer_at_both_depths_with_nonzero_sparse_fill() {
        for source_depth in [8, 16] {
            let source = Raster::from_pixels(
                4,
                1,
                [17, 8, 2, 31],
                &[
                    [1, 0, 0, 1],
                    [79, 23, 0, 127],
                    [8, 9, 3, 256],
                    [32101, 9001, 735, 65535],
                ],
            );
            let bytes = projected(&source, source_depth);
            let live = if source_depth == 16 {
                Raster::from_srgba16(4, 1, &source.to_srgba16())
            } else {
                Raster::from_srgba8(4, 1, &source.to_srgba8())
            };
            let candidate = raster_doc(source, source_depth);
            let mut live = raster_doc(live, source_depth);
            live.nodes[0].visible = false;
            let mut candidate = candidate;
            candidate.nodes[0].visible = false;
            assert_eq!(
                compare_live(&live, &candidate, &bytes),
                LiveRelation::Consistent
            );
        }
    }

    #[test]
    fn hidden_rgb_cannot_authorize_substitution_after_import_erases_it() {
        let live = raster_doc(Raster::from_srgba8(1, 1, &[91, 73, 55, 0]), 8);
        let candidate = raster_doc(Raster::empty(1, 1, [0; 4]), 8);
        let bytes = png(
            1,
            1,
            png::BitDepth::Eight,
            png::ColorType::Rgba,
            &[91, 73, 55, 0],
        );
        assert_eq!(history_matches(&live, &candidate), LiveRelation::Consistent);
        assert!(matches!(
            compare_live(&live, &candidate, &bytes),
            LiveRelation::Mismatch(_)
        ));
    }

    #[test]
    fn low_alpha_16_bit_samples_are_compared_before_lossy_import() {
        let samples = [1u16, 2, 3, 1];
        let bytes: Vec<_> = samples.iter().flat_map(|v| v.to_be_bytes()).collect();
        let bytes = png(1, 1, png::BitDepth::Sixteen, png::ColorType::Rgba, &bytes);
        let live = raster_doc(Raster::from_srgba16(1, 1, &samples), 16);
        let candidate = raster_doc(Raster::empty(1, 1, [0, 0, 0, 1]), 16);
        // The native samples coincide, but the candidate writes RGB zero.
        assert_eq!(live.nodes.len(), candidate.nodes.len());
        let NodeKind::Raster {
            raster: live_pixels,
            placement: _,
        } = &live.nodes[0].kind
        else {
            panic!()
        };
        let NodeKind::Raster {
            raster: candidate_pixels,
            placement: _,
        } = &candidate.nodes[0].kind
        else {
            panic!()
        };
        assert_eq!(live_pixels.get(0, 0), candidate_pixels.get(0, 0));
        assert!(matches!(
            compare_live(&live, &candidate, &bytes),
            LiveRelation::Mismatch(_)
        ));
    }

    #[test]
    fn noncanonical_encoding_cannot_verify_a_candidate() {
        let candidate = Raster::empty(1, 1, [0; 4]);
        for bytes in [
            png(1, 1, png::BitDepth::Eight, png::ColorType::Rgb, &[0; 3]),
            png(1, 1, png::BitDepth::Eight, png::ColorType::Grayscale, &[0]),
            png(1, 1, png::BitDepth::Sixteen, png::ColorType::Rgba, &[0; 8]),
        ] {
            assert!(matches!(
                png_rows_match(&bytes, &candidate, 8, "source").unwrap(),
                LiveRelation::Unverified(_)
            ));
        }
        let mut interlaced = projected(&candidate, 8);
        interlaced[28] = 1; // IHDR interlace field; 1x1 needs only Adam7 pass 1.
        let crc = crate::original_image_png::png_crc(b"IHDR", &interlaced[16..29]);
        interlaced[29..33].copy_from_slice(&crc.to_be_bytes());
        assert!(matches!(
            png_rows_match(&interlaced, &candidate, 8, "source").unwrap(),
            LiveRelation::Unverified(_)
        ));
    }

    #[test]
    fn different_compression_and_ancillary_metadata_are_not_pixel_identity() {
        let raster = Raster::empty(2, 1, [17, 8, 3, 400]);
        let plain = projected(&raster, 8);
        let mut alternate = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut alternate, 2, 1);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_compression(png::Compression::Fastest);
            encoder
                .add_text_chunk("Comment".into(), "Same authored samples".into())
                .unwrap();
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&raster.to_srgba8()).unwrap();
            writer.finish().unwrap();
        }
        assert_ne!(plain, alternate);
        for bytes in [plain, alternate] {
            assert_eq!(
                png_rows_match(&bytes, &raster, 8, "source").unwrap(),
                LiveRelation::Consistent
            );
        }
    }

    #[test]
    fn exact_history_compares_allocations_by_content_and_preserves_sparse_fill() {
        let a = raster_doc(Raster::from_pixels(2, 1, [31; 4], &[[9; 4], [7; 4]]), 16);
        let b = raster_doc(Raster::from_pixels(2, 1, [31; 4], &[[9; 4], [7; 4]]), 16);
        assert_eq!(history_matches(&a, &b), LiveRelation::Consistent);
        let different_fill = raster_doc(Raster::from_pixels(2, 1, [0; 4], &[[9; 4], [7; 4]]), 16);
        assert!(matches!(
            history_matches(&a, &different_fill),
            LiveRelation::Mismatch(_)
        ));
    }

    #[test]
    fn equal_writer_projection_does_not_make_exact_working_state_equal() {
        let a = Raster::empty(1, 1, [0, 0, 0, 1]);
        let b = Raster::empty(1, 1, [0, 0, 0, 2]);
        assert_eq!(a.to_srgba8(), b.to_srgba8());
        let a = raster_doc(a, 8);
        let b = raster_doc(b, 8);
        assert!(matches!(history_matches(&a, &b), LiveRelation::Mismatch(_)));
    }

    #[test]
    fn metadata_precedes_png_access_and_covers_fields_document_equality_omits() {
        let base = raster_doc(Raster::empty(1, 1, [0; 4]), 8);
        let mutations: &[fn(&mut Document)] = &[
            |d| d.source_depth = 16,
            |d| d.info = Some(emulsion_core::document::ImageInfo::default()),
            |d| d.nodes[0].opacity = f32::from_bits(1.0f32.to_bits() - 1),
            |d| d.nodes[0].kind = NodeKind::Fill { rgba: [0; 4] },
            |d| d.nodes[0].review = true,
            |d| d.nodes[0].mask_enabled = false,
        ];
        for mutate in mutations {
            let mut changed = base.clone();
            mutate(&mut changed);
            // No PNG resource exists: a metadata mismatch must return first.
            let relation = live_representation_matches(
                &base,
                &[(1, "absent".into())],
                &changed,
                &mut zip_sources(&[]),
            )
            .unwrap();
            assert!(matches!(relation, LiveRelation::Mismatch(_)));
        }
    }

    #[test]
    fn live_only_aids_and_history_only_selection_have_explicit_asymmetries() {
        let candidate = raster_doc(Raster::empty(1, 1, [0; 4]), 8);
        let mut live = candidate.clone();
        live.colors.push([23, 45, 67]);
        live.next_id = 44;
        live.selection = Some(Arc::new(Mask::empty(1, 1, 43)));
        let bytes = projected(&Raster::empty(1, 1, [0; 4]), 8);
        assert_eq!(
            compare_live(&live, &candidate, &bytes),
            LiveRelation::Consistent
        );
        assert!(matches!(
            history_matches(&live, &candidate),
            LiveRelation::Mismatch(_)
        ));
        for mutate in [
            (|d: &mut Document| d.next_id += 1) as fn(&mut Document),
            |d: &mut Document| d.selection = Some(Arc::new(Mask::empty(1, 1, 255))),
        ] {
            let mut changed = candidate.clone();
            mutate(&mut changed);
            assert!(matches!(
                history_matches(&candidate, &changed),
                LiveRelation::Mismatch(_)
            ));
        }
    }

    #[test]
    fn masks_compare_values_fill_dimensions_and_dormant_descriptors() {
        let mut a = smart_doc();
        let mut b = smart_doc();
        a.nodes[0].mask = Some(Arc::new(Mask::from_pixels(2, 1, 200, &[1, 2])));
        b.nodes[0].mask = Some(Arc::new(Mask::from_pixels(2, 1, 200, &[1, 2])));
        assert_eq!(history_matches(&a, &b), LiveRelation::Consistent);
        b.nodes[0].mask = Some(Arc::new(Mask::from_pixels(2, 1, 199, &[1, 2])));
        assert!(matches!(history_matches(&a, &b), LiveRelation::Mismatch(_)));
        b = a.clone();
        b.nodes[0].mask_transform = emulsion_core::mapping::Mapping2::Affine(
            glam::DAffine2::from_translation(glam::dvec2(0.0000000001, 0.0)),
        );
        assert!(matches!(history_matches(&a, &b), LiveRelation::Mismatch(_)));
        b = a.clone();
        let NodeKind::Smart { filter_mask, .. } = &mut b.nodes[0].kind else {
            panic!()
        };
        filter_mask.as_mut().unwrap().enabled = false;
        assert!(matches!(history_matches(&a, &b), LiveRelation::Mismatch(_)));
        b = a.clone();
        b.nodes[0].vector_mask = Some(emulsion_core::VectorMask::default());
        assert!(matches!(history_matches(&a, &b), LiveRelation::Mismatch(_)));
    }

    #[test]
    fn saved_smart_cache_and_offset_participate_only_in_exact_history() {
        let a = smart_doc();
        let source_png = projected(&Raster::empty(2, 1, [2, 1, 0, 7]), 8);
        for change_cache in [true, false] {
            let mut b = smart_doc();
            let NodeKind::Smart { cache, offset, .. } = &mut b.nodes[0].kind else {
                panic!()
            };
            if change_cache {
                *cache = Arc::new(Raster::empty(2, 1, [7; 4]));
            } else {
                *offset = (1, -1);
            }
            assert!(matches!(history_matches(&a, &b), LiveRelation::Mismatch(_)));
            assert_eq!(compare_live(&a, &b, &source_png), LiveRelation::Consistent);
        }
    }

    #[test]
    fn original_image_bytes_digests_and_bound_samples_are_exact() {
        let mut a = smart_doc();
        let mut b = smart_doc();
        for doc in [&mut a, &mut b] {
            let NodeKind::Smart { original_image, .. } = &mut doc.nodes[0].kind else {
                panic!()
            };
            *original_image = Some(Arc::new(OriginalImage::new(
                Arc::new(vec![1, 2, 3]),
                [4; 32],
                [5; 32],
            )));
        }
        assert_eq!(history_matches(&a, &b), LiveRelation::Consistent);
        // No ordinary source evidence needed for already validated originals.
        assert_eq!(
            live_representation_matches(&a, &[], &b, &mut zip_sources(&[])).unwrap(),
            LiveRelation::Consistent
        );
        // Native history retains a sparse fill; original PNGs and their source
        // digest bind in-bounds samples only. Different layouts remain valid.
        let mut different_fill = b.clone();
        let NodeKind::Smart { source, .. } = &mut different_fill.nodes[0].kind else {
            panic!()
        };
        *source = Arc::new(Raster::from_pixels(2, 1, [0; 4], &[[2, 1, 0, 7]; 2]));
        assert_eq!(
            live_representation_matches(&a, &[], &different_fill, &mut zip_sources(&[])).unwrap(),
            LiveRelation::Consistent
        );
        assert!(matches!(
            history_matches(&a, &different_fill),
            LiveRelation::Mismatch(_)
        ));
        for original in [
            OriginalImage::new(Arc::new(vec![1, 2, 4]), [4; 32], [5; 32]),
            OriginalImage::new(Arc::new(vec![1, 2, 3]), [6; 32], [5; 32]),
            OriginalImage::new(Arc::new(vec![1, 2, 3]), [4; 32], [6; 32]),
        ] {
            let NodeKind::Smart { original_image, .. } = &mut b.nodes[0].kind else {
                panic!()
            };
            *original_image = Some(Arc::new(original));
            assert!(matches!(history_matches(&a, &b), LiveRelation::Mismatch(_)));
        }
        b = a.clone();
        let NodeKind::Smart { source, .. } = &mut b.nodes[0].kind else {
            panic!()
        };
        *source = Arc::new(Raster::empty(2, 1, [3, 1, 0, 7]));
        assert!(matches!(
            live_representation_matches(&a, &[], &b, &mut zip_sources(&[])).unwrap(),
            LiveRelation::Mismatch(_)
        ));
    }

    #[test]
    fn opaque_archives_and_links_compare_content_without_opening_archives() {
        let mut a = smart_doc();
        let mut b = smart_doc();
        for doc in [&mut a, &mut b] {
            let NodeKind::Smart { editable, .. } = &mut doc.nodes[0].kind else {
                panic!()
            };
            *editable = Some(SmartEditable::Document {
                archive: Arc::new(b"opaque future archive, deliberately not decoded".to_vec()),
                external: Some(emulsion_core::smart_source::ExternalLink {
                    path: "/sources/future.ora".into(),
                    sha256: "a".repeat(64),
                    auto_refresh: false,
                    locally_modified: false,
                }),
            });
        }
        assert_eq!(history_matches(&a, &b), LiveRelation::Consistent);
        let NodeKind::Smart {
            editable:
                Some(SmartEditable::Document {
                    external,
                    archive: _,
                }),
            ..
        } = &mut b.nodes[0].kind
        else {
            panic!()
        };
        external.as_mut().unwrap().locally_modified = true;
        assert!(matches!(history_matches(&a, &b), LiveRelation::Mismatch(_)));
        b = a.clone();
        let NodeKind::Smart {
            editable:
                Some(SmartEditable::Document {
                    archive,
                    external: _,
                }),
            ..
        } = &mut b.nodes[0].kind
        else {
            panic!()
        };
        *archive = Arc::new(b"another opaque archive".to_vec());
        assert!(matches!(history_matches(&a, &b), LiveRelation::Mismatch(_)));
    }

    #[test]
    fn source_evidence_must_be_present_and_unambiguous() {
        let doc = raster_doc(Raster::empty(1, 1, [0; 4]), 8);
        for evidence in [vec![], vec![(1, "a".into()), (1, "b".into())]] {
            let relation =
                live_representation_matches(&doc, &evidence, &doc, &mut zip_sources(&[])).unwrap();
            assert!(matches!(relation, LiveRelation::Unverified(_)));
        }
    }

    #[test]
    fn verifying_one_shared_resource_does_not_verify_another_candidate_plane() {
        let mut live = raster_doc(Raster::empty(1, 1, [0; 4]), 8);
        let mut second = live.nodes[0].clone();
        second.id = 2;
        live.nodes.push(second);
        let mut candidate = live.clone();
        let NodeKind::Raster {
            raster,
            placement: _,
        } = &mut candidate.nodes[1].kind
        else {
            panic!()
        };
        *raster = Arc::new(Raster::empty(1, 1, [65535; 4]));
        let evidence = [(1, "same.png".into()), (2, "same.png".into())];
        let bytes = projected(&Raster::empty(1, 1, [0; 4]), 8);
        let relation = live_representation_matches(
            &live,
            &evidence,
            &candidate,
            &mut zip_sources(&[("same.png", &bytes)]),
        )
        .unwrap();
        assert!(matches!(relation, LiveRelation::Mismatch(_)));
    }

    #[test]
    fn pattern_and_media_resources_compare_bytes_in_independent_allocations() {
        let make = || {
            let mut doc = smart_doc();
            let mut options = emulsion_core::style_options::StyleOptions::default();
            options.pattern.image = Some(Arc::new(emulsion_core::style_options::PatternImage {
                width: 1,
                height: 1,
                pixels: vec![1, 2, 3, 4],
            }));
            doc.nodes[0].style_options.push(options);
            doc.design.local_media.insert(
                1,
                emulsion_core::design::media::LocalMedia {
                    boundary: 1,
                    name: "clip.mp4".into(),
                    kind: emulsion_core::design::media::LocalMediaKind::Video,
                    mime: "video/mp4".into(),
                    bytes: Arc::new(vec![3, 4, 5]),
                    trim_start_ms: 0,
                    trim_end_ms: None,
                    volume: 1.,
                    looping: false,
                },
            );
            doc
        };
        let a = make();
        let mut b = make();
        assert_eq!(history_matches(&a, &b), LiveRelation::Consistent);
        b.nodes[0].style_options[0].pattern.image =
            Some(Arc::new(emulsion_core::style_options::PatternImage {
                width: 1,
                height: 1,
                pixels: vec![1, 2, 3, 5],
            }));
        assert!(matches!(history_matches(&a, &b), LiveRelation::Mismatch(_)));
        b = make();
        b.design.local_media.get_mut(&1).unwrap().bytes = Arc::new(vec![3, 4, 6]);
        assert!(matches!(history_matches(&a, &b), LiveRelation::Mismatch(_)));
    }

    #[test]
    fn independent_vector_geometry_compares_without_rendering() {
        let make = || {
            let mut doc = Document::new(2, 2);
            doc.nodes.push(Node::path(
                1,
                "path",
                Arc::new(emulsion_raster::vector::Path::default()),
                emulsion_raster::vector::PathStyle::default(),
                2,
                2,
            ));
            doc
        };
        let a = make();
        let mut b = make();
        assert_eq!(history_matches(&a, &b), LiveRelation::Consistent);
        let NodeKind::Path {
            cache,
            path: _,
            style: _,
        } = &a.nodes[0].kind
        else {
            panic!()
        };
        assert!(!cache.is_rendered());
        let NodeKind::Path {
            path,
            cache: _,
            style: _,
        } = &mut b.nodes[0].kind
        else {
            panic!()
        };
        *path = Arc::new(emulsion_raster::vector::Path {
            subpaths: vec![emulsion_raster::vector::SubPath {
                anchors: vec![emulsion_raster::vector::Anchor::corner((0.125, 0.25))],
                closed: false,
            }],
        });
        assert!(matches!(history_matches(&a, &b), LiveRelation::Mismatch(_)));
        assert!(!cache.is_rendered());
    }
}
