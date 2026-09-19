//! Camera metadata (EXIF) from JPEG, TIFF, PNG, WebP and HEIF through
//! kamadak-exif, and from camera RAW files through rawler.

use emulsion_core::document::ImageInfo;
use exif::{In, Tag, Value};
use std::path::Path;

fn rational(v: &Value) -> Option<f32> {
    match v {
        Value::Rational(r) => r.first().map(|r| r.to_f32()),
        Value::SRational(r) => r.first().map(|r| r.to_f32()),
        Value::Short(s) => s.first().map(|s| *s as f32),
        Value::Long(l) => l.first().map(|l| *l as f32),
        _ => None,
    }
}

fn text(v: &Value) -> String {
    match v {
        Value::Ascii(parts) => parts
            .iter()
            .map(|p| {
                String::from_utf8_lossy(p)
                    .trim_matches(char::from(0))
                    .trim()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string(),
        other => other.display_as(Tag::Model).to_string(),
    }
}

/// Read what a standard image file says about its capture; `None` when
/// there is no EXIF at all.
pub fn read(path: &Path) -> Option<ImageInfo> {
    if crate::raw::is_raw(path) {
        return read_raw(path);
    }
    let file = std::fs::File::open(path).ok()?;
    let mut reader = std::io::BufReader::new(file);
    let exif = exif::Reader::new().read_from_container(&mut reader).ok()?;
    let get = |tag: Tag| exif.get_field(tag, In::PRIMARY).map(|f| &f.value);
    let mut info = ImageInfo::default();
    if let Some(v) = get(Tag::Make) {
        info.make = text(v);
    }
    if let Some(v) = get(Tag::Model) {
        info.model = text(v);
    }
    if let Some(v) = get(Tag::LensModel) {
        info.lens = text(v);
    }
    if info.lens.is_empty()
        && let Some(v) = get(Tag::LensSpecification)
        && let Value::Rational(r) = v
        && r.len() >= 4
    {
        let (a, b) = (r[0].to_f32(), r[1].to_f32());
        info.lens = if (a - b).abs() < 0.01 {
            format!("{a:.0}mm")
        } else {
            format!("{a:.0}-{b:.0}mm")
        };
    }
    info.focal_mm = get(Tag::FocalLength).and_then(rational).unwrap_or(0.0);
    info.focal_35mm = get(Tag::FocalLengthIn35mmFilm)
        .and_then(rational)
        .unwrap_or(0.0);
    info.f_number = get(Tag::FNumber).and_then(rational).unwrap_or(0.0);
    info.exposure_s = get(Tag::ExposureTime).and_then(rational).unwrap_or(0.0);
    info.iso = get(Tag::PhotographicSensitivity)
        .and_then(rational)
        .map(|v| v as u32)
        .unwrap_or(0);
    if let Some(v) = get(Tag::DateTimeOriginal) {
        info.taken = text(v);
    }
    if let Some(v) = get(Tag::Software) {
        info.software = text(v);
    }
    // "----", "0.0 mm" and the like mean no lens was reported.
    if info.lens.chars().all(|c| !c.is_alphanumeric() || c == '0') {
        info.lens.clear();
    }
    // Model strings often repeat the make ("Canon Canon EOS R5").
    if !info.make.is_empty()
        && info
            .model
            .to_lowercase()
            .starts_with(&info.make.to_lowercase())
    {
        info.model = info.model[info.make.len()..].trim().to_string();
    }
    let any = !info.make.is_empty()
        || !info.model.is_empty()
        || info.focal_mm > 0.0
        || info.f_number > 0.0
        || info.iso > 0;
    any.then_some(info)
}

fn read_raw(path: &Path) -> Option<ImageInfo> {
    let src = rawler::rawsource::RawSource::new(path).ok()?;
    let dec = rawler::get_decoder(&src).ok()?;
    let md = dec
        .raw_metadata(&src, &rawler::decoders::RawDecodeParams::default())
        .ok()?;
    let e = &md.exif;
    let mut info = ImageInfo {
        make: md.make.clone(),
        model: md.model.clone(),
        ..Default::default()
    };
    if let Some(l) = &md.lens {
        info.lens = format!("{} {}", l.lens_make, l.lens_model)
            .trim()
            .to_string();
    }
    if info.lens.is_empty() {
        if let Some(m) = &e.lens_model {
            info.lens = m.trim().to_string();
        }
        if let (true, Some(spec)) = (info.lens.is_empty(), &e.lens_spec) {
            let (a, b) = (spec[0].as_f32(), spec[1].as_f32());
            info.lens = if (a - b).abs() < 0.01 {
                format!("{a:.0}mm")
            } else {
                format!("{a:.0}-{b:.0}mm")
            };
        }
    }
    info.focal_mm = e.focal_length.map(|r| r.as_f32()).unwrap_or(0.0);
    info.f_number = e.fnumber.map(|r| r.as_f32()).unwrap_or(0.0);
    info.exposure_s = e.exposure_time.map(|r| r.as_f32()).unwrap_or(0.0);
    info.iso = e.iso_speed_ratings.map(|v| v as u32).unwrap_or(0);
    info.taken = e.date_time_original.clone().unwrap_or_default();
    if info.lens.chars().all(|c| !c.is_alphanumeric() || c == '0') {
        info.lens.clear();
    }
    Some(info)
}
