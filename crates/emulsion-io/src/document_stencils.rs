//! Explicitly save document objects as durable local packs without modifying the source diagram.
use crate::{
    IoError, Result,
    template_pack::{self, Kind, Manifest, Pack},
};
use emulsion_core::{
    Document, diagram,
    graph::Graph,
    project::{PageMeta, Project, ProjectKind, ProjectPage},
};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, io::Cursor, path::Path, sync::Arc};

pub fn save(root: &Path, project: &Project, name: &str) -> Result<Vec<u64>> {
    let mut pages = Vec::new();
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    let mut area = 0u64;
    for page in &project.pages {
        for entry in diagram::document_stencils(&page.doc) {
            let fragment =
                diagram::document_stencil(&page.doc, entry.source).map_err(IoError::Manifest)?;
            let mut doc = Document::new(page.doc.width, page.doc.height);
            doc.blend_space = page.doc.blend_space;
            doc.nodes = fragment.nodes;
            doc.diagram = fragment.diagram.map(Arc::new);
            doc.design = fragment.design;
            doc.normalize();
            let Some(b) = emulsion_core::geometry::node_bounds(&doc, entry.source)? else {
                continue;
            };
            emulsion_core::geometry::crop(
                &mut doc,
                emulsion_raster::IRect::new(b.x - 8, b.y - 8, b.w + 16, b.h + 16),
                0.,
            )?;
            // Canonical node IDs keep repeated imports stable even when source IDs change.
            let map = doc
                .nodes
                .iter()
                .enumerate()
                .map(|(i, n)| (n.id, i as u64 + 1))
                .collect::<std::collections::HashMap<_, _>>();
            for node in &mut doc.nodes {
                node.id = map[&node.id];
                node.parent = node.parent.map(|id| map[&id]);
                node.clip_to = node.clip_to.and_then(|id| map.get(&id).copied());
                node.locked = false;
                node.locks = Default::default();
                node.link_group = None;
            }
            doc.diagram = doc.diagram.as_ref().map(|d| Arc::new(d.remap(&map)));
            doc.design = doc.design.remap(&map);
            doc.next_id = doc.nodes.len() as u64 + 1;
            doc.validate()
                .map_err(|e| IoError::Manifest(e.to_string()))?;
            let mut encoded = Cursor::new(Vec::new());
            crate::ora::write_to(&doc, None, &mut encoded)?;
            let hash = Sha256::digest(encoded.into_inner());
            if !seen.insert(hash) {
                continue;
            }
            let pixels = u64::from(doc.width) * u64::from(doc.height);
            if !pages.is_empty()
                && (pages.len() >= 128
                    || area + pixels > emulsion_core::project::MAX_PROJECT_PIXELS)
            {
                ids.push(install(
                    root,
                    std::mem::take(&mut pages),
                    name,
                    ids.len() + 1,
                )?);
                area = 0;
            }
            let id = pages.len() as u64 + 1;
            area += pixels;
            pages.push(ProjectPage {
                meta: PageMeta {
                    id,
                    name: entry
                        .name
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(200)
                        .collect::<String>(),
                    bleed_mm: 0.,
                },
                graph: stable_graph(doc.clone())?,
                doc,
            });
            if pages.last().unwrap().meta.name.trim().is_empty() {
                pages.last_mut().unwrap().meta.name = "Imported shape".into();
            }
        }
    }
    if !pages.is_empty() {
        ids.push(install(root, pages, name, ids.len() + 1)?);
    }
    Ok(ids)
}
fn stable_graph(doc: Document) -> Result<Graph> {
    use emulsion_core::graph::{Branch, Commit};
    Graph::from_parts(
        vec![Commit {
            id: 1,
            parents: Vec::new(),
            name: "Imported stencil".into(),
            time: 0,
            auto: false,
            branch: "main".into(),
            doc,
        }],
        std::collections::BTreeMap::from([("main".into(), Branch { tip: 1, base: 1 })]),
        "main".into(),
    )
    .map_err(|e| IoError::Manifest(e.to_string()))
}
fn install(root: &Path, pages: Vec<ProjectPage>, name: &str, part: usize) -> Result<u64> {
    let project = Project {
        storyboard: None,
        kind: ProjectKind::Diagram,
        next_page_id: pages.len() as u64 + 1,
        pages,
        active: 1,
    };
    project.validate().map_err(IoError::Manifest)?;
    let mut bytes = Cursor::new(Vec::new());
    crate::project::write_to(&project, &mut bytes)?;
    let name = name
        .chars()
        .filter(|c| !c.is_control())
        .take(150)
        .collect::<String>();
    let mut manifest = Manifest::new(Kind::Stencil, format!("Saved shapes · {name} · {part}"));
    manifest.tags = vec!["Saved shapes".into()];
    manifest.description="Editable objects captured from an imported diagram. Source artwork and captions are preserved.".into();
    let pack = Pack {
        manifest,
        project,
        preview: None,
        project_bytes: bytes.into_inner(),
    };
    template_pack::install(root, pack).map(|(_, id)| id)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagram_stencils_persist_after_source_is_closed_and_reimports_deduplicate() {
        let mut b = diagram::Builder::new(2000, 1600).unwrap();
        b.add_shape(
            diagram::ShapeKind::Database,
            [800., 900., 140., 80.],
            "Orders",
        )
        .unwrap();
        let doc = b.finish().unwrap();
        let project = Project {
            storyboard: None,
            kind: ProjectKind::Diagram,
            active: 1,
            next_page_id: 2,
            pages: vec![ProjectPage {
                meta: PageMeta {
                    id: 1,
                    name: "Source".into(),
                    bleed_mm: 0.,
                },
                graph: Graph::new(doc.clone(), "Source"),
                doc,
            }],
        };
        let dir = tempfile::tempdir().unwrap();
        let ids = save(dir.path(), &project, "Architecture").unwrap();
        assert_eq!(ids, save(dir.path(), &project, "Architecture").unwrap());
        drop(project);
        let catalog = crate::creative_library::load(dir.path()).unwrap();
        assert_eq!(catalog.assets.len(), 1);
        let reopened = crate::project::read(&catalog.assets[0].path).unwrap();
        let doc = &reopened.pages[0].doc;
        assert!(doc.width < 200 && doc.height < 150);
        assert_eq!(doc.diagram.as_ref().unwrap().shapes.len(), 1);
        assert!(doc.nodes.iter().any(
            |n| matches!(&n.kind,emulsion_core::NodeKind::Text{spec,..} if spec.text=="Orders")
        ));
    }
}
