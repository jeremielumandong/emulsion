//! Live Library requests. The workspace owns selection, drafts and asynchronous jobs.
use crate::server::ToolDef;
use emulsion_core::raw::DevelopParams;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;

pub const NAMES: &[&str] = &[
    "get_library",
    "get_library_preview",
    "import_library",
    "set_library_view",
    "select_library_photos",
    "edit_library_metadata",
    "library_collection",
    "library_catalog",
    "develop_library",
    "export_library",
    "open_library_photo",
    "cancel_library_export",
    "cancel_library_enhancement",
];
pub fn is_tool(name: &str) -> bool {
    NAMES.contains(&name)
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Page {
    #[serde(default)]
    pub offset: usize,
    pub limit: Option<usize>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Import {
    #[serde(default)]
    pub deduplicate: bool,
    pub folder: PathBuf,
}
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct View {
    pub query: Option<String>,
    pub develop_section: Option<String>,
    pub collapse_stacks: Option<bool>,
    pub source: Option<Source>,
    pub collection: Option<u64>,
    pub minimum_rating: Option<u8>,
    pub flag: Option<Flag>,
    pub color_label: Option<u8>,
    pub raw_only: Option<bool>,
    pub clipping: Option<bool>,
    pub unedited: Option<bool>,
    pub sort: Option<Sort>,
    pub reverse: Option<bool>,
    pub mode: Option<Mode>,
    pub inspector: Option<Inspector>,
    /// Empty string clears the optional film recipe.
    pub recipe: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    All,
    Folder,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Flag {
    All,
    Picked,
    Rejected,
    Unflagged,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    Filename,
    CaptureTime,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Grid,
    List,
    Develop,
    Before,
    Compare,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Inspector {
    Develop,
    Info,
    Keywords,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub paths: Vec<PathBuf>,
    pub active: Option<PathBuf>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub paths: Vec<PathBuf>,
    pub rating: Option<u8>,
    pub flag: Option<Flag>,
    pub color_label: Option<u8>,
    pub keywords: Option<Vec<String>>,
    #[serde(default)]
    pub append_keywords: bool,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Collection {
    pub action: CollectionAction,
    pub name: Option<String>,
    pub id: Option<u64>,
    #[serde(default)]
    pub paths: Vec<PathBuf>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectionAction {
    Create,
    Add,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Develop {
    pub action: DevelopAction,
    pub point: Option<[f32; 2]>,
    pub name: Option<String>,
    pub settings: Option<Value>,
    pub preset: Option<Preset>,
    pub path: Option<PathBuf>,
    pub group: Option<Group>,
}
#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DevelopAction {
    Adjust,
    Auto,
    Reset,
    AsShot,
    Undo,
    Reload,
    Save,
    Sync,
    Preset,
    SavePreset,
    LoadPreset,
    Snapshot,
    RestoreSnapshot,
    SubjectMask,
    SkyMask,
    AutoSky,
    AutoPerspective,
    Denoise,
    SuperResolution,
    MatchLens,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    Neutral,
    Warm,
    BlackAndWhite,
    StrongContrast,
}
#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    All,
    WhiteBalance,
    Tone,
    Curve,
}
impl Group {
    pub fn raw(self) -> emulsion_io::raw_settings::RawSettingsGroup {
        use emulsion_io::raw_settings::RawSettingsGroup as G;
        match self {
            Self::All => G::All,
            Self::WhiteBalance => G::WhiteBalance,
            Self::Tone => G::Tone,
            Self::Curve => G::Curve,
        }
    }
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Export {
    pub settings: Option<emulsion_io::photo_export::OutputSettings>,
    pub out_dir: PathBuf,
    pub format: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogAction {
    pub action: String,
    #[serde(default)]
    pub paths: Vec<PathBuf>,
    pub name: Option<String>,
    pub path: Option<PathBuf>,
    pub rule: Option<emulsion_io::photo_catalog::SmartRule>,
}
#[derive(Debug)]
pub enum Request {
    State(Page),
    Preview,
    Import(Import),
    View(View),
    Select(Selection),
    Metadata(Metadata),
    Collection(Collection),
    Catalog(CatalogAction),
    Develop(Develop),
    Export(Export),
    Open,
    CancelExport,
    CancelEnhancement,
}
pub fn parse(name: &str, args: &Value) -> Result<Request, String> {
    fn from<T: serde::de::DeserializeOwned>(v: &Value) -> Result<T, String> {
        serde_json::from_value(v.clone()).map_err(|e| e.to_string())
    }
    fn empty(v: &Value) -> Result<(), String> {
        if v.as_object().is_some_and(|o| o.is_empty()) {
            Ok(())
        } else {
            Err("This tool takes an empty object".into())
        }
    }
    let r = match name {
        "get_library" => Request::State(from(args)?),
        "get_library_preview" => {
            empty(args)?;
            Request::Preview
        }
        "import_library" => Request::Import(from(args)?),
        "set_library_view" => Request::View(from(args)?),
        "select_library_photos" => Request::Select(from(args)?),
        "edit_library_metadata" => Request::Metadata(from(args)?),
        "library_collection" => Request::Collection(from(args)?),
        "library_catalog" => Request::Catalog(from(args)?),
        "develop_library" => Request::Develop(from(args)?),
        "export_library" => Request::Export(from(args)?),
        "cancel_library_enhancement" => {
            empty(args)?;
            Request::CancelEnhancement
        }
        "cancel_library_export" => {
            empty(args)?;
            Request::CancelExport
        }
        "open_library_photo" => {
            empty(args)?;
            Request::Open
        }
        _ => return Err("Unknown Library tool".into()),
    };
    match &r {
        Request::State(p) if p.limit.is_some_and(|n| n == 0 || n > 200) => {
            return Err("limit must be 1–200".into());
        }
        Request::View(v) => {
            validate_labels(v.minimum_rating, v.color_label)?;
            if matches!(v.flag, Some(Flag::Unflagged)) {
                return Err("Library flag filter supports all, picked or rejected".into());
            }
            if v.collection.is_some() && matches!(v.source, Some(Source::Folder)) {
                return Err("A collection uses source=all".into());
            }
        }
        Request::Select(s) => validate_paths(&s.paths, true)?,
        Request::Metadata(m) => {
            validate_paths(&m.paths, false)?;
            validate_labels(m.rating, m.color_label)?;
            if matches!(m.flag, Some(Flag::All)) {
                return Err("Metadata flag must be picked, rejected or unflagged".into());
            }
            if m.rating.is_none()
                && m.flag.is_none()
                && m.color_label.is_none()
                && m.keywords.is_none()
            {
                return Err("Supply metadata to change".into());
            }
        }
        Request::Collection(c) => {
            validate_paths(&c.paths, true)?;
            match c.action {
                CollectionAction::Create
                    if c.name.as_ref().is_none_or(|n| n.trim().is_empty()) || c.id.is_some() =>
                {
                    return Err("create requires name and no id".into());
                }
                CollectionAction::Add
                    if c.id.is_none() || c.name.is_some() || c.paths.is_empty() =>
                {
                    return Err("add requires id and paths, without name".into());
                }
                _ => {}
            }
        }
        Request::Develop(d) => {
            use DevelopAction as A;
            if (d.action == A::Adjust) != d.settings.is_some()
                || (d.action == A::Preset) != d.preset.is_some()
                || matches!(d.action, A::SavePreset | A::LoadPreset) != d.path.is_some()
                || d.group.is_some() && !matches!(d.action, A::Sync | A::Reset)
                || matches!(d.action, A::Snapshot | A::RestoreSnapshot) != d.name.is_some()
                || (d.action == A::SkyMask) != d.point.is_some()
                || d.point
                    .is_some_and(|p| p.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)))
            {
                return Err("Arguments do not match the Develop action".into());
            }
            if let Some(settings) = &d.settings {
                patch(DevelopParams::default(), settings)?;
            }
        }
        Request::Export(e) if !matches!(e.format.as_str(), "jpg" | "png" | "tif" | "webp") => {
            return Err("Library export format must be jpg, png, tif or webp".into());
        }
        _ => {}
    }
    Ok(r)
}
fn validate_labels(rating: Option<u8>, label: Option<u8>) -> Result<(), String> {
    if rating.is_some_and(|v| v > 5) || label.is_some_and(|v| v > 5) {
        Err("rating and color_label must be 0–5".into())
    } else {
        Ok(())
    }
}
fn validate_paths(paths: &[PathBuf], empty: bool) -> Result<(), String> {
    if (!empty && paths.is_empty())
        || paths.len() > 10_000
        || paths.iter().any(|p| p.as_os_str().is_empty())
    {
        Err("Supply nonempty paths (maximum 10000)".into())
    } else {
        Ok(())
    }
}
pub fn patch(params: DevelopParams, patch: &Value) -> Result<DevelopParams, String> {
    let mut value = serde_json::to_value(params).map_err(|e| e.to_string())?;
    let object = patch
        .as_object()
        .filter(|o| !o.is_empty())
        .ok_or("settings must be a nonempty object")?;
    for (key, next) in object {
        if value.get(key).is_none() {
            return Err(format!("Unknown RAW setting '{key}'"));
        }
        value[key] = next.clone();
    }
    let result: DevelopParams = serde_json::from_value(value).map_err(|e| e.to_string())?;
    result.validate().map_err(|e| e.to_string())?;
    Ok(result)
}

pub fn definitions() -> Vec<ToolDef> {
    fn def(name: &str, description: &str, properties: Value, required: &[&str]) -> ToolDef {
        ToolDef {
            name: name.into(),
            description: description.into(),
            input_schema: json!({"type":"object","additionalProperties":false,"properties":properties,"required":required}),
        }
    }
    let paths = json!({"type":"array","maxItems":10000,"items":{"type":"string","minLength":1},"description":"Canonical absolute file paths returned by get_library"});
    let label = json!({"type":"integer","minimum":0,"maximum":5});
    let group = json!({"type":"string","enum":["all","white_balance","tone","curve"]});
    let settings = crate::raw_tools::definitions()
        .into_iter()
        .find(|d| d.name == "develop_raw")
        .unwrap()
        .input_schema["properties"]["settings"]
        .clone();
    vec![
        def(
            "get_library",
            "Inspect the live Library: paginated visible files, selection, metadata, collections, filters, Develop settings, histogram, dirty/save/export state. No mutations. Use canonical paths for subsequent calls.",
            json!({"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":200,"default":100}}),
            &[],
        ),
        def(
            "get_library_preview",
            "Return the active Library photo as a PNG, including current Develop draft and optional recipe. Before mode shows as-shot; compare mode returns before and after images. Does not save or change selection.",
            json!({}),
            &[],
        ),
        def(
            "import_library",
            "Import supported images in one local folder (nonrecursive) into the catalog and display them. Originals remain in place. Waits for catalog persistence.",
            json!({"folder":{"type":"string","minLength":1},"deduplicate":{"type":"boolean","default":false}}),
            &["folder"],
        ),
        def(
            "set_library_view",
            "Patch Library search, filters, sorting, view, inspector or optional film recipe. source=all clears the collection; collection selects a catalog collection. Empty recipe clears it. Changes are reflected in the desktop Library.",
            json!({"query":{"type":"string"},"collapse_stacks":{"type":"boolean"},"develop_section":{"type":"string","enum":["basic","crop","curve","mixer","grading","masks","kelvin","history","enhance"]},"source":{"type":"string","enum":["all","folder"]},"collection":{"type":"integer","minimum":1},"minimum_rating":label,"flag":{"type":"string","enum":["all","picked","rejected"]},"color_label":label,"raw_only":{"type":"boolean"},"clipping":{"type":"boolean"},"unedited":{"type":"boolean"},"sort":{"type":"string","enum":["filename","capture_time"]},"reverse":{"type":"boolean"},"mode":{"type":"string","enum":["grid","list","develop","before","compare"]},"inspector":{"type":"string","enum":["develop","info","keywords"]},"recipe":{"type":"string"}}),
            &[],
        ),
        def(
            "select_library_photos",
            "Replace Library selection with visible canonical paths. Empty paths clears it. active must be selected; defaults to first selected. Select another active path to navigate the filmstrip.",
            json!({"paths":paths,"active":{"type":"string"}}),
            &["paths"],
        ),
        def(
            "edit_library_metadata",
            "Persist ratings, pick/unflag/reject, color labels (0 none,1 red,2 yellow,3 green,4 blue,5 purple), and keywords for explicit Library paths. Keywords replace by default or append when append_keywords=true. Pick and reject are mutually exclusive.",
            json!({"paths":paths,"rating":label,"flag":{"type":"string","enum":["picked","rejected","unflagged"]},"color_label":label,"keywords":{"type":"array","items":{"type":"string"}},"append_keywords":{"type":"boolean"}}),
            &["paths"],
        ),
        def(
            "library_collection",
            "Create a named collection (optionally empty) or add explicit Library paths to a collection ID. Catalog writes are atomic; source files are unchanged.",
            json!({"action":{"type":"string","enum":["create","add"]},"name":{"type":"string"},"id":{"type":"integer","minimum":1},"paths":paths}),
            &["action"],
        ),
        def(
            "library_catalog",
            "Photo catalog operations: smart_collection creates a live rule; stack/unstack use canonical paths; virtual_copy creates an independent recipe reference; backup/restore_backup use .emulibrary for a portable archive including photo originals, sidecars, presets and masks, or .json for catalog references; relink verifies identical original bytes and rewrites virtual references with their histories; import_lightroom reads .lrcat references, ratings, collections and supported readable histories, or an Adobe companion handoff JSON; import_presets installs XMP/lrtemplate/JSON/ZIP preset packs with compatibility warnings; save_export_preset/load_export_preset use a JSON path for the visible output settings.",
            json!({"action":{"type":"string","enum":["smart_collection","stack","unstack","virtual_copy","backup","relink","import_lightroom","restore_backup","import_presets","save_export_preset","load_export_preset"]},"paths":paths,"name":{"type":"string"},"path":{"type":"string"},"rule":{"type":"object","additionalProperties":false,"properties":{"minimum_rating":label,"color_label":label,"flagged":{"type":"boolean"},"rejected":{"type":"boolean"},"raw_only":{"type":"boolean"},"keyword":{"type":"string","maxLength":200}}}}),
            &["action"],
        ),
        def(
            "develop_library",
            "Operate on the active Library photo using the same drafts, undo and sidecars as the UI. adjust patches settings; auto/reset/as_shot/undo/preset/load_preset save immediately and report failures. sync copies group to selected photos. save flushes all drafts. reload explicitly discards active unsaved draft. Built-in preset names: neutral,warm,black_and_white,strong_contrast. Imports Emulsion JSON, Lightroom XMP and legacy lrtemplate presets with a compatibility report. snapshot/restore_snapshot use a name; match_lens resolves a measured Lensfun profile; subject_mask/sky_mask create local bitmap masks (sky_mask requires point); auto_sky uses local semantic sky segmentation; auto_perspective estimates level and perspective from image lines; denoise/super_resolution create new rendered derivatives. sensor_noise_reduction is a RAW-only pre-demosaic control in settings. point_curves contains composite/red/green/blue control points. Sampled camera WB cannot be synced across Library files. RAW highlights is recovery: positive darkens, negative brightens; UI slider uses the opposite sign.",
            json!({"action":{"type":"string","enum":["adjust","auto","reset","as_shot","undo","reload","save","sync","preset","save_preset","load_preset","snapshot","restore_snapshot","subject_mask","sky_mask","auto_sky","auto_perspective","denoise","super_resolution","match_lens"]},"point":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"number","minimum":0,"maximum":1},"description":"Normalized point in the untransformed source sky"},"name":{"type":"string","minLength":1,"maxLength":200},"settings":settings,"preset":{"type":"string","enum":["neutral","warm","black_and_white","strong_contrast"]},"path":{"type":"string","minLength":1},"group":group}),
            &["action"],
        ),
        def(
            "export_library",
            "Export selected Library photos using saved edits and the selected recipe to jpg/png/tif/webp. settings controls resizing, JPEG quality, 8/16-bit output, sharpening and an image watermark. Rejects dirty drafts. Waits for completion and reports partial failures. Existing files and originals are preserved. Source metadata is omitted by default; metadata selects copyright, camera or camera_and_location retention. Serial numbers and maker notes are always excluded. Optional publish sends versioned exports to WebDAV without replacing existing remote files.",
            json!({"out_dir":{"type":"string","minLength":1},"format":{"type":"string","enum":["jpg","png","tif","webp"]},"settings":{"type":"object","additionalProperties":false,"properties":{"publish":{"type":["object","null"],"additionalProperties":false,"required":["url"],"properties":{"url":{"type":"string"},"authorization_env":{"type":["string","null"]}}},"long_edge":{"type":"integer","minimum":0},"metadata":{"type":"string","enum":["none","copyright","camera","camera_and_location"]},"jpeg_quality":{"type":"integer","minimum":1,"maximum":100},"depth":{"type":"integer","enum":[0,8,16]},"sharpening":{"type":"number","minimum":0,"maximum":1},"watermark":{"type":["string","null"]},"watermark_opacity":{"type":"number","minimum":0,"maximum":1},"watermark_width":{"type":"number","minimum":0.01,"maximum":1}}}}),
            &["out_dir", "format"],
        ),
        def(
            "cancel_library_enhancement",
            "Cancel active local model inference; already completed derivative files are preserved.",
            json!({}),
            &[],
        ),
        def(
            "cancel_library_export",
            "Stop the Library export queue. An already encoding image may still finish; completed outputs are preserved. Inspect get_library for progress.",
            json!({}),
            &[],
        ),
        def(
            "open_library_photo",
            "Open the active saved Library image in Photo. Rejects unsaved RAW drafts. Returns the opened document ID; RAW Photo tools operate through that document's assistant relay.",
            json!({}),
            &[],
        ),
    ]
}

/// Encode a rendered Library snapshot using MCP's standard image content shape.
pub fn png_content(width: u32, height: u32, rgba: &[u8]) -> Result<Value, String> {
    use base64::Engine as _;
    let png = emulsion_io::export::png8(width, height, rgba).map_err(|e| e.to_string())?;
    Ok(
        json!({"type":"image","mimeType":"image/png","data":base64::engine::general_purpose::STANDARD.encode(png)}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schemas_and_read_only_registration_cover_library_tools() {
        let defs = definitions();
        assert_eq!(defs.len(), NAMES.len());
        for name in NAMES {
            assert!(defs.iter().any(|d| d.name == *name));
        }
        for name in ["get_library", "get_library_preview"] {
            assert!(crate::tools::READ_ONLY.contains(&name));
        }
    }
    #[test]
    fn schemas_expose_every_persisted_raw_setting() {
        let raw = crate::raw_tools::definitions()
            .into_iter()
            .find(|d| d.name == "develop_raw")
            .unwrap();
        let library = definitions()
            .into_iter()
            .find(|d| d.name == "develop_library")
            .unwrap();
        let values = serde_json::to_value(DevelopParams::default()).unwrap();
        for key in values.as_object().unwrap().keys() {
            assert!(
                raw.input_schema["properties"]["settings"]["properties"]
                    .get(key)
                    .is_some(),
                "Photo schema missing {key}"
            );
            assert!(
                library.input_schema["properties"]["settings"]["properties"]
                    .get(key)
                    .is_some(),
                "Library schema missing {key}"
            );
        }
    }
    #[test]
    fn strict_requests_reject_silent_noops_and_invalid_metadata() {
        for (name, args) in [
            ("edit_library_metadata", json!({"paths":["a"],"rating":6})),
            ("edit_library_metadata", json!({"paths":["a"]})),
            ("set_library_view", json!({"flag":"unflagged"})),
            ("develop_library", json!({"action":"save","settings":{}})),
            (
                "develop_library",
                json!({"action":"adjust","settings":{"typo":1}}),
            ),
            ("get_library", json!({"limit":0})),
            ("get_library_preview", json!({"path":"x"})),
        ] {
            assert!(parse(name, &args).is_err(), "{name} {args}");
        }
        assert!(
            parse(
                "develop_library",
                &json!({"action":"adjust","settings":{"whites":0.3,"noise_reduction":0.2}})
            )
            .is_ok()
        );
        assert!(
            parse(
                "library_collection",
                &json!({"action":"create","name":"Empty"})
            )
            .is_ok()
        );
    }
    #[test]
    fn expanded_develop_and_output_controls_are_reachable_through_mcp() {
        for action in ["auto_sky", "auto_perspective"] {
            assert!(parse("develop_library", &json!({"action":action})).is_ok());
            assert!(
                parse(
                    "develop_library",
                    &json!({"action":action,"point":[0.5,0.5]})
                )
                .is_err()
            );
        }
        let params = patch(
            DevelopParams::default(),
            &json!({
                "point_curves":[[],[[0.,1.],[0.4,0.3],[1.,0.]],[],[]],
                "sensor_noise_reduction":0.5
            }),
        )
        .unwrap();
        assert_eq!(params.point_curves[1].len, 3);
        assert_eq!(params.sensor_noise_reduction, 0.5);
        assert!(
            patch(
                params,
                &json!({"point_curves":[[],[[0.8,0.],[0.2,1.]],[],[]]})
            )
            .is_err()
        );
        let def = definitions()
            .into_iter()
            .find(|d| d.name == "export_library")
            .unwrap();
        let settings =
            serde_json::to_value(emulsion_io::photo_export::OutputSettings::default()).unwrap();
        for key in settings.as_object().unwrap().keys() {
            assert!(
                def.input_schema["properties"]["settings"]["properties"]
                    .get(key)
                    .is_some(),
                "Output schema missing {key}"
            );
        }
        assert!(parse("export_library", &json!({"out_dir":"out","format":"tif","settings":{"metadata":"camera","publish":{"url":"https://example.com/photos","authorization_env":"EMULSION_DAV_AUTH"}}})).is_ok());
    }
    #[test]
    fn partial_settings_preserve_existing_values_and_validate_all_controls() {
        let old = DevelopParams {
            exposure: 1.2,
            temperature: 0.3,
            ..Default::default()
        };
        let next = patch(old, &json!({"clarity":0.2,"vignette":-0.4})).unwrap();
        assert_eq!(next.exposure, old.exposure);
        assert_eq!(next.temperature, old.temperature);
        assert!(patch(old, &json!({"exposure":6})).is_err());
    }
}
