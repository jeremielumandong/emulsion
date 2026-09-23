//! Automatic corrections of the visible image, kept as editable adjustment layers.
use super::*;
use emulsion_raster::auto::{AutoCorrection, auto_correction};

impl EditorView {
    pub(crate) fn auto_correction_ready(&self) -> bool {
        self.effects_ready()
            && self.pending_edit_job.is_none()
            && !self.generate.busy
            && !self.tools.remove.running
            && !self.tools.quick_mask
            && !self.tools.mask_edit
            && !self.ai.job.as_ref().is_some_and(|job| !job.is_finished())
    }

    pub(crate) fn auto_correct(&mut self, mode: AutoCorrection, cx: &mut Context<Self>) {
        if !self.auto_correction_ready() {
            self.set_status("Finish the current edit and select layer content before applying an automatic correction.", false, cx);
            return;
        }
        let label = mode.label();
        let selection = self.editor.doc.selection.clone();
        if selection
            .as_ref()
            .is_some_and(|mask| emulsion_raster::select::bounds(mask).is_empty())
        {
            self.set_status(
                "The selection is empty. Select image content first.",
                false,
                cx,
            );
            return;
        }
        let source = self.composite_raster();
        let ticket = self.begin_edit_job();
        self.set_status(format!("Analyzing image for {label}..."), false, cx);
        cx.spawn(async move |this, cx| {
            let (adjustment, selection) = cx
                .background_spawn(async move {
                    let image = source.await;
                    let adjustment = auto_correction(&image, selection.as_deref(), mode);
                    (adjustment, selection)
                })
                .await;
            this.update(cx, |this, cx| {
                if !this.accept_edit_result(ticket, label, cx) {
                    return;
                }
                let mut node = Node::adjust(0, adjustment);
                node.name = label.into();
                node.mask = selection;
                // Analysis used the visible composite, so the correction belongs above it.
                if let Some(id) = this.execute(
                    Command::AddNode {
                        node: Box::new(node),
                        slot: Slot::TOP,
                    },
                    cx,
                ) {
                    this.set_layer_selection(vec![id], Some(id));
                    this.select_sidebar(SidebarTab::Properties, cx);
                    this.set_status(
                        format!(
                            "{label} added as an editable adjustment layer. Hide it to compare."
                        ),
                        false,
                        cx,
                    );
                }
            })
            .ok();
        })
        .detach();
    }
}
