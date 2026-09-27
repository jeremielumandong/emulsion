//! Data-backed charts and tables drawn with native editable text and paths.
use crate::{Command, Document, Editor, Node, NodeId, command::Slot};
use emulsion_raster::{
    vector::{Anchor, Path, PathStyle, SubPath},
    vector_geometry::rectangle,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    Bar,
    Line,
    Pie,
    Table,
}
impl Kind {
    pub const ALL: [Self; 4] = [Self::Bar, Self::Line, Self::Pie, Self::Table];
    pub fn label(self) -> &'static str {
        match self {
            Self::Bar => "Bar chart",
            Self::Line => "Line chart",
            Self::Pie => "Pie chart",
            Self::Table => "Table",
        }
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
}
impl Chart {
    pub fn example(kind: Kind) -> Self {
        Self {
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
        }
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
        if self.kind != Kind::Table {
            for row in &self.rows[1..] {
                for value in &row[1..] {
                    let number = value
                        .trim()
                        .parse::<f64>()
                        .map_err(|_| "Chart values must be numbers.")?;
                    if !number.is_finite() || number.abs() > 1e12 {
                        return Err(
                            "Chart values must be finite and between -1e12 and 1e12.".into()
                        );
                    }
                    if self.kind == Kind::Pie && number < 0. {
                        return Err("Pie values must be nonnegative.".into());
                    }
                }
            }
            if self.kind == Kind::Pie
                && (self.rows[0].len() != 2
                    || self.rows[1..]
                        .iter()
                        .map(|r| r[1].trim().parse::<f64>().unwrap())
                        .sum::<f64>()
                        <= 0.)
            {
                return Err("Pie charts need one value column with a positive total.".into());
            }
        }
        Ok(())
    }
    fn nodes(&self, origin: (f64, f64), canvas: (u32, u32)) -> Result<Vec<Node>, String> {
        self.validate()?;
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
                        rectangle(px, py, cw - 1., rh - 1.),
                        color,
                    ));
                    let mut label = text(
                        "Cell text",
                        value.clone(),
                        px + 4.,
                        py + 3.,
                        (cw - 8.).max(1.),
                        font,
                        if row == 0 { [255; 4] } else { ink },
                        row == 0,
                    );
                    if let crate::NodeKind::Text { spec, .. } = &mut label.kind {
                        let mut s = (**spec).clone();
                        s.height = Some((rh - 6.).max(1.) as f32);
                        label = Node::text(0, "Cell text", s, canvas.0, canvas.1);
                    }
                    nodes.push(label);
                }
            }
        } else if self.kind == Kind::Pie {
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
                    let mut anchors = vec![Anchor::corner(center)];
                    anchors.extend((0..=steps).map(|step| {
                        let a = start + angle * step as f64 / steps as f64;
                        Anchor::corner((center.0 + radius * a.cos(), center.1 + radius * a.sin()))
                    }));
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
            let values: Vec<Vec<f64>> = self.rows[1..]
                .iter()
                .map(|r| r[1..].iter().map(|v| v.trim().parse().unwrap()).collect())
                .collect();
            let min = values.iter().flatten().copied().fold(0., f64::min);
            let max = values.iter().flatten().copied().fold(0., f64::max);
            let span = if max > min { max - min } else { 1. };
            let left = x + 54.;
            let top = y + 66.;
            let pw = w - 74.;
            let ph = h - 125.;
            let py = |v: f64| top + (max - v) / span * ph;
            let zero = py(0.);
            nodes.push(shape(
                "Zero axis",
                rectangle(left, zero, pw, 1.),
                [175, 183, 194, 255],
            ));
            nodes.push(text(
                "Maximum",
                format!("{max}"),
                x + 2.,
                top,
                50.,
                10.,
                ink,
                false,
            ));
            nodes.push(text(
                "Minimum",
                format!("{min}"),
                x + 2.,
                top + ph - 12.,
                50.,
                10.,
                ink,
                false,
            ));
            let step = pw / values.len() as f64;
            let series = self.rows[0].len() - 1;
            for s in 0..series {
                let color = self.colors[s % self.colors.len()];
                nodes.push(text(
                    "Series",
                    self.rows[0][s + 1].clone(),
                    left + s as f64 * pw / series as f64,
                    y + h - 26.,
                    pw / series as f64,
                    11.,
                    color,
                    true,
                ));
                let mut line = Vec::new();
                for (i, row) in values.iter().enumerate() {
                    let cx = left + (i as f64 + 0.5) * step;
                    let vy = py(row[s]);
                    if self.kind == Kind::Bar {
                        let bw = step * 0.8 / series as f64;
                        if row[s] != 0. {
                            nodes.push(shape(
                                "Bar",
                                rectangle(
                                    left + i as f64 * step + step * 0.1 + s as f64 * bw,
                                    vy.min(zero),
                                    bw * 0.9,
                                    (vy - zero).abs(),
                                ),
                                color,
                            ));
                        }
                    } else {
                        line.push(Anchor::corner((cx, vy)));
                        nodes.push(shape(
                            "Data point",
                            emulsion_raster::vector_geometry::ellipse(cx - 3., vy - 3., 6., 6.),
                            color,
                        ));
                    }
                    if s == 0 {
                        nodes.push(text(
                            "Category",
                            self.rows[i + 1][0].clone(),
                            left + i as f64 * step,
                            top + ph + 5.,
                            step,
                            10.,
                            ink,
                            false,
                        ));
                    }
                }
                if self.kind == Kind::Line && line.len() > 1 {
                    nodes.push(Node::path(
                        0,
                        "Series line",
                        Arc::new(Path {
                            subpaths: vec![SubPath {
                                anchors: line,
                                closed: false,
                            }],
                        }),
                        PathStyle {
                            fill: None,
                            stroke: Some(color),
                            width: 2.,
                            ..Default::default()
                        },
                        canvas.0,
                        canvas.1,
                    ));
                }
            }
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
        assert!((f64::from(bounds.h) - (chart.size.1 - 125.)).abs() <= 2.);
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
