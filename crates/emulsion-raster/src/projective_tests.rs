use super::{Projective2, ProjectiveError, ProjectiveRect};
use crate::geom::IRect;
use glam::{DAffine2, DVec2, dvec2};

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> ProjectiveRect {
    ProjectiveRect::new(dvec2(x0, y0), dvec2(x1, y1)).unwrap()
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "actual {actual:?}, expected {expected:?}, tolerance {tolerance:?}"
    );
}

fn point_close(actual: DVec2, expected: DVec2, tolerance: f64) {
    close(actual.x, expected.x, tolerance);
    close(actual.y, expected.y, tolerance);
}

// Independent reference on the supplied, uncanonicalized coefficients.
fn reference_point(h: [f64; 9], p: DVec2) -> DVec2 {
    let w = h[6] * p.x + h[7] * p.y + h[8];
    dvec2(
        (h[0] * p.x + h[1] * p.y + h[2]) / w,
        (h[3] * p.x + h[4] * p.y + h[5]) / w,
    )
}

fn assert_enclosed(h: Projective2, source: ProjectiveRect, reference: [f64; 9]) {
    let mapped = h.map_rect(source).unwrap();
    let bounds = mapped.bounds();
    let corners = mapped.corners();
    let negative_winding = (corners[1] - corners[0])
        .perp_dot(corners[2] - corners[1])
        .is_sign_negative();
    for y in 0..=40 {
        for x in 0..=40 {
            let p = source.min() + source.size() * dvec2(f64::from(x) / 40.0, f64::from(y) / 40.0);
            let expected = reference_point(reference, p);
            let actual = h.map_point(p).unwrap();
            point_close(actual, expected, 1e-10);
            for sample in [expected, actual] {
                assert!(sample.x >= bounds.min().x && sample.x <= bounds.max().x);
                assert!(sample.y >= bounds.min().y && sample.y <= bounds.max().y);
                // Every interior sample must also remain inside the mapped
                // convex quad, not merely inside its axis-aligned bounds.
                for i in 0..4 {
                    let edge = corners[(i + 1) % 4] - corners[i];
                    let cross = edge.perp_dot(sample - corners[i]);
                    if negative_winding {
                        assert!(cross <= 1e-8);
                    } else {
                        assert!(cross >= -1e-8);
                    }
                }
            }
        }
    }
}

#[test]
fn identity_preserves_points_bounds_and_composition_exactly() {
    let identity = Projective2::IDENTITY;
    let source = rect(-3.0, 5.0, 100.0, 40.0);
    assert_eq!(
        identity
            .map_rect(source)
            .unwrap()
            .bounds()
            .to_irect()
            .unwrap(),
        IRect::new(-3, 5, 103, 35)
    );
    assert_eq!(
        identity.map_rect(source).unwrap().corners(),
        source.corners()
    );
    assert_eq!(identity.inverse().unwrap(), identity);
    assert_eq!(Projective2::default(), identity);
    let h =
        Projective2::from_row_major([2.0, 0.5, 13.0, -1.0, 4.0, 9.0, 0.001, -0.002, 1.0]).unwrap();
    assert_eq!(identity.compose(h).unwrap(), h);
    assert_eq!(h.compose(identity).unwrap(), h);
    assert_eq!(
        identity.map_point(dvec2(1e150, -1e150)).unwrap(),
        dvec2(1e150, -1e150)
    );
}

#[test]
fn affine_conversion_including_reflection_keeps_the_legacy_transform_untouched() {
    let affine = DAffine2::from_cols_array(&[-2.0, 0.5, 1.25, 3.0, 1024.0, -2048.0]);
    let original = affine.to_cols_array();
    let h = Projective2::from_affine(affine).unwrap();
    assert_eq!(affine.to_cols_array(), original);
    assert_eq!(h.to_affine().unwrap().to_cols_array(), original);
    for p in [DVec2::ZERO, dvec2(7.0, -13.0), dvec2(10_000.0, 1.0)] {
        let expected = dvec2(
            -2.0 * p.x + 1.25 * p.y + 1024.0,
            0.5 * p.x + 3.0 * p.y - 2048.0,
        );
        point_close(h.map_point(p).unwrap(), expected, 1e-10);
        point_close(h.inverse().unwrap().map_point(expected).unwrap(), p, 1e-10);
    }
}

#[test]
fn perspective_inverse_and_composition_follow_column_vector_order() {
    let a = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.02, -0.03, 1.0];
    let b = [2.0, -0.5, 5.0, 0.75, 3.0, -7.0, 0.0, 0.0, 1.0];
    let a_map = Projective2::from_row_major(a).unwrap();
    let b_map = Projective2::from_row_major(b).unwrap();
    let inverse_reference = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, -0.02, 0.03, 1.0];
    for p in [dvec2(1.0, 2.0), dvec2(-3.0, 1.0), dvec2(4.0, -2.0)] {
        point_close(
            a_map.compose(b_map).unwrap().map_point(p).unwrap(),
            reference_point(a, reference_point(b, p)),
            1e-11,
        );
        point_close(
            b_map.compose(a_map).unwrap().map_point(p).unwrap(),
            reference_point(b, reference_point(a, p)),
            1e-11,
        );
        point_close(
            a_map.inverse().unwrap().map_point(p).unwrap(),
            reference_point(inverse_reference, p),
            1e-12,
        );
        point_close(
            a_map
                .inverse()
                .unwrap()
                .map_point(a_map.map_point(p).unwrap())
                .unwrap(),
            p,
            1e-12,
        );
    }
    assert_eq!(a_map.to_affine(), Err(ProjectiveError::NotAffine));
    let almost_affine =
        Projective2::from_row_major([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1e-20, 0.0, 1.0]).unwrap();
    assert_eq!(almost_affine.to_affine(), Err(ProjectiveError::NotAffine));
}

#[test]
fn canonicalization_handles_sign_scale_and_signed_zero() {
    let coefficients = [2.0, 0.0, 4.0, 0.0, 1.0, -2.0, 0.25, 0.0, 1.0];
    let h = Projective2::from_row_major(coefficients).unwrap();
    for scale in [2.0_f64.powi(-400), -2.0_f64.powi(400), 1e100, -1e-100] {
        let scaled = Projective2::from_row_major(coefficients.map(|v| v * scale)).unwrap();
        for (actual, expected) in scaled.to_row_major().into_iter().zip(h.to_row_major()) {
            close(actual, expected, 1e-15);
            if actual == 0.0 {
                assert!(!actual.is_sign_negative());
            }
        }
        point_close(
            scaled.map_point(dvec2(2.0, 3.0)).unwrap(),
            reference_point(coefficients, dvec2(2.0, 3.0)),
            1e-12,
        );
    }
    let tiny_identity = Projective2::IDENTITY
        .to_row_major()
        .map(|v| v * f64::from_bits(16));
    assert_eq!(
        Projective2::from_row_major(tiny_identity).unwrap(),
        Projective2::IDENTITY
    );
}

#[test]
fn a_zero_bottom_right_coefficient_can_have_positive_and_negative_safe_domains() {
    // (x,y) -> (1/x,y/x); its inverse is the same projective map.
    let h = Projective2::from_row_major([0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0]).unwrap();
    assert_eq!(h.to_row_major()[8], 0.0);
    assert_eq!(h.inverse().unwrap(), h);
    assert_eq!(h.map_point(DVec2::ZERO), Err(ProjectiveError::Horizon));
    for source in [rect(2.0, -1.0, 4.0, 2.0), rect(-4.0, -1.0, -2.0, 2.0)] {
        assert_enclosed(h, source, h.to_row_major());
    }
    assert_eq!(
        h.map_rect(rect(-1.0, -1.0, 1.0, 1.0)),
        Err(ProjectiveError::Horizon)
    );
}

#[test]
fn large_translations_do_not_trigger_a_raw_determinant_cutoff() {
    let h = Projective2::from_affine(DAffine2::from_translation(dvec2(1e12, -2e12))).unwrap();
    let p = dvec2(128.0, 256.0);
    let mapped = h.map_point(p).unwrap();
    point_close(mapped, dvec2(1e12 + 128.0, -2e12 + 256.0), 0.001);
    point_close(h.inverse().unwrap().map_point(mapped).unwrap(), p, 0.002);
    let bounds = h.map_rect(rect(0.0, 0.0, 1000.0, 500.0)).unwrap().bounds();
    assert!(bounds.min().x <= 1e12 && bounds.max().x >= 1e12 + 1000.0);
    assert_eq!(bounds.to_irect(), Err(ProjectiveError::BoundsOverflow));
}

#[test]
fn power_of_two_input_scaling_preserves_a_small_translated_numerator() {
    let translation = 2.0_f64.powi(60);
    let h = Projective2::from_affine(DAffine2::from_translation(dvec2(translation, 0.0))).unwrap();
    // Dividing the input by abs(x) rounds its homogeneous constant; that
    // former evaluation returned approximately 256 instead of 128.
    point_close(
        h.map_point(dvec2(-translation + 128.0, 0.0)).unwrap(),
        dvec2(128.0, 0.0),
        1e-12,
    );
    let source = rect(-translation, -2.0, -translation + 512.0, 2.0);
    let mapped = h.map_rect(source).unwrap();
    for (actual, expected) in mapped
        .corners()
        .into_iter()
        .zip(rect(0.0, -2.0, 512.0, 2.0).corners())
    {
        point_close(actual, expected, 1e-12);
    }
}

#[test]
fn zero_and_near_zero_translated_results_remain_supported() {
    let translation = 2.0_f64.powi(20);
    let h = Projective2::from_affine(DAffine2::from_translation(dvec2(translation, 0.0))).unwrap();
    for offset in [0.0, 2.0_f64.powi(-32), -2.0_f64.powi(-32)] {
        point_close(
            h.map_point(dvec2(-translation + offset, 0.0)).unwrap(),
            dvec2(offset, 0.0),
            1e-25,
        );
    }
}

#[test]
fn ordinary_large_offset_cancellation_matches_the_stored_coefficient_map() {
    let translation = 1e12;
    let h = Projective2::from_affine(DAffine2::from_translation(dvec2(translation, 0.0))).unwrap();
    let coefficients = h.to_row_major();
    for offset in [-128.0, 0.0, 128.0] {
        let x = -translation + offset;
        // Independent single-FMA reference, without homogeneous input
        // normalization. Canonicalization itself can round affine coefficients.
        let expected = coefficients[0].mul_add(x, coefficients[2]) / coefficients[8];
        let actual = h.map_point(dvec2(x, 0.0)).unwrap();
        close(actual.x, expected, 1e-9);
        close(actual.x, offset, 0.001);
        assert_eq!(actual.y, 0.0);
    }
}

#[test]
fn compensated_evaluation_recovers_product_roundoff_before_cancellation() {
    let h = Projective2::from_row_major([0.1, 0.0, -0.11, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]).unwrap();
    let expected = 0.1_f64.mul_add(1.1, -0.11);
    close(h.map_point(dvec2(1.1, 0.0)).unwrap().x, expected, 1e-30);
}

#[test]
fn numerator_uncertainty_can_refuse_a_point_without_a_denominator_problem() {
    let h = Projective2::from_row_major([0.1, 0.1, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1e-100]).unwrap();
    assert!(h.inverse().is_ok());
    // The x numerator is exactly zero, but nonzero product corrections have
    // opposite signs. Their conservative uncertainty, magnified by 1e100,
    // cannot establish the near-zero Cartesian accuracy budget.
    assert_eq!(
        h.map_point(dvec2(-1.1, 1.1)),
        Err(ProjectiveError::PrecisionLoss)
    );
    assert_eq!(
        h.map_rect(rect(-1.1, 1.1, 1.2, 2.0)),
        Err(ProjectiveError::PrecisionLoss)
    );
    let ordinary =
        Projective2::from_row_major([0.1, 0.1, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]).unwrap();
    point_close(
        ordinary.map_point(dvec2(-1.1, 1.1)).unwrap(),
        dvec2(0.0, 1.1),
        1e-15,
    );
}

#[test]
fn tiny_and_anisotropic_scales_are_not_confused_with_singular_maps() {
    for (x, y) in [(1e-200, 1e-200), (1e-150, 1e150), (-1e100, 1e-100)] {
        let h = Projective2::from_affine(DAffine2::from_scale(dvec2(x, y))).unwrap();
        let actual = h.map_point(dvec2(1.0, 1.0)).unwrap();
        close(actual.x / x, 1.0, 1e-14);
        close(actual.y / y, 1.0, 1e-14);
        point_close(
            h.inverse().unwrap().map_point(actual).unwrap(),
            dvec2(1.0, 1.0),
            1e-13,
        );
        let identity = h.compose(h.inverse().unwrap()).unwrap();
        point_close(
            identity.map_point(dvec2(7.0, -3.0)).unwrap(),
            dvec2(7.0, -3.0),
            1e-12,
        );
    }
}

#[test]
fn map_validity_does_not_approve_horizon_touch_cross_or_filter_expansion() {
    let h = Projective2::from_row_major([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0 / 128.0, 0.0, 1.0])
        .unwrap();
    assert!(h.map_rect(rect(0.0, 0.0, 90.0, 40.0)).is_ok());
    assert_eq!(
        h.map_rect(rect(0.0, 0.0, 128.0, 40.0)),
        Err(ProjectiveError::Horizon)
    );
    assert_eq!(
        h.map_rect(rect(0.0, 0.0, 140.0, 40.0)),
        Err(ProjectiveError::Horizon)
    );
    assert!(h.map_rect(rect(140.0, 0.0, 256.0, 40.0)).is_ok());
    assert!(h.inverse().is_ok());
}

#[test]
fn denominator_cancellation_and_domain_conditioning_are_checked_separately() {
    let reciprocal =
        Projective2::from_row_major([0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0]).unwrap();
    assert!(reciprocal.map_point(dvec2(1e-14, 0.0)).is_ok());
    assert_eq!(
        reciprocal.map_rect(rect(1e-14, 0.0, 1.0, 1.0)),
        Err(ProjectiveError::UnsafeDomain)
    );
    let cancellation =
        Projective2::from_row_major([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 0.0, 1.0]).unwrap();
    assert_eq!(
        cancellation.map_point(dvec2(1.0 - 1e-15, 0.0)),
        Err(ProjectiveError::UnsafeDomain)
    );
}

#[test]
fn dense_interior_samples_stay_inside_corner_bounds_and_the_quad() {
    for coefficients in [
        [2.0, 0.5, 7.0, -0.25, 3.0, 5.0, 0.002, -0.001, 1.0],
        [-1.5, 0.25, 20.0, 0.5, 2.0, -7.0, 0.001, 0.002, 1.0],
        [1.0, 0.0, 3.0, 0.0, 1.0, 4.0, 0.0, 0.0, -1.0],
    ] {
        assert_enclosed(
            Projective2::from_row_major(coefficients).unwrap(),
            rect(-20.0, -10.0, 80.0, 60.0),
            coefficients,
        );
    }
}

#[test]
fn sampled_derivatives_agree_with_the_independent_projective_jacobian() {
    let raw = [2.0, 0.5, 7.0, -0.25, 3.0, 5.0, 0.002, -0.001, 1.0];
    let h = Projective2::from_row_major(raw).unwrap();
    // Analytic det(J) = det(H)/w^3 also establishes constant orientation
    // across the checked no-horizon domain.
    let det = raw[0] * (raw[4] * raw[8] - raw[5] * raw[7])
        - raw[1] * (raw[3] * raw[8] - raw[5] * raw[6])
        + raw[2] * (raw[3] * raw[7] - raw[4] * raw[6]);
    for y in 1..10 {
        for x in 1..10 {
            let p = dvec2(f64::from(x) * 5.0, f64::from(y) * 3.0);
            let mapped = reference_point(raw, p);
            let w = raw[6] * p.x + raw[7] * p.y + raw[8];
            let dx = dvec2(raw[0] - mapped.x * raw[6], raw[3] - mapped.y * raw[6]) / w;
            let dy = dvec2(raw[1] - mapped.x * raw[7], raw[4] - mapped.y * raw[7]) / w;
            let step = 1e-3;
            let numerical_x = (h.map_point(p + dvec2(step, 0.0)).unwrap()
                - h.map_point(p - dvec2(step, 0.0)).unwrap())
                / (2.0 * step);
            let numerical_y = (h.map_point(p + dvec2(0.0, step)).unwrap()
                - h.map_point(p - dvec2(0.0, step)).unwrap())
                / (2.0 * step);
            point_close(numerical_x, dx, 1e-8);
            point_close(numerical_y, dy, 1e-8);
            close(dx.perp_dot(dy), det / w.powi(3), 1e-12);
            assert!(dx.perp_dot(dy) > 0.0);
        }
    }
}

#[test]
fn ordered_quad_construction_accepts_both_windings_and_nonzero_source_origins() {
    let source = rect(-50.0, 20.0, 150.0, 120.0);
    let quad = [
        dvec2(5.0, 10.0),
        dvec2(180.0, -15.0),
        dvec2(150.0, 170.0),
        dvec2(-30.0, 120.0),
    ];
    for quad in [quad, [quad[0], quad[3], quad[2], quad[1]]] {
        let h = Projective2::rect_to_quad(source, quad).unwrap();
        for (p, expected) in source.corners().into_iter().zip(quad) {
            point_close(h.map_point(p).unwrap(), expected, 1e-9);
            point_close(h.inverse().unwrap().map_point(expected).unwrap(), p, 1e-9);
        }
        assert_enclosed(h, source, h.to_row_major());
    }
}

#[test]
fn quad_solver_reproduces_independently_specified_perspective_interior() {
    let source = rect(-10.0, -20.0, 200.0, 80.0);
    let raw = [1.5, 0.25, 120.0, -0.4, 2.0, -50.0, 0.001, 0.002, 1.0];
    let quad = source.corners().map(|p| reference_point(raw, p));
    assert_enclosed(
        Projective2::rect_to_quad(source, quad).unwrap(),
        source,
        raw,
    );
}

#[test]
fn quad_solver_preserves_anisotropy_and_handles_large_offsets() {
    let source = rect(0.0, 0.0, 100.0, 100.0);
    let skinny = [
        dvec2(0.0, 0.0),
        dvec2(1e-8, 0.0),
        dvec2(1e-8, 1e8),
        dvec2(0.0, 1e8),
    ];
    let h = Projective2::rect_to_quad(source, skinny).unwrap();
    let middle = h.map_point(dvec2(50.0, 50.0)).unwrap();
    close(middle.x / 5e-9, 1.0, 1e-13);
    close(middle.y / 5e7, 1.0, 1e-13);
    let source = rect(1e12, -1e12, 1e12 + 10_000.0, -1e12 + 20_000.0);
    let quad = source.corners().map(|p| p + dvec2(50_000.0, -100_000.0));
    let h = Projective2::rect_to_quad(source, quad).unwrap();
    for (p, expected) in source.corners().into_iter().zip(quad) {
        point_close(h.map_point(p).unwrap(), expected, 0.01);
    }
}

#[test]
fn quad_solver_rejects_concave_crossed_repeated_and_collinear_vertices() {
    let source = rect(0.0, 0.0, 10.0, 10.0);
    for quad in [
        [
            dvec2(0.0, 0.0),
            dvec2(10.0, 0.0),
            dvec2(3.0, 3.0),
            dvec2(0.0, 10.0),
        ],
        [
            dvec2(0.0, 0.0),
            dvec2(10.0, 10.0),
            dvec2(10.0, 0.0),
            dvec2(0.0, 10.0),
        ],
        [
            dvec2(0.0, 0.0),
            dvec2(10.0, 0.0),
            dvec2(10.0, 0.0),
            dvec2(0.0, 10.0),
        ],
        [
            dvec2(0.0, 0.0),
            dvec2(5.0, 0.0),
            dvec2(10.0, 0.0),
            dvec2(0.0, 10.0),
        ],
        [
            dvec2(0.0, 0.0),
            dvec2(1.0, 1.0),
            dvec2(2.0, 2.0),
            dvec2(3.0, 3.0),
        ],
    ] {
        assert_eq!(
            Projective2::rect_to_quad(source, quad),
            Err(ProjectiveError::InvalidQuad)
        );
    }
}

#[test]
fn precision_limited_quad_construction_refuses_instead_of_relaxing_tolerance() {
    let source = rect(0.0, 0.0, 1.0, 1.0);
    let quad = rect(1e15, 1e15, 1e15 + 1.0, 1e15 + 1.0).corners();
    assert!(Projective2::rect_to_quad(source, quad).is_err());
    // Nearly collinear after independent axis normalization.
    let quad = [
        dvec2(0.0, 0.0),
        dvec2(1.0, 1.0),
        dvec2(2.0, 2.0 + 1e-15),
        dvec2(1.0, 1.0 + 1e-15),
    ];
    assert!(Projective2::rect_to_quad(source, quad).is_err());
}

#[test]
fn nonfinite_singular_and_unrepresentable_matrices_fail_closed() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut raw = Projective2::IDENTITY.to_row_major();
        raw[0] = value;
        assert_eq!(
            Projective2::from_row_major(raw),
            Err(ProjectiveError::NonFinite)
        );
        assert_eq!(
            Projective2::IDENTITY.map_point(dvec2(value, 0.0)),
            Err(ProjectiveError::NonFinite)
        );
        let mut quad = rect(0.0, 0.0, 1.0, 1.0).corners();
        quad[0].x = value;
        assert_eq!(
            Projective2::rect_to_quad(rect(0.0, 0.0, 1.0, 1.0), quad),
            Err(ProjectiveError::NonFinite)
        );
    }
    for raw in [[0.0; 9], [1.0, 2.0, 3.0, 1.0, 2.0, 3.0, 0.0, 0.0, 1.0]] {
        assert_eq!(
            Projective2::from_row_major(raw),
            Err(ProjectiveError::Singular)
        );
    }
    assert_eq!(
        Projective2::from_row_major([1e-200, 0.0, 0.0, 0.0, 1e200, 0.0, 0.0, 0.0, 1.0]),
        Err(ProjectiveError::PrecisionLoss)
    );
    let nearly_singular = [1.0, 1.0, 0.0, 1.0, 1.0 + f64::EPSILON, 0.0, 0.0, 0.0, 1.0];
    assert_eq!(
        Projective2::from_row_major(nearly_singular),
        Err(ProjectiveError::PrecisionLoss)
    );
}

#[test]
fn composition_and_evaluation_overflow_or_underflow_return_errors() {
    let tiny = Projective2::from_affine(DAffine2::from_scale(DVec2::splat(1e-200))).unwrap();
    assert_eq!(tiny.compose(tiny), Err(ProjectiveError::PrecisionLoss));
    let huge = Projective2::from_affine(DAffine2::from_scale(DVec2::splat(1e200))).unwrap();
    assert!(huge.map_point(dvec2(1e200, 1.0)).is_err());
    assert!(huge.map_rect(rect(1e200, 0.0, 2e200, 1.0)).is_err());
    let ordinary = Projective2::from_affine(DAffine2::from_translation(dvec2(1.0, 0.0))).unwrap();
    assert!(ordinary.map_point(dvec2(f64::from_bits(1), 1e300)).is_err());
}

#[test]
fn rectangles_and_integer_bounds_validate_before_narrowing() {
    for (min, max) in [
        (DVec2::ZERO, DVec2::ZERO),
        (dvec2(2.0, 0.0), dvec2(1.0, 1.0)),
        (DVec2::ZERO, dvec2(f64::INFINITY, 1.0)),
        (dvec2(-f64::MAX, 0.0), dvec2(f64::MAX, 1.0)),
    ] {
        assert_eq!(
            ProjectiveRect::new(min, max),
            Err(ProjectiveError::InvalidRectangle)
        );
    }
    let identity = Projective2::IDENTITY;
    for source in [
        rect(-3e9, 0.0, -3e9 + 1.0, 1.0),
        rect(0.0, 0.0, 3e9, 1.0),
        rect(-2e9, 0.0, 2e9, 1.0),
        rect(
            f64::from(i32::MAX) - 0.25,
            0.0,
            f64::from(i32::MAX) + 0.25,
            1.0,
        ),
    ] {
        assert_eq!(
            identity.map_rect(source).unwrap().bounds().to_irect(),
            Err(ProjectiveError::BoundsOverflow)
        );
    }
    let source = rect(f64::from(i32::MIN), -1.0, f64::from(i32::MIN) + 2.0, 2.0);
    let bounds = identity
        .map_rect(source)
        .unwrap()
        .bounds()
        .to_irect()
        .unwrap();
    assert_eq!(bounds, IRect::new(i32::MIN, -1, 2, 3));
    assert_eq!(bounds.right(), i32::MIN + 2);
    let source = rect(f64::from(i32::MAX) - 2.0, 0.0, f64::from(i32::MAX), 1.0);
    assert_eq!(
        identity
            .map_rect(source)
            .unwrap()
            .bounds()
            .to_irect()
            .unwrap()
            .right(),
        i32::MAX
    );
}

#[test]
fn integer_representability_does_not_implicitly_approve_canvas_or_allocation_budgets() {
    let source = rect(0.0, 0.0, 50_000.0, 50_000.0);
    let bounds = Projective2::IDENTITY
        .map_rect(source)
        .unwrap()
        .bounds()
        .to_irect()
        .unwrap();
    assert_eq!(bounds, IRect::new(0, 0, 50_000, 50_000));
    assert!(u64::try_from(bounds.w).unwrap() * u64::try_from(bounds.h).unwrap() > 400_000_000);
}
