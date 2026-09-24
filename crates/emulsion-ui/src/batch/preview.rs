//! Navigation of the rendered batch preview, independent of export settings.
use super::*;
use crate::viewport::View;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
pub(super) struct PreviewNavigation {
    path: Option<PathBuf>,
    view: View,
    bounds: Option<Bounds<Pixels>>,
    dimensions: (u32, u32),
    manual: bool,
    drag: Option<Point<Pixels>>,
}

impl PreviewNavigation {
    fn sync(&mut self, path: &Path, dimensions: (u32, u32)) {
        if self.path.as_deref() != Some(path) || self.dimensions != dimensions {
            self.path = Some(path.into());
            self.dimensions = dimensions;
            self.manual = false;
            self.drag = None;
        }
    }

    fn layout(&mut self, bounds: Bounds<Pixels>) {
        self.bounds = Some(bounds);
        if !self.manual {
            self.fit();
        }
    }

    fn fit(&mut self) {
        self.manual = false;
        self.drag = None;
        if let Some(bounds) = self.bounds {
            self.view.fit(self.dimensions.0, self.dimensions.1, &bounds);
        }
    }

    fn zoom(&mut self, factor: f64, anchor: Point<Pixels>) {
        if let Some(bounds) = self.bounds {
            self.view.zoom_at(
                factor,
                (f32::from(anchor.x) as f64, f32::from(anchor.y) as f64),
                &bounds,
            );
            self.manual = true;
        }
    }

    fn step(&mut self, zoom_in: bool) {
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

    fn pan(&mut self, dx: f64, dy: f64) {
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
        let p = theme::palette(cx);
        let dimensions = image.size(0);
        let navigation = self.batch.navigation.clone();
        navigation.borrow_mut().sync(
            &path,
            (dimensions.width.0 as u32, dimensions.height.0 as u32),
        );
        let label = if navigation.borrow().manual {
            format!("{:.0}% preview", navigation.borrow().view.zoom * 100.)
        } else {
            "Fit".into()
        };
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
                cx.listener(|this, _, _, _| this.batch.navigation.borrow_mut().drag = None),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.batch.navigation.borrow_mut().drag = None),
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
                    .px(px(8.))
                    .py(px(6.))
                    .child(
                        chip("batch-preview-fit", "Fit image", false, &p)
                            .test_support()
                            .on_click(cx.listener(|this, _, _, cx| {
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
                    .child(mono(label, 10., p.muted)),
            )
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
