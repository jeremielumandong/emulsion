//! Frame fitting changes native placement only; source pixels stay embedded.
use crate::{Command, Document, NodeId, NodeKind};
use emulsion_raster::{Placement, Raster, vector_geometry};
use glam::{DAffine2, dvec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFit {
    Cover,
    Contain,
    Stretch,
}
impl ImageFit {
    pub const ALL: [Self; 3] = [Self::Cover, Self::Contain, Self::Stretch];
    pub fn label(self) -> &'static str {
        match self {
            Self::Cover => "Cover",
            Self::Contain => "Contain",
            Self::Stretch => "Stretch",
        }
    }
}

/// A single undoable crop edit. `focus` is a normalized point in the source
/// image; Cover brings it toward the frame's centre without exposing an edge.
pub fn fit_frame_image(
    doc: &Document,
    selected: NodeId,
    fit: ImageFit,
    focus: [f64; 2],
) -> Result<Command, String> {
    let (boundary, image) =
        super::frame_parts(doc, selected).ok_or("Select a frame containing an image first.")?;
    let id = image.ok_or("Place an image in this frame first.")?;
    let NodeKind::Raster {
        raster,
        placement: previous,
    } = &doc.node(id).unwrap().kind
    else {
        unreachable!()
    };
    let placement = placement(doc, boundary, raster, fit, focus, *previous)?;
    Ok(Command::SetPlacement { id, placement })
}

pub(super) fn placement(
    doc: &Document,
    boundary: NodeId,
    raster: &Raster,
    fit: ImageFit,
    focus: [f64; 2],
    previous: Placement,
) -> Result<Placement, String> {
    if focus
        .iter()
        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return Err("Choose an image focus between 0 and 100 percent.".into());
    }
    let Some(NodeKind::Path { path, .. }) = doc.node(boundary).map(|n| &n.kind) else {
        return Err("The frame boundary is not a vector shape.".into());
    };
    // Fit in the image's axes so a rotated frame/image pair keeps its angle.
    let rotate = DAffine2::from_angle(previous.rotation.to_radians());
    let mut local = (**path).clone();
    local.transform(rotate.inverse());
    let (x, y, w, h) = vector_geometry::bounds(&local).ok_or("The frame has no bounds.")?;
    if w <= 1e-8 || h <= 1e-8 || raster.width() == 0 || raster.height() == 0 {
        return Err("The frame or image is empty.".into());
    }
    let (rw, rh) = (f64::from(raster.width()), f64::from(raster.height()));
    let (sx, sy) = (w / rw, h / rh);
    let (sx, sy) = match fit {
        ImageFit::Cover => (sx.max(sy), sx.max(sy)),
        ImageFit::Contain => (sx.min(sy), sx.min(sy)),
        ImageFit::Stretch => (sx, sy),
    };
    let size = dvec2(rw * sx, rh * sy);
    let focus = [
        if previous.flip_x {
            1. - focus[0]
        } else {
            focus[0]
        },
        if previous.flip_y {
            1. - focus[1]
        } else {
            focus[1]
        },
    ];
    let position = |start: f64, span: f64, image: f64, focus: f64| {
        if fit == ImageFit::Cover {
            (start + span / 2. - image * focus).clamp(start + (span - image).min(0.), start)
        } else {
            start + (span - image) / 2.
        }
    };
    let centre = rotate.transform_point2(
        dvec2(
            position(x, w, size.x, focus[0]),
            position(y, h, size.y, focus[1]),
        ) + size / 2.,
    );
    Ok(Placement {
        x: centre.x - size.x / 2.,
        y: centre.y - size.y / 2.,
        scale_x: sx,
        scale_y: sy,
        ..previous
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Editor, Node,
        command::{AlignTarget, Alignment, Slot},
        design::{Element, frame, frame_parts, place_in_frame},
    };
    use std::sync::Arc;

    fn fixture() -> (Editor, NodeId, NodeId, NodeId, Arc<Raster>) {
        let mut editor = Editor::new(Document::new(600, 400), None);
        let group = frame(&editor.doc, Element::Rectangle)
            .paste(&mut editor, Slot::TOP, (0., 0.))
            .unwrap()[0];
        let boundary = frame_parts(&editor.doc, group).unwrap().0;
        let pixels = Arc::new(Raster::solid(400, 100, [0.2, 0.3, 0.4, 1.]));
        let image = place_in_frame(&mut editor, group, pixels.clone()).unwrap();
        (editor, group, boundary, image, pixels)
    }
    fn media(doc: &Document, id: NodeId) -> (Arc<Raster>, Placement) {
        let NodeKind::Raster { raster, placement } = &doc.node(id).unwrap().kind else {
            panic!()
        };
        (raster.clone(), *placement)
    }
    #[test]
    fn fit_focus_and_undo_preserve_pixels_clip_and_frame_bounds() {
        let (mut editor, group, boundary, image, pixels) = fixture();
        let (_, mut crop) = media(&editor.doc, image);
        crop.x += 7.;
        editor
            .execute(Command::SetPlacement {
                id: image,
                placement: crop,
            })
            .unwrap();
        let bounds = crate::geometry::node_bounds(&editor.doc, boundary).unwrap();
        let original = editor.doc.clone();
        for fit in ImageFit::ALL {
            let command = fit_frame_image(&editor.doc, group, fit, [0.5; 2]).unwrap();
            editor.execute(command).unwrap();
            let (source, p) = media(&editor.doc, image);
            assert!(Arc::ptr_eq(&source, &pixels));
            assert_eq!(editor.doc.node(image).unwrap().clip_to, Some(boundary));
            let (w, h) = (400. * p.scale_x, 100. * p.scale_y);
            match fit {
                ImageFit::Cover => {
                    assert!(w >= bounds.w as f64 && h >= bounds.h as f64);
                    assert_eq!(p.scale_x, p.scale_y);
                }
                ImageFit::Contain => {
                    assert!(w <= bounds.w as f64 && h <= bounds.h as f64);
                    assert_eq!(p.scale_x, p.scale_y);
                }
                ImageFit::Stretch => {
                    assert_eq!(w, bounds.w as f64);
                    assert_eq!(h, bounds.h as f64);
                }
            }
            assert_eq!(
                crate::geometry::node_bounds(&editor.doc, group),
                Some(bounds)
            );
            editor.undo();
            assert_eq!(editor.doc, original);
        }
        for x in [0., 1.] {
            editor
                .execute(fit_frame_image(&editor.doc, image, ImageFit::Cover, [x, 0.5]).unwrap())
                .unwrap();
            let (_, p) = media(&editor.doc, image);
            assert_eq!(
                p.x,
                bounds.x as f64 + (bounds.w as f64 - 400. * p.scale_x) * x
            );
            assert_eq!(
                crate::geometry::node_bounds(&editor.doc, group),
                Some(bounds)
            );
        }
        editor
            .execute(Command::AlignNode {
                id: group,
                alignment: Alignment::Left,
                target: AlignTarget::Canvas,
            })
            .unwrap();
        assert_eq!(
            crate::geometry::node_bounds(&editor.doc, boundary)
                .unwrap()
                .x,
            0
        );
    }

    #[test]
    fn rotated_flipped_frames_refit_without_resetting_orientation() {
        let (mut editor, group, boundary, image, pixels) = fixture();
        let (_, mut p) = media(&editor.doc, image);
        p.flip_x = true;
        editor
            .execute(Command::SetPlacement {
                id: image,
                placement: p,
            })
            .unwrap();
        editor
            .execute(Command::RotateNode {
                id: group,
                degrees: 37.,
            })
            .unwrap();
        let before = editor.doc.clone();
        let angle = media(&before, image).1.rotation;
        editor
            .execute(fit_frame_image(&editor.doc, group, ImageFit::Cover, [0.9, 0.2]).unwrap())
            .unwrap();
        let (source, p) = media(&editor.doc, image);
        assert!(Arc::ptr_eq(&source, &pixels));
        assert_eq!(p.rotation, angle);
        assert!(p.flip_x);
        let inverse = p.to_doc(400, 100).inverse();
        let NodeKind::Path { path, .. } = &editor.doc.node(boundary).unwrap().kind else {
            panic!()
        };
        for point in path
            .subpaths
            .iter()
            .flat_map(|sub| &sub.anchors)
            .map(|a| inverse.transform_point2(dvec2(a.p.0, a.p.1)))
        {
            assert!(point.x >= -1e-7 && point.x <= 400. + 1e-7);
            assert!(point.y >= -1e-7 && point.y <= 100. + 1e-7);
        }
        editor.undo();
        assert_eq!(editor.doc, before);
    }

    #[test]
    fn frame_targeting_focus_validation_and_locks_are_safe() {
        let (mut editor, group, boundary, image, pixels) = fixture();
        let unrelated = editor
            .execute(Command::AddNode {
                node: Box::new(Node::raster(0, "Unrelated", pixels, Placement::default())),
                slot: Slot::top_of(Some(group)),
            })
            .unwrap()
            .unwrap();
        assert!(frame_parts(&editor.doc, unrelated).is_none());
        assert!(fit_frame_image(&editor.doc, unrelated, ImageFit::Cover, [0.5; 2]).is_err());
        editor
            .execute(Command::SetClip {
                id: unrelated,
                clip_to: Some(boundary),
            })
            .unwrap();
        assert_eq!(
            frame_parts(&editor.doc, unrelated),
            Some((boundary, Some(unrelated)))
        );
        for focus in [[f64::NAN, 0.5], [1.01, 0.5], [-0.1, 0.5]] {
            assert!(fit_frame_image(&editor.doc, image, ImageFit::Cover, focus).is_err());
        }
        editor.doc.node_mut(group).unwrap().locks.position = true;
        let before = editor.doc.clone();
        let history = editor.history.len();
        let command = fit_frame_image(&editor.doc, image, ImageFit::Contain, [0.5; 2]).unwrap();
        assert!(editor.execute(command).is_err());
        assert_eq!(editor.doc, before);
        assert_eq!(editor.history.len(), history);
    }
}

/// A remote embed reference; no video bytes, scripts or remote thumbnails are saved.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct YouTube {
    pub video_id: String,
    #[serde(default)]
    pub start_seconds: u32,
    pub boundary: NodeId,
}
impl YouTube {
    pub fn url(&self) -> String {
        format!(
            "https://www.youtube.com/watch?v={}&t={}s",
            self.video_id, self.start_seconds
        )
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.video_id.len() != 11
            || !self
                .video_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
            || self.start_seconds > 604_800
        {
            return Err("Use a valid YouTube video and a start time of at most seven days.".into());
        }
        Ok(())
    }
}

/// Accept only ordinary YouTube video links, never arbitrary web content.
pub fn parse_youtube(input: &str) -> Result<YouTube, String> {
    let invalid = || "Paste a YouTube watch, shorts, embed or youtu.be video URL.".to_string();
    let input = input.trim();
    if input.len() > 4096
        || input
            .bytes()
            .any(|c| c.is_ascii_control() || c.is_ascii_whitespace() || c == b'\\')
    {
        return Err(invalid());
    }
    let url = input
        .strip_prefix("https://")
        .or_else(|| input.strip_prefix("http://"))
        .ok_or_else(invalid)?;
    let (host, rest) = url.split_once('/').ok_or_else(invalid)?;
    let host = host.to_ascii_lowercase();
    if ![
        "youtube.com",
        "www.youtube.com",
        "m.youtube.com",
        "youtu.be",
        "www.youtube-nocookie.com",
        "youtube-nocookie.com",
    ]
    .contains(&host.as_str())
    {
        return Err(invalid());
    }
    let (rest, fragment) = rest
        .split_once('#')
        .map_or((rest, None), |(r, f)| (r, Some(f)));
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let mut video = None;
    let mut start = None;
    let mut time_seen = false;
    for pair in query.split('&').filter(|v| !v.is_empty()) {
        let (key, value) = pair.split_once('=').ok_or_else(invalid)?;
        match key {
            "v" => {
                if video.replace(value).is_some() {
                    return Err(invalid());
                }
            }
            "t" | "start" => {
                if time_seen {
                    return Err(invalid());
                }
                time_seen = true;
                start = Some(parse_time(value).ok_or_else(invalid)?);
            }
            _ => {}
        }
    }
    if let Some(fragment) = fragment {
        if let Some(value) = fragment.strip_prefix("t=") {
            if time_seen {
                return Err(invalid());
            }
            start = Some(parse_time(value).ok_or_else(invalid)?);
        } else if !fragment.is_empty() {
            return Err(invalid());
        }
    }
    let video_id = if host == "youtu.be" {
        if video.is_some() {
            return Err(invalid());
        }
        path
    } else if path == "watch" && !host.contains("nocookie") {
        video.ok_or_else(invalid)?
    } else {
        if video.is_some() {
            return Err(invalid());
        }
        path.strip_prefix("embed/")
            .or_else(|| {
                (!host.contains("nocookie"))
                    .then(|| path.strip_prefix("shorts/"))
                    .flatten()
            })
            .ok_or_else(invalid)?
    };
    let video = YouTube {
        video_id: video_id.into(),
        start_seconds: start.unwrap_or(0),
        boundary: 0,
    };
    video.validate()?;
    Ok(video)
}
fn parse_time(input: &str) -> Option<u32> {
    if !input.is_empty() && input.bytes().all(|b| b.is_ascii_digit()) {
        return input.parse().ok();
    }
    let mut number = String::new();
    let mut total = 0u32;
    let mut previous = u32::MAX;
    for c in input.chars() {
        if c.is_ascii_digit() {
            number.push(c);
            continue;
        }
        let factor = match c {
            'h' => 3600,
            'm' => 60,
            's' => 1,
            _ => return None,
        };
        if factor >= previous || number.is_empty() {
            return None;
        }
        total = total.checked_add(number.parse::<u32>().ok()?.checked_mul(factor)?)?;
        number.clear();
        previous = factor;
    }
    (number.is_empty() && previous != u32::MAX).then_some(total)
}

fn rectangle(doc: &Document, id: NodeId) -> Option<(f64, f64, f64, f64)> {
    let NodeKind::Path { path, .. } = &doc.node(id)?.kind else {
        return None;
    };
    let sub = path.subpaths.first()?;
    if path.subpaths.len() != 1 || !sub.closed || sub.anchors.len() != 4 {
        return None;
    }
    let b = vector_geometry::bounds(path)?;
    if ![b.0, b.1, b.2, b.3].into_iter().all(f64::is_finite) || b.2 < 1. || b.3 < 1. {
        return None;
    }
    let corners = [
        (b.0, b.1),
        (b.0 + b.2, b.1),
        (b.0 + b.2, b.1 + b.3),
        (b.0, b.1 + b.3),
    ];
    for (i, a) in sub.anchors.iter().enumerate() {
        if (a.p.0 - corners[i].0).abs() > 1e-7 || (a.p.1 - corners[i].1).abs() > 1e-7 {
            return None;
        }
        let next = &sub.anchors[(i + 1) % 4];
        if a.p != a.h_in
            || a.p != a.h_out
            || !((a.p.0 - next.p.0).abs() < 1e-7 || (a.p.1 - next.p.1).abs() < 1e-7)
            || !((a.p.0 - b.0).abs() < 1e-7 || (a.p.0 - b.0 - b.2).abs() < 1e-7)
            || !((a.p.1 - b.1).abs() < 1e-7 || (a.p.1 - b.1 - b.3).abs() < 1e-7)
        {
            return None;
        }
    }
    Some(b)
}

pub(crate) fn validate(
    media: &std::collections::BTreeMap<NodeId, YouTube>,
    doc: &Document,
) -> Result<(), String> {
    if media.len() > 64 {
        return Err("A page supports up to 64 video embeds.".into());
    }
    for (group, video) in media {
        video.validate()?;
        if !doc.node(*group).is_some_and(|n| n.is_group())
            || !doc
                .node(video.boundary)
                .is_some_and(|n| n.parent == Some(*group))
            || rectangle(doc, video.boundary).is_none()
        {
            return Err("A video needs its own rectangular frame. Detach the video before rotating or reshaping it.".into());
        }
    }
    Ok(())
}

/// Playback bounds in document coordinates. Hidden ancestors suppress playback.
pub fn bounds(doc: &Document, id: NodeId) -> Option<(f64, f64, f64, f64)> {
    let boundary = doc
        .design
        .media
        .get(&id)
        .map(|video| video.boundary)
        .or_else(|| doc.design.local_media.get(&id).map(|media| media.boundary))?;
    let mut current = Some(boundary);
    for _ in 0..=doc.nodes.len() {
        let Some(id) = current else {
            return rectangle(doc, boundary);
        };
        let node = doc.node(id)?;
        if !node.visible || node.opacity <= 0. {
            return None;
        }
        current = node.parent;
    }
    None
}
fn editable(doc: &Document, id: NodeId) -> Result<(), String> {
    if doc.locked_ancestor(id).is_some() || doc.node(id).is_none() {
        return Err("Unlock the video before editing it.".into());
    }
    Ok(())
}
fn commit(editor: &mut crate::Editor, commands: Vec<Command>) -> Result<(), String> {
    if editor.in_transaction() {
        return Err("Finish the current edit first.".into());
    }
    let mut trial = editor.doc.clone();
    for command in &commands {
        command.apply(&mut trial).map_err(|e| e.to_string())?;
    }
    editor.begin("Edit YouTube video");
    for command in commands {
        if let Err(error) = editor.execute(command) {
            editor.cancel();
            return Err(error.to_string());
        }
    }
    editor.end();
    Ok(())
}

/// Adds an editable offline poster and its video link as one undoable edit.
pub fn insert_youtube(
    editor: &mut crate::Editor,
    url: &str,
    origin: (f64, f64),
    size: (f64, f64),
) -> Result<NodeId, String> {
    use crate::{Node, command::Slot};
    use emulsion_raster::vector::{Anchor, Path, PathStyle, SubPath};
    use std::sync::Arc;
    let mut video = parse_youtube(url)?;
    if ![origin.0, origin.1]
        .into_iter()
        .all(|n| n.is_finite() && n.abs() <= 100_000.)
        || ![size.0, size.1]
            .into_iter()
            .all(|n| n.is_finite() && (1. ..=100_000.).contains(&n))
    {
        return Err("Choose a video frame of 1–100000 px with a finite position.".into());
    }
    let (x, y) = origin;
    let (w, h) = size;
    let mut trial = editor.doc.clone();
    let mut commands = Vec::new();
    let mut add = |node, parent| -> Result<NodeId, String> {
        let command = Command::AddNode {
            node: Box::new(node),
            slot: Slot::top_of(parent),
        };
        let id = command
            .apply(&mut trial)
            .map_err(|e| e.to_string())?
            .ok_or("Video object not created")?;
        commands.push(command);
        Ok(id)
    };
    let group = add(Node::group(0, "YouTube video"), None)?;
    let shape = |name, path, color| {
        Node::path(
            0,
            name,
            Arc::new(path),
            PathStyle {
                fill: Some(color),
                stroke: None,
                ..Default::default()
            },
            editor.doc.width,
            editor.doc.height,
        )
    };
    video.boundary = add(
        shape(
            "Video frame",
            vector_geometry::rectangle(x, y, w, h),
            [22, 24, 29, 255],
        ),
        Some(group),
    )?;
    let r = w.min(h) * 0.10;
    let path = Path {
        subpaths: vec![SubPath {
            closed: true,
            anchors: vec![
                Anchor::corner((x + w / 2. - r / 2., y + h / 2. - r)),
                Anchor::corner((x + w / 2. + r, y + h / 2.)),
                Anchor::corner((x + w / 2. - r / 2., y + h / 2. + r)),
            ],
        }],
    };
    add(shape("Play video", path, [255, 255, 255, 255]), Some(group))?;
    add(
        Node::text(
            0,
            "Video label",
            crate::text::TextSpec {
                text: "YouTube".into(),
                x: (x + 16.) as f32,
                y: (y + h - 40.) as f32,
                size: 20.,
                color: [220, 220, 225, 255],
                font: "Geist".into(),
                ..Default::default()
            },
            editor.doc.width,
            editor.doc.height,
        ),
        Some(group),
    )?;
    let mut design = trial.design.clone();
    design.media.insert(group, video);
    commands.push(Command::SetDesign {
        design: Box::new(design),
    });
    commit(editor, commands)?;
    Ok(group)
}
/// Updating a link preserves every editable poster object and its placement.
pub fn update_youtube(editor: &mut crate::Editor, id: NodeId, url: &str) -> Result<(), String> {
    editable(&editor.doc, id)?;
    let previous = editor
        .doc
        .design
        .media
        .get(&id)
        .ok_or("Select a YouTube video first.")?;
    let mut video = parse_youtube(url)?;
    video.boundary = previous.boundary;
    let mut design = editor.doc.design.clone();
    design.media.insert(id, video);
    commit(
        editor,
        vec![Command::SetDesign {
            design: Box::new(design),
        }],
    )
}
/// Leaves native poster artwork on the page and removes only the embed link.
pub fn detach_youtube(editor: &mut crate::Editor, id: NodeId) -> Result<(), String> {
    editable(&editor.doc, id)?;
    let mut design = editor.doc.design.clone();
    if design.media.remove(&id).is_none() {
        return Err("Select a YouTube video first.".into());
    }
    commit(
        editor,
        vec![Command::SetDesign {
            design: Box::new(design),
        }],
    )
}

#[cfg(test)]
mod youtube_tests {
    use super::*;
    use crate::{Editor, command::Slot, fragment::Fragment};
    const URL: &str = "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=1m30s";
    #[test]
    fn parser_accepts_only_video_links_and_bounded_start_times() {
        for url in [
            URL,
            "https://youtu.be/dQw4w9WgXcQ?si=share&t=90",
            "https://m.youtube.com/shorts/dQw4w9WgXcQ?start=90",
            "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ#t=1m30s",
        ] {
            let video = parse_youtube(url).unwrap();
            assert_eq!(video.video_id, "dQw4w9WgXcQ");
            assert_eq!(video.start_seconds, 90);
            assert_eq!(parse_youtube(&video.url()).unwrap(), video);
        }
        for url in [
            "https://evil.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com.evil.com/watch?v=dQw4w9WgXcQ",
            "https://user@youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com:443/watch?v=dQw4w9WgXcQ",
            "javascript:bad()",
            "https://youtu.be/short",
            "https://youtu.be/dQw4w9WgXcQ/extra",
            "https://youtu.be/dQw4w9WgXcQ?t=-1",
            "https://youtu.be/dQw4w9WgXcQ?t=700000",
            "https://youtu.be/dQw4w9WgXcQ?t=1m1h",
            "https://youtu.be/dQw4w9WgXcQ?t=1&t=2",
            "https://youtube.com/watch?v=dQw4w9WgXcQ&v=aaaaaaaaaaa",
            "https://youtu.be/dQw4w9WgXcQ?t=1%26start=3",
            "https://youtu.be/dQw4w9WgXcQ\n?x=y",
        ] {
            assert!(parse_youtube(url).is_err(), "{url}");
        }
    }
    #[test]
    fn video_metadata_survives_duplicate_clipboard_and_undo() {
        let mut editor = Editor::new(Document::new(800, 600), None);
        let empty = editor.doc.clone();
        let group = insert_youtube(&mut editor, URL, (20., 30.), (400., 225.)).unwrap();
        let original = editor.doc.clone();
        assert_eq!(bounds(&editor.doc, group), Some((20., 30., 400., 225.)));
        assert!(editor.doc.nodes.iter().all(|n| matches!(
            n.kind,
            NodeKind::Group { .. } | NodeKind::Path { .. } | NodeKind::Text { .. }
        )));
        editor.undo();
        assert_eq!(editor.doc, empty);
        editor.redo();
        assert_eq!(editor.doc, original);
        let copy = editor
            .execute(Command::DuplicateNode { id: group })
            .unwrap()
            .unwrap();
        assert_ne!(
            editor.doc.design.media[&copy].boundary,
            editor.doc.design.media[&group].boundary
        );
        assert_eq!(bounds(&editor.doc, copy), bounds(&editor.doc, group));
        editor.undo();
        assert_eq!(editor.doc, original);
        let fragment = Fragment::capture(&editor.doc, &[group]).unwrap();
        let pasted = fragment.paste(&mut editor, Slot::TOP, (30., 40.)).unwrap()[0];
        assert_eq!(bounds(&editor.doc, pasted), Some((50., 70., 400., 225.)));
        assert_ne!(
            editor.doc.design.media[&pasted].boundary,
            editor.doc.design.media[&group].boundary
        );
        editor.undo();
        assert_eq!(editor.doc, original);
        update_youtube(&mut editor, group, "https://youtu.be/aaaaaaaaaaa").unwrap();
        assert_eq!(editor.doc.nodes, original.nodes);
        editor.undo();
        assert_eq!(editor.doc, original);
        detach_youtube(&mut editor, group).unwrap();
        assert!(editor.doc.design.media.is_empty());
        assert_eq!(editor.doc.nodes, original.nodes);
        editor.undo();
        assert_eq!(editor.doc, original);
        let boundary = editor.doc.design.media[&group].boundary;
        editor
            .execute(Command::RemoveNode { id: boundary })
            .unwrap();
        assert!(editor.doc.design.media.is_empty());
        editor.undo();
        assert_eq!(editor.doc, original);
    }
    #[test]
    fn playback_honors_visibility_and_unsupported_transforms_fail_atomically() {
        let mut editor = Editor::new(Document::new(800, 600), None);
        let group = insert_youtube(&mut editor, URL, (20., 30.), (400., 225.)).unwrap();
        let original = editor.doc.clone();
        for degrees in [37., 90., 180.] {
            assert!(
                editor
                    .execute(Command::RotateNode { id: group, degrees })
                    .is_err()
            );
            assert_eq!(editor.doc, original);
        }
        editor.doc.node_mut(group).unwrap().visible = false;
        assert!(bounds(&editor.doc, group).is_none());
        editor.doc.node_mut(group).unwrap().visible = true;
        editor.doc.node_mut(group).unwrap().locked = true;
        assert!(bounds(&editor.doc, group).is_some());
        assert!(update_youtube(&mut editor, group, URL).is_err());
        assert!(detach_youtube(&mut editor, group).is_err());
        editor.doc.node_mut(group).unwrap().locked = false;
        assert_eq!(editor.doc, original);
        let history = editor.history.len();
        assert!(insert_youtube(&mut editor, "https://evil.com", (0., 0.), (400., 225.)).is_err());
        assert_eq!(editor.doc, original);
        assert_eq!(editor.history.len(), history);
        let old: crate::design_metadata::Design = serde_json::from_str("{}").unwrap();
        assert!(old.media.is_empty());
    }
    #[test]
    fn independent_video_additions_remap_metadata_when_branches_merge() {
        let base = Document::new(800, 600);
        let mut ours = Editor::new(base.clone(), None);
        let mut theirs = Editor::new(base.clone(), None);
        insert_youtube(&mut ours, URL, (10., 10.), (400., 225.)).unwrap();
        insert_youtube(
            &mut theirs,
            "https://youtu.be/aaaaaaaaaaa",
            (50., 60.),
            (400., 225.),
        )
        .unwrap();
        let outcome =
            crate::graph::merge(&base, &ours.doc, &theirs.doc, &Default::default()).unwrap();
        let crate::graph::MergeOutcome::Merged(doc) = outcome else {
            panic!("independent media additions conflict");
        };
        assert_eq!(doc.design.media.len(), 2);
        assert!(
            doc.design
                .media
                .iter()
                .all(|(id, _)| bounds(&doc, *id).is_some())
        );
        doc.validate().unwrap();
    }
}

#[path = "design_local_media.rs"]
mod local;
pub use local::{
    LocalMedia, LocalMediaKind, MAX_LOCAL_ASSET_BYTES, MAX_LOCAL_PAGE_BYTES, detach_local,
    insert_local, update_local, validate_local,
};
