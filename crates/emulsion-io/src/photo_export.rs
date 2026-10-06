//! Reusable Library output presets. Originals are never modified.
use crate::{IoError, Result};
use emulsion_core::{Document, Node};
use emulsion_raster::{Raster, composite::flatten};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OutputSettings {
    /// Zero preserves dimensions; never enlarges an image.
    pub long_edge: u32,
    pub color_space: crate::photo_color::Space,
    pub metadata: crate::photo_metadata::Policy,
    pub publish: Option<crate::photo_publish::Destination>,
    pub jpeg_quality: u8,
    /// 0 follows source, otherwise 8 or 16.
    pub depth: u8,
    pub sharpening: f32,
    pub watermark: Option<PathBuf>,
    pub watermark_opacity: f32,
    pub watermark_width: f32,
}
impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            long_edge: 0,
            color_space: Default::default(),
            metadata: Default::default(),
            publish: None,
            jpeg_quality: 92,
            depth: 0,
            sharpening: 0.,
            watermark: None,
            watermark_opacity: 0.7,
            watermark_width: 0.2,
        }
    }
}
impl OutputSettings {
    pub fn validate(&self) -> Result<()> {
        if let Some(destination) = &self.publish {
            destination.validate()?;
        }
        if self.long_edge > emulsion_core::document::MAX_SIDE
            || !(1..=100).contains(&self.jpeg_quality)
            || ![0, 8, 16].contains(&self.depth)
            || !self.sharpening.is_finite()
            || !(0.0..=1.0).contains(&self.sharpening)
            || !self.watermark_opacity.is_finite()
            || !(0.0..=1.0).contains(&self.watermark_opacity)
            || !self.watermark_width.is_finite()
            || !(0.01..=1.0).contains(&self.watermark_width)
        {
            return Err(IoError::Manifest("Invalid export settings".into()));
        }
        Ok(())
    }
    pub fn prepare(&self, doc: &Document) -> Result<Document> {
        self.prepare_in_space(doc, crate::photo_color::Space::Srgb)
    }
    pub fn prepare_in_space(
        &self,
        doc: &Document,
        working: crate::photo_color::Space,
    ) -> Result<Document> {
        self.validate()?;
        if self.long_edge == 0 && self.sharpening == 0. && self.watermark.is_none() {
            let mut doc = doc.clone();
            if self.depth != 0 {
                doc.source_depth = self.depth;
            }
            return Ok(doc);
        }
        let mut raster = flatten(&doc.try_composite_tree()?, 0);
        let (w, h) = (raster.width(), raster.height());
        if self.long_edge > 0 && w.max(h) > self.long_edge {
            let scale = self.long_edge as f64 / w.max(h) as f64;
            let (ow, oh) = (
                (w as f64 * scale).round().max(1.) as u32,
                (h as f64 * scale).round().max(1.) as u32,
            );
            raster = resize(&raster, ow, oh)?;
        }
        if self.sharpening > 0. {
            raster = crate::raw::develop_raster(
                &raster,
                &emulsion_core::raw::DevelopParams {
                    sharpening: self.sharpening,
                    ..Default::default()
                },
            )?;
        }
        if let Some(path) = &self.watermark {
            let mark = crate::photo_color::convert_raster(
                crate::import::decode(path)?.raster,
                crate::photo_color::Space::Srgb,
                working,
            )?;
            let width = (raster.width() as f32 * self.watermark_width)
                .round()
                .max(1.) as u32;
            let height = (width as f64 * mark.height() as f64 / mark.width() as f64)
                .round()
                .max(1.) as u32;
            let height = height.min(raster.height());
            let mark = resize(&mark, width, height)?;
            let (w, h) = (raster.width(), raster.height());
            let margin = (w.min(h) / 50)
                .min(w.saturating_sub(width))
                .min(h.saturating_sub(height));
            let (x0, y0) = (w - width - margin, h - height - margin);
            let mut pixels = raster.to_pixels();
            for y in 0..height {
                for x in 0..width {
                    let p = mark.get(x, y);
                    let out = &mut pixels[((y0 + y) * w + x0 + x) as usize];
                    let a = p[3] as f32 / 65535. * self.watermark_opacity;
                    let da = out[3] as f32 / 65535.;
                    let oa = a + da * (1. - a);
                    for c in 0..3 {
                        out[c] = (p[c] as f32 * self.watermark_opacity + out[c] as f32 * (1. - a))
                            .round()
                            .clamp(0., 65535.) as u16;
                    }
                    out[3] = (oa * 65535.).round() as u16;
                }
            }
            raster = Raster::from_pixels(w, h, [0; 4], &pixels);
        }
        let mut output = Document::new(raster.width(), raster.height());
        output.source_depth = if self.depth == 0 {
            doc.source_depth
        } else {
            self.depth
        };
        output.nodes.push(Node::raster(
            1,
            "Export",
            Arc::new(raster),
            Default::default(),
        ));
        output.next_id = 2;
        Ok(output)
    }
}
pub(crate) fn resize(raster: &Raster, w: u32, h: u32) -> Result<Raster> {
    crate::import::check_size(w, h)?;
    // Raster stores premultiplied linear RGBA. Filter that representation directly.
    let pixels: Vec<f32> = raster
        .to_pixels()
        .into_iter()
        .flatten()
        .map(|v| v as f32 / 65535.)
        .collect();
    let image = image::ImageBuffer::<image::Rgba<f32>, _>::from_raw(
        raster.width(),
        raster.height(),
        pixels,
    )
    .ok_or_else(|| IoError::Manifest("Invalid resize source".into()))?;
    let result = image::imageops::resize(&image, w, h, image::imageops::FilterType::Lanczos3);
    let pixels: Vec<[u16; 4]> = result
        .pixels()
        .map(|p| {
            let a = p[3].clamp(0., 1.);
            [
                (p[0].clamp(0., a) * 65535.).round() as u16,
                (p[1].clamp(0., a) * 65535.).round() as u16,
                (p[2].clamp(0., a) * 65535.).round() as u16,
                (a * 65535.).round() as u16,
            ]
        })
        .collect();
    Ok(Raster::from_pixels(w, h, [0; 4], &pixels))
}

pub fn save_preset(path: &std::path::Path, settings: &OutputSettings) -> Result<()> {
    settings.validate()?;
    if path
        .extension()
        .is_none_or(|s| !s.eq_ignore_ascii_case("json"))
    {
        return Err(IoError::Manifest("Export presets require .json".into()));
    }
    if path.exists() {
        load_preset(path)?;
    }
    let bytes = serde_json::to_vec_pretty(
        &serde_json::json!({"format":"emulsion-export-preset","version":1,"settings":settings}),
    )
    .map_err(|e| IoError::Manifest(e.to_string()))?;
    let parent = path.parent().unwrap_or(std::path::Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    use std::io::Write;
    temp.write_all(&bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}
pub fn load_preset(path: &std::path::Path) -> Result<OutputSettings> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(65537)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        return Err(IoError::Manifest("Export preset too large".into()));
    }
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| IoError::Manifest(e.to_string()))?;
    if value["format"] != "emulsion-export-preset" || value["version"] != 1 {
        return Err(IoError::Manifest("Not an Emulsion export preset".into()));
    }
    let settings: OutputSettings = serde_json::from_value(value["settings"].clone())
        .map_err(|e| IoError::Manifest(e.to_string()))?;
    settings.validate()?;
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resize_watermark_and_presets_preserve_original_and_depth() {
        let dir = tempfile::tempdir().unwrap();
        let mark = dir.path().join("mark.png");
        std::fs::write(
            &mark,
            crate::export::png8(2, 1, &[255, 0, 0, 255, 255, 0, 0, 255]).unwrap(),
        )
        .unwrap();
        let mut doc = Document::new(40, 20);
        doc.source_depth = 16;
        doc.nodes.push(Node::raster(
            1,
            "Source",
            Arc::new(Raster::solid(40, 20, [0.1, 0.2, 0.3, 1.])),
            Default::default(),
        ));
        doc.next_id = 2;
        let original = doc.clone();
        let settings = OutputSettings {
            long_edge: 20,
            watermark: Some(mark),
            watermark_opacity: 1.,
            watermark_width: 0.2,
            ..Default::default()
        };
        let out = settings.prepare(&doc).unwrap();
        assert_eq!((out.width, out.height, out.source_depth), (20, 10, 16));
        assert_eq!(doc, original);
        let pixels = flatten(&out.composite_tree(), 0);
        assert!(pixels.get(18, 9)[0] > 60000);
        assert!(pixels.get(0, 0)[0] < 10000);
        let preset = dir.path().join("small.json");
        save_preset(&preset, &settings).unwrap();
        assert_eq!(load_preset(&preset).unwrap(), settings);
        let unrelated = dir.path().join("other.json");
        std::fs::write(&unrelated, b"{}").unwrap();
        assert!(save_preset(&unrelated, &settings).is_err());
    }
}
