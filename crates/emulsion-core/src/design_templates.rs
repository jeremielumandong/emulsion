//! User-supplied starter artwork, translated to editable native objects.
use super::*;
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Deserialize)]
pub(super) struct Starter {
    id: String,
    pub label: String,
    pub preset: String,
    pub w: u32,
    pub h: u32,
    bg: String,
    els: Vec<Item>,
}
#[derive(Deserialize)]
struct Item {
    kind: String,
    name: String,
    x: f64,
    y: f64,
    w: f64,
    #[serde(default)]
    h: f64,
    #[serde(default)]
    shape: String,
    #[serde(default)]
    fill: String,
    #[serde(default)]
    radius: f64,
    #[serde(default)]
    text: String,
    #[serde(default)]
    color: String,
    #[serde(default)]
    size: f64,
    #[serde(default)]
    mono: bool,
    #[serde(default)]
    bold: bool,
    #[serde(default)]
    italic: bool,
    #[serde(default)]
    align: crate::text::Align,
}
pub(super) fn spec(template: Template) -> Option<&'static Starter> {
    let id = match template {
        Template::ProductLaunch => "launch",
        Template::SeasonSale => "sale",
        Template::Event => "event",
        Template::Quote => "quote",
        Template::Presentation => "presentation",
        Template::BusinessCard => "card",
        Template::Resume => "resume",
        Template::DinnerMenu => "menu",
        Template::VideoThumbnail => "thumbnail",
        Template::PhotoCollage => "collage",
        Template::Bundled(index) => return starters().get(index),
        Template::Announcement | Template::Editorial | Template::Responsive(_) => return None,
    };
    starters().iter().find(|spec| spec.id == id)
}
pub(super) fn starters() -> &'static [Starter] {
    static STARTERS: OnceLock<Vec<Starter>> = OnceLock::new();
    STARTERS.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/design-starters.json"))
            .expect("validated bundled Design starters")
    })
}
fn color(hex: &str) -> [u8; 4] {
    let mut hex = hex.trim_start_matches('#').to_string();
    if hex.len() == 3 {
        hex = hex.chars().flat_map(|c| [c, c]).collect();
    }
    let value = u32::from_str_radix(&hex, 16).expect("bundled starter color");
    [(value >> 16) as u8, (value >> 8) as u8, value as u8, 255]
}
fn shape(kind: &str, x: f64, y: f64, w: f64, h: f64, radius: f64) -> Path {
    if kind == "rect" && radius > 0. {
        let r = (radius * h).min(w / 2.).min(h / 2.);
        let k = r * 0.5522847498;
        return Path::from_svg(&format!(
            "M {} {y} H {} C {} {y} {} {} {} {} V {} C {} {} {} {} {} {} H {} C {} {} {x} {} {x} {} V {} C {x} {} {} {y} {} {y} Z",
            x+r,x+w-r,x+w-r+k,x+w,y+r-k,x+w,y+r,
            y+h-r,x+w,y+h-r+k,x+w-r+k,y+h,x+w-r,y+h,
            x+r,x+r-k,y+h,y+h-r+k,y+h-r,y+r,y+r-k,x+r-k,x+r
        )).expect("finite starter rounded rectangle");
    }
    let points: Option<&[(f64, f64)]> = match kind {
        "star" => Some(&[
            (0.5, 0.),
            (0.61, 0.35),
            (0.98, 0.35),
            (0.68, 0.57),
            (0.79, 0.91),
            (0.5, 0.7),
            (0.21, 0.91),
            (0.32, 0.57),
            (0.02, 0.35),
            (0.39, 0.35),
        ]),
        "arrow" => Some(&[
            (0., 0.32),
            (0.58, 0.32),
            (0.58, 0.),
            (1., 0.5),
            (0.58, 1.),
            (0.58, 0.68),
            (0., 0.68),
        ]),
        _ => None,
    };
    if let Some(points) = points {
        return Path {
            subpaths: vec![SubPath {
                anchors: points
                    .iter()
                    .map(|(a, b)| Anchor::corner((x + a * w, y + b * h)))
                    .collect(),
                closed: true,
            }],
        };
    }
    match kind {
        "circle" => Element::Circle,
        "triangle" => Element::Triangle,
        "diamond" => Element::Diamond,
        // The handoff's rules are filled rectangles, including subpixel heights.
        "rect" | "line" => Element::Rectangle,
        _ => unreachable!("validated bundled shape"),
    }
    .path(x, y, w, h)
}
impl Starter {
    pub fn create(&self, w: u32, h: u32) -> Result<Document, String> {
        let mut doc = crate::creation::CanvasSpec {
            width: w as f64,
            height: h as f64,
            ..Default::default()
        }
        .create()?;
        doc.nodes[0].kind = NodeKind::Fill {
            rgba: color(&self.bg),
        };
        for item in &self.els {
            let (x, y, width, height) = (
                item.x * w as f64,
                item.y * h as f64,
                item.w * w as f64,
                item.h * h as f64,
            );
            let node = if item.kind == "text" {
                Node::text(
                    0,
                    &item.name,
                    TextSpec {
                        text: item.text.clone(),
                        font: if item.mono { "Geist Mono" } else { "Geist" }.into(),
                        size: (item.size * w.min(h) as f64) as f32,
                        line_height: 1.1,
                        color: color(&item.color),
                        bold: item.bold,
                        italic: item.italic,
                        align: item.align,
                        x: x as f32,
                        y: y as f32,
                        width: Some(width as f32),
                        ..Default::default()
                    },
                    w,
                    h,
                )
            } else {
                let frame = item.kind == "frame";
                let mut boundary = Node::path(
                    0,
                    if frame { "Frame boundary" } else { &item.name },
                    Arc::new(shape(
                        if frame { &item.shape } else { &item.kind },
                        x,
                        y,
                        width,
                        height,
                        item.radius,
                    )),
                    PathStyle {
                        fill: Some(color(&item.fill)),
                        stroke: None,
                        ..Default::default()
                    },
                    w,
                    h,
                );
                if frame {
                    // A genuine media frame: replacement and fitting use the same
                    // clipping boundary as frames authored in the Elements drawer.
                    let group = Command::AddNode {
                        node: Box::new(Node::group(0, &item.name)),
                        slot: Slot::TOP,
                    }
                    .apply(&mut doc)
                    .map_err(|e| e.to_string())?
                    .ok_or("Missing frame group")?;
                    boundary.parent = Some(group);
                    let base = Command::AddNode {
                        node: Box::new(boundary),
                        slot: Slot::top_of(Some(group)),
                    }
                    .apply(&mut doc)
                    .map_err(|e| e.to_string())?
                    .ok_or("Missing frame boundary")?;
                    // Solid vector stripes share the clipping base. Keep both
                    // the viewport and SVG/PDF exports free of pattern bitmaps.
                    let stripe = (w as f64 * 0.012).max(2.) * std::f64::consts::SQRT_2;
                    let mut path = Path::default();
                    let mut start = -height;
                    while start < width {
                        path.subpaths.push(SubPath {
                            anchors: [
                                (start, y),
                                (start + stripe, y),
                                (start + stripe + height, y + height),
                                (start + height, y + height),
                            ]
                            .into_iter()
                            .map(|(a, b)| Anchor::corner((x + a, b)))
                            .collect(),
                            closed: true,
                        });
                        start += 2. * stripe;
                    }
                    if let NodeKind::Path { path: outline, .. } = &doc.node(base).unwrap().kind {
                        path = emulsion_raster::vector_geometry::boolean(
                            &path,
                            outline,
                            emulsion_raster::vector_geometry::BooleanOp::Intersect,
                        )?;
                    }
                    let base_color = color(&item.fill);
                    let ink = [
                        base_color[0].saturating_add(24),
                        base_color[1].saturating_add(24),
                        base_color[2].saturating_add(24),
                        255,
                    ];
                    let stripes = Node::path(
                        0,
                        "Photo placeholder",
                        Arc::new(path),
                        PathStyle {
                            fill: Some(ink),
                            stroke: None,
                            ..Default::default()
                        },
                        w,
                        h,
                    );
                    let stripes = Command::AddNode {
                        node: Box::new(stripes),
                        slot: Slot::top_of(Some(group)),
                    }
                    .apply(&mut doc)
                    .map_err(|e| e.to_string())?
                    .ok_or("Missing placeholder stripes")?;
                    Command::SetClip {
                        id: stripes,
                        clip_to: Some(base),
                    }
                    .apply(&mut doc)
                    .map_err(|e| e.to_string())?;
                    continue;
                }
                boundary
            };
            Command::AddNode {
                node: Box::new(node),
                slot: Slot::TOP,
            }
            .apply(&mut doc)
            .map_err(|e| e.to_string())?;
        }
        doc.validate().map_err(|e| e.to_string())?;
        Ok(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn updated_catalog_has_ten_unique_templates_in_each_category() {
        let starters = starters();
        assert_eq!(starters.len(), 110);
        let mut ids = std::collections::HashSet::new();
        for (index, category) in Template::CATEGORIES.iter().enumerate() {
            assert_eq!(
                Template::catalog()
                    .filter(|t| t.category() == Some(index))
                    .count(),
                10,
                "{}",
                category.label
            );
        }
        for (template, spec) in Template::catalog().zip(starters) {
            assert!(ids.insert(&spec.id), "duplicate {}", spec.id);
            assert_eq!(template.label(), spec.label);
            assert_eq!(template.native_size(), (spec.w, spec.h));
            assert!(template.category().is_some());
        }
    }
    #[test]
    fn supplied_starters_preserve_artwork_geometry_text_and_editable_frames() {
        for (template, spec) in
            Template::catalog().filter_map(|template| spec(template).map(|spec| (template, spec)))
        {
            let doc = template.create(spec.w, spec.h).unwrap();
            assert_eq!(doc.children(None).len(), spec.els.len() + 1);
            assert_eq!(
                doc.nodes[0].kind,
                NodeKind::Fill {
                    rgba: color(&spec.bg)
                }
            );
            for item in &spec.els {
                let node = doc.nodes.iter().find(|n| n.name == item.name).unwrap();
                if item.kind == "text" {
                    let NodeKind::Text { spec: text, .. } = &node.kind else {
                        panic!("flattened text")
                    };
                    assert_eq!(text.text, item.text);
                    assert_eq!(text.align, item.align);
                    assert_eq!(text.line_height, 1.1);
                    assert_eq!(text.x, (item.x * spec.w as f64) as f32);
                    assert_eq!(text.size, (item.size * spec.w.min(spec.h) as f64) as f32);
                } else if item.kind == "frame" {
                    assert!(frame_parts(&doc, node.id).is_some());
                    let mut editor = crate::Editor::new(doc.clone(), None);
                    let image = place_in_frame(
                        &mut editor,
                        node.id,
                        Arc::new(emulsion_raster::Raster::solid(12, 8, [1.; 4])),
                    )
                    .unwrap();
                    assert!(editor.doc.node(image).unwrap().clip_to.is_some());
                    assert!(editor.undo());
                    assert_eq!(editor.doc, doc);
                }
            }
        }
    }
}
