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

/// Save (or overwrite) a recipe under `dir`. Returns the path.
pub fn save(dir: &Path, recipe: &Recipe) -> Result<PathBuf, RecipeError> {
    recipe.validate()?;
    std::fs::create_dir_all(dir)
        .map_err(|e| RecipeError::Invalid(format!("cannot create {}: {e}", dir.display())))?;
    let path = dir.join(file_name(&recipe.name));
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, recipe.to_toml()).map_err(|e| RecipeError::Invalid(e.to_string()))?;
    std::fs::rename(&tmp, &path).map_err(|e| RecipeError::Invalid(e.to_string()))?;
    Ok(path)
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
