//! Scalable starter effects. Only shadows use filters; foreground glyphs and
//! aligned shape borders stay paths in SVG/PDF rather than flattening a page.
use super::*;
use emulsion_core::styles::LayerStyle;
use emulsion_raster::vector::{Path, PathStyle};

pub(super) fn write(
    doc: &Document,
    id: NodeId,
    out: &mut String,
    purpose: SvgPurpose,
) -> Result<bool> {
    let node = doc.node(id).ok_or_else(|| error("Missing export layer"))?;
    if node.effects_enabled && !node.styles.is_empty() {
        if !node.style_options.iter().all(|o| {
            let mut settings = o.clone();
            settings.id = 0;
            settings == Default::default()
        }) || !node.styles.iter().all(|s| match s {
            LayerStyle::DropShadow { .. } => true,
            LayerStyle::Stroke { .. } => {
                matches!(&node.kind, NodeKind::Text { spec, .. } if spec.height.is_none())
            }
            _ => false,
        }) {
            return Ok(false);
        }
        let mut plain = doc.clone();
        let source = plain.node_mut(id).unwrap();
        source.styles.clear();
        source.style_options.clear();
        source.opacity = 1.;
        let mut body = String::new();
        node_svg_for(&plain, id, &mut body, purpose)?;
        write!(
            out,
            "<g opacity=\"{}\"><defs><g id=\"effect-art-{id}\">{body}</g></defs>",
            node.opacity
        )
        .unwrap();
        let bounds = emulsion_core::geometry::node_bounds(&plain, id).unwrap_or_default();
        for (index, style) in node.styles.iter().enumerate() {
            match style {
                LayerStyle::DropShadow {
                    color,
                    opacity,
                    angle,
                    distance,
                    size,
                } => {
                    let a = angle.to_radians();
                    let dx = (-a.cos() * distance).round();
                    let dy = (a.sin() * distance).round();
                    let pad = (size * 2. + distance + 2.).ceil() as i32;
                    let mut shadow = String::new();
                    write!(shadow, "<defs><filter id=\"effect-shadow-{id}-{index}\" filterUnits=\"userSpaceOnUse\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" color-interpolation-filters=\"linearRGB\"><feGaussianBlur in=\"SourceAlpha\" stdDeviation=\"{}\"/><feOffset dx=\"{dx}\" dy=\"{dy}\" result=\"offset\"/><feFlood flood-color=\"#{:02x}{:02x}{:02x}\" flood-opacity=\"{}\"/><feComposite in2=\"offset\" operator=\"in\"/></filter></defs><use href=\"#effect-art-{id}\" filter=\"url(#effect-shadow-{id}-{index})\"/>",
                        bounds.x-pad, bounds.y-pad, bounds.w+pad*2, bounds.h+pad*2,
                        size/2., color[0], color[1], color[2], opacity/100.).unwrap();
                    match purpose {
                        SvgPurpose::Export => out.push_str(&shadow),
                        SvgPurpose::Viewport => out.push_str(&crate::viewport_shadow::image(
                            &body,
                            &shadow,
                            id,
                            index,
                            [
                                f64::from(bounds.x) - f64::from(pad),
                                f64::from(bounds.y) - f64::from(pad),
                                f64::from(bounds.w) + 2. * f64::from(pad),
                                f64::from(bounds.h) + 2. * f64::from(pad),
                            ],
                        )?),
                    }
                }
                LayerStyle::Stroke {
                    color,
                    opacity,
                    size,
                } => {
                    let NodeKind::Text { spec, .. } = &node.kind else {
                        unreachable!()
                    };
                    let paths = emulsion_core::text::vector_paths(spec).ok_or_else(|| {
                        error("This text outline requires a rendered appearance.")
                    })?;
                    let mut outline = Path::default();
                    for (path, rgba) in paths {
                        if rgba[3] != 255 {
                            return Err(error(
                                "Translucent glyph outlines require a rendered appearance.",
                            ));
                        }
                        if *size > 0. {
                            outline.subpaths.extend(
                                emulsion_raster::vector::stroke_outline(
                                    &path,
                                    &PathStyle {
                                        width: size * 2.,
                                        ..Default::default()
                                    },
                                )
                                .map_err(error)?
                                .subpaths,
                            );
                        }
                    }
                    write!(
                        out,
                        "<path d=\"{}\" fill=\"#{:02x}{:02x}{:02x}\" fill-opacity=\"{}\"/>",
                        outline.to_svg(),
                        color[0],
                        color[1],
                        color[2],
                        opacity / 100.
                    )
                    .unwrap();
                }
                _ => unreachable!(),
            }
        }
        // Outside the filter: PDF consumers retain the sharp foreground contours.
        write!(out, "<use href=\"#effect-art-{id}\"/></g>").unwrap();
        return Ok(true);
    }
    let NodeKind::Path { path, style, .. } = &node.kind else {
        return Ok(false);
    };
    let border = node
        .clip_to
        .is_some_and(|base| emulsion_core::design::media::frame_border(doc, base) == Some(id));
    if (style.alignment == StrokeAlignment::Center && !border)
        || (node.clip_to.is_some() && !border)
    {
        return Ok(false);
    }
    let mut plain = doc.clone();
    let source = plain.node_mut(id).unwrap();
    source.opacity = 1.;
    if border {
        source.clip_to = None;
    }
    let NodeKind::Path { style: paint, .. } = &mut source.kind else {
        unreachable!()
    };
    paint.alignment = StrokeAlignment::Center;
    paint.stroke = None;
    let mut body = String::new();
    node_svg_for(&plain, id, &mut body, purpose)?;
    write!(out, "<g opacity=\"{}\">{body}", node.opacity).unwrap();
    if let Some(color) = style.stroke.filter(|_| style.width > 0.) {
        if matches!(style.stroke_paint, PathPaint::Pattern { .. }) {
            return Err(error("Pattern strokes require a rendered appearance."));
        }
        let wide = emulsion_raster::vector::stroke_outline(
            path,
            &PathStyle {
                width: style.width
                    * if style.alignment == StrokeAlignment::Center {
                        1.
                    } else {
                        2.
                    },
                alignment: StrokeAlignment::Center,
                ..*style
            },
        )
        .map_err(error)?;
        use emulsion_raster::vector_geometry::{BooleanOp, boolean};
        let stroke = if style.alignment == StrokeAlignment::Center {
            wide
        } else {
            boolean(
                &wide,
                path,
                if style.alignment == StrokeAlignment::Inside {
                    BooleanOp::Intersect
                } else {
                    BooleanOp::Subtract
                },
            )
            .map_err(error)?
        };
        let paint = svg_paint(
            out,
            &format!("aligned-stroke-{id}"),
            style.stroke_paint,
            Some(color),
            emulsion_raster::vector_geometry::bounds(path).unwrap_or((0., 0., 1., 1.)),
        );
        write!(
            out,
            "<path d=\"{}\" fill=\"{paint}\" fill-rule=\"evenodd\" fill-opacity=\"{}\"/>",
            stroke.to_svg(),
            if style.stroke_paint == PathPaint::Solid {
                color[3] as f32 / 255.
            } else {
                1.
            }
        )
        .unwrap();
    }
    out.push_str("</g>");
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aligned_borders_match_native_inside_and_outside_coverage() {
        for alignment in [StrokeAlignment::Inside, StrokeAlignment::Outside] {
            let mut doc = Document::new(120, 100);
            doc.nodes.push(emulsion_core::Node::path(
                1,
                "Border",
                std::sync::Arc::new(emulsion_raster::vector_geometry::rectangle(
                    20., 20., 80., 60.,
                )),
                PathStyle {
                    fill: Some([0, 0, 255, 255]),
                    stroke: Some([255, 0, 0, 255]),
                    width: 8.,
                    alignment,
                    ..Default::default()
                },
                120,
                100,
            ));
            doc.next_id = 2;
            let bytes = vector_svg(&doc).unwrap();
            let tree = resvg::usvg::Tree::from_data(&bytes, &Default::default()).unwrap();
            let mut image = resvg::tiny_skia::Pixmap::new(120, 100).unwrap();
            resvg::render(
                &tree,
                resvg::tiny_skia::Transform::identity(),
                &mut image.as_mut(),
            );
            let native = flatten(&doc.composite_tree(), 0).to_srgba8();
            for (x, y) in [(15, 50), (23, 50), (35, 50), (97, 50), (104, 50)] {
                let index = (y * 120 + x) * 4;
                assert_eq!(
                    &image.data()[index..index + 4],
                    &native[index..index + 4],
                    "{alignment:?} at {x},{y}"
                );
            }
        }
    }
    #[test]
    fn styled_text_exports_unfiltered_foreground_and_survives_native_roundtrip() {
        let doc = emulsion_core::design::Template::VideoThumbnail
            .create(640, 360)
            .unwrap();
        let before = doc.clone();
        let (svg, flattened) = svg(&doc).unwrap();
        assert!(!flattened);
        let svg = String::from_utf8(svg).unwrap();
        assert!(svg.contains("feGaussianBlur"));
        assert!(svg.contains("effect-shadow-"));
        assert!(svg.contains("\"/></g>"));
        assert!(!svg.contains("<image"));
        let editor = emulsion_core::project::ProjectEditor::new_project(
            emulsion_core::project::ProjectKind::Design,
            doc,
        )
        .unwrap();
        let project = editor.snapshot().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("styled.emu");
        crate::project::write(&project, &file).unwrap();
        let loaded = crate::project::read(&file).unwrap();
        let reopened = emulsion_core::project::ProjectEditor::open(loaded, None).unwrap();
        assert_eq!(reopened.doc, before);
        let output = directory.path().join("styled.pdf");
        let report = super::super::write(&project, &[1], Format::Pdf, false, &output).unwrap();
        assert!(report.rasterized_pages.is_empty());
    }
}
