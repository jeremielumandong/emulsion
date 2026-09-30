//! Native artwork supplied in Agentic AI.emu, exposed as individual diagram symbols.
use super::*;
use emulsion_core::{Editor, command::Slot, fragment::Fragment, geometry};
use std::sync::Arc;

pub(super) const PACKS: &[(&str, &str, &str)] = &[
    ("agentic-ai-llms", "Agentic AI · LLMs", "Agentic AI"),
    (
        "agentic-ai-cloud",
        "Agentic AI · Cloud applications",
        "Agentic AI",
    ),
    (
        "agentic-ai-io",
        "Agentic AI · Inputs and outputs",
        "Agentic AI",
    ),
    (
        "agentic-ai-workflow",
        "Agentic AI · Workflow components",
        "Agentic AI",
    ),
    (
        "agentic-ai-integration",
        "Agentic AI · Integration and flow",
        "Agentic AI",
    ),
];
const GROUPS: &[&str] = &[
    "Large Language Models (LLMs)",
    "Cloud applications",
    "System inputs/outputs",
    "AI workflow components",
    "System integration and flow",
];

struct Entry {
    key: String,
    name: String,
    pack: &'static str,
    source: emulsion_core::NodeId,
}
struct Catalog {
    doc: Document,
    entries: Vec<Entry>,
}
fn catalog() -> &'static Catalog {
    static DATA: OnceLock<Catalog> = OnceLock::new();
    DATA.get_or_init(|| {
        let doc = crate::ora::read_from(Cursor::new(
            &include_bytes!("../../../../assets/diagram-stencils/agentic-ai.ora")[..],
        ))
        .expect("bundled Agentic AI artwork")
        .doc;
        let mut entries = Vec::new();
        for ((pack, _, _), group) in PACKS.iter().zip(GROUPS) {
            let root = doc
                .nodes
                .iter()
                .find(|n| n.parent.is_none() && n.name == *group)
                .expect("bundled Agentic AI group");
            let mut children = doc
                .nodes
                .iter()
                .filter(|n| {
                    n.parent == Some(root.id)
                        && !n.name.starts_with("Panel –")
                        && !n.name.starts_with("Header –")
                })
                .collect::<Vec<_>>();
            children.sort_by_key(|n| {
                geometry::node_bounds(&doc, n.id)
                    .map(|b| ((b.y + b.h / 2) / 60, b.x))
                    .unwrap_or_default()
            });
            entries.extend(children.into_iter().map(|n| Entry {
                key: format!("{pack}/{}", n.name),
                name: n.name.clone(),
                pack,
                source: n.id,
            }));
        }
        Catalog { doc, entries }
    })
}
pub(super) fn contains(pack: &str) -> bool {
    PACKS.iter().any(|p| p.0 == pack)
}
pub(super) fn entries(pack: &str) -> Vec<&'static str> {
    catalog()
        .entries
        .iter()
        .filter(|e| e.pack == pack)
        .map(|e| e.key.as_str())
        .collect()
}
pub(super) fn name(key: &str) -> Option<&'static str> {
    catalog()
        .entries
        .iter()
        .find(|e| e.key == key)
        .map(|e| e.name.as_str())
}
pub(super) fn document(key: &str) -> Result<Document> {
    let data = catalog();
    let entry = data
        .entries
        .iter()
        .find(|e| e.key == key)
        .ok_or_else(|| IoError::Manifest("Unknown Agentic AI stencil".into()))?;
    let fragment = Fragment::capture(&data.doc, &[entry.source]).map_err(IoError::Manifest)?;
    let bounds = geometry::node_bounds(&data.doc, entry.source)
        .ok_or_else(|| IoError::Manifest("Agentic AI stencil has no artwork".into()))?;
    let mut artwork = Document::new(data.doc.width, data.doc.height);
    artwork.nodes = fragment.nodes;
    artwork.normalize();
    // Keep each icon's aspect ratio, including wide text logos and stroke extents.
    geometry::crop(&mut artwork, bounds, 0.);
    let scale = 100. / f64::from(bounds.w.max(bounds.h).max(1));
    let width = (f64::from(bounds.w) * scale).round().max(1.) as u32;
    let height = (f64::from(bounds.h) * scale).round().max(1.) as u32;
    geometry::resize(&mut artwork, width, height);
    let artwork = Fragment::capture(&artwork, &[entry.source]).map_err(IoError::Manifest)?;
    let x = 20. + (100. - f64::from(width)) / 2.;
    let y = 20. + (100. - f64::from(height)) / 2.;
    let mut builder = Builder::new(140, 160).map_err(IoError::Manifest)?;
    let root = builder
        .add_shape(ShapeKind::Process, [20., 20., 100., 100.], "")
        .map_err(IoError::Manifest)?;
    let mut editor = Editor::new(builder.finish().map_err(IoError::Manifest)?, None);
    artwork
        .paste(&mut editor, Slot::top_of(Some(root)), (x, y))
        .map_err(IoError::Manifest)?;
    editor.doc.node_mut(root).unwrap().name = entry.name.clone();
    let label = emulsion_core::diagram::label_position_command(
        &editor.doc,
        root,
        emulsion_core::diagram::LabelRow::Below,
        emulsion_core::diagram::LabelColumn::Center,
    )
    .map_err(IoError::Manifest)?;
    editor
        .execute(label)
        .map_err(|e| IoError::Manifest(e.to_string()))?;
    let model = Arc::make_mut(editor.doc.diagram.as_mut().unwrap());
    model
        .shapes
        .get_mut(&root)
        .unwrap()
        .data
        .insert("emulsion_stencil".into(), key.into());
    super::finish_artwork(editor.doc, root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::diagram::{self, Endpoint, Port, Routing};

    #[test]
    fn all_agentic_libraries_preserve_editable_visible_artwork() {
        let mut count = 0;
        for ((id, _, _), expected) in PACKS.iter().zip([12, 48, 12, 7, 12]) {
            assert!(super::super::packs().iter().any(|p| p.0 == *id));
            assert!(super::super::is_bundled_tag(&super::super::tag(id)));
            let keys = entries(id);
            assert_eq!(keys.len(), expected);
            count += keys.len();
            for key in keys {
                let doc = document(key).unwrap();
                doc.validate().unwrap();
                assert_eq!(doc.diagram.as_ref().unwrap().shapes.len(), 1, "{key}");
                assert!(
                    doc.nodes.iter().all(|n| matches!(
                        n.kind,
                        NodeKind::Path { .. }
                            | NodeKind::Text { .. }
                            | NodeKind::Group { .. }
                            | NodeKind::Fill { .. }
                    )),
                    "{key}"
                );
                assert!(
                    !doc.nodes
                        .iter()
                        .any(|n| n.name.starts_with("Panel –") || n.name.starts_with("Header –"))
                );
                let pixels = crate::svg_viewport::SvgViewport::new(&doc)
                    .unwrap()
                    .render((140, 160), [1., 0., 0., 1., 0., 0.])
                    .unwrap();
                assert!(
                    pixels
                        .chunks_exact(4)
                        .any(|p| p[3] > 0 && p[..3].iter().any(|c| *c < 220)),
                    "Invisible artwork: {key}"
                );
            }
        }
        assert_eq!(count, 91);
        let cohere = document("agentic-ai-llms/Cohere").unwrap();
        for original in catalog().doc.nodes.iter().filter(|n| n.parent == Some(70)) {
            let copy = cohere
                .nodes
                .iter()
                .find(|n| n.name == original.name)
                .unwrap();
            if let (NodeKind::Path { style: a, .. }, NodeKind::Path { style: b, .. }) =
                (&original.kind, &copy.kind)
            {
                assert_eq!(a.fill, b.fill, "{}", original.name);
                assert_eq!(a.stroke, b.stroke, "{}", original.name);
            }
        }
    }

    #[test]
    fn agentic_pack_roundtrips_and_places_connectable_shapes_with_undo() {
        let (pack, notes) = super::super::build("agentic-ai-llms").unwrap();
        assert!(notes.is_empty());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("llms.emustencil");
        crate::template_pack::write(&pack.project, &pack.manifest, &path).unwrap();
        let reopened = crate::template_pack::read(&path).unwrap();
        for (a, b) in pack.project.pages.iter().zip(&reopened.project.pages) {
            assert_eq!(a.doc, b.doc, "{}", a.meta.name);
        }
        let source = &reopened.project.pages[0].doc;
        let id = *source
            .diagram
            .as_ref()
            .unwrap()
            .shapes
            .keys()
            .next()
            .unwrap();
        let fragment = diagram::document_stencil(source, id).unwrap();
        let mut editor = Editor::new(Document::new(800, 600), None);
        let first = fragment.paste(&mut editor, Slot::TOP, (40., 40.)).unwrap()[0];
        let second = fragment.paste(&mut editor, Slot::TOP, (400., 40.)).unwrap()[0];
        diagram::connect(
            &mut editor,
            Endpoint {
                shape: first,
                port: Port::East,
            },
            Endpoint {
                shape: second,
                port: Port::West,
            },
            "Calls",
            Routing::Orthogonal,
        )
        .unwrap();
        editor.doc.validate().unwrap();
        assert_eq!(editor.doc.diagram.as_ref().unwrap().edges.len(), 1);
        editor.undo();
        assert!(editor.doc.diagram.as_ref().unwrap().edges.is_empty());
        editor.undo();
        assert_eq!(editor.doc.diagram.as_ref().unwrap().shapes.len(), 1);
        let (installed, _) = crate::template_pack::install(dir.path(), reopened).unwrap();
        let asset = installed.assets.first().unwrap();
        assert_eq!(asset.variants.len(), 12);
        assert!(asset.path.parent().unwrap().join("entry-11.png").exists());
    }
}
