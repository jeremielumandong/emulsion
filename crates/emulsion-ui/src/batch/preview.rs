//! Navigation of the rendered batch preview, independent of export settings.
use super::*;
use crate::viewport::View;
use gpui_kit::component::Disableable;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
pub(crate) struct PreviewNavigation {
    path: Option<PathBuf>,
    pub(crate) view: View,
    pub(crate) bounds: Option<Bounds<Pixels>>,
    pub(crate) dimensions: (u32, u32),
    pub(crate) manual: bool,
    pub(crate) drag: Option<Point<Pixels>>,
}

impl PreviewNavigation {
    pub(crate) fn sync(&mut self, path: &Path, dimensions: (u32, u32)) {
        if self.path.as_deref() != Some(path) || self.dimensions != dimensions {
            self.path = Some(path.into());
            self.dimensions = dimensions;
            self.manual = false;
            self.drag = None;
        }
    }

    pub(crate) fn layout(&mut self, bounds: Bounds<Pixels>) {
        self.bounds = Some(bounds);
        if !self.manual {
            self.fit();
        }
    }

    pub(crate) fn fit(&mut self) {
        self.manual = false;
        self.drag = None;
        if let Some(bounds) = self.bounds {
            self.view.fit(self.dimensions.0, self.dimensions.1, &bounds);
        }
    }

    pub(crate) fn zoom(&mut self, factor: f64, anchor: Point<Pixels>) {
        if let Some(bounds) = self.bounds {
            self.view.zoom_at(
                factor,
                (f32::from(anchor.x) as f64, f32::from(anchor.y) as f64),
                &bounds,
            );
            self.manual = true;
        }
    }

    pub(crate) fn step(&mut self, zoom_in: bool) {
        if let Some(bounds) = self.bounds {
            let center = bounds.center();
            self.view.step(
                zoom_in,
                (f32::from(center.x) as f64, f32::from(center.y) as f64),
                &bounds,
            );
            self.manual = true;
        }
    }

    pub(crate) fn pan(&mut self, dx: f64, dy: f64) {
        self.view.pan(dx, dy);
        self.manual = true;
    }
}

pub(super) type Navigation = Rc<RefCell<PreviewNavigation>>;

impl Workspace {
    pub(super) fn batch_image_preview(
        &mut self,
        path: PathBuf,
        image: Arc<RenderImage>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = classic::palette(cx);
        let dimensions = image.size(0);
        let navigation = self.batch.navigation.clone();
        navigation.borrow_mut().sync(
            &path,
            (dimensions.width.0 as u32, dimensions.height.0 as u32),
        );
        let rotation_panel = self.library_rotation_panel(cx);
        let rotation_grid = self.batch.develop.rotation_controls.open && !self.batch.develop.before;
        let detail = self.batch.develop.detail_region.is_some();
        if detail && !navigation.borrow().manual {
            let mut nav = navigation.borrow_mut();
            nav.manual = true;
            nav.view.zoom = 1.;
            nav.view.center = (nav.dimensions.0 as f64 * 0.5, nav.dimensions.1 as f64 * 0.5);
        }
        let label = if detail {
            "100% · full-resolution region".into()
        } else if navigation.borrow().manual {
            format!("{:.0}% preview", navigation.borrow().view.zoom * 100.)
        } else {
            "Fit".into()
        };
        let tool = self.batch.develop.canvas_tool;
        let points = self.batch.develop.canvas_points.clone();
        let guides = self
            .batch
            .develop
            .perspective_guides
            .as_ref()
            .filter(|(owner, _)| owner == &path)
            .map(|(_, guides)| guides.clone())
            .unwrap_or_default();
        let rotation = self
            .batch
            .develop
            .current_params(&path)
            .map_or(0, |p| p.rotation);
        let crop = self
            .batch
            .current
            .and_then(|i| self.batch.items.get(i))
            .and_then(|i| self.batch.develop.current_params(&i.path))
            .map(|p| p.crop);
        let spots = self.library_edit_set().map(|e| e.spots).unwrap_or_default();
        let layout = navigation.clone();
        let paint = navigation.clone();
        let surface = div()
            .id("batch-preview-canvas")
            .test_support()
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .cursor(CursorStyle::OpenHand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, _, cx| {
                    if this.library_canvas_down(e, cx) {
                        cx.stop_propagation();
                        return;
                    }
                    if this.batch.develop.picking_sky
                        && this.batch.preview.is_some()
                        && !this.batch.develop.busy
                    {
                        let seed = {
                            let nav = this.batch.navigation.borrow();
                            nav.bounds.map(|bounds| {
                                let (x, y) = nav.view.screen_to_doc(
                                    (
                                        f32::from(e.position.x) as f64,
                                        f32::from(e.position.y) as f64,
                                    ),
                                    &bounds,
                                );
                                [
                                    x as f32 / nav.dimensions.0 as f32,
                                    y as f32 / nav.dimensions.1 as f32,
                                ]
                            })
                        };
                        if let Some(seed) =
                            seed.filter(|s| s.iter().all(|v| (0.0..=1.0).contains(v)))
                        {
                            this.batch.develop.picking_sky = false;
                            this.batch.develop.mask_seed = Some(seed);
                            this.library_enhance(3, cx);
                        }
                        cx.stop_propagation();
                        return;
                    }
                    let mut nav = this.batch.navigation.borrow_mut();
                    if e.click_count == 2 {
                        nav.fit();
                    } else {
                        nav.drag = Some(e.position);
                    }
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                if this.library_canvas_move(e, cx) {
                    return;
                }
                let mut nav = this.batch.navigation.borrow_mut();
                if e.pressed_button != Some(MouseButton::Left) {
                    nav.drag = None;
                    return;
                }
                if let Some(last) = nav.drag {
                    nav.drag = Some(e.position);
                    nav.pan(
                        f32::from(e.position.x - last.x) as f64,
                        f32::from(e.position.y - last.y) as f64,
                    );
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.batch.navigation.borrow_mut().drag = None;
                    this.library_canvas_up(cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.batch.navigation.borrow_mut().drag = None;
                    this.library_canvas_up(cx);
                }),
            )
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| {
                let delta = e.delta.pixel_delta(px(20.));
                let (dx, dy) = (f32::from(delta.x) as f64, f32::from(delta.y) as f64);
                let mut nav = this.batch.navigation.borrow_mut();
                if e.modifiers.control || e.modifiers.alt || e.modifiers.platform {
                    nav.zoom((dy * 0.004).exp(), e.position);
                } else if e.modifiers.shift {
                    nav.pan(dy, dx);
                } else {
                    nav.pan(dx, dy);
                }
                cx.stop_propagation();
                cx.notify();
            }))
            .on_pinch(cx.listener(|this, e: &PinchEvent, _, cx| {
                this.batch
                    .navigation
                    .borrow_mut()
                    .zoom((1.0 + e.delta as f64).max(0.01), e.position);
                cx.stop_propagation();
                cx.notify();
            }))
            .child(
                canvas(
                    move |bounds, _, _| layout.borrow_mut().layout(bounds),
                    move |bounds, _, window, _| {
                        let nav = paint.borrow();
                        let (x, y) = nav.view.doc_to_screen((0., 0.), &bounds);
                        let rect = Bounds::new(
                            point(px(x as f32), px(y as f32)),
                            size(
                                px(nav.dimensions.0 as f32 * nav.view.zoom as f32),
                                px(nav.dimensions.1 as f32 * nav.view.zoom as f32),
                            ),
                        );
                        let _ = window.paint_image(
                            rect,
                            rect,
                            Corners::default(),
                            image.clone(),
                            0,
                            false,
                        );
                        if rotation_grid {
                            let mut grid = PathBuilder::stroke(px(1.));
                            for i in 1..8 {
                                let fraction = i as f32 / 8.;
                                let x = rect.left() + rect.size.width * fraction;
                                let y = rect.top() + rect.size.height * fraction;
                                grid.move_to(point(x, rect.top()));
                                grid.line_to(point(x, rect.bottom()));
                                grid.move_to(point(rect.left(), y));
                                grid.line_to(point(rect.right(), y));
                            }
                            if let Ok(grid) = grid.build() {
                                window.paint_path(grid, gpui_kit::white().opacity(0.35));
                            }
                        }
                        let at = |p: [f32; 2]| {
                            let p = super::local_edits::rotate_point(p, rotation);
                            let (x, y) = nav.view.doc_to_screen(
                                (
                                    p[0] as f64 * nav.dimensions.0 as f64,
                                    p[1] as f64 * nav.dimensions.1 as f64,
                                ),
                                &bounds,
                            );
                            point(px(x as f32), px(y as f32))
                        };
                        let color = gpui_kit::rgb(0xf2d37a);
                        if tool == 7 {
                            let mut path = PathBuilder::stroke(px(2.));
                            for [start, end] in &guides {
                                path.move_to(at(*start));
                                path.line_to(at(*end));
                            }
                            if let Ok(path) = path.build() {
                                window.paint_path(path, color);
                            }
                        }
                        if tool != 0 {
                            for spot in &spots {
                                let positions = if spot.mode
                                    == emulsion_core::develop_edits::SpotMode::ContentAware
                                {
                                    vec![spot.target]
                                } else {
                                    vec![spot.source, spot.target]
                                };
                                for position in positions {
                                    window.paint_quad(outline(
                                        Bounds::new(
                                            at(position) - point(px(4.), px(4.)),
                                            size(px(8.), px(8.)),
                                        ),
                                        color,
                                        BorderStyle::Solid,
                                    ));
                                }
                            }
                            if tool == 5
                                && let Some([l, t, r, b]) = crop
                            {
                                for (a, z) in [
                                    ([l, t], [r, t]),
                                    ([r, t], [r, b]),
                                    ([r, b], [l, b]),
                                    ([l, b], [l, t]),
                                    ([l + (r - l) / 3., t], [l + (r - l) / 3., b]),
                                    ([l + 2. * (r - l) / 3., t], [l + 2. * (r - l) / 3., b]),
                                    ([l, t + (b - t) / 3.], [r, t + (b - t) / 3.]),
                                    ([l, t + 2. * (b - t) / 3.], [r, t + 2. * (b - t) / 3.]),
                                ] {
                                    let mut line = PathBuilder::stroke(px(1.));
                                    line.move_to(at(a));
                                    line.line_to(at(z));
                                    if let Ok(line) = line.build() {
                                        window.paint_path(line, color);
                                    }
                                }
                                for position in [[l, t], [r, t], [r, b], [l, b]] {
                                    window.paint_quad(fill(
                                        Bounds::new(
                                            at(position) - point(px(3.), px(3.)),
                                            size(px(6.), px(6.)),
                                        ),
                                        color,
                                    ));
                                }
                            }
                            if let Some(first) = points.first() {
                                let mut line = PathBuilder::stroke(px(2.));
                                line.move_to(at(*first));
                                for point in &points[1..] {
                                    line.line_to(at(*point));
                                }
                                if let Ok(line) = line.build() {
                                    window.paint_path(line, color);
                                }
                            }
                        }
                    },
                )
                .size_full(),
            );
        div()
            .size_full()
            .flex()
            .flex_col()
            .min_h_0()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(self.library_rotation_controls(cx))
                    .px(px(8.))
                    .py(px(6.))
                    .child(
                        chip("batch-preview-fit", "Fit image", false, &p)
                            .test_support()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.batch.develop.detail_region = None;
                                this.invalidate_library_preview();
                                this.batch.navigation.borrow_mut().fit();
                                cx.notify();
                            })),
                    )
                    .child(
                        chip("batch-preview-out", "−", false, &p)
                            .aria_label("Zoom out")
                            .test_support()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.batch.navigation.borrow_mut().step(false);
                                cx.notify();
                            })),
                    )
                    .child(
                        chip("batch-preview-in", "+", false, &p)
                            .aria_label("Zoom in")
                            .test_support()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.batch.navigation.borrow_mut().step(true);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("library-detail-100")
                            .label("100%")
                            .small()
                            .ghost()
                            .disabled(
                                self.batch
                                    .develop
                                    .source
                                    .as_ref()
                                    .is_some_and(|s| s.is_proxy()),
                            )
                            .tooltip("Full-resolution detail requires the original")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.batch.develop.detail_region = Some([0.5, 0.5]);
                                this.batch.develop.canvas_tool = 0;
                                this.batch.navigation.borrow_mut().manual = false;
                                this.invalidate_library_preview();
                                cx.notify();
                            })),
                    )
                    .child(mono(label, 10., p.muted)),
            )
            .children(rotation_panel)
            .child(surface)
            .child(
                mono(
                    "Drag / scroll to pan · Ctrl+scroll to zoom · Double-click to fit",
                    9.,
                    p.muted,
                )
                .px(px(8.))
                .py(px(6.)),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[gpui_kit::test]
    fn batch_preview_controls_zoom_pan_and_fit(cx: &mut TestAppContext) {
        use crate::app_state::{AppSettings, Capabilities, CliStatus};
        use crate::workspace::Screen;
        use emulsion_io::settings::Settings;
        use gpui_kit::component::Root;
        use gpui_kit::test::TestWindowExt;
        cx.update(|cx| {
            gpui_kit::init(cx);
            theme::install(cx);
            crate::actions::bind(cx);
            cx.set_global(AppSettings(Settings::default()));
            cx.set_global(Capabilities {
                cli: CliStatus::Missing,
            });
        });
        let slot = Rc::new(RefCell::new(None));
        let saved = slot.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let ws = cx.new(|cx| {
                let mut ws = Workspace::new(window, cx);
                ws.splash = false;
                ws.screen = Screen::Batch;
                ws.batch.develop.loupe = true;
                ws.batch.current = Some(0);
                ws.batch.items.push(BatchItem {
                    path: "preview.png".into(),
                    selected: false,
                    thumb: None,
                });
                ws.batch.preview = Some((
                    "preview.png".into(),
                    None,
                    Arc::new(bgra_image(1000, 500, vec![255; 1000 * 500 * 4])),
                ));
                ws
            });
            *saved.borrow_mut() = Some(ws.clone());
            Root::new(ws, window, cx)
        });
        let ws = slot.borrow().clone().unwrap();
        cx.run_until_parked();
        let initial = cx.update(|_, cx| ws.read(cx).batch.navigation.borrow().view);
        cx.update(|window, cx| window.click("batch-preview-in", cx));
        cx.run_until_parked();
        let zoomed = cx.update(|_, cx| ws.read(cx).batch.navigation.borrow().view);
        assert!(zoomed.zoom > initial.zoom);
        let center = cx.update(|window, _| window.find("batch-preview-canvas").bounds().center());
        cx.simulate_mouse_down(center, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(
            center + point(px(30.), px(20.)),
            Some(MouseButton::Left),
            Modifiers::none(),
        );
        cx.simulate_mouse_up(
            center + point(px(30.), px(20.)),
            MouseButton::Left,
            Modifiers::none(),
        );
        cx.run_until_parked();
        let panned = cx.update(|_, cx| ws.read(cx).batch.navigation.borrow().view);
        assert_ne!(panned.center, zoomed.center);
        cx.update(|window, cx| window.click("batch-preview-fit", cx));
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| ws.read(cx).batch.navigation.borrow().view),
            initial
        );
    }

    #[test]
    fn preview_navigation_fits_zooms_at_pointer_and_resets_for_new_photo() {
        let mut nav = PreviewNavigation::default();
        nav.sync(Path::new("one.png"), (1000, 500));
        let bounds = Bounds::new(point(px(10.), px(20.)), size(px(600.), px(400.)));
        nav.layout(bounds);
        assert_eq!(nav.view.center, (500., 250.));
        assert!((nav.view.zoom - 0.52).abs() < 1e-6);
        let anchor = point(px(150.), px(180.));
        let before = nav.view.screen_to_doc((150., 180.), &bounds);
        nav.zoom(2., anchor);
        let after = nav.view.screen_to_doc((150., 180.), &bounds);
        assert!((before.0 - after.0).abs() < 1e-6 && (before.1 - after.1).abs() < 1e-6);
        nav.pan(52., 0.);
        let view = nav.view;
        nav.sync(Path::new("one.png"), (1000, 500));
        nav.layout(bounds);
        assert_eq!(nav.view, view, "recipe redraw keeps navigation");
        nav.sync(Path::new("two.png"), (1000, 500));
        nav.layout(bounds);
        assert_eq!(nav.view.center, (500., 250.));
        assert!(!nav.manual);
        nav.step(true);
        nav.fit();
        assert!((nav.view.zoom - 0.52).abs() < 1e-6);
    }
}
