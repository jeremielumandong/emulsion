//! Typed local design variables. Bindings materialize native editable properties.
use crate::{Document, Editor, NodeId, NodeKind, design_metadata::Design};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Value {
    Color([u8; 4]),
    Number(f64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Property {
    Fill,
    Stroke,
    TextColor,
    Opacity,
    FontSize,
    StrokeWidth,
    FrameGap,
    FramePadding,
}
impl Property {
    pub const ALL: [Self; 8] = [
        Self::Fill,
        Self::Stroke,
        Self::TextColor,
        Self::Opacity,
        Self::FontSize,
        Self::StrokeWidth,
        Self::FrameGap,
        Self::FramePadding,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Fill => "Fill color",
            Self::Stroke => "Stroke color",
            Self::TextColor => "Text color",
            Self::Opacity => "Opacity (0–1)",
            Self::FontSize => "Font size",
            Self::StrokeWidth => "Stroke width",
            Self::FrameGap => "Frame gap",
            Self::FramePadding => "Frame padding",
        }
    }
    pub fn is_color(self) -> bool {
        matches!(self, Self::Fill | Self::Stroke | Self::TextColor)
    }
    pub fn supports(self, doc: &Document, id: NodeId) -> bool {
        let Some(node) = doc.node(id) else {
            return false;
        };
        match self {
            Self::Fill => matches!(node.kind, NodeKind::Path { .. } | NodeKind::Fill { .. }),
            Self::Stroke | Self::StrokeWidth => matches!(node.kind, NodeKind::Path { .. }),
            Self::TextColor | Self::FontSize => matches!(node.kind, NodeKind::Text { .. }),
            Self::Opacity => true,
            Self::FrameGap | Self::FramePadding => doc.design.frames.contains_key(&id),
        }
    }
    fn check(self, value: &Value) -> Result<(), String> {
        match value {
            Value::Color(_) if self.is_color() => Ok(()),
            Value::Number(n) if !self.is_color() && n.is_finite() => {
                let (min, max) = match self {
                    Self::Opacity => (0., 1.),
                    Self::FontSize => (1., 4000.),
                    _ => (0., 10000.),
                };
                if (min..=max).contains(n) {
                    Ok(())
                } else {
                    Err(format!(
                        "{} needs a value from {min} to {max}.",
                        self.label()
                    ))
                }
            }
            _ => Err(format!(
                "{} needs a {} variable.",
                self.label(),
                if self.is_color() { "color" } else { "number" }
            )),
        }
    }
}

pub fn validate(design: &Design, doc: &Document) -> Result<(), String> {
    if design.variables.len() > 256 || design.variable_bindings.len() > crate::document::MAX_NODES {
        return Err("Use at most 256 design variables per page.".into());
    }
    for (name, value) in &design.variables {
        validate_name(name)?;
        if matches!(value, Value::Number(n) if !n.is_finite() || n.abs() > 1e9) {
            return Err("Number variables must be finite and within ±1 billion.".into());
        }
    }
    for (id, bindings) in &design.variable_bindings {
        for (property, name) in bindings {
            let supported = match property {
                Property::FrameGap | Property::FramePadding => design.frames.contains_key(id),
                _ => property.supports(doc, *id),
            };
            if !supported {
                return Err(format!(
                    "Object {id} does not support {}. Unlink that property first.",
                    property.label()
                ));
            }
            property.check(
                design
                    .variables
                    .get(name)
                    .ok_or("A binding refers to a missing variable.")?,
            )?;
        }
    }
    Ok(())
}
fn validate_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty()
        || name != name.trim()
        || name.chars().count() > 80
        || name.chars().any(char::is_control)
    {
        Err("Variable names need 1–80 characters without leading/trailing spaces or control characters.".into())
    } else {
        Ok(())
    }
}

/// Run before native dependent layout updates. Lock checks include consumers.
pub(crate) fn synchronize(before: &Document, doc: &mut Document) -> Result<(), String> {
    validate(&doc.design, doc)?;
    let bindings = doc.design.variable_bindings.clone();
    for (id, properties) in bindings {
        let old = doc.node(id).cloned().ok_or("Missing variable consumer.")?;
        let frame_before = doc.design.frames.get(&id).cloned();
        for (property, name) in properties {
            let value = doc.design.variables[&name].clone();
            match (property, value) {
                (Property::FrameGap, Value::Number(n)) => {
                    doc.design.frames.get_mut(&id).unwrap().gap = n
                }
                (Property::FramePadding, Value::Number(n)) => {
                    doc.design.frames.get_mut(&id).unwrap().padding = [n; 4]
                }
                (property, value) => {
                    let node = doc.node_mut(id).unwrap();
                    match (property, value, &mut node.kind) {
                        (Property::Opacity, Value::Number(n), _) => node.opacity = n as f32,
                        (Property::Fill, Value::Color(color), NodeKind::Fill { rgba }) => {
                            *rgba = color
                        }
                        (Property::Fill, Value::Color(color), NodeKind::Path { style, .. }) => {
                            style.fill = Some(color);
                        }
                        (Property::Stroke, Value::Color(color), NodeKind::Path { style, .. }) => {
                            style.stroke = Some(color);
                        }
                        (Property::StrokeWidth, Value::Number(n), NodeKind::Path { style, .. }) => {
                            style.width = n as f32;
                        }
                        (Property::TextColor, Value::Color(color), NodeKind::Text { spec, .. }) => {
                            if spec.color == color && spec.runs.iter().all(|r| r.style.color == color) { continue; }
                            let spec = Arc::make_mut(spec);
                            spec.color = color;
                            for run in &mut spec.runs {
                                run.style.color = color;
                            }
                        }
                        (Property::FontSize, Value::Number(n), NodeKind::Text { spec, .. }) => {
                            if spec.size == n as f32 && spec.runs.iter().all(|r| r.style.size == n as f32) { continue; }
                            let spec = Arc::make_mut(spec);
                            spec.size = n as f32;
                            for run in &mut spec.runs {
                                run.style.size = n as f32;
                            }
                        }
                        _ => {
                            return Err(
                                "Variable property and object type are incompatible.".into()
                            );
                        }
                    }
                }
            }
        }
        if doc.node(id) != Some(&old) {
            let (w, h) = (doc.width, doc.height);
            match &mut doc.node_mut(id).unwrap().kind {
                NodeKind::Text { spec, cache } => {
                    *cache = crate::vector_cache::VectorRaster::text(spec.clone(), w, h)
                }
                NodeKind::Path { path, style, cache } => {
                    *cache = crate::vector_cache::VectorRaster::path(path.clone(), *style, w, h)
                }
                _ => {}
            }
        }
        if (doc.node(id) != Some(&old) || doc.design.frames.get(&id) != frame_before.as_ref())
            && before.node(id).is_some()
        {
            let locks = before.layer_locks(id);
            if before.locked_ancestor(id).is_some()
                || locks.pixels
                || locks.position
                || locks.transparency
            {
                return Err("Unlock every affected variable consumer before updating it.".into());
            }
        }
    }
    Ok(())
}

fn edit(
    editor: &mut Editor,
    label: &str,
    change: impl FnOnce(&mut Design) -> Result<(), String>,
) -> Result<(), String> {
    let mut doc = editor.doc.clone();
    change(&mut doc.design)?;
    validate(&doc.design, &doc)?;
    editor.commit_design_document(doc, label)
}
pub fn set(editor: &mut Editor, name: &str, value: Value) -> Result<(), String> {
    validate_name(name)?;
    edit(editor, "Set design variable", |d| {
        d.variables.insert(name.into(), value);
        Ok(())
    })
}
pub fn put(editor: &mut Editor, old: Option<&str>, name: &str, value: Value) -> Result<(), String> {
    validate_name(name)?;
    edit(editor, "Edit design variable", |d| {
        if let Some(old) = old {
            if !d.variables.contains_key(old) {
                return Err("Variable no longer exists.".into());
            }
            if old != name {
                if d.variables.contains_key(name) {
                    return Err("That variable name already exists.".into());
                }
                d.variables.remove(old);
                for bindings in d.variable_bindings.values_mut() {
                    for v in bindings.values_mut() {
                        if v == old {
                            *v = name.into();
                        }
                    }
                }
            }
        } else if d.variables.contains_key(name) {
            return Err("That variable name already exists.".into());
        }
        d.variables.insert(name.into(), value);
        Ok(())
    })
}
pub fn rename(editor: &mut Editor, name: &str, to: &str) -> Result<(), String> {
    validate_name(to)?;
    if name == to {
        return Ok(());
    }
    edit(editor, "Rename design variable", |d| {
        if d.variables.contains_key(to) {
            return Err("That variable name already exists.".into());
        }
        let value = d
            .variables
            .remove(name)
            .ok_or("Variable no longer exists.")?;
        d.variables.insert(to.into(), value);
        for bindings in d.variable_bindings.values_mut() {
            for value in bindings.values_mut() {
                if value == name {
                    *value = to.into();
                }
            }
        }
        Ok(())
    })
}
pub fn remove(editor: &mut Editor, name: &str) -> Result<(), String> {
    edit(editor, "Remove design variable", |d| {
        d.variables
            .remove(name)
            .ok_or("Variable no longer exists.")?;
        for bindings in d.variable_bindings.values_mut() {
            bindings.retain(|_, value| value != name);
        }
        d.variable_bindings.retain(|_, b| !b.is_empty());
        Ok(())
    })
}
pub fn bind(
    editor: &mut Editor,
    ids: &[NodeId],
    property: Property,
    name: Option<&str>,
) -> Result<(), String> {
    if ids.is_empty()
        || ids.iter().copied().collect::<std::collections::HashSet<_>>().len() != ids.len()
        || ids.iter().any(|id| editor.doc.node(*id).is_none()) {
        return Err("Select existing objects to bind or unlink.".into());
    }
    for id in ids {
        let locks = editor.doc.layer_locks(*id);
        if editor.doc.locked_ancestor(*id).is_some()
            || locks.pixels
            || locks.position
            || locks.transparency
        {
            return Err("Unlock the selected objects first.".into());
        }
    }
    edit(
        editor,
        if name.is_some() {
            "Bind design variable"
        } else {
            "Unlink design variable"
        },
        |d| {
            for id in ids {
                if let Some(name) = name {
                    d.variable_bindings
                        .entry(*id)
                        .or_default()
                        .insert(property, name.into());
                } else if let Some(bindings) = d.variable_bindings.get_mut(id) {
                    bindings.remove(&property);
                }
            }
            d.variable_bindings.retain(|_, b| !b.is_empty());
            Ok(())
        },
    )
}

/// Resolve collisions before remapping incoming bindings, keeping both appearances.
pub fn merge_into(target: &mut Design, source: &Design) {
    let mut names = BTreeMap::new();
    for (name, value) in &source.variables {
        let stem: String = name.chars().take(64).collect();
        let mut candidate = name.clone();
        let mut index = 2;
        while target.variables.get(&candidate).is_some_and(|v| v != value) {
            candidate = format!("{stem} ({index})");
            index += 1;
        }
        target.variables.insert(candidate.clone(), value.clone());
        names.insert(name, candidate);
    }
    for (id, bindings) in &source.variable_bindings {
        target.variable_bindings.insert(
            *id,
            bindings
                .iter()
                .filter_map(|(p, name)| names.get(name).map(|v| (*p, v.clone())))
                .collect(),
        );
    }
}

#[cfg(test)]
#[path = "design_variable_tests.rs"]
mod tests;
