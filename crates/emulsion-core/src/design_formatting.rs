//! Native editable geometry and formatting shared by Design UI and MCP.
//! Returned commands must be preflighted and applied as one transaction.
use crate::{Command, Document, Node, NodeId, NodeKind, command::Slot};
use emulsion_raster::{
    vector::{Anchor, Path, PathPaint, PathStyle, SubPath},
    vector_geometry,
};
use std::sync::Arc;

pub fn fill(
    doc: &Document,
    ids: &[NodeId],
    rgba: Option<[u8; 4]>,
    paint: PathPaint,
) -> Result<Vec<Command>, String> {
    ids.iter().map(|id| {
        let node=doc.node(*id).ok_or("The selected object no longer exists.")?;
        Ok(match &node.kind {
            NodeKind::Path{path,style,..}=>Command::SetPath{id:*id,path:path.clone(),style:PathStyle{fill:rgba,fill_paint:paint,..*style}},
            NodeKind::Text{spec,..} if paint==PathPaint::Solid => {
                let mut spec=(**spec).clone();let rgba=rgba.unwrap_or([0;4]);
                spec.color=rgba;for run in &mut spec.runs {run.style.color=rgba;}
                Command::SetText{id:*id,spec:Box::new(spec)}
            }
            NodeKind::Fill{..} if paint==PathPaint::Solid=>Command::SetFillColor{id:*id,rgba:rgba.unwrap_or([0;4])},
            _=>return Err("Fill applies to text, shapes and solid-color layers; gradients apply to vector shapes.".into()),
        })
    }).collect()
}

/// An editable cubic rectangle. Radius zero retains the four original corners.
pub fn rounded_rect(x: f64, y: f64, w: f64, h: f64, r: f64) -> Path {
    let r = r.clamp(0., w.min(h) / 2.);
    if r <= 1e-6 {
        return vector_geometry::rectangle(x, y, w, h);
    }
    let k = r * 0.552_284_749_830_793_6;
    let mut anchors = [
        (x + r, y),
        (x + w - r, y),
        (x + w, y + r),
        (x + w, y + h - r),
        (x + w - r, y + h),
        (x + r, y + h),
        (x, y + h - r),
        (x, y + r),
    ]
    .map(Anchor::corner);
    anchors[1].h_out = (x + w - r + k, y);
    anchors[2].h_in = (x + w, y + r - k);
    anchors[3].h_out = (x + w, y + h - r + k);
    anchors[4].h_in = (x + w - r + k, y + h);
    anchors[5].h_out = (x + r - k, y + h);
    anchors[6].h_in = (x, y + h - r + k);
    anchors[7].h_out = (x, y + r - k);
    anchors[0].h_in = (x + r - k, y);
    Path {
        subpaths: vec![SubPath {
            anchors: anchors.into(),
            closed: true,
        }],
    }
}

/// Recognize only rectangles made of four corners or our eight-anchor round
/// rectangle. Never replace an arbitrary illustration with its bounding box.
pub fn rectangle(path: &Path) -> Option<(f64, f64, f64, f64, f64)> {
    let (x, y, w, h) = vector_geometry::bounds(path)?;
    if w <= 0. || h <= 0. || path.subpaths.len() != 1 || !path.subpaths[0].closed {
        return None;
    }
    let anchors = &path.subpaths[0].anchors;
    let r = match anchors.len() {
        4 => 0.,
        8 => anchors[0].p.0 - x,
        _ => return None,
    };
    let expected = rounded_rect(x, y, w, h, r);
    let equal = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 0.01 && (a.1 - b.1).abs() < 0.01;
    anchors
        .iter()
        .zip(&expected.subpaths[0].anchors)
        .all(|(a, b)| equal(a.p, b.p) && equal(a.h_in, b.h_in) && equal(a.h_out, b.h_out))
        .then_some((x, y, w, h, r))
}

/// Recognize a simple native text/backdrop pair by geometry, not by layer names,
/// so renaming and project/clipboard round trips preserve these controls.
pub fn text_backdrop(doc: &Document, id: NodeId) -> Option<(NodeId, Option<(NodeId, NodeId)>)> {
    let node = doc.node(id)?;
    let group = if node.is_group() {
        Some(id)
    } else {
        node.parent
    };
    let pair = group.and_then(|group| {
        let children = doc.children(Some(group));
        if children.len() != 2
            || doc.design.frames.contains_key(&group)
            || doc.design.charts.contains_key(&group)
            || doc.design.component_links.contains_key(&group)
            || doc.design.media.contains_key(&group)
        {
            return None;
        }
        let mut text = None;
        let mut background = None;
        for child in children {
            match &doc.node(child)?.kind {
                NodeKind::Text { .. } => text = Some(child),
                NodeKind::Path { .. } => background = Some(child),
                _ => return None,
            }
        }
        let text = text?;
        let background = background?;
        background_geometry(doc, text, background)?;
        let stack = doc.children(Some(group));
        if stack.iter().position(|id| *id == background)?
            >= stack.iter().position(|id| *id == text)?
        {
            return None;
        }
        Some((text, Some((group, background))))
    });
    pair.or_else(|| matches!(node.kind, NodeKind::Text { .. }).then_some((id, None)))
}

/// Recover the backdrop in text-local space, including rotated/scaled text.
pub fn background_geometry(
    doc: &Document,
    text: NodeId,
    background: NodeId,
) -> Option<(f64, f64, f64, f64, f64)> {
    let NodeKind::Text { spec, .. } = &doc.node(text)?.kind else {
        return None;
    };
    let NodeKind::Path { path, .. } = &doc.node(background)?.kind else {
        return None;
    };
    let inverse = spec.transform().inverse();
    if !inverse.is_finite() {
        return None;
    }
    let mut local = (**path).clone();
    local.transform(inverse);
    rectangle(&local)
}

pub fn background(
    doc: &Document,
    id: NodeId,
    rgba: [u8; 4],
    padding: [f32; 2],
    radius: f32,
    remove: bool,
) -> Result<(Vec<Command>, NodeId), String> {
    if !remove
        && (padding
            .iter()
            .any(|p| !p.is_finite() || !(0. ..=1000.).contains(p))
            || !radius.is_finite()
            || !(0. ..=10000.).contains(&radius))
    {
        return Err("Background padding must be 0–1000 px and radius 0–10000 px.".into());
    }
    let (text, pair) =
        text_backdrop(doc, id).ok_or("Select one text object or a text background group.")?;
    for target in std::iter::once(text).chain(pair.into_iter().flat_map(|(group, bg)| [group, bg]))
    {
        let locks = doc.layer_locks(target);
        if doc.locked_ancestor(target).is_some()
            || locks.pixels
            || locks.position
            || locks.transparency
        {
            return Err(
                "Unlock the text and its background before changing the background.".into(),
            );
        }
    }
    let text_node = doc.node(text).ok_or("Missing text object.")?;
    let mut commands = Vec::new();
    if remove {
        if let Some((group, bg)) = pair {
            commands.push(Command::RemoveNode { id: bg });
            commands.push(Command::Ungroup { id: group });
        }
        return Ok((commands, text));
    }
    let NodeKind::Text { spec, .. } = &text_node.kind else {
        return Err("Select editable text.".into());
    };
    let b = crate::text::layout(spec).bounds();
    if b.width <= 0. || b.height <= 0. {
        return Err("The text has no visible bounds.".into());
    }
    let (px, py) = (padding[0] as f64, padding[1] as f64);
    let mut path = rounded_rect(
        b.x as f64 - px,
        b.y as f64 - py,
        b.width as f64 + px * 2.,
        b.height as f64 + py * 2.,
        radius as f64,
    );
    path.transform(spec.transform());
    let path = Arc::new(path);
    if let Some((group, bg)) = pair {
        let NodeKind::Path { style, .. } = doc.node(bg).ok_or("Missing background")?.kind else {
            return Err("Invalid background".into());
        };
        commands.push(Command::SetPath {
            id: bg,
            path,
            style: PathStyle {
                fill: Some(rgba),
                fill_paint: PathPaint::Solid,
                ..style
            },
        });
        return Ok((commands, group));
    }
    let siblings = doc.children(text_node.parent);
    let index = siblings
        .iter()
        .position(|id| *id == text)
        .ok_or("Missing text object")?;
    let add = Command::AddNode {
        node: Box::new(Node::path(
            0,
            "Text background",
            path,
            PathStyle {
                fill: Some(rgba),
                stroke: None,
                ..Default::default()
            },
            doc.width,
            doc.height,
        )),
        slot: Slot {
            parent: text_node.parent,
            index,
        },
    };
    let mut trial = doc.clone();
    let bg = add
        .clone()
        .apply(&mut trial)
        .map_err(|e| e.to_string())?
        .ok_or("No background created")?;
    commands.push(add);
    let group = Command::Group {
        ids: vec![bg, text],
        name: "Text with background".into(),
    };
    let id = group
        .clone()
        .apply(&mut trial)
        .map_err(|e| e.to_string())?
        .ok_or("No group created")?;
    commands.push(group);
    Ok((commands, id))
}

/// Round native rectangles without replacing arbitrary artwork or responsive
/// frame boundaries. Fill/stroke paint and object identities remain unchanged.
pub fn corners(doc: &Document, ids: &[NodeId], radius: f32) -> Result<Vec<Command>, String> {
    if !radius.is_finite() || !(0. ..=100000.).contains(&radius) {
        return Err("Corner radius must be 0–100000 px.".into());
    }
    ids.iter()
        .map(|id| {
            if doc
                .design
                .frames
                .values()
                .any(|frame| frame.boundary == *id)
            {
                return Err("Responsive frame boundaries cannot be rounded.".into());
            }
            let Some(NodeKind::Path { path, style, .. }) = doc.node(*id).map(|n| &n.kind) else {
                return Err("Select rectangular vector shapes to change corner radius.".into());
            };
            let (x, y, w, h, _) = rectangle(path)
                .ok_or("Only axis-aligned native rectangles support corner radius.")?;
            Ok(Command::SetPath {
                id: *id,
                path: Arc::new(rounded_rect(x, y, w, h, f64::from(radius))),
                style: *style,
            })
        })
        .collect()
}
