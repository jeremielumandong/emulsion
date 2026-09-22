//! Tracks whether a drawing's current revision was explicitly inspected.
//! Receiving an image establishes an opportunity to review, not artistic quality.

use emulsion_mcp::server::ToolResult;
use serde_json::Value;
use std::collections::HashSet;

const MAX_FOLLOWUPS: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Completion {
    Finish,
    Review,
    Unreviewed,
}

#[derive(Clone, Debug, Default)]
pub struct DrawingReview {
    drawing_changed: bool,
    inspected_revisions: HashSet<u64>,
    followups: u8,
    stopped: bool,
}

impl DrawingReview {
    /// Record the revision actually rendered, which may differ from the live
    /// document when a preview completes on a background thread.
    pub fn observe(
        &mut self,
        tool: &str,
        args: &Value,
        result: &ToolResult,
        revision: u64,
        changed: bool,
    ) {
        if result.is_error {
            return;
        }
        if changed
            && matches!(
                tool,
                "paint"
                    | "hatch"
                    | "draw_path"
                    | "draw_shape"
                    | "set_path"
                    | "combine_path"
                    | "resize_path"
                    | "align_path_components"
                    | "apply_shape_stroke_preset"
                    | "fill_selection"
                    | "add_text"
                    | "generate_image"
                    | "generative_fill"
            )
        {
            self.drawing_changed = true;
        }
        let full_view = match tool {
            "get_view" => args.get("node").is_none() && args.get("region").is_none(),
            "critique" => args.get("include_images").and_then(Value::as_bool) != Some(false),
            _ => false,
        };
        if full_view
            && result.content.iter().any(|block| {
                block["type"] == "image"
                    && block["mimeType"] == "image/png"
                    && block["data"].as_str().is_some_and(|data| !data.is_empty())
            })
        {
            self.inspected_revisions.insert(revision);
        }
    }

    pub fn stop(&mut self) {
        self.stopped = true;
    }

    pub fn completion(&mut self, revision: u64) -> Completion {
        if self.stopped || !self.drawing_changed || self.inspected_revisions.contains(&revision) {
            Completion::Finish
        } else if self.followups < MAX_FOLLOWUPS {
            self.followups += 1;
            Completion::Review
        } else {
            Completion::Unreviewed
        }
    }
}

pub fn review_request(brief: &str) -> String {
    format!(
        "The drawing changed after its last explicit full-canvas inspection. Continue the same \
         request with a final visual review, respecting the person's time/cost limits and skipped \
         changes. Original request:\n{brief}\n\n\
         Call get_view with no node or region to inspect the current full composition. Inspect a \
         detail region if needed for a face, hand, silhouette or perspective junction. Compare the \
         returned images with the original request and available references; preserve the intended \
         style. Describe the single most consequential visible mismatch with document coordinates, \
         if there is one. Make only a targeted correction within the remaining budget, then call \
         get_view again on the full canvas after the last change. Do not repaint the whole picture \
         or invent a flaw. If no correction is justified, finish. If a limit prevents review or \
         correction, state that limitation plainly. Numeric critique observations alone do not \
         establish anatomy, perspective, resemblance, or artistic quality."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::{Document, Editor};
    use emulsion_mcp::exec;
    use serde_json::json;

    fn call(
        review: &mut DrawingReview,
        editor: &mut Editor,
        name: &str,
        args: Value,
    ) -> ToolResult {
        let before = editor.revision;
        let result = exec::execute(editor, name, &args);
        review.observe(
            name,
            &args,
            &result,
            editor.revision,
            before != editor.revision,
        );
        result
    }

    fn drawing() -> (DrawingReview, Editor) {
        let mut review = DrawingReview::default();
        let mut editor = Editor::new(Document::new(80, 60), None);
        let result = call(
            &mut review,
            &mut editor,
            "draw_path",
            json!({
                "name": "Triangle", "d": "M 10 50 L 40 10 L 70 50 Z", "fill": "#334455", "stroke": "none"
            }),
        );
        assert!(!result.is_error, "{result:?}");
        (review, editor)
    }

    #[test]
    fn drawing_requires_full_review_of_the_latest_revision() {
        let (mut review, mut editor) = drawing();
        assert_eq!(review.completion(editor.revision), Completion::Review);
        call(
            &mut review,
            &mut editor,
            "get_view",
            json!({"region": [0, 0, 20, 20]}),
        );
        assert!(!review.inspected_revisions.contains(&editor.revision));
        call(&mut review, &mut editor, "get_view", json!({"node": 1}));
        assert!(!review.inspected_revisions.contains(&editor.revision));
        call(&mut review, &mut editor, "get_view", json!({}));
        assert_eq!(review.completion(editor.revision), Completion::Finish);
        // Even a later opacity change invalidates the old preview.
        call(
            &mut review,
            &mut editor,
            "set_opacity",
            json!({"node": 1, "opacity": 50}),
        );
        assert_eq!(review.completion(editor.revision), Completion::Review);
        call(
            &mut review,
            &mut editor,
            "critique",
            json!({"include_images": false}),
        );
        assert_eq!(review.completion(editor.revision), Completion::Unreviewed);
        call(&mut review, &mut editor, "critique", json!({}));
        assert_eq!(review.completion(editor.revision), Completion::Finish);
    }

    #[test]
    fn failed_and_stale_previews_do_not_satisfy_review() {
        let (mut review, mut editor) = drawing();
        let old_revision = editor.revision;
        let preview = exec::execute(&mut editor, "get_view", &json!({}));
        call(
            &mut review,
            &mut editor,
            "set_opacity",
            json!({"node": 1, "opacity": 40}),
        );
        review.observe("get_view", &json!({}), &preview, old_revision, false);
        assert_eq!(review.completion(editor.revision), Completion::Review);
        review.observe(
            "get_view",
            &json!({}),
            &ToolResult::error("failed"),
            editor.revision,
            false,
        );
        assert_eq!(review.completion(editor.revision), Completion::Review);
    }

    #[test]
    fn older_preview_finishing_late_does_not_erase_current_review() {
        let (mut review, mut editor) = drawing();
        let old_revision = editor.revision;
        let old_view = exec::execute(&mut editor, "get_view", &json!({}));
        call(
            &mut review,
            &mut editor,
            "set_opacity",
            json!({"node": 1, "opacity": 40}),
        );
        call(&mut review, &mut editor, "get_view", json!({}));
        review.observe("get_view", &json!({}), &old_view, old_revision, false);
        assert_eq!(review.completion(editor.revision), Completion::Finish);
        assert!(editor.undo());
        assert_eq!(editor.revision, old_revision);
        assert_eq!(review.completion(editor.revision), Completion::Finish);
    }

    #[test]
    fn generated_art_requires_review_too() {
        for tool in ["generate_image", "generative_fill"] {
            let mut review = DrawingReview::default();
            review.observe(
                tool,
                &json!({}),
                &ToolResult::text("Generated a layer"),
                2,
                true,
            );
            assert_eq!(review.completion(2), Completion::Review);
        }
    }

    #[test]
    fn shape_edits_require_review_but_preset_management_does_not() {
        for tool in [
            "draw_shape",
            "combine_path",
            "resize_path",
            "align_path_components",
            "apply_shape_stroke_preset",
        ] {
            let mut review = DrawingReview::default();
            review.observe(tool, &json!({}), &ToolResult::text("Edited"), 2, true);
            assert_eq!(review.completion(2), Completion::Review, "{tool}");

            for (result, changed) in [
                (ToolResult::text("No change"), false),
                (ToolResult::error("Locked"), true),
            ] {
                let mut review = DrawingReview::default();
                review.observe(tool, &json!({}), &result, 2, changed);
                assert_eq!(review.completion(2), Completion::Finish, "{tool}");
            }
        }
        for tool in ["list_shape_stroke_presets", "save_shape_stroke_preset"] {
            let mut review = DrawingReview::default();
            review.observe(tool, &json!({}), &ToolResult::text("Preset"), 2, true);
            assert_eq!(review.completion(2), Completion::Finish, "{tool}");
        }
    }

    #[test]
    fn review_is_bounded_and_cancellation_never_restarts_work() {
        let (mut review, editor) = drawing();
        assert_eq!(review.completion(editor.revision), Completion::Review);
        assert_eq!(review.completion(editor.revision), Completion::Review);
        for _ in 0..3 {
            assert_eq!(review.completion(editor.revision), Completion::Unreviewed);
        }
        review.stop();
        assert_eq!(review.completion(editor.revision), Completion::Finish);
    }

    #[test]
    fn read_only_and_failed_drawing_requests_do_not_trigger_followups() {
        let mut review = DrawingReview::default();
        let mut editor = Editor::new(Document::new(80, 60), None);
        call(&mut review, &mut editor, "describe_document", json!({}));
        let result = call(&mut review, &mut editor, "paint", json!({"node": 900}));
        assert!(result.is_error);
        assert_eq!(review.completion(editor.revision), Completion::Finish);
    }
}
