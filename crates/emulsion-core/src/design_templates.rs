//! User-supplied starter artwork, translated to editable native objects.
use super::*;
use crate::styles::LayerStyle;
use emulsion_raster::vector::{PathPaint, StrokeAlignment};
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
    #[serde(default = "opaque")]
    opacity: f32,
    #[serde(default)]
    rotation: f64,
    #[serde(default)]
    gradient: Option<Gradient>,
    #[serde(default)]
    stroke: Option<Stroke>,
    #[serde(default)]
    shadow: Option<Shadow>,
    #[serde(default)]
    spacing: f64,
    #[serde(default = "line_height", rename = "lineHeight")]
    line_height: f32,
    #[serde(default)]
    underline: bool,
    #[serde(default)]
    outline: Option<Stroke>,
    #[serde(default)]
    backdrop: Option<Backdrop>,
    #[serde(default)]
    curve: crate::text_effects::TextWarp,
}
fn opaque() -> f32 {
    1.
}
fn line_height() -> f32 {
    1.1
}
fn gradient_angle() -> f32 {
    180.
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Gradient {
    kind: String,
    to: String,
    #[serde(default = "gradient_angle")]
    angle: f32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Stroke {
    color: String,
    width: f64,
    #[serde(default)]
    align: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Shadow {
    color: String,
    x: f64,
    y: f64,
    blur: f64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Backdrop {
    fill: String,
    pad_x: f64,
    pad_y: f64,
    radius: f64,
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
    if let Some(rgba) = hex.strip_prefix("rgba(").and_then(|s| s.strip_suffix(')')) {
        let values: Vec<f32> = rgba
            .split(',')
            .map(|v| v.trim().parse().expect("bundled rgba component"))
            .collect();
        assert_eq!(values.len(), 4);
        return [
            values[0] as u8,
            values[1] as u8,
            values[2] as u8,
            (values[3] * 255.).round() as u8,
        ];
    }
    let mut hex = hex.trim_start_matches('#').to_string();
    if hex.len() == 3 {
        hex = hex.chars().flat_map(|c| [c, c]).collect();
    }
    let value = u32::from_str_radix(&hex, 16).expect("bundled starter color");
    [(value >> 16) as u8, (value >> 8) as u8, value as u8, 255]
}
fn shape(kind: &str, x: f64, y: f64, w: f64, h: f64, radius: f64) -> Path {
    if kind == "rect" && radius > 0. {
        return crate::design_formatting::rounded_rect(x, y, w, h, radius);
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
        "heart" => Element::Heart,
        // The handoff's rules are filled rectangles, including subpixel heights.
        "rect" | "line" => Element::Rectangle,
        _ => unreachable!("validated bundled shape"),
    }
    .path(x, y, w, h)
}
fn radius(value: f64, unit: f64, width: f64, height: f64) -> f64 {
    if value >= 0.5 {
        width.min(height) / 2.
    } else {
        value * unit
    }
}
impl Item {
    fn path_style(&self, unit: f64) -> PathStyle {
        PathStyle {
            fill: (self.fill != "none").then(|| color(&self.fill)),
            fill_paint: self.gradient.as_ref().map_or(PathPaint::Solid, |g| {
                match g.kind.as_str() {
                    "radial" => PathPaint::RadialGradient { end: color(&g.to) },
                    // CSS 0° points up; native 0° points right.
                    "linear" => PathPaint::LinearGradient {
                        end: color(&g.to),
                        angle: (g.angle - 90. + self.rotation as f32).rem_euclid(360.),
                    },
                    _ => unreachable!("validated starter gradient"),
                }
            }),
            stroke: self.stroke.as_ref().map(|s| color(&s.color)),
            width: self.stroke.as_ref().map_or(0., |s| (s.width * unit) as f32),
            alignment: self.stroke.as_ref().map_or(StrokeAlignment::Center, |s| {
                match s.align.as_str() {
                    "inside" => StrokeAlignment::Inside,
                    "outside" => StrokeAlignment::Outside,
                    "" | "center" => StrokeAlignment::Center,
                    _ => unreachable!("validated starter stroke alignment"),
                }
            }),
            ..Default::default()
        }
    }
    fn finish(
        &self,
        doc: &mut Document,
        mut id: crate::NodeId,
        unit: f64,
        center: (f64, f64),
    ) -> Result<(), String> {
        // Effects belong to the entire media frame, so replacing its photo retains them.
        let node = doc.node_mut(id).ok_or("Missing starter object")?;
        node.opacity = self.opacity;
        if let Some(shadow) = &self.shadow {
            let rgba = color(&shadow.color);
            node.styles.push(LayerStyle::DropShadow {
                color: [rgba[0], rgba[1], rgba[2]],
                opacity: rgba[3] as f32 / 255. * 100.,
                angle: (shadow.y.atan2(-shadow.x).to_degrees() - self.rotation) as f32,
                distance: (shadow.x.hypot(shadow.y) * unit) as f32,
                size: (shadow.blur * unit) as f32,
            });
        }
        if let Some(outline) = &self.outline {
            let rgba = color(&outline.color);
            // The CSS stroke is centered, then covered by the glyph fill.
            node.styles.push(LayerStyle::Stroke {
                color: [rgba[0], rgba[1], rgba[2]],
                opacity: rgba[3] as f32 / 255. * 100.,
                size: (outline.width * unit / 2.) as f32,
            });
        }
        if let Some(backdrop) = &self.backdrop {
            let (commands, group) = crate::design_formatting::background(
                doc,
                id,
                color(&backdrop.fill),
                [
                    (backdrop.pad_x * unit) as f32,
                    (backdrop.pad_y * unit) as f32,
                ],
                if backdrop.radius >= 0.5 {
                    10000.
                } else {
                    (backdrop.radius * unit) as f32
                },
                false,
            )?;
            for command in commands {
                command.apply(doc).map_err(|e| e.to_string())?;
            }
            // Opacity applies once to the text and its backing plate together.
            doc.node_mut(id).unwrap().opacity = 1.;
            doc.node_mut(group).unwrap().opacity = self.opacity;
            id = group;
        }
        if self.rotation != 0. {
            let c = glam::dvec2(center.0, center.1);
            let transform = glam::DAffine2::from_translation(c)
                * glam::DAffine2::from_angle(self.rotation.to_radians())
                * glam::DAffine2::from_translation(-c);
            Command::TransformNodes {
                ids: vec![id],
                transform: transform.to_cols_array(),
            }
            .apply(doc)
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
impl Starter {
    pub fn create(&self, w: u32, h: u32) -> Result<Document, String> {
        let mut doc = crate::creation::CanvasSpec {
            width: w as f64,
            height: h as f64,
            resolution: crate::creation::DESIGN_PRESETS
                .iter()
                .find(|p| p.name == self.preset)
                .map_or(
                    if self.preset == "Invitation" {
                        300.
                    } else {
                        72.
                    },
                    |p| p.resolution,
                ),
            ..Default::default()
        }
        .create()?;
        doc.nodes[0].kind = NodeKind::Fill {
            rgba: color(&self.bg),
        };
        let unit = w.min(h) as f64;
        for item in &self.els {
            let (x, y, width, height) = (
                item.x * w as f64,
                item.y * h as f64,
                item.w * w as f64,
                item.h * h as f64,
            );
            let mut center = (x + width / 2., y + height / 2.);
            let node = if item.kind == "text" {
                let mut text = TextSpec {
                    text: item.text.clone(),
                    font: if item.mono { "Geist Mono" } else { "Geist" }.into(),
                    size: (item.size * unit) as f32,
                    line_height: item.line_height,
                    letter_spacing: (item.spacing * item.size * unit) as f32,
                    underline: item.underline,
                    warp: item.curve,
                    color: color(&item.color),
                    bold: item.bold,
                    italic: item.italic,
                    align: item.align,
                    x: x as f32,
                    y: y as f32,
                    width: Some(width as f32),
                    ..Default::default()
                };
                if let Some(backdrop) = &item.backdrop {
                    text.x += (backdrop.pad_x * unit) as f32;
                    text.y += (backdrop.pad_y * unit) as f32;
                    text.width = None;
                    let natural = crate::text::layout(&text).bounds().width;
                    text.width =
                        Some(natural.min((width - 2. * backdrop.pad_x * unit).max(1.) as f32));
                }
                if item.rotation != 0. {
                    let bounds = crate::text::layout(&text).bounds();
                    center = (x + width / 2., y + (bounds.y + bounds.height) as f64 / 2.);
                }
                Node::text(0, &item.name, text, w, h)
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
                        radius(item.radius, unit, width, height),
                    )),
                    item.path_style(unit),
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
                    if item.stroke.is_some() {
                        // Keep the border above replacement photos. It shares the
                        // native clipping base and transforms with the whole frame.
                        let NodeKind::Path { path, style, .. } = &doc.node(base).unwrap().kind
                        else {
                            unreachable!()
                        };
                        let border = Node::path(
                            0,
                            "Frame border",
                            path.clone(),
                            PathStyle {
                                fill: None,
                                ..*style
                            },
                            w,
                            h,
                        );
                        let border = Command::AddNode {
                            node: Box::new(border),
                            slot: Slot::top_of(Some(group)),
                        }
                        .apply(&mut doc)
                        .map_err(|e| e.to_string())?
                        .ok_or("Missing frame border")?;
                        Command::SetClip {
                            id: border,
                            clip_to: Some(base),
                        }
                        .apply(&mut doc)
                        .map_err(|e| e.to_string())?;
                    }
                    item.finish(&mut doc, group, unit, center)?;
                    continue;
                }
                boundary
            };
            let id = Command::AddNode {
                node: Box::new(node),
                slot: Slot::TOP,
            }
            .apply(&mut doc)
            .map_err(|e| e.to_string())?
            .ok_or("Missing starter object")?;
            item.finish(&mut doc, id, unit, center)?;
        }
        doc.validate().map_err(|e| e.to_string())?;
        Ok(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn updated_catalog_has_fourteen_unique_templates_in_each_supplied_category() {
        let starters = starters();
        assert_eq!(starters.len(), 154);
        let mut ids = std::collections::HashSet::new();
        for (index, category) in Template::CATEGORIES.iter().enumerate() {
            assert_eq!(
                Template::catalog()
                    .filter(|t| t.category() == Some(index))
                    .count(),
                if index == 11 { 10 } else { 14 },
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
                    assert_eq!(text.line_height, item.line_height);
                    if item.rotation == 0. && item.backdrop.is_none() {
                        assert_eq!(text.x, (item.x * spec.w as f64) as f32);
                    }
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
    #[test]
    fn advanced_styling_is_preserved_at_native_and_preview_sizes() {
        for starter in starters() {
            for factor in [1., 0.1] {
                let (w, h) = (
                    (starter.w as f64 * factor).round() as u32,
                    (starter.h as f64 * factor).round() as u32,
                );
                let doc = starter.create(w, h).unwrap();
                let unit = w.min(h) as f64;
                for item in &starter.els {
                    let node = doc.nodes.iter().find(|n| n.name == item.name).unwrap();
                    assert_eq!(node.opacity, item.opacity);
                    assert_eq!(
                        node.styles.len(),
                        usize::from(item.shadow.is_some()) + usize::from(item.outline.is_some())
                    );
                    if let NodeKind::Text { spec, .. } = &node.kind {
                        assert_eq!(
                            spec.letter_spacing,
                            (item.spacing * item.size * unit) as f32
                        );
                        assert_eq!(spec.warp, item.curve);
                        assert_eq!(spec.underline, item.underline);
                        assert!((spec.rotation as f64 - item.rotation).abs() < 0.001);
                        if item.backdrop.is_some() {
                            assert!(
                                crate::design_formatting::text_backdrop(&doc, node.id)
                                    .unwrap()
                                    .1
                                    .is_some()
                            );
                        }
                    } else {
                        let boundary = if item.kind == "frame" {
                            doc.node(frame_parts(&doc, node.id).unwrap().0).unwrap()
                        } else {
                            node
                        };
                        let NodeKind::Path { style, path, .. } = &boundary.kind else {
                            panic!("flattened shape")
                        };
                        assert_eq!(*style, item.path_style(unit));
                        let mut expected = shape(
                            if item.kind == "frame" {
                                &item.shape
                            } else {
                                &item.kind
                            },
                            item.x * w as f64,
                            item.y * h as f64,
                            item.w * w as f64,
                            item.h * h as f64,
                            radius(item.radius, unit, item.w * w as f64, item.h * h as f64),
                        );
                        let center = glam::dvec2(
                            (item.x + item.w / 2.) * w as f64,
                            (item.y + item.h / 2.) * h as f64,
                        );
                        expected.transform(
                            glam::DAffine2::from_translation(center)
                                * glam::DAffine2::from_angle(item.rotation.to_radians())
                                * glam::DAffine2::from_translation(-center),
                        );
                        for (a, b) in path
                            .subpaths
                            .iter()
                            .flat_map(|s| &s.anchors)
                            .zip(expected.subpaths.iter().flat_map(|s| &s.anchors))
                        {
                            assert!((a.p.0 - b.p.0).abs() < 0.001 && (a.p.1 - b.p.1).abs() < 0.001);
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn print_starters_use_physical_preset_resolution_and_screen_starters_keep_screen_resolution() {
        for starter in starters() {
            let doc = starter.create(starter.w, starter.h).unwrap();
            let expected = if ["A4 flyer", "Business card", "Poster", "Invitation"]
                .contains(&starter.preset.as_str())
            {
                300.
            } else {
                72.
            };
            assert_eq!(doc.resolution, expected, "{}", starter.label);
        }
        let card = Template::BusinessCard.create(1050, 600).unwrap();
        assert_eq!(card.width as f32 / card.resolution, 3.5);
        assert_eq!(card.height as f32 / card.resolution, 2.);
    }
    #[test]
    fn unknown_handoff_fields_are_not_silently_discarded() {
        let invalid = r#"{"kind":"rect","name":"shape","x":0,"y":0,"w":1,"futureEffect":{}}"#;
        assert!(serde_json::from_str::<Item>(invalid).is_err());
        assert_eq!(color("rgba(20, 30, 40, .5)"), [20, 30, 40, 128]);
    }
    #[test]
    fn replacing_a_styled_frame_keeps_the_border_above_the_photo() {
        let starter: Starter = serde_json::from_value(serde_json::json!({
            "id":"test", "label":"Border test", "preset":"Square post", "w":200,"h":150,"bg":"#000000",
            "els":[{"kind":"frame","shape":"rect","name":"Photo","x":0.1,"y":0.1,"w":0.8,"h":0.8,"fill":"#222222",
                "stroke":{"color":"#ffffff","width":0.05,"align":"inside"}}]
        })).unwrap();
        let mut editor = crate::Editor::new(starter.create(200, 150).unwrap(), None);
        let group = editor
            .doc
            .nodes
            .iter()
            .find(|n| n.name == "Photo")
            .unwrap()
            .id;
        let boundary = frame_parts(&editor.doc, group).unwrap().0;
        let border = media::frame_border(&editor.doc, boundary).unwrap();
        let image = place_in_frame(
            &mut editor,
            group,
            Arc::new(emulsion_raster::Raster::solid(20, 15, [1., 0., 0., 1.])),
        )
        .unwrap();
        let children = editor.doc.children(Some(group));
        assert!(
            children.iter().position(|id| *id == image).unwrap()
                < children.iter().position(|id| *id == border).unwrap()
        );
        let raster = emulsion_raster::composite::flatten(&editor.doc.composite_tree(), 0);
        let edge = raster.get(23, 75);
        let center = raster.get(100, 75);
        assert!(edge[0] > 60000 && edge[1] > 60000 && edge[2] > 60000);
        assert!(center[0] > 60000 && center[1] < 100 && center[2] < 100);
    }
    #[test]
    fn tracking_pixels_change_advances_by_pixels_at_every_font_size() {
        for size in [20., 200., 1600.] {
            let base = TextSpec {
                text: "MMMM".into(),
                font: "Geist Mono".into(),
                size,
                ..Default::default()
            };
            let natural = crate::text::layout(&base).bounds().width;
            for spacing in [-2., 2.] {
                let adjusted = TextSpec {
                    letter_spacing: spacing,
                    ..base.clone()
                };
                let width = crate::text::layout(&adjusted).bounds().width;
                assert!(
                    (width - natural - spacing * 4.).abs() < 0.05,
                    "size={size} spacing={spacing}: {width} vs {natural}"
                );
            }
        }
        let doc = Template::ProductLaunch.create(1080, 1080).unwrap();
        let title = doc.nodes.iter().find(|n| n.name == "Title").unwrap();
        let NodeKind::Text { spec, .. } = &title.kind else {
            panic!()
        };
        let bounds = crate::text::layout(spec).bounds();
        assert!(
            bounds.width > 450. && bounds.width < 900.,
            "headline collapsed: {bounds:?}"
        );
    }
}
