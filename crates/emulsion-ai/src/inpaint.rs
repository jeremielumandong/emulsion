//! Fill a masked region from its surroundings with LaMa. The hole's
//! neighbourhood is cropped, scaled to the model's 512² input, filled, and
//! only the hole is written back, feathered into the original.

use emulsion_raster::{IRect, Mask, Raster, select};
use ndarray::Array4;

use crate::jobs::Job;
use crate::models::{self, Task};
use crate::prep::{Map, Planes};
use crate::runner::{self, RunError};

const SIDE: usize = 512;

pub fn available() -> Option<&'static models::ModelSpec> {
    models::installed_for(Task::Inpaint)
}

/// Fill where `hole` is set. Returns the filled pixels and where they go.
pub fn fill(image: &Raster, hole: &Mask, job: &Job) -> Result<(Raster, IRect), RunError> {
    let spec = available().ok_or_else(|| RunError::NotInstalled("a fill model".into()))?;
    let model = runner::model(&models::file_path(spec, &spec.files[0]))?;
    let b = select::bounds(hole);
    if b.is_empty() {
        return Err(RunError::Other("nothing selected to fill".into()));
    }
    let canvas = IRect::new(0, 0, image.width() as i32, image.height() as i32);
    // Context around the hole: half its size each way, at least 64 px, and
    // square so the model sees an undistorted crop when possible.
    let margin = (b.w.max(b.h) / 2).max(64);
    let side = (b.w.max(b.h) + 2 * margin).max(128);
    let cx = b.x + b.w / 2;
    let cy = b.y + b.h / 2;
    let crop = IRect::new(cx - side / 2, cy - side / 2, side, side).intersect(&canvas);
    if crop.is_empty() {
        return Err(RunError::Other("selection is outside the image".into()));
    }
    job.set_stage("preparing");
    let planes = Planes::from_raster(image).flattened(0.5);
    let (w, h) = (planes.w, planes.h);
    let sub = Planes {
        w: crop.w as usize,
        h: crop.h as usize,
        px: (crop.y..crop.bottom())
            .flat_map(|y| (crop.x..crop.right()).map(move |x| (x, y)))
            .map(|(x, y)| planes.px[y as usize * w + x as usize])
            .collect(),
    };
    let hole_px = hole.to_pixels();
    let sub_mask = Map {
        w: crop.w as usize,
        h: crop.h as usize,
        v: (crop.y..crop.bottom())
            .flat_map(|y| (crop.x..crop.right()).map(move |x| (x, y)))
            .map(|(x, y)| hole_px[y as usize * w + x as usize] as f32 / 255.0)
            .collect(),
    };
    let small = sub.resized(SIDE, SIDE);
    let small_mask = sub_mask.resized(SIDE, SIDE);
    let mut img = Array4::<f32>::zeros((1, 3, SIDE, SIDE));
    let mut msk = Array4::<f32>::zeros((1, 1, SIDE, SIDE));
    for y in 0..SIDE {
        for x in 0..SIDE {
            let p = small.px[y * SIDE + x];
            for c in 0..3 {
                img[[0, c, y, x]] = p[c];
            }
            // A binary, slightly grown hole so edge pixels are re-synthesised.
            msk[[0, 0, y, x]] = if small_mask.v[y * SIDE + x] > 0.2 {
                1.0
            } else {
                0.0
            };
        }
    }
    job.check()?;
    job.progress(0.15);
    job.set_stage("filling");
    let out = model.run(&[("image", img.into_dyn()), ("mask", msk.into_dyn())])?;
    job.check()?;
    job.progress(0.85);
    let t = out
        .get("output")
        .ok_or_else(|| RunError::Shape(spec.id.into(), "no output".into()))?;
    let sh = t.shape();
    let (oh, ow) = (sh[2], sh[3]);
    // LaMa emits 0–255.
    let filled = Planes {
        w: ow,
        h: oh,
        px: (0..oh * ow)
            .map(|i| {
                let (y, x) = (i / ow, i % ow);
                [
                    t[[0, 0, y, x]] / 255.0,
                    t[[0, 1, y, x]] / 255.0,
                    t[[0, 2, y, x]] / 255.0,
                    1.0,
                ]
            })
            .collect(),
    }
    .resized(crop.w as usize, crop.h as usize);
    // Write back inside the (feathered) hole only; keep original alpha.
    let feather = select::feather(hole, 1.5);
    let fpx = feather.to_pixels();
    let src = Planes::from_raster(image);
    let px: Vec<[f32; 4]> = (0..(crop.w * crop.h) as usize)
        .map(|i| {
            let (ly, lx) = (i / crop.w as usize, i % crop.w as usize);
            let (x, y) = (crop.x as usize + lx, crop.y as usize + ly);
            let k = fpx[y * w + x] as f32 / 255.0;
            let o = src.px[y * w + x];
            let f = filled.px[i];
            let a = o[3].max(k);
            [
                o[0] + (f[0] - o[0]) * k,
                o[1] + (f[1] - o[1]) * k,
                o[2] + (f[2] - o[2]) * k,
                a,
            ]
        })
        .collect();
    let _ = h;
    job.progress(1.0);
    Ok((
        Planes {
            w: crop.w as usize,
            h: crop.h as usize,
            px,
        }
        .to_raster(),
        crop,
    ))
}
