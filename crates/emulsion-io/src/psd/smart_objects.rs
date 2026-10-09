//! Bounded PSD Smart Objects with embedded PNG source pixels and no filters.
//!
//! The ag-psd 0.3 public model exposes linked files, but its handlers discard
//! their bytes and omit them on write. This adapter only supplies that missing
//! document-level framing and validates a deliberately small placed-layer
//! subset. It never treats a layer preview or FMsk display metadata as source.
//!
//! Framing references (this is an independent implementation, not copied code):
//! - PSD specification, Linked Layer and Placed Layer Data tables:
//!   <https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/>
//! - psd-tools' MIT linked_layer.py and smart_object.py, including the source
//!   Idnt UUID association and versioned optional tails:
//!   <https://github.com/psd-tools/psd-tools/tree/d68bf46c7140a1f8c74be9c10b4e21103e820761/src/psd_tools>
//!
//! Generic descriptor decoding and placed-layer encoding remain ag-psd's.

use ag_psd::descriptor::{Descriptor, DescriptorValue, read_version_and_descriptor};
use ag_psd::psd::{
    Layer, PixelData, PlacedLayer, PlacedLayerType, Units, UnitsBounds, UnitsValue, Warp, WarpStyle,
};
use emulsion_core::{Document, Node, NodeId, NodeKind};
use emulsion_raster::{Placement, Raster, TILE, TileCoord};
use image::ImageEncoder;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

const MAX_SOURCES: usize = 1024;
const MAX_DESCRIPTOR_BYTES: usize = 64 * 1024;
const MAX_STRING: usize = 4096;

pub(super) use crate::original_image_png::SourceError as SmartError;
use crate::original_image_png::{
    MAX_SOURCE_BYTES, MAX_TOTAL_SOURCE_BYTES, SourceBudget, dimensions, native_storage,
    png_source_with_budget,
};
#[cfg(test)]
use crate::original_image_png::{MAX_TOTAL_SOURCE_NATIVE_BYTES, png_crc, png_source, strict_idat};
type Result<T> = std::result::Result<T, SmartError>;

#[derive(Clone, Copy)]
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }
    fn remaining(&self) -> &'a [u8] {
        &self.bytes[self.at..]
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(n)
            .ok_or(SmartError::Malformed("length overflow"))?;
        let out = self
            .bytes
            .get(self.at..end)
            .ok_or(SmartError::Malformed("truncated record"))?;
        self.at = end;
        Ok(out)
    }
    fn number(&mut self, n: usize) -> Result<u64> {
        Ok(self.take(n)?.iter().fold(0, |a, b| a << 8 | u64::from(*b)))
    }
    fn length(&mut self, n: usize) -> Result<usize> {
        usize::try_from(self.number(n)?)
            .map_err(|_| SmartError::Malformed("length exceeds address space"))
    }
    fn section(&mut self, n: usize) -> Result<&'a [u8]> {
        let len = self.length(n)?;
        self.take(len)
    }
    fn unicode(&mut self) -> Result<String> {
        let n = self.length(4)?;
        if n > MAX_STRING {
            return Err(SmartError::Unsupported("string is too long"));
        }
        let bytes = self.take(n * 2)?;
        let units: Vec<_> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_be_bytes(*b))
            .collect();
        String::from_utf16(&units).map_err(|_| SmartError::Malformed("invalid Unicode string"))
    }
    fn id(&mut self) -> Result<()> {
        let n = self.length(4)?;
        if n > MAX_STRING {
            return Err(SmartError::Unsupported("descriptor ID is too long"));
        }
        self.take(if n == 0 { 4 } else { n })?;
        Ok(())
    }
    fn padding(&mut self, n: usize) -> Result<()> {
        if self.take(n)?.iter().any(|b| *b != 0) {
            return Err(SmartError::Malformed("nonzero padding"));
        }
        Ok(())
    }
}

fn guid(text: &str) -> Result<String> {
    let text = text.trim_end_matches('\0');
    if text.len() != 36
        || text.bytes().enumerate().any(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b != b'-'
            } else {
                !b.is_ascii_hexdigit()
            }
        })
    {
        return Err(SmartError::Unsupported("invalid source or instance UUID"));
    }
    Ok(text.to_ascii_lowercase())
}

// Preflight only the documented descriptor types used in this small subset,
// bounding counts and recursion before invoking the dependency's general parser.
fn descriptor_shape(c: &mut Cursor<'_>, depth: usize, budget: &mut usize) -> Result<()> {
    if depth > 8 {
        return Err(SmartError::Unsupported("descriptor nesting limit"));
    }
    c.unicode()?;
    c.id()?;
    let count = c.length(4)?;
    if count > *budget {
        return Err(SmartError::Unsupported("descriptor item limit"));
    }
    *budget -= count;
    for _ in 0..count {
        c.id()?;
        descriptor_value_shape(c, depth + 1, budget)?;
    }
    Ok(())
}
fn descriptor_value_shape(c: &mut Cursor<'_>, depth: usize, budget: &mut usize) -> Result<()> {
    if depth > 8 {
        return Err(SmartError::Unsupported("descriptor nesting limit"));
    }
    match c.take(4)? {
        b"Objc" | b"GlbO" => descriptor_shape(c, depth, budget)?,
        b"VlLs" => {
            let count = c.length(4)?;
            if count > *budget {
                return Err(SmartError::Unsupported("descriptor list limit"));
            }
            *budget -= count;
            for _ in 0..count {
                descriptor_value_shape(c, depth + 1, budget)?;
            }
        }
        b"doub" => {
            c.take(8)?;
        }
        b"UntF" => {
            c.take(12)?;
        }
        b"UnFl" => {
            c.take(8)?;
        }
        b"TEXT" => {
            c.unicode()?;
        }
        b"enum" => {
            c.id()?;
            c.id()?;
        }
        b"long" => {
            c.take(4)?;
        }
        b"bool" => {
            if c.number(1)? > 1 {
                return Err(SmartError::Malformed("invalid descriptor boolean"));
            }
        }
        _ => {
            return Err(SmartError::Unsupported(
                "descriptor type outside source-only subset",
            ));
        }
    }
    Ok(())
}
fn descriptor(c: &mut Cursor<'_>) -> Result<Descriptor> {
    let start = c.at;
    if c.number(4)? != 16 {
        return Err(SmartError::Unsupported("descriptor version"));
    }
    descriptor_shape(c, 0, &mut 256)?;
    let bytes = &c.bytes[start..c.at];
    if bytes.len() > MAX_DESCRIPTOR_BYTES {
        return Err(SmartError::Unsupported("descriptor byte limit"));
    }
    let mut reader = ag_psd::reader::PsdReader::new(bytes, None, None);
    reader.strict = true;
    let d = read_version_and_descriptor(&mut reader)
        .map_err(|_| SmartError::Malformed("invalid descriptor"))?;
    if reader.offset != bytes.len() {
        return Err(SmartError::Malformed("descriptor framing mismatch"));
    }
    Ok(d)
}
fn keys(d: &Descriptor, allowed: &[&str]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for (key, _) in &d.items {
        if !seen.insert(key) {
            return Err(SmartError::Malformed("duplicate descriptor key"));
        }
        if !allowed.contains(&key.as_str()) {
            return Err(SmartError::Unsupported(
                "unknown placed/source descriptor field",
            ));
        }
    }
    Ok(())
}
fn class(d: &Descriptor, allowed: &[&str]) -> Result<()> {
    if !allowed.contains(&d.class_id.as_str()) {
        return Err(SmartError::Unsupported("unknown descriptor class"));
    }
    Ok(())
}
fn text<'a>(d: &'a Descriptor, key: &str) -> Result<&'a str> {
    match d.get(key) {
        Some(DescriptorValue::Text(v)) => Ok(v),
        _ => Err(SmartError::Unsupported("missing text descriptor field")),
    }
}
fn number_value(v: &DescriptorValue) -> Option<f64> {
    let n = match v {
        DescriptorValue::Double(v) => *v,
        DescriptorValue::Integer(v) => f64::from(*v),
        _ => return None,
    };
    n.is_finite().then_some(n)
}
fn number(d: &Descriptor, key: &str) -> Result<f64> {
    d.get(key)
        .and_then(number_value)
        .ok_or(SmartError::Unsupported("invalid numeric descriptor field"))
}
fn optional_number(d: &Descriptor, key: &str, allowed: &[f64]) -> Result<()> {
    if d.get(key).is_some() && !allowed.contains(&number(d, key)?) {
        return Err(SmartError::Unsupported("nondefault placed-layer control"));
    }
    Ok(())
}
fn nested<'a>(d: &'a Descriptor, key: &str) -> Result<&'a Descriptor> {
    match d.get(key) {
        Some(DescriptorValue::Descriptor(v)) => Ok(v),
        _ => Err(SmartError::Unsupported("missing nested descriptor")),
    }
}
fn comp_info(d: &Descriptor) -> Result<()> {
    class(d, &["null"])?;
    keys(d, &["compID", "originalCompID"])?;
    for key in ["compID", "originalCompID"] {
        optional_number(d, key, &[-1.0])?;
    }
    Ok(())
}
fn fraction(d: &Descriptor, key: &str) -> Result<()> {
    if d.get(key).is_none() {
        return Ok(());
    }
    let f = nested(d, key)?;
    class(f, &["null"])?;
    keys(f, &["numerator", "denominator"])?;
    if number(f, "numerator")? != 0.0 || number(f, "denominator")? <= 0.0 {
        return Err(SmartError::Unsupported("animated placed layer"));
    }
    Ok(())
}
fn unit_pixels(v: &DescriptorValue) -> Option<f64> {
    match v {
        DescriptorValue::UnitDouble(v) if v.units == "Pixels" && v.value.is_finite() => {
            Some(v.value)
        }
        _ => number_value(v),
    }
}
fn no_warp(d: &Descriptor, width: u32, height: u32) -> Result<()> {
    class(d, &["warp"])?;
    keys(
        d,
        &[
            "warpStyle",
            "warpValue",
            "warpPerspective",
            "warpPerspectiveOther",
            "warpRotate",
            "bounds",
            "uOrder",
            "vOrder",
        ],
    )?;
    if !matches!(d.get("warpStyle"), Some(DescriptorValue::Enum(v)) if v == "warpStyle.warpNone") {
        return Err(SmartError::Unsupported("warped source"));
    }
    for key in ["warpValue", "warpPerspective", "warpPerspectiveOther"] {
        optional_number(d, key, &[0.0])?;
    }
    for key in ["uOrder", "vOrder"] {
        optional_number(d, key, &[0.0, 4.0])?;
    }
    if d.get("warpRotate").is_some()
        && !matches!(d.get("warpRotate"), Some(DescriptorValue::Enum(v)) if v == "Ornt.Hrzn")
    {
        return Err(SmartError::Unsupported("warp orientation"));
    }
    if d.get("bounds").is_some() {
        let b = nested(d, "bounds")?;
        class(b, &["Rctn", "classFloatRect"])?;
        keys(b, &["Top ", "Left", "Btom", "Rght"])?;
        for (key, expected) in [
            ("Top ", 0.0),
            ("Left", 0.0),
            ("Btom", f64::from(height)),
            ("Rght", f64::from(width)),
        ] {
            if b.get(key).and_then(unit_pixels) != Some(expected) {
                return Err(SmartError::Unsupported("warp bounds differ from source"));
            }
        }
    }
    Ok(())
}
fn integer(v: f64) -> bool {
    v.is_finite() && v.fract() == 0.0 && (i32::MIN as f64..=i32::MAX as f64).contains(&v)
}

#[derive(Clone, Debug)]
struct Instance {
    source_id: String,
    placement: Placement,
    width: u32,
    height: u32,
}
struct Legacy {
    source_id: String,
    points: Vec<f64>,
    warp: Descriptor,
}
fn legacy(body: &[u8]) -> Result<Legacy> {
    if body.len() > MAX_DESCRIPTOR_BYTES {
        return Err(SmartError::Unsupported(
            "legacy placed descriptor byte limit",
        ));
    }
    let mut c = Cursor::new(body);
    if c.take(4)? != b"plcL" || c.number(4)? != 3 {
        return Err(SmartError::Unsupported("legacy placed-layer version"));
    }
    let length = c.length(1)?;
    let source_id = guid(
        std::str::from_utf8(c.take(length)?)
            .map_err(|_| SmartError::Malformed("legacy UUID encoding"))?,
    )?;
    for expected in [1, 1, 16, 2] {
        if c.number(4)? != expected {
            return Err(SmartError::Unsupported("legacy placed-layer controls"));
        }
    }
    let mut points = Vec::with_capacity(8);
    for _ in 0..8 {
        let bytes: [u8; 8] = c.take(8)?.try_into().expect("length checked");
        let value = f64::from_be_bytes(bytes);
        if !integer(value) {
            return Err(SmartError::Unsupported("legacy source transform"));
        }
        points.push(value);
    }
    if c.number(4)? != 0 {
        return Err(SmartError::Unsupported("legacy warp version"));
    }
    let warp = descriptor(&mut c)?;
    if c.remaining().len() > 3 {
        return Err(SmartError::Unsupported("extra legacy placed-layer data"));
    }
    c.padding(c.remaining().len())?;
    Ok(Legacy {
        source_id,
        points,
        warp,
    })
}
fn placed(body: &[u8]) -> Result<(String, Instance)> {
    if body.len() > MAX_DESCRIPTOR_BYTES {
        return Err(SmartError::Unsupported("placed descriptor byte limit"));
    }
    let mut c = Cursor::new(body);
    if c.take(4)? != b"soLD" || !matches!(c.number(4)?, 4 | 5) {
        return Err(SmartError::Unsupported("placed-layer version"));
    }
    let d = descriptor(&mut c)?;
    class(&d, &["null"])?;
    if c.remaining().len() > 3 {
        return Err(SmartError::Unsupported("extra placed-layer data"));
    }
    c.padding(c.remaining().len())?;
    keys(
        &d,
        &[
            "Idnt",
            "placed",
            "PgNm",
            "totalPages",
            "Crop",
            "frameStep",
            "duration",
            "frameCount",
            "Annt",
            "Type",
            "Trnf",
            "nonAffineTransform",
            "warp",
            "Sz  ",
            "Rslt",
            "comp",
            "compInfo",
        ],
    )?;
    let source_id = guid(text(&d, "Idnt")?)?;
    let instance_id = guid(text(&d, "placed")?)?;
    if number(&d, "Type")? != 2.0 {
        return Err(SmartError::Unsupported(
            "source is not a raster Smart Object",
        ));
    }
    optional_number(&d, "PgNm", &[1.0])?;
    optional_number(&d, "totalPages", &[1.0])?;
    optional_number(&d, "Crop", &[0.0, 1.0])?;
    optional_number(&d, "frameCount", &[0.0, 1.0])?;
    optional_number(&d, "Annt", &[16.0])?;
    optional_number(&d, "comp", &[-1.0])?;
    fraction(&d, "frameStep")?;
    fraction(&d, "duration")?;
    if d.get("compInfo").is_some() {
        comp_info(nested(&d, "compInfo")?)?;
    }
    if let Some(v) = d.get("Rslt")
        && !matches!(v, DescriptorValue::UnitDouble(v) if v.units == "Density" && v.value.is_finite() && v.value > 0.0 && v.value <= 9600.0)
    {
        return Err(SmartError::Unsupported("source resolution"));
    }
    let size = nested(&d, "Sz  ")?;
    class(size, &["Pnt "])?;
    keys(size, &["Wdth", "Hght"])?;
    let (w, h) = (number(size, "Wdth")?, number(size, "Hght")?);
    if !integer(w) || !integer(h) || w <= 0.0 || h <= 0.0 {
        return Err(SmartError::Unsupported("source dimensions"));
    }
    let (width, height) = (w as u32, h as u32);
    dimensions(width, height)?;
    if d.get("warp").is_some() {
        no_warp(nested(&d, "warp")?, width, height)?;
    }
    let Some(DescriptorValue::List(points)) = d.get("Trnf") else {
        return Err(SmartError::Unsupported("missing source transform"));
    };
    let points: Vec<f64> = points
        .iter()
        .map(number_value)
        .collect::<Option<_>>()
        .ok_or(SmartError::Unsupported("invalid transform"))?;
    if points.len() != 8 || points.iter().any(|v| !integer(*v)) {
        return Err(SmartError::Unsupported("non-integer source transform"));
    }
    let (x, y) = (points[0], points[1]);
    if points != [x, y, x + w, y, x + w, y + h, x, y + h] {
        return Err(SmartError::Unsupported("scaled/rotated/projective source"));
    }
    if let Some(value) = d.get("nonAffineTransform") {
        let Some(original) = d.get("Trnf") else {
            unreachable!()
        };
        if value != original {
            return Err(SmartError::Unsupported("non-affine source transform"));
        }
    }
    Ok((
        instance_id,
        Instance {
            source_id,
            placement: Placement::at(x, y),
            width,
            height,
        },
    ))
}

struct FileSections<'a> {
    wide: usize,
    depth: u16,
    mode: u16,
    length_at: usize,
    body: &'a [u8],
    after: &'a [u8],
    resources: &'a [u8],
}
fn file_sections(bytes: &[u8]) -> Result<FileSections<'_>> {
    let mut c = Cursor::new(bytes);
    if c.take(4)? != b"8BPS" {
        return Err(SmartError::Malformed("file signature"));
    }
    let wide = match c.number(2)? {
        1 => 4,
        2 => 8,
        _ => return Err(SmartError::Unsupported("file version")),
    };
    c.padding(6)?;
    c.take(10)?; // Channels and canvas dimensions: the main reader validates them.
    let depth = c.number(2)? as u16;
    let mode = c.number(2)? as u16;
    c.section(4)?;
    let resources = c.section(4)?;
    let length_at = c.at;
    let body = c.section(wide)?;
    Ok(FileSections {
        wide,
        depth,
        mode,
        length_at,
        body,
        after: c.remaining(),
        resources,
    })
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum TagLocation {
    Layer,
    Document,
}
fn tags<'a>(
    c: &mut Cursor<'a>,
    wide: usize,
    location: TagLocation,
    mut visit: impl FnMut(&str, &'a [u8]) -> Result<()>,
) -> Result<()> {
    while !c.remaining().is_empty() {
        // Document-level records can have additional zero alignment between
        // tags, as accepted by the main decoder and mask guard. Do not extend
        // that exception to layer extras or search through nonzero bytes.
        if location == TagLocation::Document {
            while c.remaining().first() == Some(&0) {
                c.take(1)?;
            }
            if c.remaining().is_empty() {
                break;
            }
        }
        if c.remaining().len() <= 3 && c.remaining().iter().all(|b| *b == 0) {
            c.take(c.remaining().len())?;
            break;
        }
        let signature = c.take(4)?;
        if signature != b"8BIM" && signature != b"8B64" {
            return Err(SmartError::Malformed("tagged-block signature"));
        }
        let key = std::str::from_utf8(c.take(4)?)
            .map_err(|_| SmartError::Malformed("tagged-block key"))?;
        let width =
            if signature == b"8B64" || (wide == 8 && ag_psd::additional_info::is_large_key(key)) {
                8
            } else {
                4
            };
        let body = c.section(width)?;
        visit(key, body)?;
        c.padding(body.len() % 2)?;
    }
    Ok(())
}
fn layer_instances(data: &[u8], wide: usize) -> Result<BTreeMap<String, Instance>> {
    let mut instances = BTreeMap::new();
    if data.is_empty() {
        return Ok(instances);
    }
    let mut c = Cursor::new(data);
    let count = (c.number(2)? as u16 as i16).unsigned_abs();
    let mut channel_bytes = 0usize;
    for _ in 0..count {
        c.take(16)?;
        let channels = c.length(2)?;
        if channels > 56 {
            return Err(SmartError::Malformed("layer channel count"));
        }
        for _ in 0..channels {
            c.take(2)?;
            channel_bytes = channel_bytes
                .checked_add(c.length(wide)?)
                .ok_or(SmartError::Malformed("channel byte overflow"))?;
        }
        if c.take(4)? != b"8BIM" {
            return Err(SmartError::Malformed("layer blend signature"));
        }
        c.take(8)?;
        let mut extra = Cursor::new(c.section(4)?);
        extra.section(4)?;
        extra.section(4)?;
        let name = extra.length(1)?;
        extra.take(name)?;
        extra.padding((4 - (name + 1) % 4) % 4)?;
        let mut old = None;
        let mut current = None;
        tags(&mut extra, wide, TagLocation::Layer, |key, body| {
            match key {
                "PlLd" => {
                    if old.is_some() {
                        return Err(SmartError::Malformed("duplicate legacy placed record"));
                    }
                    old = Some(legacy(body)?);
                }
                "SoLd" | "SoLE" => {
                    if current.is_some() {
                        return Err(SmartError::Malformed("duplicate placed-layer record"));
                    }
                    current = Some(placed(body)?);
                }
                "FEid" | "FXid" => {
                    return Err(SmartError::Unsupported(
                        "filter-effect records are not source-only Smart Objects",
                    ));
                }
                _ => {}
            }
            Ok(())
        })?;
        if let Some((id, instance)) = current {
            if let Some(old) = old {
                let (x, y, w, h) = (
                    instance.placement.x,
                    instance.placement.y,
                    f64::from(instance.width),
                    f64::from(instance.height),
                );
                if old.source_id != instance.source_id
                    || old.points != [x, y, x + w, y, x + w, y + h, x, y + h]
                {
                    return Err(SmartError::Unsupported(
                        "legacy and modern placed records disagree",
                    ));
                }
                no_warp(&old.warp, instance.width, instance.height)?;
            }
            if instances.len() >= MAX_SOURCES {
                return Err(SmartError::Unsupported("Smart instance count limit"));
            }
            if instances.insert(id, instance).is_some() {
                return Err(SmartError::Malformed("duplicate instance UUID"));
            }
        } else if old.is_some() {
            return Err(SmartError::Unsupported("legacy-only placed layer"));
        }
    }
    c.take(channel_bytes)?;
    if c.remaining().len() > 3 {
        return Err(SmartError::Malformed("extra layer-info bytes"));
    }
    c.padding(c.remaining().len())?;
    Ok(instances)
}

fn known_srgb_profile(profile: &[u8]) -> bool {
    // Standard sRGB IEC61966-2.1 profile in the independently authored baseline.
    // Exact bytes avoid trusting an arbitrary profile's display name or probes.
    Sha256::digest(profile)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
        == "2b3aa1645779a9e634744faf9b01e9102b0c9b88fd6deced7934df86b949af7e"
        || crate::icc::srgb_profile().is_some_and(|p| p == profile)
}
fn document_profile(resources: &[u8]) -> Result<()> {
    let mut c = Cursor::new(resources);
    let mut seen = false;
    while !c.remaining().is_empty() {
        if c.take(4)? != b"8BIM" {
            return Err(SmartError::Malformed("image-resource signature"));
        }
        let id = c.number(2)?;
        let name = c.length(1)?;
        c.take(name)?;
        c.padding((name + 1) % 2)?;
        let data = c.section(4)?;
        if id == 1039 {
            if seen {
                return Err(SmartError::Malformed("duplicate document ICC profile"));
            }
            seen = true;
            if data.len() > 4 * 1024 * 1024 || !known_srgb_profile(data) {
                return Err(SmartError::Unsupported("unknown document color profile"));
            }
        }
        c.padding(data.len() % 2)?;
    }
    Ok(())
}

fn linked_sources(
    data: &[u8],
    out: &mut BTreeMap<String, (Arc<Raster>, Arc<emulsion_core::node::OriginalImage>)>,
    budget: &mut SourceBudget,
) -> Result<()> {
    let mut c = Cursor::new(data);
    while !c.remaining().is_empty() {
        if c.remaining().len() <= 3 && c.remaining().iter().all(|b| *b == 0) {
            c.take(c.remaining().len())?;
            break;
        }
        let record = c.section(8)?;
        if record.len() > MAX_SOURCE_BYTES + MAX_DESCRIPTOR_BYTES {
            return Err(SmartError::Unsupported("linked-source record limit"));
        }
        let mut r = Cursor::new(record);
        if r.take(4)? != b"liFD" {
            return Err(SmartError::Unsupported("external or alias Smart source"));
        }
        let version = r.number(4)?;
        if !(1..=7).contains(&version) {
            return Err(SmartError::Unsupported("linked-source version"));
        }
        let n = r.length(1)?;
        let id = guid(
            std::str::from_utf8(r.take(n)?)
                .map_err(|_| SmartError::Malformed("source UUID encoding"))?,
        )?;
        r.unicode()?; // Filename is inert metadata; it is never a path to open.
        let kind = r.take(4)?;
        if !matches!(kind, b"PNG " | b"png " | b"    " | b"\0\0\0\0") {
            return Err(SmartError::Unsupported("source file type is not PNG"));
        }
        r.take(4)?; // Creator signature.
        let size = r.length(8)?;
        if size > MAX_SOURCE_BYTES {
            return Err(SmartError::Unsupported("embedded source byte limit"));
        }
        match r.number(1)? {
            0 => {}
            1 => {
                let d = descriptor(&mut r)?;
                class(&d, &["null"])?;
                keys(&d, &["compInfo"])?;
                comp_info(nested(&d, "compInfo")?)?;
            }
            _ => return Err(SmartError::Malformed("source open-file flag")),
        }
        let data = r.take(size)?;
        if version >= 5 {
            r.unicode()?;
        }
        if version >= 6 {
            let b: [u8; 8] = r.take(8)?.try_into().expect("length checked");
            if !f64::from_be_bytes(b).is_finite() {
                return Err(SmartError::Malformed("asset modification time"));
            }
        }
        if version >= 7 && r.number(1)? != 0 {
            return Err(SmartError::Unsupported("locked linked-library source"));
        }
        if r.remaining().len() > 3 {
            return Err(SmartError::Unsupported("extra source record data"));
        }
        r.padding(r.remaining().len())?;
        if out.len() >= MAX_SOURCES || out.contains_key(&id) {
            return Err(SmartError::Malformed("duplicate or excessive source UUIDs"));
        }
        let source = png_source_with_budget(data, budget)?;
        let (source, original) = if let Some((same, original)) = out
            .values()
            .find(|(_, original)| original.bytes().as_slice() == data)
        {
            (same.clone(), original.clone())
        } else {
            let original = crate::original_image_data::capture(Arc::new(data.to_vec()), &source);
            (source, original)
        };
        out.insert(id, (source, original));
        c.padding((4 - record.len() % 4) % 4)?;
    }
    Ok(())
}

#[derive(Default)]
pub(super) struct ImportSources {
    instances: BTreeMap<String, Instance>,
    sources: BTreeMap<String, (Arc<Raster>, Arc<emulsion_core::node::OriginalImage>)>,
}
impl ImportSources {
    pub(super) fn layer_kind(&self, layer: &Layer) -> Result<Option<NodeKind>> {
        let Some(placed) = &layer.additional_info.placed_layer else {
            return Ok(None);
        };
        if layer.children.is_some() {
            return Err(SmartError::Unsupported("Smart source on a group"));
        }
        let id = guid(
            placed
                .placed
                .as_deref()
                .ok_or(SmartError::Unsupported("missing placed instance UUID"))?,
        )?;
        let instance = self
            .instances
            .get(&id)
            .ok_or(SmartError::Unsupported("placed instance was not validated"))?;
        if guid(&placed.id)? != instance.source_id {
            return Err(SmartError::Malformed(
                "source association changed during parsing",
            ));
        }
        let (source, original_image) = self
            .sources
            .get(&instance.source_id)
            .ok_or(SmartError::Unsupported("missing embedded source"))?
            .clone();
        Ok(Some(NodeKind::Smart {
            editable: None,
            source: source.clone(),
            original_image: Some(original_image),
            filters: Vec::new(),
            filter_styles: Vec::new(),
            filters_enabled: true,
            filter_mask: None,
            placement: emulsion_core::SmartPlacement::Legacy(instance.placement),
            cache: source,
            offset: (0, 0),
        }))
    }
}

/// Inspect before normal PSD decoding, so discarded filterFX/source records
/// can never masquerade as this source-only editable subset.
pub(super) fn inspect(bytes: &[u8]) -> Result<ImportSources> {
    let file = file_sections(bytes)?;
    let mut c = Cursor::new(file.body);
    if c.remaining().is_empty() {
        return Ok(ImportSources::default());
    }
    let layers = c.section(file.wide)?;
    let instances = layer_instances(layers, file.wide)?;
    c.padding(layers.len() % 2)?;
    if !c.remaining().is_empty()
        && !c.remaining().starts_with(b"8BIM")
        && !c.remaining().starts_with(b"8B64")
    {
        c.section(4)?;
    }
    let mut sources = BTreeMap::new();
    let mut budget = SourceBudget::default();
    tags(&mut c, file.wide, TagLocation::Document, |key, body| {
        match key {
            "lnk2" | "lnkD" | "lnk3" => linked_sources(body, &mut sources, &mut budget)?,
            "lnkE" if !body.is_empty() => {
                return Err(SmartError::Unsupported("external linked sources"));
            }
            "FEid" | "FXid" if !body.is_empty() => {
                return Err(SmartError::Unsupported("Smart Filter records"));
            }
            "Lr16" | "Lr32" if !body.is_empty() && !instances.is_empty() => {
                return Err(SmartError::Unsupported(
                    "alternative high-depth layer records",
                ));
            }
            _ => {}
        }
        Ok(())
    })?;
    if !instances.is_empty() {
        if file.depth != 8 || file.mode != 3 {
            return Err(SmartError::Unsupported(
                "only RGB8 documents retain editable Smart sources",
            ));
        }
        document_profile(file.resources)?;
        // PSD instances sharing Idnt share future source edits. Native
        // raster-backed Smart editing is node-local; Arc sharing alone is only
        // copy-on-write storage and does not preserve that editing relationship.
        let mut referenced = BTreeSet::new();
        for instance in instances.values() {
            if !referenced.insert(&instance.source_id) {
                return Err(SmartError::Unsupported(
                    "shared editable Smart source instances",
                ));
            }
            let (source, _) = sources
                .get(&instance.source_id)
                .ok_or(SmartError::Unsupported("Smart source is not embedded"))?;
            if (source.width(), source.height()) != (instance.width, instance.height) {
                return Err(SmartError::Malformed(
                    "descriptor and PNG source dimensions disagree",
                ));
            }
        }
    }
    Ok(ImportSources { instances, sources })
}

const MAX_UUID_ATTEMPTS: usize = 8;

/// Node IDs are only document-local. Each export gets fresh RFC 9562 UUIDv4s,
/// shared consistently across its placed descriptors and embedded resources.
/// Check the final version/variant-masked bytes so the two roles cannot collide.
fn fresh_uuid_v4(
    used: &mut BTreeSet<[u8; 16]>,
    entropy: &mut impl FnMut(&mut [u8]) -> Result<()>,
) -> Result<String> {
    for _ in 0..MAX_UUID_ATTEMPTS {
        let mut bytes = [0; 16];
        entropy(&mut bytes)?;
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        if used.insert(bytes) {
            let hex = format!("{:032x}", u128::from_be_bytes(bytes));
            return Ok(format!(
                "{}-{}-{}-{}-{}",
                &hex[..8],
                &hex[8..12],
                &hex[12..16],
                &hex[16..20],
                &hex[20..]
            ));
        }
    }
    Err(SmartError::Unavailable(
        "could not generate distinct Smart Object UUIDs",
    ))
}
/// Cheap source-only capability check; pixels and resource totals are checked
/// by prepare_export on the export worker, not by foreground UI diagnostics.
pub(super) fn can_export(node: &Node) -> bool {
    let NodeKind::Smart {
        editable,
        source,
        filters,
        filter_styles,
        filters_enabled,
        filter_mask,
        placement,
        cache,
        offset,
        ..
    } = &node.kind
    else {
        return false;
    };
    let Some(placement) = placement.legacy() else {
        return false;
    };
    !node.has_projective_metadata()
        && editable.is_none()
        // Source-only PSD records do not encode the authored stack switch,
        // including a disabled stack that currently contains no filters.
        && *filters_enabled
        && filters.is_empty()
        && filter_styles.is_empty()
        && filter_mask.is_none()
        && *offset == (0, 0)
        && (source.width(), source.height()) == (cache.width(), cache.height())
        && native_storage(source.width(), source.height()).is_ok()
        && super::raster_placement_is_translated(&placement)
        && [
            placement.x,
            placement.y,
            placement.x + f64::from(source.width()),
            placement.y + f64::from(source.height()),
        ]
        .into_iter()
        .all(integer)
}

struct ExportSource {
    source_id: String,
    instance_id: String,
    placed: PlacedLayer,
    pixels: PixelData,
}
#[derive(Default)]
pub(super) struct ExportSources {
    layers: BTreeMap<NodeId, ExportSource>,
    linked: Vec<u8>,
}
impl ExportSources {
    /// Sets content/source metadata only. The parent adds layer blending and
    /// ordinary masks using the same translated-source origin as raster layers.
    pub(super) fn apply_layer(&self, node: &Node, layer: &mut Layer) -> bool {
        let Some(source) = self.layers.get(&node.id) else {
            return false;
        };
        let p = &source.placed;
        let (x, y) = (p.transform[0], p.transform[1]);
        layer.left = Some(x);
        layer.top = Some(y);
        layer.right = Some(x + f64::from(source.pixels.width));
        layer.bottom = Some(y + f64::from(source.pixels.height));
        layer.image_data = Some(source.pixels.clone());
        layer.additional_info.placed_layer = Some(p.clone());
        true
    }
    /// Append only the generated global source block, preserving all emitted
    /// layer/channel/composite bytes. Existing linked/filter blocks are refused.
    pub(super) fn insert(&self, bytes: Vec<u8>) -> Result<Vec<u8>> {
        if self.linked.is_empty() {
            return Ok(bytes);
        }
        let file = file_sections(&bytes)?;
        let mut c = Cursor::new(file.body);
        let layers = c.section(file.wide)?;
        let instances = layer_instances(layers, file.wide)?;
        if instances.len() != self.layers.len()
            || self.layers.values().any(|source| {
                instances.get(&source.instance_id).is_none_or(|i| {
                    i.source_id != source.source_id
                        || (i.width, i.height) != (source.pixels.width, source.pixels.height)
                        || (i.placement.x, i.placement.y)
                            != (source.placed.transform[0], source.placed.transform[1])
                })
            })
        {
            return Err(SmartError::Malformed(
                "emitted placed layers do not match embedded sources",
            ));
        }
        c.padding(layers.len() % 2)?;
        if !c.remaining().is_empty()
            && !c.remaining().starts_with(b"8BIM")
            && !c.remaining().starts_with(b"8B64")
        {
            c.section(4)?;
        }
        tags(&mut c, file.wide, TagLocation::Document, |key, _| {
            if matches!(key, "lnk2" | "lnkD" | "lnk3" | "lnkE" | "FEid" | "FXid") {
                Err(SmartError::Unsupported(
                    "writer already emitted source/filter blocks",
                ))
            } else {
                Ok(())
            }
        })?;
        let mut tag = b"8BIMlnk2".to_vec();
        put_length(
            &mut tag,
            self.linked.len(),
            if file.wide == 8 { 8 } else { 4 },
        )?;
        tag.extend_from_slice(&self.linked);
        if !self.linked.len().is_multiple_of(2) {
            tag.push(0);
        }
        let new_len = file
            .body
            .len()
            .checked_add(tag.len())
            .ok_or(SmartError::Unsupported("document byte overflow"))?;
        let mut out = Vec::new();
        out.try_reserve(
            bytes
                .len()
                .checked_add(tag.len())
                .ok_or(SmartError::Unsupported("document byte overflow"))?,
        )
        .map_err(|_| SmartError::Unsupported("source insertion allocation"))?;
        out.extend_from_slice(&bytes[..file.length_at]);
        put_length(&mut out, new_len, file.wide)?;
        out.extend_from_slice(file.body);
        out.extend_from_slice(&tag);
        out.extend_from_slice(file.after);
        Ok(out)
    }
}
fn put_length(out: &mut Vec<u8>, n: usize, width: usize) -> Result<()> {
    if width == 4 {
        out.extend_from_slice(
            &u32::try_from(n)
                .map_err(|_| SmartError::Unsupported("PSD block exceeds 32-bit length"))?
                .to_be_bytes(),
        );
    } else {
        out.extend_from_slice(&(n as u64).to_be_bytes());
    }
    Ok(())
}
fn put_unicode(out: &mut Vec<u8>, text: &str) {
    let units: Vec<_> = text.encode_utf16().collect();
    out.extend_from_slice(&(units.len() as u32).to_be_bytes());
    for v in units {
        out.extend_from_slice(&v.to_be_bytes());
    }
}
fn embedded_record(id: &str, png: &[u8]) -> Result<Vec<u8>> {
    let mut body = b"liFD".to_vec();
    body.extend_from_slice(&2u32.to_be_bytes());
    body.push(id.len() as u8);
    body.extend_from_slice(id.as_bytes());
    put_unicode(&mut body, "Emulsion source.png");
    body.extend_from_slice(b"PNG \0\0\0\0");
    put_length(&mut body, png.len(), 8)?;
    body.push(0); // No open-file descriptor: PNG has no layer-comp selection.
    body.extend_from_slice(png);
    let mut out = Vec::new();
    put_length(&mut out, body.len(), 8)?;
    out.extend_from_slice(&body);
    out.resize(out.len() + (4 - body.len() % 4) % 4, 0);
    Ok(out)
}
fn same_pixels(a: &Raster, b: &Raster) -> bool {
    if (a.width(), a.height()) != (b.width(), b.height()) {
        return false;
    }
    if a.content_id() == b.content_id() {
        return true;
    }
    let (columns, rows) = a.tiles_at(0);
    for ty in 0..rows {
        for tx in 0..columns {
            let at = a.base_tile(TileCoord::new(tx, ty));
            let bt = b.base_tile(TileCoord::new(tx, ty));
            if at.zip(bt).is_some_and(|(a, b)| Arc::ptr_eq(a, b)) {
                continue;
            }
            if at.is_none() && bt.is_none() && a.fill() == b.fill() {
                continue;
            }
            let width = TILE.min(a.width() - tx as u32 * TILE);
            let height = TILE.min(a.height() - ty as u32 * TILE);
            for y in 0..height {
                for x in 0..width {
                    let i = (y * TILE + x) as usize;
                    if at.map_or(a.fill(), |tile| tile[i]) != bt.map_or(b.fill(), |tile| tile[i]) {
                        return false;
                    }
                }
            }
        }
    }
    true
}
/// Expensive work belongs on the existing export background worker. Unsupported
/// source state may select appearance fallback. Unavailable entropy aborts the
/// export instead; no partly editable or silently flattened file is written.
pub(super) fn prepare_export(doc: &Document) -> Result<ExportSources> {
    prepare_export_with_entropy(doc, |bytes| {
        getrandom::fill(bytes)
            .map_err(|_| SmartError::Unavailable("system randomness is unavailable"))
    })
}

fn prepare_export_with_entropy(
    doc: &Document,
    mut entropy: impl FnMut(&mut [u8]) -> Result<()>,
) -> Result<ExportSources> {
    let mut out = ExportSources::default();
    let mut budget = SourceBudget::default();
    let mut ids = BTreeSet::new();
    // Check every source before creating any RGBA8 planes or reconstruction
    // rasters. This also bounds worst-case padded allocation, not just pixels.
    for node in &doc.nodes {
        let NodeKind::Smart {
            source,
            original_image,
            ..
        } = &node.kind
        else {
            continue;
        };
        if !can_export(node) {
            return Err(SmartError::Unsupported(
                "native Smart state outside source-only subset",
            ));
        }
        if !ids.insert(node.id) {
            return Err(SmartError::Malformed("duplicate native node ID"));
        }
        if ids.len() > MAX_SOURCES {
            return Err(SmartError::Unsupported("Smart source count limit"));
        }
        budget.charge(
            source.width(),
            source.height(),
            original_image
                .as_ref()
                .map_or(0, |original| original.bytes().len()),
        )?;
    }
    // Prepare all IDs before encoding any source. A failure returns no partial
    // export and never changes the original document or its retained PNG bytes.
    let mut used_uuids = BTreeSet::new();
    let mut prepared_ids = BTreeMap::new();
    for id in ids {
        let source = fresh_uuid_v4(&mut used_uuids, &mut entropy)?;
        let instance = fresh_uuid_v4(&mut used_uuids, &mut entropy)?;
        prepared_ids.insert(id, (source, instance));
    }
    let mut total_bytes = 0usize;
    for node in &doc.nodes {
        let NodeKind::Smart {
            source,
            original_image,
            cache,
            placement,
            ..
        } = &node.kind
        else {
            continue;
        };
        if !can_export(node) {
            return Err(SmartError::Unsupported(
                "native Smart state outside source-only subset",
            ));
        }
        if out.layers.len() >= MAX_SOURCES {
            return Err(SmartError::Unsupported("Smart source count limit"));
        }
        if !same_pixels(source, cache) {
            return Err(SmartError::Unsupported(
                "empty filter stack cache differs from source",
            ));
        }
        let placement = placement.legacy().ok_or(SmartError::Unsupported(
            "projective Smart placement requires appearance export",
        ))?;
        let pixels = source.to_srgba8();
        let png = if let Some(original) = original_image {
            crate::original_image_data::validate(original, source)
                .map_err(|_| SmartError::Unsupported("stale or invalid original PNG source"))?;
            original.bytes().as_ref().clone()
        } else {
            // Native sources without provenance must round-trip exactly through
            // RGBA8. Retained originals can also preserve hidden/low-alpha RGB.
            let reconstructed = Raster::from_srgba8(source.width(), source.height(), &pixels);
            if !same_pixels(source, &reconstructed) {
                return Err(SmartError::Unsupported(
                    "Smart source requires more than RGBA8 precision",
                ));
            }
            let mut png = Vec::new();
            image::codecs::png::PngEncoder::new(&mut png)
                .write_image(
                    &pixels,
                    source.width(),
                    source.height(),
                    image::ExtendedColorType::Rgba8,
                )
                .map_err(|_| SmartError::Unsupported("could not encode embedded source"))?;
            png
        };
        total_bytes = total_bytes
            .checked_add(png.len())
            .ok_or(SmartError::Unsupported("source byte total overflow"))?;
        if png.len() > MAX_SOURCE_BYTES || total_bytes > MAX_TOTAL_SOURCE_BYTES {
            return Err(SmartError::Unsupported("embedded PNG byte limit"));
        }
        let (id, placed) = prepared_ids
            .remove(&node.id)
            .ok_or(SmartError::Malformed("missing prepared Smart UUIDs"))?;
        out.linked.extend_from_slice(&embedded_record(&id, &png)?);
        let (x, y, w, h) = (
            placement.x,
            placement.y,
            f64::from(source.width()),
            f64::from(source.height()),
        );
        let px = |value| UnitsValue {
            units: Units::Pixels,
            value,
        };
        let p = PlacedLayer {
            id: id.clone(),
            placed: Some(placed.clone()),
            layer_type: Some(PlacedLayerType::Raster),
            page_number: Some(1.0),
            total_pages: Some(1.0),
            transform: vec![x, y, x + w, y, x + w, y + h, x, y + h],
            width: Some(w),
            height: Some(h),
            resolution: Some(UnitsValue {
                units: Units::Density,
                value: 72.0,
            }),
            warp: Some(Warp {
                style: Some(WarpStyle::None),
                bounds: Some(UnitsBounds {
                    top: px(0.0),
                    left: px(0.0),
                    bottom: px(h),
                    right: px(w),
                }),
                u_order: Some(4.0),
                v_order: Some(4.0),
                ..Default::default()
            }),
            ..Default::default()
        };
        if out
            .layers
            .insert(
                node.id,
                ExportSource {
                    source_id: id,
                    instance_id: placed,
                    placed: p,
                    pixels: PixelData {
                        width: source.width(),
                        height: source.height(),
                        data: pixels,
                    },
                },
            )
            .is_some()
        {
            return Err(SmartError::Malformed("duplicate native node ID"));
        }
    }
    Ok(out)
}

#[cfg(test)]
#[path = "smart_objects_tests.rs"]
mod tests;
