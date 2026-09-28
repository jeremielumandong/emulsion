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
        assert!(matches!(&view.read(cx).editor.doc.node(body).unwrap().kind,NodeKind::Path{style,..} if style.fill==Some([233,239,251,255])));
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
        assert!(matches!(&e.editor.doc.node(body).unwrap().kind,NodeKind::Path{style,..} if style.fill==Some([239,246,255,255]) && style.stroke==Some([59,130,246,255])));
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
