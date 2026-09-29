//! Bounded local monochrome tracing. The original bitmap is never replaced.
use super::{editable, validate};
use crate::{Command, Document, Editor, Node, NodeId, NodeKind, command::Slot};
use emulsion_raster::vector::{Anchor, Path, PathStyle, SubPath};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    pub resolution: u32,
    pub threshold: f32,
    pub alpha_only: bool,
    pub invert: bool,
    pub color: [u8; 4],
}
impl Default for Options {
    fn default() -> Self {
        Self {
            resolution: 256,
            threshold: 0.5,
            alpha_only: false,
            invert: false,
            color: [0, 0, 0, 255],
        }
    }
}
/// Returns native contours in document coordinates; preview and commit use the exact same geometry.
pub fn preview(doc: &Document, id: NodeId, options: Options) -> Result<Path, String> {
    editable(doc, id)?;
    if !(16..=512).contains(&options.resolution)
        || !options.threshold.is_finite()
        || !(0.01..=0.99).contains(&options.threshold)
    {
        return Err("Trace resolution must be 16–512 and threshold 0.01–0.99.".into());
    }
    let node = doc.node(id).ok_or("Image does not exist")?;
    let (raster, placement) = match &node.kind {
        NodeKind::Raster { raster, placement } => (raster, *placement),
        NodeKind::Smart {
            cache,
            source,
            placement,
            offset,
            ..
        } => (
            cache,
            crate::smart::cache_placement(
                placement,
                (source.width(), source.height()),
                (cache.width(), cache.height()),
                *offset,
            ),
        ),
        _ => return Err("Select an image to trace.".into()),
    };
    let factor =
        (options.resolution as f64 / f64::from(raster.width().max(raster.height()))).min(1.);
    let w = (raster.width() as f64 * factor).ceil().max(1.) as usize;
    let h = (raster.height() as f64 * factor).ceil().max(1.) as usize;
    let mut cells = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            let sx = (((x as f64 + 0.5) * raster.width() as f64 / w as f64) as u32)
                .min(raster.width() - 1);
            let sy = (((y as f64 + 0.5) * raster.height() as f64 / h as f64) as u32)
                .min(raster.height() - 1);
            let p = raster.get(sx, sy);
            let alpha = p[3] as f32 / 65535.;
            let luminance = if p[3] == 0 {
                1.
            } else {
                (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / p[3] as f32
            };
            let selected = if options.alpha_only {
                alpha >= options.threshold
            } else {
                alpha >= 0.01 && luminance <= options.threshold
            };
            cells[y * w + x] = if options.invert {
                alpha >= 0.01 && !selected
            } else {
                selected
            };
        }
    }
    type Point = (usize, usize);
    let mut edges: BTreeMap<Point, Vec<Point>> = BTreeMap::new();
    let mut add = |a, b| edges.entry(a).or_default().push(b);
    for y in 0..h {
        for x in 0..w {
            if !cells[y * w + x] {
                continue;
            }
            if y == 0 || !cells[(y - 1) * w + x] {
                add((x, y), (x + 1, y));
            }
            if x + 1 == w || !cells[y * w + x + 1] {
                add((x + 1, y), (x + 1, y + 1));
            }
            if y + 1 == h || !cells[(y + 1) * w + x] {
                add((x + 1, y + 1), (x, y + 1));
            }
            if x == 0 || !cells[y * w + x - 1] {
                add((x, y + 1), (x, y));
            }
        }
    }
    let mut path = Path::default();
    let transform = placement.to_doc(raster.width(), raster.height());
    let mut total = 0;
    while let Some((&start, _)) = edges.first_key_value() {
        let mut vertices = vec![start];
        let mut at = start;
        let mut previous: Option<Point> = None;
        loop {
            let choices = edges.get_mut(&at).ok_or("Trace contour is incomplete")?;
            // At diagonal pixel contact, follow the right turn to keep separate components.
            let pick = if let Some(prev) = previous {
                let dx = at.0 as isize - prev.0 as isize;
                let dy = at.1 as isize - prev.1 as isize;
                choices
                    .iter()
                    .position(|n| {
                        let nx = n.0 as isize - at.0 as isize;
                        let ny = n.1 as isize - at.1 as isize;
                        dx * ny - dy * nx > 0
                    })
                    .unwrap_or(0)
            } else {
                0
            };
            let next = choices.remove(pick);
            if choices.is_empty() {
                edges.remove(&at);
            }
            previous = Some(at);
            at = next;
            if at == start {
                break;
            }
            vertices.push(at);
        }
        let mut corners = Vec::new();
        for i in 0..vertices.len() {
            let a = vertices[(i + vertices.len() - 1) % vertices.len()];
            let b = vertices[i];
            let c = vertices[(i + 1) % vertices.len()];
            if (b.0 as isize - a.0 as isize) * (c.1 as isize - b.1 as isize)
                != (b.1 as isize - a.1 as isize) * (c.0 as isize - b.0 as isize)
            {
                let p = transform.transform_point2(glam::dvec2(
                    b.0 as f64 * raster.width() as f64 / w as f64,
                    b.1 as f64 * raster.height() as f64 / h as f64,
                ));
                corners.push(Anchor::corner((p.x, p.y)));
            }
        }
        total += corners.len();
        if total > emulsion_raster::vector::MAX_ANCHORS {
            return Err(
                "Trace has over 20000 points. Lower the resolution or adjust the threshold.".into(),
            );
        }
        path.subpaths.push(SubPath {
            anchors: corners,
            closed: true,
        });
    }
    if path.is_empty() {
        return Err("No foreground found. Adjust the threshold or invert the trace.".into());
    }
    validate(&path)?;
    Ok(path)
}
pub fn apply(editor: &mut Editor, id: NodeId, options: Options) -> Result<NodeId, String> {
    if editor.in_transaction() {
        return Err("Finish the current edit first.".into());
    }
    let path = preview(&editor.doc, id, options)?;
    let source = editor.doc.node(id).unwrap().clone();
    apply_preview(editor, &source, path, options.color)
}
/// Commit previously previewed contours without repeating tracing on the UI thread.
/// Source identity and content must still match the immutable preview snapshot.
pub fn apply_preview(
    editor: &mut Editor,
    source: &Node,
    path: Path,
    color: [u8; 4],
) -> Result<NodeId, String> {
    if editor.in_transaction() {
        return Err("Finish the current edit first.".into());
    }
    let id = source.id;
    editable(&editor.doc, id)?;
    validate(&path)?;
    if editor.doc.node(id) != Some(source) {
        return Err(
            "The source image changed. Reopen the trace dialog for a fresh preview.".into(),
        );
    }
    if path.is_empty() {
        return Err("No vector contours to insert.".into());
    }
    let source = editor.doc.node(id).unwrap();
    let parent = source.parent;
    let index = editor
        .doc
        .children(parent)
        .iter()
        .position(|n| *n == id)
        .unwrap()
        + 1;
    let node = Node::path(
        0,
        format!("{} trace", source.name),
        Arc::new(path),
        PathStyle {
            fill: Some(color),
            stroke: None,
            ..Default::default()
        },
        editor.doc.width,
        editor.doc.height,
    );
    editor
        .execute(Command::AddNode {
            node: Box::new(node),
            slot: Slot { parent, index },
        })
        .map_err(|e| e.to_string())?
        .ok_or("No vector trace was created.".into())
}
