//! Semantic sky segmentation using the published MIT-licensed U-2-Net model.
use crate::{
    jobs::Job,
    models,
    prep::{Map, Planes, guided_filter},
    runner::{self, RunError},
};
use emulsion_raster::{Mask, Raster};
pub fn mask(image: &Raster, job: &Job) -> Result<Mask, RunError> {
    let spec = models::spec("skyseg")
        .ok_or_else(|| RunError::Other("Sky model manifest missing".into()))?;
    let model = runner::model(&models::file_path(spec, &spec.files[0]))?;
    job.set_stage("Segmenting sky");
    job.check()?;
    let planes = Planes::from_raster(image).flattened(0.5);
    let input = planes
        .resized(320, 320)
        .to_nchw([0.485, 0.456, 0.406], [0.229, 0.224, 0.225]);
    let output = model.run(&[(&model.inputs[0], input)])?;
    job.check()?;
    let tensor = output
        .get(&model.outputs[0])
        .ok_or_else(|| RunError::Shape(spec.id.into(), "missing sky output".into()))?;
    if tensor.shape() != [1, 1, 320, 320] || tensor.iter().any(|v| !v.is_finite()) {
        return Err(RunError::Shape(
            spec.id.into(),
            "invalid sky output dimensions or values".into(),
        ));
    }
    let map = Map::from_hw(tensor).resized(planes.w, planes.h);
    job.progress(0.8);
    let mask = guided_filter(&planes, &map, 4, 1e-3).to_mask();
    job.check()?;
    job.progress(1.);
    Ok(mask)
}
#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires installed sky model"]
    fn installed_sky_inference() {
        let r = emulsion_raster::Raster::solid(32, 24, [0.1, 0.4, 0.8, 1.]);
        let mask = super::mask(&r, &crate::jobs::Job::new()).unwrap();
        assert_eq!((mask.width(), mask.height()), (32, 24));
    }
}
