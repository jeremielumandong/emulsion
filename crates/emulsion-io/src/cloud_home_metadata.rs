//! Portable Home membership: local numeric IDs and paths never leave the device.
use super::*;
use crate::creative_library::{self as library, Catalog};
use emulsion_cloud::{HomeFolder, HomeMetadata, Revision};
use emulsion_core::creation::CanvasKind;

pub fn catalog(root: &Path) -> Result<Catalog> {
    let catalog = library::load(root)?;
    if catalog.folders.iter().all(|f| f.cloud_id.is_some()) {
        return Ok(catalog);
    }
    Ok(library::update(root, |c| {
        for folder in &mut c.folders {
            folder.cloud_id.get_or_insert_with(emulsion_cloud::id);
        }
        Ok(())
    })?
    .0)
}

pub fn metadata(catalog: &Catalog, source: &Path) -> Option<HomeMetadata> {
    let record = catalog.projects.iter().find(|p| p.path == source)?;
    Some(HomeMetadata {
        name: record.name.clone(),
        folder: record
            .folder
            .and_then(|id| catalog.folders.iter().find(|f| f.id == id))
            .and_then(|f| {
                Some(HomeFolder {
                    id: f.cloud_id.clone()?,
                    name: f.name.clone(),
                })
            }),
        kind: record
            .kind_override
            .or_else(|| {
                (image::ImageFormat::from_path(source).is_ok() || crate::raw::is_raw(source))
                    .then_some(CanvasKind::Photo)
            })
            .or(record.kind)
            // Readers before storyboards reject other kinds in a header and
            // could not list the account; a storyboard travels unclassified.
            .filter(|k| *k != CanvasKind::Storyboard)
            .map(|k| k.label().into()),
    })
}

/// Only changed organization queues a new saved snapshot. Content saves use the
/// ordinary save worker; paused or missing files are left alone.
pub fn enqueue_changes(store: &Store) -> Result<Vec<String>> {
    let index = store.read()?;
    if index.bindings.is_empty() {
        return Ok(vec![]);
    }
    let catalog = catalog(&library::root())?;
    let mut errors = vec![];
    for binding in index.bindings.iter().filter(|b| !b.paused) {
        let home = metadata(&catalog, &binding.path);
        // A forgotten local reference must not erase its cloud organization.
        if home.is_none() || home == binding.saved_home {
            continue;
        }
        if let Err(error) = super::enqueue_with_home(store, &binding.path, home) {
            errors.push(format!(
                "Could not sync project details for {}: {error}",
                binding
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            ));
        }
    }
    Ok(errors)
}

/// Restore membership by portable ID, never by a potentially colliding name.
pub fn restore(root: &Path, path: &Path, revision: &Revision) -> Result<Catalog> {
    revision.validate()?;
    let home = revision.home.as_ref();
    let kind = home
        .and_then(|h| h.kind.as_deref())
        .and_then(|label| CanvasKind::ALL.into_iter().find(|k| k.label() == label));
    Ok(library::update(root, |catalog| {
        let folder = if let Some(remote) = home.and_then(|h| h.folder.as_ref()) {
            if let Some(folder) = catalog
                .folders
                .iter()
                .find(|f| f.cloud_id.as_deref() == Some(&remote.id))
            {
                Some(folder.id)
            } else {
                let id = catalog.add_project_folder(remote.name.clone())?;
                catalog
                    .folders
                    .iter_mut()
                    .find(|f| f.id == id)
                    .unwrap()
                    .cloud_id = Some(remote.id.clone());
                Some(id)
            }
        } else {
            None
        };
        let id = catalog.remember_project(
            &crate::recent::Recent {
                path: path.into(),
                opened: crate::recent::now(),
                summary: "Downloaded from cloud".into(),
            },
            kind,
        )?;
        let record = catalog.projects.iter_mut().find(|p| p.id == id).unwrap();
        if let Some(home) = home {
            record.name = home.name.clone();
            record.kind_override = kind;
            record.folder = folder;
        }
        Ok(())
    })?
    .0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_membership_and_assigned_names_round_trip_for_multiple_files() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("original-library");
        let receiving = dir.path().join("receiving-library");
        let store = Store::new(dir.path().join("cloud"));
        store
            .connect(emulsion_cloud::Account {
                provider: emulsion_cloud::Provider::GoogleDrive,
                id: "test".into(),
                registration: "test".into(),
                label: "test".into(),
                root: "test-folder".into(),
                persistent_credentials: false,
            })
            .unwrap();
        // Identical project names on two devices must not merge unrelated work.
        library::update(&receiving, |c| c.add_project_folder("Incubarity".into())).unwrap();
        let folder = library::update(&original, |c| c.add_project_folder("Incubarity".into()))
            .unwrap()
            .1;
        for (name, title) in [
            ("portrait.jpg", "Cover portrait"),
            ("sample.png", "Campaign sample"),
        ] {
            let source = dir.path().join(name);
            image::RgbImage::from_pixel(2, 2, image::Rgb([80, 120, 160]))
                .save(&source)
                .unwrap();
            library::update(&original, |c| {
                let id = c.remember_project(
                    &crate::recent::Recent {
                        path: source.clone(),
                        opened: 1,
                        summary: String::new(),
                    },
                    Some(CanvasKind::Photo),
                )?;
                c.move_project(id, Some(folder))?;
                c.projects.iter_mut().find(|p| p.id == id).unwrap().name = title.into();
                Ok(())
            })
            .unwrap();
            store
                .bind(&source, emulsion_cloud::Provider::GoogleDrive)
                .unwrap();
            let home = metadata(&catalog(&original).unwrap(), &source);
            assert!(super::super::enqueue_with_home(&store, &source, home.clone()).unwrap());
            let revision = store.read().unwrap().jobs.last().unwrap().revision.clone();
            let payload = dir.path().join(format!("{name}.zip"));
            let mut out = File::create(&payload).unwrap();
            emulsion_cloud::store::extract_object(
                &store.object_path(&revision.id).unwrap(),
                &revision,
                &mut out,
            )
            .unwrap();
            let destination = dir.path().join(format!("download-{name}"));
            std::fs::create_dir(&destination).unwrap();
            let downloaded = super::super::unpack(&payload, &destination).unwrap();
            assert_eq!(downloaded.file_name().unwrap(), name);
            let restored = restore(&receiving, &downloaded, &revision).unwrap();
            let record = restored
                .projects
                .iter()
                .find(|p| p.path == downloaded)
                .unwrap();
            assert_eq!(record.name, title);
            assert_eq!(record.kind_override, Some(CanvasKind::Photo));
            assert_eq!(metadata(&restored, &downloaded), home);
        }
        let restored = library::load(&receiving).unwrap();
        assert_eq!(restored.folders.len(), 2);
        assert_eq!(restored.projects[0].folder, restored.projects[1].folder);
        assert_ne!(restored.projects[0].folder, Some(restored.folders[0].id));
    }
}
