//! Atomic multi-page `.emu` packages. Every page is a complete native ORA,
//! including editable text/vector sources and its branch/version graph.
use crate::{IoError, Result, audio::store as audio, ora};
use emulsion_core::project::{
    MAX_PAGES, MAX_PROJECT_PIXELS, PageMeta, Project, ProjectKind, ProjectPage,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{Cursor, Read, Seek, Write};
use std::path::Path;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const VERSION: u32 = 1;
const MIME: &[u8] = b"application/x-emulsion-project";
const MAX_BYTES: u64 = 2 << 30;
const MAX_MANIFEST: u64 = 1 << 20;
/// Captions for thousands of panels outgrow the page manifest's budget.
const MAX_STORYBOARD: u64 = 64 << 20;
const STORYBOARD_ENTRY: &str = "storyboard.json";

#[derive(Serialize, Deserialize)]
struct Manifest {
    version: u32,
    kind: ProjectKind,
    active: u64,
    next_page_id: u64,
    pages: Vec<PageRecord>,
}
#[derive(Serialize, Deserialize)]
struct PageRecord {
    meta: PageMeta,
    width: u32,
    height: u32,
}

fn library_entry(id: u64) -> String {
    format!("library/{id}.ora")
}

pub fn is_project(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("emu"))
}

pub fn write(project: &Project, path: &Path) -> Result<()> {
    project.validate().map_err(IoError::Manifest)?;
    for page in &project.pages {
        ora::ensure_not_raw_original(&page.doc, path)?;
        for commit in page.graph.commits() {
            ora::ensure_not_raw_original(&commit.doc, path)?;
        }
    }
    crate::write_atomic(path, |file| write_to(project, file))
}

/// Stream a native project into another bounded package without temporary files.
pub fn write_to<W: Write + Seek>(project: &Project, writer: W) -> Result<()> {
    project.validate().map_err(IoError::Manifest)?;
    let manifest = Manifest {
        version: VERSION,
        kind: project.kind,
        active: project.active,
        next_page_id: project.next_page_id,
        pages: project
            .pages
            .iter()
            .map(|p| PageRecord {
                meta: p.meta.clone(),
                width: p.doc.width,
                height: p.doc.height,
            })
            .collect(),
    };
    let manifest = serde_json::to_vec(&manifest).map_err(|e| IoError::Manifest(e.to_string()))?;
    let mut zip = ZipWriter::new(writer);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    zip.start_file("mimetype", stored)?;
    zip.write_all(MIME)?;
    zip.start_file("project.json", stored)?;
    zip.write_all(&manifest)?;
    if let Some(board) = &project.storyboard {
        let bytes = serde_json::to_vec(board).map_err(|e| IoError::Manifest(e.to_string()))?;
        if bytes.len() as u64 > MAX_STORYBOARD {
            return Err(IoError::Manifest("Storyboard data exceeds 64 MiB.".into()));
        }
        zip.start_file(STORYBOARD_ENTRY, SimpleFileOptions::default())?;
        zip.write_all(&bytes)?;
        write_audio(&mut zip, &board.timeline)?;
    }
    let mut total = 0u64;
    // Project library drawings are native ORA documents beside the panels.
    for item in project.storyboard.iter().flat_map(|b| &b.library.items) {
        let mut bytes = Cursor::new(Vec::new());
        ora::write_to(&item.doc, None, &mut bytes)?;
        total = total
            .checked_add(check_archive(&mut ZipArchive::new(Cursor::new(
                bytes.get_ref(),
            ))?)?)
            .ok_or_else(|| IoError::Manifest("Project size overflow.".into()))?;
        if total > MAX_BYTES {
            return Err(IoError::Manifest(
                "Project exceeds the 2 GiB decoded archive budget.".into(),
            ));
        }
        zip.start_file(library_entry(item.id), stored.large_file(true))?;
        zip.write_all(bytes.get_ref())?;
    }
    for page in &project.pages {
        let mut bytes = Cursor::new(Vec::new());
        ora::write_to(&page.doc, Some(&page.graph), &mut bytes)?;
        // Bound both the outer package and the uncompressed nested entries.
        total = total
            .checked_add(check_archive(&mut ZipArchive::new(Cursor::new(
                bytes.get_ref(),
            ))?)?)
            .ok_or_else(|| IoError::Manifest("Project size overflow.".into()))?;
        if total > MAX_BYTES || bytes.get_ref().len() as u64 > MAX_BYTES {
            return Err(IoError::Manifest(
                "Project exceeds the 2 GiB decoded archive budget.".into(),
            ));
        }
        zip.start_file(
            format!("pages/{}.ora", page.meta.id),
            stored.large_file(true),
        )?;
        zip.write_all(bytes.get_ref())?;
    }
    zip.finish()?.flush()?;
    Ok(())
}

/// Sound files of a timeline, streamed from their cache files into
/// `audio/{id}.{format}` entries within the package's audio budget.
fn write_audio<W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    timeline: &emulsion_core::timeline::Timeline,
) -> Result<()> {
    let stored = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .large_file(true);
    let mut total = 0u64;
    for (id, asset) in &timeline.assets {
        let missing = || {
            IoError::Manifest(format!(
                "The sound “{}” has no file; import it again before saving.",
                asset.name
            ))
        };
        let source = asset.source.as_deref().ok_or_else(missing)?;
        let file = std::fs::File::open(source).map_err(|_| missing())?;
        let size = file.metadata()?.len();
        total = total.saturating_add(size);
        if total > audio::MAX_PACKAGE_AUDIO {
            return Err(IoError::Manifest(
                "Sounds exceed the 2 GiB a project can hold.".into(),
            ));
        }
        zip.start_file(audio::entry_name(*id, &asset.format), stored)?;
        let copied = std::io::copy(&mut file.take(size), zip)?;
        if copied != size {
            return Err(IoError::Manifest(format!(
                "The sound “{}” changed while saving.",
                asset.name
            )));
        }
    }
    Ok(())
}

/// Extract every sound of a timeline from the package into the media cache
/// and point the assets at those files.
fn read_audio<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    timeline: &mut emulsion_core::timeline::Timeline,
) -> Result<()> {
    // Formats name cache files, so check them before any are made.
    timeline.validate().map_err(IoError::Manifest)?;
    let mut total = 0u64;
    let mut extracted = Vec::new();
    let result = (|| {
        for (id, asset) in &mut timeline.assets {
            let name = audio::entry_name(*id, &asset.format);
            let entry = zip.by_name(&name).map_err(|_| {
                IoError::Manifest(format!("The sound “{}” is missing.", asset.name))
            })?;
            total = total.saturating_add(entry.size());
            if total > audio::MAX_PACKAGE_AUDIO {
                return Err(IoError::Manifest(
                    "Sounds exceed the 2 GiB a project can hold.".into(),
                ));
            }
            let size = entry.size();
            let (path, copied) = audio::copy_to_cache(entry, &asset.format, size)
                .map_err(|e| IoError::Manifest(format!("Sound “{}”: {e}", asset.name)))?;
            extracted.push(path.clone());
            if copied != size {
                return Err(IoError::Manifest(format!(
                    "The sound “{}” is damaged.",
                    asset.name
                )));
            }
            asset.source = Some(path);
        }
        Ok(())
    })();
    if result.is_err() {
        for path in extracted {
            let _ = std::fs::remove_file(path);
        }
    }
    result
}

fn check_archive<R: Read + Seek>(zip: &mut ZipArchive<R>) -> Result<u64> {
    check_entries(zip, false)
}

/// Check entry names and the decoded size budget. With `audio`, `audio/`
/// entries (a package's sounds) count against their own budget instead.
fn check_entries<R: Read + Seek>(zip: &mut ZipArchive<R>, audio: bool) -> Result<u64> {
    if zip.len() > 100_000 {
        return Err(IoError::Manifest("Too many archive entries.".into()));
    }
    let mut names = HashSet::new();
    let mut total = 0u64;
    let mut sounds = 0u64;
    for index in 0..zip.len() {
        let entry = zip.by_index(index)?;
        if !names.insert(entry.name().to_string()) || entry.enclosed_name().is_none() {
            return Err(IoError::Manifest(
                "Duplicate or unsafe archive entry.".into(),
            ));
        }
        if audio && entry.name().starts_with("audio/") {
            sounds = sounds.saturating_add(entry.size());
            if sounds > audio::MAX_PACKAGE_AUDIO {
                return Err(IoError::Manifest(
                    "Sounds exceed the 2 GiB a project can hold.".into(),
                ));
            }
            continue;
        }
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| IoError::Manifest("Archive size overflow.".into()))?;
        if total > MAX_BYTES {
            return Err(IoError::Manifest(
                "Archive exceeds the 2 GiB decoded budget.".into(),
            ));
        }
    }
    Ok(total)
}

fn read_manifest<R: Read + Seek>(zip: &mut ZipArchive<R>) -> Result<Manifest> {
    check_entries(zip, true)?;
    if ora::read_entry(zip, "mimetype", 128)? != MIME {
        return Err(IoError::Manifest("Not an Emulsion project.".into()));
    }
    let bytes = ora::read_entry(zip, "project.json", MAX_MANIFEST)?;
    // Inspect the version before deserializing future schema fields/enums.
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| IoError::Manifest(e.to_string()))?;
    let version = value
        .get("version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| IoError::Manifest("Missing project version.".into()))?;
    if version > u64::from(VERSION) {
        return Err(IoError::TooNew(version.min(u64::from(u32::MAX)) as u32));
    }
    if version != u64::from(VERSION) {
        return Err(IoError::Manifest("Unsupported project version.".into()));
    }
    let manifest: Manifest =
        serde_json::from_value(value).map_err(|e| IoError::Manifest(e.to_string()))?;
    if manifest.pages.is_empty() || manifest.pages.len() > MAX_PAGES {
        return Err(IoError::Manifest("Invalid page count.".into()));
    }
    let mut ids = HashSet::new();
    let mut pixels = 0u64;
    for page in &manifest.pages {
        page.meta.validate().map_err(IoError::Manifest)?;
        crate::import::check_size(page.width, page.height)?;
        if !ids.insert(page.meta.id) {
            return Err(IoError::Manifest("Duplicate page ID.".into()));
        }
        pixels += u64::from(page.width) * u64::from(page.height);
    }
    if pixels > MAX_PROJECT_PIXELS
        || !ids.contains(&manifest.active)
        || manifest.next_page_id <= *ids.iter().max().unwrap()
        || manifest.next_page_id == u64::MAX
    {
        return Err(IoError::Manifest(
            "Invalid page references or total area.".into(),
        ));
    }
    Ok(manifest)
}

pub fn read(path: &Path) -> Result<Project> {
    read_from(std::io::BufReader::new(std::fs::File::open(path)?))
}

pub fn read_from<R: Read + Seek>(reader: R) -> Result<Project> {
    let mut zip = ZipArchive::new(reader)?;
    let manifest = read_manifest(&mut zip)?;
    let mut total = 0u64;
    let storyboard = match manifest.kind {
        ProjectKind::Storyboard => {
            let bytes = ora::read_entry(&mut zip, STORYBOARD_ENTRY, MAX_STORYBOARD)?;
            let mut board: emulsion_core::storyboard::Storyboard =
                serde_json::from_slice(&bytes).map_err(|e| IoError::Manifest(e.to_string()))?;
            if board.library.items.len() > emulsion_core::storyboard_library::MAX_ITEMS {
                return Err(IoError::Manifest("Too many library items.".into()));
            }
            for item in &mut board.library.items {
                let bytes = ora::read_entry(&mut zip, &library_entry(item.id), MAX_BYTES)?;
                total += check_archive(&mut ZipArchive::new(Cursor::new(&bytes))?)?;
                if total > MAX_BYTES {
                    return Err(IoError::Manifest(
                        "Project exceeds the decoded archive budget.".into(),
                    ));
                }
                item.doc = std::sync::Arc::new(ora::read_from(Cursor::new(bytes))?.doc);
            }
            Some(board)
        }
        _ => None,
    };
    let mut pages = Vec::new();
    for record in manifest.pages {
        let bytes = ora::read_entry(
            &mut zip,
            &format!("pages/{}.ora", record.meta.id),
            MAX_BYTES,
        )?;
        total += check_archive(&mut ZipArchive::new(Cursor::new(&bytes))?)?;
        if total > MAX_BYTES {
            return Err(IoError::Manifest(
                "Project exceeds the decoded archive budget.".into(),
            ));
        }
        let opened = ora::read_from(Cursor::new(bytes))?;
        if let Some(error) = opened.history_error {
            return Err(IoError::Manifest(format!(
                "Page {} history: {error}",
                record.meta.name
            )));
        }
        if (opened.doc.width, opened.doc.height) != (record.width, record.height) {
            return Err(IoError::Manifest(
                "Page size differs from the project manifest.".into(),
            ));
        }
        let graph = opened
            .graph
            .unwrap_or_else(|| emulsion_core::graph::Graph::new(opened.doc.clone(), "Opened"));
        pages.push(ProjectPage {
            meta: record.meta,
            doc: opened.doc,
            graph,
        });
    }
    let mut project = Project {
        kind: manifest.kind,
        storyboard,
        active: manifest.active,
        next_page_id: manifest.next_page_id,
        pages,
    };
    project.validate().map_err(IoError::Manifest)?;
    // Sounds go to the media cache last, once everything else is valid.
    if let Some(board) = &mut project.storyboard {
        read_audio(&mut zip, &mut board.timeline)?;
    }
    Ok(project)
}

/// Home thumbnails decode only the first page, without constructing a project.
pub(crate) fn cover(path: &Path) -> Result<emulsion_core::Document> {
    let mut zip = ZipArchive::new(std::io::BufReader::new(std::fs::File::open(path)?))?;
    let manifest = read_manifest(&mut zip)?;
    let first = &manifest.pages[0];
    let bytes = ora::read_entry(&mut zip, &format!("pages/{}.ora", first.meta.id), MAX_BYTES)?;
    check_archive(&mut ZipArchive::new(Cursor::new(&bytes))?)?;
    let doc = ora::read_from(Cursor::new(bytes))?.doc;
    Ok(emulsion_core::diagram::workspace::thumbnail_document(&doc).unwrap_or(doc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::{Command, Node, NodeKind, command::Slot, project::ProjectEditor};
    fn path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "emulsion-project-{label}-{}.emu",
            std::process::id()
        ))
    }
    #[test]
    fn package_round_trips_all_pages_editable_sources_and_versions() {
        let mut session =
            ProjectEditor::new_project(ProjectKind::Design, emulsion_core::Document::new(64, 48))
                .unwrap();
        session
            .execute(Command::AddNode {
                node: Box::new(Node::new(
                    0,
                    "Background",
                    NodeKind::Fill {
                        rgba: [20, 60, 120, 255],
                    },
                )),
                slot: Slot::TOP,
            })
            .unwrap();
        session.create_version("First design");
        let second = session.duplicate_page(1).unwrap();
        session
            .rename_page(second, "Back cover".into(), 3.)
            .unwrap();
        let project = session.snapshot().unwrap();
        let file = path("roundtrip");
        write(&project, &file).unwrap();
        let reopened = read(&file).unwrap();
        assert_eq!(reopened.active, second);
        assert_eq!(reopened.pages.len(), 2);
        for (before, after) in project.pages.iter().zip(&reopened.pages) {
            assert_eq!(before.meta, after.meta);
            assert_eq!(before.doc, after.doc);
            assert_eq!(before.graph.len(), after.graph.len());
        }
        assert_eq!(cover(&file).unwrap(), project.pages[0].doc);
        std::fs::remove_file(file).unwrap();
    }

    #[test]
    fn supplied_starters_and_responsive_layout_round_trip_as_editable_pages() {
        use emulsion_core::{
            design::Template,
            design_layout::{self, Frame},
        };
        let mut session = ProjectEditor::new_project(
            ProjectKind::Design,
            Template::ProductLaunch.create(216, 216).unwrap(),
        )
        .unwrap();
        let ids = session
            .doc
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Text { .. }))
            .take(2)
            .map(|n| n.id)
            .collect();
        let group = session
            .execute(Command::Group {
                ids,
                name: "Responsive heading".into(),
            })
            .unwrap()
            .unwrap();
        session.begin("Layout");
        design_layout::enable(
            &mut session,
            group,
            Frame {
                padding: [8.; 4],
                gap: 8.,
                ..Default::default()
            },
            (180., 180.),
        )
        .unwrap();
        session.end();
        let mut projects = Vec::new();
        for template in Template::catalog().skip(1) {
            let (w, h) = template.native_size();
            let k = 240. / w.max(h) as f64;
            let doc = template
                .create((w as f64 * k).round() as u32, (h as f64 * k).round() as u32)
                .unwrap();
            if session.page_list().len() == MAX_PAGES {
                projects.push(session.snapshot().unwrap());
                session = ProjectEditor::new_project(ProjectKind::Design, doc).unwrap();
            } else {
                session.add_page(doc, template.label().into(), 0.).unwrap();
            }
        }
        projects.push(session.snapshot().unwrap());
        assert_eq!(
            projects.iter().map(|p| p.pages.len()).sum::<usize>(),
            Template::catalog().count()
        );
        for (i, original) in projects.iter().enumerate() {
            let file = path("supplied-starters");
            write(original, &file).unwrap();
            let reopened = read(&file).unwrap();
            assert_eq!(reopened.pages.len(), original.pages.len());
            for (before, after) in original.pages.iter().zip(&reopened.pages) {
                assert_eq!(before.doc, after.doc);
                assert!(
                    after
                        .doc
                        .nodes
                        .iter()
                        .any(|n| matches!(n.kind, NodeKind::Text { .. }))
                );
            }
            if i == 0 {
                assert_eq!(reopened.pages[0].doc.design.frames.len(), 1);
            }
            std::fs::remove_file(file).unwrap();
        }
    }
    #[test]
    fn rejects_future_versions_and_missing_pages_without_partial_documents() {
        for (label, manifest) in [
            ("future", serde_json::json!({"version":999})),
            (
                "missing",
                serde_json::json!({"version":1,"kind":"design","active":1,"next_page_id":2,"pages":[{"meta":{"id":1,"name":"Page 1","bleed_mm":0},"width":64,"height":48}]}),
            ),
        ] {
            let file = path(label);
            let mut zip = ZipWriter::new(std::fs::File::create(&file).unwrap());
            zip.start_file("mimetype", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(MIME).unwrap();
            zip.start_file("project.json", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(&serde_json::to_vec(&manifest).unwrap())
                .unwrap();
            zip.finish().unwrap();
            let error = read(&file).err().expect("must reject");
            if label == "future" {
                assert!(matches!(error, IoError::TooNew(999)));
            }
            std::fs::remove_file(file).unwrap();
        }
    }

    #[test]
    fn pages_roundtrip_text_vectors_masks_images_and_versions() {
        use std::sync::Arc;
        let mut doc = emulsion_core::design::Template::Editorial
            .create(160, 120)
            .unwrap();
        let mut image = Node::raster(
            0,
            "Photo",
            Arc::new(emulsion_raster::Raster::solid(24, 20, [0.3, 0.2, 0.1, 1.])),
            emulsion_raster::Placement::at(30., 40.),
        );
        image.mask = Some(Arc::new(emulsion_raster::Mask::from_fn(
            24,
            20,
            0,
            |x, y| if x > 3 && y > 2 { 255 } else { 0 },
        )));
        Command::AddNode {
            node: Box::new(image),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
        let mut session = ProjectEditor::new_project(ProjectKind::Design, doc).unwrap();
        let text = session
            .doc
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::Text { .. }))
            .unwrap()
            .id;
        let mut design = session.doc.design.clone();
        design
            .motion
            .insert(text, emulsion_core::design_metadata::Motion::default());
        design.constraints.insert(
            text,
            emulsion_core::design_metadata::Constraint {
                horizontal: emulsion_core::design_metadata::Anchor::Stretch,
                reflow_text: true,
                ..Default::default()
            },
        );
        session
            .execute(Command::SetDesign {
                design: Box::new(design),
            })
            .unwrap();
        session
            .execute(Command::SetOpacity {
                id: session.doc.nodes.last().unwrap().id,
                opacity: 0.7,
            })
            .unwrap();
        session.create_version("Masked image");
        session.duplicate_page(1).unwrap();
        let file = path("editable-roundtrip");
        let original = session.snapshot().unwrap();
        write(&original, &file).unwrap();
        let actual = read(&file).unwrap();
        for (before, after) in original.pages.iter().zip(&actual.pages) {
            assert_eq!(before.doc.design, after.doc.design);
            for (a, b) in before.graph.commits().zip(after.graph.commits()) {
                assert_eq!(a.doc.design, b.doc.design);
            }
            assert_eq!(before.graph.len(), after.graph.len());
            assert_eq!(before.doc.nodes.len(), after.doc.nodes.len());
            for (before, after) in before.doc.nodes.iter().zip(&after.doc.nodes) {
                match (&before.kind, &after.kind) {
                    (NodeKind::Text { spec: a, .. }, NodeKind::Text { spec: b, .. }) => {
                        assert_eq!(a, b)
                    }
                    (
                        NodeKind::Path {
                            path: a, style: sa, ..
                        },
                        NodeKind::Path {
                            path: b, style: sb, ..
                        },
                    ) => {
                        assert_eq!(a, b);
                        assert_eq!(sa, sb);
                    }
                    (
                        NodeKind::Raster {
                            raster: a,
                            placement: pa,
                        },
                        NodeKind::Raster {
                            raster: b,
                            placement: pb,
                        },
                    ) => {
                        assert_eq!(pa, pb);
                        assert_eq!(a.get(10, 10), b.get(10, 10));
                        assert_eq!(
                            before.mask.as_ref().unwrap().get(4, 4),
                            after.mask.as_ref().unwrap().get(4, 4)
                        );
                    }
                    (a, b) => assert_eq!(a, b),
                }
            }
            let a =
                emulsion_raster::composite::flatten(&before.doc.composite_tree(), 0).to_srgba16();
            let b =
                emulsion_raster::composite::flatten(&after.doc.composite_tree(), 0).to_srgba16();
            assert_eq!(a, b);
        }
        std::fs::remove_file(file).unwrap();
    }

    #[test]
    fn source_protection_in_inactive_pages_leaves_existing_file_untouched() {
        let file = path("protected");
        std::fs::write(&file, b"original camera bytes").unwrap();
        let mut session =
            ProjectEditor::new_project(ProjectKind::Design, emulsion_core::Document::new(16, 16))
                .unwrap();
        let mut doc = emulsion_core::Document::new(16, 16);
        doc.raw_originals.push(file.clone());
        session
            .add_page(doc, "Protected source".into(), 0.)
            .unwrap();
        session.set_active_page(1).unwrap();
        assert!(write(&session.snapshot().unwrap(), &file).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"original camera bytes");
        std::fs::remove_file(file).unwrap();
    }

    #[test]
    fn storyboard_projects_round_trip_outline_captions_timing_and_locks() {
        let file = path("storyboard");
        let mut session = emulsion_core::creation::CanvasSpec {
            name: "Board".into(),
            kind: emulsion_core::creation::CanvasKind::Storyboard,
            width: 64.,
            height: 36.,
            pages: 2,
            ..Default::default()
        }
        .create_project()
        .unwrap();
        session
            .edit_storyboard(|board| {
                let dialogue = board.caption("Dialogue").unwrap();
                let panel = board.panels.get_mut(&2).unwrap();
                let mut line = emulsion_core::storyboard::Caption::from("Where are we?");
                line.apply_style(0..5, |style| style.bold = true);
                panel.captions.insert(dialogue, line);
                panel.frames = 30;
                panel.thumbnails = Some(emulsion_core::storyboard::ThumbnailGrid {
                    gap: 2,
                    margin: 2,
                    ..emulsion_core::storyboard::ThumbnailGrid::new(2, 1)
                });
                board.settings.frame_rate = emulsion_core::storyboard::FrameRate::ntsc(24);
                board.panels.get_mut(&1).unwrap().locked = true;
                board.naming.scene_prefix = "SC".into();
                board.smart_add_layers = vec!["Set".into()];
                board.add_caption_field("Sound", false, true).map(|_| ())
            })
            .unwrap();
        let project = session.snapshot().unwrap();
        write(&project, &file).unwrap();
        let back = read(&file).unwrap();
        assert_eq!(back.kind, ProjectKind::Storyboard);
        assert_eq!(back.storyboard, project.storyboard);
        // Opening restores the lock on the panel's editor.
        assert!(
            ProjectEditor::open(back, Some(file.clone()))
                .unwrap()
                .page(1)
                .unwrap()
                .is_read_only()
        );
        let mut zip = ZipArchive::new(std::fs::File::open(&file).unwrap()).unwrap();
        assert!(zip.by_name(STORYBOARD_ENTRY).is_ok());
        std::fs::remove_file(file).unwrap();
    }

    #[test]
    fn storyboard_library_drawings_travel_with_the_package() {
        use emulsion_core::storyboard_library::ItemKind;
        let file = path("storyboard-library");
        let mut session = emulsion_core::creation::CanvasSpec {
            name: "Board".into(),
            kind: emulsion_core::creation::CanvasKind::Storyboard,
            width: 64.,
            height: 36.,
            pages: 2,
            ..Default::default()
        }
        .create_project()
        .unwrap();
        session
            .execute(Command::AddNode {
                node: Box::new(Node::raster(
                    0,
                    "Hero",
                    std::sync::Arc::new(emulsion_raster::Raster::solid(8, 6, [0.8, 0.1, 0.1, 1.])),
                    emulsion_raster::Placement::at(20., 10.),
                )),
                slot: Slot::TOP,
            })
            .unwrap();
        let hero = session.doc.nodes.last().unwrap().id;
        let layers = session
            .add_library_layers(
                session.active_page(),
                &[hero],
                "Hero",
                &["character".into()],
            )
            .unwrap();
        let panel = session.add_library_panel(2, "Empty set", &[]).unwrap();
        let project = session.snapshot().unwrap();
        write(&project, &file).unwrap();
        let back = read(&file).unwrap();
        let library = &back.storyboard.as_ref().unwrap().library;
        let original = &project.storyboard.as_ref().unwrap().library;
        assert_eq!(library.next_id, original.next_id);
        for (a, b) in library.items.iter().zip(&original.items) {
            assert_eq!(
                (a.id, &a.name, &a.tags, a.kind),
                (b.id, &b.name, &b.tags, b.kind)
            );
            assert_eq!(a.doc.nodes.len(), b.doc.nodes.len());
            assert_eq!((a.doc.width, a.doc.height), (b.doc.width, b.doc.height));
        }
        // Pixels survive at 16-bit precision; layers stay where they were.
        let NodeKind::Raster { placement, .. } = &library.item(layers).unwrap().doc.nodes[0].kind
        else {
            panic!()
        };
        assert_eq!((placement.x, placement.y), (20., 10.));
        assert_eq!(library.item(layers).unwrap().kind, ItemKind::Layers);
        assert_eq!(library.item(layers).unwrap().tags, ["character"]);
        assert_eq!(library.item(panel).unwrap().kind, ItemKind::Panel);
        // A package whose drawing is missing does not open.
        let mut zip = ZipArchive::new(std::fs::File::open(&file).unwrap()).unwrap();
        let broken = path("storyboard-library-broken");
        let mut out = ZipWriter::new(std::fs::File::create(&broken).unwrap());
        for i in 0..zip.len() {
            let entry = zip.by_index(i).unwrap();
            if entry.name() != library_entry(panel) {
                out.raw_copy_file(entry).unwrap();
            }
        }
        out.finish().unwrap();
        assert!(read(&broken).is_err());
        std::fs::remove_file(file).unwrap();
        std::fs::remove_file(broken).unwrap();
    }

    #[test]
    fn storyboard_sounds_stream_into_the_package_and_back_to_the_cache() {
        use emulsion_core::timeline::{AudioAsset, AudioTrack};
        let file = path("storyboard-audio");
        let mut session = emulsion_core::creation::CanvasSpec {
            name: "Board".into(),
            kind: emulsion_core::creation::CanvasKind::Storyboard,
            width: 64.,
            height: 36.,
            pages: 1,
            ..Default::default()
        }
        .create_project()
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let bytes: Vec<u8> = (0..300_000u32).map(|i| (i * 7 % 251) as u8).collect();
        let original = dir.path().join("line.wav");
        std::fs::write(&original, &bytes).unwrap();
        let asset = AudioAsset {
            name: "Line".into(),
            format: "wav".into(),
            duration_ms: 1000,
            sample_rate: 48_000,
            channels: 2,
            folder: "Dialogue".into(),
            source: Some(original.clone()),
        };
        let mut id = 0;
        session
            .edit_storyboard(|b| {
                id = b.timeline.add_asset(asset.clone())?;
                b.timeline.tracks.push(AudioTrack::new("Dialogue"));
                Ok(())
            })
            .unwrap();
        let project = session.snapshot().unwrap();
        write(&project, &file).unwrap();
        let back = read(&file).unwrap();
        let reopened = &back.storyboard.as_ref().unwrap().timeline;
        let sound = &reopened.assets[&id];
        let source = sound.source.clone().unwrap();
        assert_ne!(source, original);
        assert!(source.starts_with(audio::cache_root().unwrap()));
        assert_eq!(std::fs::read(&source).unwrap(), bytes);
        assert_eq!(
            AudioAsset {
                source: None,
                ..sound.clone()
            },
            AudioAsset {
                source: None,
                ..asset
            }
        );
        assert_eq!(
            reopened.tracks,
            project.storyboard.as_ref().unwrap().timeline.tracks
        );
        // Saving the reopened project streams from the cache again.
        let again = path("storyboard-audio-again");
        write(&back, &again).unwrap();
        let mut zip = ZipArchive::new(std::fs::File::open(&again).unwrap()).unwrap();
        assert_eq!(
            zip.by_name(&audio::entry_name(id, "wav")).unwrap().size(),
            bytes.len() as u64
        );
        // A package missing a sound does not open.
        let broken = path("storyboard-audio-broken");
        let mut out = ZipWriter::new(std::fs::File::create(&broken).unwrap());
        for i in 0..zip.len() {
            let entry = zip.by_index(i).unwrap();
            if !entry.name().starts_with("audio/") {
                out.raw_copy_file(entry).unwrap();
            }
        }
        out.finish().unwrap();
        assert!(read(&broken).err().unwrap().to_string().contains("missing"));
        // A sound without its file cannot be saved, and nothing is written.
        let mut lost = project.clone();
        lost.storyboard
            .as_mut()
            .unwrap()
            .timeline
            .assets
            .get_mut(&id)
            .unwrap()
            .source = None;
        let nowhere = path("storyboard-audio-lost");
        assert!(write(&lost, &nowhere).is_err());
        assert!(!nowhere.exists());
        for f in [file, again, broken] {
            std::fs::remove_file(f).unwrap();
        }
    }

    #[test]
    fn storyboard_data_is_required_for_storyboards_and_absent_otherwise() {
        let file = path("design-no-board");
        let design =
            ProjectEditor::new_project(ProjectKind::Design, emulsion_core::Document::new(8, 8))
                .unwrap()
                .snapshot()
                .unwrap();
        write(&design, &file).unwrap();
        let mut zip = ZipArchive::new(std::fs::File::open(&file).unwrap()).unwrap();
        assert!(zip.by_name(STORYBOARD_ENTRY).is_err());
        assert!(read(&file).unwrap().storyboard.is_none());
        let mut board =
            ProjectEditor::new_project(ProjectKind::Storyboard, emulsion_core::Document::new(8, 8))
                .unwrap()
                .snapshot()
                .unwrap();
        board.storyboard = None;
        assert!(write(&board, &file).is_err());
        std::fs::remove_file(file).unwrap();
    }
}
