//! Fixed Photoshop controls and separately labelled analytic native contracts.
//! Expected pixels are literals or independent f64 IEC transfer calculations,
//! never rendered with a second production blend path to manufacture an oracle.

use super::*;

const PS: BlendSpace = BlendSpace::PhotoshopSrgbV1;
const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
const WHITE: [f32; 4] = [1.0; 4];

fn fill(id: u64, color: [f32; 4]) -> CompositeNode {
    CompositeNode {
        id,
        visible: true,
        opacity: 1.0,
        blend: BlendMode::Normal,
        blending: BlendingOptions::default(),
        mask: None,
        clip_to: None,
        clip_rect: None,
        content: NodeContent::Fill(color),
    }
}
fn pixels(id: u64, color: [f32; 4]) -> CompositeNode {
    let mut node = fill(id, color);
    node.content = NodeContent::Pixels {
        raster: Arc::new(Raster::solid(1, 1, color)).into(),
        placement: Placement::default(),
    };
    node
}
fn group(id: u64, children: Vec<CompositeNode>, mode: BlendMode) -> CompositeNode {
    let mut node = fill(id, [0.0; 4]);
    node.content = NodeContent::Group(children);
    node.blend = mode;
    node
}
fn tree(nodes: Vec<CompositeNode>, target: Option<u64>) -> CompositeTree {
    CompositeTree {
        width: 1,
        height: 1,
        space: PS,
        knockout_background: target,
        nodes,
    }
}
fn render(tree: &CompositeTree) -> [f32; 4] {
    render_tile_cpu(tree, 0, TileCoord::new(0, 0))[0]
}
fn raw16(tree: &CompositeTree) -> [u16; 4] {
    color::f_to_px(render(tree))
}
fn close(actual: [f32; 4], expected: [f32; 4]) {
    for (a, b) in actual.into_iter().zip(expected) {
        assert!((a - b).abs() < 2e-6, "{actual:?} != {expected:?}");
    }
}
fn decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
/// Independent expected linear storage from encoded premultiplied coordinates.
fn encoded(rgb: [f64; 3], alpha: f64) -> [f32; 4] {
    if alpha == 0.0 {
        return [0.0; 4];
    }
    [
        (decode(rgb[0] / alpha) * alpha) as f32,
        (decode(rgb[1] / alpha) * alpha) as f32,
        (decode(rgb[2] / alpha) * alpha) as f32,
        alpha as f32,
    ]
}
fn knockout(mut node: CompositeNode, mode: Knockout, opacity: f32, fill: f32) -> CompositeNode {
    node.blending.knockout = mode;
    node.opacity = opacity;
    node.blending.fill_opacity = fill;
    node
}

#[test]
fn profile_wire_values_and_default_remain_frozen() {
    for (space, name) in [
        (BlendSpace::Linear, "linear"),
        (BlendSpace::Srgb, "srgb"),
        (PS, "photoshop-srgb-v1"),
    ] {
        let json = format!("\"{name}\"");
        assert_eq!(serde_json::to_string(&space).unwrap(), json);
        assert_eq!(serde_json::from_str::<BlendSpace>(&json).unwrap(), space);
    }
    assert_eq!(BlendSpace::default(), BlendSpace::Linear);
}

#[test]
fn photoshop_2026_four_fixed_group_knockout_controls() {
    // Numeric editable inputs of the independent fixtures. These four exact
    // controls do not establish parity for fractional or styled knockouts.
    for (knockout_mode, group_mode, expected16, expected8) in [
        (
            Knockout::None,
            BlendMode::Normal,
            [0, 13909, 14146, 65535],
            [0, 127, 128, 255],
        ),
        (
            Knockout::Shallow,
            BlendMode::Normal,
            [13909, 0, 14146, 65535],
            [127, 0, 128, 255],
        ),
        (
            Knockout::Deep,
            BlendMode::Normal,
            [13909, 0, 14146, 65535],
            [127, 0, 128, 255],
        ),
        (
            Knockout::Deep,
            BlendMode::PassThrough,
            [13909, 13909, 65535, 65535],
            [127, 127, 255, 255],
        ),
    ] {
        let source = knockout(pixels(4, BLUE), knockout_mode, 1.0, 128.0 / 255.0);
        let tree = tree(
            vec![
                pixels(1, WHITE),
                pixels(2, RED),
                group(5, vec![pixels(3, GREEN), source], group_mode),
            ],
            Some(1),
        );
        assert_eq!(raw16(&tree), expected16, "{knockout_mode:?}/{group_mode:?}");
        assert_eq!(flatten(&tree, 0).to_srgba8(), expected8);
    }
}

#[test]
fn legacy_normal_primary_raw16_is_not_redefined() {
    for space in [BlendSpace::Linear, BlendSpace::Srgb] {
        let mut source = pixels(2, BLUE);
        source.blending.fill_opacity = 128.0 / 255.0;
        let mut tree = tree(vec![pixels(1, GREEN), source], Some(1));
        tree.space = space;
        assert_eq!(raw16(&tree), [0, 32639, 32896, 65535]);
    }
}

#[test]
fn normal_fill_and_pixel_fast_paths_use_encoded_alpha() {
    let half_blue = [0.0, 0.0, 0.5, 0.5];
    let expected = encoded([0.0, 0.5, 0.5], 1.0);
    close(
        render(&tree(vec![fill(1, GREEN), fill(2, half_blue)], None)),
        expected,
    );
    let mut blue = pixels(2, BLUE);
    blue.opacity = 0.5;
    close(render(&tree(vec![pixels(1, GREEN), blue], None)), expected);
    for mode in [BlendMode::Normal, BlendMode::PassThrough] {
        close(blend_px(mode, PS, GREEN, half_blue, 0.0), expected);
    }
}

#[test]
fn analytic_knockout_separates_punch_from_paint() {
    let source = knockout(fill(3, BLUE), Knockout::Deep, 0.5, 0.5);
    assert_eq!(
        raw16(&tree(
            vec![pixels(1, RED), fill(2, GREEN), source.clone()],
            Some(1)
        )),
        [3334, 14027, 3334, 65535]
    );
    assert_eq!(
        raw16(&tree(vec![fill(2, GREEN), source], None)),
        [0, 19758, 4465, 49151]
    );
}

#[test]
fn analytic_authored_mask_opacity_and_fill_are_applied_once() {
    let mut source = knockout(pixels(3, BLUE), Knockout::Deep, 0.5, 0.5);
    source.mask = Some(Arc::new(Mask::empty(1, 1, 128)));
    assert_eq!(
        raw16(&tree(
            vec![pixels(1, RED), fill(2, GREEN), source.clone()],
            Some(1)
        )),
        [947, 34143, 947, 65535]
    );
    source.clip_rect = Some([0.0, 0.0, 0.5, 1.0]);
    let q = 32.0 / 255.0;
    close(
        render(&tree(vec![pixels(1, RED), fill(2, GREEN), source], Some(1))),
        encoded([q / 2.0, 1.0 - q, q / 2.0], 1.0),
    );
}

#[test]
fn zero_fill_punches_but_zero_opacity_does_nothing() {
    let source = knockout(fill(3, BLUE), Knockout::Deep, 0.5, 0.0);
    close(
        render(&tree(
            vec![pixels(1, RED), fill(2, GREEN), source.clone()],
            Some(1),
        )),
        encoded([0.5, 0.5, 0.0], 1.0),
    );
    let mut source = source;
    source.opacity = 0.0;
    assert_eq!(
        render(&tree(vec![pixels(1, RED), fill(2, GREEN), source], Some(1))),
        GREEN
    );
}

#[test]
fn sequential_knockouts_resolve_each_operation_instead_of_max_union() {
    let a = knockout(fill(3, BLUE), Knockout::Deep, 0.5, 0.0);
    let b = knockout(fill(4, BLUE), Knockout::Deep, 0.5, 0.0);
    close(
        render(&tree(vec![pixels(1, RED), fill(2, GREEN), a, b], Some(1))),
        encoded([0.75, 0.25, 0.0], 1.0),
    );
}

#[test]
fn transparency_shapes_off_keeps_bounds_and_paint_alpha_separate() {
    let mut source = knockout(pixels(3, [0.0, 0.0, 0.25, 0.25]), Knockout::Deep, 0.5, 0.5);
    source.blending.transparency_shapes_layer = false;
    source.mask = Some(Arc::new(Mask::empty(1, 1, 128)));
    source.clip_rect = Some([0.0, 0.0, 0.5, 1.0]);
    // Stored RGBA16 alpha is 16384/65535. q uses bounds, not that alpha.
    let q = 32.0 / 255.0;
    let a = q * (16384.0 / 65535.0) * 0.5;
    close(
        render(&tree(vec![pixels(1, RED), fill(2, GREEN), source], Some(1))),
        encoded([q - a, 1.0 - q, a], 1.0),
    );
}

#[test]
fn explicit_background_identity_is_never_inferred_and_hidden_stop_is_transparent() {
    let make = |target| {
        tree(
            vec![
                pixels(1, WHITE),
                fill(2, RED),
                knockout(fill(3, BLUE), Knockout::Deep, 1.0, 0.0),
            ],
            target,
        )
    };
    assert_eq!(render(&make(Some(1))), WHITE);
    assert_eq!(render(&make(None)), [0.0; 4]);
    assert_eq!(render(&make(Some(2))), [0.0; 4]); // ordinary non-bottom layer
    assert_eq!(render(&make(Some(404))), [0.0; 4]);
    let mut hidden = make(Some(1));
    hidden.nodes[0].visible = false;
    assert_eq!(render(&hidden), [0.0; 4]);
    let mut duplicate = make(Some(1));
    duplicate.nodes[1].id = 1;
    assert_eq!(render(&duplicate), [0.0; 4]);
    let mut wrong_kind = make(Some(1));
    wrong_kind.nodes[0] = fill(1, WHITE);
    assert_eq!(render(&wrong_kind), [0.0; 4]);
    let mut clipped = make(Some(1));
    clipped.nodes[0].clip_to = Some(0);
    assert_eq!(render(&clipped), [0.0; 4]);
}

#[test]
fn styled_raster_background_keeps_explicit_identity_through_lowering() {
    let mut background = pixels(1, WHITE);
    background.content = NodeContent::StyledGroup {
        children: vec![pixels(1, WHITE)],
        clip_source: Box::new(pixels(1, WHITE)),
        effect_mask: None,
    };
    assert_eq!(
        render(&tree(
            vec![
                background,
                fill(2, RED),
                knockout(fill(3, BLUE), Knockout::Deep, 1.0, 0.0)
            ],
            Some(1)
        )),
        WHITE
    );
}

#[test]
fn pass_through_shallow_uses_entry_and_deep_inherits_explicit_stop() {
    for (mode, expected) in [(Knockout::Shallow, RED), (Knockout::Deep, WHITE)] {
        let source = knockout(fill(5, BLUE), mode, 1.0, 0.0);
        let nested = group(6, vec![fill(4, GREEN), source], BlendMode::PassThrough);
        let outer = group(7, vec![nested], BlendMode::PassThrough);
        assert_eq!(
            render(&tree(vec![pixels(1, WHITE), fill(2, RED), outer], Some(1))),
            expected
        );
    }
}

#[test]
fn isolation_resets_deep_even_inside_nested_pass_through() {
    let source = knockout(fill(5, BLUE), Knockout::Deep, 1.0, 0.0);
    let inner = group(6, vec![fill(4, GREEN), source], BlendMode::PassThrough);
    let isolated = group(7, vec![inner], BlendMode::Normal);
    assert_eq!(
        render(&tree(
            vec![pixels(1, WHITE), fill(2, RED), isolated],
            Some(1)
        )),
        RED
    );
}

#[test]
fn group_own_knockout_resolves_in_parent_scope() {
    let source = knockout(
        group(3, vec![fill(4, BLUE)], BlendMode::Normal),
        Knockout::Deep,
        0.5,
        0.5,
    );
    assert_eq!(
        raw16(&tree(vec![pixels(1, RED), fill(2, GREEN), source], Some(1))),
        [3334, 14027, 3334, 65535]
    );
}

#[test]
fn pass_through_and_derived_rectangle_crossfades_use_encoded_endpoints() {
    let mut pass = group(3, vec![fill(2, BLUE)], BlendMode::PassThrough);
    pass.opacity = 0.5;
    close(
        render(&tree(vec![fill(1, GREEN), pass.clone()], None)),
        encoded([0.0, 0.5, 0.5], 1.0),
    );
    pass.mask = Some(Arc::new(Mask::empty(1, 1, 128)));
    pass.clip_rect = Some([0.0, 0.0, 0.5, 1.0]);
    let k = 32.0 / 255.0;
    close(
        render(&tree(vec![fill(1, GREEN), pass], None)),
        encoded([0.0, 1.0 - k, k], 1.0),
    );
    let mut clipped = fill(3, [0.0; 4]);
    clipped.clip_rect = Some([0.0, 0.0, 0.5, 1.0]);
    clipped.content = NodeContent::ClippedGroup {
        children: vec![fill(2, BLUE)],
        baseline: vec![],
    };
    close(
        render(&tree(vec![fill(1, GREEN), clipped], None)),
        encoded([0.0, 0.5, 0.5], 1.0),
    );
}

#[test]
fn pass_through_nondefault_envelope_uses_real_backdrop_and_channels() {
    let mut member = fill(2, BLUE);
    member.blend = BlendMode::Multiply;
    let mut pass = group(3, vec![member], BlendMode::PassThrough);
    pass.blending.fill_opacity = 0.5;
    // Multiply against green produces black; an isolated path would paint blue.
    close(
        render(&tree(vec![fill(1, GREEN), pass.clone()], None)),
        encoded([0.0, 0.5, 0.0], 1.0),
    );
    pass.blending.channels = [true, false, true];
    assert_eq!(render(&tree(vec![fill(1, GREEN), pass], None)), GREEN);
}

#[test]
fn source_only_clip_shape_cannot_inherit_document_background() {
    let mut base = group(
        4,
        vec![
            fill(3, GREEN),
            knockout(fill(5, BLUE), Knockout::Deep, 1.0, 0.0),
        ],
        BlendMode::PassThrough,
    );
    base.blending.blend_clipped_layers_as_group = false;
    let mut clipped = fill(6, RED);
    clipped.clip_to = Some(1);
    // Real pass-through reveals white. The independent source-only shape is
    // transparent after Deep, so the clipped red layer must have zero coverage.
    assert_eq!(
        render(&tree(vec![pixels(1, WHITE), base, clipped], Some(1))),
        WHITE
    );
}

#[test]
fn grouped_clipping_runs_profile_for_base_members_and_final_envelope() {
    let mut base = fill(2, [0.0, 0.5, 0.0, 0.5]);
    base.opacity = 0.5;
    let mut clipped = fill(3, [0.0, 0.0, 0.5, 0.5]);
    clipped.clip_to = Some(1);
    // Normalized internal source is half green/half blue, then .25 total alpha.
    close(
        render(&tree(vec![fill(1, RED), base, clipped], None)),
        encoded([0.75, 0.125, 0.125], 1.0),
    );
    let mut base = group(
        2,
        vec![fill(4, GREEN), fill(5, [0.0, 0.0, 0.5, 0.5])],
        BlendMode::Normal,
    );
    base.opacity = 0.5;
    let mut clipped = fill(3, [0.0; 4]);
    clipped.clip_to = Some(1);
    close(
        render(&tree(vec![fill(1, RED), base, clipped], None)),
        encoded([0.5, 0.25, 0.25], 1.0),
    );
}

#[test]
fn styled_source_recovery_and_effect_mask_use_encoded_coordinates() {
    let mut effect = fill(2, [0.0, 0.0, 0.0, 0.5]);
    effect.blend = BlendMode::Multiply;
    let mut styled = fill(3, [0.0; 4]);
    styled.opacity = 0.5;
    styled.content = NodeContent::StyledGroup {
        children: vec![effect],
        clip_source: Box::new(fill(4, WHITE)),
        effect_mask: None,
    };
    close(
        render(&tree(vec![fill(1, WHITE), styled.clone()], None)),
        encoded([0.75; 3], 1.0),
    );
    styled.blending.layer_mask_hides_effects = true;
    if let NodeContent::StyledGroup { effect_mask, .. } = &mut styled.content {
        *effect_mask = Some(Box::new(knockout(fill(5, WHITE), Knockout::Deep, 1.0, 0.0)));
    }
    // An effect-mask source-only knockout cannot borrow the white stop.
    assert_eq!(
        render(&tree(vec![pixels(1, WHITE), styled], Some(1))),
        WHITE
    );
}

#[test]
fn adjustment_inputs_stay_linear_but_partial_coverage_is_encoded() {
    let mut adjustment = fill(2, [0.0; 4]);
    adjustment.content = NodeContent::Adjust(Arc::new(Prepared::Threshold(0.5)));
    adjustment.opacity = 0.5;
    // Threshold(0.5) turns opaque red black; opacity mixes red/black encoded.
    close(
        render(&tree(vec![fill(1, RED), adjustment.clone()], None)),
        encoded([0.5, 0.0, 0.0], 1.0),
    );
    adjustment.blending.channels = [false, true, true];
    assert_eq!(render(&tree(vec![fill(1, RED), adjustment], None)), RED);
}

#[test]
fn byte_split_ramps_have_inclusive_independent_endpoint_oracles() {
    let black = BlendRange {
        black: 10.0 / 255.0,
        black_fade: 13.0 / 255.0,
        ..Default::default()
    };
    let white = BlendRange {
        white_fade: 20.0 / 255.0,
        white: 23.0 / 255.0,
        ..Default::default()
    };
    for (v, want) in [
        (9., 0.),
        (10., 0.25),
        (11., 0.5),
        (12., 0.75),
        (13., 1.),
        (14., 1.),
    ] {
        assert_eq!(range_coverage(black, v), want);
    }
    for (v, want) in [
        (19., 1.),
        (20., 1.),
        (21., 0.75),
        (22., 0.5),
        (23., 0.25),
        (24., 0.),
    ] {
        assert_eq!(range_coverage(white, v), want);
    }
    let joined = BlendRange {
        black: 10. / 255.,
        black_fade: 10. / 255.,
        white_fade: 23. / 255.,
        white: 23. / 255.,
    };
    assert_eq!(range_coverage(joined, 9.), 0.);
    assert_eq!(range_coverage(joined, 10.), 1.);
    assert_eq!(range_coverage(joined, 23.), 1.);
    assert_eq!(range_coverage(joined, 24.), 0.);
    // Legacy range behavior is intentionally untouched.
    assert_eq!(black.coverage(10.0 / 255.0), 0.0);
}

#[test]
fn blend_if_gray_rounding_and_transparent_underlying_are_versioned() {
    assert_eq!(blend_if_value(BlendIfChannel::Gray, BLUE), 28.0);
    assert_eq!(blend_if_value(BlendIfChannel::Gray, GREEN), 150.0);
    let gate = BlendIf {
        channel: BlendIfChannel::Blue,
        backdrop: BlendRange {
            black: 0.5,
            black_fade: 0.5,
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(blend_if_gate(gate, BLUE, [0.0; 4]), 1.0);
    assert_eq!(blend_if_gate(gate, BLUE, [0.25, 0.0, 0.0, 0.25]), 0.75);
    assert_eq!(blend_if_gate(gate, BLUE, RED), 0.0);
}

#[test]
fn blend_if_gates_knockout_once_in_the_new_profile() {
    let mut source = knockout(fill(3, BLUE), Knockout::Deep, 1.0, 0.5);
    source.blending.blend_if = BlendIf {
        channel: BlendIfChannel::Green,
        backdrop: BlendRange {
            white_fade: 254.0 / 255.0,
            white: 1.0,
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(
        raw16(&tree(vec![pixels(1, RED), fill(2, GREEN), source], Some(1))),
        [3334, 14027, 3334, 65535]
    );
}

#[test]
fn storage_conversion_unpremultiplies_before_transfer() {
    let value = encoded([0.2, 0.1, 0.05], 0.25);
    close(encode_premul(value), [0.2, 0.1, 0.05, 0.25]);
    close(decode_premul([0.2, 0.1, 0.05, 0.25]), value);
    assert_eq!(encode_premul([0.2, 0.4, 0.6, 0.0]), [0.0; 4]);
    assert_eq!(decode_premul([0.2, 0.4, 0.6, 0.0]), [0.0; 4]);
    assert_eq!(photoshop_mix(value, RED, 0.0), value);
    assert_eq!(photoshop_mix(value, RED, 1.0), RED);
}

#[test]
fn semitransparent_source_and_destination_use_encoded_premultiplied_alpha() {
    let dst = encoded([0.125, 0.25, 0.0625], 0.5);
    let src = encoded([0.2, 0.1, 0.05], 0.25);
    close(
        blend_px(BlendMode::Normal, PS, dst, src, 0.0),
        encoded([0.29375, 0.2875, 0.096875], 0.625),
    );
    // as*((1-ab)*Cs + ab*(Cb*Cs)) + (1-as)*ab*Cb
    close(
        blend_px(BlendMode::Multiply, PS, dst, src, 0.0),
        encoded([0.21875, 0.2625, 0.075], 0.625),
    );
}

#[test]
fn special_fill_kernel_and_outer_coverage_use_encoded_coordinates() {
    let dst = encoded([0.2, 0.1, 0.05], 0.5);
    let src = encoded([0.3, 0.1, 0.2], 0.5);
    // Linear Dodge kernel: Cb + .5*Cs = [.7,.3,.3]. q=.25, ab=.5.
    // Encoded result q*((1-ab)*f*Cs+ab*kernel)+(1-q)*D.
    close(
        blend_px_fill(BlendMode::LinearDodge, PS, dst, src, 0.5, 0.5),
        encoded([0.275, 0.125, 0.1], 0.5625),
    );
    assert_eq!(
        blend_px_fill(BlendMode::LinearDodge, PS, dst, src, 0.5, 0.0),
        dst
    );
}

#[test]
fn dissolve_preserves_binary_alpha_threshold_and_deterministic_noise() {
    let dst = encoded([0.2, 0.1, 0.05], 0.5);
    let src = [0.0, 0.0, 0.25, 0.25];
    assert_eq!(blend_px(BlendMode::Dissolve, PS, dst, src, 0.25), dst);
    assert_eq!(blend_px(BlendMode::Dissolve, PS, dst, src, 0.249), BLUE);
    let mut source = fill(42, src);
    source.blend = BlendMode::Dissolve;
    let tree = tree(vec![fill(1, dst), source], None);
    let pixel = render(&tree);
    let expected = if dissolve_noise(0, 0, 42) >= 0.25 {
        dst
    } else {
        BLUE
    };
    assert_eq!(pixel, expected);
    assert_eq!(pixel, render(&tree));
}

#[test]
fn cpu_public_region_flatten_and_tile_edges_share_profile_dispatch() {
    let mut tree = tree(vec![fill(1, GREEN), fill(2, [0.0, 0.0, 0.5, 0.5])], None);
    tree.width = 257;
    tree.height = 3;
    let cpu = render_tile_cpu(&tree, 0, TileCoord::new(1, 0));
    assert_eq!(cpu[1], [0.0; 4]);
    assert_eq!(cpu[3 * TILE as usize], [0.0; 4]);
    assert_eq!(render_tile(&tree, 0, TileCoord::new(1, 0)), cpu);
    assert_eq!(region(&tree, IRect::new(256, 0, 1, 1)), vec![cpu[0]]);
    assert_eq!(flatten(&tree, 0).get(256, 0), color::f_to_px(cpu[0]));
    let mip = render_tile_cpu(&tree, 1, TileCoord::new(0, 0));
    close(mip[128], encoded([0.0, 0.5, 0.5], 1.0));
    assert_eq!(mip[129], [0.0; 4]);
    assert_eq!(mip[2 * TILE as usize], [0.0; 4]);
}

#[test]
fn mip_generation_and_bilinear_sampling_still_average_linear_storage() {
    let mut node = pixels(1, WHITE);
    node.content = NodeContent::Pixels {
        raster: Arc::new(Raster::from_fn(4, 4, [0; 4], |x, _| {
            if x % 2 == 0 {
                [0, 0, 0, 65535]
            } else {
                [65535; 4]
            }
        }))
        .into(),
        placement: Placement::default(),
    };
    let mut scene = tree(vec![node], None);
    scene.width = 4;
    scene.height = 4;
    assert_eq!(
        color::f_to_px(render_tile_cpu(&scene, 1, TileCoord::new(0, 0))[0]),
        [32768, 32768, 32768, 65535]
    );
    if let NodeContent::Pixels { placement, .. } = &mut scene.nodes[0].content {
        placement.x = 0.5;
    }
    close(
        render_tile_cpu(&scene, 0, TileCoord::new(0, 0))[1],
        [0.5, 0.5, 0.5, 1.0],
    );
}

#[test]
fn adjustment_secondary_blend_if_crossfade_is_encoded_and_applied_once() {
    let mut adjustment = fill(2, [0.0; 4]);
    adjustment.content = NodeContent::Adjust(Arc::new(Prepared::Threshold(0.5)));
    adjustment.opacity = 0.6;
    adjustment.blending.blend_if = BlendIf {
        channel: BlendIfChannel::Red,
        source: BlendRange {
            black: 101.0 / 255.0,
            black_fade: 104.0 / 255.0,
            ..Default::default()
        },
        ..Default::default()
    };
    // First crossfade is encoded red=.4 (byte 102). Its source gate is 2/4,
    // making the final encoded red .7. Both interpolations keep alpha fixed.
    close(
        render(&tree(vec![fill(1, RED), adjustment], None)),
        encoded([0.7, 0.0, 0.0], 1.0),
    );
}

#[test]
fn styled_partial_source_recovery_preserves_fractional_backdrop_alpha() {
    let backdrop = encoded([0.2, 0.3, 0.4], 0.5);
    let effect_color = encoded([0.4, 0.2, 0.1], 0.5);
    let mut effect = fill(2, effect_color);
    effect.blend = BlendMode::Multiply;
    let mut styled = fill(3, [0.0; 4]);
    styled.opacity = 0.5;
    styled.content = NodeContent::StyledGroup {
        children: vec![effect],
        clip_source: Box::new(fill(4, WHITE)),
        effect_mask: None,
    };
    // Full Multiply is encoded premul [.38,.31,.29], alpha .75.
    // Half layer envelope interpolates from [.2,.3,.4], alpha .5.
    close(
        render(&tree(vec![fill(1, backdrop), styled], None)),
        encoded([0.29, 0.305, 0.345], 0.625),
    );
}

#[test]
fn fractional_stop_and_source_shape_have_separate_native_analytic_contracts() {
    let mut background = pixels(1, RED);
    background.opacity = 0.5;
    let source = knockout(fill(3, BLUE), Knockout::Deep, 0.5, 0.5);
    close(
        render(&tree(vec![background, fill(2, GREEN), source], Some(1))),
        encoded([0.125, 0.5, 0.25], 0.875),
    );
    let source = knockout(fill(3, [0.0, 0.0, 0.25, 0.25]), Knockout::Deep, 0.5, 0.5);
    close(
        render(&tree(vec![pixels(1, RED), fill(2, GREEN), source], Some(1))),
        encoded([0.0625, 0.875, 0.0625], 1.0),
    );
}

#[test]
fn pass_through_envelope_covers_a_deep_punch_once() {
    let mut pass = group(
        4,
        vec![
            fill(3, GREEN),
            knockout(fill(5, BLUE), Knockout::Deep, 1.0, 0.0),
        ],
        BlendMode::PassThrough,
    );
    pass.opacity = 0.5;
    close(
        render(&tree(vec![pixels(1, WHITE), fill(2, RED), pass], Some(1))),
        encoded([1.0, 0.5, 0.5], 1.0),
    );
}

/// Mirror core's no-op styled-Raster lowering: Fill belongs to its content,
/// while the wrapper owns knockout/opacity and retains an unfilled clip source.
fn styled_raster(node: CompositeNode) -> CompositeNode {
    let mut clip_source = node.clone();
    clip_source.opacity = 1.0;
    clip_source.blend = BlendMode::Normal;
    clip_source.blending = BlendingOptions::default();
    clip_source.clip_rect = None;
    let mut content = clip_source.clone();
    content.blending.fill_opacity = node.blending.fill_opacity;
    let mut wrapper = node;
    wrapper.mask = None;
    wrapper.blending.fill_opacity = 1.0;
    wrapper.content = NodeContent::StyledGroup {
        children: vec![content],
        clip_source: Box::new(clip_source),
        effect_mask: None,
    };
    wrapper
}

#[test]
fn styled_bounds_knockout_matches_unstyled_extent_and_authored_envelope() {
    for x in [2.0, 1.5] {
        for mask in [None, Some(128)] {
            for rectangle in [None, Some([2.0, 0.0, 0.5, 1.0])] {
                let mut background = pixels(1, WHITE);
                background.content = NodeContent::Pixels {
                    raster: Arc::new(Raster::solid(6, 1, WHITE)).into(),
                    placement: Placement::default(),
                };
                let mut source =
                    knockout(pixels(3, [0.0, 0.0, 0.25, 0.25]), Knockout::Deep, 0.5, 0.5);
                source.blending.transparency_shapes_layer = false;
                source.mask = mask.map(|value| Arc::new(Mask::empty(1, 1, value)));
                source.clip_rect = rectangle;
                if let NodeContent::Pixels { placement, .. } = &mut source.content {
                    placement.x = x;
                }
                let mut plain = tree(
                    vec![background.clone(), fill(2, RED), source.clone()],
                    Some(1),
                );
                plain.width = 6;
                let mut styled = tree(
                    vec![background, fill(2, RED), styled_raster(source)],
                    Some(1),
                );
                styled.width = 6;
                let plain = render_tile_cpu(&plain, 0, TileCoord::new(0, 0));
                let styled = render_tile_cpu(&styled, 0, TileCoord::new(0, 0));
                for pixel in 0..6 {
                    close(styled[pixel], plain[pixel]);
                }
                // The original bug punched the entire row even without a
                // rectangle. Raster bounds leave these outer pixels untouched.
                for pixel in [0, 3, 4, 5] {
                    assert_eq!(
                        styled[pixel], RED,
                        "x={x}, mask={mask:?}, rect={rectangle:?}"
                    );
                }
            }
        }
    }
}

fn wide_background(color: [f32; 4]) -> CompositeNode {
    let mut background = pixels(1, color);
    background.content = NodeContent::Pixels {
        raster: Arc::new(Raster::solid(256, 1, color)).into(),
        placement: Placement::default(),
    };
    background
}

#[test]
fn dissolve_knockout_equals_ordinary_dissolve_when_destination_is_stop() {
    for (alpha, opacity, fill_amount, mask) in [
        (1.0, 0.5, 1.0, None),
        (0.5, 1.0, 1.0, None),
        (1.0, 1.0, 1.0, Some(128)),
        (0.5, 0.6, 0.5, Some(128)),
        (1.0, 0.5, 0.0, Some(128)),
    ] {
        for transparency_shapes in [true, false] {
            let mut source = fill(43, [0.0, 0.0, alpha, alpha]);
            source.blend = BlendMode::Dissolve;
            source.opacity = opacity;
            source.blending.fill_opacity = fill_amount;
            source.blending.transparency_shapes_layer = transparency_shapes;
            source.mask = mask.map(|value| Arc::new(Mask::empty(256, 1, value)));
            source.clip_rect = Some([0.0, 0.0, 255.5, 1.0]);
            let mut scene = tree(vec![wide_background(GREEN), source], Some(1));
            scene.width = 256;
            let ordinary = render_tile_cpu(&scene, 0, TileCoord::new(0, 0));
            for knockout in [Knockout::Shallow, Knockout::Deep] {
                scene.nodes[1].blending.knockout = knockout;
                let actual = render_tile_cpu(&scene, 0, TileCoord::new(0, 0));
                assert_eq!(
                    actual, ordinary,
                    "alpha={alpha}, opacity={opacity}, fill={fill_amount}, mask={mask:?}, transparency_shapes={transparency_shapes}, knockout={knockout:?}"
                );
                for (x, pixel) in actual.iter().enumerate().take(256) {
                    let rectangle = if x == 255 { 0.5 } else { 1.0 };
                    let authored_mask = mask.map_or(1.0, |m| m as f32 / 255.0);
                    let a = alpha * authored_mask * rectangle * opacity * fill_amount;
                    let expected = if dissolve_noise(x as i32, 0, 43) < a {
                        BLUE
                    } else {
                        GREEN
                    };
                    assert_eq!(*pixel, expected);
                }
            }
        }
    }
}

#[test]
fn dissolve_knockout_uses_binary_nested_punch_and_paint_events() {
    for transparency_shapes in [true, false] {
        for fill_amount in [0.0, 0.5, 1.0] {
            let mut source = knockout(
                fill(43, [0.0, 0.0, 0.5, 0.5]),
                Knockout::Deep,
                0.5,
                fill_amount,
            );
            source.blend = BlendMode::Dissolve;
            source.blending.transparency_shapes_layer = transparency_shapes;
            source.mask = Some(Arc::new(Mask::empty(256, 1, 128)));
            let mut scene = tree(vec![wide_background(GREEN), fill(2, RED), source], Some(1));
            scene.width = 256;
            let actual = render_tile_cpu(&scene, 0, TileCoord::new(0, 0));
            let q = (if transparency_shapes { 0.5 } else { 1.0 }) * (128.0 / 255.0) * 0.5;
            let a = 0.5 * (128.0 / 255.0) * 0.5 * fill_amount;
            let mut count = [0; 3];
            for (x, pixel) in actual[..256].iter().enumerate() {
                let noise = dissolve_noise(x as i32, 0, 43);
                let outcome = if noise < a {
                    2
                } else if noise < q {
                    1
                } else {
                    0
                };
                count[outcome] += 1;
                assert_eq!(*pixel, [RED, GREEN, BLUE][outcome]);
            }
            assert!(count[0] > 0);
            if a < q {
                assert!(count[1] > 0);
            }
            if a > 0.0 {
                assert!(count[2] > 0);
            }
        }
    }
}

#[test]
fn dissolve_knockout_threshold_is_exclusive_at_exact_coverage() {
    let mut source = knockout(
        fill(43, BLUE),
        Knockout::Deep,
        dissolve_noise(0, 0, 43),
        1.0,
    );
    source.blend = BlendMode::Dissolve;
    assert_eq!(
        render(&tree(vec![pixels(1, GREEN), source.clone()], Some(1))),
        GREEN
    );
    source.opacity += 1e-6;
    assert_eq!(render(&tree(vec![pixels(1, GREEN), source], Some(1))), BLUE);
}

#[test]
fn styled_knockout_encloses_extending_effect_and_fractional_authored_mask() {
    // A real lowered Multiply shadow extends one pixel beyond each side of a
    // one-pixel blue Raster. Its 128/255 authored mask is smaller than the
    // completed appearance alpha when effects are not hidden by that mask.
    // This is an explicit native one-envelope contract, not a Photoshop oracle.
    for shadow_alpha in [0.75, 1.0] {
        for hides_effects in [false, true] {
            let mut background = wide_background(GREEN);
            if let NodeContent::Pixels { raster, .. } = &mut background.content {
                *raster = Arc::new(Raster::solid(6, 1, GREEN)).into();
            }
            let mut source = knockout(pixels(3, BLUE), Knockout::Deep, 0.5, 1.0);
            source.blending.transparency_shapes_layer = false;
            source.blending.layer_mask_hides_effects = hides_effects;
            source.mask = Some(Arc::new(Mask::empty(1, 1, 128)));
            if let NodeContent::Pixels { placement, .. } = &mut source.content {
                placement.x = 2.0;
            }
            let mut appearance = styled_raster(source);
            if let NodeContent::StyledGroup {
                children,
                clip_source,
                effect_mask,
            } = &mut appearance.content
            {
                let mut shadow = pixels(4, [0.0, 0.0, 0.0, shadow_alpha]);
                shadow.content = NodeContent::Pixels {
                    raster: Arc::new(Raster::solid(3, 1, [0.0, 0.0, 0.0, shadow_alpha])).into(),
                    placement: Placement::at(1.0, 0.0),
                };
                shadow.blend = BlendMode::Multiply;
                if hides_effects {
                    // Core moves the authored mask from content to the whole
                    // appearance in this case; do not multiply it twice.
                    children[0].mask = None;
                    let mut mask = clip_source.as_ref().clone();
                    if let NodeContent::Pixels { raster, .. } = &mut mask.content {
                        *raster = Arc::new(Raster::solid(1, 1, WHITE)).into();
                    }
                    *effect_mask = Some(Box::new(mask));
                }
                children.insert(0, shadow);
            }
            let mut scene = tree(vec![background, fill(2, RED), appearance], Some(1));
            scene.width = 6;
            let actual = render_tile_cpu(&scene, 0, TileCoord::new(0, 0));
            let mask = 128.0 / 255.0;
            let stored_shadow_alpha = if shadow_alpha == 1.0 {
                1.0
            } else {
                49151.0 / 65535.0
            };
            let source_alpha = if hides_effects {
                mask
            } else {
                stored_shadow_alpha + (1.0 - stored_shadow_alpha) * mask
            };
            close(
                actual[2],
                encoded([1.0 - 0.5 * source_alpha, 0.0, 0.5 * mask], 1.0),
            );
            for x in [1, 3] {
                let red = if hides_effects {
                    1.0
                } else {
                    1.0 - 0.5 * stored_shadow_alpha
                };
                close(actual[x], encoded([red, 0.0, 0.0], 1.0));
            }
            for x in [0, 4, 5] {
                assert_eq!(actual[x], RED);
            }
            for pixel in &actual {
                assert!(
                    pixel[3] >= 0.0 && pixel[3] <= 1.0,
                    "invalid alpha: {pixel:?}"
                );
                for channel in &pixel[..3] {
                    assert!(
                        *channel >= 0.0 && *channel <= pixel[3],
                        "invalid premultiplication: {pixel:?}"
                    );
                }
            }
        }
    }
}
