//! Saved recipes on disk and applying a recipe to a document.

use crate::{Compiled, Recipe, RecipeError, starter_set};
use emulsion_core::command::Slot;
use emulsion_core::{Command, Editor, NodeId};
use std::path::{Path, PathBuf};

/// Where a recipe came from.
#[derive(Clone, Debug, PartialEq)]
pub enum Origin {
    Starter,
    Saved(PathBuf),
}

/// Recipes shipped with Emulsion plus those saved under `dir`, saved ones
/// first. Unreadable files are skipped.
pub fn list(dir: &Path) -> Vec<(Recipe, Origin)> {
    let mut out: Vec<(Recipe, Origin)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        let mut paths: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.to_string_lossy().ends_with(".recipe.toml"))
            .collect();
        paths.sort();
        for p in paths {
            if let Ok(text) = std::fs::read_to_string(&p)
                && let Ok(r) = Recipe::from_toml(&text)
            {
                out.push((r, Origin::Saved(p)));
            }
        }
    }
    for r in starter_set().into_iter().chain(crate::cameras::presets()) {
        if !out
            .iter()
            .any(|(o, _)| o.name.eq_ignore_ascii_case(&r.name))
        {
            out.push((r, Origin::Starter));
        }
    }
    out
}

/// Find a recipe by name, saved ones first.
pub fn find(dir: &Path, name: &str) -> Option<Recipe> {
    let n = name.trim().to_lowercase();
    list(dir)
        .into_iter()
        .find(|(r, _)| r.name.to_lowercase() == n)
        .map(|(r, _)| r)
}

fn file_name(name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    format!(
        "{}.recipe.toml",
        if safe.is_empty() {
            "recipe".to_string()
        } else {
            safe
        }
    )
}

/// Save an import, replacing only an existing saved recipe with the same name.
/// An unrelated recipe with a colliding sanitized filename is never replaced.
pub fn save(dir: &Path, recipe: &Recipe) -> Result<PathBuf, RecipeError> {
    if list(dir).iter().any(|(r, origin)| {
        matches!(origin, Origin::Saved(_)) && r.name.eq_ignore_ascii_case(&recipe.name)
    }) {
        update(dir, &recipe.name, recipe)
    } else {
        create_saved(dir, recipe, false)
    }
}

/// Create a new recipe without replacing any saved or built-in name.
pub fn save_new(dir: &Path, recipe: &Recipe) -> Result<PathBuf, RecipeError> {
    create_saved(dir, recipe, true)
}

fn serialized(recipe: &Recipe) -> Result<String, RecipeError> {
    let text = toml::to_string_pretty(recipe)
        .map_err(|e| RecipeError::Invalid(format!("cannot serialize recipe: {e}")))?;
    if text.trim().is_empty() {
        return Err(RecipeError::Invalid(
            "recipe serialization was empty".into(),
        ));
    }
    Ok(text)
}

/// Write and sync privately. Neither readers nor an interrupted write can see a
/// half recipe. The temporary extension is deliberately not .recipe.toml.
fn stage(dir: &Path, text: &str) -> Result<PathBuf, RecipeError> {
    use std::io::Write;
    std::fs::create_dir_all(dir).map_err(|e| RecipeError::Invalid(e.to_string()))?;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let (path, mut file) = loop {
        let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = dir.join(format!(".recipe-stage-{}-{serial}.tmp", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => break (path, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(RecipeError::Invalid(e.to_string())),
        }
    };
    if let Err(e) = file
        .write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
    {
        drop(file);
        let _ = std::fs::remove_file(&path);
        return Err(RecipeError::Invalid(e.to_string()));
    }
    Ok(path)
}

fn create_saved(dir: &Path, recipe: &Recipe, reject_builtin: bool) -> Result<PathBuf, RecipeError> {
    let recipe = recipe.portable()?;
    if list(dir).iter().any(|(r, origin)| {
        r.name.eq_ignore_ascii_case(&recipe.name)
            && (reject_builtin || matches!(origin, Origin::Saved(_)))
    }) {
        return Err(RecipeError::Invalid(
            "a recipe with this name already exists; choose a new name or explicitly update it"
                .into(),
        ));
    }
    let text = serialized(&recipe)?;
    let path = dir.join(file_name(&recipe.name));
    let tmp = stage(dir, &text)?;
    // Atomic, exclusive publication. On filesystems without hard links, fail
    // clearly rather than expose a partial file or risk replacing another one.
    let result = std::fs::hard_link(&tmp, &path);
    let _ = std::fs::remove_file(&tmp);
    result.map_err(|e| {
        RecipeError::Invalid(format!(
            "cannot publish {} without overwriting: {e}",
            path.display()
        ))
    })?;
    Ok(path)
}

/// Replace an explicitly named saved recipe. Renaming requires Save as New.
/// Resolve its actual path from the library, never a guessed sanitized filename.
pub fn update(dir: &Path, existing_name: &str, recipe: &Recipe) -> Result<PathBuf, RecipeError> {
    let recipe = recipe.portable()?;
    if !recipe.name.eq_ignore_ascii_case(existing_name.trim()) {
        return Err(RecipeError::Invalid(
            "use Save as New to rename a recipe".into(),
        ));
    }
    let paths: Vec<_> = list(dir)
        .into_iter()
        .filter_map(|(r, origin)| match origin {
            Origin::Saved(path) if r.name.eq_ignore_ascii_case(existing_name.trim()) => Some(path),
            _ => None,
        })
        .collect();
    if paths.len() != 1 {
        return Err(RecipeError::Invalid("update requires exactly one existing saved recipe; built-in recipes cannot be replaced".into()));
    }
    let path = &paths[0];
    let tmp = stage(dir, &serialized(&recipe)?)?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(RecipeError::Invalid(e.to_string()));
    }
    Ok(path.clone())
}

pub fn delete(dir: &Path, name: &str) -> bool {
    std::fs::remove_file(dir.join(file_name(name))).is_ok()
}

/// Add a compiled recipe to the document as one history step: the group
/// at `slot`, its stages inside. Returns the group's id.
pub fn add_to(editor: &mut Editor, compiled: Compiled, slot: Slot) -> Result<NodeId, RecipeError> {
    let (group, children) = compiled;
    let label = group.name.clone();
    editor.begin(label);
    let result = (|| {
        let gid = editor
            .execute(Command::AddNode {
                node: Box::new(group),
                slot,
            })
            .map_err(|e| RecipeError::Invalid(e.to_string()))?
            .ok_or_else(|| RecipeError::Invalid("no group was created".into()))?;
        for (i, child) in children.into_iter().enumerate() {
            editor
                .execute(Command::AddNode {
                    node: Box::new(child),
                    slot: Slot {
                        parent: Some(gid),
                        index: i,
                    },
                })
                .map_err(|e| RecipeError::Invalid(e.to_string()))?;
        }
        Ok(gid)
    })();
    editor.end();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::{Document, NodeKind};

    #[test]
    fn save_list_find_delete_and_apply() {
        let dir =
            std::env::temp_dir().join(format!("emulsion-recipes-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut mine = starter_set()[0].clone();
        mine.name = "My Chrome".into();
        let p = save(&dir, &mine).unwrap();
        assert!(p.ends_with("My_Chrome.recipe.toml"));
        let all = list(&dir);
        assert_eq!(all[0].0.name, "My Chrome");
        assert!(matches!(all[0].1, Origin::Saved(_)));
        assert!(all.len() > starter_set().len());
        assert_eq!(find(&dir, "my chrome").unwrap().name, "My Chrome");

        let mut e = Editor::new(Document::new(64, 64), None);
        let gid = add_to(&mut e, mine.compile(None), Slot::TOP).unwrap();
        let g = e.doc.node(gid).unwrap();
        assert!(g.is_group() && g.name == "Recipe · My Chrome");
        let kids = e.doc.children(Some(gid));
        assert!(kids.len() >= 6);
        assert!(
            kids.iter()
                .all(|k| matches!(e.doc.node(*k).unwrap().kind, NodeKind::Adjust(_)))
        );
        assert_eq!(e.history.len(), 1, "one undo step");
        assert!(delete(&dir, "My Chrome"));
        assert!(
            find(&dir, "My Chrome").is_none(),
            "deleted recipes are gone"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
