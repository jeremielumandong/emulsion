//! Pinned draw.io stencil definitions interpreted as data, never executable code.
use super::*;
use std::{fmt::Write as FmtWrite, sync::OnceLock};
fn catalog() -> &'static BTreeMap<String, String> {
    static DATA: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    DATA.get_or_init(|| {
        let mut json = String::new();
        flate2::read::GzDecoder::new(
            &include_bytes!("../../../../assets/diagram-stencils/drawio.json.gz")[..],
        )
        .take(64 << 20)
        .read_to_string(&mut json)
        .expect("bundled stencil data");
        serde_json::from_str::<BTreeMap<String, String>>(&json)
            .expect("bundled stencil catalog")
            .into_iter()
            .map(|(key, value)| (key.to_ascii_lowercase(), value))
            .collect()
    })
}
pub fn names() -> impl Iterator<Item = &'static str> {
    catalog().keys().map(String::as_str)
}
pub fn contains(name: &str) -> bool {
    catalog().contains_key(&name.to_ascii_lowercase()) || super::dynamic::supports(name)
}
#[derive(Clone)]
struct Paint {
    fill: String,
    stroke: String,
    width: f64,
    alpha: f64,
    fill_alpha: f64,
    stroke_alpha: f64,
    dash: String,
    cap: String,
    join: String,
    font: String,
    size: f64,
    font_color: String,
    bold: bool,
    italic: bool,
}
fn attr(a: &BTreeMap<String, String>, key: &str, default: &str) -> String {
    a.get(key).cloned().unwrap_or_else(|| default.into())
}
fn body(
    xml: &str,
    style: &BTreeMap<String, String>,
    warnings: &mut BTreeSet<String>,
    depth: usize,
) -> Result<(String, f64, f64)> {
    if depth > 8 || xml.len() > 2 << 20 {
        return Err(error("Stencil include/size limit exceeded"));
    }
    let mut reader = Reader::from_str(xml);
    let mut svg = String::new();
    let mut path = String::new();
    let (mut width, mut height) = (100., 100.);
    let mut p = Paint {
        fill: attr(style, "fillColor", "#ffffff"),
        stroke: attr(style, "strokeColor", "#000000"),
        width: number(style, "strokeWidth", 1.)?,
        alpha: 1.,
        fill_alpha: 1.,
        stroke_alpha: 1.,
        dash: String::new(),
        cap: "butt".into(),
        join: "miter".into(),
        font: attr(style, "fontFamily", "Arial"),
        size: number(style, "fontSize", 12.)?,
        font_color: attr(style, "fontColor", "#000000"),
        bold: false,
        italic: false,
    };
    let mut stack = Vec::new();
    let mut count = 0;
    loop {
        match reader.read_event().map_err(|e| error(e.to_string()))? {
            Event::Start(e) | Event::Empty(e) => {
                count += 1;
                if count > 32768 {
                    return Err(error("Stencil instruction limit exceeded"));
                }
                let a = attributes(&e)?;
                let n = |k, d| number(&a, k, d);
                let name = e.name();
                match name.as_ref() {
                    "shape" => {
                        width = n("w", 100.)?;
                        height = n("h", 100.)?;
                        if width <= 0. || height <= 0. {
                            return Err(error("Invalid stencil dimensions"));
                        }
                    }
                    "path" => path.clear(),
                    "move" => write!(path, "M {} {} ", n("x", 0.)?, n("y", 0.)?).unwrap(),
                    "line" => write!(path, "L {} {} ", n("x", 0.)?, n("y", 0.)?).unwrap(),
                    "curve" => write!(
                        path,
                        "C {} {} {} {} {} {} ",
                        n("x1", 0.)?,
                        n("y1", 0.)?,
                        n("x2", 0.)?,
                        n("y2", 0.)?,
                        n("x3", 0.)?,
                        n("y3", 0.)?
                    )
                    .unwrap(),
                    "quad" => write!(
                        path,
                        "Q {} {} {} {} ",
                        n("x1", 0.)?,
                        n("y1", 0.)?,
                        n("x2", 0.)?,
                        n("y2", 0.)?
                    )
                    .unwrap(),
                    "arc" => write!(
                        path,
                        "A {} {} {} {} {} {} {} ",
                        n("rx", 0.)?,
                        n("ry", 0.)?,
                        n("x-axis-rotation", 0.)?,
                        u8::from(n("large-arc-flag", 0.)? != 0.),
                        u8::from(n("sweep-flag", 0.)? != 0.),
                        n("x", 0.)?,
                        n("y", 0.)?
                    )
                    .unwrap(),
                    "close" => path.push_str("Z "),
                    "rect" | "roundrect" => {
                        let (x, y, w, h) = (n("x", 0.)?, n("y", 0.)?, n("w", 0.)?, n("h", 0.)?);
                        let r = if name.as_ref() == "roundrect" {
                            w.min(h) * n("arcsize", 15.)? / 100.
                        } else {
                            0.
                        };
                        path = format!(
                            "M {} {y} H {} Q {} {y} {} {} V {} Q {} {} {} {} H {} Q {x} {} {x} {} V {} Q {x} {y} {} {y} Z",
                            x + r,
                            x + w - r,
                            x + w,
                            x + w,
                            y + r,
                            y + h - r,
                            x + w,
                            y + h,
                            x + w - r,
                            y + h,
                            x + r,
                            y + h,
                            y + h - r,
                            y + r,
                            x + r
                        );
                    }
                    "ellipse" => {
                        let (x, y, w, h) = (n("x", 0.)?, n("y", 0.)?, n("w", 0.)?, n("h", 0.)?);
                        path = format!(
                            "M {} {} A {} {} 0 1 1 {} {} A {} {} 0 1 1 {} {} Z",
                            x + w,
                            y + h / 2.,
                            w / 2.,
                            h / 2.,
                            x,
                            y + h / 2.,
                            w / 2.,
                            h / 2.,
                            x + w,
                            y + h / 2.
                        );
                    }
                    "fill" | "stroke" | "fillstroke" => {
                        let fill = if name.as_ref() == "stroke" {
                            "none"
                        } else {
                            &p.fill
                        };
                        let stroke = if name.as_ref() == "fill" {
                            "none"
                        } else {
                            &p.stroke
                        };
                        write!(svg,"<path d=\"{}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\" opacity=\"{}\" fill-opacity=\"{}\" stroke-opacity=\"{}\" stroke-linecap=\"{}\" stroke-linejoin=\"{}\" stroke-dasharray=\"{}\"/>",escape(&path),escape(fill),escape(stroke),p.width,p.alpha,p.fill_alpha,p.stroke_alpha,escape(&p.cap),escape(&p.join),if p.dash.is_empty(){"none"}else{&p.dash}).unwrap();
                    }
                    "save" => stack.push(p.clone()),
                    "restore" => {
                        if let Some(old) = stack.pop() {
                            p = old;
                        }
                    }
                    "fillcolor" => p.fill = attr(&a, "color", &p.fill),
                    "strokecolor" => p.stroke = attr(&a, "color", &p.stroke),
                    "strokewidth" => p.width = n("width", p.width)?,
                    "alpha" => p.alpha = n("alpha", 1.)?,
                    "fillalpha" => p.fill_alpha = n("alpha", 1.)?,
                    "strokealpha" => p.stroke_alpha = n("alpha", 1.)?,
                    "linecap" => p.cap = attr(&a, "cap", "butt"),
                    "linejoin" => p.join = attr(&a, "join", "miter"),
                    "dashed" => {
                        p.dash = if n("dashed", 0.)? == 0. {
                            String::new()
                        } else {
                            "3 3".into()
                        }
                    }
                    "dashpattern" => p.dash = attr(&a, "pattern", "3 3"),
                    "fontcolor" => p.font_color = attr(&a, "color", &p.font_color),
                    "fontsize" => p.size = n("size", p.size)?,
                    "fontfamily" => p.font = attr(&a, "family", &p.font),
                    "fontstyle" => {
                        let flags = n("style", 0.)? as u32;
                        p.bold = flags & 1 != 0;
                        p.italic = flags & 2 != 0;
                    }
                    "text" => {
                        write!(svg,"<text x=\"{}\" y=\"{}\" font-family=\"{}\" font-size=\"{}\" fill=\"{}\" font-weight=\"{}\" font-style=\"{}\" text-anchor=\"{}\">{}</text>",n("x",0.)?,n("y",0.)?,escape(&p.font),p.size,escape(&p.font_color),if p.bold{"bold"}else{"normal"},if p.italic{"italic"}else{"normal"},match a.get("align").map(String::as_str){Some("center")=>"middle",Some("right")=>"end",_=>"start"},escape(&attr(&a,"str",""))).unwrap();
                    }
                    "include-shape" => {
                        let key = attr(&a, "name", "");
                        if let Some(source) = catalog().get(&key.to_ascii_lowercase()) {
                            let (inner, iw, ih) = body(source, style, warnings, depth + 1)?;
                            write!(
                                svg,
                                "<g transform=\"translate({} {}) scale({} {})\">{inner}</g>",
                                n("x", 0.)?,
                                n("y", 0.)?,
                                n("w", iw)? / iw,
                                n("h", ih)? / ih
                            )
                            .unwrap();
                        } else {
                            warnings.insert(format!("Missing nested stencil {key}"));
                        }
                    }
                    "foreground" | "background" | "connections" | "constraint" | "miterlimit" => {}
                    other => {
                        warnings.insert(format!(
                            "Stencil instruction {other} needs appearance review"
                        ));
                    }
                }
            }
            Event::DocType(_) => return Err(error("Stencil DTDs are not supported")),
            Event::Eof => break,
            _ => {}
        }
    }
    Ok((svg, width, height))
}
pub fn svg(
    name: &str,
    style: &BTreeMap<String, String>,
    warnings: &mut BTreeSet<String>,
) -> Result<String> {
    svg_at(name, style, 100., 100., warnings)
}
pub(crate) fn svg_at(
    name: &str,
    style: &BTreeMap<String, String>,
    w: f64,
    h: f64,
    warnings: &mut BTreeSet<String>,
) -> Result<String> {
    if super::dynamic::supports(name) {
        return super::dynamic::svg(name, style, w, h, warnings);
    }
    let xml = catalog()
        .get(&name.to_ascii_lowercase())
        .ok_or_else(|| error(format!("Unknown stencil {name}")))?;
    let (body, iw, ih) = body(xml, style, warnings, 0)?;
    let fixed =
        xml.contains("aspect=\"fixed\"") || style.get("aspect").is_some_and(|v| v == "fixed");
    let (sx, sy) = if fixed {
        let scale = (w / iw).min(h / ih);
        (scale, scale)
    } else {
        (w / iw, h / ih)
    };
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\"><g transform=\"translate({} {}) scale({sx} {sy})\">{body}</g></svg>",
        (w - iw * sx) / 2.,
        (h - ih * sy) / 2.
    ))
}
pub(super) fn inline_svg(
    xml: &str,
    style: &BTreeMap<String, String>,
    warnings: &mut BTreeSet<String>,
) -> Result<String> {
    let (body, w, h) = body(xml, style, warnings, 0)?;
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\">{body}</svg>"
    ))
}
