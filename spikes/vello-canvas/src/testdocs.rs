//! Test documents, saved as ORA so the spike and the GPUI build open the
//! same files.

use emulsion_core::command::Slot;
use emulsion_core::text::TextSpec;
use emulsion_core::{Command, Document, Node, NodeId};
use emulsion_raster::blend::{BlendMode, BlendSpace};
use emulsion_raster::color;
use emulsion_raster::vector::{Anchor, Path, PathStyle, StrokeCap, StrokeJoin, SubPath};
use emulsion_raster::{Mask, Placement, Raster};
use std::path::Path as FsPath;
use std::sync::Arc;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next()
    }
    fn byte(&mut self) -> u8 {
        (self.next() * 255.0) as u8
    }
}

fn add(doc: &mut Document, node: Node) -> NodeId {
    Command::AddNode {
        node: Box::new(node),
        slot: Slot::TOP,
    }
    .apply(doc)
    .expect("add node")
    .expect("node id")
}

fn group(doc: &mut Document, ids: Vec<NodeId>, name: &str) -> NodeId {
    Command::Group {
        ids,
        name: name.into(),
    }
    .apply(doc)
    .expect("group")
    .expect("group id")
}

/// Premultiplied linear pixel from straight sRGB floats.
fn px(r: f32, g: f32, b: f32, a: f32) -> [u16; 4] {
    let a = a.clamp(0.0, 1.0);
    color::f_to_px([
        color::srgb_to_linear(r.clamp(0.0, 1.0)) * a,
        color::srgb_to_linear(g.clamp(0.0, 1.0)) * a,
        color::srgb_to_linear(b.clamp(0.0, 1.0)) * a,
        a,
    ])
}

/// A soft blob with a smooth 16-bit gradient and fine grain.
fn blob(w: u32, h: u32, rng: &mut Rng, radius: f32, tint: [f32; 3]) -> Raster {
    let (cx, cy) = (
        rng.range(0.1, 0.9) * w as f32,
        rng.range(0.1, 0.9) * h as f32,
    );
    let (sx, sy) = (rng.range(0.6, 1.6), rng.range(0.6, 1.6));
    let feather = radius * rng.range(0.05, 0.4);
    let seed = rng.next() * 1000.0;
    Raster::from_fn(w, h, [0; 4], move |x, y| {
        let (dx, dy) = ((x as f32 - cx) / sx, (y as f32 - cy) / sy);
        let d = (dx * dx + dy * dy).sqrt();
        let a = ((radius - d) / feather).clamp(0.0, 1.0);
        if a <= 0.0 {
            return [0; 4];
        }
        let t = (x as f32 / w as f32 + seed).sin() * 0.5 + 0.5;
        let grain = (((x * 7 + y * 13) % 17) as f32 / 17.0 - 0.5) * 0.02;
        px(
            tint[0] * (0.6 + 0.4 * t) + grain,
            tint[1] * (0.5 + 0.5 * (1.0 - t)) + grain,
            tint[2] + grain,
            a,
        )
    })
}

/// A full-canvas translucent texture layer.
fn wash(w: u32, h: u32, rng: &mut Rng) -> Raster {
    let (fx, fy) = (rng.range(0.001, 0.006), rng.range(0.001, 0.006));
    let tint = [rng.next(), rng.next(), rng.next()];
    Raster::from_fn(w, h, [0; 4], move |x, y| {
        let v = ((x as f32 * fx).sin() * (y as f32 * fy).cos()) * 0.5 + 0.5;
        px(tint[0] * v, tint[1], tint[2] * (1.0 - v), 0.25 + 0.5 * v)
    })
}

const MODES: [BlendMode; 12] = [
    BlendMode::Multiply,
    BlendMode::Screen,
    BlendMode::Overlay,
    BlendMode::SoftLight,
    BlendMode::HardLight,
    BlendMode::ColorDodge,
    BlendMode::Darken,
    BlendMode::Lighten,
    BlendMode::Difference,
    BlendMode::Color,
    BlendMode::Luminosity,
    BlendMode::LinearLight,
];

/// 3840×2160, 25 nodes: background, 8 full-canvas washes, 12 partial
/// blobs across blend modes, an isolated group, a pass-through group, a
/// clipped layer, a masked layer and a fill.
pub fn layers_4k() -> Document {
    let (w, h) = (3840, 2160);
    let mut rng = Rng(0x5EED_4C4B);
    let mut doc = Document::new(w, h);
    let bg = Raster::from_fn(w, h, [0; 4], |x, y| {
        let t = y as f32 / h as f32;
        let n = ((x * 7 + y * 13) % 23) as f32 / 23.0 * 0.03;
        px(
            0.15 + 0.6 * t + n,
            0.35 + 0.4 * t + n,
            0.8 - 0.5 * t + n,
            1.0,
        )
    });
    add(
        &mut doc,
        Node::raster(0, "Background", Arc::new(bg), Placement::default()),
    );
    for i in 0..8 {
        let mut n = Node::raster(
            0,
            format!("Wash {i}"),
            Arc::new(wash(w, h, &mut rng)),
            Placement::default(),
        );
        n.blend = MODES[i % MODES.len()];
        n.opacity = rng.range(0.3, 0.9);
        add(&mut doc, n);
    }
    let mut blobs = Vec::new();
    for i in 0..12 {
        let tint = [rng.next(), rng.next(), rng.next()];
        let radius = rng.range(250.0, 700.0);
        let mut n = Node::raster(
            0,
            format!("Blob {i}"),
            Arc::new(blob(w, h, &mut rng, radius, tint)),
            Placement::default(),
        );
        n.blend = MODES[(i * 5 + 3) % MODES.len()];
        n.opacity = rng.range(0.5, 1.0);
        blobs.push(add(&mut doc, n));
    }
    // Clip blob 11 to blob 10.
    Command::SetClip {
        id: blobs[11],
        clip_to: Some(blobs[10]),
    }
    .apply(&mut doc)
    .expect("clip");
    let isolated = group(&mut doc, blobs[0..4].to_vec(), "Isolated group");
    Command::SetOpacity {
        id: isolated,
        opacity: 0.8,
    }
    .apply(&mut doc)
    .expect("opacity");
    let pass = group(&mut doc, blobs[4..7].to_vec(), "Pass-through group");
    if let Some(n) = doc.node_mut(pass) {
        n.blend = BlendMode::PassThrough;
    }
    // A masked layer: the mask fades left to right.
    let mut masked = Node::raster(
        0,
        "Masked",
        Arc::new(blob(w, h, &mut rng, 900.0, [0.9, 0.7, 0.2])),
        Placement::default(),
    );
    masked.mask = Some(Arc::new(Mask::from_fn(w, h, 255, move |x, _| {
        (x * 255 / w) as u8
    })));
    masked.blend = BlendMode::Screen;
    add(&mut doc, masked);
    let mut fill = Node::new(
        0,
        "Warm fill",
        emulsion_core::NodeKind::Fill {
            rgba: [255, 200, 150, 255],
        },
    );
    fill.blend = BlendMode::Multiply;
    fill.opacity = 0.25;
    add(&mut doc, fill);
    doc
}

fn random_path(rng: &mut Rng, w: f32, h: f32, size: f32) -> Path {
    let (cx, cy) = (rng.range(0.0, w), rng.range(0.0, h));
    let n = 3 + (rng.next() * 6.0) as usize;
    let mut anchors = Vec::with_capacity(n);
    for i in 0..n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        let r = size * rng.range(0.4, 1.0);
        let p = ((cx + a.cos() * r) as f64, (cy + a.sin() * r) as f64);
        let k = rng.range(0.0, 0.5) * r;
        let (tx, ty) = (-a.sin() * k, a.cos() * k);
        anchors.push(Anchor {
            p,
            h_in: (p.0 - tx as f64, p.1 - ty as f64),
            h_out: (p.0 + tx as f64, p.1 + ty as f64),
            smooth: true,
        });
    }
    Path {
        subpaths: vec![SubPath {
            anchors,
            closed: rng.next() > 0.2,
        }],
    }
}

fn random_style(rng: &mut Rng, translucent: bool) -> PathStyle {
    let alpha = |rng: &mut Rng| {
        if translucent {
            (rng.range(0.35, 0.85) * 255.0) as u8
        } else {
            255
        }
    };
    PathStyle {
        fill: (rng.next() > 0.15).then(|| [rng.byte(), rng.byte(), rng.byte(), alpha(rng)]),
        stroke: (rng.next() > 0.3).then(|| [rng.byte(), rng.byte(), rng.byte(), alpha(rng)]),
        width: rng.range(1.0, 12.0),
        cap: [StrokeCap::Butt, StrokeCap::Round, StrokeCap::Square]
            [(rng.next() * 3.0) as usize % 3],
        join: [StrokeJoin::Miter, StrokeJoin::Round, StrokeJoin::Bevel]
            [(rng.next() * 3.0) as usize % 3],
        dash_count: if rng.next() > 0.85 { 2 } else { 0 },
        dash: [12.0, 6.0, 0.0, 0.0, 0.0, 0.0],
        ..PathStyle::default()
    }
}

const WORDS: [&str; 8] = [
    "Emulsion",
    "Vello canvas spike",
    "The quick brown fox",
    "jumps over the lazy dog",
    "Omarchy diagrams",
    "Design editor",
    "linear light",
    "0123456789",
];

fn text(rng: &mut Rng, w: f32, h: f32, translucent: bool) -> TextSpec {
    TextSpec {
        text: WORDS[(rng.next() * WORDS.len() as f32) as usize % WORDS.len()].into(),
        size: rng.range(18.0, 96.0),
        color: [
            rng.byte(),
            rng.byte(),
            rng.byte(),
            if translucent { 200 } else { 255 },
        ],
        x: rng.range(0.0, w * 0.85),
        y: rng.range(0.0, h * 0.95),
        rotation: if rng.next() > 0.7 {
            rng.range(-30.0, 30.0)
        } else {
            0.0
        },
        width: (rng.next() > 0.6).then(|| rng.range(200.0, 600.0)),
        ..TextSpec::default()
    }
}

/// 3840×2160: a background raster, `paths` random editable paths and
/// `texts` text boxes.
pub fn vectors(paths: usize, texts: usize) -> Document {
    let (w, h) = (3840u32, 2160u32);
    let mut rng = Rng(0x7EC7_0125);
    let mut doc = Document::new(w, h);
    add(
        &mut doc,
        Node::raster(
            0,
            "Paper",
            Arc::new(Raster::solid(w, h, [0.92, 0.9, 0.86, 1.0])),
            Placement::default(),
        ),
    );
    let total = paths + texts;
    for i in 0..total {
        // Interleave text boxes through the stack.
        let node =
            if texts > 0 && i % (total / texts).max(1) == 0 && i / (total / texts).max(1) < texts {
                Node::text(
                    0,
                    format!("Text {i}"),
                    text(&mut rng, w as f32, h as f32, false),
                    w,
                    h,
                )
            } else {
                let size = rng.range(20.0, 160.0);
                Node::path(
                    0,
                    format!("Path {i}"),
                    Arc::new(random_path(&mut rng, w as f32, h as f32, size)),
                    random_style(&mut rng, false),
                    w,
                    h,
                )
            };
        add(&mut doc, node);
    }
    doc
}

/// 1024×768 fidelity sheet: every blend mode as a column over a gradient,
/// groups, clipping, a mask, and overlapping translucent vectors and text.
pub fn fidelity(space: BlendSpace) -> Document {
    let (w, h) = (1024u32, 768u32);
    let mut rng = Rng(0xF1DE_117E);
    let mut doc = Document::new(w, h);
    doc.blend_space = space;
    add(
        &mut doc,
        Node::raster(
            0,
            "Gradient",
            Arc::new(Raster::from_fn(w, h, [0; 4], |x, y| {
                px(
                    x as f32 / w as f32,
                    y as f32 / h as f32,
                    1.0 - x as f32 / w as f32,
                    1.0,
                )
            })),
            Placement::default(),
        ),
    );
    let modes: Vec<BlendMode> = BlendMode::MENU
        .iter()
        .flatten()
        .copied()
        .filter(|m| *m != BlendMode::Dissolve && *m != BlendMode::Normal)
        .collect();
    let col = w as f32 / modes.len() as f32;
    for (i, mode) in modes.iter().enumerate() {
        let x0 = (i as f32 * col) as u32;
        let x1 = ((i + 1) as f32 * col) as u32;
        let mut n = Node::raster(
            0,
            format!("{mode:?}"),
            Arc::new(Raster::from_fn(w, h, [0; 4], move |x, y| {
                if x < x0 || x >= x1 || y > h / 2 {
                    return [0; 4];
                }
                let t = y as f32 / (h / 2) as f32;
                px(0.9 * t, 0.4, 1.0 - t, 0.4 + 0.6 * t)
            })),
            Placement::default(),
        );
        n.blend = *mode;
        n.opacity = 0.85;
        add(&mut doc, n);
    }
    // Clip and mask inside an isolated group.
    let base = add(
        &mut doc,
        Node::raster(
            0,
            "Clip base",
            Arc::new(blob(w, h, &mut rng, 160.0, [0.2, 0.8, 0.4])),
            Placement::default(),
        ),
    );
    let mut clipped = Node::raster(
        0,
        "Clipped",
        Arc::new(wash(w, h, &mut rng)),
        Placement::default(),
    );
    clipped.blend = BlendMode::Overlay;
    let clipped = add(&mut doc, clipped);
    Command::SetClip {
        id: clipped,
        clip_to: Some(base),
    }
    .apply(&mut doc)
    .expect("clip");
    let g = group(&mut doc, vec![base, clipped], "Group");
    Command::SetOpacity {
        id: g,
        opacity: 0.7,
    }
    .apply(&mut doc)
    .expect("opacity");
    let mut masked = Node::raster(
        0,
        "Masked",
        Arc::new(blob(w, h, &mut rng, 220.0, [0.9, 0.2, 0.6])),
        Placement::default(),
    );
    masked.mask = Some(Arc::new(Mask::from_fn(w, h, 255, move |_, y| {
        (y * 255 / h) as u8
    })));
    add(&mut doc, masked);
    // Offset (placed) layer: baked by the spike, sampled by the CPU.
    let mut placed = Node::raster(
        0,
        "Placed",
        Arc::new(blob(300, 200, &mut rng, 90.0, [0.1, 0.3, 0.9])),
        Placement::at(40.0, 520.0),
    );
    placed.blend = BlendMode::Multiply;
    add(&mut doc, placed);
    // Opaque vectors (single-object edges), then overlapping translucent ones.
    for i in 0..30 {
        let translucent = i >= 15;
        add(
            &mut doc,
            Node::path(
                0,
                format!("Path {i}"),
                Arc::new(random_path(&mut rng, w as f32, h as f32, 70.0)),
                random_style(&mut rng, translucent),
                w,
                h,
            ),
        );
    }
    for i in 0..6 {
        add(
            &mut doc,
            Node::text(
                0,
                format!("Text {i}"),
                text(&mut rng, w as f32, h as f32, i >= 3),
                w,
                h,
            ),
        );
    }
    doc
}

type Builder = Box<dyn Fn() -> Document>;

pub fn write_all(dir: &FsPath) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let jobs: Vec<(&str, Builder)> = vec![
        ("layers-4k.ora", Box::new(layers_4k)),
        ("vectors-500.ora", Box::new(|| vectors(480, 20))),
        (
            "fidelity-linear.ora",
            Box::new(|| fidelity(BlendSpace::Linear)),
        ),
        ("fidelity-srgb.ora", Box::new(|| fidelity(BlendSpace::Srgb))),
    ];
    for (name, build) in jobs {
        let t = std::time::Instant::now();
        let doc = build();
        let path = dir.join(name);
        emulsion_io::save(&doc, &path)?;
        println!(
            "{} ({} nodes) in {:.1} s",
            path.display(),
            doc.nodes.len(),
            t.elapsed().as_secs_f64()
        );
    }
    Ok(())
}
