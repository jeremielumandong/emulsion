use super::*;
use emulsion_raster::projective::{Projective2, ProjectiveRect};
use glam::{DAffine2, DVec2};

fn metadata<'a>() -> SmartSupportMetadata<'a> {
    SmartSupportMetadata {
        source_size: (4, 4),
        placement: SmartPlacement::Projective(Projective2::IDENTITY),
        filters: &[],
        styles: &[],
        filters_enabled: true,
        retained_cache: None,
        raster_mask: ComponentMaskMetadata::default(),
        filter_mask: None,
        has_vector_mask: false,
    }
}

fn mask(size: (u32, u32)) -> ComponentMaskMetadata {
    ComponentMaskMetadata {
        plane: Some(MaskPlaneMetadata {
            size,
            has_detail: true,
        }),
        transform: Mapping2::Projective(Projective2::IDENTITY),
        ..ComponentMaskMetadata::default()
    }
}

fn support(metadata: SmartSupportMetadata<'_>) -> SmartSupport {
    match preflight_stack_support(metadata).unwrap() {
        SmartSupportPreflight::Projective(support) => support,
        SmartSupportPreflight::LegacyCompatibility => panic!("expected projective metadata"),
    }
}

fn perspective(g: f64) -> Projective2 {
    Projective2::from_row_major([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, g, 0.0, 1.0]).unwrap()
}

#[test]
fn pure_legacy_returns_to_existing_admission_before_new_limits_or_checks() {
    let mut input = metadata();
    input.placement = SmartPlacement::Legacy(Placement::default());
    input.source_size = (0, u32::MAX);
    input.raster_mask.properties.feather = f32::NAN;
    input.has_vector_mask = true;
    assert!(matches!(
        preflight_stack_support(input),
        Ok(SmartSupportPreflight::LegacyCompatibility)
    ));
    // This is a compatibility route, not a claim these legacy inputs are valid.
}

#[test]
fn feature_predicates_retain_identity_disabled_dormant_and_latent_variants() {
    let mut input = metadata();
    assert_eq!(
        input.features(),
        ProjectiveFeatures {
            placement: true,
            raster_mask: false,
            filter_mask: false
        }
    );
    input.placement = SmartPlacement::Legacy(Placement::default());
    input.filters_enabled = false;
    input.raster_mask.transform = Mapping2::Projective(Projective2::IDENTITY);
    input.raster_mask.enabled = false;
    let mut filter = mask((2, 2));
    filter.enabled = false;
    input.filter_mask = Some(filter);
    let plan = support(input);
    assert_eq!(
        plan.features(),
        ProjectiveFeatures {
            placement: false,
            raster_mask: true,
            filter_mask: true
        }
    );
    assert!(plan.source_projective().is_none());
    assert!(plan.raster_mask().processing().is_none());
    assert!(plan.raster_mask().projective_plan().is_none());
    assert!(plan.filter_mask().unwrap().projective_plan().is_some());
    assert_eq!(
        plan.output_grid(),
        SmartOutputGrid {
            size: (4, 4),
            offset: (0, 0)
        }
    );
}

#[test]
fn legacy_mixed_content_uses_exact_cache_placement_without_projective_admission() {
    let filters = [Filter::BoxBlur { radius: 2.0 }];
    let placement = Placement {
        x: 3.0,
        y: -7.0,
        rotation: 31.0,
        scale_x: 1e20,
        flip_y: true,
        ..Placement::default()
    };
    let mut input = metadata();
    input.placement = SmartPlacement::Legacy(placement);
    input.filters = &filters;
    input.raster_mask = mask((4, 4));
    let plan = support(input);
    let expected = crate::smart::cache_placement(&placement, (4, 4), (8, 8), (-2, -2));
    assert_eq!(
        plan.effective_pixels(),
        EffectivePixelSupport::Legacy(expected)
    );
    assert!(plan.source_projective().is_none());
    let promoted = Projective2::from_affine(placement.to_doc(4, 4))
        .map(|projective| ProjectivePixelMapping::new(4, 4, projective));
    assert!(!matches!(promoted, Ok(Ok(_))));
}

#[test]
fn forward_mask_horizon_is_not_a_sampling_or_latent_mapping_refusal() {
    let h = perspective(0.125);
    let c = h.inverse().unwrap();
    assert!(
        c.map_rect(ProjectiveRect::new(DVec2::ZERO, DVec2::new(16.0, 4.0)).unwrap())
            .is_err()
    );
    let mut input = metadata();
    input.placement = SmartPlacement::Projective(h);
    input.raster_mask = mask((16, 4));
    input.raster_mask.transform = Mapping2::Projective(c);
    let plan = support(input);
    assert_eq!(plan.raster_mask().projective_plan().unwrap().authored(), c);
    input.raster_mask.plane = None;
    let latent = support(input);
    assert!(latent.raster_mask().processing().is_none());
    assert!(latent.raster_mask().projective_plan().is_none());
    assert_ne!(plan.key(), latent.key());
}

#[test]
fn pixel_admission_includes_source_mip_halo_even_in_bypass() {
    let mut input = metadata();
    let h = perspective(0.5);
    assert!(
        h.map_rect(ProjectiveRect::new(DVec2::ZERO, DVec2::splat(4.0)).unwrap())
            .is_ok()
    );
    input.placement = SmartPlacement::Projective(h);
    input.filters_enabled = false;
    assert!(matches!(
        preflight_stack_support(input),
        Err(SmartSupportError::Pixels {
            domain: PixelDomain::Source,
            ..
        })
    ));
}

#[test]
fn prospective_filter_expansion_certifies_its_own_mip_halo_before_rendering() {
    let filters = [Filter::BoxBlur { radius: 4.0 }];
    let disabled = [FilterStyle {
        enabled: false,
        ..FilterStyle::default()
    }];
    let zero_opacity = [FilterStyle {
        opacity: 0.0,
        ..FilterStyle::default()
    }];
    let mut input = metadata();
    input.placement = SmartPlacement::Projective(perspective(0.125));
    input.filters = &filters;
    input.styles = &disabled;
    assert_eq!(support(input).output_grid().size, (4, 4));
    input.styles = &zero_opacity;
    assert!(matches!(
        preflight_stack_support(input),
        Err(SmartSupportError::Pixels {
            domain: PixelDomain::EffectiveGrid,
            ..
        })
    ));
    input.styles = &[]; // missing styles enable their stage
    assert!(matches!(
        preflight_stack_support(input),
        Err(SmartSupportError::Pixels {
            domain: PixelDomain::EffectiveGrid,
            ..
        })
    ));
    input.filters_enabled = false;
    assert_eq!(support(input).output_grid().offset, (0, 0));
}

#[test]
fn prospective_old_cache_and_actual_publication_are_separate() {
    let filters = [Filter::BoxBlur { radius: 2.0 }];
    let old = SmartOutputGrid {
        size: (4, 4),
        offset: (0, 0),
    };
    let expected = SmartOutputGrid {
        size: (8, 8),
        offset: (-2, -2),
    };
    let mut input = metadata();
    input.filters = &filters;
    input.retained_cache = Some(old);
    let plan = support(input);
    assert_eq!(plan.output_grid(), expected);
    assert!(matches!(
        validate_actual_cache(plan.clone(), old),
        Err(SmartSupportError::ActualCacheMismatch { .. })
    ));
    assert!(matches!(
        validate_actual_cache(
            plan.clone(),
            SmartOutputGrid {
                size: expected.size,
                offset: (0, 0)
            }
        ),
        Err(SmartSupportError::ActualCacheMismatch { .. })
    ));
    let published = validate_actual_cache(plan, expected).unwrap();
    input.retained_cache = Some(expected);
    assert_eq!(published.actual_cache(), expected);
    assert!(published.support().matches_metadata(input));
}

#[test]
fn mixed_states_also_require_actual_active_cache_equality() {
    let filters = [Filter::Invert];
    let mut input = metadata();
    input.placement = SmartPlacement::Legacy(Placement::default());
    input.raster_mask.transform = Mapping2::Projective(Projective2::IDENTITY);
    input.filters = &filters;
    let plan = support(input);
    assert!(plan.footprint().active);
    assert!(matches!(
        validate_actual_cache(
            plan,
            SmartOutputGrid {
                size: (4, 4),
                offset: (1, 0)
            }
        ),
        Err(SmartSupportError::ActualCacheMismatch { .. })
    ));
}

#[test]
fn inactive_retained_cache_does_not_enlarge_pixel_or_mask_evaluation_domain() {
    let filters = [Filter::BoxBlur { radius: 250.0 }];
    let retained = SmartOutputGrid {
        size: (504, 504),
        offset: (i32::MIN, i32::MAX),
    };
    let mut input = metadata();
    input.placement = SmartPlacement::Projective(perspective(0.125));
    input.filters = &filters;
    input.filters_enabled = false;
    input.retained_cache = Some(retained);
    input.raster_mask = mask((4, 4));
    let plan = support(input);
    assert_eq!(
        plan.output_grid(),
        SmartOutputGrid {
            size: (4, 4),
            offset: (0, 0)
        }
    );
    assert!(plan.filter_buffers().is_none());
    let grid = plan.raster_mask().projective_plan().unwrap().key().output;
    assert_eq!(
        grid,
        MaskOutputGrid {
            size: (4, 4),
            offset: (0, 0)
        }
    );
    let published = validate_actual_cache(plan, retained).unwrap();
    assert_eq!(published.actual_cache(), retained);
    assert_eq!(
        published.support().source_projective().unwrap().size(),
        (4, 4)
    );
}

#[test]
fn independent_resource_limits_cover_inactive_cache_and_dormant_masks() {
    let mut input = metadata();
    input.retained_cache = Some(SmartOutputGrid {
        size: (30_001, 1),
        offset: (0, 0),
    });
    assert!(matches!(
        preflight_stack_support(input),
        Err(SmartSupportError::Resource {
            resource: SupportResource::RetainedCache,
            ..
        })
    ));
    input.retained_cache = None;
    input.raster_mask = mask((20_001, 20_000));
    input.raster_mask.enabled = false;
    assert!(matches!(
        preflight_stack_support(input),
        Err(SmartSupportError::Resource {
            resource: SupportResource::RawMask(MaskComponent::Raster),
            ..
        })
    ));
    input.raster_mask = ComponentMaskMetadata::default();
    input.filter_mask = Some(mask((0, 4)));
    input.filters_enabled = false;
    assert!(matches!(
        preflight_stack_support(input),
        Err(SmartSupportError::Resource {
            resource: SupportResource::RawMask(MaskComponent::Filter),
            ..
        })
    ));
    input.filter_mask = None;
    assert!(matches!(
        validate_actual_cache(
            support(input),
            SmartOutputGrid {
                size: (1, 30_001),
                offset: (0, 0)
            }
        ),
        Err(SmartSupportError::Resource {
            resource: SupportResource::ActualCache,
            ..
        })
    ));
}

#[test]
fn source_and_prospective_filter_resource_limits_precede_pixel_support() {
    let mut input = metadata();
    input.source_size = (30_001, 1);
    assert!(matches!(
        preflight_stack_support(input),
        Err(SmartSupportError::Resource {
            resource: SupportResource::Source,
            ..
        })
    ));
    input.source_size = (20_001, 20_000);
    assert!(matches!(
        preflight_stack_support(input),
        Err(SmartSupportError::Resource {
            resource: SupportResource::Source,
            ..
        })
    ));
    let filters = [Filter::BoxBlur { radius: 1.0 }];
    input.source_size = (30_000, 1);
    input.filters = &filters;
    assert!(matches!(
        preflight_stack_support(input),
        Err(SmartSupportError::Resource {
            resource: SupportResource::EffectiveGrid,
            size: (30_002, 3),
        })
    ));
}

#[test]
fn processed_mask_expansion_is_admitted_separately_from_raw_size() {
    let mut input = metadata();
    input.raster_mask = mask((30_000, 1));
    input.raster_mask.properties.feather = 0.5;
    input.raster_mask.enabled = false;
    assert!(matches!(
        preflight_stack_support(input),
        Err(SmartSupportError::Resource {
            resource: SupportResource::ProcessedMask(MaskComponent::Raster),
            size: (30_006, 7)
        })
    ));
    input.raster_mask.properties.density = 0.0;
    let zero = support(input);
    assert_eq!(zero.raster_mask().processing().unwrap().halo, 0);
    assert!(zero.raster_mask().projective_plan().is_some());
    input.raster_mask.properties.density = 1.0;
    input.raster_mask.plane.as_mut().unwrap().has_detail = false;
    let constant = support(input);
    assert_eq!(
        constant.raster_mask().processing().unwrap().size,
        (30_000, 1)
    );
    assert!(constant.raster_mask().projective_plan().is_some());
}

#[test]
fn shared_processing_layout_matches_literal_legacy_halos_and_shortcuts() {
    for (radius, expected) in [
        (0.0, 0),
        (0.49, 0),
        (0.5, 3),
        (1.7, 3),
        (3.4, 6),
        (1000.0, 1764),
    ] {
        let properties = MaskProperties {
            density: 0.75,
            feather: radius,
        };
        let layout = properties
            .checked_processing_layout((10, 20), true)
            .unwrap();
        assert_eq!(layout.halo, expected);
        assert_eq!(layout.size, (10 + 2 * expected, 20 + 2 * expected));
        assert_eq!(layout.origin, (-(expected as i32), -(expected as i32)));
        assert_eq!(
            layout.dense_coverage_bytes,
            Some((10 + 2 * expected) as usize * (20 + 2 * expected) as usize)
        );
        assert_eq!(
            properties
                .checked_processing_layout((10, 20), false)
                .unwrap()
                .halo,
            0
        );
        let zero = MaskProperties {
            density: 0.0,
            ..properties
        }
        .checked_processing_layout((10, 20), true)
        .unwrap();
        assert_eq!(zero.halo, 0);
        assert!(zero.feather_work.is_none());
        assert!(zero.dense_coverage_bytes.is_none());
    }
    let layout = MaskProperties {
        density: 0.5,
        feather: 1.7,
    }
    .checked_processing_layout((10, 20), true)
    .unwrap();
    assert_eq!(
        layout.feather_work.unwrap(),
        crate::mask_properties::MaskFeatherWork {
            strip_f32_bytes: 16 * 32 * 4,
            row_f32_bytes: 22 * 4,
            column_f32_bytes: 32 * 4,
            output_strip_bytes: 16 * 26,
        }
    );
    assert!(
        MaskProperties::default()
            .checked_processing_layout((10, 20), true)
            .unwrap()
            .feather_work
            .is_none()
    );
    assert!(matches!(
        MaskProperties {
            feather: f32::NAN,
            ..MaskProperties::default()
        }
        .checked_processing_layout((10, 20), true),
        Err(MaskProcessingError::Properties)
    ));
    assert!(matches!(
        MaskProperties {
            feather: 1.7,
            ..MaskProperties::default()
        }
        .checked_processing_layout((u32::MAX, 1), true),
        Err(MaskProcessingError::Layout)
    ));
}

#[test]
fn legacy_affine_derivation_retains_literal_three_pixel_feather_support_bytes() {
    use crate::composite_mask_cache::{MaskGrid, derive_raster_mask};
    use std::sync::Arc;

    for fill in [0, 255] {
        let raw = Arc::new(Mask::from_fn(4, 3, fill, |x, y| {
            if x == 1 && y == 1 { 255 - fill } else { fill }
        }));
        // Literal old halo for radius 1.7: three one-pixel box passes. The
        // oracle uses a separately padded plane, not the extracted helper.
        let padded = Mask::from_fn(10, 9, fill, |x, y| {
            if (3..7).contains(&x) && (3..6).contains(&y) {
                raw.get(x - 3, y - 3)
            } else {
                fill
            }
        });
        let feathered = emulsion_raster::select::feather_with_fill(&padded, 1.7);
        for density in [0.0, 0.5, 1.0] {
            let expected = MaskProperties {
                density,
                feather: 0.0,
            }
            .apply(&feathered);
            let actual = derive_raster_mask(
                &raw,
                MaskProperties {
                    density,
                    feather: 1.7,
                },
                Mapping2::IDENTITY,
                MaskGrid {
                    width: 10,
                    height: 9,
                    offset: (-3, -3),
                },
            )
            .unwrap();
            assert_eq!(actual.fill(), expected.fill());
            assert_eq!(actual.to_gray8(), expected.to_gray8());
        }
        let direct = derive_raster_mask(
            &raw,
            MaskProperties::default(),
            Mapping2::IDENTITY,
            MaskGrid {
                width: 4,
                height: 3,
                offset: (0, 0),
            },
        )
        .unwrap();
        assert!(Arc::ptr_eq(&direct, &raw));
        let constant = Arc::new(Mask::empty(4, 3, fill));
        let direct = derive_raster_mask(
            &constant,
            MaskProperties {
                density: 1.0,
                feather: 1000.0,
            },
            Mapping2::Affine(glam::DAffine2::from_cols_array(&[
                0.5, 0.0, 0.0, 0.5, 900.0, -70.0,
            ])),
            MaskGrid {
                width: 4,
                height: 3,
                offset: (0, 0),
            },
        )
        .unwrap();
        assert!(Arc::ptr_eq(&direct, &constant));
    }
}

#[test]
fn invalid_dormant_metadata_and_all_vector_conflicts_fail_before_shortcuts() {
    let mut input = metadata();
    input.raster_mask = mask((4, 4));
    input.raster_mask.properties.density = 0.0;
    input.raster_mask.properties.feather = f32::INFINITY;
    input.raster_mask.enabled = false;
    assert!(matches!(
        preflight_stack_support(input),
        Err(SmartSupportError::MaskProcessing { .. })
    ));
    input.raster_mask = ComponentMaskMetadata::default();
    input.filter_mask = Some(ComponentMaskMetadata::default());
    assert!(matches!(
        preflight_stack_support(input),
        Err(SmartSupportError::MissingFilterMaskPlane)
    ));
    for variant in 0..3 {
        input = metadata();
        input.has_vector_mask = true;
        if variant > 0 {
            input.placement = SmartPlacement::Legacy(Placement::default());
            if variant == 1 {
                input.raster_mask.transform = Mapping2::Projective(Projective2::IDENTITY);
            } else {
                input.filter_mask = Some(mask((4, 4)));
            }
        }
        assert!(matches!(
            preflight_stack_support(input),
            Err(SmartSupportError::VectorMaskConflict)
        ));
    }
}

#[test]
fn preparation_cancels_before_or_during_output_grid_certification() {
    let mut input = metadata();
    input.source_size = (130, 3);
    input.raster_mask = mask((130, 3));
    assert!(matches!(
        preflight_stack_support_with_control(input, || false),
        Err(SmartSupportError::Cancelled)
    ));
    let mut controls = 0;
    let result = preflight_stack_support_with_control(input, || {
        controls += 1;
        controls < 4
    });
    assert!(matches!(result, Err(SmartSupportError::Cancelled)));
    assert_eq!(controls, 4);
    assert_eq!(input.source_size, (130, 3));
    assert_eq!(input.raster_mask.plane.unwrap().size, (130, 3));
}

#[test]
fn support_keys_keep_variants_signed_zeros_and_latent_or_processing_changes() {
    let affine = Mapping2::Affine(DAffine2::IDENTITY);
    let signed = Mapping2::Affine(DAffine2::from_cols_array(&[1.0, -0.0, 0.0, 1.0, -0.0, 0.0]));
    assert_ne!(MappingKey::from(affine), MappingKey::from(signed));
    assert_ne!(
        MappingKey::from(affine),
        MappingKey::from(Mapping2::Projective(Projective2::IDENTITY))
    );
    let mut input = metadata();
    let original = support(input);
    assert!(original.matches_metadata(input));
    input.raster_mask.transform = Mapping2::Projective(Projective2::IDENTITY);
    assert!(!original.matches_metadata(input));
    let latent = support(input);
    input.raster_mask.enabled = false;
    assert!(!latent.matches_metadata(input));
    input.raster_mask.enabled = true;
    input.raster_mask.plane = mask((4, 4)).plane;
    let present = support(input);
    input.raster_mask.plane.as_mut().unwrap().has_detail = false;
    assert!(!present.matches_metadata(input));
    input = metadata();
    input.source_size = (5, 4);
    assert!(!original.matches_metadata(input));
}

#[test]
fn stack_support_key_is_not_filter_pixel_provenance() {
    let first = [Filter::AddNoise {
        amount: 1.0,
        monochrome: false,
    }];
    let changed = [Filter::AddNoise {
        amount: 90.0,
        monochrome: true,
    }];
    let styles = [FilterStyle {
        opacity: 0.0,
        ..FilterStyle::default()
    }];
    let mut input = metadata();
    input.filters = &first;
    input.styles = &styles;
    let plan = support(input);
    input.filters = &changed;
    assert!(plan.matches_metadata(input)); // same support, emphatically not same pixels
    input.filters_enabled = false;
    assert!(!plan.matches_metadata(input));
    let too_many = vec![Filter::Invert; 33];
    input.filters = &too_many;
    assert!(matches!(
        preflight_stack_support(input),
        Err(SmartSupportError::FilterCount)
    ));
    input.filters = &[];
    assert!(matches!(
        preflight_stack_support(input),
        Err(SmartSupportError::OrphanFilterStyles)
    ));
}

#[test]
fn raster_affine_metadata_keeps_legacy_threshold_infinity_and_unordered_determinants() {
    let cases = [
        ([1., 0., 0., 1., 0., 0.], true),
        ([-0., 0., 0., 1., 0., 0.], false),
        ([1., 0., 0., 1e-13, 0., 0.], false),
        ([1., 0., 0., 1e-12, 0., 0.], true),
        ([1e200, 0., 0., 1e200, 0., 0.], true),
        ([1e200, 1e200, 1e200, 1e200, 0., 0.], true),
        ([f64::INFINITY, 0., 0., 1., 0., 0.], false),
        ([f64::NAN, 0., 0., 1., 0., 0.], false),
    ];
    let cancellation = DAffine2::from_cols_array(&cases[5].0);
    assert!(cancellation.is_finite());
    assert!(cancellation.matrix2.determinant().is_nan());
    for (columns, accepted) in cases {
        let mapping = Mapping2::Affine(DAffine2::from_cols_array(&columns));
        let mut legacy = crate::Document::new(4, 4);
        let mut node = crate::Node::new(
            1,
            "Retained affine",
            crate::NodeKind::Fill { rgba: [255; 4] },
        );
        node.mask_transform = mapping;
        legacy.nodes.push(node);
        legacy.next_id = 2;
        assert_eq!(legacy.validate().is_ok(), accepted, "legacy {columns:?}");
        let mut input = metadata();
        input.raster_mask.transform = mapping;
        // No plane is sampled: this is legacy metadata admission, not stronger
        // checked matrix-operation or rendering-domain admission.
        assert_eq!(
            preflight_stack_support(input).is_ok(),
            accepted,
            "support {columns:?}"
        );
    }
}
