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
            match edge.routing {
                Routing::Straight => "Straight",
                Routing::Orthogonal => "Elbow",
                Routing::Curved => "Bendy",
                Routing::Cyclical => "Cyclical",
            },
            "Connector routing",
        )
        .dropdown_menu(move |mut menu, _, _| {
            let Some(editor) = route_owner.upgrade() else {
                return menu;
            };
            for (label, route) in [
                ("Straight", Routing::Straight),
                ("Elbow", Routing::Orthogonal),
                ("Bendy", Routing::Curved),
                ("Cyclical", Routing::Cyclical),
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
            "Line thickness",
        )
        .dropdown_menu(move |mut menu, _, _| {
            let Some(editor) = width_owner.upgrade() else {
                return menu;
            };
            for width in [0.5, 1., 1.5, 2., 3., 4., 6., 8.] {
                menu = menu.item(item(
                    &editor,
                    &format!("{width} px"),
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
            "Line pattern, crossings and corners",
        )
        .dropdown_menu(move |mut menu, w, cx| {
            let Some(editor) = line_owner.upgrade() else {
                return menu;
            };
            for (label, dash) in [
                ("Solid", vec![]),
                ("Dashed", vec![8., 5.]),
                ("Long dash", vec![16., 8.]),
                ("Dotted", vec![0., 4.]),
                ("Dash dot", vec![8., 4., 0., 4.]),
                ("Dash dot dot", vec![8., 4., 0., 4., 0., 4.]),
            ] {
                menu = menu.item(item(&editor, label, !locked, move |v, _, cx| {
                    v.connector_line(id, None, Some(&dash), cx)
                }));
            }
            menu=menu.separator();
            for (label,double) in [("Single line",false),("Double line",true)] {
                menu=menu.item(item(&editor,label,!locked,move |v,_,cx|v.update_diagram_edge(id,|e|e.double_line=double,cx)));
            }
            for (label,color) in [("No label background",None),("White label pill",Some([255;4])),("Soft blue label pill",Some([236,244,255,255]))] {
                menu=menu.item(item(&editor,label,!locked,move |v,_,cx|v.update_diagram_edge(id,|e|e.label_background=color,cx)));
            }
            let e = editor.clone();
            menu = menu
                .separator()
                .submenu("Crossings", w, cx, move |mut menu, _, _| {
                    for (label, jump) in [
                        ("None", JumpStyle::None),
                        ("Bridge", JumpStyle::Arc),
                        ("Gap", JumpStyle::Gap),
                        ("Sharp bridge", JumpStyle::Sharp),
                    ] {
                        menu = menu.item(item(&e, label, !locked, move |v, _, cx| {
                            v.update_diagram_edge(id, |e| e.jump_style = jump, cx)
                        }));
                    }
                    menu
                });
            let e = editor.clone();
            menu = menu.submenu("Corner radius", w, cx, move |mut menu, _, _| {
                for radius in [0., 3., 6., 10., 16., 24.] {
                    menu = menu.item(item(
                        &e,
                        &format!("{radius} px"),
                        !locked && corners,
                        move |v, _, cx| v.update_diagram_edge(id, |e| e.corner_radius = radius, cx),
                    ));
                }
                menu
            });
            menu.submenu("Endpoint size", w, cx, move |mut menu, _, _| {
                for scale in [0.5, 1., 1.5, 2., 3.] {
                    menu = menu.item(item(
                        &editor,
                        &format!("{scale}×"),
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
            })
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
                if !enabled || marker.kind == MarkerKind::None {
                    "None"
                } else {
                    match marker.kind {
                        MarkerKind::Block | MarkerKind::Classic => "Arrow",
                        MarkerKind::Open => "Open",
                        MarkerKind::Diamond => "◇",
                        MarkerKind::Oval => "○",
                        MarkerKind::CirclePlus => "⊕",
                        MarkerKind::Many => "Many",
                        MarkerKind::One => "One",
                        MarkerKind::MandatoryOne => "1..1",
                        MarkerKind::ZeroToOne => "0..1",
                        MarkerKind::ZeroToMany => "0..*",
                        MarkerKind::OneToMany => "1..*",
                        MarkerKind::None => "None",
                    }
                },
                if start {
                    "Start arrowhead"
                } else {
                    "End arrowhead"
                },
            )
            .dropdown_menu(move |mut menu, _, _| {
                let Some(editor) = marker_owner.upgrade() else {
                    return menu;
                };
                for kind in MarkerKind::ALL {
                    let label = match kind {
                        MarkerKind::None => "None",
                        MarkerKind::Block => "Triangle",
                        MarkerKind::Classic => "Classic arrow",
                        MarkerKind::Open => "Open arrow",
                        MarkerKind::Diamond => "Diamond",
                        MarkerKind::Oval => "Circle",
                        MarkerKind::CirclePlus => "Circle plus",
                        MarkerKind::Many => "Many",
                        MarkerKind::One => "One",
                        MarkerKind::MandatoryOne => "Exactly one",
                        MarkerKind::ZeroToOne => "Zero or one",
                        MarkerKind::ZeroToMany => "Zero or many",
                        MarkerKind::OneToMany => "One or many",
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
                        "Use hollow marker"
                    } else {
                        "Use filled marker"
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
                    .tooltip("Line color")
                    .accessibility_label("Line color")
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
                button("diagram-connector-reverse", "⇄", "Reverse direction").on_click(
                    cx.listener(move |v, _, _, cx| v.update_diagram_edge(id, Edge::reverse, cx)),
                ),
            )
            .child(marker_button(false))
            .child(
                button("diagram-connector-label", "T", "Edit connector label")
                    .on_click(cx.listener(|v, _, w, cx| v.diagram_edit_caption(w, cx))),
            )
            .child(
                Button::new("diagram-connector-more")
                    .label("…")
                    .tooltip("More connector actions")
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
