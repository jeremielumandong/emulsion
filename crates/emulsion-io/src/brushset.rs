//! Import Procreate brushes. A `.brushset` is a zip of folders, one per
//! brush, each holding `Brush.archive` (an NSKeyedArchiver property list
//! with the settings), `Shape.png` (the tip, alpha or grey) and
//! `Grain.png` (the paper texture); a single `.brush` is one such folder
//! zipped. The settings that have an Emulsion counterpart are mapped, the
//! shape and grain become image textures, and the rest is left at
//! sensible defaults — the textures are what make a brush recognisable.

use crate::{IoError, Result};
use emulsion_raster::library::BrushPreset;
use emulsion_raster::paint::{Brush, BrushBlend, GrainKind, textures};
use plist::Value;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

/// One imported brush with its texture images (PNG bytes).
pub struct Imported {
    pub preset: BrushPreset,
    pub shape_png: Option<Vec<u8>>,
    pub grain_png: Option<Vec<u8>>,
}

/// Resolve an NSKeyedArchiver plist into its root object as a flat map
/// of key → value, following `$objects` references one level deep.
fn keyed_root(v: &Value) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    let Some(dict) = v.as_dictionary() else {
        // A plain dictionary (not keyed-archived) is taken as is.
        return out;
    };
    let Some(objects) = dict.get("$objects").and_then(Value::as_array) else {
        for (k, v) in dict {
            out.insert(k.clone(), v.clone());
        }
        return out;
    };
    // Binary archives carry real UIDs; XML ones spell them as {CF$UID: n}.
    let uid_of = |v: &Value| -> Option<usize> {
        match v {
            Value::Uid(u) => Some(u.get() as usize),
            Value::Dictionary(d) if d.len() == 1 => d
                .get("CF$UID")
                .and_then(Value::as_signed_integer)
                .map(|n| n as usize),
            _ => None,
        }
    };
    let resolve = |v: &Value| -> Value {
        match uid_of(v) {
            Some(i) => objects
                .get(i)
                .cloned()
                .unwrap_or(Value::String(String::new())),
            None => v.clone(),
        }
    };
    let root = dict
        .get("$top")
        .and_then(Value::as_dictionary)
        .and_then(|t| t.get("root"))
        .and_then(uid_of)
        .and_then(|i| objects.get(i).cloned());
    if let Some(Value::Dictionary(r)) = root {
        for (k, v) in &r {
            if k.starts_with('$') {
                continue;
            }
            let rv = resolve(v);
            // Strings and numbers are all the importer needs; skip nested objects.
            match rv {
                Value::String(_) | Value::Real(_) | Value::Integer(_) | Value::Boolean(_) => {
                    out.insert(k.clone(), rv);
                }
                _ => {}
            }
        }
    }
    out
}

fn num(m: &HashMap<String, Value>, keys: &[&str]) -> Option<f32> {
    keys.iter().find_map(|k| {
        m.get(*k).and_then(|v| match v {
            Value::Real(r) => Some(*r as f32),
            Value::Integer(i) => i.as_signed().map(|i| i as f32),
            Value::Boolean(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        })
    })
}

fn text(m: &HashMap<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| m.get(*k).and_then(Value::as_string).map(str::to_string))
}

/// Map Procreate's settings onto a `Brush`. Procreate stores most values
/// as fractions of their slider range, so they land here as 0–1.
fn brush_from(m: &HashMap<String, Value>, has_shape: bool, has_grain: bool) -> Brush {
    let d = Brush::default();
    let unit = |v: Option<f32>, default: f32| v.map(|x| x.clamp(0.0, 1.0)).unwrap_or(default);
    // Size: Procreate's "paintSize"/"maxSize" are fractions of the maximum
    // brush size; 1.0 is a very large brush.
    let size_frac = unit(num(m, &["paintSize", "maxSize", "size"]), 0.15);
    let spacing = unit(num(m, &["plotSpacing", "spacing"]), 0.1);
    let opacity = unit(num(m, &["paintOpacity", "maxOpacity", "opacity"]), 1.0);
    let flow = unit(num(m, &["paintFlow", "flow"]), 1.0);
    let hardness = unit(num(m, &["shapeHardness", "hardness"]), 0.6);
    let grain_depth = unit(num(m, &["grainDepth", "grainIntensity"]), 0.6);
    let grain_scale = unit(num(m, &["grainScale", "grainZoom"]), 0.5);
    let rotation = num(m, &["shapeRotation", "shapeAngle"]).unwrap_or(0.0);
    let follow = num(m, &["shapeOrientToStroke", "shapeAzimuth"]).is_some_and(|v| v > 0.5);
    let size_pressure = unit(num(m, &["dynamicsPressureSize", "pressureSize"]), 0.0);
    let flow_pressure = unit(
        num(
            m,
            &[
                "dynamicsPressureOpacity",
                "pressureOpacity",
                "dynamicsPressureFlow",
            ],
        ),
        0.0,
    );
    let jitter = unit(num(m, &["shapeScatter", "scatter"]), 0.0);
    let taper_len = unit(
        num(m, &["taperSize", "taperStartLength", "taperLength"]),
        0.0,
    );
    let wet = unit(
        num(m, &["wetMix", "dynamicsMix", "smudgeAmount", "dilution"]),
        0.0,
    );
    let multiply = num(m, &["blendMode", "paintBlendMode"]).is_some_and(|v| (v - 1.0).abs() < 0.5);
    Brush {
        size: (4.0 + size_frac * 200.0).round(),
        hardness: if has_shape { hardness * 0.6 } else { hardness },
        opacity: opacity.max(0.05),
        flow: flow.max(0.05),
        spacing: (spacing * 0.5).clamp(0.02, 1.0),
        roundness: 1.0,
        angle: rotation * 360.0,
        follow_path: follow,
        grain: if has_grain {
            GrainKind::Paper
        } else {
            GrainKind::None
        },
        grain_scale: (2.0 + grain_scale * 14.0),
        grain_strength: if has_grain { grain_depth } else { 0.0 },
        size_pressure,
        flow_pressure,
        size_jitter: jitter * 0.5,
        scatter: jitter,
        taper_start: taper_len * 0.5,
        taper_end: taper_len * 0.5,
        wetness: wet,
        blend: if multiply {
            BrushBlend::Multiply
        } else {
            BrushBlend::Normal
        },
        ..d
    }
}

fn read_entry<R: Read + std::io::Seek>(z: &mut zip::ZipArchive<R>, name: &str) -> Option<Vec<u8>> {
    let f = z.by_name(name).ok()?;
    let mut bytes = Vec::new();
    f.take(64 << 20).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// Import a `.brushset` or `.brush` file.
pub fn import(path: &Path) -> Result<Vec<Imported>> {
    let file = std::fs::File::open(path)?;
    let mut z = zip::ZipArchive::new(std::io::BufReader::new(file))
        .map_err(|e| IoError::Unsupported(format!("not a brush archive: {e}")))?;
    // Group entries by folder: "" for a single .brush, "Name/" in a set.
    let mut folders: Vec<String> = Vec::new();
    for i in 0..z.len() {
        let Ok(f) = z.by_index(i) else { continue };
        let name = f.name().to_string();
        if name.ends_with("Brush.archive") {
            let dir = name.trim_end_matches("Brush.archive").to_string();
            if !folders.contains(&dir) {
                folders.push(dir);
            }
        }
    }
    if folders.is_empty() {
        return Err(IoError::Unsupported("no Brush.archive inside".into()));
    }
    let set_name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Imported".into());
    let mut out = Vec::new();
    for dir in folders {
        let Some(archive) = read_entry(&mut z, &format!("{dir}Brush.archive")) else {
            continue;
        };
        let settings = plist::from_bytes::<Value>(&archive)
            .map(|v| keyed_root(&v))
            .unwrap_or_default();
        let shape_png = read_entry(&mut z, &format!("{dir}Shape.png"));
        let grain_png = read_entry(&mut z, &format!("{dir}Grain.png"));
        let mut brush = brush_from(&settings, shape_png.is_some(), grain_png.is_some());
        if let Some(png) = &shape_png {
            brush.tip = textures::id_for(png);
        }
        if let Some(png) = &grain_png {
            brush.grain_tex = textures::id_for(png);
        }
        let folder_name = dir
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_string();
        let name = text(&settings, &["name", "bundledBrushName", "brushName"])
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| {
                if folder_name.is_empty() {
                    set_name.clone()
                } else {
                    folder_name.clone()
                }
            });
        out.push(Imported {
            preset: BrushPreset {
                name,
                category: format!("Imported · {set_name}"),
                note: format!(
                    "From {}",
                    path.file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default()
                ),
                brush: brush.sanitized(),
            },
            shape_png,
            grain_png,
        });
    }
    Ok(out)
}

/// Decode a texture PNG (alpha if it has one, else luminance) and put it
/// in the registry under its id. Returns the id.
pub fn register_texture(png: &[u8]) -> Result<u32> {
    let img = image::load_from_memory(png)?;
    let id = textures::id_for(png);
    let (w, h) = (img.width(), img.height());
    let rgba = img.to_rgba8();
    let has_alpha = rgba.pixels().any(|p| p.0[3] < 255);
    let gray: Vec<u8> = rgba
        .pixels()
        .map(|p| {
            if has_alpha {
                p.0[3]
            } else {
                ((p.0[0] as u32 * 54 + p.0[1] as u32 * 183 + p.0[2] as u32 * 19) / 256) as u8
            }
        })
        .collect();
    let texture = textures::Texture::from_gray8(w, h, &gray)
        .ok_or_else(|| IoError::Unsupported("texture image is empty".into()))?;
    textures::register(id, texture);
    Ok(id)
}

/// Where imported textures live.
pub fn textures_dir() -> std::path::PathBuf {
    crate::recent::data_dir().join("brushes").join("textures")
}

/// Save a texture PNG under its id and register it.
pub fn store_texture(png: &[u8]) -> Result<u32> {
    let id = register_texture(png)?;
    let dir = textures_dir();
    std::fs::create_dir_all(&dir)?;
    let dest = dir.join(format!("{id}.png"));
    if !dest.exists() {
        std::fs::write(dest, png)?;
    }
    Ok(id)
}

/// Load every stored texture into the registry (at startup).
pub fn load_textures() -> usize {
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir(textures_dir()) {
        for e in rd.flatten() {
            if let Ok(bytes) = std::fs::read(e.path())
                && register_texture(&bytes).is_ok()
            {
                n += 1;
            }
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn png_gray(w: u32, h: u32, f: impl Fn(u32, u32) -> u8) -> Vec<u8> {
        let img = image::GrayImage::from_fn(w, h, |x, y| image::Luma([f(x, y)]));
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn imports_a_synthetic_brushset() {
        // An NSKeyedArchiver-shaped plist, as XML (plist reads both forms).
        let archive = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>$archiver</key><string>NSKeyedArchiver</string>
  <key>$objects</key><array>
    <string>$null</string>
    <dict>
      <key>name</key><integer>2</integer>
      <key>paintSize</key><real>0.25</real>
      <key>plotSpacing</key><real>0.2</real>
      <key>grainDepth</key><real>0.8</real>
      <key>dynamicsPressureSize</key><real>1</real>
      <key>paintOpacity</key><real>0.9</real>
    </dict>
    <string>Soft Charcoal</string>
  </array>
  <key>$top</key><dict><key>root</key><integer>1</integer></dict>
</dict></plist>"#;
        // plist's XML form has no UID type; emulate by replacing integers 1/2 references
        // with UID markers the crate understands ("<dict><key>CF$UID</key><integer>n</integer></dict>").
        let archive = archive
            .replace(
                "<key>name</key><integer>2</integer>",
                "<key>name</key><dict><key>CF$UID</key><integer>2</integer></dict>",
            )
            .replace(
                "<key>root</key><integer>1</integer>",
                "<key>root</key><dict><key>CF$UID</key><integer>1</integer></dict>",
            );
        let shape = png_gray(16, 16, |x, y| {
            if (x as i32 - 8).pow(2) + (y as i32 - 8).pow(2) < 40 {
                255
            } else {
                0
            }
        });
        let grain = png_gray(8, 8, |x, y| ((x + y) % 2 * 200) as u8);
        let dir = std::env::temp_dir().join(format!("emulsion-brushset-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Test.brushset");
        {
            let f = std::fs::File::create(&path).unwrap();
            let mut z = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default();
            z.start_file("Charcoal/Brush.archive", opts).unwrap();
            z.write_all(archive.as_bytes()).unwrap();
            z.start_file("Charcoal/Shape.png", opts).unwrap();
            z.write_all(&shape).unwrap();
            z.start_file("Charcoal/Grain.png", opts).unwrap();
            z.write_all(&grain).unwrap();
            z.start_file("Plain/Brush.archive", opts).unwrap();
            z.write_all(b"not a plist").unwrap();
            z.finish().unwrap();
        }
        let got = import(&path).unwrap();
        assert_eq!(got.len(), 2);
        let b = &got[0];
        assert_eq!(b.preset.name, "Soft Charcoal");
        assert!(b.preset.category.starts_with("Imported"));
        assert!(b.preset.brush.tip != 0 && b.preset.brush.grain_tex != 0);
        assert_eq!(b.preset.brush.size, 54.0);
        assert!((b.preset.brush.size_pressure - 1.0).abs() < 1e-6);
        assert!((b.preset.brush.grain_strength - 0.8).abs() < 1e-6);
        assert_eq!(
            got[1].preset.name, "Plain",
            "unreadable settings fall back to the folder name"
        );
        let id = register_texture(b.shape_png.as_ref().unwrap()).unwrap();
        assert_eq!(id, b.preset.brush.tip);
        let t = textures::get(id).unwrap();
        assert!(t.sample(0.5, 0.5) > 0.9 && t.sample(0.02, 0.02) < 0.1);
        std::fs::remove_dir_all(&dir).ok();
    }
}
