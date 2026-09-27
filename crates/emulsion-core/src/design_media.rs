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
