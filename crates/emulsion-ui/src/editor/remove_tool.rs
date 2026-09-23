//! Brush-based object removal sampled from the visible document.
use super::tools::ToolDrag;
use super::*;
use emulsion_raster::paint::{Brush, Ink, Stroke};
use emulsion_raster::select::{self, Combine};
use emulsion_raster::{IRect, Mask, fill};

pub(crate) struct RemoveState {
    pub enabled: bool,
    pub after_stroke: bool,
    pub running: bool,
    pub cache: std::rc::Rc<super::quick_mask::QuickMaskCache>,
    pending: Option<Mask>,
    epoch: Option<u64>,
    request: u64,
}
impl Default for RemoveState {
    fn default() -> Self {
        Self {
            enabled: false,
            after_stroke: true,
            running: false,
            cache: Default::default(),
            pending: None,
            epoch: None,
            request: 0,
        }
    }
}
impl EditorView {
    pub(crate) fn set_remove_mode(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.cancel_remove(cx);
        self.set_tool(Tool::Heal, cx);
        self.tools.remove.enabled = enabled;
        if enabled {
            self.set_mask_edit(false, cx);
        }
        cx.notify();
    }
    pub(crate) fn remove_coverage(&self) -> Option<Mask> {
        if self.tools.remove.epoch != Some(self.operation_epoch) {
            return None;
        }
        if let Some(Drag::Tool(ToolDrag::Remove { stroke, .. })) = &self.drag {
            Some(select::combine(
                self.tools.remove.pending.as_ref(),
                &stroke.coverage(),
                Combine::Add,
            ))
        } else {
            self.tools.remove.pending.clone()
        }
    }
    pub(crate) fn remove_pending(&self) -> bool {
        self.tools.remove.epoch == Some(self.operation_epoch) && self.tools.remove.pending.is_some()
    }
    pub(crate) fn cancel_remove(&mut self, cx: &mut Context<Self>) -> bool {
        let state = &mut self.tools.remove;
        let had = state.pending.take().is_some() || state.running;
        state.running = false;
        state.epoch = None;
        state.request = state.request.wrapping_add(1);
        if had {
            cx.notify();
        }
        had
    }
    pub(crate) fn start_remove(&mut self, point: (f64, f64), cx: &mut Context<Self>) {
        if !self.layer_menu_ready()
            || self.generate.busy
            || self.pending_edit_job.is_some()
            || self.tools.remove.running
        {
            return;
        }
        if self.tools.quick_mask || self.tools.mask_edit {
            self.set_status("Select layer content before removing objects.", false, cx);
            return;
        }
        if self
            .tools
            .remove
            .epoch
            .is_some_and(|epoch| epoch != self.operation_epoch)
        {
            self.cancel_remove(cx);
        }
        self.tools.remove.epoch = Some(self.operation_epoch);
        let base = Arc::new(Raster::empty(
            self.editor.doc.width,
            self.editor.doc.height,
            [0; 4],
        ));
        let brush = Brush {
            size: self.tools.brush.size,
            hardness: 1.0,
            ..Default::default()
        };
        let clip = self
            .editor
            .doc
            .selection
            .clone()
            .map(|mask| tools::local_clip(mask, glam::DAffine2::IDENTITY));
        let mut stroke = Stroke::new(base, brush, Ink::Color([1.0; 4]), clip);
        stroke.point(point.0 as f32, point.1 as f32);
        self.drag = Some(Drag::Tool(ToolDrag::Remove {
            stroke: Box::new(stroke),
        }));
        cx.notify();
    }
    pub(crate) fn finish_remove_stroke(&mut self, stroke: Stroke, cx: &mut Context<Self>) {
        let coverage = stroke.coverage();
        if select::bounds(&coverage).is_empty() {
            return;
        }
        self.tools.remove.pending = Some(select::combine(
            self.tools.remove.pending.as_ref(),
            &coverage,
            Combine::Add,
        ));
        if self.tools.remove.after_stroke {
            self.apply_remove(cx);
        }
        cx.notify();
    }
    pub(crate) fn apply_remove(&mut self, cx: &mut Context<Self>) {
        if !self.layer_menu_ready()
            || self.generate.busy
            || self.pending_edit_job.is_some()
            || self.tools.remove.running
        {
            return;
        }
        if self.tools.remove.epoch != Some(self.operation_epoch) {
            self.cancel_remove(cx);
            return;
        }
        let Some(hole) = self.tools.remove.pending.clone() else {
            return;
        };
        let epoch = self.operation_epoch;
        let request = self.tools.remove.request;
        let composite = self.composite_raster();
        let slot = Slot::TOP;
        self.tools.remove.running = true;
        self.set_status("Removing painted area…", false, cx);
        cx.spawn(async move |this, cx| {
            let repair = cx
                .background_spawn(async move {
                    let base = composite.await;
                    removal_patch(&base, &hole)
                })
                .await;
            this.update(cx, |this, cx| {
                if this.tools.remove.request != request {
                    return;
                }
                this.tools.remove.running = false;
                if this.operation_epoch != epoch || this.editor.in_transaction() {
                    this.cancel_remove(cx);
                    this.set_status(
                        "Removal cancelled because the document changed. Paint the area again.",
                        false,
                        cx,
                    );
                    return;
                }
                this.cancel_remove(cx);
                if let Some(repair) = repair {
                    let node =
                        Node::raster(0, "Object removal", Arc::new(repair), Placement::default());
                    if let Some(id) = this.execute(
                        Command::AddNode {
                            node: Box::new(node),
                            slot,
                        },
                        cx,
                    ) {
                        this.set_layer_selection(vec![id], Some(id));
                    }
                    this.set_status("Removed painted area on a new layer.", false, cx);
                }
            })
            .ok();
        })
        .detach();
    }
}

fn removal_patch(base: &Raster, hole: &Mask) -> Option<Raster> {
    let bounds = select::bounds(hole);
    if bounds.is_empty() {
        return None;
    }
    let margin = 64;
    let region = IRect::new(
        bounds.x - margin,
        bounds.y - margin,
        bounds.w + 2 * margin,
        bounds.h + 2 * margin,
    )
    .intersect(&base.bounds());
    let source: Vec<_> = base
        .read_rect(region)
        .into_iter()
        .map(color::px_to_f)
        .collect();
    let coverage: Vec<_> = hole
        .read_rect(region)
        .into_iter()
        .map(|v| v as f32 / 255.0)
        .collect();
    let repaired = fill::content_aware(
        &source,
        &coverage,
        region.w as usize,
        region.h as usize,
        0x4EA1,
    );
    let pixels: Vec<_> = repaired
        .into_iter()
        .zip(&source)
        .zip(coverage)
        .map(|((pixel, prior), alpha)| {
            color::f_to_px([0, 1, 2, 3].map(|i| (pixel[i] - prior[i] * (1.0 - alpha)).max(0.0)))
        })
        .collect();
    Some(Raster::empty(base.width(), base.height(), [0; 4]).write_rect(region, &pixels))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[core::prelude::v1::test]
    fn removal_patch_preserves_source_and_only_contains_painted_area() {
        let base = Raster::from_fn(40, 40, [0; 4], |x, y| {
            if (18..22).contains(&x) && (18..22).contains(&y) {
                [0, 0, 0, 65535]
            } else {
                [65535; 4]
            }
        });
        let hole = Mask::from_fn(40, 40, 0, |x, y| {
            if (17..23).contains(&x) && (17..23).contains(&y) {
                255
            } else {
                0
            }
        });
        let repair = removal_patch(&base, &hole).unwrap();
        assert_eq!(base.get(20, 20), [0, 0, 0, 65535]);
        assert_eq!(repair.get(0, 0), [0; 4]);
        assert!(repair.get(20, 20)[0] > 60000);
        assert_eq!(repair.get(20, 20)[3], 65535);
    }
}
