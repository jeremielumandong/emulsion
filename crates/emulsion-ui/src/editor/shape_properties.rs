//! Shape settings live in Properties, leaving the drawing bar compact.
use super::*;
use emulsion_raster::vector::{PathPaint, PatternKind, StrokeAlignment, StrokeCap, StrokeJoin};
use gpui_kit::component::button::Button;
use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{Disableable, Sizable};

pub(super) struct ShapeFields {
    target: Option<NodeId>,
    inputs: HashMap<&'static str, Entity<InputState>>,
    colors: HashMap<&'static str, Entity<ColorPickerState>>,
    _subs: Vec<Subscription>,
}
fn secondary(paint: PathPaint) -> [u8; 4] {
    match paint {
        PathPaint::Solid => [255; 4],
        PathPaint::LinearGradient { end, .. } | PathPaint::RadialGradient { end } => end,
        PathPaint::Pattern { secondary, .. } => secondary,
    }
}
fn set_secondary(paint: &mut PathPaint, color: [u8; 4]) {
    match paint {
        PathPaint::Solid => {}
        PathPaint::LinearGradient { end, .. } | PathPaint::RadialGradient { end } => *end = color,
        PathPaint::Pattern { secondary, .. } => *secondary = color,
    }
}
fn rgba(color: [u8; 4]) -> Hsla {
    Rgba {
        r: color[0] as f32 / 255.,
        g: color[1] as f32 / 255.,
        b: color[2] as f32 / 255.,
        a: color[3] as f32 / 255.,
    }
    .into()
}
fn bytes(color: Hsla) -> [u8; 4] {
    let c = color.to_rgb();
    [c.r, c.g, c.b, c.a].map(|v| (v * 255.).round().clamp(0., 255.) as u8)
}
fn paint_index(color: Option<[u8; 4]>, paint: PathPaint) -> usize {
    if color.is_none() {
        0
    } else {
        match paint {
            PathPaint::Solid => 1,
            PathPaint::LinearGradient { .. } => 2,
            PathPaint::RadialGradient { .. } => 3,
            PathPaint::Pattern { .. } => 4,
        }
    }
}
fn number(v: f64) -> String {
    format!("{:.3}", v)
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

impl EditorView {
    #[cfg(test)]
    pub(crate) fn shape_color_picker(&self, key: &'static str) -> Entity<ColorPickerState> {
        self.shape_ui
            .fields
            .as_ref()
            .expect("rendered shape properties")
            .colors[key]
            .clone()
    }
    pub(super) fn close_shape_color_pickers(&mut self, cx: &mut Context<Self>) {
        let pickers: Vec<_> = self
            .shape_ui
            .fields
            .as_ref()
            .map(|fields| fields.colors.values().cloned().collect())
            .unwrap_or_default();
        for picker in pickers {
            picker.update(cx, |picker, cx| picker.set_open(false, cx));
        }
    }

    fn begin_shape_color_edit(&mut self, target: Option<NodeId>, key: &'static str) -> bool {
        if self.shape_target() != target
            || self.drag.is_some()
            || self.assistant.running
            || self.styles_ui.dialog_for.is_some()
        {
            return false;
        }
        if self.shape_ui.color_edit == target.map(|id| (id, key))
            && self.shape_ui.color_edit.is_some()
        {
            return true;
        }
        if self.shape_ui.color_edit.is_some() {
            self.commit_shape_color_edit();
        }
        if self.editor.in_transaction() {
            return false;
        }
        if let Some(id) = target {
            if self.editor.doc.locked_ancestor(id).is_some()
                || self.editor.doc.node(id).is_some_and(|n| n.locks.pixels)
            {
                return false;
            }
            self.editor.begin("Shape color");
            self.shape_ui.color_edit = Some((id, key));
        }
        true
    }

    fn shape_target(&self) -> Option<NodeId> {
        self.selected.filter(|id| {
            self.editor
                .doc
                .node(*id)
                .is_some_and(|n| matches!(n.kind, NodeKind::Path { .. }))
        })
    }
    fn shape_fields_sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.shape_target();
        let style = self.current_shape_style();
        let bounds = target
            .and_then(|id| self.editor.doc.node(id))
            .and_then(|n| match &n.kind {
                NodeKind::Path { path, .. } => geometry::bounds(path),
                _ => None,
            });
        let (width, height) =
            bounds.map_or((self.shape_ui.width, self.shape_ui.height), |b| (b.2, b.3));
        let angle = |p| match p {
            PathPaint::LinearGradient { angle, .. } => angle,
            _ => 0.,
        };
        let size = |p| match p {
            PathPaint::Pattern { size, .. } => size,
            _ => 16.,
        };
        let values = [
            ("width", number(width)),
            ("height", number(height)),
            ("stroke-width", number(style.width as f64)),
            ("miter", number(style.miter_limit as f64)),
            ("dash-offset", number(style.dash_offset as f64)),
            (
                "dashes",
                style.dash[..style.dash_count.min(6) as usize]
                    .iter()
                    .map(|v| number(*v as f64))
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            ("fill-angle", number(angle(style.fill_paint) as f64)),
            ("stroke-angle", number(angle(style.stroke_paint) as f64)),
            ("fill-size", number(size(style.fill_paint) as f64)),
            ("stroke-size", number(size(style.stroke_paint) as f64)),
        ];
        let colors = [
            ("fill", style.fill.unwrap_or(self.tools.fg)),
            ("stroke", style.stroke.unwrap_or(self.tools.fg)),
            ("fill-secondary", secondary(style.fill_paint)),
            ("stroke-secondary", secondary(style.stroke_paint)),
        ];
        if self
            .shape_ui
            .fields
            .as_ref()
            .is_none_or(|fields| fields.target != target)
        {
            self.commit_shape_color_edit();
            let mut inputs = HashMap::new();
            let mut pickers = HashMap::new();
            let mut subs = Vec::new();
            for (key, value) in &values {
                let key = *key;
                let input = cx.new(|cx| InputState::new(window, cx).default_value(value.clone()));
                subs.push(cx.subscribe_in(
                    &input,
                    window,
                    move |this, input, event: &InputEvent, _, cx| {
                        if this.shape_target() == target
                            && matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur)
                        {
                            let text = input.read(cx).value().to_string();
                            this.apply_shape_field(key, &text, cx);
                        }
                    },
                ));
                inputs.insert(key, input);
            }
            for (key, value) in colors {
                let picker =
                    cx.new(|cx| ColorPickerState::new(window, cx).default_value(rgba(value)));
                subs.push(cx.subscribe(
                    &picker,
                    move |this, picker, event: &ColorPickerEvent, cx| {
                        if this.shape_target() != target {
                            return;
                        }
                        if let ColorPickerEvent::Change(Some(color)) = event {
                            if !this.begin_shape_color_edit(target, key) {
                                return;
                            }
                            let mut style = this.current_shape_style();
                            let color = bytes(*color);
                            match key {
                                "fill" => style.fill = Some(color),
                                "stroke" => style.stroke = Some(color),
                                "fill-secondary" => set_secondary(&mut style.fill_paint, color),
                                _ => set_secondary(&mut style.stroke_paint, color),
                            }
                            this.apply_shape_style(style, true, cx);
                            if !picker.read(cx).is_open() {
                                this.commit_shape_color_edit();
                            }
                        }
                    },
                ));
                subs.push(cx.observe(&picker, move |this, picker, cx| {
                    if this.shape_target() != target {
                        return;
                    }
                    if picker.read(cx).is_open() {
                        if !this.begin_shape_color_edit(target, key) {
                            picker.update(cx, |picker, cx| picker.set_open(false, cx));
                        }
                    } else if this.shape_ui.color_edit == target.map(|id| (id, key)) {
                        this.commit_shape_color_edit();
                        cx.notify();
                    }
                }));
                pickers.insert(key, picker);
            }
            self.shape_ui.fields = Some(ShapeFields {
                target,
                inputs,
                colors: pickers,
                _subs: subs,
            });
            self.shape_ui.error = None;
            self.shape_ui.component = None;
        } else if let Some(fields) = &self.shape_ui.fields {
            if self.shape_ui.error.is_none() {
                for (key, value) in values {
                    let input = &fields.inputs[key];
                    if !input.read(cx).focus_handle(cx).is_focused(window)
                        && input.read(cx).value().as_str() != value
                    {
                        input.update(cx, |input, cx| input.set_value(value, window, cx));
                    }
                }
            }
            for (key, value) in colors {
                let picker = &fields.colors[key];
                if picker.read(cx).value().map(bytes) != Some(value) {
                    picker.update(cx, |picker, cx| picker.set_value(rgba(value), window, cx));
                }
            }
        }
    }

    fn apply_shape_field(&mut self, key: &str, text: &str, cx: &mut Context<Self>) {
        let mut style = self.current_shape_style();
        if key == "dashes" {
            let parts: Result<Vec<f32>, _> = text
                .split([',', ' '])
                .filter(|s| !s.is_empty())
                .map(str::parse::<f32>)
                .collect();
            let Ok(parts) = parts else {
                self.shape_ui.error =
                    Some("Enter dash and gap lengths separated by commas.".into());
                cx.notify();
                return;
            };
            if parts.len() > 6
                || parts
                    .iter()
                    .any(|v| !v.is_finite() || *v < 0. || *v > 10000. || (*v > 0. && *v < 0.25))
                || (!parts.is_empty() && parts.iter().all(|v| *v == 0.))
            {
                self.shape_ui.error = Some(
                    "Use up to six lengths: 0 or 0.25–10000 px, with a nonzero gap or dash.".into(),
                );
                cx.notify();
                return;
            }
            style.dash = [0.; 6];
            style.dash[..parts.len()].copy_from_slice(&parts);
            style.dash_count = parts.len() as u8;
        } else {
            let range = match key {
                "width" | "height" => (1., 100000.),
                "stroke-width" => (0., 500.),
                "miter" => (1., 100.),
                "fill-size" | "stroke-size" => (1., 4096.),
                "fill-angle" | "stroke-angle" => (-360., 360.),
                _ => (-100000., 100000.),
            };
            let value = text
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite() && *v >= range.0 && *v <= range.1);
            let Some(value) = value else {
                self.shape_ui.error =
                    Some(format!("Enter a value from {} to {}.", range.0, range.1));
                cx.notify();
                return;
            };
            if key == "width" || key == "height" {
                self.shape_ui.error = None;
                self.resize_shape(key == "width", value, cx);
                return;
            }
            match key {
                "stroke-width" => style.width = value as f32,
                "miter" => style.miter_limit = value as f32,
                "dash-offset" => style.dash_offset = value as f32,
                "fill-angle" => {
                    if let PathPaint::LinearGradient { angle, .. } = &mut style.fill_paint {
                        *angle = value as f32;
                    }
                }
                "stroke-angle" => {
                    if let PathPaint::LinearGradient { angle, .. } = &mut style.stroke_paint {
                        *angle = value as f32;
                    }
                }
                "fill-size" => {
                    if let PathPaint::Pattern { size, .. } = &mut style.fill_paint {
                        *size = value as f32;
                    }
                }
                "stroke-size" => {
                    if let PathPaint::Pattern { size, .. } = &mut style.stroke_paint {
                        *size = value as f32;
                    }
                }
                _ => return,
            }
        }
        self.change_shape_style(style, cx);
    }

    fn shape_choice(
        &self,
        key: &'static str,
        title: &str,
        current: usize,
        labels: &[&str],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let editor = cx.entity().downgrade();
        let labels: Vec<String> = labels.iter().map(|s| s.to_string()).collect();
        let caption = format!(
            "{title}: {}",
            labels.get(current).map_or("", String::as_str)
        );
        div()
            .id(SharedString::from(format!("shape-{key}")))
            .test_support()
            .child(
                Button::new(SharedString::from(format!("shape-{key}-button")))
                    .small()
                    .label(caption)
                    .dropdown_menu(move |mut menu, _, _| {
                        for (index, label) in labels.iter().enumerate() {
                            let weak = editor.clone();
                            menu = menu.item(
                                PopupMenuItem::new(label.clone())
                                    .checked(current == index)
                                    .on_click(move |_, _, cx| {
                                        if let Some(editor) = weak.upgrade() {
                                            editor.update(cx, |this, cx| {
                                                this.choose_shape_option(key, index, cx)
                                            });
                                        }
                                    }),
                            );
                        }
                        menu
                    }),
            )
            .into_any_element()
    }

    pub(super) fn choose_shape_option(&mut self, key: &str, index: usize, cx: &mut Context<Self>) {
        let mut style = self.current_shape_style();
        match key {
            "mode" => {
                self.shape_ui.mode = [ShapeMode::Shape, ShapeMode::Path, ShapeMode::Pixels][index];
                if self.shape_ui.mode == ShapeMode::Pixels {
                    self.shape_ui.operation = ShapeOperation::NewLayer;
                }
                cx.notify();
                return;
            }
            "operation" => {
                self.shape_ui.operation = [
                    ShapeOperation::NewLayer,
                    ShapeOperation::Component,
                    ShapeOperation::Add,
                    ShapeOperation::Subtract,
                    ShapeOperation::Intersect,
                    ShapeOperation::Exclude,
                ][index];
                cx.notify();
                return;
            }
            "component" => {
                self.shape_ui.component = index.checked_sub(1);
                self.tools.pen.selected = self.shape_ui.component.map(|i| (i, 0));
                cx.notify();
                return;
            }
            "fill-type" | "stroke-type" => {
                let (color, paint) = if key == "fill-type" {
                    (&mut style.fill, &mut style.fill_paint)
                } else {
                    (&mut style.stroke, &mut style.stroke_paint)
                };
                if index == 0 {
                    *color = None;
                } else {
                    *color = Some(color.unwrap_or(self.tools.fg));
                    let end = secondary(*paint);
                    *paint = match index {
                        1 => PathPaint::Solid,
                        2 => PathPaint::LinearGradient { end, angle: 0. },
                        3 => PathPaint::RadialGradient { end },
                        _ => PathPaint::Pattern {
                            kind: PatternKind::Checker,
                            secondary: end,
                            size: 16.,
                        },
                    };
                }
            }
            "fill-pattern" | "stroke-pattern" => {
                let paint = if key == "fill-pattern" {
                    &mut style.fill_paint
                } else {
                    &mut style.stroke_paint
                };
                if let PathPaint::Pattern { kind, .. } = paint {
                    *kind = [
                        PatternKind::Checker,
                        PatternKind::Stripes,
                        PatternKind::Dots,
                    ][index];
                }
            }
            "stroke-alignment" => {
                style.alignment = [
                    StrokeAlignment::Inside,
                    StrokeAlignment::Center,
                    StrokeAlignment::Outside,
                ][index]
            }
            "cap" => style.cap = [StrokeCap::Butt, StrokeCap::Round, StrokeCap::Square][index],
            "join" => style.join = [StrokeJoin::Miter, StrokeJoin::Round, StrokeJoin::Bevel][index],
            "stroke-preset" => match index {
                0 => style.dash_count = 0,
                1 => {
                    style.dash = [12., 6., 0., 0., 0., 0.];
                    style.dash_count = 2;
                }
                2 => {
                    style.cap = StrokeCap::Round;
                    style.dash = [0., (style.width * 2.).max(2.), 0., 0., 0., 0.];
                    style.dash_count = 2;
                }
                3 => return,
                _ => {
                    if let Some(preset) = crate::app_state::settings(cx)
                        .shape_stroke_presets
                        .get(index - 4)
                    {
                        let fill = style.fill;
                        let paint = style.fill_paint;
                        style = preset.style;
                        style.fill = fill;
                        style.fill_paint = paint;
                    }
                }
            },
            _ => return,
        }
        self.change_shape_style(style, cx);
    }

    fn shape_field(&self, key: &'static str, title: &str) -> AnyElement {
        let fields = self
            .shape_ui
            .fields
            .as_ref()
            .expect("shape fields initialized");
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().flex_1().text_xs().child(title.to_string()))
            .child(
                div()
                    .id(SharedString::from(format!("shape-{key}")))
                    .w_24()
                    .test_support()
                    .child(Input::new(&fields.inputs[key]).small()),
            )
            .into_any_element()
    }
    fn shape_color(&self, key: &'static str, title: &str) -> AnyElement {
        let fields = self
            .shape_ui
            .fields
            .as_ref()
            .expect("shape fields initialized");
        div()
            .id(SharedString::from(format!("shape-{key}-color")))
            .test_support()
            .child(
                ColorPicker::new(&fields.colors[key])
                    .small()
                    .label(title.to_string()),
            )
            .into_any_element()
    }

    pub(crate) fn shape_toolbar(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mode = match self.shape_ui.mode {
            ShapeMode::Shape => 0,
            ShapeMode::Path => 1,
            ShapeMode::Pixels => 2,
        };
        let operation = match self.shape_ui.operation {
            ShapeOperation::NewLayer => 0,
            ShapeOperation::Component => 1,
            ShapeOperation::Add => 2,
            ShapeOperation::Subtract => 3,
            ShapeOperation::Intersect => 4,
            ShapeOperation::Exclude => 5,
        };
        let mut views =
            vec![self.shape_choice("mode", "Mode", mode, &["Shape", "Path", "Pixels"], cx)];
        if self.shape_ui.mode != ShapeMode::Pixels {
            views.push(self.shape_choice(
                "operation",
                "Operation",
                operation,
                &[
                    "New layer",
                    "Add component",
                    "Combine",
                    "Subtract",
                    "Intersect",
                    "Exclude",
                ],
                cx,
            ));
        }
        views.push(
            div()
                .id("shape-properties-open")
                .test_support()
                .child(
                    Button::new("shape-properties-open-button")
                        .small()
                        .label("Fill & stroke…")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.select_sidebar(SidebarTab::Properties, cx)
                        })),
                )
                .into_any_element(),
        );
        views
    }

    pub(crate) fn shape_properties(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let target = self.shape_target();
        if self.tool != Tool::Shape && target.is_none() {
            return None;
        }
        self.shape_fields_sync(window, cx);
        let style = self.current_shape_style();
        let p = theme::palette(cx);
        let mut body = div()
            .id("shape-properties")
            .test_support()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .border_b_1()
            .border_color(p.line)
            .child(label(
                if target.is_some() {
                    "Vector shape"
                } else {
                    "New shape settings"
                },
                &p,
            ));
        if target.is_some_and(|id| self.editor.doc.locked_ancestor(id).is_some()) {
            return Some(
                body.child("Unlock the shape to edit its geometry and appearance.")
                    .into_any_element(),
            );
        }
        body = body
            .child(self.shape_field("width", "Width (px)"))
            .child(self.shape_field("height", "Height (px)"))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .child(
                        div().id("shape-link-size").test_support().child(
                            Button::new("shape-link-size-button")
                                .small()
                                .label(if self.shape_ui.linked {
                                    "Proportions: linked"
                                } else {
                                    "Proportions: free"
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.shape_ui.linked = !this.shape_ui.linked;
                                    cx.notify();
                                })),
                        ),
                    )
                    .child(
                        div().id("shape-fixed-size").test_support().child(
                            Button::new("shape-fixed-size-button")
                                .small()
                                .label(if self.shape_ui.fixed_size {
                                    "Draw: fixed size"
                                } else {
                                    "Draw: drag size"
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.shape_ui.fixed_size = !this.shape_ui.fixed_size;
                                    cx.notify();
                                })),
                        ),
                    )
                    .child(
                        div().id("shape-align-edges").test_support().child(
                            Button::new("shape-align-edges-button")
                                .small()
                                .label(if self.shape_ui.align_edges {
                                    "Align edges: on"
                                } else {
                                    "Align edges: off"
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.shape_ui.align_edges = !this.shape_ui.align_edges;
                                    cx.notify();
                                })),
                        ),
                    ),
            );
        for (stroke, color, paint) in [
            (false, style.fill, style.fill_paint),
            (true, style.stroke, style.stroke_paint),
        ] {
            body = body.child(self.shape_choice(
                if stroke { "stroke-type" } else { "fill-type" },
                if stroke { "Stroke" } else { "Fill" },
                paint_index(color, paint),
                &[
                    "None",
                    "Solid",
                    "Linear gradient",
                    "Radial gradient",
                    "Pattern",
                ],
                cx,
            ));
            if color.is_some() {
                body = body.child(self.shape_color(
                    if stroke { "stroke" } else { "fill" },
                    if matches!(paint, PathPaint::Solid) {
                        "Color"
                    } else {
                        "Start color"
                    },
                ));
                if !matches!(paint, PathPaint::Solid) {
                    body = body.child(self.shape_color(
                        if stroke {
                            "stroke-secondary"
                        } else {
                            "fill-secondary"
                        },
                        "End / pattern color",
                    ));
                }
                if matches!(paint, PathPaint::LinearGradient { .. }) {
                    body = body.child(self.shape_field(
                        if stroke { "stroke-angle" } else { "fill-angle" },
                        "Angle (°)",
                    ));
                }
                if let PathPaint::Pattern { kind, .. } = paint {
                    let index = match kind {
                        PatternKind::Checker => 0,
                        PatternKind::Stripes => 1,
                        PatternKind::Dots => 2,
                    };
                    body = body
                        .child(self.shape_choice(
                            if stroke {
                                "stroke-pattern"
                            } else {
                                "fill-pattern"
                            },
                            "Pattern",
                            index,
                            &["Checker", "Stripes", "Dots"],
                            cx,
                        ))
                        .child(self.shape_field(
                            if stroke { "stroke-size" } else { "fill-size" },
                            "Pattern size (px)",
                        ));
                }
            }
        }
        if style.stroke.is_some() {
            body = body
                .child(self.shape_field("stroke-width", "Stroke width (px)"))
                .child(self.shape_choice(
                    "stroke-alignment",
                    "Align",
                    match style.alignment {
                        StrokeAlignment::Inside => 0,
                        StrokeAlignment::Center => 1,
                        StrokeAlignment::Outside => 2,
                    },
                    &["Inside", "Center", "Outside"],
                    cx,
                ))
                .child(self.shape_choice(
                    "cap",
                    "Caps",
                    match style.cap {
                        StrokeCap::Butt => 0,
                        StrokeCap::Round => 1,
                        StrokeCap::Square => 2,
                    },
                    &["Butt", "Round", "Projecting"],
                    cx,
                ))
                .child(self.shape_choice(
                    "join",
                    "Corners",
                    match style.join {
                        StrokeJoin::Miter => 0,
                        StrokeJoin::Round => 1,
                        StrokeJoin::Bevel => 2,
                    },
                    &["Miter", "Round", "Bevel"],
                    cx,
                ));
            if style.join == StrokeJoin::Miter {
                body = body.child(self.shape_field("miter", "Miter limit"));
            }
            let preset_index = if style.dash_count == 0 {
                0
            } else if style.dash_count == 2 && style.dash[..2] == [12., 6.] {
                1
            } else if style.dash_count == 2 && style.dash[0] == 0. && style.cap == StrokeCap::Round
            {
                2
            } else {
                3
            };
            let mut presets = vec![
                "Solid".to_string(),
                "Dashed".into(),
                "Dotted".into(),
                "Custom".into(),
            ];
            presets.extend(
                crate::app_state::settings(cx)
                    .shape_stroke_presets
                    .iter()
                    .map(|p| p.name.clone()),
            );
            body = body
                .child(self.shape_choice(
                    "stroke-preset",
                    "Stroke preset",
                    preset_index,
                    &presets.iter().map(String::as_str).collect::<Vec<_>>(),
                    cx,
                ))
                .child(self.shape_field("dashes", "Dash, gap (px)"))
                .child(self.shape_field("dash-offset", "Dash offset (px)"))
                .child(
                    div().id("shape-save-stroke").test_support().child(
                        Button::new("shape-save-stroke-button")
                            .small()
                            .label("Save stroke preset")
                            .disabled(
                                crate::app_state::settings(cx).shape_stroke_presets.len() >= 32,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                let style = this.current_shape_style();
                                crate::app_state::update_settings(cx, |settings| {
                                    if settings.shape_stroke_presets.len() < 32 {
                                        settings.shape_stroke_presets.push(
                                            emulsion_io::settings::ShapeStrokePreset {
                                                name: format!(
                                                    "Stroke {}",
                                                    settings.shape_stroke_presets.len() + 1
                                                ),
                                                style,
                                            },
                                        );
                                    }
                                });
                                cx.notify();
                            })),
                    ),
                );
        }
        if let Some((_, path, _)) = self.pen_target()
            && path.subpaths.len() > 1
        {
            let mut labels = vec!["All components".to_string()];
            labels.extend((1..=path.subpaths.len()).map(|i| format!("Component {i}")));
            body = body
                .child(label("Path alignment", &p))
                .child(self.shape_choice(
                    "component",
                    "Target",
                    self.shape_ui.component.map_or(0, |i| i + 1),
                    &labels.iter().map(String::as_str).collect::<Vec<_>>(),
                    cx,
                ));
            let mut row = div().flex().flex_wrap().gap_1();
            for (id, label, axis, pos, distribute) in [
                ("left", "Left", 0, 0, false),
                ("center-x", "Center X", 0, 1, false),
                ("right", "Right", 0, 2, false),
                ("top", "Top", 1, 0, false),
                ("center-y", "Center Y", 1, 1, false),
                ("bottom", "Bottom", 1, 2, false),
                ("distribute-x", "Distribute X", 0, 0, true),
                ("distribute-y", "Distribute Y", 1, 0, true),
            ] {
                row = row.child(
                    div()
                        .id(SharedString::from(format!("shape-align-{id}")))
                        .test_support()
                        .child(
                            Button::new(SharedString::from(format!("shape-align-{id}-button")))
                                .small()
                                .label(label)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.align_shape_components(axis, pos, distribute, cx)
                                })),
                        ),
                );
            }
            body = body.child(row);
        }
        if let Some(error) = &self.shape_ui.error {
            body = body.child(div().text_xs().text_color(p.accent).child(error.clone()));
        }
        Some(body.into_any_element())
    }
}
