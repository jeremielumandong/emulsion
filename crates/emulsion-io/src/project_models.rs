//! Shot Generator models in a package: each imported glTF/GLB/OBJ file of
//! the board's shot library is stored as it was imported, as
//! `models/{id}.{ext}` beside the storyboard data, and checked (its id is
//! its content hash and it must parse) when the package opens. Personal
//! library items with sets keep their models the same way.
use crate::{IoError, Result, ora};
use emulsion_core::storyboard_shot::{MAX_MODEL_BYTES, ShotLibrary};
use std::io::{Read, Seek, Write};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

/// Write every model of `library`.
pub(crate) fn write_models<W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    library: &ShotLibrary,
) -> Result<()> {
    if library.model_bytes() > MAX_MODEL_BYTES {
        return Err(IoError::Manifest(format!(
            "The project's 3D models exceed {} MB.",
            MAX_MODEL_BYTES >> 20
        )));
    }
    for (id, asset) in &library.models {
        if asset.data.is_empty() {
            return Err(IoError::Manifest(format!(
                "3D model “{}” has no data to save.",
                asset.name
            )));
        }
        let entry = library.entry_name(id).expect("listed model");
        zip.start_file(entry, SimpleFileOptions::default().large_file(true))?;
        zip.write_all(&asset.data)?;
    }
    Ok(())
}

/// Read every model `library` lists, then check they parse.
pub(crate) fn read_models<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    library: &mut ShotLibrary,
) -> Result<()> {
    let mut budget = MAX_MODEL_BYTES as u64;
    let ids: Vec<String> = library.models.keys().cloned().collect();
    for id in ids {
        let entry = library.entry_name(&id).expect("listed model");
        let name = library.models[&id].name.clone();
        let bytes = ora::read_entry(zip, &entry, budget).map_err(|_| {
            IoError::Manifest(format!("3D model “{name}” is missing or too large."))
        })?;
        budget = budget.saturating_sub(bytes.len() as u64);
        library.models.get_mut(&id).unwrap().data = bytes.into();
    }
    library.check_models().map_err(IoError::Manifest)
}

#[cfg(test)]
mod tests {
    use super::super::{STORYBOARD_ENTRY, read, write};
    use emulsion_core::Document;
    use emulsion_core::project::{ProjectEditor, ProjectKind};
    use emulsion_scene::{Character, MannequinKind};
    use glam::Vec3;
    use std::io::Read;
    use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

    const OBJ: &[u8] = b"o box\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";

    fn path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "emulsion-models-{label}-{}.emu",
            std::process::id()
        ))
    }

    fn board() -> ProjectEditor {
        ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(64, 36)).unwrap()
    }

    #[test]
    fn sets_and_their_models_round_trip_in_the_package() {
        let file = path("round-trip");
        let mut session = board();
        session
            .edit_panel_shot(1, "Add", |shot, _| {
                shot.set
                    .add_character("Mia", Character::of(MannequinKind::Child), Vec3::ZERO, 30.);
                Ok(())
            })
            .unwrap();
        session
            .import_shot_model(1, "Crate.obj", OBJ.to_vec(), Vec3::X)
            .unwrap();
        session.update_shot_reference(1).unwrap();
        let project = session.snapshot().unwrap();
        write(&project, &file).unwrap();
        let back = read(&file).unwrap();
        assert_eq!(back.storyboard, project.storyboard);
        let library = &back.storyboard.as_ref().unwrap().shot_library;
        let (id, model) = library.models.iter().next().unwrap();
        assert_eq!(&*model.data, OBJ);
        assert!(
            library.assets().get(id).is_some(),
            "the asset library is rebuilt"
        );
        let mut zip = ZipArchive::new(std::fs::File::open(&file).unwrap()).unwrap();
        assert!(zip.by_name(&format!("models/{id}.obj")).is_ok());
        // A package missing its model, or with a damaged one, does not open.
        for (label, replace) in [("missing", None), ("damaged", Some(&b"v 1 2 3\n"[..]))] {
            let broken = path(label);
            let mut out = ZipWriter::new(std::fs::File::create(&broken).unwrap());
            let mut zip = ZipArchive::new(std::fs::File::open(&file).unwrap()).unwrap();
            for i in 0..zip.len() {
                let mut entry = zip.by_index(i).unwrap();
                let name = entry.name().to_string();
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).unwrap();
                if name.starts_with("models/") {
                    let Some(replace) = replace else { continue };
                    bytes = replace.to_vec();
                }
                out.start_file(name, SimpleFileOptions::default()).unwrap();
                std::io::Write::write_all(&mut out, &bytes).unwrap();
            }
            out.finish().unwrap();
            let error = read(&broken).err().unwrap().to_string();
            assert!(error.contains("Crate"), "{label}: {error}");
            std::fs::remove_file(broken).unwrap();
        }
        std::fs::remove_file(file).unwrap();
    }

    #[test]
    fn boards_without_sets_save_as_before() {
        let file = path("plain");
        write(&board().snapshot().unwrap(), &file).unwrap();
        let mut zip = ZipArchive::new(std::fs::File::open(&file).unwrap()).unwrap();
        let mut json = String::new();
        zip.by_name(STORYBOARD_ENTRY)
            .unwrap()
            .read_to_string(&mut json)
            .unwrap();
        for field in ["shot_library", "\"shot\"", "\"depth\"", "models/"] {
            assert!(!json.contains(field), "{field}");
        }
        assert!((0..zip.len()).all(|i| !zip.by_index(i).unwrap().name().starts_with("models/")));
        assert!(
            read(&file)
                .unwrap()
                .storyboard
                .unwrap()
                .shot_library
                .is_empty()
        );
        std::fs::remove_file(file).unwrap();
    }
}
