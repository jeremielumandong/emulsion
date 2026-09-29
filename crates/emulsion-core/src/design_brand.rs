//! Reusable brand application keeps text, paths and source images editable.
use crate::{Command, Document, Editor, NodeId, NodeKind};
pub fn commands(
    doc: &Document,
    ids: &[NodeId],
    font: &str,
    colors: &[[u8; 4]],
) -> Result<Vec<Command>, String> {
    if font.trim().is_empty() || font.len() > 200 || colors.is_empty() || colors.len() > 32 {
        return Err("Choose a font and 1–32 brand colors.".into());
    }
    let selected = ids
        .iter()
        .flat_map(|id| doc.subtree(*id))
        .collect::<std::collections::HashSet<_>>();
    let mut commands = Vec::new();
    let mut shape = 0;
    for node in &doc.nodes {
        if !selected.contains(&node.id) {
            continue;
        }
        match &node.kind {
            NodeKind::Text { spec, .. } => {
                let mut spec = (**spec).clone();
                spec.font = font.into();
                spec.color = colors[0];
                for run in &mut spec.runs {
                    run.style.font = font.into();
                    run.style.color = colors[0];
                }
                commands.push(Command::SetText {
                    id: node.id,
                    spec: Box::new(spec),
                });
            }
            NodeKind::Path { path, style, .. } => {
                let mut style = *style;
                let index = if colors.len() > 1 {
                    1 + shape % (colors.len() - 1)
                } else {
                    0
                };
                let color = colors[index];
                shape += 1;
                if style.fill.is_some() {
                    style.fill = Some(color);
                    style.fill_paint = emulsion_raster::vector::PathPaint::Solid;
                }
                if style.stroke.is_some() {
                    style.stroke = Some(colors[0]);
                    style.stroke_paint = emulsion_raster::vector::PathPaint::Solid;
                }
                commands.push(Command::SetPath {
                    id: node.id,
                    path: path.clone(),
                    style,
                });
            }
            _ => {}
        }
    }
    // Locked or invalid objects reject the complete operation before editing.
    let mut trial = doc.clone();
    for command in &commands {
        command.apply(&mut trial).map_err(|e| e.to_string())?;
    }
    Ok(commands)
}
pub fn apply(
    editor: &mut Editor,
    ids: &[NodeId],
    font: &str,
    colors: &[[u8; 4]],
) -> Result<(), String> {
    if editor.in_transaction() {
        return Err("Finish the current edit first.".into());
    }
    let commands = commands(&editor.doc, ids, font, colors)?;
    editor.begin("Apply brand kit");
    for command in commands {
        if let Err(e) = editor.execute(command) {
            editor.cancel();
            return Err(e.to_string());
        }
    }
    editor.end();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        command::Slot,
        design::{Element, Template},
    };
    #[test]
    fn applying_brand_is_one_undoable_edit_and_never_flattens_artwork() {
        let mut e = Editor::new(Template::Announcement.create(600, 400).unwrap(), None);
        let id = e
            .execute(Command::AddNode {
                node: Box::new(Element::Star.node((600, 400), [255; 4])),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let before = e.doc.clone();
        let ids = e.doc.children(None);
        apply(
            &mut e,
            &ids,
            "Geist Mono",
            &[[10, 20, 30, 255], [80, 160, 210, 255]],
        )
        .unwrap();
        assert!(
            e.doc
                .nodes
                .iter()
                .filter_map(|n| if let NodeKind::Text { spec, .. } = &n.kind {
                    Some(spec)
                } else {
                    None
                })
                .all(|spec| spec.font == "Geist Mono" && spec.color == [10, 20, 30, 255])
        );
        assert!(
            e.doc
                .nodes
                .iter()
                .all(|n| !matches!(n.kind, NodeKind::Raster { .. }))
        );
        e.undo();
        assert_eq!(e.doc, before);
        e.execute(Command::SetLocked { id, locked: true }).unwrap();
        let locked = e.doc.clone();
        assert!(apply(&mut e, &ids, "Geist", &[[1, 2, 3, 255]]).is_err());
        assert_eq!(e.doc, locked);
    }
}
