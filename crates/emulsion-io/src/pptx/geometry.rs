use super::*;
use std::fmt::Write;
pub(super) fn number(x: &Xml, key: &str, default: f64) -> f64 {
    x.attr(key)
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && v.abs() < 1e15)
        .unwrap_or(default)
}
pub(super) fn xfrm(x: Option<&Xml>, group: bool) -> Result<(DAffine2, (f64, f64))> {
    let Some(x) = x else {
        return Ok((DAffine2::IDENTITY, (100., 100.)));
    };
    let off = x.child("off");
    let ext = x.child("ext");
    let (ox, oy) = off.map_or((0., 0.), |x| {
        (number(x, "x", 0.) / EMU, number(x, "y", 0.) / EMU)
    });
    let (w, h) = ext.map_or((100., 100.), |x| {
        (number(x, "cx", 0.) / EMU, number(x, "cy", 0.) / EMU)
    });
    if !(0. ..=100000.).contains(&w)
        || !(0. ..=100000.).contains(&h)
        || ox.abs() > 1e7
        || oy.abs() > 1e7
    {
        return Err(error("Invalid shape transform extent"));
    }
    let center = dvec2(w / 2., h / 2.);
    let angle = number(x, "rot", 0.) / 60000.;
    let flips = dvec2(
        if x.attr("flipH") == "1" || x.attr("flipH") == "true" {
            -1.
        } else {
            1.
        },
        if x.attr("flipV") == "1" || x.attr("flipV") == "true" {
            -1.
        } else {
            1.
        },
    );
    let mut m = DAffine2::from_translation(dvec2(ox, oy) + center)
        * DAffine2::from_angle(angle.to_radians())
        * DAffine2::from_scale(flips)
        * DAffine2::from_translation(-center);
    if group {
        let (cx, cy) = x.child("chOff").map_or((0., 0.), |x| {
            (number(x, "x", 0.) / EMU, number(x, "y", 0.) / EMU)
        });
        let (cw, ch) = x.child("chExt").map_or((w, h), |x| {
            (
                number(x, "cx", w * EMU) / EMU,
                number(x, "cy", h * EMU) / EMU,
            )
        });
        if cw <= 0. || ch <= 0. {
            return Err(error("Invalid group child transform"));
        }
        m = m
            * DAffine2::from_scale(dvec2(w / cw, h / ch))
            * DAffine2::from_translation(dvec2(-cx, -cy));
    }
    Ok((m, (w, h)))
}
pub(super) fn color(node: &Xml, theme: &BTreeMap<String, [u8; 4]>) -> Option<[u8; 4]> {
    let c = if matches!(
        node.name.as_str(),
        "srgbClr" | "schemeClr" | "sysClr" | "prstClr"
    ) {
        node
    } else {
        node.children.iter().find(|c| {
            matches!(
                c.name.as_str(),
                "srgbClr" | "schemeClr" | "sysClr" | "prstClr"
            )
        })?
    };
    let mut value = match c.name.as_str() {
        "schemeClr" => *theme.get(c.attr("val")).unwrap_or(&[0, 0, 0, 255]),
        "prstClr" => match c.attr("val") {
            "white" => [255; 4],
            "red" => [255, 0, 0, 255],
            "blue" => [0, 0, 255, 255],
            "green" => [0, 128, 0, 255],
            _ => [0, 0, 0, 255],
        },
        _ => {
            let hex = if c.name == "sysClr" {
                c.attr("lastClr")
            } else {
                c.attr("val")
            };
            let v = u32::from_str_radix(hex, 16).ok()?;
            [(v >> 16) as u8, (v >> 8) as u8, v as u8, 255]
        }
    };
    for modifier in &c.children {
        let amount = number(modifier, "val", 100000.) / 100000.;
        match modifier.name.as_str() {
            "alpha" => value[3] = (255. * amount).clamp(0., 255.).round() as u8,
            "alphaMod" => value[3] = (f64::from(value[3]) * amount).clamp(0., 255.).round() as u8,
            "tint" => {
                for v in &mut value[..3] {
                    *v = (f64::from(*v) + (255. - f64::from(*v)) * amount)
                        .clamp(0., 255.)
                        .round() as u8
                }
            }
            "shade" | "lumMod" => {
                for v in &mut value[..3] {
                    *v = (f64::from(*v) * amount).clamp(0., 255.).round() as u8
                }
            }
            "lumOff" => {
                for v in &mut value[..3] {
                    *v = (f64::from(*v) + 255. * amount).clamp(0., 255.).round() as u8
                }
            }
            _ => (),
        }
    }
    Some(value)
}
pub(super) fn paint(node: &Xml, theme: &BTreeMap<String, [u8; 4]>) -> (Option<[u8; 4]>, PathPaint) {
    if node.child("noFill").is_some() {
        return (None, PathPaint::Solid);
    }
    if let Some(g) = node.child("gradFill") {
        let stops = g
            .child("gsLst")
            .into_iter()
            .flat_map(|x| x.children("gs"))
            .filter_map(|s| {
                Some(emulsion_raster::vector::GradientStop {
                    offset: (number(s, "pos", 0.) / 100000.) as f32,
                    color: color(s, theme)?,
                })
            })
            .collect::<Vec<_>>();
        let angle = g.child("lin").map_or(0., |x| number(x, "ang", 0.) / 60000.) as f32;
        if let Ok(paint) = PathPaint::from_stops(&stops, g.child("path").is_some(), angle) {
            return (stops.first().map(|s| s.color), paint);
        }
    }
    (
        node.child("solidFill").and_then(|x| color(x, theme)),
        PathPaint::Solid,
    )
}
pub(super) fn shape(sp: &Xml, size: (f64, f64)) -> Result<(VectorPath, Option<String>)> {
    let (w, h) = size;
    if let Some(custom) = sp.child("custGeom") {
        let mut result = VectorPath::default();
        for p in custom
            .child("pathLst")
            .into_iter()
            .flat_map(|n| n.children("path"))
        {
            let pw = number(p, "w", w * EMU);
            let ph = number(p, "h", h * EMU);
            if pw <= 0. || ph <= 0. {
                return Err(error("Invalid custom path coordinate space"));
            }
            let mut d = String::new();
            for cmd in &p.children {
                let points = cmd
                    .children("pt")
                    .map(|pt| (number(pt, "x", 0.) * w / pw, number(pt, "y", 0.) * h / ph))
                    .collect::<Vec<_>>();
                match (cmd.name.as_str(), points.as_slice()) {
                    ("moveTo", [(x, y)]) => std::write!(d, "M{x} {y} ").unwrap(),
                    ("lnTo", [(x, y)]) => std::write!(d, "L{x} {y} ").unwrap(),
                    ("cubicBezTo", [(x, y), (a, b), (c, e)]) => {
                        std::write!(d, "C{x} {y} {a} {b} {c} {e} ").unwrap()
                    }
                    ("quadBezTo", [(x, y), (a, b)]) => std::write!(d, "Q{x} {y} {a} {b} ").unwrap(),
                    ("close", _) => d.push_str("Z "),
                    _ => {
                        return Err(error(format!(
                            "Unsupported custom path command {}",
                            cmd.name
                        )));
                    }
                }
            }
            result
                .subpaths
                .extend(VectorPath::from_svg(&d).map_err(error)?.subpaths);
            if result.anchor_count() > emulsion_raster::vector::MAX_ANCHORS {
                return Err(error("Path exceeds anchor limit"));
            }
        }
        return Ok((result, None));
    }
    let preset = sp.child("prstGeom").map_or("rect", |x| x.attr("prst"));
    let (path, warning) = match preset {
        "rect" => (vector_geometry::rectangle(0., 0., w, h), None),
        "ellipse" => (vector_geometry::ellipse(0., 0., w, h), None),
        "line" => (
            VectorPath::from_svg(&format!("M0 0 L{w} {h}")).map_err(error)?,
            None,
        ),
        "triangle" => (
            VectorPath::from_svg(&format!("M{} 0 L{w} {h} L0 {h} Z", w / 2.)).map_err(error)?,
            None,
        ),
        "diamond" => (
            VectorPath::from_svg(&format!(
                "M{} 0 L{w} {} L{} {h} L0 {} Z",
                w / 2.,
                h / 2.,
                w / 2.,
                h / 2.
            ))
            .map_err(error)?,
            None,
        ),
        "roundRect" => {
            let r = w.min(h) * 0.16667;
            let d = format!(
                "M{r} 0 H{} Q{w} 0 {w} {r} V{} Q{w} {h} {} {h} H{r} Q0 {h} 0 {} V{r} Q0 0 {r} 0 Z",
                w - r,
                h - r,
                w - r,
                h - r
            );
            (VectorPath::from_svg(&d).map_err(error)?, None)
        }
        _ => (
            vector_geometry::rectangle(0., 0., w, h),
            Some(format!(
                "Preset {preset} imported as an editable bounding rectangle"
            )),
        ),
    };
    Ok((path, warning))
}
pub(super) fn color_xml(c: [u8; 4]) -> String {
    format!(
        "<a:srgbClr val=\"{:02X}{:02X}{:02X}\"><a:alpha val=\"{}\"/></a:srgbClr>",
        c[0],
        c[1],
        c[2],
        u32::from(c[3]) * 100000 / 255
    )
}
pub(super) fn fill_xml(c: Option<[u8; 4]>, paint: PathPaint) -> String {
    let Some(c) = c else {
        return "<a:noFill/>".into();
    };
    if let Some(stops) = paint.gradient_stops(c) {
        let stops = stops
            .iter()
            .map(|s| {
                format!(
                    "<a:gs pos=\"{}\">{}</a:gs>",
                    (s.offset * 100000.).round() as i32,
                    color_xml(s.color)
                )
            })
            .collect::<String>();
        let direction = if paint.is_radial() {
            "<a:path path=\"circle\"><a:fillToRect l=\"50000\" t=\"50000\" r=\"50000\" b=\"50000\"/></a:path>".into()
        } else {
            format!(
                "<a:lin ang=\"{}\" scaled=\"1\"/>",
                (paint.gradient_angle().rem_euclid(360.) * 60000.).round() as i32
            )
        };
        format!("<a:gradFill rotWithShape=\"1\"><a:gsLst>{stops}</a:gsLst>{direction}</a:gradFill>")
    } else {
        format!("<a:solidFill>{}</a:solidFill>", color_xml(c))
    }
}
