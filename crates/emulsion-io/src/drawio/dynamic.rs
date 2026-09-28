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
    "mxgraph.basic.arc", "mxgraph.basic.pie", "mxgraph.basic.partconcellipse",
    "mxgraph.infographic.partconcellipse", "mxgraph.infographic.ribbonsimple",
    "mxgraph.floorplan.stairs", "mxgraph.basic.rect", "mxgraph.bootstrap.rrect",
    "mxgraph.ios7ui.horlines", "mxgraph.atlassian.check", "mxgraph.atlassian.x",
    "mxgraph.mockup.forms.searchbox", "mxgraph.mockup.forms.combobox",
    "mxgraph.mockup.markup.line", "mxgraph.infographic.cylinder",

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
        "mxgraph.basic.rect"=>content=rect(0.,0.,w,h),
        "mxgraph.bootstrap.rrect"=>content=format!("<rect width='{w}' height='{h}' rx='{}'/>",number(s,"rSize",5.)?.clamp(0.,w.min(h)/2.)),
        "mxgraph.mockup.markup.line"=>content=format!("<path fill='none' d='M 0 0 L {w} {h}'/>"),
        "mxgraph.ios7ui.horlines"=>{
            content=String::new();let count=number(s,"lines",3.)?.clamp(1.,100.) as usize;
            for i in 0..count{write!(content,"<path fill='none' d='M 0 {} H {w}'/>",(i as f64+0.5)*h/count as f64).unwrap();}
        }
        "mxgraph.atlassian.check"=>content=format!("<path fill='none' d='M 0 {} L {} {h} L {w} 0'/>",h*0.5,w*0.3),
        "mxgraph.atlassian.x"=>content=format!("<path fill='none' d='M 0 0 L {w} {h} M {w} 0 L 0 {h}'/>"),
        "mxgraph.mockup.forms.searchbox" | "mxgraph.mockup.forms.combobox"=>{
            content=rect(0.,0.,w,h);
            let r=(h*0.2).min(w*0.1);let cx=w-h*0.5;let cy=h*0.45;
            if name.ends_with("searchbox") {write!(content,"<circle cx='{cx}' cy='{cy}' r='{r}' fill='none'/><path fill='none' d='M {} {} L {} {}'/>",cx+r*0.7,cy+r*0.7,cx+r*1.7,cy+r*1.7).unwrap();}
            else{write!(content,"<path fill='none' d='M {} 0 V {h} M {} {} L {cx} {} L {} {}'/>",w-h,w-h*0.75,h*0.4,h*0.65,w-h*0.25,h*0.4).unwrap();}
        }
        "mxgraph.floorplan.stairs"=>{
            content=rect(0.,0.,w,h);
            for i in 1..((w/25.).ceil() as usize).min(1000){write!(content,"<path fill='none' d='M {} 0 V {h}'/>",i as f64*25.).unwrap();}
            write!(content,"<path fill='none' d='M 0 {} H {w} M {} 0 L {w} {} L {} {h}'/>",h/2.,(w-25.).max(0.),h/2.,(w-25.).max(0.)).unwrap();
        }
        "mxgraph.infographic.ribbonsimple"=>{
            let a=number(s,"notch1",0.5)?.clamp(0.,w);let b=number(s,"notch2",0.5)?.clamp(0.,w);
            content=format!("<path d='M 0 {h} L {a} {} L 0 0 H {} L {w} {} L {} {h} Z'/>",h/2.,w-b,h/2.,w-b);
        }
        "mxgraph.infographic.cylinder"=>{
            let r=number(s,"size",0.15)?.clamp(0.,0.5)*h;
            content=format!("<path d='M 0 {r} A {} {r} 0 0 1 {w} {r} V {} A {} {r} 0 0 1 0 {} Z'/><ellipse cx='{}' cy='{r}' rx='{}' ry='{r}'/>",w/2.,h-r,w/2.,h-r,w/2.,w/2.);
        }
        "mxgraph.basic.arc" | "mxgraph.basic.pie" | "mxgraph.basic.partconcellipse" | "mxgraph.infographic.partconcellipse"=>{
            let start=number(s,"startAngle",0.25)?.clamp(0.,1.);
            let end=number(s,"endAngle",0.75)?.clamp(0.,1.);
            let span=if (end-start).abs()>=1. {1.}else{(end-start).rem_euclid(1.)};
            let point=|t:f64,scale:f64|{let a=t*std::f64::consts::TAU;(w/2.+a.sin()*w/2.*scale,h/2.-a.cos()*h/2.*scale)};
            let (a,b)=point(start,1.);let (mx,my)=point(start+span/2.,1.);let (c,d)=point(start+span,1.);
            let outer=format!("M {a} {b} A {} {} 0 0 1 {mx} {my} A {} {} 0 0 1 {c} {d}",w/2.,h/2.,w/2.,h/2.);
            if name.ends_with(".arc"){content=format!("<path fill='none' d='{outer}'/>");}
            else if name.ends_with(".pie"){content=format!("<path d='{outer} L {} {} Z'/>",w/2.,h/2.);}
            else{let inner=(1.-number(s,"arcWidth",0.5)?.clamp(0.,1.)).max(0.00001);let (a,b)=point(start,inner);let (mx,my)=point(start+span/2.,inner);let (c,d)=point(start+span,inner);
                content=format!("<path d='{outer} L {c} {d} A {} {} 0 0 0 {mx} {my} A {} {} 0 0 0 {a} {b} Z'/>",w/2.*inner,h/2.*inner,w/2.*inner,h/2.*inner);
            }
        }

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
                "conditional"=>{
                    write!(content,"<rect x='{}' y='{}' width='{}' height='{}' fill='none'/>",w*0.32,h*0.26,w*0.36,h*0.48).unwrap();
                    for y in [0.38,0.5,0.62]{write!(content,"<path d='M {} {} H {}'/>",w*0.38,h*y,w*0.62).unwrap();}
                }
                "escalation"=>write!(content,"<path d='M {} {} L {} {} L {} {} L {} {} Z'/>",w*0.5,h*0.25,w*0.72,h*0.72,w*0.5,h*0.55,w*0.28,h*0.72).unwrap(),
                "error"=>write!(content,"<path d='M {} {} L {} {} L {} {} L {} {} L {} {} L {} {} Z'/>",w*0.6,h*0.25,w*0.3,h*0.54,w*0.48,h*0.51,w*0.4,h*0.75,w*0.7,h*0.46,w*0.52,h*0.49).unwrap(),
                "multiple"=>{
                    let pts=(0..5).map(|i|{let a=(i as f64*72.-90.).to_radians();format!("{},{}",w*(0.5+0.26*a.cos()),h*(0.5+0.26*a.sin()))}).collect::<Vec<_>>().join(" ");
                    write!(content,"<polygon points='{pts}'/>").unwrap();
                }
                "terminate2"=>write!(content,"<ellipse cx='{}' cy='{}' rx='{}' ry='{}' fill='currentColor'/>",w*0.5,h*0.5,w*0.25,h*0.25).unwrap(),
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
