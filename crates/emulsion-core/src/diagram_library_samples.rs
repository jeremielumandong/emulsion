//! Original editable compositions inspired by the user's local Diagram references.
//! No downloaded page scripts, stock photos, branding, or flattened diagrams are bundled.
use crate::{
    Document, Node, NodeId, NodeKind,
    diagram::{self, Builder, Endpoint, Port, Routing},
    text::TextSpec,
    vector_cache::VectorRaster,
};
use std::sync::Arc;
type Color = [u8; 4];
const INK: Color = [65, 73, 83, 255];
const WHITE: Color = [255; 4];
const BLUE: Color = [227, 239, 255, 255];
const TEAL: Color = [196, 240, 232, 255];
const PURPLE: Color = [234, 225, 250, 255];
const AMBER: Color = [255, 235, 199, 255];
const ROSE: Color = [250, 222, 230, 255];
const COLORS: [Color; 5] = [BLUE, TEAL, PURPLE, AMBER, ROSE];

struct ShapeStyle {
    id: NodeId,
    fill: Color,
    bounds: [f64; 4],
    caption: bool,
}
struct Connection {
    id: NodeId,
    arrow: bool,
    points: Vec<(f64, f64)>,
}
struct Composition {
    builder: Builder,
    shapes: Vec<ShapeStyle>,
    edges: Vec<Connection>,
    parents: Vec<(NodeId, NodeId)>,
    text: Vec<TextSpec>,
}
impl Composition {
    fn new(title: &str, subtitle: &str, width: u32, height: u32) -> Result<Self, String> {
        let mut c = Self {
            builder: Builder::new(width, height)?,
            shapes: vec![],
            edges: vec![],
            parents: vec![],
            text: vec![],
        };
        c.text(title, [48., 32., width as f64 - 96., 40.], 30., true);
        c.text(subtitle, [48., 79., width as f64 - 96., 30.], 14., false);
        Ok(c)
    }
    fn text(&mut self, text: &str, b: [f64; 4], size: f32, bold: bool) {
        self.text.push(TextSpec {
            text: text.into(),
            font: "Geist".into(),
            x: b[0] as f32,
            y: b[1] as f32,
            width: Some(b[2] as f32),
            size,
            bold,
            color: INK,
            ..Default::default()
        });
    }
    fn shape(
        &mut self,
        stencil: &str,
        b: [f64; 4],
        label: &str,
        fill: Color,
    ) -> Result<NodeId, String> {
        let stencil = *diagram::stencils::STENCILS
            .iter()
            .find(|s| s.id == stencil)
            .ok_or("Missing bundled stencil")?;
        let id = self.builder.add_stencil(stencil, b, label)?;
        // Icon captions are already positioned by the shared stencil builder.
        let caption = matches!(
            stencil.id,
            "actor"
                | "server"
                | "router"
                | "workstation"
                | "firewall"
                | "load-balancer"
                | "container-service"
                | "queue"
                | "object-store"
        );
        self.shapes.push(ShapeStyle {
            id,
            fill,
            bounds: b,
            caption,
        });
        Ok(id)
    }
    fn edge(
        &mut self,
        a: (NodeId, Port),
        b: (NodeId, Port),
        label: &str,
        routing: Routing,
        arrow: bool,
    ) -> Result<NodeId, String> {
        let id = self.builder.connect(
            Endpoint {
                shape: a.0,
                port: a.1,
            },
            Endpoint {
                shape: b.0,
                port: b.1,
            },
            label,
            routing,
        )?;
        self.edges.push(Connection {
            id,
            arrow,
            points: vec![],
        });
        Ok(id)
    }
    fn link(&mut self, a: NodeId, b: NodeId, label: &str) -> Result<NodeId, String> {
        self.edge(
            (a, Port::Auto),
            (b, Port::Auto),
            label,
            Routing::Orthogonal,
            true,
        )
    }
    fn hierarchy(&mut self, a: NodeId, b: NodeId) -> Result<NodeId, String> {
        self.edge(
            (a, Port::South),
            (b, Port::North),
            "",
            Routing::Orthogonal,
            false,
        )
    }
    fn finish(self) -> Result<Document, String> {
        let mut doc = self.builder.finish()?;
        let (w, h) = (doc.width, doc.height);
        let mut model = (**doc.diagram.as_ref().unwrap()).clone();
        for s in self.shapes {
            let shape = &model.shapes[&s.id];
            if let NodeKind::Path { path, style, cache } =
                &mut doc.node_mut(shape.body).unwrap().kind
            {
                // Line-only symbols (for example the load balancer) must not
                // acquire an implicit triangular fill between open segments.
                style.fill = path.subpaths.iter().any(|p| p.closed).then_some(s.fill);
                style.stroke = (s.fill[3] != 0).then_some(INK);
                style.width = 1.;
                *cache = VectorRaster::path(path.clone(), *style, w, h);
            }
            if let NodeKind::Text { spec, cache } = &mut doc.node_mut(shape.label).unwrap().kind {
                let spec = Arc::make_mut(spec);
                spec.color = INK;
                spec.size = 15.;
                spec.line_height = 1.3;
                if shape.kind.is_container() {
                    spec.align = crate::text::Align::Left;
                    spec.x = (s.bounds[0] + 16.) as f32;
                }
                if !s.caption && !shape.kind.is_container() {
                    let lines = spec.text.lines().count().max(1) as f64;
                    spec.y = (s.bounds[1] + (s.bounds[3] - lines * 19.5) / 2.) as f32;
                }
                *cache = VectorRaster::text(Arc::new(spec.clone()), w, h);
            }
        }
        for e in self.edges {
            let edge = model.edges.get_mut(&e.id).unwrap();
            edge.arrow_end = e.arrow;
            edge.corner_radius = 8.;
            edge.waypoints = e.points;
        }
        for (child, parent) in self.parents {
            doc.node_mut(child).unwrap().parent = Some(parent);
            if model.shapes[&parent].kind.is_container() {
                model.shapes.get_mut(&child).unwrap().container = Some(parent);
            }
        }
        for spec in self.text {
            let id = doc.alloc_id();
            doc.nodes
                .push(Node::text(id, spec.text.clone(), spec, w, h));
        }
        doc.diagram = Some(Arc::new(model));
        doc.normalize();
        diagram::synchronize(&Document::new(w, h), &mut doc)?;
        doc.validate().map_err(|e| e.to_string())?;
        Ok(doc)
    }
}

pub(super) fn build(id: &str) -> Option<Result<Document, String>> {
    Some(match id {
        "business-process" => business(),
        "purchase-process" => purchase(),
        "family-tree" => family(),
        "fishbone" => fishbone(),
        "org-profiles" => profiles(),
        "branching-tree" => strategy(),
        "improvement-cycle" => cycle(),
        "infographic-flow" => roadmap(),
        "genogram" => relationships(),
        "cloud-architecture" => network(true),
        "network" => network(false),
        _ => return None,
    })
}

fn business() -> Result<Document, String> {
    let mut c = Composition::new(
        "Sales approval workflow",
        "Responsibilities and handoffs · Customer / Sales / Management / Credit",
        1400,
        860,
    )?;
    let mut lanes = vec![];
    for (i, name) in ["CUSTOMER", "SALES", "MANAGEMENT", "CREDIT"]
        .iter()
        .enumerate()
    {
        lanes.push(c.shape(
            "swimlane",
            [40., 130. + i as f64 * 170., 1320., 155.],
            name,
            COLORS[i],
        )?);
    }
    let mut nodes = vec![];
    for (lane, stencil, x, label) in [
        (0, "terminator", 95., "Request product"),
        (0, "document", 365., "Submit details"),
        (1, "process", 365., "Enter order"),
        (1, "document", 635., "Prepare quote"),
        (2, "decision", 620., "Approve?"),
        (2, "terminator", 955., "Revise quote"),
        (3, "process", 620., "Check credit"),
        (3, "terminator", 1080., "Release order"),
    ] {
        let node = c.shape(
            stencil,
            [x, 186. + lane as f64 * 170., 190., 76.],
            label,
            WHITE,
        )?;
        c.parents.push((node, lanes[lane]));
        nodes.push(node);
    }
    for (a, b, label) in [
        (0, 1, ""),
        (1, 2, ""),
        (2, 3, ""),
        (3, 4, ""),
        (4, 5, "No"),
        (4, 6, "Yes"),
        (6, 7, "Approved"),
    ] {
        c.link(nodes[a], nodes[b], label)?;
    }
    c.finish()
}
fn purchase() -> Result<Document, String> {
    let mut c = Composition::new(
        "Purchase approval",
        "From requisition to payment · Replace the steps with your procurement policy",
        1120,
        1130,
    )?;
    let mut n = vec![];
    for (stencil, b, label, color) in [
        (
            "terminator",
            [440., 140., 240., 65.],
            "Purchase request",
            TEAL,
        ),
        (
            "document",
            [440., 255., 240., 80.],
            "Complete requisition",
            BLUE,
        ),
        ("decision", [450., 395., 220., 110.], "Approved?", PURPLE),
        ("process", [75., 410., 240., 80.], "Revise request", ROSE),
        (
            "decision",
            [450., 565., 220., 110.],
            "Payment method?",
            PURPLE,
        ),
        (
            "process",
            [75., 745., 240., 80.],
            "Card authorization",
            BLUE,
        ),
        (
            "process",
            [805., 745., 240., 80.],
            "Issue purchase order",
            BLUE,
        ),
        (
            "document",
            [75., 895., 240., 80.],
            "Record card receipt",
            AMBER,
        ),
        (
            "document",
            [805., 895., 240., 80.],
            "Match supplier invoice",
            AMBER,
        ),
        (
            "terminator",
            [440., 1000., 240., 65.],
            "Approve and archive",
            TEAL,
        ),
    ] {
        n.push(c.shape(stencil, b, label, color)?);
    }
    for (a, b, label) in [
        (0, 1, ""),
        (1, 2, ""),
        (2, 3, "No"),
        (3, 1, "Resubmit"),
        (2, 4, "Yes"),
        (4, 5, "Card"),
        (4, 6, "Invoice"),
        (5, 7, ""),
        (6, 8, ""),
        (7, 9, ""),
        (8, 9, ""),
    ] {
        c.link(n[a], n[b], label)?;
    }
    c.finish()
}
fn family() -> Result<Document, String> {
    let mut c = Composition::new(
        "Our family tree",
        "Three generations · Edit names and dates to tell your family's story",
        1280,
        800,
    )?;
    let root = c.shape(
        "ellipse",
        [60., 345., 155., 110.],
        "Your name\nGeneration 1",
        TEAL,
    )?;
    for (i, y) in [225., 535.].iter().enumerate() {
        let parent = c.shape(
            "ellipse",
            [450., *y, 170., 100.],
            &format!("Parent {}\nGeneration 2", i + 1),
            BLUE,
        )?;
        c.edge(
            (root, Port::East),
            (parent, Port::West),
            "",
            Routing::Orthogonal,
            false,
        )?;
        for (j, dy) in [-90., 90.].iter().enumerate() {
            let grandparent = c.shape(
                "ellipse",
                [920., y + dy, 190., 100.],
                &format!("Grandparent {}\nGeneration 3", i * 2 + j + 1),
                PURPLE,
            )?;
            c.edge(
                (parent, Port::East),
                (grandparent, Port::West),
                "",
                Routing::Orthogonal,
                false,
            )?;
        }
    }
    c.finish()
}
fn fishbone() -> Result<Document, String> {
    let mut c = Composition::new(
        "Cause & effect",
        "Investigate the contributing factors · Replace each hypothesis with evidence",
        1360,
        760,
    )?;
    let origin = c.shape("process", [60., 388., 2., 2.], "", [0; 4])?;
    let effect = c.shape(
        "terminator",
        [1100., 344., 205., 90.],
        "Problem\nor outcome",
        BLUE,
    )?;
    let spine = c.edge(
        (origin, Port::East),
        (effect, Port::West),
        "",
        Routing::Straight,
        true,
    )?;
    for (i, (top, bottom)) in [
        ("People", "Methods"),
        ("Technology", "Environment"),
        ("Materials", "Measurement"),
    ]
    .iter()
    .enumerate()
    {
        let x = 55. + i as f64 * 325.;
        for (upper, name, color) in [(true, *top, TEAL), (false, *bottom, AMBER)] {
            let y = if upper { 155. } else { 610. };
            let category = c.shape("terminator", [x, y, 190., 46.], name, color)?;
            let rib = c.edge(
                (category, if upper { Port::South } else { Port::North }),
                (
                    spine,
                    Port::Custom {
                        x: 0.2 + i as f64 * 0.31,
                        y: 0.5,
                    },
                ),
                "",
                Routing::Straight,
                false,
            )?;
            for (j, text) in ["Possible cause", "Evidence"].iter().enumerate() {
                let cy = if upper {
                    252. + j as f64 * 65.
                } else {
                    520. - j as f64 * 65.
                };
                let cause = c.shape("process", [x - 15., cy, 128., 26.], text, [0; 4])?;
                c.edge(
                    (cause, Port::East),
                    (
                        rib,
                        Port::Custom {
                            x: 0.35 + j as f64 * 0.34,
                            y: 0.5,
                        },
                    ),
                    "",
                    Routing::Straight,
                    false,
                )?;
            }
        }
    }
    c.finish()
}
fn profile(
    c: &mut Composition,
    b: [f64; 4],
    name: &str,
    role: &str,
    color: Color,
) -> Result<NodeId, String> {
    let card = c.shape("process", b, "", color)?;
    let icon = c.shape("actor", [b[0] + 16., b[1] + 20., 36., 52.], "", WHITE)?;
    let label = c.shape(
        "process",
        [b[0] + 60., b[1] + 14., b[2] - 70., b[3] - 28.],
        &format!("{name}\n{role}"),
        [0; 4],
    )?;
    c.parents.extend([(icon, card), (label, card)]);
    Ok(card)
}
fn profiles() -> Result<Document, String> {
    let mut c = Composition::new(
        "Meet the team",
        "People, roles and reporting relationships · Replace the profile placeholders",
        1280,
        800,
    )?;
    let lead = profile(
        &mut c,
        [490., 150., 300., 100.],
        "Alex Morgan",
        "Team lead",
        TEAL,
    )?;
    for (i, (name, role, child)) in [
        ("Jordan Lee", "Engineering", "Platform team"),
        ("Sam Rivera", "Design", "Product team"),
        ("Taylor Chen", "Operations", "Delivery team"),
    ]
    .iter()
    .enumerate()
    {
        let x = 50. + i as f64 * 420.;
        let manager = profile(&mut c, [x, 355., 340., 100.], name, role, COLORS[i])?;
        let report = profile(&mut c, [x, 590., 340., 100.], "Add a name", child, WHITE)?;
        c.hierarchy(lead, manager)?;
        c.hierarchy(manager, report)?;
    }
    c.finish()
}
fn strategy() -> Result<Document, String> {
    let mut c = Composition::new(
        "Strategy at a glance",
        "One shared outcome · Five workstreams with actionable objectives",
        1400,
        800,
    )?;
    let root = c.shape(
        "terminator",
        [525., 145., 350., 75.],
        "Our shared objective",
        TEAL,
    )?;
    for (i, name) in ["People", "Customer", "Product", "Operations", "Growth"]
        .iter()
        .enumerate()
    {
        let x = 40. + i as f64 * 272.;
        let pillar = c.shape("process", [x, 340., 232., 75.], name, COLORS[i])?;
        c.hierarchy(root, pillar)?;
        for j in 0..3 {
            let task = c.shape(
                "terminator",
                [x + 25., 480. + j as f64 * 85., 207., 50.],
                &format!("Objective {}.{}", i + 1, j + 1),
                WHITE,
            )?;
            // A left-hand rail keeps connectors out of the objective labels.
            c.edge(
                (pillar, Port::West),
                (task, Port::West),
                "",
                Routing::Orthogonal,
                false,
            )?;
        }
    }
    c.finish()
}
fn cycle() -> Result<Document, String> {
    let mut c = Composition::new(
        "Continuous improvement",
        "A repeatable three-step loop · Discover, deliver, learn",
        1200,
        820,
    )?;
    let a = c.shape(
        "ellipse",
        [490., 155., 220., 170.],
        "01\nDiscover\nUnderstand the need",
        TEAL,
    )?;
    let b = c.shape(
        "ellipse",
        [830., 500., 220., 170.],
        "02\nDeliver\nTest a small change",
        BLUE,
    )?;
    let d = c.shape(
        "ellipse",
        [150., 500., 220., 170.],
        "03\nLearn\nMeasure and improve",
        PURPLE,
    )?;
    c.edge((a, Port::East), (b, Port::North), "", Routing::Curved, true)?;
    c.edges.last_mut().unwrap().points = vec![(935., 245.)];
    c.edge(
        (b, Port::South),
        (d, Port::South),
        "",
        Routing::Curved,
        true,
    )?;
    c.edges.last_mut().unwrap().points = vec![(600., 790.)];
    c.edge((d, Port::North), (a, Port::West), "", Routing::Curved, true)?;
    c.edges.last_mut().unwrap().points = vec![(260., 245.)];
    c.shape(
        "process",
        [475., 440., 250., 85.],
        "Small steps.\nBetter outcomes.",
        [0; 4],
    )?;
    c.finish()
}
fn roadmap() -> Result<Document, String> {
    let mut c = Composition::new(
        "From idea to launch",
        "A branching project roadmap · Three parallel workstreams, one release",
        1200,
        820,
    )?;
    let root = c.shape(
        "terminator",
        [460., 140., 280., 65.],
        "Discover the opportunity",
        PURPLE,
    )?;
    let end = c.shape(
        "terminator",
        [460., 695., 280., 65.],
        "Launch & measure",
        TEAL,
    )?;
    for (i, (a, b)) in [
        ("Research", "Validate needs"),
        ("Design", "Test a prototype"),
        ("Engineering", "Build & verify"),
    ]
    .iter()
    .enumerate()
    {
        let x = 65. + i as f64 * 405.;
        let one = c.shape("terminator", [x, 330., 260., 65.], a, COLORS[i])?;
        let two = c.shape("terminator", [x, 510., 260., 65.], b, COLORS[i])?;
        c.hierarchy(root, one)?;
        c.link(one, two, "")?;
        c.edge(
            (two, Port::South),
            (end, Port::North),
            "",
            Routing::Orthogonal,
            true,
        )?;
    }
    c.finish()
}
fn relationships() -> Result<Document, String> {
    let mut c = Composition::new(
        "Family relationship map",
        "A customizable genogram · Symbols and colors are descriptive placeholders",
        1200,
        800,
    )?;
    let a = c.shape("process", [390., 165., 90., 90.], "A", BLUE)?;
    let b = c.shape("ellipse", [720., 165., 90., 90.], "B", TEAL)?;
    let union = c.edge(
        (a, Port::East),
        (b, Port::West),
        "",
        Routing::Straight,
        false,
    )?;
    for (i, (stencil, label, color)) in [
        ("ellipse", "C", TEAL),
        ("process", "D", BLUE),
        ("ellipse", "E", ROSE),
    ]
    .iter()
    .enumerate()
    {
        let x = 150. + i as f64 * 380.;
        let child = c.shape(stencil, [x, 410., 90., 90.], label, *color)?;
        c.edge(
            (union, Port::Custom { x: 0.5, y: 0.5 }),
            (child, Port::North),
            "",
            Routing::Orthogonal,
            false,
        )?;
        let desc = c.shape(
            "ellipse",
            [x, 590., 90., 90.],
            &format!("{}1", label),
            WHITE,
        )?;
        c.hierarchy(child, desc)?;
    }
    c.text(
        "LEGEND   Blue: group A     Teal: group B     Rose: group C     Lines: relationships",
        [80., 735., 1040., 28.],
        14.,
        false,
    );
    c.finish()
}
fn network(cloud: bool) -> Result<Document, String> {
    let mut c = Composition::new(
        if cloud {
            "Cloud application architecture"
        } else {
            "Application network"
        },
        "Reusable infrastructure stencils · Select a component to edit its label or connections",
        1360,
        820,
    )?;
    let client = c.shape("workstation", [70., 350., 120., 95.], "Client", BLUE)?;
    let firewall = c.shape("firewall", [310., 350., 100., 95.], "Firewall", AMBER)?;
    let gateway = c.shape(
        if cloud { "load-balancer" } else { "router" },
        [540., 350., 140., 95.],
        if cloud { "Load balancer" } else { "Router" },
        TEAL,
    )?;
    let service = c.shape(
        if cloud { "container-service" } else { "server" },
        [865., 170., 110., 115.],
        "Application",
        PURPLE,
    )?;
    let database = c.shape("database", [1110., 165., 170., 110.], "Database", BLUE)?;
    let worker = c.shape("server", [865., 550., 110., 115.], "Worker", TEAL)?;
    let storage = c.shape(
        "object-store",
        [1125., 560., 140., 100.],
        "Object storage",
        AMBER,
    )?;
    for (a, b, label) in [
        (client, firewall, "HTTPS"),
        (firewall, gateway, ""),
        (gateway, service, "API"),
        (service, database, "Query"),
        (worker, storage, "Write"),
    ] {
        c.link(a, b, label)?;
    }
    if cloud {
        let queue = c.shape("queue", [850., 380., 140., 80.], "Queue", ROSE)?;
        c.edge(
            (service, Port::West),
            (queue, Port::West),
            "",
            Routing::Orthogonal,
            true,
        )?;
        c.edge(
            (queue, Port::West),
            (worker, Port::West),
            "",
            Routing::Orthogonal,
            true,
        )?;
    } else {
        c.link(gateway, worker, "")?;
    }
    c.finish()
}
