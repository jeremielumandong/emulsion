//! Replacing a storyboard panel's whole drawing, for drawings that come
//! back from outside the app (Edit in an external editor).
use super::{PageId, ProjectEditor};
use crate::Document;
use std::collections::BTreeMap;

impl ProjectEditor {
    /// Replace `panel`'s whole drawing with `doc` (at the panel's size) as
    /// one Undo step labelled `label`, keeping its storyboard data.
    pub fn replace_panel_document(
        &mut self,
        panel: PageId,
        doc: Document,
        label: &str,
    ) -> Result<(), String> {
        let board = self.board()?;
        if self.page(panel).is_none() {
            return Err("Panel does not exist.".into());
        }
        if board.is_locked(panel) {
            return Err(crate::CommandError::ReadOnly.to_string());
        }
        if (doc.width, doc.height) != (board.settings.width, board.settings.height) {
            return Err("The drawing must be the storyboard's size.".into());
        }
        self.commit_documents(BTreeMap::from([(panel, doc)]), label)
    }
}
