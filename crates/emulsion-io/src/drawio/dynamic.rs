//! Parameterized geometry for common draw.io shapes that are not XML stencils.
//! Floor-plan dimensions follow jgraph/drawio mxFloorplan.js (Apache-2.0);
//! pinned upstream attribution is distributed with assets/diagram-stencils.
use super::*;
use std::fmt::Write as FmtWrite;
pub(super) const NAMES: &[&str] = &[
    "mxgraph.floorplan.wall",
    "mxgraph.floorplan.wallcorner",
    "mxgraph.floorplan.wallu",
    "mxgraph.floorplan.room",
    "mxgraph.floorplan.window",
    "mxgraph.floorplan.doorleft",
    "mxgraph.floorplan.doorright",
    "mxgraph.floorplan.doordouble",
    "mxgraph.bpmn.shape",
    "mxgraph.bpmn.event",
    "mxgraph.bpmn.gateway2",
    "mxgraph.bpmn.task",
    "mxgraph.arrows2.arrow",
];
pub(super) fn supports(name: &str) -> bool {
    NAMES.contains(&name.to_ascii_lowercase().as_str())
}
pub(super) fn svg(
    name: &str,
    s: &BTreeMap<String, String>,
    w: f64,
    h: f64,
    warnings: &mut BTreeSet<String>,
) -> Result<String> {
    let mut content: String;
    let name = name.to_ascii_lowercase();
    let t = number(s, "wallThickness", 10.)?.clamp(0., w.min(h));
    let rect = |x: f64, y: f64, w: f64, h: f64| {
        format!("<rect x='{x}' y='{y}' width='{w}' height='{h}'/>")
    };
    match name.as_str() {
        "mxgraph.floorplan.wall" => content = rect(0., h / 2. - t / 2., w, t),
        "mxgraph.floorplan.window" => {
            content = format!(
                "{}<path fill='none' d='M 0 {} H {w}'/>",
                rect(0., h / 2. - t / 2., w, t),
                h / 2.
            )
        }
        "mxgraph.floorplan.wallcorner" => {
            content = format!("<path d='M 0 {h} V 0 H {w} V {t} H {t} V {h} Z'/>")
        }
        "mxgraph.floorplan.wallu" => {
            content = format!(
                "<path d='M 0 {h} V 0 H {w} V {h} H {} V {t} H {t} V {h} Z'/>",
                w - t
            )
        }
        "mxgraph.floorplan.room" => {
            content = format!(
                "<path fill-rule='evenodd' d='M 0 0 H {w} V {h} H 0 Z M {t} {t} H {} V {} H {t} Z'/>",
                w - t,
                h - t
            )
        }
        "mxgraph.floorplan.doorleft"
        | "mxgraph.floorplan.doorright"
        | "mxgraph.floorplan.doordouble" => {
            let double = name.ends_with("doordouble");
            let dw = if double { w / 2. } else { w };
            let left = format!("<path fill='none' d='M 0 {h} V 0 A {dw} {h} 0 0 1 {dw} {h}'/>");
            content = if name.ends_with("doorright") {
                format!("<g transform='translate({w} 0) scale(-1 1)'>{left}</g>")
            } else {
                left.clone()
            };
            if double {
                write!(
                    content,
                    "<g transform='translate({w} 0) scale(-1 1)'>{left}</g>"
                )
                .unwrap();
            }
        }
        "mxgraph.arrows2.arrow" => {
            let head = number(s, "arrowHeadWidth", 0.3)?.clamp(0.05, 0.9) * w;
            let half = number(s, "arrowWidth", 0.4)?.clamp(0.05, 1.) * h / 2.;
            content = format!(
                "<path d='M 0 {} H {} V 0 L {w} {} L {} {h} V {} H 0 Z'/>",
                h / 2. - half,
                w - head,
                h / 2.,
                w - head,
                h / 2. + half
            );
        }
        "mxgraph.bpmn.task" => {
            content = format!("<rect width='{w}' height='{h}' rx='{}'/>", w.min(h) * 0.12)
        }
        "mxgraph.bpmn.shape" | "mxgraph.bpmn.event" | "mxgraph.bpmn.gateway2" => {
            let gateway=name.ends_with("gateway2");
            let outline = if gateway {"gateway"} else {s.get("outline").map_or("standard", String::as_str)};
            let ellipse = |inset: f64| {
                format!(
                    "<ellipse cx='{}' cy='{}' rx='{}' ry='{}'/>",
                    w / 2.,
                    h / 2.,
                    (w / 2. - inset).max(0.1),
                    (h / 2. - inset).max(0.1)
                )
            };
            content = match outline {
                "none" => String::new(),
                "gateway" => format!(
                    "<path d='M {} 0 L {w} {} L {} {h} L 0 {} Z'/>",
                    w / 2.,
                    h / 2.,
                    w / 2.,
                    h / 2.
                ),
                "end" => format!(
                    "<g stroke-width='{}'>{}</g>",
                    number(s, "strokeWidth", 1.)? * 3.,
                    ellipse(0.)
                ),
                "eventInt" | "eventNonint" => format!(
                    "{}<g fill='none'>{}</g>",
                    ellipse(0.),
                    ellipse(w.min(h) * 0.09)
                ),
                _ => ellipse(0.),
            };
            let symbol = if gateway {
                match s.get("gwType").map(String::as_str) {
                    Some("parallel")=>"parallelGw", Some("inclusive")=>"inclusiveGw",
                    _=>s.get("symbol").map_or("general",String::as_str),
                }
            } else {s.get("symbol").map_or("general", String::as_str)};
            content.push_str("<g fill='none'>");
            match symbol {
                "general" | "none" => {}
                "terminate" => write!(content,"<ellipse cx='{}' cy='{}' rx='{}' ry='{}' fill='{}'/>",w/2.,h/2.,w*0.32,h*0.32,escape(s.get("strokeColor").map_or("black",String::as_str))).unwrap(),
                "message" => write!(
                    content,
                    "{}<path d='M {} {} L {} {} L {} {}'/>",
                    rect(w * 0.23, h * 0.32, w * 0.54, h * 0.36),
                    w * 0.23,
                    h * 0.32,
                    w * 0.5,
                    h * 0.5,
                    w * 0.77,
                    h * 0.32
                )
                .unwrap(),
                "timer" => {
                    content.push_str(&ellipse(w.min(h) * 0.2));
                    write!(
                        content,
                        "<path d='M {} {} V {} L {} {}'/>",
                        w * 0.5,
                        h * 0.28,
                        h * 0.5,
                        w * 0.68,
                        h * 0.55
                    )
                    .unwrap();
                }
                "exclusiveGw" | "parallelGw" | "inclusiveGw" => {
                    if symbol == "inclusiveGw" {
                        content.push_str(&ellipse(w.min(h) * 0.3));
                    } else if symbol == "parallelGw" {
                        write!(
                            content,
                            "<path d='M {} {} H {} M {} {} V {}'/>",
                            w * 0.3,
                            h * 0.5,
                            w * 0.7,
                            w * 0.5,
                            h * 0.3,
                            h * 0.7
                        )
                        .unwrap();
                    } else {
                        write!(
                            content,
                            "<path d='M {} {} L {} {} M {} {} L {} {}'/>",
                            w * 0.3,
                            h * 0.3,
                            w * 0.7,
                            h * 0.7,
                            w * 0.7,
                            h * 0.3,
                            w * 0.3,
                            h * 0.7
                        )
                        .unwrap();
                    }
                }
                _ => {
                    warnings.insert(format!("BPMN symbol {symbol} needs appearance review"));
                }
            }
            content.push_str("</g>");
        }
        _ => return Err(error("Unsupported parameterized stencil")),
    }
    Ok(format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='{w}' height='{h}'><g fill='{}' stroke='{}' stroke-width='{}'>{content}</g></svg>",
        escape(s.get("fillColor").map_or("white", String::as_str)),
        escape(s.get("strokeColor").map_or("black", String::as_str)),
        number(s, "strokeWidth", 1.)?
    ))
}
