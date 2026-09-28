//! Diagram object appearance and annotations shared by UI and automation.
use super::*;
use crate::design_appearance::Appearance;

fn parts(doc: &Document, id: NodeId) -> Result<(NodeId, NodeId), String> {
    let model = doc.diagram.as_ref().ok_or("Document has no diagram")?;
    model
        .shapes
        .get(&id)
        .map(|s| (s.body, s.label))
        .or_else(|| model.edges.get(&id).map(|e| (e.path, e.label)))
        .ok_or_else(|| "Select a diagram shape or connector".into())
}
fn editable(doc: &Document, id: NodeId) -> Result<(), String> {
    if doc.node(id).is_none() {
        return Err("Object is missing".into());
    }
    if doc
        .subtree(id)
        .iter()
        .any(|id| doc.locked_ancestor(*id).is_some())
    {
        return Err("Unlock the object before editing its details or appearance".into());
    }
    Ok(())
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ObjectStyle {
    body: Appearance,
    label: Appearance,
}
impl ObjectStyle {
    pub fn validate(&self) -> Result<(), String> {
        self.body.validate()?;
        self.label.validate()
    }
    pub fn capture(doc: &Document, id: NodeId) -> Result<Self, String> {
        let (body, label) = parts(doc, id)?;
        Ok(Self {
            body: Appearance::capture(doc.node(body).ok_or("Body is missing")?),
            label: Appearance::capture(doc.node(label).ok_or("Label is missing")?),
        })
    }
    pub fn commands(&self, doc: &Document, roots: &[NodeId]) -> Result<Vec<Command>, String> {
        let mut ids = HashSet::new();
        for id in roots {
            editable(doc, *id)?;
            ids.extend(doc.subtree(*id));
        }
        let mut commands = Vec::new();
        for node in &doc.nodes {
            if ids.contains(&node.id)
                && let Ok((body, label)) = parts(doc, node.id)
            {
                let mut body_commands =
                    self.body.commands(doc.node(body).ok_or("Body is missing")?);
                if doc.diagram.as_ref().unwrap().edges.contains_key(&node.id) {
                    for command in &mut body_commands {
                        if let Command::SetPath { style, .. } = command {
                            style.fill = None;
                        }
                    }
                }
                commands.extend(body_commands);
                commands.extend(
                    self.label
                        .commands(doc.node(label).ok_or("Label is missing")?),
                );
            }
        }
        if commands.is_empty() {
            return Err("Select diagram objects to style".into());
        }
        Ok(commands)
    }
}
/// Empty strings clear a field; omitted fields retain their values.
pub fn object_details_commands(
    doc: &Document,
    id: NodeId,
    fields: &BTreeMap<String, String>,
) -> Result<Vec<Command>, String> {
    editable(doc, id)?;
    let mut model = (**doc.diagram.as_ref().ok_or("Document has no diagram")?).clone();
    let shape = model
        .shapes
        .get_mut(&id)
        .ok_or("Details belong to a diagram shape")?;
    let mut design = doc.design.clone();
    for (key, value) in fields {
        if !["note", "alt_text", "link"].contains(&key.as_str()) || value.len() > 4096 {
            return Err("Use note, alt_text or link with at most 4,096 bytes".into());
        }
        let data_key = if key == "link" { "drawio_link" } else { key };
        if key == "link" {
            if !value.is_empty() && !crate::design_interactions::valid_url(value) {
                return Err("Enter an HTTP or HTTPS link".into());
            }
            let actions = design.interactions.entry(id).or_default();
            actions.retain(|a| !matches!(a, crate::design_interactions::Action::Url { .. }));
            if !value.is_empty() {
                actions.push(crate::design_interactions::Action::Url { url: value.clone() });
            }
            if actions.is_empty() {
                design.interactions.remove(&id);
            }
        }
        if value.is_empty() {
            shape.data.remove(data_key);
        } else {
            shape.data.insert(data_key.into(), value.clone());
        }
    }
    let mut commands = vec![Command::SetDiagram {
        diagram: Some(Arc::new(model)),
    }];
    if fields.contains_key("link") {
        commands.push(Command::SetDesign {
            design: Box::new(design),
        });
    }
    Ok(commands)
}

/// Style the semantic connector path, preserving geometry and bindings.
pub fn connector_style_command(
    doc: &Document,
    id: NodeId,
    width: Option<f32>,
    dash: Option<&[f32]>,
    color: Option<[u8; 4]>,
) -> Result<Command, String> {
    editable(doc, id)?;
    let edge = doc
        .diagram
        .as_ref()
        .and_then(|d| d.edges.get(&id))
        .ok_or("Select a connector")?;
    let NodeKind::Path { path, style, .. } =
        &doc.node(edge.path).ok_or("Missing connector path")?.kind
    else {
        return Err("Missing connector path".into());
    };
    let mut style = *style;
    if let Some(width) = width {
        if !width.is_finite() || !(0.25..=100.).contains(&width) {
            return Err("Line width must be 0.25–100 px".into());
        }
        style.width = width;
    }
    if let Some(dash) = dash {
        if dash.len() > 6
            || dash.iter().any(|n| !n.is_finite() || *n < 0. || *n > 1000.)
            || (!dash.is_empty() && !dash.iter().any(|n| *n > 0.))
        {
            return Err("Invalid line dash pattern".into());
        }
        style.dash = [0.; 6];
        style.dash[..dash.len()].copy_from_slice(dash);
        style.dash_count = dash.len() as u8;
        style.dash_offset = 0.;
    }
    if let Some(color) = color {
        style.stroke = Some(color);
        style.stroke_paint = emulsion_raster::vector::PathPaint::Solid;
    }
    Ok(Command::SetPath {
        id: edge.path,
        path: path.clone(),
        style,
    })
}
