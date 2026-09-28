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
struct CellLayout {
    row: usize,
    col: usize,
    rows: usize,
    cols: usize,
    spec: emulsion_core::text::TextSpec,
    bg: Option<[u8; 4]>,
    border: bool,
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
            let mut spec = super::labels::parse(
                &format!(
                    "<table><tr>{}</tr></table>",
                    String::from_utf8_lossy(&serialized)
                ),
                &base,
                &mut BTreeSet::new(),
            );
            spec.x = 0.;
            spec.y = 0.;
            spec.rotation = 0.;
            spec.width = None;
            spec.height = None;
            let bg = css(child, "background-color")
                .or_else(|| attr(child, "bgcolor"))
                .and_then(|c| color(&c).ok().flatten());
            let border = attr(table, "border").is_some_and(|s| s != "0")
                || css(table, "border").is_some_and(|s| !s.starts_with('0') && s != "none")
                || css(child, "border").is_some_and(|s| !s.starts_with('0') && s != "none");
            cells.push(CellLayout {
                row,
                col,
                rows: rowspan,
                cols: colspan,
                spec,
                bg,
                border,
            });
            columns = columns.max(col + colspan);
            col += colspan;
        }
    }
    if cells.is_empty() {
        return Ok(());
    }
    let padding = attr(table, "cellpadding")
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(4.)
        .clamp(0., 50.);
    let width = css(table, "width")
        .or_else(|| attr(table, "width"))
        .and_then(|s| {
            if let Some(p) = s.strip_suffix('%') {
                p.parse::<f64>().ok().map(|p| b[2] * p / 100.)
            } else {
                s.trim_end_matches("px").parse::<f64>().ok()
            }
        })
        .unwrap_or(b[2])
        .clamp(1., 30000.);
    let mut widths = vec![12.; columns];
    for cell in &cells {
        let preferred =
            f64::from(emulsion_core::text::layout(&cell.spec).bounds().width) + padding * 2.;
        let current = widths[cell.col..cell.col + cell.cols].iter().sum::<f64>();
        let extra = (preferred - current).max(0.) / cell.cols as f64;
        for w in &mut widths[cell.col..cell.col + cell.cols] {
            *w += extra;
        }
    }
    let total = widths.iter().sum::<f64>();
    for w in &mut widths {
        *w *= width / total;
    }
    let mut heights =
        vec![f64::from(base.size) * base.line_height as f64 + padding * 2.; rows.len()];
    for cell in &mut cells {
        cell.spec.width = Some(
            (widths[cell.col..cell.col + cell.cols].iter().sum::<f64>() - 2. * padding).max(1.)
                as f32,
        );
        let required =
            f64::from(emulsion_core::text::layout(&cell.spec).bounds().height) + padding * 2.;
        let current = heights[cell.row..cell.row + cell.rows].iter().sum::<f64>();
        let extra = (required - current).max(0.) / cell.rows as f64;
        for h in &mut heights[cell.row..cell.row + cell.rows] {
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
                    stroke: cell.border.then_some([0, 0, 0, 255]),
                    width: 1.,
                    ..Default::default()
                },
                doc.width,
                doc.height,
            );
            node.parent = Some(parent);
            doc.nodes.push(node);
        }
        cell.spec.x = (cx + padding) as f32;
        cell.spec.y = (cy + padding) as f32;
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
