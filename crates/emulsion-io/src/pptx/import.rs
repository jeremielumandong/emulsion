use super::*;
use geometry::{color, number, paint, xfrm};
use package::Rel;
fn name(x: &Xml) -> String {
    x.descendants("cNvPr")
        .next()
        .map(|x| x.attr("name").to_string())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| x.name.clone())
}
fn add(doc: &mut Document, mut n: Node, parent: Option<NodeId>) -> Result<NodeId> {
    if doc.nodes.len() >= MAX_OBJECTS {
        return Err(error("Slide exceeds 16,384 native objects"));
    }
    let id = doc.next_id;
    doc.next_id += 1;
    n.id = id;
    n.parent = parent;
    doc.nodes.push(n);
    Ok(id)
}
fn default_theme() -> BTreeMap<String, [u8; 4]> {
    [
        ("dk1", [0, 0, 0, 255]),
        ("lt1", [255; 4]),
        ("dk2", [31, 73, 125, 255]),
        ("lt2", [238, 236, 225, 255]),
        ("accent1", [79, 129, 189, 255]),
        ("accent2", [192, 80, 77, 255]),
        ("accent3", [155, 187, 89, 255]),
        ("accent4", [128, 100, 162, 255]),
        ("accent5", [75, 172, 198, 255]),
        ("accent6", [247, 150, 70, 255]),
        ("hlink", [0, 0, 255, 255]),
        ("folHlink", [128, 0, 128, 255]),
        ("bg1", [255; 4]),
        ("tx1", [0, 0, 0, 255]),
        ("bg2", [238, 236, 225, 255]),
        ("tx2", [31, 73, 125, 255]),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v))
    .collect()
}
fn related(package: &Package, part: &str, kind: &str) -> Result<Option<String>> {
    Ok(package
        .rels(part)?
        .values()
        .find(|r| r.kind.ends_with(kind) && !r.external)
        .map(|r| r.target.clone()))
}
pub fn read(path: &Path) -> Result<Imported> {
    let package = Package::read(path)?;
    let root = package.rels("")?;
    let main = root
        .values()
        .find(|r| r.kind.ends_with("/officeDocument") && !r.external)
        .map_or("ppt/presentation.xml", |r| r.target.as_str());
    let pres = package.xml(main)?;
    if pres.name != "presentation" {
        return Err(error("Not a PresentationML package"));
    }
    let size = pres
        .child("sldSz")
        .ok_or_else(|| error("Presentation has no slide size"))?;
    let width = (number(size, "cx", 0.) / EMU).round() as u32;
    let height = (number(size, "cy", 0.) / EMU).round() as u32;
    crate::import::check_size(width, height)?;
    let rels = package.rels(main)?;
    let ids = pres
        .child("sldIdLst")
        .ok_or_else(|| error("Presentation has no slide list"))?;
    let mut slides = Vec::new();
    let mut seen = HashSet::new();
    for s in ids.children("sldId") {
        let rel = rels
            .get(s.attr("r:id"))
            .ok_or_else(|| error("Missing slide relationship"))?;
        if rel.external || !rel.kind.ends_with("/slide") {
            return Err(error("Invalid slide relationship"));
        }
        if !seen.insert(&rel.target) {
            return Err(error("Duplicate slide part"));
        }
        slides.push(rel.target.clone());
    }
    if slides.is_empty() || slides.len() > emulsion_core::project::MAX_PAGES {
        return Err(error("PPTX must contain 1–100 slides"));
    }
    if u64::from(width) * u64::from(height) * slides.len() as u64
        > emulsion_core::project::MAX_PROJECT_PIXELS
    {
        return Err(error("Presentation exceeds total page area limit"));
    }
    let slide_ids = slides
        .iter()
        .enumerate()
        .map(|(i, s)| (s.clone(), i as u64 + 1))
        .collect::<HashMap<_, _>>();
    let mut warnings = Vec::new();
    let mut pages = Vec::new();
    let mut image_pixels = 0u64;
    for (index, part) in slides.iter().enumerate() {
        let xml = package.xml(part)?;
        let relationships = package.rels(part)?;
        let layout = related(&package, part, "/slideLayout")?;
        let master = if let Some(layout) = &layout {
            related(&package, layout, "/slideMaster")?
        } else {
            None
        };
        let mut theme = default_theme();
        if let Some(master) = &master
            && let Some(t) = related(&package, master, "/theme")?
        {
            let t = package.xml(&t)?;
            if let Some(colors) = t.descendants("clrScheme").next() {
                for c in &colors.children {
                    if let Some(value) = color(c, &theme) {
                        theme.insert(c.name.clone(), value);
                    }
                }
            }
        }
        for (alias, key) in [
            ("bg1", "lt1"),
            ("bg2", "lt2"),
            ("tx1", "dk1"),
            ("tx2", "dk2"),
        ] {
            theme.insert(alias.into(), theme[key]);
        }
        let mut doc = Document::new(width, height);
        doc.resolution = 96.;
        let master_xml = master.as_ref().map(|p| package.xml(p)).transpose()?;
        let layout_xml = layout.as_ref().map(|p| package.xml(p)).transpose()?;
        let bg = xml
            .child("cSld")
            .and_then(|x| x.child("bg"))
            .or_else(|| {
                layout_xml
                    .as_ref()
                    .and_then(|x| x.child("cSld")?.child("bg"))
            })
            .or_else(|| {
                master_xml
                    .as_ref()
                    .and_then(|x| x.child("cSld")?.child("bg"))
            });
        let background = bg
            .and_then(|x| x.descendants("solidFill").find_map(|x| color(x, &theme)))
            .unwrap_or([255; 4]);
        add(
            &mut doc,
            Node::new(0, "Slide background", NodeKind::Fill { rgba: background }),
            None,
        )?;
        let mut ctx = Context {
            package: &package,
            theme: &theme,
            slides: &slide_ids,
            warnings: &mut warnings,
            slide: index + 1,
            image_pixels: &mut image_pixels,
        };
        // Master/layout decorations become independently editable objects. Placeholders
        // are inherited into matching slide shapes instead of duplicating prompt text.
        if xml.attr("showMasterSp") != "0" {
            for (part, xml) in [
                (master.as_ref(), master_xml.as_ref()),
                (layout.as_ref(), layout_xml.as_ref()),
            ] {
                if let (Some(part), Some(xml)) = (part, xml) {
                    let rels = package.rels(part)?;
                    if let Some(tree) = xml.child("cSld").and_then(|x| x.child("spTree")) {
                        ctx.objects(tree, &mut doc, None, DAffine2::IDENTITY, &rels, None, true)?;
                    }
                }
            }
        }
        let tree = xml
            .child("cSld")
            .and_then(|x| x.child("spTree"))
            .ok_or_else(|| error("Slide has no shape tree"))?;
        let inherited = layout_xml
            .as_ref()
            .and_then(|x| x.child("cSld")?.child("spTree"));
        ctx.objects(
            tree,
            &mut doc,
            None,
            DAffine2::IDENTITY,
            &relationships,
            inherited,
            false,
        )?;
        if let Some(notes) = related(&package, part, "/notesSlide")? {
            let notes = package.xml(&notes)?;
            let mut values = Vec::new();
            for shape in notes.descendants("sp") {
                if shape
                    .descendants("ph")
                    .any(|p| matches!(p.attr("type"), "sldNum" | "sldImg" | "dt" | "hdr" | "ftr"))
                {
                    continue;
                }
                if let Some(body) = shape.child("txBody") {
                    for p in body.children("p") {
                        values.push(
                            p.descendants("t")
                                .map(|t| t.text.as_str())
                                .collect::<String>(),
                        );
                    }
                }
            }
            let text = values.join("\n");
            if text.chars().count() > 20000 {
                return Err(error("Slide notes exceed 20,000 characters"));
            }
            doc.design.speaker_notes = text;
        }
        if xml.child("timing").is_some() {
            warn(
                &mut warnings,
                index + 1,
                "Slide",
                "PowerPoint animation timelines are not imported; objects remain editable",
            );
        }
        if let Some(t) = xml.child("transition") {
            doc.design.page_transition = if t.child("fade").is_some() {
                emulsion_core::design_metadata::PageTransition::Fade
            } else {
                warn(
                    &mut warnings,
                    index + 1,
                    "Slide",
                    "Unsupported slide transition omitted",
                );
                Default::default()
            };
        }
        doc.normalize();
        doc.validate()?;
        let slide_name = xml.child("cSld").map_or("", |x| x.attr("name"));
        let slide_name = if slide_name.trim().is_empty() {
            format!("Slide {}", index + 1)
        } else {
            slide_name
                .chars()
                .filter(|c| !c.is_control())
                .take(200)
                .collect()
        };
        pages.push(ProjectPage {
            meta: PageMeta {
                id: index as u64 + 1,
                name: slide_name,
                bleed_mm: 0.,
            },
            graph: Graph::new(doc.clone(), "Imported PowerPoint slide"),
            doc,
        });
    }
    let project = Project {
        storyboard: None,
        kind: ProjectKind::Design,
        active: 1,
        next_page_id: pages.len() as u64 + 1,
        pages,
    };
    project.validate().map_err(error)?;
    Ok(Imported { project, warnings })
}
struct Context<'a> {
    package: &'a Package,
    theme: &'a BTreeMap<String, [u8; 4]>,
    slides: &'a HashMap<String, u64>,
    warnings: &'a mut Vec<String>,
    slide: usize,
    image_pixels: &'a mut u64,
}
impl Context<'_> {
    #[allow(clippy::too_many_arguments)]
    fn objects(
        &mut self,
        tree: &Xml,
        doc: &mut Document,
        parent: Option<NodeId>,
        m: DAffine2,
        rels: &BTreeMap<String, Rel>,
        inherited: Option<&Xml>,
        skip_placeholders: bool,
    ) -> Result<()> {
        for original in &tree.children {
            if matches!(original.name.as_str(), "nvGrpSpPr" | "grpSpPr" | "extLst") {
                continue;
            }
            if skip_placeholders && original.descendants("ph").next().is_some() {
                continue;
            }
            let mut object = original.clone();
            let placeholder = object.descendants("ph").next().cloned();
            if let Some(ph) = placeholder
                && let Some(base) = inherited.and_then(|t| {
                    t.children.iter().find(|s| {
                        s.descendants("ph").any(|p| {
                            p.attr("idx") == ph.attr("idx")
                                && (p.attr("type") == ph.attr("type") || ph.attr("type").is_empty())
                        })
                    })
                })
                && object.child("spPr").and_then(|x| x.child("xfrm")).is_none()
                && let Some(base) = base.child("spPr").and_then(|x| x.child("xfrm"))
                && let Some(sp) = object.children.iter_mut().find(|x| x.name == "spPr")
            {
                sp.children.insert(0, base.clone());
            }
            let label = name(&object);
            if doc.nodes.len() >= MAX_OBJECTS {
                return Err(error("Slide exceeds 16,384 native objects"));
            }
            let old_len = doc.nodes.len();
            let old_next = doc.next_id;
            if let Err(e) = self.object(&object, doc, parent, m, rels) {
                doc.nodes.truncate(old_len);
                doc.next_id = old_next;
                doc.design.interactions.retain(|id, _| *id < old_next);
                warn(
                    self.warnings,
                    self.slide,
                    &label,
                    format!("Object could not be imported: {e}"),
                );
            }
        }
        Ok(())
    }
    fn object(
        &mut self,
        x: &Xml,
        doc: &mut Document,
        parent: Option<NodeId>,
        m: DAffine2,
        rels: &BTreeMap<String, Rel>,
    ) -> Result<()> {
        let label = name(x);
        for properties in [x.child("spPr"), x.child("grpSpPr")].into_iter().flatten() {
            for feature in ["effectLst", "effectDag", "scene3d", "sp3d"] {
                if properties
                    .child(feature)
                    .is_some_and(|v| !v.children.is_empty())
                {
                    warn(
                        self.warnings,
                        self.slide,
                        &label,
                        format!(
                            "{feature} appearance is unsupported; editable base artwork retained"
                        ),
                    );
                }
            }
        }
        if x.name == "grpSp" {
            let (g, _) = xfrm(x.child("grpSpPr").and_then(|x| x.child("xfrm")), true)?;
            let id = add(doc, Node::group(0, &label), parent)?;
            self.objects(x, doc, Some(id), m * g, rels, None, false)?;
            self.link(x, doc, id, rels);
            return Ok(());
        }
        if !matches!(x.name.as_str(), "sp" | "cxnSp" | "pic" | "graphicFrame") {
            warn(
                self.warnings,
                self.slide,
                &label,
                format!("Unsupported {} object omitted", x.name),
            );
            return Ok(());
        }
        let sp = x.child("spPr");
        let transform = sp.and_then(|x| x.child("xfrm")).or_else(|| x.child("xfrm"));
        let (local, size) = xfrm(transform, false)?;
        let matrix = m * local;
        if x.name == "pic" {
            let blip = x
                .descendants("blip")
                .next()
                .ok_or_else(|| error("Picture has no image"))?;
            if blip
                .children
                .iter()
                .any(|effect| !matches!(effect.name.as_str(), "alphaModFix" | "extLst"))
            {
                warn(
                    self.warnings,
                    self.slide,
                    &label,
                    "Picture color/effect filters are unsupported; original pixels retained",
                );
            }
            let relation = rels
                .get(blip.attr("r:embed"))
                .ok_or_else(|| error("Linked/external picture is not fetched"))?;
            if relation.external {
                return Err(error("External pictures are not fetched"));
            }
            let bytes = self.package.bytes(&relation.target)?;
            let reader =
                image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
            let (w, h) = reader.into_dimensions()?;
            *self.image_pixels = self
                .image_pixels
                .checked_add(u64::from(w) * u64::from(h))
                .filter(|pixels| *pixels <= 64_000_000)
                .ok_or_else(|| error("Embedded images exceed 64 million pixels"))?;
            let image = crate::import::import_bytes(&label, bytes)?;
            let NodeKind::Raster { raster, .. } = &image.nodes[0].kind else {
                return Err(error("Unsupported embedded image"));
            };
            let crop = x.descendants("srcRect").next();
            let [l, t, r, b] = ["l", "t", "r", "b"].map(|k| {
                crop.map_or(0., |x| number(x, k, 0.) / 100000.)
                    .clamp(0., 0.999)
            });
            if l + r >= 1. || t + b >= 1. {
                return Err(error("Invalid image crop"));
            }
            let sx = size.0 / (f64::from(w) * (1. - l - r));
            let sy = size.1 / (f64::from(h) * (1. - t - b));
            let placement = Placement {
                x: -f64::from(w) * l * sx,
                y: -f64::from(h) * t * sy,
                scale_x: sx,
                scale_y: sy,
                ..Default::default()
            };
            let mut node = Node::raster(0, &label, raster.clone(), placement);
            node.opacity = blip.child("alphaModFix").map_or(1., |x| {
                (number(x, "amt", 100000.) / 100000.).clamp(0., 1.) as f32
            });
            if l + t + r + b > 0. {
                node.mask = Some(Arc::new(emulsion_raster::Mask::from_fn(w, h, 0, |x, y| {
                    if f64::from(x) >= l * f64::from(w)
                        && f64::from(x) < (1. - r) * f64::from(w)
                        && f64::from(y) >= t * f64::from(h)
                        && f64::from(y) < (1. - b) * f64::from(h)
                    {
                        255
                    } else {
                        0
                    }
                })));
            }
            let id = add(doc, node, parent)?;
            emulsion_core::transform::transform_nodes(doc, &[id], matrix.to_cols_array())
                .map_err(|e| error(e.to_string()))?;
            self.link(x, doc, id, rels);
            return Ok(());
        }
        if x.name == "graphicFrame" {
            if let Some(table) = x.descendants("tbl").next() {
                return self.table(table, doc, parent, matrix, size, &label);
            }
            let mut spec = emulsion_core::text::TextSpec {
                text: format!("Unsupported PowerPoint object: {label}"),
                size: 16.,
                width: Some(size.0.max(1.) as f32),
                height: Some(size.1.max(1.) as f32),
                ..Default::default()
            };
            let origin = matrix.transform_point2(dvec2(0., 0.));
            spec.x = origin.x as f32;
            spec.y = origin.y as f32;
            add(
                doc,
                Node::text(0, &label, spec, doc.width, doc.height),
                parent,
            )?;
            warn(
                self.warnings,
                self.slide,
                &label,
                "Chart/SmartArt/OLE object imported as a labeled editable placeholder; original appearance is unsupported",
            );
            return Ok(());
        }
        let sp = sp.ok_or_else(|| error("Shape properties missing"))?;
        let (mut path, note) = geometry::shape(sp, size)?;
        if let Some(note) = note {
            warn(self.warnings, self.slide, &label, note);
        }
        path.transform(matrix);
        let (mut fill, fill_paint) = paint(sp, self.theme);
        let line = sp.child("ln");
        let (mut stroke, stroke_paint) =
            line.map_or((None, PathPaint::Solid), |x| paint(x, self.theme));
        if let Some(style) = x.child("style") {
            if fill.is_none() && sp.child("noFill").is_none() {
                fill = style.child("fillRef").and_then(|x| color(x, self.theme));
            }
            if stroke.is_none() && line.is_none() {
                stroke = style.child("lnRef").and_then(|x| color(x, self.theme));
            }
        }
        let style = PathStyle {
            fill,
            fill_paint,
            stroke,
            stroke_paint,
            width: line.map_or(1., |x| (number(x, "w", EMU) / EMU) as f32)
                * matrix.matrix2.determinant().abs().sqrt() as f32,
            ..Default::default()
        };
        let body = x.child("txBody");
        let visible_shape = style.fill.is_some() || style.stroke.is_some();
        let group = if visible_shape && body.is_some() {
            Some(add(doc, Node::group(0, &label), parent)?)
        } else {
            parent
        };
        let mut main = None;
        if visible_shape {
            main = Some(add(
                doc,
                Node::path(0, &label, Arc::new(path), style, doc.width, doc.height),
                group,
            )?);
        }
        if let Some(body) = body {
            let spec = text::decode(body, size, matrix, self.theme)?;
            let id = add(
                doc,
                Node::text(0, &label, spec, doc.width, doc.height),
                group,
            )?;
            main = Some(id);
        }
        if let Some(id) = if group != parent { group } else { main } {
            self.link(x, doc, id, rels);
            if x.descendants("cNvPr").any(|n| n.attr("hidden") == "1") {
                doc.node_mut(id).unwrap().visible = false;
            }
        }
        for feature in ["blipFill", "pattFill"] {
            if x.descendants(feature).any(|v| !v.children.is_empty()) {
                warn(
                    self.warnings,
                    self.slide,
                    &label,
                    format!("{feature} appearance is unsupported; editable base artwork retained"),
                );
            }
        }
        Ok(())
    }
    fn link(&mut self, x: &Xml, doc: &mut Document, id: NodeId, rels: &BTreeMap<String, Rel>) {
        // A group's descendants include its children's hyperlinks. Only this
        // object's nonvisual properties and text body may assign its action.
        let links = x
            .children
            .iter()
            .filter(|part| {
                matches!(
                    part.name.as_str(),
                    "nvSpPr"
                        | "nvCxnSpPr"
                        | "nvPicPr"
                        | "nvGrpSpPr"
                        | "nvGraphicFramePr"
                        | "txBody"
                )
            })
            .flat_map(|part| part.descendants("hlinkClick"))
            .collect::<Vec<_>>();
        let Some(link) = links.first() else {
            return;
        };
        let target = |link: &Xml| rels.get(link.attr("r:id")).map(|r| (&r.target, r.external));
        if links.iter().skip(1).any(|other| {
            other.attr("action") != link.attr("action") || target(other) != target(link)
        }) {
            warn(
                self.warnings,
                self.slide,
                &name(x),
                "Multiple run hyperlinks reduced to the first object-level hyperlink",
            );
        }
        let action = match link.attr("action") {
            "ppaction://hlinkshowjump?jump=nextslide" => {
                Some(emulsion_core::design_interactions::Action::Next)
            }
            "ppaction://hlinkshowjump?jump=previousslide" => {
                Some(emulsion_core::design_interactions::Action::Previous)
            }
            _ => rels.get(link.attr("r:id")).and_then(|r| {
                if r.external {
                    if emulsion_core::design_interactions::valid_url(&r.target) {
                        Some(emulsion_core::design_interactions::Action::Url {
                            url: r.target.clone(),
                        })
                    } else {
                        warn(
                            self.warnings,
                            self.slide,
                            &name(x),
                            format!(
                                "Unsupported external hyperlink retained in this diagnostic: {}",
                                r.target
                            ),
                        );
                        None
                    }
                } else {
                    self.slides.get(&r.target).map(|page| {
                        emulsion_core::design_interactions::Action::Slide { page: *page }
                    })
                }
            }),
        };
        if let Some(action) = action {
            doc.design.interactions.insert(id, vec![action]);
        }
    }
    fn table(
        &mut self,
        table: &Xml,
        doc: &mut Document,
        parent: Option<NodeId>,
        m: DAffine2,
        size: (f64, f64),
        label: &str,
    ) -> Result<()> {
        let group = add(doc, Node::group(0, label), parent)?;
        let columns = table
            .child("tblGrid")
            .map(|g| {
                g.children("gridCol")
                    .map(|c| number(c, "w", EMU) / EMU)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let rows = table.children("tr").collect::<Vec<_>>();
        if columns.is_empty() || columns.len() > 64 || rows.len() > 256 {
            return Err(error("Table dimensions unsupported"));
        }
        let mut y = 0.;
        for row in rows {
            let height = number(row, "h", EMU * 24.) / EMU;
            let mut x = 0.;
            for (index, cell) in row.children("tc").enumerate() {
                let width = *columns
                    .get(index)
                    .ok_or_else(|| error("Invalid table grid"))?;
                let tp = cell.child("tcPr");
                let (fill, fill_paint) =
                    tp.map_or((None, PathPaint::Solid), |p| paint(p, self.theme));
                let mut path = vector_geometry::rectangle(x, y, width, height);
                path.transform(m);
                add(
                    doc,
                    Node::path(
                        0,
                        format!("{label} cell"),
                        Arc::new(path),
                        PathStyle {
                            fill,
                            fill_paint,
                            stroke: Some([180, 180, 180, 255]),
                            width: 1.,
                            ..Default::default()
                        },
                        doc.width,
                        doc.height,
                    ),
                    Some(group),
                )?;
                if let Some(body) = cell.child("txBody") {
                    let spec = text::decode(
                        body,
                        (width, height),
                        m * DAffine2::from_translation(dvec2(x, y)),
                        self.theme,
                    )?;
                    add(
                        doc,
                        Node::text(0, format!("{label} cell text"), spec, doc.width, doc.height),
                        Some(group),
                    )?;
                }
                x += width;
            }
            y += height;
        }
        let _ = size;
        warn(
            self.warnings,
            self.slide,
            label,
            "Table imported as editable grouped cells; table formulas, merge semantics and theme cell styles are not retained",
        );
        Ok(())
    }
}
