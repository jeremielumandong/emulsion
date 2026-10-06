use super::*;
use std::fmt::Write as _;
use std::io::Write;
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PKG: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
fn xml(body: impl AsRef<str>) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>{}",
        body.as_ref()
    )
    .into_bytes()
}
fn emu(v: f64) -> i64 {
    (v * EMU).round() as i64
}
fn xfrm(x: f64, y: f64, w: f64, h: f64, rot: f64, flip: (bool, bool)) -> String {
    format!(
        "<a:xfrm rot=\"{}\" flipH=\"{}\" flipV=\"{}\"><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm>",
        (rot.rem_euclid(360.) * 60000.).round() as i64,
        u8::from(flip.0),
        u8::from(flip.1),
        emu(x),
        emu(y),
        emu(w.max(0.01)),
        emu(h.max(0.01))
    )
}
fn group_xfrm(w: u32, h: u32) -> String {
    format!(
        "<a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{}\" cy=\"{}\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"{}\" cy=\"{}\"/></a:xfrm>",
        emu(w as f64),
        emu(h as f64),
        emu(w as f64),
        emu(h as f64)
    )
}
fn root_tree(w: u32, h: u32) -> String {
    format!(
        "<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr>{}</p:grpSpPr>",
        group_xfrm(w, h)
    )
}
fn rel_xml(rels: &[(String, String, String, bool)]) -> Vec<u8> {
    xml(format!(
        "<Relationships xmlns=\"{PKG}\">{}</Relationships>",
        rels.iter()
            .map(|(id, kind, target, external)| format!(
                "<Relationship Id=\"{}\" Type=\"{R}/{kind}\" Target=\"{}\"{}/>",
                escaped(id),
                escaped(target),
                if *external {
                    " TargetMode=\"External\""
                } else {
                    ""
                }
            ))
            .collect::<String>()
    ))
}
fn geometry(path: &VectorPath) -> Result<(String, (f64, f64, f64, f64))> {
    let bounds = vector_geometry::bounds(path).ok_or_else(|| error("Empty path"))?;
    let (x, y, w, h) = bounds;
    let mut d = String::new();
    for sub in &path.subpaths {
        let Some(first) = sub.anchors.first() else {
            continue;
        };
        let pt = |p: (f64, f64)| format!("<a:pt x=\"{}\" y=\"{}\"/>", emu(p.0 - x), emu(p.1 - y));
        d.push_str(&format!("<a:moveTo>{}</a:moveTo>", pt(first.p)));
        for pair in sub.anchors.windows(2) {
            if pair[0].h_out != pair[0].p || pair[1].h_in != pair[1].p {
                d.push_str(&format!(
                    "<a:cubicBezTo>{}{}{}</a:cubicBezTo>",
                    pt(pair[0].h_out),
                    pt(pair[1].h_in),
                    pt(pair[1].p)
                ));
            } else {
                d.push_str(&format!("<a:lnTo>{}</a:lnTo>", pt(pair[1].p)));
            }
        }
        if sub.closed {
            let last = sub.anchors.last().unwrap();
            if last.h_out != last.p || first.h_in != first.p {
                d.push_str(&format!(
                    "<a:cubicBezTo>{}{}{}</a:cubicBezTo>",
                    pt(last.h_out),
                    pt(first.h_in),
                    pt(first.p)
                ));
            }
            d.push_str("<a:close/>");
        }
    }
    Ok((
        format!(
            "<a:custGeom><a:avLst/><a:gdLst/><a:ahLst/><a:cxnLst/><a:rect l=\"0\" t=\"0\" r=\"r\" b=\"b\"/><a:pathLst><a:path w=\"{}\" h=\"{}\">{d}</a:path></a:pathLst></a:custGeom>",
            emu(w.max(0.01)),
            emu(h.max(0.01))
        ),
        bounds,
    ))
}
struct Slide<'a> {
    doc: &'a Document,
    index: usize,
    pages: &'a HashMap<u64, usize>,
    report: &'a mut Report,
    rels: Vec<(String, String, String, bool)>,
    media: &'a mut BTreeMap<String, Vec<u8>>,
    serial: u32,
}
impl Slide<'_> {
    fn relation(&mut self, kind: &str, target: String, external: bool) -> String {
        let id = format!("rId{}", self.rels.len() + 1);
        self.rels.push((id.clone(), kind.into(), target, external));
        id
    }
    fn props(&mut self, node: &Node) -> String {
        self.serial += 1;
        let mut link = String::new();
        if let Some(actions) = self.doc.design.interactions.get(&node.id) {
            for action in actions {
                use emulsion_core::design_interactions::Action;
                match action {
                    Action::Url { url } => {
                        let id = self.relation("hyperlink", url.clone(), true);
                        link = format!("<a:hlinkClick r:id=\"{id}\"/>");
                        break;
                    }
                    Action::Slide { page } => {
                        if let Some(index) = self.pages.get(page) {
                            let id = self.relation("slide", format!("slide{index}.xml"), false);
                            link = format!(
                                "<a:hlinkClick r:id=\"{id}\" action=\"ppaction://hlinksldjump\"/>"
                            );
                        } else {
                            warn(
                                &mut self.report.warnings,
                                self.index,
                                &node.name,
                                "Hyperlink destination is outside selected slides",
                            );
                        }
                        break;
                    }
                    Action::Next | Action::Previous => {
                        link = format!(
                            "<a:hlinkClick action=\"ppaction://hlinkshowjump?jump={}\"/>",
                            if matches!(action, Action::Next) {
                                "nextslide"
                            } else {
                                "previousslide"
                            }
                        );
                        break;
                    }
                    _ => warn(
                        &mut self.report.warnings,
                        self.index,
                        &node.name,
                        "Overlay/variant/back action has no portable PowerPoint equivalent",
                    ),
                }
            }
        }
        format!(
            "<p:cNvPr id=\"{}\" name=\"{}\">{link}</p:cNvPr>",
            self.serial,
            escaped(&node.name)
        )
    }
    fn nodes(&mut self, parent: Option<NodeId>, opacity: f32) -> Result<String> {
        let mut out = String::new();
        for id in self.doc.children(parent) {
            let n = self.doc.node(id).unwrap();
            if !n.visible {
                continue;
            }
            out.push_str(&self.node(n, opacity * n.opacity)?);
        }
        Ok(out)
    }
    fn node(&mut self, node: &Node, opacity: f32) -> Result<String> {
        self.report.objects += 1;
        let props = self.props(node);
        if node.has_mask()
            || node.clip_to.is_some()
            || !node.styles.is_empty()
            || node.blending != Default::default()
            || !matches!(
                node.blend,
                emulsion_raster::BlendMode::Normal | emulsion_raster::BlendMode::PassThrough
            )
        {
            warn(
                &mut self.report.warnings,
                self.index,
                &node.name,
                "Layer masks/clipping, blend modes or layer effects are not portable; editable base object exported",
            );
        }
        if self.doc.design.frames.contains_key(&node.id) {
            warn(
                &mut self.report.warnings,
                self.index,
                &node.name,
                "Responsive layout/clip rules export at the current page size as editable artwork",
            );
        }
        if self.doc.design.media.contains_key(&node.id)
            || self.doc.design.local_media.contains_key(&node.id)
        {
            warn(
                &mut self.report.warnings,
                self.index,
                &node.name,
                "Embedded media exports its current poster artwork; playback is not embedded",
            );
        }
        let design = &self.doc.design;
        if design.component_links.contains_key(&node.id)
            || design.style_links.contains_key(&node.id)
            || design.variable_bindings.contains_key(&node.id)
            || design.data_bindings.contains_key(&node.id)
            || design.charts.contains_key(&node.id)
        {
            warn(
                &mut self.report.warnings,
                self.index,
                &node.name,
                "Component/style/data/chart links export as current editable artwork; live source links are not retained",
            );
        }
        if node.is_group() && node.opacity < 1. {
            warn(
                &mut self.report.warnings,
                self.index,
                &node.name,
                "Group opacity is applied to editable children; overlapping transparency can differ",
            );
        }
        let multiply = |c: Option<[u8; 4]>| {
            c.map(|mut c| {
                c[3] = (f32::from(c[3]) * opacity).round() as u8;
                c
            })
        };
        match &node.kind {
            NodeKind::Group { .. } => Ok(format!(
                "<p:grpSp><p:nvGrpSpPr>{props}<p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr>{}</p:grpSpPr>{}</p:grpSp>",
                group_xfrm(self.doc.width, self.doc.height),
                self.nodes(Some(node.id), opacity)?
            )),
            NodeKind::Text { spec, .. } => {
                if spec.warp.style != emulsion_core::text_effects::WarpStyle::None
                    || spec.text_path.is_some()
                    || spec.vertical
                    || (spec.scale_x.abs() - spec.scale_y.abs()).abs() > 0.001
                {
                    warn(
                        &mut self.report.warnings,
                        self.index,
                        &node.name,
                        "Text path/warp/vertical or nonuniform glyph scaling normalized to an editable horizontal text box",
                    );
                }
                let mut spec = (**spec).clone();
                spec.color = multiply(Some(spec.color)).unwrap();
                for run in &mut spec.runs {
                    run.style.color = multiply(Some(run.style.color)).unwrap();
                }
                let b = emulsion_core::text::layout(&spec).bounds();
                let (w, h) = (
                    f64::from(spec.width.unwrap_or(b.width)) * f64::from(spec.scale_x.abs()),
                    f64::from(spec.height.unwrap_or(b.height)) * f64::from(spec.scale_y.abs()),
                );
                let center = dvec2(w / 2., h / 2.);
                let shift = glam::DMat2::from_angle(f64::from(spec.rotation).to_radians()) * center
                    - center;
                Ok(format!(
                    "<p:sp><p:nvSpPr>{props}<p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr><p:spPr>{}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/><a:ln><a:noFill/></a:ln></p:spPr>{}</p:sp>",
                    xfrm(
                        f64::from(spec.x) + shift.x,
                        f64::from(spec.y) + shift.y,
                        w,
                        h,
                        f64::from(spec.rotation),
                        (spec.scale_x < 0., spec.scale_y < 0.)
                    ),
                    text::encode(&spec)
                ))
            }
            NodeKind::Path { path, style, .. } => {
                self.path_xml(node, &props, path, *style, opacity)
            }
            NodeKind::Fill { rgba } => self.path_xml(
                node,
                &props,
                &vector_geometry::rectangle(
                    0.,
                    0.,
                    f64::from(self.doc.width),
                    f64::from(self.doc.height),
                ),
                PathStyle {
                    fill: Some(*rgba),
                    stroke: None,
                    ..Default::default()
                },
                opacity,
            ),
            NodeKind::Raster { raster, placement } => {
                self.image_xml(&props, raster, *placement, opacity)
            }
            NodeKind::Strokes { cache, .. } => {
                warn(
                    &mut self.report.warnings,
                    self.index,
                    &node.name,
                    "Vector strokes export as a picture; the strokes are not editable",
                );
                self.image_xml(&props, cache.pixels(), Placement::default(), opacity)
            }
            NodeKind::Smart {
                source,
                placement,
                cache,
                offset,
                ..
            } => {
                warn(
                    &mut self.report.warnings,
                    self.index,
                    &node.name,
                    "Smart Object exports its rendered filter appearance; Smart editability is not retained",
                );
                let pixels = emulsion_core::smart_filter_mask::effective_pixels_with_space(
                    node,
                    self.doc.blend_space,
                )?
                .ok_or_else(|| error("Smart node has no effective pixels"))?;
                let placement = emulsion_core::smart::cache_placement(
                    &placement.require_legacy("Editable PowerPoint export")?,
                    (source.width(), source.height()),
                    (cache.width(), cache.height()),
                    *offset,
                );
                self.image_xml(&props, &pixels, placement, opacity)
            }
            NodeKind::Adjust(_) => {
                warn(
                    &mut self.report.warnings,
                    self.index,
                    &node.name,
                    "Adjustment layer omitted; it cannot be represented as editable PowerPoint artwork",
                );
                Ok(String::new())
            }
        }
    }
    fn path_xml(
        &mut self,
        node: &Node,
        props: &str,
        path: &VectorPath,
        mut style: PathStyle,
        opacity: f32,
    ) -> Result<String> {
        if path.is_empty() {
            warn(
                &mut self.report.warnings,
                self.index,
                &node.name,
                "Empty path omitted",
            );
            return Ok(String::new());
        }
        if style.alignment != emulsion_raster::vector::StrokeAlignment::Center || style.even_odd {
            warn(
                &mut self.report.warnings,
                self.index,
                &node.name,
                "Stroke alignment/even-odd winding approximated by DrawingML's centered nonzero path",
            );
        }
        if matches!(style.fill_paint, PathPaint::Pattern { .. })
            || matches!(style.stroke_paint, PathPaint::Pattern { .. })
        {
            warn(
                &mut self.report.warnings,
                self.index,
                &node.name,
                "Pattern paint exported as its primary solid color",
            );
        }
        for (base, paint) in [
            (style.fill, &mut style.fill_paint),
            (style.stroke, &mut style.stroke_paint),
        ] {
            if let Some(base) = base
                && let Some(mut stops) = paint.gradient_stops(base)
            {
                for stop in &mut stops {
                    stop.color[3] = (f32::from(stop.color[3]) * opacity).round() as u8;
                }
                *paint = PathPaint::from_stops(&stops, paint.is_radial(), paint.gradient_angle())
                    .map_err(error)?;
            }
        }
        for c in [&mut style.fill, &mut style.stroke].into_iter().flatten() {
            c[3] = (f32::from(c[3]) * opacity).round() as u8;
        }
        let (geom, (x, y, w, h)) = geometry(path)?;
        let cap = match style.cap {
            emulsion_raster::vector::StrokeCap::Butt => "flat",
            emulsion_raster::vector::StrokeCap::Round => "rnd",
            emulsion_raster::vector::StrokeCap::Square => "sq",
        };
        let join = match style.join {
            emulsion_raster::vector::StrokeJoin::Miter => format!(
                "<a:miter lim=\"{}\"/>",
                (style.miter_limit * 100000.) as i64
            ),
            emulsion_raster::vector::StrokeJoin::Round => "<a:round/>".into(),
            emulsion_raster::vector::StrokeJoin::Bevel => "<a:bevel/>".into(),
        };
        if style.dash_count > 0 {
            warn(
                &mut self.report.warnings,
                self.index,
                &node.name,
                "Custom stroke dash pattern approximated as solid",
            );
        }
        Ok(format!(
            "<p:sp><p:nvSpPr>{props}<p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr>{}{geom}{}<a:ln w=\"{}\" cap=\"{cap}\">{}{join}</a:ln></p:spPr></p:sp>",
            xfrm(x, y, w, h, 0., (false, false)),
            geometry::fill_xml(style.fill, style.fill_paint),
            emu(f64::from(style.width)),
            geometry::fill_xml(style.stroke, style.stroke_paint)
        ))
    }
    fn image_xml(
        &mut self,
        props: &str,
        raster: &Raster,
        placement: Placement,
        opacity: f32,
    ) -> Result<String> {
        let key = format!("image{}.png", self.media.len() + 1);
        let bytes = crate::export::png8(raster.width(), raster.height(), &raster.to_srgba8())?;
        if self.media.values().map(Vec::len).sum::<usize>() + bytes.len() > 128 << 20 {
            return Err(error("PPTX image data exceeds 128 MiB"));
        }
        self.media.insert(key.clone(), bytes);
        let rel = self.relation("image", format!("../media/{key}"), false);
        Ok(format!(
            "<p:pic><p:nvPicPr>{props}<p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed=\"{rel}\"><a:alphaModFix amt=\"{}\"/></a:blip><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr>{}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>",
            (opacity * 100000.).round() as i64,
            xfrm(
                placement.x,
                placement.y,
                f64::from(raster.width()) * placement.scale_x,
                f64::from(raster.height()) * placement.scale_y,
                placement.rotation,
                (placement.flip_x, placement.flip_y)
            )
        ))
    }
}
pub fn write(project: &Project, selected: &[u64], path: &Path) -> Result<Report> {
    project.validate().map_err(error)?;
    let ids = selected.iter().copied().collect::<HashSet<_>>();
    if ids.is_empty() || ids.len() != selected.len() {
        return Err(error("Choose unique presentation pages"));
    }
    let pages = selected
        .iter()
        .map(|id| {
            project
                .pages
                .iter()
                .find(|p| p.meta.id == *id)
                .ok_or_else(|| error("Unknown presentation page"))
        })
        .collect::<Result<Vec<_>>>()?;
    // Editable PowerPoint export cannot represent independent vector-mask
    // geometry. Do not expose the unmasked base artwork, even for a currently
    // disabled or hidden component that remains editable in the source.
    if let Some(node) = pages
        .iter()
        .flat_map(|page| &page.doc.nodes)
        .find(|node| node.vector_mask.is_some())
    {
        return Err(error(format!(
            "{} has a native vector mask; choose a rendered appearance export instead of editable PowerPoint.",
            node.name
        )));
    }
    for node in pages.iter().flat_map(|page| &page.doc.nodes) {
        node.require_affine_capability("Editable PowerPoint export")?;
    }
    let size = (pages[0].doc.width, pages[0].doc.height);
    if pages.iter().any(|p| (p.doc.width, p.doc.height) != size) {
        return Err(error(
            "PowerPoint uses one slide size; resize selected pages to matching dimensions first",
        ));
    }
    for page in &project.pages {
        crate::ora::ensure_not_raw_original(&page.doc, path)?;
        for commit in page.graph.commits() {
            crate::ora::ensure_not_raw_original(&commit.doc, path)?;
        }
    }
    let map = selected
        .iter()
        .enumerate()
        .map(|(i, id)| (*id, i + 1))
        .collect::<HashMap<_, _>>();
    let mut parts = BTreeMap::new();
    let mut media = BTreeMap::new();
    let mut report = Report {
        pages: pages.len(),
        ..Default::default()
    };
    let mut presentation_rels = vec![(
        "rId1".into(),
        "slideMaster".into(),
        "slideMasters/slideMaster1.xml".into(),
        false,
    )];
    let mut slide_ids = String::new();
    let mut overrides = String::new();
    for (index, page) in pages.iter().enumerate() {
        let i = index + 1;
        let id = format!("rId{}", i + 1);
        presentation_rels.push((
            id.clone(),
            "slide".into(),
            format!("slides/slide{i}.xml"),
            false,
        ));
        std::write!(slide_ids, "<p:sldId id=\"{}\" r:id=\"{id}\"/>", 255 + i).unwrap();
        let mut slide = Slide {
            doc: &page.doc,
            index: i,
            pages: &map,
            report: &mut report,
            rels: vec![(
                "rId1".into(),
                "slideLayout".into(),
                "../slideLayouts/slideLayout1.xml".into(),
                false,
            )],
            media: &mut media,
            serial: 1,
        };
        let content = slide.nodes(None, 1.)?;
        if !page.doc.design.motion.is_empty() || !page.doc.design.keyframes.is_empty() {
            warn(
                &mut slide.report.warnings,
                i,
                "Slide",
                "Animation timelines export as the current editable static artwork",
            );
        }
        if !page.doc.design.speaker_notes.is_empty() {
            slide.relation(
                "notesSlide",
                format!("../notesSlides/notesSlide{i}.xml"),
                false,
            );
            let spec = emulsion_core::text::TextSpec {
                text: page.doc.design.speaker_notes.clone(),
                size: 12.,
                ..Default::default()
            };
            parts.insert(format!("ppt/notesSlides/notesSlide{i}.xml"),xml(format!("<p:notes xmlns:p=\"{P}\" xmlns:a=\"{A}\" xmlns:r=\"{R}\"><p:cSld><p:spTree>{}<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Notes\"/><p:cNvSpPr/><p:nvPr><p:ph type=\"body\" idx=\"1\"/></p:nvPr></p:nvSpPr><p:spPr/>{}</p:sp></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:notes>",root_tree(size.0,size.1),text::encode(&spec))));
            parts.insert(
                format!("ppt/notesSlides/_rels/notesSlide{i}.xml.rels"),
                rel_xml(&[(
                    "rId1".into(),
                    "slide".into(),
                    format!("../slides/slide{i}.xml"),
                    false,
                )]),
            );
            std::write!(overrides,"<Override PartName=\"/ppt/notesSlides/notesSlide{i}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml\"/>").unwrap();
        }
        let transition = if page.doc.design.page_transition
            == emulsion_core::design_metadata::PageTransition::Fade
        {
            "<p:transition><p:fade/></p:transition>"
        } else {
            ""
        };
        let data = xml(format!(
            "<p:sld xmlns:p=\"{P}\" xmlns:a=\"{A}\" xmlns:r=\"{R}\"><p:cSld name=\"{}\"><p:spTree>{}{content}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>{transition}</p:sld>",
            escaped(&page.meta.name),
            root_tree(size.0, size.1)
        ));
        if data.len() > 16 << 20 {
            return Err(error("Slide XML exceeds 16 MiB"));
        }
        parts.insert(format!("ppt/slides/slide{i}.xml"), data);
        parts.insert(
            format!("ppt/slides/_rels/slide{i}.xml.rels"),
            rel_xml(&slide.rels),
        );
        std::write!(overrides,"<Override PartName=\"/ppt/slides/slide{i}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>").unwrap();
    }
    parts.insert("ppt/presentation.xml".into(),xml(format!("<p:presentation xmlns:p=\"{P}\" xmlns:a=\"{A}\" xmlns:r=\"{R}\"><p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rId1\"/></p:sldMasterIdLst><p:sldIdLst>{slide_ids}</p:sldIdLst><p:sldSz cx=\"{}\" cy=\"{}\"/><p:notesSz cx=\"6858000\" cy=\"9144000\"/></p:presentation>",emu(size.0 as f64),emu(size.1 as f64))));
    parts.insert(
        "ppt/_rels/presentation.xml.rels".into(),
        rel_xml(&presentation_rels),
    );
    parts.insert(
        "_rels/.rels".into(),
        rel_xml(&[(
            "rId1".into(),
            "officeDocument".into(),
            "ppt/presentation.xml".into(),
            false,
        )]),
    );
    let map = "<p:clrMap accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" bg1=\"lt1\" bg2=\"lt2\" folHlink=\"folHlink\" hlink=\"hlink\" tx1=\"dk1\" tx2=\"dk2\"/>";
    parts.insert("ppt/slideMasters/slideMaster1.xml".into(),xml(format!("<p:sldMaster xmlns:p=\"{P}\" xmlns:a=\"{A}\" xmlns:r=\"{R}\"><p:cSld><p:spTree>{}</p:spTree></p:cSld>{map}<p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"rId1\"/></p:sldLayoutIdLst><p:txStyles><p:titleStyle/><p:bodyStyle/><p:otherStyle/></p:txStyles></p:sldMaster>",root_tree(size.0,size.1))));
    parts.insert(
        "ppt/slideMasters/_rels/slideMaster1.xml.rels".into(),
        rel_xml(&[
            (
                "rId1".into(),
                "slideLayout".into(),
                "../slideLayouts/slideLayout1.xml".into(),
                false,
            ),
            (
                "rId2".into(),
                "theme".into(),
                "../theme/theme1.xml".into(),
                false,
            ),
        ]),
    );
    parts.insert("ppt/slideLayouts/slideLayout1.xml".into(),xml(format!("<p:sldLayout xmlns:p=\"{P}\" xmlns:a=\"{A}\" xmlns:r=\"{R}\" type=\"blank\" preserve=\"1\"><p:cSld name=\"Blank\"><p:spTree>{}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>",root_tree(size.0,size.1))));
    parts.insert(
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels".into(),
        rel_xml(&[(
            "rId1".into(),
            "slideMaster".into(),
            "../slideMasters/slideMaster1.xml".into(),
            false,
        )]),
    );
    parts.insert("ppt/theme/theme1.xml".into(), xml(theme()));
    parts.insert("[Content_Types].xml".into(),xml(format!("<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/><Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/><Override PartName=\"/ppt/slideMasters/slideMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml\"/><Override PartName=\"/ppt/slideLayouts/slideLayout1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/><Override PartName=\"/ppt/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>{overrides}</Types>")));
    for (name, data) in media {
        parts.insert(format!("ppt/media/{name}"), data);
    }
    if parts.values().map(Vec::len).sum::<usize>() > 256 << 20 {
        return Err(error("PPTX exceeds 256 MiB expanded limit"));
    }
    for (name, data) in &parts {
        if (name.ends_with(".xml") || name.ends_with(".rels"))
            && !valid_xml_text(
                std::str::from_utf8(data).map_err(|_| error("Invalid XML encoding"))?,
            )
        {
            return Err(error(
                "Presentation contains text that XML cannot represent",
            ));
        }
    }
    crate::write_atomic(path, |file| {
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in parts {
            zip.start_file(name, options)?;
            zip.write_all(&data)?;
        }
        zip.finish()?;
        Ok(())
    })?;
    Ok(report)
}
fn theme() -> String {
    let colors = [
        ("dk1", "000000"),
        ("lt1", "FFFFFF"),
        ("dk2", "1F497D"),
        ("lt2", "EEECE1"),
        ("accent1", "4F81BD"),
        ("accent2", "C0504D"),
        ("accent3", "9BBB59"),
        ("accent4", "8064A2"),
        ("accent5", "4BACC6"),
        ("accent6", "F79646"),
        ("hlink", "0000FF"),
        ("folHlink", "800080"),
    ]
    .iter()
    .map(|(k, c)| format!("<a:{k}><a:srgbClr val=\"{c}\"/></a:{k}>"))
    .collect::<String>();
    let fill = "<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>";
    let line = format!(
        "<a:ln w=\"9525\">{fill}<a:prstDash val=\"solid\"/><a:miter lim=\"800000\"/></a:ln>"
    );
    format!(
        "<a:theme xmlns:a=\"{A}\" name=\"Emulsion\"><a:themeElements><a:clrScheme name=\"Emulsion\">{colors}</a:clrScheme><a:fontScheme name=\"Emulsion\"><a:majorFont><a:latin typeface=\"Arial\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:majorFont><a:minorFont><a:latin typeface=\"Arial\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:minorFont></a:fontScheme><a:fmtScheme name=\"Emulsion\"><a:fillStyleLst>{fill}{fill}{fill}</a:fillStyleLst><a:lnStyleLst>{line}{line}{line}</a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst>{fill}{fill}{fill}</a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>"
    )
}
