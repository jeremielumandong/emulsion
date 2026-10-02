//! The storyboard Stage around the panel being drawn: the camera frame with
//! its safe areas, field guide and overscan pasteboard; Camera view; the
//! light table of neighbouring panels; the reference dock; and the board's
//! colour palette. Geometry comes from `emulsion_core::storyboard_stage`.
//!
//! The guide values live on the board (`Storyboard.stage`, edited as one
//! Undo step each); whether safe areas show, Camera view and the reference
//! dock are ways of looking, so they are view state here. Flip view is part
//! of the shared [`View`] transform, so it works for every canvas.
use super::*;
use emulsion_core::project::PageId;
use emulsion_core::storyboard::{Frame, LightTable, StageGuides};
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
    menu::{PopupMenu, PopupMenuItem},
    tooltip::Tooltip,
};

#[cfg(test)]
#[path = "storyboard_stage_tests.rs"]
mod tests;

/// Overscan choices offered in the View menu, in percent of the frame.
const OVERSCAN_STEPS: [f64; 6] = [0., 5., 10., 15., 20., 25.];
/// Field guide sizes offered in the View menu.
const FIELD_STEPS: [u8; 4] = [8, 10, 12, 16];
/// Light table opacity steps the Stage toolbar cycles through.
const OPACITY_STEPS: [f64; 5] = [0.1, 0.2, 0.3, 0.5, 0.7];
/// The light table draws neighbours at canvas resolution, up to this size.
const LIGHT_MAX: u32 = 2048;

#[derive(Default)]
pub(crate) struct StageUi {
    /// Safe areas are hidden on this view; the board keeps their values.
    pub(crate) hide_safe_areas: bool,
    /// Only the framed shot: no overscan, guides or light table.
    pub(crate) camera_view: bool,
    /// The reference dock beside the Stage.
    pub(crate) reference_open: bool,
    /// Reference previews are mirrored; the reference itself is unchanged.
    pub(crate) reference_mirror: bool,
    /// Light table pictures to draw, farthest first.
    light: Vec<Arc<RenderImage>>,
    /// Faded pictures by panel, with what they were made from.
    baked: HashMap<PageId, (BakeKey, Arc<RenderImage>)>,
    /// Replaced pictures, released from the GPU on the next paint.
    retired: Vec<Arc<RenderImage>>,
}

#[derive(Clone, Copy, PartialEq)]
struct BakeKey {
    /// The thumbnail the picture was made from.
    source: usize,
    opacity: u64,
    tint: Option<[u8; 3]>,
    flip: (bool, bool),
}

/// What the canvas paints for the Stage under the other overlays.
#[derive(Clone, Default)]
pub(crate) struct StagePaint {
    /// The area shown around the camera frame, and the frame itself, in
    /// document pixels.
    pub(crate) pasteboard: Option<(Frame, Frame)>,
    /// Light table pictures, farthest first, covering the frame.
    pub(crate) light: Vec<Arc<RenderImage>>,
    pub(crate) frame: (f64, f64),
    /// The scene camera's frame and the Camera view mask.
    pub(crate) camera: super::storyboard_camera::CameraPaint,
    retired: Vec<Arc<RenderImage>>,
}

/// A closed rectangle outline in document pixels.
fn outline_of(f: &Frame) -> Vec<(f64, f64)> {
    vec![
        (f.x, f.y),
        (f.x + f.w, f.y),
        (f.x + f.w, f.y + f.h),
        (f.x, f.y + f.h),
        (f.x, f.y),
    ]
}

/// A neighbour's picture as the light table shows it. Paper drops out the
/// way it does on a real light table: each pixel's whiteness becomes
/// transparency, so over white it looks like the original at `opacity`
/// (or the tint colour, for tinted panels). Optionally mirrored to match a
/// flipped view.
fn bake(
    source: &RenderImage,
    opacity: f64,
    tint: Option<[u8; 3]>,
    flip: (bool, bool),
) -> Option<RenderImage> {
    let bytes = source.as_bytes(0)?;
    let size = source.size(0);
    let (w, h) = (size.width.0 as usize, size.height.0 as usize);
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        let sy = if flip.1 { h - 1 - y } else { y };
        for x in 0..w {
            let sx = if flip.0 { w - 1 - x } else { x };
            let s = &bytes[(sy * w + sx) * 4..][..4];
            // BGRA, straight alpha.
            let lightest = f64::from(s[0].min(s[1]).min(s[2])) / 255.;
            let ink = 1. - lightest;
            let alpha = f64::from(s[3]) / 255. * ink * opacity;
            let d = &mut out[(y * w + x) * 4..][..4];
            if ink > 0. {
                let color = |c: u8| (f64::from(c) / 255. - lightest) / ink * 255.;
                match tint {
                    Some([r, g, b]) => d[..3].copy_from_slice(&[b, g, r]),
                    None => {
                        for (d, s) in d[..3].iter_mut().zip(s) {
                            *d = color(*s).round().clamp(0., 255.) as u8;
                        }
                    }
                }
            }
            d[3] = (alpha * 255.).round().clamp(0., 255.) as u8;
        }
    }
    Some(viewport::bgra_image(w as u32, h as u32, out))
}

/// Paint the pasteboard and the light table, in the canvas's content mask.
pub(super) fn paint_stage(
    stage: &StagePaint,
    view: &View,
    bounds: Bounds<Pixels>,
    window: &mut Window,
) {
    for image in &stage.retired {
        let _ = window.drop_image(image.clone());
    }
    super::storyboard_camera::paint_camera(&stage.camera, view, bounds, window);
    let to_screen = |p: (f64, f64)| {
        let s = view.doc_to_screen(p, &bounds);
        point(px(s.0 as f32), px(s.1 as f32))
    };
    if let Some((area, frame)) = &stage.pasteboard {
        // The margin as four bands, so it turns and flips with the view.
        let (ax1, ay1) = (area.x + area.w, area.y + area.h);
        let (fx1, fy1) = (frame.x + frame.w, frame.y + frame.h);
        let bands = [
            (area.x, area.y, ax1, frame.y),
            (area.x, fy1, ax1, ay1),
            (area.x, frame.y, frame.x, fy1),
            (fx1, frame.y, ax1, fy1),
        ];
        let mut path = PathBuilder::fill();
        for (x0, y0, x1, y1) in bands {
            if x1 <= x0 || y1 <= y0 {
                continue;
            }
            path.move_to(to_screen((x0, y0)));
            path.line_to(to_screen((x1, y0)));
            path.line_to(to_screen((x1, y1)));
            path.line_to(to_screen((x0, y1)));
            path.close();
        }
        if let Ok(path) = path.build() {
            window.paint_path(path, hsla(0., 0., 0.5, 0.16));
        }
    }
    // Pictures are axis-aligned; flips are baked into them, and a rotated
    // view shows no light table.
    if stage.light.is_empty() || view.rotation.rem_euclid(360.) != 0. {
        return;
    }
    let a = to_screen((0., 0.));
    let b = to_screen(stage.frame);
    let rect = Bounds::from_corners(
        point(a.x.min(b.x), a.y.min(b.y)),
        point(a.x.max(b.x), a.y.max(b.y)),
    );
    for image in &stage.light {
        let _ = window.paint_image(rect, rect, Corners::default(), image.clone(), 0, false);
    }
}

impl EditorView {
    /// The board's guides while the Stage shows a panel.
    fn stage_guides(&self) -> Option<&StageGuides> {
        if self.board_open() {
            return None;
        }
        self.editor.storyboard().map(|b| &b.stage)
    }

    pub(crate) fn camera_view(&self) -> bool {
        self.stage_ui.camera_view && self.editor.storyboard().is_some()
    }

    /// Outlines the Stage draws over the panel, in document pixels: the
    /// camera frame, safe areas, the field guide with its centre cross, and
    /// a thumbnail sheet's frames. Camera view shows none of them.
    pub(crate) fn stage_lines(&self) -> Vec<Vec<(f64, f64)>> {
        let Some(guides) = self.stage_guides() else {
            return self.thumbnail_sheet_frames();
        };
        if self.camera_view() {
            return Vec::new();
        }
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        let mut lines = vec![outline_of(&Frame::centred(w, h, 100.))];
        if !self.stage_ui.hide_safe_areas {
            lines.extend(guides.safe_areas(w, h).iter().map(outline_of));
        }
        let fields = guides.field_rects(w, h);
        if let Some(first) = fields.first() {
            lines.extend(fields.iter().map(outline_of));
            let (cx, cy) = (f64::from(w) / 2., f64::from(h) / 2.);
            lines.push(vec![(first.x, cy), (first.x + first.w, cy)]);
            lines.push(vec![(cx, first.y), (cx, first.y + first.h)]);
        }
        lines.extend(self.thumbnail_sheet_frames());
        lines
    }

    pub(crate) fn stage_paint(&mut self) -> StagePaint {
        let Some(guides) = self.stage_guides() else {
            return StagePaint::default();
        };
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        let camera = self.camera_view();
        StagePaint {
            pasteboard: (!camera && guides.overscan > 0.)
                .then(|| (guides.stage_area(w, h), Frame::centred(w, h, 100.))),
            light: if camera {
                Vec::new()
            } else {
                self.stage_ui.light.clone()
            },
            frame: (f64::from(w), f64::from(h)),
            camera: self.camera_paint(),
            retired: std::mem::take(&mut self.stage_ui.retired),
        }
    }

    /// The panels the light table shows under the active one, farthest
    /// first, with their opacity and tint.
    pub(crate) fn light_table_layers(&self, cx: &App) -> Vec<(PageId, f64, Option<[u8; 3]>)> {
        if self.stage_guides().is_none() || self.camera_view() {
            return Vec::new();
        }
        let layout: Vec<_> = self.editor.page_list().iter().map(|m| m.id).collect();
        crate::app_state::settings(cx)
            .storyboard
            .light_table
            .layers(&layout, self.editor.active_page())
    }

    /// Bring the light table's pictures up to date: each neighbour's page
    /// thumbnail at canvas resolution (cached by page revision), faded and
    /// tinted once per change.
    pub(crate) fn prepare_stage(&mut self, cx: &mut Context<Self>) {
        let layers = self.light_table_layers(cx);
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        let max = w.max(h).min(LIGHT_MAX);
        let flip = (self.view.flip_x, self.view.flip_y);
        let mut light = Vec::new();
        let mut baked = HashMap::new();
        for (id, opacity, tint) in layers {
            let Some(source) = self.page_thumbnail(id, max, cx) else {
                continue;
            };
            let key = BakeKey {
                source: Arc::as_ptr(&source) as usize,
                opacity: opacity.to_bits(),
                tint,
                flip,
            };
            let image = match self.stage_ui.baked.remove(&id) {
                Some((old, image)) if old == key => image,
                previous => {
                    self.stage_ui
                        .retired
                        .extend(previous.map(|(_, image)| image));
                    let Some(image) = bake(&source, opacity, tint, flip) else {
                        continue;
                    };
                    Arc::new(image)
                }
            };
            light.push(image.clone());
            baked.insert(id, (key, image));
        }
        let stale = std::mem::replace(&mut self.stage_ui.baked, baked);
        self.stage_ui
            .retired
            .extend(stale.into_values().map(|(_, image)| image));
        self.stage_ui.light = light;
    }

    /// Change the light table settings, validated and saved like every
    /// other preference.
    pub(crate) fn update_light_table(
        &mut self,
        edit: impl FnOnce(&mut LightTable),
        cx: &mut Context<Self>,
    ) {
        let mut table = crate::app_state::settings(cx)
            .storyboard
            .light_table
            .clone();
        edit(&mut table);
        if let Err(error) = table.validate() {
            self.set_status(error, true, cx);
            return;
        }
        crate::app_state::update_settings(cx, |s| s.storyboard.light_table = table);
        self.notify_canvas(cx);
        cx.notify();
    }

    pub(crate) fn toggle_light_table(&mut self, cx: &mut Context<Self>) {
        if self.editor.storyboard().is_none() {
            return;
        }
        self.update_light_table(|t| t.enabled = !t.enabled, cx);
        let on = crate::app_state::settings(cx)
            .storyboard
            .light_table
            .enabled;
        self.set_status(
            if on {
                "Light table on: neighbouring panels show faintly on the Stage."
            } else {
                "Light table off."
            },
            false,
            cx,
        );
    }

    pub(crate) fn toggle_camera_view(&mut self, cx: &mut Context<Self>) {
        if self.editor.storyboard().is_none() {
            return;
        }
        self.stage_ui.camera_view = !self.stage_ui.camera_view;
        self.zoom_fit(cx);
        self.notify_canvas(cx);
        cx.notify();
    }

    pub(crate) fn toggle_safe_areas(&mut self, cx: &mut Context<Self>) {
        self.stage_ui.hide_safe_areas = !self.stage_ui.hide_safe_areas;
        self.notify_canvas(cx);
        cx.notify();
    }

    /// Change the board's Stage guides as one Undo step.
    pub(crate) fn edit_stage_guides(
        &mut self,
        edit: impl FnOnce(&mut StageGuides),
        cx: &mut Context<Self>,
    ) -> bool {
        let done = self.edit_board(
            |board| {
                edit(&mut board.stage);
                board.stage.validate()
            },
            cx,
        );
        self.notify_canvas(cx);
        done
    }

    /// Flip the view, not the art. Shared by every canvas, like Rotate
    /// View; drawing input maps back through the same transform.
    pub(crate) fn flip_view(&mut self, horizontal: bool, cx: &mut Context<Self>) {
        if horizontal {
            self.view.flip_x = !self.view.flip_x;
        } else {
            self.view.flip_y = !self.view.flip_y;
        }
        self.notify_canvas(cx);
        cx.notify();
    }

    /// The size Fit on Screen frames: the camera frame plus overscan on the
    /// Stage, so the margin is in view.
    pub(crate) fn stage_fit_size(&self) -> Option<(u32, u32)> {
        let guides = self.stage_guides()?;
        if self.camera_view() || guides.overscan <= 0. {
            return None;
        }
        let area = guides.stage_area(self.editor.doc.width, self.editor.doc.height);
        Some((area.w.round() as u32, area.h.round() as u32))
    }

    // ── Palette ────────────────────────────────────────────────────────

    /// The board's palette, when the canvas is a storyboard panel.
    pub(crate) fn board_palette(&self) -> Option<&[[u8; 3]]> {
        self.editor.storyboard().map(|b| b.palette.as_slice())
    }

    /// Add the foreground colour to the board's palette, one Undo step.
    pub(crate) fn add_palette_color(&mut self, cx: &mut Context<Self>) {
        let [r, g, b, _] = self.tools.fg;
        self.edit_board(
            |board| {
                if !board.palette.contains(&[r, g, b]) {
                    board.palette.push([r, g, b]);
                }
                emulsion_core::storyboard_stage::validate_palette(&board.palette)
            },
            cx,
        );
    }

    pub(crate) fn remove_palette_color(&mut self, index: usize, cx: &mut Context<Self>) {
        self.edit_board(
            |board| {
                if index < board.palette.len() {
                    board.palette.remove(index);
                }
                Ok(())
            },
            cx,
        );
    }

    /// The board palette's swatches: click to paint with a colour,
    /// right-click to remove it, + adds the foreground colour.
    pub(crate) fn board_palette_swatches(
        &self,
        vertical: bool,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let palette = self.board_palette()?;
        let [fr, fg, fb, _] = self.tools.fg;
        let swatches = palette.iter().enumerate().map(|(index, &c)| {
            let hex = format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]);
            div()
                .id(("board-palette-color", index))
                .test_support()
                .role(Role::Button)
                .aria_label(format!("Use colour {hex}"))
                .tab_index(0)
                .size(rems(1.5))
                .rounded(px(3.))
                .border_2()
                .border_color(if c == [fr, fg, fb] { p.accent } else { p.line })
                .bg(rgb(u32::from_be_bytes([0, c[0], c[1], c[2]])))
                .cursor_pointer()
                .tooltip(move |w, cx| {
                    Tooltip::new(format!("{hex}: board palette · right-click to remove"))
                        .build(w, cx)
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.set_fg([c[0], c[1], c[2], 255], cx);
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, _, _, cx| this.remove_palette_color(index, cx)),
                )
                .into_any_element()
        });
        Some(
            div()
                .id("board-palette")
                .test_support()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(rems(0.25))
                .when(vertical, |d| d.w(rems(3.25)))
                .when(!vertical, |d| d.max_w(rems(28.)))
                .children(swatches)
                .child(
                    Button::new("board-palette-add")
                        .label("+")
                        .tooltip("Add the foreground colour to the board palette")
                        .xsmall()
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| this.add_palette_color(cx))),
                )
                .into_any_element(),
        )
    }

    // ── Controls ───────────────────────────────────────────────────────

    /// The Stage toolbar and the reference dock, over the canvas.
    pub(super) fn stage_controls(
        &mut self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(guides) = self.stage_guides().cloned() else {
            return Vec::new();
        };
        let table = crate::app_state::settings(cx)
            .storyboard
            .light_table
            .clone();
        let camera = self.camera_view();
        let keys = self.stage_key_buttons(cx);
        let toggle = |id: &'static str,
                      label: &'static str,
                      tip: &'static str,
                      on: bool,
                      run: fn(&mut Self, &mut Context<Self>)| {
            Button::new(id)
                .label(label)
                .tooltip(tip)
                .xsmall()
                .when(on, |b| b.primary())
                .when(!on, |b| b.ghost())
                .on_click(cx.listener(move |this, _, _, cx| run(this, cx)))
        };
        // The toolbar floats over the canvas: keep presses on it from
        // starting a tool drag underneath.
        let mut bar = div()
            .id("storyboard-stage-toolbar")
            .test_support()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .py(px(2.))
            .rounded(px(6.))
            .bg(p.panel.opacity(0.92))
            .border_1()
            .border_color(p.line)
            .child(toggle(
                "stage-camera-view",
                "Camera",
                "Camera view: only the framed shot, as the audience sees it",
                camera,
                Self::toggle_camera_view,
            ))
            .child(toggle(
                "stage-camera-tool",
                "Move camera",
                "Camera tool: pan, zoom and turn the scene camera at the playhead (Ctrl+Alt+E)",
                self.camera_ui.editing,
                Self::toggle_camera_tool,
            ));
        if !camera {
            bar = bar
                .child(toggle(
                    "stage-safe-areas",
                    "Safe",
                    "Show the action and title safe areas",
                    !self.stage_ui.hide_safe_areas,
                    Self::toggle_safe_areas,
                ))
                .child(toggle(
                    "stage-field-guide",
                    "Field",
                    "Show the field guide on this board",
                    guides.field_guide,
                    |this, cx| {
                        this.edit_stage_guides(|g| g.field_guide = !g.field_guide, cx);
                    },
                ))
                .child(toggle(
                    "stage-light-table",
                    "Light table",
                    "Show neighbouring panels faintly (Ctrl+Alt+O)",
                    table.enabled,
                    Self::toggle_light_table,
                ));
            if table.enabled {
                let step = |id: &'static str, label: String, tip: &'static str| {
                    Button::new(id).label(label).tooltip(tip).xsmall().ghost()
                };
                bar = bar
                    .child(
                        step(
                            "stage-light-before",
                            format!("◀ {}", table.before),
                            "Panels before the active one",
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.update_light_table(
                                |t| t.before = (t.before + 1) % (LightTable::MAX_NEIGHBOURS + 1),
                                cx,
                            )
                        })),
                    )
                    .child(
                        step(
                            "stage-light-after",
                            format!("{} ▶", table.after),
                            "Panels after the active one",
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.update_light_table(
                                |t| t.after = (t.after + 1) % (LightTable::MAX_NEIGHBOURS + 1),
                                cx,
                            )
                        })),
                    )
                    .child(
                        step(
                            "stage-light-opacity",
                            format!("{:.0}%", table.opacity * 100.),
                            "Light table opacity",
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.update_light_table(
                                |t| {
                                    t.opacity = OPACITY_STEPS
                                        .into_iter()
                                        .find(|o| *o > t.opacity + 1e-6)
                                        .unwrap_or(OPACITY_STEPS[0])
                                },
                                cx,
                            )
                        })),
                    )
                    .child(toggle(
                        "stage-light-tint",
                        "Tint",
                        "Tint earlier panels red and later ones blue",
                        table.tint,
                        |this, cx| this.update_light_table(|t| t.tint = !t.tint, cx),
                    ));
            }
        }
        bar = bar.children(keys);
        bar = bar
            .child(toggle(
                "stage-flip-horizontal",
                "⇋",
                "Flip the view horizontally (the art is unchanged)",
                self.view.flip_x,
                |this, cx| this.flip_view(true, cx),
            ))
            .child(toggle(
                "stage-flip-vertical",
                "⇵",
                "Flip the view vertically (the art is unchanged)",
                self.view.flip_y,
                |this, cx| this.flip_view(false, cx),
            ))
            .child(toggle(
                "stage-reference",
                "Reference",
                "Show reference images beside the Stage",
                self.stage_ui.reference_open,
                Self::toggle_reference_view,
            ));
        let mut out = vec![
            div()
                .absolute()
                .bottom_2()
                .left_2()
                .child(bar)
                .into_any_element(),
        ];
        out.extend(self.camera_controls(p, cx));
        out.extend(self.reference_dock(p, cx));
        out
    }

    pub(crate) fn toggle_reference_view(&mut self, cx: &mut Context<Self>) {
        self.stage_ui.reference_open = !self.stage_ui.reference_open;
        cx.notify();
    }

    pub(crate) fn toggle_reference_mirror(&mut self, cx: &mut Context<Self>) {
        self.stage_ui.reference_mirror = !self.stage_ui.reference_mirror;
        cx.notify();
    }

    /// The reference panel, docked at the Stage's right edge.
    fn reference_dock(&mut self, p: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.stage_ui.reference_open {
            return None;
        }
        Some(
            div()
                .id("storyboard-reference-dock")
                .test_support()
                .occlude()
                .absolute()
                .top_2()
                .right_2()
                .w(px(260.))
                .flex()
                .flex_col()
                .bg(p.panel)
                .border_1()
                .border_color(p.line)
                .rounded(px(6.))
                .child(
                    div().flex().justify_end().px_1().pt_1().child(
                        Button::new("storyboard-reference-close")
                            .label("Close")
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_reference_view(cx))),
                    ),
                )
                .child(self.reference_panel(p, cx))
                .into_any_element(),
        )
    }

    /// View menu entries for the Stage.
    pub(super) fn stage_view_items(
        menu: PopupMenu,
        editor: &Entity<EditorView>,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let view = editor.read(cx);
        let Some(guides) = view.editor.storyboard().map(|b| b.stage.clone()) else {
            return menu;
        };
        let table = crate::app_state::settings(cx)
            .storyboard
            .light_table
            .enabled;
        let checks = [
            ("Camera View", view.stage_ui.camera_view),
            ("Safe Areas", !view.stage_ui.hide_safe_areas),
            ("Field Guide", guides.field_guide),
            ("Light Table", table),
            ("Reference View", view.stage_ui.reference_open),
            ("Mirror Reference", view.stage_ui.reference_mirror),
        ];
        let runs: [fn(&mut EditorView, &mut Context<EditorView>); 6] = [
            Self::toggle_camera_view,
            Self::toggle_safe_areas,
            |e, cx| {
                e.edit_stage_guides(|g| g.field_guide = !g.field_guide, cx);
            },
            Self::toggle_light_table,
            Self::toggle_reference_view,
            Self::toggle_reference_mirror,
        ];
        let mut menu = menu;
        for ((label, checked), run) in checks.into_iter().zip(runs) {
            let owner = editor.downgrade();
            menu = menu.item(PopupMenuItem::new(label).checked(checked).on_click(
                move |_, _, cx| {
                    owner.update(cx, run).ok();
                },
            ));
        }
        let owner = editor.downgrade();
        let overscan = guides.overscan;
        menu = menu.submenu("Overscan", window, cx, move |mut menu, _, _| {
            for step in OVERSCAN_STEPS {
                let owner = owner.clone();
                menu = menu.item(
                    PopupMenuItem::new(format!("{step:.0}%"))
                        .checked((overscan - step).abs() < 1e-9)
                        .on_click(move |_, _, cx| {
                            owner
                                .update(cx, |e, cx| {
                                    e.edit_stage_guides(|g| g.overscan = step, cx);
                                })
                                .ok();
                        }),
                );
            }
            menu
        });
        let owner = editor.downgrade();
        let fields = guides.fields;
        menu.submenu("Field Guide Size", window, cx, move |mut menu, _, _| {
            for step in FIELD_STEPS {
                let owner = owner.clone();
                menu = menu.item(
                    PopupMenuItem::new(format!("{step} fields"))
                        .checked(fields == step)
                        .on_click(move |_, _, cx| {
                            owner
                                .update(cx, |e, cx| {
                                    e.edit_stage_guides(|g| g.fields = step, cx);
                                })
                                .ok();
                        }),
                );
            }
            menu
        })
        .separator()
    }
}

impl EditorView {
    /// The Swatches section: the board palette on storyboard panels, then
    /// the colours painted with in this project.
    pub(super) fn project_colors(
        &self,
        vertical: bool,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let painted = self.painted_colors(vertical, p, cx);
        match self.board_palette_swatches(vertical, p, cx) {
            Some(board) => div()
                .flex()
                .flex_col()
                .gap(rems(0.5))
                .child(board)
                .child(painted)
                .into_any_element(),
            None => painted,
        }
    }
}
