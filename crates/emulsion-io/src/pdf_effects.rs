//! PDF filter rasterization is explicit and bounded. Foreground paths, outlined
//! text and unfiltered images keep their original representation and resolution.
use crate::{IoError, Result};
use svg2pdf::usvg::{Group, Node, Paint, Transform, Tree};

fn error(message: &str) -> IoError {
    IoError::Unsupported(message.into())
}

/// SVG filter groups cannot be represented as PDF vectors. Render only those
/// groups at at least 300 effective PPI, without downsampling native artwork.
/// `ppi` is the number of top-level SVG units per printed inch (25.4 for a
/// millimeter-based print sheet, or the document PPI for page artwork).
pub(crate) fn options(tree: &Tree, ppi: f32) -> Result<(svg2pdf::ConversionOptions, bool)> {
    if !ppi.is_finite() || ppi <= 0. {
        return Err(error("Invalid PDF output resolution"));
    }
    let mut budget = Budget::default();
    inspect(
        tree.root(),
        svg2pdf::usvg::Transform::identity(),
        &mut budget,
    )?;
    // svg2pdf has one scale for the whole conversion. Account for ancestors
    // and nested SVG image placement before choosing that common scale.
    let scale = budget
        .effects
        .iter()
        .map(|(_, _, placement)| 300. / f64::from(ppi) * placement)
        .fold(1_f64, f64::max);
    let mut total = 0.;
    for &(width, height, _) in &budget.effects {
        let width = (width * scale).round();
        let height = (height * scale).round();
        let pixels = width * height;
        if !width.is_finite()
            || !height.is_finite()
            || width < 1.
            || height < 1.
            || width > 16_000.
            || height > 16_000.
            || pixels > 16_000_000.
        {
            return Err(error(
                "A PDF effect exceeds the 16 megapixel render budget at print quality. Reduce the effect's size or export that page as PNG.",
            ));
        }
        total += pixels;
        if total > 64_000_000. {
            return Err(error(
                "PDF effects exceed the 64 megapixel page budget. Reduce effects or export that page as PNG.",
            ));
        }
    }
    Ok((
        svg2pdf::ConversionOptions {
            raster_scale: scale as f32,
            ..Default::default()
        },
        !budget.effects.is_empty(),
    ))
}

#[derive(Default)]
struct Budget {
    nodes: usize,
    // svg2pdf allocation width/height, maximum physical placement scale.
    effects: Vec<(f64, f64, f64)>,
}

fn max_scale(transform: Transform) -> f64 {
    let [a, b, c, d] = [transform.sx, transform.ky, transform.kx, transform.sy].map(f64::from);
    let sum = a * a + b * b + c * c + d * d;
    let determinant = a * d - b * c;
    ((sum + (sum * sum - 4. * determinant * determinant).max(0.).sqrt()) / 2.).sqrt()
}

fn inspect(group: &Group, parent: Transform, budget: &mut Budget) -> Result<()> {
    budget.nodes += 1;
    if budget.nodes > 100_000 {
        return Err(error(
            "PDF artwork is too complex; export fewer objects per page",
        ));
    }
    if !group.filters().is_empty() {
        // Match svg2pdf's allocation, which includes only the group's own
        // transform. Its parent transform changes the effective printed PPI.
        let bounds = group
            .layer_bounding_box()
            .transform(group.transform())
            .ok_or_else(|| error("Invalid PDF effect bounds"))?;
        let placement = max_scale(parent);
        if !placement.is_finite() {
            return Err(error("Invalid PDF effect placement"));
        }
        budget.effects.push((
            f64::from(bounds.width()),
            f64::from(bounds.height()),
            placement,
        ));
    }
    // Compose the actual local transforms. usvg's absolute transforms can be
    // stale after it inserts a pattern contentUnits/viewBox transform.
    let current = parent.pre_concat(group.transform());
    if let Some(clip) = group.clip_path() {
        inspect_clip(clip, current, budget)?;
    }
    if let Some(mask) = group.mask() {
        inspect_mask(mask, current, budget)?;
    }
    for filter in group.filters() {
        for primitive in filter.primitives() {
            if let svg2pdf::usvg::filter::Kind::Image(image) = primitive.kind() {
                inspect(image.root(), current, budget)?;
            }
        }
    }
    for node in group.children() {
        match node {
            Node::Group(child) => inspect(child, current, budget)?,
            Node::Image(image) => {
                // usvg puts image placement (including viewBox fitting) on a
                // surrounding group, so the nested tree inherits `current`.
                if image.is_visible()
                    && let svg2pdf::usvg::ImageKind::SVG(tree) = image.kind()
                {
                    inspect(tree.root(), current, budget)?;
                }
            }
            Node::Path(path) => {
                for paint in path
                    .fill()
                    .map(|fill| fill.paint())
                    .into_iter()
                    .chain(path.stroke().map(|stroke| stroke.paint()))
                {
                    if let Paint::Pattern(pattern) = paint {
                        inspect(
                            pattern.root(),
                            current.pre_concat(pattern.transform()),
                            budget,
                        )?;
                    }
                }
            }
            Node::Text(text) => inspect(text.flattened(), current, budget)?,
        }
    }
    Ok(())
}

fn inspect_clip(
    clip: &svg2pdf::usvg::ClipPath,
    parent: Transform,
    budget: &mut Budget,
) -> Result<()> {
    if let Some(nested) = clip.clip_path() {
        inspect_clip(nested, parent, budget)?;
    }
    inspect(clip.root(), parent.pre_concat(clip.transform()), budget)
}

fn inspect_mask(mask: &svg2pdf::usvg::Mask, parent: Transform, budget: &mut Budget) -> Result<()> {
    if let Some(nested) = mask.mask() {
        inspect_mask(nested, parent, budget)?;
    }
    inspect(mask.root(), parent, budget)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(region: &str) -> Tree {
        Tree::from_str(&format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="80"><defs><filter id="glow" filterUnits="userSpaceOnUse" {region}><feGaussianBlur stdDeviation="3"/></filter></defs><rect x="30" y="30" width="20" height="20" fill="#ff6600" filter="url(#glow)"/><path d="M10 10H90V15H10Z" fill="blue"/></svg>"##), &Default::default()).unwrap()
    }

    #[test]
    fn filtered_pdf_keeps_glow_pixels_and_unfiltered_foreground_paths() {
        let tree = tree(r#"x="10" y="10" width="60" height="60""#);
        let (mut options, effects) = options(&tree, 150.).unwrap();
        assert!(effects);
        assert_eq!(options.raster_scale, 2.);
        options.compress = false;
        let pdf = svg2pdf::to_pdf(&tree, options, Default::default()).unwrap();
        let pdf = String::from_utf8_lossy(&pdf);
        assert!(
            pdf.contains("/Subtype /Image"),
            "PDF filter support must not be disabled"
        );
        assert!(pdf.contains("/Width 120"));
        assert!(pdf.contains("/Height 120"));
        assert!(
            pdf.contains("10 10 m"),
            "The foreground must remain a vector path: {pdf}"
        );
    }

    #[test]
    fn nested_svg_effects_account_for_printed_image_scale() {
        use base64::Engine as _;
        let source = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="80"><defs><filter id="blur" filterUnits="userSpaceOnUse" x="10" y="10" width="60" height="60"><feGaussianBlur stdDeviation="3"/></filter></defs><rect x="30" y="30" width="20" height="20" fill="red" filter="url(#blur)"/></svg>"##;
        let encoded = base64::engine::general_purpose::STANDARD.encode(source);
        let source = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="1000" height="800"><image width="1000" height="800" href="data:image/svg+xml;base64,{encoded}"/></svg>"#
        );
        let tree = Tree::from_str(&source, &Default::default()).unwrap();
        let (options, effects) = options(&tree, 300.).unwrap();
        assert!(effects);
        assert!((options.raster_scale - 10.).abs() < 0.001);
        let bytes = svg2pdf::to_pdf(&tree, options, Default::default()).unwrap();
        let pdf = String::from_utf8_lossy(&bytes);
        assert!(pdf.contains("/Width 600"));
        assert!(pdf.contains("/Height 600"));
    }

    fn pattern(attributes: &str, content: &str) -> Tree {
        Tree::from_str(
            &format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><defs><filter id="blur" filterUnits="userSpaceOnUse" x="0" y="0" width="2" height="2"><feGaussianBlur stdDeviation="0.1"/></filter><pattern id="tile" {attributes}>{content}</pattern></defs><rect width="200" height="100" fill="url(#tile)"/></svg>"##),
            &Default::default(),
        ).unwrap()
    }

    #[test]
    fn pattern_transform_increases_filter_resolution_and_budget() {
        let content = r#"<rect width="1" height="1" fill="red" filter="url(#blur)"/>"#;
        let tree = pattern(
            r#"patternUnits="userSpaceOnUse" width="10" height="10" patternTransform="scale(10)""#,
            content,
        );
        let (options, effects) = options(&tree, 300.).unwrap();
        assert!(effects);
        assert_eq!(options.raster_scale, 10.);
        let bytes = svg2pdf::to_pdf(&tree, options, Default::default()).unwrap();
        let pdf = String::from_utf8_lossy(&bytes);
        assert!(pdf.contains("/Width 20"));
        assert!(pdf.contains("/Height 20"));
        let enormous = pattern(
            r#"patternUnits="userSpaceOnUse" width="10" height="10" patternTransform="scale(10000)""#,
            content,
        );
        assert!(super::options(&enormous, 300.).is_err());
    }

    #[test]
    fn pattern_content_units_walk_inserted_local_transforms() {
        let content = r#"<g opacity="0.9"><rect width="0.5" height="0.5" fill="red" filter="url(#blur)"/></g>"#;
        let tree = pattern(
            r#"patternUnits="userSpaceOnUse" patternContentUnits="objectBoundingBox" width="100" height="100""#,
            content,
        );
        let (options, effects) = options(&tree, 300.).unwrap();
        assert!(effects);
        assert_eq!(options.raster_scale, 200.);
    }

    #[test]
    fn pattern_view_box_composes_with_pattern_transform() {
        let content =
            r#"<g opacity="0.9"><rect width="1" height="1" fill="red" filter="url(#blur)"/></g>"#;
        let tree = pattern(
            r#"width="0.5" height="0.5" viewBox="0 0 10 10" preserveAspectRatio="none" patternTransform="scale(2)""#,
            content,
        );
        let (options, effects) = options(&tree, 300.).unwrap();
        assert!(effects);
        assert_eq!(options.raster_scale, 20.);
    }

    #[test]
    fn nested_group_ancestors_preserve_the_highest_axis_resolution() {
        let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><defs><filter id="blur" filterUnits="userSpaceOnUse" x="0" y="0" width="2" height="2"><feGaussianBlur stdDeviation="0.1"/></filter></defs><g transform="scale(4 2)" opacity="0.8"><g transform="scale(3)" opacity="0.8"><rect width="1" height="1" fill="red" filter="url(#blur)"/></g></g></svg>"#;
        let tree = Tree::from_str(source, &Default::default()).unwrap();
        let (options, _) = options(&tree, 300.).unwrap();
        assert_eq!(options.raster_scale, 12.);
    }

    #[test]
    fn excessive_filter_regions_fail_before_allocation() {
        let tree = tree(r#"x="-100000" y="-100000" width="200000" height="200000""#);
        assert!(options(&tree, 300.).is_err());
        assert!(options(&tree, 0.).is_err());
        assert!(options(&tree, f32::NAN).is_err());
    }
}
