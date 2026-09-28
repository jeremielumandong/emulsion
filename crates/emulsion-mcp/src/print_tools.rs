//! Bounded native printer discovery, preview and explicit queue submission.
use crate::{ToolDef, ToolResult};
use emulsion_io::printing as print;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::atomic::AtomicBool;
pub const READ_ONLY: &[&str] = &[
    "list_printers",
    "get_printer_capabilities",
    "preview_print_job",
];
pub const DESTRUCTIVE: &[&str] = &["submit_print_job"];
pub fn is_tool(name: &str) -> bool {
    READ_ONLY.contains(&name) || DESTRUCTIVE.contains(&name)
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
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
    let settings = json!({"printer":{"type":"string","minLength":1,"maxLength":512},"pages":{"type":"string","description":"1-based page range, e.g. 1,3-5; omitted means all pages"},"paper":{"type":"string"},"landscape":{"type":"boolean"},"placement":{"enum":["fit","fill","actual"]},"layout":{"enum":["single","contact","repeat","poster"]},"scale":{"type":"number","minimum":1,"maximum":1000},"margin_mm":{"type":"number","minimum":0,"maximum":100},"overlap_mm":{"type":"number","minimum":0,"maximum":100},"copies":{"type":"integer","minimum":1,"maximum":999},"grayscale":{"type":"boolean"},"media":{"type":"string"},"tray":{"type":"string"},"quality":{"type":"string"},"sides":{"type":"string"},"sheet":{"type":"integer","minimum":0},"expected_revision":{"type":"integer","minimum":0}});
    [
 ("list_printers","Discover installed native print queues without submitting a job.",json!({}),vec![]),
 ("get_printer_capabilities","Read native paper, tray, quality, duplex and color choices for an installed printer.",json!({"printer":{"type":"string","minLength":1,"maxLength":512}}),vec!["printer"]),
 ("preview_print_job","Render a bounded PNG preview from an immutable originating-project snapshot using the same physical sheet layout as native printing. Printer omitted uses generic PDF paper sizes; sheet is zero-based.",settings.clone(),vec![]),
 ("submit_print_job","Explicitly submit the originating-project snapshot to an installed OS print queue. Returns queue acceptance, never a claim of physical completion. Device/paper choices are validated; expected_revision protects the active-page snapshot.",settings,vec!["printer"]),
 ].into_iter().map(|(name,description,properties,required)|ToolDef{name:name.into(),description:description.into(),input_schema:json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})}).collect()
}
pub fn parse(name: &str, args: &Value) -> Result<Options, String> {
    let object = args.as_object().ok_or("Expected an argument object.")?;
    if object.values().any(Value::is_null) {
        return Err("Omit unchanged print options instead of null.".into());
    }
    if name == "list_printers" && !object.is_empty() {
        return Err("Printer listing takes no arguments.".into());
    }
    if name == "get_printer_capabilities" && object.keys().any(|k| k != "printer") {
        return Err("Capabilities accepts only printer.".into());
    }
    let options: Options = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
    if ["get_printer_capabilities", "submit_print_job"].contains(&name) && options.printer.is_none()
    {
        return Err("Select an installed printer.".into());
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
            .is_some_and(|v| !v.is_finite() || !(0. ..=100.).contains(&v))
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
            .is_some_and(|v| !["single", "contact", "repeat", "poster"].contains(&v))
    {
        return Err("Unknown print placement or layout.".into());
    }
    Ok(options)
}
fn settings(options: &Options, caps: &print::Capabilities) -> Result<print::Settings, String> {
    let paper = options.paper.as_ref().unwrap_or(&caps.default_paper);
    let settings = print::Settings {
        paper: caps
            .papers
            .iter()
            .find(|p| &p.id == paper)
            .cloned()
            .ok_or("Unsupported printer paper ID.")?,
        landscape: options.landscape.unwrap_or(false),
        grayscale: options.grayscale.unwrap_or(false),
        copies: options.copies.unwrap_or(1),
        scale: options.scale.unwrap_or(100.),
        extra_margin: options.margin_mm.unwrap_or(5.),
        overlap: options.overlap_mm.unwrap_or(5.),
        placement: match options.placement.as_deref() {
            Some("fill") => print::Placement::Fill,
            Some("actual") => print::Placement::Actual,
            _ => print::Placement::Fit,
        },
        layout: match options.layout.as_deref() {
            Some("contact") => print::Layout::Contact,
            Some("repeat") => print::Layout::Repeat,
            Some("poster") => print::Layout::Poster,
            _ => print::Layout::Single,
        },
        media: options.media.clone(),
        tray: options.tray.clone(),
        quality: options.quality.clone(),
        sides: options.sides.clone(),
    };
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
    let settings = settings(&options, &caps)?;
    let cancel = AtomicBool::new(false);
    let sources = print::prepare_sources(docs, &cancel).map_err(|e| e.to_string())?;
    let selected = if let Some(pages) = &options.pages {
        print::page_range(pages, sources.len()).map_err(|e| e.to_string())?
    } else {
        (0..sources.len()).collect()
    };
    let layout = print::layout(&sources, &selected, &settings).map_err(|e| e.to_string())?;
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
        print::preview(&sources, page, settings.grayscale, 1200).map_err(|e| e.to_string())?;
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(rgba)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    use base64::Engine as _;
    Ok(ToolResult {
        content: vec![
            json!({"type":"text","text":json!({"sheet":sheet,"sheets":layout.sheets.len(),"warnings":layout.warnings}).to_string()}),
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
}
