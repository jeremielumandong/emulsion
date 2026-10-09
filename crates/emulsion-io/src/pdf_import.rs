//! PDF and AI (.ai) pages as editable vector art. An external converter
//! turns each page into SVG — Poppler's `pdftocairo`, else MuPDF's
//! `mutool` — and the SVG import does the rest, so a page arrives as the
//! same vector group an SVG file gives. AI files open when they
//! were saved with PDF compatibility (the usual default).
use crate::ffmpeg::{Waited, command};
use crate::{IoError, Result};
use emulsion_core::Document;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// Extensions this import reads, lower case.
pub const EXTENSIONS: &[&str] = &["pdf", "ai"];

/// Whether `path` names a file this import reads, by extension.
pub fn is_pdf(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// Shown when neither converter is installed.
pub const MISSING: &str = "Importing PDF and AI files needs Poppler (pdftocairo) or MuPDF (mutool). Install one and make sure it is on PATH, then try again.";

/// Pages read from one file at most.
pub const MAX_PAGES: usize = 200;

const PAGE_TIMEOUT: Duration = Duration::from_secs(120);

fn error(s: impl Into<String>) -> IoError {
    IoError::Unsupported(s.into())
}

/// Which converter runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Converter {
    Poppler,
    MuPdf,
}

fn runs(program: &str, arg: &str) -> bool {
    command(program)
        .arg(arg)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

/// The converter on PATH, Poppler first.
pub fn converter() -> Option<Converter> {
    if runs("pdftocairo", "-v") && runs("pdfinfo", "-v") {
        Some(Converter::Poppler)
    } else if runs("mutool", "-v") {
        Some(Converter::MuPdf)
    } else {
        None
    }
}

/// Run to completion; a failure names the converter's last error line.
fn run(mut cmd: Command, cancel: &AtomicBool) -> Result<String> {
    let program = crate::ffmpeg::Program {
        name: "the PDF converter",
        missing: MISSING,
    };
    let done = crate::ffmpeg::run(&mut cmd, program, None, 1 << 20, cancel, Some(PAGE_TIMEOUT))
        .map_err(|e| error(e.to_string()))?;
    match done.waited {
        Waited::Exited(status) if status.success() => {
            Ok(String::from_utf8_lossy(&done.stdout).into_owned())
        }
        Waited::Exited(_) => Err(error(format!(
            "The PDF converter could not read this file. {}",
            crate::ffmpeg::last_line(&done.stderr)
        ))),
        Waited::Canceled => Err(error("Import canceled.")),
        Waited::TimedOut => Err(error("The PDF converter took too long on a page.")),
    }
}

fn page_count(path: &Path, tool: Converter, cancel: &AtomicBool) -> Result<usize> {
    let (mut cmd, key) = match tool {
        Converter::Poppler => (command("pdfinfo"), "Pages:"),
        Converter::MuPdf => {
            let mut cmd = command("mutool");
            cmd.arg("info");
            (cmd, "Pages:")
        }
    };
    cmd.arg(path);
    let info = run(cmd, cancel)?;
    info.lines()
        .find_map(|l| l.trim().strip_prefix(key)?.trim().parse().ok())
        .filter(|&n: &usize| n > 0)
        .ok_or_else(|| error("This file has no pages the PDF converter can read."))
}

fn page_svg(
    path: &Path,
    page: usize,
    tool: Converter,
    dir: &Path,
    cancel: &AtomicBool,
) -> Result<String> {
    let out = dir.join(format!("page-{page}.svg"));
    let cmd = match tool {
        Converter::Poppler => {
            let mut cmd = command("pdftocairo");
            cmd.args(["-svg", "-f", &page.to_string(), "-l", &page.to_string()])
                .arg(path)
                .arg(&out);
            cmd
        }
        Converter::MuPdf => {
            let mut cmd = command("mutool");
            cmd.args(["convert", "-F", "svg", "-o"])
                .arg(&out)
                .arg(path)
                .arg(page.to_string());
            cmd
        }
    };
    run(cmd, cancel)?;
    // MuPDF numbers its output even for one page.
    let numbered = dir.join(format!("page-{page}1.svg"));
    let file = if out.exists() { out } else { numbered };
    Ok(std::fs::read_to_string(file)?)
}

/// One vector document per page, up to [`MAX_PAGES`], named after the file:
/// "Layouts page 2", or just "Layouts" for a one-page file.
/// `progress` hears (page, pages) before each page converts.
pub fn pages(
    path: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, usize),
) -> Result<Vec<(String, Document)>> {
    let tool = converter().ok_or_else(|| error(MISSING))?;
    let count = page_count(path, tool, cancel)?;
    if count > MAX_PAGES {
        return Err(error(format!(
            "This file has {count} pages; Emulsion imports up to {MAX_PAGES} at a time."
        )));
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Page".into());
    let dir = tempfile::tempdir()?;
    let mut docs = Vec::with_capacity(count);
    for page in 1..=count {
        progress(page, count);
        let svg = page_svg(path, page, tool, dir.path(), cancel)?;
        // Converters can write a page with no size; the art's bounds fix it.
        let doc = crate::svg_vectors::document(&svg)
            .or_else(|_| crate::svg_vectors::fitted_document(&svg))?;
        let name = if count == 1 {
            stem.clone()
        } else {
            format!("{stem} page {page}")
        };
        docs.push((name, doc));
    }
    Ok(docs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdf_writer::{Content, Finish, Pdf, Rect, Ref};

    /// A PDF whose pages each hold one filled rectangle, `sizes` in points.
    fn sample(sizes: &[(f32, f32)]) -> Vec<u8> {
        let mut pdf = Pdf::new();
        let catalog = Ref::new(1);
        let tree = Ref::new(2);
        pdf.catalog(catalog).pages(tree);
        let ids: Vec<Ref> = (0..sizes.len() as i32 * 2)
            .step_by(2)
            .map(|i| Ref::new(10 + i))
            .collect();
        pdf.pages(tree)
            .kids(ids.iter().copied())
            .count(sizes.len() as i32);
        for (id, &(w, h)) in ids.iter().zip(sizes) {
            let content = Ref::new(id.get() + 1);
            let mut page = pdf.page(*id);
            page.media_box(Rect::new(0., 0., w, h))
                .parent(tree)
                .contents(content);
            page.finish();
            let mut c = Content::new();
            c.set_fill_rgb(0.9, 0.2, 0.1);
            c.rect(10., 10., w / 2., h / 2.);
            c.fill_nonzero();
            pdf.stream(content, &c.finish());
        }
        pdf.finish()
    }

    #[test]
    fn every_page_becomes_a_vector_document() {
        if converter().is_none() {
            eprintln!("skipped: no PDF converter on PATH");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("board.pdf");
        std::fs::write(&path, sample(&[(200., 100.), (100., 300.)])).unwrap();
        let mut seen = Vec::new();
        let docs = pages(&path, &AtomicBool::new(false), |p, n| seen.push((p, n))).unwrap();
        assert_eq!(seen, [(1, 2), (2, 2)]);
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].0, "board page 1");
        assert!(is_pdf(&path) && is_pdf(Path::new("ART.AI")) && !is_pdf(Path::new("a.svg")));
        // Points become pixels at the converter's scale; shape is kept.
        let (a, b) = (&docs[0].1, &docs[1].1);
        assert!(a.width > a.height && b.height > b.width);
        assert!(!a.nodes.is_empty());
        // An .ai file with PDF compatibility reads the same way.
        let ai = dir.path().join("art.ai");
        std::fs::write(&ai, sample(&[(50., 50.)])).unwrap();
        let art = pages(&ai, &AtomicBool::new(false), |_, _| {}).unwrap();
        assert_eq!(art.len(), 1);
        assert_eq!(art[0].0, "art");
    }

    #[test]
    fn unreadable_files_explain_themselves() {
        if converter().is_none() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.pdf");
        std::fs::write(&path, b"%!PS-Adobe-3.0 not a pdf").unwrap();
        let message = pages(&path, &AtomicBool::new(false), |_, _| {})
            .unwrap_err()
            .to_string();
        assert!(message.contains("PDF converter"), "{message}");
    }
}
