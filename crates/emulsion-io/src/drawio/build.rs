//! Build imported pages once, without an undo snapshot and reroute per XML attribute.
use super::*;
use emulsion_core::{diagram::Builder, graph::Graph, vector_cache::VectorRaster};

pub(super) const ANCHOR: &str = "emulsion_drawio_endpoint";

fn origin(key: Option<&String>, bounds: &HashMap<String, [f64; 4]>) -> (f64, f64) {
    key.and_then(|k| bounds.get(k))
        .map_or((0., 0.), |b| (b[0], b[1]))
}

pub(super) fn build(page: Page, id: u64, warnings: &mut BTreeSet<String>) -> Result<ProjectPage> {
    let mut cells = HashMap::new();
    let mut order = Vec::new();
    for cell in page.cells {
        let key = cell
            .attrs
            .get("id")
            .ok_or_else(|| error("Cell is missing an ID"))?
            .clone();
        if cells.insert(key.clone(), cell).is_some() {
            return Err(error("Duplicate diagram cell ID"));
        }
        order.push(key);
    }
    // Validate all ancestry, including layers and edge labels, before allocating a scene.
    for key in &order {
        let mut seen = HashSet::new();
        let mut current = Some(key);
        while let Some(key) = current {
            if !seen.insert(key) || seen.len() > 64 {
                return Err(error("Cyclic or deeply nested diagram containers"));
            }
            let cell = cells
                .get(key)
                .ok_or_else(|| error(format!("Missing parent {key}")))?;
            current = cell.attrs.get("parent");
        }
    }
    let has_children: HashSet<_> = cells
        .values()
        .filter_map(|c| c.attrs.get("parent"))
        .collect();
    let mut builder = Builder::new(page.width, page.height).map_err(error)?;
    let mut ids = HashMap::new();
    let mut bounds = HashMap::new();
    let mut styles = HashMap::new();
    let mut waiting = order
        .iter()
        .filter(|key| cells[*key].attrs.get("vertex").is_some_and(|v| v == "1"))
        .collect::<Vec<_>>();
    let mut edge_labels = Vec::new();
    while !waiting.is_empty() {
        let count = waiting.len();
        let mut next = Vec::new();
        for key in waiting {
            let cell = &cells[key];
            let parent = cell.attrs.get("parent");
            if parent.is_some_and(|p| cells[p].attrs.get("edge").is_some_and(|v| v == "1")) {
                edge_labels.push(key.clone());
                continue;
            }
            if parent.is_some_and(|p| {
                cells[p].attrs.get("vertex").is_some_and(|v| v == "1") && !ids.contains_key(p)
            }) {
                next.push(key);
                continue;
            }
            let offset = origin(parent, &bounds);
            let relative = cell.geometry.get("relative").is_some_and(|v| v == "1");
            let scale = if relative {
                parent
                    .and_then(|p| bounds.get(p))
                    .map_or((1., 1.), |b| (b[2], b[3]))
            } else {
                (1., 1.)
            };
            let w = number(&cell.geometry, "width", 120.)?;
            let h = number(&cell.geometry, "height", 60.)?;
            if w < 0. || h < 0. {
                return Err(error("Negative shape dimensions"));
            }
            let b = [
                number(&cell.geometry, "x", 0.)? * scale.0 + offset.0 + cell.offset.0,
                number(&cell.geometry, "y", 0.)? * scale.1 + offset.1 + cell.offset.1,
                w.max(1.),
                h.max(1.),
            ];
            let mut style = style(cell);
            // mxGraph defaults differ from the native palette. Text and groups have no body paint.
            let invisible = style.contains_key("text")
                || style.contains_key("group")
                || style.get("shape").is_some_and(|s| s == "text");
            style
                .entry("fillColor".into())
                .or_insert_with(|| if invisible { "none" } else { "#ffffff" }.into());
            style
                .entry("strokeColor".into())
                .or_insert_with(|| if invisible { "none" } else { "#000000" }.into());
            style.entry("strokeWidth".into()).or_insert("1".into());
            style.entry("fontColor".into()).or_insert("#000000".into());
            style.entry("fontSize".into()).or_insert("12".into());
            let mut kind = shape_kind(&style, warnings);
            if has_children.contains(key) {
                kind = if kind == ShapeKind::Swimlane {
                    kind
                } else {
                    ShapeKind::Container
                };
            }
            let label = plain_label(
                cell.attrs.get("value").map_or("", String::as_str),
                style.get("html").is_some_and(|v| v == "1"),
                warnings,
            );
            let node = builder.add_shape(kind, b, &label).map_err(error)?;
            ids.insert(key.clone(), node);
            bounds.insert(key.clone(), b);
            styles.insert(key.clone(), style);
        }
        if next.len() == count {
            return Err(error("Unsupported relative vertex ancestry"));
        }
        waiting = next;
    }
    let mut anchors = Vec::new();
    let mut edges = Vec::new();
    for key in &order {
        let cell = &cells[key];
        if !cell.attrs.get("edge").is_some_and(|v| v == "1") {
            continue;
        }
        let mut style = style(cell);
        for (key, value) in [
            ("strokeColor", "#000000"),
            ("fillColor", "#000000"),
            ("strokeWidth", "1"),
            ("fontColor", "#000000"),
            ("fontSize", "11"),
        ] {
            style.entry(key.into()).or_insert_with(|| value.into());
        }
        let offset = origin(cell.attrs.get("parent"), &bounds);
        let mut endpoint = |name: &str,
                            point: Option<(f64, f64)>,
                            prefix: &str|
         -> Result<Endpoint> {
            if let Some(reference) = cell.attrs.get(name) {
                if let Some(shape) = ids.get(reference) {
                    return Ok(Endpoint {
                        shape: *shape,
                        port: port(&style, prefix, warnings)?,
                    });
                }
                if !cells.contains_key(reference) {
                    return Err(error(format!(
                        "Connector {key} references missing {name} {reference}"
                    )));
                }
                // mxGraph permits a connection to another edge. Retain its position as a free endpoint.
                warnings.insert("Connections to connector paths are imported as positioned endpoints; reattach after moving the referenced connector.".into());
            }
            let point = point.or_else(|| {
                if name == "source" {
                    cell.points.first().copied()
                } else {
                    cell.points.last().copied()
                }
            });
            let (x, y) = point.unwrap_or_else(|| {
                warnings.insert(
                    "A connector without endpoint coordinates starts at its parent origin.".into(),
                );
                (0., 0.)
            });
            let node = builder
                .add_shape(
                    ShapeKind::Process,
                    [x + offset.0 - 0.5, y + offset.1 - 0.5, 1., 1.],
                    "",
                )
                .map_err(error)?;
            anchors.push(node);
            Ok(Endpoint {
                shape: node,
                port: Port::Custom { x: 0.5, y: 0.5 },
            })
        };
        let source = endpoint("source", cell.source_point, "exit")?;
        let target = endpoint("target", cell.target_point, "entry")?;
        let routing = if style.get("emulsionRouting").is_some_and(|v| v == "cyclical") { Routing::Cyclical } else if style.get("curved").is_some_and(|v| v == "1") {
            Routing::Curved
        } else if style.get("edgeStyle").is_some_and(|s| {
            matches!(
                s.as_str(),
                "orthogonalEdgeStyle" | "elbowEdgeStyle" | "entityRelationEdgeStyle"
            )
        }) {
            Routing::Orthogonal
        } else {
            Routing::Straight
        };
        let label = plain_label(
            cell.attrs.get("value").map_or("", String::as_str),
            style.get("html").is_some_and(|v| v == "1"),
            warnings,
        );
        let node = builder
            .connect(source, target, &label, routing)
            .map_err(error)?;
        edges.push((key.clone(), node, style, offset));
    }
    let mut doc = builder.finish().map_err(error)?;
    doc.nodes[0].kind = NodeKind::Fill {
        rgba: page.background,
    };
    let mut model = doc.diagram.take().unwrap().as_ref().clone();
    let mut image_pixels_remaining = 16_777_216usize;
    for key in &order {
        let Some(id) = ids.get(key).copied() else {
            continue;
        };
        let cell = &cells[key];
        let style = &styles[key];
        let shape = model.shapes.get_mut(&id).unwrap();
        if let Some(data) = cell.attrs.get("emulsionData") {
            shape.data = serde_json::from_str(data)
                .map_err(|e| error(format!("Invalid shape data: {e}")))?;
        }
        let geometry_style = style
            .iter()
            .filter(|(k, _)| {
                matches!(
                    k.as_str(),
                    "shape"
                        | "ellipse"
                        | "rhombus"
                        | "rounded"
                        | "arcSize"
                        | "absoluteArcSize"
                        | "text"
                        | "group"
                        | "swimlane"
                        | "top"
                        | "left"
                        | "right"
                        | "bottom"
                )
            })
            .map(|(k, v)| format!("{k}={v};"))
            .collect::<String>();
        if !geometry_style.is_empty() && geometry_style.len() <= 4096 {
            shape
                .data
                .insert("drawio_geometry_style".into(), geometry_style);
        }
        if let Some(link)=cell.attrs.get("link").filter(|s|!s.is_empty()) {
            if emulsion_core::design_interactions::valid_url(link) && doc.design.interactions.len()<1024 {
                doc.design.interactions.insert(id,vec![emulsion_core::design_interactions::Action::Url{url:link.clone()}]);
                shape.data.insert("drawio_link".into(),link.clone());
            } else {warnings.insert("Unsupported hyperlink retained as metadata; only HTTP(S) links can be opened.".into());if link.len()<=4096 {shape.data.insert("drawio_link".into(),link.clone());}}
        }
        apply_style(&mut doc, shape.body, shape.label, style, warnings)?;
        super::labels::apply(&mut doc, shape.label, cell, style, warnings);
        apply_cell(&mut doc, id, cell, style)?;
        // Layer and group visibility applies to all descendants, even when the layer is not a vertex.
        let mut ancestor = cell.attrs.get("parent");
        while let Some(parent) = ancestor {
            if cells[parent].attrs.get("visible").is_some_and(|v| v == "0") {
                doc.node_mut(id).unwrap().visible = false;
            }
            ancestor = cells[parent].attrs.get("parent");
        }
        if let Some(parent) = cell.attrs.get("parent").and_then(|p| ids.get(p)) {
            doc.node_mut(id).unwrap().parent = Some(*parent);
            shape.container = Some(*parent);
        }
        if style.get("shadow").is_some_and(|v|v=="1") {
            doc.node_mut(id).unwrap().styles.push(emulsion_core::styles::LayerStyle::DropShadow{color:[0,0,0],opacity:25.,angle:135.,distance:2.828427,size:0.});
        }
        let b = bounds[key];
        let artwork_start = doc.next_id;
        if let Some(image) = style.get("image") {
            super::images::insert(
                &mut doc,
                id,
                b,
                image,
                &mut image_pixels_remaining,
                warnings,
            )?;
        }
        let named = style
            .get("resIcon")
            .filter(|n| super::vendor::contains(n))
            .or_else(|| style.get("shape").filter(|n| super::vendor::contains(n)));
        if let Some(name) = named {
            let resource = style
                .get("shape")
                .is_some_and(|s| s.ends_with(".resourceIcon"));
            let mut icon_style = style.clone();
            let mut icon_bounds = b;
            if style
                .get("direction")
                .is_some_and(|v| v == "north" || v == "south")
            {
                icon_bounds = [
                    b[0] + (b[2] - b[3]) / 2.,
                    b[1] + (b[3] - b[2]) / 2.,
                    b[3],
                    b[2],
                ];
            }
            if resource {
                icon_style.insert(
                    "fillColor".into(),
                    style
                        .get("strokeColor")
                        .cloned()
                        .unwrap_or("#ffffff".into()),
                );
                icon_style.insert("strokeColor".into(), "none".into());
                icon_bounds = [
                    icon_bounds[0] + icon_bounds[2] * 0.12,
                    icon_bounds[1] + icon_bounds[3] * 0.12,
                    icon_bounds[2] * 0.76,
                    icon_bounds[3] * 0.76,
                ];
            }
            let result =
                super::vendor::svg_at(name, &icon_style, icon_bounds[2], icon_bounds[3], warnings)
                    .and_then(|svg| crate::svg_vectors::append(&mut doc, id, &svg, icon_bounds));
            if let Err(reason) = result {
                warnings.insert(format!("Stencil {name} uses a placeholder: {reason}"));
            } else {
                shape
                    .data
                    .insert("drawio_vendor_stencil".into(), name.clone());
                // The invisible native body retains stable attachment/resize bounds.
                if let NodeKind::Path {
                    style: paint,
                    cache,
                    path,
                } = &mut doc.node_mut(shape.body).unwrap().kind
                {
                    if !resource {
                        paint.fill = None;
                    }
                    paint.stroke = None;
                    *cache = VectorRaster::path(path.clone(), *paint, page.width, page.height);
                }
            }
        }
        let path = if let Some(encoded) = style
            .get("shape")
            .and_then(|s| s.strip_prefix("stencil("))
            .and_then(|s| s.strip_suffix(')'))
        {
            shape.data.insert("drawio_custom_path".into(), "1".into());
            // Keep separate fill/stroke instructions instead of painting every
            // contour with the cell's one default paint.
            if let Ok(xml) = decompress(encoded) {
                let result = super::vendor::inline_svg(&xml, style, warnings)
                    .and_then(|svg| crate::svg_vectors::append(&mut doc, id, &svg, b));
                if result.is_ok()
                    && let NodeKind::Path { style: paint, .. } =
                        &mut doc.node_mut(shape.body).unwrap().kind
                {
                    paint.fill = None;
                    paint.stroke = None;
                }
            }
            match super::stencils::decode(encoded, b, warnings) {
                Ok(path) => Some(path),
                Err(error) => {
                    warnings.insert(format!("An invalid or unsupported inline stencil uses a rectangle placeholder: {error}"));
                    Some(ShapeKind::Process.path(b))
                }
            }
        } else {
            super::shapes::path(style, b)?
        };
        if let Some(path) = path {
            let paint = match doc.node(shape.body).unwrap().kind {
                NodeKind::Path { style, .. } => style,
                _ => unreachable!(),
            };
            doc.node_mut(shape.body).unwrap().kind = Node::path(
                shape.body,
                "Shape",
                Arc::new(path),
                paint,
                doc.width,
                doc.height,
            )
            .kind;
        }
        if shape.kind == ShapeKind::Swimlane {
            let horizontal = style.get("horizontal").is_none_or(|v| v != "0");
            let size =
                number(style, "startSize", 40.)?.clamp(0., if horizontal { b[3] } else { b[2] });
            let header = if horizontal {
                [b[0], b[1], b[2], size]
            } else {
                [b[0], b[1], size, b[3]]
            };
            let paint = match doc.node(shape.body).unwrap().kind {
                NodeKind::Path { style, .. } => style,
                _ => unreachable!(),
            };
            let mut header_node = Node::path(
                doc.alloc_id(),
                "Swimlane header",
                Arc::new(ShapeKind::Process.path(header)),
                paint,
                doc.width,
                doc.height,
            );
            header_node.parent = Some(id);
            let at = doc.nodes.iter().position(|n| n.id == shape.label).unwrap();
            doc.nodes.insert(at, header_node);
            if let Some(value) = style.get("swimlaneFillColor")
                && let Ok(fill) = color(value)
            {
                let (w, h) = (doc.width, doc.height);
                if let NodeKind::Path { path, style, cache } =
                    &mut doc.node_mut(shape.body).unwrap().kind
                {
                    style.fill = fill;
                    *cache = VectorRaster::path(path.clone(), *style, w, h);
                }
            }
        }
        let direction = match style.get("direction").map(String::as_str) {
            Some("south") => 90.,
            Some("west") => 180.,
            Some("north") => 270.,
            _ => 0.,
        };
        let angle = (number(style, "rotation", 0.)? + direction).to_radians();
        let sx = if style.get("flipH").is_some_and(|v| v == "1") {
            -1.
        } else {
            1.
        };
        let sy = if style.get("flipV").is_some_and(|v| v == "1") {
            -1.
        } else {
            1.
        };
        if angle != 0. || sx != 1. || sy != 1. {
            let center = glam::dvec2(b[0] + b[2] / 2., b[1] + b[3] / 2.);
            let transform = glam::DAffine2::from_translation(center)
                * glam::DAffine2::from_angle(angle)
                * glam::DAffine2::from_scale(glam::dvec2(sx, sy))
                * glam::DAffine2::from_translation(-center);
            let (w, h) = (doc.width, doc.height);
            for node in doc.nodes.iter_mut().filter(|n| n.id >= artwork_start) {
                if let NodeKind::Path { path, style, cache } = &mut node.kind {
                    Arc::make_mut(path).transform(transform);
                    *cache = VectorRaster::path(path.clone(), *style, w, h);
                }
            }
        }
        if let NodeKind::Text { spec, cache } = &mut doc.node_mut(shape.label).unwrap().kind {
            let mut text = spec.as_ref().clone();
            let spacing = number(style, "spacing", 2.)?;
            text.x = (b[0] + spacing + number(style, "spacingLeft", 0.)?) as f32;
            text.width = Some(
                (b[2]
                    - spacing * 2.
                    - number(style, "spacingLeft", 0.)?
                    - number(style, "spacingRight", 0.)?)
                .max(1.) as f32,
            );
            let wrap = style.get("whiteSpace").is_some_and(|v| v == "wrap");
            if let Some(width) = style.get("labelWidth") {
                text.width = Some(width.parse::<f32>().ok().filter(|v| v.is_finite() && *v > 0.).ok_or_else(||error("Invalid label width"))?);
            } else if !wrap {
                // mxGraph labels overflow their shape unless wrapping is explicit.
                // In particular, external icon captions must not wrap at a narrow
                // icon width or at width minus a large label spacing offset.
                text.width = None;
                let width = (emulsion_core::text::layout(&text).bounds().width + 2.).max(1.);
                text.width = Some(width);
                let left = number(style, "spacingLeft", 0.)?;
                let right = number(style, "spacingRight", 0.)?;
                text.x = match text.align {
                    emulsion_core::text::Align::Left => b[0] + spacing + left,
                    emulsion_core::text::Align::Right => b[0] + b[2] - spacing - right - width as f64,
                    _ => b[0] + (b[2] - width as f64 + left - right) / 2.,
                } as f32;
            }
            // Measure wrapped lines with the same shaping engine as the canvas.
            // Counting source newlines places wrapped paragraphs too low.
            let measured = emulsion_core::text::layout(&text).bounds();
            let height = (measured.y + measured.height) as f64;
            let top = spacing + number(style, "spacingTop", 0.)?;
            let bottom = spacing + number(style, "spacingBottom", 0.)?;
            let label_height = if shape.kind == ShapeKind::Swimlane
                && style.get("horizontal").is_none_or(|v| v != "0")
            {
                number(style, "startSize", 40.)?.min(b[3])
            } else {
                b[3]
            };
            text.y = match style.get("verticalAlign").map(String::as_str) {
                Some("top") => b[1] + top,
                Some("bottom") => b[1] + label_height - height - bottom,
                _ => b[1] + top + (label_height - top - bottom - height) / 2.,
            } as f32;
            match style.get("verticalLabelPosition").map(String::as_str) {
                Some("bottom") => text.y = (b[1] + b[3] + spacing) as f32,
                Some("top") => text.y = (b[1] - height - spacing) as f32,
                _ => {}
            }
            match style.get("labelPosition").map(String::as_str) {
                Some("left") => text.x -= b[2] as f32,
                Some("right") => text.x += b[2] as f32,
                _ => {}
            }
            if style.get("horizontal").is_some_and(|v| v == "0") {
                let label_width = (b[3] - spacing * 2.).max(1.).max(if wrap {1.} else {text.width.unwrap_or(1.) as f64});
                text.width = Some(label_width as f32);
                text.rotation = -90.;
                let measured = emulsion_core::text::layout(&text).bounds();
                let height = (measured.y + measured.height) as f64;
                let header = if shape.kind == ShapeKind::Swimlane {
                    number(style, "startSize", 40.)?.min(b[2])
                } else {
                    b[2]
                };
                text.x = (b[0] + header / 2. - height / 2.) as f32;
                text.y = (b[1] + b[3] / 2. + label_width / 2.) as f32;
            }
            let rotation=number(style,"rotation",0.)?;
            if rotation!=0. {
                let center=glam::dvec2(b[0]+b[2]/2.,b[1]+b[3]/2.);
                let position=center+glam::DMat2::from_angle(rotation.to_radians())*(glam::dvec2(text.x as f64,text.y as f64)-center);
                text.x=position.x as f32;text.y=position.y as f32;text.rotation+=rotation as f32;
            }
            if style.contains_key("emulsionLabelRotation") {
                text.rotation = number(style, "emulsionLabelRotation", 0.)? as f32;
            }
            if style.contains_key("emulsionLabelX") {
                text.x = (b[0] + number(style, "emulsionLabelX", 0.)?) as f32;
            }
            if style.contains_key("emulsionLabelY") {
                text.y = (b[1] + number(style, "emulsionLabelY", 0.)?) as f32;
            }
            if style.contains_key("emulsionLabelWidth") {
                text.width = Some(number(style, "emulsionLabelWidth", b[2])?.max(1.) as f32);
            }
            *spec = Arc::new(text);
            *cache = VectorRaster::text(spec.clone(), page.width, page.height);
        }
    }
    for (edge_id, edge) in &model.edges {
        for endpoint in [&edge.source, &edge.target] {
            if anchors.contains(&endpoint.shape) {
                // Owning endpoint handles under the connector makes move, copy, and
                // delete operate on the whole loose connector in one transaction.
                doc.node_mut(endpoint.shape).unwrap().parent = Some(*edge_id);
            }
        }
    }
    for id in anchors {
        let shape = model.shapes.get_mut(&id).unwrap();
        shape.data.insert(ANCHOR.into(), "1".into());
        shape.layout_locked = true;
        doc.node_mut(id).unwrap().visible = false;
    }
    for (key, id, mut style, offset) in edges {
        let cell = &cells[&key];
        let edge = model.edges.get_mut(&id).unwrap();
        edge.waypoints = cell
            .points
            .iter()
            .map(|p| (p.0 + offset.0, p.1 + offset.1))
            .collect();
        edge.label_offset = cell.offset;
        edge.arrow_end = style.get("endArrow").is_none_or(|v| v != "none");
        edge.arrow_start = style.get("startArrow").is_some_and(|v| v != "none");
        edge.jump_style = match style.get("jumpStyle").map(String::as_str) { Some("arc")=>diagram::JumpStyle::Arc, Some("gap")=>diagram::JumpStyle::Gap, Some("sharp")=>diagram::JumpStyle::Sharp, _=>diagram::JumpStyle::None };
        edge.corner_radius=if style.get("rounded").is_some_and(|v| v == "1") { number(&style,"arcSize",6.)?.clamp(0.,100.) } else { 0. };
        edge.jump_size=number(&style,"jumpSize",10.)?.clamp(1.,100.);
        for (prefix, marker) in [
            ("start", &mut edge.start_marker),
            ("end", &mut edge.end_marker),
        ] {
            let value = style.get(&format!("{prefix}Arrow")).map_or(
                if prefix == "end" { "classic" } else { "none" },
                String::as_str,
            );
            if let Some(kind) = diagram::MarkerKind::from_drawio(value) {
                marker.kind = kind;
            }
            marker.filled = style.get(&format!("{prefix}Fill")).is_none_or(|v| v != "0");
            marker.size = number(&style, &format!("{prefix}Size"), 6.)?.clamp(1., 100.);
        }
        style
            .entry("strokeColor".into())
            .or_insert("#000000".into());
        style.entry("strokeWidth".into()).or_insert("1".into());
        style.entry("fontColor".into()).or_insert("#000000".into());
        apply_style(&mut doc, edge.path, edge.label, &style, warnings)?;
        // mxGraph fillColor controls arrowheads; open connector paths never fill.
        if let NodeKind::Path { path, style, cache } = &mut doc.node_mut(edge.path).unwrap().kind {
            style.fill = None;
            *cache = VectorRaster::path(path.clone(), *style, page.width, page.height);
        };
        super::labels::apply(&mut doc, edge.label, cell, &style, warnings);
        apply_cell(&mut doc, id, cell, &style)?;
        if let Some(parent) = cell.attrs.get("parent").and_then(|p| ids.get(p)) {
            doc.node_mut(id).unwrap().parent = Some(*parent);
        }
        let mut ancestor = cell.attrs.get("parent");
        while let Some(parent) = ancestor {
            if cells[parent].attrs.get("visible").is_some_and(|v| v == "0") {
                doc.node_mut(id).unwrap().visible = false;
            }
            ancestor = cells[parent].attrs.get("parent");
        }
        ids.insert(key, id);
    }
    doc.diagram = Some(Arc::new(model));
    doc.normalize();
    diagram::synchronize(&Document::new(page.width, page.height), &mut doc).map_err(error)?;
    // Edge-label cells are editable text children, not visible rectangles at the page origin.
    for key in edge_labels {
        let cell = &cells[&key];
        let parent = ids
            .get(&cell.attrs["parent"])
            .copied()
            .ok_or_else(|| error("Missing edge label parent"))?;
        let edge = &doc.diagram.as_ref().unwrap().edges[&parent];
        let label = edge.label;
        let mut node = doc.node(label).unwrap().clone();
        node.id = doc.alloc_id();
        node.parent = Some(parent);
        if let NodeKind::Text { spec, cache } = &mut node.kind {
            let mut text = spec.as_ref().clone();
            text.text = plain_label(
                cell.attrs.get("value").map_or("", String::as_str),
                style(cell).get("html").is_some_and(|v| v == "1"),
                warnings,
            );
            text.x += cell.offset.0 as f32;
            text.y += cell.offset.1 as f32;
            *spec = Arc::new(text);
            *cache = VectorRaster::text(spec.clone(), page.width, page.height);
        }
        let label_id=node.id;
        doc.nodes.push(node);
        apply_style(&mut doc,label_id,label_id,&style(cell),warnings)?;
        super::labels::apply(&mut doc,label_id,cell,&style(cell),warnings);
        let position=number(&cell.geometry,"x",0.)?.clamp(-1.,1.);
        let normal=number(&cell.geometry,"y",0.)?;
        Arc::make_mut(doc.diagram.as_mut().unwrap()).edges.get_mut(&parent).unwrap().labels.push(diagram::EdgeLabel{node:label_id,position,normal,offset:cell.offset});
    }
    doc.normalize();
    diagram::synchronize(&Document::new(page.width,page.height),&mut doc).map_err(error)?;
    // Non-vertex mxCells are named drawing layers, not drawable rectangles.
    let layer_keys=order.iter().filter(|key| {let c=&cells[*key];c.attrs.contains_key("parent") && !c.attrs.contains_key("vertex") && !c.attrs.contains_key("edge") && (c.attrs.get("value").is_some_and(|v|!v.is_empty()) || *key!="1")}).cloned().collect::<Vec<_>>();
    for key in &layer_keys {
        let cell=&cells[key];let id=doc.alloc_id();let mut layer=Node::group(id,cell.attrs.get("value").map_or("Layer",String::as_str));
        layer.visible=cell.attrs.get("visible").is_none_or(|v|v!="0");doc.nodes.push(layer);ids.insert(key.clone(),id);
    }
    for key in &order {
        if let Some(&id)=ids.get(key) && let Some(parent)=cells[key].attrs.get("parent").filter(|p|layer_keys.contains(p)).and_then(|p|ids.get(p)) {
            doc.node_mut(id).unwrap().parent=Some(*parent);
        }
    }
    // Preserve source sibling stacking. Native body/label children stay below
    // nested graph objects; putting every connector behind every shape hides
    // edges under diagram-wide background shapes.
    let order: HashMap<_, _> = order
        .iter()
        .enumerate()
        .filter_map(|(index, key)| ids.get(key).map(|id| (*id, index + 1)))
        .collect();
    doc.nodes
        .sort_by_key(|node| order.get(&node.id).copied().unwrap_or(0));
    doc.normalize();
    doc.validate().map_err(|e| error(e.to_string()))?;
    fit_contents(&mut doc, warnings)?;
    Ok(ProjectPage {
        meta: PageMeta {
            id,
            name: page.name,
            bleed_mm: 0.,
        },
        graph: Graph::new(doc.clone(), "Imported diagram"),
        doc,
    })
}

// mxGraph's paper size is not a clipping rectangle: diagrams can span multiple sheets.
fn fit_contents(doc: &mut Document, warnings: &mut BTreeSet<String>) -> Result<()> {
    let mut extent = [0., 0., doc.width as f64, doc.height as f64];
    for node in &doc.nodes {
        if let NodeKind::Path { path, .. } = &node.kind
            && let Some((x, y, w, h)) = emulsion_raster::vector_geometry::bounds(path)
        {
            extent[0] = extent[0].min(x);
            extent[1] = extent[1].min(y);
            extent[2] = extent[2].max(x + w);
            extent[3] = extent[3].max(y + h);
        }
    }
    let dx = if extent[0] < 0. { -extent[0] + 20. } else { 0. };
    let dy = if extent[1] < 0. { -extent[1] + 20. } else { 0. };
    let width = (extent[2] + dx).ceil();
    let height = (extent[3] + dy).ceil();
    if width > 30000. || height > 30000. {
        warnings.insert(
            "Diagram exceeds the 30,000 pixel canvas limit; some content remains outside the page."
                .into(),
        );
        return Ok(());
    }
    if width as u32 == doc.width && height as u32 == doc.height {
        return Ok(());
    }
    let before = doc.clone();
    doc.width = width as u32;
    doc.height = height as u32;
    let ids = doc
        .nodes
        .iter()
        .filter(|n| n.parent.is_none() && !matches!(n.kind, NodeKind::Fill { .. }))
        .map(|n| n.id)
        .collect::<Vec<_>>();
    emulsion_core::transform::transform_nodes(doc, &ids, [1., 0., 0., 1., dx, dy])
        .map_err(|e| error(e.to_string()))?;
    diagram::synchronize(&before, doc).map_err(error)?;
    for node in &mut doc.nodes {
        match &mut node.kind {
            NodeKind::Path { path, style, cache } => {
                *cache = VectorRaster::path(path.clone(), *style, doc.width, doc.height)
            }
            NodeKind::Text { spec, cache } => {
                *cache = VectorRaster::text(spec.clone(), doc.width, doc.height)
            }
            _ => {}
        }
    }
    doc.validate().map_err(|e| error(e.to_string()))?;
    warnings.insert(
        "Canvas expanded to include drawing content outside the original paper size.".into(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_endpoints_stay_editable_and_invalid_references_are_not_replaced() {
        let xml = r#"<mxGraphModel pageWidth="500" pageHeight="300"><root>
            <mxCell id="0"/><mxCell id="1" parent="0"/>
            <mxCell id="edge" edge="1" parent="1" value="Free connector" style="endArrow=none;strokeColor=#ff0000;">
                <mxGeometry relative="1" as="geometry">
                    <mxPoint x="30" y="50" as="sourcePoint"/>
                    <mxPoint x="230" y="150" as="targetPoint"/>
                </mxGeometry>
            </mxCell></root></mxGraphModel>"#;
        let imported = from_xml(xml).unwrap();
        let doc = &imported.project.pages[0].doc;
        let model = doc.diagram.as_ref().unwrap();
        assert_eq!(model.edges.len(), 1);
        assert_eq!(model.shapes.len(), 2);
        assert!(
            model
                .shapes
                .keys()
                .all(|id| !doc.node(*id).unwrap().visible)
        );
        let edge = model.edges.values().next().unwrap();
        let NodeKind::Path { path, style, .. } = &doc.node(edge.path).unwrap().kind else {
            panic!("connector must be native");
        };
        assert_eq!(style.stroke, Some([255, 0, 0, 255]));
        assert_eq!(path.subpaths[0].anchors[0].p, (30., 50.));
        assert_eq!(path.subpaths[0].anchors.last().unwrap().p, (230., 150.));
        let roundtrip = from_xml(&to_xml(&imported.project).unwrap()).unwrap();
        assert_eq!(
            roundtrip.project.pages[0]
                .doc
                .diagram
                .as_ref()
                .unwrap()
                .edges
                .len(),
            1
        );
        assert!(from_xml(&xml.replace("id=\"edge\"", "id=\"edge\" source=\"missing\"")).is_err());
    }
}
