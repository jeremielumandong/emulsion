//! The base layer's blend envelope belongs to a clipping stack, not to each
//! independently composited member. Keep shape, internal colour and final
//! opacity separate so antialiased edges do not gain alpha as members are added.

use super::*;

/// Whether this sibling list contains a visible, enabled clipping stack.
/// GPU compositors without a stack envelope must decline it, even when every
/// mode is Normal: fractional base alpha still has to be applied only once.
/// Callers walking a tree must also inspect nested sibling lists.
pub fn has_grouped_clipping(nodes: &[CompositeNode]) -> bool {
    grouped_clipping_fallback_reason(nodes).is_some()
}

fn has_knockout(node: &CompositeNode) -> bool {
    if node.blending.knockout != Knockout::None {
        return true;
    }
    match &node.content {
        NodeContent::Group(children) => children.iter().any(has_knockout),
        NodeContent::ClippedGroup { children, baseline } => {
            children.iter().chain(baseline).any(has_knockout)
        }
        NodeContent::StyledGroup {
            children,
            clip_source,
            effect_mask,
        } => {
            children.iter().any(has_knockout)
                || has_knockout(clip_source)
                || effect_mask.as_deref().is_some_and(has_knockout)
        }
        _ => false,
    }
}

fn has_styled_appearance(node: &CompositeNode) -> bool {
    match &node.content {
        NodeContent::StyledGroup { .. } => true,
        NodeContent::Group(children) => children.iter().any(has_styled_appearance),
        NodeContent::ClippedGroup { children, baseline } => {
            children.iter().chain(baseline).any(has_styled_appearance)
        }
        _ => false,
    }
}

fn stack_has_styled_appearance(
    nodes: &[CompositeNode],
    roots: &[Option<usize>],
    base: usize,
) -> bool {
    has_styled_appearance(&nodes[base])
        || nodes
            .iter()
            .zip(roots)
            .any(|(node, root)| *root == Some(base) && has_styled_appearance(node))
}

fn stack_has_knockout(nodes: &[CompositeNode], roots: &[Option<usize>], base: usize) -> bool {
    has_knockout(&nodes[base])
        || nodes
            .iter()
            .zip(roots)
            .any(|(node, root)| *root == Some(base) && has_knockout(node))
}

/// A capability diagnostic for the viewport. CPU fallback fixes ordinary
/// contiguous pixel/fill/isolated-group stacks; other combinations retain their
/// existing rendering until their shape/appearance contract is established.
/// This is intentionally explicit rather than advertising full Photoshop parity.
pub fn grouped_clipping_fallback_reason(nodes: &[CompositeNode]) -> Option<&'static str> {
    let mut roots = vec![None; nodes.len()];
    let mut grouped = vec![false; nodes.len()];
    for (i, node) in nodes.iter().enumerate() {
        if let Some(j) = node.clip_to.filter(|j| *j < i) {
            let root = roots[j].unwrap_or(j);
            roots[i] = Some(root);
            grouped[root] |= node.visible
                && nodes[root].visible
                && nodes[root].blending.blend_clipped_layers_as_group;
        }
    }
    let mut found = false;
    for (i, active) in grouped.into_iter().enumerate() {
        if !active {
            continue;
        }
        found = true;
        let root = &nodes[i];
        if stack_has_knockout(nodes, &roots, i) {
            return Some("grouped clipping uses CPU; knockout stacks retain legacy appearance");
        }
        if stack_has_styled_appearance(nodes, &roots, i) {
            return Some("grouped clipping uses CPU; styled stacks retain legacy appearance");
        }
        if matches!(root.content, NodeContent::Adjust(_)) {
            return Some(
                "grouped clipping uses CPU; adjustment-base grouping retains legacy appearance",
            );
        }
        if matches!(root.content, NodeContent::Group(_)) && root.blend == BlendMode::PassThrough {
            return Some(
                "grouped clipping uses CPU; pass-through base grouping retains legacy appearance",
            );
        }
        if matches!(root.content, NodeContent::ClippedGroup { .. }) {
            return Some(
                "grouped clipping uses CPU; derived group-base grouping retains legacy appearance",
            );
        }
        if root.blend.has_special_fill() && root.blending.fill_opacity != 1.0 {
            return Some(
                "grouped clipping uses CPU; nonlinear base Fill retains legacy appearance",
            );
        }
        if stack_end(nodes, &roots, i).is_none() {
            return Some(
                "grouped clipping uses CPU; noncontiguous links retain legacy layer order",
            );
        }
    }
    found.then_some("grouped clipping requires the CPU compositor")
}

/// Exclusive ends of the bounded clipping stacks implemented by both GPU
/// compositors. `None` means this sibling is not an active stack root. An error
/// requires CPU fallback for the entire composition, never per-member clipping.
/// Hidden siblings still participate in topology: an unrelated hidden layer
/// cannot make a noncontiguous native link into an ordinary clipping stack.
/// Callers must also check their own source, command, alpha-slot and depth limits.
pub fn gpu_clipping_stack_ends(
    nodes: &[CompositeNode],
) -> Result<Vec<Option<usize>>, &'static str> {
    fn simple(node: &CompositeNode, depth: usize, remaining: &mut usize) -> bool {
        if depth >= 16 || *remaining == 0 || node.blending != BlendingOptions::default() {
            return false;
        }
        *remaining -= 1;
        match &node.content {
            NodeContent::Pixels { .. }
            | NodeContent::ProjectivePixels(_)
            | NodeContent::Fill(_) => true,
            NodeContent::Group(children) => {
                node.blend != BlendMode::PassThrough
                    && children
                        .iter()
                        .all(|child| simple(child, depth + 1, remaining))
            }
            _ => false,
        }
    }
    let mut roots = vec![None; nodes.len()];
    let mut active = vec![false; nodes.len()];
    for (i, node) in nodes.iter().enumerate() {
        if let Some(j) = node.clip_to.filter(|j| *j < i) {
            let root = roots[j].unwrap_or(j);
            roots[i] = Some(root);
            active[root] |= node.visible
                && nodes[root].visible
                && nodes[root].blending.blend_clipped_layers_as_group;
        }
    }
    let mut ends = vec![None; nodes.len()];
    for (base, active) in active.into_iter().enumerate() {
        if !active {
            continue;
        }
        let mut end = base + 1;
        while end < nodes.len() && roots[end] == Some(base) {
            end += 1;
        }
        if end == base + 1 || roots[end..].contains(&Some(base)) {
            return Err("grouped clipping uses CPU; noncontiguous links retain legacy layer order");
        }
        // Bound recursion before the CPU compatibility check visits descendants.
        let mut remaining = 64;
        if !nodes[base..end]
            .iter()
            .all(|node| simple(node, 0, &mut remaining))
        {
            return Err(
                "grouped clipping uses CPU; GPU stacks require bounded pixel/fill/isolated groups with default blending",
            );
        }
        if stack_end(nodes, &roots, base) != Some(end) {
            return Err("grouped clipping uses CPU; unsupported stack topology");
        }
        ends[base] = Some(end);
    }
    Ok(ends)
}

/// Return the exclusive end of an ordinary contiguous clipping stack. An
/// arbitrary earlier-sibling link is legal in native documents; do not move an
/// intervening unrelated layer into/out of a group as a side effect of rendering.
fn stack_end(nodes: &[CompositeNode], roots: &[Option<usize>], base: usize) -> Option<usize> {
    let root = &nodes[base];
    if roots[base].is_some()
        || !root.blending.blend_clipped_layers_as_group
        || (root.blend.has_special_fill() && root.blending.fill_opacity != 1.0)
        || !match &root.content {
            NodeContent::Pixels { .. }
            | NodeContent::ProjectivePixels(_)
            | NodeContent::Fill(_) => true,
            // Unit-envelope rendering preserves the group's isolated child
            // blends and masks. Pass-through must still see the real backdrop.
            NodeContent::Group(_) => root.blend != BlendMode::PassThrough,
            _ => false,
        }
    {
        return None;
    }
    let mut end = base + 1;
    while end < nodes.len() && roots[end] == Some(base) {
        end += 1;
    }
    if end == base + 1
        || !nodes[base + 1..end].iter().any(|node| node.visible)
        || roots[end..].contains(&Some(base))
        || stack_has_knockout(nodes, roots, base)
        || stack_has_styled_appearance(nodes, roots, base)
    {
        return None;
    }
    Some(end)
}

pub(super) fn render_stack(
    nodes: &[CompositeNode],
    roots: &[Option<usize>],
    base: usize,
    acc: &mut FTile,
    ctx: Ctx,
    deep_punch: &mut Option<Vec<f32>>,
) -> Option<usize> {
    let end = stack_end(nodes, roots, base)?;
    let root = &nodes[base];
    if root.opacity <= 0.0 {
        return Some(end);
    }
    // Sampling through the existing source path preserves placement, mip,
    // raster/vector masks, derived rectangle clips and effective Smart pixels.
    // For isolated group roots it also retains child blend/adjustment scope.
    // The recursive knockout gate guarantees no deep punch is lost here.
    let mut unfilled = root.clone();
    unfilled.opacity = 1.0;
    unfilled.blend = BlendMode::Normal;
    unfilled.blending = BlendingOptions::default();
    unfilled.clip_to = None;
    let mut source = Scratch::zeroed();
    render_list(std::slice::from_ref(&unfilled), &mut source, ctx);
    let shape: Vec<f32> = source.iter().map(|pixel| pixel[3]).collect();
    for pixel in source.iter_mut() {
        if pixel[3] > 0.0 {
            let alpha = pixel[3];
            for channel in pixel.iter_mut() {
                *channel = *channel / alpha * root.blending.fill_opacity;
            }
        }
    }
    // Member modes are still their authored modes. They see the normalized
    // base, never the external backdrop, and must not multiply the base's
    // antialiasing/mask alpha a second time. All links already resolve to base.
    let members: Vec<_> = nodes[base + 1..end]
        .iter()
        .cloned()
        .map(|mut member| {
            member.clip_to = None;
            member
        })
        .collect();
    if let Some(mut punch) = render_list(&members, &mut source, ctx) {
        punch.iter_mut().zip(&shape).for_each(|(p, shape)| {
            *p *= shape * root.opacity;
        });
        apply_punch(acc, &punch);
        merge_punch(deep_punch, punch);
    }
    source.iter_mut().zip(&shape).for_each(|(pixel, shape)| {
        pixel.iter_mut().for_each(|channel| *channel *= shape);
    });
    // Fill changes the base's interior contribution, whereas Opacity belongs
    // to the whole completed stack. The unfilled alpha above remains the clip
    // shape even at zero Fill. OFF retains its existing separate-member path.
    let mut envelope = root.clone();
    envelope.blending.fill_opacity = 1.0;
    let coverage = (root.opacity < 1.0).then(|| vec![root.opacity; TILE_PX]);
    composite_into(
        acc,
        &mut source,
        coverage.as_deref(),
        None,
        &envelope,
        ctx,
        deep_punch,
    );
    Some(end)
}

/// The versioned profile has its own recursion at every stack stage. The
/// conservative topology/family gate is shared, but legacy rendering is not.
pub(super) fn render_photoshop_stack(
    nodes: &[CompositeNode],
    roots: &[Option<usize>],
    base: usize,
    acc: &mut FTile,
    ctx: Ctx,
    scope: photoshop_composite::Scope<'_>,
) -> Option<usize> {
    let end = stack_end(nodes, roots, base)?;
    let root = &nodes[base];
    if root.opacity <= 0.0 {
        return Some(end);
    }
    let mut unfilled = root.clone();
    unfilled.opacity = 1.0;
    unfilled.blend = BlendMode::Normal;
    unfilled.blending = BlendingOptions::default();
    unfilled.clip_to = None;
    let mut source = Scratch::zeroed();
    photoshop_composite::render_list(
        std::slice::from_ref(&unfilled),
        &mut source,
        ctx,
        photoshop_composite::Scope::TRANSPARENT,
    );
    let shape: Vec<f32> = source.iter().map(|pixel| pixel[3]).collect();
    for pixel in source.iter_mut() {
        if pixel[3] > 0.0 {
            let alpha = pixel[3];
            for channel in pixel.iter_mut() {
                *channel = *channel / alpha * root.blending.fill_opacity;
            }
        }
    }
    let members: Vec<_> = nodes[base + 1..end]
        .iter()
        .cloned()
        .map(|mut member| {
            member.clip_to = None;
            member
        })
        .collect();
    photoshop_composite::render_list(
        &members,
        &mut source,
        ctx,
        photoshop_composite::Scope::TRANSPARENT,
    );
    source.iter_mut().zip(&shape).for_each(|(pixel, shape)| {
        pixel.iter_mut().for_each(|channel| *channel *= shape);
    });
    let mut envelope = root.clone();
    envelope.blending.fill_opacity = 1.0;
    let coverage = (root.opacity < 1.0).then(|| vec![root.opacity; TILE_PX]);
    photoshop_composite::composite_into(
        acc,
        &source,
        coverage.as_deref(),
        None,
        &envelope,
        ctx,
        scope,
    );
    Some(end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(clip_to: Option<usize>) -> CompositeNode {
        CompositeNode {
            id: 1,
            visible: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
            blending: BlendingOptions::default(),
            mask: None,
            clip_to,
            clip_rect: None,
            content: NodeContent::Fill([0.2, 0.1, 0.3, 0.5]),
        }
    }

    #[test]
    fn gpu_stack_gate_preserves_hidden_topology_and_bounds() {
        let mut middle = fill(Some(0));
        middle.visible = false;
        let nodes = [fill(None), middle, fill(Some(1))];
        assert_eq!(
            gpu_clipping_stack_ends(&nodes),
            Ok(vec![Some(3), None, None])
        );
        let mut gap = fill(None);
        gap.visible = false;
        assert!(gpu_clipping_stack_ends(&[fill(None), gap, fill(Some(0))]).is_err());
        let mut nodes = vec![fill(None), fill(Some(0)), fill(None), fill(Some(1))];
        assert!(gpu_clipping_stack_ends(&nodes).is_err());
        nodes[3].visible = false;
        assert!(
            gpu_clipping_stack_ends(&nodes).is_err(),
            "hidden links still preserve native order"
        );
        let mut root = fill(None);
        root.content = NodeContent::Group(vec![fill(None), fill(Some(0))]);
        let mut member = fill(Some(0));
        member.content = NodeContent::Group(vec![fill(None)]);
        assert_eq!(
            gpu_clipping_stack_ends(&[root.clone(), member.clone()]),
            Ok(vec![Some(2), None])
        );
        member.blend = BlendMode::PassThrough;
        assert!(gpu_clipping_stack_ends(&[root.clone(), member]).is_err());
        for amount in [0.0, 0.5] {
            root.blending.fill_opacity = amount;
            assert!(gpu_clipping_stack_ends(&[root.clone(), fill(Some(0))]).is_err());
        }
        let mut styled = fill(Some(0));
        styled.content = NodeContent::StyledGroup {
            children: vec![fill(None)],
            clip_source: Box::new(fill(None)),
            effect_mask: None,
        };
        assert!(gpu_clipping_stack_ends(&[fill(None), styled]).is_err());
        let mut knockout = fill(None);
        knockout.blending.knockout = Knockout::Deep;
        let mut member = fill(Some(0));
        member.content = NodeContent::Group(vec![knockout]);
        assert!(gpu_clipping_stack_ends(&[fill(None), member]).is_err());
        let nodes: Vec<_> = (0..65).map(|i| fill((i > 0).then_some(0))).collect();
        assert!(gpu_clipping_stack_ends(&nodes).is_err());
    }

    #[test]
    fn grouped_clip_capability_detects_root_even_if_middle_is_hidden() {
        let mut nodes = vec![fill(None), fill(Some(0))];
        assert!(has_grouped_clipping(&nodes));
        nodes[1].visible = false;
        assert!(!has_grouped_clipping(&nodes));
        nodes.push(fill(Some(1)));
        assert!(has_grouped_clipping(&nodes));
        nodes[0].visible = false;
        assert!(!has_grouped_clipping(&nodes));
    }
    #[test]
    fn grouped_clip_knockout_gate_covers_every_nested_content_path() {
        let mut punch = fill(None);
        punch.blending.knockout = Knockout::Deep;
        punch.visible = false; // Conservatively gate even temporarily hidden descendants.
        let contents = [
            NodeContent::Group(vec![punch.clone()]),
            NodeContent::ClippedGroup {
                children: vec![punch.clone()],
                baseline: vec![],
            },
            NodeContent::ClippedGroup {
                children: vec![],
                baseline: vec![punch.clone()],
            },
            NodeContent::StyledGroup {
                children: vec![punch.clone()],
                clip_source: Box::new(fill(None)),
                effect_mask: None,
            },
            NodeContent::StyledGroup {
                children: vec![],
                clip_source: Box::new(punch.clone()),
                effect_mask: None,
            },
            NodeContent::StyledGroup {
                children: vec![],
                clip_source: Box::new(fill(None)),
                effect_mask: Some(Box::new(punch)),
            },
        ];
        for content in contents {
            let mut member = fill(Some(0));
            member.content = content;
            assert!(
                grouped_clipping_fallback_reason(&[fill(None), member])
                    .unwrap()
                    .contains("knockout")
            );
        }
    }
    #[test]
    fn grouped_clip_isolated_group_support_does_not_admit_pass_through_or_knockout() {
        let mut root = fill(None);
        root.content = NodeContent::Group(vec![fill(None)]);
        let mut nodes = vec![root, fill(Some(0))];
        assert_eq!(
            grouped_clipping_fallback_reason(&nodes),
            Some("grouped clipping requires the CPU compositor")
        );
        nodes[0].blend = BlendMode::PassThrough;
        assert!(
            grouped_clipping_fallback_reason(&nodes)
                .unwrap()
                .contains("pass-through")
        );
        nodes[0].blend = BlendMode::Normal;
        let NodeContent::Group(children) = &mut nodes[0].content else {
            unreachable!()
        };
        children[0].blending.knockout = Knockout::Deep;
        assert!(
            grouped_clipping_fallback_reason(&nodes)
                .unwrap()
                .contains("knockout")
        );
    }
}
