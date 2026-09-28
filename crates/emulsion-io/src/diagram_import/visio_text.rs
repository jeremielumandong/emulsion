// ShapeSheet geometry visibility and editable text frames.
fn geometry_parts(
    node: &Xml,
    master: Option<&Xml>,
    w: f64,
    h: f64,
    style: PathStyle,
    warnings: &mut BTreeSet<String>,
) -> Result<Vec<(VectorPath, PathStyle)>> {
    let own = sections_fn(node);
    let inherited = own.is_empty();
    let sections = if inherited {
        master.map(sections_fn).unwrap_or_default()
    } else {
        own
    };
    let mut parts = Vec::new();
    for section in sections {
        if number(&section, None, "NoShow", 0.)? != 0. {
            continue;
        }
        let mut source = if inherited {
            master.unwrap().clone()
        } else {
            node.clone()
        };
        source
            .children
            .retain(|n| !((n.name == "Section" && n.attr("N") == "Geometry") || n.name == "Geom"));
        source.children.push(section.clone());
        let path = if inherited {
            geometry(node, Some(&source), w, h, warnings)?
        } else {
            geometry(&source, master, w, h, warnings)?
        };
        if let Some(path) = path {
            let mut style = style;
            if number(&section, None, "NoFill", 0.)? != 0. {
                style.fill = None;
            }
            if number(&section, None, "NoLine", 0.)? != 0. {
                style.stroke = None;
            }
            parts.push((path, style));
        }
    }
    Ok(parts)
}

fn visio_text(
    node: &Xml,
    master: Option<&Xml>,
    sheet: Option<&Xml>,
    resources: &Resources,
    transform: DAffine2,
    w: f64,
    h: f64,
    shape: &mut Shape,
    warnings: &mut BTreeSet<String>,
) -> Result<()> {
    use emulsion_core::text::{self, Align, TextRun, TextStyle};
    fn row<'a>(n: &'a Xml, section: &str, ix: &str) -> Option<&'a Xml> {
        n.children("Section")
            .find(|s| s.attr("N") == section)
            .and_then(|s| s.children("Row").find(|r| r.attr("IX") == ix))
            .or_else(|| {
                (ix == "0" && section == "Character")
                    .then(|| n.child("Char"))
                    .flatten()
            })
    }
    let character = |ix: &str| {
        row(node, "Character", ix)
            .or_else(|| master.and_then(|m| row(m, "Character", ix)))
            .or_else(|| sheet.and_then(|m| row(m, "Character", ix)))
    };
    let style = |ix: &str, warnings: &mut BTreeSet<String>| -> Result<TextStyle> {
        let Some(c) = character(ix) else {
            return Ok(TextStyle {
                size: shape.text.size,
                font: shape.text.font.clone(),
                color: shape.text.color,
                ..Default::default()
            });
        };
        let bits = number(c, None, "Style", 0.)? as u32;
        let font = value(c, "Font").unwrap_or("Geist");
        let requested = resources
            .fonts
            .get(font)
            .map(String::as_str)
            .unwrap_or(font);
        let font = if resources.available_fonts.contains(requested) {
            requested.to_string()
        } else {
            let substitute = if matches!(
                requested.to_ascii_lowercase().as_str(),
                "arial" | "helvetica"
            ) && resources.available_fonts.contains("Liberation Sans")
            {
                "Liberation Sans"
            } else {
                "Geist"
            };
            if requested != substitute {
                warnings.insert(format!(
                    "Visio font {requested} is unavailable; using {substitute}."
                ));
            }
            substitute.into()
        };
        Ok(TextStyle {
            font,
            size: (number(c, None, "Size", 14. / DPI)? * DPI).clamp(1., 1000.) as f32,
            color: resources.color(c, None, "Color", [0, 0, 0, 255], warnings),
            bold: bits & 1 != 0,
            italic: bits & 2 != 0,
            underline: bits & 4 != 0,
            strikethrough: number(c, None, "Strikethru", 0.)? != 0.,
            ..Default::default()
        })
    };
    let base = style("0", warnings)?;
    let text_node = node
        .child("Text")
        .or_else(|| master.and_then(|m| m.child("Text")));
    let mut runs = Vec::new();
    if let Some(t) = text_node {
        let markers = t.children("cp").collect::<Vec<_>>();
        for (i, mark) in markers.iter().enumerate() {
            let start = mark.text_offset.min(shape.text.text.len());
            let end = markers.get(i + 1).map_or(shape.text.text.len(), |n| {
                n.text_offset.min(shape.text.text.len())
            });
            if start < end {
                runs.push(TextRun {
                    start,
                    end,
                    style: style(mark.attr("IX"), warnings)?,
                });
            }
        }
    }
    let spec = &mut shape.text;
    spec.font = base.font;
    spec.size = base.size;
    spec.color = base.color;
    spec.bold = base.bold;
    spec.italic = base.italic;
    spec.underline = base.underline;
    spec.strikethrough = base.strikethrough;
    spec.runs = runs;
    let paragraph = row(node, "Paragraph", "0")
        .or_else(|| master.and_then(|m| row(m, "Paragraph", "0")))
        .or_else(|| sheet.and_then(|m| row(m, "Paragraph", "0")));
    let (_left, right) = if let Some(p) = paragraph {
        spec.align = match number(p, None, "HorzAlign", 1.)? as i32 {
            0 => Align::Left,
            2 => Align::Right,
            _ => Align::Center,
        };
        let spacing = number(p, None, "SpLine", -1.)?;
        spec.line_height = if spacing < 0. {
            (-spacing * 1.2).clamp(0.5, 10.) as f32
        } else {
            (spacing * DPI / spec.size as f64).clamp(0.5, 10.) as f32
        };
        (
            number(p, None, "IndLeft", 0.)?,
            number(p, None, "IndRight", 0.)?,
        )
    } else {
        spec.align = Align::Center;
        (0., 0.)
    };
    let mut bullets = Vec::new();
    if let Some(t) = text_node {
        let markers = t.children("pp").collect::<Vec<_>>();
        for start in std::iter::once(0).chain(spec.text.match_indices('\n').map(|(i, _)| i + 1)) {
            let ix = markers
                .iter()
                .rev()
                .find(|m| m.text_offset <= start)
                .map_or("0", |m| m.attr("IX"));
            if let Some(p) = row(node, "Paragraph", ix)
                .or_else(|| master.and_then(|m| row(m, "Paragraph", ix)))
                .or_else(|| sheet.and_then(|m| row(m, "Paragraph", ix)))
            {
                spec.paragraphs.push(text::ParagraphStyle {
                    start,
                    format: text::ParagraphFormat {
                        align: Some(match number(p, None, "HorzAlign", 1.)? as i32 {
                            0 => Align::Left,
                            2 => Align::Right,
                            _ => Align::Center,
                        }),
                        indent: (number(p, None, "IndLeft", 0.)? * DPI).clamp(0., 10000.) as f32,
                        hanging: (-number(p, None, "IndFirst", 0.)? * DPI).clamp(0., 10000.) as f32,
                        space_before: (number(p, None, "SpBefore", 0.)? * DPI).clamp(0., 10000.)
                            as f32,
                        space_after: (number(p, None, "SpAfter", 0.)? * DPI).clamp(0., 10000.)
                            as f32,
                        ..Default::default()
                    },
                });
                if number(p, None, "Bullet", 0.)? != 0. {
                    let mut format = spec.paragraphs.last().unwrap().format;
                    format.list = text::ParagraphList::Bullet;
                    format.hanging = (number(p, None, "TextPosAfterBullet", 0.1)? * DPI)
                        .clamp(0., 10000.) as f32;
                    bullets.push((start, format));
                }
            }
        }
    }
    for (start, format) in bullets.into_iter().rev() {
        *spec = text::apply_paragraphs(spec, start..start, format).map_err(error)?;
    }
    let tw = number(node, master, "TxtWidth", w)?.max(0.001);
    let th = number(node, master, "TxtHeight", h)?.max(0.001);
    let lm = number(node, master, "LeftMargin", 0.)?;
    let rm = number(node, master, "RightMargin", 0.)? + right;
    let tm = number(node, master, "TopMargin", 0.)?;
    let bm = number(node, master, "BottomMargin", 0.)?;
    spec.width = Some(((tw - lm - rm) * DPI).max(1.) as f32);
    let measured = text::layout(spec).bounds().height as f64 / DPI;
    let spare = (th - tm - bm - measured).max(0.);
    let vertical = match number(node, master, "VerticalAlign", 1.)? as i32 {
        0 => 0.,
        2 => spare,
        _ => spare / 2.,
    };
    let text_transform = transform
        * DAffine2::from_translation(dvec2(
            number(node, master, "TxtPinX", w / 2.)?,
            number(node, master, "TxtPinY", h / 2.)?,
        ))
        * DAffine2::from_angle(number(node, master, "TxtAngle", 0.)?)
        * DAffine2::from_translation(-dvec2(
            number(node, master, "TxtLocPinX", tw / 2.)?,
            number(node, master, "TxtLocPinY", th / 2.)?,
        ));
    let origin = text_transform.transform_point2(dvec2(lm, th - tm - vertical));
    let x_axis = text_transform.transform_vector2(dvec2(1. / DPI, 0.));
    let y_axis = text_transform.transform_vector2(dvec2(0., -1. / DPI));
    spec.x = origin.x as f32;
    spec.y = origin.y as f32;
    spec.rotation = x_axis.y.atan2(x_axis.x).to_degrees() as f32;
    spec.scale_x = x_axis.length() as f32;
    spec.scale_y = (y_axis.length() * x_axis.perp_dot(y_axis).signum()) as f32;
    *spec = spec.clone().sanitized();
    shape.positioned_text = true;
    Ok(())
}

fn load_images(
    node: &mut Xml,
    source: &str,
    package: &Package,
    resources: &mut Resources,
    warnings: &mut BTreeSet<String>,
) -> Result<()> {
    let rels = relationships(package, source)?;
    fn visit(
        node: &mut Xml,
        rels: &BTreeMap<String, String>,
        package: &Package,
        resources: &mut Resources,
        warnings: &mut BTreeSet<String>,
    ) {
        if let Some(part) = node
            .child("ForeignData")
            .filter(|f| f.attr("ForeignType") == "Bitmap")
            .and_then(|f| f.child("Rel"))
            .and_then(|r| rels.get(r.attr("id")))
        {
            if !resources.images.contains_key(part) {
                if let Some(bytes) = package.entries.get(part) {
                    // Compressed package limits alone do not bound bitmap allocation.
                    let dimensions = image::ImageReader::new(std::io::Cursor::new(bytes))
                        .with_guessed_format()
                        .ok()
                        .and_then(|r| r.into_dimensions().ok());
                    let used: u64 = resources
                        .images
                        .values()
                        .map(|r| r.width() as u64 * r.height() as u64)
                        .sum();
                    if dimensions.is_none_or(|(w, h)| {
                        w == 0 || h == 0 || used + w as u64 * h as u64 > 32_000_000
                    }) {
                        warnings.insert(format!("Embedded Visio image {part} exceeds the decoded image budget or has unsupported dimensions."));
                        return;
                    }
                    match crate::import::import_bytes("Visio bitmap", bytes) {
                        Ok(doc) => {
                            if let Some(image) = doc.nodes.iter().find_map(|n| match &n.kind {
                                NodeKind::Raster { raster, .. } => Some(raster.clone()),
                                _ => None,
                            }) {
                                resources.images.insert(part.clone(), image);
                            }
                        }
                        Err(_) => {
                            warnings
                                .insert(format!("Could not decode embedded Visio image {part}."));
                        }
                    }
                }
            }
            node.image_part = Some(part.clone());
        }
        for child in &mut node.children {
            visit(child, rels, package, resources, warnings);
        }
    }
    visit(node, &rels, package, resources, warnings);
    Ok(())
}
