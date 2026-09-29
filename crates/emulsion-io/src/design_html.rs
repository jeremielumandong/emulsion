//! Self-contained, local browser presentations with retained native SVG artwork.
use crate::{IoError, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use emulsion_core::{
    Document,
    design_interactions::{Action, Runtime},
    project::{PageId, Project},
};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::{collections::BTreeSet, io::Write, path::Path};

#[derive(Debug, Default)]
pub struct Report {
    pub pages: usize,
    pub views: usize,
    pub warnings: Vec<String>,
}
fn error(message: impl Into<String>) -> IoError {
    IoError::Manifest(message.into())
}
fn artwork(doc: &Document) -> Result<String> {
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\" role=\"img\">",
        doc.width, doc.height
    );
    for id in doc.children(None) {
        let bytes = crate::project_export::viewport_subtree(doc, id)?;
        let text = String::from_utf8(bytes).map_err(|e| error(e.to_string()))?;
        let start = text.find('>').ok_or_else(|| error("Invalid SVG output"))? + 1;
        let end = text
            .rfind("</svg>")
            .ok_or_else(|| error("Invalid SVG output"))?;
        out.push_str(&text[start..end]);
    }
    out.push_str("</svg>");
    Ok(out)
}
fn media(doc: &Document, assets: &mut serde_json::Map<String, Value>) -> Value {
    let mut result = serde_json::Map::new();
    for (id, item) in &doc.design.local_media {
        if let Some(b) = emulsion_core::geometry::node_bounds(doc, item.boundary) {
            let hash = Sha256::digest(item.bytes.as_slice())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            assets
                .entry(hash.clone())
                .or_insert_with(|| json!(STANDARD.encode(item.bytes.as_slice())));
            result.insert(id.to_string(),json!({"kind":match item.kind { emulsion_core::design::media::LocalMediaKind::Video=>"video", _=>"audio" },"mime":item.mime,"asset":hash,"bounds":[b.x,b.y,b.w,b.h],"start":item.trim_start_ms as f64/1000.,"end":item.trim_end_ms.map(|v|v as f64/1000.),"volume":item.volume,"loop":item.looping,"name":item.name}));
        }
    }
    for (id, item) in &doc.design.media {
        if let Some(b) = emulsion_core::geometry::node_bounds(doc, item.boundary) {
            result.insert(id.to_string(),json!({"kind":"youtube","video":item.video_id,"start":item.start_seconds,"bounds":[b.x,b.y,b.w,b.h]}));
        }
    }
    Value::Object(result)
}
fn view(
    doc: &Document,
    report: &mut Report,
    page_name: &str,
    assets: &mut serde_json::Map<String, Value>,
) -> Result<Value> {
    if let Some((id, _)) = doc.design.keyframes.iter().find(|(_, tracks)| {
        tracks
            .iter()
            .any(|t| t.property == emulsion_core::design_keyframes::Property::TextReveal)
    }) {
        return Err(error(format!(
            "HTML export of '{page_name}' cannot preserve text-reveal keyframes on object {id} because text is exported as glyph outlines. Use native presentation or animated SVG output, or remove that track."
        )));
    }
    let mut source = doc.clone();
    for id in source.design.overlays.clone() {
        if let Some(node) = source.node_mut(id) {
            node.visible = true;
        }
    }
    let svg=artwork(&source).map_err(|e| error(format!("HTML export of '{page_name}' requires supported vector appearance: {e}. Remove unsupported blending/masks/effects or export a static PDF/image instead.")))?;
    let mut variants = serde_json::Map::new();
    let mut requested = BTreeSet::new();
    for actions in doc.design.interactions.values() {
        for action in actions {
            if let Action::Variant { target, variant } = action {
                requested.insert((*target, variant.clone()));
            }
        }
    }
    if requested.len() > 64 {
        return Err(error(
            "HTML supports up to 64 referenced component states per page.",
        ));
    }
    let mut variant_bytes = svg.len();
    for (id, name) in requested {
        let mut runtime = Runtime::default();
        runtime.variant(id, name.clone());
        let variant = runtime.source(&source).map_err(error)?;
        let artwork = artwork(&variant)?;
        variant_bytes += artwork.len();
        if variant_bytes > 128 << 20 {
            return Err(error(
                "Component states exceed the 128 MiB HTML limit. Reduce referenced states or artwork complexity.",
            ));
        }
        variants.insert(format!("{id}:{name}"),json!({"svg":artwork,"actions":variant.design.interactions,"triggers":variant.design.interaction_triggers,"media":media(&variant,assets),"keyframes":variant.design.keyframes,"motion":variant.design.motion}));
    }
    if !doc.design.media.is_empty() {
        report.warnings.push(format!(
            "{page_name}: YouTube playback requires internet access and browser embed support."
        ));
    }
    report.views += 1;
    Ok(
        json!({"width":doc.width,"height":doc.height,"svg":svg,"actions":doc.design.interactions,"triggers":doc.design.interaction_triggers,"overlays":doc.design.overlays,"variants":variants,"media":media(doc,assets),"keyframes":doc.design.keyframes,"motion":doc.design.motion,"labels":doc.nodes.iter().map(|n|(n.id.to_string(),n.name.clone())).collect::<std::collections::BTreeMap<_,_>>(),"duration":doc.design.duration_ms,"transition":doc.design.page_transition,"transition_ms":doc.design.transition_ms}),
    )
}
/// Widths are explicit native layout samples, scaled fluidly between samples.
/// Empty widths include authored dimensions, phone/tablet widths and frame breakpoints.
pub fn build(project: &Project, pages: &[PageId], widths: &[u32]) -> Result<(String, Report)> {
    project.validate().map_err(error)?;
    if pages.is_empty() || pages.iter().copied().collect::<BTreeSet<_>>().len() != pages.len() {
        return Err(error("Choose unique existing pages to export."));
    }
    if widths.len() > 16
        || widths.iter().copied().collect::<BTreeSet<_>>().len() != widths.len()
        || widths.iter().any(|w| !(64..=8192).contains(w))
    {
        return Err(error(
            "Choose at most 16 preview widths between 64 and 8192 pixels.",
        ));
    }
    let mut report = Report::default();
    let mut output = Vec::new();
    let mut assets = serde_json::Map::new();
    let mut size = 0;
    let selected: BTreeSet<_> = pages.iter().copied().collect();
    for page_id in pages {
        let page = project
            .pages
            .iter()
            .find(|p| p.meta.id == *page_id)
            .ok_or_else(|| error("Unknown export page"))?;
        let mut sizes: BTreeSet<u32> = widths.iter().copied().collect();
        sizes.insert(page.doc.width);
        if widths.is_empty() {
            sizes.extend([375, 768]);
            for frame in page.doc.design.frames.values().filter(|f| {
                f.breakpoint_reference == emulsion_core::design_layout::BreakpointReference::Canvas
            }) {
                for b in &frame.breakpoints {
                    if (64. ..=8192.).contains(&b.min_width) {
                        sizes.insert(b.min_width.ceil() as u32);
                    }
                }
            }
        }
        if sizes.len() > 17 {
            return Err(error(
                "Too many responsive widths; supply an explicit set of up to 16 widths.",
            ));
        }
        for actions in page.doc.design.interactions.values() {
            for action in actions {
                if let Action::Slide { page: target } = action
                    && !selected.contains(target)
                {
                    report.warnings.push(format!(
                        "{}: link to omitted slide {target} is unavailable in this export.",
                        page.meta.name
                    ));
                }
            }
        }
        let mut views = Vec::new();
        for width in sizes {
            let source = if width == page.doc.width {
                page.doc.clone()
            } else {
                emulsion_core::design_metadata::resize_variant(&page.doc, width, page.doc.height)
                    .map_err(error)?
                    .doc
            };
            let value = view(&source, &mut report, &page.meta.name, &mut assets)?;
            size += value.to_string().len();
            if size
                + assets
                    .values()
                    .map(|v| v.as_str().map_or(0, str::len))
                    .sum::<usize>()
                > 128 << 20
            {
                return Err(error(
                    "HTML output exceeds 128 MiB; reduce pages, media or responsive widths.",
                ));
            }
            views.push(value);
        }
        output.push(json!({"id":page.meta.id,"name":page.meta.name,"views":views}));
        report.pages += 1;
    }
    let data = serde_json::to_string(&json!({"pages":output,"assets":assets}))
        .map_err(|e| error(e.to_string()))?
        .replace('<', "\\u003c")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    if data.len() > 128 << 20 {
        return Err(error(
            "HTML output exceeds 128 MiB; reduce pages, media or responsive widths.",
        ));
    }
    report.warnings.sort();
    report.warnings.dedup();
    let html = format!(
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Emulsion presentation</title><style>{}</style><body><main id=\"stage\" aria-label=\"Presentation\"></main><nav aria-label=\"Presentation controls\"><button id=\"prev\" aria-label=\"Previous slide\">←</button><span id=\"label\"></span><button id=\"next\" aria-label=\"Next slide\">→</button><button id=\"restart\">Replay animation</button><button id=\"fullscreen\">Fullscreen</button></nav><p id=\"status\" role=\"status\"></p><script type=\"application/json\" id=\"deck\">{data}</script><script>{}</script></body></html>",
        include_str!("design_html.css"),
        include_str!("design_html.js")
    );
    Ok((html, report))
}
pub fn write(project: &Project, pages: &[PageId], widths: &[u32], path: &Path) -> Result<Report> {
    if path
        .extension()
        .and_then(|s| s.to_str())
        .is_none_or(|s| !s.eq_ignore_ascii_case("html"))
    {
        return Err(error("HTML export path must end in .html"));
    }
    let (html, report) = build(project, pages, widths)?;
    crate::write_atomic(path, |file| {
        file.write_all(html.as_bytes())?;
        Ok(())
    })?;
    Ok(report)
}

#[cfg(test)]
#[path = "design_html_tests.rs"]
mod tests;
