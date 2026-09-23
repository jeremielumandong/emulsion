//! Saved quick settings are scoped to a stable brush ID and painting tool.
use super::*;
use emulsion_io::brush_library::{BrushMark, Catalog};
use emulsion_raster::paint::Brush;

pub(super) fn slot_key(slot: Option<tools::BrushSlot>) -> Option<&'static str> {
    use tools::BrushSlot;
    match slot? {
        BrushSlot::Paint(PaintKind::Brush) => Some("paint"),
        BrushSlot::Paint(PaintKind::Smudge) => Some("smudge"),
        BrushSlot::Paint(PaintKind::Eraser) => Some("erase"),
        BrushSlot::Heal => Some("heal"),
        BrushSlot::Clone => Some("clone"),
        BrushSlot::Mask => Some("mask"),
        _ => None,
    }
}

fn apply_mark(brush: &mut Brush, mark: BrushMark) {
    // Recalling a quick setting must preserve source images, dynamics, color,
    // and any newer Studio edits to the selected definition.
    brush.size = mark.size;
    brush.opacity = mark.opacity;
    *brush = brush.sanitized();
}

impl EditorView {
    pub(super) fn active_memory_key(&self) -> Option<&'static str> {
        slot_key(tools::BrushSlot::of(self.tool, self.tools.paint))
    }

    pub(super) fn active_brush_marks(&self, cx: &App) -> [Option<BrushMark>; 4] {
        let Some(key) = self.active_memory_key() else {
            return [None; 4];
        };
        let Some(id) = self.presets.current_id.as_deref() else {
            return [None; 4];
        };
        self.presets
            .library
            .as_ref()
            .and_then(|library| library.read(cx).catalog.tool_memory(key, id))
            .map(|memory| memory.marks)
            .unwrap_or([None; 4])
    }

    fn commit_brush_memory(&mut self, draft: Catalog, cx: &mut Context<Self>) -> bool {
        let library = presets::shared_library(cx);
        match library.update(cx, |state, cx| state.commit(draft, cx)) {
            Ok(()) => {
                cx.notify();
                true
            }
            Err(error) => {
                self.set_status(format!("Couldn't save brush memories: {error}"), true, cx);
                false
            }
        }
    }

    pub(super) fn remember_active_brush(&mut self, cx: &mut Context<Self>) {
        self.remember_brush_for_slot(tools::BrushSlot::of(self.tool, self.tools.paint), cx);
    }

    pub(super) fn remember_brush_for_slot(
        &mut self,
        slot: Option<tools::BrushSlot>,
        cx: &mut Context<Self>,
    ) {
        if self.presets.applying_committed_brush {
            return;
        }
        let Some(key) = slot_key(slot) else { return };
        let Some(id) = self.presets.current_id.clone() else {
            return;
        };
        let library = presets::shared_library(cx);
        let mut draft = library.read(cx).catalog.clone();
        let brush = self.tools.brush.sanitized();
        if draft
            .tool_memory(key, &id)
            .is_some_and(|memory| memory.brush == brush)
        {
            return;
        }
        if let Err(error) = draft.remember_tool(key, &id, brush) {
            self.set_status(error.to_string(), true, cx);
            return;
        }
        self.commit_brush_memory(draft, cx);
    }

    pub(super) fn restore_active_brush_memory(&mut self, cx: &mut Context<Self>) {
        self.restore_brush_slot_memory(None, cx);
    }

    pub(super) fn restore_brush_slot_memory(
        &mut self,
        previous: Option<Brush>,
        cx: &mut Context<Self>,
    ) {
        let Some(key) = self.active_memory_key() else {
            return;
        };
        let Some(id) = self.presets.current_id.as_deref() else {
            return;
        };
        let library = presets::shared_library(cx);
        if let Some(memory) = library.read(cx).catalog.tool_memory(key, id) {
            let mark = BrushMark {
                size: if previous.is_some_and(|old| memory.brush.size == old.size) {
                    self.tools.brush.size
                } else {
                    memory.brush.size
                },
                opacity: if previous.is_some_and(|old| memory.brush.opacity == old.opacity) {
                    self.tools.brush.opacity
                } else {
                    memory.brush.opacity
                },
            };
            apply_mark(&mut self.tools.brush, mark);
        }
    }

    pub(super) fn save_brush_mark(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(key) = self.active_memory_key() else {
            return;
        };
        let Some(id) = self.presets.current_id.clone() else {
            return;
        };
        let library = presets::shared_library(cx);
        let mut draft = library.read(cx).catalog.clone();
        if let Err(error) = draft.save_mark(key, &id, index, self.tools.brush) {
            self.set_status(error.to_string(), true, cx);
            return;
        }
        if self.commit_brush_memory(draft, cx) {
            self.set_status(format!("Saved brush memory {}", index + 1), false, cx);
        }
    }

    pub(super) fn recall_brush_mark(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(mark) = self.active_brush_marks(cx).get(index).copied().flatten() else {
            return;
        };
        self.finish_tool_interaction(cx);
        apply_mark(&mut self.tools.brush, mark);
        self.remember_active_brush(cx);
        cx.notify();
    }

    pub(super) fn remove_brush_mark(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(key) = self.active_memory_key() else {
            return;
        };
        let Some(id) = self.presets.current_id.clone() else {
            return;
        };
        let library = presets::shared_library(cx);
        let mut draft = library.read(cx).catalog.clone();
        if let Err(error) = draft.remove_mark(key, &id, index) {
            self.set_status(error.to_string(), true, cx);
            return;
        }
        self.commit_brush_memory(draft, cx);
    }

    pub(super) fn transfer_brush_to(&mut self, target: PaintKind, cx: &mut Context<Self>) {
        if !matches!(
            target,
            PaintKind::Brush | PaintKind::Smudge | PaintKind::Eraser
        ) {
            return;
        }
        self.finish_tool_interaction(cx);
        let brush = self.tools.brush;
        let id = self.presets.current_id.clone();
        let name = self.presets.current.clone();
        let definition = self.presets.definition;
        self.set_paint(target, cx);
        self.tools.brush = brush;
        self.presets.current_id = id;
        self.presets.current = name;
        self.presets.definition = definition;
        self.remember_active_brush(cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_mark, slot_key};
    use crate::editor::{PaintKind, tools};
    use emulsion_io::brush_library::BrushMark;
    use emulsion_raster::paint::Brush;

    #[test]
    fn memory_recall_preserves_brush_sources_and_dynamics() {
        let mut brush = Brush {
            tip: 42,
            grain_tex: 17,
            scatter: 0.6,
            hardness: 0.7,
            ..Brush::default()
        };
        let before = brush;
        apply_mark(
            &mut brush,
            BrushMark {
                size: 88.0,
                opacity: 0.3,
            },
        );
        assert_eq!(
            brush,
            Brush {
                size: 88.0,
                opacity: 0.3,
                ..before
            }
        );
    }

    #[test]
    fn painting_tool_memories_have_distinct_stable_keys() {
        use tools::BrushSlot;
        assert_eq!(
            slot_key(Some(BrushSlot::Paint(PaintKind::Brush))),
            Some("paint")
        );
        assert_eq!(
            slot_key(Some(BrushSlot::Paint(PaintKind::Smudge))),
            Some("smudge")
        );
        assert_eq!(
            slot_key(Some(BrushSlot::Paint(PaintKind::Eraser))),
            Some("erase")
        );
        assert_eq!(slot_key(Some(BrushSlot::Paint(PaintKind::Bucket))), None);
        assert_eq!(slot_key(None), None);
    }
}
