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

/// The sRGB profile as ICC bytes, for embedding on export.
pub fn srgb_profile() -> Option<Vec<u8>> {
    ColorProfile::new_srgb().encode().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

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
                for (inks, out) in cmyk.chunks_exact(4).zip(rgb.chunks_exact_mut(3)) {
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
