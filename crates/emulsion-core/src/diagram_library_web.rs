//! Original, editable web-system reference boards. Shared by the gallery and MCP.
use crate::{
    Document, Node, NodeId, NodeKind,
    diagram::{self, Builder, Endpoint, Port, Routing},
    text::{Align, TextSpec},
    vector_cache::VectorRaster,
};
use emulsion_raster::vector::{PathPaint, PathStyle};
use std::sync::Arc;
type Color = [u8; 4];
#[derive(Clone, Copy)]
struct Tone {
    fill: Color,
    line: Color,
    ink: Color,
}
const PURPLE: Tone = Tone {
    fill: [243, 238, 255, 255],
    line: [124, 58, 237, 255],
    ink: [76, 29, 149, 255],
};
const BLUE: Tone = Tone {
    fill: [235, 243, 255, 255],
    line: [37, 99, 235, 255],
    ink: [30, 58, 138, 255],
};
const CYAN: Tone = Tone {
    fill: [230, 249, 252, 255],
    line: [8, 145, 178, 255],
    ink: [21, 94, 117, 255],
};
const TEAL: Tone = Tone {
    fill: [229, 248, 240, 255],
    line: [5, 150, 105, 255],
    ink: [6, 78, 59, 255],
};
const AMBER: Tone = Tone {
    fill: [255, 247, 224, 255],
    line: [217, 119, 6, 255],
    ink: [120, 53, 15, 255],
};
const ROSE: Tone = Tone {
    fill: [255, 237, 242, 255],
    line: [225, 29, 72, 255],
    ink: [136, 19, 55, 255],
};
const SLATE: Tone = Tone {
    fill: [237, 241, 248, 255],
    line: [100, 116, 139, 255],
    ink: [30, 41, 59, 255],
};
const NAVY: Color = [15, 23, 42, 255];
const MUTED: Color = [71, 85, 105, 255];
const WHITE: Color = [255; 4];
const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1240;
const X: [f64; 5] = [72., 444., 816., 1188., 1560.];
const Y: [f64; 3] = [322., 580., 838.];
#[derive(Clone, Copy)]
struct Flow {
    tone: Tone,
    dashed: bool,
}
const REQUEST: Flow = Flow {
    tone: BLUE,
    dashed: false,
};
const RETURN: Flow = Flow {
    tone: TEAL,
    dashed: true,
};
const EVENT: Flow = Flow {
    tone: AMBER,
    dashed: true,
};
const ERROR: Flow = Flow {
    tone: ROSE,
    dashed: true,
};
struct Card {
    id: NodeId,
    bounds: [f64; 4],
    tone: Tone,
    lane: bool,
    parent: Option<NodeId>,
}
struct Link {
    id: NodeId,
    flow: Flow,
    points: Vec<(f64, f64)>,
}
struct Board {
    builder: Builder,
    cards: Vec<Card>,
    icons: Vec<(NodeId, NodeId, Tone)>,
    links: Vec<Link>,
    text: Vec<TextSpec>,
}
impl Board {
    fn new(title: &str, subtitle: &str, tag: &str) -> Result<Self, String> {
        let mut b = Self {
            builder: Builder::new(WIDTH, HEIGHT)?,
            cards: vec![],
            icons: vec![],
            links: vec![],
            text: vec![],
        };
        b.text(
            "EMULSION  /  WEB SYSTEMS",
            [48., 28., 1400.],
            12.,
            true,
            [165, 180, 252, 255],
        );
        b.text(title, [48., 58., 1750.], 38., true, WHITE);
        b.text(
            subtitle,
            [48., 117., 1760.],
            16.,
            false,
            [203, 213, 225, 255],
        );
        for (i, (tone, label)) in [
            (PURPLE, "Browser / client"),
            (BLUE, "Requests / API"),
            (CYAN, "Edge / network"),
            (TEAL, "Success / response"),
            (AMBER, "Cache / queue / async"),
            (ROSE, "Errors / retry"),
            (SLATE, "Data / storage"),
        ]
        .iter()
        .enumerate()
        {
            b.text(
                label,
                [70. + i as f64 * 260., 207., 238.],
                13.,
                true,
                tone.ink,
            );
        }
        b.text(&format!("{tag}   •   Solid = request / action   •   Dashed = response / event / exception   •   All objects are editable"),[48.,1200.,1824.],12.,false,MUTED);
        Ok(b)
    }
    fn text(&mut self, text: &str, [x, y, w]: [f64; 3], size: f32, bold: bool, color: Color) {
        self.text.push(TextSpec {
            text: text.into(),
            font: "Geist".into(),
            x: x as f32,
            y: y as f32,
            width: Some(w as f32),
            size,
            bold,
            color,
            line_height: 1.35,
            ..Default::default()
        });
    }
    fn shape(
        &mut self,
        stencil: &str,
        bounds: [f64; 4],
        label: &str,
        tone: Tone,
        parent: Option<NodeId>,
    ) -> Result<NodeId, String> {
        let stencil = *diagram::stencils::STENCILS
            .iter()
            .find(|s| s.id == stencil)
            .ok_or("Missing web template stencil")?;
        let id = self.builder.add_stencil(stencil, bounds, label)?;
        self.cards.push(Card {
            id,
            bounds,
            tone,
            lane: stencil.kind.is_container(),
            parent,
        });
        if !stencil.kind.is_container() && bounds[2] == 288. {
            let text = label
                .lines()
                .take(2)
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase();
            let icon = if text.contains("database")
                || text.contains("cache")
                || text.contains("commit")
                || text.contains("storage")
                || text.contains("persist")
                || text.contains("store")
            {
                "database"
            } else if text.contains("queue") || text.contains("outbox") || text.contains("pub/sub")
            {
                "queue"
            } else if text.contains("browser")
                || text.contains("client")
                || text.contains("customer")
                || text.contains("interface")
            {
                "browser"
            } else if text.contains("identity")
                || text.contains("protect")
                || text.contains("auth")
                || text.contains("denied")
                || text.contains("sign-in")
            {
                "firewall"
            } else if text.contains("edge")
                || text.contains("gateway")
                || text.contains("connection")
                || text.contains("dns")
            {
                "cloud"
            } else if text.contains("worker")
                || text.contains("server")
                || text.contains("application")
            {
                "server"
            } else {
                "document"
            };
            let stencil = *diagram::stencils::STENCILS
                .iter()
                .find(|s| s.id == icon)
                .ok_or("Missing web template icon")?;
            let icon = self.builder.add_stencil(
                stencil,
                [bounds[0] + bounds[2] - 48., bounds[1] + 13., 28., 28.],
                "",
            )?;
            self.icons.push((icon, id, tone));
        }
        Ok(id)
    }
    fn lane(&mut self, row: usize, label: &str, tone: Tone) -> Result<NodeId, String> {
        self.shape(
            "container",
            [48., Y[row] - 56., 1824., 224.],
            label,
            tone,
            None,
        )
    }
    fn card(
        &mut self,
        col: usize,
        row: usize,
        label: &str,
        tone: Tone,
        parent: NodeId,
    ) -> Result<NodeId, String> {
        self.shape(
            "process",
            [X[col], Y[row], 288., 132.],
            label,
            tone,
            Some(parent),
        )
    }
    fn link(
        &mut self,
        ends: [(NodeId, Port); 2],
        label: &str,
        flow: Flow,
        points: &[(f64, f64)],
    ) -> Result<NodeId, String> {
        let id = self.builder.connect(
            Endpoint {
                shape: ends[0].0,
                port: ends[0].1,
            },
            Endpoint {
                shape: ends[1].0,
                port: ends[1].1,
            },
            label,
            Routing::Orthogonal,
        )?;
        self.links.push(Link {
            id,
            flow,
            points: points.to_vec(),
        });
        Ok(id)
    }
    fn notes(&mut self, notes: [(&str, Tone); 3]) -> Result<(), String> {
        for (i, (label, tone)) in notes.iter().enumerate() {
            self.shape(
                "process",
                [48. + i as f64 * 616., 1042., 592., 126.],
                label,
                *tone,
                None,
            )?;
        }
        Ok(())
    }
    fn finish(self) -> Result<Document, String> {
        let mut doc = self.builder.finish()?;
        for node in &mut doc.nodes {
            if let NodeKind::Fill { rgba } = &mut node.kind {
                *rgba = [248, 250, 253, 255];
            }
        }
        let mut graph = (**doc.diagram.as_ref().unwrap()).clone();
        for c in self.cards {
            let shape = graph.shapes.get_mut(&c.id).unwrap();
            if let Some(parent) = c.parent {
                doc.node_mut(c.id).unwrap().parent = Some(parent);
                shape.container = Some(parent);
            }
            if let NodeKind::Path { path, style, cache } =
                &mut doc.node_mut(shape.body).unwrap().kind
            {
                style.fill = Some(if c.lane { WHITE } else { c.tone.fill });
                style.stroke = Some(if c.lane {
                    [215, 223, 234, 255]
                } else {
                    c.tone.line
                });
                style.width = if c.lane { 1. } else { 1.3 };
                *cache = VectorRaster::path(path.clone(), *style, WIDTH, HEIGHT);
            }
            if let NodeKind::Text { spec, cache } = &mut doc.node_mut(shape.label).unwrap().kind {
                let text = Arc::make_mut(spec);
                text.font = "Geist".into();
                text.size = 14.;
                text.line_height = 1.4;
                text.bold = false;
                text.color = MUTED;
                text.align = Align::Left;
                text.x = (c.bounds[0] + 22.) as f32;
                text.y = (c.bounds[1] + 16.) as f32;
                text.width = Some((c.bounds[2] - 44.) as f32);
                let first = text.text.find('\n').unwrap_or(text.text.len());
                text.apply_style(0..first, |s| {
                    s.size = 12.;
                    s.bold = true;
                    s.color = c.tone.ink;
                    s.letter_spacing = 0.5;
                });
                if !c.lane && first < text.text.len() {
                    let end = text.text[first + 1..]
                        .find('\n')
                        .map_or(text.text.len(), |p| first + 1 + p);
                    text.apply_style(first + 1..end, |s| {
                        s.size = 19.;
                        s.bold = true;
                        s.color = c.tone.ink;
                    });
                }
                *cache = VectorRaster::text(spec.clone(), WIDTH, HEIGHT);
            }
            if !c.lane {
                rect(
                    &mut doc,
                    Some(c.id),
                    [c.bounds[0] + 1., c.bounds[1] + 18., 4., c.bounds[3] - 36.],
                    c.tone.line,
                    None,
                );
            }
        }
        for (icon, parent, tone) in self.icons {
            doc.node_mut(icon).unwrap().parent = Some(parent);
            for id in doc.subtree(icon) {
                if let NodeKind::Path { path, style, cache } = &mut doc.node_mut(id).unwrap().kind {
                    if style.fill.is_some_and(|c| c[3] > 0) {
                        style.fill = Some(tone.line);
                    }
                    if style.stroke.is_some() {
                        style.stroke = Some(tone.ink);
                        style.width = 1.;
                    }
                    *cache = VectorRaster::path(path.clone(), *style, WIDTH, HEIGHT);
                }
            }
        }
        for link in self.links {
            let edge = graph.edges.get_mut(&link.id).unwrap();
            edge.corner_radius = 10.;
            if !link.points.is_empty() {
                edge.jump_style = diagram::JumpStyle::Arc;
                edge.jump_size = 5.;
            }
            edge.waypoints = link.points;
            edge.label_normal = 14.;
            edge.label_background = Some(WHITE);
            if let NodeKind::Path { path, style, cache } =
                &mut doc.node_mut(edge.path).unwrap().kind
            {
                style.stroke = Some(link.flow.tone.line);
                style.width = 2.;
                if link.flow.dashed {
                    style.dash = [8., 5., 0., 0., 0., 0.];
                    style.dash_count = 2;
                }
                *cache = VectorRaster::path(path.clone(), *style, WIDTH, HEIGHT);
            }
            if let NodeKind::Text { spec, cache } = &mut doc.node_mut(edge.label).unwrap().kind {
                let label = Arc::make_mut(spec);
                label.font = "Geist".into();
                label.size = 12.;
                label.bold = true;
                label.color = link.flow.tone.ink;
                label.width = Some((label.text.chars().count() as f32 * 7. + 18.).clamp(56., 180.));
                edge.label_offset = (60. - f64::from(label.width.unwrap()) / 2., 14.);
                *cache = VectorRaster::text(spec.clone(), WIDTH, HEIGHT);
            }
        }
        rect(
            &mut doc,
            None,
            [0., 0., WIDTH as f64, 176.],
            NAVY,
            Some([40, 47, 91, 255]),
        );
        for (i, tone) in [PURPLE, BLUE, CYAN, TEAL, AMBER, ROSE, SLATE]
            .iter()
            .enumerate()
        {
            rect(
                &mut doc,
                None,
                [i as f64 * WIDTH as f64 / 7., 172., WIDTH as f64 / 7., 4.],
                tone.line,
                None,
            );
            rect(
                &mut doc,
                None,
                [48. + i as f64 * 260., 210., 12., 12.],
                tone.line,
                None,
            );
        }
        for spec in self.text {
            let id = doc.alloc_id();
            doc.nodes
                .push(Node::text(id, spec.text.clone(), spec, WIDTH, HEIGHT));
        }
        doc.diagram = Some(Arc::new(graph));
        doc.normalize();
        diagram::synchronize(&Document::new(WIDTH, HEIGHT), &mut doc)?;
        doc.validate().map_err(|e| e.to_string())?;
        Ok(doc)
    }
}
fn rect(
    doc: &mut Document,
    parent: Option<NodeId>,
    bounds: [f64; 4],
    color: Color,
    end: Option<Color>,
) {
    let id = doc.alloc_id();
    let mut node = Node::path(
        id,
        "Color accent",
        Arc::new(diagram::ShapeKind::Process.path(bounds)),
        PathStyle {
            fill: Some(color),
            stroke: None,
            fill_paint: end.map_or(PathPaint::Solid, |end| PathPaint::LinearGradient {
                end,
                angle: 0.,
            }),
            ..Default::default()
        },
        WIDTH,
        HEIGHT,
    );
    node.parent = parent;
    doc.nodes.push(node);
}

pub(super) fn build(id: &str) -> Option<Result<Document, String>> {
    Some(match id {
        "web-page-journey" => page_journey(),
        "web-api-platform" => api_platform(),
        "web-async-checkout" => checkout(),
        "web-realtime-updates" => realtime(),
        _ => return None,
    })
}

fn page_journey() -> Result<Document, String> {
    use Port::*;
    let mut b = Board::new(
        "From address bar to first paint",
        "A complete browser-to-server journey, with local-cache and CDN shortcuts.",
        "01 / PAGE DELIVERY",
    )?;
    let client = b.lane(0, "01  /  NAVIGATION & DELIVERY", PURPLE)?;
    let origin = b.lane(1, "02  /  ORIGIN REQUEST PROCESSING", BLUE)?;
    let render = b.lane(2, "03  /  BROWSER RENDERING", TEAL)?;
    let labels = [
        (
            0,
            0,
            "01  /  CLIENT\nOpen a URL\nNavigation starts in the browser.\nApply origin and cookie rules.",
            PURPLE,
            client,
        ),
        (
            1,
            0,
            "02  /  LOCAL CACHE\nReusable response?\nFresh: use the stored document.\nStale or missing: use the network.",
            AMBER,
            client,
        ),
        (
            2,
            0,
            "03  /  DISCOVERY\nResolve the hostname\nCheck cached DNS information.\nOtherwise ask the resolver.",
            CYAN,
            client,
        ),
        (
            3,
            0,
            "04  /  CONNECTION\nEstablish HTTPS\nReuse or negotiate a connection.\nValidate the server certificate.",
            CYAN,
            client,
        ),
        (
            4,
            0,
            "05  /  EDGE\nCheck the CDN cache\nServe a reusable response.\nForward a miss to the origin.",
            AMBER,
            client,
        ),
        (
            4,
            1,
            "06  /  PROTECTION\nOrigin gateway\nCheck policy and rate limits.\nReject invalid requests early.",
            ROSE,
            origin,
        ),
        (
            3,
            1,
            "07  /  DISTRIBUTION\nLoad balancer\nChoose a healthy application.\nPropagate a request identifier.",
            CYAN,
            origin,
        ),
        (
            2,
            1,
            "08  /  APPLICATION\nRun server logic\nAuthorize the operation.\nRead the required records.",
            BLUE,
            origin,
        ),
        (
            1,
            1,
            "09  /  STORAGE\nQuery the database\nUse a bounded connection pool.\nReturn the requested records.",
            SLATE,
            origin,
        ),
        (
            0,
            1,
            "10  /  RESPONSE\nSend the document\nRender HTML and set headers.\nReturn status, body and cache policy.",
            TEAL,
            origin,
        ),
        (
            0,
            2,
            "11  /  DOCUMENT\nParse the HTML\nBuild the DOM incrementally.\nDiscover styles, scripts and images.",
            PURPLE,
            render,
        ),
        (
            1,
            2,
            "12  /  RESOURCES\nFetch subresources\nUse the browser and edge caches.\nBuild the CSS object model.",
            CYAN,
            render,
        ),
        (
            2,
            2,
            "13  /  APPLICATION UI\nExecute JavaScript\nInitialize or hydrate the interface.\nFetch additional data as needed.",
            BLUE,
            render,
        ),
        (
            3,
            2,
            "14  /  RENDERING\nStyle and layout\nCalculate styles and geometry.\nPrepare paint instructions.",
            AMBER,
            render,
        ),
        (
            4,
            2,
            "15  /  PRESENTATION\nPaint and composite\nDisplay the page to the user.\nHandle input and later updates.",
            TEAL,
            render,
        ),
    ];
    let mut n = vec![];
    for (col, row, label, tone, lane) in labels {
        n.push(b.card(col, row, label, tone, lane)?);
    }
    for i in 0..14 {
        let ports = if i == 4 || i == 9 {
            (South, North)
        } else if (5..9).contains(&i) {
            (West, East)
        } else {
            (East, West)
        };
        b.link(
            [(n[i], ports.0), (n[i + 1], ports.1)],
            if i == 4 { "Miss" } else { "" },
            if i == 9 {
                RETURN
            } else if i > 9 {
                Flow {
                    tone: TEAL,
                    dashed: false,
                }
            } else {
                REQUEST
            },
            &[],
        )?;
    }
    b.link(
        [(n[1], South), (n[10], West)],
        "Fresh locally",
        RETURN,
        &[(588., 506.), (28., 506.), (28., 904.)],
    )?;
    b.link(
        [(n[4], East), (n[10], North)],
        "CDN hit",
        RETURN,
        &[(1892., 388.), (1892., 770.), (216., 770.)],
    )?;
    b.notes([
        ("READ THE STORY\nFollow 01 through 15\nSolid arrows show the normal network path.\nDashed green branches skip unnecessary origin work.",BLUE),
        ("MODEL A VARIANT\nMake the scenario your own\nRename the application and add your real hostnames.\nAdd an API call from JavaScript for client-rendered pages.",PURPLE),
        ("DESIGN NOTE\nA conceptual workflow\nResource loading and script execution can overlap.\nThis board explains dependencies, not exact timing.",AMBER),
    ])?;
    b.finish()
}

fn api_platform() -> Result<Document, String> {
    use Port::*;
    let mut b = Board::new(
        "An authenticated API, end to end",
        "Browser, gateway, identity, application services and storage — including failure and background-work paths.",
        "02 / SERVICE ARCHITECTURE",
    )?;
    let mut zones = vec![];
    for (i, (label, tone)) in [
        ("CLIENT", PURPLE),
        ("EDGE", CYAN),
        ("SECURITY", ROSE),
        ("SERVICES", BLUE),
        ("DATA", SLATE),
    ]
    .iter()
    .enumerate()
    {
        zones.push(b.shape(
            "container",
            [X[i] - 24., 266., 336., 740.],
            label,
            *tone,
            None,
        )?);
    }
    let specs = [
        (
            0,
            0,
            "01  /  BROWSER\nWeb application\nSend credentials with the request.\nTrack loading, success and error.",
            PURPLE,
        ),
        (
            1,
            0,
            "02  /  ENTRY POINT\nAPI gateway\nTerminate TLS and route requests.\nApply quotas and request limits.",
            CYAN,
        ),
        (
            2,
            0,
            "03  /  IDENTITY\nValidate the session\nVerify identity and token claims.\nPass a trusted principal onward.",
            ROSE,
        ),
        (
            3,
            0,
            "04  /  BUSINESS LOGIC\nApplication service\nAuthorize the specific resource.\nValidate input and run the operation.",
            BLUE,
        ),
        (
            4,
            0,
            "05  /  SOURCE OF TRUTH\nRelational database\nUse transactions and constraints.\nStore authoritative application data.",
            SLATE,
        ),
        (
            0,
            1,
            "SESSION RECOVERY\nSign-in flow\nAuthenticate with the identity service.\nRetry after obtaining a valid session.",
            PURPLE,
        ),
        (
            1,
            1,
            "STATIC CONTENT\nCDN / asset delivery\nServe versioned JavaScript and CSS.\nKeep static and API cache rules apart.",
            CYAN,
        ),
        (
            2,
            1,
            "REJECTED REQUEST\n401 / 403 response\nReturn a structured error.\nDo not expose protected records.",
            ROSE,
        ),
        (
            3,
            1,
            "LOW-LATENCY READS\nApplication cache\nScope keys to the correct tenant.\nInvalidate or expire cached values.",
            AMBER,
        ),
        (
            4,
            1,
            "LARGE OBJECTS\nObject storage\nKeep uploads outside the database.\nValidate content and access rights.",
            SLATE,
        ),
        (
            0,
            2,
            "RETURN PATH\nUpdate the interface\nDecode the JSON response.\nRender a success or error state.",
            TEAL,
        ),
        (
            1,
            2,
            "OBSERVABILITY\nTrace every boundary\nCorrelate logs, spans and metrics.\nExclude secrets from telemetry.",
            CYAN,
        ),
        (
            2,
            2,
            "BACKPRESSURE\nHandle 429 / 503\nHonor Retry-After when supplied.\nRetry only safe operations.",
            ROSE,
        ),
        (
            3,
            2,
            "BACKGROUND SERVICE\nWorker pool\nPerform slow, retryable operations.\nReport progress separately.",
            BLUE,
        ),
        (
            4,
            2,
            "ASYNC TRANSPORT\nDurable job queue\nBuffer work and delivery attempts.\nAcknowledge after successful work.",
            AMBER,
        ),
    ];
    let mut n = vec![];
    for (col, row, label, tone) in specs {
        n.push(b.card(col, row, label, tone, zones[col])?);
    }
    for i in 0..4 {
        b.link(
            [(n[i], East), (n[i + 1], West)],
            if i == 0 { "HTTPS" } else { "" },
            REQUEST,
            &[],
        )?;
    }
    b.link([(n[5], North), (n[0], South)], "Session", RETURN, &[])?;
    b.link([(n[1], South), (n[6], North)], "Assets", RETURN, &[])?;
    b.link([(n[2], South), (n[7], North)], "Invalid", ERROR, &[])?;
    b.link(
        [(n[7], West), (n[5], East)],
        "Sign in",
        ERROR,
        &[(774., 646.), (774., 516.), (402., 516.), (402., 646.)],
    )?;
    b.link([(n[3], South), (n[8], North)], "Lookup", REQUEST, &[])?;
    b.link(
        [(n[3], East), (n[14], East)],
        "Enqueue",
        EVENT,
        &[(1518., 388.), (1518., 512.), (1892., 512.), (1892., 904.)],
    )?;
    b.link([(n[14], West), (n[13], East)], "Consume", EVENT, &[])?;
    b.link(
        [(n[13], East), (n[9], West)],
        "Write",
        REQUEST,
        &[(1518., 904.), (1518., 646.)],
    )?;
    b.link(
        [(n[3], Custom { x: 0., y: 0.8 }), (n[10], East)],
        "JSON response",
        RETURN,
        &[(1146., 427.6), (1146., 770.), (402., 770.), (402., 904.)],
    )?;
    b.link(
        [(n[1], Custom { x: 0., y: 0.8 }), (n[11], North)],
        "Trace",
        Flow {
            tone: CYAN,
            dashed: true,
        },
        &[(402., 427.6), (402., 758.), (588., 758.)],
    )?;
    b.link(
        [(n[1], Custom { x: 1., y: 0.8 }), (n[12], North)],
        "Throttled",
        ERROR,
        &[(756., 427.6), (756., 802.), (960., 802.)],
    )?;
    b.notes([
        ("SECURITY BOUNDARY\nAuthentication is not authorization\nA valid identity does not grant access to every record.\nResource and tenant checks belong in the application.",ROSE),
        ("RELIABILITY BOUNDARY\nKeep expensive work asynchronous\nReturn a job identifier for long-running operations.\nExpose completion through polling or push events.",AMBER),
        ("READ THIS BOARD\nAn architecture, not a timing chart\nConnectors show responsibilities and dependencies.\nAdapt the cache, queue and storage to your system.",BLUE),
    ])?;
    b.finish()
}

fn checkout() -> Result<Document, String> {
    use Port::*;
    let mut b = Board::new(
        "Checkout without duplicate charges",
        "An asynchronous order workflow with a transactional outbox, idempotency, bounded retries and recovery.",
        "03 / ASYNCHRONOUS WORKFLOW",
    )?;
    let a = b.lane(0, "CLIENT & ORDER API  /  ACCEPT THE REQUEST", PURPLE)?;
    let w = b.lane(1, "MESSAGING & WORKERS  /  COMPLETE THE WORK", AMBER)?;
    let r = b.lane(2, "CONFIRMATION & RECOVERY  /  MAKE OUTCOMES VISIBLE", TEAL)?;
    let specs = [
        (
            0,
            0,
            "01  /  CUSTOMER\nSubmit checkout\nSend the cart and shipping details.\nInclude an idempotency key.",
            PURPLE,
            a,
        ),
        (
            1,
            0,
            "02  /  ORDER API\nValidate the request\nAuthenticate and check the cart.\nReject invalid input before writing.",
            BLUE,
            a,
        ),
        (
            2,
            0,
            "03  /  DEDUPLICATION\nCheck the request key\nReuse the result of a prior request.\nDo not create the same order twice.",
            CYAN,
            a,
        ),
        (
            3,
            0,
            "04  /  ATOMIC COMMIT\nOrder + outbox record\nWrite both in one transaction.\nCommit before acknowledging.",
            SLATE,
            a,
        ),
        (
            4,
            0,
            "05  /  ACCEPTANCE\nReturn 202 + order ID\nThe request has been accepted.\nPayment is not yet confirmed.",
            TEAL,
            a,
        ),
        (
            4,
            1,
            "06  /  OUTBOX RELAY\nPublish pending events\nRead committed outbox entries.\nTolerate duplicate publication.",
            AMBER,
            w,
        ),
        (
            3,
            1,
            "07  /  DELIVERY\nDurable queue\nBuffer OrderSubmitted messages.\nRedeliver unacknowledged work.",
            AMBER,
            w,
        ),
        (
            2,
            1,
            "08  /  PROCESSING\nPayment worker\nDeduplicate the incoming event.\nUse a stable provider request key.",
            BLUE,
            w,
        ),
        (
            1,
            1,
            "09  /  PAYMENT\nPayment provider\nAuthorize or charge the payment.\nReturn a definitive or pending result.",
            CYAN,
            w,
        ),
        (
            0,
            1,
            "10  /  PERSISTENCE\nRecord the outcome\nUpdate the order state durably.\nAcknowledge completed queue work.",
            SLATE,
            w,
        ),
        (
            0,
            2,
            "11  /  CUSTOMER UI\nShow order status\nPoll or subscribe with the order ID.\nDistinguish pending from confirmed.",
            TEAL,
            r,
        ),
        (
            1,
            2,
            "12  /  NOTIFICATIONS\nSend confirmation\nConsume the committed outcome.\nDeduplicate email or push delivery.",
            PURPLE,
            r,
        ),
        (
            2,
            2,
            "RETRYABLE FAILURE\nBackoff + jitter\nRetry transient failures only.\nKeep the original idempotency key.",
            AMBER,
            r,
        ),
        (
            3,
            2,
            "RETRY LIMIT REACHED\nDead-letter queue\nPreserve the payload and failure.\nStop automatic retry loops.",
            ROSE,
            r,
        ),
        (
            4,
            2,
            "OPERATOR WORKFLOW\nInvestigate and replay\nInspect the cause before replaying.\nReconcile any uncertain payment.",
            ROSE,
            r,
        ),
    ];
    let mut n = vec![];
    for (col, row, label, tone, lane) in specs {
        n.push(b.card(col, row, label, tone, lane)?);
    }
    for i in 0..4 {
        b.link([(n[i], East), (n[i + 1], West)], "", REQUEST, &[])?;
    }
    b.link(
        [(n[3], South), (n[5], North)],
        "Committed",
        EVENT,
        &[(1332., 514.), (1704., 514.)],
    )?;
    b.link(
        [(n[4], North), (n[0], North)],
        "Accepted, not paid",
        RETURN,
        &[(1704., 246.), (216., 246.)],
    )?;
    for i in 5..9 {
        b.link(
            [(n[i], West), (n[i + 1], East)],
            "",
            if i < 7 { EVENT } else { REQUEST },
            &[],
        )?;
    }
    b.link([(n[9], South), (n[10], North)], "Status", RETURN, &[])?;
    b.link(
        [(n[9], Custom { x: 0.8, y: 1. }), (n[11], North)],
        "Outcome",
        EVENT,
        &[(302.4, 780.), (588., 780.)],
    )?;
    b.link(
        [
            (n[7], Custom { x: 0.75, y: 1. }),
            (n[12], Custom { x: 0.75, y: 0. }),
        ],
        "Transient",
        ERROR,
        &[],
    )?;
    b.link(
        [
            (n[12], Custom { x: 0.25, y: 0. }),
            (n[7], Custom { x: 0.25, y: 1. }),
        ],
        "Retry",
        EVENT,
        &[],
    )?;
    b.link([(n[12], East), (n[13], West)], "Exhausted", ERROR, &[])?;
    b.link([(n[13], East), (n[14], West)], "Review", ERROR, &[])?;
    b.notes([
        ("DELIVERY SEMANTICS\nDesign for at-least-once delivery\nA queue may deliver a message more than once.\nUse durable deduplication at every side-effect boundary.",AMBER),
        ("BUSINESS OUTCOME\nA decline is not a transient failure\nShow a declined payment as a business result.\nDo not retry it as if the provider were unavailable.",ROSE),
        ("CONSISTENCY\nTrack the full order lifecycle\nPending → processing → confirmed or failed.\nUse provider callbacks or reconciliation for pending results.",TEAL),
    ])?;
    b.finish()
}

fn realtime() -> Result<Document, String> {
    use Port::*;
    let mut b = Board::new(
        "Live updates, with a recovery path",
        "A browser-to-server channel with authenticated subscriptions, event fan-out and cursor-based catch-up.",
        "04 / REAL-TIME EVENT DELIVERY",
    )?;
    let mut zones = vec![];
    for (i, (label, tone)) in [
        ("BROWSER", PURPLE),
        ("CONNECTION", CYAN),
        ("ACCESS & UI", ROSE),
        ("EVENT ROUTING", AMBER),
        ("APPLICATION DATA", SLATE),
    ]
    .iter()
    .enumerate()
    {
        zones.push(b.shape(
            "container",
            [X[i] - 24., 266., 336., 740.],
            label,
            *tone,
            None,
        )?);
    }
    let specs = [
        (
            0,
            0,
            "01  /  CLIENT\nOpen a live connection\nConnect using a secure channel.\nPresent the current session.",
            PURPLE,
        ),
        (
            1,
            0,
            "02  /  EDGE\nConnection gateway\nAccept the WebSocket upgrade.\nEnforce connection limits.",
            CYAN,
        ),
        (
            2,
            0,
            "03  /  AUTHORIZATION\nAuthorize subscriptions\nValidate identity and channel access.\nScope every subscription to its tenant.",
            ROSE,
        ),
        (
            3,
            0,
            "04  /  SESSION ROUTING\nConnection hub\nTrack active client subscriptions.\nDeliver events to permitted clients.",
            BLUE,
        ),
        (
            4,
            0,
            "05  /  FAN-OUT\nConnected browsers\nReceive versioned event payloads.\nApply only authorized updates.",
            TEAL,
        ),
        (
            0,
            1,
            "NETWORK INTERRUPTION\nReconnect with jitter\nUse bounded exponential backoff.\nAvoid synchronized reconnect storms.",
            AMBER,
        ),
        (
            1,
            1,
            "CONNECTION HEALTH\nHeartbeat and timeout\nDetect a dead or idle connection.\nClose it and let the client recover.",
            CYAN,
        ),
        (
            2,
            1,
            "ACCESS DENIED\nReject the subscription\nReturn a safe error or close code.\nReauthenticate before trying again.",
            ROSE,
        ),
        (
            3,
            1,
            "DISTRIBUTED DELIVERY\nPub/sub transport\nRoute events across gateway nodes.\nDo not assume it stores history.",
            AMBER,
        ),
        (
            4,
            1,
            "DURABLE HISTORY\nEvent log / data store\nRetain versions or replayable events.\nSupport a bounded catch-up window.",
            SLATE,
        ),
        (
            0,
            2,
            "RECOVERY STATE\nKeep the last cursor\nRemember the last applied version.\nPreserve it across reconnects.",
            PURPLE,
        ),
        (
            1,
            2,
            "HTTP RECOVERY\nRequest a catch-up\nSend the cursor to an authorized API.\nUse a snapshot if the cursor expired.",
            BLUE,
        ),
        (
            2,
            2,
            "CLIENT CONSISTENCY\nMerge without duplicates\nApply events in the required order.\nAdvance the cursor after applying.",
            TEAL,
        ),
        (
            3,
            2,
            "EVENT PUBLICATION\nOutbox / publisher\nPublish after the data is committed.\nInclude an event ID and version.",
            AMBER,
        ),
        (
            4,
            2,
            "BUSINESS OPERATION\nApplication service\nChange state in a transaction.\nPersist the corresponding event.",
            BLUE,
        ),
    ];
    let mut n = vec![];
    for (col, row, label, tone) in specs {
        n.push(b.card(col, row, label, tone, zones[col])?);
    }
    for i in 0..4 {
        b.link(
            [(n[i], East), (n[i + 1], West)],
            if i == 3 { "Live events" } else { "" },
            if i == 3 { EVENT } else { REQUEST },
            &[],
        )?;
    }
    b.link([(n[0], South), (n[5], North)], "Disconnect", ERROR, &[])?;
    b.link(
        [(n[5], East), (n[1], West)],
        "Reconnect",
        Flow {
            tone: PURPLE,
            dashed: true,
        },
        &[(402., 646.), (402., 388.)],
    )?;
    b.link([(n[1], South), (n[6], North)], "Ping / pong", RETURN, &[])?;
    b.link([(n[2], South), (n[7], North)], "Denied", ERROR, &[])?;
    b.link(
        [
            (n[3], Custom { x: 0.75, y: 1. }),
            (n[8], Custom { x: 0.75, y: 0. }),
        ],
        "Subscribe",
        REQUEST,
        &[],
    )?;
    b.link(
        [
            (n[8], Custom { x: 0.25, y: 0. }),
            (n[3], Custom { x: 0.25, y: 1. }),
        ],
        "Events",
        EVENT,
        &[],
    )?;
    b.link([(n[14], North), (n[9], South)], "Commit", REQUEST, &[])?;
    b.link([(n[14], West), (n[13], East)], "Outbox", EVENT, &[])?;
    b.link([(n[13], North), (n[8], South)], "Publish", EVENT, &[])?;
    b.link([(n[5], South), (n[10], North)], "Resume", REQUEST, &[])?;
    b.link([(n[10], East), (n[11], West)], "Cursor", REQUEST, &[])?;
    b.link(
        [(n[11], North), (n[9], West)],
        "Read since cursor",
        REQUEST,
        &[(588., 746.), (1518., 746.), (1518., 646.)],
    )?;
    b.link(
        [(n[9], Custom { x: 0.25, y: 1. }), (n[12], East)],
        "Replay / snapshot",
        RETURN,
        &[(1632., 800.), (1146., 800.), (1146., 904.)],
    )?;
    b.link([(n[11], East), (n[12], West)], "Apply", RETURN, &[])?;
    b.notes([
        ("DELIVERY CONTRACT\nLive transport is not durable history\nPub/sub can lose messages while a client is offline.\nUse the recovery API to close the gap.",AMBER),
        ("AUTHORIZATION\nCheck access beyond the handshake\nValidate channel and tenant access for each subscription.\nDefine how expiry or revoked permissions close a session.",ROSE),
        ("RECOVERY CONTRACT\nChoose a versioning strategy\nDocument cursor scope, ordering and retention limits.\nFall back to a fresh snapshot when replay is impossible.",TEAL),
    ])?;
    b.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn web_boards_are_editable_colored_and_keep_connections_when_moved() {
        for id in [
            "web-page-journey",
            "web-api-platform",
            "web-async-checkout",
            "web-realtime-updates",
        ] {
            let doc = build(id).unwrap().unwrap();
            let graph = doc.diagram.as_ref().unwrap();
            assert!(graph.shapes.len() >= 20 && graph.edges.len() >= 15, "{id}");
            let mut colors = std::collections::HashSet::new();
            for shape in graph.shapes.values() {
                let [x, y, w, h] = diagram::shape_bounds(&doc, shape).unwrap();
                assert!(
                    x >= 0. && y >= 0. && x + w <= WIDTH as f64 && y + h <= HEIGHT as f64,
                    "{id}: clipped object"
                );
                if let NodeKind::Path { style, .. } = &doc.node(shape.body).unwrap().kind {
                    colors.insert(style.fill);
                }
            }
            assert!(colors.len() >= 7, "{id}: incomplete palette");
            assert!(doc.nodes.iter().all(|n| matches!(
                n.kind,
                NodeKind::Group { .. }
                    | NodeKind::Fill { .. }
                    | NodeKind::Text { .. }
                    | NodeKind::Path { .. }
            )));
            let movable = graph.edges.values().next().unwrap().source.shape;
            let mut editor = crate::Editor::new(doc.clone(), None);
            editor
                .execute(crate::Command::TranslateNode {
                    id: movable,
                    dx: 13.,
                    dy: 17.,
                })
                .unwrap();
            editor.doc.validate().unwrap();
            editor.undo();
            assert_eq!(editor.doc, doc, "{id}: move undo");
        }
    }
}

/// Permanent toolbox cards reuse the template's typography, palette and icons.
pub(crate) fn insert_stencil(
    editor: &mut crate::Editor,
    id: &str,
    bounds: [f64; 4],
) -> Result<NodeId, String> {
    if bounds.iter().any(|v| !v.is_finite() || v.abs() > 1e6) || bounds[2] < 1. || bounds[3] < 1. {
        return Err("Invalid web stencil bounds".into());
    }
    let (label, tone) = match id {
        "web-browser" => ("CLIENT\nBrowser\nRequest • render • interact", PURPLE),
        "web-edge" => ("NETWORK\nDNS / CDN edge\nResolve • cache • deliver", CYAN),
        "web-gateway" => ("ENTRY POINT\nAPI gateway\nRoute • limit • observe", CYAN),
        "web-auth" => (
            "SECURITY\nIdentity / auth\nVerify token • enforce policy",
            ROSE,
        ),
        "web-api" => (
            "APPLICATION\nApplication server\nValidate • execute • respond",
            BLUE,
        ),
        "web-database" => ("PERSISTENCE\nDatabase\nQuery • transact • commit", SLATE),
        "web-cache" => ("FAST PATH\nCache\nLookup • expire • invalidate", AMBER),
        "web-queue" => ("ASYNC\nMessage queue\nBuffer • deliver • retry", AMBER),
        "web-worker" => (
            "PROCESSING\nBackground worker\nConsume • execute • acknowledge",
            BLUE,
        ),
        "web-response" => ("SUCCESS\nHTTP response\nStatus • headers • payload", TEAL),
        "web-error" => ("FAILURE\nError / retry\nTimeout • backoff • recover", ROSE),
        "web-websocket" => (
            "REALTIME\nWebSocket connection\nSubscribe • publish • reconnect",
            CYAN,
        ),
        "web-outbox" => (
            "RELIABILITY\nTransactional outbox\nCommit event • relay • deduplicate",
            AMBER,
        ),
        "web-storage" => (
            "ASSETS\nObject storage\nUpload • version • distribute",
            SLATE,
        ),
        _ => return Err("Unknown web stencil".into()),
    };
    let mut board = Board::new("", "", "")?;
    let root = board.shape("process", [0., 0., 288., 148.], label, tone, None)?;
    let mut doc = board.finish()?;
    Arc::make_mut(doc.diagram.as_mut().unwrap())
        .shapes
        .get_mut(&root)
        .unwrap()
        .data
        .insert("emulsion_stencil".into(), id.into());
    let ids = doc.subtree(root);
    crate::transform::transform_nodes(
        &mut doc,
        &ids,
        [
            bounds[2] / 288.,
            0.,
            0.,
            bounds[3] / 148.,
            bounds[0],
            bounds[1],
        ],
    )
    .map_err(|e| e.to_string())?;
    let fragment = crate::fragment::Fragment::capture(&doc, &[root])?;
    let roots = fragment.paste(editor, crate::command::Slot::TOP, (0., 0.))?;
    roots
        .first()
        .copied()
        .ok_or("Web stencil has no root".into())
}
