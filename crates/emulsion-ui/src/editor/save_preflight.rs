//! Native Save snapshots only settled document state. The sole implicit finish
//! here is the currently captured synchronous paint stroke, including CPU mask
//! samples not yet published by the frame loop. Other edit owners stay intact.
use super::*;

impl EditorView {
    pub(crate) fn prepare_native_save(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.photo_transform_ready(cx) {
            return false;
        }
        if self.raw.is_pending() {
            self.set_status(t!("shell.raw_pending_save"), false, cx);
            return false;
        }
        if self.editor.in_preview()
            || self.frame_crop_active()
            || self.responsive_preview_active()
            || self.warp.is_some()
            || self.assistant.running
            || self.pages_ui.export_pending
            || self.smart.has_pending()
            || self.pending_edit_job == Some(self.edit_ticket())
            || self.tools.photo_masks.edit.is_some()
            || self.tools.pen.building.is_some()
            || !self.tools.polygon.is_empty()
            || self.tools.crop.is_some()
        {
            self.set_status(t!("shell.finish_before_save"), false, cx);
            return false;
        }
        if matches!(
            self.drag,
            Some(Drag::Tool(tools::ToolDrag::Stroke { heal: false, .. }))
        ) {
            let Some(Drag::Tool(stroke)) = self.drag.take() else {
                unreachable!()
            };
            // tool_up catches up the stabilizer, publishes pending CPU samples
            // or GPU replay, then closes this stroke's own history transaction.
            self.tool_up(stroke, cx);
        }
        if self.drag.is_some() || self.editor.in_transaction() {
            self.set_status(t!("shell.finish_before_save"), false, cx);
            return false;
        }
        true
    }
}
