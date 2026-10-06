//! Versioned encoded-sRGB compositing with linear premultiplied storage.
//!
//! Keep this traversal separate from the legacy Deep-punch protocol. In this
//! profile knockout resolves against an explicit scope immediately; no punch
//! plane escapes an isolated group. Source sampling, mips, filters and public
//! tiles remain linear. Unproven Photoshop families still have deterministic
//! native semantics here, but must remain excluded from interchange claims.

use super::*;
use crate::blend::{decode_premul, encode_premul, photoshop_mix};

#[derive(Clone, Copy)]
pub(super) struct Scope<'a> {
    shallow: Option<&'a [[f32; 4]]>,
    deep: Option<&'a [[f32; 4]]>,
}

impl Scope<'_> {
    pub(super) const TRANSPARENT: Self = Self {
        shallow: None,
        deep: None,
    };

    fn stop(self, knockout: Knockout, index: usize) -> [f32; 4] {
        match knockout {
            Knockout::Shallow => self.shallow,
            Knockout::Deep => self.deep,
            Knockout::None => None,
        }
        .map_or([0.0; 4], |tile| tile[index])
    }
}

// IDs in derived shape/effect sources can repeat intentionally. Only real
// appearance nodes participate in Background identity validation.
fn appearance_id_count(nodes: &[CompositeNode], id: u64) -> usize {
    nodes
        .iter()
        .map(|node| {
            usize::from(node.id == id)
                + match &node.content {
                    NodeContent::Group(children)
                    | NodeContent::ClippedGroup { children, .. }
                    | NodeContent::StyledGroup { children, .. } => {
                        appearance_id_count(children, id)
                    }
                    _ => 0,
                }
        })
        .sum()
}

pub(super) fn render_root(tree: &CompositeTree, acc: &mut FTile, ctx: Ctx) {
    let background = tree.knockout_background.and_then(|id| {
        tree.nodes.first().filter(|node| {
            node.id == id
                && node.clip_to.is_none()
                && match &node.content {
                    NodeContent::Pixels { .. } | NodeContent::ProjectivePixels(_) => true,
                    // Styled raster lowering retains the authored ID on its
                    // wrapper and source. Do not mistake derived copies for
                    // duplicate authored identity, or admit arbitrary groups.
                    NodeContent::StyledGroup { clip_source, .. } => {
                        clip_source.id == id
                            && clip_source.clip_to.is_none()
                            && matches!(
                                clip_source.content,
                                NodeContent::Pixels { .. } | NodeContent::ProjectivePixels(_)
                            )
                    }
                    _ => false,
                }
                && appearance_id_count(&tree.nodes[1..], id) == 0
        })
    });
    let Some(background) = background else {
        // An invalid target is not permission to choose another layer.
        render_list(&tree.nodes, acc, ctx, Scope::TRANSPARENT);
        return;
    };
    let mut stop = Scratch::zeroed();
    render_list(
        std::slice::from_ref(background),
        &mut stop,
        ctx,
        Scope::TRANSPARENT,
    );
    let scope = Scope {
        shallow: Some(&stop),
        deep: Some(&stop),
    };
    // The target itself is rendered in transparent scope, even if its native
    // settings contain knockout. Later roots use its frozen appearance.
    render_list_impl(&tree.nodes, acc, ctx, scope, true);
}

/// All recursive source-only renders use this entry with TRANSPARENT scope.
/// No call from this module reaches the legacy traversal.
pub(super) fn render_list(nodes: &[CompositeNode], acc: &mut FTile, ctx: Ctx, scope: Scope<'_>) {
    render_list_impl(nodes, acc, ctx, scope, false);
}

fn render_list_impl(
    nodes: &[CompositeNode],
    acc: &mut FTile,
    ctx: Ctx,
    scope: Scope<'_>,
    background_first: bool,
) {
    let mut roots = vec![None; nodes.len()];
    for (i, node) in nodes.iter().enumerate() {
        if let Some(j) = node.clip_to.filter(|j| *j < i) {
            roots[i] = Some(roots[j].unwrap_or(j));
        }
    }
    let is_source: Vec<bool> = (0..nodes.len())
        .map(|i| {
            nodes
                .iter()
                .zip(&roots)
                .any(|(node, root)| node.visible && *root == Some(i))
        })
        .collect();
    let mut alphas: Vec<Option<Vec<f32>>> = vec![None; nodes.len()];
    let mut stacked_until = 0;
    for (i, node) in nodes.iter().enumerate() {
        if i < stacked_until || !node.visible {
            continue;
        }
        let node_scope = if background_first && i == 0 {
            Scope::TRANSPARENT
        } else {
            scope
        };
        if let Some(end) =
            clipped_stack::render_photoshop_stack(nodes, &roots, i, acc, ctx, node_scope)
        {
            stacked_until = end;
            continue;
        }
        let clip = match roots[i] {
            Some(j) if j < i => {
                if !nodes[j].visible {
                    continue;
                }
                // A missing source must fail closed, never become global paint.
                let mut alpha = alphas[j].clone().unwrap_or_else(|| vec![0.0; TILE_PX]);
                if !nodes[j].blending.blend_clipped_layers_as_group {
                    let scale = nodes[j].opacity * nodes[j].blending.fill_opacity;
                    alpha.iter_mut().for_each(|v| *v *= scale);
                }
                Some(alpha)
            }
            _ => None,
        };
        let rectangle = node.clip_rect.map(|rect| sample_rect(rect, ctx));
        let with_rectangle = |mask: Option<Vec<f32>>| -> Option<Vec<f32>> {
            match (mask, &rectangle) {
                (Some(mut mask), Some(rectangle)) => {
                    mask.iter_mut().zip(rectangle).for_each(|(a, b)| *a *= b);
                    Some(mask)
                }
                (None, Some(rectangle)) => Some(rectangle.clone()),
                (mask, None) => mask,
            }
        };
        let mask_doc = || {
            with_rectangle(
                node.mask
                    .as_ref()
                    .map(|mask| sample_mask(mask, &Placement::default(), ctx)),
            )
        };
        let coverage = |mask: Option<&[f32]>| -> Option<Vec<f32>> {
            if node.opacity >= 1.0 && mask.is_none() && clip.is_none() {
                return None;
            }
            let mut cov = vec![node.opacity; TILE_PX];
            if let Some(mask) = mask {
                cov.iter_mut().zip(mask).for_each(|(a, b)| *a *= b);
            }
            if let Some(clip) = &clip {
                cov.iter_mut().zip(clip).for_each(|(a, b)| *a *= b);
            }
            Some(cov)
        };
        match &node.content {
            NodeContent::Pixels { .. } | NodeContent::ProjectivePixels(_) => {
                let raster = pixel_raster(&node.content);
                let geometry = PixelGeometry::new(&node.content, ctx);
                let raster = raster.get();
                let mut source = Scratch::zeroed();
                let sampled = geometry.raster(&mut source, raster, ctx);
                let mut shape = if node.blending.knockout != Knockout::None
                    && !node.blending.transparency_shapes_layer
                {
                    Some(geometry.shape(raster, ctx))
                } else {
                    None
                };
                if !sampled && shape.is_none() {
                    if is_source[i] {
                        alphas[i] = Some(vec![0.0; TILE_PX]);
                    }
                    continue;
                }
                let mask = with_rectangle(node.mask.as_ref().map(|mask| geometry.mask(mask, ctx)));
                apply_mask(&mut source, mask.as_deref());
                // Bounds-shape and paint each receive authored mask/rectangle
                // once. Opacity and clipping remain outside both source planes.
                if let (Some(shape), Some(mask)) = (&mut shape, &mask) {
                    shape.iter_mut().zip(mask).for_each(|(a, b)| *a *= b);
                }
                if is_source[i] {
                    alphas[i] = Some(source.iter().map(|p| p[3]).collect());
                }
                composite_into(
                    acc,
                    &source,
                    coverage(None).as_deref(),
                    shape.as_deref(),
                    node,
                    ctx,
                    node_scope,
                );
            }
            NodeContent::Fill(color) => {
                let mask = mask_doc();
                let mut source = vec![*color; TILE_PX];
                apply_mask(&mut source, mask.as_deref());
                if is_source[i] {
                    alphas[i] = Some(source.iter().map(|p| p[3]).collect());
                }
                // No linear fill_over bypass in this profile. Bounds shape is
                // the whole fill frame, covered by the same authored envelope.
                composite_into(
                    acc,
                    &source,
                    coverage(None).as_deref(),
                    mask.as_deref(),
                    node,
                    ctx,
                    node_scope,
                );
            }
            NodeContent::ClippedGroup { children, baseline } => {
                let mut full = acc.clone();
                render_list(children, &mut full, ctx, node_scope);
                render_list(baseline, acc, ctx, node_scope);
                let rect = rectangle.as_ref().expect("clipped content has a rectangle");
                for ((before, after), coverage) in acc.iter_mut().zip(&full).zip(rect) {
                    *before = photoshop_mix(*before, *after, *coverage);
                }
                if is_source[i] {
                    let mut source = Scratch::zeroed();
                    let mut baseline_source = Scratch::zeroed();
                    render_list(children, &mut source, ctx, Scope::TRANSPARENT);
                    render_list(baseline, &mut baseline_source, ctx, Scope::TRANSPARENT);
                    alphas[i] = Some(
                        source
                            .iter()
                            .zip(baseline_source.iter())
                            .zip(rect)
                            .map(|((a, b), k)| b[3] + (a[3] - b[3]) * k)
                            .collect(),
                    );
                }
            }
            NodeContent::Group(children) => {
                let mask = mask_doc();
                if node.blend == BlendMode::PassThrough && node.blending.knockout == Knockout::None
                {
                    let before = acc.clone();
                    let child_scope = Scope {
                        shallow: Some(&before),
                        deep: node_scope.deep,
                    };
                    render_list(children, acc, ctx, child_scope);
                    // Shape reads never inherit the surrounding Background.
                    let source = if is_source[i] || node.blending.blend_if != BlendIf::default() {
                        let mut source = Scratch::zeroed();
                        render_list(children, &mut source, ctx, Scope::TRANSPARENT);
                        Some(source)
                    } else {
                        None
                    };
                    if is_source[i] {
                        alphas[i] = Some(
                            source
                                .as_ref()
                                .expect("clip source was rendered")
                                .iter()
                                .enumerate()
                                .map(|(j, p)| p[3] * mask.as_ref().map_or(1.0, |mask| mask[j]))
                                .collect(),
                        );
                    }
                    let cov = coverage(mask.as_deref());
                    for (j, (after, before)) in acc.iter_mut().zip(before).enumerate() {
                        let gate = source.as_ref().map_or(1.0, |source| {
                            blend_if_gate(node.blending.blend_if, source[j], before)
                        });
                        let k = cov.as_ref().map_or(1.0, |cov| cov[j])
                            * node.blending.fill_opacity
                            * gate;
                        let mut out = photoshop_mix(before, *after, k);
                        restore_channels(&mut out, before, node.blending.channels);
                        *after = out;
                    }
                } else {
                    let mut source = Scratch::zeroed();
                    // Both Deep and Shallow reset at an isolation boundary.
                    render_list(children, &mut source, ctx, Scope::TRANSPARENT);
                    apply_mask(&mut source, mask.as_deref());
                    if is_source[i] {
                        alphas[i] = Some(source.iter().map(|p| p[3]).collect());
                    }
                    composite_into(
                        acc,
                        &source,
                        coverage(None).as_deref(),
                        mask.as_deref(),
                        node,
                        ctx,
                        node_scope,
                    );
                }
            }
            NodeContent::StyledGroup {
                children,
                clip_source,
                effect_mask,
            } => {
                let mut source = Scratch::zeroed();
                render_list(children, &mut source, ctx, Scope::TRANSPARENT);
                if children
                    .iter()
                    .any(|child| child.blend != BlendMode::Normal)
                {
                    let mut appearance = acc.clone();
                    render_list(children, &mut appearance, ctx, node_scope);
                    for ((source, rendered), backdrop) in
                        source.iter_mut().zip(appearance).zip(acc.iter())
                    {
                        let alpha = source[3];
                        let rendered = encode_premul(rendered);
                        let backdrop = encode_premul(*backdrop);
                        let mut recovered = [0.0, 0.0, 0.0, alpha];
                        for c in 0..3 {
                            recovered[c] =
                                (rendered[c] - backdrop[c] * (1.0 - alpha)).clamp(0.0, alpha);
                        }
                        *source = decode_premul(recovered);
                    }
                }
                if node.blending.layer_mask_hides_effects
                    && let Some(mask_node) = effect_mask
                {
                    let mut mask = Scratch::zeroed();
                    render_list(
                        std::slice::from_ref(mask_node.as_ref()),
                        &mut mask,
                        ctx,
                        Scope::TRANSPARENT,
                    );
                    for (pixel, mask) in source.iter_mut().zip(mask.iter()) {
                        pixel.iter_mut().for_each(|v| *v *= mask[3]);
                    }
                }
                apply_mask(&mut source, rectangle.as_deref());
                if is_source[i] {
                    let mut shape = Scratch::zeroed();
                    render_list(
                        std::slice::from_ref(clip_source.as_ref()),
                        &mut shape,
                        ctx,
                        Scope::TRANSPARENT,
                    );
                    alphas[i] = with_rectangle(Some(shape.iter().map(|p| p[3]).collect()));
                }
                let bounds_shape = (node.blending.knockout != Knockout::None
                    && !node.blending.transparency_shapes_layer)
                    .then(|| {
                        let mut shape = styled_bounds_shape(clip_source, ctx);
                        if let Some(rectangle) = &rectangle {
                            shape.iter_mut().zip(rectangle).for_each(|(a, b)| *a *= b);
                        }
                        // Explicit native styled-knockout contract: the whole
                        // completed appearance shares one enclosing envelope.
                        // Effects may expand the authored bounded punch shape,
                        // but only where they actually paint. Taking the local
                        // maximum (after each mask/rectangle once) ensures every
                        // painted sample lies inside q, so conditional alpha
                        // cannot exceed one or disappear at a zero bounds mask.
                        // This is not an Adobe per-effect knockout model; that
                        // requires effect/source decomposition and fixtures.
                        shape
                            .iter_mut()
                            .zip(source.iter())
                            .for_each(|(shape, pixel)| {
                                *shape = shape.max(pixel[3]);
                            });
                        shape
                    });
                composite_into(
                    acc,
                    &source,
                    coverage(None).as_deref(),
                    bounds_shape.as_deref(),
                    node,
                    ctx,
                    node_scope,
                );
            }
            NodeContent::Adjust(op) => {
                let mask = mask_doc();
                if is_source[i] {
                    alphas[i] = Some(mask.clone().unwrap_or_else(|| vec![1.0; TILE_PX]));
                }
                apply_adjustment(acc, op, coverage(mask.as_deref()).as_deref(), node, ctx);
            }
        }
    }
}

/// Styled raster lowering retains an unfilled, unit-envelope clip source.
/// Replace only its pixels with opaque bounds, retaining placement, authored
/// mask and source rectangle. Effects and Fill cannot expand this punch to the
/// whole tile. Source-only scope also prevents a shape read borrowing a stop.
fn styled_bounds_shape(clip_source: &CompositeNode, ctx: Ctx) -> Vec<f32> {
    let mut bounds = clip_source.clone();
    bounds.opacity = 1.0;
    bounds.blend = BlendMode::Normal;
    bounds.blending = BlendingOptions::default();
    bounds.clip_to = None;
    match &clip_source.content {
        NodeContent::Pixels { raster, placement } => {
            let (width, height) = raster.size();
            bounds.content = NodeContent::Pixels {
                raster: Arc::new(Raster::solid(width, height, [0.0, 0.0, 0.0, 1.0])).into(),
                placement: *placement,
            };
        }
        NodeContent::ProjectivePixels(_) => {
            return projective_bounds_shape(clip_source, ctx).expect("checked projective source");
        }
        NodeContent::Fill(_) => bounds.content = NodeContent::Fill([0.0, 0.0, 0.0, 1.0]),
        // Other styled source families have no established bounds contract.
        // Their source-only alpha is conservative; never default to tile-wide
        // coverage merely because the lowered source is not a raster.
        _ => {}
    }
    let mut shape = Scratch::zeroed();
    render_list(
        std::slice::from_ref(&bounds),
        &mut shape,
        ctx,
        Scope::TRANSPARENT,
    );
    shape.iter().map(|pixel| pixel[3]).collect()
}

fn apply_mask(source: &mut FTile, mask: Option<&[f32]>) {
    if let Some(mask) = mask {
        for (pixel, amount) in source.iter_mut().zip(mask) {
            pixel.iter_mut().for_each(|v| *v *= amount);
        }
    }
}

fn restore_channels(out: &mut [f32; 4], before: [f32; 4], channels: [bool; 3]) {
    if !channels.iter().any(|enabled| *enabled) {
        *out = before;
        return;
    }
    for c in 0..3 {
        if !channels[c] {
            out[c] = if before[3] > 0.0 {
                before[c] / before[3] * out[3]
            } else {
                0.0
            };
        }
    }
}

/// Punch coverage q and painted alpha a are deliberately distinct. Blend the
/// source conditional on the punch over the scope stop, then mix by q. Erasing
/// D followed by source-over would wrongly retain D by (1-q)*(1-a).
pub(super) fn composite_into(
    acc: &mut FTile,
    source: &FTile,
    coverage: Option<&[f32]>,
    bounds_shape: Option<&[f32]>,
    node: &CompositeNode,
    ctx: Ctx,
    scope: Scope<'_>,
) {
    for (index, (dst, source)) in acc.iter_mut().zip(source).enumerate() {
        let k = coverage.map_or(1.0, |coverage| coverage[index]);
        if k <= 0.0 || !node.blending.channels.iter().any(|enabled| *enabled) {
            continue;
        }
        let shape = if node.blending.transparency_shapes_layer {
            source[3]
        } else {
            bounds_shape.map_or(1.0, |shape| shape[index])
        };
        if source[3] <= 0.0 && (node.blending.knockout == Knockout::None || shape <= 0.0) {
            continue;
        }
        let effective = k * blend_if_gate(node.blending.blend_if, *source, *dst);
        if effective <= 0.0 {
            continue;
        }
        let noise = if node.blend == BlendMode::Dissolve {
            let x = ctx.ox + (index % TILE as usize) as i64;
            let y = ctx.oy + (index / TILE as usize) as i64;
            dissolve_noise(x as i32, y as i32, node.id ^ ((ctx.level as u64) << 56))
        } else {
            0.0
        };
        let before = *dst;
        let mut out = if node.blending.knockout == Knockout::None {
            paint(before, *source, effective, node, noise)
        } else {
            let q = (shape * effective).clamp(0.0, 1.0);
            if q <= 0.0 {
                continue;
            }
            if node.blend == BlendMode::Dissolve {
                // Native stochastic knockout contract: one shared noise value
                // selects nested punch (q) and paint (a) events. Choose D, B or
                // opaque C directly, rather than interpolating a binary sample.
                // When D == B this is exactly ordinary Dissolve, including
                // fractional shape/mask/Opacity and Fill. Fill zero still
                // punches with probability q. No Photoshop parity is implied.
                if noise >= q {
                    before
                } else {
                    paint(
                        scope.stop(node.blending.knockout, index),
                        *source,
                        effective,
                        node,
                        noise,
                    )
                }
            } else {
                let conditional = source.map(|value| value / shape);
                let stopped = paint(
                    scope.stop(node.blending.knockout, index),
                    conditional,
                    1.0,
                    node,
                    noise,
                );
                photoshop_mix(before, stopped, q)
            }
        };
        restore_channels(&mut out, before, node.blending.channels);
        *dst = out;
    }
}

fn paint(
    dst: [f32; 4],
    source: [f32; 4],
    coverage: f32,
    node: &CompositeNode,
    noise: f32,
) -> [f32; 4] {
    if node.blend.has_special_fill() {
        blend_px_fill(
            node.blend,
            BlendSpace::PhotoshopSrgbV1,
            dst,
            source,
            coverage,
            node.blending.fill_opacity,
        )
    } else {
        blend_px(
            node.blend,
            BlendSpace::PhotoshopSrgbV1,
            dst,
            source.map(|value| value * coverage * node.blending.fill_opacity),
            noise,
        )
    }
}

/// Byte-domain native contract based on independently published calibrated
/// endpoint tests. The combined Blend If family still needs isolated Photoshop
/// fixtures before interchange can claim exact Photoshop rendering parity.
fn range_coverage(range: BlendRange, value: f32) -> f32 {
    let [black, black_fade, white_fade, white] =
        [range.black, range.black_fade, range.white_fade, range.white]
            .map(|value| (value.clamp(0.0, 1.0) * 255.0).round());
    if value < black || value > white {
        return 0.0;
    }
    let low = if black_fade > black && value < black_fade {
        (value - black + 1.0) / (black_fade - black + 1.0)
    } else {
        1.0
    };
    let high = if white > white_fade && value > white_fade {
        (white - value + 1.0) / (white - white_fade + 1.0)
    } else {
        1.0
    };
    low * high
}

fn blend_if_value(channel: BlendIfChannel, pixel: [f32; 4]) -> f32 {
    let encoded = encode_premul(pixel);
    let rgb = if pixel[3] > 0.0 {
        [encoded[0], encoded[1], encoded[2]]
            .map(|value| (value / pixel[3] * 255.0).clamp(0.0, 255.0).round())
    } else {
        [0.0; 3]
    };
    match channel {
        BlendIfChannel::Gray => {
            ((299.0 * rgb[0] + 590.0 * rgb[1] + 111.0 * rgb[2]) / 1000.0).round()
        }
        BlendIfChannel::Red => rgb[0],
        BlendIfChannel::Green => rgb[1],
        BlendIfChannel::Blue => rgb[2],
    }
}

fn blend_if_gate(blend_if: BlendIf, source: [f32; 4], backdrop: [f32; 4]) -> f32 {
    if blend_if == BlendIf::default() {
        return 1.0;
    }
    let source_gate = range_coverage(blend_if.source, blend_if_value(blend_if.channel, source));
    let backdrop_gate = range_coverage(
        blend_if.backdrop,
        blend_if_value(blend_if.channel, backdrop),
    );
    let alpha = backdrop[3].clamp(0.0, 1.0);
    source_gate * ((1.0 - alpha) + alpha * backdrop_gate)
}

fn apply_adjustment(
    acc: &mut FTile,
    op: &Prepared,
    coverage: Option<&[f32]>,
    node: &CompositeNode,
    ctx: Ctx,
) {
    let positional = op.positional();
    for (index, pixel) in acc.iter_mut().enumerate() {
        let before = *pixel;
        let alpha = before[3];
        let k = coverage.map_or(1.0, |coverage| coverage[index]) * node.blending.fill_opacity;
        if alpha <= 0.0 || k <= 0.0 {
            continue;
        }
        let rgb = [before[0] / alpha, before[1] / alpha, before[2] / alpha];
        let adjusted = if positional {
            let x = (ctx.ox + (index % TILE as usize) as i64) << ctx.level;
            let y = (ctx.oy + (index / TILE as usize) as i64) << ctx.level;
            op.apply_at(rgb, x as i32, y as i32, ctx.width, ctx.height)
        } else {
            op.apply(rgb)
        };
        let target = match node.blend {
            BlendMode::Normal | BlendMode::PassThrough | BlendMode::Dissolve => adjusted,
            mode => {
                let target = blend_px(
                    mode,
                    BlendSpace::PhotoshopSrgbV1,
                    [rgb[0], rgb[1], rgb[2], 1.0],
                    [adjusted[0], adjusted[1], adjusted[2], 1.0],
                    0.0,
                );
                [target[0], target[1], target[2]]
            }
        };
        let adjusted = photoshop_mix(
            before,
            [
                target[0] * alpha,
                target[1] * alpha,
                target[2] * alpha,
                alpha,
            ],
            k,
        );
        let gate = blend_if_gate(node.blending.blend_if, adjusted, before);
        let mut out = photoshop_mix(before, adjusted, gate);
        restore_channels(&mut out, before, node.blending.channels);
        *pixel = out;
    }
}

#[cfg(test)]
#[path = "photoshop_composite_tests.rs"]
mod tests;
