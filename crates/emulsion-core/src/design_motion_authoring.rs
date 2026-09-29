//! Atomic bulk timing and original native-track presets.
use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    FadeIn,
    SlideUp,
    Pop,
    Pulse,
    Spin,
    Typewriter,
}
impl Preset {
    pub const ALL: [Self; 6] = [
        Self::FadeIn,
        Self::SlideUp,
        Self::Pop,
        Self::Pulse,
        Self::Spin,
        Self::Typewriter,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::FadeIn => "Soft entrance",
            Self::SlideUp => "Rise",
            Self::Pop => "Pop into place",
            Self::Pulse => "Gentle pulse",
            Self::Spin => "Full turn",
            Self::Typewriter => "Typewriter reveal",
        }
    }
}
fn selection(editor: &Editor, ids: &[NodeId]) -> Result<Vec<NodeId>, String> {
    if ids.is_empty() || ids.len() > 256 {
        return Err("Choose 1–256 objects.".into());
    }
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(*id) {
            return Err("Object IDs must be unique.".into());
        }
        editable(editor, *id)?;
    }
    Ok(ids.to_vec())
}
pub fn retime(
    editor: &mut Editor,
    ids: &[NodeId],
    scale: f64,
    offset_ms: i64,
    duration_ms: Option<u32>,
) -> Result<(), String> {
    let ids = selection(editor, ids)?;
    if !scale.is_finite()
        || !(0.01..=100.).contains(&scale)
        || !(-60_000..=60_000).contains(&offset_ms)
    {
        return Err("Choose timing scale 0.01–100 and offset within 60 seconds.".into());
    }
    let mut design = editor.doc.design.clone();
    if let Some(duration) = duration_ms {
        design.duration_ms = duration;
    }
    let time = |time: u32| -> Result<u32, String> {
        let value = (f64::from(time) * scale).round() + offset_ms as f64;
        if !(0. ..=f64::from(design.duration_ms)).contains(&value) {
            return Err("Retimed points must remain within page duration.".into());
        }
        Ok(value as u32)
    };
    let mut changed = false;
    for id in ids {
        if let Some(tracks) = design.keyframes.get_mut(&id) {
            for track in tracks {
                for frame in &mut track.frames {
                    frame.time_ms = time(frame.time_ms)?;
                }
            }
            changed = true;
        }
        if let Some(motion) = design.motion.get_mut(&id) {
            motion.start_ms = time(motion.start_ms)?;
            motion.end_ms = time(motion.end_ms)?;
            motion.transition_ms = (f64::from(motion.transition_ms) * scale).round().max(1.) as u32;
            changed = true;
        }
    }
    if !changed {
        return Err("Selected objects have no motion or property tracks.".into());
    }
    editor
        .execute(Command::SetDesign {
            design: Box::new(design),
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}
pub fn apply_preset(
    editor: &mut Editor,
    ids: &[NodeId],
    preset: Preset,
    start_ms: u32,
    end_ms: u32,
) -> Result<(), String> {
    let ids = selection(editor, ids)?;
    if start_ms >= end_ms || end_ms > editor.doc.design.duration_ms || end_ms - start_ms < 2 {
        return Err("Choose an interval of at least 2 ms inside page duration.".into());
    }
    let mid = start_ms + (end_ms - start_ms) / 2;
    let frame = |time_ms, value| Keyframe {
        time_ms,
        value,
        easing: Easing::EaseInOut,
    };
    let track = |property, values: Vec<(u32, f64)>| Track {
        property,
        frames: values.into_iter().map(|(t, v)| frame(t, v)).collect(),
    };
    let presets = match preset {
        Preset::FadeIn => vec![track(Property::Opacity, vec![(start_ms, 0.), (end_ms, 1.)])],
        Preset::SlideUp => vec![
            track(Property::TranslationY, vec![(start_ms, 60.), (end_ms, 0.)]),
            track(Property::Opacity, vec![(start_ms, 0.), (end_ms, 1.)]),
        ],
        Preset::Pop => [Property::ScaleX, Property::ScaleY]
            .into_iter()
            .map(|p| track(p, vec![(start_ms, 0.65), (mid, 1.08), (end_ms, 1.)]))
            .collect(),
        Preset::Pulse => [Property::ScaleX, Property::ScaleY]
            .into_iter()
            .map(|p| track(p, vec![(start_ms, 1.), (mid, 1.06), (end_ms, 1.)]))
            .collect(),
        Preset::Typewriter => vec![track(
            Property::TextReveal,
            vec![(start_ms, 0.), (end_ms, 1.)],
        )],
        Preset::Spin => vec![track(
            Property::Rotation,
            vec![(start_ms, 0.), (end_ms, 360.)],
        )],
    };
    let mut design = editor.doc.design.clone();
    for id in ids {
        let tracks = design.keyframes.entry(id).or_default();
        for track in &presets {
            tracks.retain(|old| old.property != track.property);
            tracks.push(track.clone());
        }
    }
    editor
        .execute(Command::SetDesign {
            design: Box::new(design),
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn design_motion_presets_and_bulk_retime_are_atomic_one_undo() {
        let (mut editor, id) = super::super::tests::fixture();
        apply_preset(&mut editor, &[id], Preset::SlideUp, 0, 1000).unwrap();
        let before = editor.doc.clone();
        retime(&mut editor, &[id], 2., 100, Some(4000)).unwrap();
        assert_eq!(editor.doc.design.keyframes[&id][0].frames[1].time_ms, 2100);
        editor.undo();
        assert_eq!(editor.doc, before);
        assert!(retime(&mut editor, &[id], 0.01, 0, Some(100)).is_ok());
        editor.undo();
        let revision = editor.revision;
        assert!(retime(&mut editor, &[id], 100., 0, None).is_err());
        assert_eq!(editor.doc, before);
        assert_eq!(editor.revision, revision);
        assert!(apply_preset(&mut editor, &[id], Preset::Spin, 0, 1000).is_err());
        assert_eq!(editor.doc, before);
    }
}
