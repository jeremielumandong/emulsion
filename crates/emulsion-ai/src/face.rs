//! Face restoration: find faces (YOLOv8-face), align each to the FFHQ
//! template with a similarity transform, restore with GFPGAN at 512², and
//! paste back through the inverse transform under a feathered mask.

use emulsion_raster::Raster;
use glam::{DAffine2, DVec2, dvec2};
use ndarray::Array4;

use crate::jobs::Job;
use crate::models::{self, Task};
use crate::prep::Planes;
use crate::runner::{self, RunError};

const DET_SIDE: usize = 640;
const FACE_SIDE: usize = 512;
/// FFHQ five-point template at 512², as GFPGAN expects.
const TEMPLATE: [[f64; 2]; 5] = [
    [192.98138, 239.94708],
    [318.90277, 240.1936],
    [256.63416, 314.01935],
    [201.26117, 371.41043],
    [313.08905, 371.15118],
];

#[derive(Clone, Debug)]
pub struct Face {
    /// Box in image pixels.
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub score: f32,
    /// Eyes, nose, mouth corners in image pixels.
    pub landmarks: [[f32; 2]; 5],
}

pub fn available() -> Option<&'static models::ModelSpec> {
    models::installed_for(Task::FaceRestore)
}

pub fn detector_available() -> Option<&'static models::ModelSpec> {
    models::installed_for(Task::FaceDetect)
}

/// Faces in `image`, best first.
pub fn detect(image: &Raster, job: &Job) -> Result<Vec<Face>, RunError> {
    let spec =
        detector_available().ok_or_else(|| RunError::NotInstalled("a face detector".into()))?;
    let model = runner::model(&models::file_path(spec, &spec.files[0]))?;
    job.set_stage("finding faces");
    let planes = Planes::from_raster(image).flattened(0.5);
    let (w, h) = (planes.w, planes.h);
    // Letterbox into 640² at the top-left.
    let scale = DET_SIDE as f32 / w.max(h) as f32;
    let (fw, fh) = (
        ((w as f32 * scale).round() as usize).clamp(1, DET_SIDE),
        ((h as f32 * scale).round() as usize).clamp(1, DET_SIDE),
    );
    let small = planes.resized(fw, fh);
    let mut t = Array4::<f32>::zeros((1, 3, DET_SIDE, DET_SIDE));
    for y in 0..fh {
        for x in 0..fw {
            let p = small.px[y * fw + x];
            for c in 0..3 {
                t[[0, c, y, x]] = p[c];
            }
        }
    }
    let out = model.run(&[("input", t.into_dyn())])?;
    let o = out
        .get("output")
        .ok_or_else(|| RunError::Shape(spec.id.into(), "no output".into()))?;
    let n = o.shape()[2];
    let mut faces: Vec<Face> = Vec::new();
    for i in 0..n {
        let score = o[[0, 4, i]];
        if score < 0.5 {
            continue;
        }
        let (cx, cy, bw, bh) = (o[[0, 0, i]], o[[0, 1, i]], o[[0, 2, i]], o[[0, 3, i]]);
        let mut landmarks = [[0.0f32; 2]; 5];
        for (k, lm) in landmarks.iter_mut().enumerate() {
            *lm = [o[[0, 5 + k * 3, i]] / scale, o[[0, 6 + k * 3, i]] / scale];
        }
        faces.push(Face {
            x0: (cx - bw / 2.0) / scale,
            y0: (cy - bh / 2.0) / scale,
            x1: (cx + bw / 2.0) / scale,
            y1: (cy + bh / 2.0) / scale,
            score,
            landmarks,
        });
    }
    faces.sort_by(|a, b| b.score.total_cmp(&a.score));
    // Non-maximum suppression.
    let mut kept: Vec<Face> = Vec::new();
    for f in faces {
        if kept.iter().all(|k| iou(k, &f) < 0.4) {
            kept.push(f);
        }
    }
    Ok(kept)
}

fn iou(a: &Face, b: &Face) -> f32 {
    let ix = (a.x1.min(b.x1) - a.x0.max(b.x0)).max(0.0);
    let iy = (a.y1.min(b.y1) - a.y0.max(b.y0)).max(0.0);
    let inter = ix * iy;
    let ua = (a.x1 - a.x0) * (a.y1 - a.y0) + (b.x1 - b.x0) * (b.y1 - b.y0) - inter;
    if ua <= 0.0 { 0.0 } else { inter / ua }
}

/// Least-squares similarity (scale, rotation, translation) mapping `from`
/// onto `to`.
fn similarity(from: &[[f64; 2]; 5], to: &[[f64; 2]; 5]) -> DAffine2 {
    // Solve for a, b, tx, ty in x' = a x - b y + tx, y' = b x + a y + ty.
    let n = from.len() as f64;
    let (mut sx, mut sy, mut tx, mut ty) = (0.0, 0.0, 0.0, 0.0);
    for (f, t) in from.iter().zip(to) {
        sx += f[0];
        sy += f[1];
        tx += t[0];
        ty += t[1];
    }
    let (mx, my, mtx, mty) = (sx / n, sy / n, tx / n, ty / n);
    let (mut num_a, mut num_b, mut den) = (0.0, 0.0, 0.0);
    for (f, t) in from.iter().zip(to) {
        let (fx, fy) = (f[0] - mx, f[1] - my);
        let (gx, gy) = (t[0] - mtx, t[1] - mty);
        num_a += fx * gx + fy * gy;
        num_b += fx * gy - fy * gx;
        den += fx * fx + fy * fy;
    }
    let (a, b) = if den > 1e-9 {
        (num_a / den, num_b / den)
    } else {
        (1.0, 0.0)
    };
    let lin = DAffine2::from_cols_array(&[a, b, -b, a, 0.0, 0.0]);
    let t = dvec2(mtx, mty) - lin.transform_point2(dvec2(mx, my));
    DAffine2::from_cols_array(&[a, b, -b, a, t.x, t.y])
}

/// Restore every detected face. `strength` 0–1 blends the restored face
/// over the original (1 = fully restored).
pub fn restore(image: &Raster, strength: f32, job: &Job) -> Result<(Raster, usize), RunError> {
    let spec = available().ok_or_else(|| RunError::NotInstalled("a face restore model".into()))?;
    let faces = detect(image, job)?;
    if faces.is_empty() {
        return Err(RunError::Other("no face found".into()));
    }
    let model = runner::model(&models::file_path(spec, &spec.files[0]))?;
    let mut planes = Planes::from_raster(image);
    let (w, h) = (planes.w, planes.h);
    let total = faces.len() as f32;
    for (i, face) in faces.iter().enumerate() {
        job.check()?;
        job.set_stage(format!("restoring face {}/{}", i + 1, faces.len()));
        let from = face.landmarks.map(|p| [p[0] as f64, p[1] as f64]);
        let to_face = similarity(&from, &TEMPLATE);
        let to_image = to_face.inverse();
        // Crop by inverse mapping every face pixel into the image.
        let mut t = Array4::<f32>::zeros((1, 3, FACE_SIDE, FACE_SIDE));
        for y in 0..FACE_SIDE {
            for x in 0..FACE_SIDE {
                let p = to_image.transform_point2(dvec2(x as f64 + 0.5, y as f64 + 0.5));
                let s = sample(&planes, p);
                for c in 0..3 {
                    t[[0, c, y, x]] = s[c] * 2.0 - 1.0;
                }
            }
        }
        let out = model.run(&[("input", t.into_dyn())])?;
        let o = out
            .get("output")
            .ok_or_else(|| RunError::Shape(spec.id.into(), "no output".into()))?;
        // Paste back: for every image pixel inside the warped square, map
        // into face space and blend under a mask that fades at the border.
        let corners = [
            to_image.transform_point2(dvec2(0.0, 0.0)),
            to_image.transform_point2(dvec2(FACE_SIDE as f64, 0.0)),
            to_image.transform_point2(dvec2(0.0, FACE_SIDE as f64)),
            to_image.transform_point2(dvec2(FACE_SIDE as f64, FACE_SIDE as f64)),
        ];
        let x0 = corners
            .iter()
            .map(|c| c.x)
            .fold(f64::INFINITY, f64::min)
            .floor()
            .max(0.0) as usize;
        let y0 = corners
            .iter()
            .map(|c| c.y)
            .fold(f64::INFINITY, f64::min)
            .floor()
            .max(0.0) as usize;
        let x1 = (corners
            .iter()
            .map(|c| c.x)
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil() as usize)
            .min(w);
        let y1 = (corners
            .iter()
            .map(|c| c.y)
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil() as usize)
            .min(h);
        let fade = FACE_SIDE as f64 * 0.12;
        for y in y0..y1 {
            for x in x0..x1 {
                let f = to_face.transform_point2(dvec2(x as f64 + 0.5, y as f64 + 0.5));
                if f.x < 0.0 || f.y < 0.0 || f.x >= FACE_SIDE as f64 || f.y >= FACE_SIDE as f64 {
                    continue;
                }
                let edge =
                    f.x.min(f.y)
                        .min(FACE_SIDE as f64 - f.x)
                        .min(FACE_SIDE as f64 - f.y);
                let k = (edge / fade).clamp(0.0, 1.0) as f32 * strength.clamp(0.0, 1.0);
                if k <= 0.0 {
                    continue;
                }
                let (fx, fy) = (
                    (f.x as usize).min(FACE_SIDE - 1),
                    (f.y as usize).min(FACE_SIDE - 1),
                );
                let dst = &mut planes.px[y * w + x];
                for c in 0..3 {
                    let r = ((o[[0, c, fy, fx]] + 1.0) / 2.0).clamp(0.0, 1.0);
                    dst[c] += (r - dst[c]) * k;
                }
            }
        }
        job.progress(0.1 + 0.9 * (i as f32 + 1.0) / total);
    }
    Ok((planes.to_raster(), faces.len()))
}

fn sample(p: &Planes, at: DVec2) -> [f32; 4] {
    let x = at.x.clamp(0.0, p.w as f64 - 1.0) as usize;
    let y = at.y.clamp(0.0, p.h as f64 - 1.0) as usize;
    p.px[y * p.w + x]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn similarity_recovers_a_known_transform() {
        let m = DAffine2::from_scale_angle_translation(dvec2(1.7, 1.7), 0.3, dvec2(40.0, -12.0));
        let from = TEMPLATE;
        let to = from.map(|p| {
            let q = m.transform_point2(dvec2(p[0], p[1]));
            [q.x, q.y]
        });
        let est = similarity(&from, &to);
        for p in from {
            let a = m.transform_point2(dvec2(p[0], p[1]));
            let b = est.transform_point2(dvec2(p[0], p[1]));
            assert!((a - b).length() < 1e-6, "{a} vs {b}");
        }
        let a = Face {
            x0: 0.0,
            y0: 0.0,
            x1: 10.0,
            y1: 10.0,
            score: 1.0,
            landmarks: [[0.0; 2]; 5],
        };
        let b = Face {
            x0: 5.0,
            y0: 0.0,
            x1: 15.0,
            y1: 10.0,
            score: 1.0,
            landmarks: [[0.0; 2]; 5],
        };
        assert!((iou(&a, &b) - 1.0 / 3.0).abs() < 1e-6);
    }
}
