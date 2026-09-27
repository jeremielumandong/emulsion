//! Lucid Standard Import v1: https://lucid.readme.io/docs/overview-si
//! This is distinct from undocumented cloud backups/infrastructure JSON exports.
use super::*;
use serde_json::Value;
fn array<'a>(v: &'a Value, key: &str) -> Result<&'a [Value]> {
    match v.get(key) {
        None => Ok(&[]),
        Some(Value::Array(a)) => Ok(a),
        _ => Err(error(format!("{key} must be an array."))),
    }
}
fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 200)
        .ok_or_else(|| error(format!("Missing or invalid {key}.")))
}
fn number(v: &Value, key: &str, default: f64) -> Result<f64> {
    let Some(v) = v.get(key) else {
        return Ok(default);
    };
    let n = v
        .as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .filter(|n| n.is_finite() && n.abs() <= 1e6)
        .ok_or_else(|| error(format!("Invalid {key}.")))?;
    Ok(n)
}
fn plain(text: &str, warnings: &mut BTreeSet<String>) -> String {
    if !text.contains('<') {
        return text.to_string();
    }
    warnings.insert(
        "HTML labels retain editable plain text; inline HTML formatting needs review.".into(),
    );
    let mut out = String::new();
    let mut tag = String::new();
    let mut inside = false;
    for c in text.chars() {
        if c == '<' {
            inside = true;
            tag.clear();
        } else if c == '>' {
            inside = false;
            if ["br", "br/", "/p", "/div"].contains(&tag.trim().to_lowercase().as_str()) {
                out.push('\n');
            }
        } else if inside {
            tag.push(c);
        } else {
            out.push(c);
        }
    }
    quick_xml::escape::unescape(&out.replace("&nbsp;", " "))
        .map(|s| s.into_owned())
        .unwrap_or(out)
}
fn kind(name: &str, warnings: &mut BTreeSet<String>) -> ShapeKind {
    match name {
        "rectangle" | "process" | "text" | "hotspot" | "image" => ShapeKind::Process,
        "decision" | "diamond" => ShapeKind::Decision,
        "terminator" | "pill" | "roundedRectangle" => ShapeKind::Terminator,
        "data" | "inputOutput" | "parallelogram" => ShapeKind::Data,
        "database" | "cylinder" => ShapeKind::Database,
        "document" => ShapeKind::Document,
        "stickyNote" | "note" => ShapeKind::Note,
        "container" | "rectangleContainer" => ShapeKind::Container,
        "swimlane" => ShapeKind::Swimlane,
        "cloud" => ShapeKind::Cloud,
        "ellipse" | "circle" | "triangle" | "star" => ShapeKind::Process,
        other => {
            warnings.insert(format!(
                "Lucid shape '{other}' uses an editable rectangle; its label and data are retained."
            ));
            ShapeKind::Process
        }
    }
}
fn stroke(v: &Value) -> Result<PathStyle> {
    let width = number(v, "width", 1.)?;
    if !(0. ..=1000.).contains(&width) {
        return Err(error("Stroke width is out of range."));
    }
    let mut style = PathStyle {
        fill: None,
        stroke: Some(
            v.get("color")
                .and_then(Value::as_str)
                .map(color)
                .transpose()?
                .unwrap_or([0, 0, 0, 255]),
        ),
        width: width as f32,
        ..Default::default()
    };
    match v.get("style").and_then(Value::as_str).unwrap_or("solid") {
        "solid" => {}
        "dashed" => {
            style.dash = [8., 5., 0., 0., 0., 0.];
            style.dash_count = 2;
        }
        "dotted" => {
            style.dash = [1., 4., 0., 0., 0., 0.];
            style.dash_count = 2;
        }
        _ => return Err(error("Unknown Lucid stroke style.")),
    }
    Ok(style)
}
fn endpoint(v: &Value) -> Result<(String, Port)> {
    if string(v, "type")? != "shapeEndpoint" {
        return Err(error(
            "This Lucid file has a free or line-to-line endpoint. Export it as VSDX to preserve its drawn geometry.",
        ));
    }
    let port = if v.get("position").is_some() {
        Port::Custom {
            x: number(&v["position"], "x", 0.5)?,
            y: number(&v["position"], "y", 0.5)?,
        }
    } else {
        Port::Auto
    };
    Ok((string(v, "shapeId")?.into(), port))
}
fn arrow(v: &Value, warnings: &mut BTreeSet<String>) -> bool {
    let style = v.get("style").and_then(Value::as_str).unwrap_or("none");
    if !["none", "arrow"].contains(&style) {
        warnings.insert(format!(
            "Lucid endpoint style '{style}' uses a triangular arrowhead."
        ));
    }
    style != "none"
}
pub(super) fn package(path: &Path) -> Result<Imported> {
    let package = Package::read(path)?;
    parse(package.text("document.json")?, Some(&package))
}
pub(super) fn from_json(text: &str) -> Result<Imported> {
    parse(text, None)
}
fn parse(text: &str, package: Option<&Package>) -> Result<Imported> {
    if text.len() > 2 << 20 {
        return Err(error("Lucid document.json exceeds 2 MiB."));
    }
    let root: Value = serde_json::from_str(text).map_err(|e| error(e.to_string()))?;
    if root.get("version").and_then(Value::as_u64) != Some(1) {
        return Err(error(
            "Expected Lucid Standard Import version 1. For a Lucidchart document, export VSDX or VDX; infrastructure JSON is a different format.",
        ));
    }
    let pages = array(&root, "pages")?;
    if pages.is_empty() || pages.len() > 100 {
        return Err(error("Lucid document must contain 1–100 pages."));
    }
    let mut warnings = BTreeSet::new();
    let mut scenes = Vec::new();
    let mut unique = HashSet::new();
    if !array(&root, "collections")?.is_empty() {
        warnings.insert("External data collection bindings are not refreshed automatically; imported custom data stays local.".into());
    }
    for page in pages {
        if !unique.insert(string(page, "id")?.to_string()) {
            return Err(error("Duplicate Lucid ID."));
        }
        let mut scene = Scene {
            name: string(page, "title")?.into(),
            width: 816,
            height: 1056,
            ..Default::default()
        };
        let size = &page["settings"]["size"];
        let (mut w, mut h) = match size.get("type").and_then(Value::as_str).unwrap_or("letter") {
            "custom" => (number(size, "w", 816.)?, number(size, "h", 1056.)?),
            "letter" => (816., 1056.),
            "legal" => (816., 1344.),
            "a4" => (794., 1123.),
            "a3" => (1123., 1587.),
            "a5" => (559., 794.),
            "tabloid" => (1056., 1632.),
            "executive" => (696., 1008.),
            "folio" => (816., 1248.),
            "statement" => (528., 816.),
            _ => return Err(error("Unsupported Lucid paper size.")),
        };
        if size["format"] == "landscape" {
            std::mem::swap(&mut w, &mut h);
        }
        if w < 1. || h < 1. || w > 30000. || h > 30000. {
            return Err(error("Invalid page dimensions."));
        }
        scene.width = w.ceil() as u32;
        scene.height = h.ceil() as u32;
        scene.background = page["settings"]
            .get("fillColor")
            .and_then(Value::as_str)
            .map(color)
            .transpose()?;
        if page["settings"]["infiniteCanvas"] == true {
            warnings.insert(
                "Infinite canvases are imported as finite pages; objects keep their coordinates."
                    .into(),
            );
        }
        if !array(page, "dataBackedShapes")?.is_empty() {
            return Err(error(
                "Expand Lucid data-backed shapes or export this document as VSDX before importing.",
            ));
        }
        let mut shapes = array(page, "shapes")?.iter().collect::<Vec<_>>();
        if shapes.len() > diagram::MAX_SHAPES {
            return Err(error("Too many Lucid shapes."));
        }
        shapes.sort_by_key(|s| s["zIndex"].as_i64().unwrap_or(0));
        for shape in shapes {
            let key = string(shape, "id")?.to_string();
            if !unique.insert(key.clone()) {
                return Err(error("Duplicate Lucid ID."));
            }
            let name = string(shape, "type")?;
            let b = &shape["boundingBox"];
            let bounds = [
                number(b, "x", 0.)?,
                number(b, "y", 0.)?,
                number(b, "w", 100.)?,
                number(b, "h", 60.)?,
            ];
            let mut s = Shape::new(
                key,
                kind(name, &mut warnings),
                bounds,
                plain(
                    shape.get("text").and_then(Value::as_str).unwrap_or(""),
                    &mut warnings,
                ),
            );
            s.name = name.into();
            s.style = stroke(&shape["style"]["stroke"])?;
            s.style.fill = Some(
                shape["style"]["fill"]
                    .get("color")
                    .and_then(Value::as_str)
                    .map(color)
                    .transpose()?
                    .unwrap_or([255; 4]),
            );
            if let Some(c) = shape["style"].get("textColor").and_then(Value::as_str) {
                s.text.color = color(c)?;
            }
            let opacity = number(shape, "opacity", 100.)?;
            if !(0. ..=100.).contains(&opacity) {
                return Err(error("Invalid shape opacity."));
            }
            s.opacity = (opacity / 100.) as f32;
            if name == "text" || name == "hotspot" {
                s.style.fill = None;
                s.style.stroke = None;
            }
            let [x, y, w, h] = bounds;
            s.path = match name {
                "ellipse" | "circle" => {
                    Some(emulsion_core::design::Element::Circle.path(x, y, w, h))
                }
                "triangle" => Some(emulsion_core::design::Element::Triangle.path(x, y, w, h)),
                "star" => Some(emulsion_core::design::Element::Star.path(x, y, w, h)),
                _ => None,
            };
            let rotation = number(b, "rotation", 0.)?;
            if rotation != 0. {
                let mut path = s.path.take().unwrap_or_else(|| s.kind.path(bounds));
                let center = glam::dvec2(x + w / 2., y + h / 2.);
                path.transform(
                    glam::DAffine2::from_translation(center)
                        * glam::DAffine2::from_angle(rotation.to_radians())
                        * glam::DAffine2::from_translation(-center),
                );
                s.path = Some(path);
                warnings.insert("Rotated outlines are preserved; review their label alignment and connection ports.".into());
            }
            for datum in array(shape, "customData")? {
                s.data.insert(
                    string(datum, "key")?.into(),
                    datum
                        .get("value")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                );
            }
            if let Some(note) = shape.get("note").and_then(Value::as_str) {
                s.data.insert("note".into(), plain(note, &mut warnings));
            }
            if !array(shape, "actions")?.is_empty() {
                warnings.insert("Lucid actions are not executed in local editing.".into());
            }
            let fill = if name == "image" {
                &shape["image"]
            } else {
                &shape["style"]["fill"]
            };
            if name == "image" || fill["type"] == "image" {
                let reference = fill.get("ref").and_then(Value::as_str);
                if let (Some(package), Some(reference)) = (package, reference) {
                    let bytes = package
                        .entries
                        .get(&format!("images/{reference}"))
                        .ok_or_else(|| error("Missing embedded Lucid image."))?;
                    let image = crate::import::import_bytes(reference, bytes)?;
                    let NodeKind::Raster { raster, .. } = &image.nodes[0].kind else {
                        unreachable!()
                    };
                    s.image = Some(raster.clone());
                } else {
                    warnings.insert("An external image was not fetched; its editable placeholder and reference are retained.".into());
                    s.data.insert(
                        "image_reference".into(),
                        fill.get("url")
                            .and_then(Value::as_str)
                            .or(reference)
                            .unwrap_or("")
                            .into(),
                    );
                }
            }
            scene.shapes.push(s);
        }
        let lines = array(page, "lines")?;
        if lines.len() > diagram::MAX_EDGES {
            return Err(error("Too many Lucid lines."));
        }
        for line in lines {
            let key = string(line, "id")?.to_string();
            if !unique.insert(key.clone()) {
                return Err(error("Duplicate Lucid ID."));
            }
            let typ = string(line, "lineType")?;
            let routing = if typ == "elbow" {
                Routing::Orthogonal
            } else {
                Routing::Straight
            };
            if typ == "curved" {
                warnings.insert("Curved lines use editable straight segments.".into());
            }
            let points = array(
                line,
                if typ == "elbow" {
                    "elbowControlPoints"
                } else {
                    "joints"
                },
            )?
            .iter()
            .map(|p| Ok((number(p, "x", 0.)?, number(p, "y", 0.)?)))
            .collect::<Result<Vec<_>>>()?;
            let labels = array(line, "text")?;
            if labels.len() > 1 {
                warnings
                    .insert("Multiple line labels are combined into one editable label.".into());
            }
            let label = labels
                .iter()
                .map(|l| {
                    plain(
                        l.get("text").and_then(Value::as_str).unwrap_or(""),
                        &mut warnings,
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            scene.lines.push(Line {
                key,
                source: endpoint(&line["endpoint1"])?,
                target: endpoint(&line["endpoint2"])?,
                label,
                style: stroke(&line["stroke"])?,
                routing,
                points,
                start_arrow: arrow(&line["endpoint1"], &mut warnings),
                end_arrow: arrow(&line["endpoint2"], &mut warnings),
                parent: None,
            });
        }
        let mut memberships = HashMap::new();
        for group in array(page, "groups")?.iter().chain(array(page, "layers")?) {
            let key = string(group, "id")?.to_string();
            if !unique.insert(key.clone()) {
                return Err(error("Duplicate Lucid ID."));
            }
            for item in array(group, "items")? {
                let item = item
                    .as_str()
                    .ok_or_else(|| error("Group item must be an ID."))?;
                if memberships.insert(item.to_string(), key.clone()).is_some() {
                    return Err(error("An object belongs to multiple imported groups."));
                }
            }
            scene.groups.push((
                key.clone(),
                group
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or(&key)
                    .into(),
                None,
            ));
        }
        for s in &mut scene.shapes {
            s.parent = memberships.remove(&s.key);
        }
        for l in &mut scene.lines {
            l.parent = memberships.remove(&l.key);
        }
        for (id, _, parent) in &mut scene.groups {
            *parent = memberships.remove(id);
        }
        if !memberships.is_empty() {
            return Err(error("An imported group references a missing object."));
        }
        scenes.push(scene);
    }
    finish(scenes, warnings)
}
