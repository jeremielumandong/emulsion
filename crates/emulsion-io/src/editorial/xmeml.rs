//! XML edit list (xmeml version 5), the XML interchange most editing
//! software reads: one sequence with the panel
//! track and any reference video tracks, sound tracks with each clip's
//! gain as an Audio Levels filter, transitions starting at their cuts, and
//! sequence markers. Read back with the shared bounded XML reader.
use crate::diagram_import::xml::{self, Xml};
use anyhow::{Context, Result, bail};
use emulsion_core::timeline::{
    Edge, Edit, EditClip, EditMarker, EditTransition, FrameRate, TransitionKind,
};
use std::collections::HashMap;
use std::fmt::Write;

fn esc(text: &str) -> String {
    quick_xml::escape::escape(text).into_owned()
}

fn rate_xml(rate: FrameRate) -> String {
    format!(
        "<rate><timebase>{}</timebase><ntsc>{}</ntsc></rate>",
        rate.timebase(),
        if rate.den == 1 { "FALSE" } else { "TRUE" }
    )
}

/// FCP's name for a transition, its category, and an edge-wipe angle.
fn effect(kind: TransitionKind) -> (&'static str, &'static str, Option<i32>) {
    match kind {
        TransitionKind::Cut | TransitionKind::Dissolve => ("Cross Dissolve", "Dissolve", None),
        TransitionKind::Wipe { from } => (
            "Edge Wipe",
            "Wipe",
            Some(match from {
                Edge::Left => 0,
                Edge::Top => 90,
                Edge::Right => 180,
                Edge::Bottom => 270,
            }),
        ),
        TransitionKind::Clock => ("Clock Wipe", "Wipe", None),
        TransitionKind::Iris => ("Round Iris", "Iris", None),
        TransitionKind::Slide { from } => (
            "Push Slide",
            "Slide",
            Some(match from {
                Edge::Left => 0,
                Edge::Top => 90,
                Edge::Right => 180,
                Edge::Bottom => 270,
            }),
        ),
        TransitionKind::FadeToColor { .. } => ("Dip to Color Dissolve", "Dissolve", None),
    }
}

fn edge(angle: i64) -> Edge {
    match angle.rem_euclid(360) {
        45..135 => Edge::Top,
        135..225 => Edge::Right,
        225..315 => Edge::Bottom,
        _ => Edge::Left,
    }
}

/// The transition kind an FCP effect name stands for.
fn kind(name: &str, angle: Option<i64>, color: Option<[u8; 3]>) -> TransitionKind {
    let name = name.to_ascii_lowercase();
    let from = edge(angle.unwrap_or(0));
    if name.contains("clock") {
        TransitionKind::Clock
    } else if name.contains("iris") {
        TransitionKind::Iris
    } else if name.contains("slide") || name.contains("push") {
        TransitionKind::Slide { from }
    } else if name.contains("wipe") {
        TransitionKind::Wipe { from }
    } else if name.contains("dip") || name.contains("fade") {
        TransitionKind::FadeToColor {
            color: color.unwrap_or([0; 3]),
        }
    } else {
        TransitionKind::Dissolve
    }
}

/// Writes `<file>` once per media path, then refers to it by ID.
#[derive(Default)]
struct Files {
    ids: HashMap<String, usize>,
}

impl Files {
    fn element(&mut self, media: &str, name: &str, rate: FrameRate, audio: bool) -> String {
        if media.is_empty() {
            return String::new();
        }
        if let Some(id) = self.ids.get(media) {
            return format!("<file id=\"file-{id}\"/>");
        }
        let id = self.ids.len() + 1;
        self.ids.insert(media.to_string(), id);
        let file_name = media.rsplit(['/', '\\']).next().unwrap_or(name);
        let kind = if audio {
            "<audio><channelcount>2</channelcount></audio>"
        } else {
            "<video/>"
        };
        format!(
            "<file id=\"file-{id}\"><name>{}</name><pathurl>{}</pathurl>{}<media>{kind}</media></file>",
            esc(file_name),
            esc(&super::file_url(media)),
            rate_xml(rate)
        )
    }
}

fn clip_item(
    out: &mut String,
    id: &str,
    clip: &EditClip,
    rate: FrameRate,
    files: &mut Files,
    audio: bool,
) {
    let _ = write!(
        out,
        "<clipitem id=\"{id}\"><name>{}</name><enabled>TRUE</enabled><duration>{}</duration>{}<start>{}</start><end>{}</end><in>{}</in><out>{}</out>{}",
        esc(&clip.name),
        clip.source_out.max(clip.source_in + clip.frames()),
        rate_xml(rate),
        clip.record_in,
        clip.record_out,
        clip.source_in,
        clip.source_in + clip.frames(),
        files.element(&clip.media, &clip.name, rate, audio),
    );
    if audio {
        out.push_str(
            "<sourcetrack><mediatype>audio</mediatype><trackindex>1</trackindex></sourcetrack>",
        );
        if let Some(db) = clip.gain_db {
            let level = 10f64.powf(f64::from(db) / 20.);
            let _ = write!(
                out,
                "<filter><effect><name>Audio Levels</name><effectid>audiolevels</effectid><effectcategory>audiolevels</effectcategory><effecttype>audiolevels</effecttype><mediatype>audio</mediatype><parameter><parameterid>level</parameterid><name>Level</name><valuemin>0</valuemin><valuemax>3.98109</valuemax><value>{level:.6}</value></parameter></effect></filter>"
            );
        }
    }
    out.push_str("</clipitem>");
}

/// Write `edit` as an xmeml document; `size` is the picture size.
pub fn write(edit: &Edit, size: (u32, u32)) -> String {
    let rate = edit.rate;
    let mut files = Files::default();
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE xmeml>\n");
    let _ = write!(
        out,
        "<xmeml version=\"5\"><sequence id=\"sequence-1\"><name>{}</name><duration>{}</duration>{}",
        esc(&edit.name),
        edit.end(),
        rate_xml(rate)
    );
    let _ = write!(
        out,
        "<timecode>{}<string>{}</string><frame>{}</frame><displayformat>{}</displayformat></timecode>",
        rate_xml(rate),
        rate.timecode(edit.start),
        edit.start,
        if rate.drop_frame() { "DF" } else { "NDF" }
    );
    let _ = write!(
        out,
        "<media><video><format><samplecharacteristics>{}<width>{}</width><height>{}</height><pixelaspectratio>square</pixelaspectratio></samplecharacteristics></format>",
        rate_xml(rate),
        size.0,
        size.1
    );
    let video_tracks = edit.video.iter().map(|c| c.track + 1).max().unwrap_or(0);
    let mut n = 0;
    for track in 0..video_tracks {
        out.push_str("<track>");
        let clips: Vec<_> = edit.video.iter().filter(|c| c.track == track).collect();
        for (i, clip) in clips.iter().enumerate() {
            if let (Some(t), Some(previous)) = (clip.transition, i.checked_sub(1).map(|p| clips[p]))
                && previous.record_out == clip.record_in
                && t.frames > 0
            {
                let (name, category, angle) = effect(t.kind);
                let _ = write!(
                    out,
                    "<transitionitem>{}<start>{}</start><end>{}</end><alignment>start</alignment><effect><name>{name}</name><effectid>{name}</effectid><effectcategory>{category}</effectcategory><effecttype>transition</effecttype><mediatype>video</mediatype>",
                    rate_xml(rate),
                    clip.record_in,
                    clip.record_in + u64::from(t.frames),
                );
                if let Some(angle) = angle {
                    let _ = write!(
                        out,
                        "<parameter><parameterid>angle</parameterid><name>Angle</name><value>{angle}</value></parameter>"
                    );
                }
                if let TransitionKind::FadeToColor { color: [r, g, b] } = t.kind {
                    let _ = write!(
                        out,
                        "<parameter><parameterid>color</parameterid><name>Color</name><value><alpha>255</alpha><red>{r}</red><green>{g}</green><blue>{b}</blue></value></parameter>"
                    );
                }
                out.push_str("</effect></transitionitem>");
            }
            n += 1;
            clip_item(
                &mut out,
                &format!("clipitem-{n}"),
                clip,
                rate,
                &mut files,
                false,
            );
        }
        out.push_str("</track>");
    }
    out.push_str("</video><audio>");
    let audio_tracks = edit.audio.iter().map(|c| c.track + 1).max().unwrap_or(0);
    for track in 0..audio_tracks {
        out.push_str("<track>");
        for clip in edit.audio.iter().filter(|c| c.track == track) {
            n += 1;
            clip_item(
                &mut out,
                &format!("clipitem-{n}"),
                clip,
                rate,
                &mut files,
                true,
            );
        }
        out.push_str("</track>");
    }
    out.push_str("</audio></media>");
    for marker in &edit.markers {
        let _ = write!(
            out,
            "<marker><name>{}</name><comment></comment><in>{}</in><out>-1</out></marker>",
            esc(&marker.name),
            marker.frame
        );
    }
    out.push_str("</sequence></xmeml>\n");
    out
}

fn number(node: Option<&Xml>) -> Option<i64> {
    node.and_then(|n| n.text.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .map(|v| v.round() as i64)
}

fn read_rate(node: Option<&Xml>) -> Option<FrameRate> {
    let rate = node?;
    let base = number(rate.child("timebase"))?;
    if !(1..=120).contains(&base) {
        return None;
    }
    let ntsc = rate
        .child("ntsc")
        .is_some_and(|n| n.text.trim().eq_ignore_ascii_case("true"));
    Some(if ntsc {
        FrameRate::ntsc(base as u32)
    } else {
        FrameRate::whole(base as u32)
    })
}

/// Drop the DOCTYPE line FCP writes (the shared reader refuses DTDs).
fn without_doctype(text: &str) -> Result<String> {
    let Some(at) = text.find("<!DOCTYPE") else {
        return Ok(text.to_string());
    };
    let end = text[at..].find('>').context("Unclosed DOCTYPE")? + at;
    if text[at..end].contains('[') {
        bail!("Document type declarations with entities are not supported")
    }
    Ok(format!("{}{}", &text[..at], &text[end + 1..]))
}

/// One track's items in order: clips and transitions.
enum Item<'a> {
    Clip(&'a Xml),
    Transition(&'a Xml),
}

/// Read an xmeml document's first sequence.
pub fn read(text: &str) -> Result<Edit> {
    let root = xml::parse(&without_doctype(text)?)?;
    if root.name != "xmeml" {
        bail!("This is not an xmeml XML edit list")
    }
    let sequence = root
        .descendants("sequence")
        .next()
        .context("The XML file has no sequence")?;
    let rate = read_rate(sequence.child("rate")).context("The sequence has no frame rate")?;
    let mut edit = Edit::new(sequence.child("name").map_or("", |n| n.text.trim()), rate);
    if let Some(tc) = sequence.child("timecode") {
        edit.start = number(tc.child("frame"))
            .filter(|f| *f >= 0)
            .map(|f| f as u64)
            .or_else(|| {
                tc.child("string")
                    .and_then(|s| rate.parse_timecode(s.text.trim()))
            })
            .unwrap_or(0);
    }
    // Files by ID, so later references find their path.
    let mut paths: HashMap<String, String> = HashMap::new();
    for file in root.descendants("file") {
        if let Some(url) = file.child("pathurl") {
            paths.insert(
                file.attr("id").to_string(),
                super::url_path(url.text.trim()),
            );
        }
    }
    let media = sequence.child("media");
    for (audio, kind) in [(false, "video"), (true, "audio")] {
        let Some(section) = media.and_then(|m| m.child(kind)) else {
            continue;
        };
        for (track_index, track) in section.children("track").enumerate() {
            let items: Vec<Item> = track
                .children
                .iter()
                .filter_map(|c| match c.name.as_str() {
                    "clipitem" => Some(Item::Clip(c)),
                    "transitionitem" => Some(Item::Transition(c)),
                    _ => None,
                })
                .collect();
            for (i, item) in items.iter().enumerate() {
                let Item::Clip(node) = item else {
                    continue;
                };
                if node
                    .child("enabled")
                    .is_some_and(|e| e.text.trim().eq_ignore_ascii_case("false"))
                {
                    continue;
                }
                let before = i.checked_sub(1).and_then(|p| match items[p] {
                    Item::Transition(t) => Some(t),
                    _ => None,
                });
                let after = match items.get(i + 1) {
                    Some(Item::Transition(t)) => Some(*t),
                    _ => None,
                };
                if let Some(clip) = read_clip(node, before, after, &paths, audio, track_index) {
                    if audio {
                        edit.audio.push(clip);
                    } else {
                        edit.video.push(clip);
                    }
                }
            }
        }
    }
    for marker in sequence.children("marker") {
        if let Some(frame) = number(marker.child("in")).filter(|f| *f >= 0) {
            edit.markers.push(EditMarker {
                frame: frame as u64,
                name: marker.child("name").map_or("", |n| n.text.trim()).into(),
            });
        }
    }
    edit.sort();
    Ok(edit)
}

/// The cut a transition stands on: its start for start-aligned ones,
/// its middle when centred, its end when end-aligned.
fn cut(transition: &Xml) -> Option<i64> {
    let start = number(transition.child("start"))?;
    let end = number(transition.child("end"))?;
    Some(
        match transition
            .child("alignment")
            .map(|a| a.text.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("start") | Some("start-black") => start,
            Some("end") | Some("end-black") => end,
            _ => (start + end) / 2,
        },
    )
}

fn read_clip(
    node: &Xml,
    before: Option<&Xml>,
    after: Option<&Xml>,
    paths: &HashMap<String, String>,
    audio: bool,
    track: usize,
) -> Option<EditClip> {
    let mut start = number(node.child("start"))?;
    let mut end = number(node.child("end"))?;
    if start < 0 {
        start = cut(before?)?;
    }
    if end < 0 {
        end = cut(after?)?;
    }
    if end <= start || start < 0 {
        return None;
    }
    let source_in = number(node.child("in")).unwrap_or(0).max(0) as u64;
    let source_out = number(node.child("out"))
        .filter(|o| *o >= 0)
        .map_or(source_in + (end - start) as u64, |o| o as u64);
    let media = node
        .child("file")
        .and_then(|f| {
            f.child("pathurl")
                .map(|u| super::url_path(u.text.trim()))
                .or_else(|| paths.get(f.attr("id")).cloned())
        })
        .unwrap_or_default();
    let transition = before.filter(|_| !audio).and_then(|t| {
        let tstart = number(t.child("start"))?;
        let tend = number(t.child("end"))?;
        let effect = t.child("effect")?;
        let name = effect
            .child("effectid")
            .or_else(|| effect.child("name"))
            .map_or("", |n| n.text.trim());
        let parameter = |id: &str| {
            effect
                .children("parameter")
                .find(|p| p.child("parameterid").is_some_and(|i| i.text.trim() == id))
        };
        let angle = number(parameter("angle").and_then(|p| p.child("value")));
        let color = parameter("color").and_then(|p| p.child("value")).map(|v| {
            let c = |k: &str| number(v.child(k)).unwrap_or(0).clamp(0, 255) as u8;
            [c("red"), c("green"), c("blue")]
        });
        // Frames of the transition over this clip.
        let frames = (tend - tstart.max(start)).max(0) as u32;
        (frames > 0).then(|| EditTransition {
            kind: kind(name, angle, color),
            frames,
        })
    });
    let gain_db = node
        .descendants("parameter")
        .find(|p| {
            p.child("parameterid")
                .is_some_and(|i| i.text.trim().eq_ignore_ascii_case("level"))
        })
        .and_then(|p| p.child("value"))
        .and_then(|v| v.text.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v > 0.)
        .map(|v| ((20. * v.log10()) * 100.).round() as f32 / 100.);
    Some(EditClip {
        name: node.child("name").map_or("", |n| n.text.trim()).into(),
        media,
        track,
        source_in,
        source_out: source_out.max(source_in),
        record_in: start as u64,
        record_out: end as u64,
        transition,
        gain_db: if audio { gain_db } else { None },
    })
}
