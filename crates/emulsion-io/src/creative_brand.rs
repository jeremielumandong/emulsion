//! Nested creative asset folders are references, independent of Home projects.
use crate::{
    IoError, Result,
    creative_library::{AssetFolder, Catalog},
};
use std::collections::HashSet;
fn error(message: &str) -> IoError {
    IoError::Manifest(message.into())
}
impl Catalog {
    pub(crate) fn validate_asset_folders(&self) -> Result<()> {
        for folder in &self.asset_folders {
            if folder.name.trim().is_empty()
                || folder.name.chars().count() > 200
                || folder.name.chars().any(char::is_control)
            {
                return Err(error("Asset folders need a name of 1–200 characters."));
            }
            let mut current = Some(folder.id);
            let mut seen = HashSet::new();
            while let Some(id) = current {
                if !seen.insert(id) || seen.len() > 32 {
                    return Err(error(
                        "Asset folders must be acyclic and no deeper than 32 levels.",
                    ));
                }
                current = self
                    .asset_folders
                    .iter()
                    .find(|f| f.id == id)
                    .ok_or_else(|| error("Asset folder parent no longer exists."))?
                    .parent;
            }
        }
        Ok(())
    }
    pub fn set_asset_folder(
        &mut self,
        id: Option<u64>,
        name: String,
        parent: Option<u64>,
    ) -> Result<u64> {
        let mut next = self.clone();
        let id = if let Some(id) = id {
            let f = next
                .asset_folders
                .iter_mut()
                .find(|f| f.id == id)
                .ok_or_else(|| error("Asset folder no longer exists."))?;
            f.name = name;
            f.parent = parent;
            id
        } else {
            let id = next.next_id;
            next.next_id = id
                .checked_add(1)
                .filter(|id| *id < u64::MAX)
                .ok_or_else(|| error("Catalog ID limit reached."))?;
            next.asset_folders.push(AssetFolder { id, name, parent });
            id
        };
        next.validate()?;
        *self = next;
        Ok(id)
    }
    pub fn move_creative_asset(&mut self, id: u64, folder: Option<u64>) -> Result<()> {
        if folder.is_some_and(|id| !self.asset_folders.iter().any(|f| f.id == id)) {
            return Err(error("Asset folder no longer exists."));
        }
        self.assets
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or_else(|| error("Asset no longer exists."))?
            .folder = folder;
        Ok(())
    }
    pub fn remove_asset_folder(&mut self, id: u64) -> Result<()> {
        let parent = self
            .asset_folders
            .iter()
            .find(|f| f.id == id)
            .ok_or_else(|| error("Asset folder no longer exists."))?
            .parent;
        self.asset_folders.retain(|f| f.id != id);
        for f in &mut self.asset_folders {
            if f.parent == Some(id) {
                f.parent = parent;
            }
        }
        for asset in &mut self.assets {
            if asset.folder == Some(id) {
                asset.folder = parent;
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn creative_asset_folders_are_nested_cycle_safe_and_non_destructive() {
        let mut c = Catalog::default();
        let a = c.set_asset_folder(None, "Campaign".into(), None).unwrap();
        let b = c.set_asset_folder(None, "Logos".into(), Some(a)).unwrap();
        let before = c.clone();
        assert!(
            c.set_asset_folder(Some(a), "Campaign".into(), Some(b))
                .is_err()
        );
        assert_eq!(c, before);
        c.remove_asset_folder(a).unwrap();
        assert_eq!(c.asset_folders[0].parent, None);
        c.validate().unwrap();
        let mut old = serde_json::to_value(&c).unwrap();
        old.as_object_mut().unwrap().remove("asset_folders");
        let restored: Catalog = serde_json::from_value(old).unwrap();
        assert!(restored.asset_folders.is_empty());
    }
}
