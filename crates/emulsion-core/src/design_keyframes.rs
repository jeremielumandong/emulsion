//! Nonmutating native property animation with bounded, deterministic tracks.
use crate::{Command, Document, Editor, NodeId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Property {
    TranslationX,
    TranslationY,
    ScaleX,
    ScaleY,
    Rotation,
    Opacity,
    Visibility,
    TextReveal,
}
impl Property {
    pub const ALL: [Self; 8] = [
        Self::TranslationX,
        Self::TranslationY,
        Self::ScaleX,
        Self::ScaleY,
        Self::Rotation,
        Self::Opacity,
        Self::Visibility,
        Self::TextReveal,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::TranslationX => "Horizontal offset · px",
            Self::TranslationY => "Vertical offset · px",
            Self::ScaleX => "Horizontal scale",
            Self::ScaleY => "Vertical scale",
            Self::Rotation => "Rotation · degrees",
            Self::Opacity => "Opacity multiplier",
            Self::Visibility => "Visible (0 hidden, 1 shown)",
            Self::TextReveal => "Text revealed · fraction",
        }
    }
    pub fn initial(self) -> f64 {
        if matches!(
            self,
            Self::ScaleX | Self::ScaleY | Self::Opacity | Self::Visibility | Self::TextReveal
        ) {
            1.
        } else {
            0.
        }
    }
    fn valid(self, value: f64) -> bool {
        value.is_finite()
            && match self {
                Self::TranslationX | Self::TranslationY => (-100_000. ..=100_000.).contains(&value),
                Self::ScaleX | Self::ScaleY => (0.01..=100.).contains(&value),
                Self::Rotation => (-36_000. ..=36_000.).contains(&value),
                Self::Opacity | Self::Visibility | Self::TextReveal => (0. ..=1.).contains(&value),
            }
    }
}
pub use crate::motion::Easing;
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keyframe {
    pub time_ms: u32,
    pub value: f64,
    #[serde(default)]
    pub easing: Easing,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub property: Property,
    pub frames: Vec<Keyframe>,
}
impl Track {
    pub fn sample(&self, time_ms: u32) -> f64 {
        crate::motion::sample_by(&self.frames, f64::from(time_ms), |k| {
            crate::motion::KeyView {
                time: f64::from(k.time_ms),
                value: k.value,
                easing: k.easing,
                curve: None,
            }
        })
        .unwrap_or_else(|| self.property.initial())
    }
}
pub fn validate(
    tracks: &BTreeMap<NodeId, Vec<Track>>,
    doc: &Document,
    duration_ms: u32,
) -> Result<(), String> {
    validate_with_media(
        tracks,
        doc,
        duration_ms,
        doc.design
            .media
            .values()
            .map(|m| m.boundary)
            .chain(doc.design.local_media.values().map(|m| m.boundary)),
    )
}
pub(crate) fn validate_with_media(
    tracks: &BTreeMap<NodeId, Vec<Track>>,
    doc: &Document,
    duration_ms: u32,
    boundaries: impl IntoIterator<Item = NodeId>,
) -> Result<(), String> {
    let boundaries: Vec<_> = boundaries.into_iter().collect();
    if tracks.len() > 256
        || tracks
            .values()
            .flatten()
            .map(|t| t.frames.len())
            .sum::<usize>()
            > 4096
    {
        return Err("A page supports 256 animated objects and 4096 property keyframes.".into());
    }
    for (id, tracks) in tracks {
        if doc.node(*id).is_none() || tracks.is_empty() || tracks.len() > Property::ALL.len() {
            return Err(
                "Property tracks need an existing object and at most eight properties.".into(),
            );
        }
        let mut seen = HashSet::new();
        let has_media = boundaries
            .iter()
            .any(|boundary| *boundary == *id || doc.is_ancestor(*id, *boundary));
        for track in tracks {
            if track.property == Property::TextReveal
                && !matches!(
                    doc.node(*id).map(|n| &n.kind),
                    Some(crate::NodeKind::Text { .. })
                )
            {
                return Err("Text reveal requires a native text object.".into());
            }
            if !seen.insert(track.property as u8)
                || track.frames.is_empty()
                || track.frames.len() > 64
                || track
                    .frames
                    .windows(2)
                    .any(|p| p[0].time_ms >= p[1].time_ms)
                || track
                    .frames
                    .iter()
                    .any(|f| f.time_ms > duration_ms || !track.property.valid(f.value))
            {
                return Err("Use 1–64 ordered, unique keyframes per property, inside page duration and valid value limits.".into());
            }
            if has_media
                && track.property == Property::Rotation
                && track.frames.iter().any(|f| f.value != 0.)
            {
                return Err("Linked audio/video frames cannot rotate. Detach media before animating rotation.".into());
            }
        }
    }
    Ok(())
}
fn editable(editor: &Editor, id: NodeId) -> Result<(), String> {
    if editor.in_transaction()
        || editor.doc.node(id).is_none()
        || editor.doc.locked_ancestor(id).is_some()
        || editor.doc.layer_locks(id).position
        || editor.doc.layer_locks(id).pixels
        || editor.doc.layer_locks(id).transparency
    {
        Err("Choose an unlocked object and finish the current edit first.".into())
    } else {
        Ok(())
    }
}
pub fn set_keyframe(
    editor: &mut Editor,
    id: NodeId,
    property: Property,
    keyframe: Keyframe,
) -> Result<(), String> {
    editable(editor, id)?;
    let mut design = editor.doc.design.clone();
    let tracks = design.keyframes.entry(id).or_default();
    if let Some(track) = tracks.iter_mut().find(|t| t.property == property) {
        if let Some(frame) = track
            .frames
            .iter_mut()
            .find(|f| f.time_ms == keyframe.time_ms)
        {
            *frame = keyframe;
        } else {
            track.frames.push(keyframe);
            track.frames.sort_by_key(|f| f.time_ms);
        }
    } else {
        tracks.push(Track {
            property,
            frames: vec![keyframe],
        });
    }
    editor
        .execute(Command::SetDesign {
            design: Box::new(design),
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}
pub fn remove_keyframe(
    editor: &mut Editor,
    id: NodeId,
    property: Property,
    time_ms: u32,
) -> Result<(), String> {
    editable(editor, id)?;
    let mut design = editor.doc.design.clone();
    let tracks = design
        .keyframes
        .get_mut(&id)
        .ok_or("Object has no property keyframes")?;
    let track = tracks
        .iter_mut()
        .find(|t| t.property == property)
        .ok_or("Property has no keyframes")?;
    let previous = track.frames.len();
    track.frames.retain(|f| f.time_ms != time_ms);
    if previous == track.frames.len() {
        return Err("No keyframe at this time".into());
    }
    tracks.retain(|t| !t.frames.is_empty());
    if tracks.is_empty() {
        design.keyframes.remove(&id);
    }
    editor
        .execute(Command::SetDesign {
            design: Box::new(design),
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}
pub fn clear(editor: &mut Editor, id: NodeId) -> Result<(), String> {
    editable(editor, id)?;
    let mut design = editor.doc.design.clone();
    if design.keyframes.remove(&id).is_none() {
        return Err("Object has no property keyframes".into());
    }
    editor
        .execute(Command::SetDesign {
            design: Box::new(design),
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}
pub fn evaluate(source: &Document, time_ms: u32) -> Result<Document, String> {
    validate(&source.design.keyframes, source, source.design.duration_ms)?;
    let mut doc = source.clone();
    // Parent transforms run first; child offsets and scales compose afterward.
    let order = crate::motion::parents_first(source, source.design.keyframes.keys().copied());
    crate::motion::with_layers_unlocked(&mut doc, |doc| {
        for id in &order {
            animate(doc, id, &source.design.keyframes[id], time_ms)?;
        }
        Ok(())
    })?;
    Ok(doc)
}

fn animate(doc: &mut Document, id: &NodeId, tracks: &[Track], time_ms: u32) -> Result<(), String> {
    let original_bounds = crate::geometry::node_bounds(doc, *id).map_err(|e| e.to_string())?;
    let mut x = 0.;
    let mut y = 0.;
    let mut sx = 1.;
    let mut sy = 1.;
    let mut angle = 0.;
    for track in tracks {
        let value = track.sample(time_ms);
        match track.property {
            Property::TranslationX => x = value,
            Property::TranslationY => y = value,
            Property::ScaleX => sx = value,
            Property::ScaleY => sy = value,
            Property::Rotation => angle = value,
            Property::Visibility => {
                if let Some(node) = doc.node_mut(*id) {
                    node.visible &= value >= 0.5;
                }
            }
            Property::TextReveal => {
                use unicode_segmentation::UnicodeSegmentation;
                let (width, height) = (doc.width, doc.height);
                if let Some(crate::NodeKind::Text { spec, cache }) =
                    doc.node_mut(*id).map(|n| &mut n.kind)
                {
                    let mut next = (**spec).clone();
                    let indices: Vec<_> =
                        next.text.grapheme_indices(true).map(|(i, _)| i).collect();
                    let count = (indices.len() as f64 * value).floor() as usize;
                    let end = indices.get(count).copied().unwrap_or(next.text.len());
                    next.replace_range(end..next.text.len(), "");
                    let next = std::sync::Arc::new(next);
                    *cache = crate::vector_cache::VectorRaster::text(next.clone(), width, height);
                    *spec = next;
                }
            }
            Property::Opacity => {
                if let Some(node) = doc.node_mut(*id) {
                    node.opacity *= value as f32;
                }
            }
        }
    }
    if x != 0. || y != 0. || sx != 1. || sy != 1. || angle != 0. {
        let b = original_bounds.ok_or("Animated object has no geometry")?;
        let center = glam::dvec2(
            f64::from(b.x) + f64::from(b.w) / 2.,
            f64::from(b.y) + f64::from(b.h) / 2.,
        );
        let transform = glam::DAffine2::from_translation(center + glam::dvec2(x, y))
            * glam::DAffine2::from_angle(angle.to_radians())
            * glam::DAffine2::from_scale(glam::dvec2(sx, sy))
            * glam::DAffine2::from_translation(-center);
        crate::transform::transform_nodes(doc, &[*id], transform.to_cols_array())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn fixture() -> (Editor, NodeId) {
        let mut editor = Editor::new(Document::new(800, 600), None);
        let id = crate::design::media::insert_youtube(
            &mut editor,
            "https://youtu.be/M7lc1UVf-VE",
            (10., 20.),
            (400., 225.),
        )
        .unwrap();
        (editor, id)
    }
    #[test]
    fn design_keyframes_interpolate_without_mutating_source_and_undo() {
        let (mut editor, id) = fixture();
        set_keyframe(
            &mut editor,
            id,
            Property::TranslationX,
            Keyframe {
                time_ms: 0,
                value: 0.,
                easing: Easing::EaseIn,
            },
        )
        .unwrap();
        set_keyframe(
            &mut editor,
            id,
            Property::TranslationX,
            Keyframe {
                time_ms: 1000,
                value: 100.,
                easing: Easing::Linear,
            },
        )
        .unwrap();
        let original = editor.doc.clone();
        let preview = crate::design_metadata::at_time(&editor.doc, 500).unwrap();
        assert_eq!(crate::design::media::bounds(&preview, id).unwrap().0, 35.);
        assert_eq!(editor.doc.nodes, original.nodes);
        assert_eq!(editor.doc.design, original.design);
        remove_keyframe(&mut editor, id, Property::TranslationX, 1000).unwrap();
        editor.undo();
        assert_eq!(editor.doc.design, original.design);
        clear(&mut editor, id).unwrap();
        assert!(editor.doc.design.keyframes.is_empty());
        editor.undo();
        assert_eq!(editor.doc.design, original.design);
    }
    #[test]
    fn design_keyframes_reject_invalid_tracks_atomically() {
        let (mut editor, id) = fixture();
        let original = editor.doc.clone();
        let revision = editor.revision;
        for (p, time, value) in [
            (Property::Opacity, 0, 2.),
            (Property::ScaleX, 0, 0.),
            (Property::TranslationX, 60_001, 1.),
            (Property::Rotation, 0, 10.),
        ] {
            assert!(
                set_keyframe(
                    &mut editor,
                    id,
                    p,
                    Keyframe {
                        time_ms: time,
                        value,
                        easing: Easing::Linear
                    }
                )
                .is_err()
            );
            assert_eq!(editor.doc.design, original.design);
            assert_eq!(editor.revision, revision);
        }
        editor.doc.node_mut(id).unwrap().locked = true;
        assert!(
            set_keyframe(
                &mut editor,
                id,
                Property::Opacity,
                Keyframe {
                    time_ms: 0,
                    value: 0.5,
                    easing: Easing::Linear
                }
            )
            .is_err()
        );
    }
    #[test]
    fn design_keyframes_compose_with_bound_opacity_without_overwriting_it() {
        let (mut editor, id) = fixture();
        crate::design_variables::put(
            &mut editor,
            None,
            "Opacity",
            crate::design_variables::Value::Number(0.6),
        )
        .unwrap();
        crate::design_variables::bind(
            &mut editor,
            &[id],
            crate::design_variables::Property::Opacity,
            Some("Opacity"),
        )
        .unwrap();
        set_keyframe(
            &mut editor,
            id,
            Property::Opacity,
            Keyframe {
                time_ms: 0,
                value: 0.5,
                easing: Easing::Linear,
            },
        )
        .unwrap();
        let original = editor.doc.clone();
        let preview = crate::design_metadata::at_time(&editor.doc, 500).unwrap();
        assert!((preview.node(id).unwrap().opacity - 0.3).abs() < 1e-6);
        assert_eq!(
            preview.design.variable_bindings,
            original.design.variable_bindings
        );
        assert_eq!(editor.doc, original);
    }
    #[test]
    fn design_keyframes_proposed_media_and_rotation_validate_together() {
        let (mut editor, id) = fixture();
        let video = editor.doc.design.media[&id].clone();
        crate::design::media::detach_youtube(&mut editor, id).unwrap();
        let before = editor.doc.clone();
        let revision = editor.revision;
        let mut design = editor.doc.design.clone();
        design.media.insert(id, video);
        design.keyframes.insert(
            id,
            vec![Track {
                property: Property::Rotation,
                frames: vec![Keyframe {
                    time_ms: 0,
                    value: 20.,
                    easing: Easing::Linear,
                }],
            }],
        );
        assert!(
            editor
                .execute(Command::SetDesign {
                    design: Box::new(design)
                })
                .is_err()
        );
        assert_eq!(editor.doc, before);
        assert_eq!(editor.revision, revision);
        set_keyframe(
            &mut editor,
            id,
            Property::Rotation,
            Keyframe {
                time_ms: 0,
                value: 20.,
                easing: Easing::Linear,
            },
        )
        .unwrap();
        assert!(crate::design_metadata::at_time(&editor.doc, 0).is_ok());
    }
    #[test]
    fn design_keyframe_easing_and_hold_are_deterministic() {
        assert_eq!(Easing::EaseIn.sample(0.5), 0.25);
        assert_eq!(Easing::EaseOut.sample(0.5), 0.75);
        assert_eq!(Easing::EaseInOut.sample(0.5), 0.5);
        let track = Track {
            property: Property::Opacity,
            frames: vec![
                Keyframe {
                    time_ms: 100,
                    value: 0.2,
                    easing: Easing::Step,
                },
                Keyframe {
                    time_ms: 200,
                    value: 0.8,
                    easing: Easing::Linear,
                },
            ],
        };
        assert_eq!(track.sample(0), 0.2);
        assert_eq!(track.sample(199), 0.2);
        assert_eq!(track.sample(200), 0.8);
        assert_eq!(track.sample(1000), 0.8);
    }
}

#[path = "design_motion_authoring.rs"]
mod authoring;
pub use authoring::{Preset, apply_preset, retime};

#[cfg(test)]
mod reveal_tests {
    use super::*;
    #[test]
    fn design_text_reveal_preserves_graphemes_and_authored_visibility() {
        let mut editor = Editor::new(Document::new(320, 200), None);
        let id = editor
            .execute(Command::AddNode {
                node: Box::new(crate::Node::text(
                    0,
                    "Typewriter",
                    crate::text::TextSpec {
                        text: "A👩‍💻e\u{301}B".into(),
                        ..Default::default()
                    },
                    320,
                    200,
                )),
                slot: crate::command::Slot::TOP,
            })
            .unwrap()
            .unwrap();
        apply_preset(&mut editor, &[id], Preset::Typewriter, 0, 1000).unwrap();
        let original = editor.doc.clone();
        let preview = crate::design_metadata::at_time(&editor.doc, 500).unwrap();
        let crate::NodeKind::Text { spec, .. } = &preview.node(id).unwrap().kind else {
            panic!()
        };
        assert_eq!(spec.text, "A👩‍💻");
        assert_eq!(editor.doc, original);
        set_keyframe(
            &mut editor,
            id,
            Property::Visibility,
            Keyframe {
                time_ms: 0,
                value: 1.,
                easing: Easing::Step,
            },
        )
        .unwrap();
        editor.doc.node_mut(id).unwrap().visible = false;
        let preview = crate::design_metadata::at_time(&editor.doc, 500).unwrap();
        assert!(!preview.node(id).unwrap().visible);
    }
}
