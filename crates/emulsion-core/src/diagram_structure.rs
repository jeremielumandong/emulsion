//! Field-aware UML classes and ER entities, rendered as editable native vectors/text.
use super::*;
const KEY: &str = "emulsion_structure";
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StructuredObject {
    pub title: String,
    pub attributes: Vec<String>,
    #[serde(default)]
    pub methods: Vec<String>,
}
impl StructuredObject {
    pub fn validate(&self) -> Result<(), String> {
        if self.title.trim().is_empty()
            || self.title.len() > 200
            || self.title.contains(['\n', '\r'])
            || self.attributes.len() > 64
            || self.methods.len() > 64
            || self
                .attributes
                .iter()
                .chain(&self.methods)
                .any(|s| s.len() > 256 || s.contains(['\n', '\r']))
            || serde_json::to_string(self)
                .map_err(|e| e.to_string())?
                .len()
                > 4096
        {
            return Err("Use a title and up to 64 single-line fields/methods (4 KiB total)".into());
        }
        Ok(())
    }
    pub fn from_text(text: &str) -> Self {
        let mut sections = text.split("\n\n");
        let first = sections.next().unwrap_or("Object");
        let mut lines = first.lines();
        let title = lines
            .next()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("Object")
            .to_string();
        let rest = lines.map(str::to_owned).collect::<Vec<_>>();
        let attributes = sections
            .next()
            .map(|s| {
                s.lines()
                    .filter(|s| !s.trim().is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or(rest);
        let methods = sections
            .flat_map(str::lines)
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned)
            .collect();
        Self {
            title,
            attributes,
            methods,
        }
    }
    fn text(&self, kind: ShapeKind) -> String {
        let attributes = if self.attributes.is_empty() {
            " ".into()
        } else {
            self.attributes.join("\n")
        };
        if kind == ShapeKind::Class {
            format!(
                "{}\n\n{}\n\n{}",
                self.title,
                attributes,
                if self.methods.is_empty() {
                    " ".into()
                } else {
                    self.methods.join("\n")
                }
            )
        } else {
            format!("{}\n\n{}", self.title, attributes)
        }
    }
}
pub fn get(doc: &Document, id: NodeId) -> Option<StructuredObject> {
    doc.diagram
        .as_ref()?
        .shapes
        .get(&id)?
        .data
        .get(KEY)
        .and_then(|s| serde_json::from_str(s).ok())
}
pub fn set(
    editor: &mut Editor,
    id: NodeId,
    kind: ShapeKind,
    value: StructuredObject,
) -> Result<(), String> {
    if editor.in_transaction() {
        return Err("Finish the current edit first".into());
    }
    if !matches!(kind, ShapeKind::Class | ShapeKind::Entity) {
        return Err("Choose a UML class or ER entity".into());
    }
    value.validate()?;
    if kind == ShapeKind::Entity && !value.methods.is_empty() {
        return Err("ER entities have fields, not methods".into());
    }
    if editor.doc.subtree(id).iter().any(|id| {
        editor.doc.locked_ancestor(*id).is_some() || {
            let l = editor.doc.layer_locks(*id);
            l.position || l.pixels
        }
    }) {
        return Err("Unlock the object before editing fields".into());
    }
    let mut model = editor
        .doc
        .diagram
        .as_deref()
        .cloned()
        .ok_or("Open a diagram first")?;
    let shape = model.shapes.get_mut(&id).ok_or("Select a diagram shape")?;
    shape.kind = kind;
    shape.data.insert(
        KEY.into(),
        serde_json::to_string(&value).map_err(|e| e.to_string())?,
    );
    editor
        .execute(Command::SetDiagram {
            diagram: Some(Arc::new(model)),
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}
pub(super) fn validate(shape: &Shape) -> Result<(), String> {
    if let Some(value) = shape.data.get(KEY) {
        let value: StructuredObject =
            serde_json::from_str(value).map_err(|_| "Invalid structured object data")?;
        value.validate()?;
        if !matches!(shape.kind, ShapeKind::Class | ShapeKind::Entity)
            || (shape.kind == ShapeKind::Entity && !value.methods.is_empty())
        {
            return Err("Structured fields require a class or entity".into());
        }
    }
    Ok(())
}
pub(super) fn synchronize(
    before: &Document,
    doc: &mut Document,
    model: &mut Diagram,
) -> Result<(), String> {
    if !model.shapes.values().any(|s| s.data.contains_key(KEY)) {
        return Ok(());
    }
    let indices = doc
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id, i))
        .collect::<HashMap<_, _>>();
    for (id, shape) in &mut model.shapes {
        let Some(value) = shape.data.get(KEY) else {
            continue;
        };
        let mut value: StructuredObject =
            serde_json::from_str(value).map_err(|_| "Invalid structured object data")?;
        let body = indices[&shape.body];
        let label = indices[&shape.label];
        let (NodeKind::Path { path, style, .. }, NodeKind::Text { spec, .. }) =
            (&doc.nodes[body].kind, &doc.nodes[label].kind)
        else {
            continue;
        };
        let old_shape = before.diagram.as_ref().and_then(|d| d.shapes.get(id));
        let metadata_unchanged = old_shape
            .is_some_and(|old| old.kind == shape.kind && old.data.get(KEY) == shape.data.get(KEY));
        if metadata_unchanged
            && let Some(Node {
                kind: NodeKind::Text { spec: old, .. },
                ..
            }) = before.node(shape.label)
        {
            if old.text != spec.text {
                value = StructuredObject::from_text(&spec.text);
                value.validate()?;
                if shape.kind == ShapeKind::Entity {
                    value.methods.clear();
                }
                shape
                    .data
                    .insert(KEY.into(), serde_json::to_string(&value).unwrap());
            } else if old.size == spec.size
                && old.font == spec.font
                && old.line_height == spec.line_height
                && old.width == spec.width
                && old.letter_spacing == spec.letter_spacing
            {
                // Geometry transforms already move/rotate/scale the separators and
                // glyphs together. Reflow only a content or typography edit.
                continue;
            }
        }
        let matrix = spec.transform();
        let mut local_path = (**path).clone();
        local_path.transform(matrix.inverse());
        let Some((x, y, w, h)) = emulsion_raster::vector_geometry::bounds(&local_path) else {
            continue;
        };
        let mut text = (**spec).clone();
        text.text = value.text(shape.kind);
        text.runs.clear();
        text.paragraphs.clear();
        text.size = text.size.clamp(10., 40.);
        text.line_height = 1.5;
        text.align = Align::Left;
        let origin = matrix.transform_point2(glam::dvec2(x + 12., y + 8.));
        text.x = origin.x as f32;
        text.y = origin.y as f32;
        text.height = None;
        text.width = None;
        text.bold = false;
        text.apply_style(0..value.title.len(), |style| style.bold = true);
        let mut measured = text.clone();
        measured.x = 0.;
        measured.y = 0.;
        measured.rotation = 0.;
        measured.scale_x = 1.;
        measured.scale_y = 1.;
        let natural = crate::text::bounds(&measured);
        let width = w.max(natural.w as f64 + 24.).max(180.);
        let line = f64::from(text.size * text.line_height);
        let height = h.max(text.text.lines().count() as f64 * line + 16.);
        text.width = Some((width - 24.) as f32);
        let mut geometry = ShapeKind::Process.path([x, y, width, height]);
        let mut separators = vec![y + 8. + line * 1.5];
        if shape.kind == ShapeKind::Class {
            separators.push(y + 8. + line * (2.5 + value.attributes.len().max(1) as f64));
        }
        for at in separators {
            geometry.subpaths.push(SubPath {
                anchors: vec![Anchor::corner((x, at)), Anchor::corner((x + width, at))],
                closed: false,
            });
        }
        geometry.transform(matrix);
        let style = *style;
        let (dw, dh) = (doc.width, doc.height);
        doc.nodes[body].kind =
            Node::path(shape.body, "Body", Arc::new(geometry), style, dw, dh).kind;
        doc.nodes[label].kind = Node::text(shape.label, "Fields", text, dw, dh).kind;
    }
    Ok(())
}
