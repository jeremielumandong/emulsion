use super::*;
use emulsion_core::{Node, styles::LayerStyle};
use emulsion_raster::vector::PathStyle;

fn fixture() -> Document {
    let mut doc = Document::new(240, 180);
    let mut node = Node::path(
        1,
        "Shadowed card",
        Arc::new(emulsion_raster::vector_geometry::rectangle(
            40., 30., 120., 80.,
        )),
        PathStyle {
            fill: Some([60, 140, 220, 255]),
            stroke: None,
            ..Default::default()
        },
        240,
        180,
    );
    node.styles.push(LayerStyle::DropShadow {
        color: [0, 0, 0],
        opacity: 50.,
        angle: 120.,
        distance: 5.,
        size: 10.,
    });
    doc.nodes.push(node);
    doc.next_id = 2;
    doc
}

fn has_filter(group: &resvg::usvg::Group) -> bool {
    !group.filters().is_empty()
        || group
            .children()
            .iter()
            .any(|node| matches!(node, resvg::usvg::Node::Group(g) if has_filter(g)))
}

fn has_path(group: &resvg::usvg::Group) -> bool {
    group.children().iter().any(|node| match node {
        resvg::usvg::Node::Path(_) => true,
        resvg::usvg::Node::Group(group) => has_path(group),
        _ => false,
    })
}

#[test]
fn shadows_are_retained_without_filters_and_match_scalable_export() {
    let doc = fixture();
    let scene = SvgViewport::new(&doc).unwrap();
    assert!(scene.layers[0].primitives.is_some());
    assert!(
        scene
            .layers
            .iter()
            .all(|layer| !has_filter(layer.tree.root()))
    );
    let export = crate::project_export::vector_svg(&doc).unwrap();
    let tree = resvg::usvg::Tree::from_data(&export, &Default::default()).unwrap();
    assert!(
        has_filter(tree.root()),
        "Export must retain scalable filters"
    );
    for matrix in [
        [1., 0., 0., 1., 0., 0.],
        [2., 0., 0., 2., -50., -30.],
        [1.2, 0.2, -0.2, 1.2, 0., 0.],
    ] {
        let actual = scene.render((240, 180), matrix).unwrap();
        let mut expected = resvg::tiny_skia::Pixmap::new(240, 180).unwrap();
        let [a, b, c, d, e, f] = matrix.map(|v| v as f32);
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_row(a, b, c, d, e, f),
            &mut expected.as_mut(),
        );
        let mut expected = expected.take();
        for pixel in expected.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
        let error: usize = actual
            .iter()
            .zip(&expected)
            .map(|(a, b)| usize::from(a.abs_diff(*b)))
            .sum();
        assert!(
            error as f64 / (actual.len() as f64) < 0.5,
            "shadow appearance changed: {error}"
        );
        assert!(
            actual
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[3] > 5 && p[3] < 120 && p[0] == 0),
            "shadow disappeared"
        );
    }
    // Retain native geometry even at deep zoom, not a bitmap of the whole card.
    assert!(has_path(scene.layers[0].tree.root()));
    let same = SvgViewport::updated(&doc, Some(&scene)).unwrap();
    assert_eq!(same.rebuilt_layers, 0);
    assert!(Arc::ptr_eq(&scene.layers[0].tree, &same.layers[0].tree));
    assert!(Arc::ptr_eq(
        scene.layers[0].primitives.as_ref().unwrap(),
        same.layers[0].primitives.as_ref().unwrap()
    ));
    let mut changed = doc.clone();
    if let LayerStyle::DropShadow { size, .. } = &mut changed.nodes[0].styles[0] {
        *size = 20.;
    }
    let next = SvgViewport::updated(&changed, Some(&scene)).unwrap();
    assert_eq!(next.rebuilt_layers, 1);
    assert_ne!(
        scene.render((240, 180), [1., 0., 0., 1., 0., 0.]).unwrap(),
        next.render((240, 180), [1., 0., 0., 1., 0., 0.]).unwrap()
    );
}

#[test]
fn decoded_shadows_preserve_nested_transforms_and_isolation_boundaries() {
    let source = crate::project_export::viewport_subtree(&fixture(), 1).unwrap();
    let source = String::from_utf8(source).unwrap();
    let body = &source[source.find('>').unwrap() + 1..source.rfind("</svg>").unwrap()];
    for isolation in ["", "opacity=\"0.4\"", "clip-path=\"url(#clip)\""] {
        let source = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"300\" height=\"200\"><defs><clipPath id=\"clip\"><rect x=\"60\" y=\"40\" width=\"50\" height=\"60\"/></clipPath></defs><g transform=\"translate(17 9) rotate(13) scale(1.2 .8)\" {isolation}>{body}</g></svg>"
        );
        let tree = resvg::usvg::Tree::from_str(&source, &Default::default()).unwrap();
        let retained = primitives::retain(&tree);
        if !isolation.is_empty() {
            assert!(
                retained.is_none(),
                "Compositing boundaries must remain atomic"
            );
            continue;
        }
        let retained = retained.unwrap();
        for scale in [1., 4., 16.] {
            let transform =
                resvg::tiny_skia::Transform::from_row(scale, 0.1, -0.1, scale, -20., -15.);
            let mut expected = resvg::tiny_skia::Pixmap::new(300, 200).unwrap();
            let mut actual = expected.clone();
            resvg::render(&tree, transform, &mut expected.as_mut());
            primitives::render(&retained, transform, &mut actual.as_mut());
            let error: usize = expected
                .data()
                .iter()
                .zip(actual.data())
                .map(|(a, b)| usize::from(a.abs_diff(*b)))
                .sum();
            assert!(
                error as f64 / (actual.data().len() as f64) < 0.1,
                "Transform changed pixels: {error}"
            );
        }
    }
}

#[test]
fn parallel_shadow_bands_match_single_surface_at_fractional_zoom_and_rotation() {
    let mut scene = SvgViewport::new(&fixture()).unwrap();
    assert!(scene.parallel);
    for matrix in [
        [4.1, 0., 0., 4.1, 0.3, -10.7],
        [3.8, 0.9, -0.9, 3.8, 110., 25.],
    ] {
        scene.parallel = true;
        let bands = scene.render((1280, 720), matrix).unwrap();
        scene.parallel = false;
        let single = scene.render((1280, 720), matrix).unwrap();
        let error: usize = bands
            .iter()
            .zip(&single)
            .map(|(a, b)| usize::from(a.abs_diff(*b)))
            .sum();
        assert!(
            error as f64 / (bands.len() as f64) < 0.01,
            "Band boundaries changed pixels: {error}"
        );
    }
}
