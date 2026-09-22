use crate::style_options::*;
use crate::styles::{self, LayerStyle};
use crate::{Document, Node};
use emulsion_raster::composite::flatten;
use emulsion_raster::{BlendMode, IRect, Placement, Raster};
use std::sync::Arc;
fn doc() -> Document {
    let mut d = Document::new(64, 64);
    d.nodes.push(Node::raster(
        1,
        "shape",
        Arc::new(Raster::from_fn(64, 64, [0; 4], |x, y| {
            if (16..48).contains(&x) && (16..48).contains(&y) {
                [18000, 12000, 6000, 65535]
            } else {
                [0; 4]
            }
        })),
        Placement::default(),
    ));
    d
}
fn pixels(d: &Document) -> Vec<[u16; 4]> {
    flatten(&d.composite_tree(), 0).read_rect(IRect::new(0, 0, 64, 64))
}
#[test]
fn every_catalogue_effect_renders_and_disabled_effect_is_identity() {
    for style in LayerStyle::catalogue() {
        let mut d = doc();
        let base = pixels(&d);
        d.nodes[0].styles = vec![style.clone()];
        assert_ne!(pixels(&d), base, "{} renders", style.label());
        d.nodes[0].style_options = vec![StyleOptions {
            enabled: false,
            ..Default::default()
        }];
        assert_eq!(pixels(&d), base, "{} disable", style.label());
    }
}
#[test]
fn stroke_positions_are_hollow_with_zero_fill() {
    for position in [
        StrokePosition::Outside,
        StrokePosition::Inside,
        StrokePosition::Center,
    ] {
        let mut d = doc();
        d.nodes[0].styles = vec![LayerStyle::Stroke {
            color: [255, 0, 0],
            opacity: 100.,
            size: 4.,
        }];
        d.nodes[0].blending.fill_opacity = 0.;
        d.nodes[0].style_options = vec![StyleOptions {
            stroke_position: position,
            ..Default::default()
        }];
        let r = flatten(&d.composite_tree(), 0);
        assert_eq!(r.get(32, 32), [0; 4]);
        match position {
            StrokePosition::Outside => {
                assert!(r.get(14, 32)[3] > 0);
                assert_eq!(r.get(18, 32)[3], 0)
            }
            StrokePosition::Inside => {
                assert_eq!(r.get(14, 32)[3], 0);
                assert!(r.get(18, 32)[3] > 0)
            }
            StrokePosition::Center => {
                assert!(r.get(15, 32)[3] > 0);
                assert!(r.get(17, 32)[3] > 0)
            }
        }
    }
}
#[test]
fn multistop_gradient_kinds_reverse_and_alpha_render() {
    let mut d = doc();
    d.nodes[0].styles = vec![LayerStyle::GradientOverlay {
        from: [0; 3],
        to: [255; 3],
        angle: 0.,
        opacity: 100.,
    }];
    d.nodes[0].blending.fill_opacity = 0.;
    let mut option = StyleOptions::default();
    option.gradient.stops = vec![
        GradientStop {
            position: 0.,
            color: [255, 0, 0, 0],
        },
        GradientStop {
            position: 0.5,
            color: [0, 255, 0, 255],
        },
        GradientStop {
            position: 1.,
            color: [0, 0, 255, 255],
        },
    ];
    d.nodes[0].style_options = vec![option.clone()];
    let linear = pixels(&d);
    for kind in [
        GradientKind::Radial,
        GradientKind::Angle,
        GradientKind::Reflected,
        GradientKind::Diamond,
    ] {
        d.nodes[0].style_options[0].gradient.kind = kind;
        assert_ne!(pixels(&d), linear, "{kind:?}");
    }
    d.nodes[0].style_options[0] = option;
    d.nodes[0].style_options[0].gradient.reverse = true;
    assert_ne!(pixels(&d), linear);
    let p = flatten(&d.composite_tree(), 0);
    assert!(p.get(17, 32)[2] > p.get(17, 32)[0]);
    assert!(p.get(46, 32)[3] < p.get(17, 32)[3]);
}
#[test]
fn imported_rgba_pattern_and_gradient_strokes_render() {
    let mut d = doc();
    d.nodes[0].styles = vec![LayerStyle::Stroke {
        color: [0; 3],
        opacity: 100.,
        size: 4.,
    }];
    let mut o = StyleOptions {
        fill: FillType::Pattern,
        ..Default::default()
    };
    o.pattern.image = Some(Arc::new(PatternImage {
        width: 2,
        height: 1,
        pixels: vec![255, 0, 0, 255, 0, 0, 255, 128],
    }));
    d.nodes[0].style_options = vec![o];
    let r = flatten(&d.composite_tree(), 0);
    assert!(r.get(20, 14)[0] > r.get(20, 14)[2]);
    assert!(r.get(21, 14)[2] > r.get(21, 14)[0]);
    assert!(r.get(21, 14)[3] < r.get(20, 14)[3]);
    d.nodes[0].style_options[0].fill = FillType::Gradient;
    assert_ne!(pixels(&d), r.read_rect(IRect::new(0, 0, 64, 64)));
}
#[test]
fn advanced_controls_change_each_relevant_effect_and_global_light_invalidates() {
    for index in [0, 1, 2, 6, 7, 8] {
        let mut d = doc();
        d.nodes[0].styles = vec![LayerStyle::catalogue()[index].clone()];
        let base = pixels(&d);
        d.nodes[0].style_options = vec![StyleOptions {
            spread: 60.,
            choke: 60.,
            noise: 40.,
            invert_contour: true,
            altitude: 65.,
            glow_source: GlowSource::Center,
            bevel_style: BevelStyle::Emboss,
            technique: Technique::ChiselSoft,
            ..Default::default()
        }];
        assert_ne!(pixels(&d), base, "index {index}");
    }
    let mut d = doc();
    d.nodes[0].styles = vec![LayerStyle::catalogue()[0].clone()];
    d.nodes[0].style_options = vec![StyleOptions {
        use_global_light: true,
        ..Default::default()
    }];
    let a = styles::render(&d, &d.nodes[0]).unwrap();
    d.global_light.angle = 10.;
    let b = styles::render(&d, &d.nodes[0]).unwrap();
    assert!(!Arc::ptr_eq(&a, &b));
}
#[test]
fn effect_order_blend_and_group_content_are_rendered() {
    let mut d = doc();
    d.nodes[0].styles = vec![
        LayerStyle::ColorOverlay {
            color: [255, 0, 0],
            opacity: 100.,
        },
        LayerStyle::ColorOverlay {
            color: [0, 0, 255],
            opacity: 100.,
        },
    ];
    let first = pixels(&d);
    d.nodes[0].styles.reverse();
    assert_ne!(pixels(&d), first);
    d.nodes[0].style_options = vec![
        StyleOptions {
            blend: BlendMode::Multiply,
            ..Default::default()
        };
        2
    ];
    assert_ne!(pixels(&d), first);
    let mut group = Node::group(2, "group");
    group.styles = vec![LayerStyle::ColorOverlay {
        color: [0, 255, 0],
        opacity: 100.,
    }];
    d.nodes[0].styles.clear();
    d.nodes[0].style_options.clear();
    d.nodes[0].parent = Some(2);
    d.nodes.insert(0, group);
    let r = flatten(&d.composite_tree(), 0);
    assert_eq!(r.get(32, 32), [0, 65535, 0, 65535]);
    assert_eq!(r.get(0, 0), [0; 4]);
}
#[test]
fn options_serde_defaults_and_pattern_validation() {
    let o: StyleOptions = serde_json::from_str("{}").unwrap();
    assert_eq!(o, StyleOptions::default());
    assert!(o.valid());
    let mut o = o;
    o.pattern.image = Some(Arc::new(PatternImage {
        width: 2,
        height: 2,
        pixels: vec![0; 4],
    }));
    assert!(!o.valid());
}

#[test]
fn masked_fill_accepts_effects_and_bevel_variants_change_relief() {
    let mut d = doc();
    d.nodes[0].kind = crate::node::NodeKind::Fill {
        rgba: [100, 80, 60, 255],
    };
    d.nodes[0].mask = Some(Arc::new(emulsion_raster::Mask::from_fn(
        64,
        64,
        0,
        |x, y| {
            if (16..48).contains(&x) && (16..48).contains(&y) {
                255
            } else {
                0
            }
        },
    )));
    d.nodes[0].styles = vec![LayerStyle::ColorOverlay {
        color: [255, 0, 0],
        opacity: 100.,
    }];
    let fill = flatten(&d.composite_tree(), 0);
    assert_eq!(fill.get(32, 32), [65535, 0, 0, 65535]);
    assert_eq!(fill.get(4, 4), [0; 4]);
    d.nodes[0].styles = vec![LayerStyle::catalogue()[6].clone()];
    let base = pixels(&d);
    for style in [
        BevelStyle::Outer,
        BevelStyle::Emboss,
        BevelStyle::Pillow,
        BevelStyle::Stroke,
    ] {
        d.nodes[0].style_options = vec![StyleOptions {
            bevel_style: style,
            ..Default::default()
        }];
        assert_ne!(pixels(&d), base, "bevel {style:?}");
    }
}
