//! CMX 3600 edit decision lists. One picture track (V) and up to four
//! sound channels (A, A2, A3, A4); clip names and media files travel in
//! `* FROM CLIP NAME:`, `* TO CLIP NAME:` and `* SOURCE FILE:` comments and
//! markers as Avid-style `* LOC:` locators. Dissolves and wipes from the
//! left or top are written as transitions; other transitions as dissolves.
//! Timecodes use drop-frame numbering at 29.97 and 59.94.
use anyhow::{Result, bail};
use emulsion_core::timeline::{
    Edge, Edit, EditClip, EditMarker, EditTransition, FrameRate, TransitionKind,
};

const REEL: &str = "AX";

/// The event line: number, reel, channel, transition and its length, and
/// source and record in and out.
#[allow(clippy::too_many_arguments)]
fn line(
    number: usize,
    channel: &str,
    transition: &str,
    frames: Option<u32>,
    rate: FrameRate,
    source: (u64, u64),
    record: (u64, u64),
) -> String {
    let frames = frames.map_or("   ".to_string(), |f| format!("{:03}", f.min(999)));
    format!(
        "{number:03}  {REEL:<8} {channel:<5} {transition:<4} {frames} {} {} {} {}",
        rate.timecode(source.0),
        rate.timecode(source.1),
        rate.timecode(record.0),
        rate.timecode(record.1),
    )
}

/// The EDL code for a transition kind, and whether it is exact.
fn code(kind: TransitionKind) -> (&'static str, bool) {
    match kind {
        TransitionKind::Dissolve => ("D", true),
        TransitionKind::Wipe { from: Edge::Left } => ("W001", true),
        TransitionKind::Wipe { from: Edge::Top } => ("W002", true),
        _ => ("D", false),
    }
}

/// Write `edit` as an EDL, with warnings for what it cannot hold.
pub fn write(edit: &Edit) -> (String, Vec<String>) {
    let rate = edit.rate;
    let start = edit.start;
    let mut warnings = Vec::new();
    let mut out = vec![
        format!("TITLE: {}", one_line(&edit.name)),
        format!(
            "FCM: {}",
            if rate.drop_frame() {
                "DROP FRAME"
            } else {
                "NON-DROP FRAME"
            }
        ),
        String::new(),
    ];
    let mut number = 0;
    let mut previous: Option<&EditClip> = None;
    let mut inexact = 0;
    for clip in edit.video.iter().filter(|c| c.track == 0) {
        number += 1;
        let record = (start + clip.record_in, start + clip.record_out);
        let source = (clip.source_in, clip.source_out);
        match (clip.transition, previous) {
            (Some(t), Some(from)) if from.record_out == clip.record_in && t.frames > 0 => {
                let (code, exact) = code(t.kind);
                if !exact {
                    inexact += 1;
                }
                out.push(line(
                    number,
                    "V",
                    "C",
                    None,
                    rate,
                    (from.source_out, from.source_out),
                    (record.0, record.0),
                ));
                out.push(line(
                    number,
                    "V",
                    code,
                    Some(t.frames),
                    rate,
                    source,
                    record,
                ));
                out.push(format!("* FROM CLIP NAME: {}", one_line(&from.name)));
                out.push(format!("* TO CLIP NAME: {}", one_line(&clip.name)));
            }
            _ => {
                out.push(line(number, "V", "C", None, rate, source, record));
                out.push(format!("* FROM CLIP NAME: {}", one_line(&clip.name)));
            }
        }
        if !clip.media.is_empty() {
            out.push(format!("* SOURCE FILE: {}", one_line(&clip.media)));
        }
        out.push(String::new());
        previous = Some(clip);
    }
    if inexact > 0 {
        warnings.push(format!(
            "{inexact} transition{} an EDL cannot name {} written as dissolves.",
            if inexact == 1 { "" } else { "s" },
            if inexact == 1 { "is" } else { "are" }
        ));
    }
    if edit.video.iter().any(|c| c.track > 0) {
        warnings.push("EDLs hold one picture track; the reference video is left out.".into());
    }
    let mut skipped = 0;
    for clip in &edit.audio {
        let channel = match clip.track {
            0 => "A",
            1 => "A2",
            2 => "A3",
            3 => "A4",
            _ => {
                skipped += 1;
                continue;
            }
        };
        number += 1;
        out.push(line(
            number,
            channel,
            "C",
            None,
            rate,
            (clip.source_in, clip.source_out),
            (start + clip.record_in, start + clip.record_out),
        ));
        out.push(format!("* FROM CLIP NAME: {}", one_line(&clip.name)));
        if !clip.media.is_empty() {
            out.push(format!("* SOURCE FILE: {}", one_line(&clip.media)));
        }
        out.push(String::new());
    }
    if skipped > 0 {
        warnings.push(format!(
            "EDLs hold four sound tracks; {skipped} clip{} on later tracks {} left out.",
            if skipped == 1 { "" } else { "s" },
            if skipped == 1 { "is" } else { "are" }
        ));
    }
    if edit
        .audio
        .iter()
        .any(|c| c.gain_db.is_some_and(|g| g != 0.))
    {
        warnings.push("EDLs carry no clip gain; sound levels are left out.".into());
    }
    for marker in &edit.markers {
        out.push(format!(
            "* LOC: {} RED     {}",
            rate.timecode(start + marker.frame),
            one_line(&marker.name)
        ));
    }
    out.push(String::new());
    (out.join("\r\n"), warnings)
}

fn one_line(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// A timecode in the EDL's numbering: drop-frame when it says so (or the
/// frame separator is `;`), which only NTSC rates have.
fn timecode(text: &str, rate: FrameRate, drop: bool) -> Option<u64> {
    let drop = drop || text.contains(';');
    if drop == rate.drop_frame() {
        return rate.parse_timecode(text);
    }
    // Non-drop numbering at an NTSC rate: count frames plainly.
    let parts: Vec<u64> = text
        .split([':', ';', '.'])
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    let [h, m, s, f] = parts[..] else {
        return None;
    };
    let base = rate.timebase();
    (m < 60 && s < 60 && f < base).then_some(((h * 60 + m) * 60 + s) * base + f)
}

/// One event line, before names are attached.
struct Line {
    number: String,
    reel: String,
    channel: String,
    transition: String,
    frames: u32,
    source: (u64, u64),
    record: (u64, u64),
}

/// An event's lines and the comments after them.
type Event = (Vec<Line>, Vec<(String, String)>);

/// The tracks a channel names: picture, and a sound track.
fn channel(text: &str) -> (bool, Option<usize>) {
    let upper = text.to_ascii_uppercase();
    let (audio, video) = match upper.as_str() {
        "V" => return (true, None),
        "B" => return (true, Some(0)),
        other => match other.split_once('/') {
            Some((a, "V")) => (a.to_string(), true),
            _ => (other.to_string(), false),
        },
    };
    let track = match audio.as_str() {
        "A" | "AA" | "A1" => Some(0),
        "A2" => Some(1),
        "A3" => Some(2),
        "A4" => Some(3),
        _ => None,
    };
    (video, track)
}

/// Read an EDL at `rate` (switched to 29.97 when it says drop frame and
/// `rate` has none).
pub fn read(text: &str, rate: FrameRate) -> Result<Edit> {
    let mut name = String::new();
    let drop = text.lines().any(|l| {
        let l = l.trim().to_ascii_uppercase();
        l.starts_with("FCM:") && l.contains("DROP") && !l.contains("NON")
    });
    let rate = if drop && !rate.drop_frame() {
        FrameRate::ntsc(if rate.fps() > 40. { 60 } else { 30 })
    } else {
        rate
    };
    // Events: their lines, then the comments after them.
    let mut events: Vec<Event> = Vec::new();
    let mut markers = Vec::new();
    for raw in text.lines() {
        let l = raw.trim();
        if l.is_empty() {
            continue;
        }
        if let Some(rest) = l.strip_prefix("TITLE:") {
            name = rest.trim().to_string();
            continue;
        }
        if let Some(rest) = l.strip_prefix('*') {
            let rest = rest.trim();
            if let Some((key, value)) = rest.split_once(':') {
                let key = key.trim().to_ascii_uppercase();
                let value = value.trim().to_string();
                if key == "LOC" {
                    // TC COLOR NAME
                    let mut parts = value.splitn(3, char::is_whitespace);
                    let tc = parts.next().unwrap_or_default();
                    let rest = parts.collect::<Vec<_>>().join(" ");
                    let label = rest
                        .trim()
                        .split_once(char::is_whitespace)
                        .map_or("", |(_, n)| n)
                        .trim()
                        .to_string();
                    if let Some(frame) = timecode(tc, rate, drop) {
                        markers.push((frame, label));
                    }
                } else if let Some(event) = events.last_mut() {
                    event.1.push((key, value));
                }
            }
            continue;
        }
        let tokens: Vec<&str> = l.split_whitespace().collect();
        if tokens.len() < 8 || !tokens[0].chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let tcs = &tokens[tokens.len() - 4..];
        let parse = |t: &str| {
            timecode(t, rate, drop).ok_or_else(|| anyhow::anyhow!("Unreadable timecode {t}"))
        };
        let line = Line {
            number: tokens[0].to_string(),
            reel: tokens[1].to_string(),
            channel: tokens[2].to_string(),
            transition: tokens[3].to_ascii_uppercase(),
            frames: if tokens.len() >= 9 {
                tokens[4].parse().unwrap_or(0)
            } else {
                0
            },
            source: (parse(tcs[0])?, parse(tcs[1])?),
            record: (parse(tcs[2])?, parse(tcs[3])?),
        };
        match events.last_mut() {
            Some(event)
                if event.0.last().is_some_and(|p| p.number == line.number)
                    && event.1.is_empty() =>
            {
                event.0.push(line)
            }
            _ => events.push((vec![line], Vec::new())),
        }
    }
    if events.is_empty() {
        bail!("This EDL has no events")
    }
    let first = events
        .iter()
        .flat_map(|e| e.0.iter().map(|l| l.record.0))
        .min()
        .unwrap_or(0);
    // The sequence starts on the hour of its first event.
    let start = (0..24)
        .filter_map(|h| timecode(&format!("{h:02}:00:00:00"), rate, drop))
        .take_while(|f| *f <= first)
        .last()
        .unwrap_or(0);
    let mut edit = Edit::new(&name, rate);
    edit.start = start;
    for (lines, comments) in events {
        let comment = |key: &str| {
            comments
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
        };
        let source_file = comment("SOURCE FILE").unwrap_or_default();
        let count = lines.len();
        for (i, line) in lines.into_iter().enumerate() {
            let reel = line.reel.to_ascii_uppercase();
            if matches!(reel.as_str(), "BL" | "BLK" | "BLACK") || line.record.1 <= line.record.0 {
                continue;
            }
            let incoming = count > 1 && i == count - 1;
            let name = if incoming {
                comment("TO CLIP NAME")
            } else {
                comment("FROM CLIP NAME")
            }
            .unwrap_or_else(|| line.reel.clone());
            let transition = match line.transition.as_str() {
                "D" => Some(TransitionKind::Dissolve),
                "W002" => Some(TransitionKind::Wipe { from: Edge::Top }),
                w if w.starts_with('W') => Some(TransitionKind::Wipe { from: Edge::Left }),
                _ => None,
            }
            .filter(|_| line.frames > 0)
            .map(|kind| EditTransition {
                kind,
                frames: line.frames,
            });
            let clip = EditClip {
                name,
                media: super::url_path(&source_file),
                track: 0,
                source_in: line.source.0,
                source_out: line.source.1.max(line.source.0),
                record_in: line.record.0.saturating_sub(start),
                record_out: line.record.1.saturating_sub(start),
                transition,
                gain_db: None,
            };
            let (video, audio) = channel(&line.channel);
            if let Some(track) = audio {
                edit.audio.push(EditClip {
                    track,
                    transition: None,
                    ..clip.clone()
                });
            }
            if video {
                edit.video.push(clip);
            }
        }
    }
    edit.markers = markers
        .into_iter()
        .map(|(frame, name)| EditMarker {
            frame: frame.saturating_sub(start),
            name,
        })
        .collect();
    edit.sort();
    Ok(edit)
}
