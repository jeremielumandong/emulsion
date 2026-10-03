//! Worksheets round trip: print a sheet, draw on it, photograph it badly
//! (turned, in perspective, unevenly lit, tinted and noisy) and read the
//! drawings back onto the right panels.
use super::worksheet::{self, Panels, SheetCode, Slot, marks_origin};
use super::worksheet_scan::{Clean, ScanError, sample, scan};
use super::{Scope, profile::builtins, tests::project};
use crate::printing::{Mark, Rect, Sheet};
use emulsion_raster::warp;
use image::RgbaImage;
use std::sync::atomic::AtomicBool;

/// Rendered sheet pixels per millimetre.
const PX_PER_MM: f64 = 10.;
/// Panel resolution the drawings come back at.
const PANEL: (u32, u32) = (320, 180);
/// Pen width on paper.
const PEN_MM: f64 = 1.2;

/// A different drawing for each frame, in the frame's 0–1 coordinates.
fn drawing(index: usize) -> Vec<Vec<(f64, f64)>> {
    match index % 3 {
        0 => vec![
            vec![(0.1, 0.15), (0.9, 0.85)],
            vec![(0.2, 0.8), (0.45, 0.2)],
        ],
        1 => vec![
            (0..=24)
                .map(|i| {
                    let t = i as f64 / 24. * std::f64::consts::TAU;
                    (0.5 + 0.25 * t.cos(), 0.5 + 0.38 * t.sin())
                })
                .collect(),
        ],
        _ => vec![vec![(0.15, 0.5), (0.85, 0.5)], vec![(0.7, 0.1), (0.7, 0.9)]],
    }
}

/// Draw each frame's drawing onto `sheet` as pen strokes.
fn draw(sheet: &mut Sheet, code: &SheetCode) {
    let origin = marks_origin(sheet);
    for (index, (_, corner)) in code.frames.iter().enumerate() {
        let rect = code.frame_rect(*corner);
        for line in drawing(index) {
            sheet.marks.push(Mark::Path {
                points: line
                    .iter()
                    .map(|(u, v)| {
                        (
                            origin.0 + rect.x + u * rect.w,
                            origin.1 + rect.y + v * rect.h,
                        )
                    })
                    .collect(),
                closed: false,
                filled: false,
                stroke_mm: PEN_MM,
                color: [40, 40, 50],
            });
        }
    }
}

/// The drawing of frame `index` as a panel-sized ink mask.
fn expected(index: usize, frame: (f64, f64)) -> Vec<bool> {
    let (w, h) = (PANEL.0 as usize, PANEL.1 as usize);
    let radius = PEN_MM / 2. / frame.0 * w as f64;
    let lines = drawing(index);
    (0..w * h)
        .map(|i| {
            let (x, y) = ((i % w) as f64 + 0.5, (i / w) as f64 + 0.5);
            lines.iter().any(|line| {
                line.windows(2).any(|s| {
                    let (a, b) = (
                        (s[0].0 * w as f64, s[0].1 * h as f64),
                        (s[1].0 * w as f64, s[1].1 * h as f64),
                    );
                    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                    let t = (((x - a.0) * dx + (y - a.1) * dy) / (dx * dx + dy * dy)).clamp(0., 1.);
                    (x - a.0 - t * dx).hypot(y - a.1 - t * dy) <= radius
                })
            })
        })
        .collect()
}

/// Share of `of`'s set pixels with a set pixel of `near` within 2 pixels.
fn covered(of: &[bool], near: &[bool]) -> f64 {
    let (w, h) = (PANEL.0 as i64, PANEL.1 as i64);
    let total = of.iter().filter(|v| **v).count().max(1);
    let hits = (0..w * h)
        .filter(|i| of[*i as usize])
        .filter(|i| {
            let (x, y) = (i % w, i / w);
            (-2..=2).any(|dy| {
                (-2..=2).any(|dx| {
                    let (nx, ny) = (x + dx, y + dy);
                    nx >= 0 && ny >= 0 && nx < w && ny < h && near[(ny * w + nx) as usize]
                })
            })
        })
        .count();
    hits as f64 / total as f64
}

/// A phone photo of `sheet`: its corners land on `quad` in a `size` photo
/// on a dark table, lit unevenly from one side, warm-tinted and noisy.
fn photograph(sheet: &RgbaImage, quad: [(f64, f64); 4], size: (u32, u32)) -> RgbaImage {
    let (sw, sh) = (f64::from(sheet.width()), f64::from(sheet.height()));
    let to_sheet = warp::homography(quad, [(0., 0.), (sw, 0.), (sw, sh), (0., sh)]).unwrap();
    let mut seed = 0x2545_f491_u32;
    RgbaImage::from_fn(size.0, size.1, |x, y| {
        let (px, py) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
        let (u, v) = warp::apply(&to_sheet, (px, py));
        let base = if u >= 0. && v >= 0. && u < sw && v < sh {
            sample(sheet, u, v)
        } else {
            [0.3, 0.22, 0.15]
        };
        let light =
            0.55 + 0.4 * (px / f64::from(size.0)) as f32 + 0.1 * (py / f64::from(size.1)) as f32;
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let noise = (seed % 21) as f32 / 255. - 10. / 255.;
        let tint = [1., 0.94, 0.82];
        let c: [f32; 3] =
            std::array::from_fn(|i| (base[i] * light * tint[i] + noise).clamp(0., 1.));
        image::Rgba([
            (c[0] * 255.) as u8,
            (c[1] * 255.) as u8,
            (c[2] * 255.) as u8,
            255,
        ])
    })
}

/// A drawn six-up worksheet of the test board, rendered, with its code.
fn drawn_sheet(blank_code: bool) -> (RgbaImage, SheetCode) {
    let project = project();
    let six = builtins().remove(1);
    let mut sheets = worksheet::layout(
        &project,
        "Film",
        &Panels::Existing(Scope::All),
        &six,
        "2026-10-03",
        "TEST0001",
    )
    .unwrap();
    let code = sheets.codes.remove(0);
    let sheet = &mut sheets.layout.sheets[0];
    draw(sheet, &code);
    if blank_code {
        // Cover the code with a white sticker.
        let origin = marks_origin(sheet);
        let (x, y, s) = code.code;
        sheet.marks.push(Mark::Path {
            points: {
                let r = Rect {
                    x: origin.0 + x - 1.,
                    y: origin.1 + y - 1.,
                    w: s + 2.,
                    h: s + 2.,
                };
                vec![
                    (r.x, r.y),
                    (r.x + r.w, r.y),
                    (r.x + r.w, r.y + r.h),
                    (r.x, r.y + r.h),
                ]
            },
            closed: true,
            filled: true,
            stroke_mm: 0.1,
            color: [255, 255, 255],
        });
    }
    let side = (sheet.width.max(sheet.height) * PX_PER_MM) as u32;
    (worksheet::render(sheet, side).unwrap(), code)
}

/// Every frame comes back on its own panel looking like what was drawn.
fn check(read: &super::worksheet_scan::ScannedSheet, code: &SheetCode) {
    let ids: Vec<_> = project()
        .pages
        .iter()
        .map(|p| Slot::Panel(p.meta.id))
        .collect();
    assert_eq!(read.frames.iter().map(|f| f.slot).collect::<Vec<_>>(), ids);
    for (index, frame) in read.frames.iter().enumerate() {
        assert!(frame.drawn(), "frame {index} has ink");
        assert_eq!(frame.image.dimensions(), PANEL);
        let got: Vec<bool> = frame.image.pixels().map(|p| p.0[3] > 128).collect();
        let want = expected(index, code.frame);
        let (recall, precision) = (covered(&want, &got), covered(&got, &want));
        assert!(
            recall > 0.85 && precision > 0.9,
            "frame {index}: recall {recall:.2}, precision {precision:.2}"
        );
        // Paper is transparent, ink is dark.
        let paper = frame.image.get_pixel(PANEL.0 / 2, 3).0;
        assert_eq!(paper[3], 0, "paper near the top edge");
    }
}

#[test]
fn a_tilted_unevenly_lit_photo_comes_back_on_the_right_panels() {
    let (sheet, code) = drawn_sheet(false);
    let cancel = AtomicBool::new(false);
    // Turned about 4°, in perspective, on a 2400 × 1800 photo.
    let photo = photograph(
        &sheet,
        [(260., 230.), (2150., 110.), (2260., 1580.), (170., 1660.)],
        (2400, 1800),
    );
    let read = scan(
        &photo,
        &code.project,
        None,
        PANEL,
        Clean::Transparent,
        &cancel,
    )
    .unwrap();
    assert!(read.code_read);
    assert_eq!(read.code.encode().unwrap(), code.encode().unwrap());
    check(&read, &code);
    // Turned a quarter: the code says which corner is which.
    let photo = photograph(
        &sheet,
        [(1650., 180.), (1700., 2250.), (130., 2200.), (190., 140.)],
        (1800, 2400),
    );
    let read = scan(&photo, &code.project, None, PANEL, Clean::LineArt, &cancel).unwrap();
    check(&read, &code);
    let ink = read.frames[0]
        .image
        .pixels()
        .find(|p| p.0[3] > 200)
        .unwrap();
    assert_eq!(&ink.0[..3], &[0, 0, 0], "line art is black");
    // White keeps an opaque frame with white paper.
    let read = scan(&photo, &code.project, None, PANEL, Clean::White, &cancel).unwrap();
    let paper = read.frames[1].image.get_pixel(PANEL.0 / 2, 4).0;
    assert!(
        paper[3] == 255 && paper[..3].iter().all(|v| *v > 225),
        "{paper:?}"
    );
}

#[test]
fn other_storyboards_are_refused_and_missing_codes_need_a_layout() {
    let (sheet, code) = drawn_sheet(false);
    let cancel = AtomicBool::new(false);
    let quad = [(200., 150.), (2200., 200.), (2180., 1620.), (230., 1650.)];
    let photo = photograph(&sheet, quad, (2400, 1800));
    let error = scan(
        &photo,
        "OTHER-PROJECT",
        None,
        PANEL,
        Clean::Transparent,
        &cancel,
    )
    .unwrap_err();
    assert!(matches!(error, ScanError::Foreign(_)), "{error}");
    assert!(error.to_string().contains("another storyboard"));

    let (sheet, _) = drawn_sheet(true);
    let photo = photograph(&sheet, quad, (2400, 1800));
    let error = scan(
        &photo,
        &code.project,
        None,
        PANEL,
        Clean::Transparent,
        &cancel,
    )
    .unwrap_err();
    assert!(matches!(error, ScanError::NoCode), "{error}");
    // With the layout chosen by hand the frames still come back.
    let read = scan(
        &photo,
        &code.project,
        Some(&code),
        PANEL,
        Clean::Transparent,
        &cancel,
    )
    .unwrap();
    assert!(!read.code_read);
    check(&read, &code);
    // A page without corner marks cannot be read.
    let blank = RgbaImage::from_pixel(800, 600, image::Rgba([250, 250, 250, 255]));
    assert!(
        scan(
            &blank,
            &code.project,
            Some(&code),
            PANEL,
            Clean::White,
            &cancel
        )
        .is_err()
    );
}
