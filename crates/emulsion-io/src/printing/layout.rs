use super::*;

impl Rect {
    pub(super) fn inset(self, d: f64) -> Self {
        Self {
            x: self.x + d,
            y: self.y + d,
            w: self.w - 2. * d,
            h: self.h - 2. * d,
        }
    }
    fn intersection(self, other: Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        Self {
            x,
            y,
            w: (self.x + self.w).min(other.x + other.w) - x,
            h: (self.y + self.h).min(other.y + other.h) - y,
        }
    }
}

pub fn layout(sources: &[Source], selected: &[usize], s: &Settings) -> Result<JobLayout> {
    if selected.is_empty() || selected.iter().any(|&i| i >= sources.len()) {
        bail!("Select at least one existing page")
    }
    s.creative.validate()?;
    s.production.validate()?;
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
    let c = &s.creative;
    if s.layout == Layout::Document && c.artwork_mm.is_some() {
        bail!("Custom artwork dimensions require a sheet layout, not document page sizes")
    }
    if s.layout == Layout::Poster && (c.bleed_mm > 0. || c.crop_marks) {
        bail!("Bleed and crop marks require a single-page, contact or repeat layout")
    }
    let mut result = JobLayout {
        sheets: vec![],
        warnings: vec![],
    };
    if s.production.enabled() {
        result.warnings.push(format!("ICC-managed output flattens artwork at {} PPI using the selected profile; preview is an sRGB simulation.",s.production.dpi));
    }
    if c.bleed_mm > 0. {
        result.warnings.push("Bleed uses artwork beyond the page edge. Uncovered areas remain paper white; extend your artwork before trimming.".into());
    }
    if s.layout == Layout::Document {
        if selected.len() > 200 {
            bail!("Print jobs are limited to 200 sheets before copies")
        }
        for &source in selected {
            let (w, h) = sources[source].physical_size()?;
            let pad = c.surround();
            let (width, height) = (w + 2. * pad, h + 2. * pad);
            if width > 2000. || height > 2000. {
                bail!("Document pages exceed 2000 mm; use a paper size or tiled poster layout")
            }
            let trim = Rect {
                x: pad,
                y: pad,
                w,
                h,
            };
            result.sheets.push(Sheet {
                width,
                height,
                printable: Rect {
                    x: 0.,
                    y: 0.,
                    w: width,
                    h: height,
                },
                items: vec![Item {
                    source,
                    bounds: trim,
                    trim,
                    clip: trim.inset(-c.bleed_mm),
                    bleed: c.bleed_mm,
                    crop_marks: c.crop_marks,
                    label: None,
                }],
            });
            quality_warning(&mut result.warnings, &sources[source], 1.);
        }
        return Ok(result);
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
        items: vec![],
    };
    match s.layout {
        Layout::Document => unreachable!(),
        Layout::Single => {
            if selected.len() > 200 {
                bail!("Print jobs are limited to 200 sheets before copies")
            }
            for &source in selected {
                let mut sheet = blank();
                add(
                    &mut sheet,
                    sources,
                    source,
                    printable,
                    s,
                    &mut result.warnings,
                )?;
                result.sheets.push(sheet);
            }
        }
        Layout::Contact | Layout::Repeat => {
            let (cols, rows) = (c.columns as usize, c.rows as usize);
            let count = cols * rows;
            let items = if s.layout == Layout::Repeat {
                vec![selected[0]; count]
            } else {
                selected.to_vec()
            };
            if items.len().div_ceil(count) > 200 {
                bail!("Print jobs are limited to 200 sheets before copies")
            }
            let (w, h) = (
                (printable.w - c.gutter_mm * (cols - 1) as f64) / cols as f64,
                (printable.h - c.gutter_mm * (rows - 1) as f64) / rows as f64,
            );
            if w <= 0. || h <= 0. {
                bail!("Grid and gutters leave no room for artwork")
            }
            for chunk in items.chunks(count) {
                let mut sheet = blank();
                for (i, &source) in chunk.iter().enumerate() {
                    let cell = Rect {
                        x: printable.x + (i % cols) as f64 * (w + c.gutter_mm),
                        y: printable.y + (i / cols) as f64 * (h + c.gutter_mm),
                        w,
                        h,
                    };
                    add(&mut sheet, sources, source, cell, s, &mut result.warnings)?;
                }
                result.sheets.push(sheet);
            }
        }
        Layout::Poster => {
            if selected.len() != 1 {
                bail!("Select one source page for a tiled poster")
            }
            let source = selected[0];
            let natural = sources[source].physical_size()?;
            let k = c
                .artwork_mm
                .map(|[w, h]| (w / natural.0).min(h / natural.1))
                .unwrap_or(s.scale / 100.);
            let (w, h) = (natural.0 * k, natural.1 * k);
            quality_warning(&mut result.warnings, &sources[source], k);
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
                        trim: printable,
                        bleed: 0.,
                        crop_marks: false,
                        label: None,
                    });
                    result.sheets.push(sheet);
                }
            }
        }
    }
    Ok(result)
}

fn quality_warning(warnings: &mut Vec<String>, source: &Source, k: f64) {
    if source.rasterized && source.ppi / k < 150. {
        let warning = format!(
            "{}: {:.0} effective PPI; the print may look soft.",
            source.name,
            source.ppi / k
        );
        if !warnings.contains(&warning) {
            warnings.push(warning);
        }
    }
}

fn add(
    sheet: &mut Sheet,
    sources: &[Source],
    source: usize,
    cell: Rect,
    s: &Settings,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let doc = &sources[source];
    let natural = doc.physical_size()?;
    let c = &s.creative;
    let label =
        if matches!(s.layout, Layout::Contact | Layout::Repeat) && c.labels != LabelMode::None {
            let text = match c.labels {
                LabelMode::NumberAndName => format!("{} · {}", source + 1, doc.name),
                _ => doc.name.clone(),
            };
            Some(Label {
                text,
                bounds: Rect {
                    x: cell.x,
                    y: cell.y + cell.h - 6.,
                    w: cell.w,
                    h: 6.,
                },
            })
        } else {
            None
        };
    let cell = Rect {
        h: cell.h - if label.is_some() { 6. } else { 0. },
        ..cell
    };
    let area = cell.inset(c.surround());
    if area.w <= 0. || area.h <= 0. {
        bail!("Bleed and crop marks leave no room for artwork")
    }
    let target = if let Some([w, h]) = c.artwork_mm {
        if w > area.w + 0.001 || h > area.h + 0.001 {
            bail!(
                "Custom artwork, bleed and marks do not fit this paper/grid. Choose larger paper, fewer cells or smaller artwork."
            )
        }
        Rect {
            x: area.x + (area.w - w) / 2.,
            y: area.y + (area.h - h) / 2.,
            w,
            h,
        }
    } else {
        area
    };
    let k = match s.placement {
        Placement::Fit => (target.w / natural.0).min(target.h / natural.1),
        Placement::Fill => (target.w / natural.0).max(target.h / natural.1),
        Placement::Actual => s.scale / 100.,
    };
    let (w, h) = (natural.0 * k, natural.1 * k);
    let bounds = Rect {
        x: target.x + (target.w - w) * c.crop[0],
        y: target.y + (target.h - h) * c.crop[1],
        w,
        h,
    };
    let cropped = w > target.w + 0.01 || h > target.h + 0.01;
    let warning = "Artwork extends beyond its print area and will be cropped.";
    if cropped && !warnings.iter().any(|v| v == warning) {
        warnings.push(warning.into());
    }
    quality_warning(warnings, doc, k);
    let trim = if c.artwork_mm.is_some() || s.placement == Placement::Fill {
        target
    } else {
        bounds.intersection(target)
    };
    sheet.items.push(Item {
        source,
        bounds,
        trim,
        clip: trim.inset(-c.bleed_mm),
        bleed: c.bleed_mm,
        crop_marks: c.crop_marks,
        label,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> Source {
        Source {
            name: "Wide photo".into(),
            width: 2000,
            height: 1000,
            ppi: 254.,
            svg: String::new(),
            rasterized: true,
            document: None,
            original_paths: vec![],
        }
    }
    #[test]
    fn custom_box_has_exact_size_and_fill_crop_can_reach_both_edges() {
        let mut s = Settings {
            placement: Placement::Fill,
            ..Default::default()
        };
        s.creative.artwork_mm = Some([100., 100.]);
        for x in [0., 0.5, 1.] {
            s.creative.crop = [x, 0.5];
            let job = layout(&[source()], &[0], &s).unwrap();
            let item = &job.sheets[0].items[0];
            assert_eq!((item.trim.w, item.trim.h), (100., 100.));
            assert_eq!((item.bounds.w, item.bounds.h), (200., 100.));
            assert!((item.bounds.x - (item.trim.x - 100. * x)).abs() < 1e-9);
        }
        s.placement = Placement::Fit;
        let job = layout(&[source()], &[0], &s).unwrap();
        assert_eq!(
            (
                job.sheets[0].items[0].bounds.w,
                job.sheets[0].items[0].bounds.h
            ),
            (100., 50.)
        );
        s.creative.artwork_mm = Some([201., 100.]);
        assert!(layout(&[source()], &[0], &s).is_err());
    }
    #[test]
    fn variable_grids_paginate_in_order_and_repeat_only_the_first_source() {
        let sources = vec![source(); 9];
        let mut s = Settings {
            layout: Layout::Contact,
            ..Default::default()
        };
        s.creative.rows = 2;
        s.creative.columns = 2;
        s.creative.gutter_mm = 10.;
        let job = layout(&sources, &(0..9).collect::<Vec<_>>(), &s).unwrap();
        assert_eq!(
            job.sheets.iter().map(|s| s.items.len()).collect::<Vec<_>>(),
            vec![4, 4, 1]
        );
        assert_eq!(job.sheets[1].items[0].source, 4);
        assert!(
            (job.sheets[0].items[1].trim.x - job.sheets[0].items[0].trim.x - 105.).abs() < 1e-9
        );
        s.layout = Layout::Repeat;
        let job = layout(&sources, &[3, 1], &s).unwrap();
        assert_eq!(job.sheets.len(), 1);
        assert_eq!(job.sheets[0].items.len(), 4);
        assert!(job.sheets[0].items.iter().all(|i| i.source == 3));
        s.creative.columns = 20;
        s.creative.gutter_mm = 100.;
        assert!(layout(&sources, &[0], &s).is_err());
        s.creative.rows = 0;
        assert!(layout(&sources, &[0], &s).is_err());
    }
    #[test]
    fn marks_and_bleed_reserve_physical_space_without_scaling_document_trim() {
        let mut s = Settings {
            layout: Layout::Document,
            ..Default::default()
        };
        s.creative.bleed_mm = 3.;
        s.creative.crop_marks = true;
        let job = layout(&[source()], &[0], &s).unwrap();
        let page = &job.sheets[0];
        assert_eq!((page.width, page.height), (220., 120.));
        let item = &page.items[0];
        assert_eq!(
            (item.trim.x, item.trim.y, item.trim.w, item.trim.h),
            (10., 10., 200., 100.)
        );
        assert_eq!(
            (item.clip.x, item.clip.y, item.clip.w, item.clip.h),
            (7., 7., 206., 106.)
        );
        s.layout = Layout::Single;
        s.creative.artwork_mm = Some([190., 100.]);
        assert!(
            layout(&[source()], &[0], &s).is_err(),
            "no silent scaling of custom artwork to fit marks"
        );
        s.layout = Layout::Poster;
        assert!(layout(&[source()], &[0], &s).is_err());
    }
    #[test]
    fn nonfinite_and_out_of_range_creative_controls_are_rejected() {
        for c in [
            CreativeSettings {
                crop: [f64::NAN, 0.5],
                ..Default::default()
            },
            CreativeSettings {
                bleed_mm: -1.,
                ..Default::default()
            },
            CreativeSettings {
                artwork_mm: Some([0., 100.]),
                ..Default::default()
            },
            CreativeSettings {
                gutter_mm: f64::INFINITY,
                ..Default::default()
            },
        ] {
            assert!(
                layout(
                    &[source()],
                    &[0],
                    &Settings {
                        creative: c,
                        ..Default::default()
                    }
                )
                .is_err()
            );
        }
    }
}

#[cfg(test)]
mod label_tests {
    use super::*;
    #[test]
    fn contact_labels_reserve_space_outside_artwork_bleed_and_marks() {
        let source = Source {
            name: "long <filename> & text.png".into(),
            width: 100,
            height: 100,
            ppi: 100.,
            svg: String::new(),
            rasterized: false,
            document: None,
            original_paths: vec![],
        };
        let mut s = Settings {
            layout: Layout::Contact,
            ..Default::default()
        };
        s.creative.labels = LabelMode::NumberAndName;
        s.creative.crop_marks = true;
        s.creative.bleed_mm = 3.;
        let job = layout(&[source], &[0], &s).unwrap();
        let item = &job.sheets[0].items[0];
        let label = item.label.as_ref().unwrap();
        assert_eq!(label.bounds.h, 6.);
        assert!(item.clip.y + item.clip.h + 7. <= label.bounds.y + 0.001);
        assert!(label.text.starts_with("1 · long <filename>"));
        assert!(
            label.bounds.y + label.bounds.h
                <= job.sheets[0].printable.y + job.sheets[0].printable.h
        );
    }
}
