//! Common mxGraph geometry as editable native paths.
use super::*;
use emulsion_core::design::Element;
use emulsion_raster::vector::{Anchor, Path as VectorPath, SubPath};

pub(super) fn path(style: &BTreeMap<String, String>, b: [f64; 4]) -> Result<Option<VectorPath>> {
    let [x, y, w, h] = b;
    let shape = style.get("shape").map_or("", String::as_str);
    let polygon = |points: &[(f64, f64)], closed| VectorPath {
        subpaths: vec![SubPath {
            anchors: points
                .iter()
                .map(|(a, b)| Anchor::corner((x + a * w, y + b * h)))
                .collect(),
            closed,
        }],
    };
    let mut path = if style.contains_key("ellipse") || shape == "ellipse" {
        Element::Circle.path(x, y, w, h)
    } else {
        match shape {
            "hexagon" => polygon(
                &[
                    (0.25, 0.),
                    (0.75, 0.),
                    (1., 0.5),
                    (0.75, 1.),
                    (0.25, 1.),
                    (0., 0.5),
                ],
                true,
            ),
            "triangle" => polygon(&[(0., 0.), (1., 0.5), (0., 1.)], true),
            "line" => polygon(&[(0., 0.5), (1., 0.5)], false),
            "doubleEllipse" => {
                let mut p = Element::Circle.path(x, y, w, h);
                let inset = 3f64.min(w / 5.).min(h / 5.);
                p.subpaths.extend(
                    Element::Circle
                        .path(x + inset, y + inset, w - 2. * inset, h - 2. * inset)
                        .subpaths,
                );
                p
            }
            "cross" => polygon(
                &[
                    (0.33, 0.),
                    (0.67, 0.),
                    (0.67, 0.33),
                    (1., 0.33),
                    (1., 0.67),
                    (0.67, 0.67),
                    (0.67, 1.),
                    (0.33, 1.),
                    (0.33, 0.67),
                    (0., 0.67),
                    (0., 0.33),
                    (0.33, 0.33),
                ],
                true,
            ),
            "partialRectangle" => {
                let mut p = VectorPath::default();
                for (key, points) in [
                    ("top", [(0., 0.), (1., 0.)]),
                    ("right", [(1., 0.), (1., 1.)]),
                    ("bottom", [(1., 1.), (0., 1.)]),
                    ("left", [(0., 1.), (0., 0.)]),
                ] {
                    if style.get(key).is_none_or(|v| v != "0") {
                        p.subpaths.extend(polygon(&points, false).subpaths);
                    }
                }
                // A completely borderless table cell still needs editable bounds.
                if p.subpaths.is_empty() {
                    Element::Rectangle.path(x, y, w, h)
                } else {
                    p
                }
            }
            "actor" | "umlActor" => {
                let mut p = Element::Circle.path(x + w * 0.35, y, w * 0.3, h * 0.25);
                p.subpaths
                    .extend(polygon(&[(0.5, 0.25), (0.5, 0.65), (0.1, 1.)], false).subpaths);
                p.subpaths
                    .extend(polygon(&[(0.5, 0.65), (0.9, 1.)], false).subpaths);
                p.subpaths
                    .extend(polygon(&[(0., 0.4), (1., 0.4)], false).subpaths);
                p
            }
            "" | "rectangle" | "rect" if style.get("rounded").is_some_and(|v| v == "1") => {
                let radius = if style.get("absoluteArcSize").is_some_and(|v| v == "1") {
                    number(style, "arcSize", 20.)? / 2.
                } else {
                    w.min(h) * number(style, "arcSize", 20.)? / 100. / 2.
                };
                let r = radius.clamp(0., w.min(h) / 2.);
                VectorPath::from_svg(&format!("M {} {y} H {} Q {} {y} {} {} V {} Q {} {} {} {} H {} Q {x} {} {x} {} V {} Q {x} {y} {} {y} Z",x+r,x+w-r,x+w,x+w,y+r,y+h-r,x+w,y+h,x+w-r,y+h,x+r,y+h,y+h-r,y+r,x+r)).map_err(|e| error(e.to_string()))?
            }
            _ => return Ok(None),
        }
    };
    let angle = number(style, "rotation", 0.)?.to_radians();
    let sx = if style.get("flipH").is_some_and(|v| v == "1") {
        -1.
    } else {
        1.
    };
    let sy = if style.get("flipV").is_some_and(|v| v == "1") {
        -1.
    } else {
        1.
    };
    let center = glam::dvec2(x + w / 2., y + h / 2.);
    path.transform(
        glam::DAffine2::from_translation(center)
            * glam::DAffine2::from_angle(angle)
            * glam::DAffine2::from_scale(glam::dvec2(sx, sy))
            * glam::DAffine2::from_translation(-center),
    );
    Ok(Some(path))
}
