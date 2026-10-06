//! Bounded sampled motion exports with explicit vector/raster diagnostics.
use crate::{IoError, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use emulsion_core::Document;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Digest;
use std::{collections::HashMap, io::Write, path::Path};
const MAX_BYTES: usize = 64 << 20;
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    AnimatedSvg,
    Lottie,
    LottieRaster,
}
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub format: Format,
    pub frames: u32,
    pub fps: u32,
    pub vector_frames: u32,
    pub raster_frames: u32,
    pub bytes: usize,
    pub diagnostics: Vec<String>,
}
fn error(message: impl Into<String>) -> IoError {
    IoError::Manifest(message.into())
}
struct RasterFrame {
    width: u32,
    height: u32,
    png: Vec<u8>,
    fingerprint: [u8; 32],
}
fn raster_frame(doc: &Document) -> Result<RasterFrame> {
    let mut doc = doc.clone();
    let scale = (1024. / f64::from(doc.width.max(doc.height))).min(1.);
    if scale < 1. {
        let w = (f64::from(doc.width) * scale).round().max(1.) as u32;
        let h = (f64::from(doc.height) * scale).round().max(1.) as u32;
        emulsion_core::geometry::resize(&mut doc, w, h)?;
    }
    let raster = emulsion_raster::composite::flatten(&doc.try_composite_tree()?, 0);
    let rgba = raster.to_srgba8();
    // Encoded PNGs may differ because ICC profiles include creation metadata.
    // Deduplicate visual pixels and dimensions, not the encoder's metadata.
    let mut hash = sha2::Sha256::new();
    hash.update(doc.width.to_le_bytes());
    hash.update(doc.height.to_le_bytes());
    hash.update(&rgba);
    let mut fingerprint = [0u8; 32];
    fingerprint.copy_from_slice(&hash.finalize());
    let png = crate::export::png8(doc.width, doc.height, &rgba)?;
    Ok(RasterFrame {
        width: doc.width,
        height: doc.height,
        png,
        fingerprint,
    })
}
pub fn encode(doc: &Document, format: Format) -> Result<(Vec<u8>, Report)> {
    doc.validate().map_err(|e| error(e.to_string()))?;
    if format == Format::Lottie {
        let (bytes, mut vector) = crate::lottie::encode(doc)?;
        vector.diagnostics.insert(
            0,
            format!(
                "{} editable objects, {} animated objects; no rendered animation frames.",
                vector.nodes, vector.animated_nodes
            ),
        );
        let report = Report {
            format,
            frames: (u64::from(doc.design.duration_ms) * u64::from(doc.design.fps)).div_ceil(1000)
                as u32,
            fps: doc.design.fps,
            vector_frames: 0,
            raster_frames: 0,
            bytes: bytes.len(),
            diagnostics: vector.diagnostics,
        };
        return Ok((bytes, report));
    }
    let fps = doc.design.fps;
    let frames = (u64::from(doc.design.duration_ms) * u64::from(fps)).div_ceil(1000) as u32;
    if frames > 600 {
        return Err(error(
            "Motion interchange supports up to 600 frames. Reduce page duration or frame rate.",
        ));
    }
    let mut report=Report{format,frames,fps,vector_frames:0,raster_frames:0,bytes:0,diagnostics:vec!["Active-page sampled animation only; presentation interactions and slide transitions are not exported.".into()]};
    if !doc.design.media.is_empty() || !doc.design.local_media.is_empty() {
        report
            .diagnostics
            .push("Audio/video remain silent poster artwork in this export.".into());
    }
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">",
        doc.width, doc.height, doc.width, doc.height
    );
    let mut layers = Vec::new();
    let mut assets = Vec::new();
    let mut images = HashMap::<[u8; 32], String>::new();
    let mut payload_size = 0usize;
    for index in 0..frames {
        let time = ((u64::from(index) * 1000) / u64::from(fps)) as u32;
        let frame =
            emulsion_core::design_metadata::at_time(doc, time.min(doc.design.duration_ms - 1))
                .map_err(error)?;
        match format {
            Format::AnimatedSvg => {
                let bytes = match crate::project_export::vector_svg(&frame) {
                    Ok(bytes) => {
                        report.vector_frames += 1;
                        bytes
                    }
                    Err(_) => {
                        let RasterFrame {
                            width: w,
                            height: h,
                            png,
                            fingerprint: _,
                        } = raster_frame(&frame)?;
                        report.raster_frames += 1;
                        format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {w} {h}\"><image width=\"{w}\" height=\"{h}\" href=\"data:image/png;base64,{}\"/></svg>",doc.width,doc.height,STANDARD.encode(png)).into_bytes()
                    }
                };
                let (values, times) = if index == 0 {
                    (
                        "1;0;0".to_string(),
                        format!("0;{};1", 1. / f64::from(frames)),
                    )
                } else if index + 1 == frames {
                    (
                        "0;1;1".to_string(),
                        format!("0;{};1", f64::from(index) / f64::from(frames)),
                    )
                } else {
                    (
                        "0;1;0;0".to_string(),
                        format!(
                            "0;{};{};1",
                            f64::from(index) / f64::from(frames),
                            f64::from(index + 1) / f64::from(frames)
                        ),
                    )
                };
                svg.push_str(&format!("<image width=\"{}\" height=\"{}\" href=\"data:image/svg+xml;base64,{}\" opacity=\"{}\">",doc.width,doc.height,STANDARD.encode(bytes),if index==0{1}else{0}));
                if frames > 1 {
                    svg.push_str(&format!("<animate attributeName=\"opacity\" values=\"{values}\" keyTimes=\"{times}\" dur=\"{}ms\" calcMode=\"discrete\" repeatCount=\"indefinite\"/>",doc.design.duration_ms));
                }
                svg.push_str("</image>");
                payload_size = svg.len();
            }
            Format::Lottie => unreachable!(),
            Format::LottieRaster => {
                let RasterFrame {
                    width: w,
                    height: h,
                    png,
                    fingerprint,
                } = raster_frame(&frame)?;
                report.raster_frames += 1;
                let id = if let Some(id) = images.get(&fingerprint) {
                    id.clone()
                } else {
                    let id = format!("frame-{}", assets.len());
                    let encoded = STANDARD.encode(&png);
                    payload_size = payload_size.saturating_add(encoded.len());
                    assets.push(json!({"id":id,"w":w,"h":h,"u":"","p":format!("data:image/png;base64,{encoded}"),"e":1}));
                    images.insert(fingerprint, id.clone());
                    id
                };
                layers.push(json!({"ty":2,"ind":index+1,"nm":format!("Sample {}",index+1),"refId":id,"ip":index,"op":index+1,"st":0,"sr":1,"ks":{"o":{"a":0,"k":100},"r":{"a":0,"k":0},"p":{"a":0,"k":[0,0,0]},"a":{"a":0,"k":[0,0,0]},"s":{"a":0,"k":[f64::from(doc.width)*100./f64::from(w),f64::from(doc.height)*100./f64::from(h),100]}}}));
            }
        }
        if payload_size > MAX_BYTES {
            return Err(error(
                "Motion export exceeds 64 MiB. Reduce page dimensions, duration or frame rate.",
            ));
        }
    }
    let bytes=match format{Format::AnimatedSvg=>{svg.push_str("</svg>");svg.into_bytes()},Format::Lottie=>unreachable!(),Format::LottieRaster=>serde_json::to_vec(&json!({"v":"5.12.2","fr":fps,"ip":0,"op":frames,"w":doc.width,"h":doc.height,"nm":"Emulsion rendered frame animation","ddd":0,"assets":assets,"layers":layers})).map_err(|e|error(e.to_string()))?};
    if bytes.len() > MAX_BYTES {
        return Err(error("Motion export exceeds 64 MiB."));
    }
    report.bytes = bytes.len();
    if report.raster_frames > 0 {
        report.diagnostics.push(format!("{} rendered frames use PNG appearance at up to 1024px; these frames are not editable Lottie vectors.",report.raster_frames));
    }
    Ok((bytes, report))
}
pub fn write(doc: &Document, path: &Path, format: Format) -> Result<Report> {
    crate::ora::ensure_not_raw_original(doc, path)?;
    let (bytes, report) = encode(doc, format)?;
    crate::write_atomic(path, |file| {
        file.write_all(&bytes)?;
        Ok(())
    })?;
    Ok(report)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn design_motion_exports_are_bounded_deterministic_and_nonmutating() {
        let mut doc = Document::new(32, 24);
        doc.design.duration_ms = 200;
        doc.design.fps = 10;
        let original = doc.clone();
        let (svg, report) = encode(&doc, Format::AnimatedSvg).unwrap();
        assert_eq!(report.frames, 2);
        assert_eq!(report.vector_frames, 2);
        assert!(
            String::from_utf8(svg)
                .unwrap()
                .contains("calcMode=\"discrete\"")
        );
        let (lottie, report) = encode(&doc, Format::LottieRaster).unwrap();
        let data: serde_json::Value = serde_json::from_slice(&lottie).unwrap();
        assert_eq!(data["layers"].as_array().unwrap().len(), 2);
        assert_eq!(data["assets"].as_array().unwrap().len(), 1);
        assert_eq!(report.raster_frames, 2);
        assert_eq!(doc, original);
        doc.design.duration_ms = 60000;
        assert!(encode(&doc, Format::LottieRaster).is_ok());
        doc.design.fps = 60;
        assert!(encode(&doc, Format::LottieRaster).is_err());
    }
}

#[cfg(test)]
mod appearance_tests {
    use super::*;
    use emulsion_core::{
        Command, Editor, Node,
        command::Slot,
        design_keyframes::{self, Easing, Keyframe, Property},
    };
    use std::sync::Arc;
    #[test]
    fn design_motion_interchange_moves_reveals_and_hides_native_content() {
        let mut editor = Editor::new(Document::new(120, 90), None);
        editor.doc.design.duration_ms = 300;
        editor.doc.design.fps = 10;
        let rect = editor
            .execute(Command::AddNode {
                node: Box::new(Node::path(
                    0,
                    "Moving square",
                    Arc::new(emulsion_raster::vector_geometry::rectangle(
                        0., 0., 20., 20.,
                    )),
                    emulsion_raster::vector::PathStyle {
                        fill: Some([255, 0, 0, 255]),
                        stroke: None,
                        ..Default::default()
                    },
                    120,
                    90,
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let text = editor
            .execute(Command::AddNode {
                node: Box::new(Node::text(
                    0,
                    "Revealing text",
                    emulsion_core::text::TextSpec {
                        text: "ABCD".into(),
                        x: 0.,
                        y: 45.,
                        size: 24.,
                        ..Default::default()
                    },
                    120,
                    90,
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        for (id, property, from, to) in [
            (rect, Property::TranslationX, 0., 20.),
            (rect, Property::Visibility, 1., 0.),
            (text, Property::TextReveal, 0., 1.),
        ] {
            for (time_ms, value) in [(0, from), (200, to)] {
                design_keyframes::set_keyframe(
                    &mut editor,
                    id,
                    property,
                    Keyframe {
                        time_ms,
                        value,
                        easing: if property == Property::Visibility {
                            Easing::Step
                        } else {
                            Easing::Linear
                        },
                    },
                )
                .unwrap();
            }
        }
        let original = editor.doc.clone();
        let (svg, report) = encode(&editor.doc, Format::AnimatedSvg).unwrap();
        assert_eq!(report.frames, 3);
        assert_eq!(report.vector_frames, 3);
        let xml = std::str::from_utf8(&svg).unwrap();
        let tree = resvg::usvg::roxmltree::Document::parse(xml).unwrap();
        let frames: Vec<_> = tree
            .descendants()
            .filter(|n| n.has_tag_name("image"))
            .map(|n| {
                String::from_utf8(
                    STANDARD
                        .decode(
                            n.attribute("href")
                                .unwrap()
                                .strip_prefix("data:image/svg+xml;base64,")
                                .unwrap(),
                        )
                        .unwrap(),
                )
                .unwrap()
            })
            .collect();
        assert_eq!(frames.len(), 3);
        assert!(frames[0].contains(&format!("data-node=\"{rect}\"")));
        assert!(!frames[2].contains(&format!("data-node=\"{rect}\"")));
        // The SVG import helper deliberately enlarges small artwork. Render at
        // authored dimensions here so these probes measure native coordinates.
        let render = |svg: &str| {
            let tree = resvg::usvg::Tree::from_str(svg, &Default::default()).unwrap();
            let mut pixmap = resvg::tiny_skia::Pixmap::new(120, 90).unwrap();
            resvg::render(
                &tree,
                resvg::tiny_skia::Transform::identity(),
                &mut pixmap.as_mut(),
            );
            pixmap
        };
        let first = render(&frames[0]);
        let second = render(&frames[1]);
        assert!(first.pixel(5, 5).unwrap().alpha() > 0);
        assert_eq!(second.pixel(5, 5).unwrap().alpha(), 0);
        assert!(second.pixel(15, 5).unwrap().alpha() > 0);
        let (lottie, report) = encode(&editor.doc, Format::LottieRaster).unwrap();
        let data: serde_json::Value = serde_json::from_slice(&lottie).unwrap();
        assert_eq!(report.raster_frames, 3);
        assert_eq!(data["assets"].as_array().unwrap().len(), 3);
        assert_eq!(data["layers"][1]["ip"], 1);
        assert_eq!(data["layers"][1]["op"], 2);
        assert_eq!(editor.doc, original);
        std::fs::write(
            std::env::temp_dir().join("emulsion-motion-acceptance.svg"),
            &svg,
        )
        .unwrap();
        std::fs::write(
            std::env::temp_dir().join("emulsion-motion-acceptance.json"),
            &lottie,
        )
        .unwrap();
    }
}
