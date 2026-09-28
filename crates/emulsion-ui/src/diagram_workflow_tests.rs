use super::*;
use emulsion_core::{
    NodeKind,
    diagram::{Builder, ShapeKind},
    project::{ProjectEditor, ProjectKind},
};
use gpui_kit::test::TestWindowExt;

#[gpui_kit::test]
fn diagram_inspector_tabs_format_graph_objects_and_fill_is_undoable(cx: &mut TestAppContext) {
    let mut builder = Builder::new(800, 600).unwrap();
    let id = builder
        .add_shape(ShapeKind::Process, [100., 100., 120., 60.], "Task")
        .unwrap();
    let doc = builder.finish().unwrap();
    let body = doc.diagram.as_ref().unwrap().shapes[&id].body;
    let label = doc.diagram.as_ref().unwrap().shapes[&id].label;
    let (ws, cx) = open(cx, doc.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1000.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Diagram, doc).unwrap(),
                "Diagram".into(),
                window,
                cx,
            )
        });
        let view = ws.read(cx).editor.clone().unwrap();
        view.update(cx, |e, cx| {
            e.set_layer_selection(vec![id], Some(id));
            cx.notify();
        });
        view
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("diagram-inspector").visible());
        assert!(view.read(cx).suggestions.is_empty());
        assert!(!view.read(cx).suggest_busy);
        assert_eq!(
            view.read(cx).doc_kind.as_ref().unwrap().kind,
            emulsion_ai::kind::DocKind::Graphic
        );
        window.click(("diagram-fill", 3usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window,cx| {
        let editor=view.read(cx);
        assert!(matches!(&editor.editor.doc.node(body).unwrap().kind,NodeKind::Path{style,..} if style.fill==Some([213,232,212,255])));
        assert_eq!(editor.text_target().unwrap().0,label);
        window.click("project-undo",cx);
    });
    cx.run_until_parked();
    cx.update(|window,cx| {
        assert!(matches!(&view.read(cx).editor.doc.node(body).unwrap().kind,NodeKind::Path{style,..} if style.fill==Some(emulsion_core::diagram::DEFAULT_FILL)));
        window.click(("diagram-property-tab",1usize),cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("diagram-edit-text").visible());
        window.click(("diagram-property-tab", 2usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find(("diagram-inspector-layout", 0usize)).visible());
        window.click(("diagram-property-tab", 3usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, _| assert!(window.find("diagram-edit-data").visible()));
}

#[gpui_kit::test]
fn diagram_default_categories_insert_network_stencils_with_undo(cx: &mut TestAppContext) {
    let doc = emulsion_core::Document::new(800, 600);
    let (ws, cx) = open(cx, doc.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1000.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Diagram, doc).unwrap(),
                "Stencils".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.click(("diagram-stencil-category", 0usize), cx);
        window.click(("diagram-stencil-category", 1usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("diagram-stencil-category", 5usize), cx));
    cx.run_until_parked();
    let server = emulsion_core::diagram::stencils::STENCILS
        .iter()
        .position(|s| s.id == "server")
        .unwrap();
    cx.update(|window, cx| {
        assert!(window.find(("diagram-shape", server)).visible());
        window.click(("diagram-shape", server), cx);
    });
    cx.run_until_parked();
    cx.update(|window,cx|{
        let doc=&view.read(cx).editor.doc;
        let shape=doc.diagram.as_ref().unwrap().shapes.values().next().unwrap();
        assert_eq!(shape.data["emulsion_stencil"],"server");
        assert!(matches!(&doc.node(shape.body).unwrap().kind,NodeKind::Path{path,..} if path.subpaths.len()>2));
        window.click("project-undo",cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert!(view.read(cx).editor.doc.nodes.is_empty()));
}

#[gpui_kit::test]
fn diagram_ports_drag_connect_and_toolbar_click_connect_through_real_events(
    cx: &mut TestAppContext,
) {
    use gpui_kit::{MouseButton, MouseDownEvent};
    let mut b = Builder::new(800, 600).unwrap();
    let a = b
        .add_shape(ShapeKind::Process, [80., 150., 140., 80.], "Source")
        .unwrap();
    let target = b
        .add_shape(ShapeKind::Decision, [450., 150., 140., 80.], "Target")
        .unwrap();
    let doc = b.finish().unwrap();
    let (ws, cx) = open(cx, doc.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1000.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Diagram, doc).unwrap(),
                "Connectors".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    let (center, start, end) = cx.update(|_, cx| {
        let e = view.read(cx);
        let z = e.view.zoom;
        (
            e.doc_to_window((150., 190.)).unwrap(),
            e.doc_to_window((220. + 12. / z, 190.)).unwrap(),
            e.doc_to_window((450., 190.)).unwrap(),
        )
    });
    // Hover exposes ports even without selecting the object first.
    cx.simulate_mouse_move(center, None, Default::default());
    cx.simulate_mouse_move(start, None, Default::default());
    cx.simulate_event(MouseDownEvent {
        position: start,
        button: MouseButton::Left,
        click_count: 1,
        ..Default::default()
    });
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(
            view.read(cx)
                .editor
                .doc
                .diagram
                .as_ref()
                .unwrap()
                .edges
                .len(),
            0
        )
    });
    cx.simulate_mouse_up(end, MouseButton::Left, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            let graph = e.editor.doc.diagram.as_ref().unwrap();
            assert_eq!(graph.edges.len(), 1);
            let edge = graph.edges.values().next().unwrap();
            assert_eq!(edge.source.shape, a);
            assert_eq!(edge.target.shape, target);
            assert_eq!(edge.source.port, emulsion_core::diagram::Port::East);
            e.undo(cx);
            assert!(e.editor.doc.diagram.as_ref().unwrap().edges.is_empty());
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("diagram-canvas-connect", cx));
    cx.simulate_click(center, Default::default());
    cx.simulate_click(end, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(
            view.read(cx)
                .editor
                .doc
                .diagram
                .as_ref()
                .unwrap()
                .edges
                .len(),
            1
        )
    });
    // Releasing over empty canvas or pressing Escape must not create an edge.
    cx.update(|_, cx| view.update(cx, |e, cx| e.undo(cx)));
    cx.run_until_parked();
    let blank = cx.update(|_, cx| view.read(cx).doc_to_window((360., 380.)).unwrap());
    for cancel in [false, true] {
        cx.simulate_mouse_move(center, None, Default::default());
        cx.simulate_mouse_move(start, None, Default::default());
        cx.simulate_mouse_down(start, MouseButton::Left, Default::default());
        cx.simulate_mouse_move(
            if cancel { end } else { blank },
            Some(MouseButton::Left),
            Default::default(),
        );
        if cancel {
            cx.simulate_keystrokes("escape");
        }
        cx.simulate_mouse_up(
            if cancel { end } else { blank },
            MouseButton::Left,
            Default::default(),
        );
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = view.read(cx);
            assert!(e.editor.doc.diagram.as_ref().unwrap().edges.is_empty());
            assert!(!e.editor.in_transaction());
        });
    }
}

#[gpui_kit::test]
fn diagram_svg_canvas_keeps_text_editable_and_rebuilds_after_label_change(cx: &mut TestAppContext) {
    let mut b = Builder::new(640, 480).unwrap();
    let id = b
        .add_shape(ShapeKind::Process, [100., 100., 180., 80.], "Vector label")
        .unwrap();
    let doc = b.finish().unwrap();
    let (ws, cx) = open(cx, doc.clone());
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Diagram, doc).unwrap(),
                "Vector text".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            assert!(
                e.svg_canvas
                    .borrow()
                    .ready((e.editor.active_page(), e.editor.revision))
            );
            let label = e.editor.doc.diagram.as_ref().unwrap().shapes[&id].label;
            let NodeKind::Text { spec, .. } = &e.editor.doc.node(label).unwrap().kind else {
                panic!()
            };
            let mut spec = (**spec).clone();
            spec.text = "Updated vector label".into();
            e.execute(
                emulsion_core::Command::SetText {
                    id: label,
                    spec: Box::new(spec),
                },
                cx,
            );
            assert!(
                !e.svg_canvas
                    .borrow()
                    .ready((e.editor.active_page(), e.editor.revision))
            );
            assert!(
                e.svg_canvas
                    .borrow()
                    .displayable((e.editor.active_page(), e.editor.revision)),
                "Keep displaying the previous SVG until the edit is rendered"
            );
            assert!(
                !e.svg_canvas
                    .borrow()
                    .displayable((e.editor.active_page() + 999, e.editor.revision)),
                "Never show a stale scene on a different page"
            );
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert!(view.read(cx).svg_canvas.borrow().ready((
            view.read(cx).editor.active_page(),
            view.read(cx).editor.revision
        )))
    });
}

#[gpui_kit::test]
fn diagram_mouse_box_ctrl_selection_group_move_and_ungroup(cx: &mut TestAppContext) {
    use gpui_kit::{Modifiers, MouseButton};
    let imported = emulsion_io::drawio::from_xml(r#"<mxGraphModel pageWidth="800" pageHeight="600"><root>
        <mxCell id="0"/><mxCell id="1" parent="0"/>
        <mxCell id="a" value="First" vertex="1" parent="1"><mxGeometry x="120" y="160" width="120" height="60"/></mxCell>
        <mxCell id="b" value="Second" vertex="1" parent="1"><mxGeometry x="350" y="160" width="120" height="60"/></mxCell>
        <mxCell id="c" value="Third" vertex="1" parent="1"><mxGeometry x="600" y="400" width="120" height="60"/></mxCell>
        <mxCell id="e" edge="1" parent="1" source="a" target="b"><mxGeometry relative="1"/></mxCell>
        </root></mxGraphModel>"#).unwrap();
    let doc = imported.project.pages[0].doc.clone();
    let shape_ids: Vec<_> = doc
        .diagram
        .as_ref()
        .unwrap()
        .shapes
        .keys()
        .copied()
        .collect();
    let (a, b) = (shape_ids[0], shape_ids[1]);
    let (ws, cx) = open(cx, doc.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1000.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Diagram, doc).unwrap(),
                "Selection".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    let screen =
        |p, cx: &mut VisualTestContext| cx.update(|_, cx| view.read(cx).doc_to_window(p).unwrap());
    let frames = cx.update(|_, cx| view.read(cx).svg_canvas.borrow().rendered_frames);
    let pa = screen((180., 190.), cx);
    let pb = screen((410., 190.), cx);
    cx.simulate_click(pa, Modifiers::none());
    cx.simulate_click(
        pb,
        Modifiers {
            control: true,
            ..Default::default()
        },
    );
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(
            view.read(cx).svg_canvas.borrow().rendered_frames,
            frames,
            "Selection changes reuse the rendered vector frame"
        )
    });
    cx.update(|_, cx| assert_eq!(view.read(cx).selected_layer_ids(), vec![a, b]));
    cx.simulate_click(
        pa,
        Modifiers {
            control: true,
            ..Default::default()
        },
    );
    cx.update(|_, cx| assert_eq!(view.read(cx).selected_layer_ids(), vec![b]));
    // Start on blank canvas: select both shapes and their connecting edge.
    let from = screen((95., 125.), cx);
    let to = screen((500., 250.), cx);
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    cx.update(|window, cx| {
        let e = view.read(cx);
        assert!(e.layer_is_selected(a) && e.layer_is_selected(b));
        assert_eq!(e.selected_layer_ids().len(), 3);
        assert!(
            e.editor.doc.selection.is_none(),
            "Object selection must not create a raster mask"
        );
        window.click("diagram-canvas-group", cx);
    });
    cx.run_until_parked();
    let group = cx.update(|_, cx| {
        let e = view.read(cx);
        let g = e.selected.unwrap();
        assert_eq!(e.editor.doc.node(a).unwrap().parent, Some(g));
        assert_eq!(e.editor.doc.node(b).unwrap().parent, Some(g));
        g
    });
    let target = screen((210., 230.), cx);
    cx.simulate_mouse_down(pa, MouseButton::Left, Modifiers::none());
    for i in 1..=8 {
        let next = pa + (target - pa) * (i as f32 / 8.);
        cx.simulate_mouse_move(next, Some(MouseButton::Left), Modifiers::none());
        cx.run_until_parked();
    }
    cx.simulate_mouse_up(target, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    cx.update(|window, cx| {
        let e = view.read(cx);
        assert_eq!(
            e.selected,
            Some(group),
            "Clicking a grouped member selects its group"
        );
        e.editor.doc.validate().unwrap();
        let shape = &e.editor.doc.diagram.as_ref().unwrap().shapes[&a];
        let b = emulsion_core::diagram::shape_bounds(&e.editor.doc, shape).unwrap();
        assert!(
            b[0] > 130. && b[1] > 175.,
            "Grouped objects move together: {b:?}"
        );
        assert!(!e.editor.in_transaction());
        window.click("project-undo", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let e = view.read(cx);
        let shape = &e.editor.doc.diagram.as_ref().unwrap().shapes[&a];
        assert_eq!(
            emulsion_core::diagram::shape_bounds(&e.editor.doc, shape).unwrap(),
            [120., 160., 120., 60.]
        );
        window.click("diagram-canvas-ungroup", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            assert!(e.editor.doc.node(group).is_none());
            assert_eq!(e.editor.doc.diagram.as_ref().unwrap().shapes.len(), 3);
            e.editor.doc.validate().unwrap();
            e.select_all(cx);
            assert_eq!(e.selected_layer_ids().len(), 4);
            e.deselect(cx);
            assert!(e.selected_layer_ids().is_empty());
        });
    });
}

#[gpui_kit::test]
fn diagram_color_dialog_applies_to_selection_and_cancel_keeps_original(cx: &mut TestAppContext) {
    let mut b = Builder::new(800, 600).unwrap();
    let a = b
        .add_shape(ShapeKind::Process, [100., 100., 120., 60.], "First")
        .unwrap();
    let second = b
        .add_shape(ShapeKind::Process, [300., 100., 120., 60.], "Second")
        .unwrap();
    let doc = b.finish().unwrap();
    let (ws, cx) = open(cx, doc.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1000.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Diagram, doc).unwrap(),
                "Colors".into(),
                window,
                cx,
            )
        });
        let view = ws.read(cx).editor.clone().unwrap();
        view.update(cx, |e, cx| {
            e.set_layer_selection(vec![a, second], Some(second));
            cx.notify();
        });
        view
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("diagram-color-fill", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("style-color-picker").visible());
        window.click("style-color-swatch-0", cx);
        window.click("diagram-color-ok", cx);
    });
    cx.run_until_parked();
    cx.update(|window,cx| {
        let e=view.read(cx);
        for id in [a,second] {
            let body=e.editor.doc.diagram.as_ref().unwrap().shapes[&id].body;
            assert!(matches!(&e.editor.doc.node(body).unwrap().kind,NodeKind::Path{style,..} if style.fill==Some([0,0,0,255])));
        }
        window.click("project-undo",cx);
    });
    cx.run_until_parked();
    let original = cx.update(|window, cx| {
        let doc = view.read(cx).editor.doc.clone();
        window.click(("diagram-style", 1usize), cx);
        doc
    });
    cx.run_until_parked();
    cx.update(|window,cx| {
        let e=view.read(cx); let body=e.editor.doc.diagram.as_ref().unwrap().shapes[&a].body;
        assert!(matches!(&e.editor.doc.node(body).unwrap().kind,NodeKind::Path{style,..} if style.fill==Some([178,242,235,255]) && style.stroke==Some(emulsion_core::diagram::DEFAULT_LINE) && style.width==1.));
        window.click("project-undo",cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc, original);
        window.click("diagram-color-stroke", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.click("style-color-swatch-9", cx);
        window.click("diagram-color-cancel", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc, original);
        window.click("diagram-color-text", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.click("style-color-swatch-0", cx);
        window.click("diagram-color-ok", cx);
    });
    cx.run_until_parked();
    cx.update(|_,cx| {
        let e=view.read(cx);
        for id in [a,second] {
            let label=e.editor.doc.diagram.as_ref().unwrap().shapes[&id].label;
            assert!(matches!(&e.editor.doc.node(label).unwrap().kind,NodeKind::Text{spec,..} if spec.color==[0,0,0,255]));
        }
    });
}

#[gpui_kit::test]
fn sample_template_search_inserts_reusable_stencils_and_undoes(cx: &mut TestAppContext) {
    let doc = emulsion_core::Document::new(800, 600);
    let (ws, cx) = open(cx, doc.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1000.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Diagram, doc).unwrap(),
                "Templates".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("diagram-library-tab", 1usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.click("diagram-stencil-search", cx));
    cx.simulate_input("cloud storage");
    cx.run_until_parked();
    let index = emulsion_core::diagram_library::TEMPLATES
        .iter()
        .position(|t| t.id == "cloud-architecture")
        .unwrap();
    cx.update(|window, cx| {
        assert!(window.find(("diagram-template", index)).visible());
        window.click(("diagram-template", index), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let editor = &view.read(cx).editor;
        assert_eq!(editor.page_list().len(), 2);
        assert!(
            editor
                .doc
                .diagram
                .as_ref()
                .unwrap()
                .shapes
                .values()
                .any(|s| s
                    .data
                    .get("emulsion_stencil")
                    .is_some_and(|id| id == "load-balancer"))
        );
        assert!(!emulsion_core::diagram::document_stencils(&editor.doc).is_empty());
        window.click("project-undo", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.page_list().len(), 1));
}

#[gpui_kit::test]
fn diagram_library_templates_containers_themes_and_packs_are_functional(cx: &mut TestAppContext) {
    let doc = emulsion_core::Document::new(800, 600);
    let (ws, cx) = open(cx, doc.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1200.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Diagram, doc).unwrap(),
                "Library".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("diagram-library-tab", 1usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("diagram-template-gallery").visible());
        window.click(("diagram-template", 0usize), cx);
    });
    cx.run_until_parked();
    let original = cx.update(|window, cx| {
        let e = view.read(cx);
        assert_eq!(e.editor.page_list().len(), 2);
        assert_eq!(e.editor.doc.diagram.as_ref().unwrap().shapes.len(), 4);
        let doc = e.editor.doc.clone();
        window.click(("diagram-library-tab", 3usize), cx);
        doc
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("diagram-theme-gallery").visible());
        window.click(("diagram-theme", 1usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_ne!(view.read(cx).editor.doc, original);
        window.click("project-undo", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).editor.doc, original);
        window.click(("diagram-library-tab", 2usize), cx);
    });
    cx.run_until_parked();
    let container = emulsion_core::diagram::stencils::STENCILS
        .iter()
        .position(|s| s.id == "container")
        .unwrap();
    cx.update(|window, cx| {
        assert!(window.find(("diagram-shape", container)).visible());
        window.click(("diagram-shape", container), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(
            view.read(cx)
                .editor
                .doc
                .diagram
                .as_ref()
                .unwrap()
                .shapes
                .len(),
            5
        );
        window.click(("diagram-library-tab", 4usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, _| {
        assert!(window.find("diagram-stencil-packs").visible());
        assert!(window.find(("diagram-pack-added", 0usize)).visible());
    });
}

#[gpui_kit::test]
fn diagram_toolbox_drag_drops_one_stencil_at_pointer_and_undoes(cx: &mut TestAppContext) {
    use gpui_kit::{Modifiers, MouseButton};
    let doc = emulsion_core::Document::new(800, 600);
    let (ws, cx) = open(cx, doc.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1000.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Diagram, doc).unwrap(),
                "Drop".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    let (from, to) = cx.update(|window, cx| {
        (
            window.find(("diagram-shape", 0usize)).bounds().center(),
            view.read(cx).doc_to_window((420., 330.)).unwrap(),
        )
    });
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        from + gpui_kit::point(gpui_kit::px(12.), gpui_kit::px(0.)),
        Some(MouseButton::Left),
        Modifiers::none(),
    );
    cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    cx.update(|window, cx| {
        let e = view.read(cx);
        let model = e
            .editor
            .doc
            .diagram
            .as_ref()
            .expect("drop creates a diagram");
        assert_eq!(
            model.shapes.len(),
            1,
            "Dragging must not also trigger click insertion"
        );
        let shape = model.shapes.values().next().unwrap();
        let [x, y, w, h] = emulsion_core::diagram::shape_bounds(&e.editor.doc, shape).unwrap();
        assert!(
            (x + w / 2. - 420.).abs() < 1. && (y + h / 2. - 330.).abs() < 1.,
            "Drop center must follow the pointer: {x},{y}"
        );
        window.click("project-undo", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert!(view.read(cx).editor.doc.nodes.is_empty()));
}

#[gpui_kit::test]
fn diagram_installed_pack_drag_drops_once_and_undoes(cx: &mut TestAppContext) {
    use gpui_kit::{Modifiers, MouseButton};
    let dir = tempfile::tempdir().unwrap();
    let mut builder = Builder::new(160, 100).unwrap();
    builder
        .add_shape(ShapeKind::Process, [20., 20., 120., 60.], "Installed")
        .unwrap();
    let pack = ProjectEditor::new_project(ProjectKind::Diagram, builder.finish().unwrap()).unwrap();
    let path = dir.path().join("stencil.emu");
    emulsion_io::project::write(&pack.snapshot().unwrap(), &path).unwrap();
    let mut catalog = emulsion_io::creative_library::Catalog {
        revision: u64::MAX,
        ..Default::default()
    };
    let asset = catalog
        .add_asset(path, emulsion_io::creative_library::AssetKind::Stencil)
        .unwrap();
    catalog
        .assets
        .iter_mut()
        .find(|a| a.id == asset)
        .unwrap()
        .variants = vec!["Installed".into()];
    let doc = emulsion_core::Document::new(800, 600);
    let (ws, cx) = open(cx, doc.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1000.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Diagram, doc).unwrap(),
                "Drop".into(),
                window,
                cx,
            )
        });
        let view = ws.read(cx).editor.clone().unwrap();
        view.update(cx, |e, cx| {
            e.install_catalog(catalog.clone());
            cx.notify();
        });
        view
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.click("diagram-stencil-search", cx);
    });
    cx.run_until_parked();
    cx.simulate_input("Installed");
    cx.run_until_parked();
    let (from, to) = cx.update(|window, cx| {
        (
            window
                .find((
                    gpui_kit::ElementId::from("stencil-pack-item"),
                    format!("{asset}-0"),
                ))
                .bounds()
                .center(),
            view.read(cx).doc_to_window((420., 330.)).unwrap(),
        )
    });
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        from + gpui_kit::point(gpui_kit::px(12.), gpui_kit::px(0.)),
        Some(MouseButton::Left),
        Modifiers::none(),
    );
    cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    cx.update(|window, cx| {
        let e = view.read(cx);
        let model = e
            .editor
            .doc
            .diagram
            .as_ref()
            .expect("drop creates a diagram");
        assert_eq!(
            model.shapes.len(),
            1,
            "Dragging must not also trigger click insertion"
        );
        let shape = model.shapes.values().next().unwrap();
        let [x, y, w, h] = emulsion_core::diagram::shape_bounds(&e.editor.doc, shape).unwrap();
        assert!(
            (x + w / 2. - 420.).abs() < 1. && (y + h / 2. - 330.).abs() < 1.,
            "Drop center must follow the pointer: {x},{y}"
        );
        window.click("project-undo", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert!(view.read(cx).editor.doc.nodes.is_empty()));
}

#[gpui_kit::test]
fn diagram_object_context_arrange_and_floating_lock_toolbar_share_selection_and_undo(
    cx: &mut TestAppContext,
) {
    let mut b = Builder::new(800, 600).unwrap();
    let a = b
        .add_shape(ShapeKind::Process, [100., 140., 120., 60.], "First")
        .unwrap();
    let z = b
        .add_shape(ShapeKind::Process, [320., 140., 120., 60.], "Second")
        .unwrap();
    let doc = b.finish().unwrap();
    let (ws, cx) = open(cx, doc.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1000.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Diagram, doc.clone()).unwrap(),
                "Context menu".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    let point = cx.update(|_, cx| view.read(cx).doc_to_window((160., 170.)).unwrap());
    cx.simulate_mouse_down(point, gpui_kit::MouseButton::Right, Default::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(view.read(cx).selected_layer_ids(), vec![a]);
        assert!(window.find("diagram-object-toolbar").visible());
        window.within("popup-menu").hover(8usize, cx);
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("right");
    cx.run_until_parked();
    let button = cx.update(|window, _| window.within("submenu").find(0usize).bounds().center());
    cx.simulate_click(button, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(
            view.read(cx)
                .editor
                .doc
                .children(None)
                .into_iter()
                .filter(|id| *id == a || *id == z)
                .collect::<Vec<_>>(),
            vec![z, a]
        );
        view.update(cx, |v, cx| v.undo(cx));
        assert_eq!(view.read(cx).editor.doc, doc);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("diagram-object-lock", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(view.read(cx).editor.doc.node(a).unwrap().locked);
        assert!(window.find("diagram-object-toolbar").visible());
        window.click("diagram-object-fill", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("diagram-color-ok").is_none());
        window.click("diagram-object-lock", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert!(!view.read(cx).editor.doc.node(a).unwrap().locked));
}

#[gpui_kit::test]
fn diagram_connector_toolbar_routes_reverses_and_formats_with_undo(cx: &mut TestAppContext) {
    use emulsion_core::diagram::{Endpoint, Port, Routing};
    let mut builder = Builder::new(800, 600).unwrap();
    let a = builder
        .add_shape(ShapeKind::Process, [100., 140., 100., 60.], "A")
        .unwrap();
    let b = builder
        .add_shape(ShapeKind::Process, [400., 300., 100., 60.], "B")
        .unwrap();
    let id = builder
        .connect(
            Endpoint {
                shape: a,
                port: Port::East,
            },
            Endpoint {
                shape: b,
                port: Port::West,
            },
            "Next",
            Routing::Orthogonal,
        )
        .unwrap();
    let doc = builder.finish().unwrap();
    let (ws, cx) = open(cx, doc.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1000.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Diagram, doc.clone()).unwrap(),
                "Connector controls".into(),
                window,
                cx,
            )
        });
        let view = ws.read(cx).editor.clone().unwrap();
        view.update(cx, |v, cx| {
            v.set_layer_selection(vec![id], Some(id));
            cx.notify();
        });
        view
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("diagram-connector-toolbar").visible());
        window.click("diagram-connector-reverse", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(
            view.read(cx).editor.doc.diagram.as_ref().unwrap().edges[&id]
                .source
                .shape,
            b
        );
        view.update(cx, |v, cx| v.undo(cx));
        assert_eq!(view.read(cx).editor.doc, doc);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("diagram-connector-width", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.within("popup-menu").click(4usize, cx));
    cx.run_until_parked();
    cx.update(|_,cx|{
        let e=view.read(cx);let path=e.editor.doc.diagram.as_ref().unwrap().edges[&id].path;
        assert!(matches!(&e.editor.doc.node(path).unwrap().kind,NodeKind::Path{style,..} if style.width==3.));
        view.update(cx,|v,cx|v.undo(cx));assert_eq!(view.read(cx).editor.doc,doc);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("diagram-connector-line", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.within("popup-menu").click(8usize, cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert!(view.read(cx).editor.doc.diagram.as_ref().unwrap().edges[&id].double_line);
        view.update(cx, |v, cx| v.undo(cx));
        assert_eq!(view.read(cx).editor.doc, doc);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("diagram-connector-line", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.within("popup-menu").click(10usize, cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert!(
            view.read(cx).editor.doc.diagram.as_ref().unwrap().edges[&id]
                .label_background_path
                .is_some()
        );
        view.update(cx, |v, cx| v.undo(cx));
        assert_eq!(view.read(cx).editor.doc, doc);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("diagram-connector-route", cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.within("popup-menu").click(3usize, cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(
            view.read(cx).editor.doc.diagram.as_ref().unwrap().edges[&id].routing,
            Routing::Cyclical
        )
    });
}

#[gpui_kit::test]
fn diagram_imported_object_toolbox_drag_preserves_artwork_and_one_undo(cx: &mut TestAppContext) {
    use gpui_kit::{Modifiers, MouseButton};
    let mut builder = Builder::new(800, 600).unwrap();
    let source = builder
        .add_shape(
            ShapeKind::Process,
            [100., 120., 140., 80.],
            "Imported server",
        )
        .unwrap();
    let doc = builder.finish().unwrap();
    let (ws, cx) = open(cx, doc.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1000.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Diagram, doc.clone()).unwrap(),
                "Imported objects".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(200));
    cx.run_until_parked();
    let (from, to) = cx.update(|window, cx| {
        assert!(window.find(("diagram-used-shape", source)).visible());
        (
            window
                .find(("diagram-used-shape", source))
                .bounds()
                .center(),
            view.read(cx).doc_to_window((500., 350.)).unwrap(),
        )
    });
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        from + gpui_kit::point(gpui_kit::px(12.), gpui_kit::px(0.)),
        Some(MouseButton::Left),
        Modifiers::none(),
    );
    cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = view.read(cx);
        let model = e.editor.doc.diagram.as_ref().unwrap();
        assert_eq!(model.shapes.len(), 2);
        let (_, copy) = model.shapes.iter().find(|(id, _)| **id != source).unwrap();
        let bounds = emulsion_core::diagram::shape_bounds(&e.editor.doc, copy).unwrap();
        assert_eq!(bounds, [430., 310., 140., 80.]);
        view.update(cx, |v, cx| v.undo(cx));
        assert_eq!(view.read(cx).editor.doc, doc);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("diagram-imported-clear", cx));
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(250));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("diagram-imported-restore").visible());
        assert_eq!(view.read(cx).editor.doc, doc);
        window.click("diagram-imported-restore", cx);
    });
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(250));
    cx.run_until_parked();
    cx.update(|window, _| assert!(window.find(("diagram-used-shape", source)).visible()));
}

#[gpui_kit::test]
fn diagram_port_drag_can_attach_to_an_existing_connector(cx: &mut TestAppContext) {
    use emulsion_core::diagram::{Endpoint, Port, Routing};
    use gpui_kit::MouseButton;
    let mut builder = Builder::new(800, 600).unwrap();
    let a = builder
        .add_shape(ShapeKind::Process, [80., 100., 100., 60.], "A")
        .unwrap();
    let b = builder
        .add_shape(ShapeKind::Process, [480., 100., 100., 60.], "B")
        .unwrap();
    let source = builder
        .add_shape(ShapeKind::Process, [280., 350., 100., 60.], "Branch")
        .unwrap();
    let line = builder
        .connect(
            Endpoint {
                shape: a,
                port: Port::East,
            },
            Endpoint {
                shape: b,
                port: Port::West,
            },
            "",
            Routing::Straight,
        )
        .unwrap();
    let doc = builder.finish().unwrap();
    let (ws, cx) = open(cx, doc.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1000.)));
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Diagram, doc.clone()).unwrap(),
                "Branch connectors".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    let (center, start, end) = cx.update(|_, cx| {
        let e = view.read(cx);
        (
            e.doc_to_window((330., 380.)).unwrap(),
            e.doc_to_window((330., 350. - 12. / e.view.zoom)).unwrap(),
            e.doc_to_window((330., 130.)).unwrap(),
        )
    });
    cx.simulate_mouse_move(center, None, Default::default());
    cx.simulate_mouse_move(start, None, Default::default());
    cx.simulate_mouse_down(start, MouseButton::Left, Default::default());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Default::default());
    cx.simulate_mouse_up(end, MouseButton::Left, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = view.read(cx);
        let model = e.editor.doc.diagram.as_ref().unwrap();
        assert_eq!(model.edges.len(), 2);
        let branch = model
            .edges
            .values()
            .find(|e| e.source.shape == source)
            .unwrap();
        assert_eq!(branch.target.shape, line);
        e.editor.doc.validate().unwrap();
        view.update(cx, |v, cx| v.undo(cx));
        assert_eq!(view.read(cx).editor.doc, doc);
    });
}

#[gpui_kit::test]
fn diagram_review_controls_and_saved_view_links_work(cx: &mut TestAppContext) {
    use emulsion_core::diagram::{self, workspace::Link};
    use gpui_kit::component::WindowExt;
    let mut builder = Builder::new(800, 600).unwrap();
    let id = builder
        .add_shape(ShapeKind::Class, [100., 100., 200., 140.], "Customer")
        .unwrap();
    let doc = builder.finish().unwrap();
    let (ws, cx) = open(cx, doc.clone());
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1440.), gpui_kit::px(1000.)));
    let view = cx.update(|window, cx| {
        let mut project = ProjectEditor::new_project(ProjectKind::Diagram, doc).unwrap();
        project.path = Some("/tmp/diagram-ui-link.emu".into());
        ws.update(cx, |ws, cx| {
            ws.install_project(project, "Diagram".into(), window, cx)
        });
        let view = ws.read(cx).editor.clone().unwrap();
        view.update(cx, |e, cx| {
            e.set_layer_selection(vec![id], Some(id));
            e.diagram_default_style(false, cx);
            e.diagram_thumbnail(false, cx);
            e.diagram_edit_fields(ShapeKind::Class, window, cx);
        });
        view
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("diagram-structure-title").visible());
        assert!(window.find("diagram-structure-fields").visible());
        assert!(window.find("diagram-structure-methods").visible());
        window.close_dialog(cx);
        view.update(cx, |e, cx| {
            diagram::workspace::add_comment(&mut e.editor, id, None, "Reviewer", "Confirm fields")
                .unwrap();
            e.diagram_comments(window, cx);
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("diagram-comment-input").visible());
        assert!(window.find(("diagram-comment-resolve", 1u64)).visible());
        window.click(("diagram-comment-resolve", 1u64), cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            assert!(e.editor.doc.diagram.as_ref().unwrap().settings.threads[&1].resolved);
            let link = Link {
                project: Some("/tmp/diagram-ui-link.emu".into()),
                page: e.editor.active_page(),
                nodes: vec![id],
                view: Some([123., 234., 1.75, 15.]),
            }
            .encode()
            .unwrap();
            e.set_layer_selection(vec![], None);
            e.diagram_follow_link(&link, cx).unwrap();
            assert_eq!(e.selected, Some(id));
            assert_eq!(e.view.center, (123., 234.));
            assert_eq!(e.view.zoom, 1.75);
            let settings = &e.editor.doc.diagram.as_ref().unwrap().settings;
            assert_eq!(settings.thumbnail, vec![id]);
            assert!(settings.shape_style.is_some());
        })
    });
}

#[gpui_kit::test]
fn diagram_custom_attachment_follows_picked_point_through_move_resize_and_undo(
    cx: &mut TestAppContext,
) {
    use emulsion_core::diagram::{Port, endpoint_position};
    let mut builder = Builder::new(800, 600).unwrap();
    let a = builder
        .add_shape(ShapeKind::Process, [80., 100., 160., 120.], "Source")
        .unwrap();
    let b = builder
        .add_shape(ShapeKind::Process, [480., 100., 160., 120.], "Target")
        .unwrap();
    let doc = builder.finish().unwrap();
    let (ws, cx) = open(cx, doc.clone());
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(
                ProjectEditor::new_project(ProjectKind::Diagram, doc).unwrap(),
                "Custom attachments".into(),
                window,
                cx,
            )
        });
        ws.read(cx).editor.clone().unwrap()
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("diagram-canvas-connect", cx));
    let (start, end) = cx.update(|_, cx| {
        let e = view.read(cx);
        (
            e.doc_to_window((240., 130.)).unwrap(),
            e.doc_to_window((480., 190.)).unwrap(),
        )
    });
    cx.simulate_click(start, Default::default());
    cx.simulate_click(end, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |v, cx| {
            let edge = v
                .editor
                .doc
                .diagram
                .as_ref()
                .unwrap()
                .edges
                .values()
                .next()
                .unwrap()
                .clone();
            assert_eq!((edge.source.shape, edge.target.shape), (a, b));
            let Port::Custom { x, y } = edge.source.port else {
                panic!("Expected picked position")
            };
            assert!((x - 1.).abs() < 0.01 && (y - 0.25).abs() < 0.01);
            let source_before =
                endpoint_position(&v.editor.doc, &edge.source, (480., 190.)).unwrap();
            v.editor
                .execute(Command::TranslateNode {
                    id: a,
                    dx: 30.,
                    dy: 20.,
                })
                .unwrap();
            let moved = endpoint_position(&v.editor.doc, &edge.source, (480., 190.)).unwrap();
            assert!(
                (moved.0 - source_before.0 - 30.).abs() < 0.01
                    && (moved.1 - source_before.1 - 20.).abs() < 0.01
            );
            v.editor
                .execute(Command::TransformNodes {
                    ids: vec![a],
                    transform: [2., 0., 0., 1.5, 0., 0.],
                })
                .unwrap();
            let resized = endpoint_position(&v.editor.doc, &edge.source, (480., 190.)).unwrap();
            assert!(
                (resized.0 - moved.0 * 2.).abs() < 0.01 && (resized.1 - moved.1 * 1.5).abs() < 0.01
            );
            v.undo(cx);
            v.undo(cx);
            assert_eq!(
                endpoint_position(&v.editor.doc, &edge.source, (480., 190.)).unwrap(),
                source_before
            );
        })
    });
}
