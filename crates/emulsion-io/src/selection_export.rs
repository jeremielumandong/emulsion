//! Isolated selection export retains native geometry and ancestor clipping.
use crate::{IoError, Result};
use emulsion_core::{
    Document, NodeId, NodeKind, design_layout, geometry,
    project::{ProjectEditor, ProjectKind},
};
use emulsion_raster::{BlendMode, IRect};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::Path};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bounds {
    #[default]
    Content,
    Frame,
    Canvas,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    #[default]
    Svg,
    Pdf,
    Png,
}
impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Svg => "svg",
            Self::Pdf => "pdf",
            Self::Png => "png",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    pub bounds: Bounds,
    pub padding: u32,
    pub transparent: bool,
    pub strict_vectors: bool,
    pub overwrite: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            bounds: Bounds::Content,
            padding: 0,
            transparent: true,
            strict_vectors: false,
            overwrite: false,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub nodes: Vec<NodeId>,
    pub origin: [i32; 2],
    pub width: u32,
    pub height: u32,
    pub rasterized: bool,
    pub diagnostics: Vec<String>,
}
fn error(s: impl Into<String>) -> IoError {
    IoError::Manifest(s.into())
}
fn visible(doc: &Document, mut id: NodeId) -> bool {
    loop {
        let Some(n) = doc.node(id) else {
            return false;
        };
        if !n.visible {
            return false;
        }
        let Some(parent) = n.parent else {
            return true;
        };
        id = parent;
    }
}
fn margin(node: &emulsion_core::Node) -> i32 {
    use emulsion_core::styles::LayerStyle;
    if !node.effects_enabled {
        return 0;
    }
    node.styles
        .iter()
        .map(|s| match s {
            LayerStyle::DropShadow { distance, size, .. }
            | LayerStyle::InnerShadow { distance, size, .. }
            | LayerStyle::Satin { distance, size, .. } => (distance + size * 3.).ceil() as i32,
            LayerStyle::OuterGlow { size, .. }
            | LayerStyle::InnerGlow { size, .. }
            | LayerStyle::BevelEmboss { size, .. } => (size * 3.).ceil() as i32,
            LayerStyle::Stroke { size, .. } => size.ceil() as i32 + 1,
            _ => 0,
        })
        .max()
        .unwrap_or(0)
        .clamp(0, 600)
}
fn inflate(r: IRect, p: i32) -> IRect {
    IRect::new(
        r.x.saturating_sub(p),
        r.y.saturating_sub(p),
        r.w.saturating_add(p * 2),
        r.h.saturating_add(p * 2),
    )
}
/// Prepare a render-only copy. Authored nodes, source pixels and history are untouched.
/// Unselected nodes stay hidden in this temporary copy solely to preserve metadata references.
pub fn prepare(source: &Document, ids: &[NodeId], options: &Options) -> Result<(Document, Report)> {
    source.validate().map_err(|e| error(e.to_string()))?;
    let requested: HashSet<_> = ids.iter().copied().collect();
    if ids.is_empty()
        || ids.len() > emulsion_core::document::MAX_NODES
        || requested.len() != ids.len()
        || ids.iter().any(|id| source.node(*id).is_none())
    {
        return Err(error("Select unique existing objects to export."));
    }
    if options.padding > 1000 {
        return Err(error("Export padding must be 0–1000 pixels."));
    }
    let roots: Vec<_> = source
        .nodes
        .iter()
        .filter(|n| {
            requested.contains(&n.id)
                && !requested
                    .iter()
                    .any(|p| *p != n.id && source.is_ancestor(*p, n.id))
        })
        .map(|n| n.id)
        .collect();
    let selected: HashSet<_> = roots.iter().flat_map(|id| source.subtree(*id)).collect();
    let mut included = selected.clone();
    for id in &roots {
        let mut parent = source.node(*id).and_then(|n| n.parent);
        while let Some(id) = parent {
            included.insert(id);
            parent = source.node(id).and_then(|n| n.parent);
        }
    }
    for id in &included {
        let node = source.node(*id).unwrap();
        if visible(source, *id)
            && let Some(base) = node.clip_to
            && !selected.contains(&base)
        {
            return Err(error(format!(
                "Object {id} needs clipping base {base}. Include that base or export their containing group."
            )));
        }
    }
    if !selected.iter().any(|id| visible(source, *id)) {
        return Err(error(
            "The selection has no visible artwork. Show it before exporting.",
        ));
    }
    let mut rect = match options.bounds {
        Bounds::Canvas => IRect::new(0, 0, source.width as i32, source.height as i32),
        Bounds::Frame => {
            if roots.len() != 1 {
                return Err(error("Frame bounds require one responsive frame."));
            }
            let (x, y, w, h) = design_layout::bounds(source, roots[0])
                .ok_or_else(|| error("Select one responsive frame for frame bounds."))?;
            IRect::new(
                x.floor() as i32,
                y.floor() as i32,
                (x + w).ceil() as i32 - x.floor() as i32,
                (y + h).ceil() as i32 - y.floor() as i32,
            )
        }
        Bounds::Content => {
            let mut bounds = IRect::default();
            for node in source
                .nodes
                .iter()
                .filter(|n| selected.contains(&n.id) && !n.is_group() && visible(source, n.id))
            {
                let Some(mut b) = geometry::node_bounds(source, node.id) else {
                    continue;
                };
                let mut current = Some(node.id);
                while let Some(id) = current {
                    let n = source.node(id).unwrap();
                    b = inflate(b, margin(n));
                    if let Some([x, y, w, h]) =
                        emulsion_core::design_clipping::content_rect(source, id)
                    {
                        b = b.intersect(&IRect::new(
                            x.floor() as i32,
                            y.floor() as i32,
                            (x + w).ceil() as i32 - x.floor() as i32,
                            (y + h).ceil() as i32 - y.floor() as i32,
                        ));
                    }
                    current = n.parent;
                }
                if let Some(base) = node.clip_to
                    && let Some(clip) = geometry::node_bounds(source, base)
                {
                    b = b.intersect(&clip);
                }
                bounds = bounds.union(&b);
            }
            bounds
        }
    };
    if rect.is_empty() {
        return Err(error("The selection has no exportable bounds."));
    }
    rect = inflate(rect, options.padding as i32);
    crate::import::check_size(rect.w as u32, rect.h as u32)?;
    if rect.w as u64 * rect.h as u64 > 64_000_000 {
        return Err(error(
            "Selection export is limited to 64 million pixels. Use frame bounds or a smaller selection.",
        ));
    }
    let mut doc = source.clone();
    // A cropped output width must not activate a different responsive preset.
    for id in source.design.frames.keys() {
        if let Some(frame) = design_layout::effective_frame(source, *id) {
            doc.design.frames.insert(*id, frame);
        }
    }
    for node in &mut doc.nodes {
        node.visible &= included.contains(&node.id);
    }
    let mut diagnostics = Vec::new();
    if included.iter().any(|id| {
        let n = source.node(*id).unwrap();
        n.blend != BlendMode::Normal && !(n.is_group() && n.blend == BlendMode::PassThrough)
            || n.blending != Default::default()
            || matches!(n.kind, NodeKind::Adjust(_))
    }) {
        diagnostics.push("Selection is isolated from unselected backdrop artwork; backdrop-dependent blending or adjustments may differ.".into());
    }
    if included.len() > selected.len() {
        diagnostics.push("Ancestor groups retain their opacity, masks, effects and responsive clipping; unrelated siblings are excluded.".into());
    }
    if options.bounds == Bounds::Content
        && included
            .iter()
            .any(|id| margin(source.node(*id).unwrap()) > 0)
    {
        diagnostics.push("Content bounds reserve conservative space for layer effects.".into());
    }
    // A selected page-wide fill has finite source-canvas bounds; padding stays empty.
    for node in &mut doc.nodes {
        if node.visible
            && let NodeKind::Fill { rgba } = node.kind
        {
            node.kind = emulsion_core::Node::path(
                node.id,
                node.name.clone(),
                std::sync::Arc::new(emulsion_raster::vector_geometry::rectangle(
                    0.,
                    0.,
                    source.width as f64,
                    source.height as f64,
                )),
                emulsion_raster::vector::PathStyle {
                    fill: Some(rgba),
                    stroke: None,
                    ..Default::default()
                },
                source.width,
                source.height,
            )
            .kind;
        }
    }
    geometry::crop(&mut doc, rect, 0.);
    if !options.transparent {
        let mut background =
            emulsion_core::Node::new(0, "Export background", NodeKind::Fill { rgba: [255; 4] });
        background.id = doc
            .nodes
            .iter()
            .map(|n| n.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| error("No free background node ID."))?;
        doc.next_id = doc.next_id.max(background.id.saturating_add(1));
        doc.nodes.insert(0, background);
    }
    doc.validate().map_err(|e| error(e.to_string()))?;
    Ok((
        doc,
        Report {
            nodes: roots,
            origin: [rect.x, rect.y],
            width: rect.w as u32,
            height: rect.h as u32,
            rasterized: false,
            diagnostics,
        },
    ))
}
/// Write one ordinary SVG/PDF/PNG file atomically. No native document mutations.
pub fn write(
    source: &Document,
    ids: &[NodeId],
    format: Format,
    options: &Options,
    path: &Path,
) -> Result<Report> {
    if path
        .extension()
        .and_then(|s| s.to_str())
        .is_none_or(|ext| !ext.eq_ignore_ascii_case(format.extension()))
    {
        return Err(error(format!(
            "Choose a .{} destination.",
            format.extension()
        )));
    }
    if path.exists() && !options.overwrite {
        return Err(error(
            "The destination exists. Choose another path or explicitly allow overwrite.",
        ));
    }
    crate::ora::ensure_not_raw_original(source, path)?;
    let includes_raw = source.raw.as_ref().is_some_and(|raw| {
        ids.iter()
            .any(|id| source.subtree(*id).contains(&raw.node_id))
    });
    let mut developed = if includes_raw {
        crate::export::develop_document(source)?
    } else {
        source.clone()
    };
    // Development is resolved before isolation and must not reopen an excluded source in PDF export.
    developed.raw = None;
    let (doc, mut report) = prepare(&developed, ids, options)?;
    let (svg, rasterized) = crate::project_export::svg(&doc)?;
    report.rasterized = rasterized;
    if rasterized {
        if options.strict_vectors {
            return Err(error(
                "This selection needs rendered pixels for masks, blending or effects. Disable strict vectors or simplify its appearance.",
            ));
        }
        report.diagnostics.push("Masks, blending or effects require a rendered appearance; original editable objects remain unchanged.".into());
    }
    let destination = path;
    let directory = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temporary = tempfile::NamedTempFile::new_in(directory)?.into_temp_path();
    let path: &std::path::Path = &temporary;
    match format {
        Format::Svg => crate::write_atomic(path, |file| {
            use std::io::Write;
            file.write_all(&svg)?;
            Ok(())
        })?,
        Format::Png => {
            // Render native SVG contours directly; raster fallbacks retain existing compositor appearance.
            let tree = resvg::usvg::Tree::from_data(&svg, &Default::default())
                .map_err(|e| error(e.to_string()))?;
            let mut pixmap = resvg::tiny_skia::Pixmap::new(report.width, report.height)
                .ok_or_else(|| error("Cannot allocate selection image."))?;
            resvg::render(
                &tree,
                resvg::tiny_skia::Transform::identity(),
                &mut pixmap.as_mut(),
            );
            let mut rgba = pixmap.data().to_vec();
            for pixel in rgba.as_chunks_mut::<4>().0 {
                let a = pixel[3] as u32;
                for c in &mut pixel[..3] {
                    *c = (*c as u32 * 255 + a / 2)
                        .checked_div(a)
                        .unwrap_or(0)
                        .min(255) as u8;
                }
            }
            let bytes = crate::export::png8(report.width, report.height, &rgba)?;
            crate::write_atomic(path, |file| {
                use std::io::Write;
                file.write_all(&bytes)?;
                Ok(())
            })?;
        }
        Format::Pdf => {
            let project = ProjectEditor::new_project(ProjectKind::Design, doc)
                .map_err(error)?
                .snapshot()
                .ok_or_else(|| error("Cannot snapshot selection export."))?;
            crate::project_export::write(
                &project,
                &[project.pages[0].meta.id],
                crate::project_export::Format::Pdf,
                false,
                path,
            )?;
        }
    }
    if options.overwrite {
        temporary
            .persist(destination)
            .map_err(|e| IoError::Io(e.error))?;
    } else {
        temporary
            .persist_noclobber(destination)
            .map_err(|e| IoError::Io(e.error))?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::{
        Command, Editor, Node,
        command::Slot,
        design_layout::{self, Breakpoint, Child, Frame, FrameOverrides},
        text::TextSpec,
    };
    use emulsion_raster::{composite::flatten, vector::PathStyle, vector_geometry::rectangle};
    use std::sync::Arc;
    fn shape(e: &mut Editor, x: f64, y: f64, w: f64, h: f64, color: [u8; 4]) -> NodeId {
        e.execute(Command::AddNode {
            node: Box::new(Node::path(
                0,
                "Shape",
                Arc::new(rectangle(x, y, w, h)),
                PathStyle {
                    fill: Some(color),
                    stroke: None,
                    ..Default::default()
                },
                e.doc.width,
                e.doc.height,
            )),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap()
    }
    #[test]
    fn selection_export_keeps_native_text_transparency_and_source_history() {
        let mut e = Editor::new(Document::new(400, 300), None);
        shape(&mut e, 0., 0., 400., 300., [0, 255, 0, 255]);
        let id = e
            .execute(Command::AddNode {
                node: Box::new(Node::text(
                    0,
                    "Text",
                    TextSpec {
                        text: "Native selection".into(),
                        size: 24.,
                        x: 80.,
                        y: 60.,
                        underline: true,
                        ..Default::default()
                    },
                    400,
                    300,
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let before = e.doc.clone();
        let history = e.history.len();
        let dir = tempfile::tempdir().unwrap();
        for format in [Format::Svg, Format::Pdf, Format::Png] {
            let path = dir.path().join(format!("selection.{}", format.extension()));
            let report = write(
                &e.doc,
                &[id],
                format,
                &Options {
                    padding: 8,
                    ..Default::default()
                },
                &path,
            )
            .unwrap();
            assert!(!report.rasterized);
            assert!(report.width < 400 && report.height < 300);
            let bytes = std::fs::read(path).unwrap();
            match format {
                Format::Svg => {
                    let svg = String::from_utf8(bytes).unwrap();
                    assert!(!svg.contains("<image"));
                    assert!(!svg.contains("#00ff00"));
                }
                Format::Pdf => assert!(bytes.starts_with(b"%PDF-")),
                Format::Png => {
                    let rgba = image::load_from_memory(&bytes).unwrap().into_rgba8();
                    assert_eq!(rgba.get_pixel(0, 0)[3], 0);
                    assert!(rgba.pixels().any(|p| p[3] > 0));
                }
            }
        }
        assert_eq!(e.doc, before);
        assert_eq!(e.history.len(), history);
        let (white, _) = prepare(
            &e.doc,
            &[id],
            &Options {
                padding: 8,
                transparent: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(flatten(&white.composite_tree(), 0).get(0, 0), [65535; 4]);
    }
    #[test]
    fn selection_export_retains_ancestor_clip_and_source_breakpoint_when_cropped() {
        let mut e = Editor::new(Document::new(400, 300), None);
        let child = shape(&mut e, 20., 20., 140., 50., [255, 0, 0, 255]);
        let group = e
            .execute(Command::Group {
                ids: vec![child],
                name: "Frame".into(),
            })
            .unwrap()
            .unwrap();
        let mut frame = Frame {
            padding: [0.; 4],
            ..Default::default()
        };
        frame.children.insert(
            child,
            Child {
                absolute: true,
                ..Default::default()
            },
        );
        e.begin("Frame");
        design_layout::enable(&mut e, group, frame, (80., 80.)).unwrap();
        e.end();
        let mut design = e.doc.design.clone();
        design
            .frames
            .get_mut(&group)
            .unwrap()
            .breakpoints
            .push(Breakpoint {
                min_width: 300.,
                overrides: FrameOverrides {
                    clip_content: Some(true),
                    ..Default::default()
                },
            });
        e.execute(Command::SetDesign {
            design: Box::new(design),
        })
        .unwrap();
        let (doc, report) = prepare(&e.doc, &[child], &Options::default()).unwrap();
        assert_eq!(report.width, 80);
        assert!(
            design_layout::effective_frame(&doc, group)
                .unwrap()
                .clip_content
        );
        assert!(
            !doc.node(doc.design.frames[&group].boundary)
                .unwrap()
                .visible
        );
        let raster = flatten(&doc.composite_tree(), 0);
        assert!(raster.get(79, 10)[3] > 0);
        let (_, report) = prepare(
            &e.doc,
            &[group],
            &Options {
                bounds: Bounds::Frame,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!((report.width, report.height), (80, 80));
    }
    #[test]
    fn selection_export_rejects_incomplete_clips_and_preserves_destinations_on_failure() {
        let mut e = Editor::new(Document::new(200, 150), None);
        let base = shape(&mut e, 20., 20., 40., 40., [255, 0, 0, 255]);
        let child = shape(&mut e, 0., 0., 150., 100., [0, 0, 255, 255]);
        e.execute(Command::SetClip {
            id: child,
            clip_to: Some(base),
        })
        .unwrap();
        assert!(prepare(&e.doc, &[child], &Options::default()).is_err());
        let (doc, report) = prepare(&e.doc, &[base, child], &Options::default()).unwrap();
        assert_eq!((report.width, report.height), (40, 40));
        assert!(flatten(&doc.composite_tree(), 0).get(20, 20)[2] > 60000);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("existing.svg");
        std::fs::write(&path, b"keep").unwrap();
        assert!(
            write(
                &e.doc,
                &[base, child],
                Format::Svg,
                &Options::default(),
                &path
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"keep");
        e.doc.node_mut(child).unwrap().styles.push(
            emulsion_core::styles::LayerStyle::InnerShadow {
                color: [0; 3],
                opacity: 0.5,
                angle: 45.,
                distance: 5.,
                size: 3.,
            },
        );
        let report = write(
            &e.doc,
            &[base, child],
            Format::Svg,
            &Options {
                overwrite: true,
                ..Default::default()
            },
            &path,
        )
        .unwrap();
        assert!(report.rasterized);
        let saved = std::fs::read(&path).unwrap();
        assert!(
            write(
                &e.doc,
                &[base, child],
                Format::Svg,
                &Options {
                    overwrite: true,
                    strict_vectors: true,
                    ..Default::default()
                },
                &path
            )
            .is_err()
        );
        assert_eq!(std::fs::read(path).unwrap(), saved);
    }
}
