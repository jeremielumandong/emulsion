//! Storyboard PDF layout profiles: how panels, captions, headers and logos
//! sit on a page. Profiles are validated, kept in settings, shared as JSON
//! files, and three built-in profiles cover the usual boards.
use super::{PAGE_TOKENS, PANEL_TOKENS, validate_pattern};
use crate::printing::Paper;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::{io::Read, path::Path};

/// Where captions go beside each panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptionPlacement {
    None,
    #[default]
    Below,
    Right,
    Left,
}

/// How a panel picture fills its box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    /// The whole picture, letterboxed.
    #[default]
    Fit,
    /// The box is filled; the picture's edges are cropped.
    Fill,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
}

impl Alignment {
    pub fn text(self) -> emulsion_core::text::Align {
        match self {
            Self::Left => emulsion_core::text::Align::Left,
            Self::Center => emulsion_core::text::Align::Center,
            Self::Right => emulsion_core::text::Align::Right,
        }
    }
}

/// Every option of a storyboard PDF layout. Lengths are in millimetres, text
/// sizes in points.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    // Page
    pub paper: Paper,
    pub landscape: bool,
    /// Margin inside the paper's printable area.
    pub margin_mm: f64,
    // Panels
    pub columns: u16,
    pub rows: u16,
    pub gutter_mm: f64,
    pub fit: Fit,
    /// Line around each panel picture; 0 draws none.
    pub panel_frame_mm: f64,
    /// Line above each panel, with tokens such as {scene} and {name}; empty hides it.
    pub panel_header: String,
    /// A second line under the first, such as the duration; empty hides it.
    pub second_panel_header: String,
    pub panel_header_align: Alignment,
    pub panel_header_pt: f64,
    // Captions
    pub captions: CaptionPlacement,
    /// Share of each panel's box given to captions, in percent.
    pub caption_percent: f64,
    /// A line around each caption box.
    pub caption_frames: bool,
    /// Start each caption with its field name.
    pub caption_titles: bool,
    /// Caption fields to print, by name; empty prints the fields marked for
    /// printing.
    pub caption_fields: Vec<String>,
    pub caption_pt: f64,
    // Page header and footer
    /// Tokens: {project}, {page}, {pages}, {date}, and {act}, {seq}, {scene}
    /// of the page's first panel.
    pub page_header: String,
    pub page_header_align: Alignment,
    pub page_footer: String,
    pub page_footer_align: Alignment,
    pub page_text_pt: f64,
    /// A PNG or JPEG drawn in the page header.
    pub logo: Option<std::path::PathBuf>,
    pub logo_align: Alignment,
    pub logo_height_mm: f64,
    // Camera
    /// Draw the camera frame on each panel.
    pub camera_frame: bool,
    /// Draw the board's action and title safe areas inside the camera frame.
    pub safe_areas: bool,
    pub camera_frame_mm: f64,
    /// Line weight of camera-move arrows (panels whose scene camera moves
    /// print its start and end frames, at the camera frame weight).
    pub camera_arrow_mm: f64,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            name: "Storyboard".into(),
            paper: Paper::pdf().remove(0),
            landscape: true,
            margin_mm: 10.,
            columns: 3,
            rows: 2,
            gutter_mm: 6.,
            fit: Fit::Fit,
            panel_frame_mm: 0.3,
            panel_header: "Scene {scene} · {name}".into(),
            second_panel_header: String::new(),
            panel_header_align: Alignment::Left,
            panel_header_pt: 8.,
            captions: CaptionPlacement::Below,
            caption_percent: 35.,
            caption_frames: false,
            caption_titles: true,
            caption_fields: Vec::new(),
            caption_pt: 7.,
            page_header: "{project}".into(),
            page_header_align: Alignment::Left,
            page_footer: "Page {page} of {pages} · {date}".into(),
            page_footer_align: Alignment::Right,
            page_text_pt: 9.,
            logo: None,
            logo_align: Alignment::Right,
            logo_height_mm: 10.,
            camera_frame: false,
            safe_areas: false,
            camera_frame_mm: 0.4,
            camera_arrow_mm: 0.6,
        }
    }
}

pub const MAX_PROFILES: usize = 100;
const MAX_FILE: u64 = 256 * 1024;

fn range(value: f64, min: f64, max: f64, what: &str) -> Result<()> {
    if !value.is_finite() || !(min..=max).contains(&value) {
        bail!("{what} must be {min}–{max}")
    }
    Ok(())
}

impl Profile {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty()
            || self.name.chars().count() > 100
            || self.name.chars().any(char::is_control)
        {
            bail!("Name the profile with 1–100 characters")
        }
        let p = &self.paper;
        range(p.width, 50., 2000., "Paper width (mm)")?;
        range(p.height, 50., 2000., "Paper height (mm)")?;
        if p.margins
            .iter()
            .any(|m| !m.is_finite() || *m < 0. || *m > 100.)
        {
            bail!("Printer margins must be 0–100 mm")
        }
        range(self.margin_mm, 0., 50., "Margin (mm)")?;
        if !(1..=6).contains(&self.columns) || !(1..=8).contains(&self.rows) {
            bail!("Use 1–6 columns and 1–8 rows of panels")
        }
        range(self.gutter_mm, 0., 50., "Gutter (mm)")?;
        range(self.panel_frame_mm, 0., 5., "Panel frame (mm)")?;
        range(self.panel_header_pt, 4., 36., "Panel header size (pt)")?;
        range(self.caption_percent, 10., 80., "Caption share (%)")?;
        range(self.caption_pt, 4., 36., "Caption size (pt)")?;
        range(self.page_text_pt, 4., 36., "Header and footer size (pt)")?;
        range(self.logo_height_mm, 3., 40., "Logo height (mm)")?;
        range(self.camera_frame_mm, 0.05, 5., "Camera frame (mm)")?;
        range(self.camera_arrow_mm, 0.05, 5., "Camera arrow (mm)")?;
        for pattern in [&self.panel_header, &self.second_panel_header] {
            validate_pattern(pattern, PANEL_TOKENS)?;
        }
        for pattern in [&self.page_header, &self.page_footer] {
            validate_pattern(pattern, PAGE_TOKENS)?;
        }
        if self.caption_fields.len() > emulsion_core::storyboard::MAX_CAPTION_FIELDS
            || self
                .caption_fields
                .iter()
                .any(|f| f.trim().is_empty() || f.chars().count() > 200)
        {
            bail!("List up to 32 caption field names")
        }
        if let Some(logo) = &self.logo
            && !logo.is_absolute()
        {
            bail!("Choose the logo by its full path")
        }
        Ok(())
    }

    /// Page size in millimetres, in the profile's orientation.
    pub fn page_size(&self) -> (f64, f64) {
        if self.landscape {
            (self.paper.height, self.paper.width)
        } else {
            (self.paper.width, self.paper.height)
        }
    }

    pub fn panels_per_page(&self) -> usize {
        usize::from(self.columns) * usize::from(self.rows)
    }
}

/// The built-in profiles, which cannot be changed or removed.
pub fn builtins() -> Vec<Profile> {
    let base = Profile::default();
    vec![
        Profile {
            name: "3 per page · captions right".into(),
            landscape: false,
            columns: 1,
            rows: 3,
            captions: CaptionPlacement::Right,
            caption_percent: 40.,
            caption_frames: true,
            ..base.clone()
        },
        Profile {
            name: "6 per page · captions below".into(),
            ..base.clone()
        },
        Profile {
            name: "1 per page · large".into(),
            columns: 1,
            rows: 1,
            caption_percent: 20.,
            panel_header_pt: 11.,
            second_panel_header: "{duration} · {shot}".into(),
            caption_pt: 10.,
            camera_frame: true,
            ..base
        },
    ]
}

/// A built-in profile by name, ignoring case.
pub fn builtin(name: &str) -> Option<Profile> {
    builtins()
        .into_iter()
        .find(|p| p.name.eq_ignore_ascii_case(name))
}

/// Add or replace a saved profile, by name. Built-in names are reserved.
pub fn save(saved: &mut Vec<Profile>, profile: Profile) -> Result<()> {
    profile.validate()?;
    if builtin(&profile.name).is_some() {
        bail!(
            "“{}” is a built-in profile; save under another name",
            profile.name
        )
    }
    if let Some(existing) = saved.iter_mut().find(|p| p.name == profile.name) {
        *existing = profile;
    } else {
        if saved.len() >= MAX_PROFILES {
            bail!("At most {MAX_PROFILES} storyboard PDF profiles can be saved")
        }
        saved.push(profile);
    }
    Ok(())
}

/// Write a profile to share.
pub fn export(profile: &Profile, path: &Path) -> Result<()> {
    profile.validate()?;
    let bytes = serde_json::to_vec_pretty(profile)?;
    crate::write_atomic(path, |file| {
        std::io::Write::write_all(file, &bytes)?;
        Ok(())
    })?;
    Ok(())
}

/// Read a shared profile file.
pub fn import(path: &Path) -> Result<Profile> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_FILE + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE {
        bail!("Profile files are limited to 256 KiB")
    }
    let profile: Profile = serde_json::from_slice(&bytes)?;
    profile.validate()?;
    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_valid_and_cover_the_usual_boards() {
        let all = builtins();
        assert_eq!(all.len(), 3);
        for p in &all {
            p.validate().unwrap();
        }
        assert_eq!(all[0].panels_per_page(), 3);
        assert_eq!(all[0].captions, CaptionPlacement::Right);
        assert_eq!(all[1].panels_per_page(), 6);
        assert_eq!(all[1].captions, CaptionPlacement::Below);
        assert_eq!(all[2].panels_per_page(), 1);
        assert!(builtin("6 PER PAGE · CAPTIONS BELOW").is_some());
        assert_eq!(all[1].page_size(), (297., 210.));
    }

    #[test]
    fn invalid_options_are_rejected() {
        let ok = Profile::default();
        let cases: Vec<fn(&mut Profile)> = vec![
            |p| p.name = " ".into(),
            |p| p.columns = 0,
            |p| p.rows = 9,
            |p| p.gutter_mm = f64::NAN,
            |p| p.caption_percent = 90.,
            |p| p.caption_pt = 2.,
            |p| p.paper.width = 10.,
            |p| p.panel_header = "{page}".into(),
            |p| p.page_footer = "{name}".into(),
            |p| p.logo = Some("logo.png".into()),
            |p| p.camera_arrow_mm = 0.,
            |p| p.caption_fields = vec!["".into()],
        ];
        for (i, change) in cases.iter().enumerate() {
            let mut p = ok.clone();
            change(&mut p);
            assert!(p.validate().is_err(), "case {i}");
        }
    }

    #[test]
    fn profiles_round_trip_through_files_and_old_files_get_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("board.json");
        let mut profile = builtins().remove(0);
        profile.name = "Studio".into();
        profile.caption_fields = vec!["Action".into()];
        export(&profile, &path).unwrap();
        assert_eq!(import(&path).unwrap(), profile);
        std::fs::write(&path, r#"{"name":"Old","rows":1}"#).unwrap();
        let old = import(&path).unwrap();
        assert_eq!(old.rows, 1);
        assert_eq!(old.columns, Profile::default().columns);
        std::fs::write(&path, r#"{"name":"Bad","rows":0}"#).unwrap();
        assert!(import(&path).is_err());
        std::fs::write(&path, r#"{"name":"Bad","colour":1}"#).unwrap();
        assert!(import(&path).is_err());
    }

    #[test]
    fn saving_replaces_by_name_and_reserves_builtin_names() {
        let mut saved = Vec::new();
        let mut profile = Profile {
            name: "Mine".into(),
            ..Default::default()
        };
        save(&mut saved, profile.clone()).unwrap();
        profile.rows = 1;
        save(&mut saved, profile.clone()).unwrap();
        assert_eq!(saved, vec![profile]);
        assert!(save(&mut saved, builtins().remove(0)).is_err());
        let invalid = Profile {
            name: "Broken".into(),
            columns: 0,
            ..Default::default()
        };
        assert!(save(&mut saved, invalid).is_err());
        assert_eq!(saved.len(), 1);
    }
}
