//! Profile conversion and deliberately flattened, self-contained PDF/X output.
//! The regular PDF path retains vectors. Press PDFs contain only opaque CMYK
//! images, the supplied output profile and explicit media/trim/bleed boxes.
use super::*;
use moxcms::{ColorProfile, DataColorSpace, Layout as PixelLayout, ProfileClass, TransformOptions};
use std::{
    io::{Read, Write},
    path::PathBuf,
};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PdfStandard {
    #[default]
    Pdf,
    PdfX1a2001,
    PdfX32002,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Production {
    pub managed: bool,
    pub profile: Option<PathBuf>,
    /// ICC intent: perceptual, relative, saturation, absolute (0..3).
    pub intent: u8,
    pub dpi: u32,
    pub standard: PdfStandard,
    pub condition: String,
    /// Explicit driver setting, never inferred from the selected ICC file.
    pub driver_color_disabled: bool,
}
impl Default for Production {
    fn default() -> Self {
        Self {
            managed: false,
            profile: None,
            intent: 1,
            dpi: 300,
            standard: PdfStandard::Pdf,
            condition: "Custom print condition".into(),
            driver_color_disabled: false,
        }
    }
}
impl Production {
    pub fn enabled(&self) -> bool {
        self.managed || self.standard != PdfStandard::Pdf
    }
    pub fn validate(&self) -> Result<()> {
        if self.intent > 3
            || !(150..=600).contains(&self.dpi)
            || self.condition.trim().is_empty()
            || self.condition.len() > 200
            || self.condition.chars().any(char::is_control)
        {
            bail!(
                "Use a rendering intent from 0–3, 150–600 PPI and a print condition name of 1–200 characters"
            )
        }
        if self.enabled() && self.profile.is_none() {
            bail!("Choose an output ICC profile")
        }
        Ok(())
    }
    pub fn validate_device(&self, portal: bool) -> Result<()> {
        self.validate()?;
        if self.standard != PdfStandard::Pdf {
            bail!("PDF/X is a Save PDF destination; choose ordinary PDF for a printer queue")
        }
        if self.enabled() && (portal || !self.driver_color_disabled) {
            bail!(
                "App-managed printer output requires a native queue with driver color correction disabled. The system print portal cannot verify this; use Save PDF or printer-managed color."
            )
        }
        Ok(())
    }
}
pub struct Profile {
    bytes: Vec<u8>,
    profile: ColorProfile,
    channels: usize,
    intent: moxcms::RenderingIntent,
}
impl Profile {
    pub fn load(settings: &Production) -> Result<Self> {
        settings.validate()?;
        let path = settings
            .profile
            .as_ref()
            .context("Choose an output ICC profile")?;
        let mut bytes = vec![];
        std::fs::File::open(path)?
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 4 * 1024 * 1024 || bytes.len() < 128 {
            bail!("ICC profile must be between 128 bytes and 4 MiB")
        }
        let profile = ColorProfile::new_from_slice(&bytes).context("Invalid output ICC profile")?;
        let channels = match profile.color_space {
            DataColorSpace::Rgb => 3,
            DataColorSpace::Cmyk => 4,
            _ => bail!("Choose an RGB or CMYK output profile"),
        };
        if settings.standard != PdfStandard::Pdf
            && (channels != 4
                || profile.profile_class != ProfileClass::OutputDevice
                || bytes[8] != 2)
        {
            bail!(
                "PDF/X-1a:2001 and PDF/X-3:2002 require a CMYK ICC v2 output-device profile in this workflow"
            )
        }
        let intent = match settings.intent {
            0 => moxcms::RenderingIntent::Perceptual,
            1 => moxcms::RenderingIntent::RelativeColorimetric,
            2 => moxcms::RenderingIntent::Saturation,
            _ => moxcms::RenderingIntent::AbsoluteColorimetric,
        };
        // Fail before output if the profile has no usable PCS-to-device transform.
        ColorProfile::new_srgb().create_transform_8bit(
            PixelLayout::Rgb,
            &profile,
            if channels == 4 {
                PixelLayout::Rgba
            } else {
                PixelLayout::Rgb
            },
            TransformOptions {
                rendering_intent: intent,
                ..Default::default()
            },
        )?;
        Ok(Self {
            bytes,
            profile,
            channels,
            intent,
        })
    }
    fn transform(&self, image: &image::RgbaImage) -> Result<Vec<u8>> {
        let rgb = image
            .as_raw()
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| p[..3].iter().copied())
            .collect::<Vec<_>>();
        let mut out = vec![0; image.width() as usize * image.height() as usize * self.channels];
        let transform = ColorProfile::new_srgb().create_transform_8bit(
            PixelLayout::Rgb,
            &self.profile,
            if self.channels == 4 {
                PixelLayout::Rgba
            } else {
                PixelLayout::Rgb
            },
            TransformOptions {
                rendering_intent: self.intent,
                ..Default::default()
            },
        )?;
        transform.transform(&rgb, &mut out)?;
        Ok(out)
    }
    fn proof(&self, image: &mut image::RgbaImage) -> Result<()> {
        let device = self.transform(image)?;
        let transform = self.profile.create_transform_8bit(
            if self.channels == 4 {
                PixelLayout::Rgba
            } else {
                PixelLayout::Rgb
            },
            &ColorProfile::new_srgb(),
            PixelLayout::Rgb,
            TransformOptions {
                rendering_intent: self.intent,
                ..Default::default()
            },
        )?;
        let mut rgb = vec![0; image.width() as usize * image.height() as usize * 3];
        transform.transform(&device, &mut rgb)?;
        for (rgba, rgb) in image
            .as_mut()
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(rgb.as_chunks::<3>().0)
        {
            rgba[..3].copy_from_slice(rgb);
        }
        Ok(())
    }
    fn validate_printer(&self) -> Result<()> {
        if self.channels != 3 || self.profile.profile_class != ProfileClass::OutputDevice {
            bail!(
                "Native app-managed printing requires an RGB printer output profile. Export CMYK profiles to PDF for a press workflow."
            )
        }
        Ok(())
    }
}
pub fn validate_device_profile(settings: &Settings) -> Result<()> {
    settings.production.validate_device(false)?;
    if settings.production.enabled() {
        Profile::load(&settings.production)?.validate_printer()?;
    }
    Ok(())
}
pub fn preview(
    sources: &[Source],
    sheet: &Sheet,
    s: &Settings,
    max_side: u32,
) -> Result<image::RgbaImage> {
    s.production.validate()?;
    let mut image = super::preview(sources, sheet, s.grayscale, max_side)?;
    if s.production.enabled() {
        Profile::load(&s.production)?.proof(&mut image)?;
    }
    Ok(image)
}
/// Device RGB image with correction already applied. The caller disables ICM.
pub fn device_image(
    sources: &[Source],
    sheet: &Sheet,
    s: &Settings,
    max_side: u32,
) -> Result<image::RgbaImage> {
    s.production.validate_device(false)?;
    let mut image = super::preview(sources, sheet, s.grayscale, max_side)?;
    if s.production.enabled() {
        let p = Profile::load(&s.production)?;
        p.validate_printer()?;
        let rgb = p.transform(&image)?;
        for (rgba, rgb) in image
            .as_mut()
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(rgb.as_chunks::<3>().0)
        {
            rgba[..3].copy_from_slice(rgb);
        }
    }
    Ok(image)
}
pub fn write_pdf(
    sources: &[Source],
    job: &JobLayout,
    s: &Settings,
    path: &Path,
    cancel: &AtomicBool,
) -> Result<()> {
    write(sources, job, s, path, cancel, false)
}
pub(super) fn write_device_pdf(
    sources: &[Source],
    job: &JobLayout,
    s: &Settings,
    path: &Path,
    cancel: &AtomicBool,
) -> Result<()> {
    s.production.validate_device(false)?;
    write(sources, job, s, path, cancel, true)
}
fn write(
    sources: &[Source],
    job: &JobLayout,
    s: &Settings,
    path: &Path,
    cancel: &AtomicBool,
    device: bool,
) -> Result<()> {
    s.production.validate()?;
    if !s.production.enabled() {
        return super::write_pdf(sources, job, s.grayscale, path, cancel);
    }
    if job.sheets.is_empty() || job.sheets.len() > 200 {
        bail!("Print jobs require 1–200 sheets")
    }
    let profile = Profile::load(&s.production)?;
    if device {
        profile.validate_printer()?;
    }
    let destination = path.canonicalize().ok();
    for original in sources
        .iter()
        .flat_map(|s| &s.original_paths)
        .chain(s.production.profile.iter())
    {
        if original == path
            || destination
                .as_ref()
                .is_some_and(|p| original.canonicalize().ok().as_ref() == Some(p))
        {
            bail!("Print output cannot overwrite a source file or ICC profile")
        }
    }
    let bytes = pdf(sources, job, s, &profile, cancel, device)?;
    canceled(cancel)?;
    crate::write_atomic(path, |file| {
        file.write_all(&bytes)?;
        Ok(())
    })?;
    Ok(())
}
fn pdf(
    sources: &[Source],
    job: &JobLayout,
    s: &Settings,
    profile: &Profile,
    cancel: &AtomicBool,
    device: bool,
) -> Result<Vec<u8>> {
    use pdf_writer::types::{OutputIntentSubtype, TrappingStatus};
    use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref, Str, TextStr};
    let press = s.production.standard != PdfStandard::Pdf;
    let mut pdf = Pdf::new();
    pdf.set_version(1, if press { 3 } else { 7 });
    let mut alloc = Ref::new(1);
    let catalog = alloc.bump();
    let pages = alloc.bump();
    let info = alloc.bump();
    let icc = alloc.bump();
    let mut id = [0u8; 16];
    getrandom::fill(&mut id).map_err(|e| anyhow::anyhow!("Cannot generate PDF ID: {e}"))?;
    pdf.set_file_id((id.to_vec(), id.to_vec()));
    let mut out = pdf.document_info(info);
    out.title(TextStr("Emulsion print output"))
        .creator(TextStr("Emulsion"))
        .producer(TextStr("Emulsion profile-managed print compositor"))
        .creation_date(date())
        .modified_date(date())
        .trapped(TrappingStatus::NotTrapped);
    match s.production.standard {
        PdfStandard::PdfX1a2001 => {
            out.pair(Name(b"GTS_PDFXVersion"), Str(b"PDF/X-1:2001"));
            out.pair(Name(b"GTS_PDFXConformance"), Str(b"PDF/X-1a:2001"));
        }
        PdfStandard::PdfX32002 => {
            out.pair(Name(b"GTS_PDFXVersion"), Str(b"PDF/X-3:2002"));
        }
        _ => {}
    }
    out.finish();
    pdf.icc_profile(icc, &profile.bytes)
        .n(profile.channels as i32)
        .finish();
    let mut root = pdf.catalog(catalog);
    root.pages(pages);
    if !device {
        let mut intents = root.output_intents();
        let mut intent = intents.push();
        intent
            .subtype(if press {
                OutputIntentSubtype::PDFX
            } else {
                OutputIntentSubtype::Custom(Name(b"EmulsionICC"))
            })
            .output_condition_identifier(TextStr(&s.production.condition))
            .info(TextStr(&s.production.condition))
            .dest_output_profile(icc);
        intent.finish();
        intents.finish();
    }
    root.finish();
    let mut ids = vec![];
    for sheet in &job.sheets {
        canceled(cancel)?;
        let max_side =
            (sheet.width.max(sheet.height) / 25.4 * f64::from(s.production.dpi)).ceil() as u32;
        let image = super::preview(sources, sheet, s.grayscale, max_side)?;
        let ink = profile.transform(&image)?;
        let mut zip = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        zip.write_all(&ink)?;
        let compressed = zip.finish()?;
        let image_id = alloc.bump();
        let page = alloc.bump();
        let content = alloc.bump();
        ids.push(page);
        let mut image_obj = pdf.image_xobject(image_id, &compressed);
        image_obj
            .width(image.width() as i32)
            .height(image.height() as i32)
            .bits_per_component(8)
            .filter(pdf_writer::Filter::FlateDecode);
        if press {
            image_obj.color_space().device_cmyk();
        } else if device {
            image_obj.color_space().device_rgb();
        } else {
            image_obj.color_space().icc_based(icc);
        }
        image_obj.finish();
        let k = (72. / 25.4) as f32;
        let w = sheet.width as f32 * k;
        let h = sheet.height as f32 * k;
        let pdf_rect = |r: super::Rect| {
            Rect::new(
                r.x as f32 * k,
                (sheet.height - r.y - r.h) as f32 * k,
                (r.x + r.w) as f32 * k,
                (sheet.height - r.y) as f32 * k,
            )
        };
        let mut out = pdf.page(page);
        out.parent(pages)
            .media_box(Rect::new(0., 0., w, h))
            .contents(content);
        let (trim, bleed) = if let [item] = sheet.items.as_slice() {
            (item.trim, item.clip)
        } else {
            (sheet.printable, sheet.printable)
        };
        out.trim_box(pdf_rect(trim)).bleed_box(pdf_rect(bleed));
        out.resources().x_objects().pair(Name(b"Artwork"), image_id);
        out.finish();
        let mut commands = Content::new();
        commands
            .transform([w, 0., 0., h, 0., 0.])
            .x_object(Name(b"Artwork"));
        pdf.stream(content, &commands.finish());
        if pdf.len() > 512 * 1024 * 1024 {
            bail!("Managed PDF exceeds 512 MiB; choose fewer pages or lower output PPI")
        }
    }
    pdf.pages(pages)
        .kids(ids.iter().copied())
        .count(ids.len() as i32);
    canceled(cancel)?;
    Ok(pdf.finish())
}
fn date() -> pdf_writer::Date {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut days = seconds / 86400;
    let mut year = 1970u16;
    let leap = |y: u16| y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400));
    while year < 9999 {
        let n = if leap(year) { 366 } else { 365 };
        if days < n {
            break;
        }
        days -= n;
        year += 1;
    }
    let months = [
        31,
        if leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 0;
    while month < 11 && days >= months[month] {
        days -= months[month];
        month += 1;
    }
    pdf_writer::Date::new(year)
        .month(month as u8 + 1)
        .day(days as u8 + 1)
        .hour((seconds / 3600 % 24) as u8)
        .minute((seconds / 60 % 60) as u8)
        .second((seconds % 60) as u8)
        .utc_offset_hour(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> Source {
        Source { name:"Proof".into(),width:100,height:60,ppi:254.,svg:r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="60"><rect width="100" height="60" fill="#ff8030"/></svg>"##.into(),rasterized:false,document:None,original_paths:vec![] }
    }
    pub(super) fn cmyk_profile() -> Vec<u8> {
        fn lut(input: u8, output: u8, values: &[u8]) -> Vec<u8> {
            let mut bytes = b"mft1\0\0\0\0".to_vec();
            bytes.extend([input, output, 2, 0]);
            for i in 0..9 {
                bytes.extend((if i % 4 == 0 { 65536u32 } else { 0 }).to_be_bytes());
            }
            for _ in 0..input {
                bytes.extend(0..=255u8);
            }
            for _ in 0..(1 << input) {
                bytes.extend_from_slice(values);
            }
            for _ in 0..output {
                bytes.extend(0..=255u8);
            }
            bytes
        }
        let mut profile = crate::icc::srgb_profile().unwrap()[..128].to_vec();
        profile[8..12].copy_from_slice(&[2, 0x10, 0, 0]);
        profile[12..16].copy_from_slice(b"prtr");
        profile[16..20].copy_from_slice(b"CMYK");
        let tags = [
            (*b"A2B0", lut(4, 3, &[0, 0, 0])),
            (*b"A2B1", lut(4, 3, &[0, 0, 0])),
            (*b"A2B2", lut(4, 3, &[0, 0, 0])),
            (*b"B2A0", lut(3, 4, &[0, 0, 0, 255])),
            (*b"B2A1", lut(3, 4, &[0, 0, 0, 255])),
            (*b"B2A2", lut(3, 4, &[0, 0, 0, 255])),
        ];
        profile.extend((tags.len() as u32).to_be_bytes());
        let mut offset = 132 + 12 * tags.len() as u32;
        for (name, bytes) in &tags {
            profile.extend(name);
            profile.extend(offset.to_be_bytes());
            profile.extend((bytes.len() as u32).to_be_bytes());
            offset += bytes.len() as u32;
        }
        for (_, bytes) in tags {
            profile.extend(bytes);
        }
        let len = profile.len() as u32;
        profile[..4].copy_from_slice(&len.to_be_bytes());
        profile
    }
    #[test]
    fn rgb_output_is_profile_tagged_and_cancel_does_not_replace_destination() {
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path().join("p3.icc");
        std::fs::write(&profile, ColorProfile::new_display_p3().encode().unwrap()).unwrap();
        let mut s = Settings {
            layout: Layout::Document,
            ..Default::default()
        };
        s.production.managed = true;
        s.production.profile = Some(profile);
        let sources = [source()];
        let job = super::super::layout(&sources, &[0], &s).unwrap();
        let path = dir.path().join("managed.pdf");
        std::fs::write(&path, b"existing").unwrap();
        assert!(write_pdf(&sources, &job, &s, &path, &AtomicBool::new(true)).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"existing");
        write_pdf(&sources, &job, &s, &path, &AtomicBool::new(false)).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/ICCBased"));
        assert!(text.contains("/N 3"));
        assert!(!text.contains("GTS_PDFXVersion"));
        let p = Profile::load(&s.production).unwrap();
        let input = image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 80, 0, 255]));
        assert_ne!(p.transform(&input).unwrap(), vec![255, 80, 0]);
        assert!(s.production.validate_device(false).is_err());
        s.production.driver_color_disabled = true;
        assert!(
            p.validate_printer().is_err(),
            "display profile cannot impersonate printer profile"
        );
        assert!(
            s.production.validate_device(true).is_err(),
            "portal has no verified bypass"
        );
        // Native queues receive corrected device values, not a reverse soft
        // proof or an ICC-tagged image that a driver might convert again.
        let mut printer_profile = ColorProfile::new_display_p3().encode().unwrap();
        printer_profile[12..16].copy_from_slice(b"prtr");
        std::fs::write(s.production.profile.as_ref().unwrap(), printer_profile).unwrap();
        validate_device_profile(&s).unwrap();
        let raw = super::super::preview(&sources, &job.sheets[0], false, 80).unwrap();
        let expected = Profile::load(&s.production)
            .unwrap()
            .transform(&raw)
            .unwrap();
        let device = device_image(&sources, &job.sheets[0], &s, 80).unwrap();
        assert_eq!(
            device
                .as_raw()
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| p[..3].iter().copied())
                .collect::<Vec<_>>(),
            expected
        );
        write_device_pdf(&sources, &job, &s, &path, &AtomicBool::new(false)).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let pdf = String::from_utf8_lossy(&bytes);
        assert!(pdf.contains("/DeviceRGB"));
        assert!(!pdf.contains("/ICCBased"));
        assert!(!pdf.contains("/OutputIntents"));
    }
    #[test]
    fn press_output_has_real_cmyk_pixels_output_intent_boxes_and_pdfx_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path().join("press.icc");
        std::fs::write(&profile, cmyk_profile()).unwrap();
        let sources = [source()];
        for standard in [PdfStandard::PdfX1a2001, PdfStandard::PdfX32002] {
            let mut s = Settings {
                layout: Layout::Document,
                ..Default::default()
            };
            s.production.standard = standard;
            s.production.profile = Some(profile.clone());
            s.creative.bleed_mm = 1.;
            s.creative.crop_marks = true;
            let p = Profile::load(&s.production).unwrap();
            assert_eq!(
                p.transform(&image::RgbaImage::from_pixel(
                    1,
                    1,
                    image::Rgba([255, 255, 255, 255])
                ))
                .unwrap(),
                vec![0, 0, 0, 255]
            );
            let job = super::super::layout(&sources, &[0], &s).unwrap();
            let path = dir.path().join(format!("{standard:?}.pdf"));
            write_pdf(&sources, &job, &s, &path, &AtomicBool::new(false)).unwrap();
            let bytes = std::fs::read(path).unwrap();
            let pdf = String::from_utf8_lossy(&bytes);
            assert!(pdf.starts_with("%PDF-1.3"));
            for required in [
                "/GTS_PDFXVersion",
                "/GTS_PDFX",
                "/DeviceCMYK",
                "/DestOutputProfile",
                "/N 4",
                "/Trapped /False",
                "/TrimBox",
                "/BleedBox",
                "/CreationDate",
                "/ModDate",
                "/ID",
            ] {
                assert!(pdf.contains(required), "Missing {required}");
            }
            for forbidden in ["/DeviceRGB", "/SMask", "/Font", "/Encrypt", "/JavaScript"] {
                assert!(!pdf.contains(forbidden), "Unexpected {forbidden}");
            }
            assert!(s.production.validate_device(false).is_err());
        }
    }
    #[test]
    fn invalid_or_wrong_press_profiles_and_bad_settings_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("output.icc");
        std::fs::write(&path, crate::icc::srgb_profile().unwrap()).unwrap();
        let mut s = Production {
            standard: PdfStandard::PdfX1a2001,
            profile: Some(path.clone()),
            ..Default::default()
        };
        assert!(Profile::load(&s).is_err());
        std::fs::write(path, b"bad data").unwrap();
        assert!(Profile::load(&s).is_err());
        s.dpi = 0;
        assert!(s.validate().is_err());
        s.dpi = 300;
        s.intent = 4;
        assert!(s.validate().is_err());
    }
}
