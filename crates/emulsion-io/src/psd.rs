//! PSD documents through `ag-psd`: layers, groups, opacity, blend
//! modes, visibility and masks come across in both directions. Reading
//! turns every pixel layer into a raster node; operations that cannot be
//! reconstructed exactly use an explicitly named flattened appearance layer.
//! Native Emulsion files retain the complete editable source document. PSD
//! raster masks retain their independent pixels, bounds, link/enable state and
//! density/feather parameters when their grid needs only integer translation.
//! Other mask affines use the existing baked-coverage appearance route.

use crate::{IoError, Result, write_atomic};
use ag_psd::psd::{BlendMode as PsdBlend, ColorMode, Layer, LayerMaskData, PixelData, Psd};
use ag_psd::psd::{ReadOptions, WriteOptions};
use emulsion_core::command::Slot;
use emulsion_core::{Command, Document, MaskProperties, Node, NodeId, NodeKind};
#[cfg(test)]
use emulsion_raster::composite::BlendRange;
use emulsion_raster::composite::Knockout;
use emulsion_raster::composite::flatten;
use emulsion_raster::{BlendMode, Mask, Placement, Raster};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

#[path = "psd/mask_guard.rs"]
mod mask_guard;

#[path = "psd/blend_metadata.rs"]
mod blend_metadata;
#[path = "psd/profile.rs"]
mod profile;
#[path = "psd/smart_objects.rs"]
mod smart_objects;
#[path = "psd/vector_guard.rs"]
mod vector_guard;
#[path = "psd/vector_mask.rs"]
mod vector_mask;

#[cfg(test)]
#[path = "psd/raster_mask_tests.rs"]
mod raster_mask_tests;

#[cfg(test)]
#[path = "psd/vector_mask_tests.rs"]
mod vector_mask_tests;

#[cfg(test)]
#[path = "psd/profile_tests.rs"]
mod profile_tests;

#[cfg(test)]
#[path = "psd/blend_interchange_tests.rs"]
mod blend_interchange_tests;
#[cfg(test)]
mod blend_photoshop_fixture_tests;
#[cfg(test)]
mod smart_interchange_tests;

pub fn is_psd(path: &Path) -> bool {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|e| e == "psd" || e == "psb")
}

fn blend_in(b: Option<PsdBlend>) -> BlendMode {
    match b.unwrap_or(PsdBlend::Normal) {
        PsdBlend::PassThrough => BlendMode::PassThrough,
        PsdBlend::Normal => BlendMode::Normal,
        PsdBlend::Dissolve => BlendMode::Dissolve,
        PsdBlend::Darken => BlendMode::Darken,
        PsdBlend::Multiply => BlendMode::Multiply,
        PsdBlend::ColorBurn => BlendMode::ColorBurn,
        PsdBlend::LinearBurn => BlendMode::LinearBurn,
        PsdBlend::DarkerColor => BlendMode::DarkerColor,
        PsdBlend::Lighten => BlendMode::Lighten,
        PsdBlend::Screen => BlendMode::Screen,
        PsdBlend::ColorDodge => BlendMode::ColorDodge,
        PsdBlend::LinearDodge => BlendMode::LinearDodge,
        PsdBlend::LighterColor => BlendMode::LighterColor,
        PsdBlend::Overlay => BlendMode::Overlay,
        PsdBlend::SoftLight => BlendMode::SoftLight,
        PsdBlend::HardLight => BlendMode::HardLight,
        PsdBlend::VividLight => BlendMode::VividLight,
        PsdBlend::LinearLight => BlendMode::LinearLight,
        PsdBlend::PinLight => BlendMode::PinLight,
        PsdBlend::HardMix => BlendMode::HardMix,
        PsdBlend::Difference => BlendMode::Difference,
        PsdBlend::Exclusion => BlendMode::Exclusion,
        PsdBlend::Subtract => BlendMode::Subtract,
        PsdBlend::Divide => BlendMode::Divide,
        PsdBlend::Hue => BlendMode::Hue,
        PsdBlend::Saturation => BlendMode::Saturation,
        PsdBlend::Color => BlendMode::Color,
        PsdBlend::Luminosity => BlendMode::Luminosity,
        #[allow(unreachable_patterns)]
        _ => BlendMode::Normal,
    }
}

fn blend_out(b: BlendMode) -> PsdBlend {
    match b {
        BlendMode::PassThrough => PsdBlend::PassThrough,
        BlendMode::Normal => PsdBlend::Normal,
        BlendMode::Dissolve => PsdBlend::Dissolve,
        BlendMode::Darken => PsdBlend::Darken,
        BlendMode::Multiply => PsdBlend::Multiply,
        BlendMode::ColorBurn => PsdBlend::ColorBurn,
        BlendMode::LinearBurn => PsdBlend::LinearBurn,
        BlendMode::DarkerColor => PsdBlend::DarkerColor,
        BlendMode::Lighten => PsdBlend::Lighten,
        BlendMode::Screen => PsdBlend::Screen,
        BlendMode::ColorDodge => PsdBlend::ColorDodge,
        BlendMode::LinearDodge => PsdBlend::LinearDodge,
        BlendMode::LighterColor => PsdBlend::LighterColor,
        BlendMode::Overlay => PsdBlend::Overlay,
        BlendMode::SoftLight => PsdBlend::SoftLight,
        BlendMode::HardLight => PsdBlend::HardLight,
        BlendMode::VividLight => PsdBlend::VividLight,
        BlendMode::LinearLight => PsdBlend::LinearLight,
        BlendMode::PinLight => PsdBlend::PinLight,
        BlendMode::HardMix => PsdBlend::HardMix,
        BlendMode::Difference => PsdBlend::Difference,
        BlendMode::Exclusion => PsdBlend::Exclusion,
        BlendMode::Subtract => PsdBlend::Subtract,
        BlendMode::Divide => PsdBlend::Divide,
        BlendMode::Hue => PsdBlend::Hue,
        BlendMode::Saturation => PsdBlend::Saturation,
        BlendMode::Color => PsdBlend::Color,
        BlendMode::Luminosity => PsdBlend::Luminosity,
        #[allow(unreachable_patterns)]
        _ => PsdBlend::Normal,
    }
}

/// The first `width`×`height`×`channels` bytes of a pixel block, or `None`
/// when the data is shorter than its dimensions claim. Callers must have
/// passed the dimensions through `check_size`, so the product fits.
fn pixel_bytes(px: &PixelData, channels: usize) -> Option<&[u8]> {
    let len = (px.width as usize)
        .checked_mul(px.height as usize)?
        .checked_mul(channels)?;
    px.data.get(..len)
}

/// Grey values from a mask's pixel block, whichever layout ag-psd used.
/// The block's size must already be checked.
fn mask_bytes(px: &PixelData) -> Vec<u8> {
    let n = px.width as usize * px.height as usize;
    if px.data.len() == n * 4 {
        px.data.as_chunks::<4>().0.iter().map(|c| c[0]).collect()
    } else if px.data.len() >= n {
        px.data[..n].to_vec()
    } else {
        vec![255; n]
    }
}

fn mask_properties_in(mask: &LayerMaskData) -> Option<MaskProperties> {
    let density = mask.user_mask_density.unwrap_or(1.0);
    let feather = mask.user_mask_feather.unwrap_or(0.0);
    if !density.is_finite()
        || !(0.0..=1.0).contains(&density)
        || !feather.is_finite()
        || !(0.0..=f64::from(emulsion_core::MAX_MASK_FEATHER)).contains(&feather)
    {
        return None;
    }
    let properties = MaskProperties {
        density: density as f32,
        feather: feather as f32,
    };
    properties.valid().then_some(properties)
}

/// Keep the original mask grid, including pixels beyond the layer/canvas.
/// PSD bounds are document coordinates even when bit 0 is set. That historical
/// "position relative to layer" flag actually means unlinked; see the upstream XCF-editor
/// interoperability fix eb2741ed70d156e40bdd8f43be17100c550d502f.
fn mask_in(node: &mut Node, m: &LayerMaskData, lx: f64, ly: f64) -> Result<()> {
    let Some(px) = m.image_data.as_ref().or(m.canvas.as_ref()) else {
        return Err(IoError::Unsupported(
            "PSD raster mask has no pixel channel".into(),
        ));
    };
    crate::import::check_size(px.width, px.height)?;
    let pixels = px.width as usize * px.height as usize;
    if px.data.len() != pixels && px.data.len() != pixels * 4 {
        return Err(IoError::Unsupported(
            "PSD raster mask pixel data is truncated".into(),
        ));
    }
    let (left, top) = (m.left.unwrap_or(0.0), m.top.unwrap_or(0.0));
    let (right, bottom) = (
        m.right.unwrap_or(left + f64::from(px.width)),
        m.bottom.unwrap_or(top + f64::from(px.height)),
    );
    if [left, top, right, bottom].iter().any(|value| {
        !value.is_finite()
            || value.fract() != 0.0
            || *value < i32::MIN as f64
            || *value > i32::MAX as f64
    }) || right - left != f64::from(px.width)
        || bottom - top != f64::from(px.height)
    {
        return Err(IoError::Unsupported(
            "PSD raster mask bounds do not match its pixel channel".into(),
        ));
    }
    let fill = m.default_color.unwrap_or(255.0).clamp(0.0, 255.0) as u8;
    node.mask = Some(Arc::new(Mask::from_pixels(
        px.width,
        px.height,
        fill,
        &mask_bytes(px),
    )));
    node.mask_transform = emulsion_core::Mapping2::Affine(glam::DAffine2::from_translation(
        glam::dvec2(left - lx, top - ly),
    ));
    node.mask_enabled = !m.disabled.unwrap_or(false);
    node.mask_linked = !m.position_relative_to_layer.unwrap_or(false);
    node.mask_properties = mask_properties_in(m)
        .ok_or_else(|| IoError::Unsupported("PSD raster mask parameters are unsupported".into()))?;
    Ok(())
}

fn add(doc: &mut Document, node: Node, parent: Option<NodeId>) -> Result<NodeId> {
    Command::AddNode {
        node: Box::new(node),
        slot: Slot::top_of(parent),
    }
    .apply(doc)
    .map_err(|e| IoError::Unsupported(format!("PSD: {e}")))?
    .ok_or_else(|| IoError::Unsupported("PSD: node not added".into()))
}

fn add_layers(
    doc: &mut Document,
    layers: &[Layer],
    parent: Option<NodeId>,
    sources: &smart_objects::ImportSources,
    association: Option<&profile::Association>,
) -> Result<()> {
    // ag-psd lists layers bottom to top, as the file does.
    let mut clip_base = None;
    for l in layers {
        let name = l
            .additional_info
            .name
            .clone()
            .unwrap_or_else(|| "Layer".into());
        let mut node = if let Some(children) = &l.children {
            let mut g = Node::group(0, name);
            g.blend = blend_in(l.blend_mode);
            if let Some(m) = &l.additional_info.mask {
                mask_in(&mut g, m, 0.0, 0.0)?;
            }
            g.vector_mask = vector_mask::import(l, (doc.width, doc.height), (0.0, 0.0));
            let id = finish_node(doc, g, l, parent, association)?;
            add_layers(doc, children, Some(id), sources, association)?;
            if l.clipping.unwrap_or(false) {
                Command::SetClip {
                    id,
                    clip_to: clip_base,
                }
                .apply(doc)
                .map_err(|e| IoError::Unsupported(format!("PSD: {e}")))?;
            } else {
                clip_base = Some(id);
            }
            continue;
        } else {
            let px = l.image_data.as_ref().or(l.canvas.as_ref());
            let (left, top) = (l.left.unwrap_or(0.0), l.top.unwrap_or(0.0));
            let pixels = match px {
                Some(p) if p.width > 0 && p.height > 0 => {
                    crate::import::check_size(p.width, p.height)?;
                    pixel_bytes(p, 4).map(|data| (p, data))
                }
                _ => None,
            };
            let raster = match pixels {
                Some((p, data)) => Raster::from_srgba8(p.width, p.height, data),
                None => Raster::transparent(1, 1),
            };
            let mut n = if let Some(kind) = sources
                .layer_kind(l)
                .map_err(|error| IoError::Unsupported(error.to_string()))?
            {
                Node::new(0, name, kind)
            } else {
                Node::raster(0, name, Arc::new(raster), Placement::at(left, top))
            };
            // A Smart preview can be cropped independently of its original
            // source. Ordinary masks map against the placed source origin.
            let (left, top) = match &n.kind {
                NodeKind::Smart { placement, .. } => {
                    let placement = placement.require_legacy("PSD source import")?;
                    (placement.x, placement.y)
                }
                _ => (left, top),
            };
            if let Some(m) = &l.additional_info.mask {
                mask_in(&mut n, m, left, top)?;
            }
            n.vector_mask = vector_mask::import(l, (doc.width, doc.height), (left, top));
            n
        };
        node.blend = blend_in(l.blend_mode);
        let id = finish_node(doc, node, l, parent, association)?;
        if l.clipping.unwrap_or(false) {
            Command::SetClip {
                id,
                clip_to: clip_base,
            }
            .apply(doc)
            .map_err(|e| IoError::Unsupported(format!("PSD: {e}")))?;
        } else {
            clip_base = Some(id);
        }
    }
    Ok(())
}

fn finish_node(
    doc: &mut Document,
    mut node: Node,
    l: &Layer,
    parent: Option<NodeId>,
    association: Option<&profile::Association>,
) -> Result<NodeId> {
    node.visible = !l.hidden.unwrap_or(false);
    node.opacity = l.opacity.unwrap_or(1.0).clamp(0.0, 1.0) as f32;
    let info = &l.additional_info;
    node.blending.fill_opacity = info.fill_opacity.unwrap_or(1.0).clamp(0.0, 1.0) as f32;
    if let Some(restricted) = &info.channel_blending_restrictions {
        for channel in restricted {
            let index = *channel as usize;
            if index < 3 {
                node.blending.channels[index] = false;
            }
        }
    }
    node.blending.blend_if = blend_metadata::import(info)
        .ok_or_else(|| IoError::Unsupported("PSD Blend If ranges are unsupported".into()))?;
    node.blending.knockout = association.and_then(|a| a.knockout(l)).unwrap_or_else(|| {
        if info.knockout.unwrap_or(false) {
            Knockout::Shallow
        } else {
            Knockout::None
        }
    });
    node.blending.blend_interior_effects_as_group = info.blend_interior_elements.unwrap_or(true);
    node.blending.blend_clipped_layers_as_group = info.blend_clippend_elements.unwrap_or(true);
    node.blending.transparency_shapes_layer = info.transparency_shapes_layer.unwrap_or(true);
    // Transparency protection is not a whole-layer lock.
    node.locked = false;
    let background = parent.is_none()
        && matches!(node.kind, NodeKind::Raster { .. })
        && association.is_some_and(|a| a.is_background(l));
    let id = add(doc, node, parent)?;
    if background {
        doc.psd_background = Some(id);
    }
    Ok(id)
}

/// Evidence used for this file's current appearance, never a recovered
/// PSD-authoring document gamma preference or a promise about future edits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportProfileDecision {
    NotCompared,
    UniquePhotoshopSrgbV1,
    LegacyMatch {
        ambiguous: bool,
    },
    /// Same current appearance across supported candidates; no merged-alpha
    /// reference was used. This is not a statement about future edits.
    SameCurrentAppearance,
    SavedAppearance,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadReport {
    pub profile_decision: ImportProfileDecision,
    pub background_preserved: bool,
}

/// Open a PSD file as a layered document.
pub fn read(path: &Path) -> Result<Document> {
    read_with_report(path).map(|(doc, _)| doc)
}

/// Open a PSD and disclose bounded current-appearance profile evidence.
pub fn read_with_report(path: &Path) -> Result<(Document, ReadReport)> {
    read_bytes_with_report(&std::fs::read(path)?)
}

fn read_bytes_with_report(bytes: &[u8]) -> Result<(Document, ReadReport)> {
    if bytes.get(..4) == Some(b"8BPS") && bytes.get(24..26) == Some(&[0, 4]) {
        return read_cmyk_composite(bytes).map(|doc| {
            (
                doc,
                ReadReport {
                    profile_decision: ImportProfileDecision::SavedAppearance,
                    background_preserved: false,
                },
            )
        });
    }
    let opts = ReadOptions {
        skip_thumbnail: Some(true),
        skip_composite_image_data: Some(false),
        skip_linked_files_data: Some(true),
        use_image_data: Some(true),
        ..Default::default()
    };
    let mask_reason = mask_guard::unsupported_mask_reason(bytes)
        .map_err(|error| IoError::Unsupported(error.to_string()))?;
    let raw =
        mask_guard::raw_metadata(bytes).map_err(|error| IoError::Unsupported(error.to_string()))?;
    let (sources, smart_unsupported) = match smart_objects::inspect(bytes) {
        Ok(sources) => (sources, false),
        Err(smart_objects::SmartError::Unsupported(_)) => (Default::default(), true),
        Err(error @ smart_objects::SmartError::Malformed(_)) => {
            return Err(IoError::Unsupported(error.to_string()));
        }
        Err(error @ smart_objects::SmartError::Unavailable(_)) => {
            return Err(std::io::Error::other(error.to_string()).into());
        }
    };
    let vector_copy = mask_guard::vector_decoder_copy(bytes)
        .map_err(|error| IoError::Unsupported(error.to_string()))?;
    match ag_psd::read_psd(vector_copy.as_deref().unwrap_or(bytes), &opts) {
        Ok(psd) => {
            let association = profile::associate(&raw, &psd);
            // Deep may pass only the independent guard scan that retains every
            // other reason; never erase a selected "Deep" string in place.
            let guarded = if association.is_some() {
                raw.reason
            } else {
                mask_reason
            };
            let mut force_appearance =
                guarded.is_some() || smart_unsupported || psd_uses_saved_image(&psd, &sources);
            let mut decision = ImportProfileDecision::NotCompared;
            let real_merged = raw.real_merged
                && psd
                    .image_resources
                    .as_ref()
                    .and_then(|resources| resources.version_info.as_ref())
                    .is_some_and(|version| version.has_real_merged_data);
            let mut doc =
                from_psd_with_metadata(&psd, force_appearance, &sources, association.as_ref())?;
            if !force_appearance {
                let reference = if real_merged
                    && association.is_some()
                    && profile::selectable(&raw, &doc)
                {
                    // Validate original bytes, not the vector alias decoding copy.
                    mask_guard::validate_saved_composite(bytes)
                        .map_err(|error| IoError::Unsupported(error.to_string()))?;
                    let saved = psd
                        .image_data
                        .as_ref()
                        .or(psd.canvas.as_ref())
                        .filter(|px| (px.width, px.height) == (doc.width, doc.height))
                        .filter(|px| px.data.len() == doc.width as usize * doc.height as usize * 4)
                        .ok_or_else(|| {
                            IoError::Unsupported(
                                "PSD merged reference dimensions or samples are invalid".into(),
                            )
                        })?;
                    Some(saved.data.as_slice())
                } else {
                    None
                };
                decision =
                    profile::candidate_decision(&raw, association.as_ref(), &mut doc, reference);
                force_appearance = decision == ImportProfileDecision::SavedAppearance;
            }
            if force_appearance {
                mask_guard::validate_saved_composite(bytes)
                    .map_err(|error| IoError::Unsupported(error.to_string()))?;
                doc = from_psd_with_fallback(&psd, true)?;
                decision = ImportProfileDecision::SavedAppearance;
            }
            let report = ReadReport {
                profile_decision: decision,
                background_preserved: doc.psd_background.is_some(),
            };
            Ok((doc, report))
        }
        Err(layer_error) => {
            // A real-world PSD mask layout is ambiguous to ag-psd 0.3's
            // length heuristic. Recover only the existing saved composite,
            // with strict framing and alpha/spot-channel exclusions.
            let saved = mask_guard::saved_composite_only(bytes).map_err(|recovery_error| {
                IoError::Unsupported(format!(
                    "PSD layer data could not be decoded ({layer_error:?}); {recovery_error}"
                ))
            })?;
            let psd = ag_psd::read_psd(&saved, &opts).map_err(|error| {
                IoError::Unsupported(format!("PSD saved appearance: {error:?}"))
            })?;
            from_psd_with_fallback(&psd, true).map(|doc| {
                (
                    doc,
                    ReadReport {
                        profile_decision: ImportProfileDecision::SavedAppearance,
                        background_preserved: false,
                    },
                )
            })
        }
    }
}

/// CMYK blending cannot be reconstructed by compositing converted RGB layers.
/// Import the saved merged appearance instead. PSD stores inverted CMYK samples;
/// use an embedded CMYK ICC profile when available, otherwise generic conversion.
fn read_cmyk_composite(bytes: &[u8]) -> Result<Document> {
    fn invalid() -> IoError {
        IoError::Unsupported("CMYK PSD: truncated or invalid image data".into())
    }
    fn take<'a>(input: &mut &'a [u8], len: usize) -> Result<&'a [u8]> {
        let (head, tail) = input.split_at_checked(len).ok_or_else(invalid)?;
        *input = tail;
        Ok(head)
    }
    fn number(input: &mut &[u8], len: usize) -> Result<usize> {
        let value = take(input, len)?
            .iter()
            .fold(0u64, |n, b| (n << 8) | u64::from(*b));
        usize::try_from(value).map_err(|_| invalid())
    }
    let mut input = bytes;
    if take(&mut input, 4)? != b"8BPS" {
        return Err(invalid());
    }
    let version = number(&mut input, 2)?;
    if !matches!(version, 1 | 2) {
        return Err(invalid());
    }
    take(&mut input, 6)?;
    let channels = number(&mut input, 2)?;
    let height = number(&mut input, 4)? as u32;
    let width = number(&mut input, 4)? as u32;
    crate::import::check_size(width, height)?;
    let depth = number(&mut input, 2)?;
    if number(&mut input, 2)? != 4 || depth != 8 || channels != 4 {
        return Err(IoError::Unsupported(
            "CMYK PSD opening requires 8-bit CMYK with four channels (no extra alpha or spot channels)".into(),
        ));
    }
    let color_data_len = number(&mut input, 4)?;
    take(&mut input, color_data_len)?;
    let resources_len = number(&mut input, 4)?;
    let mut resources = take(&mut input, resources_len)?;
    let mut icc = None;
    while !resources.is_empty() {
        if take(&mut resources, 4)? != b"8BIM" {
            return Err(invalid());
        }
        let id = number(&mut resources, 2)?;
        let name_len = number(&mut resources, 1)?;
        take(&mut resources, name_len)?;
        if (name_len + 1) % 2 != 0 {
            take(&mut resources, 1)?;
        }
        let data_len = number(&mut resources, 4)?;
        let data = take(&mut resources, data_len)?;
        if id == 1039 {
            icc = Some(data);
        }
        if data_len % 2 != 0 {
            take(&mut resources, 1)?;
        }
    }
    let layer_len = number(&mut input, if version == 2 { 8 } else { 4 })?;
    take(&mut input, layer_len)?;
    let compression = number(&mut input, 2)?;
    let pixels = width as usize * height as usize;
    let planes = match compression {
        0 => take(&mut input, pixels * 4)?.to_vec(),
        1 => {
            let rows = height as usize * 4;
            let row_lengths = (0..rows)
                .map(|_| number(&mut input, if version == 2 { 4 } else { 2 }))
                .collect::<Result<Vec<_>>>()?;
            let mut output = Vec::with_capacity(pixels * 4);
            for len in row_lengths {
                let mut row = take(&mut input, len)?;
                let end = output.len() + width as usize;
                while !row.is_empty() {
                    let count = take(&mut row, 1)?[0] as i8;
                    match count {
                        0..=127 => {
                            let len = count as usize + 1;
                            if output.len() + len > end {
                                return Err(invalid());
                            }
                            output.extend_from_slice(take(&mut row, len)?);
                        }
                        -127..=-1 => {
                            let len = (1 - i16::from(count)) as usize;
                            if output.len() + len > end {
                                return Err(invalid());
                            }
                            let value = take(&mut row, 1)?[0];
                            output.resize(output.len() + len, value);
                        }
                        -128 => {}
                    }
                }
                if output.len() != end {
                    return Err(invalid());
                }
            }
            output
        }
        _ => {
            return Err(IoError::Unsupported(
                "CMYK PSD opening currently supports raw and RLE compression".into(),
            ));
        }
    };
    let mut cmyk = Vec::with_capacity(pixels * 4);
    for i in 0..pixels {
        for channel in 0..4 {
            cmyk.push(255 - planes[pixels * channel + i]);
        }
    }
    let rgba = crate::icc::cmyk_to_srgba8(icc, &cmyk);
    let mut doc = Document::new(width, height);
    doc.blend_space = emulsion_raster::blend::BlendSpace::Srgb;
    doc.source_depth = 8;
    add(
        &mut doc,
        Node::raster(
            0,
            "CMYK PSD appearance (converted to RGB, flattened)",
            Arc::new(Raster::from_srgba8(width, height, &rgba)),
            Placement::default(),
        ),
        None,
    )?;
    doc.validate()
        .map_err(|e| IoError::Unsupported(format!("PSD: {e}")))?;
    Ok(doc)
}

/// Build a document from a parsed PSD, rejecting sizes and pixel blocks
/// that do not match rather than trusting the file.
#[cfg(test)]
fn from_psd(psd: &Psd) -> Result<Document> {
    from_psd_with_fallback(psd, false)
}

fn layers_need_composite(
    layers: &[Layer],
    rgb: bool,
    sources: &smart_objects::ImportSources,
) -> bool {
    layers.iter().any(|l| {
        blend_metadata::import(&l.additional_info).is_none()
            || (!rgb
                && l.additional_info
                    .blending_ranges
                    .as_ref()
                    .is_some_and(|r| !r.ranges.is_empty()))
            || sources.layer_kind(l).is_err()
            || l.additional_info.adjustment.is_some()
            || l.additional_info.effects.is_some()
            // Public cross-application fixtures disagree on clipping a group
            // itself. Do not substitute native group-clip semantics silently.
            || (l.children.is_some() && l.clipping == Some(true))
            || !vector_mask::can_import(l)
            || l.additional_info.vector_fill.is_some()
            || l.additional_info.vector_stroke.is_some()
            || l.additional_info.real_mask.is_some()
            || l.additional_info.mask.as_ref().is_some_and(|m| {
                m.from_vector_data == Some(true)
                    || mask_properties_in(m).is_none()
                    || (m.image_data.is_none() && m.canvas.is_none())
            })
            || l.children
                .as_deref()
                .is_some_and(|children| layers_need_composite(children, rgb, sources))
    })
}

fn psd_uses_saved_image(psd: &Psd, sources: &smart_objects::ImportSources) -> bool {
    psd.children.as_ref().is_none_or(|layers| {
        layers.is_empty()
            || layers_need_composite(
                layers,
                matches!(psd.color_mode, None | Some(ColorMode::Rgb)),
                sources,
            )
    })
}

fn from_psd_with_fallback(psd: &Psd, force_appearance: bool) -> Result<Document> {
    from_psd_with_sources(psd, force_appearance, &Default::default())
}

fn from_psd_with_sources(
    psd: &Psd,
    force_appearance: bool,
    sources: &smart_objects::ImportSources,
) -> Result<Document> {
    from_psd_with_metadata(psd, force_appearance, sources, None)
}

fn from_psd_with_metadata(
    psd: &Psd,
    force_appearance: bool,
    sources: &smart_objects::ImportSources,
    association: Option<&profile::Association>,
) -> Result<Document> {
    let (w, h) = (psd.width as u32, psd.height as u32);
    crate::import::check_size(w, h)?;
    if !matches!(
        psd.color_mode,
        None | Some(ColorMode::Rgb) | Some(ColorMode::Grayscale)
    ) {
        return Err(IoError::Unsupported(format!(
            "PSD colour mode {:?} (only RGB and greyscale open)",
            psd.color_mode
        )));
    }
    let mut doc = Document::new(w, h);
    doc.blend_space = emulsion_raster::blend::BlendSpace::Srgb;
    doc.source_depth = 8;
    let use_composite = force_appearance
        || psd.children.as_deref().is_some_and(|layers| {
            layers_need_composite(
                layers,
                matches!(psd.color_mode, None | Some(ColorMode::Rgb)),
                sources,
            )
        });
    let needs_saved_image = force_appearance || psd_uses_saved_image(psd, sources);
    if needs_saved_image
        && psd
            .image_resources
            .as_ref()
            .and_then(|r| r.version_info.as_ref())
            .is_some_and(|version| !version.has_real_merged_data)
    {
        return Err(IoError::Unsupported(
            "PSD requires a saved merged appearance; save it with Maximize Compatibility enabled in the authoring app".into(),
        ));
    }
    match &psd.children {
        Some(layers) if !layers.is_empty() && !use_composite => {
            add_layers(&mut doc, layers, None, sources, association)?
        }
        _ => {
            // A flat file: the composite is the only picture.
            let px = psd
                .image_data
                .as_ref()
                .or(psd.canvas.as_ref())
                .ok_or_else(|| {
                    IoError::Unsupported("PSD has neither layers nor a composite".into())
                })?;
            crate::import::check_size(px.width, px.height)?;
            if (px.width, px.height) != (w, h) {
                return Err(IoError::Unsupported(
                    "PSD composite dimensions do not match its header".into(),
                ));
            }
            let data = pixel_bytes(px, 4).ok_or_else(|| {
                IoError::Unsupported("PSD composite image data is truncated".into())
            })?;
            let r = Raster::from_srgba8(px.width, px.height, data);
            add(
                &mut doc,
                Node::raster(
                    0,
                    if use_composite {
                        "PSD appearance (unsupported layer features flattened)"
                    } else {
                        "Background"
                    },
                    Arc::new(r),
                    Placement::default(),
                ),
                None,
            )?;
        }
    }
    doc.validate()
        .map_err(|e| IoError::Unsupported(format!("PSD: {e}")))?;
    Ok(doc)
}

/// Render one node by itself in document space (for nodes PSD has
/// no equivalent for, and for transformed rasters).
fn render_alone(doc: &Document, id: NodeId) -> Result<Raster> {
    let mut d = doc.clone();
    d.psd_background = None;
    let keep: std::collections::HashSet<NodeId> = {
        // The node and its ancestors stay visible; everything else hides.
        let mut set = std::collections::HashSet::new();
        let mut cur = Some(id);
        while let Some(c) = cur {
            set.insert(c);
            cur = d.node(c).and_then(|n| n.parent);
        }
        set
    };
    let hide: Vec<NodeId> = d
        .nodes
        .iter()
        .filter(|n| !keep.contains(&n.id) && !is_descendant(&d, n.id, id))
        .map(|n| n.id)
        .collect();
    for n in d.nodes.iter_mut() {
        if keep.contains(&n.id) {
            n.visible = true;
            n.opacity = 1.0;
            n.blend = if n.kind.is_group() {
                BlendMode::PassThrough
            } else {
                BlendMode::Normal
            };
            n.clip_to = None;
            // This layer's blending envelope is emitted as PSD metadata below.
            // Ancestor envelopes are emitted on their own group records. Do not
            // bake Fill/Blend If/channels/knockout and then apply them twice.
            n.blending = Default::default();
            if n.id != id {
                n.mask = None;
                n.vector_mask = None;
            }
        } else if hide.contains(&n.id) {
            n.visible = false;
        }
    }
    Ok(flatten(&d.try_composite_tree()?, 0))
}

fn is_descendant(doc: &Document, node: NodeId, of: NodeId) -> bool {
    let mut cur = doc.node(node).and_then(|n| n.parent);
    while let Some(c) = cur {
        if c == of {
            return true;
        }
        cur = doc.node(c).and_then(|n| n.parent);
    }
    false
}

/// Trim a document-space raster to its opaque bounds; (left, top, pixels).
fn trimmed(r: &Raster) -> (f64, f64, PixelData) {
    let (w, h) = (r.width(), r.height());
    let data = r.to_srgba8();
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0u32, 0u32);
    for y in 0..h {
        for x in 0..w {
            if data[((y * w + x) * 4 + 3) as usize] != 0 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x1 <= x0 || y1 <= y0 {
        return (
            0.0,
            0.0,
            PixelData {
                width: 1,
                height: 1,
                data: vec![0; 4],
            },
        );
    }
    let (tw, th) = (x1 - x0, y1 - y0);
    let mut out = Vec::with_capacity((tw * th * 4) as usize);
    for y in y0..y1 {
        let s = ((y * w + x0) * 4) as usize;
        out.extend_from_slice(&data[s..s + (tw * 4) as usize]);
    }
    (
        x0 as f64,
        y0 as f64,
        PixelData {
            width: tw,
            height: th,
            data: out,
        },
    )
}

fn mask_out(mask: &Mask, x: f64, y: f64, disabled: bool) -> LayerMaskData {
    LayerMaskData {
        left: Some(x),
        top: Some(y),
        right: Some(x + mask.width() as f64),
        bottom: Some(y + mask.height() as f64),
        default_color: Some(mask.fill() as f64),
        disabled: Some(disabled),
        image_data: Some(PixelData {
            width: mask.width(),
            height: mask.height(),
            data: mask
                .to_gray8()
                .into_iter()
                .flat_map(|v| [v, v, v, 255])
                .collect(),
        }),
        ..Default::default()
    }
}

/// Standard PSD raster-mask rectangles describe translated pixel grids, not
/// arbitrary affines. Keep the raw grid and editable parameters only when no
/// resampling is needed, and when every bound fits the signed 32-bit record.
fn editable_mask_origin(node: &Node, x: f64, y: f64) -> Option<(f64, f64)> {
    let mask = node.mask.as_ref()?;
    let [a, b, c, d, tx, ty] = node.mask_transform.affine()?.to_cols_array();
    if [a, b, c, d] != [1.0, 0.0, 0.0, 1.0] || !matches!(mask.fill(), 0 | 255) {
        return None;
    }
    let (left, top) = (x + tx, y + ty);
    let bounds = [
        left,
        top,
        left + mask.width() as f64,
        top + mask.height() as f64,
    ];
    bounds
        .iter()
        .all(|v| {
            v.is_finite() && v.fract() == 0.0 && *v >= i32::MIN as f64 && *v <= i32::MAX as f64
        })
        .then_some((left, top))
}

fn layer_mask_out(doc: &Document, node: &Node, x: f64, y: f64) -> Result<Option<LayerMaskData>> {
    let Some(raw_mask) = node.mask.as_ref() else {
        return Ok(None);
    };
    let mut mask = if let Some((left, top)) = editable_mask_origin(node, x, y) {
        let mut mask = mask_out(raw_mask, left, top, !node.mask_enabled);
        let properties = node.mask_properties;
        if properties.density != 1.0 {
            mask.user_mask_density = Some(properties.density as f64);
        }
        if properties.feather != 0.0 {
            mask.user_mask_feather = Some(properties.feather as f64);
        }
        mask
    } else {
        // Affine sampling and intrinsic feather do not commute. Bake both
        // together, leaving default PSD parameters so they are not applied twice.
        let coverage = doc.mask_for_inspection(node)?.ok_or_else(|| {
            IoError::Unsupported("PSD raster mask has no inspection coverage".into())
        })?;
        let mut mask = mask_out(&coverage, x, y, !node.mask_enabled);
        // The baked plane covers the complete output layer (or document for a
        // group). Outside it there is no exported content, so use a portable
        // binary background rather than a nonstandard derived gray default.
        if !matches!(mask.default_color, Some(0.0 | 255.0)) {
            mask.default_color = Some(0.0);
        }
        mask
    };
    mask.position_relative_to_layer = Some(!node.mask_linked);
    Ok(Some(mask))
}

/// Some PSD layers remain editable while their unsupported mask affine must
/// be baked. Callers can disclose that loss separately from a whole-document
/// appearance fallback. Native export never changes these descriptors.
pub fn has_baked_raster_masks(doc: &Document) -> bool {
    doc.nodes
        .iter()
        .any(|node| node.mask.is_some() && !has_editable_raster_mask(node))
}

fn has_editable_raster_mask(node: &Node) -> bool {
    match &node.kind {
        NodeKind::Group { .. } => editable_mask_origin(node, 0.0, 0.0).is_some(),
        NodeKind::Raster { placement, .. } if raster_placement_is_translated(placement) => {
            editable_mask_origin(node, placement.x, placement.y).is_some()
        }
        NodeKind::Smart { placement, .. } if smart_objects::can_export(node) => {
            placement.legacy().is_some_and(|placement| {
                editable_mask_origin(node, placement.x, placement.y).is_some()
            })
        }
        _ => false,
    }
}

/// PSD's independently documented Density byte is the sole representation
/// adjustment allowed in the appearance reference. Only serialized independent
/// parameters are rounded: baked masks, pixels, Opacity, Fill and geometry keep
/// their strict guards. Call only after the original document passes admission,
/// so removing a rounded-to-one parameter cannot admit an unsupported header.
fn mask_density_export_reference(doc: &Document) -> Option<(Document, usize)> {
    fn round_density(density: &mut f32) -> usize {
        let rounded = ((f64::from(*density) * 255.0).round() / 255.0) as f32;
        if rounded == *density {
            return 0;
        }
        *density = rounded;
        1
    }
    let mut reference = doc.clone();
    let mut rounded = 0;
    for (source, node) in doc.nodes.iter().zip(&mut reference.nodes) {
        if has_editable_raster_mask(source) {
            rounded += round_density(&mut node.mask_properties.density);
        }
        if source.mask.is_some()
            && vector_mask::export(doc, source).is_some()
            && let Some(vector) = &mut node.vector_mask
        {
            rounded += round_density(&mut vector.properties.density);
        }
    }
    (rounded != 0).then_some((reference, rounded))
}

fn raster_placement_is_translated(placement: &Placement) -> bool {
    placement.scale_x == 1.0
        && placement.scale_y == 1.0
        && placement.rotation == 0.0
        && !placement.flip_x
        && !placement.flip_y
        && placement.x.fract() == 0.0
        && placement.y.fract() == 0.0
}

/// PSD clipping uses contiguous runs over the nearest unclipped base. Emulsion
/// also allows arbitrary lower siblings; these and backdrop-dependent effects
/// need an explicit merged appearance instead of a misleading layered export.
/// This cheap structural gate does not render. `write_with_report` additionally
/// checks current blend-space appearance and returns the actual export decision.
pub fn needs_appearance_fallback(doc: &Document) -> bool {
    fn unsupported_clips(doc: &Document, parent: Option<NodeId>) -> bool {
        let mut base = None;
        for id in doc.children(parent) {
            let node = doc.node(id).expect("existing child");
            if let Some(target) = node.clip_to {
                if node.kind.is_group() || base != Some(target) {
                    return true;
                }
            } else {
                base = Some(id);
            }
            if node.kind.is_group() && unsupported_clips(doc, Some(id)) {
                return true;
            }
        }
        false
    }
    !profile::supported_envelopes(doc)
        || !profile::background_exportable(doc)
        || doc.nodes.iter().any(|n| {
            // Unsupported topology/parameters must retain the combined result,
            // including currently hidden or disabled editable components.
            n.has_projective_metadata()
            || (n.vector_mask.is_some() && vector_mask::export(doc, n).is_none())
            // PSD specifies binary outside coverage. Some readers interpret
            // every non-white byte as black, so a native gray fill needs the
            // merged appearance rather than a nonportable mask record.
            || n.mask.as_ref().is_some_and(|m| !matches!(m.fill(), 0 | 255))
            || (blend_metadata::needs_appearance(&n.blending)
                && !(n.blending.knockout == Knockout::Deep
                    && doc.blend_space == emulsion_raster::blend::BlendSpace::PhotoshopSrgbV1))
            || (matches!(n.kind, NodeKind::Smart { .. }) && !smart_objects::can_export(n))
            || crate::ora::has_filter_mask(n)
            || matches!(n.kind, NodeKind::Adjust(_))
            || !n.styles.is_empty()
            || n.blending.layer_mask_hides_effects
        })
        || unsupported_clips(doc, None)
}

#[cfg(test)]
fn layer_for(doc: &Document, n: &Node) -> Layer {
    layer_for_sources(doc, n, &Default::default()).expect("valid test export layer")
}

fn layer_for_sources(
    doc: &Document,
    n: &Node,
    sources: &smart_objects::ExportSources,
) -> Result<Layer> {
    n.require_affine_capability("Layered PSD export")?;
    let mut l = Layer {
        blend_mode: Some(blend_out(n.blend)),
        opacity: Some(n.opacity as f64),
        hidden: Some(!n.visible),
        clipping: Some(n.clip_to.is_some()),
        ..Default::default()
    };
    l.additional_info.name = Some(n.name.clone());
    l.additional_info.fill_opacity = Some(n.blending.fill_opacity as f64);
    let mut restrictions: Vec<f64> = n
        .blending
        .channels
        .iter()
        .enumerate()
        .filter_map(|(i, enabled)| (!enabled).then_some(i as f64))
        .collect();
    // ag-psd 0.3's reader intentionally leaves the final 4-byte word for
    // padding. Repeating the final restriction is harmless to PSD readers and
    // makes files produced here round-trip through that reader faithfully.
    if let Some(last) = restrictions.last().copied() {
        restrictions.push(last);
    }
    l.additional_info.channel_blending_restrictions = Some(restrictions);
    l.additional_info.blending_ranges = blend_metadata::export(n.blending.blend_if);
    l.additional_info.blend_interior_elements = Some(n.blending.blend_interior_effects_as_group);
    l.additional_info.blend_clippend_elements = Some(n.blending.blend_clipped_layers_as_group);
    l.additional_info.transparency_shapes_layer = Some(n.blending.transparency_shapes_layer);
    l.additional_info.knockout = Some(n.blending.knockout != Knockout::None);
    match &n.kind {
        NodeKind::Group { .. } => {
            let kids: Vec<Layer> = doc
                .children(Some(n.id))
                .into_iter()
                .filter_map(|id| doc.node(id))
                .map(|c| layer_for_sources(doc, c, sources))
                .collect::<Result<_>>()?;
            l.children = Some(kids);
            l.additional_info.mask = layer_mask_out(doc, n, 0.0, 0.0)?;
        }
        NodeKind::Raster { raster, placement } if raster_placement_is_translated(placement) => {
            l.left = Some(placement.x.round());
            l.top = Some(placement.y.round());
            l.right = Some(placement.x.round() + raster.width() as f64);
            l.bottom = Some(placement.y.round() + raster.height() as f64);
            l.image_data = Some(PixelData {
                width: raster.width(),
                height: raster.height(),
                data: raster.to_srgba8(),
            });
            l.additional_info.mask = layer_mask_out(doc, n, placement.x, placement.y)?;
        }
        NodeKind::Smart { placement, .. } if sources.apply_layer(n, &mut l) => {
            let placement = placement.require_legacy("Layered PSD export")?;
            l.additional_info.mask = layer_mask_out(doc, n, placement.x, placement.y)?;
        }
        _ => {
            // Rasterise in place: masks and transforms are baked in.
            let (x, y, px) = trimmed(&render_alone(doc, n.id)?);
            l.left = Some(x);
            l.top = Some(y);
            l.right = Some(x + px.width as f64);
            l.bottom = Some(y + px.height as f64);
            l.image_data = Some(px);
        }
    }
    l.additional_info.vector_mask = vector_mask::export(doc, n);
    if l.additional_info.vector_mask.is_some() {
        vector_mask::add_parameters(n, &mut l.additional_info.mask);
    }
    Ok(l)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppearanceFallback {
    UnsupportedFeatures,
    /// Intended pixels differ from PSD layer inputs reconstructed under the
    /// PhotoshopSrgbV1 contract. Includes Normal alpha/stop semantics and PSD
    /// sample/envelope quantization other than the documented Density byte.
    /// No portable PSD field stores our profile.
    BlendSpaceDifference,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WriteReport {
    pub appearance_fallback: Option<AppearanceFallback>,
    pub baked_raster_masks: bool,
    /// Independent raster/vector Density values rounded to PSD's nearest byte,
    /// including hidden/disabled parameters. The layered merged preview uses
    /// that same representation; native state is unchanged. Zero when flattened.
    pub rounded_mask_densities: usize,
}

/// Write the document as a layered PSD (PSB above 30 000 px).
pub fn write(doc: &Document, path: &Path) -> Result<()> {
    write_with_report(doc, path).map(|_| ())
}

/// Plan and write in one background job. The report reflects the actual write,
/// including a current-pixel blend-space check, without a second UI-thread render.
/// This is an 8-bit appearance guard, not a claim of third-party renderer parity
/// or equivalent future edits in two different document blending conventions.
pub fn write_with_report(doc: &Document, path: &Path) -> Result<WriteReport> {
    write_with_source_preparer(doc, path, smart_objects::prepare_export)
}

// A private preparation seam keeps transient identifier failures testable
// without process-global randomness overrides or a second serialization path.
fn write_with_source_preparer(
    doc: &Document,
    path: &Path,
    prepare_sources: fn(
        &Document,
    ) -> std::result::Result<
        smart_objects::ExportSources,
        smart_objects::SmartError,
    >,
) -> Result<WriteReport> {
    doc.validate()?;
    let native_flat = profile::try_render_cpu(doc)?;
    let mut appearance_fallback =
        needs_appearance_fallback(doc).then_some(AppearanceFallback::UnsupportedFeatures);
    if appearance_fallback.is_none() && !profile::within_budget(doc) {
        appearance_fallback = Some(AppearanceFallback::UnsupportedFeatures);
    }
    let density_reference = appearance_fallback
        .is_none()
        .then(|| mask_density_export_reference(doc))
        .flatten();
    let reference = density_reference.as_ref().map_or(doc, |(doc, _)| doc);
    let rounded_mask_densities = density_reference.as_ref().map_or(0, |(_, count)| *count);
    let flat = if rounded_mask_densities == 0 {
        std::borrow::Cow::Borrowed(native_flat.as_slice())
    } else {
        std::borrow::Cow::Owned(profile::try_render_cpu(reference)?)
    };
    if appearance_fallback.is_none()
        && reference.blend_space != emulsion_raster::blend::BlendSpace::PhotoshopSrgbV1
    {
        let mut compatible = reference.clone();
        compatible.blend_space = emulsion_raster::blend::BlendSpace::PhotoshopSrgbV1;
        if profile::try_render_cpu(&compatible)?.as_slice() != flat.as_ref() {
            appearance_fallback = Some(AppearanceFallback::BlendSpaceDifference);
        }
    }
    if appearance_fallback.is_none() && profile::requires_same_current(reference) {
        let mut legacy = reference.clone();
        legacy.blend_space = emulsion_raster::blend::BlendSpace::Srgb;
        let mut compatible = reference.clone();
        compatible.blend_space = emulsion_raster::blend::BlendSpace::PhotoshopSrgbV1;
        if profile::try_render_cpu(&legacy)? != profile::try_render_cpu(&compatible)? {
            appearance_fallback = Some(AppearanceFallback::BlendSpaceDifference);
        }
    }
    let sources = if appearance_fallback.is_none() {
        match prepare_sources(doc) {
            Ok(sources) => sources,
            Err(error @ smart_objects::SmartError::Unavailable(_)) => {
                return Err(std::io::Error::other(error.to_string()).into());
            }
            Err(_) => {
                appearance_fallback = Some(AppearanceFallback::UnsupportedFeatures);
                Default::default()
            }
        }
    } else {
        Default::default()
    };
    let mut bytes = if appearance_fallback.is_none() {
        encode_document(reference, path, &flat, None, &sources)?
    } else {
        encode_document(doc, path, &native_flat, appearance_fallback, &sources)?
    };
    if appearance_fallback.is_none()
        && !doc.nodes.is_empty()
        && !profile::emitted_matches(&bytes, &flat)?
    {
        // Compare actual 8-bit encoded inputs too: native 0.5 opacity becomes
        // 128/255 on disk, and source samples are quantized independently. Only
        // the standard Density byte was normalized in the reference above.
        // Reporting a layered success before this check would be misleading.
        appearance_fallback = Some(AppearanceFallback::BlendSpaceDifference);
        bytes = encode_document(
            doc,
            path,
            &native_flat,
            appearance_fallback,
            &Default::default(),
        )?;
    }
    let report = WriteReport {
        appearance_fallback,
        baked_raster_masks: appearance_fallback.is_none() && has_baked_raster_masks(doc),
        rounded_mask_densities: if appearance_fallback.is_none() {
            rounded_mask_densities
        } else {
            0
        },
    };
    write_atomic(path, |f| {
        f.write_all(&bytes)?;
        Ok(())
    })?;
    Ok(report)
}

fn encode_document(
    doc: &Document,
    path: &Path,
    flat: &[u8],
    appearance_fallback: Option<AppearanceFallback>,
    sources: &smart_objects::ExportSources,
) -> Result<Vec<u8>> {
    let mut children: Vec<Layer> = if appearance_fallback.is_some() || doc.nodes.is_empty() {
        let mut layer = Layer {
            left: Some(0.0),
            top: Some(0.0),
            right: Some(doc.width as f64),
            bottom: Some(doc.height as f64),
            image_data: Some(PixelData {
                width: doc.width,
                height: doc.height,
                data: flat.to_vec(),
            }),
            ..Default::default()
        };
        layer.additional_info.name = Some(
            if doc.nodes.is_empty() {
                "Empty document"
            } else {
                "Emulsion appearance (unsupported features flattened)"
            }
            .into(),
        );
        vec![layer]
    } else {
        doc.children(None)
            .into_iter()
            .filter_map(|id| doc.node(id))
            .map(|n| layer_for_sources(doc, n, sources))
            .collect::<Result<_>>()?
    };
    let export_metadata = if appearance_fallback.is_none() && !doc.nodes.is_empty() {
        Some(profile::prepare_layers(doc, &mut children)?)
    } else {
        children[0].additional_info.id = Some(1.0);
        None
    };
    let psd = Psd {
        image_resources: Some(ag_psd::psd::ImageResources {
            version_info: Some(ag_psd::psd::VersionInfo {
                has_real_merged_data: true,
                writer_name: "Emulsion".into(),
                reader_name: "Emulsion".into(),
                file_version: 1.0,
            }),
            ..Default::default()
        }),
        width: doc.width as f64,
        height: doc.height as f64,
        channels: Some(4.0),
        bits_per_channel: Some(8.0),
        color_mode: Some(ColorMode::Rgb),
        children: Some(children),
        image_data: Some(PixelData {
            width: doc.width,
            height: doc.height,
            data: flat.to_vec(),
        }),
        ..Default::default()
    };
    let psb = doc.width > 30_000
        || doc.height > 30_000
        || path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("psb"));
    let opts = WriteOptions {
        // Only the proven explicit bottom-root target gets special encoding.
        // The writer emits alpha on every other layer regardless of opacity.
        no_background: Some(appearance_fallback.is_some() || doc.psd_background.is_none()),
        generate_thumbnail: Some(false),
        trim_image_data: Some(false),
        psb: Some(psb),
        compress: Some(true),
        ..Default::default()
    };
    let bytes = ag_psd::write_psd(&psd, &opts);
    let bytes = if appearance_fallback.is_none() {
        sources
            .insert(bytes)
            .map_err(|error| IoError::Unsupported(error.to_string()))?
    } else {
        bytes
    };
    let bytes = if let Some(metadata) = export_metadata {
        profile::patch_export(bytes, &psd, &metadata)?
    } else {
        bytes
    };
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmyk_fixture(version: u16, rle: bool) -> Vec<u8> {
        let mut bytes = b"8BPS".to_vec();
        bytes.extend_from_slice(&version.to_be_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&4u16.to_be_bytes());
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend_from_slice(&2u32.to_be_bytes());
        bytes.extend_from_slice(&8u16.to_be_bytes());
        bytes.extend_from_slice(&4u16.to_be_bytes());
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(&vec![0; if version == 2 { 8 } else { 4 }]);
        bytes.extend_from_slice(&u16::from(rle).to_be_bytes());
        // Cyan and 50% black, in PSD's inverted planar CMYK encoding.
        let planes = [[0, 255], [255, 255], [255, 255], [255, 127]];
        if rle {
            for _ in 0..4 {
                if version == 2 {
                    bytes.extend_from_slice(&3u32.to_be_bytes());
                } else {
                    bytes.extend_from_slice(&3u16.to_be_bytes());
                }
            }
        }
        for plane in planes {
            if rle {
                bytes.push(1); // two literal bytes
            }
            bytes.extend_from_slice(&plane);
        }
        bytes
    }

    #[test]
    fn cmyk_psd_and_psb_composites_open_with_correct_ink_polarity() {
        for version in [1, 2] {
            for rle in [false, true] {
                let bytes = cmyk_fixture(version, rle);
                let path = std::env::temp_dir().join(format!(
                    "emulsion-cmyk-{}-{version}-{rle}.psd",
                    std::process::id()
                ));
                std::fs::write(&path, &bytes).unwrap();
                let doc = read(&path).unwrap();
                std::fs::remove_file(path).unwrap();
                assert_eq!(
                    flatten(&doc.composite_tree(), 0).to_srgba8(),
                    [0, 255, 255, 255, 127, 127, 127, 255]
                );
                assert!(doc.nodes[0].name.contains("flattened"));
            }
        }
    }

    #[test]
    fn cmyk_psd_resources_and_packbits_repeats_are_read() {
        let mut bytes = cmyk_fixture(1, true);
        // A three-byte ICC payload exercises both Pascal-name and data padding.
        // An invalid profile uses the documented generic conversion fallback.
        let resource = [b'8', b'B', b'I', b'M', 4, 15, 0, 0, 0, 0, 0, 3, 1, 2, 3, 0];
        bytes[30..34].copy_from_slice(&(resource.len() as u32).to_be_bytes());
        bytes.splice(34..34, resource);
        // Replace the magenta row with a two-pixel repeat plus PackBits no-op.
        bytes[67..70].copy_from_slice(&[255, 255, 128]);
        let doc = read_cmyk_composite(&bytes).unwrap();
        assert_eq!(
            flatten(&doc.composite_tree(), 0).to_srgba8(),
            [0, 255, 255, 255, 127, 127, 127, 255]
        );
    }

    #[test]
    fn cmyk_psd_rejects_truncated_and_unsupported_data() {
        for rle in [false, true] {
            let bytes = cmyk_fixture(1, rle);
            for len in 0..bytes.len() {
                assert!(read_cmyk_composite(&bytes[..len]).is_err(), "length {len}");
            }
        }
        for (offset, value) in [(23, 16), (13, 5), (39, 2)] {
            let mut bytes = cmyk_fixture(1, false);
            bytes[offset] = value;
            assert!(read_cmyk_composite(&bytes).is_err());
        }
        let mut bytes = cmyk_fixture(1, true);
        bytes[48] = 2; // literal run exceeds two-pixel row
        assert!(read_cmyk_composite(&bytes).is_err());
        bytes[48] = 254; // repeated run exceeds two-pixel row
        assert!(read_cmyk_composite(&bytes).is_err());
    }

    fn roundtrip(doc: &Document, name: &str) -> Document {
        let path =
            std::env::temp_dir().join(format!("emulsion-psd-{}-{name}.psd", std::process::id()));
        write(doc, &path).unwrap();
        let restored = read(&path).unwrap();
        let _ = std::fs::remove_file(path);
        restored
    }

    fn persistent_mask_document(enabled: bool, group: bool) -> Document {
        let mut doc = Document::new(11, 9);
        let mut masked = if group {
            Node::group(0, "Mask properties")
        } else {
            Node::raster(
                0,
                "Mask properties",
                Arc::new(Raster::solid(7, 5, [1.0, 0.0, 0.0, 1.0])),
                Placement::at(2.0, 1.0),
            )
        };
        let (w, h) = if group { (11, 9) } else { (7, 5) };
        masked.mask = Some(Arc::new(Mask::from_fn(w, h, 0, |x, y| {
            if x >= 2 && x < w - 2 && y >= 1 && y < h - 1 {
                255
            } else {
                0
            }
        })));
        masked.mask_enabled = enabled;
        masked.mask_transform =
            emulsion_core::Mapping2::Affine(glam::DAffine2::from_cols_array(&[
                1.0, 0.0, 0.0, 1.0, 0.5, 0.0,
            ]));
        masked.mask_properties = emulsion_core::MaskProperties {
            density: 0.5,
            feather: 1.75,
        };
        let id = add(&mut doc, masked, None).unwrap();
        if group {
            add(
                &mut doc,
                Node::raster(
                    0,
                    "Child",
                    Arc::new(Raster::solid(11, 9, [1.0, 0.0, 0.0, 1.0])),
                    Placement::default(),
                ),
                Some(id),
            )
            .unwrap();
        }
        doc
    }

    // Groups are stored after their children, so nodes[0] is not generally
    // the masked node. Select the fixture component independently of ordering.
    fn persistent_mask_node(doc: &Document) -> &Node {
        doc.nodes
            .iter()
            .find(|node| node.mask.is_some())
            .expect("retained mask component")
    }

    #[test]
    fn persistent_mask_properties_psd_bakes_channel_and_preserves_disabled_state() {
        for group in [false, true] {
            for enabled in [true, false] {
                let doc = persistent_mask_document(enabled, group);
                assert!(!needs_appearance_fallback(&doc));
                let node = persistent_mask_node(&doc);
                let raw = node.mask.as_ref().unwrap().clone();
                let expected = doc.mask_for_inspection(node).unwrap().unwrap();
                let layer = layer_for(&doc, node);
                let channel = layer.additional_info.mask.unwrap();
                assert_eq!(channel.disabled, Some(!enabled));
                assert_eq!(
                    channel.default_color,
                    Some(0.0),
                    "baked outside fill is portable"
                );
                assert_eq!(
                    mask_bytes(channel.image_data.as_ref().unwrap()),
                    expected.to_gray8()
                );
                assert_ne!(
                    expected.to_gray8(),
                    raw.to_gray8(),
                    "effective coverage is exported"
                );
                let reopened = roundtrip(&doc, &format!("mask-properties-{group}-{enabled}"));
                assert_eq!(reopened.nodes.len(), doc.nodes.len());
                let restored = persistent_mask_node(&reopened);
                assert_eq!(restored.mask_enabled, enabled);
                assert_eq!(
                    restored.mask_properties,
                    Default::default(),
                    "properties are baked once"
                );
                assert_eq!(restored.mask.as_ref().unwrap().fill(), 0);
                assert_eq!(
                    restored.mask.as_ref().unwrap().to_gray8(),
                    expected.to_gray8()
                );
                assert_eq!(
                    flatten(&reopened.composite_tree(), 0).to_srgba8(),
                    flatten(&doc.composite_tree(), 0).to_srgba8(),
                    "PSD appearance for group={group}, enabled={enabled}",
                );
                assert!(Arc::ptr_eq(
                    &raw,
                    persistent_mask_node(&doc).mask.as_ref().unwrap()
                ));
            }
        }
    }

    #[test]
    fn independent_document_space_mask_properties_psd_uses_current_canvas_grid() {
        for enabled in [true, false] {
            let mut doc = persistent_mask_document(enabled, true);
            let raw = persistent_mask_node(&doc).mask.as_ref().unwrap().clone();
            Command::Crop {
                rect: emulsion_raster::IRect::new(3, 2, 6, 5),
                rotation: 0.0,
            }
            .apply(&mut doc)
            .unwrap();
            assert!(Arc::ptr_eq(
                &raw,
                persistent_mask_node(&doc).mask.as_ref().unwrap()
            ));
            assert_eq!((raw.width(), raw.height()), (11, 9));
            let effective = doc
                .mask_for_inspection(persistent_mask_node(&doc))
                .unwrap()
                .unwrap();
            assert_eq!((effective.width(), effective.height()), (6, 5));
            let layer = layer_for(&doc, persistent_mask_node(&doc));
            let mask = layer.additional_info.mask.unwrap();
            let pixels = mask.image_data.as_ref().unwrap();
            assert_eq!((pixels.width, pixels.height), (6, 5));
            assert_eq!(mask_bytes(pixels), effective.to_gray8());
            assert_eq!(mask.disabled, Some(!enabled));
            let reopened = roundtrip(&doc, &format!("independent-mask-extents-{enabled}"));
            assert_eq!(persistent_mask_node(&reopened).mask_enabled, enabled);
            assert_eq!(
                flatten(&reopened.composite_tree(), 0).to_srgba8(),
                flatten(&doc.composite_tree(), 0).to_srgba8(),
            );
        }
    }

    #[test]
    fn persistent_mask_properties_psd_baked_placement_keeps_enabled_and_disabled_appearance() {
        for enabled in [true, false] {
            let mut doc = persistent_mask_document(enabled, false);
            let NodeKind::Raster { placement, .. } = &mut doc.nodes[0].kind else {
                panic!("raster");
            };
            placement.scale_x = 1.5;
            placement.scale_y = 1.25;
            placement.rotation = 0.25;
            let expected = flatten(&doc.composite_tree(), 0).to_srgba8();
            let reopened = roundtrip(&doc, &format!("mask-properties-placement-{enabled}"));
            assert_eq!(reopened.nodes.len(), 1);
            assert!(
                reopened.nodes[0].mask.is_none(),
                "transformed layer appearance is baked"
            );
            assert_eq!(flatten(&reopened.composite_tree(), 0).to_srgba8(), expected);
        }
    }

    #[test]
    fn backdrop_adjustment_export_uses_explicit_appearance_fallback() {
        let mut doc = Document::new(8, 8);
        add(
            &mut doc,
            Node::raster(
                0,
                "Gray",
                Arc::new(Raster::solid(8, 8, [0.125, 0.125, 0.125, 1.0])),
                Placement::default(),
            ),
            None,
        )
        .unwrap();
        add(
            &mut doc,
            Node::adjust(
                0,
                emulsion_raster::Adjustment::Exposure {
                    exposure: 2.0,
                    offset: 0.0,
                    gamma: 1.0,
                },
            ),
            None,
        )
        .unwrap();
        let restored = roundtrip(&doc, "adjustment-fidelity");
        assert_eq!(
            flatten(&doc.composite_tree(), 0).to_srgba8(),
            flatten(&restored.composite_tree(), 0).to_srgba8()
        );
        assert_eq!(restored.nodes.len(), 1);
        assert!(restored.nodes[0].name.contains("flattened"));
    }

    #[test]
    fn group_mask_and_contiguous_clip_remain_layered_and_keep_appearance() {
        let mut doc = Document::new(8, 8);
        let mut group = Node::group(0, "Masked group");
        group.mask = Some(Arc::new(Mask::from_fn(8, 8, 0, |_, y| {
            if y < 4 { 255 } else { 0 }
        })));
        let group = add(&mut doc, group, None).unwrap();
        let base = add(
            &mut doc,
            Node::raster(
                0,
                "Base",
                Arc::new(Raster::solid(4, 8, [1.0, 0.0, 0.0, 1.0])),
                Placement::default(),
            ),
            Some(group),
        )
        .unwrap();
        let top = add(
            &mut doc,
            Node::raster(
                0,
                "Clipped",
                Arc::new(Raster::solid(8, 8, [0.0, 0.0, 1.0, 1.0])),
                Placement::default(),
            ),
            Some(group),
        )
        .unwrap();
        Command::SetClip {
            id: top,
            clip_to: Some(base),
        }
        .apply(&mut doc)
        .unwrap();
        let restored = roundtrip(&doc, "group-clip-fidelity");
        assert_eq!(restored.nodes.len(), 3);
        assert!(
            restored
                .nodes
                .iter()
                .any(|n| n.kind.is_group() && n.mask.is_some())
        );
        assert!(restored.nodes.iter().any(|n| n.clip_to.is_some()));
        assert_eq!(
            flatten(&doc.composite_tree(), 0).to_srgba8(),
            flatten(&restored.composite_tree(), 0).to_srgba8()
        );
    }

    #[test]
    fn fractional_raster_placement_is_baked_without_snapping() {
        let mut doc = Document::new(12, 12);
        add(
            &mut doc,
            Node::raster(
                0,
                "Fractional",
                Arc::new(Raster::solid(4, 4, [1.0, 0.0, 0.0, 1.0])),
                Placement::at(2.5, 2.5),
            ),
            None,
        )
        .unwrap();
        let restored = roundtrip(&doc, "fractional-placement");
        assert_eq!(
            flatten(&doc.composite_tree(), 0).to_srgba8(),
            flatten(&restored.composite_tree(), 0).to_srgba8()
        );
    }

    #[test]
    fn advanced_blending_metadata_encoding_does_not_claim_renderer_eligibility() {
        let mut doc = Document::new(4, 4);
        let id = add(
            &mut doc,
            Node::raster(
                0,
                "Advanced",
                Arc::new(Raster::solid(4, 4, [0.8, 0.2, 0.1, 1.0])),
                Placement::default(),
            ),
            None,
        )
        .unwrap();
        let layer = doc.node_mut(id).unwrap();
        layer.blend = BlendMode::LinearDodge;
        layer.blending.fill_opacity = 0.4;
        layer.blending.channels = [true, false, true];
        layer.blending.knockout = Knockout::Shallow;
        layer.blending.blend_interior_effects_as_group = false;
        layer.blending.blend_clipped_layers_as_group = false;
        layer.blending.transparency_shapes_layer = false;
        layer.blending.blend_if.source = BlendRange {
            black: 0.1,
            black_fade: 0.2,
            white_fade: 0.8,
            white: 0.9,
        };
        assert!(needs_appearance_fallback(&doc));
        let appearance = roundtrip(&doc, "advanced-blending-metadata");
        assert_eq!(appearance.nodes.len(), 1);
        assert!(appearance.nodes[0].name.contains("appearance"));
        assert_eq!(profile::render_cpu(&appearance), profile::render_cpu(&doc));
        // Direct encoder/decoder transport remains covered independently.
        let psd = Psd {
            width: 4.0,
            height: 4.0,
            children: Some(vec![layer_for(&doc, doc.node(id).unwrap())]),
            ..Default::default()
        };
        let bytes = ag_psd::write_psd(
            &psd,
            &WriteOptions {
                no_background: Some(true),
                ..Default::default()
            },
        );
        let restored =
            from_psd(&ag_psd::read_psd(&bytes, &ReadOptions::default()).unwrap()).unwrap();
        assert_eq!(
            restored.nodes.len(),
            1,
            "metadata-only decoder retains the layer"
        );
        let layer = &restored.nodes[0];
        assert_eq!(layer.blend, BlendMode::LinearDodge);
        assert!((layer.blending.fill_opacity - 0.4).abs() <= 1.0 / 255.0);
        assert_eq!(layer.blending.channels, [true, false, true]);
        assert!((layer.blending.blend_if.source.black - 0.1).abs() <= 1.0 / 255.0);
        assert_eq!(layer.blending.knockout, Knockout::Shallow);
        assert!(!layer.blending.blend_interior_effects_as_group);
        assert!(!layer.blending.blend_clipped_layers_as_group);
        assert!(!layer.blending.transparency_shapes_layer);
        assert_eq!(
            restored.blend_space,
            emulsion_raster::blend::BlendSpace::Srgb
        );
    }

    #[test]
    fn styles_and_noncontiguous_clipping_export_preserve_appearance() {
        let mut doc = Document::new(8, 8);
        let base = add(
            &mut doc,
            Node::raster(
                0,
                "Base",
                Arc::new(Raster::solid(4, 8, [1.0, 0.0, 0.0, 1.0])),
                Placement::default(),
            ),
            None,
        )
        .unwrap();
        add(
            &mut doc,
            Node::raster(
                0,
                "Between",
                Arc::new(Raster::solid(2, 8, [0.0, 1.0, 0.0, 1.0])),
                Placement::at(6.0, 0.0),
            ),
            None,
        )
        .unwrap();
        let top = add(
            &mut doc,
            Node::raster(
                0,
                "Clipped",
                Arc::new(Raster::solid(8, 8, [0.0, 0.0, 1.0, 1.0])),
                Placement::default(),
            ),
            None,
        )
        .unwrap();
        Command::SetClip {
            id: top,
            clip_to: Some(base),
        }
        .apply(&mut doc)
        .unwrap();
        let restored = roundtrip(&doc, "noncontiguous-clip");
        assert_eq!(restored.nodes.len(), 1);
        assert_eq!(
            flatten(&doc.composite_tree(), 0).to_srgba8(),
            flatten(&restored.composite_tree(), 0).to_srgba8()
        );
        Command::SetClip {
            id: top,
            clip_to: None,
        }
        .apply(&mut doc)
        .unwrap();
        Command::SetStyles {
            id: top,
            styles: vec![emulsion_core::styles::LayerStyle::ColorOverlay {
                color: [255, 200, 20],
                opacity: 100.0,
            }],
        }
        .apply(&mut doc)
        .unwrap();
        let restored = roundtrip(&doc, "style-overlay");
        assert_eq!(restored.nodes.len(), 1);
        assert_eq!(
            flatten(&doc.composite_tree(), 0).to_srgba8(),
            flatten(&restored.composite_tree(), 0).to_srgba8()
        );
    }

    #[test]
    fn advanced_blending_export_preserves_appearance() {
        let mut doc = Document::new(8, 8);
        let mut layer = Node::raster(
            0,
            "Blended",
            Arc::new(Raster::solid(8, 8, [0.8, 0.3, 0.1, 1.])),
            Placement::default(),
        );
        layer.blending.fill_opacity = 0.4;
        layer.blending.channels = [false, true, true];
        add(&mut doc, layer, None).unwrap();
        let restored = roundtrip(&doc, "advanced-blending");
        assert_eq!(
            flatten(&doc.composite_tree(), 0).to_srgba8(),
            flatten(&restored.composite_tree(), 0).to_srgba8()
        );
    }

    #[test]
    fn malformed_pixel_blocks_are_errors_not_panics() {
        let block = |width, height, len| PixelData {
            width,
            height,
            data: vec![255; len],
        };
        let flat = |image_data| Psd {
            width: 2.0,
            height: 2.0,
            color_mode: Some(ColorMode::Rgb),
            image_data: Some(image_data),
            ..Default::default()
        };
        // Truncated, overflowing and oversized composites.
        for px in [
            block(2, 2, 15),
            block(1 << 16, 1 << 16, 16),
            block(u32::MAX, u32::MAX, 16),
            block(0, 2, 16),
        ] {
            assert!(from_psd(&flat(px)).is_err());
        }
        // A short layer opens as an empty layer; a huge one is refused;
        // a malformed independent mask is refused rather than silently dropped.
        let layer = |px, mask: Option<PixelData>| Psd {
            width: 2.0,
            height: 2.0,
            color_mode: Some(ColorMode::Rgb),
            children: Some(vec![Layer {
                image_data: Some(px),
                additional_info: ag_psd::psd::LayerAdditionalInfo {
                    mask: mask.map(|m| LayerMaskData {
                        image_data: Some(m),
                        left: Some(1e300),
                        top: Some(-1e300),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                ..Default::default()
            }]),
            ..Default::default()
        };
        assert!(from_psd(&layer(block(2, 2, 3), None)).is_ok());
        assert!(from_psd(&layer(block(u32::MAX, 3, 16), None)).is_err());
        assert!(from_psd(&layer(block(2, 2, 16), Some(block(u32::MAX, u32::MAX, 4)))).is_err());
        assert!(from_psd(&layer(block(2, 2, 16), Some(block(2, 2, 1)))).is_err());
    }

    #[test]
    fn flat_psd_without_children_uses_its_composite() {
        let path = std::env::temp_dir().join(format!("emulsion-flat-{}.psd", std::process::id()));
        let psd = Psd {
            width: 2.0,
            height: 2.0,
            color_mode: Some(ColorMode::Rgb),
            bits_per_channel: Some(8.0),
            channels: Some(4.0),
            image_data: Some(PixelData {
                width: 2,
                height: 2,
                data: [30, 90, 180, 255].repeat(4),
            }),
            ..Default::default()
        };
        let bytes = ag_psd::write_psd(&psd, &WriteOptions::default());
        std::fs::write(&path, bytes).unwrap();
        let restored = read(&path).unwrap();
        assert_eq!(
            flatten(&restored.composite_tree(), 0).to_srgba8(),
            [30, 90, 180, 255].repeat(4)
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn psd_round_trips_layers_groups_masks_and_blend() {
        let mut d = Document::new(64, 48);
        let bg = Raster::solid(64, 48, [0.2, 0.4, 0.6, 1.0]);
        add(
            &mut d,
            Node::raster(0, "Background", Arc::new(bg), Placement::default()),
            None,
        )
        .unwrap();
        let g = add(&mut d, Node::group(0, "Bits"), None).unwrap();
        let mut top = Node::raster(
            0,
            "Red square",
            Arc::new(Raster::solid(10, 10, [1.0, 0.0, 0.0, 1.0])),
            Placement::at(20.0, 15.0),
        );
        top.opacity = 0.5;
        top.blend = BlendMode::Multiply;
        let mut mask = vec![255u8; 100];
        for v in mask.iter_mut().take(50) {
            *v = 0;
        }
        top.mask = Some(Arc::new(Mask::from_pixels(10, 10, 255, &mask)));
        top.mask_enabled = true;
        add(&mut d, top, Some(g)).unwrap();
        let mut hidden = Node::raster(
            0,
            "Hidden",
            Arc::new(Raster::solid(4, 4, [0.0, 1.0, 0.0, 1.0])),
            Placement::at(1.0, 1.0),
        );
        hidden.visible = false;
        add(&mut d, hidden, None).unwrap();

        let dir = std::env::temp_dir().join(format!("emulsion-psd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rt.psd");
        // This partial-opacity legacy blend differs from the explicit new
        // contract. Real export reports a merged appearance; independently
        // exercise the ordinary layer metadata transport below.
        assert_eq!(
            write_with_report(&d, &path).unwrap().appearance_fallback,
            Some(AppearanceFallback::BlendSpaceDifference)
        );
        let appearance = read(&path).unwrap();
        assert_eq!(profile::render_cpu(&appearance), profile::render_cpu(&d));
        let psd = Psd {
            width: 64.0,
            height: 48.0,
            children: Some(
                d.children(None)
                    .into_iter()
                    .map(|id| layer_for(&d, d.node(id).unwrap()))
                    .collect(),
            ),
            ..Default::default()
        };
        let bytes = ag_psd::write_psd(
            &psd,
            &WriteOptions {
                no_background: Some(true),
                trim_image_data: Some(false),
                ..Default::default()
            },
        );
        let back = from_psd(&ag_psd::read_psd(&bytes, &ReadOptions::default()).unwrap()).unwrap();
        assert_eq!((back.width, back.height), (64, 48));
        let roots = back.children(None);
        assert_eq!(roots.len(), 3, "{roots:?}");
        let names: Vec<String> = roots
            .iter()
            .map(|id| back.node(*id).unwrap().name.clone())
            .collect();
        assert_eq!(names, ["Background", "Bits", "Hidden"]);
        let group = back.node(roots[1]).unwrap();
        assert!(group.kind.is_group());
        let kids = back.children(Some(group.id));
        assert_eq!(kids.len(), 1);
        let sq = back.node(kids[0]).unwrap();
        assert_eq!(sq.name, "Red square");
        assert!((sq.opacity - 0.5).abs() < 0.01);
        assert_eq!(sq.blend, BlendMode::Multiply);
        let NodeKind::Raster { raster, placement } = &sq.kind else {
            panic!("raster");
        };
        assert_eq!((raster.width(), raster.height()), (10, 10));
        assert_eq!((placement.x, placement.y), (20.0, 15.0));
        assert!(raster.get(5, 5)[0] > 60000);
        let m = sq.mask.as_ref().expect("mask");
        assert_eq!(m.get(0, 0), 0);
        assert_eq!(m.get(9, 9), 255);
        assert!(sq.mask_enabled);
        assert!(!back.node(roots[2]).unwrap().visible);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_photoshop_blend_mode_maps_both_ways() {
        // The 27 PSD layer blend modes plus group pass-through.
        let modes = [
            PsdBlend::PassThrough,
            PsdBlend::Normal,
            PsdBlend::Dissolve,
            PsdBlend::Darken,
            PsdBlend::Multiply,
            PsdBlend::ColorBurn,
            PsdBlend::LinearBurn,
            PsdBlend::DarkerColor,
            PsdBlend::Lighten,
            PsdBlend::Screen,
            PsdBlend::ColorDodge,
            PsdBlend::LinearDodge,
            PsdBlend::LighterColor,
            PsdBlend::Overlay,
            PsdBlend::SoftLight,
            PsdBlend::HardLight,
            PsdBlend::VividLight,
            PsdBlend::LinearLight,
            PsdBlend::PinLight,
            PsdBlend::HardMix,
            PsdBlend::Difference,
            PsdBlend::Exclusion,
            PsdBlend::Subtract,
            PsdBlend::Divide,
            PsdBlend::Hue,
            PsdBlend::Saturation,
            PsdBlend::Color,
            PsdBlend::Luminosity,
        ];
        let mut seen = std::collections::HashSet::new();
        for mode in modes {
            let ours = blend_in(Some(mode));
            assert!(seen.insert(format!("{ours:?}")), "{mode:?} shares a mode");
            assert_eq!(
                format!("{:?}", blend_out(ours)),
                format!("{mode:?}"),
                "{mode:?} does not round-trip"
            );
        }
        assert_eq!(seen.len(), 28);
    }
}
