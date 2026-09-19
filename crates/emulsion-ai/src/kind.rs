//! What kind of picture is this? Photo, drawing, screenshot or flat
//! graphic, from cheap measurements (camera data, colour count, paper,
//! edge hardness), with Jev refining the call when a key is configured.
//! The kind steers suggestions: lens profiles and face restore make sense
//! for photographs, not for a manga page or a UI capture.

use emulsion_core::Document;
use emulsion_raster::color;
use emulsion_raster::composite::{flatten, level_size};
use serde_json::{Map, Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DocKind {
    #[default]
    Unknown,
    Photo,
    Drawing,
    Screenshot,
    Graphic,
}

impl DocKind {
    pub fn label(self) -> &'static str {
        match self {
            DocKind::Unknown => "unknown",
            DocKind::Photo => "photograph",
            DocKind::Drawing => "drawing",
            DocKind::Screenshot => "screenshot",
            DocKind::Graphic => "graphic",
        }
    }

    pub fn from_key(k: &str) -> DocKind {
        match k {
            "photo" | "photograph" => DocKind::Photo,
            "drawing" => DocKind::Drawing,
            "screenshot" => DocKind::Screenshot,
            "graphic" => DocKind::Graphic,
            _ => DocKind::Unknown,
        }
    }

    /// Photographic corrections (lenses, faces, noise) apply.
    pub fn is_photographic(self) -> bool {
        matches!(self, DocKind::Photo | DocKind::Unknown)
    }

    pub const ALL: [DocKind; 4] = [
        DocKind::Photo,
        DocKind::Drawing,
        DocKind::Screenshot,
        DocKind::Graphic,
    ];
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Classification {
    pub kind: DocKind,
    /// 0–1.
    pub confidence: f32,
    /// One line on why.
    pub evidence: String,
    /// The measurements, for Jev and the curious.
    pub metrics: Map<String, Value>,
    /// Who decided: "rules" or "jev".
    pub by: &'static str,
}

/// Measurements over a small composite.
fn measure(doc: &Document) -> Map<String, Value> {
    let mut level = 0;
    while {
        let (w, h) = level_size(doc.width, doc.height, level);
        w.max(h) > 256 && level < 16
    } {
        level += 1;
    }
    let small = flatten(&doc.composite_tree(), level);
    let px = small.to_pixels();
    let (w, h) = (small.width() as usize, small.height() as usize);
    let mut colors = std::collections::HashSet::new();
    let mut sat = 0.0f64;
    let mut light = 0.0f64;
    let mut n = 0usize;
    let mut srgb: Vec<[u8; 4]> = Vec::with_capacity(px.len());
    for p in &px {
        let c = color::premul_to_srgba8(color::px_to_f(*p));
        srgb.push(c);
        if c[3] < 128 {
            continue;
        }
        n += 1;
        // Quantise to 5 bits per channel so JPEG noise does not inflate the count.
        colors.insert(((c[0] >> 3) as u32) << 10 | ((c[1] >> 3) as u32) << 5 | (c[2] >> 3) as u32);
        let (mx, mn) = (
            c[0].max(c[1]).max(c[2]) as f64,
            c[0].min(c[1]).min(c[2]) as f64,
        );
        sat += if mx > 0.0 { (mx - mn) / mx } else { 0.0 };
        light += (0.2126 * c[0] as f64 + 0.7152 * c[1] as f64 + 0.0722 * c[2] as f64) / 255.0;
    }
    let n = n.max(1) as f64;
    // Edge hardness: fraction of horizontal neighbours with a big jump, and
    // of exactly equal neighbours (flat fills of a UI or vector graphic).
    let (mut hard, mut flat, mut pairs) = (0usize, 0usize, 0usize);
    for y in 0..h {
        for x in 1..w {
            let (a, b) = (srgb[y * w + x - 1], srgb[y * w + x]);
            let d = (a[0] as i32 - b[0] as i32).abs()
                + (a[1] as i32 - b[1] as i32).abs()
                + (a[2] as i32 - b[2] as i32).abs();
            pairs += 1;
            if d > 120 {
                hard += 1;
            }
            if d == 0 {
                flat += 1;
            }
        }
    }
    let pairs = pairs.max(1) as f64;
    // Rows that are one colour edge to edge: bars, panels, borders.
    let uniform_rows = (0..h)
        .filter(|&y| {
            let first = srgb[y * w];
            (1..w).all(|x| {
                let c = srgb[y * w + x];
                (c[0] as i32 - first[0] as i32).abs() < 6
                    && (c[1] as i32 - first[1] as i32).abs() < 6
                    && (c[2] as i32 - first[2] as i32).abs() < 6
            })
        })
        .count() as f64
        / h.max(1) as f64;
    let mut m = Map::new();
    m.insert("distinct_colors".into(), json!(colors.len()));
    m.insert("color_fraction".into(), json!(colors.len() as f64 / n));
    m.insert("saturation".into(), json!(sat / n));
    m.insert("lightness".into(), json!(light / n));
    m.insert("hard_edge_fraction".into(), json!(hard as f64 / pairs));
    m.insert("flat_fraction".into(), json!(flat as f64 / pairs));
    m.insert("uniform_row_fraction".into(), json!(uniform_rows));
    m.insert("has_camera_data".into(), json!(doc.info.is_some()));
    m.insert(
        "screen_size".into(),
        json!(matches!(
            (doc.width, doc.height),
            (1920, 1080)
                | (2560, 1440)
                | (3840, 2160)
                | (1366, 768)
                | (1440, 900)
                | (2560, 1600)
                | (1080, 1920)
                | (1170, 2532)
                | (1290, 2796)
                | (1284, 2778)
                | (1179, 2556)
        )),
    );
    m.insert("width".into(), json!(doc.width));
    m.insert("height".into(), json!(doc.height));
    // Paper-and-marks reading from the critique.
    let c = crate::critique::analyze(doc);
    for k in ["drawing_on_paper", "empty_fraction", "value_groups"] {
        if let Some(v) = c.metrics.get(k) {
            m.insert(k.into(), v.clone());
        }
    }
    m
}

fn f(m: &Map<String, Value>, k: &str) -> f64 {
    m.get(k).and_then(Value::as_f64).unwrap_or(0.0)
}

/// Classify from measurements alone.
pub fn classify(doc: &Document) -> Classification {
    let m = measure(doc);
    let camera = m.get("has_camera_data") == Some(&json!(true));
    let screen = m.get("screen_size") == Some(&json!(true));
    let flat = f(&m, "flat_fraction");
    let hard = f(&m, "hard_edge_fraction");
    let colors = f(&m, "color_fraction");
    let uniform = f(&m, "uniform_row_fraction");
    let drawing_on_paper = m.get("drawing_on_paper") == Some(&json!(true));
    let light = f(&m, "lightness");
    let sat = f(&m, "saturation");
    let mut scores: Vec<(DocKind, f64, &str)> = Vec::new();
    // Photographs: camera data, smooth gradients (few equal neighbours),
    // many distinct colours.
    let mut photo = 0.0;
    if camera {
        photo += 0.6;
    }
    photo += ((0.25 - flat) / 0.25).clamp(0.0, 1.0) * 0.3;
    photo += (colors * 8.0).clamp(0.0, 1.0) * 0.2;
    scores.push((DocKind::Photo, photo, "camera data and smooth tone"));
    // Screenshots: flat fills, uniform rows, hard edges, screen-sized.
    let mut shot = 0.0;
    shot += (flat / 0.5).clamp(0.0, 1.0) * 0.4;
    shot += (uniform / 0.15).clamp(0.0, 1.0) * 0.3;
    shot += (hard / 0.08).clamp(0.0, 1.0) * 0.15;
    if screen {
        shot += 0.25;
    }
    if camera {
        shot -= 0.4;
    }
    scores.push((
        DocKind::Screenshot,
        shot,
        "flat fills, straight bars and hard edges",
    ));
    // Drawings: paper with marks, little colour.
    let mut draw = 0.0;
    if drawing_on_paper {
        draw += 0.6;
    }
    draw += ((0.25 - sat) / 0.25).clamp(0.0, 1.0) * 0.2;
    draw += ((light - 0.6) / 0.35).clamp(0.0, 1.0) * 0.2;
    if camera {
        draw -= 0.3;
    }
    scores.push((DocKind::Drawing, draw, "mostly paper with marks on it"));
    // Graphics: few colours, flat, but not screen-shaped and not paper.
    let mut graphic = 0.0;
    graphic += ((0.02 - colors) / 0.02).clamp(0.0, 1.0) * 0.4;
    graphic += (flat / 0.4).clamp(0.0, 1.0) * 0.3;
    if !screen && !drawing_on_paper {
        graphic += 0.15;
    }
    if camera {
        graphic -= 0.4;
    }
    scores.push((DocKind::Graphic, graphic, "a handful of flat colours"));
    scores.sort_by(|a, b| b.1.total_cmp(&a.1));
    let (kind, top, why) = scores[0];
    let second = scores.get(1).map(|s| s.1).unwrap_or(0.0);
    let confidence = ((top - second) * 1.5 + top * 0.3).clamp(0.0, 1.0) as f32;
    let (kind, confidence) = if top < 0.3 {
        (DocKind::Unknown, 0.0)
    } else {
        (kind, confidence)
    };
    Classification {
        kind,
        confidence,
        evidence: why.to_string(),
        metrics: m,
        by: "rules",
    }
}

/// Ask Jev to pick the kind from the measurements; falls back to the
/// rules' answer on any error.
pub fn classify_with_jev(doc: &Document, jev: &crate::jev::Jev) -> Classification {
    let mut c = classify(doc);
    let state = json!({
        "measurements": c.metrics,
        "rules_guess": c.kind.label(),
        "context": "An image editor deciding what kind of picture is open, from statistics of a small copy (no pixels sent).",
    });
    let criteria: Map<String, Value> = DocKind::ALL
        .iter()
        .map(|k| {
            (
                k.label().to_string(),
                json!(match k {
                    DocKind::Photo => "a photograph from a camera or phone",
                    DocKind::Drawing => "a drawing, sketch, manga page or painting on paper",
                    DocKind::Screenshot => "a capture of a screen or app window",
                    DocKind::Graphic => "a flat vector-like graphic, logo or diagram",
                    DocKind::Unknown => "cannot tell",
                }),
            )
        })
        .collect();
    let mut q = Map::new();
    q.insert(
        "kind".into(),
        json!({ "type": "choice", "instructions": "What kind of picture do these measurements describe?", "criteria": criteria }),
    );
    if let Ok(answers) = jev.evaluate(&state, q)
        && let Some(a) = answers.get("kind")
    {
        let picked = a
            .get("choice")
            .or_else(|| a.get("answer"))
            .or_else(|| a.get("value"))
            .and_then(Value::as_str)
            .map(DocKind::from_key)
            .unwrap_or(DocKind::Unknown);
        let conf = a.get("confidence").and_then(Value::as_f64).unwrap_or(0.5) as f32;
        if picked != DocKind::Unknown {
            c.kind = picked;
            c.confidence = conf.max(c.confidence * 0.5);
            c.by = "jev";
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::command::Slot;
    use emulsion_core::{Command, Node};
    use emulsion_raster::{Placement, Raster};
    use std::sync::Arc;

    fn doc_from(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4] + Sync) -> Document {
        let r = Raster::from_fn(w, h, [0; 4], |x, y| {
            color::f_to_px(color::srgba8_to_premul(f(x, y)))
        });
        let mut d = Document::new(w, h);
        Command::AddNode {
            node: Box::new(Node::raster(0, "P", Arc::new(r), Placement::default())),
            slot: Slot::TOP,
        }
        .apply(&mut d)
        .unwrap();
        d
    }

    #[test]
    fn kinds_are_told_apart() {
        // A screenshot: flat panels, a title bar, hard edges.
        let shot = doc_from(1920, 1080, |x, y| {
            if y < 60 {
                [40, 40, 44, 255]
            } else if x < 300 {
                [245, 245, 247, 255]
            } else if (x / 200 + y / 120) % 2 == 0 {
                [255, 255, 255, 255]
            } else {
                [230, 232, 236, 255]
            }
        });
        let c = classify(&shot);
        assert_eq!(c.kind, DocKind::Screenshot, "{c:?}");
        // A drawing: paper with dark strokes.
        let draw = doc_from(600, 400, |x, y| {
            if (x as i32 - 300).pow(2) + (y as i32 - 200).pow(2) < 22_000 && (x + y) % 9 < 2 {
                [30, 30, 30, 255]
            } else {
                [246, 242, 232, 255]
            }
        });
        let c = classify(&draw);
        assert_eq!(c.kind, DocKind::Drawing, "{c:?}");
        // A photograph-like gradient with camera data.
        let mut photo = doc_from(900, 600, |x, y| {
            let (fx, fy) = (x as f32 / 900.0, y as f32 / 600.0);
            [
                (120.0 + 100.0 * fx + 20.0 * (fy * 31.0).sin()) as u8,
                (90.0 + 80.0 * fy + 15.0 * (fx * 47.0).cos()) as u8,
                (60.0 + 120.0 * (1.0 - fx) * fy) as u8,
                255,
            ]
        });
        photo.info = Some(emulsion_core::document::ImageInfo {
            make: "Sony".into(),
            model: "ILCE-7M3".into(),
            ..Default::default()
        });
        let c = classify(&photo);
        assert_eq!(c.kind, DocKind::Photo, "{c:?}");
        assert!(c.confidence > 0.3 && c.kind.is_photographic());
    }
}
