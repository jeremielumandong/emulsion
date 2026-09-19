//! Smart layers: source pixels plus a filter stack, rendered into a cache.
//!
//! Filters may spread past the source (blurs), so the cache can be larger
//! than the source; `offset` says where the cache's top-left sits in
//! source pixels (zero or negative). The cache is placed so that source
//! pixel (0, 0) lands exactly where it did before filtering, whatever the
//! placement's rotation or scale.

use emulsion_filters::{Filter, apply_stack};
use emulsion_raster::{Placement, Raster};
use glam::dvec2;
use std::sync::Arc;

/// Render the stack. Returns the cache and its offset in source pixels.
pub fn render(source: &Raster, filters: &[Filter]) -> (Arc<Raster>, (i32, i32)) {
    let (r, off) = apply_stack(source, filters);
    (Arc::new(r), off)
}

/// The placement to draw a cache of `cw × ch` with, given the source's
/// placement and size and the cache's offset.
pub fn cache_placement(
    p: &Placement,
    (sw, sh): (u32, u32),
    (cw, ch): (u32, u32),
    offset: (i32, i32),
) -> Placement {
    if offset == (0, 0) && (sw, sh) == (cw, ch) {
        return *p;
    }
    let want = p.to_doc(sw, sh).transform_point2(dvec2(0.0, 0.0));
    let mut q = *p;
    let got = q
        .to_doc(cw, ch)
        .transform_point2(dvec2(-offset.0 as f64, -offset.1 as f64));
    q.x += want.x - got.x;
    q.y += want.y - got.y;
    q
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_placement_keeps_source_origin_fixed() {
        let p = Placement {
            x: 100.0,
            y: 50.0,
            scale_x: 1.5,
            scale_y: 1.5,
            rotation: 30.0,
            flip_x: false,
            flip_y: false,
        };
        let q = cache_placement(&p, (200, 100), (240, 140), (-20, -20));
        let a = p.to_doc(200, 100).transform_point2(dvec2(0.0, 0.0));
        let b = q.to_doc(240, 140).transform_point2(dvec2(20.0, 20.0));
        assert!((a - b).length() < 1e-6, "{a} vs {b}");
        // And an arbitrary source pixel too, since scale and rotation are shared.
        let a = p.to_doc(200, 100).transform_point2(dvec2(150.0, 70.0));
        let b = q.to_doc(240, 140).transform_point2(dvec2(170.0, 90.0));
        assert!((a - b).length() < 1e-6);
    }

    #[test]
    fn render_reports_spread() {
        let src = Raster::solid(20, 20, [1.0, 0.0, 0.0, 1.0]);
        let (cache, off) = render(&src, &[Filter::GaussianBlur { radius: 4.0 }]);
        assert!(cache.width() > 20 && off.0 < 0);
        let (same, off0) = render(&src, &[]);
        assert_eq!((same.width(), off0), (20, (0, 0)));
    }
}
