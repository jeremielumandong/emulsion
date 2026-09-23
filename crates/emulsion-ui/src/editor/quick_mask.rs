//! Photoshop's Quick Mask: the selection shown as a red overlay on what is
//! not selected, edited with the paint tools (black masks, white selects,
//! the eraser selects). Each stroke is an undoable selection change; the
//! document's pixels are never touched.
use super::*;
use crate::viewport::View;
use emulsion_raster::Mask;
use rayon::prelude::*;
use std::cell::RefCell;

/// Screen pixels per overlay sample; the overlay is soft, so a coarse
/// grid keeps it cheap to rebuild while a stroke is being painted.
const STEP: f32 = 3.;
/// Photoshop's default Quick Mask opacity.
const OPACITY: f32 = 0.5;

/// The last overlay image and the selection and view it was drawn for.
#[derive(Default)]
pub(crate) struct QuickMaskCache(RefCell<Option<(u64, Arc<RenderImage>)>>);

impl EditorView {
    /// Enter or leave Quick Mask. Leaving keeps what was painted as the
    /// selection; a mask left entirely white means nothing is selected.
    pub fn toggle_quick_mask(&mut self, cx: &mut Context<Self>) {
        self.mask_view.layer = None;
        self.finish_tool_interaction(cx);
        self.tools.quick_mask = !self.tools.quick_mask;
        if self.tools.quick_mask {
            if !matches!(self.tool, Tool::Brush)
                || !matches!(
                    self.tools.paint,
                    PaintKind::Brush | PaintKind::Eraser | PaintKind::Smudge
                )
            {
                self.set_paint(PaintKind::Brush, cx);
            }
            self.set_status(
                "Quick Mask: paint black to mask (red), white or the eraser to select. Press Q again to finish.",
                false,
                cx,
            );
        } else {
            let whole = self
                .editor
                .doc
                .selection
                .as_ref()
                .is_some_and(|sel| sel.to_gray8().iter().all(|&v| v == 255));
            if whole {
                self.execute(Command::SetSelection { selection: None }, cx);
            }
            self.set_status(
                if self.editor.doc.selection.is_some() {
                    "Quick Mask finished: the unmasked area is selected."
                } else {
                    "Quick Mask finished: nothing was masked, so nothing is selected."
                },
                false,
                cx,
            );
        }
        cx.notify();
    }

    /// The mask a Quick Mask stroke paints into: the selection, or all
    /// selected when there is none.
    pub(super) fn quick_mask_target(&self) -> Arc<Mask> {
        self.editor
            .doc
            .selection
            .clone()
            .unwrap_or_else(|| Arc::new(Mask::white(self.editor.doc.width, self.editor.doc.height)))
    }

    /// Why the current tool cannot paint in Quick Mask, if it cannot.
    pub(super) fn quick_mask_blocks(&self) -> Option<&'static str> {
        if !self.tools.quick_mask {
            return None;
        }
        match self.tool {
            Tool::Brush
                if matches!(
                    self.tools.paint,
                    PaintKind::Bucket | PaintKind::Gradient | PaintKind::Liquify
                ) =>
            {
                Some("In Quick Mask, paint with the Brush, Eraser or Smudge.")
            }
            Tool::Heal | Tool::Clone => {
                Some("In Quick Mask, paint with the Brush, Eraser or Smudge.")
            }
            _ => None,
        }
    }
}

impl EditorView {
    /// Photoshop's Quick Mask button, under the colour swatches.
    pub(super) fn quick_mask_button(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        use gpui_kit::component::{
            Sizable,
            button::{Button, ButtonVariants},
        };
        let on = self.tools.quick_mask;
        Button::new("quick-mask-toggle")
            .ghost()
            .small()
            .accessibility_label(if on {
                "Exit Quick Mask mode"
            } else {
                "Edit in Quick Mask mode"
            })
            .tooltip(if on {
                "Exit Quick Mask (Q): turn the painted mask into the selection"
            } else {
                "Edit in Quick Mask mode (Q): paint the selection, with the masked area in red"
            })
            .when(on, |b| b.bg(p.soft_bg).border_1().border_color(p.accent))
            .child(
                super::rail::tool_icon("emulsion-mask")
                    .size_4()
                    .text_color(if on { p.accent } else { p.ink }),
            )
            .on_click(cx.listener(|this, _, window, cx| {
                this.toggle_quick_mask(cx);
                window.focus(&this.canvas_focus, cx);
            }))
            .into_any_element()
    }
}

/// Paint the red Quick Mask overlay over the canvas, resampled through the
/// view so it follows zoom, pan and rotation.
pub(crate) fn paint(
    selection: &Mask,
    view: &View,
    bounds: Bounds<Pixels>,
    cache: &QuickMaskCache,
    window: &mut Window,
) {
    paint_mask(selection, view, bounds, cache, true, window);
}

pub(crate) fn paint_coverage(
    selection: &Mask,
    view: &View,
    bounds: Bounds<Pixels>,
    cache: &QuickMaskCache,
    window: &mut Window,
) {
    paint_mask(selection, view, bounds, cache, false, window);
}

fn paint_mask(
    selection: &Mask,
    view: &View,
    bounds: Bounds<Pixels>,
    cache: &QuickMaskCache,
    invert: bool,
    window: &mut Window,
) {
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        selection.content_id().hash(&mut h);
        invert.hash(&mut h);
        for v in [view.zoom, view.center.0, view.center.1, view.rotation] {
            v.to_bits().hash(&mut h);
        }
        for v in [
            bounds.origin.x,
            bounds.origin.y,
            bounds.size.width,
            bounds.size.height,
        ] {
            f32::from(v).to_bits().hash(&mut h);
        }
        h.finish()
    };
    let cached = cache
        .0
        .borrow()
        .as_ref()
        .filter(|(k, _)| *k == key)
        .map(|(_, image)| image.clone());
    let image = match cached {
        Some(image) => image,
        None => {
            let (bw, bh) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            let (dw, dh) = (
                (bw / STEP).ceil().max(1.) as u32,
                (bh / STEP).ceil().max(1.) as u32,
            );
            let (w, h) = (selection.width() as f64, selection.height() as f64);
            let (ox, oy) = (
                f32::from(bounds.origin.x) as f64,
                f32::from(bounds.origin.y) as f64,
            );
            let mut buf = vec![0u8; (dw * dh * 4) as usize];
            buf.par_chunks_mut((dw * 4) as usize)
                .enumerate()
                .for_each(|(row, line)| {
                    for col in 0..dw as usize {
                        let sx = ox + (col as f64 + 0.5) * STEP as f64;
                        let sy = oy + (row as f64 + 0.5) * STEP as f64;
                        let (x, y) = view.screen_to_doc((sx, sy), &bounds);
                        if x < 0. || y < 0. || x >= w || y >= h {
                            continue;
                        }
                        let coverage = selection.get(x as u32, y as u32);
                        let masked = if invert { 255 - coverage } else { coverage };
                        let a = (masked as f32 * OPACITY).round() as u8;
                        // Premultiplied BGRA red.
                        line[col * 4..col * 4 + 4].copy_from_slice(&[0, 0, a, a]);
                    }
                });
            let image = Arc::new(crate::viewport::bgra_image(dw, dh, buf));
            if let Some((_, old)) = cache.0.borrow_mut().replace((key, image.clone())) {
                let _ = window.drop_image(old);
            }
            image
        }
    };
    let _ = window.paint_image(bounds, bounds, Corners::default(), image, 0, false);
}
