//! Physical sheet composition shared by preview, PDF and native print adapters.
//! All device I/O is blocking: callers must run it on a worker, never the UI thread.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod cups;
#[cfg(target_os = "linux")]
pub mod portal;
mod render;
#[cfg(target_os = "windows")]
mod windows;
pub use render::{Source, prepare_sources, preview, write_pdf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Printer {
    pub id: String,
    pub name: String,
    pub default: bool,
    pub status: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Paper {
    pub id: String,
    pub name: String,
    pub width: f64,
    pub height: f64,
    /// Top, right, bottom, left, in millimeters, in portrait device coordinates.
    pub margins: [f64; 4],
}
impl Paper {
    pub fn pdf() -> Vec<Self> {
        [
            ("A4", 210., 297.),
            ("Letter", 215.9, 279.4),
            ("4 × 6", 101.6, 152.4),
            ("5 × 7", 127., 177.8),
            ("8 × 10", 203.2, 254.),
            ("A3", 297., 420.),
            ("A5", 148., 210.),
            ("Business card · 3.5 × 2", 50.8, 88.9),
            ("Poster · 18 × 24", 457.2, 609.6),
        ]
        .into_iter()
        .map(|(name, width, height)| Self {
            id: name.into(),
            name: name.into(),
            width,
            height,
            margins: [0.; 4],
        })
        .collect()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Choice {
    pub id: String,
    pub name: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capabilities {
    pub papers: Vec<Paper>,
    pub default_paper: String,
    pub media: Vec<Choice>,
    pub trays: Vec<Choice>,
    pub quality: Vec<Choice>,
    pub sides: Vec<Choice>,
    pub color: bool,
}
impl Capabilities {
    pub fn pdf() -> Self {
        Self {
            papers: Paper::pdf(),
            default_paper: "A4".into(),
            media: vec![],
            trays: vec![],
            quality: vec![],
            sides: vec![],
            color: true,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    Fit,
    Fill,
    Actual,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// PDF pages retain each source's dimensions and resolution, without scaling.
    Document,
    Single,
    Contact,
    Repeat,
    Poster,
}
#[derive(Clone, Debug)]
pub struct Settings {
    pub paper: Paper,
    pub landscape: bool,
    pub placement: Placement,
    pub layout: Layout,
    pub scale: f64,
    pub extra_margin: f64,
    pub overlap: f64,
    pub copies: u16,
    pub grayscale: bool,
    pub media: Option<String>,
    pub tray: Option<String>,
    pub quality: Option<String>,
    pub sides: Option<String>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            paper: Paper::pdf().remove(0),
            landscape: false,
            placement: Placement::Fit,
            layout: Layout::Single,
            scale: 100.,
            extra_margin: 5.,
            overlap: 5.,
            copies: 1,
            grayscale: false,
            media: None,
            tray: None,
            quality: None,
            sides: None,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}
#[derive(Clone, Debug)]
pub struct Item {
    pub source: usize,
    pub bounds: Rect,
    pub clip: Rect,
}
#[derive(Clone, Debug)]
pub struct Sheet {
    pub width: f64,
    pub height: f64,
    pub printable: Rect,
    pub items: Vec<Item>,
}
#[derive(Clone, Debug)]
pub struct JobLayout {
    pub sheets: Vec<Sheet>,
    pub warnings: Vec<String>,
}

/// Match physical page size in either orientation, allowing pixel rounding at print PPI.
pub fn matching_paper(source: &Source, papers: &[Paper]) -> Option<(Paper, bool)> {
    let (w, h) = source.physical_size().ok()?;
    papers.iter().find_map(|p| {
        if (p.width - w).abs() < 0.5 && (p.height - h).abs() < 0.5 {
            Some((p.clone(), false))
        } else if (p.height - w).abs() < 0.5 && (p.width - h).abs() < 0.5 {
            Some((p.clone(), true))
        } else {
            None
        }
    })
}

pub fn canceled(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        bail!("Canceled")
    }
    Ok(())
}

/// Parse user-facing, 1-based ranges, preserving document order and rejecting typos.
pub fn page_range(value: &str, count: usize) -> Result<Vec<usize>> {
    if value.trim().is_empty() {
        return Ok((0..count).collect());
    }
    let mut selected = std::collections::BTreeSet::new();
    for part in value.split(',') {
        let pair = part.trim().split('-').map(str::trim).collect::<Vec<_>>();
        if pair.is_empty() || pair.len() > 2 {
            bail!("Use page ranges such as 1-3, 5")
        }
        let start: usize = pair[0].parse().context("Use page ranges such as 1-3, 5")?;
        let end: usize = pair
            .last()
            .unwrap()
            .parse()
            .context("Invalid page number")?;
        if start == 0 || end < start || end > count {
            bail!("Page ranges must be within 1–{count}")
        }
        selected.extend(start - 1..end);
    }
    Ok(selected.into_iter().collect())
}

pub fn layout(sources: &[Source], selected: &[usize], settings: &Settings) -> Result<JobLayout> {
    let s = settings;
    if selected.is_empty() || selected.iter().any(|&i| i >= sources.len()) {
        bail!("Select at least one existing page")
    }
    if !(1..=999).contains(&s.copies)
        || !s.scale.is_finite()
        || !(1. ..=1000.).contains(&s.scale)
        || !s.extra_margin.is_finite()
        || !(0. ..=100.).contains(&s.extra_margin)
        || !s.overlap.is_finite()
        || !(0. ..=50.).contains(&s.overlap)
    {
        bail!("Check copies, scale, margins and overlap")
    }
    if s.layout == Layout::Document {
        if selected.len() > 200 {
            bail!("Print jobs are limited to 200 sheets before copies")
        }
        let sheets = selected
            .iter()
            .map(|&source| {
                let (width, height) = sources[source].physical_size()?;
                if width > 2000. || height > 2000. {
                    bail!("Document pages exceed 2000 mm; use a paper size or tiled poster layout")
                }
                let bounds = Rect {
                    x: 0.,
                    y: 0.,
                    w: width,
                    h: height,
                };
                Ok(Sheet {
                    width,
                    height,
                    printable: bounds,
                    items: vec![Item {
                        source,
                        bounds,
                        clip: bounds,
                    }],
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let warnings = selected
            .iter()
            .map(|&i| &sources[i])
            .filter(|source| source.rasterized && source.ppi < 150.)
            .map(|source| {
                format!(
                    "{}: {:.0} effective PPI; the print may look soft.",
                    source.name, source.ppi
                )
            })
            .collect();
        return Ok(JobLayout { sheets, warnings });
    }
    let p = &s.paper;
    if !p.width.is_finite()
        || !p.height.is_finite()
        || !(10. ..=2000.).contains(&p.width)
        || !(10. ..=2000.).contains(&p.height)
        || p.margins.iter().any(|v| !v.is_finite() || *v < 0.)
    {
        bail!("Invalid printer paper dimensions")
    }
    let (width, height, m) = if s.landscape {
        (
            p.height,
            p.width,
            [p.margins[3], p.margins[0], p.margins[1], p.margins[2]],
        )
    } else {
        (p.width, p.height, p.margins)
    };
    let printable = Rect {
        x: m[3] + s.extra_margin,
        y: m[0] + s.extra_margin,
        w: width - m[1] - m[3] - 2. * s.extra_margin,
        h: height - m[0] - m[2] - 2. * s.extra_margin,
    };
    if printable.w <= 0. || printable.h <= 0. {
        bail!("Margins leave no printable area")
    }
    let blank = || Sheet {
        width,
        height,
        printable,
        items: Vec::new(),
    };
    let mut result = JobLayout {
        sheets: vec![],
        warnings: vec![],
    };
    let mut add = |sheet: &mut Sheet, source: usize, cell: Rect| -> Result<()> {
        let doc = &sources[source];
        if !doc.ppi.is_finite() || doc.ppi <= 0. || doc.width == 0 || doc.height == 0 {
            bail!("Invalid source dimensions or resolution")
        }
        let natural = (
            doc.width as f64 / doc.ppi * 25.4,
            doc.height as f64 / doc.ppi * 25.4,
        );
        let k = match s.placement {
            Placement::Fit => (cell.w / natural.0).min(cell.h / natural.1),
            Placement::Fill => (cell.w / natural.0).max(cell.h / natural.1),
            Placement::Actual => s.scale / 100.,
        };
        let (w, h) = (natural.0 * k, natural.1 * k);
        if (w > cell.w + 0.01 || h > cell.h + 0.01)
            && !result
                .warnings
                .iter()
                .any(|v| v == "Artwork extends beyond its print area and will be cropped.")
        {
            result
                .warnings
                .push("Artwork extends beyond its print area and will be cropped.".into());
        }
        if doc.rasterized && doc.ppi / k < 150. {
            result.warnings.push(format!(
                "{}: {:.0} effective PPI; the print may look soft.",
                doc.name,
                doc.ppi / k
            ));
        }
        sheet.items.push(Item {
            source,
            bounds: Rect {
                x: cell.x + (cell.w - w) / 2.,
                y: cell.y + (cell.h - h) / 2.,
                w,
                h,
            },
            clip: cell,
        });
        Ok(())
    };
    match s.layout {
        Layout::Document => unreachable!("document pages composed above"),
        Layout::Single => {
            for &source in selected {
                let mut sheet = blank();
                add(&mut sheet, source, printable)?;
                result.sheets.push(sheet);
            }
        }
        Layout::Contact | Layout::Repeat => {
            let items = if s.layout == Layout::Repeat {
                vec![selected[0]; 6]
            } else {
                selected.to_vec()
            };
            let (w, h) = ((printable.w - 5.) / 2., (printable.h - 10.) / 3.);
            if w <= 0. || h <= 0. {
                bail!("Paper is too small for this contact sheet")
            }
            for chunk in items.chunks(6) {
                let mut sheet = blank();
                for (i, &source) in chunk.iter().enumerate() {
                    add(
                        &mut sheet,
                        source,
                        Rect {
                            x: printable.x + (i % 2) as f64 * (w + 5.),
                            y: printable.y + (i / 2) as f64 * (h + 5.),
                            w,
                            h,
                        },
                    )?;
                }
                result.sheets.push(sheet);
            }
        }
        Layout::Poster => {
            if selected.len() != 1 {
                bail!("Select one source page for a tiled poster")
            }
            let source = selected[0];
            let doc = &sources[source];
            if !doc.ppi.is_finite() || doc.ppi <= 0. {
                bail!("Invalid source resolution")
            }
            let (w, h) = (
                doc.width as f64 / doc.ppi * 25.4 * s.scale / 100.,
                doc.height as f64 / doc.ppi * 25.4 * s.scale / 100.,
            );
            let (stepx, stepy) = (printable.w - s.overlap, printable.h - s.overlap);
            if stepx <= 0. || stepy <= 0. {
                bail!("Overlap must be smaller than the printable area")
            }
            let cols = ((w - s.overlap) / stepx).ceil().max(1.) as usize;
            let rows = ((h - s.overlap) / stepy).ceil().max(1.) as usize;
            if cols.saturating_mul(rows) > 200 {
                bail!("Poster exceeds 200 sheets; reduce the scale")
            }
            for y in 0..rows {
                for x in 0..cols {
                    let mut sheet = blank();
                    sheet.items.push(Item {
                        source,
                        bounds: Rect {
                            x: printable.x - x as f64 * stepx,
                            y: printable.y - y as f64 * stepy,
                            w,
                            h,
                        },
                        clip: printable,
                    });
                    result.sheets.push(sheet);
                }
            }
        }
    }
    if result.sheets.len() > 200 {
        bail!("Print jobs are limited to 200 sheets before copies")
    }
    Ok(result)
}

pub fn discover() -> Result<Vec<Printer>> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        cups::discover()
    }
    #[cfg(target_os = "windows")]
    {
        windows::discover()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        bail!("Printing is unavailable on this platform")
    }
}
pub fn capabilities(printer: &str) -> Result<Capabilities> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        cups::capabilities(printer)
    }
    #[cfg(target_os = "windows")]
    {
        windows::capabilities(printer)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = printer;
        bail!("Printing is unavailable")
    }
}
/// Check native device options without creating a job (useful for preflight).
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub fn validate_device_settings(printer: &str, settings: &Settings) -> Result<()> {
    cups::validate(printer, settings)
}

/// Returns acceptance by the OS queue, never a claim of physical completion.
pub fn submit(
    printer: &str,
    title: &str,
    sources: &[Source],
    layout: &JobLayout,
    settings: &Settings,
    cancel: &AtomicBool,
) -> Result<String> {
    canceled(cancel)?;
    if settings.layout == Layout::Document {
        bail!("Document page sizes are for PDF. Select printer paper for a physical print job.")
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("print.pdf");
        write_pdf(sources, layout, settings.grayscale, &path, cancel)?;
        canceled(cancel)?;
        cups::submit(printer, title, &path, settings, cancel)
    }
    #[cfg(target_os = "windows")]
    {
        windows::submit(printer, title, sources, layout, settings, cancel)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = (printer, title, sources, layout, settings);
        bail!("Printing is unavailable")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> Source {
        Source {
            name: "Test".into(),
            width: 1200,
            height: 600,
            ppi: 300.,
            svg: String::new(),
            rasterized: true,
            original_paths: vec![],
        }
    }
    #[test]
    fn physical_fit_fill_and_actual_size() {
        let sources = [source()];
        let mut s = Settings::default();
        let fit = layout(&sources, &[0], &s).unwrap();
        let b = fit.sheets[0].items[0].bounds;
        assert!((b.w - 200.).abs() < 0.001);
        assert!((b.h - 100.).abs() < 0.001);
        s.placement = Placement::Actual;
        let a = layout(&sources, &[0], &s).unwrap();
        assert!((a.sheets[0].items[0].bounds.w - 101.6).abs() < 0.001);
        s.placement = Placement::Fill;
        assert!(!layout(&sources, &[0], &s).unwrap().warnings.is_empty());
    }
    #[test]
    fn asymmetric_margins_rotate_and_invalid_values_fail() {
        let mut s = Settings::default();
        s.paper.margins = [1., 2., 3., 4.];
        s.landscape = true;
        let result = layout(&[source()], &[0], &s).unwrap();
        assert_eq!(result.sheets[0].printable.x, 8.);
        assert_eq!(result.sheets[0].printable.y, 9.);
        s.extra_margin = f64::NAN;
        assert!(layout(&[source()], &[0], &s).is_err());
    }
    #[test]
    fn contact_sheets_and_poster_overlap() {
        let sources = vec![source(); 7];
        let mut s = Settings {
            layout: Layout::Contact,
            ..Default::default()
        };
        assert_eq!(
            layout(&sources, &[0, 1, 2, 3, 4, 5, 6], &s)
                .unwrap()
                .sheets
                .len(),
            2
        );
        s.layout = Layout::Poster;
        s.scale = 400.;
        let l = layout(&sources, &[0], &s).unwrap();
        assert_eq!(l.sheets.len(), 3);
        assert!(
            (l.sheets[0].items[0].bounds.x - l.sheets[1].items[0].bounds.x - 195.).abs() < 0.01
        );
    }
    #[test]
    fn ranges_are_ordered_deduplicated_and_validated() {
        assert_eq!(page_range("3, 1-2, 2", 3).unwrap(), vec![0, 1, 2]);
        for range in ["0", "3-2", "1-9", "1,", "-1"] {
            assert!(page_range(range, 3).is_err());
        }
    }
    #[test]
    fn document_pdf_keeps_mixed_page_sizes_without_fit_crop_or_extra_margins() {
        let mut card = source();
        card.width = 1050;
        card.height = 600;
        let mut invite = source();
        invite.width = 1500;
        invite.height = 2100;
        let sources = [card, invite];
        let settings = Settings {
            layout: Layout::Document,
            landscape: true,
            scale: 250.,
            extra_margin: 30.,
            placement: Placement::Fill,
            ..Default::default()
        };
        let job = layout(&sources, &[1, 0], &settings).unwrap();
        assert_eq!(job.sheets.len(), 2);
        for (sheet, (w, h)) in job.sheets.iter().zip([(127., 177.8), (88.9, 50.8)]) {
            assert!((sheet.width - w).abs() < 1e-8);
            assert!((sheet.height - h).abs() < 1e-8);
            assert_eq!(sheet.items[0].bounds.x, 0.);
            assert_eq!(sheet.items[0].bounds.y, 0.);
            assert_eq!(sheet.items[0].bounds.w, sheet.width);
            assert_eq!(sheet.items[0].bounds.h, sheet.height);
        }
        assert!(job.warnings.is_empty());
        let mut invalid = sources[0].clone();
        invalid.ppi = f64::NAN;
        assert!(layout(&[invalid], &[0], &settings).is_err());
        assert!(
            submit(
                "unused",
                "Test",
                &sources,
                &job,
                &settings,
                &AtomicBool::new(false)
            )
            .is_err()
        );
    }
    #[test]
    fn matches_only_supported_paper_with_pixel_rounding_and_correct_orientation() {
        let mut a4 = source();
        a4.width = 2480;
        a4.height = 3508;
        let papers = Paper::pdf();
        let (paper, landscape) = matching_paper(&a4, &papers).unwrap();
        assert_eq!(paper.id, "A4");
        assert!(!landscape);
        std::mem::swap(&mut a4.width, &mut a4.height);
        assert!(matching_paper(&a4, &papers).unwrap().1);
        assert!(matching_paper(&a4, &papers[1..2]).is_none());
        a4.ppi = 72.;
        assert!(matching_paper(&a4, &papers).is_none());
    }
}
