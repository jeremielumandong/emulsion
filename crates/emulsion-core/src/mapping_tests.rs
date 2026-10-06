use super::{Mapping2, SmartPlacement, mask_to_document, preserve_mask_world, source_rect};
use emulsion_raster::projective::{Projective2, ProjectiveError, ProjectiveRect};
use emulsion_raster::{IRect, Placement};
use glam::{DAffine2, DVec2, dvec2};

fn bits(affine: DAffine2) -> [u64; 6] {
    affine.to_cols_array().map(f64::to_bits)
}

fn close(actual: DVec2, expected: DVec2) {
    assert!(
        (actual - expected).abs().max_element() < 1e-8,
        "{actual:?} != {expected:?}"
    );
}

fn perspective() -> Projective2 {
    Projective2::from_row_major([1.0, 0.25, 3.0, -0.125, 1.5, 2.0, 0.001, -0.002, 1.0]).unwrap()
}

fn legacy() -> Placement {
    Placement {
        x: 37.25,
        y: -12.5,
        scale_x: 1.375,
        scale_y: 0.875,
        rotation: 33.0,
        flip_x: true,
        flip_y: false,
    }
}

#[test]
fn affine_representation_and_identity_keep_all_coefficient_bits() {
    let columns = [-2.0, -0.0, 0.3, 1.2, 13.1, -5.7];
    let original = columns.map(f64::to_bits);
    let mapping = Mapping2::from_affine_columns(columns).unwrap();
    assert_eq!(bits(mapping.try_affine().unwrap()), original);
    for identity in [
        Mapping2::IDENTITY,
        Mapping2::Projective(Projective2::IDENTITY),
    ] {
        assert_eq!(
            bits(identity.compose(mapping).unwrap().try_affine().unwrap()),
            original
        );
        assert_eq!(
            bits(mapping.compose(identity).unwrap().try_affine().unwrap()),
            original
        );
    }
    assert_eq!(
        bits(
            mapping
                .with_source_offset((0, 0))
                .unwrap()
                .try_affine()
                .unwrap()
        ),
        original
    );
    assert_eq!(
        bits(mapping.in_cache((0, 0)).unwrap().try_affine().unwrap()),
        original
    );
    assert_eq!(Mapping2::default(), Mapping2::IDENTITY);
}

#[test]
fn legacy_retention_does_not_apply_projective_precision_admission() {
    // Finite columns, finite determinant and finite affine inverse, but their
    // dynamic range cannot survive projective canonicalization. The old raster
    // mask document check accepts this representation; retaining it must not
    // make old files unreadable. This is not sampling-domain approval.
    let affine = DAffine2::from_scale(dvec2(1e300, 1e-100));
    assert!(affine.matrix2.determinant().is_finite());
    assert!(affine.inverse().is_finite());
    let mapping = Mapping2::from_affine(affine).unwrap();
    assert_eq!(bits(mapping.try_affine().unwrap()), bits(affine));
    assert_eq!(mapping.validate_representation(), Ok(()));
    assert_eq!(Mapping2::IDENTITY.compose(mapping).unwrap(), mapping);
    assert_eq!(
        mapping.validate_for_operation(),
        Err(ProjectiveError::PrecisionLoss)
    );
    assert_eq!(mapping.to_projective(), Err(ProjectiveError::PrecisionLoss));
    assert!(mapping.map_point(DVec2::ONE).is_err());
    assert!(
        mapping
            .compose(Mapping2::Affine(DAffine2::from_scale(DVec2::splat(2.0))))
            .is_err()
    );
}

#[test]
fn representation_validation_does_not_replace_native_compatibility_validation() {
    let singular = Mapping2::from_affine_columns([0.0; 6]).unwrap();
    assert_eq!(singular.validate_representation(), Ok(()));
    assert_eq!(
        singular.validate_for_operation(),
        Err(ProjectiveError::Singular)
    );
    assert_eq!(singular.inverse(), Err(ProjectiveError::Singular));
    assert_eq!(
        singular.map_point(DVec2::ZERO),
        Err(ProjectiveError::Singular)
    );
    assert!(singular.bounds(source_rect((2, 2)).unwrap()).is_err());
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut columns = DAffine2::IDENTITY.to_cols_array();
        columns[4] = bad;
        assert_eq!(
            Mapping2::from_affine_columns(columns),
            Err(ProjectiveError::NonFinite)
        );
        let raw = Mapping2::Affine(DAffine2::from_cols_array(&columns));
        assert_eq!(raw.try_affine(), Err(ProjectiveError::NonFinite));
        assert_eq!(
            Mapping2::IDENTITY.compose(raw),
            Err(ProjectiveError::NonFinite)
        );
    }
}

#[test]
fn affine_operations_use_the_original_glam_arithmetic() {
    let left = DAffine2::from_cols_array(&[-1.2, 0.3, 0.1, 0.7, 13.7, -9.1]);
    let right = DAffine2::from_cols_array(&[2.3, 0.4, -0.5, 1.1, -3.2, 4.9]);
    let mapping = Mapping2::from_affine(left).unwrap();
    let result = mapping
        .compose(Mapping2::from_affine(right).unwrap())
        .unwrap();
    assert!(matches!(result, Mapping2::Affine(_)));
    assert_eq!(bits(result.try_affine().unwrap()), bits(left * right));
    assert_eq!(
        bits(mapping.inverse().unwrap().try_affine().unwrap()),
        bits(left.inverse())
    );
    let point = dvec2(5.3, -7.2);
    assert_eq!(
        mapping.map_point(point).unwrap(),
        left.transform_point2(point)
    );
}

#[test]
fn mixed_composition_inverse_and_affine_extraction_are_explicit() {
    let affine = Mapping2::from_affine(DAffine2::from_translation(dvec2(7.0, 11.0))).unwrap();
    let projective = Mapping2::Projective(perspective());
    for (left, right) in [(affine, projective), (projective, affine)] {
        let combined = left.compose(right).unwrap();
        assert!(matches!(combined, Mapping2::Projective(_)));
        let point = dvec2(21.0, 9.0);
        let mapped = combined.map_point(point).unwrap();
        close(
            mapped,
            left.map_point(right.map_point(point).unwrap()).unwrap(),
        );
        close(
            combined.inverse().unwrap().map_point(mapped).unwrap(),
            point,
        );
        assert_eq!(combined.try_affine(), Err(ProjectiveError::NotAffine));
    }
    let near_affine =
        Projective2::from_row_major([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1e-15, 0.0, 1.0]).unwrap();
    assert_eq!(
        Mapping2::Projective(near_affine).try_affine(),
        Err(ProjectiveError::NotAffine)
    );
    let exactly_affine = Projective2::from_affine(affine.try_affine().unwrap()).unwrap();
    let retained = Mapping2::Projective(exactly_affine);
    assert_eq!(retained.try_affine().unwrap(), affine.try_affine().unwrap());
    assert!(matches!(retained, Mapping2::Projective(_)));
}

#[test]
fn dual_identity_composition_retains_the_right_hand_baseline() {
    let projected_identity = Mapping2::Projective(Projective2::IDENTITY);
    assert_eq!(
        Mapping2::IDENTITY.compose(projected_identity).unwrap(),
        projected_identity
    );
    assert_eq!(
        projected_identity.compose(Mapping2::IDENTITY).unwrap(),
        Mapping2::IDENTITY
    );
    assert_eq!(
        projected_identity.with_source_offset((0, 0)).unwrap(),
        projected_identity
    );
    assert_eq!(projected_identity.inverse().unwrap(), projected_identity);
}

#[test]
fn legacy_conversion_depends_on_source_size_and_preserves_placement_arithmetic() {
    let placement = legacy();
    let smart = SmartPlacement::Legacy(placement);
    for size in [(90, 60), (120, 85)] {
        let mapping = smart.source_to_document(size).unwrap();
        assert!(matches!(mapping, Mapping2::Affine(_)));
        assert_eq!(
            bits(mapping.try_affine().unwrap()),
            bits(placement.to_doc(size.0, size.1))
        );
        assert_eq!(
            bits(smart.try_affine(size).unwrap()),
            bits(placement.to_doc(size.0, size.1))
        );
    }
    assert_ne!(
        smart.source_to_document((90, 60)).unwrap(),
        smart.source_to_document((120, 85)).unwrap()
    );
    assert_eq!(
        SmartPlacement::default(),
        SmartPlacement::Legacy(Placement::default())
    );
}

#[test]
fn projective_conversion_is_independent_of_source_and_cache_size() {
    let smart = SmartPlacement::Projective(perspective());
    assert_eq!(
        smart.source_to_document((90, 60)).unwrap(),
        smart.source_to_document((120, 85)).unwrap()
    );
    assert_eq!(
        smart.source_to_document((90, 60)).unwrap(),
        Mapping2::Projective(perspective())
    );
    assert_eq!(smart.try_affine((90, 60)), Err(ProjectiveError::NotAffine));
    for invalid in [(0, 10), (10, 0), (0, 0)] {
        assert_eq!(
            smart.source_to_document(invalid),
            Err(ProjectiveError::InvalidRectangle)
        );
        assert_eq!(
            SmartPlacement::Legacy(legacy()).source_to_document(invalid),
            Err(ProjectiveError::InvalidRectangle)
        );
    }
}

#[test]
fn identity_retains_smart_representation_before_any_new_domain_admission() {
    for original in [
        SmartPlacement::Legacy(legacy()),
        SmartPlacement::Projective(perspective()),
        SmartPlacement::Projective(Projective2::IDENTITY),
    ] {
        assert_eq!(
            original
                .left_compose_projective(Projective2::IDENTITY, (90, 60))
                .unwrap(),
            original
        );
    }
    // Valid legacy fields can overflow when an operation evaluates to_doc.
    // A no-op is retention, not a request to convert or validate a new domain.
    let extreme = SmartPlacement::Legacy(Placement {
        scale_x: 1e308,
        ..Placement::default()
    });
    assert!(extreme.source_to_document((90, 60)).is_err());
    assert_eq!(
        extreme
            .left_compose_projective(Projective2::IDENTITY, (90, 60))
            .unwrap(),
        extreme
    );
}

#[test]
fn meaningful_projective_edits_left_compose_and_do_not_discard_tiny_deltas() {
    let size = (90, 60);
    let original = SmartPlacement::Legacy(legacy());
    let first = original
        .left_compose_projective(perspective(), size)
        .unwrap();
    let delta = Projective2::from_affine(DAffine2::from_translation(dvec2(-8.0, 3.0))).unwrap();
    let second = first.left_compose_projective(delta, size).unwrap();
    for source in source_rect(size).unwrap().corners() {
        let baseline = original
            .source_to_document(size)
            .unwrap()
            .map_point(source)
            .unwrap();
        close(
            second
                .source_to_document(size)
                .unwrap()
                .map_point(source)
                .unwrap(),
            delta
                .map_point(perspective().map_point(baseline).unwrap())
                .unwrap(),
        );
    }
    let tiny = Projective2::from_affine(DAffine2::from_translation(dvec2(1e-14, 0.0))).unwrap();
    let result = SmartPlacement::default()
        .left_compose_projective(tiny, size)
        .unwrap();
    assert_eq!(result, SmartPlacement::Projective(tiny));
    assert_ne!(result, SmartPlacement::default());
}

#[test]
fn cache_offset_composition_has_the_declared_sign_and_order() {
    let size = (90, 60);
    let offset = (-12, -9);
    let translation = DAffine2::from_translation(dvec2(-12.0, -9.0));
    let original = SmartPlacement::Legacy(legacy());
    let cache = original.cache_to_document(size, offset).unwrap();
    assert_eq!(
        bits(cache.try_affine().unwrap()),
        bits(legacy().to_doc(size.0, size.1) * translation)
    );
    let smart = SmartPlacement::Projective(perspective());
    let cache = smart.cache_to_document(size, offset).unwrap();
    for point in [DVec2::ZERO, dvec2(31.0, 22.0)] {
        close(
            cache.map_point(point).unwrap(),
            perspective().map_point(point + dvec2(-12.0, -9.0)).unwrap(),
        );
    }
}

#[test]
fn mask_cache_mapping_keeps_existing_affine_order_and_handles_minimum_offset() {
    let columns = [1.25, 0.3, -0.2, 0.75, 7.1, -4.2];
    let component = Mapping2::from_affine_columns(columns).unwrap();
    for offset in [(0, 0), (-12, -9), (i32::MIN, i32::MAX)] {
        let expected = crate::composite_mask_cache::mask_to_output(columns, offset);
        assert_eq!(
            bits(component.in_cache(offset).unwrap().try_affine().unwrap()),
            bits(expected)
        );
    }
    let offset = (i32::MIN, i32::MAX);
    assert_eq!(
        Mapping2::IDENTITY
            .with_source_offset(offset)
            .unwrap()
            .map_point(DVec2::ZERO)
            .unwrap(),
        dvec2(f64::from(i32::MIN), f64::from(i32::MAX))
    );
}

#[test]
fn cache_basis_cancels_without_entering_the_mask_world_map() {
    let source = Mapping2::Projective(perspective());
    let component = Mapping2::from_affine_columns([1.25, 0.3, -0.2, 0.75, 7.1, -4.2]).unwrap();
    let world = mask_to_document(source, component).unwrap();
    let through_cache = source
        .with_source_offset((-12, -9))
        .unwrap()
        .compose(component.in_cache((-12, -9)).unwrap())
        .unwrap();
    for point in [DVec2::ZERO, dvec2(31.0, 22.0)] {
        close(
            through_cache.map_point(point).unwrap(),
            world.map_point(point).unwrap(),
        );
    }
}

#[test]
fn affine_unlinked_compensation_keeps_existing_world_first_grouping() {
    let old = legacy().to_doc(90, 60);
    let new = Placement {
        x: 61.0,
        rotation: 48.0,
        ..legacy()
    }
    .to_doc(90, 60);
    let component = DAffine2::from_cols_array(&[1.1, 0.2, -0.3, 0.7, 3.9, -1.2]);
    let result = preserve_mask_world(
        Mapping2::Affine(old),
        Mapping2::Affine(new),
        Mapping2::Affine(component),
    )
    .unwrap();
    assert_eq!(
        bits(result.try_affine().unwrap()),
        bits(new.inverse() * (old * component))
    );
    let unchanged = preserve_mask_world(
        Mapping2::Affine(old),
        Mapping2::Affine(old),
        Mapping2::Affine(component),
    )
    .unwrap();
    assert_eq!(bits(unchanged.try_affine().unwrap()), bits(component));
}

#[test]
fn unlinked_compensation_preserves_world_geometry_through_repeated_content_moves() {
    let old = Mapping2::Affine(legacy().to_doc(90, 60));
    let new = Mapping2::Projective(perspective()).compose(old).unwrap();
    let component = Mapping2::from_affine_columns([1.1, 0.2, -0.3, 0.7, 3.9, -1.2]).unwrap();
    let compensated = preserve_mask_world(old, new, component).unwrap();
    let moved_again = Mapping2::Affine(DAffine2::from_translation(dvec2(-8.0, 3.0)))
        .compose(new)
        .unwrap();
    let compensated_again = preserve_mask_world(new, moved_again, compensated).unwrap();
    for point in [DVec2::ZERO, dvec2(7.0, 11.0), dvec2(40.0, 20.0)] {
        let expected = mask_to_document(old, component)
            .unwrap()
            .map_point(point)
            .unwrap();
        close(
            mask_to_document(new, compensated)
                .unwrap()
                .map_point(point)
                .unwrap(),
            expected,
        );
        close(
            mask_to_document(moved_again, compensated_again)
                .unwrap()
                .map_point(point)
                .unwrap(),
            expected,
        );
    }
}

#[test]
fn relative_mask_horizon_does_not_invalidate_its_finite_world_map() {
    let source = Mapping2::Projective(
        Projective2::from_row_major([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.125, 0.0, 1.0]).unwrap(),
    );
    let relative = preserve_mask_world(Mapping2::IDENTITY, source, Mapping2::IDENTITY).unwrap();
    assert_eq!(relative.validate_for_operation(), Ok(()));
    let intrinsic = source_rect((16, 8)).unwrap();
    assert!(relative.map_rect(intrinsic).is_err());
    let world = mask_to_document(source, relative).unwrap();
    assert_eq!(
        world.map_rect(intrinsic).unwrap().corners(),
        intrinsic.corners()
    );
    assert!(relative.map_point(dvec2(8.0, 2.0)).is_err());
}

#[test]
fn source_admission_does_not_approve_a_filter_expanded_cache() {
    let smart = SmartPlacement::Projective(
        Projective2::from_row_major([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.125, 0.0, 1.0]).unwrap(),
    );
    let source = source_rect((4, 4)).unwrap();
    assert!(
        smart
            .source_to_document((4, 4))
            .unwrap()
            .map_rect(source)
            .is_ok()
    );
    let expanded = smart.cache_to_document((4, 4), (-12, 0)).unwrap();
    assert!(expanded.map_rect(source_rect((16, 4)).unwrap()).is_err());
}

#[test]
fn affine_bounds_keep_original_corners_and_conservatively_enclose_the_domain() {
    let affine = legacy().to_doc(90, 60);
    let mapping = Mapping2::Affine(affine);
    let source = source_rect((90, 60)).unwrap();
    let mapped = mapping.map_rect(source).unwrap();
    assert_eq!(
        mapped.corners(),
        source.corners().map(|point| affine.transform_point2(point))
    );
    for y in 0..=20 {
        for x in 0..=20 {
            let point = affine.transform_point2(dvec2(f64::from(x) * 4.5, f64::from(y) * 3.0));
            assert!(point.cmpge(mapped.bounds().min()).all());
            assert!(point.cmple(mapped.bounds().max()).all());
        }
    }
    assert!(mapped.bounds().to_irect().is_ok());
    assert_eq!(
        Mapping2::IDENTITY
            .bounds(source)
            .unwrap()
            .to_irect()
            .unwrap(),
        IRect::new(0, 0, 90, 60)
    );
}

#[test]
fn affine_corner_bounds_do_not_certify_every_interior_point() {
    let affine = DAffine2::from_cols_array(&[1_048_576.0, 0.0, 0.0, 1.0, -2_097_152.0, 0.0]);
    let mapping = Mapping2::from_affine(affine).unwrap();
    let source = source_rect((4, 4)).unwrap();
    let mapped = mapping.map_rect(source).unwrap();
    assert_eq!(
        mapped.corners(),
        source.corners().map(|point| affine.transform_point2(point))
    );
    assert_eq!(mapping.bounds(source).unwrap(), mapped.bounds());
    assert!(mapped.bounds().to_irect().is_ok());
    // The endpoints pass their result-relative budgets. At this interior zero
    // crossing, the same arithmetic has an error allowance above the unchanged
    // 1e-9 absolute budget. A bounds result must not hide that pointwise refusal.
    assert_eq!(
        mapping.map_point(dvec2(2.0, 2.0)),
        Err(ProjectiveError::PrecisionLoss)
    );
}

#[test]
fn projective_rectangle_checks_are_reused() {
    let h = perspective();
    let source = ProjectiveRect::new(dvec2(-3.0, -2.0), dvec2(90.0, 60.0)).unwrap();
    let expected = h.map_rect(source).unwrap();
    let actual = Mapping2::Projective(h).map_rect(source).unwrap();
    assert_eq!(actual.corners(), expected.corners());
    assert_eq!(actual.bounds(), expected.bounds());
}

#[test]
fn finite_forward_support_does_not_require_a_finite_inverse_aabb() {
    let h =
        Projective2::from_row_major([1.3, 10.0, 300.0, 0.2, 1.0, 200.0, 0.001, 0.0, 1.0]).unwrap();
    let mapping = Mapping2::Projective(h);
    let source = source_rect((256, 128)).unwrap();
    let mapped = mapping.map_rect(source).unwrap();
    let enclosing = ProjectiveRect::new(mapped.bounds().min(), mapped.bounds().max()).unwrap();
    assert!(mapping.inverse().unwrap().map_rect(enclosing).is_err());
    for point in source.corners() {
        close(
            mapping
                .inverse()
                .unwrap()
                .map_point(mapping.map_point(point).unwrap())
                .unwrap(),
            point,
        );
    }
}

#[test]
fn integer_bounds_are_a_separate_gate_from_finite_geometry() {
    let mapping = Mapping2::from_affine(DAffine2::from_translation(dvec2(1e12, -1e12))).unwrap();
    let bounds = mapping.bounds(source_rect((4, 4)).unwrap()).unwrap();
    assert!(bounds.min().is_finite());
    assert_eq!(bounds.to_irect(), Err(ProjectiveError::BoundsOverflow));
}

#[test]
fn extreme_nonfinite_and_degenerate_operations_fail_without_a_fallback() {
    let huge = Mapping2::from_affine(DAffine2::from_scale(DVec2::splat(1e200))).unwrap();
    assert!(huge.inverse().is_err()); // Legacy determinant overflows.
    assert!(huge.compose(huge).is_err());
    assert!(huge.map_point(DVec2::splat(1e200)).is_err());
    assert_eq!(
        Mapping2::IDENTITY.map_point(dvec2(f64::NAN, 0.0)),
        Err(ProjectiveError::NonFinite)
    );
    let cancellation =
        Mapping2::from_affine(DAffine2::from_translation(dvec2(-1e12, 0.0))).unwrap();
    assert_eq!(
        cancellation.map_point(dvec2(1e12, 0.0)),
        Err(ProjectiveError::PrecisionLoss)
    );
    let tiny = Mapping2::from_affine(DAffine2::from_scale(DVec2::splat(1e-200))).unwrap();
    assert_eq!(tiny.inverse(), Err(ProjectiveError::PrecisionLoss));
    assert_eq!(
        tiny.map_point(DVec2::splat(1e-200)),
        Err(ProjectiveError::PrecisionLoss)
    );
    let subnormal_determinant =
        Mapping2::from_affine(DAffine2::from_scale(DVec2::splat(1e-160))).unwrap();
    assert_eq!(
        subnormal_determinant.inverse(),
        Err(ProjectiveError::PrecisionLoss)
    );
}
