//! Display bytes that need no recipe can go straight to GPUI's BGRA image.

/// Preserve the compositor path for translucent pixels (premultiplication can
/// change their rounding). Ordinary opaque RAW previews need only a swizzle.
pub(super) fn opaque_bgra(mut rgba: Vec<u8>) -> Result<Vec<u8>, Vec<u8>> {
    if !rgba.len().is_multiple_of(4) || rgba.as_chunks::<4>().0.iter().any(|p| p[3] != 255) {
        return Err(rgba);
    }
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    Ok(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translucent_fallback_preserves_the_input_buffer() {
        let bytes = vec![13, 97, 240, 255, 9, 31, 80, 32];
        assert_eq!(opaque_bgra(bytes.clone()), Err(bytes));
        assert_eq!(opaque_bgra(vec![1, 2, 3]), Err(vec![1, 2, 3]));
    }
}
