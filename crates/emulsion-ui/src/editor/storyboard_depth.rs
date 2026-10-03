//! The panel inspector's 3D section: the panel's Shot Generator set (with
//! a button to open it) and layer depth (L6, C6, V6, C12): each layer's
//! distance behind the panel plane for parallax under scene camera moves,
//! a top and a side view of the layers in depth with the camera's field of
//! view (projected with the same camera the parallax uses), and making the
//! selected layer follow an object of the panel's Shot Generator set.
use super::*;
use emulsion_core::project::PageId;
use emulsion_core::storyboard_shot::{DEPTH_RANGE, parallax_camera};
use gpui_kit::component::{
    Disableable, Sizable,
    button::{Button, ButtonVariants},
};

/// Depth presets: in front of the panel, the panel, middle, far, sky.
const PRESETS: [(&str, f64); 5] = [
    ("Near", -0.5),
    ("Panel", 0.),
    ("Mid", 1.),
    ("Far", 4.),
    ("Sky", 30.),
];

impl EditorView {
    fn set_depth(&mut self, panel: PageId, node: NodeId, depth: f64, cx: &mut Context<Self>) {
        let depth = depth.clamp(*DEPTH_RANGE.start(), *DEPTH_RANGE.end());
        let depth = (depth * 100.).round() / 100.;
        self.edit_board(|b| b.set_layer_depth(panel, node, depth), cx);
    }

    /// The panel inspector's Layer depth section.
    pub(super) fn layer_depth_section(
        &self,
        panel: PageId,
        locked: bool,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(board) = self.editor.storyboard() else {
            return div().into_any_element();
        };
        let depths = board
            .panels
            .get(&panel)
            .map(|p| p.depth.clone())
            .unwrap_or_default();
        let doc = &self.editor.doc;
        let mut root = div()
            .id("storyboard-layer-depth")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(label("3D set and layer depth", p))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(div().flex_1().child(mono(
                        match self.editor.panel_shot(panel) {
                            Some(shot) => format!(
                                "3D set: {} objects{}",
                                shot.set.objects.len(),
                                if shot.layer.is_some() { " · reference layer" } else { "" }
                            ),
                            None => "No 3D set yet".into(),
                        },
                        10.,
                        p.muted,
                    )))
                    .child(
                        Button::new("storyboard-open-shot-generator")
                            .label("Shot Generator")
                            .tooltip("Build this panel's shot in 3D (Ctrl+Alt+Shift+G)")
                            .xsmall()
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_shot_generator(cx))),
                    ),
            )
            .child(mono(
                "Far layers move less when the scene camera pans or zooms (0 = the panel, 1 = twice as far).",
                9.5,
                p.muted,
            ));
        let roots: Vec<NodeId> = doc.children(None).into_iter().rev().collect();
        for (i, id) in roots.iter().copied().enumerate() {
            let Some(node) = doc.node(id) else {
                continue;
            };
            let depth = depths.get(&id).copied().unwrap_or(0.);
            let step = if depth.abs() < 2. { 0.25 } else { 1. };
            let mut presets = div().flex().flex_wrap().gap(px(3.));
            for (j, (name, value)) in PRESETS.into_iter().enumerate() {
                presets = presets.child(
                    chip(
                        ("storyboard-depth-preset", i * 10 + j),
                        name,
                        depth == value,
                        p,
                    )
                    .test_support()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !locked {
                            this.set_depth(panel, id, value, cx)
                        }
                    })),
                );
            }
            root = root.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                div()
                                    .flex_1()
                                    .truncate()
                                    .text_size(px(11.))
                                    .child(node.name.clone()),
                            )
                            .child(mono(format!("{depth:.2}"), 10., p.muted))
                            .child(
                                Button::new(("storyboard-depth-less", i))
                                    .label("−")
                                    .xsmall()
                                    .ghost()
                                    .disabled(locked)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.set_depth(panel, id, depth - step, cx)
                                    })),
                            )
                            .child(
                                Button::new(("storyboard-depth-more", i))
                                    .label("+")
                                    .xsmall()
                                    .ghost()
                                    .disabled(locked)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.set_depth(panel, id, depth + step, cx)
                                    })),
                            ),
                    )
                    .child(presets),
            );
        }
        if !depths.is_empty() {
            root = root.child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(self.depth_view(panel, false, p))
                    .child(self.depth_view(panel, true, p)),
            );
        }
        root.children(self.follow_controls(panel, locked, p, cx))
            .into_any_element()
    }

    /// A top (or `side`) view of the panel's layers in depth: the panel
    /// plane, each layer at its depth at the size it is drawn in 3D, and
    /// the camera's field of view at the playhead (V6).
    fn depth_view(&self, panel: PageId, side: bool, p: &Palette) -> AnyElement {
        let Some(board) = self.editor.storyboard() else {
            return div().into_any_element();
        };
        let (w, h) = (
            f64::from(board.settings.width),
            f64::from(board.settings.height),
        );
        let state = self
            .stage_camera()
            .map(|(_, s)| s)
            .unwrap_or_else(|| board.rest_camera());
        let (camera, distance) = parallax_camera(board.settings.width, state);
        let depths = board
            .panels
            .get(&panel)
            .map(|p| p.depth.clone())
            .unwrap_or_default();
        let doc = &self.editor.doc;
        let selected = self.selected;
        let rest = if side { h / 2. } else { w / 2. };
        // Segments in (across, depth) world units, pixels.
        let mut layers: Vec<(f64, f64, f64, bool)> = Vec::new();
        for id in doc.children(None) {
            let depth = depths.get(&id).copied().unwrap_or(0.);
            let Some(b) = emulsion_core::geometry::node_bounds(doc, id) else {
                continue;
            };
            let (lo, hi) = if side {
                (f64::from(b.y), f64::from(b.y + b.h))
            } else {
                (f64::from(b.x), f64::from(b.x + b.w))
            };
            let scale = 1. + depth;
            layers.push((
                rest + (lo - rest) * scale,
                rest + (hi - rest) * scale,
                depth * distance,
                Some(id) == selected,
            ));
        }
        let eye_across = if side { state.y } else { state.x };
        let eye_depth = -distance / state.zoom.max(1e-3);
        let half = if side {
            camera.vertical_fov_deg(board.aspect()) / 2.
        } else {
            camera.horizontal_fov_deg() / 2.
        };
        let far = layers.iter().map(|l| l.2).fold(distance * 0.25, f64::max);
        let spread = (far - eye_depth) * f64::from(half.to_radians().tan());
        let (ink, muted, accent, line) = (p.ink, p.muted, p.accent, p.line);
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .flex_1()
            .child(mono(if side { "Side" } else { "Top" }, 9.5, p.muted))
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        let (bw, bh) = (
                            f64::from(f32::from(bounds.size.width)),
                            f64::from(f32::from(bounds.size.height)),
                        );
                        let lo = layers
                            .iter()
                            .map(|l| l.0)
                            .fold(eye_across - spread, f64::min);
                        let hi = layers
                            .iter()
                            .map(|l| l.1)
                            .fold(eye_across + spread, f64::max);
                        let k = ((bw - 8.) / (hi - lo).max(1.))
                            .min((bh - 8.) / (far - eye_depth).max(1.));
                        let at = |across: f64, depth: f64| {
                            point(
                                bounds.origin.x + px((4. + (across - lo) * k) as f32),
                                bounds.origin.y + px((bh - 4. - (depth - eye_depth) * k) as f32),
                            )
                        };
                        let mut stroke =
                            |a: Point<Pixels>, b: Point<Pixels>, width: f32, color: Hsla| {
                                let mut path = PathBuilder::stroke(px(width));
                                path.move_to(a);
                                path.line_to(b);
                                if let Ok(path) = path.build() {
                                    window.paint_path(path, color);
                                }
                            };
                        // The field of view.
                        let eye = at(eye_across, eye_depth);
                        stroke(eye, at(eye_across - spread, far), 1., line);
                        stroke(eye, at(eye_across + spread, far), 1., line);
                        // The panel plane as drawn.
                        stroke(at(0., 0.), at(if side { h } else { w }, 0.), 1., muted);
                        for (a, b, depth, chosen) in &layers {
                            stroke(
                                at(*a, *depth),
                                at(*b, *depth),
                                if *chosen { 3. } else { 2. },
                                if *chosen { accent } else { ink },
                            );
                        }
                    },
                )
                .h(px(90.))
                .w_full(),
            )
            .into_any_element()
    }

    /// Make the selected layer follow an object of the panel's set (C12).
    fn follow_controls(
        &self,
        panel: PageId,
        locked: bool,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let shot = self.editor.panel_shot(panel)?;
        let layer = self
            .selected
            .filter(|id| self.editor.doc.node(*id).is_some())?;
        let following = shot.attachments.get(&layer).map(|a| a.object);
        let mut row = div().flex().flex_wrap().gap(px(3.));
        for (i, o) in shot
            .set
            .objects
            .iter()
            .filter(|o| !matches!(o.kind, emulsion_scene::ObjectKind::Light(_)))
            .enumerate()
        {
            let object = o.id;
            let on = following == Some(object);
            row = row.child(
                chip(("storyboard-follow", i), o.name.clone(), on, p)
                    .test_support()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if locked {
                            return;
                        }
                        let result = if on {
                            this.editor.detach_layer_from_shot(panel, layer)
                        } else {
                            this.editor
                                .attach_layer_to_shot(panel, layer, object, None, None)
                        };
                        match result {
                            Ok(()) => this.after_change(cx),
                            Err(error) => this.set_status(error, true, cx),
                        }
                    })),
            );
        }
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(3.))
                .child(mono(
                    "The selected layer follows (moves with the set):",
                    9.5,
                    p.muted,
                ))
                .child(row)
                .into_any_element(),
        )
    }
}
