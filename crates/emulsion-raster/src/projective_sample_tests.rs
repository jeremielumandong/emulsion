use super::*;
use crate::BlendMode;
use crate::blend::BlendSpace;
use crate::composite::{
    BlendingOptions, CompositeNode, CompositeTree, Knockout, NodeContent, Placement, flatten,
    render_tile_cpu,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn node(content: NodeContent) -> CompositeNode {
    CompositeNode {
        id: 1,
        visible: true,
        opacity: 1.0,
        blend: BlendMode::Normal,
        blending: BlendingOptions::default(),
        mask: None,
        clip_to: None,
        clip_rect: None,
        content,
    }
}
fn tree(nodes: Vec<CompositeNode>, space: BlendSpace, width: u32, height: u32) -> CompositeTree {
    let tree = CompositeTree {
        width,
        height,
        space,
        knockout_background: None,
        nodes,
    };
    tree.validate_projective_resources().unwrap();
    tree
}
fn projected(raster: Arc<Raster>, h: Projective2) -> CompositeNode {
    node(NodeContent::projective_pixels(raster.into(), h).unwrap())
}
fn pattern(width: u32, height: u32) -> Arc<Raster> {
    Arc::new(Raster::from_fn(width, height, [0; 4], |x, y| {
        let alpha = 20_000 + ((x * 97 + y * 23) % 40_000) as u16;
        [
            alpha / 2,
            ((x * 151) % u32::from(alpha)) as u16,
            ((y * 211) % u32::from(alpha)) as u16,
            alpha,
        ]
    }))
}

// Independent slow oracle: test-only Gauss-Jordan inverse, direct rational
// differentiation and four individual Plane reads. No production mapping,
// differential-domain, grid, interpolation or tile-cache helper is used.
fn invert(matrix: [f64; 9]) -> [f64; 9] {
    let mut a = [[0.0; 6]; 3];
    for (r, row) in a.iter_mut().enumerate() {
        row[..3].copy_from_slice(&matrix[r * 3..r * 3 + 3]);
        row[r + 3] = 1.0;
    }
    for c in 0..3 {
        let pivot = (c..3)
            .max_by(|a1, b1| a[*a1][c].abs().total_cmp(&a[*b1][c].abs()))
            .unwrap();
        a.swap(c, pivot);
        let d = a[c][c];
        for v in &mut a[c] {
            *v /= d;
        }
        let row = a[c];
        for (r, other) in a.iter_mut().enumerate() {
            if r != c {
                let k = other[c];
                for (v, n) in other.iter_mut().zip(row) {
                    *v -= k * n;
                }
            }
        }
    }
    std::array::from_fn(|i| a[i / 3][i % 3 + 3])
}
fn reference_point(inverse: [f64; 9], x: f64, y: f64, step: f64) -> Option<(DVec2, f64)> {
    let [a, b, c, d, e, f, g, h, i] = inverse;
    let (u, v, w) = (a * x + b * y + c, d * x + e * y + f, g * x + h * y + i);
    if w == 0.0 {
        return None;
    }
    let j = [
        (a * w - u * g) / (w * w),
        (b * w - u * h) / (w * w),
        (d * w - v * g) / (w * w),
        (e * w - v * h) / (w * w),
    ];
    let trace = j.iter().map(|v| v * v).sum::<f64>();
    let determinant = j[0] * j[3] - j[1] * j[2];
    let singular = ((trace
        + (trace * trace - 4.0 * determinant * determinant)
            .max(0.0)
            .sqrt())
        * 0.5)
        .sqrt();
    Some((
        DVec2::new(u / w, v / w),
        (singular * step).max(1.0).log2().floor(),
    ))
}
fn reference_sample<P: Pix>(
    plane: &Plane<P>,
    point: DVec2,
    mip: f64,
    outside: P,
) -> ([P; 4], f32, f32) {
    let level = mip.min(plane.max_level() as f64) as u32;
    let p = point / (1_u32 << level) as f64 - DVec2::splat(0.5);
    let (width, height) = plane.level_size(level);
    if !p.is_finite() || p.x < -1.0 || p.y < -1.0 || p.x > width as f64 || p.y > height as f64 {
        return ([outside; 4], 0.0, 0.0);
    }
    let (x, y) = (p.x.floor() as i64, p.y.floor() as i64);
    let values = [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(dx, dy)| {
        let (x, y) = (x + dx, y + dy);
        if x < 0 || y < 0 || x >= i64::from(width) || y >= i64::from(height) {
            return outside;
        }
        let coord = TileCoord::new((x / i64::from(TILE)) as i32, (y / i64::from(TILE)) as i32);
        plane.tile(level, coord).map_or(plane.fill(), |tile| {
            tile[(y % i64::from(TILE)) as usize * TILE as usize + (x % i64::from(TILE)) as usize]
        })
    });
    (
        values,
        (p.x - p.x.floor()) as f32,
        (p.y - p.y.floor()) as f32,
    )
}
fn lerp([a, b, c, d]: [f32; 4], x: f32, y: f32) -> f32 {
    (a * (1.0 - x) + b * x) * (1.0 - y) + (c * (1.0 - x) + d * x) * y
}
fn oracle(
    raster: &Raster,
    mask: Option<&Mask>,
    h: Projective2,
    level: u32,
    x: u32,
    y: u32,
) -> [f32; 4] {
    let step = (1_u32 << level) as f64;
    let Some((point, mip)) = reference_point(
        invert(h.to_row_major()),
        (x as f64 + 0.5) * step,
        (y as f64 + 0.5) * step,
        step,
    ) else {
        return [0.0; 4];
    };
    let (values, ax, ay) = reference_sample(raster, point, mip, [0; 4]);
    let values = values.map(color::px_to_f);
    let coverage = mask.map_or(1.0, |mask| {
        let (values, ax, ay) = reference_sample(mask, point, mip, mask.fill());
        lerp(values.map(f32::from), ax, ay) / 255.0
    });
    std::array::from_fn(|c| lerp(values.map(|p| p[c]), ax, ay) * coverage)
}
fn close(actual: [f32; 4], expected: [f32; 4], tolerance: f32) {
    for (a, e) in actual.into_iter().zip(expected) {
        assert!((a - e).abs() <= tolerance, "{actual:?} != {expected:?}");
    }
}

#[test]
fn perspective_rotated_reflected_profiles_masks_and_tile_seams_match_slow_oracle() {
    let raster = pattern(384, 320);
    let perspective =
        Projective2::from_row_major([0.72, 0.19, 65.0, -0.08, 0.65, 73.0, 0.0007, -0.0003, 1.0])
            .unwrap();
    let reflected = perspective
        .compose(
            Projective2::from_affine(DAffine2::from_cols_array(&[
                -1.0, 0.0, 0.0, 1.0, 384.0, 0.0,
            ]))
            .unwrap(),
        )
        .unwrap();
    for h in [perspective, reflected] {
        for fill in [0, 255] {
            // Deliberately different mask dimensions: its coordinates still
            // follow exactly H, never a dimension-dependent Placement frame.
            let mask = Arc::new(Mask::from_fn(193, 157, fill, |x, y| {
                ((x * 3 + y * 7) % 256) as u8
            }));
            for space in [
                BlendSpace::Linear,
                BlendSpace::Srgb,
                BlendSpace::PhotoshopSrgbV1,
            ] {
                let mut subject = projected(raster.clone(), h);
                subject.mask = Some(mask.clone());
                let tree = tree(vec![subject], space, 640, 512);
                for level in [0, 1, 2] {
                    let (width, height) =
                        crate::composite::level_size(tree.width, tree.height, level);
                    for tile in [TileCoord::new(0, 0), TileCoord::new(1, 0)] {
                        let actual = render_tile_cpu(&tree, level, tile);
                        assert_eq!(
                            actual,
                            render_tile_cpu(&tree, level, tile),
                            "repeat is deterministic"
                        );
                        for y in (0..TILE).step_by(7) {
                            for x in (0..TILE).step_by(5).chain([254, 255]) {
                                let (dx, dy) = (tile.x as u32 * TILE + x, tile.y as u32 * TILE + y);
                                let expected = if dx < width && dy < height {
                                    oracle(&raster, Some(&mask), h, level, dx, dy)
                                } else {
                                    [0.0; 4]
                                };
                                close(actual[(y * TILE + x) as usize], expected, 3e-6);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn local_minification_changes_mips_and_tracks_largest_inverse_singular_value() {
    let h =
        Projective2::from_row_major([0.45, 0.04, 40.0, 0.08, 0.4, 30.0, 0.001, 0.0, 1.0]).unwrap();
    let map = ProjectivePixelMapping::new(512, 256, h).unwrap();
    let raster = Raster::empty(512, 256, [0; 4]);
    let tile = ProjectiveTile::new(&map, 1.0, 0, 0);
    let levels: std::collections::BTreeSet<_> = tile
        .samples
        .iter()
        .flatten()
        .filter_map(|s| s.grid(&raster).map(|g| g.level))
        .collect();
    assert!(
        levels.len() > 1 && levels.iter().any(|level| *level > 0),
        "{levels:?}"
    );
    for (index, sample) in tile.samples.iter().enumerate().step_by(13) {
        let Some(sample) = sample else {
            continue;
        };
        let (point, mip) = reference_point(
            invert(h.to_row_major()),
            (index % TILE as usize) as f64 + 0.5,
            (index / TILE as usize) as f64 + 0.5,
            1.0,
        )
        .unwrap();
        assert!((point - sample.point).abs().max_element() < 1e-6);
        assert_eq!(sample.log_footprint.floor(), mip);
    }
}

#[test]
fn bilinear_edge_halo_survives_mathematical_quad_culling() {
    let raster = Arc::new(Raster::solid(8, 8, [0.4, 0.2, 0.1, 1.0]));
    let h =
        Projective2::from_row_major([1.0, 0.0, 10.75, 0.0, 1.0, 10.75, 0.002, 0.0, 1.0]).unwrap();
    let actual = render_tile_cpu(
        &tree(
            vec![projected(raster.clone(), h)],
            BlendSpace::Linear,
            32,
            32,
        ),
        0,
        TileCoord::new(0, 0),
    );
    // x=10.5 is outside the exact left edge x=10.75, but within its
    // half-source-pixel reconstruction support.
    let pixel = actual[(13 * TILE + 10) as usize];
    assert!(pixel[3] > 0.0 && pixel[3] < 1.0);
    close(pixel, oracle(&raster, None, h, 0, 10, 13), 2e-6);
}

#[test]
fn inverse_horizon_through_enclosing_aabb_is_not_a_sampling_window() {
    let h =
        Projective2::from_row_major([1.3, 10.0, 300.0, 0.2, 1.0, 200.0, 0.001, 0.0, 1.0]).unwrap();
    let map = ProjectivePixelMapping::new(256, 128, h).unwrap();
    let bounds = h.map_rect(map.intrinsic()).unwrap().bounds();
    let enclosing = ProjectiveRect::new(bounds.min(), bounds.max()).unwrap();
    assert!(
        h.inverse().unwrap().map_rect(enclosing).is_err(),
        "fixture must cross the inverse horizon"
    );
    let raster = pattern(256, 128);
    let tree = tree(
        vec![projected(raster.clone(), h)],
        BlendSpace::Linear,
        2048,
        512,
    );
    for coord in [
        TileCoord::new(1, 0),
        TileCoord::new(3, 0),
        TileCoord::new(5, 1),
    ] {
        let actual = render_tile_cpu(&tree, 0, coord);
        for y in (0..TILE).step_by(11) {
            for x in (0..TILE).step_by(13) {
                close(
                    actual[(y * TILE + x) as usize],
                    oracle(
                        &raster,
                        None,
                        h,
                        0,
                        coord.x as u32 * TILE + x,
                        coord.y as u32 * TILE + y,
                    ),
                    3e-6,
                );
            }
        }
    }
}

#[test]
fn huge_off_canvas_bounds_do_not_fetch_tiles_or_allocate_a_warp() {
    let h = Projective2::from_affine(DAffine2::from_cols_array(&[1e6, 0.0, 0.0, 1e6, 1e9, 1e9]))
        .unwrap();
    let map = ProjectivePixelMapping::new(128, 128, h).unwrap();
    assert!(map.bounds().w > 100_000_000);
    let raster = pattern(128, 128);
    let samples = ProjectiveTile::new(&map, 1.0, 0, 0);
    assert_eq!(samples.samples.len(), TILE_PX);
    assert!(samples.samples.iter().all(Option::is_none));
    let output = render_tile_cpu(
        &tree(
            vec![projected(raster.clone(), h)],
            BlendSpace::Linear,
            32,
            32,
        ),
        0,
        TileCoord::new(0, 0),
    );
    assert!(output.iter().all(|p| *p == [0.0; 4]));
    let mut cache = TileCache::new(raster.as_ref());
    for x in [-1, i64::MAX] {
        assert_eq!(cache.get(0, x, 0, [0; 4]), [0; 4]);
    }
    assert_eq!(
        cache.fetches, 0,
        "exterior support cannot fetch a source tile"
    );
}

#[test]
fn cache_capacity_and_lookup_budget_are_fixed_and_include_mip_identity() {
    let raster = pattern(1024, 768);
    let mut cache = TileCache::new(raster.as_ref());
    for index in 0..TILE_PX {
        let level = (index % 3) as u32;
        let (w, h) = raster.level_size(level);
        let grid = Grid {
            level,
            x: (index as u32 % w) as i64,
            y: (index as u32 % h) as i64,
            ax: 0.2,
            ay: 0.3,
        };
        let actual = cache.quad(grid, [0; 4]);
        let expected = raster
            .tile(
                level,
                TileCoord::new(
                    (grid.x / i64::from(TILE)) as i32,
                    (grid.y / i64::from(TILE)) as i32,
                ),
            )
            .unwrap();
        assert_eq!(
            actual[0],
            expected[(grid.y as usize % TILE as usize) * TILE as usize
                + grid.x as usize % TILE as usize]
        );
    }
    assert!(cache.fetches <= 4 * TILE_PX);
    assert_eq!(cache.entries.len(), CACHE_CAPACITY);
}

#[test]
fn checked_payload_rejects_dimensions_and_retains_source_arc_and_pixels() {
    assert!(matches!(
        ProjectivePixelMapping::new(0, 8, Projective2::IDENTITY),
        Err(ProjectivePixelError::SourceDimensions)
    ));
    assert!(ProjectivePixelMapping::new(30_001, 1, Projective2::IDENTITY).is_err());
    assert!(ProjectivePixelMapping::new(30_000, 30_000, Projective2::IDENTITY).is_err());
    let mismatch = LazyRaster::deferred((8, 8), Arc::new(|| Arc::new(Raster::empty(7, 8, [0; 4]))));
    assert!(matches!(
        ProjectivePixels::new(mismatch, Projective2::IDENTITY),
        Err(ProjectivePixelError::SourceMismatch)
    ));
    let raster = pattern(64, 48);
    let original_id = raster.content_id();
    let original_pixels = raster.read_rect(raster.bounds());
    let calls = Arc::new(AtomicUsize::new(0));
    let deferred = LazyRaster::deferred((64, 48), {
        let raster = raster.clone();
        let calls = calls.clone();
        Arc::new(move || {
            calls.fetch_add(1, Ordering::SeqCst);
            raster.clone()
        })
    });
    let h = Projective2::from_row_major([1.0, 0.1, 4.0, 0.0, 1.0, 6.0, 0.002, 0.0, 1.0]).unwrap();
    let payload = ProjectivePixels::new(deferred, h).unwrap();
    assert!(Arc::ptr_eq(payload.raster().get(), &raster));
    let tree = tree(
        vec![node(NodeContent::ProjectivePixels(payload.clone()))],
        BlendSpace::Linear,
        96,
        80,
    );
    for _ in 0..3 {
        let _ = flatten(&tree, 0);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(raster.content_id(), original_id);
    assert_eq!(raster.read_rect(raster.bounds()), original_pixels);
    assert!(Arc::ptr_eq(payload.raster().get(), &raster));
    let unsafe_halo =
        Projective2::from_row_major([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.02, 0.0, 1.0]).unwrap();
    assert!(
        unsafe_halo
            .map_rect(ProjectiveRect::new(DVec2::ZERO, DVec2::splat(128.0)).unwrap())
            .is_ok()
    );
    assert!(
        ProjectivePixelMapping::new(128, 128, unsafe_halo).is_err(),
        "unsupported halo is rejected at construction"
    );
}

#[test]
fn existing_affine_identity_and_integer_translation_remain_exact() {
    let raster = pattern(48, 31);
    for placement in [Placement::default(), Placement::at(7.0, 11.0)] {
        for space in [BlendSpace::Linear, BlendSpace::Srgb] {
            let tree = tree(
                vec![node(NodeContent::Pixels {
                    raster: raster.clone().into(),
                    placement,
                })],
                space,
                80,
                64,
            );
            let tile = render_tile_cpu(&tree, 0, TileCoord::new(0, 0));
            for y in 0..64 {
                for x in 0..80 {
                    let (sx, sy) = (x as i32 - placement.x as i32, y as i32 - placement.y as i32);
                    let expected = if sx >= 0 && sy >= 0 && sx < 48 && sy < 31 {
                        color::px_to_f(raster.get(sx as u32, sy as u32))
                    } else {
                        [0.0; 4]
                    };
                    assert_eq!(tile[(y * TILE + x) as usize], expected);
                }
            }
        }
    }
}

#[test]
fn projected_clipping_base_and_masked_knockout_apply_shape_once() {
    let h =
        Projective2::from_row_major([1.0, 0.3, 25.0, 0.2, 0.9, 18.0, 0.002, 0.001, 1.0]).unwrap();
    for space in [
        BlendSpace::Linear,
        BlendSpace::Srgb,
        BlendSpace::PhotoshopSrgbV1,
    ] {
        let source = Arc::new(Raster::solid(64, 48, [0.3, 0.1, 0.2, 0.5]));
        let mask = Arc::new(Mask::empty(64, 48, 128));
        let mut base = projected(source.clone(), h);
        base.mask = Some(mask.clone());
        let mut member = node(NodeContent::Fill([0.0, 1.0, 0.0, 1.0]));
        member.clip_to = Some(0);
        member.id = 2;
        let clip = render_tile_cpu(
            &tree(vec![base.clone(), member], space, 128, 128),
            0,
            TileCoord::new(0, 0),
        );
        for (index, p) in clip.iter().enumerate().take(128 * TILE as usize) {
            let (x, y) = (
                (index % TILE as usize) as u32,
                (index / TILE as usize) as u32,
            );
            if x < 128 {
                let alpha = oracle(&source, Some(&mask), h, 0, x, y)[3];
                close(*p, [0.0, alpha, 0.0, alpha], 3e-6);
            }
        }
        base.content =
            NodeContent::projective_pixels(Arc::new(Raster::empty(64, 48, [0; 4])).into(), h)
                .unwrap();
        base.blending.knockout = Knockout::Shallow;
        base.blending.transparency_shapes_layer = false;
        let backdrop = node(NodeContent::Fill([0.3, 0.2, 0.1, 1.0]));
        let output = render_tile_cpu(
            &tree(vec![backdrop, base], space, 128, 128),
            0,
            TileCoord::new(0, 0),
        );
        let opaque = Raster::solid(64, 48, [0.0, 0.0, 0.0, 1.0]);
        for y in 0..128 {
            for x in 0..128 {
                let shape = oracle(&opaque, Some(&mask), h, 0, x, y)[3];
                // Knockout alpha is independent of the blend color profile.
                assert!((output[(y * TILE + x) as usize][3] - (1.0 - shape)).abs() < 3e-6);
            }
        }
    }
}

#[test]
fn projected_styled_masks_and_bounds_knockout_keep_the_same_geometry() {
    let h =
        Projective2::from_row_major([0.9, -0.2, 29.0, 0.15, 0.85, 15.0, 0.002, 0.0, 1.0]).unwrap();
    let raster = Arc::new(Raster::solid(67, 51, [0.3, 0.2, 0.1, 0.7]));
    let mask = Arc::new(Mask::empty(67, 51, 128));
    for space in [
        BlendSpace::Linear,
        BlendSpace::Srgb,
        BlendSpace::PhotoshopSrgbV1,
    ] {
        let paint = projected(raster.clone(), h);
        let mut direct = paint.clone();
        direct.mask = Some(mask.clone());
        let mut envelope = projected(Arc::new(Raster::solid(67, 51, [0.0, 0.0, 0.0, 1.0])), h);
        envelope.mask = Some(mask.clone());
        let mut styled = node(NodeContent::StyledGroup {
            children: vec![paint],
            clip_source: Box::new(direct.clone()),
            effect_mask: Some(Box::new(envelope)),
        });
        styled.blending.layer_mask_hides_effects = true;
        let actual = render_tile_cpu(
            &tree(vec![styled], space, 128, 128),
            0,
            TileCoord::new(0, 0),
        );
        let opaque = Raster::solid(67, 51, [0.0, 0.0, 0.0, 1.0]);
        for y in 0..128 {
            for x in 0..128 {
                // The existing styled pipeline samples its effect envelope and
                // completed source separately, so the reconstructed edge appears
                // in both factors. Verify that declared pipeline, not an imagined
                // one-pass equivalence at antialiased exterior boundaries.
                let coverage = oracle(&opaque, Some(&mask), h, 0, x, y)[3];
                let expected = oracle(&raster, None, h, 0, x, y).map(|v| v * coverage);
                close(actual[(y * TILE + x) as usize], expected, 3e-6);
            }
        }
        direct.content =
            NodeContent::projective_pixels(Arc::new(Raster::empty(67, 51, [0; 4])).into(), h)
                .unwrap();
        let mut styled = node(NodeContent::StyledGroup {
            children: vec![direct.clone()],
            clip_source: Box::new(direct),
            effect_mask: None,
        });
        styled.blending.knockout = Knockout::Shallow;
        styled.blending.transparency_shapes_layer = false;
        let actual = render_tile_cpu(
            &tree(
                vec![node(NodeContent::Fill([0.2, 0.1, 0.0, 1.0])), styled],
                space,
                128,
                128,
            ),
            0,
            TileCoord::new(0, 0),
        );
        let opaque = Raster::solid(67, 51, [0.0, 0.0, 0.0, 1.0]);
        for y in 0..128 {
            for x in 0..128 {
                let expected = 1.0 - oracle(&opaque, Some(&mask), h, 0, x, y)[3];
                assert!((actual[(y * TILE + x) as usize][3] - expected).abs() < 3e-6);
            }
        }
    }
}

#[test]
fn opaque_bounds_shape_matches_odd_sized_source_mips_without_a_source_allocation() {
    let h = Projective2::from_row_major([0.7, 0.1, 8.0, 0.05, 0.6, 9.0, 0.003, 0.0, 1.0]).unwrap();
    let map = ProjectivePixelMapping::new(67, 51, h).unwrap();
    let solid = Raster::solid(67, 51, [0.0, 0.0, 0.0, 1.0]);
    for level in 0..=5 {
        let tile = ProjectiveTile::new(&map, (1_u32 << level) as f64, 0, 0);
        let shape = tile.shape(&solid);
        let mut paint = crate::tile::ftile();
        tile.raster(&solid, &mut paint);
        for (shape, paint) in shape.into_iter().zip(paint) {
            assert!((shape - paint[3]).abs() < 2e-7);
        }
    }
}

#[test]
fn homogeneous_rescaling_preserves_checked_sampling_and_differentials() {
    let coefficients = [0.8, 0.15, 23.0, -0.1, 0.9, 31.0, 0.002, 0.001, 1.0];
    let raster = pattern(80, 56);
    let original = Projective2::from_row_major(coefficients).unwrap();
    let expected = render_tile_cpu(
        &tree(
            vec![projected(raster.clone(), original)],
            BlendSpace::Linear,
            128,
            128,
        ),
        0,
        TileCoord::new(0, 0),
    );
    for scale in [-1e100, 1e-100, 16.0] {
        let h = Projective2::from_row_major(coefficients.map(|v| v * scale)).unwrap();
        let actual = render_tile_cpu(
            &tree(
                vec![projected(raster.clone(), h)],
                BlendSpace::Linear,
                128,
                128,
            ),
            0,
            TileCoord::new(0, 0),
        );
        for (a, e) in actual.into_iter().zip(&expected) {
            close(a, *e, 3e-6);
        }
    }
}

#[test]
fn unsupported_differential_precision_is_rejected_before_sampling() {
    let tiny = Projective2::from_affine(DAffine2::from_scale(DVec2::splat(1e-120))).unwrap();
    // Valid, invertible general geometry is not automatically a supported pixel
    // sampling domain. Its inverse footprint exceeds the renderer's admitted
    // coefficient/differential range and must not become mip-zero output.
    assert!(
        tiny.map_rect(ProjectiveRect::new(DVec2::ZERO, DVec2::splat(16.0)).unwrap())
            .is_ok()
    );
    assert!(ProjectivePixelMapping::new(16, 16, tiny).is_err());
}

#[test]
fn visible_large_offset_anisotropic_minification_matches_analytic_inverse() {
    // Every input coefficient is dyadic and their construction is exact. The
    // closed-form inverse uses local offsets directly, independently of the
    // production rebasing, inversion, matrix residuals, and point sampler.
    let (offset, y_offset, sx, sy, g) = (536_870_912.0, 64.0, 1.0 / 32.0, 0.5, 1.0 / 16_384.0);
    let h = Projective2::from_row_major([
        sx + offset * g,
        0.0,
        offset,
        y_offset * g,
        sy,
        y_offset,
        g,
        0.0,
        1.0,
    ])
    .unwrap();
    let raster = pattern(512, 256);
    let scene = tree(
        vec![projected(raster.clone(), h)],
        BlendSpace::Linear,
        offset as u32 + 32,
        256,
    );
    let actual = crate::composite::try_render_tile_cpu(
        &scene,
        0,
        TileCoord::new(offset as i32 / TILE as i32, 0),
    )
    .unwrap();
    let mut visible = 0;
    for y in 0..256 {
        for x in 0..32 {
            let (dx, dy) = (x as f64 + 0.5, y as f64 + 0.5 - y_offset);
            let den = sx - g * dx;
            let point = DVec2::new(dx / den, dy * sx / (sy * den));
            let jacobian = [
                sx / (den * den),
                0.0,
                dy * sx * g / (sy * den * den),
                sx / (sy * den),
            ];
            let trace = jacobian.iter().map(|v| v * v).sum::<f64>();
            let determinant = jacobian[0] * jacobian[3];
            let sigma =
                ((trace + (trace * trace - 4.0 * determinant * determinant).sqrt()) * 0.5).sqrt();
            let (pixels, ax, ay) =
                reference_sample(&raster, point, sigma.log2().floor().max(0.0), [0; 4]);
            let pixels = pixels.map(color::px_to_f);
            let expected = std::array::from_fn(|c| lerp(pixels.map(|p| p[c]), ax, ay));
            visible += usize::from(expected[3] > 0.1);
            close(actual[(y * TILE + x) as usize], expected, 4e-6);
        }
    }
    assert!(
        visible > 100,
        "large-offset fixture must actually draw retained pixels"
    );
}

#[test]
fn projective_output_tile_seams_match_oracle_at_reduced_output_levels() {
    let raster = pattern(1536, 512);
    let h = Projective2::from_row_major([2.0, 0.1, 300.0, 0.05, 1.2, 25.0, 0.00003, 0.00001, 1.0])
        .unwrap();
    let scene = tree(
        vec![projected(raster.clone(), h)],
        BlendSpace::Linear,
        4096,
        1024,
    );
    for level in [1, 2, 3] {
        let left = render_tile_cpu(&scene, level, TileCoord::new(0, 0));
        let right = render_tile_cpu(&scene, level, TileCoord::new(1, 0));
        let mut visible = false;
        for y in 8..64 {
            for x in [254, 255, 256, 257] {
                let actual = if x < TILE {
                    left[(y * TILE + x) as usize]
                } else {
                    right[(y * TILE + x - TILE) as usize]
                };
                let expected = oracle(&raster, None, h, level, x, y);
                visible |= expected[3] > 0.1;
                close(actual, expected, 3e-6);
            }
        }
        assert!(
            visible,
            "level {level} must cover the real output tile seam"
        );
    }
}

#[test]
fn external_mask_resource_admission_covers_hidden_and_derived_sources() {
    let source = projected(pattern(16, 16), Projective2::IDENTITY);
    let mut bad = node(NodeContent::Fill([0.0; 4]));
    bad.mask = Some(Arc::new(Mask::empty(1 << 30, 1 << 30, 255)));
    bad.visible = false;
    let forms = [
        bad.clone(),
        node(NodeContent::Group(vec![bad.clone()])),
        node(NodeContent::ClippedGroup {
            children: vec![],
            baseline: vec![bad.clone()],
        }),
        node(NodeContent::StyledGroup {
            children: vec![bad.clone()],
            clip_source: Box::new(source.clone()),
            effect_mask: None,
        }),
        node(NodeContent::StyledGroup {
            children: vec![],
            clip_source: Box::new(bad.clone()),
            effect_mask: None,
        }),
        node(NodeContent::StyledGroup {
            children: vec![],
            clip_source: Box::new(source.clone()),
            effect_mask: Some(Box::new(bad)),
        }),
    ];
    for form in forms {
        // Use raw descriptors deliberately: the fallible entry must reject
        // before any Plane mip request, without allocating by nominal area.
        let scene = CompositeTree {
            width: 32,
            height: 32,
            space: BlendSpace::Linear,
            knockout_background: None,
            nodes: vec![source.clone(), form],
        };
        assert_eq!(
            scene.validate_projective_resources(),
            Err(ProjectivePixelError::MaskDimensions)
        );
        assert!(matches!(
            crate::composite::try_render_tile_cpu(&scene, 0, TileCoord::new(0, 0)),
            Err(ProjectivePixelError::MaskDimensions)
        ));
        assert!(matches!(
            crate::composite::try_flatten(&scene, 0),
            Err(ProjectivePixelError::MaskDimensions)
        ));
    }
    let mut direct = source;
    direct.mask = Some(Arc::new(Mask::empty(25_000, 25_000, 0)));
    let scene = CompositeTree {
        width: 32,
        height: 32,
        space: BlendSpace::Linear,
        knockout_background: None,
        nodes: vec![direct],
    };
    assert_eq!(
        scene.validate_projective_resources(),
        Err(ProjectivePixelError::MaskDimensions)
    );
}

#[test]
fn projected_rectangle_clips_and_styled_clipping_bases_preserve_combined_output() {
    let h = Projective2::from_row_major([1.2, 0.2, 25.0, 0.1, 1.1, 19.0, 0.002, 0.0, 1.0]).unwrap();
    let raster = Arc::new(Raster::solid(64, 48, [0.3, 0.1, 0.2, 0.6]));
    let mask = Arc::new(Mask::empty(64, 48, 173));
    for space in [
        BlendSpace::Linear,
        BlendSpace::Srgb,
        BlendSpace::PhotoshopSrgbV1,
    ] {
        let mut source = projected(raster.clone(), h);
        source.mask = Some(mask.clone());
        let mut base = node(NodeContent::StyledGroup {
            children: vec![source.clone()],
            clip_source: Box::new(source),
            effect_mask: None,
        });
        let rect = [35.25, 24.5, 42.5, 36.25];
        base.clip_rect = Some(rect);
        let mut member = node(NodeContent::Fill([0.0, 1.0, 0.0, 1.0]));
        member.id = 2;
        member.clip_to = Some(0);
        let actual = render_tile_cpu(
            &tree(vec![base, member], space, 128, 128),
            0,
            TileCoord::new(0, 0),
        );
        for y in 0..128 {
            for x in 0..128 {
                let coverage_x = ((x as f64 + 1.0).min(rect[0] + rect[2])
                    - (x as f64).max(rect[0]))
                .clamp(0.0, 1.0);
                let coverage_y = ((y as f64 + 1.0).min(rect[1] + rect[3])
                    - (y as f64).max(rect[1]))
                .clamp(0.0, 1.0);
                let rectangle = (coverage_x * coverage_y) as f32;
                let before = oracle(&raster, Some(&mask), h, 0, x, y).map(|v| v * rectangle);
                let alpha = before[3];
                let expected = crate::blend::blend_px(
                    BlendMode::Normal,
                    space,
                    before,
                    [0.0, alpha, 0.0, alpha],
                    0.0,
                );
                close(actual[(y * TILE + x) as usize], expected, 4e-6);
            }
        }
    }
}

#[test]
fn differential_admission_checks_inverse_construction_and_jacobian_residual() {
    let wrong_inverse =
        Projective2::from_affine(DAffine2::from_translation(DVec2::new(2e-6, 0.0))).unwrap();
    let support = ProjectiveRect::new(DVec2::ZERO, DVec2::splat(16.0)).unwrap();
    assert!(
        wrong_inverse
            .checked_differential(Projective2::IDENTITY, DVec2::ZERO, support)
            .is_err(),
        "stable evaluation alone must not approve the wrong inverse"
    );
    let wrong_derivative =
        Projective2::from_affine(DAffine2::from_scale(DVec2::splat(1.1))).unwrap();
    let small_support = ProjectiveRect::new(DVec2::ZERO, DVec2::splat(1e-6)).unwrap();
    assert!(
        wrong_derivative
            .checked_differential(Projective2::IDENTITY, DVec2::ZERO, small_support)
            .is_err(),
        "a small absolute point residual cannot approve a wrong local footprint"
    );
}

#[test]
fn resource_admission_walk_is_iterative_and_preserves_legacy_only_contract() {
    let mut legacy = node(NodeContent::Fill([0.0; 4]));
    legacy.mask = Some(Arc::new(Mask::empty(1 << 30, 1 << 30, 255)));
    for _ in 0..512 {
        legacy = node(NodeContent::Group(vec![legacy]));
    }
    let mut scene = CompositeTree {
        width: 32,
        height: 32,
        space: BlendSpace::Linear,
        knockout_background: None,
        nodes: vec![legacy],
    };
    assert!(
        scene.validate_projective_resources().is_ok(),
        "legacy-only producer validation is unchanged"
    );
    scene
        .nodes
        .push(projected(pattern(16, 16), Projective2::IDENTITY));
    assert_eq!(
        scene.validate_projective_resources(),
        Err(ProjectivePixelError::MaskDimensions),
        "deep masks are checked, not skipped by a traversal depth cap"
    );
}
