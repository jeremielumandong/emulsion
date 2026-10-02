//! Scratch dialogue (AI8, AI9): a voice cast per board — each character
//! from the Dialogue captions mapped to a local text-to-speech voice with a
//! rate and pitch — and the scratch lines generated from it. Synthesis runs
//! outside the core (`emulsion_io::voices`); this holds the data, reads the
//! dialogue out of captions, and lays generated takes on the timeline.
//!
//! A scratch take is an ordinary sound in the library whose ID is listed in
//! [`VoiceCast::lines`]. Generating again for a panel removes only the clips
//! of those listed sounds, never the user's own recordings or imports.
use crate::project::PageId;
use crate::storyboard::{CaptionId, MAX_CAPTION_CHARS, MAX_PANEL_FRAMES, Storyboard};
use crate::timeline::audio::{AssetId, MAX_ASSETS, MAX_TRACKS};
use crate::timeline::{AudioAsset, AudioClip, AudioTrack, Timeline};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// The library folder scratch takes go into.
pub const SCRATCH_FOLDER: &str = "Scratch dialogue";
/// The track scratch takes are placed on ("Scratch dialogue 2"… when a
/// take does not fit there).
pub const SCRATCH_TRACK: &str = "Scratch dialogue";
/// The track enhanced clips go on when the original clip stays.
pub const ENHANCED_TRACK: &str = "Enhanced dialogue";
/// Most characters one cast holds.
pub const MAX_VOICES: usize = 256;
/// Speaking rate, as a multiple of the engine's normal speed.
pub const RATE_RANGE: std::ops::RangeInclusive<f32> = 0.5..=2.0;
/// Highest pitch (eSpeak NG's 0–99 scale; 50 is the voice's own).
pub const MAX_PITCH: u8 = 99;
/// The pitch a voice speaks at unless changed.
pub const NORMAL_PITCH: u8 = 50;

/// Which text-to-speech engine voices come from (a preference).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineChoice {
    /// Piper when it is installed and has a voice, otherwise eSpeak NG.
    #[default]
    Auto,
    Piper,
    Espeak,
}

impl EngineChoice {
    pub fn is_auto(&self) -> bool {
        *self == Self::Auto
    }
}

/// A voice of one engine.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "engine", rename_all = "snake_case")]
pub enum VoiceEngine {
    /// An eSpeak NG voice such as `en-us` or `en-gb+f2` (language and
    /// variant).
    Espeak { voice: String },
    /// A Piper voice model (`.onnx`): a file name in the Piper voices
    /// folder, or a full path; `speaker` picks one voice of a multi-speaker
    /// model.
    Piper {
        model: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        speaker: Option<u32>,
    },
}

impl VoiceEngine {
    /// "eSpeak NG en-us+f2" or "Piper en_US-amy-medium (speaker 3)".
    pub fn label(&self) -> String {
        match self {
            Self::Espeak { voice } => format!("eSpeak NG {voice}"),
            Self::Piper { model, speaker } => {
                let name = std::path::Path::new(model)
                    .file_stem()
                    .map_or_else(|| model.clone(), |s| s.to_string_lossy().into_owned());
                match speaker {
                    Some(s) => format!("Piper {name} (speaker {s})"),
                    None => format!("Piper {name}"),
                }
            }
        }
    }
}

fn one() -> f32 {
    1.
}
fn normal_pitch() -> u8 {
    NORMAL_PITCH
}
fn is_one(v: &f32) -> bool {
    *v == 1.
}
fn is_normal_pitch(v: &u8) -> bool {
    *v == NORMAL_PITCH
}

/// A character's voice.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Voice {
    #[serde(flatten)]
    pub engine: VoiceEngine,
    /// Multiple of normal speed, within [`RATE_RANGE`].
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub rate: f32,
    /// 0–99; 50 is the voice's own pitch.
    #[serde(default = "normal_pitch", skip_serializing_if = "is_normal_pitch")]
    pub pitch: u8,
}

impl Voice {
    pub fn new(engine: VoiceEngine) -> Self {
        Self {
            engine,
            rate: 1.,
            pitch: NORMAL_PITCH,
        }
    }
}

/// How strongly a line is stressed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Emphasis {
    #[default]
    None,
    Moderate,
    Strong,
}

/// How one line is delivered, on top of its character's voice: the
/// intonation controls of Regenerate line.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Delivery {
    /// Multiplies the voice's rate (the result stays in [`RATE_RANGE`]).
    pub rate: f32,
    /// Added to the voice's pitch (the result stays 0–99).
    pub pitch: i8,
    /// Stress (eSpeak NG reads it as SSML emphasis; Piper speaks a
    /// little slower and livelier).
    pub emphasis: Emphasis,
    /// Piper's expressiveness (its noise scale), 0–1.
    pub variation: f32,
}

/// Piper's own default noise scale.
pub const DEFAULT_VARIATION: f32 = 0.667;

impl Default for Delivery {
    fn default() -> Self {
        Self {
            rate: 1.,
            pitch: 0,
            emphasis: Emphasis::None,
            variation: DEFAULT_VARIATION,
        }
    }
}

impl Delivery {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
    pub fn validate(&self) -> Result<(), String> {
        if !self.rate.is_finite() || !RATE_RANGE.contains(&self.rate) {
            return Err("A line's rate is 0.5–2 times its voice's.".into());
        }
        if !(-50..=50).contains(&self.pitch) {
            return Err("A line's pitch changes by −50 to +50.".into());
        }
        if !self.variation.is_finite() || !(0.0..=1.0).contains(&self.variation) {
            return Err("A line's variation is 0–1.".into());
        }
        Ok(())
    }
    /// The rate and pitch a line is spoken at with `voice`.
    pub fn applied(&self, voice: &Voice) -> (f32, u8) {
        let rate = (voice.rate * self.rate).clamp(*RATE_RANGE.start(), *RATE_RANGE.end());
        let pitch = (i32::from(voice.pitch) + i32::from(self.pitch)).clamp(0, MAX_PITCH.into());
        (rate, pitch as u8)
    }
}

/// What a scratch take says, kept so it can be regenerated.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScratchLine {
    pub panel: PageId,
    /// Its order among the panel's lines, from 0.
    pub line: u32,
    pub character: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Delivery::is_default")]
    pub delivery: Delivery,
}

/// The board's voice cast and its scratch takes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VoiceCast {
    /// Voices by character key (see [`character_key`]).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub voices: BTreeMap<String, Voice>,
    /// Sounds that are scratch takes, by sound ID.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub lines: BTreeMap<AssetId, ScratchLine>,
}

fn check_text(text: &str, max: usize, what: &str) -> Result<(), String> {
    if text.trim().is_empty() || text.chars().count() > max || text.chars().any(char::is_control) {
        return Err(format!("{what} must be 1–{max} characters."));
    }
    Ok(())
}

impl VoiceCast {
    pub fn is_empty(&self) -> bool {
        self.voices.is_empty() && self.lines.is_empty()
    }

    /// The voice cast for `character`, if any.
    pub fn voice(&self, character: &str) -> Option<&Voice> {
        self.voices.get(&character_key(character))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.voices.len() > MAX_VOICES {
            return Err(format!(
                "A voice cast holds at most {MAX_VOICES} characters."
            ));
        }
        for (name, voice) in &self.voices {
            check_text(name, 200, "Character names")?;
            if *name != character_key(name) {
                return Err("Cast character names are kept in capitals.".into());
            }
            match &voice.engine {
                VoiceEngine::Espeak { voice } => {
                    if voice.is_empty()
                        || voice.len() > 100
                        || voice.starts_with('-')
                        || !voice
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || "-_+./".contains(c))
                    {
                        return Err(format!("“{voice}” is not an eSpeak NG voice name."));
                    }
                }
                VoiceEngine::Piper { model, speaker } => {
                    check_text(model, 4096, "Piper model paths")?;
                    if speaker.is_some_and(|s| s > 100_000) {
                        return Err("Piper speaker numbers are 0–100000.".into());
                    }
                }
            }
            if !voice.rate.is_finite() || !RATE_RANGE.contains(&voice.rate) {
                return Err("Voice rates are 0.5–2 times normal speed.".into());
            }
            if voice.pitch > MAX_PITCH {
                return Err(format!("Voice pitches are 0–{MAX_PITCH}."));
            }
        }
        if self.lines.len() > MAX_ASSETS {
            return Err(format!("A board holds at most {MAX_ASSETS} scratch lines."));
        }
        for line in self.lines.values() {
            check_text(&line.text, MAX_CAPTION_CHARS, "Scratch lines")?;
            if line.character.chars().count() > 200 || line.character.chars().any(char::is_control)
            {
                return Err("Character names must be 1–200 characters.".into());
            }
            line.delivery.validate()?;
        }
        Ok(())
    }
}

/// One spoken line read from a Dialogue caption.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogueLine {
    /// As written, without parentheticals ("MIA"); empty when no cue says
    /// who speaks.
    pub character: String,
    /// A delivery note such as "(quietly)".
    pub parenthetical: Option<String>,
    /// The words to speak.
    pub text: String,
}

/// Screenplay extensions that belong to the cue, not the delivery.
const EXTENSIONS: &[&str] = &[
    "O.S.", "V.O.", "O.C.", "CONT'D", "CONT’D", "CONTD", "PRE-LAP",
];

/// Split "MIA (V.O.) (quietly)" into the name and a delivery note.
fn split_cue(cue: &str) -> (String, Option<String>) {
    let mut name = String::new();
    let mut notes = Vec::new();
    let mut rest = cue;
    while let Some(open) = rest.find('(') {
        name.push_str(&rest[..open]);
        let Some(close) = rest[open..].find(')') else {
            rest = &rest[open..];
            break;
        };
        let inner = rest[open + 1..open + close].trim();
        if !EXTENSIONS.iter().any(|e| inner.eq_ignore_ascii_case(e)) && !inner.is_empty() {
            notes.push(format!("({inner})"));
        }
        rest = &rest[open + close + 1..];
    }
    name.push_str(rest);
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    (name, (!notes.is_empty()).then(|| notes.join(" ")))
}

/// The key a character is cast under: the name without parentheticals,
/// spaces collapsed, in capitals ("Mia (V.O.)" → "MIA").
pub fn character_key(name: &str) -> String {
    split_cue(name).0.to_uppercase()
}

/// The words of a line as they are spoken: parenthetical directions and
/// Fountain emphasis markers removed, spaces collapsed.
pub fn spoken_text(text: &str) -> String {
    let (bare, _) = crate::storyboard_estimate::strip_parentheticals(text);
    bare.split_whitespace()
        .map(|w| w.replace(['*', '_'], ""))
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The spoken lines of a Dialogue caption, in order. Script import writes
/// one line per dialogue block, "MIA (quietly): Is anyone there?"; a line
/// without a cue continues the line before it (or is spoken by nobody in
/// particular when it comes first). Cues are recognised exactly as the
/// duration estimate counts them. Lines with nothing to say are skipped.
pub fn dialogue_lines(caption: &str) -> Vec<DialogueLine> {
    let mut out: Vec<DialogueLine> = Vec::new();
    for raw in caption.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let cue = crate::storyboard_estimate::speaker(line)
            .map(|(cue, text)| (split_cue(cue), text.trim()));
        match cue {
            Some(((character, parenthetical), text)) => out.push(DialogueLine {
                character,
                parenthetical,
                text: text.to_string(),
            }),
            None => match out.last_mut() {
                Some(last) => {
                    if !last.text.is_empty() {
                        last.text.push(' ');
                    }
                    last.text.push_str(line);
                }
                None => out.push(DialogueLine {
                    character: String::new(),
                    parenthetical: None,
                    text: line.to_string(),
                }),
            },
        }
    }
    out.retain(|l| !spoken_text(&l.text).is_empty());
    out
}

/// A line to synthesize, from [`Storyboard::scratch_plan`].
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedLine {
    pub panel: PageId,
    pub line: u32,
    pub character: String,
    /// The words to speak (see [`spoken_text`]).
    pub text: String,
    /// The cast voice; `None` asks the engine for a default voice.
    pub voice: Option<Voice>,
    /// The character's place among the board's characters, so uncast
    /// characters get different default voices.
    pub cast_index: usize,
    pub delivery: Delivery,
}

/// What generating scratch dialogue did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScratchReport {
    /// Takes placed.
    pub placed: usize,
    /// Earlier scratch clips removed.
    pub removed: usize,
    /// Panels made longer to fit their lines.
    pub extended: Vec<PageId>,
    /// Locked panels whose lines run past their end.
    pub locked: Vec<PageId>,
}

/// A clip or sound name for a line: "MIA: Is anyone there?".
pub fn line_name(character: &str, text: &str) -> String {
    let text: String = if text.chars().count() > 80 {
        text.chars().take(79).chain(['…']).collect()
    } else {
        text.into()
    };
    let name = if character.is_empty() {
        text
    } else {
        format!("{character}: {text}")
    };
    let name: String = name
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(200)
        .collect();
    if name.trim().is_empty() {
        "Scratch line".into()
    } else {
        name
    }
}

/// The first track named `base` (or "`base` 2"…) with room for
/// `start..end`, adding one when none has room.
fn track_with_room(
    timeline: &mut Timeline,
    base: &str,
    start: u64,
    end: u64,
) -> Result<usize, String> {
    let named = |t: &AudioTrack| {
        t.name == base
            || t.name
                .strip_prefix(base)
                .and_then(|n| n.strip_prefix(' '))
                .is_some_and(|n| n.parse::<u32>().is_ok())
    };
    if let Some(i) = timeline
        .tracks
        .iter()
        .position(|t| named(t) && t.has_room(start, end))
    {
        return Ok(i);
    }
    if timeline.tracks.len() >= MAX_TRACKS {
        return Err(format!(
            "There is no room for another audio track (at most {MAX_TRACKS})."
        ));
    }
    let mut n = 1;
    let name = loop {
        let name = if n == 1 {
            base.to_string()
        } else {
            format!("{base} {n}")
        };
        if !timeline.tracks.iter().any(|t| t.name == name) {
            break name;
        }
        n += 1;
    };
    timeline.tracks.push(AudioTrack::new(&name));
    Ok(timeline.tracks.len() - 1)
}

/// Clip length in frames for `ms` of sound.
fn frames_for(board: &Storyboard, ms: u64) -> u64 {
    board
        .settings
        .frame_rate
        .seconds_to_frames(ms as f64 / 1000.)
        .max(1)
}

impl Storyboard {
    /// The caption field dialogue is written in ("Dialogue").
    pub fn dialogue_field(&self) -> Option<CaptionId> {
        self.caption("Dialogue")
    }

    /// The spoken lines of a panel's Dialogue caption.
    pub fn panel_dialogue(&self, panel: PageId) -> Vec<DialogueLine> {
        let Some(field) = self.dialogue_field() else {
            return Vec::new();
        };
        self.panels
            .get(&panel)
            .and_then(|p| p.captions.get(&field))
            .map_or_else(Vec::new, |c| dialogue_lines(&c.text))
    }

    /// Every character who speaks on the board, as first written, in board
    /// order, followed by cast characters no caption names.
    pub fn characters(&self, layout: &[PageId]) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for id in layout {
            for line in self.panel_dialogue(*id) {
                if !line.character.is_empty() && seen.insert(character_key(&line.character)) {
                    out.push(line.character);
                }
            }
        }
        for key in self.voices.voices.keys() {
            if seen.insert(key.clone()) {
                out.push(key.clone());
            }
        }
        out
    }

    /// The lines of the playing panels in `scope` (in board order) to
    /// synthesize, each with its character's cast voice.
    pub fn scratch_plan(&self, layout: &[PageId], scope: &[PageId]) -> Vec<PlannedLine> {
        let characters: Vec<String> = self
            .characters(layout)
            .iter()
            .map(|c| character_key(c))
            .collect();
        let wanted: HashSet<_> = scope.iter().collect();
        let mut out = Vec::new();
        for (panel, _) in self.playing(layout) {
            if !wanted.contains(&panel) {
                continue;
            }
            for (line, said) in self.panel_dialogue(panel).into_iter().enumerate() {
                let key = character_key(&said.character);
                out.push(PlannedLine {
                    panel,
                    line: line as u32,
                    voice: self.voices.voice(&said.character).cloned(),
                    cast_index: characters.iter().position(|c| *c == key).unwrap_or(0),
                    character: said.character,
                    text: spoken_text(&said.text),
                    delivery: Delivery::default(),
                });
            }
        }
        out
    }

    /// Whether sound `asset` is a scratch take.
    pub fn is_scratch(&self, asset: AssetId) -> bool {
        self.voices.lines.contains_key(&asset)
    }

    /// Remove the scratch clips of `panels` and the scratch sounds no clip
    /// uses any more. Returns how many clips went.
    fn remove_scratch(&mut self, panels: &BTreeSet<PageId>) -> usize {
        let lines = &self.voices.lines;
        let doomed = |asset: &AssetId| lines.get(asset).is_some_and(|l| panels.contains(&l.panel));
        let mut removed = 0;
        for track in &mut self.timeline.tracks {
            let before = track.clips.len();
            track.clips.retain(|c| !doomed(&c.asset));
            removed += before - track.clips.len();
        }
        let used: HashSet<AssetId> = self
            .timeline
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.asset))
            .collect();
        let assets = &mut self.timeline.assets;
        assets.retain(|id, _| used.contains(id) || !doomed(id));
        // Records of sounds that are gone (deleted from the library too).
        self.voices.lines.retain(|id, _| assets.contains_key(id));
        removed
    }

    /// Lay generated takes on the timeline as one change: earlier scratch
    /// clips of the `scope` panels are removed, each panel's lines are
    /// placed back to back from its start on the Scratch dialogue track,
    /// and with `extend` unlocked panels grow to fit their lines. Each take
    /// pairs its line with its imported sound.
    pub fn apply_scratch(
        &mut self,
        layout: &[PageId],
        scope: &[PageId],
        takes: Vec<(PlannedLine, AudioAsset)>,
        extend: bool,
    ) -> Result<ScratchReport, String> {
        let scope: BTreeSet<PageId> = scope.iter().copied().collect();
        if let Some(stray) = takes.iter().find(|(l, _)| !scope.contains(&l.panel)) {
            return Err(format!("Panel {} is not being generated.", stray.0.panel));
        }
        let mut report = ScratchReport {
            removed: self.remove_scratch(&scope),
            ..Default::default()
        };
        let mut need: BTreeMap<PageId, u64> = BTreeMap::new();
        for (line, asset) in &takes {
            *need.entry(line.panel).or_default() += frames_for(self, asset.duration_ms);
        }
        for (&id, &frames) in &need {
            let locked = self.is_locked(id);
            let panel = self.panels.get_mut(&id).ok_or("Panel does not exist.")?;
            if frames <= u64::from(panel.frames) {
                continue;
            }
            if locked {
                report.locked.push(id);
            } else if extend {
                panel.frames = frames.min(u64::from(MAX_PANEL_FRAMES)) as u32;
                report.extended.push(id);
            }
        }
        let starts: BTreeMap<PageId, u64> = self.panel_starts(layout).into_iter().collect();
        let mut offset: BTreeMap<PageId, u64> = BTreeMap::new();
        for (line, mut asset) in takes {
            let start = *starts
                .get(&line.panel)
                .ok_or("Scratch lines go on panels that play.")?
                + offset.get(&line.panel).copied().unwrap_or(0);
            let frames = frames_for(self, asset.duration_ms);
            *offset.entry(line.panel).or_default() += frames;
            let name = line_name(&line.character, &line.text);
            asset.name = name.clone();
            asset.folder = SCRATCH_FOLDER.into();
            let id = self.timeline.add_asset(asset)?;
            let track = track_with_room(&mut self.timeline, SCRATCH_TRACK, start, start + frames)?;
            self.timeline.place(
                track,
                AudioClip {
                    asset: id,
                    name,
                    start,
                    frames,
                    ..AudioClip::default()
                },
            )?;
            self.voices.lines.insert(
                id,
                ScratchLine {
                    panel: line.panel,
                    line: line.line,
                    character: line.character,
                    text: line.text,
                    delivery: line.delivery,
                },
            );
            report.placed += 1;
        }
        Ok(report)
    }

    /// Regenerate line: every clip of scratch sound `old` plays the new take
    /// `asset` instead (from its start, as long as it is, up to the next
    /// clip on the track). The earlier take stays in the library. Returns
    /// the new sound's ID.
    pub fn replace_scratch_take(
        &mut self,
        old: AssetId,
        mut asset: AudioAsset,
        delivery: Delivery,
    ) -> Result<AssetId, String> {
        let line = self
            .voices
            .lines
            .get(&old)
            .cloned()
            .ok_or("That sound is not a scratch line.")?;
        let previous = self
            .timeline
            .assets
            .get(&old)
            .ok_or("That sound is no longer in the library.")?;
        asset.name = previous.name.clone();
        asset.folder = previous.folder.clone();
        let frames = frames_for(self, asset.duration_ms);
        let id = self.timeline.add_asset(asset)?;
        for track in &mut self.timeline.tracks {
            let starts: Vec<u64> = track.clips.iter().map(|c| c.start).collect();
            for (i, clip) in track.clips.iter_mut().enumerate() {
                if clip.asset != old {
                    continue;
                }
                let room = starts.get(i + 1).map_or(u64::MAX, |next| next - clip.start);
                clip.asset = id;
                clip.offset_ms = 0;
                clip.frames = frames.min(room).max(1);
                if clip.fade_in + clip.fade_out > clip.frames {
                    clip.fade_in = 0;
                    clip.fade_out = 0;
                }
                clip.envelope.retain(|k| k.frame < clip.frames);
            }
        }
        self.voices
            .lines
            .insert(id, ScratchLine { delivery, ..line });
        Ok(id)
    }

    /// Enhance dialogue: `asset` (the cleaned-up sound of the clip at
    /// `(track, clip)`) joins the library beside the original. The clip
    /// plays it in place, or with `new_track` a copy of the clip playing
    /// it goes on the Enhanced dialogue track and the original clip stays.
    /// Returns where the clip playing it is.
    pub fn place_enhanced(
        &mut self,
        (track, clip): (usize, usize),
        mut asset: AudioAsset,
        new_track: bool,
    ) -> Result<(usize, usize), String> {
        let original = self
            .timeline
            .tracks
            .get(track)
            .and_then(|t| t.clips.get(clip))
            .cloned()
            .ok_or("That clip no longer exists.")?;
        let source = self
            .timeline
            .assets
            .get(&original.asset)
            .ok_or("A clip refers to a missing sound.")?;
        let name: String = format!("{} (enhanced)", source.name)
            .chars()
            .take(200)
            .collect();
        asset.name = name;
        asset.folder = source.folder.clone();
        let duration = asset.duration_ms.max(1);
        let id = self.timeline.add_asset(asset)?;
        if let Some(line) = self.voices.lines.get(&original.asset).cloned() {
            self.voices.lines.insert(id, line);
        }
        let mut placed = AudioClip {
            asset: id,
            ..original.clone()
        };
        if placed.offset_ms >= duration {
            placed.offset_ms = 0;
        }
        if !new_track {
            self.timeline.tracks[track].clips[clip] = placed;
            return Ok((track, clip));
        }
        placed.name = format!("{} (enhanced)", original.name)
            .chars()
            .take(200)
            .collect();
        let to = track_with_room(
            &mut self.timeline,
            ENHANCED_TRACK,
            placed.start,
            placed.end(),
        )?;
        let start = placed.start;
        self.timeline.place(to, placed)?;
        let index = self.timeline.tracks[to]
            .clips
            .iter()
            .position(|c| c.start == start)
            .unwrap_or(0);
        Ok((to, index))
    }
}
