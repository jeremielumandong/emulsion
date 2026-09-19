//! Monocular depth with Depth Anything v2: a relative map where nearer is
//! brighter, at the image's own size. Feeds depth-of-field masks, fog and
//! depth-aware grading.

use emulsion_raster::Raster;

use crate::jobs::Job;
use crate::models::{self, Task};
use crate::prep::{Map, Planes};
use crate::runner::{self, RunError};

/// Longest model side; inputs must be multiples of the 14-pixel patch.
const SIDE: usize = 518;
const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

pub fn available() -> Option<&'static models::ModelSpec> {
    models::installed_for(Task::Depth)
}

/// Relative depth of `image`, 0 (far) to 1 (near).
pub fn estimate(image: &Raster, job: &Job) -> Result<Map, RunError> {
    let spec = available().ok_or_else(|| RunError::NotInstalled("a depth model".into()))?;
    let model = runner::model(&models::file_path(spec, &spec.files[0]))?;
    job.set_stage("preparing");
    let planes = Planes::from_raster(image).flattened(0.5);
    let (w, h) = (planes.w, planes.h);
    // Keep the aspect, longest side 518, both sides multiples of 14.
    let scale = SIDE as f32 / w.max(h) as f32;
    let fit = |v: usize| (((v as f32 * scale) / 14.0).round().max(1.0) as usize) * 14;
    let (mw, mh) = (fit(w), fit(h));
    let input = planes.resized(mw, mh).to_nchw(MEAN, STD);
    job.check()?;
    job.progress(0.15);
    job.set_stage("estimating depth");
    let out = model.run(&[("pixel_values", input)])?;
    job.check()?;
    job.progress(0.85);
    let t = out
        .get("predicted_depth")
        .ok_or_else(|| RunError::Shape(spec.id.into(), "no predicted_depth".into()))?;
    let m = Map::from_hw(t).normalized().resized(w, h);
    job.progress(1.0);
    Ok(m)
}
