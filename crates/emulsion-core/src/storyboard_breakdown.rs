//! Storyboards laid out from a breakdown: scenes of panels with captions by
//! field name, shot notes, a camera hint, shot data and a duration. A
//! breakdown is what the assistant writes after reading a script; a script
//! import builds the same panel clip mechanically. Either way the result is
//! a `PanelClip`, so inserting it is the ordinary one-step paste, which maps
//! caption fields by name and adds the missing ones.
use crate::Document;
use crate::project::{ClipPanel, MAX_PAGES, PanelClip};
use crate::storyboard::{
    CameraAngle, CaptionField, MAX_CAPTION_CHARS, MAX_CAPTION_FIELDS, MAX_PANEL_FRAMES, Panel,
    ShotSize, Storyboard,
};
use crate::storyboard_estimate::WordRates;
use serde::{Deserialize, Serialize};

/// The caption field shot notes go to.
pub const NOTES: &str = "Notes";
/// The caption field the camera hint goes to.
pub const CAMERA: &str = "Camera";
/// The caption field holding a scene heading; single-line.
pub const SLUGGING: &str = "Slugging";
/// Most source beats one panel lists.
pub const MAX_SOURCE_BEATS: usize = 200;

/// Builds a panel clip for a board: scenes in order, panels named by the
/// board's naming rules within their scene, captions by field name.
pub struct ClipBuilder<'a> {
    board: &'a Storyboard,
    blank: Document,
    clip: PanelClip,
}

impl<'a> ClipBuilder<'a> {
    pub fn new(board: &'a Storyboard) -> Result<Self, String> {
        Ok(Self {
            board,
            blank: board.blank_panel()?,
            clip: PanelClip {
                frame_rate: board.settings.frame_rate,
                fields: Vec::new(),
                scenes: Vec::new(),
                whole_scenes: true,
                panels: Vec::new(),
            },
        })
    }

    /// Start a scene; an empty name takes the next name from the naming
    /// rules.
    pub fn scene(&mut self, name: &str) {
        let name = name.trim();
        self.clip.scenes.push(if name.is_empty() {
            self.board.naming.scene_name(self.clip.scenes.len())
        } else {
            name.chars().take(200).collect()
        });
    }

    /// Panels so far in the current scene.
    pub fn scene_panels(&self) -> usize {
        let scene = self.clip.scenes.len().saturating_sub(1) as u64;
        self.clip
            .panels
            .iter()
            .filter(|p| p.panel.scene == scene)
            .count()
    }

    fn field(&mut self, name: &str) -> Result<u64, String> {
        if let Some(f) = self
            .clip
            .fields
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case(name))
        {
            return Ok(f.id);
        }
        crate::storyboard::check_name(name, "Caption field")?;
        if self.clip.fields.len() >= MAX_CAPTION_FIELDS {
            return Err(format!("Use at most {MAX_CAPTION_FIELDS} caption fields."));
        }
        let id = self.clip.fields.len() as u64 + 1;
        self.clip.fields.push(CaptionField {
            id,
            name: name.trim().into(),
            multiline: !name.trim().eq_ignore_ascii_case(SLUGGING),
            print: true,
        });
        Ok(id)
    }

    /// Add a panel of `frames` to the current scene with `captions` (field
    /// name, text; empty text is skipped, long text cut to the caption
    /// limit). Returns the panel to set shot data or a transition on.
    pub fn panel(&mut self, frames: u32, captions: &[(&str, &str)]) -> Result<&mut Panel, String> {
        if self.clip.scenes.is_empty() {
            self.scene("");
        }
        if self.clip.panels.len() >= MAX_PAGES {
            return Err(format!(
                "A breakdown makes at most {MAX_PAGES} panels; split it per scene."
            ));
        }
        let scene = self.clip.scenes.len() as u64 - 1;
        let number = self.scene_panels() + 1;
        let mut panel = Panel::new(scene, frames.clamp(1, MAX_PANEL_FRAMES));
        for (name, text) in captions {
            let text: String = text.trim_end().chars().take(MAX_CAPTION_CHARS).collect();
            if text.trim().is_empty() {
                continue;
            }
            let id = self.field(name)?;
            panel.captions.insert(id, text.into());
        }
        self.clip.panels.push(ClipPanel {
            name: self.board.naming.panel_name(number),
            doc: self.blank.clone(),
            panel,
        });
        Ok(&mut self.clip.panels.last_mut().unwrap().panel)
    }

    pub fn finish(self) -> Result<PanelClip, String> {
        if self.clip.panels.is_empty() {
            return Err("The breakdown has no panels.".into());
        }
        Ok(self.clip)
    }
}

/// A storyboard breakdown, as the assistant writes it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Breakdown {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub scenes: Vec<BreakdownScene>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BreakdownScene {
    /// The scene name, usually its heading; empty takes the naming rules.
    #[serde(default)]
    pub name: String,
    pub panels: Vec<BreakdownPanel>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BreakdownPanel {
    /// Caption text by field name (Action, Dialogue, Slugging…).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub captions: std::collections::BTreeMap<String, String>,
    /// Shot notes, added to the Notes caption.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// A camera hint (PUSH IN, PAN LEFT), in the Camera caption.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<String>,
    #[serde(default)]
    pub size: ShotSize,
    #[serde(default)]
    pub angle: CameraAngle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seconds: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frames: Option<u32>,
    /// The script beats this panel shows (IDs from read_storyboard_script).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_beats: Vec<String>,
}

impl Breakdown {
    /// Check the breakdown and lay it out for `board`. Panels without a
    /// duration are timed from their captions with `rates`, or take the
    /// board's default length when they hold no words. Errors name the
    /// scene and panel (1-based).
    pub fn clip(&self, board: &Storyboard, rates: &WordRates) -> Result<PanelClip, String> {
        rates.validate()?;
        if self.scenes.is_empty() {
            return Err("The breakdown has no scenes.".into());
        }
        let mut builder = ClipBuilder::new(board)?;
        for (s, scene) in self.scenes.iter().enumerate() {
            let at = |p: Option<usize>| match p {
                Some(p) => format!("Scene {} panel {}", s + 1, p + 1),
                None => format!("Scene {}", s + 1),
            };
            if scene.name.chars().count() > 200 {
                return Err(format!("{}: names are at most 200 characters.", at(None)));
            }
            if scene.panels.is_empty() {
                return Err(format!("{}: give the scene at least one panel.", at(None)));
            }
            builder.scene(&scene.name);
            for (p, item) in scene.panels.iter().enumerate() {
                let fail = |e: String| format!("{}: {e}", at(Some(p)));
                let frames = item.frames(board, rates).map_err(fail)?;
                let mut captions: Vec<(&str, &str)> = item
                    .captions
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect();
                let notes;
                if let Some(extra) = item.notes.as_deref().filter(|n| !n.trim().is_empty()) {
                    let existing = captions
                        .iter()
                        .position(|(k, _)| k.eq_ignore_ascii_case(NOTES));
                    notes = match existing {
                        Some(i) => format!("{}\n{extra}", captions.remove(i).1),
                        None => extra.to_string(),
                    };
                    captions.push((NOTES, &notes));
                }
                if let Some(camera) = item.camera.as_deref() {
                    if captions.iter().any(|(k, _)| k.eq_ignore_ascii_case(CAMERA)) {
                        return Err(fail(format!(
                            "give the camera hint in `camera` or the {CAMERA} caption, not both."
                        )));
                    }
                    captions.push((CAMERA, camera));
                }
                for (name, text) in &captions {
                    if text.chars().count() > MAX_CAPTION_CHARS {
                        return Err(fail(format!(
                            "the {name} caption is longer than {MAX_CAPTION_CHARS} characters."
                        )));
                    }
                }
                let panel = builder.panel(frames, &captions).map_err(fail)?;
                panel.size = item.size;
                panel.angle = item.angle;
            }
        }
        builder.finish()
    }
}

impl BreakdownPanel {
    fn frames(&self, board: &Storyboard, rates: &WordRates) -> Result<u32, String> {
        let rate = board.settings.frame_rate;
        let frames = match (self.frames, self.seconds) {
            (Some(_), Some(_)) => return Err("give frames or seconds, not both.".into()),
            (Some(f), None) => u64::from(f),
            (None, Some(s)) if s.is_finite() && s > 0. => rate.seconds_to_frames(s),
            (None, Some(_)) => return Err("seconds must be above zero.".into()),
            (None, None) => {
                let mut panel = Panel::new(0, board.settings.panel_frames);
                for (name, text) in &self.captions {
                    if let Some(id) = board.caption(name) {
                        panel.captions.insert(id, text.as_str().into());
                    }
                }
                return Ok(rates
                    .caption_seconds(board, &panel)
                    .map_or(board.settings.panel_frames, |s| rates.frames(board, s)));
            }
        };
        if frames == 0 || frames > u64::from(MAX_PANEL_FRAMES) {
            return Err(format!("durations are 1–{MAX_PANEL_FRAMES} frames."));
        }
        Ok(frames as u32)
    }
}

/// Whether `id` looks like a beat ID from read_storyboard_script ("s2b5").
pub fn is_beat_id(id: &str) -> bool {
    beat_index(id).is_some()
}

/// The 0-based scene and beat a beat ID names.
pub fn beat_index(id: &str) -> Option<(usize, usize)> {
    let rest = id.strip_prefix('s')?;
    let (scene, beat) = rest.split_once('b')?;
    let parse = |t: &str| {
        (!t.is_empty() && t.chars().all(|c| c.is_ascii_digit()))
            .then(|| t.parse::<usize>().ok())
            .flatten()
            .filter(|n| *n > 0)
    };
    Some((parse(scene)? - 1, parse(beat)? - 1))
}

/// The beat ID for the 0-based `scene` and `beat`.
pub fn beat_id(scene: usize, beat: usize) -> String {
    format!("s{}b{}", scene + 1, beat + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{ProjectEditor, ProjectKind};
    use serde_json::json;

    fn editor() -> ProjectEditor {
        ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(64, 36)).unwrap()
    }

    fn breakdown(value: serde_json::Value) -> Breakdown {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn a_breakdown_pastes_as_one_step_with_fields_by_name() {
        let mut p = editor();
        let b = breakdown(json!({"scenes":[
            {"name":"INT. KITCHEN - NIGHT","panels":[
                {"captions":{"Slugging":"INT. KITCHEN - NIGHT","Action":"Rain on the window."},"size":"wide","seconds":3,"notes":"Establishing","source_beats":["s1b1"]},
                {"captions":{"dialogue":"MIA: Is anyone there?","Notes":"Hold"},"notes":"Her eyes","camera":"PUSH IN","size":"close_up"}
            ]},
            {"panels":[{"captions":{"SFX":"Thunder"},"frames":12}]}
        ]}));
        let board = p.storyboard().unwrap();
        let clip = b.clip(board, &WordRates::default()).unwrap();
        assert_eq!(clip.scenes[0], "INT. KITCHEN - NIGHT");
        assert_eq!(clip.panels[0].panel.frames, 72);
        // 3 words at 150 wpm and a pause: 1.7 s at 24 fps.
        assert_eq!(clip.panels[1].panel.frames, 41);
        let ids = p.paste_panels(Some(1), &clip).unwrap();
        assert_eq!(ids.len(), 3);
        let board = p.storyboard().unwrap();
        let text = |id, field: &str| {
            board.panels[&id]
                .captions
                .get(&board.caption(field).unwrap())
                .map(|c| c.text.clone())
        };
        assert_eq!(text(ids[1], "Notes").as_deref(), Some("Hold\nHer eyes"));
        assert_eq!(text(ids[1], "Camera").as_deref(), Some("PUSH IN"));
        assert_eq!(text(ids[2], "SFX").as_deref(), Some("Thunder"));
        assert_eq!(board.panels[&ids[1]].size, ShotSize::CloseUp);
        // The unnamed scene took a name from the rules.
        assert_ne!(board.scenes[&board.panels[&ids[2]].scene].name, "");
        assert!(p.undo());
        assert_eq!(p.page_list().len(), 1);
        assert!(p.storyboard().unwrap().caption("Camera").is_none());
    }

    #[test]
    fn invalid_breakdowns_name_the_scene_and_panel() {
        let p = editor();
        let board = p.storyboard().unwrap();
        let rates = WordRates::default();
        let error = |v| breakdown(v).clip(board, &rates).unwrap_err();
        assert!(error(json!({"scenes":[]})).contains("no scenes"));
        assert!(error(json!({"scenes":[{"panels":[]}]})).contains("Scene 1"));
        let both = error(json!({"scenes":[{"panels":[{}, {"seconds":1,"frames":2}]}]}));
        assert!(
            both.starts_with("Scene 1 panel 2") && both.contains("not both"),
            "{both}"
        );
        assert!(error(json!({"scenes":[{"panels":[{"frames":0}]}]})).contains("frames"));
        let long = "x".repeat(MAX_CAPTION_CHARS + 1);
        assert!(
            error(json!({"scenes":[{"panels":[{"captions":{"Action":long}}]}]})).contains("Action")
        );
        let twice =
            error(json!({"scenes":[{"panels":[{"camera":"PAN","captions":{"camera":"TILT"}}]}]}));
        assert!(twice.contains("not both"));
        assert!(serde_json::from_value::<Breakdown>(json!({"scenes":[],"extra":1})).is_err());
        // Blank panels take the board's default length.
        let blank = breakdown(json!({"scenes":[{"panels":[{}]}]}))
            .clip(board, &rates)
            .unwrap();
        assert_eq!(blank.panels[0].panel.frames, board.settings.panel_frames);
    }

    #[test]
    fn beat_ids_round_trip() {
        assert_eq!(beat_id(1, 4), "s2b5");
        assert_eq!(beat_index("s2b5"), Some((1, 4)));
        for bad in ["s0b1", "s1", "b1", "s1b", "s1b-1", "x1b1", "s1b1x"] {
            assert!(!is_beat_id(bad), "{bad}");
        }
    }
}
