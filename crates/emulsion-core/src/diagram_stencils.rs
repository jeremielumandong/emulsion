//! Bundled, original vector stencils. No network, account, or vendor assets required.
use super::*;

#[derive(Clone, Copy, Debug)]
pub struct Stencil {
    pub id: &'static str,
    pub label: &'static str,
    pub category: &'static str,
    pub keywords: &'static str,
    pub kind: ShapeKind,
    svg: Option<&'static str>,
    pub(super) label_below: bool,
}
impl Stencil {
    pub fn default_size(self) -> (f64, f64) {
        if self.id.starts_with("web-") {
            (288., 148.)
        } else if self.kind.is_container() {
            (420., 280.)
        } else {
            (140., 80.)
        }
    }

    pub fn path(self, [x, y, w, h]: Bounds) -> Path {
        if let Some(icon) = self.id.strip_prefix("web-") {
            let icon = match icon {
                "browser" => "browser",
                "database" | "cache" | "storage" => "database",
                "queue" | "outbox" => "queue",
                "api" | "worker" => "server",
                "auth" | "error" => "firewall",
                "edge" | "gateway" | "websocket" => "cloud",
                _ => "document",
            };
            let mut path = self.kind.default_path([x, y, w, h]);
            if let Some(stencil) = STENCILS.iter().find(|s| s.id == icon) {
                path.subpaths.extend(
                    stencil
                        .path([x + w * 0.6, y + h * 0.15, w * 0.28, h * 0.4])
                        .subpaths,
                );
            }
            return path;
        }
        if let Some(svg) = self.svg {
            let mut path = Path::from_svg(svg).expect("bundled stencil path");
            path.transform(glam::DAffine2::from_cols_array(&[
                w / 100.,
                0.,
                0.,
                h / 100.,
                x,
                y,
            ]));
            path
        } else {
            self.kind.default_path([x, y, w, h])
        }
    }
    pub fn matches(self, query: &str) -> bool {
        let words = format!("{} {} {}", self.label, self.category, self.keywords).to_lowercase();
        query
            .split_whitespace()
            .all(|q| words.contains(&q.to_lowercase()))
    }
    pub fn insert(self, editor: &mut Editor, bounds: Bounds) -> Result<NodeId, String> {
        if editor.in_transaction() {
            return Err("Finish the current edit first.".into());
        }
        if self.id.starts_with("web-") {
            return crate::diagram_library::insert_web_stencil(editor, self.id, bounds);
        }
        editor.begin("Insert diagram stencil");
        let result = (|| {
            let id = add_shape_inner(editor, self.kind, bounds, self.label, false)?;
            let mut model = editor.doc.diagram.as_deref().unwrap().clone();
            let shape = model.shapes.get_mut(&id).unwrap();
            shape.data.insert("emulsion_stencil".into(), self.id.into());
            let body = shape.body;
            let label = shape.label;
            let NodeKind::Path { style, .. } = &editor.doc.node(body).unwrap().kind else {
                unreachable!()
            };
            if !matches!(self.kind, ShapeKind::Class | ShapeKind::Entity) {
                editor
                    .execute(Command::SetPath {
                        id: body,
                        path: Arc::new(self.path(bounds)),
                        style: *style,
                    })
                    .map_err(|e| e.to_string())?;
            }
            if self.label_below {
                let NodeKind::Text { spec, .. } = &editor.doc.node(label).unwrap().kind else {
                    unreachable!()
                };
                let mut spec = spec.as_ref().clone();
                spec.x = bounds[0] as f32;
                spec.y = (bounds[1] + bounds[3] + 5.) as f32;
                spec.width = Some(bounds[2] as f32);
                editor
                    .execute(Command::SetText {
                        id: label,
                        spec: Box::new(spec),
                    })
                    .map_err(|e| e.to_string())?;
            }
            editor
                .execute(Command::Rename {
                    id,
                    name: self.label.into(),
                })
                .map_err(|e| e.to_string())?;
            editor
                .execute(Command::SetDiagram {
                    diagram: Some(Arc::new(model)),
                })
                .map_err(|e| e.to_string())?;
            workspace::apply_default(editor, id, false)?;
            Ok(id)
        })();
        match result {
            Ok(id) => {
                editor.end();
                Ok(id)
            }
            Err(e) => {
                editor.cancel();
                Err(e)
            }
        }
    }
}
macro_rules! native {
    ($id:literal,$label:literal,$category:literal,$keywords:literal,$kind:ident) => {
        Stencil {
            id: $id,
            label: $label,
            category: $category,
            keywords: $keywords,
            kind: ShapeKind::$kind,
            svg: None,
            label_below: false,
        }
    };
}
macro_rules! vector {
    ($id:literal,$label:literal,$category:literal,$keywords:literal,$path:literal,$below:literal) => {
        Stencil {
            id: $id,
            label: $label,
            category: $category,
            keywords: $keywords,
            kind: ShapeKind::Process,
            svg: Some($path),
            label_below: $below,
        }
    };
}
pub const CATEGORIES: &[&str] = &[
    "Flowchart",
    "General",
    "UML / Software",
    "Entity relationship",
    "BPMN / Business",
    "Network / Infrastructure",
    "Cloud / Architecture",
    "Wireframe / UX",
    "Office / Floor plan",
    "Electrical",
    "Planning",
    "Web systems",
];
/// The initial twelve IDs retain the existing drawer's ordering for shortcuts.
pub const STENCILS: &[Stencil] = &[
    native!(
        "process",
        "Process",
        "Flowchart",
        "step activity task",
        Process
    ),
    native!(
        "decision",
        "Decision",
        "Flowchart",
        "branch choice gateway diamond",
        Decision
    ),
    native!(
        "terminator",
        "Start / End",
        "Flowchart",
        "terminal rounded",
        Terminator
    ),
    native!(
        "data",
        "Input / Output",
        "Flowchart",
        "data parallelogram",
        Data
    ),
    native!(
        "database",
        "Database",
        "Flowchart",
        "storage cylinder sql",
        Database
    ),
    native!(
        "document",
        "Document",
        "Flowchart",
        "file report paper",
        Document
    ),
    native!("note", "Note", "General", "annotation comment sticky", Note),
    native!(
        "uml-class",
        "UML Class",
        "UML / Software",
        "attributes methods object",
        Class
    ),
    native!(
        "entity",
        "Entity",
        "Entity relationship",
        "erd table record fields database sql",
        Entity
    ),
    native!(
        "container",
        "Container",
        "General",
        "group frame boundary",
        Container
    ),
    native!(
        "swimlane",
        "Swimlane",
        "BPMN / Business",
        "pool lane team responsibility",
        Swimlane
    ),
    native!(
        "cloud",
        "Cloud",
        "Cloud / Architecture",
        "internet service external",
        Cloud
    ),
    vector!(
        "ellipse",
        "Ellipse",
        "General",
        "circle oval",
        "M100 50C100 77.6142 77.6142 100 50 100C22.3858 100 0 77.6142 0 50C0 22.3858 22.3858 0 50 0C77.6142 0 100 22.3858 100 50Z",
        false
    ),
    vector!(
        "triangle",
        "Triangle",
        "General",
        "direction warning",
        "M50 0L100 100H0Z",
        false
    ),
    vector!(
        "hexagon",
        "Preparation",
        "Flowchart",
        "hexagon initialize",
        "M25 0H75L100 50L75 100H25L0 50Z",
        false
    ),
    vector!(
        "subprocess",
        "Subprocess",
        "Flowchart",
        "predefined process function",
        "M0 0H100V100H0Z M12 0V100 M88 0V100",
        false
    ),
    vector!(
        "manual-input",
        "Manual input",
        "Flowchart",
        "entry keyboard",
        "M0 25L100 0V100H0Z",
        false
    ),
    vector!(
        "delay",
        "Delay",
        "Flowchart",
        "wait queue",
        "M0 0H50C117 0 117 100 50 100H0Z",
        false
    ),
    vector!(
        "off-page",
        "Off-page connector",
        "Flowchart",
        "reference continuation",
        "M0 0H100V65L50 100L0 65Z",
        false
    ),
    vector!(
        "actor",
        "Actor",
        "UML / Software",
        "person user participant role",
        "M65 15C65 23.2843 58.2843 30 50 30C41.7157 30 35 23.2843 35 15C35 6.71573 41.7157 0 50 0C58.2843 0 65 6.71573 65 15Z M50 30V65 M10 43H90 M50 65L10 100 M50 65L90 100",
        true
    ),
    vector!(
        "component",
        "Component",
        "UML / Software",
        "module service software",
        "M15 0H100V100H15Z M0 20H30V38H0Z M0 60H30V78H0Z",
        false
    ),
    vector!(
        "deployment",
        "Deployment node",
        "UML / Software",
        "host system hardware",
        "M0 20L20 0H100V80L80 100H0Z M0 20H80V100 M80 20L100 0",
        false
    ),
    vector!(
        "package",
        "Package",
        "UML / Software",
        "namespace folder",
        "M0 20V0H40V20H100V100H0Z M0 20H40",
        false
    ),
    vector!(
        "use-case",
        "Use case",
        "UML / Software",
        "scenario requirement oval",
        "M100 50C100 77.6142 77.6142 100 50 100C22.3858 100 0 77.6142 0 50C0 22.3858 22.3858 0 50 0C77.6142 0 100 22.3858 100 50Z",
        false
    ),
    vector!(
        "lifeline",
        "Lifeline",
        "UML / Software",
        "sequence participant time",
        "M0 0H100V30H0Z M50 30V45 M50 55V65 M50 75V85 M50 95V100",
        true
    ),
    vector!(
        "weak-entity",
        "Weak entity",
        "Entity relationship",
        "erd dependent double rectangle",
        "M0 0H100V100H0Z M6 6H94V94H6Z",
        false
    ),
    native!(
        "relationship",
        "Relationship",
        "Entity relationship",
        "erd association diamond",
        Decision
    ),
    vector!(
        "attribute",
        "Attribute",
        "Entity relationship",
        "erd property field oval",
        "M100 50C100 77.6142 77.6142 100 50 100C22.3858 100 0 77.6142 0 50C0 22.3858 22.3858 0 50 0C77.6142 0 100 22.3858 100 50Z",
        false
    ),
    vector!(
        "multi-attribute",
        "Multivalued attribute",
        "Entity relationship",
        "erd multiple property",
        "M100 50C100 77.6142 77.6142 100 50 100C22.3858 100 0 77.6142 0 50C0 22.3858 22.3858 0 50 0C77.6142 0 100 22.3858 100 50Z M93 50C93 73.7482 73.7482 93 50 93C26.2518 93 7 73.7482 7 50C7 26.2518 26.2518 7 50 7C73.7482 7 93 26.2518 93 50Z",
        false
    ),
    vector!(
        "bpmn-event",
        "Start event",
        "BPMN / Business",
        "bpmn circle trigger",
        "M100 50C100 77.6142 77.6142 100 50 100C22.3858 100 0 77.6142 0 50C0 22.3858 22.3858 0 50 0C77.6142 0 100 22.3858 100 50Z",
        false
    ),
    vector!(
        "bpmn-intermediate",
        "Intermediate event",
        "BPMN / Business",
        "bpmn timer boundary",
        "M100 50C100 77.6142 77.6142 100 50 100C22.3858 100 0 77.6142 0 50C0 22.3858 22.3858 0 50 0C77.6142 0 100 22.3858 100 50Z M92 50C92 73.196 73.196 92 50 92C26.804 92 8 73.196 8 50C8 26.804 26.804 8 50 8C73.196 8 92 26.804 92 50Z",
        false
    ),
    native!(
        "bpmn-task",
        "Task",
        "BPMN / Business",
        "bpmn work activity",
        Process
    ),
    vector!(
        "exclusive-gateway",
        "Exclusive gateway",
        "BPMN / Business",
        "bpmn xor decision",
        "M50 0L100 50L50 100L0 50Z M35 35L65 65 M65 35L35 65",
        true
    ),
    vector!(
        "parallel-gateway",
        "Parallel gateway",
        "BPMN / Business",
        "bpmn and fork join",
        "M50 0L100 50L50 100L0 50Z M30 50H70 M50 30V70",
        true
    ),
    vector!(
        "message",
        "Message",
        "BPMN / Business",
        "bpmn email envelope",
        "M0 0H100V100H0Z M0 0L50 55L100 0",
        true
    ),
    vector!(
        "server",
        "Server",
        "Network / Infrastructure",
        "host computer rack compute",
        "M10 0H90V100H10Z M10 30H90 M10 60H90 M22 14H30 M22 44H30 M22 78H30 M45 14H78 M45 44H78 M45 78H78",
        true
    ),
    vector!(
        "router",
        "Router",
        "Network / Infrastructure",
        "routing gateway network",
        "M0 15H100V85H0Z M15 38H85 M70 25L85 38L70 51 M85 65H15 M30 52L15 65L30 78",
        true
    ),
    vector!(
        "switch",
        "Network switch",
        "Network / Infrastructure",
        "ethernet lan ports",
        "M0 20H100V80H0Z M10 42H25V60H10Z M32 42H47V60H32Z M54 42H69V60H54Z M76 42H91V60H76Z",
        true
    ),
    vector!(
        "firewall",
        "Firewall",
        "Network / Infrastructure",
        "security protection boundary",
        "M0 0H100V100H0Z M0 33H100 M0 66H100 M33 0V33 M66 0V33 M50 33V66 M33 66V100 M66 66V100",
        true
    ),
    vector!(
        "workstation",
        "Workstation",
        "Network / Infrastructure",
        "desktop pc monitor computer",
        "M0 0H100V75H0Z M40 75V100 M60 75V100 M20 100H80 M0 62H100",
        true
    ),
    vector!(
        "rack",
        "Equipment rack",
        "Network / Infrastructure",
        "cabinet data center datacenter server",
        "M5 0H95V100H5Z M15 10H85V30H15Z M15 40H85V60H15Z M15 70H85V90H15Z",
        true
    ),
    vector!(
        "wifi",
        "Wireless access point",
        "Network / Infrastructure",
        "wifi wlan antenna",
        "M0 25Q50 -15 100 25 M15 45Q50 15 85 45 M32 65Q50 45 68 65 M58 86C58 90.4183 54.4183 94 50 94C45.5817 94 42 90.4183 42 86C42 81.5817 45.5817 78 50 78C54.4183 78 58 81.5817 58 86Z",
        true
    ),
    vector!(
        "load-balancer",
        "Load balancer",
        "Cloud / Architecture",
        "distribution traffic proxy",
        "M0 50H40 M40 50V10H100 M40 50H100 M40 50V90H100 M85 0L100 10L85 20 M85 40L100 50L85 60 M85 80L100 90L85 100",
        true
    ),
    vector!(
        "object-store",
        "Object storage",
        "Cloud / Architecture",
        "bucket blob s3 azure gcp aws storage",
        "M0 10Q50 -10 100 10L85 90Q50 110 15 90Z M0 10Q50 30 100 10",
        true
    ),
    vector!(
        "container-service",
        "Container service",
        "Cloud / Architecture",
        "docker kubernetes pod orchestration",
        "M50 0L100 25V75L50 100L0 75V25Z M0 25L50 50L100 25 M50 50V100",
        true
    ),
    vector!(
        "function",
        "Function",
        "Cloud / Architecture",
        "serverless lambda compute api",
        "M0 0H100V100H0Z M30 20H48L70 80 M48 45L25 80 M65 80H82",
        true
    ),
    vector!(
        "queue",
        "Message queue",
        "Cloud / Architecture",
        "events messaging broker pubsub kafka",
        "M0 10H100V90H0Z M20 10V90 M40 10V90 M60 10V90 M80 10V90",
        true
    ),
    vector!(
        "browser",
        "Browser window",
        "Wireframe / UX",
        "web application website page",
        "M0 0H100V100H0Z M0 20H100 M10 10H15 M22 10H27 M34 10H39",
        true
    ),
    vector!(
        "mobile",
        "Mobile screen",
        "Wireframe / UX",
        "phone app ios android handset",
        "M20 0H80V100H20Z M20 12H80 M20 85H80 M45 93H55",
        true
    ),
    vector!(
        "button",
        "Button",
        "Wireframe / UX",
        "control action form",
        "M8 15H92Q100 15 100 25V75Q100 85 92 85H8Q0 85 0 75V25Q0 15 8 15Z",
        false
    ),
    vector!(
        "input",
        "Text input",
        "Wireframe / UX",
        "form field entry",
        "M0 20H100V80H0Z M12 32V68",
        true
    ),
    vector!(
        "image-placeholder",
        "Image placeholder",
        "Wireframe / UX",
        "picture photo media",
        "M0 0H100V100H0Z M0 100L35 45L60 75L80 40L100 75 M82 20C82 24.4183 78.4183 28 74 28C69.5817 28 66 24.4183 66 20C66 15.5817 69.5817 12 74 12C78.4183 12 82 15.5817 82 20Z",
        true
    ),
    vector!(
        "checkbox",
        "Checkbox",
        "Wireframe / UX",
        "form checked toggle",
        "M5 5H95V95H5Z M20 50L42 73L82 25",
        true
    ),
    vector!(
        "desk",
        "Desk",
        "Office / Floor plan",
        "workstation office table furniture",
        "M0 0H100V100H0Z M5 5H95V80H5Z M10 90H90",
        true
    ),
    vector!(
        "chair",
        "Chair",
        "Office / Floor plan",
        "seat furniture",
        "M20 0H80V25H20Z M10 30H90V90H10Z M25 90V100 M75 90V100",
        true
    ),
    vector!(
        "meeting-table",
        "Meeting table",
        "Office / Floor plan",
        "conference room office furniture",
        "M20 20H80V80H20Z M30 0H45V15H30Z M55 0H70V15H55Z M30 85H45V100H30Z M55 85H70V100H55Z M0 40H15V60H0Z M85 40H100V60H85Z",
        true
    ),
    vector!(
        "door",
        "Door",
        "Office / Floor plan",
        "entrance architecture building",
        "M0 0V100H100 M0 0C55.2285 0 100 44.7715 100 100",
        true
    ),
    vector!(
        "stairs",
        "Stairs",
        "Office / Floor plan",
        "steps building architecture",
        "M0 0H100V100H0Z M0 20H100 M0 40H100 M0 60H100 M0 80H100",
        true
    ),
    vector!(
        "resistor",
        "Resistor",
        "Electrical",
        "circuit electronics passive resistance",
        "M0 50H15L25 20L35 80L45 20L55 80L65 20L75 80L85 50H100",
        true
    ),
    vector!(
        "capacitor",
        "Capacitor",
        "Electrical",
        "circuit electronics passive capacitance",
        "M0 50H40 M40 15V85 M60 15V85 M60 50H100",
        true
    ),
    vector!(
        "ground",
        "Ground",
        "Electrical",
        "earth circuit reference",
        "M50 0V50 M0 50H100 M20 72H80 M40 94H60",
        true
    ),
    vector!(
        "battery",
        "Battery",
        "Electrical",
        "power source dc circuit",
        "M0 50H25 M25 10V90 M45 30V70 M65 10V90 M85 30V70 M85 50H100",
        true
    ),
    vector!(
        "diode",
        "Diode",
        "Electrical",
        "semiconductor rectifier electronics circuit",
        "M0 50H25 M25 10L75 50L25 90Z M75 10V90 M75 50H100",
        true
    ),
    vector!(
        "electrical-switch",
        "Switch",
        "Electrical",
        "contact circuit open relay",
        "M0 70H25L80 15 M75 70H100",
        true
    ),
    vector!(
        "milestone",
        "Milestone",
        "Planning",
        "timeline project gantt deliverable",
        "M50 0L100 50L50 100L0 50Z",
        false
    ),
    vector!(
        "calendar",
        "Calendar",
        "Planning",
        "schedule date project",
        "M0 10H100V100H0Z M0 30H100 M25 0V20 M75 0V20 M33 30V100 M66 30V100 M0 53H100 M0 76H100",
        true
    ),
    vector!(
        "kanban",
        "Kanban board",
        "Planning",
        "agile project workflow tasks",
        "M0 0H100V100H0Z M33 0V100 M66 0V100 M0 20H100 M6 30H27V50H6Z M39 30H60V50H39Z M72 30H94V50H72Z",
        true
    ),
    vector!(
        "org-role",
        "Organization role",
        "Planning",
        "org chart team person hierarchy",
        "M0 0H100V100H0Z M0 30H100",
        false
    ),
    native!(
        "web-browser",
        "Browser / client",
        "Web systems",
        "frontend request",
        Process
    ),
    native!(
        "web-edge",
        "DNS / CDN edge",
        "Web systems",
        "network cache",
        Process
    ),
    native!(
        "web-gateway",
        "API gateway",
        "Web systems",
        "routing proxy",
        Process
    ),
    native!(
        "web-auth",
        "Identity / auth",
        "Web systems",
        "security token",
        Process
    ),
    native!(
        "web-api",
        "Application server",
        "Web systems",
        "backend service",
        Process
    ),
    native!(
        "web-database",
        "Database",
        "Web systems",
        "persistence sql",
        Process
    ),
    native!(
        "web-cache",
        "Cache",
        "Web systems",
        "redis storage",
        Process
    ),
    native!(
        "web-queue",
        "Message queue",
        "Web systems",
        "async event",
        Process
    ),
    native!(
        "web-worker",
        "Background worker",
        "Web systems",
        "async job",
        Process
    ),
    native!(
        "web-response",
        "HTTP response",
        "Web systems",
        "success payload",
        Process
    ),
    native!(
        "web-error",
        "Error / retry",
        "Web systems",
        "failure timeout",
        Process
    ),
    native!(
        "web-websocket",
        "WebSocket connection",
        "Web systems",
        "realtime socket",
        Process
    ),
    native!(
        "web-outbox",
        "Transactional outbox",
        "Web systems",
        "events delivery",
        Process
    ),
    native!(
        "web-storage",
        "Object storage",
        "Web systems",
        "assets files",
        Process
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_default_stencil_is_editable_connectable_and_one_undo_step() {
        let mut ids = HashSet::new();
        for stencil in STENCILS {
            assert!(ids.insert(stencil.id));
            assert!(CATEGORIES.contains(&stencil.category));
            let mut editor = Editor::new(Document::new(800, 600), None);
            let id = stencil
                .insert(&mut editor, [100., 100., 140., 80.])
                .unwrap();
            editor.doc.validate().unwrap();
            assert_eq!(
                editor.doc.diagram.as_ref().unwrap().shapes[&id].data["emulsion_stencil"],
                stencil.id
            );
            editor.undo();
            assert!(editor.doc.nodes.is_empty());
            editor.redo();
            let target = add_shape(
                &mut editor,
                ShapeKind::Process,
                [400., 100., 120., 60.],
                "Target",
            )
            .unwrap();
            connect(
                &mut editor,
                Endpoint {
                    shape: id,
                    port: Port::East,
                },
                Endpoint {
                    shape: target,
                    port: Port::West,
                },
                "",
                Routing::Orthogonal,
            )
            .unwrap();
            editor.doc.validate().unwrap();
        }
    }
    #[test]
    fn search_uses_categories_and_keywords() {
        assert!(STENCILS.iter().any(|s| s.matches("network firewall")));
        assert!(STENCILS.iter().any(|s| s.matches("aws storage")));
        assert!(STENCILS.iter().any(|s| s.matches("electrical capacitor")));
        assert!(!STENCILS.iter().any(|s| s.matches("not-a-stencil")));
    }
}
