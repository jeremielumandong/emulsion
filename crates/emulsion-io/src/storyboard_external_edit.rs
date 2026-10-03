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
    /// Layered Photoshop document, read by Photoshop, Krita, GIMP,
    /// Affinity and most painting apps.
    #[default]
    Psd,
    /// OpenRaster, Krita's and MyPaint's exchange format.
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
        match self {
            Self::Psd => crate::psd::write(doc, path),
            Self::Ora => crate::ora::write(doc, path),
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
        if let Err(e) = format.write(&doc, &path) {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e);
        }
        Ok(Self {
            panel,
            app: app.into(),
            format,
            watch: SaveWatch::new(FileStamp::of(&path)),
            dir,
            path,
            synced: doc,
        })
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
        };
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
/// stays the panel's.
pub fn adopt_layers(current: &Document, external: &Document) -> Document {
    let mut ext = fit_to_frame(external, current.width, current.height);
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
    doc.nodes = ext.nodes;
    doc.next_id = next;
    doc
}

/// `current` with `external`'s layers (fitted, with new IDs) added on top,
/// each top-level one named after `app`.
pub fn stack_layers(current: &Document, external: &Document, app: &str) -> Document {
    let mut ext = fit_to_frame(external, current.width, current.height);
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
    doc
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
        ExternalEdit::start(1, "Panel 1/A", doc, format, root, "Krita").unwrap()
    }

    /// What the external app does: read the file, add a layer, double the
    /// canvas (the panel must keep its size) and save.
    fn edit_outside(edit: &ExternalEdit) {
        let mut doc = edit.read().unwrap();
        let mut editor = emulsion_core::Editor::new(doc.clone(), None);
        editor
            .execute(Command::AddNode {
                node: Box::new(layer("Shading", [0.2, 0.2, 0.2, 0.5], 32, 18)),
                slot: Slot::TOP,
            })
            .unwrap();
        doc = editor.doc;
        emulsion_core::geometry::resize(&mut doc, 64, 36);
        edit.format.write(&doc, &edit.path).unwrap();
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
            assert_eq!(p.history.steps().next().unwrap().name, "Edit in Krita");
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
        assert!(both.contains(&"Shading (Krita)".to_string()), "{both:?}");
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
