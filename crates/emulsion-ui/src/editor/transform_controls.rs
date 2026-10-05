//! Pointer-facing transform controls. Mode changes never edit the document.
use super::transform::Handle;
use super::*;

pub(super) const CLICK_SLOP_PX: f64 = 4.;
const ROTATION_OFFSET_PX: f64 = 24.;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TransformControlMode {
    #[default]
    Resize,
    Rotate,
}

#[derive(Clone, Copy)]
pub(crate) struct BoxControls {
    pub mode: TransformControlMode,
    pub side_handle: bool,
    pub obstacle: Option<Bounds<Pixels>>,
}

/// The same screen-space geometry is used for painting and hit testing. Try
/// the other edges if the usual top handle would fall behind the chrome.
pub(super) fn rotation_handle(
    quad: [(f64, f64); 4],
    bounds: Bounds<Pixels>,
) -> Option<((f64, f64), (f64, f64))> {
    find_rotation_handle(quad, bounds, false, None)
}

/// Design's floating All tools toolbar occupies a strip above or below the
/// artwork. Keep the complete pointer target beside its vertical extent.
pub(super) fn side_rotation_handle(
    quad: [(f64, f64); 4],
    bounds: Bounds<Pixels>,
) -> Option<((f64, f64), (f64, f64))> {
    find_rotation_handle(quad, bounds, true, None)
}

pub(super) fn rotation_handle_with_obstacle(
    quad: [(f64, f64); 4],
    bounds: Bounds<Pixels>,
    side: bool,
    obstacle: Option<Bounds<Pixels>>,
) -> Option<((f64, f64), (f64, f64))> {
    if obstacle.is_some() {
        find_rotation_handle(quad, bounds, side, obstacle)
    } else if side {
        side_rotation_handle(quad, bounds)
    } else {
        rotation_handle(quad, bounds)
    }
}

fn find_rotation_handle(
    quad: [(f64, f64); 4],
    bounds: Bounds<Pixels>,
    avoid_top: bool,
    obstacle: Option<Bounds<Pixels>>,
) -> Option<((f64, f64), (f64, f64))> {
    let center = ((quad[0].0 + quad[2].0) / 2., (quad[0].1 + quad[2].1) / 2.);
    let artwork_top = quad.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let artwork_bottom = quad.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    (0..4).find_map(|i| {
        let a = quad[i];
        let b = quad[(i + 1) % 4];
        let mid = ((a.0 + b.0) / 2., (a.1 + b.1) / 2.);
        let dx = mid.0 - center.0;
        let dy = mid.1 - center.1;
        let length = dx.hypot(dy);
        if length < 1e-6 {
            return None;
        }
        let handle = (
            mid.0 + dx / length * ROTATION_OFFSET_PX,
            mid.1 + dy / length * ROTATION_OFFSET_PX,
        );
        let inset = 9.;
        // Keep the complete hit target clear of ruler controls. Reserve the
        // same space when rulers are hidden so toggling them cannot jump it.
        let ruler = super::snap::RULER_PX as f64;
        let blocked = obstacle.is_some_and(|b| {
            handle.0 >= f32::from(b.left()) as f64 - inset
                && handle.0 <= f32::from(b.right()) as f64 + inset
                && handle.1 >= f32::from(b.top()) as f64 - inset
                && handle.1 <= f32::from(b.bottom()) as f64 + inset
        });
        (handle.0 >= f32::from(bounds.left()) as f64 + inset + ruler
            && handle.0 <= f32::from(bounds.right()) as f64 - inset
            && handle.1 >= f32::from(bounds.top()) as f64 + inset + ruler
            && handle.1 <= f32::from(bounds.bottom()) as f64 - inset
            && !blocked
            && (!avoid_top
                || (handle.1 >= artwork_top + inset && handle.1 <= artwork_bottom - inset)))
            .then_some((mid, handle))
    })
}

impl EditorView {
    /// Photo and Paint edit a standalone document; Design uses the same Move
    /// geometry. Diagram and Storyboard keep their own pointer conventions.
    pub(crate) fn has_transform_controls(&self) -> bool {
        !self.library_only && (self.editor.kind().is_none() || self.is_design())
    }

    pub(super) fn side_transform_handle(&self) -> bool {
        self.is_design() && self.design_full_tools()
    }

    pub(super) fn rotation_handle_for_frame(
        &self,
        quad: [(f64, f64); 4],
        bounds: Bounds<Pixels>,
    ) -> Option<((f64, f64), (f64, f64))> {
        rotation_handle_with_obstacle(
            quad,
            bounds,
            self.side_transform_handle(),
            self.rotation_control_obstacle(),
        )
    }

    pub(super) fn rotation_control_obstacle(&self) -> Option<Bounds<Pixels>> {
        (self.side_transform_handle()
            && self.drag.is_none()
            && !self.frame_crop_active()
            && !self.previewing()
            && self.selected_layer_roots().len() == 1)
            .then(|| self.transform_toolbar_bounds.get())
            .flatten()
    }

    pub(super) fn transform_screen_quad(&self) -> Option<[(f64, f64); 4]> {
        let bounds = self.canvas_bounds()?;
        Some(
            self.transform_box()?
                .map(|p| self.view.doc_to_screen(p, &bounds)),
        )
    }

    /// Test the actual rotated frame, not its axis-aligned bounding box.
    pub(super) fn transform_contains(&self, pos: Point<Pixels>) -> bool {
        let Some(quad) = self.transform_screen_quad() else {
            return false;
        };
        let p = (f32::from(pos.x) as f64, f32::from(pos.y) as f64);
        let crosses = (0..4)
            .map(|i| {
                let (a, b) = (quad[i], quad[(i + 1) % 4]);
                (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
            })
            .collect::<Vec<_>>();
        crosses.iter().all(|d| *d >= 0.) || crosses.iter().all(|d| *d <= 0.)
    }

    pub(super) fn arm_transform_click(&mut self, event: &MouseDownEvent, was_selected: bool) {
        let plain = !event.modifiers.shift
            && !event.modifiers.control
            && !event.modifiers.platform
            && !event.modifiers.alt;
        let eligible = self.has_transform_controls()
            && was_selected
            && event.click_count == 1
            && plain
            && self.transform_contains(event.position);
        if let Some(Drag::Move(gesture)) = &mut self.drag {
            gesture.toggle_controls = eligible;
        }
    }

    pub(super) fn toggle_transform_controls(&mut self, cx: &mut Context<Self>) {
        self.transform_control_mode = match self.transform_control_mode {
            TransformControlMode::Resize => TransformControlMode::Rotate,
            TransformControlMode::Rotate => TransformControlMode::Resize,
        };
        self.set_status(
            t!(match self.transform_control_mode {
                TransformControlMode::Resize => "editor.transform.resize_controls_hint",
                TransformControlMode::Rotate => "editor.transform.rotation_controls_hint",
            }),
            false,
            cx,
        );
    }

    pub(crate) fn transform_cursor(&self, position: Point<Pixels>) -> CursorStyle {
        if self.warp.is_some() {
            return CursorStyle::Arrow;
        }
        if matches!(self.drag, Some(Drag::Move(_))) {
            return CursorStyle::ClosedHand;
        }
        let handle = match self.drag {
            Some(Drag::Transform(grab)) => Some(grab.handle),
            Some(_) => None,
            None => self
                .handle_hit(position)
                .filter(|handle| !self.is_diagram() || matches!(handle, Handle::Corner(_))),
        };
        let Some(handle) = handle else {
            return CursorStyle::Arrow;
        };
        if handle == Handle::Rotate {
            return CursorStyle::Crosshair;
        }
        let Some(quad) = self.transform_screen_quad() else {
            return CursorStyle::Arrow;
        };
        let (a, b) = match handle {
            Handle::Corner(i) => (quad[i], quad[(i + 2) % 4]),
            Handle::Edge(i) => (quad[(i + 1) % 4], quad[(i + 2) % 4]),
            Handle::Rotate => unreachable!(),
        };
        let sector =
            (((b.1 - a.1).atan2(b.0 - a.0).to_degrees() / 45.).round() as i32).rem_euclid(4);
        match sector {
            0 => CursorStyle::ResizeLeftRight,
            1 => CursorStyle::ResizeUpLeftDownRight,
            2 => CursorStyle::ResizeUpDown,
            _ => CursorStyle::ResizeUpRightDownLeft,
        }
    }

    /// GPUI has no native rotation cursor. Show the operation beside its
    /// precise crosshair, just as Zoom shows a platform-independent magnifier.
    pub(super) fn rotation_cursor(&self, p: &Palette, window: &Window) -> Option<AnyElement> {
        if self.tool != Tool::Move
            || self.space_held
            || self.warp.is_some()
            || self.is_diagram()
            || self.frame_crop_active()
        {
            return None;
        }
        let bounds = self.canvas_bounds()?;
        let position = window.mouse_position();
        if !bounds.contains(&position) {
            return None;
        }
        let rotates = match self.drag {
            Some(Drag::Transform(grab)) => grab.handle == Handle::Rotate,
            Some(_) => false,
            None => self.handle_hit(position) == Some(Handle::Rotate),
        };
        if !rotates {
            return None;
        }
        let icon = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2"><path d="M19 10a7 7 0 1 0-1 7M19 4v6h-6"/></svg>"#.as_slice();
        Some(
            div()
                .id("transform-rotation-cursor")
                .absolute()
                .left(
                    (position.x - bounds.origin.x + px(12.))
                        .min((bounds.size.width - px(28.)).max(px(0.))),
                )
                .top(
                    (position.y - bounds.origin.y + px(12.))
                        .min((bounds.size.height - px(28.)).max(px(0.))),
                )
                .size(px(28.))
                .p(px(2.))
                .bg(p.panel)
                .border_1()
                .border_color(p.ink)
                .child(svg().data(icon).size_full().text_color(p.ink))
                .test_support()
                .into_any_element(),
        )
    }

    pub(super) fn transform_mode_controls(
        &self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        if !self.has_transform_controls()
            || self.transformable().is_none()
            || self.warp.is_some()
            || self.frame_crop_active()
        {
            return Vec::new();
        }
        let buttons = [
            (
                "transform-mode-resize",
                "editor.transform.resize_controls",
                TransformControlMode::Resize,
            ),
            (
                "transform-mode-rotate",
                "editor.transform.rotation_controls",
                TransformControlMode::Rotate,
            ),
        ]
        .into_iter()
        .map(|(id, label, mode)| {
            chip(id, t!(label), self.transform_control_mode == mode, p)
                .test_support()
                .aria_toggled(if self.transform_control_mode == mode {
                    gpui_kit::accesskit::Toggled::True
                } else {
                    gpui_kit::accesskit::Toggled::False
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    if this.drag.is_none() {
                        this.transform_control_mode = mode;
                        // Keep Escape in the containing More popover until
                        // it closes; inline controls return to the canvas.
                        window.focus(&this.photo_options_menu_focus(cx), cx);
                        cx.notify();
                    }
                }))
                .into_any_element()
        })
        .collect::<Vec<_>>();
        vec![
            div()
                .id("transform-mode-controls")
                .flex()
                .flex_none()
                .gap_1()
                .children(buttons)
                .into_any_element(),
        ]
    }
}
