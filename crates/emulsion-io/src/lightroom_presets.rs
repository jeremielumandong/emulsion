//! Import preset *data*. Lightroom Lua plug-ins are not executed.
use crate::{IoError, Result};
use emulsion_core::raw::DevelopParams;
use quick_xml::{NsReader, events::Event, name::ResolveResult};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Read, path::Path};

const MAX_BYTES: u64 = 4 * 1024 * 1024;
const CRS: &str = "http://ns.adobe.com/camera-raw-settings/1.0/";
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportedPreset {
    pub name: String,
    pub params: DevelopParams,
    pub applied: Vec<String>,
    pub warnings: Vec<String>,
}
fn error(s: impl ToString) -> IoError {
    IoError::Manifest(format!("Preset import: {}", s.to_string()))
}
pub fn load(path: &Path, base: DevelopParams) -> Result<ImportedPreset> {
    let ext = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    if ext == "lrplugin" || path.is_dir() {
        return Err(error(
            "Lightroom .lrplugin packages require Adobe's Lua SDK host. Import the plug-in's exported .xmp or .lrtemplate presets instead.",
        ));
    }
    if ext == "json" {
        return Ok(ImportedPreset {
            name: path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
            params: crate::raw_settings::load_preset(path)?,
            applied: vec!["Emulsion settings".into()],
            warnings: vec![],
        });
    }
    if !["xmp", "lrtemplate"].contains(&ext.as_str()) {
        return Err(error(
            "Choose an .xmp, .lrtemplate, or Emulsion .json preset",
        ));
    }
    let mut text = String::new();
    std::fs::File::open(path)?
        .take(MAX_BYTES + 1)
        .read_to_string(&mut text)?;
    if text.len() as u64 > MAX_BYTES {
        return Err(error("preset exceeds 4 MiB"));
    }
    let values = if ext == "xmp" {
        xmp(&text)?
    } else {
        legacy(&text)?
    };
    translate(
        values,
        base,
        path.file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
    )
}
fn xmp(text: &str) -> Result<BTreeMap<String, String>> {
    let mut reader = NsReader::from_str(text);
    let mut values = BTreeMap::new();
    let mut stack: Vec<Option<String>> = Vec::new();
    let mut count = 0usize;
    loop {
        let event = reader.read_event().map_err(error)?;
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                count += 1;
                if count > 100000 || stack.len() > 64 {
                    return Err(error("XML nesting or element limit exceeded"));
                }
                let (ns, local) = reader.resolver().resolve_element(e.name());
                let key = matches!(ns,ResolveResult::Bound(n) if n.as_ref()==CRS)
                    .then(|| local.as_ref().to_owned());
                for a in e.attributes() {
                    let a = a.map_err(error)?;
                    let (ns, local) = reader.resolver().resolve_attribute(a.key);
                    if matches!(ns,ResolveResult::Bound(n) if n.as_ref()==CRS) {
                        let value = a
                            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .map_err(error)?
                            .into_owned();
                        values.insert(local.as_ref().to_owned(), value);
                    }
                }
                // Empty elements must not change the ancestor stack.
                if !empty {
                    stack.push(key);
                }
            }
            Event::End(_) => {
                stack.pop();
            }
            Event::GeneralRef(e) => {
                if let Some(key) = stack.iter().rev().find_map(|s| s.as_ref()) {
                    let text = if let Some(c) = e.resolve_char_ref().map_err(error)? {
                        c.to_string()
                    } else {
                        match e.as_ref() {
                            "amp" => "&",
                            "lt" => "<",
                            "gt" => ">",
                            "quot" => "\"",
                            "apos" => "'",
                            _ => return Err(error("Unknown XML entity")),
                        }
                        .into()
                    };
                    values
                        .entry(key.clone())
                        .or_insert_with(String::new)
                        .push_str(&text);
                }
            }
            Event::Text(e) => {
                if let Some(key) = stack.iter().rev().find_map(|s| s.as_ref()) {
                    let decoded = e.xml_content(quick_xml::XmlVersion::Implicit1_0);
                    let value = quick_xml::escape::unescape(&decoded).map_err(error)?;
                    if !value.trim().is_empty() {
                        let entry = values.entry(key.clone()).or_insert_with(String::new);
                        if key.starts_with("ToneCurve") && !entry.is_empty() {
                            entry.push(';');
                        }
                        entry.push_str(if key.starts_with("ToneCurve") {
                            value.trim()
                        } else {
                            &value
                        });
                    }
                }
            }
            Event::DocType(_) => return Err(error("DOCTYPE is not permitted in presets")),
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(values)
}

// A small, non-executing parser for Lua table literals. Calls, operators and
// bytecode are rejected. The only accepted values are data literals/tables.
#[derive(Clone, Debug, PartialEq)]
enum Token {
    Word(String),
    Text(String),
    Number(String),
    Open,
    Close,
    Equal,
    Comma,
}
fn tokens(s: &str) -> Result<Vec<Token>> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        match b[i] {
            c if c.is_ascii_whitespace() => {
                i += 1;
            }
            b'-' if b.get(i + 1) == Some(&b'-') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'{' => {
                out.push(Token::Open);
                i += 1;
            }
            b'}' => {
                out.push(Token::Close);
                i += 1;
            }
            b'=' => {
                out.push(Token::Equal);
                i += 1;
            }
            b',' | b';' => {
                out.push(Token::Comma);
                i += 1;
            }
            b'"' | b'\'' => {
                let quote = b[i];
                i += 1;
                let mut bytes = Vec::new();
                while i < b.len() && b[i] != quote {
                    if b[i] == b'\\' {
                        i += 1;
                        if i == b.len() {
                            return Err(error("Unterminated quoted value"));
                        }
                        bytes.push(match b[i] {
                            b'n' => b'\n',
                            b'r' => b'\r',
                            b't' => b'\t',
                            v => v,
                        });
                    } else {
                        bytes.push(b[i]);
                    }
                    i += 1;
                }
                if i == b.len() {
                    return Err(error("Unterminated quoted value"));
                }
                i += 1;
                out.push(Token::Text(String::from_utf8(bytes).map_err(error)?));
            }
            c if c.is_ascii_digit() || c == b'-' || c == b'+' || c == b'.' => {
                let start = i;
                i += 1;
                while i < b.len() && (b[i].is_ascii_digit() || b".eE+-".contains(&b[i])) {
                    i += 1;
                }
                let n = &s[start..i];
                let f = n.parse::<f64>().map_err(error)?;
                if !f.is_finite() {
                    return Err(error("Nonfinite number"));
                }
                out.push(Token::Number(n.into()));
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let start = i;
                i += 1;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                    i += 1;
                }
                out.push(Token::Word(s[start..i].into()));
            }
            _ => {
                return Err(error(
                    "Only literal .lrtemplate data is supported; Lua code is never executed",
                ));
            }
        }
        if out.len() > 100000 {
            return Err(error("Preset token limit exceeded"));
        }
    }
    Ok(out)
}
fn legacy(text: &str) -> Result<BTreeMap<String, String>> {
    fn table(
        tokens: &[Token],
        i: &mut usize,
        depth: usize,
        values: &mut BTreeMap<String, String>,
        prefix: &str,
    ) -> Result<String> {
        if depth > 32 || tokens.get(*i) != Some(&Token::Open) {
            return Err(error("Malformed preset table"));
        }
        *i += 1;
        let mut list = Vec::new();
        while tokens.get(*i) != Some(&Token::Close) {
            let key = if matches!(tokens.get(*i), Some(Token::Word(_)))
                && tokens.get(*i + 1) == Some(&Token::Equal)
            {
                let Some(Token::Word(k)) = tokens.get(*i) else {
                    unreachable!()
                };
                *i += 2;
                Some(k.clone())
            } else {
                None
            };
            let value = match tokens.get(*i) {
                Some(Token::Open) => table(
                    tokens,
                    i,
                    depth + 1,
                    values,
                    key.as_deref().unwrap_or(prefix),
                )?,
                Some(Token::Text(v) | Token::Number(v)) => {
                    *i += 1;
                    v.clone()
                }
                Some(Token::Word(v)) if ["true", "false", "nil"].contains(&v.as_str()) => {
                    *i += 1;
                    v.clone()
                }
                _ => return Err(error("Malformed or executable Lua preset value")),
            };
            if let Some(key) = key {
                if prefix == "settings" || prefix.is_empty() {
                    values.insert(key, value);
                }
            } else {
                list.push(value);
            }
            if tokens.get(*i) == Some(&Token::Comma) {
                *i += 1;
            } else if tokens.get(*i) != Some(&Token::Close) {
                return Err(error("Expected comma in preset"));
            }
        }
        *i += 1;
        Ok(list.join(","))
    }
    let t = tokens(text)?;
    let mut i =
        if matches!(t.first(),Some(Token::Word(v)) if v=="s") && t.get(1) == Some(&Token::Equal) {
            2
        } else if matches!(t.first(),Some(Token::Word(v)) if v=="return") {
            1
        } else {
            0
        };
    let mut values = BTreeMap::new();
    table(&t, &mut i, 0, &mut values, "")?;
    if i != t.len() {
        return Err(error("Unexpected code after preset table"));
    }
    values.remove("settings");
    values.remove("value");
    Ok(values)
}
pub fn from_adobe_settings(
    value: &serde_json::Value,
    base: DevelopParams,
) -> Result<ImportedPreset> {
    let object = value
        .as_object()
        .ok_or_else(|| error("Adobe settings must be an object"))?;
    let mut values = BTreeMap::new();
    for (key, value) in object {
        let text = match value {
            serde_json::Value::String(v) => v.clone(),
            serde_json::Value::Array(points) => points
                .iter()
                .flat_map(|v| {
                    if let Some(a) = v.as_array() {
                        a.clone()
                    } else {
                        vec![v.clone()]
                    }
                })
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(","),
            _ => value.to_string(),
        };
        values.insert(key.clone(), text);
    }
    translate(values, base, "Lightroom development".into())
}
pub fn from_legacy_settings(text: &str, base: DevelopParams) -> Result<ImportedPreset> {
    translate(legacy(text)?, base, "Lightroom history".into())
}
fn translate(
    values: BTreeMap<String, String>,
    mut p: DevelopParams,
    name: String,
) -> Result<ImportedPreset> {
    let mut report = ImportedPreset {
        name,
        params: p,
        applied: vec![],
        warnings: vec![
            "Adobe/VSCO settings are translated to Emulsion's renderer; the appearance may differ."
                .into(),
        ],
    };
    for (key, value) in &values {
        if ["Name", "title"].contains(&key.as_str()) {
            report.name = value.chars().take(200).collect();
            continue;
        }
        if [
            "UUID",
            "PresetType",
            "Cluster",
            "Group",
            "Description",
            "SupportsAmount",
            "SupportsColor",
            "SupportsMonochrome",
            "SupportsHighDynamicRange",
            "SupportsNormalDynamicRange",
            "SupportsSceneReferred",
            "SupportsOutputReferred",
            "CameraModelRestriction",
            "Version",
            "ProcessVersion",
            "HasSettings",
            "HasCrop",
            "AlreadyApplied",
            "AutoTone",
            "internalName",
            "id",
            "type",
            "value",
        ]
        .contains(&key.as_str())
        {
            continue;
        }
        let number = || -> Result<f32> {
            let v = value
                .parse::<f32>()
                .map_err(|_| error(format!("Invalid numeric {key}")))?;
            if v.is_finite() {
                Ok(v)
            } else {
                Err(error(format!("Nonfinite {key}")))
            }
        };
        let unit = || number().map(|v| (v / 100.).clamp(-1., 1.));
        let mut applied = true;
        match key.as_str() {
            "Exposure2012" | "Exposure" => p.exposure = number()?.clamp(-5., 5.),
            "Contrast2012" | "Contrast" => p.contrast = unit()?,
            "Highlights2012" => p.highlights = -unit()?,
            "Shadows2012" => p.shadows = unit()?,
            "Whites2012" => p.whites = unit()?,
            "Blacks2012" => p.blacks = unit()?,
            "Saturation" => p.saturation = unit()?,
            "Vibrance" => p.vibrance = unit()?,
            "Texture" => p.texture = unit()?,
            "Clarity2012" | "Clarity" => p.clarity = unit()?,
            "Dehaze" => p.dehaze = unit()?,
            "PostCropVignetteAmount" | "VignetteAmount" => p.vignette = unit()?,
            "Sharpness" => p.sharpening = (number()? / 150.).clamp(0., 1.),
            "LuminanceSmoothing" => p.noise_reduction = unit()?.max(0.),
            "Temperature" => {
                p.kelvin = Some(number()?.clamp(2000., 50000.));
                p.temperature = 0.;
                p.wb_override = None;
            }
            "Tint" => p.tint = (number()? / 150.).clamp(-1., 1.),
            "WhiteBalance" if value == "As Shot" => {
                p.kelvin = None;
                p.temperature = 0.;
                p.tint = 0.;
                p.wb_override = None;
            }
            "WhiteBalance" if value == "Custom" => {}
            "ConvertToGrayscale" if value.eq_ignore_ascii_case("true") => p.saturation = -1.,
            "ConvertToGrayscale" => {}
            "CropLeft" => p.crop[0] = number()?,
            "CropTop" => p.crop[1] = number()?,
            "CropRight" => p.crop[2] = number()?,
            "CropBottom" => p.crop[3] = number()?,
            "CropAngle" => p.straighten = number()?,
            "ToneCurvePV2012"
            | "ToneCurve"
            | "ToneCurvePV2012Red"
            | "ToneCurvePV2012Green"
            | "ToneCurvePV2012Blue"
            | "ToneCurveRed"
            | "ToneCurveGreen"
            | "ToneCurveBlue" => {
                let n: Vec<f32> = value
                    .split([',', ';'])
                    .filter(|v| !v.trim().is_empty())
                    .map(|v| v.trim().parse::<f32>().map_err(error))
                    .collect::<Result<_>>()?;
                if n.len() < 4 || !n.len().is_multiple_of(2) {
                    return Err(error("Invalid curve points"));
                }
                let points: Vec<_> = n
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|p| [p[0] / 255., p[1] / 255.])
                    .collect();
                let channel = if key.ends_with("Red") {
                    1
                } else if key.ends_with("Green") {
                    2
                } else if key.ends_with("Blue") {
                    3
                } else {
                    0
                };
                p.point_curves[channel] =
                    emulsion_core::raw::PointCurve::try_from(points).map_err(error)?;
                if channel == 0 {
                    p.tone_curve = DevelopParams::LINEAR_CURVE;
                }
            }
            "ToneCurveName" | "ToneCurveName2012" => {}
            "SplitToningShadowHue" => p.grading[0][0] = number()?,
            "SplitToningShadowSaturation" => p.grading[0][1] = unit()?.max(0.),
            "SplitToningHighlightHue" => p.grading[2][0] = number()?,
            "SplitToningHighlightSaturation" => p.grading[2][1] = unit()?.max(0.),
            "ColorGradeMidtoneHue" => p.grading[1][0] = number()?,
            "ColorGradeMidtoneSat" => p.grading[1][1] = unit()?.max(0.),
            "CameraProfile" | "CameraProfileDigest" | "Look" | "LookTable" => {
                report.warnings.push(format!(
                    "{key}: {} requires an Adobe/DCP profile that is not applied",
                    value.chars().take(200).collect::<String>()
                ));
                applied = false;
            }
            _ => {
                applied = false;
                for (c, color) in [
                    "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Magenta",
                ]
                .iter()
                .enumerate()
                {
                    for (j, prefix) in [
                        "HueAdjustment",
                        "SaturationAdjustment",
                        "LuminanceAdjustment",
                    ]
                    .iter()
                    .enumerate()
                    {
                        if key == &format!("{prefix}{color}") {
                            p.hsl[c][j] = unit()?;
                            applied = true;
                        }
                    }
                }
                if !applied {
                    report.warnings.push(format!("Unsupported setting: {key}"));
                }
            }
        }
        if applied {
            report.applied.push(key.clone());
        }
    }
    if report.applied.is_empty() {
        return Err(error(
            "No supported develop adjustments were found in this preset",
        ));
    }
    p.validate().map_err(error)?;
    report.params = p;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn xmp_uses_namespace_and_preserves_omitted_settings() {
        let values=xmp(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:camera="http://ns.adobe.com/camera-raw-settings/1.0/"><rdf:RDF><rdf:Description camera:Exposure2012="1.25" camera:Highlights2012="-30" camera:HueAdjustmentBlue="20" camera:CameraProfile="VSCO Film camera profile"><camera:Name><rdf:Alt><rdf:li>Film &amp; Color</rdf:li></rdf:Alt></camera:Name><camera:ToneCurvePV2012><rdf:Seq><rdf:li>0, 0</rdf:li><rdf:li>128, 110</rdf:li><rdf:li>255, 255</rdf:li></rdf:Seq></camera:ToneCurvePV2012></rdf:Description></rdf:RDF></x:xmpmeta>"#).unwrap();
        let base = DevelopParams {
            vibrance: 0.4,
            ..Default::default()
        };
        let report = translate(values, base, "fallback".into()).unwrap();
        assert_eq!(report.name, "Film & Color");
        assert_eq!(report.params.exposure, 1.25);
        assert_eq!(report.params.highlights, 0.3);
        assert_eq!(report.params.hsl[5][0], 0.2);
        assert_eq!(report.params.vibrance, 0.4);
        assert!(
            report
                .warnings
                .iter()
                .any(|s| s.contains("VSCO Film camera profile"))
        );
        assert!(report.params.point_curves[0].output(0.5) < 0.5);
    }
    #[test]
    fn legacy_vsco_style_tables_are_data_and_never_executed() {
        let values=legacy(r#"s = { title = "Film 02", value = { settings = { Exposure2012 = -0.5, Saturation = -20, ToneCurvePV2012 = { 0, 0, 128, 130, 255, 255 }, CameraProfile = "VSCO", }, }, }"#).unwrap();
        let report = translate(values, DevelopParams::default(), "fallback".into()).unwrap();
        assert_eq!(report.name, "Film 02");
        assert_eq!(report.params.exposure, -0.5);
        assert_eq!(report.params.saturation, -0.2);
        for bad in [
            "s={value=os.execute('touch /tmp/nope')}",
            "s={} function attack() end",
            "s={exposure=0/0}",
            "s={title='unterminated}",
        ] {
            assert!(legacy(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn zip_presets_stay_inside_bank_and_report_profiles_and_invalid_code() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let pack = dir.path().join("pack.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&pack).unwrap());
        for (name, bytes) in [
            (
                "../../outside.lrtemplate",
                "s={title='Film', value={settings={Exposure2012=0.75, CameraProfile='VSCO'}}}",
            ),
            ("bad.lrtemplate", "s={value=os.execute('no')}"),
            ("profiles/Camera.dcp", "profile"),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        let bank = dir.path().join("bank");
        let report = install_into(&pack, &bank).unwrap();
        assert_eq!(report.files.len(), 1);
        assert_eq!(report.files[0].parent(), Some(bank.as_path()));
        assert!(!dir.path().join("outside.lrtemplate").exists());
        assert_eq!(
            load(&report.files[0], DevelopParams::default())
                .unwrap()
                .params
                .exposure,
            0.75
        );
        assert!(report.warnings.iter().any(|w| w.contains("VSCO")));
        assert!(report.warnings.iter().any(|w| w.contains("bad.lrtemplate")));
        assert!(report.warnings.iter().any(|w| w.contains("Camera.dcp")));
        assert_eq!(install_into(&pack, &bank).unwrap().files, report.files);
    }
    #[test]
    fn malformed_unknown_and_profile_only_presets_fail_visibly() {
        assert!(xmp("<!DOCTYPE x [<!ENTITY a SYSTEM 'file:///etc/passwd'>]><x/>").is_err());
        assert!(xmp("<x><y></x>").is_err());
        for values in [
            BTreeMap::from([("CameraProfile".into(), "VSCO".into())]),
            BTreeMap::from([("Exposure2012".into(), "NaN".into())]),
        ] {
            assert!(translate(values, DevelopParams::default(), "x".into()).is_err());
        }
    }
}

#[derive(Debug, Serialize)]
pub struct InstalledPresets {
    pub files: Vec<std::path::PathBuf>,
    pub warnings: Vec<String>,
}
pub fn library_dir() -> std::path::PathBuf {
    crate::recent::data_dir().join("develop-presets")
}
pub fn installed() -> Vec<std::path::PathBuf> {
    let mut files: Vec<_> = std::fs::read_dir(library_dir())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|s| {
                ["xmp", "lrtemplate", "json"]
                    .iter()
                    .any(|e| s.eq_ignore_ascii_case(e))
            })
        })
        .collect();
    files.sort();
    files.truncate(2000);
    files
}
/// Copy validated preset data into the local bank. ZIP members are never
/// extracted by their supplied paths, and no Lua or plug-in code is executed.
pub fn install(path: &Path) -> Result<InstalledPresets> {
    install_into(path, &library_dir())
}
fn install_into(path: &Path, directory: &Path) -> Result<InstalledPresets> {
    use sha2::{Digest, Sha256};
    fn save(
        directory: &Path,
        name: &str,
        bytes: &[u8],
    ) -> Result<(std::path::PathBuf, Vec<String>)> {
        let name = Path::new(name)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        let safe: String = name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || ".-_ ".contains(c) {
                    c
                } else {
                    '_'
                }
            })
            .take(160)
            .collect();
        let hash: String = Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let path = directory.join(format!("{}-{safe}", &hash[..12]));
        std::fs::create_dir_all(directory)?;
        let mut temporary = tempfile::Builder::new()
            .suffix(&format!(
                ".{}",
                Path::new(&safe)
                    .extension()
                    .unwrap_or_default()
                    .to_string_lossy()
            ))
            .tempfile_in(directory)?;
        use std::io::Write;
        temporary.write_all(bytes)?;
        let report = load(temporary.path(), DevelopParams::default())?;
        if !path.exists() {
            temporary.persist_noclobber(&path).map_err(|e| e.error)?;
        }
        Ok((path, report.warnings))
    }
    let mut report = InstalledPresets {
        files: vec![],
        warnings: vec![],
    };
    if path
        .extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("zip"))
    {
        let mut archive = zip::ZipArchive::new(std::fs::File::open(path)?)?;
        if archive.len() > 2000 {
            return Err(error("Preset pack contains too many members"));
        }
        let mut total = 0usize;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().to_string();
            let ext = Path::new(&name)
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase();
            if !["xmp", "lrtemplate", "json"].contains(&ext.as_str()) {
                if ["dcp", "lcp", "lrplugin"].contains(&ext.as_str()) {
                    report.warnings.push(format!(
                        "Not installed: {name} requires a separate profile or Adobe plug-in host"
                    ));
                }
                continue;
            }
            if entry.size() > MAX_BYTES {
                return Err(error("Preset member exceeds 4 MiB"));
            }
            let mut bytes = Vec::new();
            entry.by_ref().take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
            total += bytes.len();
            if bytes.len() as u64 > MAX_BYTES || total > 32 * 1024 * 1024 {
                return Err(error("Preset pack exceeds import limits"));
            }
            match save(directory, &name, &bytes) {
                Ok((path, warnings)) => {
                    report.files.push(path);
                    report.warnings.extend(warnings);
                }
                Err(e) => report.warnings.push(format!("{name}: {e}")),
            };
        }
    } else {
        load(path, DevelopParams::default())?;
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(error("Preset exceeds import limits"));
        }
        let (file, warnings) = save(
            directory,
            &path.file_name().unwrap_or_default().to_string_lossy(),
            &bytes,
        )?;
        report.files.push(file);
        report.warnings.extend(warnings);
    }
    if report.files.is_empty() {
        return Err(error(format!(
            "No compatible presets in this pack. {}",
            report.warnings.join(" ")
        )));
    }
    Ok(report)
}
