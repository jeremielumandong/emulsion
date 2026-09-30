//! Soft effects are retained in document space; foreground geometry stays vector.
//! Baking happens during scene construction, never during pan/zoom painting.
use crate::{IoError, Result};
use base64::Engine as _;

const MAX_PIXELS: f64 = 1_048_576.;
const MAX_SIDE: f64 = 2048.;

fn dimensions(width: f64, height: f64) -> Result<(u32, u32)> {
    if !width.is_finite() || !height.is_finite() || width <= 0. || height <= 0. {
        return Err(IoError::Unsupported("Invalid shadow bounds".into()));
    }
    // Two samples per document pixel preserve small soft shadows. Large effects
    // use fewer samples, bounding both retained images and filter scratch space.
    let scale = 2_f64
        .min((MAX_PIXELS / width / height).sqrt())
        .min(MAX_SIDE / width.max(height));
    Ok((
        (width * scale).floor().max(1.) as u32,
        (height * scale).floor().max(1.) as u32,
    ))
}

pub(crate) fn image(
    body: &str,
    shadow: &str,
    id: u64,
    index: usize,
    bounds: [f64; 4],
) -> Result<String> {
    let [x, y, width, height] = bounds;
    let (w, h) = dimensions(width, height)?;
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"{x} {y} {width} {height}\" preserveAspectRatio=\"none\"><defs><g id=\"effect-art-{id}\">{body}</g></defs>{shadow}</svg>"
    );
    let tree = resvg::usvg::Tree::from_str(&svg, &crate::svg_vectors::options())
        .map_err(|e| IoError::Unsupported(e.to_string()))?;
    let mut pixels = resvg::tiny_skia::Pixmap::new(w, h)
        .ok_or_else(|| IoError::Unsupported("Cannot allocate shadow cache".into()))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixels.as_mut(),
    );
    let png = pixels
        .encode_png()
        .map_err(|e| IoError::Unsupported(e.to_string()))?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(png);
    Ok(format!(
        "<image id=\"viewport-shadow-{id}-{index}\" image-rendering=\"smooth\" x=\"{x}\" y=\"{y}\" width=\"{width}\" height=\"{height}\" preserveAspectRatio=\"none\" href=\"data:image/png;base64,{encoded}\"/>"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_storage_is_bounded_for_large_and_thin_artwork() {
        for (width, height) in [(200., 100.), (1e9, 1e9), (1e12, 1.), (1., 1e12)] {
            let (w, h) = dimensions(width, height).unwrap();
            assert!(u64::from(w) * u64::from(h) <= MAX_PIXELS as u64);
            assert!(w <= MAX_SIDE as u32 && h <= MAX_SIDE as u32);
            assert!(w > 0 && h > 0);
        }
        assert_eq!(dimensions(200., 100.).unwrap(), (400, 200));
        assert!(dimensions(f64::NAN, 10.).is_err());
        assert!(dimensions(0., 10.).is_err());
    }
}
