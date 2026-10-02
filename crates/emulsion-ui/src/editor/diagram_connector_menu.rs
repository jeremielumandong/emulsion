//! Floating controls for attached, editable connectors.
use super::*;
use emulsion_core::diagram::{Edge, JumpStyle, MarkerKind};

impl EditorView {
    fn connector_line(
        &mut self,
        id: NodeId,
        width: Option<f32>,
        dash: Option<&[f32]>,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        match diagram::connector_style_command(&self.editor.doc, id, width, dash, None) {
            Ok(command) => {
                self.execute(command, cx);
            }
            Err(error) => self.set_status(error, true, cx),
        }
    }
    pub(super) fn diagram_connector_toolbar(
        &mut self,
        id: NodeId,
        edge: Edge,
        x: f32,
        y: f32,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let corners = matches!(edge.routing, Routing::Straight | Routing::Orthogonal);
        let locked = self.editor.doc.locked_ancestor(id).is_some();
        let style = match self.editor.doc.node(edge.path).map(|n| &n.kind) {
            Some(NodeKind::Path { style, .. }) => *style,
            _ => Default::default(),
        };
        let owner = cx.weak_entity();
        let button = |id, label: &str, tooltip: &str| {
            Button::new(id)
                .label(label.to_owned())
                .tooltip(tooltip.to_owned())
                .accessibility_label(tooltip.to_owned())
                .xsmall()
                .ghost()
                .h(px(30.))
                .disabled(locked)
        };
        let route_owner = owner.clone();
        let routes = button(
            "diagram-connector-route",
            &match edge.routing {
                Routing::Straight => t!("editor.diagram_connector_menu.straight"),
                Routing::Orthogonal => {
                    if edge.routing_warning.is_some() {
                        t!("editor.diagram_connector_menu.elbow_warning")
                    } else {
                        t!("editor.diagram_connector_menu.elbow")
                    }
                }
                Routing::Curved => t!("editor.diagram_connector_menu.bendy"),
                Routing::Cyclical => t!("editor.diagram_connector_menu.cyclical"),
            },
            edge.routing_warning
                .as_deref()
                .unwrap_or(&t!("editor.diagram_connector_menu.routing")),
        )
        .dropdown_menu(move |mut menu, _, _| {
            let Some(editor) = route_owner.upgrade() else {
                return menu;
            };
            for (label, route) in [
                (
                    t!("editor.diagram_connector_menu.straight"),
                    Routing::Straight,
                ),
                (
                    t!("editor.diagram_connector_menu.elbow"),
                    Routing::Orthogonal,
                ),
                (t!("editor.diagram_connector_menu.bendy"), Routing::Curved),
                (
                    t!("editor.diagram_connector_menu.cyclical"),
                    Routing::Cyclical,
                ),
            ] {
                menu = menu.item(item(&editor, label, !locked, move |v, _, cx| {
                    v.update_diagram_edge(
                        id,
                        |e| {
                            e.routing = route;
                            e.waypoints.clear();
                        },
                        cx,
                    )
                }));
            }
            menu
        });
        let width_owner = owner.clone();
        let width = button(
            "diagram-connector-width",
            &format!("{} px", style.width),
            &t!("editor.diagram_connector_menu.line_thickness"),
        )
        .dropdown_menu(move |mut menu, _, _| {
            let Some(editor) = width_owner.upgrade() else {
                return menu;
            };
            for width in [0.5, 1., 1.5, 2., 3., 4., 6., 8.] {
                menu = menu.item(item(
                    &editor,
                    format!("{width} px"),
                    !locked,
                    move |v, _, cx| v.connector_line(id, Some(width), None, cx),
                ));
            }
            menu
        });
        let line_owner = owner.clone();
        let lines = button(
            "diagram-connector-line",
            if style.dash_count == 0 { "―" } else { "┄" },
            &t!("editor.diagram_connector_menu.line_pattern"),
        )
        .dropdown_menu(move |mut menu, w, cx| {
            let Some(editor) = line_owner.upgrade() else {
                return menu;
            };
            for (label, dash) in [
                (t!("editor.diagram_connector_menu.solid"), vec![]),
                (t!("editor.diagram_connector_menu.dashed"), vec![8., 5.]),
                (t!("editor.diagram_connector_menu.long_dash"), vec![16., 8.]),
                (t!("editor.diagram_connector_menu.dotted"), vec![0., 4.]),
                (
                    t!("editor.diagram_connector_menu.dash_dot"),
                    vec![8., 4., 0., 4.],
                ),
                (
                    t!("editor.diagram_connector_menu.dash_dot_dot"),
                    vec![8., 4., 0., 4., 0., 4.],
                ),
            ] {
                menu = menu.item(item(&editor, label, !locked, move |v, _, cx| {
                    v.connector_line(id, None, Some(&dash), cx)
                }));
            }
            menu = menu.separator();
            for (label, double) in [
                (t!("editor.diagram_connector_menu.single_line"), false),
                (t!("editor.diagram_connector_menu.double_line"), true),
            ] {
                menu = menu.item(item(&editor, label, !locked, move |v, _, cx| {
                    v.update_diagram_edge(id, |e| e.double_line = double, cx)
                }));
            }
            for (label, color) in [
                (
                    t!("editor.diagram_connector_menu.no_label_background"),
                    None,
                ),
                (
                    t!("editor.diagram_connector_menu.white_label_pill"),
                    Some([255; 4]),
                ),
                (
                    t!("editor.diagram_connector_menu.soft_blue_label_pill"),
                    Some([236, 244, 255, 255]),
                ),
            ] {
                menu = menu.item(item(&editor, label, !locked, move |v, _, cx| {
                    v.update_diagram_edge(id, |e| e.label_background = color, cx)
                }));
            }
            let e = editor.clone();
            menu = menu.separator().submenu(
                t!("editor.diagram_connector_menu.crossings"),
                w,
                cx,
                move |mut menu, _, _| {
                    for (label, jump) in [
                        (t!("editor.diagram_connector_menu.none"), JumpStyle::None),
                        (t!("editor.diagram_connector_menu.bridge"), JumpStyle::Arc),
                        (t!("editor.diagram_connector_menu.gap"), JumpStyle::Gap),
                        (
                            t!("editor.diagram_connector_menu.sharp_bridge"),
                            JumpStyle::Sharp,
                        ),
                    ] {
                        menu = menu.item(item(&e, label, !locked, move |v, _, cx| {
                            v.update_diagram_edge(id, |e| e.jump_style = jump, cx)
                        }));
                    }
                    menu
                },
            );
            let e = editor.clone();
            menu = menu.submenu(
                t!("editor.diagram_connector_menu.corner_radius"),
                w,
                cx,
                move |mut menu, _, _| {
                    for radius in [0., 3., 6., 10., 16., 24.] {
                        menu = menu.item(item(
                            &e,
                            format!("{radius} px"),
                            !locked && corners,
                            move |v, _, cx| {
                                v.update_diagram_edge(id, |e| e.corner_radius = radius, cx)
                            },
                        ));
                    }
                    menu
                },
            );
            menu.submenu(
                t!("editor.diagram_connector_menu.endpoint_size"),
                w,
                cx,
                move |mut menu, _, _| {
                    for scale in [0.5, 1., 1.5, 2., 3.] {
                        menu = menu.item(item(
                            &editor,
                            format!("{scale}×"),
                            !locked,
                            move |v, _, cx| {
                                v.update_diagram_edge(
                                    id,
                                    |e| {
                                        e.start_marker.size = 10. * scale;
                                        e.end_marker.size = 10. * scale;
                                    },
                                    cx,
                                )
                            },
                        ));
                    }
                    menu
                },
            )
        });
        let marker_button = |start: bool| {
            let marker = if start {
                edge.start_marker
            } else {
                edge.end_marker
            };
            let enabled = if start {
                edge.arrow_start
            } else {
                edge.arrow_end
            };
            let marker_owner = owner.clone();
            button(
                if start {
                    "diagram-connector-start"
                } else {
                    "diagram-connector-end"
                },
                &if !enabled || marker.kind == MarkerKind::None {
                    t!("editor.diagram_connector_menu.none")
                } else {
                    match marker.kind {
                        MarkerKind::Block | MarkerKind::Classic => {
                            t!("editor.diagram_connector_menu.arrow")
                        }
                        MarkerKind::Open => t!("editor.diagram_connector_menu.open"),
                        MarkerKind::Diamond => "◇".into(),
                        MarkerKind::Oval => "○".into(),
                        MarkerKind::CirclePlus => "⊕".into(),
                        MarkerKind::Many => t!("editor.diagram_connector_menu.many"),
                        MarkerKind::One => t!("editor.diagram_connector_menu.one"),
                        MarkerKind::MandatoryOne => "1..1".into(),
                        MarkerKind::ZeroToOne => "0..1".into(),
                        MarkerKind::ZeroToMany => "0..*".into(),
                        MarkerKind::OneToMany => "1..*".into(),
                        MarkerKind::None => t!("editor.diagram_connector_menu.none"),
                    }
                },
                &if start {
                    t!("editor.diagram_connector_menu.start_arrowhead")
                } else {
                    t!("editor.diagram_connector_menu.end_arrowhead")
                },
            )
            .dropdown_menu(move |mut menu, _, _| {
                let Some(editor) = marker_owner.upgrade() else {
                    return menu;
                };
                for kind in MarkerKind::ALL {
                    let label = match kind {
                        MarkerKind::None => t!("editor.diagram_connector_menu.none"),
                        MarkerKind::Block => t!("editor.diagram_connector_menu.triangle"),
                        MarkerKind::Classic => t!("editor.diagram_connector_menu.classic_arrow"),
                        MarkerKind::Open => t!("editor.diagram_connector_menu.open_arrow"),
                        MarkerKind::Diamond => t!("editor.diagram_connector_menu.diamond"),
                        MarkerKind::Oval => t!("editor.diagram_connector_menu.circle"),
                        MarkerKind::CirclePlus => t!("editor.diagram_connector_menu.circle_plus"),
                        MarkerKind::Many => t!("editor.diagram_connector_menu.many"),
                        MarkerKind::One => t!("editor.diagram_connector_menu.one"),
                        MarkerKind::MandatoryOne => t!("editor.diagram_connector_menu.exactly_one"),
                        MarkerKind::ZeroToOne => t!("editor.diagram_connector_menu.zero_or_one"),
                        MarkerKind::ZeroToMany => t!("editor.diagram_connector_menu.zero_or_many"),
                        MarkerKind::OneToMany => t!("editor.diagram_connector_menu.one_or_many"),
                    };
                    menu = menu.item(item(&editor, label, !locked, move |v, _, cx| {
                        v.update_diagram_edge(
                            id,
                            |e| {
                                if start {
                                    e.start_marker.kind = kind;
                                    e.arrow_start = kind != MarkerKind::None;
                                } else {
                                    e.end_marker.kind = kind;
                                    e.arrow_end = kind != MarkerKind::None;
                                }
                            },
                            cx,
                        )
                    }));
                }
                menu.separator().item(item(
                    &editor,
                    if marker.filled {
                        t!("editor.diagram_connector_menu.hollow_marker")
                    } else {
                        t!("editor.diagram_connector_menu.filled_marker")
                    },
                    !locked,
                    move |v, _, cx| {
                        v.update_diagram_edge(
                            id,
                            |e| {
                                if start {
                                    e.start_marker.filled = !marker.filled;
                                } else {
                                    e.end_marker.filled = !marker.filled;
                                }
                            },
                            cx,
                        )
                    },
                ))
            })
        };
        let more_owner = owner.clone();
        div()
            .id("diagram-connector-toolbar")
            .test_support()
            .absolute()
            .left(px(x))
            .top(px(y))
            .h(px(40.))
            .flex()
            .items_center()
            .gap(px(3.))
            .p(px(4.))
            .rounded(px(8.))
            .bg(p.panel)
            .border_1()
            .border_color(p.line)
            .shadow_md()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(routes)
            .child(
                Button::new("diagram-connector-color")
                    .tooltip(t!("editor.diagram_connector_menu.line_color"))
                    .accessibility_label(t!("editor.diagram_connector_menu.line_color"))
                    .xsmall()
                    .ghost()
                    .size(px(30.))
                    .disabled(locked)
                    .child(
                        div()
                            .size(px(18.))
                            .rounded_full()
                            .bg(gpui::rgba(u32::from_be_bytes(
                                style.stroke.unwrap_or(diagram::DEFAULT_LINE),
                            )))
                            .border_1()
                            .border_color(p.ink),
                    )
                    .on_click(cx.listener(|v, _, w, cx| v.diagram_color_dialog("stroke", w, cx))),
            )
            .child(width)
            .child(lines)
            .child(marker_button(true))
            .child(
                button(
                    "diagram-connector-reverse",
                    "⇄",
                    &t!("editor.diagram_connector_menu.reverse"),
                )
                .on_click(
                    cx.listener(move |v, _, _, cx| v.update_diagram_edge(id, Edge::reverse, cx)),
                ),
            )
            .child(marker_button(false))
            .child(
                button(
                    "diagram-connector-text-color",
                    "A̲",
                    &t!("editor.diagram_connector_menu.text_color"),
                )
                .on_click(cx.listener(|v, _, w, cx| v.diagram_color_dialog("text", w, cx))),
            )
            .child(
                button(
                    "diagram-connector-label",
                    "T",
                    &t!("editor.diagram_connector_menu.edit_label"),
                )
                .on_click(cx.listener(|v, _, w, cx| v.diagram_edit_caption(w, cx))),
            )
            .child(
                Button::new("diagram-connector-more")
                    .label("…")
                    .tooltip(t!("editor.diagram_connector_menu.more_actions"))
                    .xsmall()
                    .ghost()
                    .dropdown_menu(move |menu, w, cx| {
                        let Some(editor) = more_owner.upgrade() else {
                            return menu;
                        };
                        Self::diagram_object_menu(menu, &editor, w, cx)
                    }),
            )
            .into_any_element()
    }
}
