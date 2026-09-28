//! Offline vendor packs, curated from the pinned draw.io stencil source.
use crate::{
    IoError, Result,
    template_pack::{Kind, Manifest, Pack},
};
use emulsion_core::{
    Document, NodeKind,
    diagram::{Builder, ShapeKind},
    graph::Graph,
    project::{PageMeta, Project, ProjectKind, ProjectPage},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Cursor,
};
pub const PACKS: &[(&str, &str, &str)] = &[
    ("aws4", "AWS Architecture", "Cloud"),
    ("azure", "Azure", "Cloud"),
    ("gcp2", "Google Cloud", "Cloud"),
    ("kubernetes", "Kubernetes", "Cloud"),
    ("cisco", "Cisco Network", "Network"),
    ("networks", "Network Devices", "Network"),
    ("bpmn", "BPMN", "Business"),
    ("flowchart", "Flowchart", "Business"),
    ("floorplan", "Floor Plans", "Business"),
    ("electrical", "Electrical", "Business"),
    ("mockup", "Wireframe UI", "UI"),
    ("office", "Office", "Business"),
];
pub fn entries(pack: &str) -> Vec<&'static str> {
    let prefix = format!("mxgraph.{pack}.");
    let mut names = crate::drawio::vendor::names()
        .filter(|n| n.starts_with(&prefix))
        .collect::<Vec<_>>();
    // Prefer common building blocks; bound each first-use catalog to 64 entries.
    names.sort_by_key(|n| {
        (
            !([
                "server", "router", "switch", "ec2", "s3", "database", "storage", "compute",
                "gateway", "cloud", "start", "end", "process", "task", "wall", "door", "chair",
                "resistor", "button", "input", "pc",
            ]
            .iter()
            .any(|word| n.rsplit('.').next().unwrap_or(n).contains(word))),
            *n,
        )
    });
    names.truncate(64);
    names
}
pub fn document(name: &str) -> Result<Document> {
    document_with_notes(name).map(|v| v.0)
}
fn document_with_notes(name: &str) -> Result<(Document, Vec<String>)> {
    let mut b = Builder::new(180, 140).map_err(IoError::Manifest)?;
    let id = b
        .add_shape(ShapeKind::Process, [20., 15., 140., 100.], "")
        .map_err(IoError::Manifest)?;
    let mut doc = b.finish().map_err(IoError::Manifest)?;
    let mut warnings = BTreeSet::new();
    let style = BTreeMap::from([
        ("fillColor".into(), "#e8efff".into()),
        ("strokeColor".into(), "#334155".into()),
    ]);
    let svg = crate::drawio::vendor::svg_at(name, &style, 140., 100., &mut warnings)?;
    let first = doc.next_id;
    crate::svg_vectors::append(&mut doc, id, &svg, [20., 15., 140., 100.])?;
    if !doc.nodes.iter().filter(|n|n.id>=first&&n.visible).any(|n|matches!(&n.kind,NodeKind::Path{path,style,..} if !path.is_empty()&&(style.fill.is_some()||style.stroke.is_some()))) {
        return Err(IoError::Unsupported("Stencil produced no visible vector geometry".into()));
    }
    let body = doc.diagram.as_ref().unwrap().shapes[&id].body;
    let (w, h) = (doc.width, doc.height);
    if let NodeKind::Path { path, style, cache } = &mut doc.node_mut(body).unwrap().kind {
        style.fill = None;
        style.stroke = None;
        *cache = emulsion_core::vector_cache::VectorRaster::path(path.clone(), *style, w, h);
    }
    doc.normalize();
    doc.validate()
        .map_err(|e| IoError::Manifest(e.to_string()))?;
    Ok((doc, warnings.into_iter().collect()))
}
pub fn build(id: &str) -> Result<(Pack, Vec<String>)> {
    let (_, name, category) = PACKS
        .iter()
        .find(|p| p.0 == id)
        .ok_or_else(|| IoError::Manifest("Unknown bundled pack".into()))?;
    let mut pages = Vec::new();
    let mut notes = Vec::new();
    for key in entries(id) {
        match document_with_notes(key) {
            Ok((doc, warnings)) => {
                notes.extend(warnings.into_iter().map(|w| format!("{key}: {w}")));
                let id = pages.len() as u64 + 1;
                pages.push(ProjectPage {
                    meta: PageMeta {
                        id,
                        name: key.rsplit('.').next().unwrap_or(key).replace('_', " "),
                        bleed_mm: 0.,
                    },
                    graph: Graph::new(doc.clone(), "Bundled stencil"),
                    doc,
                });
            }
            Err(e) => notes.push(format!("{key}: {e}")),
        }
    }
    if pages.is_empty() {
        return Err(IoError::Manifest(
            "No renderable stencils in this pack".into(),
        ));
    }
    let project = Project {
        kind: ProjectKind::Diagram,
        next_page_id: pages.len() as u64 + 1,
        pages,
        active: 1,
    };
    project.validate().map_err(IoError::Manifest)?;
    let mut bytes = Cursor::new(Vec::new());
    crate::project::write_to(&project, &mut bytes)?;
    let mut manifest = Manifest::new(Kind::Stencil, format!("{name} · draw.io"));
    manifest.tags = vec![category.to_string(), format!("drawio:{id}")];
    manifest.author =
        "draw.io contributors · pinned source 0f419a92c769adb5fb20f2b18053a5ae8c7e4993".into();
    manifest.license="See bundled draw.io Apache license and stencil asset terms; vendor trademarks belong to their owners.".into();
    Ok((
        Pack {
            manifest,
            project,
            preview: None,
            project_bytes: bytes.into_inner(),
        },
        notes,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_bundled_family_builds_a_native_vector_pack() {
        for (id, _, _) in PACKS {
            let (pack, notes) = build(id).unwrap_or_else(|e| panic!("{id}: {e}"));
            assert!(!pack.project.pages.is_empty(), "{id}: {notes:?}");
            assert!(pack.project.pages.len() <= 64);
            for page in &pack.project.pages {
                assert!(
                    page.doc
                        .nodes
                        .iter()
                        .filter(|n| matches!(n.kind, NodeKind::Path { .. }))
                        .count()
                        > 1
                );
            }
            let page = &pack.project.pages[0];
            crate::svg_viewport::SvgViewport::new(&page.doc)
                .unwrap()
                .render((192, 192), [1., 0., 0., 1., 0., 0.])
                .unwrap();
        }
    }
    #[test]
    fn installed_pack_has_per_entry_preview_and_native_artwork() {
        let (mut pack, _) = build("kubernetes").unwrap();
        pack.project.pages.truncate(1);
        let mut bytes = Cursor::new(Vec::new());
        crate::project::write_to(&pack.project, &mut bytes).unwrap();
        pack.project_bytes = bytes.into_inner();
        let dir = tempfile::tempdir().unwrap();
        let (catalog, _) = crate::template_pack::install(dir.path(), pack).unwrap();
        let asset = catalog.assets.first().unwrap();
        assert!(asset.path.parent().unwrap().join("entry-0.png").exists());
        let reopened = crate::project::read(&asset.path).unwrap();
        assert!(
            reopened.pages[0]
                .doc
                .nodes
                .iter()
                .filter(|n| matches!(n.kind, NodeKind::Path { .. }))
                .count()
                > 1
        );
    }
}
