//! Visio OPC/VDX diagrams using evaluated ShapeSheet values, native paths and text.
//! https://learn.microsoft.com/en-us/office/client-developer/visio/visio-file-format-reference
use super::xml::Xml;
use super::*;
use emulsion_raster::vector::{Anchor, SubPath};
use glam::{DAffine2, DVec2, dvec2};
const DPI: f64 = 96.;
fn value<'a>(node: &'a Xml, key: &str) -> Option<&'a str> {
    node.children
        .iter()
        .find(|n| n.name == "Cell" && n.attr("N") == key)
        .map(|n| n.attr("V"))
        .filter(|s| !s.is_empty() && *s != "Inh")
        .or_else(|| {
            node.child(key)
                .map(|n| n.text.trim())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            node.children
                .iter()
                .filter(|n| {
                    [
                        "XForm",
                        "XForm1D",
                        "Line",
                        "Fill",
                        "Char",
                        "TextBlock",
                        "PageProps",
                    ]
                    .contains(&n.name.as_str())
                })
                .find_map(|n| value(n, key))
        })
}
fn number(node: &Xml, master: Option<&Xml>, key: &str, default: f64) -> Result<f64> {
    let Some(v) = value(node, key).or_else(|| master.and_then(|n| value(n, key))) else {
        return Ok(default);
    };
    if v.eq_ignore_ascii_case("Themed") {
        return Ok(master.and_then(|m|value(m,key)).and_then(|v|v.parse::<f64>().ok()).filter(|v|v.is_finite()&&v.abs()<=1e6).unwrap_or(default));
    }
    v.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && v.abs() <= 1e6)
        .ok_or_else(|| error(format!("Visio cell {key} has no finite evaluated value.")))
}
fn part(base: &str, target: &str) -> Result<String> {
    if target.contains(['\\', ':', '?', '#']) {
        return Err(error("Unsupported external package relationship."));
    }
    let mut parts = if target.starts_with('/') {
        Vec::new()
    } else {
        base.split('/').map(str::to_string).collect::<Vec<_>>()
    };
    if !target.starts_with('/') {
        parts.pop();
    }
    for component in target.trim_start_matches('/').split('/') {
        match component {
            "." | "" => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(error("Unsafe package relationship."));
                }
            }
            s => parts.push(s.into()),
        }
    }
    Ok(parts.join("/"))
}
fn relationships(package: &Package, source: &str) -> Result<BTreeMap<String, String>> {
    let (dir, name) = source.rsplit_once('/').unwrap_or(("", source));
    let rel = if dir.is_empty() {
        format!("_rels/{name}.rels")
    } else {
        format!("{dir}/_rels/{name}.rels")
    };
    if !package.entries.contains_key(&rel) {
        return Ok(BTreeMap::new());
    }
    let root = xml::parse(package.text(&rel)?)?;
    let mut out = BTreeMap::new();
    for relationship in root.children("Relationship") {
        if relationship.attr("TargetMode") == "External" {
            continue;
        }
        let id = relationship.attr("Id");
        if id.is_empty() || out.contains_key(id) {
            return Err(error("Invalid relationship ID."));
        }
        out.insert(id.into(), part(source, relationship.attr("Target"))?);
    }
    Ok(out)
}
#[derive(Default)]
struct Resources {
    masters: BTreeMap<String, Xml>,
    colors: BTreeMap<String, [u8; 4]>,
    fonts: BTreeMap<String, String>,
    styles: BTreeMap<String, Xml>,
}
impl Resources {
    fn from_document(document: &Xml) -> Result<Self> {
        let mut out = Self::default();
        for c in document.descendants("ColorEntry") {
            out.colors
                .insert(c.attr("IX").into(), color(c.attr("RGB"))?);
        }
        for f in document
            .descendants("FaceName")
            .chain(document.descendants("FontEntry"))
        {
            out.fonts.insert(f.attr("ID").into(), f.attr("Name").into());
        }
        for s in document.descendants("StyleSheet") {
            out.styles.insert(s.attr("ID").into(), s.clone());
        }
        Ok(out)
    }
    fn color(
        &self,
        node: &Xml,
        master: Option<&Xml>,
        key: &str,
        default: [u8; 4],
        warnings: &mut BTreeSet<String>,
    ) -> [u8; 4] {
        let Some(value) = value(node, key).or_else(|| master.and_then(|n| value(n, key))) else {
            return default;
        };
        if let Ok(c) = color(value) {
            return c;
        }
        if let Some(c) = self.colors.get(value) {
            return *c;
        }
        // The base palette is specified by Visio's indexed colors.
        if let Ok(index) = value.parse::<usize>() {
            let palette = [
                [0, 0, 0, 255],
                [255; 4],
                [255, 0, 0, 255],
                [0, 255, 0, 255],
                [0, 0, 255, 255],
                [255, 255, 0, 255],
                [255, 0, 255, 255],
                [0, 255, 255, 255],
            ];
            if let Some(c) = palette.get(index) {
                return *c;
            }
        }
        warnings.insert(
            "Theme-dependent Visio colors without evaluated RGB values use a default color.".into(),
        );
        default
    }
}
pub(super) fn package(path: &Path) -> Result<Imported> {
    let mut signature = [0; 8];
    let mut file = std::fs::File::open(path)?;
    if file.read_exact(&mut signature).is_ok()
        && signature == [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]
    {
        return Err(error(
            "This file contains legacy binary Visio data despite its extension. Convert it in Visio to a real .vsdx, .vssx, or .vstx package before import.",
        ));
    }
    let package = Package::read(path)?;
    let document = xml::parse(package.text("visio/document.xml")?)?;
    let mut resources = Resources::from_document(&document)?;
    let mut warnings = BTreeSet::new();
    if package
        .entries
        .keys()
        .any(|p| p.ends_with("vbaProject.bin"))
    {
        warnings.insert("Visio macros are not imported or executed.".into());
    }
    if package.entries.contains_key("visio/masters/masters.xml") {
        let masters = xml::parse(package.text("visio/masters/masters.xml")?)?;
        let rels = relationships(&package, "visio/masters/masters.xml")?;
        for master in masters.children("Master") {
            let rel = master
                .child("Rel")
                .ok_or_else(|| error("Missing Visio master relationship."))?;
            let target = rels
                .get(rel.attr("id"))
                .ok_or_else(|| error("Missing Visio master part."))?;
            let content = xml::parse(package.text(target)?)?;
            if resources
                .masters
                .insert(master.attr("ID").into(), content)
                .is_some()
            {
                return Err(error("Duplicate Visio master."));
            }
        }
    }
    let mut scenes = Vec::new();
    let stencil_package = path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ["vssx", "vssm"]
            .iter()
            .any(|ext| e.eq_ignore_ascii_case(ext))
    });
    if (!stencil_package || resources.masters.is_empty()) && package.entries.contains_key("visio/pages/pages.xml") {
        if stencil_package { warnings.insert("This stencil package contains drawing pages instead of masters; its pages were imported.".into()); }
        let pages = xml::parse(package.text("visio/pages/pages.xml")?)?;
        let rels = relationships(&package, "visio/pages/pages.xml")?;
        let mut page_ids = HashSet::new();
        for page in pages.children("Page") {
            if !page_ids.insert(page.attr("ID")) {
                return Err(error("Duplicate Visio page ID."));
            }
            let rel = page
                .child("Rel")
                .ok_or_else(|| error("Missing Visio page relationship."))?;
            let target = rels
                .get(rel.attr("id"))
                .ok_or_else(|| error("Missing Visio page part."))?;
            let content = xml::parse(package.text(target)?)?;
            scenes.push(scene(page, &content, &resources, &mut warnings)?);
        }
    } else if !resources.masters.is_empty() {
        let masters = xml::parse(package.text("visio/masters/masters.xml")?)?;
        for master in masters.children("Master") {
            scenes.push(scene(
                master,
                &resources.masters[master.attr("ID")],
                &resources,
                &mut warnings,
            )?);
        }
        warnings.insert("Visio stencil masters are imported as separate editable pages.".into());
    }
    finish(scenes, warnings)
}
pub(super) fn from_xml(text: &str) -> Result<Imported> {
    let root = xml::parse(text)?;
    if root.name != "VisioDocument" {
        return Err(error("Expected Visio VDX/VSX XML."));
    }
    let mut resources = Resources::from_document(&root)?;
    let mut warnings = BTreeSet::new();
    let mut scenes = Vec::new();
    if let Some(masters) = root.child("Masters") {
        for master in masters.children("Master") {
            if resources
                .masters
                .insert(master.attr("ID").into(), master.clone())
                .is_some()
            {
                return Err(error("Duplicate Visio master."));
            }
        }
    }
    if let Some(pages) = root.child("Pages") {
        for page in pages.children("Page") {
            scenes.push(scene(page, page, &resources, &mut warnings)?);
        }
    } else {
        for master in resources.masters.values() {
            scenes.push(scene(master, master, &resources, &mut warnings)?);
        }
    }
    finish(scenes, warnings)
}
fn scene(
    header: &Xml,
    content: &Xml,
    resources: &Resources,
    warnings: &mut BTreeSet<String>,
) -> Result<Scene> {
    let empty = Xml::default();
    let sheet = header.child("PageSheet").unwrap_or(&empty);
    let w = number(sheet, None, "PageWidth", 8.5)? * DPI;
    let h = number(sheet, None, "PageHeight", 11.)? * DPI;
    if w < 1. || h < 1. || w > 30000. || h > 30000. {
        return Err(error("Invalid Visio page dimensions."));
    }
    let mut scene = Scene {
        name: header.attr("Name").to_string(),
        width: w.ceil() as u32,
        height: h.ceil() as u32,
        ..Default::default()
    };
    let transform = DAffine2::from_cols_array(&[DPI, 0., 0., -DPI, 0., h]);
    let mut connects = HashMap::<String, (Option<String>, Option<String>)>::new();
    if let Some(list) = content.child("Connects") {
        for c in list.children("Connect") {
            let edge = connects.entry(c.attr("FromSheet").into()).or_default();
            let target = c.attr("ToSheet").to_string();
            match c.attr("FromCell") {
                "BeginX" | "BeginY" => edge.0 = Some(target),
                "EndX" | "EndY" => edge.1 = Some(target),
                _ => {
                    warnings
                        .insert("A non-endpoint Visio glue constraint needs manual review.".into());
                }
            }
        }
    }
    let mut pending = Vec::new();
    if let Some(shapes) = content.child("Shapes") {
        for shape in shapes.children("Shape") {
            shape_into(
                shape,
                None,
                transform,
                None,
                "",
                0,
                resources,
                &mut scene,
                &mut pending,
                warnings,
            )?;
        }
    }
    let bounds = scene
        .shapes
        .iter()
        .map(|s| (s.key.clone(), s.bounds))
        .collect::<HashMap<_, _>>();
    for (mut line, start, end) in pending {
        if let Some((Some(source), Some(target))) = connects.get(&line.key) {
            let a = bounds
                .get(source)
                .ok_or_else(|| error("Visio connector source is missing."))?;
            let b = bounds
                .get(target)
                .ok_or_else(|| error("Visio connector target is missing."))?;
            let port = |p: DVec2, r: &[f64; 4]| Port::Custom {
                x: ((p.x - r[0]) / r[2]).clamp(0., 1.),
                y: ((p.y - r[1]) / r[3]).clamp(0., 1.),
            };
            line.source = (source.clone(), port(start, a));
            line.target = (target.clone(), port(end, b));
            scene.lines.push(line);
        } else {
            // An unattached line is still an editable vector; do not invent graph endpoints.
            let points = std::iter::once((start.x, start.y))
                .chain(line.points)
                .chain(std::iter::once((end.x, end.y)))
                .collect::<Vec<_>>();
            let minx = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
            let miny = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
            let maxx = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
            let maxy = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
            let mut s = Shape::new(
                line.key,
                ShapeKind::Process,
                [minx, miny, (maxx - minx).max(1.), (maxy - miny).max(1.)],
                line.label,
            );
            s.name = "Unbound connector".into();
            s.style = line.style;
            s.parent = line.parent;
            s.path = Some(VectorPath {
                subpaths: vec![SubPath {
                    anchors: points.into_iter().map(Anchor::corner).collect(),
                    closed: false,
                }],
            });
            scene.shapes.push(s);
            warnings.insert("Unbound or partially bound Visio lines remain editable vectors; reconnect them to enable automatic routing.".into());
        }
    }
    Ok(scene)
}
#[allow(clippy::too_many_arguments)]
fn shape_into(
    node: &Xml,
    inherited: Option<&Xml>,
    parent_transform: DAffine2,
    parent: Option<String>,
    prefix: &str,
    depth: usize,
    resources: &Resources,
    scene: &mut Scene,
    pending: &mut Vec<(Line, DVec2, DVec2)>,
    warnings: &mut BTreeSet<String>,
) -> Result<()> {
    if depth >= 64 {
        return Err(error("Visio master/group nesting exceeds 64 levels."));
    }
    if scene.shapes.len() + pending.len() >= diagram::MAX_SHAPES + diagram::MAX_EDGES {
        return Err(error("Visio object limit exceeded."));
    }
    let master = resources
        .masters
        .get(node.attr("Master"))
        .and_then(|m| m.child("Shapes"))
        .and_then(|m| m.children("Shape").next())
        .or(inherited);
    let id = node.attr("ID");
    if id.is_empty() {
        return Err(error("Visio shape has no ID."));
    }
    let key = format!("{prefix}{id}");
    if ["LineWeight","LinePattern","FillPattern","LineColorTrans","FillForegndTrans"].iter().any(|key|value(node,key).is_some_and(|v|v.eq_ignore_ascii_case("Themed"))) {
        warnings.insert("Theme-dependent Visio style values without evaluated numbers use inherited values or native defaults.".into());
    }
    let w = number(node, master, "Width", 1.)?;
    let h = number(node, master, "Height", 0.6)?;
    let pin = dvec2(
        number(node, master, "PinX", w / 2.)?,
        number(node, master, "PinY", h / 2.)?,
    );
    let master_w = master
        .map(|m| number(m, None, "Width", w))
        .transpose()?
        .unwrap_or(w);
    let master_h = master
        .map(|m| number(m, None, "Height", h))
        .transpose()?
        .unwrap_or(h);
    let master_scale = dvec2(
        if master_w.abs() > 1e-9 {
            w / master_w
        } else {
            1.
        },
        if master_h.abs() > 1e-9 {
            h / master_h
        } else {
            1.
        },
    );
    let local_cell = |key, extent, scale| -> Result<f64> {
        if value(node, key).is_some() {
            number(node, None, key, extent / 2.)
        } else if let Some(m) = master {
            Ok(number(m, None, key, extent / (2. * scale))? * scale)
        } else {
            Ok(extent / 2.)
        }
    };
    let local = dvec2(
        local_cell("LocPinX", w, master_scale.x)?,
        local_cell("LocPinY", h, master_scale.y)?,
    );
    let angle = number(node, master, "Angle", 0.)?;
    let flip = dvec2(
        if number(node, master, "FlipX", 0.)? != 0. {
            -1.
        } else {
            1.
        },
        if number(node, master, "FlipY", 0.)? != 0. {
            -1.
        } else {
            1.
        },
    );
    let transform = parent_transform
        * DAffine2::from_translation(pin)
        * DAffine2::from_angle(angle)
        * DAffine2::from_scale(flip)
        * DAffine2::from_translation(-local);
    let name = if !node.attr("NameU").is_empty() {
        node.attr("NameU")
    } else {
        master.map(|m| m.attr("NameU")).unwrap_or("Shape")
    };
    let text = node
        .child("Text")
        .or_else(|| master.and_then(|m| m.child("Text")))
        .map(|n| n.text.trim_end_matches('\n').to_string())
        .unwrap_or_default();
    let style_sheet = |key| {
        resources.styles.get(if node.attr(key).is_empty() {
            master.map(|m| m.attr(key)).unwrap_or("")
        } else {
            node.attr(key)
        })
    };
    let inherited_style = |sheet: Option<&Xml>| {
        let mut fallback = master.cloned().unwrap_or_default();
        if let Some(sheet) = sheet {
            fallback.children.extend(sheet.children.iter().cloned());
        }
        fallback
    };
    let line_fallback = inherited_style(style_sheet("LineStyle"));
    let fill_fallback = inherited_style(style_sheet("FillStyle"));
    let line_style = Some(&line_fallback);
    let fill_style = Some(&fill_fallback);
    let mut style = PathStyle {
        fill: Some(resources.color(node, fill_style, "FillForegnd", [255; 4], warnings)),
        stroke: Some(resources.color(node, line_style, "LineColor", [0, 0, 0, 255], warnings)),
        width: (number(node, line_style, "LineWeight", 0.01)? * DPI) as f32,
        ..Default::default()
    };
    if number(node, fill_style, "FillPattern", 1.)? == 0. {
        style.fill = None;
    }
    let line_pattern = number(node, line_style, "LinePattern", 1.)?;
    if line_pattern == 0. {
        style.stroke = None;
    } else if line_pattern != 1. {
        style.dash = [8., 4., 0., 0., 0., 0.];
        style.dash_count = 2;
        if line_pattern != 2. {
            warnings.insert("Visio decorative line patterns use a simple editable dash.".into());
        }
    }
    if let Some(fill) = &mut style.fill {
        fill[3] = ((1. - number(node, fill_style, "FillForegndTrans", 0.)?.clamp(0., 1.)) * 255.)
            .round() as u8;
    }
    if let Some(stroke) = &mut style.stroke {
        stroke[3] = ((1. - number(node, line_style, "LineColorTrans", 0.)?.clamp(0., 1.)) * 255.)
            .round() as u8;
    }
    let geometry = geometry(node, master, w, h, warnings)?;
    let one_d = value(node, "BeginX").is_some()
        || node.attr("Type") == "1D"
        || master.is_some_and(|m| value(m, "BeginX").is_some());
    if one_d {
        let start = parent_transform.transform_point2(dvec2(
            number(node, master, "BeginX", pin.x - w / 2.)?,
            number(node, master, "BeginY", pin.y)?,
        ));
        let end = parent_transform.transform_point2(dvec2(
            number(node, master, "EndX", pin.x + w / 2.)?,
            number(node, master, "EndY", pin.y)?,
        ));
        let points = geometry
            .as_ref()
            .and_then(|p| p.subpaths.first())
            .map(|p| {
                p.anchors
                    .iter()
                    .skip(1)
                    .take(p.anchors.len().saturating_sub(2))
                    .map(|a| {
                        let p = transform.transform_point2(dvec2(a.p.0, a.p.1));
                        (p.x, p.y)
                    })
                    .collect()
            })
            .unwrap_or_default();
        style.fill = None;
        pending.push((
            Line {
                key,
                source: (String::new(), Port::Auto),
                target: (String::new(), Port::Auto),
                label: text,
                style,
                routing: Routing::Straight,
                points,
                start_arrow: number(node, master, "BeginArrow", 0.)? != 0.,
                end_arrow: number(node, master, "EndArrow", 0.)? != 0.,
                parent,
            },
            start,
            end,
        ));
        return Ok(());
    }
    let children = node
        .child("Shapes")
        .or_else(|| master.and_then(|m| m.child("Shapes")));
    let lower = name.to_lowercase();
    let kind = if children.is_some() {
        ShapeKind::Container
    } else if lower.contains("decision") {
        ShapeKind::Decision
    } else if lower.contains("database") {
        ShapeKind::Database
    } else if lower.contains("terminator") || lower.contains("start/end") {
        ShapeKind::Terminator
    } else {
        ShapeKind::Process
    };
    let corners = [dvec2(0., 0.), dvec2(w, 0.), dvec2(w, h), dvec2(0., h)]
        .map(|p| transform.transform_point2(p));
    let min = corners
        .into_iter()
        .fold(DVec2::splat(f64::INFINITY), DVec2::min);
    let max = corners
        .into_iter()
        .fold(DVec2::splat(f64::NEG_INFINITY), DVec2::max);
    let bounds = [
        min.x,
        min.y,
        (max.x - min.x).max(1.),
        (max.y - min.y).max(1.),
    ];
    let mut s = Shape::new(key.clone(), kind, bounds, text);
    s.name = name.to_string();
    s.parent = parent;
    s.style = style;
    if let Some(mut path) = geometry {
        path.transform(transform);
        s.path = Some(path);
    } else if children.is_some() {
        s.style.fill = None;
        s.style.stroke = None;
    } else {
        warnings.insert(
            "Shapes without supported geometry use editable outlines based on their bounds.".into(),
        );
    }
    fn character(n: &Xml) -> Option<&Xml> {
        n.children("Section")
            .find(|n| n.attr("N") == "Character")
            .and_then(|s| s.children("Row").next())
            .or_else(|| n.child("Char"))
    }
    let character = character(node)
        .or_else(|| master.and_then(character))
        .or_else(|| style_sheet("TextStyle").and_then(character));
    if let Some(character) = character {
        s.text.size = (number(character, None, "Size", 14. / DPI)? * DPI).clamp(1., 1000.) as f32;
        s.text.color = resources.color(character, None, "Color", [0, 0, 0, 255], warnings);
        let style = number(character, None, "Style", 0.)? as u32;
        s.text.bold = style & 1 != 0;
        s.text.italic = style & 2 != 0;
        if let Some(font) = value(character, "Font").and_then(|id| resources.fonts.get(id)) {
            s.text.font = font.clone();
        }
    }
    for section in node
        .children("Section")
        .filter(|s| s.attr("N") == "Property")
    {
        for row in section.children("Row") {
            let key = value(row, "Label").unwrap_or(row.attr("N"));
            if !key.is_empty() {
                s.data
                    .insert(key.into(), value(row, "Value").unwrap_or("").into());
            }
        }
    }
    for property in node.children("Prop") {
        let key = value(property, "Label").unwrap_or(property.attr("NameU"));
        if !key.is_empty() {
            s.data
                .insert(key.into(), value(property, "Value").unwrap_or("").into());
        }
    }
    if node.child("ForeignData").is_some() {
        warnings.insert(
            "Embedded Visio OLE/foreign objects use placeholders; their graphics need review."
                .into(),
        );
    }
    if angle != 0. {
        warnings.insert("Rotated outlines are preserved; review label alignment and ports.".into());
    }
    scene.shapes.push(s);
    if let Some(children) = children {
        let inherited_children = node.child("Shapes").is_none();
        let child_prefix = if inherited_children {
            format!("{key}/master/")
        } else {
            prefix.into()
        };
        for child in children.children("Shape") {
            shape_into(
                child,
                master.and_then(|m| m.child("Shapes")).and_then(|shapes| {
                    shapes
                        .children("Shape")
                        .find(|m| m.attr("ID") == child.attr("MasterShape"))
                }),
                if inherited_children {
                    transform * DAffine2::from_scale(master_scale)
                } else {
                    transform
                },
                Some(key.clone()),
                &child_prefix,
                depth + 1,
                resources,
                scene,
                pending,
                warnings,
            )?;
        }
    }
    Ok(())
}
fn geometry(
    node: &Xml,
    master: Option<&Xml>,
    w: f64,
    h: f64,
    warnings: &mut BTreeSet<String>,
) -> Result<Option<VectorPath>> {
    let sections = |n: &Xml| {
        n.children
            .iter()
            .filter(|n| (n.name == "Section" && n.attr("N") == "Geometry") || n.name == "Geom")
            .cloned()
            .collect::<Vec<_>>()
    };
    let mut sections = sections(node);
    let inherited_geometry = sections.is_empty();
    let master_w = master
        .map(|m| number(m, None, "Width", w))
        .transpose()?
        .unwrap_or(w);
    let master_h = master
        .map(|m| number(m, None, "Height", h))
        .transpose()?
        .unwrap_or(h);
    let scale_x = if inherited_geometry && master_w.abs() > 1e-9 {
        w / master_w
    } else {
        1.
    };
    let scale_y = if inherited_geometry && master_h.abs() > 1e-9 {
        h / master_h
    } else {
        1.
    };
    if sections.is_empty() {
        sections = master.map(sections_fn).unwrap_or_default();
    }
    let mut out = VectorPath::default();
    for section in sections {
        if number(&section, None, "NoShow", 0.)? != 0. {
            continue;
        }
        let mut svg = String::new();
        let mut current = dvec2(0., 0.);
        for row in section.children.iter().filter(|n| {
            n.name == "Row"
                || [
                    "MoveTo",
                    "LineTo",
                    "Ellipse",
                    "ArcTo",
                    "EllipticalArcTo",
                    "RelEllipticalArcTo",
                    "PolylineTo",
                    "NURBSTo",
                    "PolyLineTo",
                    "RelMoveTo",
                    "RelLineTo",
                    "RelCubBezTo",
                    "RelQuadBezTo",
                ]
                .contains(&n.name.as_str())
        }) {
            if row.attr("Del") == "1" {
                continue;
            }
            let typ = if row.name == "Row" {
                row.attr("T")
            } else {
                &row.name
            };
            let relative = typ.starts_with("Rel");
            let sx = if relative { w } else { scale_x };
            let sy = if relative { h } else { scale_y };
            let x = number(row, None, "X", 0.)? * sx;
            let y = number(row, None, "Y", 0.)? * sy;
            match typ {
                "MoveTo" | "RelMoveTo" => svg.push_str(&format!("M {x} {y} ")),
                "LineTo" | "RelLineTo" => svg.push_str(&format!("L {x} {y} ")),
                "RelCubBezTo" => svg.push_str(&format!(
                    "C {} {} {} {} {x} {y} ",
                    number(row, None, "A", 0.)? * w,
                    number(row, None, "B", 0.)? * h,
                    number(row, None, "C", 0.)? * w,
                    number(row, None, "D", 0.)? * h
                )),
                "RelQuadBezTo" => svg.push_str(&format!(
                    "Q {} {} {x} {y} ",
                    number(row, None, "A", 0.)? * w,
                    number(row, None, "B", 0.)? * h
                )),
                "ArcTo" | "EllipticalArcTo" | "RelEllipticalArcTo" => {
                    let start=current/dvec2(sx,sy);
                    let end=dvec2(x/sx,y/sy);
                    let (through,angle,ratio)=if typ=="ArcTo" {
                        let bow=number(row,None,"A",0.)?;
                        let delta=end-start;
                        ((start+end)/2.+dvec2(-delta.y,delta.x).normalize_or_zero()*bow,0.,1.)
                    } else {
                        (dvec2(number(row,None,"A",0.)?,number(row,None,"B",0.)?),number(row,None,"C",0.)?,number(row,None,"D",1.)?)
                    };
                    if let Some(curve)=super::visio_curves::ellipse_arc(start,through,end,angle,ratio,dvec2(sx,sy)) {
                        svg.push_str(&curve);
                    } else {svg.push_str(&format!("L {x} {y} "));}
                }
                "NURBSTo" => {
                    let formula=row.children.iter().find(|n|n.name=="Cell"&&n.attr("N")=="E").map(|n|if n.attr("F").is_empty(){n.attr("V")}else{n.attr("F")}).or_else(||row.child("E").map(|n|n.text.trim())).unwrap_or("");
                    let values=super::visio_curves::formula(formula,"NURBS");
                    let ends=[number(row,None,"A",0.)?,number(row,None,"B",1.)?,number(row,None,"C",0.)?,number(row,None,"D",1.)?];
                    if let Some(curve)=values.and_then(|v|super::visio_curves::nurbs(current,dvec2(x,y),&v,ends,dvec2(w,h),dvec2(scale_x,scale_y))) {svg.push_str(&curve);}else{warnings.insert("NURBS has unevaluated or invalid knots; endpoint retained.".into());svg.push_str(&format!("L {x} {y} "));}
                }
                "PolylineTo" | "PolyLineTo" => {
                    let formula=row.children.iter().find(|n|n.name=="Cell"&&n.attr("N")=="A").map(|n|if n.attr("F").is_empty(){n.attr("V")}else{n.attr("F")}).or_else(||row.child("A").map(|n|n.text.trim())).unwrap_or("");
                    if let Some(values)=super::visio_curves::formula(formula,"POLYLINE").filter(|v|v.len()>=2&&v.len()%2==0) {
                        let px=if values[0]==0.{w}else{scale_x};
                        let py=if values[1]==0.{h}else{scale_y};
                        for pair in values[2..].chunks_exact(2) {svg.push_str(&format!("L {} {} ",pair[0]*px,pair[1]*py));}
                    } else {warnings.insert("Polyline formula has unevaluated values; endpoint retained.".into());}
                    svg.push_str(&format!("L {x} {y} "));
                }
                "Ellipse" => {
                    let a = number(row, None, "A", master_w)? * scale_x;
                    let b = number(row, None, "B", y / scale_y)? * scale_y;
                    let c = number(row, None, "C", x / scale_x)? * scale_x;
                    let d = number(row, None, "D", master_h)? * scale_y;
                    let rx = (a - x).hypot(b - y);
                    let ry = (c - x).hypot(d - y);
                    let mut ellipse =
                        emulsion_core::design::Element::Circle.path(-rx, -ry, rx * 2., ry * 2.);
                    ellipse.transform(
                        DAffine2::from_translation(dvec2(x, y))
                            * DAffine2::from_angle((b - y).atan2(a - x)),
                    );
                    out.subpaths.extend(ellipse.subpaths);
                }
                _ => {
                    warnings.insert(format!(
                        "Visio geometry {typ} uses its endpoint as a straight segment."
                    ));
                    if !svg.is_empty() {
                        svg.push_str(&format!("L {x} {y} "));
                    }
                }
            }
            current=dvec2(x,y);
        }
        if !svg.is_empty() {
            let mut path = VectorPath::from_svg(&svg).map_err(|e| error(e.to_string()))?;
            for sub in &mut path.subpaths {
                if sub.anchors.len() > 2
                    && sub.anchors.first().unwrap().p == sub.anchors.last().unwrap().p
                {
                    sub.anchors.pop();
                    sub.closed = true;
                }
            }
            out.subpaths.extend(path.subpaths);
        }
    }
    Ok((!out.subpaths.is_empty()).then_some(out))
}
fn sections_fn(n: &Xml) -> Vec<Xml> {
    n.children
        .iter()
        .filter(|n| (n.name == "Section" && n.attr("N") == "Geometry") || n.name == "Geom")
        .cloned()
        .collect()
}
