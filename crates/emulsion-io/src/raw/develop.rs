use super::{DevelopParams, cancelled};
mod advanced;
mod detail;
use crate::{IoError, Result};
use emulsion_raster::{Raster, TILE, TILE_PX, TileCoord};
use rawler::{
    Orientation, RawImage,
    imgop::{
        chromatic_adaption::bradford_adaption_matrix,
        develop::{Intermediate, ProcessingStep, RawDevelop},
        matrix::{multiply, normalize, pseudo_inverse},
        sensor::SensorType,
        xyz::{Illuminant, SRGB_TO_XYZ_D65},
    },
    rawimage::{RawImageData, RawPhotometricInterpretation},
};
use rayon::prelude::*;
use std::sync::atomic::AtomicBool;

fn invalid(message: &str) -> IoError {
    IoError::MalformedRaw(message.into())
}

pub(super) fn validate(raw: &RawImage) -> Result<()> {
    let width = u32::try_from(raw.width).map_err(|_| invalid("RAW width overflow"))?;
    let height = u32::try_from(raw.height).map_err(|_| invalid("RAW height overflow"))?;
    super::check_sensor_size(width, height)?;
    if !matches!(raw.cpp, 1 | 3 | 4) {
        return Err(IoError::UnsupportedRaw(format!(
            "{} components per sensor pixel",
            raw.cpp
        )));
    }
    let count = raw
        .width
        .checked_mul(raw.height)
        .and_then(|n| n.checked_mul(raw.cpp))
        .ok_or_else(|| invalid("RAW size overflow"))?;
    let len = match &raw.data {
        RawImageData::Integer(data) => data.len(),
        RawImageData::Float(data) => data.len(),
    };
    if len != count {
        return Err(invalid(
            "RAW sensor data length does not match its dimensions",
        ));
    }
    let black = &raw.blacklevel;
    if black.width == 0
        || black.height == 0
        || black.cpp != raw.cpp
        || black
            .width
            .checked_mul(black.height)
            .and_then(|n| n.checked_mul(black.cpp))
            != Some(black.levels.len())
    {
        return Err(invalid("Invalid RAW black-level repeat pattern"));
    }
    let white = &raw.whitelevel.0;
    if white.len() != 1 && white.len() != raw.cpp && !(raw.cpp == 1 && white.len() == 4) {
        return Err(IoError::UnsupportedRaw(
            "Unsupported RAW white-level pattern".into(),
        ));
    }
    for black in black.as_vec() {
        if !black.is_finite() || black < 0.0 || white.iter().any(|&white| white as f32 <= black) {
            return Err(invalid(
                "RAW white level must be finite and greater than black level",
            ));
        }
    }
    for area in [raw.active_area, raw.crop_area].into_iter().flatten() {
        if area.d.w == 0
            || area.d.h == 0
            || area.p.x.checked_add(area.d.w).is_none_or(|x| x > raw.width)
            || area
                .p
                .y
                .checked_add(area.d.h)
                .is_none_or(|y| y > raw.height)
        {
            return Err(invalid("RAW crop lies outside the sensor"));
        }
    }
    if let (Some(active), Some(crop)) = (raw.active_area, raw.crop_area)
        && (crop.p.x < active.p.x
            || crop.p.y < active.p.y
            || crop.p.x + crop.d.w > active.p.x + active.d.w
            || crop.p.y + crop.d.h > active.p.y + active.d.h)
    {
        return Err(invalid("RAW default crop lies outside its active area"));
    }
    if let RawPhotometricInterpretation::Cfa(cfa) = &raw.photometric {
        if raw.cpp != 1
            || !((cfa.cfa.is_rgb() && matches!(cfa.sensor, SensorType::Bayer | SensorType::Xtrans))
                || (cfa.cfa.unique_colors() == 4 && cfa.sensor == SensorType::Bayer))
        {
            return Err(IoError::UnsupportedRaw(
                "Unsupported sensor mosaic; expected Bayer or X-Trans".into(),
            ));
        }
        if raw.width < 16 || raw.height < 16 {
            return Err(invalid("RAW mosaic is too small for demosaicing"));
        }
    }
    Ok(())
}

/// Normalize the actual repeat pattern, including odd-sized mosaics and X-Trans.
/// Values above nominal white remain available until the rendering boundary.
fn normalized(raw: &RawImage, cancel: &AtomicBool) -> Result<Vec<f32>> {
    let black = raw.blacklevel.as_vec();
    let white = &raw.whitelevel.0;
    let mut data = raw.data.as_f32().into_owned();
    for (y, row) in data.chunks_exact_mut(raw.width * raw.cpp).enumerate() {
        cancelled(cancel)?;
        for (i, sample) in row.iter_mut().enumerate() {
            if !sample.is_finite() {
                return Err(invalid("Non-finite RAW sensor sample"));
            }
            let x = i / raw.cpp;
            let channel = i % raw.cpp;
            let b = black[((y % raw.blacklevel.height) * raw.blacklevel.width
                + x % raw.blacklevel.width)
                * raw.cpp
                + channel];
            let wi = if white.len() == 1 {
                0
            } else if raw.cpp == 1 {
                (y % 2) * 2 + x % 2
            } else {
                channel
            };
            *sample = (*sample - b).max(0.0) / (white[wi] as f32 - b);
        }
    }
    Ok(data)
}

fn camera_matrix(raw: &RawImage, channels: usize) -> Result<[[f32; 4]; 3]> {
    let (illuminant, matrix) = raw
        .color_matrix_find_first([
            Illuminant::D65,
            Illuminant::D50,
            Illuminant::A,
            Illuminant::B,
            Illuminant::C,
            Illuminant::D55,
            Illuminant::D75,
            Illuminant::Daylight,
            Illuminant::Flash,
        ])
        .ok_or_else(|| {
            IoError::UnsupportedRaw(
                "Missing camera color matrix; cannot accurately develop this camera variant".into(),
            )
        })?;
    if matrix.len() != channels * 3 || matrix.iter().any(|n| !n.is_finite()) {
        return Err(invalid("Invalid camera color matrix"));
    }
    let mut xyz_to_camera = [[0.0; 3]; 4];
    for (row, values) in xyz_to_camera.iter_mut().zip(matrix.as_chunks::<3>().0) {
        row.copy_from_slice(values);
    }
    // The stored matrix maps XYZ under its calibration illuminant to camera.
    // Convert D65 XYZ to that illuminant before applying the camera matrix.
    if illuminant != Illuminant::D65 {
        xyz_to_camera = multiply(
            &xyz_to_camera,
            &bradford_adaption_matrix(&Illuminant::D65, &illuminant),
        );
    }
    let camera_to_rgb = pseudo_inverse(normalize(multiply(&xyz_to_camera, &SRGB_TO_XYZ_D65)));
    if camera_to_rgb
        .iter()
        .flatten()
        .any(|v| !v.is_finite() || v.abs() > 100.0)
    {
        return Err(invalid("Singular or unstable camera color matrix"));
    }
    Ok(camera_to_rgb)
}

fn profile_matrix(raw: &RawImage, p: &DevelopParams, channels: usize) -> Result<[[f32; 4]; 3]> {
    let Some(digest) = p.camera_profile else {
        return camera_matrix(raw, channels);
    };
    let profile = crate::camera_profiles::load(&digest)?;
    if channels != 3 || !profile.compatible(&raw.clean_make, &raw.clean_model) {
        return Err(invalid("Camera profile does not match this camera"));
    }
    let blend = profile.blend(p.kelvin);
    if !profile.forward[0].is_empty() {
        let f: [[f32; 3]; 3] = std::array::from_fn(|r| {
            std::array::from_fn(|c| {
                profile.forward[0][r * 3 + c] * (1. - blend)
                    + profile.forward[1]
                        .get(r * 3 + c)
                        .copied()
                        .unwrap_or(profile.forward[0][r * 3 + c])
                        * blend
            })
        });
        let xyz = multiply(
            &bradford_adaption_matrix(&Illuminant::D50, &Illuminant::D65),
            &f,
        );
        let rgb = multiply(&rawler::imgop::xyz::XYZ_TO_SRGB_D65, &xyz);
        return Ok(std::array::from_fn(|r| {
            [rgb[r][0], rgb[r][1], rgb[r][2], 0.]
        }));
    }
    let mut cameras = [[[0.; 3]; 4]; 2];
    for (i, camera) in cameras.iter_mut().enumerate() {
        let values = if profile.matrices[i].is_empty() {
            &profile.matrices[0]
        } else {
            &profile.matrices[i]
        };
        for r in 0..3 {
            camera[r].copy_from_slice(&values[r * 3..r * 3 + 3]);
        }
        let illuminant = Illuminant::try_from(profile.illuminants[i])
            .map_err(|_| invalid("Unknown profile illuminant"))?;
        *camera = multiply(
            camera,
            &bradford_adaption_matrix(&Illuminant::D65, &illuminant),
        );
    }
    let camera = std::array::from_fn::<_, 4, _>(|r| {
        std::array::from_fn::<_, 3, _>(|c| {
            cameras[0][r][c] * (1. - blend) + cameras[1][r][c] * blend
        })
    });
    let result = pseudo_inverse(normalize(multiply(&camera, &SRGB_TO_XYZ_D65)));
    if result
        .iter()
        .flatten()
        .any(|v| !v.is_finite() || v.abs() > 100.)
    {
        return Err(invalid("Unstable camera profile matrix"));
    }
    Ok(result)
}

fn white_balance(raw: &RawImage, params: &DevelopParams, channels: usize) -> Result<[f32; 4]> {
    let mut wb = params.wb_override.unwrap_or(raw.wb_coeffs);
    if wb[..channels].iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return Err(IoError::UnsupportedRaw(
            "Missing or invalid as-shot white balance for this RAW variant".into(),
        ));
    }
    if let Some(k) = params.kelvin {
        let xyz = illuminant_xyz(k);
        let matrix = if let Some(digest) = params.camera_profile {
            let profile = crate::camera_profiles::load(&digest)?;
            let blend = profile.blend(Some(k));
            let mut matrices = [[[0.; 3]; 3]; 2];
            for (i, matrix) in matrices.iter_mut().enumerate() {
                let values = if profile.matrices[i].is_empty() {
                    &profile.matrices[0]
                } else {
                    &profile.matrices[i]
                };
                for r in 0..3 {
                    matrix[r].copy_from_slice(&values[r * 3..r * 3 + 3]);
                }
                let illuminant = Illuminant::try_from(profile.illuminants[i])
                    .map_err(|_| invalid("Unknown profile illuminant"))?;
                *matrix = multiply(
                    matrix,
                    &bradford_adaption_matrix(&Illuminant::D65, &illuminant),
                );
            }
            (0..9)
                .map(|i| {
                    matrices[0][i / 3][i % 3] * (1. - blend) + matrices[1][i / 3][i % 3] * blend
                })
                .collect::<Vec<_>>()
        } else {
            let (_, matrix) = raw
                .color_matrix_find_first([
                    Illuminant::D65,
                    Illuminant::D50,
                    Illuminant::A,
                    Illuminant::Daylight,
                ])
                .ok_or_else(|| invalid("No camera calibration for Kelvin white balance"))?;
            matrix.clone()
        };
        if matrix.len() != channels * 3 {
            return Err(invalid("Invalid camera white balance matrix"));
        }
        for (c, row) in matrix.as_chunks::<3>().0.iter().enumerate() {
            let response = row[0] * xyz[0] + row[1] * xyz[1] + row[2] * xyz[2];
            if response <= 0. || !response.is_finite() {
                return Err(invalid("White point is outside camera calibration"));
            }
            wb[c] = 1. / response;
        }
    }
    let green = wb[1];
    for value in &mut wb[..channels] {
        *value /= green;
    }
    wb[0] *= 2f32.powf(0.7 * params.temperature);
    wb[2] *= 2f32.powf(-0.7 * params.temperature);
    wb[1] *= 2f32.powf(-0.4 * params.tint);
    // Four-color sensors have a distinct fourth primary, not a second green.
    Ok(wb)
}

// CIE daylight locus above 4000 K, Planckian locus below it.
fn illuminant_xyz(t: f32) -> [f32; 3] {
    let x = if t < 4000. {
        -0.2661239e9 / t.powi(3) - 0.2343589e6 / t.powi(2) + 0.8776956e3 / t + 0.179910
    } else if t <= 7000. {
        -4.607e9 / t.powi(3) + 2.9678e6 / t.powi(2) + 99.11 / t + 0.244063
    } else {
        -2.0064e9 / t.powi(3) + 1.9018e6 / t.powi(2) + 247.48 / t + 0.237040
    };
    let y = if t < 4000. {
        -0.9549476 * x.powi(3) - 1.3741859 * x * x + 2.09137 * x - 0.16748867
    } else {
        -3. * x * x + 2.87 * x - 0.275
    };
    [x / y, 1., (1. - x - y) / y]
}

/// Develop an already decoded, linear RGB image through the same tone/detail path.
pub(super) fn render_raster(source: &Raster, params: &DevelopParams) -> Result<Raster> {
    if params.wide_gamut {
        return Err(invalid("Wide-gamut working space requires a RAW original"));
    }
    if params.camera_profile.is_some() {
        return Err(invalid("Camera profiles require a RAW original"));
    }
    if params.sensor_noise_reduction > 0. {
        return Err(invalid("Sensor denoise requires a camera RAW mosaic"));
    }
    params.validate().map_err(invalid)?;
    if (DevelopParams {
        rotation: 0,
        ..*params
    }) == DevelopParams::default()
    {
        return advanced::geometry(source.clone(), params, &AtomicBool::new(false));
    }
    let original = source.to_pixels();
    let gain = if let Some(k) = params.kelvin {
        let white = illuminant_xyz(k);
        let d65 = illuminant_xyz(6504.);
        let matrix = rawler::imgop::xyz::XYZ_TO_SRGB_D65;
        std::array::from_fn(|c| {
            let response = (0..3).map(|i| matrix[c][i] * white[i]).sum::<f32>();
            let reference = (0..3).map(|i| matrix[c][i] * d65[i]).sum::<f32>();
            (reference / response.max(0.01)).clamp(0.05, 20.)
        })
    } else {
        [1.; 3]
    };
    let wb = [
        gain[0] * 2f32.powf(params.temperature * 0.7),
        gain[1] * 2f32.powf(-params.tint * 0.4),
        gain[2] * 2f32.powf(-params.temperature * 0.7),
    ];
    let pixels = original
        .iter()
        .map(|p| {
            std::array::from_fn(|c| {
                if p[3] > 0 {
                    p[c] as f32 / p[3] as f32 * wb[c]
                } else {
                    0.
                }
            })
        })
        .collect();
    let neutral_geometry = DevelopParams {
        rotation: 0,
        crop: [0., 0., 1., 1.],
        straighten: 0.,
        perspective: [0.; 2],
        distortion: 0.,
        aberration: [0.; 2],
        lens_profile: None,
        ..*params
    };
    let result = finish(
        source.width() as usize,
        source.height() as usize,
        pixels,
        Orientation::Normal,
        &neutral_geometry,
        &AtomicBool::new(false),
    )?;
    let mut pixels = result.to_pixels();
    for (out, input) in pixels.iter_mut().zip(original) {
        for value in &mut out[..3] {
            *value = (*value as f32 * input[3] as f32 / 65535.)
                .round()
                .min(input[3] as f32) as u16;
        }
        out[3] = input[3];
    }
    advanced::geometry(
        Raster::from_pixels(source.width(), source.height(), [0; 4], &pixels),
        params,
        &AtomicBool::new(false),
    )
}

fn shape(value: f32, params: &DevelopParams) -> f32 {
    let mut value = (value * 2f32.powf(params.exposure)).max(0.0);
    if params.highlights > 0.0 && value > 0.7 {
        let over = value - 0.7;
        value = 0.7 + over / (1.0 + over * params.highlights * 3.0);
    }
    if params.highlights < 0.0 && value > 0.5 {
        value += -params.highlights * (value - 0.5) * 0.5;
    }
    if params.shadows != 0.0 {
        value = value.powf(1.0 - 0.35 * params.shadows);
    }
    value = ((value - params.black_point) / (1.0 - params.black_point)).max(0.0);
    if params.whites != 0.0 || params.blacks != 0.0 {
        let level = value.clamp(0., 1.);
        value = (value
            + params.whites * level * level * 0.35
            + params.blacks * (1. - level).powi(2) * 0.08)
            .max(0.);
    }
    // Brightness moves midtones while leaving the black and white endpoints.
    if params.brightness != 0.0 {
        value = value.powf(2f32.powf(-params.brightness));
    }
    if params.contrast != 0.0 && value > 0.0 && value < 1.0 {
        let power = 2f32.powf(params.contrast);
        let a = value.powf(power);
        value = a / (a + (1.0 - value).powf(power));
    }
    value
}

fn luminance(rgb: [f32; 3]) -> f32 {
    rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722
}

fn curve_value(value: f32, params: &DevelopParams) -> f32 {
    if params.tone_curve == DevelopParams::LINEAR_CURVE {
        return value;
    }
    params
        .curve_output(value.clamp(0.0, 1.0).powf(1.0 / 2.2))
        .powf(2.2)
}

fn tone(pixel: [f32; 3], params: &DevelopParams) -> [f32; 3] {
    let mut pixel = advanced::calibrate(pixel, params).map(|v| shape(v, params));
    pixel = advanced::parametric(pixel, params);
    let before = luminance(pixel);
    if params.tone_curve != DevelopParams::LINEAR_CURVE {
        let after = curve_value(before, params);
        pixel = if before > 1e-8 {
            pixel.map(|v| v * after / before)
        } else {
            [after; 3]
        };
    }
    for (c, v) in pixel.iter_mut().enumerate() {
        if params.point_curves[0].len > 0 || params.point_curves[c + 1].len > 0 {
            let encoded = v.clamp(0., 1.).powf(1. / 2.2);
            *v = params
                .point_curve_output(c + 1, params.point_curve_output(0, encoded))
                .powf(2.2);
        }
    }
    if params.dehaze != 0.0 {
        if params.dehaze > 0.0 {
            let dark = pixel
                .iter()
                .copied()
                .fold(f32::INFINITY, f32::min)
                .clamp(0., 0.8);
            let transmission = (1. - params.dehaze * dark * 0.8).max(0.2);
            pixel = pixel.map(|v| (v - (1. - transmission)) / transmission);
        } else {
            let haze = -params.dehaze * 0.3;
            pixel = pixel.map(|v| v * (1. - haze) + haze);
        }
    }
    if params.saturation == 0.0 && params.vibrance == 0.0 {
        return advanced::color(pixel, params);
    }
    let gray = luminance(pixel);
    let max = pixel.iter().copied().fold(0., f32::max);
    let min = pixel.iter().copied().fold(f32::INFINITY, f32::min);
    let chroma = if max > 1e-6 {
        ((max - min) / max).clamp(0., 1.)
    } else {
        0.
    };
    let vibrance = 1. + params.vibrance * (1. - chroma);
    advanced::color(
        pixel.map(|v| gray + (v - gray) * (1.0 + params.saturation) * vibrance),
        params,
    )
}

pub(super) fn render(
    raw: &RawImage,
    params: &DevelopParams,
    cancel: &AtomicBool,
) -> Result<Raster> {
    render_in_space(raw, params, cancel, false)
}
pub(super) fn render_in_space(
    raw: &RawImage,
    params: &DevelopParams,
    cancel: &AtomicBool,
    working: bool,
) -> Result<Raster> {
    params.validate().map_err(invalid)?;
    cancelled(cancel)?;
    validate(raw)?;
    let developed = demosaic_denoised(raw, params.sensor_noise_reduction, cancel)?;
    let (w, h, mut pixels) = working_rgb(raw, params, developed)?;
    if let Some(d) = params.camera_profile {
        crate::camera_profiles::apply(
            crate::camera_profiles::load(&d)?.as_ref(),
            &mut pixels,
            params.kelvin,
        )?;
    }
    cancelled(cancel)?;
    finish_in_space(w, h, pixels, raw.orientation, params, cancel, working)
}

// Preview camera samples stay in floating point before WB/matrix conversion.
// The global byte reservation spans all open RAW sources and is released on drop.
static PREVIEW_BYTES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
const PREVIEW_BUDGET: usize = 128 * 1024 * 1024;
pub(super) struct PreviewStage {
    strength: f32,
    width: usize,
    height: usize,
    channels: usize,
    data: Vec<[f32; 4]>,
    bytes: usize,
}
impl Drop for PreviewStage {
    fn drop(&mut self) {
        PREVIEW_BYTES.fetch_sub(self.bytes, std::sync::atomic::Ordering::Relaxed);
    }
}
fn reserve_preview(bytes: usize) -> bool {
    PREVIEW_BYTES
        .fetch_update(
            std::sync::atomic::Ordering::Relaxed,
            std::sync::atomic::Ordering::Relaxed,
            |used| {
                used.checked_add(bytes)
                    .filter(|total| *total <= PREVIEW_BUDGET)
            },
        )
        .is_ok()
}
pub(super) fn preview(
    raw: &RawImage,
    p: &DevelopParams,
    cache: &std::sync::Mutex<Option<PreviewStage>>,
    cancel: &AtomicBool,
) -> Result<Raster> {
    p.validate().map_err(invalid)?;
    cancelled(cancel)?;
    let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
    if cache
        .as_ref()
        .is_some_and(|s| s.strength != p.sensor_noise_reduction)
    {
        *cache = None;
    }
    if cache.is_none() {
        validate(raw)?;
        let developed = demosaic_denoised(raw, p.sensor_noise_reduction, cancel)?;
        let (w, h, channels) = match &developed {
            Intermediate::Monochrome(v) => (v.width, v.height, 1),
            Intermediate::ThreeColor(v) => (v.width, v.height, 3),
            Intermediate::FourColor(v) => (v.width, v.height, 4),
        };
        let scale = (1280. / w.max(h) as f64).min(1.);
        let (pw, ph) = (
            (w as f64 * scale).round().max(1.) as usize,
            (h as f64 * scale).round().max(1.) as usize,
        );
        let bytes = pw * ph * std::mem::size_of::<[f32; 4]>();
        if !reserve_preview(bytes) {
            return Err(invalid(
                "Preview cache budget is in use; close another RAW preview",
            ));
        }
        let mut stage = PreviewStage {
            strength: p.sensor_noise_reduction,
            width: pw,
            height: ph,
            channels,
            data: vec![[0.; 4]; pw * ph],
            bytes,
        };
        for y in 0..ph {
            cancelled(cancel)?;
            let y0 = y * h / ph;
            let y1 = ((y + 1) * h / ph).max(y0 + 1);
            for x in 0..pw {
                let x0 = x * w / pw;
                let x1 = ((x + 1) * w / pw).max(x0 + 1);
                let sum = &mut stage.data[y * pw + x];
                for sy in y0..y1 {
                    for sx in x0..x1 {
                        let sample = match &developed {
                            Intermediate::Monochrome(v) => [v.data[sy * w + sx], 0., 0., 0.],
                            Intermediate::ThreeColor(v) => {
                                let v = v.data[sy * w + sx];
                                [v[0], v[1], v[2], 0.]
                            }
                            Intermediate::FourColor(v) => v.data[sy * w + sx],
                        };
                        for c in 0..channels {
                            sum[c] += sample[c];
                        }
                    }
                }
                let count = ((y1 - y0) * (x1 - x0)) as f32;
                for v in sum {
                    *v /= count;
                }
            }
        }
        *cache = Some(stage);
    }
    let stage = cache.as_ref().unwrap();
    let mut pixels: Vec<[f32; 3]> = if stage.channels == 1 {
        stage.data.iter().map(|v| [v[0]; 3]).collect()
    } else {
        let matrix = profile_matrix(raw, p, stage.channels)?;
        let wb = white_balance(raw, p, stage.channels)?;
        stage
            .data
            .par_iter()
            .map(|v| transform(*v, matrix, wb))
            .collect()
    };
    cancelled(cancel)?;
    if let Some(d) = p.camera_profile {
        crate::camera_profiles::apply(
            crate::camera_profiles::load(&d)?.as_ref(),
            &mut pixels,
            p.kelvin,
        )?;
    }
    finish(
        stage.width,
        stage.height,
        pixels,
        raw.orientation,
        p,
        cancel,
    )
}

fn demosaic(raw: &RawImage, cancel: &AtomicBool) -> Result<Intermediate> {
    demosaic_denoised(raw, 0., cancel)
}
fn demosaic_denoised(raw: &RawImage, strength: f32, cancel: &AtomicBool) -> Result<Intermediate> {
    let mut linear = raw.clone();
    let mut data = normalized(raw, cancel)?;
    if strength > 0. {
        let RawPhotometricInterpretation::Cfa(cfa) = &raw.photometric else {
            return Err(invalid("Sensor denoise requires a CFA RAW original"));
        };
        let source = &data;
        let rows: Result<Vec<Vec<f32>>> = source
            .par_chunks(raw.width)
            .enumerate()
            .map(|(y, row)| {
                cancelled(cancel)?;
                Ok(row
                    .iter()
                    .enumerate()
                    .map(|(x, &center)| {
                        let color = cfa.cfa.color_at(y, x);
                        let sigma = 0.002 + 0.025 * strength * center.max(0.).sqrt();
                        let mut sum = center;
                        let mut weight = 1.;
                        for yy in y.saturating_sub(3)..=(y + 3).min(raw.height - 1) {
                            for xx in x.saturating_sub(3)..=(x + 3).min(raw.width - 1) {
                                if (xx == x && yy == y) || cfa.cfa.color_at(yy, xx) != color {
                                    continue;
                                }
                                let value = source[yy * raw.width + xx];
                                let distance =
                                    (xx as f32 - x as f32).powi(2) + (yy as f32 - y as f32).powi(2);
                                let w = (-(value - center).powi(2) / (2. * sigma * sigma)
                                    - distance / 18.)
                                    .exp();
                                sum += value * w;
                                weight += w;
                            }
                        }
                        center + (sum / weight - center) * strength
                    })
                    .collect())
            })
            .collect();
        data = rows?.into_iter().flatten().collect();
    }
    linear.data = RawImageData::Float(data);
    // Keep rawler's sensor demosaic and crop implementation, but skip its
    // calibration: that path modifies over-range colors before exposure.
    let dev = RawDevelop::new_with(&[
        ProcessingStep::Demosaic,
        ProcessingStep::FujiRotate,
        ProcessingStep::CropActiveArea,
        ProcessingStep::CropDefault,
    ]);
    cancelled(cancel)?;
    let developed = dev
        .develop_intermediate(&linear)
        .map_err(crate::raw_probe::decoder_error)?;
    drop(linear);
    cancelled(cancel)?;
    Ok(developed)
}

fn working_rgb(
    raw: &RawImage,
    params: &DevelopParams,
    developed: Intermediate,
) -> Result<(usize, usize, Vec<[f32; 3]>)> {
    Ok(match developed {
        Intermediate::Monochrome(pixels) => (
            pixels.width,
            pixels.height,
            pixels.data.into_iter().map(|v| [v; 3]).collect(),
        ),
        Intermediate::ThreeColor(pixels) => {
            let matrix = profile_matrix(raw, params, 3)?;
            let wb = white_balance(raw, params, 3)?;
            let data = pixels
                .data
                .into_iter()
                .map(|p| transform([p[0], p[1], p[2], 0.0], matrix, wb))
                .collect();
            (pixels.width, pixels.height, data)
        }
        Intermediate::FourColor(pixels) => {
            let matrix = profile_matrix(raw, params, 4)?;
            let wb = white_balance(raw, params, 4)?;
            let data = pixels
                .data
                .into_iter()
                .map(|p| transform(p, matrix, wb))
                .collect();
            (pixels.width, pixels.height, data)
        }
    })
}

fn transform(pixel: [f32; 4], matrix: [[f32; 4]; 3], wb: [f32; 4]) -> [f32; 3] {
    matrix.map(|row| {
        (0..4)
            .map(|i| {
                if pixel[i] == 0.0 {
                    0.0
                } else {
                    row[i] * pixel[i] * wb[i]
                }
            })
            .sum()
    })
}

fn oriented_index(w: usize, h: usize, orientation: Orientation, x: usize, y: usize) -> usize {
    match orientation {
        Orientation::HorizontalFlip => y * w + w - 1 - x,
        Orientation::Rotate180 => (h - 1 - y) * w + w - 1 - x,
        Orientation::VerticalFlip => (h - 1 - y) * w + x,
        Orientation::Transpose => x * w + y,
        Orientation::Rotate90 => (h - 1 - x) * w + y,
        Orientation::Transverse => (h - 1 - x) * w + w - 1 - y,
        Orientation::Rotate270 => x * w + w - 1 - y,
        _ => y * w + x,
    }
}

pub(super) fn neutral_white_balance(
    raw: &RawImage,
    params: &DevelopParams,
    x: u32,
    y: u32,
) -> Result<DevelopParams> {
    params.validate().map_err(invalid)?;
    validate(raw)?;
    let developed = demosaic(raw, &AtomicBool::new(false))?;
    let sample = |w: usize, h: usize| -> Result<usize> {
        let swap = matches!(
            raw.orientation,
            Orientation::Transpose
                | Orientation::Rotate90
                | Orientation::Transverse
                | Orientation::Rotate270
        );
        let (ow, oh) = if swap { (h, w) } else { (w, h) };
        if x as usize >= ow || y as usize >= oh {
            return Err(invalid("White balance sample is outside the RAW image"));
        }
        Ok(oriented_index(
            w,
            h,
            raw.orientation,
            x as usize,
            y as usize,
        ))
    };
    let (pixel, channels) = match developed {
        Intermediate::Monochrome(_) => {
            return Err(IoError::UnsupportedRaw(
                "Neutral white balance requires a color sensor".into(),
            ));
        }
        Intermediate::ThreeColor(p) => {
            let v = p.data[sample(p.width, p.height)?];
            ([v[0], v[1], v[2], 0.0], 3)
        }
        Intermediate::FourColor(p) => (p.data[sample(p.width, p.height)?], 4),
    };
    if pixel[..channels]
        .iter()
        .any(|v| !v.is_finite() || *v < 0.005 || *v >= 0.98)
    {
        return Err(IoError::UnsupportedRaw(
            "Choose a neutral sample that is neither dark nor sensor-clipped".into(),
        ));
    }
    let mut wb = [1.0; 4];
    for channel in 0..channels {
        wb[channel] = pixel[1] / pixel[channel];
    }
    let result = DevelopParams {
        wb_override: Some(wb),
        kelvin: None,
        temperature: 0.0,
        tint: 0.0,
        ..*params
    };
    result.validate().map_err(invalid)?;
    Ok(result)
}

pub(super) fn auto_adjust(raw: &RawImage, params: &DevelopParams) -> Result<DevelopParams> {
    params.validate().map_err(invalid)?;
    validate(raw)?;
    let (_, _, pixels) = working_rgb(raw, params, demosaic(raw, &AtomicBool::new(false))?)?;
    let stride = pixels.len().div_ceil(65536).max(1);
    let samples: Vec<_> = pixels
        .iter()
        .step_by(stride)
        .map(|p| p.map(|v| v.max(0.0)))
        .collect();
    let mut levels: Vec<_> = samples
        .iter()
        .map(|p| p.iter().copied().fold(0.0f32, f32::max))
        .collect();
    if levels.iter().any(|v| !v.is_finite()) {
        return Err(invalid("Invalid RAW auto-adjust samples"));
    }
    levels.sort_by(f32::total_cmp);
    let mut result = DevelopParams {
        exposure: 0.0,
        black_point: 0.0,
        brightness: 0.0,
        contrast: 0.0,
        highlights: 0.0,
        shadows: 0.0,
        ..*params
    };
    let Some(&white) = levels.get((levels.len().saturating_sub(1) * 995) / 1000) else {
        return Ok(result);
    };
    if white <= 1e-6 {
        return Ok(result);
    }
    result.exposure = (0.95 / white).log2().clamp(-5.0, 5.0);
    let gain = 2f32.powf(result.exposure);
    // Ignore a small dark tail, but do not crush an entirely low-contrast scene.
    let low = levels[(levels.len() - 1) / 200];
    result.black_point = (low * gain * 0.5).clamp(0.0, 0.1);
    let mut mids: Vec<_> = samples
        .iter()
        .map(|p| {
            ((luminance(*p) * gain - result.black_point) / (1.0 - result.black_point))
                .clamp(0.0, 1.0)
        })
        .collect();
    mids.sort_by(f32::total_cmp);
    let median = mids[mids.len() / 2];
    if median > 1e-6 && median < 0.999 {
        result.brightness = -(0.18f32.ln() / median.ln()).log2().clamp(-1.0, 1.0);
    }
    result.validate().map_err(invalid)?;
    Ok(result)
}

fn finish(
    w: usize,
    h: usize,
    rgb: Vec<[f32; 3]>,
    orientation: Orientation,
    params: &DevelopParams,
    cancel: &AtomicBool,
) -> Result<Raster> {
    finish_in_space(w, h, rgb, orientation, params, cancel, false)
}
fn finish_in_space(
    w: usize,
    h: usize,
    mut rgb: Vec<[f32; 3]>,
    orientation: Orientation,
    params: &DevelopParams,
    cancel: &AtomicBool,
    working: bool,
) -> Result<Raster> {
    if params.wide_gamut {
        crate::photo_color::convert_float(
            &mut rgb,
            crate::photo_color::Space::Srgb,
            crate::photo_color::Space::ProPhoto,
        )?;
    }
    crate::import::check_size(w as u32, h as u32)?;
    if rgb.len() != w * h || rgb.iter().flatten().any(|v| !v.is_finite()) {
        return Err(invalid("Invalid developed RAW pixels"));
    }
    let swap = matches!(
        orientation,
        Orientation::Transpose
            | Orientation::Rotate90
            | Orientation::Transverse
            | Orientation::Rotate270
    );
    let (ow, oh) = if swap { (h, w) } else { (w, h) };
    let spatial = params.color_noise_reduction != 0.
        || params.texture != 0.
        || params.clarity != 0.
        || params.sharpening != 0.
        || params.noise_reduction != 0.;
    let rgb = if spatial {
        let toned: Vec<_> = rgb.into_par_iter().map(|v| tone(v, params)).collect();
        detail::apply(w, h, toned, params, cancel)?
    } else {
        rgb
    };
    let local_adjustments = params.masks.iter().any(|m| m.enabled);
    let bitmaps = crate::photo_develop::load_masks(params)?;
    let edge = TILE as usize;
    let columns = ow.div_ceil(edge);
    // Write linear RGBA16 directly into tiles, avoiding a full-frame buffer
    // and copy. Each worker keeps the scalar tone/quantization order intact.
    let tiles: Result<Vec<_>> = (0..columns * oh.div_ceil(edge))
        .into_par_iter()
        .map(|i| -> Result<_> {
            cancelled(cancel)?;
            let (tx, ty) = (i % columns, i / columns);
            let (x0, y0) = (tx * edge, ty * edge);
            let mut tile = vec![[0; 4]; TILE_PX];
            for ly in 0..edge.min(oh - y0) {
                cancelled(cancel)?;
                for lx in 0..edge.min(ow - x0) {
                    let index = oriented_index(w, h, orientation, x0 + lx, y0 + ly);
                    let mut p = if spatial {
                        rgb[index]
                    } else {
                        tone(rgb[index], params)
                    };
                    if local_adjustments {
                        p = advanced::local(
                            p,
                            ((x0 + lx) as f32 + 0.5) / ow as f32,
                            ((y0 + ly) as f32 + 0.5) / oh as f32,
                            params,
                            &bitmaps,
                        );
                    }
                    if params.vignette != 0. {
                        let nx = ((x0 + lx) as f32 + 0.5) / ow as f32 * 2. - 1.;
                        let ny = ((y0 + ly) as f32 + 0.5) / oh as f32 * 2. - 1.;
                        let falloff = ((nx * nx + ny * ny) * 0.5).clamp(0., 1.);
                        let gain = 2f32.powf(params.vignette * falloff * 2.);
                        p = p.map(|v| v * gain);
                    }
                    let p = p.map(|v| (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16);
                    tile[ly * edge + lx] = [p[0], p[1], p[2], u16::MAX];
                }
            }
            Ok((TileCoord::new(tx as i32, ty as i32), tile.into()))
        })
        .collect();
    let tiles = tiles?;
    cancelled(cancel)?;
    let raster = Raster::from_tiles(ow as u32, oh as u32, [0; 4], tiles)
        .ok_or_else(|| invalid("Invalid developed RAW tiles"))?;
    let raster = if let Some(digest) = params.local_edits {
        crate::develop_edits::apply(raster, &crate::develop_edits::load(&digest)?, cancel)?
    } else {
        raster
    };
    let raster = advanced::geometry(raster, params, cancel)?;
    if params.wide_gamut && !working {
        crate::photo_color::convert_raster(
            raster,
            crate::photo_color::Space::ProPhoto,
            crate::photo_color::Space::Srgb,
        )
    } else {
        Ok(raster)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rawler::{
        decoders::Camera,
        rawimage::{BlackLevel, WhiteLevel},
    };
    use std::collections::HashMap;
    #[test]
    fn cached_preview_matches_full_small_source_and_reuses_camera_stage() {
        let raw = sensor();
        let cache = std::sync::Mutex::new(None);
        let cancel = AtomicBool::new(false);
        let p = DevelopParams::default();
        let preview = preview(&raw, &p, &cache, &cancel).unwrap();
        let full = render(&raw, &p, &cancel).unwrap();
        assert_eq!(preview.to_pixels(), full.to_pixels());
        let address = cache.lock().unwrap().as_ref().unwrap().data.as_ptr();
        let changed = DevelopParams {
            exposure: 0.7,
            temperature: 0.2,
            ..p
        };
        let next = super::preview(&raw, &changed, &cache, &cancel).unwrap();
        assert_eq!(
            address,
            cache.lock().unwrap().as_ref().unwrap().data.as_ptr()
        );
        assert_ne!(next.to_pixels(), preview.to_pixels());
        assert!(super::preview(&raw, &p, &cache, &AtomicBool::new(true)).is_err());
    }
    #[test]
    fn new_color_controls_render_and_neutral_calibration_is_neutral() {
        let p = DevelopParams {
            calibration: [[0.6, 0.5], [-0.2, 0.4], [0.3, -0.4]],
            ..Default::default()
        };
        assert_eq!(advanced::calibrate([0.4; 3], &p), [0.4; 3]);
        for p in [
            p,
            DevelopParams {
                parametric: [0., 0.6, 0., 0.],
                ..Default::default()
            },
            DevelopParams {
                global_grading: [240., 0.5, 0.1],
                ..Default::default()
            },
        ] {
            assert_ne!(
                tone([0.2, 0.3, 0.4], &p),
                tone([0.2, 0.3, 0.4], &DevelopParams::default())
            );
        }
    }
    #[test]
    fn sensor_denoise_reduces_cfa_noise_and_preserves_original_samples() {
        let mut raw = sensor();
        raw.width = 64;
        raw.height = 64;
        raw.cpp = 1;
        raw.whitelevel = WhiteLevel::new(vec![4095]);
        raw.blacklevel = BlackLevel::new(&[64u16], 1, 1, 1);
        let cfa = rawler::CFA::new("RGGB");
        raw.photometric = RawPhotometricInterpretation::Cfa(rawler::rawimage::CFAConfig::new(
            &cfa,
            &Default::default(),
        ));
        raw.data = RawImageData::Integer(
            (0..4096)
                .map(|i| {
                    let color = cfa.color_at(i / 64, i % 64);
                    let signal = [0.2, 0.3, 0.4][color];
                    let noise = if (i * 13 + i / 64 * 7) % 5 < 2 {
                        -0.005
                    } else {
                        0.005
                    };
                    (64. + (signal + noise) * 4031.) as u16
                })
                .collect(),
        );
        let before = raw.data.as_f32().into_owned();
        let cancel = AtomicBool::new(false);
        let plain = render(&raw, &DevelopParams::default(), &cancel).unwrap();
        let denoised = render(
            &raw,
            &DevelopParams {
                sensor_noise_reduction: 1.,
                ..Default::default()
            },
            &cancel,
        )
        .unwrap();
        let variance = |r: &Raster| {
            let pixels: Vec<f32> = (12..52)
                .flat_map(|y| (12..52).map(move |x| r.get(x, y)[1] as f32))
                .collect();
            let mean = pixels.iter().sum::<f32>() / pixels.len() as f32;
            pixels.iter().map(|v| (v - mean).powi(2)).sum::<f32>()
        };
        assert!(variance(&denoised) < variance(&plain));
        assert_eq!(raw.data.as_f32().into_owned(), before);
    }
    fn sensor() -> RawImage {
        RawImage {
            camera: Camera::default(),
            make: "Synthetic".into(),
            model: "Fixture".into(),
            clean_make: "Synthetic".into(),
            clean_model: "Fixture".into(),
            width: 2,
            height: 1,
            cpp: 3,
            bps: 14,
            wb_coeffs: [2.0, 1.0, 1.5, f32::NAN],
            whitelevel: WhiteLevel::new(vec![1100; 3]),
            blacklevel: BlackLevel::new(&[100u16; 3], 1, 1, 3),
            xyz_to_cam: [[0.0; 3]; 4],
            photometric: RawPhotometricInterpretation::LinearRaw,
            active_area: None,
            crop_area: None,
            blackareas: vec![],
            orientation: Orientation::Normal,
            data: RawImageData::Integer(vec![100, 600, 1100, 850, 600, 350]),
            color_matrix: HashMap::from([(
                Illuminant::D65,
                rawler::imgop::xyz::XYZ_TO_SRGB_D65
                    .into_iter()
                    .flatten()
                    .collect(),
            )]),
            dng_tags: HashMap::new(),
            fuji_rotation_width: None,
        }
    }
    #[test]
    fn tonal_defaults_identity_and_curves_are_monotonic() {
        for curve in [
            DevelopParams::LINEAR_CURVE,
            DevelopParams::MEDIUM_CONTRAST_CURVE,
            DevelopParams::STRONG_CONTRAST_CURVE,
        ] {
            let params = DevelopParams {
                tone_curve: curve,
                ..Default::default()
            };
            let mut previous = 0.0;
            for i in 0..=1000 {
                let v = i as f32 / 1000.0;
                assert_eq!(tone([v; 3], &DevelopParams::default()), [v; 3]);
                let next = curve_value(v, &params);
                assert!(next >= previous);
                previous = next;
            }
            assert_eq!(curve_value(0.0, &params), 0.0);
            assert_eq!(curve_value(1.0, &params), 1.0);
        }
        for contrast in [-1.0, 1.0] {
            let p = DevelopParams {
                contrast,
                brightness: 0.5,
                ..Default::default()
            };
            assert_eq!(shape(0.0, &p), 0.0);
            assert_eq!(shape(1.0, &p), 1.0);
        }
        let p = DevelopParams {
            saturation: -1.0,
            ..Default::default()
        };
        let gray = tone([0.2, 0.4, 0.6], &p);
        assert_eq!(gray[0], gray[1]);
        assert_eq!(gray[1], gray[2]);
        let p = DevelopParams {
            black_point: 0.1,
            ..Default::default()
        };
        assert_eq!(shape(0.05, &p), 0.0);
        assert_eq!(shape(1.0, &p), 1.0);
    }

    #[test]
    fn auto_is_deterministic_finite_and_preserves_white_balance() {
        let mut raw = sensor();
        let p = DevelopParams {
            temperature: 0.3,
            tint: -0.2,
            ..Default::default()
        };
        let a = auto_adjust(&raw, &p).unwrap();
        assert_eq!(a, auto_adjust(&raw, &p).unwrap());
        assert_eq!(a.temperature, p.temperature);
        assert_eq!(a.tint, p.tint);
        a.validate().unwrap();
        raw.data = RawImageData::Integer(vec![100; 6]);
        let black = auto_adjust(&raw, &p).unwrap();
        black.validate().unwrap();
        assert_eq!(black.exposure, 0.0);
    }

    #[test]
    fn neutral_sample_uses_oriented_camera_channels_and_rejects_clipping() {
        for orientation in 1..=8 {
            let mut raw = sensor();
            raw.orientation = Orientation::from_u16(orientation);
            // First sensor pixel has clipped blue. Second is usable neutral.
            let swap = orientation >= 5;
            let (w, h) = if swap { (1, 2) } else { (2, 1) };
            for y in 0..h {
                for x in 0..w {
                    let index = oriented_index(2, 1, raw.orientation, x, y);
                    let result =
                        neutral_white_balance(&raw, &DevelopParams::default(), x as u32, y as u32);
                    if index == 0 {
                        assert!(result.is_err());
                    } else {
                        let p = result.unwrap();
                        let wb = p.wb_override.unwrap();
                        assert!((wb[0] - 2.0 / 3.0).abs() < 1e-6);
                        assert_eq!(wb[2], 2.0);
                        let pixels = render(&raw, &p, &AtomicBool::new(false)).unwrap();
                        let rgb = pixels.get(x as u32, y as u32);
                        assert!((rgb[0] as i32 - rgb[1] as i32).abs() < 5);
                        assert!((rgb[1] as i32 - rgb[2] as i32).abs() < 5);
                    }
                }
            }
        }
        let mut raw = sensor();
        raw.data = RawImageData::Integer(vec![100; 6]);
        assert!(neutral_white_balance(&raw, &DevelopParams::default(), 0, 0).is_err());
        assert!(neutral_white_balance(&raw, &DevelopParams::default(), 100, 0).is_err());
    }
    #[test]
    fn black_white_levels_and_exposure_preserve_headroom() {
        let raw = sensor();
        assert_eq!(
            normalized(&raw, &AtomicBool::new(false)).unwrap(),
            vec![0.0, 0.5, 1.0, 0.75, 0.5, 0.25]
        );
        let params = DevelopParams {
            exposure: -1.0,
            ..Default::default()
        };
        let result = render(&raw, &params, &AtomicBool::new(false)).unwrap();
        // WB takes red to 1.5; -1EV must recover 0.75, without pre-exposure clipping.
        assert!((result.get(1, 0)[0] as f32 / 65535.0 - 0.75).abs() < 0.001);
        assert!((result.get(1, 0)[1] as f32 / 65535.0 - 0.25).abs() < 0.001);
    }
    #[test]
    fn rejects_bad_levels_samples_params_and_cancellation() {
        let mut raw = sensor();
        raw.whitelevel = WhiteLevel::new(vec![100; 3]);
        assert!(render(&raw, &DevelopParams::default(), &AtomicBool::new(false)).is_err());
        let mut raw = sensor();
        raw.data = RawImageData::Float(vec![f32::NAN; 6]);
        assert!(render(&raw, &DevelopParams::default(), &AtomicBool::new(false)).is_err());
        let params = DevelopParams {
            exposure: f32::NAN,
            ..Default::default()
        };
        assert!(render(&sensor(), &params, &AtomicBool::new(false)).is_err());
        assert!(render(&sensor(), &DevelopParams::default(), &AtomicBool::new(true)).is_err());
    }

    #[test]
    fn manual_white_balance_changes_linear_channels_before_rendering() {
        let raw = sensor();
        let neutral = DevelopParams {
            exposure: -2.0,
            ..Default::default()
        };
        let warm = DevelopParams {
            temperature: 1.0,
            ..neutral
        };
        let baseline = render(&raw, &neutral, &AtomicBool::new(false))
            .unwrap()
            .get(1, 0);
        let changed = render(&raw, &warm, &AtomicBool::new(false))
            .unwrap()
            .get(1, 0);
        assert!(changed[0] > baseline[0]);
        assert_eq!(changed[1], baseline[1]);
        assert!(changed[2] < baseline[2]);
    }

    #[test]
    fn odd_dimensions_and_spatial_black_pattern_are_fully_normalized() {
        let mut raw = sensor();
        raw.width = 3;
        raw.height = 3;
        raw.cpp = 1;
        raw.photometric = RawPhotometricInterpretation::BlackIsZero;
        raw.blacklevel = BlackLevel::new(&[100u16, 200, 300, 400], 2, 2, 1);
        raw.whitelevel = WhiteLevel::new(vec![1100]);
        raw.data = RawImageData::Integer(vec![600, 650, 600, 700, 750, 700, 600, 650, 600]);
        validate(&raw).unwrap();
        assert_eq!(
            normalized(&raw, &AtomicBool::new(false)).unwrap(),
            vec![0.5; 9]
        );
    }

    #[test]
    fn invalid_matrix_and_buffer_fail_without_inventing_color() {
        let mut raw = sensor();
        raw.color_matrix.clear();
        assert!(render(&raw, &DevelopParams::default(), &AtomicBool::new(false)).is_err());
        let mut raw = sensor();
        raw.data = RawImageData::Integer(vec![1, 2]);
        assert!(render(&raw, &DevelopParams::default(), &AtomicBool::new(false)).is_err());
    }
    #[test]
    fn finish_matches_dense_reference_across_tiles_and_orientations() {
        let adjusted = DevelopParams {
            exposure: -0.75,
            black_point: 0.03,
            brightness: 0.2,
            contrast: -0.15,
            saturation: 0.3,
            highlights: -0.2,
            shadows: 0.1,
            tone_curve: DevelopParams::MEDIUM_CONTRAST_CURVE,
            ..Default::default()
        };
        let values = [
            -0.2,
            0.0,
            0.49 / 65535.0,
            0.51 / 65535.0,
            0.18,
            0.50001,
            1.0,
            1.7,
        ];
        for (w, h) in [(3, 2), (1, 257), (257, 1), (256, 256), (259, 257)] {
            let source: Vec<_> = (0..w * h)
                .map(|i| {
                    [
                        values[i % values.len()],
                        values[(i / w + 3) % values.len()],
                        values[(i * 5 + i / w + 1) % values.len()],
                    ]
                })
                .collect();
            for exif in 1..=8 {
                let orientation = Orientation::from_u16(exif);
                let (ow, oh) = if exif >= 5 { (h, w) } else { (w, h) };
                for params in [DevelopParams::default(), adjusted] {
                    // Preserve the former scalar finishing loop as an exact oracle.
                    let mut pixels = Vec::with_capacity(ow * oh);
                    for y in 0..oh {
                        for x in 0..ow {
                            let index = oriented_index(w, h, orientation, x, y);
                            let p = tone(source[index], &params)
                                .map(|v| (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16);
                            pixels.push([p[0], p[1], p[2], u16::MAX]);
                        }
                    }
                    let expected = Raster::from_pixels(ow as u32, oh as u32, [0; 4], &pixels);
                    let actual = finish(
                        w,
                        h,
                        source.clone(),
                        orientation,
                        &params,
                        &AtomicBool::new(false),
                    )
                    .unwrap();
                    assert_eq!((actual.width(), actual.height()), (ow as u32, oh as u32));
                    assert_eq!(actual.fill(), [0; 4]);
                    assert_eq!(actual.to_pixels(), pixels, "{w}x{h}, EXIF {exif}");
                    assert_eq!(actual.tile_count(), expected.tile_count());
                    for (coord, tile) in expected.base_tiles() {
                        assert_eq!(actual.base_tile(*coord), Some(tile));
                    }
                    assert_eq!(
                        actual.tile(1, emulsion_raster::TileCoord::new(0, 0)),
                        expected.tile(1, emulsion_raster::TileCoord::new(0, 0))
                    );
                }
            }
        }
    }

    #[test]
    fn finish_black_pixels_are_opaque_and_tile_padding_is_transparent() {
        use emulsion_raster::{TILE, TileCoord};

        let raster = finish(
            257,
            259,
            vec![[0.0; 3]; 257 * 259],
            Orientation::Normal,
            &DevelopParams::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(raster.tile_count(), 4);
        assert!(raster.to_pixels().iter().all(|p| *p == [0, 0, 0, 65535]));
        let edge = raster.base_tile(TileCoord::new(1, 1)).unwrap();
        for y in 0..TILE {
            for x in 0..TILE {
                let expected = if x == 0 && y < 3 {
                    [0, 0, 0, 65535]
                } else {
                    [0; 4]
                };
                assert_eq!(edge[(y * TILE + x) as usize], expected);
            }
        }
    }

    #[test]
    fn finish_rejects_malformed_pixels_and_cancellation() {
        let params = DevelopParams::default();
        let finish_pixels = |pixels| {
            finish(
                2,
                2,
                pixels,
                Orientation::Normal,
                &params,
                &AtomicBool::new(false),
            )
        };
        for len in [0, 3, 5] {
            assert!(matches!(
                finish_pixels(vec![[0.0; 3]; len]),
                Err(IoError::MalformedRaw(_))
            ));
        }
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for channel in 0..3 {
                let mut pixels = vec![[0.5; 3]; 4];
                pixels[3][channel] = bad;
                assert!(matches!(
                    finish_pixels(pixels),
                    Err(IoError::MalformedRaw(_))
                ));
            }
        }
        assert!(matches!(
            finish(
                259,
                257,
                vec![[0.5; 3]; 259 * 257],
                Orientation::Rotate90,
                &params,
                &AtomicBool::new(true),
            ),
            Err(IoError::Unsupported(message)) if message == "RAW development cancelled"
        ));
    }

    #[test]
    fn every_exif_orientation_is_honored() {
        let source: Vec<_> = (1..=6).map(|v| [v as f32 / 10.0; 3]).collect();
        let expected = [
            vec![1, 2, 3, 4, 5, 6],
            vec![3, 2, 1, 6, 5, 4],
            vec![6, 5, 4, 3, 2, 1],
            vec![4, 5, 6, 1, 2, 3],
            vec![1, 4, 2, 5, 3, 6],
            vec![4, 1, 5, 2, 6, 3],
            vec![6, 3, 5, 2, 4, 1],
            vec![3, 6, 2, 5, 1, 4],
        ];
        for (i, expected) in expected.into_iter().enumerate() {
            let raster = finish(
                3,
                2,
                source.clone(),
                Orientation::from_u16(i as u16 + 1),
                &DevelopParams::default(),
                &AtomicBool::new(false),
            )
            .unwrap();
            let actual: Vec<_> = raster
                .to_pixels()
                .iter()
                .map(|p| (p[0] as f32 / 65535.0 * 10.0).round() as i32)
                .collect();
            assert_eq!(actual, expected, "orientation {}", i + 1);
        }
    }
}

// HDR extraction bypasses tone curves and u16 quantization, preserving sensor range.
pub(super) fn linear_hdr(
    raw: &RawImage,
    wb: [f32; 4],
    cancel: &AtomicBool,
) -> Result<crate::photo_hdr::Frame> {
    validate(raw)?;
    if raw
        .width
        .checked_mul(raw.height)
        .is_none_or(|n| n > crate::photo_hdr::MAX_PIXELS)
    {
        return Err(invalid("HDR input exceeds 24 megapixels"));
    }
    let developed = demosaic(raw, cancel)?;
    let signals: Vec<f32> = match &developed {
        Intermediate::Monochrome(v) => v.data.clone(),
        Intermediate::ThreeColor(v) => v
            .data
            .iter()
            .map(|p| p.iter().copied().fold(0., f32::max))
            .collect(),
        Intermediate::FourColor(v) => v
            .data
            .iter()
            .map(|p| p.iter().copied().fold(0., f32::max))
            .collect(),
    };
    let params = DevelopParams {
        wb_override: Some(wb),
        ..Default::default()
    };
    let (w, h, rgb) = working_rgb(raw, &params, developed)?;
    let swap = matches!(
        raw.orientation,
        Orientation::Transpose
            | Orientation::Rotate90
            | Orientation::Transverse
            | Orientation::Rotate270
    );
    let (ow, oh) = if swap { (h, w) } else { (w, h) };
    let mut pixels = Vec::with_capacity(ow * oh);
    let mut signal = Vec::with_capacity(ow * oh);
    for y in 0..oh {
        cancelled(cancel)?;
        for x in 0..ow {
            let i = oriented_index(w, h, raw.orientation, x, y);
            pixels.push(rgb[i]);
            signal.push(signals[i]);
        }
    }
    Ok(crate::photo_hdr::Frame {
        image: crate::photo_hdr::FloatImage {
            width: ow as u32,
            height: oh as u32,
            pixels,
        },
        signal,
    })
}
pub(super) fn render_linear_rgb(
    w: u32,
    h: u32,
    mut pixels: Vec<[f32; 3]>,
    params: &DevelopParams,
    cancel: &AtomicBool,
) -> Result<Raster> {
    params.validate().map_err(invalid)?;
    let gain = if let Some(k) = params.kelvin {
        let white = illuminant_xyz(k);
        let d65 = illuminant_xyz(6504.);
        let m = rawler::imgop::xyz::XYZ_TO_SRGB_D65;
        std::array::from_fn(|c| {
            let response = (0..3).map(|i| m[c][i] * white[i]).sum::<f32>();
            let reference = (0..3).map(|i| m[c][i] * d65[i]).sum::<f32>();
            (reference / response.max(0.01)).clamp(0.05, 20.)
        })
    } else {
        [1.; 3]
    };
    let wb = [
        gain[0] * 2f32.powf(params.temperature * 0.7),
        gain[1] * 2f32.powf(-params.tint * 0.4),
        gain[2] * 2f32.powf(-params.temperature * 0.7),
    ];
    for p in &mut pixels {
        for c in 0..3 {
            p[c] *= wb[c];
        }
    }
    finish(
        w as usize,
        h as usize,
        pixels,
        Orientation::Normal,
        params,
        cancel,
    )
}
