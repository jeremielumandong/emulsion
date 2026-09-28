//! Named, linked appearance styles stored with native design pages.
use crate::{
    Command, Document, Editor, NodeId, design_appearance::Appearance, design_metadata::Design,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_STYLES: usize = 128;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedStyle {
    pub appearance: Appearance,
}

fn name_valid(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= 80 && !name.chars().any(char::is_control)
}

pub fn validate(design: &Design, doc: &Document) -> Result<(), String> {
    if design.saved_styles.len() > MAX_STYLES {
        return Err("A page supports up to 128 saved styles.".into());
    }
    for (name, style) in &design.saved_styles {
        if !name_valid(name) {
            return Err("Style names need 1–80 characters without control characters.".into());
        }
        style.appearance.validate()?;
    }
    if design.style_links.len() > crate::document::MAX_NODES
        || design
            .style_links
            .iter()
            .any(|(id, name)| doc.node(*id).is_none() || !design.saved_styles.contains_key(name))
    {
        return Err("A linked style refers to a missing object or style.".into());
    }
    Ok(())
}

fn imported_name(design: &Design, name: &str, style: &SavedStyle) -> String {
    if design
        .saved_styles
        .get(name)
        .is_none_or(|existing| existing == style)
    {
        return name.to_string();
    }
    let stem: String = name.chars().take(64).collect();
    for index in 2.. {
        let candidate = format!("{stem} ({index})");
        if design
            .saved_styles
            .get(&candidate)
            .is_none_or(|existing| existing == style)
        {
            return candidate;
        }
    }
    unreachable!()
}

/// Clipboard imports preserve both libraries when names collide. The incoming
/// node IDs must already have been remapped to the receiving document.
pub fn merge_into(target: &mut Design, additions: &Design) {
    let names: BTreeMap<_, _> = additions
        .saved_styles
        .iter()
        .map(|(name, style)| {
            let imported = imported_name(target, name, style);
            target.saved_styles.insert(imported.clone(), style.clone());
            (name.clone(), imported)
        })
        .collect();
    for (id, name) in &additions.style_links {
        if let Some(imported) = names.get(name) {
            target.style_links.insert(*id, imported.clone());
        }
    }
}

fn editable(doc: &Document, id: NodeId) -> Result<(), String> {
    let node = doc.node(id).ok_or("Select an existing object.")?;
    let locks = doc.layer_locks(id);
    if doc.locked_ancestor(id).is_some() || locks.pixels || locks.position || locks.transparency {
        return Err("Unlock all linked objects before applying or updating their style.".into());
    }
    if matches!(node.kind, crate::NodeKind::Adjust(_)) {
        return Err("Adjustment layers cannot use saved appearance styles.".into());
    }
    Ok(())
}

fn commit(
    editor: &mut Editor,
    label: &str,
    mut commands: Vec<Command>,
    design: Design,
) -> Result<(), String> {
    if editor.in_transaction() {
        return Err("Finish the current edit before changing saved styles.".into());
    }
    commands.push(Command::SetDesign {
        design: Box::new(design),
    });
    let mut trial = editor.doc.clone();
    for command in &commands {
        command
            .apply(&mut trial)
            .map_err(|error| error.to_string())?;
    }
    editor.begin(label);
    for command in commands {
        if let Err(error) = editor.execute(command) {
            editor.cancel();
            return Err(error.to_string());
        }
    }
    editor.end();
    Ok(())
}

pub fn create(editor: &mut Editor, source: NodeId, name: &str) -> Result<(), String> {
    let name = name.trim();
    editable(&editor.doc, source)?;
    if !name_valid(name) {
        return Err("Choose a style name of 1–80 characters.".into());
    }
    if editor.doc.design.saved_styles.contains_key(name) {
        return Err(
            "That style name already exists. Choose another name or update the existing style."
                .into(),
        );
    }
    let style = SavedStyle {
        appearance: Appearance::capture(editor.doc.node(source).unwrap()),
    };
    let mut design = editor.doc.design.clone();
    design.saved_styles.insert(name.into(), style);
    design.style_links.insert(source, name.into());
    commit(editor, "Save reusable style", Vec::new(), design)
}

/// Imports from another page if necessary and links every target as one edit.
pub fn apply(
    editor: &mut Editor,
    targets: &[NodeId],
    name: &str,
    style: &SavedStyle,
) -> Result<String, String> {
    apply_portable(editor,targets,name,style,&BTreeMap::new())
}
/// Apply a cross-page style together with its portable font resources.
pub fn apply_portable(editor:&mut Editor,targets:&[NodeId],name:&str,style:&SavedStyle,fonts:&BTreeMap<String,crate::design_fonts::EmbeddedFont>)->Result<String,String>{
    if targets.is_empty() {
        return Err("Select objects to apply a style.".into());
    }
    let mut design = editor.doc.design.clone();
    for alias in style.appearance.font_families() {if let Some(font)=fonts.get(&alias){design.fonts.insert(alias,font.clone());}}
    let name = imported_name(&design, name, style);
    design.saved_styles.insert(name.clone(), style.clone());
    let mut commands = Vec::new();
    for id in targets.iter().copied().collect::<BTreeSet<_>>() {
        editable(&editor.doc, id)?;
        commands.extend(style.appearance.commands(editor.doc.node(id).unwrap()));
        design.style_links.insert(id, name.clone());
    }
    commit(editor, "Apply reusable style", commands, design)?;
    Ok(name)
}

/// Explicit update publishes the selected appearance to every linked consumer
/// on this page. Ordinary edits remain local until this command is used.
pub fn update(editor: &mut Editor, name: &str, source: NodeId) -> Result<(), String> {
    editable(&editor.doc, source)?;
    if !editor.doc.design.saved_styles.contains_key(name) {
        return Err("This saved style no longer exists.".into());
    }
    let style = SavedStyle {
        appearance: Appearance::capture(editor.doc.node(source).unwrap()),
    };
    let mut design = editor.doc.design.clone();
    let mut commands = Vec::new();
    let mut ids: BTreeSet<_> = design
        .style_links
        .iter()
        .filter_map(|(id, linked)| (linked == name).then_some(*id))
        .collect();
    ids.insert(source);
    for id in ids {
        editable(&editor.doc, id)?;
        // Publishing samples the source's first text style. Do not flatten its
        // own rich-text runs merely because it also becomes a linked consumer.
        if id != source {
            commands.extend(style.appearance.commands(editor.doc.node(id).unwrap()));
        }
        design.style_links.insert(id, name.into());
    }
    design.saved_styles.insert(name.into(), style);
    commit(editor, "Update reusable style", commands, design)
}

pub fn reset(editor: &mut Editor, targets: &[NodeId]) -> Result<(), String> {
    let design = editor.doc.design.clone();
    let mut commands = Vec::new();
    for id in targets.iter().copied().collect::<BTreeSet<_>>() {
        let Some(name) = design.style_links.get(&id) else {
            continue;
        };
        editable(&editor.doc, id)?;
        commands.extend(
            design.saved_styles[name]
                .appearance
                .commands(editor.doc.node(id).unwrap()),
        );
    }
    if commands.is_empty() {
        return Err("Select an object linked to a saved style.".into());
    }
    commit(editor, "Reset linked style", commands, design)
}

pub fn detach(editor: &mut Editor, targets: &[NodeId]) -> Result<(), String> {
    let mut design = editor.doc.design.clone();
    let mut changed = false;
    for id in targets {
        changed |= design.style_links.remove(id).is_some();
    }
    if !changed {
        return Err("Select an object linked to a saved style.".into());
    }
    commit(editor, "Detach linked style", Vec::new(), design)
}

pub fn rename(editor: &mut Editor, old: &str, new: &str) -> Result<(), String> {
    let new = new.trim();
    if old == new {
        return Ok(());
    }
    if !name_valid(new) {
        return Err("Choose a style name of 1–80 characters.".into());
    }
    let mut design = editor.doc.design.clone();
    if design.saved_styles.contains_key(new) {
        return Err("That style name already exists.".into());
    }
    let style = design
        .saved_styles
        .remove(old)
        .ok_or("This saved style no longer exists.")?;
    design.saved_styles.insert(new.into(), style);
    for link in design.style_links.values_mut() {
        if link == old {
            *link = new.into();
        }
    }
    commit(editor, "Rename reusable style", Vec::new(), design)
}

pub fn remove(editor: &mut Editor, name: &str) -> Result<(), String> {
    let mut design = editor.doc.design.clone();
    if design.saved_styles.remove(name).is_none() {
        return Err("This saved style no longer exists.".into());
    }
    design.style_links.retain(|_, link| link != name);
    commit(editor, "Remove reusable style", Vec::new(), design)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Node, NodeKind, command::Slot, text::TextSpec};

    fn text(editor: &mut Editor, value: &str, x: f32, size: f32) -> NodeId {
        editor
            .execute(Command::AddNode {
                node: Box::new(Node::text(
                    0,
                    value,
                    TextSpec {
                        text: value.into(),
                        x,
                        y: 50.,
                        size,
                        width: Some(200.),
                        ..Default::default()
                    },
                    800,
                    600,
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap()
    }

    #[test]
    fn linked_styles_propagate_without_replacing_content_and_undo_atomically() {
        let mut editor = Editor::new(Document::new(800, 600), None);
        let source = text(&mut editor, "Heading", 10., 52.);
        let target = text(&mut editor, "Different words", 300., 24.);
        create(&mut editor, source, "Headings").unwrap();
        let saved = editor.doc.design.saved_styles["Headings"].clone();
        let before = editor.doc.clone();
        apply(&mut editor, &[target], "Headings", &saved).unwrap();
        let NodeKind::Text { spec, .. } = &editor.doc.node(target).unwrap().kind else {
            panic!()
        };
        assert_eq!(
            (spec.text.as_str(), spec.x, spec.size),
            ("Different words", 300., 52.)
        );
        editor.undo();
        assert_eq!(editor.doc, before);
        editor.redo();
        editor
            .execute(Command::SetOpacity {
                id: source,
                opacity: 0.4,
            })
            .unwrap();
        let before_update = editor.doc.clone();
        update(&mut editor, "Headings", source).unwrap();
        assert_eq!(editor.doc.node(target).unwrap().opacity, 0.4);
        editor.undo();
        assert_eq!(editor.doc, before_update);
        editor.redo();
        editor
            .execute(Command::SetOpacity {
                id: target,
                opacity: 0.9,
            })
            .unwrap();
        reset(&mut editor, &[target]).unwrap();
        assert_eq!(editor.doc.node(target).unwrap().opacity, 0.4);
        detach(&mut editor, &[target]).unwrap();
        assert!(!editor.doc.design.style_links.contains_key(&target));
        assert_eq!(editor.doc.node(target).unwrap().opacity, 0.4);
        rename(&mut editor, "Headings", "Titles").unwrap();
        assert_eq!(editor.doc.design.style_links[&source], "Titles");
        let nodes = editor.doc.nodes.clone();
        remove(&mut editor, "Titles").unwrap();
        assert_eq!(editor.doc.nodes, nodes);
        assert!(editor.doc.design.style_links.is_empty());
        editor.undo();
        assert!(editor.doc.design.saved_styles.contains_key("Titles"));
    }

    #[test]
    fn locked_consumers_prevent_partial_update_and_name_collisions_preserve_both_styles() {
        let mut editor = Editor::new(Document::new(800, 600), None);
        let source = text(&mut editor, "Source", 10., 48.);
        let target = text(&mut editor, "Target", 300., 24.);
        create(&mut editor, source, "Shared").unwrap();
        let saved = editor.doc.design.saved_styles["Shared"].clone();
        apply(&mut editor, &[target], "Shared", &saved).unwrap();
        editor
            .execute(Command::SetLocked {
                id: target,
                locked: true,
            })
            .unwrap();
        editor
            .execute(Command::SetOpacity {
                id: source,
                opacity: 0.5,
            })
            .unwrap();
        let locked = editor.doc.clone();
        assert!(update(&mut editor, "Shared", source).is_err());
        assert_eq!(editor.doc, locked);

        let fragment = crate::fragment::Fragment::capture(&editor.doc, &[source]).unwrap();
        let mut other = Editor::new(Document::new(800, 600), None);
        let other_source = text(&mut other, "Other", 20., 20.);
        create(&mut other, other_source, "Shared").unwrap();
        let original = other.doc.design.saved_styles["Shared"].clone();
        let pasted = fragment.paste(&mut other, Slot::TOP, (0., 0.)).unwrap()[0];
        assert_eq!(other.doc.design.saved_styles["Shared"], original);
        assert_eq!(other.doc.design.style_links[&pasted], "Shared (2)");
        assert_eq!(other.doc.design.saved_styles["Shared (2)"], saved);
        let duplicate = other
            .execute(Command::DuplicateNode { id: pasted })
            .unwrap()
            .unwrap();
        assert_eq!(other.doc.design.style_links[&duplicate], "Shared (2)");
        other.execute(Command::RemoveNode { id: pasted }).unwrap();
        assert!(!other.doc.design.style_links.contains_key(&pasted));
        assert!(other.doc.design.saved_styles.contains_key("Shared (2)"));
        other.doc.validate().unwrap();
    }

    #[test]
    fn style_data_roundtrips_and_rejects_invalid_names_and_values() {
        let mut editor = Editor::new(Document::new(800, 600), None);
        let source = text(&mut editor, "Source", 10., 48.);
        for name in ["", "\n", &"x".repeat(81)] {
            assert!(create(&mut editor, source, name).is_err());
        }
        create(&mut editor, source, "Heading").unwrap();
        let bytes = serde_json::to_vec(&editor.doc.design).unwrap();
        let restored: Design = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored, editor.doc.design);
        let mut json = serde_json::to_value(&restored).unwrap();
        json["saved_styles"]["Heading"]["appearance"]["opacity"] = 5.into();
        let invalid: Design = serde_json::from_value(json).unwrap();
        assert!(invalid.validate(&editor.doc).is_err());
    }

    #[test]
    fn publishing_mixed_typography_preserves_the_source_runs() {
        let mut editor = Editor::new(Document::new(800, 600), None);
        let source = text(&mut editor, "Bold and italic", 10., 48.);
        let target = text(&mut editor, "Other words", 300., 24.);
        create(&mut editor, source, "Mixed heading").unwrap();
        let style = editor.doc.design.saved_styles["Mixed heading"].clone();
        apply(&mut editor, &[target], "Mixed heading", &style).unwrap();
        let NodeKind::Text { spec, .. } = &editor.doc.node(source).unwrap().kind else {
            panic!()
        };
        let mut changed = (**spec).clone();
        changed.apply_style(0..4, |style| style.bold = true);
        changed.apply_style(9..15, |style| style.italic = true);
        editor
            .execute(Command::SetText {
                id: source,
                spec: Box::new(changed),
            })
            .unwrap();
        let before = editor.doc.node(source).unwrap().clone();
        update(&mut editor, "Mixed heading", source).unwrap();
        assert_eq!(editor.doc.node(source).unwrap(), &before);
        let NodeKind::Text { spec, .. } = &editor.doc.node(target).unwrap().kind else {
            panic!()
        };
        assert!(spec.style_at(0).bold);
        assert_eq!(spec.text, "Other words");
    }
}
