//! Current-appearance selection, not inference of the authoring app's gamma setting.
//!
//! Only full-resolution CPU renders from editable inputs are compared. Merged
//! pixels never enter the layer tree, and unsupported metadata cannot earn
//! eligibility by looking harmless in the saved image.
use super::*;
use emulsion_raster::blend::BlendSpace;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct Association {
    knockout: BTreeMap<u32, Knockout>,
    pub background: Option<u32>,
}

fn layer_id(layer: &Layer) -> Option<u32> {
    let id = layer.additional_info.id?;
    (id.is_finite() && id.fract() == 0.0 && id > 0.0 && id <= f64::from(u32::MAX))
        .then_some(id as u32)
}

impl Association {
    pub fn knockout(&self, layer: &Layer) -> Option<Knockout> {
        self.knockout.get(&layer_id(layer)?).copied()
    }
    pub fn is_background(&self, layer: &Layer) -> bool {
        self.background.is_some() && self.background == layer_id(layer)
    }
    pub fn has_deep(&self) -> bool {
        self.knockout.values().any(|value| *value == Knockout::Deep)
    }
}

/// Match the complete decoded hierarchy to raw record framing and unique lyid
/// values. Names, offsets guessed from names, and global tag searches play no
/// part. Bounding dividers are framing records, never native nodes.
pub(super) fn associate(raw: &mask_guard::RawMetadata<'_>, psd: &Psd) -> Option<Association> {
    if !raw.single_layer_body || raw.records.len() > 2048 {
        return None;
    }
    let mut ids = BTreeSet::new();
    for record in &raw.records {
        let mut keys = BTreeSet::new();
        if record.tags.iter().any(|(key, _)| !keys.insert(*key)) {
            return None;
        }
        if let Some(id) = record.number(b"lyid")
            && (id == 0 || !ids.insert(id))
        {
            return None;
        }
    }
    fn walk(
        layers: &[Layer],
        raw: &[mask_guard::RawLayer<'_>],
        at: &mut usize,
        depth: usize,
        out: &mut Association,
    ) -> Option<()> {
        if layers.is_empty() {
            return Some(());
        }
        if depth > 32 {
            return None;
        }
        for layer in layers {
            if let Some(children) = &layer.children {
                if raw.get(*at)?.divider()? != 3 {
                    return None;
                }
                *at += 1;
                walk(children, raw, at, depth + 1, out)?;
            }
            let record = raw.get(*at)?;
            *at += 1;
            let raw_blend =
                ag_psd::helpers::to_blend_mode(std::str::from_utf8(record.blend_key).ok()?)?;
            if record.opacity != (layer.opacity.unwrap_or(1.0) * 255.0).round() as u8
                || (layer.children.is_none()
                    && raw_blend != layer.blend_mode.unwrap_or(PsdBlend::Normal))
                || layer.children.is_some() != matches!(record.divider()?, 1 | 2)
                || (layer.children.is_none() && record.divider()? != 0)
                || record.number(b"lyid")? != layer_id(layer)?
                || (record.flags & 2 != 0) != layer.hidden.unwrap_or(false)
                || (record.clipping != 0) != layer.clipping.unwrap_or(false)
            {
                return None;
            }
            let knockout = match record.one(b"knko") {
                Some(body) => blend_metadata::knockout_record(body)?,
                None => Knockout::None,
            };
            if (knockout != Knockout::None) != layer.additional_info.knockout.unwrap_or(false) {
                return None;
            }
            out.knockout.insert(layer_id(layer)?, knockout);
        }
        Some(())
    }
    let layers = psd.children.as_deref()?;
    let mut result = Association::default();
    let mut at = 0;
    walk(layers, &raw.records, &mut at, 0, &mut result)?;
    if at != raw.records.len() {
        return None;
    }
    let record = raw.records.first()?;
    let first = layers.first()?;
    // Visibility is the only accepted variation (0x09 versus 0x0b). A hidden
    // Background keeps identity, but contributes a transparent knockout stop.
    if first.children.is_none()
        && record.divider() == Some(0)
        && record.channels == [0, 1, 2]
        && record.flags & !2 == 0x09
        && record.number(b"lspf") == Some(0x0d)
        && record.one(b"lnsr") == Some(b"bgnd")
        && record.clipping == 0
    {
        result.background = record.number(b"lyid");
    }
    Some(result)
}

/// Export/import eligibility is independent of current pixel equality. Native
/// contracts for these families are not yet independently proven against third-party renders.
pub(super) fn supported_envelopes(doc: &Document) -> bool {
    let knockout = doc
        .nodes
        .iter()
        .any(|n| n.blending.knockout != Knockout::None);
    doc.nodes.iter().all(|node| {
        let b = &node.blending;
        if b.blend_if.source != Default::default()
            || b.blend_if.backdrop != Default::default()
            || b.channels != [true; 3]
            || !b.transparency_shapes_layer
            || (!b.blend_interior_effects_as_group && knockout)
            || !b.blend_clipped_layers_as_group
            || b.layer_mask_hides_effects
            || !node.styles.is_empty()
            || matches!(node.kind, NodeKind::Adjust(_))
            || (node.blend.has_special_fill() && b.fill_opacity != 1.0)
        {
            return false;
        }
        if node.blend == BlendMode::PassThrough {
            if node.opacity != 1.0 || b.fill_opacity != 1.0 || b.knockout != Knockout::None {
                return false;
            }
            if node.mask.is_some() || node.vector_mask.is_some() {
                // Preserve the established source-only geometry route, never
                // infer that an earlier hidden/empty node is a harmless backdrop.
                if knockout
                    || node.parent.is_some()
                    || doc.children(None).first() != Some(&node.id)
                    || doc
                        .nodes
                        .iter()
                        .any(|n| !matches!(n.blend, BlendMode::Normal | BlendMode::PassThrough))
                {
                    return false;
                }
            }
        }
        if knockout {
            if !matches!(node.blend, BlendMode::Normal | BlendMode::PassThrough)
                || node.mask.is_some()
                || node.vector_mask.is_some()
                || node.clip_to.is_some()
                || node.opacity != 1.0
                || matches!(node.kind, NodeKind::Smart { .. })
            {
                return false;
            }
            if (b.knockout != Knockout::None && node.parent.is_none())
                || (b.knockout == Knockout::None && b.fill_opacity != 1.0)
            {
                return false;
            }
            if let NodeKind::Raster { raster, placement } = &node.kind
                && (!raster_placement_is_translated(placement)
                    || raster
                        .to_srgba8()
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .any(|p| !matches!(p[3], 0 | 255)))
            {
                return false;
            }
        }
        true
    })
}

/// This geometry-preserving exception requires agreement of both supported
/// candidate renderers, as well as the emitted-layer check at export.
pub(super) fn requires_same_current(doc: &Document) -> bool {
    doc.nodes.iter().any(|node| {
        node.blend == BlendMode::PassThrough && (node.mask.is_some() || node.vector_mask.is_some())
    })
}

/// Rendering uses exactly the production RGBA16 quantization then RGBA8
/// conversion, while bypassing installed acceleration for every candidate.
/// Legacy-only importer candidate rendering. Native projective export uses
/// the fallible preparation path below.
pub(super) fn render_cpu(doc: &Document) -> Vec<u8> {
    assert!(!doc.nodes.iter().any(|node| node.has_projective_metadata()));
    try_render_cpu(doc).expect("validated legacy PSD candidate")
}

pub(super) fn try_render_cpu(doc: &Document) -> crate::Result<Vec<u8>> {
    use emulsion_raster::{TILE, TileCoord, color, composite::render_tile_cpu};
    let tree = doc.try_composite_tree()?;
    let mut raster = Raster::transparent(doc.width, doc.height);
    for y in 0..doc.height.div_ceil(TILE) {
        for x in 0..doc.width.div_ceil(TILE) {
            let coordinate = TileCoord::new(x as i32, y as i32);
            let pixels = render_tile_cpu(&tree, 0, coordinate)
                .into_iter()
                .map(color::f_to_px)
                .collect();
            raster.set_tile(coordinate, pixels);
        }
    }
    Ok(raster.to_srgba8())
}

pub(super) fn selectable(raw: &mask_guard::RawMetadata<'_>, doc: &Document) -> bool {
    raw.opaque_rgb8 && admissible(raw, doc)
}

pub(super) fn admissible(raw: &mask_guard::RawMetadata<'_>, doc: &Document) -> bool {
    if !raw.rgb8
        || !raw.srgb
        || !raw.single_layer_body
        || !raw.known_document_metadata
        || !within_budget(doc)
        || !supported_envelopes(doc)
    {
        return false;
    }
    raw.records.iter().all(|record| {
        let channels: BTreeSet<i16> = record.channels.iter().copied().collect();
        if channels.len() != record.channels.len()
            || channels.iter().any(|id| !matches!(*id, -2..=2))
            || (record.divider() == Some(0) && ![0, 1, 2].iter().all(|id| channels.contains(id)))
            || record.flags & !0x1b != 0
            || record.clipping > 1
        {
            return false;
        }
        record.tags.iter().all(|(key, body)| match *key {
            b"brst" => body.is_empty(), // Decoder drops the last restriction word.
            b"clbl" | b"tsly" => *body == [1, 0, 0, 0],
            // This typed flag is dormant without an effects family. The
            // independently supported Smart source fixture stores infx=0.
            b"infx" => body.len() == 4 && body[0] <= 1 && body[1..] == [0; 3],
            b"knko" => blend_metadata::knockout_record(body).is_some(),
            b"iOpa" => body.len() == 4 && body[1..] == [0; 3],
            b"lspf" => record.number(key).is_some_and(|flags| flags & !0x0f == 0),
            b"lsct" | b"lsdk" => {
                matches!(body.len(), 4 | 12 | 16)
                    && record.divider().is_some_and(|kind| kind <= 3)
                    && (body.len() == 4
                        || (&body[4..8] == b"8BIM" && matches!(&body[8..12], b"norm" | b"pass")))
                    && (body.len() != 16 || body[12..] == [0; 4])
            }
            b"lnsr" => body.len() == 4,
            b"lyid" => body.len() == 4,
            b"lclr" => body.len() == 8 && body[2..] == [0; 6],
            b"fxrp" => body.len() == 16,
            b"lyvr" => *body == [0, 0, 0, 1],
            b"luni" | b"vmsk" | b"vsms" | b"SoLd" | b"SoLE" | b"PlLd" | b"Plcd" => true,
            _ => false,
        })
    })
}

/// Shared import/export admission for the new profile. All authored state must
/// first pass the envelope gate, even on hidden nodes. A supported non-Normal
/// mode can be retained when visibility alone proves it cannot contribute to
/// the current image; zero opacity, clipping, and pixel contents are not proofs.
pub(super) fn selector_eligible(doc: &Document) -> bool {
    supported_envelopes(doc)
        && doc.nodes.iter().all(|node| {
            if matches!(node.blend, BlendMode::Normal | BlendMode::PassThrough) {
                return true;
            }
            let mut current = Some(node.id);
            while let Some(id) = current {
                let Some(ancestor) = doc.node(id) else {
                    return false;
                };
                if !ancestor.visible {
                    return true;
                }
                current = ancestor.parent;
            }
            false
        })
}

/// One candidate-decision policy for opening original bytes and verifying our
/// emitted editable inputs. A reference is used only for admitted opaque RGB8
/// data; callers must validate original saved bytes before passing that data.
/// The other route compares fresh candidates with one another, never matte-
/// decoded merged pixels. Deep always requires an authoritative unique match.
pub(super) fn candidate_decision(
    raw: &mask_guard::RawMetadata<'_>,
    association: Option<&Association>,
    doc: &mut Document,
    reference: Option<&[u8]>,
) -> ImportProfileDecision {
    doc.blend_space = BlendSpace::Srgb;
    let deep = association.is_some_and(Association::has_deep);
    let decision = if !supported_envelopes(doc) {
        ImportProfileDecision::SavedAppearance
    } else if !raw.real_merged || association.is_none() || !admissible(raw, doc) {
        ImportProfileDecision::NotCompared
    } else if raw.opaque_rgb8 {
        reference.map_or(ImportProfileDecision::NotCompared, |saved| {
            select(doc, saved)
        })
    } else if !deep {
        let legacy = render_cpu(doc);
        doc.blend_space = BlendSpace::PhotoshopSrgbV1;
        let same = render_cpu(doc) == legacy;
        doc.blend_space = BlendSpace::Srgb;
        if same {
            ImportProfileDecision::SameCurrentAppearance
        } else {
            ImportProfileDecision::SavedAppearance
        }
    } else {
        ImportProfileDecision::SavedAppearance
    };
    if deep && decision != ImportProfileDecision::UniquePhotoshopSrgbV1 {
        ImportProfileDecision::SavedAppearance
    } else {
        decision
    }
}

pub(super) fn select(doc: &mut Document, saved: &[u8]) -> ImportProfileDecision {
    let mut matches = Vec::new();
    for space in [
        BlendSpace::Linear,
        BlendSpace::Srgb,
        BlendSpace::PhotoshopSrgbV1,
    ] {
        doc.blend_space = space;
        if render_cpu(doc) == saved {
            matches.push(space);
        }
    }
    if requires_same_current(doc)
        && !(matches.contains(&BlendSpace::Srgb) && matches.contains(&BlendSpace::PhotoshopSrgbV1))
    {
        return ImportProfileDecision::SavedAppearance;
    }
    if matches == [BlendSpace::PhotoshopSrgbV1] && selector_eligible(doc) {
        ImportProfileDecision::UniquePhotoshopSrgbV1
    } else if matches.contains(&BlendSpace::Srgb) {
        doc.blend_space = BlendSpace::Srgb;
        ImportProfileDecision::LegacyMatch {
            ambiguous: matches.len() > 1,
        }
    } else {
        // Linear-only equality does not establish an independently proven PSD
        // interpretation. Keep the genuine saved image instead of guessing.
        ImportProfileDecision::SavedAppearance
    }
}

pub(super) fn within_budget(doc: &Document) -> bool {
    let pixels = u64::from(doc.width) * u64::from(doc.height);
    if pixels > 16_777_216
        || doc.nodes.len() > 1024
        || pixels.saturating_mul(doc.nodes.len().max(1) as u64) > 67_108_864
    {
        return false;
    }
    let mut source_pixels = 0u64;
    doc.nodes.iter().all(|node| {
        source_pixels = source_pixels.saturating_add(match &node.kind {
            NodeKind::Raster { raster, .. } => {
                u64::from(raster.width()) * u64::from(raster.height())
            }
            NodeKind::Smart { source, .. } => {
                u64::from(source.width()) * u64::from(source.height())
            }
            _ => 0,
        });
        if source_pixels > 67_108_864 {
            return false;
        }
        let mut parent = node.parent;
        let mut depth = 0;
        while let Some(id) = parent {
            depth += 1;
            if depth > 32 {
                return false;
            }
            parent = doc.node(id).and_then(|node| node.parent);
        }
        true
    })
}

/// A PSD Background record is narrower than native dormant identity.
/// Never convert an ordinary opaque bottom raster merely because it fits.
pub(super) fn background_exportable(doc: &Document) -> bool {
    let Some(id) = doc.psd_background else {
        return true;
    };
    let Some(node) = doc.node(id) else {
        return false;
    };
    let NodeKind::Raster { raster, placement } = &node.kind else {
        return false;
    };
    doc.children(None).first() == Some(&id)
        && placement.x == 0.0
        && placement.y == 0.0
        && raster_placement_is_translated(placement)
        && (raster.width(), raster.height()) == (doc.width, doc.height)
        && raster
            .to_srgba8()
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[3] == 255)
        && node.mask.is_none()
        && node.vector_mask.is_none()
        && node.clip_to.is_none()
        && node.blend == BlendMode::Normal
        && node.opacity == 1.0
        && node.blending == Default::default()
        && node.styles.is_empty()
}

#[derive(Default)]
pub(super) struct ExportMetadata {
    knockout: BTreeMap<u32, Knockout>,
    background: Option<u32>,
}

pub(super) fn prepare_layers(doc: &Document, layers: &mut [Layer]) -> Result<ExportMetadata> {
    fn walk(
        doc: &Document,
        parent: Option<NodeId>,
        layers: &mut [Layer],
        next: &mut u32,
        out: &mut ExportMetadata,
    ) -> Result<()> {
        let ids = doc.children(parent);
        if ids.len() != layers.len() {
            return Err(IoError::Unsupported("PSD export hierarchy mismatch".into()));
        }
        for (id, layer) in ids.into_iter().zip(layers) {
            let node = doc.node(id).expect("validated child");
            let raw_id = *next;
            *next = next
                .checked_add(1)
                .ok_or_else(|| IoError::Unsupported("too many PSD layer IDs".into()))?;
            layer.additional_info.id = Some(f64::from(raw_id));
            out.knockout.insert(raw_id, node.blending.knockout);
            if doc.psd_background == Some(id) {
                out.background = Some(raw_id);
                layer.transparency_protected = Some(true);
                layer.additional_info.name_source = Some("bgnd".into());
                layer.additional_info.protected_info = Some(ag_psd::psd::ProtectedInfo {
                    transparency: Some(true),
                    composite: Some(false),
                    position: Some(true),
                    artboards: Some(true),
                });
            }
            if let Some(children) = layer.children.as_mut() {
                walk(doc, Some(id), children, next, out)?;
            }
        }
        Ok(())
    }
    let mut result = ExportMetadata::default();
    walk(doc, None, layers, &mut 1, &mut result)?;
    Ok(result)
}

/// Repair ag-psd's bool-only knko writer only after proving every emitted lyid,
/// group frame, and Background role against the prepared decoded hierarchy.
/// Every offset points to a body returned by the strict record walk. Failure
/// occurs before write_atomic can mutate the destination.
pub(super) fn patch_export(
    mut bytes: Vec<u8>,
    psd: &Psd,
    expected: &ExportMetadata,
) -> Result<Vec<u8>> {
    let fail = || IoError::Unsupported("PSD emitted layer metadata association mismatch".into());
    let mut patches = Vec::new();
    {
        let raw = mask_guard::raw_metadata(&bytes)
            .map_err(|error| IoError::Unsupported(error.to_string()))?;
        let association = associate(&raw, psd).ok_or_else(fail)?;
        if raw.reason.is_some()
            || association.background != expected.background
            || association.knockout.len() != expected.knockout.len()
        {
            return Err(fail());
        }
        for record in &raw.records {
            if record.divider() == Some(3) {
                continue;
            }
            let id = record.number(b"lyid").ok_or_else(fail)?;
            let intended = expected.knockout.get(&id).ok_or_else(fail)?;
            let body = record.one(b"knko").ok_or_else(fail)?;
            let encoded = blend_metadata::knockout_record(body).ok_or_else(fail)?;
            if encoded
                != if *intended == Knockout::None {
                    Knockout::None
                } else {
                    Knockout::Shallow
                }
            {
                return Err(fail());
            }
            if *intended == Knockout::Deep {
                patches.push(body.as_ptr() as usize - bytes.as_ptr() as usize);
            }
        }
    }
    for offset in patches {
        bytes[offset] = 2;
    }
    let raw = mask_guard::raw_metadata(&bytes)
        .map_err(|error| IoError::Unsupported(error.to_string()))?;
    let actual = associate(&raw, psd).ok_or_else(fail)?;
    if actual.knockout != expected.knockout || actual.background != expected.background {
        return Err(fail());
    }
    Ok(bytes)
}

/// The emitted layer channels and quantized envelopes must reproduce the saved
/// image before a layered export is reported. Decode only our just-encoded,
/// bounded inputs; the merged pixels are deliberately skipped by the decoder.
pub(super) fn emitted_matches(bytes: &[u8], expected: &[u8]) -> Result<bool> {
    let raw =
        mask_guard::raw_metadata(bytes).map_err(|error| IoError::Unsupported(error.to_string()))?;
    let normalized = mask_guard::vector_decoder_copy(bytes)
        .map_err(|error| IoError::Unsupported(error.to_string()))?;
    let decoded = ag_psd::read_psd(
        normalized.as_deref().unwrap_or(bytes),
        &ReadOptions {
            skip_composite_image_data: Some(true),
            skip_thumbnail: Some(true),
            skip_linked_files_data: Some(true),
            use_image_data: Some(true),
            ..Default::default()
        },
    )
    .map_err(|error| IoError::Unsupported(format!("PSD emitted layer decode: {error:?}")))?;
    let association = associate(&raw, &decoded)
        .ok_or_else(|| IoError::Unsupported("PSD emitted layer association failed".into()))?;
    let sources = smart_objects::inspect(bytes).map_err(|error| match error {
        smart_objects::SmartError::Unavailable(_) => {
            std::io::Error::other(error.to_string()).into()
        }
        _ => IoError::Unsupported(error.to_string()),
    })?;
    let mut doc = from_psd_with_metadata(&decoded, false, &sources, Some(&association))?;
    // The actual PSD-compatible rendering must still match after quantization;
    // a legacy-only match must not hide a changed PSD-compatible appearance.
    doc.blend_space = BlendSpace::PhotoshopSrgbV1;
    if render_cpu(&doc) != expected {
        return Ok(false);
    }
    // Run the exact same candidate decision as import, including visibility,
    // legacy ambiguity, source-only mask limits, and Deep's unique-match rule.
    let decision = candidate_decision(&raw, Some(&association), &mut doc, Some(expected));
    if decision == ImportProfileDecision::SavedAppearance {
        return Ok(false);
    }
    Ok(render_cpu(&doc) == expected)
}
