//! Salient-object matte: the soft mask of "the subject" of a picture, from
//! RMBG-1.4 or ISNet. Feeds Select Subject and Remove Background.

use emulsion_raster::{Mask, Raster};

use crate::jobs::Job;
use crate::models::{self, Task};
use crate::prep::{Map, Planes, guided_filter};
use crate::runner::{self, RunError};

/// Model input side; both matte models were trained at 1024².
const SIDE: usize = 1024;

pub struct MatteOptions {
    /// Snap the matte to image edges with a guided filter.
    pub refine: bool,
    /// Guided-filter radius in pixels at the image's own scale.
    pub radius: usize,
}

impl Default for MatteOptions {
    fn default() -> Self {
        Self {
            refine: true,
            radius: 6,
        }
    }
}

/// Which installed model will run, if any.
pub fn available() -> Option<&'static models::ModelSpec> {
    models::installed_for(Task::Matte)
}

/// The subject matte of `image` at its own size, 0 = background.
pub fn matte(image: &Raster, opts: &MatteOptions, job: &Job) -> Result<Mask, RunError> {
    let spec = available().ok_or_else(|| RunError::NotInstalled("a subject matte model".into()))?;
    let model = runner::model(&models::file_path(spec, &spec.files[0]))?;
    job.set_stage("preparing");
    let planes = Planes::from_raster(image).flattened(0.5);
    let small = planes.resized(SIDE, SIDE);
    // Both models take ImageNet-ish inputs: RMBG (0.5, 1.0); ISNet (0.5, 1.0) too.
    let input = small.to_nchw([0.5; 3], [1.0; 3]);
    job.check()?;
    job.progress(0.15);
    job.set_stage(format!("running {}", spec.name));
    let name = model
        .inputs
        .first()
        .cloned()
        .unwrap_or_else(|| "input".into());
    let out = model.run(&[(&name, input)])?;
    job.check()?;
    job.progress(0.8);
    let first = model
        .outputs
        .first()
        .cloned()
        .unwrap_or_else(|| "output".into());
    let t = out
        .get(&first)
        .ok_or_else(|| RunError::Shape(spec.id.into(), "no output".into()))?;
    let m = Map::from_hw(t).normalized().resized(planes.w, planes.h);
    job.set_stage("refining");
    let m = if opts.refine {
        guided_filter(&planes, &m, opts.radius.max(1), 1e-3)
    } else {
        m
    };
    job.progress(1.0);
    Ok(m.to_mask())
}

/// Turn a soft matte into a selection-friendly mask: anything below `lo`
/// is dropped and above `hi` is solid, keeping soft edges between.
pub fn harden(m: &Mask, lo: u8, hi: u8) -> Mask {
    let (w, h) = (m.width(), m.height());
    let px: Vec<u8> = m
        .to_pixels()
        .into_iter()
        .map(|v| {
            if v <= lo {
                0
            } else if v >= hi {
                255
            } else {
                (((v - lo) as u32 * 255) / (hi - lo).max(1) as u32) as u8
            }
        })
        .collect();
    Mask::from_pixels(w, h, 0, &px)
}

/// `image` with the matte applied as alpha, for Remove Background.
pub fn cut_out(image: &Raster, m: &Mask) -> Raster {
    let (w, h) = (image.width(), image.height());
    let src = image.to_pixels();
    let a = m.to_pixels();
    let px: Vec<[u16; 4]> = src
        .iter()
        .zip(a)
        .map(|(p, k)| {
            let k = k as u32;
            [
                ((p[0] as u32 * k) / 255) as u16,
                ((p[1] as u32 * k) / 255) as u16,
                ((p[2] as u32 * k) / 255) as u16,
                ((p[3] as u32 * k) / 255) as u16,
            ]
        })
        .collect();
    Raster::from_pixels(w, h, [0; 4], &px)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harden_and_cut_out() {
        let m = Mask::from_pixels(4, 1, 0, &[0, 60, 200, 255]);
        let h = harden(&m, 40, 220);
        assert_eq!(h.to_pixels(), vec![0, 28, 226, 255]);
        let img = Raster::from_pixels(4, 1, [0; 4], &[[65535, 0, 0, 65535]; 4]);
        let cut = cut_out(&img, &h);
        assert_eq!(cut.get(0, 0)[3], 0);
        assert_eq!(cut.get(3, 0)[3], 65535);
        assert!(cut.get(2, 0)[3] > 50000 && cut.get(2, 0)[3] < 60000);
    }
}
