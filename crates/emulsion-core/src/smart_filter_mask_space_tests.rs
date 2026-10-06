//! RGB-space correction only. The independent Photoshop target is a rasterized
//! pixel layer, never a saved merged preview or the unmasked filtered cache.
use crate::mapping::{Mapping2, SmartPlacement};
use crate::smart_filter_mask::{effective_pixels, effective_pixels_with_space};
use crate::{Command, Document, Editor, Node, NodeKind, SmartFilterMask};
use emulsion_filters::{Filter, FilterStyle};
use emulsion_raster::blend::BlendSpace;
use emulsion_raster::{Mask, Placement, Raster, color, composite::flatten};
use std::sync::Arc;

const SOURCE: &[u8; 6400] =
    include_bytes!("../tests/fixtures/photoshop-smart-filter-mask/projected-unfiltered-rgba8.bin");
const FILTERED: &[u8; 6400] =
    include_bytes!("../tests/fixtures/photoshop-smart-filter-mask/unmasked-filtered-rgba8.bin");
const MASK: &[u8; 1600] =
    include_bytes!("../tests/fixtures/photoshop-smart-filter-mask/shared-mask-u8.bin");
const TARGET: &[u8; 6400] = include_bytes!(
    "../tests/fixtures/photoshop-smart-filter-mask/photoshop-rasterized-target-rgba8.bin"
);

fn fixture() -> Document {
    let mut doc = Document::new(40, 40);
    doc.blend_space = BlendSpace::PhotoshopSrgbV1;
    doc.nodes.push(Node::new(
        1,
        "Independent Photoshop stack-mask inputs",
        NodeKind::Smart {
            filters_enabled: true,
            editable: None,
            original_image: None,
            // This is the placed FEid pre-filter cache, NOT the embedded PNG
            // editable source. Injecting it isolates the mask operation from
            // Photoshop's Gaussian kernel and nonuniform source resampling.
            source: Arc::new(Raster::from_srgba8(40, 40, SOURCE)),
            cache: Arc::new(Raster::from_srgba8(40, 40, FILTERED)),
            filters: vec![Filter::GaussianBlur { radius: 4.5 }],
            filter_styles: vec![],
            filter_mask: Some(SmartFilterMask::new(Arc::new(Mask::from_fn(
                40,
                40,
                255,
                |x, y| MASK[(y * 40 + x) as usize],
            )))),
            offset: (0, 0),
            placement: SmartPlacement::Legacy(Placement::default()),
        },
    ));
    doc.next_id = 2;
    doc
}

fn errors(actual: &[u8]) -> (u8, u8) {
    assert_eq!(actual.len(), TARGET.len());
    let (mut rgb, mut alpha) = (0, 0);
    for (a, b) in actual
        .as_chunks::<4>()
        .0
        .iter()
        .zip(TARGET.as_chunks::<4>().0)
    {
        alpha = alpha.max(a[3].abs_diff(b[3]));
        if b[3] != 0 {
            for c in 0..3 {
                rgb = rgb.max(a[c].abs_diff(b[c]));
            }
        }
    }
    (rgb, alpha)
}

fn assert_independent_target(actual: &[u8]) {
    let (rgb, alpha) = errors(actual);
    // Declared before implementation: separate 8-bit saved stages can differ
    // by one code. Hidden RGB is undefined; alpha is checked everywhere.
    assert!(
        rgb <= 1 && alpha <= 1,
        "RGB error {rgb}; alpha error {alpha}"
    );
}

#[test]
fn smart_filter_profile_matches_independent_rasterized_final_pixels() {
    let doc = fixture();
    let result = effective_pixels_with_space(&doc.nodes[0], doc.blend_space)
        .unwrap()
        .unwrap();
    assert_independent_target(&result.to_srgba8());
    assert_independent_target(&flatten(&doc.composite_tree(), 0).to_srgba8());
    assert_independent_target(&flatten(&doc.solo(1).unwrap().composite_tree(), 0).to_srgba8());

    // Two independent negative controls: unmasked cache and old linear mix.
    assert!(errors(FILTERED).0 > 20);
    let legacy = effective_pixels(&doc.nodes[0]).unwrap().unwrap();
    assert!(errors(&legacy.to_srgba8()).0 >= 30);
}

#[test]
fn smart_filter_profile_respects_negative_cache_origin_and_document_mask() {
    let mut doc = fixture();
    let NodeKind::Smart {
        source,
        offset,
        placement,
        filter_mask,
        ..
    } = &mut doc.nodes[0].kind
    else {
        unreachable!()
    };
    // FEid B's finite occupied rectangle is x=15..33, y=18..28. This
    // synthetic native representation keeps those same inputs in source-local
    // coordinates while the filter cache and authored mask stay document-wide.
    let projected = source.clone();
    *source = Arc::new(Raster::from_fn(18, 10, [0; 4], |x, y| {
        projected.get(x + 15, y + 18)
    }));
    *offset = (-15, -18);
    *placement = SmartPlacement::Legacy(Placement::at(15., 18.));
    filter_mask.as_mut().unwrap().transform =
        Mapping2::from_affine_columns([1., 0., 0., 1., -15., -18.]).unwrap();
    assert_independent_target(&flatten(&doc.composite_tree(), 0).to_srgba8());
    let NodeKind::Smart { filter_mask, .. } = &mut doc.nodes[0].kind else {
        unreachable!()
    };
    // Incorrectly treating document mask coordinates as source-local must fail.
    filter_mask.as_mut().unwrap().transform = Mapping2::IDENTITY;
    let wrong = flatten(&doc.composite_tree(), 0).to_srgba8();
    assert!(errors(&wrong).0 > 20 || errors(&wrong).1 > 20);
}

#[test]
fn smart_filter_old_profiles_remain_exact_for_every_mask_code() {
    let mut doc = fixture();
    for m in 0..=255u32 {
        let NodeKind::Smart { filter_mask, .. } = &mut doc.nodes[0].kind else {
            unreachable!()
        };
        filter_mask.as_mut().unwrap().pixels = Arc::new(Mask::empty(40, 40, m as u8));
        let NodeKind::Smart { source, cache, .. } = &doc.nodes[0].kind else {
            unreachable!()
        };
        let legacy = effective_pixels(&doc.nodes[0]).unwrap().unwrap();
        for space in [BlendSpace::Linear, BlendSpace::Srgb] {
            let out = effective_pixels_with_space(&doc.nodes[0], space)
                .unwrap()
                .unwrap();
            for y in 0..40 {
                for x in 0..40 {
                    let a = source.get(x, y);
                    let b = cache.get(x, y);
                    let expected = std::array::from_fn(|c| {
                        ((u32::from(a[c]) * (255 - m) + u32::from(b[c]) * m + 127) / 255) as u16
                    });
                    assert_eq!(out.get(x, y), expected);
                    assert_eq!(out.get(x, y), legacy.get(x, y));
                }
            }
        }
    }
}

#[test]
fn smart_filter_profile_alpha_endpoints_and_partial_transparency() {
    // A separate native micro-oracle uses f64 transfer functions, not the
    // implementation's f32 helper. Distinct endpoint alphas detect accidental
    // straight-RGB interpolation and transfer of premultiplied channels.
    let decode = |v: f64| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let encode = |v: f64| {
        if v <= 0.0031308 {
            v * 12.92
        } else {
            1.055 * v.powf(1. / 2.4) - 0.055
        }
    };
    let mut doc = fixture();
    for (a, b) in [
        ([12, 220, 49, 51], [242, 31, 180, 204]),
        ([0, 0, 0, 0], [31, 140, 221, 127]),
        ([174, 23, 199, 93], [0, 0, 0, 0]),
        ([0; 4], [0; 4]),
    ] {
        let a = color::f_to_px(color::srgba8_to_premul(a));
        let b = color::f_to_px(color::srgba8_to_premul(b));
        for m in [0, 1, 64, 127, 128, 192, 254, 255u32] {
            let NodeKind::Smart {
                source,
                cache,
                filter_mask,
                ..
            } = &mut doc.nodes[0].kind
            else {
                unreachable!()
            };
            *source = Arc::new(Raster::empty(1, 1, a));
            *cache = Arc::new(Raster::empty(1, 1, b));
            filter_mask.as_mut().unwrap().pixels = Arc::new(Mask::empty(1, 1, m as u8));
            let out = effective_pixels_with_space(&doc.nodes[0], doc.blend_space)
                .unwrap()
                .unwrap();
            let pixel = out.get(0, 0);
            let aw = u32::from(a[3]) * (255 - m);
            let bw = u32::from(b[3]) * m;
            let alpha = ((aw + bw + 127) / 255) as u16;
            assert_eq!(pixel[3], alpha);
            assert!(pixel[..3].iter().all(|v| *v <= alpha));
            if m == 0 {
                assert_eq!(pixel, a);
            } else if m == 255 {
                assert_eq!(pixel, b);
                let NodeKind::Smart { cache, .. } = &doc.nodes[0].kind else {
                    unreachable!()
                };
                assert!(Arc::ptr_eq(&out, cache));
            } else if alpha != 0 {
                for c in 0..3 {
                    let encoded = |p: [u16; 4]| {
                        if p[3] == 0 {
                            0.
                        } else {
                            encode(f64::from(p[c]) / f64::from(p[3]))
                        }
                    };
                    let straight = (encoded(a) * f64::from(aw) + encoded(b) * f64::from(bw))
                        / f64::from(aw + bw);
                    let expected = (decode(straight) * f64::from(alpha)).round() as u16;
                    assert!(pixel[c].abs_diff(expected) <= 1);
                }
            } else {
                assert_eq!(pixel, [0; 4]);
            }
        }
    }
}

#[test]
fn smart_filter_profile_toggle_rekeys_pixels_and_preserves_sources_and_history() {
    let mut doc = fixture();
    let retained = Arc::new(crate::node::OriginalImage::new(
        Arc::new(b"immutable encoded-source test sentinel".to_vec()),
        [1; 32],
        [2; 32],
    ));
    let NodeKind::Smart { original_image, .. } = &mut doc.nodes[0].kind else {
        unreachable!()
    };
    *original_image = Some(retained.clone());
    let before = doc.nodes[0].clone();
    let mut editor = Editor::new(doc, None);
    let profiled = effective_pixels_with_space(&before, BlendSpace::PhotoshopSrgbV1)
        .unwrap()
        .unwrap();
    for space in [
        BlendSpace::Linear,
        BlendSpace::Srgb,
        BlendSpace::PhotoshopSrgbV1,
    ] {
        assert_eq!(
            Command::SetBlendSpace { space }.dirty(&editor.doc),
            crate::Dirty::All
        );
        editor.execute(Command::SetBlendSpace { space }).unwrap();
        assert_eq!(editor.doc.nodes[0], before);
        let out = effective_pixels_with_space(&editor.doc.nodes[0], space)
            .unwrap()
            .unwrap();
        let tree = flatten(&editor.doc.composite_tree(), 0);
        assert_eq!(tree.to_srgba8(), out.to_srgba8());
        if space == BlendSpace::PhotoshopSrgbV1 {
            assert!(Arc::ptr_eq(&out, &profiled));
            assert_independent_target(&tree.to_srgba8());
        } else {
            assert!(!Arc::ptr_eq(&out, &profiled));
            assert!(errors(&tree.to_srgba8()).0 >= 30);
        }
    }
    assert!(editor.undo());
    assert_eq!(editor.doc.blend_space, BlendSpace::Srgb);
    assert!(editor.redo());
    assert_eq!(editor.doc.blend_space, BlendSpace::PhotoshopSrgbV1);
    let NodeKind::Smart {
        source,
        cache,
        filter_mask,
        original_image,
        ..
    } = &editor.doc.nodes[0].kind
    else {
        unreachable!()
    };
    let NodeKind::Smart {
        source: old_source,
        cache: old_cache,
        filter_mask: old_mask,
        ..
    } = &before.kind
    else {
        unreachable!()
    };
    assert!(Arc::ptr_eq(source, old_source));
    assert!(Arc::ptr_eq(cache, old_cache));
    assert!(Arc::ptr_eq(
        &filter_mask.as_ref().unwrap().pixels,
        &old_mask.as_ref().unwrap().pixels
    ));
    assert_eq!(source.to_srgba16(), old_source.to_srgba16());
    assert_eq!(cache.to_srgba16(), old_cache.to_srgba16());
    assert!(Arc::ptr_eq(original_image.as_ref().unwrap(), &retained));
    assert_eq!(
        retained.bytes().as_slice(),
        b"immutable encoded-source test sentinel"
    );
}

#[test]
fn smart_filter_profile_rasterize_and_styled_render_keep_exact_appearance() {
    for with_mask in [false, true] {
        let mut doc = fixture();
        doc.nodes[0]
            .styles
            .push(crate::styles::LayerStyle::ColorOverlay {
                color: [49, 210, 67],
                opacity: 37.,
            });
        if with_mask {
            doc.nodes[0].mask = Some(Arc::new(Mask::empty(40, 40, 173)));
        }
        let rect = emulsion_raster::IRect::new(0, 0, 40, 40);
        let expected_raster = flatten(&doc.composite_tree(), 0);
        let expected = expected_raster.to_srgba8();
        // A separately baked reference catches profile loss through the style
        // solo-render path while keeping all style algorithms unchanged.
        let mut reference = doc.clone();
        reference.nodes[0].kind = NodeKind::Raster {
            raster: effective_pixels_with_space(&doc.nodes[0], doc.blend_space)
                .unwrap()
                .unwrap(),
            placement: Placement::default(),
        };
        assert_eq!(
            flatten(&reference.composite_tree(), 0).to_srgba8(),
            expected
        );
        assert_eq!(
            flatten(&reference.composite_tree(), 0).read_rect(rect),
            expected_raster.read_rect(rect)
        );
        let mut editor = Editor::new(doc.clone(), None);
        editor.execute(Command::Rasterize { id: 1 }).unwrap();
        assert_eq!(
            flatten(&editor.doc.composite_tree(), 0).to_srgba8(),
            expected
        );
        assert_eq!(
            flatten(&editor.doc.composite_tree(), 0).read_rect(rect),
            expected_raster.read_rect(rect)
        );
        assert!(editor.undo());
        assert_eq!(editor.doc, doc);
        assert_eq!(
            flatten(&editor.doc.composite_tree(), 0).to_srgba8(),
            expected
        );
        assert!(editor.redo());
        assert_eq!(
            flatten(&editor.doc.composite_tree(), 0).read_rect(rect),
            expected_raster.read_rect(rect)
        );
        assert_eq!(
            flatten(&editor.doc.composite_tree(), 0).to_srgba8(),
            expected
        );
    }
}

fn alias_fixture(mask_state: &str) -> Document {
    let mut doc = fixture();
    let NodeKind::Smart { filter_mask, .. } = &mut doc.nodes[0].kind else {
        unreachable!()
    };
    match mask_state {
        "absent" => *filter_mask = None,
        "disabled" => filter_mask.as_mut().unwrap().enabled = false,
        "white" => filter_mask.as_mut().unwrap().pixels = Arc::new(Mask::empty(40, 40, 255)),
        "zero-density" => filter_mask.as_mut().unwrap().properties.density = 0.,
        "partial" => {}
        _ => unreachable!(),
    }
    doc
}

fn assert_profile_alias_preserves_smart_state(node: &Node, expected: &Arc<Raster>) {
    let before = node.clone();
    let NodeKind::Smart {
        source,
        cache,
        offset,
        filter_mask,
        ..
    } = &before.kind
    else {
        unreachable!()
    };
    let source_pixels = source.read_rect(source.bounds());
    let cache_pixels = cache.read_rect(cache.bounds());
    let mask_pixels = filter_mask
        .as_ref()
        .map(|mask| mask.pixels.read_rect(mask.pixels.bounds()));
    for space in [
        BlendSpace::Linear,
        BlendSpace::Srgb,
        BlendSpace::PhotoshopSrgbV1,
    ] {
        let out = effective_pixels_with_space(node, space).unwrap().unwrap();
        assert!(Arc::ptr_eq(&out, expected), "wrong alias in {space:?}");
        // Node equality covers authored metadata and source/mask identities,
        // but intentionally excludes the derived raw cache and its offset.
        assert_eq!(node, &before);
        let NodeKind::Smart {
            cache: actual_cache,
            offset: actual_offset,
            ..
        } = &node.kind
        else {
            unreachable!()
        };
        assert!(Arc::ptr_eq(actual_cache, cache));
        assert_eq!(actual_offset, offset);
        assert_eq!(source.read_rect(source.bounds()), source_pixels);
        assert_eq!(cache.read_rect(cache.bounds()), cache_pixels);
        assert_eq!(
            filter_mask
                .as_ref()
                .map(|mask| mask.pixels.read_rect(mask.pixels.bounds())),
            mask_pixels
        );
    }
}

#[test]
fn smart_filter_profile_mask_noops_keep_active_cache_alias() {
    for zero_opacity in [false, true] {
        for mask_state in ["absent", "disabled", "white", "zero-density"] {
            let mut doc = alias_fixture(mask_state);
            let NodeKind::Smart {
                source,
                cache,
                offset,
                filters,
                filter_styles,
                filters_enabled,
                ..
            } = &mut doc.nodes[0].kind
            else {
                unreachable!()
            };
            if zero_opacity {
                *filter_styles = vec![FilterStyle {
                    opacity: 0.,
                    ..Default::default()
                }];
                (*cache, *offset) =
                    crate::smart::render_stack(source, filters, filter_styles, *filters_enabled);
                // Enabled opacity zero still has an expanded cache, unlike a
                // disabled stage. It must not take the source-alias bypass.
                assert!(cache.width() > source.width());
                assert!(offset.0 < 0);
            }
            let full = cache.clone();
            assert!(!Arc::ptr_eq(source, &full));
            assert_profile_alias_preserves_smart_state(&doc.nodes[0], &full);
        }
    }
}

#[test]
fn smart_filter_profile_dormant_stacks_alias_source_and_preserve_stale_cache() {
    for state in ["empty", "root-disabled", "all-stages-disabled"] {
        for mask_state in ["absent", "disabled", "white", "zero-density", "partial"] {
            let mut doc = alias_fixture(mask_state);
            let NodeKind::Smart {
                source,
                cache,
                offset,
                filters,
                filter_styles,
                filters_enabled,
                ..
            } = &mut doc.nodes[0].kind
            else {
                unreachable!()
            };
            match state {
                "empty" => filters.clear(),
                "root-disabled" => *filters_enabled = false,
                "all-stages-disabled" => {
                    *filter_styles = vec![FilterStyle {
                        enabled: false,
                        ..Default::default()
                    }];
                }
                _ => unreachable!(),
            }
            // Deliberately retain a distinct stale filtered cache and offset.
            // Reading effective pixels bypasses them without rewriting either.
            *offset = (-3, -2);
            assert!(!Arc::ptr_eq(source, cache));
            assert_ne!(
                source.read_rect(source.bounds()),
                cache.read_rect(cache.bounds())
            );
            let original = source.clone();
            assert_profile_alias_preserves_smart_state(&doc.nodes[0], &original);
        }
    }
}

// Independent f64 transfer/Normal-over model for the mask-bake contract below.
// It consumes known source/effect storage, never a flattened reference image.
fn encoded_premul64(pixel: [f64; 4]) -> [f64; 4] {
    if pixel[3] == 0. {
        return [0.; 4];
    }
    let mut result = pixel;
    for c in 0..3 {
        let v = pixel[c] / pixel[3];
        result[c] = pixel[3]
            * if v <= 0.0031308 {
                12.92 * v
            } else {
                1.055 * v.powf(1. / 2.4) - 0.055
            };
    }
    result
}

fn encoded_over64(backdrop: [f64; 4], source: [f64; 4]) -> [f64; 4] {
    std::array::from_fn(|c| source[c] + backdrop[c] * (1. - source[3]))
}

fn assert_quantized_render_pixel(actual: [f32; 4], stored: [u16; 4], expected_encoded: [f64; 4]) {
    let encoded = encoded_premul64(actual.map(f64::from));
    // The scalar oracle uses f64; the renderer's two Normal-over stages use
    // f32 transfer functions. Reserve 32 f32 epsilons per unit of alpha for
    // their arithmetic. This scales to zero with coverage and is not a
    // tolerance fitted to the observed mask-bake pixel differences.
    let roundoff = 32. * f64::from(f32::EPSILON) * expected_encoded[3];
    for c in 0..4 {
        assert!(
            (encoded[c] - expected_encoded[c]).abs() <= roundoff,
            "channel {c}: encoded {encoded:?}; independent model {expected_encoded:?}"
        );
        let linear = if c == 3 || expected_encoded[3] == 0. {
            expected_encoded[c]
        } else {
            let v = expected_encoded[c] / expected_encoded[3];
            expected_encoded[3]
                * if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
        };
        // For a premultiplied sRGB decode D, |d(A D(C/A))/dC| <= 2.4/1.055
        // and |d(A D(C/A))/dA| <= 2.4/1.055 - 1. Propagate only the f32
        // evaluation budget, then perform the final u16 rounding explicitly.
        let linear_error = roundoff * if c == 3 { 1. } else { 2. * (2.4 / 1.055) - 1. };
        let low = ((linear - linear_error).clamp(0., 1.) * 65535. + 0.5) as u16;
        let high = ((linear + linear_error).clamp(0., 1.) * 65535. + 0.5) as u16;
        assert!(
            (low..=high).contains(&stored[c]),
            "channel {c}: stored {stored:?}; independent range {low}..={high}"
        );
    }
}

#[test]
fn smart_filter_profile_apply_mask_obeys_u16_quantization_contract() {
    use emulsion_raster::composite::render_tile_cpu;
    use emulsion_raster::{IRect, TILE, TileCoord};

    for representable in [false, true] {
        for styled in [false, true] {
            for coverage in [0u8, 173, 255] {
                let mut doc = fixture();
                if representable {
                    let NodeKind::Smart { source, cache, .. } = &mut doc.nodes[0].kind else {
                        unreachable!()
                    };
                    // Every channel is divisible by 255, so even coverage=173
                    // has an exactly representable masked u16 result.
                    *source = Arc::new(Raster::empty(40, 40, [u16::MAX, 0, 0, u16::MAX]));
                    *cache = source.clone();
                }
                if styled {
                    doc.nodes[0]
                        .styles
                        .push(crate::styles::LayerStyle::ColorOverlay {
                            color: [49, 210, 67],
                            opacity: 37.,
                        });
                }
                doc.nodes[0].mask = Some(Arc::new(Mask::empty(40, 40, coverage)));
                let effective = effective_pixels_with_space(&doc.nodes[0], doc.blend_space)
                    .unwrap()
                    .unwrap();
                // This f64 rational-nearest reference is independent of the
                // command's integer multiply/divide. With divisor 255, its
                // maximum residual is 127/255 of one u16 code, below 1/2.
                let expected = Arc::new(Raster::from_fn(40, 40, [0; 4], |x, y| {
                    effective
                        .get(x, y)
                        .map(|v| (f64::from(v) * f64::from(coverage) / 255.).round() as u16)
                }));
                let mut reference = doc.clone();
                reference.nodes[0].kind = NodeKind::Raster {
                    raster: expected.clone(),
                    placement: Placement::default(),
                };
                reference.nodes[0].mask = None;
                let mut editor = Editor::new(doc.clone(), None);
                editor.execute(Command::ApplyLayerMask { id: 1 }).unwrap();
                let baked = &editor.doc;
                let NodeKind::Raster { raster, placement } = &baked.nodes[0].kind else {
                    panic!("ApplyLayerMask did not produce a raster");
                };
                assert_eq!(*placement, Placement::default());
                assert!(baked.nodes[0].mask.is_none());
                let rect = IRect::new(0, 0, 40, 40);
                assert_eq!(raster.read_rect(rect), expected.read_rect(rect));

                let before_effects = crate::styles::render(&doc, &doc.nodes[0]);
                let after_effects = crate::styles::render(baked, &baked.nodes[0]);
                let effect_at = |effects: Option<&crate::styles::Rendered>, x: u32, y: u32| {
                    let Some(effects) = effects else {
                        return [0; 4];
                    };
                    assert!(effects.below.is_empty());
                    assert_eq!(effects.above.len(), 1);
                    let effect = &effects.above[0];
                    let (x, y) = (x as i32 - effect.rect.x, y as i32 - effect.rect.y);
                    if x < 0 || y < 0 || x >= effect.rect.w || y >= effect.rect.h {
                        [0; 4]
                    } else {
                        effect.raster.get(x as u32, y as u32)
                    }
                };
                for background in [None, Some([0, 0, 0, 255]), Some([255; 4])] {
                    let with_background = |input: &Document| {
                        let mut result = input.clone();
                        if let Some(rgba) = background {
                            result
                                .nodes
                                .insert(0, Node::new(2, "Backdrop", NodeKind::Fill { rgba }));
                            result.next_id = 3;
                        }
                        result
                    };
                    let before_tree = with_background(&doc).composite_tree();
                    let after_tree = with_background(baked).composite_tree();
                    let reference_tree = with_background(&reference).composite_tree();
                    let before = flatten(&before_tree, 0);
                    let after = flatten(&after_tree, 0);
                    // Exact full-image oracle: styles, profile and placement
                    // must match independently quantized content, not raw F.
                    assert_eq!(
                        after.read_rect(rect),
                        flatten(&reference_tree, 0).read_rect(rect)
                    );
                    let before_float = render_tile_cpu(&before_tree, 0, TileCoord::new(0, 0));
                    let after_float = render_tile_cpu(&after_tree, 0, TileCoord::new(0, 0));
                    let before_bytes = before.to_srgba8();
                    let after_bytes = after.to_srgba8();
                    for y in 0..40 {
                        for x in 0..40 {
                            let index = (y * TILE + x) as usize;
                            let source = effective
                                .get(x, y)
                                .map(|v| f64::from(v) / 65535. * f64::from(coverage) / 255.);
                            let quantized = expected.get(x, y).map(|v| f64::from(v) / 65535.);
                            let effect_before = effect_at(before_effects.as_deref(), x, y);
                            let effect_after = effect_at(after_effects.as_deref(), x, y);
                            for c in 0..4 {
                                // The overlay is linear in alpha before its
                                // own nearest-u16 rounding. A <1/2-code input
                                // change at 37% opacity can cross one boundary.
                                assert!(effect_before[c].abs_diff(effect_after[c]) <= 1);
                            }
                            let model = |source, effect: [u16; 4]| {
                                let appearance = encoded_over64(
                                    encoded_premul64(source),
                                    encoded_premul64(effect.map(|v| f64::from(v) / 65535.)),
                                );
                                let backdrop = background
                                    .map_or([0.; 4], |rgba| rgba.map(|v| f64::from(v) / 255.));
                                encoded_over64(backdrop, appearance)
                            };
                            let before_model = model(source, effect_before);
                            let after_model = model(quantized, effect_after);
                            assert_quantized_render_pixel(
                                before_float[index],
                                before.get(x, y),
                                before_model,
                            );
                            assert_quantized_render_pixel(
                                after_float[index],
                                after.get(x, y),
                                after_model,
                            );
                            if background.is_some() {
                                // Compare visible opaque colors. Independently
                                // predicted encoded differences must be less
                                // than half an 8-bit code; final storage/byte
                                // rounding may move the exported pixel by one.
                                let byte_index = ((y * 40 + x) * 4) as usize;
                                for c in 0..4 {
                                    assert!((before_model[c] - after_model[c]).abs() * 255. < 0.5);
                                    assert!(
                                        before_bytes[byte_index + c]
                                            .abs_diff(after_bytes[byte_index + c])
                                            <= 1
                                    );
                                }
                            }
                        }
                    }
                    if coverage == 0 || coverage == 255 || representable {
                        assert_eq!(after.read_rect(rect), before.read_rect(rect));
                        assert_eq!(after_bytes, before_bytes);
                    }
                }
                assert!(editor.undo());
                assert_eq!(editor.doc, doc);
                assert_eq!(
                    flatten(&editor.doc.composite_tree(), 0).read_rect(rect),
                    flatten(&doc.composite_tree(), 0).read_rect(rect)
                );
                assert!(editor.redo());
                assert_eq!(
                    flatten(&editor.doc.composite_tree(), 0).read_rect(rect),
                    flatten(&reference.composite_tree(), 0).read_rect(rect)
                );
            }
        }
    }
}
