//! Edit a storyboard panel in another application (SB2): the panel is
//! written as a layered PSD or ORA into a folder the app manages, the
//! chosen editor opens it, and every save there comes back as one Undo
//! step on the panel. The UI polls the file with [`SaveWatch`] (size and
//! modification time, settled for a moment so a save still being written
//! is never read) and brings each settled save back with
//! [`ExternalEdit::bring_back`].
//!
//! Layers keep their identity by name: a layer whose name matches one on
//! the panel takes that layer's ID, so its keyframes and layer comps keep
//! working. The drawing is fitted to the panel's size the way imports
//! are, and the panel's timing, captions and keyframes are never touched.
use emulsion_core::project::{PageId, ProjectEditor};
use emulsion_core::storyboard_naming::fit_to_frame;
use emulsion_core::{Document, NodeId};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// The file format the panel goes out as.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditFormat {
    /// Layered PSD document, read by most image editors and painting
    /// apps.
    #[default]
    Psd,
    /// OpenRaster, the open layered exchange format.
    Ora,
}

impl EditFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Psd => "psd",
            Self::Ora => "ora",
        }
    }

    pub fn write(self, doc: &Document, path: &Path) -> Result<(), String> {
        self.write_with_report(doc, path).map(|_| ())
    }

    /// Return the completed PSD write's report; ORA has no PSD losses.
    pub fn write_with_report(
        self,
        doc: &Document,
        path: &Path,
    ) -> Result<Option<crate::psd::WriteReport>, String> {
        match self {
            Self::Psd => crate::psd::write_with_report(doc, path).map(Some),
            Self::Ora => crate::ora::write(doc, path).map(|()| None),
        }
        .map_err(|e| e.to_string())
    }

    pub fn read(self, path: &Path) -> Result<Document, String> {
        match self {
            Self::Psd => crate::psd::read(path),
            Self::Ora => crate::ora::read(path),
        }
        .map_err(|e| e.to_string())
    }
}

/// A file's size and modification time: what changes when it is saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStamp {
    pub modified: SystemTime,
    pub len: u64,
}

impl FileStamp {
    pub fn of(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Self {
            modified: meta.modified().ok()?,
            len: meta.len(),
        })
    }
}

/// How long a save must stay unchanged before it is read.
pub const SETTLE: Duration = Duration::from_millis(600);

/// Notices saves of a watched file. A save counts once its stamp has stayed
/// the same for [`SETTLE`], so a file still being written is never read.
#[derive(Clone, Debug)]
pub struct SaveWatch {
    /// The stamp last brought back (or written).
    seen: Option<FileStamp>,
    /// A new stamp, and when it was first seen.
    pending: Option<(FileStamp, Instant)>,
}

impl SaveWatch {
    pub fn new(written: Option<FileStamp>) -> Self {
        Self {
            seen: written,
            pending: None,
        }
    }

    /// Look at the file's stamp `now` at `at`. True when a new save has
    /// settled and should be read; it then counts as seen.
    pub fn poll(&mut self, now: Option<FileStamp>, at: Instant) -> bool {
        let Some(stamp) = now else {
            // Some apps save by deleting and renaming: wait for the file.
            self.pending = None;
            return false;
        };
        if Some(stamp) == self.seen {
            self.pending = None;
            return false;
        }
        match self.pending {
            Some((pending, since)) if pending == stamp => {
                if at.duration_since(since) >= SETTLE {
                    self.seen = Some(stamp);
                    self.pending = None;
                    true
                } else {
                    false
                }
            }
            _ => {
                self.pending = Some((stamp, at));
                false
            }
        }
    }
}

/// What to do with a save when the panel also changed in Emulsion since
/// it went out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// The external drawing replaces the panel's.
    TakeExternal,
    /// The external drawing's layers go on top of the panel's.
    KeepBoth,
    /// The save is ignored.
    KeepMine,
}

/// One panel open in an external editor.
#[derive(Debug)]
pub struct ExternalEdit {
    pub panel: PageId,
    /// The editor's name, for the Undo label and the status chip.
    pub app: String,
    pub format: EditFormat,
    /// The panel's own folder; removed when the edit ends.
    dir: PathBuf,
    pub path: PathBuf,
    /// The panel's drawing as it went out or last came back, to tell
    /// whether it changed in Emulsion meanwhile.
    synced: Document,
    pub watch: SaveWatch,
    /// Consumed by the initial completion, never replayed on watched saves.
    initial_write_report: Option<crate::psd::WriteReport>,
}

/// The folder external edits of the project `project_id` are written to.
pub fn edit_root(project_id: &str) -> PathBuf {
    let id: String = project_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(64)
        .collect();
    std::env::temp_dir()
        .join("emulsion-external-edit")
        .join(if id.is_empty() { "project".into() } else { id })
}

fn file_stem(name: &str) -> String {
    let stem: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_.".contains(c) {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    match stem.trim().trim_matches('.') {
        "" => "Panel".into(),
        stem => stem.into(),
    }
}

impl ExternalEdit {
    /// Write `doc` (panel `panel`, named `name`) under `root` for `app` to
    /// open.
    pub fn start(
        panel: PageId,
        name: &str,
        doc: Document,
        format: EditFormat,
        root: &Path,
        app: &str,
    ) -> Result<Self, String> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = root.join(format!("panel-{panel}-{}-{seq}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!("{}.{}", file_stem(name), format.extension()));
        let initial_write_report = match format.write_with_report(&doc, &path) {
            Ok(report) => report,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&dir);
                return Err(e);
            }
        };
        Ok(Self {
            panel,
            app: app.into(),
            format,
            watch: SaveWatch::new(FileStamp::of(&path)),
            dir,
            path,
            synced: doc,
            initial_write_report,
        })
    }

    /// The report for the initial file, once. Later watcher polls describe
    /// another application's saves and must not repeat this export's notices.
    pub fn take_initial_write_report(&mut self) -> Option<crate::psd::WriteReport> {
        self.initial_write_report.take()
    }

    /// The Undo label of a save brought back.
    pub fn label(&self) -> String {
        format!("Edit in {}", self.app)
    }

    /// Whether the panel changed in Emulsion since it went out or last
    /// came back.
    pub fn changed_here(&self, project: &ProjectEditor) -> bool {
        project
            .page(self.panel)
            .is_some_and(|editor| editor.doc != self.synced)
    }

    /// Read the saved file.
    pub fn read(&self) -> Result<Document, String> {
        self.format.read(&self.path)
    }

    /// Bring the saved drawing `external` back onto the panel as one Undo
    /// step. Returns whether the panel changed.
    pub fn bring_back(
        &mut self,
        project: &mut ProjectEditor,
        external: &Document,
        resolution: Resolution,
    ) -> Result<bool, String> {
        let current = project
            .page(self.panel)
            .ok_or("The panel being edited was deleted.")?
            .doc
            .clone();
        let next = match resolution {
            Resolution::KeepMine => {
                self.synced = current;
                return Ok(false);
            }
            Resolution::TakeExternal => adopt_layers(&current, external),
            Resolution::KeepBoth => stack_layers(&current, external, &self.app),
        }
        .map_err(|error| error.to_string())?;
        if next == current {
            self.synced = current;
            return Ok(false);
        }
        project.replace_panel_document(self.panel, next, &self.label())?;
        self.synced = project.page(self.panel).unwrap().doc.clone();
        Ok(true)
    }
}

impl Drop for ExternalEdit {
    /// The panel's folder goes when the edit stops or the project closes.
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// `nodes` with every ID (and the parents and clipping bases pointing at
/// them) changed by `map`.
fn remap(nodes: &mut [emulsion_core::Node], map: &HashMap<NodeId, NodeId>) {
    let fix = |id: &mut NodeId| {
        if let Some(n) = map.get(id) {
            *id = *n;
        }
    };
    for node in nodes {
        fix(&mut node.id);
        if let Some(p) = &mut node.parent {
            fix(p);
        }
        if let Some(c) = &mut node.clip_to {
            fix(c);
        }
    }
}

fn next_free(doc: &Document) -> NodeId {
    doc.nodes
        .iter()
        .map(|n| n.id + 1)
        .max()
        .unwrap_or(1)
        .max(doc.next_id)
}

/// `current` with its layers replaced by `external`'s, fitted to its size.
/// An external layer named like a panel layer takes that layer's ID (in
/// order, for repeated names), so its keyframes follow; the others get new
/// IDs. Everything that is not a layer (guides, selection, resolution)
/// stays the panel's. Fitting and final validation complete before a candidate
/// is returned; a failure leaves both input documents unchanged.
pub fn adopt_layers(current: &Document, external: &Document) -> crate::Result<Document> {
    let mut ext = fit_to_frame(external, current.width, current.height)?;
    let mut by_name: HashMap<&str, Vec<NodeId>> = HashMap::new();
    for node in &current.nodes {
        by_name.entry(node.name.as_str()).or_default().push(node.id);
    }
    let mut taken = HashSet::new();
    let mut next = next_free(current).max(next_free(&ext));
    let mut map = HashMap::new();
    for node in &ext.nodes {
        let reused = by_name
            .get(node.name.as_str())
            .and_then(|ids| ids.iter().find(|id| !taken.contains(*id)).copied());
        let id = reused.unwrap_or_else(|| {
            next += 1;
            next - 1
        });
        taken.insert(id);
        map.insert(node.id, id);
    }
    remap(&mut ext.nodes, &map);
    let mut doc = current.clone();
    // Transfer only the external document's explicit role, never a reused name/ID.
    doc.psd_background = ext.psd_background.and_then(|id| map.get(&id).copied());
    // Keep established Linear/sRGB external-edit behavior. Entering or leaving
    // the versioned PSD-compatible profile adopts the external scene's profile too.
    if current.blend_space == emulsion_raster::blend::BlendSpace::PhotoshopSrgbV1
        || ext.blend_space == emulsion_raster::blend::BlendSpace::PhotoshopSrgbV1
    {
        doc.blend_space = ext.blend_space;
    }
    doc.nodes = ext.nodes;
    doc.prune_psd_background();
    doc.next_id = next;
    doc.validate()?;
    Ok(doc)
}

/// `current` with `external`'s layers (fitted, with new IDs) added on top,
/// each top-level one named after `app`. An unsupported fit or invalid combined
/// document returns an error without publishing or changing either input.
pub fn stack_layers(current: &Document, external: &Document, app: &str) -> crate::Result<Document> {
    let mut ext = fit_to_frame(external, current.width, current.height)?;
    let mut next = next_free(current);
    let map: HashMap<NodeId, NodeId> = ext
        .nodes
        .iter()
        .map(|n| {
            next += 1;
            (n.id, next - 1)
        })
        .collect();
    remap(&mut ext.nodes, &map);
    for node in ext.nodes.iter_mut().filter(|n| n.parent.is_none()) {
        node.name = format!("{} ({app})", node.name);
    }
    let mut doc = current.clone();
    doc.nodes.extend(ext.nodes);
    doc.next_id = next;
    doc.validate()?;
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::command::Slot;
    use emulsion_core::project::ProjectKind;
    use emulsion_core::storyboard::{LayerMotion, LayerProperty, MotionKey, PropertyTrack};
    use emulsion_core::{Command, Node, NodeKind};
    use emulsion_raster::{Placement, Raster};
    use std::sync::Arc;

    fn layer(name: &str, rgba: [f32; 4], w: u32, h: u32) -> Node {
        Node::new(
            0,
            name,
            NodeKind::Raster {
                raster: Arc::new(Raster::solid(w, h, rgba)),
                placement: Placement::default(),
            },
        )
    }

    /// A one-panel storyboard with layers "Background" and "Ink", the
    /// Ink layer animated.
    fn board() -> (ProjectEditor, NodeId) {
        let mut p =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(32, 18)).unwrap();
        p.execute(Command::AddNode {
            node: Box::new(layer("Background", [1.; 4], 32, 18)),
            slot: Slot::TOP,
        })
        .unwrap();
        let ink = p
            .execute(Command::AddNode {
                node: Box::new(layer("Ink", [0., 0., 0., 1.], 32, 18)),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        p.edit_storyboard(|b| {
            let motion = LayerMotion {
                pivot: None,
                tracks: vec![PropertyTrack {
                    property: LayerProperty::Opacity,
                    keys: vec![MotionKey {
                        frame: 0,
                        value: 0.5,
                        easing: Default::default(),
                        curve: None,
                    }],
                }],
            };
            b.panels.get_mut(&1).unwrap().motion.insert(ink, motion);
            b.panels.get_mut(&1).unwrap().frames = 30;
            Ok(())
        })
        .unwrap();
        (p, ink)
    }

    fn names(doc: &Document) -> Vec<String> {
        doc.nodes.iter().map(|n| n.name.clone()).collect()
    }

    fn start(p: &ProjectEditor, format: EditFormat, root: &Path) -> ExternalEdit {
        let doc = p.page(1).unwrap().doc.clone();
        ExternalEdit::start(1, "Panel 1/A", doc, format, root, "PaintApp").unwrap()
    }

    /// What the external app does: read the file, add a layer, double the
    /// canvas (the panel must keep its size) and save. The PSD fixture models
    /// an 8-bit editor's nearest-neighbor resize of the stored pixel grid.
    fn edit_outside(edit: &ExternalEdit) {
        let mut doc = edit.read().unwrap();
        if edit.format == EditFormat::Psd {
            // The opaque starting image is ambiguous and imports in legacy
            // sRGB. Simulate the external PSD editor's encoded-sRGB Normal
            // compositing before adding translucent paint; retaining native
            // linear source-over would correctly force appearance-only export.
            doc.blend_space = emulsion_raster::blend::BlendSpace::PhotoshopSrgbV1;
        }
        let shading = if edit.format == EditFormat::Psd {
            // Author actual PSD byte samples: native half alpha and
            // higher-precision color can honestly trigger export fallback.
            Node::raster(
                0,
                "Shading",
                Arc::new(Raster::from_srgba8(
                    32,
                    18,
                    &[170, 170, 170, 128].repeat(32 * 18),
                )),
                Placement::default(),
            )
        } else {
            layer("Shading", [0.2, 0.2, 0.2, 0.5], 32, 18)
        };
        let mut editor = emulsion_core::Editor::new(doc.clone(), None);
        editor
            .execute(Command::AddNode {
                node: Box::new(shading),
                slot: Slot::TOP,
            })
            .unwrap();
        doc = editor.doc;
        if edit.format == EditFormat::Psd {
            // Resize the external editor's stored pixels, rather than leaving
            // native scaled placements whose bilinear edges must be quantized
            // again when PSD rasterizes them. All named layers remain separate.
            for node in &mut doc.nodes {
                let NodeKind::Raster { raster, placement } = &mut node.kind else {
                    panic!("the external-edit fixture must contain raster layers");
                };
                assert_eq!(*placement, Placement::default());
                *raster = Arc::new(Raster::from_fn(
                    raster.width() * 2,
                    raster.height() * 2,
                    [0; 4],
                    |x, y| raster.get(x / 2, y / 2),
                ));
            }
            doc.width *= 2;
            doc.height *= 2;
            assert_eq!((doc.width, doc.height), (64, 36));
            let before = doc.clone();
            let expected = [85, 85, 85, 255].repeat(64 * 36);
            assert_eq!(
                emulsion_raster::composite::flatten(&doc.composite_tree(), 0).to_srgba8(),
                expected,
                "the external fixture uses encoded-sRGB Normal compositing"
            );
            assert_eq!(
                crate::psd::write_with_report(&doc, &edit.path).unwrap(),
                crate::psd::WriteReport {
                    appearance_fallback: None,
                    baked_raster_masks: false,
                    rounded_mask_densities: 0,
                },
                "the external-save fixture must retain its named layers"
            );
            assert_eq!(doc, before, "export must not change the fixture");
            let back = edit.read().unwrap();
            assert_eq!((back.width, back.height), (64, 36));
            assert_eq!(names(&back), names(&doc));
            assert_eq!(
                emulsion_raster::composite::flatten(&back.composite_tree(), 0).to_srgba8(),
                expected,
                "layered PSD must reproduce the external editor's exact pixels"
            );
            for (actual, source) in back.nodes.iter().zip(&doc.nodes) {
                let (
                    NodeKind::Raster { raster: actual, .. },
                    NodeKind::Raster { raster: source, .. },
                ) = (&actual.kind, &source.kind)
                else {
                    panic!("the external save must preserve every raster layer");
                };
                assert_eq!(actual.to_srgba8(), source.to_srgba8());
            }
        } else {
            emulsion_core::geometry::resize(&mut doc, 64, 36).unwrap();
            edit.format.write(&doc, &edit.path).unwrap();
        }
    }

    #[test]
    fn native_bilinear_resize_discloses_appearance_fallback() {
        let root = tempfile::tempdir().unwrap();
        let (p, _) = board();
        let edit = start(&p, EditFormat::Psd, root.path());
        let mut doc = edit.read().unwrap();
        doc.blend_space = emulsion_raster::blend::BlendSpace::PhotoshopSrgbV1;
        Command::AddNode {
            node: Box::new(layer("Shading", [0.2, 0.2, 0.2, 0.5], 32, 18)),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
        emulsion_core::geometry::resize(&mut doc, 64, 36).unwrap();
        let before = doc.clone();
        let expected = emulsion_raster::composite::flatten(&doc.composite_tree(), 0).to_srgba8();
        assert!(
            expected
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] != 255),
            "bilinear scaling must retain the fixture's partial edge coverage"
        );
        assert_eq!(
            crate::psd::write_with_report(&doc, &edit.path).unwrap(),
            crate::psd::WriteReport {
                appearance_fallback: Some(crate::psd::AppearanceFallback::BlendSpaceDifference),
                baked_raster_masks: false,
                rounded_mask_densities: 0,
            },
            "an explicit profile cannot make arbitrary native samples PSD-exact"
        );
        assert_eq!(doc, before, "fallback must preserve the native source");
        let back = edit.read().unwrap();
        assert_eq!((back.width, back.height), (64, 36));
        assert_eq!(back.nodes.len(), 1, "appearance-only export is disclosed");
        assert_eq!(
            emulsion_raster::composite::flatten(&back.composite_tree(), 0).to_srgba8(),
            expected,
            "fallback must retain the original exact appearance"
        );
    }

    #[test]
    fn initial_write_report_is_actual_and_consumed_once_without_changing_the_panel() {
        use crate::psd::{AppearanceFallback, WriteReport};
        let ordinary = WriteReport {
            appearance_fallback: None,
            baked_raster_masks: false,
            rounded_mask_densities: 0,
        };
        for case in ["ordinary", "flattened", "baked_and_rounded", "ora"] {
            let root = tempfile::tempdir().unwrap();
            let (p, _) = board();
            let before = p.page(1).unwrap().doc.clone();
            let stamp = p.stamp();
            let mut doc = before.clone();
            let ink = doc.nodes.iter_mut().find(|n| n.name == "Ink").unwrap();
            if case == "flattened" {
                ink.mask = Some(Arc::new(emulsion_raster::Mask::empty(32, 18, 127)));
            } else if case == "baked_and_rounded" {
                ink.mask = Some(Arc::new(emulsion_raster::Mask::empty(32, 18, 255)));
                {
                    let node = &mut *ink;
                    let mut affine = node
                        .mask_transform
                        .affine()
                        .expect("affine fixture mapping");
                    affine.translation.x = 0.5;
                    node.mask_transform = emulsion_core::Mapping2::Affine(affine);
                }
                ink.mask_enabled = false;
                let background = doc
                    .nodes
                    .iter_mut()
                    .find(|n| n.name == "Background")
                    .unwrap();
                background.mask = Some(Arc::new(emulsion_raster::Mask::empty(32, 18, 255)));
                background.mask_properties.density = 0.1;
                background.mask_enabled = false;
            }
            let original = doc.clone();
            let format = if case == "ora" {
                EditFormat::Ora
            } else {
                EditFormat::Psd
            };
            let mut edit =
                ExternalEdit::start(1, "Panel", doc, format, root.path(), "PaintApp").unwrap();
            let expected = match case {
                "flattened" => Some(WriteReport {
                    appearance_fallback: Some(AppearanceFallback::UnsupportedFeatures),
                    ..ordinary
                }),
                "baked_and_rounded" => Some(WriteReport {
                    baked_raster_masks: true,
                    rounded_mask_densities: 1,
                    ..ordinary
                }),
                "ora" => None,
                _ => Some(ordinary),
            };
            assert_eq!(edit.take_initial_write_report(), expected, "{case}");
            assert!(edit.path.is_file());
            assert_eq!(edit.synced, original);
            let now = Instant::now();
            for i in 0..3 {
                assert!(!edit.watch.poll(FileStamp::of(&edit.path), now + SETTLE * i));
                assert_eq!(edit.take_initial_write_report(), None, "{case}: no replay");
            }
            assert_eq!(p.page(1).unwrap().doc, before);
            assert_eq!(p.stamp(), stamp);
        }
    }

    #[test]
    fn failed_initial_write_returns_no_edit_or_report_and_removes_its_folder() {
        let root = tempfile::tempdir().unwrap();
        let (p, _) = board();
        let mut doc = p.page(1).unwrap().doc.clone();
        // Invalid input fails validation inside the write, after its folder exists.
        doc.width = 0;
        assert!(
            ExternalEdit::start(1, "Panel", doc, EditFormat::Psd, root.path(), "PaintApp").is_err()
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        assert!(
            EditFormat::Psd
                .write_with_report(
                    &p.page(1).unwrap().doc,
                    &root.path().join("missing/panel.psd")
                )
                .is_err()
        );
    }

    #[test]
    fn a_save_comes_back_as_one_undo_step_keeping_keyframes() {
        for format in [EditFormat::Psd, EditFormat::Ora] {
            let root = tempfile::tempdir().unwrap();
            let (mut p, ink) = board();
            let mut edit = start(&p, format, root.path());
            assert!(edit.path.exists());
            assert!(
                edit.path
                    .to_string_lossy()
                    .ends_with(&format!("Panel 1_A.{}", format.extension()))
            );
            edit_outside(&edit);
            assert!(!edit.changed_here(&p));
            let external = edit.read().unwrap();
            assert!(
                edit.bring_back(&mut p, &external, Resolution::TakeExternal)
                    .unwrap()
            );
            let doc = &p.page(1).unwrap().doc;
            assert_eq!(
                (doc.width, doc.height),
                (32, 18),
                "{format:?}: panel size kept"
            );
            assert!(names(doc).contains(&"Shading".to_string()));
            let kept = doc.nodes.iter().find(|n| n.name == "Ink").unwrap();
            assert_eq!(kept.id, ink, "{format:?}: matched by name");
            let panel = &p.storyboard().unwrap().panels[&1];
            assert!(panel.motion.contains_key(&ink));
            assert_eq!(panel.frames, 30);
            assert_eq!(p.history.steps().next().unwrap().name, "Edit in PaintApp");
            assert!(p.undo());
            assert!(!names(&p.page(1).unwrap().doc).contains(&"Shading".to_string()));
            assert_eq!(p.storyboard().unwrap().panels[&1].frames, 30);
        }
    }

    #[test]
    fn a_panel_changed_here_asks_and_each_answer_works() {
        let root = tempfile::tempdir().unwrap();
        let (mut p, _) = board();
        let mut edit = start(&p, EditFormat::Psd, root.path());
        edit_outside(&edit);
        p.execute(Command::AddNode {
            node: Box::new(layer("Mine", [0., 1., 0., 1.], 32, 18)),
            slot: Slot::TOP,
        })
        .unwrap();
        assert!(edit.changed_here(&p), "a conflict to ask about");
        let external = edit.read().unwrap();

        assert!(
            !edit
                .bring_back(&mut p, &external, Resolution::KeepMine)
                .unwrap()
        );
        assert!(names(&p.page(1).unwrap().doc).contains(&"Mine".to_string()));
        assert!(!edit.changed_here(&p), "keeping mine syncs to mine");

        assert!(
            edit.bring_back(&mut p, &external, Resolution::KeepBoth)
                .unwrap()
        );
        let both = names(&p.page(1).unwrap().doc);
        assert!(both.contains(&"Mine".to_string()));
        assert!(both.contains(&"Shading (PaintApp)".to_string()), "{both:?}");
        assert!(both.contains(&"Ink".to_string()));
        assert!(p.undo());

        assert!(
            edit.bring_back(&mut p, &external, Resolution::TakeExternal)
                .unwrap()
        );
        let taken = names(&p.page(1).unwrap().doc);
        assert!(!taken.contains(&"Mine".to_string()));
        assert!(taken.contains(&"Shading".to_string()));
    }

    // Source-only checked-fit controls; the coordinator owns execution.
    fn projected_fit_document() -> Document {
        let mut doc = Document::new(29_994, 1);
        let mut node = Node::smart(
            1,
            "Projected external source",
            Arc::new(Raster::solid(2, 2, [1., 0., 0., 1.])),
            Vec::new(),
            Placement::default(),
        );
        let NodeKind::Smart { placement, .. } = &mut node.kind else {
            unreachable!()
        };
        *placement = emulsion_core::SmartPlacement::Projective(
            emulsion_raster::projective::Projective2::IDENTITY,
        );
        node.styles
            .push(emulsion_core::styles::LayerStyle::DropShadow {
                color: [0; 3],
                opacity: 1.0,
                angle: 0.0,
                distance: 0.0,
                size: 1.0,
            });
        doc.nodes.push(node);
        doc.next_id = 2;
        // The style's three-pixel pad makes the admitted width exactly 30,000.
        doc.validate().unwrap();
        doc
    }

    fn retained_source(doc: &Document) -> &Arc<Raster> {
        doc.nodes
            .iter()
            .find_map(|node| match &node.kind {
                NodeKind::Smart { source, .. } => Some(source),
                _ => None,
            })
            .expect("projected Smart source")
    }

    #[test]
    fn external_layer_candidates_propagate_checked_fit_refusal_without_changing_inputs() {
        use crate::native_relation::{LiveRelation, history_matches};
        let external = projected_fit_document();
        let before_external = external.clone();
        let current = Document::new(29_995, 1);
        let before_current = current.clone();
        current.validate().unwrap();
        // This valid destination is one pixel wider. Its fitted effect canvas
        // would exceed the padded side limit, so no candidate can be published.
        for result in [
            adopt_layers(&current, &external),
            stack_layers(&current, &external, "PaintApp"),
        ] {
            assert!(matches!(result, Err(crate::IoError::Command(_))));
        }
        assert_eq!(
            history_matches(&current, &before_current),
            LiveRelation::Consistent
        );
        assert_eq!(
            history_matches(&external, &before_external),
            LiveRelation::Consistent
        );
        assert!(Arc::ptr_eq(
            retained_source(&external),
            retained_source(&before_external)
        ));

        let supported = Document::new(29_993, 1);
        for result in [
            adopt_layers(&supported, &external),
            stack_layers(&supported, &external, "PaintApp"),
        ] {
            let candidate = result.unwrap();
            candidate.validate().unwrap();
            assert_eq!((candidate.width, candidate.height), (29_993, 1));
            assert!(Arc::ptr_eq(
                retained_source(&candidate),
                retained_source(&external)
            ));
            assert!(
                candidate
                    .nodes
                    .iter()
                    .any(|node| node.has_projective_metadata())
            );
        }
        assert_eq!(
            history_matches(&external, &before_external),
            LiveRelation::Consistent
        );
    }

    #[test]
    fn refused_external_fit_preserves_panel_history_sync_and_managed_file() {
        use crate::native_relation::{LiveRelation, history_matches};
        for resolution in [Resolution::TakeExternal, Resolution::KeepBoth] {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join("session");
            std::fs::create_dir(&dir).unwrap();
            let path = dir.join("panel.ora");
            std::fs::write(&path, b"managed external file remains untouched").unwrap();
            let current = Document::new(29_995, 1);
            let mut project =
                ProjectEditor::new_project(ProjectKind::Storyboard, current.clone()).unwrap();
            project
                .execute(Command::AddNode {
                    node: Box::new(Node::new(
                        0,
                        "Existing redo",
                        NodeKind::Fill {
                            rgba: [0, 0, 0, 255],
                        },
                    )),
                    slot: Slot::TOP,
                })
                .unwrap();
            assert!(project.undo());
            assert!(project.can_redo());
            let stamp = project.stamp();
            let history_len = project.history.len();
            let undoable = project.can_undo();
            let graph = project.page(1).unwrap().graph.clone();
            let mut edit = ExternalEdit {
                panel: 1,
                app: "PaintApp".into(),
                format: EditFormat::Ora,
                dir,
                path: path.clone(),
                synced: current.clone(),
                watch: SaveWatch::new(FileStamp::of(&path)),
                initial_write_report: None,
            };
            let seen = edit.watch.seen;
            let pending = edit.watch.pending;
            let external = projected_fit_document();
            let external_before = external.clone();
            let expected = fit_to_frame(&external, current.width, current.height)
                .unwrap_err()
                .to_string();
            assert_eq!(
                edit.bring_back(&mut project, &external, resolution)
                    .unwrap_err(),
                expected
            );
            assert_eq!(project.stamp(), stamp);
            assert_eq!(project.history.len(), history_len);
            assert_eq!(project.can_undo(), undoable);
            assert!(project.can_redo());
            assert_eq!(
                history_matches(&project.page(1).unwrap().doc, &current),
                LiveRelation::Consistent
            );
            assert_eq!(
                history_matches(&edit.synced, &current),
                LiveRelation::Consistent
            );
            assert_eq!(
                history_matches(&external, &external_before),
                LiveRelation::Consistent
            );
            assert!(Arc::ptr_eq(
                retained_source(&external),
                retained_source(&external_before)
            ));
            assert_eq!(edit.watch.seen, seen);
            assert_eq!(edit.watch.pending, pending);
            let actual_graph = &project.page(1).unwrap().graph;
            assert_eq!(actual_graph.head(), graph.head());
            assert_eq!(actual_graph.branches(), graph.branches());
            assert_eq!(actual_graph.len(), graph.len());
            for (actual, expected) in actual_graph.commits().zip(graph.commits()) {
                assert_eq!(actual.id, expected.id);
                assert_eq!(
                    history_matches(&actual.doc, &expected.doc),
                    LiveRelation::Consistent
                );
            }
            assert_eq!(
                std::fs::read(&path).unwrap(),
                b"managed external file remains untouched"
            );
            // A subsequent supported import still publishes exactly one step.
            let mut supported = external.clone();
            supported.nodes[0].styles.clear();
            assert!(
                edit.bring_back(&mut project, &supported, resolution)
                    .unwrap()
            );
            assert_eq!(
                project.history.steps().next().unwrap().name,
                "Edit in PaintApp"
            );
            assert!(Arc::ptr_eq(
                retained_source(&project.page(1).unwrap().doc),
                retained_source(&external)
            ));
            assert!(project.undo());
            assert_eq!(
                history_matches(&project.page(1).unwrap().doc, &current),
                LiveRelation::Consistent
            );
            assert_eq!(
                std::fs::read(&path).unwrap(),
                b"managed external file remains untouched"
            );
        }
    }

    #[test]
    fn stopping_removes_the_files() {
        let root = tempfile::tempdir().unwrap();
        let (p, _) = board();
        let edit = start(&p, EditFormat::Ora, root.path());
        let path = edit.path.clone();
        assert!(path.exists());
        drop(edit);
        assert!(!path.exists());
        assert!(!path.parent().unwrap().exists());
    }

    #[test]
    fn a_locked_panel_refuses_the_save() {
        let root = tempfile::tempdir().unwrap();
        let (mut p, _) = board();
        let mut edit = start(&p, EditFormat::Psd, root.path());
        edit_outside(&edit);
        p.edit_storyboard(|b| {
            b.panels.get_mut(&1).unwrap().locked = true;
            Ok(())
        })
        .unwrap();
        let external = edit.read().unwrap();
        assert!(
            edit.bring_back(&mut p, &external, Resolution::TakeExternal)
                .is_err()
        );
    }

    #[test]
    fn saves_are_read_once_they_settle() {
        let t = Instant::now();
        let stamp = |len| {
            Some(FileStamp {
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(len),
                len,
            })
        };
        let mut watch = SaveWatch::new(stamp(1));
        assert!(!watch.poll(stamp(1), t));
        assert!(!watch.poll(stamp(2), t), "just saved: wait");
        assert!(!watch.poll(stamp(3), t + SETTLE), "still being written");
        assert!(!watch.poll(stamp(3), t + SETTLE + SETTLE / 2));
        assert!(watch.poll(stamp(3), t + SETTLE * 2), "settled");
        assert!(!watch.poll(stamp(3), t + SETTLE * 3), "read once");
        assert!(
            !watch.poll(None, t + SETTLE * 4),
            "a save replacing the file"
        );
        assert!(!watch.poll(stamp(4), t + SETTLE * 5));
        assert!(watch.poll(stamp(4), t + SETTLE * 6));
    }

    #[test]
    fn edit_folders_belong_to_their_project() {
        let root = edit_root("abc-123/../x");
        assert!(root.ends_with("emulsion-external-edit/abc-123x"));
        assert!(edit_root("").ends_with("emulsion-external-edit/project"));
    }
}
