//! Import script › Break down with the assistant…: instead of the
//! mechanical one-panel-per-beat split, hand the script to the assistant,
//! which reads it with read_storyboard_script, plans shot coverage and
//! builds the board with build_storyboard_from_breakdown (one Undo step).
//! Offered only when an assistant CLI is installed.
use super::*;
use crate::app_state::{self, CliStatus};
use emulsion_core::project::PageId;
use std::path::Path;

/// Why the assistant cannot take the breakdown right now, if it cannot.
pub(crate) fn assistant_unavailable(cx: &App) -> Option<String> {
    let label = emulsion_assistant::provider::by_id(&app_state::settings(cx).provider).label;
    match app_state::cli(cx) {
        CliStatus::Found { .. } => None,
        CliStatus::Checking => Some(format!("Looking for {label}…")),
        CliStatus::Missing => Some(format!(
            "Breaking a script down needs the assistant, and {label} is not installed. See Settings."
        )),
    }
}

/// The request the assistant gets for `path`, placed after panel `after`.
pub(crate) fn breakdown_request(path: &Path, after: PageId) -> String {
    format!(
        "Break down the screenplay at {} into storyboard scenes, panels and captions. \
         Follow the storyboarding playbook: read every page of it with read_storyboard_script, \
         plan the shot coverage of each scene, then build it with one build_storyboard_from_breakdown \
         call (script set to that path, after panel {after}) and cover the beats it reports as uncovered. \
         Finish with estimate_storyboard_durations as a dry run and adjust action beats that need more \
         or less time than their words.",
        path.display()
    )
}

impl EditorView {
    /// Start the assistant on a breakdown of the script at `path`, landing
    /// after the active panel's scene or at the end.
    pub(crate) fn break_down_script(
        &mut self,
        path: &Path,
        at_end: bool,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if let Some(reason) = assistant_unavailable(cx) {
            return Err(reason);
        }
        let after = if at_end {
            self.editor.page_list().last().map(|m| m.id)
        } else {
            None
        }
        .unwrap_or(self.editor.active_page());
        self.start_turn(breakdown_request(path, after), cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn breakdown_requests_name_the_script_tools_and_placement() {
        let text = breakdown_request(Path::new("/scripts/storm.fountain"), 7);
        for needle in [
            "/scripts/storm.fountain",
            "read_storyboard_script",
            "build_storyboard_from_breakdown",
            "after panel 7",
            "estimate_storyboard_durations",
        ] {
            assert!(text.contains(needle), "{needle}");
        }
    }
}
