//! Data-backed charts and tables drawn with native editable text and paths.
use crate::{Command, Document, Editor, Node, NodeId, command::Slot};
use emulsion_raster::{
    vector::{Anchor, Path, PathStyle, SubPath},
    vector_geometry::rectangle,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
#[path = "design_chart_formulas.rs"]
mod formulas;
#[path = "design_chart_plot.rs"]
mod plot;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    Bar,
    Line,
    Pie,
    Table,
    Area,
    Scatter,
    StackedBar,
    Donut,
}
impl Kind {
    pub const ALL: [Self; 8] = [
        Self::Bar,
        Self::Line,
        Self::Pie,
        Self::Table,
        Self::Area,
        Self::Scatter,
        Self::StackedBar,
        Self::Donut,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Bar => "Bar chart",
            Self::Line => "Line chart",
            Self::Pie => "Pie chart",
            Self::Table => "Table",
            Self::Area => "Area chart",
            Self::Scatter => "Scatter plot",
            Self::StackedBar => "Stacked bar",
            Self::Donut => "Donut chart",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Axis {
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// Number of evenly-spaced labelled ticks, including both endpoints.
    pub ticks: u32,
    pub label: String,
    pub show_labels: bool,
}
impl Default for Axis {
    fn default() -> Self {
        Self {
            min: None,
            max: None,
            ticks: 5,
            label: String::new(),
            show_labels: true,
        }
    }
}
impl Axis {
    fn validate(&self) -> Result<(), String> {
        if [self.min, self.max]
            .into_iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1e12)
            || self.min.zip(self.max).is_some_and(|(a, b)| a >= b)
            || !(2..=20).contains(&self.ticks)
            || self.label.chars().count() > 100
        {
            return Err("Axis bounds must be finite within ±1e12, minimum below maximum, 2–20 ticks, and labels up to 100 characters.".into());
        }
        Ok(())
    }
    fn range(&self, natural: (f64, f64)) -> Result<(f64, f64), String> {
        let a = self.min.unwrap_or(natural.0);
        let mut b = self.max.unwrap_or(natural.1);
        if self.max.is_none() && b <= a {
            b = a + a.abs().max(1.) * 0.1;
        }
        let a = if self.min.is_none() && a >= b {
            b - b.abs().max(1.) * 0.1
        } else {
            a
        };
        if a >= b {
            return Err("Axis minimum must be below maximum.".into());
        }
        Ok((a, b))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Merge {
    /// Zero-based coordinates; row 0 is the header. Covered data remains stored.
    pub row: usize,
    pub column: usize,
    pub rows: usize,
    pub columns: usize,
}
impl Merge {
    pub fn contains(&self, row: usize, column: usize) -> bool {
        row >= self.row
            && row < self.row.saturating_add(self.rows)
            && column >= self.column
            && column < self.column.saturating_add(self.columns)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Chart {
    pub kind: Kind,
    pub title: String,
    /// Header row followed by data rows. Numeric chart values use decimal text.
    pub rows: Vec<Vec<String>>,
    pub colors: Vec<[u8; 4]>,
    pub size: (f64, f64),
    #[serde(default)]
    pub x_axis: Axis,
    #[serde(default)]
    pub y_axis: Axis,
    #[serde(default)]
    pub merges: Vec<Merge>,
    #[serde(default)]
    pub formulas: bool,
}
impl Chart {
    pub fn example(kind: Kind) -> Self {
        let mut chart = Self {
            kind,
            title: kind.label().into(),
            rows: vec![
                vec!["Category".into(), "Value".into()],
                vec!["First".into(), "30".into()],
                vec!["Second".into(), "50".into()],
                vec!["Third".into(), "20".into()],
            ],
            colors: vec![
                [80, 110, 235, 255],
                [235, 110, 80, 255],
                [70, 175, 140, 255],
                [180, 105, 220, 255],
                [235, 185, 65, 255],
                [70, 180, 205, 255],
            ],
            size: (600., 400.),
            x_axis: Axis::default(),
            y_axis: Axis::default(),
            merges: Vec::new(),
            formulas: false,
        };
        if kind == Kind::Scatter {
            chart.rows = vec![
                vec!["X".into(), "Y".into()],
                vec!["10".into(), "30".into()],
                vec!["25".into(), "50".into()],
                vec!["45".into(), "20".into()],
            ];
        }
        chart
    }
    pub fn validate(&self) -> Result<(), String> {
        if !(2..=51).contains(&self.rows.len())
            || !(2..=9).contains(&self.rows[0].len())
            || self.rows.iter().any(|row| {
                row.len() != self.rows[0].len()
                    || row.iter().any(|cell| cell.chars().count() > 1000)
            })
        {
            return Err("Use a header and 1–50 rows, with 2–9 columns and at most 1000 characters per cell.".into());
        }
        if self.title.chars().count() > 200
            || self.colors.is_empty()
            || self.colors.len() > 16
            || ![self.size.0, self.size.1]
                .into_iter()
                .all(|v| v.is_finite() && (160. ..=10000.).contains(&v))
        {
            return Err(
                "Choose a title up to 200 characters, 1–16 colors and dimensions of 160–10000 px."
                    .into(),
            );
        }
        self.x_axis.validate()?;
        self.y_axis.validate()?;
        if self.merges.len() > self.rows.len() * self.rows[0].len() {
            return Err("Too many merged cells.".into());
        }
        let mut occupied = std::collections::HashSet::new();
        for m in &self.merges {
            if m.rows == 0
                || m.columns == 0
                || (m.rows == 1 && m.columns == 1)
                || m.row.saturating_add(m.rows) > self.rows.len()
                || m.column.saturating_add(m.columns) > self.rows[0].len()
            {
                return Err(
                    "Merge a rectangular range of at least two cells inside the table.".into(),
                );
            }
            for row in m.row..m.row + m.rows {
                for col in m.column..m.column + m.columns {
                    if !occupied.insert((row, col)) {
                        return Err("Merged cell regions cannot overlap.".into());
                    }
                }
            }
        }
        let rows = self.resolved_rows()?;
        if self.kind != Kind::Table {
            for row in &rows[1..] {
                for value in &row[usize::from(self.kind != Kind::Scatter)..] {
                    let number = value
                        .trim()
                        .parse::<f64>()
                        .map_err(|_| "Chart values must be numbers.")?;
                    if !number.is_finite() || number.abs() > 1e12 {
                        return Err(
                            "Chart values must be finite and between -1e12 and 1e12.".into()
                        );
                    }
                    if matches!(self.kind, Kind::Pie | Kind::Donut) && number < 0. {
                        return Err("Pie values must be nonnegative.".into());
                    }
                }
            }
            if matches!(self.kind, Kind::Pie | Kind::Donut)
                && (self.rows[0].len() != 2
                    || rows[1..]
                        .iter()
                        .map(|r| r[1].trim().parse::<f64>().unwrap())
                        .sum::<f64>()
                        <= 0.)
            {
                return Err(
                    "Pie and donut charts need one value column with a positive total.".into(),
                );
            }
        }
        Ok(())
    }
    pub fn resolved_rows(&self) -> Result<Vec<Vec<String>>, String> {
        if self.rows.is_empty()
            || self.rows[0].is_empty()
            || self.rows.iter().any(|r| r.len() != self.rows[0].len())
        {
            return Err("Formula grid must be rectangular and nonempty.".into());
        }
        if self.formulas {
            formulas::resolve(&self.rows)
        } else {
            Ok(self.rows.clone())
        }
    }
    fn nodes(&self, origin: (f64, f64), canvas: (u32, u32)) -> Result<Vec<Node>, String> {
        self.validate()?;
        let mut resolved = self.clone();
        resolved.rows = self.resolved_rows()?;
        resolved.formulas = false;
        resolved.resolved_nodes(origin, canvas)
    }
    fn resolved_nodes(&self, origin: (f64, f64), canvas: (u32, u32)) -> Result<Vec<Node>, String> {
        if ![origin.0, origin.1]
            .into_iter()
            .all(|v| v.is_finite() && v.abs() <= 1e9)
        {
            return Err("Choose finite chart coordinates within ±1 billion pixels.".into());
        }
        let (x, y) = origin;
        let (w, h) = self.size;
        let mut nodes = Vec::new();
        let shape = |name: &str, path: Path, color| {
            Node::path(
                0,
                name,
                Arc::new(path),
                PathStyle {
                    fill: Some(color),
                    stroke: None,
                    ..Default::default()
                },
                canvas.0,
                canvas.1,
            )
        };
        let text =
            |name: &str, value: String, x: f64, y: f64, width: f64, size: f32, color, bold| {
                Node::text(
                    0,
                    name,
                    crate::text::TextSpec {
                        text: value,
                        x: x as f32,
                        y: y as f32,
                        width: Some(width as f32),
                        height: Some((size * 2.4).max(1.)),
                        size,
                        color,
                        bold,
                        font: "Geist".into(),
                        ..Default::default()
                    },
                    canvas.0,
                    canvas.1,
                )
            };
        let ink = [32, 38, 48, 255];
        nodes.push(shape("Chart background", rectangle(x, y, w, h), [255; 4]));
        nodes.push(text(
            "Chart title",
            self.title.clone(),
            x + 16.,
            y + 12.,
            w - 32.,
            22.,
            ink,
            true,
        ));
        if self.kind == Kind::Table {
            let cw = (w - 32.) / self.rows[0].len() as f64;
            let rh = (h - 60.) / self.rows.len() as f64;
            let font = (rh / 2.6).clamp(6., 18.) as f32;
            for (row, values) in self.rows.iter().enumerate() {
                for (col, value) in values.iter().enumerate() {
                    let merge = self.merges.iter().find(|m| m.contains(row, col));
                    if merge.is_some_and(|m| m.row != row || m.column != col) {
                        continue;
                    }
                    let cell_width = cw * merge.map_or(1, |m| m.columns) as f64;
                    let cell_height = rh * merge.map_or(1, |m| m.rows) as f64;
                    let px = x + 16. + col as f64 * cw;
                    let py = y + 48. + row as f64 * rh;
                    let color = if row == 0 {
                        self.colors[0]
                    } else if row % 2 == 0 {
                        [240, 243, 249, 255]
                    } else {
                        [250, 251, 253, 255]
                    };
                    nodes.push(shape(
                        "Table cell",
                        rectangle(px, py, cell_width - 1., cell_height - 1.),
                        color,
                    ));
                    let mut label = text(
                        "Cell text",
                        value.clone(),
                        px + 4.,
                        py + 3.,
                        (cell_width - 8.).max(1.),
                        font,
                        if row == 0 { [255; 4] } else { ink },
                        row == 0,
                    );
                    if let crate::NodeKind::Text { spec, .. } = &mut label.kind {
                        let mut s = (**spec).clone();
                        s.height = Some((cell_height - 6.).max(1.) as f32);
                        label = Node::text(0, "Cell text", s, canvas.0, canvas.1);
                    }
                    nodes.push(label);
                }
            }
        } else if matches!(self.kind, Kind::Pie | Kind::Donut) {
            let total: f64 = self.rows[1..]
                .iter()
                .map(|r| r[1].trim().parse::<f64>().unwrap())
                .sum();
            let radius = ((w * 0.52).min(h - 80.) / 2.).max(1.);
            let center = (x + radius + 24., y + 55. + radius);
            let mut start = -std::f64::consts::FRAC_PI_2;
            for (i, row) in self.rows[1..].iter().enumerate() {
                let value = row[1].trim().parse::<f64>().unwrap();
                let angle = value / total * std::f64::consts::TAU;
                let color = self.colors[i % self.colors.len()];
                if angle > 0. {
                    let steps = (angle * 32.).ceil().max(1.) as usize;
                    let mut anchors = if self.kind == Kind::Donut {
                        Vec::new()
                    } else {
                        vec![Anchor::corner(center)]
                    };
                    anchors.extend((0..=steps).map(|step| {
                        let a = start + angle * step as f64 / steps as f64;
                        Anchor::corner((center.0 + radius * a.cos(), center.1 + radius * a.sin()))
                    }));
                    if self.kind == Kind::Donut {
                        anchors.extend((0..=steps).rev().map(|step| {
                            let a = start + angle * step as f64 / steps as f64;
                            Anchor::corner((
                                center.0 + radius * 0.55 * a.cos(),
                                center.1 + radius * 0.55 * a.sin(),
                            ))
                        }));
                    }
                    nodes.push(shape(
                        &row[0],
                        Path {
                            subpaths: vec![SubPath {
                                anchors,
                                closed: true,
                            }],
                        },
                        color,
                    ));
                }
                let ly = y + 55. + i as f64 * ((h - 65.) / (self.rows.len() - 1) as f64).min(26.);
                nodes.push(text(
                    "Legend",
                    format!("{} · {:.1}%", row[0], value / total * 100.),
                    x + w * 0.62,
                    ly,
                    w * 0.35,
                    12.,
                    color,
                    false,
                ));
                start += angle;
            }
        } else {
            nodes.extend(plot::draw(self, origin, canvas)?);
        }
        Ok(nodes)
    }
}

/// A data update replaces all generated artwork, so independent pixel, position,
/// and transparency locks on any descendant must be respected as well.
fn check_editable(doc: &Document, id: NodeId) -> Result<(), String> {
    if !doc.design.charts.contains_key(&id) || !doc.node(id).is_some_and(|n| n.is_group()) {
        return Err("Select a data-backed chart or table.".into());
    }
    for node in doc
        .nodes
        .iter()
        .filter(|n| n.id == id || doc.is_ancestor(id, n.id))
    {
        let locks = doc.layer_locks(node.id);
        if doc.locked_ancestor(node.id).is_some()
            || locks.pixels
            || locks.position
            || locks.transparency
        {
            return Err("Unlock the chart and its artwork before editing chart data.".into());
        }
    }
    Ok(())
}

/// Preserve the native artwork and remove only its data association.
pub fn detach(editor: &mut Editor, id: NodeId) -> Result<(), String> {
    check_editable(&editor.doc, id)?;
    let mut design = editor.doc.design.clone();
    design.charts.remove(&id);
    editor
        .execute(Command::SetDesign {
            design: Box::new(design),
        })
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Rebuild data-backed artwork as one transaction. Trial execution guarantees
/// protected descendants and invalid geometry cannot produce a partial chart.
pub fn apply(
    editor: &mut Editor,
    existing: Option<NodeId>,
    chart: Chart,
    origin: (f64, f64),
) -> Result<NodeId, String> {
    if let Some(id) = existing {
        check_editable(&editor.doc, id)?;
    }
    let nodes = chart.nodes(origin, (editor.doc.width, editor.doc.height))?;
    let mut trial = editor.doc.clone();
    let mut commands = Vec::new();
    let mut apply = |command: Command, doc: &mut Document| {
        let result = command.clone().apply(doc).map_err(|e| e.to_string())?;
        commands.push(command);
        Ok::<_, String>(result)
    };
    let group = if let Some(id) = existing {
        if !trial.design.charts.contains_key(&id) || !trial.node(id).is_some_and(|n| n.is_group()) {
            return Err("Select a data-backed chart or table.".into());
        }
        for child in trial.children(Some(id)) {
            apply(Command::RemoveNode { id: child }, &mut trial)?;
        }
        id
    } else {
        apply(
            Command::AddNode {
                node: Box::new(Node::group(0, chart.kind.label())),
                slot: Slot::TOP,
            },
            &mut trial,
        )?
        .ok_or("Missing chart group")?
    };
    for node in nodes {
        apply(
            Command::AddNode {
                node: Box::new(node),
                slot: Slot::top_of(Some(group)),
            },
            &mut trial,
        )?;
    }
    let mut design = trial.design.clone();
    design.charts.insert(group, chart);
    apply(
        Command::SetDesign {
            design: Box::new(design),
        },
        &mut trial,
    )?;
    editor.begin("Edit chart or table");
    for command in commands {
        if let Err(error) = editor.execute(command) {
            editor.cancel();
            return Err(error.to_string());
        }
    }
    editor.end();
    Ok(group)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn charts_are_native_reeditable_and_preserve_links_through_duplicate_and_undo() {
        for kind in Kind::ALL {
            let mut editor = Editor::new(Document::new(800, 600), None);
            let group = apply(&mut editor, None, Chart::example(kind), (20., 30.)).unwrap();
            assert!(editor.doc.nodes.iter().all(|n| matches!(
                n.kind,
                crate::NodeKind::Group { .. }
                    | crate::NodeKind::Path { .. }
                    | crate::NodeKind::Text { .. }
            )));
            let original = editor.doc.clone();
            let copy = editor
                .execute(Command::DuplicateNode { id: group })
                .unwrap()
                .unwrap();
            assert_eq!(
                editor.doc.design.charts[&copy],
                editor.doc.design.charts[&group]
            );
            editor.undo();
            assert_eq!(editor.doc, original);
            let mut changed = Chart::example(kind);
            changed.rows[1][1] = "75".into();
            apply(&mut editor, Some(group), changed, (20., 30.)).unwrap();
            assert_eq!(editor.doc.design.charts[&group].rows[1][1], "75");
            editor.undo();
            assert_eq!(editor.doc, original);
            let fragment = crate::fragment::Fragment::capture(&editor.doc, &[group]).unwrap();
            let mut other = Editor::new(Document::new(800, 600), None);
            fragment.paste(&mut other, Slot::TOP, (0., 0.)).unwrap();
            assert_eq!(other.doc.design.charts.len(), 1);
            let (&pasted, data) = other.doc.design.charts.iter().next().unwrap();
            assert_eq!(data, &original.design.charts[&group]);
            assert!(other.doc.node(pasted).unwrap().is_group());
            other.doc.validate().unwrap();
            other.undo();
            assert!(other.doc.nodes.is_empty());
            assert!(other.doc.design.charts.is_empty());
            other.redo();
            assert_eq!(
                other.doc.design.charts[&pasted],
                original.design.charts[&group]
            );
            editor
                .execute(Command::SetLocked {
                    id: group,
                    locked: true,
                })
                .unwrap();
            let locked = editor.doc.clone();
            assert!(apply(&mut editor, Some(group), Chart::example(kind), (20., 30.)).is_err());
            assert_eq!(editor.doc, locked);
        }
    }
    #[test]
    fn chart_validation_rejects_nonfinite_ragged_and_invalid_pies() {
        for kind in Kind::ALL {
            let mut chart = Chart::example(kind);
            chart.rows[1].pop();
            assert!(chart.validate().is_err());
        }
        for value in ["NaN", "inf", "text", "-1"] {
            let mut chart = Chart::example(Kind::Pie);
            chart.rows[1][1] = value.into();
            assert!(chart.validate().is_err());
        }
    }
    #[test]
    fn fractional_values_fill_plot_and_nonfinite_origins_are_atomic() {
        let mut editor = Editor::new(Document::new(800, 600), None);
        let mut chart = Chart::example(Kind::Bar);
        chart.rows = vec![
            vec!["Label".into(), "Value".into()],
            vec!["A".into(), "0.25".into()],
        ];
        let group = apply(&mut editor, None, chart.clone(), (20., 30.)).unwrap();
        let bar = editor
            .doc
            .nodes
            .iter()
            .find(|node| node.name == "Bar")
            .unwrap();
        let bounds = crate::geometry::node_bounds(&editor.doc, bar.id).unwrap();
        assert!((f64::from(bounds.h) - (chart.size.1 - 150.)).abs() <= 2.);
        let before = editor.doc.clone();
        for origin in [(f64::NAN, 0.), (0., f64::INFINITY), (1e12, 0.)] {
            assert!(apply(&mut editor, Some(group), chart.clone(), origin).is_err());
            assert_eq!(editor.doc, before);
        }
        detach(&mut editor, group).unwrap();
        assert!(editor.doc.design.charts.is_empty());
        assert_eq!(editor.doc.nodes, before.nodes);
        editor.undo();
        assert_eq!(editor.doc, before);
    }

    #[test]
    fn descendant_and_independent_locks_prevent_rebuild_and_detach() {
        for locks in [
            crate::node::LayerLocks {
                pixels: true,
                ..Default::default()
            },
            crate::node::LayerLocks {
                position: true,
                ..Default::default()
            },
            crate::node::LayerLocks {
                transparency: true,
                ..Default::default()
            },
        ] {
            let mut editor = Editor::new(Document::new(800, 600), None);
            let group = apply(&mut editor, None, Chart::example(Kind::Bar), (20., 30.)).unwrap();
            let child = editor.doc.children(Some(group))[0];
            editor
                .execute(Command::SetLayerLocks { id: child, locks })
                .unwrap();
            let before = editor.doc.clone();
            assert!(
                apply(
                    &mut editor,
                    Some(group),
                    Chart::example(Kind::Line),
                    (20., 30.)
                )
                .is_err()
            );
            assert!(detach(&mut editor, group).is_err());
            assert_eq!(editor.doc, before);
            editor.undo();
            editor
                .execute(Command::SetLocked {
                    id: child,
                    locked: true,
                })
                .unwrap();
            let before = editor.doc.clone();
            assert!(
                apply(
                    &mut editor,
                    Some(group),
                    Chart::example(Kind::Bar),
                    (20., 30.)
                )
                .is_err()
            );
            assert_eq!(editor.doc, before);
        }
    }
}

#[cfg(test)]
mod advanced_tests {
    use super::*;
    use crate::NodeKind;
    #[test]
    fn native_advanced_chart_types_and_axes_validate_and_undo() {
        for kind in [Kind::Area, Kind::Scatter, Kind::StackedBar, Kind::Donut] {
            let mut editor = Editor::new(Document::new(800, 600), None);
            let mut chart = Chart::example(kind);
            chart.y_axis = Axis {
                min: Some(10.),
                max: Some(40.),
                ticks: 4,
                label: "Units".into(),
                ..Default::default()
            };
            let id = apply(&mut editor, None, chart.clone(), (10., 20.)).unwrap();
            assert_eq!(editor.doc.design.charts[&id], chart);
            assert!(editor.doc.nodes.iter().all(|n| matches!(
                n.kind,
                NodeKind::Group { .. } | NodeKind::Path { .. } | NodeKind::Text { .. }
            )));
            let before = editor.doc.clone();
            chart.y_axis.max = Some(5.);
            assert!(apply(&mut editor, Some(id), chart, (10., 20.)).is_err());
            assert_eq!(editor.doc, before);
            editor.undo();
            assert!(editor.doc.nodes.is_empty());
        }
        let mut scatter = Chart::example(Kind::Scatter);
        scatter.rows[1][0] = "Category".into();
        assert!(scatter.validate().is_err());
        let mut json = serde_json::to_value(Chart::example(Kind::Bar)).unwrap();
        for key in ["x_axis", "y_axis", "merges"] {
            json.as_object_mut().unwrap().remove(key);
        }
        let old: Chart = serde_json::from_value(json).unwrap();
        assert_eq!(old.x_axis, Axis::default());
        assert!(old.merges.is_empty());
    }
    #[test]
    fn advanced_plot_geometry_has_real_holes_stacks_and_clipped_scatter_data() {
        let mut donut = Chart::example(Kind::Donut);
        donut.rows = vec![
            vec!["Category".into(), "Value".into()],
            vec!["Only".into(), "1".into()],
        ];
        let nodes = donut.nodes((0., 0.), (600, 400)).unwrap();
        let NodeKind::Path { path, style, .. } =
            &nodes.iter().find(|n| n.name == "Only").unwrap().kind
        else {
            panic!()
        };
        let raster = path.rasterize(style, 600, 400);
        assert_eq!(
            raster.get(180, 211)[3],
            0,
            "donut center must be transparent, not a white overlay"
        );
        assert!(raster.get(180, 80)[3] > 60000);
        let mut stacked = Chart::example(Kind::StackedBar);
        stacked.rows = vec![
            vec![
                "Category".into(),
                "One".into(),
                "Two".into(),
                "Three".into(),
            ],
            vec!["A".into(), "10".into(), "-20".into(), "30".into()],
        ];
        let bars = stacked
            .nodes((0., 0.), (600, 400))
            .unwrap()
            .into_iter()
            .filter(|n| n.name == "Bar")
            .map(|n| {
                let NodeKind::Path { path, .. } = n.kind else {
                    panic!()
                };
                emulsion_raster::vector_geometry::bounds(&path).unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(bars.len(), 3);
        assert!(
            bars.iter()
                .all(|b| (b.0 - bars[0].0).abs() < 1e-8 && (b.2 - bars[0].2).abs() < 1e-8)
        );
        assert!(
            (bars.iter().map(|b| b.3).sum::<f64>() - 250.).abs() < 1e-8,
            "positive and negative stacks share a -20..40 domain"
        );
        assert!(
            (bars[0].1 - bars[2].1 - bars[2].3).abs() < 1e-8,
            "positive segments meet without overlapping"
        );
        let mut scatter = Chart::example(Kind::Scatter);
        scatter.rows = vec![
            vec!["X".into(), "Y".into()],
            vec!["-10".into(), "25".into()],
            vec!["50".into(), "25".into()],
            vec!["200".into(), "25".into()],
        ];
        scatter.x_axis.min = Some(0.);
        scatter.x_axis.max = Some(100.);
        let nodes = scatter.nodes((0., 0.), (600, 400)).unwrap();
        assert_eq!(
            nodes.iter().filter(|n| n.name == "Scatter point").count(),
            1
        );
    }

    #[test]
    fn table_merges_preserve_cells_and_invalid_overlaps_are_atomic() {
        let mut editor = Editor::new(Document::new(800, 600), None);
        let table = Chart::example(Kind::Table);
        let id = apply(&mut editor, None, table.clone(), (0., 0.)).unwrap();
        let before = editor.doc.clone();
        let mut merged = table.clone();
        merged.merges.push(Merge {
            row: 0,
            column: 0,
            rows: 1,
            columns: 2,
        });
        apply(&mut editor, Some(id), merged.clone(), (0., 0.)).unwrap();
        assert_eq!(editor.doc.design.charts[&id].rows, table.rows);
        assert_eq!(editor.doc.nodes.len() + 2, before.nodes.len());
        let after = editor.doc.clone();
        merged.merges.push(Merge {
            row: 0,
            column: 1,
            rows: 2,
            columns: 1,
        });
        assert!(apply(&mut editor, Some(id), merged, (0., 0.)).is_err());
        assert_eq!(editor.doc, after);
        editor.undo();
        assert_eq!(editor.doc, before);
        editor.redo();
        assert_eq!(editor.doc, after);
        apply(&mut editor, Some(id), table, (0., 0.)).unwrap();
        assert_eq!(editor.doc.nodes.len(), before.nodes.len());
    }
}

#[cfg(test)]
mod formula_workflow_tests {
    use super::*;
    #[test]
    fn design_chart_formula_updates_native_cells_atomically_and_undo_restores_source() {
        let mut editor = Editor::new(Document::new(600, 400), None);
        let mut chart = Chart::example(Kind::Table);
        chart.formulas = true;
        chart.rows = vec![
            vec!["Item".into(), "Value".into()],
            vec!["A".into(), "10".into()],
            vec!["B".into(), "5".into()],
            vec!["Total".into(), "=SUM(B2:B3)".into()],
        ];
        let id = apply(&mut editor, None, chart.clone(), (0., 0.)).unwrap();
        let before = editor.doc.clone();
        assert!(
            editor
                .doc
                .nodes
                .iter()
                .any(|n| matches!(&n.kind,crate::NodeKind::Text{spec,..}if spec.text=="15"))
        );
        assert_eq!(editor.doc.design.charts[&id].rows[3][1], "=SUM(B2:B3)");
        chart.rows[1][1] = "12".into();
        apply(&mut editor, Some(id), chart.clone(), (0., 0.)).unwrap();
        assert!(
            editor
                .doc
                .nodes
                .iter()
                .any(|n| matches!(&n.kind,crate::NodeKind::Text{spec,..}if spec.text=="17"))
        );
        let changed = editor.doc.clone();
        chart.rows[3][1] = "=B4".into();
        assert!(apply(&mut editor, Some(id), chart, (0., 0.)).is_err());
        assert_eq!(editor.doc, changed);
        editor.undo();
        assert_eq!(editor.doc, before);
        let saved = serde_json::to_string(&editor.doc.design.charts[&id]).unwrap();
        let restored: Chart = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored.resolved_rows().unwrap()[3][1], "15");
        assert!(restored.formulas);
    }
}
