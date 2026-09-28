//! Bounded HTML table layout into editable cell text and vector backgrounds.
use super::*;
use emulsion_core::vector_cache::VectorRaster;
use html5ever::{parse_document, tendril::TendrilSink};
use markup5ever_rcdom::{Handle, NodeData, RcDom, SerializableHandle};
fn tag(node: &Handle) -> &str {
    if let NodeData::Element { name, .. } = &node.data {
        name.local.as_ref()
    } else {
        ""
    }
}
fn attr(node: &Handle, key: &str) -> Option<String> {
    if let NodeData::Element { attrs, .. } = &node.data {
        attrs
            .borrow()
            .iter()
            .find(|a| a.name.local.as_ref() == key)
            .map(|a| a.value.to_string())
    } else {
        None
    }
}
fn css(node: &Handle, key: &str) -> Option<String> {
    attr(node, "style")?
        .split(';')
        .filter_map(|s| s.split_once(':'))
        .find(|(k, _)| k.trim() == key)
        .map(|(_, v)| v.trim().into())
}
fn descendants(node: &Handle, name: &str, depth: usize, out: &mut Vec<Handle>) {
    if depth > 64 {
        return;
    }
    if tag(node) == name {
        out.push(node.clone());
        return;
    }
    for child in node.children.borrow().iter() {
        descendants(child, name, depth + 1, out);
    }
}
fn dimension(value: &str, available: f64) -> Option<f64> {
    let value = value.trim();
    let n = if let Some(p) = value.strip_suffix('%') {
        p.parse::<f64>().ok()? * available / 100.
    } else {
        value.trim_end_matches("px").parse::<f64>().ok()?
    };
    (n.is_finite() && n >= 0.).then_some(n.min(30000.))
}
fn padding(node: &Handle, fallback: f64) -> [f64; 4] {
    let mut result = [fallback; 4];
    if let Some(value) = css(node, "padding") {
        let values = value
            .split_whitespace()
            .map(|v| dimension(v, 0.))
            .collect::<Option<Vec<_>>>();
        if let Some(v) = values {
            result = match v.as_slice() {
                [a] => [*a; 4],
                [a, b] => [*a, *b, *a, *b],
                [a, b, c] => [*a, *b, *c, *b],
                [a, b, c, d] => [*a, *b, *c, *d],
                _ => result,
            };
        }
    }
    for (i, key) in [
        "padding-top",
        "padding-right",
        "padding-bottom",
        "padding-left",
    ]
    .iter()
    .enumerate()
    {
        if let Some(v) = css(node, key).and_then(|v| dimension(&v, 0.)) {
            result[i] = v;
        }
    }
    result.map(|v| v.clamp(0., 100.))
}
struct CellLayout {
    row: usize,
    col: usize,
    rows: usize,
    cols: usize,
    spec: emulsion_core::text::TextSpec,
    bg: Option<[u8; 4]>,
    border: bool,
    border_color: [u8; 4],
    border_width: f32,
    border_dash: Vec<f32>,
    padding: [f64; 4],
    width: Option<String>,
    height: Option<f64>,
    vertical: String,
}
pub(super) fn append(
    doc: &mut Document,
    parent: NodeId,
    label: NodeId,
    html: &str,
    b: [f64; 4],
    style: &BTreeMap<String, String>,
    warnings: &mut BTreeSet<String>,
) -> Result<()> {
    if !html.to_ascii_lowercase().contains("<table") {
        return Ok(());
    }
    let dom = parse_document(RcDom::default(), Default::default()).one(html);
    let mut tables = Vec::new();
    descendants(&dom.document, "table", 0, &mut tables);
    if tables.len() != 1 {
        return Ok(());
    }
    let table = &tables[0];
    let mut rows = Vec::new();
    descendants(table, "tr", 0, &mut rows);
    if rows.is_empty() || rows.len() > 128 {
        return Ok(());
    }
    let base = match &doc
        .node(label)
        .ok_or_else(|| error("Missing table label"))?
        .kind
    {
        NodeKind::Text { spec, .. } => (**spec).clone(),
        _ => return Ok(()),
    };
    let default_padding = attr(table, "cellpadding")
        .and_then(|s| dimension(&s, 0.))
        .unwrap_or(4.)
        .clamp(0., 50.);
    let mut cells = Vec::new();
    let mut occupied = HashSet::new();
    let mut columns = 0usize;
    for (row, tr) in rows.iter().enumerate() {
        let mut col = 0;
        for child in tr
            .children
            .borrow()
            .iter()
            .filter(|c| matches!(tag(c), "td" | "th"))
        {
            while occupied.contains(&(row, col)) {
                col += 1;
            }
            let colspan = attr(child, "colspan")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, 32);
            let rowspan = attr(child, "rowspan")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, rows.len() - row);
            if col + colspan > 64 || cells.len() >= 512 {
                return Ok(());
            }
            for r in row..row + rowspan {
                for c in col..col + colspan {
                    occupied.insert((r, c));
                }
            }
            let mut serialized = Vec::new();
            html5ever::serialize(
                &mut serialized,
                &SerializableHandle::from(child.clone()),
                html5ever::serialize::SerializeOpts {
                    traversal_scope: html5ever::serialize::TraversalScope::IncludeNode,
                    ..Default::default()
                },
            )?;
            let mut cell_base = base.clone();
            cell_base.align = if tag(child) == "th" {
                emulsion_core::text::Align::Center
            } else {
                emulsion_core::text::Align::Left
            };
            cell_base.paragraphs.clear();
            let mut spec = super::labels::parse(
                &format!(
                    "<table><tr>{}</tr></table>",
                    String::from_utf8_lossy(&serialized)
                ),
                &cell_base,
                &mut BTreeSet::new(),
            );
            spec.x = 0.;
            spec.y = 0.;
            spec.rotation = 0.;
            spec.width = None;
            spec.height = None;
            let bg = css(child, "background-color")
                .or_else(|| attr(child, "bgcolor"))
                .or_else(|| css(tr, "background-color"))
                .or_else(|| css(table, "background-color"))
                .and_then(|c| color(&c).ok().flatten());
            let border = attr(table, "border").is_some_and(|s| s != "0")
                || css(table, "border").is_some_and(|s| !s.starts_with('0') && s != "none")
                || css(child, "border").is_some_and(|s| !s.starts_with('0') && s != "none");
            let border_css = css(child, "border")
                .or_else(|| css(table, "border"))
                .unwrap_or_default();
            let border_tokens = border_css.split_whitespace().collect::<Vec<_>>();
            let border_color = css(child, "border-color")
                .or_else(|| css(table, "border-color"))
                .and_then(|s| color(&s).ok().flatten())
                .or_else(|| border_tokens.iter().find_map(|s| color(s).ok().flatten()))
                .unwrap_or([0, 0, 0, 255]);
            let border_width = css(child, "border-width")
                .or_else(|| css(table, "border-width"))
                .and_then(|s| dimension(&s, 0.))
                .or_else(|| border_tokens.iter().find_map(|s| dimension(s, 0.)))
                .unwrap_or(1.)
                .min(20.) as f32;
            let border_style = css(child, "border-style").or_else(|| css(table, "border-style"));
            let dashed =
                border_style.as_deref() == Some("dashed") || border_tokens.contains(&"dashed");
            let dotted =
                border_style.as_deref() == Some("dotted") || border_tokens.contains(&"dotted");
            let border_dash = if dashed {
                vec![4. * border_width, 3. * border_width]
            } else if dotted {
                vec![border_width, 2. * border_width]
            } else {
                vec![]
            };
            let border = border
                && border_width > 0.
                && !matches!(border_style.as_deref(), Some("none" | "hidden"));
            if let Some(align) = css(child, "text-align")
                .or_else(|| attr(child, "align"))
                .or_else(|| css(tr, "text-align"))
                .or_else(|| css(table, "text-align"))
            {
                spec.align = match align.as_str() {
                    "right" => emulsion_core::text::Align::Right,
                    "center" => emulsion_core::text::Align::Center,
                    _ => emulsion_core::text::Align::Left,
                };
            }
            cells.push(CellLayout {
                padding: padding(child, default_padding),
                width: css(child, "width").or_else(|| attr(child, "width")),
                height: css(child, "height")
                    .or_else(|| attr(child, "height"))
                    .or_else(|| css(tr, "height"))
                    .and_then(|v| dimension(&v, b[3])),
                vertical: css(child, "vertical-align")
                    .or_else(|| attr(child, "valign"))
                    .unwrap_or_else(|| "middle".into()),
                row,
                col,
                rows: rowspan,
                cols: colspan,
                spec,
                bg,
                border,
                border_color,
                border_width,
                border_dash,
            });
            columns = columns.max(col + colspan);
            col += colspan;
        }
    }
    if cells.is_empty() {
        return Ok(());
    }
    let width = css(table, "width")
        .or_else(|| attr(table, "width"))
        .and_then(|v| dimension(&v, b[2]))
        .unwrap_or(b[2])
        .clamp(1., 30000.);
    let mut widths = vec![12.; columns];
    let mut explicit = vec![false; columns];
    let mut cols = Vec::new();
    descendants(table, "col", 0, &mut cols);
    let mut column = 0;
    for col in cols {
        let span = attr(&col, "span")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(1)
            .clamp(1, 64);
        let size = css(&col, "width")
            .or_else(|| attr(&col, "width"))
            .and_then(|v| dimension(&v, width));
        for i in column..(column + span).min(columns) {
            if let Some(size) = size {
                widths[i] = size.max(1.);
                explicit[i] = true;
            }
        }
        column += span;
    }
    for cell in &cells {
        if let Some(size) = cell.width.as_ref().and_then(|v| dimension(v, width)) {
            for i in cell.col..cell.col + cell.cols {
                widths[i] = (size / cell.cols as f64).max(1.);
                explicit[i] = true;
            }
        }
    }
    for cell in cells
        .iter()
        .filter(|_| css(table, "table-layout").as_deref() != Some("fixed"))
    {
        let preferred = f64::from(emulsion_core::text::layout(&cell.spec).bounds().width)
            + cell.padding[1]
            + cell.padding[3];
        let current = widths[cell.col..cell.col + cell.cols].iter().sum::<f64>();
        let flexible = (cell.col..cell.col + cell.cols)
            .filter(|i| !explicit[*i])
            .count();
        if flexible > 0 {
            let extra = (preferred - current).max(0.) / flexible as f64;
            for i in cell.col..cell.col + cell.cols {
                if !explicit[i] {
                    widths[i] += extra;
                }
            }
        }
    }
    let total = widths.iter().sum::<f64>();
    let fixed = widths
        .iter()
        .zip(&explicit)
        .filter(|(_, e)| **e)
        .map(|(w, _)| *w)
        .sum::<f64>();
    if fixed < width && explicit.iter().any(|e| !*e) {
        let flexible = total - fixed;
        for (w, e) in widths.iter_mut().zip(&explicit) {
            if !e {
                *w *= (width - fixed) / flexible;
            }
        }
    } else {
        for w in &mut widths {
            *w *= width / total;
        }
    }
    let mut heights =
        vec![f64::from(base.size) * base.line_height as f64 + default_padding * 2.; rows.len()];
    for cell in &mut cells {
        cell.spec.width = Some(
            (widths[cell.col..cell.col + cell.cols].iter().sum::<f64>()
                - cell.padding[1]
                - cell.padding[3])
                .max(1.) as f32,
        );
        let required = (f64::from(emulsion_core::text::layout(&cell.spec).bounds().height)
            + cell.padding[0]
            + cell.padding[2])
            .max(cell.height.unwrap_or(0.));
        let current = heights[cell.row..cell.row + cell.rows].iter().sum::<f64>();
        let extra = (required - current).max(0.) / cell.rows as f64;
        for h in &mut heights[cell.row..cell.row + cell.rows] {
            *h += extra;
        }
    }
    if let Some(requested) = css(table, "height")
        .or_else(|| attr(table, "height"))
        .and_then(|s| dimension(&s, b[3]))
    {
        let extra = (requested - heights.iter().sum::<f64>()).max(0.) / heights.len() as f64;
        for h in &mut heights {
            *h += extra;
        }
    }
    let height = heights.iter().sum::<f64>();
    let x = b[0]
        + match style.get("align").map(String::as_str) {
            Some("left") => 0.,
            Some("right") => b[2] - width,
            _ => (b[2] - width) / 2.,
        };
    let y = b[1]
        + match style.get("verticalAlign").map(String::as_str) {
            Some("top") => 0.,
            Some("bottom") => b[3] - height,
            _ => (b[3] - height) / 2.,
        };
    let first = doc.next_id;
    for mut cell in cells {
        let cx = x + widths[..cell.col].iter().sum::<f64>();
        let cy = y + heights[..cell.row].iter().sum::<f64>();
        let w = widths[cell.col..cell.col + cell.cols].iter().sum::<f64>();
        let h = heights[cell.row..cell.row + cell.rows].iter().sum::<f64>();
        if cell.bg.is_some() || cell.border {
            let id = doc.alloc_id();
            let mut node = Node::path(
                id,
                "Table cell",
                Arc::new(emulsion_raster::vector_geometry::rectangle(cx, cy, w, h)),
                emulsion_raster::vector::PathStyle {
                    fill: cell.bg,
                    stroke: cell.border.then_some(cell.border_color),
                    width: cell.border_width,
                    dash: {
                        let mut dash = [0.; 6];
                        dash[..cell.border_dash.len()].copy_from_slice(&cell.border_dash);
                        dash
                    },
                    dash_count: cell.border_dash.len() as u8,
                    ..Default::default()
                },
                doc.width,
                doc.height,
            );
            node.parent = Some(parent);
            doc.nodes.push(node);
        }
        cell.spec.x = (cx + cell.padding[3]) as f32;
        let content = f64::from(emulsion_core::text::layout(&cell.spec).bounds().height);
        let spare = (h - cell.padding[0] - cell.padding[2] - content).max(0.);
        cell.spec.y = (cy
            + cell.padding[0]
            + match cell.vertical.as_str() {
                "top" | "baseline" => 0.,
                "bottom" => spare,
                _ => spare / 2.,
            }) as f32;
        let id = doc.alloc_id();
        let mut node = Node::text(id, "Table cell text", cell.spec, doc.width, doc.height);
        node.parent = Some(parent);
        doc.nodes.push(node);
    }
    let rotation = number(style, "rotation", 0.)?;
    if rotation != 0. {
        let center = glam::dvec2(b[0] + b[2] / 2., b[1] + b[3] / 2.);
        let t = glam::DAffine2::from_translation(center)
            * glam::DAffine2::from_angle(rotation.to_radians())
            * glam::DAffine2::from_translation(-center);
        let ids = doc
            .nodes
            .iter()
            .filter(|n| n.id >= first)
            .map(|n| n.id)
            .collect::<Vec<_>>();
        emulsion_core::transform::transform_nodes(doc, &ids, t.to_cols_array())
            .map_err(|e| error(e.to_string()))?;
    }
    let (w, h) = (doc.width, doc.height);
    if let NodeKind::Text { spec, cache } = &mut doc.node_mut(label).unwrap().kind {
        let s = Arc::make_mut(spec);
        s.text.clear();
        s.runs.clear();
        s.paragraphs.clear();
        *cache = VectorRaster::text(spec.clone(), w, h);
    }
    warnings.insert("HTML table imported as editable cells with column spans, row spans and wrapped text; browser CSS outside supported cell styling may differ.".into());
    Ok(())
}
