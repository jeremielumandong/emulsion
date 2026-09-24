//! Differential integration coverage for experimental cross-frame layout reuse.
use gpui_kit::{
    App, AppContext as _, Bounds, Context, Element, ElementId, Entity, GlobalElementId,
    InspectorElementId, InteractiveElement as _, IntoElement, LayoutId, Modifiers, MouseButton,
    ParentElement as _, Pixels, Render, Style, Styled as _, TestAppContext, VisualTestContext,
    Window, canvas, div, px, rems, size,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

type Geometry = Vec<(usize, Bounds<Pixels>)>;

#[derive(Clone, Default)]
struct Observations {
    geometry: Rc<RefCell<Geometry>>,
    clicks: Rc<RefCell<Vec<usize>>>,
    live_callbacks: Rc<Cell<usize>>,
    measured_paints: Rc<Cell<usize>>,
}

struct LayoutFixture {
    order: Vec<usize>,
    nested: bool,
    width: f32,
    text: Option<String>,
    measured: bool,
    revision: usize,
    observations: Observations,
}

impl Render for LayoutFixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.observations.geometry.borrow_mut().clear();
        let mut rows = Vec::new();
        for &index in &self.order {
            let geometry = self.observations.geometry.clone();
            let clicks = self.observations.clicks.clone();
            rows.push(
                div()
                    .id(("layout-row", index))
                    .relative()
                    .w(rems(self.width))
                    .h(px(24. + index as f32))
                    .flex_shrink_0()
                    .on_mouse_down(MouseButton::Left, move |_, _, _| {
                        clicks.borrow_mut().push(index);
                    })
                    .child(
                        canvas(
                            move |bounds, _, _| geometry.borrow_mut().push((index, bounds)),
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .into_any_element(),
            );
        }
        if let Some(text) = &self.text {
            let geometry = self.observations.geometry.clone();
            rows.push(
                div()
                    .relative()
                    .w(rems(self.width))
                    .text_size(rems(1.))
                    .child(text.clone())
                    .child(
                        canvas(
                            move |bounds, _, _| geometry.borrow_mut().push((900, bounds)),
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .into_any_element(),
            );
        }
        if self.measured {
            rows.push(
                MeasuredProbe {
                    revision: self.revision,
                    observations: self.observations.clone(),
                }
                .into_any_element(),
            );
        }
        let body = if self.nested {
            div()
                .flex()
                .flex_col()
                .ml_4()
                .child(div().flex().flex_col().gap_1().children(rows))
        } else {
            div().flex().flex_col().gap_1().children(rows)
        };
        let geometry = self.observations.geometry.clone();
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .p_2()
            .child(body)
            .child(
                canvas(
                    move |bounds, _, _| geometry.borrow_mut().push((999, bounds)),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
    }
}

struct CallbackLifetime(Rc<Cell<usize>>);

impl Drop for CallbackLifetime {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

struct MeasuredProbe {
    revision: usize,
    observations: Observations,
}

impl IntoElement for MeasuredProbe {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for MeasuredProbe {
    type RequestLayoutState = Rc<Cell<Option<usize>>>;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        _: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let state = Rc::new(Cell::new(None));
        let measured_state = state.clone();
        let revision = self.revision;
        let count = self.observations.live_callbacks.clone();
        count.set(count.get() + 1);
        let lifetime = CallbackLifetime(count);
        let id = window.request_measured_layout(Style::default(), move |_, _, _, _| {
            // The closure must initialize this frame's paint state, even when
            // the node's style and available space have not changed.
            let _keep_alive = &lifetime;
            measured_state.set(Some(revision));
            size(px(60. + revision as f32), px(20. + revision as f32))
        });
        (id, state)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Window,
        _: &mut App,
    ) {
        self.observations.geometry.borrow_mut().push((1000, bounds));
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
        assert_eq!(state.get(), Some(self.revision));
        let count = &self.observations.measured_paints;
        count.set(count.get() + 1);
    }
}

fn open_fixture(
    cx: &mut TestAppContext,
    reuse: bool,
) -> (Entity<LayoutFixture>, Observations, &mut VisualTestContext) {
    let observations = Observations::default();
    let observed = observations.clone();
    let (view, cx) = cx.add_window_view(move |window, _| {
        window.set_layout_reuse_enabled(reuse);
        LayoutFixture {
            order: vec![0, 1, 2],
            nested: false,
            width: 12.,
            text: None,
            measured: false,
            revision: 0,
            observations: observed,
        }
    });
    (view, observations, cx)
}

fn frame(
    view: &Entity<LayoutFixture>,
    observations: &Observations,
    cx: &mut VisualTestContext,
    change: impl FnOnce(&mut LayoutFixture),
) -> Geometry {
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            change(view);
            cx.notify();
        })
    });
    cx.run_until_parked();
    let mut geometry = observations.geometry.borrow().clone();
    geometry.sort_by_key(|(index, _)| *index);
    assert!(!geometry.is_empty());
    geometry
}

#[gpui_kit::test]
fn layout_reuse_repeated_frames_match_fresh_geometry(cx: &mut TestAppContext) {
    let mut runs = Vec::new();
    for reuse in [false, true] {
        let (view, observed, cx) = open_fixture(cx, reuse);
        let frames: Vec<_> = (0..4)
            .map(|_| frame(&view, &observed, cx, |_| {}))
            .collect();
        let stats = cx.update(|window, _| window.layout_reuse_stats());
        if reuse {
            assert!(stats.nodes_reused > 0);
            assert_eq!(stats.nodes_created, 0);
        } else {
            assert_eq!(stats.nodes_reused, 0);
        }
        runs.push(frames);
    }
    assert_eq!(runs[0], runs[1]);
}

#[gpui_kit::test]
fn layout_reuse_topology_and_pointer_targets_match_fresh_frames(cx: &mut TestAppContext) {
    let mut runs = Vec::new();
    for reuse in [false, true] {
        let (view, observed, cx) = open_fixture(cx, reuse);
        let mut frames = Vec::new();
        for (nested, order) in [
            (false, vec![0, 1, 2]),
            (true, vec![3, 0, 2, 1]),
            (false, vec![2]),
            (true, vec![1, 0]),
            (false, vec![0, 1, 2]),
        ] {
            let geometry = frame(&view, &observed, cx, |view| {
                view.nested = nested;
                view.order = order.clone();
            });
            for index in order {
                let bounds = geometry.iter().find(|(id, _)| *id == index).unwrap().1;
                let clicks_before = observed.clicks.borrow().len();
                cx.simulate_mouse_down(bounds.center(), MouseButton::Left, Modifiers::none());
                cx.simulate_mouse_up(bounds.center(), MouseButton::Left, Modifiers::none());
                assert_eq!(observed.clicks.borrow().len(), clicks_before + 1);
                assert_eq!(observed.clicks.borrow().last(), Some(&index));
            }
            frames.push(geometry);
        }
        runs.push((frames, observed.clicks.borrow().clone()));
    }
    assert_eq!(runs[0], runs[1]);
}

#[gpui_kit::test]
fn layout_reuse_resize_rem_and_text_changes_match_fresh_frames(cx: &mut TestAppContext) {
    let mut runs = Vec::new();
    for reuse in [false, true] {
        let (view, observed, cx) = open_fixture(cx, reuse);
        let mut frames = Vec::new();
        for (width, rem, scale, text) in [
            (500., 16., 1., "Short text"),
            (
                300.,
                20.,
                1.25,
                "A longer sentence that wraps across several lines in a narrow column.",
            ),
            (700., 12., 1.5, "Changed text"),
            (500., 16., 1., "Short text"),
        ] {
            cx.simulate_scale_factor_change(scale);
            cx.simulate_resize(size(px(width), px(450.)));
            cx.update(|window, _| window.set_rem_size(px(rem)));
            frames.push(frame(&view, &observed, cx, |view| {
                view.width = if rem == 20. { 7. } else { 12. };
                view.text = Some(text.into());
            }));
        }
        assert_ne!(frames[0], frames[1]);
        runs.push(frames);
    }
    assert_eq!(runs[0], runs[1]);
}

#[gpui_kit::test]
fn layout_reuse_measurements_initialize_fresh_paint_state_and_release_captures(
    cx: &mut TestAppContext,
) {
    let mut runs = Vec::new();
    for reuse in [false, true] {
        let (view, observed, cx) = open_fixture(cx, reuse);
        let mut frames = Vec::new();
        for revision in [0, 1, 1, 7, 0] {
            let previous = observed.measured_paints.get();
            frames.push(frame(&view, &observed, cx, |view| {
                view.measured = true;
                view.revision = revision;
            }));
            assert!(observed.measured_paints.get() > previous);
            assert_eq!(
                observed.live_callbacks.get(),
                0,
                "frame retained a measurement capture"
            );
        }
        frames.push(frame(&view, &observed, cx, |view| view.measured = false));
        assert_eq!(observed.live_callbacks.get(), 0);
        runs.push(frames);
    }
    assert_eq!(runs[0], runs[1]);
}

#[gpui_kit::test]
fn layout_reuse_retired_nodes_are_pruned_after_large_frames(cx: &mut TestAppContext) {
    let (view, observed, cx) = open_fixture(cx, true);
    let initial = frame(&view, &observed, cx, |_| {});
    let small = cx.update(|window, _| window.layout_reuse_stats().retained_nodes);
    frame(&view, &observed, cx, |view| view.order = (0..150).collect());
    let large = cx.update(|window, _| window.layout_reuse_stats().retained_nodes);
    assert!(large > small);
    let final_frame = frame(&view, &observed, cx, |view| view.order = vec![0, 1, 2]);
    let final_count = cx.update(|window, _| window.layout_reuse_stats().retained_nodes);
    assert_eq!(final_count, small);
    assert_eq!(final_frame, initial);
}

#[gpui_kit::test]
fn layout_reuse_runtime_switch_preserves_geometry(cx: &mut TestAppContext) {
    let (view, observed, cx) = open_fixture(cx, false);
    let baseline = frame(&view, &observed, cx, |_| {});
    for enabled in [true, false, true, false] {
        cx.update(|window, _| window.set_layout_reuse_enabled(enabled));
        for _ in 0..2 {
            assert_eq!(frame(&view, &observed, cx, |_| {}), baseline);
        }
        let stats = cx.update(|window, _| window.layout_reuse_stats());
        if enabled {
            assert!(stats.nodes_reused > 0);
        } else {
            assert_eq!(stats.nodes_reused, 0);
        }
    }
}

struct RootSwitch {
    nested: bool,
    bounds: Rc<Cell<Bounds<Pixels>>>,
}

impl Render for RootSwitch {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let bounds = self.bounds.clone();
        let child = canvas(move |value, _, _| bounds.set(value), |_, _, _, _| {})
            .w(px(50.))
            .h(px(20.));
        if self.nested {
            div().p_4().child(child).into_any_element()
        } else {
            child.into_any_element()
        }
    }
}

#[gpui_kit::test]
fn layout_reuse_previous_child_can_become_an_independent_root(cx: &mut TestAppContext) {
    let mut runs = Vec::new();
    for reuse in [false, true] {
        let bounds = Rc::new(Cell::new(Bounds::default()));
        let recorded = bounds.clone();
        let (view, cx) = cx.add_window_view(move |window, _| {
            window.set_layout_reuse_enabled(reuse);
            RootSwitch {
                nested: true,
                bounds: recorded,
            }
        });
        let mut frames = Vec::new();
        for nested in [true, false, true, false] {
            cx.update(|_, cx| {
                view.update(cx, |view, cx| {
                    view.nested = nested;
                    cx.notify();
                })
            });
            cx.run_until_parked();
            if !nested {
                assert_eq!(bounds.get().origin, gpui_kit::point(px(0.), px(0.)));
                assert_eq!(bounds.get().size, size(px(50.), px(20.)));
            }
            frames.push(bounds.get());
        }
        runs.push(frames);
    }
    assert_eq!(runs[0], runs[1]);
}

struct CachedChild {
    renders: Rc<Cell<usize>>,
    bounds: Rc<Cell<Bounds<Pixels>>>,
}

impl Render for CachedChild {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.renders.set(self.renders.get() + 1);
        let bounds = self.bounds.clone();
        div()
            .size_full()
            .child(canvas(move |value, _, _| bounds.set(value), |_, _, _, _| {}).size_full())
    }
}

struct CachedHost {
    child: Entity<CachedChild>,
    offset: f32,
}

impl Render for CachedHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let mut style = div().w(px(120.)).h(px(45.)).flex_none();
        div()
            .flex()
            .size_full()
            .child(div().w(px(self.offset)).h(px(45.)).flex_none())
            .child(self.child.clone().cached(style.style().clone()))
    }
}

#[gpui_kit::test]
fn layout_reuse_coexists_with_cached_view_hits_and_misses(cx: &mut TestAppContext) {
    let mut runs = Vec::new();
    for reuse in [false, true] {
        let renders = Rc::new(Cell::new(0));
        let bounds = Rc::new(Cell::new(Bounds::default()));
        let counted = renders.clone();
        let recorded = bounds.clone();
        let (host, cx) = cx.add_window_view(move |window, cx| {
            window.set_layout_reuse_enabled(reuse);
            CachedHost {
                child: cx.new(|_| CachedChild {
                    renders: counted,
                    bounds: recorded,
                }),
                offset: 0.,
            }
        });
        let mut frames = Vec::new();
        for offset in [0., 0., 25., 25., 0., 0.] {
            let (previous_offset, before) =
                cx.update(|_, cx| (host.read(cx).offset, renders.get()));
            cx.update(|_, cx| {
                host.update(cx, |host, cx| {
                    host.offset = offset;
                    cx.notify();
                })
            });
            cx.run_until_parked();
            if offset == previous_offset {
                assert_eq!(renders.get(), before, "unchanged child cache missed");
            } else {
                assert!(renders.get() > before, "changed bounds reused stale view");
            }
            assert_eq!(bounds.get().origin.x, px(offset));
            frames.push(bounds.get());
        }
        runs.push(frames);
    }
    assert_eq!(runs[0], runs[1]);
}

#[derive(Clone, Copy, Debug)]
enum TextPolicy {
    Nowrap,
    Wrap,
    Truncate,
    Clamp,
    Plain,
    Opaque,
}

struct IntrinsicTextFixture {
    policy: TextPolicy,
    text: String,
    color: u32,
    font: gpui_kit::Font,
    font_size: f32,
    width: f32,
    latest: Rc<RefCell<Option<gpui_kit::TextLayout>>>,
    probe_observations: Observations,
}

impl Render for IntrinsicTextFixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        *self.latest.borrow_mut() = None;
        let content = match self.policy {
            TextPolicy::Plain => canvas(|_, _, _| {}, |_, _, _, _| {})
                .w(px(64.))
                .h(px(20.))
                .into_any_element(),
            TextPolicy::Opaque => MeasuredProbe {
                revision: 4,
                observations: self.probe_observations.clone(),
            }
            .into_any_element(),
            _ => {
                let text = gpui_kit::StyledText::new(self.text.clone()).with_runs(vec![
                    gpui_kit::TextRun {
                        len: self.text.len(),
                        font: self.font.clone(),
                        color: gpui_kit::rgb(0xffffff).into(),
                        background_color: Some(gpui_kit::rgb(self.color).into()),
                        ..Default::default()
                    },
                ]);
                *self.latest.borrow_mut() = Some(text.layout().clone());
                text.into_any_element()
            }
        };
        let mut body = div().w(px(self.width)).text_size(px(self.font_size));
        body = match self.policy {
            TextPolicy::Wrap => body.whitespace_normal(),
            TextPolicy::Truncate => body.truncate(),
            TextPolicy::Clamp => body.whitespace_nowrap().line_clamp(1),
            _ => body.whitespace_nowrap(),
        };
        div().size_full().child(body.child(content))
    }
}

#[derive(Debug, PartialEq)]
struct ShapedTextSnapshot {
    bounds: Bounds<Pixels>,
    len: usize,
    line_height: Pixels,
    line_widths: Vec<Pixels>,
    font_sizes: Vec<Pixels>,
    wrap_counts: Vec<usize>,
    glyphs: Vec<(usize, u32, gpui_kit::Point<Pixels>, usize)>,
}

fn text_snapshot(latest: &Rc<RefCell<Option<gpui_kit::TextLayout>>>) -> Option<ShapedTextSnapshot> {
    let latest = latest.borrow();
    let layout = latest.as_ref()?;
    let lines = layout.line_layouts();
    Some(ShapedTextSnapshot {
        bounds: layout.bounds(),
        len: layout.len(),
        line_height: layout.line_height(),
        line_widths: lines.iter().map(|line| line.width()).collect(),
        font_sizes: lines
            .iter()
            .map(|line| line.unwrapped_layout.font_size)
            .collect(),
        wrap_counts: lines
            .iter()
            .map(|line| line.wrap_boundaries.len())
            .collect(),
        glyphs: lines
            .iter()
            .flat_map(|line| {
                line.unwrapped_layout.runs.iter().flat_map(|run| {
                    run.glyphs
                        .iter()
                        .map(|glyph| (run.font_id.0, glyph.id.0, glyph.position, glyph.index))
                })
            })
            .collect(),
    })
}

fn open_intrinsic_text(
    cx: &mut TestAppContext,
    reuse: bool,
    specialize: bool,
) -> (Entity<IntrinsicTextFixture>, &mut VisualTestContext) {
    cx.add_window_view(move |window, _| {
        window.set_layout_reuse_enabled(reuse);
        window.set_intrinsic_text_layout_reuse(specialize);
        IntrinsicTextFixture {
            policy: TextPolicy::Nowrap,
            text: "A stable label".into(),
            color: 0x274e83,
            font: gpui_kit::Font::default(),
            font_size: 16.,
            width: 180.,
            latest: Rc::default(),
            probe_observations: Observations::default(),
        }
    })
}

fn intrinsic_text_frame(
    view: &Entity<IntrinsicTextFixture>,
    cx: &mut VisualTestContext,
    change: impl FnOnce(&mut IntrinsicTextFixture),
) -> Option<ShapedTextSnapshot> {
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            change(view);
            cx.notify();
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let fixture = view.read(cx);
        let snapshot = text_snapshot(&fixture.latest);
        if snapshot.as_ref().is_some_and(|snapshot| snapshot.len > 0) {
            let color: gpui_kit::Hsla = gpui_kit::rgb(fixture.color).into();
            assert!(
                window
                    .painted_quads()
                    .iter()
                    .any(|quad| quad.background == color.into()),
                "the current styled text background was not painted"
            );
        }
        assert_eq!(fixture.probe_observations.live_callbacks.get(), 0);
        snapshot
    })
}

#[gpui_kit::test]
fn layout_reuse_intrinsic_text_refreshes_glyphs_and_decorations(cx: &mut TestAppContext) {
    let mut runs = Vec::new();
    for (reuse, specialize) in [(false, false), (true, false), (true, true)] {
        let (view, cx) = open_intrinsic_text(cx, reuse, specialize);
        let mut frames = Vec::new();
        for (text, color) in [
            ("A stable label", 0x274e83),
            ("A stable label", 0x9a3214),
            ("A different label", 0x197442),
            ("A different label", 0x274e83),
            ("First line\nSecond line", 0x274e83),
            ("", 0x274e83),
            ("Restored", 0x274e83),
        ] {
            let frame = intrinsic_text_frame(&view, cx, |view| {
                view.text = text.into();
                view.color = color;
            })
            .unwrap();
            assert_eq!(frame.len, text.len());
            if reuse
                && specialize
                && (color == 0x9a3214 || text == "A different label" && color == 0x274e83)
            {
                let stats = cx.update(|window, _| window.layout_reuse_stats());
                assert!(stats.intrinsic_nodes > 0);
                assert_eq!(stats.measurement_calls, 0);
                assert!(
                    stats.intrinsic_reused > 0,
                    "color-only change dirtied intrinsic layout"
                );
            }
            frames.push(frame);
        }
        assert_eq!(
            frames[0], frames[1],
            "color must not change geometry or glyphs"
        );
        assert_ne!(frames[1].glyphs, frames[2].glyphs);
        runs.push(frames);
    }
    assert_eq!(runs[0], runs[1]);
    assert_eq!(runs[1], runs[2]);
}

#[gpui_kit::test]
fn layout_reuse_intrinsic_text_font_size_and_dpi_match_cold_layout(cx: &mut TestAppContext) {
    let mut runs = Vec::new();
    for (reuse, specialize) in [(false, false), (true, false), (true, true)] {
        let (view, cx) = open_intrinsic_text(cx, reuse, specialize);
        let mut frames = Vec::new();
        for (scale, font_size, weight, style) in [
            (
                1.,
                16.,
                gpui_kit::FontWeight::NORMAL,
                gpui_kit::FontStyle::Normal,
            ),
            (
                1.,
                16.,
                gpui_kit::FontWeight::BOLD,
                gpui_kit::FontStyle::Italic,
            ),
            (
                1.25,
                19.,
                gpui_kit::FontWeight::NORMAL,
                gpui_kit::FontStyle::Normal,
            ),
            (
                1.5,
                13.,
                gpui_kit::FontWeight::BOLD,
                gpui_kit::FontStyle::Normal,
            ),
            (
                1.,
                16.,
                gpui_kit::FontWeight::NORMAL,
                gpui_kit::FontStyle::Normal,
            ),
        ] {
            cx.simulate_scale_factor_change(scale);
            frames.push(
                intrinsic_text_frame(&view, cx, |view| {
                    view.font_size = font_size;
                    view.font.weight = weight;
                    view.font.style = style;
                })
                .unwrap(),
            );
        }
        assert_eq!(frames.first(), frames.last());
        // TestAppContext uses a mock text system: bold/italic resolve to the
        // same font and glyph IDs. Keep their cross-mode parity coverage below;
        // font size and DPI do affect mock measurements and fresh paint state.
        assert_ne!(frames[0].font_sizes, frames[2].font_sizes);
        assert_ne!(frames[0].glyphs, frames[2].glyphs);
        assert_ne!(frames[0].bounds, frames[2].bounds);
        runs.push(frames);
    }
    assert_eq!(runs[0], runs[1]);
    assert_eq!(runs[1], runs[2]);
}

#[gpui_kit::test]
fn layout_reuse_intrinsic_text_transitions_preserve_measurement_and_paint(cx: &mut TestAppContext) {
    let mut runs = Vec::new();
    for (reuse, specialize) in [(false, false), (true, false), (true, true)] {
        let (view, cx) = open_intrinsic_text(cx, reuse, specialize);
        let mut frames = Vec::new();
        for (policy, width) in [
            (TextPolicy::Nowrap, 95.),
            (TextPolicy::Wrap, 95.),
            (TextPolicy::Wrap, 150.),
            (TextPolicy::Truncate, 75.),
            (TextPolicy::Truncate, 140.),
            (TextPolicy::Clamp, 95.),
            (TextPolicy::Plain, 95.),
            (TextPolicy::Nowrap, 95.),
            (TextPolicy::Opaque, 95.),
            (TextPolicy::Nowrap, 95.),
        ] {
            frames.push(intrinsic_text_frame(&view, cx, |view| {
                view.policy = policy;
                view.width = width;
                view.text = "Several words that need wrapping or truncation".into();
            }));
            if reuse && specialize {
                let stats = cx.update(|window, _| window.layout_reuse_stats());
                match policy {
                    TextPolicy::Nowrap => assert!(stats.intrinsic_nodes > 0),
                    _ => assert_eq!(stats.intrinsic_nodes, 0),
                }
            }
        }
        assert!(
            frames[1]
                .as_ref()
                .unwrap()
                .wrap_counts
                .iter()
                .any(|count| *count > 0)
        );
        assert!(frames[3].as_ref().unwrap().len < frames[0].as_ref().unwrap().len);
        assert_eq!(frames[0], frames[7]);
        assert_eq!(frames[7], frames[9]);
        assert!(frames[6].is_none() && frames[8].is_none());
        runs.push(frames);
    }
    assert_eq!(runs[0], runs[1]);
    assert_eq!(runs[1], runs[2]);
}
