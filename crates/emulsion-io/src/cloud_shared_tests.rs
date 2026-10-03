//! Two devices share one storyboard through a fake provider: no network
//! and no real account.
use super::*;
use emulsion_cloud::{Provider, providers::synchronize};
use emulsion_core::Document;
use emulsion_core::project::{ProjectEditor, ProjectKind};
use emulsion_core::storyboard::Panel;
use emulsion_core::storyboard_merge::{ConflictKey, Resolution};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Write;

/// The provider's file listing, shared by both devices.
#[derive(Default)]
struct Fake {
    entries: RefCell<Vec<(RemoteRevision, Vec<u8>)>>,
}
impl FileProvider for Fake {
    fn list(&self) -> Result<Vec<RemoteRevision>> {
        Ok(self.entries.borrow().iter().map(|e| e.0.clone()).collect())
    }
    fn upload(&self, revision: &Revision, object: &Path) -> Result<()> {
        self.entries.borrow_mut().push((
            RemoteRevision {
                remote_id: revision.id.clone(),
                revision: revision.clone(),
            },
            std::fs::read(object)?,
        ));
        Ok(())
    }
    fn download(&self, remote: &RemoteRevision, out: &mut File) -> Result<()> {
        let entries = self.entries.borrow();
        let entry = entries
            .iter()
            .find(|e| e.0.remote_id == remote.remote_id)
            .context("missing")?;
        out.write_all(&entry.1)?;
        Ok(())
    }
}

fn account() -> Account {
    Account {
        provider: Provider::Dropbox,
        id: "team".into(),
        registration: String::new(),
        label: "Studio".into(),
        root: String::new(),
        persistent_credentials: false,
    }
}

/// A device: its own cloud folder and index, the shared account.
fn device(root: &Path, name: &str) -> Store {
    let store = Store::new(root.join(name));
    store.connect(account()).unwrap();
    store.set_author(Some(name)).unwrap();
    store
}

fn save(store: &Store, editor: &ProjectEditor, path: &Path, cloud: &Fake) {
    crate::project::write(&editor.snapshot().unwrap(), path).unwrap();
    assert!(super::super::enqueue_with_home(store, path, None).unwrap());
    synchronize(store, &account(), cloud).unwrap();
}

fn caption(e: &mut ProjectEditor, page: u64, field: &str, text: &str) {
    e.edit_storyboard(|b| {
        let id = b.caption(field).unwrap();
        b.panels
            .get_mut(&page)
            .unwrap()
            .captions
            .insert(id, text.into());
        Ok(())
    })
    .unwrap();
}

fn text(e: &ProjectEditor, page: u64, field: &str) -> String {
    let b = e.storyboard().unwrap();
    b.panels[&page]
        .captions
        .get(&b.caption(field).unwrap())
        .map(|c| c.text.clone())
        .unwrap_or_default()
}

#[test]
fn two_devices_save_concurrently_and_a_merge_leaves_one_head() {
    let dir = tempfile::tempdir().unwrap();
    let cloud = Fake::default();
    let (a, b) = (device(dir.path(), "Maya"), device(dir.path(), "Ravi"));

    // Maya starts the board and syncs it.
    let mut maya =
        ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(32, 18)).unwrap();
    let blank = maya.storyboard().unwrap().blank_panel().unwrap();
    let items = (2..=3)
        .map(|n| (format!("Panel {n}"), Panel::new(0, 24)))
        .collect();
    maya.insert_panels(Some(1), &blank, items, None).unwrap();
    let p: Vec<u64> = maya.page_list().iter().map(|m| m.id).collect();
    let maya_path = dir.path().join("board.emu");
    crate::project::write(&maya.snapshot().unwrap(), &maya_path).unwrap();
    a.bind(&maya_path, Provider::Dropbox).unwrap();
    save(&a, &maya, &maya_path, &cloud);
    let first = cloud.list().unwrap()[0].clone();

    // Ravi downloads it on his machine.
    let ravi_path = shared::download_copy(&b, &account(), &cloud, &first).unwrap();
    let mut ravi = ProjectEditor::open(
        crate::project::read(&ravi_path).unwrap(),
        Some(ravi_path.clone()),
    )
    .unwrap();

    // Both work from the same base and save: two heads.
    caption(&mut ravi, p[1], "Action", "Ravi's action");
    caption(&mut ravi, p[0], "Notes", "Ravi was here");
    ravi.insert_panels(
        Some(p[2]),
        &blank,
        vec![("Ravi new".into(), Panel::new(0, 12))],
        None,
    )
    .unwrap();
    save(&b, &ravi, &ravi_path, &cloud);
    caption(&mut maya, p[1], "Action", "Maya's action");
    caption(&mut maya, p[2], "Dialogue", "Maya's line");
    maya.insert_panels(
        Some(p[2]),
        &blank,
        vec![("Maya new".into(), Panel::new(0, 12))],
        None,
    )
    .unwrap();
    save(&a, &maya, &maya_path, &cloud);
    let listing = cloud.list().unwrap();
    assert_eq!(emulsion_cloud::heads(&listing).len(), 2);

    // Maya sees Ravi's save waiting, with him among the collaborators,
    // from the listing her last sync remembered.
    a.remember_remote(&first.revision.project, &listing)
        .unwrap();
    let state = shared::sharing(&a, &maya_path, None).unwrap().unwrap();
    assert_eq!(state.waiting.len(), 1);
    assert_eq!(state.waiting[0].revision.author.as_deref(), Some("Ravi"));
    let names: Vec<_> = state
        .collaborators
        .iter()
        .filter_map(|c| c.author.clone())
        .collect();
    assert!(names.contains(&"Ravi".to_string()) && names.contains(&"Maya".to_string()));

    // She merges it, taking Ravi's version of the panel both changed.
    let prepared = shared::prepare_merge(&a, &cloud, &maya_path, None).unwrap();
    assert_eq!(prepared.base.id, first.revision.id);
    assert_eq!(prepared.waiting, 0);
    let base = crate::project::read(&prepared.base_path).unwrap();
    let theirs = crate::project::read(&prepared.theirs).unwrap();
    let choices = BTreeMap::from([(ConflictKey::Panel(p[1]), Resolution::Theirs)]);
    let report = maya
        .merge_board(&base, &theirs, &choices, Some(&prepared.head.id))
        .unwrap();
    assert_eq!(report.conflicts.len(), 1);
    save(&a, &maya, &maya_path, &cloud);

    // One head, with both parents.
    let listing = cloud.list().unwrap();
    let heads = emulsion_cloud::heads(&listing);
    assert_eq!(heads.len(), 1);
    let merge = &heads[0].revision;
    assert_eq!(merge.merged.as_deref(), Some(prepared.head.id.as_str()));
    assert!(
        shared::sharing(&a, &maya_path, Some(&listing))
            .unwrap()
            .unwrap()
            .waiting
            .is_empty()
    );
    // Both artists' changes are in it, and the conflict went Ravi's way.
    let merged = ProjectEditor::open(
        crate::project::read(&shared::fetch_revision(&a, &cloud, &heads[0]).unwrap()).unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(text(&merged, p[1], "Action"), "Ravi's action");
    assert_eq!(text(&merged, p[0], "Notes"), "Ravi was here");
    assert_eq!(text(&merged, p[2], "Dialogue"), "Maya's line");
    let names: Vec<_> = merged.page_list().iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        ["Page 1", "Panel 2", "Panel 3", "Maya new", "Ravi new"]
    );

    // Ravi now sees the merge waiting; merging it brings him level.
    let state = shared::sharing(&b, &ravi_path, Some(&listing))
        .unwrap()
        .unwrap();
    assert_eq!(state.waiting.len(), 1);
    let prepared = shared::prepare_merge(&b, &cloud, &ravi_path, None).unwrap();
    let base = crate::project::read(&prepared.base_path).unwrap();
    let theirs = crate::project::read(&prepared.theirs).unwrap();
    let report = ravi
        .merge_board(&base, &theirs, &BTreeMap::new(), Some(&prepared.head.id))
        .unwrap();
    assert!(report.conflicts.is_empty());
    assert_eq!(text(&ravi, p[2], "Dialogue"), "Maya's line");
    save(&b, &ravi, &ravi_path, &cloud);
    assert_eq!(emulsion_cloud::heads(&cloud.list().unwrap()).len(), 1);
    // The fetched revisions were verified and cached once.
    assert!(a.root.join("revisions").join(&first.revision.id).is_dir());
}
