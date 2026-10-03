//! Conform a storyboard to an edit made in editing software: match the
//! edit's picture clips to panels by name (or by the panel ID in the media
//! file names editorial export writes), then take the edit's durations,
//! order, transitions and the sound clips that play the board's own sounds.
//! Panels the edit reorders join the scene they land in; everything else
//! keeps its scene. Planning never changes the board, so the same plan is
//! the dry-run report and, applied, one Undo step
//! (`ProjectEditor::conform_storyboard`).
use crate::project::PageId;
use crate::storyboard::{MAX_PANEL_FRAMES, Storyboard};
use crate::timeline::audio::{AssetId, AudioClip, AudioTrack, MAX_TRACKS};
use crate::timeline::{Edit, EditClip, FrameRate, Transition, TransitionKind};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// A file name stem for a panel's media: its name made safe for editing
/// software, then `_p` and the panel ID, so a renamed clip still finds its
/// panel.
pub fn panel_media_stem(name: &str, id: PageId) -> String {
    format!("{}_p{id}", safe_stem(name))
}

/// A file name stem for a sound: its name made safe, then `_s` and the
/// sound ID.
pub fn sound_media_stem(name: &str, id: AssetId) -> String {
    format!("{}_s{id}", safe_stem(name))
}

/// Letters, digits, `-` and `_` only (others become `_`), at most 60.
pub fn safe_stem(name: &str) -> String {
    let stem: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(60)
        .collect();
    if stem.is_empty() { "clip".into() } else { stem }
}

/// The ID after `_{tag}` at the end of a media stem.
fn stem_id(stem: &str, tag: char) -> Option<u64> {
    let (_, id) = stem.rsplit_once(&format!("_{tag}"))?;
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
        .then(|| id.parse().ok())
        .flatten()
}

/// What to do when the edit's frame rate differs from the board's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateChoice {
    /// Keep times: convert the edit's frames to the board's rate.
    #[default]
    Convert,
    /// Keep frame counts as they are.
    Keep,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Retimed {
    pub panel: PageId,
    pub name: String,
    pub from: u32,
    pub to: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PanelRef {
    pub panel: PageId,
    pub name: String,
}

/// What a conform matched and changes.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ConformReport {
    /// Panels matched to a clip of the edit.
    pub matched: usize,
    pub retimed: Vec<Retimed>,
    /// Panels the edit puts in a new place.
    pub moved: Vec<PanelRef>,
    /// Picture clips no panel matched, as "name at timecode".
    pub unmatched: Vec<String>,
    /// Playing panels the edit leaves out; they keep their place and
    /// duration.
    pub left_out: Vec<PanelRef>,
    /// Panels whose transition changes.
    pub transitions: usize,
    /// Sound clips placed from the edit.
    pub sound_clips: usize,
    /// Sound clips that play no sound of the board.
    pub unmatched_sounds: Vec<String>,
    pub warnings: Vec<String>,
    pub edit_rate: String,
    pub board_rate: String,
    /// Whether the rates differ, so the rate choice matters.
    pub rate_differs: bool,
}

impl ConformReport {
    /// Whether applying would change nothing.
    pub fn is_noop(&self) -> bool {
        self.retimed.is_empty()
            && self.moved.is_empty()
            && self.transitions == 0
            && self.sound_clips == 0
    }

    /// A few lines for a person: counts first, then names.
    pub fn summary(&self) -> String {
        let plural =
            |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        let mut lines = vec![format!(
            "{} matched · {} retimed · {} moved · {} · {}",
            plural(self.matched, "panel", "panels"),
            self.retimed.len(),
            self.moved.len(),
            plural(
                self.transitions,
                "transition changed",
                "transitions changed"
            ),
            plural(self.sound_clips, "sound clip", "sound clips"),
        )];
        if self.rate_differs {
            lines.push(format!(
                "The edit runs at {}; the board at {}.",
                self.edit_rate, self.board_rate
            ));
        }
        let list = |items: Vec<String>| {
            let shown: Vec<_> = items.iter().take(8).cloned().collect();
            let more = items.len().saturating_sub(shown.len());
            let mut text = shown.join(", ");
            if more > 0 {
                text.push_str(&format!(" and {more} more"));
            }
            text
        };
        if !self.unmatched.is_empty() {
            lines.push(format!(
                "Clips with no panel: {}",
                list(self.unmatched.clone())
            ));
        }
        if !self.left_out.is_empty() {
            lines.push(format!(
                "Panels not in the edit (kept): {}",
                list(self.left_out.iter().map(|p| p.name.clone()).collect())
            ));
        }
        if !self.unmatched_sounds.is_empty() {
            lines.push(format!(
                "Sound clips with no sound in the board: {}",
                list(self.unmatched_sounds.clone())
            ));
        }
        lines.extend(self.warnings.iter().cloned());
        lines.join("\n")
    }
}

/// The board after a conform, its page order and the report.
#[derive(Clone, Debug)]
pub struct ConformPlan {
    pub order: Vec<PageId>,
    /// Durations, transitions and sound applied, keyframes kept in sync,
    /// in the old order (scenes unchanged).
    pub timed: Storyboard,
    /// `timed` with moved panels in the scenes they land in, for `order`.
    pub board: Storyboard,
    pub report: ConformReport,
}

/// Edit frames to board frames.
struct Rates {
    edit: FrameRate,
    board: FrameRate,
    convert: bool,
}

impl Rates {
    fn frame(&self, f: u64) -> u64 {
        if self.convert {
            self.board.seconds_to_frames(self.edit.frames_to_seconds(f))
        } else {
            f
        }
    }
    fn ms(&self, f: u64) -> u64 {
        (self.edit.frames_to_seconds(f) * 1000.).round() as u64
    }
}

/// Plan conforming `board` (pages `layout`, each with its name) to `edit`.
pub fn plan(
    board: &Storyboard,
    layout: &[(PageId, String)],
    edit: &Edit,
    choice: RateChoice,
) -> Result<ConformPlan, String> {
    edit.validate()?;
    let order: Vec<PageId> = layout.iter().map(|(id, _)| *id).collect();
    board.validate(&order)?;
    let names: HashMap<PageId, &str> = layout.iter().map(|(id, n)| (*id, n.as_str())).collect();
    let rates = Rates {
        edit: edit.rate,
        board: board.settings.frame_rate,
        convert: choice == RateChoice::Convert && edit.rate != board.settings.frame_rate,
    };
    let mut report = ConformReport {
        edit_rate: edit.rate.label(),
        board_rate: board.settings.frame_rate.label(),
        rate_differs: edit.rate != board.settings.frame_rate,
        ..ConformReport::default()
    };
    let timecode = |f: u64| edit.rate.timecode(edit.start + f);

    // Match picture clips to playing panels, in record order.
    let playing: Vec<PageId> = board.playing(&order).iter().map(|(id, _)| *id).collect();
    let mut by_name: HashMap<String, Vec<PageId>> = HashMap::new();
    for id in playing.iter().rev() {
        by_name
            .entry(names[id].trim().to_lowercase())
            .or_default()
            .push(*id);
    }
    let mut clips: Vec<&EditClip> = edit.video.iter().collect();
    clips.sort_by_key(|c| (c.record_in, c.track));
    let mut used = HashSet::new();
    let mut matched: Vec<(PageId, &EditClip)> = Vec::new();
    let mut unmatched_by_track: Vec<(usize, String)> = Vec::new();
    let mut tracks_with_panels = HashSet::new();
    for clip in clips {
        let by_stem = clip
            .media_stem()
            .and_then(|s| stem_id(s, 'p'))
            .filter(|id| playing.contains(id));
        let found = by_name
            .get_mut(&clip.name.trim().to_lowercase())
            .and_then(|ids| {
                while let Some(id) = ids.pop() {
                    if !used.contains(&id) {
                        return Some(id);
                    }
                }
                None
            })
            .or(by_stem.filter(|id| !used.contains(id)));
        match found {
            Some(id) => {
                used.insert(id);
                tracks_with_panels.insert(clip.track);
                matched.push((id, clip));
            }
            None if by_stem.is_some() || names_used(&used, &names, &clip.name) => {
                tracks_with_panels.insert(clip.track);
                report.warnings.push(format!(
                    "“{}” at {} uses a panel again; only its first use is applied.",
                    clip.name,
                    timecode(clip.record_in)
                ));
            }
            None => unmatched_by_track.push((
                clip.track,
                format!("{} at {}", clip.name, timecode(clip.record_in)),
            )),
        }
    }
    // Clips on tracks without panels (reference pictures) are not missing
    // panels.
    report.unmatched = unmatched_by_track
        .into_iter()
        .filter(|(track, _)| tracks_with_panels.contains(track))
        .map(|(_, text)| text)
        .collect();
    if matched.is_empty() {
        return Err("No clip in the edit matches a panel by name or media file.".into());
    }
    report.matched = matched.len();
    report.left_out = playing
        .iter()
        .filter(|id| !used.contains(*id))
        .map(|id| PanelRef {
            panel: *id,
            name: names[id].into(),
        })
        .collect();
    if !report.left_out.is_empty() {
        report.warnings.push(
            "Panels not in the edit keep their place and duration, so later sound may not line up."
                .into(),
        );
    }

    // Durations: each matched clip runs to the next matched clip, so gaps
    // join the panel before them.
    let base = matched[0].1.record_in;
    if base > 0 {
        report.warnings.push(format!(
            "The edit starts {} frames before its first panel; the board starts at that panel.",
            base
        ));
    }
    let mut timed = board.clone();
    for (i, (id, clip)) in matched.iter().enumerate() {
        let end = match matched.get(i + 1) {
            Some((_, next)) if next.record_in > clip.record_out => {
                report.warnings.push(format!(
                    "A gap after “{}” at {} is added to its duration.",
                    clip.name,
                    timecode(clip.record_out)
                ));
                next.record_in
            }
            Some((_, next)) => next.record_in.min(clip.record_out).max(clip.record_in + 1),
            None => clip.record_out,
        };
        let frames = rates.frame(end - base) - rates.frame(clip.record_in - base);
        let frames = frames.clamp(1, u64::from(MAX_PANEL_FRAMES)) as u32;
        let locked = board.is_locked(*id);
        let panel = timed.panels.get_mut(id).unwrap();
        if panel.frames != frames {
            if locked {
                report.warnings.push(format!(
                    "{} is locked, so it keeps {} frames.",
                    names[id], panel.frames
                ));
                continue;
            }
            report.retimed.push(Retimed {
                panel: *id,
                name: names[id].into(),
                from: panel.frames,
                to: frames,
            });
            panel.frames = frames;
        }
        // Transitions: the first matched clip has nothing to enter from.
        let next = if i == 0 {
            Transition::default()
        } else {
            match clip.transition {
                None => Transition::default(),
                Some(t) => {
                    let kind = if t.kind == TransitionKind::Dissolve && !panel.transition.is_cut()
                        || same_kind(t.kind, panel.transition.kind)
                    {
                        // A dissolve stands in for kinds a format cannot
                        // name; keep the board's own.
                        panel.transition.kind
                    } else {
                        t.kind
                    };
                    let frames = (rates.frame(u64::from(t.frames)) as u32).min(panel.frames);
                    Transition { kind, frames }
                }
            }
        };
        let next = if next.is_cut() {
            Transition::default()
        } else {
            next
        };
        let current = if panel.transition.is_cut() {
            Transition::default()
        } else {
            panel.transition
        };
        if next != current {
            if locked {
                report
                    .warnings
                    .push(format!("{} is locked, so its transition stays.", names[id]));
            } else {
                panel.transition = next;
                report.transitions += 1;
            }
        }
    }
    conform_audio(&mut timed, edit, &rates, base, &mut report);
    timed.sync_keyframes(board, &order);

    // Order: panels outside the longest run kept in order have moved.
    let edit_order: Vec<PageId> = matched.iter().map(|(id, _)| *id).collect();
    let position: HashMap<PageId, usize> =
        order.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let kept = longest_increasing(&edit_order.iter().map(|id| position[id]).collect::<Vec<_>>());
    let moved: HashSet<PageId> = edit_order
        .iter()
        .enumerate()
        .filter(|(i, _)| !kept.contains(i))
        .map(|(_, id)| *id)
        .collect();
    if moved.is_empty() {
        return Ok(ConformPlan {
            order,
            board: timed.clone(),
            timed,
            report,
        });
    }
    if let Some(id) = moved.iter().find(|id| board.is_locked(**id)) {
        return Err(format!(
            "The edit moves {}, which is locked. Unlock it to conform.",
            names[id]
        ));
    }
    // Panels outside the edit (and thumbnail sheets) travel with the
    // matched panel after them; those after the last stay at the end.
    let in_edit: HashSet<PageId> = edit_order.iter().copied().collect();
    let mut runs: HashMap<PageId, Vec<PageId>> = HashMap::new();
    let mut pending = Vec::new();
    for id in &order {
        if in_edit.contains(id) {
            runs.insert(*id, std::mem::take(&mut pending));
        } else {
            pending.push(*id);
        }
    }
    let mut new_order = Vec::with_capacity(order.len());
    let mut next = timed.clone();
    for id in &edit_order {
        let scene = timed.panels[id].scene;
        let landing = if moved.contains(id) {
            // The scene before it, or after it at the very start.
            new_order
                .last()
                .map(|p| next.panels[p].scene)
                .or_else(|| {
                    edit_order
                        .iter()
                        .find(|p| !moved.contains(*p))
                        .map(|p| next.panels[p].scene)
                })
                .unwrap_or(scene)
        } else {
            scene
        };
        if moved.contains(id) {
            report.moved.push(PanelRef {
                panel: *id,
                name: names[id].into(),
            });
        }
        for follower in runs.remove(id).unwrap_or_default() {
            if timed.panels[&follower].scene == scene {
                next.panels.get_mut(&follower).unwrap().scene = landing;
            }
            new_order.push(follower);
        }
        next.panels.get_mut(id).unwrap().scene = landing;
        new_order.push(*id);
    }
    new_order.append(&mut pending);
    next.reconcile(&new_order);
    next.validate(&new_order)?;
    Ok(ConformPlan {
        order: new_order,
        timed,
        board: next,
        report,
    })
}

fn names_used(used: &HashSet<PageId>, names: &HashMap<PageId, &str>, name: &str) -> bool {
    used.iter()
        .any(|id| names[id].trim().eq_ignore_ascii_case(name.trim()))
}

fn same_kind(a: TransitionKind, b: TransitionKind) -> bool {
    std::mem::discriminant(&a) == std::mem::discriminant(&b)
}

/// Indices of a longest strictly increasing subsequence of `values`.
fn longest_increasing(values: &[usize]) -> HashSet<usize> {
    let n = values.len();
    let mut length = vec![1usize; n];
    let mut previous = vec![usize::MAX; n];
    for i in 0..n {
        for j in 0..i {
            if values[j] < values[i] && length[j] + 1 > length[i] {
                length[i] = length[j] + 1;
                previous[i] = j;
            }
        }
    }
    let mut out = HashSet::new();
    let Some(mut i) = (0..n).max_by_key(|&i| (length[i], std::cmp::Reverse(i))) else {
        return out;
    };
    loop {
        out.insert(i);
        if previous[i] == usize::MAX {
            break;
        }
        i = previous[i];
    }
    out
}

/// Replace the board's sound clips with the edit's clips that play the
/// board's sounds, keeping each reused clip's fades and effects. An edit
/// without sound leaves the board's sound alone.
fn conform_audio(
    board: &mut Storyboard,
    edit: &Edit,
    rates: &Rates,
    base: u64,
    report: &mut ConformReport,
) {
    if edit.audio.is_empty() {
        return;
    }
    let timeline = &board.timeline;
    let sound = |clip: &EditClip| -> Option<AssetId> {
        if let Some(id) = clip
            .media_stem()
            .and_then(|s| stem_id(s, 's'))
            .filter(|id| timeline.assets.contains_key(id))
        {
            return Some(id);
        }
        let named = |n: &str| {
            timeline
                .assets
                .iter()
                .find(|(_, a)| a.name.trim().eq_ignore_ascii_case(n.trim()))
                .map(|(id, _)| *id)
        };
        named(&clip.name).or_else(|| clip.media_stem().and_then(named))
    };
    let resolved: Vec<(&EditClip, Option<AssetId>)> =
        edit.audio.iter().map(|c| (c, sound(c))).collect();
    if resolved.iter().all(|(_, s)| s.is_none()) {
        report.unmatched_sounds = edit.audio.iter().map(|c| c.name.clone()).collect();
        report.warnings.push(
            "No sound clip in the edit plays a sound of this board; sound is unchanged.".into(),
        );
        return;
    }
    // Existing clips, to reuse their fades, envelope and EQ.
    let mut old: Vec<(usize, AudioClip)> = timeline
        .tracks
        .iter()
        .enumerate()
        .flat_map(|(t, track)| track.clips.iter().map(move |c| (t, c.clone())))
        .collect();
    let mut tracks: Vec<AudioTrack> = timeline
        .tracks
        .iter()
        .map(|t| AudioTrack {
            clips: Vec::new(),
            ..t.clone()
        })
        .collect();
    let mut placed = 0;
    for (clip, asset) in resolved {
        let Some(asset) = asset else {
            report.unmatched_sounds.push(clip.name.clone());
            continue;
        };
        if clip.track >= MAX_TRACKS {
            report.warnings.push(format!(
                "“{}” is on audio track {}; the board holds {MAX_TRACKS}.",
                clip.name,
                clip.track + 1
            ));
            continue;
        }
        if clip.record_out <= base {
            report.warnings.push(format!(
                "“{}” plays before the first panel and is left out.",
                clip.name
            ));
            continue;
        }
        let duration = board.timeline.assets[&asset].duration_ms;
        let offset_ms = rates.ms(clip.source_in);
        if offset_ms >= duration.max(1) {
            report.warnings.push(format!(
                "“{}” starts after its sound ends and is left out.",
                clip.name
            ));
            continue;
        }
        let start_edit = clip.record_in.max(base);
        let start = rates.frame(start_edit - base);
        let frames = (rates.frame(clip.record_out - base) - start).max(1);
        let reuse = old
            .iter()
            .position(|(t, c)| c.asset == asset && *t == clip.track && c.offset_ms == offset_ms)
            .or_else(|| old.iter().position(|(_, c)| c.asset == asset));
        let mut next = match reuse {
            Some(i) => old.remove(i).1,
            None => AudioClip {
                asset,
                name: clip.name.trim().chars().take(200).collect(),
                ..AudioClip::default()
            },
        };
        if next.name.trim().is_empty() {
            next.name = board.timeline.assets[&asset].name.clone();
        }
        next.start = start;
        next.frames = frames;
        next.offset_ms = offset_ms;
        if let Some(gain) = clip.gain_db {
            next.gain_db = gain.clamp(
                crate::timeline::audio::MIN_GAIN_DB,
                crate::timeline::audio::MAX_GAIN_DB,
            );
        }
        if next.fade_in + next.fade_out > frames {
            next.fade_in = 0;
            next.fade_out = 0;
        }
        next.envelope.retain(|k| k.frame < frames);
        while tracks.len() <= clip.track {
            tracks.push(AudioTrack::new(&format!("Audio {}", tracks.len() + 1)));
        }
        let track = &mut tracks[clip.track];
        if track
            .clips
            .iter()
            .any(|c| next.start < c.end() && c.start < next.end())
        {
            report.warnings.push(format!(
                "“{}” overlaps another clip on audio track {} and is left out.",
                clip.name,
                clip.track + 1
            ));
            continue;
        }
        track.clips.push(next);
        track.clips.sort_by_key(|c| c.start);
        placed += 1;
    }
    if !old.is_empty() {
        report.warnings.push(format!(
            "{} sound clip{} not in the edit {} removed.",
            old.len(),
            if old.len() == 1 { "" } else { "s" },
            if old.len() == 1 { "is" } else { "are" }
        ));
    }
    report.sound_clips = placed;
    let mut next = board.timeline.clone();
    next.tracks = tracks;
    if next.validate().is_ok() {
        board.timeline = next;
    } else {
        report.sound_clips = 0;
        report
            .warnings
            .push("The edit's sound does not fit the board's tracks; sound is unchanged.".into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems_carry_ids() {
        assert_eq!(panel_media_stem("Panel 1/a", 7), "Panel_1_a_p7");
        assert_eq!(stem_id("Panel_1_a_p7", 'p'), Some(7));
        assert_eq!(stem_id("Rain_s12", 's'), Some(12));
        assert_eq!(stem_id("Rain_s12", 'p'), None);
        assert_eq!(stem_id("Panel_p", 'p'), None);
        assert_eq!(safe_stem("  "), "clip");
    }

    #[test]
    fn longest_run_in_order() {
        let kept = longest_increasing(&[0, 2, 1, 3]);
        assert_eq!(kept.len(), 3);
        assert!(kept.contains(&0) && kept.contains(&3));
        assert!(longest_increasing(&[]).is_empty());
        assert_eq!(longest_increasing(&[3, 2, 1]).len(), 1);
    }
}
