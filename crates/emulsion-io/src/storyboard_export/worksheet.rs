//! Paper worksheets: empty storyboard frames to draw on by hand, for chosen
//! panels or for new ones, at the board's aspect ratio. Each sheet carries
//! four corner marks and a QR code naming the storyboard, the sheet and
//! where each frame is, so a photo or scan of the drawn sheet comes back
//! onto the right panels (see `worksheet_scan`). Sheets are ordinary print
//! sheets laid out by a storyboard profile, so the PDF, printing and the
//! preview all use the print pipeline.
use super::sheet::{GAP_MM, caption_beside, line_mm, picture_alignment, text};
use super::{Alignment, Entry, Profile, Scope, entries, select};
use crate::printing::{self, JobLayout, Mark, Rect, Sheet};
use anyhow::{Context, Result, bail};
use emulsion_core::project::{PageId, Project};
use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicBool;

/// The first field of every worksheet code.
const TAG: &str = "EMW1";
/// Most frames one sheet holds.
pub const MAX_FRAMES: usize = 12;
/// Most new panels one print offers.
pub const MAX_NEW_PANELS: u32 = 200;
/// Most sheets one print makes.
pub const MAX_SHEETS: usize = 200;
/// Side of a corner mark, in millimetres: a thin black square ring around
/// a black centre (10 : 8 : 3), unlike a QR finder (7 : 5 : 3) so neither
/// is taken for the other.
pub const FIDUCIAL_MM: f64 = 8.;
/// Space between the corner marks and the frames.
const FIDUCIAL_GAP_MM: f64 = 3.;
/// The orientation mark: a solid square on the line between the top
/// corner marks, next to the top-left one, so a sheet whose code cannot be
/// read still shows which way up it is. Centre in the code's axes, side.
/// Sheets printed before it existed are turned by their frames alone.
pub const ORIENTATION_MARK: ((f64, f64), f64) = ((FIDUCIAL_MM * 1.5 + 2., 0.), 4.);
/// Preferred and smallest QR module sizes, in millimetres.
const MODULE_MM: f64 = 0.6;
const MIN_MODULE_MM: f64 = 0.45;
/// Largest QR side, quiet zone included (the profile's logo limit).
const MAX_CODE_MM: f64 = 40.;
const QUIET_MODULES: usize = 4;
/// Lengths in the code are tenths of a millimetre, four digits.
const MAX_LENGTH_MM: f64 = 999.9;
/// Space between caption rules.
const RULE_STEP_MM: f64 = 6.;
const INK: [u8; 3] = [0, 0, 0];
const FRAME: [u8; 3] = [90, 90, 90];
const RULE: [u8; 3] = [200, 200, 200];
const WHITE: [u8; 3] = [255, 255, 255];

/// What a frame on a sheet is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    /// An existing panel.
    Panel(PageId),
    /// The n-th new panel of the print, from 1; imports add them in order.
    New(u32),
}

/// What a worksheet print covers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Panels {
    /// Existing panels, labelled with their scene and number.
    Existing(Scope),
    /// This many empty frames that become new panels.
    New(u32),
}

/// Everything a sheet's QR code says. Lengths are millimetres from the
/// centre of the top-left corner mark, in the sheet's own axes.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetCode {
    /// The storyboard's project ID, in capitals.
    pub project: String,
    /// This sheet: the print's batch ID and the page number.
    pub sheet: String,
    /// From the top-left corner mark's centre to the bottom-right one's.
    pub marks: (f64, f64),
    /// The QR code's square (without its quiet zone): x, y and side.
    pub code: (f64, f64, f64),
    /// Width and height of every frame.
    pub frame: (f64, f64),
    /// Each frame's slot and top-left corner.
    pub frames: Vec<(Slot, (f64, f64))>,
}

/// A storyboard's project ID as codes carry it: capitals, digits and
/// dashes, at most 64 characters.
pub fn project_key(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .map(|c| c.to_ascii_uppercase())
        .take(64)
        .collect()
}

fn tenths(mm: f64) -> Result<String> {
    if !mm.is_finite() || !(0. ..=MAX_LENGTH_MM).contains(&mm) {
        bail!("Worksheets are limited to paper up to 999 mm")
    }
    Ok(format!("{:04}", (mm * 10.).round() as u32))
}

fn length(text: &str) -> Result<f64> {
    if text.len() != 4 {
        bail!("bad length")
    }
    Ok(f64::from(text.parse::<u32>()?) / 10.)
}

impl SheetCode {
    /// The code's text: QR alphanumeric characters only (capitals, digits,
    /// `:` `/` `+` `-`), so the code stays small.
    pub fn encode(&self) -> Result<String> {
        let pair =
            |(a, b): (f64, f64)| -> Result<String> { Ok(format!("{}/{}", tenths(a)?, tenths(b)?)) };
        let frames = self
            .frames
            .iter()
            .map(|(slot, corner)| {
                let slot = match slot {
                    Slot::Panel(id) => id.to_string(),
                    Slot::New(n) => format!("N{n}"),
                };
                Ok(format!("{slot}/{}", pair(*corner)?))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(format!(
            "{TAG}:{}:{}:{}:{}/{}:{}:{}",
            project_key(&self.project),
            self.sheet,
            pair(self.marks)?,
            pair((self.code.0, self.code.1))?,
            tenths(self.code.2)?,
            pair(self.frame)?,
            frames.join("+")
        ))
    }

    /// Read a code's text; `None` when it is not a worksheet code.
    pub fn decode(text: &str) -> Option<Self> {
        let parts: Vec<_> = text.trim().split(':').collect();
        let [tag, project, sheet, marks, code, frame, frames] = parts.as_slice() else {
            return None;
        };
        if *tag != TAG || project.is_empty() || sheet.is_empty() {
            return None;
        }
        let numbers =
            |text: &str| -> Option<Vec<f64>> { text.split('/').map(|t| length(t).ok()).collect() };
        let pair = |text: &str| -> Option<(f64, f64)> {
            match numbers(text)?.as_slice() {
                [a, b] => Some((*a, *b)),
                _ => None,
            }
        };
        let [cx, cy, side] = numbers(code)?.as_slice().try_into().ok()?;
        let frames = frames
            .split('+')
            .map(|item| {
                let (slot, corner) = item.split_once('/')?;
                let slot = match slot.strip_prefix('N') {
                    Some(n) => Slot::New(n.parse().ok().filter(|n| *n > 0)?),
                    None => Slot::Panel(slot.parse().ok()?),
                };
                Some((slot, pair(corner)?))
            })
            .collect::<Option<Vec<_>>>()?;
        let code = Self {
            project: project.to_string(),
            sheet: sheet.to_string(),
            marks: pair(marks)?,
            code: (cx, cy, side),
            frame: pair(frame)?,
            frames,
        };
        let (w, h) = code.marks;
        let inside = |(x, y): (f64, f64), (fw, fh): (f64, f64)| {
            x >= 0. && y >= 0. && x + fw <= w && y + fh <= h
        };
        (w > 0.
            && h > 0.
            && code.frame.0 > 0.
            && code.frame.1 > 0.
            && !code.frames.is_empty()
            && code.frames.len() <= MAX_FRAMES
            && inside((cx, cy), (side, side))
            && code.frames.iter().all(|(_, c)| inside(*c, code.frame)))
        .then_some(code)
    }

    /// A frame's rectangle in the code's axes.
    pub fn frame_rect(&self, corner: (f64, f64)) -> Rect {
        Rect {
            x: corner.0,
            y: corner.1,
            w: self.frame.0,
            h: self.frame.1,
        }
    }
}

/// A sheet's QR modules, dark first: (side in modules, dark flags by row).
fn qr_modules(text: &str) -> Result<(usize, Vec<bool>)> {
    let code = qrcode::QrCode::with_error_correction_level(text, qrcode::EcLevel::M)
        .map_err(|e| anyhow::anyhow!("The worksheet code does not fit: {e}"))?;
    let dark = code
        .to_colors()
        .into_iter()
        .map(|c| c == qrcode::Color::Dark)
        .collect();
    Ok((code.width(), dark))
}

/// Draw a QR code: one filled path per run of dark modules in a row.
fn qr_marks(marks: &mut Vec<Mark>, text: &str, x: f64, y: f64, side: f64) -> Result<()> {
    let (n, dark) = qr_modules(text)?;
    let m = side / n as f64;
    for row in 0..n {
        let mut col = 0;
        while col < n {
            if !dark[row * n + col] {
                col += 1;
                continue;
            }
            let start = col;
            while col < n && dark[row * n + col] {
                col += 1;
            }
            let (x0, x1) = (x + start as f64 * m, x + col as f64 * m);
            let (y0, y1) = (y + row as f64 * m, y + (row + 1) as f64 * m);
            marks.push(Mark::Path {
                points: vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)],
                closed: true,
                filled: true,
                stroke_mm: 0.01,
                color: INK,
            });
        }
    }
    Ok(())
}

fn square(marks: &mut Vec<Mark>, (cx, cy): (f64, f64), side: f64, color: [u8; 3]) {
    let h = side / 2.;
    marks.push(Mark::Path {
        points: vec![
            (cx - h, cy - h),
            (cx + h, cy - h),
            (cx + h, cy + h),
            (cx - h, cy + h),
        ],
        closed: true,
        filled: true,
        stroke_mm: 0.01,
        color,
    });
}

/// A corner mark centred on `centre`.
fn fiducial(marks: &mut Vec<Mark>, centre: (f64, f64)) {
    square(marks, centre, FIDUCIAL_MM, INK);
    square(marks, centre, FIDUCIAL_MM * 0.8, WHITE);
    square(marks, centre, FIDUCIAL_MM * 0.3, INK);
}

/// The largest rectangle of `aspect` (width / height) in `area`, placed by
/// `align` (0 = left/top, 1 = right/bottom).
fn fit(aspect: f64, area: Rect, align: [f64; 2]) -> Rect {
    let (w, h) = if area.w / area.h > aspect {
        (area.h * aspect, area.h)
    } else {
        (area.w, area.w / aspect)
    };
    Rect {
        x: area.x + (area.w - w) * align[0],
        y: area.y + (area.h - h) * align[1],
        w,
        h,
    }
}

/// One frame to print: what it is for, its label and its caption text.
struct Frame {
    slot: Slot,
    label: String,
    entry: Option<Entry>,
}

/// Worksheet pages and the code printed on each.
#[derive(Clone, Debug)]
pub struct Worksheets {
    pub layout: JobLayout,
    pub codes: Vec<SheetCode>,
}

/// The profile a worksheet page is laid out with: `profile`'s paper, grid
/// and caption placement, room for the corner marks around the frames, a
/// header band as tall as the code and a footer for the instructions.
fn sheet_profile(profile: &Profile, code_mm: f64) -> Profile {
    Profile {
        margin_mm: (profile.margin_mm + FIDUCIAL_MM + FIDUCIAL_GAP_MM).min(50.),
        page_header: "{project}".into(),
        page_footer: "{page}".into(),
        logo_height_mm: code_mm,
        panel_header: "{name}".into(),
        second_panel_header: String::new(),
        ..profile.clone()
    }
}

/// A fresh batch ID for a print: eight capitals and digits.
pub fn batch_id() -> String {
    let mut bytes = [0u8; 4];
    if getrandom::fill(&mut bytes).is_err() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        bytes = nanos.to_le_bytes();
    }
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// Lay out worksheets for `panels` of `project` with `profile`'s paper,
/// panels per page and caption placement. `batch` names the print in each
/// sheet ID.
pub fn layout(
    project: &Project,
    name: &str,
    panels: &Panels,
    profile: &Profile,
    date: &str,
    batch: &str,
) -> Result<Worksheets> {
    profile.validate()?;
    let board = super::board(project)?;
    let aspect = f64::from(board.settings.width) / f64::from(board.settings.height);
    let frames: Vec<Frame> = match panels {
        Panels::Existing(scope) => select(entries(project)?, scope)?
            .into_iter()
            .map(|e| Frame {
                slot: Slot::Panel(e.page),
                label: format!("Scene {} · Panel {}", e.scene, e.number),
                entry: Some(e),
            })
            .collect(),
        Panels::New(count) => {
            if !(1..=MAX_NEW_PANELS).contains(count) {
                bail!("Print 1–{MAX_NEW_PANELS} new panels")
            }
            (1..=*count)
                .map(|n| Frame {
                    slot: Slot::New(n),
                    label: format!("New panel {n}"),
                    entry: None,
                })
                .collect()
        }
    };
    let per_page = profile.panels_per_page();
    if per_page > MAX_FRAMES {
        bail!("Worksheets hold up to {MAX_FRAMES} panels a page; choose fewer panels per page")
    }
    let pages = frames.len().div_ceil(per_page);
    if pages > MAX_SHEETS {
        bail!("Worksheets are limited to {MAX_SHEETS} pages; print fewer panels")
    }
    let project_id = project_key(&board.project_id);
    if project_id.is_empty() {
        bail!("This storyboard has no project ID; save it first")
    }
    // Every number in the code has a fixed width, so the longest code of
    // the print sizes the QR before the layout is known.
    let probe = |chunk: &[Frame], page: usize| SheetCode {
        project: project_id.clone(),
        sheet: format!("{batch}-{page}"),
        marks: (0., 0.),
        code: (0., 0., 0.),
        frame: (0., 0.),
        frames: chunk.iter().map(|f| (f.slot, (0., 0.))).collect(),
    };
    let mut modules = 0;
    for (page, chunk) in frames.chunks(per_page).enumerate() {
        modules = modules.max(qr_modules(&probe(chunk, page + 1).encode()?)?.0);
    }
    let total = (modules + 2 * QUIET_MODULES) as f64;
    let module = (MAX_CODE_MM / total).min(MODULE_MM);
    if module < MIN_MODULE_MM {
        bail!("Too many panels a page for the worksheet code; choose fewer panels per page")
    }
    let code_mm = total * module;
    let sheet_profile = sheet_profile(profile, code_mm);
    let geometry = super::sheet::page(&sheet_profile, true)?;
    let header = geometry.header.context("Missing worksheet header")?;
    let footer = geometry.footer.context("Missing worksheet footer")?;
    let printable = geometry.printable;
    let reach = FIDUCIAL_GAP_MM + FIDUCIAL_MM / 2.;
    let origin = (printable.x - reach, printable.y - reach);
    let marks_size = (printable.w + 2. * reach, printable.h + 2. * reach);
    let local = |(x, y): (f64, f64)| (x - origin.0, y - origin.1);
    // The code sits at the right of the header, inside its quiet zone.
    let quiet = QUIET_MODULES as f64 * module;
    let code_box = (
        header.x + header.w - code_mm + quiet,
        header.y + quiet,
        code_mm - 2. * quiet,
    );
    let align = picture_alignment(profile.captions);
    let mut out = Worksheets {
        layout: JobLayout {
            sheets: Vec::new(),
            warnings: Vec::new(),
        },
        codes: Vec::new(),
    };
    let fields = super::sheet::printed_fields(&board.captions, profile).0;
    for (page, chunk) in frames.chunks(per_page).enumerate() {
        let mut sheet = Sheet {
            width: geometry.width,
            height: geometry.height,
            printable: geometry.printable,
            items: Vec::new(),
            marks: Vec::new(),
        };
        let mut code = SheetCode {
            project: project_id.clone(),
            sheet: format!("{batch}-{}", page + 1),
            marks: marks_size,
            code: (code_box.0 - origin.0, code_box.1 - origin.1, code_box.2),
            frame: (0., 0.),
            frames: Vec::new(),
        };
        for (frame, cell) in chunk.iter().zip(&geometry.cells) {
            let picture = fit(aspect, cell.image, align);
            code.frame = (picture.w, picture.h);
            code.frames
                .push((frame.slot, local((picture.x, picture.y))));
            sheet.marks.push(Mark::Frame {
                bounds: picture,
                stroke_mm: 0.3,
                color: FRAME,
            });
            if let Some(rect) = cell.headers[0] {
                sheet.marks.push(Mark::Text {
                    spec: Box::new(text(
                        frame.label.clone(),
                        profile.panel_header_pt,
                        profile.panel_header_align,
                    )),
                    bounds: rect,
                });
            }
            if let Some(rect) = cell.caption {
                let rect = caption_beside(profile.captions, picture, rect);
                let mut y = rect.y + RULE_STEP_MM;
                while y <= rect.y + rect.h {
                    sheet.marks.push(Mark::Path {
                        points: vec![(rect.x, y), (rect.x + rect.w, y)],
                        closed: false,
                        filled: false,
                        stroke_mm: 0.15,
                        color: RULE,
                    });
                    y += RULE_STEP_MM;
                }
                if let Some(entry) = &frame.entry {
                    let spec = super::sheet::captions(
                        &fields,
                        &entry.panel.captions,
                        profile.caption_titles,
                        profile.caption_pt,
                    );
                    if !spec.text.is_empty() {
                        sheet.marks.push(Mark::Text {
                            spec: Box::new(spec),
                            bounds: rect,
                        });
                    }
                }
            }
        }
        let text_line = |line: String, align: Alignment, band: Rect| Mark::Text {
            spec: Box::new(text(line, profile.page_text_pt, align)),
            bounds: Rect {
                h: line_mm(profile.page_text_pt),
                w: band.w - code_mm - GAP_MM,
                ..band
            },
        };
        sheet.marks.push(text_line(
            format!(
                "{name} · Worksheet {} of {pages} · {date} · Sheet {}",
                page + 1,
                code.sheet
            ),
            Alignment::Left,
            header,
        ));
        sheet.marks.push(Mark::Text {
            spec: Box::new(text(
                "Draw inside the frames. Keep the four corner marks and the code clear, and photograph or scan the whole sheet.".into(),
                profile.page_text_pt,
                Alignment::Left,
            )),
            bounds: footer,
        });
        for corner in [
            (0., 0.),
            (marks_size.0, 0.),
            (marks_size.0, marks_size.1),
            (0., marks_size.1),
        ] {
            fiducial(&mut sheet.marks, (corner.0 + origin.0, corner.1 + origin.1));
        }
        let ((ox, oy), side) = ORIENTATION_MARK;
        square(&mut sheet.marks, (ox + origin.0, oy + origin.1), side, INK);
        qr_marks(
            &mut sheet.marks,
            &code.encode()?,
            code_box.0,
            code_box.1,
            code_box.2,
        )?;
        out.layout.sheets.push(sheet);
        out.codes.push(code);
    }
    Ok(out)
}

/// The code of a sheet of new panels laid out by `profile`, for a photo
/// whose own code cannot be read: its frames become new panels.
pub fn blank_code(project: &Project, profile: &Profile) -> Result<SheetCode> {
    let count = profile.panels_per_page() as u32;
    layout(project, "", &Panels::New(count), profile, "", "MANUAL00")?
        .codes
        .pop()
        .context("Missing worksheet layout")
}

/// Write worksheets as a PDF. Returns each page's code.
pub fn write_pdf(
    project: &Project,
    name: &str,
    panels: &Panels,
    profile: &Profile,
    path: &std::path::Path,
    cancel: &AtomicBool,
) -> Result<Vec<SheetCode>> {
    let sheets = layout(project, name, panels, profile, &super::today(), &batch_id())?;
    printing::write_pdf(&[], &sheets.layout, false, path, cancel)?;
    Ok(sheets.codes)
}

/// Where the top-left corner mark's centre, the origin of a sheet's code,
/// sits on the page in millimetres.
pub fn marks_origin(sheet: &Sheet) -> (f64, f64) {
    let reach = FIDUCIAL_GAP_MM + FIDUCIAL_MM / 2.;
    (sheet.printable.x - reach, sheet.printable.y - reach)
}

/// A worksheet page as pixels, `max_side` on its long side.
pub fn render(sheet: &Sheet, max_side: u32) -> Result<image::RgbaImage> {
    printing::preview(&[], sheet, false, max_side)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storyboard_export::{profile::builtins, tests::project};

    #[test]
    fn codes_round_trip_and_reject_other_text() {
        let code = SheetCode {
            project: "1b4e28ba-2fa1-11d2-883f-0016d3cca427".into(),
            sheet: "A1B2C3D4-2".into(),
            marks: (270.5, 183.),
            code: (230., 2.5, 30.),
            frame: (80., 45.),
            frames: vec![(Slot::Panel(17), (5., 40.)), (Slot::New(3), (90.25, 40.))],
        };
        let text = code.encode().unwrap();
        assert!(
            text.chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || ":/+-".contains(c)),
            "{text}"
        );
        let back = SheetCode::decode(&text).unwrap();
        assert_eq!(back.project, "1B4E28BA-2FA1-11D2-883F-0016D3CCA427");
        assert_eq!(back.frames[0], (Slot::Panel(17), (5., 40.)));
        assert_eq!(back.frames[1].0, Slot::New(3));
        assert!((back.frames[1].1.0 - 90.3).abs() < 1e-9, "tenths of a mm");
        for bad in [
            "",
            "hello",
            &text.replace("EMW1", "EMW2"),
            &text.replace("+N3/", "+N0/"),
            // A frame outside the marks.
            &text.replace("0800/0450", "9000/0450"),
        ] {
            assert!(SheetCode::decode(bad).is_none(), "{bad}");
        }
        assert!(
            SheetCode {
                marks: (1200., 10.),
                ..code
            }
            .encode()
            .is_err()
        );
    }

    #[test]
    fn sheets_hold_frames_at_the_board_aspect_with_marks_and_codes() {
        let project = project();
        let six = builtins().remove(1);
        let sheets = layout(
            &project,
            "Film",
            &Panels::Existing(Scope::All),
            &six,
            "2026-10-03",
            "ABCD0123",
        )
        .unwrap();
        assert_eq!(sheets.layout.sheets.len(), 1);
        let code = &sheets.codes[0];
        assert_eq!(code.frames.len(), 3);
        let ids: Vec<_> = project
            .pages
            .iter()
            .map(|p| Slot::Panel(p.meta.id))
            .collect();
        assert_eq!(
            code.frames.iter().map(|f| f.0).collect::<Vec<_>>(),
            ids,
            "board order"
        );
        assert!((code.frame.0 / code.frame.1 - 64. / 36.).abs() < 1e-6);
        assert_eq!(code.sheet, "ABCD0123-1");
        let sheet = &sheets.layout.sheets[0];
        let texts: Vec<_> = sheet
            .marks
            .iter()
            .filter_map(|m| match m {
                Mark::Text { spec, .. } => Some(spec.text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            texts.contains(&"Scene 1 · Panel 2".to_string()),
            "{texts:?}"
        );
        assert!(texts.contains(&"Scene 2 · Panel 1".to_string()));
        assert!(
            texts.iter().any(|t| t.contains("Mia runs")),
            "captions print"
        );
        // New panels: 14 over three six-up pages, numbered through the print.
        let blank = layout(&project, "Film", &Panels::New(14), &six, "", "B").unwrap();
        assert_eq!(blank.codes.len(), 3);
        assert_eq!(blank.codes[2].frames[1].0, Slot::New(14));
        assert!(layout(&project, "Film", &Panels::New(0), &six, "", "B").is_err());
        let crowded = Profile {
            columns: 6,
            rows: 4,
            ..six
        };
        assert!(layout(&project, "Film", &Panels::New(30), &crowded, "", "B").is_err());
        let manual = blank_code(&project, &builtins().remove(0)).unwrap();
        assert_eq!(manual.frames.len(), 3);
        assert!(matches!(manual.frames[0].0, Slot::New(1)));
    }

    #[test]
    fn worksheets_write_a_pdf_and_render_their_code() {
        let project = project();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worksheets.pdf");
        let cancel = AtomicBool::new(false);
        let one = builtins().remove(2);
        let codes = write_pdf(
            &project,
            "Film",
            &Panels::Existing(Scope::All),
            &one,
            &path,
            &cancel,
        )
        .unwrap();
        assert_eq!(codes.len(), 3);
        assert!(std::fs::read(&path).unwrap().starts_with(b"%PDF-"));
        let sheets = layout(&project, "Film", &Panels::New(1), &one, "2026-10-03", "B").unwrap();
        let image = render(&sheets.layout.sheets[0], 1800).unwrap();
        let gray = image::DynamicImage::ImageRgba8(image).to_luma8();
        let mut prepared = rqrr::PreparedImage::prepare_from_greyscale(
            gray.width() as usize,
            gray.height() as usize,
            |x, y| gray.get_pixel(x as u32, y as u32).0[0],
        );
        let grids = prepared.detect_grids();
        let texts: Vec<_> = grids.iter().filter_map(|g| g.decode().ok()).collect();
        assert_eq!(texts.len(), 1);
        assert_eq!(texts[0].1, sheets.codes[0].encode().unwrap());
    }
}
