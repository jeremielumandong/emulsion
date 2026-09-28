//! Tiled inference on black-subtracted, white-normalized Bayer data.
use crate::{
    models,
    runner::{self, RunError},
};
use std::sync::atomic::{AtomicBool, Ordering};

fn model() -> Result<std::sync::Arc<runner::Model>, RunError> {
    let spec = models::installed_for(models::Task::SensorDenoise).ok_or_else(|| {
        RunError::NotInstalled("RawNIND Bayer denoise (install in Models)".into())
    })?;
    let archive = models::file_path(spec, &spec.files[0]);
    let target = models::model_dir(spec).join("model_bayer.onnx");
    static EXTRACT: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _lock = EXTRACT.lock().unwrap_or_else(|e| e.into_inner());
    if !target.exists() {
        let extract = || -> Result<(), Box<dyn std::error::Error>> {
            let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?)?;
            let mut entry = zip.by_name("rawdenoise-nind/model_bayer.onnx")?;
            if entry.size() != 31_056_425 {
                return Err("Unexpected sensor model size".into());
            }
            let partial = target.with_extension("partial");
            let mut file = std::fs::File::create(&partial)?;
            std::io::copy(&mut entry, &mut file)?;
            file.sync_all()?;
            std::fs::rename(partial, &target)?;
            Ok(())
        };
        extract().map_err(|e| RunError::Other(e.to_string()))?;
    }
    // Pin the tested inference contract even when an upstream release asset changes.
    static VERIFIED: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    if VERIFIED.get() != Some(&target) {
        use sha2::{Digest, Sha256};
        let bytes = std::fs::read(&target).map_err(|e| RunError::Other(e.to_string()))?;
        let hash: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if hash != "da27509dab6a2915da67e988acd86cf71f9d5bbc8d1aa0ed32933578a887b901" {
            return Err(RunError::Other(
                "Sensor model checksum mismatch; reinstall the supported model".into(),
            ));
        }
        let _ = VERIFIED.set(target.clone());
    }
    runner::model(&target)
}

/// Output is camera RGB without white balance or output color encoding.
pub fn denoise(
    data: &[f32],
    width: usize,
    height: usize,
    red: [usize; 2],
    cancel: &AtomicBool,
) -> Result<Vec<[f32; 3]>, RunError> {
    if width < 2 || height < 2 || data.len() != width * height || red.iter().any(|v| *v > 1) {
        return Err(RunError::Other("Invalid Bayer sensor dimensions".into()));
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(RunError::Other("Sensor denoise cancelled".into()));
    }
    let model = model()?;
    let mut result = vec![[0.; 3]; width * height];
    let mirror = |v: isize, n: usize| {
        let period = 2 * (n - 1) as isize;
        let v = v.rem_euclid(period);
        if v >= n as isize {
            (period - v) as usize
        } else {
            v as usize
        }
    };
    let check = || {
        if cancel.load(Ordering::Relaxed) {
            Err(RunError::Other("Sensor denoise cancelled".into()))
        } else {
            Ok(())
        }
    };
    const SIDE: usize = 512;
    const OUT: usize = SIDE * 2;
    const BORDER: usize = 64;
    const STEP: usize = OUT - BORDER * 2;
    for top in (0..height).step_by(STEP) {
        for left in (0..width).step_by(STEP) {
            check()?;
            let mut input = ndarray::Array4::<f32>::zeros((1, 4, SIDE, SIDE));
            let mut input_mean = 0f64;
            for c in 0..4 {
                for y in 0..SIDE {
                    for x in 0..SIDE {
                        let sx = mirror(
                            left as isize - BORDER as isize + (x * 2 + c % 2 + red[0]) as isize,
                            width,
                        );
                        let sy = mirror(
                            top as isize - BORDER as isize + (y * 2 + c / 2 + red[1]) as isize,
                            height,
                        );
                        let value = data[sy * width + sx].clamp(0., 1.);
                        input[[0, c, y, x]] = value;
                        input_mean += value as f64;
                    }
                }
            }
            let outputs = model.run(&[("input", input.into_dyn())])?;
            check()?;
            let out = outputs
                .get("output")
                .ok_or_else(|| RunError::Other("Missing sensor model output".into()))?;
            if out.shape() != [1, 3, OUT, OUT] || out.iter().any(|v| !v.is_finite()) {
                return Err(RunError::Other("Invalid sensor model output".into()));
            }
            let output_mean: f64 = (0..OUT)
                .flat_map(|y| (0..OUT).map(move |x| (y, x)))
                .map(|(y, x)| {
                    (out[[0, 0, y, x]] + 2. * out[[0, 1, y, x]] + out[[0, 2, y, x]]) as f64
                })
                .sum();
            let gain = if output_mean > 1e-12 {
                (input_mean * 4. / output_mean) as f32
            } else {
                1.
            };
            for y in top..(top + STEP).min(height) {
                for x in left..(left + STEP).min(width) {
                    let ox = x - left + BORDER - red[0];
                    let oy = y - top + BORDER - red[1];
                    result[y * width + x] =
                        std::array::from_fn(|c| (out[[0, c, oy, ox]] * gain).max(0.));
                }
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Requires the optional RawNIND Bayer model and ONNX Runtime"]
    fn real_model_preserves_bayer_phase_color_and_tile_seams() {
        for red in [[0, 0], [0, 1], [1, 0], [1, 1]] {
            let (w, h) = (1000, 64);
            let data: Vec<_> = (0..w * h)
                .map(|i| {
                    let c = if i % w % 2 == red[0] && i / w % 2 == red[1] {
                        0
                    } else if i % w % 2 != red[0] && i / w % 2 != red[1] {
                        2
                    } else {
                        1
                    };
                    [0.2, 0.3, 0.4][c]
                })
                .collect();
            let out = denoise(&data, w, h, red, &AtomicBool::new(false)).unwrap();
            for x in [100, 895, 896, 940] {
                for (v, expected) in out[32 * w + x].iter().zip([0.2, 0.3, 0.4]) {
                    assert!(
                        (v - expected).abs() < 0.025,
                        "phase {red:?}, x {x}, pixel {:?}",
                        out[32 * w + x]
                    );
                }
            }
        }
    }
    #[test]
    #[ignore = "Requires the optional RawNIND Bayer model and ONNX Runtime"]
    fn real_model_produces_finite_camera_rgb_and_preserves_gain() {
        let data: Vec<_> = (0..64 * 64)
            .map(|i| 0.3 + ((i * 73 % 97) as f32 / 96. - 0.5) * 0.08)
            .collect();
        let out = denoise(&data, 64, 64, [0, 0], &AtomicBool::new(false)).unwrap();
        assert_eq!(out.len(), data.len());
        assert!(out.iter().flatten().all(|v| v.is_finite() && *v >= 0.));
        let mean = out.iter().flatten().sum::<f32>() / (out.len() * 3) as f32;
        assert!((mean - 0.3).abs() < 0.08, "mean {mean}");
        let input_error = data.iter().map(|v| (v - 0.3).powi(2)).sum::<f32>() / data.len() as f32;
        let output_error =
            out.iter().flatten().map(|v| (v - 0.3).powi(2)).sum::<f32>() / (out.len() * 3) as f32;
        assert!(
            output_error < input_error,
            "input {input_error}, output {output_error}"
        );
        assert!(denoise(&data, 64, 64, [0, 0], &AtomicBool::new(true)).is_err());
    }
}
