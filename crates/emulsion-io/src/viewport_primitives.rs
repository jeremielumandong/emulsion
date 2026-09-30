//! Retain decoded shadow pixels instead of asking resvg to decode PNG each frame.
//! Isolation boundaries (opacity, masks, clipping and blends) stay with resvg.
use resvg::{tiny_skia as sk, usvg};

pub(super) enum Primitive {
    Svg(usvg::Node, sk::Transform),
    Shadow(sk::Pixmap, sk::Transform),
}

/// Only local drawing operations may be split at viewport band boundaries.
/// Filters, masks, patterns and embedded SVG can sample beyond those boundaries.
pub(super) fn band_safe(group: &usvg::Group) -> bool {
    group.filters().is_empty()
        && group.mask().is_none()
        && group.children().iter().all(|node| match node {
            usvg::Node::Group(group) => band_safe(group),
            usvg::Node::Path(path) => {
                path.fill()
                    .is_none_or(|f| !matches!(f.paint(), usvg::Paint::Pattern(_)))
                    && path
                        .stroke()
                        .is_none_or(|s| !matches!(s.paint(), usvg::Paint::Pattern(_)))
            }
            usvg::Node::Image(image) => {
                group.id().starts_with("viewport-shadow-")
                    && matches!(image.kind(), usvg::ImageKind::PNG(_))
            }
            usvg::Node::Text(_) => false,
        })
}

pub(super) fn retain(tree: &usvg::Tree) -> Option<Vec<Primitive>> {
    fn visit(group: &usvg::Group, primitives: &mut Vec<Primitive>) {
        for node in group.children() {
            if let usvg::Node::Group(child) = node
                && !child.should_isolate()
            {
                visit(child, primitives);
                continue;
            }
            if let usvg::Node::Image(image) = node
                && image.is_visible()
                // usvg moves an image's ID onto its placement group.
                && group.id().starts_with("viewport-shadow-")
                && let usvg::ImageKind::PNG(data) = image.kind()
                && let Ok(pixels) = sk::Pixmap::decode_png(data)
            {
                primitives.push(Primitive::Shadow(pixels, image.abs_transform()));
            } else {
                // render_node translates by -abs_layer_bbox before rendering;
                // cancel that placement, retaining the ancestor transform.
                let Some(bounds) = node.abs_layer_bounding_box() else {
                    continue;
                };
                primitives.push(Primitive::Svg(
                    node.clone(),
                    group.abs_transform().pre_translate(bounds.x(), bounds.y()),
                ));
            }
        }
    }
    let mut primitives = Vec::new();
    visit(tree.root(), &mut primitives);
    primitives
        .iter()
        .any(|p| matches!(p, Primitive::Shadow(..)))
        .then_some(primitives)
}

pub(super) fn render(
    primitives: &[Primitive],
    transform: sk::Transform,
    target: &mut sk::PixmapMut<'_>,
) {
    for primitive in primitives {
        match primitive {
            Primitive::Svg(node, placement) => {
                resvg::render_node(node, transform.pre_concat(*placement), target);
            }
            Primitive::Shadow(pixels, placement) => {
                // Match SVG's padded image sampling, with smooth interpolation
                // for the soft effect. Foreground contours still use resvg.
                let paint = sk::Paint {
                    shader: sk::Pattern::new(
                        pixels.as_ref(),
                        sk::SpreadMode::Pad,
                        sk::FilterQuality::Bilinear,
                        1.,
                        sk::Transform::identity(),
                    ),
                    ..Default::default()
                };
                let rect =
                    sk::Rect::from_xywh(0., 0., pixels.width() as f32, pixels.height() as f32)
                        .unwrap();
                target.fill_rect(rect, &paint, transform.pre_concat(*placement), None);
            }
        }
    }
}
