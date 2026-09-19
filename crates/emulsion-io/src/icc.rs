//! Colour management on import: pictures that carry an ICC profile
//! (Display P3 phone photos, Adobe RGB camera JPEGs, ProPhoto TIFFs) are
//! converted to sRGB as they are decoded, so the working space is always
//! sRGB and every adjustment and recipe sees the colours the photographer
//! saw. Profiles that fail to parse are ignored and the picture is treated
//! as sRGB, which is what happened before.

use moxcms::{ColorProfile, Layout, TransformExecutor, TransformOptions};

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
