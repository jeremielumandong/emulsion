//! Offline vendor packs: the pinned draw.io stencil source plus the official
//! AWS and Azure icon sets (see `scripts/refresh-cloud-stencils.py`).
mod agentic;
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
    io::{Cursor, Read},
    sync::OnceLock,
};
const DRAWIO_PACKS: &[(&str, &str, &str)] = &[
    ("gcp2", "Google Cloud", "Cloud"),
    ("kubernetes", "Kubernetes", "Cloud"),
    ("cisco", "Cisco Network", "Network"),
    ("networks", "Network Devices", "Network"),
    ("bpmn", "BPMN", "Business"),
    ("floorplan", "Floor Plans", "Business"),
    ("electrical", "Electrical", "Business"),
    ("mockup", "Wireframe UI", "UI"),
    ("office", "Office", "Business"),
    ("android", "Android Mockups", "UI"),
    ("basic", "Geometric Shapes", "UI"),
    ("arrows", "Arrows", "UI"),
    ("gmdl", "Material Design", "UI"),
    ("rack", "Server Racks", "Network"),
    ("cabinets", "Cabinets", "Network"),
    ("networks2", "Network Infrastructure", "Network"),
    ("cisco19", "Cisco 2019", "Network"),
    ("cisco_safe", "Cisco SAFE", "Network"),
    ("eip", "Enterprise Integration", "Business"),
    ("pid", "Process Engineering", "Business"),
    ("fluid_power", "Fluid Power", "Business"),
    ("ibm_cloud", "IBM Cloud", "Cloud"),
    ("ibm", "IBM", "Cloud"),
    ("alibaba_cloud", "Alibaba Cloud", "Cloud"),
    ("salesforce", "Salesforce", "Business"),
    ("sitemap", "Site Maps", "UI"),
    ("weblogos", "Web Logos", "UI"),
    ("webicons", "Web Icons", "UI"),
    ("openstack", "OpenStack", "Cloud"),
    ("citrix", "Citrix", "Network"),
    ("citrix2", "Citrix Modern", "Network"),
    ("veeam", "Veeam", "Network"),
    ("veeam2", "Veeam Modern", "Network"),
    ("vvd", "VMware Validated Design", "Network"),
    ("gcp", "Google Cloud Legacy", "Cloud"),
    ("gcp3", "Google Cloud 3", "Cloud"),
    ("kubernetes2", "Kubernetes Modern", "Cloud"),
    ("flowchart", "Flowchart Symbols", "Business"),
    ("bootstrap", "Bootstrap UI", "UI"),
    ("ios7", "iOS Mockups", "UI"),
    ("lean_mapping", "Value Stream", "Business"),
    ("signs", "Signs", "Business"),
];
/// An official vendor icon set bundled as SVG artwork, keyed `"{pack}/{entry}"`.
struct IconPack {
    id: String,
    name: String,
    provider: String,
    keys: Vec<String>,
}
struct Icons {
    packs: Vec<IconPack>,
    svg: BTreeMap<String, String>,
}
fn icons() -> &'static Icons {
    #[derive(serde::Deserialize)]
    struct RawEntry {
        name: String,
        svg: String,
    }
    #[derive(serde::Deserialize)]
    struct RawPack {
        id: String,
        name: String,
        provider: String,
        entries: Vec<RawEntry>,
    }
    static DATA: OnceLock<Icons> = OnceLock::new();
    DATA.get_or_init(|| {
        let mut json = String::new();
        flate2::read::GzDecoder::new(
            &include_bytes!("../../../assets/diagram-stencils/cloud-icons.json.gz")[..],
        )
        .take(64 << 20)
        .read_to_string(&mut json)
        .expect("bundled icon data");
        let mut svg = BTreeMap::new();
        let packs = serde_json::from_str::<Vec<RawPack>>(&json)
            .expect("bundled icon catalog")
            .into_iter()
            .map(|pack| IconPack {
                keys: pack
                    .entries
                    .into_iter()
                    .map(|entry| {
                        let key = format!("{}/{}", pack.id, entry.name);
                        svg.insert(key.clone(), entry.svg);
                        key
                    })
                    .collect(),
                id: pack.id,
                name: pack.name,
                provider: pack.provider,
            })
            .collect();
        Icons { packs, svg }
    })
}
fn icon_pack(id: &str) -> Option<&'static IconPack> {
    icons().packs.iter().find(|p| p.id == id)
}
/// Every bundled pack as `(id, name, category)`: vendor icon sets first, then draw.io families.
pub fn packs() -> &'static [(&'static str, &'static str, &'static str)] {
    static ALL: OnceLock<Vec<(&'static str, &'static str, &'static str)>> = OnceLock::new();
    ALL.get_or_init(|| {
        agentic::PACKS
            .iter()
            .copied()
            .chain(
                icons()
                    .packs
                    .iter()
                    .map(|p| (p.id.as_str(), p.name.as_str(), "Cloud")),
            )
            .chain(DRAWIO_PACKS.iter().copied())
            .collect()
    })
}
/// Catalog tag linking an installed library asset back to its bundled pack.
pub fn tag(id: &str) -> String {
    if is_builtin(id) {
        format!("builtin:{id}")
    } else if icon_pack(id).is_some() {
        format!("icons:{id}")
    } else {
        format!("drawio:{id}")
    }
}
pub fn is_bundled_tag(tag: &str) -> bool {
    tag.starts_with("drawio:")
        || tag.starts_with("icons:")
        || tag.strip_prefix("builtin:").is_some_and(is_builtin)
}
/// Original native artwork listed alongside the standard shape libraries.
pub fn is_builtin(id: &str) -> bool {
    agentic::contains(id)
}
/// Human-readable shape name for an entry key.
pub fn entry_name(key: &str) -> String {
    if key.starts_with("agentic-ai-")
        && let Some(name) = agentic::name(key)
    {
        return name.into();
    }
    if icons().svg.contains_key(key) {
        key.split_once('/')
            .map_or(key, |(_, name)| name)
            .to_string()
    } else {
        key.rsplit('.').next().unwrap_or(key).replace('_', " ")
    }
}
pub fn entries(pack: &str) -> Vec<&'static str> {
    if is_builtin(pack) {
        return agentic::entries(pack);
    }
    if let Some(icons) = icon_pack(pack) {
        return icons.keys.iter().map(String::as_str).collect();
    }
    let prefix = format!("mxgraph.{pack}.");
    let mut names = crate::drawio::vendor::names()
        .filter(|n| n.starts_with(&prefix))
        .collect::<Vec<_>>();
    // Prefer common building blocks while exposing every definition in the family.
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
    names
}
pub fn document(name: &str) -> Result<Document> {
    document_with_notes(name).map(|v| v.0)
}
fn document_with_notes(name: &str) -> Result<(Document, Vec<String>)> {
    if name.starts_with("agentic-ai-") {
        return agentic::document(name).map(|doc| (doc, Vec::new()));
    }
    if let Some(svg) = icons().svg.get(name) {
        return icon_document(name, svg).map(|doc| (doc, Vec::new()));
    }
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
    finish_artwork(doc, id).map(|doc| (doc, warnings.into_iter().collect()))
}
/// Solid-fill icons become native editable paths; gradient artwork (most Azure
/// icons) is retained as scalable SVG so it keeps the vendor's exact appearance.
fn icon_document(key: &str, svg: &str) -> Result<Document> {
    use base64::Engine;
    let bounds = [20., 20., 100., 100.];
    let mut b = Builder::new(140, 140).map_err(IoError::Manifest)?;
    let id = b
        .add_shape(ShapeKind::Process, bounds, "")
        .map_err(IoError::Manifest)?;
    let mut doc = b.finish().map_err(IoError::Manifest)?;
    let first = doc.next_id;
    let uri = format!(
        "data:image/svg+xml;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(svg)
    );
    crate::drawio::images::insert(
        &mut doc,
        id,
        bounds,
        true,
        &uri,
        &mut 16_777_216,
        &mut BTreeSet::new(),
    )?;
    if !doc.nodes.iter().any(|n| n.id >= first && n.visible) {
        return Err(IoError::Unsupported(
            "Icon produced no visible artwork".into(),
        ));
    }
    if let Some(node) = doc.node_mut(id) {
        node.name = entry_name(key);
    }
    // Vendor icons are captioned underneath; the empty label is named on placement.
    let below = emulsion_core::diagram::label_position_command(
        &doc,
        id,
        emulsion_core::diagram::LabelRow::Below,
        emulsion_core::diagram::LabelColumn::Center,
    )
    .map_err(IoError::Manifest)?;
    let (w, h) = (doc.width, doc.height);
    if let emulsion_core::Command::SetText { id: label, spec } = below
        && let Some(emulsion_core::Node {
            kind: NodeKind::Text { spec: text, cache },
            ..
        }) = doc.node_mut(label)
    {
        *text = std::sync::Arc::new(*spec);
        *cache = emulsion_core::vector_cache::VectorRaster::text(text.clone(), w, h);
    }
    finish_artwork(doc, id)
}
/// Hide the placeholder body so only the vendor artwork shows.
fn finish_artwork(mut doc: Document, id: emulsion_core::NodeId) -> Result<Document> {
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
    Ok(doc)
}
pub fn build(id: &str) -> Result<(Pack, Vec<String>)> {
    let (_, name, category) = packs()
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
                        name: entry_name(key),
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
    let mut manifest;
    if is_builtin(id) {
        manifest = Manifest::new(Kind::Stencil, name.to_string());
        manifest.tags = vec![category.to_string(), "AI".into(), tag(id)];
        manifest.description = "Editable icons from the Agentic AI collection, preserving the supplied artwork and colors.".into();
    } else if let Some(icons) = icon_pack(id) {
        manifest = Manifest::new(Kind::Stencil, name.to_string());
        manifest.tags = vec![category.to_string(), icons.provider.clone(), tag(id)];
        manifest.author = match icons.provider.as_str() {
            "AWS" => "Amazon Web Services · Architecture Icons".into(),
            _ => "Microsoft · Azure Public Service Icons".into(),
        };
        manifest.license = "Official vendor architecture icons, used under the vendor's icon terms; trademarks belong to their owners.".into();
    } else {
        manifest = Manifest::new(Kind::Stencil, format!("{name} · draw.io"));
        manifest.tags = vec![category.to_string(), tag(id)];
        manifest.author =
            "draw.io contributors · pinned source 0f419a92c769adb5fb20f2b18053a5ae8c7e4993".into();
        manifest.license="See bundled draw.io Apache license and stencil asset terms; vendor trademarks belong to their owners.".into();
    }
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
        for (id, _, _) in DRAWIO_PACKS {
            let (pack, notes) = build(id).unwrap_or_else(|e| panic!("{id}: {e}"));
            assert!(!pack.project.pages.is_empty(), "{id}: {notes:?}");
            assert!(pack.project.pages.len() <= emulsion_core::project::MAX_PAGES);
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
    fn every_vendor_icon_builds_with_its_name() {
        let ids = packs().iter().map(|p| p.0).collect::<Vec<_>>();
        assert!(
            !ids.iter()
                .any(|id| id.starts_with("aws4") || *id == "azure" || *id == "mscae")
        );
        for pack in &icons().packs {
            let (built, notes) = build(&pack.id).unwrap_or_else(|e| panic!("{}: {e}", pack.id));
            assert_eq!(
                built.project.pages.len(),
                pack.keys.len(),
                "{}: {notes:?}",
                pack.id
            );
            assert!(!built.manifest.name.contains("draw.io"));
            assert!(built.manifest.tags.iter().any(|t| is_bundled_tag(t)));
            for page in &built.project.pages {
                assert!(!page.meta.name.is_empty() && !page.meta.name.contains('/'));
            }
        }
        let native = document("aws-compute/Amazon EC2").unwrap();
        assert!(native.nodes.iter().any(|n| n.name == "Amazon EC2"));
        assert!(
            native
                .nodes
                .iter()
                .filter(|n| matches!(n.kind, NodeKind::Path { .. }))
                .count()
                > 1
        );
        let gradient = entries("azure-compute")[0];
        assert!(
            document(gradient)
                .unwrap()
                .nodes
                .iter()
                .any(|n| matches!(n.kind, NodeKind::Smart { .. } | NodeKind::Path { .. }))
        );
    }
    #[test]
    fn installed_pack_has_per_entry_preview_and_native_artwork() {
        let (mut pack, _) = build("gcp2").unwrap();
        assert!(pack.project.pages.len() > 101);
        pack.project.pages.truncate(101);
        let mut bytes = Cursor::new(Vec::new());
        crate::project::write_to(&pack.project, &mut bytes).unwrap();
        pack.project_bytes = bytes.into_inner();
        let dir = tempfile::tempdir().unwrap();
        let (catalog, _) = crate::template_pack::install(dir.path(), pack).unwrap();
        let asset = catalog.assets.first().unwrap();
        assert_eq!(asset.variants.len(), 101);
        assert!(asset.path.parent().unwrap().join("entry-100.png").exists());
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
