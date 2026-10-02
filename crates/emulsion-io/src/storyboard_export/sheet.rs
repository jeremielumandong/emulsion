//! Storyboard pages: panels in a grid with their headers and captions, a page
//! header with an optional logo, and a footer. Pages are ordinary print
//! sheets, so preview, Save PDF and printing all use the print pipeline.
use super::{
    Alignment, CaptionPlacement, Entry, Fit, Profile, Scope, entries, expand, panel_token, select,
};
use crate::printing::{self, JobLayout, Layout, Mark, Placement, Rect, Sheet, Source};
use anyhow::{Context, Result, bail};
use emulsion_core::{
    project::Project,
    storyboard::{Caption, CaptionField, CaptionId, FrameRate, StageGuides},
    text::{TextRun, TextSpec},
};
use std::{
    io::Read,
    sync::{Arc, atomic::AtomicBool},
};

/// Most pages one board may print.
pub const MAX_PAGES: usize = 1000;
const MM_PER_PT: f64 = 25.4 / 72.;
/// Space between bands and between a panel and its captions.
const GAP_MM: f64 = 2.;

/// Text line height for a size in points, in millimetres.
fn line_mm(pt: f64) -> f64 {
    pt * MM_PER_PT * 1.45
}

/// The storyboard data a layout prints, read once from the project.
#[derive(Clone, Debug)]
pub struct Job {
    pub project: String,
    pub date: String,
    pub rate: FrameRate,
    /// Panel size in pixels.
    pub size: (u32, u32),
    pub stage: StageGuides,
    /// The board's caption fields.
    pub captions: Vec<CaptionField>,
    pub entries: Vec<Entry>,
    pub logo: Option<Logo>,
}

#[derive(Clone, Debug)]
pub struct Logo {
    pub data: Arc<Vec<u8>>,
    pub mime: &'static str,
    /// Width divided by height.
    pub aspect: f64,
}

impl Logo {
    /// Read a PNG or JPEG logo, up to 10 MiB.
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .with_context(|| format!("Cannot open the logo {}", path.display()))?
            .take(10 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 10 * 1024 * 1024 {
            bail!("Logos are limited to 10 MiB")
        }
        let mime = match image::guess_format(&bytes) {
            Ok(image::ImageFormat::Png) => "image/png",
            Ok(image::ImageFormat::Jpeg) => "image/jpeg",
            _ => bail!("Use a PNG or JPEG logo"),
        };
        let size = image::ImageReader::new(std::io::Cursor::new(&bytes))
            .with_guessed_format()?
            .into_dimensions()?;
        if size.0 == 0 || size.1 == 0 {
            bail!("The logo has no pixels")
        }
        Ok(Self {
            data: Arc::new(bytes),
            mime,
            aspect: f64::from(size.0) / f64::from(size.1),
        })
    }
}

impl Job {
    pub fn new(
        project: &Project,
        name: &str,
        scope: &Scope,
        logo: Option<&std::path::Path>,
        date: String,
    ) -> Result<Self> {
        let board = super::board(project)?;
        Ok(Self {
            project: name.into(),
            date,
            rate: board.settings.frame_rate,
            size: (board.settings.width, board.settings.height),
            stage: board.stage.clone(),
            captions: board.captions.clone(),
            entries: select(entries(project)?, scope)?,
            logo: logo.map(Logo::load).transpose()?,
        })
    }

    /// The caption fields `profile` prints, in board order of its list, with
    /// a warning for each listed name this board lacks.
    pub fn fields(&self, profile: &Profile) -> (Vec<(String, CaptionId)>, Vec<String>) {
        if profile.caption_fields.is_empty() {
            let fields = self
                .captions
                .iter()
                .filter(|f| f.print)
                .map(|f| (f.name.clone(), f.id))
                .collect();
            return (fields, Vec::new());
        }
        let mut fields = Vec::new();
        let mut warnings = Vec::new();
        for wanted in &profile.caption_fields {
            match self
                .captions
                .iter()
                .find(|f| f.name.eq_ignore_ascii_case(wanted.trim()))
            {
                Some(f) => fields.push((f.name.clone(), f.id)),
                None => warnings.push(format!(
                    "This storyboard has no “{}” caption field; it is left out.",
                    wanted.trim()
                )),
            }
        }
        (fields, warnings)
    }
}

/// One panel's place on a page.
#[derive(Clone, Copy, Debug)]
pub struct Cell {
    /// The first and second panel header lines.
    pub headers: [Option<Rect>; 2],
    pub image: Rect,
    pub caption: Option<Rect>,
}

/// Where everything goes on every page of a profile.
#[derive(Clone, Debug)]
pub struct Page {
    pub width: f64,
    pub height: f64,
    pub printable: Rect,
    pub header: Option<Rect>,
    pub footer: Option<Rect>,
    pub cells: Vec<Cell>,
}

/// Page geometry for `profile`, in millimetres from the page's top left.
pub fn page(profile: &Profile, logo: bool) -> Result<Page> {
    profile.validate()?;
    let (width, height) = profile.page_size();
    let p = profile.paper.margins;
    let m = if profile.landscape {
        [p[3], p[0], p[1], p[2]]
    } else {
        p
    };
    let margin = profile.margin_mm;
    let printable = Rect {
        x: m[3] + margin,
        y: m[0] + margin,
        w: width - m[1] - m[3] - 2. * margin,
        h: height - m[0] - m[2] - 2. * margin,
    };
    let (mut top, mut bottom) = (printable.y, printable.y + printable.h);
    let header = (!profile.page_header.trim().is_empty() || logo).then(|| {
        let mut h = line_mm(profile.page_text_pt);
        if logo {
            h = h.max(profile.logo_height_mm);
        }
        top += h + GAP_MM;
        Rect {
            y: printable.y,
            h,
            ..printable
        }
    });
    let footer = (!profile.page_footer.trim().is_empty()).then(|| {
        let h = line_mm(profile.page_text_pt);
        bottom -= h + GAP_MM;
        Rect {
            y: bottom + GAP_MM,
            h,
            ..printable
        }
    });
    let (cols, rows) = (f64::from(profile.columns), f64::from(profile.rows));
    let gutter = profile.gutter_mm;
    let cw = (printable.w - gutter * (cols - 1.)) / cols;
    let ch = (bottom - top - gutter * (rows - 1.)) / rows;
    let line = line_mm(profile.panel_header_pt);
    let lines = [&profile.panel_header, &profile.second_panel_header]
        .map(|pattern| !pattern.trim().is_empty());
    let header_h = lines.iter().filter(|on| **on).count() as f64 * line;
    let too_small = || {
        anyhow::anyhow!(
            "Panels do not fit: use fewer panels per page, smaller margins or larger paper"
        )
    };
    if cw < 10. || ch - header_h < 10. {
        return Err(too_small());
    }
    let share = profile.caption_percent / 100.;
    let mut cells = Vec::new();
    for row in 0..profile.rows {
        for col in 0..profile.columns {
            let cell = Rect {
                x: printable.x + f64::from(col) * (cw + gutter),
                y: top + f64::from(row) * (ch + gutter),
                w: cw,
                h: ch,
            };
            let mut y = cell.y;
            let headers = lines.map(|on| {
                on.then(|| {
                    y += line;
                    Rect {
                        y: y - line,
                        h: line,
                        ..cell
                    }
                })
            });
            let body = Rect {
                y: cell.y + header_h,
                h: cell.h - header_h,
                ..cell
            };
            let (image, caption) = match profile.captions {
                CaptionPlacement::None => (body, None),
                CaptionPlacement::Below => {
                    let h = body.h * (1. - share);
                    (
                        Rect { h, ..body },
                        Some(Rect {
                            y: body.y + h + GAP_MM,
                            h: body.h - h - GAP_MM,
                            ..body
                        }),
                    )
                }
                CaptionPlacement::Right => {
                    let w = body.w * (1. - share);
                    (
                        Rect { w, ..body },
                        Some(Rect {
                            x: body.x + w + GAP_MM,
                            w: body.w - w - GAP_MM,
                            ..body
                        }),
                    )
                }
                CaptionPlacement::Left => {
                    let w = body.w * share;
                    (
                        Rect {
                            x: body.x + w + GAP_MM,
                            w: body.w - w - GAP_MM,
                            ..body
                        },
                        Some(Rect { w, ..body }),
                    )
                }
            };
            if image.w < 5. || image.h < 5. || caption.is_some_and(|c| c.w < 5. || c.h < 5.) {
                return Err(too_small());
            }
            cells.push(Cell {
                headers,
                image,
                caption,
            });
        }
    }
    Ok(Page {
        width,
        height,
        printable,
        header,
        footer,
        cells,
    })
}

/// Text of `pt` points; sheet text sizes are in tenths of a millimetre.
fn text(text: String, pt: f64, align: Alignment) -> TextSpec {
    TextSpec {
        text,
        size: (pt * MM_PER_PT * 10.) as f32,
        align: align.text(),
        ..Default::default()
    }
}

/// The printed captions of a panel as one text: each field on its own line,
/// optionally after its name in bold, keeping the caption's formatting.
pub fn captions(
    fields: &[(String, CaptionId)],
    captions: &std::collections::BTreeMap<CaptionId, Caption>,
    titles: bool,
    pt: f64,
) -> TextSpec {
    let mut spec = text(String::new(), pt, Alignment::Left);
    let scale = spec.size / Caption::base_style().size;
    let base = spec.base_style();
    for (name, id) in fields {
        let Some(caption) = captions.get(id).filter(|c| !c.text.trim().is_empty()) else {
            continue;
        };
        if !spec.text.is_empty() {
            spec.text.push('\n');
        }
        if titles {
            let start = spec.text.len();
            spec.text.push_str(name);
            spec.text.push_str(": ");
            let mut style = base.clone();
            style.bold = true;
            spec.runs.push(TextRun {
                start,
                end: spec.text.len(),
                style,
            });
        }
        let offset = spec.text.len();
        spec.text.push_str(&caption.text);
        spec.runs.extend(caption.runs.iter().map(|run| {
            let mut style = run.style.clone();
            style.size *= scale;
            style.letter_spacing *= scale;
            style.baseline *= scale;
            TextRun {
                start: run.start + offset,
                end: run.end + offset,
                style,
            }
        }));
    }
    spec
}

const INK: [u8; 3] = [0, 0, 0];
const RULE: [u8; 3] = [140, 140, 140];
const CAMERA: [u8; 3] = [220, 40, 40];
const SAFE: [u8; 3] = [60, 120, 220];

/// A camera move on a printed panel: the frame where it starts and where it
/// ends (corners in sheet millimetres) and an arrow between them, centre to
/// centre, or corner to corner when the camera only zooms or turns.
fn camera_move_marks(
    marks: &mut Vec<Mark>,
    frames: [[(f64, f64); 4]; 2],
    frame_mm: f64,
    arrow_mm: f64,
) {
    for corners in frames {
        marks.push(Mark::Path {
            points: corners.to_vec(),
            closed: true,
            filled: false,
            stroke_mm: frame_mm,
            color: CAMERA,
        });
    }
    let centre = |c: &[(f64, f64); 4]| ((c[0].0 + c[2].0) / 2., (c[0].1 + c[2].1) / 2.);
    let (mut a, mut b) = (centre(&frames[0]), centre(&frames[1]));
    let head = (arrow_mm * 4.).max(1.5);
    if (b.0 - a.0).hypot(b.1 - a.1) < head * 1.5 {
        (a, b) = (frames[0][0], frames[1][0]);
    }
    let length = (b.0 - a.0).hypot(b.1 - a.1);
    if length < 1e-6 {
        return;
    }
    let (ux, uy) = ((b.0 - a.0) / length, (b.1 - a.1) / length);
    let head = head.min(length * 0.6);
    let base = (b.0 - ux * head, b.1 - uy * head);
    marks.push(Mark::Path {
        points: vec![a, base],
        closed: false,
        filled: false,
        stroke_mm: arrow_mm,
        color: CAMERA,
    });
    let half = head * 0.45;
    marks.push(Mark::Path {
        points: vec![
            b,
            (base.0 - uy * half, base.1 + ux * half),
            (base.0 + uy * half, base.1 - ux * half),
        ],
        closed: true,
        filled: true,
        stroke_mm: arrow_mm / 2.,
        color: CAMERA,
    });
}

/// Lay out the `selected` entries of `job` (indexes, in print order) with
/// `profile`. `sources[i]` is the picture of `job.entries[i]`.
pub fn layout(
    job: &Job,
    sources: &[Source],
    selected: &[usize],
    profile: &Profile,
) -> Result<JobLayout> {
    if sources.len() != job.entries.len()
        || selected.is_empty()
        || selected.iter().any(|&i| i >= job.entries.len())
    {
        bail!("Choose at least one panel to print")
    }
    let geometry = page(profile, job.logo.is_some())?;
    let (fields, warnings) = job.fields(profile);
    let per_page = geometry.cells.len();
    let pages = selected.len().div_ceil(per_page);
    if pages > MAX_PAGES {
        bail!(
            "Storyboard PDFs are limited to {MAX_PAGES} pages; print fewer panels or more per page"
        )
    }
    let mut settings = printing::Settings {
        layout: Layout::Contact,
        placement: match profile.fit {
            Fit::Fit => Placement::Fit,
            Fit::Fill => Placement::Fill,
        },
        ..Default::default()
    };
    // A letterboxed picture sits against its captions; they take the room
    // it leaves.
    settings.creative.crop = match profile.captions {
        CaptionPlacement::None => [0.5, 0.5],
        CaptionPlacement::Below => [0.5, 0.],
        CaptionPlacement::Right => [0., 0.],
        CaptionPlacement::Left => [1., 0.],
    };
    let mut result = JobLayout {
        sheets: Vec::new(),
        warnings,
    };
    let patterns = [&profile.panel_header, &profile.second_panel_header];
    for (number, chunk) in selected.chunks(per_page).enumerate() {
        let mut sheet = Sheet {
            width: geometry.width,
            height: geometry.height,
            printable: geometry.printable,
            items: Vec::new(),
            marks: Vec::new(),
        };
        for (i, &source) in chunk.iter().enumerate() {
            let cell = geometry.cells[i];
            let entry = &job.entries[source];
            printing::place(
                &mut sheet,
                sources,
                source,
                cell.image,
                &settings,
                &mut result.warnings,
            )?;
            let item = sheet.items.last().context("Missing panel picture")?;
            let (picture, bounds) = (item.trim, item.bounds);
            let (w, h) = (f64::from(job.size.0), f64::from(job.size.1));
            if profile.safe_areas {
                for frame in job.stage.safe_areas(job.size.0, job.size.1) {
                    sheet.marks.push(Mark::Frame {
                        bounds: Rect {
                            x: bounds.x + frame.x / w * bounds.w,
                            y: bounds.y + frame.y / h * bounds.h,
                            w: frame.w / w * bounds.w,
                            h: frame.h / h * bounds.h,
                        },
                        stroke_mm: profile.camera_frame_mm / 2.,
                        color: SAFE,
                    });
                }
            }
            if profile.camera_frame {
                sheet.marks.push(Mark::Frame {
                    bounds: picture,
                    stroke_mm: profile.camera_frame_mm,
                    color: CAMERA,
                });
            }
            if let Some(frames) = &entry.camera_move {
                let on_page =
                    |(x, y): (f64, f64)| (bounds.x + x / w * bounds.w, bounds.y + y / h * bounds.h);
                camera_move_marks(
                    &mut sheet.marks,
                    frames.map(|f| f.map(on_page)),
                    profile.camera_frame_mm,
                    profile.camera_arrow_mm,
                );
            }
            if profile.panel_frame_mm > 0. {
                sheet.marks.push(Mark::Frame {
                    bounds: picture,
                    stroke_mm: profile.panel_frame_mm,
                    color: INK,
                });
            }
            for (rect, pattern) in cell.headers.iter().zip(patterns) {
                if let Some(rect) = rect {
                    let line = expand(pattern, |t| panel_token(entry, &job.project, job.rate, t))?;
                    sheet.marks.push(Mark::Text {
                        spec: Box::new(text(
                            line,
                            profile.panel_header_pt,
                            profile.panel_header_align,
                        )),
                        bounds: *rect,
                    });
                }
            }
            if let Some(rect) = cell.caption {
                let rect = match profile.captions {
                    CaptionPlacement::Below => {
                        let y = (picture.y + picture.h + GAP_MM).min(rect.y);
                        Rect {
                            y,
                            h: rect.y + rect.h - y,
                            ..rect
                        }
                    }
                    CaptionPlacement::Right => {
                        let x = (picture.x + picture.w + GAP_MM).min(rect.x);
                        Rect {
                            x,
                            w: rect.x + rect.w - x,
                            ..rect
                        }
                    }
                    CaptionPlacement::Left => Rect {
                        w: (picture.x - GAP_MM - rect.x).max(rect.w),
                        ..rect
                    },
                    CaptionPlacement::None => rect,
                };
                let spec = captions(
                    &fields,
                    &entry.panel.captions,
                    profile.caption_titles,
                    profile.caption_pt,
                );
                let inset = if profile.caption_frames {
                    sheet.marks.push(Mark::Frame {
                        bounds: rect,
                        stroke_mm: 0.2,
                        color: RULE,
                    });
                    1.5
                } else {
                    0.
                };
                if !spec.text.is_empty() {
                    sheet.marks.push(Mark::Text {
                        spec: Box::new(spec),
                        bounds: rect.inset(inset),
                    });
                }
            }
        }
        let first = &job.entries[chunk[0]];
        let page_value = |token: &str| -> Option<String> {
            Some(match token {
                "page" => (number + 1).to_string(),
                "pages" => pages.to_string(),
                "date" => job.date.clone(),
                "project" | "act" | "seq" | "scene" => {
                    panel_token(first, &job.project, job.rate, token)?
                }
                _ => return None,
            })
        };
        if let Some(mut band) = geometry.header {
            if let Some(logo) = &job.logo {
                let h = profile.logo_height_mm.min(band.h);
                let w = (h * logo.aspect).min(band.w / 3.);
                let x = match profile.logo_align {
                    Alignment::Left => band.x,
                    Alignment::Center => band.x + (band.w - w) / 2.,
                    Alignment::Right => band.x + band.w - w,
                };
                sheet.marks.push(Mark::Image {
                    data: logo.data.clone(),
                    mime: logo.mime,
                    bounds: Rect { x, y: band.y, w, h },
                });
                // Keep the header text clear of the logo.
                match profile.logo_align {
                    Alignment::Left => {
                        band.x += w + GAP_MM;
                        band.w -= w + GAP_MM;
                    }
                    Alignment::Right => band.w -= w + GAP_MM,
                    Alignment::Center => {}
                }
            }
            let line = expand(&profile.page_header, page_value)?;
            if !line.trim().is_empty() {
                sheet.marks.push(Mark::Text {
                    spec: Box::new(text(line, profile.page_text_pt, profile.page_header_align)),
                    bounds: Rect {
                        h: line_mm(profile.page_text_pt),
                        ..band
                    },
                });
            }
        }
        if let Some(band) = geometry.footer {
            let line = expand(&profile.page_footer, page_value)?;
            sheet.marks.push(Mark::Text {
                spec: Box::new(text(line, profile.page_text_pt, profile.page_footer_align)),
                bounds: band,
            });
        }
        result.sheets.push(sheet);
    }
    Ok(result)
}

/// The pictures of a job's panels, ready for the print pipeline.
pub fn sources(project: &Project, job: &Job, cancel: &AtomicBool) -> Result<Vec<Source>> {
    let docs = job
        .entries
        .iter()
        .map(|e| {
            let page = project
                .pages
                .iter()
                .find(|p| p.meta.id == e.page)
                .context("Missing panel page")?;
            Ok((e.name.clone(), page.doc.clone()))
        })
        .collect::<Result<Vec<_>>>()?;
    printing::prepare_sources(docs, cancel)
}

/// Write a storyboard PDF. Returns the number of pages.
pub fn write_pdf(
    project: &Project,
    name: &str,
    scope: &Scope,
    profile: &Profile,
    path: &std::path::Path,
    cancel: &AtomicBool,
) -> Result<usize> {
    profile.validate()?;
    let job = Job::new(
        project,
        name,
        scope,
        profile.logo.as_deref(),
        super::today(),
    )?;
    let sources = sources(project, &job, cancel)?;
    let all: Vec<_> = (0..job.entries.len()).collect();
    let layout = layout(&job, &sources, &all, profile)?;
    printing::write_pdf(&sources, &layout, false, path, cancel)?;
    Ok(layout.sheets.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storyboard_export::{profile::builtins, tests::project};

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn grid_maths_places_captions_right_below_and_left() {
        let mut p = builtins().remove(0);
        p.paper.margins = [0.; 4];
        let page = page(&p, false).unwrap();
        assert_eq!(page.cells.len(), 3);
        assert!(close(page.width, 210.) && close(page.height, 297.));
        let cell = page.cells[0];
        let caption = cell.caption.unwrap();
        // Captions right: 60% picture, then a gap and 40% captions.
        assert!(close(cell.image.x, 10.));
        assert!(close(caption.x, cell.image.x + cell.image.w + GAP_MM));
        assert!(close(caption.x + caption.w, 200.));
        assert!(close(cell.image.y, caption.y));
        // Rows are stacked with the gutter, under the page header.
        let header = page.header.unwrap();
        assert!(close(
            cell.headers[0].unwrap().y,
            header.y + header.h + GAP_MM
        ));
        let step = page.cells[1].image.y - cell.image.y;
        assert!(close(page.cells[2].image.y - page.cells[1].image.y, step));
        assert!(close(
            step - p.gutter_mm,
            page.cells[1].headers[0].unwrap().y - cell.headers[0].unwrap().y - p.gutter_mm
        ));
        assert!(page.cells[2].image.y + page.cells[2].image.h <= page.footer.unwrap().y);
        p.captions = CaptionPlacement::Below;
        let below = super::page(&p, false).unwrap().cells[0];
        let caption = below.caption.unwrap();
        assert!(close(caption.y, below.image.y + below.image.h + GAP_MM));
        assert!(close(caption.x, below.image.x));
        p.captions = CaptionPlacement::Left;
        let left = super::page(&p, false).unwrap().cells[0];
        assert!(left.caption.unwrap().x < left.image.x);
        p.captions = CaptionPlacement::None;
        p.second_panel_header = "{duration}".into();
        let none = super::page(&p, false).unwrap().cells[0];
        assert!(none.caption.is_none() && none.headers[1].is_some());
        p.rows = 8;
        p.panel_header_pt = 36.;
        assert!(super::page(&p, false).is_err());
    }

    #[test]
    fn six_per_page_paginates_in_board_order() {
        let p = builtins().remove(1);
        let page = page(&p, false).unwrap();
        assert_eq!(page.cells.len(), 6);
        assert!(close(page.cells[1].image.y, page.cells[0].image.y));
        assert!(page.cells[3].image.y > page.cells[0].image.y);
    }

    #[test]
    fn captions_keep_their_formatting_with_bold_titles() {
        let project = project();
        let job = Job::new(&project, "Film", &Scope::All, None, "2026-10-01".into()).unwrap();
        let (fields, warnings) = job.fields(&Profile::default());
        assert!(warnings.is_empty());
        let spec = captions(&fields, &job.entries[0].panel.captions, true, 10.);
        assert_eq!(
            spec.text,
            "Action: Mia runs, \"fast\"\nDialogue: Wait,\nfor me!"
        );
        assert!(spec.style_at(0).bold, "title");
        assert!(spec.style_at(8).bold, "bold caption run");
        assert!(!spec.style_at(13).bold);
        assert!((spec.style_at(8).size - spec.size).abs() < 0.01);
        let plain = captions(&fields, &job.entries[0].panel.captions, false, 10.);
        assert!(plain.text.starts_with("Mia runs"));
        let only = Profile {
            caption_fields: vec!["dialogue".into(), "Mood".into()],
            ..Default::default()
        };
        let (fields, warnings) = job.fields(&only);
        assert_eq!(fields.len(), 1);
        assert!(warnings[0].contains("Mood"));
    }

    #[test]
    fn pdf_has_one_page_per_grid_and_draws_headers_and_frames() {
        let project = project();
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let logo = dir.path().join("logo.png");
        image::RgbaImage::from_pixel(40, 20, image::Rgba([0, 0, 255, 255]))
            .save(&logo)
            .unwrap();
        let mut profile = builtins().remove(0);
        profile.camera_frame = true;
        profile.safe_areas = true;
        let job = Job::new(
            &project,
            "Film",
            &Scope::All,
            Some(&logo),
            "2026-10-01".into(),
        )
        .unwrap();
        profile.logo = Some(logo);
        let sources = sources(&project, &job, &cancel).unwrap();
        assert!(layout(&job, &sources, &[], &profile).is_err());
        assert!(layout(&job, &sources, &[3], &profile).is_err());
        let layout = layout(&job, &sources, &[0, 1, 2], &profile).unwrap();
        assert_eq!(layout.sheets.len(), 1);
        let sheet = &layout.sheets[0];
        assert_eq!(sheet.items.len(), 3);
        let texts: Vec<_> = sheet
            .marks
            .iter()
            .filter_map(|m| match m {
                Mark::Text { spec, .. } => Some(spec.text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            texts.contains(&"Scene 1 · Panel 1".to_string()),
            "{texts:?}"
        );
        assert!(texts.contains(&"Scene 2 · Panel 3".to_string()));
        assert!(texts.contains(&"Film".to_string()));
        assert!(texts.contains(&"Page 1 of 1 · 2026-10-01".to_string()));
        assert!(sheet.marks.iter().any(|m| matches!(m, Mark::Image { .. })));
        let frames = sheet
            .marks
            .iter()
            .filter(|m| matches!(m, Mark::Frame { color, .. } if *color == CAMERA))
            .count();
        assert_eq!(frames, 3);
        let preview = printing::preview(&sources, sheet, false, 600).unwrap();
        assert!(
            preview.pixels().any(|p| p.0[2] > 200 && p.0[0] < 50),
            "logo"
        );

        let path = dir.path().join("board.pdf");
        let six = builtins().remove(1);
        assert_eq!(
            write_pdf(&project, "Film", &Scope::All, &six, &path, &cancel).unwrap(),
            1
        );
        let one = builtins().remove(2);
        assert_eq!(
            write_pdf(&project, "Film", &Scope::All, &one, &path, &cancel).unwrap(),
            3
        );
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        assert!(String::from_utf8_lossy(&bytes).contains("/Count 3"));
        let scene = job.entries[2].scene_id;
        assert_eq!(
            write_pdf(&project, "Film", &Scope::Scene(scene), &one, &path, &cancel).unwrap(),
            1
        );
    }

    #[test]
    fn moving_cameras_print_start_and_end_frames_with_an_arrow() {
        use emulsion_core::storyboard::{CameraKey, CameraState, SceneCamera, Shake};
        let mut project = project();
        let board = project.storyboard.as_mut().unwrap();
        let first = project.pages[0].meta.id;
        let scene = board.panels[&first].scene;
        let rest = board.rest_camera();
        // Panel 1 (frames 0–47) pushes in on the right; panel 2 holds.
        board.cameras.insert(
            scene,
            SceneCamera {
                keys: vec![
                    CameraKey::at(0, rest),
                    CameraKey::at(
                        47,
                        CameraState {
                            x: 48.,
                            zoom: 2.,
                            ..rest
                        },
                    ),
                ],
                shake: Some(Shake::PRESETS[0].1),
            },
        );
        let job = Job::new(&project, "Film", &Scope::All, None, "2026-10-01".into()).unwrap();
        let moves: Vec<_> = job
            .entries
            .iter()
            .map(|e| e.camera_move.is_some())
            .collect();
        assert_eq!(moves, [true, false, false], "shake alone is not a move");
        let [start, end] = job.entries[0].camera_move.unwrap();
        assert_eq!(start[2], (64., 36.));
        assert!((end[0].0 - 32.).abs() < 1e-9 && (end[2].1 - 27.).abs() < 1e-9);
        let cancel = AtomicBool::new(false);
        let sources = sources(&project, &job, &cancel).unwrap();
        let mut profile = builtins().remove(0);
        profile.camera_arrow_mm = 0.8;
        let layout = layout(&job, &sources, &[0, 1, 2], &profile).unwrap();
        let paths: Vec<_> = layout.sheets[0]
            .marks
            .iter()
            .filter_map(|m| match m {
                Mark::Path {
                    points,
                    closed,
                    filled,
                    stroke_mm,
                    color,
                } if *color == CAMERA => Some((points.len(), *closed, *filled, *stroke_mm)),
                _ => None,
            })
            .collect();
        assert_eq!(
            paths,
            [
                (4, true, false, profile.camera_frame_mm),
                (4, true, false, profile.camera_frame_mm),
                (2, false, false, 0.8),
                (3, true, true, 0.4),
            ]
        );
        // The arrow runs from the first frame's centre to the second's.
        let item = &layout.sheets[0].items[0].bounds;
        let Mark::Path { points, .. } = layout.sheets[0]
            .marks
            .iter()
            .find(|m| matches!(m, Mark::Path { points, .. } if points.len() == 2))
            .unwrap()
        else {
            unreachable!()
        };
        assert!((points[0].0 - (item.x + item.w / 2.)).abs() < 1e-6);
        assert!(points[1].0 > points[0].0);
        // It draws.
        let preview = printing::preview(&sources, &layout.sheets[0], false, 600).unwrap();
        assert!(preview.pixels().any(|p| p.0[0] > 200 && p.0[1] < 80));
    }
}
