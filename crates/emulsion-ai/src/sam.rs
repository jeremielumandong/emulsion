//! Promptable segmentation with SlimSAM (a pruned Segment Anything).
//!
//! The image is encoded once into an [`Embedding`]; every click or box then
//! runs only the light prompt encoder and mask decoder, so interactive
//! selection stays snappy on a CPU.

use emulsion_raster::Mask;
use emulsion_raster::Raster;
use ndarray::{Array3, Array4, ArrayD};

use crate::jobs::Job;
use crate::models::{self, Task};
use crate::prep::{Map, Planes};
use crate::runner::{self, RunError};

const SIDE: usize = 1024;
const MASK_SIDE: usize = 256;
const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

/// A point prompt in image pixels; `positive` false marks background.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
    pub positive: bool,
}

/// The encoded image and how it was fitted into the model's square.
pub struct Embedding {
    pub width: u32,
    pub height: u32,
    /// Model pixels per image pixel.
    scale: f32,
    /// Size of the image inside the 1024² square, in model pixels.
    fitted: (usize, usize),
    embeddings: ArrayD<f32>,
    positional: ArrayD<f32>,
}

pub fn available() -> Option<&'static models::ModelSpec> {
    models::installed_for(Task::Segment)
}

fn files() -> Result<(std::sync::Arc<runner::Model>, std::sync::Arc<runner::Model>), RunError> {
    let spec = available().ok_or_else(|| RunError::NotInstalled("a segmentation model".into()))?;
    let enc = runner::model(&models::file_path(spec, &spec.files[0]))?;
    let dec = runner::model(&models::file_path(spec, &spec.files[1]))?;
    Ok((enc, dec))
}

/// Encode `image`: the slow half, a second or two on a CPU.
pub fn encode(image: &Raster, job: &Job) -> Result<Embedding, RunError> {
    let (enc, _) = files()?;
    job.set_stage("encoding image");
    let planes = Planes::from_raster(image).flattened(0.5);
    let (w, h) = (planes.w, planes.h);
    let scale = SIDE as f32 / w.max(h) as f32;
    let fitted = (
        ((w as f32 * scale).round() as usize).clamp(1, SIDE),
        ((h as f32 * scale).round() as usize).clamp(1, SIDE),
    );
    let small = planes.resized(fitted.0, fitted.1);
    // Pad bottom/right with the mean (zero after normalisation), as SAM does.
    let mut t = Array4::<f32>::zeros((1, 3, SIDE, SIDE));
    for y in 0..fitted.1 {
        for x in 0..fitted.0 {
            let p = small.px[y * fitted.0 + x];
            for c in 0..3 {
                t[[0, c, y, x]] = (p[c] - MEAN[c]) / STD[c];
            }
        }
    }
    job.check()?;
    job.progress(0.1);
    let out = enc.run(&[("pixel_values", t.into_dyn())])?;
    job.progress(0.95);
    let embeddings = out
        .get("image_embeddings")
        .cloned()
        .ok_or_else(|| RunError::Shape("slimsam".into(), "no image_embeddings".into()))?;
    let positional = out
        .get("image_positional_embeddings")
        .cloned()
        .ok_or_else(|| {
            RunError::Shape("slimsam".into(), "no image_positional_embeddings".into())
        })?;
    job.progress(1.0);
    Ok(Embedding {
        width: image.width(),
        height: image.height(),
        scale,
        fitted,
        embeddings,
        positional,
    })
}

/// A mask from point prompts and an optional box `(x0, y0, x1, y1)` in
/// image pixels. Returns the matte (0–255 soft) and the model's confidence.
pub fn decode(
    emb: &Embedding,
    points: &[Point],
    bbox: Option<(f32, f32, f32, f32)>,
) -> Result<(Mask, f32), RunError> {
    let (_, dec) = files()?;
    let mut coords: Vec<[f32; 2]> = Vec::new();
    let mut labels: Vec<i64> = Vec::new();
    for p in points {
        coords.push([p.x * emb.scale, p.y * emb.scale]);
        labels.push(if p.positive { 1 } else { 0 });
    }
    if let Some((x0, y0, x1, y1)) = bbox {
        coords.push([x0.min(x1) * emb.scale, y0.min(y1) * emb.scale]);
        labels.push(2);
        coords.push([x0.max(x1) * emb.scale, y0.max(y1) * emb.scale]);
        labels.push(3);
    }
    if coords.is_empty() {
        return Err(RunError::Other("a point or box is needed".into()));
    }
    let n = coords.len();
    let mut pts = Array4::<f32>::zeros((1, 1, n, 2));
    let mut lab = Array3::<i64>::zeros((1, 1, n));
    for (i, (c, l)) in coords.iter().zip(&labels).enumerate() {
        pts[[0, 0, i, 0]] = c[0];
        pts[[0, 0, i, 1]] = c[1];
        lab[[0, 0, i]] = *l;
    }
    let out = dec.run_mixed(
        &[
            ("input_points", pts.into_dyn()),
            ("image_embeddings", emb.embeddings.clone()),
            ("image_positional_embeddings", emb.positional.clone()),
        ],
        &[("input_labels", lab.into_dyn())],
    )?;
    let scores = out
        .get("iou_scores")
        .ok_or_else(|| RunError::Shape("slimsam".into(), "no iou_scores".into()))?;
    let masks = out
        .get("pred_masks")
        .ok_or_else(|| RunError::Shape("slimsam".into(), "no pred_masks".into()))?;
    // Best of the three candidate masks.
    let k = masks.shape()[2];
    let (best, score) = (0..k)
        .map(|i| (i, scores[[0, 0, i]]))
        .fold((0, f32::NEG_INFINITY), |a, b| if b.1 > a.1 { b } else { a });
    let ms = masks.shape();
    let (mh, mw) = (ms[3], ms[4]);
    let logits: Vec<f32> = (0..mh * mw)
        .map(|i| masks[[0, 0, best, i / mw, i % mw]])
        .collect();
    // The 256² mask covers the padded square: keep the fitted part.
    let fw = ((emb.fitted.0 as f32 / SIDE as f32) * mw as f32)
        .round()
        .max(1.0) as usize;
    let fh = ((emb.fitted.1 as f32 / SIDE as f32) * mh as f32)
        .round()
        .max(1.0) as usize;
    let crop: Vec<f32> = (0..fh)
        .flat_map(|y| (0..fw).map(move |x| (x, y)))
        .map(|(x, y)| logits[y.min(mh - 1) * mw + x.min(mw - 1)])
        .collect();
    let m = Map {
        w: fw,
        h: fh,
        v: crop,
    }
    .resized(emb.width as usize, emb.height as usize);
    // Logits → soft edge over about one model pixel.
    let soft = Map {
        w: m.w,
        h: m.h,
        v: m.v.iter().map(|l| 1.0 / (1.0 + (-l * 2.0).exp())).collect(),
    };
    Ok((soft.to_mask(), score))
}

#[allow(dead_code)]
const _: usize = MASK_SIDE;
