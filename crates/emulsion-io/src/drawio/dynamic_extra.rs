//! Native SVG translations of JGraph draw.io shapes (Apache-2.0).
//! Copyright (c) 2006-2010, JGraph Holdings Ltd. See the pinned sources in
//! assets/diagram-stencils/UPSTREAM-DYNAMIC.json and LICENSE-APACHE.
use super::*;
use std::fmt::Write;
pub(super) const NAMES: &[&str] = &[
    "dimension",
    "mxgraph.mockup.containers.browserwindow",
    "mxgraph.mockup.graphics.simpleicon",
    "mxgraph.mockup.graphics.icongrid",
    "mxgraph.mockup.graphics.piechart",
    "mxgraph.mockup.markup.curlybrace",
    "mxgraph.infographic.bannersinglefold",
    "mxgraph.infographic.shadedcube",
    "mxgraph.bootstrap.topbutton",
    "mxgraph.ios7ui.phone",
    "mxgraph.ios7ui.downloadbar",
    "mxgraph.arrows2.uturnarrow",
];
pub(super) fn content(name: &str, s: &BTreeMap<String, String>, w: f64, h: f64) -> Result<String> {
    let mut out = String::new();
    match name {
        "dimension" => {
            let sw = number(s, "strokeWidth", 1.)? / 2.;
            let al = 10. + 2. * sw;
            let cy = h - al / 2.;
            write!(out,"<path fill='none' d='M 0 0 V {h} M {sw} {cy} L {} {} M {sw} {cy} L {} {} M {sw} {cy} H {} M {w} 0 V {h} M {} {cy} L {} {} M {} {cy} L {} {}'/>",sw+al,cy-al/2.,sw+al,cy+al/2.,w-sw,w-sw,w-al-sw,cy-al/2.,w-sw,w-al-sw,cy+al/2.).unwrap();
        }
        "mxgraph.mockup.containers.browserwindow" => {
            let w = w.max(260.);
            let h = h.max(110.);
            let inside = escape(s.get("strokeColor3").map_or("#c4c4c4", String::as_str));
            let close = escape(s.get("strokeColor2").map_or("#008cff", String::as_str));
            write!(out,"<rect width='{w}' height='{h}'/><g fill='none'><circle cx='{}' cy='15' r='10'/><circle cx='{}' cy='15' r='10'/><circle cx='{}' cy='15' r='10' stroke='{close}'/><g stroke='{inside}'><path d='M 0 40 H 30 V 15 A 5 5 0 0 1 35 10 H 170 A 5 5 0 0 1 175 15 V 40 H {w} M 0 110 H {w}'/><rect x='100' y='55' width='{}' height='35' rx='5'/></g></g>",w-65.,w-40.,w-15.,w-110.).unwrap();
            let text = s
                .get("mainText")
                .map_or("http://www.draw.io,Page 1", String::as_str)
                .split(',')
                .collect::<Vec<_>>();
            let color = escape(s.get("fontColor").map_or("#666666", String::as_str));
            for (x, y, text) in [
                (65, 25, text.get(1).copied().unwrap_or("")),
                (130, 73, text[0]),
            ] {
                write!(out,"<text x='{x}' y='{y}' font-size='17' dominant-baseline='central' stroke='none' fill='{color}'>{}</text>",escape(text)).unwrap();
            }
            for (x, y) in [(37, 17), (107, 64)] {
                write!(out,"<g fill='none' stroke='{inside}' transform='translate({x} {y})'><path d='M 0 0 H 11 L 15 4 V 18 H 0 Z M 11 0 V 4 L 15 5'/></g>").unwrap();
            }
            write!(out,"<g fill='{inside}' stroke='{inside}'><path transform='translate(12 64)' d='M 0 10 L 10 0 V 6 H 20 V 14 H 10 V 20 Z'/><path transform='translate(42 64)' d='M 20 10 L 10 0 V 6 H 0 V 14 H 10 V 20 Z'/><path transform='translate(72 64)' d='M 15.6 13.3 A 6 6 0 1 1 13.5 5.04 L 11.9 6.5 L 19.8 8.3 L 18 .8 L 16.3 2.4 A 9.8 9.8 0 1 0 18.4 16 Z'/></g>").unwrap();
        }
        "mxgraph.mockup.graphics.simpleicon" | "mxgraph.mockup.graphics.icongrid" => {
            let (nx, ny) = if name.ends_with("icongrid") {
                let size = s
                    .get("gridSize")
                    .map_or("3,3", String::as_str)
                    .split(',')
                    .map(|v| v.trim().parse::<usize>().unwrap_or(3).clamp(1, 32))
                    .collect::<Vec<_>>();
                (size[0], *size.get(1).unwrap_or(&3))
            } else {
                (1, 1)
            };
            let bw = w / (nx as f64 * 1.5 - 0.5);
            let bh = h / (ny as f64 * 1.5 - 0.5);
            for x in 0..nx {
                for y in 0..ny {
                    let x = x as f64 * bw * 1.5;
                    let y = y as f64 * bh * 1.5;
                    write!(out,"<rect x='{x}' y='{y}' width='{bw}' height='{bh}'/><path fill='none' d='M {x} {y} L {} {} M {x} {} L {} {y}'/>",x+bw,y+bh,y+bh,x+bw).unwrap();
                }
            }
        }
        "mxgraph.mockup.graphics.piechart" => {
            let parts = s
                .get("parts")
                .map_or("10,20,30", String::as_str)
                .split(',')
                .take(128)
                .map(|v| {
                    v.trim()
                        .parse::<f64>()
                        .ok()
                        .filter(|v| v.is_finite() && *v >= 0.)
                        .unwrap_or(0.)
                        .min(1e6)
                })
                .collect::<Vec<_>>();
            let colors = s
                .get("partColors")
                .map_or("#333333,#666666,#999999", String::as_str)
                .split(',')
                .collect::<Vec<_>>();
            let total = parts.iter().sum::<f64>();
            let mut begin = 0.;
            write!(
                out,
                "<ellipse cx='{}' cy='{}' rx='{}' ry='{}'/>",
                w / 2.,
                h / 2.,
                w / 2.,
                h / 2.
            )
            .unwrap();
            for (i, part) in parts.iter().enumerate() {
                if *part == 0. || total == 0. {
                    continue;
                }
                let span = part / total;
                let point = |t: f64| {
                    (
                        w / 2. - w / 2. * (t * std::f64::consts::TAU).sin(),
                        h / 2. - h / 2. * (t * std::f64::consts::TAU).cos(),
                    )
                };
                let (a, b) = point(begin);
                let (m, n) = point(begin + span / 2.);
                let (c, d) = point(begin + span);
                let fill = escape(colors.get(i).copied().unwrap_or("#ff0000"));
                write!(out,"<path fill='{fill}' d='M {} {} L {c} {d} A {} {} 0 0 1 {m} {n} A {} {} 0 0 1 {a} {b} Z'/>",w/2.,h/2.,w/2.,h/2.,w/2.,h/2.).unwrap();
                begin += span;
            }
        }
        "mxgraph.mockup.markup.curlybrace" => {
            let mid = h / 2.;
            let r = (w * 0.125).min(mid);
            write!(out,"<path fill='none' d='M 0 {} A {r} {r} 0 0 1 {r} {mid} H {} A {r} {r} 0 0 0 {} {} A {r} {r} 0 0 0 {} {mid} H {} A {r} {r} 0 0 1 {w} {}'/>",mid+r,w/2.-r,w/2.,mid-r,w/2.+r,w-r,mid+r).unwrap();
        }
        "mxgraph.bootstrap.topbutton" => {
            let r = number(s, "rSize", 10.)?.clamp(0., w.min(h) / 2.);
            write!(out,"<path d='M 0 {r} A {r} {r} 0 0 1 {r} 0 H {} A {r} {r} 0 0 1 {w} {r} V {h} H 0 Z'/>",w-r).unwrap();
        }
        "mxgraph.ios7ui.phone" => {
            write!(out,"<rect width='{w}' height='{h}' rx='25'/><g fill='none'><rect x='{}' y='{}' width='{}' height='{}'/><ellipse cx='{}' cy='{}' rx='{}' ry='{}'/><rect x='{}' y='{}' width='{}' height='{}' rx='{}' ry='{}'/><ellipse cx='{}' cy='{}' rx='{}' ry='{}'/><rect x='{}' y='{}' width='{}' height='{}' rx='{}'/></g>",w*0.0625,h*0.15,w*0.875,h*0.7,w*0.5,h*0.0475,w*0.0125,h*0.00625,w*0.375,h*0.075,w*0.25,h*0.01875,w*0.02,h*0.01,w*0.5,h*0.925,w*0.1,h*0.05,w*0.4575,h*0.905,w*0.085,h*0.04375,h*0.00625).unwrap();
        }
        "mxgraph.ios7ui.downloadbar" => {
            let pos = number(s, "barPos", 80.)?.clamp(0., 100.) / 100. * w;
            write!(out,"<path fill='none' stroke='{}' stroke-width='2' d='M 0 {} H {w}'/><path fill='none' stroke-width='2' d='M 0 {} H {pos}'/>",escape(s.get("fillColor").map_or("white",String::as_str)),h/2.,h/2.).unwrap();
            if let Some(text) = s.get("buttonText") {
                write!(out,"<text x='{}' y='{}' text-anchor='middle' dominant-baseline='central' fill='{}' stroke='none' font-family='{}' font-size='{}' font-weight='bold'>{}</text>",w/2.,h*0.2,escape(s.get("fontColor").map_or("black",String::as_str)),escape(s.get("fontFamily").map_or("Arial",String::as_str)),number(s,"fontSize",12.)?,escape(text)).unwrap();
            }
        }
        "mxgraph.infographic.shadedcube" => {
            let iso = (w
                * (number(s, "isoAngle", 15.)?.clamp(0.01, 94.) * std::f64::consts::PI / 200.)
                    .tan())
            .min(h / 2.);
            write!(out,"<path d='M {} 0 L {w} {iso} V {} L {} {h} L 0 {} V {iso} Z'/><path stroke='none' fill='black' fill-opacity='.2' d='M {} {} L {w} {iso} V {} L {} {h} Z'/><path stroke='none' fill='white' fill-opacity='.2' d='M {} {} L 0 {iso} V {} L {} {h} Z'/>",w/2.,h-iso,w/2.,h-iso,w/2.,iso*2.,h-iso,w/2.,w/2.,iso*2.,h-iso,w/2.).unwrap();
        }
        "mxgraph.infographic.bannersinglefold" => {
            let dy = number(s, "dy", 0.5)?.clamp(0., h / 2.).min(w / 2.);
            let dx = number(s, "dx", 0.5)?.clamp(0., (w - 2. * dy).max(0.));
            let dx2 = number(s, "dx2", 0.5)?.clamp(0., (w - dx - 2. * dy).max(0.));
            let notch = number(s, "notch", 0.5)?.clamp(0., dx);
            write!(out,"<path d='M {dx2} 0 H {} V {dy} H {w} L {} {} L {w} {h} H {} V {} H {dx2} L 0 {} Z'/><path stroke='none' fill='black' fill-opacity='.05' d='M {w} {dy} H {} V {} L {} {h} H {w} L {} {} Z'/><path stroke='none' fill='black' fill-opacity='.4' d='M {} {} H {} V {h} Z'/>",w-dx,w-notch,(h+dy)/2.,w-dx-2.*dy,h-dy,(h-dy)/2.,w-dx,h-dy,w-dx-2.*dy,w-notch,(h+dy)/2.,w-dx,h-dy,w-dx-2.*dy).unwrap();
        }
        "mxgraph.arrows2.uturnarrow" => {
            let head = number(s, "arrowHead", 40.)?.clamp(0., h);
            let dy = number(s, "dy", 0.5)?.clamp(0., head / 2.);
            let dx = (h - head / 2. + dy) / 2.;
            let length = number(s, "dx2", 25.)?.max(0.);
            let inner = (dx - 2. * dy).max(0.);
            write!(out,"<path d='M {dx} 0 L {} {} L {dx} {head} V {} A {inner} {inner} 0 0 0 {dx} {} H {} V {h} H {dx} A {dx} {dx} 0 0 1 {dx} {} Z'/>",dx+length,head/2.,head/2.+dy,h-2.*dy,w.max(dx),head/2.-dy).unwrap();
        }
        _ => return Err(error("Unsupported native dynamic shape")),
    }
    Ok(out)
}
