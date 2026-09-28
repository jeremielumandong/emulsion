//! Offline editable diagram starters and shared UI/MCP themes.
use crate::{
    Command, Document, NodeId, NodeKind,
    diagram::{Builder, Endpoint, Port, Routing, ShapeKind},
};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub struct Template {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
}
pub const TEMPLATES: &[Template] = &[
    Template {
        id: "flowchart",
        name: "Flowchart",
        description: "A decision with two outcomes",
    },
    Template {
        id: "org-chart",
        name: "Org chart",
        description: "Teams and reporting relationships",
    },
    Template {
        id: "timeline",
        name: "Timeline",
        description: "Four connected project milestones",
    },
    Template {
        id: "concept-map",
        name: "Concept map",
        description: "A central idea with related concepts",
    },
    Template {
        id: "network",
        name: "Network diagram",
        description: "Client, gateway, services and database",
    },
    Template {
        id: "swimlanes",
        name: "Process with swimlanes",
        description: "Work shared between two teams",
    },
    Template {
        id: "kanban",
        name: "Kanban board",
        description: "To do, in progress and done containers",
    },
    Template {
        id: "uml",
        name: "UML classes",
        description: "Three connected editable class objects",
    },
];
#[derive(Clone, Copy)]
pub struct Theme {
    pub id: &'static str,
    pub name: &'static str,
    pub fill: [u8; 4],
    pub line: [u8; 4],
    pub text: [u8; 4],
}
pub const THEMES: &[Theme] = &[
    Theme {
        id: "neutral",
        name: "Monochrome grey",
        fill: [248, 250, 252, 255],
        line: [100, 116, 139, 255],
        text: [30, 41, 59, 255],
    },
    Theme {
        id: "blue",
        name: "Monochrome blue",
        fill: [239, 246, 255, 255],
        line: [59, 130, 246, 255],
        text: [30, 58, 138, 255],
    },
    Theme {
        id: "green",
        name: "Monochrome teal",
        fill: [236, 253, 245, 255],
        line: [16, 185, 129, 255],
        text: [6, 78, 59, 255],
    },
    Theme {
        id: "amber",
        name: "Monochrome earth",
        fill: [255, 251, 235, 255],
        line: [180, 130, 45, 255],
        text: [120, 83, 35, 255],
    },
    Theme {
        id: "chalk",
        name: "Chalk",
        fill: [245, 237, 255, 255],
        line: [145, 94, 190, 255],
        text: [70, 40, 100, 255],
    },
    Theme {
        id: "default",
        name: "Emulsion default",
        fill: [233, 239, 251, 255],
        line: [69, 96, 154, 255],
        text: [35, 47, 67, 255],
    },
];
impl Template {
    pub fn insert(self, editor: &mut crate::project::ProjectEditor) -> Result<u64, String> {
        if editor.kind() != Some(crate::project::ProjectKind::Diagram) {
            return Err("Open a Diagram project first".into());
        }
        let source = crate::project::ProjectEditor::new_project(
            crate::project::ProjectKind::Diagram,
            self.build()?,
        )?;
        let mut project = source.snapshot().ok_or("Cannot create template page")?;
        project.pages[0].meta.name = self.name.into();
        let pages = editor.import_pages(project)?;
        Ok(pages[0])
    }

    pub fn build(self) -> Result<Document, String> {
        use ShapeKind::*;
        let mut b = Builder::new(960, 640)?;
        let mut nodes = Vec::new();
        let mut containers = Vec::new();
        type ShapeSpec = (ShapeKind, [f64; 4], &'static str);
        type EdgeSpec = (usize, usize, &'static str);
        let (shapes, edges): (Vec<ShapeSpec>, Vec<EdgeSpec>) = match self.id {
            "flowchart" => (
                vec![
                    (Terminator, [80., 270., 150., 70.], "Start"),
                    (Decision, [350., 240., 180., 130.], "Approved?"),
                    (Process, [680., 140., 170., 80.], "Proceed"),
                    (Process, [680., 390., 170., 80.], "Review"),
                ],
                vec![(0, 1, ""), (1, 2, "Yes"), (1, 3, "No")],
            ),
            "org-chart" => (
                vec![
                    (Process, [385., 80., 190., 80.], "Team lead"),
                    (Process, [100., 280., 190., 80.], "Engineering"),
                    (Process, [385., 280., 190., 80.], "Design"),
                    (Process, [670., 280., 190., 80.], "Operations"),
                    (Process, [100., 470., 190., 80.], "Delivery"),
                ],
                vec![(0, 1, ""), (0, 2, ""), (0, 3, ""), (1, 4, "")],
            ),
            "timeline" => (
                vec![
                    (Terminator, [55., 260., 160., 90.], "Discover"),
                    (Process, [280., 260., 160., 90.], "Plan"),
                    (Process, [505., 260., 160., 90.], "Build"),
                    (Terminator, [730., 260., 160., 90.], "Launch"),
                ],
                vec![(0, 1, "Week 1"), (1, 2, "Week 2"), (2, 3, "Week 3")],
            ),
            "concept-map" => (
                vec![
                    (Terminator, [385., 270., 190., 90.], "Central idea"),
                    (Process, [70., 100., 180., 80.], "People"),
                    (Process, [70., 455., 180., 80.], "Process"),
                    (Process, [710., 100., 180., 80.], "Technology"),
                    (Process, [710., 455., 180., 80.], "Outcomes"),
                ],
                vec![(0, 1, ""), (0, 2, ""), (0, 3, ""), (0, 4, "")],
            ),
            "network" => (
                vec![
                    (Process, [50., 275., 150., 80.], "Client"),
                    (Cloud, [300., 250., 190., 120.], "Gateway"),
                    (Process, [650., 120., 180., 80.], "Application"),
                    (Database, [650., 390., 180., 100.], "Database"),
                ],
                vec![(0, 1, "HTTPS"), (1, 2, "API"), (2, 3, "Query")],
            ),
            "swimlanes" => {
                let a = b.add_shape(Swimlane, [45., 60., 870., 230.], "Requesting team")?;
                let c = b.add_shape(Swimlane, [45., 345., 870., 230.], "Delivery team")?;
                containers = vec![(0, a), (1, a), (2, c), (3, c)];
                (
                    vec![
                        (Terminator, [110., 145., 160., 75.], "Request"),
                        (Process, [400., 145., 160., 75.], "Approve"),
                        (Process, [400., 435., 160., 75.], "Implement"),
                        (Terminator, [680., 435., 160., 75.], "Deliver"),
                    ],
                    vec![(0, 1, ""), (1, 2, ""), (2, 3, "")],
                )
            }
            "kanban" => {
                for (i, label) in ["To do", "In progress", "Done"].into_iter().enumerate() {
                    let id =
                        b.add_shape(Container, [50. + i as f64 * 295., 70., 270., 490.], label)?;
                    containers.push((i, id));
                }
                (
                    vec![
                        (Note, [80., 180., 210., 110.], "Define requirements"),
                        (Note, [375., 180., 210., 110.], "Build prototype"),
                        (Note, [670., 180., 210., 110.], "Kickoff complete"),
                    ],
                    vec![],
                )
            }
            "uml" => (
                vec![
                    (
                        Class,
                        [65., 210., 230., 180.],
                        "Customer\nname: string\nemail: string",
                    ),
                    (
                        Class,
                        [365., 210., 230., 180.],
                        "Order\nid: integer\ntotal: decimal",
                    ),
                    (
                        Class,
                        [665., 210., 230., 180.],
                        "Product\nname: string\nprice: decimal",
                    ),
                ],
                vec![(0, 1, "places"), (1, 2, "contains")],
            ),
            _ => return Err("Unknown diagram template".into()),
        };
        for (kind, bounds, label) in shapes {
            nodes.push(b.add_shape(kind, bounds, label)?);
        }
        for (a, c, label) in edges {
            b.connect(
                Endpoint {
                    shape: nodes[a],
                    port: Port::Auto,
                },
                Endpoint {
                    shape: nodes[c],
                    port: Port::Auto,
                },
                label,
                Routing::Orthogonal,
            )?;
        }
        let mut doc = b.finish()?;
        for (index, parent) in containers {
            let id = nodes[index];
            doc.node_mut(id).unwrap().parent = Some(parent);
            Arc::make_mut(doc.diagram.as_mut().unwrap())
                .shapes
                .get_mut(&id)
                .unwrap()
                .container = Some(parent);
        }
        doc.normalize();
        doc.validate().map_err(|e| e.to_string())?;
        Ok(doc)
    }
}
/// A command plan lets project and document hosts preserve their own undo boundary.
pub fn theme_commands(
    doc: &Document,
    roots: &[NodeId],
    theme: Theme,
) -> Result<Vec<Command>, String> {
    let model = doc.diagram.as_ref().ok_or("This page is not a diagram")?;
    if roots.iter().any(|id| doc.node(*id).is_none()) {
        return Err("Selected object no longer exists".into());
    }
    let mut commands = Vec::new();
    for n in &doc.nodes {
        if !roots
            .iter()
            .any(|id| *id == n.id || doc.is_ancestor(*id, n.id))
        {
            continue;
        }
        match &n.kind {
            NodeKind::Path { path, style, .. } => {
                let mut style = *style;
                let edge = model
                    .edges
                    .values()
                    .any(|e| e.path == n.id || e.arrow == n.id);
                if style.fill.is_some_and(|c| c[3] > 0) {
                    style.fill = Some(if edge { theme.line } else { theme.fill });
                }
                if style.stroke.is_some() {
                    style.stroke = Some(theme.line);
                    style.width = 1.5;
                }
                commands.push(Command::SetPath {
                    id: n.id,
                    path: path.clone(),
                    style,
                });
            }
            NodeKind::Text { spec, .. } => {
                let mut spec = (**spec).clone();
                spec.color = theme.text;
                spec.apply_style(0..spec.text.len(), |s| s.color = theme.text);
                commands.push(Command::SetText {
                    id: n.id,
                    spec: Box::new(spec),
                });
            }
            _ => {}
        }
    }
    Ok(commands)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_starters_are_editable_valid_and_themes_undo_atomically() {
        for template in TEMPLATES {
            let doc = template.build().unwrap();
            doc.validate().unwrap();
            assert!(doc.diagram.as_ref().unwrap().shapes.len() >= 3);
            for theme in THEMES {
                let mut e = crate::Editor::new(doc.clone(), None);
                e.begin("Theme");
                for command in theme_commands(&doc, &doc.children(None), *theme).unwrap() {
                    e.execute(command).unwrap();
                }
                e.end();
                e.doc.validate().unwrap();
                e.undo();
                assert_eq!(e.doc, doc);
            }
        }
    }
}
