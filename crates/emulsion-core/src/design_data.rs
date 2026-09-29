//! Persisted local data bindings; generation resolves data outside the document.
use crate::{Command, Document, Editor, NodeId, NodeKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    #[default]
    Cover,
    Contain,
    Stretch,
}
impl Fit {
    pub fn native(self) -> crate::design::media::ImageFit {
        match self {
            Self::Cover => crate::design::media::ImageFit::Cover,
            Self::Contain => crate::design::media::ImageFit::Contain,
            Self::Stretch => crate::design::media::ImageFit::Stretch,
        }
    }
}
fn center() -> [f64; 2] {
    [0.5; 2]
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Binding {
    Text {
        column: String,
    },
    Image {
        column: String,
        #[serde(default)]
        fit: Fit,
        #[serde(default = "center")]
        focus: [f64; 2],
    },
}
impl Binding {
    pub fn column(&self) -> &str {
        match self {
            Self::Text { column } | Self::Image { column, .. } => column,
        }
    }
    fn validate(&self) -> Result<(), String> {
        let name = self.column();
        if name.trim().is_empty()
            || name != name.trim()
            || name.len() > 200
            || name.chars().any(char::is_control)
        {
            return Err(
                "CSV column names must be 1–200 bytes without surrounding whitespace.".into(),
            );
        }
        if let Self::Image { focus, .. } = self
            && focus
                .iter()
                .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        {
            return Err("Image focus coordinates must be between 0 and 1.".into());
        }
        Ok(())
    }
}
pub fn validate(bindings: &BTreeMap<NodeId, Binding>, doc: &Document) -> Result<(), String> {
    if bindings.len() > 1024 {
        return Err("A page supports at most 1024 data bindings.".into());
    }
    for (id, binding) in bindings {
        binding.validate()?;
        let Some(node) = doc.node(*id) else {
            return Err("A data-bound object is missing.".into());
        };
        match (binding, &node.kind) {
            (Binding::Text { .. }, NodeKind::Text { .. })
            | (Binding::Image { .. }, NodeKind::Raster { .. }) => (),
            _ => return Err("Bind text to a text object or images to a raster image.".into()),
        }
        if doc.raw.as_ref().is_some_and(|raw| raw.node_id == *id) {
            return Err(
                "Develop the RAW source into a regular Design image before binding CSV data."
                    .into(),
            );
        }
    }
    Ok(())
}
pub fn target(doc: &Document, id: NodeId) -> NodeId {
    if doc.node(id).is_some_and(|n| n.is_group()) {
        crate::design::frame_parts(doc, id)
            .and_then(|(_, image)| image)
            .unwrap_or(id)
    } else {
        id
    }
}
pub fn set(editor: &mut Editor, id: NodeId, binding: Option<Binding>) -> Result<NodeId, String> {
    if editor.in_transaction() {
        return Err("Finish the current edit before changing data bindings.".into());
    }
    let id = target(&editor.doc, id);
    let locks = editor.doc.layer_locks(id);
    if editor.doc.node(id).is_none()
        || editor.doc.locked_ancestor(id).is_some()
        || locks.pixels
        || locks.position
        || locks.transparency
    {
        return Err("Unlock the data-bound object first.".into());
    }
    let mut design = editor.doc.design.clone();
    match binding {
        Some(binding) => {
            binding.validate()?;
            design.data_bindings.insert(id, binding);
        }
        None => {
            design.data_bindings.remove(&id);
        }
    }
    editor
        .execute(Command::SetDesign {
            design: Box::new(design),
        })
        .map_err(|e| e.to_string())?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Node, command::Slot, fragment::Fragment, text::TextSpec};
    #[test]
    fn data_bindings_follow_duplicate_clipboard_and_undo_and_reject_invalid_targets() {
        let mut e = Editor::new(Document::new(300, 200), None);
        let id = e
            .execute(Command::AddNode {
                node: Box::new(Node::text(
                    0,
                    "Name",
                    TextSpec {
                        text: "Name".into(),
                        ..Default::default()
                    },
                    300,
                    200,
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let binding = Binding::Text {
            column: "name".into(),
        };
        let original = e.doc.clone();
        set(&mut e, id, Some(binding.clone())).unwrap();
        e.undo();
        assert_eq!(e.doc, original);
        e.redo();
        let copy = e.execute(Command::DuplicateNode { id }).unwrap().unwrap();
        assert_eq!(e.doc.design.data_bindings[&copy], binding);
        let fragment = Fragment::capture(&e.doc, &[copy]).unwrap();
        let mut target = Editor::new(Document::new(300, 200), None);
        let pasted = fragment.paste(&mut target, Slot::TOP, (0., 0.)).unwrap();
        assert_eq!(target.doc.design.data_bindings[&pasted[0]], binding);
        let before = target.doc.clone();
        let history = target.history.len();
        assert!(
            set(
                &mut target,
                pasted[0],
                Some(Binding::Image {
                    column: "photo".into(),
                    fit: Fit::Cover,
                    focus: [0.5; 2]
                })
            )
            .is_err()
        );
        assert!(
            set(
                &mut target,
                pasted[0],
                Some(Binding::Text { column: " ".into() })
            )
            .is_err()
        );
        assert_eq!(target.doc, before);
        assert_eq!(target.history.len(), history);
        target
            .execute(Command::SetLocked {
                id: pasted[0],
                locked: true,
            })
            .unwrap();
        let protected = target.doc.clone();
        assert!(set(&mut target, pasted[0], None).is_err());
        assert_eq!(target.doc, protected);
        target.undo();
        set(&mut target, pasted[0], None).unwrap();
        assert!(target.doc.design.data_bindings.is_empty());
        target.doc.validate().unwrap();
    }
}
