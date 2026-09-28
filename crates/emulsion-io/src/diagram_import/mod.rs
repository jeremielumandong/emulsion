//! Local editable diagram interchange. No cloud service or source-file writes.
use crate::{IoError, Result};
use emulsion_core::{
    Document, Node, NodeKind,
    diagram::{self, Builder, Endpoint, Port, Routing, ShapeKind},
    graph::Graph,
    project::{PageMeta, Project, ProjectKind, ProjectPage},
    text::TextSpec,
    vector_cache::VectorRaster,
};
use emulsion_raster::vector::{Path as VectorPath, PathStyle};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    io::Read,
    path::Path,
    sync::Arc,
};
mod legacy_visio;
mod lucid;
mod visio;
mod visio_curves;
mod xml;
pub use crate::drawio::Imported;
const MAX_FILE: u64 = 64 << 20;
fn error(message: impl Into<String>) -> IoError {
    IoError::Manifest(message.into())
}
pub fn is_diagram(path: &Path) -> bool {
    if path
        .extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("svg"))
    {
        // A bounded header probe keeps ordinary SVG artwork on its existing path.
        let mut header = Vec::new();
        return std::fs::File::open(path).ok().is_some_and(|file| {
            file.take(8192).read_to_end(&mut header).is_ok()
                && String::from_utf8_lossy(&header).contains("<svg")
                && String::from_utf8_lossy(&header).contains("content=")
        });
    }
    path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        [
            "drawio",
            "xml",
            "vsdx",
            "vsdm",
            "vstx",
            "vssx",
            "vssm",
            "vstm",
            "vss",
            "vst",
            "vdx",
            "vsx",
            "lucid",
            "lucidjson",
            "vsd",
        ]
        .iter()
        .any(|e| s.eq_ignore_ascii_case(e))
    })
}
pub fn read(path: &Path) -> Result<Imported> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "vsdx" | "vsdm" | "vstx" | "vssx" | "vssm" | "vstm" => visio::package(path).or_else(|original| {
            legacy_visio::read(path).map(|mut imported|{imported.warnings.push(format!("Native Visio geometry could not be evaluated; converted vector appearance used: {original}"));imported}).map_err(|_|original)
        }),
        "vdx" | "vsx" => visio::from_xml(&read_text(path)?).or_else(|original|legacy_visio::read(path).map(|mut i|{i.warnings.push(format!("Converted Visio XML after native import failed: {original}"));i}).map_err(|_|original)),
        "lucid" => lucid::package(path),
        "lucidjson" | "json" => lucid::from_json(&read_text(path)?),
        "vsd" | "vss" | "vst" => legacy_visio::read(path),
        "xml" => {
            let text = read_text(path)?;
            if text.contains("VisioDocument") {
                visio::from_xml(&text)
            } else {
                crate::drawio::from_xml(&text)
            }
        }
        _ => crate::drawio::read(path),
    }
}
fn read_text(path: &Path) -> Result<String> {
    let mut text = String::new();
    std::fs::File::open(path)?
        .take(MAX_FILE + 1)
        .read_to_string(&mut text)?;
    if text.len() as u64 > MAX_FILE {
        return Err(error("Diagram exceeds the 64 MiB input limit."));
    }
    Ok(text)
}
struct Package {
    entries: BTreeMap<String, Vec<u8>>,
}
impl Package {
    fn read(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path)?;
        if file.metadata()?.len() > MAX_FILE {
            return Err(error("Diagram package exceeds 64 MiB."));
        }
        let mut zip = zip::ZipArchive::new(file)?;
        if zip.len() > 4096 {
            return Err(error("Too many diagram archive entries."));
        }
        let mut entries = BTreeMap::new();
        let mut total = 0u64;
        for i in 0..zip.len() {
            let entry = zip.by_index(i)?;
            let name = entry.name().to_string();
            if entry.enclosed_name().is_none() || name.contains('\\') || entries.contains_key(&name)
            {
                return Err(error("Unsafe or duplicate diagram archive path."));
            }
            total = total
                .checked_add(entry.size())
                .ok_or_else(|| error("Archive size overflow."))?;
            if total > 256 << 20 || entry.size() > MAX_FILE {
                return Err(error("Diagram archive exceeds its decoded size limit."));
            }
            let mut bytes = Vec::new();
            entry.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_FILE {
                return Err(error("Diagram archive entry is too large."));
            }
            entries.insert(name, bytes);
        }
        Ok(Self { entries })
    }
    fn text(&self, name: &str) -> Result<&str> {
        let bytes = self
            .entries
            .get(name)
            .ok_or_else(|| error(format!("Missing diagram part: {name}")))?;
        std::str::from_utf8(bytes).map_err(|_| error("Diagram XML/JSON must be UTF-8."))
    }
}
struct Shape {
    key: String,
    name: String,
    kind: ShapeKind,
    bounds: [f64; 4],
    text: TextSpec,
    path: Option<VectorPath>,
    style: PathStyle,
    data: BTreeMap<String, String>,
    parent: Option<String>,
    opacity: f32,
    visible: bool,
    image: Option<Arc<emulsion_raster::Raster>>,
}
impl Shape {
    fn new(key: String, kind: ShapeKind, bounds: [f64; 4], text: String) -> Self {
        Self {
            key,
            name: kind.label().into(),
            kind,
            bounds,
            text: TextSpec {
                text,
                font: "Geist".into(),
                size: 14.,
                color: [0, 0, 0, 255],
                ..Default::default()
            },
            path: None,
            style: PathStyle {
                fill: Some([255; 4]),
                stroke: Some([0, 0, 0, 255]),
                width: 1.,
                ..Default::default()
            },
            data: BTreeMap::new(),
            parent: None,
            opacity: 1.,
            visible: true,
            image: None,
        }
    }
}
struct Line {
    key: String,
    source: (String, Port),
    target: (String, Port),
    label: String,
    style: PathStyle,
    routing: Routing,
    points: Vec<(f64, f64)>,
    start_arrow: bool,
    end_arrow: bool,
    parent: Option<String>,
}
#[derive(Default)]
struct Scene {
    fit: bool,
    name: String,
    width: u32,
    height: u32,
    background: Option<[u8; 4]>,
    shapes: Vec<Shape>,
    lines: Vec<Line>,
    groups: Vec<(String, String, Option<String>)>,
}
impl Scene {
    fn build(self, id: u64, warnings: &mut BTreeSet<String>) -> Result<ProjectPage> {
        let mut ids = HashMap::new();
        let mut builder = Builder::new(self.width, self.height).map_err(error)?;
        for s in &self.shapes {
            if s.key.is_empty() || ids.contains_key(&s.key) {
                return Err(error("Missing or duplicate shape ID."));
            }
            ids.insert(
                s.key.clone(),
                builder
                    .add_shape(s.kind, s.bounds, &s.text.text)
                    .map_err(error)?,
            );
        }
        for l in &self.lines {
            if l.key.is_empty() || ids.contains_key(&l.key) {
                return Err(error("Missing or duplicate line ID."));
            }
            let source = *ids
                .get(&l.source.0)
                .ok_or_else(|| error(format!("Missing connector source {}", l.source.0)))?;
            let target = *ids
                .get(&l.target.0)
                .ok_or_else(|| error(format!("Missing connector target {}", l.target.0)))?;
            ids.insert(
                l.key.clone(),
                builder
                    .connect(
                        Endpoint {
                            shape: source,
                            port: l.source.1,
                        },
                        Endpoint {
                            shape: target,
                            port: l.target.1,
                        },
                        &l.label,
                        l.routing,
                    )
                    .map_err(error)?,
            );
        }
        let mut doc = builder.finish().map_err(error)?;
        let mut model = doc.diagram.take().unwrap().as_ref().clone();
        if let Some(bg) = self.background {
            doc.nodes[0].kind = NodeKind::Fill { rgba: bg };
        }
        let mut parents = Vec::new();
        for (key, name, parent) in self.groups {
            if key.is_empty() || ids.contains_key(&key) {
                return Err(error("Duplicate group ID."));
            }
            let group = doc.alloc_id();
            doc.nodes
                .push(Node::new(group, name, NodeKind::Group { collapsed: false }));
            ids.insert(key, group);
            parents.push((group, parent));
        }
        for s in self.shapes {
            let id = ids[&s.key];
            let shape = model.shapes.get_mut(&id).unwrap();
            shape.data = s.data;
            shape.data.insert("import_id".into(), s.key);
            let node = doc.node_mut(id).unwrap();
            node.name = s.name;
            node.opacity = s.opacity;
            node.visible = s.visible;
            let node = doc.node_mut(shape.body).unwrap();
            let NodeKind::Path { path, style, cache } = &mut node.kind else {
                unreachable!()
            };
            if let Some(custom) = s.path {
                *path = Arc::new(custom);
            }
            *style = s.style;
            *cache = VectorRaster::path(path.clone(), *style, self.width, self.height);
            let node = doc.node_mut(shape.label).unwrap();
            let NodeKind::Text { spec, cache } = &mut node.kind else {
                unreachable!()
            };
            let mut text = s.text;
            text.x = s.bounds[0] as f32 + 8.;
            text.y = (s.bounds[1] + (s.bounds[3] - text.size as f64 * 1.4).max(0.) / 2.) as f32;
            text.width = Some((s.bounds[2] - 16.).max(1.) as f32);
            *spec = Arc::new(text);
            *cache = VectorRaster::text(spec.clone(), self.width, self.height);
            if let Some(image) = s.image {
                let image_id = doc.alloc_id();
                let [x, y, w, h] = s.bounds;
                let sx = w / image.width() as f64;
                let sy = h / image.height() as f64;
                let placement = emulsion_raster::Placement {
                    x: x - (image.width() as f64 - w) / 2.,
                    y: y - (image.height() as f64 - h) / 2.,
                    scale_x: sx,
                    scale_y: sy,
                    ..Default::default()
                };
                let mut node = Node::raster(image_id, "Embedded image", image, placement);
                node.parent = Some(id);
                node.clip_to = Some(shape.body);
                let label_index = doc.nodes.iter().position(|n| n.id == shape.label).unwrap();
                doc.nodes.insert(label_index, node);
            }
            parents.push((id, s.parent));
        }
        for l in self.lines {
            let id = ids[&l.key];
            let edge = model.edges.get_mut(&id).unwrap();
            edge.waypoints = l.points;
            edge.arrow_start = l.start_arrow;
            edge.arrow_end = l.end_arrow;
            if let NodeKind::Path { path, style, cache } =
                &mut doc.node_mut(edge.path).unwrap().kind
            {
                *style = l.style;
                *cache = VectorRaster::path(path.clone(), *style, self.width, self.height);
            }
            parents.push((id, l.parent));
        }
        for (id, parent) in parents {
            if let Some(parent) = parent {
                doc.node_mut(id).unwrap().parent = Some(
                    *ids.get(&parent)
                        .ok_or_else(|| error("Missing imported group."))?,
                );
            }
        }
        let parents = doc
            .nodes
            .iter()
            .map(|n| (n.id, n.parent))
            .collect::<HashMap<_, _>>();
        for n in &doc.nodes {
            let mut seen = HashSet::new();
            let mut current = Some(n.id);
            while let Some(id) = current {
                if !seen.insert(id) || seen.len() > 64 {
                    return Err(error("Cyclic or deeply nested imported groups."));
                }
                current = parents.get(&id).copied().flatten();
            }
        }
        doc.normalize();
        doc.diagram = Some(Arc::new(model));
        diagram::synchronize(&Document::new(self.width, self.height), &mut doc).map_err(error)?;
        if self.fit {
            let b=doc.nodes.iter().filter(|n|n.parent.is_none() && !matches!(n.kind,NodeKind::Fill{..})).filter_map(|n|emulsion_core::geometry::node_bounds(&doc,n.id)).fold(emulsion_raster::IRect::default(),|a,b|a.union(&b));
            if !b.is_empty(){emulsion_core::geometry::crop(&mut doc,emulsion_raster::IRect::new(b.x-2,b.y-2,b.w+4,b.h+4),0.);}
        }
        doc.validate().map_err(|e| error(e.to_string()))?;
        if doc
            .diagram
            .as_ref()
            .unwrap()
            .shapes
            .values()
            .filter_map(|s| diagram::shape_bounds(&doc, s))
            .any(|[x, y, w, h]| {
                x < 0. || y < 0. || x + w > self.width as f64 || y + h > self.height as f64
            })
        {
            warnings.insert(
                "Some objects extend outside the page; enlarge the canvas to include them.".into(),
            );
        }
        Ok(ProjectPage {
            meta: PageMeta {
                id,
                name: if self.name.trim().is_empty() {
                    format!("Page {id}")
                } else {
                    self.name
                },
                bleed_mm: 0.,
            },
            graph: Graph::new(doc.clone(), "Imported diagram"),
            doc,
        })
    }
}
fn finish(scenes: Vec<Scene>, mut warnings: BTreeSet<String>) -> Result<Imported> {
    if scenes.is_empty() { return Err(error("Diagram contains no drawing pages or stencil masters.")); }
    if scenes.len() > emulsion_core::project::MAX_PAGES {
        return Err(error("Diagram page count exceeds the project limit."));
    }
    let pages = scenes
        .into_iter()
        .enumerate()
        .map(|(i, s)| s.build(i as u64 + 1, &mut warnings))
        .collect::<Result<Vec<_>>>()?;
    let project = Project {
        kind: ProjectKind::Diagram,
        active: 1,
        next_page_id: pages.len() as u64 + 1,
        pages,
    };
    project.validate().map_err(error)?;
    Ok(Imported {
        project,
        warnings: warnings.into_iter().collect(),
    })
}
fn color(s: &str) -> Result<[u8; 4]> {
    let hex = s
        .strip_prefix('#')
        .ok_or_else(|| error(format!("Unsupported color {s}")))?;
    let hex = if hex.len() == 3 || hex.len() == 4 {
        hex.chars().flat_map(|c| [c, c]).collect::<String>()
    } else {
        hex.to_string()
    };
    if ![6, 8].contains(&hex.len()) || !hex.is_ascii() {
        return Err(error("Invalid color."));
    }
    let mut rgba = [255; 4];
    for (i, c) in rgba.iter_mut().enumerate().take(hex.len() / 2) {
        *c = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).map_err(|_| error("Invalid color."))?;
    }
    Ok(rgba)
}

#[cfg(test)]
mod tests;
