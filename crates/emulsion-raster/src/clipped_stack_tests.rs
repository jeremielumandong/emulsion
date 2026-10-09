//! Algebraic clipping-stack oracles. Expected values are derived independently
//! from source-over and Multiply/Screen equations, not another compositor path.
use super::*;

fn fill(id: u64, rgba: [f32; 4]) -> CompositeNode {
    CompositeNode {
        id,
        visible: true,
        opacity: 1.0,
        blend: BlendMode::Normal,
        blending: BlendingOptions::default(),
        mask: None,
        clip_to: None,
        clip_rect: None,
        content: NodeContent::Fill(rgba),
    }
}

fn render(nodes: Vec<CompositeNode>) -> [f32; 4] {
    render_tile_cpu(
        &CompositeTree {
            width: 1,
            height: 1,
            space: BlendSpace::Linear,
            knockout_background: None,
            nodes,
        },
        0,
        TileCoord::new(0, 0),
    )[0]
}

fn close(actual: [f32; 4], expected: [f32; 4]) {
    for (a, b) in actual.into_iter().zip(expected) {
        assert!((a - b).abs() < 3e-5, "{actual:?} != {expected:?}");
    }
}

fn scene(base_alpha: f32, member_alpha: f32) -> Vec<CompositeNode> {
    let backdrop = fill(1, [0.5, 0.25, 0.75, 1.0]);
    let mut base = fill(
        2,
        [
            0.2 * base_alpha,
            0.4 * base_alpha,
            0.6 * base_alpha,
            base_alpha,
        ],
    );
    base.blend = BlendMode::Multiply;
    let mut member = fill(
        3,
        [
            0.8 * member_alpha,
            0.2 * member_alpha,
            0.4 * member_alpha,
            member_alpha,
        ],
    );
    member.clip_to = Some(1);
    vec![backdrop, base, member]
}

#[test]
fn grouped_clip_base_mode_applies_once_to_completed_stack() {
    // ON: Multiply(D, C), not Normal(C, Multiply(D, B)).
    close(render(scene(1.0, 1.0)), [0.4, 0.05, 0.3, 1.0]);
    let mut separate = scene(1.0, 1.0);
    separate[1].blending.blend_clipped_layers_as_group = false;
    close(render(separate), [0.8, 0.2, 0.4, 1.0]);
}

#[test]
fn grouped_clip_base_opacity_attenuates_completed_stack_once() {
    for (opacity, expected) in [
        (0.0, [0.5, 0.25, 0.75, 1.0]),
        (0.5, [0.45, 0.15, 0.525, 1.0]),
        (1.0, [0.4, 0.05, 0.3, 1.0]),
    ] {
        let mut nodes = scene(1.0, 1.0);
        nodes[1].opacity = opacity;
        close(render(nodes), expected);
    }
}

#[test]
fn grouped_clip_member_mode_is_preserved_inside_base_envelope() {
    let mut nodes = scene(1.0, 1.0);
    nodes[2].blend = BlendMode::Screen;
    // Screen(B,C)=(.84,.52,.76); Multiply(D, that)=(.42,.13,.57).
    close(render(nodes), [0.42, 0.13, 0.57, 1.0]);
}

#[test]
fn grouped_clip_fractional_base_shape_is_applied_only_once() {
    // G=.5B+.5C=(.5,.3,.5); D*(1-.5)+Multiply(D,G)*.5.
    close(render(scene(0.5, 0.5)), [0.375, 0.1625, 0.5625, 1.0]);
    let mut transparent = scene(0.5, 0.5);
    transparent[0].content = NodeContent::Fill([0.0; 4]);
    close(render(transparent), [0.25, 0.15, 0.25, 0.5]);
}

#[test]
fn grouped_clip_normal_mode_still_preserves_fractional_edge_alpha() {
    let mut nodes = scene(0.5, 0.5);
    nodes[0].content = NodeContent::Fill([0.0; 4]);
    nodes[1].blend = BlendMode::Normal;
    close(render(nodes), [0.25, 0.15, 0.25, 0.5]);
}

#[test]
fn grouped_clip_mask_and_rectangle_are_shape_not_repeated_member_opacity() {
    let mut nodes = scene(1.0, 0.5);
    nodes[1].mask = Some(Arc::new(Mask::empty(1, 1, 128)));
    let alpha = 128.0 / 255.0;
    close(
        render(nodes),
        [
            0.5 - 0.25 * alpha,
            0.25 - 0.175 * alpha,
            0.75 - 0.375 * alpha,
            1.0,
        ],
    );
    let mut nodes = scene(1.0, 0.5);
    nodes[1].clip_rect = Some([0.0, 0.0, 0.5, 1.0]);
    // Rectangle coverage is integrated at pixel centres by sample_rect.
    let coverage = sample_rect(
        [0.0, 0.0, 0.5, 1.0],
        Ctx {
            level: 0,
            scale: 1.0,
            ox: 0,
            oy: 0,
            space: BlendSpace::Linear,
            width: 1,
            height: 1,
        },
    )[0];
    close(
        render(nodes),
        [
            0.5 - 0.25 * coverage,
            0.25 - 0.175 * coverage,
            0.75 - 0.375 * coverage,
            1.0,
        ],
    );
}

#[test]
fn grouped_clip_uses_effective_pixel_source_and_placement() {
    let mut nodes = scene(0.5, 0.5);
    nodes[1].content = NodeContent::Pixels {
        raster: Arc::new(Raster::solid(1, 1, [0.1, 0.2, 0.3, 0.5])).into(),
        placement: Placement::default(),
    };
    close(render(nodes.clone()), [0.375, 0.1625, 0.5625, 1.0]);
    if let NodeContent::Pixels { placement, .. } = &mut nodes[1].content {
        placement.x = 2.0;
    }
    close(render(nodes), [0.5, 0.25, 0.75, 1.0]);
}

#[test]
fn grouped_clip_member_opacity_and_fill_remain_local() {
    let mut opacity = scene(0.5, 1.0);
    opacity[2].opacity = 0.5;
    close(render(opacity), [0.375, 0.1625, 0.5625, 1.0]);
    let mut fill = scene(0.5, 1.0);
    fill[2].blending.fill_opacity = 0.5;
    close(render(fill), [0.375, 0.1625, 0.5625, 1.0]);
}

#[test]
fn grouped_clip_zero_base_fill_preserves_unfilled_clip_shape() {
    let mut nodes = scene(0.5, 1.0);
    nodes[1].blending.fill_opacity = 0.0;
    // Compatibility invariant: Fill does not erase the base's clip shape.
    // This is deliberately not labelled a PSD reference golden for special Fill.
    close(render(nodes), [0.45, 0.15, 0.525, 1.0]);
}

#[test]
fn grouped_clip_chain_uses_root_across_hidden_middle() {
    let direct = scene(0.5, 0.5);
    let mut chained = direct.clone();
    let mut middle = fill(4, [1.0, 0.0, 0.0, 1.0]);
    middle.clip_to = Some(1);
    middle.opacity = 0.0;
    middle.visible = false;
    chained.insert(2, middle);
    chained[3].clip_to = Some(2);
    close(render(chained.clone()), render(direct));
    chained[1].visible = false;
    close(render(chained), [0.5, 0.25, 0.75, 1.0]);
}

#[test]
fn grouped_clip_adjustment_member_is_scoped_to_normalized_base() {
    let mut nodes = scene(0.5, 1.0);
    nodes[2].content = NodeContent::Adjust(Arc::new(
        crate::adjust::Adjustment::Exposure {
            exposure: 1.0,
            offset: 0.0,
            gamma: 1.0,
        }
        .prepare(),
    ));
    // Internal base doubles to (.4,.8,1); root alpha .5 applies once.
    close(render(nodes), [0.35, 0.225, 0.75, 1.0]);
}

#[test]
fn grouped_clip_is_recursive_inside_isolated_and_pass_through_groups() {
    for blend in [BlendMode::Normal, BlendMode::PassThrough] {
        let mut group = fill(5, [0.0; 4]);
        group.blend = blend;
        group.content = NodeContent::Group(scene(0.5, 0.5));
        close(render(vec![group]), [0.375, 0.1625, 0.5625, 1.0]);
    }
}

#[test]
fn grouped_clip_no_visible_members_preserves_standalone_special_fill() {
    let mut nodes = scene(0.5, 1.0);
    nodes[1].blend = BlendMode::ColorDodge;
    nodes[1].blending.fill_opacity = 0.5;
    nodes[2].visible = false;
    close(render(nodes.clone()), render(nodes[..2].to_vec()));
}

#[test]
fn grouped_clip_noncontiguous_links_do_not_reorder_unrelated_siblings() {
    let mut nodes = scene(1.0, 0.5);
    nodes.insert(2, fill(4, [0.0, 1.0, 0.0, 1.0]));
    // Preserve the existing generalized native-link order until a noncontiguous
    // group contract exists. The unrelated green layer remains below the clip.
    close(render(nodes), [0.4, 0.6, 0.2, 1.0]);
}

#[test]
fn grouped_clip_special_base_fill_is_not_changed_by_a_transparent_member() {
    for blend in BlendMode::MENU
        .iter()
        .flatten()
        .copied()
        .filter(|mode| mode.has_special_fill())
    {
        let mut nodes = scene(0.5, 0.0);
        nodes[1].blend = blend;
        nodes[1].blending.fill_opacity = 0.5;
        close(render(nodes.clone()), render(nodes[..2].to_vec()));
    }
}

#[test]
fn grouped_clip_knockout_root_is_not_changed_by_a_transparent_member() {
    for knockout in [Knockout::Shallow, Knockout::Deep] {
        for fill in [0.0, 0.5] {
            for opacity in [0.5, 1.0] {
                for masked in [false, true] {
                    let mut nodes = scene(1.0, 0.0);
                    nodes[1].blend = BlendMode::Normal;
                    nodes[1].blending.knockout = knockout;
                    nodes[1].blending.fill_opacity = fill;
                    nodes[1].opacity = opacity;
                    if masked {
                        nodes[1].mask = Some(Arc::new(Mask::empty(1, 1, 128)));
                    }
                    close(render(nodes.clone()), render(nodes[..2].to_vec()));
                }
            }
        }
    }
    let mut nodes = scene(1.0, 0.0);
    nodes[1].blending.knockout = Knockout::Deep;
    nodes[1].blending.fill_opacity = 0.0;
    close(render(nodes), [0.0; 4]);
}

#[test]
fn grouped_clip_member_knockout_preserves_legacy_appearance() {
    for knockout in [Knockout::Shallow, Knockout::Deep] {
        let mut nodes = scene(1.0, 0.5);
        nodes[2].blending.knockout = knockout;
        let mut independent = nodes.clone();
        independent[2].clip_to = None;
        close(render(nodes), render(independent));
    }
}

#[test]
fn grouped_clip_zero_root_alpha_cannot_add_color_or_coverage() {
    close(render(scene(0.0, 1.0)), [0.5, 0.25, 0.75, 1.0]);
    let mut nodes = scene(0.0, 1.0);
    nodes[0].content = NodeContent::Fill([0.0; 4]);
    close(render(nodes), [0.0; 4]);
}

#[test]
fn grouped_clip_root_and_member_masks_apply_in_distinct_scopes() {
    let mut nodes = scene(1.0, 1.0);
    nodes[1].mask = Some(Arc::new(Mask::empty(1, 1, 128)));
    nodes[2].mask = Some(Arc::new(Mask::empty(1, 1, 64)));
    let root = 128.0 / 255.0;
    let member = 64.0 / 255.0;
    let color = [0.2 + 0.6 * member, 0.4 - 0.2 * member, 0.6 - 0.2 * member];
    close(
        render(nodes),
        [
            0.5 * (1.0 - root) + 0.5 * color[0] * root,
            0.25 * (1.0 - root) + 0.25 * color[1] * root,
            0.75 * (1.0 - root) + 0.75 * color[2] * root,
            1.0,
        ],
    );
}

#[test]
fn grouped_clip_srgb_multiply_matches_independently_converted_constants() {
    let pixels = render_tile_cpu(
        &CompositeTree {
            width: 1,
            height: 1,
            space: BlendSpace::Srgb,
            knockout_background: None,
            nodes: scene(1.0, 1.0),
        },
        0,
        TileCoord::new(0, 0),
    );
    // IEC 61966-2-1 piecewise encode(D)*encode(C), then decode, calculated
    // independently in f64. Do not derive expected values through blend_px.
    close(pixels[0], [0.4017248, 0.055072315, 0.30234984, 1.0]);
}

fn isolated_group_scene(base_alpha: f32, member_alpha: f32) -> Vec<CompositeNode> {
    let mut nodes = scene(base_alpha, member_alpha);
    let mut child = fill(50, [0.0; 4]);
    child.content = nodes[1].content.clone();
    nodes[1].content = NodeContent::Group(vec![child]);
    nodes
}

#[test]
fn grouped_clip_isolated_group_root_uses_mode_opacity_and_fractional_shape_once() {
    for (opacity, alpha, member_alpha, expected) in [
        (1.0, 1.0, 1.0, [0.4, 0.05, 0.3, 1.0]),
        (0.5, 1.0, 1.0, [0.45, 0.15, 0.525, 1.0]),
        (0.0, 1.0, 1.0, [0.5, 0.25, 0.75, 1.0]),
        (1.0, 0.5, 0.5, [0.375, 0.1625, 0.5625, 1.0]),
        (1.0, 0.0, 1.0, [0.5, 0.25, 0.75, 1.0]),
    ] {
        let mut nodes = isolated_group_scene(alpha, member_alpha);
        nodes[1].opacity = opacity;
        close(render(nodes), expected);
    }
    let mut nodes = isolated_group_scene(0.5, 0.5);
    nodes[0].content = NodeContent::Fill([0.0; 4]);
    close(render(nodes), [0.25, 0.15, 0.25, 0.5]);
}

#[test]
fn grouped_clip_isolated_group_root_mask_is_applied_once_after_child_shape() {
    let mut nodes = isolated_group_scene(0.5, 0.5);
    nodes[1].mask = Some(Arc::new(Mask::empty(1, 1, 128)));
    // Root shape is child alpha(.5)*mask(128/255). Internal color is (.5,.3,.5).
    close(render(nodes), [0.4372549, 0.20607843, 0.65588235, 1.0]);
}

#[test]
fn grouped_clip_isolated_group_children_blend_against_transparency() {
    let mut nodes = isolated_group_scene(1.0, 0.5);
    let NodeContent::Group(children) = &mut nodes[1].content else {
        unreachable!()
    };
    children[0].blend = BlendMode::Multiply;
    // First child's Multiply sees transparent group backdrop, so retains B.
    // Half C over B is (.5,.3,.5); only the root then multiplies external D.
    close(render(nodes), [0.25, 0.075, 0.375, 1.0]);
}

#[test]
fn grouped_clip_isolated_group_multiple_child_blends_remain_internal() {
    let mut nodes = isolated_group_scene(1.0, 0.5);
    let NodeContent::Group(children) = &mut nodes[1].content else {
        unreachable!()
    };
    let mut screen = fill(51, [0.8, 0.2, 0.4, 1.0]);
    screen.blend = BlendMode::Screen;
    children.push(screen);
    // Child stack Screen(B,C)=(.84,.52,.76); the half-alpha clip produces
    // (.82,.36,.58). Root Multiply against D gives (.41,.09,.435).
    close(render(nodes), [0.41, 0.09, 0.435, 1.0]);
}

#[test]
fn grouped_clip_isolated_group_deep_knockout_descendant_keeps_existing_punch() {
    let mut nodes = isolated_group_scene(0.5, 0.0);
    let NodeContent::Group(children) = &mut nodes[1].content else {
        unreachable!()
    };
    children[0].blending.knockout = Knockout::Deep;
    children[0].blending.fill_opacity = 0.0;
    // The translucent child punches the external backdrop despite zero Fill.
    // This stack is deliberately compatibility-gated, never normalized.
    close(render(nodes), [0.25, 0.125, 0.375, 0.5]);
}

#[test]
fn grouped_clip_isolated_group_boundary_does_not_consume_adjacent_stack() {
    let mut nodes = isolated_group_scene(1.0, 1.0);
    let mut other = fill(5, [0.1, 0.2, 0.3, 0.5]);
    other.blend = BlendMode::Normal;
    other.content = NodeContent::Group(vec![fill(51, [0.1, 0.2, 0.3, 0.5])]);
    nodes.push(other);
    let mut top = fill(6, [0.0, 0.5, 0.0, 0.5]);
    top.clip_to = Some(3);
    nodes.push(top);
    // Second internal color=(.1,.7,.3), alpha1, external shape.5.
    // Normal over first stack's (.4,.05,.3) gives (.25,.375,.3).
    close(render(nodes), [0.25, 0.375, 0.3, 1.0]);
}

fn styled_content(child: CompositeNode) -> NodeContent {
    NodeContent::StyledGroup {
        clip_source: Box::new(child.clone()),
        children: vec![child],
        effect_mask: None,
    }
}

#[test]
fn grouped_clip_styled_member_preserves_legacy_until_effect_order_is_verified() {
    let mut nodes = scene(1.0, 1.0);
    nodes[2].content = styled_content(fill(70, [0.8, 0.2, 0.4, 1.0]));
    // Even a simple styled wrapper is deliberately outside the new contract.
    // Its outer/interior effects must not silently gain a normalized backdrop.
    close(render(nodes), [0.8, 0.2, 0.4, 1.0]);
}

#[test]
fn grouped_clip_nested_styled_appearance_is_also_compatibility_gated() {
    let mut root_style = isolated_group_scene(1.0, 1.0);
    let NodeContent::Group(children) = &mut root_style[1].content else {
        unreachable!()
    };
    children[0].content = styled_content(fill(71, [0.2, 0.4, 0.6, 1.0]));
    close(render(root_style), [0.8, 0.2, 0.4, 1.0]);

    let mut member_style = scene(1.0, 1.0);
    let mut child = fill(72, [0.0; 4]);
    child.content = styled_content(fill(73, [0.8, 0.2, 0.4, 1.0]));
    member_style[2].content = NodeContent::Group(vec![child]);
    close(render(member_style), [0.8, 0.2, 0.4, 1.0]);
}
