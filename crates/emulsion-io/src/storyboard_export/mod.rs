//! Storyboard exports: PDF boards laid out by a profile (printed through the
//! shared print pipeline), panel images named by a pattern, and a CSV of
//! captions and timing. Everything reads a project snapshot; nothing here
//! changes the storyboard.
use anyhow::{Context, Result, bail};
use emulsion_core::{
    project::{PageId, Project},
    storyboard::{
        CAMERA_ANGLES, FrameRate, GroupId, PANEL_STATUSES, Panel, SHOT_SIZES, Storyboard,
        TAG_PALETTE,
    },
};

pub mod csv;
pub mod images;
pub mod movie;
pub mod profile;
pub mod sheet;

pub use profile::{Alignment, CaptionPlacement, Fit, Profile};

/// One panel in board order, with the names exports print.
#[derive(Clone, Debug)]
pub struct Entry {
    pub page: PageId,
    /// Position in the board, from 1.
    pub index: usize,
    pub name: String,
    pub act: String,
    pub sequence: String,
    pub scene: String,
    pub scene_id: GroupId,
    /// Position in its scene, from 1.
    pub number: usize,
    /// Start in frames on the running time; thumbnail sheets take no time.
    pub start: u64,
    pub panel: Panel,
    /// When the scene camera moves during this panel: its frame's corners
    /// on the panel (in panel pixels) at the panel's first and last frames,
    /// without shake.
    pub camera_move: Option<[[(f64, f64); 4]; 2]>,
}

impl Entry {
    /// Frames this panel adds to the running time.
    pub fn length(&self) -> u64 {
        if self.panel.thumbnails.is_some() {
            0
        } else {
            u64::from(self.panel.frames)
        }
    }
}

/// The storyboard of a project.
pub fn board(project: &Project) -> Result<&Storyboard> {
    project
        .storyboard
        .as_ref()
        .context("Open a storyboard project first")
}

/// Every panel in board order.
pub fn entries(project: &Project) -> Result<Vec<Entry>> {
    let board = board(project)?;
    let layout: Vec<_> = project.pages.iter().map(|p| p.meta.id).collect();
    board.validate(&layout).map_err(anyhow::Error::msg)?;
    // Printed camera moves leave shake out.
    let mut steady = board.clone();
    for camera in steady.cameras.values_mut() {
        camera.shake = None;
    }
    let mut out = Vec::new();
    let mut start = 0;
    for scene in board.outline(&layout) {
        for (number, &page) in scene.panels.iter().enumerate() {
            let meta = &project
                .pages
                .iter()
                .find(|p| p.meta.id == page)
                .context("Missing panel page")?
                .meta;
            let entry = Entry {
                page,
                index: out.len() + 1,
                name: meta.name.clone(),
                act: board.acts[&scene.act].name.clone(),
                sequence: board.sequences[&scene.sequence].name.clone(),
                scene: board.scenes[&scene.scene].name.clone(),
                scene_id: scene.scene,
                number: number + 1,
                start,
                panel: board.panels[&page].clone(),
                camera_move: None,
            };
            let entry = Entry {
                camera_move: camera_move(&steady, &layout, &entry),
                ..entry
            };
            start += entry.length();
            out.push(entry);
        }
    }
    Ok(out)
}

/// The camera frame's corners at `entry`'s first and last frames, when the
/// camera moves between them.
fn camera_move(
    board: &Storyboard,
    layout: &[PageId],
    entry: &Entry,
) -> Option<[[(f64, f64); 4]; 2]> {
    if entry.length() == 0 || !board.cameras.contains_key(&entry.scene_id) {
        return None;
    }
    let corners = |frame: u64| board.camera_corners(board.camera_at(layout, frame as f64));
    let (first, last) = (
        corners(entry.start),
        corners(entry.start + entry.length() - 1),
    );
    let moved = first
        .iter()
        .zip(&last)
        .any(|(a, b)| (a.0 - b.0).abs() > 1e-6 || (a.1 - b.1).abs() > 1e-6);
    moved.then_some([first, last])
}

/// What an export covers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    #[default]
    All,
    /// These panels, exported in board order.
    Panels(Vec<PageId>),
    /// Every panel of one scene.
    Scene(GroupId),
}

/// The entries `scope` selects, in board order.
pub fn select(entries: Vec<Entry>, scope: &Scope) -> Result<Vec<Entry>> {
    let chosen: Vec<_> = match scope {
        Scope::All => entries,
        Scope::Panels(ids) => {
            if let Some(missing) = ids
                .iter()
                .find(|id| !entries.iter().any(|e| e.page == **id))
            {
                bail!("Panel {missing} is not in this storyboard")
            }
            entries
                .into_iter()
                .filter(|e| ids.contains(&e.page))
                .collect()
        }
        Scope::Scene(scene) => entries
            .into_iter()
            .filter(|e| e.scene_id == *scene)
            .collect(),
    };
    if chosen.is_empty() {
        bail!("Choose at least one panel to export")
    }
    Ok(chosen)
}

/// `HH:MM:SS:FF` at the nominal (rounded) frame rate, non-drop-frame.
pub fn timecode(frames: u64, rate: FrameRate) -> String {
    let fps = rate.fps().round().max(1.) as u64;
    let seconds = frames / fps;
    format!(
        "{:02}:{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60,
        frames % fps
    )
}

pub fn shot_label(panel: &Panel) -> &'static str {
    label(&SHOT_SIZES, panel.size)
}
pub fn angle_label(panel: &Panel) -> &'static str {
    label(&CAMERA_ANGLES, panel.angle)
}
pub fn status_label(panel: &Panel) -> &'static str {
    label(&PANEL_STATUSES, panel.status)
}
pub fn tag_label(panel: &Panel) -> &'static str {
    panel
        .tag
        .and_then(|t| TAG_PALETTE.get(usize::from(t)))
        .map_or("", |(name, _)| name)
}
fn label<T: PartialEq>(table: &[(T, &'static str)], value: T) -> &'static str {
    table
        .iter()
        .find(|(v, _)| *v == value)
        .map_or("", |(_, l)| l)
}

/// Tokens a panel answers in headers and file names.
pub const PANEL_TOKENS: &[&str] = &[
    "project", "act", "seq", "scene", "panel", "name", "index", "frames", "duration", "timecode",
    "shot", "angle", "status",
];
/// Tokens of page headers and footers: the page's first panel and the page.
pub const PAGE_TOKENS: &[&str] = &["project", "act", "seq", "scene", "page", "pages", "date"];

/// The value of a panel token.
pub fn panel_token(entry: &Entry, project: &str, rate: FrameRate, token: &str) -> Option<String> {
    Some(match token {
        "project" => project.into(),
        "act" => entry.act.clone(),
        "seq" => entry.sequence.clone(),
        "scene" => entry.scene.clone(),
        "panel" => entry.number.to_string(),
        "name" => entry.name.clone(),
        "index" => entry.index.to_string(),
        "frames" => entry.panel.frames.to_string(),
        "duration" => format!("{:.2} s", f64::from(entry.panel.frames) / rate.fps()),
        "timecode" => timecode(entry.start, rate),
        "shot" => shot_label(&entry.panel).into(),
        "angle" => angle_label(&entry.panel).into(),
        "status" => status_label(&entry.panel).into(),
        _ => return None,
    })
}

/// Split a pattern into literal text and `{token}` or `{token:width}` parts.
fn parts(pattern: &str) -> Result<Vec<(bool, &str, usize)>> {
    if pattern.chars().count() > 200 || pattern.chars().any(char::is_control) {
        bail!("Patterns are limited to 200 characters without line breaks")
    }
    let mut out = Vec::new();
    let mut rest = pattern;
    while let Some(open) = rest.find(['{', '}']) {
        if rest[open..].starts_with('}') {
            bail!("Close each token with }} after opening it with {{")
        }
        out.push((false, &rest[..open], 0));
        let close = rest[open..]
            .find('}')
            .map(|i| i + open)
            .context("Close each token with }, for example {scene}")?;
        let inner = &rest[open + 1..close];
        let (name, width) = match inner.split_once(':') {
            Some((name, width)) => (
                name,
                width
                    .parse::<usize>()
                    .ok()
                    .filter(|w| (1..=6).contains(w))
                    .context("Pad tokens to 1–6 digits, for example {index:3}")?,
            ),
            None => (inner, 0),
        };
        out.push((true, name, width));
        rest = &rest[close + 1..];
    }
    out.push((false, rest, 0));
    Ok(out)
}

/// Check a pattern only uses `allowed` tokens.
pub fn validate_pattern(pattern: &str, allowed: &[&str]) -> Result<()> {
    for (token, name, _) in parts(pattern)? {
        if token && !allowed.contains(&name) {
            bail!(
                "Unknown token {{{name}}}. Use {}",
                allowed
                    .iter()
                    .map(|t| format!("{{{t}}}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }
    Ok(())
}

/// Replace every token with `value(token)`. `{token:3}` pads a value that is a
/// whole number with zeros to three digits; other values are left as they are.
pub fn expand(pattern: &str, value: impl Fn(&str) -> Option<String>) -> Result<String> {
    let mut out = String::new();
    for (token, name, width) in parts(pattern)? {
        if !token {
            out.push_str(name);
            continue;
        }
        let text = value(name).with_context(|| format!("Unknown token {{{name}}}"))?;
        if width > 0 && !text.is_empty() && text.chars().all(|c| c.is_ascii_digit()) {
            out.push_str(&format!("{text:0>width$}"));
        } else {
            out.push_str(&text);
        }
    }
    Ok(out)
}

/// Today's date as `YYYY-MM-DD` (UTC).
pub fn today() -> String {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86_400) as i64;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use emulsion_core::{
        Document,
        project::{ProjectEditor, ProjectKind},
        storyboard::Level,
    };

    /// Two scenes: panels 1–2 in "1", panel 3 in "2", with captions.
    pub(crate) fn project() -> Project {
        let mut editor =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(64, 36)).unwrap();
        let blank = editor.storyboard().unwrap().blank_panel().unwrap();
        let first = editor.active_page();
        let frames = editor.storyboard().unwrap().settings.panel_frames;
        let panels = vec![
            ("Panel 2".to_string(), Panel::new(0, frames)),
            ("Panel 3".to_string(), Panel::new(0, 12)),
        ];
        let ids = editor
            .insert_panels(Some(first), &blank, panels, None)
            .unwrap();
        editor
            .edit_storyboard(|b| {
                let layout = [first, ids[0], ids[1]];
                b.split(&layout, ids[1], Level::Scene, Some("2"))?;
                let action = b.caption("Action").unwrap();
                let dialogue = b.caption("Dialogue").unwrap();
                let panel = b.panels.get_mut(&first).unwrap();
                let mut caption = emulsion_core::storyboard::Caption::from("Mia runs, \"fast\"");
                caption.apply_style(0..3, |s| s.bold = true);
                panel.captions.insert(action, caption);
                panel.captions.insert(dialogue, "Wait,\nfor me!".into());
                panel.size = emulsion_core::storyboard::ShotSize::CloseUp;
                panel.tag = Some(0);
                Ok(())
            })
            .unwrap();
        let mut project = editor.snapshot().unwrap();
        project.pages[0].meta.name = "Panel 1".into();
        project
    }

    #[test]
    fn entries_follow_the_outline_with_running_time() {
        let project = project();
        let entries = entries(&project).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(
            entries.iter().map(|e| e.scene.as_str()).collect::<Vec<_>>(),
            ["1", "1", "2"]
        );
        assert_eq!(
            entries.iter().map(|e| e.number).collect::<Vec<_>>(),
            [1, 2, 1]
        );
        assert_eq!(entries[1].start, 48);
        assert_eq!(entries[2].start, 96);
        let scene = select(entries.clone(), &Scope::Scene(entries[2].scene_id)).unwrap();
        assert_eq!(scene.len(), 1);
        assert!(select(entries.clone(), &Scope::Panels(vec![999])).is_err());
        let chosen = select(
            entries.clone(),
            &Scope::Panels(vec![entries[2].page, entries[0].page]),
        )
        .unwrap();
        assert_eq!(chosen[0].page, entries[0].page, "board order is kept");
    }

    #[test]
    fn tokens_expand_with_padding_and_unknown_tokens_fail() {
        let project = project();
        let entries = entries(&project).unwrap();
        let rate = FrameRate::whole(24);
        let value = |t: &str| panel_token(&entries[1], "Film", rate, t);
        assert_eq!(
            expand("{seq}_{scene:3}_{panel:2}-{index}", value).unwrap(),
            "Sequence 1_001_02-2"
        );
        assert_eq!(expand("{name:4}", value).unwrap(), "Panel 2");
        assert_eq!(expand("{timecode}", value).unwrap(), "00:00:02:00");
        validate_pattern("{act}/{shot}", PANEL_TOKENS).unwrap();
        for bad in [
            "{nope}",
            "{scene",
            "scene}",
            "{index:9}",
            "{index:x}",
            "a\nb",
        ] {
            assert!(validate_pattern(bad, PANEL_TOKENS).is_err(), "{bad}");
        }
        assert!(expand("{page}", value).is_err());
    }

    #[test]
    fn timecode_uses_the_nominal_rate_and_dates_are_iso() {
        assert_eq!(timecode(0, FrameRate::whole(24)), "00:00:00:00");
        assert_eq!(timecode(24 * 3661 + 5, FrameRate::whole(24)), "01:01:01:05");
        assert_eq!(timecode(31, FrameRate::ntsc(30)), "00:00:01:01");
        let date = today();
        assert_eq!(date.len(), 10);
        assert!(date.starts_with("20"));
    }
}
