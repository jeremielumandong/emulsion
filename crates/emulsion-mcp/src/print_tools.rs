//! Bounded native printer discovery, preview and explicit queue submission.
use crate::{ToolDef, ToolResult};
use emulsion_io::printing as print;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::atomic::AtomicBool;
pub const READ_ONLY: &[&str] = &[
    "list_printers",
    "list_print_presets",
    "get_printer_capabilities",
    "preview_print_job",
];
pub const DESTRUCTIVE: &[&str] = &[
    "submit_print_job",
    "export_print_pdf",
    "save_print_preset",
    "delete_print_preset",
];
pub fn is_tool(name: &str) -> bool {
    READ_ONLY.contains(&name) || DESTRUCTIVE.contains(&name)
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    pub output_path: Option<String>,
    pub production: Option<print::production::Production>,
    pub labels: Option<print::LabelMode>,
    pub photos: Option<Vec<print::sources::PhotoInput>>,
    pub video_path: Option<String>,
    pub frame_times_ms: Option<Vec<u32>>,
    pub source_page: Option<usize>,
    pub frame_nodes: Option<Vec<u64>>,
    pub preset_name: Option<String>,
    pub artwork_width_mm: Option<f64>,
    pub artwork_height_mm: Option<f64>,
    pub rows: Option<u16>,
    pub columns: Option<u16>,
    pub gutter_mm: Option<f64>,
    pub crop_x_percent: Option<f64>,
    pub crop_y_percent: Option<f64>,
    pub bleed_mm: Option<f64>,
    pub crop_marks: Option<bool>,
    pub printer: Option<String>,
    pub pages: Option<String>,
    pub paper: Option<String>,
    pub landscape: Option<bool>,
    pub placement: Option<String>,
    pub layout: Option<String>,
    pub scale: Option<f64>,
    pub margin_mm: Option<f64>,
    pub overlap_mm: Option<f64>,
    pub copies: Option<u16>,
    pub grayscale: Option<bool>,
    pub media: Option<String>,
    pub tray: Option<String>,
    pub quality: Option<String>,
    pub sides: Option<String>,
    pub sheet: Option<usize>,
    pub expected_revision: Option<u64>,
}
pub fn definitions() -> Vec<ToolDef> {
    let mut settings = json!({"printer":{"type":"string","minLength":1,"maxLength":512},"pages":{"type":"string","description":"1-based page range, e.g. 1,3-5; omitted means all pages"},"paper":{"type":"string"},"landscape":{"type":"boolean"},"placement":{"enum":["fit","fill","actual"]},"layout":{"enum":["document","single","contact","repeat","poster"]},"scale":{"type":"number","minimum":1,"maximum":1000},"margin_mm":{"type":"number","minimum":0,"maximum":100},"overlap_mm":{"type":"number","minimum":0,"maximum":50},"copies":{"type":"integer","minimum":1,"maximum":999},"grayscale":{"type":"boolean"},"media":{"type":"string"},"tray":{"type":"string"},"quality":{"type":"string"},"sides":{"type":"string"},"sheet":{"type":"integer","minimum":0},"expected_revision":{"type":"integer","minimum":0}});
    let extra = json!({"preset_name":{"type":"string","minLength":1,"maxLength":100,"description":"Saved local layout preset name; for saving, replaces a preset with the same name."},
        "artwork_width_mm":{"type":"number","minimum":1,"maximum":2000},"artwork_height_mm":{"type":"number","minimum":1,"maximum":2000},
        "rows":{"type":"integer","minimum":1,"maximum":20},"columns":{"type":"integer","minimum":1,"maximum":20},
        "gutter_mm":{"type":"number","minimum":0,"maximum":100},"crop_x_percent":{"type":"number","minimum":0,"maximum":100},
        "crop_y_percent":{"type":"number","minimum":0,"maximum":100},"bleed_mm":{"type":"number","minimum":0,"maximum":20},"crop_marks":{"type":"boolean"}});
    let advanced = json!({
        "output_path":{"type":"string","minLength":1},
        "production":{"type":"object","additionalProperties":false,"properties":{
            "managed":{"type":"boolean"},"profile":{"type":"string"},"intent":{"type":"integer","minimum":0,"maximum":3},
            "dpi":{"type":"integer","minimum":150,"maximum":600},"standard":{"enum":["pdf","pdf_x1a2001","pdf_x32002"]},
            "condition":{"type":"string","minLength":1,"maxLength":200},"driver_color_disabled":{"type":"boolean"}}},
        "labels":{"enum":["none","name","number_and_name"]},
        "photos":{"type":"array","minItems":1,"maxItems":200,"items":{"type":"object","additionalProperties":false,"required":["path"],"properties":{
            "path":{"type":"string","minLength":1},"params":{"type":"object","description":"Native DevelopParams snapshot, including unsaved Library settings."},"expected_digest":{"type":"string"}}}},
        "video_path":{"type":"string","minLength":1,"description":"Local video file; requires installed FFmpeg and frame_times_ms."},
        "frame_times_ms":{"type":"array","minItems":1,"maxItems":100,"items":{"type":"integer","minimum":0,"maximum":86400000}},
        "frame_nodes":{"type":"array","minItems":1,"maxItems":200,"items":{"type":"integer","minimum":1},"description":"Responsive Design frame IDs to print individually."},
        "source_page":{"type":"integer","minimum":0,"description":"Zero-based source page for animation frames; defaults to first page."}
    });
    settings
        .as_object_mut()
        .unwrap()
        .extend(advanced.as_object().unwrap().clone());
    settings
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    [
 ("export_print_pdf","Write composed print sheets to PDF. Supports edited photo snapshots, local-video or animation frame timestamps, labels, ICC conversion and flattened CMYK PDF/X. Never submits to a printer.",settings.clone(),vec!["output_path"]),
 ("list_print_presets","Read local named print layout presets without printer I/O.",json!({}),vec![]),
 ("save_print_preset","Save or replace a local creative print layout preset. Printer-specific options and copies are not saved. Never submits a print job.",settings.clone(),vec!["preset_name"]),
 ("delete_print_preset","Delete a named local print layout preset. Never submits a print job.",json!({"preset_name":{"type":"string","minLength":1,"maxLength":100}}),vec!["preset_name"]),
 ("list_printers","Discover installed native print queues without submitting a job.",json!({}),vec![]),
 ("get_printer_capabilities","Read native paper, tray, quality, duplex and color choices for an installed printer.",json!({"printer":{"type":"string","minLength":1,"maxLength":512}}),vec!["printer"]),
 ("preview_print_job","Render a bounded PNG preview from an immutable originating-project snapshot using the same physical sheet layout as native printing. Printer omitted uses generic PDF paper sizes; layout=document preserves each page physical size without scaling or margins. Sheet is zero-based.",settings.clone(),vec![]),
 ("submit_print_job","Explicitly submit the originating-project snapshot to an installed OS print queue. Returns queue acceptance, never a claim of physical completion. Device/paper choices are validated; expected_revision protects the active-page snapshot.",settings,vec!["printer"]),
 ].into_iter().map(|(name,description,properties,required)|ToolDef{name:name.into(),description:description.into(),input_schema:json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})}).collect()
}
pub fn parse(name: &str, args: &Value) -> Result<Options, String> {
    let object = args.as_object().ok_or("Expected an argument object.")?;
    if object.values().any(Value::is_null) {
        return Err("Omit unchanged print options instead of null.".into());
    }
    if ["list_printers", "list_print_presets"].contains(&name) && !object.is_empty() {
        return Err("Printer listing takes no arguments.".into());
    }
    if name == "get_printer_capabilities" && object.keys().any(|k| k != "printer") {
        return Err("Capabilities accepts only printer.".into());
    }
    if name == "delete_print_preset" && object.keys().any(|k| k != "preset_name") {
        return Err("Delete preset accepts only preset_name.".into());
    }
    let options: Options = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
    if ["get_printer_capabilities", "submit_print_job"].contains(&name) && options.printer.is_none()
    {
        return Err("Select an installed printer.".into());
    }
    if ["save_print_preset", "delete_print_preset"].contains(&name) && options.preset_name.is_none()
    {
        return Err("Enter a preset name.".into());
    }
    if options
        .preset_name
        .as_ref()
        .is_some_and(|v| v.trim().is_empty() || v.len() > 100 || v.chars().any(char::is_control))
    {
        return Err("Use a preset name of 1–100 characters without control characters.".into());
    }
    creative(&options, print::CreativeSettings::default())?
        .validate()
        .map_err(|e| e.to_string())?;
    if name == "export_print_pdf" && (options.output_path.is_none() || options.printer.is_some()) {
        return Err("PDF export needs output_path and no printer queue.".into());
    }
    if options
        .photos
        .as_ref()
        .is_some_and(|p| p.is_empty() || p.len() > 200)
        || options
            .frame_times_ms
            .as_ref()
            .is_some_and(|t| t.is_empty() || t.len() > 100 || t.iter().any(|&v| v > 86400000))
    {
        return Err("Select 1–200 photos or 1–100 timestamps within 24 hours.".into());
    }
    if options
        .frame_nodes
        .as_ref()
        .is_some_and(|f| f.is_empty() || f.len() > 200 || f.contains(&0))
    {
        return Err("Choose 1–200 valid Design frame IDs.".into());
    }
    if options.frame_nodes.is_some()
        && (options.photos.is_some()
            || options.video_path.is_some()
            || options.frame_times_ms.is_some())
    {
        return Err("Choose one print source type.".into());
    }
    if options.photos.is_some()
        && (options.video_path.is_some() || options.frame_times_ms.is_some())
        || options.video_path.is_some() && options.frame_times_ms.is_none()
    {
        return Err(
            "Choose either photos, page animation timestamps, or a local video with timestamps."
                .into(),
        );
    }
    if let Some(p) = &options.production {
        p.validate().map_err(|e| e.to_string())?;
    }
    for value in [
        &options.printer,
        &options.paper,
        &options.media,
        &options.tray,
        &options.quality,
        &options.sides,
        &options.pages,
    ]
    .into_iter()
    .flatten()
    {
        if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
            return Err(
                "Print identifiers need 1–512 characters without control characters.".into(),
            );
        }
    }
    if options
        .scale
        .is_some_and(|v| !v.is_finite() || !(1. ..=1000.).contains(&v))
        || options
            .margin_mm
            .is_some_and(|v| !v.is_finite() || !(0. ..=100.).contains(&v))
        || options
            .overlap_mm
            .is_some_and(|v| !v.is_finite() || !(0. ..=50.).contains(&v))
        || options.copies.is_some_and(|v| !(1..=999).contains(&v))
    {
        return Err("Print sizes, scale or copies are outside the supported range.".into());
    }
    if options
        .placement
        .as_deref()
        .is_some_and(|v| !["fit", "fill", "actual"].contains(&v))
        || options
            .layout
            .as_deref()
            .is_some_and(|v| !["document", "single", "contact", "repeat", "poster"].contains(&v))
    {
        return Err("Unknown print placement or layout.".into());
    }
    if options.layout.as_deref() == Some("document")
        && (options.printer.is_some() || name == "submit_print_job")
    {
        return Err(
            "Document page sizes are for PDF preview; choose a sheet layout for printer output."
                .into(),
        );
    }
    Ok(options)
}
fn creative(
    options: &Options,
    mut c: print::CreativeSettings,
) -> Result<print::CreativeSettings, String> {
    match (options.artwork_width_mm, options.artwork_height_mm) {
        (Some(w), Some(h)) => c.artwork_mm = Some([w, h]),
        (None, None) => (),
        _ => return Err("Supply both artwork_width_mm and artwork_height_mm.".into()),
    }
    c.labels = options.labels.unwrap_or(c.labels);
    c.rows = options.rows.unwrap_or(c.rows);
    c.columns = options.columns.unwrap_or(c.columns);
    c.gutter_mm = options.gutter_mm.unwrap_or(c.gutter_mm);
    c.crop = [
        options
            .crop_x_percent
            .map(|v| v / 100.)
            .unwrap_or(c.crop[0]),
        options
            .crop_y_percent
            .map(|v| v / 100.)
            .unwrap_or(c.crop[1]),
    ];
    c.bleed_mm = options.bleed_mm.unwrap_or(c.bleed_mm);
    c.crop_marks = options.crop_marks.unwrap_or(c.crop_marks);
    Ok(c)
}
fn settings(options: &Options, caps: &print::Capabilities) -> Result<print::Settings, String> {
    let base = if let Some(name) = &options.preset_name {
        let presets = print::presets::load(&print::presets::path()).map_err(|e| e.to_string())?;
        let preset = presets
            .iter()
            .find(|p| p.name == name.trim())
            .ok_or("Unknown print preset.")?;
        preset.settings.clone()
    } else {
        print::Settings::default()
    };
    resolve_settings(options, caps, base)
}
fn resolve_settings(
    options: &Options,
    caps: &print::Capabilities,
    base: print::Settings,
) -> Result<print::Settings, String> {
    let paper = options
        .paper
        .as_ref()
        .unwrap_or(if options.preset_name.is_some() {
            &base.paper.id
        } else {
            &caps.default_paper
        });
    let settings = print::Settings {
        production: options.production.clone().unwrap_or(base.production),
        creative: creative(options, base.creative)?,
        paper: caps
            .papers
            .iter()
            .find(|p| &p.id == paper)
            .cloned()
            .ok_or("Unsupported printer paper ID.")?,
        landscape: options.landscape.unwrap_or(base.landscape),
        grayscale: options.grayscale.unwrap_or(base.grayscale) || !caps.color,
        copies: options.copies.unwrap_or(base.copies),
        scale: options.scale.unwrap_or(base.scale),
        extra_margin: options.margin_mm.unwrap_or(base.extra_margin),
        overlap: options.overlap_mm.unwrap_or(base.overlap),
        placement: match options.placement.as_deref() {
            Some("fill") => print::Placement::Fill,
            Some("actual") => print::Placement::Actual,
            Some("fit") => print::Placement::Fit,
            _ => base.placement,
        },
        layout: match options.layout.as_deref() {
            Some("document") => print::Layout::Document,
            Some("contact") => print::Layout::Contact,
            Some("repeat") => print::Layout::Repeat,
            Some("poster") => print::Layout::Poster,
            Some("single") => print::Layout::Single,
            _ => base.layout,
        },
        media: options.media.clone(),
        tray: options.tray.clone(),
        quality: options.quality.clone(),
        sides: options.sides.clone(),
    };
    if settings.layout == print::Layout::Document && options.printer.is_some() {
        return Err("Document-size presets require PDF; override layout with a sheet layout for this printer.".into());
    }
    for (value, choices) in [
        (&options.media, &caps.media),
        (&options.tray, &caps.trays),
        (&options.quality, &caps.quality),
        (&options.sides, &caps.sides),
    ] {
        if value
            .as_ref()
            .is_some_and(|v| !choices.iter().any(|c| &c.id == v))
        {
            return Err("Unsupported printer media/tray/quality/duplex choice.".into());
        }
    }
    Ok(settings)
}
/// Blocking: live hosts call on the worker after capturing a source snapshot.
pub fn execute(
    name: &str,
    options: Options,
    title: &str,
    docs: Vec<(String, emulsion_core::Document)>,
) -> ToolResult {
    match run(name, options, title, docs) {
        Ok(result) => result,
        Err(e) => ToolResult::error(e),
    }
}
fn run(
    name: &str,
    options: Options,
    title: &str,
    docs: Vec<(String, emulsion_core::Document)>,
) -> Result<ToolResult, String> {
    if name == "list_print_presets" {
        return Ok(ToolResult::text(
            serde_json::to_string(
                &print::presets::load(&print::presets::path()).map_err(|e| e.to_string())?,
            )
            .unwrap(),
        ));
    }
    if name == "delete_print_preset" {
        let name = options
            .preset_name
            .as_deref()
            .ok_or("Enter a preset name.")?;
        let presets = print::presets::remove(&print::presets::path(), name.trim())
            .map_err(|e| e.to_string())?;
        return Ok(ToolResult::text(json!({"presets":presets}).to_string()));
    }
    if name == "list_printers" {
        return Ok(ToolResult::text(
            serde_json::to_string(&print::discover().map_err(|e| e.to_string())?).unwrap(),
        ));
    }
    let caps = if let Some(printer) = &options.printer {
        let devices = print::discover().map_err(|e| e.to_string())?;
        if !devices.iter().any(|p| &p.id == printer) {
            return Err("Printer is not an installed queue. Call list_printers first.".into());
        }
        print::capabilities(printer).map_err(|e| e.to_string())?
    } else {
        print::Capabilities::pdf()
    };
    if name == "get_printer_capabilities" {
        return Ok(ToolResult::text(serde_json::to_string(&caps).unwrap()));
    }
    let settings = if name == "save_print_preset" {
        let mut explicit = options.clone();
        explicit.preset_name = None;
        settings(&explicit, &caps)?
    } else {
        settings(&options, &caps)?
    };
    if name == "save_print_preset" {
        let name = options
            .preset_name
            .as_deref()
            .ok_or("Enter a preset name.")?;
        let presets = print::presets::save(&print::presets::path(), name, &settings)
            .map_err(|e| e.to_string())?;
        return Ok(ToolResult::text(json!({"presets":presets}).to_string()));
    }
    let cancel = AtomicBool::new(false);
    if options.printer.is_some() {
        print::production::validate_device_profile(&settings).map_err(|e| e.to_string())?;
    }
    let sources = if let Some(photos) = &options.photos {
        print::sources::photos(photos, &cancel)
    } else if let Some(path) = &options.video_path {
        print::sources::video(
            std::path::Path::new(path),
            options
                .frame_times_ms
                .as_deref()
                .ok_or("Choose timestamps")?,
            &cancel,
        )
    } else if let Some(ids) = &options.frame_nodes {
        let (name, doc) = docs
            .get(options.source_page.unwrap_or(0))
            .ok_or("Source page is outside the project")?;
        print::sources::frames(name, doc, ids, &cancel)
    } else if let Some(times) = &options.frame_times_ms {
        let (name, doc) = docs
            .get(options.source_page.unwrap_or(0))
            .ok_or("Source page is outside the project")?;
        print::sources::animation(name, doc, times, &cancel)
    } else {
        print::prepare_sources(docs, &cancel)
    }
    .map_err(|e| e.to_string())?;
    let selected = if let Some(pages) = &options.pages {
        print::page_range(pages, sources.len()).map_err(|e| e.to_string())?
    } else {
        (0..sources.len()).collect()
    };
    let layout = print::layout(&sources, &selected, &settings).map_err(|e| e.to_string())?;
    if name == "export_print_pdf" {
        let path =
            std::path::Path::new(options.output_path.as_deref().ok_or("Choose output_path")?);
        print::production::write_pdf(&sources, &layout, &settings, path, &cancel)
            .map_err(|e| e.to_string())?;
        return Ok(ToolResult::text(json!({"path":path,"sheets":layout.sheets.len(),"standard":settings.production.standard,"warnings":layout.warnings}).to_string()));
    }
    if name == "submit_print_job" {
        let printer = options.printer.ok_or("Choose a printer.")?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        print::validate_device_settings(&printer, &settings).map_err(|e| e.to_string())?;
        let accepted = print::submit(&printer, title, &sources, &layout, &settings, &cancel)
            .map_err(|e| e.to_string())?;
        return Ok(ToolResult::text(json!({"accepted":true,"queue":printer,"job":accepted,"sheets":layout.sheets.len(),"warnings":layout.warnings}).to_string()));
    }
    let sheet = options.sheet.unwrap_or(0);
    let page = layout
        .sheets
        .get(sheet)
        .ok_or("Preview sheet is outside the composed job.")?;
    let rgba =
        print::production::preview(&sources, page, &settings, 1200).map_err(|e| e.to_string())?;
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(rgba)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    use base64::Engine as _;
    Ok(ToolResult {
        content: vec![
            json!({"type":"text","text":json!({"sheet":sheet,"sheets":layout.sheets.len(),"width_mm":page.width,"height_mm":page.height,"warnings":layout.warnings}).to_string()}),
            json!({"type":"image","mimeType":"image/png","data":base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())}),
        ],
        is_error: false,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn print_tools_validate_before_any_device_io() {
        assert!(parse("submit_print_job", &json!({"copies":1})).is_err());
        assert!(parse("preview_print_job", &json!({"copies":0})).is_err());
        assert!(
            parse(
                "get_printer_capabilities",
                &json!({"printer":"queue","shell":"x"})
            )
            .is_err()
        );
        assert!(
            parse(
                "preview_print_job",
                &json!({"layout":"poster","scale":125.})
            )
            .is_ok()
        );
    }
    #[test]
    fn preview_uses_native_layout_without_device_submission() {
        let result = execute(
            "preview_print_job",
            Options::default(),
            "Test",
            vec![("Page".into(), emulsion_core::Document::new(100, 100))],
        );
        assert!(!result.is_error, "{:?}", result.content);
        assert_eq!(result.content[1]["type"], "image");
    }
    #[test]
    fn document_size_preview_exposes_physical_dimensions_and_rejects_device_submission() {
        let options = parse("preview_print_job", &json!({"layout":"document"})).unwrap();
        assert!(
            parse(
                "submit_print_job",
                &json!({"printer":"test", "layout":"document"})
            )
            .is_err()
        );
        let mut doc = emulsion_core::Document::new(1050, 600);
        doc.resolution = 300.;
        let result = execute(
            "preview_print_job",
            options,
            "Card",
            vec![("Card".into(), doc)],
        );
        assert!(!result.is_error, "{:?}", result.content);
        let info: Value =
            serde_json::from_str(result.content[0]["text"].as_str().unwrap()).unwrap();
        assert!((info["width_mm"].as_f64().unwrap() - 88.9).abs() < 0.001);
        assert!((info["height_mm"].as_f64().unwrap() - 50.8).abs() < 0.001);
    }
    #[test]
    fn creative_options_validate_and_preview_the_same_custom_trim() {
        for args in [
            json!({"artwork_width_mm":100}),
            json!({"rows":0}),
            json!({"crop_x_percent":101}),
            json!({"bleed_mm":21}),
            json!({"overlap_mm":51}),
        ] {
            assert!(parse("preview_print_job", &args).is_err(), "{args}");
        }
        assert!(
            parse(
                "delete_print_preset",
                &json!({"preset_name":"Proof","copies":2})
            )
            .is_err()
        );
        assert!(parse("save_print_preset", &json!({"layout":"single"})).is_err());
        let args = json!({"layout":"single", "placement":"fill", "artwork_width_mm":100., "artwork_height_mm":100.,
            "bleed_mm":3., "crop_marks":true, "crop_x_percent":0., "crop_y_percent":100.});
        let options = parse("preview_print_job", &args).unwrap();
        let settings = settings(&options, &print::Capabilities::pdf()).unwrap();
        assert_eq!(settings.creative.artwork_mm, Some([100., 100.]));
        assert_eq!(settings.creative.crop, [0., 1.]);
        let result = execute(
            "preview_print_job",
            options,
            "Proof",
            vec![("Page".into(), emulsion_core::Document::new(200, 100))],
        );
        assert!(!result.is_error, "{:?}", result.content);
    }
    #[test]
    fn explicit_options_override_incompatible_preset_paper_and_layout() {
        let mut base = print::Settings {
            layout: print::Layout::Document,
            ..Default::default()
        };
        base.paper.id = "unavailable".into();
        let mut options = Options {
            preset_name: Some("Local proof".into()),
            printer: Some("queue".into()),
            ..Default::default()
        };
        assert!(resolve_settings(&options, &print::Capabilities::pdf(), base.clone()).is_err());
        options.paper = Some("A4".into());
        options.layout = Some("single".into());
        let settings = resolve_settings(&options, &print::Capabilities::pdf(), base).unwrap();
        assert_eq!(settings.paper.id, "A4");
        assert_eq!(settings.layout, print::Layout::Single);
    }
    #[test]
    fn source_and_production_options_reject_ambiguous_jobs_before_io() {
        for args in [
            json!({"video_path":"a.mp4"}),
            json!({"frame_times_ms":[]}),
            json!({"frame_nodes":[]}),
            json!({"frame_nodes":[1],"frame_times_ms":[0]}),
            json!({"photos":[],"labels":"name"}),
            json!({"production":{"managed":true}}),
            json!({"production":{"dpi":0}}),
        ] {
            assert!(parse("preview_print_job", &args).is_err(), "{args}");
        }
        assert!(parse("export_print_pdf", &json!({})).is_err());
        assert!(
            parse(
                "export_print_pdf",
                &json!({"output_path":"proof.pdf","printer":"queue"})
            )
            .is_err()
        );
    }
    #[test]
    fn photo_print_pdf_export_keeps_the_input_and_includes_label_artwork() {
        let dir = std::env::temp_dir().join(format!(
            "emulsion-print-mcp-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let input = dir.join("selected.png");
        let output = dir.join("contact.pdf");
        image::RgbaImage::from_pixel(20, 20, image::Rgba([255, 0, 0, 255]))
            .save(&input)
            .unwrap();
        let before = std::fs::read(&input).unwrap();
        let options=parse("export_print_pdf",&json!({"output_path":output,"photos":[{"path":input}],"layout":"contact","labels":"number_and_name"})).unwrap();
        let result = execute("export_print_pdf", options, "Selected", vec![]);
        assert!(!result.is_error, "{:?}", result.content);
        assert!(std::fs::read(&output).unwrap().starts_with(b"%PDF"));
        assert_eq!(std::fs::read(&input).unwrap(), before);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
