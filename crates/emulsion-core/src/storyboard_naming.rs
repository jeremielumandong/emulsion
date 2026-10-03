//! Storyboard naming rules, renumbering, thumbnail sheets and the defaults
//! new storyboards start from.
use crate::Document;
use emulsion_raster::IRect;
use serde::{Deserialize, Serialize};

const MAX_PREFIX_CHARS: usize = 40;
const MAX_DIGITS: u8 = 6;

/// How scenes and panels are named and renumbered.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Naming {
    pub scene_prefix: String,
    pub scene_start: u32,
    pub scene_step: u32,
    /// Minimum digits; shorter numbers are padded with zeros.
    pub scene_digits: u8,
    pub panel_prefix: String,
    pub panel_digits: u8,
    /// Panel numbers restart in every scene; otherwise they run through the
    /// project.
    pub panels_per_scene: bool,
    /// A scene started inside another is named after it with a letter
    /// (10 → 10A → 10B) instead of taking the next number.
    pub insert_letters: bool,
}

impl Default for Naming {
    fn default() -> Self {
        Self {
            scene_prefix: String::new(),
            scene_start: 1,
            scene_step: 1,
            scene_digits: 0,
            panel_prefix: "Panel ".into(),
            panel_digits: 0,
            panels_per_scene: true,
            insert_letters: true,
        }
    }
}

impl Naming {
    pub fn validate(&self) -> Result<(), String> {
        for prefix in [&self.scene_prefix, &self.panel_prefix] {
            if prefix.chars().count() > MAX_PREFIX_CHARS || prefix.chars().any(char::is_control) {
                return Err(format!(
                    "Name prefixes are limited to {MAX_PREFIX_CHARS} characters."
                ));
            }
        }
        if self.scene_start > 1_000_000 || !(1..=1000).contains(&self.scene_step) {
            return Err("Scene numbers start at 0–1000000 and step by 1–1000.".into());
        }
        if self.scene_digits > MAX_DIGITS || self.panel_digits > MAX_DIGITS {
            return Err(format!("Pad numbers to at most {MAX_DIGITS} digits."));
        }
        Ok(())
    }
    /// The name of the scene at `index` in outline order.
    pub fn scene_name(&self, index: usize) -> String {
        let number = u64::from(self.scene_start) + index as u64 * u64::from(self.scene_step);
        let digits = usize::from(self.scene_digits);
        format!("{}{number:0digits$}", self.scene_prefix)
            .trim()
            .into()
    }
    /// The name of a panel numbered `number` (from 1).
    pub fn panel_name(&self, number: usize) -> String {
        let digits = usize::from(self.panel_digits);
        format!("{}{number:0digits$}", self.panel_prefix)
            .trim()
            .into()
    }
}

/// The name for a scene inserted after `previous`: the same name with the
/// next free letter. `None` when `previous` already ends in Z.
pub fn inserted_name(previous: &str, taken: impl Fn(&str) -> bool) -> Option<String> {
    let (base, mut letter) = match previous.chars().next_back() {
        Some(c @ 'A'..='Y')
            if previous[..previous.len() - 1]
                .chars()
                .next_back()
                .is_some_and(|d| d.is_ascii_digit()) =>
        {
            (&previous[..previous.len() - 1], c as u8 + 1)
        }
        _ => (previous, b'A'),
    };
    while letter <= b'Z' {
        let name = format!("{base}{}", letter as char);
        if !taken(&name) && name.chars().count() <= 200 {
            return Some(name);
        }
        letter += 1;
    }
    None
}

/// What `renumber` renames.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "groups")]
pub enum RenumberScope {
    All,
    /// Acts, sequences or scenes, with every scene and panel inside them.
    Groups(Vec<crate::storyboard::GroupId>),
}

/// A sheet of thumbnail frames drawn on one panel, later converted into one
/// panel per cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThumbnailGrid {
    pub columns: u8,
    pub rows: u8,
    /// Space between cells, in pixels.
    pub gap: u32,
    /// Space around the grid, in pixels.
    pub margin: u32,
}

impl ThumbnailGrid {
    pub const MAX_CELLS_PER_SIDE: u8 = 8;
    const MIN_CELL: i64 = 8;

    pub fn new(columns: u8, rows: u8) -> Self {
        Self {
            columns,
            rows,
            gap: 24,
            margin: 32,
        }
    }
    pub fn validate(&self, width: u32, height: u32) -> Result<(), String> {
        let max = Self::MAX_CELLS_PER_SIDE;
        if !(1..=max).contains(&self.columns) || !(1..=max).contains(&self.rows) {
            return Err(format!("Thumbnail sheets have 1–{max} columns and rows."));
        }
        if self.cells(width, height).is_empty() {
            return Err(
                "Thumbnail cells are too small; use fewer cells, a smaller gap or margin.".into(),
            );
        }
        Ok(())
    }
    /// Camera frames in row order: the largest rectangle with the sheet's
    /// aspect ratio centred in each cell. Empty when cells are too small.
    pub fn cells(&self, width: u32, height: u32) -> Vec<IRect> {
        let (columns, rows) = (i64::from(self.columns), i64::from(self.rows));
        let (gap, margin) = (i64::from(self.gap), i64::from(self.margin));
        let cell_w = (i64::from(width) - 2 * margin - (columns - 1) * gap) / columns.max(1);
        let cell_h = (i64::from(height) - 2 * margin - (rows - 1) * gap) / rows.max(1);
        if columns < 1 || rows < 1 {
            return Vec::new();
        }
        // Fit the frame aspect (width:height) inside the cell.
        let (aspect_w, aspect_h) = (i64::from(width), i64::from(height));
        let (frame_w, frame_h) = if cell_w * aspect_h <= cell_h * aspect_w {
            (cell_w, cell_w * aspect_h / aspect_w.max(1))
        } else {
            (cell_h * aspect_w / aspect_h.max(1), cell_h)
        };
        if frame_w < Self::MIN_CELL || frame_h < Self::MIN_CELL {
            return Vec::new();
        }
        let mut out = Vec::new();
        for row in 0..rows {
            for column in 0..columns {
                let x = margin + column * (cell_w + gap) + (cell_w - frame_w) / 2;
                let y = margin + row * (cell_h + gap) + (cell_h - frame_h) / 2;
                out.push(IRect {
                    x: x as i32,
                    y: y as i32,
                    w: frame_w as i32,
                    h: frame_h as i32,
                });
            }
        }
        out
    }
}

/// `rect` of `doc` scaled to `width` × `height`, keeping every layer
/// editable. Pixels outside `rect` are dropped. The caller makes `rect` match
/// the target aspect ratio; any difference stretches.
pub fn fit_document(doc: &Document, rect: IRect, width: u32, height: u32) -> Document {
    let mut out = doc.clone();
    let full = IRect {
        x: 0,
        y: 0,
        w: doc.width as i32,
        h: doc.height as i32,
    };
    if rect != full {
        crate::geometry::crop(&mut out, rect, 0.);
        crate::geometry::trim_to_canvas(&mut out);
    }
    if (out.width, out.height) != (width, height) {
        crate::geometry::resize(&mut out, width, height);
    }
    out
}

/// `doc` at `width` × `height`: unchanged when it already is, otherwise its
/// centre cropped to that aspect and scaled.
pub fn fit_to_frame(doc: &Document, width: u32, height: u32) -> Document {
    if (doc.width, doc.height) == (width, height) {
        doc.clone()
    } else {
        fit_document(doc, centred_frame(doc, width, height), width, height)
    }
}

/// The largest rectangle with `width:height` aspect centred on `doc`.
pub fn centred_frame(doc: &Document, width: u32, height: u32) -> IRect {
    let (dw, dh) = (i64::from(doc.width), i64::from(doc.height));
    let (w, h) = (i64::from(width), i64::from(height));
    let (fw, fh) = if dw * h <= dh * w {
        (dw, (dw * h / w).max(1))
    } else {
        ((dh * w / h).max(1), dh)
    };
    IRect {
        x: ((dw - fw) / 2) as i32,
        y: ((dh - fh) / 2) as i32,
        w: fw as i32,
        h: fh as i32,
    }
}

/// A caption field new storyboards start with.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaptionPreset {
    pub name: String,
    pub multiline: bool,
    pub print: bool,
}

/// User preferences for new storyboards and the board view.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub naming: Naming,
    /// Duration of new panels.
    pub panel_seconds: f64,
    pub captions: Vec<CaptionPreset>,
    /// Layers Smart add carries into the next panel, by name.
    pub smart_add_layers: Vec<String>,
    /// Board view thumbnail width, in logical pixels.
    pub thumbnail_width: u32,
    pub show_captions_on_board: bool,
    /// Stage guides new storyboards start with.
    pub stage: crate::storyboard_stage::StageGuides,
    /// Palette new storyboards start with.
    pub palette: Vec<[u8; 3]>,
    /// Light table settings for the Stage.
    pub light_table: crate::storyboard_stage::LightTable,
    /// The microphone sound is recorded from, by name; the system default
    /// when unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_input: Option<String>,
    /// Underline misspelt words in captions.
    pub check_spelling: bool,
    /// Words the spelling checker accepts, added with Add to dictionary.
    pub spelling_words: Vec<String>,
    /// The name review notes are signed with.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub review_author: String,
    /// Leave review layers out of Board and Timeline thumbnails too.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hide_review_in_thumbnails: bool,
    /// The text-to-speech engine scratch voices use.
    #[serde(skip_serializing_if = "crate::storyboard_voices::EngineChoice::is_auto")]
    pub voice_engine: crate::storyboard_voices::EngineChoice,
    /// The folder of downloaded Piper voices (`.onnx` with `.onnx.json`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub piper_voices: Option<String>,
    /// Word rates for Timing › Estimate durations from captions.
    #[serde(skip_serializing_if = "crate::storyboard_estimate::WordRates::is_default")]
    pub duration_rates: crate::storyboard_estimate::WordRates,
    /// The program Edit in external editor opens panels with (a path, or a
    /// command on the PATH); the system's default app for the file type
    /// when empty.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub external_editor: String,
    /// Send panels to the external editor as OpenRaster instead of PSD.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub external_editor_ora: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            naming: Naming::default(),
            panel_seconds: 2.,
            captions: [
                ("Action", true, true),
                ("Dialogue", true, true),
                ("Slugging", false, true),
                ("Notes", true, false),
            ]
            .into_iter()
            .map(|(name, multiline, print)| CaptionPreset {
                name: name.into(),
                multiline,
                print,
            })
            .collect(),
            smart_add_layers: vec!["Background".into()],
            thumbnail_width: 200,
            show_captions_on_board: true,
            stage: Default::default(),
            palette: crate::storyboard_stage::DEFAULT_PALETTE.to_vec(),
            light_table: Default::default(),
            audio_input: None,
            check_spelling: true,
            spelling_words: Vec::new(),
            review_author: String::new(),
            hide_review_in_thumbnails: false,
            voice_engine: Default::default(),
            piper_voices: None,
            duration_rates: Default::default(),
            external_editor: String::new(),
            external_editor_ora: false,
        }
    }
}

impl Preferences {
    pub const THUMBNAIL_WIDTHS: std::ops::RangeInclusive<u32> = 96..=480;
    pub const MAX_SPELLING_WORDS: usize = 10_000;

    pub fn validate(&self) -> Result<(), String> {
        self.naming.validate()?;
        if !self.panel_seconds.is_finite() || !(0.01..=600.).contains(&self.panel_seconds) {
            return Err("New panels last 0.01–600 seconds.".into());
        }
        if self.captions.len() > crate::storyboard::MAX_CAPTION_FIELDS {
            return Err(format!(
                "Use at most {} caption fields.",
                crate::storyboard::MAX_CAPTION_FIELDS
            ));
        }
        let mut names = std::collections::HashSet::new();
        for caption in &self.captions {
            crate::storyboard::check_name(&caption.name, "Caption field")?;
            if !names.insert(caption.name.trim().to_lowercase()) {
                return Err("Caption field names must be unique.".into());
            }
        }
        if self.smart_add_layers.len() > 64
            || self
                .smart_add_layers
                .iter()
                .any(|n| n.trim().is_empty() || n.chars().count() > 200)
        {
            return Err("Smart add takes up to 64 layer names of 1–200 characters.".into());
        }
        self.stage.validate()?;
        self.light_table.validate()?;
        self.duration_rates.validate()?;
        crate::storyboard_stage::validate_palette(&self.palette)?;
        if !Self::THUMBNAIL_WIDTHS.contains(&self.thumbnail_width) {
            return Err("Board thumbnails are 96–480 pixels wide.".into());
        }
        if self
            .piper_voices
            .as_ref()
            .is_some_and(|p| p.trim().is_empty() || p.chars().count() > 4096)
        {
            return Err("The Piper voices folder path is 1–4096 characters.".into());
        }
        if self
            .audio_input
            .as_ref()
            .is_some_and(|n| n.trim().is_empty() || n.chars().count() > 400)
        {
            return Err("Audio input device names are 1–400 characters.".into());
        }
        if self.spelling_words.len() > Self::MAX_SPELLING_WORDS
            || self
                .spelling_words
                .iter()
                .any(|w| w.trim().is_empty() || w.chars().count() > 100)
        {
            return Err(format!(
                "The personal dictionary holds up to {} words of 1–100 characters.",
                Self::MAX_SPELLING_WORDS
            ));
        }
        if self.review_author.chars().count() > crate::storyboard_review::MAX_AUTHOR_CHARS
            || self.review_author.chars().any(char::is_control)
        {
            return Err(format!(
                "The review author name is at most {} characters.",
                crate::storyboard_review::MAX_AUTHOR_CHARS
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_prefix_padding_and_step() {
        let naming = Naming {
            scene_prefix: "SC".into(),
            scene_start: 10,
            scene_step: 10,
            scene_digits: 3,
            panel_digits: 2,
            ..Naming::default()
        };
        assert_eq!(naming.scene_name(0), "SC010");
        assert_eq!(naming.scene_name(2), "SC030");
        assert_eq!(naming.panel_name(3), "Panel 03");
        assert_eq!(Naming::default().scene_name(4), "5");
        assert!(
            Naming {
                scene_step: 0,
                ..Naming::default()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn inserted_scenes_take_the_next_free_letter() {
        let taken = |n: &str| n == "10A";
        assert_eq!(inserted_name("10", taken).unwrap(), "10B");
        assert_eq!(inserted_name("10B", |_| false).unwrap(), "10C");
        assert_eq!(inserted_name("Chase", |_| false).unwrap(), "ChaseA");
        assert_eq!(inserted_name("10Z", |_| false).unwrap(), "10ZA");
        assert!(inserted_name("1", |_| true).is_none());
    }

    #[test]
    fn thumbnail_cells_keep_the_frame_aspect() {
        let grid = ThumbnailGrid::new(3, 2);
        let cells = grid.cells(1920, 1080);
        assert_eq!(cells.len(), 6);
        for cell in &cells {
            let ratio = f64::from(cell.w) / f64::from(cell.h);
            assert!((ratio - 16. / 9.).abs() < 0.02, "{cell:?}");
            assert!(cell.x >= 32 && cell.x + cell.w <= 1920 - 32);
        }
        assert!(cells[1].x > cells[0].x && cells[3].y > cells[0].y);
        assert!(ThumbnailGrid::new(8, 8).validate(64, 64).is_err());
        assert!(ThumbnailGrid::new(0, 2).validate(1920, 1080).is_err());
    }

    #[test]
    fn fitting_crops_and_scales_to_the_frame() {
        let doc = Document::new(400, 200);
        let rect = IRect {
            x: 100,
            y: 50,
            w: 200,
            h: 100,
        };
        let fitted = fit_document(&doc, rect, 80, 40);
        assert_eq!((fitted.width, fitted.height), (80, 40));
        let frame = centred_frame(&doc, 100, 100);
        assert_eq!((frame.x, frame.y, frame.w, frame.h), (100, 0, 200, 200));
    }

    #[test]
    fn preferences_validate_and_default_to_the_usual_captions() {
        let prefs = Preferences::default();
        prefs.validate().unwrap();
        assert_eq!(prefs.captions.len(), 4);
        let json = serde_json::to_string(&prefs).unwrap();
        assert_eq!(serde_json::from_str::<Preferences>(&json).unwrap(), prefs);
        assert_eq!(serde_json::from_str::<Preferences>("{}").unwrap(), prefs);
        assert!(
            Preferences {
                thumbnail_width: 10,
                ..prefs
            }
            .validate()
            .is_err()
        );
    }
}
