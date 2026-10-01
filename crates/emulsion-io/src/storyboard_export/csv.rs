//! Captions, timing and shot data as CSV: one row per panel, every caption
//! field as plain text, quoted as RFC 4180 describes.
use super::{Scope, angle_label, entries, select, shot_label, status_label, tag_label, timecode};
use anyhow::Result;
use emulsion_core::project::Project;
use std::path::Path;

/// One field, quoted when it holds a comma, quote or line break.
fn field(out: &mut String, value: &str) {
    if value.contains([',', '"', '\n', '\r']) {
        out.push('"');
        out.push_str(&value.replace('"', "\"\""));
        out.push('"');
    } else {
        out.push_str(value);
    }
}

fn row(out: &mut String, values: impl IntoIterator<Item = String>) {
    for (i, value) in values.into_iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        field(out, &value);
    }
    out.push_str("\r\n");
}

/// The CSV text for the panels of `scope`.
pub fn text(project: &Project, scope: &Scope) -> Result<String> {
    let board = super::board(project)?;
    let rate = board.settings.frame_rate;
    let chosen = select(entries(project)?, scope)?;
    let mut out = String::new();
    let mut header: Vec<String> = [
        "Index", "Act", "Sequence", "Scene", "Panel", "Frames", "Seconds", "Start", "End",
        "Duration",
    ]
    .map(String::from)
    .to_vec();
    header.extend(board.captions.iter().map(|f| f.name.clone()));
    header.extend(
        [
            "Shot size",
            "Angle",
            "Status",
            "Tag",
            "Locked",
            "Thumbnail sheet",
        ]
        .map(String::from),
    );
    row(&mut out, header);
    for e in &chosen {
        let length = e.length();
        let yes = |on: bool| if on { "Yes" } else { "No" }.to_string();
        let mut values = vec![
            e.index.to_string(),
            e.act.clone(),
            e.sequence.clone(),
            e.scene.clone(),
            e.name.clone(),
            e.panel.frames.to_string(),
            format!("{:.3}", f64::from(e.panel.frames) / rate.fps()),
            timecode(e.start, rate),
            timecode(e.start + length, rate),
            timecode(length, rate),
        ];
        values.extend(board.captions.iter().map(|f| {
            e.panel
                .captions
                .get(&f.id)
                .map_or_else(String::new, |c| c.text.clone())
        }));
        values.extend([
            shot_label(&e.panel).into(),
            angle_label(&e.panel).into(),
            status_label(&e.panel).into(),
            tag_label(&e.panel).into(),
            yes(board.is_locked(e.page)),
            yes(e.panel.thumbnails.is_some()),
        ]);
        row(&mut out, values);
    }
    Ok(out)
}

/// Write the CSV, with a UTF-8 byte order mark so spreadsheets read accents.
/// Returns the number of panel rows.
pub fn write(project: &Project, scope: &Scope, path: &Path) -> Result<usize> {
    let text = text(project, scope)?;
    let rows = text.matches("\r\n").count() - 1;
    for page in &project.pages {
        crate::ora::ensure_not_raw_original(&page.doc, path)?;
    }
    crate::write_atomic(path, |file| {
        std::io::Write::write_all(file, "\u{feff}".as_bytes())?;
        std::io::Write::write_all(file, text.as_bytes())?;
        Ok(())
    })?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storyboard_export::tests::project;

    #[test]
    fn rows_hold_captions_timing_and_shot_data_with_quoting() {
        let project = project();
        let csv = text(&project, &Scope::All).unwrap();
        let lines: Vec<_> = csv.split("\r\n").collect();
        assert_eq!(
            lines[0],
            "Index,Act,Sequence,Scene,Panel,Frames,Seconds,Start,End,Duration,Action,Dialogue,Slugging,Notes,Shot size,Angle,Status,Tag,Locked,Thumbnail sheet"
        );
        assert_eq!(
            lines[1],
            "1,Act 1,Sequence 1,1,Panel 1,48,2.000,00:00:00:00,00:00:02:00,00:00:02:00,\"Mia runs, \"\"fast\"\"\",\"Wait,\nfor me!\",,,Close-up,Not set,Rough,Red,No,No"
        );
        assert!(lines[3].starts_with(
            "3,Act 1,Sequence 1,2,Panel 3,12,0.500,00:00:04:00,00:00:04:12,00:00:00:12,"
        ));
        // Every record parses back to the same number of fields.
        let rows = crate::diagram_data::csv_rows(&csv).unwrap();
        assert_eq!(rows.len(), 4);
        assert!(rows.iter().all(|r| r.len() == rows[0].len()));
        assert_eq!(rows[1][10], "Mia runs, \"fast\"");
        assert_eq!(rows[1][11], "Wait,\nfor me!");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("board.csv");
        let scene = crate::storyboard_export::entries(&project).unwrap()[2].scene_id;
        assert_eq!(write(&project, &Scope::Scene(scene), &path).unwrap(), 1);
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with("\u{feff}Index,".as_bytes()));
    }
}
