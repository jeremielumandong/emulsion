//! Brand typography and palette application preserves native editable objects.
use crate::{Command, Document, Editor, NodeId, NodeKind, design_fonts::EmbeddedFont};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TypographyRole {
    pub font: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub line_height: f32,
    pub letter_spacing: f32,
    pub color: Option<[u8; 4]>,
}
impl Default for TypographyRole {
    fn default() -> Self {
        Self {
            font: "Geist".into(),
            size: 24.,
            bold: false,
            italic: false,
            line_height: 1.2,
            letter_spacing: 0.,
            color: None,
        }
    }
}
impl TypographyRole {
    pub fn validate(&self) -> Result<(), String> {
        if self.font.trim().is_empty()
            || self.font.len() > 200
            || !self.size.is_finite()
            || !(1. ..=4000.).contains(&self.size)
            || !self.line_height.is_finite()
            || !(0.5..=4.).contains(&self.line_height)
            || !self.letter_spacing.is_finite()
            || !(-4000. ..=4000.).contains(&self.letter_spacing)
        {
            Err("Typography roles need a font, 1–4000 px size, 0.5–4 line height and −50–500 letter spacing.".into())
        } else {
            Ok(())
        }
    }
    pub fn sample(doc: &Document, id: NodeId) -> Result<Self, String> {
        let NodeKind::Text { spec, .. } = &doc.node(id).ok_or("Select text")?.kind else {
            return Err("Select a native text object.".into());
        };
        Ok(Self {
            font: spec.font.clone(),
            size: spec.size,
            bold: spec.bold,
            italic: spec.italic,
            line_height: spec.line_height,
            letter_spacing: spec.letter_spacing,
            color: Some(spec.color),
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorTarget {
    Fill,
    Stroke,
    Text,
    All,
}
fn selected(doc: &Document, ids: &[NodeId]) -> Result<HashSet<NodeId>, String> {
    if ids.is_empty() || ids.iter().any(|id| doc.node(*id).is_none()) {
        return Err("Select existing objects first.".into());
    }
    Ok(ids.iter().flat_map(|id| doc.subtree(*id)).collect())
}
pub fn extract_colors(doc: &Document, ids: &[NodeId]) -> Result<Vec<[u8; 4]>, String> {
    let selected = selected(doc, ids)?;
    let mut colors = Vec::new();
    let mut push = |color| {
        if colors.len() < 32 && !colors.contains(&color) {
            colors.push(color);
        }
    };
    for node in &doc.nodes {
        if !selected.contains(&node.id) {
            continue;
        }
        match &node.kind {
            NodeKind::Text { spec, .. } => {
                push(spec.color);
                for run in &spec.runs {
                    push(run.style.color);
                }
            }
            NodeKind::Fill { rgba } => push(*rgba),
            NodeKind::Path { style, .. } => {
                for (base, paint) in [
                    (style.fill, style.fill_paint),
                    (style.stroke, style.stroke_paint),
                ] {
                    if let Some(base) = base {
                        push(base);
                        match paint {
                            emulsion_raster::vector::PathPaint::LinearGradient { end, .. }
                            | emulsion_raster::vector::PathPaint::RadialGradient { end } => {
                                push(end)
                            }
                            emulsion_raster::vector::PathPaint::Pattern { secondary, .. } => {
                                push(secondary)
                            }
                            emulsion_raster::vector::PathPaint::LinearStops {
                                stops,
                                count,
                                ..
                            }
                            | emulsion_raster::vector::PathPaint::RadialStops { stops, count } => {
                                for stop in stops.iter().take(usize::from(count.min(16))) {
                                    push(stop.color);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(colors)
}
pub fn apply_color(
    editor: &mut Editor,
    ids: &[NodeId],
    color: [u8; 4],
    target: ColorTarget,
) -> Result<(), String> {
    if editor.in_transaction() {
        return Err("Finish the current edit first.".into());
    }
    let ids = selected(&editor.doc, ids)?;
    let mut trial = Editor::try_new(editor.doc.clone(), None).map_err(|e| e.to_string())?;
    let mut count = 0;
    for id in ids {
        let node = trial.doc.node(id).unwrap();
        let cmd = match &node.kind {
            NodeKind::Path { path, style, .. } if target != ColorTarget::Text => {
                let mut style = *style;
                if matches!(target, ColorTarget::Fill | ColorTarget::All) {
                    style.fill = Some(color);
                    style.fill_paint = emulsion_raster::vector::PathPaint::Solid;
                }
                if matches!(target, ColorTarget::Stroke | ColorTarget::All) {
                    style.stroke = Some(color);
                    style.stroke_paint = emulsion_raster::vector::PathPaint::Solid;
                    style.width = style.width.max(1.);
                }
                Some(Command::SetPath {
                    id,
                    path: path.clone(),
                    style,
                })
            }
            NodeKind::Fill { .. } if matches!(target, ColorTarget::Fill | ColorTarget::All) => {
                Some(Command::SetFillColor { id, rgba: color })
            }
            NodeKind::Text { spec, .. }
                if matches!(target, ColorTarget::Text | ColorTarget::All) =>
            {
                let mut spec = (**spec).clone();
                spec.color = color;
                for run in &mut spec.runs {
                    run.style.color = color;
                }
                Some(Command::SetText {
                    id,
                    spec: Box::new(spec),
                })
            }
            _ => None,
        };
        if let Some(command) = cmd {
            trial.execute(command).map_err(|e| e.to_string())?;
            count += 1;
        }
    }
    if count == 0 {
        return Err("Selection contains no editable objects for that color target.".into());
    }
    editor.commit_design_document(trial.doc, "Apply palette color")
}
pub fn apply_role(
    editor: &mut Editor,
    ids: &[NodeId],
    role: &TypographyRole,
    fonts: &BTreeMap<String, EmbeddedFont>,
) -> Result<(), String> {
    if editor.in_transaction() {
        return Err("Finish the current edit first.".into());
    }
    role.validate()?;
    let ids = selected(&editor.doc, ids)?;
    let mut trial = Editor::try_new(editor.doc.clone(), None).map_err(|e| e.to_string())?;
    if let Some(font) = fonts.get(&role.font) {
        let mut design = trial.doc.design.clone();
        design.fonts.insert(role.font.clone(), font.clone());
        trial
            .execute(Command::SetDesign {
                design: Box::new(design),
            })
            .map_err(|e| e.to_string())?;
    }
    let mut count = 0;
    for id in ids {
        if let NodeKind::Text { spec, .. } = &trial.doc.node(id).unwrap().kind {
            let mut spec = (**spec).clone();
            spec.font = role.font.clone();
            spec.size = role.size;
            spec.bold = role.bold;
            spec.italic = role.italic;
            spec.line_height = role.line_height;
            spec.letter_spacing = role.letter_spacing;
            if let Some(color) = role.color {
                spec.color = color;
            }
            for run in &mut spec.runs {
                run.style.font = role.font.clone();
                run.style.size = role.size;
                run.style.bold = role.bold;
                run.style.italic = role.italic;
                run.style.letter_spacing = role.letter_spacing;
                if let Some(color) = role.color {
                    run.style.color = color;
                }
            }
            trial
                .execute(Command::SetText {
                    id,
                    spec: Box::new(spec),
                })
                .map_err(|e| e.to_string())?;
            count += 1;
        }
    }
    if count == 0 {
        return Err("Select editable text before applying a typography role.".into());
    }
    editor.commit_design_document(trial.doc, "Apply typography role")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Node, command::Slot, text::TextSpec};
    #[test]
    fn brand_roles_palettes_target_only_requested_paint_and_undo_atomically() {
        let mut e = Editor::new(Document::new(400, 300), None);
        let text = e
            .execute(Command::AddNode {
                node: Box::new(Node::text(
                    0,
                    "Title",
                    TextSpec {
                        text: "Keep these words".into(),
                        x: 45.,
                        y: 50.,
                        ..Default::default()
                    },
                    400,
                    300,
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let shape = e
            .execute(Command::AddNode {
                node: Box::new(
                    crate::design::Element::Rectangle.node((400, 300), [10, 20, 30, 255]),
                ),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let before = e.doc.clone();
        let stroke = match &e.doc.node(shape).unwrap().kind {
            NodeKind::Path { style, .. } => style.stroke,
            _ => panic!(),
        };
        apply_color(&mut e, &[shape], [50, 60, 70, 0], ColorTarget::Fill).unwrap();
        assert!(
            matches!(&e.doc.node(shape).unwrap().kind,NodeKind::Path{style,..}if style.fill==Some([50,60,70,0])&&style.stroke==stroke)
        );
        assert!(
            extract_colors(&e.doc, &[shape])
                .unwrap()
                .contains(&[50, 60, 70, 0])
        );
        assert!(e.undo());
        assert_eq!(e.doc, before);
        let role = TypographyRole {
            size: 31.,
            bold: true,
            letter_spacing: 1.25,
            ..Default::default()
        };
        apply_role(&mut e, &[text], &role, &BTreeMap::new()).unwrap();
        assert!(
            matches!(&e.doc.node(text).unwrap().kind,NodeKind::Text{spec,..}if spec.text=="Keep these words"&&spec.size==31.&&spec.x==45.&&spec.y==50.)
        );
        assert!(e.undo());
        assert_eq!(e.doc, before);
        e.execute(Command::SetLocked {
            id: shape,
            locked: true,
        })
        .unwrap();
        let before = e.doc.clone();
        assert!(apply_color(&mut e, &[text, shape], [1, 2, 3, 255], ColorTarget::All).is_err());
        assert_eq!(e.doc, before);
    }
}
