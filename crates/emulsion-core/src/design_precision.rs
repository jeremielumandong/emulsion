//! Page measurement units, ruler origins and atomic numeric spacing.
use crate::{Command, Document, Editor, NodeId, NodeKind};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    #[default]
    Pixels,
    Millimeters,
    Inches,
    Points,
}
impl Unit {
    pub const ALL: [Self; 4] = [Self::Pixels, Self::Millimeters, Self::Inches, Self::Points];
    pub fn label(self) -> &'static str {
        match self {
            Self::Pixels => "px",
            Self::Millimeters => "mm",
            Self::Inches => "in",
            Self::Points => "pt",
        }
    }
    pub fn factor(self, dpi: f32) -> f64 {
        match self {
            Self::Pixels => 1.,
            Self::Millimeters => f64::from(dpi) / 25.4,
            Self::Inches => f64::from(dpi),
            Self::Points => f64::from(dpi) / 72.,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub unit: Unit,
    pub origin: [f64; 2],
}
impl Settings {
    pub fn validate(self) -> Result<(), String> {
        if self.origin.iter().any(|v| !v.is_finite() || v.abs() > 1e9) {
            Err("Ruler origins must be finite document pixels within ±1 billion.".into())
        } else {
            Ok(())
        }
    }
}
pub fn set(editor: &mut Editor, settings: Settings) -> Result<(), String> {
    settings.validate()?;
    let mut design = editor.doc.design.clone();
    design.precision = settings;
    commit(
        editor,
        "Page measurement settings",
        vec![Command::SetDesign {
            design: Box::new(design),
        }],
    )
}
fn bounds(doc: &Document, id: NodeId) -> Result<(f64, f64, f64, f64), String> {
    if let Some(b) = crate::design_layout::bounds(doc, id) {
        return Ok(b);
    }
    if let Some(NodeKind::Path { path, .. }) = doc.node(id).map(|n| &n.kind) {
        return emulsion_raster::vector_geometry::bounds(path).ok_or("Empty path.".into());
    }
    crate::geometry::node_bounds(doc, id)
        .map(|b| (b.x as f64, b.y as f64, b.w as f64, b.h as f64))
        .ok_or("Object has no measurable bounds.".into())
}
fn commit(editor: &mut Editor, label: &str, commands: Vec<Command>) -> Result<(), String> {
    if editor.in_transaction() {
        return Err("Finish the current edit first.".into());
    }
    let mut trial = editor.doc.clone();
    for command in &commands {
        command.apply(&mut trial).map_err(|e| e.to_string())?;
    }
    editor.begin(label);
    for command in commands {
        if let Err(e) = editor.execute(command) {
            editor.cancel();
            return Err(e.to_string());
        }
    }
    editor.end();
    Ok(())
}
/// Position the geometric top-left relative to the ruler origin in the selected page unit.
pub fn position(editor: &mut Editor, id: NodeId, x: f64, y: f64) -> Result<(), String> {
    if !x.is_finite() || !y.is_finite() {
        return Err("Position must be finite.".into());
    }
    let factor = editor
        .doc
        .design
        .precision
        .unit
        .factor(editor.doc.resolution);
    let origin = editor.doc.design.precision.origin;
    let (x0, y0, _, _) = bounds(&editor.doc, id)?;
    let (dx, dy) = (x * factor + origin[0] - x0, y * factor + origin[1] - y0);
    if dx.abs() > 1e9 || dy.abs() > 1e9 {
        return Err("Position exceeds document coordinate limits.".into());
    }
    commit(
        editor,
        "Position object",
        vec![Command::TranslateNode { id, dx, dy }],
    )
}
/// Set an exact edge-to-edge gap in page units; first object by geometric order stays fixed.
pub fn spacing(
    editor: &mut Editor,
    ids: &[NodeId],
    vertical: bool,
    gap: f64,
) -> Result<(), String> {
    if ids.len() < 2
        || ids.len() > 1000
        || ids.iter().collect::<std::collections::HashSet<_>>().len() != ids.len()
    {
        return Err("Choose 2–1000 unique objects.".into());
    }
    if !gap.is_finite() || gap.abs() > 100000. {
        return Err("Spacing must be finite within ±100000 page units.".into());
    }
    let gap = gap
        * editor
            .doc
            .design
            .precision
            .unit
            .factor(editor.doc.resolution);
    let mut items = Vec::new();
    let selected: std::collections::HashSet<_> = ids.iter().copied().collect();
    for id in ids {
        let mut ancestor = editor.doc.node(*id).and_then(|n| n.parent);
        while let Some(parent) = ancestor {
            if selected.contains(&parent) {
                return Err("Select independent objects, not an ancestor and its child.".into());
            }
            ancestor = editor.doc.node(parent).and_then(|n| n.parent);
        }
        let (x, y, w, h) = bounds(&editor.doc, *id)?;
        items.push((
            *id,
            if vertical { y } else { x },
            if vertical { h } else { w },
        ));
    }
    items.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    let mut edge = items[0].1 + items[0].2;
    let mut commands = Vec::new();
    for &(id, start, length) in &items[1..] {
        let delta = edge + gap - start;
        if delta.abs() > 1e9 {
            return Err("Spacing exceeds document coordinate limits.".into());
        }
        commands.push(Command::TranslateNode {
            id,
            dx: if vertical { 0. } else { delta },
            dy: if vertical { delta } else { 0. },
        });
        edge += gap + length;
    }
    commit(editor, "Space objects", commands)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Node, command::Slot};
    use std::sync::Arc;
    #[test]
    fn measurement_units_origin_spacing_and_locks_are_atomic() {
        let mut e = Editor::new(Document::new(500, 500), None);
        e.doc.resolution = 254.;
        let mut ids = Vec::new();
        for x in [0., 30., 60.] {
            ids.push(
                e.execute(Command::AddNode {
                    node: Box::new(Node::path(
                        0,
                        "Box",
                        Arc::new(emulsion_raster::vector_geometry::rectangle(x, 0., 10., 10.)),
                        Default::default(),
                        500,
                        500,
                    )),
                    slot: Slot::TOP,
                })
                .unwrap()
                .unwrap(),
            );
        }
        set(
            &mut e,
            Settings {
                unit: Unit::Millimeters,
                origin: [20., 30.],
            },
        )
        .unwrap();
        let before = e.doc.clone();
        position(&mut e, ids[0], 2., 3.).unwrap();
        assert_eq!(bounds(&e.doc, ids[0]).unwrap().0, 40.);
        e.undo();
        assert_eq!(e.doc, before);
        spacing(&mut e, &ids, false, 1.).unwrap();
        assert_eq!(bounds(&e.doc, ids[1]).unwrap().0, 20.);
        assert_eq!(bounds(&e.doc, ids[2]).unwrap().0, 40.);
        e.execute(Command::SetLocked {
            id: ids[2],
            locked: true,
        })
        .unwrap();
        let before = e.doc.clone();
        assert!(spacing(&mut e, &ids, false, 4.).is_err());
        assert_eq!(e.doc, before);
        assert_eq!(Unit::Points.factor(72.), 1.);
    }
}
