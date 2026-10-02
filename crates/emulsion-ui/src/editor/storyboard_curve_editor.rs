//! The ease curve editor (L4): one key segment's easing, chosen from the
//! presets or shaped as a bezier curve (`motion::Curve`) by dragging its two
//! handles on a small graph. Reusable: layer keys and camera keys both call
//! [`EditorView::ease_curve_editor`] with their own change callback.
use super::*;
use emulsion_core::motion::{self, Curve, Easing};
use gpui_kit::component::{
    Disableable, Sizable,
    button::Button,
    menu::{DropdownMenu, PopupMenuItem},
};
use std::cell::Cell;

/// Runs once per preset choice or finished handle drag, with the new easing
/// and curve (`None` for a plain preset).
pub(crate) type EaseChange =
    Rc<dyn Fn(&mut EditorView, Easing, Option<Curve>, &mut Context<EditorView>)>;

/// The graph's size, in pixels.
const GRAPH: f32 = 120.;
/// The progress range the graph shows, bottom to top.
const Y_MIN: f64 = -0.5;
const Y_MAX: f64 = 1.5;
/// Handle grab distance, in pixels.
const GRAB: f32 = 9.;

/// A handle being dragged on one editor's graph.
struct CurveDrag {
    id: SharedString,
    /// 0 for the first handle, 1 for the second.
    handle: usize,
    easing: Easing,
    curve: Curve,
    change: EaseChange,
}

#[derive(Default)]
pub(crate) struct CurveUi {
    /// Each editor's graph, measured when painted.
    graphs: HashMap<SharedString, Rc<Cell<Option<Bounds<Pixels>>>>>,
    drag: Option<CurveDrag>,
}

/// The bezier curve that looks like `easing`, to start editing from.
pub(crate) fn preset_curve(easing: Easing) -> Curve {
    let (x1, y1, x2, y2) = match easing {
        Easing::Linear | Easing::Step => (0.25, 0.25, 0.75, 0.75),
        Easing::EaseIn => (0.42, 0., 1., 1.),
        Easing::EaseOut => (0., 0., 0.58, 1.),
        Easing::EaseInOut => (0.42, 0., 0.58, 1.),
    };
    Curve { x1, y1, x2, y2 }
}

/// `curve` with handle `handle` moved to (x, y) in curve units, kept valid.
pub(crate) fn move_handle(mut curve: Curve, handle: usize, x: f64, y: f64) -> Curve {
    let (x, y) = (x.clamp(0., 1.), y.clamp(-10., 10.));
    if handle == 0 {
        (curve.x1, curve.y1) = (x, y);
    } else {
        (curve.x2, curve.y2) = (x, y);
    }
    curve
}

/// Graph point of curve point (x, y).
fn to_graph(b: &Bounds<Pixels>, x: f64, y: f64) -> Point<Pixels> {
    let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
    let fy = ((y - Y_MIN) / (Y_MAX - Y_MIN)) as f32;
    point(
        b.origin.x + px(x as f32 * w),
        b.origin.y + px((1. - fy) * h),
    )
}

/// Curve point of graph point `p`.
fn from_graph(b: &Bounds<Pixels>, p: Point<Pixels>) -> (f64, f64) {
    let (w, h) = (
        f32::from(b.size.width).max(1.),
        f32::from(b.size.height).max(1.),
    );
    let x = f64::from(f32::from(p.x - b.origin.x) / w);
    let fy = f64::from(1. - f32::from(p.y - b.origin.y) / h);
    (x, Y_MIN + fy * (Y_MAX - Y_MIN))
}

impl EditorView {
    /// An easing editor for one key segment: a preset menu (and Custom
    /// curve) over a graph of the ease, whose two bezier handles drag to
    /// shape it. `change` runs once per menu choice or finished drag, so an
    /// edit made through `edit_storyboard` is one Undo step. `id` must be
    /// unique among editors on screen.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn ease_curve_editor(
        &mut self,
        id: impl Into<SharedString>,
        easing: Easing,
        curve: Option<Curve>,
        disabled: bool,
        change: EaseChange,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id: SharedString = id.into();
        let graph = self
            .layer_keys
            .curves
            .graphs
            .entry(id.clone())
            .or_default()
            .clone();
        // A drag in progress shows its curve.
        let (shown_easing, shown_curve) = match &self.layer_keys.curves.drag {
            Some(drag) if drag.id == id => (drag.easing, Some(drag.curve)),
            _ => (easing, curve),
        };
        let menu_change = change.clone();
        let owner = cx.weak_entity();
        let menu = Button::new(SharedString::from(format!("{id}-preset")))
            .label(format!(
                "Ease: {} ▾",
                if curve.is_some() {
                    "Custom curve"
                } else {
                    easing.label()
                }
            ))
            .small()
            .outline()
            .disabled(disabled)
            .dropdown_menu(move |mut menu, _, _| {
                for choice in Easing::ALL {
                    let (owner, change) = (owner.clone(), menu_change.clone());
                    menu = menu.item(
                        PopupMenuItem::new(choice.label())
                            .checked(curve.is_none() && easing == choice)
                            .on_click(move |_, _, cx| {
                                owner.update(cx, |e, cx| change(e, choice, None, cx)).ok();
                            }),
                    );
                }
                let (owner, change) = (owner.clone(), menu_change.clone());
                menu.separator().item(
                    PopupMenuItem::new("Custom curve")
                        .checked(curve.is_some())
                        .on_click(move |_, _, cx| {
                            let start = curve.unwrap_or_else(|| preset_curve(easing));
                            owner
                                .update(cx, |e, cx| change(e, easing, Some(start), cx))
                                .ok();
                        }),
                )
            });
        let (line, ink, accent, muted) = (p.line, p.ink, p.accent, p.muted);
        let measure = graph.clone();
        let paint_id = id.clone();
        let weak = cx.weak_entity();
        let plot = canvas(
            move |bounds, _, _| measure.set(Some(bounds)),
            move |bounds, _, window, _| {
                // The 0 and 1 lines.
                for y in [0., 1.] {
                    let a = to_graph(&bounds, 0., y);
                    window.paint_quad(fill(Bounds::new(a, size(bounds.size.width, px(1.))), line));
                }
                let mut path = PathBuilder::stroke(px(1.5));
                for i in 0..=48 {
                    let t = f64::from(i) / 48.;
                    let v = motion::ease(shown_easing, shown_curve, t);
                    let q = to_graph(&bounds, t, v.clamp(Y_MIN, Y_MAX));
                    if i == 0 {
                        path.move_to(q);
                    } else {
                        path.line_to(q);
                    }
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, ink);
                }
                if let Some(c) = shown_curve {
                    let mut arms = PathBuilder::stroke(px(1.));
                    arms.move_to(to_graph(&bounds, 0., 0.));
                    arms.line_to(to_graph(&bounds, c.x1, c.y1.clamp(Y_MIN, Y_MAX)));
                    arms.move_to(to_graph(&bounds, 1., 1.));
                    arms.line_to(to_graph(&bounds, c.x2, c.y2.clamp(Y_MIN, Y_MAX)));
                    if let Ok(arms) = arms.build() {
                        window.paint_path(arms, accent.opacity(0.6));
                    }
                    for (x, y) in [(c.x1, c.y1), (c.x2, c.y2)] {
                        let q = to_graph(&bounds, x, y.clamp(Y_MIN, Y_MAX));
                        let r = px(4.);
                        window.paint_quad(fill(
                            Bounds::new(point(q.x - r, q.y - r), size(r * 2., r * 2.)),
                            accent,
                        ));
                    }
                }
                // Follow the handle anywhere in the window while dragging.
                let (moved, released) = (weak.clone(), weak.clone());
                let (move_id, up_id) = (paint_id.clone(), paint_id.clone());
                window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        moved
                            .update(cx, |this, cx| {
                                this.curve_drag_move(&move_id, e.position, cx)
                            })
                            .ok();
                    }
                });
                window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture && e.button == MouseButton::Left {
                        released
                            .update(cx, |this, cx| this.curve_drag_end(&up_id, cx))
                            .ok();
                    }
                });
            },
        )
        .size_full();
        let down_id = id.clone();
        div()
            .id(id.clone())
            .test_support()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(menu)
            .child(
                div()
                    .id(SharedString::from(format!("{id}-graph")))
                    .test_support()
                    .relative()
                    .w(px(GRAPH))
                    .h(px(GRAPH))
                    .rounded(px(3.))
                    .border_1()
                    .border_color(line)
                    .bg(p.soft_bg)
                    .when(!disabled, |d| d.cursor(CursorStyle::Crosshair))
                    .child(plot)
                    .when(!disabled, |d| {
                        d.on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                                this.curve_drag_start(
                                    &down_id,
                                    e.position,
                                    easing,
                                    curve,
                                    change.clone(),
                                    cx,
                                );
                                cx.stop_propagation();
                            }),
                        )
                    }),
            )
            .child(mono(
                match shown_curve {
                    Some(c) => format!("Curve {:.2}, {:.2} · {:.2}, {:.2}", c.x1, c.y1, c.x2, c.y2),
                    None => "Drag the graph to shape a custom curve.".into(),
                },
                10.,
                muted,
            ))
            .into_any_element()
    }

    /// Grab the handle nearest the pointer (a preset becomes its curve).
    pub(crate) fn curve_drag_start(
        &mut self,
        id: &SharedString,
        position: Point<Pixels>,
        easing: Easing,
        curve: Option<Curve>,
        change: EaseChange,
        cx: &mut Context<Self>,
    ) {
        let Some(bounds) = self.layer_keys.curves.graphs.get(id).and_then(|g| g.get()) else {
            return;
        };
        let curve = curve.unwrap_or_else(|| preset_curve(easing));
        let distance = |x: f64, y: f64| {
            let q = to_graph(&bounds, x, y.clamp(Y_MIN, Y_MAX));
            f32::from(q.x - position.x).hypot(f32::from(q.y - position.y))
        };
        let (d1, d2) = (distance(curve.x1, curve.y1), distance(curve.x2, curve.y2));
        // Near a handle grabs it; elsewhere the nearer handle jumps there.
        let handle = usize::from(d2 < d1);
        let mut drag = CurveDrag {
            id: id.clone(),
            handle,
            easing,
            curve,
            change,
        };
        if d1.min(d2) > GRAB {
            let (x, y) = from_graph(&bounds, position);
            drag.curve = move_handle(drag.curve, handle, x, y);
        }
        self.layer_keys.curves.drag = Some(drag);
        cx.notify();
    }

    pub(crate) fn curve_drag_move(
        &mut self,
        id: &SharedString,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(bounds) = self.layer_keys.curves.graphs.get(id).and_then(|g| g.get()) else {
            return;
        };
        let Some(drag) = self.layer_keys.curves.drag.as_mut().filter(|d| &d.id == id) else {
            return;
        };
        let (x, y) = from_graph(&bounds, position);
        drag.curve = move_handle(drag.curve, drag.handle, x, y);
        cx.notify();
    }

    /// Finish the drag: the curve lands through the editor's callback.
    pub(crate) fn curve_drag_end(&mut self, id: &SharedString, cx: &mut Context<Self>) {
        if !self
            .layer_keys
            .curves
            .drag
            .as_ref()
            .is_some_and(|d| &d.id == id)
        {
            return;
        }
        let Some(drag) = self.layer_keys.curves.drag.take() else {
            return;
        };
        (drag.change)(self, drag.easing, Some(drag.curve), cx);
        cx.notify();
    }
}
