//! Native vector operations with numeric controls and transactional Apply.
use super::*;
use emulsion_core::design_vectors::{self as vectors, Combine, MatchProperty};
use gpui_kit::component::{Selectable, Sizable, WindowExt, button::Button};
#[derive(Clone, Copy, PartialEq)]
enum Action {
    Point,
    Join,
    Split,
    Skew,
    Envelope,
    Perspective,
}
impl Action {
    fn label(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::Point => t!("editor.design_vector_ui.anchor_handles"),
            Self::Join => t!("editor.design_vector_ui.join"),
            Self::Split => t!("editor.design_vector_ui.split"),
            Self::Skew => t!("editor.design_vector_ui.skew"),
            Self::Envelope => t!("editor.design_vector_ui.envelope"),
            Self::Perspective => t!("editor.design_vector_ui.perspective"),
        }
    }
}
/// Localized display name for a numeric field; the English key stays the lookup id.
fn field_label(key: &str) -> std::borrow::Cow<'static, str> {
    match key {
        "Subpath" => t!("editor.design_vector_ui.field_subpath"),
        "Anchor" => t!("editor.design_vector_ui.field_anchor"),
        "Second subpath" => t!("editor.design_vector_ui.field_second_subpath"),
        "Incoming X" => t!("editor.design_vector_ui.field_incoming_x"),
        "Incoming Y" => t!("editor.design_vector_ui.field_incoming_y"),
        "Outgoing X" => t!("editor.design_vector_ui.field_outgoing_x"),
        "Outgoing Y" => t!("editor.design_vector_ui.field_outgoing_y"),
        "Horizontal degrees" => t!("editor.design_vector_ui.field_horizontal_degrees"),
        "Vertical degrees" => t!("editor.design_vector_ui.field_vertical_degrees"),
        "Origin X" => t!("editor.design_vector_ui.field_origin_x"),
        "Origin Y" => t!("editor.design_vector_ui.field_origin_y"),
        "Top left X" => t!("editor.design_vector_ui.field_top_left_x"),
        "Top left Y" => t!("editor.design_vector_ui.field_top_left_y"),
        "Top right X" => t!("editor.design_vector_ui.field_top_right_x"),
        "Top right Y" => t!("editor.design_vector_ui.field_top_right_y"),
        "Bottom left X" => t!("editor.design_vector_ui.field_bottom_left_x"),
        "Bottom left Y" => t!("editor.design_vector_ui.field_bottom_left_y"),
        "Bottom right X" => t!("editor.design_vector_ui.field_bottom_right_x"),
        "Bottom right Y" => t!("editor.design_vector_ui.field_bottom_right_y"),
        "Sampling tolerance" => t!("editor.design_vector_ui.field_sampling_tolerance"),
        _ => std::borrow::Cow::Owned(key.to_owned()),
    }
}
struct Form {
    action: Action,
    fields: Vec<(&'static str, Entity<InputState>)>,
}
impl Form {
    fn value(&self, key: &str, cx: &App) -> Result<f64, String> {
        self.fields
            .iter()
            .find(|(k, _)| *k == key)
            .unwrap()
            .1
            .read(cx)
            .value()
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| {
                t!(
                    "editor.design_vector_ui.enter_finite",
                    field = field_label(key)
                )
                .into_owned()
            })
    }
    fn index(&self, key: &str, cx: &App) -> Result<usize, String> {
        let v = self.value(key, cx)?;
        if v < 0. || v.fract() != 0. || v > 20000. {
            return Err(t!(
                "editor.design_vector_ui.nonnegative_integer",
                field = field_label(key)
            )
            .into_owned());
        }
        Ok(v as usize)
    }
    fn apply(&self, e: &mut emulsion_core::Editor, id: NodeId, cx: &App) -> Result<(), String> {
        let point = |x, y| Ok::<_, String>((self.value(x, cx)?, self.value(y, cx)?));
        match self.action {
            Action::Point => vectors::point(
                e,
                id,
                self.index("Subpath", cx)?,
                self.index("Anchor", cx)?,
                point("X", "Y")?,
                Some(point("Incoming X", "Incoming Y")?),
                Some(point("Outgoing X", "Outgoing Y")?),
                false,
            ),
            Action::Join => vectors::join(
                e,
                id,
                self.index("Subpath", cx)?,
                self.index("Second subpath", cx)?,
            ),
            Action::Split => {
                vectors::split(e, id, self.index("Subpath", cx)?, self.index("Anchor", cx)?)
            }
            Action::Skew => vectors::skew(
                e,
                id,
                self.value("Horizontal degrees", cx)?,
                self.value("Vertical degrees", cx)?,
                point("Origin X", "Origin Y")?,
            ),
            Action::Perspective => vectors::perspective(
                e,
                id,
                [
                    point("Top left X", "Top left Y")?,
                    point("Top right X", "Top right Y")?,
                    point("Bottom right X", "Bottom right Y")?,
                    point("Bottom left X", "Bottom left Y")?,
                ],
                self.value("Sampling tolerance", cx)?,
            ),
            Action::Envelope => vectors::mesh(
                e,
                id,
                2,
                2,
                &[
                    point("Top left X", "Top left Y")?,
                    point("Top right X", "Top right Y")?,
                    point("Bottom left X", "Bottom left Y")?,
                    point("Bottom right X", "Bottom right Y")?,
                ],
                self.value("Sampling tolerance", cx)?,
            ),
        }
    }
}
impl Render for Form {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let keys: &[&str] = match self.action {
            Action::Point => &[
                "Subpath",
                "Anchor",
                "X",
                "Y",
                "Incoming X",
                "Incoming Y",
                "Outgoing X",
                "Outgoing Y",
            ],
            Action::Join => &["Subpath", "Second subpath"],
            Action::Split => &["Subpath", "Anchor"],
            Action::Skew => &[
                "Horizontal degrees",
                "Vertical degrees",
                "Origin X",
                "Origin Y",
            ],
            Action::Envelope | Action::Perspective => &[
                "Top left X",
                "Top left Y",
                "Top right X",
                "Top right Y",
                "Bottom left X",
                "Bottom left Y",
                "Bottom right X",
                "Bottom right Y",
                "Sampling tolerance",
            ],
        };
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div().flex().flex_wrap().gap_1().children(
                    [
                        Action::Point,
                        Action::Join,
                        Action::Split,
                        Action::Skew,
                        Action::Envelope,
                        Action::Perspective,
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(i, a)| {
                        Button::new(("vector-action", i))
                            .small()
                            .outline()
                            .label(a.label())
                            .selected(self.action == a)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.action = a;
                                cx.notify();
                            }))
                    }),
                ),
            )
            .children(
                self.fields
                    .iter()
                    .filter(|(key, _)| keys.contains(key))
                    .enumerate()
                    .map(|(i, (key, input))| {
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().w(px(160.)).child(field_label(key)))
                            .child(
                                div()
                                    .flex_1()
                                    .child(Input::new(input).id(("vector-value", i))),
                            )
                    }),
            )
            .child(match self.action {
                Action::Point => t!("editor.design_vector_ui.hint_point"),
                Action::Join => t!("editor.design_vector_ui.hint_join"),
                Action::Split => t!("editor.design_vector_ui.hint_split"),
                Action::Skew => t!("editor.design_vector_ui.hint_skew"),
                Action::Perspective => t!("editor.design_vector_ui.hint_perspective"),
                Action::Envelope => t!("editor.design_vector_ui.hint_envelope"),
            })
    }
}
impl EditorView {
    pub(super) fn show_vector_editor(
        &mut self,
        id: NodeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(Node {
            kind: NodeKind::Path { path, .. },
            ..
        }) = self.editor.doc.node(id)
        else {
            return;
        };
        let (x, y, w, h) =
            emulsion_raster::vector_geometry::bounds(path).unwrap_or((0., 0., 1., 1.));
        let a = path
            .subpaths
            .first()
            .and_then(|s| s.anchors.first())
            .copied()
            .unwrap_or_else(|| emulsion_raster::vector::Anchor::corner((x, y)));
        let values = [
            ("Subpath", 0.),
            ("Anchor", 0.),
            ("Second subpath", 1.),
            ("X", a.p.0),
            ("Y", a.p.1),
            ("Incoming X", a.h_in.0),
            ("Incoming Y", a.h_in.1),
            ("Outgoing X", a.h_out.0),
            ("Outgoing Y", a.h_out.1),
            ("Horizontal degrees", 0.),
            ("Vertical degrees", 0.),
            ("Origin X", x + w / 2.),
            ("Origin Y", y + h / 2.),
            ("Top left X", x),
            ("Top left Y", y),
            ("Top right X", x + w),
            ("Top right Y", y),
            ("Bottom left X", x),
            ("Bottom left Y", y + h),
            ("Bottom right X", x + w),
            ("Bottom right Y", y + h),
            ("Sampling tolerance", 0.25),
        ];
        let form = cx.new(|cx| Form {
            action: Action::Point,
            fields: values
                .into_iter()
                .map(|(label, v)| {
                    (
                        label,
                        cx.new(|cx| InputState::new(window, cx).default_value(v.to_string())),
                    )
                })
                .collect(),
        });
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let form = form.clone();
            let owner = owner.clone();
            dialog
                .title(t!("editor.design_vector_ui.dialog_title"))
                .width(px(550.))
                .child(form.clone())
                .footer(crate::widgets::form_dialog_footer(t!(
                    "editor.design_vector_ui.apply"
                )))
                .on_ok(move |_, _, cx| {
                    owner
                        .update(cx, |this, cx| {
                            match form.read(cx).apply(&mut this.editor, id, cx) {
                                Ok(()) => {
                                    this.after_change(cx);
                                    true
                                }
                                Err(e) => {
                                    this.set_status(e, true, cx);
                                    false
                                }
                            }
                        })
                        .unwrap_or(false)
                })
        });
    }
    pub(super) fn vector_actions(&self, id: NodeId, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                Button::new("vector-edit")
                    .small()
                    .label(t!("editor.design_vector_ui.edit_points"))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.show_vector_editor(id, window, cx)
                    })),
            )
            .child(
                Button::new("vector-stroke-outline")
                    .small()
                    .label(t!("editor.design_vector_ui.stroke_outline"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.prepare_page_action(cx) {
                            return;
                        }
                        match vectors::outline_stroke(&mut this.editor, id) {
                            Ok(new) => {
                                this.set_layer_selection(vec![new], Some(new));
                                this.after_change(cx);
                            }
                            Err(e) => this.set_status(e, true, cx),
                        }
                    })),
            )
            .child(
                div().flex().flex_wrap().gap_1().children(
                    [
                        (Combine::Component, t!("editor.design_vector_ui.compound")),
                        (Combine::Union, t!("editor.design_vector_ui.union")),
                        (Combine::Subtract, t!("editor.design_vector_ui.subtract")),
                        (Combine::Intersect, t!("editor.design_vector_ui.intersect")),
                        (Combine::Exclude, t!("editor.design_vector_ui.exclude")),
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(i, (operation, label))| {
                        Button::new(("vector-combine", i))
                            .small()
                            .label(label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.prepare_page_action(cx) {
                                    return;
                                }
                                let ids = this.selected_layer_roots();
                                match vectors::combine(&mut this.editor, &ids, operation) {
                                    Ok(id) => {
                                        this.set_layer_selection(vec![id], Some(id));
                                        this.after_change(cx);
                                    }
                                    Err(e) => this.set_status(e, true, cx),
                                }
                            }))
                    }),
                ),
            )
            .child(t!("editor.design_vector_ui.select_matching"))
            .child(
                div().flex().flex_wrap().gap_1().children(
                    [
                        (MatchProperty::Kind, t!("home.detail_type")),
                        (MatchProperty::Fill, t!("design.direct.fill")),
                        (MatchProperty::Stroke, t!("editor.design_vector_ui.stroke")),
                        (MatchProperty::Opacity, t!("design.direct.opacity")),
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(i, (property, label))| {
                        Button::new(("vector-matching", i))
                            .small()
                            .label(label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                match vectors::matching(&this.editor.doc, id, property) {
                                    Ok(ids) => {
                                        this.set_layer_selection(ids, Some(id));
                                        this.after_change(cx);
                                    }
                                    Err(e) => this.set_status(e, true, cx),
                                }
                            }))
                    }),
                ),
            )
    }
}
