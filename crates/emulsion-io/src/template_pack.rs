//! Portable, versioned local template/stencil packs, including GitHub installation.
//! Packages contain data only. Repository scripts are never invoked.
use crate::{
    IoError, Result,
    creative_library::{self as library, AssetKind, Catalog},
    project,
};
use emulsion_core::project::{Project, ProjectKind};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    io::{Cursor, Read, Seek, Write},
    path::Path,
};
const MAX_PACK: u64 = 256 << 20;
const MANIFEST: &str = "emulsion-template.json";
fn error(s: impl Into<String>) -> IoError {
    IoError::Manifest(s.into())
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Design,
    Stencil,
}
impl Kind {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Design => "emutemplate",
            Self::Stencil => "emustencil",
        }
    }
    pub fn asset(self) -> AssetKind {
        match self {
            Self::Design => AssetKind::Template,
            Self::Stencil => AssetKind::Stencil,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub format_version: u32,
    pub kind: Kind,
    pub name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub project: String,
    #[serde(default)]
    pub preview: Option<String>,
}
impl Manifest {
    pub fn new(kind: Kind, name: String) -> Self {
        Self {
            format_version: 1,
            kind,
            name,
            author: String::new(),
            license: String::new(),
            description: String::new(),
            tags: Vec::new(),
            project: "project.emu".into(),
            preview: None,
        }
    }
    fn validate(&self) -> Result<()> {
        if self.format_version != 1 {
            return Err(error("Unsupported template package version."));
        }
        if self.name.trim().is_empty()
            || self.name.chars().count() > 200
            || self.name.chars().any(char::is_control)
            || self.author.len() > 2000
            || self.license.len() > 4000
            || self.description.len() > 8000
            || self.tags.len() > 50
            || self
                .tags
                .iter()
                .any(|s| s.is_empty() || s.chars().count() > 200 || s.chars().any(char::is_control))
        {
            return Err(error("Invalid template metadata."));
        }
        safe_path(&self.project)?;
        if let Some(preview) = &self.preview {
            safe_path(preview)?;
        }
        Ok(())
    }
}
pub struct Pack {
    pub manifest: Manifest,
    pub project: Project,
    pub preview: Option<Vec<u8>>,
    pub(crate) project_bytes: Vec<u8>,
}
fn safe_path(s: &str) -> Result<()> {
    if s.is_empty()
        || s.len() > 1024
        || s.contains(['\\', ':'])
        || s.starts_with('/')
        || s.split('/').any(|p| p == ".." || p == "." || p.is_empty())
    {
        return Err(error("Unsafe package path."));
    }
    Ok(())
}
pub fn is_pack(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|e| {
        e.eq_ignore_ascii_case("emutemplate") || e.eq_ignore_ascii_case("emustencil")
    })
}
pub fn read(path: &Path) -> Result<Pack> {
    let f = std::fs::File::open(path)?;
    if f.metadata()?.len() > MAX_PACK {
        return Err(error("Template package exceeds 256 MiB."));
    }
    read_archive(f, None)
}
/// Convert supported local stencil sources into the same reusable offline pack
/// format as authored packages. No remote image or catalog URLs are fetched.
pub fn read_stencil_source(path: &Path) -> Result<(Pack, Vec<String>)> {
    if is_pack(path) {
        return Ok((read(path)?, Vec::new()));
    }
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Imported stencils")
        .chars()
        .take(200)
        .collect::<String>();
    let (project, warnings) = if path.is_dir()
        || path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
    {
        let mut paths = if path.is_dir() {
            let mut paths = std::fs::read_dir(path)?
                .map(|entry| entry.map(|e| e.path()))
                .collect::<std::io::Result<Vec<_>>>()?;
            paths.retain(|p| {
                p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg"))
            });
            paths.sort();
            paths
        } else {
            vec![path.to_path_buf()]
        };
        if paths.is_empty() || paths.len() > emulsion_core::project::MAX_PAGES {
            return Err(error(
                "SVG folder entry count exceeds the project page limit.",
            ));
        }
        let mut pages = Vec::new();
        for (index, path) in paths.drain(..).enumerate() {
            let mut doc = crate::open(&path)?;
            // Ordinary SVG artwork also needs a graph object so toolbox drops,
            // picking, moving and attached connectors work like native stencils.
            if doc.diagram.is_none() {
                use emulsion_core::{
                    Command, Editor, NodeKind,
                    command::Slot,
                    diagram::{self, ShapeKind},
                };
                let roots = doc.children(None);
                let (w, h) = (doc.width as f64, doc.height as f64);
                let mut editor = Editor::new(doc, None);
                let group = diagram::add_shape(&mut editor, ShapeKind::Process, [0., 0., w, h], "")
                    .map_err(error)?;
                let body = editor.doc.diagram.as_ref().unwrap().shapes[&group].body;
                if let NodeKind::Path { path, style, .. } = &editor.doc.node(body).unwrap().kind {
                    let mut style = *style;
                    style.fill = None;
                    style.stroke = None;
                    editor
                        .execute(Command::SetPath {
                            id: body,
                            path: path.clone(),
                            style,
                        })
                        .map_err(|e| error(e.to_string()))?;
                }
                for id in roots {
                    editor
                        .execute(Command::MoveNode {
                            id,
                            slot: Slot::top_of(Some(group)),
                        })
                        .map_err(|e| error(e.to_string()))?;
                }
                doc = editor.doc;
            }
            let id = index as u64 + 1;
            let title = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Stencil")
                .chars()
                .take(200)
                .collect();
            pages.push(emulsion_core::project::ProjectPage {
                meta: emulsion_core::project::PageMeta {
                    id,
                    name: title,
                    bleed_mm: 0.,
                },
                graph: emulsion_core::graph::Graph::new(doc.clone(), "Imported stencil"),
                doc,
            });
        }
        let next_page_id = pages.len() as u64 + 1;
        (
            Project {
                kind: ProjectKind::Diagram,
                pages,
                active: 1,
                next_page_id,
            },
            Vec::new(),
        )
    } else {
        let imported = crate::diagram_import::read(path)?;
        (imported.project, imported.warnings)
    };
    project.validate().map_err(error)?;
    let mut bytes = Cursor::new(Vec::new());
    project::write_to(&project, &mut bytes)?;
    if bytes.get_ref().len() as u64 > MAX_PACK {
        return Err(error("Stencil content exceeds 256 MiB."));
    }
    let manifest = Manifest::new(Kind::Stencil, name);
    manifest.validate()?;
    Ok((
        Pack {
            manifest,
            project,
            preview: None,
            project_bytes: bytes.into_inner(),
        },
        warnings,
    ))
}

fn read_archive<R: Read + Seek>(reader: R, prefix: Option<&str>) -> Result<Pack> {
    let mut zip = zip::ZipArchive::new(reader)?;
    if zip.len() > 10_000 {
        return Err(error("Too many package entries."));
    }
    let mut names = HashSet::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let e = zip.by_index(i)?;
        let name = e.name().to_string();
        if e.enclosed_name().is_none() || name.contains('\\') || !names.insert(name) {
            return Err(error("Unsafe or duplicate archive entry."));
        }
        total = total
            .checked_add(e.size())
            .ok_or_else(|| error("Package size overflow."))?;
        if total > MAX_PACK {
            return Err(error("Decoded template package exceeds 256 MiB."));
        }
    }
    let root = if let Some(prefix) = prefix {
        let mut roots = names
            .iter()
            .filter(|name| name.ends_with(MANIFEST))
            .filter_map(|name| {
                let (_, rest) = name.split_once('/')?;
                (rest == format!("{prefix}{MANIFEST}"))
                    .then(|| name.trim_end_matches(MANIFEST).to_string())
            });
        let root = roots.next().ok_or_else(|| {
            error("No emulsion-template.json in the selected repository directory.")
        })?;
        if roots.next().is_some() {
            return Err(error("Ambiguous template repository root."));
        }
        root
    } else {
        String::new()
    };
    let read_entry = |zip: &mut zip::ZipArchive<R>, name: &str, limit: u64| -> Result<Vec<u8>> {
        let mut b = Vec::new();
        zip.by_name(name)?.take(limit + 1).read_to_end(&mut b)?;
        if b.len() as u64 > limit {
            return Err(error("Template entry exceeds its size limit."));
        }
        Ok(b)
    };
    let manifest: Manifest = serde_json::from_slice(&read_entry(
        &mut zip,
        &format!("{root}{MANIFEST}"),
        64 << 10,
    )?)
    .map_err(|e| error(e.to_string()))?;
    manifest.validate()?;
    let project_bytes = read_entry(&mut zip, &format!("{root}{}", manifest.project), MAX_PACK)?;
    let project = project::read_from(Cursor::new(&project_bytes))?;
    if (manifest.kind == Kind::Stencil) != (project.kind == ProjectKind::Diagram) {
        return Err(error("Template kind does not match the native project."));
    }
    let preview = manifest
        .preview
        .as_ref()
        .map(|p| read_entry(&mut zip, &format!("{root}{p}"), 8 << 20))
        .transpose()?;
    if let Some(preview) = &preview {
        crate::import::import_bytes("Preview", preview)?;
    }
    Ok(Pack {
        manifest,
        project,
        preview,
        project_bytes,
    })
}
/// Build a shareable stencil snapshot without dropping referenced clipping bases.
/// Removing raw Fill nodes alone left dangling clip/design references in real diagrams.
pub fn stencil_project(project: &Project) -> Result<Project> {
    use emulsion_core::NodeKind;
    let mut shared = project.clone();
    if shared.kind != ProjectKind::Diagram {
        return Err(error("Stencil export requires a Diagram project."));
    }
    for page in &mut shared.pages {
        let clips = page
            .doc
            .nodes
            .iter()
            .filter_map(|n| n.clip_to)
            .collect::<HashSet<_>>();
        page.doc.nodes.retain(|n| {
            !(n.parent.is_none()
                && matches!(n.kind, NodeKind::Fill { .. })
                && n.mask.is_none()
                && n.styles.is_empty()
                && !clips.contains(&n.id))
        });
        let ids = page.doc.nodes.iter().map(|n| n.id).collect();
        page.doc.design.retain_nodes(&ids);
        page.doc.validate().map_err(|e| error(e.to_string()))?;
        page.graph = emulsion_core::graph::Graph::new(page.doc.clone(), "Stencil");
    }
    if shared.pages.iter().all(|p| p.doc.nodes.is_empty()) {
        return Err(error(
            "There is no artwork to export. Place shapes on a diagram page first.",
        ));
    }
    shared.validate().map_err(error)?;
    Ok(shared)
}

pub fn write(project: &Project, manifest: &Manifest, path: &Path) -> Result<()> {
    manifest.validate()?;
    project.validate().map_err(error)?;
    for p in &project.pages {
        crate::ora::ensure_not_raw_original(&p.doc, path)?;
        for commit in p.graph.commits() {
            crate::ora::ensure_not_raw_original(&commit.doc, path)?;
        }
    }
    if (manifest.kind == Kind::Stencil) != (project.kind == ProjectKind::Diagram) {
        return Err(error(
            "Use a Diagram project for stencils and a Design project for templates.",
        ));
    }
    // Shared packs carry current artwork, never private local version history.
    let mut shared = project.clone();
    for p in &mut shared.pages {
        p.graph = emulsion_core::graph::Graph::new(p.doc.clone(), "Template");
    }
    let mut bytes = Cursor::new(Vec::new());
    project::write_to(&shared, &mut bytes)?;
    if bytes.get_ref().len() as u64 > MAX_PACK {
        return Err(error("Template content exceeds 256 MiB."));
    }
    let mut manifest = manifest.clone();
    manifest.project = "project.emu".into();
    let mut native = zip::ZipArchive::new(Cursor::new(bytes.get_ref()))?;
    let first = shared
        .pages
        .first()
        .ok_or_else(|| error("Empty template project."))?;
    let page = crate::ora::read_entry(
        &mut native,
        &format!("pages/{}.ora", first.meta.id),
        MAX_PACK,
    )?;
    let mut page = zip::ZipArchive::new(Cursor::new(page))?;
    let preview = crate::ora::read_entry(&mut page, "Thumbnails/thumbnail.png", 8 << 20)?;
    manifest.preview = Some("preview.png".into());
    let metadata = serde_json::to_vec_pretty(&manifest).map_err(|e| error(e.to_string()))?;
    crate::write_atomic(path, |file| {
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file(MANIFEST, options)?;
        zip.write_all(&metadata)?;
        zip.start_file("project.emu", options)?;
        zip.write_all(bytes.get_ref())?;
        zip.start_file("preview.png", options)?;
        zip.write_all(&preview)?;
        zip.finish()?;
        Ok(())
    })
}
pub fn install(root: &Path, pack: Pack) -> Result<(Catalog, u64)> {
    let mut hasher = Sha256::new();
    hasher.update(&pack.project_bytes);
    hasher.update(serde_json::to_vec(&pack.manifest).map_err(|e| error(e.to_string()))?);
    let hash = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let dir = root.join("packs").join(hash);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("project.emu");
    crate::write_atomic(&path, |f| {
        f.write_all(&pack.project_bytes)?;
        Ok(())
    })?;
    let meta = serde_json::to_vec_pretty(&pack.manifest).map_err(|e| error(e.to_string()))?;
    crate::write_atomic(&dir.join(MANIFEST), |f| {
        f.write_all(&meta)?;
        Ok(())
    })?;
    if let Some(preview) = pack.preview {
        crate::write_atomic(&dir.join("preview.png"), |f| {
            f.write_all(&preview)?;
            Ok(())
        })?;
    }
    // Previews use the same vector renderer as the canvas, at thumbnail resolution.
    for (index, page) in pack.project.pages.iter().enumerate() {
        if let Ok(scene) = crate::svg_viewport::SvgViewport::new(&page.doc) {
            let scale = 192. / (page.doc.width.max(page.doc.height) as f64);
            if let Ok(mut bytes) = scene.render(
                (192, 192),
                [
                    scale,
                    0.,
                    0.,
                    scale,
                    (192. - page.doc.width as f64 * scale) / 2.,
                    (192. - page.doc.height as f64 * scale) / 2.,
                ],
            ) {
                for p in bytes.as_chunks_mut::<4>().0 {
                    p.swap(0, 2);
                    if p[3] > 0 {
                        for i in 0..3 {
                            p[i] = (p[i] as u32 * 255 / p[3] as u32).min(255) as u8;
                        }
                    }
                }
                if let Ok(png) = crate::export::png8(192, 192, &bytes) {
                    crate::write_atomic(&dir.join(format!("entry-{index}.png")), |f| {
                        f.write_all(&png)?;
                        Ok(())
                    })?;
                }
            }
        }
    }
    library::update(root, |c| {
        let id = c.add_asset(path, pack.manifest.kind.asset())?;
        let asset = c.assets.iter_mut().find(|a| a.id == id).unwrap();
        asset.name = pack.manifest.name;
        asset.tags = pack.manifest.tags;
        asset.attribution = pack.manifest.author;
        asset.license = pack.manifest.license;
        asset.variants = pack
            .project
            .pages
            .iter()
            .map(|p| p.meta.name.clone())
            .collect();
        Ok(id)
    })
}
#[derive(Debug, PartialEq, Eq)]
pub struct GithubSource {
    pub download: String,
    pub directory: String,
}
impl GithubSource {
    pub fn parse(url: &str) -> Result<Self> {
        let path = url
            .trim()
            .strip_prefix("https://github.com/")
            .ok_or_else(|| error("Enter an HTTPS github.com repository URL."))?;
        if path.contains(['?', '#', '%', '\\', ':', '@']) {
            return Err(error(
                "Use a repository or /tree/<ref>/<directory> URL without query parameters.",
            ));
        }
        let parts = path.trim_end_matches('/').split('/').collect::<Vec<_>>();
        if parts.len() < 2
            || parts.iter().any(|p| {
                p.is_empty()
                    || *p == "."
                    || *p == ".."
                    || !p
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
            })
        {
            return Err(error("Invalid GitHub repository path."));
        }
        let owner = parts[0];
        let repo = parts[1].trim_end_matches(".git");
        if repo.is_empty() {
            return Err(error("Missing GitHub repository name."));
        }
        let (reference, directory) = match parts.as_slice() {
            [_, _] => ("", String::new()),
            [_, _, "tree", reference, rest @ ..] => {
                let d = if rest.is_empty() {
                    String::new()
                } else {
                    format!("{}/", rest.join("/"))
                };
                (*reference, d)
            }
            _ => {
                return Err(error(
                    "Use the repository URL or its /tree/<ref>/<directory> page.",
                ));
            }
        };
        Ok(Self {
            download: format!("https://api.github.com/repos/{owner}/{repo}/zipball/{reference}"),
            directory,
        })
    }
}
pub fn download_github(url: &str) -> Result<Pack> {
    let source = GithubSource::parse(url)?;
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .https_only(true)
            .timeout_global(Some(std::time::Duration::from_secs(60)))
            .build(),
    );
    let response = agent
        .get(&source.download)
        .header("User-Agent", "Emulsion-template-installer")
        .call()
        .map_err(|e| error(format!("GitHub download failed: {e}")))?;
    let mut bytes = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(MAX_PACK + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_PACK {
        return Err(error("Repository download exceeds 256 MiB."));
    }
    read_archive(Cursor::new(bytes), Some(&source.directory))
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::{NodeKind, design::Template, project::ProjectEditor};
    fn folder(label: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("emulsion-packs-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
    #[test]
    fn stencil_export_requires_artwork_and_preserves_source() {
        let blank = ProjectEditor::new_project(ProjectKind::Diagram,
            emulsion_core::Document::new(400, 240)).unwrap().snapshot().unwrap();
        assert!(stencil_project(&blank).is_err());
        let mut builder = emulsion_core::diagram::Builder::new(400, 240).unwrap();
        builder.add_shape(emulsion_core::diagram::ShapeKind::Process,
            [20., 20., 120., 60.], "Service").unwrap();
        let original = ProjectEditor::new_project(ProjectKind::Diagram,
            builder.finish().unwrap()).unwrap().snapshot().unwrap();
        let before = original.clone();
        let prepared = stencil_project(&original).unwrap();
        assert_eq!(original.pages[0].doc, before.pages[0].doc);
        assert_eq!(prepared.pages[0].doc.diagram, original.pages[0].doc.diagram);
        assert!(prepared.pages[0].doc.nodes.iter().all(|n| !matches!(n.kind, NodeKind::Fill { .. })));
        let root = folder("export-preparation");
        let path = root.join("service.emustencil");
        write(&prepared, &Manifest::new(Kind::Stencil, "Service".into()), &path).unwrap();
        assert_eq!(read(&path).unwrap().project.pages[0].doc, prepared.pages[0].doc);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn portable_templates_preserve_pages_sources_and_install_offline() {
        let root = folder("roundtrip");
        let mut editor = ProjectEditor::new_project(
            ProjectKind::Design,
            Template::Editorial.create(160, 120).unwrap(),
        )
        .unwrap();
        editor.create_version("Private draft");
        let second = editor.duplicate_page(1).unwrap();
        editor.rename_page(second, "Reverse".into(), 3.).unwrap();
        let before = editor.snapshot().unwrap();
        let file = root.join("sample.emutemplate");
        let mut meta = Manifest::new(Kind::Design, "Editorial sample".into());
        meta.author = "Example author".into();
        meta.license = "CC0-1.0".into();
        meta.tags = vec!["print".into()];
        write(&before, &meta, &file).unwrap();
        let pack = read(&file).unwrap();
        assert!(pack.preview.is_some());
        assert_eq!(pack.project.pages.len(), 2);
        for (a, b) in before.pages.iter().zip(&pack.project.pages) {
            assert_eq!(a.doc, b.doc);
            assert_eq!(a.meta, b.meta);
            assert_eq!(b.graph.len(), 1);
            assert!(
                b.doc
                    .nodes
                    .iter()
                    .any(|n| matches!(n.kind, NodeKind::Text { .. }))
            );
        }
        let (catalog, id) = install(&root, pack).unwrap();
        let asset = catalog.assets.iter().find(|a| a.id == id).unwrap();
        assert_eq!(asset.attribution, meta.author);
        assert_eq!(asset.variants[1], "Reverse");
        assert_eq!(project::read(&asset.path).unwrap().pages.len(), 2);
        // Reinstall repairs the managed copy and never creates a duplicate catalog item.
        std::fs::write(&asset.path, b"damaged").unwrap();
        let (again, same_id) = install(&root, read(&file).unwrap()).unwrap();
        assert_eq!(same_id, id);
        assert_eq!(again.assets.len(), 1);
        assert!(project::read(&asset.path).is_ok());
        std::fs::remove_file(&file).unwrap();
        assert!(project::read(&asset.path).is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn stencil_connections_remain_editable_after_file_roundtrip() {
        use emulsion_core::diagram::{Builder, Endpoint, Port, Routing, ShapeKind};
        let root = folder("stencil");
        let mut b = Builder::new(400, 240).unwrap();
        let a = b
            .add_shape(ShapeKind::Process, [10., 20., 100., 50.], "Start")
            .unwrap();
        let z = b
            .add_shape(ShapeKind::Decision, [220., 20., 100., 70.], "Ready?")
            .unwrap();
        b.connect(
            Endpoint {
                shape: a,
                port: Port::East,
            },
            Endpoint {
                shape: z,
                port: Port::West,
            },
            "Next",
            Routing::Orthogonal,
        )
        .unwrap();
        let project = ProjectEditor::new_project(ProjectKind::Diagram, b.finish().unwrap())
            .unwrap()
            .snapshot()
            .unwrap();
        let file = root.join("sample.emustencil");
        write(
            &project,
            &Manifest::new(Kind::Stencil, "Workflow".into()),
            &file,
        )
        .unwrap();
        let pack = read(&file).unwrap();
        assert_eq!(pack.project.pages[0].doc, project.pages[0].doc);
        assert_eq!(
            pack.project.pages[0]
                .doc
                .diagram
                .as_ref()
                .unwrap()
                .edges
                .len(),
            1
        );
        assert!(
            write(
                &project,
                &Manifest::new(Kind::Design, "Wrong".into()),
                &file
            )
            .is_err()
        );
        assert!(read(&file).is_ok()); // A failed export did not replace the file.
        std::fs::remove_dir_all(root).unwrap();
    }
    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            z.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            z.write_all(bytes).unwrap();
        }
        z.finish().unwrap().into_inner()
    }
    #[test]
    fn repository_archive_selects_directory_and_rejects_unsafe_manifests() {
        let root = folder("repository");
        let file = root.join("pack.emutemplate");
        let project = ProjectEditor::new_project(
            ProjectKind::Design,
            Template::Editorial.create(80, 60).unwrap(),
        )
        .unwrap()
        .snapshot()
        .unwrap();
        write(&project, &Manifest::new(Kind::Design, "Demo".into()), &file).unwrap();
        let pack = read(&file).unwrap();
        let mut manifest = pack.manifest;
        manifest.preview = None;
        let meta = serde_json::to_vec(&manifest).unwrap();
        let data = archive(&[
            ("owner-repo-sha/packs/demo/emulsion-template.json", &meta),
            ("owner-repo-sha/packs/demo/project.emu", &pack.project_bytes),
        ]);
        assert!(read_archive(Cursor::new(&data), Some("packs/demo/")).is_ok());
        assert!(read_archive(Cursor::new(&data), Some("")).is_err());
        manifest.project = "../outside.emu".into();
        let meta = serde_json::to_vec(&manifest).unwrap();
        let data = archive(&[(MANIFEST, &meta)]);
        assert!(read_archive(Cursor::new(data), None).is_err());
        let data = archive(&[("../outside", b"bad")]);
        assert!(read_archive(Cursor::new(data), None).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn github_urls_are_strict_and_support_pinned_subdirectories() {
        let a = GithubSource::parse("https://github.com/artist/templates.git").unwrap();
        assert_eq!(
            a.download,
            "https://api.github.com/repos/artist/templates/zipball/"
        );
        let a = GithubSource::parse("https://github.com/artist/templates/tree/abc123/packs/flow/")
            .unwrap();
        assert_eq!(a.directory, "packs/flow/");
        assert!(a.download.ends_with("/abc123"));
        for url in [
            "http://github.com/a/b",
            "https://github.com.evil.test/a/b",
            "https://github.com/a/b/tree/main/../secret",
            "https://github.com/a/b?x=y",
            "https://github.com/a/b/blob/main/file",
            "https://github.com/a/b/tree/main/%2e%2e",
        ] {
            assert!(GithubSource::parse(url).is_err(), "{url}");
        }
    }
    #[test]
    fn svg_folder_installs_reusable_editable_stencil_entries() {
        let root =
            std::env::temp_dir().join(format!("emulsion-diagram-pack-{}", std::process::id()));
        let source = root.join("source");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("Box.svg"),r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="60"><rect x="1" y="1" width="98" height="58" fill="#ff8800"/></svg>"##).unwrap();
        let (pack, warnings) = read_stencil_source(&source).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(pack.project.pages.len(), 1);
        let doc = &pack.project.pages[0].doc;
        doc.validate().unwrap();
        assert_eq!(doc.diagram.as_ref().unwrap().shapes.len(), 1);
        let (catalog, id) = install(&root.join("library"), pack).unwrap();
        let asset = catalog.assets.iter().find(|a| a.id == id).unwrap();
        assert_eq!(asset.variants, vec!["Box"]);
        assert!(asset.path.exists());
        project::read(&asset.path).unwrap().validate().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
