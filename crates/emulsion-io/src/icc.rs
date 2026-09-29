//! Colour management on import: pictures that carry an ICC profile
//! (Display P3 phone photos, Adobe RGB camera JPEGs, ProPhoto TIFFs) are
//! converted to sRGB as they are decoded, so the working space is always
//! sRGB and every adjustment and recipe sees the colours the photographer
//! saw. CMYK TIFF and PSD inks are converted with their embedded profile,
//! with a subtractive approximation for missing or unusable profiles. JPEG
//! decoders already return RGB; their CMYK profiles must not be reapplied.
//! Unusable RGB profiles leave the decoded RGB values unchanged.

use moxcms::{ColorProfile, DataColorSpace, Layout, TransformExecutor, TransformOptions};

/// Does the transform leave colours alone (the profile is sRGB, or close
/// enough)? Probed on a handful of colours so a no-op pass is skipped.
fn is_identity_8(t: &dyn TransformExecutor<u8>) -> bool {
    let probe: [u8; 24] = [
        0, 0, 0, 255, 255, 255, 255, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 128, 64,
        200, 255,
    ];
    let mut out = [0u8; 24];
    t.transform(&probe, &mut out).is_ok()
        && probe
            .as_chunks::<4>()
            .0
            .iter()
            .zip(out.as_chunks::<4>().0)
            .all(|(a, b)| (0..3).all(|i| (a[i] as i32 - b[i] as i32).abs() <= 1))
}

/// Is `icc` an RGB profile whose colours already match sRGB?
pub(crate) fn is_srgb(icc: &[u8]) -> bool {
    ColorProfile::new_from_slice(icc).is_ok_and(|src| {
        src.color_space == DataColorSpace::Rgb
            && src
                .create_transform_8bit(
                    Layout::Rgba,
                    &ColorProfile::new_srgb(),
                    Layout::Rgba,
                    TransformOptions::default(),
                )
                .is_ok_and(|t| is_identity_8(t.as_ref()))
    })
}

/// Convert interleaved RGBA8 pixels in place from `icc` to sRGB. Returns
/// whether anything was changed.
pub fn to_srgb_8(icc: &[u8], rgba: &mut [u8]) -> bool {
    let Ok(src) = ColorProfile::new_from_slice(icc) else {
        return false;
    };
    // RGB decoders may already have converted CMYK. Never reinterpret alpha as K.
    if src.color_space == DataColorSpace::Cmyk {
        return false;
    }
    let dst = ColorProfile::new_srgb();
    let Ok(t) = src.create_transform_8bit(
        Layout::Rgba,
        &dst,
        Layout::Rgba,
        TransformOptions::default(),
    ) else {
        return false;
    };
    if is_identity_8(t.as_ref()) {
        return false;
    }
    let mut out = vec![0u8; rgba.len()];
    if t.transform(rgba, &mut out).is_err() {
        return false;
    }
    // Alpha is not part of the colour transform; keep the original.
    for (o, i) in out
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(rgba.as_chunks::<4>().0)
    {
        o[3] = i[3];
    }
    rgba.copy_from_slice(&out);
    true
}

/// Convert interleaved RGBA16 pixels in place from `icc` to sRGB.
pub fn to_srgb_16(icc: &[u8], rgba: &mut [u16]) -> bool {
    let Ok(src) = ColorProfile::new_from_slice(icc) else {
        return false;
    };
    // RGB decoders may already have converted CMYK. Never reinterpret alpha as K.
    if src.color_space == DataColorSpace::Cmyk {
        return false;
    }
    let dst = ColorProfile::new_srgb();
    if let Ok(t8) = src.create_transform_8bit(
        Layout::Rgba,
        &dst,
        Layout::Rgba,
        TransformOptions::default(),
    ) && is_identity_8(t8.as_ref())
    {
        return false;
    }
    let Ok(t) = src.create_transform_16bit(
        Layout::Rgba,
        &dst,
        Layout::Rgba,
        TransformOptions::default(),
    ) else {
        return false;
    };
    let mut out = vec![0u16; rgba.len()];
    if t.transform(rgba, &mut out).is_err() {
        return false;
    }
    for (o, i) in out
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(rgba.as_chunks::<4>().0)
    {
        o[3] = i[3];
    }
    rgba.copy_from_slice(&out);
    true
}

/// A decoded raster brought to sRGB when its file carried another profile.
pub fn to_srgb_raster(
    icc: &[u8],
    raster: emulsion_raster::Raster,
    depth: u8,
) -> emulsion_raster::Raster {
    let (w, h) = (raster.width(), raster.height());
    if depth == 16 {
        let mut px = raster.to_srgba16();
        if to_srgb_16(icc, &mut px) {
            return emulsion_raster::Raster::from_srgba16(w, h, &px);
        }
    } else {
        let mut px = raster.to_srgba8();
        if to_srgb_8(icc, &mut px) {
            return emulsion_raster::Raster::from_srgba8(w, h, &px);
        }
    }
    raster
}

/// Encode `profile` for embedding. moxcms stamps the header's creation date
/// (bytes 24..36) with the current time; pin it so the same picture always
/// exports to the same bytes.
pub(crate) fn encode_profile(profile: &ColorProfile) -> Result<Vec<u8>, moxcms::CmsError> {
    let mut bytes = profile.encode()?;
    // 2026-01-01 00:00:00, as big-endian u16 fields.
    const CREATED: [u8; 12] = [0x07, 0xea, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0];
    if let Some(date) = bytes.get_mut(24..36) {
        date.copy_from_slice(&CREATED);
    }
    Ok(bytes)
}

/// The sRGB profile as ICC bytes, for embedding on export.
pub fn srgb_profile() -> Option<Vec<u8>> {
    static PROFILE: std::sync::OnceLock<Option<Vec<u8>>> = std::sync::OnceLock::new();
    PROFILE
        .get_or_init(|| encode_profile(&ColorProfile::new_srgb()).ok())
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_profiles_do_not_carry_the_encoding_time() {
        let p3 = ColorProfile::new_display_p3();
        let first = encode_profile(&p3).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert_eq!(encode_profile(&p3).unwrap(), first);
        assert_eq!(&first[24..28], &[0x07, 0xea, 0, 1]);
        let srgb = srgb_profile().unwrap();
        assert_eq!(
            ColorProfile::new_from_slice(&srgb).unwrap().color_space,
            DataColorSpace::Rgb
        );
    }

    #[test]
    fn display_p3_red_becomes_a_less_saturated_srgb_red() {
        let p3 = ColorProfile::new_display_p3().encode().unwrap();
        // A P3 orange inside the sRGB gamut lands on different sRGB values
        // (P3 primaries are wider, so the same code means a purer colour);
        // neutral grey is neutral in both and stays put.
        let mut px = [200u8, 100, 50, 200, 128, 128, 128, 255];
        assert!(to_srgb_8(&p3, &mut px));
        assert_eq!(px[3], 200, "alpha untouched");
        assert_ne!(&px[..3], &[200, 100, 50], "{px:?}");
        assert!(
            px[0] >= 200 && px[1] < 100,
            "more saturated in sRGB terms: {px:?}"
        );
        assert!(
            (px[4] as i32 - 128).abs() < 6 && px[4] == px[5] && px[5] == px[6],
            "{px:?}"
        );
        let mut wide = [51400u16, 25700, 12850, 65535];
        assert!(to_srgb_16(&p3, &mut wide));
        assert!(wide[0] >= 51000 && wide[1] < 25700, "{wide:?}");
        // An sRGB profile is left alone.
        let srgb = srgb_profile().unwrap();
        let mut same = [10u8, 20, 30, 255];
        assert!(!to_srgb_8(&srgb, &mut same));
        assert_eq!(same, [10, 20, 30, 255]);
        assert!(!to_srgb_8(b"not a profile", &mut same));
    }
}

// moxcms uses Layout::Rgba for four CMYK inks; RGB output avoids treating K as alpha.
macro_rules! cmyk_converter {
    ($name:ident, $sample:ty, $transform:ident) => {
        /// Convert conventional CMYK ink samples to opaque sRGB pixels.
        /// Missing or unusable profiles use a subtractive approximation.
        pub(crate) fn $name(icc: Option<&[u8]>, cmyk: &[$sample]) -> Vec<$sample> {
            let mut rgb = vec![0; cmyk.len() / 4 * 3];
            let managed = icc
                .and_then(|bytes| ColorProfile::new_from_slice(bytes).ok())
                .filter(|profile| profile.color_space == DataColorSpace::Cmyk)
                .and_then(|profile| {
                    profile
                        .$transform(
                            Layout::Rgba,
                            &ColorProfile::new_srgb(),
                            Layout::Rgb,
                            TransformOptions::default(),
                        )
                        .ok()
                })
                .is_some_and(|transform| transform.transform(cmyk, &mut rgb).is_ok());
            if !managed {
                let max = <$sample>::MAX as u64;
                for (inks, out) in cmyk
                    .chunks_exact(4)
                    .zip(rgb.as_chunks_mut::<3>().0.iter_mut())
                {
                    for channel in 0..3 {
                        out[channel] = (((max - inks[channel] as u64) * (max - inks[3] as u64)
                            + max / 2)
                            / max) as $sample;
                    }
                }
            }
            rgb.chunks_exact(3)
                .flat_map(|p| [p[0], p[1], p[2], <$sample>::MAX])
                .collect()
        }
    };
}

cmyk_converter!(cmyk_to_srgba8, u8, create_transform_8bit);
cmyk_converter!(cmyk_to_srgba16, u16, create_transform_16bit);

#[cfg(test)]
mod cmyk_tests {
    use super::*;

    // A valid four-input ICC LUT whose PCS output is always black. This makes
    // the managed result observably different from unprofiled white paper.
    fn black_cmyk_profile() -> Vec<u8> {
        let mut profile = srgb_profile().unwrap()[..128].to_vec();
        profile[16..20].copy_from_slice(b"CMYK");
        let mut lut = b"mft1\0\0\0\0\x04\x03\x02\0".to_vec();
        for i in 0..9 {
            lut.extend_from_slice(&(if i % 4 == 0 { 65536u32 } else { 0 }).to_be_bytes());
        }
        for _ in 0..4 {
            lut.extend(0..=255u8);
        }
        lut.extend([0; 16 * 3]);
        for _ in 0..3 {
            lut.extend(0..=255u8);
        }
        profile.extend_from_slice(&1u32.to_be_bytes());
        profile.extend_from_slice(b"A2B0");
        profile.extend_from_slice(&144u32.to_be_bytes());
        profile.extend_from_slice(&(lut.len() as u32).to_be_bytes());
        profile.extend(lut);
        let len = profile.len() as u32;
        profile[..4].copy_from_slice(&len.to_be_bytes());
        profile
    }

    #[test]
    fn cmyk_profiles_transform_inks_and_never_rgb_alpha() {
        let profile = black_cmyk_profile();
        assert_eq!(cmyk_to_srgba8(None, &[0, 0, 0, 0]), [255; 4]);
        assert_eq!(
            cmyk_to_srgba8(Some(&profile), &[0, 0, 0, 0]),
            [0, 0, 0, 255]
        );
        assert_eq!(
            cmyk_to_srgba16(Some(&profile), &[0, 0, 0, 0]),
            [0, 0, 0, 65535]
        );
        let mut rgba = [255; 4];
        assert!(!to_srgb_8(&profile, &mut rgba));
        assert_eq!(rgba, [255; 4]);
        let mut wide = [65535; 4];
        assert!(!to_srgb_16(&profile, &mut wide));
        assert_eq!(wide, [65535; 4]);
    }

    #[test]
    fn unprofiled_inks_have_opaque_black_and_full_precision() {
        assert_eq!(
            cmyk_to_srgba8(Some(b"invalid"), &[0, 0, 0, 255, 255, 0, 0, 0]),
            [0, 0, 0, 255, 0, 255, 255, 255]
        );
        assert_eq!(
            cmyk_to_srgba16(None, &[1, 0, 0, 0]),
            [65534, 65535, 65535, 65535]
        );
    }
}

/// Explicit Library viewing transforms. Default sRGB presentation leaves color
/// handling to the display system; manual display conversion is opt-in.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PhotoView {
    pub display: Option<std::path::PathBuf>,
    pub proof: Option<std::path::PathBuf>,
    pub gamut_warning: bool,
}
fn view_profile(path: &std::path::Path, proof: bool) -> crate::Result<ColorProfile> {
    use std::io::Read;
    let mut bytes = vec![];
    std::fs::File::open(path)?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(crate::IoError::Unsupported(
            "ICC profile exceeds 4 MiB".into(),
        ));
    }
    let profile = ColorProfile::new_from_slice(&bytes)
        .map_err(|e| crate::IoError::Unsupported(format!("ICC profile: {e}")))?;
    if profile.color_space != DataColorSpace::Rgb
        && !(proof && profile.color_space == DataColorSpace::Cmyk)
    {
        return Err(crate::IoError::Unsupported(
            if proof {
                "Soft proofing requires an RGB or CMYK ICC profile"
            } else {
                "Display conversion requires an RGB ICC profile"
            }
            .into(),
        ));
    }
    Ok(profile)
}
impl PhotoView {
    pub fn validate(&self) -> crate::Result<()> {
        if let Some(path) = &self.display {
            view_profile(path, false)?;
        }
        if let Some(path) = &self.proof {
            view_profile(path, true)?;
        }
        // Confirm both directions are usable before accepting a printer profile.
        self.apply(&mut [0, 0, 0, 255])?;
        Ok(())
    }
    /// Input/output is BGRA8 display data. Export and histogram pixels are unchanged.
    pub fn apply(&self, bgra: &mut [u8]) -> crate::Result<()> {
        if self.display.is_none() && self.proof.is_none() {
            return Ok(());
        }
        let bad = |e: moxcms::CmsError| {
            crate::IoError::Unsupported(format!("ICC viewing transform: {e}"))
        };
        let srgb = ColorProfile::new_srgb();
        let mut pixels = bgra
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2] as f32 / 255., p[1] as f32 / 255., p[0] as f32 / 255.])
            .collect::<Vec<_>>();
        let mut gamut = vec![false; pixels.len() / 3];
        if let Some(path) = &self.proof {
            let proof = view_profile(path, true)?;
            let (layout, channels) = if proof.color_space == DataColorSpace::Cmyk {
                (Layout::Rgba, 4)
            } else {
                (Layout::Rgb, 3)
            };
            let transform = srgb
                .create_transform_f32(Layout::Rgb, &proof, layout, TransformOptions::default())
                .map_err(bad)?;
            let mut output = vec![0.; pixels.len() / 3 * channels];
            transform.transform(&pixels, &mut output).map_err(bad)?;
            for (index, p) in output.chunks_exact_mut(channels).enumerate() {
                gamut[index] = p.iter().any(|v| *v < -0.0001 || *v > 1.0001);
                for v in p {
                    *v = v.clamp(0., 1.);
                }
            }
            let original = pixels.clone();
            let reverse = proof
                .create_transform_f32(layout, &srgb, Layout::Rgb, TransformOptions::default())
                .map_err(bad)?;
            reverse.transform(&output, &mut pixels).map_err(bad)?;
            for (i, (a, b)) in original
                .as_chunks::<3>()
                .0
                .iter()
                .zip(pixels.as_chunks::<3>().0.iter())
                .enumerate()
            {
                gamut[i] |= a.iter().zip(b).any(|(a, b)| (a - b).abs() > 0.015);
            }
        }
        if let Some(path) = &self.display {
            let display = view_profile(path, false)?;
            let transform = srgb
                .create_transform_f32(
                    Layout::Rgb,
                    &display,
                    Layout::Rgb,
                    TransformOptions::default(),
                )
                .map_err(bad)?;
            let mut output = vec![0.; pixels.len()];
            transform.transform(&pixels, &mut output).map_err(bad)?;
            pixels = output;
        }
        for (index, (p, rgb)) in bgra
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(pixels.as_chunks::<3>().0.iter())
            .enumerate()
        {
            if self.gamut_warning && gamut[index] {
                p[..3].copy_from_slice(&[255, 0, 255]);
            } else {
                for c in 0..3 {
                    p[2 - c] = (rgb[c].clamp(0., 1.) * 255. + 0.5) as u8;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod photo_view_tests {
    use super::*;
    #[test]
    #[ignore = "Requires EMULSION_TEST_CMYK_PROFILE pointing to a printer ICC profile"]
    fn cmyk_soft_proof_preserves_alpha_and_rejects_printer_as_display() {
        let path = std::path::PathBuf::from(
            std::env::var_os("EMULSION_TEST_CMYK_PROFILE").expect("printer ICC fixture"),
        );
        let input = vec![20, 80, 160, 128, 190, 70, 25, 255, 255, 255, 255, 0];
        let view = PhotoView {
            proof: Some(path.clone()),
            ..Default::default()
        };
        view.validate().unwrap();
        let mut output = input.clone();
        view.apply(&mut output).unwrap();
        assert_ne!(input, output);
        for (a, b) in input
            .as_chunks::<4>()
            .0
            .iter()
            .zip(output.as_chunks::<4>().0)
        {
            assert_eq!(a[3], b[3]);
        }
        let mut repeated = input.clone();
        view.apply(&mut repeated).unwrap();
        assert_eq!(output, repeated);
        assert!(
            PhotoView {
                display: Some(path),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn viewing_is_opt_in_and_preserves_alpha() {
        let input = vec![20, 80, 160, 128, 190, 70, 25, 255];
        let mut untouched = input.clone();
        PhotoView::default().apply(&mut untouched).unwrap();
        assert_eq!(untouched, input);
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("srgb.icc");
        std::fs::write(&file, srgb_profile().unwrap()).unwrap();
        let view = PhotoView {
            display: Some(file.clone()),
            proof: Some(file),
            gamut_warning: true,
        };
        view.validate().unwrap();
        view.apply(&mut untouched).unwrap();
        for (a, b) in input.iter().zip(&untouched) {
            assert!((*a as i16 - *b as i16).abs() <= 1);
        }
        let bad = dir.path().join("bad.icc");
        std::fs::write(&bad, b"bad").unwrap();
        assert!(
            PhotoView {
                display: Some(bad),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
}
