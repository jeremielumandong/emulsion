//! Construct a new graph off the editing thread without per-object undo snapshots.
use super::*;
pub struct Builder {
    doc: Document,
    model: Diagram,
    edges: Vec<Node>,
}
impl Builder {
    pub fn new(width: u32, height: u32) -> Result<Self, String> {
        let mut doc = Document::new(width, height);
        doc.validate().map_err(|e| e.to_string())?;
        let id = doc.alloc_id();
        doc.nodes.push(Node::new(
            id,
            "Background",
            NodeKind::Fill { rgba: [255; 4] },
        ));
        Ok(Self {
            doc,
            model: Diagram::default(),
            edges: Vec::new(),
        })
    }
    pub fn add_shape(
        &mut self,
        kind: ShapeKind,
        bounds: Bounds,
        label: &str,
    ) -> Result<NodeId, String> {
        if !valid_bounds(bounds)
            || label.chars().count() > crate::text::MAX_CHARS
            || self.model.shapes.len() >= MAX_SHAPES
            || self.doc.nodes.len() + self.edges.len() + 3 > crate::document::MAX_NODES
        {
            return Err("Shape or graph size limit exceeded.".into());
        }
        let [x, y, w, h] = bounds;
        let group = self.doc.alloc_id();
        let body = self.doc.alloc_id();
        let label_id = self.doc.alloc_id();
        let color = if kind == ShapeKind::Note {
            [255, 237, 166, 255]
        } else if kind.is_container() {
            [241, 244, 249, 255]
        } else {
            [233, 239, 251, 255]
        };
        let mut path = Node::path(
            body,
            "Shape",
            Arc::new(kind.path(bounds)),
            PathStyle {
                fill: Some(color),
                stroke: Some([69, 96, 154, 255]),
                width: 2.,
                ..Default::default()
            },
            self.doc.width,
            self.doc.height,
        );
        path.parent = Some(group);
        let mut text = Node::text(
            label_id,
            "Label",
            TextSpec {
                text: label.into(),
                font: "Geist".into(),
                size: 14.,
                x: (x + 8.) as f32,
                y: (y + if kind.is_container()
                    || matches!(kind, ShapeKind::Class | ShapeKind::Entity)
                {
                    8.
                } else {
                    (h - 20.) / 2.
                }) as f32,
                width: Some((w - 16.).max(1.) as f32),
                align: Align::Center,
                color: [35, 47, 67, 255],
                ..Default::default()
            },
            self.doc.width,
            self.doc.height,
        );
        text.parent = Some(group);
        self.doc.nodes.extend([
            path,
            text,
            Node::new(group, kind.label(), NodeKind::Group { collapsed: false }),
        ]);
        self.model.shapes.insert(
            group,
            Shape {
                body,
                label: label_id,
                kind,
                container: None,
                data: BTreeMap::new(),
                layout_locked: false,
                conditions: Vec::new(),
                unconditional_style: None,
            },
        );
        Ok(group)
    }
    pub fn connect(
        &mut self,
        source: Endpoint,
        target: Endpoint,
        label: &str,
        routing: Routing,
    ) -> Result<NodeId, String> {
        if !self.model.shapes.contains_key(&source.shape)
            || !self.model.shapes.contains_key(&target.shape)
            || !source.port.valid()
            || !target.port.valid()
            || label.chars().count() > crate::text::MAX_CHARS
            || self.model.edges.len() >= MAX_EDGES
            || self.doc.nodes.len() + self.edges.len() + 4 > crate::document::MAX_NODES
        {
            return Err("Invalid connection or graph size limit exceeded.".into());
        }
        let group = self.doc.alloc_id();
        let path = self.doc.alloc_id();
        let arrow = self.doc.alloc_id();
        let label_id = self.doc.alloc_id();
        for (id, name, fill) in [
            (path, "Connection", None),
            (arrow, "Arrow", Some([69, 96, 154, 255])),
        ] {
            let mut node = Node::path(
                id,
                name,
                Arc::new(Path::default()),
                PathStyle {
                    fill,
                    stroke: Some([69, 96, 154, 255]),
                    width: 2.,
                    ..Default::default()
                },
                self.doc.width,
                self.doc.height,
            );
            node.parent = Some(group);
            self.edges.push(node);
        }
        let mut text = Node::text(
            label_id,
            "Connector label",
            TextSpec {
                text: label.into(),
                font: "Geist".into(),
                size: 12.,
                width: Some(120.),
                align: Align::Center,
                color: [35, 47, 67, 255],
                ..Default::default()
            },
            self.doc.width,
            self.doc.height,
        );
        text.parent = Some(group);
        self.edges.extend([
            text,
            Node::new(group, "Connector", NodeKind::Group { collapsed: false }),
        ]);
        self.model.edges.insert(
            group,
            Edge {
                path,
                arrow,
                label: label_id,
                source,
                target,
                routing,
                waypoints: Vec::new(),
                label_offset: (0., 0.),
                start_marker: Marker::default(),
                end_marker: Marker::default(),
                arrow_end: true,
                arrow_start: false,
            },
        );
        Ok(group)
    }
    pub fn finish(mut self) -> Result<Document, String> {
        self.doc.nodes.splice(1..1, self.edges);
        self.doc.diagram = Some(Arc::new(self.model));
        self.doc.validate().map_err(|e| e.to_string())?;
        synchronize(
            &Document::new(self.doc.width, self.doc.height),
            &mut self.doc,
        )?;
        self.doc.validate().map_err(|e| e.to_string())?;
        Ok(self.doc)
    }
}
