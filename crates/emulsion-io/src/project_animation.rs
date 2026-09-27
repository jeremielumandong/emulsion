//! Deterministic page motion export with bounded frame count and memory.
use crate::{IoError, Result};
use emulsion_core::project::Project;
use std::path::Path;
pub fn write_gif(project: &Project, path: &Path) -> Result<usize> {
    project.validate().map_err(IoError::Manifest)?;
    let frames: usize = project
        .pages
        .iter()
        .map(|p| {
            (u64::from(p.doc.design.duration_ms) * u64::from(p.doc.design.fps)).div_ceil(1000)
                as usize
        })
        .sum();
    if frames > 6000 {
        return Err(IoError::Manifest(
            "Animation export supports up to 6,000 frames. Reduce durations or frame rates.".into(),
        ));
    }
    for page in &project.pages {
        crate::ora::ensure_not_raw_original(&page.doc, path)?;
        for commit in page.graph.commits() {
            crate::ora::ensure_not_raw_original(&commit.doc, path)?;
        }
    }
    let first = &project.pages[0].doc;
    let scale = 800. / first.width.max(first.height) as f64;
    let size = (
        (first.width as f64 * scale.min(1.)).round().max(1.) as u32,
        (first.height as f64 * scale.min(1.)).round().max(1.) as u32,
    );
    crate::write_atomic(path, |file| {
        use image::codecs::gif::{GifEncoder, Repeat};
        let mut encoder = GifEncoder::new_with_speed(file, 10);
        encoder.set_repeat(Repeat::Infinite)?;
        for page in &project.pages {
            let source = crate::export::develop_document(&page.doc)?;
            let settings = &source.design;
            let count = (u64::from(settings.duration_ms) * u64::from(settings.fps)).div_ceil(1000);
            for frame in 0..count {
                let time = (frame * 1000 / u64::from(settings.fps)) as u32;
                let doc = emulsion_core::design_metadata::at_time(&source, time)
                    .map_err(IoError::Manifest)?;
                let mut level = 0;
                while (doc.width.max(doc.height) >> level) > 1600 && level < 8 {
                    level += 1;
                }
                let raster = emulsion_raster::composite::flatten(&doc.composite_tree(), level);
                let image =
                    image::RgbaImage::from_raw(raster.width(), raster.height(), raster.to_srgba8())
                        .ok_or_else(|| {
                            IoError::Manifest("Invalid rendered animation frame".into())
                        })?;
                let ratio = (size.0 as f64 / image.width() as f64)
                    .min(size.1 as f64 / image.height() as f64);
                let image = image::imageops::resize(
                    &image,
                    (image.width() as f64 * ratio).round().max(1.) as u32,
                    (image.height() as f64 * ratio).round().max(1.) as u32,
                    image::imageops::FilterType::Triangle,
                );
                let mut canvas =
                    image::RgbaImage::from_pixel(size.0, size.1, image::Rgba([255, 255, 255, 255]));
                let x = (size.0 - image.width()) / 2;
                let y = (size.1 - image.height()) / 2;
                image::imageops::overlay(&mut canvas, &image, x as i64, y as i64);
                let next = ((frame + 1) * 1000 / u64::from(settings.fps))
                    .min(u64::from(settings.duration_ms)) as u32;
                let delay = image::Delay::from_numer_denom_ms((next - time).max(1), 1);
                encoder.encode_frame(image::Frame::from_parts(canvas, 0, 0, delay))?;
            }
        }
        Ok(())
    })?;
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::{
        Command, Document, Node, NodeKind,
        command::Slot,
        design_metadata::{Effect, Motion},
        project::{ProjectEditor, ProjectKind},
    };
    use image::AnimationDecoder;
    #[test]
    fn exported_frames_follow_saved_timing_and_do_not_mutate_the_project() {
        let mut editor =
            ProjectEditor::new_project(ProjectKind::Design, Document::new(16, 16)).unwrap();
        let id = editor
            .execute(Command::AddNode {
                node: Box::new(Node::new(
                    0,
                    "Animated red",
                    NodeKind::Fill {
                        rgba: [255, 0, 0, 255],
                    },
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let mut design = editor.doc.design.clone();
        design.duration_ms = 1000;
        design.fps = 4;
        design.motion.insert(
            id,
            Motion {
                enter: Effect::Fade,
                end_ms: 1000,
                transition_ms: 500,
                ..Default::default()
            },
        );
        editor
            .execute(Command::SetDesign {
                design: Box::new(design),
            })
            .unwrap();
        let project = editor.snapshot().unwrap();
        let before = editor.doc.clone();
        let path =
            std::env::temp_dir().join(format!("emulsion-motion-export-{}.gif", std::process::id()));
        assert_eq!(write_gif(&project, &path).unwrap(), 4);
        let decoder = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(
            std::fs::File::open(&path).unwrap(),
        ))
        .unwrap();
        let frames = decoder.into_frames().collect_frames().unwrap();
        assert_eq!(frames.len(), 4);
        assert_eq!(frames[0].buffer().get_pixel(8, 8).0, [255, 255, 255, 255]);
        assert_eq!(frames[2].buffer().get_pixel(8, 8).0, [255, 0, 0, 255]);
        assert_eq!(frames[0].delay().numer_denom_ms(), (250, 1));
        assert_eq!(editor.doc, before);
        std::fs::remove_file(path).unwrap();
    }
}
