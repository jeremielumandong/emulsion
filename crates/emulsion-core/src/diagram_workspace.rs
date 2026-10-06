//! Persistent diagram defaults, local review threads and portable view references.
use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Extend the editing surface beyond the finite print/export page.
    pub infinite_canvas: bool,
    pub shape_style: Option<ObjectStyle>,
    pub connector_style: Option<ObjectStyle>,
    pub thumbnail: Vec<NodeId>,
    pub threads: BTreeMap<u64, Thread>,
}

pub fn infinite_canvas(doc: &Document) -> bool {
    doc.diagram
        .as_ref()
        .is_some_and(|m| m.settings.infinite_canvas)
}

pub fn set_infinite_canvas(editor: &mut Editor, enabled: bool) -> Result<(), String> {
    if editor.in_transaction() {
        return Err("Finish the current edit first".into());
    }
    let mut model = editor.doc.diagram.as_deref().cloned().unwrap_or_default();
    model.settings.infinite_canvas = enabled;
    editor
        .execute(Command::SetDiagram {
            diagram: Some(Arc::new(model)),
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Visible artwork, excluding page-wide background fills.
pub fn content_bounds(
    doc: &Document,
) -> Result<Option<emulsion_raster::IRect>, crate::GeometryError> {
    let mut bounds: Option<emulsion_raster::IRect> = None;
    for id in doc.children(None) {
        let node = doc.node(id).expect("document child");
        if !node.visible || matches!(node.kind, NodeKind::Fill { .. }) {
            continue;
        }
        if let Some(next) = crate::geometry::node_bounds(doc, id)? {
            bounds = Some(bounds.map_or(next, |old| old.union(&next)));
        }
    }
    Ok(bounds)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Thread {
    pub object: NodeId,
    pub resolved: bool,
    pub messages: Vec<Message>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub author: String,
    pub text: String,
}
impl Settings {
    pub fn validate(&self, model: &Diagram, doc: &Document) -> Result<(), String> {
        for style in [&self.shape_style, &self.connector_style]
            .into_iter()
            .flatten()
        {
            style.validate()?;
        }
        if self.thumbnail.len() > 1000 || self.thumbnail.iter().any(|id| doc.node(*id).is_none()) {
            return Err("Invalid thumbnail selection".into());
        }
        if self.threads.len() > 512 {
            return Err("A page supports at most 512 comment threads".into());
        }
        for (id, t) in &self.threads {
            if *id == 0
                || !model.contains_endpoint(t.object)
                || t.messages.is_empty()
                || t.messages.len() > 100
            {
                return Err("Invalid comment thread".into());
            }
            for m in &t.messages {
                validate_message(&m.author, &m.text)?;
            }
        }
        Ok(())
    }
    pub(super) fn retain(&mut self, ids: &HashSet<NodeId>) {
        self.thumbnail.retain(|id| ids.contains(id));
        self.threads.retain(|_, t| ids.contains(&t.object));
    }
    pub(super) fn remap(&self, map: &HashMap<NodeId, NodeId>) -> Self {
        let mut out = self.clone();
        out.thumbnail = self
            .thumbnail
            .iter()
            .filter_map(|id| map.get(id).copied())
            .collect();
        out.threads.retain(|_, t| {
            if let Some(id) = map.get(&t.object) {
                t.object = *id;
                true
            } else {
                false
            }
        });
        out
    }
    pub(super) fn fragment(&self, ids: &HashSet<NodeId>) -> Self {
        Self {
            threads: self
                .threads
                .iter()
                .filter(|(_, t)| ids.contains(&t.object))
                .map(|(id, t)| (*id, t.clone()))
                .collect(),
            ..Default::default()
        }
    }
    pub fn append_threads(&mut self, other: Self) -> Result<(), String> {
        if self.threads.len() + other.threads.len() > 512 {
            return Err("Pasting these objects would exceed 512 comment threads".into());
        }
        for (_, thread) in other.threads {
            let id = self
                .threads
                .keys()
                .next_back()
                .copied()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or("Comment ID limit reached")?;
            self.threads.insert(id, thread);
        }
        Ok(())
    }
}
fn validate_message(author: &str, text: &str) -> Result<(), String> {
    if author.trim().is_empty()
        || author.len() > 120
        || text.trim().is_empty()
        || text.len() > 4096
        || author.chars().any(char::is_control)
    {
        Err("Comments need an author (1–120 bytes) and message (1–4096 bytes)".into())
    } else {
        Ok(())
    }
}
fn update(
    editor: &mut Editor,
    change: impl FnOnce(&mut Diagram) -> Result<(), String>,
) -> Result<(), String> {
    if editor.in_transaction() {
        return Err("Finish the current edit first".into());
    }
    let mut model = editor
        .doc
        .diagram
        .as_deref()
        .cloned()
        .ok_or("Open a diagram first")?;
    change(&mut model)?;
    editor
        .execute(Command::SetDiagram {
            diagram: Some(Arc::new(model)),
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}
pub fn set_default_style(
    editor: &mut Editor,
    object: Option<NodeId>,
    connector: bool,
) -> Result<(), String> {
    let style = object
        .map(|id| ObjectStyle::capture(&editor.doc, id))
        .transpose()?;
    update(editor, move |m| {
        if connector {
            m.settings.connector_style = style
        } else {
            m.settings.shape_style = style
        }
        Ok(())
    })
}
pub(super) fn apply_default(
    editor: &mut Editor,
    id: NodeId,
    connector: bool,
) -> Result<(), String> {
    let settings = &editor
        .doc
        .diagram
        .as_ref()
        .ok_or("Missing diagram")?
        .settings;
    let style = if connector {
        &settings.connector_style
    } else {
        &settings.shape_style
    };
    if let Some(style) = style {
        for command in style.commands(&editor.doc, &[id])? {
            editor.execute(command).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
pub fn set_thumbnail(editor: &mut Editor, ids: Vec<NodeId>) -> Result<(), String> {
    if ids.len() > 1000 || ids.iter().any(|id| editor.doc.node(*id).is_none()) {
        return Err("Select up to 1,000 existing objects for the thumbnail".into());
    }
    update(editor, move |m| {
        m.settings.thumbnail = ids;
        Ok(())
    })
}
pub fn thumbnail_bounds(doc: &Document) -> Result<Option<Bounds>, crate::GeometryError> {
    let Some(diagram) = &doc.diagram else {
        return Ok(None);
    };
    let mut bounds: Option<emulsion_raster::IRect> = None;
    for id in &diagram.settings.thumbnail {
        if let Some(next) = crate::geometry::node_bounds(doc, *id)? {
            bounds = Some(bounds.map_or(next, |old| old.union(&next)));
        }
    }
    Ok(bounds.map(|b| {
        [
            b.x as f64 - 12.,
            b.y as f64 - 12.,
            b.w as f64 + 24.,
            b.h as f64 + 24.,
        ]
    }))
}
pub fn thumbnail_document(doc: &Document) -> Result<Option<Document>, crate::CommandError> {
    let Some([x, y, w, h]) = thumbnail_bounds(doc)? else {
        return Ok(None);
    };
    let mut preview = doc.clone();
    Arc::make_mut(preview.diagram.as_mut().expect("thumbnail diagram"))
        .settings
        .thumbnail
        .clear();
    // This is a read-only preview; document locks must not block cropping it.
    for node in &mut preview.nodes {
        node.locked = false;
        node.locks = Default::default();
    }
    Command::Crop {
        rect: emulsion_raster::IRect::new(
            x.floor() as i32,
            y.floor() as i32,
            w.ceil() as i32,
            h.ceil() as i32,
        ),
        rotation: 0.,
    }
    .apply(&mut preview)?;
    Ok(Some(preview))
}
pub fn add_comment(
    editor: &mut Editor,
    object: NodeId,
    thread: Option<u64>,
    author: &str,
    text: &str,
) -> Result<u64, String> {
    validate_message(author, text)?;
    let mut result = 0;
    update(editor, |m| {
        if !m.contains_endpoint(object) {
            return Err("Comment object no longer exists".into());
        }
        let id = match thread {
            Some(id) => id,
            None => m
                .settings
                .threads
                .keys()
                .next_back()
                .copied()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or("Comment ID limit reached")?,
        };
        if thread.is_some() && !m.settings.threads.contains_key(&id) {
            return Err("Comment thread no longer exists".into());
        }
        let t = m.settings.threads.entry(id).or_insert(Thread {
            object,
            resolved: false,
            messages: vec![],
        });
        if t.object != object {
            return Err("Reply belongs to another object".into());
        }
        t.messages.push(Message {
            author: author.trim().into(),
            text: text.trim().into(),
        });
        t.resolved = false;
        result = id;
        Ok(())
    })?;
    Ok(result)
}
pub fn resolve_comment(editor: &mut Editor, id: u64, resolved: bool) -> Result<(), String> {
    update(editor, |m| {
        m.settings
            .threads
            .get_mut(&id)
            .ok_or("Comment thread no longer exists")?
            .resolved = resolved;
        Ok(())
    })
}
pub fn delete_comment_thread(editor: &mut Editor, id: u64) -> Result<(), String> {
    update(editor, |m| {
        m.settings
            .threads
            .remove(&id)
            .ok_or("Comment thread no longer exists")?;
        Ok(())
    })
}

/// Open these references in the editor's Open diagram link action. They never
/// execute URLs or load files; the matching project must already be open.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub project: Option<String>,
    pub page: u64,
    pub nodes: Vec<NodeId>,
    pub view: Option<[f64; 4]>,
}
impl Link {
    fn validate(&self) -> Result<(), String> {
        if self.page == 0
            || self.nodes.len() > 1000
            || self.nodes.contains(&0)
            || self.project.as_ref().is_some_and(|p| p.len() > 4096)
            || self.view.is_some_and(|v| {
                !v.iter().all(|n| n.is_finite() && n.abs() <= 1e6)
                    || v[2] < 0.01
                    || v[2] > 64.
                    || v[3].abs() > 360.
            })
        {
            return Err("Invalid diagram link".into());
        }
        Ok(())
    }
    pub fn encode(&self) -> Result<String, String> {
        self.validate()?;
        Ok(format!(
            "emulsion://diagram/{}",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(self).map_err(|e| e.to_string())?)
        ))
    }
    pub fn decode(text: &str) -> Result<Self, String> {
        if text.len() > 32768 {
            return Err("Diagram link exceeds its size limit".into());
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(
                text.trim()
                    .strip_prefix("emulsion://diagram/")
                    .ok_or("Paste an Emulsion diagram link")?,
            )
            .map_err(|_| "Invalid diagram link encoding")?;
        let value: Self = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        value.validate()?;
        Ok(value)
    }
    pub fn check_project(&self, editor: &crate::project::ProjectEditor) -> Result<(), String> {
        if self.project.is_none()
            || self.project.as_deref()
                != editor.path.as_ref().map(|p| p.to_string_lossy()).as_deref()
        {
            return Err("Open the project named in this link first".into());
        }
        let page = editor
            .page(self.page)
            .ok_or("Linked page no longer exists")?;
        if self.nodes.iter().any(|id| page.doc.node(*id).is_none()) {
            return Err("Linked selection no longer exists".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn infinite_canvas_is_optional_persistent_and_undoable() {
        let mut editor = Editor::new(Document::new(800, 600), None);
        set_infinite_canvas(&mut editor, true).unwrap();
        assert!(infinite_canvas(&editor.doc));
        let json = serde_json::to_value(editor.doc.diagram.as_ref().unwrap()).unwrap();
        let restored: Diagram = serde_json::from_value(json.clone()).unwrap();
        assert!(restored.settings.infinite_canvas);
        let mut legacy = json;
        legacy["settings"]
            .as_object_mut()
            .unwrap()
            .remove("infinite_canvas");
        let restored: Diagram = serde_json::from_value(legacy).unwrap();
        assert!(!restored.settings.infinite_canvas);
        editor.undo();
        assert!(!infinite_canvas(&editor.doc));
        editor.redo();
        assert!(infinite_canvas(&editor.doc));
        assert_eq!((editor.doc.width, editor.doc.height), (800, 600));
    }

    #[test]
    fn review_defaults_thumbnail_comments_copy_and_undo() {
        let mut editor = Editor::new(Document::new(900, 600), None);
        let a = add_shape(
            &mut editor,
            ShapeKind::Process,
            [80., 80., 180., 80.],
            "Source",
        )
        .unwrap();
        let body = editor.doc.diagram.as_ref().unwrap().shapes[&a].body;
        let NodeKind::Path {
            path, mut style, ..
        } = editor.doc.node(body).unwrap().kind.clone()
        else {
            panic!()
        };
        style.fill = Some([120, 220, 180, 255]);
        editor
            .execute(Command::SetPath {
                id: body,
                path,
                style,
            })
            .unwrap();
        set_default_style(&mut editor, Some(a), false).unwrap();
        let b = add_shape(
            &mut editor,
            ShapeKind::Process,
            [380., 80., 180., 80.],
            "New",
        )
        .unwrap();
        let body = editor.doc.diagram.as_ref().unwrap().shapes[&b].body;
        assert!(
            matches!(&editor.doc.node(body).unwrap().kind,NodeKind::Path{style,..} if style.fill==Some([120,220,180,255]))
        );
        let thread = add_comment(&mut editor, b, None, "Reviewer", "Check this step").unwrap();
        add_comment(&mut editor, b, Some(thread), "Author", "Updated").unwrap();
        resolve_comment(&mut editor, thread, true).unwrap();
        editor.undo();
        assert!(!editor.doc.diagram.as_ref().unwrap().settings.threads[&thread].resolved);
        set_thumbnail(&mut editor, vec![b]).unwrap();
        let preview = thumbnail_document(&editor.doc).unwrap().unwrap();
        assert!(preview.width < 300 && preview.height < 200);
        let fragment = crate::fragment::Fragment::capture(&editor.doc, &[b]).unwrap();
        let copied = fragment
            .paste(&mut editor, crate::command::Slot::TOP, (0., 180.))
            .unwrap();
        assert!(
            editor
                .doc
                .diagram
                .as_ref()
                .unwrap()
                .settings
                .threads
                .values()
                .any(|t| copied.contains(&t.object))
        );
        let saved = editor.doc.clone();
        editor.execute(Command::RemoveNode { id: b }).unwrap();
        editor.doc.validate().unwrap();
        assert!(
            editor
                .doc
                .diagram
                .as_ref()
                .unwrap()
                .settings
                .thumbnail
                .is_empty()
        );
        editor.undo();
        assert_eq!(editor.doc, saved);
    }
    #[test]
    fn deleting_body_retires_comments_without_blocking_edit_and_undo_restores() {
        let mut editor = Editor::new(Document::new(600, 400), None);
        let id = add_shape(
            &mut editor,
            ShapeKind::Process,
            [30., 30., 180., 80.],
            "Object",
        )
        .unwrap();
        add_comment(&mut editor, id, None, "A", "Review").unwrap();
        let before = editor.doc.clone();
        let body = editor.doc.diagram.as_ref().unwrap().shapes[&id].body;
        editor.execute(Command::RemoveNode { id: body }).unwrap();
        assert!(
            editor
                .doc
                .diagram
                .as_ref()
                .unwrap()
                .settings
                .threads
                .is_empty()
        );
        editor.undo();
        assert_eq!(editor.doc, before);
    }
    #[test]
    fn links_validate_and_roundtrip() {
        let link = Link {
            project: Some("/tmp/a project.emu".into()),
            page: 2,
            nodes: vec![12],
            view: Some([100., 200., 1.5, 30.]),
        };
        assert_eq!(Link::decode(&link.encode().unwrap()).unwrap(), link);
        assert!(Link::decode("https://example.com").is_err());
        assert!(
            Link {
                view: Some([0., 0., 0., 0.]),
                ..link
            }
            .encode()
            .is_err()
        );
    }
    #[test]
    fn structured_rotation_and_font_changes_keep_compartments_aligned() {
        let mut editor = Editor::new(Document::new(1200, 1000), None);
        let id = add_shape(
            &mut editor,
            ShapeKind::Class,
            [100., 100., 230., 200.],
            "Customer",
        )
        .unwrap();
        editor
            .execute(Command::RotateNode { id, degrees: 30. })
            .unwrap();
        let before = editor.doc.clone();
        let fields = structure::StructuredObject {
            title: "Customer".into(),
            attributes: vec!["id: uuid".into(), "name: string".into()],
            methods: vec!["save()".into()],
        };
        structure::set(&mut editor, id, ShapeKind::Class, fields).unwrap();
        let shape = &editor.doc.diagram.as_ref().unwrap().shapes[&id];
        let NodeKind::Text { spec, .. } = &editor.doc.node(shape.label).unwrap().kind else {
            panic!()
        };
        assert!((spec.rotation - 30.).abs() < 0.01);
        let NodeKind::Path { path, .. } = &editor.doc.node(shape.body).unwrap().kind else {
            panic!()
        };
        let line = &path.subpaths[1].anchors;
        let dx = line[1].p.0 - line[0].p.0;
        let dy = line[1].p.1 - line[0].p.1;
        assert!((dy.atan2(dx).to_degrees() - 30.).abs() < 0.01);
        editor.undo();
        assert_eq!(editor.doc, before);
    }
    #[test]
    fn structured_fields_resize_reflow_move_and_undo() {
        let mut editor = Editor::new(Document::new(900, 600), None);
        let id = add_shape(
            &mut editor,
            ShapeKind::Process,
            [30., 30., 180., 60.],
            "Class",
        )
        .unwrap();
        let original = editor.doc.clone();
        let fields = structure::StructuredObject {
            title: "Customer".into(),
            attributes: vec!["id: uuid [PK]".into(), "name: string".into()],
            methods: vec!["save(): bool".into()],
        };
        structure::set(&mut editor, id, ShapeKind::Class, fields.clone()).unwrap();
        assert_eq!(structure::get(&editor.doc, id), Some(fields));
        let b = shape_bounds(
            &editor.doc,
            &editor.doc.diagram.as_ref().unwrap().shapes[&id],
        )
        .unwrap();
        assert!(b[3] > 120.);
        let built = editor.doc.clone();
        editor
            .execute(Command::TranslateNode {
                id,
                dx: 60.,
                dy: 40.,
            })
            .unwrap();
        editor.doc.validate().unwrap();
        editor.undo();
        assert_eq!(editor.doc, built);
        editor.undo();
        assert_eq!(editor.doc, original);
    }
}
